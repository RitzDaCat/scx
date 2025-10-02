/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Utility Helper Functions
 * Copyright (c) 2025 RitzDaCat
 *
 * Common utility functions used throughout the scheduler.
 * This file is AI-friendly: ~150 lines, single responsibility.
 */
#ifndef __GAMER_HELPERS_BPF_H
#define __GAMER_HELPERS_BPF_H

#include "config.bpf.h"
#include "types.bpf.h"

/* External tunables */
extern const volatile bool numa_enabled;

/*
 * Get shared dispatch queue ID for CPU
 *
 * Returns:
 * - NUMA node ID if NUMA enabled
 * - SHARED_DSQ (0) otherwise
 *
 * Cached in cpu_ctx to avoid repeated lookups.
 */
static inline u64 shared_dsq(s32 cpu)
{
	struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
	u64 node;

	/* Return cached value if available */
	if (cctx && cctx->shared_dsq_id)
		return cctx->shared_dsq_id;

	/* NUMA-aware: use node ID as DSQ */
	if (numa_enabled) {
		node = __COMPAT_scx_bpf_cpu_node(cpu);
		if (cctx)
			cctx->shared_dsq_id = node;
		return node;
	}

	/* Non-NUMA: use global shared DSQ */
	if (cctx)
		cctx->shared_dsq_id = SHARED_DSQ;
	return SHARED_DSQ;
}

/*
 * Check if task can only run on a single CPU
 *
 * Per-CPU tasks cannot migrate and should bypass migration logic.
 */
static inline bool is_pcpu_task(const struct task_struct *p)
{
	return p->nr_cpus_allowed == 1;
}

/*
 * Calculate Exponential Moving Average (EMA)
 *
 * Formula: new_avg = (old_avg * 3 + new_val) / 4
 *
 * This gives ~75% weight to old value, ~25% to new value,
 * providing smooth averaging without expensive FP math.
 */
static inline u64 calc_avg(u64 old_avg, u64 new_val)
{
	return ((old_avg << 1) + old_avg + new_val) >> 2;
}

static inline u32 calc_avg32(u32 old_avg, u32 new_val)
{
	return ((old_avg << 1) + old_avg + new_val) >> 2;
}

/*
 * Scale value by task weight
 *
 * Used for time slice scaling: higher nice = lower weight = shorter slice.
 */
static u64 scale_by_task_weight(const struct task_struct *p, u64 value)
{
	return (value * p->scx.weight) / 100;
}

/*
 * Scale value inversely by task weight
 *
 * Used for deadline/vtime scaling: higher nice = lower weight = later deadline.
 */
static u64 scale_by_task_weight_inverse(const struct task_struct *p, u64 value)
{
	return (value * 100) / p->scx.weight;
}

/*
 * Kick Bitmap Helpers
 *
 * Used to track which CPUs need to be kicked (interrupted)
 * to check for higher-priority work.
 */

/* Global kick bitmap */
extern volatile u64 kick_mask[KICK_WORDS];

/*
 * Set kick bit for CPU
 */
static __always_inline void set_kick_cpu(s32 cpu)
{
	u32 w, bit;

	if (cpu < 0 || (u32)cpu >= MAX_CPUS)
		return;

	w = (u32)cpu >> 6;  /* Word index (cpu / 64) */
	if (w >= KICK_WORDS)
		return;

	bit = 1ULL << (cpu & 63);  /* Bit within word */
	__atomic_fetch_or(&kick_mask[w], bit, __ATOMIC_RELAXED);
}

/*
 * Clear kick bit for CPU
 */
static __always_inline void clear_kick_cpu(s32 cpu)
{
	u32 w;
	u64 bit;

	if (cpu < 0 || (u32)cpu >= MAX_CPUS)
		return;

	w = (u32)cpu >> 6;
	if (w >= KICK_WORDS)
		return;

	bit = 1ULL << (cpu & 63);
	__atomic_fetch_and(&kick_mask[w], ~bit, __ATOMIC_RELAXED);
}

/*
 * CPU Frequency Scaling
 */

/* External tunable */
extern const volatile bool cpufreq_enabled;

/*
 * Update target CPU performance level based on utilization
 *
 * @cctx: CPU context
 * @now: Current timestamp
 * @slice: Time slice consumed by last task
 */
static void update_target_cpuperf(struct cpu_ctx *cctx, u64 now, u64 slice)
{
	u64 delta_t, perf_lvl;

	if (!cpufreq_enabled)
		return;

	/* Skip if uninitialized or clock skew detected */
	if (!cctx->last_update || now < cctx->last_update) {
		cctx->last_update = now;
		return;
	}

	delta_t = now - cctx->last_update;

	/* Skip if zero delta or time jump (>1s) */
	if (!delta_t || delta_t > NSEC_PER_SEC) {
		cctx->last_update = now;
		return;
	}

	/* Calculate performance level: (slice / delta_t) normalized to [0, SCX_CPUPERF_ONE] */
	perf_lvl = MIN(slice * SCX_CPUPERF_ONE / delta_t, SCX_CPUPERF_ONE);
	cctx->perf_lvl = calc_avg(cctx->perf_lvl, perf_lvl);
	cctx->last_update = now;
}

/*
 * Apply target cpufreq performance level to CPU
 *
 * Uses hysteresis to avoid frequent freq changes:
 * - HIGH_THRESH: boost to max
 * - LOW_THRESH: drop to 50%
 * - Between: maintain current level
 */
static void update_cpufreq(s32 cpu)
{
	struct cpu_ctx *cctx;
	u64 perf_lvl;

	if (!cpufreq_enabled)
		return;

	cctx = try_lookup_cpu_ctx(cpu);
	if (!cctx)
		return;

	/* Apply hysteresis thresholds */
	if (cctx->perf_lvl >= CPUFREQ_HIGH_THRESH)
		perf_lvl = SCX_CPUPERF_ONE;  /* Max performance */
	else if (cctx->perf_lvl <= CPUFREQ_LOW_THRESH)
		perf_lvl = SCX_CPUPERF_ONE / 2;  /* 50% performance */
	else
		perf_lvl = cctx->perf_lvl;  /* Maintain current */

	scx_bpf_cpuperf_set(cpu, perf_lvl);
}

#endif /* __GAMER_HELPERS_BPF_H */
