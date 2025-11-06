# Analysis of `migration_disabled` Runtime Errors

**Date:** 2025-11-06  
**Status:** Resolved. This document serves as a post-mortem analysis of two related runtime crashes and the implemented solution.

---

## 1. Problem Summary

The `scx_gamer` scheduler experienced three distinct runtime crashes, all resulting in a kernel panic and a debug dump. The core issue in all cases was an attempt by the scheduler's aggressive dispatch logic to move a task that was marked as "migration disabled."

The error message was definitive:
`runtime error (SCX_DSQ_LOCAL[_ON] cannot move migration disabled ...)`

This indicates the `sched_ext` framework correctly intercepted and blocked an illegal scheduling decision.

**Critical Insight:** There are **two distinct types** of migration restrictions:
1. **Permanent CPU Affinity:** A task is restricted to a set of CPUs via `taskset` or `cpus_ptr`. This is checked via the CPU affinity mask.
2. **Temporary Migration Disable:** A driver temporarily calls `migrate_disable()` during a critical section. The affinity mask doesn't change, but the task cannot be moved.

---

## 2. Incident Analysis

### Incident 1: `vkd3d-swapchain` Crash (Enqueue Path)

-   **Task:** `vkd3d-swapchain[260496]`
-   **Operation:** The scheduler attempted to move the task from CPU 6 to CPU 1.
-   **Trigger:** This error occurred in the `gamer_enqueue` BPF hook. One of our new "front-running" optimizations (for input, compositor, or audio) identified this graphics thread as critical and tried to force-dispatch it to an idle CPU.
-   **Root Cause:** The task had `migrate_disable()` active, likely because the GPU driver was in a critical section during an `ioctl` syscall. Our front-running logic did not check for this state before attempting the move.

### Incident 2: `RHISubmissionTh` Crash (Idle Steal Path)

-   **Task:** `RHISubmissionTh[260459]`
-   **Operation:** The scheduler attempted to move the task from CPU 6 to CPU 4.
-   **Trigger:** The backtrace originated from `swapper/6`, the kernel's idle task. This means the error occurred in the `gamer_select_cpu` BPF hook. An idle CPU (CPU 4) was proactively trying to "steal" work from a busy CPU (CPU 6).
-   **Root Cause:** The `RHISubmissionTh` task was permanently pinned to CPU 6 (affinity mask `0x40`). Our "idle steal" logic did not check the task's CPU affinity mask before attempting to pull it.

### Incident 3: `vkd3d-swapchain` Crash (Reoccurrence - Enqueue Path)

-   **Task:** `vkd3d-swapchain[260496]` (same task as Incident 1)
-   **Operation:** The scheduler attempted to move the task from CPU 6 to CPU 4.
-   **Trigger:** The error was triggered by `wineserver[260188]` waking up. This indicates the crash occurred in the `gamer_enqueue` BPF hook, when one of our front-running optimizations attempted to force-dispatch the GPU thread.
-   **Root Cause:** After the initial fix (Incident 1), we correctly added `bpf_cpumask_test_cpu` checks to verify **permanent** CPU affinity. However, `vkd3d-swapchain` had **temporarily** disabled migrations (via `migrate_disable()`) during a GPU driver critical section. The permanent affinity mask still allowed CPU 4, but the temporary migration disable forbade the move. Our code did not check for this temporary state.

**Key Lesson:** Permanent affinity checks (`bpf_cpumask_test_cpu`) are necessary but not sufficient. We must also check for temporary migration disable state (`is_migration_disabled()`).

---

## 3. The Fix: Two-Layer Protection

The root cause evolved as we discovered two distinct types of migration restrictions. The final solution implements **two-layer protection** for all aggressive dispatch paths.

### Layer 1: Permanent Affinity Check (Initial Fix)

After Incident 1 and 2, we added permanent CPU affinity checks using `bpf_cpumask_test_cpu`.

-   **Function:** `bpf_cpumask_test_cpu(cpu, p->cpus_ptr)`
-   **Logic:** Checks if the target `cpu` is present in the task's allowed CPU mask (`p->cpus_ptr`).
-   **Performance:** Tier 0, sub-nanosecond operation.

### Layer 2: Temporary Migration Disable Check (Final Fix)

After Incident 3, we discovered that permanent affinity checks were insufficient. We added a second check for temporary migration disable state.

-   **Function:** `is_migration_disabled(p)`
-   **Logic:** Checks if the task currently has migrations disabled (via `migrate_disable()`). This is a temporary state, often set during driver critical sections.
-   **Strategy:** If migration is disabled and we've selected a different CPU, we **override the move** and dispatch the task to its **current CPU** instead. This still provides the latency benefit of priority dispatch, just without the migration.

### Final Implementation (`src/bpf/main.bpf.c`)

All three front-running paths in `gamer_enqueue` now use this **two-layer check**:

```c
if (game_cpu >= 0) {
    /* SAFETY CHECK: Do not move tasks that are temporarily migration-disabled */
    if (is_migration_disabled(p) && game_cpu != prev_cpu) {
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | prev_cpu, task_slice(p), enq_flags);
        wakeup_cpu(prev_cpu);
    } else if (bpf_cpumask_test_cpu(game_cpu, p->cpus_ptr)) {
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | game_cpu, task_slice(p), enq_flags);
        wakeup_cpu(game_cpu);
    }
    /* ... rest of logic ... */
}
```

**Why This Works:**
-   **Layer 1 (`bpf_cpumask_test_cpu`):** Catches permanent affinity violations (e.g., `RHISubmissionTh` pinned to CPU 6).
-   **Layer 2 (`is_migration_disabled`):** Catches temporary migration disable (e.g., `vkd3d-swapchain` in a GPU driver critical section).
-   **Fallback Strategy:** When migration is disabled, we still force-dispatch to the current CPU, preserving latency benefits without violating kernel constraints.

The `gamer_select_cpu` idle steal path only needs Layer 1 (permanent affinity check), as idle steal operations don't trigger the same temporary migration disable scenarios.

---

## 4. Further Investigation: How to Find What Refused to Move

To investigate why a specific process or thread is pinned, you can use standard Linux command-line tools.

### `taskset` Command

The `taskset` utility is the primary tool for viewing and setting CPU affinity.

-   **Find the Process ID (PID):**
    ```bash
    ps aux | grep vkd3d-swapchain
    ```
-   **Check the Affinity:** Use the `-p` flag with the PID.
    ```bash
    # Example for PID 260496
    taskset -p 260496
    ```
    -   **Output for a pinned task:** `pid 260496's current affinity mask: 40` (This is hex for `0100 0000`, meaning only CPU 6).
    -   **Output for a normal task:** `pid X's current affinity mask: ffff` (This means all CPUs 0-15 are allowed).

### `/proc` Filesystem

You can get more detailed information directly from the `/proc` filesystem.

-   **View Status:**
    ```bash
    cat /proc/<PID>/status
    ```
-   **Look for these lines:**
    -   `Cpus_allowed`: This shows the affinity mask in hexadecimal format.
    -   `Cpus_allowed_list`: This shows the allowed CPUs in a human-readable list (e.g., `6` or `0-15`).

By using these tools on tasks like `RHISubmissionTh` or `vkd3d-swapchain` while a game is running, you can confirm their pinning and better understand the constraints your scheduler is operating under.
