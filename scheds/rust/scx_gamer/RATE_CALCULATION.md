# Keyboard Rate Calculation Optimization

## Problem: Slow Rate Convergence

### Before Fix (Old EMA: 87.5% old + 12.5% new)
```
Fast typing at 10 Hz instantaneous (100ms between keys):

Event 1:  60 Hz (initial)
Event 2:  (60*7 + 10)/8 = 58.75 Hz  ❌ Still way too high!
Event 3:  (58.75*7 + 10)/8 = 52.6 Hz
Event 4:  (52.6*7 + 10)/8 = 47.3 Hz
Event 5:  (47.3*7 + 10)/8 = 42.6 Hz
Event 10: ~28 Hz
Event 20: ~15 Hz                     ❌ Takes 20 events to show real rate!
```

### After Fix (New EMA: 50% old + 50% new for keyboard)
```
Fast typing at 10 Hz instantaneous (100ms between keys):

Event 1:  5 Hz (low initial)
Event 2:  (5 + 10)/2 = 7.5 Hz       ✅ Already approaching real rate!
Event 3:  (7.5 + 10)/2 = 8.75 Hz
Event 4:  (8.75 + 10)/2 = 9.4 Hz    ✅ Within 10% after 4 events!
Event 5:  (9.4 + 10)/2 = 9.7 Hz
Event 10: ~9.97 Hz                   ✅ Accurate!
```

## Changes Made

### 1. Faster EMA for Keyboard (Line 91-93)
**Before:** `rate_new = (rate_prev * 7 + instant) >> 3;` (all devices)  
**After:** `rate_new = (rate_prev + instant) >> 1;` (keyboard only)

- **Keyboard:** 50% old + 50% new (fast response)
- **Mouse:** 87.5% old + 12.5% new (smooth for high-rate jitter)

### 2. Lower Initial Rate for Keyboard (Line 106)
**Before:** `rate_new = 60;` (all devices)  
**After:** `rate_new = 5;` (keyboard) / `60` (mouse/other)

Starting low lets the fast EMA ramp up quickly to the real rate.

### 3. Kept 2s Timeout Window (Line 83)
```c
rate_window_ns = 2000000000ULL; /* 2s - allows slow typing/gaming */
```

This prevents rate reset during WASD movement (1-2s gaps between keys).

## Rate Convergence Comparison

### Typing at 10 Hz (100ms gaps)

| Event # | Old (87.5/12.5) | New (50/50) | Real | Error (Old) | Error (New) |
|---------|-----------------|-------------|------|-------------|-------------|
| 1       | 60.0 Hz        | 5.0 Hz      | 10   | +500%       | -50%        |
| 2       | 58.8 Hz        | 7.5 Hz      | 10   | +488%       | -25%        |
| 3       | 52.6 Hz        | 8.8 Hz      | 10   | +426%       | -12%        |
| 4       | 47.3 Hz        | 9.4 Hz      | 10   | +373%       | -6%         |
| 5       | 42.6 Hz        | 9.7 Hz      | 10   | +326%       | -3%         |
| 10      | 27.7 Hz        | 9.97 Hz     | 10   | +177%       | -0.3%       |

**Result:** New EMA reaches 90% accuracy in 4 events vs 20+ events!

### Rapid Typing at 20 Hz (50ms gaps)

| Event # | Old (87.5/12.5) | New (50/50) | Real | Error (Old) | Error (New) |
|---------|-----------------|-------------|------|-------------|-------------|
| 1       | 60.0 Hz        | 5.0 Hz      | 20   | +200%       | -75%        |
| 2       | 55.0 Hz        | 12.5 Hz     | 20   | +175%       | -37%        |
| 3       | 50.6 Hz        | 16.3 Hz     | 20   | +153%       | -19%        |
| 4       | 46.8 Hz        | 18.1 Hz     | 20   | +134%       | -9%         |
| 5       | 43.4 Hz        | 19.1 Hz     | 20   | +117%       | -5%         |

**Result:** Reaches 90% accuracy in 5 events instead of 25+ events!

### Gaming WASD (5 Hz - 200ms gaps)

| Event # | Old (87.5/12.5) | New (50/50) | Real | Error (Old) | Error (New) |
|---------|-----------------|-------------|------|-------------|-------------|
| 1       | 60.0 Hz        | 5.0 Hz      | 5    | +1100%      | 0%          |
| 2       | 53.1 Hz        | 5.0 Hz      | 5    | +962%       | 0%          |
| 3       | 47.0 Hz        | 5.0 Hz      | 5    | +840%       | 0%          |
| 10      | 14.6 Hz        | 5.0 Hz      | 5    | +192%       | 0%          |

**Result:** Perfect accuracy from event 1!

## Boost Window Scaling

The boost window multiplier (line 114) is still optimal:
```c
int mult = rate_new / 5;  // Range: 1x to 20x
```

**Examples:**
- **5 Hz** (WASD): mult = 1 → 1x boost window (5ms)
- **10 Hz** (typing): mult = 2 → 2x boost window (10ms)
- **20 Hz** (fast): mult = 4 → 4x boost window (20ms)
- **60 Hz** (repeat): mult = 12 → 12x boost window (60ms)

This scaling ensures continuous boost during sustained activity.

## Expected TUI Display

### Before Fix
```
Input Lanes:  Keyboard: 0 Hz    Mouse: 1000 Hz  Other: 0 Hz
(After 30 keypresses: Keyboard: 13 Hz)  ❌ Too slow to converge!
```

