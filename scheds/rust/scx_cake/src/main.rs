// SPDX-License-Identifier: GPL-2.0
//
// scx_cake — a clean-slate sched_ext scheduler.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

mod bpf_skel;
pub use bpf_skel::*;
pub mod bpf_intf;
pub use bpf_intf::*;

#[cfg(cake_multi_ccd)]
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::mem::MaybeUninit;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use libbpf_rs::MapCore;
use libbpf_rs::MapFlags;
use libbpf_rs::OpenObject;
use log::info;
use scx_utils::build_id;
use scx_utils::compat;
use scx_utils::scx_ops_attach;
use scx_utils::scx_ops_load;
use scx_utils::scx_ops_open;
use scx_utils::try_set_rlimit_infinity;
use scx_utils::uei_exited;
use scx_utils::uei_report;
use scx_utils::Topology;
use scx_utils::UserExitInfo;
use scx_utils::NR_CPU_IDS;

const SCHEDULER_NAME: &str = "scx_cake";

/// scx_cake: a gaming-first sched_ext scheduler — one master algorithm on
/// kernel primitives, no feature flags, no knobs, no runtime telemetry.
/// Placement is the kernel's idle-CPU pick with direct dispatch on a hit.
/// Under saturation, wakeups queue on one global vtime queue while
/// slice-expired tasks requeue on their own CPU's queue ("wakeups global,
/// continuations local"), and each CPU dispatches the earliest eligible of
/// the two. The time slice is a compile-time constant.
#[derive(Debug, Parser)]
struct Opts {
    /// Enable verbose libbpf output and runtime diagnostics. A release run
    /// without this prints only identity, attach and exit.
    #[clap(short = 'v', long, action = clap::ArgAction::SetTrue)]
    verbose: bool,

    /// Print the version and exit.
    #[clap(short = 'V', long, action = clap::ArgAction::SetTrue)]
    version: bool,
}

struct Scheduler<'a> {
    skel: BpfSkel<'a>,
    struct_ops: Option<libbpf_rs::Link>,
    /// Frame-clock incumbent bucket, held against near-ties (§R.22).
    frame_bucket: Option<u32>,
    /// Pessimistic frame estimate: fast down, slow up. Feeds the slice cap,
    /// which must not widen when the mode wobbles (§G18).
    frame_floor: u64,
    /// Print runtime diagnostics. Off by default: a release run is not a
    /// measurement session, and a scheduler logging into a game is noise.
    verbose: bool,
}

