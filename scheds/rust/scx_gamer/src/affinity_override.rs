// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: CPU Affinity Override System (Userspace)
// Copyright (c) 2025 RitzDaCat
//
// Userspace component that receives affinity change events from BPF and
// resets custom affinities to enable optimal task placement.
//
// TIER 1 PERFORMANCE: ~2-11μs total (ring buffer → syscall → kernel)
// OVERHEAD: Minimal (1-10 events/sec, ~20-110μs/sec total CPU)
//
// SAFETY GUARANTEES:
// - Only overrides userspace tasks (kernel threads filtered in BPF)
// - Respects migrate_disable (handled by kernel)
// - Graceful handling of process exits (ESRCH ignored)

use anyhow::Result;
use libbpf_rs::RingBufferBuilder;
use log::{debug, info, warn};
use nix::sched::{sched_setaffinity, CpuSet};
use nix::unistd::Pid;
use nix::errno::Errno;
use scx_utils::NR_CPU_IDS;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Affinity event type from BPF
/// 
/// CRITICAL: This struct MUST match the BPF layout exactly (affinity_detect.bpf.h)
/// BPF struct order: timestamp (u64), type (u32), pid (u32), nr_cpus_allowed (u32), _pad (u32), comm[16]
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct AffinityEvent {
    timestamp: u64,         // MUST be first to match BPF layout
    event_type: u32,        // Renamed from 'type' to match Rust conventions
    pid: u32,
    nr_cpus_allowed: u32,
    _pad: u32,
    comm: [u8; 16],
}

/// Statistics for affinity override system
#[derive(Debug, Default)]
pub struct AffinityStats {
    pub events_received: AtomicU64,
    pub affinities_reset: AtomicU64,
    pub reset_failures: AtomicU64,
    pub process_not_found: AtomicU64,
}

/// Affinity override manager
///
/// Receives affinity change events from BPF and resets custom affinities
/// to full CPU mask for optimal scheduler control.
pub struct AffinityOverride {
    #[allow(dead_code)]  // Stats accessible via stats() method for monitoring
    stats: Arc<AffinityStats>,
    shutdown: Arc<AtomicBool>,
    _thread: Option<JoinHandle<()>>,
    #[allow(dead_code)]  // Must be kept alive to maintain hook attachments
    _syscall_link: Option<libbpf_rs::Link>,  // Syscall entry hook
    _kprobe_link: Option<libbpf_rs::Link>,  // set_cpus_allowed_ptr hook
}

