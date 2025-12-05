/* SPDX-License-Identifier: GPL-2.0 */
/*
 * boost.bpf.h - Priority boost system
 *
 * Implements the boost_shift priority system:
 * - boost_shift 0-7 maps to 2^N priority multiplier
 * - Higher boost = earlier deadline = runs sooner
 *
 * Key functions:
 * - should_preempt(): Compare priorities to decide if preemption is warranted
 * - smart_kick_cpu(): Priority-aware preemption (avoid unnecessary IPIs)
 * - apply_boost_to_deadline(): Integrate boost into deadline calculation
 *
 * Core calculation functions (calculate_boost_shift, calculate_deadline)
 * are in helpers.bpf.h for use across modules.
 */

#ifndef __PRIORITY_BOOST_BPF_H
#define __PRIORITY_BOOST_BPF_H

#include "../helpers.bpf.h"

/* ============================================================================
 * SECTION 1: PRIORITY COMPARISON
 * ============================================================================ */

/*
 * Compare two boost levels.
 * Returns true if boost_a has HIGHER priority than boost_b.
 *
 * Higher boost_shift = higher priority = should preempt lower.
 */
static __always_inline bool boost_is_higher(u8 boost_a, u8 boost_b)
{
    return boost_a > boost_b;
}

/*
 * Check if a waking task should preempt the currently running task.
 *
 * We preempt if:
 * 1. Waking task has strictly higher boost than running task
 * 2. Running task is background (boost 0) and waking is not
 *
 * We do NOT preempt if:
 * 1. Boosts are equal (let running task finish its slice)
 * 2. Running task has higher boost (it's more important)
 *
 * This reduces unnecessary IPIs and context switches.
 */
static __always_inline bool should_preempt(u8 waking_boost, u8 running_boost)
{
    /* Always preempt background tasks for any priority work */
    if (running_boost == BOOST_BACKGROUND && waking_boost > BOOST_BACKGROUND)
        return true;
    
    /* Only preempt if waking task has strictly higher priority */
    return waking_boost > running_boost;
}

/* ============================================================================
 * SECTION 2: SMART KICK (Priority-Aware Preemption)
 * ============================================================================ */

/*
 * Kick a CPU with priority awareness.
 *
 * Instead of always using SCX_KICK_PREEMPT (which sends an IPI and forces
 * immediate context switch), we check if the waking task actually has
 * higher priority than the currently running task.
 *
 * Benefits:
 * - Reduces IPI storms (expensive cross-CPU interrupts)
 * - Reduces unnecessary context switches
 * - Improves cache locality (tasks finish their work)
 * - Reduces frametime variance
 *
 * The old v1 code used SCX_KICK_PREEMPT in 9+ places unconditionally,
 * causing 4.5x more frames >5ms than scx_cosmos.
 */
static __always_inline void smart_kick_cpu(s32 cpu, u8 waking_boost)
{
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (!cctx)
        return;
    
    u8 running_boost = cctx->current_boost;
    
    if (should_preempt(waking_boost, running_boost)) {
        /* Waking task has higher priority - preempt immediately */
        scx_bpf_kick_cpu(cpu, SCX_KICK_PREEMPT);
        STAT_INC(nr_preempt_kick);
    } else {
        /* Equal or lower priority - just signal, no forced preemption */
        scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
        STAT_INC(nr_preempt_avoided);
    }
}

/*
 * Kick CPU for latency-critical task (always preempt).
 * Use this for input handlers where we KNOW we need immediate execution.
 */
static __always_inline void kick_for_latency_critical(s32 cpu)
{
    scx_bpf_kick_cpu(cpu, SCX_KICK_PREEMPT);
    STAT_INC(nr_preempt_kick);
}

/* ============================================================================
 * SECTION 3: DEADLINE INTEGRATION
 * ============================================================================ */

/*
 * Apply boost to deadline calculation.
 *
 * The deadline determines task ordering in the scheduler.
 * Earlier deadline = runs sooner.
 *
 * Formula: deadline = vtime + (slice >> boost_shift)
 *
 * With boost_shift = 7 (input), a 10µs slice becomes:
 *   deadline_increment = 10000 >> 7 = 78ns
 *
 * With boost_shift = 0 (background), same slice:
 *   deadline_increment = 10000 >> 0 = 10000ns
 *
 * So input handler gets 128x earlier deadline than background task.
 */
