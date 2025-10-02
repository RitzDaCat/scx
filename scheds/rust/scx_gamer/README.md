scx_gamer — sched_ext gaming scheduler

## ⚠️ Experimental / AI-Assisted Development

This scheduler is **experimental** and was developed with significant AI assistance as a test of AI capabilities in producing functional kernel scheduling code. The primary goals are to explore whether AI can help reduce frame latency and input latency in gaming workloads, and to evaluate AI-generated code quality in a complex systems programming context.

**Use at your own risk.** This is a research/testing project and may not be suitable for production use.

## Overview
scx_gamer is a Linux sched_ext (eBPF) scheduler that attempts to minimize input latency and frame-time variance for games. It favors task–CPU locality under light/medium load and switches to a global EDF-like policy under heavy load to sustain responsiveness. It provides input- and frame-aware boost windows, NUMA and SMT-aware placement, and a lightweight userspace control loop with monitoring.

## Goals
- Preserve cache locality and reduce needless migrations when not overloaded
- Deliver consistent frame pacing under load (reduce 1%/0.1% low spikes)
- Optimize input-to-photon latency with input and frame windows
- Stay safe: cleanly detach to CFS on exit or stall
- Test AI capabilities in systems programming and scheduler development

Architecture at a glance
- Userspace (Rust):
  - CLI, topology detection, BPF skeleton management.
  - Event-driven loop via epoll: evdev FDs (input), timerfd (frame Hz, CPU util), and inotify on DRM debugfs vblank file.
  - Optional input detection via evdev for input-active windows (no polling).
  - Optional frame cadence via fixed Hz (timerfd) or DRM vblank counter (inotify event-driven).
  - Stats server and in-process monitor.
  - Watchdog to auto-fallback to CFS when stalled.
- BPF (C):
  - Per-CPU dispatch queues (DSQs) with round-robin.
  - Global EDF fallback when system is busy; hysteresis to avoid thrash.
  - Migration limiter, mm-affinity LRU hints (bounded), NUMA-local preference.
  - SMT contention avoidance, with guarded allowance for light paired threads.
  - Input / frame / NAPI-softirq windows are per-CPU and short-lived.
  - Criticality EMAs (interactive_avg per-CPU, interactive_sys_avg) to adjust slice and policy.
  - Futex/chain-boost for wake chains.
  - Wakeup timer re-arms at configurable period to kick idle CPUs with pending work.

How it works (high-level)
1) Under light load, scx_gamer dispatches from per-CPU DSQs (round-robin). This maximizes locality and minimizes lock contention.
2) When the EMA of system CPU utilization exceeds a threshold, it transitions to an EDF-mode global queue to improve responsiveness and load balancing under contention. Hysteresis prevents rapid toggling.
3) Short windows elevate responsiveness:
   - Input window (evdev activity): shorter slices, relaxed migration limit, optional NAPI preference to help input processing.
   - Frame window (timer cadence or DRM vblank): block most migrations to stabilize frame pacing and modestly tighten slice. Driven by timerfd or inotify (no polling).
4) Placement and migration decisions consider NUMA boundaries, mm-affinity hints (per-mm recent CPU), SMT contention, and migration rate limiting.
5) Safety: SIGINT/SIGTERM and Drop detach struct_ops; watchdog exits if no dispatch progress for N seconds.

Intended benefits
- Local-first approach when uncongested may retain hot caches for game and render threads
- EDF under pressure attempts to prioritize time-critical tasks and stabilize latency
- Windows aim to align boosts to real input/render phases, gated to the foreground game when provided
- NUMA and SMT awareness attempts to avoid cross-node stalls and sibling contention

Results may vary depending on workload, hardware, and configuration.

Requirements
- Linux with sched_ext (scx) enabled (see repository root docs for kernel versions).
- Root privileges to attach sched_ext and read DRM debugfs (optional) or be in the video group.
- debugfs mounted at /sys/kernel/debug for DRM vblank counter (optional).

Build
```bash
cargo build -p scx_gamer --release
```