impl<'a> Scheduler<'a> {
    fn init(opts: &Opts, open_object: &'a mut MaybeUninit<OpenObject>) -> Result<Self> {
        try_set_rlimit_infinity();

        // The startup banner is the only "telemetry" cake emits, and it is
        // one-shot: version, machine shape, the compiled constants, and
        // which kernel fast paths the RUNNING kernel provides (probed from
        // BTF, not what the source requested) — so every log is
        // self-describing about what actually loaded.
        let topo = Topology::new().context("failed to read topology")?;
        let physical = topo.all_cores.len();
        let total = topo.all_cpus.len();
        let smt = total.saturating_sub(physical);

        let slice_us = bpf_intf::consts_SLICE_NS as u64 / bpf_intf::consts_NSEC_PER_USEC as u64;
        let queued_wakeup = *compat::SCX_OPS_ALLOW_QUEUED_WAKEUP != 0;
        let dsq_peek = compat::ksym_exists("scx_bpf_dsq_peek").unwrap_or(false);

        info!(
            "🍰 {} {}",
            SCHEDULER_NAME,
            build_id::full_version(env!("CARGO_PKG_VERSION"))
        );
        info!("   cores   {physical} physical + {smt} SMT = {total} CPUs");
        info!("   slice   {slice_us}µs · queues {total} per-CPU vtime + 1 global wake");
        info!(
            "   kernel  queued_wakeup {} · dsq_peek {}",
            if queued_wakeup { "on" } else { "UNSUPPORTED" },
            // cake calls scx_bpf_dsq_peek() unconditionally -- the
            // __COMPAT_ iterator arm was deleted with the other compat
            // ladders. On a kernel without the ksym the load fails
            // outright, so "MISSING" is the honest word, not "fallback".
            if dsq_peek { "native" } else { "MISSING" },
        );

        // Open the BPF program.
        let mut skel_builder = BpfSkelBuilder::default();
        skel_builder.obj_builder.debug(opts.verbose);
        let mut skel = scx_ops_open!(skel_builder, open_object, cake_ops, None)?;

        // Linux CPU numbering does not guarantee that an SMT sibling is
        // cpu +/- nr_cpus/2. Populate the immutable BPF lookup from sysfs
        // topology and cycle within wider-than-SMT2 cores. The fallback path
        // treats -1 as "no online sibling" and uses the kernel idle picker.
        let rodata = skel
            .maps
            .rodata_data
            .as_mut()
            .context("BPF rodata unavailable for CPU topology")?;

        // The CPU id span the steal ring and neighbour probe scan. It is a
        // load-time constant, so it goes in rodata (frozen before load, hence
        // folded by the verifier) instead of being written into mutable BPF
        // state from ops.init. `ops.init` refuses to attach if this is
        // narrower than the kernel's own nr_cpu_ids.
        anyhow::ensure!(
            *NR_CPU_IDS <= bpf_intf::consts_MAX_CPUS as usize,
            "host nr_cpu_ids {} exceeds Cake's compiled MAX_CPUS {}",
            *NR_CPU_IDS,
            bpf_intf::consts_MAX_CPUS
        );
        rodata.nr_cpu_span = *NR_CPU_IDS as u32;

        // Hardware-anchored thresholds: measured, never derived from SLICE_NS.
        // Clamped so a probe perturbed by host load cannot mis-tune the
        // scheduler, and logged so the value used is always on the record.
        match probe_handoff_hop_ns() {
            Some(probe) => {
                // The admission threshold IS the tail of a genuine handoff --
                // used directly, never a mean times an invented multiplier.
                // DIAGNOSTIC ONLY -- deliberately does not drive policy yet.
                //
                // Measured 2026-07-30: driving cake_handoff_max_ns from this
                // probe cost mutex-handoff -35.66% (CI[-40.01,-31.31]) with
                // migrations 2.4-2.7x up, at both `2 * mean` (1240ns) and, by
                // inspection, `p99` (798ns). The known-good threshold is
                // 1464ns = 2.34x this probe's median.
                //
                // The probe is not wrong about the hardware -- its 625ns
                // median cross-validates the independent 606ns rdtscp floor
                // to 3%. It is measuring the wrong DISTRIBUTION: a clean
                // condvar ping-pong on an idle host, where p99 sits just
                // 1.28x the median, while real handoffs under contention
                // carry lock acquisition and cache misses and run far wider.
                // That is a workload property, and no startup probe can see
                // it. Making this threshold host-adaptive needs runtime
                // observation of `used` in ops.stopping, not a boot-time
                // measurement -- same machinery as G9.7.
                let (med, p99) = (probe.median, probe.p99);
                let hm = rodata.cake_handoff_max_ns;
                if opts.verbose {
                    info!("   class   starvation = mean wait > mean burst (no threshold)");
                    info!(
                        "   probe   hop median {med}ns p99 {p99}ns (diagnostic) · handoff_max {hm}ns"
                    );
                }
            }
            None => {
                if opts.verbose {
                    log::warn!("   probe   handoff probe failed (diagnostic only)");
                }
            }
        }

        // Interrupt affinity is a host property, so it is measured rather than
        // assumed. Logged whenever it finds anything: a scheduler quietly
        // declining a CPU must always say which one and why (§G21).
        {
            let hot = probe_irq_hot_cpus(*NR_CPU_IDS);
            let flags = &mut rodata.cpu_irq_hot;
            let mut named: Vec<usize> = Vec::new();

            for (cpu, is_hot) in hot.iter().enumerate() {
                if *is_hot && cpu < flags.len() {
                    flags[cpu] = 1;
                    named.push(cpu);
                }
            }
            if !named.is_empty() {
                info!("   irq     interrupt-sink CPUs {named:?} — deprioritised for placement");
            } else if opts.verbose {
                info!("   irq     no interrupt sink found; placement unrestricted");
            }
        }

        let siblings = &mut rodata.cpu_sibling;
        siblings.fill(-1);
        for core in topo.all_cores.values() {
            let cpu_ids: Vec<usize> = core.cpus.keys().copied().collect();

            if cpu_ids.len() < 2 {
                continue;
            }
            for (idx, cpu) in cpu_ids.iter().copied().enumerate() {
                let sibling = cpu_ids[(idx + 1) % cpu_ids.len()];

                anyhow::ensure!(
                    cpu < siblings.len() && sibling <= i32::MAX as usize,
                    "CPU topology id exceeds Cake's compiled sibling map"
                );
                siblings[cpu] = sibling as i32;
            }
        }

        // Bootstrap both ends of the display band rather than a hard-coded
        // refresh rate, and bootstrap each in its SAFE direction. The clock
        // starts slow so occupant protection is never divided into a zero; the
        // FLOOR starts fast so the slice cap begins tight and relaxes only as
        // evidence arrives. A 1/60 s floor was 4x too loose on this 240 Hz
        // display -- the wrong direction for a bound (§G18, §G19).
        if let Some(bss) = skel.maps.bss_data.as_mut() {
            bss.cake_frame_ns = bpf_intf::consts_FRAME_PERIOD_MAX_NS as u64;
            bss.cake_frame_floor_ns = bpf_intf::consts_FRAME_PERIOD_MIN_NS as u64;
        }

        #[cfg(cake_multi_ccd)]
        {
            let rodata = skel
                .maps
                .rodata_data
                .as_mut()
                .context("BPF rodata unavailable for cache topology")?;
            let order = &mut rodata.cpu_steal_order;
            let nr_span = bpf_intf::consts_BUILD_NR_CPUS as usize;
            anyhow::ensure!(
                topo.all_cpus
                    .keys()
                    .next_back()
                    .is_none_or(|cpu| *cpu < nr_span),
                "runtime CPU topology exceeds Cake's build-host CPU span"
            );
            order.fill(0);

            let llc_cache: BTreeMap<usize, usize> = topo
                .all_llcs
                .iter()
                .map(|(id, llc)| {
                    (
                        *id,
                        llc.all_cpus
                            .values()
                            .map(|cpu| cpu.cache_size)
                            .max()
                            .unwrap_or(0),
                    )
                })
                .collect();
            let policy = bpf_intf::consts_CCD_STEAL_POLICY;

            for src in topo.all_cpus.values() {
                let mut candidates: Vec<_> = topo
                    .all_cpus
                    .values()
                    .filter(|dst| dst.id != src.id)
                    .collect();
                candidates.sort_by_key(|dst| {
                    let class = if dst.llc_id == src.llc_id {
                        0
                    } else if policy > 1 && llc_cache[&dst.llc_id] == llc_cache[&src.llc_id] {
                        1
                    } else {
                        2
                    };
                    (class, (dst.id + nr_span - src.id) % nr_span)
                });
                let base = src.id * nr_span;
                for (slot, dst) in candidates.into_iter().enumerate() {
                    order[base + slot] = dst.id as u16;
                }
            }
        }

        // Load and attach.
        let mut skel = scx_ops_load!(skel, cake_ops, uei)?;
        let struct_ops = Some(scx_ops_attach!(skel, cake_ops)?);

        info!("🍰 attached — wakeups queue globally, continuations locally");

        // The file capabilities (cap_bpf,cap_perfmon,cap_sys_nice) are only
        // needed to load and attach; detach and map access use already-open
        // fds. Drop them all: while any elevated capability stays in the
        // permitted set, the kernel's ptrace access check denies
        // /proc/<pid>/exe to unprivileged observers even with dumpable
        // restored, which breaks the sudoless bench runner's hash-of-exe
        // identity verification (and holding dead privileges is bad hygiene).
        drop_all_capabilities();

        Ok(Self {
            skel,
            struct_ops,
            frame_bucket: None,
            frame_floor: 0,
            verbose: opts.verbose,
        })
    }

