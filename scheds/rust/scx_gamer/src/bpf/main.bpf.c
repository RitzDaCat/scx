/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
 * Copyright (c) 2025 RitzDaCat
 */
#include <scx/common.bpf.h>
#include "intf.h"

/*
 * Maximum amount of CPUs supported by the scheduler when flat or preferred
 * idle CPU scan is enabled.
 */
#define MAX_CPUS	1024

/*
 * Shared DSQ used to schedule tasks in deadline mode when the system is
 * saturated.
 *
 * When system is not saturated tasks will be dispatched to the local DSQ
 * in round-robin mode.
 */
#define SHARED_DSQ		0
/* Tunables / thresholds (documented for readability). */
#define INTERACTIVE_SLICE_SHRINK_THRESH 256ULL	/* per-CPU interactive_avg threshold to shrink slice */
#define INTERACTIVE_SMT_ALLOW_THRESH     128ULL	/* allow SMT pairing when below this interactivity */
#define WAKE_FREQ_SHIFT                  8		/* wakeup_freq >> SHIFT maps to modest factor */
#define CHAIN_BOOST_MAX                  4		/* max chain boost depth */
#define CHAIN_BOOST_STEP                 2		/* increment per sync-wake event */
/* GPU submission thread detection thresholds */
#define GPU_SUBMIT_EXEC_THRESH_NS        100000ULL	/* <100μs exec per wake suggests GPU submit */
#define GPU_SUBMIT_FREQ_MIN              50ULL		/* min wakeup freq (50 = ~2ms period for 500fps) */
#define GPU_SUBMIT_STABLE_SAMPLES        8		/* require N consistent samples before classification */
/* Background task detection thresholds */
#define BACKGROUND_EXEC_THRESH_NS        5000000ULL	/* >5ms exec suggests CPU-intensive background work */
#define BACKGROUND_FREQ_MAX              10ULL		/* low wakeup freq (<10 = >100ms sleep) indicates batch */
#define BACKGROUND_STABLE_SAMPLES        4		/* require N consistent samples */
/* Command flags for userspace -> BPF triggers via BSS cmd_flags. */
#define CMD_INPUT   (1u << 0)
#define CMD_FRAME   (1u << 1)
#define CMD_NAPI    (1u << 2)

/*
 * Thresholds for applying hysteresis to CPU performance scaling:
 *  - CPUFREQ_LOW_THRESH: below this level, reduce performance to minimum
 *  - CPUFREQ_HIGH_THRESH: above this level, raise performance to maximum
 *
 * Values between the two thresholds retain the current smoothed performance level.
 */
#define CPUFREQ_LOW_THRESH	(SCX_CPUPERF_ONE / 4)
#define CPUFREQ_HIGH_THRESH	(SCX_CPUPERF_ONE - SCX_CPUPERF_ONE / 4)

/*
 * Rate limit for MM hint updates to reduce hot-path overhead.
 * Only update MM hint if at least this much time has passed since last update.
 */
#define MM_HINT_UPDATE_INTERVAL_NS	10000000ULL  /* 10ms */

/*
 * Subset of CPUs to prioritize.
 */
private(GAMER) struct bpf_cpumask __kptr *primary_cpumask;

/*
 * Set to true when @primary_cpumask is empty (primary domain includes all
 * the CPU).
 */
const volatile bool primary_all = true;

/*
 * Enable flat iteration to find idle CPUs (fast but inaccurate).
 */
const volatile bool flat_idle_scan = false;

/*
 * CPUs in the system have SMT is enabled.
 */
const volatile bool smt_enabled = true;

/*
 * Enable preferred cores prioritization.
 */
const volatile bool preferred_idle_scan = false;

/*
 * CPUs sorted by their capacity in descendent order.
 */
const volatile u64 preferred_cpus[MAX_CPUS];

/*
 * Enable cpufreq integration.
 */
const volatile bool cpufreq_enabled = true;

/*
 * Enable NUMA optimizatons.
 */
const volatile bool numa_enabled;

/*
 * Aggressively try to avoid SMT contention.
 *
 * Default to true here, so veristat takes the more complicated path.
 */
const volatile bool avoid_smt = true;

/*
 * Enable address space affinity.
 */
const volatile bool mm_affinity;

/*
 * Enable deferred wakeup.
 */
const volatile bool deferred_wakeups = true;

/*
 * Ignore synchronous wakeup events.
 */
const volatile bool no_wake_sync;

/*
 * Disable stats collection for maximum performance (no atomic ops in hot path).
 */
const volatile bool no_stats;

/* Input boost configuration (ns). */
const volatile u64 input_window_ns;

/* Foreground game/application tgid (0 = disabled, apply globally) */
const volatile u32 foreground_tgid;
/* Runtime-updatable foreground tgid (overrides foreground_tgid if non-zero) */
volatile u32 detected_fg_tgid;
/* Enable use of per-mm recent CPU hint map. */
const volatile bool mm_hint_enabled = true;

/*
 * Default time slice.
 */
const volatile u64 slice_ns = 10000ULL;

/*
 * Wakeup timer period. If zero, falls back to slice_ns. Tunable from userspace.
 */
const volatile u64 wakeup_timer_ns;

/*
 * Maximum runtime that can be charged to a task.
 */
const volatile u64 slice_lag = 20000000ULL;

/*
 * Current global CPU utilization percentage in the range [0 .. 1024].
 */
volatile u64 cpu_util;

/*
 * Migration limiter configuration (from userspace).
 */
const volatile u64 mig_window_ns;
const volatile u32 mig_max_per_window;
/* NUMA spill threshold: if local node shared DSQ depth is below this, avoid migrations. */
const volatile u32 numa_spill_thresh;
/* Prefer NAPI/softirq CPU during input window (opt-in) */
const volatile bool prefer_napi_on_input;

#define MIG_TOKEN_SCALE               1024ULL

/* Stats counters (BSS, accumulate). */
volatile u64 rr_enq;
volatile u64 edf_enq;
volatile u64 nr_direct_dispatches;
volatile u64 nr_shared_dispatches;
volatile u64 nr_migrations;
volatile u64 nr_mig_blocked;
volatile u64 nr_sync_local;
volatile u64 nr_frame_mig_block;
volatile u64 cpu_util_avg;
volatile u64 interactive_sys_avg;
/* Window activity accounting (accumulated by wakeup timer). */
volatile u64 win_input_ns_total;
volatile u64 win_frame_ns_total;
volatile u64 timer_elapsed_ns_total;
/* Hint/selection quality metrics and fg runtime accounting. */
volatile u64 nr_idle_cpu_pick;
volatile u64 nr_mm_hint_hit;
volatile u64 fg_runtime_ns_total;
volatile u64 total_runtime_ns_total;
/* Trigger counters. */
volatile u64 nr_input_trig;
volatile u64 nr_frame_trig;
/* GPU thread physical core affinity enforcement. */
volatile u64 nr_gpu_phys_kept;
/* SYNC wake fast path counter. */
volatile u64 nr_sync_wake_fast;
/* Task classification counters. */
volatile u64 nr_gpu_submit_threads;
volatile u64 nr_background_threads;
volatile u64 nr_compositor_threads;
volatile u64 nr_network_threads;
volatile u64 nr_system_audio_threads;
volatile u64 nr_game_audio_threads;
volatile u64 nr_input_handler_threads;

/* Userspace-triggered commands (set bits; drained in wakeup_timerfn). */
volatile u32 cmd_flags;

/* Global window until timestamps to avoid per-CPU writes. */
volatile u64 input_until_global;
volatile u64 napi_until_global;

/* Bitmap of CPUs with local DSQ work pending that may need a kick. */
#define KICK_WORDS ((MAX_CPUS + 63) / 64)
volatile u64 kick_mask[KICK_WORDS];

/* Conditional stats increment - no-op if no_stats enabled */
static __always_inline void stat_inc(volatile u64 *counter)
{
	if (!no_stats)
		__atomic_fetch_add(counter, 1, __ATOMIC_RELAXED);
}

static __always_inline void set_kick_cpu(s32 cpu)
{
    /* Bound check to appease verifier and avoid OOB on kick_mask. */
    if (cpu < 0 || (u32)cpu >= MAX_CPUS)
        return;
    u32 w = (u32)cpu >> 6;
    /* Additional bounds check on word index for verifier safety */
    if (w >= KICK_WORDS)
        return;
    u64 bit = 1ULL << (cpu & 63);
    __atomic_fetch_or(&kick_mask[w], bit, __ATOMIC_RELAXED);
}

static __always_inline void clear_kick_cpu(s32 cpu)
{
    /* Bound check to appease verifier and avoid OOB on kick_mask. */
    if (cpu < 0 || (u32)cpu >= MAX_CPUS)
        return;
    u32 w = (u32)cpu >> 6;
    /* Additional bounds check on word index for verifier safety */
    if (w >= KICK_WORDS)
        return;
    u64 bit = 1ULL << (cpu & 63);
    __atomic_fetch_and(&kick_mask[w], ~bit, __ATOMIC_RELAXED);
}

char _license[] SEC("license") = "GPL";

/*
 * Scheduler's exit status.
 */
UEI_DEFINE(uei);

/*
 * Maximum amount of CPUs supported by the system.
 */
static u64 nr_cpu_ids;

/*
 * Moved vtime tracking to per-CPU context.
 */

/* Forward declaration for per-CPU context lookup used by helpers below. */
struct cpu_ctx *try_lookup_cpu_ctx(s32 cpu);



/* Helpers to check per-CPU boost windows. (Defined after cpu_ctx) */

/*
 * Per-task context.
 */
