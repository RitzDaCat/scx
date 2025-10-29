# LMAX-Inspired Performance Optimization Analysis

**Date:** 2025-01-XX  
**Target:** Ultra-low latency optimizations inspired by LMAX Disruptor and HFT systems

---

## Executive Summary

Analysis of `scx_gamer` codebase for LMAX/HFT-style optimizations. Identified **12 optimization opportunities** across memory barriers, cache utilization, NUMA awareness, and lock-free algorithms.

**Expected Impact:** Additional **50-200ns** latency reduction per hot path operation.

---

## LMAX Disruptor Principles Applied

### **1. Lock-Free Architecture** ✅ **Already Implemented**
- Ring buffer: Lock-free `SegQueue` ✅
- Atomic operations: `Arc<AtomicU32>` for game detection ✅
- Zero mutex contention in hot paths ✅

### **2. Single Writer Principle** ⚠️ **Partially Implemented**
- BPF writes to ring buffer (single writer) ✅
- Userspace reads from ring buffer (single reader) ✅
- **Issue:** Multiple BPF CPUs may write simultaneously (needs per-CPU buffers)

### **3. Cache-Line Optimization** ✅ **Already Implemented**
- `task_ctx` cache-line aligned (64 bytes) ✅
- Hot fields in first cache line ✅
- Cold data separated ✅

### **4. False Sharing Avoidance** ⚠️ **Needs Review**
- Per-CPU structures: Need verification of cache-line separation
- Atomic counters: May share cache lines across CPUs

---

## Identified Optimization Opportunities

### **Phase 1: Memory Barrier Optimization** (High Impact, Low Risk)

#### **1.1 Replace `__sync_fetch_and_add` with `__atomic_fetch_add`**

**Current:**
```c
__sync_fetch_and_add(&compositor_detect_page_flips, 1);
```

**Optimized:**
```c
__atomic_fetch_add(&compositor_detect_page_flips, 1, __ATOMIC_RELAXED);
```

**Benefit:** 
- `__sync_*` uses full memory barrier (`__ATOMIC_SEQ_CST`)
- `__ATOMIC_RELAXED` only requires atomicity, no ordering constraints
- **Savings:** ~5-10ns per operation (on x86, but helps on ARM/POWER)

**Risk:** Low - Statistics counters don't need strict ordering

**Files to Update:**
- `compositor_detect.bpf.h` - All statistics counters
- `gpu_detect.bpf.h` - GPU call counters
- `network_detect.bpf.h` - Network call counters
- `audio_detect.bpf.h` - Audio call counters
- All other detection modules

**Estimated Impact:** ~20-50ns savings per hot path (when stats enabled)

---

#### **1.2 Use Acquire/Release Semantics Selectively**

**Current:** Full barriers everywhere (overkill)

**Optimized:** 
- **Acquire** for reads (consumer side)
- **Release** for writes (producer side)
- **Relaxed** for statistics (no ordering needed)

**Example:**
```c
// Producer (BPF): Use RELEASE
__atomic_store_n(&ring_buffer_head, new_head, __ATOMIC_RELEASE);

// Consumer (Userspace): Use ACQUIRE
u64 head = __atomic_load_n(&ring_buffer_head, __ATOMIC_ACQUIRE);
```

**Benefit:** ~2-5ns savings per operation (architecture-dependent)

---

### **Phase 2: Cache Optimization** (Medium Impact, Low Risk)

#### **2.1 Per-CPU Statistics (Eliminate False Sharing)**

**Current:**
```c
volatile u64 compositor_detect_page_flips;  // Shared across all CPUs
```

**Optimized:**
```c
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, u64);
} compositor_detect_page_flips_percpu SEC(".maps");
```

**Benefit:**
- Eliminates false sharing (each CPU has its own cache line)
- **Savings:** ~10-30ns per stat update (avoids cache line bouncing)

**Trade-off:** 
- Need aggregation in userspace (only for stats display)
- One-time cost vs per-operation savings

---

#### **2.2 Cache-Line Padding for Hot Structures**