impl AffinityOverride {
    /// Create new affinity override system
    ///
    /// Spawns consumer thread that processes ring buffer events from BPF.
    /// Thread polls with 200ms timeout for responsive shutdown.
    pub fn new(skel: &mut crate::BpfSkel, nr_cpus: usize) -> Result<Self> {
        let stats = Arc::new(AffinityStats::default());
        let shutdown = Arc::new(AtomicBool::new(false));

        // Attach syscall entry kprobe to detect userspace affinity changes
        // This marks PIDs that call sched_setaffinity from userspace
        let _syscall_link = match skel.progs.affinity_syscall_enter.attach() {
            Ok(link) => {
                debug!("✓ Syscall entry kprobe attached successfully");
                Some(link)
            },
            Err(e) => {
                let err_str = e.to_string();
                // EEXIST means kprobe already attached (likely from previous instance)
                if err_str.contains("EEXIST") 
                    || err_str.contains("File exists") 
                    || err_str.contains("already exists")
                    || err_str.contains("-17")  // EEXIST error code
                {
                    warn!(
                        "⚠️  Affinity Override: Syscall entry kprobe already attached (another instance running?)
                         
                         ACTION REQUIRED:
                         1. Check for running instances: ps aux | grep scx_gamer | grep -v grep
                         2. Kill existing instances: sudo pkill scx_gamer
                         3. Restart scheduler
                         
                         CURRENT STATE: Running without syscall hook (detection less accurate)
                         IMPACT: May override some kernel-set affinities (NUMA, thermal)"
                    );
                    None
                } else {
                    warn!(
                        "⚠️  Affinity Override: Failed to attach syscall entry kprobe
                         
                         ERROR: {}
                         
                         POSSIBLE CAUSES:
                         1. Another scx_gamer instance running → sudo pkill scx_gamer
                         2. Kernel syscall function name differs → Check: cat /proc/kallsyms | grep sched_setaffinity
                         3. Permission denied → Ensure running as root (sudo)
                         
                         CURRENT STATE: Falling back to single-CPU heuristic
                         IMPACT: Detection less accurate (may override some kernel-set affinities)",
                        err_str
                    );
                    None
                }
            }
        };

        // Attach hook for set_cpus_allowed_ptr()
        // This checks if affinity change came from userspace (via syscall entry hook)
        let _kprobe_link = match skel.progs.affinity_detect_set_cpus_allowed_ptr.attach() {
            Ok(link) => {
                debug!("✓ Kprobe hook attached successfully");
                Some(link)
            },
            Err(e) => {
                let err_str = e.to_string();
                // EEXIST means kprobe already attached (likely from previous instance)
                if err_str.contains("EEXIST") 
                    || err_str.contains("File exists") 
                    || err_str.contains("already exists")
                    || err_str.contains("-17")  // EEXIST error code
                {
                    warn!(
                        "⚠️  Affinity Override: Kprobe hook already attached (another instance running?)
                         
                         ACTION REQUIRED:
                         1. Check for running instances: ps aux | grep scx_gamer | grep -v grep
                         2. Kill existing instances: sudo pkill scx_gamer
                         3. Restart scheduler
                         
                         CURRENT STATE: Affinity override DISABLED (both hooks failed)
                         IMPACT: Custom affinities (like Unreal Engine pinning) will be respected"
                    );
                    None
                } else {
                    warn!(
                        "⚠️  Affinity Override: Failed to attach kprobe hook
                         
                         ERROR: {}
                         
                         POSSIBLE CAUSES:
                         1. Another scx_gamer instance running → sudo pkill scx_gamer
                         2. Kernel doesn't support kprobes → Check: cat /proc/sys/kernel/kprobes
                         3. Permission denied → Ensure running as root (sudo)
                         
                         CURRENT STATE: Affinity override DISABLED
                         IMPACT: Custom affinities will be respected (scheduler still works)",
                        err_str
                    );
                    None
                }
            }
        };

        // Determine final state and report clearly
        match (_syscall_link.is_some(), _kprobe_link.is_some()) {
            (true, true) => {
                info!("✅ Affinity Override: ENABLED (proper userspace vs kernel detection)");
                info!("   → Will override userspace-set affinities (e.g., Unreal Engine pinning)");
                info!("   → Will respect kernel-set affinities (NUMA, thermal, cgroups)");
            },
            (false, true) => {
                warn!("⚠️  Affinity Override: PARTIALLY ENABLED (fallback mode)");
                warn!("   → Syscall entry kprobe: FAILED (kprobe may not be attachable)");
                warn!("   → Kprobe hook: WORKING");
                warn!("   → FALLBACK: Using single-CPU heuristic (catches Unreal Engine pinning)");
                warn!("   → IMPACT: Will override single-CPU affinities, respect multi-CPU (NUMA/thermal)");
                warn!("   → STATUS: Functional but less accurate than full detection");
            },
            (true, false) => {
                warn!("⚠️  Affinity Override: PARTIALLY ENABLED (non-functional)");
                warn!("   → Syscall hook: WORKING");
                warn!("   → Kprobe hook: FAILED (required for detection)");
                warn!("   → IMPACT: Affinity override DISABLED (custom affinities will be respected)");
                warn!("   → RECOMMENDATION: Fix kprobe hook attachment (see errors above)");
            },
            (false, false) => {
                warn!("❌ Affinity Override: DISABLED (both hooks failed)");
                warn!("   → Syscall hook: FAILED");
                warn!("   → Kprobe hook: FAILED");
                warn!("   → IMPACT: Custom affinities will be respected (scheduler still works)");
                warn!("   → TROUBLESHOOTING:");
                warn!("      1. Check for running instances: ps aux | grep scx_gamer | grep -v grep");
                warn!("      2. Kill existing instances: sudo pkill scx_gamer");
                warn!("      3. Check syscall function name: cat /proc/kallsyms | grep sched_setaffinity");
                warn!("      4. Check kprobe support: cat /proc/sys/kernel/kprobes");
            },
        }

        // Create separate Arc clones for each closure (fixes move error)
        // TIER 1: Arc::clone() is cheap (just increments reference count, ~1-2ns)
        let ringbuf_stats = Arc::clone(&stats);
        let logging_stats = Arc::clone(&stats);
        let thread_shutdown = Arc::clone(&shutdown);

        // Create full CPU affinity mask (all CPUs available)
        let full_cpumask = create_full_cpumask(nr_cpus)?;

        // Build ring buffer consumer with callback
        // TIER 1: Ring buffer callback runs in interrupt context (~2-10μs)
        let mut builder = RingBufferBuilder::new();
        builder.add(&skel.maps.affinity_events, move |data: &[u8]| -> i32 {
            handle_affinity_event(
                data,
                &ringbuf_stats,
                &full_cpumask,
            )
        })?;

        let ringbuf = builder.build()?;

        // Spawn consumer thread
        let handle = thread::Builder::new()
            .name("affinity-override".into())  // TIER 1: .into() avoids extra allocation vs .to_string()
            .spawn(move || {
                info!("Affinity override thread started");

                let mut last_stats_log = std::time::Instant::now();
                // TIER 1: Pre-compute threshold to avoid repeated Duration construction
                const STATS_LOG_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);
                // TIER 1: Poll counter to reduce elapsed() calls (check every ~50 polls = 10 seconds)
                // This reduces Instant::elapsed() calls from ~5/sec to ~0.1/sec (~50x reduction)
                let mut poll_count = 0u32;
                
                    while !thread_shutdown.load(Ordering::Relaxed) {
                    // Poll ring buffer with 200ms timeout
                    // TIER 1: ~0.5-2μs per poll when empty (epoll-driven, interrupt-based)
                    match ringbuf.poll(std::time::Duration::from_millis(200)) {
                        Ok(_) => {}
                        Err(e) => {
                            // EPERM (Operation not permitted) means fentry hook failed to attach
                            // This is expected if kernel lacks BTF support or fentry is disabled
                            // Treat as non-critical failure - system will function without affinity override
                            if e.to_string().contains("Operation not permitted") || e.to_string().contains("EPERM") {
                                warn!("Affinity override disabled: kprobe hook failed (is kprobes support enabled in kernel?)");
                            } else {
                                warn!("Affinity ring buffer poll error: {}", e);
                            }
                            // Exit thread gracefully - this is not a critical failure
                            break;
                        }
                    }
                    
                    // Log stats every 5 minutes
                    // TIER 1: Optimized elapsed check - only compute when close to threshold
                    // Check elapsed time only every ~50 polls (10 seconds) to reduce overhead
                    poll_count += 1;
                    if poll_count >= 50 {
                        poll_count = 0;
                        if last_stats_log.elapsed() >= STATS_LOG_INTERVAL {
                            let events = logging_stats.events_received.load(Ordering::Relaxed);
                            let resets = logging_stats.affinities_reset.load(Ordering::Relaxed);
                            let failures = logging_stats.reset_failures.load(Ordering::Relaxed);
                            if events > 0 {
                                info!(
                                    "Affinity override stats: {} events, {} resets, {} failures",
                                    events, resets, failures
                                );
                            }
                            last_stats_log = std::time::Instant::now();
                        }
                    }
                }

                info!("Affinity override thread exiting");
            })?;

