# Wakeup Chain Front-Run: Critical Architectural Discussion

**Date:** 2025-11-05  
**Status:** Analysis & Implementation Discussion  
**Impact:** **CRITICAL** - Could eliminate 10µs-3ms+ wakeup chain latency

---

## The Core Problem

**Your nanosecond-level BPF scheduler is fighting a millisecond-level I/O path.**

The current scheduler is **reactive** - it waits for wakeups. But the theoretical fastest approach is **proactive** - detect input BEFORE any user-space process wakes up, then force-dispatch the game thread immediately.

---

## Current Implementation Analysis

### What We Have ✅

1. **fentry/input_event hook** (`main.bpf.c:1682`)
   - Detects input at kernel level (~10µs from hardware)
   - Sets input boost flags (`input_until_global`, `continuous_input_mode`)
   - Writes to ring buffer (for userspace)

2. **Game Detection**
   - `detected_fg_tgid` - knows which game is running
   - `is_exact_game_thread` - identifies game threads

3. **Force Dispatch Infrastructure**
   - `scx_bpf_dsq_insert()` - can force-dispatch tasks
   - Used for per-CPU kthreads, GPU threads, etc.

### What We're Missing ❌

1. **No game thread tracking map**
   - No way to lookup game thread `task_struct` from fentry hook
   - `game_threads_map` exists but only stores TIDs (u32), not pointers

2. **No force-dispatch in fentry hook**
   - Hook sets boost flags but doesn't dispatch game thread
   - Game thread waits for natural wakeup from compositor

3. **No "front-run" logic**
   - Current flow: Input → Compositor → Game (reactive)
   - Needed: Input → Game (proactive, parallel to compositor)

---

## Implementation Challenges

### Challenge 1: BPF Cannot Store task_struct Pointers

**Problem:** `task_struct` pointers become invalid when task exits. BPF verifier rejects storing them.

**Solutions:**

**Option A: Use Task Storage (Recommended)**
- Store game thread info in `task_ctx` (already per-task)
- In fentry hook, iterate tasks? ❌ Too expensive
- Better: Use per-CPU flag + enqueue hook ✅

**Option B: Per-CPU Flag + Enqueue Hook (Simplest)**
- fentry hook sets per-CPU flag: "input_arrived_for_game"
- When ANY game thread wakes (enqueue_task), check flag
- If flag set, force-dispatch immediately (bypass normal path)
- **Cost:** ~50-100ns (flag check + conditional dispatch)

**Option C: Store TIDs, Lookup via PID**
- BPF cannot do PID → task lookup
- Would need userspace assistance ❌ Too slow

### Challenge 2: How to Find Game Threads

**Current:** `is_exact_game_thread` checks `p->tgid == fg_tgid`

**In fentry hook:** No `task_struct` available - only input device

**Solution:** Don't dispatch from fentry hook
- Set per-CPU flag: "input_arrived_for_game"
- Dispatch in `enqueue_task` when game thread wakes
- This still breaks the chain (game wakes faster)

### Challenge 3: Task State Checking

**Problem:** Need to know if game thread is sleeping

**Solution:** 
- If task is in `enqueue_task`, it's waking up (was sleeping)
- If task is already on CPU, skip dispatch (already running)
- Use `scx_bpf_task_cpu()` - returns -1 if not on CPU

---

## Proposed Implementation (Simplified)

### Phase 1: Per-CPU Input Flag

**New BPF Map:**
```c
/* Per-CPU flag: input arrived for game (set by fentry hook) */
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __type(key, u32);
    __type(value, u64);  /* Timestamp when input arrived */
    __uint(max_entries, 1);
} input_arrived_for_game SEC(".maps");
```

**In fentry hook (`input_event_raw`):**
```c
if (should_boost && detected_fg_tgid != 0) {
    u32 key = 0;
    u64 now = scx_bpf_now();
    bpf_map_update_elem(&input_arrived_for_game, &key, &now, BPF_ANY);
    /* Flag set - enqueue hook will check this */
}
```

### Phase 2: Force Dispatch in Enqueue Hook

**In `gamer_enqueue` (after per-CPU kthread check):**
```c
/* FRONT-RUN: Check if input arrived and this is a game thread */
u32 fg_tgid = get_fg_tgid();
bool is_game_thread = fg_tgid && ((u32)p->tgid == fg_tgid);

if (is_game_thread) {
    u32 key = 0;
    u64 *input_time = bpf_map_lookup_elem(&input_arrived_for_game, &key);
    
    if (input_time && (now - *input_time) < 1000000) {  /* Within 1ms */
        /* Input arrived recently - force dispatch NOW (front-run compositor) */
        s32 cpu = scx_bpf_task_cpu(p);
        
        if (cpu < 0) {  /* Task not on CPU (sleeping) */
            /* Force dispatch to best CPU */
            cpu = pick_idle_cpu_cached(p, prev_cpu, enq_flags, true, &cache);
            if (cpu >= 0) {
                scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, task_slice(p), enq_flags);
                wakeup_cpu(cpu);
                
                /* Clear flag - input processed */
                u64 zero = 0;
                bpf_map_update_elem(&input_arrived_for_game, &key, &zero, BPF_ANY);
                
                PROF_END_HIST(enqueue);
                return;  /* INSTANT RETURN - bypass normal path */
            }
        }
    }
}
```

---

## Performance Analysis

### Current Path (Reactive)

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

### Proposed Path (Front-Run)

| Step | Latency | Cumulative |
|------|---------|------------|
| Hardware Input | 125µs | 125µs |
| Kernel Processing | 5-20µs | 130-145µs |
| **fentry Hook** | **50-100ns** | **130.05-145.1µs** |
| ├─ Set Flag | 10-30ns | |
| ├─ Set Boost | 10-30ns | |
| └─ Ring Buffer | 100-200ns (if stats) | |
| evdev Write | 1-5µs | 131.05-150.1µs |
| **Game Enqueue** | **~100-200ns** | **131.05-150.1µs** |
| ├─ Check Flag | 1-2ns | |
| ├─ Force Dispatch | 10-30ns | |
| └─ CPU Selection | 20-50ns | |
| **Game Running** | **N/A** | **Game already running!** |
| Compositor Wake | 1-5µs | 132.05-155.1µs |
| Compositor Process | 10-2000µs | 142.05-2155.1µs |
| **Total (Game)** | | **~131-150µs** |

**Latency Reduction:** **~27-2160µs** (17-94% improvement)

---

## Implementation Priority

### Priority: **HIGH** ⚠️

**Reasoning:**
- **Massive latency reduction:** 27-2160µs (17-94%)
- **Eliminates wakeup chain:** Core architectural improvement
- **Relatively simple:** Uses existing hooks, adds flag + dispatch
- **High impact:** Directly addresses input latency bottleneck

**Effort:** Low-Medium (1-2 hours)
- Add per-CPU input flag map
- Add flag check in enqueue hook
- Add force dispatch logic
- Test with Kovaaks

---

## Next Steps

1. **Review implementation approach** (per-CPU flag vs. task tracking)
2. **Implement per-CPU flag** (simplest approach)
3. **Add force dispatch in enqueue** (conditional on flag)
4. **Test with Kovaaks** (verify latency reduction)
5. **Measure impact** (compare before/after metrics)

---

**Last Updated:** 2025-11-05
