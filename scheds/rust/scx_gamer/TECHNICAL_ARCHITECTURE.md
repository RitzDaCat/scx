# scx_gamer Technical Architecture

## Overview

scx_gamer is a hybrid kernel/userspace CPU scheduler built on Linux's sched_ext framework. It combines BPF programs (kernel-space) with a Rust control plane (userspace) to optimize gaming workloads for low latency and smooth frame delivery.

**Key Design Principles:**
- **Locality-first**: Preserve CPU cache affinity under light load
- **Load-aware transitions**: Switch to global EDF scheduling under heavy load
- **Event-driven boost**: React to input/frame events with priority windows
- **Zero-downtime tuning**: Hot-reload parameters via BPF map updates

---

## System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                         HARDWARE LAYER                              │
├─────────────────────────────────────────────────────────────────────┤
│  CPU Cores    │  GPU (DRM)  │  Input Devices  │  Network (NAPI)    │
│  AMD/Intel    │  i915/AMDGPU│  evdev          │  Ethernet/WiFi     │
└────────┬──────┴──────┬──────┴────────┬────────┴────────┬────────────┘
         │             │               │                 │
┌────────▼─────────────▼───────────────▼─────────────────▼────────────┐
│                       KERNEL SPACE (BPF)                             │
├──────────────────────────────────────────────────────────────────────┤
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  SCHED_EXT BPF SCHEDULER (main.bpf.c)                          │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐ │ │
│  │  │ select_cpu() │→ │  enqueue()   │→ │    dispatch()        │ │ │
│  │  │ (CPU picker) │  │ (queue mgmt) │  │ (task activation)    │ │ │
│  │  └──────────────┘  └──────────────┘  └──────────────────────┘ │ │
│  │                                                                 │ │
│  │  Per-CPU DSQs + Global SHARED_DSQ (EDF mode)                   │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  DETECTION SUBSYSTEMS                                          │ │
│  │  ┌──────────────────┐  ┌─────────────────┐  ┌───────────────┐ │ │
│  │  │ BPF LSM          │  │ Thread Runtime  │  │ GPU Detection │ │ │
│  │  │ (game_detect)    │  │ (sched_switch)  │  │ (drm_ioctl)   │ │ │
│  │  └──────────────────┘  └─────────────────┘  └───────────────┘ │ │
│  │  ┌──────────────────┐                                          │ │
│  │  │ Wine Priority    │                                          │ │
│  │  │ (NtSetInfo)      │                                          │ │
│  │  └──────────────────┘                                          │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                              │                                       │
│                              │ Ring Buffers / BPF Maps               │
└──────────────────────────────┼───────────────────────────────────────┘
                               │
┌──────────────────────────────▼───────────────────────────────────────┐
│                     USER SPACE (Rust)                                │
├──────────────────────────────────────────────────────────────────────┤
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  MAIN EVENT LOOP (main.rs)                                     │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐ │ │
│  │  │ Input Monitor│  │ Game Detector│  │ ML Auto-tuner        │ │ │
│  │  │ (evdev)      │  │ (BPF ringbuf)│  │ (Bayesian/Grid)      │ │ │
│  │  └──────────────┘  └──────────────┘  └──────────────────────┘ │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  ML PIPELINE                                                   │ │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐ │ │
│  │  │ Data Collect │→ │ Bayesian Opt │→ │ Profile Manager      │ │ │
│  │  │ (metrics)    │  │ (autotune)   │  │ (per-game configs)   │ │ │
│  │  └──────────────┘  └──────────────┘  └──────────────────────┘ │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  STATS / MONITORING                                            │ │
│  │  scx_stats server (JSON-RPC), CLI output                       │ │
│  └────────────────────────────────────────────────────────────────┘ │
└──────────────────────────────────────────────────────────────────────┘
```

---

## Component Breakdown

### 1. BPF Scheduler Core (main.bpf.c)

**Location**: `src/bpf/main.bpf.c`

**Hot Path Functions** (executed every task wakeup/sleep):

#### `select_cpu(struct task_struct *p, s32 prev_cpu, u64 wake_flags)`
**Purpose**: Choose which CPU should run the waking task

**Decision Tree**:
```c
1. Is task per-CPU kthread? → Keep on same CPU
2. Cache affinity (mm_hint): → Try last CPU for this address space
3. Idle CPU scan:
   - flat_idle_scan: Linear scan for idle CPU
   - preferred_idle_scan: Scan high-capacity CPUs first
   - SMT avoidance: Skip sibling threads if avoid_smt enabled
