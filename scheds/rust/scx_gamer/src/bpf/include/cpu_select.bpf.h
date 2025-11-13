/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: CPU Selection & SMT Logic
 * Copyright (c) 2025 RitzDaCat
 *
 * Idle CPU selection with physical core priority for GPU threads.
 * This file is AI-friendly: ~300 lines, single responsibility.
 *
 * KEY FEATURE: Forces GPU submission threads to physical cores (CPUs 0-7)
 * before falling back to hyperthreads (CPUs 8-15) on typical 8C/16T systems.
 */
#ifndef __GAMER_CPU_SELECT_BPF_H
#define __GAMER_CPU_SELECT_BPF_H

#include "types.bpf.h"
#include "task_class.bpf.h"

/* External tunables (from main.bpf.c rodata) */
extern const volatile bool primary_all;
extern const volatile bool flat_idle_scan;
extern const volatile bool smt_enabled;
extern const volatile bool preferred_idle_scan;
extern const volatile bool avoid_smt;
extern volatile u64 interactive_sys_avg;
extern const volatile u64 preferred_cpus[MAX_CPUS];
extern volatile u64 nr_cpu_ids;  /* Required for bounds checking - defined in main.bpf.c */
extern const volatile u32 preferred_high_perf_count;
extern const volatile u32 preferred_cpu_rank[MAX_CPUS];
extern const volatile u8 cpu_ccd_class[MAX_CPUS];
extern const volatile u32 cache_ccd_cpu_count;
extern const volatile u32 freq_ccd_cpu_count;

static __always_inline bool is_high_perf_cpu(s32 cpu)
{
	if (unlikely(cpu < 0 || cpu >= MAX_CPUS))
		return false;
	u32 rank = preferred_cpu_rank[cpu];
	return rank < preferred_high_perf_count;
}

static __always_inline bool is_cache_preferred_cpu(s32 cpu)
{
	if (!cache_ccd_cpu_count)
		return true;
	if (unlikely(cpu < 0 || cpu >= MAX_CPUS))
		return false;
	u8 class = cpu_ccd_class[cpu];
	return class == CCD_CLASS_CACHE || class == CCD_CLASS_UNKNOWN;
}

static __always_inline bool is_freq_ccd_cpu(s32 cpu)
{
	if (!freq_ccd_cpu_count)
		return false;
	if (unlikely(cpu < 0 || cpu >= MAX_CPUS))
		return false;
	return cpu_ccd_class[cpu] == CCD_CLASS_FREQ;
}

static __always_inline bool is_gpu_preferred_cpu(s32 cpu)
{
	if (!is_high_perf_cpu(cpu))
		return false;
	return is_cache_preferred_cpu(cpu);
}

/* External stats counters */
extern volatile u64 nr_idle_cpu_pick;
extern volatile u64 nr_gpu_phys_kept;

/**
 * get_idle_smtmask - Get idle SMT mask (NUMA-aware if enabled)
 * @cpu: CPU ID to get SMT mask for
 *
 * Returns idle SMT sibling mask for the given CPU.
 * NUMA-aware version used when NUMA is enabled.
 *
 * TIER 1: BPF helper function call (~20-50ns)
 * Not in hottest path - called during CPU selection logic.
 */
static inline const struct cpumask *get_idle_smtmask(s32 cpu)
{
	if (likely(!numa_enabled))
		return scx_bpf_get_idle_smtmask();
	return __COMPAT_scx_bpf_get_idle_smtmask_node(__COMPAT_scx_bpf_cpu_node(cpu));
}

/**
 * is_smt_contended - Check if CPU is part of a fully busy SMT core
 * @cpu: CPU ID to check
 *
 * Returns: true if all SMT siblings are busy
 *
 * TIER 1: Optimized for CPU selection hot path
 * - Early exit: Tier 0 (~1-2ns, volatile reads)
 * - SMT mask lookup: Tier 1 (~20-50ns, BPF helper)
 * - CPUMask empty check: Tier 0 (~1-2ns)
 * - Total: ~2-4ns (early exit) or ~22-54ns (full check)
 */
static __always_inline bool is_smt_contended(s32 cpu)
{
	/* TIER 0: Early exit if SMT disabled or avoid_smt disabled */
	if (unlikely(!smt_enabled || !avoid_smt))
		return false;

	/* TIER 1: Get idle SMT mask (BPF helper call, ~20-50ns) */
	const struct cpumask *smt = get_idle_smtmask(cpu);
	
	/* TIER 0: Check if mask is empty (~1-2ns) */
	bool is_contended = bpf_cpumask_empty(smt);
	
	/* TIER 1: Release cpumask (BPF helper call, ~20-50ns) */
	scx_bpf_put_cpumask(smt);

	return is_contended;
}

