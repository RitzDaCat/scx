/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
 * Copyright (c) 2025 RitzDaCat
 *
 * Interface definitions and utility macros for BPF code.
 * All macros are TIER 0 (compile-time evaluated, zero runtime overhead).
 */
#ifndef __INTF_H
#define __INTF_H

#include <limits.h>

/*
 * TIER 0 MACROS: Compile-time evaluated, zero runtime overhead
 * These macros expand inline and are optimized away by the compiler.
 * 
 * PERFORMANCE NOTES:
 * - MAX/MIN: Single comparison + conditional move (~1-2ns)
 * - CLAMP: Two comparisons + conditional moves (~2-4ns)
 * - ARRAY_SIZE: Compile-time constant (zero runtime cost)
 * 
 * SAFETY: MAX/MIN/CLAMP evaluate arguments multiple times.
 * Only use with simple expressions (variables, constants), NOT function calls.
 * Example: MIN(x, y) ✓  |  MIN(func(), y) ✗ (func() called twice)
 */

/* Maximum of two values (compile-time optimized) */
#define MAX(x, y) ((x) > (y) ? (x) : (y))

/* Minimum of two values (compile-time optimized) */
#define MIN(x, y) ((x) < (y) ? (x) : (y))

/* Clamp value between lo and hi (compile-time optimized)
 * WARNING: Evaluates arguments multiple times - use only with simple expressions */
#define CLAMP(val, lo, hi) MIN(MAX(val, lo), hi)

/* Array size calculation (compile-time constant, zero runtime cost) */
#define ARRAY_SIZE(x) (sizeof(x) / sizeof((x)[0]))

/* Cache line alignment attribute (64 bytes = typical x86_64 cache line size)
 * TIER 0: Compile-time layout optimization, prevents false sharing
 * Used for: task_ctx, cpu_ctx (hot path data structures) */
#define CACHE_ALIGNED __attribute__((aligned(64)))

/*
 * Time conversion constants (compile-time constants)
 * TIER 0: All values computed at compile time, zero runtime cost
 */
enum consts {
    NSEC_PER_USEC = 1000ULL,
    NSEC_PER_MSEC = (1000ULL * NSEC_PER_USEC),      /* 1,000,000 ns */
    NSEC_PER_SEC = (1000ULL * NSEC_PER_MSEC),       /* 1,000,000,000 ns */
};

enum tailcall_slot {
	TAILCALL_SLOT_SELECT_CPU = 0,
	TAILCALL_SLOT_ENQUEUE_SIGNAL = 1,
	TAILCALL_SLOT__MAX,
};

/*
 * Preferred core cache age threshold
 * TIER 0: Compile-time constant (25ms = 25,000,000 ns)
 * Used for: GPU thread preferred CPU caching (prevents stale cache hits)
 */
#define PREF_CORE_MAX_AGE_NS (25ULL * NSEC_PER_MSEC)

/*
 * Type definitions (only when vmlinux.h not available)
 * TIER 0: Compile-time type aliases, zero runtime cost
 */
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

/*
 * Migration limiter configuration structure
 * Used for: Rate limiting task migrations to prevent thrashing
 * TIER 1: Accessed during migration decisions (~100-200ns overhead)
 */
struct mig_limiter_cfg {
    u64 window_ns;        /* Time window for migration counting */
    u32 max_per_window;  /* Maximum migrations allowed per window */
};

/*
 * CPU argument structure
 * Used for: CPU-specific operations and configuration
 * TIER 1: Accessed during CPU selection (~10-20ns overhead) */
struct cpu_arg {
	s32 cpu_id;
};

/*
 * Input lane enumeration
 * TIER 0: Enum values are compile-time constants
 * Used for: Input device classification (keyboard, mouse, controller)
 * 
 * Performance: Enum comparison is single integer comparison (~1ns)
 * Array indexing using enum is optimized to direct array access
 */
enum input_lane {
	INPUT_LANE_KEYBOARD = 0,    /* Keyboard input events */
	INPUT_LANE_MOUSE = 1,       /* Mouse input events */
	INPUT_LANE_CONTROLLER = 2,  /* Game controller input events */
	INPUT_LANE_OTHER = 3,       /* Other input devices */
	INPUT_LANE_MAX,             /* Array size sentinel */
};

#endif /* __INTF_H */
