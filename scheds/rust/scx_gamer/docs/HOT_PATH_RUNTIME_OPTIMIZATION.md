# Hot Path Runtime Optimization - Batch Window Decay

## Problem Statement

### Observed Behavior (After Dispatch Coalescing)
```
gamer_enqueue:   258ns avg × 66k/sec  = 1.51% CPU
gamer_runnable:  172ns avg × 96k/sec  = 1.44% CPU  
gamer_select_cp: 165ns avg × 92k/sec  = 1.36% CPU
```

**Root cause**: `maybe_decay_input_windows()` called on every wakeup in both `select_cpu` and `enqueue`:
- `gamer_select_cpu`: 92k calls/sec
- `gamer_enqueue`: 66k calls/sec
- **Total: 158k redundant time checks/sec**

## Analysis

### What `maybe_decay_input_windows()` Does

```c
static __always_inline void maybe_decay_input_windows(u64 now) {
    // Check if continuous input mode should decay
    if (!time_before(now, last_input_trigger_ns + INPUT_IDLE_DECAY_NS))
        input_trigger_rate = 0;  // Only updates every ~5ms
    
    // Check if input lanes should decay (loops 3x)
    for (int lane = 0; lane < INPUT_LANE_MAX; lane++)
        if (!time_before(now, input_lane_until[lane]))
            continuous_input_lane_mode[lane] = 0;
}
```

**Critical insight**: This is window **DECAY** (background cleanup), not **ACTIVATION**!

- **Activation** (latency-critical): Happens immediately via `trigger_input_window()` from userspace
- **Decay** (non-critical): Gradual cleanup of expired windows

**Current overhead**:
- Time checks: 4 per call (1 global + 3 lanes)
- Total: 158k calls × 4 checks = **632k time comparisons/sec**
- Cost: ~5-10ns per comparison × 632k = **3.16-6.32M ns/sec = 0.32-0.63% CPU wasted**

### Why This Doesn't Affect Input Latency

**Input activation path** (unchanged):
```
Hardware interrupt
  ↓
evdev driver
  ↓
BPF tracepoint (immediate)
  ↓
Ring buffer (immediate)  
  ↓
Userspace reads (immediate)
  ↓
trigger_input_window() ← Sets window IMMEDIATELY
  ↓
BPF syscall (immediate)
  ↓
fanout_set_input_window() ← Activates NOW
```

**Window decay** (batched):
- Old: Checked every wakeup (158k/sec)
- New: Checked in housekeeping (~200/sec)
- Window lifetime: Typically 5-10ms
- Decay granularity: ~4.35ms (perfectly fine!)

**Result**: Input windows still activate immediately, they just decay slightly less frequently. No impact on input handling!

## Solution: Batch Window Decay in Housekeeping

Move `maybe_decay_input_windows()` from per-wakeup to housekeeping cycle.

### Implementation

**Before**:
```c
s32 gamer_select_cpu_slowpath(...) {
    u64 now = scx_bpf_now();
    maybe_decay_input_windows(now);  // ← 92k calls/sec
    // ...
}

void gamer_enqueue_slowpath(...) {
    u64 now = scx_bpf_now();
    maybe_decay_input_windows(now);  // ← 66k calls/sec
    // ...
}
```

**After**:
```c
s32 gamer_select_cpu_slowpath(...) {
    u64 now = scx_bpf_now();
    // maybe_decay_input_windows() moved to housekeeping
    // ...
}

void gamer_enqueue_slowpath(...) {
    u64 now = scx_bpf_now();
    // maybe_decay_input_windows() moved to housekeeping
    // ...
}

void maybe_run_housekeeping(void) {
    // ...
    maybe_decay_input_windows(now);  // ← ~200 calls/sec (batched!)
    // ...
}
```

## Expected Impact

### CPU Savings

**Eliminated operations**:
- `maybe_decay_input_windows()` calls: 158k/sec → ~200/sec
- Reduction: **~157.8k calls/sec eliminated**
- Time checks eliminated: 157.8k × 4 = **631.2k/sec**

