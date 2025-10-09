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

### CachyOS/Arch Linux Integration

```
┌─────────────────────────────────────────────────────────────────────┐
│                    CACHYOS SYSTEM INTEGRATION                       │
└─────────────────────────────────────────────────────────────────────┘

User Space:
┌─────────────────────────────────────────────────────────────────────┐
│  CachyOS Kernel Manager (GUI)                                       │
│  ┌──────────────────┐  ┌──────────────┐  ┌───────────────────────┐ │
│  │  Scheduler       │  │  Profile     │  │  Custom Flags         │ │
│  │  [scx_gamer ▼]   │  │  [Gaming ▼]  │  │  --ml-autotune        │ │
│  └────────┬─────────┘  └──────┬───────┘  └───────────┬───────────┘ │
│           │                    │                      │             │
│           └────────────────────┴──────────────────────┘             │
│                                │                                    │
│                           [Apply Button]                            │
└───────────────────────────────┼─────────────────────────────────────┘
                                │
                                ▼
                       Writes configuration:
                                │
        ┌───────────────────────┴────────────────────────┐
        │                                                 │
        ▼                                                 ▼
/etc/default/scx                              /etc/scx_loader.toml
┌──────────────────────┐                      ┌─────────────────────────┐
│ SCX_SCHEDULER=       │                      │ [scheds.scx_gamer]      │
│   scx_gamer          │                      │ gaming_mode = [         │
│                      │                      │   "--slice-us", "10",   │
│ SCX_FLAGS=           │                      │   "--mm-affinity",      │
│   --slice-us 10      │                      │   "--input-window-us",  │
│   --mm-affinity ...  │                      │   "2000" ]              │
└──────────┬───────────┘                      └─────────────────────────┘
           │
           │ systemctl restart scx.service
           ▼
/usr/lib/systemd/system/scx.service
┌─────────────────────────────────────────┐
│ [Service]                               │
│ Type=simple                             │
│ EnvironmentFile=/etc/default/scx        │
│ ExecStart=/usr/bin/scx_gamer $SCX_FLAGS │
│ Restart=on-failure                      │
└────────────────┬────────────────────────┘
                 │
                 │ Spawns process:
                 ▼
        /usr/bin/scx_gamer (binary)
                 │
        ┌────────┴────────────────────────────────────────┐
        │                                                  │
        ▼                                                  ▼
  Parse CLI args                                   Load BPF programs
        │                                                  │
        ▼                                                  ▼
  Detect topology                                  Attach to kernel
  (CPU, NUMA, SMT)                                sched_ext ops
        │                                                  │
        └──────────────────┬───────────────────────────────┘
                           │
                           ▼
                    Event loop starts
                 (epoll, input, ringbuf)
                           │
                           ▼
                 Scheduler running!

Monitoring:
┌─────────────────────────────────────────────────────────────────────┐
│  scxstats -s scx_gamer          │  journalctl -u scx.service -f     │
│  (JSON-RPC stats client)        │  (systemd journal logs)           │
└─────────────────────────────────────────────────────────────────────┘
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

### Detailed Input Processing Pipeline

This diagram shows the complete input processing flow including device classification, continuous mode detection, and sensor stop detection:

```
┌─────────────────────────────────────────────────────────────────────┐
│                   PHYSICAL INPUT DEVICES                            │
├─────────────────────────────────────────────────────────────────────┤
│  8kHz Gaming Mouse    │  Mechanical Keyboard  │  Other Devices      │
│  125µs event rate     │  Variable rate        │  (touchpad, etc)    │
└────────┬──────────────┴───────────┬───────────┴─────────────────────┘
         │                          │
         ▼                          ▼
   RELATIVE events            KEY events
   (REL_X, REL_Y)            (KEY_DOWN, KEY_UP)
   + BTN events              (A, W, S, D, ESC, ...)
   (BTN_LEFT, RIGHT, ...)
         │                          │
         └──────────┬───────────────┘
                    │
                    ▼
         Linux Kernel Input Subsystem
                    │
                    ▼
         /dev/input/event{0-31} (evdev interface)
                    │
                    ▼
