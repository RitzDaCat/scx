/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Dispatch Macros
 * 
 * DRY helper macros for safe, affinity-aware task dispatching.
 * These macros handle common edge cases:
 * - migration_disabled race conditions
 * - CPU affinity changes (Wine/Proton games)
 * - PRIQ/FIFO DSQ mode conflicts
 * 
 * Copyright (c) 2025 RitzDaCat
 */
#ifndef __DISPATCH_MACROS_BPF_H
#define __DISPATCH_MACROS_BPF_H

/* ═══════════════════════════════════════════════════════════════════════════
 * SMT TOPOLOGY HELPERS
 * 
 * On x86, physical cores are even-numbered (0, 2, 4, ...) and SMT threads
 * are odd-numbered (1, 3, 5, ...). These macros abstract the bit math.
 * ═══════════════════════════════════════════════════════════════════════════ */
#define SMT_SIBLING(cpu)      (((cpu) & 1) ? ((cpu) - 1) : ((cpu) + 1))
#define PHYSICAL_CORE(cpu)    (((u32)(cpu)) & 0xFE)
#define IS_SMT_THREAD(cpu)    ((cpu) & 1)
#define IS_PHYSICAL_CORE(cpu) (!((cpu) & 1))

/* ═══════════════════════════════════════════════════════════════════════════
 * CPU AFFINITY CHECK
 * ═══════════════════════════════════════════════════════════════════════════ */
#define CAN_RUN_ON(p, cpu)    bpf_cpumask_test_cpu((cpu), (p)->cpus_ptr)

/* ═══════════════════════════════════════════════════════════════════════════
 * DISPATCH_SAFE: The One True Dispatch Pattern
 * 
 * ALWAYS use this macro for dispatching. It handles THREE safety issues:
 * 
 * 1. migration_disabled race: Task might call migrate_disable() after our
 *    check but before kernel dispatch. Solution: dispatch to prev_cpu.
 * 
 * 2. PRIQ/FIFO DSQ conflict: SHARED_DSQ uses PRIQ mode, local DSQs use FIFO.
 *    Solution: local DSQ for valid prev_cpu, shared DSQ with vtime for fallback.
 * 
 * 3. CPU affinity change: Task's cpus_ptr might exclude prev_cpu (Wine/Proton
 *    games change thread affinity dynamically). Solution: check CAN_RUN_ON.
 * 
 * Hot path (99%+): prev_cpu is valid -> local DSQ (FIFO, ~0ns)
 * Cold path (rare): prev_cpu invalid -> shared DSQ (PRIQ, ~50ns)
 * ═══════════════════════════════════════════════════════════════════════════ */
#define DISPATCH_SAFE(p, target_cpu, prev_cpu, slice, flags) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			/* Hot path: prev_cpu is allowed - use local DSQ (FIFO) */ \
			scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), (flags)); \
		} else { \
			/* Cold path: prev_cpu not allowed - use shared DSQ (PRIQ) */ \
			scx_bpf_dsq_insert_vtime((p), shared_dsq(prev_cpu), (slice), \
						 scx_bpf_now(), (flags)); \
		} \
		/* Kick target_cpu for A.B.C - prepares it for future work */ \
		scx_bpf_kick_cpu((target_cpu), SCX_KICK_IDLE); \
	} while (0)

/* Legacy alias for compatibility */
#define SAFE_DISPATCH_TO_CPU(p, target_cpu, prev_cpu, slice, flags) \
	DISPATCH_SAFE(p, target_cpu, prev_cpu, slice, flags)

/* ═══════════════════════════════════════════════════════════════════════════
 * DISPATCH_LOCAL_*: Affinity-safe local dispatch macros
 * 
 * These macros check CPU affinity before dispatching to local DSQ.
 * If prev_cpu is not in task's affinity mask, falls back to shared DSQ.
 * ═══════════════════════════════════════════════════════════════════════════ */

/* Dispatch to prev_cpu when it's idle - with affinity check */
#define DISPATCH_LOCAL_IDLE(p, prev_cpu, slice) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), 0); \
			scx_bpf_kick_cpu((prev_cpu), SCX_KICK_IDLE); \
		} else { \
			scx_bpf_dsq_insert_vtime((p), shared_dsq(prev_cpu), (slice), \
						 scx_bpf_now(), 0); \
		} \
	} while (0)

