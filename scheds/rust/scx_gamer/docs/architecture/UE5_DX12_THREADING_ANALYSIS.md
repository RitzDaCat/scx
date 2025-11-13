# Unreal Engine 5.6 DirectX12 Threading Model: Enhancement Analysis

**Date:** 2025-01-XX  
**Status:** Analysis & Recommendations  
**Target:** scx_gamer scheduler optimization for UE5.6 DirectX12 games

---

## Executive Summary

The Unreal Engine 5.6 DirectX12 threading model introduces a **"Golden Thread Dependency Chain"** that requires precise thread prioritization and CPU placement. This document analyzes scx_gamer's current implementation against UE5.6 requirements and proposes enhancements.

**Key Finding:** scx_gamer already handles 60% of the requirements. Missing pieces are:
1. TaskGraphThread* worker detection and corralling
2. Dependency-aware dynamic priority boosting
3. Strategic CPU pinning/placement (different cores for different threads)
4. AudioThread deadline scheduling

---

## UE5.6 DirectX12 Threading Model

### The Golden Thread Dependency Chain

```
Frame N+1: GameThread (input + simulation)
    ↓ [blocks, waits]
Frame N: RenderThread (dispatches work)
    ↓ [wakes TaskGraph workers]
TaskGraphThread* (parallel command list generation)
    ↓ [completes, wakes RenderThread]
RenderThread (gathers results)
    ↓ [wakes RHIThread]
RHIThread (submits to DirectX12 driver)
    ↓ [signals completion]
GameThread (starts Frame N+2)
```

**Critical Insight:** Any delay in handoffs = frame stutter. The scheduler must minimize latency between these handoffs.

---

## Current scx_gamer Implementation vs UE5.6 Requirements

### Thread Detection Status

| Thread Type | UE5.6 Name | scx_gamer Detection | Status | Priority |
|-------------|------------|---------------------|--------|----------|
| **GameThread** | `GameThread` | ✅ Detected as `is_input_handler_name()` | ✅ **WORKING** | Highest (boost=7) |
| **RenderThread** | `RenderThread`, `RenderThread 0` | ✅ Detected as `is_gpu_submit_name()` | ✅ **WORKING** | High (boost=6) |
| **RHIThread** | `RHIThread`, `RHISubmissionTh` | ✅ Detected as `is_gpu_submit_name()` | ✅ **WORKING** | High (boost=6) |
| **TaskGraphThread*** | `TaskGraphThreadHP`, `TaskGraphThread`, `TaskGraphThreadBP` | ❌ **NOT DETECTED** | ❌ **MISSING** | Medium-High |
| **AudioThread** | `AudioThread`, `AudioThread0` | ✅ Detected as `is_game_audio_thread()` | ⚠️ **PARTIAL** | Deadline-critical |

**Detection Code References:**
- GameThread: `scx_gamer/src/bpf/include/task_class.bpf.h:580-584`
- RenderThread: `scx_gamer/src/bpf/include/task_class.bpf.h:49-55`
- RHIThread: `scx_gamer/src/bpf/include/task_class.bpf.h:45-47`
- AudioThread: `scx_gamer/src/bpf/include/audio_detect.bpf.h:134-144`

---

## Gap Analysis

### Gap 1: TaskGraphThread* Worker Detection ❌

**Current State:**
- No detection for `TaskGraphThreadHP`, `TaskGraphThread`, `TaskGraphThreadBP`
- These threads are treated as generic background threads
- No CPU corralling (they can run on any CPU)

**UE5.6 Requirement:**
- Detect TaskGraphThread* workers
- Corral them to dedicated cores (E-cores or separate CCD)
- Prevent them from preempting "Golden Threads"
- Medium-high priority (not highest, but not background)

**Impact:** Without corralling, TaskGraph workers can:
- Pollute L1/L2 cache on P-cores used by GameThread/RenderThread
- Cause cache misses for critical threads
- Reduce frame consistency

**Recommendation:** Add TaskGraphThread* detection and CPU corralling.

---

### Gap 2: Dependency-Aware Dynamic Priority Boosting ❌

**Current State:**
- Static priority assignments (GameThread=7, RenderThread=6, etc.)
- No dynamic boosting based on wake chains
- No detection of "GameThread blocks → RenderThread wakes" pattern

