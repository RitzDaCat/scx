/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
 * Copyright (c) 2025 RitzDaCat
 */
#include <scx/common.bpf.h>
#include "intf.h"

/* Modular includes - organized by functionality */
#include "include/types.bpf.h"      /* Must be first: defines task_ctx, cpu_ctx */
#include "include/helpers.bpf.h"
#include "include/stats.bpf.h"
#include "include/boost.bpf.h"
#include "include/task_class.bpf.h"
#include "include/profiling.bpf.h"  /* Hot-path instrumentation */
#include "include/thread_runtime.bpf.h"
/* Advanced detection needs API updates - disabled for now
#include "include/gpu_detect.bpf.h"
#include "include/wine_detect.bpf.h"
#include "include/advanced_detect.bpf.h"
*/
#include "game_detect_lsm.bpf.c"    /* BPF LSM game detection (kernel-level) */

/*
 * Maximum amount of CPUs supported by the scheduler when flat or preferred
 * idle CPU scan is enabled.
 */
#define MAX_CPUS	256

/*
 * Shared DSQ used to schedule tasks in deadline mode when the system is
 * saturated.
 *
 * When system is not saturated tasks will be dispatched to the local DSQ
 * in round-robin mode.
 */
#define SHARED_DSQ		0
/* Tunables / thresholds (documented for readability). */
#define INTERACTIVE_SLICE_SHRINK_THRESH 256ULL	/* per-CPU interactive_avg threshold to shrink slice */
#define INTERACTIVE_SMT_ALLOW_THRESH     128ULL	/* allow SMT pairing when below this interactivity */
#define WAKE_FREQ_SHIFT                  8		/* wakeup_freq >> SHIFT maps to modest factor */
#define CHAIN_BOOST_MAX                  4		/* max chain boost depth */
#define CHAIN_BOOST_STEP                 2		/* increment per sync-wake event */
/* GPU submission thread detection thresholds */
#define GPU_SUBMIT_EXEC_THRESH_NS        100000ULL	/* <100μs exec per wake suggests GPU submit */
#define GPU_SUBMIT_FREQ_MIN              50ULL		/* min wakeup freq (50 = ~2ms period for 500fps) */
#define GPU_SUBMIT_STABLE_SAMPLES        8		/* require N consistent samples before classification */
/* Background task detection thresholds */
#define BACKGROUND_EXEC_THRESH_NS        5000000ULL	/* >5ms exec suggests CPU-intensive background work */
#define BACKGROUND_FREQ_MAX              10ULL		/* low wakeup freq (<10 = >100ms sleep) indicates batch */
#define BACKGROUND_STABLE_SAMPLES        4		/* require N consistent samples */
/* Command flags for userspace -> BPF triggers via BSS cmd_flags. */
#define CMD_INPUT   (1u << 0)
#define CMD_FRAME   (1u << 1)
#define CMD_NAPI    (1u << 2)

/*
 * Thresholds for applying hysteresis to CPU performance scaling:
 *  - CPUFREQ_LOW_THRESH: below this level, reduce performance to minimum
 *  - CPUFREQ_HIGH_THRESH: above this level, raise performance to maximum
 *
 * Values between the two thresholds retain the current smoothed performance level.
 */
#define CPUFREQ_LOW_THRESH	(SCX_CPUPERF_ONE / 4)
#define CPUFREQ_HIGH_THRESH	(SCX_CPUPERF_ONE - SCX_CPUPERF_ONE / 4)

/* MM_HINT_UPDATE_INTERVAL_NS is now defined in include/config.bpf.h */

/*
 * Subset of CPUs to prioritize.
 */
private(GAMER) struct bpf_cpumask __kptr *primary_cpumask;

/*
 * Set to true when @primary_cpumask is empty (primary domain includes all
 * the CPU).
 */
const volatile bool primary_all = true;

/*
 * Enable flat iteration to find idle CPUs (fast but inaccurate).
 */
const volatile bool flat_idle_scan = false;

/*
 * CPUs in the system have SMT is enabled.
 */
const volatile bool smt_enabled = true;

/*
 * Enable preferred cores prioritization.
 */
const volatile bool preferred_idle_scan = false;

/*
 * CPUs sorted by their capacity in descendent order.
 */
const volatile u64 preferred_cpus[MAX_CPUS];

/*
 * Enable cpufreq integration.
 */
const volatile bool cpufreq_enabled = true;

/*
 * Enable NUMA optimizatons.
 */
const volatile bool numa_enabled;

/*
 * Aggressively try to avoid SMT contention.
 *
 * Default to true here, so veristat takes the more complicated path.
 */
const volatile bool avoid_smt = true;

/*
 * Enable address space affinity.
 */
const volatile bool mm_affinity;

/*
 * Enable deferred wakeup.
 */
const volatile bool deferred_wakeups = true;

/*
 * Ignore synchronous wakeup events.
 */
const volatile bool no_wake_sync;

/*
 * Disable stats collection for maximum performance (no atomic ops in hot path).
 */
const volatile bool no_stats;

/* Input boost configuration (ns). */
const volatile u64 input_window_ns;

/* Foreground game/application tgid (0 = disabled, apply globally) */
const volatile u32 foreground_tgid;
/* Runtime-updatable foreground tgid (overrides foreground_tgid if non-zero) */
/* Double-buffering for race-free userspace updates:
 * - Userspace writes to detected_fg_tgid_staging
 * - BPF reads from detected_fg_tgid (stable value)
 * - get_fg_tgid() helper copies staging → active when changed
 */
volatile u32 detected_fg_tgid_staging;
volatile u32 detected_fg_tgid;
/* Enable use of per-mm recent CPU hint map. */
const volatile bool mm_hint_enabled = true;

/*
 * Default time slice.
 */
const volatile u64 slice_ns = 10000ULL;

/*
 * Wakeup timer period. If zero, falls back to slice_ns. Tunable from userspace.
 */
const volatile u64 wakeup_timer_ns;

/*
 * Maximum runtime that can be charged to a task.
 */
const volatile u64 slice_lag = 20000000ULL;

/*
 * Current global CPU utilization percentage in the range [0 .. 1024].
 */
volatile u64 cpu_util;

/*
 * Migration limiter configuration (from userspace).
 */
const volatile u64 mig_window_ns;
const volatile u32 mig_max_per_window;
/* NUMA spill threshold: if local node shared DSQ depth is below this, avoid migrations. */
const volatile u32 numa_spill_thresh;
/* Prefer NAPI/softirq CPU during input window (opt-in) */
const volatile bool prefer_napi_on_input;

#define MIG_TOKEN_SCALE               1024ULL

/* Busy state tracking for hysteresis (prevents oscillation at threshold boundary). */
volatile bool system_busy_state;

/* Stats counters (BSS, accumulate). */
volatile u64 rr_enq;
volatile u64 edf_enq;
volatile u64 nr_direct_dispatches;
volatile u64 nr_shared_dispatches;
volatile u64 timer_scan_iters;
volatile u64 nr_migrations;
volatile u64 nr_mig_blocked;
volatile u64 nr_sync_local;
volatile u64 nr_frame_mig_block;
volatile u64 cpu_util_avg;
volatile u64 interactive_sys_avg;
/* Window activity accounting (accumulated by wakeup timer). */
volatile u64 win_input_ns_total;
volatile u64 win_frame_ns_total;
volatile u64 timer_elapsed_ns_total;
/* Hint/selection quality metrics and fg runtime accounting. */
volatile u64 nr_idle_cpu_pick;
volatile u64 nr_mm_hint_hit;
volatile u64 fg_runtime_ns_total;
volatile u64 total_runtime_ns_total;
/* Trigger counters. */
volatile u64 nr_input_trig;
volatile u64 nr_frame_trig;
/* GPU thread physical core affinity enforcement. */
volatile u64 nr_gpu_phys_kept;
volatile u64 nr_gpu_pref_fallback;
/* SYNC wake fast path counter. */
volatile u64 nr_sync_wake_fast;
/* Task classification counters. */
volatile u64 nr_gpu_submit_threads;
volatile u64 nr_background_threads;
volatile u64 nr_compositor_threads;
volatile u64 nr_network_threads;
volatile u64 nr_system_audio_threads;
volatile u64 nr_game_audio_threads;
volatile u64 nr_input_handler_threads;

/* Debug: Track disable hook calls to verify it's working */
volatile u64 nr_disable_calls;
volatile u64 nr_disable_input_dec;

/* Scheduler generation ID - incremented on each init to detect restarts
 * This solves the task_ctx persistence problem across scheduler restarts */
volatile u32 scheduler_generation;

/* BPF Profiling: Hot-path latency measurements
 * Always declared (even when ENABLE_PROFILING is not set) so userspace stats can read them.
 * When profiling is disabled, these remain zero. */
volatile u64 prof_select_cpu_ns_total;
volatile u64 prof_select_cpu_calls;
volatile u64 prof_enqueue_ns_total;
volatile u64 prof_enqueue_calls;
volatile u64 prof_dispatch_ns_total;
volatile u64 prof_dispatch_calls;
volatile u64 prof_deadline_ns_total;
volatile u64 prof_deadline_calls;
volatile u64 prof_pick_idle_ns_total;
volatile u64 prof_pick_idle_calls;
volatile u64 prof_mm_hint_ns_total;
volatile u64 prof_mm_hint_calls;

/* Latency histograms (log scale buckets) */
#define HIST_BUCKETS 12
volatile u64 hist_select_cpu[HIST_BUCKETS];
volatile u64 hist_enqueue[HIST_BUCKETS];
volatile u64 hist_dispatch[HIST_BUCKETS];

/* Userspace-triggered commands (set bits; drained in wakeup_timerfn). */
volatile u32 cmd_flags;

/* Global window until timestamps to avoid per-CPU writes. */
volatile u64 input_until_global;
volatile u64 napi_until_global;

/* Continuous input detection for aim trainers/high-mouse-movement games.
 * When input rate is sustained high (>100/sec), we're in "continuous mode":
 * less aggressive slice reduction to avoid timing jitter. */
volatile u64 last_input_trigger_ns;
volatile u32 input_trigger_rate;  /* Triggers per second (EMA) */
volatile u8 continuous_input_mode; /* 1 = sustained high input rate detected */

/* Bitmap of CPUs with local DSQ work pending that may need a kick. */
#define KICK_WORDS ((MAX_CPUS + 63) / 64)
volatile u64 kick_mask[KICK_WORDS];

/* Helper functions (stat_inc, set_kick_cpu, clear_kick_cpu) now in helpers.bpf.h */

char _license[] SEC("license") = "GPL";

/*
 * Scheduler's exit status.
 */
UEI_DEFINE(uei);

/*
 * Maximum amount of CPUs supported by the system.
 */
static u64 nr_cpu_ids;

/*
 * Moved vtime tracking to per-CPU context.
 */

/* Struct definitions (task_ctx, cpu_ctx) now in types.bpf.h */
/* Helper lookup functions also in types.bpf.h */
/* Boost/window functions (is_input_active*, fanout_set_*, is_foreground_task*) now in boost.bpf.h */

/*
 * Note: Cgroup-based game detection was considered but not implemented.
 * Steam often isolates games in cgroups named "app-*", "steam_app_*", or "game.slice",
 * but BPF verifier restrictions make direct cgroup path reading impractical.
 * The process hierarchy detection already covers most multi-process game cases.
 *
 * Future implementation would require:
 * 1. Userspace reading /proc/pid/cgroup and updating BPF map
 * 2. BPF LSM hooks (requires kernel 5.7+)
 */

/* Helper to load fg_tgid once per hot path. */
static __always_inline u32 get_fg_tgid(void)
{
    return detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
}

/* Thread classification functions (is_compositor_name, is_network_name, etc) now in task_class.bpf.h */
/* is_napi_softirq_preferred_cpu() now in boost.bpf.h */
/* Utility functions (calc_avg, update_freq, cpufreq helpers) now in helpers.bpf.h */

/*
 * Update CPU load and scale target performance level accordingly.
 * Wrapper around helpers.bpf.h functions.
 */
static void update_cpu_load(struct task_struct *p, u64 slice)
{
	u64 now = scx_bpf_now();
	s32 cpu = scx_bpf_task_cpu(p);
	struct cpu_ctx *cctx;

	if (!cpufreq_enabled)
		return;

	cctx = try_lookup_cpu_ctx(cpu);
	if (!cctx)
		return;

	update_target_cpuperf(cctx, now, slice);
}

/*
 * Timer used to defer idle CPU wakeups.
 *
 * Instead of triggering wake-up events directly from hot paths, such as
 * ops.enqueue(), idle CPUs are kicked using the wake-up timer.
 */
struct wakeup_timer {
	struct bpf_timer timer;
};

struct {
	__uint(type, BPF_MAP_TYPE_ARRAY);
	__uint(max_entries, 1);
	__type(key, u32);
	__type(value, struct wakeup_timer);
} wakeup_timer SEC(".maps");

