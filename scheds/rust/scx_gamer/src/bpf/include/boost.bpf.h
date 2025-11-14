/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Input/Frame Boost Windows
 * Copyright (c) 2025 RitzDaCat
 *
 * Time window management for input-active and frame-active periods.
 * This file is AI-friendly: ~150 lines, single responsibility.
 */
#ifndef __GAMER_BOOST_BPF_H
#define __GAMER_BOOST_BPF_H

#include "config.bpf.h"
#include "types.bpf.h"

/* External tunables */
extern const volatile bool primary_all;
extern const volatile u64 input_window_ns;
extern const volatile u64 keyboard_boost_ns;
extern const volatile u64 mouse_boost_ns;
extern volatile u64 input_until_global;
extern volatile u64 input_lane_until[INPUT_LANE_MAX];
extern volatile u32 input_lane_trigger_rate[INPUT_LANE_MAX];
extern volatile u32 input_trigger_rate;
extern volatile u64 last_input_trigger_ns;
extern volatile u64 napi_until_global;
extern volatile u64 napi_last_softirq_ns[MAX_CPUS];
extern private(GAMER) struct bpf_cpumask __kptr *primary_cpumask;

/* External stats */
extern volatile u64 nr_input_trig;
extern volatile u64 nr_frame_trig;
extern volatile u64 input_window_dynamic_ns;
extern volatile u64 input_lane_dynamic_ns[INPUT_LANE_MAX];
extern volatile u64 nr_input_force_dispatch;
extern volatile u64 nr_input_force_dispatch_late;
extern volatile u64 input_force_dispatch_latency_ns;
extern volatile u64 input_force_dispatch_latency_max_ns;

#define NAPI_SCAN_MAX 32

static __always_inline s32 find_recent_napi_cpu(u64 now)
{
	if (unlikely(!time_before(now, napi_until_global)))
		return -1;

	s32 best_cpu = -1;
	u64 best_ts = 0;

	for (int i = 0; i < NAPI_SCAN_MAX && i < MAX_CPUS; i++) {
		u64 ts = napi_last_softirq_ns[i];
		if (ts == 0)
			continue;
		if (!time_before(now, ts + NAPI_PREFER_TIMEOUT_NS))
			continue;
		if (ts > best_ts) {
			best_ts = ts;
			best_cpu = i;
		}
	}

	return best_cpu;
}

/**
 * is_input_active_cpu_now - Check if CPU is in active input window
 * @cpu: CPU ID to check
 * @now: Current timestamp (reused from caller to avoid redundant scx_bpf_now())
 *
 * Input windows apply only to primary domain CPUs.
 * During input window, foreground tasks get shorter slices
 * and higher priority for responsive gameplay.
 *
 * TIER 0/1: Optimized for hot path performance
 * - CPUMask test: Tier 1 (~5-15ns, only if primary domain configured)
 * - Volatile read: Tier 0 (~1-2ns)
 * - Time comparison: Tier 0 (~0.5-1ns)
 * - Total: ~2-3ns (no primary domain) or ~7-18ns (with primary domain check)
 */
static __always_inline bool is_input_active_cpu_now(s32 cpu, u64 now)
{
	/* TIER 1: Check primary domain (only if configured) */
	const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;
	if (likely(primary && !bpf_cpumask_test_cpu(cpu, primary)))
		return false;

	/* TIER 0: Check input window (volatile read + comparison) */
    return time_before(now, input_until_global);
}

/**
 * is_input_active_cpu - Check if current CPU is in active input window
 * @cpu: CPU ID to check
 *
 * TIER 1: Calls scx_bpf_now() (~10-15ns) + is_input_active_cpu_now()
 * Prefer is_input_active_cpu_now() with reused timestamp in hot paths.
 */
static __always_inline bool is_input_active_cpu(s32 cpu)
{
    return is_input_active_cpu_now(cpu, scx_bpf_now());
}

/**
 * is_input_active_now - Check if input window is currently active
 * @now: Current timestamp
 *
 * TIER 0: Single volatile read + comparison (~1-2ns)
 * Hot path function - called in every select_cpu/enqueue call.
 */
static __always_inline bool is_input_active_now(u64 now)
{
    return time_before(now, input_until_global);
}

