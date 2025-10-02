/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Migration Limiter (Token Bucket)
 * Copyright (c) 2025 RitzDaCat
 *
 * Rate-limits task migrations to reduce cache thrashing.
 * This file is AI-friendly: ~180 lines, single responsibility.
 */
#ifndef __GAMER_MIGRATION_BPF_H
#define __GAMER_MIGRATION_BPF_H

#include "config.bpf.h"
#include "types.bpf.h"

/* External tunables */
extern const volatile bool smt_enabled;
extern const volatile bool avoid_smt;
extern const volatile bool numa_enabled;
extern const volatile u64 mig_window_ns;
extern const volatile u32 mig_max_per_window;
extern const volatile u32 numa_spill_thresh;

/* External stats */
extern volatile u64 nr_mig_blocked;
extern volatile u64 nr_frame_mig_block;

/* External helpers */
extern const struct cpumask *get_idle_smtmask(s32 cpu);
extern bool is_pcpu_task(const struct task_struct *p);
extern u64 shared_dsq(s32 cpu);

/*
 * Check if CPU is part of a fully busy SMT core
 *
 * Returns true if all SMT siblings are busy (no idle siblings).
 * Used to force migration away from contended cores.
 */
static bool is_smt_contended(s32 cpu)
{
	const struct cpumask *smt;
	bool is_contended;

	if (!smt_enabled || !avoid_smt)
		return false;

	smt = get_idle_smtmask(cpu);
	is_contended = bpf_cpumask_empty(smt);
	scx_bpf_put_cpumask(smt);

	return is_contended;
}

/*
 * Refill migration tokens using token bucket algorithm
 *
 * Tokens refill over time, allowing bursts of migrations
 * while preventing sustained high migration rates.
 *
 * @tctx: Task context containing token state
 * @now: Current timestamp
 *
 * Formula:
 *   tokens_to_add = (elapsed / window) * max_tokens
 *
 * Implementation uses overflow-safe arithmetic to prevent
 * intermediate multiplication overflow.
 */
static void refill_migration_tokens(struct task_ctx *tctx, u64 now)
{
	u64 max_tokens = mig_max_per_window * MIG_TOKEN_SCALE;
	u64 elapsed, full_windows, remainder_ns, add;

	/* Initialize refill timestamp if needed */
	if (!tctx->mig_last_refill || tctx->mig_last_refill > now) {
		tctx->mig_last_refill = now;
		return;
	}

	/* Already at max tokens */
	if (tctx->mig_tokens >= max_tokens)
		return;

	/* Calculate elapsed time */
	elapsed = now - tctx->mig_last_refill;
	if (elapsed == 0)
		return;

	/* Full refill if elapsed > 2 * window */
	if (elapsed > mig_window_ns * 2) {
		tctx->mig_tokens = max_tokens;
		tctx->mig_last_refill = now;
		return;
	}

	/* Overflow-safe token calculation */
	full_windows = elapsed / mig_window_ns;
	remainder_ns = elapsed % mig_window_ns;

	/* Tokens from full windows (no overflow, max_tokens is u32) */
	add = full_windows * max_tokens;

	/* Fractional tokens from remainder */
	if (remainder_ns && mig_window_ns) {
		/* Scale down to prevent overflow in multiplication */
		u64 scale = (mig_window_ns >> 20) ? (mig_window_ns >> 20) : 1;
		u64 scaled_rem = remainder_ns / scale;
		u64 scaled_win = mig_window_ns / scale;
		if (scaled_win > 0)
			add += (scaled_rem * max_tokens) / scaled_win;
	}

	/* Add tokens (capped at max) */
	if (add) {
		tctx->mig_tokens = MIN(tctx->mig_tokens + add, max_tokens);
		tctx->mig_last_refill = now;
	}
}

/*
 * Check if task should attempt migration to idle CPU
 *
 * Returns true if migration is allowed, false if blocked.
 *
 * Migration is always allowed if:
 * - SMT core is fully contended
 * - Task is being re-enqueued (preempted)
 *
 * Otherwise, migration is rate-limited using token bucket.
 */
static bool need_migrate(const struct task_struct *p, s32 prev_cpu, u64 enq_flags, bool is_busy)
{
	struct task_ctx *tctx;
	u64 now;

	/* Per-CPU tasks cannot migrate */
	if (is_pcpu_task(p))
		return false;

	/* Always migrate if SMT core is fully contended */
	if (is_smt_contended(prev_cpu))
		return true;

	/* Check if migration attempt is needed */
	if ((!__COMPAT_is_enq_cpu_selected(enq_flags) && !scx_bpf_task_running(p)) ||
	    (enq_flags & SCX_ENQ_REENQ)) {

		tctx = try_lookup_task_ctx(p);
		if (!tctx)
			return true;  /* Allow migration if no context */

		now = scx_bpf_now();

		/* Token bucket rate limiting */
		if (mig_window_ns && mig_max_per_window) {
			refill_migration_tokens(tctx, now);

			/* Check if we have tokens available */
			if (tctx->mig_tokens < MIG_TOKEN_SCALE) {
				/* No tokens: block migration */
				stat_inc(&nr_mig_blocked);
				return false;
			}

			/* Consume one token */
			tctx->mig_tokens -= MIG_TOKEN_SCALE;
		}

		/* NUMA-aware migration: avoid spilling if local DSQ is not saturated */
		if (numa_enabled && is_busy && numa_spill_thresh) {
			u64 depth = scx_bpf_dsq_nr_queued(shared_dsq(prev_cpu));
			if (depth < numa_spill_thresh)
				return false;
		}

		return true;
	}

	return false;
}

/*
 * Check if migration should be blocked during frame-critical periods
 *
 * During input/frame windows, we may want to block migrations for
 * critical threads to preserve cache affinity.
 *
 * Returns true if migration should be blocked.
 */
static bool should_block_frame_migration(const struct task_struct *p,
					 struct task_ctx *tctx,
					 bool in_input_window)
{
	/* GPU threads: prefer cache affinity during critical periods */
	if (tctx && tctx->is_gpu_submit && in_input_window) {
		stat_inc(&nr_frame_mig_block);
		return true;
	}

	return false;
}

#endif /* __GAMER_MIGRATION_BPF_H */
