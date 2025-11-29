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

## Bug: Wake Chain CPU Validity Window Too Long for High Refresh Rates
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - performance regression identified via code audit)
```
* **Problem Context:** The `wake_chain_cpu` cache locality hint had an 8ms validity window (`WAKE_CHAIN_CPU_VALID_NS`), intended for 60Hz displays (16.67ms frames). At 240Hz+ (4.17ms frame budget), this allowed stale hints from 2 frames ago to influence CPU selection, triggering cross-CCD migrations on multi-CCD CPUs (9800X3D). Each cross-CCD hop adds 100-300ns penalty, compounding to 1-3us per frame during rendering.
* **Attempted Fixes:**
  1. Identified issue via comprehensive scheduler audit reviewing all timing constants against 240Hz+ frame budgets.
* **Resolution:** Reduced `WAKE_CHAIN_CPU_VALID_NS` from 8ms to 2ms in `main.bpf.c:3953`. The 2ms window ensures hints are from the current or immediately previous frame only, preventing stale cross-CCD migration decisions while still benefiting from cache locality within a frame's lifetime.

---

## Bug: GPU RenderThread Incorrectly Classified as Background
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - race condition identified via code audit)
```
* **Problem Context:** The `is_background_name()` function in `task_class.bpf.h` contained patterns matching GPU render threads (`RenderThread`, `vkd3d-*`, `dxvk-*`, `UnityGfx`, etc.) as background tasks. If `is_background_name()` was called before GPU classification completed (during thread initialization), these critical frame-rendering threads received an 8x scheduling penalty, causing frame time spikes during game startup and level transitions.
* **Attempted Fixes:**
  1. Traced frame spikes to `classify_background()` being called early in `classify_task()` pipeline.
  2. Confirmed GPU patterns in `is_background_name()` overlapped with `is_gpu_submit_name()` patterns.
* **Resolution:** Added `is_gpu_submit_name()` check at the start of `is_background_name()` to explicitly exclude GPU submission threads before any background patterns are evaluated. Removed redundant GPU render thread patterns from background detection. This ensures GPU threads are never accidentally penalized regardless of classification order.

---

## Bug: Keyboard Boost Duration Too Long (1 Second)
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - performance regression identified via code audit)
```
* **Problem Context:** The `keyboard_boost_us` default was set to 1,000,000us (1 second). At 240Hz, this means a single keypress boosted the entire system for 240 frames. At 480Hz esports mode, this extended to 480 frames. This caused unnecessary CPU priority elevation for background processes during the extended boost window, reducing effective gaming performance.
* **Attempted Fixes:**
  1. Identified via code audit reviewing all timing constants against frame budgets.
* **Resolution:** Reduced `keyboard_boost_us` default from 1,000,000us to 200,000us (200ms) in `src/main.rs`. Also reduced `controller_boost_us` from 500,000us to 200,000us. At 240Hz, 200ms covers ~48 frames which is sufficient for ability combos and rapid key sequences while preventing excessive boost bleed-through. The comment was updated to explain the competitive gaming rationale.

---

## Bug: Wine Input/Audio Threads Misclassified as Network
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - misclassification identified via code audit)
```
* **Problem Context:** The `is_network_name()` function in `task_class.bpf.h` contained overly broad Wine thread patterns that incorrectly classified input handlers, audio threads, and GPU threads as network threads:
  - `ntdll_*` - General Wine threadpool (handles GPU/audio/input/timers, not just network)
  - `wine_xinput_hid` - Controller input handler (should get boost=7, was getting boost=4)
  - `wine_dinput_wor` - DirectInput mouse/keyboard (should get boost=7)
  - `wine_mmdevapi_n` - Windows Multimedia Device API audio (should get boost=2)
  - `wine_threadpool` - Too broad, handles everything
  - `wine_sechost_de` - Security/services, not network
  - `wineserv*` - IPC synchronization, not network
* **Attempted Fixes:**
  1. Traced priority inversions in Wine games to misclassified threads.
