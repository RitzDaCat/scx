/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Virtual Time & Deadline Calculations
 * Copyright (c) 2025 RitzDaCat
 *
 * Deadline calculations with gaming-specific thread priorities.
 * This file is AI-friendly: ~200 lines, single responsibility.
 */
#ifndef __GAMER_VTIME_BPF_H
#define __GAMER_VTIME_BPF_H

#include "config.bpf.h"
#include "types.bpf.h"

/* External tunables */
extern const volatile u64 slice_lag;
extern volatile u64 input_until_global;

/* External helper (defined in main.bpf.c) */
extern u64 scale_by_task_weight(const struct task_struct *p, u64 value);
extern u64 scale_by_task_weight_inverse(const struct task_struct *p, u64 value);

/*
 * Get foreground TGID (runtime-updatable or configured)
 */
static inline u32 get_fg_tgid(void)
{
	extern volatile u32 detected_fg_tgid;
	extern const volatile u32 foreground_tgid;
	return detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
}

/*
 * Calculate time slice for task @p
 *
 * Factors:
 * - Input/frame boost windows: shorter slices for fast preemption
 * - Per-CPU interactive average: shrink slice when system is busy
 * - Task wakeup frequency: shorter for highly interactive tasks
 * - Task weight: scale slice proportionally
 *
 * @cctx: optional pre-fetched cpu_ctx (NULL to auto-fetch)
 * @fg_tgid: optional pre-loaded fg_tgid (0 to load fresh)
 */
