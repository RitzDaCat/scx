# Page Flip Detection: VSync Mode Compatibility Analysis

**Date:** 2025-01-XX  
**Question:** How does page flip detection work with different VSync modes (VSync ON, Mailbox, VSync OFF)?

---

## Executive Summary

**✅ Page Flip Hook Works for ALL VSync Modes**

The `drm_mode_page_flip` function is called **regardless of VSync mode**. VSync mode only affects **when** the page flip happens, not **if** it happens.

**Key Insight:** `drm_mode_page_flip` is the kernel API that swaps frame buffers. It's called by the compositor in all modes, but the kernel schedules it differently based on the present mode.

---

## How Page Flip Works

### **What is `drm_mode_page_flip`?**

`drm_mode_page_flip` is a DRM (Direct Rendering Manager) kernel function that:
1. Swaps the front buffer (currently displayed) with the back buffer (newly rendered frame)
2. Schedules the swap to happen at the appropriate time (based on VSync mode)
3. Returns immediately (non-blocking) - actual flip happens later

**Function Signature:**
```c
int drm_mode_page_flip(struct drm_device *dev,
                       struct drm_crtc *crtc,
                       struct drm_framebuffer *fb,
                       uint32_t flags,
                       struct drm_modeset_acquire_ctx *ctx)
```

**Our Hook:**
```c
SEC("fentry/drm_mode_page_flip")
int BPF_PROG(detect_compositor_page_flip, ...)
```

This hook fires **every time** `drm_mode_page_flip` is called, regardless of VSync mode.

---

## VSync Mode Behavior

### **1. VSync ON (FIFO Mode)**

**How it works:**
- Compositor calls `drm_mode_page_flip` for each frame
- Kernel **queues** the page flip request
- Page flip happens at the **next VSync boundary** (v-blank)
- If multiple flips are queued, they're presented in order (FIFO)

**Timeline:**
```
T+0ms:    Compositor calls drm_mode_page_flip(frame1)
T+0.1ms:  Hook fires ✅ (immediate)
T+4ms:    VSync fires (240Hz display)
T+4.1ms:  Kernel executes page flip → frame1 displayed
T+4.2ms:  Compositor calls drm_mode_page_flip(frame2)
T+4.3ms:  Hook fires ✅ (immediate)
T+8ms:    VSync fires → frame2 displayed
```

**Hook Behavior:**
- ✅ Fires **immediately** when compositor calls page flip
- ✅ Frequency: Matches frame rate (60-240Hz)
- ✅ Timing: Predictable (every VSync interval)

**Benefits for Our Hook:**
- **Predictable timing** - can optimize compositor boost before VSync
- **Consistent frequency** - matches display refresh rate
- **Easy to track** - regular intervals make VSync prediction possible

---

### **2. Mailbox Mode**

**How it works:**
- Compositor calls `drm_mode_page_flip` for each frame (may be faster than refresh rate)
- Kernel **queues** up to **one** pending page flip
- Page flip happens at the **next VSync boundary**
- If a new flip is requested before VSync, the **old pending flip is replaced** (latest frame wins)

**Timeline:**
```
T+0ms:    Compositor calls drm_mode_page_flip(frame1)
T+0.1ms:  Hook fires ✅ (immediate)
T+2ms:    Compositor calls drm_mode_page_flip(frame2) [replaces frame1]
T+2.1ms:  Hook fires ✅ (immediate)
T+4ms:    VSync fires → frame2 displayed (frame1 never shown)
```

**Hook Behavior:**
- ✅ Fires **immediately** when compositor calls page flip
- ⚠️ Frequency: May be **higher** than refresh rate (if GPU renders faster)
- ⚠️ Not all hooks correspond to displayed frames (some frames dropped)