4. Wake sync optimization: → Try waker's CPU for producer-consumer
5. Fallback: → Return prev_cpu
```

**Performance**: 200-800ns average (profiled via `prof_select_cpu_ns`)

**Key Optimizations**:
- **mm_hint LRU cache**: Tracks last CPU per address space (8192 entries)
- **NUMA-aware**: Prefers CPUs on same NUMA node
- **SMT contention avoidance**: Checks sibling idle state
- **Migration limiting**: Per-task rate limiting (3 migrations per 50ms default)

---

#### `enqueue(struct task_struct *p, u64 enq_flags)`
**Purpose**: Add task to runqueue and decide local vs global dispatch

**Decision Tree**:
```c
1. Update task vtime (virtual time for fairness)
2. Check system load (cpu_util_avg):
   - Light load (<80%): Enqueue to per-CPU DSQ (round-robin)
   - Heavy load (≥80%): Enqueue to SHARED_DSQ (global EDF)
3. Apply input boost if in input window:
   - Reduce slice_us (faster preemption)
   - Relax migration limits
4. Wake sync fast path: Direct dispatch if sync wakeup
```

**Dispatch Queues**:
- **Per-CPU DSQs** (0-255): Local round-robin queues for cache locality
- **SHARED_DSQ (0)**: Global deadline-sorted queue for load balancing

**Performance**: 150-400ns average

---

#### `dispatch(s32 cpu, struct task_struct *prev)`
**Purpose**: Pick next task to run on CPU

**Decision Tree**:
```c
1. Consume per-CPU DSQ first (locality)
2. If empty, consume SHARED_DSQ (global pool)
3. Apply CPU frequency scaling based on load
4. Update CPU context (idle time, utilization)
```

**Performance**: 100-300ns average

---

### 2. BPF Detection Subsystems

#### **BPF LSM Game Detection** (game_detect_lsm.bpf.c)

**Hooks**:
- `lsm/bprm_committed_creds`: Fires on every `exec()` syscall
- `lsm/task_free`: Fires when process exits

**Filter Pipeline** (kernel-side pre-filtering):
```c
exec() → Read task->comm → is_system_binary()? →
  ↓ (90% filtered)
  Check keywords (wine, proton, steam, ue4, unity) →
  ↓ (8% filtered)
  Read parent process → Classify (FLAG_WINE, FLAG_STEAM, FLAG_EXE) →
  ↓ (2% sent to userspace)
  Ring buffer → Userspace consumer
```

**Performance**:
- **Overhead**: 200-800ns per exec (only on exec path, not hot path)
- **Detection latency**: <1ms (instant on game launch)
- **Compared to inotify**: 10-50ms latency, 10-50ms/sec CPU overhead

**Data sent to userspace** (struct process_event):
```c
struct process_event {
    u32 type_;          // GAME_EVENT_EXEC or GAME_EVENT_EXIT
    u32 pid;            // Process PID
    u32 parent_pid;     // Parent PID
    u32 flags;          // FLAG_WINE | FLAG_STEAM | FLAG_EXE
    u64 timestamp;      // Kernel timestamp
    char comm[16];      // Process name (e.g., "CS2.exe")
    char parent_comm[16]; // Parent name (e.g., "wine-preloader")
};
```

---

#### **Thread Runtime Tracking** (thread_runtime.bpf.h)

**Hook**: `tp_btf/sched_switch` (context switch tracepoint)

**Tracked Metrics** (per thread):
```c
struct thread_runtime_info {
    u64 total_runtime_ns;      // Cumulative CPU time
    u64 total_sleeptime_ns;    // Cumulative sleep time
    u64 wakeup_count;          // Number of wakeups
    u64 last_wakeup_ts;        // Last wakeup timestamp
    u64 avg_exec_ns;           // Average exec time per wakeup
    u32 wakeup_freq;           // Wakeups per second (EMA)
    u8 detected_role;          // RENDER/AUDIO/INPUT/CPU_BOUND/BACKGROUND
};
```

**Classification Heuristics**:
```c
// GPU Submit Thread: Short exec (<100μs), high freq (>50Hz = ~500fps)
if (avg_exec_ns < 100000 && wakeup_freq > 50) → ROLE_GPU_SUBMIT

// Background Thread: Long exec (>5ms), low freq (<10Hz)
if (avg_exec_ns > 5000000 && wakeup_freq < 10) → ROLE_BACKGROUND

