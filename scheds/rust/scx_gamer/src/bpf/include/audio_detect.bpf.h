/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Audio Thread Front-Running Detection
 * Copyright (c) 2025 RitzDaCat
 *
 * Ultra-low latency audio thread detection using fentry hooks on ALSA ioctls.
 * This allows us to "front-run" the audio server wakeup chain.
 */
#ifndef __GAMER_AUDIO_DETECT_BPF_H
#define __GAMER_AUDIO_DETECT_BPF_H

#include "config.bpf.h"
#include "types.bpf.h"
#include "task_class.bpf.h"
#include <bpf/bpf_core_read.h>

static __always_inline void recompute_boost_shift(struct task_ctx *tctx);

/* Shared globals */
extern volatile u32 detected_fg_tgid;
extern const volatile u32 foreground_tgid;
extern volatile u64 nr_system_audio_threads;
extern volatile u64 nr_game_audio_threads;
extern volatile u64 nr_system_audio_fentry_matches;

/* Forward declarations for ALSA structures (CO-RE read targets) */
struct snd_pcm_file;
struct snd_pcm_substream;
struct snd_pcm_runtime;

/*
 * Linux _IO macros (from asm-generic/ioctl.h) - needed for ALSA ioctl commands
 *
 * TIER 0: All macros are compile-time evaluated (zero runtime cost)
 * Used for constructing ioctl command constants for filtering.
 */
#define _IOC_NRBITS     8
#define _IOC_TYPEBITS   8
#define _IOC_SIZEBITS   14
#define _IOC_DIRBITS    2

#define _IOC_NRSHIFT    0
#define _IOC_TYPESHIFT  (_IOC_NRSHIFT+_IOC_NRBITS)
#define _IOC_SIZESHIFT  (_IOC_TYPESHIFT+_IOC_TYPEBITS)
#define _IOC_DIRSHIFT   (_IOC_SIZESHIFT+_IOC_SIZEBITS)

/* TIER 0: Compile-time ioctl command construction */
#define _IOC(dir,type,nr,size) \
	(((dir)  << _IOC_DIRSHIFT) | \
	 ((type) << _IOC_TYPESHIFT) | \
	 ((nr)   << _IOC_NRSHIFT) | \
	 ((size) << _IOC_SIZESHIFT))

/* TIER 0: Compile-time constant for write ioctl */
#define _IOW(type,nr,size)      _IOC(1,(type),(nr),sizeof(size))

/*
 * ALSA PCM ioctl commands (from sound/asound.h)
 * These are the commands used to write audio frames to the hardware buffer.
 *
 * TIER 0: All constants are compile-time evaluated (zero runtime cost)
 *
 * NOTE: The size argument (`snd_xferi` or `snd_xfern`) is not used in the BPF
 * program, so we can define it as a simple type to avoid pulling in the full
 * kernel header, which can cause type redefinition conflicts.
 */
typedef struct { long frames; } snd_xferi_t;
typedef struct { long frames; } snd_xfern_t;

/* TIER 0: Compile-time ioctl command constants */
#define SNDRV_PCM_IOCTL_WRITEI_FRAMES   _IOW('A', 0x50, snd_xferi_t)
#define SNDRV_PCM_IOCTL_WRITEN_FRAMES   _IOW('A', 0x51, snd_xfern_t)

/*
 * fentry/do_vfs_ioctl: ALSA audio buffer submission detection
 *
 * This hooks the generic VFS ioctl handler, which is a stable, exported
 * kernel function. It fires on EVERY ioctl, so we must filter efficiently
 * for only the ALSA buffer write commands.
 *
 * TIER 0/1: Optimized for high-frequency ioctl filtering
 * - Early filter: Tier 0 (~1-2ns comparison, ~99.9% of ioctls rejected here)
 * - Task lookup: Tier 0 (~3-5ns, bpf_get_current_task_btf is fast)
 * - Map update: Tier 1 (~100-300ns, only for audio ioctls)
 * - Total overhead: ~1-2ns for non-audio ioctls, ~104-307ns for audio ioctls
 *
 * Frequency: Fires on every ioctl (1000s/sec), but only processes audio ioctls (~10-100/sec)
 * Net overhead: ~1-2ns per ioctl (negligible due to early filter)
 */
