# Rate Monotonic Scheduling (RMS) Implementation Plan

**Date:** 2025-11-05  
**Priority:** ⚠️ **HIGHEST** - Direct gaming and low-latency input benefits  
**Status:** Planning → Implementation

---

## Executive Summary

**Goal:** Implement Rate Monotonic Scheduling to assign priorities based on task periods (shorter period = higher priority).

**Expected Impact:**
- **240Hz games** get higher priority than 60Hz games
- **8000Hz mice** get higher priority than 1000Hz mice
- **~1ms** improvement in frame timing consistency
- **~50-100ns** faster input processing per event

**Effort:** Low-Medium  
**Risk:** Low (additive enhancement)

---

## Implementation Phases

### Phase 1: Data Structure Changes ✅

**Add RMS fields to `task_ctx` structure:**
```c
struct task_ctx {
    // ... existing fields ...
    
    /* Rate Monotonic Scheduling */
    u8 rms_priority;            // RMS priority (0-7, shorter period = higher)
    u64 detected_period_ns;     // Detected task period for periodic tasks
    bool is_periodic;            // Is this a confirmed periodic task?
};
```

**Location:** `src/bpf/include/types.bpf.h`

**Cache Line Consideration:**
- RMS fields are warm (accessed during classification and deadline calculation)
- Place in CACHE LINE 1 (hot path) or CACHE LINE 2 (warm path)
- `rms_priority` is hot (used in deadline calculation)
- `detected_period_ns` and `is_periodic` are warm (used during classification)

---

### Phase 2: RMS Priority Calculation Function ✅

**Create function to calculate RMS priority from period:**
```c
/**
 * Calculate Rate Monotonic Scheduling priority from task period.
 * 
 * RMS Principle: Shorter period = Higher priority
 * 
 * Priority mapping:
 * - Priority 7: ≤125µs (8000Hz+ input)
 * - Priority 6: ≤250µs (4000Hz input)
 * - Priority 5: ≤4.17ms (240Hz+ frames)
 * - Priority 4: ≤8.33ms (120Hz frames)
 * - Priority 3: ≤16.67ms (60Hz frames)
 * - Priority 2: >16.67ms (lower frame rates)
 * 
 * @period_ns: Task period in nanoseconds
 * @return: RMS priority (0-7, higher = more priority)
 */
static inline u8 calculate_rms_priority_from_period(u64 period_ns)
{
    // RMS: Shorter period = higher priority
    if (period_ns <= 125000ULL)        // ≤125µs (8000Hz+ input)
        return 7;
    else if (period_ns <= 250000ULL)   // ≤250µs (4000Hz input)
        return 6;
    else if (period_ns <= 4167000ULL)  // ≤4.17ms (240Hz+ frames)
        return 5;
    else if (period_ns <= 8333000ULL)  // ≤8.33ms (120Hz frames)
        return 4;
    else if (period_ns <= 16667000ULL) // ≤16.67ms (60Hz frames)
        return 3;
    else                               // >60Hz
        return 2;
}
```

**Location:** `src/bpf/include/helpers.bpf.h` or new `src/bpf/include/rms.bpf.h`

**Performance:** O(1) - Simple conditional chain, ~5-10ns

---

### Phase 3: Period Detection for GPU/Compositor Threads ✅

**Detect frame period from existing `frame_interval_ns`:**

**Location:** `src/bpf/main.bpf.c` - `gamer_runnable()` or deadline calculation

**Logic:**
```c
// For GPU/compositor threads, use frame_interval_ns as period
if (tctx->is_gpu_submit || tctx->is_compositor) {
    u64 frame_period = frame_interval_ns;
    if (frame_period > 0 && frame_period < 100000000ULL) {  // Valid: 10ms - 10s
        tctx->detected_period_ns = frame_period;
        tctx->is_periodic = true;
        tctx->rms_priority = calculate_rms_priority_from_period(frame_period);
    }
}
```

**Notes:**
- `frame_interval_ns` is already tracked (EMA of inter-frame time)
- Valid range: 4.17ms (240Hz) to 16.67ms (60Hz) for gaming
- Set `is_periodic = true` when frame period is detected

---

### Phase 4: Period Detection for Input Handlers ✅

**Detect input polling period from input trigger rate:**

**Location:** `src/bpf/main.bpf.c` - Input handler detection or `gamer_runnable()`

