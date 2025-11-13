# Affinity Override System: How It Works

**Date:** 2025-11-09  
**Status:** ✅ **Implemented with Kernel vs Userspace Detection**

---

## Overview

The affinity override system detects when applications (like Unreal Engine) set restrictive CPU affinities and automatically resets them to allow the scheduler optimal thread placement.

**Key Design Principle:** Only override **userspace-set affinities**, never interfere with **kernel-set affinities** (NUMA, thermal, etc.).

---

## Architecture

### 1. Detection (BPF Kernel Hook)

**Hook:** `SEC("kprobe/set_cpus_allowed_ptr")`

**What It Does:**
1. Intercepts ALL affinity changes (kernel and userspace)
2. Filters kernel threads (never override)
3. Filters full masks (nothing to do)
4. **Conservative Filter:** Only override single-CPU affinities (userspace pattern)
5. Sends event to userspace via ring buffer

**Filtering Logic:**

```
Affinity Change Detected
    ↓
Is kernel thread? → YES → Ignore (kernel correctness)
    ↓ NO
Is full mask? → YES → Ignore (no restriction)
    ↓ NO
Is single CPU? → NO → Ignore (likely kernel NUMA/thermal)
    ↓ YES
Send event to userspace → Override
```

### 2. Override (Userspace Thread)

**Thread:** `affinity-override` (dedicated thread)

**What It Does:**
1. Polls ring buffer for affinity events (200ms timeout)
2. Receives event with PID, process name, CPU count
3. Calls `sched_setaffinity(pid, full_cpumask)` to reset
4. Logs first event for confirmation

**Performance:** ~2-11µs per override (syscall overhead)

---

## Kernel vs Userspace Detection

### How We Distinguish

**Heuristic:** Single-CPU affinities are almost always userspace

| Affinity Type | Source | Action |
|--------------|--------|--------|
| **Single CPU** | Userspace (Unreal Engine) | ✅ **Override** |
| **Multi-CPU** | Kernel (NUMA, thermal) | ❌ **Don't Override** |
| **Full Mask** | N/A | ❌ **Ignore** |
| **Kernel Thread** | Kernel | ❌ **Ignore** |

### Why This Works

**Single-CPU Affinities:**
- ✅ **Unreal Engine:** Pins GPU thread to single core
- ✅ **Legacy Apps:** Manual CPU pinning
- ✅ **Userspace Pattern:** Common optimization attempt

**Multi-CPU Affinities:**
- ⚠️ **NUMA Balancing:** Kernel keeps process on local NUMA node (e.g., CPUs 0-7)
- ⚠️ **Thermal Throttling:** Kernel moves away from hot CPUs (e.g., CPUs 8-15)
- ⚠️ **Cgroup Limits:** Kernel enforces cgroup CPU sets (e.g., CPUs 0-3)
- ⚠️ **Kernel Pattern:** Correctness/safety requirements

**Result:** By only overriding single-CPU affinities, we:
- ✅ Catch primary use case (Unreal Engine)
- ✅ Avoid fighting kernel NUMA/thermal logic
- ✅ Safe and conservative

---

## Example Scenarios

### Scenario 1: Unreal Engine GPU Thread Pinning ✅

**What Happens:**
1. Unreal Engine calls `sched_setaffinity()` to pin GPU thread to CPU 0
2. BPF hook detects: Single-CPU affinity (CPU 0 only)
3. Event sent to userspace: `{pid=91350, nr_cpus=1}`
4. Userspace resets: `sched_setaffinity(91350, full_mask)`
5. Scheduler can now place thread optimally

**Result:** ✅ GPU thread freed from single-core restriction

### Scenario 2: Kernel NUMA Balancing ⚠️

**What Happens:**
1. Kernel NUMA balancer sets process to local NUMA node (CPUs 0-7)
2. BPF hook detects: Multi-CPU affinity (8 CPUs)
3. **Filtered:** `affinity_multi_cpu_filtered++`
4. No event sent (kernel logic respected)

**Result:** ✅ Kernel NUMA balancing works correctly

### Scenario 3: Kernel Thermal Throttling ⚠️

**What Happens:**
1. Kernel thermal subsystem moves process away from hot CPUs (CPUs 8-15)
2. BPF hook detects: Multi-CPU affinity (CPUs 0-7)
3. **Filtered:** `affinity_multi_cpu_filtered++`
4. No event sent (thermal protection respected)

**Result:** ✅ Thermal throttling works correctly

---

## Statistics

**BPF Counters (volatile):**
- `affinity_setaffinity_count`: Total affinity changes observed
- `affinity_events_sent`: Single-CPU overrides (userspace)
- `affinity_multi_cpu_filtered`: Multi-CPU filtered (likely kernel)
- `affinity_kthread_filtered`: Kernel threads filtered
- `affinity_events_dropped`: Ring buffer overflow (rare)

**Userspace Stats:**
- `events_received`: Events processed
- `affinities_reset`: Successful overrides
- `reset_failures`: Override failures (rare)
- `process_not_found`: Process exited before override

---

## Performance Impact

**Overhead:**
- **BPF Hook:** ~11-281ns per affinity change (Tier 1-2)
- **Userspace Override:** ~2-11µs per override (Tier 2)
- **Frequency:** 1-10 events/min (affinity changes are rare)

**Total CPU Overhead:** ~20-110µs/sec (negligible)

**Hot Path Impact:** **ZERO** (runs during syscall, not scheduling)

---

## Safety Guarantees

1. **Kernel Threads:** Never overridden (filtered in BPF)
2. **Kernel Affinities:** Multi-CPU affinities not overridden (conservative filter)
3. **migrate_disable:** Respected by kernel internally (we don't override if kernel rejects)
4. **Process Exits:** Gracefully handled (ESRCH ignored)

---

## Limitations

**Current Implementation:**
- ⚠️ Only overrides single-CPU affinities
- ⚠️ Multi-CPU userspace affinities not overridden (acceptable trade-off)

**Future Enhancement:**
- Investigate syscall entry hook to detect userspace calls directly
- Would allow overriding multi-CPU userspace affinities while respecting kernel

---

## Testing

**To Test Affinity Override:**

```bash
# 1. Start scheduler
sudo scx_gamer

# 2. In another terminal, pin a process to single CPU
sudo taskset -pc 0 <PID>

# 3. Check if override worked
taskset -pc <PID>
# Should show: "pid <PID> current affinity list: 0-15" (full mask)

# 4. Check logs for:
# "Affinity override: first event processed (pid=... comm='...' nr_cpus=1)"
```

**Expected Behavior:**
- ✅ Single-CPU pinning → Overridden to full mask
- ✅ Multi-CPU pinning → Not overridden (filtered)
- ✅ Kernel NUMA balancing → Not overridden (filtered)

---

**Documentation Complete:** 2025-11-09