**Current:** `task_ctx` is cache-line aligned, but verify other structures

**Check:**
- `cpu_ctx` alignment
- Ring buffer metadata alignment
- BPF map structures

**Example:**
```c
struct CACHE_ALIGNED cpu_ctx {
    // Hot fields first (within 64 bytes)
    u64 last_idle_scan;
    u64 interactive_avg;
    // ... rest of fields
} __attribute__((aligned(64)));
```

---

### **Phase 3: NUMA Awareness** (High Impact, Medium Risk)

#### **3.1 NUMA-Aware CPU Selection**

**Current:** CPU selection doesn't consider NUMA topology

**Optimized:**
```c
// Track NUMA node for each CPU
static __always_inline s32 select_cpu_numa_aware(struct task_struct *p) {
    s32 current_cpu = scx_bpf_task_cpu(p);
    s32 current_numa = get_numa_node(current_cpu);
    
    // Prefer CPUs on same NUMA node (lower memory latency)
    for (s32 cpu = 0; cpu < nr_cpu_ids; cpu++) {
        if (get_numa_node(cpu) == current_numa && is_cpu_idle(cpu)) {
            return cpu;
        }
    }
    
    // Fallback to any CPU
    return select_cpu_generic(p);
}
```

**Benefit:**
- **~50-100ns** savings per memory access (local NUMA node)
- Especially important for GPU/compositor threads (memory-intensive)

**Implementation:**
- Use kernel's `cpu_to_node()` function
- Cache NUMA topology in BPF map

---

#### **3.2 NUMA-Aware Ring Buffer**

**Current:** Single ring buffer for all CPUs

**Optimized:** Per-NUMA-node ring buffers

**Benefit:**
- Reduce cross-NUMA memory access
- **~20-50ns** savings per ring buffer operation

**Trade-off:** More complex userspace aggregation

---

### **Phase 4: Branch Prediction** ✅ **Already Implemented**

**Current:** `likely()`/`unlikely()` hints used ✅

**Additional Opportunity:**
- Profile hot branches and optimize ordering
- Use `__builtin_expect()` for critical paths

---

### **Phase 5: Memory Prefetching** (Low Impact, Low Risk)

#### **5.1 Prefetch Next Ring Buffer Entry**

**Current:** Some prefetching in userspace ✅

**Enhancement:**
```c
// In BPF ring buffer consumer
void *next_entry = &ring_buffer[(index + 1) % size];
__builtin_prefetch(next_entry, 0, 3);  // Prefetch for read, high temporal locality
```

**Benefit:** ~5-10ns savings if next entry not in cache

---

#### **5.2 Prefetch Task Context**

**Current:** Task context lookup may cause cache miss

**Optimized:**
```c
// Prefetch task_ctx before accessing
struct task_ctx *tctx = bpf_task_storage_get(&task_ctx_stor, p, 0, 0);
if (tctx) {
    __builtin_prefetch(tctx, 0, 3);  // Prefetch for read
    // ... continue with other checks
    // ... then access tctx fields
}
```

**Benefit:** ~10-20ns savings if task_ctx not in cache

---

### **Phase 6: Instruction-Level Optimizations** (Low Impact, Low Risk)

#### **6.1 Avoid Function Call Overhead**

**Current:** Some inline functions may not be inlined

**Check:**
- Ensure `__always_inline` on all hot-path helpers
- Verify compiler actually inlines (check assembly)

---

#### **6.2 Minimize Stack Allocations**

**Current:** Review for stack allocations in hot paths

**Optimized:** Use BPF map or global variables for temporary storage

---

### **Phase 7: Single Writer Per Buffer** (High Impact, Medium Risk)

#### **7.1 Per-CPU Ring Buffers**

**Current:** Single ring buffer, multiple BPF CPUs may write

**Optimized:** Per-CPU ring buffers

**Benefit:**
- Eliminates contention between CPUs
- **~20-50ns** savings per write (no atomic operations needed)