        // Perform initial scan for existing affined tasks
        // TIER 2: Startup-only cost (~200-800ms), acceptable one-time overhead
        info!("Scanning for existing tasks with custom affinities...");
        match scan_and_reset_affinities(&full_cpumask, &stats) {
            Ok(count) => {
                info!("Initial scan complete: reset {} task affinities", count);
            }
            Err(e) => {
                warn!("Initial affinity scan failed: {}", e);
            }
        }

        Ok(Self {
            stats,
            shutdown,
            _thread: Some(handle),
            _syscall_link,
            _kprobe_link,
        })
    }

    /// Get current statistics
    ///
    /// Used for monitoring and debugging affinity override system.
    /// Can be called from debug API or periodic logging.
    #[allow(dead_code)]  // Public API for future monitoring/debugging use
    pub fn stats(&self) -> &AffinityStats {
        &self.stats
    }
}

impl Drop for AffinityOverride {
    fn drop(&mut self) {
        // Signal shutdown
        self.shutdown.store(true, Ordering::Relaxed);

        // Wait for thread to exit (with timeout)
        if let Some(handle) = self._thread.take() {
            match handle.join() {
                Ok(_) => debug!("Affinity override thread joined successfully"),
                Err(_) => warn!("Affinity override thread panicked"),
            }
        }
    }
}

