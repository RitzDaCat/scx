// SPDX-License-Identifier: GPL-2.0
//
// GPU queue depth monitor using drm gpu_busy_percent sysfs metrics.
// Provides coarse queue busy signal so TaskGraph borrowing can react instantly
// when the GPU pipeline drains.

use std::fs::{self, File};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::info;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(30);

pub struct GpuQueueMonitor {
    busy_file: File,
    last_sample: Instant,
}

impl GpuQueueMonitor {
    pub fn new() -> Option<Self> {
        let busy_path = discover_busy_path()?;
        let busy_file = File::open(&busy_path).ok()?;
        info!("GPU queue monitor: using {}", busy_path.display());
        Some(Self {
            busy_file,
            last_sample: Instant::now() - SAMPLE_INTERVAL,
        })
    }

    /// Returns busy percentage (0-100) when a new sample is available.
    pub fn poll(&mut self) -> Option<u32> {
        let now = Instant::now();
        if now.duration_since(self.last_sample) < SAMPLE_INTERVAL {
            return None;
        }
        self.last_sample = now;

        self.busy_file.seek(SeekFrom::Start(0)).ok()?;
        let mut buf = [0u8; 32];
        let read_len = self.busy_file.read(&mut buf).ok()?;
        if read_len == 0 {
            return None;
        }
        let value_str = std::str::from_utf8(&buf[..read_len]).ok()?.trim();
        let busy = value_str.parse::<u32>().ok()?;
        Some(busy.min(100))
    }

    pub fn guard_ns(busy_percent: u32) -> u64 {
        match busy_percent {
            80..=u32::MAX => 8_000_000, // 8ms for heavy GPU workloads
            50..=79 => 5_000_000,       // 5ms for moderate load
            20..=49 => 3_000_000,       // 3ms for light load
            _ => 0,                     // Treat as idle
        }
    }
}

fn discover_busy_path() -> Option<PathBuf> {
    let drm_root = Path::new("/sys/class/drm");
    let entries = fs::read_dir(drm_root).ok()?;

    let mut candidates = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let fname = entry.file_name().to_string_lossy().to_lowercase();
        if !fname.starts_with("card") {
            continue;
        }
        let device_dir = path.join("device");
        let busy_file = device_dir.join("gpu_busy_percent");
        if busy_file.exists() {
            candidates.push(busy_file);
        }
    }

    candidates.into_iter().next()
}

pub fn monotonic_nanos() -> u64 {
    unsafe {
        let mut ts: libc::timespec = std::mem::zeroed();
        if libc::clock_gettime(libc::CLOCK_MONOTONIC, &mut ts) == 0 {
            (ts.tv_sec as u64)
                .saturating_mul(1_000_000_000)
                .saturating_add(ts.tv_nsec as u64)
        } else {
            0
        }
    }
}
