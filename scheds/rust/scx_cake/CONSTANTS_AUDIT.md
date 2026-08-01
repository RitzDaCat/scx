# scx_cake — constant audit, 2026-07-30

**Maintainer direction:** flag every brittle / magic number and remove it. Cake must be
correct on any CPU. A performance drop is an accepted price for removing the rot.

Classification follows `CLAUDE.md` §Design laws → *DENOMINATE EVERY CONSTANT IN WHAT IT
PHYSICALLY MEASURES*: **A** hardware-anchored (measure, freeze into rodata), **B**
workload-adaptive (derive from observed per-task behaviour, never a global), **C**
scheduling-relative (slice multiples are correct), **D** externally anchored (wall clock
/ display).

## THE ADAPTATION MODEL — how cake travels to any CPU, GPU and game

**"No magic numbers" is not a style rule; it is what makes the scheduler portable.**
Every quantity cake uses belongs to exactly one tier, and the tier says what it adapts
to. If a value cannot be placed in a tier, it is debt.

| tier | source | adapts to | cost |
|---|---|---|---|
| **0 — hardware, measured at boot** | `build.rs` + the loader, frozen into `const volatile` rodata so the verifier folds it | a different **CPU**: core count, SMT pairing, LLC/CCD layout, hop cost | zero on the hot path |
| **1 — workload, measured at runtime** | fields the kernel already maintains in `task_struct`, plus one vote histogram | a different **game**, a different **GPU**, a different **refresh rate** | a few arithmetic ops |
| **2 — dimensionless ratios** | source constants that are *counts* or *fractions*, never magnitudes | nothing — and they do not need to | one immediate |
| **3 — DEBT** | magnitudes still baked in | nothing. These are the remaining bugs | — |

### Tier 1 is the interesting one — every term is already in `task_struct`

| quantity | how it is derived | why it travels |
|---|---|---|
| mean burst | `sum_exec_runtime / nvcsw` | per-task, exact. A 1.4 µs prefetch thread and a 415 µs renderer each get their own answer |
| starvation | `run_delay × nvcsw > sum_exec × pcount` | threshold **1.0 is a definition** — "waits longer than it runs" — not a tuning |
| wake period | `(now − start_time) / nvcsw` | per-task cadence; a 240 Hz thread and a 60 Hz thread self-scale |
| frame period | mode of in-band wake cadences, from a per-CPU vote histogram | follows the **actual display and GPU**: 60/144/240 Hz and VRR, with no configuration |
| frame floor | the same estimate, fast-down/slow-up | a *bound* must take the pessimistic side of a noisy input (§G18) |

**Nothing here names a thread, a game, a CPU model or a refresh rate.** Swap the GPU and
the frame period moves on its own; swap the game and every thread reports its own burst;
swap the CPU and Tier 0 re-measures while Tier 1 is unaffected because it is per-task.

### Tier 2 is legitimate and must not be "fixed"

`slice = 2 × own burst`, `cap = frame >> 1`, `protect = frame >> 4`, `probe = frame >> 2`,
floor decay `/16`, `HOME_PREEMPT_RAN_CREDIT_SHIFT = 1`. These are **ratios of a measured
quantity**, so they carry no machine assumption. A ratio is not a magic number; a
magnitude is.

### The test to apply before adding any constant

> Name the physical quantity it measures. If you cannot, it is a magic number.
> If you can, which tier supplies it? If the answer is "I picked it", it is debt.

## SCOREBOARD (reconciled against the live source, 2026-08-01)

**4 deleted, 10 remain.** Verified by grepping `cake.bpf.c` + `intf.h`, not from memory.

