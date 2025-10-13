// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use udev;

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;
pub use bpf_intf::*;

mod stats;
mod trigger;
mod game_detect;
mod game_detect_bpf;  // BPF LSM-based game detection (modern, kernel-level)
mod ml_collect;
mod ml_scoring;
mod ml_autotune;
mod ml_bayesian;
mod ml_profiles;
mod cpu_detect;
mod tui;
mod process_monitor;
// Thread learning modules removed - experimental, not production-ready
// mod thread_patterns;
// mod thread_sampler;
use crate::trigger::TriggerOps;
use crate::game_detect::GameDetector;
use crate::game_detect_bpf::BpfGameDetector;
use crate::ml_collect::MLCollector;
use crate::ml_autotune::MLAutotuner;
use crate::ml_profiles::ProfileManager;
use crate::cpu_detect::CpuInfo;
// Thread learning removed:
// use crate::thread_patterns::ThreadPatternManager;
// use crate::thread_sampler::ThreadSampler;
use rustc_hash::{FxHashMap, FxHashSet};
use std::ffi::c_int;
// removed: userspace /proc/stat util sampling
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::path::Path;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
// use crossbeam::channel::RecvTimeoutError;
use evdev::EventType;
use libbpf_rs::libbpf_sys;
use libbpf_rs::AsRawLibbpf;
use libbpf_rs::MapCore;
use libbpf_rs::OpenObject;
use libbpf_rs::ProgramInput;
use log::{info, warn};
use nix::sched::{sched_setaffinity, CpuSet};
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::unistd::Pid;
use scx_stats::prelude::*;
use scx_utils::build_id;
use scx_utils::compat;
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
use scx_utils::init_libbpf_logging;
use stats::Metrics;
use once_cell::sync::Lazy;

const SCHEDULER_NAME: &str = "scx_gamer";

// Cache CPU detection to avoid repeated /proc/cpuinfo reads
static CPU_INFO: Lazy<CpuInfo> = Lazy::new(|| {
    CpuInfo::detect().expect("Failed to detect CPU")
});

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
#[derive(Debug, Clone, Copy)]
struct DeviceInfo {
    idx: usize,
    lane: InputLane,
}

#[derive(Debug, Clone, clap::Parser)]
#[command(
    name = "scx_gamer",
    version,
    disable_version_flag = true,
    about = "Lightweight scheduler optimized for preserving task-to-CPU locality."
)]
struct Opts {
    /// Exit debug dump buffer length. 0 indicates default.
    #[clap(long, default_value = "0")]
    exit_dump_len: u32,

    /// Maximum scheduling slice duration in microseconds.
    #[clap(short = 's', long, default_value = "10")]
    slice_us: u64,

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

    /// Enable preferred idle CPU scanning.
    ///
    /// With this option enabled, the scheduler will prioritize assigning tasks to higher-ranked
    /// cores before considering lower-ranked ones.
    #[clap(short = 'P', long, action = clap::ArgAction::SetTrue)]
    preferred_idle_scan: bool,

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
    #[clap(short = 'S', long, action = clap::ArgAction::SetTrue)]
    avoid_smt: bool,

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

    /// Enable address space affinity.
    ///
    /// This option allows to keep tasks that share the same address space (e.g., threads of the
    /// same process) on the same CPU across wakeups.
    ///
    /// This can improve locality and performance in certain cache-sensitive workloads.
    #[clap(short = 'a', long, action = clap::ArgAction::SetTrue)]
    mm_affinity: bool,

    /// Migration limiter: window size in milliseconds.
    #[clap(long, default_value = "50")]
    mig_window_ms: u64,

    /// Migration limiter: maximum migrations allowed per task within the window.
    #[clap(long, default_value = "3")]
    mig_max: u32,

    /// Input-active boost window in microseconds (0=disabled).
    /// 5ms window covers Wine/Proton input translation layer delays (200-500µs)
    /// plus game processing time (500-2000µs), ensuring full input pipeline is boosted.
    #[clap(long, default_value = "5000")]
    input_window_us: u64,

    /// Watchdog: if no dispatch progress is observed for N seconds, exit to restore CFS (0=off).
    #[clap(long, default_value = "0")]
    watchdog_secs: u64,

    /// Prefer NAPI/softirq CPUs briefly during input window.
    #[clap(long, action = clap::ArgAction::SetTrue)]
    prefer_napi_on_input: bool,

    /// Disable per-mm recent CPU hint (cache affinity hinting, enabled by default).
    #[clap(long, action = clap::ArgAction::SetTrue)]
    disable_mm_hint: bool,

    /// Per-mm hint LRU size (entries). Clamped to [128, 65536].
    #[clap(long, default_value = "8192")]
    mm_hint_size: u32,

    /// Foreground application TGID (PID of the game’s process group). 0=disable gating.
    #[clap(long, default_value = "0")]
    foreground_pid: u32,

    /// Wakeup timer period in microseconds (min 250). 0=use slice_us.
    #[clap(long, default_value = "500")]
    wakeup_timer_us: u64,

    /// Enable stats monitoring with the specified interval.
    #[clap(long)]
    stats: Option<f64>,

    /// Run in stats monitoring mode with the specified interval. Scheduler
    /// is not launched.
    #[clap(long)]
    monitor: Option<f64>,

    /// Run in TUI (Terminal UI) mode with the specified interval. Scheduler
    /// is not launched. Provides interactive dashboard.
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

    /// Enable ML data collection (samples saved to ~/.scx_gamer/ml_data/)
    #[clap(long, action = clap::ArgAction::SetTrue)]
    ml_collect: bool,

    /// ML sample interval in seconds (default: 5s)
    #[clap(long, default_value = "5.0")]
    ml_sample_interval: f64,

    /// Export ML training data to CSV and exit
    #[clap(long)]
    ml_export_csv: Option<String>,

    /// Show best config for a game and exit
    #[clap(long)]
    ml_show_best: Option<String>,

    /// Enable automated parameter tuning (learning mode)
    /// The scheduler will automatically try different configurations and find the optimal one
    #[clap(long, action = clap::ArgAction::SetTrue)]
    ml_autotune: bool,

