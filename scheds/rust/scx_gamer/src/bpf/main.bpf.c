/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer - Gaming-optimized sched_ext scheduler
 *
 * A scheduler designed for competitive gaming with:
 * - 100% hook-based thread classification (no heuristics)
 * - Priority boost via boost_shift (0-7 levels, 2^N multiplier)
 * - A.B.C. (Always Be Casting) - proactive CPU preparation
 * - Physical core preference for latency-critical tasks
 *
 * File structure:
 * - main.bpf.c: struct_ops callbacks only (this file)
 * - include/config.bpf.h: Constants and tunables
 * - include/types.bpf.h: Data structures and maps
 * - include/helpers.bpf.h: Utility functions
 * - include/priority/boost.bpf.h: Priority system
 * - include/detection/: Fentry hooks for thread classification
 * - include/core/: CPU selection, enqueue, dispatch
 */

#include <scx/common.bpf.h>
#include "intf.h"

/* === LAYER 3: Foundation === */
#include "include/config.bpf.h"
#include "include/types.bpf.h"
#include "include/helpers.bpf.h"

/* === LAYER 5: Priority System === */
#include "include/priority/boost.bpf.h"

/* === LAYER 6: Detection Hooks === */
#include "include/detection/input.bpf.h"
#include "include/detection/gpu.bpf.h"
#include "include/detection/audio.bpf.h"
#include "include/detection/sync.bpf.h"

/* === LAYER 7: Core Scheduling === */
#include "include/core/cpu_select.bpf.h"
#include "include/core/dispatch.bpf.h"

/* === Tunables (set from userspace before load) === */
const volatile u64 slice_ns = SLICE_NS_DEFAULT;
const volatile bool avoid_smt = true;
const volatile bool no_stats = false;
const volatile u32 foreground_tgid = 0;
const volatile bool debug = false;

/* === Global State === */
u64 vtime_now;

char _license[] SEC("license") = "GPL";

/* ============================================================================
 * STRUCT_OPS CALLBACKS
 * ============================================================================
 *
 * These are the core scheduler callbacks called by the kernel.
 * Keep this file minimal - delegate to helpers and modules.
 */

/*
 * select_cpu - Choose a CPU for a waking task (with DIRECT DISPATCH)
 *
 * This is the PRIMARY dispatch path for gaming (~70-90% of tasks).
 * When an idle CPU is found, we dispatch directly here, skipping
 * the enqueue() callback entirely for lowest latency.
 *
 * Selection priority:
 * 1. Previous CPU if idle (best cache locality)
 * 2. Physical core for latency-critical tasks (avoid SMT)
 * 3. Any idle CPU
 * 4. Fallback to previous CPU (will go through enqueue)
 *
 * Returns: Selected CPU
 */
s32 BPF_STRUCT_OPS(gamer_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    s32 cpu;
    u64 slice;
    
    /*
     * Strategy 1: Prefer prev_cpu for cache locality.
     * Try to claim prev_cpu FIRST before looking for other CPUs.
     * This dramatically improves cache hit rates and reduces migrations.
     *
     * NOTE: Also verify prev_cpu is still allowed. The kernel typically
     * validates this, but Wine/Proton games can change thread affinities
     * so rapidly that a race can occur.
     */
    if (bpf_cpumask_test_cpu(prev_cpu, p->cpus_ptr) &&
        scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
        slice = task_slice(tctx);
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | prev_cpu, slice, 0);
        
        STAT_INC(nr_direct_dispatch);
        STAT_INC(nr_same_cpu);
        track_cpu_selection(tctx, prev_cpu, prev_cpu);
        return prev_cpu;
    }
    
    /*
     * Strategy 2: Find another idle CPU.
     *
     * IMPORTANT: scx_bpf_pick_idle_cpu() both FINDS and CLAIMS the CPU
     * (clears the idle bit). So if select_cpu_for_task returns a CPU
     * different from prev_cpu, that CPU has been claimed for us.
     */
    cpu = select_cpu_for_task(p, tctx, prev_cpu, wake_flags);
    
    /*
     * DIRECT DISPATCH: If we got an idle CPU (not prev_cpu fallback),
     * it's already claimed by pick_idle_cpu. Dispatch directly!
     *
     * CRITICAL: Verify CPU is still in task's allowed cpumask!
     * Wine/Proton games (especially with ntsync) aggressively modify
     * thread affinities. Between pick_idle_cpu() and dsq_insert(),
     * the affinity can change, causing:
     *   "SCX_DSQ_LOCAL[_ON] target CPU X not allowed for task"
     *
     * If affinity changed, fall through to enqueue() which handles this.
     */
    if (cpu >= 0 && cpu != prev_cpu) {
        /* Verify CPU is still allowed (affinity may have changed) */
        if (!bpf_cpumask_test_cpu(cpu, p->cpus_ptr)) {
            /* Affinity changed - CPU no longer allowed, fallback to enqueue */
            STAT_INC(nr_affinity_failures);
            track_cpu_selection(tctx, prev_cpu, prev_cpu);
            return prev_cpu;
        }
        
        slice = task_slice(tctx);
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, slice, 0);
        
        STAT_INC(nr_direct_dispatch);
        track_cpu_selection(tctx, prev_cpu, cpu);
        return cpu;
    }
    
    /* No idle CPU available - fall through to enqueue() */
    track_cpu_selection(tctx, prev_cpu, prev_cpu);
    return prev_cpu;
}

