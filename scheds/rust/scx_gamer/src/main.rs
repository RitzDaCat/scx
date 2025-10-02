// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;
pub use bpf_intf::*;

mod stats;
mod trigger;
mod game_detect;
use crate::trigger::TriggerOps;
use crate::game_detect::GameDetector;
use std::collections::{HashMap, HashSet};
use std::ffi::c_int;
// removed: userspace /proc/stat util sampling
use std::mem::MaybeUninit;
use std::os::fd::AsRawFd;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
// use crossbeam::channel::RecvTimeoutError;
use evdev::EventType;
use libbpf_rs::libbpf_sys;
use libbpf_rs::AsRawLibbpf;
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
use stats::Metrics;

const SCHEDULER_NAME: &str = "scx_gamer";

// Auto-detection tuning
const MIN_KEYBOARD_TRIGGER_GAP_US: u64 = 100;  // debounce input triggers (matches hardware polling)
const MIN_MOUSE_TRIGGER_GAP_US: u64 = 100;     // allow high-polling mice (1000Hz+)

#[derive(Debug, clap::Parser)]
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
    #[clap(long, default_value = "2000")]
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
}

// CPU parsing helpers moved to scx_utils::cpu_list

// removed: CpuTimes and userspace util sampling helpers

struct Scheduler<'a> {
    skel: BpfSkel<'a>,
    opts: &'a Opts,
    struct_ops: Option<libbpf_rs::Link>,
    stats_server: StatsServer<(), Metrics>,
    input_devs: Vec<evdev::Device>,
    input_fd_to_idx: HashMap<i32, usize>,
    registered_epoll_fds: HashSet<i32>,
    last_input_trigger: Option<Instant>,
    epoll_fd: Option<Epoll>,
    trig: trigger::BpfTrigger,
    game_detector: Option<GameDetector>,
}

impl<'a> Scheduler<'a> {
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

            // Verify we don't exceed MAX_CPUS (1024) to prevent array out-of-bounds
            const MAX_CPUS: usize = 1024;
            if cpus.len() > MAX_CPUS {
                bail!(
                    "System has {} CPUs but scheduler MAX_CPUS is {}. Recompile with larger MAX_CPUS.",
                    cpus.len(), MAX_CPUS
                );
            }

            let min_cap = cpus.iter().map(|cpu| cpu.cpu_capacity).min().unwrap_or(0);
            let max_cap = cpus.iter().map(|cpu| cpu.cpu_capacity).max().unwrap_or(0);

