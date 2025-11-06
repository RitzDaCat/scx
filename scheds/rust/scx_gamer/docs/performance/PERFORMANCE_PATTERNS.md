# Performance Patterns: Kernel, eBPF, and Rust

This document provides a tiered ranking of common code patterns in Linux Kernel (C), eBPF, and Rust, from fastest to slowest. The primary focus is on minimizing latency in performance-critical code.

---

## 1. Linux Kernel (C) Performance Patterns

**Constraints:** Direct hardware management, cache efficiency, and concurrency primitives.

| Tier | Patterns & Idioms | Rationale |
| :--- | :--- | :--- |
| **S (Fastest)** | <ul><li>Per-CPU variables</li><li>Lock-free algorithms (atomics)</li><li>Read-Copy-Update (RCU) for readers</li><li>`likely()` / `unlikely()` branch hints</li><li>Bitwise operations</li></ul> | <ul><li>**Per-CPU:** Eliminates lock contention entirely for per-core data.</li><li>**Lock-free:** Avoids sleeping and scheduler overhead associated with locks.</li><li>**RCU:** Allows readers to proceed with zero synchronization overhead.</li><li>**Branch Hints:** Optimizes instruction pipeline by avoiding mispredictions.</li><li>**Bitwise:** Single-cycle CPU instructions.</li></ul> |
| **A (Fast)** | <ul><li>Spinlocks</li><li>Careful cache line alignment</li><li>Pre-allocated memory pools</li><li>`inline` functions</li></ul> | <ul><li>**Spinlocks:** Busy-waits for a very short duration; efficient if contention is low and critical sections are small.</li><li>**Cache Alignment:** Prevents false sharing and ensures optimal cache utilization.</li><li>**Memory Pools:** Avoids the overhead of `kmalloc` in hot paths.</li><li>**Inlining:** Eliminates function call overhead.</li></ul> |
| **B (Moderate)** | <ul><li>Mutexes</li><li>Standard `kmalloc`/`kfree`</li><li>Pointer chasing across data structures</li></ul> | <ul><li>**Mutexes:** Can cause the process to sleep, invoking scheduler overhead. Slower than spinlocks but necessary for longer critical sections.</li><li>**kmalloc:** General-purpose allocator; has overhead and potential for contention.</li><li>**Pointer Chasing:** Can lead to CPU cache misses, stalling execution.</li></ul> |
| **F (Slowest)** | <ul><li>`vmalloc`</li><li>Any blocking I/O</li><li>Floating-point operations</li></ul> | <ul><li>**vmalloc:** Allocates virtually contiguous but physically non-contiguous memory, which is slow to map and access.</li><li>**Blocking I/O:** Puts the thread to sleep for an indeterminate amount of time.</li><li>**Floating Point:** Requires saving/restoring FPU registers, adding significant context-switch overhead in the kernel.</li></ul> |

### Kernel C Code Examples (Fastest to Slowest)

```c
// Example 1 (S-Tier): Per-CPU variables and atomics. Lock-free and highly efficient.
DEFINE_PER_CPU(unsigned long, packet_count) = 0;

void count_packet(void) {
    // this_cpu_inc operates on the current CPU's copy of packet_count.
    // No locking is required, eliminating contention.
    this_cpu_inc(packet_count);
}

// Example 2 (A-Tier): Spinlock for a short, contended critical section.
DEFINE_SPINLOCK(my_lock);
struct list_head my_list;

void add_to_list(struct list_head *new) {
    unsigned long flags;
    spin_lock_irqsave(&my_lock, flags);
    list_add(new, &my_list);
    spin_unlock_irqrestore(&my_lock, flags);
}

// Example 3 (B-Tier): Mutex for a critical section that may sleep.
DEFINE_MUTEX(my_mutex);
struct my_struct *g_ptr = NULL;

int update_global_ptr(void) {
    struct my_struct *new_ptr;

    // kmalloc can sleep, so a mutex is required, not a spinlock.
    new_ptr = kmalloc(sizeof(*new_ptr), GFP_KERNEL);
    if (!new_ptr)
        return -ENOMEM;

    mutex_lock(&my_mutex);
    if (g_ptr)
        kfree(g_ptr);
    g_ptr = new_ptr;
    mutex_unlock(&my_mutex);

    return 0;
}

// Example 4 (F-Tier): vmalloc for a large, virtually-contiguous buffer.
// Significantly slower than kmalloc due to non-contiguous physical pages.
void *buffer;
void setup_buffer(void) {
    // Request a large buffer that doesn't need to be physically contiguous.
    buffer = vmalloc(1024 * 1024); // 1 MiB
    if (!buffer) {
        // Handle error
    }
}
```

