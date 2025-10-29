# Complete Latency Chain: Game State Change → Monitor Display

**Date:** 2025-01-XX  
**Goal:** Minimize time from in-game target movement to visible on monitor  
**Focus:** End-to-end latency optimization for competitive gaming

---

## Executive Summary

**Total Latency Chain:** ~8-16ms from game state change to monitor display  
**Breakdown:**
- **Game Logic:** ~1-3ms (CPU-bound simulation/physics)
- **GPU Submission:** ~100-200µs (already optimized)
- **GPU Processing:** ~2-8ms (hardware, not controllable by scheduler)
- **GPU Interrupt:** ~50-200µs (optimized with boost 4)
- **Compositor:** ~200µs-1ms (optimized with boost 5, physical cores)
- **Page Flip:** ~100-200µs (detection pending)
- **Display Pipeline:** ~1-2ms (VSync, scanout, pixel response)

**Optimization Potential:** ~1-3ms reduction achievable through scheduler improvements

---

## Complete Latency Chain Breakdown

### **Stage 1: Game State Change**
**Location:** Game engine (userspace)  
**Latency:** ~0-1ms (depends on game loop timing)

**What Happens:**
1. Game logic processes input (aim, movement, etc.)
2. Physics simulation updates world state
3. Render thread prepares frame data

**Scheduler Impact:** ✅ Already optimized
- Game threads get foreground boost during input window
- Render threads get GPU boost (level 6)

**Optimization Opportunities:**
- ⚠️ **None** - Game logic is application-level, not scheduler-controlled

---

### **Stage 2: GPU Command Submission**
**Location:** GPU driver (kernel)  
**Latency:** ~100-200µs (already optimized)

**What Happens:**
1. Game thread calls OpenGL/Vulkan/DirectX API
2. Driver builds command buffer
3. `drm_ioctl` called to submit commands
4. **BPF Hook:** `detect_gpu_submit_drm` classifies thread
5. Commands queued to GPU

**Current Optimizations:**
- ✅ Fentry hook on `drm_ioctl` (<1ms detection latency)
- ✅ GPU threads get boost level 6 (8x priority)
- ✅ Physical core preference (no SMT)
- ✅ Fast path in deadline calculation (no window checks)

**Scheduler Impact:** ✅ **Optimal**

**Code Location:** `include/gpu_detect.bpf.h:191-213`

---

### **Stage 3: GPU Processing**
**Location:** GPU hardware  
**Latency:** ~2-8ms (hardware-dependent, not controllable)

**What Happens:**
1. GPU executes command buffer
2. Vertex/pixel shaders run
3. Frame rendered to GPU memory
4. Frame ready signal generated

**Scheduler Impact:** ❌ **Not controllable** - Hardware-only

**Optimization Opportunities:**
- ⚠️ **None** - Hardware processing time is fixed
- ✅ **Future:** Could optimize GPU workload scheduling (out of scope)

---

### **Stage 4: GPU Interrupt (Frame Completion)**
**Location:** GPU driver interrupt handler  
**Latency:** ~50-200µs (optimized with boost 4)

**What Happens:**
1. GPU generates interrupt when frame completes
2. Kernel interrupt handler executes
3. Interrupt thread wakes compositor
4. Frame buffer marked as ready

**Current Optimizations:**
- ✅ GPU interrupt detection (tracepoint-based)
- ✅ Boost level 4 (6x priority) - **RECENTLY INCREASED**
- ✅ Fast wakeup of compositor threads

**Scheduler Impact:** ✅ **Optimized** (just improved)

**Optimization Opportunities:**
- ✅ **DONE:** Increased boost from 2 to 4
- ⚠️ **Possible:** IRQ affinity pinning (ensure interrupts go to fast cores)

**Code Location:** `main.bpf.c:2488-2489`

---

### **Stage 5: Compositor Processing**
**Location:** Compositor (KWin/Mutter/etc.)  
**Latency:** ~200µs-1ms (optimized with boost 5, physical cores)

**What Happens:**
1. Compositor wakes on GPU interrupt
2. Reads frame from GPU memory
3. Applies window effects/compositing
4. Prepares frame for display
5. Calls `drm_mode_page_flip` to present frame

**Current Optimizations:**
- ✅ Compositor detection (`drm_mode_setcrtc`, `drm_mode_setplane`)
- ✅ Boost level 5 (7x priority) - **RECENTLY INCREASED**
- ✅ Physical core preference - **RECENTLY ADDED**
- ✅ Fast path in deadline calculation (boost_shift >= 5)

**Scheduler Impact:** ✅ **Optimized** (just improved)

