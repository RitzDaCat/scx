# Wakeup Chain Front-Run Optimization Analysis

**Date:** 2025-11-05  
**Status:** Analysis Complete - Implementation Plan Ready  
**Impact:** **Critical** - Could eliminate 10µs-3ms+ wakeup chain latency

---

## Executive Summary

**The Problem:** Current scheduler reacts to wakeups (enqueue_task) AFTER the I/O path has already started. This creates a "wakeup chain" that adds 10µs-3ms+ latency:

```
Input Event → Compositor Wakes → Compositor Processes → Game Wakes → Game Runs
     ↓              ↓                    ↓                  ↓            ↓
  125µs         1-5µs              10-2000µs           1-5µs        5-50µs
```

**The Solution:** Use BPF fentry hook to detect input BEFORE compositor wakes, then force-dispatch game thread immediately. This "front-runs" the game thread, eliminating the wakeup chain.

**Expected Impact:** 
- **Latency Reduction:** 10µs-3ms+ → ~1-5µs (dominated only by first syscall wakeup)
- **Eliminates:** Two separate scheduling decisions
- **Eliminates:** User-space compositor processing delay

---

## Current Implementation Analysis

### Current Flow (Reactive)

**1. Input Event Detection:**
- ✅ **fentry/input_event hook exists** (`main.bpf.c:1682`)
- ✅ **Detects input at kernel level** (~10µs from hardware)
- ✅ **Writes to ring buffer** (for userspace processing)
- ✅ **Sets input boost flags** (continuous_input_mode, input_until_global)

**2. What Happens Next:**
- Compositor wakes up (waits for input from evdev)
- Compositor reads input event
- Compositor processes event (determines focused window)
- Compositor sends event to game
- **Game wakes up** (second wakeup event)
- Scheduler sees game is runnable
- Game runs

**3. The Gap:**
- ❌ **No force-dispatch** - Game thread waits for natural wakeup
- ❌ **No game thread lookup** - No map storing game thread pointers
- ❌ **No front-run logic** - fentry hook doesn't preemptively dispatch game

---

## Proposed "Front-Run" Implementation

### Architecture

```
Hardware Input (125µs)
    ↓
USB Controller → Kernel Input Core
    ↓
input_event() Function Called
    ↓
[fentry/input_event Hook] ← BPF "Scout" Program
    │
    ├─→ Check: Is gaming device? (Tier 1: 1-2ns)
    ├─→ Check: Is input for game? (Tier 1: 1-2ns)
    ├─→ Lookup: Game thread task_struct (Tier 2: 20-50ns)
    ├─→ Force Dispatch: scx_bpf_dsq_insert(game_task, ...) (Tier 2: 10-30ns)
    └─→ Set: Input boost flags (Tier 2: 10-30ns)
    
Total BPF Hook Cost: ~50-100ns

    ↓
evdev Writes Event → Compositor Wakes (1-5µs)
    │
    ├─→ [Compositor Processing] (10-2000µs)
    └─→ [Game Already Running!] ← Force-dispatched in parallel
```

---

## Implementation Plan

### Phase 1: Game Thread Tracking Map

**New BPF Map:**
```c
/* Game thread task_struct pointers for force dispatch
 * Key: TGID (process ID)
 * Value: Array of task_struct pointers (main thread, render thread, etc.)
 * 
 * PERF: Per-CPU array for lock-free access (Tier 1: 20-50ns)
 * Alternative: Task storage (Tier 2: 20-50ns) - already per-task
 */
struct {
    __uint(type, BPF_MAP_TYPE_HASH);
    __uint(max_entries, 1024);  /* Support up to 1024 game processes */
    __type(key, u32);            /* TGID */
    __type(value, struct game_thread_info);
} game_threads_map SEC(".maps");

struct game_thread_info {
    struct task_struct *main_thread;      /* Main game thread */
    struct task_struct *render_thread;    /* Render thread (if separate) */
    struct task_struct *input_thread;     /* Input handler thread */
    u32 num_threads;                      /* Total threads tracked */
    u64 last_update_ns;                   /* Last update timestamp */
};
```

