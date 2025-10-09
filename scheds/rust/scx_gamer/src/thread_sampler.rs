// SPDX-License-Identifier: GPL-2.0
//
// Thread Runtime Sampling
// Copyright (c) 2025 RitzDaCat
//
// Collects runtime statistics from /proc to identify thread roles.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Raw thread statistics from /proc/{pid}/task/{tid}/stat
#[derive(Debug, Clone, Default)]
pub struct ThreadStats {
    pub tid: u32,
    pub comm: String,
    pub utime: u64,      // CPU time in user mode (clock ticks)
    pub stime: u64,      // CPU time in kernel mode (clock ticks)
    pub num_threads: u32,
    pub vsize: u64,      // Virtual memory size
}

impl ThreadStats {
    /// Parse /proc/{pid}/task/{tid}/stat
    pub fn from_proc(pid: u32, tid: u32) -> Result<Self> {
        let path = format!("/proc/{}/task/{}/stat", pid, tid);
        let content = fs::read_to_string(&path)
            .with_context(|| format!("Failed to read {}", path))?;

        Self::parse_stat(&content, tid)
    }

    /// Parse stat file content
    /// Format: pid (comm) state ppid pgrp session ... utime stime ...
    fn parse_stat(content: &str, tid: u32) -> Result<Self> {
        // Find comm (enclosed in parentheses, may contain spaces)
        let comm_start = content.find('(').context("Invalid stat format")?;
        let comm_end = content.rfind(')').context("Invalid stat format")?;
        let comm = content[comm_start + 1..comm_end].to_string();

        // Parse fields after comm
        let fields: Vec<&str> = content[comm_end + 2..].split_whitespace().collect();

        if fields.len() < 13 {
            anyhow::bail!("Insufficient fields in stat file");
        }

        // Field indices (0-based after comm):
        // 0=state 1=ppid 2=pgrp ... 11=utime 12=stime 13=cutime 14=cstime ... 17=num_threads 20=vsize
        Ok(Self {
            tid,
            comm,
            utime: fields.get(11).and_then(|s| s.parse().ok()).unwrap_or(0),
            stime: fields.get(12).and_then(|s| s.parse().ok()).unwrap_or(0),
            num_threads: fields.get(17).and_then(|s| s.parse().ok()).unwrap_or(0),
            vsize: fields.get(20).and_then(|s| s.parse().ok()).unwrap_or(0),
        })
    }

    /// Get total CPU time (utime + stime)
    pub fn total_time(&self) -> u64 {
        self.utime + self.stime
    }
}

/// Thread sampler for a process
pub struct ThreadSampler {
    pid: u32,
    samples: HashMap<u32, Vec<ThreadStats>>,  // tid -> [samples]
    sample_interval: std::time::Duration,
    last_sample: std::time::Instant,
}

impl ThreadSampler {
    pub fn new(pid: u32, sample_interval: std::time::Duration) -> Self {
        Self {
            pid,
            samples: HashMap::new(),
            sample_interval,
            last_sample: std::time::Instant::now(),
        }
    }

    /// Sample all threads of the process
    pub fn sample(&mut self) -> Result<()> {
        let now = std::time::Instant::now();
        if now.duration_since(self.last_sample) < self.sample_interval {
            return Ok(());  // Not time to sample yet
        }
        self.last_sample = now;

        let task_dir = PathBuf::from(format!("/proc/{}/task", self.pid));
        if !task_dir.exists() {
            anyhow::bail!("Process {} not found", self.pid);
        }

        for entry in fs::read_dir(&task_dir)? {
            let entry = entry?;
            if let Ok(tid) = entry.file_name().to_str().unwrap_or("").parse::<u32>() {
                if let Ok(stats) = ThreadStats::from_proc(self.pid, tid) {
                    self.samples.entry(tid).or_insert_with(Vec::new).push(stats);
                }
            }
        }

        Ok(())
    }

    /// Get average CPU time percentage for a thread
    pub fn get_cpu_time_pct(&self, tid: u32) -> f64 {
        if let Some(samples) = self.samples.get(&tid) {
            if samples.len() < 2 {
                return 0.0;
            }

            let first = &samples[0];
            let last = &samples[samples.len() - 1];

            let delta_time = last.total_time().saturating_sub(first.total_time());
            let total_delta: u64 = samples.iter()
                .map(|s| s.total_time())
                .sum::<u64>()
                .saturating_sub(first.total_time() * samples.len() as u64);

            if total_delta == 0 {
                return 0.0;
            }

            (delta_time as f64 / total_delta as f64) * 100.0
        } else {
            0.0
        }
    }

    /// Get average wakeup frequency (estimated from sample count)
    pub fn get_wakeup_freq(&self, tid: u32) -> u64 {
        if let Some(samples) = self.samples.get(&tid) {
            if samples.len() < 2 {
                return 0;
            }

            // Estimate: samples per second based on sample interval
            let duration_secs = self.sample_interval.as_secs_f64() * samples.len() as f64;
            if duration_secs > 0.0 {
                (samples.len() as f64 / duration_secs) as u64
            } else {
                0
            }
        } else {
            0
        }
    }

    /// Get thread by TID
    pub fn get_thread(&self, tid: u32) -> Option<&Vec<ThreadStats>> {
        self.samples.get(&tid)
    }

    /// Get all threads
    pub fn get_all_threads(&self) -> Vec<u32> {
        self.samples.keys().copied().collect()
    }

    /// Get sample count for thread
    pub fn get_sample_count(&self, tid: u32) -> u32 {
        self.samples.get(&tid).map(|v| v.len() as u32).unwrap_or(0)
    }

    /// Clear samples (for new sampling session)
    pub fn clear(&mut self) {
        self.samples.clear();
        self.last_sample = std::time::Instant::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_stat() {
        let stat = "1234 (test process) S 1 1234 1234 0 -1 4194304 123 0 0 0 10 5 0 0 20 0 4 0 12345 1024000 100 18446744073709551615";
        let stats = ThreadStats::parse_stat(stat, 1234).unwrap();

        assert_eq!(stats.tid, 1234);
        assert_eq!(stats.comm, "test process");
        assert_eq!(stats.utime, 10);
        assert_eq!(stats.stime, 5);
        assert_eq!(stats.total_time(), 15);
    }

    #[test]
    fn test_thread_sampler_creation() {
        let sampler = ThreadSampler::new(1234, std::time::Duration::from_secs(1));
        assert_eq!(sampler.pid, 1234);
        assert_eq!(sampler.samples.len(), 0);
    }
}
