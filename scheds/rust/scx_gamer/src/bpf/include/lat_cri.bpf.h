/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Pure Hook-Based Priority (Phase 5 - Zero Behavioral Detection)
 * Copyright (c) 2025 RitzDaCat
 *
 * COMPLETELY REMOVED: All behavioral/statistical priority calculation.
 * - No frequency tracking (wakeup_freq, wake_freq, run_freq)
 * - No EMA calculations
 * - No lat_cri formula
 * - No greedy penalty
 *
 * Priority is now 100% based on kernel hook flags:
 * - is_input_handler (from fentry/input_event)
 * - is_gpu_submit (from fentry/drm_ioctl, fentry/security_file_open)
 * - is_compositor (from fentry/drm_mode_page_flip)
 * - is_audio (from fentry/snd_*)
 * - is_network (from fentry/sock_*)
 * - is_game_critical (propagated through wake chains)
 *
 * This file is now essentially empty - kept for compatibility.
 * All priority logic is in the boost_shift calculation.
 */
#ifndef __GAMER_LAT_CRI_BPF_H
#define __GAMER_LAT_CRI_BPF_H

/* No behavioral calculations - priority is purely flag-based */

#endif /* __GAMER_LAT_CRI_BPF_H */