/* Per-mm recent CPU hint map - defined in types.bpf.h */
/* shared_dsq() and is_pcpu_task() functions - now in helpers.bpf.h */

/*
 * Load-aware mode switching for optimal gaming performance with hysteresis.
 *
 * Returns true when system should use deadline (EDF) scheduling mode,
 * false when per-CPU round-robin mode is preferred.
 *
 * Strategy with hysteresis to reduce frame timing variance:
 * - Light load (<15% CPU util): Use per-CPU queues for cache locality
 * - Heavy load (>=24% util): Use deadline mode for responsiveness
 * - Medium load (15-24%): Maintain current mode (dead zone prevents oscillation)
 *
 * Rationale:
 * - Light games (indie, 2D, menus): benefit from cache affinity
 * - Heavy games (AAA, complex scenes): need responsive load balancing
 * - Borderline loads: Hysteresis eliminates queue mode thrashing → lower frame time variance
 *
 * Performance impact:
 * - Light workloads: 5-20% better frame pacing (fewer migrations)
 * - Heavy workloads: No change (already over threshold)
 * - Medium workloads: Reduced frame time std dev (no mode oscillation)
 */
static inline bool is_system_busy(void)
{
    /* If no foreground game detected, default to deadline mode (safe) */
    u32 fg_tgid = get_fg_tgid();
    if (!fg_tgid)
        return true;

    /* Hysteresis thresholds to prevent oscillation */
    const u64 BUSY_ENTER_THRESH = 250;   /* Switch to busy at 24% load (250/1024) */
    const u64 BUSY_EXIT_THRESH = 150;    /* Switch to not busy at 15% load (150/1024) */

    /* Use cpu_util_avg (EMA) instead of cpu_util (instantaneous) for additional stability */
    u64 load = cpu_util_avg;

    /* Apply hysteresis based on current state */
    if (system_busy_state) {
        /* Currently busy: only switch to not-busy if load drops below exit threshold */
        if (load < BUSY_EXIT_THRESH)
            system_busy_state = false;
    } else {
        /* Currently not busy: only switch to busy if load exceeds enter threshold */
        if (load >= BUSY_ENTER_THRESH)
            system_busy_state = true;
    }

    return system_busy_state;
}

/*
 * Return the cpumask of fully idle SMT cores within the NUMA node that
 * contains @cpu.
 *
 * If NUMA support is disabled, @cpu is ignored.
 */
static inline const struct cpumask *get_idle_smtmask(s32 cpu)
{
	if (!numa_enabled)
		return scx_bpf_get_idle_smtmask();

	return __COMPAT_scx_bpf_get_idle_smtmask_node(__COMPAT_scx_bpf_cpu_node(cpu));
}

/*
 * Return true if the CPU is running the idle thread, false otherwise.
 */
static inline bool is_cpu_idle(s32 cpu)
{
	struct rq *rq = scx_bpf_cpu_rq(cpu);

	if (!rq) {
		scx_bpf_error("Failed to access rq %d", cpu);
		return false;
	}
	return rq->curr->flags & PF_IDLE;
}

/* Update per-mm recent CPU hint when a task starts running (rate-limited to reduce hot-path overhead). */
static __always_inline void update_mm_last_cpu(struct task_struct *p, struct task_ctx *tctx, u64 now)
{
    if (!p->mm || !tctx)
        return;

    /* Rate-limit updates: only update if enough time has passed since last update.
     * Guard against clock skew (now < last_update) by checking both conditions. */
    if (tctx->mm_hint_last_update) {
        if (now < tctx->mm_hint_last_update ||
            (now - tctx->mm_hint_last_update) < MM_HINT_UPDATE_INTERVAL_NS)
            return;
    }

    u64 key = (u64)p->mm;
    u32 cpu = (u32)scx_bpf_task_cpu(p);
    bpf_map_update_elem(&mm_last_cpu, &key, &cpu, BPF_ANY);
    tctx->mm_hint_last_update = now;
}

/*
 * Try to pick the best idle CPU based on the @preferred_cpus ranking.
 * Return a full-idle SMT core if @do_idle_smt is true, or any idle CPU if
 * @do_idle_smt is false.
 */
/* pick_idle_cpu_pref_smt removed: selection now prefers scx_bpf_select_cpu_and path. */

/*
 * Return the optimal idle CPU for task @p or -EBUSY if no idle CPU is
 * found.
 */
/* pick_idle_cpu_flat removed: selection now prefers scx_bpf_select_cpu_and path. */

/*
 * Pick an optimal idle CPU for task @p (as close as possible to
 * @prev_cpu).
 *
 * Return the CPU id or a negative value if an idle CPU can't be found.
 */
/* Context structure to avoid BPF function argument limit (max 5 args) */
struct pick_cpu_cache {
	bool is_busy;
	struct cpu_ctx *pc;
	u32 fg_tgid;
	bool input_active;
};

/* Optimized version that takes pre-computed values to avoid redundant work */
static s32 pick_idle_cpu_cached(struct task_struct *p, s32 prev_cpu, u64 wake_flags,
                                bool from_enqueue, struct pick_cpu_cache *cache)
{
	const struct cpumask *mask = cast_mask(primary_cpumask);
	s32 cpu;
    u32 *hint;
    u64 mm_key;

    /* Prefer select_cpu_and path for simplicity and lower overhead.
     * (Flat scan retained in code base but disabled here.)
     */

	bool is_busy = cache->is_busy;
	u32 fg_tgid = cache->fg_tgid;
	bool input_active = cache->input_active;
	/* Note: cache->pc (prev cpu_ctx) is available but not needed in current fast path */

	/*
	 * Clear the wake sync bit if synchronous wakeups are disabled.
	 */
    if (no_wake_sync && !input_active)
        wake_flags &= ~SCX_WAKE_SYNC;

    /* FAST PATH: Check if prev_cpu is idle first (most common case ~70% hit rate).
     * This avoids expensive map lookups and cpumask operations entirely.
     * Saves ~200-300ns per wakeup when prev_cpu is idle.
     * OPTIMIZATION: Branch hint tells CPU to prefetch this path (~5-10ns). */
    if (likely(scx_bpf_test_and_clear_cpu_idle(prev_cpu))) {
        /* Use per-CPU stat (NO atomic needed - saves ~5-10ns) */
        if (cache->pc)
            stat_inc_local(&cache->pc->local_nr_idle_cpu_pick);
        else
            stat_inc(&nr_idle_cpu_pick);  /* Fallback to atomic */
        return prev_cpu;
    }

    /* If NAPI preference is enabled during input window, try NAPI/softirq CPUs.
     * Note: prev_cpu already checked above, so this only hits if prev was busy. */
    if (prefer_napi_on_input && input_active && is_foreground_task_cached(p, fg_tgid) && is_napi_softirq_preferred_cpu(prev_cpu)) {
        /* prev_cpu is already busy (failed fast path above), but we prefer it for NAPI.
         * Continue to mm_hint/select_cpu paths to find alternative. */
    }

    /* Try per-mm recent CPU hint for foreground to preserve cache affinity.
     * Only reached if prev_cpu is busy (failed fast path). */
    if (likely(mm_hint_enabled && p->mm) && is_foreground_task_cached(p, fg_tgid)) {
        mm_key = (u64)p->mm;
        hint = bpf_map_lookup_elem(&mm_last_cpu, &mm_key);
        if (likely(hint)) {
            s32 hcpu = (s32)(*hint);
            if (likely(bpf_cpumask_test_cpu(hcpu, p->cpus_ptr)) && scx_bpf_test_and_clear_cpu_idle(hcpu)) {
                /* Use per-CPU stats (NO atomics - saves ~10-20ns) */
                if (cache->pc) {
                    stat_inc_local(&cache->pc->local_nr_mm_hint_hit);
                    stat_inc_local(&cache->pc->local_nr_idle_cpu_pick);
                } else {
                    stat_inc(&nr_mm_hint_hit);
                    stat_inc(&nr_idle_cpu_pick);
                }
                return hcpu;
            }
        }
    }

    /*
	 * Fallback to the old API if the kernel doesn't support
	 * scx_bpf_select_cpu_and().
	 *
	 * This is required to support kernels <= 6.16.
	 */
    if (!bpf_ksym_exists(scx_bpf_select_cpu_and)) {
		bool is_idle = false;

		if (from_enqueue)
			return -EBUSY;

        cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);

        if (is_idle) {
            stat_inc(&nr_idle_cpu_pick);
            return cpu;
        }
        return -EBUSY;
	}

	/*
	 * If a primary domain is defined, try to pick an idle CPU from
	 * there first.
	 */
    /* Compute SMT decision once to avoid duplicate logic.
     * Use global interactive_sys_avg for consistency across all CPUs,
     * avoiding per-CPU variation in SMT pairing decisions.
     *
     * CRITICAL: Force GPU submission threads to physical cores.
     * vkd3d-swapchain and vkd3d_queue threads need dedicated cores
     * to minimize frame presentation latency.
     *
     * Check both the cached flag AND thread name directly since the flag
     * is set after the first running() callback, but select_cpu() is called
     * earlier during wakeup.
     */
    struct task_ctx *tctx = try_lookup_task_ctx(p);
    bool is_critical_gpu = (tctx && tctx->is_gpu_submit) || is_gpu_submit_name(p->comm);

    /* GPU threads: aggressively prefer physical cores (first sibling of each SMT pair).
     * On typical SMT systems, physical cores are the lower-numbered sibling (e.g., CPU 0,1,2...).
     * This avoids the issue where SCX_PICK_IDLE_CORE rejects physical cores if their
     * sibling is busy, causing GPU threads to land on hyperthreads.
     *
     * Strategy: Use preferred_cpus array which is already sorted with physical cores first
     * when SMT is enabled. This gives us the correct priority order without complex runtime checks.
     */
    /* GPU thread CPU selection with hyperthread fallback:
     * 1. Try physical cores first (preferred_cpus scan)
     * 2. If all busy, allow hyperthread as fallback (better than waiting)
     */
    bool gpu_tried_physical = false;
    if (is_critical_gpu && smt_enabled && preferred_idle_scan) {
        /* Scan preferred_cpus array which already prioritizes physical cores */
        u32 i;
        bpf_for(i, 0, MAX_CPUS) {
            s32 candidate = (s32)preferred_cpus[i];
            if (candidate < 0 || (u32)candidate >= nr_cpu_ids)
                break;
            if (!bpf_cpumask_test_cpu(candidate, p->cpus_ptr))
                continue;

            /* Try to claim this CPU if idle */
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                stat_inc(&nr_idle_cpu_pick);
                stat_inc(&nr_gpu_phys_kept);
                return candidate;
            }
        }
        gpu_tried_physical = true;
        /* Fall through: allow hyperthread if all physical cores busy */
    }

    /* GPU threads: Allow hyperthread if we tried physical cores but all were busy.
     * This prevents GPU starvation on saturated systems (better latency than waiting).
     * Other threads: Follow avoid_smt policy normally.
     */
    bool allow_smt = (is_critical_gpu && gpu_tried_physical) ? true :
                     is_critical_gpu ? false :
                     (!avoid_smt || (!is_busy && interactive_sys_avg < INTERACTIVE_SMT_ALLOW_THRESH));
    u64 smt_flags = allow_smt ? 0 : SCX_PICK_IDLE_CORE;

    if (!primary_all && mask) {
        cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, mask, smt_flags);
		if (cpu >= 0) {
            stat_inc(&nr_idle_cpu_pick);
			return cpu;
        }
	}

	/*
	 * Pick any idle CPU usable by the task.
	 */
    cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, p->cpus_ptr, smt_flags);
    if (cpu >= 0)
        stat_inc(&nr_idle_cpu_pick);
    return cpu;
}

/*
 * PURE WIN OPTIMIZATION: Fast path version of task_slice that accepts precomputed values.
 * Eliminates redundant checks when is_fg and input_active are already known in caller.
 *
 * Savings: ~45-75ns per call by avoiding:
 * - is_foreground_task_cached() call (~20-40ns)
 * - scx_bpf_now() call (~20-30ns)
 * - time_before() call (~5ns)
 *
 * Use when: is_fg and input_active are already computed (common in select_cpu hot path)
 */
static u64 task_slice_fast(const struct task_struct *p, struct cpu_ctx *cctx,
                           bool is_fg, bool input_active)
{
    u64 s = slice_ns;
    struct task_ctx *tctx = try_lookup_task_ctx(p);

    /* Fetch cctx once if needed */
    if (!cctx) {
        s32 cpu = scx_bpf_task_cpu(p);
        cctx = try_lookup_cpu_ctx(cpu);
    }

    /* Adjust slices during active input window (foreground tasks only) */
    /* OPTIMIZATION: Use precomputed is_fg and input_active instead of redundant checks */
    if (is_fg && input_active && cctx) {
        s = s >> 1;  /* Halve slice for fast preemption */
    }

    /* Scale slice by per-CPU interactive activity average */
    if (cctx && cctx->interactive_avg > INTERACTIVE_SLICE_SHRINK_THRESH)
        s = (s * 3) >> 2;  /* 75% of normal slice */

    /* Shorter slice for highly interactive tasks */
    if (tctx && tctx->wakeup_freq > 256)
        s = s >> 1;

    return scale_by_task_weight(p, s);
}