/*
 * runnable - Task is becoming runnable (waking up)
 *
 * Called when a task wakes from sleep. This is the key place to:
 * 1. Reset exec_runtime (runtime since last sleep)
 * 2. Track wakeup timestamp for frequency calculation
 *
 * exec_runtime reset is critical for deadline calculation:
 * - Tasks that just woke have exec_runtime=0 → earlier deadline
 * - This prioritizes short-burst interactive tasks over CPU hogs
 */
void BPF_STRUCT_OPS(gamer_runnable, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    u64 now = bpf_ktime_get_ns();
    
    if (!tctx)
        return;
    
    /* Reset exec_runtime - task just woke, hasn't run yet this burst */
    tctx->exec_runtime = 0;
    
    /* Track wakeup timestamp */
    tctx->last_woke_at = now;
    
    STAT_INC(nr_wakeups);
}

/*
 * enqueue - FALLBACK dispatch path
 *
 * This is only called when select_cpu could NOT find an idle CPU
 * and direct dispatch was not possible. This handles ~10-30% of tasks.
 *
 * Two fallback paths:
 * 1. MEDIUM PATH: Latency-critical task → Local DSQ + preemptive kick
 * 2. SLOW PATH: Background task → Shared DSQ with deadline ordering
 *
 * Input/Sync Window Boost:
 * Game tasks waking during input/sync windows get temporary priority boost.
 */
void BPF_STRUCT_OPS(gamer_enqueue, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    u64 slice;
    u64 now = bpf_ktime_get_ns();
    s32 cpu;
    u8 boost;
    bool is_critical = false;
    
    STAT_INC(nr_enqueued);
    
    /* Calculate slice based on task type */
    slice = task_slice(tctx);
    
    /* Get current boost level */
    boost = tctx ? tctx->boost_shift : BOOST_BACKGROUND;
    
    /*
     * NOTE: Wait time is now measured in running() callback for accuracy.
     * Measuring here (at enqueue) misses the DSQ wait time - the time
     * between being enqueued and actually starting to run.
     */
    
    if (tctx) {
        /* Check if task is latency-critical */
        is_critical = task_is_latency_critical(tctx);
        
        /* Input/Sync Window Boost for game tasks */
        if (!is_critical && task_is_foreground_game(tctx)) {
            if (in_input_window(now)) {
                if (boost < BOOST_GAME_MAIN)
                    boost = BOOST_GAME_MAIN;
                tctx->boost_shift = boost;
                is_critical = true;  /* Treat as critical */
                STAT_INC(nr_input_window_boosts);
            } else if (in_sync_window(now)) {
                if (boost < BOOST_GAME_MAIN)
                    boost = BOOST_GAME_MAIN;
                tctx->boost_shift = boost;
                is_critical = true;
                STAT_INC(nr_sync_window_boosts);
            }
        }
        
        /* Starvation rescue */
        if (task_is_starved(tctx, now)) {
            boost = get_starvation_rescue_boost(tctx);
            tctx->boost_shift = boost;
            is_critical = true;
            emit_debug_event(EVENT_STARVATION, p, tctx, tctx->last_cpu, -1);
        }
    }
    
    /* Update global vtime */
    vtime_now += slice >> boost;
    
    /* Get the CPU this task will run on */
    cpu = scx_bpf_task_cpu(p);
    
    /*
     * FALLBACK DISPATCH DECISION:
     * Since we're here, no idle CPU was found. Choose path based on criticality.
     */
    /*
     * ALL tasks go to SHARED_DSQ with deadline ordering.
     * 
     * CRITICAL FIX: Using local DSQ (SCX_DSQ_LOCAL_ON) for critical tasks
     * was causing starvation! CPU-pinned tasks (kworkers, ksoftirqd) sit
     * in SHARED_DSQ, but if local DSQ always has work, dispatch() is
     * never called to serve them.
     *
     * Solution: Use SHARED_DSQ for everything. Critical tasks get early
     * deadlines and a CPU kick to ensure timely execution.
     */
    u64 deadline = task_deadline(p, tctx, vtime_now);
    scx_bpf_dsq_insert_vtime(p, SHARED_DSQ, slice, deadline, enq_flags);
    STAT_INC(nr_shared_dispatch);
    
    if (is_critical) {
        /* Kick the CPU to ensure timely pickup of critical task */
        smart_kick_cpu(cpu, boost);
    }
}

