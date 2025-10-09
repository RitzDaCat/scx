/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: BPF LSM Game Detection
 * Copyright (c) 2025 RitzDaCat
 *
 * Kernel-level game process tracking using LSM hooks.
 * Replaces expensive userspace /proc scanning with event-driven detection.
 *
 * Performance: ~20-50μs/sec CPU overhead (vs 10-50ms/sec with /proc polling)
 * Detection latency: <1ms (vs 0-100ms with inotify)
 *
 * NOTE: This file is included into main.bpf.c and relies on its headers
 * (scx/common.bpf.h provides vmlinux.h, bpf_helpers, bpf_core_read, etc.)
 */

#include "include/game_detect.bpf.h"

/*
 * Ring buffer for sending process events to userspace
 * Size: 256KB = ~4000 events buffered
 */
struct {
	__uint(type, BPF_MAP_TYPE_RINGBUF);
	__uint(max_entries, 256 * 1024);
} process_events SEC(".maps");

/*
 * Current game PID tracked by userspace
 * Single-entry array for atomic read/write from userspace
 */
struct {
	__uint(type, BPF_MAP_TYPE_ARRAY);
	__uint(max_entries, 1);
	__type(key, u32);
	__type(value, u32);
} current_game_map SEC(".maps");

/*
 * Statistics: events processed and sent
 */
volatile u64 lsm_exec_count;      /* Total exec hooks fired */
volatile u64 lsm_exit_count;      /* Total exit hooks fired */
volatile u64 lsm_events_sent;     /* Events sent to userspace */
volatile u64 lsm_events_dropped;  /* Events dropped (ring buffer full) */

/*
 * LSM Hook: Process Execution
 *
 * Fires when a process calls exec() to replace its program image.
 * This catches game launches, Wine exec chains, and Steam game starts.
 *
 * Critical path: NO - runs after security checks, not in scheduling path
 * Overhead: 200-800ns per exec (mostly string operations)
 * Frequency: 50-200 execs/sec on busy system
 * Events sent: 1-10/sec (90-95% filtered in kernel)
 */
SEC("lsm/bprm_committed_creds")
int BPF_PROG(game_detect_exec, struct linux_binprm *bprm)
{
	struct task_struct *task, *parent;
	struct process_event *evt;
	char comm[16] = {0};
	char parent_comm[16] = {0};
	u32 flags = 0;
	u32 pid, parent_pid = 0;

	__sync_fetch_and_add(&lsm_exec_count, 1);

	task = bpf_get_current_task_btf();
	if (!task)
		return 0;

	/* Read process name (task->comm is always in kernel memory) */
	bpf_probe_read_kernel_str(comm, sizeof(comm), task->comm);

	/* FAST PATH: Filter out obvious system binaries
	 * Rejects 80-90% of processes in <50ns
	 * Reduces ring buffer traffic and userspace overhead */
	if (is_system_binary(comm))
		return 0;

	/* Read PID */
	pid = BPF_CORE_READ(task, tgid);

	/* Classify based on comm keywords */
	flags |= classify_comm(comm);

	/* Read parent process for Wine/Steam detection
	 * Wine games typically have: wine-preloader → game.exe
	 * Steam games have: reaper → pressure-vessel-* → game */
	parent = BPF_CORE_READ(task, real_parent);
	if (parent) {
		parent_pid = BPF_CORE_READ(parent, tgid);
		bpf_probe_read_kernel_str(parent_comm, sizeof(parent_comm), parent->comm);

		/* Check parent for Wine/Steam */
		if (contains_substr(parent_comm, "wine", 16, 4) ||
		    contains_substr(parent_comm, "proton", 16, 6))
			flags |= FLAG_PARENT_WINE;

		if (contains_substr(parent_comm, "steam", 16, 5) ||
		    contains_substr(parent_comm, "reaper", 16, 6))
			flags |= FLAG_PARENT_STEAM;
	}

	/* CONSERVATIVE FILTERING: Send to userspace if ANY game indicator present
	 * Userspace does deep classification (cmdline, cgroup, memory, threads)
	 * This approach prioritizes accuracy over kernel filtering efficiency */
	if (flags == 0)
		return 0;  /* No game indicators, ignore */

	/* Reserve space in ring buffer */
	evt = bpf_ringbuf_reserve(&process_events, sizeof(*evt), 0);
	if (!evt) {
		/* Ring buffer full (extremely rare) */
		__sync_fetch_and_add(&lsm_events_dropped, 1);
		return 0;
	}

	/* Populate event */
	evt->type = GAME_EVENT_EXEC;
	evt->pid = pid;
	evt->parent_pid = parent_pid;
	evt->flags = flags;
	evt->timestamp = bpf_ktime_get_ns();
	__builtin_memcpy(evt->comm, comm, sizeof(comm));
	__builtin_memcpy(evt->parent_comm, parent_comm, sizeof(parent_comm));

	/* Submit to userspace (zero-copy handoff) */
	bpf_ringbuf_submit(evt, 0);
	__sync_fetch_and_add(&lsm_events_sent, 1);

	return 0;
}

/*
 * LSM Hook: Process Termination
 *
 * Fires when a process exits (task_struct being freed).
 * Used to detect when the tracked game closes.
 *
 * Critical path: NO - runs during process cleanup
 * Overhead: 100-300ns (mostly map lookup)
 * Frequency: 50-200 exits/sec, but only 1 event sent (tracked game only)
 */
SEC("lsm/task_free")
int BPF_PROG(game_detect_exit, struct task_struct *task)
{
	struct process_event *evt;
	u32 pid, zero = 0;
	u32 *current_game;

	__sync_fetch_and_add(&lsm_exit_count, 1);

	if (!task)
		return 0;

	pid = BPF_CORE_READ(task, tgid);

	/* Check if this is the currently tracked game */
	current_game = bpf_map_lookup_elem(&current_game_map, &zero);
	if (!current_game || *current_game != pid)
		return 0;  /* Not our game, ignore */

	/* Game exited! Notify userspace immediately */
	evt = bpf_ringbuf_reserve(&process_events, sizeof(*evt), 0);
	if (!evt) {
		__sync_fetch_and_add(&lsm_events_dropped, 1);
		return 0;
	}

	evt->type = GAME_EVENT_EXIT;
	evt->pid = pid;
	evt->parent_pid = 0;
	evt->flags = 0;
	evt->timestamp = bpf_ktime_get_ns();
	bpf_probe_read_kernel_str(evt->comm, sizeof(evt->comm), task->comm);

	bpf_ringbuf_submit(evt, 0);
	__sync_fetch_and_add(&lsm_events_sent, 1);

	return 0;
}
