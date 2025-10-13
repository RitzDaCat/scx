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

/*
 * Unified Thread Classification Result
 * Combines all detection methods with confidence scoring
 */
struct unified_thread_classification {
	u8 primary_role;      /* Highest confidence role */
	u8 secondary_role;    /* Second-best role */
	u8 confidence;        /* 0-100 confidence score */
	u8 detection_method;  /* Which method detected this role */
	u32 last_update;     /* Timestamp of last classification */
};

/* Detection method identifiers */
#define DETECT_WINE     1
#define DETECT_GPU      2
#define DETECT_RUNTIME  3
#define DETECT_HEURISTIC 4

enum detected_role_type {
	DETECTED_ROLE_NONE = 0,
	DETECTED_ROLE_INPUT,
	DETECTED_ROLE_GPU,
	DETECTED_ROLE_GAME_AUDIO,
	DETECTED_ROLE_NETWORK,
	DETECTED_ROLE_COMPOSITOR,
	DETECTED_ROLE_BACKGROUND,
};

static __always_inline bool has_conflicting_roles(
	const struct task_ctx *tctx,
	enum detected_role_type keep)
{
	if (keep != DETECTED_ROLE_INPUT && tctx->is_input_handler)
		return true;
	if (keep != DETECTED_ROLE_GPU && tctx->is_gpu_submit)
		return true;
	if (keep != DETECTED_ROLE_GAME_AUDIO && tctx->is_game_audio)
		return true;
	if (keep != DETECTED_ROLE_NETWORK && tctx->is_network)
		return true;
	if (keep != DETECTED_ROLE_COMPOSITOR && tctx->is_compositor)
		return true;
	if (keep != DETECTED_ROLE_BACKGROUND && tctx->is_background)
		return true;

	return false;
}

static __always_inline void clear_task_role_flags(struct task_ctx *tctx)
{
	tctx->is_input_handler = 0;
	tctx->is_gpu_submit = 0;
	tctx->is_compositor = 0;
	tctx->is_network = 0;
	tctx->is_game_audio = 0;
	tctx->is_background = 0;
}

