# scx_gamer v2.0 - Error Log and Resolutions

This document tracks issues encountered during development and their solutions.

---

## Issue #1: BPF Verifier Array Out-of-Bounds Access

**Date:** 2024-12-04

**Error Message:**
```
libbpf: prog 'gamer_running': BPF program load failed: -EACCES
invalid access to map value, value_size=192 off=2104 size=8
R0 max value is outside of the allowed memory range
```

**Location:** `main.bpf.c:198` in `gamer_running()` callback

**Root Cause:**

The BPF verifier performs static analysis to ensure memory safety. In this case:

```c
// In gamer_running()
if (tctx->boost_shift <= BOOST_MAX)
    STAT_INC(boost_histogram[tctx->boost_shift]);
```

The STAT_INC macro expands to read `boost_shift` again from memory:
```c
// Expanded from STAT_INC(boost_histogram[tctx->boost_shift])
__stats->boost_histogram[tctx->boost_shift]++;
```

**Why the verifier fails:**
1. First read: `tctx->boost_shift` is checked with `if (w1 > 0x7)`
2. Second read: Inside STAT_INC, `tctx->boost_shift` is read AGAIN from memory
3. The verifier doesn't track that these two reads return the same value
4. It sees: u8 (0-255) × 8 = offset 0-2040, but histogram only has 8 entries (64 bytes)
5. Calculated max offset 2104 > map value size 192 → **REJECTED**

**Solution:**

Read `boost_shift` once into a local variable BEFORE the bounds check:

```c
// WRONG - reads from memory twice
if (tctx->boost_shift <= BOOST_MAX)
    STAT_INC(boost_histogram[tctx->boost_shift]);

// CORRECT - reads once, verifier can track the bound
u8 boost = tctx->boost_shift;
if (boost <= BOOST_MAX)
    STAT_INC(boost_histogram[boost]);
```

**Files Changed:**
- `src/bpf/main.bpf.c` - Fixed `gamer_running()` to use local variable

**Status:** ✅ RESOLVED

**Fix Applied:**
```c
void BPF_STRUCT_OPS(gamer_running, struct task_struct *p)
{
    // ...
    u8 boost = BOOST_BACKGROUND;
    
    if (tctx) {
        tctx->last_run_ns = now;
        
        /* Read boost once - BPF verifier needs single read for bounds tracking */
        boost = tctx->boost_shift;
        
        /* Update boost histogram (bounded access) */
        if (boost <= BOOST_MAX)
            STAT_INC(boost_histogram[boost]);
    }
    
    if (cctx) {
        cctx->current_boost = boost;
    }
    // ...
}
```

**Lesson Learned:**

When working with BPF verifier and array bounds:
1. **Always read values into local variables before bounds checks**
2. The verifier tracks register values, not memory locations
3. Multiple reads from the same memory address are treated as potentially different values
4. This is a common BPF gotcha with array indexing

---

## Issue #2: Kernel Headers Version Mismatch

**Date:** 2024-12-04

**Error Message:**
```
[MISSING] linux-headers    for kernel 6.18.0-2-cachyos
```

**Root Cause:**

The `build.sh` dependency check was looking for headers matching the exact running kernel version. After a kernel update (but before reboot), the running kernel is older than the installed headers package.

- Running kernel: `6.18.0-2-cachyos`
- Installed headers: `linux-cachyos-headers 6.18.0-3`

**Solution:**

Modified `build.sh` to:
1. Check if headers package is installed (not just exact version match)
2. Show a warning instead of failing when versions mismatch
3. Recommend reboot but allow build to proceed

**Files Changed:**
- `build.sh` - Enhanced kernel headers detection logic

**Status:** ✅ RESOLVED

---

## Issue #3: Invalid DSQ ID in gamer_dispatch

**Date:** 2024-12-04

**Error Message:**
```
runtime error (invalid DSQ ID 0x0000000000000005)
scx_bpf_dsq_move_to_local+0xee/0xf0
bpf_prog_9ca1ff5e6ab220e7_gamer_dispatch+0x3d/0x4e
```

**Location:** `main.bpf.c:170` in `gamer_dispatch()` callback

**Root Cause:**

The dispatch function was incorrectly using `SCX_DSQ_LOCAL_ON | cpu`:

```c
// WRONG - SCX_DSQ_LOCAL_ON is for INSERTING, not CONSUMING
if (scx_bpf_dsq_move_to_local(SCX_DSQ_LOCAL_ON | cpu))
    return;
```

**Why it fails:**
1. `SCX_DSQ_LOCAL_ON` is a special flag used with `scx_bpf_dsq_insert()` to insert tasks into a CPU's local DSQ
2. `scx_bpf_dsq_move_to_local()` expects an **actual DSQ ID** to consume FROM
3. `SCX_DSQ_LOCAL_ON | 5` evaluates to just `5` (the flag bits are stripped), which is DSQ ID 5
4. DSQ 5 was never created → **invalid DSQ ID error**

**Important Distinction:**
- **Inserting tasks:** Use `scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, ...)` 
- **Consuming tasks:** Use `scx_bpf_dsq_move_to_local(SHARED_DSQ)` with your actual DSQ ID
- **Local DSQ:** Automatically consumed by the kernel - no action needed in dispatch()