struct CACHE_ALIGNED task_ctx {
    /* hot-path first: frequently read/updated fields */
    u64 exec_runtime;      /* accumulated since last sleep */
    u64 last_run_at;       /* timestamp when started running */
    u64 wakeup_freq;       /* EMA of inter-wakeup frequency */
    u64 last_woke_at;      /* last wake timestamp */
    /* migration limiter state (scaled token bucket) */
    u64 mig_tokens;        /* scaled by MIG_TOKEN_SCALE */
    u64 mig_last_refill;
    /* small scalar used in hot paths */
    u32 chain_boost;
    /* rate-limiting for mm hint updates */
    u64 mm_hint_last_update;
    /* GPU submission thread detection: tracks avg exec time per wake */
    u64 exec_avg;          /* EMA of exec_runtime per wake cycle */
    /* Background task detection: sample counter for stable classification */
    u16 low_cpu_samples;   /* consecutive wakes with <100μs exec time */
    u16 high_cpu_samples;  /* consecutive wakes with >5ms exec time */
    /* Page fault tracking for cache thrashing detection */
    u64 last_pgfault_total; /* last sampled maj_flt + min_flt */
    u64 pgfault_rate;       /* page faults per wake cycle (EMA) */
    /* Task role classification flags */
    u8 is_gpu_submit:1;    /* likely GPU command submission thread */
    u8 is_background:1;    /* likely background/batch work (shader compile, asset load) */
    u8 is_compositor:1;    /* compositor/window manager thread */
    u8 is_network:1;       /* network/netcode thread (critical for online games) */
    u8 is_system_audio:1;  /* system audio (PipeWire/ALSA) - high priority but not blocking input */
    u8 is_game_audio:1;    /* game audio thread - lower priority than input */
    u8 is_input_handler:1; /* input processing thread - HIGHEST priority for gaming */
    u8 reserved_flags:1;   /* reserved for future use */
};

struct {
	__uint(type, BPF_MAP_TYPE_TASK_STORAGE);
	__uint(map_flags, BPF_F_NO_PREALLOC);
	__type(key, int);
	__type(value, struct task_ctx);
} task_ctx_stor SEC(".maps");

/*
 * Return a local task context from a generic task.
 */
struct task_ctx *try_lookup_task_ctx(const struct task_struct *p)
{
	return bpf_task_storage_get(&task_ctx_stor,
					(struct task_struct *)p, 0, 0);
}

/*
 * Per-CPU context.
 */
struct CACHE_ALIGNED cpu_ctx {
    /* hot-path first */
    u64 vtime_now;         /* cached system vruntime reference */
    u64 interactive_avg;   /* per-CPU interactivity EMA */
    /* cpufreq related */
    u64 last_update;
    u64 perf_lvl;
    /* misc */
    u64 shared_dsq_id;
    u32 last_cpu_idx;      /* for idle scan rotation */
};

struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__type(key, u32);
	__type(value, struct cpu_ctx);
	__uint(max_entries, 1);
} cpu_ctx_stor SEC(".maps");

/*
 * Return a CPU context.
 */
struct cpu_ctx *try_lookup_cpu_ctx(s32 cpu)
{
	const u32 idx = 0;
	return bpf_map_lookup_percpu_elem(&cpu_ctx_stor, &idx, cpu);
}

/* Helpers to check per-CPU boost windows. */
static __always_inline bool is_input_active_cpu(s32 cpu)
{
    const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;
    if (primary && !bpf_cpumask_test_cpu(cpu, primary))
        return false;
    return time_before(scx_bpf_now(), input_until_global);
}
/* Helpers to fan-out windows across primary CPUs. */
static __always_inline void fanout_set_input_window(void)
{
    input_until_global = scx_bpf_now() + input_window_ns;
}

static __always_inline void fanout_set_napi_window(void)
{
    napi_until_global = scx_bpf_now() + input_window_ns;
}

static __always_inline bool is_input_active(void)
{
    return is_input_active_cpu(bpf_get_smp_processor_id());
}

/*
 * Note: Cgroup-based game detection was considered but not implemented.
 * Steam often isolates games in cgroups named "app-*", "steam_app_*", or "game.slice",
 * but BPF verifier restrictions make direct cgroup path reading impractical.
 * The process hierarchy detection already covers most multi-process game cases.
 *
 * Future implementation would require:
 * 1. Userspace reading /proc/pid/cgroup and updating BPF map
 * 2. BPF LSM hooks (requires kernel 5.7+)
 */

/* Return true if @p belongs to the foreground application (tgid match with hierarchy support).
 * Checks task's TGID, parent's TGID, and grandparent's TGID to support multi-process games.
 * Examples: Steam->game, game->overlay, game+voicechat, launcher->game->renderer
 * @fg_tgid_cached: optional pre-loaded fg_tgid value (0 = load fresh)
 */
static __always_inline bool is_foreground_task_cached(const struct task_struct *p, u32 fg_tgid_cached)
{
    u32 fg_tgid = fg_tgid_cached ? fg_tgid_cached : (detected_fg_tgid ? detected_fg_tgid : foreground_tgid);

    /* Auto-detect mode: if no fg_tgid specified, try cgroup-based detection */
    if (!fg_tgid) {
        /* Could check cgroup here, but due to BPF limitations, we treat all as foreground.
         * This preserves legacy behavior where all tasks get equal priority.
         * Users can still specify foreground_tgid for explicit game isolation. */
        return true;
    }

    /* Direct match: task itself is foreground */
    if ((u32)p->tgid == fg_tgid)
        return true;

    /* Check parent process (one level up): handles game->overlay, game->voicechat */
    struct task_struct *parent = p->real_parent;
    if (parent && (u32)parent->tgid == fg_tgid)
        return true;

    /* Check grandparent (two levels up): handles launcher->game->renderer chains */
    if (parent) {
        struct task_struct *grandparent = parent->real_parent;
        if (grandparent && (u32)grandparent->tgid == fg_tgid)
            return true;
    }

    return false;
}

static __always_inline bool is_foreground_task(const struct task_struct *p)
{
    return is_foreground_task_cached(p, 0);
}

/* Helper to load fg_tgid once per hot path. */
static __always_inline u32 get_fg_tgid(void)
{
    return detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
}

/*
 * Check if task comm matches known compositor names.
 * Compositors are critical for frame delivery to display.
 */
static __always_inline bool is_compositor_name(const char *comm)
{
    /* KDE Plasma Wayland compositor */
    if (comm[0] == 'k' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 'n')
        return true;
    /* GNOME Mutter compositor */
    if (comm[0] == 'm' && comm[1] == 'u' && comm[2] == 't' && comm[3] == 't')
        return true;
    /* Weston reference compositor */
    if (comm[0] == 'w' && comm[1] == 'e' && comm[2] == 's' && comm[3] == 't')
        return true;
    /* Sway (i3-like) compositor */
    if (comm[0] == 's' && comm[1] == 'w' && comm[2] == 'a' && comm[3] == 'y')
        return true;
    /* Hyprland compositor */
    if (comm[0] == 'H' && comm[1] == 'y' && comm[2] == 'p' && comm[3] == 'r')
        return true;
    /* labwc (Openbox-like) compositor */
    if (comm[0] == 'l' && comm[1] == 'a' && comm[2] == 'b' && comm[3] == 'w')
        return true;
    /* Xwayland server */
    if (comm[0] == 'X' && comm[1] == 'w' && comm[2] == 'a' && comm[3] == 'y')
        return true;
    return false;
}

/*
 * Check if task comm matches network/netcode thread naming patterns.
 * Network threads are critical for online games: player input -> network -> server.
 */
static __always_inline bool is_network_name(const char *comm)
{
    /* Unreal Engine network threads */
    if (comm[0] == 'W' && comm[1] == 'e' && comm[2] == 'b' && comm[3] == 'S' &&
        comm[4] == 'o' && comm[5] == 'c' && comm[6] == 'k')
        return true;  /* WebSocketClient */
    if (comm[0] == 'U' && comm[1] == 'd' && comm[2] == 'p' && comm[3] == 'S')
        return true;  /* UdpSocket */
    if (comm[0] == 'R' && comm[1] == 't' && comm[2] == 'c')
        return true;  /* RtcWorkerThread, RtcSignalingThr, RtcNetworkThrea */
    if (comm[0] == 'H' && comm[1] == 't' && comm[2] == 't' && comm[3] == 'p' &&
        comm[4] == 'M' && comm[5] == 'a' && comm[6] == 'n')
        return true;  /* HttpManagerThre */
    if (comm[0] == 'I' && comm[1] == 'o' && comm[2] == 'S')
        return true;  /* IoService */
    if (comm[0] == 'I' && comm[1] == 'o' && comm[2] == 'D')
        return true;  /* IoDispatcher */
    if (comm[0] == 'I' && comm[1] == 'O' && comm[2] == 'T' && comm[3] == 'h')
        return true;  /* IOThreadPool */
    if (comm[0] == 'N' && comm[1] == 'A' && comm[2] == 'T' && comm[3] == 'S')
        return true;  /* NATSClientThrea */
    if (comm[0] == 'O' && comm[1] == 'n' && comm[2] == 'l' && comm[3] == 'i' &&
        comm[4] == 'n' && comm[5] == 'e' && comm[6] == 'A')
        return true;  /* OnlineAsyncTask */
    /* Common patterns: "network", "netcode", "net_", "recv", "send", "socket" */
    if (comm[0] == 'n' && comm[1] == 'e' && comm[2] == 't')
        return true;
    /* WoW uppercase network threads */
    if (comm[0] == 'N' && comm[1] == 'e' && comm[2] == 't')
        return true;  /* NetThread, Net Queue, Network */
    if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'c' && comm[3] == 'v')
        return true;
    if (comm[0] == 's' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd')
        return true;
    if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'c' && comm[3] == 'k')
        return true;
    if (comm[0] == 'i' && comm[1] == 'o' && comm[2] == '_')
        return true;
    if (comm[0] == 'p' && comm[1] == 'a' && comm[2] == 'c' && comm[3] == 'k')
        return true;
    return false;
}

