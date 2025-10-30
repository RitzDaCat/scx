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
extern volatile u64 last_input_trigger_ns;
extern volatile u64 napi_until_global;
extern volatile u64 napi_last_softirq_ns[MAX_CPUS];
extern private(GAMER) struct bpf_cpumask __kptr *primary_cpumask;

/* External stats */
extern volatile u64 nr_input_trig;
extern volatile u64 nr_frame_trig;

/*
 * Check if CPU is in active input window
 *
 * Input windows apply only to primary domain CPUs.
 * During input window, foreground tasks get shorter slices
 * and higher priority for responsive gameplay.
 */
static __always_inline bool is_input_active_cpu_now(s32 cpu, u64 now)
{
	const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;

	/* Check if CPU is in primary domain */
	if (primary && !bpf_cpumask_test_cpu(cpu, primary))
		return false;

	/* Check if we're within the input window */
    return time_before(now, input_until_global);
}

/*
 * Check if current CPU is in active input window
 */
static __always_inline bool is_input_active_cpu(s32 cpu)
{
    return is_input_active_cpu_now(cpu, scx_bpf_now());
}

static __always_inline bool is_input_active_now(u64 now)
{
    return time_before(now, input_until_global);
}

static __always_inline bool is_input_lane_active(u8 lane, u64 now)
{
	if (lane >= INPUT_LANE_MAX)
		return time_before(now, input_until_global);
	/* BPF VERIFIER: Explicit bounds check before array access */
	if (lane < INPUT_LANE_MAX)
		return time_before(now, input_lane_until[lane]);
	return time_before(now, input_until_global);
}

static __always_inline void fanout_set_input_lane(u8 lane, u64 now)
{
    /* BPF VERIFIER: Ensure lane is within bounds - clamp to valid range */
    u8 safe_lane = lane;
    if (safe_lane >= INPUT_LANE_MAX)
        safe_lane = INPUT_LANE_OTHER;
    
    /* BPF VERIFIER: Verify safe_lane is definitely within bounds */
    if (safe_lane >= INPUT_LANE_MAX)
        return;

    /* Simple model: Each input event extends boost window by fixed duration.
     * No rate calculation, no EMA - just "input active for next X ms".
     * 
     * Per-lane boost durations (tunable from userspace):
     * - Mouse: Default 8ms (covers 1000-8000Hz polling + small movement bursts)
     * - Keyboard: Default 1000ms (casual-friendly - covers ability chains and menu navigation)
     * - Controller: 500ms (console-style games with analog input)
     * - Other: NO BOOST (non-gaming devices don't need scheduler priority)
     * 
     * HFT PATTERN: Branchless boost duration selection using lookup table.
     * Eliminates branch misprediction penalty (~2-6ns savings per input event).
     * Note: Array initialized with runtime values (volatile externs) for BPF compatibility.
     */
    u64 boost_durations[INPUT_LANE_MAX] = {
        [INPUT_LANE_KEYBOARD] = keyboard_boost_ns,  /* Tunable: default 1000ms */
        [INPUT_LANE_MOUSE] = mouse_boost_ns,        /* Tunable: default 8ms */
        [INPUT_LANE_CONTROLLER] = 500000000ULL,     /* 500ms - console-style games */
        [INPUT_LANE_OTHER] = 0,                     /* No boost for other devices */
    };
    
    /* BPF VERIFIER: Explicit bounds check immediately before array access */
    if (safe_lane >= INPUT_LANE_MAX)
        return;
    u64 boost_duration_ns = boost_durations[safe_lane];
    
    if (boost_duration_ns == 0) {
        /* Other devices: no boost. Track event but don't prioritize.
         * This prevents touchpads, system devices, etc. from affecting game performance. */
        /* BPF VERIFIER: Bounds check immediately before array access */
        if (safe_lane < INPUT_LANE_MAX)
            input_lane_last_trigger_ns[safe_lane] = now;
        return;  /* Early return - no boost window extension */
    }

    /* Extend boost window: each input pushes expiry forward */
    /* BPF VERIFIER: Bounds check immediately before each array access */
    if (safe_lane >= INPUT_LANE_MAX)
        return;
    u64 lane_expiry = now + boost_duration_ns;
    
    if (safe_lane >= INPUT_LANE_MAX)
        return;
    input_lane_until[safe_lane] = lane_expiry;
    
    if (safe_lane >= INPUT_LANE_MAX)
        return;
    continuous_input_lane_mode[safe_lane] = 1;  /* Mark lane as boosted */

    /* Update global input window if this lane extends it */
    if (time_before(input_until_global, lane_expiry))
        input_until_global = lane_expiry;
    
    /* Track last trigger time for statistics/debugging */
    /* BPF VERIFIER: Bounds check before array access */
    if (safe_lane < INPUT_LANE_MAX)
        input_lane_last_trigger_ns[safe_lane] = now;
}

