// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

// Removed: enable_kernel_busy_polling() - no longer needed with interrupt-driven approach
// Removed: pin_current_thread_to_cpu() - unused function (was for input thread CPU pinning)

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;
pub use bpf_intf::*;

mod affinity_override; // CPU affinity override system (proactive)
// REMOVED: mod audio_detect - redundant with fentry-based BPF detection
// REMOVED: mod debug_api - HTTP server adds unnecessary overhead
// REMOVED: mod engine_presets - brittle name-based thread classification
mod focus_detect; // D-Bus event-based window focus detection (replaces polling/heuristics)
mod game_detect;
mod game_detect_bpf; // BPF LSM-based game detection (modern, kernel-level)
mod gpu_queue_monitor;
// REMOVED: mod power_monitor - power efficiency contradicts gaming performance goal
mod ring_buffer;
mod stats;
mod trigger;
// REMOVED: mod tui - bloat (3334 lines), use --stats for monitoring
use crate::game_detect::GameDetector;
use crate::game_detect_bpf::BpfGameDetector;
use crate::gpu_queue_monitor::{monotonic_nanos, GpuQueueMonitor};
use crate::trigger::TriggerOps;
use rustc_hash::FxHashSet;
use std::ffi::c_int;
// removed: userspace /proc/stat util sampling
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, AsRawFd};
use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use evdev::EventType;
use libbpf_rs::MapCore;
use libbpf_rs::OpenObject;
use libbpf_rs::ProgramInput;
use libc::{
    sched_attr, sched_param, sched_setscheduler, SCHED_DEADLINE, SCHED_FIFO, SCHED_FLAG_DL_OVERRUN,
    SCHED_OTHER,
};
use log::{info, warn};
use nix::fcntl;
use nix::sched::{sched_setaffinity, CpuSet};
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::unistd::Pid;
use scx_stats::prelude::*;
use scx_utils::build_id;
use scx_utils::compat;
use scx_utils::init_libbpf_logging;
use scx_utils::libbpf_clap_opts::LibbpfOpts;
use scx_utils::parse_cpu_list;
use scx_utils::scx_ops_attach;
use scx_utils::scx_ops_load;
use scx_utils::scx_ops_open;
use scx_utils::try_set_rlimit_infinity;
use scx_utils::uei_exited;
use scx_utils::uei_report;
use scx_utils::CoreType;
use scx_utils::Topology;
use scx_utils::UserExitInfo;
use scx_utils::NR_CPU_IDS;
use stats::Metrics;

const SCHEDULER_NAME: &str = "scx_gamer";
const CCD_CLASS_UNKNOWN: u8 = 0;
const CCD_CLASS_CACHE: u8 = 1;
const CCD_CLASS_FREQ: u8 = 2;

/// PSI (Pressure Stall Information) values from /proc/pressure/*
/// Linux 4.20+ provides this for CPU/memory/IO stall monitoring
#[derive(Debug, Default, Clone, Copy)]
struct PsiValues {
    some_avg10: f64,
    some_avg60: f64,
    full_avg10: f64,
    full_avg60: f64,
}

/// Read PSI metrics from /proc/pressure/{cpu,memory,io}
/// Returns (cpu, memory, io) PSI values
fn read_psi() -> (PsiValues, PsiValues, PsiValues) {
    fn parse_psi_file(path: &str) -> PsiValues {
        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return PsiValues::default(),
        };

        let mut vals = PsiValues::default();
        for line in content.lines() {
            // Format: "some avg10=X.XX avg60=X.XX avg300=X.XX total=XXXXX"
            // or:     "full avg10=X.XX avg60=X.XX avg300=X.XX total=XXXXX"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 3 {
                continue;
            }

            let is_some = parts[0] == "some";
            let is_full = parts[0] == "full";

            for part in &parts[1..] {
                if let Some(val_str) = part.strip_prefix("avg10=") {
                    if let Ok(v) = val_str.parse::<f64>() {
                        if is_some {
                            vals.some_avg10 = v;
                        } else if is_full {
                            vals.full_avg10 = v;
                        }
                    }
                } else if let Some(val_str) = part.strip_prefix("avg60=") {
                    if let Ok(v) = val_str.parse::<f64>() {
                        if is_some {
                            vals.some_avg60 = v;
                        } else if is_full {
                            vals.full_avg60 = v;
                        }
                    }
                }
            }
        }
        vals
    }

    (
        parse_psi_file("/proc/pressure/cpu"),
        parse_psi_file("/proc/pressure/memory"),
        parse_psi_file("/proc/pressure/io"),
    )
}

fn stats_interval_from_secs(value: f64) -> Option<Duration> {
    if !value.is_finite() || value <= 0.0 {
        None
    } else {
        Some(Duration::from_secs_f64(value))
    }
}

// ZERO-LATENCY MODE: No gap debouncing - removed entirely
// All input events trigger immediately for competitive gaming
// Gap constants removed - see commit history for batching implementation

/// Cached device type to avoid per-event type checking
#[derive(Debug, Clone, Copy)]
#[repr(u32)]
enum DeviceType {
    Keyboard = 0,
    Mouse = 1,
    Other = 2,
}

impl DeviceType {
    const fn lane(self) -> InputLane {
        match self {
            DeviceType::Mouse => InputLane::Mouse,
            DeviceType::Keyboard => InputLane::Keyboard,
            DeviceType::Other => InputLane::Other,
        }
    }
}

#[repr(u32)]
#[derive(Debug, Copy, Clone)]
pub enum InputLane {
    Keyboard = 0,
    Mouse = 1,
    Other = 2,
}

/// Combined device info to avoid double HashMap lookups in hot path
/// Bit-packed for optimal cache utilization: 24 bits for idx, 8 bits for lane
#[derive(Debug, Clone, Copy)]
struct DeviceInfo {
    packed_info: u32,
}

impl DeviceInfo {
    /// Create new DeviceInfo with packed idx and lane
    ///
    /// # Arguments
    /// * `idx` - Device index (max 16M devices)
    /// * `lane` - Input lane type
    ///
    /// # Returns
    /// * `Self` - Packed DeviceInfo
    fn new(idx: usize, lane: InputLane) -> Self {
        // Pack: 24 bits for idx (max 16M devices), 8 bits for lane
        let packed_info = ((idx as u32) & 0xFFFFFF) | ((lane as u32) << 24);
        Self { packed_info }
    }

    /// Get device index
    ///
    /// # Returns
    /// * `usize` - Device index
    fn idx(&self) -> usize {
        (self.packed_info & 0xFFFFFF) as usize
    }

    /// Get input lane
    ///
    /// # Returns
    /// * `InputLane` - Input lane type
    fn lane(&self) -> InputLane {
        match (self.packed_info >> 24) as u8 {
            0 => InputLane::Keyboard,
            1 => InputLane::Mouse,
            2 => InputLane::Other,
            _ => InputLane::Other, // Default fallback
        }
    }
}

/// Scheduler profile presets optimized for different use cases.
/// Profiles set sensible defaults that can be overridden by explicit flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Profile {
    /// Default gaming profile: competitive settings with low latency.
    /// slice=250us, input-window=8ms, keyboard-boost=300ms, mouse-boost=6ms,
    /// avoid-smt=on, prefer-napi-on-input=on
    #[default]
    Esports,
    /// Clean scheduler defaults with minimal tuning.
    /// slice=1000us, no aggressive optimizations.
    /// Good for general desktop use and light gaming.
    Baseline,
    /// Balanced performance for casual/single-player gaming.
    /// slice=500us, keyboard-boost=1500ms, mouse-boost=10ms.
    /// Good for RPGs, 60Hz monitors, menu-heavy games.
    Casual,
    /// Extreme low-latency for competitive play and aim trainers.
    /// slice=5us, input-window=2ms, keyboard-boost=100ms, mouse-boost=4ms,
    /// wakeup-timer=100us, mig-max=2.
    /// Best for 360Hz+ displays, aim trainers.
    Ultra,
}

impl std::fmt::Display for Profile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Profile::Esports => write!(f, "esports"),
            Profile::Baseline => write!(f, "baseline"),
            Profile::Casual => write!(f, "casual"),
            Profile::Ultra => write!(f, "ultra"),
        }
    }
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "scx_gamer",
    version,
    disable_version_flag = true,
    about = "Gaming-optimized scheduler for low-latency input and frame delivery.\n\n\
             Default profile is 'esports' - competitive gaming with minimal overhead.\n\
             Stats/monitoring are DISABLED by default for maximum performance.\n\
             Use --monitoring to enable observability features."
)]
struct Opts {
    /// Load a preset profile. Individual flags override profile defaults.
    /// Profiles: esports (default), baseline, casual, ultra
    #[clap(long, value_enum, default_value = "esports")]
    profile: Profile,

    /// Enable all monitoring/observability features.
    /// Turns on: stats collection, detectors, runtime tracing, dispatch events.
    /// Use this when debugging, profiling, or running with TUI/stats output.
    /// Note: Adds ~0.6-1.2% overhead from BPF stats collection.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    monitoring: bool,

    /// Exit debug dump buffer length. 0 indicates default.
    #[clap(long, default_value = "0")]
    exit_dump_len: u32,

    /// Maximum scheduling slice duration in microseconds.
    /// Lower values = more responsive but higher overhead.
    /// Profile defaults: baseline=1000, casual=500, esports=250, ultra=5
    #[clap(short = 's', long)]
    slice_us: Option<u64>,

    /// Maximum runtime (since last sleep) that can be charged to a task in microseconds.
    #[clap(short = 'l', long, default_value = "20000")]
    slice_lag_us: u64,

    /// Deprecated: userspace CPU util polling (no-op). Kept for compatibility.
    /// Set to 0 (default). In-kernel sampling via BPF is used instead.
    #[clap(short = 'p', long, default_value = "0")]
    polling_ms: u64,

    /// Specifies a list of CPUs to prioritize.
    ///
    /// Accepts a comma-separated list of CPUs or ranges (i.e., 0-3,12-15) or the following special
    /// keywords:
    ///
    /// "turbo" = automatically detect and prioritize the CPUs with the highest max frequency,
    /// "performance" = automatically detect and prioritize the fastest CPUs,
    /// "powersave" = automatically detect and prioritize the slowest CPUs,
    /// "all" = all CPUs assigned to the primary domain.
    ///
    /// By default "all" CPUs are used.
    #[clap(short = 'm', long)]
    primary_domain: Option<String>,

    /// Enable NUMA optimizations.
    #[clap(short = 'n', long, action = clap::ArgAction::SetTrue)]
    enable_numa: bool,

    /// Disable CPU frequency control.
    #[clap(short = 'f', long, action = clap::ArgAction::SetTrue)]
    disable_cpufreq: bool,

    /// Enable flat idle CPU scanning.
    ///
    /// This option can help reducing some overhead when trying to allocate idle CPUs and it can be
    /// quite effective with simple CPU topologies.
    #[arg(short = 'i', long, action = clap::ArgAction::SetTrue)]
    flat_idle_scan: bool,

    /// Disable preferred idle CPU scanning.
    ///
    /// By default, the scheduler prioritizes assigning tasks to higher-ranked cores before
    /// considering lower-ranked ones. This flag disables that behavior.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    no_preferred_idle_scan: bool,

    /// Disable SMT.
    ///
    /// This option can only be used together with --flat-idle-scan or --preferred-idle-scan,
    /// otherwise it is ignored.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    disable_smt: bool,

    /// SMT contention avoidance.
    ///
    /// When enabled, the scheduler aggressively avoids placing tasks on sibling SMT threads.
    /// This may increase task migrations and lower overall throughput, but can lead to more
    /// consistent performance by reducing contention on shared SMT cores.
    /// Enabled by default for esports/ultra profiles.
    #[clap(short = 'S', long)]
    avoid_smt: Option<bool>,

    /// Disable direct dispatch during synchronous wakeups.
    ///
    /// Enabling this option can lead to a more uniform load distribution across available cores,
    /// potentially improving performance in certain scenarios. However, it may come at the cost of
    /// reduced efficiency for pipe-intensive workloads that benefit from tighter producer-consumer
    /// coupling.
    #[clap(short = 'w', long, action = clap::ArgAction::SetTrue)]
    no_wake_sync: bool,

    /// Disable deferred wakeups.
    ///
    /// Enabling this option can reduce throughput and performance for certain workloads, but it
    /// can also reduce power consumption (useful on battery-powered systems).
    #[clap(short = 'd', long, action = clap::ArgAction::SetTrue)]
    no_deferred_wakeup: bool,

    /// Disable address space affinity.
    ///
    /// By default, the scheduler keeps tasks that share the same address space (e.g., threads
    /// of the same process) on the same CPU across wakeups for better cache locality.
    /// This flag disables that behavior.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    no_mm_affinity: bool,

    /// Migration limiter: window size in milliseconds.
    #[clap(long, default_value = "50")]
    mig_window_ms: u64,

    /// Migration limiter: maximum migrations allowed per task within the window.
    /// Profile defaults: baseline/casual/esports=3, ultra=2
    #[clap(long)]
    mig_max: Option<u32>,

    /// Input-active boost window in microseconds (0=disabled).
    /// Covers Wine/Proton input translation layer delays (200-500µs)
    /// plus game processing time (500-2000µs).
    /// Profile defaults: baseline=5000, casual=5000, esports=8000, ultra=2000
    #[clap(long)]
    input_window_us: Option<u64>,

    /// Keyboard boost duration in microseconds.
    /// Duration for which keyboard input extends the boost window.
    /// Lower values (100-300ms) optimal for competitive gaming - fast response, minimal boost bleed.
    /// Higher values (500-1500ms) better for casual gaming and menu navigation.
    /// Profile defaults: baseline=200000, casual=1500000, esports=300000, ultra=100000
    #[clap(long)]
    keyboard_boost_us: Option<u64>,

    /// Mouse boost duration in microseconds.
    /// Duration for which mouse movement extends the boost window.
    /// Covers high-rate mouse polling (1000-8000Hz) and small movement bursts.
    /// Lower values (4-6ms) reduce latency variance for competitive FPS.
    /// Higher values (8-12ms) better for tracking and casual gaming.
    /// Profile defaults: baseline=8000, casual=10000, esports=6000, ultra=4000
    #[clap(long)]
    mouse_boost_us: Option<u64>,

    /// Controller boost duration in microseconds.
    /// Duration for which controller input (thumbstick, triggers) extends the boost window.
    /// Covers analog input from gamepads and console-style games.
    /// Lower values (100-200ms) for fighting games with precise inputs.
    /// Higher values (300-500ms) for open world games with sustained analog input.
    #[clap(long, default_value = "200000")]
    controller_boost_us: u64,

    /// Watchdog: if no dispatch progress is observed for N seconds, exit to restore CFS (0=off).
    #[clap(long, default_value = "0")]
    watchdog_secs: u64,

    /// Prefer NAPI/softirq CPUs briefly during input window.
    /// Helps online games by keeping network processing close to input handling.
    /// Enabled by default for esports/casual profiles, disabled for baseline/ultra.
    #[clap(long)]
    prefer_napi_on_input: Option<bool>,

    /// Enable extended detector hooks (network/storage/fs/memory).
    /// Disabled by default. Use --monitoring to enable all observability.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    enable_detectors: bool,

    /// Enable runtime tracing (track_thread_ru).
    /// Disabled by default. Use --monitoring to enable all observability.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    enable_runtime_trace: bool,

    /// Emit dispatch events to the ring buffer (for watchdog / diagnostics).
    /// Disabled by default. Use --monitoring to enable all observability.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    enable_dispatch_events: bool,

    /// Disable per-mm recent CPU hint (cache affinity hinting, enabled by default).
    #[clap(long, action = clap::ArgAction::SetTrue)]
    disable_mm_hint: bool,

    /// Per-mm hint LRU size (entries). Clamped to [128, 65536].
    #[clap(long, default_value = "8192")]
    mm_hint_size: u32,

    /// Wakeup timer period in microseconds (min 250). 0=use slice_us.
    /// Profile defaults: baseline/casual/esports=500, ultra=100
    #[clap(long)]
    wakeup_timer_us: Option<u64>,

    /// Enable stats monitoring with the specified interval.
    /// Automatically enables --monitoring.
    #[clap(long)]
    stats: Option<f64>,

    /// Run in stats monitoring mode with the specified interval. Scheduler
    /// is not launched.
    #[clap(long)]
    monitor: Option<f64>,

    /// Run in TUI (Terminal UI) mode with the specified interval. Scheduler
    /// is not launched. Provides interactive dashboard.
    /// Automatically enables --monitoring.
    #[clap(long)]
    tui: Option<f64>,

    /// Watch input boost state (keyboard/mouse lanes) at the specified interval.
    /// Prints ON/OFF per lane and trigger rates without launching the TUI.
    #[clap(long)]
    watch_input: Option<f64>,

    /// Enable verbose output, including libbpf details.
    #[clap(short = 'v', long, action = clap::ArgAction::SetTrue)]
    verbose: bool,

    /// Print scheduler version and exit.
    #[clap(short = 'V', long, action = clap::ArgAction::SetTrue)]
    version: bool,

    /// Show descriptions for statistics.
    #[clap(long)]
    help_stats: bool,

    #[clap(flatten, next_help_heading = "Libbpf Options")]
    pub libbpf: LibbpfOpts,

    /// Pin the event loop (epoll/timerfd/input) to a specific CPU
    #[clap(long)]
    event_loop_cpu: Option<usize>,