**Implementation:**
```c
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, RING_BUFFER_SIZE);
    __type(key, u32);
    __type(value, struct input_event);
} input_ring_buffer_percpu SEC(".maps");
```

**Trade-off:**
- More complex userspace aggregation
- Slightly more memory usage

---

### **Phase 8: Zero-Copy Operations** ✅ **Already Implemented**

**Current:** Ring buffer provides zero-copy ✅

**Additional Opportunity:**
- Verify no unnecessary copies in userspace processing

---

## Performance Impact Summary

| Optimization | Impact | Risk | Effort | Priority |
|-------------|--------|------|--------|----------|
| **1.1: Atomic Relaxed** | High | Low | Low | 🔴 **HIGH** |
| **1.2: Acquire/Release** | Medium | Low | Medium | 🟡 **MEDIUM** |
| **2.1: Per-CPU Stats** | Medium | Low | Medium | 🟡 **MEDIUM** |
| **2.2: Cache Padding** | Low | Low | Low | 🟢 **LOW** |
| **3.1: NUMA Awareness** | High | Medium | High | 🟡 **MEDIUM** |
| **3.2: NUMA Ring Buffer** | Medium | Medium | High | 🟢 **LOW** |
| **5.1: Prefetch Buffer** | Low | Low | Low | 🟢 **LOW** |
| **5.2: Prefetch Context** | Low | Low | Low | 🟢 **LOW** |
| **7.1: Per-CPU Buffers** | High | Medium | High | 🟡 **MEDIUM** |

---

## Recommended Implementation Order

### **Immediate (High Priority):**
1. ✅ Replace `__sync_*` with `__atomic_*` relaxed (Statistics only)
2. ✅ Use acquire/release for ring buffer synchronization
3. ✅ Verify cache-line alignment of hot structures

### **Short-term (Medium Priority):**
4. ⚠️ Implement per-CPU statistics (eliminate false sharing)
5. ⚠️ Add NUMA-aware CPU selection

### **Long-term (Lower Priority):**
6. ⚠️ Per-CPU ring buffers (if contention detected)
7. ⚠️ Enhanced prefetching (if profiling shows cache misses)

---

## Code Review Checklist

- [ ] Statistics counters use `__ATOMIC_RELAXED`
- [ ] Ring buffer uses acquire/release semantics
- [ ] Per-CPU structures are cache-line aligned
- [ ] No false sharing in hot paths
- [ ] NUMA topology considered in CPU selection
- [ ] All hot-path functions are `__always_inline`
- [ ] Prefetching used for predictable memory access
- [ ] Single writer per buffer (or per-CPU buffers)

---

## Testing Strategy

### **Before/After Metrics:**
1. **Latency:** Measure `select_cpu()` with perf
2. **Cache Misses:** Use `perf stat -e cache-misses`
3. **Atomic Operations:** Count atomic instruction overhead
4. **False Sharing:** Use `perf c2c` to detect

### **Benchmark Scenarios:**
1. **High-FPS Input:** 1000+ FPS mouse movement
2. **Multi-CPU Contention:** All CPUs active
3. **NUMA Systems:** Multi-socket servers
4. **Cache Pressure:** Background tasks competing for cache

---

## Known Limitations

### **BPF Constraints:**
- No dynamic memory allocation
- Limited loop unrolling
- Restricted function calls
- No standard library

### **Trade-offs:**
- Per-CPU structures → More memory usage
- NUMA awareness → More complex code
- Prefetching → Potential cache pollution if misused

---

## References

- **LMAX Disruptor:** Lock-free ring buffer design
- **HFT Techniques:** Memory barrier optimization, cache-line awareness
- **Linux Kernel:** `Documentation/memory-barriers.txt`
- **BPF Performance:** Kernel BPF documentation

---

## Next Steps

1. **Profile current codebase** to identify bottlenecks
2. **Implement Phase 1** optimizations (atomic operations)
3. **Benchmark improvements** with perf tools
4. **Iterate** based on profiling results

---

**Expected Overall Improvement:** **50-200ns** reduction in hot-path latency with minimal code changes.