/*
 * Check if task comm matches SYSTEM audio (PipeWire/ALSA/PulseAudio).
 * System audio has strict latency requirements but shouldn't block game input.
 */
static __always_inline bool is_system_audio_name(const char *comm)
{
    /* PipeWire audio server (modern Linux standard) */
    if (comm[0] == 'p' && comm[1] == 'i' && comm[2] == 'p' && comm[3] == 'e')
        return true;
    /* Check for "pipewire" or "pw-" prefix */
    if (comm[0] == 'p' && comm[1] == 'w' && comm[2] == '-')
        return true;
    /* ALSA (Advanced Linux Sound Architecture) */
    if (comm[0] == 'a' && comm[1] == 'l' && comm[2] == 's' && comm[3] == 'a')
        return true;
    /* JACK audio connection kit (pro audio) */
    if (comm[0] == 'j' && comm[1] == 'a' && comm[2] == 'c' && comm[3] == 'k')
        return true;
    /* PulseAudio (legacy, but still common) */
    if (comm[0] == 'p' && comm[1] == 'u' && comm[2] == 'l' && comm[3] == 's')
        return true;
    return false;
}

/*
 * Check if task comm matches GAME audio thread patterns.
 * Game audio is important but shouldn't delay input processing.
 */
static __always_inline bool is_game_audio_name(const char *comm)
{
    /* Unreal Engine audio threads */
    if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' &&
        comm[4] == 'o' && comm[5] == 'D' && comm[6] == 'e' && comm[7] == 'v')
        return true;  /* AudioDeviceBuff */
    if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' &&
        comm[4] == 'o' && comm[5] == 'E' && comm[6] == 'n' && comm[7] == 'c')
        return true;  /* AudioEncoder */
    if (comm[0] == 'F' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
        return true;  /* FAudio_AudioCli */
    /* Bink audio (common video codec in games) */
    if (comm[0] == 'B' && comm[1] == 'i' && comm[2] == 'n' && comm[3] == 'k' &&
        comm[4] == ' ' && comm[5] == 'S' && comm[6] == 'n' && comm[7] == 'd')
        return true;  /* Bink Snd */
    /* Generic game audio threads: "audio", "sound", "snd_" */
    if (comm[0] == 'a' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i')
        return true;
    if (comm[0] == 's' && comm[1] == 'o' && comm[2] == 'u' && comm[3] == 'n')
        return true;
    if (comm[0] == 's' && comm[1] == 'n' && comm[2] == 'd' && comm[3] == '_')
        return true;
    /* OpenAL (common game audio library) */
    if (comm[0] == 'o' && comm[1] == 'p' && comm[2] == 'e' && comm[3] == 'n' &&
        comm[4] == 'a' && comm[5] == 'l')
        return true;
    /* FMOD (game audio engine) */
    if (comm[0] == 'f' && comm[1] == 'm' && comm[2] == 'o' && comm[3] == 'd')
        return true;
    /* Wwise (game audio engine) */
    if (comm[0] == 'w' && comm[1] == 'w' && comm[2] == 'i' && comm[3] == 's')
        return true;
    return false;
}

/*
 * Check if task comm matches GPU submission thread patterns.
 * These threads submit rendering commands to the GPU driver.
 */
static __always_inline bool is_gpu_submit_name(const char *comm)
{
    /* DXVK threads (DX9/10/11 to Vulkan translation - VERY common with Proton) */
    if (comm[0] == 'd' && comm[1] == 'x' && comm[2] == 'v' && comm[3] == 'k' && comm[4] == '-')
        return true;  /* dxvk-submit, dxvk-queue, dxvk-frame, dxvk-cs, dxvk-shader-* */
    /* Unreal Engine RHI (Render Hardware Interface) threads */
    if (comm[0] == 'R' && comm[1] == 'H' && comm[2] == 'I')
        return true;  /* RHIThread, RHISubmissionTh, RHIInterruptThr */
    /* Unreal RenderThread */
    if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
        comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T')
        return true;  /* RenderThread 0 */
    /* vkd3d threads (Vulkan/D3D12 translation layer for Proton) */
    if (comm[0] == 'v' && comm[1] == 'k' && comm[2] == 'd' && comm[3] == '3')
        return true;  /* vkd3d_queue, vkd3d_fence, vkd3d-swapchain */
    /* Bracketed Vulkan threads (WoW, etc.) */
    if (comm[0] == '[' && comm[1] == 'v' && comm[2] == 'k')
        return true;  /* [vkrt] Analysis, [vkps] Update, [vkcf] Analysis */
    /* Unity render threads */
    if (comm[0] == 'U' && comm[1] == 'n' && comm[2] == 'i' && comm[3] == 't' &&
        comm[4] == 'y' && comm[5] == 'G' && comm[6] == 'f' && comm[7] == 'x')
        return true;  /* UnityGfxDevice */
    /* Generic "render" or "gpu" thread names */
    if (comm[0] == 'r' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
        comm[4] == 'e' && comm[5] == 'r')
        return true;
    if (comm[0] == 'g' && comm[1] == 'p' && comm[2] == 'u')
        return true;
    return false;
}

/*
 * Check if task comm matches input processing thread patterns.
 * Input handlers are THE most critical for gaming - mouse/keyboard lag ruins gameplay.
 *
 * NOTE: Unreal Engine processes input on GameThread, not a separate input thread!
 */
static __always_inline bool is_input_handler_name(const char *comm)
{
    /* Unreal Engine GameThread (handles input + game logic) */
    if (comm[0] == 'G' && comm[1] == 'a' && comm[2] == 'm' && comm[3] == 'e' &&
        comm[4] == 'T' && comm[5] == 'h' && comm[6] == 'r')
        return true;  /* GameThread */
    /* SDL input threads (very common in games) */
    if (comm[0] == 'S' && comm[1] == 'D' && comm[2] == 'L')
        return true;
    /* Input/event processing threads */
    if (comm[0] == 'i' && comm[1] == 'n' && comm[2] == 'p' && comm[3] == 'u')
        return true;
    if (comm[0] == 'e' && comm[1] == 'v' && comm[2] == 'e' && comm[3] == 'n')
        return true;
    /* GLFW input (common game library) */
    if (comm[0] == 'g' && comm[1] == 'l' && comm[2] == 'f' && comm[3] == 'w')
        return true;
    /* Qt/GTK input threads (less common in games but possible) */
    if (comm[0] == 'Q' && comm[1] == 't' && comm[2] == 'I' && comm[3] == 'n')
        return true;
    return false;
}

static __always_inline bool is_napi_softirq_preferred_cpu(s32 cpu)
{
    const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;
    if (primary && !bpf_cpumask_test_cpu(cpu, primary))
        return false;
    return time_before(scx_bpf_now(), napi_until_global);
}

/* removed unused is_napi_softirq_preferred() to silence -Wunused-function */

/*
 * Exponential weighted moving average (EWMA).
 *
 * Copied from scx_lavd. Returns the new average as:
 *
 *	new_avg := (old_avg * .75) + (new_val * .25);
 */
static u64 calc_avg(u64 old_val, u64 new_val)
{
	return (old_val - (old_val >> 2)) + (new_val >> 2);
}

/*
 * Update the average frequency of an event.
 *
 * The frequency is computed from the given interval since the last event
 * and combined with the previous frequency using an exponential weighted
 * moving average.
 *
 * Returns the previous frequency unchanged if interval is zero (prevents
 * division by zero from concurrent wakeups or clock issues).
 */
static u64 update_freq(u64 freq, u64 interval)
{
        u64 new_freq;

        /* Guard against division by zero from same-nanosecond wakeups or clock skew */
        if (!interval)
                return freq;

        new_freq = (100 * NSEC_PER_MSEC) / interval;
        return calc_avg(freq, new_freq);
}

/*
 * Update CPU load and scale target performance level accordingly.
 */
static void update_cpu_load(struct task_struct *p, u64 slice)
{
	u64 now = scx_bpf_now();
	s32 cpu = scx_bpf_task_cpu(p);
	u64 perf_lvl, delta_t;
	struct cpu_ctx *cctx;

	if (!cpufreq_enabled)
		return;

	cctx = try_lookup_cpu_ctx(cpu);
	if (!cctx)
		return;

	/*
	 * Evaluate dynamic cpuperf scaling factor using the average CPU
	 * utilization, normalized in the range [0 .. SCX_CPUPERF_ONE].
	 */
	/* Skip update if uninitialized, or if we detect clock skew (now < last_update).
	 * Also skip if delta_t is zero or suspiciously large (>1s) to handle time jumps. */
	if (!cctx->last_update || now < cctx->last_update) {
		cctx->last_update = now;
		return;
	}
	delta_t = now - cctx->last_update;
	if (!delta_t || delta_t > NSEC_PER_SEC) {
		cctx->last_update = now;
		return;
	}

	/*
	 * Refresh target performance level.
	 */
	perf_lvl = MIN(slice * SCX_CPUPERF_ONE / delta_t, SCX_CPUPERF_ONE);
	cctx->perf_lvl = calc_avg(cctx->perf_lvl, perf_lvl);
	cctx->last_update = now;
}

/*
 * Apply target cpufreq performance level to @cpu.
 */
static void update_cpufreq(s32 cpu)
{
	struct cpu_ctx *cctx;
	u64 perf_lvl;

	if (!cpufreq_enabled)
		return;

	cctx = try_lookup_cpu_ctx(cpu);
	if (!cctx)
		return;

	/*
	 * Apply target performance level to the cpufreq governor.
	 */
	if (cctx->perf_lvl >= CPUFREQ_HIGH_THRESH)
		perf_lvl = SCX_CPUPERF_ONE;
	else if (cctx->perf_lvl <= CPUFREQ_LOW_THRESH)
		perf_lvl = SCX_CPUPERF_ONE / 2;
	else
		perf_lvl = cctx->perf_lvl;

	scx_bpf_cpuperf_set(cpu, perf_lvl);
}

/*
 * Timer used to defer idle CPU wakeups.
 *
 * Instead of triggering wake-up events directly from hot paths, such as
 * ops.enqueue(), idle CPUs are kicked using the wake-up timer.
 */
struct wakeup_timer {
	struct bpf_timer timer;
};

struct {
	__uint(type, BPF_MAP_TYPE_ARRAY);
	__uint(max_entries, 1);
	__type(key, u32);
	__type(value, struct wakeup_timer);
} wakeup_timer SEC(".maps");

/* Per-mm recent CPU hint to improve wake affinity across threads. */
struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __type(key, u64);   /* mm pointer */
    __type(value, u32); /* last cpu */
    __uint(max_entries, 4096);
} mm_last_cpu SEC(".maps");

