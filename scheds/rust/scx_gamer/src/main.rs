// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer v2.0 - Gaming-optimized sched_ext scheduler
// Copyright (c) 2025 RitzDaCat
//
// A scheduler designed for competitive gaming with:
// - 100% hook-based thread classification (no heuristics)
// - Priority boost via boost_shift (0-7 levels)
// - A.B.C. (Always Be Casting) - proactive CPU preparation
// - Physical core preference for latency-critical tasks

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;

use std::collections::VecDeque;
use std::ffi::CStr;
use std::mem::MaybeUninit;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use clap::Parser;
use libbpf_rs::MapCore;
use libbpf_rs::OpenObject;
use libbpf_rs::RingBufferBuilder;
use log::{info, warn};

use scx_utils::build_id;
use scx_utils::scx_ops_attach;
use scx_utils::scx_ops_load;
use scx_utils::scx_ops_open;
use scx_utils::uei_exited;
use scx_utils::uei_report;
use scx_utils::UserExitInfo;

const SCHEDULER_NAME: &str = "scx_gamer";

/// Debug event types (must match enum debug_event_type in types.bpf.h)
const EVENT_STARVATION: u32 = 8;
const EVENT_LONG_WAIT: u32 = 11;

/// Debug event structure (must match struct debug_event in types.bpf.h)
#[repr(C)]
#[derive(Clone, Copy)]
struct DebugEvent {
    timestamp_ns: u64,
    event_type: u32,
    pid: u32,
    tgid: u32,
    boost_old: u8,
    boost_new: u8,
    flags: u8,
    _pad0: u8,
    cpu_from: i32,
    cpu_to: i32,
    wait_ns: u64,
    runtime_ns: u64,
    comm: [u8; 16],
}

/// Task that had a long wait (for debugging outliers)
#[derive(Clone)]
struct SlowTask {
    comm: String,
    pid: u32,
    wait_ms: f64,
    cpu: i32,
    boost: u8,
}

/// Maximum slow/rescued tasks to keep in history
const MAX_TASK_HISTORY: usize = 10;

/// Gaming-optimized sched_ext scheduler
#[derive(Parser, Debug)]
#[command(name = SCHEDULER_NAME)]
#[command(about = "Gaming-optimized sched_ext scheduler for low-latency input and frame delivery")]
#[command(version)]
struct Args {
    /// Base time slice in microseconds (default: 10)
    #[arg(long, default_value = "10")]
    slice_us: u64,

    /// Avoid SMT siblings for latency-critical tasks
    /// Avoid scheduling on SMT siblings (hyperthreads) - usually hurts performance
    #[arg(long, default_value = "false")]
    avoid_smt: bool,

    /// Statistics display interval in seconds (0 = disabled)
    #[arg(long, short = 's', default_value = "0")]
    stats: u64,

    /// Disable ALL statistics collection for maximum performance
    #[arg(long)]
    no_stats: bool,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,

    /// Enable BPF debug prints
    #[arg(long)]
    debug: bool,

    /// Manually set foreground game PID (0 = auto-detect)
    #[arg(long, default_value = "0")]
    foreground_pid: u32,
}

struct Scheduler<'a> {
    skel: BpfSkel<'a>,
    #[allow(dead_code)] // Held to keep struct_ops attached
    struct_ops: Option<libbpf_rs::Link>,
    /// Recent slow tasks - waited >10ms (shared with ring buffer callback)
    slow_tasks: Arc<Mutex<VecDeque<SlowTask>>>,
    /// Recent rescued tasks - triggered starvation rescue at enqueue
    rescued_tasks: Arc<Mutex<VecDeque<SlowTask>>>,
}

