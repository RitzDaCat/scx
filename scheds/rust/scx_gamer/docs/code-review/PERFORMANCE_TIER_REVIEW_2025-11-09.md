# Performance Tier Review: Recent Affinity Override Changes

**Date:** 2025-11-09  
**Reviewer:** AI Assistant  
**Scope:** Affinity override system implementation (kprobe hook + userspace)

---

## Performance Tier Definitions

| Tier | Overhead | Use Case | Example |
|------|----------|----------|---------|
| **Tier 0** | 0-10ns | Hot path, compile-time eliminated | Flag checks, simple comparisons |
| **Tier 1** | 10-100ns | Hot path, minimal overhead | Per-CPU array access, atomic ops |
| **Tier 2** | 100ns-1µs | Warm path, acceptable overhead | Map lookups, simple syscalls |
| **Tier 3** | 1-10µs | Cold path, moderate overhead | Shared map contention, I/O |
| **Tier 4+** | 10µs+ | Non-critical, high overhead | File I/O, network, allocations |

---

## Change 1: `cpumask_weight_upto_256()` Function

**Location:** `src/bpf/main.bpf.c:5308-5320`

### Current Implementation

```c
static __always_inline u32 cpumask_weight_upto_256(const struct cpumask *mask)
{
	u32 weight = 0;
	u32 nlongs = (nr_cpu_ids + 63) / 64;
	#pragma unroll
	for (int i = 0; i < 4; i++) {
		if ((u32)i >= nlongs)
			break;
		u64 word = BPF_CORE_READ(mask, bits[i]);
		weight += (u32)__builtin_popcountll(word);
	}
	return weight;
}
```

### Performance Analysis

**Operations:**
- `BPF_CORE_READ`: ~5-10ns per read (memory access)
- `__builtin_popcountll`: Hardware instruction (~1-2ns per u64)
- Loop with `#pragma unroll`: Compiler unrolls (no loop overhead)
- Early exit: `if (i >= nlongs) break` (branch prediction friendly)

**Estimated Overhead:**
- 4 memory reads: ~20-40ns
- 4 popcount operations: ~4-8ns
- Loop overhead: ~0ns (unrolled)
- **Total: ~24-48ns**

### Tier Assessment: **TIER 1** ✅

**Justification:**
- ✅ Runs during syscall (NOT scheduling hot path)
- ✅ Affinity changes are rare (1-10 events/min)
- ✅ Uses hardware popcount (optimal)
- ✅ Loop unrolled (no iteration overhead)
- ✅ Early exit for systems with <256 CPUs

**Recommendation:** Add tier comment:

```c
/* TIER 1: ~24-48ns (4 memory reads + 4 hardware popcounts)
 * Acceptable overhead since affinity changes are rare and outside hot path */
```

---

## Change 2: Kprobe Hook on `set_cpus_allowed_ptr`

**Location:** `src/bpf/main.bpf.c:5322-5362`

### Current Implementation

```c
SEC("kprobe/set_cpus_allowed_ptr")
int BPF_PROG(affinity_detect_set_cpus_allowed_ptr, struct task_struct *p,
	    const struct cpumask *new_mask)
{
	// Atomic counter increment
	__atomic_fetch_add(&affinity_setaffinity_count, 1, __ATOMIC_RELAXED);
	
	// Early exit: kernel thread check
	if (is_kthread(p)) {
		__atomic_fetch_add(&affinity_kthread_filtered, 1, __ATOMIC_RELAXED);
		return 0;
	}
	
	// Compute mask weight
	nr_cpus_allowed = cpumask_weight_upto_256(new_mask);
	
	// Early exit: full mask check
	if (!is_custom_affinity(nr_cpus_allowed, nr_cpu_ids))
		return 0;
	
	// Ring buffer reserve/submit (~30-60ns)
	evt = bpf_ringbuf_reserve(&affinity_events, sizeof(*evt), 0);
	// ... populate event ...
	bpf_ringbuf_submit(evt, 0);
}
```

### Performance Analysis

**Fast Path (Kernel Thread):**
- Atomic increment: ~5-10ns
- `is_kthread()` check: ~1-2ns (flag read)
- Atomic increment: ~5-10ns
- **Total: ~11-22ns** → **TIER 1** ✅

**Fast Path (Full Mask):**
- Atomic increment: ~5-10ns
- `is_kthread()` check: ~1-2ns
- `cpumask_weight_upto_256()`: ~24-48ns
- `is_custom_affinity()`: ~1ns (comparison)
- **Total: ~31-61ns** → **TIER 1** ✅

**Slow Path (Custom Affinity):**
- Fast path overhead: ~31-61ns
- Ring buffer reserve: ~30-60ns
- `BPF_CORE_READ(p, tgid)`: ~5-10ns
- `bpf_ktime_get_ns()`: ~10-20ns
- `bpf_probe_read_kernel_str()`: ~50-100ns (string copy)
- Ring buffer submit: ~20-50ns
- **Total: ~146-281ns** → **TIER 2** ✅

### Tier Assessment: **TIER 1-2** ✅

**Justification:**
- ✅ **NOT in scheduling hot path** (runs during syscall)
- ✅ Early exits for common cases (kthreads, full masks)
- ✅ Ring buffer operations are optimized (zero-copy)
- ✅ Affinity changes are rare (1-10/min)

**Recommendation:** Add tier comment:

```c
/* TIER 1-2: ~11-281ns depending on path
 * Fast path (kthread/full mask): ~11-61ns (Tier 1)
 * Slow path (custom affinity): ~146-281ns (Tier 2)
 * Acceptable since affinity changes are rare and outside hot path */
```