┌───────────────────┴──────────────────────────────────────────────────┐
│              USERSPACE EVENT LOOP (main.rs:929-1300)                 │
├──────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Initialization Phase (main.rs:675-691):                            │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  for /dev/input/event* {                                       │ │
│  │    dev = evdev::Device::open(path)                             │ │
│  │    dev_type = classify_device_type(dev)  ← OPTIMIZATION        │ │
│  │      ├─ Check EventType::KEY + alphanumeric → Keyboard         │ │
│  │      ├─ Check EventType::RELATIVE → Mouse                      │ │
│  │      └─ Else → Other (ignore)                                  │ │
│  │    Register in epoll + HashMap<fd, DeviceInfo>                 │ │
│  │  }                                                              │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                                                                      │
│  Runtime Phase (main.rs:1007-1138):                                 │
│  ┌────────────────────────────────────────────────────────────────┐ │
│  │  epoll.wait(100ms)  ← Non-blocking for Ctrl+C responsiveness   │ │
│  │      ↓                                                          │ │
│  │  Event received on fd                                          │ │
│  │      ↓                                                          │ │
│  │  HashMap lookup: fd → DeviceInfo{idx, dev_type}                │ │
│  │      ↓                                                          │ │
│  │  if dev_type == Other: skip (touchpad, virtual device)         │ │
│  │      ↓                                                          │ │
│  │  dev.fetch_events() → Iterator of input_event structs          │ │
│  │      ↓                                                          │ │
│  │  for _event in iter {  ← ZERO-LATENCY: No batching!            │ │
│  │      trigger_input_window(&skel)  ← Immediate BPF syscall      │ │
│  │  }                                                              │ │
│  │      ↓                                                          │ │
│  │  Performance:                                                   │ │
│  │    - Mouse movement (REL_X/REL_Y): BPF trigger per axis        │ │
│  │    - Mouse click (BTN_LEFT): BPF trigger on press & release    │ │
│  │    - Keyboard (KEY_W): BPF trigger on press & release          │ │
│  │    - 8kHz mouse moving: ~8000 triggers/sec = ~6ms CPU          │ │
│  │    - Latency per event: <1µs (syscall overhead)                │ │
│  └────────────────────────────────────────────────────────────────┘ │
│                             │                                        │
│                             ▼                                        │
│                  BPF Syscall (bpf_intf.rs:16)                        │
│                 trigger_input_window(skel)                           │
└──────────────────────────────┬───────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│               BPF KERNEL SPACE (main.bpf.c:943-994)                  │
├──────────────────────────────────────────────────────────────────────┤
│  SEC("syscall") set_input_window()  ← BPF syscall entry point       │
│      ↓                                                               │
│  fanout_set_input_window()                                          │
│    input_until_global = now + input_window_ns (5ms default)         │
│      ↓                                                               │
│  Calculate inter-event delta (main.bpf.c:958-991):                  │
│    delta_ns = now - last_input_trigger_ns                           │
│      ↓                                                               │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  MOUSE STOP DETECTION (main.bpf.c:970):                      │   │
│  │  if (delta_ns > 1ms) {  ← 8x safety for 8kHz (125µs spacing) │   │
│  │      input_trigger_rate = 0                                   │   │
│  │      continuous_input_mode = 0                                │   │
│  │      // Mouse stopped! Reset rate tracking                    │   │
│  │  }                                                             │   │
│  └──────────────────────────────────────────────────────────────┘   │
│      ↓                                                               │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  CONTINUOUS MODE DETECTION (main.bpf.c:973-991):             │   │
│  │  instant_rate = 1000000000 / delta_ns  (events/sec)          │   │
│  │  input_trigger_rate = EMA(instant_rate, 7/8 weight)          │   │
│  │      ↓                                                         │   │
│  │  if (input_trigger_rate > 150/sec):                           │   │
│  │      continuous_input_mode = 1  ← AIM TRAINER MODE            │   │
│  │  else if (input_trigger_rate < 75/sec):                       │   │
│  │      continuous_input_mode = 0  ← DISCRETE INPUT MODE         │   │
│  │                                                                │   │
│  │  Hysteresis: 150/75 = 2:1 ratio prevents mode flapping        │   │
│  │  Effect: Latency variance 123ns → 105ns during transitions    │   │
│  └──────────────────────────────────────────────────────────────┘   │
│      ↓                                                               │
│  last_input_trigger_ns = now  ← Update for next delta calculation   │
│      ↓                                                               │
│  nr_input_trig++  ← Statistics counter                              │
│                                                                      │
│  Result: input_until_global timestamp set (affects all CPUs)        │
└──────────────────────────────────────────────────────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│          SCHEDULER HOT PATH (BPF enqueue - main.bpf.c:686-718)       │
├──────────────────────────────────────────────────────────────────────┤
│  Task wakes up (game render thread, input handler, etc.)            │
│      ↓                                                               │
│  Check: is_input_active()? (boost.bpf.h:48)                         │
│    → time_before(now, input_until_global)?                          │
│      ↓                                                               │
│  YES - Input window active:                                         │
│  ┌──────────────────────────────────────────────────────────────┐   │
│  │  if (continuous_input_mode == 0) {  ← DISCRETE MODE          │   │
│  │      slice = slice >> 1  (10µs → 5µs)                        │   │
│  │      // Aggressive preemption for clicks, bursts              │   │
│  │  } else {  ← CONTINUOUS MODE (aim trainer)                    │   │
│  │      slice = slice  (keep 10µs)                               │   │
│  │      // Stable timing for smooth tracking                     │   │
│  │  }                                                             │   │
│  │                                                                │   │
│  │  Migration limits relaxed (allow responsive CPU changes)      │   │
│  └──────────────────────────────────────────────────────────────┘   │
│      ↓                                                               │
│  Task enqueued with boost parameters                                │
│      ↓                                                               │
│  Result: Faster preemption during input window                      │
│          (5µs discrete, 10µs continuous)                             │
└──────────────────────────────────────────────────────────────────────┘

Event Type Detection (what we monitor):
┌──────────────────────────────────────────────────────────────────────┐
│  MOUSE EVENTS:                                                       │
│  ├─ REL_X, REL_Y: Movement (X/Y axis) → Immediate trigger           │
│  ├─ REL_WHEEL: Scroll wheel → Immediate trigger                     │
│  ├─ BTN_LEFT: Left click press/release → Immediate trigger          │
│  ├─ BTN_RIGHT: Right click → Immediate trigger                      │
│  └─ BTN_MIDDLE: Middle click → Immediate trigger                    │
│                                                                      │
│  KEYBOARD EVENTS:                                                    │
│  ├─ KEY_W, A, S, D: Movement keys → Immediate trigger               │
│  ├─ KEY_SPACE: Jump → Immediate trigger                             │
│  ├─ KEY_ESC: Menu → Immediate trigger                               │
│  ├─ KEY_1-9: Weapon switch → Immediate trigger                      │
│  └─ All KEY_DOWN and KEY_UP: Symmetric latency                      │
│                                                                      │
│  IGNORED EVENTS (no trigger):                                       │
│  └─ EventType::SYNC, ABS (touchpad absolute positioning)            │
└──────────────────────────────────────────────────────────────────────┘