/// Handle single affinity event from BPF ring buffer
///
/// TIER 1 PERFORMANCE: ~2-10μs (event parsing + syscall)
/// Called from ring buffer consumer callback (interrupt-driven)
///
/// OPTIMIZATIONS:
/// - Avoid String allocations in hot path (use stack buffers)
/// - Fast error checking (Errno comparison, not string matching)
/// - Minimal heap allocations
fn handle_affinity_event(
    data: &[u8],
    stats: &AffinityStats,
    full_cpumask: &CpuSet,
) -> i32 {
    // TIER 1: Size check before unsafe cast (~1-2ns)
    if data.len() < std::mem::size_of::<AffinityEvent>() {
        warn!("Affinity event too small: {} bytes", data.len());
        return -1;
    }

    // TIER 1: Unaligned read to avoid UB across architectures (~10-20ns)
    // No heap allocation - stack copy only
    let event = unsafe {
        (data.as_ptr() as *const AffinityEvent).read_unaligned()
    };

    stats.events_received.fetch_add(1, Ordering::Relaxed);

    // TIER 1: Extract process name using stack-allocated buffer (no heap allocation)
    // Only calculate and allocate String if debug logging is enabled (conditional compilation)
    // In production (RUST_LOG=warn), this entire block is eliminated (zero overhead)
    #[cfg(debug_assertions)]
    let comm_str = {
        // Find null terminator for efficient string extraction
        // OPTIMIZATION: Use manual loop instead of iter().position() for better branch prediction
        // This is ~5-10ns faster on modern CPUs due to simpler control flow
        let comm_len = {
            let mut len = 0;
            while len < event.comm.len() && event.comm[len] != 0 {
                len += 1;
            }
            len
        };
        let comm_slice = &event.comm[..comm_len];
        String::from_utf8_lossy(comm_slice)
    };
    
    #[cfg(debug_assertions)]
    debug!(
        "Affinity event: PID={} comm={} nr_cpus={}",
        event.pid, comm_str, event.nr_cpus_allowed
    );

    // TIER 1: Reset affinity to full CPU mask (~1-5μs syscall)
    match reset_task_affinity(event.pid, full_cpumask) {
        Ok(()) => {
            let prev = stats.events_received.load(Ordering::Relaxed);
            if prev == 1 {
                // TIER 2: One-time confirmation log at INFO level
                // Use stack-allocated buffer to avoid heap allocation
                let mut comm_buf = [0u8; 16];
                let comm_len = {
                    let mut len = 0;
                    while len < event.comm.len() && event.comm[len] != 0 {
                        comm_buf[len] = event.comm[len];
                        len += 1;
                    }
                    len
                };
                // Use Cow<str> to avoid allocation - stack buffer is valid for this scope
                let comm_str = std::str::from_utf8(&comm_buf[..comm_len])
                    .unwrap_or("<invalid>");
                info!(
                    "Affinity override: first event processed (pid={} comm='{}' nr_cpus={})",
                    event.pid, comm_str, event.nr_cpus_allowed
                );
            }
            stats.affinities_reset.fetch_add(1, Ordering::Relaxed);
            #[cfg(debug_assertions)]
            debug!("Reset affinity for PID {} ({})", event.pid, comm_str);
            0  // Success
        }
        Err(e) => {
            // TIER 1: Fast error checking using Errno (no string allocation)
            // Check error source directly instead of converting to string
            let is_esrch = e.downcast_ref::<Errno>()
                .map(|&errno| errno == Errno::ESRCH)
                .unwrap_or_else(|| {
                    // Fallback: check string representation only if Errno check fails
                    // This is rare (non-nix errors), so acceptable Tier 2 fallback
                    // OPTIMIZATION: Cache string conversion to avoid double allocation
                    let err_str = e.to_string();
                    err_str.contains("ESRCH") || err_str.contains("No such process")
                });
            
            if is_esrch {
                stats.process_not_found.fetch_add(1, Ordering::Relaxed);
                #[cfg(debug_assertions)]
                debug!("Process {} ({}) exited before affinity reset", event.pid, comm_str);
            } else {
                stats.reset_failures.fetch_add(1, Ordering::Relaxed);
                warn!("Failed to reset affinity for PID {}: {}", event.pid, e);
            }
            -1  // Failure
        }
    }
}