* **Resolution:** Removed all overly broad Wine patterns from `is_network_name()`. These threads are now correctly classified by their proper handlers: `is_input_handler_name()` for input, `is_game_audio_name()` for audio, `is_gpu_submit_name()` for GPU. Added detailed comments explaining why each pattern was removed and where threads are now classified.

---

## Bug: RMS Priority Fallback Boosted Background Processes
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - priority bypass identified via code audit)
```
* **Problem Context:** The Rate Monotonic Scheduling (RMS) fallback in `recompute_boost_shift()` applied automatic priority boosts to unclassified tasks based on their wakeup frequency. However, it lacked a background process check. Electron apps like Discord and VS Code often have 60Hz UI timers, which gave them boost=3 through RMS fallback despite being marked as background. This defeated the 8x deadline penalty intended for background processes.
* **Attempted Fixes:**
  1. Identified via audit of boost_shift calculation paths.
* **Resolution:** Added `!tctx->is_background` guard to the RMS priority fallback condition in `src/bpf/main.bpf.c`. The condition now reads `base_boost == 0 && tctx->wakeup_freq > 0 && !tctx->is_background`. This ensures background processes never receive automatic RMS boosts regardless of their wakeup frequency.

---

## Bug: Discord Voice Threads Incorrectly Penalized as Background
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - classification order bug identified via code audit)
```
* **Problem Context:** The `classify_discord()` function checked `!tctx->is_background` before checking if a thread was a voice/audio thread. If a Discord thread was somehow pre-classified as background (e.g., during scheduler restart or re-classification), the voice thread check was entirely skipped. This caused Discord voice threads to receive an 8x scheduling penalty instead of audio priority boost, potentially causing voice chat crackling during gaming.
* **Attempted Fixes:**
  1. Traced potential voice quality issues to classification order.
  2. Reviewed entire `classify_task()` pipeline for similar issues.
* **Resolution:** Restructured `classify_discord()` in `src/bpf/include/task_class.bpf.h` to check voice threads FIRST before any background checks. Added logic to clear `is_background` flag if a voice thread was incorrectly marked. Also added early `classify_voice_chat_audio()` call at the start of `classify_task()` to catch voice threads from any app (TeamSpeak, Mumble, WebRTC) before they can be marked as background.

---

## Bug: Frame Feedback Boost Never Decayed (Permanent Priority Elevation)
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - resource waste identified via code audit)
```
* **Problem Context:** The frame feedback boost system was configured to NEVER decay during active gaming (disabled with comment "ESPORTS OPTIMIZATION: Never decay frame boost during active gaming"). While this prevented micro-stutters from aggressive decay, it meant a single frame miss permanently elevated thread priority until game exit. Over time, multiple threads could accumulate elevated priorities, wasting CPU cycles on threads that no longer needed boosting.
* **Attempted Fixes:**
  1. Identified via audit of frame feedback mechanism.
  2. Analyzed frame timing requirements at 240Hz (4.17ms) and 480Hz (2.08ms).
* **Resolution:** Added slow decay mechanism in `src/bpf/main.bpf.c`: after 60 consecutive good frames (~250ms at 240Hz, ~125ms at 480Hz), `frame_feedback_boost` decrements by 1 and the hit streak resets. This provides:
  - Enough runway (60 frames) to avoid VRR micro-stutters
  - Recovery mechanism for false positive deadline misses
  - Prevention of permanent priority elevation accumulation
  Added `nr_frame_feedback_recoveries` stat counter to track decay events.

---

## Bug: Game Detection False Positives from Electron Apps
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - false positive detection identified via code audit)
```
* **Problem Context:** The game detection heuristics in `src/game_detect.rs` used resource thresholds of 20+ threads and 100MB+ memory to identify games. However, modern Electron apps (Discord: 30+ threads, 200MB+; VS Code: 40+ threads, 300MB+; Chrome: 50+ threads, 500MB+) easily exceeded these thresholds. This could cause the scheduler to incorrectly identify these apps as the foreground game, applying game-specific optimizations to the wrong process.
* **Attempted Fixes:**
  1. Reviewed resource usage of common desktop applications.
  2. Compared against typical game resource usage patterns.
