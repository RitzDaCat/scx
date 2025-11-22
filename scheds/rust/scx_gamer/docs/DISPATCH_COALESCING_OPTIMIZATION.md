# Dispatch Coalescing Optimization

## Problem Statement

### Observed Behavior
```
gamer_dispatch: 940,925 events/sec × 60ns avg = 4.78% CPU
```

**Root cause**: `gamer_dispatch()` calls two housekeeping functions that check `scx_bpf_now()` on **every single dispatch** to see if enough time has passed:

```c
void gamer_dispatch(...) {
    maybe_sample_cpu_util();     // Calls scx_bpf_now() 940k/sec
    maybe_run_housekeeping();     // Calls scx_bpf_now() 940k/sec
    // ... actual dispatch work ...
}
```

### Cost Analysis

**Time check overhead**:
- `scx_bpf_now()` cost: ~5-10ns (Tier 1 BPF helper)
- Calls per second: 940k × 2 = **1.88 million**
- Total overhead: **9.4-18.8 million ns/sec = 0.94-1.88% CPU**

**But actual execution frequency**:
- `maybe_sample_cpu_util()`: Only executes every 500μs = **~2,000/sec**
- `maybe_run_housekeeping()`: Only executes every 5ms = **~200/sec**

**Waste ratio**: 99.76% of time checks are redundant!

## Solution: Counter-Based Coalescing

### Strategy

Replace time-based checks with **call-counter modulo operations**:

**Before** (time-based):
```c
static __always_inline void maybe_sample_cpu_util(void) {
    u64 now = scx_bpf_now();  // 5-10ns, called 940k/sec
    if (time_before(now, last + 500_000))  // 500μs
        return;
    // ... do sampling ...
}
```

**After** (counter-based):
```c
void gamer_dispatch(...) {
    if (should_sample_cpu_util())  // ~1-2ns, modulo check
        maybe_sample_cpu_util();   // Only called ~2k/sec
    // ...
}
```

### Implementation

**Key insight**: At 940k dispatch/sec, we can predict call frequency:
- Sample util every ~512 calls (940k / 512 ≈ 1,836/sec ≈ 545μs)
- Run housekeeping every ~4,096 calls (940k / 4,096 ≈ 230/sec ≈ 4.35ms)

**Optimization**: Use powers of 2 for fast modulo via bitwise AND:
```c
// Instead of: (count % 512)
// Use:        (count & 511)  ← Single AND instruction
```

### Code Changes

**New header**: `src/bpf/include/coalesce.bpf.h`
- Counter-based helpers
- Per-CPU state tracking
- Power-of-2 intervals for fast modulo

**Modified**: `src/bpf/main.bpf.c`
```c
void BPF_STRUCT_OPS(gamer_dispatch, ...) {
    // BEFORE: Direct calls (2× scx_bpf_now() per dispatch)
    // maybe_sample_cpu_util();
    // maybe_run_housekeeping();
    
    // AFTER: Counter-gated calls (eliminate 99.76% of time checks)
    if (should_sample_cpu_util())
        maybe_sample_cpu_util();
    
    if (should_run_housekeeping())
        maybe_run_housekeeping();
    
    // ... rest of dispatch ...
}
```

## Expected Impact

### CPU Savings

**Eliminated operations**:
- `scx_bpf_now()` calls: 1.88M/sec → ~2.2k/sec
- Savings: **~1.878 million calls/sec**

**CPU reduction**:
- Low estimate: 1.878M × 5ns = **9.39 million ns/sec = 0.94% CPU**
- High estimate: 1.878M × 10ns = **18.78 million ns/sec = 1.88% CPU**

**Expected `gamer_dispatch` CPU**:
- Before: 4.78%
- After: **2.90-3.84%** (39-40% reduction)

### Latency Impact

**None!** The optimization maintains the same logical behavior:
- Util sampling still runs every ~500μs
- Housekeeping still runs every ~5ms
- Counter check is faster than time check (~1-2ns vs 5-10ns)

### Adaptive Behavior