static u64 task_slice_with_ctx_cached(const struct task_struct *p, struct cpu_ctx *cctx, u32 fg_tgid)
{
	extern const volatile u64 slice_ns;
	extern bool is_foreground_task_cached(const struct task_struct *p, u32 fg_tgid);
	u64 s = slice_ns;
	struct task_ctx *tctx = try_lookup_task_ctx(p);

	/* Fetch cpu_ctx once if needed */
	if (!cctx) {
		s32 cpu = scx_bpf_task_cpu(p);
		cctx = try_lookup_cpu_ctx(cpu);
	}

	/* Adjust slices during active input window (foreground tasks only) */
	if (is_foreground_task_cached(p, fg_tgid) && cctx) {
		u64 now = scx_bpf_now();
		if (time_before(now, input_until_global)) {
			s = s >> 1;  /* Halve slice for fast preemption */
		}
	}

	/* Scale slice by per-CPU interactive activity average */
	if (cctx && cctx->interactive_avg > INTERACTIVE_SLICE_SHRINK_THRESH)
		s = (s * 3) >> 2;  /* 75% of normal slice */

	/* Shorter slice for highly interactive tasks */
	if (tctx && tctx->wakeup_freq > 256)
		s = s >> 1;

	return scale_by_task_weight(p, s);
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
 * Calculate virtual deadline for task @p
 *
 * Gaming-Optimized Priority Order (during input window):
 * 1. Input handlers (10x boost)    - mouse/keyboard lag is WORST
 * 2. GPU submit (8x boost)          - visual feedback critical
 * 3. System audio (7x boost)        - voice chat quality
 * 4. Game audio (6x boost)          - immersion
 * 5. Compositor (5x boost)          - frame presentation
 * 6. Network (4x boost)             - online gameplay
 * 7. Foreground tasks (4x boost)    - general game logic
 * 8. Background tasks (penalty)     - deprioritize cache pollution
 *
 * Formula:
 *   deadline = vruntime + exec_vruntime
 *
 * Where:
 * - vruntime: total accumulated runtime (inversely scaled by weight)
 * - exec_vruntime: runtime since last sleep (inversely scaled by weight)
 *
 * @cctx: optional pre-fetched cpu_ctx (NULL to auto-fetch)
 */
static u64 task_dl_with_ctx(struct task_struct *p, struct task_ctx *tctx, struct cpu_ctx *cctx)
{
	u32 fg_tgid;
	u64 now, exec_component, wake_factor, vsleep_max, vbase, vtime_min;
	bool in_input_window;

	/* Safety: return safe default if tctx is NULL */
	if (!tctx)
		return p->scx.dsq_vtime;

	/* Check boost windows */
	fg_tgid = get_fg_tgid();
	now = scx_bpf_now();
	in_input_window = time_before(now, input_until_global);

	/*
	 * Gaming Priority Fast Paths (during boost windows)
	 */

	/* PRIORITY 1: Input handlers - HIGHEST priority */
	if (tctx->is_input_handler && in_input_window)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 7);  /* 10x boost */

	/* PRIORITY 2: GPU submission threads */
	if (tctx->is_gpu_submit)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 6);  /* 8x boost */

	/* PRIORITY 3: System audio (PipeWire/ALSA) */
	if (tctx->is_system_audio)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 5) + (tctx->exec_runtime >> 6);  /* 7x */

	/* PRIORITY 4: Game audio */
	if (tctx->is_game_audio)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 5) + (tctx->exec_runtime >> 7);  /* 6x */

	/* PRIORITY 5: Compositor */
	if (tctx->is_compositor)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 5);  /* 5x boost */

	/* PRIORITY 6: Network threads (during input window) */
	if (tctx->is_network && in_input_window)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 4);  /* 4x boost */

	/* PRIORITY 7: Foreground game threads (during input window) */
	if (fg_tgid && (u32)p->tgid == fg_tgid && in_input_window)
		return p->scx.dsq_vtime + (tctx->exec_runtime >> 4);  /* 4x boost */

	/*
	 * Standard Path: Full deadline calculation with vruntime limiting
	 */

	/* Calculate wakeup frequency factor */
	wake_factor = 1;
	if (tctx->wakeup_freq > 0)
		wake_factor = MIN(1 + (tctx->wakeup_freq >> WAKE_FREQ_SHIFT), CHAIN_BOOST_MAX);

	vsleep_max = scale_by_task_weight(p, slice_lag * wake_factor);

	/* Get vtime baseline */
	if (!cctx) {
		s32 cpu = scx_bpf_task_cpu(p);
		cctx = try_lookup_cpu_ctx(cpu);
	}
	vbase = cctx ? cctx->vtime_now : 0;

	/* Prevent underflow: vtime_min = max(0, vbase - vsleep_max) */
	vtime_min = vbase > vsleep_max ? vbase - vsleep_max : 0;

	/* Update task vtime if it's fallen too far behind */
	if (time_before(p->scx.dsq_vtime, vtime_min))
		p->scx.dsq_vtime = vtime_min;

	/* Calculate exec component (inversely scaled by weight) */
	exec_component = scale_by_task_weight_inverse(p, tctx->exec_runtime);

	/* Thread class modifiers */
	if (tctx->is_gpu_submit)
		exec_component = exec_component >> 2;  /* 4x deadline boost */
	else if (tctx->is_background)
		exec_component = exec_component << 2;  /* 4x penalty (later deadline) */

	/* Page fault penalty: deprioritize asset-loading threads */
	if (tctx->pgfault_rate > 50 && !tctx->is_input_handler &&
	    !tctx->is_system_audio && !tctx->is_gpu_submit)
		exec_component = (exec_component * 3) >> 1;  /* 1.5x penalty */

	/* Reduce exec impact for highly interactive tasks */
	if (wake_factor > 1)
		exec_component = exec_component / wake_factor;

	/* Chain boost: synchronous wake chains get deadline bonus */
	if (tctx->chain_boost > 0)
		exec_component = exec_component / (1 + tctx->chain_boost);

	return p->scx.dsq_vtime + exec_component;
}

static u64 task_dl(struct task_struct *p)
{
	struct task_ctx *tctx = try_lookup_task_ctx(p);
	return task_dl_with_ctx(p, tctx, NULL);
}

#endif /* __GAMER_VTIME_BPF_H */
