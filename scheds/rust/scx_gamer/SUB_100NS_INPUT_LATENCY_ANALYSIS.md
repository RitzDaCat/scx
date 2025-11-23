# Sub-100ns Input Latency Analysis

## Current State: ~151ns Total Input Latency

### Breakdown:
```
Total: ~151ns
├─ input_event_raw (fentry):     26-30ns  [KERNEL - Cannot optimize]
├─ Input boost recording:        ~26ns    [Already optimized]
└─ gamer_select_cpu (ultra-fast): ~95-105ns  [TARGET FOR OPTIMIZATION]
   ├─ is_input_handler_cached(): ~1-5ns
   ├─ scx_bpf_now():             ~10-15ns  ← OPPORTUNITY #1 (10-15ns savings)
   ├─ time_before() check:       ~1-2ns
   ├─ Slice calculation:         ~2-5ns    ← OPPORTUNITY #2 (2-5ns savings)
   ├─ scx_bpf_test_and_clear():  ~10-15ns  [KERNEL - Cannot optimize]
   ├─ scx_bpf_dsq_insert():      ~20-30ns  ← OPPORTUNITY #3 (see below)
   ├─ PROF_END_HIST (coalesced): ~0-2ns    (runs 1/64 times)
   └─ RETURN_SELECTED_CPU:       ~5-10ns   ← OPPORTUNITY #4 (5-10ns savings)
```

**Target: 95-105ns → 45-55ns (need to save ~50ns)**

---

## Optimization Opportunities

### 🎯 OPPORTUNITY #1: Eliminate Timestamp Call (Save ~10-15ns)

**Current:**
```c
if (unlikely(is_input_handler_cached(p))) {
    now = scx_bpf_now();  // ~10-15ns
    
    if (time_before(now, input_until_global)) {
        // ... dispatch
    }
}
```

**Optimization: Speculative Always-Boost Mode**

Input handlers are ALWAYS latency-critical. We could skip the window check entirely:

```c
if (unlikely(is_input_handler_cached(p))) {
    // Skip timestamp and window check entirely!
    // Input handlers ALWAYS get fast path treatment
    
    u64 input_slice = continuous_input_mode ? slice_ns : (slice_ns >> 2);
    
    if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, input_slice, 0);
        return prev_cpu;  // Direct return, no macro overhead
    }
    // ... fallback
}
```

**Savings:** 10-15ns (eliminates `scx_bpf_now()` call)

**Risk:** Low
- Input handlers are ALWAYS latency-critical
- Window expiry only affects deadline calculation, not CPU selection
- We'd still use the input handler flag which is already cached

**Trade-off:**
- Slightly higher priority for input handlers even outside input window
- But input handlers are rare (1-2 threads) so minimal system impact

---

### 🎯 OPPORTUNITY #2: Fixed Slice for Input Handlers (Save ~2-5ns)

**Current:**
```c
u64 input_slice = continuous_input_mode ? slice_ns : (slice_ns >> 2);
```

**Optimization: Fixed Slice**

Input handlers benefit from consistency. Use a fixed optimal slice:

```c
// At file scope:
#define INPUT_HANDLER_SLICE_NS 2500  // 2.5µs - optimal for input processing

// In fast path:
if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
    scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, INPUT_HANDLER_SLICE_NS, 0);
    return prev_cpu;
}
```

**Savings:** 2-5ns (eliminates conditional evaluation and shift operation)

**Risk:** Very Low
- 2.5µs is already the bursty mode slice (slice_ns >> 2)
- Continuous mode doesn't benefit from longer slices for input threads
- Input handlers yield quickly anyway

---

### 🎯 OPPORTUNITY #3: Direct CPU Return (Save ~20-30ns?)

**Current:**
```c
scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, input_slice, 0);  // ~20-30ns
RETURN_SELECTED_CPU(prev_cpu);  // ~5-10ns
```

**Research Needed:**

Can we bypass `scx_bpf_dsq_insert()` for input handlers?

Options to investigate:
1. **Direct return + enqueue path dispatch**: Return CPU immediately, let enqueue path handle actual dispatch
2. **Inline dispatch**: Can we call a lighter-weight dispatch helper?
3. **Deferred dispatch**: Signal input handler priority without immediate dispatch

**Blocker:** Need to understand SCX contract - does `select_cpu()` REQUIRE dispatch or just CPU selection?

**Potential Savings:** 20-30ns if we can defer dispatch

**Risk:** High - might violate scheduler contract

---

### 🎯 OPPORTUNITY #4: Inline Return (Save ~5-10ns)

**Current:**
```c
#define RETURN_SELECTED_CPU(val)    \
    do {                            \
        if (tctx && now) {          \
            tctx->last_idle_cpu_hint = (val);  \
            tctx->last_idle_cpu_hint_ts = now; \
        }                           \
        PROF_END_HIST(select_cpu);  \
        return (val);               \
    } while (0)
```

**Optimization: Direct Return for Input Handlers**

Input handlers don't benefit from idle CPU hints (they're latency-critical, not throughput-critical):

```c
// In input handler fast path:
if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
    scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, INPUT_HANDLER_SLICE_NS, 0);
    return prev_cpu;  // Direct return, skip macro overhead
}
```

**Savings:** 5-10ns (skip conditional checks, profiling, hint updates)

**Risk:** Very Low
- Input handlers don't benefit from CPU hints (always prefer prev_cpu)
- Profiling already coalesced 64:1
- `tctx` not loaded in ultra-fast path

---

### 🎯 OPPORTUNITY #5: Optimize input_event_raw Further (Save ~5-10ns?)

**Current (best case): ~26-30ns**

