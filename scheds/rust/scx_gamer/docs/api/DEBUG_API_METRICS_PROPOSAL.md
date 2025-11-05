# Debug API Metrics Enhancement Proposal

**Purpose:** Add missing metrics to verify all scheduler code paths are working 100%

**Status:** Proposal - Ready for Implementation

---

## Currently Tracked But NOT Exposed

### **1. CPU Placement Verification** (Critical for Cache Affinity)

**Missing Metrics:**
- `nr_gpu_phys_kept` - GPU threads kept on physical cores (cache affinity)
- `nr_compositor_phys_kept` - Compositor threads kept on physical cores
- `nr_gpu_pref_fallback` - GPU preferred core fallback (when preferred core unavailable)

**Why Needed:**
- Verifies GPU/compositor threads are staying on physical cores (cache affinity working)
- `nr_gpu_pref_fallback` shows when preferred core caching fails (may indicate CPU contention)

**BPF Location:** `main.bpf.c` lines 230-232 (already tracked)

---

### **2. Deadline Miss Detection** (Critical for Auto-Recovery)

**Missing Metrics:**
- `nr_deadline_misses` - Total deadline misses detected
- `nr_auto_boosts` - Auto-boost actions taken (self-healing)

**Why Needed:**
- Verifies deadline tracking is working (`expected_deadline` being set)
- Verifies auto-recovery is triggering when threads miss deadlines
- Helps identify threads that need higher boost levels

**BPF Location:** `main.bpf.c` lines 247-248 (already tracked)

---

### **3. Scheduler State Tracking** (Critical for Thread Re-Classification)

**Missing Metrics:**
- `scheduler_generation` - Generation counter (incremented on scheduler restart/game change)
- `detected_fg_tgid` - Runtime-detected foreground TGID (vs `foreground_tgid` config)

**Why Needed:**
- `scheduler_generation` verifies thread re-classification is working after game changes
- `detected_fg_tgid` shows if game detection is working (vs manual `--foreground-pid`)

**BPF Location:** `main.bpf.c` line 283 (`scheduler_generation`), line 161 (`detected_fg_tgid`)

---

### **4. Window Status** (Critical for Input/Frame Window Verification)

**Missing Metrics:**
- `input_window_active` - Is input window currently active? (1=yes, 0=no)
- `frame_window_active` - Is frame window currently active? (1=yes, 0=no)
- `input_window_until_ns` - Timestamp when input window expires
- `frame_window_until_ns` - Timestamp when frame window expires

**Why Needed:**
- Verifies input/frame windows are being triggered correctly
- Shows timing of window activation (useful for debugging window duration issues)
- Currently only have cumulative `win_input_ns` / `win_frame_ns`, not current state

**BPF Location:** `boost.bpf.h` - `input_until_global`, `frame_until_global` (or per-CPU versions)

---

### **5. Boost Shift Distribution** (Critical for Priority Verification)

**Missing Metrics:**
- `boost_distribution[0-7]` - Count of threads at each boost level (0=no boost, 7=max)

**Why Needed:**
- Verifies boost values are being applied correctly
- Shows distribution of thread priorities (e.g., "10 threads at boost 7, 3 at boost 6")
- Helps identify if boost computation is working (`recompute_boost_shift()`)

**BPF Location:** Would need to add counters (`nr_boost_shift_0` through `nr_boost_shift_7`)

**Implementation:** Increment counters in `recompute_boost_shift()` when boost changes

---

### **6. Migration Cooldown Tracking** (Critical for Cache Affinity)

**Missing Metrics:**
- `nr_mig_blocked_cooldown` - Migrations blocked by cooldown (32ms post-migration)
- `nr_mig_blocked_frame_window` - Migrations blocked during frame window (already have `nr_frame_mig_block`)

**Why Needed:**
- Verifies migration cooldown is working (prevents cache thrashing)
- Separates cooldown blocks from frame window blocks (different reasons)

**BPF Location:** `main.bpf.c` - `need_migrate()` function (line ~2100)

---

### **7. Game Detection Details** (Critical for Robustness)

**Missing Metrics:**
- `game_detection_method` - Detection method used ("bpf_lsm", "inotify", "manual", "none")
- `game_detection_score` - Detection confidence score (0-100)
- `game_detection_timestamp` - When game was detected (unix timestamp)

**Why Needed:**
- Verifies game detection is working (not relying on manual `--foreground-pid`)
- Shows detection confidence (low scores may indicate false positives)
- Helps debug detection failures

**Rust Location:** `game_detect.rs` - needs to expose detection metadata

---

### **8. Input Lane Status** (Critical for Continuous Input Mode)

**Missing Metrics:**
- `input_lane_keyboard_active` - Is keyboard lane in continuous input mode? (1=yes, 0=no)
- `input_lane_mouse_active` - Is mouse lane in continuous input mode? (1=yes, 0=no)
- `input_lane_other_active` - Is other lane in continuous input mode? (1=yes, 0=no)
- `input_lane_keyboard_rate` - Keyboard trigger rate (events/sec)
- `input_lane_mouse_rate` - Mouse trigger rate (events/sec)

**Why Needed:**
- Verifies continuous input mode is working per-lane
- Shows which input devices are active (useful for debugging multi-device setups)
- Currently only have global `continuous_input_mode` and `input_trigger_rate`