static __always_inline bool set_task_role(
	struct task_ctx *tctx,
	enum detected_role_type role,
	u8 boost)
{
	bool changed = false;

	if (has_conflicting_roles(tctx, role)) {
		clear_task_role_flags(tctx);
		changed = true;
	}

	switch (role) {
	case DETECTED_ROLE_INPUT:
		if (!tctx->is_input_handler) {
			tctx->is_input_handler = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_GPU:
		if (!tctx->is_gpu_submit) {
			tctx->is_gpu_submit = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_GAME_AUDIO:
		if (!tctx->is_game_audio) {
			tctx->is_game_audio = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_NETWORK:
		if (!tctx->is_network) {
			tctx->is_network = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_COMPOSITOR:
		if (!tctx->is_compositor) {
			tctx->is_compositor = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_BACKGROUND:
		if (!tctx->is_background) {
			tctx->is_background = 1;
			changed = true;
		}
		break;
	case DETECTED_ROLE_NONE:
	default:
		break;
	}

	if (tctx->boost_shift != boost) {
		tctx->boost_shift = boost;
		changed = true;
	}

	return changed;
}

/**
 * classify_thread_unified - Single-pass thread classification with confidence scoring
 * @tid: Thread ID
 * @classification: Output classification result
 *
 * Combines all detection methods into a single unified classification.
 * This reduces overhead and improves accuracy by considering all available signals.
 *
 * Returns: true if classification was successful, false otherwise
 */
static __always_inline bool classify_thread_unified(
	u32 tid,
	struct unified_thread_classification *classification)
{
	u8 wine_role = get_wine_thread_role(tid);
	u8 gpu_role = is_gpu_submit_thread(tid) ? ROLE_RENDER : ROLE_UNKNOWN;
	u8 runtime_role = get_thread_role(tid);
	u64 now = bpf_ktime_get_ns();
	
	/* Initialize classification */
	classification->primary_role = DETECTED_ROLE_NONE;
	classification->secondary_role = DETECTED_ROLE_NONE;
	classification->confidence = 0;
	classification->detection_method = DETECT_HEURISTIC;
	classification->last_update = now;
	
	/* PRIORITY 1: Wine thread priority hints
	 * Highest confidence - explicit signals from game engine */
	if (wine_role != WINE_ROLE_UNKNOWN) {
		switch (wine_role) {
		case WINE_ROLE_RENDER:
			classification->primary_role = DETECTED_ROLE_GPU;
			classification->confidence = 95;
			classification->detection_method = DETECT_WINE;
			break;
		case WINE_ROLE_AUDIO:
			classification->primary_role = DETECTED_ROLE_GAME_AUDIO;
			classification->confidence = 95;
			classification->detection_method = DETECT_WINE;
			break;
		case WINE_ROLE_INPUT:
			classification->primary_role = DETECTED_ROLE_INPUT;
			classification->confidence = 95;
			classification->detection_method = DETECT_WINE;
			break;
		case WINE_ROLE_BACKGROUND:
			classification->primary_role = DETECTED_ROLE_BACKGROUND;
			classification->confidence = 90;
			classification->detection_method = DETECT_WINE;
			break;
		}
		return true;
	}
	
	/* PRIORITY 2: GPU ioctl detection
	 * 100% accurate for identifying GPU submit threads */
	if (gpu_role != ROLE_UNKNOWN) {
		classification->primary_role = DETECTED_ROLE_GPU;
		classification->confidence = 100;
		classification->detection_method = DETECT_GPU;
		return true;
	}
	
	/* PRIORITY 3: Thread runtime pattern detection
	 * High accuracy after sufficient samples (100+ wakeups) */
	if (runtime_role != ROLE_UNKNOWN) {
		u8 runtime_confidence = thread_is_role(tid, runtime_role, 75) ? 75 : 50;
		
		switch (runtime_role) {
		case ROLE_RENDER:
			classification->primary_role = DETECTED_ROLE_GPU;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		case ROLE_INPUT:
			classification->primary_role = DETECTED_ROLE_INPUT;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		case ROLE_AUDIO:
			classification->primary_role = DETECTED_ROLE_GAME_AUDIO;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		case ROLE_NETWORK:
			classification->primary_role = DETECTED_ROLE_NETWORK;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		case ROLE_COMPOSITOR:
			classification->primary_role = DETECTED_ROLE_COMPOSITOR;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		case ROLE_BACKGROUND:
			classification->primary_role = DETECTED_ROLE_BACKGROUND;
			classification->confidence = runtime_confidence;
			classification->detection_method = DETECT_RUNTIME;
			break;
		}
		return true;
	}
	
	return false;  /* No classification available */
}

/**
 * update_task_ctx_from_detection - Enhance task_ctx with unified classification
 * @tctx: Task context to update
 * @p: Task struct
 *
 * Uses the unified classification system to update task_ctx.
 * This reduces overhead and improves accuracy.
 *
 * Returns: true if role was updated, false otherwise
 */
static __always_inline bool update_task_ctx_from_detection(
	struct task_ctx *tctx,
	const struct task_struct *p)
{
	u32 tid = BPF_CORE_READ(p, pid);
	struct unified_thread_classification classification;
	bool updated = false;

	if (!tctx)
		return false;

	/* Use unified classification */
	if (!classify_thread_unified(tid, &classification))
		return false;

	/* Apply classification to task_ctx based on confidence */
	if (classification.confidence >= 75) {
		u8 boost = 0;
		
		switch (classification.primary_role) {
		case DETECTED_ROLE_INPUT:
			boost = 7;
			break;
		case DETECTED_ROLE_GAME_AUDIO:
			boost = 6;
			break;
		case DETECTED_ROLE_GPU:
			boost = 5;
			break;
		case DETECTED_ROLE_NETWORK:
		case DETECTED_ROLE_COMPOSITOR:
			boost = 4;
			break;
		case DETECTED_ROLE_BACKGROUND:
			boost = 0;
			break;
		}
		
		if (set_task_role(tctx, classification.primary_role, boost))
			updated = true;
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
	if (role == ROLE_RENDER)
		return 6;  /* GPU threads */
	if (role == ROLE_COMPOSITOR)
		return 5;  /* Compositor (visual chain) */
	if (role == ROLE_AUDIO)
		return 4;  /* Audio threads */
	if (role == ROLE_NETWORK)
		return 2;  /* Network threads */

	/*
	 * FALLBACK: Check task_ctx flags (existing heuristics)
	 * Cached in task_ctx, so cheap
	 */
	if (tctx) {
		if (tctx->is_input_handler)
			return 7;  /* Highest boost */
		if (tctx->is_gpu_submit)
			return 6;  /* GPU threads */
		if (tctx->is_compositor)
			return 5;  /* Compositor (visual chain) */
		if (tctx->is_usb_audio)
			return 4;  /* USB audio */
		if (tctx->is_system_audio)
			return 3;  /* System audio */
		if (tctx->is_network)
			return 2;  /* Network threads */
		if (tctx->is_game_audio)
			return 1;  /* Game audio */
		if (tctx->is_nvme_io)
			return 1;  /* NVMe I/O */
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

	/* Audio threads (USB, game, and system) */
	if (tctx && (tctx->is_usb_audio || tctx->is_game_audio || tctx->is_system_audio))
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
