# Expanded Performance Patterns Review

**Date:** 2025-11-05  
**Status:** Analysis Complete - Additional Optimizations Identified

---

## Performance Tier Analysis

### ✅ **Tier 0: Sub-10ns (Already Optimal)**

#### 1. Register Arithmetic ✅
- **Status:** ✅ **Optimal** - Using register arithmetic throughout
- **Example:** `prev_cpu & ~1` (physical core calculation)
- **No Action Needed**

#### 2. Direct task_struct Read ✅
- **Status:** ✅ **Optimal** - Using cached flags (`p->scx.flags`)
- **Example:** `is_gpu_submit_cached(p)` → `p->scx.flags & SCX_GAMER_FLAG_GPU_SUBMIT`
- **No Action Needed**

#### 3. Static Inline Functions ✅
- **Status:** ✅ **Optimal** - All helpers are `static __always_inline`
- **Example:** `preload_hot_path_data()`, `get_fg_tgid()`, `is_input_handler_cached()`
- **No Action Needed**

#### 4. scx_bpf_now() ✅
- **Status:** ✅ **Optimal** - Using `scx_bpf_now()` not `bpf_ktime_get_ns()`
- **Example:** Line 2852 - single call, reused throughout
- **No Action Needed**

#### 5. Compiler-Unrolled Loops ✅
- **Status:** ✅ **Good** - Using unrolled loops where appropriate
- **Example:** `for (int i = 0; i < 64; i++)` for ring buffer distribution
- **Opportunity:** Could unroll more loops (see Issue 1)

---

### ✅ **Tier 1: 10-50ns (Already Optimal)**

#### 6. Per-CPU Array Read ✅
- **Status:** ✅ **Optimal** - Using `BPF_MAP_TYPE_PERCPU_ARRAY` for `cpu_ctx`
- **Example:** `try_lookup_cpu_ctx(cpu)` → `bpf_map_lookup_percpu_elem()`
- **No Action Needed**

#### 7. Per-CPU Array Write ✅
- **Status:** ✅ **Optimal** - Updating per-CPU counters
- **Example:** `cctx->local_nr_direct_dispatches++`
- **No Action Needed**

#### 8. Topology-Aware Map ⚠️ **OPPORTUNITY**
- **Status:** ⚠️ **Partial** - Using NUMA-aware DSQs, but not L3-aware maps
- **Current:** `numa_enabled` flag, per-node DSQs
- **Opportunity:** Use L3 cache domain arrays instead of per-CPU arrays (see Issue 2)

#### 9. SCX_DSQ_LOCAL_ON ✅
- **Status:** ✅ **Optimal** - Using direct dispatch extensively
- **Example:** Line 2869 - `scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | per_cpu_bound, ...)`
- **No Action Needed**

#### 10. Per-CPU Hash Read ⚠️ **OPPORTUNITY**
- **Status:** ⚠️ **Not Used** - Could replace shared hash maps with per-CPU hash maps
- **Opportunity:** Convert `mm_last_cpu` LRU hash to per-CPU hash (see Issue 3)

---

### ⚠️ **Tier 2: 50-200ns (Warm Path Only)**

#### 11. bpf_ringbuf_output() ✅
- **Status:** ✅ **Optimal** - Only used in warm paths (enqueue/running)
- **Example:** Conditional writes with `if (likely(!no_stats))`
- **No Action Needed**

#### 12. BPF_MAP_TYPE_QUEUE ⚠️ **NOT USED**
- **Status:** ⚠️ **Not Used** - Could use for E-Core queues
- **Opportunity:** Consider for future E-Core optimization

---

### ❌ **Tier 3: 200ns+ (Anti-Patterns)**

#### 14. Shared Map: Read ⚠️ **ISSUE FOUND**
- **Status:** ⚠️ **Used in Hot Path** - `mm_last_cpu` and `system_audio_tgids_map`
- **Locations:**
  - Line 851: `bpf_map_lookup_elem(&mm_last_cpu, &mm_key)` - CPU selection
  - Line 3830: `bpf_map_lookup_elem(&system_audio_tgids_map, &tgid)` - Classification
- **Impact:** ~100-300ns per lookup (shared map contention)
- **Fix:** Convert to per-CPU hash or topology-aware map (see Issue 3)

#### 15. Shared Map: Write ⚠️ **ISSUE FOUND**
- **Status:** ⚠️ **Used in Warm Path** - `mm_last_cpu` updates
- **Location:** Line 700 - `bpf_map_update_elem(&mm_last_cpu, &key, &cpu, BPF_ANY)`
- **Impact:** ~150-500ns per update (shared map contention)
- **Fix:** Convert to per-CPU hash or remove if not critical (see Issue 3)

#### 16. scx_bpf_migrate() ✅
- **Status:** ✅ **Not Used** - Using `SCX_DSQ_LOCAL_ON` instead
- **No Action Needed**

---

## Critical Issues Found

### Issue 1: Variable Loops vs Compiler-Unrolled Loops ⚠️ **MEDIUM**

**Locations:**
- Line 2444: `bpf_for(cpu, 8, nr_cpu_ids)` - Timer aggregation
- Line 4956: `bpf_for(cpu, 0, nr_cpu_ids)` - Initialization
- Line 4988: `bpf_for(node, 0, __COMPAT_scx_bpf_nr_node_ids())` - NUMA setup

**Current:** Variable loops with verifier overhead  
**Opportunity:** Compiler-unrolled loops if bounds are small (< 64)

**Impact:** ~20-50ns per iteration  
**Risk:** Medium - Need to verify loop bounds