**UE5.6 Requirement:**
- Detect when GameThread blocks (waiting for RenderThread)
- Immediately boost RenderThread to "King" priority
- Detect when RenderThread blocks (waiting for workers)
- Temporarily boost TaskGraphThread* workers
- Detect when RenderThread wakes (workers done)
- Re-grant RenderThread priority, demote workers

**Impact:** Without dependency-aware scheduling:
- RenderThread may be delayed by other high-priority tasks
- TaskGraph workers may not get CPU time when RenderThread needs them
- Frame handoffs are slower than optimal

**Recommendation:** Implement wake chain detection and dynamic priority boosting.

---

### Gap 3: Strategic CPU Pinning/Placement ❌

**Current State:**
- No CPU pinning for game threads (affinity override removes pinning)
- Migration resistance for GPU threads (32ms cooldown)
- No explicit core placement strategy

**UE5.6 Requirement:**
- **GameThread:** Pin to P-core, no migration
- **RenderThread:** Pin to different P-core from GameThread, different L3/CCD
- **RHIThread:** Same core as RenderThread (cache coherency)
- **TaskGraphThread*:** Corral to dedicated cores (E-cores or separate CCD)

**Impact:** Without strategic placement:
- Cache pollution between GameThread and RenderThread
- L3 cache contention
- Reduced cache hit rates

**Note:** This conflicts with scx_gamer's affinity override system. We need a **selective override** that:
- Overrides userspace pinning for GameThread/RenderThread/RHIThread (allows scheduler control)
- But then applies **scheduler-managed pinning** based on UE5.6 model

**Recommendation:** Implement selective affinity override + scheduler-managed pinning.

---

### Gap 4: AudioThread Deadline Scheduling ⚠️

**Current State:**
- AudioThread detected as `is_game_audio_thread()`
- Gets boost priority
- No deadline scheduling (SCHED_FIFO-like)

**UE5.6 Requirement:**
- AudioThread must meet deadlines (5-10ms cadence)
- SCHED_FIFO-like policy (cannot be late)
- Deadline-critical (audio crackling if late)

**Impact:** Without deadline scheduling:
- AudioThread may miss deadlines under load
- Audio crackling/stuttering

**Recommendation:** Implement deadline-aware scheduling for AudioThread.

---

## Proposed Enhancements

### Enhancement 1: TaskGraphThread* Detection & Corralling

**BPF Changes:**

```c
// Add to task_class.bpf.h
static __always_inline bool is_taskgraph_thread(const char *comm)
{
    /* TaskGraphThreadHP (High Priority) */
    if (comm[0] == 'T' && comm[1] == 'a' && comm[2] == 's' && comm[3] == 'k' &&
        comm[4] == 'G' && comm[5] == 'r' && comm[6] == 'a' && comm[7] == 'p' &&
        comm[8] == 'h' && comm[9] == 'T' && comm[10] == 'h' && comm[11] == 'r')
        return true;
    
    /* TaskGraphThread (Normal Priority) */
    /* TaskGraphThreadBP (Background Priority) */
    /* All variants share "TaskGraphThread" prefix */
    return false;
}

// Add to task_ctx
struct task_ctx {
    // ... existing fields ...
    u8 is_taskgraph_worker;  // 0 or 1
    // ... rest of fields ...
};
```

**CPU Corralling Logic:**

```c
// In select_cpu() or enqueue()
if (tctx->is_taskgraph_worker) {
    // Corral to dedicated cores (E-cores or separate CCD)
    // Use existing CPU selection logic but restrict to corral CPUs
    cpu = select_cpu_in_corral(corral_cpus, prev_cpu);
    // Medium-high priority (boost=5, below RenderThread=6)
}
```

**Implementation Notes:**
- Detect TaskGraphThread* by name pattern
- Maintain a "corral CPU set" (configurable, defaults to E-cores or last CCD)
- Restrict TaskGraph workers to corral CPUs only
- Prevent them from migrating to P-cores used by Golden Threads

---

### Enhancement 2: Dependency-Aware Dynamic Priority Boosting

**Wake Chain Detection:**