**Benefits for Our Hook:**
- ✅ Still fires on every page flip request
- ✅ Can detect when compositor is ready (even if frame doesn't display)
- ⚠️ Need to track actual VSync events to know which frames displayed

**Challenges:**
- Hook fires more frequently than frames are displayed
- Need to correlate with VSync to know actual presentation timing

---

### **3. VSync OFF (Immediate Mode)**

**How it works:**
- Compositor calls `drm_mode_page_flip` for each frame
- Kernel **executes page flip immediately** (no VSync wait)
- Frame buffer swap happens as soon as possible
- May cause tearing (multiple frames displayed in one refresh cycle)

**Timeline:**
```
T+0ms:    Compositor calls drm_mode_page_flip(frame1)
T+0.1ms:  Hook fires ✅ (immediate)
T+0.15ms: Kernel executes page flip → frame1 displayed (immediate)
T+2ms:    Compositor calls drm_mode_page_flip(frame2)
T+2.1ms:  Hook fires ✅ (immediate)
T+2.15ms: Kernel executes page flip → frame2 displayed (immediate)
```

**Hook Behavior:**
- ✅ Fires **immediately** when compositor calls page flip
- ⚠️ Frequency: Matches GPU render rate (may be > refresh rate)
- ⚠️ Timing: **Asynchronous** - no VSync alignment
- ⚠️ Multiple page flips may happen during one display refresh cycle

**Benefits for Our Hook:**
- ✅ Fires on every page flip (no missed events)
- ✅ Lowest latency detection (immediate flip)
- ✅ Can detect high-FPS rendering (1000+ FPS)

**Challenges:**
- No predictable timing (can't optimize for VSync)
- Very high frequency (may fire >240Hz)
- Some frames may not be visible (display can't keep up)

---

## Hook Implementation Details

### **What Our Hook Detects:**

```c
SEC("fentry/drm_mode_page_flip")
int BPF_PROG(detect_compositor_page_flip, struct drm_crtc *crtc,
             struct drm_framebuffer *fb, u32 flags,
             struct drm_modeset_acquire_ctx *ctx)
{
    u32 tid = bpf_get_current_pid_tgid();
    
    /* Register compositor thread */
    register_compositor_thread(tid, COMPOSITOR_TYPE_UNKNOWN);
    
    /* OPTIMIZATION: Immediately boost compositor thread */
    struct task_ctx *tctx = try_lookup_task_ctx(bpf_get_current_task_btf());
    if (tctx && tctx->is_compositor) {
        tctx->exec_runtime = 0;  /* Reset vruntime for immediate scheduling */
    }
    
    return 0;  /* Don't interfere with page flip */
}
```

**What this detects:**
- ✅ **Compositor is ready** to present a frame
- ✅ **Frame presentation request** (regardless of when it actually displays)
- ✅ **Compositor activity** (useful for boosting compositor threads)

**What this does NOT detect:**
- ❌ Actual VSync timing (when frame is displayed)
- ❌ Frame drops (in mailbox mode, some frames never display)
- ❌ Tearing (in VSync OFF mode)

---

## Mode-Specific Considerations

### **VSync ON Mode:**

**Hook Behavior:** ✅ **Optimal**
- Fires at predictable intervals (matches refresh rate)
- Every hook corresponds to a displayed frame
- Can predict next VSync timing

**Optimization Strategy:**
- Pre-boost compositor ~1-2ms before expected VSync
- Use page flip frequency to estimate VSync timing
- Track frame presentation latency (submit → flip → VSync)

**Code Enhancement:**
```c
/* Track page flip intervals to predict VSync */
u64 now = bpf_ktime_get_ns();
u64 delta = now - info->last_page_flip_ts;
info->last_page_flip_ts = now;

/* Estimate next VSync (assuming consistent timing) */
if (delta > 0 && delta < 20000000ULL) {  /* < 20ms */
    info->estimated_vsync_interval = delta;
    info->next_vsync_estimate = now + delta;
}
```

---

### **Mailbox Mode:**

**Hook Behavior:** ⚠️ **Functional but needs refinement**
- Fires more frequently than frames are displayed
- Some hooks don't correspond to displayed frames (dropped frames)

**Optimization Strategy:**
- Still useful for boosting compositor (compositor is active)
- Track VSync events separately to know actual presentation
- Count page flips vs VSync events to detect frame drops

**Code Enhancement:**
```c
/* Track both page flips and VSync events */
volatile u64 page_flip_count;
volatile u64 vsync_event_count;

/* In page flip hook: */
__sync_fetch_and_add(&page_flip_count, 1);

/* In VSync hook (if we add one): */
__sync_fetch_and_add(&vsync_event_count, 1);

/* Frame drop rate = (page_flip_count - vsync_event_count) / page_flip_count */
```

---

### **VSync OFF Mode:**

**Hook Behavior:** ✅ **Works but limited optimization**
- Fires very frequently (matches GPU render rate)
- No VSync timing to optimize around
- Asynchronous - can't predict timing

**Optimization Strategy:**
- Use hook to boost compositor immediately (already doing this)
- Can't optimize for VSync (doesn't exist in this mode)
- Focus on minimizing compositor processing latency

**Code Enhancement:**
```c
/* In VSync OFF mode, just boost immediately */
if (tctx && tctx->is_compositor) {
    tctx->exec_runtime = 0;  /* Immediate boost */
    /* Don't try to predict VSync - there isn't one */
}
```

---

## Detection Accuracy by Mode

| Mode | Hook Fires | Frames Displayed | Accuracy |
|------|------------|------------------|----------|
| **VSync ON** | Every frame | Every hook | ✅ **100%** - Perfect correlation |
| **Mailbox** | Every frame | Some hooks | ⚠️ **Variable** - Some frames dropped |
| **VSync OFF** | Every frame | Some hooks | ⚠️ **Variable** - Display may miss frames |

**Key Insight:** The hook always fires when the compositor requests a page flip. VSync mode affects whether that frame actually gets displayed.

---

## Practical Recommendations

### **Hook Implementation:**

```c
SEC("fentry/drm_mode_page_flip")
int BPF_PROG(detect_compositor_page_flip, struct drm_crtc *crtc,
             struct drm_framebuffer *fb, u32 flags,
             struct drm_modeset_acquire_ctx *ctx)
{
    u32 tid = bpf_get_current_pid_tgid();
    
    /* Register compositor thread (works in all modes) */
    register_compositor_thread(tid, COMPOSITOR_TYPE_UNKNOWN);
    
    /* Boost compositor immediately (works in all modes) */
    struct task_ctx *tctx = try_lookup_task_ctx(bpf_get_current_task_btf());
    if (tctx && tctx->is_compositor) {
        tctx->exec_runtime = 0;  /* Reset vruntime for immediate scheduling */
    }
    
    /* Track page flip timing for VSync prediction (VSync ON/Mailbox) */
    struct compositor_thread_info *info = bpf_map_lookup_elem(&compositor_threads_map, &tid);
    if (info) {
        u64 now = bpf_ktime_get_ns();
        u64 delta = now - info->last_operation_ts;
        info->last_operation_ts = now;
        
        /* Only track VSync timing if interval is reasonable (VSync ON/Mailbox) */
        if (delta > 0 && delta < 20000000ULL) {  /* < 20ms (reasonable for 60-240Hz) */
            info->operation_freq_hz = (u32)(1000000000ULL / delta);
        }
    }
    
    return 0;
}
```

### **Mode Detection:**

We can infer VSync mode from page flip frequency:
- **VSync ON:** Frequency matches refresh rate (60-240Hz)
- **Mailbox:** Frequency may exceed refresh rate (GPU renders faster)
- **VSync OFF:** Frequency very high (matches GPU render rate, may be >240Hz)

**Code to detect mode:**
```c
/* Infer VSync mode from page flip frequency */
u32 freq = get_compositor_freq(tid);
if (freq >= 50 && freq <= 250) {
    /* Likely VSync ON - frequency matches refresh rate */
} else if (freq > 250) {
    /* Likely VSync OFF or Mailbox - frequency exceeds refresh rate */
} else {
    /* Unknown or irregular timing */
}
```

---

## Conclusion

**✅ Page Flip Hook Works for ALL VSync Modes**

**Key Points:**
1. `drm_mode_page_flip` is called regardless of VSync mode
2. Hook fires immediately when compositor requests page flip
3. VSync mode affects **when** flip happens, not **if** hook fires
4. Hook is useful for boosting compositor in all modes
5. VSync-aware optimizations work best in VSync ON mode

**Recommendation:**
- ✅ **Implement the hook** - it works in all modes
- ✅ **Boost compositor immediately** - beneficial in all modes
- ⚠️ **VSync prediction** - only useful in VSync ON/Mailbox modes
- ⚠️ **Frame drop tracking** - useful in Mailbox/VSync OFF modes

**Expected Benefits:**
- **VSync ON:** Optimal - perfect timing correlation
- **Mailbox:** Good - detect compositor activity, some frames may not display
- **VSync OFF:** Good - detect compositor activity, can't predict timing

The hook is **universally useful** but provides **more optimization opportunities** in VSync ON mode due to predictable timing.