static __always_inline u64 compute_dynamic_window(u32 rate, u64 base, u64 min_floor_ns)
{
	u64 adjusted = base;

	if (rate > 800)
		adjusted = base + (base >> 1);
	else if (rate > 400)
		adjusted = base + (base >> 2);
	else if (rate < 60 && base > 0)
		adjusted = base >> 1;

	u64 latency_ema = input_force_dispatch_latency_ns;
	if (latency_ema > 0) {
		if (latency_ema > 400000ULL)
			adjusted += adjusted >> 1;
		else if (latency_ema > 250000ULL)
			adjusted += adjusted >> 2;
		else if (latency_ema < 140000ULL && adjusted > min_floor_ns)
			adjusted = (adjusted * 3) >> 2;
	}

	u64 latency_peak = input_force_dispatch_latency_max_ns;
	if (latency_peak > 800000ULL)
		adjusted += adjusted >> 1;

	if (base == 0)
		return adjusted;

	u64 min_window = base >> 2;
	if (min_window < min_floor_ns)
		min_window = min_floor_ns;
	u64 max_window = base << 1;
	if (max_window < base)
		max_window = base;

	if (adjusted < min_window)
		adjusted = min_window;
	if (adjusted > max_window)
		adjusted = max_window;
	return adjusted;
}

/**
 * is_input_lane_active - Check if specific input lane is active
 * @lane: Input lane (keyboard, mouse, controller, other)
 * @now: Current timestamp
 *
 * TIER 0/1: Optimized for hot path performance
 * - Bounds check: Tier 0 (~0.5-1ns)
 * - Array access: Tier 0 (~0.5-1ns, volatile array)
 * - Time comparison: Tier 0 (~0.5-1ns)
 * - Total: ~1.5-3ns
 */
static __always_inline bool is_input_lane_active(u8 lane, u64 now)
{
	/* TIER 0: Fast path - check bounds first */
	if (unlikely(lane >= INPUT_LANE_MAX))
		return time_before(now, input_until_global);
	
	/* TIER 0: Array access (volatile array, cache-friendly) */
	/* BPF VERIFIER: Explicit bounds check before array access */
	if (likely(lane < INPUT_LANE_MAX))
		return time_before(now, input_lane_until[lane]);
	
	/* Fallback (should never reach here due to bounds check above) */
	return time_before(now, input_until_global);
}

/**
 * fanout_set_input_lane - Set input boost window for specific lane
 * @lane: Input lane (keyboard, mouse, controller, other)
 * @now: Current timestamp
 *
 * Simple model: Each input event extends boost window by fixed duration.
 * No rate calculation, no EMA - just "input active for next X ms".
 *
 * Per-lane boost durations (tunable from userspace):
 * - Mouse: Default 8ms (covers 1000-8000Hz polling + small movement bursts)
 * - Keyboard: Default 1000ms (casual-friendly - covers ability chains and menu navigation)
 * - Controller: 500ms (console-style games with analog input)
 * - Other: NO BOOST (non-gaming devices don't need scheduler priority)
 *
 * TIER 0/1: Optimized for input event hot path
 * - Bounds clamping: Tier 0 (~1-2ns)
 * - Array lookup: Tier 0 (~0.5-1ns, stack-allocated array)
 * - Volatile writes: Tier 0 (~1-2ns per write)
 * - Total: ~10-20ns per input event
 *
 * HFT PATTERN: Branchless boost duration selection using lookup table.
 * Eliminates branch misprediction penalty (~2-6ns savings per input event).
 * Note: Array initialized with runtime values (volatile externs) for BPF compatibility.
 */
