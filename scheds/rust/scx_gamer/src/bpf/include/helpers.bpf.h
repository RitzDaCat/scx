/* SPDX-License-Identifier: GPL-2.0 */
/*
 * helpers.bpf.h - Shared utility functions and macros
 *
 * Provides:
 * - Context lookup helpers (task_ctx, cpu_ctx)
 * - Statistics helpers (STAT_INC, get_stats)
 * - Time and slice calculations
 * - Common inline functions used across modules
 *
 * All functions are static __always_inline for BPF verifier compatibility.
 */

#ifndef __HELPERS_BPF_H
#define __HELPERS_BPF_H

#include "types.bpf.h"

/* Forward declaration for no_stats tunable */
extern const volatile bool no_stats;

/* ============================================================================
 * SECTION 1: CONTEXT LOOKUP HELPERS
 * ============================================================================ */

/*
 * Lookup task context from task storage.
 * Returns NULL if task is not tracked yet.
 */
static __always_inline struct task_ctx *lookup_task_ctx(struct task_struct *p)
{
    return bpf_task_storage_get(&task_ctxs, p, 0, 0);
}

/*
 * Lookup or create task context.
 * Creates new context with default values if not exists.
 */
static __always_inline struct task_ctx *lookup_create_task_ctx(struct task_struct *p)
{
    return bpf_task_storage_get(&task_ctxs, p, 0, BPF_LOCAL_STORAGE_GET_F_CREATE);
}

/*
 * Lookup CPU context by CPU ID.
 * Returns NULL if CPU ID is invalid.
 */
static __always_inline struct cpu_ctx *lookup_cpu_ctx(s32 cpu)
{
    if (cpu < 0 || cpu >= MAX_CPUS)
        return NULL;
    
    u32 key = (u32)cpu;
    return bpf_map_lookup_elem(&cpu_ctxs, &key);
}

/* ============================================================================
 * SECTION 2: STATISTICS HELPERS
 * ============================================================================ */

/*
 * Get pointer to global statistics.
 * Returns NULL if stats disabled or lookup fails.
 */
static __always_inline struct gamer_stats *get_stats(void)
{
    if (no_stats)
        return NULL;
    
    u32 key = 0;
    return bpf_map_lookup_elem(&stats_map, &key);
}

/*
 * Increment a statistics counter.
 * No-op if stats disabled.
 *
 * Usage: STAT_INC(nr_enqueued);
 */
#define STAT_INC(field) \
    do { \
        if (!no_stats) { \
            struct gamer_stats *__stats = get_stats(); \
            if (__stats) \
                __sync_fetch_and_add(&__stats->field, 1); \
        } \
    } while (0)

/*
 * Add value to a statistics counter.
 * No-op if stats disabled.
 *
 * Usage: STAT_ADD(sum_runtime_ns, delta);
 */
#define STAT_ADD(field, val) \
    do { \
        if (!no_stats) { \
            struct gamer_stats *__stats = get_stats(); \
            if (__stats) \
                __sync_fetch_and_add(&__stats->field, (val)); \
        } \
    } while (0)

/*
 * Set maximum value (atomic compare-and-swap pattern).
 * Used for tracking max_wait_ns, etc.
 */
#define STAT_MAX(field, val) \
    do { \
        if (!no_stats) { \
            struct gamer_stats *__stats = get_stats(); \
            if (__stats && (val) > __stats->field) \
                __stats->field = (val); \
        } \
    } while (0)

/* ============================================================================
 * SECTION 3: TIME AND SLICE CALCULATIONS
 * ============================================================================ */

/* Forward declaration for slice_ns tunable */
extern const volatile u64 slice_ns;

/*
 * Calculate time slice for a task based on its classification.
 * Input handlers get shorter slices (yield quickly).
 * Background tasks get longer slices (less overhead).
 */
static __always_inline u64 task_slice(struct task_ctx *tctx)
{
    if (!tctx)
        return slice_ns;
    
    /* Input handlers: ultra-short slice, yield immediately after processing */
    if (tctx->flags & FLAG_INPUT)
        return slice_ns / SLICE_INPUT_DIVISOR;
    
    /* Game/GPU/Audio: standard slice */
    if (tctx->flags & FLAGS_LATENCY_CRITICAL)
        return slice_ns;
    
    /* Background: longer slice for efficiency */
    return slice_ns * SLICE_BACKGROUND_MULT;
}

/*
 * Calculate virtual deadline for task ordering (basic formula).
 *
 * Formula: deadline = vtime + (runtime >> boost_shift)
 *
 * Higher boost_shift = smaller deadline increment = runs sooner
 * This is EEVDF-inspired but with hook-based boost instead of behavior.
 */
static __always_inline u64 calculate_deadline(u64 vtime, u64 runtime, u8 boost_shift)
{
    /* Clamp boost to valid range */
    if (boost_shift > BOOST_MAX)
        boost_shift = BOOST_MAX;
    
    /* deadline = vtime + (runtime >> boost_shift) */
    return vtime + (runtime >> boost_shift);
}