**Logic:**
```c
// For input handlers, calculate period from input polling rate
if (tctx->is_input_handler) {
    // Calculate period from input trigger rate
    // input_trigger_rate is events per second (Hz)
    u64 input_period = 0;
    if (input_trigger_rate > 0) {
        input_period = 1000000000ULL / input_trigger_rate;  // Period in ns
    }
    
    // Fallback: Use wakeup_freq if input_trigger_rate not available
    if (input_period == 0 && tctx->wakeup_freq > 0) {
        // wakeup_freq is wakeups per 100ms, convert to period
        // period_ns = 100000000ULL / (wakeup_freq / 10)
        // Simplified: period_ns = 1000000000ULL / (wakeup_freq * 10)
        input_period = 1000000000ULL / (tctx->wakeup_freq * 10);
    }
    
    if (input_period > 0 && input_period < 10000000ULL) {  // Valid: 125µs - 10ms
        tctx->detected_period_ns = input_period;
        tctx->is_periodic = true;
        tctx->rms_priority = calculate_rms_priority_from_period(input_period);
    }
}
```

**Notes:**
- Input polling rates: 1000Hz (1ms), 4000Hz (250µs), 8000Hz (125µs)
- Use `input_trigger_rate` if available, fallback to `wakeup_freq`
- Set `is_periodic = true` when input period is detected

---

### Phase 5: Integrate RMS Priority into Boost Calculation ✅

**Modify `recompute_boost_shift()` to consider RMS priority:**

**Location:** `src/bpf/main.bpf.c` - `recompute_boost_shift()` function

**Current Logic:**
```c
// Current: Fixed boost_shift based on classification
if (tctx->is_input_handler)
    tctx->boost_shift = 7;  // Fixed priority
else if (tctx->is_gpu_submit)
    tctx->boost_shift = 6;  // Fixed priority
```

**New Logic:**
```c
// Enhanced: Use MAX(fixed_boost, rms_priority) for periodic tasks
if (tctx->is_input_handler) {
    u8 base_boost = 7;  // Base priority for input handlers
    if (tctx->is_periodic && tctx->rms_priority > 0) {
        // Use RMS priority if higher than base (shouldn't happen for input, but allows refinement)
        tctx->boost_shift = MAX(base_boost, tctx->rms_priority);
    } else {
        tctx->boost_shift = base_boost;
    }
} else if (tctx->is_gpu_submit) {
    u8 base_boost = 6;  // Base priority for GPU threads
    if (tctx->is_periodic && tctx->rms_priority > 0) {
        // RMS priority: 240Hz (4.17ms) → 5, but we want 240Hz > 60Hz
        // So: Use RMS priority directly for periodic tasks
        tctx->boost_shift = MAX(base_boost, tctx->rms_priority);
    } else {
        tctx->boost_shift = base_boost;
    }
} else if (tctx->is_compositor) {
    u8 base_boost = 5;  // Base priority for compositor
    if (tctx->is_periodic && tctx->rms_priority > 0) {
        tctx->boost_shift = MAX(base_boost, tctx->rms_priority);
    } else {
        tctx->boost_shift = base_boost;
    }
}
```

**Alternative Approach (Recommended):**
```c
// For periodic tasks, RMS priority becomes the primary priority
// For non-periodic tasks, use classification-based priority

if (tctx->is_periodic && tctx->rms_priority > 0) {
    // Periodic task: Use RMS priority as primary
    // RMS ensures shorter period = higher priority
    tctx->boost_shift = tctx->rms_priority;
} else {
    // Non-periodic task: Use classification-based priority
    if (tctx->is_input_handler)
        tctx->boost_shift = 7;
    else if (tctx->is_gpu_submit)
        tctx->boost_shift = 6;
    // ... etc
}
```

**Recommendation:** Use alternative approach - RMS priority for periodic tasks, classification priority for others.

---

### Phase 6: Update Deadline Calculation ✅

**Modify `task_dl_with_ctx_cached()` to use RMS priority:**

**Location:** `src/bpf/main.bpf.c` - `task_dl_with_ctx_cached()` function

**Current Logic:**
```c
// Uses tctx->boost_shift directly
u64 boosted_exec = tctx->exec_runtime >> tctx->boost_shift;
```

**New Logic:**
```c
// boost_shift already includes RMS priority (from recompute_boost_shift)
// No changes needed - RMS priority is already integrated!
```

**Note:** Since RMS priority is integrated into `boost_shift` during classification, deadline calculation automatically benefits.

---

### Phase 7: Testing & Validation ✅

**Test Cases:**

