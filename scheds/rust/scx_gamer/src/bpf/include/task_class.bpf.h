/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Thread Classification (Phase 4 - Zero Name Matching)
 * Copyright (c) 2025 RitzDaCat
 *
 * ALL NAME-BASED DETECTION HAS BEEN REMOVED.
 * 
 * Thread classification is now based ENTIRELY on:
 * 1. Kernel hooks (fentry) - GPU, input, audio, network subsystems
 * 2. Latency criticality (lat_cri) - derived from behavioral metrics
 * 3. Foreground detection - window focus based (external)
 * 4. Nice values - user/app controlled priority hints
 *
 * WHY NO NAME MATCHING:
 * - App names change between versions
 * - Forks/alternatives have different names (chromium vs brave vs chrome)
 * - Wine/Proton apps have different naming
 * - New apps won't be covered
 * - Even "stable" names aren't guaranteed (Discord could rebrand)
 *
 * This file now only contains utility helpers, no classification logic.
 */
#ifndef __GAMER_TASK_CLASS_BPF_H
#define __GAMER_TASK_CLASS_BPF_H

#include "config.bpf.h"

/* Apply baseline boost derived from role presets. Keeps maximum value so multiple
 * classifications can cooperate without losing the highest requested priority. */
static __always_inline void apply_class_boost(struct task_ctx *tctx, u8 boost)
{
	if (boost > tctx->class_boost)
		tctx->class_boost = boost;
}

/* REMOVED: All name-based functions
 * 
 * Previously contained:
 * - is_compiler_name() - now use nice values
 * - is_background_name() - now use foreground detection + nice values
 * - is_discord_name() - removed
 * - is_chromium_name() - removed
 * - is_steam_webhelper_name() - removed
 * - is_cursor_name() - removed
 * - is_plasma_systemmonitor_name() - removed
 * - is_voice_chat_audio_thread() - use fentry/audio hooks
 * - is_nvme_io_thread() - removed (arbitrary thresholds)
 * - is_nvme_hot_path_thread() - removed (arbitrary thresholds)
 * - detect_audio_buffer_size() - removed (arbitrary thresholds)
 * - is_gaming_traffic_pattern() - removed (arbitrary thresholds)
 * - classify_* functions - removed
 *
 * Classification now happens via:
 * - GPU: fentry/drm_ioctl, fentry/security_file_open (opens /dev/nvidia*, /dev/dri/render*)
 * - Audio: fentry/snd_pcm_* hooks
 * - Input: fentry/input_event
 * - Network: fentry/sock_* hooks
 * - Compositor: fentry on KDE/Wayland APIs
 * - Background: Automatically deprioritized via low lat_cri (low wake freq = less critical)
 */

#endif /* __GAMER_TASK_CLASS_BPF_H */