/*
 * Maximum deadline increment (priority spread between task types).
 *
 * This determines the maximum priority difference between task types:
 * - INPUT task: deadline = now + 78µs (128x reduction)
 * - BG task: deadline = now + 5ms (no reduction, capped)
 *
 * 5ms cap ensures background tasks run within 5ms of arriving,
 * regardless of how many high-priority tasks are in the queue.
 */
#define MAX_DEADLINE_INCREMENT_NS (5 * 1000 * 1000)  /* 5ms cap */

/*
 * Calculate task's absolute deadline for dispatch ordering.
 *
 * v2.4 FIX: Use ABSOLUTE TIME instead of virtual time!
 *
 * Previous approach (vtime-based) had a fatal flaw:
 * - All waking tasks got dsq_vtime capped to vtime_now - 20ms
 * - This meant ALL tasks had the same base vtime
 * - Priority was ONLY determined by exec_cost (boost)
 * - High-priority tasks arriving LATER could beat old background tasks
 * - Result: 4+ second starvation for background tasks!
 *
 * New approach (absolute time):
 * - Deadline = current_time + exec_cost
 * - Tasks that arrived EARLIER have LOWER deadlines (run first)
 * - A background task at T=0 with deadline=5ms runs BEFORE
 *   an input task at T=1s with deadline=1s+78µs
 * - Guarantees NO task waits more than ~5ms after any LATER task
 *
 * Priority still works:
 * - INPUT at T=0: deadline = 0 + 78µs = 78µs
 * - BG at T=0: deadline = 0 + 5ms = 5ms
 * - INPUT runs first (78µs < 5ms) ✓
 *
 * But fairness is preserved:
 * - BG at T=0: deadline = 5ms
 * - INPUT at T=10ms: deadline = 10ms + 78µs = 10.078ms
 * - BG runs first (5ms < 10.078ms) ✓
 */
static __always_inline u64 task_deadline(
    struct task_struct *p,
    struct task_ctx *tctx,
    u64 vtime_now)  /* Note: vtime_now parameter kept for API compat, but ignored */
{
    u64 now = bpf_ktime_get_ns();
    u64 exec_cost;
    u8 boost;
    
    if (!tctx)
        return now + MAX_DEADLINE_INCREMENT_NS;
    
    /* Get boost (clamp to valid range) */
    boost = tctx->boost_shift;
    if (boost > BOOST_MAX)
        boost = BOOST_MAX;
    
    /* Apply boost to exec_runtime: higher boost = smaller deadline increment */
    exec_cost = tctx->exec_runtime >> boost;
    
    /* Cap deadline increment to prevent excessive spread */
    if (exec_cost > MAX_DEADLINE_INCREMENT_NS)
        exec_cost = MAX_DEADLINE_INCREMENT_NS;
    
    /*
     * Deadline = absolute time + priority-weighted cost.
     *
     * Lower deadline = higher priority = runs sooner.
     * Tasks that arrived earlier naturally have lower deadlines.
     */
    return now + exec_cost;
}

/* time_before() is provided by scx/common.bpf.h */

/* ============================================================================
 * SECTION 4: BOOST CALCULATION
 * ============================================================================ */

/* Forward declaration for foreground_tgid tunable */
extern const volatile u32 foreground_tgid;

/*
 * Calculate boost_shift from task flags.
 * Called when task is classified or reclassified.
 *
 * Priority order: input > gpu > audio > compositor > game > foreground > background
 */
static __always_inline u8 calculate_boost_shift(struct task_ctx *tctx)
{
    if (!tctx)
        return BOOST_BACKGROUND;
    
    /* Check flags in priority order (highest first) */
    if (tctx->flags & FLAG_INPUT)
        return BOOST_INPUT;
    
    if (tctx->flags & FLAG_GPU)
        return BOOST_GPU;
    
    if (tctx->flags & FLAG_AUDIO)
        return BOOST_AUDIO;
    
    if (tctx->flags & FLAG_COMPOSITOR)
        return BOOST_COMPOSITOR;
    
    /* Game thread detection */
    if (tctx->flags & FLAG_GAME) {
        /* Main thread (pid == tgid) gets higher boost */
        /* Note: This is a simplification; proper check needs task_struct */
        return BOOST_GAME_WORKER;
    }
    
    /* Foreground app (non-game) */
    if (foreground_tgid && tctx->tgid == foreground_tgid)
        return BOOST_FOREGROUND;
    
    return BOOST_BACKGROUND;
}

/*
 * Update task's boost_shift and record in histogram.
 */
static __always_inline void update_task_boost(struct task_ctx *tctx)
{
    if (!tctx)
        return;
    
    u8 new_boost = calculate_boost_shift(tctx);
    tctx->boost_shift = new_boost;
    
    /* Update histogram */
    if (new_boost <= BOOST_MAX)
        STAT_INC(boost_histogram[new_boost]);
}

