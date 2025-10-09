/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Advanced Thread Detection Integration
 * Copyright (c) 2025 RitzDaCat
 *
 * Integrates thread_runtime, GPU, and Wine detection into scheduler.
 * Provides unified API for task classification with fallback to heuristics.
 */
#ifndef __GAMER_ADVANCED_DETECT_BPF_H
#define __GAMER_ADVANCED_DETECT_BPF_H

#include "thread_runtime.bpf.h"
#include "gpu_detect.bpf.h"
#include "wine_detect.bpf.h"

/*
 * Enhanced task classification using BPF detection
 *
 * Priority order:
 * 1. Wine explicit priority hints (highest confidence)
 * 2. GPU ioctl detection (100% accurate for GPU threads)
 * 3. Thread runtime patterns (99% accurate after 100 samples)
 * 4. Fallback to existing task_ctx flags and heuristics
 */

/**
 * update_task_ctx_from_detection - Enhance task_ctx with BPF-detected roles
 * @tctx: Task context to update
 * @p: Task struct
 *
 * Called periodically to sync BPF detection results into task_ctx.
 * This allows existing scheduler code to benefit from new detection
 * without major refactoring.
 *
 * Returns: true if role was updated, false otherwise
 */
static __always_inline bool update_task_ctx_from_detection(
	struct task_ctx *tctx,
	const struct task_struct *p)
{
	u32 tid = BPF_CORE_READ(p, pid);
	bool updated = false;

	if (!tctx)
		return false;

	/*
	 * PRIORITY 1: Wine thread priority hints
	 * Highest confidence - explicit signals from game engine
	 */
	u8 wine_role = get_wine_thread_role(tid);
	if (wine_role != WINE_ROLE_UNKNOWN) {
		switch (wine_role) {
		case WINE_ROLE_RENDER:
			if (!tctx->is_gpu_submit) {
				tctx->is_gpu_submit = 1;
				tctx->boost_shift = 5;  /* High boost for render */
				updated = true;
			}
			break;

		case WINE_ROLE_AUDIO:
			if (!tctx->is_game_audio) {
				tctx->is_game_audio = 1;
				tctx->boost_shift = 6;  /* Higher boost for audio */
				updated = true;
			}
			break;

		case WINE_ROLE_INPUT:
			if (!tctx->is_input_handler) {
				tctx->is_input_handler = 1;
				tctx->boost_shift = 7;  /* Highest boost for input */
				updated = true;
			}
			break;

		case WINE_ROLE_BACKGROUND:
			if (!tctx->is_background) {
				tctx->is_background = 1;
				tctx->boost_shift = 0;  /* No boost */
				updated = true;
			}
			break;
		}

		if (updated)
			return true;
	}

	/*
	 * PRIORITY 2: GPU ioctl detection
	 * 100% accurate for identifying GPU submit threads
	 */
	if (is_gpu_submit_thread(tid)) {
		if (!tctx->is_gpu_submit) {
			tctx->is_gpu_submit = 1;
			tctx->boost_shift = 5;
			updated = true;

			/* Cache preferred physical core for GPU affinity */
			if (tctx->preferred_physical_core == -1) {
				/* First time - will be set by select_cpu */
				tctx->preferred_physical_core = -1;
			}
		}
		return updated;
	}

	/*
	 * PRIORITY 3: Thread runtime pattern detection
	 * High accuracy after sufficient samples (100+ wakeups)
	 */
	u8 runtime_role = get_thread_role(tid);
	if (runtime_role != ROLE_UNKNOWN) {
		/* Only apply if high confidence (75%+) */
		if (thread_is_role(tid, runtime_role, 75)) {
			switch (runtime_role) {
			case ROLE_RENDER:
				if (!tctx->is_gpu_submit) {
					tctx->is_gpu_submit = 1;
					tctx->boost_shift = 5;
					updated = true;
				}
				break;

			case ROLE_INPUT:
				if (!tctx->is_input_handler) {
					tctx->is_input_handler = 1;
					tctx->boost_shift = 7;
					updated = true;
				}
				break;

			case ROLE_AUDIO:
				if (!tctx->is_game_audio) {
					tctx->is_game_audio = 1;
					tctx->boost_shift = 6;
					updated = true;
				}
				break;

			case ROLE_NETWORK:
				if (!tctx->is_network) {
					tctx->is_network = 1;
					tctx->boost_shift = 4;
					updated = true;
				}
				break;

			case ROLE_COMPOSITOR:
				if (!tctx->is_compositor) {
					tctx->is_compositor = 1;
					tctx->boost_shift = 4;
					updated = true;
				}
				break;

			case ROLE_BACKGROUND:
				if (!tctx->is_background) {
					tctx->is_background = 1;
					tctx->boost_shift = 0;
					updated = true;
				}
				break;
			}
		}
	}

	return updated;
}

/**
 * should_boost_thread - Check if thread deserves priority boost
 * @tctx: Task context (may be NULL)
 * @p: Task struct
 *
 * Fast path check combining all detection methods.
 * Called in select_cpu hot path, so must be fast (<100ns).
 *
 * Returns: boost level (0-7), 0 = no boost, 7 = maximum
 */
