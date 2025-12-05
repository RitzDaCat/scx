/* SPDX-License-Identifier: GPL-2.0 */
/*
 * dispatch.bpf.h - Task dispatch logic
 *
 * Implements safe dispatch with:
 * - Affinity-aware CPU targeting
 * - Priority-based DSQ selection
 * - Safe fallback to shared DSQ
 *
 * Key principle: NEVER dispatch to a CPU not in task's cpumask.
 * The kernel will crash/hang if we violate affinity.
 *
 * DSQ Strategy (v2.1):
 * - Direct dispatch (SCX_DSQ_LOCAL_ON | cpu): ONLY when idle CPU found in select_cpu
 * - Shared DSQ (SHARED_DSQ): ALL tasks from enqueue() - with deadline ordering
 *
 * CRITICAL: Using local DSQ in enqueue() causes starvation of CPU-pinned
 * tasks (kworkers, ksoftirqd). They sit in SHARED_DSQ but dispatch() is
 * never called if local DSQ always has work.
 *
 * v2.1 fix: Use SHARED_DSQ for everything in enqueue(). Critical tasks
 * get early deadlines and CPU kicks for timely execution.
 */

#ifndef __CORE_DISPATCH_BPF_H
#define __CORE_DISPATCH_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"

/* ============================================================================
 * AFFINITY SAFETY
 * ============================================================================ */

/*
 * Check if a CPU is valid for a task's affinity.
 *
 * CRITICAL: Always verify before dispatching to a specific CPU.
 * Violating affinity causes kernel panics or hangs.
 */
static __always_inline bool cpu_in_affinity(struct task_struct *p, s32 cpu)
{
    if (cpu < 0 || cpu >= MAX_CPUS)
        return false;
    
    /* Check task's cpumask */
    return bpf_cpumask_test_cpu(cpu, p->cpus_ptr);
}

/*
 * Get a safe CPU for dispatch.
 *
 * If target_cpu is not in affinity, falls back to task's current CPU.
 * If that also fails, returns -1 to signal shared DSQ dispatch.
 */
static __always_inline s32 get_safe_dispatch_cpu(struct task_struct *p, s32 target_cpu)
{
    /* Check target CPU */
    if (target_cpu >= 0 && cpu_in_affinity(p, target_cpu))
        return target_cpu;
    
    /* Fallback to task's current CPU */
    s32 task_cpu = scx_bpf_task_cpu(p);
    if (cpu_in_affinity(p, task_cpu))
        return task_cpu;
    
    /* Can't find safe CPU - will use shared DSQ */
    return -1;
}

/* Forward declaration for global vtime */
extern u64 vtime_now;

/* ============================================================================
 * DISPATCH HELPERS
 * ============================================================================ */

/*
 * Dispatch task to shared DSQ with deadline ordering.
 *
 * Uses scx_bpf_dsq_insert_vtime() which orders tasks by their
 * deadline (vtime parameter). Earlier deadline = runs first.
 *
 * This is the core change from FIFO to deadline-ordered dispatch.
 */
static __always_inline void dispatch_with_deadline(
    struct task_struct *p,
    struct task_ctx *tctx,
    u64 slice,
    u64 enq_flags)
{
    u64 deadline = task_deadline(p, tctx, vtime_now);
    
    /* Deadline-ordered insertion - tasks run in deadline order, not FIFO */
    scx_bpf_dsq_insert_vtime(p, SHARED_DSQ, slice, deadline, enq_flags);
    STAT_INC(nr_shared_dispatch);
}

/*
 * Dispatch task to shared DSQ (FIFO fallback).
 *
 * Used when we don't have task context (shouldn't happen often).
 * Falls back to FIFO ordering.
 */
static __always_inline void dispatch_shared_fifo(
    struct task_struct *p,
    u64 slice,
    u64 enq_flags)
{
    scx_bpf_dsq_insert(p, SHARED_DSQ, slice, enq_flags);
    STAT_INC(nr_shared_dispatch);
}

/* ============================================================================
 * DISPATCH DECISION
 * ============================================================================ */

/*
 * Decide how to dispatch a task.
 *
 * Returns true if task should be direct-dispatched (latency-critical).
 * Returns false if task should go to shared DSQ.
 */
static __always_inline bool should_direct_dispatch(struct task_ctx *tctx)
{
    if (!tctx)
        return false;
    
    /* Latency-critical tasks always get direct dispatch */
    if (task_is_latency_critical(tctx))
        return true;
    
    /* Foreground game tasks get direct dispatch */
    if (task_is_foreground_game(tctx))
        return true;
    
    /* High-boost tasks get direct dispatch */
    if (tctx->boost_shift >= BOOST_GAME_WORKER)
        return true;
    
    return false;
}

/* ============================================================================
 * KICK HELPERS
 * ============================================================================ */

/*
 * Kick CPU after dispatching a task to it.
 *
 * Uses smart_kick_cpu for priority-aware preemption.
 */
static __always_inline void kick_after_dispatch(s32 cpu, u8 task_boost)
{
    if (cpu < 0)
        return;
    
    /* Use priority-aware kick from boost.bpf.h */
    smart_kick_cpu(cpu, task_boost);
}

/*
 * Dispatch with deadline ordering and CPU kick.
 *
 * This is the primary dispatch function for latency-critical tasks.
 * Combines deadline-ordered dispatch with priority-aware CPU kicking.
 *
 * Why deadline + kick:
 * 1. Deadline ordering ensures the task gets fair priority in the queue
 * 2. Kicking ensures the target CPU wakes up quickly to process it
 * 3. If target CPU is busy, other CPUs can still pick up the task
 *
 * This replaces the old FIFO + kick approach which ignored deadlines.
 */
static __always_inline void dispatch_direct_and_kick(
    struct task_struct *p,
    struct task_ctx *tctx,
    s32 target_cpu,
    u64 slice,
    u64 enq_flags)
{
    s32 safe_cpu = get_safe_dispatch_cpu(p, target_cpu);
    u8 boost = tctx ? tctx->boost_shift : BOOST_BACKGROUND;
    u64 deadline;
    
    /* Calculate deadline with slice_lag cap and boost */
    deadline = task_deadline(p, tctx, vtime_now);
    
    /* Deadline-ordered insertion to SHARED_DSQ */
    scx_bpf_dsq_insert_vtime(p, SHARED_DSQ, slice, deadline, enq_flags);
    STAT_INC(nr_shared_dispatch);
    
    /* Kick the target CPU to wake it up */
    if (safe_cpu >= 0) {
        kick_after_dispatch(safe_cpu, boost);
    }
}

#endif /* __CORE_DISPATCH_BPF_H */