    /// Duration per trial in autotune mode (seconds)
    #[clap(long, default_value = "120")]
    ml_autotune_trial_duration: u64,

    /// Maximum total autotune session duration (seconds)
    #[clap(long, default_value = "900")]
    ml_autotune_max_duration: u64,

    /// Use Bayesian optimization instead of grid search (faster convergence)
    #[clap(long, action = clap::ArgAction::SetTrue)]
    ml_bayesian: bool,

    /// Enable per-game profiles (auto-load best config for detected games)
    #[clap(long, action = clap::ArgAction::SetTrue)]
    ml_profiles: bool,

    /// List all saved game profiles and exit
    #[clap(long, action = clap::ArgAction::SetTrue)]
    ml_list_profiles: bool,

    // Thread learning CLI options removed - experimental feature not production-ready
    // If needed in future, restore from git history
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
    input_fd_info: FxHashMap<i32, DeviceInfo>,
    registered_epoll_fds: FxHashSet<i32>,
    trig: trigger::BpfTrigger,
    input_trigger_fn: fn(&trigger::BpfTrigger, &mut BpfSkel, InputLane),
    bpf_game_detector: Option<BpfGameDetector>,    // BPF LSM game detection (kernel-level, preferred)
    game_detector: Option<GameDetector>,           // Fallback inotify detection (if BPF unavailable)
    ml_collector: Option<MLCollector>,  // ML data collection for per-game tuning
    ml_autotuner: Option<MLAutotuner>,  // Automated parameter exploration
    profile_manager: Option<ProfileManager>,  // Per-game config profiles
    last_detected_game: String,  // Track game changes for profile loading
}