---

## 2. eBPF Performance Patterns

**Constraints:** BPF verifier, limited instruction set, and safe kernel interaction.

| Tier | Patterns & Idioms | Rationale |
| :--- | :--- | :--- |
| **S (Fastest)** | <ul><li>Operations on stack variables</li><li>`BPF_MAP_TYPE_ARRAY` & `PERCPU_ARRAY`</li><li>`__always_inline` functions</li><li>Direct packet/context data access</li></ul> | <ul><li>**Stack:** Register and stack access is the fastest possible; no indirection.</li><li>**Array Maps:** Simple array indexing is faster than hashing. Per-CPU arrays avoid locking.</li><li>**Inlining:** The BPF JIT can create a single, optimized block of machine code.</li><li>**Direct Access:** Reading data directly from a context (e.g., `__sk_buff`) is faster than using helpers.</li></ul> |
| **A (Fast)** | <ul><li>`BPF_MAP_TYPE_HASH` & `PERCPU_HASH`</li><li>Tail calls</li><li>Bounded loops (unrolled by compiler)</li></ul> | <ul><li>**Hash Maps:** Incur the cost of a hashing function but provide efficient key-value lookups.</li><li>**Tail Calls:** Minimal overhead for chaining BPF programs, but slightly slower than a fully inlined program.</li><li>**Loops:** The verifier requires loops to have a provable upper bound. The JIT can often unroll and optimize these.</li></ul> |
| **B (Moderate)** | <ul><li>`bpf_loop()` helper</li><li>BPF-to-BPF function calls (not inlined)</li></ul> | <ul><li>**bpf_loop():** A helper to create loops, which has more overhead than a compiler-unrolled loop.</li><li>**Function Calls:** Incur standard function call overhead, which can be significant in tight loops.</li></ul> |
| **F (Slowest)** | <ul><li>`bpf_probe_read_kernel()`</li><li>`bpf_trace_printk()` (for debugging)</li></ul> | <ul><li>**Probe Read:** A complex helper that must safely read arbitrary kernel memory, involving significant overhead and checks.</li><li>**printk:** A debugging helper that writes to the trace pipe; extremely slow and should never be used in production code.</li></ul> |

### eBPF Code Examples (Fastest to Slowest)

```c
// Assumes appropriate map definitions and BPF context (e.g., XDP).

// Example 1 (S-Tier): Per-CPU Array Map
// Fastest map type for counters. Lock-free, direct array access.
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, u64);
} packet_counts SEC(".maps");

SEC("xdp")
int count_packets(struct xdp_md *ctx) {
    u32 key = 0;
    u64 *count = bpf_map_lookup_elem(&packet_counts, &key);
    if (count) {
        __sync_fetch_and_add(count, 1);
    }
    return XDP_PASS;
}

// Example 2 (A-Tier): Hash Map
// Efficient key-value lookup, but slower than an array due to hashing.
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);
    __type(key, u32); // Source IP address
    __type(value, u64); // Packet count
} ip_counts SEC(".maps");

// ... inside program ...
// u32 src_ip = ... extract from packet ...
// u64 *count = bpf_map_lookup_elem(&ip_counts, &src_ip);
// ... update count ...

// Example 3 (F-Tier): Reading from kernel memory
// Very slow due to the safety checks required. Avoid in hot paths.
SEC("kprobe/some_func")
int trace_func(struct pt_regs *ctx) {
    char comm[16];
    bpf_get_current_comm(&comm, sizeof(comm));

    // Example of a slow, debug-only helper.
    bpf_trace_printk("Process %s called function", comm);
    return 0;
}
```