**Update Logic:**
- Update map in `gamer_runnable()` when game thread detected
- Store task_struct pointers (valid for lifetime of process)
- Refresh on scheduler generation change (game switch)

---

### Phase 2: Enhanced fentry Hook

**Current Hook:** `input_event_raw` (`main.bpf.c:1682`)

**Additions:**
```c
SEC("fentry/input_event")
int BPF_PROG(input_event_raw, struct input_dev *dev,
             unsigned int type, unsigned int code, int value)
{
    /* Existing code: Device detection, ring buffer write, boost flags */
    
    /* NEW: Front-Run Logic - Force Dispatch Game Thread */
    if (likely(is_gaming_device(dev) && detected_fg_tgid != 0)) {
        /* Lookup game threads for this TGID */
        struct game_thread_info *game_info = bpf_map_lookup_elem(
            &game_threads_map, &detected_fg_tgid);
        
        if (game_info && game_info->main_thread) {
            struct task_struct *game_task = game_info->main_thread;
            
            /* Check if game thread is sleeping (waiting for input) */
            if (scx_bpf_task_state(game_task) == TASK_INTERRUPTIBLE ||
                scx_bpf_task_state(game_task) == TASK_UNINTERRUPTIBLE) {
                
                /* Force dispatch game thread NOW (before compositor processes) */
                s32 cpu = bpf_get_smp_processor_id();
                u64 slice = task_slice_fast(game_task, ...);
                
                scx_bpf_dsq_insert(game_task, SCX_DSQ_LOCAL_ON | cpu, slice, 0);
                
                /* PERF: This happens in ~50-100ns, BEFORE compositor wakes */
            }
        }
    }
    
    /* Continue with existing logic... */
}
```

---

### Phase 3: Task State Checking

**Challenge:** BPF cannot directly check `task_struct->state` (requires helper)

**Solution:** Use `scx_bpf_task_cpu()` or `scx_bpf_task_runnable()`
- If task is on a CPU (running): Skip dispatch (already running)
- If task is not on a CPU: Force dispatch (likely sleeping)

**Alternative:** Track sleep state in `task_ctx`
- Update `task_ctx->last_woke_at` when task wakes
- Check if `now - last_woke_at > threshold` → likely sleeping

---

## Performance Analysis

### Current Path (Default)

| Step | Latency | Cumulative |
|------|---------|------------|
| Hardware Input | 125µs | 125µs |
| Kernel Processing | 5-20µs | 130-145µs |
| evdev Write | 1-5µs | 131-150µs |
| **Compositor Wake** | **1-5µs** | **132-155µs** |
| Compositor Schedule | 5-50µs | 137-205µs |
| Compositor Process | 10-2000µs | 147-2205µs |
| **Game Wake** | **1-5µs** | **148-2210µs** |
| Game Schedule | 5-50µs | 153-2260µs |
| Game Runs | 5-50µs | 158-2310µs |
| **Total** | | **~158-2310µs** |

---

### Proposed Path (Front-Run)

| Step | Latency | Cumulative |
|------|---------|------------|
| Hardware Input | 125µs | 125µs |
| Kernel Processing | 5-20µs | 130-145µs |
| **fentry Hook** | **50-100ns** | **130.05-145.1µs** |
| ├─ Device Check | 1-2ns | |
| ├─ Game Lookup | 20-50ns | |
| ├─ Force Dispatch | 10-30ns | |
| └─ Boost Flags | 10-30ns | |
| **Game Force-Dispatched** | **~50-100ns** | **130.1-145.2µs** |
| evdev Write | 1-5µs | 131.1-150.2µs |
| Compositor Wake | 1-5µs | 132.1-155.2µs |
| Compositor Schedule | 5-50µs | 137.1-205.2µs |
| Compositor Process | 10-2000µs | 147.1-2205.2µs |
| **Game Already Running!** | **N/A** | **Game running in parallel** |
| **Total (Game)** | | **~130-145µs** |

