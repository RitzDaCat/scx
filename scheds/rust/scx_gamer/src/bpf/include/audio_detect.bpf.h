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

/* Linux _IO macros (from asm-generic/ioctl.h) - needed for ALSA ioctl commands */
#define _IOC_NRBITS     8
#define _IOC_TYPEBITS   8
#define _IOC_SIZEBITS   14
#define _IOC_DIRBITS    2

#define _IOC_NRSHIFT    0
#define _IOC_TYPESHIFT  (_IOC_NRSHIFT+_IOC_NRBITS)
#define _IOC_SIZESHIFT  (_IOC_TYPESHIFT+_IOC_TYPEBITS)
#define _IOC_DIRSHIFT   (_IOC_SIZESHIFT+_IOC_SIZEBITS)

#define _IOC(dir,type,nr,size) \
	(((dir)  << _IOC_DIRSHIFT) | \
	 ((type) << _IOC_TYPESHIFT) | \
	 ((nr)   << _IOC_NRSHIFT) | \
	 ((size) << _IOC_SIZESHIFT))

#define _IOW(type,nr,size)      _IOC(1,(type),(nr),sizeof(size))

/*
 * ALSA PCM ioctl commands (from sound/asound.h)
 * These are the commands used to write audio frames to the hardware buffer.
 *
 * NOTE: The size argument (`snd_xferi` or `snd_xfern`) is not used in the BPF
 * program, so we can define it as a simple type to avoid pulling in the full
 * kernel header, which can cause type redefinition conflicts.
 */
typedef struct { long frames; } snd_xferi_t;
typedef struct { long frames; } snd_xfern_t;
#define SNDRV_PCM_IOCTL_WRITEI_FRAMES   _IOW('A', 0x50, snd_xferi_t)
#define SNDRV_PCM_IOCTL_WRITEN_FRAMES   _IOW('A', 0x51, snd_xfern_t)

/*
 * fentry/do_vfs_ioctl: ALSA audio buffer submission detection
 *
 * This hooks the generic VFS ioctl handler, which is a stable, exported
 * kernel function. It fires on EVERY ioctl, so we must filter efficiently
 * for only the ALSA buffer write commands.
 */
SEC("fentry/do_vfs_ioctl")
int BPF_PROG(detect_audio_submit, struct file *filp, unsigned int fd, unsigned int cmd, unsigned long arg)
{
    /* Filter for audio buffer write commands */
    if (cmd != SNDRV_PCM_IOCTL_WRITEI_FRAMES && cmd != SNDRV_PCM_IOCTL_WRITEN_FRAMES) {
        return 0; /* Not a write ioctl, ignore */
    }

    struct task_struct *p = (void *)bpf_get_current_task_btf();
    if (!p) {
        return 0;
    }

    /* This task is submitting audio. Cache its pointer for front-running. */
    u32 tgid = (u32)p->tgid;
    u64 val = (u64)(unsigned long)p;
    bpf_map_update_elem(&audio_thread_ptr_map, &tgid, &val, BPF_ANY);
    
    return 0;
}

/*
 * Helper functions for thread classification (stubs for now)
 * These check if a thread ID is in the audio detection map.
 * Currently these return false as the fentry hooks aren't fully implemented yet.
 * The scheduler falls back to name-based detection for audio threads.
 */
static __always_inline bool is_system_audio_thread(u32 tid)
{
	/* TODO: Implement fentry-based detection similar to GPU detection */
	return false;
}

static __always_inline bool is_usb_audio_thread(u32 tid)
{
	/* TODO: Implement fentry-based detection for USB audio */
	return false;
}

static __always_inline bool is_game_audio_thread(u32 tid)
{
	/* TODO: Implement fentry-based detection for game audio */
	return false;
}

#endif /* __GAMER_AUDIO_DETECT_BPF_H */