---

## 3. Rust Performance Patterns

**Constraints:** The borrow checker, with a key goal of leveraging zero-cost abstractions to generate code equivalent to optimal C/C++.

| Tier | Patterns & Idioms | Rationale |
| :--- | :--- | :--- |
| **S (Fastest)** | <ul><li>Stack-allocated variables (copy types)</li><li>Iterators over collections</li><li>Static dispatch (monomorphization)</li><li>Enums with `#[repr(u*)]`</li><li>`slice`/`array` access via iterators</li></ul> | <ul><li>**Stack:** No allocation or indirection overhead.</li><li>**Iterators:** Often compile down to the most optimal loop structure, eliminating bounds checks.</li><li>**Static Dispatch:** The compiler generates specialized code for each type at compile time, enabling inlining and other optimizations.</li><li>**Enums:** Pattern matching on simple enums compiles to a highly efficient jump table.</li></ul> |
| **A (Fast)** | <ul><li>Slices (`&[T]`)</li><li>`Vec<T>` with pre-allocated capacity</li><li>Bounds-checked slice/array access (`[i]`)</li><li>`Box<T>` for heap ownership</li></ul> | <ul><li>**Slices:** A lightweight view of contiguous memory.</li><li>**`Vec` with capacity:** Avoids repeated reallocations and copies during growth.</li><li>**Bounds Checking:** A single cheap integer comparison, but can prevent compiler vectorization in loops.</li><li>**`Box`:** A single, permanent heap allocation with no overhead on access.</li></ul> |
| **B (Moderate)** | <ul><li>Dynamic dispatch (`dyn Trait`)</li><li>`String` / `Vec<T>` without capacity</li><li>`Mutex`, `RwLock`</li></ul> | <ul><li>**Dynamic Dispatch:** Requires vtable lookups at runtime, preventing inlining.</li><li>**No Capacity:** Can lead to frequent reallocations, which are slow.</li><li>**Locks:** Introduce synchronization overhead and potential thread contention.</li></ul> |
| **F (Slowest)** | <ul><li>Reference counting (`Rc`, `Arc`)</li><li>Cloning heap-allocated types in loops</li><li>Channels (MPSC)</li></ul> | <ul><li>**Reference Counting:** Requires atomic read-modify-write operations on the count for each clone/drop, which can be slow and cause cache contention.</li><li>**Cloning:** A full heap allocation and deep copy of data. Extremely slow in hot paths.</li><li>**Channels:** Powerful for concurrency but involve locking and buffering, adding significant overhead compared to direct function calls.</li></ul> |

### Rust Code Examples (Fastest to Slowest)

```rust
use std::sync::{Arc, Mutex};

// Example 1 (S-Tier): Iterator over a slice.
// Compiles to optimal machine code, often with no bounds checks.
fn sum_array(data: &[i32]) -> i32 {
    data.iter().sum()
}

// Example 2 (A-Tier): Pre-allocating a Vec.
// Avoids reallocations inside the loop.
fn create_vec(size: usize) -> Vec<i32> {
    let mut vec = Vec::with_capacity(size);
    for i in 0..size {
        vec.push(i as i32);
    }
    vec
}

// Example 3 (B-Tier): Dynamic dispatch.
// The compiler doesn't know the concrete type of `drawable` at compile time,
// requiring a vtable lookup (pointer indirection) to call `.draw()`.
trait Drawable {
    fn draw(&self);
}

struct Circle;
impl Drawable for Circle { fn draw(&self) { /* ... */ } }

fn draw_item(drawable: &dyn Drawable) {
    drawable.draw(); // Slower than static dispatch
}

// Example 4 (F-Tier): Cloning an Arc in a loop.
// Each clone requires an atomic increment, which is slow and causes contention.
fn process_shared_data(data: Arc<Mutex<Vec<i32>>>) {
    for _ in 0..1000 {
        // Cloning the Arc is expensive in a tight loop.
        let data_clone = Arc::clone(&data);
        std::thread::spawn(move || {
            let mut guard = data_clone.lock().unwrap();
            guard.push(1);
        });
    }
}
```
