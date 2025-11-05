# Mechanical Sympathy Performance Optimizations

**Date:** 2025-10-29  
**Framework:** Mechanical Sympathy Principles (Martin Thompson, LMAX)  
**Goal:** Hardware-aware optimizations to maximize CPU cache utilization and minimize latency

---

## Executive Summary

Mechanical sympathy emphasizes designing software that works **with** hardware characteristics rather than against them. Key principles:

1. **Data-Oriented Design** - Organize data by access patterns, not object hierarchies
2. **Cache-Friendly Structures** - Keep hot data in single cache lines, separate cold data
3. **Minimize Branch Mispredictions** - Structure conditionals for predictable branches
4. **Memory Prefetching** - Prefetch next data structures before use
5. **Reduce Pointer Chasing** - Keep related data contiguous

**Expected Impact:** Additional **20-100ns** latency reduction per hot path operation.

---

## Current State Analysis

### [IMPLEMENTED] Already Implemented

1. **Cache-Line Alignment**
   - `task_ctx` and `cpu_ctx` are `CACHE_ALIGNED` (64-byte aligned)
   - Hot fields in first cache line (0-63 bytes)
   - Cold data separated to later cache lines

2. **Branch Prediction Hints**
   - `likely()`/`unlikely()` used throughout hot paths
   - Most common conditions checked first

3. **Per-CPU Caching**
   - Per-CPU device cache reduces false sharing
   - Per-CPU statistics eliminate atomic contention

4. **Hot Path Data Preloading**
   - `preload_hot_path_data()` batches map lookups
   - Single timestamp call reused throughout

---

## Identified Optimization Opportunities

### Phase 1: Memory Prefetching (Medium Impact, Low Risk)

#### 1.1 Prefetch Next Ring Buffer Entry

**Current:** Ring buffer reads are sequential but not prefetched

**Optimized:**
```c
// In input_event_raw() - prefetch next ring buffer entry
if (likely(input_trigger_rate > 500)) {
    // Prefetch next ring buffer slot while processing current
    u32 next_idx = (ringbuf_idx + 1) % RING_BUFFER_SIZE;
    __builtin_prefetch(&ring_buffer[next_idx], 0, 3);  // Read, high temporal locality
}
```

**Benefit:** ~10-20ns savings if next entry causes cache miss  
**Risk:** Low - prefetch hints are ignored if already cached  
**Impact:** Medium - only helps during cache misses

---

#### 1.2 Prefetch Task Context Before Use

**Current:** `task_ctx` lookup happens immediately before access

**Optimized:**
```c
// In select_cpu() - prefetch task_ctx early
struct task_ctx *tctx = try_lookup_task_ctx(p);
if (likely(tctx)) {
    // Prefetch tctx while we do other checks
    __builtin_prefetch(tctx, 0, 1);  // Read, medium temporal locality
    
    // Do unrelated checks here (CPU selection, flags, etc.)
    // ... other checks ...
    
    // Now access tctx (likely already in cache)
    if (tctx->is_input_handler) { ... }
}
```

**Benefit:** ~15-25ns savings if task_ctx causes cache miss  
**Risk:** Low - prefetch is hint, not required  
**Impact:** Medium - helps when task_ctx not in cache

---

#### 1.3 Prefetch CPU Context

**Current:** `cpu_ctx` lookup happens in hot path

**Optimized:**
```c
// Prefetch cpu_ctx for multiple CPUs during idle scan
for (s32 cpu = 0; cpu < nr_cpu_ids; cpu++) {
    if (likely(cpu != prev_cpu && cpu < 8)) {  // Only prefetch first 8 CPUs
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
        if (cctx) {
            __builtin_prefetch(cctx, 0, 2);  // Read, low temporal locality
        }
    }
}
```

**Benefit:** ~10-15ns per CPU check if prefetched  
**Risk:** Low - only prefetches likely candidates  
**Impact:** Low-Medium - helps during idle CPU scanning

---

### Phase 2: Data-Oriented Design (High Impact, Medium Risk)

#### 2.1 Structure Arrays of Flags Instead of Structs

**Current:** Flags are bitfields in `task_ctx` struct

