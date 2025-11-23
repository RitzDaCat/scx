/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: BPF Hot-Path Profiling
 * Copyright (c) 2025 RitzDaCat
 *
 * Instrumentation for measuring scheduler latency in critical paths.
 * Compile-time disabled by default for maximum performance.
 *
 * To enable profiling for development/debugging:
 *   Add CFLAGS="-DENABLE_PROFILING" to build command
 */
#ifndef __GAMER_PROFILING_BPF_H
#define __GAMER_PROFILING_BPF_H

#include "config.bpf.h"
#include "stats.bpf.h"
#include "coalesce.bpf.h"  /* For should_profile() coalescing */

#ifdef ENABLE_PROFILING

/*
 * Hot-Path Latency Profiling (ENABLED)
 *
 * Measures execution time (in nanoseconds) for critical scheduler operations.
 * Accumulated counters allow calculating average latency: total_ns / call_count
 *
 * TIER 2: Profiling enabled (acceptable overhead for debugging)
 * - Timestamp calls: Tier 1 (~10-15ns each, 2 per measurement = ~20-30ns)
 * - Atomic operations: Tier 0 (~1-2ns each, 2 per measurement = ~2-4ns)
 * - Total overhead: ~22-34ns per measurement
 *
 * WARNING: Adds ~50-150ns overhead per scheduling decision when enabled.
 * Only enable for development/debugging.
 */

/*
 * Profiling Counters
 *
 * TIER 0: Volatile counters (fast atomic increments, ~1-2ns)
 * Used for latency measurement accumulation.
 */
extern volatile u64 prof_select_cpu_ns_total;
extern volatile u64 prof_select_cpu_calls;

extern volatile u64 prof_enqueue_ns_total;
extern volatile u64 prof_enqueue_calls;

extern volatile u64 prof_dispatch_ns_total;
extern volatile u64 prof_dispatch_calls;

extern volatile u64 prof_deadline_ns_total;
extern volatile u64 prof_deadline_calls;

extern volatile u64 prof_pick_idle_ns_total;
extern volatile u64 prof_pick_idle_calls;

/* MM hint profiling removed - was prof_mm_hint_ns_total, prof_mm_hint_calls */

/**
 * PROF_START - Start profiling measurement
 * @name: Profiling section name (e.g., select_cpu, enqueue)
 *
 * TIER 2: Profiling enabled
 * - Conditional check: Tier 0 (~0.5-1ns)
 * - Timestamp: Tier 1 (~10-15ns, scx_bpf_now)
 * - Total: ~10.5-16ns (when enabled)
 *
 * Use these to wrap critical code sections:
 *
 * PROF_START(select_cpu);
 * ... critical code ...
 * PROF_END(select_cpu);
 */
#define PROF_START(name) \
	u64 __prof_##name##_start = 0; \
	if (likely(!no_stats) && should_profile()) __prof_##name##_start = scx_bpf_now()

/**
 * PROF_END - End profiling measurement and record
 * @name: Profiling section name (must match PROF_START)
 *
 * TIER 2: Profiling enabled
 * - Conditional checks: Tier 0 (~0.5-1ns each)
 * - Timestamp: Tier 1 (~10-15ns, scx_bpf_now)
 * - Atomic operations: Tier 0 (~1-2ns each, 2 operations)
 * - Total: ~12-19ns (when enabled)
 */