| constant | status |
|---|---|
| `PEER_WAKE_HYSTERESIS_NS` | **DELETED** — §G11.2; the arm fired 0.19–2.94%, collapsed to DEEP |
| `cake_preempt_protect_ns` | **DELETED** — §G11.4, became `frame >> 4` |
| `COMPUTE_OCCUPANT_MIN_RAN_NS` | **DELETED** — §G11.4, became `frame >> 2` |
| `cake_chain_burst_ns` | **DELETED** — §G12, replaced by the constant-free starvation test |
| `SLICE_NS` | remains — **the root**; grant role is frame-derived, vtime-unit role is the last architectural piece |
| `cake_handoff_max_ns` | remains — the boot probe exists and is still forbidden to drive it |
| `HOME_PREEMPT_YOUNG_NS` | remains — Class B: a per-task question asked with a global constant |
| `HOME_PREEMPT_BASE_MARGIN_NS` · `SLEEPER_LAG_NS` · `DEEP_WAKE_HYSTERESIS_NS` | remain — the vtime-unit family; inherit their arbitrariness from `SLICE_NS` |
| `WAKE_STARVE_WALL_NS` | remains — Class D in form but hard-codes 120 Hz |
| `CAKE_NEIGHBOUR_PROBE_DEPTH` | remains — magic depth 3 |
| `STATE_SLOT_BYTES` | remains — real physics (128 B) hard-coded instead of probed |
| `HOME_PREEMPT_RAN_CREDIT_SHIFT` | remains — **legitimate**, a dimensionless ratio |

**Added during the campaign, declared rather than hidden:** `FRAME_HZ_MIN/MAX` and
`FRAME_BUCKET_SHIFT`/`FRAME_BUCKETS` (plausibility band and vote geometry — these die
with the frame clock if it is retired), `CAKE_RATIO_SHIFT` (a pre-scale that cancels;
representation, not policy), and `FRAME_*_PROTECT_SHIFT` / `FRAME_SLICE_CAP_SHIFT`
(dimensionless ratios of a *measured* frame — the legitimate replacement form).

**Also fixed, and it was not on the original list:** `>> log2(nvcsw)` over-read every
burst by up to 2× with a sawtooth resetting at each power of two, corrupting four live
decisions. Replaced with an exact divide — and the object got **20% smaller**, because
`cake_log2_u64` was ten branches inlined at five sites (§G11.1).

---

## THE HEADLINE — the 2026-07-30 "no magic numbers" fix was nominal

`2a4948e33` / `b74536d16` moved two constants out of `SLICE_NS` divisors and into
`const volatile` rodata, labelled **HARDWARE-ANCHORED** with a comment reading *"NOT of
the timeslice, so they are dose-responsed absolutes rather than SLICE_NS divisors."*

The values were not changed. Verified by arithmetic:

| rodata constant | value on disk | equals | exact? |
|---|---|---|---|
| `cake_handoff_max_ns` | 1464 | `SLICE_NS / 2048` = 1464 | **exact** |
| `cake_preempt_protect_ns` | 375000 | `SLICE_NS / 8` = 375000 | **exact** |
| `cake_chain_burst_ns` | 93696 | `HOME_PREEMPT_YOUNG_NS` = 93750 | **0.058% apart** |

Both "hardware-anchored" constants are still `SLICE_NS` divided by a power of two. The
third is derived from the first by an invented `<< 6` and lands on top of a fourth
constant that was reached by a supposedly independent route.

**So cake does not have ~15 constants. It has ONE — `SLICE_NS = 3 ms` — and a family of
power-of-two divisors of it, three of which are relabelled as physics.** `SLICE_NS`
itself is a dose-response U-curve minimum measured on one 9800X3D against one benchmark
set. Nothing in the family travels to another CPU or another workload.

---

## FLAGGED — remove or re-derive

Ordered by blast radius.