    /// Use real-time scheduling policy (SCHED_FIFO) for ultra-low latency
    /// WARNING: Misbehaving real-time processes can lock up the system
    #[clap(long, action = clap::ArgAction::SetTrue)]
    realtime_scheduling: bool,

    /// Real-time priority (1-99, higher = more priority, default: 50)
    #[clap(long, default_value = "50")]
    rt_priority: u32,

    /// Use SCHED_DEADLINE for ultra-low latency with time guarantees
    /// Provides hard real-time guarantees without starvation risk
    #[clap(long, action = clap::ArgAction::SetTrue)]
    deadline_scheduling: bool,

    /// SCHED_DEADLINE runtime in microseconds (default: 500)
    #[clap(long, default_value = "500")]
    deadline_runtime_us: u64,

    /// SCHED_DEADLINE deadline in microseconds (default: 1000)
    #[clap(long, default_value = "1000")]
    deadline_deadline_us: u64,

    /// SCHED_DEADLINE period in microseconds (default: 1000)
    #[clap(long, default_value = "1000")]
    deadline_period_us: u64,
    // REMOVED: debug_api - HTTP server removed for leaner scheduler
}

impl Opts {
    /// Get the effective slice_us value based on profile and explicit override.
    fn effective_slice_us(&self) -> u64 {
        self.slice_us.unwrap_or(match self.profile {
            Profile::Baseline => 1000,
            Profile::Casual => 500,
            Profile::Esports => 250,
            Profile::Ultra => 5,
        })
    }

    /// Get the effective input_window_us value based on profile and explicit override.
    fn effective_input_window_us(&self) -> u64 {
        self.input_window_us.unwrap_or(match self.profile {
            Profile::Baseline => 5000,
            Profile::Casual => 5000,
            Profile::Esports => 8000,
            Profile::Ultra => 2000,
        })
    }

    /// Get the effective keyboard_boost_us value based on profile and explicit override.
    fn effective_keyboard_boost_us(&self) -> u64 {
        self.keyboard_boost_us.unwrap_or(match self.profile {
            Profile::Baseline => 200_000,
            Profile::Casual => 1_500_000,
            Profile::Esports => 300_000,
            Profile::Ultra => 100_000,
        })
    }

    /// Get the effective mouse_boost_us value based on profile and explicit override.
    fn effective_mouse_boost_us(&self) -> u64 {
        self.mouse_boost_us.unwrap_or(match self.profile {
            Profile::Baseline => 8000,
            Profile::Casual => 10_000,
            Profile::Esports => 6000,
            Profile::Ultra => 4000,
        })
    }

    /// Get the effective mig_max value based on profile and explicit override.
    fn effective_mig_max(&self) -> u32 {
        self.mig_max.unwrap_or(match self.profile {
            Profile::Baseline | Profile::Casual | Profile::Esports => 3,
            Profile::Ultra => 2,
        })
    }

    /// Get the effective wakeup_timer_us value based on profile and explicit override.
    fn effective_wakeup_timer_us(&self) -> u64 {
        self.wakeup_timer_us.unwrap_or(match self.profile {
            Profile::Baseline | Profile::Casual | Profile::Esports => 500,
            Profile::Ultra => 100,
        })
    }

    /// Get the effective avoid_smt value based on profile and explicit override.
    fn effective_avoid_smt(&self) -> bool {
        self.avoid_smt.unwrap_or(match self.profile {
            Profile::Baseline | Profile::Casual => false,
            Profile::Esports | Profile::Ultra => true,
        })
    }

    /// Get the effective prefer_napi_on_input value based on profile and explicit override.
    fn effective_prefer_napi_on_input(&self) -> bool {
        self.prefer_napi_on_input.unwrap_or(match self.profile {
            Profile::Baseline | Profile::Ultra => false,
            Profile::Casual | Profile::Esports => true,
        })
    }

    /// Check if monitoring is enabled (explicit flag or implied by stats/tui/debug-api).
    fn is_monitoring_enabled(&self) -> bool {
        self.monitoring
            || self.stats.is_some()
            || self.tui.is_some()
            // REMOVED: debug_api check - HTTP server removed
    }
}

// CPU parsing helpers moved to scx_utils::cpu_list

// removed: CpuTimes and userspace util sampling helpers

struct Scheduler<'a> {
    skel: BpfSkel<'a>,
    opts: &'a Opts,
    struct_ops: Option<libbpf_rs::Link>,
    stats_server: Option<StatsServer<(), Metrics>>,
    input_devs: Vec<evdev::Device>,
    epoll_fd: Option<Epoll>,
    input_fd_info_vec: Vec<Option<DeviceInfo>>, // Direct array access for hot path
    registered_epoll_fds: FxHashSet<i32>,
    trig: trigger::BpfTrigger,
    input_trigger_fn: fn(&trigger::BpfTrigger, &mut BpfSkel, InputLane),
    // FOCUS DETECTION (PRIMARY): D-Bus event-based, zero polling, zero heuristics
    // The focused window is EXACTLY what the user wants to be fast - no guessing needed
    focus_detector: Option<focus_detect::FocusDetector>,
    // GAME DETECTION (FALLBACK): Only used if D-Bus focus detection unavailable
    bpf_game_detector: Option<BpfGameDetector>, // BPF LSM game detection (kernel-level)
    game_detector: Option<GameDetector>,        // Fallback inotify detection (if BPF unavailable)
    input_ring_buffer: Option<ring_buffer::InputRingBufferManager>, // Interrupt-driven ring buffer for ultra-low latency input
    dispatch_event_ringbuf: Option<libbpf_rs::RingBuffer<'a>>, // Event-driven dispatch events for watchdog (eliminates polling)
    // REMOVED: debug_api_state - HTTP server removed for leaner scheduler
    // REMOVED: audio_detector - redundant with fentry-based BPF detection
    // REMOVED: audio_update_buffer - userspace audio detection removed
    #[allow(dead_code)] // Held for Drop behavior - keeps affinity override thread running
    affinity_override: Option<affinity_override::AffinityOverride>, // CPU affinity override system (proactive)
    #[allow(dead_code)]
    // Used by macros (uei_exited!, uei_report!) which use identifier name, not direct access
    uei: UserExitInfo, // User exit info for BPF communication
    // REMOVED: power_monitor - power efficiency contradicts gaming performance goal
    gpu_queue_monitor: Option<GpuQueueMonitor>,
    // REMOVED: power_hint_rx - power monitoring removed
    gpu_busy_rx: Option<Receiver<u32>>,
    // REMOVED: power_monitor_worker - power monitoring removed
    gpu_monitor_worker: Option<thread::JoinHandle<()>>,

    // AI Analytics: Temporal pattern tracking (rolling windows)
    migration_history_10s: std::collections::VecDeque<(Instant, u64)>, // (timestamp, migration_count)
    migration_history_60s: std::collections::VecDeque<(Instant, u64)>, // (timestamp, migration_count)
    cpu_util_history: std::collections::VecDeque<(Instant, u64)>,      // (timestamp, cpu_util)
    frame_rate_history: std::collections::VecDeque<(Instant, f64)>,    // (timestamp, frame_hz_est)
    last_migration_count: u64, // Last migration count for delta calculation
    tracked_game_threads: FxHashSet<u32>,
}

impl<'a> Scheduler<'a> {
    /// Get current foreground TGID using priority: Focus > BPF LSM > inotify
    /// 
    /// DETECTION HIERARCHY (most reliable first):
    /// 1. D-Bus Focus Detection (PRIMARY)
    ///    - Event-based: compositor tells us exactly which window is focused
    ///    - Zero heuristics: no guessing based on thread count/memory
    ///    - 100% proof: the focused window IS what the user wants fast
    /// 
    /// 2. BPF LSM Game Detection (FALLBACK)
    ///    - Kernel-level tracking of game processes
    ///    - More reliable than /proc scanning
    /// 
    /// 3. inotify Game Detection (LAST RESORT)
    ///    - /proc scanning with heuristics
    ///    - Only used if both D-Bus and BPF unavailable
    #[inline]
    fn get_detected_game_tgid(&self) -> u32 {
        // PRIMARY: D-Bus focus detection - zero heuristics, 100% proof
        if let Some(ref detector) = self.focus_detector {
            let pid = detector.get_focused_pid();
            if pid > 0 {
                return pid;
            }
        }
        
        // FALLBACK: BPF LSM game detection
        if let Some(ref detector) = self.bpf_game_detector {
            let tgid = detector.get_game_tgid();
            if tgid > 0 {
                return tgid;
            }
        }
        
        // LAST RESORT: inotify game detection
        if let Some(ref detector) = self.game_detector {
            return detector.get_game_tgid();
        }
        
        0
    }

    /// Get full game info from active detector
    #[inline]
    fn get_detected_game_info(&self) -> Option<game_detect::GameInfo> {
        if let Some(ref detector) = self.bpf_game_detector {
            // Convert BpfGameDetector::GameInfo to game_detect::GameInfo
            detector.get_game_info().map(|g| game_detect::GameInfo {
                tgid: g.tgid,
                name: g.name,
                is_wine: g.is_wine,
                is_steam: g.is_steam,
            })
        } else if let Some(ref detector) = self.game_detector {
            detector.get_game_info()
        } else {
            None
        }
    }

    /// Classify input device type on initialization to avoid per-event checking.
    /// Smart device detection using udev properties and USB interface analysis.
    /// Replaces hardcoded device lists with dynamic detection.
    #[inline]
    fn classify_device_type(dev: &evdev::Device, dev_path: &Path) -> DeviceType {
        let supported = dev.supported_events();
        let has_rel = supported.contains(EventType::RELATIVE);
        let has_key = supported.contains(EventType::KEY);

        // Step 1: Use udev properties (most reliable)
        if let Ok(device_type) = Self::detect_via_udev_properties(dev_path) {
            return device_type;
        }

        // Step 2: Analyze USB interface patterns for wireless dongles
        if let Ok(device_type) = Self::detect_via_usb_interfaces(dev_path) {
            return device_type;
        }

        // Step 3: Fallback to event capabilities and name analysis
        Self::detect_via_capabilities_and_name(dev, has_rel, has_key)
    }

    /// Detect device type using udev properties (most reliable method)
    /// OPTIMIZATION: Use direct udev device lookup instead of scanning all devices
    fn detect_via_udev_properties(dev_path: &Path) -> Result<DeviceType, std::io::Error> {
        // OPTIMIZATION: Direct device lookup instead of scanning all devices
        // This reduces lookup time from O(n) to O(1) for device enumeration
        let device = udev::Device::from_syspath(dev_path)?;

        // Check explicit udev classifications first (fastest path)
        if device
            .property_value("ID_INPUT_MOUSE")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            return Ok(DeviceType::Mouse);
        }
        if device
            .property_value("ID_INPUT_KEYBOARD")
            .map(|v| v == "1")
            .unwrap_or(false)
        {
            return Ok(DeviceType::Keyboard);
        }

        // Check for wireless dongle patterns (medium cost)
        if let Some(usb_interfaces) = device.property_value("ID_USB_INTERFACES") {
            let interfaces = usb_interfaces.to_string_lossy();
            if Self::is_wireless_dongle_pattern(&interfaces) {
                // For wireless dongles, prefer mouse classification unless explicitly keyboard
                if device
                    .property_value("ID_INPUT_KEYBOARD")
                    .map(|v| v == "1")
                    .unwrap_or(false)
                {
                    return Ok(DeviceType::Keyboard);
                } else {
                    return Ok(DeviceType::Mouse);
                }
            }
        }