### After Fix
```
Input Lanes:  Keyboard: 5 Hz    Mouse: 1000 Hz  Other: 0 Hz
(After 5 keypresses:  Keyboard: 10 Hz)  ✅ Quick convergence!
(While holding key:   Keyboard: 35 Hz)  ✅ Tracks repeat rate!
(WASD movement:       Keyboard: 5 Hz)   ✅ Stable low rate!
```

## Why Keep Slow EMA for Mouse?

Mouse polling is **1000-8000 Hz** with micro-jitter, so we want heavy smoothing:
- Raw samples: 995 Hz, 1005 Hz, 998 Hz, 1002 Hz (±0.5% jitter)
- Old EMA: 1000 Hz (stable, smooth)
- New EMA: Would oscillate 997-1003 Hz (noisy)

Keyboard is **5-60 Hz** with **large** gaps (50-200ms), so we want fast response:
- Raw samples: 5 Hz, 10 Hz, 5 Hz (real pattern changes)
- Old EMA: Lags behind by 10-20 events
- New EMA: Tracks within 3-5 events ✅

## Decay Behavior (Responsive Stop Detection)

### Problem: Rates Stayed "Frozen" After Input Stopped
Without decay, rates would stay at their last calculated value indefinitely, making the TUI confusing and the scheduler less responsive.

### Solution: Per-Lane Decay Timeouts

Each input lane has a different decay timeout based on expected usage patterns:

| Lane | Decay Timeout | Reason |
|------|---------------|--------|
| **Mouse** | 10ms | High polling rate (1000-8000 Hz), 10ms gap = definitely stopped |
| **Keyboard** | 500ms | Slow typing/gaming has natural pauses (200-500ms between keys) |
| **Other** | 100ms | Middle ground for misc devices |

### How It Works

The timer callback (runs every 500µs = 2kHz) checks elapsed time since last input:

```c
// Keyboard: 500ms timeout
if (now - last_keyboard_event > 500ms) {
    keyboard_rate = 0 Hz;  // DECAY!
}

// Mouse: 10ms timeout  
if (now - last_mouse_event > 10ms) {
    mouse_rate = 0 Hz;  // DECAY!
}
```

### Expected TUI Behavior

**Before Decay Fix:**
```
Type "hello" → Keyboard: 12 Hz
Stop typing...
5 seconds later → Keyboard: 12 Hz  ❌ Still showing rate!
```

**After Decay Fix:**
```
Type "hello" → Keyboard: 12 Hz
Stop typing...
0.5 seconds later → Keyboard: 0 Hz  ✅ Decayed to zero!

Move mouse → Mouse: 1000 Hz
Stop moving...
0.01 seconds later → Mouse: 0 Hz  ✅ Ultra-fast decay!
```

### Why Different Timeouts?

**Mouse (10ms):**
- 8000 Hz mouse = 1 event every 0.125ms
- 1000 Hz mouse = 1 event every 1ms
- 10ms gap = at least 10 missed events = mouse stopped
- **Result:** Near-instant decay (10ms feels instant to humans)

**Keyboard (500ms):**
- Normal typing: 100-300ms between keypresses
- Gaming WASD: 200-500ms between direction changes
- 500ms gap = definitely stopped, not just thinking
- **Result:** Doesn't decay during natural typing pauses

### Continuous Input Mode

The decay also resets the "continuous input" flag:
```c
continuous_input_lane_mode[KEYBOARD] = 0;  // Exit burst mode
```

This affects the boost window scaling (line 114 in boost.bpf.h):
- Normal mode: 1x-20x boost based on rate
- Continuous mode: Sustained long boost window

### Scheduler Impact

**Responsive Priority Adjustment:**
1. **Input starts** → Rate climbs (3-5 events) → Boost active
2. **Input continues** → Rate stable → Sustained boost
3. **Input stops** → Rate decays (10-500ms) → Boost expires
4. **CPU priorities rebalance** → Non-input tasks get fair share

**Before decay:**
- Input stops but scheduler thinks input is still active for minutes
- Other tasks starved of priority unfairly

**After decay:**
- Input stops and scheduler knows within 10-500ms
- Fair CPU allocation resumes immediately

## Build and Test

```bash
cargo build --release
sudo ./target/release/scx_gamer --tui 1.0 --verbose
```

**Expected Results:**
1. First keypress: Shows ~5 Hz
2. After 3-5 keypresses: Shows accurate rate (8-12 Hz for typing)
3. Key hold: Climbs to 30-60 Hz (repeat rate)
4. WASD gaming: Stable at 3-8 Hz (intentional slow rate)
5. Fast spam: Reaches 15-25 Hz quickly

## Technical Details

**EMA Formula:**
- Old: `rate_new = (rate_prev * 7 + instant) >> 3` = (7/8 old + 1/8 new)
- New: `rate_new = (rate_prev + instant) >> 1` = (1/2 old + 1/2 new)

**Convergence Rate:**
- 50/50 EMA: α = 0.5 → reaches 90% in ~4 events
- 87.5/12.5 EMA: α = 0.125 → reaches 90% in ~18 events

**Why This Matters for Gaming:**
- Keyboard events directly affect scheduler boost priority
- Faster rate tracking = more responsive boost adjustments
- Better match between actual activity and displayed metrics
- User can see real-time feedback of input patterns

