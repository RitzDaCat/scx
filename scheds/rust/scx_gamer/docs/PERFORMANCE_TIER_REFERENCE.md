# Performance Tier Reference Guide

**Purpose:** Reference table for categorizing code patterns by performance characteristics (latency, throughput, overhead).

**Usage:** Use this guide during code reviews to identify optimization opportunities and ensure hot paths use Tier 0/1 patterns.

---

## Tier Classification System

| Tier | Latency Range | Use Case | Description |
|------|--------------|----------|-------------|
| **Tier 0** | < 10ns | Hot paths (every call) | Zero-allocation, compile-time optimized, register operations |
| **Tier 1** | 10-100ns | Hot paths (frequent) | Minimal overhead, stack-only, cache-friendly |
| **Tier 2** | 100ns-1μs | Warm paths (common) | Acceptable overhead, efficient algorithms |
| **Tier 3** | 1-10μs | Cool paths (occasional) | File I/O, small allocations, acceptable for rare events |
| **Tier 4** | 10-100μs | Startup/init | One-time costs, initialization operations |
| **Tier 5** | 100μs-1ms | Background tasks | Periodic operations, acceptable delays |
| **Tier 6** | 1-10ms | User-facing (async) | Network I/O, acceptable for non-blocking operations |
| **Tier 7** | 10-100ms | Batch processing | Heavy computation, acceptable for offline tasks |
| **Tier 8** | 100ms-1s | Long-running ops | Database queries, acceptable for user-initiated actions |
| **Tier 9** | 1-10s | Very slow ops | Complex analysis, acceptable only for rare operations |
| **Tier 10** | > 10s | Avoid entirely | Blocking operations, should be redesigned |

---

## Tier 0: Absolute Fastest (< 10ns)

**Characteristics:** Zero allocations, compile-time evaluated, register operations, branchless when possible.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Register operations** | 0.1-0.5ns | `a + b`, `a & b`, `a << 1` | CPU register arithmetic |
| **Compile-time constants** | 0ns | `#define MAX_CPUS 256` | Evaluated at compile time |
| **Macro expansions** | 0.1-1ns | `MAX(x, y)`, `MIN(x, y)` | Inline expansion, no function call |
| **Enum comparisons** | 0.5-1ns | `if (lane == INPUT_LANE_MOUSE)` | Single integer comparison |
| **Bitwise operations** | 0.1-0.5ns | `flags & MASK`, `val \| FLAG` | Direct CPU instructions |
| **Array indexing** | 0.5-2ns | `arr[i]` (cache hit) | Direct memory access, cache line hit |
| **Struct field access** | 0.5-2ns | `tctx->boost_shift` | Direct offset calculation |
| **Unlikely/likely hints** | 0ns | `if (unlikely(x))` | Compiler optimization hint |
| **Static assertions** | 0ns | `_Static_assert(sizeof(x) == 16)` | Compile-time check |
| **Type casts** | 0.1-0.5ns | `(u32)value` | No runtime cost |
| **Pointer arithmetic** | 0.5-1ns | `ptr + offset` | Direct calculation |
| **Sizeof operator** | 0ns | `sizeof(struct task_ctx)` | Compile-time constant |

**BPF-Specific Tier 0:**
| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Cached flag checks** | 1-2ns | `p->scx.flags & SCX_GAMER_FLAG_GPU` | Direct struct field read |
| **Per-CPU variable access** | 1-3ns | `__per_cpu_var[idx]` | No locking, cache-friendly |
| **Atomic relaxed ops** | 2-5ns | `__atomic_load_n(ptr, __ATOMIC_RELAXED)` | No memory barrier |
| **Branch prediction hints** | 0ns | `likely()`, `unlikely()` | Compiler optimization |

---

## Tier 1: Very Fast (10-100ns)

