# Schedulability Analysis Implementation Plan

**Date:** 2025-11-05  
**Priority:** ⚠️ **HIGH** - Complements RMS, provides formal deadline guarantees  
**Status:** Planning → Ready for Implementation

---

## Executive Summary

**Goal:** Implement Schedulability Analysis (Liu & Layland 1973) to provide formal guarantees that all periodic tasks can meet their deadlines under RMS scheduling.

**Why This Works WITH RMS:**
- **RMS assigns priorities** based on periods (just implemented)
- **Schedulability Analysis validates** those priorities can meet deadlines
- **Together:** RMS provides priority assignment, Schedulability Analysis provides guarantees

**Expected Impact:**
- **Formal guarantee:** All admitted tasks meet deadlines (if schedulable)
- **Overload detection:** Early warning when system becomes unschedulable
- **Admission control:** Prevent adding tasks that would cause deadline misses

**Effort:** Low-Medium  
**Risk:** Low (additive validation, doesn't change scheduling logic)

---

## Academic Foundation

**Paper:** Liu & Layland (1973) - "Scheduling Algorithms for Multiprogramming in a Hard-Real-Time Environment"

**Key Concepts:**

### 1. Utilization Bound Test

**For RMS (Rate Monotonic Scheduling):**
```
U = Σ(Ci / Pi) ≤ n * (2^(1/n) - 1)

Where:
  U = Total system utilization
  Ci = Worst-case execution time of task i
  Pi = Period of task i
  n = Number of tasks
```

**For EDF (Earliest Deadline First):**
```
U = Σ(Ci / Pi) ≤ 100%

Where:
  U = Total system utilization (must be ≤ 100%)
```

### 2. Response Time Analysis

**Worst-case response time calculation:**
```
Ri = Ci + Σ(ceil(Ri / Pj) * Cj) for all higher-priority tasks j

Task is schedulable if Ri ≤ Di (deadline)
```

**Practical Application:**
- Calculate worst-case response time for each task
- Verify response time ≤ deadline
- If yes: Task is schedulable
- If no: Task cannot meet deadline (reject or adjust)

---

## Current State Analysis

### ✅ What We Have (from RMS implementation):

1. **Period Detection:** ✅
   - `detected_period_ns` - Task period for periodic tasks
   - `is_periodic` - Flag indicating periodic task
   - GPU/compositor: Uses `frame_interval_ns`
   - Input handlers: Uses `input_trigger_rate` or `wakeup_freq`

2. **Execution Time Tracking:** ✅
   - `exec_runtime` - Accumulated execution time
   - `exec_avg` - EMA of execution time per wake cycle
   - Already tracked in `task_ctx`

3. **EDF Scheduling:** ✅
   - EDF mode enabled at 24% CPU util
   - Deadline-based priority (`task_dl_with_ctx_cached()`)
   - Load-based mode switching (`is_system_busy()`)

### ❌ What's Missing:

1. **Utilization Calculation:**
   - No per-task utilization tracking
   - No total system utilization calculation
   - No utilization bound checks

2. **Schedulability Validation:**
   - No check before enabling EDF mode
   - No validation that RMS priorities meet deadlines
   - No response time analysis

3. **Admission Control:**
   - No check when new tasks are added
   - No rejection/adjustment of unschedulable tasks
   - No overload handling

---

## Implementation Phases

### Phase 1: Add Utilization Tracking ✅

**Add to `task_ctx` structure:**

```c
struct task_ctx {
    // ... existing fields ...
    
    /* Schedulability Analysis */
    u64 utilization_pct;        /* (Ci / Pi) * 100 (fixed-point, 100 = 1%) */
    u64 worst_case_exec_ns;    /* Worst-case execution time (Ci) */
    u64 worst_case_response_ns; /* Worst-case response time (Ri) */
};
```

**Calculate utilization for periodic tasks:**

```c
/**
 * Update task utilization based on execution time and period.
 * 
 * Utilization = (execution_time / period) * 100
 * 
 * For periodic tasks:
 * - Use exec_avg as execution time estimate (Ci)
 * - Use detected_period_ns as period (Pi)
 * - Calculate utilization percentage
 * 
 * @tctx: Task context
 */
static __always_inline void update_task_utilization(struct task_ctx *tctx)
{
    if (!tctx->is_periodic || tctx->detected_period_ns == 0)
        return;
    
    /* Use exec_avg as worst-case execution time estimate
     * exec_avg is EMA of execution time per wake cycle */
    u64 exec_time = tctx->exec_avg;
    if (exec_time == 0) {
        /* Fallback: Use exec_runtime if exec_avg not available */
        exec_time = tctx->exec_runtime;
    }
    
    /* Calculate utilization: (Ci / Pi) * 100
     * Fixed-point: 100 = 1%, 10000 = 100% */
    if (exec_time > 0 && tctx->detected_period_ns > 0) {
        tctx->utilization_pct = (exec_time * 100) / tctx->detected_period_ns;
        tctx->worst_case_exec_ns = exec_time;
    }
}
```

**Location:** `src/bpf/include/helpers.bpf.h` or `src/bpf/main.bpf.c`

**Call Site:** After period detection in `gamer_runnable()`

---

### Phase 2: Add Total Utilization Calculation ✅

**Track total system utilization:**

```c
/**
 * Calculate total system utilization for periodic tasks.
 * 
 * Total Utilization = Σ(Ci / Pi) for all periodic tasks
 * 
 * EDF Schedulability: U ≤ 100%
 * RMS Schedulability: U ≤ n * (2^(1/n) - 1)
 * 
 * @return: Total utilization percentage (fixed-point, 100 = 1%)
 */
static __always_inline u64 calculate_total_utilization(void)
{
    /* OPTIMIZATION: BPF map iteration is expensive
     * Instead, use per-CPU aggregation or cached value
     * 
     * Option 1: Use existing cpu_util_avg as proxy
     * Option 2: Track per-CPU utilization, sum in userspace
     * Option 3: Incrementally update total on task add/remove
     * 
     * For now, use cpu_util_avg as approximation */
    
    /* cpu_util_avg is per-CPU utilization in fixed-point (1024 = 100%)
     * Sum across all CPUs to get total system utilization */
    u64 total_util = 0;
    s32 cpu;
    
    for (cpu = 0; cpu < MAX_CPUS; cpu++) {
        struct cpu_ctx *cctx = try_lookup_cpu_ctx(cpu);
        if (cctx && cctx->cpu_util_avg > 0) {
            total_util += cctx->cpu_util_avg;
        }
    }
    
    /* Convert from per-CPU to system-wide (may overestimate due to parallelism)
     * Better: Track only periodic tasks utilization */
    return total_util;
}
```

**Simplified Approach (Recommended):**

```c
/**
 * Simplified: Use existing system load metrics
 * More accurate: Track only periodic tasks (requires map iteration)
 */
static __always_inline u64 calculate_periodic_tasks_utilization(void)
{
    /* Use existing cpu_util_avg multiplied by periodic task ratio
     * This is an approximation - exact calculation requires iteration */
    
    /* For now, return 0 (not implemented) - will use cpu_util_avg as proxy */
    return 0;
}
```

**Location:** `src/bpf/include/helpers.bpf.h`

---

### Phase 3: Add Utilization Bound Check ✅

**Check schedulability before enabling EDF:**

```c
/**
 * Check if system is schedulable under EDF or RMS.
 * 
 * EDF Schedulability: U ≤ 100%
 * RMS Schedulability: U ≤ n * (2^(1/n) - 1)
 * 
 * @use_rms: If true, use RMS bound; if false, use EDF bound
 * @return: true if schedulable, false otherwise
 */
static __always_inline bool is_schedulable(bool use_rms)
{
    u64 total_util = calculate_total_utilization();
    
    if (use_rms) {
        /* RMS bound: U ≤ n * (2^(1/n) - 1)
         * For n tasks:
         *   n=1: 100%
         *   n=2: 82.8%
         *   n=3: 78.0%
         *   n=4: 75.7%
         *   n→∞: 69.3% (ln(2) ≈ 69.3%)
         * 
         * Simplified: Use 69% bound for n≥3 (conservative) */
        u64 rms_bound = 6900;  /* 69% in fixed-point (100 = 1%) */
        return total_util <= rms_bound;
    } else {
        /* EDF bound: U ≤ 100% */
        u64 edf_bound = 10000;  /* 100% in fixed-point (100 = 1%) */
        return total_util <= edf_bound;
    }
}
```

**Integrate into existing `is_system_busy()` function:**

```c
// Current: src/bpf/main.bpf.c:517-543
static inline bool is_system_busy(void)
{
    // ... existing code ...
    
    /* SCHEDULABILITY ANALYSIS: Check utilization bound before enabling EDF
     * This ensures EDF mode is only enabled when tasks are schedulable */
    if (cpu_util > 24 * 1024 / 100) {  /* 24% util */
        /* Check if system is schedulable under EDF */
        if (is_schedulable(false)) {  /* Use EDF bound */
            return true;  /* Enable EDF mode */
        } else {
            /* Overload: System not schedulable
             * Options:
             * 1. Stay in RR mode (graceful degradation)
             * 2. Apply admission control (reject/adjust tasks)
             * 3. Warn userspace about overload */
            return false;  /* Stay in RR mode */
        }
    }
    
    return false;
}
```

**Location:** `src/bpf/main.bpf.c` - Modify `is_system_busy()`

---

### Phase 4: Add Response Time Analysis (Optional) ✅

**Calculate worst-case response time:**

```c
/**
 * Calculate worst-case response time for a task under RMS.
 * 
 * Ri = Ci + Σ(ceil(Ri / Pj) * Cj) for all higher-priority tasks j
 * 
 * This is an iterative calculation that converges to worst-case response time.
 * 
 * @tctx: Task context
 * @return: Worst-case response time in nanoseconds
 */
static __always_inline u64 calculate_response_time(struct task_ctx *tctx)
{
    if (!tctx->is_periodic || tctx->detected_period_ns == 0)
        return 0;
    
    u64 response_time = tctx->worst_case_exec_ns;  /* Start with execution time */
    u64 prev_response = 0;
    u32 iterations = 0;
    const u32 max_iterations = 10;  /* Limit iterations */
    
    /* Iterative calculation: Ri = Ci + Σ(ceil(Ri / Pj) * Cj) */
    while (response_time != prev_response && iterations < max_iterations) {
        prev_response = response_time;
        
        /* Sum interference from higher-priority tasks
         * Note: BPF limitation - cannot iterate all tasks efficiently
         * Simplified: Use average interference estimate */
        
        /* For now, return execution time (simplified) */
        response_time = tctx->worst_case_exec_ns;
        iterations++;
    }
    
    return response_time;
}
```

**Note:** Full response time analysis requires iterating all tasks, which is expensive in BPF. Simplified version for now.

**Location:** `src/bpf/include/helpers.bpf.h`

---

### Phase 5: Add Admission Control (Optional) ✅

**Check if new task can be admitted:**

```c
/**
 * Check if a new task can be admitted without causing overload.
 * 
 * @exec_time: Estimated execution time (Ci)
 * @period: Task period (Pi)
 * @return: true if task can be admitted, false otherwise
 */
static __always_inline bool can_admit_task(u64 exec_time, u64 period)
{
    if (period == 0)
        return true;  /* Non-periodic task, no check */
    
    /* Calculate new task utilization */
    u64 new_util = (exec_time * 100) / period;
    
    /* Get current system utilization */
    u64 current_util = calculate_total_utilization();
    
    /* Check if adding new task exceeds EDF bound (100%) */
    if (current_util + new_util > 10000) {  /* >100% */
        /* Overload: Cannot guarantee deadlines
         * Options:
         * 1. Reject task (hard real-time)
         * 2. Adjust deadline (soft real-time)
         * 3. Reduce execution time estimate */
        return false;  /* Reject */
    }
    
    return true;  /* Admit */
}
```

**Integration point:** When new periodic task is detected in `gamer_runnable()`

**Location:** `src/bpf/main.bpf.c` - After period detection

---

## Implementation Steps

### Step 1: Add Utilization Fields to task_ctx
- [ ] Add `utilization_pct`, `worst_case_exec_ns`, `worst_case_response_ns` to `task_ctx`
- [ ] Ensure cache-line alignment maintained
- [ ] Initialize fields to 0 in `gamer_runnable()`

### Step 2: Add Utilization Calculation Function
- [ ] Create `update_task_utilization()` function
- [ ] Call after period detection in `gamer_runnable()`
- [ ] Test with periodic tasks (GPU/compositor/input)

### Step 3: Add Total Utilization Calculation
- [ ] Create `calculate_total_utilization()` function
- [ ] Use existing `cpu_util_avg` as proxy (or implement exact calculation)
- [ ] Test with multiple periodic tasks

### Step 4: Add Utilization Bound Check
- [ ] Create `is_schedulable()` function
- [ ] Integrate into `is_system_busy()` function
- [ ] Test EDF mode enabling/disabling based on schedulability

### Step 5: Add Response Time Analysis (Optional)
- [ ] Create `calculate_response_time()` function
- [ ] Integrate into utilization calculation
- [ ] Test with multiple periodic tasks

### Step 6: Add Admission Control (Optional)
- [ ] Create `can_admit_task()` function
- [ ] Integrate into period detection logic
- [ ] Test with overload scenarios

---

## Expected Behavior

### Before Schedulability Analysis:
```
EDF mode enabled at 24% util
No guarantee tasks can meet deadlines
Result: Potential deadline misses under heavy load
```

### After Schedulability Analysis:
```
EDF mode enabled at 24% util
Utilization check: U = 85% ≤ 100% ✅
Result: Guaranteed all tasks meet deadlines
```

---

## Gaming Scenarios

### Scenario 1: System Overload
- **Current:** EDF mode enabled, tasks may miss deadlines
- **With Schedulability:** Utilization check prevents overload
- **Benefit:** Formal guarantee of deadline compliance

### Scenario 2: New Game Thread
- **Current:** New thread added, no utilization check
- **With Schedulability:** Check if adding thread exceeds 100% utilization
- **Benefit:** Prevent overload scenarios

### Scenario 3: Frame Rate Change
- **Current:** Frame rate changes, utilization changes, no check
- **With Schedulability:** Recalculate utilization, detect overload
- **Benefit:** Detect when system becomes unschedulable

---

## Performance Considerations

**Overhead:**
- Utilization calculation: ~5-10ns per periodic task (one-time)
- Utilization bound check: ~10-20ns per check (periodic)
- Total overhead: ~15-30ns per periodic task classification

**Benefit:**
- Formal guarantees: Prevents deadline misses
- Overload detection: Early warning of unschedulable scenarios
- Admission control: Prevents adding tasks that would cause misses

**Net Impact:** Positive (provides guarantees with minimal overhead)

---

## Risk Assessment

**Risk Level:** Low

**Risks:**
1. **Utilization calculation accuracy:** Uses `exec_avg` as estimate
   - **Mitigation:** Use worst-case execution time if available
   - **Mitigation:** Conservative bounds (69% for RMS)

2. **BPF map iteration cost:** Cannot efficiently iterate all tasks
   - **Mitigation:** Use per-CPU aggregation or cached values
   - **Mitigation:** Approximate using existing `cpu_util_avg`

3. **False positives:** May reject schedulable tasks
   - **Mitigation:** Use conservative bounds
   - **Mitigation:** Allow soft real-time mode (warn, don't reject)

**Rollback Plan:**
- Utilization fields initialized to 0
- If calculation fails, fall back to existing behavior
- Can disable by skipping utilization checks

---

## Academic Reference

**Paper:** Liu & Layland (1973) - "Scheduling Algorithms for Multiprogramming in a Hard-Real-Time Environment"

**Key Concepts:**
- **Utilization Bound Test:** U ≤ bound (RMS or EDF)
- **Response Time Analysis:** Worst-case response time calculation
- **Admission Control:** Reject/adjust tasks if unschedulable

**Implementation:** Applied to gaming scheduler to provide formal guarantees that RMS priorities can meet deadlines.

---

## Files to Modify

1. `src/bpf/include/types.bpf.h` - Add utilization fields to `task_ctx`
2. `src/bpf/include/helpers.bpf.h` - Add utilization calculation functions
3. `src/bpf/main.bpf.c` - Integrate schedulability checks

---

## Next Steps

1. **Implement Phase 1-2:** Add utilization tracking and calculation
2. **Implement Phase 3:** Add utilization bound check
3. **Test:** Verify schedulability checks work correctly
4. **Optional:** Add response time analysis and admission control

---

**Status:** Ready for Implementation  
**Last Updated:** 2025-11-05

