# Performance Audit & Code Review

## Findings

### 1. Critical Logic Bug: Hardcoded Tier Classification
The `tier_classify_lut` in `cake.bpf.c` is hardcoded with thresholds (100us, 2ms, 8ms) that correspond to the `Gaming` profile. However, `main.rs` allows selecting other profiles (`Esports`, `Legacy`) with different thresholds (e.g., `Legacy` uses 6ms, 16ms, 80ms).

**Impact:** The BPF scheduler uses incorrect classification thresholds for non-Gaming profiles. For `Legacy`, tasks that should be T0 (<6ms) will be demoted to T1/T2 much earlier (at 100us/2ms), defeating the purpose of the profile.

**Fix:** Make `tier_classify_lut` and `tier_lut_shift` dynamic variables in `.rodata`, populated by `main.rs` based on the selected profile.

### 2. Dead Code: DVFS Support
The `cake.bpf.c` file contains `enable_dvfs` and `tier_perf_target` variables which are documented as "loader-compat only (tick removed, DVFS dead)". However, `main.rs` still contains logic to enable DVFS for the `Battery` profile and populate these variables.

**Impact:** Unnecessary complexity and potential confusion. The `Battery` profile advertises DVFS support which does not exist in the kernel datapath.

**Fix:** Remove DVFS-related code from `main.rs`, `intf.h`, and `cake.bpf.c`.

### 3. Optimization Opportunity: Dynamic Recheck Masks
The `tier_recheck_mask` (controlling how often tasks are re-evaluated for tier changes) is hardcoded.

**Impact:** Suboptimal recheck rates for different profiles/quantums.

**Fix:** Move `tier_recheck_mask` to `.rodata` and calculate it in `main.rs` based on the profile's quantum.

### 4. Magic Numbers
*   `TIER_LUT_SHIFT` (4) is hardcoded.
*   `TIER_LUT_ENTRIES` (512) is hardcoded.
*   `tier_recheck_mask` values are hardcoded.
*   `tier_classify_lut` contents are hardcoded.

### 5. False Sharing & Alignment
*   Excellent handling of alignment. `struct cake_scratch` (112B) and `struct mega_mailbox_entry` (128B) are correctly padded and aligned to prevent false sharing.
*   `struct cake_per_cpu` is 256B aligned.
*   `struct tier_snap` is 64B aligned.

### 6. Division & Modulus
*   Good use of bitwise operations for modulus (`& (CAKE_MAX_CPUS - 1)`).
*   Good use of shifts for division.
*   `victim -= nr_llcs` is used instead of modulo, which is good.

## Most Expensive Operations
Based on code analysis (no runtime tracing available):

1.  **`scx_bpf_dsq_insert_vtime`**: Called in `cake_enqueue`. This involves kernel-side DSQ management, locking, and potentially rbtree operations. It's the core scheduling primitive.
2.  **`scx_bpf_test_and_clear_cpu_idle`**: Called in `cake_select_cpu`. This is an atomic operation that modifies scheduler state.
3.  **`bpf_cpumask_test_cpu`**: Used in `cake_init_task` and `cake_set_cpumask`. Iterating over CPUs and calling this kfunc is relatively expensive compared to simple bitwise ops.
4.  **`bpf_get_smp_processor_id`**: Frequently used. While fast, it's still a function call. `cake.bpf.c` optimizes this by deferring calls where possible.
5.  **`scx_bpf_dsq_move_to_local`**: Used in `cake_dispatch`. This moves tasks between DSQs, involving locks.

## Recommendations for Research Focus
1.  **Dynamic Classification LUT:** Implementing the dynamic LUT is the highest priority to fix the logic bug for non-Gaming profiles.
2.  **Optimize `cake_select_cpu`:** The "psychic cache" and "confidence gate" mechanisms are already highly optimized. Further gains might come from refining the "nearby idle scan" (Gate 1c) to be more topology-aware without excessive iteration.
3.  **Remove Dead Code:** Cleaning up the DVFS code will simplify the codebase.