    fn exited(&mut self) -> bool {
        uei_exited!(&self.skel, uei)
    }

    fn run(&mut self, shutdown: Arc<AtomicBool>) -> Result<UserExitInfo> {
        // Frame clock, reported under --verbose on a material change only, so
        // a moving line there means the observed cadence really moved (§G11).
        let mut shown: u64 = 0;
        while !shutdown.load(Ordering::Relaxed) && !self.exited() {
            std::thread::sleep(Duration::from_secs(1));

            let observed = self.publish_frame_clock();
            if self.verbose && observed != 0 && observed.abs_diff(shown) > shown / 16 {
                shown = observed;
                info!(
                    "   frame   observed period {}us ({:.1} Hz)",
                    observed / 1000,
                    1e9 / observed as f64
                );
            }
        }

        self.struct_ops.take();
        info!("🍰 {SCHEDULER_NAME} detached — default scheduler restored");
        self.report_diag();
        uei_report!(&self.skel, uei)
    }

    /// DIAGNOSTIC (to be reverted): G24 census dump — raw counters at detach,
    /// summed across CPUs; rates are derived offline against the denominators.
    fn report_diag(&self) {
        const NAMES: [&str; 29] = [
            "selcpu",
            "enq",
            "dispatch",
            "notify",
            "serial_taken",
            "home_taken",
            "ranked_idle",
            "dfl_idle",
            "r6_try",
            "r6_repick",
            "qmark_try",
            "qmark_defer",
            "syncgate_try",
            "syncgate_lag",
            "syncgate_waker",
            "kt_idle",
            "enq_starve",
            "g17_divert",
            "pinned_try",
            "pinned_kick",
            "qmark_setw",
            "qmark_clrw",
            "steal_try",
            "steal_hit",
            "sib_kick",
            "pick_kick",
            "notify_preempt",
            "neigh_run",
            "neigh_hit",
        ];
        let map = &self.skel.maps.cake_diag;
        for (i, name) in NAMES.iter().enumerate() {
            let key = (i as u32).to_ne_bytes();
            let total: u64 = match map.lookup_percpu(&key, MapFlags::ANY) {
                Ok(Some(vals)) => vals
                    .iter()
                    .map(|v| u64::from_ne_bytes(v.as_slice().try_into().unwrap_or([0; 8])))
                    .sum(),
                _ => 0,
            };
            info!("   diag    {name:<16} {total}");
        }
    }