/*
 * Activate input boost window across all primary CPUs
 *
 * Called when input events (keyboard/mouse) are detected.
 * Sets global timestamp for input window expiration.
 */
static __always_inline void fanout_set_input_window(u64 now)
{
    input_until_global = now + input_window_ns;
}

/*
 * Activate NAPI/softirq preference window
 *
 * Used with --prefer-napi-on-input flag to keep tasks
 * on CPUs that recently handled network interrupts.
 */
static __always_inline void fanout_set_napi_window(void)
{
	napi_until_global = scx_bpf_now() + input_window_ns;
}

/*
 * Check if CPU recently handled NAPI/softirq
 *
 * Returns true if this CPU should be preferred for network-related
 * task placement during input windows.
 */
static __always_inline bool is_napi_softirq_preferred_cpu(s32 cpu, u64 now)
{
	if (!time_before(now, napi_until_global))
		return false;

	if (cpu < 0 || (u32)cpu >= MAX_CPUS)
		return false;

	/* Favor CPUs that handled net softirq within the timeout window */
	return time_before(now, napi_last_softirq_ns[cpu] + NAPI_PREFER_TIMEOUT_NS);
}

/*
 * Foreground Task Detection
 *
 * Checks if task belongs to foreground application.
 * Supports process hierarchy (parent/grandparent) for
 * multi-process games (Steam->game, launcher->game->renderer).
 */

/*
 * Check if task is foreground with cached fg_tgid
 *
 * @fg_tgid_cached: Pre-loaded fg_tgid (0 = load fresh)
 *
 * Hierarchy support:
 * - Direct match: task->tgid == fg_tgid
 * - Parent match: task->parent->tgid == fg_tgid
 * - Grandparent match: task->parent->parent->tgid == fg_tgid
 */
static __always_inline bool is_foreground_task_cached(const struct task_struct *p, u32 fg_tgid_cached)
{
	extern volatile u32 detected_fg_tgid;
	extern const volatile u32 foreground_tgid;
	u32 fg_tgid = fg_tgid_cached ? fg_tgid_cached :
	              (detected_fg_tgid ? detected_fg_tgid : foreground_tgid);

	/* Auto-detect mode: if no fg_tgid specified, treat all as foreground */
	if (!fg_tgid)
		return true;

	/* Direct match */
	if ((u32)p->tgid == fg_tgid)
		return true;

	/* Parent match (game->overlay, game->voicechat) */
	struct task_struct *parent = p->real_parent;
	if (parent && (u32)parent->tgid == fg_tgid)
		return true;

	/* Grandparent match (launcher->game->renderer) */
	if (parent) {
		struct task_struct *grandparent = parent->real_parent;
		if (grandparent && (u32)grandparent->tgid == fg_tgid)
			return true;
	}

	return false;
}

/*
 * Check if task is foreground (auto-load fg_tgid)
 */
static __always_inline bool is_foreground_task(const struct task_struct *p)
{
	return is_foreground_task_cached(p, 0);
}

#endif /* __GAMER_BOOST_BPF_H */
