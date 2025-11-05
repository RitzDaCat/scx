# Frame Rate Detection Analysis

**Date:** 2025-01-28  
**Question:** Can we determine the FPS that the game is running at for RMS priority assignment?

---

## Current State

### ❌ **Direct Frame Rate Detection: NOT AVAILABLE**

**Problem:**
1. **Page Flip Hook Disabled**: `drm_mode_page_flip` fentry hook is commented out
   - Reason: Not available in all kernel configurations (not exported in kernel BTF)
   - Impact: Cannot detect actual frame presentation events

2. **Frame Timing Variables Not Updated**:
   ```c
   volatile u64 last_page_flip_ns;      /* Declared but NOT updated */
   volatile u64 frame_interval_ns;       /* Declared but NOT updated */
   volatile u64 frame_count;              /* Declared but NOT updated */
   ```
   - Comment in code: "Frame timing updates removed from fentry hooks due to BPF backend limitations"
   - These variables exist but are never written to

---

## Alternative Detection Methods

### [STATUS: IMPLEMENTED] **Method 1: GPU Thread Wakeup Frequency** (Available)

**What We Have:**
- GPU threads tracked with `wakeup_freq` (calculated from wakeup intervals)
- GPU submit threads wake up at frame rate (60-240Hz typically)
- Already classified: `tctx->is_gpu_submit`

**How It Works:**
```c
// GPU thread wakes up once per frame
// wakeup_freq is EMA of wakeup frequency
u64 frame_period_ns = 1000000000ULL / tctx->wakeup_freq;  // Convert to period
```

**Pros:**
- [IMPLEMENTED] Already tracked for all GPU threads
- [IMPLEMENTED] Accurately reflects game frame rate (GPU submits once per frame)
- [IMPLEMENTED] No additional hooks needed

**Cons:**
- [NOTE] Requires GPU thread to be classified first
- [NOTE] May lag frame rate changes slightly (EMA smoothing)

**Accuracy:** High - GPU threads wake up once per frame, so frequency = frame rate

---

### [STATUS: IMPLEMENTED] **Method 2: Compositor Operation Frequency** (Available)

**What We Have:**
- Compositor threads tracked with `operation_freq_hz` (from DRM operations)
- Compositor operations correlate with frame presentation
- Already tracked: `compositor_thread_info.operation_freq_hz`

**How It Works:**
```c
// Compositor operations (setcrtc, setplane) happen at frame rate
// info->operation_freq_hz is EMA of operation frequency
u64 frame_period_ns = 1000000000ULL / info->operation_freq_hz;
```

**Pros:**
- [IMPLEMENTED] Already tracked for compositor threads
- [IMPLEMENTED] Reflects display refresh rate (compositor matches display)

**Cons:**
- [NOTE] Reflects display refresh rate, not game render rate
- [NOTE] VSync OFF: May not match game FPS (game renders faster than display)

**Accuracy:** Medium - Reflects display refresh rate, not necessarily game FPS

---

### [NOTE] **Method 3: Frame Window Activity** (Indirect)

**What We Have:**
- `win_frame_ns_total` - Total time spent in frame windows
- `timer_elapsed_ns_total` - Total timer elapsed time
- Ratio gives frame window percentage

**How It Works:**
```c
// Frame window percentage doesn't directly give FPS
// But can infer activity level
u64 frame_window_pct = (win_frame_ns_total * 100) / timer_elapsed_ns_total;
```

**Pros:**
- [IMPLEMENTED] Already tracked
- [IMPLEMENTED] Indicates frame activity level

**Cons:**
- ❌ Doesn't give actual FPS
- ❌ Only indicates activity, not rate

**Accuracy:** Low - Not useful for FPS detection

---

## Recommended Approach: GPU Thread Wakeup Frequency

### [STATUS: IMPLEMENTED] **Best Method: Use GPU Thread `wakeup_freq`**

**Why:**
1. **Direct Correlation**: GPU threads wake up once per frame
2. **Already Tracked**: `wakeup_freq` is EMA-smoothed wakeup frequency
3. **High Accuracy**: Matches game frame rate (not display refresh rate)
4. **Works for All Games**: Any game with GPU threads will have this

