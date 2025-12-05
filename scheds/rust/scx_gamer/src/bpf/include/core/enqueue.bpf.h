/* SPDX-License-Identifier: GPL-2.0 */
/*
 * enqueue.bpf.h - Task enqueue logic
 *
 * Implements task enqueue decisions:
 * - DSQ selection (local vs shared)
 * - Slice calculation based on task type
 * - Preemption decisions
 *
 * Core logic is in main.bpf.c gamer_enqueue().
 * This file contains extended enqueue helpers.
 */

#ifndef __CORE_ENQUEUE_BPF_H
#define __CORE_ENQUEUE_BPF_H

/* TODO: Implement in Phase 5
 *
 * Functions to implement:
 * - should_direct_dispatch() - Check if task should bypass shared DSQ
 * - smart_kick_cpu() - Priority-aware preemption
 */

#endif /* __CORE_ENQUEUE_BPF_H */