    /// Take the winning cadence out of the vote histogram and publish it.
    ///
    /// A game drives many threads off one frame loop, so the frame period is
    /// the bucket the most wakeups landed in; the bucket's own sum makes the
    /// published value exact rather than a bucket centre. Argmax lives here
    /// rather than in BPF — derive in the loader, compare in the BPF — and
    /// each bucket is cleared as it is read, so a stale frame rate cannot
    /// keep winning. Returns the published period, 0 when there are no votes.
    fn publish_frame_clock(&mut self) -> u64 {
        const WIDTH: usize = std::mem::size_of::<u64>() * 2;
        // A challenger must clearly win, not narrowly win. Several real
        // cadences coexist on a desktop -- a second engine or an editor is a
        // legitimately large crowd -- and without a switching cost the winner
        // alternates between them every poll.
        const DISPLACE: u64 = 2;

        let hist = &self.skel.maps.cake_frame_hist;
        let (mut best_bucket, mut best_count, mut best_sum) = (0u32, 0u64, 0u64);
        let (mut held_count, mut held_sum) = (0u64, 0u64);

        for idx in 0..bpf_intf::consts_FRAME_BUCKETS as u32 {
            let key = idx.to_ne_bytes();
            let Ok(Some(percpu)) = hist.lookup_percpu(&key, MapFlags::ANY) else {
                continue;
            };
            let (mut count, mut sum) = (0u64, 0u64);
            for cpu in &percpu {
                if cpu.len() < WIDTH {
                    continue;
                }
                count += u64::from_ne_bytes(cpu[..8].try_into().unwrap());
                sum += u64::from_ne_bytes(cpu[8..16].try_into().unwrap());
            }
            if count == 0 {
                continue;
            }
            if count > best_count {
                (best_bucket, best_count, best_sum) = (idx, count, sum);
            }
            if self.frame_bucket == Some(idx) {
                (held_count, held_sum) = (count, sum);
            }
            let zeroed = vec![vec![0u8; WIDTH]; percpu.len()];
            let _ = hist.update_percpu(&key, &zeroed, MapFlags::ANY);
        }

        // Keep the incumbent unless it fell silent or was clearly beaten.
        let (period, bucket) = if held_count > 0 && best_count < held_count * DISPLACE {
            (held_sum / held_count, self.frame_bucket)
        } else if best_count > 0 {
            (best_sum / best_count, Some(best_bucket))
        } else {
            return 0;
        };

        // A bound takes the pessimistic side of a noisy estimate: drop to a new
        // low at once, climb back over ~16 polls. One 17372us excursion off a
        // true 3621us previously doubled the slice cap (§G18).
        self.frame_floor = match self.frame_floor {
            0 => period,
            f if period < f => period,
            f => f + (period - f) / 16,
        };
        self.frame_bucket = bucket;
        if let Some(bss) = self.skel.maps.bss_data.as_mut() {
            bss.cake_frame_ns = period;
            bss.cake_frame_floor_ns = self.frame_floor;
        }
        period
    }
}

