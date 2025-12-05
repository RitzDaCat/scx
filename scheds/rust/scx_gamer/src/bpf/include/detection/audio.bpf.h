/* SPDX-License-Identifier: GPL-2.0 */
/*
 * audio.bpf.h - Audio thread detection hooks
 *
 * Detects audio threads via fentry hooks on:
 * - do_vfs_ioctl(): Catches ALSA PCM write operations
 *
 * Sets FLAG_AUDIO on task_ctx for threads doing audio I/O.
 *
 * Priority: HIGH (boost_shift = 5, 32x priority)
 *
 * Why audio detection matters:
 * - Audio underruns cause audible pops/clicks (very noticeable)
 * - Audio buffers are typically 5-10ms, missing deadline is instant failure
 * - Game audio threads need consistent low-latency scheduling
 * - PipeWire/PulseAudio threads also benefit
 */

#ifndef __DETECTION_AUDIO_BPF_H
#define __DETECTION_AUDIO_BPF_H

#include "../helpers.bpf.h"
#include "../priority/boost.bpf.h"

/* ============================================================================
 * ALSA IOCTL DETECTION
 * ============================================================================ */

/* ALSA PCM ioctl commands for audio write operations */
#define SNDRV_PCM_IOCTL_WRITEI_FRAMES  0x4150  /* Write interleaved frames */
#define SNDRV_PCM_IOCTL_WRITEN_FRAMES  0x4151  /* Write non-interleaved frames */
#define SNDRV_PCM_IOCTL_READI_FRAMES   0x4131  /* Read interleaved (mic input) */
#define SNDRV_PCM_IOCTL_READN_FRAMES   0x4132  /* Read non-interleaved */

/* PipeWire/PulseAudio use memfd, but the audio thread pattern is similar */

/*
 * Check if ioctl is ALSA audio related.
 */
static __always_inline bool is_audio_ioctl(unsigned int cmd)
{
    /* ALSA PCM write operations */
    if (cmd == SNDRV_PCM_IOCTL_WRITEI_FRAMES || 
        cmd == SNDRV_PCM_IOCTL_WRITEN_FRAMES)
        return true;
    
    /* ALSA PCM read operations (voice chat, mic) */
    if (cmd == SNDRV_PCM_IOCTL_READI_FRAMES || 
        cmd == SNDRV_PCM_IOCTL_READN_FRAMES)
        return true;
    
    return false;
}

/* ============================================================================
 * HELPER: Classify task as audio thread
 * ============================================================================ */

static __always_inline void classify_as_audio(struct task_struct *p, u64 now)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    if (!tctx)
        return;
    
    /* Set audio flag */
    tctx->flags |= FLAG_AUDIO;
    
    /* Only update boost if not already higher */
    if (tctx->boost_shift < BOOST_AUDIO)
        tctx->boost_shift = BOOST_AUDIO;
    
    tctx->classified_at_ns = now;
    
    /* NOTE: Per-hook stats tracked in fentry callbacks, not here */
}

/* ============================================================================
 * FENTRY HOOKS
 * ============================================================================ */

/*
 * VFS ioctl handler - catches ALSA ioctls
 *
 * ALSA audio goes through the VFS ioctl path.
 * We filter for PCM read/write operations.
 */
SEC("fentry/do_vfs_ioctl")
int BPF_PROG(detect_audio_ioctl, unsigned int fd, unsigned int cmd, unsigned long arg)
{
    /* Quick check if this is an audio ioctl */
    if (!is_audio_ioctl(cmd))
        return 0;
    
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    
    /* Track per-hook stats */
    STAT_INC(nr_audio_ioctl);
    STAT_INC(nr_audio_detected);
    
    /* Classify as audio thread */
    classify_as_audio(p, now);
    
    return 0;
}

/*
 * PCM period elapsed - audio interrupt handler
 *
 * This fires when an audio period completes, triggering the next buffer fill.
 * The woken thread should get audio boost.
 *
 * Note: This may fire in interrupt context, so we just record the timestamp.
 */
SEC("fentry/snd_pcm_period_elapsed")
int BPF_PROG(detect_pcm_period_elapsed, void *substream)
{
    /* Track per-hook stats */
    STAT_INC(nr_pcm_period);
    STAT_INC(nr_audio_detected);
    
    return 0;
}

#endif /* __DETECTION_AUDIO_BPF_H */