/* ============================================================================
 * SECTION 5: DEBUG EVENT LOGGING
 * ============================================================================ */

/* Forward declaration for debug tunable */
extern const volatile bool debug;

/*
 * Emit a debug event to the ring buffer.
 * Only emits events when debug=true.
 *
 * Usage: DEBUG_EVENT(EVENT_ENQUEUE, p, tctx, cpu, -1);
 */
static __always_inline void emit_debug_event(
    u32 event_type,
    struct task_struct *p,
    struct task_ctx *tctx,
    s32 cpu_from,
    s32 cpu_to)
{
    if (!debug)
        return;
    
    struct debug_event *evt;
    evt = bpf_ringbuf_reserve(&debug_events, sizeof(*evt), 0);
    if (!evt)
        return;
    
    evt->timestamp_ns = bpf_ktime_get_ns();
    evt->event_type = event_type;
    evt->pid = p ? p->pid : 0;
    evt->tgid = p ? p->tgid : 0;
    evt->boost_old = tctx ? tctx->boost_shift : 0;
    evt->boost_new = tctx ? tctx->boost_shift : 0;
    evt->flags = tctx ? tctx->flags : 0;
    evt->cpu_from = cpu_from;
    evt->cpu_to = cpu_to;
    
    /*
     * For wait-related events (STARVATION, LONG_WAIT), use last_woke_at 
     * (time became runnable) to show actual queue wait time.
     * For other events, use last_run_ns (time last ran).
     */
    if ((event_type == EVENT_STARVATION || event_type == EVENT_LONG_WAIT) 
        && tctx && tctx->last_woke_at > 0) {
        evt->wait_ns = evt->timestamp_ns - tctx->last_woke_at;
    } else {
        evt->wait_ns = tctx ? (evt->timestamp_ns - tctx->last_run_ns) : 0;
    }
    evt->runtime_ns = tctx ? tctx->sum_runtime_ns : 0;
    
    /* Copy task name (comm) */
    if (p) {
        bpf_probe_read_kernel_str(evt->comm, sizeof(evt->comm), p->comm);
    } else {
        evt->comm[0] = '\0';
    }
    
    bpf_ringbuf_submit(evt, 0);
}

/*
 * Emit boost change event (includes old and new boost).
 */
static __always_inline void emit_boost_change_event(
    struct task_struct *p,
    struct task_ctx *tctx,
    u8 old_boost,
    u8 new_boost)
{
    if (!debug)
        return;
    
    struct debug_event *evt;
    evt = bpf_ringbuf_reserve(&debug_events, sizeof(*evt), 0);
    if (!evt)
        return;
    
    evt->timestamp_ns = bpf_ktime_get_ns();
    evt->event_type = EVENT_BOOST_CHANGE;
    evt->pid = p ? p->pid : 0;
    evt->tgid = p ? p->tgid : 0;
    evt->boost_old = old_boost;
    evt->boost_new = new_boost;
    evt->flags = tctx ? tctx->flags : 0;
    evt->cpu_from = -1;
    evt->cpu_to = -1;
    evt->wait_ns = 0;
    evt->runtime_ns = tctx ? tctx->sum_runtime_ns : 0;
    
    if (p) {
        bpf_probe_read_kernel_str(evt->comm, sizeof(evt->comm), p->comm);
    } else {
        evt->comm[0] = '\0';
    }
    
    bpf_ringbuf_submit(evt, 0);
}

/*
 * Update wait time histogram.
 * Buckets: <1us, 1-10us, 10-100us, 100us-1ms, 1-10ms, 10-100ms, 100ms-1s, 1-3s, 3-5s, 5-10s, >10s
 *
 * Extended buckets for starvation debugging:
 * - 1-3s, 3-5s, 5-10s, >10s help diagnose severe scheduling issues
 */
static __always_inline void update_wait_histogram(u64 wait_ns)
{
    u32 bucket;
    
    if (wait_ns < 1000)                    bucket = 0;   /* <1us */
    else if (wait_ns < 10000)              bucket = 1;   /* 1-10us */
    else if (wait_ns < 100000)             bucket = 2;   /* 10-100us */
    else if (wait_ns < 1000000)            bucket = 3;   /* 100us-1ms */
    else if (wait_ns < 10000000)           bucket = 4;   /* 1-10ms */
    else if (wait_ns < 100000000)          bucket = 5;   /* 10-100ms */
    else if (wait_ns < 1000000000ULL)      bucket = 6;   /* 100ms-1s */
    else if (wait_ns < 3000000000ULL)      bucket = 7;   /* 1-3s */
    else if (wait_ns < 5000000000ULL)      bucket = 8;   /* 3-5s */
    else if (wait_ns < 10000000000ULL)     bucket = 9;   /* 5-10s */
    else                                   bucket = 10;  /* >10s */
    
    STAT_INC(wait_histogram[bucket]);
    STAT_ADD(total_wait_ns, wait_ns);
}

#endif /* __HELPERS_BPF_H */

