/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Dispatch Coalescing for CPU Reduction
 * Copyright (c) 2025 RitzDaCat
 *
 * Counter-based coalescing to reduce CPU usage in high-frequency dispatch path.
 * Replaces time-based checks with modulo counters to avoid redundant scx_bpf_now() calls.
 */
#ifndef __GAMER_COALESCE_BPF_H
#define __GAMER_COALESCE_BPF_H

/*
 * DISPATCH COALESCING STRATEGY
 * 
 * Problem: gamer_dispatch() is called ~940k times/sec, consuming 4.78% CPU.
 * Both maybe_sample_cpu_util() and maybe_run_housekeeping() call scx_bpf_now()
 * every dispatch to check if enough time has passed, but only execute rarely.
 * 
 * Solution: Use modulo counters instead of time checks.
 * 
 * Target frequencies:
 * - CPU util sampling: Every 500μs = ~2000/sec
 * - Housekeeping: Every 5ms = ~200/sec
 * 
 * At 940k dispatch/sec:
 * - Sample util every ~470 calls (940k / 2000)
 * - Run housekeeping every ~4700 calls (940k / 200)
 * 
 * Savings: ~1.88 million scx_bpf_now() calls/sec eliminated = ~0.94-1.88% CPU
 */

/* Coalescing intervals (call counts, not time) */
#define UTIL_SAMPLE_EVERY     512    /* Power of 2 for fast modulo (bitwise AND) */
#define HOUSEKEEPING_EVERY    4096   /* Power of 2 for fast modulo */
#define CLASSIFICATION_CHECK_EVERY 256 /* Check classification refresh every 256 runnable calls */

/* Per-CPU dispatch call counters (avoid atomic contention) */
struct dispatch_coalesce_ctx {
	u32 dispatch_call_count;         /* Incremented on every dispatch */
	u32 util_sample_calls;           /* Calls since last util sample */
	u32 housekeeping_calls;          /* Calls since last housekeeping */
	u32 runnable_call_count;         /* Incremented on every runnable */
};

/* Per-CPU array for coalescing state */
struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__type(key, u32);
	__type(value, struct dispatch_coalesce_ctx);
	__uint(max_entries, 1);
} dispatch_coalesce_stor SEC(".maps");

/**
 * should_sample_cpu_util - Counter-based coalescing for CPU util sampling
 * 
 * Returns true every ~512 dispatch calls (adaptive to actual dispatch rate).
 * This maintains ~500μs sampling rate at 940k dispatch/sec without calling scx_bpf_now().
 * 
 * TIER 0: Modulo operation on per-CPU counter (~1-2ns vs 5-10ns for scx_bpf_now())
 * Savings: ~940k scx_bpf_now() calls/sec eliminated = ~4.7-9.4 million ns/sec
 */
static __always_inline bool should_sample_cpu_util(void)
{
	const u32 idx = 0;
	struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
	if (!ctx)
		return false;  /* Fallback: skip if map lookup fails */
	
	ctx->util_sample_calls++;
	
	/* TIER 0: Bitwise AND for modulo (faster than % operator)
	 * (X & (N-1)) == (X % N) when N is power of 2 */
	if ((ctx->util_sample_calls & (UTIL_SAMPLE_EVERY - 1)) == 0) {
		ctx->util_sample_calls = 0;  /* Reset counter */
		return true;
	}
	
	return false;
}

/**
 * should_run_housekeeping - Counter-based coalescing for housekeeping tasks
 * 
 * Returns true every ~4096 dispatch calls (adaptive to actual dispatch rate).
 * This maintains ~5ms housekeeping rate at 940k dispatch/sec without calling scx_bpf_now().
 * 
 * TIER 0: Modulo operation on per-CPU counter (~1-2ns vs 5-10ns for scx_bpf_now())
 * Savings: ~940k scx_bpf_now() calls/sec eliminated = ~4.7-9.4 million ns/sec
 */
static __always_inline bool should_run_housekeeping(void)
{
	const u32 idx = 0;
	struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
	if (!ctx)
		return false;  /* Fallback: skip if map lookup fails */
	
	ctx->housekeeping_calls++;
	
	/* TIER 0: Bitwise AND for modulo (faster than % operator) */
	if ((ctx->housekeeping_calls & (HOUSEKEEPING_EVERY - 1)) == 0) {
		ctx->housekeeping_calls = 0;  /* Reset counter */
		return true;
	}
	
	return false;
}

/**
 * increment_dispatch_counter - Track total dispatch calls (for stats)
 * 
 * Optional counter for monitoring dispatch frequency.
 * Can be used to auto-tune UTIL_SAMPLE_EVERY and HOUSEKEEPING_EVERY dynamically.
 */
static __always_inline void increment_dispatch_counter(void)
{
	const u32 idx = 0;
	struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
	if (ctx)
		ctx->dispatch_call_count++;
}

/*
 * RUNNABLE CLASSIFICATION COALESCING STRATEGY
 * 
 * Problem: gamer_runnable() is called ~124k times/sec under gaming load.
 * Every call executes scx_bpf_now() to check if classification should refresh,
 * even though most tasks have stable classifications (exponential backoff).
 * 
 * Solution: Gate the classification time check with a counter.
 * Only call scx_bpf_now() and check classification refresh every N runnable calls.
 * 
 * At 124k runnable/sec with CLASSIFICATION_CHECK_EVERY=256:
 * - Classification check runs ~484 times/sec (124k / 256)
 * - Eliminates ~123.5k scx_bpf_now() calls/sec
 * 
 * Savings: ~123.5k scx_bpf_now() calls/sec = ~0.62-1.24M ns/sec = 0.06-0.12% CPU
 * 
 * CRITICAL: This doesn't skip classification for new tasks or when explicitly needed.
 * It only gates the periodic refresh check for already-classified tasks.
 */

/**
 * should_check_classification - Counter-based coalescing for classification refresh
 * 
 * Returns true every ~256 runnable calls to gate expensive classification time checks.
 * New tasks (is_first_classification=true) bypass this and always run classification.
 * 
 * TIER 0: Modulo operation on per-CPU counter (~1-2ns vs 5-10ns for scx_bpf_now())
 * Savings: ~123k scx_bpf_now() calls/sec eliminated under gaming load
 */
static __always_inline bool should_check_classification(void)
{
	const u32 idx = 0;
	struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
	if (!ctx)
		return true;  /* Fallback: always check if map lookup fails */
	
	ctx->runnable_call_count++;
	
	/* TIER 0: Bitwise AND for modulo (faster than % operator) */
	bool should_check = (ctx->runnable_call_count & (CLASSIFICATION_CHECK_EVERY - 1)) == 0;
	
	return should_check;
}

#endif /* __GAMER_COALESCE_BPF_H */