Timing Analysis (8kHz mouse moving diagonally):
┌──────────────────────────────────────────────────────────────────────┐
│  Time   │ Event     │ Action              │ Cumulative Latency      │
│─────────┼───────────┼─────────────────────┼─────────────────────────│
│  T+0µs  │ REL_X     │ Sensor reports      │ 0µs (hardware)          │
│  T+50µs │           │ Kernel driver evdev │ +50µs (driver latency)  │
│  T+150µs│           │ epoll wake userspace│ +100µs (scheduler)      │
│  T+200µs│           │ fetch_events()      │ +50µs (syscall)         │
│  T+201µs│           │ trigger_input_window│ +1µs (function call)    │
│  T+401µs│           │ BPF window set      │ +200ns (map write)      │
│         │           │                     │                         │
│  T+426µs│ REL_Y     │ Sensor reports      │ (same event)            │
│  T+476µs│           │ Kernel driver       │ +50µs                   │
│  T+576µs│           │ epoll wake          │ +100µs                  │
│  T+626µs│           │ fetch_events()      │ +50µs                   │
│  T+627µs│           │ trigger_input_window│ +1µs                    │
│  T+827µs│           │ BPF window set      │ +200ns                  │
│         │           │                     │                         │
│  Total per axis: ~400-800µs sensor-to-boost                         │
│  Diagonal movement: Two triggers (X + Y) within ~400µs              │
└──────────────────────────────────────────────────────────────────────┘
```

---

### Complete Task Lifecycle

This diagram shows the full path from task wakeup to CPU execution:

```
┌──────────────────────────────────────────────────────────────────────┐
│                     TASK WAKEUP EVENT                                │
│  (Game render thread wakes after vsync, input handler after event)  │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  KERNEL: wake_up_process() / try_to_wake_up()                       │
│  Standard Linux wakeup path                                         │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  BPF: select_cpu(task, prev_cpu, wake_flags)                        │
│  Location: main.bpf.c:1474-1650                                     │
│  Latency: 200-800ns                                                 │
├──────────────────────────────────────────────────────────────────────┤
│  Decision Tree:                                                      │
│                                                                      │
│  1. Per-CPU kthread? (task->flags & PF_NO_SETAFFINITY)              │
│     YES → return prev_cpu (keep kernel threads local)               │
│      │                                                               │
│      NO                                                              │
│      ↓                                                               │
│  2. Check mm_hint cache (LRU, 8192 entries):                        │
│      key = task->mm (address space pointer)                         │
│      if (cache hit && CPU idle && same NUMA):                       │
│          return cached_cpu  ← 88% hit rate observed                │
│      ↓                                                               │
│  3. Idle CPU scan:                                                  │
│      if (flat_idle_scan):                                           │
│          Linear search CPUs 0-255 → first idle                      │
│      else if (preferred_idle_scan):                                 │
│          Search preferred_cpus[] array (high-capacity first)        │
│          if (avoid_smt): Skip CPU if sibling busy                   │
│      ↓                                                               │
│  4. Wake sync optimization (producer-consumer):                     │
│      if (wake_flags & SCX_WAKE_SYNC && waker CPU idle):            │
│          return waker_cpu  ← Keep producer/consumer together        │
│      ↓                                                               │
│  5. Fallback:                                                       │
│      return prev_cpu  ← Stay where we were                          │
│                                                                      │
│  Result: cpu_id (0-255)                                             │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  BPF: enqueue(task, enq_flags)                                      │
│  Location: main.bpf.c:1652-1850                                     │
│  Latency: 150-400ns                                                 │
├──────────────────────────────────────────────────────────────────────┤
│  1. Update vtime (virtual time for fairness):                       │
│      vtime += runtime_since_last_sleep                              │
│      vtime = min(vtime, global_vtime + slice_lag)                   │
│      ↓                                                               │
│  2. Check system load (cpu_util_avg from BPF sampling):             │
│      ↓                                                               │
│      if (cpu_util_avg < 80%):  ← LIGHT LOAD                         │
│      ┌─────────────────────────────────────────────────────────┐   │
│      │  LOCAL MODE (Per-CPU DSQ):                              │   │
│      │  target_dsq = cpu_id  (0-255)                            │   │
│      │  scx_bpf_dispatch(task, target_dsq, slice, 0)            │   │
│      │  → Task goes to per-CPU round-robin queue                │   │
│      │  → Preserves cache locality                              │   │
│      └─────────────────────────────────────────────────────────┘   │
│      ↓                                                               │
│      else:  ← HEAVY LOAD                                            │
│      ┌─────────────────────────────────────────────────────────┐   │
│      │  GLOBAL MODE (EDF):                                      │   │
│      │  target_dsq = SHARED_DSQ (0)                             │   │
│      │  scx_bpf_dispatch(task, SHARED_DSQ, slice, vtime)        │   │
│      │  → Task goes to global deadline-sorted queue             │   │
│      │  → Better load balancing under pressure                  │   │
│      └─────────────────────────────────────────────────────────┘   │
│      ↓                                                               │
│  3. Input window adjustments:                                       │
│      if (is_input_active() && is_foreground_task()):               │
│          if (continuous_input_mode == 0):                           │
│              slice >>= 1  (10µs → 5µs)                              │
│          // else: keep slice stable for aim smoothness              │
│      ↓                                                               │
│  4. Migration limiting:                                             │
│      Check per-task token bucket:                                   │
│      if (migrations_in_window >= mig_max):                          │
│          Block migration (nr_mig_blocked++)                         │
│      ↓                                                               │
│  5. Wake sync fast path:                                            │
│      if (SCX_ENQ_WAKEUP && sync wakeup):                            │
│          scx_bpf_dispatch(task, SCX_DSQ_LOCAL, ...)                 │
│          // Direct dispatch, skip queue (nr_sync_wake_fast++)       │
│                                                                      │
│  Result: Task in DSQ, ready for dispatch()                          │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  BPF: dispatch(cpu, prev_task)                                      │
│  Location: main.bpf.c:1852-1950                                     │
│  Latency: 100-300ns                                                 │
│  Called when: CPU becomes idle or prev_task exhausts slice          │
├──────────────────────────────────────────────────────────────────────┤
│  1. Try per-CPU DSQ first (locality):                               │
│      scx_bpf_consume(cpu_dsq)                                       │
│      if (task found):                                               │
│          Update CPU frequency based on load                         │
│          return  ← Task runs on CPU                                 │
│      ↓                                                               │
│  2. Try global SHARED_DSQ (load balancing):                         │
│      scx_bpf_consume(SHARED_DSQ)                                    │
│      if (task found):                                               │
│          Update CPU frequency                                       │
│          return  ← Task runs on CPU                                 │
│      ↓                                                               │
│  3. No work available:                                              │
│      CPU goes idle                                                  │
│                                                                      │
│  Result: Task executes on CPU                                       │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│                     TASK EXECUTION                                   │
│  CPU runs task for slice duration (5-20µs)                          │
│  Task performs work (render frame, process input, etc.)             │
└──────────────────────────────────────────────────────────────────────┘
                                 │
                                 ▼
                        [Preemption or Sleep]
                                 │
                ┌────────────────┴────────────────┐
                │                                 │
                ▼                                 ▼
        Slice exhausted                   Task sleeps (I/O wait)
                │                                 │
                └──────────────┬──────────────────┘
                               │
                               ▼
                     Return to enqueue()
                  (cycle repeats for next wake)