/*
 * Return the global system shared DSQ.
 */
static inline u64 shared_dsq(s32 cpu)
{
    struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
    if (cctx && cctx->shared_dsq_id)
        return cctx->shared_dsq_id;
    if (numa_enabled) {
        u64 node = __COMPAT_scx_bpf_cpu_node(cpu);
        if (cctx)
            cctx->shared_dsq_id = node;
        return node;
    }
    if (cctx)
        cctx->shared_dsq_id = SHARED_DSQ;
    return SHARED_DSQ;
}

/*
 * Return true if @p can only run on a single CPU, false otherwise.
 */
static inline bool is_pcpu_task(const struct task_struct *p)
{
	return p->nr_cpus_allowed == 1 || is_migration_disabled(p);
}

/*
 * Always use deadline mode (EDF scheduling) for gaming workloads.
 *
 * scx_gamer is specialized for gaming, where latency always matters more
 * than cache locality. This eliminates mode switching overhead and tuning burden.
 */
static inline bool is_system_busy(void)
{
    return true;
}

/*
 * Return the cpumask of fully idle SMT cores within the NUMA node that
 * contains @cpu.
 *
 * If NUMA support is disabled, @cpu is ignored.
 */
static inline const struct cpumask *get_idle_smtmask(s32 cpu)
{
	if (!numa_enabled)
		return scx_bpf_get_idle_smtmask();

	return __COMPAT_scx_bpf_get_idle_smtmask_node(__COMPAT_scx_bpf_cpu_node(cpu));
}

/*
 * Return true if the CPU is running the idle thread, false otherwise.
 */
static inline bool is_cpu_idle(s32 cpu)
{
	struct rq *rq = scx_bpf_cpu_rq(cpu);

	if (!rq) {
		scx_bpf_error("Failed to access rq %d", cpu);
		return false;
	}
	return rq->curr->flags & PF_IDLE;
}

/* Update per-mm recent CPU hint when a task starts running (rate-limited to reduce hot-path overhead). */
static __always_inline void update_mm_last_cpu(struct task_struct *p, struct task_ctx *tctx, u64 now)
{
    if (!p->mm || !tctx)
        return;

    /* Rate-limit updates: only update if enough time has passed since last update.
     * Guard against clock skew (now < last_update) by checking both conditions. */
    if (tctx->mm_hint_last_update) {
        if (now < tctx->mm_hint_last_update ||
            (now - tctx->mm_hint_last_update) < MM_HINT_UPDATE_INTERVAL_NS)
            return;
    }

    u64 key = (u64)p->mm;
    u32 cpu = (u32)scx_bpf_task_cpu(p);
    bpf_map_update_elem(&mm_last_cpu, &key, &cpu, BPF_ANY);
    tctx->mm_hint_last_update = now;
}

/*
 * Try to pick the best idle CPU based on the @preferred_cpus ranking.
 * Return a full-idle SMT core if @do_idle_smt is true, or any idle CPU if
 * @do_idle_smt is false.
 */
/* pick_idle_cpu_pref_smt removed: selection now prefers scx_bpf_select_cpu_and path. */

/*
 * Return the optimal idle CPU for task @p or -EBUSY if no idle CPU is
 * found.
 */
/* pick_idle_cpu_flat removed: selection now prefers scx_bpf_select_cpu_and path. */

/*
 * Pick an optimal idle CPU for task @p (as close as possible to
 * @prev_cpu).
 *
 * Return the CPU id or a negative value if an idle CPU can't be found.
 */
/* Context structure to avoid BPF function argument limit (max 5 args) */
struct pick_cpu_cache {
	bool is_busy;
	struct cpu_ctx *pc;
	u32 fg_tgid;
	bool input_active;
};

/* Optimized version that takes pre-computed values to avoid redundant work */
static s32 pick_idle_cpu_cached(struct task_struct *p, s32 prev_cpu, u64 wake_flags,
                                bool from_enqueue, struct pick_cpu_cache *cache)
{
	const struct cpumask *mask = cast_mask(primary_cpumask);
	s32 cpu;
    u32 *hint;
    u64 mm_key;

    /* Prefer select_cpu_and path for simplicity and lower overhead.
     * (Flat scan retained in code base but disabled here.)
     */

	bool is_busy = cache->is_busy;
	u32 fg_tgid = cache->fg_tgid;
	bool input_active = cache->input_active;
	/* Note: cache->pc (prev cpu_ctx) is available but not needed in current fast path */

	/*
	 * Clear the wake sync bit if synchronous wakeups are disabled.
	 */
    if (no_wake_sync && !input_active)
        wake_flags &= ~SCX_WAKE_SYNC;

    /* If NAPI preference is enabled during input window, try to keep prev CPU if idle. */
    if (prefer_napi_on_input && input_active && is_foreground_task_cached(p, fg_tgid) && is_napi_softirq_preferred_cpu(prev_cpu)) {
        if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
            stat_inc(&nr_idle_cpu_pick);
            return prev_cpu;
        }
    }

    /* Try per-mm recent CPU hint first for foreground to preserve cache affinity. */
    if (mm_hint_enabled && p->mm && is_foreground_task_cached(p, fg_tgid)) {
        mm_key = (u64)p->mm;
        hint = bpf_map_lookup_elem(&mm_last_cpu, &mm_key);
        if (hint) {
            s32 hcpu = (s32)(*hint);
            if (bpf_cpumask_test_cpu(hcpu, p->cpus_ptr) && scx_bpf_test_and_clear_cpu_idle(hcpu)) {
                stat_inc(&nr_mm_hint_hit);
                stat_inc(&nr_idle_cpu_pick);
                return hcpu;
            }
        }
    }

    /*
	 * Fallback to the old API if the kernel doesn't support
	 * scx_bpf_select_cpu_and().
	 *
	 * This is required to support kernels <= 6.16.
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
	 * If a primary domain is defined, try to pick an idle CPU from
	 * there first.
	 */
    /* Compute SMT decision once to avoid duplicate logic.
     * Use global interactive_sys_avg for consistency across all CPUs,
     * avoiding per-CPU variation in SMT pairing decisions.
     *
     * CRITICAL: Force GPU submission threads to physical cores.
     * vkd3d-swapchain and vkd3d_queue threads need dedicated cores
     * to minimize frame presentation latency.
     *
     * Check both the cached flag AND thread name directly since the flag
     * is set after the first running() callback, but select_cpu() is called
     * earlier during wakeup.
     */
    struct task_ctx *tctx = try_lookup_task_ctx(p);
    bool is_critical_gpu = (tctx && tctx->is_gpu_submit) || is_gpu_submit_name(p->comm);

    /* GPU threads: aggressively prefer physical cores (first sibling of each SMT pair).
     * On typical SMT systems, physical cores are the lower-numbered sibling (e.g., CPU 0,1,2...).
     * This avoids the issue where SCX_PICK_IDLE_CORE rejects physical cores if their
     * sibling is busy, causing GPU threads to land on hyperthreads.
     *
     * Strategy: Use preferred_cpus array which is already sorted with physical cores first
     * when SMT is enabled. This gives us the correct priority order without complex runtime checks.
     */
    if (is_critical_gpu && smt_enabled && preferred_idle_scan) {
        /* Scan preferred_cpus array which already prioritizes physical cores */
        u32 i;
        bpf_for(i, 0, MAX_CPUS) {
            s32 candidate = (s32)preferred_cpus[i];
            if (candidate < 0 || (u32)candidate >= nr_cpu_ids)
                break;
            if (!bpf_cpumask_test_cpu(candidate, p->cpus_ptr))
                continue;

            /* Try to claim this CPU if idle */
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
                stat_inc(&nr_idle_cpu_pick);
                stat_inc(&nr_gpu_phys_kept);
                return candidate;
            }
        }
        /* If no CPU from preferred list is idle, fall through to standard selection */
    }

    bool allow_smt = is_critical_gpu ? false :
                     (!avoid_smt || (!is_busy && interactive_sys_avg < INTERACTIVE_SMT_ALLOW_THRESH));
    u64 smt_flags = allow_smt ? 0 : SCX_PICK_IDLE_CORE;

    if (!primary_all && mask) {
        cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, mask, smt_flags);
		if (cpu >= 0) {
            stat_inc(&nr_idle_cpu_pick);
			return cpu;
        }
	}

	/*
	 * Pick any idle CPU usable by the task.
	 */
    cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, p->cpus_ptr, smt_flags);
    if (cpu >= 0)
        stat_inc(&nr_idle_cpu_pick);
    return cpu;
}


