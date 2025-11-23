# Hot Path → Cold Path Optimization Opportunities
**Date:** 2025-11-23  
**Scheduler:** scx_gamer  
**Goal:** Reduce hot path overhead without impacting gaming latency

---

## Current Hot Path Metrics (Palworld)

| **Function** | **CPU %** | **Avg Runtime (ns)** | **Events/sec** | **Total ns/sec** |
|--------------|-----------|---------------------|----------------|------------------|
| **gamer_dispatch** | 2.29% | 50ns | 451,160 | ~22.6M ns |
| **gamer_select_cp** | 1.40% | 173ns | 80,857 | ~14M ns |
| **gamer_enqueue** | 1.38% | 266ns | 51,775 | ~13.8M ns |
| **gamer_runnable** | 1.31% | 157ns | 82,986 | ~13M ns |
| **TOTAL** | **~6.38%** | - | - | **~63.4M ns/sec** |

**Target:** Move ~1-2% CPU to cold paths → ~10-20M ns/sec savings

---

## Optimization #1: Coalesce Profiling Overhead

### Current State (HOT PATH - Every Call)
```c
void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    PROF_START_HIST(dispatch);  // ← Histogram update (~5-10ns)
    
    // ... actual work ...
    
    PROF_END_HIST(dispatch);  // ← Histogram update (~5-10ns)
}
```

**Cost:**
- `PROF_START_HIST` + `PROF_END_HIST`: ~10-20ns per call
- Across all hot paths: ~(451k + 80k + 51k + 82k) = **665k calls/sec**
- **Total overhead:** 6.6-13M ns/sec = **0.66-1.3% CPU** 🔥

### Proposed (COLD PATH - Coalesced)
```c
static __always_inline bool should_profile(void)
{
    const u32 idx = 0;
    struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
    if (!ctx)
        return false;
    
    ctx->profile_call_count++;
    
    // Profile every 64th call (still get good histogram data)
    if ((ctx->profile_call_count & 63) == 0) {
        ctx->profile_call_count = 0;
        return true;
    }
    
    return false;
}

void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    u64 profile_start = should_profile() ? scx_bpf_now() : 0;
    
    // ... actual work ...
    
    if (profile_start) {
        u64 profile_end = scx_bpf_now();
        // Store in histogram
    }
}
```

**Savings:**
- Profile 1 in 64 calls instead of all calls
- **Overhead reduction:** 98.4% (63/64 calls skip profiling)
- **CPU saved:** ~0.65-1.3% (keeps histogram data quality high)

**Impact on Gaming:** ✅ NONE - profiling is diagnostic only

---

## Optimization #2: Coalesce Deadline Tracking

### Current State (HOT PATH - Every Enqueue)
```c
// In gamer_enqueue (runs ~51k times/sec)
u64 deadline = task_dl_with_ctx_cached(p, tctx, prev_cctx, fg_tgid);
tctx->expected_deadline = deadline;  // ← Stored on EVERY enqueue
```

**Cost:**
- `task_dl_with_ctx_cached()`: ~50-100ns (complex deadline calculation)
- `tctx->expected_deadline = deadline`: ~1-2ns (store)
- **Total overhead:** ~51k calls × 50-100ns = **2.5-5M ns/sec = 0.25-0.5% CPU**

### Current Usage (COLD PATH - Only for Stats)
```c
// In gamer_stopping (runs ~65k times/sec)
if (tctx->expected_deadline > 0 && tctx->boost_shift >= 3) {  // ← Only critical threads
    u64 current_vtime = p->scx.dsq_vtime;
    
    if (unlikely(current_vtime > tctx->expected_deadline)) {
        // Deadline miss detected - emit event for diagnostics
        if (likely(dispatch_event_enable) && likely(!no_stats)) {
            // Ring buffer write (~100-200ns)
        }
    }
}
```

**Observation:**
- Deadline tracking is ONLY used for diagnostics (deadline miss detection)
- Protected by `dispatch_event_enable` && `!no_stats` gates
- NOT used for actual scheduling decisions

### Proposed (COLD PATH - Coalesced)
```c
// In gamer_enqueue
if (should_track_deadline()) {  // Check every 64th call
    u64 deadline = task_dl_with_ctx_cached(p, tctx, prev_cctx, fg_tgid);
    tctx->expected_deadline = deadline;
}
```

**OR - More Aggressive:**
```c
// In gamer_enqueue
if (likely(dispatch_event_enable) && likely(!no_stats)) {
    // Only track deadlines when stats are enabled
    u64 deadline = task_dl_with_ctx_cached(p, tctx, prev_cctx, fg_tgid);
    tctx->expected_deadline = deadline;
}
```