| # | constant | site | value | class | why it is rot |
|---|---|---|---|---|---|
| 1 | `SLICE_NS` | `intf.h:43` | 3 ms | — | The root. A U-curve minimum from one host + one benchmark set, with no physical denomination. Six other constants are divisors of it. |
| 2 | `cake_handoff_max_ns` | `cake.bpf.c:150` | 1464 | claims A | Exactly `SLICE_NS/2048`. The loader *does* probe the real hop cost (625 ns median, cross-validates the 606 ns rdtscp floor to 3%) and is **explicitly forbidden from driving this value** because doing so cost mutex-handoff −35.66%. A number that beat a benchmark is frozen in over a number that measures the hardware. |
| 3 | `cake_preempt_protect_ns` | `cake.bpf.c:151` | 375000 | claims A | Exactly `SLICE_NS/8`, documented as not a slice divisor. |
| 4 | `cake_chain_burst_ns` | `main.rs:157` | `handoff_max << 6` | claims A | Invented multiplier, sitting under a comment that reads *"used directly, never a mean times an invented multiplier."* Gates routing, the cadence dose, preempt immunity **and** the slice floor — one unvalidated shift controls the whole G10 direction. Also a **cliff**: 90 µs and 95 µs threads get opposite treatment. |
| 5 | `HOME_PREEMPT_YOUNG_NS` | `intf.h:52` | `SLICE_NS/32` | **B** | Asks a per-task question ("spinner or mid-request?") with a global constant. Already flagged open as G9.7. |
| 6 | `HOME_PREEMPT_RAN_CREDIT_SHIFT` | `intf.h:57` | 1 | — | A bare shift with no denomination at all. |
| 7 | `WAKE_STARVE_WALL_NS` | `intf.h:63` | 24 ms | **D** | Correct *form* (wall clock), but hard-codes "~3 frames at 120 Hz". Wrong on 60/144/240 Hz and on VRR. Census: fires **0 of 11,980**. |
| 8 | `CAKE_NEIGHBOUR_PROBE_DEPTH` | `cake.bpf.c:139` | 3 | — | Magic depth. Census: 4.8% effective, 91.6% of the work on the miss path. |
| 9 | `STATE_SLOT_BYTES` | `intf.h:81` | 128 | **A** | Real physics (x86 adjacent-line prefetch pair) hard-coded as a literal. Wrong on ARM parts with 64 B or 256 B behaviour. Should be probed. |
| 10 | `COMPUTE_OCCUPANT_MIN_RAN_NS` | `intf.h:56` | `SLICE_NS/2` | C | Legal class, but `/2` is unjustified — no dose-response on record. |
| 11 | `HOME_PREEMPT_BASE_MARGIN_NS` | `intf.h:53` | `SLICE_NS/2` | C | Same. Census: the pinned-fire arm is **0% on all three workloads**. |
| 12 | `SLEEPER_LAG_NS` | `intf.h:51` | `SLICE_NS/2` | C | Census: **saturated 94–99.95%** — a predicate that is almost always true is not a predicate. |
| 13 | `PEER_WAKE_HYSTERESIS_NS` | `intf.h:55` | `2 × SLICE_NS` | C | Census: near-dead arm (2.94% schbench / 0.19% futex). |
| 14 | `DEEP_WAKE_HYSTERESIS_NS` | `intf.h:54` | `1 × SLICE_NS` | C | The `1×` is arbitrary; also 3 ms of margin on a 5.56 ms frame. |

## NOT FLAGGED — these are legitimate

Representation and sizing, not policy: `NSEC_PER_USEC/MSEC`, `RECIP_SHIFT`/`RECIP_ONE`/
`RECIP_MASK`/`RECIP_TABLE_SIZE`/`RECIP_INDEX_MASK`/`MAX_RECIP_WEIGHT`,
`STATIC_PRIO_BASE`, `IDLE_RECIP_INDEX` (a kernel nice-table index), `MAX_CPUS`,
`WAKE_DSQ`, `CAKE_HINT_CONF_SHIFT`/`_MAX` (bit-field geometry),
`WATCHDOG_TIMEOUT_MS` (a safety bound, not a scheduling threshold), `NR_CCDS` /
`BUILD_NR_CPUS` / `CAKE_CCD_STEAL_POLICY` (topology).

