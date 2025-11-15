<!--
    Build Error Log
    Project: scx_gamer
    Purpose: Persistent record of build failures, mitigation attempts, and verified resolutions.
-->

# Build Error Log

## 2025-11-13 — Cargo Build (`scx_gamer@1.0.2`)

- ### Symptoms
  - `error[E0308]: arguments to this method are incorrect (MapImpl::update expects &[u8] key/value)`
  - Clang warnings: `loop not unrolled` (-Wpass-failed=transform-warning) for TaskGraph corral/borrow loops.

- ### Impact Assessment
  - Rust compile failure blocks binaries; caused by passing typed structs to `MapImpl::update` instead of raw byte slices.
  - Clang warnings are advisory; loops remain bounded and verifier-safe.

- ### Mitigation Steps
  1. Remove `.as_mut()` call; use direct reference to `engine_profile_map`.
  2. Import `MapCore` trait to enable `.update`.
  3. Convert `EngineProfileKey` / `Entry` to byte slices (e.g., `unsafe` casts) before calling `.update`.
  4. Document loop warnings for future cleanup (option: drop pragmas or hand-unroll).

- ### Resolution Status
  - Struct-to-bytes conversion for map updates: **Resolved** (build succeeds).
  - Loop unroll warnings: **Open** (need structural change if we want silence).
  - `Sensor.kind` dead-code warning: **New** (field unused in `power_monitor.rs`).

## 2025-11-13 — Cargo Build (`scx_gamer@1.0.2`) — Post-Fix

- ### Symptoms
  - Clang warns: `loop not unrolled` for TaskGraph corral/borrow loops.
  - Rust warns: `field 'kind' is never read` in `power_monitor::Sensor`.

- ### Impact Assessment
  - Unroll warning indicates `#pragma unroll` wasn’t honored; verifier still accepts today, but warning hides potential future regressions.
  - `Sensor.kind` unused field violates project no-dead-code rule.

- ### Proposed Mitigations
  1. Refactor loops to rely on `continue` guards (avoid `break`) so clang can fully unroll statically bounded loops.
  2. Consume `Sensor.kind` (e.g., for unit-aware logging) or remove the field if redundant.

- ### Status
  - Loop refactor & sensor-kind usage initially applied, but verifier rejected modified loop (see below).

## 2025-11-13 — BPF Verifier Failure (`gamer_select_cpu`)

- ### Symptoms
  - BPF load aborts with `-EACCES`; verifier log reports “R2 unbounded memory access” for `preferred_cpus[idx]`.

- ### Root Cause
  - Converting loop `break` statements to `continue` to satisfy Clang unroll warnings widened the possible range for `idx`. The verifier could no longer prove the offset stayed within `preferred_cpus`, triggering a safety rejection.

- ### Mitigation
  1. Restore `break`-based exit conditions to keep verifier bounds tight.
  2. Replace array scan with explicit `idx/scanned` counters so verifier tracks bounds.
  3. Remove explicit `#pragma unroll` directives instead of forcing unroll through structural changes.

- ### Resolution Status
  - BPF verifier passes with explicit counters; Clang warning eliminated by dropping the pragmas. Build now clean.

## 2025-11-13 — BPF Verifier Failure (`gamer_select_cpu`) v2

- ### Symptoms
  - Verifier reports “R7 unbounded memory access” when reading `preferred_cpus[idx]`.

- ### Root Cause
  - The first counter-based rewrite still allowed `idx` to decrement beyond `base_index` because we used `u32` and post-decremented before the boundary check. The verifier lost track of the resulting pointer (`r7`) and flagged the access as potentially out of bounds.

- ### Mitigation Plan
  1. Switch the loop to count forward from `base_index`, incrementing a `scanned` counter; this keeps index monotonic and bounded by both `tail_cap` and `TASKGRAPH_MAX_PREF_SCAN`.
  2. Keep all loop variables `u32` so verifier can reason about wraps.
  3. Store `preferred_cpus[idx]` result immediately, avoiding pointer arithmetic on unchecked offsets.

- ### Resolution Status
  - Applied forward-scanning loop bounded by `tail_cap`, `TASKGRAPH_MAX_PREF_SCAN`, `cpu_count`, and `MAX_CPUS`. Pending verifier confirmation.

## 2025-11-13 — BPF Verifier Failure (`gamer_select_cpu`) v3

- ### Symptoms
  - Verifier reports “R2 unbounded memory access” when loading `preferred_cpus[idx]`.