**Savings:**
- Coalesced (1/64): **~2.4-4.9M ns/sec = 0.24-0.49% CPU**
- Skip when no_stats: **~2.5-5M ns/sec = 0.25-0.5% CPU** (Esports profile)

**Impact on Gaming:** ✅ NONE - deadline tracking is diagnostic only

---

## Optimization #3: Coalesce CPUfreq Updates

### Current State (HOT PATH - Every Task Start)
```c
// In gamer_running (runs ~89k times/sec @ Palworld)
if (likely(cpufreq_enabled))
    update_cpufreq(cpu);  // ← Called on EVERY task start
```

**Cost:**
- `update_cpufreq()`: ~20-50ns (per-CPU state update)
- **Total overhead:** 89k calls × 20-50ns = **1.8-4.5M ns/sec = 0.18-0.45% CPU**

### Rationale for Coalescing
- CPUfreq governors (performance/schedutil) don't need millisecond-level updates
- Governor reacts to CPU utilization over time, not single task starts
- Even 10ms granularity is fine for CPUfreq scaling

### Proposed (COLD PATH - Coalesced)
```c
static __always_inline bool should_update_cpufreq(void)
{
    const u32 idx = 0;
    struct dispatch_coalesce_ctx *ctx = bpf_map_lookup_elem(&dispatch_coalesce_stor, &idx);
    if (!ctx)
        return true;  // Fallback: always update if lookup fails
    
    ctx->cpufreq_call_count++;
    
    // Update every 128 task starts (~1-2ms @ Palworld)
    if ((ctx->cpufreq_call_count & 127) == 0) {
        ctx->cpufreq_call_count = 0;
        return true;
    }
    
    return false;
}

// In gamer_running
if (likely(cpufreq_enabled) && should_update_cpufreq())
    update_cpufreq(cpu);
```

**Savings:**
- Update 1 in 128 task starts (still ~1-2ms granularity)
- **Overhead reduction:** 99.2% (127/128 calls skip update)
- **CPU saved:** ~1.8-4.4M ns/sec = 0.18-0.44% CPU

**Impact on Gaming:** ✅ NONE - CPUfreq doesn't need sub-millisecond updates

---

## Optimization #4: Already Optimized (Keep As-Is)

### Ring Buffer Writes (Dispatch Events)
```c
// In execute_enqueue_plan
if (plan->emit_dispatch_evt && likely(dispatch_event_enable) && likely(!no_stats)) {
    // Ring buffer write (~100-200ns)
}
```

**Status:** ✅ Already optimized
- Gated by `dispatch_event_enable` && `!no_stats`
- Esports profile disables this (--no-stats)
- No further optimization needed

### Stats Collection
```c
// In various paths
if (likely(!no_stats)) {
    // Stats collection
}
```

**Status:** ✅ Already optimized
- Gated by `no_stats` flag
- Esports profile disables this (--no-stats)
- No further optimization needed

---

## Optimization #5: Potential - Lazy vtime Updates

### Current State (HOT PATH - Every Task Start)
```c
// In gamer_running (runs ~89k times/sec)
struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
if (cctx && time_before(cctx->vtime_now, p->scx.dsq_vtime))
    cctx->vtime_now = p->scx.dsq_vtime;  // ← Update on EVERY task start
```

**Cost:**
- Map lookup: ~20-50ns
- Comparison + store: ~2-5ns
- **Total overhead:** ~22-55ns × 89k = **2-5M ns/sec = 0.2-0.5% CPU**

**Analysis:**
- vtime updates are used for deadline scheduling
- Needs to be accurate for task ordering
- **Risk:** High - affects scheduling decisions

**Recommendation:** ⚠️ Keep as-is (too risky to coalesce)

---

## Summary of Safe Optimizations

| **Optimization** | **Location** | **Current Overhead** | **Savings** | **Risk** |
|------------------|--------------|---------------------|-------------|----------|
| **Profiling Coalescing** | All hot paths | 0.66-1.3% CPU | 0.65-1.3% | ✅ None (diagnostic only) |
| **Deadline Tracking** | gamer_enqueue | 0.25-0.5% CPU | 0.24-0.5% | ✅ None (diagnostic only) |
| **CPUfreq Updates** | gamer_running | 0.18-0.45% CPU | 0.18-0.44% | ✅ None (1-2ms granularity ok) |
| **TOTAL SAVINGS** | - | **1.09-2.25% CPU** | **1.07-2.24% CPU** | ✅ Zero gaming impact |

---

## Implementation Priority

### Priority 1: Profiling Coalescing (HIGHEST IMPACT) ✅
**Expected savings:** 0.65-1.3% CPU  
**Complexity:** Low  
**Risk:** None (diagnostic only)