---

## THE REPLACEMENT — denominate the family in the FRAME, not the slice

A games-only scheduler has exactly one externally anchored clock, and it is not the
timeslice: it is the **frame period**. It is observable from inside BPF without any new
input — the present/swapchain thread wakes once per frame, so its inter-wake interval
*is* the frame period, self-measuring and self-updating at any refresh rate, on any CPU,
under VRR.

Re-denominating the family in frame time makes every threshold a fraction of a frame:

| today | becomes |
|---|---|
| `SLICE_NS = 3 ms` (host-fitted) | a fraction of the observed frame period |
| `WAKE_STARVE_WALL_NS` = "3 frames at 120 Hz" | literally 3 observed frame periods |
| `cake_chain_burst_ns` = `handoff_max << 6` | not a burst threshold at all — see below |
| `cake_handoff_max_ns` = `SLICE_NS/2048` | the probed hop cost, allowed to drive policy |

**And #4 should not survive as a threshold in any denomination.** Chain membership is a
question about a thread's position in a wake graph, not about a magnitude, so no
threshold on burst length can answer it robustly — which is separately evidenced by the
HD2 profile: the threads that actually stall the frame (`vkd3d_fence` 4.16 wait/run,
`vkd3d_queue` 0.98, `vkd3d-swapchain` 0.95) are *short-burst by construction*, and
`renderer`/`main` — the two the boundary was fitted to — barely wait at all
(0.0087 / 0.041). The boundary is measuring the wrong axis.

Replace with a per-task, constant-free signal:
- **frame-locked wake cadence** — does this task wake once per observed frame period?
  Tolerance denominated in that same period, so there is no global constant.
- **waker blocked immediately after waking me** — a boolean per event, no magnitude.
  (This is G9.4's mechanism, which failed because it was stored per-CPU, not per-task.
  The mechanism itself was never tested.)

Prior art already in the corpus and never built: the 2026-07-09 *criticality-scoped
protector* design — pure-eBPF frame-paced-blocker detection.

---

## SELF-DENOMINATION AUDIT (2026-07-30) — what actually needs a global clock

Every remaining use site classified by **what reference it needs**, not by what it is
named. Read off the source, not the names.

| reference needed | constants | count |
|---|---|---|
| **SELF** — the task's or occupant's own observed behaviour | `cake_preempt_protect_ns`, `HOME_PREEMPT_YOUNG_NS`, `COMPUTE_OCCUPANT_MIN_RAN_NS`, the slice-grant **cap** | **4** |
| **HARDWARE** — correctly global, just needs the probe to drive it | `cake_handoff_max_ns`, the slice-grant **floor** | 2 |
| **VTIME UNIT** — a margin against the global frontier | `SLICE_NS` (frontier-window role), `HOME_PREEMPT_BASE_MARGIN_NS`, `SLEEPER_LAG_NS`, `DEEP_WAKE_HYSTERESIS_NS` | 4 |
| **FRAME CLOCK** — genuinely wall-clock, externally anchored | `WAKE_STARVE_WALL_NS` | **1** |
| dimensionless ratio, already fine | `HOME_PREEMPT_RAN_CREDIT_SHIFT` | 1 |

**Finding 1 — four constants are the SAME QUESTION.** `cake_preempt_protect_ns` (375 µs),
`HOME_PREEMPT_YOUNG_NS` (93.75 µs) and `COMPUTE_OCCUPANT_MIN_RAN_NS` (1.5 ms) all gate on
`ran` and all ask *"has this occupant run long enough to be worth interrupting?"* Three
different magnitudes for one question, none denominated in anything. The answer is
per-task and needs no global reference at all: **a fraction of the occupant's own burst**,
which `cake_burst_ns` now computes exactly. Collapsing them deletes three constants and
needs no clock. This is G9.7's registered hypothesis, generalised from one site to three.