Run (direct, recommended for Ctrl+C)
- Foreground run lets Ctrl+C cleanly restore CFS.
```bash
sudo ./target/release/scx_gamer --stats 1 \
  --input-window-us 2000 \
  --frame-window-us 2000 \
  --wakeup-timer-us 500
```

Optional: drive frame windows from display events
- Fixed cadence: `--frame-hz 240` (or 60/120/144).
- DRM vblank (good with VRR): `--drm-debugfs card:crtc` (e.g., 0:0). If not provided, we auto-detect the most active vblank source under `/sys/kernel/debug/dri/*` and subscribe via inotify (no busy read loop). If debugfs isn’t available, omit and use `--frame-hz`.

Run via scx_loader (DBus service)
```bash
cargo build -p scx_loader --release
sudo ./tools/scx_loader/target/release/scx_loader --set gamer
# Use scxctl to observe status
cargo build -p scxctl --release
./tools/scxctl/target/release/scxctl status
```
Note: Running via the loader may place scx_gamer under a service; Ctrl+C stops your client, not the scheduler. Use scxctl or systemd to stop.

Clean shutdown
- Direct run: Ctrl+C is handled; scheduler detaches and CFS is restored.
- Watchdog: `--watchdog-secs N` auto-detaches if dispatch progress stalls.

CLI reference
Defaults shown in parentheses. Units in comments.
- `--exit-dump-len <u32>` (0): BPF exit dump length.
- `-s, --slice-us <u64>` (10): Max scheduling slice (µs).
- `-l, --slice-lag-us <u64>` (20000): Max vtime debt charged (µs).
- `-c, --cpu-busy-thresh <u64>` (75): Busy enter threshold (%).
- `--cpu-busy-exit <u64>`: Busy exit threshold (%). Default: busy_enter-10, floored at 0.
- `-p, --polling-ms <u64>` (0): Deprecated. CPU util is sampled in-kernel; this flag is a no-op.
- `-m, --primary-domain <list|keyword>`: Preferred CPU set: comma/range list or `turbo|performance|powersave|all`.
- `-n, --enable-numa`: Enable NUMA-aware policies.
- `-f, --disable-cpufreq`: Disable cpufreq integration.
- `-i, --flat-idle-scan`: Light-weight idle scan.
- `-P, --preferred-idle-scan`: Prefer higher-capacity CPUs first.
- `--disable-smt`: Disable SMT placement (only with an idle scan mode).
- `-S, --avoid-smt`: Avoid SMT contention where possible.
- `-w, --no-wake-sync`: Disable synchronous wake direct dispatch.
- `-d, --no-deferred-wakeup`: Disable deferred wakeups.
- `-a, --mm-affinity`: Keep same-mm threads on the same CPU across wakeups.
- `--mig-window-ms <u64>` (50): Migration limiter window (ms).
- `--mig-max <u32>` (3): Max migrations per task per window.
- `--input-window-us <u64>` (2000): Enable input-active boost window (µs). 0=off.
- `--frame-window-us <u64>` (0): Enable frame-active window (µs). 0=off.
- `--frame-hz <f64>`: Fixed cadence for frame window (Hz). Requires frame-window-us>0.
- `--drm-debugfs <card:crtc>`: Drive frame window from DRM vblank counter.
- `--foreground-pid <u32>` (0): Restrict input/frame effects to this TGID (game). 0=global.
- `--watchdog-secs <u64>` (0): Exit to CFS if no dispatch progress for N seconds. 0=off.
- `--prefer-napi-on-input`: Prefer NAPI/softirq CPUs briefly during input window.
- `--enable-mm-hint`: Enable per-mm recent CPU hinting for cache affinity.
- `--mm-hint-size <u32>` (4096): Size of the mm hint LRU (clamped 128-65536 entries).
- `--vblank-sample-ms <u64>` (200): DRM vblank auto-detect sample window (10-1000 ms).
- `--wakeup-timer-us <u64>` (0): Periodic wakeup timer (µs). 0=use slice_us. Clamped to ≥250µs in-kernel.
- `--event-loop-cpu <usize>`: Pin the event loop (epoll/timerfd/inotify) to a specific CPU. If omitted, scx_gamer auto-pins to a low-capacity housekeeping CPU.
- `--stats <sec>`: Run scheduler and print live metrics every N seconds.
- `--monitor <sec>`: Monitor-only mode; do not attach the scheduler.
- `-v, --verbose`: Verbose logging.
- `-V, --version`: Print version.
- `--help-stats`: Print metric descriptions and exit.