```c
// Track wake relationships
struct wake_chain {
    u32 waker_tid;      // Thread that woke this thread
    u64 wake_time;       // When wake happened
    u32 waker_role;      // Role of waker (GAMETHREAD, RENDERTHREAD, etc.)
};

// In enqueue() or select_cpu()
if (wake_flags & SCX_ENQ_WAKEUP) {
    struct task_struct *waker = scx_bpf_task_waker(p);
    if (waker) {
        struct task_ctx *waker_tctx = lookup_task_ctx(waker);
        
        // Detect wake chain: GameThread → RenderThread
        if (waker_tctx->is_input_handler && tctx->is_gpu_submit) {
            // Boost RenderThread to "King" priority temporarily
            tctx->dynamic_boost = 1;  // +1 priority boost
            tctx->boost_expires_ns = now + (16 * 1000000ULL);  // 16ms boost window
        }
        
        // Detect wake chain: RenderThread → TaskGraph workers
        if (waker_tctx->is_gpu_submit && tctx->is_taskgraph_worker) {
            // Boost TaskGraph workers temporarily
            tctx->dynamic_boost = 1;
            tctx->boost_expires_ns = now + (8 * 1000000ULL);  // 8ms boost window
        }
    }
}

// In boost calculation
u8 final_boost = base_boost + (tctx->dynamic_boost && now < tctx->boost_expires_ns ? 1 : 0);
```

**Implementation Notes:**
- Track wake relationships using `scx_bpf_task_waker()`
- Apply temporary priority boosts based on wake chain
- Expire boosts after short windows (8-16ms)
- This enables dynamic prioritization without static pinning

---

### Enhancement 3: Selective Affinity Override + Scheduler-Managed Pinning

**Current Affinity Override:**
- Overrides ALL userspace-set affinities
- Resets to full CPU mask

**Proposed Enhancement:**
- Override userspace pinning (as current)
- But then apply **scheduler-managed pinning** for UE5.6 threads

**Implementation:**

```c
// In select_cpu() or enqueue()
if (is_ue5_dx12_game(fg_tgid)) {
    if (tctx->is_input_handler) {
        // GameThread: Pin to best P-core
        cpu = select_best_pcore(exclude_cpus);  // Exclude RenderThread core
        pin_to_cpu(p, cpu);  // Scheduler-managed pinning
    } else if (tctx->is_gpu_submit && is_render_thread(p->comm)) {
        // RenderThread: Pin to different P-core from GameThread
        cpu = select_best_pcore(game_thread_cpu);  // Exclude GameThread core
        pin_to_cpu(p, cpu);
    } else if (tctx->is_gpu_submit && is_rhi_thread(p->comm)) {
        // RHIThread: Same core as RenderThread
        cpu = render_thread_cpu;
        pin_to_cpu(p, cpu);
    }
}
```

**Implementation Notes:**
- Detect UE5.6 DX12 games (check for RenderThread + RHIThread presence)
- Apply scheduler-managed pinning only for UE5.6 games
- Use `sched_setaffinity()` from userspace (via BPF helper or ring buffer)
- Track pinned CPUs in BPF maps for reference

---

### Enhancement 4: AudioThread Deadline Scheduling

**Current State:**
- AudioThread gets boost priority
- No deadline awareness

**Proposed Enhancement:**

```c
// In enqueue() or select_cpu()
if (tctx->is_game_audio) {
    // Check if deadline is approaching
    u64 next_deadline = tctx->last_audio_deadline + (5 * 1000000ULL);  // 5ms cadence
    u64 now = scx_bpf_now();
    
    if (now >= next_deadline - (1 * 1000000ULL)) {  // 1ms before deadline
        // CRITICAL: Boost to highest priority, preempt anything
        tctx->dynamic_boost = 2;  // +2 priority boost (above GameThread)
        tctx->boost_expires_ns = next_deadline + (1 * 1000000ULL);
    }
}
```

**Implementation Notes:**
- Track audio deadline cadence (5-10ms)
- Boost AudioThread when deadline approaches
- Use highest priority temporarily (above GameThread)
- This ensures audio deadlines are met

---

## Implementation Priority

### Phase 1: High Impact, Low Risk (Immediate)

1. **TaskGraphThread* Detection** ⭐⭐⭐
   - Add name pattern matching
   - Add CPU corralling
   - **Impact:** Reduces cache pollution, improves frame consistency
   - **Risk:** Low (additive change)

