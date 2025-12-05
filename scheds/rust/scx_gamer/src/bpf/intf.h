/* SPDX-License-Identifier: GPL-2.0 */
/*
 * intf.h - Interface definitions shared between BPF and userspace
 *
 * This file defines structures and constants that must be identical
 * in both the BPF program and the Rust userspace code.
 *
 * Note: This file is included by both BPF (C) and Rust (via bindgen).
 */

#ifndef __INTF_H
#define __INTF_H

/*
 * Type definitions:
 * - In BPF context: types come from vmlinux.h, don't include stdint.h
 * - In userspace context: define types from stdint.h
 */
#ifndef __KERNEL__
#ifndef __BPF__
/* Userspace: use standard C types */
#include <stdint.h>
typedef uint8_t  __u8;
typedef uint16_t __u16;
typedef uint32_t __u32;
typedef uint64_t __u64;
typedef int8_t   __s8;
typedef int16_t  __s16;
typedef int32_t  __s32;
typedef int64_t  __s64;
#endif
#endif

/* ============================================================================
 * CONSTANTS
 * ============================================================================ */

#define SCHEDULER_NAME "scx_gamer"

/* Maximum CPUs - must match config.bpf.h */
#define MAX_CPUS 512

/* Shared DSQ ID */
#define SHARED_DSQ 0

/* Boost levels - must match config.bpf.h */
#define BOOST_BACKGROUND  0
#define BOOST_FOREGROUND  1
#define BOOST_GAME_WORKER 2
#define BOOST_GAME_MAIN   3
#define BOOST_COMPOSITOR  4
#define BOOST_AUDIO       5
#define BOOST_GPU         6
#define BOOST_INPUT       7
#define BOOST_MAX         7

/* Task flags - must match config.bpf.h */
#define FLAG_GAME       (1 << 0)
#define FLAG_INPUT      (1 << 1)
#define FLAG_GPU        (1 << 2)
#define FLAG_AUDIO      (1 << 3)
#define FLAG_COMPOSITOR (1 << 4)
#define FLAG_NETWORK    (1 << 5)
#define FLAG_SYNC       (1 << 6)
#define FLAG_STALE      (1 << 7)

#endif /* __INTF_H */