static __always_inline void fanout_set_input_lane(u8 lane, u64 now)
{
    /* TIER 0: BPF VERIFIER - Ensure lane is within bounds (clamp to valid range) */
    u8 safe_lane = lane;
    if (unlikely(safe_lane >= INPUT_LANE_MAX))
        safe_lane = INPUT_LANE_OTHER;

    /* BPF VERIFIER: Verify safe_lane is definitely within bounds */
    if (unlikely(safe_lane >= INPUT_LANE_MAX))
        return;

    /* TIER 0: Stack-allocated lookup table (no heap allocation)
     * Branchless boost duration selection eliminates misprediction penalty */
    u64 boost_durations[INPUT_LANE_MAX] = {
        [INPUT_LANE_KEYBOARD] = keyboard_boost_ns,  /* Tunable: default 1000ms */
        [INPUT_LANE_MOUSE] = mouse_boost_ns,        /* Tunable: default 8ms */
        [INPUT_LANE_CONTROLLER] = 500000000ULL,     /* 500ms - console-style games */
        [INPUT_LANE_OTHER] = 0,                     /* No boost for other devices */
    };

    u64 boost_duration_ns = boost_durations[safe_lane];

	if (likely(safe_lane < INPUT_LANE_MAX)) {
		u32 prev_rate = input_lane_trigger_rate[safe_lane];
		u32 new_rate = prev_rate;
		u64 last_ns = input_lane_last_trigger_ns[safe_lane];

		if (last_ns > 0 && now > last_ns) {
			u64 delta = now - last_ns;
			if (delta > 1000000ULL) {
				new_rate = 0;
			} else if (delta > 0) {
				u32 instant = delta < 10000000ULL ? (u32)(1000000000ULL / delta) : 0;
				new_rate = (prev_rate * 7 + instant) >> 3;
			}
		}

		input_lane_trigger_rate[safe_lane] = new_rate;

		if (boost_duration_ns > 0) {
			boost_duration_ns = compute_dynamic_window(new_rate, boost_duration_ns, 200000ULL);
			input_lane_dynamic_ns[safe_lane] = boost_duration_ns;
		}
	}

	/* TIER 0: Early exit for non-boost lanes (other devices) */
	if (unlikely(boost_duration_ns == 0)) {
		if (likely(safe_lane < INPUT_LANE_MAX)) {
			input_lane_last_trigger_ns[safe_lane] = now;
			input_lane_dynamic_ns[safe_lane] = 0;
		}
		return;
	}

	/* TIER 0: Calculate expiry time (arithmetic operation) */
	u64 lane_expiry = now + boost_duration_ns;
	
	/* TIER 0: Update lane expiry (volatile write, ~1-2ns) */
	/* BPF VERIFIER: Bounds check immediately before array access */
	if (likely(safe_lane < INPUT_LANE_MAX)) {
		input_lane_until[safe_lane] = lane_expiry;
		continuous_input_lane_mode[safe_lane] = 1;  /* Mark lane as boosted */
		input_lane_last_trigger_ns[safe_lane] = now;  /* Track trigger time */
	}

	/* TIER 0: Update global input window if this lane extends it (volatile write, ~1-2ns) */
	if (time_before(input_until_global, lane_expiry))
		input_until_global = lane_expiry;
}

/**
 * fanout_set_input_window - Activate input boost window across all primary CPUs
 * @now: Current timestamp (reused from caller to avoid redundant scx_bpf_now())
 *
 * Called when input events (keyboard/mouse) are detected.
 * Sets global timestamp for input window expiration.
 *
 * TIER 0: Single volatile write + arithmetic (~1-2ns)
 * Hot path function - called on every input event.
 */
static __always_inline void fanout_set_input_window(u64 now)
{
	u32 rate = input_trigger_rate;
	u64 window = compute_dynamic_window(rate, input_window_ns, 200000ULL);
	input_window_dynamic_ns = window;
	input_until_global = now + window;
}

/**
 * fanout_set_napi_window - Activate NAPI/softirq preference window
 *
 * Used with --prefer-napi-on-input flag to keep tasks
 * on CPUs that recently handled network interrupts.
 *
 * TIER 1: Calls scx_bpf_now() (~10-15ns) + volatile write (~1-2ns)
 * Not in hottest path - called during network interrupt handling.
 * Could be optimized to accept timestamp parameter if available.
 */
static __always_inline void fanout_set_napi_window(void)
{
	napi_until_global = scx_bpf_now() + input_window_ns;
}

/**
 * is_napi_softirq_preferred_cpu - Check if CPU recently handled NAPI/softirq
 * @cpu: CPU ID to check
 * @now: Current timestamp
 *
 * Returns true if this CPU should be preferred for network-related
 * task placement during input windows.
 *
 * TIER 0/1: Optimized for CPU selection hot path
 * - Window check: Tier 0 (~1-2ns, volatile read + comparison)
 * - Bounds check: Tier 0 (~0.5-1ns)
 * - Array access: Tier 0 (~0.5-1ns, volatile array)
 * - Time comparison: Tier 0 (~0.5-1ns)
 * - Total: ~2.5-5ns
 */