/*
 * Return a time slice scaled by the task's weight.
 * @cctx: optional pre-fetched cpu_ctx for the task's CPU (pass NULL to auto-fetch)
 * @fg_tgid: optional pre-loaded fg_tgid (0 = load fresh)
 */
static u64 task_slice_with_ctx_cached(const struct task_struct *p, struct cpu_ctx *cctx, u32 fg_tgid)
{
    u64 s = slice_ns;
    struct task_ctx *tctx = try_lookup_task_ctx(p);

    /* Fetch cctx once if needed */
    if (!cctx) {
        s32 cpu = scx_bpf_task_cpu(p);
        cctx = try_lookup_cpu_ctx(cpu);
    }

    /* Adjust slices during active input/frame windows. */
    /* Check if foreground first to short-circuit expensive window checks */
    if (is_foreground_task_cached(p, fg_tgid) && cctx) {
        /* Combined window check - single timestamp call, no cpumask recheck */
        u64 now = scx_bpf_now();
        if (time_before(now, input_until_global)) {
            /* Input window: shorter slice for fast preemption. */
            s = s >> 1;
        }
    }

    /* Scale slice by per-CPU interactive activity average (simple EMA proxy).
     * As interactive_avg grows, slice shrinks modestly: s = s * 3/4 when high.
     */
    if (cctx && cctx->interactive_avg > INTERACTIVE_SLICE_SHRINK_THRESH)
        s = (s * 3) >> 2;
    if (tctx && tctx->wakeup_freq > 256)
        s = s >> 1; /* shorter slice for highly interactive tasks */
    return scale_by_task_weight(p, s);
}

static u64 task_slice_with_ctx(const struct task_struct *p, struct cpu_ctx *cctx)
{
    return task_slice_with_ctx_cached(p, cctx, 0);
}

static u64 task_slice(const struct task_struct *p)
{
    return task_slice_with_ctx(p, NULL);
}

/*
 * Calculate and return the virtual deadline for the given task.
 *
 *  The deadline is defined as:
 *
 *    deadline = vruntime + exec_vruntime
 *
 * Here, `vruntime` represents the task's total accumulated runtime,
 * inversely scaled by its weight, while `exec_vruntime` accounts the
 * runtime accumulated since the last sleep event, also inversely scaled by
 * the task's weight.
 *
 * Fairness is driven by `vruntime`, while `exec_vruntime` helps prioritize
 * tasks that sleep frequently and use the CPU in short bursts (resulting
 * in a small `exec_vruntime` value), which are typically latency critical.
 *
 * Additionally, to prevent over-prioritizing tasks that sleep for long
 * periods of time, the vruntime credit they can accumulate while sleeping
 * is limited by @slice_lag, which is also scaled based on the task's
 * weight.
 *
 * To prioritize tasks that sleep frequently over those with long sleep
 * intervals, @slice_lag is also adjusted in function of the task's wakeup
 * frequency: tasks that sleep often have a bigger slice lag, allowing them
 * to accumulate more time-slice credit than tasks with infrequent, long
 * sleeps.
 *
 * @cctx: optional pre-fetched cpu_ctx (pass NULL to auto-fetch)
 */
static u64 task_dl_with_ctx(struct task_struct *p, struct task_ctx *tctx, struct cpu_ctx *cctx)
{
    /* Safety: return safe default if tctx is NULL */
    if (!tctx)
        return p->scx.dsq_vtime;

    /* Fast path for foreground tasks during boost windows - minimal deadline calculation. */
    u32 fg_tgid = get_fg_tgid();
    u64 now = scx_bpf_now();
    bool in_input_window = time_before(now, input_until_global);

    /* PRIORITY 1 (HIGHEST): Input handlers during input window
     * Mouse/keyboard lag is THE WORST experience for gamers.
     * Your crosshair MUST move when you move your mouse. Period. */
    if (tctx->is_input_handler && in_input_window)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 7); /* 10x boost (HIGHEST) */

    /* PRIORITY 2: GPU submission threads - visual feedback is critical
     * Once input is processed, we need to SEE the result on screen ASAP.
     * Always boosted when foreground app is active. */
    if (tctx->is_gpu_submit)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 6); /* 8x boost */

    /* PRIORITY 3: System audio (PipeWire/ALSA) - voice chat quality
     * High priority for Discord/TeamSpeak, but doesn't block input.
     * Still needs <5ms latency to avoid crackling. */
    if (tctx->is_system_audio)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 5) + (tctx->exec_runtime >> 6); /* 7x boost */

    /* PRIORITY 4: Game audio - important for immersion
     * Always boosted for smooth audio playback. */
    if (tctx->is_game_audio)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 5) + (tctx->exec_runtime >> 7); /* 6x boost */

    /* PRIORITY 5: Compositor - frame presentation path
     * Always boosted for smooth compositor operation. */
    if (tctx->is_compositor)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 5); /* 5x boost */

    /* PRIORITY 6: Network threads - online gameplay
     * Important but has RTT tolerance (20-100ms), lower than input (<10ms). */
    if (tctx->is_network && in_input_window)
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 4); /* 4x boost */

    /* PRIORITY 7: Foreground game threads during input window */
    if (fg_tgid && (u32)p->tgid == fg_tgid && in_input_window) {
        /* General game logic during active input */
        return p->scx.dsq_vtime + (tctx->exec_runtime >> 4); /* 4x boost */
    }

    /* Standard path for background tasks or foreground outside boost windows. */
    /* Pre-scale using coarse wakeup factor to reduce arithmetic cost. */
    u64 wake_factor = 1;
    if (tctx->wakeup_freq > 0)
        wake_factor = MIN(1 + (tctx->wakeup_freq >> WAKE_FREQ_SHIFT), CHAIN_BOOST_MAX);
    u64 vsleep_max = scale_by_task_weight(p, slice_lag * wake_factor);

    if (!cctx) {
        s32 cpu = scx_bpf_task_cpu(p);
        cctx = try_lookup_cpu_ctx(cpu);
    }
    u64 vbase = cctx ? cctx->vtime_now : 0;
    /* Protect against underflow: ensure vbase >= vsleep_max before subtraction */
    u64 vtime_min = vbase > vsleep_max ? vbase - vsleep_max : 0;

	if (time_before(p->scx.dsq_vtime, vtime_min))
		p->scx.dsq_vtime = vtime_min;

    /* Earlier deadlines for highly interactive tasks: decrease exec_vruntime impact
     * proportional to wakeup frequency to reduce input latency. */
    u64 exec_component = scale_by_task_weight_inverse(p, tctx->exec_runtime);

    /* GPU submission threads: always prioritize to minimize GPU idle time */
    if (tctx->is_gpu_submit)
        exec_component = exec_component >> 2; /* 4x deadline boost */

    /* Background tasks: deprioritize to prevent cache pollution during critical frames */
    if (tctx->is_background)
        exec_component = exec_component << 2; /* 4x penalty (later deadline) */

    /* Page fault penalty: threads with high fault rates are loading assets, not rendering.
     * Slight penalty to preserve cache for hot loops. Threshold: >50 faults per wake.
     * This catches texture streaming, level loading, asset decompression threads.
     * Exempt input handlers, GPU submit, and system audio from penalty. */
    if (tctx->pgfault_rate > 50 && !tctx->is_input_handler &&
        !tctx->is_system_audio && !tctx->is_gpu_submit)
        exec_component = (exec_component * 3) >> 1;  /* 1.5x penalty */

    /* wake_factor >= 1 (initialized to 1, only increased), safe to divide */
    if (tctx->wakeup_freq > 0 && wake_factor > 0)
        exec_component = exec_component / wake_factor;
    /* Apply futex/chain boost with fast decay: reduce exec_component further. */
    /* Divisor is (1 + min(chain_boost, 3)) >= 1, safe to divide */
    if (tctx->chain_boost)
        exec_component = exec_component / (1 + MIN((u64)tctx->chain_boost, 3));
    return p->scx.dsq_vtime + exec_component;
}

/*
 * Initialize a new cpumask, return 0 in case of success or a negative
 * value otherwise.
 */
static int init_cpumask(struct bpf_cpumask **p_cpumask)
{
	struct bpf_cpumask *mask;

	mask = *p_cpumask;
	if (mask)
		return 0;

	mask = bpf_cpumask_create();
	if (!mask)
		return -ENOMEM;

	mask = bpf_kptr_xchg(p_cpumask, mask);
	if (mask)
		bpf_cpumask_release(mask);

	/* Verify the exchange succeeded */
	if (!*p_cpumask) {
		/* This shouldn't happen, but handle defensively */
		return -ENOMEM;
	}
	return 0;
}

/*
 * Called from user-space to add CPUs to the the primary domain.
 */
SEC("syscall")
int enable_primary_cpu(struct cpu_arg *input)
{
	struct bpf_cpumask *mask;
	int err = 0;

	err = init_cpumask(&primary_cpumask);
	if (err)
		return err;

    if (input->cpu_id < 0 || input->cpu_id >= (s32)nr_cpu_ids) {
        return -EINVAL;
    }

	bpf_rcu_read_lock();
	mask = primary_cpumask;
	if (mask)
		bpf_cpumask_set_cpu(input->cpu_id, mask);
	bpf_rcu_read_unlock();

	return err;
}

/* Syscalls to trigger boost windows from userspace. */
SEC("syscall")
int set_input_window(void *unused)
{
    fanout_set_input_window();
        __atomic_fetch_add(&nr_input_trig, 1, __ATOMIC_RELAXED);
    return 0;
}

