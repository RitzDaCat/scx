# Keyboard 0 Hz Debug Guide

## Changes Made

### 1. Fixed Division Bug (main.rs:986-988)
**Before:** Divided rates by 1000 → `60 Hz / 1000 = 0 Hz`  
**After:** Direct copy → `60 Hz = 60 Hz`

### 2. Fixed First Event Bug (boost.bpf.h:78-97)
**Before:** Skipped rate calculation on first keyboard event  
**After:** Initialize first event to 60 Hz

### 3. Fixed Keyboard Timeout (boost.bpf.h:83)
**Before:** 600ms window (too short for gaming keypresses)  
**After:** 2000ms window (allows WASD gaming patterns)

### 4. Added Event Filtering (main.rs:1216-1226)
**Before:** Triggered on ALL events (including SYN sync events)  
**After:** Only trigger on KEY, RELATIVE, ABSOLUTE events

### 5. Added Debug Logging (main.rs:714-720, 1211-1229)
- Shows which devices are registered with vendor/product IDs
- Logs every keyboard event with code and value
- Tracks event processing count

## Test Plan

### Step 1: Build with Debug Logging
```bash
cd /home/ritz/Documents/Repo/Linux/scx/scheds/rust/scx_gamer
cargo build --release
```

### Step 2: Run with Verbose Logging
```bash
sudo ./target/release/scx_gamer --tui 1.0 --verbose 2>&1 | tee keyboard_debug.log
```

### Step 3: Check Device Registration
Look for lines like:
```
Registered Keyboard device: Wooting 60HE (vendor=0x31e3 product=0x1210 fd=12 lane=Keyboard)
```

**Expected:**
- Should see your Wooting keyboard listed
- Vendor should be `0x31e3` (Wooting's vendor ID)
- Lane should be `Keyboard`

### Step 4: Test Keyboard Input

**Test 1: Single Keypress**
- Press and release W once
- Expected: Should see 2 events (press + release)
- Expected rate: ~60 Hz (initial value)

**Test 2: Rapid Keypresses**
- Spam W key 10 times quickly
- Expected: Should see 20 events (10 press + 10 release)
- Expected rate: Should climb to match your actual rate (5-15 Hz)

**Test 3: Key Hold**
- Hold W key down for 2 seconds
- Expected: Should see continuous repeat events (EV_KEY value=2)
- Expected rate: Should show high Hz (30-60+ Hz)

**Test 4: Slow Typing**
- Type W, wait 1 second, type A, wait 1 second, type S
- Expected: Rate should stay alive between keypresses
- Expected rate: ~1-2 Hz

### Step 5: Check Log Output

Search for these patterns:
```bash
# Check device registration
grep "Registered Keyboard" keyboard_debug.log

# Check event processing
grep "Keyboard event" keyboard_debug.log | head -20

# Check event counts
grep "Processed.*events from keyboard" keyboard_debug.log
```

**Expected Event Types:**
- `value=1` = Key press (should trigger boost)
- `value=0` = Key release (should trigger boost)
- `value=2` = Key repeat/hold (should trigger boost)

## Troubleshooting

### Issue: Wooting Not Detected
If you don't see the Wooting in the registered devices:

1. Check if it's visible to evdev:
```bash
sudo evtest
# Should list: "Wooting 60HE"
```

2. Check vendor ID:
```bash
cat /sys/class/input/event*/device/id/vendor
# Should show "31e3" for Wooting
```

3. If vendor ID is different, update main.rs line 415:
```rust
0x31e3 => return DeviceType::Keyboard, // Your Wooting vendor ID
```

### Issue: Events Not Being Processed
If devices are registered but events aren't logged:

1. Check file permissions:
```bash
ls -la /dev/input/event*
# Scheduler needs read access
```

2. Run as root:
```bash
sudo ./target/release/scx_gamer --tui 1.0 --verbose
```

### Issue: Rate Still Shows 0 Hz
If events are being processed but rate is still 0:

1. Check if BPF is updating the rate:
```bash
# In another terminal while scheduler is running:
sudo bpftool map dump name input_lane_trigger_rate
```

2. Check if metrics are being read correctly:
```bash
# Look for this in the log:
grep "input_lane_keyboard_rate" keyboard_debug.log
```

## Expected Behavior

### Normal Gaming (WASD Movement)
- Rate: 2-10 Hz (individual keypresses with 100-500ms gaps)
- Shows activity even with 1-2s between keys
- Should NOT drop to 0 Hz unless no input for 2+ seconds

### Rapid Typing
- Rate: 10-30 Hz (quick sequential keypresses)
- Should track bursts of activity

### Key Holding (Continuous)
- Rate: 30-60+ Hz (key repeat events)
- Should show sustained high rate while held

### Mouse for Comparison
- Rate: 125-8000 Hz (depending on polling rate)
- Should be much higher than keyboard
- Helps verify the system is working

## Key Code Locations

1. **Device Classification:** `main.rs:405-459`
2. **Device Registration:** `main.rs:699-732`
3. **Event Processing:** `main.rs:1205-1234`
4. **BPF Trigger:** `bpf_intf.rs:77-96`
5. **Rate Calculation:** `boost.bpf.h:69-124`
6. **Metrics Reading:** `main.rs:986-988`
7. **TUI Display:** `tui.rs:35-80`

## Quick Verification Commands

```bash
# 1. Check Wooting is detected by kernel
lsusb | grep -i wooting

# 2. Check evdev devices
sudo evtest 2>&1 | grep -i wooting

# 3. Watch events in real-time
sudo evtest /dev/input/eventX  # Replace X with your Wooting event number

# 4. Test the scheduler with logging
sudo ./target/release/scx_gamer --verbose --tui 1.0 2>&1 | grep -E "(Registered|Keyboard event|Processed)"
```