1. **240Hz Game Priority**
   - Run 240Hz game
   - Verify: GPU thread `rms_priority = 5` (4.17ms period)
   - Verify: `boost_shift >= 5` (RMS priority applied)
   - Verify: Higher priority than 60Hz game

2. **8000Hz Mouse Priority**
   - Use 8000Hz mouse
   - Verify: Input handler `rms_priority = 7` (125µs period)
   - Verify: `boost_shift = 7` (highest priority)
   - Verify: Higher priority than 1000Hz mouse

3. **Frame Rate Change**
   - Start at 60Hz, change to 240Hz
   - Verify: RMS priority updates automatically
   - Verify: `boost_shift` increases

4. **Periodic Task Detection**
   - Verify: GPU/compositor threads marked as periodic
   - Verify: Input handlers marked as periodic
   - Verify: Non-periodic tasks not affected

---

## Implementation Steps

### Step 1: Add RMS Fields to task_ctx
- [ ] Add `rms_priority`, `detected_period_ns`, `is_periodic` to `task_ctx`
- [ ] Ensure cache-line alignment maintained
- [ ] Initialize fields to 0/false in `gamer_runnable()`

### Step 2: Create RMS Priority Calculation Function
- [ ] Create `calculate_rms_priority_from_period()` function
- [ ] Add to `helpers.bpf.h` or create `rms.bpf.h`
- [ ] Test with known periods (125µs, 4.17ms, 16.67ms)

### Step 3: Detect Period for GPU/Compositor Threads
- [ ] Add period detection in `gamer_runnable()` or deadline calculation
- [ ] Use `frame_interval_ns` as period source
- [ ] Set `is_periodic = true` when period detected
- [ ] Calculate `rms_priority` from period

### Step 4: Detect Period for Input Handlers
- [ ] Add period detection for input handlers
- [ ] Use `input_trigger_rate` or `wakeup_freq` as period source
- [ ] Set `is_periodic = true` when period detected
- [ ] Calculate `rms_priority` from period

### Step 5: Integrate RMS into Boost Calculation
- [ ] Modify `recompute_boost_shift()` to use RMS priority for periodic tasks
- [ ] Use RMS priority as primary for periodic tasks
- [ ] Keep classification priority for non-periodic tasks

### Step 6: Testing
- [ ] Test with 240Hz game (verify priority 5)
- [ ] Test with 60Hz game (verify priority 3)
- [ ] Test with 8000Hz mouse (verify priority 7)
- [ ] Test with 1000Hz mouse (verify priority 4)
- [ ] Verify frame timing consistency improvement

---

## Performance Considerations

**Overhead:**
- Period detection: ~5-10ns per classification (one-time)
- RMS priority calculation: ~5-10ns per calculation (cached)
- Total overhead: ~10-20ns per periodic task classification

**Benefit:**
- Better priority assignment: 100-500ns improvement per scheduling decision
- Better frame delivery: ~1ms improvement in frame timing consistency
- Better input processing: ~50-100ns faster per input event

**Net Impact:** Positive (large benefit, minimal overhead)

---

## Risk Assessment

**Risk Level:** Low

**Risks:**
1. **Cache-line alignment:** Adding fields might break alignment
   - **Mitigation:** Verify with `_Static_assert` after changes

2. **Period detection accuracy:** Wrong period detected
   - **Mitigation:** Use existing `frame_interval_ns` (already validated)
   - **Mitigation:** Validate period ranges (125µs - 16.67ms)

3. **Priority inversion:** RMS priority might conflict with classification
   - **Mitigation:** Use MAX(base_boost, rms_priority) to ensure minimum priority

**Rollback Plan:**
- RMS fields initialized to 0/false (non-periodic)
- If RMS priority calculation fails, falls back to classification priority
- Can disable RMS by setting `is_periodic = false` for all tasks

---

## Success Metrics

**Before RMS:**
- 240Hz game priority: 6 (same as 60Hz)
- 8000Hz mouse priority: 7 (same as 1000Hz)
- Frame timing variance: ±2-3ms

**After RMS:**
- 240Hz game priority: 5 (higher than 60Hz)
- 8000Hz mouse priority: 7 (same, but could refine)
- Frame timing variance: ±1-2ms (improved)

**Validation:**
- Measure frame timing consistency (should improve ~1ms)
- Measure input latency (should improve ~50-100ns)
- Verify no regressions in non-periodic tasks

---

**Last Updated:** 2025-11-05  
**Status:** Ready for Implementation

