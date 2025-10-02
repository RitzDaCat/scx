/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Statistics Collection
 * Copyright (c) 2025 Andrea Righi <arighi@nvidia.com>
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
extern volatile u64 nr_mm_hint_hit;

/* Runtime accounting */
extern volatile u64 fg_runtime_ns_total;
extern volatile u64 total_runtime_ns_total;

/* Trigger counters */
extern volatile u64 nr_input_trig;
extern volatile u64 nr_frame_trig;

/* GPU thread affinity */
extern volatile u64 nr_gpu_phys_kept;

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

/*
 * Conditional stats increment (no-op if stats disabled)
 */
static __always_inline void stat_inc(volatile u64 *counter)
{
	if (!no_stats)
		__atomic_fetch_add(counter, 1, __ATOMIC_RELAXED);
}

/*
 * Conditional stats add (no-op if stats disabled)
 */
static __always_inline void stat_add(volatile u64 *counter, u64 value)
{
	if (!no_stats)
		__atomic_fetch_add(counter, value, __ATOMIC_RELAXED);
}

#endif /* __GAMER_STATS_BPF_H */