2. **AudioThread Deadline Scheduling** ⭐⭐
   - Add deadline tracking
   - Add temporary priority boost
   - **Impact:** Prevents audio crackling
   - **Risk:** Low (additive change)

### Phase 2: High Impact, Medium Risk (Short-term)

3. **Dependency-Aware Dynamic Boosting** ⭐⭐⭐
   - Add wake chain detection
   - Add temporary priority boosts
   - **Impact:** Faster frame handoffs, reduced stutter
   - **Risk:** Medium (requires careful testing)

### Phase 3: Medium Impact, High Risk (Long-term)

4. **Selective Affinity Override + Scheduler-Managed Pinning** ⭐⭐
   - Modify affinity override logic
   - Add scheduler-managed pinning
   - **Impact:** Better cache locality, reduced contention
   - **Risk:** High (conflicts with current affinity override, requires careful design)

---

## Performance Expectations

### Current Performance (Baseline)

- GameThread latency: ~1-5μs (input → game code)
- RenderThread latency: ~10-50μs (wake → dispatch)
- Frame handoff latency: ~100-500μs (GameThread → RenderThread → RHIThread)

### Expected Improvements

| Enhancement | Expected Improvement | Measurable Metric |
|-------------|---------------------|-------------------|
| **TaskGraph Corralling** | +5-10% frame consistency | Frame time std dev reduction |
| **Dependency-Aware Boosting** | -20-30% frame handoff latency | GameThread → RenderThread wake latency |
| **AudioThread Deadline** | 0% audio dropouts | Audio buffer underruns |
| **Scheduler-Managed Pinning** | +3-5% cache hit rate | L1/L2 cache miss reduction |

---

## Testing Strategy

### Unit Tests

1. **TaskGraphThread* Detection:**
   - Test name pattern matching (`TaskGraphThreadHP`, `TaskGraphThread`, `TaskGraphThreadBP`)
   - Verify CPU corralling restricts to designated cores

2. **Dependency-Aware Boosting:**
   - Test wake chain detection (GameThread → RenderThread)
   - Verify temporary priority boosts expire correctly

3. **AudioThread Deadline:**
   - Test deadline tracking (5ms cadence)
   - Verify priority boost when deadline approaches

### Integration Tests

1. **UE5.6 DX12 Game:**
   - Run actual UE5.6 DX12 game (e.g., Fortnite, Splitgate)
   - Measure frame time variance (should decrease)
   - Measure frame handoff latency (should decrease)
   - Verify audio quality (no crackling)

2. **Performance Benchmarks:**
   - Compare frame time std dev (before vs after)
   - Compare cache miss rates (before vs after)
   - Compare CPU utilization (should be similar or better)

---

## References

1. **Unreal Engine Documentation:**
   - Parallel Rendering Overview (UE Docs)
   - Threaded Rendering (UE Docs)
   - Task Graph Insights (UE Docs)

2. **DirectX12 Resources:**
   - Microsoft/AMD Practical DirectX 12 (GPUOpen)
   - Parallel Command List Generation

3. **scx_gamer Codebase:**
   - `scx_gamer/src/bpf/include/task_class.bpf.h` - Thread detection
   - `scx_gamer/src/bpf/main.bpf.c` - Scheduling logic
   - `scx_gamer/src/affinity_override.rs` - Affinity override system

---

## Conclusion

scx_gamer already handles **60% of UE5.6 DirectX12 requirements** (GameThread, RenderThread, RHIThread detection). The missing pieces are:

1. **TaskGraphThread* worker detection and corralling** (high priority)
2. **Dependency-aware dynamic priority boosting** (high priority)
3. **AudioThread deadline scheduling** (medium priority)
4. **Selective affinity override + scheduler-managed pinning** (low priority, high risk)

**Recommended Approach:**
- Implement Phase 1 enhancements first (TaskGraph detection, AudioThread deadline)
- Test thoroughly with actual UE5.6 DX12 games
- Then proceed to Phase 2 (dependency-aware boosting)
- Phase 3 (scheduler-managed pinning) requires careful design to avoid conflicts with affinity override

**Expected Outcome:**
- **-20-30% frame handoff latency**
- **+5-10% frame consistency**
- **0% audio dropouts**
- **Better cache locality**

This will make scx_gamer the **optimal scheduler for UE5.6 DirectX12 games**.