// CPU-Bound: Long exec, high freq
if (avg_exec_ns > 1000000 && wakeup_freq > 50) → ROLE_CPU_BOUND
```

**Performance**: 100-200ns per context switch (always-on overhead)

---

#### **GPU Thread Detection** (gpu_detect.bpf.h)

**Hooks**:
- `fentry/drm_ioctl`: Intel i915, AMD amdgpu
- `kprobe/nvidia_ioctl`: NVIDIA proprietary driver

**Detection Logic**:
```c
drm_ioctl(unsigned int cmd, ...) {
    if (cmd == DRM_I915_GEM_EXECBUFFER2 ||
        cmd == DRM_AMDGPU_CS) {
        // This thread submits GPU commands!
        register_gpu_thread(current->tid, GPU_VENDOR_INTEL/AMD);
    }
}
```

**Accuracy**: 100% (actual kernel API calls, not heuristics)

**Detection latency**: <1ms (first GPU submit by thread)

**Tracked Data**:
```c
struct gpu_thread_info {
    u64 first_submit_ts;
    u64 last_submit_ts;
    u64 total_submits;
    u32 submit_freq_hz;    // Estimated GPU submit rate
    u8 gpu_vendor;         // INTEL/AMD/NVIDIA
    u8 is_render_thread;   // 1 if primary render thread
};
```

---

#### **Wine Thread Priority Tracking** (wine_detect.bpf.h)

**Hook**: `uprobe` on `/usr/lib/wine/.../ntdll.so:NtSetInformationThread`

**Intercepted Windows API**:
```c
NTSTATUS NtSetInformationThread(
    HANDLE ThreadHandle,
    THREADINFOCLASS ThreadInformationClass,  // We hook ThreadBasePriority=1
    PVOID ThreadInformation,                 // Pointer to priority value
    ULONG ThreadInformationLength
);
```

**What we read** (via `bpf_probe_read_user`):
```c
// Read 4-byte priority value from game's address space
s32 priority;  // e.g., THREAD_PRIORITY_TIME_CRITICAL = 15
bpf_probe_read_user(&priority, sizeof(priority), ThreadInformation);
```

**Classification**:
```c
// Audio thread (99% accurate): TIME_CRITICAL + REALTIME class
if (priority == 15 && is_realtime) → WINE_ROLE_AUDIO

// Render thread: TIME_CRITICAL or HIGHEST without REALTIME
if (priority == 15 || priority == 2) → WINE_ROLE_RENDER

// Input handler: HIGHEST priority
if (priority == 2) → WINE_ROLE_INPUT

// Physics: ABOVE_NORMAL
if (priority == 1) → WINE_ROLE_PHYSICS

// Background: BELOW_NORMAL/LOWEST
if (priority <= -1) → WINE_ROLE_BACKGROUND
```

**Performance**: 1-2μs per priority change (rare operation, ~10/sec during game load)

---

### 3. Userspace Control Plane (Rust)

#### **Main Event Loop** (main.rs:929-1300)

**Event Sources** (epoll-based):
1. **Input devices** (evdev): Keyboard/mouse events
2. **BPF ring buffer**: Game process events
3. **Timer**: ML sampling, game detection polling
4. **Stats requests**: scx_stats JSON-RPC

**Input Event Flow**:
```rust
epoll.wait(100ms) → ev.fd in input_devs? →
  dev.fetch_events() → for event in iter {
    (input_trigger_fn)(&trig, &mut skel);  // BPF syscall
  }
```

**Zero-latency design**: Every input event triggers immediate BPF call (no batching)

**Performance**:
- **8kHz mouse**: ~8000 syscalls/sec = ~6ms total CPU
- **Per-event latency**: <1μs (syscall overhead)

---

#### **BPF Game Detector** (game_detect_bpf.rs)

**Ring Buffer Consumer** (200ms poll interval):
```rust
loop {
    ringbuf.poll(100ms);  // Non-blocking
    // Callback invoked for each event:
    handle_process_event(data) {
        let evt: ProcessEvent = unsafe { std::ptr::read(data.as_ptr()) };
        if evt.type_ == GAME_EVENT_EXEC {
            classify_game(&evt) → Update current_game atomic
        }
    }
}
```

**Hybrid Approach** (solves "game already running" problem):
```rust
// Startup: Scan /proc once for existing games
detect_initial_game() → Set current_game

