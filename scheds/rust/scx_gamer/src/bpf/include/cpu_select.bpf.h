/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: CPU Selection & SMT Logic
 * Copyright (c) 2025 Andrea Righi <arighi@nvidia.com>
 *
 * Idle CPU selection with physical core priority for GPU threads.
 * This file is AI-friendly: ~300 lines, single responsibility.
 *
 * KEY FEATURE: Forces GPU submission threads to physical cores (CPUs 0-7)
 * before falling back to hyperthreads (CPUs 8-15) on typical 8C/16T systems.
 */
#ifndef __GAMER_CPU_SELECT_BPF_H
#define __GAMER_CPU_SELECT_BPF_H

#include "types.bpf.h"
#include "task_class.bpf.h"

/* External tunables (from main.bpf.c rodata) */
extern const volatile bool primary_all;
extern const volatile bool flat_idle_scan;
extern const volatile bool smt_enabled;
extern const volatile bool preferred_idle_scan;
extern const volatile bool avoid_smt;
extern const volatile u64 preferred_cpus[MAX_CPUS];
extern volatile u64 interactive_sys_avg;

/* External stats counters */
extern volatile u64 nr_idle_cpu_pick;
extern volatile u64 nr_gpu_phys_kept;

/*
 * Get idle SMT mask (NUMA-aware if enabled)
 */
static inline const struct cpumask *get_idle_smtmask(s32 cpu)
{
	if (!numa_enabled)
		return scx_bpf_get_idle_smtmask();
	return __COMPAT_scx_bpf_get_idle_smtmask_node(__COMPAT_scx_bpf_cpu_node(cpu));
}

/*
 * Check if CPU is part of a fully busy SMT core
 * Returns: true if all SMT siblings are busy
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
 * Try to find an idle physical core (prefer lower CPU IDs)
 *
 * This function is critical for GPU thread performance. On typical SMT systems:
 * - Physical cores: CPUs 0 to (nr_cores - 1)
 * - Hyperthreads: CPUs nr_cores to (nr_cpus - 1)
 *
 * Returns: CPU ID >= 0 on success, -ENOENT if none found
 */
static s32 pick_idle_physical_core(const struct task_struct *p, s32 prev_cpu)
{
	const struct cpumask *allowed = p->cpus_ptr;
	u32 nr_cores = nr_cpu_ids / 2;  /* Assumes symmetric SMT */
	s32 cpu;

	/* First try prev_cpu if it's a physical core */
	if (prev_cpu >= 0 && (u32)prev_cpu < nr_cores &&
	    bpf_cpumask_test_cpu(prev_cpu, allowed) &&
	    scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
		return prev_cpu;
	}

	/* Scan physical cores (lower half of CPU IDs) */
	bpf_for(cpu, 0, nr_cores) {
		if (cpu >= MAX_CPUS)
			break;

		if (bpf_cpumask_test_cpu(cpu, allowed) &&
		    scx_bpf_test_and_clear_cpu_idle(cpu)) {
			return cpu;
		}
	}

	return -ENOENT;
}

/*
 * Pick optimal idle CPU for task @p
 *
 * Priority order:
 * 1. GPU threads: Physical cores only (if available)
 * 2. Regular tasks with avoid_smt: Full idle SMT cores
 * 3. Regular tasks: Any idle CPU
 *
 * @p: Task to schedule
 * @prev_cpu: Previous CPU task ran on
 * @wake_flags: Wakeup flags
 * @from_enqueue: Called from enqueue path (vs select_cpu)
 *
 * Returns: CPU ID >= 0, or -EBUSY if no idle CPU found
 */
static s32 pick_idle_cpu(const struct task_struct *p, s32 prev_cpu,
			 u64 wake_flags, bool from_enqueue)
{
	const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;
	struct task_ctx *tctx;
	bool is_critical_gpu;
	bool is_busy;
	bool allow_smt;
	u64 smt_flags;
	s32 cpu;

	/*
	 * Fallback to old API for kernels <= 6.16 without scx_bpf_select_cpu_and()
	 */
	if (!bpf_ksym_exists(scx_bpf_select_cpu_and)) {
		bool is_idle = false;

		if (from_enqueue)
			return -EBUSY;

		cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
		if (is_idle) {
			stat_inc(&nr_idle_cpu_pick);
			return cpu;
		}
		return -EBUSY;
	}

	/*
	 * Determine if this is a critical GPU thread requiring physical core
	 */
	tctx = try_lookup_task_ctx(p);
	is_critical_gpu = (tctx && tctx->is_gpu_submit) || is_gpu_submit_name(p->comm);

	/*
	 * CRITICAL PATH: GPU threads must use physical cores for minimal latency
	 *
	 * Problem: SCX_PICK_IDLE_CORE only picks when entire SMT core is idle.
	 * On busy systems, this causes GPU threads to land on hyperthreads.
	 *
	 * Solution: Explicitly scan physical cores first, accepting busy siblings.
	 */
	if (is_critical_gpu && smt_enabled) {
		cpu = pick_idle_physical_core(p, prev_cpu);
		if (cpu >= 0) {
			stat_inc(&nr_idle_cpu_pick);
			stat_inc(&nr_gpu_phys_kept);
			return cpu;
		}
		/* If no physical core available, fall through to normal path */
	}

	/*
	 * For non-GPU threads, apply normal SMT avoidance logic
	 */
	is_busy = interactive_sys_avg >= INTERACTIVE_SLICE_SHRINK_THRESH;
	allow_smt = is_critical_gpu ? false :
		    (!avoid_smt || (!is_busy && interactive_sys_avg < INTERACTIVE_SMT_ALLOW_THRESH));
	smt_flags = allow_smt ? 0 : SCX_PICK_IDLE_CORE;

	/*
	 * Try primary domain first (if configured)
	 */
	if (primary && !primary_all) {
		cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, primary, smt_flags);
		if (cpu >= 0) {
			stat_inc(&nr_idle_cpu_pick);
			return cpu;
		}
	}

	/*
	 * Pick any idle CPU from task's allowed mask
	 */
	cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, p->cpus_ptr, smt_flags);
	if (cpu >= 0)
		stat_inc(&nr_idle_cpu_pick);

	return cpu;
}

#endif /* __GAMER_CPU_SELECT_BPF_H */