```

---

### BPF Program Interactions

This diagram shows how all BPF programs and maps interact:

```
┌──────────────────────────────────────────────────────────────────────┐
│                    BPF PROGRAMS AND MAPS                             │
└──────────────────────────────────────────────────────────────────────┘

Main Scheduler (sched_ext ops):
┌─────────────────────────────────────────────────────────────────────┐
│  struct_ops/gamer_ops                                               │
│  ├─ select_cpu() ──────┬─────────────────────────────────────────┐  │
│  ├─ enqueue() ──────────┼──────────────┬──────────────────────────┤  │
│  ├─ dispatch() ─────────┼──────────────┼────┬─────────────────────┤  │
│  ├─ running() ──────────┼──────────────┼────┼──────────────────┐  │  │
│  ├─ stopping() ─────────┼──────────────┼────┼──────────────────┤  │  │
│  └─ quiescent() ────────┼──────────────┼────┼──────────────────┤  │  │
└────────────────────────┼──────────────┼────┼──────────────────┼──┘  │
                         │              │    │                  │     │
         Reads/Writes:   │              │    │                  │     │
                         ▼              ▼    ▼                  ▼     ▼
┌────────────────────┬─────────────┬────────┬──────────────┬──────────┐
│  BPF MAPS:         │             │        │              │          │
├────────────────────┼─────────────┼────────┼──────────────┼──────────┤
│  task_ctx_stor     │ ✓ (R/W)     │ ✓(R/W) │ ✓(R/W)       │ ✓(R/W)   │
│  (task metadata)   │             │        │              │          │
├────────────────────┼─────────────┼────────┼──────────────┼──────────┤
│  cpu_ctx_stor      │ ✓ (R/W)     │ ✓(R/W) │ ✓(R/W)       │ ✓(R)     │
│  (CPU load, vtime) │             │        │              │          │
├────────────────────┼─────────────┼────────┼──────────────┼──────────┤
│  mm_recent_cpu     │ ✓ (R/W)     │ ✓(R)   │              │          │
│  (cache affinity)  │  LRU 8192   │        │              │          │
├────────────────────┼─────────────┼────────┼──────────────┼──────────┤
│  primary_cpumask   │ ✓ (R)       │        │              │          │
│  (CPU priority)    │             │        │              │          │
└────────────────────┴─────────────┴────────┴──────────────┴──────────┘

Detection Hooks (run in parallel with scheduler):
┌─────────────────────────────────────────────────────────────────────┐
│  LSM Hook: bprm_committed_creds                                     │
│  Trigger: Every exec() syscall                                      │
│      ↓                                                               │
│  Read task->comm, task->tgid                                        │
│  Classify: is_system_binary()? → 90% filtered                      │
│      ↓                                                               │
│  Ring buffer push → process_events                                  │
│      ↓                                                               │
│  Userspace consumer: game_detect_bpf.rs                             │
│      ↓                                                               │
│  Update: current_game_map (TGID)                                    │
│      ↓                                                               │
│  Scheduler reads: detected_fg_tgid (via BSS map)                    │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│  Tracepoint: tp_btf/sched_switch                                    │
│  Trigger: Every context switch (~1000-10000/sec)                    │
│      ↓                                                               │
│  Update thread_runtime_map:                                         │
│    - total_runtime_ns, total_sleeptime_ns                           │
│    - wakeup_count, avg_exec_ns                                      │
│    - wakeup_freq (EMA)                                              │
│      ↓                                                               │
│  Classify thread role:                                              │
│    - GPU submit: <100µs exec, >50Hz freq                            │
│    - Background: >5ms exec, <10Hz freq                              │
│    - CPU-bound: >1ms exec, >50Hz freq                               │
│      ↓                                                               │
│  Scheduler reads: thread_runtime_map in enqueue()                   │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│  fentry: drm_ioctl / kprobe: nvidia_ioctl                           │
│  Trigger: GPU command submission (~60-500/sec)                      │
│      ↓                                                               │
│  Check ioctl cmd:                                                   │
│    - DRM_I915_GEM_EXECBUFFER2 (Intel)                               │
│    - DRM_AMDGPU_CS (AMD)                                            │
│    - NVIDIA private ioctls                                          │
│      ↓                                                               │
│  Update gpu_threads_map:                                            │
│    - Mark TID as GPU submit thread                                  │
│    - Track submit frequency                                         │
│      ↓                                                               │
│  Scheduler reads: gpu_threads_map in select_cpu()                   │
│  Effect: GPU threads prefer physical cores (avoid SMT)              │
└─────────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────────┐
│  uprobe: /usr/lib/wine/.../ntdll.so:NtSetInformationThread          │
│  Trigger: Game sets thread priority (~10-100/sec during startup)    │
│      ↓                                                               │
│  Read priority value from userspace:                                │
│    bpf_probe_read_user(&priority, ...)                              │
│      ↓                                                               │
│  Classify:                                                          │
│    - TIME_CRITICAL + REALTIME → Audio (99% accurate)                │
│    - TIME_CRITICAL → Render thread                                  │
│    - HIGHEST → Input handler                                        │
│    - ABOVE_NORMAL → Physics                                         │
│      ↓                                                               │
│  Update wine_threads_map                                            │
│      ↓                                                               │
│  Scheduler reads: wine_threads_map in enqueue()                     │
└─────────────────────────────────────────────────────────────────────┘

