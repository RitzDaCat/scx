# Memory Layout & Cache Line Alignment (scx_gamer)

This document details the cache-line optimization strategy used in `scx_gamer` to minimize false sharing and maximize memory throughput.

## Core Principles

1.  **64-Byte Alignment**: Critical data structures are padded to 64 bytes (cache line size on x86/AMD64) to prevent false sharing between CPUs.
2.  **Structure Padding**: Structs are explicitly padded using `u8 _pad[N]` to ensure they don't straddle cache lines.
3.  **Static Assertions**: `_Static_assert` is used to enforce size guarantees at compile time.

## Optimized Structures

### 1. Per-CPU Context (`cpu_ctx`)
*   **Size**: 128 bytes (2 cache lines)
*   **Alignment**: `__attribute__((aligned(64)))`
*   **Layout**:
    *   **Line 1 (0-63)**: Ultra-hot fields (vtime, stats accumulators) accessed on every schedule event.
    *   **Line 2 (64-127)**: Warm fields (cpufreq, shared DSQ) accessed less frequently.
    *   **Padding**: Explicit `u8 _pad_end[16]` to ensure 128-byte total size.

### 2. Task Context (`task_ctx`)
*   **Size**: ~192 bytes (3 cache lines)
*   **Alignment**: `__attribute__((aligned(64)))`
*   **Layout**:
    *   **Line 1 (0-63)**: **CRITICAL**. Contains all bitfields (`is_input_handler`, `is_gpu_submit`) and runtime stats needed for `select_cpu()`.
    *   **Line 2 (64-127)**: Warm data (migration tokens, classification history).
    *   **Line 3 (128+)**: Cold/Analysis data (RMS metrics, deadling tracking).

### 3. Input & Signal Tracking (Global BSS)
To prevent false sharing on global variables, we use struct-wrapped globals with padding.

| Variable | New Struct | Padding Strategy |
| :--- | :--- | :--- |
| `hotpath_signals` | `struct hotpath_cpu_signal` | Each CPU signal padded to 64 bytes to allow lock-free per-CPU updates. |
| `input_until_global` | `struct global_input_tracker` | Grouped with trigger rate and key count, padded to 64B. |
| `napi_last_softirq_ns` | `struct napi_cpu_tracker` | Array of 64B-padded structs. Prevents NAPI on CPU 0 from invalidating CPU 1's cache. |
| `input_lane_until` | `struct input_lane_state` | Array of 64B-padded structs. Mouse updates (8kHz) don't thrash Keyboard state. |

### 4. Engine Profiles (`engine_profile_entry`)
*   **Old Size**: 24 bytes (packed ~2.6 entries per cache line).
*   **New Size**: 64 bytes (1 entry per cache line).
*   **Benefit**: Profiling updates for different threads (e.g., WorkerThread 1 vs WorkerThread 2) never contend for the same cache line.

## Verification

All optimized structures verify their size at compile time:

```c
_Static_assert(sizeof(struct cpu_ctx) % 64 == 0, "cpu_ctx alignment");
_Static_assert(sizeof(struct hot_path_cache) == 32, "hot_path_cache size");
_Static_assert(sizeof(struct engine_profile_entry) == 64, "engine_profile_entry size");
```

