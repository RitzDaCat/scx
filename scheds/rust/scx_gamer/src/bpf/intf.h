/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
 * Copyright (c) 2025 RitzDaCat
 */

#ifndef __INTF_H
#define __INTF_H

#include <limits.h>

#define MAX(x, y) ((x) > (y) ? (x) : (y))
#define MIN(x, y) ((x) < (y) ? (x) : (y))
#define CLAMP(val, lo, hi) MIN(MAX(val, lo), hi)
#define ARRAY_SIZE(x) (sizeof(x) / sizeof((x)[0]))
#define CACHE_ALIGNED __attribute__((aligned(64)))

enum consts {
    NSEC_PER_USEC = 1000ULL,
    NSEC_PER_MSEC = (1000ULL * NSEC_PER_USEC),
    NSEC_PER_SEC = (1000ULL * NSEC_PER_MSEC),
};

#ifndef __VMLINUX_H__
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long u64;

typedef signed char s8;
typedef signed short s16;
typedef signed int s32;
typedef signed long s64;

typedef int pid_t;
#endif /* __VMLINUX_H__ */

struct mig_limiter_cfg {
    u64 window_ns;
    u32 max_per_window;
};

struct cpu_arg {
	s32 cpu_id;
};

#endif /* __INTF_H */
