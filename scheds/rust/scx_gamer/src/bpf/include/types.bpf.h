/* SPDX-License-Identifier: GPL-2.0 */
/*
 * types.bpf.h - Data structures and BPF maps
 *
 * Defines:
 * - task_ctx: Per-task scheduling context (64 bytes, cache-aligned)
 * - cpu_ctx: Per-CPU context (64 bytes, cache-aligned)
 * - gamer_stats: Global statistics structure
 * - BPF maps for storing contexts
 *
 * Design principle: All hot-path data fits in a single cache line (64 bytes).
 */

#ifndef __TYPES_BPF_H
#define __TYPES_BPF_H

#include "config.bpf.h"

/* ============================================================================
 * SECTION 1: TASK CONTEXT
 * ============================================================================
 *
 * Per-task scheduling context. Stored in BPF_MAP_TYPE_TASK_STORAGE.
 * Size: 64 bytes (single cache line)
 * Alignment: 64-byte aligned
 */

struct task_ctx {
    /* === CACHE LINE 1 (64 bytes) === */
    
    /* Byte 0: Classification flags (hot - read every select_cpu) */
    u8 flags;
    
    /* Byte 1: Current priority boost (0-7) */
    u8 boost_shift;
    
    /* Bytes 2-3: Reserved for future flags */
    u16 _reserved_flags;
    
    /* Bytes 4-7: Preferred CPU cache (-1 = no preference) */
    s32 preferred_cpu;
    
    /* Bytes 8-15: Runtime since last sleep (for deadline calculation)
     * Reset in ops.runnable(), accumulated in ops.stopping()
     * Capped at SLICE_LAG_NS to prevent unbounded credit */
    u64 exec_runtime;
    
    /* Bytes 16-23: Last dispatch timestamp */
    u64 last_run_ns;
    
    /* Bytes 24-31: Cumulative runtime (for statistics only) */
    u64 sum_runtime_ns;
    
    /* Bytes 32-35: Last CPU ran on */
    s32 last_cpu;
    
    /* Bytes 36-39: Process group ID (for game detection) */
    u32 tgid;
    
    /* Bytes 40-47: Timestamp of last classification */
    u64 classified_at_ns;
    
    /* Bytes 48-55: Timestamp of last wakeup (for wakeup frequency) */
    u64 last_woke_at;
    
    /* Bytes 56-63: Reserved for future use */
    u64 _reserved;
    
} __attribute__((aligned(64)));

/* Flag test helpers */
#define IS_GAME(tctx)       ((tctx)->flags & FLAG_GAME)
#define IS_INPUT(tctx)      ((tctx)->flags & FLAG_INPUT)
#define IS_GPU(tctx)        ((tctx)->flags & FLAG_GPU)
#define IS_AUDIO(tctx)      ((tctx)->flags & FLAG_AUDIO)
#define IS_COMPOSITOR(tctx) ((tctx)->flags & FLAG_COMPOSITOR)
#define IS_LATENCY_CRITICAL(tctx) ((tctx)->flags & FLAGS_LATENCY_CRITICAL)

/* ============================================================================
 * SECTION 2: CPU CONTEXT
 * ============================================================================
 *
 * Per-CPU context. Stored in BPF_MAP_TYPE_ARRAY.
 * Size: 64 bytes (single cache line)
 * Alignment: 64-byte aligned
 */

struct cpu_ctx {
    /* === CACHE LINE 1 (64 bytes) === */
    
    /* Bytes 0-7: Timestamp of last input event routed here */
    u64 last_input_ns;
    
    /* Bytes 8-11: Boost level of currently running task */
    u32 current_boost;
    
    /* Bytes 12-15: CPU flags */
    u32 flags;
    
    /* Bytes 16-19: Physical core ID (for SMT pairing) */
    s32 core_id;
    
    /* Bytes 20-23: SMT sibling CPU ID (-1 if no sibling) */
    s32 sibling_cpu;
    