Recommended starting points
- High-Hz VRR FPS (e.g., 240 Hz):
```bash
sudo ./target/release/scx_gamer \
  --input-window-us 2000 \
  --frame-window-us 2000 \
  --drm-debugfs 0:0 \
  --event-loop-cpu 0 \
  --wakeup-timer-us 500 \
  --stats 1
```
- Fixed Hz displays (e.g., 144 Hz):
```bash
sudo ./target/release/scx_gamer \
  --input-window-us 2000 \
  --frame-window-us 2000 \
  --frame-hz 144 \
  --event-loop-cpu 0 \
  --wakeup-timer-us 500 \
  --stats 1
```

Monitoring & metrics
- `--stats <sec>` prints per-interval deltas for:
  - RR/EDF enqueue counts, dispatch mix (direct/shared), migrations, migration blocks.
  - Sync-local count, frame-window migration blocks.
  - CPU util instant and EMA; estimated FPS from frame events.
- `--monitor <sec>` runs a monitor without attaching the scheduler.

Design details
- Busy detection and hysteresis: cpu_util is sampled in-kernel (BPF) each wakeup timer tick; BPF maintains an EMA used to enter/exit EDF mode. Thresholds are configurable and scaled into 0–1024.
- Per-CPU windows: input/frame/NAPI windows are recorded in per-CPU context, and userspace triggers fan-out via a syscall; primary CPUs can be restricted to a subset.
- Migration control: migrations are limited per-task per window; during frame windows most migrations are blocked.
- NUMA & locality: prefer local node when system is busy; avoid spilling if local DSQ depth is below a threshold; mm_last_cpu is an LRU hash (bounded) to guide wake affinity.
- SMT: avoid siblings when contended; allow siblings for light paired threads when system is not busy and interactive activity is low.
- Criticality: interactive_avg per-CPU and interactive_sys_avg guide slice scaling and policy choice; futex/chain boost raises priority for locking chains.
- Wakeup timer: kicks idle CPUs with pending DSQ tasks and updates cpu_util_avg EMA each period. Period is configurable and clamped.
- Safety: SIGINT/SIGTERM and Drop detach struct_ops; watchdog triggers exit if no progress.
- Event-driven control loop: epoll integrates evdev, timerfd, and inotify (DRM vblank file) so reactions are immediate and CPU overhead is minimal.

Troubleshooting
- Scheduler won’t stop with Ctrl+C: ensure you ran scx_gamer directly in the foreground. If launched by scx_loader/systemd, stop with scxctl or systemd.
- DRM debugfs not readable: run as root or add user to video group and mount debugfs. Fallback to `--frame-hz` cadence if needed.
- No visible monitor output: use `--stats <sec>` (scheduler attached) or `--monitor <sec>` (monitor-only). Ensure the terminal is not being cleared by another tool.
- Network hitches: if `--prefer-napi-on-input` is enabled, it briefly biases NAPI CPUs during input; toggle off to compare.
- Anti-cheat: scx_gamer is a kernel scheduling policy and does not inject into game processes. However, aggressive timer settings or service management may interact with anti-cheat heuristics; prefer direct foreground runs while testing.

Testing and validation
This scheduler is under active development and testing. When evaluating:
- Compare with established schedulers (scx_lavd, scx_cosmos, default CFS) on identical scenarios
- Test scenarios: OBS/game capture, VRR on/off, varying workloads
- Measure objective metrics: 1%/0.1% frame-time lows, input latency, network stability
- Your mileage may vary; results depend heavily on hardware, game engine, and configuration

Glossary
- DSQ: dispatch queue.
- EDF: earliest-deadline-first.
- EMA: exponential moving average.
- NAPI: New API (network softirq processing path).
- VRR: variable refresh rate.

License
GPL-2.0-only

Maintainers
- Paul Reitz <PaulAnthonyReitz@gmail.com>