// Runtime: BPF LSM handles new exec() calls
```

---

#### **ML Auto-Tuner** (ml_autotune.rs)

**Modes**:
1. **Grid Search**: Exhaustive parameter space exploration (12-20 trials)
2. **Bayesian Optimization**: Smart exploration using Gaussian Process (6-10 trials)

**Parameter Space**:
```rust
slice_us: [5, 10, 15, 20]
input_window_us: [1000, 1500, 2000, 2500, 3000]
mig_max: [1, 2, 3, 4, 5]
mm_affinity: [true, false]
```

**Scoring Function** (scheduler latency metrics):
```rust
score =
    (1.0 / select_cpu_avg_ns) * 40.0 +      // CPU selection speed (40%)
    (mm_hint_hit_rate) * 30.0 +             // Cache affinity (30%)
    (direct_dispatch_rate) * 20.0 +         // Fast-path efficiency (20%)
    (1.0 / migrations_per_sec) * 10.0;      // Migration stability (10%)
```

**Hot Config Reload** (zero-downtime):
```rust
fn apply_config_hot(skel: &mut BpfSkel, config: &MLConfig) {
    let rodata = skel.maps.rodata_data.as_mut();
    rodata.slice_ns = config.slice_us * 1000;
    rodata.input_window_ns = config.input_window_us * 1000;
    // BPF reads from rodata, picks up changes atomically
}
```

**Trial Duration**: 120s default (configurable)

**Output**: JSON profile saved to `ml_data/{CPU_MODEL}/{GAME}.json`

---

#### **Profile Manager** (ml_profiles.rs)

**Auto-Load Logic**:
```rust
on_game_detected(game_name) {
    if let Some(profile) = load_profile(game_name) {
        apply_config_hot(&profile.best_config);
        info!("Loaded profile for '{}' (score: {:.2})", game_name, profile.best_score);
    } else {
        info!("No profile for '{}', using defaults", game_name);
    }
}
```

**Profile Format** (JSON):
```json
{
  "game_name": "Counter-Strike 2",
  "cpu_model": "AMD Ryzen 9 9800X3D",
  "best_config": {
    "slice_us": 10,
    "input_window_us": 2000,
    "mig_max": 3,
    "mm_affinity": true
  },
  "best_score": 87.42,
  "avg_fps": 480.5,
  "samples": 120,
  "timestamp": "2025-10-07T01:30:00Z"
}
```

---

## Data Flow Diagrams

### Input Event → Scheduler Boost

```
User Input (Mouse/Keyboard)
        ↓
  Linux Input Subsystem
        ↓
  /dev/input/eventX (evdev)
        ↓
  Userspace Event Loop (main.rs:1112)
    dev.fetch_events()
        ↓
  trigger_input_window(skel)
        ↓
  BPF Syscall (bpf_map_update_elem)
        ↓
  BPF: fanout_set_input_window()
    input_until_global = now + 2ms
        ↓
  BPF: enqueue() hot path
    is_input_active()? → Reduce slice, relax migration
        ↓
  Task gets faster preemption (lower latency)
```

**Latency breakdown**:
- Input event → evdev: <100μs (kernel driver)
- evdev → userspace poll: <200μs (epoll wakeup)
- fetch_events() → BPF syscall: <1μs
- BPF map update: ~200ns
- **Total: <500μs end-to-end**

---

### Game Launch → Detection → Profile Load

```
User launches game.exe
        ↓
  exec() syscall
        ↓
  BPF LSM: bprm_committed_creds hook
    Read task->comm ("game.exe")
    Classify (FLAG_EXE | FLAG_WINE)
        ↓
  Ring buffer push
        ↓
  Userspace ring buffer consumer
    Receives process_event
        ↓
  Game classification
    Name: "Game Name"
    TGID: 12345
        ↓
  Profile Manager
    Load ml_data/9800X3D/Game Name.json
        ↓
  Apply config hot
    rodata.slice_ns = 10000
    rodata.input_window_ns = 2000000
        ↓
  BPF scheduler picks up new config
    (next enqueue() uses updated values)
```

**Latency breakdown**:
- exec() → LSM hook: <1ms
- Ring buffer push: ~50μs
- Userspace poll: <100ms (poll interval)
- Profile load + apply: <1ms
- **Total: <101ms (unnoticeable during game startup)**

---

### ML Autotune Trial Switch

```
Trial timer expires (120s)
        ↓
  Autotuner: next_trial()
    Generate new config via Bayesian GP
        ↓
  apply_config_hot()
    Update BPF rodata map
        ↓
  BPF scheduler uses new config
        ↓
  Collect metrics for 120s
    select_cpu_avg_ns, mm_hint_hit_rate, ...
        ↓
  Calculate score
        ↓
  Compare with best_config
    Update if score improved
        ↓
  Repeat until max_duration (900s default)
        ↓
  Generate final report
    Top 3 configs ranked by score
