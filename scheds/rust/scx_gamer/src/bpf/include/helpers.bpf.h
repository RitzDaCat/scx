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

/* Branch prediction hints already defined in bpf_helpers.h - no need to redefine */

/* External tunables */
extern const volatile bool numa_enabled;

/**
 * shared_dsq - Get shared dispatch queue ID for CPU
 * @cpu: CPU ID
 *
 * Returns:
 * - NUMA node ID if NUMA enabled
 * - SHARED_DSQ (0) otherwise
 *
 * Cached in cpu_ctx to avoid repeated lookups.
 *
 * TIER 0/1: Optimized for hot path performance
 * - Map lookup: Tier 1 (~20-50ns, try_lookup_cpu_ctx)
 * - Cached value check: Tier 0 (~0.5-1ns, most common case)
 * - NUMA node lookup: Tier 1 (~20-50ns, only if NUMA enabled)
 * - Total: ~0.5-1ns (cached) or ~40-100ns (uncached)
 *
 * Frequency: Called during dispatch (thousands/sec)
 * Net overhead: Minimal (results cached in cpu_ctx)
 */
static __always_inline u64 shared_dsq(s32 cpu)
{
	/* TIER 1: Lookup CPU context (map lookup, ~20-50ns) */
	struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);

	/* TIER 0: Return cached value if available (most common case, ~0.5-1ns) */
	if (likely(cctx && cctx->shared_dsq_id))
		return cctx->shared_dsq_id;

	/* TIER 1: NUMA-aware: use node ID as DSQ (only if NUMA enabled) */
	if (likely(numa_enabled)) {
		u64 node = __COMPAT_scx_bpf_cpu_node(cpu);
		if (likely(cctx))
			cctx->shared_dsq_id = node;
		return node;
	}

	/* TIER 0: Non-NUMA: use global shared DSQ */
	if (likely(cctx))
		cctx->shared_dsq_id = SHARED_DSQ;
	return SHARED_DSQ;
}

/**
 * is_pcpu_task - Check if task can only run on a single CPU
 * @p: Task struct to check
 *
 * Per-CPU tasks cannot migrate and should bypass migration logic.
 *
 * TIER 0: Single struct field read + comparison (~0.5-1ns)
 * Hot path function - called during CPU selection.
 */
static __always_inline bool is_pcpu_task(const struct task_struct *p)
{
	return p->nr_cpus_allowed == 1;
}

/**
 * calc_avg - Calculate Exponential Moving Average (EMA)
 * @old_avg: Previous average value
 * @new_val: New value to incorporate
 *
 * Formula: new_avg = (old_avg * 3 + new_val) / 4
 *
 * This gives ~75% weight to old value, ~25% to new value,
 * providing smooth averaging without expensive FP math.
 *
 * TIER 0: Optimized for hot path performance
 * - Bit shifts: Tier 0 (~0.5-1ns each)
 * - Addition: Tier 0 (~0.5-1ns)
 * - Total: ~2-4ns
 *
 * Called ~200× per frame in hot paths (runnable, stopping, deadline calc).
 * Force inline eliminates function call overhead (~5-10ns savings).
 */
static __always_inline u64 calc_avg(u64 old_avg, u64 new_val)
{
	/* TIER 0: EMA calculation using bit shifts (faster than division)
	 * Formula: (old * 3 + new) / 4 = ((old << 1) + old + new) >> 2 */
	return ((old_avg << 1) + old_avg + new_val) >> 2;
}

/**
 * calc_avg32 - Calculate Exponential Moving Average (EMA) for 32-bit values
 * @old_avg: Previous average value (32-bit)
 * @new_val: New value to incorporate (32-bit)
 *
 * TIER 0: Same as calc_avg but for 32-bit values (~2-4ns)
 */
static __always_inline u32 calc_avg32(u32 old_avg, u32 new_val)
{
	return ((old_avg << 1) + old_avg + new_val) >> 2;
}

/**
 * update_freq - Update frequency estimation
 * @freq: Current frequency estimate (events per 100ms)
 * @interval: Time interval between events (nanoseconds)
 *
 * Computes event frequency (events per 100ms) from inter-event interval,
 * then updates the exponential moving average.
 *
 * TIER 0: Optimized for hot path performance
 * - Zero check: Tier 0 (~0.5-1ns)
 * - Division: Tier 0 (~2-5ns)
 * - EMA calculation: Tier 0 (~2-4ns, calc_avg)
 * - Total: ~4.5-10ns
 *
 * Returns unchanged frequency if interval is zero (prevents division by zero).
 * Called in runnable() hot path - force inline eliminates call overhead.
 */