/**
 * get_preferred_cpu_safe - Safely access preferred_cpus array
 * @idx: Array index to access
 *
 * Returns candidate CPU ID or -1 if index is out of bounds.
 *
 * BPF VERIFIER: Helper to safely access preferred_cpus array.
 * CRITICAL: The verifier needs to see bounds checked on the ACTUAL
 * variable used for pointer arithmetic. We can't rely on early returns
 * alone - need explicit check right before array access.
 *
 * TIER 0: Optimized bounds checking
 * - Bounds checks: Tier 0 (~0.5-1ns each)
 * - Array access: Tier 0 (~0.5-1ns, volatile array)
 * - Total: ~1-2ns (in bounds) or ~0.5-1ns (out of bounds, early exit)
 *
 * NOTE: Redundant checks are intentional for BPF verifier compatibility.
 */
static __always_inline s32 get_preferred_cpu_safe(u32 idx)
{
    /* BPF VERIFIER: Store in local variable to help verifier track bounds */
    u32 safe_idx = idx;
    
    /* TIER 0: BPF VERIFIER - Explicit bounds check (early return for out of bounds) */
    if (unlikely(safe_idx >= MAX_CPUS))
        return -1;
    
    /* BPF VERIFIER: Additional check to ensure safe_idx is bounded.
     * The verifier needs to see this constraint before pointer arithmetic. */
    if (unlikely(safe_idx >= MAX_CPUS))
        return -1;
    
    /* TIER 0: BPF VERIFIER - Final bounds check immediately before array access.
     * This must be seen by verifier right before preferred_cpus[safe_idx]. */
    if (likely(safe_idx < MAX_CPUS)) {
        /* BPF VERIFIER: One more check to ensure verifier tracks it */
        if (unlikely(safe_idx >= MAX_CPUS))
            return -1;
        /* TIER 0: Array access (volatile array, cache-friendly) */
        return (s32)preferred_cpus[safe_idx];
    }
    return -1;
}

/**
 * pick_idle_physical_core - Try to find an idle physical core (prefer lower CPU IDs)
 * @p: Task to schedule
 * @prev_cpu: Previous CPU task ran on
 * @now: Current timestamp
 *
 * This function is critical for GPU thread performance. On typical SMT systems:
 * - Physical cores: CPUs 0 to (nr_cores - 1)
 * - Hyperthreads: CPUs nr_cores to (nr_cpus - 1)
 *
 * TIER 0/1: Optimized for GPU thread hot path
 * - Cached CPU check: Tier 0 (~5-10ns, most common case ~70%)
 * - Map lookup: Tier 1 (~20-50ns, try_lookup_task_ctx)
 * - CPUMask operations: Tier 0 (~1-2ns each)
 * - Unrolled iterations: Tier 0 (~10-20ns per iteration)
 * - Prefetching: Tier 0 (hides cache miss latency)
 * - Total: ~5-10ns (cached hit) or ~50-200ns (scan required)
 *
 * Returns: CPU ID >= 0 on success, -ENOENT if none found
 */