**Finding 2 — the frame clock has ONE direct customer, but it is the root.**
`WAKE_STARVE_WALL_NS` is the only genuinely wall-clock threshold. However the VTIME UNIT
family is denominated in `SLICE_NS`, and CLAUDE.md holds that slice multiples are
*correct* for scheduling-relative quantities — the unit genuinely is a turn. So those
four are already legal in FORM and inherit their arbitrariness purely from `SLICE_NS`
being a hand-fitted 3 ms. **Frame-deriving `SLICE_NS` therefore fixes five constants by
inheritance**, which is the clock's real job.

**Consequence for step 1c: it is LOW STAKES. Do not gold-plate crowd selection.** The
clock sets a slice length and a starvation bound. Neither needs better than a few
percent, and both already tolerate the 0.05–0.4% the mode delivers when it lands. Cheap
hysteresis, then stop.

**Finding 3 — a LIVE DEFECT in §G10.4, found by this audit.** G10.4 states the grant is
`clamp(2 × burst, chain_burst, SLICE_NS)`. It is true on **three of six insert sites**:

| site | grant |
|---|---|
| `:642` direct-idle admission | `cake_task_slice(p)` ✓ |
| `:788` self-race wake | `cake_task_slice(p)` ✓ |
| `:923` continuation arm | `cake_task_slice(p)` ✓ |
| `:603` co-location path | **flat `SLICE_NS`** |
| `:823` **the main wake arm** | **flat `SLICE_NS`** |
| `:885` kthread path | flat `SLICE_NS` (defensible — a kthread has no meaningful burst) |

`:823` is the one that matters: **§G10.2 routes every render stage through the wake arm**,
so the exact tasks G10.4 was written to protect still receive a flat 3 ms preemption
timer on a 5.56 ms frame. Two of the three are straightforward to fix; the kthread path
should stay flat and be documented as deliberate.

## THE KEYSTONE (2026-07-31) — a constant-free starvation signal the kernel already keeps

**Maintainer direction: eliminate ALL magic numbers. A value tuned to this machine does
not work on anyone else's.**

The live HD2 capture supplies the replacement. Measure a thread's wake latency **as a
fraction of its own wake period** — dimensionless, per-task, no global clock, no tuned
magnitude. Real data, `hd2-native-20260731`, 30 s, native:

| thread | own period | p99 wait | **missed cycles** |
|---|---|---|---|
| cuda-EvtHandlr | 47.8 µs | 272.79 µs | **5.71×** |
| ad pool !LP worker | 24.3 µs | 128.23 µs | **5.27×** |
| vkd3d_fence | 54.8 µs | 265.62 µs | **4.85×** |
| main | 78.8 µs | 250.09 µs | **3.17×** |
| vkd3d-swapchain | 96.0 µs | 277.95 µs | **2.90×** |
| thread pool worker | 26.5 µs | 14.78 µs | 0.56× |
| vkd3d_queue | 31.3 µs | 2.64 µs | 0.08× |
| renderer | 482.1 µs | 6.13 µs | **0.01×** |
| ModulePrefetch | 305.3 µs | 3.77 µs | 0.01× |

**570× separation, with a clean 5× gap between 0.56 and 2.90.** The threshold is
**1.0** — *a task that waits longer than its own cycle has missed one* — which is a
definition, not a tuning.

**And cake can compute it with ZERO new storage, no map, no per-wake bookkeeping.** The
kernel already maintains both terms in `task_struct`:

```
run_delay  = p->sched_info.run_delay   /* ns runnable-but-not-running, lifetime */
pcount     = p->sched_info.pcount      /* times scheduled in */
mean_wait  = run_delay / pcount
own_period = (now - p->start_time) / nvcsw        /* cake_burst_ns's twin */
starved    ⟺ run_delay * nvcsw > pcount * (now - p->start_time)   /* no divide */
```