```c
u64 now_shared = scx_bpf_now();  // ~10-15ns

if (likely(continuous_input_mode && input_trigger_rate > 500)) {
    struct device_cache_entry *cached = bpf_map_lookup_elem(&device_cache_percpu, &cache_slot);
    
    if (likely(cached && cached->whitelisted && cached->dev_ptr == dev_key)) {
        record_input_boost(cached->lane_hint, now_shared, NULL);
        return 0;
    }
}
```

**Optimization: Eliminate Timestamp in Fast Path**

`record_input_boost()` uses the timestamp for rate limiting and window updates. Can we:
1. Use a cached/stale timestamp?
2. Defer timestamp to slower update path?
3. Use atomic counter instead of timestamp?

**Potential Savings:** 5-10ns if we can eliminate `scx_bpf_now()`

**Risk:** Medium
- Window timing might be less precise
- But 10-15ns staleness is negligible for 6ms/300ms windows

---

## Combined Optimization Strategy

### **CONSERVATIVE Approach (Low Risk):**

Apply optimizations #1, #2, #4:

```c
if (unlikely(is_input_handler_cached(p))) {
    // OPTIMIZATION #1: Skip timestamp and window check
    // OPTIMIZATION #2: Fixed slice
    
    if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, INPUT_HANDLER_SLICE_NS, 0);
        return prev_cpu;  // OPTIMIZATION #4: Direct return
    }
    
    // Physical core fallback
    if (prev_cpu & 1) {
        s32 phys_cpu = prev_cpu & ~1;
        if (bpf_cpumask_test_cpu(phys_cpu, p->cpus_ptr) &&
            scx_bpf_test_and_clear_cpu_idle(phys_cpu)) {
            scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, INPUT_HANDLER_SLICE_NS, 0);
            return phys_cpu;
        }
    }
}
```

**Expected Savings:**
- Optimization #1: 10-15ns
- Optimization #2: 2-5ns
- Optimization #4: 5-10ns
- **Total: 17-30ns reduction**

**New Latency:**
- input_event_raw: 26-30ns
- Input boost: 26ns
- select_cpu: **65-88ns** (was 95-105ns)
- **Total: ~117-144ns** (was ~151ns)

**Still short of 100ns goal by ~17-44ns**

---

### **AGGRESSIVE Approach (Medium Risk):**

Apply all optimizations including #5:

**Modify `input_event_raw` to use deferred timestamp:**

```c
// Use a shared timestamp updated every ~100µs (negligible staleness for 6ms windows)
volatile u64 input_event_shared_timestamp = 0;

SEC("fentry/input_event")
int BPF_PROG(input_event_raw, ...) {
    // OPTIMIZATION: Use shared timestamp (updated in dispatch or housekeeping)
    u64 now_shared = input_event_shared_timestamp;
    if (!now_shared)
        now_shared = scx_bpf_now();  // Fallback
    
    // ... rest of fast path
}
```

**Expected Savings:**
- Conservative optimizations: 17-30ns
- input_event_raw optimization: 5-10ns
- **Total: 22-40ns reduction**

**New Latency:**
- input_event_raw: **20-25ns** (was 26-30ns)
- Input boost: 26ns
- select_cpu: **65-88ns** (was 95-105ns)
- **Total: ~111-139ns**

**Still short of 100ns goal by ~11-39ns**

---

## The 100ns Barrier

### **Why 100ns is HARD:**

The remaining overhead is primarily **kernel helpers** we cannot optimize:

1. `scx_bpf_test_and_clear_cpu_idle()`: ~10-15ns (atomic operation)
2. `scx_bpf_dsq_insert()`: ~20-30ns (kernel dispatch queue)
3. `scx_bpf_now()`: ~10-15ns (VDSO call)

These are **kernel-side operations** that BPF cannot bypass.

**Total kernel overhead: ~40-60ns minimum**

### **Theoretical Minimum:**

```
Theoretical best case:
- input_event_raw (minimal): ~15-20ns (kernel hook entry)
- Input boost (cached):      ~15-20ns (if we cache timestamp)
- Kernel helpers:            ~40-60ns (unavoidable)
- BPF logic overhead:        ~5-10ns
-----------------------------------------
TOTAL:                       ~75-110ns
```

**The 100ns barrier is achievable only with aggressive caching and minimal kernel helper calls.**

---

## Recommended Next Steps

### **Phase 1: Conservative Optimizations (LOW RISK)**

Implement optimizations #1, #2, #4:
- Expected: ~117-144ns (from ~151ns)
- Savings: ~17-30ns
- Risk: Very low

### **Phase 2: Aggressive Optimizations (MEDIUM RISK)**

Add optimization #5 (shared timestamp):
- Expected: ~111-139ns (from ~117-144ns)
- Savings: ~5-10ns additional
- Risk: Medium (timestamp precision)

### **Phase 3: Research (HIGH RISK)**

Investigate optimization #3 (bypass scx_bpf_dsq_insert):
- Potential: ~80-110ns (if successful)
- Savings: ~20-30ns additional
- Risk: High (might violate scheduler contract)
- Requires: Deep dive into SCX internals

---

## Conclusion

**Realistic Goal: ~110-140ns** (with conservative + aggressive optimizations)

**Stretch Goal: ~80-110ns** (requires kernel helper bypass research)

**Hard Limit: ~75ns** (theoretical minimum with perfect caching)

The **100ns barrier is achievable** but requires aggressive optimization including:
1. ✅ Eliminating unnecessary timestamp calls
2. ✅ Fixed slices for input handlers
3. ✅ Direct returns (skip macro overhead)
4. ⚠️  Shared timestamp caching (medium risk)
5. ❌ Bypassing scx_bpf_dsq_insert (needs research)

**Recommendation:** Start with Phase 1 (conservative) to get to ~120-140ns, then evaluate if Phase 2 is worth the complexity.