SEC("syscall")
int set_napi_softirq_window(void *unused)
{
    fanout_set_napi_window();
    return 0;
}

/*
 * Kick idle CPUs with pending tasks.
 *
 * Instead of waking up CPU when tasks are enqueued, we defer the wakeup
 * using this timer handler, in order to have a faster enqueue hot path.
 */
static int wakeup_timerfn(void *map, int *key, struct bpf_timer *timer)
{
	s32 cpu;
	int err;

	/*
	 * Iterate over all CPUs and wake up those that have pending tasks
	 * in their local DSQ.
	 *
	 * Note that tasks are only enqueued in ops.enqueue(), but we never
	 * wake-up the CPUs from there to reduce overhead in the hot path.
	 *
	 * Optimization: Iterate bitmap words first, skip zero words to avoid
	 * checking CPUs with no pending work. This reduces iteration overhead
	 * from O(nr_cpu_ids) to O(CPUs_with_work) on average.
         */
    {
        s32 w, bcpu;
        const struct cpumask *primary = !primary_all ? cast_mask(primary_cpumask) : NULL;

        /* Iterate kick bitmap words, skip empty words */
        bpf_for(w, 0, KICK_WORDS) {
            u64 mask = kick_mask[w];
            if (!mask)
                continue;  /* Skip empty word - no CPUs to kick in this range */

            /* Scan set bits in this word */
            s32 bit_idx;
            bpf_for(bit_idx, 0, 64) {
                if (!(mask & (1ULL << bit_idx)))
                    continue;

                bcpu = (w << 6) + bit_idx;
                if (bcpu < 0 || bcpu >= (s32)nr_cpu_ids)
                    break;

                /* Check primary domain membership */
                if (primary && !bpf_cpumask_test_cpu(bcpu, primary))
                    goto next_bit;

                /* Kick if CPU is idle and has queued work */
                if (scx_bpf_dsq_nr_queued(SCX_DSQ_LOCAL_ON | bcpu) && is_cpu_idle(bcpu))
                    scx_bpf_kick_cpu(bcpu, SCX_KICK_IDLE);

next_bit:
                clear_kick_cpu(bcpu);
            }
        }
        if (primary)
            scx_bpf_put_cpumask(primary);
    }

    /* Sample instantaneous CPU utilization in-kernel: proportion of non-idle CPUs. */
    {
        u64 busy = 0;
        u64 ncpus = nr_cpu_ids ? nr_cpu_ids : 1;
        bpf_for(cpu, 0, nr_cpu_ids)
            if (!is_cpu_idle(cpu))
                busy++;
        cpu_util = (busy * 1024) / ncpus;
    }

    /* Update EMA of CPU util in BPF to stabilize busy detection. */
    {
        /* 3/4 old + 1/4 new (same calc_avg). */
        u64 old = cpu_util_avg;
        u64 new = cpu_util;
        cpu_util_avg = (old - (old >> 2)) + (new >> 2);
    }

	    /* Accumulate window activity and elapsed time for monitor percentages. */
    {
        u64 period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
        u64 now = scx_bpf_now();
        __atomic_fetch_add(&timer_elapsed_ns_total, period, __ATOMIC_RELAXED);
        if (time_before(now, input_until_global))
            __atomic_fetch_add(&win_input_ns_total, period, __ATOMIC_RELAXED);
    }

    /* Drain userspace-triggered commands. */
    {
        u32 flags = __atomic_exchange_n(&cmd_flags, 0, __ATOMIC_RELAXED);
        if (flags & CMD_INPUT)
        {
            fanout_set_input_window();
            __atomic_fetch_add(&nr_input_trig, 1, __ATOMIC_RELAXED);
        }
        if (flags & CMD_NAPI)
            fanout_set_napi_window();
    }

    /* Re-arm the wakeup timer. */
    {
        u64 period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
        err = bpf_timer_start(timer, period, 0);
    }
	if (err)
		scx_bpf_error("Failed to re-arm wakeup timer");

	return 0;
}

/*
 * Return true if the CPU is part of a fully busy SMT core, false
 * otherwise.
 *
 * If SMT is disabled or SMT contention avoidance is disabled, always
 * return false (since there's no SMT contention or it's ignored).
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
 * Return true if we should attempt a task migration to an idle CPU, false
 * otherwise.
 */
static bool need_migrate(const struct task_struct *p, s32 prev_cpu, u64 enq_flags, bool is_busy)
{
	/*
	 * Per-CPU tasks are not allowed to migrate.
	 */
	if (is_pcpu_task(p))
		return false;

	/*
	 * Always attempt to migrate if we're contending an SMT core.
	 */
	if (is_smt_contended(prev_cpu))
		return true;

	/*
	 * Attempt a migration on wakeup (if ops.select_cpu() was skipped)
	 * or if the task was re-enqueued due to a higher scheduling class
	 * stealing the CPU it was queued on.
	 */
    if ((!__COMPAT_is_enq_cpu_selected(enq_flags) && !scx_bpf_task_running(p)) ||
        (enq_flags & SCX_ENQ_REENQ)) {
        struct task_ctx *tctx = try_lookup_task_ctx(p);
        u64 now;
        bool input_active;

        if (!tctx)
            return true;

        now = scx_bpf_now();
        if (mig_window_ns && mig_max_per_window) {
            u64 max_tokens = mig_max_per_window * MIG_TOKEN_SCALE;

            /* Early exit if tokens are already at max - no refill needed */
            if (tctx->mig_tokens >= max_tokens) {
                /* Still update last_refill for future calculations */
                if (!tctx->mig_last_refill || tctx->mig_last_refill > now)
                    tctx->mig_last_refill = now;
            } else {
                if (!tctx->mig_last_refill || tctx->mig_last_refill > now)
                    tctx->mig_last_refill = now;

                if (now > tctx->mig_last_refill) {
                    u64 elapsed = now - tctx->mig_last_refill;
                    /* Cap elapsed time to prevent overflow in multiplication.
                     * If elapsed > 2*window, grant full refill anyway. */
                    if (elapsed > mig_window_ns * 2) {
                        tctx->mig_tokens = max_tokens;
                        tctx->mig_last_refill = now;
                    } else {
                        /* Overflow-safe token calculation using only division and addition.
                         * The formula: tokens = (elapsed / window) * max_tokens + fractional_tokens
                         * is rewritten to prevent any intermediate multiplication overflow. */
                        u64 full_windows = elapsed / mig_window_ns;
                        u64 remainder_ns = elapsed % mig_window_ns;

                        /* Calculate tokens from full windows - no overflow since max_tokens is u32 */
                        u64 add = full_windows * max_tokens;

                        /* Add fractional tokens: (remainder / window) * max_tokens
                         * Reorder as: (remainder * max_tokens) / window, but split to avoid overflow.
                         * Since max_tokens is clamped by mig_max_per_window config (typically <100),
                         * and remainder < window, we use fixed-point scaling to prevent overflow. */
                        if (remainder_ns && mig_window_ns) {
                            /* Scale down both terms if needed to prevent overflow in multiplication.
                             * Use max(window >> 20, 1) to ensure denominator is never zero. */
                            u64 scale = (mig_window_ns >> 20) ? (mig_window_ns >> 20) : 1;
                            u64 scaled_rem = remainder_ns / scale;
                            u64 scaled_win = mig_window_ns / scale;
                            if (scaled_win > 0)
                                add += (scaled_rem * max_tokens) / scaled_win;
                        }

                        if (add) {
                            tctx->mig_tokens = MIN(tctx->mig_tokens + add, max_tokens);
                            tctx->mig_last_refill = now;
                        }
                    }
                }
            }
        }

        /* Stronger local preference under NUMA: avoid spilling if local DSQ depth is low. */
        if (numa_enabled && is_busy && numa_spill_thresh) {
            u64 depth = scx_bpf_dsq_nr_queued(shared_dsq(prev_cpu));
            if (depth < numa_spill_thresh)
                return false;
        }
        input_active = is_input_active_cpu(prev_cpu);
        /* Skip migration limiter for foreground tasks - let them migrate freely to escape slow CPUs.
         * Background tasks still subject to limiting to prevent thrashing. */
        if (mig_window_ns && mig_max_per_window && !input_active && !is_foreground_task(p)) {
            u64 need = MIG_TOKEN_SCALE;
            if (tctx->mig_tokens < need) {
                __atomic_fetch_add(&nr_mig_blocked, 1, __ATOMIC_RELAXED);
                return false;
            }
            tctx->mig_tokens -= need;
        }
        __atomic_fetch_add(&nr_migrations, 1, __ATOMIC_RELAXED);
        return true;
    }
    return false;
}

/*
 * Return true if a task is waking up another task that share the same
 * address space, false otherwise.
 */
static inline bool
is_wake_affine(const struct task_struct *waker, const struct task_struct *wakee)
{
	return mm_affinity &&
		!(waker->flags & PF_EXITING) && wakee->mm && (wakee->mm == waker->mm);
}