**Latency Reduction:** **~28-2165µs** (eliminated wakeup chain)

---

## Implementation Challenges

### Challenge 1: Task Struct Lifetime

**Problem:** `task_struct` pointers become invalid when task exits

**Solution:**
- Verify task validity before dispatch (use `scx_bpf_task_pid()`)
- Refresh map entries periodically (on scheduler generation change)
- Use task storage instead of direct pointers (safer, same performance)

**Recommendation:** Use `BPF_MAP_TYPE_TASK_STORAGE` for game thread tracking
- Automatically invalidated on task exit
- Per-task, no manual cleanup needed
- Same performance (Tier 2: 20-50ns)

---

### Challenge 2: Multiple Game Threads

**Problem:** Games have multiple threads (main, render, input handler)

**Solution:**
- Track multiple threads per game (array in `game_thread_info`)
- Force-dispatch all relevant threads (main + input handler)
- Prioritize input handler thread (highest boost)

**Recommendation:** Focus on input handler thread initially
- Simplest implementation
- Highest impact (input processing)
- Can expand to other threads later

---

### Challenge 3: Compositor Still Processes

**Problem:** Compositor still needs to process input (can't skip)

**Solution:**
- **Don't skip compositor** - It still processes normally
- **Run game in parallel** - Game runs while compositor processes
- **Compositor → Game path still works** - Redundant but harmless

**Result:** Dual-path processing
- Fast path: Game force-dispatched (130-145µs)
- Slow path: Compositor → Game (147-2205µs)
- Game benefits from fast path, compositor path is redundant

---

## Expected Performance Impact

### Latency Reduction

**Best Case (Simple Input):**
- Current: ~158µs (compositor fast, game fast)
- Proposed: ~130µs (force dispatch)
- **Improvement:** ~28µs (18% reduction)

**Worst Case (Complex Compositor):**
- Current: ~2310µs (compositor slow, game slow)
- Proposed: ~130µs (force dispatch bypasses compositor)
- **Improvement:** ~2180µs (94% reduction)

**Average Case:**
- Current: ~500-1000µs (typical compositor processing)
- Proposed: ~130-145µs (force dispatch)
- **Improvement:** ~370-870µs (74-87% reduction)

---

### Framerate Impact

**Eliminated:**
- Jitter from compositor processing delays
- Variable wakeup timing
- Dependency on compositor responsiveness

**Improved:**
- Consistent input latency
- Predictable game thread wakeup
- Better frame timing

---

## Risk Assessment

### Low Risk ✅

**Why:**
- BPF hook already exists (proven safe)
- Force dispatch is standard sched-ext pattern
- Compositor path still works (redundant but safe)
- Game thread validation prevents invalid dispatches

**Mitigation:**
- Verify task validity before dispatch
- Use task storage for automatic cleanup
- Handle edge cases (task exit, game switch)

---

## Implementation Priority

### Priority: **HIGH** ⚠️

**Reasoning:**
- **Massive latency reduction:** 370-870µs average (74-87%)
- **Eliminates wakeup chain:** Core architectural improvement
- **Relatively simple:** Uses existing hooks, adds lookup + dispatch
- **High impact:** Directly addresses input latency bottleneck

**Effort:** Medium (2-4 hours)
- Add game thread tracking map
- Enhance fentry hook with force dispatch
- Add task validation logic
- Test with Kovaaks/aim trainers

---

## Next Steps

1. **Design game thread tracking** (map structure, update logic)
2. **Enhance fentry hook** (add force dispatch logic)
3. **Add task validation** (prevent invalid dispatches)
4. **Test with Kovaaks** (verify latency reduction)
5. **Measure impact** (compare before/after metrics)

---

**Last Updated:** 2025-11-05