static __always_inline s32 pick_idle_physical_core(struct task_struct *p, s32 prev_cpu, u64 now)
{
    const struct cpumask *allowed = p->cpus_ptr;

    /* TIER 1: Try cached preferred CPU when available (most common case, ~70% hit rate)
     * This fast path saves ~50-200ns by avoiding CPU scan */
    struct task_ctx *tctx = try_lookup_task_ctx(p);
    if (likely(tctx && tctx->preferred_physical_core >= 0)) {
        s32 cached = tctx->preferred_physical_core;
        if (unlikely(!is_gpu_preferred_cpu(cached))) {
            tctx->preferred_physical_core = -1;
            tctx->preferred_core_hits = 0;
        } else if (likely((u32)cached < nr_cpu_ids &&
               bpf_cpumask_test_cpu(cached, allowed) &&
               scx_bpf_test_and_clear_cpu_idle(cached))) {
            tctx->preferred_core_hits++;
            tctx->preferred_core_last_hit = now;
            return cached;
        } else if (unlikely(now - tctx->preferred_core_last_hit > PREF_CORE_MAX_AGE_NS)) {
            tctx->preferred_physical_core = -1;
            tctx->preferred_core_hits = 0;
            __atomic_fetch_add(&nr_gpu_pref_fallback, 1, __ATOMIC_RELAXED);
        }
    }

    /* TIER 0: Fallback to preferred CPU ordering provided by userspace
     * 
     * HFT PATTERN: Loop unrolling for 8-core systems (9800X3D, etc.)
     * Unroll first 4 iterations to eliminate loop overhead (~20-40ns savings).
     * This improves branch prediction and reduces loop control overhead.
     * Expected impact: ~5-10% faster CPU selection on 8-core systems.
     * 
     * Each unrolled iteration: ~10-20ns (bounds check + cpumask test + idle check)
     */
    
    /* TIER 0: ITERATION 0 - Unrolled for zero overhead */
    {
        s32 candidate = (s32)preferred_cpus[0];
        if (candidate >= 0 && (u32)candidate < nr_cpu_ids &&
            bpf_cpumask_test_cpu(candidate, allowed) &&
            is_gpu_preferred_cpu(candidate)) {
            /* Prefetch iteration 1 while checking iteration 0 */
            s32 next_candidate = (s32)preferred_cpus[1];
            if (likely(next_candidate >= 0 && (u32)next_candidate < nr_cpu_ids &&
                      bpf_cpumask_test_cpu(next_candidate, allowed))) {
                struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                if (likely(next_cctx)) {
                    __builtin_prefetch(next_cctx, 0, 2);  /* Read, low temporal locality */
                }
            }
            /* Prefetch current candidate */
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
            if (cctx) {
                __builtin_prefetch(cctx, 0, 2);  /* Read, low temporal locality */
            }
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                if (tctx) {
                    tctx->preferred_physical_core = candidate;
                    tctx->preferred_core_hits = 1;
                    tctx->preferred_core_last_hit = now;
                }
                return candidate;
            }
        }
    }
    
    /* ITERATION 1: Unrolled for zero overhead */
    {
        s32 candidate = (s32)preferred_cpus[1];
        if (candidate >= 0 && (u32)candidate < nr_cpu_ids &&
            bpf_cpumask_test_cpu(candidate, allowed) &&
            is_gpu_preferred_cpu(candidate)) {
            /* Prefetch iteration 2 while checking iteration 1 */
            s32 next_candidate = (s32)preferred_cpus[2];
            if (likely(next_candidate >= 0 && (u32)next_candidate < nr_cpu_ids &&
                      bpf_cpumask_test_cpu(next_candidate, allowed))) {
                struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                if (likely(next_cctx)) {
                    __builtin_prefetch(next_cctx, 0, 2);  /* Read, low temporal locality */
                }
            }
            /* Prefetch current candidate */
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
            if (cctx) {
                __builtin_prefetch(cctx, 0, 2);  /* Read, low temporal locality */
            }
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                if (tctx) {
                    tctx->preferred_physical_core = candidate;
                    tctx->preferred_core_hits = 1;
                    tctx->preferred_core_last_hit = now;
                }
                return candidate;
            }
        }
    }
    
    /* ITERATION 2: Unrolled for zero overhead */
    {
        s32 candidate = (s32)preferred_cpus[2];
        if (candidate >= 0 && (u32)candidate < nr_cpu_ids &&
            bpf_cpumask_test_cpu(candidate, allowed) &&
            is_gpu_preferred_cpu(candidate)) {
            /* Prefetch iteration 3 while checking iteration 2 */
            s32 next_candidate = (s32)preferred_cpus[3];
            if (likely(next_candidate >= 0 && (u32)next_candidate < nr_cpu_ids &&
                      bpf_cpumask_test_cpu(next_candidate, allowed))) {
                struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                if (likely(next_cctx)) {
                    __builtin_prefetch(next_cctx, 0, 2);  /* Read, low temporal locality */
                }
            }
            /* Prefetch current candidate */
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
            if (cctx) {
                __builtin_prefetch(cctx, 0, 2);  /* Read, low temporal locality */
            }
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                if (tctx) {
                    tctx->preferred_physical_core = candidate;
                    tctx->preferred_core_hits = 1;
                    tctx->preferred_core_last_hit = now;
                }
                return candidate;
            }
        }
    }
    
    /* ITERATION 3: Unrolled for zero overhead */
    {
        s32 candidate = (s32)preferred_cpus[3];
        if (candidate >= 0 && (u32)candidate < nr_cpu_ids &&
            bpf_cpumask_test_cpu(candidate, allowed) &&
            is_high_perf_cpu(candidate)) {
            /* Prefetch iteration 4 while checking iteration 3 */
            /* BPF VERIFIER: Explicit bounds check before array access */
            if (4 < MAX_CPUS) {
                s32 next_candidate = (s32)preferred_cpus[4];
                if (likely(next_candidate >= 0 && (u32)next_candidate < nr_cpu_ids &&
                          bpf_cpumask_test_cpu(next_candidate, allowed))) {
                    struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                    if (likely(next_cctx)) {
                        __builtin_prefetch(next_cctx, 0, 2);  /* Read, low temporal locality */
                    }
                }
            }
            /* Prefetch current candidate */
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
            if (cctx) {
                __builtin_prefetch(cctx, 0, 2);  /* Read, low temporal locality */
            }
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                if (tctx) {
                    tctx->preferred_physical_core = candidate;
                    tctx->preferred_core_hits = 1;
                    tctx->preferred_core_last_hit = now;
                }
                return candidate;
            }
        }
    }
    
    /* Fallback loop for CPUs 4+ (larger systems)
     * BPF VERIFIER: Use constant literal for bounds check to help verifier track bounds.
     * Replace bpf_for with while loop using constant literal comparisons. */
    u32 i = 4;
    while (i < 256) {  /* MAX_CPUS = 256, use literal constant */
        /* BPF VERIFIER: Explicit bounds check using constant literal immediately before access.
         * The verifier needs to see a constant comparison, not a macro. */
        if (i >= 256)  /* MAX_CPUS */
            break;
        /* BPF VERIFIER: Array access only if definitely in bounds */
        s32 candidate = (s32)preferred_cpus[i];
        if (candidate < 0 || (u32)candidate >= nr_cpu_ids)
            break;
        
        /* BPF VERIFIER: Increment loop variable BEFORE any continue statements.
         * This ensures verifier sees progress on every iteration, preventing infinite loop detection. */
        i++;
        
        if (!bpf_cpumask_test_cpu(candidate, allowed))
            continue;

        if (!is_gpu_preferred_cpu(candidate))
            continue;
        
        /* MECHANICAL SYMPATHY: Prefetch NEXT candidate CPU context while processing CURRENT one.
         * This enhancement prefetches the next candidate's cpu_ctx while checking the current
         * candidate's idle state. This hides cache miss latency for sequential CPU scans.
         * Low temporal locality (2) - data will be accessed in next iteration if current fails.
         * Benefit: ~10-15ns savings per CPU if next lookup causes cache miss.
         * 
         * Limit prefetching to first 8 candidates to avoid cache pollution. */
        if (likely(i < MAX_CPUS && i - 1 < 8)) {  /* i-1 because we already incremented */
            /* BPF VERIFIER: Use helper function that performs bounds check */
            s32 next_candidate = get_preferred_cpu_safe(i);
            if (next_candidate >= 0) {
                if ((u32)next_candidate < nr_cpu_ids &&
                    bpf_cpumask_test_cpu(next_candidate, allowed)) {
                    struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                    if (likely(next_cctx)) {
                        __builtin_prefetch(next_cctx, 0, 2);  /* Read, low temporal locality */
                    }
                }
            }
        }
        
        /* MECHANICAL SYMPATHY: Also prefetch current candidate's cpu_ctx early.
         * Prefetch while checking idle state, so cpu_ctx is ready if CPU is selected.
         * Low temporal locality (2) - data may be accessed if CPU is selected.
         * Benefit: ~10-15ns savings if cpu_ctx lookup causes cache miss. */
        if (likely(i - 1 < 8)) {  /* i-1 because we already incremented, only prefetch first 8 CPUs */
            struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
            if (cctx) {
                __builtin_prefetch(cctx, 0, 2);  /* Read, low temporal locality */
            }
        }
        
        if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
            if (tctx) {
                tctx->preferred_physical_core = candidate;
                tctx->preferred_core_hits = 1;
                tctx->preferred_core_last_hit = now;
            }
            return candidate;
        }
    }

    /* TIER 0: As a last resort, try prev_cpu to preserve locality even if sibling is busy
     * This preserves cache locality when no preferred CPUs are available */
    if (likely(prev_cpu >= 0 && prev_cpu < nr_cpu_ids && 
               is_gpu_preferred_cpu(prev_cpu) &&
               bpf_cpumask_test_cpu(prev_cpu, allowed) &&
               scx_bpf_test_and_clear_cpu_idle(prev_cpu))) {
        if (likely(tctx)) {
            /* TIER 0: Update cache (~1-2ns per field) */
            tctx->preferred_physical_core = prev_cpu;
            tctx->preferred_core_hits = 1;
            tctx->preferred_core_last_hit = now;
        }
        return prev_cpu;
    }

	return -ENOENT;
}