s32 BPF_STRUCT_OPS(gamer_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags)
{
	const struct task_struct *current = (void *)bpf_get_current_task_btf();
	bool is_busy = is_system_busy();
	struct cpu_ctx *prev_cctx = try_lookup_cpu_ctx(prev_cpu);
	u32 fg_tgid = get_fg_tgid();
	bool input_active = is_input_active();
	bool is_fg = is_foreground_task_cached(p, fg_tgid);
	s32 cpu;

	/* Check if this is a critical GPU thread early to bypass fast paths */
	struct task_ctx *tctx = try_lookup_task_ctx(p);
	bool is_critical_gpu = (tctx && tctx->is_gpu_submit) || is_gpu_submit_name(p->comm);

	/*
	 * Fast path: SYNC wake for foreground task during input window.
	 * Check most likely conditions first for better branch prediction.
	 * IMPORTANT: Skip fast path for GPU threads - they MUST use physical cores.
	 */
    if ((wake_flags & SCX_WAKE_SYNC) && is_fg && !is_critical_gpu) {
		if (!no_wake_sync || input_active) {
			/* Apply chain boost BEFORE dispatch so it affects deadline if task is re-enqueued */
			if (input_active) {
				if (!tctx)
					tctx = try_lookup_task_ctx(p);
				if (tctx)
					tctx->chain_boost = MIN(tctx->chain_boost + CHAIN_BOOST_STEP, CHAIN_BOOST_MAX);
			}
			/* Transiently keep the wakee local on sync wake to reduce input latency. */
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_with_ctx_cached(p, prev_cctx, fg_tgid), 0);
			stat_inc(&nr_sync_wake_fast);
			return prev_cpu;
		}
    }

	/*
	 * When the waker and wakee share the same address space and were previously
	 * running on the same CPU, there's a high chance of finding hot cache data
	 * on that CPU. In such cases, prefer keeping the wakee on the same CPU.
	 *
	 * This optimization is applied only when the system is not saturated,
	 * to avoid introducing too much unfairness.
	 * IMPORTANT: Skip for GPU threads - they must use physical cores.
	 */
	if (!is_busy && !is_critical_gpu && is_wake_affine(current, p)) {
		cpu = bpf_get_smp_processor_id();
		if (cpu == prev_cpu) {
			/* Verify CPU is idle before direct dispatch to avoid overloading */
			if (scx_bpf_test_and_clear_cpu_idle(cpu)) {
				scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_with_ctx_cached(p, prev_cctx, fg_tgid), 0);
				return cpu;
			}
		}
	}

    /* Pass cached values to avoid redundant lookups in pick_idle_cpu */
	struct pick_cpu_cache cache = {
		.is_busy = is_busy,
		.pc = prev_cctx,
		.fg_tgid = fg_tgid,
		.input_active = input_active,
	};
    cpu = pick_idle_cpu_cached(p, prev_cpu, wake_flags, false, &cache);

	/* Dispatch to local DSQ if we found idle CPU or system not busy */
	if (cpu >= 0) {
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_with_ctx_cached(p, prev_cctx, fg_tgid), 0);
		return cpu;
	}

	if (!is_busy) {
		scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice_with_ctx_cached(p, prev_cctx, fg_tgid), 0);
	}

	return prev_cpu;
}

/*
 * Wake-up @cpu if it's idle.
 */
static inline void wakeup_cpu(s32 cpu)
{
	/*
	 * If deferred wakeups are enabled all the wakeup events are
	 * performed asynchronously by wakeup_timerfn().
	 */
	if (deferred_wakeups)
		return;
	scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
}

void BPF_STRUCT_OPS(gamer_enqueue, struct task_struct *p, u64 enq_flags)
{
	s32 prev_cpu = scx_bpf_task_cpu(p), cpu;
	struct task_ctx *tctx;
	struct cpu_ctx *prev_cctx;
    bool is_busy = is_system_busy();
	u32 fg_tgid = get_fg_tgid();
	bool input_active = is_input_active();

	/*
	 * Attempt to dispatch directly to an idle CPU if the task can
	 * migrate.
	 */
    if (need_migrate(p, prev_cpu, enq_flags, is_busy)) {
		prev_cctx = try_lookup_cpu_ctx(prev_cpu);
		struct pick_cpu_cache cache = {
			.is_busy = is_busy,
			.pc = prev_cctx,
			.fg_tgid = fg_tgid,
			.input_active = input_active,
		};
        cpu = pick_idle_cpu_cached(p, prev_cpu, 0, true, &cache);
		if (cpu >= 0) {
			scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, task_slice(p), enq_flags);
            __atomic_fetch_add(&nr_direct_dispatches, 1, __ATOMIC_RELAXED);
			wakeup_cpu(cpu);
			return;
		}
	}

	/*
	 * Keep using the same CPU if the system is not busy, otherwise
	 * fallback to the shared DSQ.
	 */
	/* Optimized: check input window with single timestamp, no redundant checks */
	bool window_active = false;
	if (is_busy && is_foreground_task_cached(p, fg_tgid)) {
		u64 now = scx_bpf_now();
		window_active = time_before(now, input_until_global);
	}
    if (!is_busy || window_active) {
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice(p), enq_flags);
        set_kick_cpu(prev_cpu);
        __atomic_fetch_add(&rr_enq, 1, __ATOMIC_RELAXED);
		wakeup_cpu(prev_cpu);
		return;
	}

	/*
	 * Dispatch to the shared DSQ, using deadline-based scheduling.
	 * Fetch prev_cpu's context once for both shared_dsq() and task_dl().
	 */
	tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;
	prev_cctx = try_lookup_cpu_ctx(prev_cpu);
	scx_bpf_dsq_insert_vtime(p, shared_dsq(prev_cpu),
				 task_slice(p), task_dl_with_ctx(p, tctx, prev_cctx), enq_flags);
    __atomic_fetch_add(&edf_enq, 1, __ATOMIC_RELAXED);
	wakeup_cpu(prev_cpu);
}

void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
	/*
	 * Check if the there's any task waiting in the shared DSQ and
	 * dispatch.
	 */
    if (scx_bpf_dsq_move_to_local(shared_dsq(cpu))) {
        __atomic_fetch_add(&nr_shared_dispatches, 1, __ATOMIC_RELAXED);
        return;
    }

	/*
	 * If the previous task expired its time slice, but no other task
	 * wants to run on this SMT core, allow the previous task to run
	 * for another time slot.
	 */
	if (prev && (prev->scx.flags & SCX_TASK_QUEUED) && !is_smt_contended(cpu))
		prev->scx.slice = task_slice(prev);
}

void BPF_STRUCT_OPS(gamer_cpu_release, s32 cpu, struct scx_cpu_release_args *args)
{
	/*
	 * A higher scheduler class stole the CPU, re-enqueue all the tasks
	 * that are waiting on this CPU and give them a chance to pick
	 * another idle CPU.
	 */
	scx_bpf_reenqueue_local();
}

