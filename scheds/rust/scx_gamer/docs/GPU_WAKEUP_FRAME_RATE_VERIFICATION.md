# GPU Wakeup vs Frame Rate: Accuracy Verification

**Date:** 2025-01-28  
**Question:** Are we certain GPU threads wake up once per frame and can we use `wakeup_freq` accurately for frame rate?

---

## Critical Analysis

### [NOTE] **Answer: NOT GUARANTEED - Requires Verification**

**Reality Check:**
- GPU submission thread wake-ups **may not** equal frame rate in all cases
- Multiple factors can affect correlation
- Requires validation per game/engine

---

## What We Actually Detect

### **Our Detection Method:**
```c
// Hook: fentry/drm_ioctl
// Detects: GPU command submission calls
// Examples: DRM_I915_GEM_EXECBUFFER2 (Intel), DRM_AMDGPU_CS (AMD)
```

**What This Means:**
- We detect **GPU command buffer submissions**
- Each submission typically represents **one frame's worth of commands**
- But **not guaranteed** to be 1:1 with displayed frames

---

## Scenarios Where Wake-Up ≠ Frame Rate

### ❌ **Scenario 1: Triple Buffering**

**How It Works:**
```
Game renders frame 1 → Submit to GPU buffer A
Game renders frame 2 → Submit to GPU buffer B (frame 1 still rendering)
Game renders frame 3 → Submit to GPU buffer C (frames 1-2 still in pipeline)
VSync fires → Display buffer A
```

**Wake-Up Pattern:**
- GPU thread wakes: 3 times per VSync period
- Frames displayed: 1 per VSync period
- **Ratio: 3:1** (not 1:1)

**Impact:**
- `wakeup_freq` = 3× actual frame rate
- RMS priority would be **too high** (wrong period)

**Mitigation:**
- Could detect triple buffering by correlating with page flips
- Or cap wakeup_freq to display refresh rate

---

### ❌ **Scenario 2: Multiple GPU Threads**

**How It Works:**
```
Game has 2 GPU submission threads:
- Thread A: Submits even frames
- Thread B: Submits odd frames
```

**Wake-Up Pattern:**
- Each thread wakes: 60Hz (120Hz total)
- Frames displayed: 60Hz
- **Ratio: 2:1** (not 1:1)

**Impact:**
- `wakeup_freq` per thread = 60Hz
- But total submissions = 120Hz
- RMS priority might be correct for individual threads, but system-level frequency is wrong

**Mitigation:**
- Track all GPU threads, sum frequencies
- Or use per-thread priority (current approach is per-thread)

---

### ❌ **Scenario 3: VSync OFF / High-FPS Rendering**

**How It Works:**
```
Game renders at 500 FPS
Display refresh: 240Hz
VSync: OFF
```

**Wake-Up Pattern:**
- GPU thread wakes: 500Hz
- Frames displayed: 240Hz (display limited)
- **Ratio: ~2:1** (not 1:1)

**Impact:**
- `wakeup_freq` = 500Hz
- Actual displayed frames = 240Hz
- RMS priority would be **too high** (faster period than display)

**Mitigation:**
- Cap wakeup_freq to display refresh rate
- Or detect display refresh rate separately

---

### [NOTE] **Scenario 4: Frame Skipping / Dropped Frames**

**How It Works:**
```
Game targets 60 FPS
Under load: Drops to 45 FPS
GPU submissions: Still 60/sec (attempts)
Frames displayed: 45/sec (actual)
```

**Wake-Up Pattern:**
- GPU thread wakes: 60Hz (attempts)
- Frames displayed: 45Hz (actual)
- **Ratio: 1.33:1** (not 1:1)

**Impact:**
- `wakeup_freq` = 60Hz (too optimistic)
- RMS priority might be correct for attempted rate, but not actual

**Mitigation:**
- This is actually **desired behavior** (prioritize based on target rate)
- But could mislead if frame drops are severe

---

## When Wake-Up DOES Equal Frame Rate

### [STATUS: IMPLEMENTED] **Scenario 1: VSync ON, Single GPU Thread**

**How It Works:**
```
Game renders at display refresh rate
VSync: ON
Single GPU submission thread
```

**Wake-Up Pattern:**
- GPU thread wakes: Matches VSync rate (60/120/240Hz)
- Frames displayed: Same rate
- **Ratio: 1:1** [STATUS: IMPLEMENTED] **Accuracy:** High

