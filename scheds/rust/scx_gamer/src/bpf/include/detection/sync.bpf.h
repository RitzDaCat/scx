/* SPDX-License-Identifier: GPL-2.0 */
/*
 * sync.bpf.h - Wine/Proton synchronization detection hooks
 *
 * Detects Wine/Proton sync primitives via fentry hooks on:
 * - eventfd_signal_mask(): esync signaling (most common)
 * - do_futex(): fsync operations
 *
 * Sets FLAG_SYNC on task_ctx for Wine/Proton sync threads.
 *
 * Priority: Triggers A.B.C. preparation for game threads
 *
 * Background:
 * Wine/Proton implement Windows synchronization primitives using:
 * - esync: eventfd-based (faster, default in modern Proton)
 * - fsync: futex-based (kernel support required)
 * - ntsync: kernel driver (newest, best performance)
 *
 * When a sync signal fires, it usually means a game thread is about to wake.
 * We use this to proactively prepare CPUs (A.B.C. pattern).
 */

#ifndef __DETECTION_SYNC_BPF_H
#define __DETECTION_SYNC_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"
#include "input.bpf.h"  /* For abc_speculative_kick */

/* ============================================================================
 * SYNC DETECTION STATE
 * ============================================================================ */

/* Timestamp of last sync signal (for window tracking) */
volatile u64 last_sync_signal_ns;

/* Sync window - tasks waking after sync get boost consideration */
#define SYNC_WINDOW_NS (1 * 1000 * 1000)  /* 1ms */

/* ============================================================================
 * HELPER: Handle sync signal
 * ============================================================================ */

static __always_inline void handle_sync_signal(struct task_struct *p, u64 now)
{
    /*
     * No foreground_tgid check required!
     * 
     * These sync primitives (esync/fsync/ntsync) are Wine/Proton-specific.
     * Any task using them is automatically game-related.
     * This follows the 100% hook-based detection design philosophy.
     */
    
    /* Update sync timestamp for window boost detection */
    last_sync_signal_ns = now;
    
    /* Mark task with sync flag */
    struct task_ctx *tctx = lookup_task_ctx(p);
    if (tctx) {
        tctx->flags |= FLAG_SYNC;
        tctx->classified_at_ns = now;  /* For boost decay tracking */
        /* Don't change boost - sync threads themselves aren't critical,
         * but they signal that game threads are about to wake */
    }
    
    /* NOTE: Per-hook stats tracked in fentry callbacks, not here */
    
    /* A.B.C.: Speculatively prepare CPUs for game threads about to wake */
    s32 cpu = bpf_get_smp_processor_id();
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    
    /* Kick sibling if running low-priority work */
    if (cctx && cctx->sibling_cpu >= 0) {
        struct cpu_ctx *sibling = lookup_cpu_ctx(cctx->sibling_cpu);
        if (sibling && sibling->current_boost < BOOST_GAME_WORKER) {
            scx_bpf_kick_cpu(cctx->sibling_cpu, SCX_KICK_IDLE);
        }
    }
}

/* ============================================================================
 * FENTRY HOOKS
 * ============================================================================ */

/*
 * eventfd signal - esync implementation
 *
 * Wine esync uses eventfd to implement Windows sync primitives.
 * When a game signals a sync object, game threads are about to wake.
 */
SEC("fentry/eventfd_signal_mask")
int BPF_PROG(detect_esync_signal, void *eventfd_ctx, __u64 mask)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_esync);
    STAT_INC(nr_sync_detected);
    
    handle_sync_signal(p, now);
    
    return 0;
}

/*
 * futex operations - fsync implementation
 *
 * Wine fsync uses futexes for synchronization.
 * FUTEX_WAKE operations indicate game threads are being signaled.
 */
SEC("fentry/do_futex")
int BPF_PROG(detect_fsync_futex, u32 *uaddr, int op, u32 val,
             void *timeout, u32 *uaddr2, u32 val2, u32 val3)
{
    /* Only care about wake operations */
    int futex_op = op & 0x7F;  /* Mask out flags */
    
    /* FUTEX_WAKE = 1, FUTEX_WAKE_OP = 5, FUTEX_WAKE_BITSET = 10 */
    if (futex_op != 1 && futex_op != 5 && futex_op != 10)
        return 0;
    
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_fsync);
    STAT_INC(nr_sync_detected);
    
    handle_sync_signal(p, now);
    
    return 0;
}

/*
 * ntsync ioctl - ntsync kernel driver implementation
 *
 * ntsync is the newest and most efficient Wine sync mechanism.
 * It uses a dedicated kernel module for native NT sync primitives.
 *
 * Check availability: lsmod | grep ntsync
 */
SEC("fentry/ntsync_char_ioctl")
int BPF_PROG(detect_ntsync, void *file, unsigned int cmd, unsigned long arg)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_ntsync);
    STAT_INC(nr_sync_detected);
    
    handle_sync_signal(p, now);
    
    return 0;
}

/* ============================================================================
 * SYNC WINDOW CHECK
 * ============================================================================ */

static __always_inline bool in_sync_window(u64 now)
{
    if (last_sync_signal_ns == 0)
        return false;
    
    return (now - last_sync_signal_ns) < SYNC_WINDOW_NS;
}

#endif /* __DETECTION_SYNC_BPF_H */