**Optimization Opportunities:**
- ✅ **DONE:** Increased boost from 3 to 5
- ✅ **DONE:** Added physical core preference
- ⚠️ **Pending:** Page flip detection hook (`drm_mode_page_flip`)
- ⚠️ **Future:** VSync-aware scheduling (predictive boost before VSync)

**Code Location:** 
- Boost: `main.bpf.c:2464-2465`
- Physical cores: `main.bpf.c:660-676`

---

### **Stage 6: Page Flip (Frame Presentation)**
**Location:** DRM driver  
**Latency:** ~100-200µs (not yet optimized)

**What Happens:**
1. Compositor calls `drm_mode_page_flip`
2. DRM driver swaps frame buffers
3. New frame queued for display scanout
4. VSync event scheduled

**Current Optimizations:**
- ⚠️ **None** - Page flip not yet detected

**Scheduler Impact:** ⚠️ **Not optimized** (pending implementation)

**Optimization Opportunities:**
- 🔴 **HIGH PRIORITY:** Add fentry hook on `drm_mode_page_flip`
- 🔴 **HIGH PRIORITY:** Immediately boost compositor thread when page flip detected
- ⚠️ **Future:** Track page flip timing for VSync prediction

**Proposed Implementation:**
```c
SEC("fentry/drm_mode_page_flip")
int BPF_PROG(detect_compositor_page_flip, struct drm_crtc *crtc,
             struct drm_framebuffer *fb, u32 flags,
             struct drm_modeset_acquire_ctx *ctx)
{
    u32 tid = bpf_get_current_pid_tgid();
    register_compositor_thread(tid, COMPOSITOR_TYPE_UNKNOWN);
    
    /* OPTIMIZATION: Immediately boost compositor thread for next VSync */
    struct task_ctx *tctx = try_lookup_task_ctx(bpf_get_current_task_btf());
    if (tctx && tctx->is_compositor) {
        tctx->exec_runtime = 0;  /* Reset vruntime for immediate scheduling */
    }
    
    return 0;
}
```

**Code Location:** `include/compositor_detect.bpf.h` (to be added)

---

### **Stage 7: VSync & Display Pipeline**
**Location:** Display hardware  
**Latency:** ~1-2ms (VSync + scanout + pixel response)

**What Happens:**
1. VSync event fires (60Hz = 16.67ms, 240Hz = 4.17ms)
2. Display controller starts scanout
3. Pixels updated row by row
4. Pixel response time (1-4ms depending on monitor)

**Scheduler Impact:** ❌ **Not controllable** - Hardware timing

**Optimization Opportunities:**
- ⚠️ **None** - Hardware limits
- ⚠️ **Future:** VSync-aware scheduling could pre-boost compositor before VSync

**Hardware Limits:**
- **VSync Frequency:** Fixed by display refresh rate
- **Scanout Time:** ~1-2ms (depends on resolution)
- **Pixel Response:** Monitor-dependent (1-4ms typical)

---

## Target Movement Detection Scenario

### **Scenario:** Enemy changes direction in FPS game

**Timeline:**
```
T+0ms:    Enemy AI changes direction (game state)
T+1ms:    Render thread submits frame with new position
T+1.2ms:  GPU starts processing frame
T+4ms:    GPU completes rendering (hardware)
T+4.1ms:  GPU interrupt fires (boost 4 - fast wakeup)
T+4.3ms:  Compositor wakes (boost 5, physical core)
T+5ms:    Compositor finishes processing
T+5.1ms:  Page flip occurs (frame presented)
T+6ms:    VSync fires (next refresh cycle)
T+7ms:    Display scanout completes
T+8ms:    Pixel response completes (visible on monitor)
```

**Total Latency:** ~8ms from game state change to visible

**Critical Path:**
- GPU processing: ~3ms (hardware, fixed)
- Compositor: ~0.7ms (scheduler-controlled, optimized)
- Display pipeline: ~2ms (hardware, fixed)

---

## Optimization Strategies

### **🔴 Critical Path Optimizations**

#### 1. **Page Flip Detection** (HIGH PRIORITY)
**Impact:** ~100-200µs reduction in compositor wakeup latency  
**Implementation:** Add fentry hook on `drm_mode_page_flip`

**Benefits:**
- Detect actual frame presentation timing
- Enable VSync-aware scheduling
- Optimize compositor boost timing

**Code:** See Stage 6 above

#### 2. **VSync-Aware Compositor Scheduling** (MEDIUM PRIORITY)
**Impact:** ~0.5-1ms reduction through predictive boosting  
**Implementation:** Track page flip intervals, pre-boost compositor before VSync

**Benefits:**
- Pre-boost compositor 1-2ms before VSync
- Reduce frame presentation jitter
- Improve frame timing consistency

**Challenge:** Requires VSync event detection (userspace notification)