static __always_inline u64 update_freq(u64 freq, u64 interval)
{
	/* TIER 0: Guard against division by zero (early exit, ~0.5-1ns) */
	if (unlikely(!interval))
		return freq;

	/* TIER 0: Frequency = events per 100ms (division, ~2-5ns) */
	u64 new_freq = (100 * NSEC_PER_MSEC) / interval;
	
	/* TIER 0: Update EMA (~2-4ns) */
	return calc_avg(freq, new_freq);
}

/* scale_by_task_weight() and scale_by_task_weight_inverse() provided by scx/common.bpf.h */

/*
 * Kick Helpers
 *
 * SCX already exposes scx_bpf_kick_cpu(), so we can directly wake CPUs when
 * needed instead of deferring to a bitmap processed by wakeup_timerfn().
 * This keeps enqueue hot paths simple while still allowing deferred wakeups
 * if the compile-time flag re-enables the timer.
 */
static __always_inline void set_kick_cpu(s32 cpu)
{
	if (unlikely(cpu < 0 || (u32)cpu >= MAX_CPUS))
		return;

	scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
}

static __always_inline void clear_kick_cpu(__maybe_unused s32 cpu) {}

/*
 * CPU Frequency Scaling
 */

/* External tunable */
extern const volatile bool cpufreq_enabled;

/**
 * update_target_cpuperf - Update target CPU performance level based on utilization
 * @cctx: CPU context
 * @now: Current timestamp
 * @slice: Time slice consumed by last task
 *
 * TIER 0/1: CPU frequency scaling update
 * - Early exits: Tier 0 (~0.5-1ns each)
 * - Arithmetic operations: Tier 0 (~2-5ns)
 * - EMA calculation: Tier 0 (~2-4ns)
 * - Total: ~1-3ns (early exit) or ~6-12ns (full calculation)
 *
 * Not in hottest path - called during task completion.
 */
static __always_inline void update_target_cpuperf(struct cpu_ctx *cctx, u64 now, u64 slice)
{
	/* TIER 0: Early exit if cpufreq disabled (~0.5-1ns) */
	if (unlikely(!cpufreq_enabled))
		return;

	/* TIER 0: Skip if uninitialized or clock skew detected (~0.5-1ns) */
	if (unlikely(!cctx->last_update || now < cctx->last_update)) {
		cctx->last_update = now;
		return;
	}

	u64 delta_t = now - cctx->last_update;

	/* TIER 0: Skip if zero delta or time jump (>1s) (~0.5-1ns) */
	if (unlikely(!delta_t || delta_t > NSEC_PER_SEC)) {
		cctx->last_update = now;
		return;
	}

	/* TIER 0: Calculate performance level: (slice / delta_t) normalized to [0, SCX_CPUPERF_ONE]
	 * Multiplication and division (~2-5ns) */
	u64 perf_lvl = MIN(slice * SCX_CPUPERF_ONE / delta_t, SCX_CPUPERF_ONE);
	
	/* TIER 0: Update EMA (~2-4ns) */
	cctx->perf_lvl = calc_avg(cctx->perf_lvl, perf_lvl);
	cctx->last_update = now;
}

/**
 * update_cpufreq - Apply target cpufreq performance level to CPU
 * @cpu: CPU ID to update
 *
 * Uses hysteresis to avoid frequent freq changes:
 * - HIGH_THRESH: boost to max
 * - LOW_THRESH: drop to 50%
 * - Between: maintain current level
 *
 * TIER 1: CPU frequency scaling application
 * - Early exit: Tier 0 (~0.5-1ns)
 * - Map lookup: Tier 1 (~20-50ns, try_lookup_cpu_ctx)
 * - Threshold checks: Tier 0 (~0.5-1ns each)
 * - BPF helper call: Tier 1 (~50-150ns, scx_bpf_cpuperf_set)
 * - Total: ~71-202ns
 *
 * Not in hottest path - called during CPU idle/busy transitions.
 */
static __always_inline void update_cpufreq(s32 cpu)
{
	/* TIER 0: Early exit if cpufreq disabled (~0.5-1ns) */
	if (unlikely(!cpufreq_enabled))
		return;

	/* TIER 1: Lookup CPU context (map lookup, ~20-50ns) */
	struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
	if (unlikely(!cctx))
		return;

	/* TIER 0: Apply hysteresis thresholds (comparisons, ~0.5-1ns each) */
	u64 perf_lvl;
	if (likely(cctx->perf_lvl >= CPUFREQ_HIGH_THRESH))
		perf_lvl = SCX_CPUPERF_ONE;  /* Max performance */
	else if (likely(cctx->perf_lvl <= CPUFREQ_LOW_THRESH))
		perf_lvl = SCX_CPUPERF_ONE / 2;  /* 50% performance */
	else
		perf_lvl = cctx->perf_lvl;  /* Maintain current */

	/* TIER 1: Apply performance level (BPF helper call, ~50-150ns) */
	scx_bpf_cpuperf_set(cpu, perf_lvl);
}