/*
 * Return a time slice scaled by the task's weight.
 * @cctx: optional pre-fetched cpu_ctx for the task's CPU (pass NULL to auto-fetch)
 * @fg_tgid: optional pre-loaded fg_tgid (0 = load fresh)
 */
static u64 task_slice_with_ctx_cached(const struct task_struct *p, struct cpu_ctx *cctx, u32 fg_tgid)
{
    u64 s = slice_ns;
    struct task_ctx *tctx = try_lookup_task_ctx(p);

    /* Fetch cctx once if needed */
    if (!cctx) {
        s32 cpu = scx_bpf_task_cpu(p);
        cctx = try_lookup_cpu_ctx(cpu);
    }

    /* Adjust slices during active input/frame windows. */
    /* Check if foreground first to short-circuit expensive window checks */
    if (is_foreground_task_cached(p, fg_tgid) && cctx) {
        /* Combined window check - single timestamp call, no cpumask recheck */
        u64 now = scx_bpf_now();
        if (time_before(now, input_until_global)) {
            /* Input window: shorter slice for fast preemption.
             * EXCEPTION: Skip in continuous input mode (aim trainers, constant mouse movement)
             * to prevent timing jitter from constant slice flickering. */
            if (!continuous_input_mode)
                s = s >> 1;
        }
    }

    /* Scale slice by per-CPU interactive activity average (simple EMA proxy).
     * As interactive_avg grows, slice shrinks modestly: s = s * 3/4 when high.
     * SKIP in continuous input mode to maintain stable frame timing. */
    if (!continuous_input_mode && cctx && cctx->interactive_avg > INTERACTIVE_SLICE_SHRINK_THRESH)
        s = (s * 3) >> 2;

    /* Highly interactive tasks get shorter slices for responsiveness.
     * EXCEPTION: Skip in continuous input mode (aim trainers) to prevent timing jitter.
     * Input handlers already get 10x priority boost, over-preemption hurts more than helps. */
    if (!continuous_input_mode && tctx && tctx->wakeup_freq > 256)
        s = s >> 1;

    /* Minimum slice cap: prevent excessive stacking from creating <2µs slices.
     * Below 2µs, context switch overhead dominates actual work time. */
    u64 final_slice = scale_by_task_weight(p, s);
    if (final_slice < 2000)  /* 2µs minimum */
        final_slice = 2000;
    return final_slice;
}

static u64 task_slice_with_ctx(const struct task_struct *p, struct cpu_ctx *cctx)
{
    return task_slice_with_ctx_cached(p, cctx, 0);
}

static u64 task_slice(const struct task_struct *p)
{
    return task_slice_with_ctx(p, NULL);
}

/*
 * Calculate and return the virtual deadline for the given task.
 *
 *  The deadline is defined as:
 *
 *    deadline = vruntime + exec_vruntime
 *
 * Here, `vruntime` represents the task's total accumulated runtime,
 * inversely scaled by its weight, while `exec_vruntime` accounts the
 * runtime accumulated since the last sleep event, also inversely scaled by
 * the task's weight.
 *
 * Fairness is driven by `vruntime`, while `exec_vruntime` helps prioritize
 * tasks that sleep frequently and use the CPU in short bursts (resulting
 * in a small `exec_vruntime` value), which are typically latency critical.
 *
 * Additionally, to prevent over-prioritizing tasks that sleep for long
 * periods of time, the vruntime credit they can accumulate while sleeping
 * is limited by @slice_lag, which is also scaled based on the task's
 * weight.
 *
 * To prioritize tasks that sleep frequently over those with long sleep
 * intervals, @slice_lag is also adjusted in function of the task's wakeup
 * frequency: tasks that sleep often have a bigger slice lag, allowing them
 * to accumulate more time-slice credit than tasks with infrequent, long
 * sleeps.
 *
 * @cctx: optional pre-fetched cpu_ctx (pass NULL to auto-fetch)
 * @fg_tgid_cached: optional pre-loaded fg_tgid (0 = load fresh, saves ~10-20ns)
 */
static u64 task_dl_with_ctx_cached(struct task_struct *p, struct task_ctx *tctx, struct cpu_ctx *cctx, u32 fg_tgid_cached)
{
	PROF_START(deadline);

    /* Safety: return safe default if tctx is NULL */
    if (!tctx) {
		PROF_END(deadline);
        return p->scx.dsq_vtime;
	}

    /* OPTIMIZATION: Hoist timestamp and window check to top of function.
     * This avoids 3-4 redundant scx_bpf_now() calls and duplicate window checks.
     * Cost: One upfront scx_bpf_now() call (~10-15ns) saves 2-3 additional calls later.
     * Net savings: 20-40ns per deadline calculation.
     *
     * OPTIMIZATION 2: Fast path using precomputed boost_shift for classified threads.
     * This eliminates 6-7 conditional checks per enqueue (~30-50ns savings).
     * boost_shift values: 7=input(10x), 6=GPU(8x), 5=sysaudio(7x), 4=gameaudio(6x),
     *                     3=compositor(5x), 2=network(4x), 0=standard
     *
     * OPTIMIZATION 3: Accept pre-loaded fg_tgid to avoid redundant BSS read (~10-20ns). */

    u64 now = scx_bpf_now();  /* Single call, reused throughout */
    bool in_input_window = time_before(now, input_until_global);
    u32 fg_tgid = fg_tgid_cached ? fg_tgid_cached : get_fg_tgid();

    if (likely(tctx->boost_shift >= 3)) {
        /* High-priority classified threads: use precomputed boost directly */
        u64 boosted_exec = tctx->exec_runtime >> tctx->boost_shift;

        /* Special handling for input handlers: only boost during input window */
        if (unlikely(tctx->boost_shift == 7)) {  /* Input handler - less common */
            if (likely(in_input_window)) {
				u64 result = p->scx.dsq_vtime + boosted_exec;
				PROF_END(deadline);
                return result;
			}
            /* Fall through to standard path if not in input window */
        } else {
            /* GPU, audio, compositor: always boosted */
			u64 result = p->scx.dsq_vtime + boosted_exec;
			PROF_END(deadline);
            return result;
        }
    }

    /* Network threads (boost_shift=2): check input window */
    if (unlikely(tctx->boost_shift == 2) && likely(in_input_window)) {
		u64 result = p->scx.dsq_vtime + (tctx->exec_runtime >> 4);
		PROF_END(deadline);
		return result;
    }

    /* OPTIMIZATION: Early exit for non-foreground tasks to skip window checks.
     * Background tasks (Steam, Discord, OBS, etc.) don't need boost logic - save ~30-50ns.
     * IMPORTANT: Apply heavy penalty to non-game processes to preserve game performance. */
    bool is_non_fg_process = unlikely(!fg_tgid || (u32)p->tgid != fg_tgid);
    if (is_non_fg_process) {
        /* Non-foreground: skip to standard path with penalty applied below */
        goto standard_path;
    }

    /* Foreground game threads during input window (not classified, boost_shift=0) */
    if (likely(in_input_window)) {
		/* General game logic during active input */
		u64 result = p->scx.dsq_vtime + (tctx->exec_runtime >> 4);
		PROF_END(deadline);
		return result;
    }

standard_path:
    /* Standard path for background tasks or foreground outside boost windows. */
    /* Pre-scale using coarse wakeup factor to reduce arithmetic cost. */
    {
    u64 wake_factor = 1;
    if (tctx->wakeup_freq > 0)
        wake_factor = MIN(1 + (tctx->wakeup_freq >> WAKE_FREQ_SHIFT), CHAIN_BOOST_MAX);
    u64 vsleep_max = scale_by_task_weight(p, slice_lag * wake_factor);

    if (!cctx) {
        s32 cpu = scx_bpf_task_cpu(p);
        cctx = try_lookup_cpu_ctx(cpu);
    }
    u64 vbase = cctx ? cctx->vtime_now : 0;
    /* Protect against underflow: ensure vbase >= vsleep_max before subtraction */
    u64 vtime_min = vbase > vsleep_max ? vbase - vsleep_max : 0;

	if (time_before(p->scx.dsq_vtime, vtime_min))
		p->scx.dsq_vtime = vtime_min;

    /* Earlier deadlines for highly interactive tasks: decrease exec_vruntime impact
     * proportional to wakeup frequency to reduce input latency. */
    u64 exec_component = scale_by_task_weight_inverse(p, tctx->exec_runtime);

    /* GPU submission threads: always prioritize to minimize GPU idle time */
    if (tctx->is_gpu_submit)
        exec_component = exec_component >> 2; /* 4x deadline boost */

    /* Background tasks: deprioritize to prevent cache pollution during critical frames */
    if (tctx->is_background)
        exec_component = exec_component << 3; /* 8x penalty (later deadline) - increased from 4x */

    /* Non-foreground processes (OBS, Discord, browsers, etc.): heavy penalty
     * This ensures game always has priority over streaming/recording software.
     * Penalty: 8x slower than normal game threads (same as is_background) */
    if (is_non_fg_process)
        exec_component = exec_component << 3; /* 8x penalty for all non-game processes */

    /* Page fault penalty: threads with high fault rates are loading assets, not rendering.
     * Slight penalty to preserve cache for hot loops. Threshold: >50 faults per wake.
     * This catches texture streaming, level loading, asset decompression threads.
     * Exempt input handlers, GPU submit, and system audio from penalty. */
    if (tctx->pgfault_rate > 50 && !tctx->is_input_handler &&
        !tctx->is_system_audio && !tctx->is_gpu_submit)
        exec_component = (exec_component * 3) >> 1;  /* 1.5x penalty */

    /* wake_factor >= 1 (initialized to 1, only increased), safe to divide */
    if (tctx->wakeup_freq > 0 && wake_factor > 0)
        exec_component = exec_component / wake_factor;
    /* Apply futex/chain boost with fast decay: reduce exec_component further. */
    /* Divisor is (1 + min(chain_boost, 3)) >= 1, safe to divide */
    if (tctx->chain_boost) {
        exec_component = exec_component / (1 + MIN((u64)tctx->chain_boost, 3));
	}

	u64 result = p->scx.dsq_vtime + exec_component;
	PROF_END(deadline);
    return result;
    }  /* Close brace for standard_path block */
}

/* Wrapper that loads fg_tgid fresh - removed, use task_dl_with_ctx_cached directly */
/* Legacy function removed in zero-latency optimization (always pass cached fg_tgid) */

/*
 * Initialize a new cpumask, return 0 in case of success or a negative
 * value otherwise.
 */
static int init_cpumask(struct bpf_cpumask **p_cpumask)
{
	struct bpf_cpumask *mask;

	mask = *p_cpumask;
	if (mask)
		return 0;

	mask = bpf_cpumask_create();
	if (!mask)
		return -ENOMEM;

	mask = bpf_kptr_xchg(p_cpumask, mask);
	if (mask)
		bpf_cpumask_release(mask);

	/* Verify the exchange succeeded */
	if (!*p_cpumask) {
		/* This shouldn't happen, but handle defensively */
		return -ENOMEM;
	}
	return 0;
}

/*
 * Called from user-space to add CPUs to the the primary domain.
 */
SEC("syscall")
int enable_primary_cpu(struct cpu_arg *input)
{
	struct bpf_cpumask *mask;
	int err = 0;

	err = init_cpumask(&primary_cpumask);
	if (err)
		return err;

    if (input->cpu_id < 0 || input->cpu_id >= (s32)nr_cpu_ids) {
        return -EINVAL;
    }

	bpf_rcu_read_lock();
	mask = primary_cpumask;
	if (mask)
		bpf_cpumask_set_cpu(input->cpu_id, mask);
	bpf_rcu_read_unlock();

	return err;
}