**Implementation:**
1. Add `profile_call_count` to `dispatch_coalesce_ctx`
2. Add `should_profile()` helper (similar to `should_sample_cpu_util()`)
3. Gate `PROF_START_HIST/PROF_END_HIST` with `should_profile()` check
4. Profile every 64th call (still excellent histogram data)

### Priority 2: Deadline Tracking Gate (MEDIUM IMPACT) ✅
**Expected savings:** 0.24-0.5% CPU  
**Complexity:** Very Low  
**Risk:** None (already gated in usage)

**Implementation:**
1. Gate `expected_deadline` calculation with `dispatch_event_enable` && `!no_stats`
2. Skip deadline tracking when stats disabled (Esports profile)

### Priority 3: CPUfreq Coalescing (LOW-MEDIUM IMPACT) ✅
**Expected savings:** 0.18-0.44% CPU  
**Complexity:** Low  
**Risk:** None (CPUfreq tolerates >10ms updates)

**Implementation:**
1. Add `cpufreq_call_count` to `dispatch_coalesce_ctx`
2. Add `should_update_cpufreq()` helper
3. Update every 128th call (~1-2ms granularity)

---

## Expected Total Impact

### Before Optimizations:
- **Total BPF CPU:** 7.98% (current @ Palworld 4x coalescing)

### After All 3 Optimizations:
- **Savings:** 1.07-2.24% CPU
- **New Total:** **5.74-6.91% BPF CPU** 🎯
- **Gap to 3% goal:** 2.74-3.91%

### Additional Optimizations Needed:
- Current: ~6-7% CPU
- Goal: 3% CPU
- Remaining: **~3-4% CPU reduction needed**

**Next steps after these optimizations:**
1. Analyze remaining hot path work in select_cpu/enqueue
2. Consider more aggressive coalescing (32x housekeeping)
3. Investigate if any expensive operations in CPU selection can be cached

---

## Code Structure Recommendations

### Add to `coalesce.bpf.h`:
```c
struct dispatch_coalesce_ctx {
    u32 dispatch_call_count;
    u32 util_sample_calls;
    u32 input_decay_calls;
    u32 housekeeping_calls;
    u32 runnable_call_count;
    
    // NEW: Add these counters
    u32 profile_call_count;      // For profiling coalescing
    u32 cpufreq_call_count;      // For CPUfreq coalescing
};

#define PROFILE_EVERY 64  // Profile every 64th call
#define CPUFREQ_UPDATE_EVERY 128  // Update CPUfreq every 128 task starts

static __always_inline bool should_profile(void) { ... }
static __always_inline bool should_update_cpufreq(void) { ... }
```

### Modify hot paths:
```c
// gamer_dispatch, gamer_select_cpu, gamer_enqueue, gamer_runnable
void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    u64 prof_start = should_profile() ? scx_bpf_now() : 0;
    
    // ... work ...
    
    if (prof_start) {
        u64 prof_end = scx_bpf_now();
        record_histogram(prof_end - prof_start);
    }
}

// gamer_enqueue
if (likely(dispatch_event_enable) && likely(!no_stats)) {
    u64 deadline = task_dl_with_ctx_cached(p, tctx, prev_cctx, fg_tgid);
    tctx->expected_deadline = deadline;
}

// gamer_running
if (likely(cpufreq_enabled) && should_update_cpufreq())
    update_cpufreq(cpu);
```

---

## Testing Strategy

### Validation (CRITICAL):
1. **Input latency:** Measure with input_event_raw latency (should be unchanged)
2. **Frame times:** Monitor frame pacing (should be unchanged)
3. **Audio latency:** Check audio deadline misses (should be unchanged)
4. **CPU usage:** Monitor total BPF CPU% (should drop 1-2%)

### Success Criteria:
- ✅ Input latency: <30ns (unchanged)
- ✅ Frame times: No degradation
- ✅ Audio: No deadline misses
- ✅ Total BPF CPU: 5.7-6.9% (down from 7.98%)

---

## Conclusion

**Safe to move to cold paths:** ~1.07-2.24% CPU overhead
- ✅ Profiling overhead (diagnostic only)
- ✅ Deadline tracking (stats only)
- ✅ CPUfreq updates (tolerates 1-2ms delay)

**Keep on hot path:** ~4-5% CPU (actual scheduling work)
- vtime updates (affects scheduling)
- Task context lookups (required for decisions)
- CPU selection logic (core functionality)
- Boost calculations (affects priorities)

**Next milestone:** 5.7-6.9% total BPF CPU (after these optimizations)  
**Ultimate goal:** 3% total BPF CPU (need 2.7-3.9% more reduction)

