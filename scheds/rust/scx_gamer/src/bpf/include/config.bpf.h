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
#define CONFIG_GAMER_ENABLE_LEGACY_CLASSIFY	0
#endif

#ifndef CONFIG_GAMER_ENABLE_EXTENDED_STATS
#define CONFIG_GAMER_ENABLE_EXTENDED_STATS	0
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
 * CPU Scan Limits (BPF verifier friendly)
 *
 * TIER 0: Compile-time constants
 */
#define TASKGRAPH_MAX_PREF_SCAN   12	/* TaskGraph corral: scan up to 12 CPUs */
#define TASKGRAPH_BORROW_MAX_SCAN 12	/* Borrow scan limit */
#define FRAME_PHYS_SCAN_MAX       64	/* Frame thread physical scan limit */

/*
 * Histogram Configuration
 */
#define HIST_BUCKETS	12

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
 * Thread Classification: 100% Event-Driven
 *
 * REMOVED: All behavioral heuristic thresholds (GPU_SUBMIT_EXEC_THRESH_NS,
 * BACKGROUND_EXEC_THRESH_NS, etc.) were eliminated because they violate our
 * "100% proof, no guesswork" rule.
 *
 * Detection is now purely event-driven via kernel hooks:
 * - GPU: fentry/drm_ioctl, fentry/security_file_open, fentry/dma_fence_signal
 * - Input: fentry/input_event, fentry/hid_input_report, fentry/hid_irq_in
 * - Audio: fentry/snd_pcm_period_elapsed
 * - Network: fentry/netif_receive_skb, fentry/udp_rcv
 * - Wine/Proton: fentry/eventfd_signal_mask, fentry/do_futex, fentry/ntsync_*
 *
 * No heuristics. No arbitrary thresholds. Just kernel proof.
 */

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
/* BUG FIX: Reduced from 500µs to 200µs to prevent stale hints from causing
 * cross-CCX migrations at high frame rates (240Hz = 4.17ms frames).
 * At 200µs, hints from different frames are rejected, reducing cache misses
 * from unnecessary CCX hops (~100-300ns penalty per hop on AMD Ryzen). */
#define IDLE_HINT_VALID_NS		(200ULL * NSEC_PER_USEC)

/*
 * Starvation Prevention
 *
 * TIER 0: Compile-time constant
 *
 * STARVATION_THRESHOLD_NS: Maximum time a runnable task can wait without running
 * before receiving an emergency priority boost. This prevents non-game tasks
 * (audio servers, system services) from being completely starved by aggressive
 * game thread boosting.
 *
 * 500ms chosen because:
 * - Long enough to not interfere with normal game thread prioritization
 * - Short enough to prevent audio crackling (PipeWire timeout ~1s)
 * - Short enough to prevent UI freezes in background apps
 * - Aligns with kernel watchdog expectations (5s hard limit)
 *
 * EMERGENCY_BOOST_SHIFT: Temporary boost level for starved tasks (boost=4)
 * This gives them ~16x deadline reduction to ensure they run soon without
 * completely overriding input handlers (boost=7) or GPU threads (boost=6).
 */
#define STARVATION_THRESHOLD_NS		(500ULL * NSEC_PER_MSEC)
#define EMERGENCY_BOOST_SHIFT		4

/* NAPI preference tracking
 *
 * TIER 0: Compile-time constants
 */
#define NET_RX_SOFTIRQ	3
#define NET_TX_SOFTIRQ	2
#define NAPI_PREFER_TIMEOUT_NS	(5ULL * NSEC_PER_MSEC)

/*
 * Input Handling Configuration
 *
 * TIER 0: Compile-time constants
 */
#define KBD_REFRESH_THROTTLE_NS   10000000ULL	/* 10ms = 100 Hz max refresh */
#define PRE_WAKE_LEAD_NS          30000ULL	/* 30µs lead time before predicted input */
#define PRE_WAKE_WINDOW_NS        100000ULL	/* 100µs total window for pre-wake */
#define INPUT_QUEUE_DRAIN_THRESHOLD 1

/*
 * Speculative Preemption Configuration
 *
 * TIER 0: Compile-time constants
 */
#define SPECULATIVE_PREEMPT_THRESHOLD  5	/* boost_shift 0-4 are preemptable */
#define INPUT_RESERVATION_NS           50000ULL	/* 50µs CPU reservation for input */
#define SYNC_PREEMPT_THRESHOLD         5	/* Don't preempt tasks with boost >= 5 */

/*
 * Device Cache Configuration
 */
#define DEVICE_CACHE_SLOTS	32

/*
 * Sampling Intervals
 *
 * TIER 0: Compile-time constants
 */
#define UTIL_SAMPLE_INTERVAL_NS       (500ULL * NSEC_PER_USEC)	/* 0.5ms */
#define HOUSEKEEPING_INTERVAL_NS      (5ULL * NSEC_PER_MSEC)	/* 5ms */

/*
 * Input Event Types (from linux/input-event-codes.h)
 *
 * TIER 0: Compile-time constants
 */
#define EV_KEY      0x01	/* Button/key press */
#define EV_REL      0x02	/* Relative movement (mouse) */
#define EV_ABS      0x03	/* Absolute axis (analog input) */

#define KEY_RELEASE 0
#define KEY_PRESS   1
#define KEY_REPEAT  2

#define BTN_MISC    0x100

/*
 * Futex Constants
 *
 * TIER 0: Compile-time constants (for Wine/Proton sync detection)
 */
#ifndef FUTEX_CMD_MASK
#define FUTEX_CMD_MASK     0x3f
#endif
#ifndef FUTEX_WAIT
#define FUTEX_WAIT         0
#endif
#ifndef FUTEX_WAKE
#define FUTEX_WAKE         1
#endif
#ifndef FUTEX_REQUEUE
#define FUTEX_REQUEUE      3
#endif
#ifndef FUTEX_CMP_REQUEUE
#define FUTEX_CMP_REQUEUE  4
#endif
#ifndef FUTEX_WAKE_BITSET
#define FUTEX_WAKE_BITSET  10
#endif

#endif /* __GAMER_CONFIG_BPF_H */