impl<'a> Scheduler<'a> {
    fn init(opts: &Args, open_object: &'a mut MaybeUninit<OpenObject>) -> Result<Self> {
        // Initialize libbpf logging
        scx_utils::init_libbpf_logging(None);

        // Open BPF skeleton using scx_ops_open! which:
        // 1. Opens the skeleton
        // 2. Sets hotplug_seq
        // 3. Calls import_enums!() to initialize __SCX_* constants
        let mut skel_builder = BpfSkelBuilder::default();
        skel_builder.obj_builder.debug(opts.verbose);
        
        // Note: None for open_opts means no custom options
        let open_opts = None::<libbpf_rs::libbpf_sys::bpf_object_open_opts>;
        let mut open_skel = scx_ops_open!(skel_builder, open_object, gamer_ops, open_opts)
            .context("Failed to open BPF skeleton")?;

        // Configure tunables before loading
        if let Some(rodata) = open_skel.maps.rodata_data.as_mut() {
            rodata.slice_ns = opts.slice_us * 1000; // Convert µs to ns
            rodata.avoid_smt = opts.avoid_smt;
            rodata.no_stats = opts.no_stats;
            rodata.foreground_tgid = opts.foreground_pid;
            rodata.debug = opts.debug;
        }

        // Load BPF program
        let mut skel = scx_ops_load!(open_skel, gamer_ops, uei)
            .context("Failed to load BPF program")?;

        // Attach scheduler
        let struct_ops = Some(scx_ops_attach!(skel, gamer_ops)
            .context("Failed to attach scheduler")?);

        // Create task tracking histories
        let slow_tasks = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_TASK_HISTORY)));
        let rescued_tasks = Arc::new(Mutex::new(VecDeque::with_capacity(MAX_TASK_HISTORY)));

        info!("{} v{} started", SCHEDULER_NAME, env!("CARGO_PKG_VERSION"));
        info!("  slice_ns: {}µs", opts.slice_us);
        info!("  avoid_smt: {}", opts.avoid_smt);
        info!("  no_stats: {}", opts.no_stats);

        Ok(Self { skel, struct_ops, slow_tasks, rescued_tasks })
    }

    /// Create a ring buffer to consume debug events
    fn create_ring_buffer(&self) -> Result<libbpf_rs::RingBuffer<'static>> {
        let slow_tasks = Arc::clone(&self.slow_tasks);
        let rescued_tasks = Arc::clone(&self.rescued_tasks);
        
        let mut builder = RingBufferBuilder::new();
        
        // Get the debug_events map
        let map = &self.skel.maps.debug_events;
        
        // SAFETY: The callback lifetime is tied to the RingBuffer which we return
        // The Arc<Mutex<>> ensures thread-safe access to task lists
        builder.add(map, move |data: &[u8]| {
            if data.len() < std::mem::size_of::<DebugEvent>() {
                return 0;
            }
            
            // SAFETY: We check the size above
            let event: &DebugEvent = unsafe { &*(data.as_ptr() as *const DebugEvent) };
            
            // Extract task name (null-terminated)
            let comm = CStr::from_bytes_until_nul(&event.comm)
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|_| String::from_utf8_lossy(&event.comm).trim_end_matches('\0').to_string());
            
            let task = SlowTask {
                comm,
                pid: event.pid,
                wait_ms: event.wait_ns as f64 / 1_000_000.0,
                cpu: event.cpu_from,
                boost: event.boost_old,
            };
            
            // Route to appropriate list based on event type
            match event.event_type {
                EVENT_LONG_WAIT => {
                    // Task waited >10ms (detected in running())
                    if let Ok(mut tasks) = slow_tasks.lock() {
                        if tasks.len() >= MAX_TASK_HISTORY {
                            tasks.pop_front();
                        }
                        tasks.push_back(task);
                    }
                }
                EVENT_STARVATION => {
                    // Task triggered starvation rescue (detected in enqueue())
                    if let Ok(mut tasks) = rescued_tasks.lock() {
                        if tasks.len() >= MAX_TASK_HISTORY {
                            tasks.pop_front();
                        }
                        tasks.push_back(task);
                    }
                }
                _ => {} // Ignore other event types
            }
            
            0 // Return 0 to continue processing
        })?;
        
        Ok(builder.build()?)
    }

    /// Poll ring buffer for new events (non-blocking)
    fn poll_events(&self, ring_buffer: &libbpf_rs::RingBuffer) {
        // Poll with 0 timeout for non-blocking
        let _ = ring_buffer.poll(Duration::from_millis(0));
    }

    fn print_stats(&self) {
        // Read stats from BPF map
        let stats_map = &self.skel.maps.stats_map;
        let key: u32 = 0;
        
        if let Ok(stats_bytes) = stats_map.lookup(&key.to_ne_bytes(), libbpf_rs::MapFlags::ANY) {
            if let Some(bytes) = stats_bytes {
                // Parse new enhanced stats structure (must match gamer_stats in types.bpf.h)
                // Section 1: Core Scheduling (offset 0, 64 bytes)
                let nr_enqueued = Self::read_u64(&bytes, 0);      // offset 0
                let nr_dispatched = Self::read_u64(&bytes, 8);    // offset 8
                let nr_direct = Self::read_u64(&bytes, 16);       // offset 16 - direct dispatch (fast path)
                let nr_shared = Self::read_u64(&bytes, 24);       // offset 24 - shared DSQ (slow path)
                
                // Section 2: Detection - Per-Hook Stats (offset 64, 136 bytes)
                // Input hooks (3)
                let nr_hid_irq_in = Self::read_u64(&bytes, 64);
                let nr_input_event = Self::read_u64(&bytes, 72);
                let nr_hid_input_report = Self::read_u64(&bytes, 80);
                // GPU hooks (3)
                let nr_drm_ioctl = Self::read_u64(&bytes, 88);
                let nr_drm_atomic_commit = Self::read_u64(&bytes, 96);
                let nr_dma_fence_signal = Self::read_u64(&bytes, 104);
                // Audio hooks (2)
                let nr_audio_ioctl = Self::read_u64(&bytes, 112);
                let nr_pcm_period = Self::read_u64(&bytes, 120);
                // Sync hooks (3)
                let nr_esync = Self::read_u64(&bytes, 128);
                let nr_fsync = Self::read_u64(&bytes, 136);
                let nr_ntsync = Self::read_u64(&bytes, 144);
                // Aggregate totals
                let nr_input = Self::read_u64(&bytes, 152);
                let nr_gpu = Self::read_u64(&bytes, 160);
                let nr_audio = Self::read_u64(&bytes, 168);
                let nr_sync = Self::read_u64(&bytes, 176);
                // Window boosts
                let nr_input_window = Self::read_u64(&bytes, 184);
                let nr_sync_window = Self::read_u64(&bytes, 192);
                
                // Section 3: Boost histogram (offset 200, 64 bytes)
                let mut boost_hist = [0u64; 8];
                for i in 0..8 {
                    boost_hist[i] = Self::read_u64(&bytes, 200 + i * 8);
                }
                
                // Section 4: CPU Selection (offset 264, 64 bytes)
                let nr_physical = Self::read_u64(&bytes, 264);
                let nr_smt = Self::read_u64(&bytes, 272);
                let nr_migrations = Self::read_u64(&bytes, 280);
                let nr_same_cpu = Self::read_u64(&bytes, 288);
                
                // Section 5: Preemption (offset 328, 64 bytes)
                let nr_preempt = Self::read_u64(&bytes, 328);
                let nr_avoided = Self::read_u64(&bytes, 336);
                let nr_idle = Self::read_u64(&bytes, 344);
                
                // Section 6: Wait histogram (offset 392, 88 bytes - 11 buckets)
                let mut wait_hist = [0u64; 11];
                for i in 0..11 {
                    wait_hist[i] = Self::read_u64(&bytes, 392 + i * 8);
                }
                
                // Section 7: Health (offset 480, 64 bytes)
                let max_wait_ns = Self::read_u64(&bytes, 480);
                let nr_starvation = Self::read_u64(&bytes, 488);
                let nr_errors = Self::read_u64(&bytes, 496);
                
                // Display comprehensive stats
                println!("\n\x1b[1;36m╔═══════════════════════════════════════════════════════════════╗\x1b[0m");
                println!("\x1b[1;36m║               scx_gamer v2.0 - SCHEDULER STATS                ║\x1b[0m");
                println!("\x1b[1;36m╚═══════════════════════════════════════════════════════════════╝\x1b[0m");
                
                // Scheduling with dispatch path breakdown
                println!("\n\x1b[1;33m📊 SCHEDULING\x1b[0m");
                let total_dispatch = nr_direct + nr_enqueued;
                let direct_pct = if total_dispatch > 0 { nr_direct as f64 / total_dispatch as f64 * 100.0 } else { 0.0 };
                let enqueue_pct = if total_dispatch > 0 { nr_enqueued as f64 / total_dispatch as f64 * 100.0 } else { 0.0 };
                let shared_pct = if nr_enqueued > 0 { nr_shared as f64 / nr_enqueued as f64 * 100.0 } else { 0.0 };
                
                println!("  \x1b[32mDirect dispatch (fast):\x1b[0m {:>10} ({:>5.1}%)", nr_direct, direct_pct);
                println!("  \x1b[33mEnqueue fallback:      \x1b[0m {:>10} ({:>5.1}%)", nr_enqueued, enqueue_pct);
                println!("  \x1b[90mShared DSQ (deadline): \x1b[0m {:>10} ({:>5.1}% of enqueue)", nr_shared, shared_pct);
                println!("  Dispatched (running):  {:>10}", nr_dispatched);
                
                // Detection - Per-Hook Breakdown
                println!("\n\x1b[1;33m🎮 DETECTION HOOKS\x1b[0m");
                println!("  \x1b[36m[INPUT]\x1b[0m  hid_irq: {:>8}  input_event: {:>8}  hid_report: {:>8}  │ Total: {:>8}",
                    nr_hid_irq_in, nr_input_event, nr_hid_input_report, nr_input);
                // Color GPU based on whether dma_fence is firing (needed for NVIDIA)
                let gpu_color = if nr_dma_fence_signal > 0 { "\x1b[32m" } else if nr_drm_ioctl > 0 { "\x1b[33m" } else { "\x1b[31m" };
                println!("  {}[GPU]\x1b[0m    drm_ioctl: {:>7}  atomic_commit: {:>5}  dma_fence: {:>7}  │ Total: {:>8}",
                    gpu_color, nr_drm_ioctl, nr_drm_atomic_commit, nr_dma_fence_signal, nr_gpu);
                println!("  \x1b[34m[AUDIO]\x1b[0m  alsa_ioctl: {:>6}  pcm_period: {:>8}                      │ Total: {:>8}",
                    nr_audio_ioctl, nr_pcm_period, nr_audio);
                // Color sync based on which mechanisms are firing
                let sync_color = if nr_ntsync > 0 { "\x1b[32m" } else if nr_fsync > 0 { "\x1b[33m" } else if nr_esync > 0 { "\x1b[33m" } else { "\x1b[31m" };
                println!("  {}[SYNC]\x1b[0m   esync: {:>10}  fsync: {:>10}  ntsync: {:>9}  │ Total: {:>8}",
                    sync_color, nr_esync, nr_fsync, nr_ntsync, nr_sync);
                println!("  Window boosts → Input: {:>6}   Sync: {:>6}", nr_input_window, nr_sync_window);
                
                // Boost histogram with visual bars
                println!("\n\x1b[1;33m⚡ PRIORITY DISTRIBUTION\x1b[0m");
                let labels = ["BG(0)", "FG(1)", "GW(2)", "GM(3)", "CO(4)", "AU(5)", "GP(6)", "IN(7)"];
                let colors = ["\x1b[90m", "\x1b[37m", "\x1b[32m", "\x1b[32m", "\x1b[36m", "\x1b[34m", "\x1b[35m", "\x1b[31m"];
                let total_boost: u64 = boost_hist.iter().sum();
                if total_boost > 0 {
                    for (i, &count) in boost_hist.iter().enumerate() {
                        if count > 0 {
                            let pct = count as f64 / total_boost as f64 * 100.0;
                            let bar = "█".repeat((pct / 3.0) as usize).chars().take(20).collect::<String>();
                            println!("  {}: {:>10} {:>5.1}% {}{}█\x1b[0m", 
                                labels[i], count, pct, colors[i], bar);
                        }
                    }
                }
                
                // Wait time histogram (extended for starvation debugging)
                println!("\n\x1b[1;33m⏱️  WAIT TIME (task queue latency)\x1b[0m");
                let wait_labels = [
                    "<1µs", "1-10µs", "10-100µs", "0.1-1ms", "1-10ms", 
                    "10-100ms", "0.1-1s", "1-3s", "3-5s", "5-10s", ">10s"
                ];
                let total_wait: u64 = wait_hist.iter().sum();
                if total_wait > 0 {
                    for (i, &count) in wait_hist.iter().enumerate() {
                        if count > 0 {
                            let pct = count as f64 / total_wait as f64 * 100.0;
                            let bar = "▓".repeat((pct / 3.0) as usize).chars().take(20).collect::<String>();
                            // Color coding: green=good, yellow=warning, red=bad, magenta=critical starvation
                            let color = if i <= 2 { 
                                "\x1b[32m"  // Green: <100µs (excellent)
                            } else if i <= 4 { 
                                "\x1b[33m"  // Yellow: 100µs-10ms (acceptable)
                            } else if i <= 6 { 
                                "\x1b[31m"  // Red: 10ms-1s (bad)
                            } else { 
                                "\x1b[35m"  // Magenta: >1s (CRITICAL STARVATION)
                            };
                            println!("  {:>8}: {:>10} {:>5.1}% {}{}▓\x1b[0m",
                                wait_labels[i], count, pct, color, bar);
                        }
                    }
                }
                
                // CPU Selection
                println!("\n\x1b[1;33m🖥️  CPU SELECTION\x1b[0m");
                let total_cpu = nr_physical + nr_smt;
                if total_cpu > 0 {
                    println!("  Physical: {:>8} ({:>5.1}%)   SMT: {:>8} ({:>5.1}%)",
                        nr_physical, nr_physical as f64 / total_cpu as f64 * 100.0,
                        nr_smt, nr_smt as f64 / total_cpu as f64 * 100.0);
                }
                println!("  Migrations: {:>6}   Same CPU: {:>8}", nr_migrations, nr_same_cpu);
                
                // Preemption
                println!("\n\x1b[1;33m🔄 PREEMPTION DECISIONS\x1b[0m");
                let total_kick = nr_preempt + nr_avoided + nr_idle;
                if total_kick > 0 {
                    println!("  \x1b[33mPreempt:\x1b[0m {:>8} ({:>5.1}%)  \x1b[32mAvoided:\x1b[0m {:>8} ({:>5.1}%)  \x1b[36mIdle:\x1b[0m {:>8}",
                        nr_preempt, nr_preempt as f64 / total_kick as f64 * 100.0,
                        nr_avoided, nr_avoided as f64 / total_kick as f64 * 100.0,
                        nr_idle);
                }
                
                // Health
                println!("\n\x1b[1;33m❤️  HEALTH\x1b[0m");
                let max_ms = max_wait_ns as f64 / 1_000_000.0;
                let color = if max_ms < 1.0 { "\x1b[32m" } else if max_ms < 10.0 { "\x1b[33m" } else { "\x1b[31m" };
                println!("  Max wait: {}{:.2}ms\x1b[0m   Starvation rescues: {:>6}   Errors: {}",
                    color, max_ms, nr_starvation, nr_errors);
                
                // Show slow tasks (waited >10ms) - catches ALL long waits including direct dispatch
                if let Ok(tasks) = self.slow_tasks.lock() {
                    if !tasks.is_empty() {
                        println!("\n\x1b[1;33m⏰ SLOW TASKS (waited >10ms)\x1b[0m");
                        let boost_names = ["BG", "FG", "GW", "GM", "CO", "AU", "GP", "IN"];
                        for task in tasks.iter().rev().take(5) {
                            let boost_name = if (task.boost as usize) < boost_names.len() { 
                                boost_names[task.boost as usize] 
                            } else { 
                                "??" 
                            };
                            
                            // Identify kernel background workers vs user tasks
                            let is_kworker = task.comm.starts_with("kworker");
                            let is_kernel_bg = is_kworker || 
                                task.comm.starts_with("ksoftirqd") ||
                                task.comm.starts_with("kcompactd") ||
                                task.comm.starts_with("khugepaged") ||
                                task.comm.starts_with("migration");
                            
                            if is_kernel_bg {
                                // Dim gray for kernel background - not concerning
                                println!("  \x1b[90m{:>16} (pid:{:>6}) cpu:{:>2} boost:{} waited {:.2}ms [kernel-bg]\x1b[0m",
                                    task.comm, task.pid, task.cpu, boost_name, task.wait_ms);
                            } else {
                                // Highlighted for user tasks - might need attention
                                let wait_color = if task.wait_ms < 15.0 { "\x1b[33m" } else { "\x1b[31m" };
                                println!("  \x1b[1m{:>16}\x1b[0m (pid:{:>6}) cpu:{:>2} boost:{} waited {}{:.2}ms\x1b[0m ⚠️",
                                    task.comm, task.pid, task.cpu, boost_name, wait_color, task.wait_ms);
                            }
                        }
                    }
                }
                
                // Show rescued tasks (triggered starvation rescue at enqueue)
                if let Ok(tasks) = self.rescued_tasks.lock() {
                    if !tasks.is_empty() {
                        println!("\n\x1b[1;35m🚨 RESCUED TASKS (starvation rescue triggered)\x1b[0m");
                        let boost_names = ["BG", "FG", "GW", "GM", "CO", "AU", "GP", "IN"];
                        for task in tasks.iter().rev().take(5) {
                            let boost_name = if (task.boost as usize) < boost_names.len() { 
                                boost_names[task.boost as usize] 
                            } else { 
                                "??" 
                            };
                            
                            // Identify kernel background workers
                            let is_kernel_bg = task.comm.starts_with("kworker") || 
                                task.comm.starts_with("ksoftirqd") ||
                                task.comm.starts_with("kcompactd") ||
                                task.comm.starts_with("khugepaged") ||
                                task.comm.starts_with("migration");
                            
                            if is_kernel_bg {
                                // Dim for kernel background
                                println!("  \x1b[90m{:>16} (pid:{:>6}) cpu:{:>2} boost:{} waited {:.2}ms [kernel-bg]\x1b[0m",
                                    task.comm, task.pid, task.cpu, boost_name, task.wait_ms);
                            } else {
                                // Red highlight for user tasks needing rescue
                                println!("  \x1b[31;1m{:>16}\x1b[0m (pid:{:>6}) cpu:{:>2} boost:{} waited {:.2}ms ⚠️",
                                    task.comm, task.pid, task.cpu, boost_name, task.wait_ms);
                            }
                        }
                    }
                }
                
                println!("\n\x1b[90m───────────────────────────────────────────────────────────────────\x1b[0m");
            }
        }
    }
    
    fn read_u64(bytes: &[u8], offset: usize) -> u64 {
        if offset + 8 <= bytes.len() {
            u64::from_ne_bytes(bytes[offset..offset+8].try_into().unwrap_or([0; 8]))
        } else {
            0
        }
    }

    fn run(&mut self, opts: &Args) -> Result<UserExitInfo> {
        let mut last_stats = Instant::now();
        let stats_interval = if opts.stats > 0 && !opts.no_stats {
            Some(Duration::from_secs(opts.stats))
        } else {
            None
        };

        // Create ring buffer for debug events (if debug mode enabled)
        let ring_buffer = if opts.debug {
            match self.create_ring_buffer() {
                Ok(rb) => {
                    info!("Debug ring buffer enabled - tracking rescued tasks");
                    Some(rb)
                }
                Err(e) => {
                    warn!("Failed to create ring buffer: {}", e);
                    None
                }
            }
        } else {
            None
        };

        loop {
            // Check for scheduler exit
            if uei_exited!(&self.skel, uei) {
                break;
            }

            // Poll ring buffer for debug events
            if let Some(ref rb) = ring_buffer {
                self.poll_events(rb);
            }

            // Print stats if interval elapsed
            if let Some(interval) = stats_interval {
                if last_stats.elapsed() >= interval {
                    self.print_stats();
                    last_stats = Instant::now();
                }
            }

            // Sleep to avoid busy-waiting
            std::thread::sleep(Duration::from_millis(100));
        }

        // Get exit info
        uei_report!(&self.skel, uei)
    }
}