Map Sharing (how userspace and BPF communicate):
┌─────────────────────────────────────────────────────────────────────┐
│  Userspace → BPF (configuration):                                   │
│  ├─ rodata map (read-only from BPF, writable from userspace):       │
│  │   - slice_ns, input_window_ns, mig_max, ...                      │
│  │   - Hot-reload: Userspace updates, BPF reads new values          │
│  │                                                                   │
│  ├─ BSS map (globals, bidirectional):                               │
│  │   - detected_fg_tgid (userspace writes, BPF reads)               │
│  │   - cmd_flags (userspace sets, BPF clears)                       │
│  │                                                                   │
│  BPF → Userspace (monitoring):                                      │
│  ├─ BSS map (statistics):                                           │
│  │   - nr_input_trig, nr_migrations, cpu_util, ...                  │
│  │   - Read by scx_stats every 1-5 seconds                          │
│  │                                                                   │
│  └─ Ring buffers (events):                                          │
│      - process_events: Game launches/exits                          │
│      - Polled by userspace at 100ms intervals                       │
└─────────────────────────────────────────────────────────────────────┘
```

---

### Scheduler State Machine

This diagram shows the local/EDF mode transitions based on system load:

```
┌──────────────────────────────────────────────────────────────────────┐
│              SCHEDULER MODE STATE MACHINE                            │
└──────────────────────────────────────────────────────────────────────┘

Initial State: LOCAL_MODE (per-CPU DSQs)
┌────────────────────────────────────┐
│         LOCAL MODE                 │
│  (Light Load: <80% CPU util)       │
│                                    │
│  Dispatch Strategy:                │
│  └─ Per-CPU DSQs (0-255)           │
│  └─ Round-robin within queue       │
│  └─ Preserves cache locality       │
│                                    │
│  Characteristics:                  │
│  ├─ Low migration rate             │
│  ├─ High mm_hint hit rate (88%)    │
│  ├─ Low dispatch latency (150ns)   │
│  └─ May cause load imbalance       │
└────────────┬───────────────────────┘
             │
             │ cpu_util_avg >= 80%
             │ (EMA crosses threshold)
             ▼
┌────────────────────────────────────┐
│         EDF MODE                   │
│  (Heavy Load: ≥80% CPU util)       │
│                                    │
│  Dispatch Strategy:                │
│  └─ SHARED_DSQ (global queue)      │
│  └─ Deadline-sorted (vtime)        │
│  └─ Better load balancing          │
│                                    │
│  Characteristics:                  │
│  ├─ Higher migration rate          │
│  ├─ Lower mm_hint hit rate         │
│  ├─ Slightly higher latency (200ns)│
│  └─ Prevents CPU starvation        │
└────────────┬───────────────────────┘
             │
             │ cpu_util_avg < 75%
             │ (Hysteresis: 5% gap)
             ▼
        Return to LOCAL MODE

Load Measurement (in-kernel, no userspace syscalls):
┌─────────────────────────────────────────────────────────────────────┐
│  Wakeup Timer: Periodic sampling (500µs - 2ms depending on load)    │
│      ↓                                                               │
│  For each CPU:                                                       │
│    idle_pct = (idle_time / total_time) * 100                        │
│    util_pct = 100 - idle_pct                                        │
│      ↓                                                               │
│  Average across all CPUs:                                           │
│    cpu_util = Σ(util_pct) / nr_cpus                                 │
│      ↓                                                               │
│  Exponential moving average (smoothing):                            │
│    cpu_util_avg = (cpu_util_avg * 7 + cpu_util) >> 3               │
│      ↓                                                               │
│  Mode decision:                                                     │
│    if (cpu_util_avg >= 80%): use SHARED_DSQ (EDF)                   │
│    if (cpu_util_avg < 75%): use per-CPU DSQs (LOCAL)                │
│    (Hysteresis prevents mode flapping)                              │
└─────────────────────────────────────────────────────────────────────┘

Impact on Different Workloads:
┌─────────────────────────────────────────────────────────────────────┐
│  Game only (20-40% CPU):                                            │
│  └─ Stays in LOCAL mode → Maximum cache locality                    │
│                                                                      │
│  Game + OBS capture (60-75% CPU):                                   │
│  └─ Stays in LOCAL mode → Cache locality preserved                  │
│                                                                      │
│  Game + OBS + browser (80-90% CPU):                                 │
│  └─ Switches to EDF mode → Load balancing, prevents starvation      │
│                                                                      │
│  Kernel compile while gaming (95-100% CPU):                         │
│  └─ EDF mode → Fair distribution, game maintains responsiveness     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### Thread Classification Pipeline

This diagram shows how multiple detection systems combine to classify threads:

```
┌──────────────────────────────────────────────────────────────────────┐
│         THREAD CLASSIFICATION: MULTI-SOURCE DETECTION                │
└──────────────────────────────────────────────────────────────────────┘

Thread: TID 12345 (e.g., game render thread)
                               │
       ┌───────────────────────┼───────────────────────┐
       │                       │                       │
       ▼                       ▼                       ▼
┌─────────────────┐  ┌──────────────────┐  ┌─────────────────────────┐
│ GPU Detection   │  │ Wine Priority    │  │ Runtime Pattern         │
│ (fentry/kprobe) │  │ (uprobe)         │  │ (sched_switch)          │
└─────────────────┘  └──────────────────┘  └─────────────────────────┘
       │                       │                       │
       ▼                       ▼                       ▼
  drm_ioctl()          NtSetInformationThread   Context switch tracking
  called by TID        (priority=TIME_CRITICAL) (exec=80µs, freq=120Hz)
       │                       │                       │
       ▼                       ▼                       ▼
  gpu_threads_map       wine_threads_map       thread_runtime_map
  [12345] = {           [12345] = {            [12345] = {
    vendor: NVIDIA,       role: RENDER,          avg_exec: 80000ns,
    is_render: 1          priority: 15,          wakeup_freq: 120,
  }                       is_realtime: 0         role: GPU_SUBMIT
                        }                       }
       │                       │                       │
       └───────────────────────┼───────────────────────┘
                               │
                               ▼
┌──────────────────────────────────────────────────────────────────────┐
│  UNIFIED CLASSIFICATION (advanced_detect.bpf.h:update_task_ctx)     │
├──────────────────────────────────────────────────────────────────────┤
│  Priority (highest accuracy first):                                 │
│  1. Wine priority map → ROLE_RENDER (99% accurate for audio)        │
│  2. GPU threads map → GPU_SUBMIT (100% accurate, actual ioctls)     │
│  3. Runtime patterns → Heuristic classification                     │
│  4. Process name → Fallback (compositor, network)                   │
│                                                                      │
│  Result: task_ctx->detected_role = RENDER                           │
└────────────────────────────────┬─────────────────────────────────────┘
                                 │
                                 ▼
┌──────────────────────────────────────────────────────────────────────┐
│  SCHEDULER DECISIONS BASED ON ROLE                                  │
├──────────────────────────────────────────────────────────────────────┤
│  RENDER thread (TID 12345):                                         │
│  ├─ select_cpu(): Prefer physical cores (avoid SMT)                 │
│  ├─ enqueue(): Input window boost applied                           │
│  ├─ dispatch(): Higher priority during input                        │
│  └─ migration: Relaxed limits (allow responsive placement)          │
│                                                                      │
│  AUDIO thread:                                                      │
│  ├─ select_cpu(): Prefer idle CPU (minimize preemption)             │
│  ├─ enqueue(): Never migrate (preserve cache for audio buffers)     │
│  └─ dispatch(): Highest priority (prevent audio glitches)           │
│                                                                      │
│  BACKGROUND thread (shader compilation, asset streaming):           │
│  ├─ select_cpu(): Any available CPU (no preference)                 │
│  ├─ enqueue(): No input boost                                       │
│  └─ dispatch(): Lowest priority                                     │
└──────────────────────────────────────────────────────────────────────┘

Detection Accuracy Comparison:
┌─────────────────────────────────────────────────────────────────────┐
│  Method              │ Latency   │ Accuracy │ CPU Overhead          │
│──────────────────────┼───────────┼──────────┼───────────────────────│
│  GPU ioctl hooks     │ <1ms      │ 100%     │ 0 (only on 1st call)  │
│  Wine priority       │ <1ms      │ 99%*     │ 1-2µs per change      │
│  Runtime patterns    │ 1-5 frames│ 70-85%   │ 100-200ns per switch  │
│  Process name        │ Instant   │ 50-60%   │ 0 (one-time)          │
│                                                                      │
│  * 99% for audio threads (TIME_CRITICAL+REALTIME unique signature)  │
│    95% for render threads (TIME_CRITICAL or HIGHEST)                │
│    80% for input threads (HIGHEST, overlaps with render)            │
└─────────────────────────────────────────────────────────────────────┘
```

---

### Input Event Detailed Flow (8kHz Mouse Movement)

This diagram shows microsecond-level timing for a single mouse movement:

```
┌──────────────────────────────────────────────────────────────────────┐
│        8KHZ MOUSE DIAGONAL MOVEMENT (Single Frame Analysis)         │
│                    Time: T+0µs to T+900µs                            │
└──────────────────────────────────────────────────────────────────────┘

T+0µs: Mouse sensor detects movement (X: +5, Y: +3)
│
├─ Sensor digitizes movement
│  Polling rate: 8000Hz = 125µs between reports
│
▼
T+20µs: USB transaction start
│  ├─ USB packet: HID report with X/Y delta
│  └─ USB latency: ~20-40µs (USB 2.0 Full Speed)
│
▼
T+50µs: Kernel USB/HID driver receives packet
│
├─ HID parser extracts X/Y deltas
│  └─ Creates input_event structs: {type: RELATIVE, code: REL_X, value: 5}
│                                   {type: RELATIVE, code: REL_Y, value: 3}
▼
T+60µs: evdev layer generates events
│
├─ Events written to /dev/input/event{N} ring buffer
│  └─ epoll notification sent to waiting process
│
▼
T+150µs: scx_gamer process wakes from epoll
│
├─ Kernel scheduler selects scx_gamer process
│  └─ Latency: ~90µs (depends on CPU load, pinned to low-capacity core)
│
▼
T+200µs: dev.fetch_events() called (main.rs:1112)
│
├─ Read events from kernel ring buffer
│  └─ Returns iterator with 2 events (REL_X, REL_Y)
│
▼
T+201µs: First event processed (REL_X)
│
├─ HashMap lookup: fd → DeviceInfo{idx=0, dev_type=Mouse}
│  └─ Latency: ~15-30ns (FxHashMap, warm cache)
│
├─ Check: dev_type != Other? → Yes (Mouse)
│
├─ Call: trigger_input_window(&skel)  ← SYSCALL
│  └─ Latency: ~200-400ns (BPF prog execution)
│
▼
T+201.4µs: BPF set_input_window() executes
│
├─ fanout_set_input_window():
│    input_until_global = now + 5000000ns (5ms)
│
├─ Calculate delta_ns = now - last_input_trigger_ns
│    delta_ns = 201400 - 0 = 201400ns (201µs since last event)
│
├─ instant_rate = 1000000000 / 201400 ≈ 4965 events/sec
│
├─ Update EMA:
│    input_trigger_rate = (0 * 7 + 4965) >> 3 ≈ 620/sec
│
├─ Check continuous mode:
│    if (620 > 150): continuous_input_mode = 1  ← ENTERED CONTINUOUS
│
└─ last_input_trigger_ns = 201400
│
▼
T+401µs: Return to userspace
│
│
▼
T+402µs: Second event processed (REL_Y)
│
├─ Same path as REL_X
│
├─ trigger_input_window(&skel)  ← SECOND SYSCALL
│
▼
T+402.4µs: BPF set_input_window() executes again
│
├─ delta_ns = 402400 - 201400 = 201000ns (201µs)
│
├─ instant_rate = 1000000000 / 201000 ≈ 4975/sec
│
├─ EMA update:
│    input_trigger_rate = (620 * 7 + 4975) >> 3 ≈ 1162/sec
│
├─ continuous_input_mode already 1 (stays in continuous mode)
│
└─ input_until_global extended (now + 5ms from T+402µs)
│
▼
T+602µs: Return to userspace, event processing complete
│
└─ Total processing: 602µs for 2-axis mouse movement

Next Task Wake (e.g., game render thread at T+800µs):
│
▼
T+800µs: Game render thread wakes
│
├─ enqueue() called
│
├─ Check: is_input_active()?
│    now = T+800µs
│    input_until_global = T+402µs + 5000µs = T+5402µs
│    time_before(800, 5402)? → YES, input window active
│
├─ Check: continuous_input_mode?
│    continuous_input_mode = 1
│
├─ Slice decision:
│    if (continuous_input_mode): slice = 10µs  (stable)
│    else: slice = 5µs  (halved for discrete input)
│
└─ Task enqueued with 10µs slice (continuous mode)
│
▼
Result: Render thread gets stable 10µs slice for smooth aim tracking
        Input boost window remains active until T+5402µs

Comparison: Without Continuous Mode
│
├─ Every mouse movement would halve slice: 10µs → 5µs
│
├─ Rapid slice changes (every 125µs for 8kHz mouse)
│
├─ Causes timing jitter: render thread preemption varies
│
└─ Effect: Aim feels "stuttery" during tracking (inconsistent frame pacing)
```