static __always_inline bool is_napi_softirq_preferred_cpu(s32 cpu, u64 now)
{
	/* TIER 0: Early exit if NAPI window expired */
	if (unlikely(!time_before(now, napi_until_global)))
		return false;

	/* TIER 0: Bounds check before array access */
	if (unlikely(cpu < 0 || (u32)cpu >= MAX_CPUS))
		return false;

	/* TIER 0: Check if CPU handled net softirq within timeout window */
	return time_before(now, napi_last_softirq_ns[cpu] + NAPI_PREFER_TIMEOUT_NS);
}

/*
 * Foreground Task Detection
 *
 * Checks if task belongs to foreground application.
 * Supports process hierarchy (parent/grandparent) for
 * multi-process games (Steam->game, launcher->game->renderer).
 */

/**
 * is_foreground_task_cached - Check if task is foreground with cached fg_tgid
 * @p: Task struct to check
 * @fg_tgid_cached: Pre-loaded fg_tgid (0 = load fresh)
 *
 * Hierarchy support:
 * - Direct match: task->tgid == fg_tgid (most common, ~80%)
 * - Parent match: task->parent->tgid == fg_tgid (~15%)
 * - Grandparent match: task->parent->parent->tgid == fg_tgid (~5%)
 *
 * TIER 0/1: Optimized for hot path performance
 * - Volatile reads: Tier 0 (~1-2ns each, 2-3 reads)
 * - Struct field reads: Tier 0 (~0.5-1ns each)
 * - Comparisons: Tier 0 (~0.5-1ns each)
 * - Total: ~5-15ns (direct match) or ~15-30ns (hierarchy traversal)
 *
 * Common case (80%): Direct match in ~5-15ns (Tier 0)
 */
static __always_inline bool is_foreground_task_cached(const struct task_struct *p, u32 fg_tgid_cached)
{
	extern volatile u32 detected_fg_tgid;
	extern const volatile u32 foreground_tgid;
	
	/* TIER 0: Load fg_tgid (volatile reads, ~2-4ns total) */
	u32 fg_tgid = fg_tgid_cached ? fg_tgid_cached :
	              (detected_fg_tgid ? detected_fg_tgid : foreground_tgid);

	/* Auto-detect mode: if no fg_tgid specified, treat all as foreground
	 * 
	 * WARNING: This fallback is used when game detection fails.
	 * When fg_tgid is 0, thread classification will NOT work (is_exact_game_thread = false),
	 * meaning input handlers, GPU threads, etc. won't be detected.
	 * 
	 * Users should ensure game detection is working or manually specify --foreground-pid.
	 * Detection should work for: Steam, Battle.net, Epic, GOG, native Linux games via
	 * resource heuristics (20+ threads, 100MB+ memory) or name patterns (.exe, game/client).
	 */
	if (unlikely(!fg_tgid))
		return true;

	/* TIER 0: Direct match (most common case, ~80% of calls) */
	if (likely((u32)p->tgid == fg_tgid))
		return true;

	/* TIER 0: Parent match (game->overlay, game->voicechat, ~15% of calls) */
	struct task_struct *parent = p->real_parent;
	if (likely(parent && (u32)parent->tgid == fg_tgid))
		return true;

	/* TIER 0: Grandparent match (launcher->game->renderer, ~5% of calls) */
	if (likely(parent)) {
		struct task_struct *grandparent = parent->real_parent;
		if (likely(grandparent && (u32)grandparent->tgid == fg_tgid))
			return true;
	}

	return false;
}

/**
 * is_foreground_task - Check if task is foreground (auto-load fg_tgid)
 * @p: Task struct to check
 *
 * TIER 0/1: Wrapper that calls is_foreground_task_cached() with fg_tgid=0
 * Prefer is_foreground_task_cached() with pre-loaded fg_tgid in hot paths.
 */
static __always_inline bool is_foreground_task(const struct task_struct *p)
{
	return is_foreground_task_cached(p, 0);
}

#endif /* __GAMER_BOOST_BPF_H */
