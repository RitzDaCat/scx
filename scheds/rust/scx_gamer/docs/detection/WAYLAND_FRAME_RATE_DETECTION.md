# Wayland Frame Rate Detection Analysis

**Date:** 2025-01-28  
**Question:** Can we use Wayland protocols to detect frame rate accurately?

---

## Wayland Frame Timing Protocols

### [STATUS: IMPLEMENTED] **1. `wp_presentation` Protocol** (Most Accurate)

**What It Provides:**
- **Frame presentation timestamps** - Exact time when frame was displayed
- **Refresh rate information** - Display refresh rate (60/120/240Hz)
- **VSync timing** - When VSync occurred
- **Frame intervals** - Time between displayed frames

**Why It's Better:**
- [IMPLEMENTED] Reflects **actual displayed frames** (not submissions)
- [IMPLEMENTED] Works regardless of triple buffering (tracks display, not submission)
- [IMPLEMENTED] Provides accurate refresh rate
- [IMPLEMENTED] No ambiguity about what constitutes a "frame"

**How It Works:**
```
Game renders frame → Submits to compositor → Compositor presents → wp_presentation event
                                                                    ↑
                                                         Timestamp of actual display
```

---

### [STATUS: IMPLEMENTED] **2. `wl_surface.frame` Callbacks**

**What It Provides:**
- **Frame callback timing** - When compositor wants next frame
- **Rendering synchronization** - Aligns game rendering with display

**Limitations:**
- [NOTE] Reflects compositor's **request** for frames, not actual display
- [NOTE] May fire faster than display refresh (VSync OFF scenarios)
- [NOTE] Less accurate than `wp_presentation`

---

## Can We Access This from BPF?

### ❌ **Challenge: Wayland is Userspace Protocol**

**Problem:**
- Wayland protocols run **entirely in userspace**
- Communication via Unix sockets (`/run/user/1000/wayland-0`)
- No direct kernel API for Wayland protocol events
- BPF can't hook userspace library calls easily

**Current State:**
- [IMPLEMENTED] We detect compositor threads (KWin, Mutter, etc.)
- [IMPLEMENTED] We detect DRM operations (kernel-level, compositor → kernel)
- ❌ We **cannot** directly hook Wayland protocol events from BPF

---

## Potential Solutions

### **Option 1: Hook Userspace Wayland Library Calls** (Complex)

**Method:**
```c
// Hook libwayland-client functions
SEC("uprobe/libwayland-client")
int BPF_PROG(track_wp_presentation, struct wl_presentation *presentation, ...)
{
    // Extract presentation timestamp
    // Update frame_interval_ns
}
```

**Pros:**
- [IMPLEMENTED] Direct access to Wayland presentation times
- [IMPLEMENTED] Most accurate frame timing

**Cons:**
- ❌ Requires `uprobe` (userspace probes) - not always available
- ❌ Complex - need to understand Wayland library internals
- ❌ May not work with static linking
- ❌ Less reliable than kernel hooks

**Status:** [NOTE] **Possible but complex**

---

### **Option 2: Userspace Integration** (Recommended)

**Method:**
- **Userspace helper** reads Wayland events (via `libwayland-client`)
- Updates BPF map with frame timing data
- BPF reads from map during scheduling

**Implementation:**
```rust
// In userspace (Rust)
use wayland_client::protocol::wp_presentation_time::WpPresentation;

fn track_presentation_times(presentation: &WpPresentation) {
    // Receive presentation events
    // Calculate frame intervals
    // Update BPF map: frame_interval_ns
}
```

**Pros:**
- [IMPLEMENTED] Most reliable approach
- [IMPLEMENTED] Full access to Wayland protocols
- [IMPLEMENTED] Can use existing Wayland libraries
- [IMPLEMENTED] Accurate frame timing

**Cons:**
- [NOTE] Requires userspace component
- [NOTE] Adds complexity to scheduler
- [NOTE] Latency: Map update → BPF read (still very fast ~100ns)

**Status:** [STATUS: IMPLEMENTED] **Recommended - Most Practical**

---

### **Option 3: Monitor Wayland Socket** (Alternative)

**Method:**
- Monitor `/run/user/1000/wayland-0` socket
- Parse Wayland protocol messages
- Extract `wp_presentation` events

**Pros:**
- [IMPLEMENTED] No library dependencies
- [IMPLEMENTED] Works with any Wayland compositor

**Cons:**
- ❌ Very complex (binary protocol parsing)
- ❌ Fragile (protocol changes break it)
- ❌ Higher latency

**Status:** ❌ **Not Recommended**

---

## Recommended Approach: Userspace Wayland Integration

### **Implementation Plan**

#### **Step 1: Add Wayland Presentation Tracking**

```rust
// New module: wayland_frame_tracker.rs
use wayland_client::protocol::wp_presentation_time::WpPresentation;
use wayland_client::protocol::wl_surface::WlSurface;

pub struct WaylandFrameTracker {
    presentation: WpPresentation,
    last_presentation_time: Option<Instant>,
    frame_intervals: Vec<Duration>,
}

impl WaylandFrameTracker {
    pub fn new() -> Result<Self> {
        // Connect to Wayland display
        // Bind wp_presentation protocol
        // Register for presentation events
    }
    
    pub fn on_presentation(&mut self, timestamp: u64) {
        // Calculate frame interval
        // Update BPF map: frame_interval_ns
        // Store in EMA for RMS priority calculation
    }
}
```

#### **Step 2: Update BPF Map**

