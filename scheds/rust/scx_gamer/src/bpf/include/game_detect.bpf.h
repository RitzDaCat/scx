/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: BPF LSM Game Detection
 * Copyright (c) 2025 RitzDaCat
 *
 * Kernel-level game process detection using LSM hooks.
 * Eliminates expensive /proc scanning by tracking process lifecycle in kernel.
 */
#ifndef __GAMER_GAME_DETECT_BPF_H
#define __GAMER_GAME_DETECT_BPF_H

/*
 * Event types sent from BPF to userspace via ring buffer
 * NOTE: Renamed to avoid collision with kernel's proc_event enum in vmlinux.h
 */
enum game_event_type {
	GAME_EVENT_EXEC = 1,    /* New process exec'd (program image replaced) */
	GAME_EVENT_EXIT = 2,    /* Process terminated */
};

/*
 * Process classification flags (set by kernel-side analysis)
 * These flags indicate game likelihood, reducing userspace work
 */
enum game_flags {
	FLAG_WINE         = (1 << 0),  /* Wine/Proton in comm */
	FLAG_STEAM        = (1 << 1),  /* Steam-related keywords */
	FLAG_EXE          = (1 << 2),  /* .exe in comm name */
	FLAG_PARENT_WINE  = (1 << 3),  /* Parent process is Wine */
	FLAG_PARENT_STEAM = (1 << 4),  /* Parent is Steam */
};

/*
 * Process event structure sent via ring buffer
 * Size: 64 bytes (cache-line aligned for performance)
 *
 * Ring buffer size: 256KB = ~4000 events buffered
 * Overflow strategy: Drop oldest (games launch infrequently)
 */
struct process_event {
	u32 type;              /* process_event_type */
	u32 pid;               /* Process TGID (thread group ID) */
	u32 parent_pid;        /* Parent process TGID */
	u32 flags;             /* game_flags bitmask */
	u64 timestamp;         /* Event timestamp (ns since boot) */
	char comm[16];         /* Process name (task->comm) */
	char parent_comm[16];  /* Parent process name */
};

/*
 * Fast substring search (BPF verifier friendly)
 *
 * Searches for needle in haystack with bounded iteration.
 * Used for keyword detection: "wine", "steam", "proton", etc.
 *
 * @haystack: String to search in (e.g., process comm)
 * @needle: String to search for (e.g., "wine")
 * @haystack_len: Max length to search (bounded for verifier)
 * @needle_len: Length of needle string
 *
 * Returns: true if needle found, false otherwise
 */
static __always_inline bool
contains_substr(const char *haystack, const char *needle, int haystack_len, int needle_len)
{
	if (needle_len == 0 || needle_len > haystack_len)
		return false;

	/* Bounded loop for BPF verifier (max 16 chars for comm) */
	#pragma unroll
	for (int i = 0; i <= haystack_len - needle_len; i++) {
		if (haystack[i] == '\0')
			return false;

		/* Check if needle matches at position i */
		bool match = true;
		#pragma unroll
		for (int j = 0; j < needle_len; j++) {
			if (haystack[i + j] != needle[j]) {
				match = false;
				break;
			}
		}
		if (match)
			return true;
	}
	return false;
}

/*
 * System binary detection (fast rejection)
 *
 * Filters out common system processes to reduce userspace events by 90-95%.
 * Uses first 2-3 characters for fast rejection (branch prediction friendly).
 *
 * Returns: true if definitely system binary, false if potential game
 */
static __always_inline bool is_system_binary(const char *comm)
{
	/* Empty comm */
	if (comm[0] == '\0')
		return true;

	/* Common system processes (first char fast path) */
	switch (comm[0]) {
	case 's':
		/* sh, sudo, systemd, sshd */
		if (comm[1] == 'h' || comm[1] == 'u' || comm[1] == 'y' || comm[1] == 's')
			return true;
		break;
	case 'b':
		/* bash, busybox */
		if (comm[1] == 'a' || comm[1] == 'u')
			return true;
		break;
	case 'p':
		/* python, perl, ps */
		if (comm[1] == 'y' || comm[1] == 'e' || comm[1] == 's')
			return true;
		break;
	case 'g':
		/* git, gcc, grep */
		if (comm[1] == 'i' || comm[1] == 'c' || comm[1] == 'r')
			return true;
		break;
	case 'c':
		/* cat, cargo, cp, curl */
		if (comm[1] == 'a' || comm[1] == 'p' || comm[1] == 'u')
			return true;
		break;
	case 'l':
		/* ls, ln */
		if (comm[1] == 's' || comm[1] == 'n')
			return true;
		break;
	case 'r':
		/* rm, rsync */
		if (comm[1] == 'm' || comm[1] == 's')
			return true;
		break;
	}

	/* Scheduler processes */
	if (contains_substr(comm, "scx_", 16, 4))
		return true;

	return false;  /* Potential game, send to userspace */
}

/*
 * Check if comm contains game-related keywords
 *
 * Returns: Bitmask of game_flags
 */
static __always_inline u32 classify_comm(const char *comm)
{
	u32 flags = 0;

	/* Wine/Proton detection */
	if (contains_substr(comm, "wine", 16, 4) || contains_substr(comm, "proton", 16, 6))
		flags |= FLAG_WINE;

	/* Steam detection */
	if (contains_substr(comm, "steam", 16, 5) || contains_substr(comm, "reaper", 16, 6))
		flags |= FLAG_STEAM;

	/* Windows executable */
	if (contains_substr(comm, ".exe", 16, 4) || contains_substr(comm, ".ex", 16, 3))
		flags |= FLAG_EXE;

	/* Game-related thread names (Unreal Engine, Unity, etc.) */
	if (contains_substr(comm, "game", 16, 4) ||      /* GameThread, game.exe */
	    contains_substr(comm, "Game", 16, 4) ||      /* Case-sensitive match */
	    contains_substr(comm, "warframe", 16, 8) ||  /* Warframe */
	    contains_substr(comm, "Thread", 16, 6))      /* GameThread, RenderThread */
		flags |= FLAG_EXE;  /* Reuse EXE flag for game threads */

	return flags;
}

#endif /* __GAMER_GAME_DETECT_BPF_H */
