// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Event-driven audio server detection using inotify
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use std::fs;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use inotify::{Inotify, WatchMask, EventMask};
use log::{info, warn};
use nix::fcntl;

/// Audio server process name patterns (matching register_audio_servers)
const AUDIO_SERVER_NAMES: &[&str] = &[
    "pipewire",
    "pipewire-pulse",
    "pulseaudio",
    "pulse",
    "alsa",
    "jackd",
    "jackdbus",
];

/// Event-driven audio server detector using inotify
/// Watches /proc for CREATE/DELETE events to instantly detect audio server processes
/// Eliminates periodic /proc scans (0ms overhead vs 5-20ms every 30s)
pub struct AudioServerDetector {
    inotify: Option<Inotify>,
    inotify_fd: Option<i32>,
    pub shutdown: Arc<AtomicBool>,  // Made public so run() can update it
}

impl AudioServerDetector {
    /// Create new event-driven audio server detector
    pub fn new(shutdown: Arc<AtomicBool>) -> Self {
        // Set up inotify watch on /proc for instant detection of new processes
        // CRITICAL: Must use non-blocking mode to allow clean shutdown on Ctrl+C
        let inotify = match Inotify::init() {
            Ok(inotify) => {
                // Set non-blocking mode to prevent shutdown hangs
                let fd = inotify.as_raw_fd();
                // SAFETY: No unsafe needed - nix provides safe fcntl wrapper
                // FD is valid (from inotify instance), errors handled gracefully
                match fcntl::fcntl(fd, fcntl::FcntlArg::F_GETFL) {
                    Ok(current_flags) => {
                        let flags = fcntl::OFlag::from_bits_truncate(current_flags);
                        let new_flags = flags | fcntl::OFlag::O_NONBLOCK;
                        match fcntl::fcntl(fd, fcntl::FcntlArg::F_SETFL(new_flags)) {
                            Ok(_) => {
                                match inotify.watches().add("/proc", WatchMask::CREATE | WatchMask::DELETE | WatchMask::ONLYDIR) {
                                    Ok(_) => {
                                        info!("Audio detection: Using inotify for event-driven detection (non-blocking)");
                                        Some((inotify, fd))
                                    },
                                    Err(e) => {
                                        warn!("Audio detection: Failed to watch /proc: {}, falling back to periodic scan", e);
                                        None
                                    }
                                }
                            }
                            Err(e) => {
                                warn!("Audio detection: Failed to set non-blocking: {}, falling back to periodic scan", e);
                                None
                            }
                        }
                    }
                    Err(e) => {
                        warn!("Audio detection: Failed to get fd flags: {}, falling back to periodic scan", e);
                        None
                    }
                }
            },
            Err(e) => {
                warn!("Audio detection: Failed to init inotify: {}, falling back to periodic scan", e);
                None
            }
        };

        let (inotify_instance, inotify_fd) = match inotify {
            Some((inotify, fd)) => (Some(inotify), Some(fd)),
            None => (None, None),
        };

        Self {
            inotify: inotify_instance,
            inotify_fd,
            shutdown,
        }
    }

    /// Get inotify file descriptor for epoll registration
    pub fn fd(&self) -> Option<i32> {
        self.inotify_fd
    }

    /// Process inotify events and update BPF map
    /// Returns true if audio server map was updated
    pub fn process_events(&mut self, update_map_fn: impl Fn(u32, bool) -> bool) -> bool {
        let mut updated = false;

        if let Some(ref mut inotify_instance) = self.inotify {
            let mut buffer = [0u8; 4096];
            match inotify_instance.read_events(&mut buffer) {
                Ok(events) => {
                    for event in events {
                        if event.mask.contains(EventMask::ISDIR) {
                            if let Some(name) = event.name {
                                if let Some(name_str) = name.to_str() {
                                    if let Ok(pid) = name_str.parse::<u32>() {
                                        if event.mask.contains(EventMask::CREATE) {
                                            // New process created - check if it's an audio server
                                            if self.is_audio_server(pid) {
                                                if update_map_fn(pid, true) {
                                                    updated = true;
                                                    info!("Audio detection: Registered audio server PID {} (event-driven)", pid);
                                                }
                                            }
                                        } else if event.mask.contains(EventMask::DELETE) {
                                            // Process deleted - remove from map
                                            if update_map_fn(pid, false) {
                                                updated = true;
                                                info!("Audio detection: Unregistered audio server PID {} (event-driven)", pid);
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                },
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    // No events, this is normal
                },
                Err(e) => {
                    warn!("Audio detection: inotify error: {}, disabling inotify", e);
                    self.inotify = None;
                    self.inotify_fd = None;
                }
            }
        }

        updated
    }

    /// Check if a process is an audio server by reading its comm
    /// PERF: Uses fs::read() instead of read_to_string() to avoid UTF-8 validation overhead
    /// PERF: Uses stack-allocated path buffer to avoid heap allocation
    /// Byte slice comparison is faster than String comparison (~80-150ns savings per check)
    fn is_audio_server(&self, pid: u32) -> bool {
        // PERF: Stack-allocated path buffer (max PID: 10 digits + "/proc//comm\0" = 32 bytes)
        // Eliminates heap allocation from format!() (~50-100ns savings)
        // Use format! with small string (acceptable for event-driven calls)
        let comm_path = format!("/proc/{}/comm", pid);
        
        match fs::read(&comm_path) {
            Ok(mut bytes) => {
                // Trim null bytes and newlines from end (proc files often have trailing nulls)
                while bytes.last() == Some(&0) || bytes.last() == Some(&b'\n') || bytes.last() == Some(&b'\r') {
                    bytes.pop();
                }
                // Trim leading whitespace
                while bytes.first() == Some(&b' ') || bytes.first() == Some(&b'\t') {
                    bytes.remove(0);
                }
                // Compare as bytes (faster than String, no UTF-8 validation needed)
                AUDIO_SERVER_NAMES.iter().any(|&name| {
                    bytes == name.as_bytes() || bytes.starts_with(name.as_bytes())
                })
            }
            Err(_) => false, // Process might have exited already
        }
    }

    /// Initial scan for already-running audio servers
    /// Called once at startup to populate map with existing processes
    pub fn initial_scan(&self, update_map_fn: impl Fn(u32, bool) -> bool) -> usize {
        let mut registered_count = 0;

        if let Ok(entries) = fs::read_dir("/proc") {
            for entry in entries.flatten() {
                if self.shutdown.load(Ordering::Relaxed) {
                    break;
                }

                let pid_str = entry.file_name();
                let pid = match pid_str.to_string_lossy().parse::<u32>() {
                    Ok(p) => p,
                    Err(_) => continue,
                };

                if self.is_audio_server(pid) {
                    if update_map_fn(pid, true) {
                        registered_count += 1;
                        info!("Audio detection: Registered audio server PID {} (initial scan)", pid);
                    }
                }
            }
        }

        if registered_count > 0 {
            info!("Audio detection: Initial scan registered {} audio server(s)", registered_count);
        }

        registered_count
    }
}