    /* Bytes 24-27: NUMA node ID */
    u32 node_id;
    
    /* Bytes 28-31: Reserved */
    u32 _reserved0;
    
    /* Bytes 32-63: Reserved for future use */
    u64 _reserved[4];
    
} __attribute__((aligned(64)));

/* ============================================================================
 * SECTION 3: STATISTICS
 * ============================================================================
 *
 * Global statistics. Updated atomically in BPF, read from userspace.
 * Disable collection with --no-stats for maximum performance.
 */

struct gamer_stats {
    /* === SECTION 1: Core Scheduling (64 bytes) === */
    u64 nr_enqueued;           /* Tasks going through enqueue() fallback */
    u64 nr_dispatched;         /* Tasks dispatched (started running) */
    u64 nr_direct_dispatch;    /* Direct dispatch from select_cpu (fast path) */
    u64 nr_shared_dispatch;    /* Shared DSQ dispatch (slow path) */
    u64 nr_wakeups;            /* Total task wakeups */
    u64 nr_yields;             /* Voluntary yields */
    u64 nr_slice_expiry;       /* Slice exhaustion (preempted by time) */
    u64 _sched_reserved;
    
    /* === SECTION 2: Detection Events - Per-Hook Stats (128 bytes) === */
    /* Input hooks (3) */
    u64 nr_hid_irq_in;         /* fentry/hid_irq_in - USB HID interrupt */
    u64 nr_input_event;        /* fentry/input_event - Input subsystem event */
    u64 nr_hid_input_report;   /* fentry/hid_input_report - HID report */
    
    /* GPU hooks (3) */
    u64 nr_drm_ioctl;          /* fentry/drm_ioctl - DRM command submit */
    u64 nr_drm_atomic_commit;  /* fentry/drm_atomic_commit - Frame submit */
    u64 nr_dma_fence_signal;   /* fentry/dma_fence_signal - GPU fence (NVIDIA!) */
    
    /* Audio hooks (2) */
    u64 nr_audio_ioctl;        /* fentry/do_vfs_ioctl - ALSA ioctl */
    u64 nr_pcm_period;         /* fentry/snd_pcm_period_elapsed - Audio period */
    
    /* Sync hooks (3) */
    u64 nr_esync;              /* fentry/eventfd_signal_mask - Wine esync */
    u64 nr_fsync;              /* fentry/do_futex - Wine fsync */
    u64 nr_ntsync;             /* fentry/ntsync_char_ioctl - Wine ntsync */
    
    /* Aggregate totals (for quick display) */
    u64 nr_input_detected;     /* Total input hooks fired */
    u64 nr_gpu_detected;       /* Total GPU hooks fired */
    u64 nr_audio_detected;     /* Total audio hooks fired */
    u64 nr_sync_detected;      /* Total sync hooks fired */
    
    /* Window boost stats */
    u64 nr_input_window_boosts;/* Tasks boosted by input window */
    u64 nr_sync_window_boosts; /* Tasks boosted by sync window */
    
    /* === SECTION 3: Priority Distribution (64 bytes) === */
    u64 boost_histogram[8];    /* Count per boost level (0-7) */
    
    /* === SECTION 4: CPU Selection (64 bytes) === */
    u64 nr_physical_selected;  /* Physical core selections */
    u64 nr_smt_selected;       /* SMT sibling selections */
    u64 nr_migrations;         /* CPU migrations */
    u64 nr_same_cpu;           /* Task stayed on same CPU */
    u64 nr_idle_found;         /* Found idle CPU */
    u64 nr_preempt_needed;     /* No idle CPU, will preempt */
    u64 _cpu_reserved[2];
    
    /* === SECTION 5: Preemption (64 bytes) === */
    u64 nr_preempt_kick;       /* Preemptive kicks (SCX_KICK_PREEMPT) */
    u64 nr_preempt_avoided;    /* Kicks avoided (equal/higher priority) */
    u64 nr_idle_kick;          /* Idle kicks (SCX_KICK_IDLE) */
    u64 nr_latency_critical_kicks; /* Kicks for latency-critical tasks */
    u64 _preempt_reserved[4];
    