impl Drop for Scheduler<'_> {
    fn drop(&mut self) {
        info!("{} shutting down", SCHEDULER_NAME);
    }
}

fn main() -> Result<()> {
    let opts = Args::parse();

    // Validate options
    if opts.slice_us == 0 || opts.slice_us > 1000 {
        anyhow::bail!("slice_us must be between 1 and 1000");
    }
    if opts.no_stats && opts.stats > 0 {
        warn!("--no-stats overrides --stats, statistics disabled");
    }

    // Initialize logging
    let log_level = if opts.verbose {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Info
    };
    simplelog::TermLogger::init(
        log_level,
        simplelog::Config::default(),
        simplelog::TerminalMode::Mixed,
        simplelog::ColorChoice::Auto,
    )
    .context("Failed to initialize logger")?;

    // Print build info
    info!("{} v{}", SCHEDULER_NAME, env!("CARGO_PKG_VERSION"));
    info!("Build ID: {}", build_id::full_version(env!("CARGO_PKG_VERSION")));

    // Initialize and run scheduler
    let mut open_object = MaybeUninit::uninit();
    let mut scheduler = Scheduler::init(&opts, &mut open_object)?;
    let _uei = scheduler.run(&opts)?;

    // Report exit status
    info!("Scheduler exited");

    Ok(())
}