/// Re-execute this image to service a kernel-requested restart.
///
/// `drop_all_capabilities()` runs after every successful attach, and a
/// permitted set cleared through `capset` cannot be regained in-process —
/// file capabilities are applied only at `execve`. Re-entering
/// `Scheduler::init` would therefore fail `BPF_PROG_LOAD` with `EPERM` and
/// kill the scheduler instead of restarting it. Exec'ing ourselves restores
/// the file capabilities from the bounding set (which `capset` never
/// touched), keeps the PID and `/proc/<pid>/exe` identity the bench runner
/// verifies, and re-runs `PR_SET_DUMPABLE` on the way in. The struct_ops link
/// is already dropped by `run()`, so the scheduler is detached before we
/// replace the image.
/// One host's wake+block+switch hop, as a distribution rather than a mean.
struct HandoffProbe {
    /// Typical hop — the switch-cost anchor for the preempt-protect window.
    median: u64,
    /// Tail of a GENUINE handoff. This is the admission threshold itself, not
    /// a base to multiply: measured 2026-07-30, using `2 * mean` instead cost
    /// mutex-handoff -35.66% (CI[-40.01,-31.31]) with migrations 2.4-2.7x up,
    /// because enough real handoffs land in the tail above the mean that a
    /// mean-derived cut loses them. The in-tree dose-response wanted a 99.67%
    /// firing rate on this workload, so the statistic that matches the
    /// requirement is a high percentile, not an invented multiplier.
    p99: u64,
}

/// Which CPUs are interrupt sinks on THIS host.
///
/// IRQ affinity is a property of the machine, not of any policy value, so it is
/// measured here rather than guessed at or hard-coded -- on the development
/// host three device interrupts are 100%-pinned to their own CPUs (nvidia, USB,
/// NIC), and the nvidia one cost the game's main thread a 116x within-CPU wake
/// penalty (§G21).
///
/// Judged per IRQ LINE, never per CPU total: a fair-share test on totals is
/// dominated by the loudest source, so it can only ever flag the loudest sink
/// and missed two of the three on this host (§G23). Two samples give a RATE;
/// since-boot totals would keep flagging a CPU whose interrupt source has long
/// since moved. A line marks its CPU when it fires at least once per
/// millisecond -- sparser cannot account for microsecond-scale wake delays --
/// AND lands almost entirely on one CPU, which is what pinned affinity produces
/// by construction. Returns an empty set rather than a half-measured one if
/// anything about the probe looks wrong.
fn probe_irq_hot_cpus(nr_cpus: usize) -> Vec<bool> {
    /// Below one interrupt per millisecond a line is too sparse to matter.
    const MIN_LINE_RATE_HZ: f64 = 1_000.0;
    /// A pinned interrupt is 100% on one CPU by construction; the slack
    /// tolerates an affinity move mid-window.
    const SINK_CONCENTRATION: f64 = 0.95;
    const WINDOW: Duration = Duration::from_millis(120);

    let none = vec![false; nr_cpus];
    let Some((cols_a, a)) = read_irq_lines(nr_cpus) else {
        return none;
    };
    std::thread::sleep(WINDOW);
    let Some((cols_b, b)) = read_irq_lines(nr_cpus) else {
        return none;
    };
    // A hotplug mid-window shifts what every column means.
    if cols_a != cols_b {
        return none;
    }

    let secs = WINDOW.as_secs_f64();
    let mut hot = vec![false; nr_cpus];
    for (label, (before, _)) in &a {
        let Some((after, desc)) = b.get(label) else {
            continue;
        };
        // Only full per-CPU rows can attribute a stream to a CPU; ERR/MIS
        // style single-total rows cannot.
        if before.len() != cols_a.len() || after.len() != cols_a.len() {
            continue;
        }

        let deltas: Vec<u64> = before
            .iter()
            .zip(after.iter())
            .map(|(x, y)| y.saturating_sub(*x))
            .collect();
        let total: u64 = deltas.iter().sum();
        if (total as f64) < MIN_LINE_RATE_HZ * secs {
            continue;
        }

        let Some((peak_col, peak)) = deltas.iter().copied().enumerate().max_by_key(|(_, d)| *d)
        else {
            continue;
        };
        if peak as f64 >= SINK_CONCENTRATION * total as f64 {
            let cpu = cols_a[peak_col];
            if !hot[cpu] {
                hot[cpu] = true;
                info!(
                    "   irq     {label}: {desc} pins CPU {cpu} at {:.0}/s",
                    total as f64 / secs
                );
            }
        }
    }

    // A probe perturbed by host load must never mis-tune the scheduler: if it
    // wants to condemn half the machine it has measured something other than
    // interrupt affinity, so condemn nothing.
    if hot.iter().filter(|h| **h).count() * 2 >= nr_cpus {
        return none;
    }
    hot
}

