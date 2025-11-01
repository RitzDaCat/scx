# TUI Debugging & Monitoring Review

**Date:** 2025-01-28  
**Scope:** Review TUI for debugging capabilities, game swapping, and ring buffer overflow handling

---

## Executive Summary

**Overall Assessment:** ⚠️ **Functional but Missing Critical Debug Info**

**Key Findings:**
- ✅ Basic metrics displayed (CPU, migrations, latency)
- 🔴 **CRITICAL:** Ring buffer overflow count not displayed (hardcoded to 0)
- 🔴 **CRITICAL:** No game swap detection/logging
- ⚠️ **Missing:** Ring buffer latency metrics (p50, p95, p99)
- ⚠️ **Missing:** Fentry event breakdown (gaming vs filtered)
- ⚠️ **Missing:** Queue dropped metrics display

**Total Issues:** 6 (2 Critical, 4 Important)

---

## 1. Critical Issues

### 🔴 **ISSUE #1: Ring Buffer Overflow Not Displayed**

**Severity:** Critical  
**Impact:** Cannot detect/debug ring buffer overflow during gaming  
**Location:** `tui.rs:907-919`

**Problem:**
```rust
fn render_queue_status(f: &mut Frame, area: Rect, _metrics: &Metrics, _state: &TuiState) {
    let lines = vec![
        Line::from(vec![
            Span::styled("RB dropped", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(format!("{}", 0), Style::default().fg(Color::Yellow)),  // HARDCODED!
        ]),
    ];
```

**Analysis:**
- Health check shows overflow status (OK/Not OK) but not count
- Queue status widget shows hardcoded `0` instead of `metrics.ringbuf_overflow_events`
- No alert/logging when overflow occurs
- Userspace dropped count (`rb_queue_dropped_total`) not displayed

**Available Metrics:**
- `metrics.ringbuf_overflow_events` - BPF overflow count
- `metrics.rb_queue_dropped_total` - Userspace queue drops
- `metrics.rb_queue_high_watermark` - Queue depth warning

**Recommendation:**
1. Display actual overflow count in queue status widget
2. Add overflow alert in `evaluate_alerts()` 
3. Show cumulative overflow count (not just per-interval)
4. Display userspace queue drops separately

**Fix Priority:** **CRITICAL** - Blocks overflow debugging

---

### 🔴 **ISSUE #2: Game Swap Not Detected/Logged**

**Severity:** Critical  
**Impact:** Cannot track game changes during session  
**Location:** `tui.rs:1637-1639`

**Problem:**
```rust
if metrics.fg_pid > 0 && st.game_pid != metrics.fg_pid as u32 {
    st.game_pid = metrics.fg_pid as u32;  // Only updates, no logging
}
```

**Analysis:**
- Game PID updated silently when it changes
- No event log entry for game swap
- No statistics reset when game changes (statistics carry over)
- Game name change not detected (only PID checked)

**Impact:**
- Cannot track game changes during session
- Statistics from previous game pollute new game metrics
- Cannot correlate performance issues with specific games

**Recommendation:**
1. Detect game swap (PID or app name change)
2. Log game swap to event log with game name
3. Optionally reset statistics on game swap (configurable)
4. Track game session start time

**Fix Priority:** **CRITICAL** - Essential for multi-game sessions

---

## 2. Important Missing Metrics

### ⚠️ **ISSUE #3: Ring Buffer Latency Metrics Not Displayed**

**Severity:** Important  
**Impact:** Cannot debug input latency spikes  
**Location:** Metrics available but not displayed

**Available Metrics (Not Displayed):**
- `ringbuf_latency_avg_ns` - Average latency
- `ringbuf_latency_p50_ns` - Median latency
- `ringbuf_latency_p95_ns` - 95th percentile
- `ringbuf_latency_p99_ns` - 99th percentile
- `ringbuf_latency_min_ns` - Minimum latency
- `ringbuf_latency_max_ns` - Maximum latency

**Current Display:**
- Only shows BPF latency (select_cpu, enqueue, dispatch)
- Missing ring buffer latency (kernel→userspace→processing)

**Recommendation:**
- Add ring buffer latency metrics to Performance tab
- Show p50/p95/p99 as latency percentiles
- Alert on high p95/p99 latency (>1ms)

**Fix Priority:** **HIGH** - Critical for latency debugging

---

### ⚠️ **ISSUE #4: Fentry Event Breakdown Not Shown**

**Severity:** Important  
**Impact:** Cannot verify input detection is working correctly  
**Location:** Metrics available but not displayed

**Available Metrics (Not Displayed):**
- `fentry_total_events` - Total events seen
- `fentry_gaming_events` - Events from gaming devices
- `fentry_filtered_events` - Events filtered (non-gaming)
- `fentry_boost_triggers` - Boost activations

**Current Display:**
- Only shows "Fentry Hook: Enabled/Disabled" (boolean)
- No breakdown of event types
- No verification that filtering is working

**Recommendation:**
- Add fentry event breakdown to Threads tab or new "Input" tab
- Show gaming vs filtered event ratio
- Alert if filtered events > 50% (might indicate misconfiguration)

**Fix Priority:** **MEDIUM** - Useful for input detection debugging

---

### ⚠️ **ISSUE #5: Continuous Input Mode Status Not Displayed**

**Severity:** Important  
**Impact:** Cannot verify continuous input mode is active  
**Location:** Metrics available but not displayed

**Available Metrics (Not Displayed):**
- `continuous_input_mode` - Continuous mode active (1/0)
- `continuous_input_lane_keyboard` - Keyboard lane active
- `continuous_input_lane_mouse` - Mouse lane active
- `continuous_input_lane_other` - Other lane active