impl<'a> Scheduler<'a> {
    /// Get current game TGID from active detector (BPF LSM or inotify fallback)
    #[inline]
    fn get_detected_game_tgid(&self) -> u32 {
        if let Some(ref detector) = self.bpf_game_detector {
            detector.get_game_tgid()
        } else if let Some(ref detector) = self.game_detector {
            detector.get_game_tgid()
        } else {
            0
        }
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
        if device.property_value("ID_INPUT_MOUSE").map(|v| v == "1").unwrap_or(false) {
            return Ok(DeviceType::Mouse);
        }
        if device.property_value("ID_INPUT_KEYBOARD").map(|v| v == "1").unwrap_or(false) {
            return Ok(DeviceType::Keyboard);
        }
        
        // Check for wireless dongle patterns (medium cost)
        if let Some(usb_interfaces) = device.property_value("ID_USB_INTERFACES") {
            let interfaces = usb_interfaces.to_string_lossy();
            if Self::is_wireless_dongle_pattern(&interfaces) {
                // For wireless dongles, prefer mouse classification unless explicitly keyboard
                if device.property_value("ID_INPUT_KEYBOARD").map(|v| v == "1").unwrap_or(false) {
                    return Ok(DeviceType::Keyboard);
                } else {
                    return Ok(DeviceType::Mouse);
                }
            }
        }
        
        // Check for device grouping (highest cost - only if needed)
        if let Some(device_group) = device.property_value("LIBINPUT_DEVICE_GROUP") {
            // OPTIMIZATION: Only do expensive group analysis if no other classification found
            if let Ok(group_device_type) = Self::detect_device_group_primary_type_cached(&device_group.to_string_lossy()) {
                return Ok(group_device_type);
            }
        }
        
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "Device not found in udev"))
    }

    /// Cached version of device group detection to avoid repeated expensive scans
    /// OPTIMIZATION: Use static cache to avoid repeated udev enumeration
    fn detect_device_group_primary_type_cached(device_group: &str) -> Result<DeviceType, std::io::Error> {
        use std::collections::HashMap;
        use std::sync::Mutex;
        use once_cell::sync::Lazy;
        
        // Static cache for device group analysis (expensive operation)
        static GROUP_CACHE: Lazy<Mutex<HashMap<String, DeviceType>>> = Lazy::new(|| Mutex::new(HashMap::new()));
        
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
    fn detect_device_group_primary_type_uncached(device_group: &str) -> Result<DeviceType, std::io::Error> {
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
            if device.property_value("ID_INPUT_MOUSE").map(|v| v == "1").unwrap_or(false) {
                mouse_count += 1;
            }
            if device.property_value("ID_INPUT_KEYBOARD").map(|v| v == "1").unwrap_or(false) {
                keyboard_count += 1;
            }
            if device.property_value("ID_INPUT_JOYSTICK").map(|v| v == "1").unwrap_or(false) {
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
                        if product_str.contains("dongle") || 
                           product_str.contains("receiver") || 
                           product_str.contains("adapter") {
                            // Dongle detected - classify based on interface
                            if let Some(usb_interfaces) = device.property_value("ID_USB_INTERFACES") {
                                let interfaces = usb_interfaces.to_string_lossy();
                                if interfaces.contains("030102") { // HID mouse interface
                                    return Ok(DeviceType::Mouse);
                                } else if interfaces.contains("030101") { // HID keyboard interface
                                    return Ok(DeviceType::Keyboard);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "USB interface analysis failed"))
    }

    /// Detect device type using event capabilities and name analysis (fallback)
    fn detect_via_capabilities_and_name(dev: &evdev::Device, has_rel: bool, has_key: bool) -> DeviceType {
        let name_lc = dev.name().unwrap_or(" ").to_ascii_lowercase();
        
        // Name-based detection with better heuristics
        if name_lc.contains("mouse") || name_lc.contains("trackball") || name_lc.contains("trackpad") {
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
        interfaces.contains("030102") || interfaces.contains("030101") || interfaces.contains("030000")
    }

    /// Register all threads of the detected game in game_threads_map
    /// This enables BPF thread runtime tracking for accurate role detection
    fn register_game_threads(skel: &BpfSkel, tgid: u32) {
        let game_threads_map = &skel.maps.game_threads_map;
        let task_dir = format!("/proc/{}/task", tgid);

        let mut thread_count = 0;
        if let Ok(entries) = std::fs::read_dir(&task_dir) {
            for entry in entries.flatten() {
                if let Ok(tid_str) = entry.file_name().into_string() {
                    if let Ok(tid) = tid_str.parse::<u32>() {
                        let marker: u8 = 1;
                        // Register thread in BPF map for tracking
                        if game_threads_map.update(&tid.to_ne_bytes(), &[marker], libbpf_rs::MapFlags::ANY).is_ok() {
                            thread_count += 1;
                        }
                    }
                }
            }
        }

        if thread_count > 0 {
            info!("Thread tracking: Registered {} game threads for TGID {}", thread_count, tgid);
        }
    }

    #[inline]
    fn auto_event_loop_cpu() -> Option<usize> {
        // Prefer a LITTLE/low-capacity CPU as housekeeping, else the lowest-capacity CPU.
        let topo = Topology::new().ok()?;
            let mut little: Vec<(usize, usize)> = topo
            .all_cpus
            .iter()
            .map(|(id, cpu)| (*id, cpu.cpu_capacity))
            .filter(|(id, _)| topo.all_cpus.get(id).map(|c| matches!(c.core_type, CoreType::Little)).unwrap_or(false))
            .collect();
        if !little.is_empty() {
            little.sort_by_key(|(_, cap)| *cap);
            return little.first().map(|(id, _)| *id);
        }
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
        let has_little = topo.all_cpus.values().any(|c| matches!(c.core_type, CoreType::Little));
        let has_big = topo.all_cpus.values().any(|c| !matches!(c.core_type, CoreType::Little));
        let is_hybrid = has_little && has_big;

        // Auto-enable preferred idle scan for hybrid CPUs unless flat scan is explicitly enabled
        let preferred_idle_scan = if is_hybrid && !opts.flat_idle_scan && !opts.preferred_idle_scan {
            info!("Hybrid CPU topology detected, auto-enabling preferred idle scan");
            true
        } else {
            opts.preferred_idle_scan
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
        rodata.slice_ns = opts.slice_us * 1000;
        rodata.slice_lag = opts.slice_lag_us * 1000;
        rodata.cpufreq_enabled = !opts.disable_cpufreq;
        rodata.deferred_wakeups = !opts.no_deferred_wakeup;
        rodata.flat_idle_scan = opts.flat_idle_scan;
        rodata.smt_enabled = smt_enabled;
        rodata.numa_enabled = opts.enable_numa;
        rodata.no_wake_sync = opts.no_wake_sync;
        rodata.avoid_smt = opts.avoid_smt;
        rodata.mm_affinity = opts.mm_affinity;

        // Generate the list of available CPUs sorted by capacity in descending order.
        // For SMT systems with uniform capacity, prioritize physical cores over hyperthreads.
        let enable_preferred_scan = preferred_idle_scan || smt_enabled;

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

            // Initialize ALL entries to sentinel value (-1 as u64::MAX) first
            // This prevents uninitialized entries (which default to 0, a valid CPU ID)
            // from being treated as valid CPUs by the BPF code
            for i in 0..256 {
                rodata.preferred_cpus[i] = u64::MAX;
            }

            // Now fill in the actual CPU IDs
            for (i, cpu) in cpus.iter().enumerate() {
                rodata.preferred_cpus[i] = cpu.id as u64;
            }
            info!(
                "Preferred CPUs: {:?}",
                &rodata.preferred_cpus[0..cpus.len()]
            );
        }
        rodata.preferred_idle_scan = enable_preferred_scan;
        rodata.mig_window_ns = opts.mig_window_ms * 1_000_000;
        rodata.mig_max_per_window = opts.mig_max;
        rodata.input_window_ns = opts.input_window_us * 1000;
        rodata.prefer_napi_on_input = opts.prefer_napi_on_input;
        rodata.mm_hint_enabled = !opts.disable_mm_hint;
        rodata.wakeup_timer_ns = if opts.wakeup_timer_us == 0 { 0 } else { opts.wakeup_timer_us.max(250) * 1000 };
        rodata.foreground_tgid = opts.foreground_pid;
        // Enable stats collection when any consumer is active (stats, monitor, or TUI)
        rodata.no_stats = !(opts.stats.is_some() || opts.monitor.is_some() || opts.tui.is_some());

        // Configure mm_last_cpu LRU size before load
        let mm_size = opts.mm_hint_size.clamp(128, 65536);
        // SAFETY: BPF map `mm_last_cpu` is valid at this point (skel is open but not loaded).
        // `mm_size` is clamped to [128, 65536] above, within BPF map size limits.
        // libbpf guarantees the map pointer remains valid for the lifetime of `skel`.
        // This call MUST happen before scx_ops_load!() to configure the map size.
        let ret = unsafe {
            libbpf_sys::bpf_map__set_max_entries(
                skel.maps.mm_last_cpu.as_libbpf_object().as_ptr(),
                mm_size,
            )
        };
        if ret != 0 {
            bail!("Failed to set mm_last_cpu map size to {}: error {}", mm_size, ret);
        }

        // Define the primary scheduling domain.
        let primary_cpus = if let Some(ref domain) = opts.primary_domain {
            match parse_cpu_list(domain) {
                Ok(cpus) => cpus,
                Err(e) => bail!("Error parsing primary domain: {}", e),
            }
        } else {
            (0..*NR_CPU_IDS).collect()
        };
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

        let mut input_devs: Vec<evdev::Device> = Vec::new();
        let mut input_fd_info = FxHashMap::default();
        if opts.input_window_us > 0 {
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
                                        // Set O_NONBLOCK for safety
                                        unsafe {
                                            let flags = libc::fcntl(fd, libc::F_GETFL);
                                            if flags >= 0 {
                                                let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
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
                                        input_fd_info.insert(fd, DeviceInfo { idx: input_devs.len(), lane });
                                        input_devs.push(dev);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            info!("Found {} input devices for raw input monitoring", input_devs.len());
        }

        // Initialize ML autotuner if enabled
        let ml_autotuner = if opts.ml_autotune {
            let baseline_config = opts_to_ml_config(opts);
            let trial_duration = Duration::from_secs(opts.ml_autotune_trial_duration);
            let max_duration = Duration::from_secs(opts.ml_autotune_max_duration);

            info!("ML Autotune: Enabled (trial: {}s, max: {}s)",
                  opts.ml_autotune_trial_duration,
                  opts.ml_autotune_max_duration);

            if opts.ml_bayesian {
                info!("ML Autotune: Using Bayesian optimization (faster convergence)");
                Some(MLAutotuner::new_bayesian(
                    baseline_config,
                    trial_duration,
                    max_duration,
                ))
            } else {
                info!("ML Autotune: Using grid search");
                Some(MLAutotuner::new_grid_search(
                    baseline_config,
                    trial_duration,
                    max_duration,
                ))
            }
        } else {
            None
        };

        // Initialize ML collector if enabled (or auto-enabled by autotune)
        let ml_collector = if opts.ml_collect || opts.ml_autotune {
            // Use cached CPU detection for hardware-specific training data
            let cpu_id = CPU_INFO.short_id();

            // Use project-relative path for git-committable training data
            // Structure: ./ml_data/{cpu_model}/{game}.json
            let ml_dir = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("ml_data")
                .join(&cpu_id);

            let config = opts_to_ml_config(opts);
            let interval = Duration::from_secs_f64(opts.ml_sample_interval);

            if opts.ml_autotune {
                info!("ML: Data collection auto-enabled for autotune mode");
            } else {
                info!("ML: Data collection enabled (interval: {:.1}s)", opts.ml_sample_interval);
            }
            info!("ML: CPU detected: {} ({})", CPU_INFO.model_name, cpu_id);
            info!("ML: Training data: {}", ml_dir.display());

            Some(MLCollector::new(ml_dir, config, interval)?)
        } else {
            None
        };

        // Initialize profile manager if enabled
        let profile_manager = if opts.ml_profiles || opts.ml_autotune {
            // Use cached CPU detection for hardware-specific profiles
            let cpu_id = CPU_INFO.short_id();

            // Use project-relative path: ./ml_data/{cpu_model}/profiles/
            let profiles_dir = std::env::current_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."))
                .join("ml_data")
                .join(&cpu_id)
                .join("profiles");

            info!("Profile: Enabled for {} ({})", CPU_INFO.model_name, cpu_id);
            info!("Profile: Storage: {}", profiles_dir.display());
            Some(ProfileManager::new(profiles_dir)?)
        } else {
            None
        };

        // Thread learning feature removed - experimental, not production-ready
        // If needed in future, restore from git history

        // Select input trigger function at init time based on prefer_napi_on_input flag
        // This avoids runtime branching on every input event (saves 10-20ns per event)
        let input_trigger_fn: fn(&trigger::BpfTrigger, &mut BpfSkel, InputLane) = if opts.prefer_napi_on_input {
            |trig, skel, lane| {
                match lane {
                    InputLane::Mouse => {
                        let _ = trig.trigger_input_with_napi_lane(skel, lane);
                    }
                    _ => {
                        let _ = trig.trigger_input_lane(skel, lane);
                    }
                }
            }
        } else {
            |trig, skel, lane| {
                let _ = trig.trigger_input_lane(skel, lane);
            }
        };

        // Initialize game detection: Try BPF LSM first (kernel-level), fallback to inotify
        // BPF LSM benefits (kernel 6.17+):
        // - 60-650× lower CPU overhead (μs/sec vs ms/sec)
        // - 10-100× faster detection (<1ms vs 0-100ms)
        // - Instant game exit detection (<1ms vs 5s polling)
        // - Zero recurring /proc scans (event-driven)
        let (bpf_game_detector, game_detector_fallback) = match BpfGameDetector::new(&mut skel) {
            Ok(detector) => {
                info!("Game detection: Using BPF LSM (kernel-level tracking)");
                (Some(detector), None)
            }
            Err(e) => {
                info!("Game detection: BPF LSM unavailable ({}), using inotify fallback", e);
                (None, Some(GameDetector::new()))
            }
        };

        let scheduler = Self {
            skel,
            opts,
            struct_ops,
            stats_server: Some(stats_server),
            input_devs,
            epoll_fd: None,
            input_fd_info,
            registered_epoll_fds: FxHashSet::default(),
            trig: trigger::BpfTrigger::default(),
            input_trigger_fn,
            bpf_game_detector,
            game_detector: game_detector_fallback,
            ml_collector,
            ml_autotuner,
            profile_manager,
            last_detected_game: String::new(),
        };

        Ok(scheduler)
    }

    fn enable_primary_cpu(skel: &mut BpfSkel<'_>, cpu: i32) -> Result<(), u32> {
        let prog = &mut skel.progs.enable_primary_cpu;
        let mut args = cpu_arg {
            cpu_id: cpu as c_int,
        };
        let input = ProgramInput {
            // SAFETY: Creating a mutable slice from `args` for BPF program input.
            // - `args` is a valid cpu_arg struct on the stack
            // - Lifetime is scoped to this function (args outlives the slice)
            // - size_of_val returns the correct struct size
            // - BPF program reads this as immutable context (no concurrent mutation)
            context_in: Some(unsafe {
                std::slice::from_raw_parts_mut(
                    &mut args as *mut _ as *mut u8,
                    std::mem::size_of_val(&args),
                )
            }),
            ..Default::default()
        };
        let out = prog.test_run(input).unwrap();
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
        let fg_app = self.get_detected_game_info()
            .map(|g| g.name)
            .unwrap_or_else(String::new);

        // Use detected_fg_tgid if available, fallback to foreground_tgid
        let fg_pid = if bss.detected_fg_tgid > 0 {
            bss.detected_fg_tgid as u64
        } else {
            ro.foreground_tgid as u64
        };

        // Read fentry raw input stats (kernel-level input detection)
        // This shows if fentry hooks are active vs falling back to userspace evdev
        let (fentry_total, fentry_triggers, fentry_gaming, fentry_filtered) = {
            let stats_map = &self.skel.maps.raw_input_stats_map;
            let key = 0u32;

            let per_cpu_stats = match stats_map.lookup_percpu(&key.to_ne_bytes(), libbpf_rs::MapFlags::ANY) {
                Ok(Some(per_cpu)) => per_cpu,
                _ => Vec::new(),
            };

            if per_cpu_stats.is_empty() {
                (0, 0, 0, 0)
            } else {
                let mut total = 0u64;
                let mut gaming = 0u64;
                let mut filtered = 0u64;
                let mut triggers = 0u64;

                for bytes in per_cpu_stats {
                    if bytes.len() < 64 {
                        continue;
                    }
                    let parse_u64 = |offset: usize| -> u64 {
                        u64::from_ne_bytes(bytes[offset..offset+8].try_into().unwrap_or([0u8; 8]))
                    };

                    total = total.saturating_add(parse_u64(0));
                    gaming = gaming.saturating_add(parse_u64(40));
                    filtered = filtered.saturating_add(parse_u64(48));
                    triggers = triggers.saturating_add(parse_u64(56));
                }

                (total, triggers, gaming, filtered)
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
            frame_hz_est: 0.0,  // Frame timing removed
            fg_pid,
            fg_app,
            fg_fullscreen: 0,
            win_input_ns: bss.win_input_ns_total,
            win_frame_ns: bss.win_frame_ns_total,
            timer_elapsed_ns: bss.timer_elapsed_ns_total,
            idle_pick: bss.nr_idle_cpu_pick,
            mm_hint_hit: bss.nr_mm_hint_hit,
            fg_cpu_pct: if bss.total_runtime_ns_total > 0 { (bss.fg_runtime_ns_total.saturating_mul(100) / bss.total_runtime_ns_total) as u64 } else { 0 },
            input_trig: bss.nr_input_trig,
            frame_trig: bss.nr_frame_trig,
            sync_wake_fast: bss.nr_sync_wake_fast,
            gpu_submit_threads: bss.nr_gpu_submit_threads,
            background_threads: bss.nr_background_threads,
            compositor_threads: bss.nr_compositor_threads,
            network_threads: bss.nr_network_threads,
            system_audio_threads: bss.nr_system_audio_threads,
            game_audio_threads: bss.nr_game_audio_threads,
            input_handler_threads: bss.nr_input_handler_threads,
            input_trigger_rate: bss.input_trigger_rate as u64,
            continuous_input_mode: bss.continuous_input_mode as u64,
            continuous_input_lane_keyboard: bss.continuous_input_lane_mode[InputLane::Keyboard as usize] as u64,
            continuous_input_lane_mouse: bss.continuous_input_lane_mode[InputLane::Mouse as usize] as u64,
            continuous_input_lane_other: bss.continuous_input_lane_mode[InputLane::Other as usize] as u64,

            // Fentry hook stats (cumulative totals from kernel hooks)
            fentry_total_events: fentry_total,
            fentry_boost_triggers: fentry_triggers,
            fentry_gaming_events: fentry_gaming,
            fentry_filtered_events: fentry_filtered,

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
            .expect("stats server not initialized")
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
            info!("event loop pinned to CPU {}{}",
                cpu,
                if self.opts.event_loop_cpu.is_none() { " (auto)" } else { "" }
            );
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
            epfd.add(bfd, EpollEvent::new(EpollFlags::EPOLLIN, fd as u64)).map_err(|e| anyhow::anyhow!(e))?;
            self.registered_epoll_fds.insert(fd);
        }

        // Userspace CPU util sampling deprecated: rely on BPF-side sampling.
        // Store fds
        self.epoll_fd = Some(epfd);

        // Userspace CPU stats removed; rely on BPF-provided cpu_util

        // Watchdog state
        let watchdog_enabled = self.opts.watchdog_secs > 0;
        let mut last_dispatch_total: u64 = {
            let bss = self.skel.maps.bss_data.as_ref().unwrap();
            (bss.nr_direct_dispatches as u64) + (bss.nr_shared_dispatches as u64)
        };
        let mut last_progress_t = Instant::now();
        let mut last_watchdog_check = Instant::now();

        // Monitoring state
        let mut last_metrics_log = Instant::now();
        let mut prev_mig_blocked: u64 = 0;
        let mut prev_frame_mig_block: u64 = 0;
        let mut prev_mm_hint_hit: u64 = 0;
        let mut prev_idle_pick: u64 = 0;

        // Event loop
        let mut events: [EpollEvent; 16] = [EpollEvent::empty(); 16];
        let mut cached_game_tgid: u32 = 0;
        let mut last_game_check = Instant::now();
        // ZERO-LATENCY INPUT: No batching, no debouncing, immediate BPF syscall on every event
        // Every mouse/keyboard event triggers fanout_set_input_window() synchronously
        // BPF input window (default 2ms) provides natural priority boost coalescing
        while !shutdown.load(Ordering::Relaxed) && !self.exited() {
            // Early: service pending stats requests to avoid starvation during heavy input
            while stats_request_rx.try_recv().is_ok() {
                let metrics = self.get_metrics();

                let game_info = self.get_detected_game_info()
                    .map(|g| ml_collect::GameInfo { tgid: g.tgid, name: g.name, is_wine: g.is_wine, is_steam: g.is_steam })
                    .unwrap_or_else(|| ml_collect::GameInfo { tgid: 0, name: "system".to_string(), is_wine: false, is_steam: false });

                if let Some(ref mut autotuner) = self.ml_autotuner {
                    let sample = ml_collect::PerformanceSample {
                        timestamp: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs(),
                        config: opts_to_ml_config(self.opts),
                        metrics: ml_collect::MLCollector::convert_metrics_static(&metrics),
                        game: game_info,
                    };
                    autotuner.record_sample(sample);
                }

                if let Some(ref mut ml) = self.ml_collector {
                    if let Err(e) = ml.record_sample(&metrics) {
                        warn!("ML: Failed to record sample: {}", e);
                    }
                }

                stats_response_tx.send(metrics)?;
            }
            // Use 50ms timeout to ensure timely stats and shutdown responsiveness even when idle
            const EPOLL_TIMEOUT_MS: u16 = 50;
            match self.epoll_fd.as_ref().unwrap().wait(&mut events, Some(EPOLL_TIMEOUT_MS)) {
                Ok(_) => { /* Process events below */ },
                Err(e) if e == nix::errno::Errno::EINTR => continue,  // Interrupted by signal
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
                    let bss = self.skel.maps.bss_data.as_mut().unwrap();
                    // SAFETY: Write to staging area, BPF will copy atomically via get_fg_tgid()
                    // This double-buffering prevents torn reads during hot-path classification
                    bss.detected_fg_tgid_staging = detected_tgid;

                    // Populate game_threads_map for BPF thread tracking
                    if detected_tgid > 0 {
                        Self::register_game_threads(&self.skel, detected_tgid);
                    }

                    // Update ML collector with new game
                    let game_info_for_ml = self.get_detected_game_info().map(|g| {
                        info!("ML: Detected game '{}' (tgid: {}, wine: {}, steam: {})",
                              g.name, g.tgid, g.is_wine, g.is_steam);
                        ml_collect::GameInfo {
                            tgid: g.tgid,
                            name: g.name.clone(),
                            is_wine: g.is_wine,
                            is_steam: g.is_steam,
                        }
                    });
                    if let Some(ref mut ml) = self.ml_collector {
                        ml.set_game(game_info_for_ml);
                    }

                    // Auto-load profile for detected game
                    if let Some(ref game_info) = self.get_detected_game_info() {
                        if self.last_detected_game != game_info.name {
                            self.last_detected_game = game_info.name.clone();

                            if let Some(ref manager) = self.profile_manager {
                                if let Some(profile) = manager.get_profile(&game_info.name) {
                                    info!(
                                        "Profile: Auto-loading '{}' (score: {:.2}, FPS: {:.1})",
                                        game_info.name,
                                        profile.best_score,
                                        profile.avg_fps
                                    );

                                    // Apply the saved config
                                    if let Err(e) = ml_autotune::apply_config_hot(&mut self.skel, &profile.best_config) {
                                        warn!("Profile: Failed to apply config: {}", e);
                                    }
                                } else {
                                    info!("Profile: No saved config for '{}', using defaults", game_info.name);
                                }
                            }
                        }
                    }
                }
            }  // End of rate-limited game detection block

            for ev in events.iter() {
                let tag = ev.data();
                if tag == 0 { continue; }

                // MICRO-OPT: Direct cast, no intermediate variable (saves register)
                let fd = tag as i32;
                let flags = ev.events();

                if flags.contains(EpollFlags::EPOLLHUP) || flags.contains(EpollFlags::EPOLLERR) {
                    if self.input_fd_info.remove(&fd).is_some() {
                        // Device disconnected - remove from tracking
                        self.registered_epoll_fds.remove(&fd);
                        // SAFETY: Creating BorrowedFd for epoll deletion on device disconnection.
                        // - fd was validated >= 0 during registration (line 820)
                        // - fd is only deleted once (removed from input_fd_to_idx map)
                        // - BorrowedFd lifetime is scoped to this delete call
                        // - Device is already disconnected (EPOLLHUP), so fd is still valid but unusable
                        if fd >= 0 {
                            let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                            let _ = self.epoll_fd.as_ref().unwrap().delete(bfd);
                        }
                    }
                    continue;
                }

                // PERF: Single HashMap lookup gets both idx and dev_type (saves ~15-30ns per event)
                if let Some(&DeviceInfo { idx, lane }) = self.input_fd_info.get(&fd) {
                    // Validate idx is within bounds before access (handles vector reallocation)
                    if idx >= self.input_devs.len() {
                        // Stale index, clean it up
                        self.input_fd_info.remove(&fd);
                        continue;
                    }
                    if let Some(dev) = self.input_devs.get_mut(idx) {
                        if let Ok(iter) = dev.fetch_events() {
                            let mut event_count = 0;
                            const MAX_EVENTS_PER_FD: usize = 512;
                            for event in iter {
                                event_count += 1;
                                if event_count > MAX_EVENTS_PER_FD { break; }
                                
                                // Only trigger on actual input activity, not SYN or zero-delta events
                                if !matches!(lane, InputLane::Other) {
                                    match event.event_type() {
                                        evdev::EventType::KEY => {
                                            // Treat press, release, and repeats as activity to sustain boost
                                            (self.input_trigger_fn)(&self.trig, &mut self.skel, lane);
                                        }
                                        evdev::EventType::RELATIVE => {
                                            // Only trigger on actual mouse movement (non-zero delta)
                                            // Filters out sensor noise and polling events
                                            if event.value() != 0 {
                                                (self.input_trigger_fn)(&self.trig, &mut self.skel, lane);
                                            }
                                        }
                                        evdev::EventType::ABSOLUTE => {
                                            // Trigger on analog input (touchpads, etc.)
                                            (self.input_trigger_fn)(&self.trig, &mut self.skel, lane);
                                        }
                                        _ => {} // Skip SYN and other non-input events
                                    }
                                }
                                // Note: avoid servicing stats here to prevent borrow conflicts with dev iterator
                            }
                            // No per-event debug logs in release to avoid overhead under verbose logging
                        }
                    }
                }
            }

            // ML Autotune: Check if we should switch to next trial
            if let Some(ref mut autotuner) = self.ml_autotuner {
                if autotuner.should_switch_trial() {
                    if let Some(next_config) = autotuner.next_trial() {
                        // Apply new configuration hot (without restart!)
                        if let Err(e) = ml_autotune::apply_config_hot(&mut self.skel, &next_config) {
                            warn!("ML Autotune: Failed to apply config: {}", e);
                        }

                        // ML collector updates are handled automatically by autotuner
                        // No manual intervention needed here
                    } else {
                        // Autotune complete! Print final report
                        let report = autotuner.generate_report();
                        info!("{}", report);

                        // Optionally: Apply best config and continue running
                        if let Some((best_config, score)) = autotuner.get_best_config() {
                            info!("ML Autotune: Applying best config (score: {:.2})", score);
                            if let Err(e) = ml_autotune::apply_config_hot(&mut self.skel, &best_config) {
                                warn!("ML Autotune: Failed to apply best config: {}", e);
                            }
                        }

                        // Clear autotuner to stop further switching
                        self.ml_autotuner = None;
                    }
                }
            }

            // Service any pending stats requests without blocking
            while stats_request_rx.try_recv().is_ok() {
                let metrics = self.get_metrics();

                // Record ML sample for autotune trial
                // Get game info before mutable borrow (borrow checker)
                let game_info = self.get_detected_game_info()
                    .map(|g| ml_collect::GameInfo {
                        tgid: g.tgid,
                        name: g.name,
                        is_wine: g.is_wine,
                        is_steam: g.is_steam,
                    })
                    .unwrap_or_else(|| ml_collect::GameInfo {
                        tgid: 0,
                        name: "system".to_string(),
                        is_wine: false,
                        is_steam: false,
                    });

                if let Some(ref mut autotuner) = self.ml_autotuner {

                    let sample = ml_collect::PerformanceSample {
                        timestamp: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap()
                            .as_secs(),
                        config: opts_to_ml_config(self.opts),
                        metrics: ml_collect::MLCollector::convert_metrics_static(&metrics),
                        game: game_info,
                    };

                    autotuner.record_sample(sample);
                }

                // Record ML sample if collector is enabled (before sending to stats)
                if let Some(ref mut ml) = self.ml_collector {
                    if let Err(e) = ml.record_sample(&metrics) {
                        warn!("ML: Failed to record sample: {}", e);
                    }
                }

                stats_response_tx.send(metrics)?;
            }

            // DRM debugfs polling removed - inotify-based detection is already in place above
            // If inotify fails, the timer fallback (frame_hz) provides frame windows

            // Watchdog: detect lack of dispatch progress and trigger clean shutdown.
            // Only check every 100ms to reduce BPF map access overhead
            if watchdog_enabled && last_watchdog_check.elapsed() >= Duration::from_millis(100) {
                last_watchdog_check = Instant::now();
                let bss = self.skel.maps.bss_data.as_ref().unwrap();
                let dispatch_total = (bss.nr_direct_dispatches as u64) + (bss.nr_shared_dispatches as u64);
                if dispatch_total != last_dispatch_total {
                    last_dispatch_total = dispatch_total;
                    last_progress_t = Instant::now();
                } else if last_progress_t.elapsed() >= Duration::from_secs(self.opts.watchdog_secs) {
                    // Check if system is genuinely deadlocked or just fully idle
                    // Use cpu_util from BPF which tracks busy CPUs (computed every timer tick)
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
                            self.opts.watchdog_secs,
                            (cpu_util * 100) / 1024
                        );
                        shutdown.store(true, Ordering::Relaxed);
                    }
                }
            }

            // Log migration and hint metrics every 10 seconds
            if last_metrics_log.elapsed() >= Duration::from_secs(10) {
                last_metrics_log = Instant::now();
                let bss = self.skel.maps.bss_data.as_ref().unwrap();
                let mig_blocked = bss.nr_mig_blocked;
                let frame_mig_block = bss.nr_frame_mig_block;
                let mm_hint_hit = bss.nr_mm_hint_hit;
                let idle_pick = bss.nr_idle_cpu_pick;

                let delta_mig_blocked = mig_blocked.saturating_sub(prev_mig_blocked);
                let delta_frame_mig = frame_mig_block.saturating_sub(prev_frame_mig_block);
                let delta_hint_hit = mm_hint_hit.saturating_sub(prev_mm_hint_hit);
                let delta_idle_pick = idle_pick.saturating_sub(prev_idle_pick);

                if delta_mig_blocked > 0 || delta_frame_mig > 0 || delta_hint_hit > 0 {
                    let hint_rate = if delta_idle_pick > 0 {
                        (delta_hint_hit * 100) as f64 / delta_idle_pick as f64
                    } else {
                        0.0
                    };
                    info!(
                        "metrics: mig_blocked={}, frame_mig_blocked={}, mm_hint_hit_rate={:.1}% ({}/{})",
                        delta_mig_blocked, delta_frame_mig, hint_rate, delta_hint_hit, delta_idle_pick
                    );
                }

                prev_mig_blocked = mig_blocked;
                prev_frame_mig_block = frame_mig_block;
                prev_mm_hint_hit = mm_hint_hit;
                prev_idle_pick = idle_pick;
            }
        }

        info!("Scheduler main loop exited, cleaning up...");
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
        self.input_fd_info.clear();
        self.input_devs.clear();
        uei_report!(&self.skel, uei)
    }
}

/// Convert Opts to ML SchedulerConfig (inline for zero-cost abstraction)
#[inline]
fn opts_to_ml_config(opts: &Opts) -> ml_collect::SchedulerConfig {
    ml_collect::SchedulerConfig {
        slice_us: opts.slice_us,
        slice_lag_us: opts.slice_lag_us,
        input_window_us: opts.input_window_us,
        mig_window_ms: opts.mig_window_ms,
        mig_max: opts.mig_max,
        mm_affinity: opts.mm_affinity,
        avoid_smt: opts.avoid_smt,
        preferred_idle_scan: opts.preferred_idle_scan,
        enable_numa: opts.enable_numa,
        wakeup_timer_us: opts.wakeup_timer_us,
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
        self.input_fd_info.clear();
        self.input_devs.clear();
    }
}

/// Helper to get ML data directory (CPU-specific, project-relative)
fn get_ml_data_dir() -> Result<std::path::PathBuf> {
    let cpu_id = CPU_INFO.short_id();
    Ok(std::env::current_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("ml_data")
        .join(&cpu_id))
}

fn collect_input_devices(opts: &Opts) -> Vec<String> {
    let mut open_object = MaybeUninit::uninit();
    let result = Scheduler::init(opts, &mut open_object).map(|sched| {
        sched
            .input_devs
            .iter()
            .filter_map(|dev| dev.name().map(|s| s.to_string()))
            .collect::<Vec<_>>()
    });
    result.unwrap_or_default()
}

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

    // List profiles command
    if opts.ml_list_profiles {
        let ml_dir = get_ml_data_dir()?;
        let profiles_dir = ml_dir.join("profiles");
        let manager = ProfileManager::new(profiles_dir)?;

        let games = manager.list_games();
        if games.is_empty() {
            println!("No saved profiles found.");
        } else {
            println!("Saved Game Profiles:");
            println!("═══════════════════════════════════════════════════════════");
            for game in &games {
                if let Some(profile) = manager.get_profile(game) {
                    println!("{}", game);
                    println!("  Score: {:.2}  FPS: {:.1}  Jitter: {:.2}ms  Latency: {}ns",
                             profile.best_score, profile.avg_fps, profile.avg_jitter_ms, profile.avg_latency_ns);
                    println!("  Config: --slice-us {} --input-window-us {} --mig-max {}",
                             profile.best_config.slice_us, profile.best_config.input_window_us, profile.best_config.mig_max);
                    if profile.best_config.mm_affinity { println!("    --mm-affinity"); }
                    if profile.best_config.avoid_smt { println!("    --avoid-smt"); }
                    println!();
                }
            }

            let summary = manager.get_summary();
            println!("═══════════════════════════════════════════════════════════");
            println!("Total games: {}  Avg score: {:.2}  Avg FPS: {:.1}",
                     summary.total_games, summary.avg_score, summary.avg_fps);
        }
        return Ok(());
    }


    // ML export command: export all collected data to CSV for training
    if let Some(ref csv_path) = opts.ml_export_csv {
        let ml_dir = get_ml_data_dir()?;
        let config = opts_to_ml_config(&opts);
        let collector = MLCollector::new(ml_dir.clone(), config, Duration::from_secs_f64(opts.ml_sample_interval))?;
        collector.export_training_csv(csv_path)?;
        println!("ML training data exported to: {}", csv_path);
        println!("Training data from: {}", ml_dir.display());
        return Ok(());
    }

    // ML best config command: show best known configuration for a game
    if let Some(ref game_name) = opts.ml_show_best {
        let ml_dir = get_ml_data_dir()?;
        let config = opts_to_ml_config(&opts);
        let collector = MLCollector::new(ml_dir.clone(), config, Duration::from_secs_f64(opts.ml_sample_interval))?;
        let summary = collector.get_game_summary(game_name)?;

        println!("ML Summary for '{}':", game_name);
        println!("  Samples collected: {}", summary.sample_count);
        println!("  Avg CPU util: {:.1}%", summary.avg_cpu_util);
        println!("  Avg select_cpu latency: {:.0}ns", summary.avg_select_cpu_latency_ns);
        println!("  Avg enqueue latency: {:.0}ns", summary.avg_enqueue_latency_ns);

        if let Some(best_cfg) = summary.best_config {
            println!("\nBest Configuration (score: {:.2}):", summary.best_score.unwrap_or(0.0));
            println!("  --slice-us {}", best_cfg.slice_us);
            println!("  --slice-lag-us {}", best_cfg.slice_lag_us);
            println!("  --input-window-us {}", best_cfg.input_window_us);
            println!("  --mig-window-ms {}", best_cfg.mig_window_ms);
            println!("  --mig-max {}", best_cfg.mig_max);
            if best_cfg.mm_affinity { println!("  --mm-affinity"); }
            if best_cfg.avoid_smt { println!("  --avoid-smt"); }
            if best_cfg.preferred_idle_scan { println!("  --preferred-idle-scan"); }
            if best_cfg.enable_numa { println!("  --enable-numa"); }
            println!("  --wakeup-timer-us {}", best_cfg.wakeup_timer_us);
        } else {
            println!("\nNo configuration data available yet.");
        }

        return Ok(());
    }

    let loglevel = if opts.verbose {
        simplelog::LevelFilter::Debug
    } else {
        simplelog::LevelFilter::Warn
    };

    let mut lcfg = simplelog::ConfigBuilder::new();
    lcfg.set_time_offset_to_local()
        .expect("Failed to set local time offset")
        .set_time_level(simplelog::LevelFilter::Error)
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
    })
    .context("Error setting Ctrl-C handler")?;

    let input_device_names = opts.tui
        .map(|_| collect_input_devices(&opts))
        .unwrap_or_else(Vec::new);

    let tui_thread = if let Some(intv) = opts.tui {
        let shutdown_copy = shutdown.clone();
        let opts_copy = opts.clone();
        let devices_copy = input_device_names.clone();
        Some(std::thread::spawn(move || {
            let tui_interval = Duration::from_secs_f64(intv);
            let result = tui::monitor_tui(tui_interval, shutdown_copy, &opts_copy, devices_copy);
            match result {
                Ok(_) => {
                    info!("TUI exited normally");
                }
                Err(e) => {
                    log::warn!("TUI monitor thread finished because of an error {}", e)
                }
            }
        }))
    } else {
        None
    };

    let stats_thread = if let Some(intv) = opts.monitor.or(opts.stats) {
        let shutdown_copy = shutdown.clone();
        Some(std::thread::spawn(move || {
            let stats_interval = Duration::from_secs_f64(intv);
            match stats::monitor(stats_interval, shutdown_copy) {
                Ok(_) => {}
                Err(e) => {
                    log::warn!("stats monitor thread finished because of an error {}", e)
                }
            }
        }))
    } else {
        None
    };

    // Input watch mode: spawn watcher alongside scheduler so stats server is available
    let watch_thread = if let Some(intv) = opts.watch_input {
        let shutdown_copy = shutdown.clone();
        Some(std::thread::spawn(move || {
            let stats_interval = Duration::from_secs_f64(intv);
            let _ = stats::monitor_watch_input(stats_interval, shutdown_copy);
        }))
    } else {
        None
    };

    // Monitor-only mode: just run the stats thread
    if opts.monitor.is_some() {
        if let Some(jh) = stats_thread {
            let _ = jh.join();
        }
        return Ok(());
    }

    // (Input polling handled within Scheduler::run loop.)

    let mut open_object = MaybeUninit::uninit();
    loop {
        let mut sched = Scheduler::init(&opts, &mut open_object)?;
        if !sched.run(shutdown.clone())?.should_restart() {
            break;
        }
    }

    // Wait for TUI thread to finish cleanup (if it was running)
    // This ensures terminal is properly restored before we exit
    if let Some(jh) = tui_thread {
        info!("Waiting for TUI thread to finish cleanup...");
        let _ = jh.join();
        // Give terminal a moment to fully restore
        std::thread::sleep(std::time::Duration::from_millis(100));
    }

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