**Example Fix:**
```c
/* BEFORE */
bpf_for(cpu, 0, nr_cpu_ids) {
    /* ... */
}

/* AFTER - If nr_cpu_ids <= 64 */
for (int cpu = 0; cpu < 64 && cpu < nr_cpu_ids; cpu++) {
    /* ... */
}
```

---

### Issue 2: Topology-Aware Maps Not Used ⚠️ **LOW**

**Current:** Using per-CPU arrays for all CPU-local data  
**Opportunity:** Use L3 cache domain arrays for better cache locality

**Impact:** ~5-15ns improvement per lookup (better cache locality)  
**Risk:** High - Requires topology detection, complexity increase

**Example:**
```c
/* CURRENT */
struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);  /* Per-CPU array */

/* POTENTIAL */
u32 l3_id = get_l3_domain_id(cpu);  /* Topology pre-computed */
struct cpu_ctx *cctx = bpf_map_lookup_elem(&cpu_ctx_l3_array, &l3_id);
```

**Status:** ⚠️ **Low Priority** - Current per-CPU arrays are already Tier 1

---

### Issue 3: Shared Hash Maps in Hot Path ⚠️ **HIGH**

**Location 1:** `mm_last_cpu` (LRU Hash Map)
- **Usage:** Line 851 - CPU selection hint
- **Frequency:** Every CPU selection call
- **Impact:** ~100-300ns per lookup (shared map contention)

**Location 2:** `system_audio_tgids_map` (Hash Map)
- **Usage:** Line 3830 - Audio thread classification
- **Frequency:** Every classification call
- **Impact:** ~100-300ns per lookup (shared map contention)

**Current Tier:** Tier 3 (100-300ns) - **Anti-Pattern**  
**Target Tier:** Tier 1 (10-50ns) - Per-CPU Hash

**Options:**

#### Option A: Convert to Per-CPU Hash (Recommended)
```c
/* BEFORE */
struct {
    __uint(type, BPF_MAP_TYPE_LRU_HASH);
    __type(key, u64);  /* MM pointer */
    __type(value, u32);  /* CPU ID */
} mm_last_cpu SEC(".maps");

/* AFTER */
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_HASH);
    __type(key, u64);  /* MM pointer */
    __type(value, u32);  /* CPU ID */
} mm_last_cpu_percpu SEC(".maps");
```

**Impact:** ~100-250ns improvement per lookup  
**Trade-off:** Per-CPU buckets (less accurate, but faster)

#### Option B: Remove if Not Critical
- **Analysis:** MM hint improves cache locality, but not critical for gaming
- **Impact:** ~100-300ns saved per CPU selection
- **Risk:** Low - Gaming workloads less sensitive to cache locality than server workloads

**Recommendation:** **Option B** - Remove MM hint for gaming workloads (low cache locality benefit, high overhead)

---

## Additional Optimizations

### Optimization 1: More Compiler-Unrolled Loops

**Opportunity:** Convert variable loops to unrolled loops where safe

**Impact:** ~20-50ns per iteration  
**Risk:** Medium - Need to verify bounds

**Priority:** Medium

---

### Optimization 2: Topology-Aware Maps (Future)

**Opportunity:** Use L3 cache domain arrays instead of per-CPU arrays

**Impact:** ~5-15ns improvement (better cache locality)  
**Risk:** High - Complexity increase

**Priority:** Low - Current per-CPU arrays are already Tier 1

---

### Optimization 3: Remove Shared Maps (High Priority)

**Opportunity:** Remove or convert shared hash maps

**Impact:** ~100-300ns saved per lookup  
**Risk:** Low - Gaming workloads less sensitive to MM hints

**Priority:** **HIGH** - Easy win, significant impact

---

## Recommended Action Plan

### Phase 1: Remove Shared Maps (Immediate - High Impact)

1. **Remove MM Hint (`mm_last_cpu`)**
   - **Impact:** ~100-300ns saved per CPU selection
   - **Risk:** Low - Gaming workloads less sensitive to cache locality
   - **Time:** 30 minutes

2. **Convert `system_audio_tgids_map` to Per-CPU Hash**
   - **Impact:** ~100-250ns improvement per classification
   - **Risk:** Low - Per-CPU buckets acceptable for audio detection
   - **Time:** 1 hour

**Expected Total Improvement:** ~200-550ns per hot path call

---

### Phase 2: Compiler-Unrolled Loops (Medium Priority)

1. Convert variable loops to unrolled loops where safe
   - **Impact:** ~20-50ns per iteration
   - **Risk:** Medium - Need to verify bounds
   - **Time:** 2-3 hours

---

### Phase 3: Topology-Aware Maps (Future - Low Priority)

1. Implement L3 cache domain arrays
   - **Impact:** ~5-15ns improvement
   - **Risk:** High - Complexity increase
   - **Time:** 1-2 days

---

## Conclusion

**Current Status:** ✅ **Mostly Optimal** - Using Tier 0/1 patterns extensively

**Critical Issues:**
1. ⚠️ **Shared hash maps in hot path** (Tier 3 anti-pattern)
   - `mm_last_cpu`: ~100-300ns per lookup
   - `system_audio_tgids_map`: ~100-300ns per lookup

**Recommended Actions:**
1. **Remove MM hint** (low benefit for gaming, high overhead)
2. **Convert audio map to per-CPU hash** (maintains functionality, improves performance)

**Expected Improvement:** ~200-550ns per hot path call

**Status:** Ready for implementation

---

**Last Updated:** 2025-11-05

