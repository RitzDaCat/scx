# Affinity Override System: Kernel vs Userspace Detection Analysis

**Date:** 2025-11-09  
**Issue:** Need to distinguish kernel-set affinities (must respect) from userspace-set affinities (should override)

---

## Current Implementation

### What We Currently Do ✅

1. **Filter Kernel Threads:**
   ```c
   if (is_kthread(p)) {
       return 0;  // Ignore kernel threads
   }
   ```
   - ✅ **Correct:** Kernel threads have affinities set for correctness (per-CPU kworkers, etc.)
   - ✅ **Safe:** We never override kernel thread affinities

2. **Detect ALL Affinity Changes:**
   - Hooks `set_cpus_allowed_ptr()` which is called for **ALL** affinity changes
   - This includes:
     - ✅ Userspace syscalls (`sched_setaffinity()`)
     - ⚠️ Kernel internal changes (NUMA balancing, thermal throttling, etc.)

3. **Override to Full Mask:**
   - Userspace resets ALL detected custom affinities to full CPU mask
   - Kernel respects `migrate_disable` internally (safety)

### The Problem ⚠️

**Current behavior:** We override **ALL** custom affinities for userspace tasks, including:
- ✅ Userspace-set (Unreal Engine) → **Should override** ✓
- ⚠️ Kernel-set (NUMA balancing, thermal) → **Should NOT override** ✗

**Example scenario:**
1. Kernel NUMA balancer sets process to local NUMA node (CPUs 0-7)
2. Our hook detects this and sends event
3. We override back to full mask (CPUs 0-15)
4. Kernel NUMA balancer sets it again → **Fight between kernel and userspace**

---

## Why This Matters

### Kernel-Set Affinities (Must Respect)

| Source | Purpose | Example |
|--------|---------|---------|
| **NUMA Balancing** | Keep process on local NUMA node | Process on node 0, CPUs 0-7 |
| **Thermal Throttling** | Move away from hot CPUs | CPUs 8-15 (cooler cores) |
| **CPU Hotplug** | Migrate off CPU being removed | Exclude CPU 15 |
| **Cgroup CPU Sets** | Enforce cgroup restrictions | CPUs 0-3 (cgroup limit) |

**These are correctness/safety requirements** - overriding them can cause:
- Performance degradation (NUMA)
- Thermal issues (throttling)
- System instability (hotplug)

### Userspace-Set Affinities (Should Override)

| Source | Purpose | Example |
|--------|---------|---------|
| **Unreal Engine** | Pin GPU thread to single core | CPU 0 only |
| **Custom Applications** | Manual CPU pinning | CPUs 4-7 |
| **Legacy Code** | Old affinity logic | Single CPU |

**These are optimization attempts** - overriding them allows scheduler to optimize.

---

## Solution: Detect Caller Context

### Option 1: Hook Syscall Entry Point (Preferred) ✅

**Idea:** Hook `sched_setaffinity` syscall entry instead of `set_cpus_allowed_ptr()`

**Pros:**
- ✅ Only catches userspace calls
- ✅ Ignores kernel internal changes
- ✅ Perfect separation

**Cons:**
- ❌ We already tried this - failed with EACCES (verifier blocks `bpf_task_from_pid()`)
- ❌ Requires different approach

### Option 2: Check Call Stack (Complex) ⚠️

**Idea:** Use BPF stack trace to detect if caller is kernel or userspace

**Pros:**
- ✅ Can distinguish caller context
- ✅ Works with current hook

**Cons:**
- ❌ Complex (requires stack unwinding)
- ❌ Overhead (stack trace collection)
- ❌ May not work reliably (inlining, tail calls)

### Option 3: Heuristic-Based Filtering (Pragmatic) ✅

**Idea:** Use heuristics to guess if affinity change is kernel-set

**Heuristics:**
1. **NUMA Node Alignment:** If mask aligns with NUMA node boundaries → likely kernel
2. **Thermal Pattern:** If mask excludes hot CPUs → likely kernel
3. **Cgroup Pattern:** If mask matches cgroup CPU set → likely kernel
4. **Single CPU:** If mask is single CPU → likely userspace (Unreal Engine pattern)