**Implementation:**
```c
// In task_dl_with_ctx_cached() for GPU threads
if (tctx->is_gpu_submit && tctx->wakeup_freq > 0) {
    // Convert wakeup frequency to frame period
    u64 frame_period_ns = 1000000000ULL / tctx->wakeup_freq;
    
    // Calculate RMS priority from period
    u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
    
    // Use RMS priority if higher than current boost_shift
    if (rms_priority > tctx->boost_shift) {
        // Use RMS priority for deadline calculation
        u64 boosted_exec = tctx->exec_runtime >> rms_priority;
        deadline = p->scx.dsq_vtime + boosted_exec;
    }
}
```

**Frame Rate Mapping:**
```
wakeup_freq = 240 → frame_period = 4.17ms → RMS priority = 7 (240Hz)
wakeup_freq = 144 → frame_period = 6.94ms → RMS priority = 6 (144Hz)
wakeup_freq = 120 → frame_period = 8.33ms → RMS priority = 6 (120Hz)
wakeup_freq = 60  → frame_period = 16.67ms → RMS priority = 5 (60Hz)
```

---

## Implementation Details

### Step 1: Add RMS Priority Calculation

```c
static inline u8 calculate_rms_priority_from_period(u64 period_ns)
{
    // RMS: Shorter period = higher priority
    
    if (period_ns <= 4167000ULL)      // ≤4.17ms (240Hz+)
        return 7;
    else if (period_ns <= 6940000ULL)  // ≤6.94ms (144Hz)
        return 6;
    else if (period_ns <= 8333000ULL)  // ≤8.33ms (120Hz)
        return 6;
    else if (period_ns <= 16667000ULL)  // ≤16.67ms (60Hz)
        return 5;
    else                               // >60Hz (low FPS)
        return 4;
}
```

### Step 2: Use in Deadline Calculation

```c
// In task_dl_with_ctx_cached() for GPU threads
if (likely(tctx->is_gpu_submit)) {
    u8 effective_priority = tctx->boost_shift;  // Default to classified priority
    
    // Calculate RMS priority from wakeup frequency
    if (tctx->wakeup_freq > 0) {
        u64 frame_period_ns = 1000000000ULL / tctx->wakeup_freq;
        u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
        
        // Use higher priority (RMS or classified)
        effective_priority = MAX(effective_priority, rms_priority);
    }
    
    u64 boosted_exec = tctx->exec_runtime >> effective_priority;
    deadline = p->scx.dsq_vtime + boosted_exec;
}
```

---

## Accuracy Assessment

### GPU Thread Wakeup Frequency

**Expected Accuracy:**
- **High-FPS (240Hz+)**: [IMPLEMENTED] Accurate (wakeup_freq directly measures frame rate)
- **Medium-FPS (60-144Hz)**: [IMPLEMENTED] Accurate (matches frame rate)
- **Low-FPS (<60Hz)**: [IMPLEMENTED] Accurate (still tracks correctly)

**Limitations:**
- [NOTE] EMA smoothing may lag rapid FPS changes (~1-2 frames)
- [NOTE] Requires GPU thread classification (automatic via fentry hooks)
- [NOTE] Doesn't work if game doesn't use GPU threads (rare)

**Conclusion:** [STATUS: IMPLEMENTED] **Suitable for RMS priority assignment**

---

## Alternative: Future Enhancement

### Enable Page Flip Hook (Future Work)

**If kernel support improves:**
1. Re-enable `drm_mode_page_flip` hook
2. Update `last_page_flip_ns` and `frame_interval_ns` from hook
3. Use compositor frame interval for RMS priority

**Benefits:**
- Direct frame presentation detection
- More accurate than wakeup frequency
- Works even if GPU threads aren't classified

**Current Status:** ❌ Not available (kernel BTF limitation)

---

## Conclusion

### [STATUS: IMPLEMENTED] **YES - We Can Determine Game FPS**

**Method:** Use GPU thread `wakeup_freq` (already tracked)

**Accuracy:** High - GPU threads wake up once per frame, so frequency = frame rate

**Implementation:**
1. Calculate frame period from `wakeup_freq`
2. Map period to RMS priority
3. Use RMS priority for deadline calculation

**Expected Result:**
- 240Hz games → Priority 7 (highest)
- 144Hz games → Priority 6
- 120Hz games → Priority 6
- 60Hz games → Priority 5

**Ready to Implement:** [IMPLEMENTED] Yes - All required data is already available

