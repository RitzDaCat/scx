// SPDX-License-Identifier: GPL-2.0
//
// GPU queue depth monitor using drm gpu_busy_percent sysfs metrics.
// Provides coarse queue busy signal so TaskGraph borrowing can react instantly
// when the GPU pipeline drains.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use log::info;

const SAMPLE_INTERVAL: Duration = Duration::from_millis(30);

pub struct GpuQueueMonitor {
    busy_path: PathBuf,
    last_sample: Instant,
}

impl GpuQueueMonitor {
    pub fn new() -> Option<Self> {
        let busy_path = discover_busy_path()?;
        info!("GPU queue monitor: using {}", busy_path.display());
        Some(Self {
            busy_path,
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

        let value = fs::read_to_string(&self.busy_path).ok()?;
        let busy = value.trim().parse::<u32>().ok()?;
        Some(busy.min(100))
    }

    pub fn guard_ns(&self, busy_percent: u32) -> u64 {
        match busy_percent {
            80..=u32::MAX => 8_000_000,   // 8ms for heavy GPU workloads
            50..=79 => 5_000_000,          // 5ms for moderate load
            20..=49 => 3_000_000,          // 3ms for light load
            _ => 0,                        // Treat as idle
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

