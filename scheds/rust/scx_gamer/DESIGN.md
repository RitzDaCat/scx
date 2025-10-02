scx_gamer design overview

Goals
- Minimize input-to-photon latency and stabilize frametimes under load.
- Prefer locality when light, switch to EDF-like policy when busy.
- Be safe and observable: watchdog, clean detach, rich metrics.

High-level architecture
- Userspace (Rust)
  - CLI, topology discovery, BPF skeleton management.
  - Event loop via epoll integrating: evdev (input), timerfd (fixed Hz), inotify (DRM vblank).
  - Auto behaviors: vblank source auto-detect, event-loop CPU auto pinning.
  - Monitor/metrics printing and watchdog.
- BPF (C)
  - Per-CPU DSQs (RR); shared DSQ for EDF-like under contention.
  - NUMA, SMT, migration limiter, mm-affinity hints.
  - Windows (input, frame, NAPI) now tracked with global-until timestamps; read-gated by primary cpumask.
  - CPU util sampling and EMA maintained in-kernel via wakeup timer.
  - Futex/chain boost with fast decay and caps.
  - Preferred CPU ranking falls back to CPU ID order when capacity values are uniform.

Key runtime states
- Light (not busy):
  - Prefer local RR dispatch; wake-affine allowed; NUMA/locality emphasized.
- Busy:
  - Enqueues can fall back to shared DSQ using EDF-like deadlines; fairness maintained via vruntime and lag caps.
- Windows:
  - Input window: shorter slices, relaxed limiter, optional NAPI bias; foreground-gated.
  - Frame window: block most migrations for FG tasks to stabilize mid-frame; short duration (fraction of frame).

Busy detection
- cpu_util (instant) and cpu_util_avg (EMA) computed in BPF each wakeup timer tick.
- Thresholds (enter/exit) set from CLI, with adaptive nudging from system interactivity EMA.
- We cache the computed busy decision per op and plumb it where needed to avoid repeated checks.

Scheduling logic (hot paths)
- select_cpu():
  - If sync-wake and FG within input window, keep local (co-location) and boost chain.
  - Otherwise prefer scx_bpf_select_cpu_and() for simplicity; SMT allowance depends on activity and busy.
- enqueue():
  - If migration desirable and idle target exists, direct-dispatch to that CPU.
  - If not busy or inside FG window on prev CPU, keep local RR; else enqueue to shared DSQ (EDF-like).
- dispatch():
  - Pull from shared DSQ if available; otherwise extend slice if sibling not contended.

Data structures
- task_ctx (per-task): last_run_at, exec_runtime, wakeup_freq (EMA), migration limiter window/counter, chain_boost.
- cpu_ctx (per-CPU): perf level (for cpufreq), vtime_now, interactive_avg, cached last scan index.
- Global window timestamps: input_until_global, frame_until_global, napi_until_global (avoid O(NCPU) writes).
- mm_last_cpu: LRU hash for recent per-mm CPU affinity (bounded capacity).

Windows and gating
- Foreground task gating: input/frame/NAPI effects apply only to the configured TGID when set; global fallback when not.
- Primary cpumask gating: window reads are masked to the configured primary CPUs when defined.

Vblank integration
- Preferred: inotify on DRM debugfs vblank_event (auto-detected card:crtc). Event-driven; no busy polling.
- Fallback: fixed --frame-hz (or auto-read mode) when debugfs is unavailable.

Safety and robustness
- Watchdog exits on stalled dispatch progress to restore CFS.
- Ctrl-C clean detach and UEI reporting.
- Boundaries and caps: chain boost max depth, migration limiter, numeric clamps.

Metrics
- RR/EDF enqueues, dispatch mix, migrations, blocks (limiter and frame), sync-local, cpu_util/EMA, FPS~ estimate.
- Monitor prints include FG PID when set.

Tuning guidance (quick)
- 240 Hz VRR: frame_window_us ≈ 2000–2500; input_window_us ≈ 1500–2000; wakeup_timer_us ≥ 250.
- 480 Hz VRR: frame_window_us ≈ 1000–1300; input_window_us ≈ 1000–1500.

Future work
- Backend trait to decouple userspace from BPF for easier testing.
- Compositor/portal vblank sources on Wayland as alternative to debugfs.
- Further hot-path reductions: struct layout alignment, task_dl fixed-point pre-scaling where safe.


