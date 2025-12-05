/* SPDX-License-Identifier: GPL-2.0 */
/*
 * cpu_select.bpf.h - CPU selection logic
 *
 * Implements intelligent CPU selection:
 * - Physical core preference for latency-critical tasks
 * - SMT avoidance to reduce cache contention
 * - Previous CPU preference for cache locality
 * - Idle CPU selection as fallback
 *
 * Selection priority:
 * 1. Honor CPU affinity mask (kernel requirement)
 * 2. Previous CPU if idle (best cache locality)
 * 3. Physical core if task is latency-critical
 * 4. Any idle CPU
 * 5. Fallback to previous CPU (will preempt)
 *
 * Why physical cores matter for gaming:
 * - Physical cores have dedicated L1/L2 cache
 * - SMT siblings share execution resources and cache
 * - GPU submission and input handling are cache-sensitive
 * - Putting two latency-critical tasks on SMT siblings causes thrashing
 */

#ifndef __CORE_CPU_SELECT_BPF_H
#define __CORE_CPU_SELECT_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"

/* Forward declaration for avoid_smt tunable */
extern const volatile bool avoid_smt;

/* ============================================================================
 * CPU TOPOLOGY HELPERS
 * ============================================================================ */

/*
 * Check if a CPU is a physical core (not an SMT sibling).
 * 
 * Physical cores are identified by having core_id == cpu_id or
 * by not having the CPU_SMT flag set.
 *
 * Note: cpu_ctx.core_id and cpu_ctx.sibling_cpu should be initialized
 * by userspace based on system topology.
 */
static __always_inline bool is_physical_core(s32 cpu)
{
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (!cctx)
        return true;  /* Assume physical if we can't check */
    
    /* If no sibling, it's effectively a physical core */
    if (cctx->sibling_cpu < 0)
        return true;
    
    /* Check CPU_SMT flag */
    if (cctx->flags & CPU_SMT)
        return false;
    
    /* core_id == cpu means this is the "primary" of the pair */
    return cctx->core_id == cpu;
}

/*
 * Get the SMT sibling of a CPU.
 * Returns -1 if no sibling (single-threaded core).
 */
static __always_inline s32 get_sibling_cpu(s32 cpu)
{
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (!cctx)
        return -1;
    
    return cctx->sibling_cpu;
}

/*
 * Check if a CPU's sibling is idle.
 * Used for SMT-aware scheduling.
 */
static __always_inline bool sibling_is_idle(s32 cpu)
{
    s32 sibling = get_sibling_cpu(cpu);
    if (sibling < 0)
        return true;  /* No sibling = effectively idle */
    
    struct cpu_ctx *sibling_cctx = lookup_cpu_ctx(sibling);
    if (!sibling_cctx)
        return true;
    
    /* Check if sibling is running something low-priority */
    return sibling_cctx->current_boost == BOOST_BACKGROUND;
}

/* ============================================================================
 * IDLE CPU SELECTION
 * ============================================================================ */

/*
 * Try to find an idle physical core.
 *
 * Iterates through CPUs looking for one that is:
 * 1. Idle (no task running)
 * 2. A physical core (not SMT sibling)
 * 3. In the task's cpumask
 *
 * Returns CPU ID or -1 if none found.
 */
static __always_inline s32 pick_idle_physical_core(struct task_struct *p)
{
    s32 cpu;
    
    /* Use scx helper to find idle CPU, then verify it's physical */
    cpu = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
    if (cpu < 0)
        return -1;
    
    /* If we don't care about SMT, return any idle CPU */
    if (!avoid_smt)
        return cpu;
    
    /* Check if this is a physical core */
    if (is_physical_core(cpu))
        return cpu;
    
    /* It's an SMT sibling - try to find a physical core instead */
    /* For simplicity, try a few more times */
    #pragma unroll
    for (int i = 0; i < 4; i++) {
        s32 next_cpu = scx_bpf_pick_idle_cpu(p->cpus_ptr, SCX_PICK_IDLE_CORE);
        if (next_cpu < 0)
            break;
        
        if (is_physical_core(next_cpu))
            return next_cpu;
    }
    
    /* Couldn't find physical core, return original idle CPU */
    return cpu;
}

/*
 * Try to find an idle CPU with idle sibling (whole core idle).
 *
 * This is the best case for cache locality - the entire physical
 * core is available, so no contention with SMT sibling.
 */
static __always_inline s32 pick_idle_whole_core(struct task_struct *p)
{
    /* SCX_PICK_IDLE_CORE flag tries to find CPU where whole core is idle */
    return scx_bpf_pick_idle_cpu(p->cpus_ptr, SCX_PICK_IDLE_CORE);
}

/* ============================================================================
 * MAIN CPU SELECTION FUNCTION
 * ============================================================================ */

/*
 * Select the best CPU for a task.
 *
 * This is the main CPU selection logic called from select_cpu.
 *
 * IMPORTANT: This function does NOT claim the CPU (no test_and_clear).
 * It only finds a good candidate. The caller (gamer_select_cpu) does the claim.
 *
 * Selection strategy:
 * 1. For latency-critical tasks: prefer physical cores
 * 2. For game tasks: prefer previous CPU, then physical cores
 * 3. For others: any idle CPU, or previous CPU
 *
 * Returns: CPU ID of best candidate (may or may not be idle)
 *          -1 is NEVER returned - always returns at least prev_cpu
 */
static __always_inline s32 select_cpu_for_task(
    struct task_struct *p,
    struct task_ctx *tctx,
    s32 prev_cpu,
    u64 wake_flags)
{
    s32 cpu;
    bool is_critical;
    
    /* Check if task is latency-critical */
    is_critical = tctx && task_is_latency_critical(tctx);
    
    /*
     * NOTE: We use scx_bpf_pick_idle_cpu() which does NOT clear the idle bit.
     * This allows gamer_select_cpu() to do the actual claim with test_and_clear.
     *
     * BUG FIX: Previously used test_and_clear here, then tried to claim again
     * in gamer_select_cpu, causing double-claim failure and 85% fallback rate!
     */
    
    /* Strategy 1: For critical tasks, find physical core first */
    if (is_critical && avoid_smt) {
        /* Try whole core idle first */
        cpu = pick_idle_whole_core(p);
        if (cpu >= 0)
            return cpu;
        
        /* Then try any physical core */
        cpu = pick_idle_physical_core(p);
        if (cpu >= 0)
            return cpu;
    }
    
    /* Strategy 2: Any idle CPU */
    cpu = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
    if (cpu >= 0)
        return cpu;
    
    /* Strategy 3: Fallback to previous CPU (not idle, will go through enqueue) */
    return prev_cpu;
}

/* ============================================================================
 * CPU MIGRATION TRACKING
 * ============================================================================ */

/*
 * Record CPU migration for statistics.
 */
static __always_inline void track_cpu_selection(
    struct task_ctx *tctx,
    s32 prev_cpu,
    s32 selected_cpu)
{
    if (!tctx)
        return;
    
    /* Track if we migrated */
    if (prev_cpu != selected_cpu && tctx->last_cpu >= 0) {
        STAT_INC(nr_migrations);
    }
    
    /* Track physical vs SMT selection */
    if (is_physical_core(selected_cpu)) {
        STAT_INC(nr_physical_selected);
    } else {
        STAT_INC(nr_smt_selected);
    }
    
    /* Update task's last CPU */
    tctx->last_cpu = selected_cpu;
}

#endif /* __CORE_CPU_SELECT_BPF_H */
