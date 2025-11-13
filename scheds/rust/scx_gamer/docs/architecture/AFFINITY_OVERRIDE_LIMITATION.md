# Affinity Override System - Known Limitation

**Date:** 2025-11-09  
**Status:** ⚠️ **Non-Functional** (fentry attachment fails)

---

## Problem

The affinity override system fails to attach its fentry hook:

```
Failed to attach fentry hook (requires BTF + kernel 5.5+): failed to attach BPF program
```

## Root Cause

The kernel function `sched_setaffinity` is **not available** for fentry attachment on kernel 6.17.7-3-cachyos.

### Evidence

1. ✅ **BTF Available:** `/sys/kernel/btf/vmlinux` exists
2. ✅ **Fentry Works:** `SEC("fentry/input_event")` successfully attaches
3. ❌ **sched_setaffinity Fails:** Specific function not hookable

### Possible Reasons

1. **Internal Function Name Changed:** Kernel 6.x might use `__sched_setaffinity`, `do_sched_setaffinity`, or inline the function
2. **Not Exported:** Function not in BTF export list for fentry attachment
3. **Kernel Configuration:** Some kernel builds don't export scheduler internals

---

## Impact

**Without Affinity Override:**
- ❌ Cannot intercept Unreal Engine's single-core GPU thread pinning
- ❌ Custom CPU affinities set by applications **ARE respected**
- ℹ️ Scheduler still works optimally **except** for artificially restricted threads

**Example:** If Unreal Engine pins render thread to CPU 0:
- Without override: Thread stays on CPU 0 (suboptimal, contention)
- With override: Thread movable to any CPU (optimal, load balanced)

---

## Alternative Solutions

### Option 1: kprobe Instead of fentry (Slower)

Replace `SEC("fentry/sched_setaffinity")` with `SEC("kprobe/sched_setaffinity")`:

**Pros:**
- More compatible across kernel versions
- kprobes more widely available

**Cons:**
- Higher overhead (~200ns → ~500-1000ns)
- Still might not work if function isn't exported

### Option 2: Periodic /proc Scanning (Like Audio Detector)

Similar to `audio_detect.rs`, periodically scan `/proc/[pid]/status` for custom affinities:

**Pros:**
- Works on all kernels (no BPF requirements)
- Proven approach (audio detector uses this)

**Cons:**
- Higher CPU overhead (scan every 1-5 seconds)
- Slower detection (seconds vs microseconds)

### Option 3: Ptrace-Based Monitoring

Monitor `PTRACE_SYSCALL` for `sched_setaffinity` calls:

**Pros:**
- Kernel-agnostic
- Precise syscall interception

**Cons:**
- Very high overhead
- Requires ptrace permissions
- Complex implementation

### Option 4: Document as Experimental

Mark feature as experimental and require specific kernel configurations:

```rust
#[cfg(feature = "affinity_override_experimental")]
impl AffinityOverride {
    // ... existing code
}
```

---

## Recommended Action

**For Production:** Disable affinity override feature (it's optional).

**For Development:** Investigate kernel function name:
1. Check if `__sched_setaffinity` or `do_sched_setaffinity` exists
2. Try kprobe instead of fentry
3. Consider /proc scanning fallback

---

## Code Location

- BPF: `src/bpf/main.bpf.c:5322` - `SEC("fentry/sched_setaffinity")`
- Rust: `src/affinity_override.rs:75` - `.attach()` call
- Detection: `src/bpf/include/affinity_detect.bpf.h`

---

## Testing on Other Kernels

If you have access to other systems, test on:
- Vanilla kernel 6.11+ (Ubuntu/Fedora/Arch)
- Kernel 5.15 LTS
- Different distributions

This will help determine if it's CachyOS-specific or universal.

---

**Status:** Feature disabled by design (graceful fallback working correctly).  
**Priority:** Low (optional optimization, not critical for scheduler functionality).