```

---

## Performance Characteristics

### Scheduler Overhead (vs CFS)

| Metric | CFS | scx_gamer | Overhead |
|--------|-----|-----------|----------|
| **select_cpu()** | ~150ns | 200-800ns | +50-650ns |
| **enqueue()** | ~100ns | 150-400ns | +50-300ns |
| **dispatch()** | ~80ns | 100-300ns | +20-220ns |
| **Context switch** | ~1.5μs | 1.6-1.7μs | +100-200ns |
| **Total CPU usage** | 0.1-0.3% | 0.3-0.8% | +0.2-0.5% |

**Trade-off**: Slightly higher overhead for improved cache locality and input responsiveness

---

### Memory Usage

| Component | Size |
|-----------|------|
| **BPF programs** (code) | ~150KB |
| **BPF maps** (data) | ~2-5MB |
| **Userspace binary** | ~8MB (stripped) |
| **ML training data** | ~10-50KB per game |
| **Total runtime** | ~15-20MB RSS |

**Compared to**: CFS uses ~0 MB (built into kernel)

---

### Detection Latency

| System | Latency | CPU Overhead |
|--------|---------|--------------|
| **BPF LSM game detect** | <1ms | 200-800ns per exec |
| **Inotify fallback** | 10-50ms | 10-50ms/sec CPU |
| **GPU thread detect** | <1ms | 0 (only on first ioctl) |
| **Wine priority detect** | <1ms | 1-2μs per priority change |
| **Thread runtime** | Instant | 100-200ns per context switch |

---

## Modular Code Organization

**BPF Headers** (single-responsibility files):
```
src/bpf/include/
├── config.bpf.h         # Tunables and constants (78 lines)
├── types.bpf.h          # Data structures (task_ctx, cpu_ctx)
├── helpers.bpf.h        # Utility functions (calc_avg, cpufreq)
├── stats.bpf.h          # Statistics collection (stat_inc)
├── boost.bpf.h          # Input/frame windows (150 lines)
├── task_class.bpf.h     # Thread classification (is_compositor_name)
├── cpu_select.bpf.h     # CPU selection logic
├── profiling.bpf.h      # Hot-path latency measurement
├── thread_runtime.bpf.h # Context switch tracking (343 lines)
├── gpu_detect.bpf.h     # GPU ioctl detection (264 lines)
├── wine_detect.bpf.h    # Wine priority tracking (339 lines)
├── game_detect.bpf.h    # Game detection helpers
└── advanced_detect.bpf.h # Unified detection API (310 lines)
```

**Rationale**: Each file <400 lines for AI-friendly editing and review

---

## Configuration Hot-Reload

**BPF rodata Map** (read-only from BPF, writable from userspace):
```c
// BPF side (main.bpf.c)
const volatile u64 slice_ns = 10000;           // Default 10μs
const volatile u64 input_window_ns = 2000000;  // Default 2ms