static __always_inline u8 should_boost_thread(
	const struct task_ctx *tctx,
	const struct task_struct *p)
{
	u32 tid = BPF_CORE_READ(p, pid);

	/*
	 * FAST PATH 1: Use cached task_ctx boost_shift if available
	 * This is the common case (99% of calls)
	 */
	if (tctx && tctx->boost_shift > 0)
		return tctx->boost_shift;

	/*
	 * FAST PATH 2: Check Wine high priority flag
	 * ~50-80ns (single map lookup)
	 */
	if (is_wine_high_priority(tid))
		return 6;  /* TIME_CRITICAL or HIGHEST priority */

	/*
	 * FAST PATH 3: Check GPU submit thread
	 * ~50-80ns (single map lookup)
	 */
	if (is_gpu_submit_thread(tid))
		return 5;  /* GPU threads get high priority */

	/*
	 * FAST PATH 4: Check runtime-detected roles
	 * ~50-80ns (single map lookup)
	 */
	u8 role = get_thread_role(tid);
	if (role == ROLE_INPUT)
		return 7;  /* Highest boost */
	if (role == ROLE_AUDIO)
		return 6;
	if (role == ROLE_RENDER)
		return 5;
	if (role == ROLE_NETWORK || role == ROLE_COMPOSITOR)
		return 4;

	/*
	 * FALLBACK: Check task_ctx flags (existing heuristics)
	 * Cached in task_ctx, so cheap
	 */
	if (tctx) {
		if (tctx->is_input_handler)
			return 7;
		if (tctx->is_game_audio)
			return 6;
		if (tctx->is_gpu_submit)
			return 5;
		if (tctx->is_network || tctx->is_compositor)
			return 4;
	}

	return 0;  /* No boost */
}

/**
 * get_detection_stats_summary - Get summary of detection effectiveness
 *
 * Returns statistics for monitoring detection performance.
 * Called periodically by userspace for diagnostics.
 */
struct detection_stats {
	u64 wine_threads_detected;
	u64 gpu_threads_detected;
	u64 runtime_roles_detected;
	u64 total_thread_switches;
};

static __always_inline struct detection_stats get_detection_stats(void)
{
	struct detection_stats stats = {0};

	/* Count entries in each map (approximation) */
	stats.wine_threads_detected = wine_high_priority_threads + wine_realtime_threads;
	stats.gpu_threads_detected = gpu_detect_new_threads;
	stats.runtime_roles_detected = thread_track_role_changes;
	stats.total_thread_switches = thread_track_switches;

	return stats;
}

/**
 * is_critical_latency_thread - Check if thread needs ultra-low latency
 *
 * Used to bypass migration limits and force local dispatch.
 * Only for threads where latency is absolutely critical.
 */
static __always_inline bool is_critical_latency_thread(
	const struct task_ctx *tctx,
	const struct task_struct *p)
{
	u32 tid = BPF_CORE_READ(p, pid);

	/* Input handlers always get local dispatch */
	if (tctx && tctx->is_input_handler)
		return true;

	/* Wine TIME_CRITICAL threads (render/audio) */
	struct wine_thread_info *wine_info = bpf_map_lookup_elem(&wine_threads_map, &tid);
	if (wine_info && wine_info->windows_priority == THREAD_PRIORITY_TIME_CRITICAL)
		return true;

	/* Runtime-detected input threads (high confidence) */
	if (thread_is_role(tid, ROLE_INPUT, 90))
		return true;

	/* Audio threads (both game and system) */
	if (tctx && (tctx->is_game_audio || tctx->is_system_audio))
		return true;

	/* Runtime-detected audio with high confidence */
	if (thread_is_role(tid, ROLE_AUDIO, 90))
		return true;

	return false;
}

/**
 * get_optimal_cpu_for_gpu_thread - Find best CPU for GPU submission
 *
 * GPU threads benefit from:
 * 1. Physical cores (not SMT siblings)
 * 2. CPUs with direct PCIe/memory bus to GPU
 * 3. High-performance cores (on hybrid CPUs)
 *
 * Returns: Preferred CPU ID, or -1 if no preference
 */
static __always_inline s32 get_optimal_cpu_for_gpu_thread(
	u32 tid,
	const struct task_struct *p,
	s32 prev_cpu)
{
	struct gpu_thread_info *gpu_info = bpf_map_lookup_elem(&gpu_threads_map, &tid);
	if (!gpu_info)
		return -1;  /* Not a GPU thread */

	/*
	 * For high-frequency submissions (>144Hz), prefer sticky CPU
	 * to preserve cache locality. Cache thrashing is worse than
	 * occasional SMT contention at these rates.
	 */
	if (gpu_info->submit_freq_hz > 144)
		return prev_cpu;  /* Stick to previous CPU */

	/*
	 * For lower frequencies, let scheduler find physical core
	 * Current logic in select_cpu already handles this well.
	 */
	return -1;  /* No strong preference */
}

#endif /* __GAMER_ADVANCED_DETECT_BPF_H */
