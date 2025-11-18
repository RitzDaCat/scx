/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Configuration and Tunables
 * Copyright (c) 2025 RitzDaCat
 *
 * All scheduler tunables, thresholds, and constants.
 * This file is AI-friendly: ~100 lines, single responsibility.
 *
 * TIER 0: All macros are compile-time constants (zero runtime cost)
 * This file contains only preprocessor definitions that are evaluated at compile time.
 * No runtime overhead - all values are substituted directly into code during compilation.
 */
#ifndef __GAMER_CONFIG_BPF_H
#define __GAMER_CONFIG_BPF_H

/*
 * CCD Classification
 *
 * TIER 0: Compile-time constants
 */
#define CCD_CLASS_UNKNOWN	0
#define CCD_CLASS_CACHE		1
#define CCD_CLASS_FREQ		2

/*
 * Feature Toggles
 *
 * TIER 0: Compile-time constants (zero runtime cost)
 * These provide coarse build-time control over optional subsystems so we can
 * strip legacy heuristics or high-volume telemetry in lean deployments.
 */
#ifndef CONFIG_GAMER_ENABLE_LEGACY_CLASSIFY
#define CONFIG_GAMER_ENABLE_LEGACY_CLASSIFY	1
#endif

#ifndef CONFIG_GAMER_ENABLE_EXTENDED_STATS
#define CONFIG_GAMER_ENABLE_EXTENDED_STATS	1
#endif

/*
 * CPU Configuration
 *
 * TIER 0: Compile-time constant
 */
#define MAX_CPUS	256

/*
 * Dispatch Queue IDs
 *
 * TIER 0: Compile-time constant
 */
#define SHARED_DSQ	0

/*
 * Performance Tuning Thresholds
 *
 * TIER 0: All compile-time constants (zero runtime cost)
 */

/* Interactive scheduling thresholds */
#define INTERACTIVE_SLICE_SHRINK_THRESH	256ULL	/* Shrink slice when interactive_avg > this */
#define INTERACTIVE_SMT_ALLOW_THRESH	128ULL	/* Allow SMT pairing when interactive < this */

/* Wakeup frequency scaling */
#define WAKE_FREQ_SHIFT			8	/* wakeup_freq >> SHIFT for boost factor */
#define CHAIN_BOOST_MAX			4	/* Maximum chain boost depth */
#define CHAIN_BOOST_STEP		2	/* Chain boost increment per sync-wake */

/*
 * Thread Classification Thresholds
 *
 * TIER 0: All compile-time constants (zero runtime cost)
 */

/* GPU submission thread detection */
#define GPU_SUBMIT_EXEC_THRESH_NS	100000ULL	/* <100μs exec suggests GPU submit */
#define GPU_SUBMIT_FREQ_MIN		50ULL		/* Min wakeup freq (500fps = 2ms) */
#define GPU_SUBMIT_STABLE_SAMPLES	8		/* Samples needed for classification */

/* Background task detection */
#define BACKGROUND_EXEC_THRESH_NS	5000000ULL	/* >5ms exec suggests CPU-intensive */
#define BACKGROUND_FREQ_MAX		10ULL		/* Low freq (<10 = >100ms sleep) */
#define BACKGROUND_STABLE_SAMPLES	4		/* Samples for stable classification */

/*
 * CPU Frequency Scaling
 *
 * TIER 0: Compile-time arithmetic (evaluated at compile time)
 */
#define CPUFREQ_LOW_THRESH	(SCX_CPUPERF_ONE / 4)
#define CPUFREQ_HIGH_THRESH	(SCX_CPUPERF_ONE - SCX_CPUPERF_ONE / 4)

/*
 * Memory Management
 * Optimized for high refresh rate gaming (240Hz+ = 2-4ms frame budget)
 *
 * TIER 0: Compile-time constant
 */
#define MM_HINT_UPDATE_INTERVAL_NS	2000000ULL	/* 2ms (was 10ms) - allows ~2 updates per 240Hz frame */

/*
 * Migration Control
 *
 * TIER 0: Compile-time constant
 */
#define MIG_TOKEN_SCALE			1024ULL		/* Token bucket scaling factor */

/*
 * Classification refresh cadence
 * Run slow-path classification at most every X nanoseconds per thread.
 */
#define CLASSIFICATION_REFRESH_NS	(2ULL * NSEC_PER_MSEC)
#define CLASSIFICATION_BACKOFF_MAX_SHIFT	3
#define IDLE_HINT_VALID_NS		(500ULL * NSEC_PER_USEC)

/* NAPI preference tracking
 *
 * TIER 0: Compile-time constants
 */
#define NET_RX_SOFTIRQ	3
#define NET_TX_SOFTIRQ	2
#define NAPI_PREFER_TIMEOUT_NS	(5ULL * NSEC_PER_MSEC)

#endif /* __GAMER_CONFIG_BPF_H */