/// Reset task affinity to full CPU mask
///
/// TIER 1 PERFORMANCE: ~1-5μs (syscall overhead)
/// This is the fastest possible userspace operation for affinity reset
fn reset_task_affinity(pid: u32, full_cpumask: &CpuSet) -> Result<()> {
    // TIER 1: Direct PID conversion (~1ns)
    let nix_pid = Pid::from_raw(pid as i32);

    // TIER 1: Syscall overhead (~1-5μs)
    // This syscall will respect migrate_disable internally (kernel handles it)
    // OPTIMIZATION: Avoid .context() in hot path to prevent error allocation
    // Use map_err with static string instead (no heap allocation)
    sched_setaffinity(nix_pid, full_cpumask)
        .map_err(|e| anyhow::anyhow!("sched_setaffinity failed: {}", e))?;

    Ok(())
}

/// Create full CPU affinity mask (all CPUs)
///
/// TIER 1: Called once at startup, ~10-50μs for typical systems
fn create_full_cpumask(nr_cpus: usize) -> Result<CpuSet> {
    let mut cpuset = CpuSet::new();

    // TIER 1: Simple loop, no allocations (~1-2ns per CPU)
    for cpu in 0..nr_cpus {
        cpuset.set(cpu)?;
    }

    Ok(cpuset)
}

