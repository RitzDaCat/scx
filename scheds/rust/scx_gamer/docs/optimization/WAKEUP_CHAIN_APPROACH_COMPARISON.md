# Wakeup Chain Front-Run: Approach Comparison

> **Update (2025-11-15):** The production implementation now stores wake flags in the `hotpath_signals` shared BSS (no helper calls) instead of the `input_arrived_for_game` map described below. This document keeps the original evaluation for reference; see `WAKEUP_CHAIN_IMPLEMENTATION.md` for the shipping layout.

**Date:** 2025-11-05  
**Question:** Which approach is faster - Compositor Wake Detection vs Direct Game Thread Dispatch?

---

## Timing Analysis

### Approach 1: Compositor Wake Detection (As Described)

**Timeline:**
```
T=0µs:     Input arrives (hardware)
T=~10µs:   fentry hook fires, sets per-CPU flag (~50ns)
T=~1-5µs:  Compositor wakes (enqueue_task fires)
           ├─ Check: is_compositor_task? (~1-2ns)
           ├─ Check: input flag set? (~1-2ns)
           ├─ Lookup: game thread from map (~20-50ns)
           └─ Force dispatch: scx_bpf_dsq_insert(~10-30ns)
           
T=~1-5µs:  Game thread FORCE-DISPATCHED (running in parallel!)
T=~10-2000µs: Compositor processes (game already running)
T=~10-2000µs: Compositor sends event to game (redundant, game already running)
```

**Total Latency:** **~1-5µs** (from input to game dispatch)

**Key Insight:** Game thread dispatched **immediately** when compositor wakes, before compositor processes.

---

### Approach 2: Direct Game Thread Dispatch

**Timeline:**
```
T=0µs:     Input arrives (hardware)
T=~10µs:   fentry hook fires, sets per-CPU flag (~50ns)
T=~10-2000µs: Compositor processes (game still sleeping)
T=~10-2000µs: Compositor sends event to game
T=~10-2000µs: Game thread wakes NATURALLY (enqueue_task fires)
           ├─ Check: is_game_thread? (~1-2ns)
           ├─ Check: input flag set? (~1-2ns)
           └─ Force dispatch: scx_bpf_dsq_insert(~10-30ns)
           
T=~10-2000µs: Game thread dispatched (AFTER compositor processing)
```

**Total Latency:** **~10-2000µs** (from input to game dispatch)

**Key Problem:** Game thread still waits for compositor to process and send event.

---

## Performance Comparison

| Metric | Approach 1 (Compositor Wake) | Approach 2 (Game Thread Wake) |
|--------|------------------------------|-------------------------------|
| **Latency** | **~1-5µs** | **~10-2000µs** |
| **Wakeup Chain** | ✅ **Broken** (game dispatched immediately) | ❌ **Still Exists** (waits for compositor) |
| **Compositor Dependency** | ✅ **Parallel** (game runs independently) | ❌ **Sequential** (game waits for compositor) |
| **Implementation Complexity** | Medium (need compositor detection) | Low (simple flag check) |
| **Performance Gain** | **27-2160µs saved** | **0µs saved** (defeats purpose) |

---

## Why Approach 1 is Faster

### The Critical Difference

**Approach 1:** Dispatches game thread **before** compositor processes
- Game runs in parallel with compositor
- Game doesn't wait for compositor to finish
- Breaks the wakeup chain

**Approach 2:** Dispatches game thread **after** compositor processes
- Game still waits for compositor event
- Compositor processing delay remains
- Wakeup chain still exists

### The Problem with Approach 2

Approach 2 **doesn't break the wakeup chain** because:
1. Game thread wakes naturally (waiting for compositor event)
2. Compositor must process input first (~10-2000µs)
3. Compositor sends event to game
4. Only then does game thread wake
5. Flag check happens too late

**Result:** Approach 2 provides **zero latency improvement** - game still waits for compositor.

---

## Implementation Details

### Approach 1: Compositor Wake Detection

