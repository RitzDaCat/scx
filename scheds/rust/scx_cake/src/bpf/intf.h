/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_cake — shared interface between the BPF scheduler and the Rust loader.
 * DESIGN.md has the design; HYPOTHESES.md §S the measurements behind these.
 *
 * This software may be used and distributed according to the terms of the GNU
 * General Public License version 2.
 */
#ifndef __CAKE_INTF_H
#define __CAKE_INTF_H

#ifndef __VMLINUX_H__
typedef unsigned char u8;
typedef unsigned short u16;
typedef unsigned int u32;
typedef unsigned long u64;

typedef signed char s8;
typedef signed short s16;
typedef signed int s32;
typedef signed long s64;
#endif /* __VMLINUX_H__ */

/*
 * Build-host topology, detected by build.rs. The ONLY definitions a cflag may
 * set: policy is source-only so an A/B is two commits, not two flags (§S.6).
 */
#ifndef CAKE_NR_CCDS
#define CAKE_NR_CCDS 1
#endif
#ifndef CAKE_NR_CPUS
#define CAKE_NR_CPUS 1024
#endif

/* Multi-CCD steal order: 0 off, 1 same-CCD first, 2 also group cache tiers. */
#define CAKE_CCD_STEAL_POLICY 2

enum consts {
	NSEC_PER_USEC	= 1000,
	NSEC_PER_MSEC	= (1000 * NSEC_PER_USEC),

	/* The slice every task gets. Dose-responsed U-curve minimum (§S.1). */
	SLICE_NS	= 3000 * NSEC_PER_USEC,

	/*
	 * SCHEDULING-RELATIVE policy: slice divisors are correct here because
	 * each measures turns of service behind, the occupant's used turn, or
	 * the margin it earns back. Hardware-anchored values belong in
	 * cake.bpf.c rodata instead (§S.2).
	 */
	SLEEPER_LAG_NS			= SLICE_NS / 2,
	HOME_PREEMPT_YOUNG_NS		= SLICE_NS / 32,
	HOME_PREEMPT_BASE_MARGIN_NS	= SLICE_NS / 2,
	DEEP_WAKE_HYSTERESIS_NS		= 1 * SLICE_NS,
	HOME_PREEMPT_RAN_CREDIT_SHIFT	= 1,

	/* Occupant protection as a fraction of a FRAME (§G11.4). */
	FRAME_PREEMPT_PROTECT_SHIFT	= 4,
	FRAME_PROBE_PROTECT_SHIFT	= 2,
	FRAME_SLICE_CAP_SHIFT		= 1,

	/* Pre-scale for the wait:run cross-multiply; it cancels (§G12). */
	CAKE_RATIO_SHIFT		= 16,

	/*
	 * WALL-CLOCK starvation bound for the global wake queue, ~3 frames at
	 * 120 Hz: a vtime bound cannot bound a wall-clock stall (§S.4).
	 */
	WAKE_STARVE_WALL_NS		= 24 * NSEC_PER_MSEC,
	WAKE_STARVE_REFRESH_NS		= WAKE_STARVE_WALL_NS / 2,

	/*
	 * FRAME CLOCK plausibility band: one second over the fastest and
	 * slowest rate a display runs at. A clamp, not a knob (§G11).
	 */
	FRAME_HZ_MAX			= 500,
	FRAME_HZ_MIN			= 25,
	FRAME_PERIOD_MIN_NS		= 1000 * NSEC_PER_MSEC / FRAME_HZ_MAX,
	FRAME_PERIOD_MAX_NS		= 1000 * NSEC_PER_MSEC / FRAME_HZ_MIN,
	/* Vote buckets spanning that band; width need only separate cadences. */
	FRAME_BUCKET_SHIFT		= 17,
	FRAME_BUCKETS			= 512,

	NR_CCDS				= CAKE_NR_CCDS,
	BUILD_NR_CPUS			= CAKE_NR_CPUS,
	CCD_STEAL_POLICY		= CAKE_CCD_STEAL_POLICY,

	/* Fixed-point weight scaling: representation, not policy (§S.7). */
	RECIP_SHIFT		= 20,
	RECIP_ONE		= 1 << RECIP_SHIFT,
	RECIP_MASK		= RECIP_ONE - 1,
	STATIC_PRIO_BASE	= 100,
	RECIP_TABLE_SIZE	= 64,
	RECIP_INDEX_MASK	= RECIP_TABLE_SIZE - 1,
	IDLE_RECIP_INDEX	= 40,
	MAX_RECIP_WEIGHT	= 357913941,

	/* Cache-isolated mutable-state slot geometry (§R.10). */
	STATE_SLOT_BYTES	= 128,
	STATE_SLOT_WORDS	= STATE_SLOT_BYTES / sizeof(u64),

	/* Runnable-stall safety watchdog, not a scheduling threshold. */
	WATCHDOG_TIMEOUT_MS	= 5 * 1000,

	/*
	 * Verifier sizing bound for per-CPU state and the steal loops, a power
	 * of 2 so hot-path indexes reduce to a mask. NOT the DSQ count. WAKE_DSQ
	 * sits one id above that range and holds wakeups, so the FIRST CPU to
	 * block anywhere finds them; MAX_CPUS + 1 is retired, not recycled (§S.7).
	 */
	MAX_CPUS	= 1024,
	WAKE_DSQ	= MAX_CPUS,

	/* The steal-ring queue hint, one bit per CPU (§G25). */
	QMASK_WORDS	= MAX_CPUS / 64,
};

/* DIAGNOSTIC ONLY (§G25 census) — revert with the commit that adds it. */
enum cake_census_slot {
	CENSUS_RING_CALLS = 0,	/* cake_ring_steal entered */
	CENSUS_RING_STEPS,	/* ring iterations executed */
	CENSUS_RING_HITS,	/* moved work local */
	CENSUS_WORD_LOADS,	/* qmask word actually loaded in the walk */
	CENSUS_SET_CALLS,	/* cake_qmark_set entered */
	CENSUS_SET_ATOMIC,	/* ... and the bit really changed */
	CENSUS_CLR_CALLS,	/* cake_qmark_clear entered */
	CENSUS_CLR_ATOMIC,	/* ... and the bit really changed */
	CENSUS_SLOTS,
};

#endif /* __CAKE_INTF_H */