    /* === SECTION 6: Wait Time Histogram (88 bytes) === */
    /* Buckets: <1us, 1-10us, 10-100us, 100us-1ms, 1-10ms, 10-100ms, 100ms-1s, 1-3s, 3-5s, 5-10s, >10s */
    u64 wait_histogram[11];
    
    /* === SECTION 7: Health & Debugging (64 bytes) === */
    u64 max_wait_ns;           /* Longest task wait time seen */
    u64 nr_starvation_rescues; /* Tasks rescued from starvation */
    u64 nr_errors;             /* Error count */
    u64 nr_affinity_failures;  /* CPU affinity violations prevented */
    u64 total_runtime_ns;      /* Sum of all task runtimes */
    u64 total_wait_ns;         /* Sum of all task wait times */
    u64 _health_reserved[2];
};

/* ============================================================================
 * SECTION 4: BPF MAPS
 * ============================================================================ */

/* Per-task context storage */
struct {
    __uint(type, BPF_MAP_TYPE_TASK_STORAGE);
    __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int);
    __type(value, struct task_ctx);
} task_ctxs SEC(".maps");

/* Per-CPU context (array for O(1) access) */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, MAX_CPUS);
    __type(key, u32);
    __type(value, struct cpu_ctx);
} cpu_ctxs SEC(".maps");

/* Global scheduler statistics */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, struct gamer_stats);
} stats_map SEC(".maps");

/* ============================================================================
 * SECTION 5: DEBUG EVENT RING BUFFER
 * ============================================================================
 *
 * For real-time event tracing. Only populated when debug mode is enabled.
 * Each event is 64 bytes for cache alignment.
 */

/* Event types */
enum debug_event_type {
    EVENT_ENQUEUE = 1,         /* Task enqueued */
    EVENT_DISPATCH = 2,        /* Task dispatched to CPU */
    EVENT_DETECT_INPUT = 3,    /* Input detected */
    EVENT_DETECT_GPU = 4,      /* GPU submit detected */
    EVENT_DETECT_AUDIO = 5,    /* Audio callback detected */
    EVENT_DETECT_SYNC = 6,     /* Wine/Proton sync detected */
    EVENT_BOOST_CHANGE = 7,    /* Task boost level changed */
    EVENT_STARVATION = 8,      /* Task starved, getting rescue */
    EVENT_PREEMPT = 9,         /* Preemption occurred */
    EVENT_MIGRATE = 10,        /* Task migrated CPUs */
    EVENT_LONG_WAIT = 11,      /* Task waited >10ms (for debugging outliers) */
};

/* Debug event structure (64 bytes) */
struct debug_event {
    u64 timestamp_ns;          /* Event timestamp */
    u32 event_type;            /* enum debug_event_type */
    u32 pid;                   /* Task PID */
    u32 tgid;                  /* Task TGID (process) */
    u8 boost_old;              /* Previous boost level */
    u8 boost_new;              /* New boost level */
    u8 flags;                  /* Task flags */
    u8 _pad0;
    s32 cpu_from;              /* Source CPU (-1 if N/A) */
    s32 cpu_to;                /* Target CPU (-1 if N/A) */
    u64 wait_ns;               /* Time spent waiting (if applicable) */
    u64 runtime_ns;            /* Runtime so far */
    char comm[16];             /* Task name (first 16 chars) */
} __attribute__((aligned(64)));

/* Ring buffer for debug events (only used when debug=true) */
struct {
    __uint(type, BPF_MAP_TYPE_RINGBUF);
    __uint(max_entries, 256 * 1024); /* 256KB = ~4000 events */
} debug_events SEC(".maps");

#endif /* __TYPES_BPF_H */