**In `gamer_enqueue`:**
```c
/* After per-CPU kthread check */
bool is_compositor = (tctx && tctx->is_compositor) || is_compositor_name(p->comm);

if (is_compositor) {
    /* Check if input arrived recently */
    u32 key = 0;
    u64 *input_time = bpf_map_lookup_elem(&input_arrived_for_game, &key);
    
    if (input_time && (now - *input_time) < 1000000) {  /* Within 1ms */
        /* Input arrived - force dispatch game thread NOW */
        u32 fg_tgid = get_fg_tgid();
        if (fg_tgid != 0) {
            /* Lookup game thread task_struct from map */
            struct game_thread_info *game_info = bpf_map_lookup_elem(
                &game_threads_map, &fg_tgid);
            
            if (game_info && game_info->input_thread) {
                struct task_struct *game_task = game_info->input_thread;
                
                /* Force dispatch game thread */
                s32 cpu = pick_idle_cpu_cached(game_task, ...);
                if (cpu >= 0) {
                    scx_bpf_dsq_insert(game_task, SCX_DSQ_LOCAL_ON | cpu, 
                                      task_slice(game_task), 0);
                    wakeup_cpu(cpu);
                }
            }
        }
    }
}
```

**Challenge:** Need to store game thread `task_struct` pointers, which BPF can't do directly.

**Solution:** Use task storage + per-CPU array for recent game thread PIDs
- Store input handler thread PID in per-CPU array
- Lookup PID → task via `bpf_task_storage_get()` with PID key
- Or: Use userspace to maintain game thread PID list

---

### Approach 2: Direct Game Thread Dispatch

**In `gamer_enqueue`:**
```c
/* After per-CPU kthread check */
u32 fg_tgid = get_fg_tgid();
bool is_game_thread = fg_tgid && ((u32)p->tgid == fg_tgid);

if (is_game_thread) {
    /* Check if input arrived recently */
    u32 key = 0;
    u64 *input_time = bpf_map_lookup_elem(&input_arrived_for_game, &key);
    
    if (input_time && (now - *input_time) < 1000000) {  /* Within 1ms */
        /* Input arrived - force dispatch this game thread */
        s32 cpu = pick_idle_cpu_cached(p, prev_cpu, enq_flags, true, &cache);
        if (cpu >= 0) {
            scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, task_slice(p), enq_flags);
            wakeup_cpu(cpu);
            
            /* Clear flag */
            u64 zero = 0;
            bpf_map_update_elem(&input_arrived_for_game, &key, &zero, BPF_ANY);
            
            PROF_END_HIST(enqueue);
            return;  /* INSTANT RETURN */
        }
    }
}
```

**Challenge:** Game thread wakes **after** compositor processes, so latency improvement is zero.

---

## Conclusion

### Approach 1 is **MUCH FASTER** ✅

**Reasoning:**
- **Approach 1:** Dispatches game thread **immediately** when compositor wakes (~1-5µs)
- **Approach 2:** Dispatches game thread **after** compositor processes (~10-2000µs)
- **Difference:** Approach 1 saves **5-1995µs** compared to Approach 2

**Approach 1 breaks the wakeup chain. Approach 2 doesn't.**

### Implementation Recommendation

**Use Approach 1** - Compositor Wake Detection

**Why:**
- Achieves the goal: breaks wakeup chain
- Massive latency improvement: 27-2160µs saved
- Game runs in parallel with compositor

**Implementation Strategy:**
- Store game thread PIDs (not task_struct pointers) in per-CPU array
- Update PIDs when game threads classified (in `gamer_runnable`)
- In compositor `enqueue_task`, lookup game thread PID
- Use PID → task lookup (if available) or userspace assistance

**Alternative (Simpler):**
- Use userspace to maintain "input handler thread PID" list
- Update via BPF map from userspace
- BPF reads PID list and dispatches via `scx_bpf_dsq_insert()`

---

**Last Updated:** 2025-11-05

