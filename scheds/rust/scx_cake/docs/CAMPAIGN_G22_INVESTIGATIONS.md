# Campaign G22 — twenty investigations into game performance

Opened 2026-08-02, maintainer direction: "this is a campaign so do 20 more
investigations to improve game perf to find additional performance."

**An investigation is hypothesis → discriminating measurement → verdict.** A null is a
result and is recorded with the same weight as a win; an unrecorded null gets paid for
twice. Not every investigation becomes a commit.

**Cost discipline.** Retained-trace analysis costs nothing and needs no game, so it runs
first and picks the targets. Live capture is 22 s + parse. A built change scored ABBA is
~20 min. Order the queue by (expected information) / (minutes), not by interest.

## Status board

| # | investigation | tier | verdict |
|---|---|---|---|
| 1 | IRQ sink completeness — are there sinks beyond nvidia? | retained/host | **✅ THREE sinks: nvidia/13, USB/5, ethernet/2** |
| 2 | G21.1 — SMT sibling of a sink (CPU 5 at 1.30×) | — | **⛔ WITHDRAWN — CPU 5 is its own USB sink; confounded** |
| 3 | `data-loop.0` RT audio preempts the renderer | retained | **✅ 62.4% of renderer preemptions; cake can't schedule RT** |
| 4 | `kwin_wayland` preempts render + input | retained | **✅ majority preemptor of input (55%) + vkd3d_queue (59%)** |
| 5 | What is left in `main`'s tail after G21 | retained | **✅ 24.9% busy+moved, 8.6% idle+moved** |
| 6 | The wakes that migrate — who and why | retained | **✅ helps 2 threads, hurts 2 — no blunt rule** |
| 7 | `cuda-EvtHandlr` 609k wakes | retained | **✅ 1.3% moved carry 17.2% of its delay** |
| 8 | `vkd3d_fence` — the busiest thread on the box | retained | **✅ SMT 6.36× only at 12+ busy; no lever** |
| 9 | Frame clock bootstraps at 1/60 s on a 240 Hz panel | source | **✅ FALSE — seeds at band edges (40 ms / 2 ms)** |
| 10 | Does G21 hold under 8-spinner load (expected null) | capture | **✅ CONFIRMED NULL — full overlap, p50 identical** |
| 11 | `Window & Input` — the 4.5× domain, never targeted | retained | **✅ worst CPU is the USB sink (0.91 / 15.0 p99)** |
| 12 | `FAudio` game audio — the ~5× domain | retained | **✅ clean except CPU 1 (nvme0q1) at 3.2×** |
| 13 | IO / DirectStorage — never validly measured | capture | **⛔ BLOCKED ON WORKLOAD — no DirectStorage thread exists in this scene** |
| 14 | Network / PartyNetworking — never validly measured | capture | **✅ n=549/22 s — CONFIRMED unusable, needs its own capture** |
| 15 | `rcu_preempt` on the render path | retained | **✅ steady 5–11% third source everywhere** |
| 16 | wineserver / proton syscall overhead | retained | **✅ NULL — 175k wakes at mean 0.30 µs, p99 1.0** |
| 17 | Migration cost charged by G21 (+9.7%) | retained | **✅ subsumed by 6 — stratified, not a blunt cost** |
| 18 | Thread-pool workers — 698k wakes | retained | **✅ 2.4% moved at 9.5×; `cuda-EvtHandlr` is 21.5% of its preempts** |
| 19 | Same-core vs cross-core wake cost (SMT pairing) | retained | **✅ MOSTLY CONFOUND — 1.3× where avoidable** |
| 20 | Frames — the campaign's standing endpoint gap | **FRAME** | **🏆 G21 WINS BOTH SCORED METRICS 2/2** |

**19 closed, 1 blocked on workload.** Two changed the priority list rather than
adding to it: #2 withdrew a change I had already recommended, and #9 deleted a false open
item from `CAMPAIGN_LEDGER.md`.

**The one target worth building next is the `idle+moved` stratum** (below): 3-25× cost, no
defensible reason to exist, and two candidate mechanisms that demand opposite fixes — so
the next step is the discriminating measurement, not a patch.

## Findings

### 1 — THREE interrupt sinks, not one. G21's premise was narrower than the truth.

| IRQ | device | CPU | lifetime | concentration |
|---|---|---|---|---|
| 115 | nvidia | 13 | 1.40 B | **100%** |
| 46 | **xhci_hcd (USB)** | **5** | 707 M | **100%** |
| 135 | **enp10s0 (ethernet)** | **2** | 231 M | **100%** |

