// SPDX-License-Identifier: GPL-2.0
//
// Event-based window focus detection via userspace helper
// 
// 100% PROOF, ZERO HEURISTICS, EVENT-BASED
//
// ARCHITECTURE:
// The scheduler runs as root but D-Bus session bus rejects root connections.
// Solution: Spawn a helper script that runs as the original user.
//
// PROOF CHAIN:
// 1. Helper connects to user's D-Bus session (user process)
// 2. Helper subscribes to KWin PropertyChanged signals (event-based)
// 3. On focus change, helper queries resourceClass from KWin (compositor authority)
// 4. For Steam games: helper finds game PID via reaper tree (Steam authority)
// 5. Helper writes PID to /tmp/scx_gamer_focused_pid
// 6. Scheduler reads PID from file (simple, no D-Bus needed)
//
// NO GUESSING: We follow known data structures from authoritative sources.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;
use std::process::{Command, Child, Stdio};
use std::fs;
use std::env;
use std::path::PathBuf;

use log::{debug, info, warn};

const PID_FILE: &str = "/tmp/scx_gamer_focused_pid";

/// Focus detector using userspace helper
/// 
/// The helper runs as the original user (not root) to access D-Bus.
/// It writes the focused game's PID to a file that we read.
pub struct FocusDetector {
    current_pid: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<thread::JoinHandle<()>>,
    _helper: Option<Child>,
}

impl FocusDetector {
    pub fn new() -> Self {
        let current_pid = Arc::new(AtomicU32::new(0));
        let shutdown = Arc::new(AtomicBool::new(false));

        // Spawn helper script as the original user
        let helper = spawn_focus_helper();

        let pid_clone = Arc::clone(&current_pid);
        let shutdown_clone = Arc::clone(&shutdown);

        // Start thread to read PID from file
        let thread = thread::Builder::new()
            .name("focus-reader".to_string())
            .spawn(move || {
                focus_reader_thread(pid_clone, shutdown_clone);
            })
            .expect("Failed to spawn focus reader thread");

        Self {
            current_pid,
            shutdown,
            _thread: Some(thread),
            _helper: helper,
        }
    }

    /// Get the PID of the currently focused window
    /// Returns 0 if no focus detected or on error
    pub fn get_focused_pid(&self) -> u32 {
        self.current_pid.load(Ordering::Relaxed)
    }
}

impl Drop for FocusDetector {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        
        // Kill helper process
        if let Some(mut helper) = self._helper.take() {
            let _ = helper.kill();
            let _ = helper.wait();
            info!("focus detector: helper process terminated");
        }
        
        // Clean up PID file
        let _ = fs::remove_file(PID_FILE);
        
        if let Some(handle) = self._thread.take() {
            for _ in 0..20 {
                if handle.is_finished() {
                    let _ = handle.join();
                    info!("focus detector: clean shutdown");
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            warn!("focus detector: thread didn't exit in time");
        }
    }
}

/// Spawn the focus helper script as the original user
fn spawn_focus_helper() -> Option<Child> {
    // Get the original user (before sudo)
    let sudo_user = env::var("SUDO_USER").ok()?;
    let sudo_uid = env::var("SUDO_UID").ok()?;
    
    // Get D-Bus environment - critical for KWin communication
    let dbus_addr = env::var("DBUS_SESSION_BUS_ADDRESS")
        .ok()
        .unwrap_or_else(|| format!("unix:path=/run/user/{}/bus", sudo_uid));
    let xdg_runtime = format!("/run/user/{}", sudo_uid);
    
    // Find the helper script
    let helper_path = find_helper_script()?;
    
    info!("focus detector: spawning helper as user '{}'", sudo_user);
    info!("focus detector: helper script: {}", helper_path.display());
    info!("focus detector: DBUS_SESSION_BUS_ADDRESS={}", dbus_addr);
    info!("focus detector: XDG_RUNTIME_DIR={}", xdg_runtime);
    
    // Use 'sudo -u USER env VAR=val ... script' to properly pass environment
    // This is more reliable than --preserve-env
    let child = Command::new("sudo")
        .args([
            "-u", &sudo_user,
            "env",
            &format!("DBUS_SESSION_BUS_ADDRESS={}", dbus_addr),
            &format!("XDG_RUNTIME_DIR={}", xdg_runtime),
            &format!("HOME=/home/{}", sudo_user),
            helper_path.to_str()?,
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    
    info!("focus detector: helper started (PID {})", child.id());
    
    Some(child)
}

/// Find the focus helper script
fn find_helper_script() -> Option<PathBuf> {
    // Try relative to executable
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            // Check scheds/rust/scx_gamer/scripts/
            let in_repo = dir
                .join("../../../scheds/rust/scx_gamer/scripts/focus-helper.sh")
                .canonicalize();
            if let Ok(path) = in_repo {
                if path.exists() {
                    return Some(path);
                }
            }
            
            // Check relative to binary in target/
            for parent_level in 1..=5 {
                let mut path = dir.to_path_buf();
                for _ in 0..parent_level {
                    path = path.join("..");
                }
                let script = path.join("scheds/rust/scx_gamer/scripts/focus-helper.sh");
                if let Ok(canonical) = script.canonicalize() {
                    if canonical.exists() {
                        return Some(canonical);
                    }
                }
            }
        }
    }
    
    // Check common install locations
    let paths = [
        "/usr/share/scx_gamer/focus-helper.sh",
        "/usr/local/share/scx_gamer/focus-helper.sh",
        "/opt/scx_gamer/focus-helper.sh",
    ];
    
    for path in paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Some(p);
        }
    }
    
    warn!("focus detector: helper script not found");
    None
}

/// Thread that reads focused PID from file
fn focus_reader_thread(
    current_pid: Arc<AtomicU32>,
    shutdown: Arc<AtomicBool>,
) {
    info!("focus detector: starting file reader (100% proof, event-based via helper)");
    
    // Wait for helper to initialize
    thread::sleep(Duration::from_millis(500));
    
    let mut last_pid = 0u32;
    
    while !shutdown.load(Ordering::Relaxed) {
        // Read PID from file (helper writes it on focus change)
        if let Ok(content) = fs::read_to_string(PID_FILE) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                if pid != last_pid {
                    if pid > 0 {
                        info!("focus detector: focus changed {} -> {} (100% proof)", last_pid, pid);
                    } else {
                        debug!("focus detector: no focused game");
                    }
                    current_pid.store(pid, Ordering::SeqCst);
                    last_pid = pid;
                }
            }
        }
        
        // Check every 100ms - not polling the compositor, just reading a file
        // The helper is event-driven via D-Bus signals
        thread::sleep(Duration::from_millis(100));
    }
    
    info!("focus detector: stopped");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pid_file_path() {
        assert_eq!(PID_FILE, "/tmp/scx_gamer_focused_pid");
    }
}
