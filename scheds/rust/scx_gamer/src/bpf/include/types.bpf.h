/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Type Definitions
 * Copyright (c) 2025 RitzDaCat
 *
 * All data structures, maps, and type definitions.
 * This file is AI-friendly: ~200 lines, data structures only.
 */
#ifndef __GAMER_TYPES_BPF_H
#define __GAMER_TYPES_BPF_H

#include "config.bpf.h"

/*
 * Per-Task Context (Cache-line optimized layout)
 *
 * CRITICAL: First 64 bytes (one cache line) contain ALL fields accessed in select_cpu fast paths.
 * This eliminates cache misses on the hottest code path (called on every wakeup).
 *
 * Layout reasoning:
 * - Bytes 0-7:   is_input_handler, is_gpu_submit, boost_shift (checked FIRST in select_cpu)
 * - Bytes 8-15:  preferred_physical_core (GPU fast path)
 * - Bytes 16-63: exec_runtime, last_run_at, wakeup_freq (hot-path scheduling data)
 * - Bytes 64+:   Cold data (migration tokens, page faults, classification samples)
 */
struct CACHE_ALIGNED task_ctx {
	/* CACHE LINE 1 (0-63 bytes): ULTRA-HOT fields accessed in every select_cpu call
	 * Grouping these eliminates ~80% of cache misses in select_cpu fast paths */

	/* Task role classification flags - FIRST byte for instant access */
	u8 is_input_handler:1;		/* Checked FIRST in select_cpu (line 1392) */
	u8 is_gpu_submit:1;		/* Checked SECOND in select_cpu (line 1414) */
	u8 is_compositor:1;		/* Window manager/compositor */
	u8 is_network:1;		/* Network/netcode thread */
	u8 is_system_audio:1;		/* System audio (PipeWire/ALSA) */
	u8 is_game_audio:1;		/* Game audio thread */
	u8 is_background:1;		/* Background/batch work */
	u8 reserved_flags:1;		/* Reserved for future use */

	/* Precomputed deadline boost shift (byte 1) - used in deadline calculation */
	u8 boost_shift;			/* 0=no boost, 7=10x boost for input handlers */

	/* Scheduler generation tracking (byte 2-3) - detects scheduler restarts */
	u16 scheduler_gen;		/* Generation ID when thread was classified */
	s32 preferred_physical_core;	/* GPU thread cached core (-1=unset) */
	u32 preferred_core_hits;	/* Successful preferred-core placements */
	u64 preferred_core_last_hit;	/* Timestamp of last preferred-core success */

	/* Hot-path scheduling data (bytes 8-63) */
	u64 exec_runtime;		/* Accumulated runtime since last sleep */
	u64 last_run_at;		/* Timestamp when started running */
	u64 wakeup_freq;		/* EMA of inter-wakeup frequency */
	u64 last_woke_at;		/* Last wake timestamp */
	u64 exec_avg;			/* EMA of exec_runtime per wake cycle */
	u32 chain_boost;		/* Sync-wake chain boost depth */

	/* CACHE LINE 2 (64+ bytes): Cold data accessed less frequently */

	/* Migration limiter state (scaled token bucket) */
	u64 mig_tokens;			/* Scaled by MIG_TOKEN_SCALE */
	u64 mig_last_refill;		/* Last token refill timestamp */

	/* MM hint rate limiting */
	u64 mm_hint_last_update;	/* Last MM hint update time */

	/* Thread classification metrics */
	u16 low_cpu_samples;		/* Consecutive wakes with <100μs exec */
	u16 high_cpu_samples;		/* Consecutive wakes with >5ms exec */
	u32 _pad3;			/* Alignment */

	/* Cache thrashing detection */
	u64 last_pgfault_total;		/* Last sampled maj_flt + min_flt */
	u64 pgfault_rate;		/* Page faults per wake (EMA) */
};

/*
 * Per-CPU Context
 */
struct CACHE_ALIGNED cpu_ctx {
	/* Hot-path fields */
	u64 vtime_now;			/* Cached system vruntime reference */
	u64 interactive_avg;		/* Per-CPU interactivity EMA */

	/* CPU frequency control */
	u64 last_update;		/* Last cpufreq update timestamp */
	u64 perf_lvl;			/* Current performance level */

	/* Miscellaneous */
	u64 shared_dsq_id;		/* Assigned shared DSQ ID */
	u32 last_cpu_idx;		/* For idle scan rotation */

	/* Per-CPU stat accumulators (no atomics needed!)
	 * These are aggregated into global counters periodically by the timer.
	 * Eliminates expensive atomic operations in hot paths (30-50ns savings). */
	u64 local_nr_idle_cpu_pick;
	u64 local_nr_mm_hint_hit;
	u64 local_nr_sync_wake_fast;
	u64 local_nr_migrations;
	u64 local_nr_mig_blocked;

	/* PERF: Additional hot-path counters migrated from atomics */
	u64 local_nr_direct_dispatches;
	u64 local_rr_enq;
	u64 local_edf_enq;
	u64 local_nr_shared_dispatches;
};

/*
 * BPF Maps
 */

/* Task storage */
struct {
	__uint(type, BPF_MAP_TYPE_TASK_STORAGE);
	__uint(map_flags, BPF_F_NO_PREALLOC);
	__type(key, int);
	__type(value, struct task_ctx);
} task_ctx_stor SEC(".maps");

/* Per-CPU storage */
struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__type(key, u32);
	__type(value, struct cpu_ctx);
	__uint(max_entries, 1);
} cpu_ctx_stor SEC(".maps");

/* Per-MM recent CPU hint (LRU cache) */
struct {
	__uint(type, BPF_MAP_TYPE_LRU_HASH);
	__type(key, u64);	/* MM pointer */
	__type(value, u32);	/* Last CPU ID */
	__uint(max_entries, 8192);	/* Configurable via userspace */
} mm_last_cpu SEC(".maps");

/* Primary CPU mask */
private(GAMER) struct bpf_cpumask __kptr *primary_cpumask;

/*
 * Context Lookup Helpers
 */
static inline struct task_ctx *try_lookup_task_ctx(const struct task_struct *p)
{
	return bpf_task_storage_get(&task_ctx_stor, (struct task_struct *)p, 0, 0);
}

static inline struct cpu_ctx *try_lookup_cpu_ctx(s32 cpu)
{
	const u32 idx = 0;
	return bpf_map_lookup_percpu_elem(&cpu_ctx_stor, &idx, cpu);
}

#endif /* __GAMER_TYPES_BPF_H */
