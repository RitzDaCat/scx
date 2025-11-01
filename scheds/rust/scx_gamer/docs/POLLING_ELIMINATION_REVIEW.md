# Polling Elimination Review

**Date:** 2025-01-29  
**Purpose:** Identify all polling operations and replace with event-driven or faster alternatives

---

## Executive Summary

**Current State:** The scheduler is **already highly optimized** with most hot paths using event-driven mechanisms. However, several polling operations remain that could be improved.

**Key Findings:**
- ✅ **Already Event-Driven:** Input events, game detection (inotify), audio server detection (inotify), ring buffer processing
- ⚠️ **Polling Operations Identified:** 8 operations using periodic polling
- 🎯 **Optimization Opportunities:** 3 high-priority, 3 medium-priority, 2 low-priority

---

## Detailed Analysis

### 1. ⚠️ epoll_wait Timeout (100ms) - MEDIUM PRIORITY

**Location:** `src/main.rs:2287-2291`

**Current Implementation:**
```rust
const EPOLL_TIMEOUT_MS: u16 = 100; // 100ms timeout for responsive shutdown and stats
match epfd.wait(&mut events, Some(EPOLL_TIMEOUT_MS)) {
```

**Issue:** 100ms timeout means we wake up every 100ms even when no events occur, just to check shutdown/stats.

**Impact:** 
- Wakeup frequency: 10Hz (every 100ms)
- CPU overhead: ~5-10µs per wakeup (epoll_wait overhead)
- Total overhead: ~50-100µs/sec

**Optimization Options:**

#### Option A: Increase Timeout (LOW IMPACT)
- **Change:** Increase timeout to 1000ms (1 second)
- **Benefit:** Reduces wakeups from 10Hz → 1Hz (~90% reduction)
- **Trade-off:** Shutdown response time increases from 100ms → 1000ms (still acceptable)
- **Complexity:** Trivial (1 line change)
- **Recommendation:** ✅ **IMPLEMENT** - Minimal risk, significant CPU savings

#### Option B: Separate Timer FD for Shutdown/Stats (HIGH IMPACT)
- **Change:** Use `timerfd` for periodic stats checks, remove timeout from epoll_wait
- **Benefit:** Zero wakeups when idle (no timeout needed)
- **Trade-off:** Additional FD management complexity
- **Complexity:** Medium (requires timerfd setup)
- **Recommendation:** ⚠️ **CONSIDER** - Better but more complex

**Priority:** **MEDIUM** - Current overhead is low, but can be eliminated

---

### 2. ⚠️ Watchdog Check (Every 100ms) - HIGH PRIORITY

**Location:** `src/main.rs:2674-2702`

**Current Implementation:**
```rust
if watchdog_enabled && last_watchdog_check.elapsed() >= Duration::from_millis(100) {
    last_watchdog_check = Instant::now();
    let dispatch_total = bss.nr_direct_dispatches + bss.nr_shared_dispatches;
    if dispatch_total != last_dispatch_total {
        // Progress detected
    } else if last_progress_t.elapsed() >= Duration::from_secs(effective_watchdog_secs) {
        // Deadlock detected
    }
}
```

**Issue:** Polling BPF map every 100ms (10Hz) to check dispatch progress.

**Impact:**
- Wakeup frequency: 10Hz (every 100ms)
- BPF map read overhead: ~50-100ns per read
- Total overhead: ~500-1000ns/sec (negligible but unnecessary)

**Optimization Options:**

#### Option A: Event-Driven via Ring Buffer (HIGH IMPACT)
- **Change:** Emit dispatch events to ring buffer when dispatches occur
- **Benefit:** Zero polling overhead - only checks when dispatches happen
- **Implementation:** BPF emits dispatch event → userspace reads from ring buffer → updates watchdog
- **Complexity:** Medium (requires BPF ring buffer integration)
- **Recommendation:** ✅ **IMPLEMENT** - Eliminates polling completely

