# Diablo IV chop analysis — cake exonerated at the wake tier (2026-08-02)

Maintainer-reported visible chop in Diablo IV under the G23 build. Analysis
only; nothing changed. First Diablo IV data in the corpus.

## Context and identity

- `sudo ./scx_cake` attached 19:13:11, once (`enable_seq` 1), zero stalls,
  never ejected. Binary identity behaviorally CONFIRMED as the G23 tip: both
  interrupt sinks are avoided (below), which only the per-line detector can
  produce. (Byte-proof unavailable — root-owned /proc; sudoless invariant
  broken by hand-launch. Earlier "USB sink missed at attach" inference is
  FALSIFIED by this data.)
- Regime: load 3-5, MangoHud cpu_load 19.5%, gpu_load mean 64% (18 unique
  values — a live reading, not the 97.0 artifact). Brave (pid 42851) runs a
  15-thread `Thread<NN>` pool producing ~47% of ALL sched events; no Brave
  audio stream. TACT streamers near-idle during the window (1,063 wakes).

## The chop, quantified (MangoHud per-frame, 32 s, 4,575 frames)

| metric | value | reference (HD2 G22 run) |
|---|---|---|
| median frametime | 4.46 ms (~224 fps) | 3.3 ms |
| p95 / p99 / p99.9 | 20.22 / 26.25 / 33.29 ms | — / — / ~4.4 ms |
| **severe (>2× median)** | **24.92% — 1,140 of 4,575** | **0.000-0.008%** |
| worst 10 | 32.0-35.5 ms | — |

A quarter of all frames land at 20-35 ms against a 4.5 ms median, with
neither CPU (19.5%) nor GPU (64%) saturated.

## Cake's service to Diablo (percpu_wake on 25 s / 7.8M events, same window)

| role | n | mean | p99 | CPU5 share | CPU13 share |
|---|---|---|---|---|---|
| vkd3d_queue | 77k | 1.09 µs | 3 µs | 0.3% | 0.3% |
| vkd3d_fence | 80k | 2.18 µs | 20 µs | 0.8% | 1.3% |
| vkd3d-swapchain | 260k | 1.69 µs | 9 µs | 0.6% | 0.5% |
| RenderJobWorker | 23k | 4.81 µs | 56 µs | 1.6% | 1.5% |
| Diablo IV.exe (39 thr) | 227k | 2.01 µs | 10 µs | 0.7% | 0.5% |

Fair share is 6.25%/CPU: G23 crushes both sinks to 0.3-1.6% across every
role, and residual sink wakes still pay 7-18 µs means — the penalty is real
in this game too, and avoided. CPU 2 (NIC, unflagged at attach) shows a mild
penalty (fence p99 72 µs, swapchain p99 129 µs at ~4-5% share) — the
predicted "third sink flags only under net load" gap, µs-class.

## Verdict

**The chop is not scheduler-shaped.** Severe frames arrive at ~36/s; wake
delays above 1 ms are order-of-a-few per role per 25 s (max column), three
orders of magnitude short of accounting for the hitch rate, and every role's
wake service is µs-class. A 20-35 ms hitch with idle CPU capacity and µs
runqueue delays lives in blocked-time or on the GPU/present side —
candidates, in prior order: pipeline/shader recompilation after the repair
(fresh files, cold caches), GPU contention from the Brave worker pool /
compositor, engine-side asset waits. These need GPU-side instruments or the
discriminator below, not sched traces.

**Discriminating next step (user-side, 60 s):** close or fully suspend
Brave, play 30 s, re-log frames via the MangoHud socket. Severe% collapsing
implicates the browser; unchanged severe% while caches warm implicates
recompilation (it also decays on its own as the cache fills).

## The elimination chain (same evening, retest + decomposition)

**Clean-field retest** (user's Brave closed; the 15-thread `Thread<NN>` pool
turned out to be the ASSISTANT HARNESS's own Playwright headless browser —
SwiftShader software-GL, killed for the retest; a measurable noise source
this project's own tooling put on the box):

| | run 1 (churn) | run 2 (clean field) |
|---|---|---|
| severe (>2× median) | 24.92% | **25.47%** |
| median | 4.46 ms | 4.09 ms |
| gpu / cpu load | 64% / 19.5% | 49% / 13.9% |

Severe rate is INVARIANT to removing all external load → contention
falsified. Temporal shape: **1,102 / 897 isolated single-frame spikes, mean
run length 1.0, uniform 23-27% across every third of both captures** →
focus-loss throttling falsified (needs one contiguous block), shader-cache
warming heavily disfavored (needs bursts + decay). The pattern is
metronomic: ~3 fast frames, one 20-35 ms frame.

**Scheduler kill-shot** (`wake_maxdecomp.py`, 200 µs threshold, same
session): vkd3d_queue 18 / vkd3d-swapchain 132 / RenderJobWorker 95 events
per 10k transitions above 200 µs, **zero events above 1 ms on every role**;
worst holders are kwin_wayland at 224-252 µs and worker-self at 589 µs. The
cake slice-cap/preemption theory for a 30/s hitch cadence is dead — three
orders of magnitude short in magnitude and rate. Caveat: sched trace window
(19:29) precedes the frame logs (19:35/19:41) by ~6 min, same session.

**Standing attribution:** frame *pacing* on the present path — Diablo's
limiter / vkd3d-proton swapchain rhythm / 240 Hz VRR interplay (the
3-fast-1-slow shape is a fence/present-queue signature) — or engine-internal
waits. Game-config territory, not scheduler territory. First knob to try:
an in-game FPS cap (~120) or vsync toggle; a pacing sawtooth flattens under
a cap, an engine wait does not.

## Artifacts

- Frame log: `~/Benchmarks/Diablo IV_2026-08-02_19-35-54.csv` (+ summary)
- Sched trace: `~/Benchmarks/diablo_sched_2026-08-02_1929.perf.data.zst`
  (911 MB → 64 MB; `zstd -d` then `percpu_wake.py <file> <roles>`)
- Diablo thread inventory, timeline (19:00 launch, 19:13:11 attach,
  19:21:39/19:23:59 session deaths, repair, relaunch): session transcript
  2026-08-02 evening.