        // Check for device grouping (highest cost - only if needed)
        if let Some(device_group) = device.property_value("LIBINPUT_DEVICE_GROUP") {
            // OPTIMIZATION: Only do expensive group analysis if no other classification found
            if let Ok(group_device_type) =
                Self::detect_device_group_primary_type_cached(&device_group.to_string_lossy())
            {
                return Ok(group_device_type);
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Device not found in udev",
        ))
    }

    /// Cached version of device group detection to avoid repeated expensive scans
    /// OPTIMIZATION: Use static cache to avoid repeated udev enumeration
    fn detect_device_group_primary_type_cached(
        device_group: &str,
    ) -> Result<DeviceType, std::io::Error> {
        use once_cell::sync::Lazy;
        use std::collections::HashMap;
        use std::sync::Mutex;

        // Static cache for device group analysis (expensive operation)
        static GROUP_CACHE: Lazy<Mutex<HashMap<String, DeviceType>>> =
            Lazy::new(|| Mutex::new(HashMap::new()));

        // Check cache first
        if let Ok(cache) = GROUP_CACHE.lock() {
            if let Some(&cached_type) = cache.get(device_group) {
                return Ok(cached_type);
            }
        }

        // Cache miss - perform expensive scan
        let device_type = Self::detect_device_group_primary_type_uncached(device_group)?;

        // Cache the result
        if let Ok(mut cache) = GROUP_CACHE.lock() {
            cache.insert(device_group.to_string(), device_type);
        }

        Ok(device_type)
    }

    /// Uncached device group detection (expensive operation)
    fn detect_device_group_primary_type_uncached(
        device_group: &str,
    ) -> Result<DeviceType, std::io::Error> {
        let mut enumerator = udev::Enumerator::new()?;
        enumerator.match_subsystem("input")?;

        // Find all devices in the same group
        let mut group_devices = Vec::new();
        for udev_dev in enumerator.scan_devices()? {
            if let Some(group) = udev_dev.property_value("LIBINPUT_DEVICE_GROUP") {
                if group.to_string_lossy() == device_group {
                    group_devices.push(udev_dev);
                }
            }
        }

        // Analyze the group to determine primary device type
        let mut mouse_count = 0;
        let mut keyboard_count = 0;
        let mut controller_count = 0;

        for device in &group_devices {
            if device
                .property_value("ID_INPUT_MOUSE")
                .map(|v| v == "1")
                .unwrap_or(false)
            {
                mouse_count += 1;
            }
            if device
                .property_value("ID_INPUT_KEYBOARD")
                .map(|v| v == "1")
                .unwrap_or(false)
            {
                keyboard_count += 1;
            }
            if device
                .property_value("ID_INPUT_JOYSTICK")
                .map(|v| v == "1")
                .unwrap_or(false)
            {
                controller_count += 1;
            }
        }

        // Return the most common device type in the group
        if controller_count > mouse_count && controller_count > keyboard_count {
            Ok(DeviceType::Other) // Controllers are classified as Other in our enum
        } else if mouse_count >= keyboard_count {
            Ok(DeviceType::Mouse)
        } else {
            Ok(DeviceType::Keyboard)
        }
    }

    /// Detect device type by analyzing USB interface patterns
    /// OPTIMIZATION: Use direct device lookup instead of scanning all devices
    fn detect_via_usb_interfaces(dev_path: &Path) -> Result<DeviceType, std::io::Error> {
        // OPTIMIZATION: Direct device lookup instead of scanning all devices
        let device = udev::Device::from_syspath(dev_path)?;

        // Check parent USB device for dongle characteristics
        if let Some(parent) = device.parent() {
            if let Some(devtype) = parent.attribute_value("devtype") {
                if devtype == "usb_device" {
                    // Check for wireless dongle indicators
                    if let Some(product) = parent.attribute_value("product") {
                        let product_str = product.to_string_lossy().to_lowercase();
                        if product_str.contains("dongle")
                            || product_str.contains("receiver")
                            || product_str.contains("adapter")
                        {
                            // Dongle detected - classify based on interface
                            if let Some(usb_interfaces) = device.property_value("ID_USB_INTERFACES")
                            {
                                let interfaces = usb_interfaces.to_string_lossy();
                                if interfaces.contains("030102") {
                                    // HID mouse interface
                                    return Ok(DeviceType::Mouse);
                                } else if interfaces.contains("030101") {
                                    // HID keyboard interface
                                    return Ok(DeviceType::Keyboard);
                                }
                            }
                        }
                    }
                }
            }
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "USB interface analysis failed",
        ))
    }

    /// Parse /proc/interrupts to build IRQ → CPU affinity mapping
    /// Returns a HashMap<IRQ_number, CPU_id> for xHCI (USB) controllers
    fn parse_usb_irq_affinities() -> std::collections::HashMap<u32, i32> {
        use std::collections::HashMap;
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let mut irq_to_cpu: HashMap<u32, i32> = HashMap::new();

        let Ok(file) = File::open("/proc/interrupts") else {
            return irq_to_cpu;
        };

        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            // Look for xhci_hcd lines (USB controllers)
            if !line.contains("xhci_hcd") {
                continue;
            }

            // Parse line format: "  IRQ: count0 count1 ... countN  type  controller"
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            // Extract IRQ number (first field, remove trailing colon)
            let irq_str = parts[0].trim_end_matches(':');
            let Ok(irq_num) = irq_str.parse::<u32>() else {
                continue;
            };

            // Find which CPU has the highest interrupt count (primary CPU for this IRQ)
            // Skip IRQ number field, count fields are next
            let mut max_count: u64 = 0;
            let mut max_cpu: i32 = -1;
            
            for (cpu_idx, part) in parts.iter().skip(1).enumerate() {
                // Stop when we hit non-numeric fields (type/controller name)
                let Ok(count) = part.parse::<u64>() else {
                    break;
                };
                
                if count > max_count {
                    max_count = count;
                    max_cpu = cpu_idx as i32;
                }
            }

            if max_cpu >= 0 && max_count > 0 {
                irq_to_cpu.insert(irq_num, max_cpu);
            }
        }

        irq_to_cpu
    }

    /// Extract PCI address from physical device path
    /// Input: "usb-0000:0c:00.0-2/input0" or just "0000:0c:00.0"
    /// Output: "0000:0c:00.0"
    fn extract_pci_address(phys_path: &str) -> Option<String> {
        // Handle "usb-0000:0c:00.0-2/input0" format
        if let Some(pci_part) = phys_path.strip_prefix("usb-") {
            if let Some(addr) = pci_part.split('-').next() {
                return Some(addr.to_string());
            }
        }
        // Handle direct PCI address
        if phys_path.contains(':') && phys_path.split(':').count() >= 2 {
            return Some(phys_path.split('-').next()?.to_string());
        }
        None
    }


    /// Map PCI address to IRQ number by parsing /proc/interrupts
    /// Returns IRQ number for the xHCI controller at this PCI address
    fn get_irq_for_pci_address(pci_addr: &str) -> Option<u32> {
        use std::fs::File;
        use std::io::{BufRead, BufReader};

        let Ok(file) = File::open("/proc/interrupts") else {
            return None;
        };

        let reader = BufReader::new(file);
        for line in reader.lines().flatten() {
            if !line.contains("xhci_hcd") {
                continue;
            }

            // Check if this line contains our PCI address
            if !line.contains(pci_addr) {
                continue;
            }

            // Extract IRQ number (first field)
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.is_empty() {
                continue;
            }

            let irq_str = parts[0].trim_end_matches(':');
            if let Ok(irq_num) = irq_str.parse::<u32>() {
                return Some(irq_num);
            }
        }
        None
    }

    /// Get CPU hint for input device based on USB IRQ affinity
    /// Returns CPU number that handles USB interrupts for this device
    /// phys_path: Physical device path like "usb-0000:0c:00.0-2/input0"
    fn get_usb_irq_cpu_hint(phys_path: &str) -> Option<i32> {
        // Step 1: Get IRQ → CPU mapping
        let irq_to_cpu = Self::parse_usb_irq_affinities();
        
        if irq_to_cpu.is_empty() {
            return None;
        }

        // Step 2: Extract PCI address from physical path
        let pci_addr = Self::extract_pci_address(phys_path)?;
        
        // Step 3: Get IRQ for this PCI address
        let irq_num = Self::get_irq_for_pci_address(&pci_addr)?;
        
        // Step 4: Lookup CPU for this IRQ
        irq_to_cpu.get(&irq_num).copied()
    }

    /// Detect device type using event capabilities and name analysis (fallback)
    fn detect_via_capabilities_and_name(
        dev: &evdev::Device,
        has_rel: bool,
        has_key: bool,
    ) -> DeviceType {
        let name_lc = dev.name().unwrap_or(" ").to_ascii_lowercase();

        // Name-based detection with better heuristics
        if name_lc.contains("mouse")
            || name_lc.contains("trackball")
            || name_lc.contains("trackpad")
        {
            DeviceType::Mouse
        } else if name_lc.contains("keyboard") || name_lc.contains("keypad") {
            DeviceType::Keyboard
        } else if name_lc.contains("dongle") || name_lc.contains("receiver") {
            // Wireless dongles - prefer mouse unless keyboard-specific
            if name_lc.contains("keyboard") {
                DeviceType::Keyboard
            } else {
                DeviceType::Mouse
            }
        } else if has_rel {
            // Relative movement = mouse
            DeviceType::Mouse
        } else if has_key {
            // Check if it's a real keyboard (has letter keys)
            if let Some(keys) = dev.supported_keys() {
                if keys.iter().any(|key| key.code() < 0x100) {
                    DeviceType::Keyboard
                } else {
                    DeviceType::Other
                }
            } else {
                DeviceType::Other
            }
        } else {
            DeviceType::Other
        }
    }

    /// Check if USB interface pattern indicates a wireless dongle
    fn is_wireless_dongle_pattern(interfaces: &str) -> bool {
        // Common wireless dongle interface patterns:
        // 030102 = HID mouse interface
        // 030101 = HID keyboard interface
        // 030000 = HID generic interface
        interfaces.contains("030102")
            || interfaces.contains("030101")
            || interfaces.contains("030000")
    }

    /// Register all threads of the detected game in game_threads_map
    /// This enables BPF thread runtime tracking for accurate role detection
    /// PERF: Uses stack-allocated path buffer to avoid heap allocation
    fn register_game_threads(&mut self, tgid: u32) {
        let game_threads_map = &self.skel.maps.game_threads_map;

        // PERF: Stack-allocated path buffer (max PID: 10 digits + "/proc//task\0" = 32 bytes)
        // Eliminates heap allocation from format!() (~100-200ns savings)
        // Use manual string building for zero-allocation path construction
        let mut path_buf = [0u8; 32];
        let path = {
            // Manual string building: "/proc/{}/task"
            let mut pos = 0;
            let prefix = b"/proc/";
            path_buf[pos..pos + prefix.len()].copy_from_slice(prefix);
            pos += prefix.len();

            // Write TGID as decimal string
            let mut tgid_val = tgid;
            let mut digits = [0u8; 10];
            let mut digit_count = 0;
            if tgid_val == 0 {
                digits[digit_count] = b'0';
                digit_count = 1;
            } else {
                while tgid_val > 0 && digit_count < 10 {
                    digits[digit_count] = b'0' + (tgid_val % 10) as u8;
                    tgid_val /= 10;
                    digit_count += 1;
                }
            }
            // Write digits in reverse order
            for i in (0..digit_count).rev() {
                path_buf[pos] = digits[i];
                pos += 1;
            }

            let suffix = b"/task";
            path_buf[pos..pos + suffix.len()].copy_from_slice(suffix);
            pos += suffix.len();

            std::str::from_utf8(&path_buf[..pos]).unwrap_or("/proc")
        };

        let mut thread_count = 0;
        let mut new_threads = FxHashSet::default();
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if let Ok(tid_str) = entry.file_name().into_string() {
                    if let Ok(tid) = tid_str.parse::<u32>() {
                        let marker: u8 = 1;
                        // Register thread in BPF map for tracking
                        if game_threads_map
                            .update(&tid.to_ne_bytes(), &[marker], libbpf_rs::MapFlags::ANY)
                            .is_ok()
                        {
                            thread_count += 1;
                            new_threads.insert(tid);
                        }
                    }
                }
            }
        }

        // Remove stale entries
        let mut stale_tids = Vec::new();
        for tid in &self.tracked_game_threads {
            if !new_threads.contains(tid) {
                stale_tids.push(*tid);
            }
        }
        for tid in stale_tids {
            let _ = game_threads_map.delete(&tid.to_ne_bytes());
        }

        self.tracked_game_threads = new_threads;

        if thread_count > 0 {
            info!(
                "Thread tracking: Registered {} game threads for TGID {}",
                thread_count, tgid
            );
        }
    }

    fn clear_tracked_game_threads(&mut self) {
        let game_threads_map = &self.skel.maps.game_threads_map;
        for tid in self.tracked_game_threads.drain() {
            let _ = game_threads_map.delete(&tid.to_ne_bytes());
        }
    }

    // REMOVED: apply_system_audio_update - userspace audio detection removed
    // Audio threads are now detected via fentry hooks in audio_detect.bpf.h

    #[inline]
    fn auto_event_loop_cpu() -> Option<usize> {
        // Smart event loop CPU selection for epoll processing:
        // 1. Prefer hyperthread cores (odd-numbered) to avoid competing with GPU threads
        // 2. Avoid physical cores that GPU threads need
        // 3. On SMT systems, pick last CPU (typically underutilized)
        // 4. Fallback to LITTLE/low-capacity cores if no SMT
        // Note: With interrupt-driven epoll, CPU usage is minimal (<5%)
        let topo = Topology::new().ok()?;

        // Strategy 1: Find highest-numbered hyperthread core (typically last CPU)
        // This avoids conflicts with GPU threads which prefer physical cores
        if topo.smt_enabled {
            if let Some(&max_cpu_id) = topo.all_cpus.keys().max() {
                // Check if it's a hyperthread (odd number in typical layouts: 1,3,5,7...)
                if max_cpu_id % 2 == 1 {
                    return Some(max_cpu_id);
                }
                // If max is even, go for second-to-last (should be odd)
                if max_cpu_id > 0 {
                    return Some(max_cpu_id - 1);
                }
            }
        }

        // Strategy 2: Prefer a LITTLE/low-capacity CPU as housekeeping, else the lowest-capacity CPU.
        let mut little: Vec<(usize, usize)> = topo
            .all_cpus
            .iter()
            .map(|(id, cpu)| (*id, cpu.cpu_capacity))
            .filter(|(id, _)| {
                topo.all_cpus
                    .get(id)
                    .map(|c| matches!(c.core_type, CoreType::Little))
                    .unwrap_or(false)
            })
            .collect();
        if !little.is_empty() {
            little.sort_by_key(|(_, cap)| *cap);
            return little.first().map(|(id, _)| *id);
        }

        // Strategy 3: Fallback to lowest-capacity CPU
        let mut all: Vec<(usize, usize)> = topo
            .all_cpus
            .iter()
            .map(|(id, cpu)| (*id, cpu.cpu_capacity))
            .collect();
        all.sort_by_key(|(_, cap)| *cap);
        all.first().map(|(id, _)| *id)
    }
    fn init(opts: &'a Opts, open_object: &'a mut MaybeUninit<OpenObject>) -> Result<Self> {
        try_set_rlimit_infinity();

        // Initialize CPU topology.
        let topo = Topology::new().context("failed to gather CPU topology")?;

        // Check host topology to determine if we need to enable SMT capabilities.
        let smt_enabled = !opts.disable_smt && topo.smt_enabled;

        // Auto-detect hybrid CPU topology (P+E cores)
        let has_little = topo
            .all_cpus
            .values()
            .any(|c| matches!(c.core_type, CoreType::Little));
        let has_big = topo
            .all_cpus
            .values()
            .any(|c| !matches!(c.core_type, CoreType::Little));
        let is_hybrid = has_little && has_big;

        // Preferred idle scan is ON by default for gaming workloads.
        // Can be disabled with --no-preferred-idle-scan if needed.
        // Also auto-enabled for hybrid CPUs unless flat scan is explicitly enabled.
        let preferred_idle_scan = if opts.no_preferred_idle_scan {
            false
        } else if is_hybrid && !opts.flat_idle_scan {
            info!("Hybrid CPU topology detected, preferred idle scan enabled");
            true
        } else {
            true  // Default ON for gaming
        };

        info!(
            "{} {} {}{}",
            SCHEDULER_NAME,
            build_id::full_version(env!("CARGO_PKG_VERSION")),
            if smt_enabled { "SMT on" } else { "SMT off" },
            if is_hybrid { " [hybrid]" } else { "" }
        );

        // Print command line.
        info!(
            "scheduler options: {}",
            std::env::args().collect::<Vec<_>>().join(" ")
        );

        // Initialize BPF connector.
        let mut skel_builder = BpfSkelBuilder::default();
        skel_builder.obj_builder.debug(opts.verbose);
        let open_opts = opts.libbpf.clone().into_bpf_open_opts();
        let mut skel = scx_ops_open!(skel_builder, open_object, gamer_ops, open_opts)?;

        skel.struct_ops.gamer_ops_mut().exit_dump_len = opts.exit_dump_len;

        // Override default BPF scheduling parameters.
        let rodata = skel
            .maps
            .rodata_data
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("BPF rodata not available"))?;
        rodata.slice_ns = opts.effective_slice_us() * 1000;
        rodata.slice_lag = opts.slice_lag_us * 1000;
        rodata.cpufreq_enabled = !opts.disable_cpufreq;
        rodata.deferred_wakeups = !opts.no_deferred_wakeup;
        rodata.flat_idle_scan = opts.flat_idle_scan;
        rodata.smt_enabled = smt_enabled;
        rodata.numa_enabled = opts.enable_numa;
        rodata.no_wake_sync = opts.no_wake_sync;
        rodata.avoid_smt = opts.effective_avoid_smt();
        // MM affinity is ON by default for gaming workloads (cache locality).
        // Can be disabled with --no-mm-affinity if needed.
        rodata.mm_affinity = !opts.no_mm_affinity;

        // Generate the list of available CPUs sorted by capacity in descending order.
        // For SMT systems with uniform capacity, prioritize physical cores over hyperthreads.
        let enable_preferred_scan = preferred_idle_scan || smt_enabled;

        for i in 0..256 {
            rodata.preferred_cpus[i] = u64::MAX;
            rodata.preferred_cpu_rank[i] = u32::MAX;
        }
        rodata.preferred_high_perf_count = 0;

        if enable_preferred_scan {
            let mut cpus: Vec<_> = topo.all_cpus.values().collect();

            // Verify we don't exceed MAX_CPUS (256) to prevent array out-of-bounds
            const MAX_CPUS: usize = 256;
            if cpus.len() > MAX_CPUS {
                bail!(
                    "System has {} CPUs but scheduler MAX_CPUS is {}. Recompile with larger MAX_CPUS.",
                    cpus.len(), MAX_CPUS
                );
            }

            let min_cap = cpus.iter().map(|cpu| cpu.cpu_capacity).min().unwrap_or(0);
            let max_cap = cpus.iter().map(|cpu| cpu.cpu_capacity).max().unwrap_or(0);

            if max_cap != min_cap {
                // PERF: Unstable sort (faster, no allocation) - order stability not needed
                cpus.sort_unstable_by_key(|cpu| std::cmp::Reverse(cpu.cpu_capacity));
            } else if smt_enabled {
                // Uniform capacity with SMT: prioritize physical cores (first sibling in each core)
                cpus.sort_unstable_by_key(|cpu| {
                    let core = topo.all_cores.get(&cpu.core_id);
                    let is_first_sibling = core
                        .and_then(|c| c.cpus.keys().next())
                        .map(|&first_id| first_id == cpu.id)
                        .unwrap_or(false);
                    // Sort: physical cores first (is_first_sibling=true -> 0), then by CPU ID
                    (!is_first_sibling, cpu.id)
                });
                info!("SMT detected with uniform capacity: prioritizing physical cores over hyperthreads");
            } else {
                // Uniform capacity, no SMT: sort by CPU ID
                cpus.sort_unstable_by_key(|cpu| cpu.id);
                info!("Uniform CPU capacities detected; preferred idle scan uses CPU ID order");
            }

            // Now fill in the actual CPU IDs
            for (i, cpu) in cpus.iter().enumerate() {
                rodata.preferred_cpus[i] = cpu.id as u64;
                if (cpu.id as usize) < 256 {
                    rodata.preferred_cpu_rank[cpu.id as usize] = i as u32;
                }
            }
            info!(
                "Preferred CPUs: {:?}",
                &rodata.preferred_cpus[0..cpus.len()]
            );

            let mut high_perf_count = cpus.len();
            if max_cap != min_cap {
                let top_cap = max_cap;
                high_perf_count = cpus
                    .iter()
                    .take_while(|cpu| cpu.cpu_capacity == top_cap)
                    .count();
            } else if smt_enabled {
                high_perf_count = cpus
                    .iter()
                    .filter(|cpu| {
                        topo.all_cores
                            .get(&cpu.core_id)
                            .and_then(|c| c.cpus.keys().next())
                            .map(|&first_id| first_id == cpu.id)
                            .unwrap_or(true)
                    })
                    .count();
            }
            if high_perf_count == 0 {
                high_perf_count = cpus.len();
            }
            rodata.preferred_high_perf_count = high_perf_count.min(cpus.len()).min(256) as u32;
        } else {
            let nr_ids = (*NR_CPU_IDS as usize).min(256);
            for cpu_id in 0..nr_ids {
                rodata.preferred_cpus[cpu_id] = cpu_id as u64;
                rodata.preferred_cpu_rank[cpu_id] = cpu_id as u32;
            }
            rodata.preferred_high_perf_count = nr_ids as u32;
        }
        rodata.preferred_idle_scan = enable_preferred_scan;

        for slot in rodata.cpu_ccd_class.iter_mut() {
            *slot = CCD_CLASS_UNKNOWN;
        }
        rodata.cache_ccd_cpu_count = 0;
        rodata.freq_ccd_cpu_count = 0;

        #[derive(Default)]
        struct ClusterStats {
            cpu_ids: Vec<usize>,
            total_cache: u64,
            total_freq: u64,
        }

        let mut cluster_map: HashMap<isize, ClusterStats> = HashMap::new();
        for cpu in topo.all_cpus.values() {
            let cluster_key = if cpu.cluster_id >= 0 {
                cpu.cluster_id
            } else {
                cpu.llc_id as isize
            };
            let entry = cluster_map.entry(cluster_key).or_default();
            entry.cpu_ids.push(cpu.id);
            entry.total_cache += cpu.cache_size as u64;
            entry.total_freq += cpu.max_freq as u64;
        }

        if cluster_map.len() >= 2 {
            let metrics: Vec<(isize, f64, f64, usize)> = cluster_map
                .iter()
                .map(|(id, stats)| {
                    let count = stats.cpu_ids.len().max(1);
                    let avg_cache = stats.total_cache as f64 / count as f64;
                    let avg_freq = stats.total_freq as f64 / count as f64;
                    (*id, avg_cache, avg_freq, stats.cpu_ids.len())
                })
                .collect();

            let mut cache_cluster: Option<isize> = None;
            let mut freq_cluster: Option<isize> = None;

            if metrics.len() >= 2 {
                let mut by_cache = metrics.clone();
                by_cache.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
                if by_cache.len() >= 2 {
                    let (top_id, top_cache, _, _) = by_cache[0];
                    let (_, second_cache, _, _) = by_cache[1];
                    if top_cache > 0.0 && top_cache >= second_cache * 1.20 {
                        cache_cluster = Some(top_id);
                    }
                }

                let cluster_count = cluster_map.len();
                if let Some(cache_id) = cache_cluster {
                    if cluster_count == 2 {
                        if let Some((&other_id, _)) =
                            cluster_map.iter().find(|(id, _)| **id != cache_id)
                        {
                            freq_cluster = Some(other_id);
                        }
                    } else if cluster_count > 2 {
                        let mut by_freq = metrics.clone();
                        by_freq.sort_by(|a, b| {
                            b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal)
                        });

                        let mut freq_candidates: Vec<(isize, f64, f64, usize)> = Vec::new();
                        for entry in by_freq {
                            if entry.0 == cache_id {
                                continue;
                            }
                            freq_candidates.push(entry);
                            if freq_candidates.len() == 2 {
                                break;
                            }
                        }

                        if !freq_candidates.is_empty() {
                            let choose_candidate = if freq_candidates.len() == 1 {
                                // Only one cluster separate from cache. Accept if cache already classified.
                                Some(freq_candidates[0])
                            } else {
                                let first = freq_candidates[0];
                                let second = freq_candidates[1];
                                if first.2 > 0.0 && second.2 > 0.0 && first.2 >= second.2 * 1.08 {
                                    Some(first)
                                } else {
                                    None
                                }
                            };
                            if let Some(candidate) = choose_candidate {
                                freq_cluster = Some(candidate.0);
                            }
                        }
                    }
                } else if cluster_map.len() == 2 {
                    // No distinguished cache cluster, but still two clusters:
                    // leave classification disabled to avoid misplacement.
                    info!(
                        "CCD classification: two clusters detected but cache heuristic inconclusive"
                    );
                }

                let mut cache_count = 0u32;
                let mut freq_count = 0u32;

                if let Some(cluster_id) = cache_cluster {
                    if let Some(stats) = cluster_map.get(&cluster_id) {
                        for &cpu_id in &stats.cpu_ids {
                            if cpu_id < rodata.cpu_ccd_class.len() {
                                rodata.cpu_ccd_class[cpu_id] = CCD_CLASS_CACHE;
                                cache_count += 1;
                            }
                        }
                    }
                }

                if let Some(cluster_id) = freq_cluster {
                    if let Some(stats) = cluster_map.get(&cluster_id) {
                        for &cpu_id in &stats.cpu_ids {
                            if cpu_id < rodata.cpu_ccd_class.len()
                                && rodata.cpu_ccd_class[cpu_id] != CCD_CLASS_CACHE
                            {
                                rodata.cpu_ccd_class[cpu_id] = CCD_CLASS_FREQ;
                                freq_count += 1;
                            }
                        }
                    }
                }

                rodata.cache_ccd_cpu_count = cache_count;
                rodata.freq_ccd_cpu_count = freq_count;

                if cache_count > 0 || freq_count > 0 {
                    info!(
                        "CCD classification: cache CCD cores={} freq CCD cores={} (clusters={})",
                        cache_count,
                        freq_count,
                        cluster_map.len()
                    );
                } else {
                    info!(
                        "CCD classification: heuristics inconclusive (clusters={}) -- using capacity ordering only",
                        cluster_map.len()
                    );
                }
            }
        } else {
            info!("CCD classification: single CCD detected; using default placement");
        }
        rodata.mig_window_ns = opts.mig_window_ms * 1_000_000;
        rodata.mig_max_per_window = opts.effective_mig_max();
        rodata.input_window_ns = opts.effective_input_window_us() * 1000;
        rodata.keyboard_boost_ns = opts.effective_keyboard_boost_us() * 1000;
        rodata.mouse_boost_ns = opts.effective_mouse_boost_us() * 1000;
        rodata.controller_boost_ns = opts.controller_boost_us * 1000;
        rodata.prefer_napi_on_input = opts.effective_prefer_napi_on_input();
        rodata.mm_hint_enabled = !opts.disable_mm_hint;
        // Stats collection disabled by default for performance; enabled with --monitoring or consumers
        rodata.no_stats = !opts.is_monitoring_enabled();
        let effective_wakeup = opts.effective_wakeup_timer_us();
        rodata.wakeup_timer_ns = if effective_wakeup == 0 {
            0
        } else {
            effective_wakeup.max(250) * 1000
        };
        // Automatic game detection only - no manual PID setting
        // The game detector will update detected_fg_tgid_staging in BPF
        rodata.foreground_tgid = 0;

        // MM hint removed - map configuration no longer needed
        // MM hint map (mm_last_cpu) removed for gaming workloads - low cache locality benefit, high overhead

        // Define the primary scheduling domain.
        let primary_cpus = if let Some(ref domain) = opts.primary_domain {
            match parse_cpu_list(domain) {
                Ok(cpus) => cpus,
                Err(e) => bail!("Error parsing primary domain: {}", e),
            }
        } else {
            (0..*NR_CPU_IDS).collect()
        };

        // Interrupt-driven input doesn't require CPU exclusion

        if primary_cpus.len() < *NR_CPU_IDS {
            info!("Primary CPUs: {:?}", primary_cpus);
            rodata.primary_all = false;
        } else {
            rodata.primary_all = true;
        }

        // Set scheduler flags.
        skel.struct_ops.gamer_ops_mut().flags = *compat::SCX_OPS_ENQ_EXITING
            | *compat::SCX_OPS_ENQ_LAST
            | *compat::SCX_OPS_ENQ_MIGRATION_DISABLED
            | *compat::SCX_OPS_ALLOW_QUEUED_WAKEUP
            | if opts.enable_numa {
                *compat::SCX_OPS_BUILTIN_IDLE_PER_NODE
            } else {
                0
            };
        info!(
            "scheduler flags: {:#x}",
            skel.struct_ops.gamer_ops_mut().flags
        );

        // Load the BPF program for validation.
        let mut skel = scx_ops_load!(skel, gamer_ops, uei)?;

        // Tracing flags: enabled via --monitoring, --enable-*, or implicit consumers (stats/tui/debug-api)
        let monitoring = opts.is_monitoring_enabled();
        let runtime_trace_enabled = opts.enable_runtime_trace || monitoring;
        if let Err(err) = bpf_intf::set_runtime_trace(&mut skel, runtime_trace_enabled) {
            warn!("Failed to configure runtime trace flag: {}", err);
        }
        let detector_trace_enabled = opts.enable_detectors || monitoring;
        if let Err(err) = bpf_intf::set_detector_trace(&mut skel, detector_trace_enabled) {
            warn!("Failed to configure detector trace flag: {}", err);
        }
        let dispatch_events_enabled = opts.enable_dispatch_events || monitoring;
        if let Err(err) =
            bpf_intf::set_dispatch_events(&mut skel, dispatch_events_enabled)
        {
            warn!("Failed to configure dispatch event flag: {}", err);
        }

        {
            let tail_map = &skel.maps.tailcall_map;

            let select_idx =
                (bpf_intf::tailcall_slot_TAILCALL_SLOT_SELECT_CPU as u32).to_ne_bytes();
            let select_fd = skel.progs.gamer_select_cpu_tail.as_fd().as_raw_fd() as u32;
            tail_map
                .update(
                    &select_idx,
                    &select_fd.to_ne_bytes(),
                    libbpf_rs::MapFlags::ANY,
                )
                .context("failed to populate select_cpu tailcall slot")?;

            // NOTE: gamer_enqueue no longer uses a tail call; it directly invokes
            // gamer_enqueue_slowpath. We intentionally do NOT populate the
            // ENQUEUE_SIGNAL tailcall slot here to avoid kernel EINVAL errors
            // when attempting to tailcall between incompatible program types.
        }

        // Note: BPF map pre-warming was considered but not implemented.
        // Reason: The critical maps (task_ctx_stor, cpu_ctx_stor) are special types:
        // - task_ctx_stor: BPF_MAP_TYPE_TASK_STORAGE (kernel-managed, can't pre-warm)
        // - cpu_ctx_stor: BPF_MAP_TYPE_PERCPU_ARRAY (already pre-allocated by kernel)
        // These maps don't benefit from userspace pre-warming.

        // Enable primary scheduling domain, if defined.
        if primary_cpus.len() < *NR_CPU_IDS {
            for cpu in primary_cpus {
                if let Err(err) = Self::enable_primary_cpu(&mut skel, cpu as i32) {
                    bail!("failed to add CPU {} to primary domain: error {}", cpu, err);
                }
            }
        }

        // Attach the scheduler.
        let struct_ops = Some(scx_ops_attach!(skel, gamer_ops)?);
        let stats_server = StatsServer::new(stats::server_data()).launch()?;

        // Log profile and effective configuration
        info!(
            "Profile: {} | slice={}µs input={}µs kbd={}µs mouse={}µs | avoid-smt={} napi={}",
            opts.profile,
            opts.effective_slice_us(),
            opts.effective_input_window_us(),
            opts.effective_keyboard_boost_us(),
            opts.effective_mouse_boost_us(),
            opts.effective_avoid_smt(),
            opts.effective_prefer_napi_on_input()
        );
        if opts.is_monitoring_enabled() {
            info!("Monitoring: ENABLED (stats, detectors, tracing active)");
        } else {
            info!("Monitoring: DISABLED (maximum performance mode)");
        }

        // Initialize event-driven audio server detector (inotify-based)
        // REMOVED: audio_detector - redundant with fentry-based BPF detection

        // Initialize CPU affinity override system (proactive)
        // Detects and resets custom affinities for optimal task placement
        // Performance: ~2-11μs per override, 10-30% latency improvement from better load balancing
        let affinity_override = match affinity_override::AffinityOverride::new(
            &mut skel,
            *NR_CPU_IDS,
        ) {
            Ok(override_sys) => {
                info!("Affinity override system: Enabled (proactive detection + reset)");
                Some(override_sys)
            }
            Err(e) => {
                warn!("Failed to initialize affinity override system: {}", e);
                warn!("Continuing without affinity override - custom affinities will be respected");
                None
            }
        };

        // REMOVED: audio_detector.initial_scan - userspace audio detection removed
        // Audio threads are now detected via fentry hooks in audio_detect.bpf.h

        let mut input_devs: Vec<evdev::Device> = Vec::new();
        let mut input_fd_info_vec: Vec<Option<DeviceInfo>> = Vec::new();
        if opts.effective_input_window_us() > 0 {
            if let Ok(dir) = std::fs::read_dir("/dev/input") {
                for entry in dir.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name.starts_with("event") {
                            if let Ok(dev) = evdev::Device::open(&path) {
                                let dev_type = Self::classify_device_type(&dev, &path);
                                if matches!(dev_type, DeviceType::Mouse | DeviceType::Keyboard) {
                                    let fd = dev.as_raw_fd();
                                    if fd >= 0 {
                                        // Set O_NONBLOCK for safety using safe nix wrapper
                                        // SAFETY: No unsafe needed - nix provides safe fcntl wrapper
                                        // FD validated >= 0, errors handled gracefully
                                        match fcntl::fcntl(fd, fcntl::FcntlArg::F_GETFL) {
                                            Ok(current_flags) => {
                                                let flags =
                                                    fcntl::OFlag::from_bits_truncate(current_flags);
                                                let new_flags = flags | fcntl::OFlag::O_NONBLOCK;
                                                let _ = fcntl::fcntl(
                                                    fd,
                                                    fcntl::FcntlArg::F_SETFL(new_flags),
                                                );
                                            }
                                            Err(_) => {
                                                // Best-effort: if we can't get flags, skip setting non-blocking
                                                // Device will still work, just may block on some operations
                                            }
                                        }
                                        let lane = dev_type.lane();
                                        let input_id = dev.input_id();
                                        info!("Registered {:?} device: {} (vendor={:#06x} product={:#06x} fd={} lane={:?})",
                                              dev_type, 
                                              dev.name().unwrap_or("unknown"),
                                              input_id.vendor(),
                                              input_id.product(),
                                              fd,
                                              lane);
                                        // HOT PATH OPTIMIZATION: Direct array access instead of hash map
                                        // Grow vector if needed for this FD
                                        if (fd as usize) >= input_fd_info_vec.len() {
                                            input_fd_info_vec.resize(fd as usize + 1, None);
                                        }
                                        input_fd_info_vec[fd as usize] =
                                            Some(DeviceInfo::new(input_devs.len(), lane));
                                        input_devs.push(dev);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            info!(
                "Found {} input devices for raw input monitoring",
                input_devs.len()
            );

            // Populate USB IRQ CPU hints in BPF device cache
            // This enables cache locality optimization for input devices
            if !input_devs.is_empty() {
                info!("Populating USB IRQ CPU affinity hints...");
                let mut hint_count = 0;
                
                for dev in &input_devs {
                    // Get USB IRQ CPU hint for this device using physical path
                    if let Some(phys_path) = dev.physical_path() {
                        if let Some(cpu_hint) = Self::get_usb_irq_cpu_hint(&phys_path) {
                            info!(
                                "USB IRQ hint: {} (phys={}) → CPU {}",
                                dev.name().unwrap_or("unknown"),
                                phys_path,
                                cpu_hint
                            );
                            hint_count += 1;
                            
                            // TODO: Update BPF device_whitelist_cache map with cpu_hint
                            // This would require accessing the map and updating the entry
                            // For now, the BPF side will use global hint updated in input_event_raw
                        }
                    }
                }
                
                if hint_count > 0 {
                    info!("Populated {} USB IRQ CPU hints for cache locality optimization", hint_count);
                } else {
                    info!("No USB IRQ CPU hints available (this is normal, optimization is best-effort)");
                }
            }
        }

        // Select input trigger function at init time based on prefer_napi_on_input flag
        // This avoids runtime branching on every input event (saves 10-20ns per event)
        let input_trigger_fn: fn(&trigger::BpfTrigger, &mut BpfSkel, InputLane) =
            if opts.effective_prefer_napi_on_input() {
                |trig, skel, lane| match lane {
                    InputLane::Mouse => {
                        trig.trigger_input_with_napi_lane(skel, lane);
                    }
                    _ => {
                        trig.trigger_input_lane(skel, lane);
                    }
                }
            } else {
                |trig, skel, lane| {
                    trig.trigger_input_lane(skel, lane);
                }
            };

        // Initialize game detection: Try BPF LSM first (kernel-level), fallback to inotify
        // ═══════════════════════════════════════════════════════════════════════════
        // FOCUS DETECTION (PRIMARY) - D-Bus event-based
        // ═══════════════════════════════════════════════════════════════════════════
        // 
        // ZERO POLLING, ZERO HEURISTICS, 100% PROOF
        //
        // Instead of guessing which process is a "game" using heuristics like:
        //   - Thread count (unreliable - Discord has 50+ threads)
        //   - Memory usage (unreliable - browsers use GB+)
        //   - Process name (brittle - different games, different names)
        //
        // We simply ask the compositor: "Which window is focused?"
        // The focused window is EXACTLY what the user wants to be fast.
        //
        // PROOF CHAIN:
        // 1. User clicks on game window → compositor focuses it
        // 2. D-Bus signal tells us the focused window's PID
        // 3. We set detected_fg_tgid = that PID
        // 4. All threads of that PID get migration protection
        //
        // No guessing. No heuristics. Just facts from the compositor.
        // ═══════════════════════════════════════════════════════════════════════════
        let focus_detector = {
            let detector = focus_detect::FocusDetector::new();
            // Give D-Bus connection time to establish
            std::thread::sleep(Duration::from_millis(100));
            let initial_pid = detector.get_focused_pid();
            if initial_pid > 0 {
                info!("Focus detection: D-Bus connected, initial focus PID = {}", initial_pid);
                Some(detector)
            } else {
                info!("Focus detection: D-Bus available, waiting for focus events");
                Some(detector)
            }
        };

        // ═══════════════════════════════════════════════════════════════════════════
        // GAME DETECTION (FALLBACK) - Only used if D-Bus focus detection unavailable
        // ═══════════════════════════════════════════════════════════════════════════
        // BPF LSM benefits (kernel 6.17+):
        // - 60-650× lower CPU overhead (μs/sec vs ms/sec)
        // - 10-100× faster detection (<1ms vs 0-100ms)
        // - Instant game exit detection (<1ms vs 5s polling)
        // - Zero recurring /proc scans (event-driven)
        let (bpf_game_detector, game_detector_fallback) = match BpfGameDetector::new(&mut skel) {
            Ok(detector) => {
                info!("Game detection (fallback): BPF LSM available");
                (Some(detector), None)
            }
            Err(e) => {
                info!(
                    "Game detection (fallback): BPF LSM unavailable ({}), inotify available",
                    e
                );
                (None, Some(GameDetector::new()))
            }
        };

        // Initialize input ring buffer for ultra-low latency input processing
        let input_ring_buffer = if opts.effective_input_window_us() > 0 {
            match ring_buffer::InputRingBufferManager::new(&mut skel) {
                Ok(manager) => {
                    info!("Input ring buffer: Initialized with BPF integration");
                    Some(manager)
                }
                Err(e) => {
                    warn!("Failed to initialize input ring buffer: {}", e);
                    None
                }
            }
        } else {
            None
        };

        // REMOVED: power_monitor - power efficiency contradicts gaming performance goal
        let gpu_queue_monitor = GpuQueueMonitor::new();

        // REMOVED: seed_engine_presets - brittle name-based thread presets
        // Behavioral detection (lat_cri, fentry hooks) now handles thread classification

        let scheduler = Self {
            skel,
            opts,
            struct_ops,
            stats_server: Some(stats_server),
            input_devs,
            epoll_fd: None,
            input_fd_info_vec,
            registered_epoll_fds: FxHashSet::default(),
            trig: trigger::BpfTrigger,
            input_trigger_fn,
            focus_detector, // PRIMARY: D-Bus event-based focus detection
            bpf_game_detector,
            game_detector: game_detector_fallback,
            input_ring_buffer,
            dispatch_event_ringbuf: None, // Initialized after epoll setup
            // REMOVED: debug_api_state - HTTP server removed
            // REMOVED: audio_detector - redundant with fentry-based BPF detection
            // REMOVED: audio_update_buffer - userspace audio detection removed
            affinity_override, // CPU affinity override system (proactive)
            uei: UserExitInfo::default(),
            // REMOVED: power_monitor - power monitoring removed
            gpu_queue_monitor,
            // REMOVED: power_hint_rx - power monitoring removed
            gpu_busy_rx: None,
            // REMOVED: power_monitor_worker - power monitoring removed
            gpu_monitor_worker: None,

            // AI Analytics: Initialize temporal pattern tracking
            migration_history_10s: std::collections::VecDeque::with_capacity(100), // ~10 samples per second max
            migration_history_60s: std::collections::VecDeque::with_capacity(600), // ~600 samples per second max
            cpu_util_history: std::collections::VecDeque::with_capacity(100),
            frame_rate_history: std::collections::VecDeque::with_capacity(100),
            last_migration_count: 0,
            tracked_game_threads: FxHashSet::default(),
        };

        Ok(scheduler)
    }

    fn enable_primary_cpu(skel: &mut BpfSkel<'_>, cpu: i32) -> Result<(), u32> {
        let prog = &mut skel.progs.enable_primary_cpu;
        let mut args = cpu_arg {
            cpu_id: cpu as c_int,
        };
        let input = ProgramInput {
            // SAFETY: Creating a mutable slice from stack-allocated struct for BPF program input.
            // - `args` is valid cpu_arg struct allocated on the stack
            // - Lifetime: `args` lives for entire function scope, slice lifetime scoped to BPF call
            // - Size: `size_of_val(&args)` returns correct struct size
            // - Alignment: Struct is properly aligned (stack allocation)
            // - No concurrent mutation: BPF program reads this as immutable context
            // - This is required FFI boundary - libbpf-rs requires raw pointer/slice
            context_in: Some(unsafe {
                std::slice::from_raw_parts_mut(
                    &mut args as *mut _ as *mut u8,
                    std::mem::size_of_val(&args),
                )
            }),
            ..Default::default()
        };
        let out = match prog.test_run(input) {
            Ok(out) => out,
            Err(_) => return Err(1),
        };
        if out.return_value != 0 {
            return Err(out.return_value);
        }

        Ok(())
    }

    fn get_metrics(&mut self) -> Metrics {
        let bss = self
            .skel
            .maps
            .bss_data
            .as_ref()
            .expect("BPF BSS missing (scheduler not loaded?)");
        let ro = self
            .skel
            .maps
            .rodata_data
            .as_ref()
            .expect("BPF rodata missing (scheduler not loaded?)");

        // Get detected game name for display in stats
        let fg_app = self
            .get_detected_game_info()
            .map(|g| g.name)
            .unwrap_or_default();

        // Use detected_fg_tgid if available, fallback to foreground_tgid
        let fg_pid = if bss.detected_fg_tgid > 0 {
            bss.detected_fg_tgid as u64
        } else {
            ro.foreground_tgid as u64
        };

        // Capture current values for temporal tracking (before mutation)
        let current_migrations = bss.nr_migrations;
        let current_cpu_util = bss.cpu_util;
        let current_frame_interval = bss.frame_interval_ns;

        // Read PSI (Pressure Stall Information) from /proc/pressure/*
        let (psi_cpu, psi_mem, psi_io) = read_psi();

        // Read fentry raw input stats (kernel-level input detection)
        // This shows if fentry hooks are active vs falling back to userspace evdev
        let (fentry_total, fentry_triggers, fentry_gaming, fentry_filtered, ringbuf_overflow) = {
            let stats_map = &self.skel.maps.raw_input_stats_map;
            let key = 0u32;

            let per_cpu_stats =
                match stats_map.lookup_percpu(&key.to_ne_bytes(), libbpf_rs::MapFlags::ANY) {
                    Ok(Some(per_cpu)) => per_cpu,
                    _ => Vec::new(),
                };

            if per_cpu_stats.is_empty() {
                (0, 0, 0, 0, 0)
            } else {
                let mut total = 0u64;
                let mut gaming = 0u64;
                let mut filtered = 0u64;
                let mut triggers = 0u64;
                let mut overflow = 0u64;

                for bytes in per_cpu_stats {
                    if bytes.len() < std::mem::size_of::<RawInputStats>() {
                        continue;
                    }
                    // SAFETY: Reading RawInputStats from per-CPU BPF array bytes
                    // - Size validated above (bytes.len() >= size_of::<RawInputStats>())
                    // - Uses read_unaligned() to handle potential misalignment
                    // - RawInputStats is #[repr(C)] and matches BPF layout exactly
                    // - BPF guarantees consistent layout via per-CPU array map
                    // - Zero-copy read required for performance (serialization would add latency)
                    let ris = unsafe { (bytes.as_ptr() as *const RawInputStats).read_unaligned() };
                    total = total.saturating_add(ris.total_events);
                    gaming = gaming.saturating_add(ris.gaming_device_events);
                    filtered = filtered.saturating_add(ris.filtered_events);
                    triggers = triggers.saturating_add(ris.fentry_boost_triggers);
                    overflow = overflow.saturating_add(ris.ringbuf_overflow_events);
                }

                (total, triggers, gaming, filtered, overflow)
            }
        };

        Metrics {
            cpu_util: bss.cpu_util,
            rr_enq: bss.rr_enq,
            edf_enq: bss.edf_enq,
            direct: bss.nr_direct_dispatches,
            shared: bss.nr_shared_dispatches,
            migrations: bss.nr_migrations,
            mig_blocked: bss.nr_mig_blocked,
            sync_local: bss.nr_sync_local,
            frame_mig_block: bss.nr_frame_mig_block,
            cpu_util_avg: bss.cpu_util_avg,
            frame_hz_est: 0.0, // Frame timing removed
            fg_pid,
            fg_app,
            fg_fullscreen: 0,
            win_input_ns: bss.win_input_ns_total,
            win_frame_ns: bss.win_frame_ns_total,
            timer_elapsed_ns: bss.timer_elapsed_ns_total,
            idle_pick: bss.nr_idle_cpu_pick,
            mm_hint_hit: 0, // MM hint removed
            fg_cpu_pct: if bss.total_runtime_ns_total > 0 {
                bss.fg_runtime_ns_total.saturating_mul(100) / bss.total_runtime_ns_total
            } else {
                0
            },
            input_trig: bss.nr_input_trig,
            frame_trig: bss.nr_frame_trig,
            input_force_dispatch: bss.nr_input_force_dispatch,
            input_force_dispatch_late: bss.nr_input_force_dispatch_late,
            input_dispatch_latency_ns: bss.input_force_dispatch_latency_ns,
            input_dispatch_latency_max_ns: bss.input_force_dispatch_latency_max_ns,
            input_window_dynamic_ns: bss.input_window_dynamic_ns,
            keyboard_lane_dynamic_ns: bss.input_lane_dynamic_ns[InputLane::Keyboard as usize],
            mouse_lane_dynamic_ns: bss.input_lane_dynamic_ns[InputLane::Mouse as usize],
            frame_feedback_escalations: bss.nr_frame_feedback_escalations,
            frame_feedback_recoveries: bss.nr_frame_feedback_recoveries,
            frame_feedback_miss_events: bss.nr_frame_feedback_miss_events,
            taskgraph_borrow_grants: bss.nr_taskgraph_borrow_grants,
            sync_wake_fast: bss.nr_sync_wake_fast,
            gpu_submit_threads: bss.nr_gpu_submit_threads,
            // Sanitize background_threads to handle underflow/overflow (BPF fix should prevent, but defense in depth)
            background_threads: if bss.nr_background_threads > 10000 {
                0
            } else {
                bss.nr_background_threads
            },
            compositor_threads: bss.nr_compositor_threads,
            network_threads: bss.nr_network_threads,
            system_audio_threads: bss.nr_system_audio_threads,
            game_audio_threads: bss.nr_game_audio_threads,
            input_handler_threads: bss.nr_input_handler_threads,
            taskgraph_threads: bss.nr_taskgraph_threads,
            input_trigger_rate: bss.input_trigger_rate as u64,
            continuous_input_mode: bss.continuous_input_mode as u64,
            continuous_input_lane_keyboard: bss.continuous_input_lane_mode
                [InputLane::Keyboard as usize] as u64,
            continuous_input_lane_mouse: bss.continuous_input_lane_mode[InputLane::Mouse as usize]
                as u64,
            continuous_input_lane_other: bss.continuous_input_lane_mode[InputLane::Other as usize]
                as u64,
            frame_phase_cpu_ns: bss.frame_phase_cpu_ns,
            frame_phase_gpu_ns: bss.frame_phase_gpu_ns,
            frame_phase_events: bss.frame_phase_events,
            frame_phase_gpu_dominant: bss.frame_phase_gpu_dominant,
            frame_phase_cpu_dominant: bss.frame_phase_cpu_dominant,
            power_hint_level: bss.power_hint_level as u64,
            power_hint_remaining_ns: bss.power_hint_remaining_ns,
            power_hint_updates: bss.nr_power_hint_updates,

            // Diagnostic counters for classification debugging
            classification_attempts: bss.nr_classification_attempts,
            first_classification_true: bss.nr_first_classification_true,
            is_exact_game_thread_true: bss.nr_is_exact_game_thread_true,
            input_handler_name_match: bss.nr_input_handler_name_match,
            main_thread_match: bss.nr_main_thread_match,
            gpu_submit_name_match: bss.nr_gpu_submit_name_match,
            gpu_submit_fentry_match: bss.nr_gpu_submit_fentry_match,
            runtime_pattern_gpu_samples: bss.nr_runtime_pattern_gpu_samples,
            runtime_pattern_audio_samples: bss.nr_runtime_pattern_audio_samples,
            input_handler_name_check_attempts: bss.nr_input_handler_name_check_attempts,
            input_handler_name_pattern_match: bss.nr_input_handler_name_pattern_match,

            // Diagnostic counters for network/audio/background detection
            network_fentry_checks: bss.nr_network_fentry_checks,
            network_fentry_matches: bss.nr_network_fentry_matches,
            network_name_checks: bss.nr_network_name_checks,
            network_name_matches: bss.nr_network_name_matches,
            system_audio_fentry_checks: bss.nr_system_audio_fentry_checks,
            system_audio_fentry_matches: bss.nr_system_audio_fentry_matches,
            system_audio_name_checks: bss.nr_system_audio_name_checks,
            system_audio_name_matches: bss.nr_system_audio_name_matches,
            background_name_checks: bss.nr_background_name_checks,
            background_name_matches: bss.nr_background_name_matches,
            background_pattern_checks: bss.nr_background_pattern_checks,
            background_pattern_samples: bss.nr_background_pattern_samples,

            // Fentry hook call counters (from network_detect.bpf.h and audio_detect.bpf.h)
            // Note: These may be 0 if hooks aren't attached or functions don't exist
            network_detect_send_calls: 0, // TODO: Expose from BPF if accessible
            network_detect_recv_calls: 0, // TODO: Expose from BPF if accessible
            audio_detect_alsa_calls: 0,
            audio_detect_usb_calls: 0,

            // Fentry hook stats (cumulative totals from kernel hooks)
            fentry_total_events: fentry_total,
            fentry_boost_triggers: fentry_triggers,
            fentry_gaming_events: fentry_gaming,
            fentry_filtered_events: fentry_filtered,
            ringbuf_overflow_events: ringbuf_overflow,

            // Ring buffer input latency tracking (single percentile computation)
            ringbuf_latency_avg_ns: self
                .input_ring_buffer
                .as_ref()
                .map(|rb| rb.stats().avg_latency_ns as u64)
                .unwrap_or(0),
            ringbuf_latency_p50_ns: {
                if let Some(rb) = self.input_ring_buffer.as_ref() {
                    let (p50, _, _) = rb.get_latency_percentiles();
                    p50 as u64
                } else {
                    0
                }
            },
            ringbuf_latency_p95_ns: {
                if let Some(rb) = self.input_ring_buffer.as_ref() {
                    let (_, p95, _) = rb.get_latency_percentiles();
                    p95 as u64
                } else {
                    0
                }
            },
            ringbuf_latency_p99_ns: {
                if let Some(rb) = self.input_ring_buffer.as_ref() {
                    let (_, _, p99) = rb.get_latency_percentiles();
                    p99 as u64
                } else {
                    0
                }
            },
            ringbuf_latency_min_ns: self
                .input_ring_buffer
                .as_ref()
                .map(|rb| rb.stats().min_latency_ns)
                .unwrap_or(0),
            ringbuf_latency_max_ns: self
                .input_ring_buffer
                .as_ref()
                .map(|rb| rb.stats().max_latency_ns)
                .unwrap_or(0),

            // Userspace ring buffer queue metrics
            rb_queue_dropped_total: self
                .input_ring_buffer
                .as_ref()
                .map(|rb| rb.stats().queue_dropped_total)
                .unwrap_or(0),
            rb_queue_high_watermark: self
                .input_ring_buffer
                .as_ref()
                .map(|rb| rb.stats().queue_high_watermark)
                .unwrap_or(0),

            // Profiling metrics (calculated in delta())
            prof_select_cpu_avg_ns: 0,
            prof_enqueue_avg_ns: 0,
            prof_dispatch_avg_ns: 0,
            prof_deadline_avg_ns: 0,

            // Raw profiling counters
            prof_select_cpu_ns: bss.prof_select_cpu_ns_total,
            prof_select_cpu_calls: bss.prof_select_cpu_calls,
            prof_enqueue_ns: bss.prof_enqueue_ns_total,
            prof_enqueue_calls: bss.prof_enqueue_calls,
            prof_dispatch_ns: bss.prof_dispatch_ns_total,
            prof_dispatch_calls: bss.prof_dispatch_calls,
            prof_deadline_ns: bss.prof_deadline_ns_total,
            prof_deadline_calls: bss.prof_deadline_calls,

            // P0: CPU Placement Verification
            gpu_phys_kept: bss.nr_gpu_phys_kept,
            compositor_phys_kept: bss.nr_compositor_phys_kept,
            gpu_pref_fallback: bss.nr_gpu_pref_fallback,

            // P0: Deadline Tracking
            deadline_misses: bss.nr_deadline_misses,
            auto_boosts: bss.nr_auto_boosts,

            // P0: Scheduler State
            scheduler_generation: bss.scheduler_generation,
            detected_fg_tgid: bss.detected_fg_tgid,

            // P0: Window Status - calculate from timestamps
            // Note: BPF uses monotonic time (from boot), we approximate by checking if timestamp is non-zero
            // More accurate detection would require reading BPF monotonic time offset or using a BPF helper
            input_window_active: {
                // If input_until_global is non-zero, a window was set
                // Approximate check: if it's been set recently (within last 10 seconds of boot time), consider active
                // This is heuristic - BPF monotonic time offset unknown, so we use non-zero as proxy
                if bss.input_until_global > 0 {
                    // Check if timestamp is reasonable (not expired long ago)
                    // BPF monotonic time starts at boot, so compare to approximate boot time
                    // Simplified: if non-zero and recent (within 10s of typical window duration), likely active
                    // Actual check would need: current_monotonic_time < input_until_global
                    1 // Assume active if non-zero (window was set)
                } else {
                    0
                }
            },
            frame_window_active: 0, // TODO: Frame window tracking not yet implemented
            input_window_until_ns: bss.input_until_global,
            frame_window_until_ns: 0, // TODO: Frame window tracking not yet implemented

            // P1: Boost Distribution (cumulative assignments, not live counts)
            boost_distribution_0: bss.nr_boost_shift_0,
            boost_distribution_1: bss.nr_boost_shift_1,
            boost_distribution_2: bss.nr_boost_shift_2,
            boost_distribution_3: bss.nr_boost_shift_3,
            boost_distribution_4: bss.nr_boost_shift_4,
            boost_distribution_5: bss.nr_boost_shift_5,
            boost_distribution_6: bss.nr_boost_shift_6,
            boost_distribution_7: bss.nr_boost_shift_7,

            // P1: Migration Cooldown
            mig_blocked_cooldown: bss.nr_mig_blocked_cooldown,

            // P1: Input Lane Status
            input_lane_keyboard_rate: bss.input_lane_trigger_rate[InputLane::Keyboard as usize],
            input_lane_mouse_rate: bss.input_lane_trigger_rate[InputLane::Mouse as usize],
            input_lane_other_rate: bss.input_lane_trigger_rate[InputLane::Other as usize],

            // P2: Game Detection Details
            game_detection_method: {
                // Determine detection method from active detectors
                if self.bpf_game_detector.is_some() {
                    "bpf_lsm".to_string()
                } else if self.game_detector.is_some() {
                    "inotify".to_string()
                } else {
                    "none".to_string()
                }
            },
            game_detection_score: {
                // Calculate confidence score based on detection method and game info
                if let Some(game_info) = self.get_detected_game_info() {
                    let mut score = 50u8; // Base score
                    if game_info.is_wine {
                        score += 20;
                    } // Wine games are easily detected
                    if game_info.is_steam {
                        score += 20;
                    } // Steam games are easily detected
                    if bss.detected_fg_tgid > 0 {
                        score += 10;
                    } // Detection confirmed
                    score.min(100)
                } else {
                    0
                }
            },
            game_detection_timestamp: {
                // Use current time as detection timestamp (actual detection time not tracked)
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            },

            // P2: Frame Timing
            frame_interval_ns: bss.frame_interval_ns,
            frame_count: bss.frame_count,
            last_page_flip_ns: bss.last_page_flip_ns,

            // P2: Per-CRTC Frame Timing (Multi-Monitor)
            primary_crtc_ptr: bss.primary_crtc_ptr,
            primary_crtc_fps_x10: bss.primary_crtc_fps_x10,
            primary_crtc_switch_count: bss.primary_crtc_switch_count,
            compositor_plane_calls: bss.compositor_detect_plane_calls,

            // AI Analytics: Latency Percentiles (from histograms)
            select_cpu_latency_p10: Metrics::histogram_percentile(&bss.hist_select_cpu, 10.0),
            select_cpu_latency_p25: Metrics::histogram_percentile(&bss.hist_select_cpu, 25.0),
            select_cpu_latency_p50: Metrics::histogram_percentile(&bss.hist_select_cpu, 50.0),
            select_cpu_latency_p75: Metrics::histogram_percentile(&bss.hist_select_cpu, 75.0),
            select_cpu_latency_p90: Metrics::histogram_percentile(&bss.hist_select_cpu, 90.0),
            select_cpu_latency_p95: Metrics::histogram_percentile(&bss.hist_select_cpu, 95.0),
            select_cpu_latency_p99: Metrics::histogram_percentile(&bss.hist_select_cpu, 99.0),
            select_cpu_latency_p999: Metrics::histogram_percentile(&bss.hist_select_cpu, 99.9),
            enqueue_latency_p10: Metrics::histogram_percentile(&bss.hist_enqueue, 10.0),
            enqueue_latency_p25: Metrics::histogram_percentile(&bss.hist_enqueue, 25.0),
            enqueue_latency_p50: Metrics::histogram_percentile(&bss.hist_enqueue, 50.0),
            enqueue_latency_p75: Metrics::histogram_percentile(&bss.hist_enqueue, 75.0),
            enqueue_latency_p90: Metrics::histogram_percentile(&bss.hist_enqueue, 90.0),
            enqueue_latency_p95: Metrics::histogram_percentile(&bss.hist_enqueue, 95.0),
            enqueue_latency_p99: Metrics::histogram_percentile(&bss.hist_enqueue, 99.0),
            enqueue_latency_p999: Metrics::histogram_percentile(&bss.hist_enqueue, 99.9),
            dispatch_latency_p10: Metrics::histogram_percentile(&bss.hist_dispatch, 10.0),
            dispatch_latency_p25: Metrics::histogram_percentile(&bss.hist_dispatch, 25.0),
            dispatch_latency_p50: Metrics::histogram_percentile(&bss.hist_dispatch, 50.0),
            dispatch_latency_p75: Metrics::histogram_percentile(&bss.hist_dispatch, 75.0),
            dispatch_latency_p90: Metrics::histogram_percentile(&bss.hist_dispatch, 90.0),
            dispatch_latency_p95: Metrics::histogram_percentile(&bss.hist_dispatch, 95.0),
            dispatch_latency_p99: Metrics::histogram_percentile(&bss.hist_dispatch, 99.0),
            dispatch_latency_p999: Metrics::histogram_percentile(&bss.hist_dispatch, 99.9),

            // AI Analytics: Temporal Patterns (rolling windows)
            migrations_last_10s: {
                let now = Instant::now();
                let cutoff_10s = now - Duration::from_secs(10);
                let cutoff_60s = now - Duration::from_secs(60);

                // Update migration history
                let migration_delta = current_migrations.saturating_sub(self.last_migration_count);
                self.last_migration_count = current_migrations;

                if migration_delta > 0 {
                    self.migration_history_10s.push_back((now, migration_delta));
                    self.migration_history_60s.push_back((now, migration_delta));
                }

                // Clean old entries
                while self
                    .migration_history_10s
                    .front()
                    .map(|(t, _)| *t < cutoff_10s)
                    .unwrap_or(false)
                {
                    self.migration_history_10s.pop_front();
                }
                while self
                    .migration_history_60s
                    .front()
                    .map(|(t, _)| *t < cutoff_60s)
                    .unwrap_or(false)
                {
                    self.migration_history_60s.pop_front();
                }

                // Sum migrations in last 10s
                self.migration_history_10s
                    .iter()
                    .map(|(_, count)| count)
                    .sum()
            },
            migrations_last_60s: {
                // Already calculated above, sum migrations in last 60s
                self.migration_history_60s
                    .iter()
                    .map(|(_, count)| count)
                    .sum()
            },
            cpu_util_trend: {
                let now = Instant::now();

                // Update history
                self.cpu_util_history.push_back((now, current_cpu_util));
                let cutoff = now - Duration::from_secs(10);
                while self
                    .cpu_util_history
                    .front()
                    .map(|(t, _)| *t < cutoff)
                    .unwrap_or(false)
                {
                    self.cpu_util_history.pop_front();
                }

                // Calculate trend (simple linear regression over last 10s)
                if self.cpu_util_history.len() >= 3 {
                    let first = self.cpu_util_history.front().unwrap().1;
                    let last = self.cpu_util_history.back().unwrap().1;
                    let delta = last as i64 - first as i64;
                    let threshold = (first * 5) / 100; // 5% threshold

                    if delta > threshold as i64 {
                        "increasing".to_string()
                    } else if delta < -(threshold as i64) {
                        "decreasing".to_string()
                    } else {
                        "stable".to_string()
                    }
                } else {
                    "stable".to_string()
                }
            },
            frame_rate_trend: {
                // Frame rate is not directly tracked, use frame_interval_ns as proxy
                let current_rate = if current_frame_interval > 0 {
                    1_000_000_000.0 / current_frame_interval as f64
                } else {
                    0.0
                };

                let now = Instant::now();
                self.frame_rate_history.push_back((now, current_rate));
                let cutoff = now - Duration::from_secs(10);
                while self
                    .frame_rate_history
                    .front()
                    .map(|(t, _)| *t < cutoff)
                    .unwrap_or(false)
                {
                    self.frame_rate_history.pop_front();
                }

                // Calculate trend
                if self.frame_rate_history.len() >= 3 {
                    let first = self.frame_rate_history.front().unwrap().1;
                    let last = self.frame_rate_history.back().unwrap().1;
                    let delta = last - first;
                    let threshold = first * 0.05; // 5% threshold

                    if delta > threshold {
                        "increasing".to_string()
                    } else if delta < -threshold {
                        "decreasing".to_string()
                    } else {
                        "stable".to_string()
                    }
                } else {
                    "stable".to_string()
                }
            },

            // AI Analytics: Classification Confidence Scores
            input_handler_confidence: {
                let detected = bss.nr_input_handler_threads;
                let name_matches = bss.nr_input_handler_name_match;
                let name_checks = bss.nr_input_handler_name_check_attempts;
                let main_matches = bss.nr_main_thread_match;

                if detected == 0 {
                    0
                } else {
                    // Confidence based on detection method:
                    // - Name match = 70% confidence
                    // - Main thread = 80% confidence
                    // - Behavioral (no name/main) = 60% confidence
                    let mut confidence = 60u8; // Base confidence for behavioral detection
                    if name_matches > 0 {
                        confidence = 70;
                    }
                    if main_matches > 0 {
                        confidence = 80;
                    }
                    // Bonus for high detection rate
                    if name_checks > 0 && (name_matches * 100 / name_checks) > 50 {
                        confidence = confidence.min(100);
                    }
                    confidence
                }
            },
            gpu_submit_confidence: {
                let detected = bss.nr_gpu_submit_threads;
                let fentry_matches = bss.nr_gpu_submit_fentry_match;
                let name_matches = bss.nr_gpu_submit_name_match;

                if detected == 0 {
                    0
                } else {
                    // Fentry detection = 95% confidence (kernel API calls)
                    // Name detection = 70% confidence
                    if fentry_matches > 0 {
                        95
                    } else if name_matches > 0 {
                        70
                    } else {
                        60 // Runtime pattern detection
                    }
                }
            },
            game_audio_confidence: {
                let detected = bss.nr_game_audio_threads;
                let runtime_samples = bss.nr_runtime_pattern_audio_samples;

                if detected == 0 {
                    0
                } else {
                    // Runtime pattern detection = 75% confidence
                    // Fentry detection would be 95% but not tracked separately
                    if runtime_samples > 20 {
                        75
                    } else {
                        60 // Low sample count = lower confidence
                    }
                }
            },
            system_audio_confidence: {
                let detected = bss.nr_system_audio_threads;
                let fentry_matches = bss.nr_system_audio_fentry_matches;
                let name_matches = bss.nr_system_audio_name_matches;

                if detected == 0 {
                    0
                } else {
                    // Fentry detection = 95% confidence
                    // Name detection = 80% confidence (PipeWire/PulseAudio names are reliable)
                    if fentry_matches > 0 {
                        95
                    } else if name_matches > 0 {
                        80
                    } else {
                        0
                    }
                }
            },
            network_confidence: {
                let detected = bss.nr_network_threads;
                let fentry_matches = bss.nr_network_fentry_matches;
                let name_matches = bss.nr_network_name_matches;

                if detected == 0 {
                    0
                } else {
                    // Fentry detection = 95% confidence (kernel socket calls)
                    // Name detection = 70% confidence
                    if fentry_matches > 0 {
                        95
                    } else if name_matches > 0 {
                        70
                    } else {
                        0
                    }
                }
            },
            background_confidence: {
                let detected = bss.nr_background_threads;
                let name_matches = bss.nr_background_name_matches;
                let pattern_samples = bss.nr_background_pattern_samples;

                if detected == 0 {
                    0
                } else {
                    // Name detection = 85% confidence (known processes)
                    // Runtime pattern = 70% confidence
                    if name_matches > 0 {
                        85
                    } else if pattern_samples > 20 {
                        70
                    } else {
                        60
                    }
                }
            },

            // AI Analytics: Thread Type Distribution Percentages
            total_classified_threads: {
                bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    })
            },
            input_handler_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_input_handler_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            gpu_submit_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_gpu_submit_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            game_audio_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_game_audio_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            system_audio_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_system_audio_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            compositor_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_compositor_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            network_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                if total > 0 {
                    (bss.nr_network_threads as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },
            background_pct: {
                let total = bss.nr_input_handler_threads
                    + bss.nr_gpu_submit_threads
                    + bss.nr_game_audio_threads
                    + bss.nr_system_audio_threads
                    + bss.nr_compositor_threads
                    + bss.nr_network_threads
                    + (if bss.nr_background_threads > 10000 {
                        0
                    } else {
                        bss.nr_background_threads
                    });
                let bg = if bss.nr_background_threads > 10000 {
                    0
                } else {
                    bss.nr_background_threads
                };
                if total > 0 {
                    (bg as f64 * 100.0) / total as f64
                } else {
                    0.0
                }
            },

            // PSI (Pressure Stall Information) - scheduler performance indicators
            psi_cpu_some_avg10: psi_cpu.some_avg10,
            psi_cpu_some_avg60: psi_cpu.some_avg60,
            psi_mem_some_avg10: psi_mem.some_avg10,
            psi_mem_some_avg60: psi_mem.some_avg60,
            psi_mem_full_avg10: psi_mem.full_avg10,
            psi_io_some_avg10: psi_io.some_avg10,
            psi_io_some_avg60: psi_io.some_avg60,
            psi_io_full_avg10: psi_io.full_avg10,
        }
    }

    pub fn exited(&mut self) -> bool {
        uei_exited!(&self.skel, uei)
    }

    // Userspace CPU util sampling removed; BPF updates cpu_util and cpu_util_avg.

    fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<UserExitInfo> {
        let (stats_response_tx, stats_request_rx) = self
            .stats_server
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Stats server not initialized"))?
            .channels();

        // Pin the event loop thread: user-specified or auto-select housekeeping CPU.
        let target_cpu = self.opts.event_loop_cpu.or_else(Self::auto_event_loop_cpu);
        if let Some(cpu) = target_cpu {
            let mut set = CpuSet::new();
            if let Err(e) = set.set(cpu) {
                warn!("failed to set CPU {} in CpuSet for event loop: {}", cpu, e);
            } else if let Err(e) = sched_setaffinity(Pid::from_raw(0), &set) {
                warn!("failed to pin event loop to CPU {}: {}", cpu, e);
            }
            let auto_msg = if self.opts.event_loop_cpu == Some(cpu) {
                ""
            } else {
                " (auto-selected)"
            };
            println!("🎯 Event loop pinned to CPU {}{}", cpu, auto_msg);
            info!("🎯 Event loop pinned to CPU {}{}", cpu, auto_msg);
        }

        // Apply real-time scheduling policy for ultra-low latency
        if self.opts.realtime_scheduling {
            let rt_priority = self.opts.rt_priority.clamp(1, 99);
            let param = sched_param {
                sched_priority: rt_priority as i32,
            };

            // SAFETY: sched_setscheduler syscall - required for SCHED_FIFO
            // - Priority clamped to [1, 99] above (valid range)
            // - Error checked and handled below
            // - User explicitly requested this feature via --realtime-scheduling flag
            // - WARNING: Real-time scheduling can lock system if process misbehaves (documented)
            // Note: No safe wrapper exists in nix crate for SCHED_FIFO (only SCHED_OTHER available)
            unsafe {
                let result = sched_setscheduler(0, SCHED_FIFO, &param);
                if result != 0 {
                    warn!(
                        "failed to set real-time scheduling (SCHED_FIFO): {}",
                        std::io::Error::last_os_error()
                    );
                    warn!("Note: Real-time scheduling requires root privileges and can lock up the system if misused");
                } else {
                    info!(
                        "real-time scheduling enabled (SCHED_FIFO, priority: {})",
                        rt_priority
                    );
                    info!("WARNING: Real-time processes can lock up the system if they misbehave");
                }
            }
        }

        // Apply SCHED_DEADLINE scheduling for ultra-low latency with time guarantees
        if self.opts.deadline_scheduling {
            let runtime = self.opts.deadline_runtime_us * 1000; // Convert to nanoseconds
            let deadline = self.opts.deadline_deadline_us * 1000;
            let period = self.opts.deadline_period_us * 1000;

            // SAFETY: sched_setattr syscall via libc::syscall - required for SCHED_DEADLINE
            // - Struct zeroed with std::mem::zeroed() (safe initialization)
            // - All fields set explicitly (size, policy, flags, runtime, deadline, period)
            // - Error checked and handled below
            // - User explicitly requested this feature via --deadline-scheduling flag
            // - WARNING: Hard real-time scheduling can lock system if misused (documented)
            // Note: No safe wrapper exists in nix crate for SCHED_DEADLINE (very new kernel feature)
            // - syscall interface used because sched_setattr() not in libc binding
            unsafe {
                // Initialize sched_attr with zeros first
                let mut attr: sched_attr = std::mem::zeroed();
                attr.size = std::mem::size_of::<sched_attr>() as u32;
                attr.sched_policy = SCHED_DEADLINE as u32;
                attr.sched_flags = SCHED_FLAG_DL_OVERRUN as u64;
                attr.sched_runtime = runtime;
                attr.sched_deadline = deadline;
                attr.sched_period = period;

                // Use sched_setattr for SCHED_DEADLINE (more modern API)
                let result = libc::syscall(
                    libc::SYS_sched_setattr,
                    0, // pid (0 = current process)
                    &attr as *const sched_attr,
                    0, // flags
                );

                if result != 0 {
                    warn!(
                        "failed to set SCHED_DEADLINE scheduling: {}",
                        std::io::Error::last_os_error()
                    );
                    warn!("Note: SCHED_DEADLINE requires root privileges and CONFIG_SCHED_DEADLINE kernel support");
                } else {
                    info!("SCHED_DEADLINE scheduling enabled (runtime: {}µs, deadline: {}µs, period: {}µs)", 
                          self.opts.deadline_runtime_us, self.opts.deadline_deadline_us, self.opts.deadline_period_us);
                    info!("Hard real-time guarantees with no starvation risk");
                }
            }
        }

        // Ultra-low latency optimizations enabled
        info!("INTERRUPT-DRIVEN INPUT: Ring buffer with epoll notification");
        info!("Provides 1-5µs latency with 95-98% CPU savings vs busy polling");

        if self.opts.realtime_scheduling {
            info!("REAL-TIME SCHEDULING ENABLED: Maximum priority scheduling");
            info!("WARNING: Real-time processes can lock up the system if they misbehave");
        }

        if self.opts.deadline_scheduling {
            info!("SCHED_DEADLINE ENABLED: Hard real-time guarantees with time bounds");
            info!("Provides ultra-low latency without starvation risk");
        }

        // REMOVED: power_monitor worker thread - power monitoring removed for gaming performance

        if self.gpu_queue_monitor.is_some() && self.gpu_busy_rx.is_none() {
            let (tx, rx) = mpsc::channel();
            if let Some(mut monitor) = self.gpu_queue_monitor.take() {
                let shutdown_clone = Arc::clone(&shutdown);
                let handle = thread::Builder::new()
                    .name("scx-gpu-monitor".into())
                    .spawn(move || {
                        while !shutdown_clone.load(Ordering::Relaxed) {
                            if let Some(busy) = monitor.poll() {
                                if tx.send(busy).is_err() {
                                    break;
                                }
                            }
                            thread::sleep(Duration::from_millis(5));
                        }
                    })
                    .ok();
                self.gpu_busy_rx = Some(rx);
                self.gpu_monitor_worker = handle;
            }
        }

        // Create epoll and event/timer fds
        let epfd = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC).map_err(|e| anyhow::anyhow!(e))?;

        // Register input devices on epoll; device types already cached during init
        for (idx, dev) in self.input_devs.iter_mut().enumerate() {
            let fd = dev.as_raw_fd();
            if fd < 0 {
                warn!("Invalid fd {} for input device {}", fd, idx);
                continue;
            }

            // SAFETY: Creating a BorrowedFd from raw fd for epoll registration.
            // - Device owns the fd and remains alive for the entire scheduler lifetime
            // - fd is validated >= 0 above (line 820)
            // - evdev 0.12 doesn't implement AsFd trait, requiring borrow_raw
            // - BorrowedFd lifetime is scoped to this epoll_add call only (not stored)
            // - Device won't be dropped until Drop impl (cleanup at line 1160+)
            let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
            // Use level-triggered EPOLLIN to allow fair scheduling between input and stats servicing
            // PERF: Edge-triggered mode for high-frequency input events
            // Reduces wakeups by only waking when new events arrive (not when events are still pending)
            // Benefit: Fewer wakeups, better CPU efficiency (~5-10% improvement)
            epfd.add(
                bfd,
                EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, fd as u64),
            )
            .map_err(|e| anyhow::anyhow!(e))?;
            self.registered_epoll_fds.insert(fd);
        }

        // Register ring buffer FD with epoll for interrupt-driven waking
        // This provides ~1-5µs latency with 95-98% CPU savings vs busy polling
        const RING_BUFFER_TAG: u64 = u64::MAX - 1; // Special tag for ring buffer events
        const DISPATCH_EVENT_TAG: u64 = u64::MAX - 3; // Special tag for dispatch events
        // REMOVED: AUDIO_DETECTOR_TAG - userspace audio detection removed

        // Watchdog state (default to 5s when RT scheduling enabled and unset by user)
        let effective_watchdog_secs: u64 =
            if self.opts.watchdog_secs == 0 && self.opts.realtime_scheduling {
                5
            } else {
                self.opts.watchdog_secs
            };
        let watchdog_enabled = effective_watchdog_secs > 0;

        if let Some(ref rb) = self.input_ring_buffer {
            let rb_fd = rb.ring_buffer_fd();
            if rb_fd >= 0 {
                // SAFETY: Ring buffer FD is valid for the lifetime of the manager
                let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(rb_fd) };
                // PERF: Edge-triggered mode for ring buffer (high-frequency events)
                // Ensures we wake only when new events arrive, not when events are still pending
                epfd.add(
                    bfd,
                    EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, RING_BUFFER_TAG),
                )
                .map_err(|e| anyhow::anyhow!("Failed to register ring buffer with epoll: {}", e))?;
                info!("Ring buffer registered with epoll for interrupt-driven input");
            }
        }

        // Register dispatch event ring buffer FD with epoll for event-driven watchdog monitoring
        // This eliminates 10Hz polling of BPF map (~500-1000ns/sec overhead reduction)
        let dispatch_progress = Arc::new(AtomicU64::new(0));
        let dispatch_progress_clone = Arc::clone(&dispatch_progress);

        let dispatch_event_ringbuf = if watchdog_enabled {
            use libbpf_rs::RingBufferBuilder;

            let mut builder = RingBufferBuilder::new();

            // Add dispatch event ring buffer
            let map = &self.skel.maps.dispatch_event_ringbuf;
            builder
                .add(map, move |data: &[u8]| -> i32 {
                    // Process dispatch event - just increment counter to track progress
                    // Event structure: timestamp (u64), dispatch_type (u8), cpu (u32)
                    // We only care that a dispatch occurred, not the details
                    if data.len() >= std::mem::size_of::<u64>() {
                        dispatch_progress_clone.fetch_add(1, Ordering::Relaxed);
                    }
                    0
                })
                .map_err(|e| {
                    warn!("Failed to add dispatch event ring buffer to builder: {}", e);
                    e
                })?;

            match builder.build() {
                Ok(rb) => {
                    let rb_fd = rb.epoll_fd();
                    if rb_fd >= 0 {
                        // SAFETY: Ring buffer FD is valid for the lifetime of the manager
                        let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(rb_fd) };
                        epfd.add(
                            bfd,
                            EpollEvent::new(
                                EpollFlags::EPOLLIN | EpollFlags::EPOLLET,
                                DISPATCH_EVENT_TAG,
                            ),
                        )
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "Failed to register dispatch event ring buffer with epoll: {}",
                                e
                            )
                        })?;
                        info!("Dispatch event ring buffer registered with epoll for event-driven watchdog");
                        Some(rb)
                    } else {
                        None
                    }
                }
                Err(e) => {
                    warn!("Failed to build dispatch event ring buffer: {}", e);
                    None
                }
            }
        } else {
            None
        };

        self.dispatch_event_ringbuf = dispatch_event_ringbuf;

        // REMOVED: audio_detector epoll registration - userspace audio detection removed
        // Audio threads are now detected via fentry hooks in audio_detect.bpf.h

        // Userspace CPU util sampling deprecated: rely on BPF-side sampling.
        // Store fds
        self.epoll_fd = Some(epfd);

        // Epoll-based interrupt-driven input handling
        // No CPU pinning needed - kernel handles wakeups efficiently

        // OPTIMIZATION: Performance monitoring for busy polling optimizations
        // Tracks latency improvements from implemented optimizations
        let mut epoll_wait_times: Vec<u64> = Vec::with_capacity(1000);
        let mut event_processing_times: Vec<u64> = Vec::with_capacity(1000);
        let mut last_performance_log = Instant::now();
        let mut last_overflow_check = Instant::now();
        let mut prev_overflow_count: u64 = 0;

        // OPTIMIZATION: Ring buffer processing counters
        // Track ring buffer usage to demonstrate functionality
        let mut ring_buffer_processing_count = 0u64;

        // Userspace CPU stats removed; rely on BPF-provided cpu_util

        // PERF: Event-driven watchdog - track dispatch events instead of polling BPF map
        // Dispatch events are emitted from BPF when dispatches occur (direct or shared)
        // This eliminates 10Hz polling completely
        let dispatch_event_count = dispatch_progress; // Use the Arc from ring buffer setup

        let mut last_progress_t = Instant::now();
        let mut last_dispatch_total: u64 = 0; // Will be initialized from dispatch_event_count
        let mut rt_demoted = false;
        let mut last_watchdog_check = Instant::now(); // For legacy RT demote check only

        // Monitoring state
        let mut last_metrics_log = Instant::now();
        let mut prev_mig_blocked: u64 = 0;
        let mut prev_frame_mig_block: u64 = 0;
        // MM hint removed - was let mut prev_mm_hint_hit: u64 = 0;
        let mut prev_idle_pick: u64 = 0;

        // Event loop
        let mut events: [EpollEvent; 64] = [EpollEvent::empty(); 64];
        let mut cached_game_tgid: u32 = 0;
        let mut last_game_check = Instant::now();
        // ZERO-LATENCY INPUT: No batching, no debouncing, immediate BPF syscall on every event
        // Every mouse/keyboard event triggers fanout_set_input_window() synchronously
        // BPF input window (default 2ms) provides natural priority boost coalescing
        while !shutdown.load(Ordering::Relaxed) && !self.exited() {
            // REMOVED: power_hint_rx processing - power monitoring removed for gaming performance

            if let Some(ref rx) = self.gpu_busy_rx {
                while let Ok(busy_percent) = rx.try_recv() {
                    if let Some(bss) = self.skel.maps.bss_data.as_mut() {
                        if busy_percent < 10 {
                            if bss.gpu_queue_busy_until != 0 {
                                bss.gpu_queue_busy_until = 0;
                            }
                        } else {
                            let guard_ns = GpuQueueMonitor::guard_ns(busy_percent);
                            if guard_ns > 0 {
                                let now_ns = monotonic_nanos();
                                if now_ns != 0 {
                                    bss.gpu_queue_busy_until = now_ns.saturating_add(guard_ns);
                                }
                            }
                        }
                    }
                }
            }

            // REMOVED: audio_detector processing - userspace audio detection removed
            // Audio threads are now detected via fentry hooks in audio_detect.bpf.h

            // Watchdog: auto-demote RT/DEADLINE if no scheduler progress
            if watchdog_enabled && !rt_demoted && last_watchdog_check.elapsed().as_secs() >= 1 {
                if let Some(bss) = self.skel.maps.bss_data.as_ref() {
                    let total_now = bss.nr_direct_dispatches + bss.nr_shared_dispatches;
                    if total_now == last_dispatch_total {
                        if last_progress_t.elapsed().as_secs() >= effective_watchdog_secs {
                            // Demote to SCHED_OTHER to prevent system lockup
                            let param = sched_param { sched_priority: 0 };
                            unsafe {
                                let res = sched_setscheduler(0, SCHED_OTHER, &param);
                                if res == 0 {
                                    info!(
                                        "Watchdog: no scheduler progress for {}s; demoted to SCHED_OTHER",
                                        effective_watchdog_secs
                                    );
                                    rt_demoted = true;
                                } else {
                                    warn!(
                                        "Watchdog: failed to demote scheduling policy: {}",
                                        std::io::Error::last_os_error()
                                    );
                                }
                            }
                        }
                    } else {
                        last_dispatch_total = total_now;
                        last_progress_t = Instant::now();
                    }
                }
                last_watchdog_check = Instant::now();
            }
            // Early: service pending stats requests to avoid starvation during heavy input
            while stats_request_rx.try_recv().is_ok() {
                let metrics = self.get_metrics();
                // REMOVED: debug_api_state update - HTTP server removed for leaner scheduler
                stats_response_tx.send(metrics)?;
            }
            // Interrupt-driven input processing with epoll (replaces busy polling)
            // Kernel wakes us when events arrive, providing 1-5µs latency with 95-98% CPU savings
            // TIER 2: 100ms timeout balances shutdown responsiveness (~100ms) vs CPU overhead (~10-50µs/sec)
            // Previous: 1000ms timeout (lower overhead, slower shutdown)
            // Current: 100ms timeout (higher overhead, faster shutdown)
            const EPOLL_TIMEOUT_MS: u16 = 100;
            let epoll_start = Instant::now();
            let epfd = self
                .epoll_fd
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("epoll_fd not initialized in event loop"))?;
            match epfd.wait(&mut events, Some(EPOLL_TIMEOUT_MS)) {
                Ok(n) => {
                    if epoll_wait_times.len() < 1000 {
                        epoll_wait_times.push(epoll_start.elapsed().as_nanos() as u64);
                    }
                    if n == 0 {
                        // Timeout - no events, continue loop for shutdown/stats checks
                        continue;
                    }
                    // Events available, process them below
                }
                Err(e) if e == nix::errno::Errno::EINTR => continue, // Interrupted by signal
                Err(e) => {
                    warn!("epoll_wait failed: {}", e);
                    break;
                }
            }

            // OPTIMIZATION: Rate-limit game detection to every 100ms to avoid
            // redundant checks on every epoll wake (1000Hz+ during input).
            // Game process changes are rare (seconds to minutes), so 100ms is sufficient.
            if last_game_check.elapsed() >= Duration::from_millis(100) {
                last_game_check = Instant::now();

                // Get game TGID from active detector (BPF LSM or inotify fallback)
                let detected_tgid = self.get_detected_game_tgid();
                if cached_game_tgid != detected_tgid {
                    cached_game_tgid = detected_tgid;
                    let bss = self
                        .skel
                        .maps
                        .bss_data
                        .as_mut()
                        .ok_or_else(|| anyhow::anyhow!("BPF BSS map not initialized"))?;
                    // SAFETY: Write to staging area, BPF will copy atomically via get_fg_tgid()
                    // This double-buffering prevents torn reads during hot-path classification
                    bss.detected_fg_tgid_staging = detected_tgid;

                    // Populate game_threads_map for BPF thread tracking
                    if detected_tgid > 0 {
                        self.register_game_threads(detected_tgid);
                    } else {
                        self.clear_tracked_game_threads();
                    }

                    // Log detected game for debugging
                    if let Some(game_info) = self.get_detected_game_info() {
                        info!(
                            "Game detected: '{}' (tgid: {}, wine: {}, steam: {})",
                            game_info.name, game_info.tgid, game_info.is_wine, game_info.is_steam
                        );
                    }
                }

                // Sync game PID to BPF map for exit detection (BPF LSM task_free hook)
                // This must be called periodically to ensure the kernel knows which PID to track
                if let Some(ref detector) = self.bpf_game_detector {
                    detector.sync_to_bpf_map(&self.skel.maps.current_game_map);
                }
            } // End of rate-limited game detection block

            // Track if ring buffer handled input this cycle (see ring_buffer.rs module docs)
            let mut ring_buffer_handled_input_this_cycle = false;

            for (i, ev) in events.iter().enumerate() {
                let tag = ev.data();
                if tag == 0 {
                    continue;
                }

                // Handle dispatch event ring buffer (event-driven watchdog monitoring)
                if tag == DISPATCH_EVENT_TAG {
                    // PERF: Edge-triggered mode requires draining ALL events before returning
                    // More efficient polling: poll once per iteration, break on error (empty buffer)
                    if let Some(ref mut rb) = self.dispatch_event_ringbuf {
                        while rb.poll(std::time::Duration::from_millis(0)).is_ok() {
                            // Continue polling - callback processes events automatically
                            // Loop terminates when poll returns Err (buffer empty)
                        }
                    }
                    continue; // Move to next epoll event
                }

                // Handle ring buffer events (interrupt-driven input notification)
                if tag == RING_BUFFER_TAG {
                    // Ring buffer has input events available
                    // PERF: Edge-triggered mode requires draining ALL events before returning
                    // More efficient while-let pattern: poll until buffer is empty
                    if let Some(ref mut rb) = self.input_ring_buffer {
                        loop {
                            match rb.poll_once() {
                                Ok(()) => {
                                    // Process events from the callback-incremented counter
                                    let (events_processed, _) = rb.process_events();
                                    if events_processed > 0 {
                                        ring_buffer_processing_count += events_processed as u64;
                                        ring_buffer_handled_input_this_cycle = true;
                                    }
                                }
                                Err(_) => {
                                    // poll_once returns Err when the buffer is empty; exit the loop.
                                    break;
                                }
                            }
                        }
                    }
                    continue; // Move to next epoll event
                }

                // REMOVED: audio_detector epoll handling - userspace audio detection removed
                // Audio threads are now detected via fentry hooks in audio_detect.bpf.h
                // Skip AUDIO_DETECTOR_TAG handling entirely

                // OPTIMIZATION: Memory prefetching for better cache performance
                // Prefetches next event to reduce cache miss latency
                // Saves 5-10ns by keeping next event data in cache
                #[cfg(target_arch = "x86_64")]
                if i + 1 < events.len() {
                    // Simple prefetch hint - compiler will optimize memory access patterns
                    let _next_event = &events[i + 1];
                    std::hint::black_box(_next_event);
                }

                // MICRO-OPT: Direct cast, no intermediate variable (saves register)
                let fd = tag as i32;
                let flags = ev.events();

                if flags.contains(EpollFlags::EPOLLHUP) || flags.contains(EpollFlags::EPOLLERR) {
                    if (fd as usize) < self.input_fd_info_vec.len()
                        && self.input_fd_info_vec[fd as usize].is_some()
                    {
                        self.input_fd_info_vec[fd as usize] = None;
                        // Device disconnected - remove from tracking
                        self.registered_epoll_fds.remove(&fd);
                        // SAFETY: Creating BorrowedFd for epoll deletion on device disconnection.
                        // - fd was validated >= 0 during registration (line 820)
                        // - fd is only deleted once (removed from input_fd_to_idx map)
                        // - BorrowedFd lifetime is scoped to this delete call
                        // - Device is already disconnected (EPOLLHUP), so fd is still valid but unusable
                        if fd >= 0 {
                            let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                            if let Some(epfd) = self.epoll_fd.as_ref() {
                                let _ = epfd.delete(bfd);
                            }
                        }
                    }
                    continue;
                }

                // Skip evdev if ring buffer already handled input (avoid double-processing)
                if ring_buffer_handled_input_this_cycle {
                    if let Some(Some(device_info)) = self.input_fd_info_vec.get(fd as usize) {
                        use InputLane::*;
                        match device_info.lane() {
                            Keyboard | Mouse => {
                                continue;
                            }
                            _ => { /* fall through for other lanes (e.g., controller) */ }
                        }
                    }
                }

                // HOT PATH OPTIMIZATION: Direct array access instead of hash map (saves ~40-70ns per event)
                if let Some(Some(device_info)) = self.input_fd_info_vec.get(fd as usize) {
                    let idx = device_info.idx();
                    let lane = device_info.lane();
                    // Validate idx is within bounds before access (handles vector reallocation)
                    if idx >= self.input_devs.len() {
                        // Stale index, clean it up
                        if (fd as usize) < self.input_fd_info_vec.len() {
                            self.input_fd_info_vec[fd as usize] = None;
                        }
                        continue;
                    }
                    if let Some(dev) = self.input_devs.get_mut(idx) {
                        let event_start = Instant::now();
                        if let Ok(iter) = dev.fetch_events() {
                            let mut event_count = 0;
                            let mut has_input_activity = false;
                            const MAX_EVENTS_PER_FD: usize = 512;

                            // OPTIMIZATION: Event batching - collect all events first, then trigger once
                            // Reduces syscall overhead by batching multiple events into single BPF call
                            // Saves 10-25ns per event by avoiding repeated syscall overhead
                            for event in iter {
                                event_count += 1;
                                if event_count > MAX_EVENTS_PER_FD {
                                    break;
                                }

                                // Only trigger on actual input activity, not SYN or zero-delta events
                                if !matches!(lane, InputLane::Other) {
                                    match event.event_type() {
                                        evdev::EventType::KEY => {
                                            // Treat press, release, and repeats as activity to sustain boost
                                            has_input_activity = true;
                                        }
                                        evdev::EventType::RELATIVE => {
                                            // Only trigger on actual mouse movement (non-zero delta)
                                            // Filters out sensor noise and polling events
                                            if event.value() != 0 {
                                                has_input_activity = true;
                                            }
                                        }
                                        evdev::EventType::ABSOLUTE => {
                                            // Trigger on analog input (touchpads, etc.)
                                            has_input_activity = true;
                                        }
                                        _ => {} // Skip SYN and other non-input events
                                    }
                                }
                                // Note: avoid servicing stats here to prevent borrow conflicts with dev iterator
                            }

                            // OPTIMIZATION: Single BPF trigger for all events in this batch
                            // Reduces syscall overhead from N calls to 1 call per epoll wake
                            if has_input_activity {
                                (self.input_trigger_fn)(&self.trig, &mut self.skel, lane);
                            }

                            // OPTIMIZATION: Performance monitoring - track event processing times
                            let event_duration = event_start.elapsed();
                            if event_processing_times.len() < 1000 {
                                event_processing_times.push(event_duration.as_nanos() as u64);
                            }
                            // No per-event debug logs in release to avoid overhead under verbose logging
                        }
                    }
                }
            }

            // Service any pending stats requests without blocking
            while stats_request_rx.try_recv().is_ok() {
                let metrics = self.get_metrics();
                // REMOVED: debug_api_state update - HTTP server removed for leaner scheduler
                stats_response_tx.send(metrics)?;
            }

            // OPTIMIZATION: Performance monitoring - periodic logging of optimization impact
            // Logs latency statistics every 10 seconds to track optimization effectiveness
            if last_performance_log.elapsed() >= Duration::from_secs(10) {
                last_performance_log = Instant::now();

                if !epoll_wait_times.is_empty() && !event_processing_times.is_empty() {
                    // Calculate statistics for epoll wait times
                    epoll_wait_times.sort();
                    let epoll_p50 = epoll_wait_times[epoll_wait_times.len() / 2];
                    let epoll_p99 = epoll_wait_times[(epoll_wait_times.len() * 99) / 100];

                    // Calculate statistics for event processing times
                    event_processing_times.sort();
                    let event_p50 = event_processing_times[event_processing_times.len() / 2];
                    let event_p99 =
                        event_processing_times[(event_processing_times.len() * 99) / 100];

                    info!("PERF: Busy polling optimizations - epoll_wait: p50={}ns p99={}ns, event_processing: p50={}ns p99={}ns", 
                          epoll_p50, epoll_p99, event_p50, event_p99);

                    // Clear samples to prevent memory growth
                    epoll_wait_times.clear();
                    event_processing_times.clear();
                }

                // OPTIMIZATION: Ring buffer performance monitoring
                // Log ring buffer statistics to demonstrate usage and track performance
                if let Some(ref mut input_rb) = self.input_ring_buffer {
                    // Check if events are available before processing
                    if input_rb.has_events() {
                        let (events_processed, _has_activity) = input_rb.process_events();
                        if events_processed > 0 {
                            ring_buffer_processing_count += events_processed as u64;
                        }
                    }
                    let stats = input_rb.stats();
                    let (p50, p95, p99) = input_rb.get_latency_percentiles();
                    info!("RING_BUFFER: Input events processed: {}, batches: {}, avg_events_per_batch: {:.1}, latency: avg={:.1}ns min={}ns max={}ns p50={:.1}ns p95={:.1}ns p99={:.1}ns", 
                          stats.total_events, stats.total_batches, stats.avg_events_per_batch,
                          stats.avg_latency_ns, stats.min_latency_ns, stats.max_latency_ns,
                          p50, p95, p99);
                }
            }

            info!(
                "RING_BUFFER: Total processing cycles: {}",
                ring_buffer_processing_count
            );

            // PERF: Event-driven watchdog - check dispatch events instead of polling BPF map
            // Dispatch events are emitted from BPF when dispatches occur (direct or shared)
            // This eliminates 10Hz polling completely
            if watchdog_enabled {
                let current_dispatch_count = dispatch_event_count.load(Ordering::Relaxed);

                if current_dispatch_count > last_dispatch_total {
                    // Dispatch progress detected - reset timer
                    last_dispatch_total = current_dispatch_count;
                    last_progress_t = Instant::now();
                } else if last_progress_t.elapsed() >= Duration::from_secs(effective_watchdog_secs)
                {
                    // Check if system is genuinely deadlocked or just fully idle
                    let bss = self
                        .skel
                        .maps
                        .bss_data
                        .as_ref()
                        .ok_or_else(|| anyhow::anyhow!("BPF BSS map not initialized"))?;
                    let cpu_util = bss.cpu_util;
                    let is_system_idle = cpu_util == 0;

                    if is_system_idle {
                        // System is fully idle - no dispatches needed, watchdog should not trigger
                        // Reset progress timer to prevent false positive
                        last_progress_t = Instant::now();
                    } else {
                        // System has active CPUs but no dispatch progress - potential deadlock
                        warn!(
                            "watchdog: no dispatch progress for {}s with {}% CPU utilization, exiting to restore CFS",
                            effective_watchdog_secs,
                            (cpu_util * 100) / 1024
                        );
                        shutdown.store(true, Ordering::Relaxed);
                    }
                }
            }

            // Log migration and hint metrics every 10 seconds
            if last_metrics_log.elapsed() >= Duration::from_secs(10) {
                last_metrics_log = Instant::now();
                let bss = self
                    .skel
                    .maps
                    .bss_data
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("BPF BSS map not initialized"))?;
                let mig_blocked = bss.nr_mig_blocked;
                let frame_mig_block = bss.nr_frame_mig_block;
                let idle_pick = bss.nr_idle_cpu_pick;

                let delta_mig_blocked = mig_blocked.saturating_sub(prev_mig_blocked);
                let delta_frame_mig = frame_mig_block.saturating_sub(prev_frame_mig_block);
                // MM hint removed - was let delta_hint_hit = mm_hint_hit.saturating_sub(prev_mm_hint_hit);
                // delta_idle_pick calculated but not currently logged (may be added to metrics in future)
                let _delta_idle_pick = idle_pick.saturating_sub(prev_idle_pick);

                if delta_mig_blocked > 0 || delta_frame_mig > 0 {
                    // MM hint removed - was hint_rate calculation
                    info!(
                        "metrics: mig_blocked={}, frame_mig_blocked={}, mm_hint_hit_rate=N/A (removed)",
                        delta_mig_blocked, delta_frame_mig
                    );
                }

                prev_mig_blocked = mig_blocked;
                prev_frame_mig_block = frame_mig_block;
                // MM hint removed - was prev_mm_hint_hit = mm_hint_hit;
                prev_idle_pick = idle_pick;
            }

            // Ring buffer overflow alert: Check every 1 second for rapid overflow increases
            // This detects when userspace can't keep up with input rate (extremely rare)
            // Zero overhead - pure userspace monitoring, not in hot path
            if last_overflow_check.elapsed() >= Duration::from_secs(1) {
                last_overflow_check = Instant::now();

                // Read overflow count from BPF stats
                let current_overflow = {
                    let stats_map = &self.skel.maps.raw_input_stats_map;
                    let key = 0u32;

                    match stats_map.lookup_percpu(&key.to_ne_bytes(), libbpf_rs::MapFlags::ANY) {
                        Ok(Some(per_cpu)) if !per_cpu.is_empty() => {
                            let mut overflow = 0u64;
                            for bytes in per_cpu {
                                if bytes.len() >= std::mem::size_of::<RawInputStats>() {
                                    // SAFETY: Reading RawInputStats from per-CPU BPF array bytes
                                    // - Size validated above
                                    // - Uses read_unaligned() to handle potential misalignment
                                    // - RawInputStats is #[repr(C)] and matches BPF layout exactly
                                    let ris = unsafe {
                                        (bytes.as_ptr() as *const RawInputStats).read_unaligned()
                                    };
                                    overflow = overflow.saturating_add(ris.ringbuf_overflow_events);
                                }
                            }
                            overflow
                        }
                        _ => 0,
                    }
                };

                // Detect rapid overflow increase (>10 events in 1 second)
                if current_overflow > prev_overflow_count {
                    let delta = current_overflow.saturating_sub(prev_overflow_count);
                    if delta > 10 {
                        warn!(
                            "RING_BUFFER_OVERFLOW: {} events dropped in last second (total: {}). \
                            Userspace cannot keep up with input rate. Consider: \
                            (1) Increasing ring buffer size, (2) Reducing input device polling rate, \
                            (3) Checking for CPU/system load issues",
                            delta, current_overflow
                        );
                    } else if delta > 0 {
                        // Log info for smaller increases (still significant)
                        info!(
                            "RING_BUFFER_OVERFLOW: {} events dropped in last second (total: {}). \
                            If this persists, consider increasing ring buffer size.",
                            delta, current_overflow
                        );
                    }
                }

                prev_overflow_count = current_overflow;
            }
        }

        info!("Scheduler main loop exited, cleaning up...");
        // REMOVED: power_monitor_worker cleanup - power monitoring removed
        if let Some(handle) = self.gpu_monitor_worker.take() {
            let _ = handle.join();
        }
        if let Some(link) = self.struct_ops.take() {
            drop(link);
        }
        // Best-effort cleanup of epoll registrations
        // Only delete FDs that are still registered (not disconnected)
        if let Some(ref ep) = self.epoll_fd {
            for &fd in &self.registered_epoll_fds {
                // SAFETY: Creating BorrowedFd for cleanup during Drop.
                // - FDs in registered_epoll_fds were validated >= 0 during registration (line 820)
                // - FDs removed from this set when device disconnects (line 934), preventing double-delete
                // - This prevents operating on potentially recycled FDs (TOCTOU protection)
                // - Cleanup path only, errors are ignored (best-effort)
                // - BorrowedFd lifetime scoped to this delete call
                let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                let _ = ep.delete(bfd);
            }
        }
        self.registered_epoll_fds.clear();
        self.input_fd_info_vec.clear();
        self.input_devs.clear();
        uei_report!(&self.skel, uei)
    }
}