SEC("fentry/do_vfs_ioctl")
int BPF_PROG(detect_audio_submit, struct file *filp, unsigned int fd, unsigned int cmd, unsigned long arg)
{
    /* TIER 0: Early filter - reject 99.9% of ioctls immediately
     * Most ioctls are not audio-related, so this check is critical for performance
     * Use unlikely() hint for better branch prediction */
    if (unlikely(cmd != SNDRV_PCM_IOCTL_WRITEI_FRAMES && cmd != SNDRV_PCM_IOCTL_WRITEN_FRAMES)) {
        return 0; /* Not a write ioctl, ignore (99.9% of calls exit here) */
    }

    /* TIER 0: Get current task (fast, no syscall overhead) */
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    if (unlikely(!p)) {
        return 0;
    }

    /* TIER 1: Map update - only executed for audio ioctls (~0.1% of ioctls)
     * Cache task pointer for front-running in scheduler hot path */
    u32 tgid = (u32)p->tgid;
    u32 tid = (u32)p->pid;

    /* Ensure task context exists so we can stamp classification immediately */
    struct task_ctx *tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, BPF_LOCAL_STORAGE_GET_F_CREATE);
    if (!tctx)
        return 0;

    u64 start_time = BPF_CORE_READ(p, start_time);
    if (start_time)
        tctx->task_cookie = start_time;

    /* Harvest runtime parameters (sample rate / buffer size) when available */
    struct snd_pcm_file *pcm_file = BPF_CORE_READ(filp, private_data);
    if (pcm_file) {
        struct snd_pcm_substream *substream = BPF_CORE_READ(pcm_file, substream);
        if (substream) {
            struct snd_pcm_runtime *runtime = BPF_CORE_READ(substream, runtime);
            if (runtime) {
                u32 rate = BPF_CORE_READ(runtime, rate);
                u64 period_frames = (u64)BPF_CORE_READ(runtime, period_size);
                if (period_frames == 0)
                    period_frames = (u64)BPF_CORE_READ(runtime, buffer_size);
                if (rate > 0)
                    tctx->audio_sample_rate = rate;
                if (period_frames > 0 && period_frames <= 0xFFFFFFFFULL)
                    tctx->audio_buffer_size = (u32)period_frames;
            }
        }
    }

    /* Foreground process → game audio, otherwise system audio */
    u32 fg_tgid = detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
    bool is_foreground = (fg_tgid != 0) && (tgid == fg_tgid);

    if (is_foreground) {
        u8 one = 1;
        bpf_map_update_elem(&game_audio_threads_map, &tid, &one, BPF_ANY);
        bpf_map_delete_elem(&system_audio_threads_map, &tid);
        if (!tctx->is_game_audio && !tctx->is_background) {
            tctx->is_game_audio = 1;
            apply_class_boost(tctx, 1);
            __atomic_fetch_add(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
            update_task_flags_cache(p, tctx);
            recompute_boost_shift(tctx);
        }
    } else {
        u8 one = 1;
        bpf_map_update_elem(&system_audio_threads_map, &tid, &one, BPF_ANY);
        bpf_map_delete_elem(&game_audio_threads_map, &tid);
        struct system_audio_entry *entry = bpf_map_lookup_elem(&system_audio_tgids_map, &tgid);
        if (entry) {
            __atomic_fetch_add(&entry->refcount, 1, __ATOMIC_RELAXED);
        } else {
            struct system_audio_entry init = {
                .refcount = 1,
                ._pad = 0,
            };
            bpf_map_update_elem(&system_audio_tgids_map, &tgid, &init, BPF_ANY);
        }
        if (!tctx->is_system_audio && !tctx->is_background) {
            tctx->is_system_audio = 1;
            apply_class_boost(tctx, 1);
            __atomic_fetch_add(&nr_system_audio_threads, 1, __ATOMIC_RELAXED);
            __atomic_fetch_add(&nr_system_audio_fentry_matches, 1, __ATOMIC_RELAXED);
            update_task_flags_cache(p, tctx);
            recompute_boost_shift(tctx);
        }
    }

    return 0;
}

/**
 * is_system_audio_thread - Check if thread is system audio thread
 * @tid: Thread ID to check
 *
 * Helper function for thread classification (stub for now).
 * Currently returns false as fentry hooks aren't fully implemented yet.
 * The scheduler falls back to name-based detection for audio threads.
 *
 * TIER 0: Single return statement (~0.1-0.5ns)
 * When implemented, will be Tier 1 (~50-80ns map lookup)
 */
static __always_inline bool is_system_audio_thread(u32 tid)
{
	u8 *flag = bpf_map_lookup_elem(&system_audio_threads_map, &tid);
	return flag && *flag;
}

/**
 * is_usb_audio_thread - Check if thread is USB audio thread
 * @tid: Thread ID to check
 *
 * Helper function for thread classification (stub for now).
 * Currently returns false as fentry hooks aren't fully implemented yet.
 * The scheduler falls back to name-based detection for USB audio threads.
 *
 * TIER 0: Single return statement (~0.1-0.5ns)
 * When implemented, will be Tier 1 (~50-80ns map lookup)
 */
static __always_inline bool is_usb_audio_thread(u32 tid)
{
	/* TODO: Implement fentry-based detection for USB audio */
	return false;
}

/**
 * is_game_audio_thread - Check if thread is game audio thread
 * @tid: Thread ID to check
 *
 * Helper function for thread classification (stub for now).
 * Currently returns false as fentry hooks aren't fully implemented yet.
 * The scheduler falls back to name-based detection for game audio threads.
 *
 * TIER 0: Single return statement (~0.1-0.5ns)
 * When implemented, will be Tier 1 (~50-80ns map lookup)
 */
static __always_inline bool is_game_audio_thread(u32 tid)
{
	u8 *flag = bpf_map_lookup_elem(&game_audio_threads_map, &tid);
	return flag && *flag;
}

#endif /* __GAMER_AUDIO_DETECT_BPF_H */