/*
 * dispatch - Move tasks from DSQ to CPU
 *
 * Called when a CPU needs work. Consume from DSQs in priority order.
 */
void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    /*
     * Local DSQ is automatically consumed by the kernel.
     * We only need to consume from our shared DSQ here.
     *
     * Note: SCX_DSQ_LOCAL_ON is for INSERTING tasks, not consuming.
     * For consuming, we use the actual DSQ ID (SHARED_DSQ = 0).
     */
    scx_bpf_dsq_move_to_local(SHARED_DSQ);
}

/*
 * running - Task is starting to run
 *
 * Update CPU context with currently running task's boost level.
 * This is used by smart_kick_cpu() to decide if preemption is warranted.
 */
void BPF_STRUCT_OPS(gamer_running, struct task_struct *p)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    struct cpu_ctx *cctx = lookup_cpu_ctx(scx_bpf_task_cpu(p));
    u64 now = bpf_ktime_get_ns();
    u8 boost = BOOST_BACKGROUND;
    
    if (tctx) {
        /*
         * ACCURATE WAIT TIME: Measure from wakeup (runnable) to now (running).
         *
         * This captures TOTAL scheduling latency including:
         * 1. Time in select_cpu() and enqueue()
         * 2. Time waiting in DSQ for CPU to pick us up
         *
         * Previously measured at enqueue() which missed DSQ wait time.
         * Tasks could wait 10+ seconds in DSQ without being counted!
         */
        if (tctx->last_woke_at > 0) {
            u64 wait_ns = now - tctx->last_woke_at;
            update_wait_histogram(wait_ns);
            
            /* Track max wait for health monitoring */
            STAT_MAX(max_wait_ns, wait_ns);
            
            /*
             * LONG WAIT DEBUG: Log tasks that waited >10ms.
             * This catches outliers on BOTH dispatch paths:
             * - Direct dispatch (99.6%) - no starvation check!
             * - Enqueue fallback (0.4%) - has starvation check
             *
             * Helps identify WHICH tasks are hitting 15-20ms waits.
             */
            if (wait_ns > LONG_WAIT_THRESH_NS) {
                emit_debug_event(EVENT_LONG_WAIT, p, tctx, 
                                 scx_bpf_task_cpu(p), -1);
            }
        }
        
        /* Track when task started running */
        tctx->last_run_ns = now;
        
        /*
         * BOOST DECAY: Check if boost has expired.
         *
         * Without this, boosts are permanent and cause starvation.
         * A task classified as INPUT once would keep INPUT priority forever.
         *
         * Decay rules:
         * - If classified_at_ns is 0 or too old, reset to BACKGROUND
         * - Only decay if boost is above BACKGROUND
         */
        boost = tctx->boost_shift;
        
        if (boost > BOOST_BACKGROUND) {
            u64 classified_at = tctx->classified_at_ns;
            
            /* Check if boost has expired (5ms since last classification) */
            if (classified_at == 0 || (now - classified_at) > BOOST_DECAY_NS) {
                /* Boost expired - decay to background */
                tctx->boost_shift = BOOST_BACKGROUND;
                tctx->flags &= ~FLAGS_LATENCY_CRITICAL;  /* Clear latency flags too */
                boost = BOOST_BACKGROUND;
            }
        }
        
        /* Update boost histogram (bounded access) */
        if (boost <= BOOST_MAX)
            STAT_INC(boost_histogram[boost]);
    }
    
    if (cctx) {
        /* Record current task's boost for preemption decisions */
        cctx->current_boost = boost;
    }
    
    STAT_INC(nr_dispatched);
}

/*
 * stopping - Task is stopping (yielding CPU)
 *
 * Update runtime accounting for deadline calculation and fairness.
 * This is where we:
 * 1. Calculate how long the task ran this burst
 * 2. Accumulate exec_runtime (capped at SLICE_LAG_NS)
 * 3. Update task's vtime for fairness tracking
 * 4. Reset last_woke_at for preempted tasks (v2.4 fix)
 */