Every device pins one CPU, and nvme queues spread over CPUs 0/1/2/4 at ~10 M each.
**USB is the mouse and keyboard**, so the input thread has an interrupt sink of its own.

**This CORRECTS the G21 write-up.** CPU 5's 1.30× penalty was attributed to being CPU 13's
SMT sibling. It is that, but it is also the USB sink in its own right, and the two cannot
be separated from the G21 data. G21.1 as originally framed is withdrawn.

G21's probe did not flag CPU 5, correctly: at sample time it carried 4,100 irq/s against a
3,834 fair share (1.07×), because USB interrupt rate follows the mouse polling rate rather
than the lifetime total. **The sink set is rate-dependent and therefore workload-dependent.**

### 11 — input's worst CPU is the USB sink, confirming §1 on the domain that cares

`Window & Input` per-CPU wake→run, cake G21 arm: **CPU 5 costs 0.91 µs mean / 15.0 µs p99
against ~0.50 / 2-3 everywhere else.** The thread that consumes USB input is slowest on the
CPU that takes the USB interrupt.

### 4 — `kwin_wayland` is the majority preemptor of the input thread

`Window & Input` is preempted while runnable 596 times in 22 s, a median **8 µs into its
run**, and **55.2% of those are `kwin_wayland`**. The compositor cuts the input thread off
almost immediately after it starts.

### 6 / 17 — a wake that MOVES off its target CPU costs 10-18×

| role | stayed on target | moved | share moved |
|---|---|---|---|
| `Window & Input` | 0.45 µs | **5.79 µs (12.9×)** | 1.4% |
| `FAudio_AudioCli` | 0.44 µs | **4.37 µs (9.9×)** | 1.6% |
| `PartyNetworking` | 0.55 µs | **9.79 µs (17.8×)** | 2.6% |

Consistent across three unrelated threads. For `Window & Input` those 1.4% of wakes carry
**15.6%** of the thread's whole wake-delay budget.

### 19 — SMT sibling contention is MOSTLY A CONFOUND. Weak lever, do not build it.

Raw, it looked huge: sibling-busy wakes cost 2.0× (input), 1.8% (audio), 2.8× (network) on
the mean and 6-18× on p99. **Two controls — restrict to wakes served by an IDLE target CPU,
then bucket by how many CPUs were busy at that instant — remove most of it:**

| role | 0-3 busy | 4-7 busy | 8-11 busy | 12+ busy |
|---|---|---|---|---|
| `main` | 1.40× | 1.34× | 1.25× | **3.56×** |
| `vkd3d_fence` | 1.65× | 1.31× | 0.97× | **6.36×** |
| `renderer` | 0.97× | 0.97× | 0.89× | 1.09× |

**The renderer shows no sibling effect at any concurrency.** The large ratios live only at
saturation — and that is precisely where no free-sibling CPU exists to move to (`main` at
12+ busy: 161 free-sibling wakes against 1,036 busy). Where the choice exists the effect is
1.3×; where the effect is 3-6× the choice does not exist. **Not worth a placement rule.**

### 3 / 15 — `data-loop.0` is the dominant preemptor of the whole render path

The SCHED_FIFO PipeWire thread (prio −21, affinity 0-15, so it floats) takes the CPU from
every game thread measured:

| victim | `data-loop.0` | `kwin_wayland` | `rcu_preempt` | median run-age |
|---|---|---|---|---|
| `renderer` | **62.4%** | 22.5% | 11.3% | 508 µs |
| `main` | **53.8%** | 16.1% | 9.4% | 97 µs |
| `cuda-EvtHandlr` | **47.6%** | 19.0% | 5.2% | 3 µs |
| `thread pool wor` | **30.1%** | 13.1% | 5.5% | 34 µs |
| `vkd3d_queue` | 30.0% | **59.0%** | 9.0% | 52 µs |

**Cake cannot schedule it** — RT sits above SCX entirely — so the only lever is refusing to
co-locate latency threads with it. Cake already has RT-owned-CPU avoidance (M8); that it
still lands 2,054 times on the renderer says the existing guard does not cover a floating
RT thread. `rcu_preempt` is a steady 5-11% everywhere and is the third source.

`kwin_wayland` is the MAJORITY preemptor of `vkd3d_queue` (59.0%) and `Window & Input`
(55.2%) — the compositor, not the game, owns those two interruptions.