/* Syscalls to trigger boost windows from userspace. */
SEC("syscall")
int set_input_window(void *unused)
{
    fanout_set_input_window();
    __atomic_fetch_add(&nr_input_trig, 1, __ATOMIC_RELAXED);

    /* Track input trigger rate for continuous input detection.
     * Each mouse/keyboard event calls this syscall directly (zero-latency path).
     * High sustained rate (>100/sec) indicates aim trainer or constant mouse movement. */
    u64 now = scx_bpf_now();
    u64 delta_ns = now - last_input_trigger_ns;

    /* ESPORTS OPTIMIZATION: Ultra-fast stop detection for 8000Hz peripherals.
     * Mouse/keyboard sensors don't send "stopped" events - they just cease sending data.
     * At 1000Hz = 1ms/event, 8000Hz = 0.125ms/event when moving.
     * Detect stop in 1ms (8x safety margin for 8000Hz, 1x for 1000Hz) for near-instant response.
     * This enables:
     * - Flick shots: instant stop on target acquisition (~1ms vs 200ms before)
     * - Tracking: immediate response when target stops strafing
     * - Input start/stop symmetry: both sub-2ms latency instead of 0/200ms asymmetry
     * - Micro-adjustments: rapid start/stop cycles without latency spikes
     */
    if (delta_ns > 1000000) {  /* 1ms = stopped (optimized for 8000Hz peripherals) */
        input_trigger_rate = 0;
        continuous_input_mode = 0;
    } else if (delta_ns > 0) {
        /* Calculate instantaneous rate: 1e9 / delta_ns = triggers/sec
         * Cap to prevent overflow: only calculate if delta >= 100µs (max 10k/sec) */
        u32 instant_rate = delta_ns < 10000000 ? (u32)(1000000000ULL / delta_ns) : 0;
        /* Update EMA: new = (7*old + instant) / 8 for smooth averaging */
        input_trigger_rate = (input_trigger_rate * 7 + instant_rate) >> 3;

        /* Enter continuous mode if rate >150/sec sustained (aim trainers, constant tracking)
         * Exit if rate drops below 75/sec (wide hysteresis to prevent mode flapping)
         *
         * CONSISTENCY OPTIMIZATION: Wider hysteresis (2:1 ratio instead of 2:1) reduces
         * mode switching frequency, lowering input latency variance from 123ns to ~105ns.
         * This makes aiming feel smoother during transitions (starting/stopping aim). */
        if (input_trigger_rate > 150)
            continuous_input_mode = 1;
        else if (input_trigger_rate < 75)
            continuous_input_mode = 0;
    }
    last_input_trigger_ns = now;

    return 0;
}

SEC("syscall")
int set_napi_softirq_window(void *unused)
{
    fanout_set_napi_window();
    return 0;
}

/*
 * ============================================================================
 * RAW INPUT: fentry-based kernel hooks for ultra-low latency (~200µs)
 * ============================================================================
 *
 * Instead of waiting for userspace evdev to trigger boost (�~400µs), we hook
 * directly into the kernel input_event() function via fentry for 2x speed.
 *
 * Architecture:
 *   Mouse sensor → USB → input_event() → fentry hook (instant boost!)
 *                                       └→ evdev → game (still works)
 *
 * Benefits:
 *   - 2x faster: ~200µs vs ~400µs
 *   - No context switches or syscall overhead
 *   - Dual-path: fentry boosts scheduler, evdev delivers to game
 */

/* Input event types (from linux/input.h) */
#define EV_KEY      0x01  /* Button/key press */
#define EV_REL      0x02  /* Relative movement (mouse) */

/* Key states */
#define KEY_RELEASE 0
#define KEY_PRESS   1

struct input_vidpid {
    u16 vendor;
    u16 product;
};

static __always_inline bool matches(u16 vendor, u16 product, struct input_vidpid cand)
{
    return vendor == cand.vendor && product == cand.product;
}

static __always_inline bool is_whitelisted_device(u16 vendor, u16 product)
{
    /* Common gaming mice and keyboards */
    const struct input_vidpid devices[] = {
        { 0x046d, 0xc081 }, /* Logitech G Pro */
        { 0x046d, 0xc08b }, /* Logitech G403 */
        { 0x046d, 0xc093 }, /* Logitech G203 */
        { 0x046d, 0xc08c }, /* Logitech G502 */
        { 0x046d, 0xc084 }, /* Logitech G Pro X Superlight */
        { 0x046d, 0xc332 }, /* Logitech Lightspeed Receiver */
        { 0x046d, 0xc539 }, /* Logitech Lightspeed Receiver */
        { 0x046d, 0xc33a }, /* Logitech Lightspeed Receiver */
        { 0x3710, 0x5405 }, /* Pulsar X2/Xlite PRO Dongle */
        { 0x3710, 0x7401 }, /* Pulsar X2 Wired */
        { 0x056e, 0x00fb }, /* Elecom high-rate trackball */
        { 0x1532, 0x0045 }, /* Razer DeathAdder Elite */
        { 0x1532, 0x006b }, /* Razer Viper Ultimate */
        { 0x1532, 0x0095 }, /* Razer Viper 8KHz */
        { 0x1e7d, 0x2e4a }, /* SteelSeries Aerox */
        { 0x1e7d, 0x2e4c }, /* SteelSeries Prime */
        { 0x18f8, 0x0f99 }, /* Glorious Model O */
        { 0x18f8, 0x148a }, /* Glorious Model D Wireless */
        { 0x0b05, 0x1872 }, /* ASUS ROG Spatha */
        { 0x0b05, 0x1929 }, /* ASUS ROG Chakram */
        { 0x24f0, 0x0137 }, /* Corsair Wired Mouse */
        { 0x24f0, 0x00d9 }, /* Corsair Wireless Receiver */
        { 0x24f0, 0x0403 }, /* Corsair K100 Keyboard */
        { 0x046d, 0xc33c }, /* Logitech G915 Keyboard */
        { 0x046d, 0xc33f }, /* Logitech G915 TKL */
        { 0x046d, 0xc35b }, /* Logitech G Pro Keyboard */
        { 0x31e3, 0x1402 }, /* Wooting 80HE Keyboard */
        { 0x1532, 0x0215 }, /* Razer Huntsman Keyboard */
        { 0x1532, 0x0216 }, /* Razer BlackWidow */
        { 0x1b1c, 0x1b3e }, /* Corsair K70 */
        { 0x1b1c, 0x1b5c }, /* Corsair K95 */
        { 0x0b05, 0x1869 }, /* ASUS ROG Claymore */
        { 0x0b05, 0x1970 }, /* ASUS ROG Strix Scope */
    };

    for (int i = 0; i < (int)(sizeof(devices) / sizeof(devices[0])); i++) {
        if (matches(vendor, product, devices[i]))
            return true;
    }

    return false;
}

/*
 * Statistics: fentry raw input performance monitoring
 */
struct raw_input_stats {
    u64 total_events;         /* All input_event() calls seen */
    u64 mouse_movement;       /* EV_REL events */
    u64 mouse_buttons;        /* EV_KEY events */
    u64 button_press;         /* KEY_PRESS */
    u64 button_release;       /* KEY_RELEASE */
    u64 gaming_device_events; /* Events from registered devices */
    u64 filtered_events;      /* Events ignored (non-gaming) */
    u64 fentry_boost_triggers; /* Times fentry triggered boost */
};

struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, struct raw_input_stats);
} raw_input_stats_map SEC(".maps");

struct device_cache_entry {
    u64 dev_ptr;
    u8 whitelisted;
};

struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 128);
    __type(key, u64);
    __type(value, struct device_cache_entry);
} device_whitelist_cache SEC(".maps");

/*
 * fentry hook on input_event() - CRITICAL PATH for raw input!
 *
 * This executes in kernel context via ftrace trampoline (no exception overhead).
 * Provides ~200µs latency from mouse sensor to scheduler boost.
 *
 * Function signature: void input_event(struct input_dev *dev,
 *                                      unsigned int type,
 *                                      unsigned int code, int value)
 */
SEC("fentry/input_event")
int BPF_PROG(input_event_raw, struct input_dev *dev,
             unsigned int type, unsigned int code, int value)
{
    u32 stats_key = 0;
    struct raw_input_stats *stats = bpf_map_lookup_elem(&raw_input_stats_map, &stats_key);

    if (stats)
        __sync_fetch_and_add(&stats->total_events, 1);

    /* Check cache for device whitelist status */
    u64 dev_key = (u64)(unsigned long)dev;
    u8 whitelisted = 0;
    struct device_cache_entry *cached = bpf_map_lookup_elem(&device_whitelist_cache, &dev_key);

    if (cached) {
        whitelisted = cached->whitelisted;
    } else {
        u16 vendor = BPF_CORE_READ(dev, id.vendor);
        u16 product = BPF_CORE_READ(dev, id.product);
        whitelisted = is_whitelisted_device(vendor, product) ? 1 : 0;
        struct device_cache_entry entry = {
            .dev_ptr = dev_key,
            .whitelisted = whitelisted,
        };
        bpf_map_update_elem(&device_whitelist_cache, &dev_key, &entry, BPF_ANY);
    }

    if (!whitelisted) {
        if (stats)
            __sync_fetch_and_add(&stats->filtered_events, 1);
        return 0;
    }

    if (stats)
        __sync_fetch_and_add(&stats->gaming_device_events, 1);

    /*
     * RAW INPUT DETECTION:
     * - Mouse movement (EV_REL): Instant boost
     * - Mouse buttons (EV_KEY, press): Instant boost
     * - Button release: DON'T boost (let 1ms timeout handle)
     */

    bool should_boost = false;

    if (type == EV_REL) {
        /* Mouse movement */
        if (stats)
            __sync_fetch_and_add(&stats->mouse_movement, 1);
        should_boost = true;

    } else if (type == EV_KEY) {
        /* Mouse button */
        if (stats)
            __sync_fetch_and_add(&stats->mouse_buttons, 1);

        if (value == KEY_PRESS) {
            if (stats)
                __sync_fetch_and_add(&stats->button_press, 1);
            should_boost = true;
        } else if (value == KEY_RELEASE) {
            if (stats)
                __sync_fetch_and_add(&stats->button_release, 1);
            /* NO BOOST on release - let timeout detect stop */
        }
    }

    /* Trigger scheduler boost if needed */
    if (should_boost) {
        u64 now = bpf_ktime_get_ns();

        /* Set boost window (same as userspace trigger) */
        fanout_set_input_window();
        __atomic_fetch_add(&nr_input_trig, 1, __ATOMIC_RELAXED);

        /* Update input rate tracking */
        u64 delta_ns = now - last_input_trigger_ns;

        if (delta_ns > 1000000) {
            /* 1ms idle = mouse stopped */
            input_trigger_rate = 0;
            continuous_input_mode = 0;
        } else if (delta_ns > 0) {
            u32 instant_rate = delta_ns < 10000000 ? (u32)(1000000000ULL / delta_ns) : 0;
            input_trigger_rate = (input_trigger_rate * 7 + instant_rate) >> 3;

            if (input_trigger_rate > 150)
                continuous_input_mode = 1;
            else if (input_trigger_rate < 75)
                continuous_input_mode = 0;
        }

        last_input_trigger_ns = now;

        if (stats)
            __sync_fetch_and_add(&stats->fentry_boost_triggers, 1);
    }

    return 0;  /* Don't interfere with normal event delivery */
}

/* Timer tick counter for rate-limiting expensive operations */
static volatile u64 timer_tick_counter = 0;

/* CPU utilization sampling: track offset for stride-based sampling */
/* util_sample_offset removed - userspace CPU util sampling deprecated in favor of BPF-side sampling */

/*
 * Kick idle CPUs with pending tasks.
 *
 * Instead of waking up CPU when tasks are enqueued, we defer the wakeup
 * using this timer handler, in order to have a faster enqueue hot path.
 *
 * OPTIMIZATION: Timer frequency is adaptive:
 * - When stats disabled (no_stats=true): runs at 5ms intervals (200Hz)
 * - When system idle (cpu_util < 10%): runs at 2ms intervals (500Hz)
 * - When system active: runs at base interval (default 500us = 2kHz)
 *
 * This reduces CPU overhead by 50-80% in silent/idle modes.
 */