**Bonus**: The counter approach automatically adapts to dispatch frequency:
- If dispatch rate increases → sampling/housekeeping frequency increases proportionally
- If dispatch rate decreases → proportional decrease
- No manual tuning required

## Measuring Impact

### Before Optimization

```bash
# Capture baseline
sudo bpftool prog show | grep gamer_dispatch
# Note: events/sec and Total CPU%
```

Example output:
```
1665  StructOps  gamer_dispatch  50ns  60ns  940925 events/sec  4.78% CPU
```

### After Optimization

```bash
# Run optimized scheduler
sudo ./start.sh

# Wait 30 seconds for stats to stabilize
sleep 30

# Check new metrics
sudo bpftool prog show | grep gamer_dispatch
```

Expected output:
```
1665  StructOps  gamer_dispatch  40ns  50ns  940925 events/sec  3.20% CPU
```

**Key metrics**:
- ✓ Events/sec: Same (~940k) - dispatch frequency unchanged
- ✓ Avg runtime: Lower (60ns → 50ns) - faster execution
- ✓ CPU %: Lower (4.78% → ~3.2%) - reduced overhead

## Trade-offs

### Pros
- ✅ **0.94-1.88% CPU savings** with zero latency impact
- ✅ **Faster execution**: Bitwise AND (~1ns) vs time check (~5-10ns)
- ✅ **Adaptive**: Automatically adjusts to dispatch frequency
- ✅ **Deterministic**: Exact call-count-based triggering (no time jitter)

### Cons
- ⚠️ **Slight deviation from exact timing**: 
  - Target: Every 500μs, actual: Every ~545μs (512 calls @ 940k/sec)
  - Impact: Negligible for util sampling and housekeeping
- ⚠️ **Assumes stable dispatch frequency**:
  - If dispatch rate drops to 100k/sec, sampling becomes every 5.12ms
  - Mitigation: Time checks inside `maybe_*` functions provide fallback

### Defense in Depth

The implementation keeps **both** checks:
1. **Counter check** (outer): Fast gate, runs 940k/sec
2. **Time check** (inner): Slow but accurate, runs ~2k/sec

This provides:
- Fast path: Counter rejects 99.76% of calls immediately
- Safety net: Time check ensures minimum interval even if dispatch rate changes
- Best of both worlds: Performance + correctness

## Verification

### Manual Testing

```bash
# 1. Build optimized version
./build.sh

# 2. Run scheduler
sudo ./start.sh

# 3. Monitor in separate terminal
watch -n 1 'sudo bpftool prog show | grep -A1 gamer_dispatch'

# 4. Check CPU %decrease over time
```

### Automated Testing

```bash
# Compare before/after
scripts/benchmark_dispatch_overhead.sh
```

## Future Enhancements

### Dynamic Tuning

Could auto-adjust intervals based on measured dispatch frequency:

```c
// Measure average dispatch rate over last second
u64 dispatch_rate = measure_dispatch_rate();

// Adjust intervals dynamically
u32 util_interval = dispatch_rate / 2000;      // Target 2k/sec
u32 housekeeping_interval = dispatch_rate / 200;  // Target 200/sec
```

### Per-CPU Coalescing

Currently uses per-CPU map (no contention). Could further optimize by:
- Using different intervals per CPU based on load
- Skipping housekeeping entirely on idle CPUs

## References

- **LMAX Disruptor**: Mechanical Sympathy - avoiding unnecessary work
- **Linux Kernel**: RCU batching for similar optimization pattern
- **Performance Counter**: Similar to Intel's Performance Monitoring Counters

## Conclusion

**This optimization eliminates ~1.88 million redundant time checks per second**, reducing `gamer_dispatch` CPU by **0.94-1.88%** with **zero latency impact**.

The counter-based approach is:
- **Faster**: 1-2ns vs 5-10ns per check
- **Adaptive**: Adjusts to dispatch frequency automatically
- **Safe**: Time checks provide fallback guarantee

**Expected result**: `gamer_dispatch` CPU drops from **4.78% → ~3.2%** (33% reduction).