**BPF Location:** `main.bpf.c` lines 314-316 (`continuous_input_lane_mode[]`, `input_lane_trigger_rate[]`)

---

### **9. Preferred Core Tracking** (Critical for GPU Thread Optimization)

**Missing Metrics:**
- `gpu_preferred_core_hits` - Total preferred core hits (aggregated from `task_ctx->preferred_core_hits`)
- `gpu_preferred_core_misses` - Total preferred core misses (when fallback used)

**Why Needed:**
- Verifies preferred core caching is working (GPU threads hitting cached cores)
- Shows effectiveness of GPU thread CPU placement optimization

**BPF Location:** Would need to aggregate from `task_ctx->preferred_core_hits` (per-thread tracking exists)

---

### **10. Frame Timing Details** (Critical for Frame Window Accuracy)

**Missing Metrics:**
- `frame_interval_ns` - Estimated frame interval (EMA of inter-frame time)
- `frame_count` - Total frames presented
- `last_page_flip_ns` - Timestamp of last page flip (frame presentation)

**Why Needed:**
- Verifies frame timing detection is working
- Shows frame rate estimation accuracy
- Helps debug frame window timing issues

**BPF Location:** `main.bpf.c` lines 226-228 (already tracked)

---

## Implementation Priority

### **P0 - Critical for Verification** (Implement First)
1. ✅ CPU Placement Verification (`nr_gpu_phys_kept`, `nr_compositor_phys_kept`, `nr_gpu_pref_fallback`)
2. ✅ Deadline Miss Detection (`nr_deadline_misses`, `nr_auto_boosts`)
3. ✅ Scheduler State (`scheduler_generation`, `detected_fg_tgid`)
4. ✅ Window Status (`input_window_active`, `frame_window_active`)

### **P1 - Important for Debugging** (Implement Second)
5. ✅ Boost Distribution (`boost_distribution[0-7]`)
6. ✅ Migration Cooldown (`nr_mig_blocked_cooldown`)
7. ✅ Input Lane Status (`input_lane_*_active`, `input_lane_*_rate`)

### **P2 - Nice to Have** (Implement Third)
8. ✅ Game Detection Details (`game_detection_method`, `game_detection_score`)
9. ✅ Preferred Core Tracking (`gpu_preferred_core_hits`)
10. ✅ Frame Timing Details (`frame_interval_ns`, `frame_count`, `last_page_flip_ns`)

---

## API Response Example

```json
{
  "cpu_placement": {
    "gpu_phys_kept": 1234,
    "compositor_phys_kept": 567,
    "gpu_pref_fallback": 89
  },
  "deadline_tracking": {
    "deadline_misses": 12,
    "auto_boosts": 3
  },
  "scheduler_state": {
    "scheduler_generation": 5,
    "detected_fg_tgid": 253504,
    "fg_pid": 253504
  },
  "window_status": {
    "input_window_active": 1,
    "frame_window_active": 0,
    "input_window_until_ns": 1738271234567890,
    "frame_window_until_ns": 0
  },
  "boost_distribution": {
    "boost_0": 1245,
    "boost_1": 23,
    "boost_2": 45,
    "boost_3": 12,
    "boost_4": 8,
    "boost_5": 15,
    "boost_6": 3,
    "boost_7": 53
  },
  "migration_cooldown": {
    "mig_blocked_cooldown": 234,
    "mig_blocked_frame_window": 56
  },
  "input_lanes": {
    "keyboard_active": 1,
    "mouse_active": 1,
    "other_active": 0,
    "keyboard_rate": 125,
    "mouse_rate": 500
  },
  "game_detection": {
    "method": "bpf_lsm",
    "score": 95,
    "timestamp": 1738271234
  },
  "gpu_preferred_cores": {
    "hits": 5678,
    "misses": 234
  },
  "frame_timing": {
    "frame_interval_ns": 16666666,
    "frame_count": 12345,
    "last_page_flip_ns": 1738271234567890
  }
}
```

---

## Verification Checklist

Once implemented, use these checks to verify scheduler functionality:

- [ ] **CPU Placement:** `gpu_phys_kept > 0` (GPU threads staying on physical cores)
- [ ] **Deadline Tracking:** `deadline_misses` increasing (deadline detection working)
- [ ] **Auto-Recovery:** `auto_boosts > 0` when `deadline_misses` high (self-healing working)
- [ ] **Game Detection:** `detected_fg_tgid > 0` (game detection working)
- [ ] **Window Status:** `input_window_active` toggles on input (resets after 2ms)
- [ ] **Boost Distribution:** `boost_7 > 0` (input handlers getting max boost)
- [ ] **Migration Cooldown:** `mig_blocked_cooldown > 0` (cooldown preventing migrations)
- [ ] **Input Lanes:** `keyboard_active` or `mouse_active` = 1 during gaming
- [ ] **Frame Timing:** `frame_interval_ns` ≈ 16.67ms for 60fps (frame detection working)

---

## Notes

- All P0 metrics are already tracked in BPF, just need to expose in API
- P1 metrics require minor BPF additions (boost distribution counters)
- P2 metrics require more complex aggregation (preferred core hits from per-thread data)