            if max_cap != min_cap {
                // Heterogeneous capacity: sort by capacity descending
                cpus.sort_by_key(|cpu| std::cmp::Reverse(cpu.cpu_capacity));
            } else if smt_enabled {
                // Uniform capacity with SMT: prioritize physical cores (first sibling in each core)
                cpus.sort_by_key(|cpu| {
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
                cpus.sort_by_key(|cpu| cpu.id);
                info!("Uniform CPU capacities detected; preferred idle scan uses CPU ID order");
            }

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
        rodata.no_stats = opts.stats.is_none() && opts.monitor.is_none();

        // Configure mm_last_cpu LRU size before load
        let mm_size = opts.mm_hint_size.clamp(128, 65536);
        unsafe {
            libbpf_sys::bpf_map__set_max_entries(
                skel.maps.mm_last_cpu.as_libbpf_object().as_ptr(),
                mm_size,
            );
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

        // Initialize input devices for input-active window if enabled
        let mut input_devs = Vec::new();
        if opts.input_window_us > 0 {
            if let Ok(dir) = std::fs::read_dir("/dev/input") {
                for entry in dir.flatten() {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                        if name.starts_with("event") {
                            if let Ok(dev) = evdev::Device::open(&path) {
                                input_devs.push(dev);
                            }
                        }
                    }
                }
            }
        }

        let scheduler = Self {
            skel,
            opts,
            struct_ops,
            stats_server,
            input_devs,
            epoll_fd: None,
            input_fd_to_idx: HashMap::new(),
            registered_epoll_fds: HashSet::new(),
            last_input_trigger: None,
            trig: trigger::BpfTrigger::default(),
            game_detector: Some(GameDetector::new()),
        };

        Ok(scheduler)
    }

    fn enable_primary_cpu(skel: &mut BpfSkel<'_>, cpu: i32) -> Result<(), u32> {
        let prog = &mut skel.progs.enable_primary_cpu;
        let mut args = cpu_arg {
            cpu_id: cpu as c_int,
        };
        let input = ProgramInput {
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

    fn get_metrics(&self) -> Metrics {
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
            fg_pid: ro.foreground_tgid as u64,
            fg_app: String::new(),
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
        }
    }

    pub fn exited(&mut self) -> bool {
        uei_exited!(&self.skel, uei)
    }

    // Userspace CPU util sampling removed; BPF updates cpu_util and cpu_util_avg.

    fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<UserExitInfo> {
        let (stats_response_tx, stats_request_rx) = self.stats_server.channels();

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

        // Register input devices on epoll
        for (idx, dev) in self.input_devs.iter().enumerate() {
            let fd = dev.as_raw_fd();
            if fd < 0 {
                warn!("Invalid fd {} for input device {}", fd, idx);
                continue;
            }
            self.input_fd_to_idx.insert(fd, idx);
            // Safety: Device owns the fd and remains alive for this call. The fd is validated >= 0.
            // evdev 0.12 doesn't implement AsFd trait, so we must use borrow_raw.
            // The borrowed fd lifetime is scoped to this epoll_add call only.
            let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
            epfd.add(bfd, EpollEvent::new(EpollFlags::EPOLLIN | EpollFlags::EPOLLET, fd as u64)).map_err(|e| anyhow::anyhow!(e))?;
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
        while !shutdown.load(Ordering::Relaxed) && !self.exited() {
            // Use 1-second timeout to check shutdown flag regularly without busy-polling.
            // This ensures Ctrl+C is handled within 1s even on idle systems.
            const EPOLL_TIMEOUT_MS: u16 = 1000;
            match self.epoll_fd.as_ref().unwrap().wait(&mut events, Some(EPOLL_TIMEOUT_MS)) {
                Ok(_) => { /* Process events below */ },
                Err(e) if e == nix::errno::Errno::EINTR => continue,  // Interrupted by signal
                Err(e) => {
                    warn!("epoll_wait failed: {}", e);
                    break;
                }
            }

            if let Some(detector) = &self.game_detector {
                let detected_tgid = detector.get_game_tgid();
                if cached_game_tgid != detected_tgid {
                    cached_game_tgid = detected_tgid;
                    let bss = self.skel.maps.bss_data.as_mut().unwrap();
                    bss.detected_fg_tgid = detected_tgid;
                }
            }

            for ev in events.iter() {
                let tag = ev.data();
                if tag == 0 { continue; }

                // Input device file descriptors
                {
                    let other_fd = tag;
                        let fd = other_fd as i32;
                        let flags = ev.events();

                        if flags.contains(EpollFlags::EPOLLHUP) || flags.contains(EpollFlags::EPOLLERR) {
                            if self.input_fd_to_idx.remove(&fd).is_some() {
                                // Device disconnected - remove from tracking and epoll
                                self.registered_epoll_fds.remove(&fd);
                                // Safety: We validated fd >= 0 during registration, scoped to this delete call
                                if fd >= 0 {
                                    let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                                    let _ = self.epoll_fd.as_ref().unwrap().delete(bfd);
                                }
                            }
                            continue;
                        }

                        if let Some(&idx) = self.input_fd_to_idx.get(&fd) {
                            // Validate idx is within bounds before access (handles vector reallocation)
                            if idx >= self.input_devs.len() {
                                // Stale index, clean it up
                                self.input_fd_to_idx.remove(&fd);
                                continue;
                            }
                            if let Some(dev) = self.input_devs.get_mut(idx) {
                                if let Ok(mut iter) = dev.fetch_events() {
                                    // Optimization: Check only first event to determine device type
                                    // Drain remaining events without processing to clear buffer (10-20µs saved)
                                    let mut found_input = false;
                                    let mut is_keyboard = false;

                                    if let Some(e) = iter.next() {
                                        let et = e.event_type();
                                        if et == EventType::KEY {
                                            found_input = true;
                                            is_keyboard = true;
                                        } else if et == EventType::RELATIVE {
                                            found_input = true;
                                            is_keyboard = false;
                                        }
                                    }

                                    // Drain remaining events without processing
                                    for _ in iter {}

                                    if found_input {
                                        let now = Instant::now();
                                        let gap = if is_keyboard {
                                            MIN_KEYBOARD_TRIGGER_GAP_US
                                        } else {
                                            MIN_MOUSE_TRIGGER_GAP_US
                                        };
                                        let do_trigger = match self.last_input_trigger {
                                            Some(prev) => now.saturating_duration_since(prev).as_micros() as u64 >= gap,
                                            None => true,
                                        };
                                        if do_trigger {
                                            if self.opts.prefer_napi_on_input {
                                                self.trig.trigger_input_with_napi(&mut self.skel);
                                            } else {
                                                self.trig.trigger_input(&mut self.skel);
                                            }
                                            self.last_input_trigger = Some(now);
                                        }
                                    }
                                }
                            }
                        }
                }
            }

            // Service any pending stats requests without blocking
            while stats_request_rx.try_recv().is_ok() {
                let metrics = self.get_metrics();
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
        let _ = self.struct_ops.take();
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
                // Safety: We only delete FDs that we successfully registered and haven't
                // been removed due to device disconnection. This prevents operating on
                // potentially recycled FDs. The fd is validated >= 0 during registration.
                // Cleanup path only, scoped to this delete call.
                let bfd = unsafe { std::os::fd::BorrowedFd::borrow_raw(fd) };
                let _ = ep.delete(bfd);
            }
        }
        self.registered_epoll_fds.clear();
        self.input_fd_to_idx.clear();
        self.input_devs.clear();
    }
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

    let loglevel = simplelog::LevelFilter::Info;

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

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    ctrlc::set_handler(move || {
        shutdown_clone.store(true, Ordering::Relaxed);
    })
    .context("Error setting Ctrl-C handler")?;

    let stats_thread = if let Some(intv) = opts.monitor.or(opts.stats) {
        let shutdown_copy = shutdown.clone();
        Some(std::thread::spawn(move || {
            match stats::monitor(Duration::from_secs_f64(intv), shutdown_copy) {
                Ok(_) => {}
                Err(e) => {
                    log::warn!(
                        "stats monitor thread finished because of an error {}",
                        e
                    )
                }
            }
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

    // Wait for stats thread to finish (with timeout) - only for --stats mode
    if opts.stats.is_some() {
        if let Some(jh) = stats_thread {
            info!("Waiting for stats thread to finish...");
            // Give it 1 second to finish gracefully
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
    }

    Ok(())
}