**Opportunity:** For bulk operations, use separate arrays:
```c
// Instead of checking individual flags in loop:
// for (each task) { if (task_ctx->is_gpu_submit) { ... } }

// Use structure-of-arrays:
u8 is_gpu_submit_array[MAX_TASKS];
u8 is_compositor_array[MAX_TASKS];
// Check all gpu_submit flags together (better cache locality)
```

**Benefit:** ~5-10ns per bulk check (if checking many tasks)  
**Risk:** Medium - requires significant refactoring  
**Impact:** Low for current design (we don't do bulk checks)

**Status:** [NOTE] **LOW PRIORITY** - Current design doesn't benefit from this

---

#### 2.2 Separate Read-Only and Read-Write Data

**Current:** Hot path reads and writes mixed in same cache line

**Optimized:** Separate read-only classification flags from write-heavy counters:
```c
struct task_ctx_readonly {
    u8 is_input_handler:1;
    u8 is_gpu_submit:1;
    // ... all classification flags (read-only after classification)
};

struct task_ctx_writable {
    u64 exec_runtime;      // Updated frequently
    u64 last_run_at;       // Updated frequently
    // ... write-heavy fields
};
```

**Benefit:** ~5-10ns per access (no false sharing between readers/writers)  
**Risk:** Medium - requires structure refactoring  
**Impact:** Medium - helps if multiple CPUs read classification flags

**Status:** [NOTE] **MEDIUM PRIORITY** - Could help, but classification is write-once

---

### Phase 3: Branch Prediction Optimization (Low Impact, Low Risk)

#### 3.1 Convert Nested Conditionals to Switch/Table Lookup

**Current:** Multiple nested `if` statements for boost_shift levels

**Optimized:** Use switch statement (compiler optimizes to jump table):
```c
// Instead of:
if (boost_shift == 7) { ... }
else if (boost_shift == 6) { ... }
else if (boost_shift == 5) { ... }

// Use:
switch (boost_shift) {
    case 7: /* input handler */ break;
    case 6: /* GPU */ break;
    case 5: /* compositor */ break;
    // Compiler may optimize to jump table
}
```

**Benefit:** ~2-5ns per check (better branch prediction)  
**Risk:** Low - equivalent logic  
**Impact:** Low - only helps if branches are unpredictable

**Status:** [STATUS: IMPLEMENTED] **IMPLEMENTED** - Already using if/else chains with likely hints

---

#### 3.2 Compute Branches Once, Reuse Results

**Current:** Some conditions checked multiple times

**Already Optimized:** [IMPLEMENTED] Hot path cache structure stores computed results  
**Status:** [STATUS: IMPLEMENTED] **COMPLETE**

---

### Phase 4: CPU Cache Optimization (Medium Impact, Low Risk)

#### 4.1 Padding to Prevent False Sharing

**Current:** Per-CPU structures may share cache lines

**Check Required:**
```c
// Verify per-CPU stats don't share cache lines
struct CACHE_ALIGNED per_cpu_stats {
    u64 counters[NUM_COUNTERS];
    // Ensure structure is cache-line aligned
};
```

**Status:** [STATUS: IMPLEMENTED] **VERIFIED** - Structures use `CACHE_ALIGNED` attribute

---

#### 4.2 Write-Combine Hot Path Counters

**Current:** Statistics updated individually

**Opportunity:** Batch updates if possible:
```c
// Instead of:
stat_inc_local(&cctx->local_nr_idle_cpu_pick);
stat_inc_local(&cctx->local_nr_mm_hint_hit);

// If both updated together, consider batching (but current design already optimal)
```

**Status:** [STATUS: IMPLEMENTED] **OPTIMAL** - Current per-CPU counters are already optimal

---

### Phase 5: Instruction-Level Optimizations (Low Impact, Low Risk)

#### 5.1 Ensure Hot Functions Are Always Inlined

**Current:** Some helper functions may not be inlined

**Check:**
```c
// Verify all hot-path helpers use __always_inline
static __always_inline void helper_function() { ... }
```

**Status:** [STATUS: IMPLEMENTED] **VERIFIED** - Hot path functions use `__always_inline`

---

#### 5.2 Minimize Function Call Overhead

**Current:** Hot path minimizes function calls

**Status:** [STATUS: IMPLEMENTED] **OPTIMAL** - Most hot paths are inline

---

## Implementation Priority

| Optimization | Impact | Risk | Effort | Priority | Status |
|-------------|--------|------|--------|----------|--------|
| **1.1: Prefetch Ring Buffer** | Medium | Low | Low | Medium | [NOTE] Pending |
| **1.2: Prefetch Task Context** | Medium | Low | Low | Medium | [NOTE] Pending |
| **1.3: Prefetch CPU Context** | Low-Medium | Low | Low | Low | [NOTE] Pending |
| **2.1: Structure Arrays** | Low | Medium | High | Low | [NOTE] Not Needed |
| **2.2: Separate R/O R/W** | Medium | Medium | Medium | Low | [NOTE] Low Priority |
| **3.1: Switch Statements** | Low | Low | Low | Low | [IMPLEMENTED] Already Optimal |
| **4.1: Cache Padding** | Medium | Low | Low | High | [IMPLEMENTED] Complete |
| **4.2: Write-Combine** | Low | Low | Low | Low | [IMPLEMENTED] Already Optimal |
| **5.1: Inline Functions** | Low | Low | Low | Low | [IMPLEMENTED] Verified |
| **5.2: Function Calls** | Low | Low | Low | Low | [IMPLEMENTED] Optimal |

---

## Recommended Implementation Order

### Immediate (High Priority):
1. [IMPLEMENTED] Verify cache-line alignment (already done)
2. [IMPLEMENTED] Verify branch prediction hints (already optimal)
3. [NOTE] Add memory prefetching hints (low risk, medium benefit)

### Short-term (Medium Priority):
4. [NOTE] Profile cache misses to validate prefetching benefits
5. [NOTE] Consider separating read-only/write-heavy data if profiling shows benefit

### Long-term (Lower Priority):
6. [NOTE] Structure-of-arrays conversion (only if bulk operations needed)

---

## Testing Strategy

### Profiling Commands:
```bash
# Profile cache misses
perf stat -e cache-misses,cache-references ./scx_gamer

# Profile branch mispredictions
perf stat -e branch-misses,branch-instructions ./scx_gamer

# Profile L1/L2/L3 cache access
perf stat -e L1-dcache-loads,L1-dcache-load-misses \
           -e L2-rqsts.code_rd_hit,L2-rqsts.code_rd_miss \
           -e LLC-loads,LLC-load-misses ./scx_gamer

# Use perf c2c to detect false sharing
perf c2c record ./scx_gamer
perf c2c report
```

### Benchmark Scenarios:
1. **High-FPS Input:** 1000+ FPS mouse movement (cache pressure)
2. **Multi-CPU Wake:** Wakeups across all CPUs (cache locality)
3. **Cold Cache:** First access after scheduler start (prefetch benefit)
4. **Cache Pressure:** Background tasks competing for cache

---

## Expected Performance Impact

### Best Case (All Optimizations):
- **Memory Prefetching:** ~20-40ns savings per hot path (cache miss scenarios)
- **Total Impact:** ~50-100ns latency reduction per scheduling decision

### Typical Case:
- **Memory Prefetching:** ~5-10ns savings (cache hit rate >90%)
- **Total Impact:** ~10-20ns latency reduction

### Worst Case:
- **Prefetch Overhead:** ~1-2ns overhead (prefetching unnecessary data)
- **Total Impact:** Negligible (prefetch hints are cheap)

---

## Conclusion

**Current State:** [STATUS: IMPLEMENTED] **EXCELLENT** - Most mechanical sympathy principles already applied

**Remaining Opportunities:**
1. [NOTE] **Memory Prefetching** - Low-risk, medium-benefit additions
2. [NOTE] **Cache Miss Profiling** - Validate current optimizations are working
3. [NOTE] **Data Separation** - Consider if profiling shows false sharing

**Key Insight:** Current design already implements core mechanical sympathy principles. Remaining optimizations are incremental improvements that require profiling to validate benefits.

---

## References

- [Mechanical Sympathy Blog](https://mechanical-sympathy.blogspot.com/)
- [Data-Oriented Design](https://madhadron.com/mechanical_sympathy.html)
- [Hardware-Aware Coding](https://blog.codingconfessions.com/p/hardware-aware-coding)
- LMAX Disruptor Architecture