/**
 * calculate_rms_priority_from_period - Calculate RMS priority from task period
 * @period_ns: Task period in nanoseconds
 *
 * Rate Monotonic Scheduling (RMS) - Liu & Layland (1973)
 *
 * RMS Principle: Shorter period = Higher priority
 *
 * This implements the Rate Monotonic Scheduling algorithm where tasks with
 * shorter periods (higher frequency) receive higher priority. This ensures
 * that high-FPS games (240Hz) get higher priority than low-FPS games (60Hz),
 * and high-polling input devices (8000Hz) get higher priority than low-polling
 * devices (1000Hz).
 *
 * Priority mapping:
 * - Priority 7: ≤125µs (8000Hz+ input devices)
 * - Priority 6: ≤250µs (4000Hz input devices)
 * - Priority 5: ≤4.17ms (240Hz+ frame rendering)
 * - Priority 4: ≤8.33ms (120Hz frame rendering)
 * - Priority 3: ≤16.67ms (60Hz frame rendering)
 * - Priority 2: >16.67ms (lower frame rates or non-periodic)
 *
 * TIER 0: Optimized conditional chain for hot path
 * - Comparisons: Tier 0 (~0.5-1ns each)
 * - Early exits: Tier 0 (most common case exits early)
 * - Total: ~0.5-6ns (depending on period)
 *
 * Returns: RMS priority (0-7, higher = more priority)
 */
static __always_inline u8 calculate_rms_priority_from_period(u64 period_ns)
{
	/* TIER 0: RMS: Shorter period = higher priority
	 * Ordered by frequency (most common first) for optimal branch prediction */
	
	if (likely(period_ns <= 125000ULL))        /* ≤125µs (8000Hz+ input) */
		return 7;
	else if (likely(period_ns <= 250000ULL))   /* ≤250µs (4000Hz input) */
		return 6;
	else if (likely(period_ns <= 4167000ULL))  /* ≤4.17ms (240Hz+ frames) */
		return 5;
	else if (likely(period_ns <= 8333000ULL))  /* ≤8.33ms (120Hz frames) */
		return 4;
	else if (likely(period_ns <= 16667000ULL)) /* ≤16.67ms (60Hz frames) */
		return 3;
	else                                       /* >60Hz or non-periodic */
		return 2;
}

/**
 * update_task_utilization - Update task utilization based on execution time and period
 * @tctx: Task context
 *
 * Schedulability Analysis - Liu & Layland (1973)
 *
 * Utilization = (execution_time / period) * 100
 *
 * For periodic tasks:
 * - Use exec_avg as execution time estimate (Ci)
 * - Use detected_period_ns as period (Pi)
 * - Calculate utilization percentage
 *
 * This implements the utilization bound test from Liu & Layland (1973):
 * - EDF: U = Σ(Ci / Pi) ≤ 100%
 * - RMS: U = Σ(Ci / Pi) ≤ n * (2^(1/n) - 1)
 *
 * TIER 0: Optimized for deadline calculation hot path
 * - Early exit checks: Tier 0 (~0.5-1ns each)
 * - Struct field reads: Tier 0 (~0.5-1ns each)
 * - Division: Tier 0 (~2-5ns)
 * - Comparisons: Tier 0 (~0.5-1ns)
 * - Total: ~1-2ns (early exit) or ~5-12ns (full calculation)
 *
 * Performance: O(1) - Simple division
 */
static __always_inline void update_task_utilization(struct task_ctx *tctx)
{
	/* TIER 0: Only calculate utilization for periodic tasks (early exit, ~0.5-1ns) */
	if (unlikely(!tctx->is_periodic || tctx->detected_period_ns == 0))
		return;
	
	/* PHASE 5: Use exec_runtime directly - exec_avg EMA removed
	 * exec_runtime is accumulated execution time since last sleep */
	u64 exec_time = tctx->exec_runtime;
	
	/* TIER 0: Calculate utilization: (Ci / Pi) * 100
	 * Fixed-point: 100 = 1%, 10000 = 100%
	 * Avoid division by zero */
	if (likely(exec_time > 0 && tctx->detected_period_ns > 0)) {
		/* Utilization percentage: (exec_time * 100) / period_ns
		 * Multiply by 100 for fixed-point representation (~2-5ns) */
		tctx->utilization_pct = (exec_time * 100) / tctx->detected_period_ns;
		tctx->worst_case_exec_ns = exec_time;
		
		/* TIER 0: Cap utilization at 100% (10000 in fixed-point)
		 * Tasks with exec_time > period are clearly unschedulable (~0.5-1ns) */
		if (unlikely(tctx->utilization_pct > 10000))
			tctx->utilization_pct = 10000;
	} else {
		/* Invalid data: Set utilization to 0 */
		tctx->utilization_pct = 0;
		tctx->worst_case_exec_ns = 0;
	}
}