**Pros:**
- ✅ Simple to implement
- ✅ Low overhead
- ✅ Works with current hook

**Cons:**
- ⚠️ Not 100% accurate (heuristics can be wrong)
- ⚠️ May miss some kernel-set affinities

### Option 4: Whitelist Approach (Conservative) ✅

**Idea:** Only override if we're confident it's userspace

**Strategy:**
- Override if: Single CPU (common userspace pattern)
- Don't override if: Multiple CPUs, NUMA-aligned, or matches cgroup

**Pros:**
- ✅ Safe (conservative)
- ✅ Catches common case (Unreal Engine single-core pinning)
- ✅ Low risk of breaking kernel logic

**Cons:**
- ⚠️ May miss some userspace affinities (multi-CPU pinning)

---

## Recommended Solution: Hybrid Approach

### Phase 1: Immediate (Current + Whitelist)

**Keep current implementation but add conservative filtering:**

```c
// Only override if mask is single CPU (common userspace pattern)
// This catches Unreal Engine's single-core GPU thread pinning
if (nr_cpus_allowed == 1) {
    // Override - definitely userspace (kernel rarely pins to single CPU)
    send_event_to_userspace();
} else {
    // Don't override - could be kernel (NUMA, thermal, etc.)
    return 0;
}
```

**Rationale:**
- ✅ Catches primary use case (Unreal Engine single-core pinning)
- ✅ Safe (doesn't interfere with kernel NUMA/thermal)
- ✅ Simple (minimal code change)

### Phase 2: Enhanced Detection (Future)

**Add syscall entry hook if verifier allows:**

```c
// Try to hook syscall entry point
SEC("tracepoint/syscalls/sys_enter_sched_setaffinity")
int BPF_PROG(sched_setaffinity_enter, ...) {
    // This only catches userspace calls
    // Store PID in per-CPU map for later lookup
}
```

**Then in `set_cpus_allowed_ptr()`:**
```c
// Check if this change originated from userspace syscall
if (is_from_userspace_syscall(p->tgid)) {
    // Override
} else {
    // Don't override (kernel-set)
}
```

---

## Implementation Plan

### Step 1: Add Single-CPU Filter (Quick Win)

**Modify BPF hook to only override single-CPU affinities:**

```c
/* Compute target nr_cpus_allowed from requested mask */
nr_cpus_allowed = cpumask_weight_upto_256(new_mask);
if (!is_custom_affinity(nr_cpus_allowed, nr_cpu_ids))
    return 0; /* Full mask - nothing to do */

/* CONSERVATIVE FILTER: Only override single-CPU affinities
 * This catches common userspace pattern (Unreal Engine GPU thread pinning)
 * while avoiding interference with kernel NUMA/thermal balancing (multi-CPU) */
if (nr_cpus_allowed != 1) {
    /* Multi-CPU affinity - likely kernel-set (NUMA, thermal, cgroup)
     * Don't override to avoid fighting kernel logic */
    return 0;
}

/* Single-CPU affinity - likely userspace (Unreal Engine pattern)
 * Safe to override */
```

### Step 2: Add Statistics

**Track what we're filtering:**

```c
volatile u64 affinity_single_cpu_overrides;  /* Single-CPU overrides (userspace) */
volatile u64 affinity_multi_cpu_filtered;    /* Multi-CPU filtered (likely kernel) */
```

### Step 3: Test and Validate

**Test scenarios:**
1. ✅ Unreal Engine single-core pinning → Should override
2. ✅ Kernel NUMA balancing (multi-CPU) → Should NOT override
3. ✅ Userspace multi-CPU pinning → Currently won't override (acceptable trade-off)

---

## Current Status

**What Works:**
- ✅ Kernel thread filtering (correct)
- ✅ Detection of custom affinities
- ✅ Override mechanism (userspace syscall)

**What's Missing:**
- ⚠️ Distinction between kernel-set vs userspace-set affinities
- ⚠️ May override kernel NUMA/thermal affinities (undesirable)

**Recommendation:**
- **Immediate:** Add single-CPU filter (catches Unreal Engine case)
- **Future:** Investigate syscall entry hook if verifier allows

---

**Analysis Complete:** 2025-11-09