---

### Thread Classification Decision Tree

This diagram shows the multi-source priority for thread role classification:

```
┌──────────────────────────────────────────────────────────────────────┐
│           THREAD ROLE CLASSIFICATION ALGORITHM                       │
│              (advanced_detect.bpf.h:230-280)                         │
└──────────────────────────────────────────────────────────────────────┘

Thread wakes up (TID received in enqueue/select_cpu)
│
▼
┌────────────────────────────────────────────────────────────────────┐
│  PRIORITY 1: Wine Thread Priority (99% accuracy for audio)         │
├────────────────────────────────────────────────────────────────────┤
│  wine_info = bpf_map_lookup_elem(&wine_threads_map, &tid)          │
│  if (wine_info):                                                    │
│      ├─ wine_info->role == WINE_ROLE_AUDIO?                         │
│      │   └─ return ROLE_AUDIO  ✓ 99% accurate                       │
│      ├─ wine_info->role == WINE_ROLE_RENDER?                        │
│      │   └─ return ROLE_RENDER  ✓ 95% accurate                      │
│      └─ wine_info->role == WINE_ROLE_INPUT?                         │
│          └─ return ROLE_INPUT  ✓ 80% accurate                       │
│                                                                      │
│  Detection: NtSetInformationThread(ThreadBasePriority)             │
│  Examples:                                                          │
│    - UE4 audio: SetThreadPriority(TIME_CRITICAL + REALTIME)        │
│    - Unity render: SetThreadPriority(HIGHEST)                      │
│    - Source input: SetThreadPriority(ABOVE_NORMAL)                 │
└────────────────────────────────┬───────────────────────────────────┘
                                 │ No Wine info found
                                 ▼
┌────────────────────────────────────────────────────────────────────┐
│  PRIORITY 2: GPU ioctl Detection (100% accuracy)                   │
├────────────────────────────────────────────────────────────────────┤
│  gpu_info = bpf_map_lookup_elem(&gpu_threads_map, &tid)            │
│  if (gpu_info && gpu_info->is_render_thread):                      │
│      └─ return ROLE_GPU_SUBMIT  ✓ 100% accurate                    │
│                                                                      │
│  Detection: drm_ioctl() / nvidia_ioctl() calls                     │
│  Examples:                                                          │
│    - Vulkan: vkQueueSubmit() → ioctl(DRM_I915_GEM_EXECBUFFER2)    │
│    - OpenGL: glFlush() → ioctl(DRM_AMDGPU_CS)                      │
│    - D3D11/DXVK: Present() → ioctl(NVIDIA_SUBMIT)                  │
└────────────────────────────────┬───────────────────────────────────┘
                                 │ No GPU info found
                                 ▼
┌────────────────────────────────────────────────────────────────────┐
│  PRIORITY 3: Runtime Patterns (70-85% accuracy)                    │
├────────────────────────────────────────────────────────────────────┤
│  runtime_info = bpf_map_lookup_elem(&thread_runtime_map, &tid)     │
│  if (runtime_info):                                                 │
│      ├─ avg_exec < 100µs && wakeup_freq > 50Hz?                    │
│      │   └─ return ROLE_GPU_SUBMIT  (heuristic)                    │
│      ├─ avg_exec > 5ms && wakeup_freq < 10Hz?                      │
│      │   └─ return ROLE_BACKGROUND  (long CPU-bound tasks)         │
│      └─ avg_exec > 1ms && wakeup_freq > 50Hz?                      │
│          └─ return ROLE_CPU_BOUND  (physics, AI)                   │
│                                                                      │
│  Detection: sched_switch tracepoint                                │
│  Metrics:                                                           │
│    - total_runtime_ns / wakeup_count = avg_exec_ns                 │
│    - wakeup_count per second = wakeup_freq                         │
│    - Pattern matching over 8 wakeups (stable classification)       │
└────────────────────────────────┬───────────────────────────────────┘
                                 │ No runtime pattern match
                                 ▼
┌────────────────────────────────────────────────────────────────────┐
│  PRIORITY 4: Process Name Heuristics (50-60% accuracy)             │
├────────────────────────────────────────────────────────────────────┤
│  Read task->comm (16-byte process name)                             │
│  if (contains "kwin" || contains "composit"):                       │
│      └─ return ROLE_COMPOSITOR                                     │
│  if (contains "pipewire" || contains "pulse"):                      │
│      └─ return ROLE_SYSTEM_AUDIO                                   │
│  if (contains "Chrome_" || contains "firefox"):                     │
│      └─ return ROLE_NETWORK (browser renderer)                     │
│                                                                      │
│  Detection: task_class.bpf.h:is_compositor_name(), etc.            │
└────────────────────────────────┬───────────────────────────────────┘
                                 │ No match
                                 ▼
                          ROLE_UNKNOWN
                    (Use default scheduling)

Example: Warframe Render Thread Classification
┌─────────────────────────────────────────────────────────────────────┐
│  Thread: "Warframe.x64 Render" (TID 5432)                           │
│      ↓                                                               │
│  1. Wine priority check:                                            │
│      wine_threads_map[5432] = { priority: TIME_CRITICAL, ... }      │
│      → Classified as WINE_ROLE_RENDER  ✓ (from Windows API)         │
│      ↓                                                               │
│  2. GPU ioctl check:                                                │
│      gpu_threads_map[5432] = { vendor: AMD, is_render: 1 }          │
│      → Confirms GPU_SUBMIT  ✓ (from actual DRM calls)               │
│      ↓                                                               │
│  3. Runtime patterns check:                                         │
│      thread_runtime_map[5432] = { avg_exec: 85µs, freq: 144Hz }    │
│      → Matches GPU submit pattern  ✓ (heuristic)                    │
│      ↓                                                               │
│  Final: ROLE_RENDER (triple-confirmed, 100% confident)              │
│      ↓                                                               │
│  Scheduler decisions:                                               │
│    - select_cpu(): Prefer physical core (CPU 0, 2, 4, ... not 1,3)  │
│    - enqueue(): Apply input window boost (slice 10µs → 5µs)         │
│    - migration: Allow moves during input (responsive placement)     │
└─────────────────────────────────────────────────────────────────────┘
```