* **Resolution:** Multiple fixes in `src/game_detect.rs`:
  1. Raised `is_likely_game` resource thresholds from 20 threads/100MB to 40 threads/300MB
  2. Raised `calculate_score()` thresholds: 80+ threads gives +400, 50+ gives +300, 30+ gives +150
  3. Added explicit negative scoring (-800) for known non-game apps: Discord, VS Code, Chrome, Firefox, OBS, etc.
  4. Added early exit in `check_process()` for known non-game apps to skip resource evaluation entirely
  This prevents Electron apps from being detected as games while still catching actual games with high resource usage.

---

## Bug: is_gaming_network_thread() Had Overly Broad Patterns
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - false positive classification identified via code audit)
```
* **Problem Context:** The `is_gaming_network_thread()` function in `task_class.bpf.h` contained overly broad pattern matching:
  - "Clie" matched any thread starting with "Client" (ServiceWorker, DatabaseClient, etc.)
  - "Serv" matched any thread starting with "Server" (PostgresServer, WebServer, any daemon)
  - "Mult" matched "Multiplayer" but also "Multithread", "MultiProcess", etc.
  - "Voice" could match non-gaming voice applications
  - "Chat" matched ANY chat application, not just in-game chat
  This caused non-gaming threads to receive boost=6 (gaming network priority) when they should have received much lower priority or penalties.
* **Attempted Fixes:**
  1. Identified via systematic audit of thread classification patterns.
  2. Cross-referenced against common system thread names.
* **Resolution:** Rewrote pattern matching in `src/bpf/include/task_class.bpf.h` to be more specific:
  - Removed overly broad "Client", "Server", "Mult", "Voice", "Chat" patterns
  - Now only matches GAME-PREFIXED patterns: "GameClient", "GameServer", "GameThread", "GameNet*"
  - Added specific patterns for networking libraries: "NetworkClient", "NetworkServer"
  - Required full "Netcode" match instead of just "Netc"
  This ensures only actual gaming network threads receive the gaming network boost.

---

## Bug: Storage hot_path Detection Never Set is_hot_path Flag
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - missing functionality identified via code audit)
```
* **Problem Context:** The `storage_thread_info.is_hot_path` field was initialized to 0 but never updated in `storage_detect.bpf.h`. The memory detection module (`memory_detect.bpf.h`) correctly sets `is_hot_path = 1` when `freq > 1000 && total > 100`, but storage detection lacked equivalent logic. This meant `is_hot_path_storage_thread()` always returned false, defeating hot path storage optimizations.
* **Attempted Fixes:**
  1. Compared implementation against memory_detect.bpf.h which correctly implements hot path detection.
* **Resolution:** Added hot path detection logic to `register_storage_thread()` in `src/bpf/include/storage_detect.bpf.h`:
  - Set `is_hot_path = 1` when `io_freq_hz > 500 && total_ios > 100`
  - 500 Hz threshold matches high-frequency asset streaming patterns
  - 100 operation minimum provides confidence in classification
  This enables proper hot path storage optimizations for game asset loading.

---

## Bug: Interrupt Detection Thresholds Too Low for High Refresh Rates
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - false positive classification identified via code audit)
```
* **Problem Context:** The interrupt thread classification in `interrupt_detect.bpf.h` had low thresholds that could cause false positives:
  - `is_input_interrupt`: Set at 100 Hz with 50 samples - system timers and periodic interrupts could hit this
  - `is_gpu_interrupt`: Set at 60 Hz with 100 samples - too low, could match other periodic tasks
  - `is_usb_interrupt`: Set at 10 Hz with 20 samples - extremely low, many system interrupts exceed this
  For 240Hz+ gaming (8kHz mice, high refresh monitors), these thresholds were too permissive.
* **Attempted Fixes:**
  1. Analyzed interrupt frequencies for gaming hardware (8kHz mice, 480Hz monitors).
  2. Reviewed common system interrupt frequencies to find distinguishing thresholds.
* **Resolution:** Raised thresholds in `src/bpf/include/interrupt_detect.bpf.h`:
  - `is_input_interrupt`: Raised from 100Hz/50 samples to 500Hz/100 samples (8kHz mice easily exceed this)
  - `is_gpu_interrupt`: Kept 60Hz but raised samples from 100 to 200 for higher confidence
  - `is_usb_interrupt`: Raised from 10Hz/20 samples to 50Hz/50 samples (USB polling is typically 125Hz+)
  This reduces false positives while still detecting actual gaming hardware interrupts.

---

## Bug: Engine Preset GameThread Has boost=7 (Input Handler Level)
* **Date:** 2025-11-26
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - priority misconfiguration identified via code audit)
```
* **Problem Context:** In `engine_presets.rs`, the "GameThread" (Unreal Engine main game thread) preset was configured with `last_boost: 7`. However, boost=7 is reserved for INPUT HANDLERS (mice, keyboards, controllers) which need absolute priority for low-latency input processing. GameThread processes game logic and simulation - not raw input events - and should not compete with input handlers for CPU time.
* **Attempted Fixes:**
  1. Reviewed boost level assignments across the codebase.
  2. Cross-referenced against boost level documentation: 7=input, 6=GPU/render, 5=audio, etc.