---

## Change 3: First-Event Logging in Userspace

**Location:** `src/affinity_override.rs:264-278`

### Current Implementation

```rust
let prev = stats.events_received.load(Ordering::Relaxed);
if prev == 1 {
    // Build comm string cheaply for the first event only
    let comm_len = {
        let mut len = 0;
        while len < event.comm.len() && event.comm[len] != 0 { len += 1; }
        len
    };
    let comm_slice = &event.comm[..comm_len];
    let comm_str = String::from_utf8_lossy(comm_slice);
    info!(
        "Affinity override: first event processed (pid={} comm='{}' nr_cpus={})",
        event.pid, comm_str, event.nr_cpus_allowed
    );
}
```

### Performance Analysis

**Overhead:**
- Atomic load: ~5-10ns
- Comparison: ~1ns
- String length calculation: ~10-50ns (manual loop)
- `String::from_utf8_lossy()`: **~100-500ns** (heap allocation + UTF-8 validation)
- `info!()` macro: **~1-10µs** (formatting + I/O to stderr)

**Total: ~1-10µs** (only on first event)

### Tier Assessment: **TIER 3** ⚠️

**Issues:**
- ❌ **Heap allocation** (`String::from_utf8_lossy()`)
- ❌ **I/O operation** (`info!()` writes to stderr)
- ⚠️ Only runs once (acceptable, but not optimal)

**Recommendation:** Optimize to **TIER 2**

**Option A: Use stack-allocated buffer (no heap)**

```rust
if prev == 1 {
    // TIER 2: Stack-allocated buffer, no heap allocation
    let mut comm_buf = [0u8; 16];
    let comm_len = {
        let mut len = 0;
        while len < event.comm.len() && event.comm[len] != 0 {
            comm_buf[len] = event.comm[len];
            len += 1;
        }
        len
    };
    // Use Cow<str> to avoid allocation
    let comm_str = std::str::from_utf8(&comm_buf[..comm_len])
        .unwrap_or("<invalid>");
    info!(
        "Affinity override: first event processed (pid={} comm='{}' nr_cpus={})",
        event.pid, comm_str, event.nr_cpus_allowed
    );
}
```

**Option B: Conditional compilation (zero overhead in release)**

```rust
#[cfg(debug_assertions)]
if prev == 1 {
    // ... logging code ...
}
```

**Recommendation:** Use **Option A** (stack buffer) for production logging, or **Option B** (conditional) if logging is debug-only.

---

## Change 4: Epoll Timeout Reduction

**Location:** `src/main.rs:2354`

### Current Implementation

```rust
// PERF: Use 100ms timeout to improve shutdown responsiveness while keeping CPU overhead low
const EPOLL_TIMEOUT_MS: u16 = 100; // 100ms timeout for faster Ctrl-C handling
```

**Previous:** 1000ms timeout  
**Current:** 100ms timeout

### Performance Analysis

**Impact:**
- **More frequent wakeups:** 10x increase (1Hz → 10Hz)
- **Wakeup overhead:** ~1-5µs per epoll_wait() call
- **Additional CPU overhead:** ~10-50µs/sec (10 wakeups/sec × 1-5µs)

**Context:**
- Runs in **event loop thread** (pinned to housekeeping CPU)
- **NOT in scheduling hot path**
- Only affects idle loop (when no events)

### Tier Assessment: **TIER 2** ✅

**Justification:**
- ✅ Acceptable overhead (~10-50µs/sec)
- ✅ Improves UX (faster Ctrl-C response)
- ✅ Not in hot path (event loop only)
- ✅ Housekeeping CPU (isolated from game workload)

**Recommendation:** Keep as-is. Consider documenting trade-off:

```rust
// TIER 2: 100ms timeout balances shutdown responsiveness (~100ms) vs CPU overhead (~10-50µs/sec)
// Previous: 1000ms timeout (lower overhead, slower shutdown)
// Current: 100ms timeout (higher overhead, faster shutdown)
const EPOLL_TIMEOUT_MS: u16 = 100;
```

---

## Summary of Recommendations

| Change | Current Tier | Target Tier | Action Required |
|--------|--------------|-------------|-----------------|
| `cpumask_weight_upto_256()` | **TIER 1** ✅ | **TIER 1** | Add tier comment |
| Kprobe hook | **TIER 1-2** ✅ | **TIER 1-2** | Add tier comment |
| First-event logging | **TIER 3** ⚠️ | **TIER 2** | Optimize (stack buffer or conditional) |
| Epoll timeout | **TIER 2** ✅ | **TIER 2** | Add tier comment |

---

## Critical Path Analysis

**Question:** Do these changes impact scheduling hot paths?

**Answer:** **NO** ✅

1. **Affinity override hook:** Runs during `sched_setaffinity()` syscall (cold path)
2. **Userspace override:** Runs in separate thread (not in scheduler)
3. **Epoll timeout:** Only affects event loop (not scheduling decisions)

**Conclusion:** All changes are **outside hot paths** and have **minimal impact** on gaming performance.

---

## Final Verdict

**Overall Assessment:** ✅ **Acceptable** (Tier 1-2 for critical paths)

**Issues Found:**
- ⚠️ First-event logging uses heap allocation (Tier 3)
- ✅ All other code is Tier 1-2 (optimal)

**Recommendations:**
1. ✅ Add tier comments to BPF code
2. ⚠️ Optimize first-event logging (stack buffer or conditional)
3. ✅ Document epoll timeout trade-off

**Priority:** Low (logging optimization is cosmetic, not performance-critical)

---

**Review Complete:** 2025-11-09