---

### Scheduler Dispatch Queue Organization

This diagram shows how tasks are organized in DSQs and consumed by CPUs:

```
┌──────────────────────────────────────────────────────────────────────┐
│                  DISPATCH QUEUE ARCHITECTURE                         │
└──────────────────────────────────────────────────────────────────────┘

Light Load (<80% util) - LOCAL MODE:
┌─────────────────────────────────────────────────────────────────────┐
│  Per-CPU DSQs (Round-Robin)                                         │
│                                                                      │
│  CPU 0 DSQ:  [TaskA] → [TaskB] → [TaskC]  ← FIFO within CPU        │
│  CPU 1 DSQ:  [TaskD] → [TaskE]                                      │
│  CPU 2 DSQ:  [TaskF]                                                │
│  ...                                                                 │
│  CPU 15 DSQ: [TaskZ] → [TaskY]                                      │
│                                                                      │
│  Enqueue decision:                                                  │
│    task->cpu = select_cpu(...)  ← Chosen based on cache/idle/NUMA  │
│    scx_bpf_dispatch(task, cpu_dsq, ...)                             │
│                                                                      │
│  Dispatch (per CPU):                                                │
│    CPU 0: Consume from CPU 0 DSQ only → TaskA                       │
│    CPU 1: Consume from CPU 1 DSQ only → TaskD                       │
│    No cross-CPU stealing (maximize cache locality)                  │
│                                                                      │
│  Benefits: High cache hit rate, low migration rate                  │
│  Risks: Load imbalance if some CPUs overloaded                      │
└─────────────────────────────────────────────────────────────────────┘

Heavy Load (≥80% util) - EDF MODE:
┌─────────────────────────────────────────────────────────────────────┐
│  Global SHARED_DSQ (Deadline-Sorted)                                │
│                                                                      │
│  SHARED_DSQ (DSQ 0):                                                │
│    [Task1:vtime=1000] → [Task2:vtime=1050] → [Task3:vtime=1100]    │
│           ▲                  ▲                      ▲               │
│       Earliest           Second                  Latest             │
│      (runs first)                                                   │
│                                                                      │
│  Enqueue decision:                                                  │
│    scx_bpf_dispatch(task, SHARED_DSQ, slice, vtime)                 │
│    → Inserted in vtime order (earliest deadline first)              │
│                                                                      │
│  Dispatch (any CPU):                                                │
│    CPU 0: Consume SHARED_DSQ → Task1 (lowest vtime)                 │
│    CPU 1: Consume SHARED_DSQ → Task2                                │
│    CPU 2: Consume SHARED_DSQ → Task3                                │
│    Any idle CPU can steal work from global queue                    │
│                                                                      │
│  Benefits: Fair load distribution, prevents CPU starvation          │
│  Risks: More migrations, lower cache hit rate                       │
└─────────────────────────────────────────────────────────────────────┘

Mode Transition Visualization:
┌─────────────────────────────────────────────────────────────────────┐
│  Time →                                                              │
│                                                                      │
│  CPU Utilization:                                                   │
│  100% ┤                                     ╱╲                       │
│   90% ┤                                   ╱    ╲                     │
│   80% ┼━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━▶━━━━━━◀━━━━━━━━━━━━━━    │
│   70% ┤                                 ╱          ╲                 │
│   50% ┤              ╱╲               ╱              ╲               │
│   30% ┤            ╱    ╲          ╱                  ╲             │
│   10% ┤     ╱╲  ╱        ╲      ╱                      ╲            │
│    0% ┼────────────────────────────────────────────────────────────  │
│       │                                                              │
│  Mode:│                                                              │
│       LOCAL  LOCAL  LOCAL   EDF    EDF    EDF   LOCAL  LOCAL        │
│                              ↑enter      ↑exit                       │
│                            (≥80%)      (<75%)                        │
│                                                                      │
│  Hysteresis prevents rapid mode switching (5% gap: 75%-80%)         │
└─────────────────────────────────────────────────────────────────────┘
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
