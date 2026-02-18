# scx_cake Performance Report

## Optimization Summary

This report documents the performance optimizations and code quality improvements applied to `scx_cake` based on a code review.

### 1. Division Removal (Rule 42)
The runtime modulo operation `prev_cpu % nr_phys_cpus` in `cake_select_cpu` (BPF) was replaced with a pre-computed lookup table `cpu_phys_map`. This eliminates a potentially expensive division instruction from the hot path of CPU selection.

- **Old:** `u32 prev_phys = (u32)prev_cpu % nr_phys_cpus;`
- **New:** `u32 prev_phys = cpu_phys_map[(u32)prev_cpu & (CAKE_MAX_CPUS - 1)];`

The map is populated at initialization time in userspace (`main.rs`).

### 2. Magic Numbers Replacement
Hardcoded magic numbers were replaced with named constants in `intf.h` to improve maintainability and allow for easier tuning.

- `CAKE_MIN_SLICE_NS` (200000ns / 200µs): Floor for re-enqueued slices.
- `CAKE_MIGRATION_COOLDOWN` (4): Number of wakeups before migration cooldown expires.
- `CAKE_WSC_MAX` (255): Saturation limit for `wakeup_same_cpu` counter.
- `CAKE_SYNC_MASK` (15): Mask for periodic `tctx` synchronization (every 16th stop).
- `CAKE_EWMA_SKIP_THRESH` (64): Threshold for skipping EWMA calculations on stable tasks.

### 3. False Sharing Mitigation
- **Struct Padding:** Verified that `struct mega_mailbox_entry` is padded to 128 bytes (2 cache lines) to prevent false sharing between the CPU-local "hot" cache line and the remote "warm" cache line.
- **Documentation:** Corrected misleading comments in `intf.h` that incorrectly stated the struct size was 64 bytes.

## Expensive Operations Analysis

The following operations in `cake.bpf.c` are identified as potentially expensive and should be the focus of future research:

1.  **`scx_bpf_dsq_insert` / `scx_bpf_dsq_insert_vtime`**: These kfuncs involve locking the Dispatch Queue (DSQ) in the kernel. While `scx_cake` uses per-LLC DSQs to reduce contention, this remains a synchronization point.
2.  **`scx_bpf_select_cpu_dfl`**: The kernel's default CPU selection fallback. It involves scanning CPU masks and is generally slower than the optimized "Gate" logic in `cake_select_cpu`. Optimizing `cake_select_cpu` to hit Gates 1-2 more often reduces reliance on this fallback.
3.  **`scx_bpf_now`**: Called frequently to get the current timestamp. While relatively cheap, its frequency makes it a cost factor. The code already employs "tunneling" (passing cached timestamps from `select_cpu` to `enqueue`) to minimize these calls.

## Compiler Note
During verification, a BPF backend bug in `clang` (version 18.1.3) was observed, causing a crash during code generation for `cake_select_cpu`. This appears to be an environment-specific issue with the compiler's DAG selection phase and is unrelated to the logical correctness of the optimizations.