#define PROF_END(name) \
	if (likely(!no_stats && __prof_##name##_start)) { \
		u64 __prof_elapsed = scx_bpf_now() - __prof_##name##_start; \
		__atomic_fetch_add(&prof_##name##_ns_total, __prof_elapsed, __ATOMIC_RELAXED); \
		__atomic_fetch_add(&prof_##name##_calls, 1, __ATOMIC_RELAXED); \
	}

/*
 * Percentile Tracking (Histogram)
 *
 * Track latency distribution to capture p50, p99, p99.9 metrics.
 * Uses logarithmic buckets to reduce memory overhead.
 *
 * TIER 0: Compile-time constant (zero runtime cost)
 */
#define HIST_BUCKETS 12

/*
 * Histogram buckets (log scale):
 * 0: <100ns, 1: 100-200ns, 2: 200-400ns, 3: 400-800ns,
 * 4: 800ns-1.6us, 5: 1.6-3.2us, 6: 3.2-6.4us, 7: 6.4-12.8us,
 * 8: 12.8-25.6us, 9: 25.6-51.2us, 10: 51.2-102.4us, 11: >102.4us
 *
 * TIER 0: Volatile arrays (fast atomic increments, ~1-2ns per bucket)
 */
extern volatile u64 hist_select_cpu[HIST_BUCKETS];
extern volatile u64 hist_enqueue[HIST_BUCKETS];
extern volatile u64 hist_dispatch[HIST_BUCKETS];

/**
 * ns_to_bucket - Convert nanoseconds to histogram bucket index
 * @ns: Latency in nanoseconds
 *
 * Uses log2 for logarithmic bucketing.
 *
 * TIER 0: Optimized bucket calculation
 * - Comparisons: Tier 0 (~0.5-1ns each)
 * - Bit shifts: Tier 0 (~0.5-1ns each)
 * - Loop iterations: Tier 0 (typically 0-3 iterations, unrolled by compiler)
 * - Total: ~2-8ns (depending on latency value)
 *
 * Returns: Bucket index (0-11)
 */
static __always_inline u32 ns_to_bucket(u64 ns)
{
	u32 bucket = 0;
	u64 threshold = 100;  /* Start at 100ns */

	/* TIER 0: Find bucket: each bucket doubles the threshold
	 * Loop typically executes 0-3 times, compiler may unroll */
	while (likely(ns >= threshold && bucket < HIST_BUCKETS - 1)) {
		bucket++;
		threshold <<= 1;  /* Double threshold (bit shift, ~0.5-1ns) */
	}

	return bucket;
}

/**
 * PROF_HIST - Record latency measurement in histogram
 * @name: Histogram name (e.g., select_cpu, enqueue)
 * @elapsed_ns: Elapsed time in nanoseconds
 *
 * TIER 2: Profiling enabled
 * - Conditional check: Tier 0 (~0.5-1ns)
 * - Bucket calculation: Tier 0 (~2-8ns, ns_to_bucket)
 * - Bounds check: Tier 0 (~0.5-1ns)
 * - Atomic operation: Tier 0 (~1-2ns)
 * - Total: ~4-12ns (when enabled)
 */
#define PROF_HIST(name, elapsed_ns) \
	if (likely(!no_stats)) { \
		u32 bucket = ns_to_bucket(elapsed_ns); \
		if (likely(bucket < HIST_BUCKETS)) \
			__atomic_fetch_add(&hist_##name[bucket], 1, __ATOMIC_RELAXED); \
	}

/**
 * PROF_START_HIST - Start profiling with histogram tracking
 * @name: Profiling section name
 *
 * TIER 2: Same as PROF_START (~10.5-16ns when enabled)
 */
#define PROF_START_HIST(name) \
	u64 __prof_##name##_start = 0; \
	if (likely(!no_stats) && should_profile()) __prof_##name##_start = scx_bpf_now()

/**
 * PROF_END_HIST - End profiling and record in histogram
 * @name: Profiling section name (must match PROF_START_HIST)
 *
 * TIER 2: Profiling enabled
 * - Conditional checks: Tier 0 (~0.5-1ns each)
 * - Timestamp: Tier 1 (~10-15ns, scx_bpf_now)
 * - Atomic operations: Tier 0 (~1-2ns each, 3 operations)
 * - Bucket calculation: Tier 0 (~2-8ns, ns_to_bucket)
 * - Total: ~14-27ns (when enabled)
 */
#define PROF_END_HIST(name) \
	if (likely(!no_stats && __prof_##name##_start)) { \
		u64 __prof_elapsed = scx_bpf_now() - __prof_##name##_start; \
		__atomic_fetch_add(&prof_##name##_ns_total, __prof_elapsed, __ATOMIC_RELAXED); \
		__atomic_fetch_add(&prof_##name##_calls, 1, __ATOMIC_RELAXED); \
		u32 bucket = ns_to_bucket(__prof_elapsed); \
		if (likely(bucket < HIST_BUCKETS)) \
			__atomic_fetch_add(&hist_##name[bucket], 1, __ATOMIC_RELAXED); \
	}

#else /* !ENABLE_PROFILING */

/*
 * Hot-Path Latency Profiling (DISABLED)
 *
 * All profiling macros are compile-time no-ops for maximum performance.
 * This is the default configuration for production/gaming use.
 *
 * TIER 0: Profiling disabled (zero runtime overhead)
 * - All macros expand to empty do-while(0) loops
 * - Compiler optimizes these away entirely (dead code elimination)
 * - Zero overhead in production builds
 *
 * To enable profiling: rebuild with CFLAGS="-DENABLE_PROFILING"
 */

/* TIER 0: No-op macros - compiler will optimize these away entirely */
#define PROF_START(name)		do {} while (0)
#define PROF_END(name)			do {} while (0)
#define PROF_HIST(name, elapsed_ns)	do {} while (0)
#define PROF_START_HIST(name)		do {} while (0)
#define PROF_END_HIST(name)		do {} while (0)

#endif /* ENABLE_PROFILING */

#endif /* __GAMER_PROFILING_BPF_H */