/// Per-line interrupt counts from `/proc/interrupts`, plus the header's
/// column-to-CPU map.
///
/// Columns are mapped by the header's `CPU<n>` labels, never by position: with
/// any CPU offline the columns are not contiguous, and a positional map would
/// attribute a sink to the wrong CPU. Each row keeps its last field (the device
/// name) so a flagged sink can be named.
fn read_irq_lines(nr_cpus: usize) -> Option<(Vec<usize>, HashMap<String, (Vec<u64>, String)>)> {
    let text = std::fs::read_to_string("/proc/interrupts").ok()?;
    let mut lines = text.lines();
    let cols = lines
        .next()?
        .split_whitespace()
        .map(|h| h.strip_prefix("CPU").and_then(|n| n.parse::<usize>().ok()))
        .collect::<Option<Vec<usize>>>()?;
    if cols.is_empty() || cols.iter().any(|c| *c >= nr_cpus) {
        return None;
    }

    let mut rows = HashMap::new();
    for line in lines {
        let fields: Vec<&str> = line.split_whitespace().collect();
        let Some(label) = fields.first() else {
            continue;
        };
        // Row label, e.g. "115:" or "LOC:".
        if !label.ends_with(':') {
            continue;
        }

        let mut counts = Vec::with_capacity(cols.len());
        for f in &fields[1..] {
            if counts.len() == cols.len() {
                break;
            }
            match f.parse::<u64>() {
                Ok(v) => counts.push(v),
                // Trailing type/description columns are not counts.
                Err(_) => break,
            }
        }
        let desc = fields.last().copied().unwrap_or("").to_string();
        rows.insert(label.trim_end_matches(':').to_string(), (counts, desc));
    }
    Some((cols, rows))
}