#### Option B: Reduce Polling Frequency (LOW IMPACT)
- **Change:** Increase check interval from 100ms → 500ms
- **Benefit:** 80% reduction in polling frequency
- **Trade-off:** Deadlock detection delay increases from 100ms → 500ms (still acceptable)
- **Complexity:** Trivial (1 line change)
- **Recommendation:** ⚠️ **CONSIDER** - Quick win but not optimal

**Priority:** **HIGH** - Can be completely eliminated with event-driven approach

---

### 3. ⚠️ Ring Buffer Overflow Check (Every 1 Second) - LOW PRIORITY

**Location:** `src/main.rs:2740-2789`

**Current Implementation:**
```rust
if last_overflow_check.elapsed() >= Duration::from_secs(1) {
    last_overflow_check = Instant::now();
    // Read overflow count from BPF stats
    let current_overflow = { /* BPF map read */ };
    // Detect rapid overflow increase
}
```

**Issue:** Polling BPF map every 1 second to check for overflow events.

**Impact:**
- Wakeup frequency: 1Hz (every 1 second)
- BPF map read overhead: ~50-100ns per read
- Total overhead: ~50-100ns/sec (negligible)

**Optimization Options:**

#### Option A: Event-Driven via Ring Buffer (MEDIUM IMPACT)
- **Change:** Emit overflow events to ring buffer when overflow occurs
- **Benefit:** Zero polling overhead - only checks when overflow happens
- **Implementation:** BPF emits overflow event → userspace reads from ring buffer → alerts
- **Complexity:** Low (ring buffer already exists, just add event emission)
- **Recommendation:** ✅ **IMPLEMENT** - Clean event-driven approach