**Current Display:**
- No indication of continuous input mode status
- Cannot verify if continuous mode is working

**Recommendation:**
- Add continuous input mode status to Input Status widget
- Show active lanes (keyboard/mouse/other)
- Alert if continuous mode should be active but isn't

**Fix Priority:** **MEDIUM** - Useful for verifying input mode

---

### ⚠️ **ISSUE #6: Queue Metrics Not Fully Displayed**

**Severity:** Important  
**Impact:** Cannot debug userspace queue issues  
**Location:** `tui.rs:907-919`

**Problem:**
- `rb_queue_dropped_total` - Not displayed
- `rb_queue_high_watermark` - Not displayed
- Only shows hardcoded `0` for dropped

**Recommendation:**
- Display userspace queue drops separately from BPF overflow
- Show queue high watermark (warns if queue backing up)
- Alert if queue drops > 0

**Fix Priority:** **MEDIUM** - Completes queue monitoring

---

## 3. What's Already Good ✅

### ✅ **Core Metrics Display**
- CPU utilization (current, average, foreground %)
- Migration statistics (total, blocked, rate)
- BPF latency (select_cpu, enqueue, dispatch)
- Thread classification counts
- Input/frame window percentages

### ✅ **Health Check Widget**
- Shows scheduler status
- Shows game detection status
- Shows fentry hook status
- Shows ring buffer overflow status (but not count)

### ✅ **Event Log**
- Logs warnings and errors
- Tracks alerts (input idle, migration blocking, latency)
- Shows timestamps

### ✅ **Historical Data**
- Tracks CPU trends
- Tracks latency trends
- Tracks queue mix trends

---

## 4. Recommendations Summary

### Immediate Fixes (Critical):

1. **Fix Ring Buffer Overflow Display** (Issue #1)
   - Replace hardcoded `0` with `metrics.ringbuf_overflow_events`
   - Add overflow alert in `evaluate_alerts()`
   - Display cumulative overflow count

2. **Add Game Swap Detection** (Issue #2)
   - Detect game PID/app name changes
   - Log game swap to event log
   - Optionally reset statistics on game swap

### Important Additions:

3. **Add Ring Buffer Latency Metrics** (Issue #3)
   - Display p50/p95/p99 latency in Performance tab
   - Alert on high latency spikes

4. **Add Fentry Event Breakdown** (Issue #4)
   - Show gaming vs filtered event counts
   - Display in Threads or new Input tab

5. **Add Continuous Input Mode Status** (Issue #5)
   - Show continuous mode status
   - Display active lanes

6. **Complete Queue Metrics** (Issue #6)
   - Display userspace queue drops
   - Show queue high watermark

---

## 5. Testing Scenarios

### Scenario 1: Ring Buffer Overflow
**Current:** ❌ Cannot detect overflow (hardcoded 0)  
**After Fix:** ✅ Shows overflow count, alerts on overflow

### Scenario 2: Game Swap
**Current:** ❌ Silent game change, stats carry over  
**After Fix:** ✅ Logs game swap, optionally resets stats

### Scenario 3: Input Latency Spike
**Current:** ⚠️ Shows BPF latency but not ring buffer latency  
**After Fix:** ✅ Shows full latency chain (BPF + ring buffer)

### Scenario 4: Fentry Not Working
**Current:** ⚠️ Shows enabled/disabled but not event breakdown  
**After Fix:** ✅ Shows event counts, detects if filtering broken

---

## 6. Proposed UI Changes

### Queue Status Widget (Fix Issue #1):
```
INPUT QUEUE
─────────────────────────
BPF Overflow:  0 events
Userspace Drop: 0 events
Queue Depth:    42 / 128
RB Latency:     p50: 2.1µs
                p95: 8.3µs
                p99: 15.2µs
```

### Game Swap Detection (Fix Issue #2):
```
Event Log:
[12:34:56] INFO  Game detected: Counter-Strike 2 (PID: 12345)
[12:45:12] INFO  Game swapped: Counter-Strike 2 → Apex Legends (PID: 67890)
[12:45:12] INFO  Statistics reset for new game session
```

### Input Status Widget (Fix Issue #5):
```
INPUT STATUS
─────────────────────────
Input Window:  ACTIVE (45%)
Fentry Hook:   ENABLED
Continuous:    ACTIVE (Keyboard, Mouse)
Gaming Events: 12,345 / 12,450 (99.2%)
```

---

## 7. Priority Ranking

**P0 (Critical - Fix Immediately):**
1. Ring buffer overflow display (Issue #1)
2. Game swap detection/logging (Issue #2)

**P1 (High - Add Soon):**
3. Ring buffer latency metrics (Issue #3)

**P2 (Medium - Nice to Have):**
4. Fentry event breakdown (Issue #4)
5. Continuous input mode status (Issue #5)
6. Complete queue metrics (Issue #6)

---

## 8. Implementation Notes

### For Ring Buffer Overflow Fix:
- Update `render_queue_status()` to use actual metrics
- Add overflow alert: `if metrics.ringbuf_overflow_events > 0`
- Track cumulative overflow (add to `HistoricalData`)

### For Game Swap Detection:
- Compare `metrics.fg_pid` and `metrics.fg_app` with previous
- Log to event log: `"Game swapped: {old} → {new} (PID: {pid})"`
- Add configurable auto-reset: `--tui-reset-on-game-swap`

### For Latency Metrics:
- Add ring buffer latency chart to Performance tab
- Use percentile display: `p50: {p50}ns, p95: {p95}ns, p99: {p99}ns`
- Alert threshold: p95 > 1000ns (1µs) or p99 > 5000ns (5µs)