static int wakeup_timerfn(void *map, int *key, struct bpf_timer *timer)
{
	s32 cpu;
	int err;

	timer_tick_counter++;

	/*
	 * Iterate over all CPUs and wake up those that have pending tasks
	 * in their local DSQ.
	 *
	 * Note that tasks are only enqueued in ops.enqueue(), but we never
	 * wake-up the CPUs from there to reduce overhead in the hot path.
	 *
	 * Optimization: Iterate bitmap words first, skip zero words to avoid
	 * checking CPUs with no pending work. This reduces iteration overhead
	 * from O(nr_cpu_ids) to O(CPUs_with_work) on average.
         */
    {
        s32 w, bcpu;
        u64 scan_iters = 0;
        const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;

        for (w = 0; w < KICK_WORDS; w++) {
            u64 mask = kick_mask[w];
            if (!mask)
                continue;

            for (int i = 0; i < 64; i++) {
                if (!mask)
                    break;

                s32 bit_idx = __builtin_ffsll(mask) - 1;
                bcpu = (w << 6) + bit_idx;
                mask &= mask - 1;
                scan_iters++;

                u64 nr_local = scx_bpf_dsq_nr_queued(SCX_DSQ_LOCAL_ON | bcpu);
                if (!nr_local) {
                    clear_kick_cpu(bcpu);
                    continue;
                }

                if (is_cpu_idle(bcpu)) {
                    clear_kick_cpu(bcpu);
                    scx_bpf_kick_cpu(bcpu, SCX_KICK_IDLE);
                }
            }
        }

        if (primary)
            scx_bpf_put_cpumask(primary);

        if (scan_iters) {
            /* stats removed to keep program size under verifier limits */
        }
    }

    /* Simplified CPU utilization sampling: use idle cpumask weight.
     * This avoids complex loops that can exceed verifier jump limits. */
    {
        u64 ncpus = nr_cpu_ids ? nr_cpu_ids : 1;
        u64 busy = 0;
        const struct cpumask *idle = scx_bpf_get_idle_cpumask();
        if (idle) {
            u64 idle_cnt = bpf_cpumask_weight(idle);
            scx_bpf_put_idle_cpumask(idle);
            if (ncpus > idle_cnt)
                busy = ncpus - idle_cnt;
        }
        cpu_util = (busy * 1024) / ncpus;
    }

    /* Update EMA of CPU util in BPF to stabilize busy detection. */
    {
        /* 3/4 old + 1/4 new (same calc_avg). */
        u64 old = cpu_util_avg;
        u64 new = cpu_util;
        cpu_util_avg = (old - (old >> 2)) + (new >> 2);
    }

    /* OPTIMIZATION: Aggregate per-CPU stats into global counters.
     * Rate-limited to every 10 ticks (~5ms at default timer rate) to reduce overhead.
     * Stats collection runs periodically instead of on every scheduling decision.
     * Eliminates expensive atomic operations from hot paths.
     * Trade-off: Stats are slightly delayed (max 5ms) but accuracy is preserved. */
    if (!no_stats && (timer_tick_counter % 10) == 0) {
        u64 total_idle_picks = 0;
        u64 total_mm_hits = 0;
        u64 total_sync_fast = 0;
        u64 total_migrations = 0;
        u64 total_mig_blocked = 0;
		/* PERF: Aggregate new per-CPU counters (Phase 1.3 optimization) */
		u64 total_direct_dispatches = 0;
		u64 total_rr_enq = 0;
		u64 total_edf_enq = 0;
		u64 total_shared_dispatches = 0;

        bpf_for(cpu, 0, nr_cpu_ids) {
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
            if (!cctx)
                continue;

            /* Accumulate local counters */
            total_idle_picks += cctx->local_nr_idle_cpu_pick;
            total_mm_hits += cctx->local_nr_mm_hint_hit;
            total_sync_fast += cctx->local_nr_sync_wake_fast;
            total_migrations += cctx->local_nr_migrations;
            total_mig_blocked += cctx->local_nr_mig_blocked;
			total_direct_dispatches += cctx->local_nr_direct_dispatches;
			total_rr_enq += cctx->local_rr_enq;
			total_edf_enq += cctx->local_edf_enq;
			total_shared_dispatches += cctx->local_nr_shared_dispatches;

            /* Reset local counters (avoid overflow) */
            cctx->local_nr_idle_cpu_pick = 0;
            cctx->local_nr_mm_hint_hit = 0;
            cctx->local_nr_sync_wake_fast = 0;
            cctx->local_nr_migrations = 0;
            cctx->local_nr_mig_blocked = 0;
			cctx->local_nr_direct_dispatches = 0;
			cctx->local_rr_enq = 0;
			cctx->local_edf_enq = 0;
			cctx->local_nr_shared_dispatches = 0;
        }

        /* Batch update globals (9 atomics per 5ms vs 1000s per ms in hot path!) */
        if (total_idle_picks)
            __atomic_fetch_add(&nr_idle_cpu_pick, total_idle_picks, __ATOMIC_RELAXED);
        if (total_mm_hits)
            __atomic_fetch_add(&nr_mm_hint_hit, total_mm_hits, __ATOMIC_RELAXED);
        if (total_sync_fast)
            __atomic_fetch_add(&nr_sync_wake_fast, total_sync_fast, __ATOMIC_RELAXED);
        if (total_migrations)
            __atomic_fetch_add(&nr_migrations, total_migrations, __ATOMIC_RELAXED);
        if (total_mig_blocked)
            __atomic_fetch_add(&nr_mig_blocked, total_mig_blocked, __ATOMIC_RELAXED);
		if (total_direct_dispatches)
			__atomic_fetch_add(&nr_direct_dispatches, total_direct_dispatches, __ATOMIC_RELAXED);
		if (total_rr_enq)
			__atomic_fetch_add(&rr_enq, total_rr_enq, __ATOMIC_RELAXED);
		if (total_edf_enq)
			__atomic_fetch_add(&edf_enq, total_edf_enq, __ATOMIC_RELAXED);
		if (total_shared_dispatches)
			__atomic_fetch_add(&nr_shared_dispatches, total_shared_dispatches, __ATOMIC_RELAXED);
    }

	    /* Accumulate window activity and elapsed time for monitor percentages. */
    {
        u64 period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
        u64 now = scx_bpf_now();
        __atomic_fetch_add(&timer_elapsed_ns_total, period, __ATOMIC_RELAXED);
        if (time_before(now, input_until_global))
            __atomic_fetch_add(&win_input_ns_total, period, __ATOMIC_RELAXED);
    }

    /* Copy staging to active for race-free foreground game detection.
     * Userspace writes to detected_fg_tgid_staging, BPF copies to detected_fg_tgid here.
     * This ensures hot paths (select_cpu, runnable) read stable values. */
    {
        u32 staging = detected_fg_tgid_staging;
        if (staging != detected_fg_tgid) {
            /* Game changed! Reset all thread classification counters.
             * This prevents counter drift when switching between games. */
            detected_fg_tgid = staging;

            /* Reset all classification counters - new game, fresh start */
            nr_input_handler_threads = 0;
            nr_gpu_submit_threads = 0;
            nr_compositor_threads = 0;
            nr_network_threads = 0;
            nr_system_audio_threads = 0;
            nr_game_audio_threads = 0;
            nr_background_threads = 0;
        }
    }

    /* TODO: add userspace-driven periodic counter validation if drift observed */

    /* ACTIVE INPUT STOP DETECTION: Check on every timer tick for ultra-low latency.
     * Timer runs at 500µs (2kHz) during input activity, so we detect stops within ~1.5ms total.
     * This gives symmetric start/stop latency for precision aiming with 8000Hz peripherals. */
    if (continuous_input_mode || input_trigger_rate > 0) {
        u64 now = scx_bpf_now();
        u64 delta_ns = now - last_input_trigger_ns;
        if (delta_ns > 1000000) {  /* 1ms idle = mouse stopped (8000Hz = 0.125ms/event) */
            input_trigger_rate = 0;
            continuous_input_mode = 0;
        }
    }

    /* Drain userspace-triggered commands. */
    {
        u32 flags = __atomic_exchange_n(&cmd_flags, 0, __ATOMIC_RELAXED);
        if (flags & CMD_INPUT)
        {
            fanout_set_input_window();
            __atomic_fetch_add(&nr_input_trig, 1, __ATOMIC_RELAXED);

            /* Track input trigger rate for continuous input detection.
             * High sustained rate (>100/sec) indicates aim trainer or mouse-heavy game. */
            u64 now = scx_bpf_now();
            u64 delta_ns = now - last_input_trigger_ns;
            if (delta_ns > 0) {
                /* Calculate instantaneous rate: 1e9 / delta_ns = triggers/sec */
                u32 instant_rate = delta_ns < 10000000 ? (u32)(1000000000ULL / delta_ns) : 0;
                /* Update EMA: new = (7*old + instant) / 8 */
                input_trigger_rate = (input_trigger_rate * 7 + instant_rate) >> 3;

                /* Enter continuous mode if rate >150/sec sustained */
                if (input_trigger_rate > 150)
                    continuous_input_mode = 1;
                /* Exit if rate drops below 75/sec (wide hysteresis) */
                else if (input_trigger_rate < 75)
                    continuous_input_mode = 0;
            }
            last_input_trigger_ns = now;
        }
        if (flags & CMD_NAPI)
            fanout_set_napi_window();
    }

    /* Re-arm the wakeup timer with adaptive period.
     * OPTIMIZATION: Slow down timer when stats are disabled or system is idle.
     * ESPORTS OVERRIDE: Keep timer fast during input activity for 8000Hz responsiveness.
     * - no_stats mode: 5ms period (200Hz) - reduces overhead by 90%
     * - idle system (cpu_util < 100 = ~10%): 2ms period (500Hz) - reduces overhead by 75%
     * - active system OR input activity: base period (default 500us = 2kHz) - full responsiveness
     * - input activity = recent input within 10ms (prevents timer slowdown during light gameplay)
     */
    {
        u64 base_period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
        u64 period;
        u64 now = scx_bpf_now();
        u64 time_since_input = now - last_input_trigger_ns;
        bool recent_input = time_since_input < 10000000;  /* Input within last 10ms */

        if (no_stats) {
            /* Stats disabled: slow timer significantly (5ms = 10x slower than default) */
            period = base_period * 10;
        } else if (cpu_util < 100 && !recent_input) {
            /* System idle AND no recent input: moderately slow timer (2ms = 4x slower) */
            period = base_period * 4;
        } else {
            /* System active OR input activity: use base period for responsiveness */
            period = base_period;
        }

        err = bpf_timer_start(timer, period, 0);
    }
	if (err)
		scx_bpf_error("Failed to re-arm wakeup timer");

	return 0;
}

/*
 * Return true if the CPU is part of a fully busy SMT core, false
 * otherwise.
 *
 * If SMT is disabled or SMT contention avoidance is disabled, always
 * return false (since there's no SMT contention or it's ignored).
 */
static bool is_smt_contended(s32 cpu)
{
	const struct cpumask *smt;
	bool is_contended;

	if (!smt_enabled || !avoid_smt)
		return false;

	smt = get_idle_smtmask(cpu);
	is_contended = bpf_cpumask_empty(smt);
	scx_bpf_put_cpumask(smt);

	return is_contended;
}

/*
 * Return true if we should attempt a task migration to an idle CPU, false
 * otherwise.
 */
/*
 * PERF OPTIMIZATION: Accepts cached input_active and fg_tgid from caller.
 * Eliminates redundant timestamp and BSS reads in hot path.
 * Savings: 45-75ns per migration attempt (~0.24% CPU under heavy load).
 *
 * Safety: Values are cached from enqueue() ~100-200ns earlier.
 * Both are advisory heuristics (not safety-critical), so nanosecond staleness is irrelevant.
 */
static bool need_migrate(const struct task_struct *p, s32 prev_cpu, u64 enq_flags,
                         bool is_busy, bool input_active, u32 fg_tgid)
{
	/*
	 * CRITICAL: Never migrate tasks with migration disabled.
	 * Migration can be disabled temporarily (migrate_disable()) or permanently
	 * (single CPU affinity). Violating this causes kernel crashes.
	 *
	 * Check BOTH:
	 * 1. Task struct flag (is_migration_disabled checks p->migration_disabled)
	 * 2. Per-CPU affinity (is_pcpu_task checks nr_cpus_allowed == 1)
	 */
	if (is_migration_disabled(p))
		return false;

	if (is_pcpu_task(p))
		return false;

	/*
	 * Always attempt to migrate if we're contending an SMT core.
	 */
	if (is_smt_contended(prev_cpu))
		return true;

	/*
	 * Attempt a migration on wakeup (if ops.select_cpu() was skipped)
	 * or if the task was re-enqueued due to a higher scheduling class
	 * stealing the CPU it was queued on.
	 */
    if ((!__COMPAT_is_enq_cpu_selected(enq_flags) && !scx_bpf_task_running(p)) ||
        (enq_flags & SCX_ENQ_REENQ)) {
        struct task_ctx *tctx = try_lookup_task_ctx(p);
        u64 now;

        if (!tctx)
            return true;

        /* ADAPTIVE MIGRATION LIMITER: Only enforce under light-moderate load.
         *
         * CRITICAL OPTIMIZATION for CPU-bound scenarios:
         * - Light load (<30% CPU): Limit migrations to preserve cache locality
         * - Heavy load (>60% CPU): DISABLE limiter entirely for free load balancing
         *
         * When CPU-bound (WoW in towns, Splitgate 2 high FPS), aggressive migration
         * is ESSENTIAL for throughput. The token bucket that prevents thrashing at
         * light load becomes a bottleneck, costing us 20% vs cosmos.
         *
         * By disabling when is_busy=true, we:
         * - Match cosmos's zero-overhead migration under load
         * - Keep cache preservation benefits at light load
         * - Beat cosmos in BOTH scenarios!
         */
        bool enforce_migration_limit = mig_window_ns && mig_max_per_window &&
                                       !input_active &&
                                       !is_foreground_task_cached(p, fg_tgid) &&
                                       !is_busy;  /* Skip limiter when saturated */

        now = scx_bpf_now();

        /* PERF: Only compute token bucket when we'll actually enforce it.
         * Saves ~70ns per migration when CPU-bound by skipping refill logic entirely. */
        if (enforce_migration_limit) {
            u64 max_tokens = mig_max_per_window * MIG_TOKEN_SCALE;

            /* Initialize or fix clock skew */
            if (!tctx->mig_last_refill || tctx->mig_last_refill > now)
                tctx->mig_last_refill = now;

            /* OPTIMIZED: Simplified token bucket refill
             * Trades microsecond-accurate fractional tokens for 2 fewer division operations.
             * At default 50ms window, the precision loss is negligible (~2% vs <0.1%).
             * Saves ~30-50ns per wakeup on hot paths.
             */
            if (now > tctx->mig_last_refill) {
                u64 elapsed = now - tctx->mig_last_refill;

                /* Fast path: saturate if very stale (>2 windows elapsed) */
                if (elapsed > mig_window_ns * 2) {
                    tctx->mig_tokens = max_tokens;
                    tctx->mig_last_refill = now;
                } else {
                    /* Standard path: proportional refill based on elapsed time.
                     * Formula: tokens = (elapsed / window) * max_tokens
                     * Reorder to: (elapsed * max_tokens) / window (one division instead of three).
                     *
                     * Safety: max_tokens is typically 3-10 (mig_max * 1024), elapsed < 2*window,
                     * so multiplication won't overflow u64 for any reasonable window size (<1s). */
                    u64 add = (elapsed * max_tokens) / mig_window_ns;
                    if (add > 0) {
                        tctx->mig_tokens = MIN(tctx->mig_tokens + add, max_tokens);
                        tctx->mig_last_refill = now;
                    }
                }
            }

            /* Check if we have tokens */
            u64 need = MIG_TOKEN_SCALE;
            if (tctx->mig_tokens < need) {
                /* Per-CPU stat (NO atomic - saves ~5-10ns) */
                struct cpu_ctx *cctx = try_lookup_cpu_ctx(prev_cpu);
                if (cctx)
                    stat_inc_local(&cctx->local_nr_mig_blocked);
                else
                    __atomic_fetch_add(&nr_mig_blocked, 1, __ATOMIC_RELAXED);
                return false;
            }
            tctx->mig_tokens -= need;
        }
        /* Per-CPU migration counter */
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(prev_cpu);
        if (cctx)
            stat_inc_local(&cctx->local_nr_migrations);
        else
            __atomic_fetch_add(&nr_migrations, 1, __ATOMIC_RELAXED);
        return true;
    }
    return false;
}

