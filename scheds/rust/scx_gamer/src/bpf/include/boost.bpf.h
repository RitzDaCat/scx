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
extern volatile u64 input_until_global;
extern volatile u64 napi_until_global;
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
static __always_inline bool is_input_active_cpu(s32 cpu)
{
	const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;

	/* Check if CPU is in primary domain */
	if (primary && !bpf_cpumask_test_cpu(cpu, primary))
		return false;

	/* Check if we're within the input window */
	return time_before(scx_bpf_now(), input_until_global);
}

/*
 * Check if current CPU is in active input window
 */
static __always_inline bool is_input_active(void)
{
	return is_input_active_cpu(bpf_get_smp_processor_id());
}

/*
 * Activate input boost window across all primary CPUs
 *
 * Called when input events (keyboard/mouse) are detected.
 * Sets global timestamp for input window expiration.
 */
static __always_inline void fanout_set_input_window(void)
{
	input_until_global = scx_bpf_now() + input_window_ns;
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
static __always_inline bool is_napi_softirq_preferred_cpu(s32 cpu)
{
	/* NAPI preference tracking would go here */
	/* For now, just check if we're in the NAPI window */
	return time_before(scx_bpf_now(), napi_until_global);
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