#### Option B: Remove Periodic Check (LOW IMPACT)
- **Change:** Only check overflow when processing ring buffer events
- **Benefit:** Eliminates separate polling loop
- **Trade-off:** Overflows might not be detected immediately (but they're rare)
- **Complexity:** Trivial (remove check, add to ring buffer processing)
- **Recommendation:** ⚠️ **CONSIDER** - Simplest solution

**Priority:** **LOW** - Low overhead, but can be improved

---

### 4. ⚠️ Game Detection Liveness Check (Every 5 Seconds) - MEDIUM PRIORITY

**Location:** `src/game_detect.rs:235-244`

**Current Implementation:**
```rust
if last_liveness_check.elapsed() >= LIVENESS_CHECK_INTERVAL {
    last_liveness_check = std::time::Instant::now();
    if let Some(ref game) = cache.last_game {
        if !process_exists(game.tgid) {
            // Game exited
        }
    }
}
```

**Issue:** Polling `/proc` every 5 seconds to check if game process still exists.

**Impact:**
- Wakeup frequency: 0.2Hz (every 5 seconds)
- `/proc` read overhead: ~1-5µs per read (`stat` syscall)
- Total overhead: ~0.2-1µs/sec (negligible)

**Optimization Options:**

#### Option A: Event-Driven via inotify/BPF LSM (HIGH IMPACT)
- **Change:** Use inotify or BPF LSM hook to detect process exit
- **Benefit:** Zero polling overhead - instant detection when process exits
- **Implementation:** BPF LSM `task_free` hook already exists → emit event → userspace reads
- **Complexity:** Low (LSM hook already implemented, just needs event emission)
- **Recommendation:** ✅ **IMPLEMENT** - Already have the infrastructure

#### Option B: Use Existing LSM Hook (HIGH IMPACT)
- **Change:** `game_detect_lsm.bpf.c` already has `task_free` hook → emit exit event to ring buffer
- **Benefit:** Instant detection (<1ms latency vs 0-5s delay)
- **Implementation:** Add ring buffer event to `BPF_PROG(game_detect_exit)` in `game_detect_lsm.bpf.c`
- **Complexity:** Low (ring buffer already exists)
- **Recommendation:** ✅ **IMPLEMENT** - Best solution, already partially implemented

**Priority:** **MEDIUM** - Low overhead but can be improved with existing infrastructure

---

### 5. ⚠️ Periodic Performance Logging (Every 10 Seconds) - LOW PRIORITY

**Location:** `src/main.rs:2626-2667`

**Current Implementation:**
```rust
if last_performance_log.elapsed() >= Duration::from_secs(10) {
    last_performance_log = Instant::now();
    // Log epoll wait times, ring buffer stats, etc.
}
```

**Issue:** Periodic logging of performance metrics every 10 seconds.

**Impact:**
- Wakeup frequency: 0.1Hz (every 10 seconds)
- Logging overhead: ~10-50µs per log
- Total overhead: ~1-5µs/sec (negligible)

**Optimization Options:**

#### Option A: Keep as-is (RECOMMENDED)
- **Rationale:** Logging is necessary for debugging/monitoring, overhead is negligible
- **Recommendation:** ✅ **KEEP** - No optimization needed

**Priority:** **LOW** - Overhead is negligible, logging is useful

---

### 6. ⚠️ Periodic Metrics Logging (Every 10 Seconds) - LOW PRIORITY

**Location:** `src/main.rs:2705-2735`

**Current Implementation:**
```rust
if last_metrics_log.elapsed() >= Duration::from_secs(10) {
    last_metrics_log = Instant::now();
    // Log migration and hint metrics
}
```

**Issue:** Periodic logging of migration/hint metrics every 10 seconds.

**Impact:**
- Wakeup frequency: 0.1Hz (every 10 seconds)
- BPF map read overhead: ~50-100ns per read
- Logging overhead: ~10-50µs per log
- Total overhead: ~1-5µs/sec (negligible)

**Optimization Options:**

#### Option A: Keep as-is (RECOMMENDED)
- **Rationale:** Logging is necessary for debugging/monitoring, overhead is negligible
- **Recommendation:** ✅ **KEEP** - No optimization needed

**Priority:** **LOW** - Overhead is negligible, logging is useful

---

### 7. ⚠️ Stats Thread (Every 1 Second) - MEDIUM PRIORITY

**Location:** `src/main.rs:3072-3085`

**Current Implementation:**
```rust
let stats_interval = Duration::from_secs(1); // 1 second updates
loop {
    std::thread::sleep(stats_interval);
    // Trigger stats request
}
```

**Issue:** Background thread sleeps for 1 second, then triggers stats request.

**Impact:**
- Wakeup frequency: 1Hz (every 1 second)
- Thread wakeup overhead: ~1-5µs per wakeup
- Stats request overhead: ~100-200µs per request (includes BPF map reads)
- Total overhead: ~100-205µs/sec

**Optimization Options:**

#### Option A: Event-Driven via Ring Buffer (HIGH IMPACT)
- **Change:** Emit stats events to ring buffer when metrics change significantly
- **Benefit:** Eliminates polling thread - stats updated only when needed
- **Trade-off:** Need to define "significant change" threshold
- **Complexity:** Medium (requires BPF → ring buffer → stats update)
- **Recommendation:** ⚠️ **CONSIDER** - Complex but eliminates polling

#### Option B: Increase Interval (LOW IMPACT)
- **Change:** Increase stats interval from 1s → 5s
- **Benefit:** 80% reduction in polling frequency
- **Trade-off:** Stats update delay increases (may be acceptable)
- **Complexity:** Trivial (1 line change)
- **Recommendation:** ⚠️ **CONSIDER** - Quick win

#### Option C: Keep as-is (RECOMMENDED)
- **Rationale:** Stats thread is necessary for TUI/debug API, overhead is acceptable
- **Recommendation:** ✅ **KEEP** - Current implementation is reasonable

**Priority:** **MEDIUM** - Overhead is acceptable, but could be improved

---

### 8. ⚠️ BPF Timer-Based Aggregation (Every 5ms) - LOW PRIORITY

**Location:** `src/bpf/main.bpf.c:1993-2328`

**Current Implementation:**
```c
if (!no_stats && (timer_tick_counter % 10) == 0) {
    // Aggregate per-CPU counters into globals (every 10 ticks = 5ms)
}
```

**Issue:** BPF timer runs every 500µs, aggregates stats every 10 ticks (5ms).

**Impact:**
- Timer frequency: 2000Hz (every 500µs)
- Aggregation frequency: 200Hz (every 5ms)
- Aggregation overhead: ~100-200ns per aggregation
- Total overhead: ~20-40µs/sec

**Optimization Options:**

#### Option A: Keep as-is (RECOMMENDED)
- **Rationale:** Timer is necessary for deadline tracking, aggregation overhead is minimal
- **Recommendation:** ✅ **KEEP** - Already optimized (rate-limited to every 10 ticks)

**Priority:** **LOW** - Already optimized, overhead is minimal

---

## Summary Table

| Operation | Frequency | Overhead | Priority | Recommendation |
|-----------|-----------|----------|----------|----------------|
| epoll_wait timeout | 10Hz (100ms) | ~50-100µs/sec | MEDIUM | Increase timeout to 1s |
| Watchdog check | 10Hz (100ms) | ~500-1000ns/sec | HIGH | Event-driven via ring buffer |
| Ring buffer overflow | 1Hz (1s) | ~50-100ns/sec | LOW | Event-driven via ring buffer |
| Game liveness check | 0.2Hz (5s) | ~0.2-1µs/sec | MEDIUM | Event-driven via LSM hook |
| Performance logging | 0.1Hz (10s) | ~1-5µs/sec | LOW | Keep as-is |
| Metrics logging | 0.1Hz (10s) | ~1-5µs/sec | LOW | Keep as-is |
| Stats thread | 1Hz (1s) | ~100-205µs/sec | MEDIUM | Consider increase interval |
| BPF timer aggregation | 200Hz (5ms) | ~20-40µs/sec | LOW | Keep as-is |

---

## Recommendations

### High Priority (Implement Now)

1. **Watchdog Check → Event-Driven**
   - Emit dispatch events to ring buffer when dispatches occur
   - Eliminates 10Hz polling completely
   - Benefit: Zero polling overhead

### Medium Priority (Consider Implementing)

2. **Game Liveness Check → Event-Driven**
   - Use existing BPF LSM `task_free` hook → emit exit event
   - Eliminates 0.2Hz polling
   - Benefit: Instant detection (<1ms vs 0-5s delay)

3. **epoll_wait Timeout → Increase**
   - Increase timeout from 100ms → 1000ms
   - Reduces wakeups from 10Hz → 1Hz
   - Benefit: ~90% reduction in wakeups

4. **Stats Thread → Increase Interval**
   - Increase interval from 1s → 5s
   - Reduces polling frequency by 80%
   - Benefit: Lower CPU overhead

### Low Priority (Optional)

5. **Ring Buffer Overflow → Event-Driven**
   - Emit overflow events to ring buffer when overflow occurs
   - Eliminates 1Hz polling
   - Benefit: Cleaner event-driven approach

---

## Implementation Priority

1. ✅ **Watchdog Check** - Highest impact, eliminates 10Hz polling
2. ✅ **Game Liveness Check** - Already have infrastructure, easy win
3. ✅ **epoll_wait Timeout** - Trivial change, significant benefit
4. ⚠️ **Stats Thread** - Consider increasing interval
5. ⚠️ **Ring Buffer Overflow** - Optional, low overhead

---

## Total Overhead Reduction

**Current Polling Overhead:** ~200-350µs/sec  
**After Optimizations:** ~50-100µs/sec  
**Reduction:** ~70-85% reduction in polling overhead

**Note:** These optimizations are primarily about **clean architecture** and **eliminating unnecessary wakeups** rather than performance-critical improvements. The scheduler is already highly optimized, and these are incremental improvements.