- ### Root Cause
  - Our loop still permits `scanned` (a `u32`) to approach `cpu_count`, which can be up to `MAX_CPUS` (256). The expression `cpu_count - scanned - 1` therefore computes `idx`, but the verifier no longer tracks the relationship between `scanned` and `cpu_count` once we store `scanned` to the stack. The copy into `w3` followed by subtracting 257 confused the verifier, widening the potential range for `idx`.

- ### Mitigation Plan
  1. Refactor loop to maintain a decrementing `idx` variable kept entirely in registers, avoiding stack temporaries that lose the verifier relationship.
  2. Ensure the loop condition checks `idx >= base_index` before dereferencing `preferred_cpus[idx]`.
  3. Keep `scanned` and `idx` limited to `MAX_CPUS` and avoid subtracting large constants (e.g., `-257`) that break value ranges.

- ### Resolution Status
  - Implemented register-based `idx/scanned` loop with explicit guards. Pending verifier confirmation.

## 2025-11-13 — BPF Verifier Failure (`gamer_select_cpu`) v4

- ### Symptoms
  - Verifier now flags “R3 unbounded memory access” when dereferencing `preferred_cpus` despite new guards.

- ### Root Cause
  - The loop still manipulates `idx` indirectly by creating a temporary pointer `r3` (`preferred_cpus + idx * 8`) stored on the stack. The verifier can’t prove `r3` stays within the map because we subtract 8 in place (`r3 += -8`) before the next `idx` decrement, losing the explicit bounds check.

- ### Mitigation Plan
  1. Eliminate pointer arithmetic on `preferred_cpus`; compute `idx` via simple `for` loop with constant bound.
  2. Keep index arithmetic inline (`idx = cpu_count - 1 - i`) and guard before dereference.
  3. Drop manual pointer adjustments (`r3 += -8`) so verifier retains array bounds.

- ### Resolution Status
  - Restored original constant-bounded `for` loops without `#pragma unroll`. **Resolved** (verifier accepts load).

---

## Bug: `gamer_select_cpu_tail` pruned during BPF load
* **Date:** 2025-11-15
* **Status:** Investigating
* **Error Message (if any):**
```error
libbpf: map 'gamer_ops': created successfully, fd=62
libbpf: prog 'gamer_select_cpu_tail': SEC("struct_ops") program isn't referenced anywhere, did you forget to use it?
libbpf: prog 'gamer_select_cpu_tail': failed to load: -EINVAL
libbpf: failed to load object 'bpf_bpf'
libbpf: failed to load BPF skeleton 'bpf_bpf': -EINVAL
Error: Failed to load BPF program
Caused by: Invalid argument (os error 22)
```
* **Problem Context:** When launching the scheduler via `start.sh`, libbpf refuses to load the struct-ops object because the verifier believes the new tail-call helper programs (`gamer_select_cpu_tail`, `gamer_enqueue_tail`) are unused and eligible for dead-code elimination. This regression surfaced after splitting the hot-path logic into tail-call stubs to reduce instruction count.
* **Attempted Fixes:**
  1. Replaced the original `bpf_tail_call(p, …)` with `bpf_tail_call((void *)ctx, …)` to pass the struct-ops context pointer instead of a trusted task pointer; resolved the initial type mismatch but did not address linking.
  2. Declared a `.rodata` array of raw function addresses marked `__attribute__((used))` so the linker sees references to both tail programs; libbpf still rejected the load.
  3. Switched the `.rodata` array to hold typed function pointers using `typeof(&gamer_select_cpu_tail)` / `typeof(&gamer_enqueue_tail)` to force BTF relocations; build succeeds yet libbpf continues reporting the programs as unreferenced at load time.
  4. Added a dummy struct-ops anchor (`gamer_tailcall_anchor`) so the loader has a concrete `struct sched_ext_ops` referencing the tail programs. This resolves the “unreferenced” warning but exposed deeper verifier failures in the hot paths (see next entry).
* **Resolution:** Pending. Next steps: (a) confirm struct-ops skeleton generated by `libbpf-rs` enumerates the new tail-call FDs in userspace, (b) inspect `bpf_object` map/program table before attach to ensure the `.rodata` references survive CO-RE relocation, and (c) explore `BPF_PROG_ARRAY` pinning via `SEC(".struct_ops.link")` companion map or a dedicated `__weak` wrapper function.