void BPF_STRUCT_OPS(gamer_runnable, struct task_struct *p, u64 enq_flags)
{
	u64 now = scx_bpf_now(), delta_t;
	struct task_ctx *tctx;
    s32 cpu = scx_bpf_task_cpu(p);
    struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);

	tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;

	/*
	 * Reset exec runtime (accumulated execution time since last
	 * sleep).
	 */
	tctx->exec_runtime = 0;

	/*
	 * Detect compositor tasks on first wakeup by checking comm name.
	 * Compositors are the critical path for presenting frames to the display.
	 * Boosting compositor priority during frame windows reduces presentation latency by 1-2ms.
	 */
	if (!tctx->is_compositor && is_compositor_name(p->comm)) {
		tctx->is_compositor = 1;
		__atomic_fetch_add(&nr_compositor_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Detect network/netcode threads for online games.
	 * Network threads are critical path: player input -> network -> server.
	 * Boosting during input windows reduces input-to-server latency.
	 */
	if (!tctx->is_network && is_foreground_task(p) && is_network_name(p->comm)) {
		tctx->is_network = 1;
		__atomic_fetch_add(&nr_network_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Detect SYSTEM audio (PipeWire/ALSA/PulseAudio) - system-wide audio server.
	 * High priority but shouldn't block game input processing.
	 */
	if (!tctx->is_system_audio && is_system_audio_name(p->comm)) {
		tctx->is_system_audio = 1;
		__atomic_fetch_add(&nr_system_audio_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Detect GAME audio threads (OpenAL/FMOD/Wwise/game-specific audio).
	 * Important for immersion but lower priority than input responsiveness.
	 */
	if (!tctx->is_game_audio && is_foreground_task(p) && is_game_audio_name(p->comm)) {
		tctx->is_game_audio = 1;
		__atomic_fetch_add(&nr_game_audio_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Detect INPUT HANDLER threads (SDL/GLFW/input event processing).
	 * HIGHEST priority for gaming - mouse/keyboard lag is unacceptable.
	 * This is what makes aim feel responsive.
	 */
	if (!tctx->is_input_handler && is_foreground_task(p) && is_input_handler_name(p->comm)) {
		tctx->is_input_handler = 1;
		__atomic_fetch_add(&nr_input_handler_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Main thread of THE FOREGROUND GAME PROCESS = input handler.
	 * Many games (WoW, older engines, single-threaded games) handle input on main thread.
	 * Heavy main threads NEED the boost - that's where the game logic lives.
	 *
	 * IMPORTANT: Check exact TGID match, not hierarchy (is_foreground_task includes children).
	 * This prevents boosting 100+ Wine helper processes.
	 */
	u32 fg_tgid = detected_fg_tgid ? detected_fg_tgid : foreground_tgid;
	if (!tctx->is_input_handler && p->tgid == fg_tgid && p->pid == p->tgid) {
		tctx->is_input_handler = 1;
		__atomic_fetch_add(&nr_input_handler_threads, 1, __ATOMIC_RELAXED);
	}

	/*
	 * Update the task's wakeup frequency based on the time since
	 * the last wakeup, then cap at 10000 to handle high-frequency tasks
	 * (audio at 48kHz, high-polling-rate devices) while avoiding overflow.
	 * Freq value is roughly wakeups per 100ms: 10000 ≈ 100kHz wakeup rate.
	 */
	delta_t = now - tctx->last_woke_at;
	tctx->wakeup_freq = update_freq(tctx->wakeup_freq, delta_t);
	tctx->wakeup_freq = MIN(tctx->wakeup_freq, 10000);
    tctx->last_woke_at = now;
    /* Fast decay chain boost on wake. */
    tctx->chain_boost = tctx->chain_boost >> 1;

    /* Update per-CPU interactive EMA when tasks wake frequently. */
    if (cctx) {
        u64 old = cctx->interactive_avg;
        u64 new = tctx->wakeup_freq;
        cctx->interactive_avg = (old - (old >> 2)) + (new >> 2);
    }

    /* Maintain a system-level interactive EMA to modulate busy thresholds (foreground-biased). */
    if (is_foreground_task(p)) {
        u64 old = interactive_sys_avg;
        u64 new = tctx->wakeup_freq;
        interactive_sys_avg = (old - (old >> 2)) + (new >> 2);
    }
}

void BPF_STRUCT_OPS(gamer_running, struct task_struct *p)
{
	struct task_ctx *tctx;
	s32 cpu;
	u64 now;

	tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;

	cpu = scx_bpf_task_cpu(p);

	/*
	 * Save a timestamp when the task begins to run (used to evaluate
	 * the used time slice).
	 */
	now = scx_bpf_now();
	tctx->last_run_at = now;

	/*
	 * Update current system's vruntime.
	 */
    {
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
        if (cctx && time_before(cctx->vtime_now, p->scx.dsq_vtime))
            cctx->vtime_now = p->scx.dsq_vtime;
    }

	/*
	 * Refresh cpufreq performance level.
	 */
	update_cpufreq(cpu);

    /* Update per-mm recent CPU hint (rate-limited). */
    update_mm_last_cpu(p, tctx, now);
}

void BPF_STRUCT_OPS(gamer_stopping, struct task_struct *p, bool runnable)
{
	struct task_ctx *tctx;
	u64 slice;

	tctx = try_lookup_task_ctx(p);
	if (!tctx)
		return;

	/*
	 * Evaluate the used time slice.
	 */
	slice = MIN(scx_bpf_now() - tctx->last_run_at, slice_ns);

	/*
	 * Update the vruntime and the total accumulated runtime since last
	 * sleep.
	 *
	 * exec_runtime tracks RAW (unscaled) time since last wake. It's reset
	 * to 0 in gamer_runnable(), so weight changes between wake cycles don't
	 * cause drift. Both vruntime and deadline calculation scale consistently
	 * by task weight.
	 *
	 * Cap the maximum accumulated time since last sleep to @slice_lag,
	 * to prevent starving CPU-intensive tasks.
	 */
	p->scx.dsq_vtime += scale_by_task_weight_inverse(p, slice);
	tctx->exec_runtime = MIN(tctx->exec_runtime + slice, slice_lag);

    /*
     * Update exec_avg EMA for GPU submission detection.
     * Track average execution time per wake cycle (reset in runnable).
     */
    tctx->exec_avg = calc_avg(tctx->exec_avg, tctx->exec_runtime);

    /*
     * Track page fault rate to detect asset-loading threads vs hot-loop threads.
     * High page fault rate indicates cache thrashing (loading new assets/textures).
     * Low/zero page faults indicate hot rendering loops (should preserve cache).
     */
    u64 current_pgfaults = p->maj_flt + p->min_flt;
    u64 pgfault_delta = current_pgfaults - tctx->last_pgfault_total;
    tctx->last_pgfault_total = current_pgfaults;
    /* Update EMA of page faults per wake (use 4:1 ratio like other EMAs) */
    tctx->pgfault_rate = calc_avg(tctx->pgfault_rate, pgfault_delta);

    /*
     * Detect GPU submission threads by thread name.
     * Name-based detection only - no heuristics to avoid false positives.
     */
    if (!tctx->is_gpu_submit && is_foreground_task(p) && is_gpu_submit_name(p->comm)) {
        tctx->is_gpu_submit = 1;
        __atomic_fetch_add(&nr_gpu_submit_threads, 1, __ATOMIC_RELAXED);
    }

    /*
     * Detect background tasks: high CPU usage (>5ms), low wakeup frequency.
     * These are shader compilers, asset loaders, or other batch work.
     */
    if (is_foreground_task(p) && tctx->wakeup_freq < BACKGROUND_FREQ_MAX) {
        if (tctx->exec_avg > BACKGROUND_EXEC_THRESH_NS) {
            tctx->high_cpu_samples = MIN(tctx->high_cpu_samples + 1, BACKGROUND_STABLE_SAMPLES);
            if (tctx->high_cpu_samples >= BACKGROUND_STABLE_SAMPLES && !tctx->is_background) {
                tctx->is_background = 1;
                __atomic_fetch_add(&nr_background_threads, 1, __ATOMIC_RELAXED);
            }
        } else {
            tctx->high_cpu_samples = 0;
            if (tctx->high_cpu_samples == 0 && tctx->is_background) {
                tctx->is_background = 0;
                __atomic_fetch_sub(&nr_background_threads, 1, __ATOMIC_RELAXED);
            }
        }
    } else {
        /* Reset background flag if wakeup pattern changes */
        tctx->high_cpu_samples = 0;
        if (tctx->is_background) {
            tctx->is_background = 0;
            __atomic_fetch_sub(&nr_background_threads, 1, __ATOMIC_RELAXED);
        }
    }

    /*
     * Update per-CPU statistics and propagate chain boost across wake chains.
     */
    update_cpu_load(p, slice);
    /* Runtime accounting for foreground vs total. */
    __atomic_fetch_add(&total_runtime_ns_total, slice, __ATOMIC_RELAXED);
    if (is_foreground_task(p))
        __atomic_fetch_add(&fg_runtime_ns_total, slice, __ATOMIC_RELAXED);
    if (runnable && tctx->chain_boost) {
        /* If the task remains runnable (likely woke someone), decay slower and allow inheritance. */
        /* slow decay when still in chain */
        tctx->chain_boost = MAX(tctx->chain_boost - 1, 1);
    }
}

void BPF_STRUCT_OPS(gamer_enable, struct task_struct *p)
{
	{
		s32 cpu = scx_bpf_task_cpu(p);
		struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
		if (cctx)
			p->scx.dsq_vtime = cctx->vtime_now;
	}
}

s32 BPF_STRUCT_OPS(gamer_init_task, struct task_struct *p,
		   struct scx_init_task_args *args)
{
	struct task_ctx *tctx;

	tctx = bpf_task_storage_get(&task_ctx_stor, p, 0,
				    BPF_LOCAL_STORAGE_GET_F_CREATE);
	if (!tctx)
		return -ENOMEM;

	return 0;
}

s32 BPF_STRUCT_OPS_SLEEPABLE(gamer_init)
{
	struct bpf_timer *timer;
	u32 key = 0;
	int err;

	nr_cpu_ids = scx_bpf_nr_cpu_ids();

	/* Initialize all CPU contexts explicitly to ensure clean state */
	{
		s32 cpu;
		bpf_for(cpu, 0, nr_cpu_ids) {
			struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
			if (cctx) {
				cctx->vtime_now = 0;
				cctx->interactive_avg = 0;
				cctx->last_update = 0;
				cctx->perf_lvl = SCX_CPUPERF_ONE;
				cctx->shared_dsq_id = 0;
				cctx->last_cpu_idx = 0;
			}
		}
	}

	/*
	 * Create separate per-node DSQs if NUMA optimization is enabled,
	 * otherwise use a single shared DSQ.
	 */
	if (numa_enabled) {
		int node;

		bpf_for(node, 0, __COMPAT_scx_bpf_nr_node_ids()) {
			err = scx_bpf_create_dsq(node, node);
			if (err) {
				scx_bpf_error("failed to create node DSQ %d: %d", node, err);
				return err;
			}
		}
	} else {
		err = scx_bpf_create_dsq(SHARED_DSQ, -1);
		if (err) {
			scx_bpf_error("failed to create shared DSQ: %d", err);
			return err;
		}
	}

    if (deferred_wakeups) {
		timer = bpf_map_lookup_elem(&wakeup_timer, &key);
		if (!timer) {
			scx_bpf_error("Failed to lookup wakeup timer");
			return -ESRCH;
		}

		bpf_timer_init(timer, &wakeup_timer, CLOCK_MONOTONIC);
		bpf_timer_set_callback(timer, wakeup_timerfn);

        {
            u64 period = wakeup_timer_ns ? wakeup_timer_ns : slice_ns;
            err = bpf_timer_start(timer, period, 0);
        }
        if (err) {
            scx_bpf_error("Failed to arm wakeup timer, falling back to synchronous wakeups");
#ifdef __COMPAT_scx_bpf_set_deferred_wakeups
            __COMPAT_scx_bpf_set_deferred_wakeups(false);
#else
            bpf_printk("scx_bpf_set_deferred_wakeups compat symbol missing; deferred wakeups disabled\n");
#endif
        }
    }

    return 0;
}

void BPF_STRUCT_OPS(gamer_exit, struct scx_exit_info *ei)
{
	UEI_RECORD(uei, ei);
}

SCX_OPS_DEFINE(gamer_ops,
	       .select_cpu		= (void *)gamer_select_cpu,
	       .enqueue			= (void *)gamer_enqueue,
	       .dispatch		= (void *)gamer_dispatch,
	       .runnable		= (void *)gamer_runnable,
	       .running			= (void *)gamer_running,
	       .stopping		= (void *)gamer_stopping,
	       .cpu_release		= (void *)gamer_cpu_release,
	       .enable			= (void *)gamer_enable,
	       .init_task		= (void *)gamer_init_task,
	       .init			= (void *)gamer_init,
	       .exit			= (void *)gamer_exit,
	       .timeout_ms		= 5000,
	       .name			= "gamer");