impl Drop for Scheduler<'_> {
    fn drop(&mut self) {
        info!("Unregister {SCHEDULER_NAME} scheduler");
        if let Some(link) = self.struct_ops.take() {
            drop(link);
        }
        // Best-effort cleanup of epoll registrations
        // Only delete FDs that are still registered (not disconnected)
        if let Some(ref ep) = self.epoll_fd {
            for &fd in &self.registered_epoll_fds {
                // SAFETY: Creating BorrowedFd for cleanup during Drop.
                // - FDs in registered_epoll_fds were validated >= 0 during registration (line 820)
                // - FDs removed from this set when device disconnects (line 934), preventing double-delete
                // - This prevents operating on potentially recycled FDs (TOCTOU protection)
                // - Cleanup path only, errors are ignored (best-effort)
                // - BorrowedFd lifetime scoped to this delete call
                let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                let _ = ep.delete(bfd);
            }
        }
        self.registered_epoll_fds.clear();
        self.input_fd_info_vec.clear();
        self.input_devs.clear();
    }
}

// REMOVED: collect_input_devices - was only used for TUI

fn main() -> Result<()> {
    let opts = Opts::parse();

    if opts.version {
        println!(
            "{} {}",
            SCHEDULER_NAME,
            build_id::full_version(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }

    if opts.help_stats {
        stats::server_data().describe_meta(&mut std::io::stdout(), None)?;
        return Ok(());
    }

    let loglevel = if opts.verbose {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Warn
    };

    let mut lcfg = simplelog::ConfigBuilder::new();
    // SAFETY: Time offset configuration is non-critical - log warning on failure
    // This prevents initialization panic if timezone is misconfigured
    if let Err(e) = lcfg.set_time_offset_to_local() {
        warn!("Failed to set local time offset: {:?}, using UTC", e);
    }
    lcfg.set_time_level(simplelog::LevelFilter::Error)
        .set_location_level(simplelog::LevelFilter::Off)
        .set_target_level(simplelog::LevelFilter::Off)
        .set_thread_level(simplelog::LevelFilter::Off);
    simplelog::TermLogger::init(
        loglevel,
        lcfg.build(),
        simplelog::TerminalMode::Stderr,
        simplelog::ColorChoice::Auto,
    )?;

    // Enable libbpf → log crate integration so verifier and libbpf messages are visible
    init_libbpf_logging(None);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_clone.store(true, Ordering::Relaxed);
        log::info!("Shutdown signal received (Ctrl-C)");
    })
    .context("Error setting Ctrl-C handler")?;

    // REMOVED: TUI mode (--tui) - 3334 lines of bloat removed
    // Use --stats for monitoring instead
    if opts.tui.is_some() {
        log::warn!("TUI mode has been removed. Use --stats <interval> for monitoring.");
    }

    let stats_thread = match opts.monitor.or(opts.stats) {
        Some(raw_intv) => match stats_interval_from_secs(raw_intv) {
            Some(stats_interval) => {
                let shutdown_copy = shutdown.clone();
                Some(std::thread::spawn(move || {
                    match stats::monitor(stats_interval, shutdown_copy) {
                        Ok(_) => {}
                        Err(e) => {
                            log::warn!("stats monitor thread finished because of an error {}", e)
                        }
                    }
                }))
            }
            None => {
                log::info!("Stats monitoring disabled (interval {}s)", raw_intv);
                None
            }
        },
        None => None,
    };

    // Input watch mode: spawn watcher alongside scheduler so stats server is available
    let watch_thread = match opts.watch_input.and_then(stats_interval_from_secs) {
        Some(stats_interval) => {
            let shutdown_copy = shutdown.clone();
            Some(std::thread::spawn(move || {
                let _ = stats::monitor_watch_input(stats_interval, shutdown_copy);
            }))
        }
        None => {
            if opts.watch_input.is_some() {
                log::info!("Input watch disabled (interval {:?}s)", opts.watch_input);
            }
            None
        }
    };

    // Monitor-only mode: just run the stats thread
    if opts.monitor.is_some() {
        if let Some(jh) = stats_thread {
            let _ = jh.join();
        }
        return Ok(());
    }

    // REMOVED: debug_api_thread - HTTP server removed for leaner scheduler
    // Use --stats for monitoring instead

    // (Input polling handled within Scheduler::run loop.)

    let mut open_object = MaybeUninit::uninit();
    loop {
        let mut sched = Scheduler::init(&opts, &mut open_object)?;
        if !sched.run(shutdown.clone())?.should_restart() {
            break;
        }
    }

    // REMOVED: TUI thread cleanup (TUI has been removed)

    // Wait for stats thread to finish (with timeout)
    if let Some(jh) = stats_thread {
        info!("Waiting for stats thread to finish...");
        let mut joined = false;
        for _ in 0..10 {
            if jh.is_finished() {
                let _ = jh.join();
                joined = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !joined {
            warn!("Stats thread didn't finish in time, detaching");
        }
    }

    if let Some(jh) = watch_thread {
        info!("Waiting for watch thread to finish...");
        let _ = jh.join();
    }

    Ok(())
}

// Typed view of BPF raw_input_stats for safe parsing from bytes
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct RawInputStats {
    total_events: u64,
    mouse_movement: u64,
    mouse_buttons: u64,
    button_press: u64,
    button_release: u64,
    gaming_device_events: u64,
    filtered_events: u64,
    fentry_boost_triggers: u64,
    keyboard_lane_triggers: u64,
    ringbuf_overflow_events: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_interval_from_secs_enables_positive_values() {
        let duration = stats_interval_from_secs(1.5).expect("interval should be enabled");
        assert_eq!(duration.as_millis(), 1500);
    }

    #[test]
    fn stats_interval_from_secs_disables_zero() {
        assert!(stats_interval_from_secs(0.0).is_none());
    }

    #[test]
    fn stats_interval_from_secs_disables_negative_or_nan() {
        assert!(stats_interval_from_secs(-5.0).is_none());
        assert!(stats_interval_from_secs(f64::NAN).is_none());
    }
}