**Characteristics:** Minimal overhead, stack-only allocations, cache-friendly, efficient algorithms.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Stack-allocated buffers** | 0ns alloc | `u8 buf[32]` | Zero heap allocation cost |
| **Small struct copies** | 5-20ns | `struct cache c = {.x = 1};` | Stack copy, < 64 bytes |
| **String length (manual)** | 5-10ns | `while (s[i] != 0) i++` | Manual loop, no allocation |
| **Integer parsing** | 10-30ns | `strtol()` for small numbers | Efficient parsing |
| **Hash table lookup** | 10-50ns | `bpf_map_lookup_elem()` (hit) | Cache-friendly, O(1) average |
| **Per-CPU map lookup** | 10-30ns | `bpf_map_lookup_percpu_elem()` | No contention, cache-friendly |
| **Timestamp read** | 10-15ns | `scx_bpf_now()`, `bpf_ktime_get_ns()` | Kernel timestamp |
| **Atomic operations** | 5-20ns | `__atomic_fetch_add()` (relaxed) | No memory barrier overhead |
| **Memory prefetch** | 0ns | `__builtin_prefetch(ptr, 0, 1)` | Hides cache miss latency |
| **String comparison** | 10-50ns | `strncmp()` (short strings) | < 16 bytes, cache-friendly |
| **Split operations** | 10-20ns | `split_once(':')` | Single split, no allocation |
| **Conditional moves** | 1-3ns | `x = condition ? a : b` | Branchless when optimized |

**BPF-Specific Tier 1:**
| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Task context lookup** | 20-50ns | `try_lookup_task_ctx()` (hit) | BPF map lookup, cached |
| **CPU context lookup** | 10-30ns | `try_lookup_cpu_ctx()` (hit) | Per-CPU array, fast |
| **Ring buffer reserve** | 30-60ns | `bpf_ringbuf_reserve()` (success) | Per-CPU, lock-free |
| **Ring buffer submit** | 20-50ns | `bpf_ringbuf_submit()` | Per-CPU, lock-free |
| **CPUMask test** | 5-15ns | `bpf_cpumask_test_cpu()` | Bitmap test, cache-friendly |
| **DSQ operations** | 20-50ns | `scx_bpf_dsq_insert()` | Lock-free, per-CPU |

---

## Tier 2: Fast (100ns-1μs)

**Characteristics:** Acceptable overhead for common operations, efficient algorithms, minimal allocations.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Small heap allocation** | 100-300ns | `malloc(64)` | Small allocations, fast path |
| **String operations** | 50-200ns | `String::from_utf8_lossy()` | Small strings, < 256 bytes |
| **Vector push (small)** | 50-150ns | `vec.push(x)` (pre-allocated) | No reallocation |
| **HashMap lookup** | 50-200ns | `HashMap::get()` (hit) | Cache-friendly, O(1) average |
| **File read (small)** | 200-500ns | `read_file(256 bytes)` | Kernel page cache hit |
| **Network syscall** | 200-500ns | `sendmsg()` (local) | Kernel syscall overhead |
| **Mutex lock (uncontended)** | 50-200ns | `pthread_mutex_lock()` | Fast path, no contention |
| **Condition variable wait** | 100-500ns | `pthread_cond_wait()` (immediate) | No actual wait |
| **Epoll add** | 100-300ns | `epoll_ctl(EPOLL_CTL_ADD)` | Kernel syscall |
| **Timer setup** | 200-500ns | `timerfd_settime()` | Kernel timer creation |

**BPF-Specific Tier 2:**
| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Map update** | 100-300ns | `bpf_map_update_elem()` | Hash map insert/update |
| **Ring buffer (contended)** | 100-200ns | `bpf_ringbuf_reserve()` (contended) | Multiple writers |
| **Task storage get** | 100-250ns | `bpf_task_storage_get()` | Per-task storage lookup |
| **DSQ move** | 100-300ns | `scx_bpf_dsq_move_to_local()` | Queue manipulation |
| **CPU kick** | 100-200ns | `scx_bpf_kick_cpu()` | Inter-CPU signaling |

---

## Tier 3: Acceptable (1-10μs)

**Characteristics:** File I/O, small allocations, acceptable for occasional operations, not hot paths.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **File read (medium)** | 1-5μs | `read_file(4KB)` | Page cache hit, small file |
| **Heap allocation (medium)** | 500ns-2μs | `malloc(1024)` | Medium allocation |
| **String formatting** | 1-5μs | `format!("{}", value)` | Small format strings |
| **Vector reallocation** | 1-3μs | `vec.push()` (realloc) | Growth, copy existing |
| **HashMap insert** | 1-5μs | `HashMap::insert()` (rehash) | Hash collision, rehash |
| **Mutex lock (contended)** | 1-10μs | `pthread_mutex_lock()` | Contention, spin wait |
| **Epoll wait (immediate)** | 1-5μs | `epoll_wait()` (events ready) | No actual wait |
| **Timerfd read** | 1-5μs | `read(timerfd)` | Kernel timer read |
| **Process status read** | 1-5μs | `read_file("/proc/pid/status")` | Kernel procfs |