---

### [STATUS: IMPLEMENTED] **Scenario 2: Frame-Locked Rendering**

**How It Works:**
```
Game engine waits for frame completion before submitting next
No triple buffering
One submission per displayed frame
```

**Wake-Up Pattern:**
- GPU thread wakes: Once per frame
- Frames displayed: Same rate
- **Ratio: 1:1** [STATUS: IMPLEMENTED] **Accuracy:** High

---

## Recommendations

### **Option 1: Conservative Approach** (Recommended)

**Cap `wakeup_freq` to Display Refresh Rate:**
```c
// Detect display refresh rate (from compositor or config)
u32 display_refresh_hz = detect_display_refresh_rate();  // e.g., 240Hz

// Cap GPU thread wakeup_freq to display rate
if (tctx->is_gpu_submit) {
    u32 capped_freq = MIN(tctx->wakeup_freq, display_refresh_hz * 10);  // *10 for per-100ms
    u64 frame_period_ns = 1000000000ULL / capped_freq;
    u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
}
```

**Pros:**
- Prevents over-prioritization from triple buffering
- Works for VSync ON/OFF
- Conservative (safe)

**Cons:**
- Doesn't help VSync OFF high-FPS games (capped to display rate)
- Requires display refresh rate detection

---

### **Option 2: Per-Thread Approach** (Current, Acceptable)

**Use `wakeup_freq` As-Is:**
```c
// Use GPU thread wakeup_freq directly
if (tctx->is_gpu_submit && tctx->wakeup_freq > 0) {
    u64 frame_period_ns = 1000000000ULL / tctx->wakeup_freq;
    u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
}
```

**Pros:**
- Simple, no additional detection needed
- Works for single-thread GPU games
- Accurate for VSync ON scenarios

**Cons:**
- May over-prioritize triple-buffered games
- May over-prioritize VSync OFF high-FPS games
- Inaccurate for multi-thread GPU games

**Assessment:** [NOTE] **Acceptable but not perfect**

---

### **Option 3: Compositor-Based Detection** (Most Accurate)

**Use Compositor Operation Frequency:**
```c
// Compositor operations match display refresh rate
if (tctx->is_compositor) {
    u32 comp_freq = compositor_operation_freq_hz;  // From compositor_detect
    u64 frame_period_ns = 1000000000ULL / comp_freq;
    u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
}
```

**Pros:**
- Reflects actual display refresh rate
- Works regardless of game rendering method
- Most accurate for displayed frames

**Cons:**
- Only works for compositor threads (not GPU threads)
- Doesn't reflect game render rate (only display rate)

**Assessment:** [STATUS: IMPLEMENTED] **Best for compositor priority, not GPU priority**

---

## Verification Methods

### **Method 1: Correlation Testing**

**Test:**
1. Run game at known FPS (e.g., 60 FPS locked)
2. Measure GPU thread `wakeup_freq`
3. Measure actual frame rate (via page flips or frame counters)
4. Compare ratio

**Expected:**
- VSync ON: 1:1 ratio
- Triple buffering: 2:1 or 3:1 ratio
- VSync OFF high-FPS: Variable ratio

---

### **Method 2: Game-Specific Validation**

**Test Multiple Games:**
- Warframe (144Hz target)
- Splitgate (480Hz observed)
- Simple indie games (60Hz locked)

**Compare:**
- GPU `wakeup_freq` vs actual frame rate
- Identify patterns per game engine

---

## Conclusion

### [NOTE] **Not Guaranteed: Use With Caution**

**Reality:**
- GPU submission wake-ups **often** correlate with frame rate
- But **not always** 1:1 (triple buffering, multi-thread, VSync OFF)
- Accuracy depends on game engine implementation

**Recommendation:**
1. **Conservative Approach:** Cap `wakeup_freq` to display refresh rate
2. **Per-Thread Approach:** Use as-is, accept some inaccuracy
3. **Hybrid Approach:** Use GPU `wakeup_freq` for GPU threads, compositor freq for compositor threads

**For RMS Priority:**
- [STATUS: IMPLEMENTED] **Safe to use** if capped to display refresh rate
- [NOTE] **Use with caution** if using raw `wakeup_freq`
- [STATUS: IMPLEMENTED] **Best accuracy** from compositor operation frequency (for compositor threads)

**Final Answer:** **Not certain, but usable with caveats and conservative capping**