**Solution:**

```c
void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    /* Local DSQ is automatically consumed by the kernel.
     * We only need to consume from our shared DSQ here. */
    scx_bpf_dsq_move_to_local(SHARED_DSQ);
}
```

**Status:** ✅ RESOLVED

**Lesson Learned:**

In sched_ext:
- `SCX_DSQ_LOCAL_ON | cpu` = Special flag for **insertion** that targets a CPU's local DSQ
- `scx_bpf_dsq_move_to_local(dsq_id)` = Consumes from DSQ with actual `dsq_id`
- The kernel automatically handles local DSQ consumption before calling `dispatch()`

---

## Issue #4: SCX Enum Constants Not Initialized (SCX_DSQ_LOCAL_ON = 0)

**Date:** 2024-12-04

**Error Message:**
```
runtime error (non-existent DSQ 0x9 for WoW.exe[124205])
```

**Location:** `main.rs` - BPF skeleton initialization

**Root Cause:**

The BPF code uses `SCX_DSQ_LOCAL_ON` which is a **weak volatile variable** that must be initialized by userspace:

```c
// In scx/enums.autogen.bpf.h
const volatile u64 __SCX_DSQ_LOCAL_ON __weak;  // Defaults to 0!
#define SCX_DSQ_LOCAL_ON __SCX_DSQ_LOCAL_ON
```

In `main.rs`, we were calling `skel_builder.open()` directly:
```rust
// WRONG - bypasses import_enums!
let mut open_skel = skel_builder.open(open_object)?;
```

This bypasses the `import_enums!` macro which is responsible for setting:
```rust
rodata.__SCX_DSQ_LOCAL_ON = scx_enums.SCX_DSQ_LOCAL_ON;  // Never called!
```

**Result:**
- `SCX_DSQ_LOCAL_ON = 0` (uninitialized)
- `SCX_DSQ_LOCAL_ON | 9` = `0 | 9` = `9`
- Kernel sees DSQ ID 9, which doesn't exist → **runtime error**

**Solution:**

Use `scx_ops_open!` macro which calls `import_enums!` internally:

```rust
// CORRECT - import_enums! initializes all __SCX_* constants
let open_opts: Option<&libbpf_rs::skel::OpenOpts> = None;
let mut open_skel = scx_ops_open!(skel_builder, open_object, gamer_ops, open_opts)?;
```

**Status:** ✅ RESOLVED

**Lesson Learned:**

In sched_ext Rust schedulers:
1. **Always use `scx_ops_open!`** instead of direct `.open()` call
2. **Always use `scx_ops_load!`** instead of direct `.load()` call
3. **Always use `scx_ops_attach!`** instead of direct `.attach()` call

These macros handle critical initialization including:
- `import_enums!()` - Initialize SCX enum constants
- `uei_set_size!()` - Set user exit info size
- hotplug_seq initialization
- Backward compatibility checks

---

## Issue #5: Game Freeze Due to Cross-CPU Local DSQ Starvation

**Date:** 2024-12-04

**Symptom:** Games would hard freeze shortly after starting the scheduler.

**Root Cause:**

The dispatch code was using `SCX_DSQ_LOCAL_ON | cpu` to insert tasks directly into a specific CPU's local dispatch queue:

```c
// PROBLEMATIC CODE - causes starvation
scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | safe_cpu, slice, enq_flags);
```

**Why this causes freezes:**

```
Timeline of a game freeze:

1. select_cpu() → chooses CPU 5 for game thread
2. enqueue() → inserts to CPU 5's LOCAL DSQ
3. CPU 5 is BUSY running another task
4. CPU 7 becomes IDLE, needs work
5. CPU 7 calls dispatch()
6. CPU 7 checks its own local DSQ → empty
7. CPU 7 checks SHARED_DSQ → game thread NOT THERE!
8. Game thread is STUCK on CPU 5's local DSQ
9. Game thread waits indefinitely for CPU 5
10. GAME FREEZE!
```

The fundamental problem: when you insert to a CPU's local DSQ, **only that specific CPU can consume it**. If that CPU is busy, the task waits. Other idle CPUs cannot help because they cannot see tasks on other CPUs' local DSQs.

**Solution:**

Always use `SHARED_DSQ` for task insertion, but kick the target CPU:

```c
// FIXED CODE - no starvation possible
scx_bpf_dsq_insert(p, SHARED_DSQ, slice, enq_flags);
STAT_INC(nr_shared_dispatch);

/* Kick the target CPU - if idle, it grabs task first */
if (safe_cpu >= 0) {
    kick_after_dispatch(safe_cpu, boost);
}
```

**Benefits of this approach:**
1. If target CPU is idle → it wakes up and grabs task immediately
2. If target CPU is busy → ANY idle CPU can pick up the task
3. No starvation possible
4. Tasks always find a CPU to run on

**Status:** ✅ RESOLVED

**Lesson Learned:**

In sched_ext, `SCX_DSQ_LOCAL_ON | cpu` is a **scheduling hint**, not a guarantee. Use it carefully:
- **Safe**: When you're 100% sure the target CPU will consume it
- **Unsafe**: When the target CPU might be busy (causes starvation)
- **Better pattern**: Use SHARED_DSQ + kick target CPU for low-latency without starvation risk

---
