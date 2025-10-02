# GPU Physical Core Fix for scx_gamer

## Problem Identified

On SMT-enabled systems (like your 8-core/16-thread system), GPU submission threads were ending up on hyperthreads (CPUs 8-15) instead of physical cores (CPUs 0-7), causing increased latency for frame delivery.

### Root Cause

1. **SCX_PICK_IDLE_CORE behavior**: This flag only selects a CPU if the **entire SMT core** is idle (both siblings free). On a busy system, this often fails even when physical cores are available.

2. **No physical core preference**: The scheduler had no explicit logic to prefer physical cores over hyperthreads for critical GPU threads.

3. **CPU topology detection**: For systems with uniform CPU capacity (no hybrid P/E cores), the `preferred_idle_scan` feature was disabled, meaning no CPU priority ordering was established.

## Changes Made

### 1. BPF Code (src/bpf/main.bpf.c)

**Location**: Lines 976-1002 in `pick_idle_cpu_cached()` function

**Change**: Added GPU thread fast path that scans the `preferred_cpus` array (which now prioritizes physical cores on SMT systems) before falling back to standard selection.

```c
if (is_critical_gpu && smt_enabled && preferred_idle_scan) {
    /* Scan preferred_cpus array which already prioritizes physical cores */
    u32 i;
    bpf_for(i, 0, MAX_CPUS) {
        s32 candidate = (s32)preferred_cpus[i];
        if (candidate < 0 || (u32)candidate >= nr_cpu_ids)
            break;
        if (!bpf_cpumask_test_cpu(candidate, p->cpus_ptr))
            continue;

        /* Try to claim this CPU if idle */
        if (scx_bpf_test_and_clear_cpu_idle(candidate)) {
            stat_inc(&nr_idle_cpu_pick);
            stat_inc(&nr_gpu_phys_kept);  // New stat counter
            return candidate;
        }
    }
}
```

### 2. Rust Code (src/main.rs)

**Location**: Lines 333-381 in `Scheduler::init()`

**Changes**:
- Auto-enable `preferred_idle_scan` for **all** SMT systems (not just hybrid CPUs)
- For uniform-capacity SMT systems, sort CPUs to prioritize physical cores (first sibling of each core pair)

```rust
let enable_preferred_scan = preferred_idle_scan || smt_enabled;

if enable_preferred_scan {
    // ... existing capacity checks ...

    if max_cap != min_cap {
        // Heterogeneous: sort by capacity
        cpus.sort_by_key(|cpu| std::cmp::Reverse(cpu.cpu_capacity));
    } else if smt_enabled {
        // Uniform capacity + SMT: prioritize physical cores
        cpus.sort_by_key(|cpu| {
            let core = topo.all_cores.get(&cpu.core_id);
            let is_first_sibling = core
                .and_then(|c| c.cpus.keys().next())
                .map(|&first_id| first_id == cpu.id)
                .unwrap_or(false);
            (!is_first_sibling, cpu.id)  // Physical first, then by ID
        });
        info!("SMT detected with uniform capacity: prioritizing physical cores over hyperthreads");
    }
}
```

### 3. Copyright Updates

Updated all source files to credit RitzDaCat:
- `src/bpf/main.bpf.c`
- `src/main.rs`
- `src/bpf_intf.rs`
- `src/bpf_skel.rs`

## Expected Behavior

**Before**:
- GPU threads (vkd3d-swapchain, vkd3d_queue, dxvk-*, etc.) could be placed on hyperthreads (CPUs 8-15)
- Higher latency for frame submission and presentation

**After**:
- GPU threads are **aggressively prioritized** for physical cores (CPUs 0-7)
- Physical cores are preferred even if their sibling hyperthread is busy
- Falls back to hyperthreads only if no physical core is idle
- New stat counter `nr_gpu_phys_kept` tracks successful physical core placements

## Testing

### 1. Verify Physical Core Placement

```bash
# Run a game with Proton/Wine
# In another terminal, monitor GPU thread placement
watch -n 0.5 'ps -eLo pid,tid,comm,psr | grep -E "(vkd3d|dxvk|RenderThread)" | head -20'
```

**Expected**: GPU threads should primarily show CPUs 0-7 (physical cores), not 8-15 (hyperthreads).

### 2. Check Stats

```bash
# Run scheduler with stats enabled
sudo scx_gamer --stats 1.0

# Look for:
# - nr_gpu_phys_kept: should increase when GPU threads are scheduled
# - gpu_submit_threads: count of detected GPU threads
```

### 3. Preferred CPU Order

Check logs on startup:

```bash
sudo scx_gamer | grep "Preferred CPUs"
```

**Expected output** (on your system):
```
SMT detected with uniform capacity: prioritizing physical cores over hyperthreads
Preferred CPUs: [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15]
```

Physical cores 0-7 come first, then hyperthreads 8-15.

## Performance Impact

**Positive**:
- Lower GPU frame submission latency (fewer cache conflicts, dedicated core resources)
- More consistent frame times
- Better input-to-photon latency

**Neutral**:
- Minimal overhead: only affects GPU thread scheduling path
- No impact on non-GPU threads
- Falls back gracefully if no physical core available

## Future Enhancements

1. Add CLI flag `--gpu-physical-only` to strictly prohibit GPU threads on hyperthreads
2. Extend to other critical thread types (compositors, input handlers)
3. Dynamic adjustment based on system load

## Build Information

- **Compiled successfully**: ✅
- **Target**: x86_64-unknown-linux-gnu
- **Profile**: release (optimized)
- **BPF file size**: 2027 lines (to be refactored into modules for AI-friendliness)

---

**Author**: RitzDaCat
**License**: GPL-2.0
**Date**: 2025-01-XX