/* Dispatch to prev_cpu with smart preemption - with affinity check */
#define DISPATCH_LOCAL_PREEMPT(p, prev_cpu, slice, boost) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), 0); \
			smart_kick_cpu((prev_cpu), (boost)); \
		} else { \
			scx_bpf_dsq_insert_vtime((p), shared_dsq(prev_cpu), (slice), \
						 scx_bpf_now(), 0); \
		} \
	} while (0)

/* Try dispatch to prev_cpu if idle - with affinity check
 * Returns early via success_stmt if dispatched, otherwise continues */
#define TRY_DISPATCH_IF_IDLE(p, prev_cpu, slice, success_stmt) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) { \
				scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), 0); \
				scx_bpf_kick_cpu((prev_cpu), SCX_KICK_IDLE); \
				success_stmt; \
			} \
		} \
	} while (0)

/* ═══════════════════════════════════════════════════════════════════════════
 * DISPATCH_CHECKED: Universal affinity-safe dispatch (DRY helper)
 * 
 * Use this for ALL dispatch calls. Checks CPU affinity before dispatching
 * to local DSQ. Falls back to shared DSQ with PRIQ if affinity disallows.
 * ═══════════════════════════════════════════════════════════════════════════ */
#define DISPATCH_CHECKED(p, prev_cpu, slice, flags) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), (flags)); \
			scx_bpf_kick_cpu((prev_cpu), SCX_KICK_IDLE); \
		} else { \
			scx_bpf_dsq_insert_vtime((p), shared_dsq(prev_cpu), (slice), \
						 scx_bpf_now(), (flags)); \
		} \
	} while (0)

/* Affinity-safe insert WITHOUT kick (for custom kick callers) */
#define DISPATCH_INSERT_ONLY(p, prev_cpu, slice, flags) \
	do { \
		if (likely(bpf_cpumask_test_cpu((prev_cpu), (p)->cpus_ptr))) { \
			scx_bpf_dsq_insert((p), SCX_DSQ_LOCAL_ON | (prev_cpu), (slice), (flags)); \
		} else { \
			scx_bpf_dsq_insert_vtime((p), shared_dsq(prev_cpu), (slice), \
						 scx_bpf_now(), (flags)); \
		} \
	} while (0)

/* Affinity-safe insert + regular wakeup_cpu() */
#define DISPATCH_AND_WAKE(p, prev_cpu, slice, flags) \
	do { \
		DISPATCH_INSERT_ONLY(p, prev_cpu, slice, flags); \
		wakeup_cpu(prev_cpu); \
	} while (0)

/* Affinity-safe insert + wakeup_cpu_for_input() for input-critical paths */
#define DISPATCH_AND_WAKE_INPUT(p, prev_cpu, slice, flags) \
	do { \
		DISPATCH_INSERT_ONLY(p, prev_cpu, slice, flags); \
		wakeup_cpu_for_input(prev_cpu); \
	} while (0)

/* ═══════════════════════════════════════════════════════════════════════════
 * DRY HELPER MACROS: Reduce code duplication in hot paths
 * ═══════════════════════════════════════════════════════════════════════════ */

/* Check if timestamp is within time window
 * Replaces: (time != 0 && (now - time) < window) */
#define IS_WITHIN_WINDOW(now, timestamp, window_ns) \
	((timestamp) != 0 && ((now) - (timestamp)) < (window_ns))

/* Verify task hasn't been recycled via cookie validation */
#define TASK_COOKIE_VALID(p, tctx) \
	({ \
		u64 _cookie = BPF_CORE_READ(p, start_time); \
		(_cookie && (tctx)->task_cookie == _cookie); \
	})

/* Force-dispatch and return from enqueue */
#define FORCE_DISPATCH_RETURN(p, prev_cpu, flags) \
	do { \
		DISPATCH_AND_WAKE(p, prev_cpu, task_slice(p), flags); \
		PROF_END_HIST(enqueue); \
		return; \
	} while (0)

/* Force-dispatch with input wake and return */
#define FORCE_DISPATCH_INPUT_RETURN(p, prev_cpu, flags) \
	do { \
		DISPATCH_AND_WAKE_INPUT(p, prev_cpu, task_slice(p), flags); \
		PROF_END_HIST(enqueue); \
		return; \
	} while (0)

#endif /* __DISPATCH_MACROS_BPF_H */

