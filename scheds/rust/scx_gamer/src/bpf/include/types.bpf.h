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
 * Per-Task Context (Hot-path optimized layout)
 */
struct CACHE_ALIGNED task_ctx {
	/* Hot-path fields first: frequently read/updated */
	u64 exec_runtime;		/* Accumulated runtime since last sleep */
	u64 last_run_at;		/* Timestamp when started running */
	u64 wakeup_freq;		/* EMA of inter-wakeup frequency */
	u64 last_woke_at;		/* Last wake timestamp */

	/* Migration limiter state (scaled token bucket) */
	u64 mig_tokens;			/* Scaled by MIG_TOKEN_SCALE */
	u64 mig_last_refill;		/* Last token refill timestamp */

	/* Scheduling hints */
	u32 chain_boost;		/* Sync-wake chain boost depth */

	/* MM hint rate limiting */
	u64 mm_hint_last_update;	/* Last MM hint update time */

	/* Thread classification metrics */
	u64 exec_avg;			/* EMA of exec_runtime per wake cycle */
	u16 low_cpu_samples;		/* Consecutive wakes with <100μs exec */
	u16 high_cpu_samples;		/* Consecutive wakes with >5ms exec */

	/* Cache thrashing detection */
	u64 last_pgfault_total;		/* Last sampled maj_flt + min_flt */
	u64 pgfault_rate;		/* Page faults per wake (EMA) */

	/* Task role classification flags (bitfield for cache efficiency) */
	u8 is_gpu_submit:1;		/* GPU command submission thread */
	u8 is_background:1;		/* Background/batch work */
	u8 is_compositor:1;		/* Window manager/compositor */
	u8 is_network:1;		/* Network/netcode thread */
	u8 is_system_audio:1;		/* System audio (PipeWire/ALSA) */
	u8 is_game_audio:1;		/* Game audio thread */
	u8 is_input_handler:1;		/* Input processing - HIGHEST priority */
	u8 reserved_flags:1;		/* Reserved for future use */
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