**BPF-Specific Tier 3:**
| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Ring buffer (full)** | 1-5μs | `bpf_ringbuf_reserve()` (retry) | Buffer full, retry |
| **Map lookup (miss)** | 1-5μs | `bpf_map_lookup_elem()` (miss) | Hash collision chain |
| **Task classification** | 1-5μs | `classify_task()` (full scan) | String matching, heuristics |
| **Device lookup** | 1-5μs | `device_profile_lookup()` | Hash map + string compare |

---

## Tier 4: Startup/Init (10-100μs)

**Characteristics:** One-time costs, initialization operations, acceptable at startup.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Large heap allocation** | 10-50μs | `malloc(1MB)` | Large allocation, page allocation |
| **File read (large)** | 10-100μs | `read_file(64KB)` | Large file, multiple pages |
| **Process scan** | 10-100μs | `readdir("/proc")` | Directory traversal |
| **Map initialization** | 10-50μs | `HashMap::with_capacity(1000)` | Pre-allocate buckets |
| **Thread creation** | 10-100μs | `thread::spawn()` | Kernel thread creation |
| **BPF program load** | 50-200μs | `bpf_prog_load()` | Kernel verification + JIT |
| **BPF map creation** | 10-50μs | `bpf_map_create()` | Kernel map allocation |

---

## Tier 5: Background Tasks (100μs-1ms)

**Characteristics:** Periodic operations, acceptable delays, background processing.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Full process scan** | 100μs-1ms | `scan_all_processes()` | Iterate /proc, classify |
| **Statistics aggregation** | 100μs-500μs | `aggregate_stats()` | Multiple map lookups |
| **Log file write** | 100μs-1ms | `write_log()` (buffered) | File I/O, buffered |
| **Configuration reload** | 100μs-1ms | `reload_config()` | File read + parse |
| **Memory cleanup** | 100μs-1ms | `gc_collect()` | Garbage collection |

---

## Tier 6: User-Facing Async (1-10ms)

**Characteristics:** Network I/O, acceptable for non-blocking operations, user-initiated.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Network I/O (local)** | 1-5ms | `recv()` (localhost) | Local network, low latency |
| **Network I/O (LAN)** | 1-10ms | `recv()` (LAN) | Local network, typical latency |
| **Database query (simple)** | 1-10ms | `SELECT * FROM table` | Simple query, indexed |
| **File write (large)** | 1-10ms | `write_file(1MB)` | Large file, disk I/O |
| **System call (slow)** | 1-5ms | `futex_wait()` | Kernel wait, context switch |

---

## Tier 7: Batch Processing (10-100ms)

**Characteristics:** Heavy computation, acceptable for offline tasks, batch operations.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Database query (complex)** | 10-100ms | `JOIN` queries, aggregations | Complex SQL, multiple tables |
| **File processing** | 10-100ms | `process_large_file()` | Parse, transform large files |
| **Network I/O (WAN)** | 10-100ms | `recv()` (internet) | Wide area network |
| **Compression** | 10-100ms | `compress_data()` | CPU-intensive compression |
| **Encryption** | 10-100ms | `encrypt_data()` | Cryptographic operations |

---

## Tier 8: Long-Running Ops (100ms-1s)

**Characteristics:** Database queries, acceptable for user-initiated actions, complex operations.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Database query (very complex)** | 100ms-1s | Complex analytics queries | Multiple joins, aggregations |
| **File system scan** | 100ms-1s | `find /large/directory` | Large directory traversal |
| **Image processing** | 100ms-1s | `process_image()` | Image manipulation, filters |
| **Video encoding** | 100ms-1s | `encode_frame()` | Per-frame encoding |

---

## Tier 9: Very Slow (1-10s)

**Characteristics:** Complex analysis, acceptable only for rare operations, offline processing.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Full system scan** | 1-10s | `scan_all_files()` | Entire filesystem scan |
| **Large data analysis** | 1-10s | `analyze_dataset()` | Complex algorithms |
| **Video encoding (full)** | 1-10s | `encode_video()` | Full video encoding |
| **Database migration** | 1-10s | `migrate_database()` | Schema changes, data migration |

---

## Tier 10: Avoid Entirely (> 10s)

**Characteristics:** Blocking operations, should be redesigned, unacceptable latency.

| Pattern | Latency | Example | Notes |
|---------|---------|---------|-------|
| **Blocking I/O** | > 10s | `read()` (blocking, no timeout) | Should use async I/O |
| **Synchronous network** | > 10s | `connect()` (blocking) | Should use async/epoll |
| **Long computation** | > 10s | `compute()` (single-threaded) | Should be parallelized |
| **Full backup** | > 10s | `backup_system()` | Should be background task |