```rust
// Update BPF volatile variables
if let Some(ref mut bss) = skel.maps.bss_data.as_mut() {
    bss.frame_interval_ns = calculated_interval_ns;
    bss.last_page_flip_ns = presentation_timestamp_ns;
    bss.frame_count += 1;
}
```

#### **Step 3: Use in BPF Scheduling**

```c
// BPF already reads frame_interval_ns (from userspace updates)
// Use for RMS priority calculation
if (frame_interval_ns > 0) {
    u64 frame_period_ns = frame_interval_ns;
    u8 rms_priority = calculate_rms_priority_from_period(frame_period_ns);
}
```

---

## Comparison: Wayland vs GPU Wakeup

| Aspect | GPU Wakeup (`wakeup_freq`) | Wayland `wp_presentation` |
|--------|---------------------------|--------------------------|
| **Accuracy** | [NOTE] May over-count (triple buffering) | [IMPLEMENTED] Actual displayed frames |
| **Latency** | [IMPLEMENTED] Real-time (BPF hook) | [NOTE] ~100ns (map read) |
| **Complexity** | [IMPLEMENTED] Already implemented | [NOTE] Requires userspace component |
| **Reliability** | [NOTE] Depends on game engine | [IMPLEMENTED] Protocol-standardized |
| **Coverage** | [IMPLEMENTED] All GPU games | [NOTE] Wayland only (not X11) |

---

## Frame Rate Detection Priority

### **Priority 1: Wayland `wp_presentation`** (Best for Wayland)

**Use Case:** Wayland sessions (KWin, Mutter, etc.)

**Implementation:**
- Userspace Wayland client reads `wp_presentation` events
- Updates BPF map with frame intervals
- BPF uses for RMS priority calculation

**Expected Accuracy:** [STATUS: IMPLEMENTED] **Very High** - Reflects actual displayed frames

---

### **Priority 2: Compositor DRM Operations** (Fallback)

**Use Case:** All compositors (Wayland + X11)

**Implementation:**
- Already tracking `compositor_thread_info.operation_freq_hz`
- Reflects display refresh rate (not game render rate)

**Expected Accuracy:** [NOTE] **Medium** - Display refresh rate, not game FPS

---

### **Priority 3: GPU Wakeup Frequency** (Last Resort)

**Use Case:** Games without Wayland integration

**Implementation:**
- Use `wakeup_freq` capped to display refresh rate
- Conservative approach to avoid over-prioritization

**Expected Accuracy:** [NOTE] **Low-Medium** - Depends on buffering strategy

---

## Wayland Protocols Available

### **1. `wp_presentation` Extension**

**Provides:**
- `presented` event with timestamp
- `refresh` event with refresh rate
- `clock_id` for consistent timing

**Example:**
```rust
presentation.on_presented(|event| {
    let timestamp = event.tv_sec * 1_000_000_000 + event.tv_nsec;
    // Use timestamp for frame interval calculation
});
```

---

### **2. `wl_surface.frame` Callbacks**

**Provides:**
- Frame callback timing
- Rendering synchronization hints

**Example:**
```rust
surface.on_frame(move |callback, _| {
    // Called when compositor wants next frame
    // Can measure intervals for frame rate estimation
});
```

---

## Implementation Considerations

### **Dependencies**

**Required:**
- `wayland-client` crate (Rust)
- `wayland-protocols` (for `wp_presentation`)

**Optional:**
- `wayland-backend` (for socket monitoring)

---

### **Integration Points**

**1. Scheduler Initialization:**
```rust
// In main.rs scheduler init
let wayland_tracker = if is_wayland_session() {
    Some(WaylandFrameTracker::new()?)
} else {
    None
};
```

**2. Event Loop:**
```rust
// In main event loop
if let Some(ref mut tracker) = wayland_tracker {
    tracker.poll_events();
    tracker.update_bpf_maps(&mut skel);
}
```

**3. BPF Usage:**
```c
// Already implemented - just needs frame_interval_ns populated
if (frame_interval_ns > 0) {
    // Use for RMS priority
}
```

---

## Conclusion

### [STATUS: IMPLEMENTED] **YES - Wayland Provides Better Frame Rate Detection**

**Key Advantages:**
1. **`wp_presentation` protocol** - Accurate frame presentation timestamps
2. **Reflects actual displayed frames** - Not submissions (handles triple buffering)
3. **Protocol-standardized** - Works across all Wayland compositors

**Implementation:**
- [STATUS: IMPLEMENTED] **Recommended:** Userspace Wayland client + BPF map updates
- [STATUS: IMPLEMENTED] **Already have:** BPF code to read `frame_interval_ns`
- [NOTE] **Need:** Userspace Wayland integration component

**Expected Result:**
- **More accurate** frame rate detection than GPU wakeup
- **Better RMS priority** assignment (based on actual displayed frames)
- **Works for triple-buffered games** (tracks display, not submission)

**Next Steps:**
1. Add `wayland-client` dependency
2. Implement `WaylandFrameTracker` module
3. Integrate with scheduler event loop
4. Update BPF map from Wayland events
5. Use `frame_interval_ns` for RMS priority calculation

---

## References

- **Wayland Book:** [Frame Callbacks](https://wayland-book.com/surfaces-in-depth/frame-callbacks.html)
- **Wayland Protocols:** `wp_presentation_time` extension
- **Emersion Blog:** [Wayland Rendering Loop](https://emersion.fr/blog/2018/wayland-rendering-loop/)

