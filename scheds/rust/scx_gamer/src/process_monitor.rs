// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Per-Process Resource Monitoring
// Copyright (c) 2025 RitzDaCat
//
// Tracks CPU and GPU usage for specific processes (game, OBS, etc.)

use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct ProcessStats {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: f64,
    pub gpu_percent: f64,
    pub threads: usize,
    pub memory_mb: u64,
}

#[derive(Debug, Clone)]
struct ProcStatSnapshot {
    utime: u64,      // User time
    stime: u64,      // System time
    timestamp: Instant,
}

pub struct ProcessMonitor {
    last_snapshots: HashMap<u32, ProcStatSnapshot>,
    system_hz: u64,  // System clock ticks per second
}

impl ProcessMonitor {
    pub fn new() -> Result<Self> {
        // Get system clock ticks per second
        let system_hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as u64;

        Ok(Self {
            last_snapshots: HashMap::new(),
            system_hz,
        })
    }

    /// Get CPU usage for a specific process
    /// Returns % of a single CPU core (0-100 per core, can exceed 100 on multi-core)
    pub fn get_process_stats(&mut self, pid: u32) -> Option<ProcessStats> {
        // Read /proc/[pid]/stat
        let stat_path = format!("/proc/{}/stat", pid);
        let stat_content = fs::read_to_string(&stat_path).ok()?;

        // Parse stat file (format: pid (comm) state ppid ... utime stime ...)
        // Fields: https://man7.org/linux/man-pages/man5/proc.5.html
        // We need: utime (field 14), stime (field 15)
        let parts: Vec<&str> = stat_content.split_whitespace().collect();
        if parts.len() < 52 {
            return None;
        }

        let utime = parts[13].parse::<u64>().ok()?;
        let stime = parts[14].parse::<u64>().ok()?;
        let num_threads = parts[19].parse::<usize>().ok()?;
        let vsize_bytes = parts[22].parse::<u64>().ok()?;

        let now = Instant::now();

        // Calculate CPU% since last sample
        let cpu_percent = if let Some(prev) = self.last_snapshots.get(&pid) {
            let delta_time = now.duration_since(prev.timestamp).as_secs_f64();
            let delta_utime = utime.saturating_sub(prev.utime);
            let delta_stime = stime.saturating_sub(prev.stime);
            let delta_total = delta_utime + delta_stime;

            if delta_time > 0.0 {
                // Convert ticks to CPU%
                // delta_total is in clock ticks, divide by system_hz to get seconds
                // Then divide by delta_time and multiply by 100 for percentage
                ((delta_total as f64) / (self.system_hz as f64)) / delta_time * 100.0
            } else {
                0.0
            }
        } else {
            0.0  // First sample, no delta available
        };

        // Store current snapshot for next calculation
        self.last_snapshots.insert(pid, ProcStatSnapshot {
            utime,
            stime,
            timestamp: now,
        });

        // Get process name from /proc/[pid]/comm
        let name = fs::read_to_string(format!("/proc/{}/comm", pid))
            .ok()
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|| "unknown".to_string());

        // Get GPU usage (NVIDIA only for now)
        let gpu_percent = get_gpu_usage_nvidia(pid).unwrap_or(0.0);

        Some(ProcessStats {
            pid,
            name,
            cpu_percent,
            gpu_percent,
            threads: num_threads,
            memory_mb: vsize_bytes / (1024 * 1024),
        })
    }

    /// Get stats for multiple processes
    #[allow(dead_code)]
    pub fn get_multi_process_stats(&mut self, pids: &[u32]) -> Vec<ProcessStats> {
        pids.iter()
            .filter_map(|&pid| self.get_process_stats(pid))
            .collect()
    }
}

/// Get GPU usage for a specific process (NVIDIA only)
/// Returns GPU utilization % (0-100)
fn get_gpu_usage_nvidia(pid: u32) -> Option<f64> {
    // Run nvidia-smi to get per-process GPU usage
    // Format: nvidia-smi --query-compute-apps=pid,used_memory --format=csv,noheader,nounits
    let output = std::process::Command::new("nvidia-smi")
        .args(&[
            "--query-compute-apps=pid,used_memory",
            "--format=csv,noheader,nounits"
        ])
        .output()
        .ok()?;

    let stdout = String::from_utf8_lossy(&output.stdout);

    // Parse output to find our PID
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(',').collect();
        if parts.len() >= 2 {
            if let Ok(proc_pid) = parts[0].trim().parse::<u32>() {
                if proc_pid == pid {
                    // Found our process
                    // Note: nvidia-smi doesn't give us GPU%, only memory usage
                    // We'd need to correlate with total GPU usage
                    // For now, return a rough estimate based on memory usage
                    let mem_mb = parts[1].trim().parse::<f64>().ok()?;
                    // Rough heuristic: assume proportional to VRAM usage
                    // This is imprecise but gives a ballpark
                    return Some(mem_mb / 100.0);  // Very rough estimate
                }
            }
        }
    }

    None
}

/// Find OBS process PID by name
pub fn find_obs_pid() -> Option<u32> {
    // Read /proc to find obs process
    let proc_dir = fs::read_dir("/proc").ok()?;

    for entry in proc_dir.flatten() {
        if let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() {
            if let Ok(comm) = fs::read_to_string(format!("/proc/{}/comm", pid)) {
                let comm_lower = comm.to_lowercase();
                if comm_lower.contains("obs") {
                    return Some(pid);
                }
            }
        }
    }

    None
}