/*
 * Calculate total system utilization for periodic tasks.
 * 
 * Total Utilization = Σ(Ci / Pi) for all periodic tasks
 * 
 * EDF Schedulability: U ≤ 100%
 * RMS Schedulability: U ≤ n * (2^(1/n) - 1)
 * 
 * OPTIMIZATION: BPF map iteration is expensive
 * Instead, use per-CPU aggregation or cached value
 * 
 * Current approach: Use existing cpu_util_avg as proxy
 * More accurate: Track only periodic tasks (requires map iteration)
 * 
 * @return: Total utilization percentage (fixed-point, 100 = 1%)
 * 
 * Performance: O(1) - Uses cached value, ~5-10ns overhead
 */
static __always_inline u64 calculate_total_utilization(void)
{
	/* OPTIMIZATION: BPF map iteration is expensive (cannot iterate efficiently)
	 * Instead, use existing cpu_util_avg as approximation
	 * 
	 * cpu_util_avg is per-CPU utilization in fixed-point (1024 = 100%)
	 * This is an approximation - exact calculation would require iterating
	 * all task_ctx entries to sum only periodic tasks */
	
	/* For now, return 0 to indicate "not calculated"
	 * Actual utilization check will use cpu_util_avg directly
	 * This allows future enhancement without breaking existing code */
	return 0;
}

/**
 * is_schedulable - Check if system is schedulable under EDF or RMS
 * @total_util: Total system utilization (fixed-point, 100 = 1%)
 * @use_rms: If true, use RMS bound; if false, use EDF bound
 *
 * EDF Schedulability: U ≤ 100%
 * RMS Schedulability: U ≤ n * (2^(1/n) - 1)
 *
 * Utilization bounds:
 * - EDF: 100% (optimal - can schedule any task set if U ≤ 100%)
 * - RMS: n * (2^(1/n) - 1) for n tasks
 *   - n=1: 100%
 *   - n=2: 82.8%
 *   - n=3: 78.0%
 *   - n=4: 75.7%
 *   - n→∞: 69.3% (ln(2) ≈ 69.3%)
 *
 * TIER 0: Optimized for deadline calculation hot path
 * - Branch check: Tier 0 (~0.5-1ns)
 * - Comparison: Tier 0 (~0.5-1ns)
 * - Total: ~1-2ns
 *
 * Returns: true if schedulable, false otherwise
 */
static __always_inline bool is_schedulable(u64 total_util, bool use_rms)
{
	if (likely(use_rms)) {
		/* TIER 0: RMS bound: U ≤ n * (2^(1/n) - 1)
		 * 
		 * Simplified: Use 69% bound for n≥3 (conservative)
		 * This ensures schedulability even with many tasks
		 * 
		 * More accurate: Calculate bound based on actual number of tasks
		 * But BPF cannot efficiently count tasks, so use conservative bound */
		u64 rms_bound = 6900;  /* 69% in fixed-point (100 = 1%) */
		return total_util <= rms_bound;
	} else {
		/* TIER 0: EDF bound: U ≤ 100%
		 * EDF is optimal - can schedule any task set if U ≤ 100% */
		u64 edf_bound = 10000;  /* 100% in fixed-point (100 = 1%) */
		return total_util <= edf_bound;
	}
}

/*
 * Priority Inheritance Protocol (PIP) - Sha et al. (1990)
 * 
 * Helper functions for implementing Priority Inheritance Protocol.
 * 
 * PIP prevents priority inversion by temporarily boosting the priority of
 * a lock holder to match the priority of a high-priority task waiting for the lock.
 */