### 6 / 17 — migration causality: it HELPS two threads and HURTS two. Stratified.

Raw correlation says moved wakes cost 3.7-32× and carry 17-42% of each thread's delay
budget. That is exactly what a CURE correlated with its disease looks like, and the corpus
already falsified migration as the cause of the Palworld tail once. Stratifying by whether
the target CPU was busy at wake time:

| role | target busy + stayed | target busy + moved | verdict |
|---|---|---|---|
| `vkd3d_queue` | 31.52 µs | **7.26 µs** | **migration helps 4.3×** |
| `renderer` | 6.71 µs | **4.16 µs** | migration helps 1.6× |
| `main` | **9.54 µs** | 11.77 µs | migration costs 1.23× |
| `Window & Input` | **2.19 µs** | 5.79 µs | migration costs 2.64× |

**One blunt migration rule must lose a side** — the same law as futex-vs-x265. Do not
propose a global "migrate less" or "migrate more" change.

### THE OPEN LEVER — wakes moved off an IDLE target, which is pure loss

| role | idle+moved | cost vs idle+stayed | share of the thread's delay |
|---|---|---|---|
| `main` | 1,955 (0.5%) | 7.55 vs 0.30 = **25×** | **8.6%** |
| `renderer` | 3,041 (7.3%) | 1.67 vs 0.53 = **3.2×** | **18.5%** |
| `vkd3d_queue` | 6,193 (3.9%) | 1.60 vs 0.32 = **5.0×** | **11.8%** |

There was no provocation: `select_cpu` named a CPU, that CPU was idle at wake time, and the
task ran somewhere else anyway, paying 3-25×. **Unlike every other row in this document
this one has no defensible reason to exist**, and it is the largest clean target found.

Most likely mechanism, UNVERIFIED: cake's core law is *wakeups queue globally*, so a
globally-queued wake carries `select_cpu`'s CPU as its `target_cpu` but is consumed by
whichever CPU dispatches first. If so, the cost is the price of the global route on a game,
and the question is whether an idle named target should short-circuit it. Falsifying
alternative: the idle bit was already reserved by a concurrent waker, so `is_idle` was
false to cake even though the trace shows the CPU idle. **Distinguish these before building
anything** — they call for opposite changes.

### 12 / 14 — audio and network, per-CPU

`FAudio_AudioCli` is clean (mean 0.51) except CPU 1 at 1.36 (3.2×) — CPU 1 holds nvme0q1.
`PartyNetworking` remains **n=549 in 22 s and unusable for scoring**, as STATE.md said; it
concentrates 28% of its wakes on CPU 0 and shows CPU 1 at 37 µs p99. Both need a capture
designed for them, which is investigations 13 and 14.

### 20 — FRAMES. The campaign's first frame win, on the metrics GAME-FIRST names.

ABBA, 40 s arms, ~12,000 frames each, MangoHud driven through its per-PID control socket
with nobody at the desk. `mangohudctl` does NOT work for this — the harness's
`mangohud-socket` helper is not installed, so the payload `:logging=1;` / `:logging=0;`
goes straight to the abstract socket `@mangohud-<pid>`.

| metric | A1 / A2 | B1 / B2 (G21) | delta |
|---|---|---|---|
| **0.1% low** (scored) | 194.7 / 196.2 | **203.5 / 217.4** | **+7.70%, separated 2/2** |
| **p99.9 − median** (scored) | 1.360 / 1.312 ms | **0.953 / 0.873** | **−31.64%, separated 2/2** |
| severe-frame % (screen) | 0.000 / 0.008 | 0.008 / 0.000 | overlap, ~zero both |
| avg fps | 302.1 / 302.3 | 302.1 / 302.0 | overlap — GPU-bound |
| median ms | 3.315 / 3.314 | 3.326 / 3.325 | **+0.33%, B WORSE, separated** |

**Every G10–G21 verdict before this was a proxy. This is the endpoint.** G21 wins both
metrics the game contract actually scores, and the wake-latency proxy pointed the right way.

**Caveats that must travel with it.** n=2 per arm. **0.1% low is computed from ~12 frames
per arm at this capture length** — thin, and the reason p99.9−median matters more here; the
two agree, which is the real support. The scene is whatever the game was left sitting in
(302 fps, GPU 97%). And the median is genuinely **0.011 ms worse** under G21, separated 2/2
— tiny, but it is a real cost and not noise: pushing work off CPU 13 spreads it, and the
typical frame pays a hair for the tail improvement.