#### 3. **GPU Interrupt IRQ Affinity** (MEDIUM PRIORITY)
**Impact:** ~50-100µs reduction in interrupt handling latency  
**Implementation:** Pin GPU interrupts to fast CPU cores

**Benefits:**
- Faster interrupt processing
- Lower latency in waking compositor
- Better cache locality

**Code Location:** Userspace initialization (not BPF)

#### 4. **Frame Timing Tracking** (LOW PRIORITY)
**Impact:** Enables data-driven optimization  
**Implementation:** Track GPU submit → page flip latency

**Benefits:**
- Identify bottlenecks in frame pipeline
- Optimize based on actual timing data
- Detect frame drops/stalls

---

### **🟡 Secondary Optimizations**

#### 5. **Compositor Thread CPU Affinity**
**Impact:** ~100-200µs reduction through cache locality  
**Implementation:** Pin compositor threads to specific CPUs

**Benefits:**
- Better cache locality
- Reduced migration overhead
- More consistent frame timing

**Risks:** May conflict with physical core preference logic

#### 6. **Frame Buffer Ready Detection**
**Impact:** ~100-300µs reduction in compositor wait time  
**Implementation:** Detect when GPU frame buffer is ready

**Benefits:**
- Compositor can start processing immediately
- Don't wait for GPU interrupt
- Reduce pipeline latency

**Challenge:** Requires GPU driver cooperation

---

## Latency Reduction Roadmap

### **Phase 1: Quick Wins** ✅ **COMPLETED**
- ✅ Increase compositor boost: 3 → 5
- ✅ Increase GPU interrupt boost: 2 → 4
- ✅ Add physical core preference for compositor

**Expected Benefit:** ~1-2ms reduction

### **Phase 2: Detection Enhancements** (NEXT)
- ⚠️ Add page flip detection hook
- ⚠️ Add frame timing tracking

**Expected Benefit:** ~200-500µs reduction + enables Phase 3

### **Phase 3: Advanced Optimization** (FUTURE)
- ⚠️ VSync-aware compositor scheduling
- ⚠️ GPU interrupt IRQ affinity
- ⚠️ Frame buffer ready detection

**Expected Benefit:** Additional ~0.5-1ms reduction

---

## Testing & Validation

### **Metrics to Track:**
1. **Frame Presentation Latency:** GPU submit → Page flip time
2. **Compositor Wakeup Latency:** GPU interrupt → Compositor wake time
3. **Frame Presentation Jitter:** Variance in page flip timing
4. **VSync Miss Rate:** Percentage of frames missed

### **Test Scenarios:**
1. **High FPS (240+):** Verify compositor can keep up
2. **Variable FPS:** Test frame timing consistency
3. **CPU Saturation:** Verify compositor priority holds under load
4. **Multi-Monitor:** Test compositor behavior with multiple displays
5. **Target Movement:** Measure end-to-end latency for target direction changes

### **Validation Tools:**
- **PresentMon:** Track frame presentation timing
- **Custom BPF maps:** Track frame timing statistics
- **High-speed camera:** Verify visual latency (gold standard)

---

## Hardware Considerations

### **Monitor Selection:**
- **Refresh Rate:** Higher = lower latency (240Hz = 4.17ms vs 60Hz = 16.67ms)
- **Pixel Response:** Lower = faster visual updates (1ms vs 4ms)
- **G-Sync/FreeSync:** Reduces VSync latency by eliminating wait

### **GPU Selection:**
- **Driver Quality:** Better drivers = lower overhead
- **Memory Bandwidth:** Higher = faster frame buffer transfers
- **PCIe Bandwidth:** Higher = faster command submission

### **CPU Considerations:**
- **Physical Cores:** More = better for GPU/compositor threads
- **Clock Speed:** Higher = faster compositor processing
- **Cache Size:** Larger = better cache locality

---

## Conclusion

**Current State:** Frame presentation pipeline is well-optimized with recent improvements:
- Compositor boost increased to 5 (7x priority)
- GPU interrupt boost increased to 4 (6x priority)
- Physical core preference for compositor threads
- Fast path enabled for compositor (boost_shift >= 5)

**Remaining Opportunities:**
1. **Page flip detection** (high priority, ~200µs benefit)
2. **VSync-aware scheduling** (medium priority, ~0.5-1ms benefit)
3. **IRQ affinity** (medium priority, ~50-100µs benefit)

**Total Expected Improvement:** ~1.5-2.5ms reduction from current state

**Hardware Limits:**
- GPU processing: ~2-8ms (fixed)
- Display pipeline: ~1-2ms (fixed)
- **Software Limit:** ~1-2ms remaining (already optimized)

**Target Achievement:** ~6-10ms total latency from game state change to monitor display (competitive gaming standard)