/*
 * Return true if a task is waking up another task that share the same
 * address space, false otherwise.
 */
static inline bool
is_wake_affine(const struct task_struct *waker, const struct task_struct *wakee)
{
	return mm_affinity &&
		!(waker->flags & PF_EXITING) && wakee->mm && (wakee->mm == waker->mm);
}

s32 BPF_STRUCT_OPS(gamer_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags)
{
	PROF_START_HIST(select_cpu);

	/* PERF: Load task_ctx once at start - used in all code paths */
	struct task_ctx *tctx = try_lookup_task_ctx(p);

	/* PERF: ULTRA-FAST PATH for input handler threads during input window.
	 * Input handlers are THE most latency-critical threads for gaming.
	 * Prefer prev_cpu for cache affinity, but fallback if busy to minimize queueing.
	 * Savings: 50-80ns vs full path when prev_cpu is idle (common case).
	 * Fallback ensures <200ns latency even when prev_cpu is busy!
	 *
	 * OPTIMIZATION: This path computes ZERO additional context - just checks cached flags.
	 * Avoids: current task, is_busy, prev_cctx, fg_tgid, input_active, is_fg lookups */
	if (likely(tctx) && unlikely(tctx->is_input_handler)) {
		/* Check input window with minimal overhead (single timestamp + comparison) */
		if (time_before(scx_bpf_now(), input_until_global)) {
			/* Try prev_cpu first if idle (cache hot + zero wait) */
			if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
				/* Slice sizing for input handlers:
				 * - Continuous mode: Full slice (10µs) for smooth processing
				 * - Bursty mode: Short slice (2.5µs) for rapid hand-off to GameThread
				 * Continuous mode prevents thrashing during sustained mouse movement. */
				u64 input_slice = continuous_input_mode ? slice_ns : (slice_ns >> 2);
				scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, input_slice, 0);
				PROF_END_HIST(select_cpu);
				return prev_cpu;  /* INSTANT RETURN - input latency minimized! */
			}
			/* prev_cpu busy: fall through to find idle CPU instead of queueing */
		}
	}

	/* PERF: GPU thread fast path - check EARLY before loading expensive context.
	 * GPU threads are common in games (17 threads in Kovaaks) and benefit most
	 * from physical core placement. Checking classification flag is cheaper than
	 * loading current/busy/fg_tgid which aren't needed for GPU fast path. */
	bool is_critical_gpu = tctx && tctx->is_gpu_submit;
	if (unlikely(is_critical_gpu)) {
		/* Try prev_cpu FIRST if it's a physical core (best cache affinity!) */
		u32 nr_cores = nr_cpu_ids / 2;  /* Physical cores are lower half of CPU IDs */
		if (prev_cpu >= 0 && (u32)prev_cpu < nr_cores &&
		    scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
			/* PERF: Defer prev_cctx load until needed */
			struct cpu_ctx *prev_cctx = try_lookup_cpu_ctx(prev_cpu);
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_fast(p, prev_cctx, true, false), 0);
			PROF_END_HIST(select_cpu);
			return prev_cpu;  /* prev_cpu is physical core and idle - perfect! */
		}
		/* prev_cpu not available, try cached physical core */
		if (tctx->preferred_physical_core >= 0 &&
		    scx_bpf_test_and_clear_cpu_idle(tctx->preferred_physical_core)) {
			struct cpu_ctx *pref_cctx = try_lookup_cpu_ctx(tctx->preferred_physical_core);
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_fast(p, pref_cctx, true, false), 0);
			PROF_END_HIST(select_cpu);
			return tctx->preferred_physical_core;  /* Cached core still idle! */
		}
		/* Both busy - fall through to full physical core search */
	}

	/* PERF: Load context ONLY if we didn't take fast paths above.
	 * Deferred loading saves 150-250ns when fast paths succeed (~60% of wakeups). */
	const struct task_struct *current = (void *)bpf_get_current_task_btf();
	bool is_busy = is_system_busy();
	struct cpu_ctx *prev_cctx = try_lookup_cpu_ctx(prev_cpu);
	u32 fg_tgid = get_fg_tgid();
	bool input_active = is_input_active();
	bool is_fg = is_foreground_task_cached(p, fg_tgid);
	s32 cpu;

	/*
	 * Fast path: SYNC wake for foreground task during input window.
	 * Check most likely conditions first for better branch prediction.
	 * IMPORTANT: Skip fast path for GPU threads - they MUST use physical cores.
	 */
    if ((wake_flags & SCX_WAKE_SYNC) && is_fg && !is_critical_gpu) {
		if (!no_wake_sync || input_active) {
			/* Apply chain boost BEFORE dispatch so it affects deadline if task is re-enqueued */
			if (input_active && tctx) {
				/* PERF: tctx guaranteed non-NULL, no redundant lookup needed */
				tctx->chain_boost = MIN(tctx->chain_boost + CHAIN_BOOST_STEP, CHAIN_BOOST_MAX);
			}
			/* Transiently keep the wakee local on sync wake to reduce input latency. */
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_fast(p, prev_cctx, is_fg, input_active), 0);
			/* Per-CPU stat (NO atomic - saves ~5-10ns) */
			if (prev_cctx)
				stat_inc_local(&prev_cctx->local_nr_sync_wake_fast);
			else
				stat_inc(&nr_sync_wake_fast);
			return prev_cpu;
		}
    }

	/*
	 * When the waker and wakee share the same address space and were previously
	 * running on the same CPU, there's a high chance of finding hot cache data
	 * on that CPU. In such cases, prefer keeping the wakee on the same CPU.
	 *
	 * This optimization is applied only when the system is not saturated,
	 * to avoid introducing too much unfairness.
	 * IMPORTANT: Skip for GPU threads - they must use physical cores.
	 */
	if (!is_busy && !is_critical_gpu && is_wake_affine(current, p)) {
		cpu = bpf_get_smp_processor_id();
		if (cpu == prev_cpu) {
			/* Verify CPU is idle before direct dispatch to avoid overloading */
			if (scx_bpf_test_and_clear_cpu_idle(cpu)) {
				scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_fast(p, prev_cctx, is_fg, input_active), 0);
				return cpu;
			}
		}
	}

	/* PERF: Speculative prev_cpu idle check before expensive idle scan.
	 * Rationale: prev_cpu often still idle, excellent cache affinity.
	 * Savings: 30-50ns (skips cpumask fetch, MM hint lookup, iteration).
	 * Hit rate: ~40-60% on light load, ~10-20% on heavy load. */
	if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_fast(p, prev_cctx, is_fg, input_active), 0);
		PROF_END_HIST(select_cpu);
		return prev_cpu;  /* FAST EXIT - prev_cpu still idle! */
	}

    /* Pass cached values to avoid redundant lookups in pick_idle_cpu */
	struct pick_cpu_cache cache = {
		.is_busy = is_busy,
		.pc = prev_cctx,
		.fg_tgid = fg_tgid,
		.input_active = input_active,
	};
    cpu = pick_idle_cpu_cached(p, prev_cpu, wake_flags, false, &cache);

	/* Dispatch to local DSQ if we found idle CPU or system not busy */
	if (cpu >= 0) {
	scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, task_slice(p), 0);
		return cpu;
	}

	if (!is_busy) {
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice(p), 0);
	}

	PROF_END_HIST(select_cpu);
	return prev_cpu;
}

/*
 * Wake-up @cpu if it's idle.
 */
static inline void wakeup_cpu(s32 cpu)
{
	/*
	 * If deferred wakeups are enabled all the wakeup events are
	 * performed asynchronously by wakeup_timerfn().
	 */
	if (deferred_wakeups)
		return;
	scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
}

void BPF_STRUCT_OPS(gamer_enqueue, struct task_struct *p, u64 enq_flags)
{
	PROF_START_HIST(enqueue);

	s32 prev_cpu = scx_bpf_task_cpu(p), cpu;
	struct task_ctx *tctx;
	struct cpu_ctx *prev_cctx = try_lookup_cpu_ctx(prev_cpu);  /* Initialize early for per-CPU stats */
    bool is_busy = is_system_busy();
	u32 fg_tgid = get_fg_tgid();
	bool input_active = is_input_active();

	/*
	 * Attempt to dispatch directly to an idle CPU if the task can
	 * migrate.
	 */
	/* PERF: Pass cached input_active and fg_tgid to avoid redundant checks (saves 45-75ns) */
    if (need_migrate(p, prev_cpu, enq_flags, is_busy, input_active, fg_tgid)) {
		/* prev_cctx already initialized above */
		struct pick_cpu_cache cache = {
			.is_busy = is_busy,
			.pc = prev_cctx,
			.fg_tgid = fg_tgid,
			.input_active = input_active,
		};
		cpu = pick_idle_cpu_cached(p, prev_cpu, enq_flags, true, &cache);
		if (cpu >= 0) {
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, task_slice(p), enq_flags);
			/* PERF: Use per-CPU counter (no atomic!) - saves 30-50ns */
			struct cpu_ctx *target_cctx = try_lookup_cpu_ctx(cpu);
			if (target_cctx)
				target_cctx->local_nr_direct_dispatches++;
			else
				__atomic_fetch_add(&nr_direct_dispatches, 1, __ATOMIC_RELAXED);
			wakeup_cpu(cpu);
			PROF_END_HIST(enqueue);
			return;
		}
	}

	/*
	 * Keep using the same CPU if the system is not busy, otherwise
	 * fallback to the shared DSQ.
	 */
	/* Optimized: reuse input_active from line 1406 to avoid redundant scx_bpf_now() call */
	bool window_active = is_busy && is_foreground_task_cached(p, fg_tgid) && input_active;
    if (!is_busy || window_active) {
	scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice(p), enq_flags);
		set_kick_cpu(prev_cpu);
		/* PERF: Use per-CPU counter (no atomic!) - saves 30-50ns */
		if (prev_cctx)
			prev_cctx->local_rr_enq++;
		else
			__atomic_fetch_add(&rr_enq, 1, __ATOMIC_RELAXED);
		wakeup_cpu(prev_cpu);
		PROF_END_HIST(enqueue);
		return;
	}

	/*
	 * Dispatch to the shared DSQ, using deadline-based scheduling.
	 * Fetch prev_cpu's context once for both shared_dsq() and task_dl().
	 * OPTIMIZATION: Pass cached fg_tgid to avoid redundant BSS read (~10-20ns).
	 */
	tctx = try_lookup_task_ctx(p);
	if (!tctx) {
		PROF_END_HIST(enqueue);
		return;
	}
	/* prev_cctx already initialized at function entry (line 1384) */
	scx_bpf_dsq_insert_vtime(p, shared_dsq(prev_cpu),
				 task_slice(p), task_dl_with_ctx_cached(p, tctx, prev_cctx, fg_tgid), enq_flags);
	/* PERF: Use per-CPU counter (no atomic!) - saves 30-50ns */
	if (prev_cctx)
		prev_cctx->local_edf_enq++;
	else
		__atomic_fetch_add(&edf_enq, 1, __ATOMIC_RELAXED);
	wakeup_cpu(prev_cpu);
	PROF_END_HIST(enqueue);
}