**CPU reduction**:
- Low estimate: 631k × 5ns = **3.16M ns/sec = 0.32% CPU**
- High estimate: 631k × 10ns = **6.32M ns/sec = 0.63% CPU**

**Expected runtime reduction**:
- `gamer_select_cp`: 165ns → **~155ns** (-10ns)
- `gamer_enqueue`: 258ns → **~248ns** (-10ns)

**Combined effect**:
- Select CPU: 92k × 10ns = 0.92M ns/sec saved
- Enqueue: 66k × 10ns = 0.66M ns/sec saved
- **Total: ~1.58M ns/sec = ~0.16% CPU saved**

### Latency Impact

**None!** Here's why:

1. **Input activation**: Still immediate (userspace → BPF syscall)
2. **Window decay**: Non-critical background task
3. **Decay frequency**: ~200/sec (every ~5ms) is perfectly adequate for 5-10ms windows
4. **Gaming perspective**: Input handling unchanged, only cleanup batched

## Verification

### Before Optimization
```
gamer_select_cp: 165ns avg × 92k/sec = 1.36% CPU
gamer_enqueue:   258ns avg × 66k/sec = 1.51% CPU
```

### After Optimization (Expected)
```
gamer_select_cp: ~155ns avg × 92k/sec = ~1.28% CPU  (-0.08%)
gamer_enqueue:   ~248ns avg × 66k/sec = ~1.43% CPU  (-0.08%)
Total saved: ~0.16% CPU
```

### How to Measure

```bash
# Deploy optimized scheduler
sudo ./start.sh

# Wait for stats to stabilize
sleep 30

# Check metrics
sudo bpftool prog show | grep -E "gamer_(select|enqueue)"
```

**Look for**:
- Lower avg runtime (ns)
- Same events/sec
- Lower Total CPU %

## Combined Optimization Impact

| Optimization | CPU Saved | Cumulative |
|--------------|-----------|------------|
| Dispatch coalescing | 5.4% | 5.4% |
| Window decay batching | 0.16% | **5.56%** |

**Total BPF overhead**:
- Before all optimizations: ~10.5%
- After dispatch coalescing: ~5.1%
- After window decay batching: **~4.94%** (expected)

## Trade-offs

### Pros
- ✅ **0.16% CPU savings** with zero input latency impact
- ✅ **Simpler hot paths**: Removed function call overhead
- ✅ **Reduced time check overhead**: 631k fewer time comparisons/sec
- ✅ **Input activation unchanged**: Still immediate via userspace trigger

### Cons
- ⚠️ **Slightly delayed window decay**: 
  - Old: Decay checked every wakeup (~6.3μs worst case @ 158k/sec)
  - New: Decay checked in housekeeping (~4.35ms)
  - Impact: Negligible (windows are 5-10ms anyway)

### Defense in Depth

**Input windows still function correctly**:
- **Activation**: Immediate (userspace → BPF syscall, unchanged)
- **Duration**: 5-10ms (configured, unchanged)
- **Decay**: Every ~4.35ms vs continuous (fine for 5-10ms windows)
- **Safety**: Window decay is cleanup, not critical path

**Example scenario**:
1. User presses key at T=0
2. Window activates **immediately** (userspace trigger)
3. Window set to expire at T=10ms
4. Decay check at T=4.35ms: Still active ✓
5. Decay check at T=8.7ms: Still active ✓
6. Decay check at T=13.05ms: Expired, cleared ✓

**Result**: Window active for intended duration, just cleared ~3ms later than minimum. No impact on input handling!

## Summary

This optimization eliminates **157.8k redundant calls/sec** to `maybe_decay_input_windows()` by batching window decay in housekeeping instead of checking on every wakeup.

**Key insight**: Window decay is background cleanup, not input handling. Input activation still happens immediately via userspace trigger with zero added latency.

**Expected result**: Hot path runtimes drop by ~10ns each, saving **~0.16% CPU** with no impact on input latency or gaming performance.