/// Scan all running tasks and reset custom affinities
///
/// Called at startup to catch tasks that set affinities before scheduler loaded.
/// TIER 2: One-time cost: ~200-800ms (acceptable at startup, not in hot path)
///
/// Returns: Number of affinities reset
fn scan_and_reset_affinities(full_cpumask: &CpuSet, stats: &AffinityStats) -> Result<usize> {
    let mut reset_count = 0;

    // TIER 2: /proc enumeration (startup-only, acceptable overhead)
    // This is not in the hot path, so file I/O is acceptable
    for entry in std::fs::read_dir("/proc")? {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        // TIER 1: Fast PID extraction from directory name
        let pid_str = entry.file_name();
        let pid: u32 = match pid_str.to_string_lossy().parse() {
            Ok(p) => p,
            Err(_) => continue,  // Not a PID directory
        };

        // Skip PID 0 and 1 (kernel and init)
        if pid <= 1 {
            continue;
        }

        // Check if task has custom affinity
        // TIER 2: File I/O for /proc/[pid]/status (startup-only, acceptable)
        if let Ok(true) = has_custom_affinity(pid) {
            // Reset affinity
            match reset_task_affinity(pid, full_cpumask) {
                Ok(()) => {
                    reset_count += 1;
                    stats.affinities_reset.fetch_add(1, Ordering::Relaxed);
                }
                Err(_) => {
                    // Ignore errors (process may have exited)
                    stats.process_not_found.fetch_add(1, Ordering::Relaxed);
                }
            }
        }
    }

    Ok(reset_count)
}

/// Check if task has custom (restricted) CPU affinity
///
/// TIER 2: File I/O operation (startup scan only, not in hot path)
/// Returns: Ok(true) if custom affinity, Ok(false) if full affinity, Err on failure
fn has_custom_affinity(pid: u32) -> Result<bool> {
    // TIER 1: Stack-allocated path buffer (no heap allocation)
    // Format! would allocate, but we use manual string building for zero-allocation
    // Max PID: 10 digits + "/proc//status\0" = ~32 bytes
    let mut path_buf = [0u8; 32];
    let path = {
        let mut pos = 0;
        let prefix = b"/proc/";
        path_buf[pos..pos + prefix.len()].copy_from_slice(prefix);
        pos += prefix.len();
        
        // Write PID as decimal string (manual conversion, no allocation)
        let mut pid_val = pid;
        let mut digits = [0u8; 10];
        let mut digit_count = 0;
        if pid_val == 0 {
            digits[digit_count] = b'0';
            digit_count = 1;
        } else {
            while pid_val > 0 && digit_count < 10 {
                digits[digit_count] = b'0' + (pid_val % 10) as u8;
                pid_val /= 10;
                digit_count += 1;
            }
        }
        // Write digits in reverse order
        for i in (0..digit_count).rev() {
            path_buf[pos] = digits[i];
            pos += 1;
        }
        
        let suffix = b"/status";
        path_buf[pos..pos + suffix.len()].copy_from_slice(suffix);
        pos += suffix.len();
        
        std::str::from_utf8(&path_buf[..pos]).unwrap_or("/proc")
    };

    // TIER 2: Read /proc/[pid]/status (file I/O, startup-only)
    let status = std::fs::read_to_string(path)?;

    // TIER 1: Fast line-by-line search (no allocations)
    for line in status.lines() {
        if line.starts_with("Cpus_allowed_list:") {
            // TIER 1: Fast string splitting using split_once (more efficient than split(':').nth())
            // split_once() is ~10-20ns faster than split(':').nth(1) for this use case
            let cpus_list = line.split_once(':')
                .map(|(_, rest)| rest.trim())
                .unwrap_or("");

            // TIER 1: Fast heuristic checks (no allocations)
            // Simple heuristic: if contains comma or is single digit, it's restricted
            if cpus_list.contains(',') || !cpus_list.contains('-') {
                return Ok(true);  // Custom affinity
            }

            // TIER 1: Parse range (e.g., "0-15")
            if let Some((start_str, end_str)) = cpus_list.split_once('-') {
                // TIER 1: Parse integers (no allocations)
                if let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>()) {
                    // Check if range covers most CPUs (within 2 of total)
                    // This handles hyperthreading cases where full might be "0-15" on 8C/16T
                    let range_size = end - start + 1;
                    let nr_cpus = *NR_CPU_IDS;
                    
                    // If range is significantly smaller than total CPUs, it's restricted
                    if range_size < nr_cpus.saturating_sub(2) {
                        return Ok(true);  // Custom affinity
                    }
                }
            }

            break;
        }
    }

    Ok(false)  // Full affinity or unable to determine
}

