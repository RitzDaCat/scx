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

#ifdef ENABLE_PROFILING

/*
 * Hot-Path Latency Profiling (ENABLED)
 *
 * Measures execution time (in nanoseconds) for critical scheduler operations.
 * Accumulated counters allow calculating average latency: total_ns / call_count
 *
 * WARNING: Adds ~50-150ns overhead per scheduling decision.
 * Only enable for development/debugging.
 */

/* select_cpu profiling */
extern volatile u64 prof_select_cpu_ns_total;
extern volatile u64 prof_select_cpu_calls;

/* enqueue profiling */
extern volatile u64 prof_enqueue_ns_total;
extern volatile u64 prof_enqueue_calls;

/* dispatch profiling */
extern volatile u64 prof_dispatch_ns_total;
extern volatile u64 prof_dispatch_calls;

/* deadline calculation profiling */
extern volatile u64 prof_deadline_ns_total;
extern volatile u64 prof_deadline_calls;

/* CPU selection sub-path profiling */
extern volatile u64 prof_pick_idle_ns_total;
extern volatile u64 prof_pick_idle_calls;

/* mm_hint lookup profiling */
extern volatile u64 prof_mm_hint_ns_total;
extern volatile u64 prof_mm_hint_calls;

/*
 * Profiling Macros
 *
 * Use these to wrap critical code sections:
 *
 * PROF_START(select_cpu);
 * ... critical code ...
 * PROF_END(select_cpu);
 */

#define PROF_START(name) \
	u64 __prof_##name##_start = 0; \
	if (!no_stats) __prof_##name##_start = scx_bpf_now()

#define PROF_END(name) \
	if (!no_stats && __prof_##name##_start) { \
		u64 __prof_elapsed = scx_bpf_now() - __prof_##name##_start; \
		__atomic_fetch_add(&prof_##name##_ns_total, __prof_elapsed, __ATOMIC_RELAXED); \
		__atomic_fetch_add(&prof_##name##_calls, 1, __ATOMIC_RELAXED); \
	}

/*
 * Percentile Tracking (Histogram)
 *
 * Track latency distribution to capture p50, p99, p99.9 metrics.
 * Uses logarithmic buckets to reduce memory overhead.
 */

/* Histogram buckets (log scale):
 * 0: <100ns, 1: 100-200ns, 2: 200-400ns, 3: 400-800ns,
 * 4: 800ns-1.6us, 5: 1.6-3.2us, 6: 3.2-6.4us, 7: 6.4-12.8us,
 * 8: 12.8-25.6us, 9: 25.6-51.2us, 10: 51.2-102.4us, 11: >102.4us */
#define HIST_BUCKETS 12

extern volatile u64 hist_select_cpu[HIST_BUCKETS];
extern volatile u64 hist_enqueue[HIST_BUCKETS];
extern volatile u64 hist_dispatch[HIST_BUCKETS];

/*
 * Convert nanoseconds to histogram bucket index
 * Uses log2 for logarithmic bucketing
 */
static __always_inline u32 ns_to_bucket(u64 ns)
{
	u32 bucket = 0;
	u64 threshold = 100;  /* Start at 100ns */

	/* Find bucket: each bucket doubles the threshold */
	while (ns >= threshold && bucket < HIST_BUCKETS - 1) {
		bucket++;
		threshold <<= 1;  /* Double threshold */
	}

	return bucket;
}

/*
 * Record latency measurement in histogram
 */
#define PROF_HIST(name, elapsed_ns) \
	if (!no_stats) { \
		u32 bucket = ns_to_bucket(elapsed_ns); \
		if (bucket < HIST_BUCKETS) \
			__atomic_fetch_add(&hist_##name[bucket], 1, __ATOMIC_RELAXED); \
	}

/*
 * Combined profiling: accumulate total time AND histogram
 */
#define PROF_START_HIST(name) \
	u64 __prof_##name##_start = 0; \
	if (!no_stats) __prof_##name##_start = scx_bpf_now()

#define PROF_END_HIST(name) \
	if (!no_stats && __prof_##name##_start) { \
		u64 __prof_elapsed = scx_bpf_now() - __prof_##name##_start; \
		__atomic_fetch_add(&prof_##name##_ns_total, __prof_elapsed, __ATOMIC_RELAXED); \
		__atomic_fetch_add(&prof_##name##_calls, 1, __ATOMIC_RELAXED); \
		u32 bucket = ns_to_bucket(__prof_elapsed); \
		if (bucket < HIST_BUCKETS) \
			__atomic_fetch_add(&hist_##name[bucket], 1, __ATOMIC_RELAXED); \
	}

#else /* !ENABLE_PROFILING */

/*
 * Hot-Path Latency Profiling (DISABLED)
 *
 * All profiling macros are compile-time no-ops for maximum performance.
 * This is the default configuration for production/gaming use.
 *
 * To enable profiling: rebuild with CFLAGS="-DENABLE_PROFILING"
 */

/* No-op macros - compiler will optimize these away entirely */
#define PROF_START(name)		do {} while (0)
#define PROF_END(name)			do {} while (0)
#define PROF_HIST(name, elapsed_ns)	do {} while (0)
#define PROF_START_HIST(name)		do {} while (0)
#define PROF_END_HIST(name)		do {} while (0)

#endif /* ENABLE_PROFILING */

#endif /* __GAMER_PROFILING_BPF_H */