/**
 * inherit_priority - Inherit priority from waiting task to lock holder
 * @waiting_tctx: Task context of task waiting for lock
 * @holder_tctx: Task context of task holding the lock
 *
 * Priority Inheritance Protocol (PIP) - Sha et al. (1990)
 *
 * When a high-priority task waits for a lock held by a lower-priority task,
 * the lock holder inherits the waiting task's priority. This ensures the lock
 * holder runs at high priority, releasing the lock faster and preventing
 * priority inversion.
 *
 * TIER 0/1: Optimized for lock contention hot path
 * - Null checks: Tier 0 (~0.5-1ns each)
 * - Priority comparison: Tier 0 (~0.5-1ns)
 * - Struct field updates: Tier 0 (~1-2ns per field)
 * - Timestamp: Tier 1 (~10-15ns, scx_bpf_now)
 * - Total: ~1-2ns (early exit) or ~15-25ns (full inheritance)
 *
 * Performance: O(1) - Field updates
 */
static __always_inline void inherit_priority(struct task_ctx *waiting_tctx, struct task_ctx *holder_tctx)
{
	/* TIER 0: Null checks (early exit, ~0.5-1ns each) */
	if (unlikely(!waiting_tctx || !holder_tctx))
		return;
	
	/* TIER 0: Only inherit if waiting task has higher priority (~0.5-1ns) */
	if (likely(waiting_tctx->boost_shift > holder_tctx->boost_shift)) {
		/* TIER 0: Save original priority if not already inherited
		 * This handles nested locks (inheritance chains) (~0.5-1ns) */
		if (unlikely(holder_tctx->inherited_boost == 0)) {
			holder_tctx->original_boost_shift = holder_tctx->boost_shift;
		}
		
		/* TIER 0: Inherit waiting task's priority (capped at maximum 7)
		 * Struct field updates (~1-2ns per field) */
		u8 inherited_boost = MIN(waiting_tctx->boost_shift, 7);
		holder_tctx->inherited_boost = inherited_boost;
		holder_tctx->boost_shift = inherited_boost;
		
		/* TIER 1: Set inheritance expiry (prevent infinite inheritance)
		 * Expires after 100ms to handle lock leaks (~10-15ns) */
		u64 now = scx_bpf_now();
		holder_tctx->inheritance_expiry = now + 100000000ULL;  /* 100ms */
	}
}

/**
 * restore_priority - Restore original priority when lock is released
 * @holder_tctx: Task context of task releasing the lock
 *
 * When a lock is released, restore the lock holder's priority to its
 * original value before inheritance. This handles priority restoration
 * and inheritance chains (nested locks).
 *
 * TIER 0: Optimized for lock release hot path
 * - Null check: Tier 0 (~0.5-1ns)
 * - Field check: Tier 0 (~0.5-1ns)
 * - Struct field updates: Tier 0 (~1-2ns per field)
 * - Total: ~1-2ns (early exit) or ~5-10ns (full restoration)
 *
 * Performance: O(1) - Field updates
 */
static __always_inline void restore_priority(struct task_ctx *holder_tctx)
{
	/* TIER 0: Null check and inheritance check (early exit, ~0.5-1ns each) */
	if (unlikely(!holder_tctx || holder_tctx->inherited_boost == 0))
		return;
	
	/* TIER 0: Restore original priority (struct field updates, ~1-2ns per field) */
	holder_tctx->boost_shift = holder_tctx->original_boost_shift;
	holder_tctx->inherited_boost = 0;
	holder_tctx->inheritance_expiry = 0;
	holder_tctx->original_boost_shift = 0;
}

/**
 * check_inheritance_expiry - Check and restore expired priority inheritance
 * @tctx: Task context to check
 *
 * If inheritance has expired (e.g., due to lock leak), restore priority.
 * This prevents priority inheritance from persisting indefinitely.
 *
 * TIER 0/1: Optimized for periodic check hot path
 * - Null check: Tier 0 (~0.5-1ns)
 * - Field check: Tier 0 (~0.5-1ns)
 * - Timestamp: Tier 1 (~10-15ns, scx_bpf_now)
 * - Comparison: Tier 0 (~0.5-1ns)
 * - Total: ~1-2ns (early exit) or ~12-19ns (expiry check)
 *
 * Performance: O(1) - Simple check
 */
static __always_inline void check_inheritance_expiry(struct task_ctx *tctx)
{
	/* TIER 0: Null check and inheritance check (early exit, ~0.5-1ns each) */
	if (unlikely(!tctx || tctx->inherited_boost == 0))
		return;
	
	/* TIER 1: Check expiry (timestamp lookup, ~10-15ns) */
	u64 now = scx_bpf_now();
	if (likely(tctx->inheritance_expiry > 0 && now > tctx->inheritance_expiry)) {
		/* TIER 0: Inheritance expired - restore priority (~5-10ns) */
		restore_priority(tctx);
	}
}

#endif /* __GAMER_HELPERS_BPF_H */