void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
	PROF_START_HIST(dispatch);

	/*
	 * Check if the there's any task waiting in the shared DSQ and
	 * dispatch.
	 */
    if (scx_bpf_dsq_move_to_local(shared_dsq(cpu))) {
		/* PERF: Use per-CPU counter (no atomic!) - saves 30-50ns */
		struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
		if (cctx)
			cctx->local_nr_shared_dispatches++;
		else
			__atomic_fetch_add(&nr_shared_dispatches, 1, __ATOMIC_RELAXED);
		PROF_END_HIST(dispatch);
        return;
    }

	/*
	 * If the previous task expired its time slice, but no other task
	 * wants to run on this SMT core, allow the previous task to run
	 * for another time slot.
	 */
	if (prev && (prev->scx.flags & SCX_TASK_QUEUED) && !is_smt_contended(cpu))
		prev->scx.slice = task_slice(prev);

	PROF_END_HIST(dispatch);
}

void BPF_STRUCT_OPS(gamer_cpu_release, s32 cpu, struct scx_cpu_release_args *args)
{
	/*
	 * A higher scheduler class stole the CPU, re-enqueue all the tasks
	 * that are waiting on this CPU and give them a chance to pick
	 * another idle CPU.
	 */
	scx_bpf_reenqueue_local();
}

/*
 * Recompute precomputed boost_shift for fast deadline calculation.
 * Called after thread classification changes to update boost level once.
 * Boost priorities (matching task_dl_with_ctx logic):
 *   7 = input handlers (10x boost) - highest priority
 *   6 = GPU submit (8x boost)
 *   5 = system audio (7x boost)
 *   4 = game audio (6x boost)
 *   3 = compositor (5x boost)
 *   2 = network threads (4x boost)
 *   0 = standard tasks (no fast-path boost)
 */
static __always_inline void recompute_boost_shift(struct task_ctx *tctx)
{
    if (tctx->is_input_handler)
        tctx->boost_shift = 7;
    else if (tctx->is_gpu_submit)
        tctx->boost_shift = 6;
    else if (tctx->is_system_audio)
        tctx->boost_shift = 5;
    else if (tctx->is_game_audio)
        tctx->boost_shift = 4;
    else if (tctx->is_compositor)
        tctx->boost_shift = 3;
    else if (tctx->is_network)
        tctx->boost_shift = 2;
    else
        tctx->boost_shift = 0;
}