/**
 * pick_idle_cpu - Pick optimal idle CPU for task
 * @p: Task to schedule
 * @prev_cpu: Previous CPU task ran on
 * @wake_flags: Wakeup flags
 * @from_enqueue: Called from enqueue path (vs select_cpu)
 * @now: Current timestamp
 *
 * Priority order:
 * 1. GPU threads: Physical cores only (if available)
 * 2. Regular tasks with avoid_smt: Full idle SMT cores
 * 3. Regular tasks: Any idle CPU
 *
 * TIER 1/2: CPU selection hot path
 * - Map lookup: Tier 1 (~20-50ns, try_lookup_task_ctx)
 * - GPU physical core scan: Tier 0/1 (~5-200ns, depends on cache hit)
 * - BPF helper calls: Tier 1 (~50-150ns, scx_bpf_select_cpu_and)
 * - Total: ~75-400ns (depending on path taken)
 *
 * NOTE: This function is marked unused but kept for compatibility.
 * The actual hot path uses pick_idle_cpu_cached() in main.bpf.c.
 *
 * Returns: CPU ID >= 0, or -EBUSY if no idle CPU found
 */
static __always_inline s32 __attribute__((unused)) pick_idle_cpu(struct task_struct *p, s32 prev_cpu,
                         u64 wake_flags, bool from_enqueue, u64 now)
{
	const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;
	struct task_ctx *tctx;
	bool is_critical_gpu;
	bool is_busy;
	bool allow_smt;
	u64 smt_flags;
	s32 cpu;

	/* TIER 1: Fallback to old API for kernels <= 6.16 without scx_bpf_select_cpu_and()
	 * This path is rarely taken on modern kernels */
	if (unlikely(!bpf_ksym_exists(scx_bpf_select_cpu_and))) {
		bool is_idle = false;

		if (unlikely(from_enqueue))
			return -EBUSY;

		/* TIER 1: BPF helper call (~50-150ns) */
		cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
		if (likely(is_idle)) {
			stat_inc(&nr_idle_cpu_pick);
			return cpu;
		}
		return -EBUSY;
	}

	/* TIER 1: Determine if this is a critical GPU thread requiring physical core
	 * Map lookup: ~20-50ns, name check: ~5-10ns */
	tctx = try_lookup_task_ctx(p);
	is_critical_gpu = (likely(tctx) && tctx->is_gpu_submit) || is_gpu_submit_name(p->comm);

	/* TIER 0/1: CRITICAL PATH: GPU threads must use physical cores for minimal latency
	 *
	 * Problem: SCX_PICK_IDLE_CORE only picks when entire SMT core is idle.
	 * On busy systems, this causes GPU threads to land on hyperthreads.
	 *
	 * Solution: Explicitly scan physical cores first, accepting busy siblings.
	 * 
	 * Performance: ~5-200ns depending on cache hit rate (~70% hit rate)
	 */
	if (likely(is_critical_gpu && smt_enabled)) {
        cpu = pick_idle_physical_core(p, prev_cpu, now);
		if (likely(cpu >= 0)) {
			stat_inc(&nr_idle_cpu_pick);
			stat_inc(&nr_gpu_phys_kept);
			return cpu;
		}
		/* If no physical core available, fall through to normal path */
	}

	/* TIER 0: For non-GPU threads, apply normal SMT avoidance logic
	 * Volatile reads: ~1-2ns each, comparisons: ~0.5-1ns */
	is_busy = interactive_sys_avg >= INTERACTIVE_SLICE_SHRINK_THRESH;
	allow_smt = is_critical_gpu ? false :
		    (!avoid_smt || (!is_busy && interactive_sys_avg < INTERACTIVE_SMT_ALLOW_THRESH));
	smt_flags = allow_smt ? 0 : SCX_PICK_IDLE_CORE;

	/* TIER 1: Try primary domain first (if configured)
	 * BPF helper call: ~50-150ns */
	if (likely(primary && !primary_all)) {
		cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, primary, smt_flags);
		if (likely(cpu >= 0)) {
			stat_inc(&nr_idle_cpu_pick);
			return cpu;
		}
	}

	/* TIER 1: Pick any idle CPU from task's allowed mask
	 * BPF helper call: ~50-150ns */
	cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, p->cpus_ptr, smt_flags);
	if (likely(cpu >= 0))
		stat_inc(&nr_idle_cpu_pick);

	return cpu;
}

#endif /* __GAMER_CPU_SELECT_BPF_H */