---

## Anti-Patterns by Tier

### Tier 0 Anti-Patterns (Avoid in Hot Paths):
- ❌ Heap allocations (`malloc`, `new`, `Box::new`)
- ❌ String formatting (`format!`, `sprintf`)
- ❌ Function calls (use inline macros)
- ❌ Dynamic dispatch (use static dispatch)
- ❌ Exception handling (use error codes)

### Tier 1 Anti-Patterns (Avoid in Frequent Paths):
- ❌ Large allocations (> 1KB)
- ❌ File I/O operations
- ❌ Network syscalls
- ❌ Mutex locks (use lock-free structures)
- ❌ String cloning (use references)

### Tier 2 Anti-Patterns (Avoid in Common Paths):
- ❌ Large file reads (> 4KB)
- ❌ Complex string operations
- ❌ Vector reallocations
- ❌ HashMap rehashing
- ❌ Contended mutexes

---

## BPF-Specific Performance Guidelines

### Tier 0 BPF Patterns:
- ✅ Cached flag checks (`p->scx.flags & MASK`)
- ✅ Per-CPU variable access
- ✅ Atomic relaxed operations
- ✅ Branch prediction hints (`likely()`, `unlikely()`)
- ✅ Compile-time constants

### Tier 1 BPF Patterns:
- ✅ Task context lookup (cached)
- ✅ CPU context lookup (per-CPU)
- ✅ Ring buffer operations (per-CPU)
- ✅ CPUMask operations
- ✅ DSQ operations (lock-free)

### Tier 2 BPF Patterns:
- ✅ Map updates (hash map)
- ✅ Task storage operations
- ✅ Ring buffer (contended)
- ✅ CPU kick operations

### Tier 3+ BPF Patterns (Avoid in Hot Paths):
- ❌ Full task classification
- ❌ Device lookup (hash map + string compare)
- ❌ Ring buffer retries (buffer full)
- ❌ Map lookup misses (hash collision chain)

---

## Code Review Checklist

When reviewing code, ask:

1. **Is this in a hot path?** (called > 1000x/sec)
   - Must be Tier 0/1
   - No allocations, no syscalls, no file I/O

2. **Is this in a warm path?** (called > 100x/sec)
   - Should be Tier 1/2
   - Minimal allocations, efficient algorithms

3. **Is this in a cool path?** (called < 100x/sec)
   - Can be Tier 2/3
   - Acceptable overhead for occasional operations

4. **Is this startup/init?** (called once)
   - Can be Tier 4/5
   - One-time cost acceptable

5. **Is this background/async?** (non-blocking)
   - Can be Tier 5/6
   - Acceptable for non-critical paths

---

## Examples: Before/After Optimization

### Example 1: Timestamp Reuse
```c
// TIER 2: Multiple timestamp calls
void function() {
    u64 now1 = scx_bpf_now();  // 10-15ns
    // ... code ...
    u64 now2 = scx_bpf_now();  // 10-15ns (redundant)
    // ... code ...
    u64 now3 = scx_bpf_now();  // 10-15ns (redundant)
}

// TIER 1: Single timestamp call
void function() {
    u64 now = scx_bpf_now();  // 10-15ns (once)
    // ... code uses 'now' ...
}
```

### Example 2: String Building
```rust
// TIER 3: Heap allocation
let path = format!("/proc/{}/status", pid);  // 1-5μs

// TIER 1: Stack buffer
let mut buf = [0u8; 32];
let path = build_proc_path(pid, b"status", &mut buf);  // 0ns alloc
```

### Example 3: Map Lookup Caching
```c
// TIER 2: Multiple lookups
struct task_ctx *tctx1 = try_lookup_task_ctx(p);  // 20-50ns
struct task_ctx *tctx2 = try_lookup_task_ctx(p);  // 20-50ns (redundant)

// TIER 1: Single lookup, reuse
struct task_ctx *tctx = try_lookup_task_ctx(p);  // 20-50ns (once)
// ... reuse tctx ...
```

---

## Summary

- **Tier 0/1:** Use in hot paths (scheduler, input handlers, GPU threads)
- **Tier 2/3:** Use in warm paths (common operations, occasional events)
- **Tier 4/5:** Use for startup/init and background tasks
- **Tier 6+:** Avoid in performance-critical code

**Golden Rule:** If it's called > 1000x/sec, it must be Tier 0/1. If it's called > 100x/sec, it should be Tier 1/2.