static __always_inline u64 apply_boost_to_deadline(
    struct task_ctx *tctx,
    u64 vtime,
    u64 slice)
{
    if (!tctx)
        return vtime + slice;
    
    return calculate_deadline(vtime, slice, tctx->boost_shift);
}

/* ============================================================================
 * SECTION 4: BOOST CLASSIFICATION (Extended)
 * ============================================================================ */

/*
 * Check if task is latency-critical based on current flags.
 * Used for fast-path decisions.
 */
static __always_inline bool task_is_latency_critical(struct task_ctx *tctx)
{
    if (!tctx)
        return false;
    
    return (tctx->flags & FLAGS_LATENCY_CRITICAL) != 0;
}

/*
 * Check if task belongs to the foreground game.
 */
static __always_inline bool task_is_foreground_game(struct task_ctx *tctx)
{
    if (!tctx)
        return false;
    
    /* Check if it has any game-related flag */
    if (tctx->flags & FLAGS_GAME_RELATED)
        return true;
    
    /* Check if it belongs to the foreground tgid */
    if (foreground_tgid && tctx->tgid == foreground_tgid)
        return true;
    
    return false;
}

/*
 * Get a human-readable priority tier for debugging.
 * Returns 0-3 where 0 is highest priority.
 */
static __always_inline u8 get_priority_tier(u8 boost_shift)
{
    if (boost_shift >= BOOST_GPU)       /* 6-7: Input, GPU */
        return 0;  /* Ultra-high priority */
    if (boost_shift >= BOOST_COMPOSITOR) /* 4-5: Audio, Compositor */
        return 1;  /* High priority */
    if (boost_shift >= BOOST_FOREGROUND) /* 1-3: Game, Foreground */
        return 2;  /* Medium priority */
    return 3;      /* 0: Background */
}

/* ============================================================================
 * SECTION 5: STARVATION PREVENTION
 * ============================================================================ */

/*
 * Maximum time a task can wait before being rescued from starvation.
 * 
 * v2.5: Lowered from 100ms to 20ms for faster intervention.
 * 
 * Why 20ms:
 * - 100ms allowed 19ms+ outliers without intervention
 * - 20ms catches outliers before they become noticeable
 * - ~1.2 frames at 60Hz, ~4.8 frames at 240Hz
 * - Matches MAX_DEADLINE_INCREMENT_NS for consistency
 */
#define STARVATION_THRESH_NS (20ULL * 1000 * 1000)  /* 20ms */

/*
 * Check if a task has been starved and needs rescue.
 * Returns true if task has waited too long since becoming runnable.
 *
 * CRITICAL FIX: Now uses last_woke_at (time became runnable) instead of
 * last_run_ns (time last ran). This catches tasks starving in the DSQ,
 * not just tasks that slept too long.
 */
static __always_inline bool task_is_starved(struct task_ctx *tctx, u64 now)
{
    if (!tctx)
        return false;
    
    /* Check time since task became runnable (woke up) */
    if (tctx->last_woke_at == 0)
        return false;
    
    u64 wait_time = now - tctx->last_woke_at;
    
    if (wait_time > STARVATION_THRESH_NS) {
        STAT_INC(nr_starvation_rescues);
        return true;
    }
    
    return false;
}

/*
 * Get rescue boost for starved task.
 * 
 * CRITICAL FIX: Boost to HIGHEST priority (INPUT level) to ensure
 * the task actually gets scheduled. Previous boost to GAME_WORKER (level 2)
 * was ineffective when all game tasks were at INPUT (level 7) - the
 * starved task would still have a later deadline than everything else.
 */
static __always_inline u8 get_starvation_rescue_boost(struct task_ctx *tctx)
{
    /*
     * Boost to INPUT level (highest priority).
     * This ensures starved tasks get the earliest possible deadline.
     */
    return BOOST_INPUT;
}

#endif /* __PRIORITY_BOOST_BPF_H */
