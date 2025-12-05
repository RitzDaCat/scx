/* SPDX-License-Identifier: GPL-2.0 */
/*
 * input.bpf.h - Input device detection hooks
 *
 * Detects input handler threads via fentry hooks on:
 * - hid_irq_in(): USB HID interrupt (earliest detection point)
 * - input_event(): Kernel input subsystem events
 *
 * Sets FLAG_INPUT on task_ctx and triggers A.B.C. preemption.
 *
 * Priority: HIGHEST (boost_shift = 7, 128x priority)
 *
 * Detection flow:
 * 1. hid_irq_in fires when USB HID device sends data (mouse movement, click)
 * 2. We immediately mark the current task as input handler
 * 3. A.B.C.: Speculatively kick low-priority CPUs to prepare for game response
 * 4. input_event fires for keyboard/gamepad events through input subsystem
 */

#ifndef __DETECTION_INPUT_BPF_H
#define __DETECTION_INPUT_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"

/* ============================================================================
 * INPUT DETECTION STATE
 * ============================================================================ */

/* Timestamp of most recent input event (shared across CPUs) */
volatile u64 last_input_ns;

/* Input event window - tasks waking within this window get boosted */
#define INPUT_WINDOW_NS (2 * 1000 * 1000)  /* 2ms - covers typical input→frame chain */

/* ============================================================================
 * HELPER: Classify task as input handler
 * ============================================================================ */

static __always_inline void classify_as_input(struct task_struct *p, u64 now)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    if (!tctx)
        return;
    
    /* Set input flag and boost */
    tctx->flags |= FLAG_INPUT;
    tctx->boost_shift = BOOST_INPUT;
    tctx->classified_at_ns = now;
    
    /* NOTE: Per-hook stats tracked in fentry callbacks, not here */
}

/* ============================================================================
 * HELPER: A.B.C. - Always Be Casting
 * ============================================================================ 
 *
 * When we detect input, proactively kick CPUs running low-priority tasks
 * to prepare them for the imminent high-priority game work.
 *
 * This reduces input→frame latency by ~0.5-2ms because:
 * 1. Game threads wake immediately after input processing
 * 2. If CPUs are busy with background work, they'd need to finish their slice
 * 3. By kicking early, CPU is ready when game thread wakes
 */

static __always_inline void abc_speculative_kick(u64 now)
{
    s32 cpu = bpf_get_smp_processor_id();
    
    /* Update global timestamp for input window tracking */
    last_input_ns = now;
    
    /* Mark CPU as having recent input */
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (cctx)
        cctx->last_input_ns = now;
    
    /* 
     * A.B.C.: Kick other CPUs running background tasks
     * This is speculative - we're betting game threads will wake soon
     * and those CPUs should be ready.
     *
     * For v2.0, we do a simple single-CPU kick of our sibling if available.
     * This handles the common case of game thread on sibling core.
     */
    if (cctx && cctx->sibling_cpu >= 0) {
        struct cpu_ctx *sibling_cctx = lookup_cpu_ctx(cctx->sibling_cpu);
        if (sibling_cctx && sibling_cctx->current_boost < BOOST_GAME_WORKER) {
            scx_bpf_kick_cpu(cctx->sibling_cpu, SCX_KICK_IDLE);
        }
    }
}

/* ============================================================================
 * FENTRY HOOKS
 * ============================================================================ */

/*
 * HID URB completion - earliest possible USB input detection
 *
 * This fires when a USB HID device (mouse, keyboard) completes a URB.
 * It's the earliest point we can detect input before any processing.
 *
 * The current task is typically a kernel worker or interrupt handler,
 * but we use this to trigger A.B.C. preparation.
 */
SEC("fentry/hid_irq_in")
int BPF_PROG(detect_hid_irq_in, void *urb)
{
    u64 now = bpf_ktime_get_ns();
    
    /* Track per-hook stats */
    STAT_INC(nr_hid_irq_in);
    STAT_INC(nr_input_detected);
    
    /* Trigger A.B.C. speculative preemption */
    abc_speculative_kick(now);
    
    return 0;
}

/*
 * Input event handler - standard input subsystem
 *
 * This fires for all input events (keyboard, mouse, gamepad) going
 * through the Linux input subsystem.
 *
 * The current task processing the event should get input boost.
 */
SEC("fentry/input_event")
int BPF_PROG(detect_input_event, void *dev, unsigned int type, 
             unsigned int code, int value)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_input_event);
    STAT_INC(nr_input_detected);
    
    /* Classify this task as input handler */
    classify_as_input(p, now);
    
    /* Also trigger A.B.C. */
    abc_speculative_kick(now);
    
    return 0;
}

/*
 * HID input report - another early detection point
 *
 * Fires when HID driver processes an input report.
 */
SEC("fentry/hid_input_report")
int BPF_PROG(detect_hid_input_report, void *hid, void *report, 
             int len, int interrupt)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_hid_input_report);
    STAT_INC(nr_input_detected);
    
    /* Classify and trigger A.B.C. */
    classify_as_input(p, now);
    abc_speculative_kick(now);
    
    return 0;
}

/* ============================================================================
 * INPUT WINDOW CHECK
 * ============================================================================
 *
 * Used by other code to check if we're in an input response window.
 * Tasks waking during this window may get priority boost.
 */

static __always_inline bool in_input_window(u64 now)
{
    if (last_input_ns == 0)
        return false;
    
    return (now - last_input_ns) < INPUT_WINDOW_NS;
}

#endif /* __DETECTION_INPUT_BPF_H */
