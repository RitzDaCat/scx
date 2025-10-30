# Loop Unrolling Implementation: HFT Pattern

**Date:** 2025-01-28  
**Pattern:** HFT loop unrolling for CPU scanning hot path

---

## Implementation Plan

### **Target: CPU Scan Loop in `pick_idle_physical_core()`**

**Current Code:**
```c
bpf_for(i, 0, MAX_CPUS) {
    s32 candidate = (s32)preferred_cpus[i];
    if (candidate < 0 || (u32)candidate >= nr_cpu_ids)
        break;
    if (!bpf_cpumask_test_cpu(candidate, allowed))
        continue;
    // ... prefetch and check idle ...
    if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
        return candidate;
    }
}
```

**Optimization:**
- Unroll first 4 iterations (8-core systems common case)
- Eliminates loop overhead (increment, comparison, branch)
- Better CPU branch prediction

---

## Implementation Strategy

### **Option 1: Manual Unrolling** (Recommended)

**Approach:** Unroll first 4 iterations explicitly

**Benefits:**
- Clear and maintainable
- BPF verifier friendly
- Predictable performance

**Code:**
```c
// Unroll first 4 iterations for 8-core systems
// This eliminates loop overhead for common case

/* Iteration 0 */
if (i < MAX_CPUS) {
    s32 candidate = (s32)preferred_cpus[0];
    if (candidate >= 0 && (u32)candidate < nr_cpu_ids &&
        bpf_cpumask_test_cpu(candidate, allowed)) {
        // Prefetch candidate 1 while checking candidate 0
        if (likely(1 < MAX_CPUS)) {
            s32 next_candidate = (s32)preferred_cpus[1];
            if (next_candidate >= 0 && (u32)next_candidate < nr_cpu_ids &&
                bpf_cpumask_test_cpu(next_candidate, allowed)) {
                struct cpu_ctx *next_cctx = try_lookup_cpu_ctx(next_candidate);
                if (likely(next_cctx)) {
                    __builtin_prefetch(next_cctx, 0, 2);
                }
            }
        }
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(candidate);
        if (cctx) {
            __builtin_prefetch(cctx, 0, 2);
        }
        if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
            if (tctx) {
                tctx->preferred_physical_core = candidate;
                tctx->preferred_core_hits = 1;
                tctx->preferred_core_last_hit = now;
            }
            return candidate;
        }
    }
}

/* Iteration 1 */
if (i + 1 < MAX_CPUS) {
    s32 candidate = (s32)preferred_cpus[1];
    // ... same pattern ...
}

/* Iteration 2 */
if (i + 2 < MAX_CPUS) {
    s32 candidate = (s32)preferred_cpus[2];
    // ... same pattern ...
}

/* Iteration 3 */
if (i + 3 < MAX_CPUS) {
    s32 candidate = (s32)preferred_cpus[3];
    // ... same pattern ...
}

// Fallback to loop for remaining CPUs
bpf_for(i, 4, MAX_CPUS) {
    // ... existing loop code ...
}
```

---

### **Option 2: Conditional Unrolling** (Alternative)

**Approach:** Use compile-time constant to enable/disable unrolling

**Benefits:**
- Configurable
- Easy to disable if issues arise

**Code:**
```c
#define UNROLL_CPU_SCAN 1  // Enable unrolling

#if UNROLL_CPU_SCAN
    // Unrolled code for first 4 iterations
#else
    // Loop code
#endif
```

---

## Expected Performance Impact

### **On 8-Core System (9800X3D):**

**Before:**
- Loop overhead: ~5-10ns per iteration
- 4 iterations: ~20-40ns overhead
- Branch misprediction: ~1-3ns per iteration

**After:**
- Zero loop overhead for first 4 iterations
- Better branch prediction (straight-line code)
- Savings: ~20-40ns per CPU selection

**Cumulative:**
- CPU selection happens on every wakeup
- High-FPS gaming: ~240 wakeups/sec
- Total savings: ~4.8-9.6µs/sec per CPU selection call

---

## BPF Verifier Considerations

### **Potential Issues:**
1. **Code size limits** - Unrolled code increases size
2. **Verifier complexity** - May hit verification time limits
3. **Static analysis** - Verifier must prove safety of all paths

### **Mitigation:**
- Test thoroughly with BPF verifier
- Fallback to loop if unrolling fails
- Document verifier compatibility

---

## Testing Plan

1. **Compile test** - Verify BPF compilation succeeds
2. **Verifier test** - Ensure unrolled code passes verification
3. **Performance test** - Measure CPU selection latency
4. **Regression test** - Verify no functionality changes

---

## Rollout Strategy

### **Phase 1: Conservative** (Recommended)
- Unroll only first 2 iterations
- Lower risk, still measurable benefit
- Easy to revert if issues

### **Phase 2: Aggressive**
- Unroll first 4 iterations
- Maximum benefit for 8-core systems
- Higher verifier risk

---

## Alternative: Macro-Based Approach

**Option:** Create macro to generate unrolled code

```c
#define UNROLL_CPU_CHECK(n) \
    do { \
        s32 candidate = (s32)preferred_cpus[n]; \
        if (candidate >= 0 && (u32)candidate < nr_cpu_ids && \
            bpf_cpumask_test_cpu(candidate, allowed)) { \
            /* ... check idle ... */ \
            if (scx_bpf_test_and_clear_cpu_idle(candidate)) { \
                return candidate; \
            } \
        } \
    } while (0)

// Usage:
UNROLL_CPU_CHECK(0);
UNROLL_CPU_CHECK(1);
UNROLL_CPU_CHECK(2);
UNROLL_CPU_CHECK(3);
```

**Benefits:**
- Reduces code duplication
- Easier to maintain
- Same performance as manual unrolling

---

## Recommendation

**Implement manual unrolling for first 4 iterations:**
- Clear and maintainable
- Maximum benefit for 8-core systems
- Lower risk than macro approach
- Easy to verify and test

**Expected Impact:**
- ~20-40ns savings per CPU selection
- ~5-10% faster CPU selection on 8-core systems
- Better branch prediction
- Negligible code size increase