void BPF_STRUCT_OPS(gamer_stopping, struct task_struct *p, bool runnable)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    struct cpu_ctx *cctx = lookup_cpu_ctx(scx_bpf_task_cpu(p));
    u64 now = bpf_ktime_get_ns();
    u64 delta = 0;
    
    if (tctx && tctx->last_run_ns > 0) {
        /* Calculate how long task ran */
        delta = now - tctx->last_run_ns;
        
        /* Cap delta to slice_ns to prevent outliers */
        if (delta > slice_ns)
            delta = slice_ns;
        
        /* Accumulate exec_runtime since last sleep (capped at SLICE_LAG_NS)
         * This is used for deadline calculation - higher exec_runtime = later deadline */
        tctx->exec_runtime += delta;
        if (tctx->exec_runtime > SLICE_LAG_NS)
            tctx->exec_runtime = SLICE_LAG_NS;
        
        /* Track cumulative runtime for statistics */
        tctx->sum_runtime_ns += delta;
        
        /* Update task's vtime for fairness tracking
         * vtime advances by runtime, scaled by weight (we use boost as proxy)
         * Higher boost = less vtime charge = can run more before falling behind */
        u8 boost = tctx->boost_shift;
        if (boost > BOOST_MAX)
            boost = BOOST_MAX;
        
        /* Advance task's vtime: vtime += delta >> boost
         * (or just delta if boost is 0) */
        p->scx.dsq_vtime += delta >> boost;
        
        /*
         * v2.4 FIX: Reset last_woke_at for preempted tasks.
         *
         * If runnable=true, task was PREEMPTED (not sleeping).
         * It will be scheduled again without going through runnable().
         * Set last_woke_at = now so wait time measurement is accurate.
         *
         * Without this fix, preempted tasks accumulate "wait time" from
         * their original wakeup, causing false >1s readings!
         *
         * Example of the bug:
         *   1. Task wakes at T=0 → last_woke_at = 0
         *   2. Task runs 100ms, gets preempted
         *   3. Task runs again at T=2s
         *   4. Wait time = 2s - 0 = 2s (WRONG! Should be ~0)
         */
        if (runnable) {
            tctx->last_woke_at = now;
        }
    }
    
    if (cctx) {
        /* Clear current boost - CPU is now available */
        cctx->current_boost = 0;
    }
}

/*
 * init_task - Initialize per-task context
 *
 * Called when a task is first seen by the scheduler.
 * Create and initialize task_ctx with default values.
 */
s32 BPF_STRUCT_OPS(gamer_init_task, struct task_struct *p,
                   struct scx_init_task_args *args)
{
    struct task_ctx *tctx;
    
    tctx = lookup_create_task_ctx(p);
    if (!tctx)
        return -ENOMEM;
    
    /* Initialize with defaults */
    tctx->flags = 0;
    tctx->boost_shift = BOOST_BACKGROUND;
    tctx->preferred_cpu = -1;
    tctx->exec_runtime = 0;       /* Runtime since last sleep */
    tctx->last_run_ns = 0;
    tctx->sum_runtime_ns = 0;
    tctx->last_cpu = -1;
    tctx->tgid = p->tgid;
    tctx->classified_at_ns = 0;
    tctx->last_woke_at = 0;
    
    /* Initialize task's vtime to current global vtime */
    p->scx.dsq_vtime = vtime_now;
    
    /* Check if task belongs to foreground game */
    if (foreground_tgid && p->tgid == foreground_tgid) {
        tctx->flags |= FLAG_GAME;
        tctx->boost_shift = BOOST_GAME_WORKER;
    }
    
    return 0;
}

/*
 * init - Initialize the scheduler
 *
 * Called once when scheduler is loaded.
 * Initialize CPU contexts and any global state.
 */
s32 BPF_STRUCT_OPS_SLEEPABLE(gamer_init)
{
    s32 cpu;
    
    /* Initialize global vtime */
    vtime_now = 0;
    
    /* Initialize CPU contexts */
    bpf_for(cpu, 0, MAX_CPUS) {
        struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
        if (cctx) {
            cctx->last_input_ns = 0;
            cctx->current_boost = 0;
            cctx->flags = 0;
            cctx->core_id = cpu;  /* Will be set properly by userspace */
            cctx->sibling_cpu = -1;
            cctx->node_id = 0;
        }
    }
    
    /* Create shared DSQ */
    return scx_bpf_create_dsq(SHARED_DSQ, -1);
}

/* User Exit Info for error reporting - must be defined before use */
UEI_DEFINE(uei);

/*
 * exit - Cleanup when scheduler unloads
 *
 * Called when scheduler is being unloaded.
 */
void BPF_STRUCT_OPS(gamer_exit, struct scx_exit_info *ei)
{
    /* Record exit info for userspace */
    UEI_RECORD(uei, ei);
}

/* ============================================================================
 * STRUCT_OPS DEFINITION
 * ============================================================================ */

SCX_OPS_DEFINE(gamer_ops,
	       .select_cpu     = (void *)gamer_select_cpu,
	       .runnable       = (void *)gamer_runnable,
	       .enqueue        = (void *)gamer_enqueue,
	       .dispatch       = (void *)gamer_dispatch,
	       .running        = (void *)gamer_running,
	       .stopping       = (void *)gamer_stopping,
	       .init_task      = (void *)gamer_init_task,
	       .init           = (void *)gamer_init,
	       .exit           = (void *)gamer_exit,
	       .name           = "gamer");