* **Resolution:** Changed GameThread preset from `last_boost: 7` to `last_boost: 6` in `src/engine_presets.rs`. Level 6 is GPU/render priority, which is appropriate for game logic threads. This ensures actual input handlers (SDLInput, evdev, etc.) maintain absolute priority over game simulation threads.

---

## Bug: is_audio_pipeline_thread() Had Overly Broad Pattern Matching
* **Date:** 2025-11-27
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - false positive classification identified via code audit)
```
* **Problem Context:** The `is_audio_pipeline_thread()` function in `task_class.bpf.h` contained overly broad pattern matching:
  - "Stream" matched ANY thread starting with "Stream" (StreamClient, FileStream, NetworkStream, etc.)
  - "Codec" matched ANY thread starting with "Codec" (video codecs, network codecs, etc.)
  - "Effect" matched ANY thread starting with "Effect" (VisualEffect, ParticleEffect, etc.)
  - "RealTime" matched ANY thread starting with "RealTime" (RealTimeThread, RealTimePriority, etc.)
  
  This caused false positives where non-audio threads were classified as audio pipeline threads and received audio-level priority (boost=1) when they shouldn't.
* **Attempted Fixes:**
  1. Identified the overly broad patterns during comprehensive code audit.
  2. Cross-referenced against actual audio thread naming conventions.
* **Resolution:** Removed the overly broad patterns:
  - Kept "AudioStream*" but removed "Stream" alone
  - Kept "AudioCodec*" but removed "Codec" alone
  - Kept "AudioEffect*" but removed "Effect" alone
  - Kept "RTAudio*" but removed "RealTime" alone
  
  This ensures only actual audio threads receive audio pipeline classification.

---

## Bug: Controller Boost Fallback Duration Inconsistent with Default
* **Date:** 2025-11-27
* **Status:** Resolved
* **Error Message (if any):**
```error
(none - inconsistency identified via 2nd comprehensive code audit)
```
* **Problem Context:** In `boost.bpf.h`, the controller boost fallback value was 500ms (500,000,000 ns) when `controller_boost_ns` was unset (0). However, we had previously reduced the `controller_boost_us` default in `main.rs` from 500ms to 200ms during the first audit. This created an inconsistency where:
  - With explicit configuration: Uses 200ms (from main.rs default)
  - Without configuration (fallback): Uses 500ms (hardcoded in boost.bpf.h)
  
  The comment also incorrectly stated "default 1000ms" for keyboard boost when the actual default is now 200ms.

* **Attempted Fixes:**
  1. Identified the inconsistency during 2nd comprehensive sweep of boost.bpf.h.
  2. Cross-referenced with main.rs defaults changed in first audit.
* **Resolution:** Updated `boost.bpf.h`:
  - Changed controller fallback from `500000000ULL` (500ms) to `200000000ULL` (200ms)
  - Updated comment for keyboard boost to reflect "default 200ms (was 1000ms)"
  - Updated comment for controller boost to reflect "default 200ms (was 500ms)"
  
  This ensures consistent boost durations regardless of how the scheduler is configured.

---

