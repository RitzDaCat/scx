# scx_cake Performance Audit

This document summarizes the findings of a code review and performance audit of the `scx_cake` scheduler.

## 1. Expensive Operations

The following operations were identified as potentially expensive and should be monitored or optimized if profiling indicates a bottleneck:

*   **KFuncs:**
    *   `scx_bpf_dsq_insert_vtime` / `scx_bpf_dsq_insert`: Core scheduling operations. Unavoidable but costly (~30-80ns depending on contention).
    *   `bpf_get_smp_processor_id()`: Used frequently. The code already optimizes this by deferring calls until necessary (e.g., after Gate 1).
    *   `scx_bpf_now()`: Timestamp retrieval. Cached in `cake_scratch` where possible to avoid repeated calls.
    *   `scx_bpf_test_and_clear_cpu_idle()`: Atomic operation on the idle mask. Used in `cake_select_cpu`.
    *   `bpf_cpumask_test_cpu()`: Used in `cake_init_task` loops. Can be slow if called excessively, but `cake_set_cpumask` optimizes this by caching the affinity mask.

*   **Loops:**
    *   `cake_select_cpu`: Contains unrolled loops (`#pragma unroll`) for scanning idle CPUs. This is O(1) due to the constant bound (4) but still involves multiple checks.
    *   `cake_dispatch`: Loops over `nr_llcs` to steal tasks. Bounded by `CAKE_MAX_LLCS` (8).
    *   `cake_init_task`: Loops over `CAKE_MAX_CPUS` to build the affinity mask. This is a cold path (task creation), so it is acceptable.

## 2. Optimizations & Anti-Patterns

*   **Division/Modulus:**
    *   **Finding:** No inefficient division (`/`) or modulus (`%`) operators were found in hot paths.
    *   **Optimization:** The code uses bitwise operations for powers of 2 (e.g., `& (CAKE_MAX_CPUS - 1)`).
    *   **Optimization:** `cake_dispatch` performs a "modulo" via subtraction: `if (victim >= nr_llcs) victim -= nr_llcs;`. This avoids the expensive `%` operator.

*   **False Sharing:**
    *   **Finding:** `struct cake_per_cpu` is aligned to 256 bytes, ensuring isolation between CPUs.
    *   **Finding:** `struct cake_scratch` fits within 128 bytes and is padded. The size is approximately 112 bytes (64B `gr_cache` + 8B `cached_now` + 4B `cached_llc` + 32B padding). This fits safely within the 128-byte alignment of the `mbox` + `scr` layout in `cake_per_cpu`.
    *   **Verification:** `_Static_assert` in `cake.bpf.c` guards against size regression.

*   **Magic Numbers:**
    *   **Action:** Several magic numbers were refactored into named constants in `intf.h` and `cake.bpf.c`:
        *   `CAKE_MIN_SLICE_NS` (200us)
        *   `CAKE_PRIO_NICE_0` (120)
        *   `CAKE_PRIO_NICE_10` (130)
        *   `CAKE_MIGRATION_COOLDOWN` (4)
        *   `CAKE_MAX_RT_US` (65535)
        *   `TIER_RECHECK_Tx` (255, 63, 15, 7)

## 3. Code Quality & Dead Code

*   **Unused Code:**
    *   `enable_dvfs`: Defined in BPF but unused (logic removed). Kept for loader compatibility. Comment updated to reflect this.
    *   `tier_perf_target`: Defined but unused. Kept for loader compatibility.
    *   `enable_stats`: `const bool = false`. Used in dead-code-eliminated blocks. This is a standard pattern for conditional compilation in BPF.

*   **Comments:**
    *   Comments in `cake.bpf.c` are generally high quality and explain the "Why" behind optimizations (e.g., "JIT eliminates unused P/E-core steering").

## 4. Recommendations for Future Research

*   **Bottleneck Analysis:** Focus on `scx_bpf_dsq_insert_vtime` cost. If this is high, investigating batched insertions or further reducing the number of insertions (e.g., via more aggressive local execution) could yield gains.
*   **Topology Scanning:** The unrolled loops in `cake_select_cpu` are efficient, but on very large core count systems (e.g., 128+ cores, though `CAKE_MAX_CPUS` is 64), the linear scan might eventually scale poorly. The current bitmask approach is optimal for <64 cores.
