/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Statistics Collection
 * Copyright (c) 2025 RitzDaCat
 *
 * Conditional statistics helpers for performance monitoring.
 * This file is AI-friendly: ~100 lines, single responsibility.
 */
#ifndef __GAMER_STATS_BPF_H
#define __GAMER_STATS_BPF_H

#include "config.bpf.h"

/* External tunable */
extern const volatile bool no_stats;

/*
 * Statistics Counters (BSS - zero-initialized, accumulate over time)
 *
 * TIER 0: Volatile counters (fast atomic increments, ~1-2ns)
 * Used for performance monitoring and debugging.
 * All counters are conditional on !no_stats for zero overhead when disabled.
 */

/* Enqueue/dispatch stats */
extern volatile u64 rr_enq;
extern volatile u64 edf_enq;
extern volatile u64 nr_direct_dispatches;
extern volatile u64 nr_shared_dispatches;

/* Migration stats */
extern volatile u64 nr_migrations;
extern volatile u64 nr_mig_blocked;
extern volatile u64 nr_sync_local;
extern volatile u64 nr_frame_mig_block;

/* System load metrics */
extern volatile u64 cpu_util;
extern volatile u64 cpu_util_avg;
extern volatile u64 interactive_sys_avg;

/* Window activity accounting */
extern volatile u64 win_input_ns_total;
extern volatile u64 win_frame_ns_total;
extern volatile u64 timer_elapsed_ns_total;

/* Selection quality metrics */
extern volatile u64 nr_idle_cpu_pick;
/* MM hint removed - was extern volatile u64 nr_mm_hint_hit; */

/* Runtime accounting */
extern volatile u64 fg_runtime_ns_total;
extern volatile u64 total_runtime_ns_total;

/* Trigger counters */
extern volatile u64 nr_input_trig;
extern volatile u64 nr_frame_trig;
extern volatile u64 nr_input_force_dispatch;
extern volatile u64 nr_input_force_dispatch_late;
extern volatile u64 input_force_dispatch_latency_ns;
extern volatile u64 input_force_dispatch_latency_max_ns;
extern volatile u64 input_window_dynamic_ns;
extern volatile u64 input_lane_dynamic_ns[INPUT_LANE_MAX];
extern volatile u64 frame_phase_cpu_ns;
extern volatile u64 frame_phase_gpu_ns;
extern volatile u64 frame_phase_events;
extern volatile u64 frame_phase_gpu_dominant;
extern volatile u64 frame_phase_cpu_dominant;
extern volatile u8 power_hint_level;
extern volatile u64 power_hint_expiry_ns;
extern volatile u64 power_hint_remaining_ns;
extern volatile u64 nr_power_hint_updates;

/* Frame feedback loop metrics */
extern volatile u64 nr_frame_feedback_escalations;
extern volatile u64 nr_frame_feedback_recoveries;
extern volatile u64 nr_frame_feedback_miss_events;

/* GPU thread affinity */
extern volatile u64 nr_gpu_phys_kept;
extern volatile u64 nr_gpu_pref_fallback;
extern volatile u64 nr_taskgraph_borrow_grants;

/* Fast path counters */
extern volatile u64 nr_sync_wake_fast;

/* Thread classification counts */
extern volatile u64 nr_gpu_submit_threads;
extern volatile u64 nr_background_threads;
extern volatile u64 nr_compositor_threads;
extern volatile u64 nr_network_threads;
extern volatile u64 nr_system_audio_threads;
extern volatile u64 nr_game_audio_threads;
extern volatile u64 nr_input_handler_threads;
extern volatile u64 nr_taskgraph_threads;	/* Unreal Engine TaskGraph workers (UE5.6 DX12) */

/**
 * stat_inc - Conditional stats increment (no-op if stats disabled)
 * @counter: Pointer to volatile counter to increment
 *
 * Uses atomic operations - slower but works from any context.
 *
 * TIER 0/1: Conditional atomic increment
 * - Conditional check: Tier 0 (~0.5-1ns)
 * - Atomic increment: Tier 0 (~1-2ns, when enabled)
 * - Total: ~0.5-1ns (disabled) or ~1.5-3ns (enabled)
 *
 * Frequency: Called throughout scheduler hot paths
 * Net overhead: Zero when disabled, minimal when enabled
 */
#if CONFIG_GAMER_ENABLE_EXTENDED_STATS
static __always_inline void stat_inc(volatile u64 *counter)
{
	/* TIER 0: Early exit if stats disabled (~0.5-1ns) */
	if (likely(!no_stats))
		/* TIER 0: Atomic increment (~1-2ns) */
		__atomic_fetch_add(counter, 1, __ATOMIC_RELAXED);
}

/**
 * stat_add - Conditional stats add (no-op if stats disabled)
 * @counter: Pointer to volatile counter to add to
 * @value: Value to add to counter
 *
 * Uses atomic operations - slower but works from any context.
 *
 * TIER 0/1: Conditional atomic add
 * - Conditional check: Tier 0 (~0.5-1ns)
 * - Atomic add: Tier 0 (~1-2ns, when enabled)
 * - Total: ~0.5-1ns (disabled) or ~1.5-3ns (enabled)
 */
static __always_inline void stat_add(volatile u64 *counter, u64 value)
{
	/* TIER 0: Early exit if stats disabled (~0.5-1ns) */
	if (likely(!no_stats))
		/* TIER 0: Atomic add (~1-2ns) */
		__atomic_fetch_add(counter, value, __ATOMIC_RELAXED);
}

/**
 * stat_inc_local - Per-CPU stat increment (NO atomics needed!)
 * @local_counter: Pointer to per-CPU local counter (in cpu_ctx)
 *
 * Increments a per-CPU local counter stored in cpu_ctx.
 * These are aggregated to globals periodically by the timer.
 *
 * TIER 0: Optimized for hot path performance
 * - Conditional checks: Tier 0 (~0.5-1ns each)
 * - Direct increment: Tier 0 (~0.5-1ns, no atomic overhead)
 * - Total: ~0.5-1ns (disabled) or ~1.5-3ns (enabled)
 *
 * Savings: ~5-10ns per stat update vs stat_inc (no atomic, no cache line bouncing)
 * Use this in hot paths where cpu_ctx is already available.
 */
static __always_inline void stat_inc_local(u64 *local_counter)
{
	/* TIER 0: Early exit if stats disabled or counter NULL (~0.5-1ns each) */
	if (likely(!no_stats && local_counter))
		/* TIER 0: Direct increment (no atomic, ~0.5-1ns) */
		(*local_counter)++;
}
#else

/* TELEMETRY DISABLED: Provide empty helpers so hot paths stay branch-free. */
static __always_inline void stat_inc(volatile u64 *counter)
{
	(void)counter;
}

static __always_inline void stat_add(volatile u64 *counter, u64 value)
{
	(void)counter;
	(void)value;
}

static __always_inline void stat_inc_local(u64 *local_counter)
{
	(void)local_counter;
}
#endif /* CONFIG_GAMER_ENABLE_EXTENDED_STATS */

#endif /* __GAMER_STATS_BPF_H */