/// Measure one wake + block + switch hop on THIS host.
///
/// Two threads ping-pong through a condvar, which exercises exactly the path
/// `cake_handoff_max_ns` is a proxy for: a blocking handoff between two
/// tasks. It was previously written as a divisor of `SLICE_NS` (/2048), which
/// made it wrong at any other slice value and wrong on any other CPU -- at a
/// 1 ms slice it evaluated to 488 ns, below this host's ~606 ns sleep floor, so
/// nothing could qualify and the co-location gate silently disabled itself.
///
/// Runs before the scheduler attaches, so it measures the stock kernel path.
/// ~2200 round trips, a few milliseconds of startup. Returns nanoseconds per
/// one-way hop, or None if the probe could not complete -- callers keep the
/// compiled-in defaults rather than trusting a failed measurement.
fn probe_handoff_hop_ns() -> Option<HandoffProbe> {
    const WARMUP: u32 = 200;
    const ITERS: u32 = 2_000;
    let total = WARMUP + ITERS;

    let pair = Arc::new((std::sync::Mutex::new(0u32), std::sync::Condvar::new()));
    let peer = Arc::clone(&pair);

    let responder = std::thread::Builder::new()
        .name("cake-probe".into())
        .spawn(move || {
            let (lock, cv) = &*peer;
            let mut turn = lock.lock().ok()?;
            for _ in 0..total {
                while *turn % 2 == 0 {
                    turn = cv.wait(turn).ok()?;
                }
                *turn = turn.wrapping_add(1);
                cv.notify_one();
            }
            Some(())
        })
        .ok()?;

    let mut hops: Vec<u64> = Vec::with_capacity(ITERS as usize);
    {
        let (lock, cv) = &*pair;
        let mut turn = lock.lock().ok()?;
        for i in 0..total {
            let t0 = std::time::Instant::now();
            *turn = turn.wrapping_add(1);
            cv.notify_one();
            while *turn % 2 == 1 {
                turn = cv.wait(turn).ok()?;
            }
            if i >= WARMUP {
                // Two hops per round trip: our wake of the peer, its wake of us.
                hops.push(t0.elapsed().as_nanos() as u64 / 2);
            }
        }
    }
    responder.join().ok()?;

    if hops.len() < ITERS as usize / 2 {
        return None;
    }
    hops.sort_unstable();
    let median = hops[hops.len() / 2];
    let p99 = hops[hops.len() * 99 / 100];
    if median == 0 || p99 == 0 {
        return None;
    }
    Some(HandoffProbe { median, p99 })
}

fn reexec_self() -> Result<()> {
    use std::os::unix::process::CommandExt;

    let exe = std::fs::read_link("/proc/self/exe")
        .context("failed to resolve /proc/self/exe for restart")?;
    /* exec() only returns on failure. */
    let err = std::process::Command::new(exe)
        .args(std::env::args_os().skip(1))
        .exec();

    Err(anyhow::Error::new(err).context("re-exec after kernel restart request failed"))
}

/// Clear the effective, permitted, and inheritable capability sets.
fn drop_all_capabilities() {
    #[repr(C)]
    struct CapHeader {
        version: u32,
        pid: i32,
    }
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct CapData {
        effective: u32,
        permitted: u32,
        inheritable: u32,
    }
    const LINUX_CAPABILITY_VERSION_3: u32 = 0x2008_0522;
    let hdr = CapHeader {
        version: LINUX_CAPABILITY_VERSION_3,
        pid: 0,
    };
    let data = [CapData {
        effective: 0,
        permitted: 0,
        inheritable: 0,
    }; 2];
    let rc = unsafe { libc::syscall(libc::SYS_capset, &hdr, data.as_ptr()) };
    if rc != 0 {
        log::warn!("capset drop failed ({})", std::io::Error::last_os_error());
    }
}

fn main() -> Result<()> {
    // File capabilities (cap_bpf,cap_perfmon,cap_sys_nice) clear the dumpable
    // flag, which makes /proc/self/exe root-only and defeats the sudoless
    // bench runner's process-identity verification (hash of /proc/<pid>/exe).
    // The binary holds no secrets, so restore normal /proc introspection.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 1, 0, 0, 0);
    }

    let opts = Opts::parse();

    if opts.version {
        println!(
            "{} {}",
            SCHEDULER_NAME,
            build_id::full_version(env!("CARGO_PKG_VERSION"))
        );
        return Ok(());
    }

    let mut lcfg = simplelog::ConfigBuilder::new();
    lcfg.set_time_level(simplelog::LevelFilter::Error)
        .set_location_level(simplelog::LevelFilter::Off)
        .set_target_level(simplelog::LevelFilter::Off)
        .set_thread_level(simplelog::LevelFilter::Off);
    simplelog::TermLogger::init(
        simplelog::LevelFilter::Info,
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

    let mut open_object = MaybeUninit::uninit();
    let mut sched = Scheduler::init(&opts, &mut open_object)?;

    if sched.run(shutdown.clone())?.should_restart() {
        info!("🍰 restart requested by the kernel — re-executing");
        reexec_self()?;
    }

    Ok(())
}
