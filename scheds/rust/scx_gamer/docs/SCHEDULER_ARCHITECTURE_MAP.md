# scx_gamer: Complete Scheduler Architecture Map

This document provides a comprehensive mapping of the scx_gamer scheduler, tracing execution paths from startup through all scheduling decisions.

---

## Table of Contents

1. [High-Level Architecture](#1-high-level-architecture)
2. [Scheduler Startup Flow](#2-scheduler-startup-flow)
3. [BPF Program Structure](#3-bpf-program-structure)
4. [Game Detection System](#4-game-detection-system)
5. [Thread Classification System](#5-thread-classification-system)
6. [Input Handling Pipeline](#6-input-handling-pipeline)
7. [Core Scheduling Operations](#7-core-scheduling-operations)
8. [Optimization Systems](#8-optimization-systems)
9. [Data Structures](#9-data-structures)
10. [File Manifest](#10-file-manifest)

---

## 1. High-Level Architecture

```
+-----------------------------------------------------------------------------------+
|                              scx_gamer Architecture                                |
+-----------------------------------------------------------------------------------+
|                                                                                   |
|  +-----------------------+     +------------------------+     +----------------+  |
|  |   Userspace (Rust)    |     |    BPF Programs (C)    |     |  Kernel        |  |
|  +-----------------------+     +------------------------+     +----------------+  |
|  |                       |     |                        |     |                |  |
|  | main.rs               |<--->| main.bpf.c             |<--->| sched_ext      |  |
|  | - Scheduler init      |     | - select_cpu           |     | framework      |  |
|  | - Event loop          |     | - enqueue              |     |                |  |
|  | - Input polling       |     | - dispatch             |     +----------------+  |
|  | - Game detection      |     | - running/stopping     |     |                |  |
|  | - Stats collection    |     | - timer callback       |     | Input          |  |
|  |                       |     |                        |     | Subsystem      |  |
|  | game_detect_bpf.rs    |     | Detection hooks:       |     | (evdev)        |  |
|  | - BPF LSM detection   |     | - gpu_detect.bpf.h     |     |                |  |
|  | - Ring buffer consume |     | - audio_detect.bpf.h   |     +----------------+  |
|  |                       |     | - network_detect.bpf.h |     |                |  |
|  | ring_buffer.rs        |     | - storage_detect.bpf.h |     | DRM/GPU        |  |
|  | - Input event RB      |     |                        |     | Subsystem      |  |
|  |                       |     | Classification:        |     |                |  |
|  | stats.rs              |     | - task_class.bpf.h     |     +----------------+  |
|  | - Metrics export      |     | - boost.bpf.h          |     |                |  |
|  +-----------------------+     +------------------------+     | Audio/ALSA     |  |
|                                                               +----------------+  |
+-----------------------------------------------------------------------------------+
```

### Component Interaction Flow

```
User Input → evdev → main.rs event loop → BPF syscall trigger → input_until_global
     │                     │                                           │
     │                     ▼                                           ▼
     │         game_detect_bpf.rs ───────────────────────► detected_fg_tgid
     │                                                           │
     │                                                           ▼
     └──────────────────────────────────────────────► BPF select_cpu/enqueue
                                                              │
                                                              ▼
                                                      Thread Classification
                                                      + Priority Boosting
                                                              │
                                                              ▼
                                                       CPU Selection
                                                       + Dispatch
```

---

## 2. Scheduler Startup Flow

### 2.1 Entry Point (`main.rs`)

```
main() → Opts::parse()
    │
    ├── CPU Detection (cpu_detect.rs)
    │   └── CpuInfo::detect() → model_name, safe_name
    │
    ├── BPF Skeleton Loading
    │   ├── scx_ops_open() → OpenObject
    │   ├── Configure volatiles:
    │   │   ├── slice_ns (default 10µs)
    │   │   ├── input_window_ns (default 5000µs)
    │   │   ├── keyboard_boost_ns (default 1000ms)
    │   │   ├── mouse_boost_ns (default 8ms)
    │   │   ├── foreground_tgid
    │   │   ├── enable_numa, avoid_smt, mm_affinity
    │   │   └── mig_window_ns, mig_max_per_window
    │   │
    │   ├── scx_ops_load() → Load BPF into kernel
    │   └── scx_ops_attach() → Attach struct_ops
    │
    ├── CPU Topology Setup
    │   ├── Topology::new() → System topology
    │   ├── Initialize primary_cpumask (preferred CPUs)
    │   ├── Setup preferred_cpus[] array (capacity sorted)
    │   └── Configure cpu_ccd_class[] (P-core/E-core classification)
    │
    ├── Input Subsystem Initialization
    │   ├── evdev::enumerate() → List input devices
    │   ├── classify_device_type() → Keyboard/Mouse/Controller
    │   ├── Setup epoll for event-driven input
    │   └── Initialize input ring buffers (16× distributed)
    │
    ├── Game Detection Initialization
    │   ├── BpfGameDetector::new() (preferred)
    │   │   ├── Build ring buffer consumer
    │   │   ├── detect_initial_game() → Scan /proc for running games
    │   │   └── Spawn consumer thread
    │   └── GameDetector::new() (fallback if BPF LSM unavailable)
    │       └── inotify-based /proc monitoring
    │
    ├── Optional Subsystems
    │   ├── AudioServerDetector (inotify on /run/user/*/pipewire-0)
    │   ├── AffinityOverride (proactive CPU pinning)
    │   ├── PowerMonitor (battery/thermal awareness)
    │   └── GpuQueueMonitor (GPU busy state tracking)
    │
    └── Event Loop (run())
        ├── epoll_wait() for:
        │   ├── Input device events
        │   ├── Ring buffer events (BPF → userspace)
        │   ├── Dispatch events (watchdog)
        │   └── Audio detector events
        │
        └── Process events → Update BPF state
```

### 2.2 BPF Program Initialization (`main.bpf.c`)

```
gamer_init() SEC("struct_ops/init")
    │
    ├── Initialize scheduler_generation
    ├── Reset classification counters
    ├── Setup wakeup timer
    │   └── bpf_timer_init() + bpf_timer_start()
    │
    └── Initialize per-CPU contexts (cpu_ctx)
        └── vtime_now, interactive_avg, shared_dsq_id
```

---

## 3. BPF Program Structure

### 3.1 File Organization

```
src/bpf/
├── main.bpf.c              # Core scheduling operations
├── intf.h                  # Interface definitions (shared with Rust)
├── profiling_config.h      # Build-time profiling toggle
│
└── include/
    ├── types.bpf.h         # Data structures (task_ctx, cpu_ctx, maps)
    ├── config.bpf.h        # Configuration constants
    ├── helpers.bpf.h       # Utility functions
    ├── boost.bpf.h         # Input/frame boost windows
    ├── task_class.bpf.h    # Thread classification
    ├── cpu_select.bpf.h    # CPU selection algorithms
    ├── profiling.bpf.h     # Performance instrumentation
    ├── thread_runtime.bpf.h # Runtime tracking
    │
    ├── gpu_detect.bpf.h    # GPU thread detection (fentry/drm_ioctl)
    ├── compositor_detect.bpf.h # Compositor detection
    ├── audio_detect.bpf.h  # Audio thread detection (fentry/snd_pcm_*)
    ├── network_detect.bpf.h # Network thread detection
    ├── storage_detect.bpf.h # Storage/NVMe detection
    ├── memory_detect.bpf.h # Memory-intensive detection
    ├── interrupt_detect.bpf.h # IRQ handler detection
    ├── filesystem_detect.bpf.h # Filesystem operation detection
    └── affinity_detect.bpf.h # Affinity tracking
```

### 3.2 Key BPF Operations

| Operation | Function | Description |
|-----------|----------|-------------|
| `select_cpu` | `gamer_select_cpu()` | Choose CPU for waking task |
| `enqueue` | `gamer_enqueue()` | Insert task into dispatch queue |
| `dequeue` | `gamer_dequeue()` | Remove task from queue |
| `dispatch` | `gamer_dispatch()` | Dispatch tasks to CPUs |
| `running` | `gamer_running()` | Task started running |
| `stopping` | `gamer_stopping()` | Task stopped running |
| `runnable` | `gamer_runnable()` | Task became runnable |
| `quiescent` | `gamer_quiescent()` | Task went to sleep |
| `disable` | `gamer_disable()` | Task leaving scheduler |
| `init` | `gamer_init()` | Scheduler initialization |
| `exit` | `gamer_exit()` | Scheduler shutdown |

---

## 4. Game Detection System

### 4.1 Detection Hierarchy

```
Game Detection Priority:
1. BPF LSM Detection (game_detect_bpf.rs) [Preferred]
   └── exec() system call hooks
   └── Process events via ring buffer
   └── ~1-5ms detection latency

2. Inotify Fallback (game_detect.rs)
   └── /proc monitoring
   └── Periodic scanning
   └── ~100-500ms detection latency

3. Manual Override (--foreground-pid)
   └── User-specified TGID
```

### 4.2 BPF LSM Detection Flow (`game_detect_lsm.bpf.c`)

```
exec() system call
    │
    ▼
tracepoint/sched/sched_process_exec
    │
    ├── Check parent process:
    │   ├── Is parent "steam", "reaper", "gamescope"?
    │   ├── Is parent Wine/Proton process?
    │   └── Check grandparent hierarchy
    │
    ├── Check process characteristics:
    │   ├── .exe suffix (Windows executable)
    │   ├── Wine/Proton indicators
    │   └── Game name patterns
    │
    ├── Set flags:
    │   ├── FLAG_WINE (0x01)
    │   ├── FLAG_STEAM (0x02)
    │   └── FLAG_EXE (0x04)
    │
    └── Submit to process_events ring buffer
        │
        ▼
    Userspace consumer (game_detect_bpf.rs)
        │
        ├── validate_game_candidate()
        │   ├── Read /proc/PID/cmdline
        │   ├── Read /proc/PID/status (threads, memory)
        │   ├── Check MangohHUD shm (strong game signal)
        │   └── Calculate confidence score
        │
        └── Update detected_fg_tgid_staging
            │
            ▼
        BPF sync_detected_fg()
            └── Copy staging → detected_fg_tgid
```

### 4.3 Game Classification Scoring

```python
# Scoring Algorithm (validate_game_candidate)
score = 0

# Strong positive signals
if has_mangohud_shm():    score += 1000
if is_wine:               score += 100
if is_steam:              score += 50
if ends_with(".exe"):     score += 200
if threads >= 50:         score += 300
if threads >= 20:         score += 150
if memory_mb >= 500:      score += 200

# Negative signals (wrappers/launchers)
if name in ["python", "bash", "reaper", "steam.exe"]:
    score -= 500
if threads < 5:           score -= 200
if memory_mb < 50:        score -= 100

# Accept if score > 0
```

---

## 5. Thread Classification System

### 5.1 Classification Hierarchy

```
Thread Classification (task_class.bpf.h)
│
├── Input Handlers (boost_shift=7, 10x priority)
│   ├── GameThread (Unreal Engine)
│   ├── MainThread (Unity)
│   ├── SDL*, glfw*, input*, event*
│   └── wine_xinput_hid, wine_dinput_worker
│
├── GPU Submit (boost_shift=6, 8x priority)
│   ├── dxvk-* (DXVK translation layer)
│   ├── vkd3d-* (VKD3D-Proton)
│   ├── RHIThread, RenderThread (Unreal)
│   └── UnityGfxDevice
│
├── Compositor (boost_shift=5, 7x priority)
│   ├── kwin_wayland, mutter, sway
│   ├── Hyprland, labwc, Xwayland
│   └── CompositorTileW
│
├── Ethernet NIC Interrupt (boost_shift=4)
│   └── irq/*, netif_*, eth_*
│
├── Network (boost_shift=3)
│   ├── WebSocketClient, UdpSocket
│   ├── RtcWorkerThread, HttpManager
│   └── wine_rpcrt4_*, wine_net*
│
├── USB Audio (boost_shift=2)
│   └── snd_usb_*, goxlr*, focusrite*
│
├── System Audio (boost_shift=1)
│   ├── pipewire, pw-*, pulseaudio
│   ├── alsa*, jackdbus
│   └── module-rt, data-loop.*
│
├── Game Audio (boost_shift=1)
│   ├── AudioThread, FAudioCli
│   ├── fmod*, wwise*, openal*
│   └── Bink Snd
│
└── Background (8x penalty)
    ├── steamwebhelper, discord
    ├── chromium, cursor
    └── plasma-systemmonitor
```

### 5.2 Classification Flow

```
gamer_runnable() / gamer_running()
    │
    ├── Get/Create task_ctx via bpf_task_storage_get()
    │
    ├── Check scheduler_generation (invalidate stale classification)
    │
    ├── Fast Path (boost_shift already set):
    │   └── Return immediately
    │
    └── Slow Path (classify_task):
        │
        ├── classify_input_handler()
        │   └── is_input_handler_name(comm) → set is_input_handler
        │
        ├── classify_gpu_submit()
        │   ├── is_gpu_submit_name(comm)
        │   └── is_gpu_submit_thread(tid) [fentry detection]
        │
        ├── classify_audio()
        │   ├── TGID lookup in system_audio_tgids_map
        │   ├── is_system_audio_name(comm)
        │   └── is_game_audio_name(comm)
        │
        ├── classify_network()
        │   └── is_network_name(comm)
        │
        ├── classify_background()
        │   └── is_background_name(comm)
        │
        └── recompute_boost_shift(tctx)
            │
            └── boost_shift = max(
                  class_boost,
                  is_input_handler ? 7 : 0,
                  is_gpu_submit ? 6 : 0,
                  is_compositor ? 5 : 0,
                  ...
                )
```

### 5.3 Fentry-Based Detection

```
Fentry hooks (direct kernel function attachment):

GPU Detection (gpu_detect.bpf.h):
├── fentry/drm_ioctl → Intel/AMD DRM calls
│   └── Check for DRM_I915_GEM_EXECBUFFER2, DRM_AMDGPU_CS
└── kprobe/nv_drm_ioctl → NVIDIA DRM calls

Audio Detection (audio_detect.bpf.h):
├── fentry/snd_pcm_period_elapsed
├── fentry/snd_pcm_start
└── fentry/snd_pcm_stop

Network Detection (network_detect.bpf.h):
├── fentry/sock_sendmsg
├── fentry/tcp_sendmsg
└── fentry/udp_sendmsg

Storage Detection (storage_detect.bpf.h):
├── fentry/blk_mq_submit_bio
└── fentry/nvme_queue_rq

Compositor Detection (compositor_detect.bpf.h):
├── fentry/drm_mode_setcrtc
└── fentry/drm_mode_setplane
```

---

## 6. Input Handling Pipeline

### 6.1 Userspace Input Processing

```
Event Loop (main.rs::run())
    │
    ├── epoll_wait() returns
    │   └── Input device fd ready
    │
    ├── Device fd lookup:
    │   └── input_fd_info_vec[fd] → DeviceInfo (idx, lane)
    │
    ├── Read evdev events:
    │   └── dev.fetch_events()
    │
    ├── For each event:
    │   ├── Classify by EventType:
    │   │   ├── KEY → key press/release
    │   │   ├── RELATIVE → mouse movement
    │   │   └── ABSOLUTE → touchpad/tablet
    │   │
    │   ├── Track keyboard state:
    │   │   ├── Key press → kbd_pressed_count++
    │   │   └── Key release → kbd_pressed_count--
    │   │
    │   └── Trigger BPF boost:
    │       └── input_trigger_fn(lane)
    │           │
    │           ▼
    │       BPF syscall: set_input_lane()
    │           │
    │           └── fanout_set_input_lane(lane, now)
    │
    └── Update BPF maps:
        └── kbd_pressed_count → bss.kbd_pressed_count
```

### 6.2 BPF Input Boost Processing

```
fanout_set_input_lane(lane, now) [boost.bpf.h]
    │
    ├── Calculate boost duration:
    │   ├── Keyboard: 1000ms (default)
    │   ├── Mouse: 8ms (default)
    │   ├── Controller: 500ms
    │   └── Other: 0 (no boost)
    │
    ├── Apply dynamic window adjustment:
    │   └── compute_dynamic_window(rate, base_duration)
    │       ├── High rate (>800Hz): +50% window
    │       ├── Medium rate (>400Hz): +25% window
    │       └── Low rate (<60Hz): -50% window
    │
    ├── Update lane expiry:
    │   └── input_lane_until[lane] = now + boost_duration
    │
    └── Update global window:
        └── input_until_global = max(current, lane_expiry)
```

### 6.3 Input Window Effects on Scheduling

```
During input window (is_input_active_now() == true):

select_cpu():
├── GPU/Compositor threads → Prefer physical cores (no SMT)
├── Input handlers → Maximum priority CPU selection
└── Foreground threads → Shorter slices for preemption

task_slice():
├── Slice halved (slice_ns >> 1)
└── Interactive scaling applied

task_dl() (deadline calculation):
├── Input handlers → 10x boost (boost_shift=7)
├── GPU threads → 8x boost (boost_shift=6)
└── Foreground game threads → 4x boost
```

---

## 7. Core Scheduling Operations

### 7.1 select_cpu Flow

```
gamer_select_cpu(p, prev_cpu, wake_flags) SEC("struct_ops/select_cpu")
    │
    ├── Per-CPU kthread check:
    │   └── is_per_cpu_kthread() → Return bound CPU directly
    │
    ├── Preload hot path data:
    │   └── preload_hot_path_data()
    │       ├── task_ctx lookup
    │       ├── cpu_ctx lookup
    │       ├── fg_tgid check
    │       ├── input_active check
    │       └── is_busy check
    │
    ├── Sync wake fast path:
    │   └── If SCX_WAKE_SYNC && !no_wake_sync:
    │       └── Direct dispatch to waker's CPU
    │
    └── pick_idle_cpu_cached():
        │
        ├── Fast path: prev_cpu idle check
        │   └── scx_bpf_test_and_clear_cpu_idle(prev_cpu)
        │       └── ~70% hit rate, returns immediately
        │
        ├── NAPI preference (if enabled during input window):
        │   └── Check napi_last_softirq_ns[] for recent NET_RX
        │
        ├── Frame thread special handling:
        │   └── GPU/Compositor → Scan preferred_cpus[] for physical cores
        │       ├── Unrolled loop for first 4 CPUs
        │       └── Bounded loop for rest (FRAME_PHYS_SCAN_MAX)
        │
        └── General case:
            └── scx_bpf_select_cpu_and(p, prev_cpu, wake_flags, mask, smt_flags)
```

### 7.2 enqueue Flow

```
gamer_enqueue(p, enq_flags) SEC("struct_ops/enqueue")
    │
    ├── Build enqueue plan:
    │   │
    │   ├── Determine DSQ:
    │   │   ├── Direct dispatch (sync wake) → SCX_DSQ_LOCAL
    │   │   ├── Per-CPU local → SCX_DSQ_LOCAL
    │   │   └── Shared deadline → SHARED_DSQ
    │   │
    │   ├── Calculate slice:
    │   │   └── task_slice_fast(p, cctx, is_fg, input_active)
    │   │
    │   └── Calculate deadline:
    │       └── task_dl_with_ctx_cached(p, tctx, cctx, fg_tgid)
    │           │
    │           ├── Ultra-fast path (boost_shift >= 5):
    │           │   └── p->scx.dsq_vtime + (exec_runtime >> boost_shift)
    │           │
    │           ├── Frame-aware adjustment (GPU/Compositor):
    │           │   ├── Calculate time_until_next_frame
    │           │   └── Increase urgency as frame deadline approaches
    │           │
    │           └── Standard path:
    │               ├── Scale by wakeup frequency
    │               ├── Apply background penalty (8x)
    │               └── Apply page fault penalty (1.5x)
    │
    ├── Migration check:
    │   └── need_migrate() → Check migration tokens
    │       ├── Token bucket algorithm
    │       └── 32ms cooldown after migration
    │
    └── Execute plan:
        └── execute_enqueue_plan()
            ├── scx_bpf_dsq_insert_vtime() or scx_bpf_dsq_insert()
            ├── Update stats (direct/RR/EDF)
            └── Wake target CPU if needed
```

### 7.3 dispatch Flow

```
gamer_dispatch(cpu, prev_p) SEC("struct_ops/dispatch")
    │
    ├── Check local DSQ:
    │   └── scx_bpf_dsq_move_to_local(SCX_DSQ_LOCAL)
    │
    ├── Housekeeping (coalesced):
    │   ├── maybe_decay_input_windows_fast() [every ~1024 dispatches]
    │   └── maybe_run_housekeeping() [every ~5ms]
    │
    ├── Consume from shared DSQ:
    │   └── scx_bpf_dsq_move_to_local(SHARED_DSQ)
    │
    └── CPU frequency update:
        └── update_cpu_load(p, slice)
```

### 7.4 Deadline Calculation

```
Deadline Formula:
deadline = vruntime + exec_vruntime

Where:
- vruntime = accumulated runtime / weight (fairness)
- exec_vruntime = runtime since last sleep / weight (interactivity)

Boost Application:
- boost_shift=7 (input): deadline = vtime + (exec >> 7) = ~10x boost
- boost_shift=6 (GPU):   deadline = vtime + (exec >> 6) = ~8x boost
- boost_shift=5 (comp):  deadline = vtime + (exec >> 5) = ~7x boost
- boost_shift=0 (std):   deadline = vtime + exec (no boost)

Background Penalty:
- is_background: exec_component << 3 (8x later deadline)
- Non-foreground: exec_component << 3 (8x penalty)
```

---

## 8. Optimization Systems

### 8.1 Performance Tiers

```
TIER 0 (0-5ns): Zero-cost operations
├── Compile-time constants
├── Bitwise operations
├── Cached flag checks (task_struct->scx.flags)
└── Volatile BSS reads

TIER 1 (5-50ns): Fast path operations
├── scx_bpf_now() timestamp
├── Task storage lookup
├── Per-CPU array lookup
└── Idle CPU test/clear

TIER 2 (50-200ns): Medium cost
├── Hash map lookup
├── CPUmask operations
└── Multiple map lookups

TIER 3 (200-500ns): Avoid in hot path
├── Shared map with contention
├── Name pattern matching
└── Classification slow path
```

### 8.2 Cache Optimization

```
task_ctx Layout (384 bytes, 6 cache lines):

Cache Line 1 (0-63 bytes) - ULTRA HOT:
├── Classification flags (is_input_handler, is_gpu_submit, etc.)
├── boost_shift
├── preferred_physical_core
└── exec_runtime, last_run_at, wakeup_freq

Cache Line 2+ (64+ bytes) - Cold:
├── Migration tokens
├── Audio metrics
├── Deadline miss detection
└── Frame feedback state

cpu_ctx Layout (128 bytes, 2 cache lines):
├── vtime_now, interactive_avg (hot)
├── Per-CPU stat accumulators (no atomics needed!)
└── CPUfreq state
```

### 8.3 LMAX Disruptor Patterns

```
Ring Buffer Distribution:
├── 16 per-CPU ring buffers (input_events_ringbuf_0 through _15)
├── CPU ID % 16 selects buffer
├── Single-writer guarantee eliminates contention
└── Expected: ~20-50ns per write (vs ~100ns+ with contention)

Hot Path Signal Structure (hotpath_signals):
├── input_ns[MAX_CPUS] - Per-CPU input timestamps
├── Cache line padding between fields
└── Eliminates false sharing (~10-20ns savings)

Per-CPU Stat Accumulators:
├── local_nr_idle_cpu_pick, local_nr_migrations, etc.
├── No atomic operations in hot path
├── Aggregated periodically by housekeeping timer
└── Saves ~30-50ns per stat increment
```

### 8.4 Frame Feedback Loop

```
Frame Timing Tracking:
├── last_page_flip_ns - Last VSync/page flip
├── frame_interval_ns - EMA of frame time
└── frame_count - Total frames presented

Frame-Aware Deadline Adjustment:
├── Calculate time_until_next_frame
├── GPU threads: 25% deadline reduction near frame boundary
└── Compositor: 50% deadline reduction near frame boundary

Feedback Loop (GPU/Compositor threads):
├── Track frame_deadline_seen per thread
├── frame_miss_streak++ on deadline miss
├── frame_feedback_boost++ after 2 consecutive misses
├── frame_feedback_boost-- after 3 on-time frames
└── Automatic boost decay prevents permanent over-boosting
```

### 8.5 Schedulability Analysis (Liu & Layland 1973)

```
Rate Monotonic Scheduling (RMS):
├── Periodic tasks get rms_priority based on period
├── Shorter period = higher priority
└── Used for audio (buffer-aware), frame timing

EDF Schedulability Check:
├── is_schedulable(total_util, use_rms)
├── EDF bound: U ≤ 100%
├── RMS bound: U ≤ n(2^(1/n) - 1) ≈ 69% for large n
└── Only enable EDF mode if schedulable
```

---

## 9. Data Structures

### 9.1 Task Context (`task_ctx`)

```c
struct CACHE_ALIGNED task_ctx {
    /* Classification flags (byte 0) */
    u8 is_input_handler:1;
    u8 is_gpu_submit:1;
    u8 is_compositor:1;
    u8 is_network:1;
    u8 is_system_audio:1;
    u8 is_game_audio:1;
    u8 is_nvme_io:1;
    u8 is_background:1;
    // ... more flags
    
    /* Precomputed boost (byte 1) */
    u8 boost_shift;  /* 0=none, 7=10x for input */
    
    /* Hot scheduling data */
    u64 exec_runtime;
    u64 last_run_at;
    u64 wakeup_freq;  /* EMA of wakeups */
    
    /* Cold data (cache line 2+) */
    u64 mig_tokens;
    u64 expected_deadline;
    u32 deadline_misses;
    
    /* Frame feedback */
    u8 frame_feedback_boost;
    u16 frame_miss_streak;
    u16 frame_hit_streak;
};
```

### 9.2 CPU Context (`cpu_ctx`)

```c
struct CACHE_ALIGNED cpu_ctx {
    /* Hot path scheduling */
    u64 vtime_now;
    u64 interactive_avg;
    
    /* Per-CPU stat accumulators (NO atomics!) */
    u64 local_nr_idle_cpu_pick;
    u64 local_nr_direct_dispatches;
    u64 local_nr_migrations;
    
    /* CPUfreq */
    u64 last_update;
    u64 perf_lvl;
};
```

### 9.3 Key BPF Maps

| Map | Type | Key | Value | Purpose |
|-----|------|-----|-------|---------|
| `task_ctx_stor` | TASK_STORAGE | task_struct | task_ctx | Per-task scheduling context |
| `cpu_ctx_stor` | PERCPU_ARRAY | 0 | cpu_ctx | Per-CPU scheduling state |
| `gpu_threads_map` | HASH | TID | gpu_thread_info | GPU thread tracking |
| `system_audio_tgids_map` | LRU_HASH | TGID | system_audio_entry | Audio server processes |
| `graphics_api_map` | HASH | TGID | u8 | DX11/DX12 API mode |
| `engine_profile_map` | LRU_HASH | comm | engine_profile_entry | Thread name → behavior cache |
| `input_events_ringbuf_N` | RINGBUF | - | gamer_input_event | Per-CPU input events |
| `process_events` | RINGBUF | - | process_event | Game detection events |

---

## 10. File Manifest

### Rust Source (`src/`)

| File | Purpose |
|------|---------|
| `main.rs` | Entry point, scheduler lifecycle, event loop |
| `bpf_skel.rs` | Generated BPF skeleton bindings |
| `bpf_intf.rs` | Interface types shared with BPF |
| `game_detect.rs` | Inotify-based game detection (fallback) |
| `game_detect_bpf.rs` | BPF LSM game detection (preferred) |
| `ring_buffer.rs` | Input event ring buffer manager |
| `cpu_detect.rs` | CPU model detection |
| `audio_detect.rs` | Audio server detection |
| `affinity_override.rs` | Proactive CPU affinity system |
| `gpu_queue_monitor.rs` | GPU busy state tracking |
| `power_monitor.rs` | Battery/thermal awareness |
| `engine_presets.rs` | Game engine profile seeding |
| `stats.rs` | Metrics collection and export |
| `tui.rs` | Terminal UI dashboard |
| `debug_api.rs` | HTTP debug endpoint |
| `ml_*.rs` | Machine learning autotune (experimental) |
| `trigger.rs` | BPF syscall triggers |

### BPF Source (`src/bpf/`)

| File | Purpose |
|------|---------|
| `main.bpf.c` | Core scheduling operations |
| `intf.h` | Shared constants and types |
| `game_detect_lsm.bpf.c` | Process exec detection |
| `include/types.bpf.h` | Data structures, maps |
| `include/helpers.bpf.h` | Utility functions |
| `include/boost.bpf.h` | Input/frame boost windows |
| `include/task_class.bpf.h` | Thread classification |
| `include/cpu_select.bpf.h` | CPU selection algorithms |
| `include/gpu_detect.bpf.h` | GPU fentry hooks |
| `include/audio_detect.bpf.h` | Audio fentry hooks |
| `include/network_detect.bpf.h` | Network fentry hooks |
| `include/storage_detect.bpf.h` | Storage fentry hooks |
| `include/compositor_detect.bpf.h` | Compositor detection |
| `include/memory_detect.bpf.h` | Memory operation tracking |
| `include/interrupt_detect.bpf.h` | IRQ handler tracking |
| `include/filesystem_detect.bpf.h` | Filesystem operation tracking |
| `include/coalesce.bpf.h` | Dispatch coalescing |
| `include/config.bpf.h` | Build-time configuration |
| `include/profiling.bpf.h` | Performance instrumentation |

---

## Appendix: Quick Reference

### Boost Shift Values

| Value | Priority | Thread Type | Effect |
|-------|----------|-------------|--------|
| 7 | Maximum | Input handlers | 10x faster deadline |
| 6 | Very High | GPU submit | 8x faster deadline |
| 5 | High | Compositor | 7x faster deadline |
| 4 | Medium-High | Ethernet NIC IRQ | 6x faster deadline |
| 3 | Medium | Network, gaming traffic | 5x faster deadline |
| 2 | Low-Medium | USB audio | 4x faster deadline |
| 1 | Low | Audio, peripheral, storage | 3x faster deadline |
| 0 | Standard | Unclassified | Normal deadline |

### Key Tunables (CLI)

| Option | Default | Description |
|--------|---------|-------------|
| `--slice-us` | 10 | Base time slice (µs) |
| `--input-window-us` | 5000 | Input boost window (µs) |
| `--keyboard-boost-us` | 1000000 | Keyboard boost duration (µs) |
| `--mouse-boost-us` | 8000 | Mouse boost duration (µs) |
| `--mig-window-ms` | 50 | Migration window (ms) |
| `--mig-max` | 3 | Max migrations per window |
| `--avoid-smt` | false | Avoid SMT siblings |
| `--enable-numa` | false | NUMA-aware placement |

---

*Last Updated: 2025-11-24*
*Version: 1.0.2*

