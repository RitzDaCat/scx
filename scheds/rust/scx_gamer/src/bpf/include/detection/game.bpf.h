/* SPDX-License-Identifier: GPL-2.0 */
/*
 * game.bpf.h - Game process detection via LSM hooks
 *
 * Detects game processes via:
 * - LSM file_open: Detect game executables
 * - Process hierarchy: Track child threads
 *
 * Sets FLAG_GAME on task_ctx for all threads of the foreground game.
 *
 * Priority: Sets base boost for game threads
 */

#ifndef __DETECTION_GAME_BPF_H
#define __DETECTION_GAME_BPF_H

/* TODO: Implement in Phase 3
 *
 * Hooks to implement:
 * - SEC("lsm/file_open") - Detect game executable launch
 */

#endif /* __DETECTION_GAME_BPF_H */