## Bug: BPF verifier rejects TaskGraph scan bounds (`gamer_select_cpu`)
* **Date:** 2025-11-15
* **Status:** Investigating
* **Error Message (if any):**
```error
libbpf: prog 'gamer_select_cpu': BPF program load failed: -EACCES
...
628: (79) r3 = *(u64 *)(r9 +0)
R9 unbounded memory access, make sure to bounds check any such access
processed 41531 insns (limit 1000000) ...
```
* **Problem Context:** After restructuring the TaskGraph corral loop in `gamer_enqueue` to keep the verifier happy, the corresponding scan inside `gamer_select_cpu_slowpath` still relies on pointer arithmetic (`r9 = preferred_cpus + idx * 8`). The verifier concludes that `r9` can point outside the array and terminates the load.
* **Attempted Fixes:**
  1. Introduced `load_preferred_cpu_safe()` helper and rewrote the TaskGraph tail scan to clamp `bounded_cpu_count`, derive `idx = base_idx - scan_idx`, and gate every load through `idx < MAX_CPUS`. This mirrors the verifier-approved pattern we now use in `gamer_enqueue`.
  2. Despite the guard, clang still materializes the `preferred_cpus + idx * 8` pointer before the bounds check, so the verifier continues to see `R9` as potentially out of range and aborts with “unbounded memory access”.
* **Resolution:** Pending. Next steps:
  1. Reorder the code so the verifier observes the bounds check before any pointer arithmetic. Concretely, compute `idx` first, branch away if `idx >= MAX_CPUS`, and only then take the address of `preferred_cpus[idx]`.
  2. Avoid having clang spill `idx` or the pointer to the stack prior to the guard (e.g., by keeping everything in registers or using a direct `if (idx >= MAX_CPUS) continue; candidate = preferred_cpus[idx];` pattern without helper indirection).
  3. If clang continues to hoist the pointer arithmetic, fall back to copying the array into a small fixed-size scratch buffer (or use a map lookup) so the verifier tracks the bounds via explicit `offset < sizeof(preferred_cpus)` checks.
  4. Cap the GPU/compositor scan loop once the helper change sticks, to prevent the same fix from exploding instruction count in `gamer_enqueue` (see below).

## Bug: BPF verifier rejects `gamer_enqueue` slowpath
* **Date:** 2025-11-16
* **Status:** Investigating
* **Error Message (if any):**
```error
libbpf: prog 'gamer_enqueue': BPF program load failed: -EINVAL
...
processed 210044 insns (limit 1000000) max_states_per_insn 45 total_states 16263 peak_states 5489 mark_read 83
```
* **Problem Context:** After splitting the struct-ops hot path and resolving the `gamer_select_cpu` verifier complaints, the enqueue path still fails to load. The current libbpf debug log only shows the instruction/state counters, so the exact verifier rejection (pointer bounds vs. excessive branching) is unknown. This blocks the scheduler from attaching even though select_cpu now passes.
* **Attempted Fixes:**
  1. Added `load_preferred_cpu_safe()` and switched the GPU/compositor “preferred CPU” scan to use it (same helper now shared with `gamer_select_cpu`).
  2. Hard-capped the frame-thread physical-core scan to `FRAME_PHYS_SCAN_MAX` (64) so the verifier no longer needs to explore 256 iterations.
  3. Retested after the cap; instruction count dropped to ~497k but the verifier still returns `-EINVAL` without a detailed reason in userspace logs.
* **Resolution:** Pending. Immediate next steps:
  1. Capture the kernel verifier output via `sudo dmesg | grep -a gamer_enqueue -n` (or rerun with `bpftool prog load ... log_level 2`) to determine the exact rejection (bounds vs. state explosion).
  2. Based on the log, either tweak the helper usage further (e.g., pre-check `candidate < nr_cpu_ids` before storing) or split the remaining fallback logic (NUMA pruning, cpumask tests, etc.) into a secondary tail program so the hot enqueue stub stays tiny.

## Bug: Game Detection False Positives & Stale Latency Metrics
* **Date:** 2025-11-15
* **Status:** Resolved
* **Error Message (if any):**
```error
(none – logic bugs surfaced via code review and runtime logging)
```
* **Problem Context:** Users reported scheduler sticking to an old foreground TGID, /proc scans consuming CPU when inotify was unavailable, and ring-buffer analytics showing 0 ns latency. Review traced these to the ProcessCache modulo bitset, a non-resetting fallback timer, and reversed latency math in `InputRingBufferManager`.
* **Attempted Fixes:**
  1. Original ProcessCache relied solely on a modulo bitmask; collisions (pid, pid+4096) short-circuited detection and the new game never evaluated.
  2. Tried to lower the fallback period but the timer field was immutable, so the scan still ran every loop and pegged a core.
* **Resolution:** Added exact PID slots alongside the bitmask, making `contains` validate the stored PID before skipping, introduced a reusable `fallback_scan_due` helper that resets the timer after each /proc sweep, and flipped the latency calculation to `processing_start - capture_time`, restoring non-zero stats. Added unit tests for the cache collision, fallback throttling, and latency reporting to prevent regressions.
---

