/* SPDX-License-Identifier: GPL-2.0 */
/*
 * gpu.bpf.h - GPU thread detection hooks
 *
 * Detects GPU submission threads via fentry hooks on:
 * - drm_ioctl(): DRM ioctl handler (catches AMD/Intel/NVIDIA submissions)
 *
 * Sets FLAG_GPU on task_ctx for threads submitting GPU work.
 *
 * Priority: HIGH (boost_shift = 6, 64x priority)
 *
 * Why GPU detection matters:
 * - GPU command submission is on the critical path for frame delivery
 * - Delayed GPU submission = delayed frame = increased input lag
 * - These threads need priority right behind input handlers
 *
 * Detection strategy:
 * - Hook drm_ioctl which handles all DRM driver ioctls
 * - Detect submission-related ioctls (AMD, Intel, nouveau)
 * - Mark thread as GPU and give it boost
 */

#ifndef __DETECTION_GPU_BPF_H
#define __DETECTION_GPU_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"

/* ============================================================================
 * GPU IOCTL DETECTION
 * ============================================================================ */

/* DRM ioctl command structure - bits 0-7 are command number, 8-15 are group */
#define DRM_IOCTL_NR(n)     ((n) & 0xFF)
#define DRM_IOCTL_BASE      0x40  /* DRM ioctls start at 0x40 */

/* AMD amdgpu submission ioctls */
#define DRM_AMDGPU_CS       0x04  /* Command submission */
#define DRM_AMDGPU_WAIT_CS  0x05  /* Wait for CS completion */
#define DRM_AMDGPU_GEM_CREATE 0x00 /* GEM buffer creation (frequent during gaming) */

/* Intel i915 submission ioctls */
#define DRM_I915_GEM_EXECBUFFER2 0x29  /* Command buffer execution */
#define DRM_I915_GEM_WAIT        0x2C  /* Wait for completion */

/* Generic DRM syncobj ioctls (modern drivers) */
#define DRM_IOCTL_SYNCOBJ_WAIT   0xC3  /* Syncobj wait */
#define DRM_IOCTL_SYNCOBJ_SIGNAL 0xC6  /* Syncobj signal */

/*
 * Check if ioctl command is GPU submission related.
 * Returns true for commands that are on the critical rendering path.
 */
static __always_inline bool is_gpu_submit_ioctl(unsigned int cmd)
{
    unsigned int nr = DRM_IOCTL_NR(cmd);
    
    /* AMD amdgpu */
    if (nr == DRM_AMDGPU_CS || nr == DRM_AMDGPU_WAIT_CS)
        return true;
    
    /* Intel i915 */
    if (nr == DRM_I915_GEM_EXECBUFFER2 || nr == DRM_I915_GEM_WAIT)
        return true;
    
    /* DRM syncobj operations */
    if (nr == DRM_IOCTL_SYNCOBJ_WAIT || nr == DRM_IOCTL_SYNCOBJ_SIGNAL)
        return true;
    
    return false;
}

/* ============================================================================
 * HELPER: Classify task as GPU thread
 * ============================================================================ */

static __always_inline void classify_as_gpu(struct task_struct *p, u64 now)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    if (!tctx)
        return;
    
    /* Set GPU flag */
    tctx->flags |= FLAG_GPU;
    
    /* Only update boost if not already higher (e.g., input) */
    if (tctx->boost_shift < BOOST_GPU)
        tctx->boost_shift = BOOST_GPU;
    
    tctx->classified_at_ns = now;
    
    /* NOTE: Per-hook stats tracked in fentry callbacks, not here */
}

/* ============================================================================
 * FENTRY HOOKS
 * ============================================================================ */

/*
 * DRM ioctl handler - catches all DRM driver ioctls
 *
 * This is the main entry point for GPU operations:
 * - Command submission (rendering)
 * - Buffer management
 * - Synchronization primitives
 *
 * NOTE: Works for AMD/Intel, but NOT NVIDIA! Use dma_fence_signal for NVIDIA.
 */
SEC("fentry/drm_ioctl")
int BPF_PROG(detect_drm_ioctl, void *filp, unsigned int cmd, unsigned long arg)
{
    /* Quick check if this is a submit-related ioctl */
    if (!is_gpu_submit_ioctl(cmd))
        return 0;
    
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_drm_ioctl);
    STAT_INC(nr_gpu_detected);
    
    /*
     * Classify as GPU - any DRM submit ioctl is latency-critical.
     */
    classify_as_gpu(p, now);
    
    return 0;
}

/*
 * DRM atomic commit - frame submission
 *
 * This fires when a frame is submitted for display.
 * Critical for frame pacing and tear-free presentation.
 */
SEC("fentry/drm_atomic_commit")
int BPF_PROG(detect_drm_atomic_commit, void *state)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_drm_atomic_commit);
    STAT_INC(nr_gpu_detected);
    
    /* Atomic commits are always important for frame delivery */
    classify_as_gpu(p, now);
    
    return 0;
}

/*
 * DMA fence signal - UNIVERSAL GPU completion hook
 *
 * This fires when ANY GPU driver (AMD, Intel, NVIDIA) completes work.
 * This is the ONLY hook that reliably works for NVIDIA GPUs!
 *
 * Benefits:
 * - Works for ALL GPU vendors including NVIDIA
 * - Signals actual GPU completion (not just submission)
 * - Can be used for A.B.C. (GPU done → game thread about to wake)
 */
SEC("fentry/dma_fence_signal")
int BPF_PROG(detect_dma_fence_signal, void *fence)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_dma_fence_signal);
    STAT_INC(nr_gpu_detected);
    
    /* Classify the thread signaling the fence as GPU-related */
    classify_as_gpu(p, now);
    
    return 0;
}

#endif /* __DETECTION_GPU_BPF_H */
