/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Configuration and Tunables
 * Copyright (c) 2025 RitzDaCat
 *
 * All scheduler tunables, thresholds, and constants.
 * This file is AI-friendly: ~100 lines, single responsibility.
 */
#ifndef __GAMER_CONFIG_BPF_H
#define __GAMER_CONFIG_BPF_H

/*
 * CPU Configuration
 */
#define MAX_CPUS	256

/*
 * Dispatch Queue IDs
 */
#define SHARED_DSQ	0

/*
 * Performance Tuning Thresholds
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
 */
#define CPUFREQ_LOW_THRESH	(SCX_CPUPERF_ONE / 4)
#define CPUFREQ_HIGH_THRESH	(SCX_CPUPERF_ONE - SCX_CPUPERF_ONE / 4)

/*
 * Memory Management
 * Optimized for high refresh rate gaming (240Hz+ = 2-4ms frame budget)
 */
#define MM_HINT_UPDATE_INTERVAL_NS	2000000ULL	/* 2ms (was 10ms) - allows ~2 updates per 240Hz frame */

/*
 * Migration Control
 */
#define MIG_TOKEN_SCALE			1024ULL		/* Token bucket scaling factor */

/*
 * Userspace Command Flags (BSS cmd_flags bits)
 */
#define CMD_INPUT	(1u << 0)	/* Input event trigger */
#define CMD_FRAME	(1u << 1)	/* Frame event trigger */
#define CMD_NAPI	(1u << 2)	/* NAPI preference trigger */

/*
 * Kick Bitmap Configuration
 */
#define KICK_WORDS	((MAX_CPUS + 63) / 64)

#endif /* __GAMER_CONFIG_BPF_H */