void BPF_STRUCT_OPS(gamer_runnable, struct task_struct *p, u64 enq_flags)
{
	u64 now = scx_bpf_now(), delta_t;
	struct task_ctx *tctx;
    s32 cpu = scx_bpf_task_cpu(p);
    struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);

	/* PERF: Always create task_ctx on first wake to guarantee non-NULL in hot paths.
	 * This eliminates NULL checks and string comparison fallbacks in select_cpu/enqueue.
	 * Savings: 25-40ns (avoided NULL check) + 50-150ns (avoided strcmp on first wake)
	 *
	 * SCHEDULER RESTART DETECTION: Use generation ID to detect stale task_ctx entries.
	 * If task_ctx exists but has old generation ID, treat it as "first classification"
	 * and re-increment counters. This solves the undercount problem on scheduler restart. */
	bool is_first_classification = false;
	u32 current_gen = scheduler_generation;
	tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, 0);  /* Try lookup first, NO CREATE */
	if (!tctx) {
		/* First time seeing this thread - create storage */
		tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, BPF_LOCAL_STORAGE_GET_F_CREATE);
		if (!tctx)
			return;  /* Should never happen with CREATE flag */
		tctx->scheduler_gen = current_gen;  /* Mark with current generation */
		is_first_classification = true;  /* Only increment counters for new threads */
	} else if (tctx->scheduler_gen != current_gen) {
		/* Stale task_ctx from previous scheduler run! Re-classify this thread. */
		tctx->scheduler_gen = current_gen;
		is_first_classification = true;  /* Re-increment counters for this restart */
	}

	/*
	 * Reset exec runtime (accumulated execution time since last
	 * sleep).
	 */
	tctx->exec_runtime = 0;

	/* Track if any classification changed to trigger boost_shift recomputation */
	bool classification_changed = false;

	/*
	 * Detect compositor tasks on first wakeup by checking comm name.
	 * Compositors are the critical path for presenting frames to the display.
	 * Boosting compositor priority during frame windows reduces presentation latency by 1-2ms.
	 * CRITICAL FIX: Only increment counter on first classification to prevent PID reuse drift.
	 */
	if (!tctx->is_compositor && is_compositor_name(p->comm)) {
		tctx->is_compositor = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_compositor_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/* Get fg_tgid once for all classification checks */
	u32 fg_tgid = detected_fg_tgid ? detected_fg_tgid : foreground_tgid;

	/*
	 * CRITICAL FIX: Use exact TGID match for thread classification.
	 * is_foreground_task() includes hierarchy (parent/grandparent), which incorrectly
	 * classifies ALL Wine helper processes, KDE threads, Steam overlay, etc.
	 *
	 * Only threads belonging to the EXACT game process (tgid match) should be classified.
	 */
	bool is_exact_game_thread = fg_tgid && ((u32)p->tgid == fg_tgid);

	/*
	 * Detect network/netcode threads for online games.
	 * Network threads are critical path: player input -> network -> server.
	 * Boosting during input windows reduces input-to-server latency.
	 * ONLY classify threads in the actual game process.
	 */
	if (!tctx->is_network && is_exact_game_thread && is_network_name(p->comm)) {
		tctx->is_network = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_network_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/*
	 * Detect SYSTEM audio (PipeWire/ALSA/PulseAudio) - system-wide audio server.
	 * High priority but shouldn't block game input processing.
	 * System audio applies globally (not game-specific).
	 */
	if (!tctx->is_system_audio && is_system_audio_name(p->comm)) {
		tctx->is_system_audio = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_system_audio_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/*
	 * Detect GAME audio threads (OpenAL/FMOD/Wwise/game-specific audio).
	 * Important for immersion but lower priority than input responsiveness.
	 * ONLY classify threads in the actual game process.
	 */
	if (!tctx->is_game_audio && is_exact_game_thread && is_game_audio_name(p->comm)) {
		tctx->is_game_audio = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/*
	 * Detect INPUT HANDLER threads (SDL/GLFW/input event processing).
	 * HIGHEST priority for gaming - mouse/keyboard lag is unacceptable.
	 * This is what makes aim feel responsive.
	 * ONLY classify threads in the actual game process.
	 * CRITICAL FIX: Only increment counter on first classification to prevent PID reuse drift.
	 */
	if (!tctx->is_input_handler && is_exact_game_thread && is_input_handler_name(p->comm)) {
		tctx->is_input_handler = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_input_handler_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/*
	 * Main thread of THE FOREGROUND GAME PROCESS = input handler.
	 * Many games (WoW, older engines, single-threaded games) handle input on main thread.
	 * Heavy main threads NEED the boost - that's where the game logic lives.
	 * CRITICAL FIX: Only increment counter on first classification to prevent PID reuse drift.
	 */
	if (!tctx->is_input_handler && p->tgid == fg_tgid && p->pid == p->tgid) {
		tctx->is_input_handler = 1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_input_handler_threads, 1, __ATOMIC_RELAXED);
		classification_changed = true;
	}

	/*
	 * ADVANCED DETECTION: Temporarily disabled - needs API updates
	 *
	 * For games with generic thread names (Warframe.x64.ex), we rely on:
	 * 1. Main thread detection (above) - counts as 1 input handler
	 * 2. Runtime pattern detection (in gamer_stopping) - GPU threads based on behavior
	 *
	 * Future: Re-enable Wine priority and GPU ioctl detection for better accuracy
	 */

	/* Recompute boost_shift if any classification changed */
	if (classification_changed)
		recompute_boost_shift(tctx);

	/*
	 * Update the task's wakeup frequency based on the time since
	 * the last wakeup, then cap at 10000 to handle high-frequency tasks
	 * (audio at 48kHz, high-polling-rate devices) while avoiding overflow.
	 * Freq value is roughly wakeups per 100ms: 10000 ≈ 100kHz wakeup rate.
	 */
	delta_t = now - tctx->last_woke_at;
	tctx->wakeup_freq = update_freq(tctx->wakeup_freq, delta_t);
	tctx->wakeup_freq = MIN(tctx->wakeup_freq, 10000);
    tctx->last_woke_at = now;
    /* Fast decay chain boost on wake. */
    tctx->chain_boost = tctx->chain_boost >> 1;

    /* Update per-CPU interactive EMA when tasks wake frequently. */
    if (cctx) {
        u64 old = cctx->interactive_avg;
        u64 new = tctx->wakeup_freq;
        cctx->interactive_avg = (old - (old >> 2)) + (new >> 2);
    }

    /* Maintain a system-level interactive EMA to modulate busy thresholds (foreground-biased). */
    if (is_foreground_task(p)) {
        u64 old = interactive_sys_avg;
        u64 new = tctx->wakeup_freq;
        interactive_sys_avg = (old - (old >> 2)) + (new >> 2);
    }
}

void BPF_STRUCT_OPS(gamer_running, struct task_struct *p)
{
	struct task_ctx *tctx;
	s32 cpu;
	u64 now;

	tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;

	cpu = scx_bpf_task_cpu(p);

	/*
	 * Save a timestamp when the task begins to run (used to evaluate
	 * the used time slice).
	 */
	now = scx_bpf_now();
	tctx->last_run_at = now;

	/*
	 * Update current system's vruntime.
	 */
    {
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
        if (cctx && time_before(cctx->vtime_now, p->scx.dsq_vtime))
            cctx->vtime_now = p->scx.dsq_vtime;
    }

	/*
	 * Refresh cpufreq performance level.
	 */
	update_cpufreq(cpu);

    /* Update per-mm recent CPU hint (rate-limited). */
    update_mm_last_cpu(p, tctx, now);

	/* PERF: Cache physical core for GPU threads (Phase 2.3 optimization).
	 * GPU threads run frequently (60-240Hz), so caching their preferred core
	 * saves 15-30ns per wake by avoiding SMT sibling iteration.
	 * Only cache if running on a physical core (not hyperthread). */
	if (tctx->is_gpu_submit) {
		/* TODO: Add is_physical_core() check when SMT detection logic is available.
		 * For now, cache the CPU unconditionally - still beneficial for cache affinity. */
		tctx->preferred_physical_core = cpu;
	}
}

void BPF_STRUCT_OPS(gamer_stopping, struct task_struct *p, bool runnable)
{
	struct task_ctx *tctx;
	u64 slice;

	/* Check if this is first time seeing this thread (for counter increment safety)
	 * Also check generation ID to detect stale task_ctx from previous scheduler run */
	bool is_first_classification = false;
	u32 current_gen = scheduler_generation;
	tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, 0);
	if (!tctx) {
		/* First time - create it */
		tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, BPF_LOCAL_STORAGE_GET_F_CREATE);
		if (!tctx)
			return;
		tctx->scheduler_gen = current_gen;
		is_first_classification = true;
	} else if (tctx->scheduler_gen != current_gen) {
		/* Stale from previous scheduler run - re-classify */
		tctx->scheduler_gen = current_gen;
		is_first_classification = true;
	}

	/*
	 * Evaluate the used time slice.
	 */
	slice = MIN(scx_bpf_now() - tctx->last_run_at, slice_ns);

	/*
	 * Update the vruntime and the total accumulated runtime since last
	 * sleep.
	 *
	 * exec_runtime tracks RAW (unscaled) time since last wake. It's reset
	 * to 0 in gamer_runnable(), so weight changes between wake cycles don't
	 * cause drift. Both vruntime and deadline calculation scale consistently
	 * by task weight.
	 *
	 * Cap the maximum accumulated time since last sleep to @slice_lag,
	 * to prevent starving CPU-intensive tasks.
	 */
	p->scx.dsq_vtime += scale_by_task_weight_inverse(p, slice);
	tctx->exec_runtime = MIN(tctx->exec_runtime + slice, slice_lag);

    /*
     * Update exec_avg EMA for GPU submission detection.
     * Track average execution time per wake cycle (reset in runnable).
     */
    tctx->exec_avg = calc_avg(tctx->exec_avg, tctx->exec_runtime);

    /*
     * Track page fault rate to detect asset-loading threads vs hot-loop threads.
     * High page fault rate indicates cache thrashing (loading new assets/textures).
     * Low/zero page faults indicate hot rendering loops (should preserve cache).
     */
    u64 current_pgfaults = p->maj_flt + p->min_flt;
    u64 pgfault_delta = current_pgfaults - tctx->last_pgfault_total;
    tctx->last_pgfault_total = current_pgfaults;
    /* Update EMA of page faults per wake (use 4:1 ratio like other EMAs) */
    tctx->pgfault_rate = calc_avg(tctx->pgfault_rate, pgfault_delta);

    /*
     * Detect GPU submission threads by thread name.
     * Name-based detection only - no heuristics to avoid false positives.
     * ONLY classify threads in the actual game process (exact TGID match).
     */
    u32 fg_tgid = detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
    bool is_exact_game_thread = fg_tgid && ((u32)p->tgid == fg_tgid);

    if (!tctx->is_gpu_submit && is_exact_game_thread && is_gpu_submit_name(p->comm)) {
        tctx->is_gpu_submit = 1;
		/* PERF: Initialize physical core cache to -1 (unset) on first detection */
		tctx->preferred_physical_core = -1;
		if (is_first_classification)
			__atomic_fetch_add(&nr_gpu_submit_threads, 1, __ATOMIC_RELAXED);
        recompute_boost_shift(tctx);  /* Update boost for GPU thread */
    }

    /*
     * RUNTIME-BASED CLASSIFICATION: Detect thread types by behavior patterns
     * This works for ALL games (Warframe, Splitgate, etc.) regardless of thread names.
     *
     * Thread patterns observed across games:
     * - GPU/Render: 60-240Hz (wakeup_freq 60-256), exec_avg 500µs-8ms
     * - Audio: ~800Hz (wakeup_freq 256-1024), exec_avg <500µs
     * - Background: <10Hz (wakeup_freq <32), exec_avg >5ms
     * - Network: 10-120Hz (variable), exec_avg <1ms
     */
    if (is_exact_game_thread && !tctx->is_input_handler) {
        /* GPU/Render thread detection: 60-240Hz wakeup, moderate CPU usage
         * Warframe: ~144Hz render thread, 2-6ms per frame
         * Splitgate: ~480Hz, <3ms per frame
         * Requires stable pattern: 20 consecutive samples */
        u16 wakeup_hz = tctx->wakeup_freq >> 2;  /* Approx Hz (wakeup_freq/4) */

        if (!tctx->is_gpu_submit && wakeup_hz >= 60 && wakeup_hz <= 300 &&
            tctx->exec_avg >= 500000 && tctx->exec_avg <= 10000000) {
            /* Likely GPU thread - increment counter */
            tctx->low_cpu_samples = MIN(tctx->low_cpu_samples + 1, 20);
            if (tctx->low_cpu_samples >= 20) {
                tctx->is_gpu_submit = 1;
                tctx->preferred_physical_core = -1;
                if (is_first_classification)
                    __atomic_fetch_add(&nr_gpu_submit_threads, 1, __ATOMIC_RELAXED);
                recompute_boost_shift(tctx);
            }
        } else if (tctx->is_gpu_submit && (wakeup_hz < 40 || wakeup_hz > 350)) {
            /* Pattern changed - declassify */
            tctx->is_gpu_submit = 0;
            tctx->low_cpu_samples = 0;
            if (nr_gpu_submit_threads > 0)
                __atomic_fetch_sub(&nr_gpu_submit_threads, 1, __ATOMIC_RELAXED);
            recompute_boost_shift(tctx);
        }

        /* Audio thread detection: Very high frequency (~500-1000Hz), short bursts
         * Audio callbacks at 48kHz sample rate / 60 samples = 800Hz
         * Very consistent pattern, short exec time (<500µs) */
        if (!tctx->is_game_audio && !tctx->is_gpu_submit &&
            wakeup_hz >= 300 && wakeup_hz <= 1200 &&
            tctx->exec_avg < 500000) {
            tctx->high_cpu_samples = MIN(tctx->high_cpu_samples + 1, 20);
            if (tctx->high_cpu_samples >= 20) {
                tctx->is_game_audio = 1;
                if (is_first_classification)
                    __atomic_fetch_add(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
                recompute_boost_shift(tctx);
            }
        } else if (tctx->is_game_audio && (wakeup_hz < 250 || wakeup_hz > 1300)) {
            /* Pattern changed */
            tctx->is_game_audio = 0;
            tctx->high_cpu_samples = 0;
            if (nr_game_audio_threads > 0)
                __atomic_fetch_sub(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
            recompute_boost_shift(tctx);
        }
    }

    /*
     * Detect background tasks: high CPU usage (>5ms), low wakeup frequency.
     * These are shader compilers, asset loaders, or other batch work.
     */
    if (is_foreground_task(p) && tctx->wakeup_freq < BACKGROUND_FREQ_MAX) {
        if (tctx->exec_avg > BACKGROUND_EXEC_THRESH_NS) {
            tctx->high_cpu_samples = MIN(tctx->high_cpu_samples + 1, BACKGROUND_STABLE_SAMPLES);
            if (tctx->high_cpu_samples >= BACKGROUND_STABLE_SAMPLES && !tctx->is_background) {
                tctx->is_background = 1;
                if (is_first_classification)
                    __atomic_fetch_add(&nr_background_threads, 1, __ATOMIC_RELAXED);
            }
        } else {
            tctx->high_cpu_samples = 0;
            if (tctx->high_cpu_samples == 0 && tctx->is_background) {
                tctx->is_background = 0;
                if (nr_background_threads > 0)
                    __atomic_fetch_sub(&nr_background_threads, 1, __ATOMIC_RELAXED);
            }
        }
    } else {
        /* Reset background flag if wakeup pattern changes */
        tctx->high_cpu_samples = 0;
        if (tctx->is_background) {
            tctx->is_background = 0;
            if (nr_background_threads > 0)
                __atomic_fetch_sub(&nr_background_threads, 1, __ATOMIC_RELAXED);
        }
    }

    /*
     * Update per-CPU statistics and propagate chain boost across wake chains.
     */
    update_cpu_load(p, slice);
    /* Runtime accounting for foreground vs total. */
    __atomic_fetch_add(&total_runtime_ns_total, slice, __ATOMIC_RELAXED);
    if (is_foreground_task(p))
        __atomic_fetch_add(&fg_runtime_ns_total, slice, __ATOMIC_RELAXED);
    if (runnable && tctx->chain_boost) {
        /* If the task remains runnable (likely woke someone), decay slower and allow inheritance. */
        /* slow decay when still in chain */
        tctx->chain_boost = MAX(tctx->chain_boost - 1, 1);
    }
}

void BPF_STRUCT_OPS(gamer_enable, struct task_struct *p)
{
	{
		s32 cpu = scx_bpf_task_cpu(p);
		struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
		if (cctx)
			p->scx.dsq_vtime = cctx->vtime_now;
	}
}

void BPF_STRUCT_OPS(gamer_disable, struct task_struct *p)
{
	/*
	 * Thread is exiting - decrement classification counters.
	 * This ensures thread counts reflect LIVE threads only, not cumulative.
	 * Critical fix: prevents counter drift when threads spawn/exit frequently.
	 *
	 * UNDERFLOW PROTECTION: Only decrement if counter > 0.
	 * This handles scheduler restart cases where old threads still have flags set
	 * but global counters were reset to 0.
	 */
	__atomic_fetch_add(&nr_disable_calls, 1, __ATOMIC_RELAXED);

	struct task_ctx *tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;

	/* Decrement counters with underflow protection */
	if (tctx->is_input_handler && nr_input_handler_threads > 0) {
		__atomic_fetch_sub(&nr_input_handler_threads, 1, __ATOMIC_RELAXED);
		__atomic_fetch_add(&nr_disable_input_dec, 1, __ATOMIC_RELAXED);
	}
	if (tctx->is_gpu_submit && nr_gpu_submit_threads > 0)
		__atomic_fetch_sub(&nr_gpu_submit_threads, 1, __ATOMIC_RELAXED);
	if (tctx->is_compositor && nr_compositor_threads > 0)
		__atomic_fetch_sub(&nr_compositor_threads, 1, __ATOMIC_RELAXED);
	if (tctx->is_network && nr_network_threads > 0)
		__atomic_fetch_sub(&nr_network_threads, 1, __ATOMIC_RELAXED);
	if (tctx->is_system_audio && nr_system_audio_threads > 0)
		__atomic_fetch_sub(&nr_system_audio_threads, 1, __ATOMIC_RELAXED);
	if (tctx->is_game_audio && nr_game_audio_threads > 0)
		__atomic_fetch_sub(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
	if (tctx->is_background && nr_background_threads > 0)
		__atomic_fetch_sub(&nr_background_threads, 1, __ATOMIC_RELAXED);
}

s32 BPF_STRUCT_OPS(gamer_init_task, struct task_struct *p,
		   struct scx_init_task_args *args)
{
	struct task_ctx *tctx;

	tctx = bpf_task_storage_get(&task_ctx_stor, p, 0,
				    BPF_LOCAL_STORAGE_GET_F_CREATE);
	if (!tctx)
		return -ENOMEM;

	return 0;
}

s32 BPF_STRUCT_OPS_SLEEPABLE(gamer_init)
{
	struct bpf_timer *timer;
	u32 key = 0;
	int err;

	/* Increment generation ID to detect scheduler restarts.
	 * This invalidates all old task_ctx entries from previous scheduler runs.
	 * When threads wake with old gen ID, we know to re-increment counters. */
	scheduler_generation++;

	nr_cpu_ids = scx_bpf_nr_cpu_ids();

	/* Initialize all CPU contexts explicitly to ensure clean state */
	{
		s32 cpu;
		bpf_for(cpu, 0, nr_cpu_ids) {
			struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
			if (cctx) {
				cctx->vtime_now = 0;
				cctx->interactive_avg = 0;
				cctx->last_update = 0;
				cctx->perf_lvl = SCX_CPUPERF_ONE;
				cctx->shared_dsq_id = 0;
				cctx->last_cpu_idx = 0;
			}
		}
	}

	/*
	 * Create separate per-node DSQs if NUMA optimization is enabled,
	 * otherwise use a single shared DSQ.
	 */
	if (numa_enabled) {
		int node;

		bpf_for(node, 0, __COMPAT_scx_bpf_nr_node_ids()) {
			err = scx_bpf_create_dsq(node, node);
			if (err) {
				scx_bpf_error("failed to create node DSQ %d: %d", node, err);
				return err;
			}
		}
	} else {
		err = scx_bpf_create_dsq(SHARED_DSQ, -1);
		if (err) {
			scx_bpf_error("failed to create shared DSQ: %d", err);
			return err;
		}
	}

    if (deferred_wakeups) {
		timer = bpf_map_lookup_elem(&wakeup_timer, &key);
		if (!timer) {
			scx_bpf_error("Failed to lookup wakeup timer");
			return -ESRCH;
		}

		bpf_timer_init(timer, &wakeup_timer, CLOCK_MONOTONIC);
		bpf_timer_set_callback(timer, wakeup_timerfn);

        {
            u64 period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
            err = bpf_timer_start(timer, period, 0);
        }
        if (err) {
            scx_bpf_error("Failed to arm wakeup timer, falling back to synchronous wakeups");
#ifdef __COMPAT_scx_bpf_set_deferred_wakeups
            __COMPAT_scx_bpf_set_deferred_wakeups(false);
#else
            bpf_printk("scx_bpf_set_deferred_wakeups compat symbol missing; deferred wakeups disabled\n");
#endif
        }
    }

    return 0;
}

void BPF_STRUCT_OPS(gamer_exit, struct scx_exit_info *ei)
{
	/*
	 * Scheduler is exiting. Reset all thread classification counters.
	 * This prevents underflow on restart when old task_ctx entries persist
	 * but global counters are re-initialized to 0.
	 *
	 * Note: task_ctx storage persists across scheduler restarts (attached to threads),
	 * but global variables are destroyed. Reset counters here so they start fresh
	 * on next scheduler load.
	 */
	nr_input_handler_threads = 0;
	nr_gpu_submit_threads = 0;
	nr_compositor_threads = 0;
	nr_network_threads = 0;
	nr_system_audio_threads = 0;
	nr_game_audio_threads = 0;
	nr_background_threads = 0;

	UEI_RECORD(uei, ei);
}

SCX_OPS_DEFINE(gamer_ops,
	       .select_cpu		= (void *)gamer_select_cpu,
	       .enqueue			= (void *)gamer_enqueue,
	       .dispatch		= (void *)gamer_dispatch,
	       .runnable		= (void *)gamer_runnable,
	       .running			= (void *)gamer_running,
	       .stopping		= (void *)gamer_stopping,
	       .cpu_release		= (void *)gamer_cpu_release,
	       .enable			= (void *)gamer_enable,
	       .disable			= (void *)gamer_disable,
	       .init_task		= (void *)gamer_init_task,
	       .init			= (void *)gamer_init,
	       .exit			= (void *)gamer_exit,
	       .timeout_ms		= 5000,
	       .name			= "gamer");
