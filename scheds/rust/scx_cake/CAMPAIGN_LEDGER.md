# scx_cake — games campaign ledger (G10 → G20)

**Purpose: reflect and spot trends at a glance.** STATE.md holds the narrative in
reverse-chronological sections; this holds one row per experiment so the arc, the
verdicts and the cost are readable in one pass. HYPOTHESES.md §G11.1–§G17 holds the
design rationale. `scx_cake_bench_assets` holds the raw runs.

**Rule for this file: every number is measured, and its EVIDENCE CLASS is named.** A blank
verdict means never measured — not "fine".

## Evidence classes, strongest first

| class | what it can conclude | what it cannot |
|---|---|---|
| **FRAME** | a gaming result (0.1% low, p99.9−median, severe-frame ratio) | — |
| **WAKE** | per-role wake-to-run latency on the real game | that frames moved |
| **CENSUS** | whether a decision fires, and how often | whether it helps |
| **STATIC** | insns, spills, divides — deterministic build attribution | any performance claim |
| **SIM** | load shape only. **Disqualified for latency** (2 µs vs the game's 265 µs) | tails, frames |

## The ledger

| # | change | evidence | verdict | key numbers |
|---|---|---|---|---|
| G10.2/3 | route on burst CLASS, not the wakeup bit | STATIC | **unmeasured** | — |
| G10.4/5 | per-task slice; stages preempt-immune | STATIC | **unmeasured** | +230 insns |
| G10.6 | chain-gate the cadence dose | STATIC | **unmeasured** | 0 spills, 1450 insns |
| G11 s1 | frame clock, minimum selector | WAKE-ish | ❌ **null** | median 4828 µs vs true 5556, oscillating |
| G11 s1b | frame clock, **mode** selector | direct | ✅ | 5533–5580 µs = **0.05–0.4%** error |
| G11 s1c | incumbent hysteresis | direct | ✅ | locked **28 s of 30** |
| G11.1 | exact burst (kill `>>log2`) | STATIC | ✅ | banding ≤2× removed; **1530→1219 insns (−20%)** |
| G11.2 | collapse the PEER arm | CENSUS | ✅ | arm fired 0.19–2.94% |
| G11.3 | wake arm gets the per-task slice | STATIC | **unmeasured** | G10.4 covered only 3 of 6 insert sites |
| G11.4/5 | protection + slice cap → frame fractions | STATIC | **unmeasured** | 1.08× at 180 Hz; corrects 240 Hz |
| G12 | starvation predicate replaces burst class | SIM + CENSUS | ⚠️ **null on sim** | fires 1.48% game / 19.44% saturated (**13× span**) |
| **G13** | **cache-warm home claim** | **WAKE** | ✅ **KEPT** | same-CPU gap **8.9–16.7 → 0.9–4.5 pts**; wake p99 **cake 4/5** |
| G14 | preempt on the continuation arm | WAKE | ❌ **reverted** | renderer unchanged; locality **−2 to −4** on every role |
| G15 | that preempt's guard → waker's own cycle | WAKE | ❌ **reverted** | renderer 41.8→31.1 **but** fence + swapchain lost their wins |
| G16 | kick HOME when home is idle | WAKE | ❌ **reverted** | null — **proved** the kernel already rescheds an idle owner |
| **G17** | **anti-collision: never queue behind a served peer** | **WAKE, interleaved** | ✅ **KEPT** | renderer **−17.3%**, main −21.7%, queue −49.2%, fence −14.0% |
| **G18** | **slice cap takes the PESSIMISTIC frame estimate** | **WAKE, interleaved** | ⚖️ **kept; endpoint untestable** | wake p99 **−16%** 2/2; stalls >2ms **underpowered at n=2** (0–1 per arm) |
| — | **first FRAME read of the campaign** (G17 vs native) | **FRAME** | ⚖️ **tie on score, win on smoothness** | deadline miss **−53%**, FT stddev **−16%**, p99−med **−13%**; 0.1% low + p99.9−med **tied** |
| — | **max-stall decomposition** (retained traces, no capture) | **WAKE** | 🚨 **retracts a headline** | perf arms ring buffers per CPU, so G18's 4.78 ms "residual" was its own attach transient. By RATE cake is **3–5× better** than native, not worse |
| — | **five-domain sweep** (input/audio/net/IO/render, retained) | **WAKE, interleaved** | 🏆 **cake 4 of 7, 2/2 each** | win scales INVERSELY with burst: FAudio 5 us **5.0x**, input 6 us **4.5x**, renderer 40 us **2.2x**. RT audio untouchable, network+IO unmeasurable at this n |
| — | ~~harness is the floor (`python3` holds)~~ | — | ❌ **RETRACTED same day** | `python3` was the 8 SPINNERS (793% of a core, 174 CPU-s of 352). Comm inferred, never checked. `bench/cakeload.c` removes the ambiguity |
| — | **G14/G15/G16 rechecked on input** | **WAKE, cross-run ratio** | ⚪ **no resurrection** | G14 0.20 vs shipped 0.22, G16 0.39, **G15 0.64 — worst on input too, falsification confirmed** |
| — | **input-thread decomposition** (`Window & Input`, retained traces) | **WAKE, interleaved** | 🏆 **cake wins 2/2** | delays >200 us/10k: native 294.6/93.2 vs cake **39.0/47.8** — **4.5x**, no overlap. Never scored before |
| **G20** | **a kthread wake spends an idle CPU, not an occupant** | **WAKE + FRAME** | ❌ **null; mechanism UNTESTED** | wake >200 µs 10.7/12.3 vs G18 18.0/**9.5** — no separation. `DP-2` fired **45×** tonight vs 11,518 this afternoon on the SAME G18 code, so its premise was absent |
| — | **frame ABCCBA, native vs G18 vs G20** | **FRAME** | 🔻 **cake LOSES** | 0.1% low native **225.8** vs G18 212.8 / G20 213.9 — **2/2 no overlap, −5.6%**. Native also wins p99.9−med, deadline miss, stddev, max FT. Easy scene (native 1.00% miss vs 3.02% on 08-01) |

## The frame result (2026-08-01, the campaign's first)

ABBA, 2 runs/arm, HD2 menu, GPU-bound 97%, 240 Hz VRR, unattended.

```
240 Hz deadline miss %   native 3.019  |  G17 1.419   -53%   G17 2/2
FT stddev ms             native 0.258  |  G17 0.217   -16%   G17 2/2
p99 - median ms          native 0.711  |  G17 0.619   -13%   G17 2/2
0.1% low                 native 193.3  |  G17 188.6          overlap  <- SCORED metric
p99.9 - median ms        native 1.085  |  G17 1.088          overlap  <- SCORED metric
spikes >2x median %      native 0.004  |  G17 0.008          overlap  <- SCREEN metric
```

**Tie on the three metrics GAME-FIRST actually names; clear win on consistency and
deadline adherence.** Caveats that must travel with it: menu scene not gameplay,
GPU-bound so avg FPS cannot separate, n=2, and NOT comparable to the 2026-07-30
war-table numbers.

## Trends

**Renderer wake p99 (cake, real game under load).** Only the interleaved pair is valid;
cross-run rows are shown for completeness and must not be compared to each other.

```
G13  49.8us  ████████████████████   <- interleaved, same run
G17  41.2us  ████████████████       <- interleaved, same run   -17.3%
native 6-9us ███
```

**Object size and safety** — zero spills and zero fills in every function, every commit:

```
G10.6  1450 insns
G11.1  1219   (-20%, exact arithmetic is SMALLER)
G13    1359
G17    1433
```

**Magic-number removal:** 14 flagged → **4 deleted** (`PEER_WAKE_HYSTERESIS_NS`,
`cake_preempt_protect_ns`, `COMPUTE_OCCUPANT_MIN_RAN_NS`, `cake_chain_burst_ns`),
10 remain, ~6 added in legitimate form (dimensionless ratios of a measured frame).
Reconciled scoreboard in CONSTANTS_AUDIT.md.

## What the campaign established about GAMES (independent of any patch)

1. **Regime is the first covariate.** `vkd3d_fence` wake p99: **3.16 µs** quiet, ~50 µs
   loaded, **265 µs** heavily contended — **84×**. A calm-machine game measurement has
   nothing to fix.
2. **`ops.enqueue` sees 0.14% of a game's dispatches** (23.6% saturated). Nearly all of
   cake's routing is inert on a game.
3. **The vtime clamp is decoration on a game** — the target queue already held work
   **163 times in ~7M** direct dispatches (1 in 42,935).
4. **Cake's wake path is not slower than native** — renderer p50 matches to two decimals.
   Only ~4% of wakes go bad.
5. **Cake fixed worker interference and created peer interference**: workers block the
   renderer 8.6% (native 42.5%), peers 58.1% (native 18.9%).
6. **~30% of slow wakes on BOTH schedulers land on an idle CPU** — C-state exit, ~24–93 µs,
   which no placement policy fixes.

## Open, in priority order

1. **Frame data for everything above.** Frames are the endpoint; the whole G10–G17 arc is
   scored on a proxy. Now unattended — see the rig in STATE.md.
2. G17's **mechanism** is unexplained (peer share 58.1%→56.9% while the tail fell 17%).
3. `SLICE_NS`'s vtime-unit role — the last architectural constant.
4. Frame clock bootstraps at 1/60 s on a **240 Hz VRR** display: 4× too slow.
