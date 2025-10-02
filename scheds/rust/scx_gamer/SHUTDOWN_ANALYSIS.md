# Shutdown Hang Analysis - scx_gamer

## Problem
After running for several hours, Ctrl+C does not cleanly exit the scheduler.

## Root Causes Identified

### 1. **Stats Channel Deadlock** ⚠️ CRITICAL

**Location**: `src/main.rs` lines 717-721

```rust
// Service any pending stats requests without blocking
while stats_request_rx.try_recv().is_ok() {
    let metrics = self.get_metrics();
    stats_response_tx.send(metrics)?;  // ⚠️ BLOCKING SEND!
}
```

**Problem**:
- The main loop uses `try_recv()` (non-blocking) to receive stats requests
- BUT uses `.send()` (BLOCKING) to send responses
- If the stats thread dies or stops receiving, `.send()` will **block forever**
- The `?` operator means any error will break the loop, but blocking prevents that

**Why it hangs**:
1. Stats thread might be blocked in `recv()` waiting for data
2. Main loop tries to `send()` response
3. If channel buffer is full, `send()` blocks
4. Ctrl+C flag is set, but main loop never checks it (stuck in `send()`)

### 2. **Stats Thread May Not Respect Shutdown Flag**

**Location**: `src/stats.rs` line 211-218

```rust
pub fn monitor(intv: Duration, shutdown: Arc<AtomicBool>) -> Result<()> {
    scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        || shutdown.load(Ordering::Relaxed),  // ✅ Checks shutdown
        |metrics| metrics.format(&mut std::io::stdout()),
    )
}
```

This calls `scx_utils::monitor_stats()` which we don't control. If that function:
- Blocks on I/O (stdout)
- Doesn't check the shutdown closure frequently
- Has internal deadlocks

...then the stats thread won't exit cleanly.

### 3. **Stats Thread Join Timeout Too Short**

**Location**: `src/main.rs` lines 896-913

```rust
// Give it 1 second to finish gracefully
let mut joined = false;
for _ in 0..10 {
    if jh.is_finished() {
        let _ = jh.join();
        joined = true;
        break;
    }
    std::thread::sleep(Duration::from_millis(100));
}
if !joined {
    warn!("Stats thread didn't finish in time, detaching");  // ⚠️ Just detaches!
}
```

**Problem**:
- Only waits 1 second (10 × 100ms)
- If stats thread is blocked, this just gives up and **detaches**
- The main thread exits, but stats thread is still running
- Process appears hung because **stats thread is keeping it alive**

### 4. **String Clone in Delta Calculation**

**Location**: `src/stats.rs` line 139

```rust
fg_app: self.fg_app.clone(),  // ⚠️ Repeated String clones
```

**Issue**: Not causing hang, but after hours of operation:
- If `fg_app` is a long string
- Cloned every stats interval (every 1 second with `--stats 1.0`)
- Could cause memory fragmentation (not leak, but churn)

---

## Memory Growth Analysis

### Likely NOT Memory Leaks

The stats code doesn't have obvious memory leaks:
- ✅ All allocations are on stack or in `Metrics` struct
- ✅ No `Vec::push()` without bounds
- ✅ No `HashMap` accumulation
- ✅ String clone is replaced, not accumulated

### Potential Memory Churn

After hours of operation:
- String clones every second: `3,600 clones/hour`
- After 10 hours: `36,000 clones`
- If `fg_app` is long, this creates allocator pressure

---

## Why Ctrl+C Hangs

**Most Likely Scenario**:

1. Main loop runs for hours
2. Stats thread makes request via channel
3. Main loop calls `stats_response_tx.send(metrics)?`
4. **Stats thread is blocked** (maybe stdout buffer full, or internal deadlock)
5. Channel buffer fills up
6. `send()` blocks waiting for stats thread to receive
7. User presses Ctrl+C
8. `shutdown` flag is set to `true`
9. **Main loop never checks flag** - it's stuck in `send()`
10. After 1 second timeout, main thread detaches stats thread
11. **Stats thread continues running**, keeping process alive
12. Process appears hung

---

## Fixes Required

### Fix 1: Non-Blocking Stats Response ✅ CRITICAL

```rust
// Service any pending stats requests without blocking
while stats_request_rx.try_recv().is_ok() {
    let metrics = self.get_metrics();
    // ✅ Use try_send instead of send to avoid blocking
    match stats_response_tx.try_send(metrics) {
        Ok(_) => {},
        Err(e) => {
            // Channel full or disconnected - stats thread is slow/dead
            warn!("Failed to send stats response: {:?}", e);
            break;  // Exit stats servicing, don't hang
        }
    }
}
```

### Fix 2: Abort Stats Thread on Timeout ✅ IMPORTANT

```rust
// Wait for stats thread to finish (with timeout) - only for --stats mode
if opts.stats.is_some() {
    if let Some(jh) = stats_thread {
        info!("Waiting for stats thread to finish...");
        // Give it 2 seconds to finish gracefully
        let mut joined = false;
        for _ in 0..20 {
            if jh.is_finished() {
                let _ = jh.join();
                joined = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !joined {
            warn!("Stats thread didn't finish in time - this is a bug!");
            // Note: Can't force-kill thread in safe Rust
            // Process will exit anyway, orphaning the thread
        }
    }
}
```

### Fix 3: Avoid Repeated String Clones ✅ OPTIMIZATION

```rust
fn delta(&self, prev: &Self) -> Self {
    Self {
        // ... other fields ...
        fg_app: self.fg_app.clone(),  // Still needed, but consider:
        // Alternative: Use Arc<String> in Metrics to avoid clones
        // ... other fields ...
    }
}
```

Or change `Metrics::fg_app` to `Arc<String>`:
```rust
pub fg_app: Arc<String>,  // Cheap to clone
```

---

## Testing the Fix

After applying Fix 1 & 2:

```bash
# Run scheduler
sudo /path/to/scx_gamer --stats 1.0

# In another terminal, wait 10 seconds then:
# Press Ctrl+C in scheduler terminal

# Expected behavior:
# - Exits within 2 seconds
# - Logs: "Scheduler main loop exited, cleaning up..."
# - Logs: "Unregister scx_gamer scheduler"
# - Clean exit

# If still hangs:
ps aux | grep scx_gamer
# Kill with: sudo kill -9 <PID>
```

---

## Long-Term Monitoring

To detect if this happens again:

```bash
# Monitor process memory over time
watch -n 60 'ps -p <PID> -o pid,vsz,rss,stat,wchan:20'

# Check if WCHAN shows blocking:
# - "pipe_r" = blocked reading pipe
# - "futex" = blocked on lock
# - "-" = running normally
```

---

## Summary

**Root Cause**: `stats_response_tx.send()` is blocking, preventing shutdown flag from being checked.

**Fix Priority**:
1. ✅ **CRITICAL**: Change `send()` to `try_send()` (Fix 1)
2. ✅ **IMPORTANT**: Better stats thread handling (Fix 2)
3. ⏳ **OPTIONAL**: Optimize string clones (Fix 3)

**Impact**: Should fix Ctrl+C hang completely.