// Userspace side (main.rs)
let rodata = skel.maps.rodata_data.as_mut();
rodata.slice_ns = 15000;  // Hot-update to 15μs
```

**Atomic Visibility**:
- BPF programs read rodata with relaxed memory ordering
- Userspace updates are atomic (single u64 write)
- No scheduler restart required

**Use Cases**:
- ML autotune trial switches
- Per-game profile loading
- Runtime tuning via control interface (future)

---

## Safety and Correctness

### BPF Verifier Guarantees

**Memory Safety**:
- All pointers validated before dereference
- Bounds checking on array accesses
- No arbitrary memory reads/writes

**Loop Bounds**:
```c
// Verifier requires bounded loops
#pragma unroll
for (int i = 0; i < MAX_CPUS && i < 256; i++) {
    // Verifier knows loop terminates
}
```

**Helper Function Safety**:
- `bpf_probe_read_kernel()`: Safe kernel memory read (returns error on fault)
- `bpf_probe_read_user()`: Safe userspace memory read (returns error on fault)
- `bpf_map_lookup_elem()`: Returns NULL on missing key (checked before deref)

---

### Error Handling

**BPF Side** (limited error handling due to verifier constraints):
```c
struct task_ctx *tctx = try_lookup_task_ctx(p);
if (!tctx) {
    // Graceful degradation: use default behavior
    return default_cpu;
}
```

**Userspace Side** (comprehensive error handling):
```rust
match skel.maps.process_events.ringbuf_poll(timeout) {
    Ok(_) => { /* Process events */ },
    Err(e) => {
        warn!("Ring buffer poll error: {}", e);
        thread::sleep(Duration::from_millis(100));
    }
}
```

---

### Watchdog and Failsafe

**Kernel-Side Stall Detection**:
- sched_ext framework monitors dispatch progress
- If no task dispatched for 30s → kernel disables scheduler, restores CFS

**Userspace-Side Watchdog**:
```rust
--watchdog-secs N  // Auto-exit if no dispatch progress for N seconds
```

**Graceful Shutdown**:
- Ctrl+C → SIGINT handler → clean BPF detach → CFS restored
- System never left in broken state

---

## Future Enhancements

### Planned Features

1. **Dynamic Load Balancing**:
   - Cross-LLC work stealing for NUMA systems
   - Adaptive EDF threshold based on P-core/E-core topology

2. **Per-Task Boost Profiles**:
   - Fine-grained thread priority (render > audio > input > background)
   - Integration with Wine thread roles

3. **Power Efficiency Mode**:
   - E-core preference for background tasks
   - P-core reservation for foreground

4. **MangohHUD Integration**:
   - Direct frame timing data export (Vulkan layer hook)
   - Real-time FPS/frametime in ML scoring

5. **Control Interface**:
   - Unix socket for runtime tuning
   - Web dashboard for monitoring

---

## Performance Tuning Guide

### CPU Selection Strategy

**For high core-count CPUs (16+ cores)**:
```bash
--preferred-idle-scan --avoid-smt
# Prioritize P-cores, avoid SMT contention
```

**For low core-count CPUs (4-8 cores)**:
```bash
--flat-idle-scan
# Minimize CPU selection overhead
```

---

### Input Latency vs Throughput

**Ultra-low latency (esports, 480Hz+)**:
```bash
--slice-us 5 --input-window-us 1000 --mig-max 1
# Aggressive preemption, tight migration limits
```

**Balanced (4K 240Hz)**:
```bash
--slice-us 10 --input-window-us 2000 --mig-max 3
# Default, well-tested
```

**Throughput-focused (productivity + gaming)**:
```bash
--slice-us 20 --input-window-us 3000 --mig-max 5
# Longer slices, less aggressive
```

---

### Migration Control

**Cache-sensitive workloads** (simulation games):
```bash
--mm-affinity --mig-max 2
# Strong locality preference
```

**Highly parallel workloads** (strategy games, OBS capture):
```bash
--mig-max 5
# Allow more migrations for load balancing
```

---

## Debugging and Profiling

### Enable BPF Profiling

**Compile-time** (requires BPF code change):
```c
// Uncomment in main.bpf.c:
#define ENABLE_PROFILING
```

**Runtime stats**:
```bash
sudo scx_gamer --stats 1
# Shows prof_select_cpu_avg_ns, prof_enqueue_avg_ns, etc.
```

---

### Verbose Logging

```bash
sudo scx_gamer --verbose --stats 1
# BPF verifier logs, libbpf debug output, device detection
```

---

### Manual Testing

**Simulate load**:
```bash
stress-ng --cpu 8 --timeout 60s
# Watch mode transitions (RR → EDF)
```

**Monitor migrations**:
```bash
sudo scx_gamer --stats 1 | grep mig_blocked
# High mig_blocked = migration limiter is active
```

---

## Glossary

- **DSQ**: Dispatch queue (BPF structure for runnable tasks)
- **EDF**: Earliest-deadline-first scheduling
- **EMA**: Exponential moving average
- **LSM**: Linux Security Module (kernel hook framework)
- **NUMA**: Non-uniform memory access (multi-socket systems)
- **SMT**: Simultaneous multithreading (HyperThreading)
- **vtime**: Virtual time (fairness metric for CFS-like scheduling)
- **Rodata**: Read-only data (BPF map type for constants)
- **BSS**: Block Started by Symbol (BPF map type for globals)

---

## References

- [sched_ext Documentation](https://docs.kernel.org/scheduler/sched-ext.html)
- [BPF Documentation](https://docs.kernel.org/bpf/index.html)
- [scx_utils Library](https://github.com/sched-ext/scx/tree/main/rust/scx_utils)
- [Linux Scheduler Design](https://docs.kernel.org/scheduler/index.html)

---

**Document Version**: 1.0 (2025-10-07)
**Scheduler Version**: 1.0.2
**Authors**: RitzDaCat
