/* SPDX-License-Identifier: GPL-2.0 */
/*
 * config.bpf.h - Configuration constants and tunables
 *
 * This file is the SINGLE SOURCE OF TRUTH for all scheduler constants.
 * All magic numbers must be defined here with explanatory comments.
 *
 * Sections:
 * 1. System limits
 * 2. Time slices
 * 3. Boost levels
 * 4. Flags
 * 5. Tunables (volatile - set from userspace)
 */

#ifndef __CONFIG_BPF_H
#define __CONFIG_BPF_H

/* ============================================================================
 * SECTION 1: SYSTEM LIMITS
 * ============================================================================ */

/* Maximum CPUs supported - must match kernel CONFIG_NR_CPUS */
#define MAX_CPUS 512

/* Shared dispatch queue ID */
#define SHARED_DSQ 0

/* ============================================================================
 * SECTION 2: TIME SLICES (in nanoseconds)
 * ============================================================================ */

/*
 * Base time slice - all other slices derive from this.
 * Default: 10µs (10000ns)
 *
 * Why 10µs:
 * - Matches scx_cosmos proven default
 * - At 240Hz (4.17ms frame), allows 417 scheduling decisions per frame
 * - Low enough for responsive input handling
 * - High enough to avoid excessive context switch overhead
 */
#define SLICE_NS_DEFAULT 10000ULL

/* Slice multipliers (relative to base) */
#define SLICE_INPUT_DIVISOR  4   /* Input: base/4 = 2.5µs - yield quickly */
#define SLICE_GAME_MULT      1   /* Game: base*1 = 10µs - standard */
#define SLICE_BACKGROUND_MULT 2  /* Background: base*2 = 20µs - less overhead */

/* Maximum slice for starvation prevention */
#define SLICE_MAX_NS (20 * 1000 * 1000ULL)  /* 20ms */

/*
 * Slice lag - Maximum vtime credit a sleeping task can accumulate.
 * 
 * Why 20ms:
 * - ~1.2 frames at 60Hz, ~0.5 frames at 240Hz
 * - Allows waking tasks a small "burst" window
 * - Prevents long-sleeping tasks from dominating after wake
 * - Matches scx_cosmos proven default
 *
 * Without this cap, a task sleeping 10 seconds could wake up
 * with 10 seconds of "credit" and starve all other tasks.
 */
#define SLICE_LAG_NS (20 * 1000 * 1000ULL)  /* 20ms */

/*
 * Input/sync window duration for A.B.C. (Always Be Casting)
 * 
 * When input or sync events occur, game threads waking within
 * this window get a priority boost (likely responding to input).
 */
#define WINDOW_DURATION_NS (1000 * 1000ULL)  /* 1ms */

/*
 * Boost decay time - how long a boost lasts after classification.
 *
 * CRITICAL: Without decay, boosts are PERMANENT and cause starvation.
 * A task classified as INPUT at startup would keep INPUT priority forever,
 * even if it's now doing background work.
 *
 * Why 5ms:
 * - Long enough to cover input → frame → present pipeline
 * - Short enough to not starve other tasks
 * - Approximately 1 frame at 200Hz
 */
#define BOOST_DECAY_NS (5 * 1000 * 1000ULL)  /* 5ms */

/*
 * Long wait threshold for debug logging.
 * 
 * When a task waits longer than this before running, we emit a debug event
 * to identify which tasks are hitting outlier wait times.
 * 
 * Why 10ms:
 * - Catches outliers (anything >10ms is notable for gaming)
 * - Below starvation threshold (20ms)
 * - Helps diagnose 15-20ms waits before rescue kicks in
 */
#define LONG_WAIT_THRESH_NS (10 * 1000 * 1000ULL)  /* 10ms */

/* ============================================================================
 * SECTION 3: BOOST LEVELS
 * ============================================================================
 *
 * Priority is expressed as boost_shift (0-7).
 * Effective priority multiplier = 2^boost_shift
 *
 * Higher boost = smaller deadline increment = runs sooner
 */

#define BOOST_BACKGROUND  0   /* 1x   - background tasks */
#define BOOST_FOREGROUND  1   /* 2x   - foreground non-game */
#define BOOST_GAME_WORKER 2   /* 4x   - game worker threads */
#define BOOST_GAME_MAIN   3   /* 8x   - game main thread */
#define BOOST_COMPOSITOR  4   /* 16x  - KWin, window compositor */
#define BOOST_AUDIO       5   /* 32x  - audio threads */
#define BOOST_GPU         6   /* 64x  - GPU command submission */
#define BOOST_INPUT       7   /* 128x - input handlers (highest) */

#define BOOST_MAX         7   /* Maximum boost level */

/* ============================================================================
 * SECTION 4: FLAGS
 * ============================================================================
 *
 * Task classification flags (stored in task_ctx.flags)
 */

/* Task type flags */
#define FLAG_GAME       (1 << 0)  /* Thread belongs to foreground game */
#define FLAG_INPUT      (1 << 1)  /* Input handler (mouse/keyboard) */
#define FLAG_GPU        (1 << 2)  /* GPU command submission */
#define FLAG_AUDIO      (1 << 3)  /* Audio thread */
#define FLAG_COMPOSITOR (1 << 4)  /* Window compositor */
#define FLAG_NETWORK    (1 << 5)  /* Network I/O thread */
#define FLAG_SYNC       (1 << 6)  /* Wine/Proton sync primitive */
#define FLAG_STALE      (1 << 7)  /* Needs reclassification */

/* Combined flag masks */
#define FLAGS_LATENCY_CRITICAL (FLAG_INPUT | FLAG_GPU | FLAG_AUDIO | FLAG_COMPOSITOR)
#define FLAGS_GAME_RELATED     (FLAG_GAME | FLAG_GPU | FLAG_AUDIO | FLAG_SYNC)

/* CPU context flags */
#define CPU_IDLE           (1 << 0)  /* Currently idle */
#define CPU_SMT            (1 << 1)  /* Is SMT sibling (hyperthread) */
#define CPU_PREFERRED      (1 << 2)  /* In preferred/high-perf set */
#define CPU_INPUT_RESERVED (1 << 3)  /* Reserved for imminent input */

/* ============================================================================
 * SECTION 5: TUNABLES (set from userspace via volatile)
 * ============================================================================
 *
 * These are declared in main.bpf.c with 'const volatile' so userspace
 * can modify them before loading the BPF program.
 */

/* Tunable declarations - actual definitions in main.bpf.c */
/* const volatile u64 slice_ns = SLICE_NS_DEFAULT; */
/* const volatile bool avoid_smt = true; */
/* const volatile bool no_stats = false; */
/* const volatile u32 foreground_tgid = 0; */

/* ============================================================================
 * SECTION 6: DEBUG
 * ============================================================================ */

#ifdef DEBUG
#define DBG_PRINT(fmt, ...) bpf_printk("scx_gamer: " fmt, ##__VA_ARGS__)
#else
#define DBG_PRINT(fmt, ...) do {} while (0)
#endif

#endif /* __CONFIG_BPF_H */