**PORTABILITY VERIFIED, and the obvious trap avoided.** `sched_info` is gated by
`CONFIG_SCHED_INFO`, **not** by `CONFIG_SCHEDSTATS` and **not** by the
`kernel.sched_schedstats` sysctl. On this host that sysctl reads **0** and `run_delay`
still advances — proven with three spinners pinned to one CPU: exec +622 ms each,
run_delay +1377 ms each, exactly the 2/3 wait a 3-on-1 contention implies. `PSI`,
`TASKSTATS` and `SCHEDSTATS` all select `SCHED_INFO`, so it is present on essentially
every distro kernel. Guard the read with `bpf_core_field_exists()` regardless, and
degrade to "never starved" if absent.

*A first attempt at this test read zero on an idle `renderer` thread and looked like a
falsification — but its `sum_exec_runtime` was also flat, i.e. the thread simply never
ran. Recorded because the false negative is easy to repeat.*

## WHAT THE KEYSTONE ELIMINATES

| constant | disposition |
|---|---|
| `cake_chain_burst_ns` (93.7 µs) | **DELETE** — chain membership becomes "is this thread being starved relative to its own cycle", not "is its burst above a magnitude". The capture proved the burst axis is wrong: the whole render chain reads 1.4–64.5 µs and classifies as *worker*. |
| the frame clock, its 2–40 ms band, `FRAME_BUCKET_*` | **LIKELY DELETE** — per-task periods need no global cadence, which is what the falsification (§STATE) says anyway |
| `FRAME_*_PROTECT_SHIFT`, `FRAME_SLICE_CAP_SHIFT` | **RE-BASE** onto the occupant's own period instead of a global frame; they stay dimensionless ratios |
| `WAKE_STARVE_WALL_NS` (24 ms) | N of the waiter's own periods |
| `HOME_PREEMPT_YOUNG_NS` | `cake_handoff_yields`' shape — "within one handoff of the end of its own burst" — already in tree |
| `cake_handoff_max_ns` (1464) | let the boot probe drive it (625 ns measured, cross-validates the 606 ns floor) |
| `CAKE_NEIGHBOUR_PROBE_DEPTH` (3) | order by `cpu_sibling` topology, already in rodata |
| `STATE_SLOT_BYTES` (128) | probe the cache line size from sysfs at boot |
| `SLICE_NS` + `SLEEPER_LAG_NS` / `HOME_PREEMPT_BASE_MARGIN_NS` / `DEEP_WAKE_HYSTERESIS_NS` | the vtime-unit family — its own architectural experiment |
| `HOME_PREEMPT_RAN_CREDIT_SHIFT` (1) | **KEEP** — a dimensionless ratio, not a magnitude |

## SEQUENCE

Removing #1 moves every C-class constant under it, so the order matters.

1. **Instrument before deleting.** Run `bench/scx_cake_wake_latency.py` on HD2 under
   native and under cake — per-role wake-to-run p50/p95/p99/max. This is the only
   endpoint that can tell whether removing a constant helped or hurt frame delivery,
   and it has never been run on this game.
2. **Delete the dead ones first — free, no policy change.** #11 pinned-fire arm (0%),
   #7 wake-starve (0 of 11,980, keep the bound, make it cheap), #13 PEER arm
   (collapse to DEEP), #8 neighbour probe depth. Census already proves these decide
   nothing.
3. **Let the probe drive #2.** The −35.66% mutex-handoff result is a *benchmark* cost
   and this campaign has accepted that price. Re-measure on the game.
4. **Introduce the frame clock**, re-denominate #1/#7/#10/#12/#14 in it.
5. **Replace #4 and #5 with the per-task signal**, deleting both constants.

**Endpoint:** HD2 wake-to-run p99/max for the vkd3d roles, plus severe-frame ratio.
**Not** a benchmark ledger — this campaign has pre-accepted benchmark regressions.
