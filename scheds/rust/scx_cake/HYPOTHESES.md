# scx_cake hypothesis graph

**The research structure (adopted 2026-07-19).** Nodes are mechanism claims with an
evidence status; edges are dependencies. Rules of use:

- **Before building any mutation, place it on this graph.** If every parent is
  FALSIFIED, do not build it. If a parent is OPEN/UNTESTED, test the parent first
  (cheapest discriminating experiment — usually a census or observed pair, ~4 min).
- **Pick the next experiment by graph cut**: the one that prunes the largest open
  subtree, not the next item in a queue.
- **A falsification propagates**: mark every descendant blocked-by-falsified rather
  than re-testing it. A regime qualifier (like R1 below) re-opens descendants only
  for that regime.
- Statuses: `CONFIRMED(evidence)` / `FALSIFIED(evidence)` / `OPEN` / `REGIME(x)` —
  true only in regime x. Regimes: **Rq** quiet desktop, **Rc** contended (external
  compute ≥1 core).

Legend: `<-` depends on. Evidence = STATE.md gap list + runs/exact_pair + memory.

---

## G1. Wake service under load (the trunk)

- **N1 CONFIRMED(Rq)**: "wakeups global, continuations local" is the winning routing
  law in quiet regimes (+57 futex, +23 pipe, +49 schbench-sat, 2026-07-17/18).
- **N2 CONFIRMED**: sleeper clamp quantizes vtime — erases head identity AND preempt
  deservingness. (Falsified F-vtime-identity; falsified G/H margins.)
- **N3 CONFIRMED(Rq)**: stranded tail wake ≡ storm head — no state signal separates
  them; extra wake service of any form re-splits handoff pairs. (7 falsifications,
  arc 2026-07-19.) *Regime-qualified 2026-07-19: proven in Rq only.*
- **N4 OPEN (TOP TARGET)**: under Rc, wake-global service degrades to slice cadence —
  ALSO audible: benchmark-storm saturation starves the GoXLR Utility audio app under
  BOTH schedulers (overrun bursts in native AND cake windows, 2026-07-19 call) — the
  criticality-protector direction (standing direction 2) attaches here as N4d OPEN:
  protect the active audio/input chain under saturation; a win here is user-audible.
  cake futex 2.05M→0.35M (−82%) while native holds BOTH workloads; kworker p99 17ms;
  schbench-light −1.4→−9.6. `<- N1` (the same routing law is the suspect).
  Discriminator: load-ladder observed pairs (idle→1c→3c external compute), curves for
  cake AND native. Children (all OPEN, blocked on N4 evidence):
  - N4a: bounded wake service is safe in Rc even though falsified in Rq (N3 may not
    transfer — the falsifying regressions were Rq futex storms).
  - N4b: the collapse is occupant-slice waiting (no preempt for globals mid-slice).
  - N4c: the collapse is WAKE_DSQ ordering (herd position), not slice waiting.
- **N5 CONFIRMED**: pinned tasks (user AND kthread) bypass the wake path; unpinned
  service can never reach them. (Census 2026-07-19; basis of K and L.)
  - N5a **CONFIRMED**: pinned-user-wake preempt by raw depth recovers lock-pi
    −86.6→−71.8 (K, kept; 8-block). Residual −72 plateau: **N5b OPEN, narrowed
    2026-07-19** — branch census (tag `archive/lockpi-plateau-diag`): 82% of pinned wakes
    land on IDLE-owned CPUs (core rescheds; healthy), busy targets split ~50/50
    fired(22k)/margin-refused(20k). K2 null ⇒ refusals alone aren't the tail either.
    **RESOLVED 2026-07-19 evening: the plateau WAS the regime.** Under the day's
    contended host, K+L measure lock-pi at −3.8% CI[−6.5,−1.1] — near parity from
    −86.6. (Run exact_pair_20260719T183137Z; UnrealEditor 200-300% during arms.)
    Remaining −3.8 may close in true Rq — measure when actually quiet.
  - N5c **CONFIRMED**: all-kthread local insert fixes watchdog eviction (L, now 5/5
    survival incl. two Rc runs where baseline died in the same session). Futex
    score-neutrality UNMEASURABLE vs baseline under Rc (baseline evicts); the −80.8
    reading is N4's collapse (mutation-independent: J −83.6, L −80.8, L-survival
    −82.x all within noise of each other). Pending: TRUE-Rq score check + game gate.
- **N6 FALSIFIED**: ops.quiescent can be registered casually — costs futex −84% at
  stage 0 via core fast-path loss (J). Any future sleep-marker needs a different hook.
- **N7 OPEN (kernel lane)**: waker-intent signal (handoff vs tail wake) would escape
  N3 in Rq. Blocked on kernel patch; only relevant if N4 work doesn't subsume it.

## G2. The cache/memcpy share trade

- **M1 CONFIRMED**: ccm trade is CPU-share reallocation (sleeper catch-up), equal
  per-usr-s efficiency; zero-sum in share space. (2026-07-18 attribution.)
- **M2 CONFIRMED**: unlike-type SMT pairing raises memcpy efficiency +73% — total
  throughput headroom exists (pinned diagnostic; feasibility 347/480 CPU·s).
- **M3 FALSIFIED**: stochastic drift (v1/v2) can produce segregation — wake remixing
  defeats it. `<- M2`
- **M4 FALSIFIED**: per-CPU class marks + wake-veto — collides with futex homing
  (−28pt). `<- M2`
- **M5 OPEN**: per-TASK duty class with handoff-exempt wakes achieves segregation.
  `<- M2, blocked-by-lessons M3+M4+N6 (no quiescent hook; marker must avoid the
  wake fast path)`. Discriminator: needs a class-bit design that survives N6.
- **M6 OPEN (kernel lane)**: PMU kfunc for true memory-boundness classes. `<- M5`

## G4. Ordering structure (opened 2026-07-19; visual review artifact fb054574)

- **S0 CONFIRMED**: cake is not tree-free — vtime DSQs are kernel rb_root_cached
  (same family as EEVDF); the architectural difference is SHAPE (forest of tiny
  trees + FIFO/direct bypasses + lockless peeks), and position is chosen once at
  enqueue by key computation — "better key" is the cheap lever, in-place reorder
  the expensive one, preemption the structure-bypass.
- **S1 CONFIRMED (2026-07-20, mode-Rq)**: cadence-proportional sleeper depth
  (built 0ce54fb27) recovers futex +65pt in the degraded quiet mode (1.4M→3.3M,
  3× reproduced; 8-block +12.2% vs native) and holds pipe/ccm/sat at sealed
  values; schbench-light cost ~0.8pt (8-block trusted −2.24 vs −1.42 pre-stack).
  Game gate still mandatory before ship (2026-07-10 service-margin WoW caution).
- **S3 OPEN (NEW 2026-07-20): futex host-state MODES** — identical code+boot+
  regime reads 4.7M / 1.4M cake while native is flat; winning mode = high-ctxsw
  handoff churn (283M), losing mode = occupant-stall (84M). The host variable is
  UNKNOWN (not external CPU, not kernel, not boot, not code). Discriminator:
  census wake-queue composition + occupancy at bench start in both modes; find
  and force the 4.7M mode. All historical futex deltas must be read mode-tagged.
- **S2 OPEN (speculative)**: bounded-horizon bucket ring — the clamp bounds live
  keys to a 2-slice window, the textbook calendar-queue case: 64 bucket-DSQs of
  slice/32, ffs-bitmask dispatch, O(1) both ends, 94 µs ordering precision, global
  lock pressure diffused across 64 queues. `<- S1` (buckets are only as good as
  the key; prove S1 on rb-trees first, then the ring is a pure-speed refactor).
  Risks: frontier-rotation bookkeeping, verifier budget, starvation proof rebase.

## G5. The missing bound (opened 2026-07-27 — the Palworld game-tail trunk)

Full markup and line-level source citations:
`docs/CAKE_113_VS_120_DELTA_2026-07-27.md`. Everything here is SOURCE-DERIVED and
UNMEASURED — no node below has a benchmark row behind it yet.

- **B0 CONFIRMED**: the Palworld loss is a queueing/notification shape, not overhead
  or locality. Median identical to 3 decimals; ~4-5 **isolated** severe stalls per
  45 s (singletons — normal frame either side); 1% low worst-avg −15.2%. Migration
  FALSIFIED as the cause (schbench-light: cake migrates 28-51% more and loses ~2%).
  Cache misses cost µs; a 10-40 ms late frame is a task nothing ran.
- **B1 OPEN (trunk)**: cake has **no bound** on wake→run when its fast path's
  assumptions fail, where EEVDF bounds the same case three ways (wakeup eligibility
  check, hrtick at remaining slice, 1 kHz `entity_tick`). Supporting pattern, not
  proof: every sealed cake win is a mean/throughput statistic and every game-tail
  loss is a worst-case statistic — the signature of a good average with no floor.
  `<- B0`. Children are the candidate mechanisms; they are not exclusive, and B1
  survives if any one holds.
  - **B2 OPEN**: a home-routed wake onto a busy CPU with no idle CPU available
    receives **no kick at all** — `cake_enqueue`'s wake tail guards the fallback
    with `else if (!home && …)`, so `home == true` + no idle returns having inserted
    and notified nobody. The only other notification is the home-preempt at
    `HOME_PREEMPT_YOUNG_NS` = SLICE/32 = **93.75 µs**, i.e. the occupant is
    preemptible for 3.1% of its slice. Wait is then up to one contended slice
    (4.5 ms) against an 8.33 ms frame budget. **Discriminator P3** (read-only
    counter, no policy change, no receipt, no game A/B): rate of that exit under
    game load. **~0 kills B2.** Closely related to `N4b`.
  - **B3 OPEN**: the starvation seals are denominated in **vtime**, not wall time —
    the peer-wake hysteresis is 2 slices = "6.00 ms" (its `CAKE_*` macro and the
    OVF-rescue seal beside it are both gone as of 2026-07-27; the surviving seal
    is now the literal `PEER_WAKE_HYSTERESIS_NS` in `intf.h`, and the OVF
    deletion removed the *other* vtime-denominated seal outright, which narrows
    B3 rather than closing it),
    but the frontier advances only in `cake_running`, which the kernel skips on
    keep-running slice refills. Advance rate is ~1/depth of wall rate, so a 6 ms
    constant means 6·*d* ms. Rarity follows from needing depth AND phase to
    coincide — which fits "4-5 isolated singletons", unlike any continuous tax.
    **Discriminator P4**: frontier advance ÷ wall time per CPU during a stall
    window. **Tracks wall rate ⇒ B3 dead.** `<- B1`
  - **B4 OPEN**: `SCX_OPS_DEFINE(cake_ops, …)` registers **no `.tick`**, so a missed
    notification is uncorrectable until an unrelated event visits that CPU. Only
    backstop is the 5000 ms watchdog — a 40 ms stall clears it by 125×, invisible
    to every safety mechanism cake has. This is why B2/B3 are permanent rather than
    self-healing, so B4 is a *severity multiplier* on both. `<- B1`
- **B5 OPEN**: "restore 1.1.3" is NOT the fix and 1.1.3 is NOT a clean control.
  Four changes at once: slice 2 ms → 3/4.5 ms; no WAKE_DSQ/OVF tier → 6 ms vtime
  seals; no depth multiplier → present; latch kick → margin-gated. 1.1.3's kick
  policy is *cruder* but its **coverage is better** (a latch has no 93.75 µs window
  to miss) and its exposure is smaller on every dimension that sets stall length.
  So 1.2.0's decision quality is higher and its tail exposure is larger. The
  interesting target is "1.2.0's decisions with 1.1.3's coverage", not either
  binary. **Currently unmeasurable:** shipped 1.1.3 does not attach on kernel
  7.1.5-1-cachyos (`map 'cake_ops': BPF map skeleton link is uninitialized`).
- **B6 OPEN (do not conflate)**: 1.2.0 lost 1.1.3's zero-spill discipline (1.1.3's
  function split is a *compiler-pressure* decomposition — "3 args + 1 survivor =
  0 spills"). Worth restoring on its own merits at scheduler frequencies, but the
  hot path measures ~0.2% of one core in-game, so spills **cannot** explain 10-40 ms
  stalls. Keep this node separate from B1 so neither is used to justify the other.

## G6. The spill law as a proxy (opened 2026-07-27)

- **P1 FALSIFIED**: "any value held across any call registers as a spill, independent
  of pressure" (the `4d5b5f96d` premise). `cake_enqueue_wake` holds `p` in **r6 across
  all nine calls** at zero spills. The 8-instruction subprogram that produced the claim
  was the special case; LLVM promotes freely once a function has real work.
- **P2 CONFIRMED**: LLVM will evict a live PARAMETER to hold a compile-time CONSTANT.
  With `enq_flags` restored, insn 196 spills it and insn 215 takes r8 for the immediate
  `0x400` (`WAKE_DSQ`) while r7 and r9 sit free. Neither call-site splitting (83->95
  insns) nor hoisting `vt` above the id selection (neutral) talks it out of this.
- **P3 CONFIRMED (2026-07-27)**: both decisions restored, whole-program census held
  at 0 spills / 0 fills. `cake_home_claim` went 2 calls -> 1; the contended turn
  costs one BSS load. Static result only -- the POLICY question (does the new
  converged-pair signal route as well as the old one?) is what futex answers. a decision costs a spill iff it consults the
  KERNEL mid-flight; the same decision reading cake's own memory costs none. So each
  deleted decision is restorable at zero by changing its SIGNAL, not its meaning.
  `<- P1`. Children:
  - P3a **FALSIFIED (2026-07-27, futex -13.29%, reverted)**: the cadence home claim
    over-claims by construction. Under a futex storm EVERY worker has a tiny burst,
    so "burst < SLICE/2" fires for nearly every wake and routes it HOME — "wakeups
    global" inverted, the law whose violation measured a 20-50x collapse. The old
    callback-CPU test was narrow *by accident of its signal*; replacing it with a
    per-task property made it broad *by construction*. **LESSON: when swapping a
    signal, check the new one's FIRING RATE on the target workload, not just its
    semantics.** Evidence: exact_pair_20260728T035637Z_75cd506b41e1.
  - P3b **FALSIFIED (2026-07-27, ~-11pt more, reverted)**: `qmark` is NOT a drop-in
    proxy for `nr_queued != 0`. nr_queued is an instantaneous count; qmark is a
    STICKY hint — set by every enqueue, cleared only when the owner peeks an empty
    queue — so under a storm it is set essentially permanently and far more
    continuations took the 1.5x turn than the rule intended. Evidence:
    exact_pair_20260728T034725Z_f682a5857952.
  - P3c **CONFIRMED**: the `enq_flags` literal is not a loss at all on the wake path —
    a PRIQ insert into a custom DSQ reads none of the caller's positional bits, and
    forwarding `SCX_ENQ_IMMED` there would trip `WARN_ON_ONCE` (`dsq_inc_nr`).
- **P4 OPEN**: the census counts bytecode r10 traffic, so callee-saved promotion reads
  as free — but the JIT push/pops r6-r9 in the prologue. The real machine cost of
  "zero" vs "3 spills" may be much closer than the bytecode says.
  Discriminator: `bpftool prog dump jited` on the loaded program.

## G3. Harness/evidence (meta)

- **H1 CONFIRMED**: aggregate scores cannot localize losses; decision-level evidence
  (observatory/census) finds mechanisms in single runs. (schbench, lock-pi, P0.)
- **H2 CONFIRMED**: noise covariates expose scheduler-dependent background treatment
  — arms are only comparable WITH their covariates (UnrealEditor 260% vs 50%).
- **H3 CONFIRMED**: guard verdicts are regime-relative; compare same-day same-regime
  baselines only. (schbench −9.6 baseline shift.)
- **H4 OPEN**: replay simulator would rank policies offline (roadmap #2, unbuilt).
- **H5 ADOPTED (maintainer direction 2026-07-19)**: the master formula exists but is
  CONDITIONED — it must take regime + workload behavior as inputs. Prerequisites:
  (a) machine-readable experiment ledger — BUILT but was EMITTING ZEROS until
  2026-07-24: it read the analysis block off `COMPLETE.json` (which has none)
  and defaulted to `0`, so all 133 sealed rows read as perfect ties. Repaired
  to read `pair_result.json`, numerics default `None`; now 149 transactions
  with real verdicts, validated against the sealed pipe row. Any prior
  ledger-derived conclusion is void.
  (b) behavioral catalog per benchmark/game — decision-stream signatures (wake
  cadence, burst distribution, dependency shape) as formula inputs. OPEN.
- **H6 CONFIRMED (2026-07-24, gear Gate 1)**: offered background load is a real
  exogenous predictor of cake's delta — pooled workload-centred rho = −0.42,
  n = 125, t = −5.15; futex quiet→loaded +10.97 → −82.15 (rho −0.63, n=28).
  This is N4 confirmed at corpus scale rather than from survival runs.
  **Sub-finding H6a:** `ext_cpu_cake_med` is ENDOGENOUS (cake starves
  background work under contention — the covariate is co-produced with the
  delta); regime analysis must use `ext_cpu_native_med`. Sharpens H2.
  **Sub-finding H6b:** mutex-handoff — the Wayland-input shape — is the only
  workload that IMPROVES under load (+0.66 → +7.56, rho +0.48). A single
  global gear is therefore wrong; the gear must be conditional on workload
  shape, not machine-wide regime. → `docs/LEDGER_REPAIR_AND_REGIME_GATE_2026-07-24.md`

## Current graph cut (what to test next, in order)

1'. **B2/B3 discriminators (P3, P4) — TAKE THIS FIRST.** Two read-only counters,
   one run, no policy change, no receipt, no game A/B, no quiet host required.
   They prune the largest open subtree on the board: B2 and B3 are the only
   mechanisms proposed for the game-tail trunk B1, and either counter returning
   ~0 falsifies its node outright. Highest information gain per minute currently
   available, and it is the graph-cut rule applied literally — everything else
   below is throughput work while the hard constraint is game tails.

*(The remainder is the standing throughput queue; resume after B1 resolves.)*

0. **Gate 2 — `sched.data` retention in the broker `--observe` path.** Blocking
   item for the gear controller and for any per-event N4 attribution; must be
   on BEFORE the next contended capture, or that window yields aggregates only
   (as all ten prior observed transactions did). Harness work, justified by a
   named measurement.
1. **N4 discriminators** — load-ladder observed pairs (blocked until quiet host to
   build the ladder cleanly; can run under any regime since load is controlled).
   *Note (2026-07-24): N4's direction is now CONFIRMED at corpus scale by H6;
   what the ladder still owes is the per-event mechanism, not the existence of
   the effect.*
2. **N5c gates** — Rq futex score check for L + game gate (needs quiet host / game).
3. **N5b** — census the lock-pi −72 plateau.
4. **M5** — design the class bit within N6's constraint.

## G7 — mutex-handoff p99 (opened 2026-07-28)

**G7.0 CONFIRMED — the loss is architectural, not from the law-compliance arc.**
`handoff_p99_usec` is a p99 LATENCY tail on TWO threads, not throughput. native
1.0-1.14 us, cake HEAD 1.7 us (-47%), cake 1.1.3 1.7 us (-53.6%). 1.1.3 predates
every commit in the arc and loses the same, 8 blocks, no overlap either run.
Two threads on 16 CPUs means no contention, so every mechanism cake uses to
conserve throughput under saturation is pure overhead here. 0.7 us is about one
extra wakeup path.

**G7.1 STALE — the sealed `mutex-handoff +10.3%` (2026-07-19) no longer holds.**
The same July binary now reads -53.6% on this host. Environmental, most likely
the kernel: 7.1.5 is new enough it broke 1.1.3's libbpf struct_ops outright.
Do not cite that row.

**G7.2 FALSIFIED — the WAKE_SYNC split-redirect is not the mechanism.**
Predicted: it splits the serial pair and costs an IPI per handoff, so disabling
it should improve mutex and regress pipe (the concentrate-vs-spread law made
visible). Measured, 4 blocks per cell: mutex -46.7% -> -57.4%, pipe +23.1% ->
+20.2%. BOTH worse. Probably never fires for mutex-handoff at all. Full record:
`docs/MUTEX_HANDOFF_DOSE_2026-07-28.md`.

**G7.3 OPEN — next cut.** Remaining candidates, cheapest first: the
direct-dispatch exact-head guard in `select_cpu`; `SLEEPER_LAG_NS` clamp depth;
the empty-home carve-out's sleeper gates. THE one worth pricing first is
structural: cake always routes through `enqueue` + a DSQ, where EEVDF can keep a
WAKE_SYNC pair on-CPU with no queue at all. It is the only candidate that
plausibly costs a whole wakeup rather than a few hundred ns.

**Method.** Both eliminations came from dose-response, not source reading. The
source reading was correct about what the code DOES and silent about whether it
RUNS -- twice.

## G8 — tiered admission: direct as far as it goes, DSQ only when it must
### (design direction, maintainer-set 2026-07-28)

**Principle.** Direct dispatch is strictly faster than routing through a DSQ.
Use it as far as the load allows steering correctly; blend into the DSQ only
once the load genuinely requires queueing. Cake today has two of the three
tiers and is missing the one that matters for serial handoff.

**The bind, measured.** For a WAKE_SYNC serial handoff cake has exactly two
options and BOTH are wrong:

  (a) redirect to another idle CPU + LOCAL  -> cross-CPU wakeup + IPI, ~0.7 us;
  (b) stay on the waker's CPU + LOCAL       -> wakee queues behind a waker that
                                              is still running.

G7.2 priced the swap: disabling the redirect moves (a)->(b) and mutex-handoff
goes -46.7% -> -57.4%. Trading one bad option for the other, which is why the
falsification was informative rather than merely negative.

Native does neither. It places the wakee on the waker's CPU, which is about to
block, so the handoff is a direct context switch: no IPI, no queue, no wait.
That is the whole 1.0 us vs 1.7 us.

**The missing tier: LOCAL_ON the waker's CPU.** Cake declines it today --
"that would force a preempt and bypass live-vtime eligibility". For a GENUINE
serial handoff the preempt is not a cost, it is the point: the waker is about
to sleep, so preempting it costs nothing and skips both the IPI and the queue.
The eligibility bypass is the real objection, and it is what must be bounded.

**Proposed tiers, cheapest first:**

  1. SERIAL HANDOFF -> `LOCAL_ON | waker_cpu`, preempting. Conditions, all
     state-only (no identity, valid under queued wakeups): WAKE_SYNC set; dfl
     returned the waker's own CPU; that CPU's vtime DSQ is empty (nothing older
     to jump). Empty queue is what makes the eligibility bypass safe -- there is
     no claim to bypass.
  2. IDLE CAPACITY, NOT SERIAL -> `LOCAL` on a genuinely idle CPU. Today's
     behaviour, and it keeps the pipe win (+23.1%): a buffered stream should
     spread.
  3. SATURATED -> DSQ + vtime arbitration. Unchanged; this is where fairness
     and work conservation live.

The tier boundary is queue emptiness, not workload identity -- the same rule
cake already trusts for the empty-home carve-out.

**Falsifiable predictions.**
  P1 mutex-handoff p99 1.7 us -> approaching native's 1.0-1.14 us.
  P2 perf-sched-pipe UNCHANGED at +23% (a buffered stream's target CPU is
     rarely empty, so tier 1 should not fire for it). If pipe regresses, the
     tier-1 condition is too loose and is catching parallel pairs.
  P3 futex UNCHANGED at +54% (no WAKE_SYNC on futex wakes at all).
  P4 schbench-light must not regress -- it is the frontier-trade canary.

**Method.** Dose the tier-1 condition rather than shipping it whole: build it
gated, and vary the gate. Per G7 the source reading twice said what the code
does and nothing about whether it fires.

**G8 RESULT — tier-1 FALSIFIED AS BUILT, and the mechanism VINDICATED
(measured 2026-07-28, committed `f7fb5f788`, reverted `c3c2c27f3`).**
The revert landed with an empty message and the numbers sat unrecorded on
disk for a day. They are:

| prediction | measured | verdict |
|---|---|---|
| P1 mutex p99 1.7 -> ~1.0 us | cake p99 **1.750 -> 1.750** | **FAILED** |
| P2 pipe UNCHANGED at +23% | **+23.14% -> +8.98%** | **FAILED** |
| P4 schbench-light no regress | -0.46% (CI -0.77, 0.0) | marginal |

**P1's apparent gain was NATIVE drifting, not cake improving.** The scored
median moved -46.72% -> -39.87%, which reads like a win. Arm-attributed, it
is not: cake's p99 was 1.72 (redirect-on) / 1.750 (tier1) while native's went
1.139 -> 1.250 between runs. Cake did not move at all. **Score deltas on this
benchmark must be read per ARM -- the paired design does not protect against
one arm drifting across blocks.**

**Why it could not have fired: `futex_wake` never sets `WF_SYNC`.** Verified
at the kernel source, not inferred: `futex_wake()` -> `wake_up_q()` ->
`wake_up_process(task)` -> `try_to_wake_up(p, TASK_NORMAL, **0**)`
(`kernel/sched/core.c:4545`). Tier-1 was gated on
`wake_flags & CAKE_WAKE_SYNC`, so on `mutex_handoff` -- a pthread
mutex+condvar, i.e. pure futex -- the gate is dead code. **This is the SECOND
consecutive hypothesis (G7.2, G8) aimed at `mutex_handoff` through a
`WAKE_SYNC` gate that workload never sets.** Before gating anything on a
kernel flag, confirm the workload sets it.

**THE SCOREBOARD WAS MEASURING ONE PERCENTILE.** Arm-attributed raw output,
8 blocks per arm, three interleaved exact-pairs:

| arm | avg | p50 | **p99 (scored)** | p999 |
|---|---|---|---|---|
| native EEVDF | 0.855 | 0.837 | **1.14** | 2.25 |
| cake redirect-on | 0.824 | 0.808 | **1.72** | 1.99 |
| cake redirect-off | 0.829 | 0.808 | **1.750** | 2.01 |
| cake tier-1 | 0.840 | 0.817 | **1.750** | 2.76 |

Cake is **-3.6% on avg and -3.5% on p50** -- FASTER than native at the typical
handoff -- and better at p999 in two of three runs. It loses only p99. The
"-47% architectural loss" that opened G7 is one percentile of one benchmark.

**The p99 is PLACEMENT-INVARIANT.** Three structurally different placement
policies -- redirect on, redirect off, tier-1 LOCAL_ON+preempt -- give
1.72 / 1.750 / 1.750. Every remaining G7.3 candidate is a placement candidate,
and placement does not move this number.

## G9 — the second mode, and the co-location asymmetry (opened 2026-07-28)

**G9.0 CONFIRMED — cake's p99 is a SECOND MODE, not a tail.** Histogram of the
`mutex_handoff` shape, 800k samples, 100 ns bins, cake `cake_1.2.0` (receipt
`20260728T171317Z_direct-clamp`, tree-identical to HEAD `66f5fcd4a`) vs native,
matched load (1.59 / 1.77):

```
             0.9-1.0  1.0-1.1  1.1-1.2  1.2-1.3  1.3-1.4  1.4-1.5  1.5-1.6  1.6-1.7
cake          0.839%   0.263%   0.128%   0.148%   0.100%   0.331%   0.440%   0.211%
native        2.720%   0.692%   0.679%   0.095%   0.274%   0.174%   0.047%   0.034%
```

Cake troughs at 1.1-1.4 us and then **rises again to a peak at 1.5-1.6 us**.
Native decays monotonically. Mass above 1.4 us: **cake 1.53%, native 0.56%** --
so the 99th percentile falls INSIDE cake's second mode and outside native's.
That is the entire scored gap. cake p50 0.673 / p90 0.817 / p95 0.866 /
**p99 1.548**; native p50 0.692 / p90 0.856 / p95 0.903 / **p99 1.144**.

**G9.1 CONFIRMED — cake WINS co-located and LOSES split; native is the
opposite.** Handoff cost ladder, both threads pinned, interleaved rounds,
matched load. Spin rows are kernel-free (`rdtscp`-stamped); futex rows are the
real condvar shape (`clock_gettime`):

| config | native p50/p99 | cake p50/p99 |
|---|---|---|
| SPIN SMT siblings (0,8) | 38.5 / 57.7 ns | 28.8 / 48.1 ns |
| SPIN same-CCX cores (0,1) | 38.5 / 57.7 ns | 38.5 / 57.7 ns |
| **FUTEX same CPU (0,0)** | 702 / **1731** ns | **683 / 760 ns** |
| FUTEX SMT siblings (0,8) | 807 / 914 ns | 1317 / 1413 ns |
| FUTEX same-CCX cores (0,1) | 663 / 779 ns | 1182 / 1452 ns |
| FUTEX unpinned (as scored) | 673 / 1096 ns | 654 / 1471 ns |

Two facts, both large:
  1. **Co-located, cake p99 is 760 ns -- the best of any configuration on
     either scheduler, and 2.3x better than native's 1731.**
  2. **Split, cake pays ~500 ns more than native on every handoff** (1182-1317
     vs 663-807 p50). Cake's cross-CPU handoff is the defect.

Unpinned cake gets the best p50 of all (654 ns) because it co-locates most of
the time; the ~1.5% it splits land at 1.2-1.4 us, which IS the second mode.
**So the mode is the split fraction, and G8's mechanism was right while its
trigger was dead.** Forcing co-location takes cake p99 1548 -> 760 ns: a 2.0x
win that also beats native's best achievable p99 (1096) by 30%.

**G9.2 OPEN — re-land tier-1 on a trigger that fires.** Proposed gate, every
term state-only and none of them `WAKE_SYNC`:
  - `WAKE_SYNC ABSENT` (futex wakes qualify; pipe still takes today's redirect
    and keeps its +23.1%, which is what P2 was protecting);
  - target's vtime DSQ EMPTY (a load discriminator: true for a 2-thread
    handoff, false under `stress-ng-futex` t32/t64, so the +54% futex row
    should not move).

**Falsifiable predictions.**
  P1 mutex p99 1.55 -> ~0.8 us, and the 1.4-1.7 us mode drains.
  P2 pipe UNCHANGED at +23% -- the gate excludes it by construction, so any
     pipe movement means the WAKE_SYNC-absence term is not doing what it says.
  P3 stress-ng-futex UNCHANGED at +54% (queues are never empty there).
  P4 schbench-light must not regress.

**Method.** Read the histogram, not the percentile: the endpoint is "does the
1.4-1.7 us mode drain", which p99 alone cannot show. Tooling in
`bench/handoff_shape.c` (histogram) and `bench/floor_ladder.c`
(pinned ladder).

**G9.2 RESULT — MECHANISM CONFIRMED, TRIGGER FALSIFIED. Built `e3b6d7961`,
reverted `285f6330f` (2026-07-29).**

**P1 passed, beyond its own prediction.** mutex-handoff shape, interleaved vs
native, matched load, both reps identical to 1 ns:

| | p50 | p90 | p95 | **p99** | p999 |
|---|---|---|---|---|---|
| cake baseline `5c813a004` | 0.654 | 0.721 | 0.740 | **1.442** | 1.702 |
| **cake G9.2** | **0.596** | **0.606** | **0.615** | **0.625** | **1.192** |
| native (same run) | 0.683 | 0.740 | 0.750 | 0.818-1.105 | 1.923 |

**The mode DRAINED.** p50-to-p99 spread collapsed from 788 ns to **29 ns** --
the distribution became a spike -- and cake beat native at EVERY percentile,
including the p99 that was the entire "-47% architectural loss" of G7.0.
**P2 passed too**: pipe 0.623-0.675 vs native 0.727-0.737 usecs/op.

**P3 FIRED, and it is disqualifying.** Cake-vs-cake, interleaved:

| rep | baseline `5c813a004` | G9.2 `e3b6d7961` |
|---|---|---|
| 1 | 2,235,836 futex bogo-ops/s | **13,909** |
| 2 | 2,247,897 | **22,982** |

**-99.4%, a ~130x collapse**, attributable to G9.2 alone.

**THE LESSON, PAID TWICE NOW: I checked the gate's SEMANTICS and not its
FIRING RATE.** The registered claim was "empty queues are a LOAD
discriminator: true for a two-thread handoff, false under stress-ng-futex
t32/t64". That is simply wrong for 8 threads on 16 CPUs, where the per-CPU
vtime DSQs are empty most of the time -- so the gate fired on essentially
every futex wake and welded the workload onto its wakers' CPUs. Identical in
kind to the 2026-07-28 signal substitutions (futex -24%), and the memory entry
warning about it was written *before* this was built.

**PER-CPU EMPTINESS CANNOT SEPARATE A SERIAL PAIR FROM PARALLEL WORKERS**, and
the reason is structural rather than a tuning miss: a parallel workload with
fewer threads than CPUs has empty per-CPU queues *by definition*. The two
workloads differ in a GLOBAL quantity -- mutex-handoff has 2 runnable tasks
system-wide, stress-ng-futex has 8+ -- which no per-CPU test can see.

**G9.3 FALSIFIED — a global load signal is still not a seriality signal.**
Built `9b20ede23`, reverted. Gate: `WAKE_SYNC` absent + waker's queues empty +
`cake_system_serial()` (>= 3/4 of CPUs idle, via `scx_bpf_get_idle_cpumask` +
`bpf_cpumask_weight`, ordered last so the cheap terms short-circuit it).

| prediction | measured | verdict |
|---|---|---|
| P1 mutex p99 -> ~0.65 us, mode drains | 1.442 -> **1.077 / 1.192** | **PARTIAL** |
| P2 pipe unchanged | 0.8225 -> 0.8412 median (within its 2.1% rep spread) | pass |
| P3 futex unchanged | 2,364,878 -> 2,348,860 (**-0.68%**) | **PASS** |
| P4 schbench-light no regress | p99 **2618 -> 2900, +10.8%, 4/4, ranges disjoint** | **FAIL** |

**P3 is the real success and it should be kept in mind: the global gate fixed
exactly what it was designed to fix.** G9.2 cost futex -99.4%; G9.3 costs
-0.68%. Per-CPU emptiness really was the bug, and idle-count really does
exclude a futex storm.

**But P4 kills it, for a principled reason that generalises.** `schbench -m 1
-t 2` is three threads on sixteen CPUs, so ~13 are idle and the gate FIRES —
yet schbench is not a serial handoff. Its message thread wakes a worker and
KEEPS RUNNING. Co-locating the wakee behind a waker that does not yield is the
same lock-holder-preemption shape that made the PREEMPT variant wrong, arrived
at from the other direction.

**THE LESSON, now paid three times: every state-only proxy tried measures
LOAD, and seriality is not a load property.** Queue emptiness (G9.2) and idle
count (G9.3) both answer "is the machine busy". The property that actually
separates `mutex_handoff` from `schbench` and `stress-ng-futex` is whether
**the waker blocks immediately after waking** — a TEMPORAL property of the
waker, invisible to any instantaneous snapshot of system state.

**G9.4 BUILT AND KEPT (2026-07-29, `0cd66a850`) — the repair worked, partially.**
A per-CPU saturating confidence counter: `select_cpu` marks WOKE on the waker's
CPU, `ops.stopping` asks "did this occupant wake someone and then BLOCK
quickly" (`used < HOME_PREEMPT_YOUNG_NS`, no clock read -- `used` and
`runnable` are both already in hand) and advances 0..3. Gate fires at conf==3.

**Firing rate measured BEFORE the policy was wired** (`94a74a643`), which is
the rule three dead gates paid for:

| | conf>=1 | conf>=2 | **conf>=3** |
|---|---|---|---|
| mutex_handoff | 99.88% | 99.83% | **99.81%** |
| schbench -m1 -t2 | 26.10% | 17.46% | **13.82%** |
| stress-ng futex | 1.64% | 0.14% | **0.03%** |

Separation mutex:futex at conf>=3 is **3327x**. And because futex reads 0.03%
on the bit alone, `cake_system_serial()` (three kfuncs: get_idle_cpumask +
cpumask_weight + put_idle_cpumask) was **DELETED** — the gate is now one load
and a compare against a line the CPU already owns. `select_cpu` 189 -> 156.

**LEDGER, interleaved, 2 reps, medians:**

| metric | baseline | G9.3 | **G9.4** | G9.4 vs baseline |
|---|---|---|---|---|
| mutex p50 | 0.663 | 0.6055 | **0.601 us** | **−62 ns (−9.4%)** |
| mutex p95 | 1.4955 | 1.000 | **0.9085 us** | **−587 ns (−39%)** |
| mutex p99 | 1.6875 | 1.322 | **1.245 us** | **−443 ns (−26%)** |
| schbench req p99 | 2620 | 2920 | **2820 us** | +200 (+7.6%) |
| schbench rps | 855.2 | 845.6 | **850.2** | −0.58% |
| futex bogo-ops/s | 2.372M | 2.365M | **2.353M** | −0.80% |

**G9.4 beats G9.3 on every single metric** — mutex better, schbench recovered
by a third on request p99 and by half on rps, futex within noise of both.

**The repair is PARTIAL and the residue is exactly the firing rate.** schbench
still trips the gate 13.82% of the time, and still costs +7.6% request p99 /
−0.58% rps. **Next tightening, and it is a constant, not a redesign:** the
`used < HOME_PREEMPT_YOUNG_NS` test is loose at 93.75 us when a real
mutex_handoff quantum is ~600 ns. Dropping the threshold toward ~6 us
(`SLICE_NS/512`) should hold mutex at ~99.8% while cutting schbench's 13.82%
hard. Census the firing rate again before wiring it.

**G9.4 SUPERSEDES: a per-waker learned handoff bit.** The only signal left that is
not a load proxy: mark a task as a handoff partner when it BLOCKS within a
short window of having woken someone, observed in `ops.stopping` (which
already knows `runnable`, i.e. whether the task is blocking or being
preempted). Co-locate only when the waker carries that bit.
  - It is per-TASK, so it survives queued wakeups (the iron-rule-6 problem
    that degraded the old converged-pair test).
  - It is temporal, so it can distinguish schbench's message thread (wakes,
    keeps running) from mutex_handoff's partner (wakes, blocks immediately).
  - Cost: one bit and one timestamp comparison per stop, on a line the task's
    own CPU already owns.
  - **Firing-rate check FIRST, before building**: instrument the bit and count
    how often it would be set for mutex_handoff vs schbench vs
    stress-ng-futex. Three gates have now died on firing rate; measure it
    before writing the policy, not after.

**G9.4 RESULT — BUILT, MEASURED, FAILS GAME-FIRST (2026-07-29).** The learned
bit shipped (`0cd66a850`) and the firing-rate check was done first, as required.
Benchmarks: mutex-handoff **+9.01%** CI[+6.42,+11.61], schbench-light **−9.40%**
CI[−9.64,−9.16]. Games: Helldivers 2 war-table static scene, ABCCBA, matched
load (gpu_pct 97.97/98.00/97.93) — 0.1% low **104.15** vs 1.1.3 150.28 and
native 141.84, and **the only arm in twelve runs to emit a frame over 2× median**
(0.136% vs exactly 0.000% for both baselines, all eight of their runs).

**Where G9.4's own design note was wrong.** It claimed the per-task bit "is
per-TASK, so it survives queued wakeups". **It is not per-task** — it lives in
`cake.run[wc]`, indexed by CPU, because `SCX_OPS_ALLOW_QUEUED_WAKEUP` makes the
waker unidentifiable in `select_cpu`. So the "serial" verdict is written by
whichever task last ran on that CPU and consumed by an unrelated wake. With
HD2's 92 threads at 70–83K migrations/thread/30s, that is a lottery. Iron rule 6
was not escaped, only relocated.

**The actual defect is simpler and was NOT in the design note.**
`scx_bpf_dsq_nr_queued()` counts tasks WAITING, never the one EXECUTING, so a
CPU busy mid-slice reads as empty. G9.4 also **deleted `cake_system_serial()`**
(G9.3's ≥3/4-idle census) as "cheaper AND sharper" — it was the gate's only
load-sensitive term. HD2 at 36% CPU leaves ~10/16 idle against a threshold of
12, so G9.3 would have declined where G9.4 fires. **Already known on futex:**
`cake_wake_preempt_compute` exists because *"1.4% of global wakes wait out full
occupant slices — latencies quantized at exactly the two slice lengths"*, and
the co-location path `return`s before both preempt helpers.

**G9.5 FALSIFIED — the hitch is NOT one slice (2026-07-29).** Registered
prediction: if the wakee queues behind a running task's remainder, hitch size
scales with `SLICE_NS`. At 1 ms: Δp99 3.011 → **1.479** (predicted ~1.004),
p99.9−median 4.299 → **3.093** (predicted ~1.433), max−median 6.188 → **3.929**
(predicted ~2.063). In slice units the excess GREW (1.43× → 3.09×). Fitting
`excess = a + b·SLICE`: a slice-independent residue of **0.71–2.80 ms** remains.
**~Half the hitch is slice-scaled queueing; half is something else** — most
likely chain serialisation, since the render path is five futex hops with
200–415 µs bursts. Reverted; the 07-04 U-curve minimum stands. Do not re-derive
the "3.011 ≈ 3.000 ms" coincidence as evidence; it was over-read.

**G9.6 OPEN — the liveness term (`1d5ffd205`, tip `41c277e24`).** Co-locate only
when there is no live SCX occupant, or the occupant is inside the validated
young-curr window. Reuses `cake_occupant_live()` + `HOME_PREEMPT_YOUNG_NS`; no
new constant, no preempt added. Zero spills preserved (TOTAL 0, 1091→1113 insns).

| workload | G9.4 | **G9.6** | migrations A→B |
|---|---|---|---|
| mutex-handoff | +9.01% | **+6.29%** CI[+3.22,+9.36] | 6608→3222 (−51%) |
| schbench-light | −9.40% | **−2.66%** CI[−2.95,−2.36] | 2520→6602 (+162%) |

70% of the win kept, 72% of the regression removed, and the migration counters
move OPPOSITE ways on the two workloads — the term keeps the bet where the
occupant yields and refuses it where it does not.

**G9.7 — REGISTERED 2026-07-30, maintainer chose SPLIT over revert/hunt.**

*Residual this attacks:* a young `main` mid-frame still qualifies, and schbench's
migrations went to 2.7× the *gate-less baseline*, so surviving co-locations
concentrate load and provoke steals.

**HYPOTHESIS.** `HOME_PREEMPT_YOUNG_NS` is an ABSOLUTE threshold standing in for
a RELATIVE question. It conflates *about to block* (mutex-handoff occupant, whole
burst ≈ 1 µs) with *just started a long run* (HD2 `main`, 208 µs burst, young for
its first 45%). Replace "is the occupant young?" with **"is the occupant within
one handoff of the end of its OWN typical burst?"** — S1's storage-free estimator
`sum_exec_runtime >> log2(nvcsw)` applied to `curr`, compared against the
existing `cake_handoff_max_ns` rodata. Predicts the gate keeps firing for a
serial pair and stops firing mid-frame, with **no new constant** and one fewer
call (the current gate calls `cake_occupant_live`, whose scaled vtime it never
reads).

**STEPS (budget 4; re-diagnose at 8).**
1. Rewrite `cake_handoff_yields` standalone: drop the unused vtime scaling, test
   `remaining = burst − ran` against `cake_handoff_max_ns`.
2. ATTRIBUTION, free, no game, no maintainer: firing-rate census of the new gate
   on mutex-handoff / schbench-light / stress-ng-futex, 15 s each.
3. SCREEN: HD2 war-table fast config (15 s + 10 s settle, 3-arm ABCCBA, 2.5 min).
4. SCORE: full ledger vs the gate-less baseline `5c813a004` if the screen holds.

**ENDPOINT.** HD2 0.1% low recovers toward the gate-less **148.8** *while*
mutex-handoff keeps its win over `5c813a004`. Both halves, or it is not a split.

**KILL CONDITIONS, pre-registered.**
- Census shows mutex-handoff admit rate collapsing (< 50%, vs G9.6's 99.99%) →
  the discriminator ate the win. **Retune the threshold, do not revert** — the
  census tells you which direction, and that is step 1 of the repair.
- Build/verify failure, a stall, or a watchdog kill.
- `cake_select_cpu` spills regress materially on the per-function census.
- A flat interim read at step 2 is **NOT** a kill condition — step 2 is
  attribution, and the endpoint is step 3/4.

**G9.8 SPECULATIVE — propagate the chain, not a credit.** S1 already converts
"unused slice fraction" into priority for the YIELDER (`cake_cadence_depth`,
`:619`). It does nothing for the SUCCESSOR, yet the HD2 profile says frame tails
come from *"a handoff in the main↔renderer chain stalling"*. Proposal: a
promoted cadence task that yields inside the handoff quantum passes
`max(own_depth, waker_depth)` — **max, not sum, so it cannot compound around a
producer/consumer cycle** — capped by S1's existing 2-slice bound. Storage: the
spare words in `cake_run_slot` (128 B carrying three u64s), so no new cache
line. NOTE the framing constraint: the unused slice is NOT a transferable
credit (it was permission, never an allocation); S1 works because it is a
CLASSIFIER, not a currency. Keep that or it becomes a fairness leak.

## The floor, measured (2026-07-28)

For any future "how fast can this go" question, the ladder is now on record:

  - **~3 ns** SMT siblings, **~13 ns** cross-core -- pure cache-line transfer,
    both threads already running, no kernel. (28.8 / 38.5 ns measured, minus
    2x12.7 ns of `rdtscp` instrument.) This is the floor, and it is reachable
    ONLY if the waiter never blocks.
  - **~606 ns** is the price of going to sleep (644 futex vs 38 spin) -- two
    syscalls, `try_to_wake_up`, `schedule()`, `switch_to`, both return paths.
    Cake's BPF callbacks are only ~100-200 ns of that.
  - Therefore **sub-500 ns p99 is NOT reachable by any change to cake's BPF
    code**: a scheduler costing literally zero still lands ~450-550 ns p50.
    Moving it needs the app to spin, or a directed-switch kfunc
    (FUTEX_SWAP / `yield_to` shape) that skips `try_to_wake_up` and
    `pick_next_task` -- a kernel patch, which standing directions put in scope
    at exactly this kind of wall.
  - The instrument itself costs 39 ns per measured interval (2x `clock_gettime`
    vDSO at 19.6 ns), so `mutex_handoff` cannot report below ~40 ns regardless.

## §G10 — GAMES-ONLY CAKE (registered 2026-07-30, maintainer direction)

**Maintainer direction:** cake is steered to beat every other scheduler on GAMING.
Games first, benchmarks second. Build everything that helps frame delivery; audit out
what was built for benchmarks and hurts or does nothing for games.

**HYPOTHESIS.** Cake's decision structure is tuned for 2-thread serial handoff — the
shape of `mutex-handoff` (+44.24%) and `perf-sched-pipe` (+5.97%) — and that shape is
the OPPOSITE regime from a 5-stage render chain on a machine with 10 idle CPUs. The
corpus measured the consequence: the one chain-shaped benchmark in the suite,
`schbench-light`, moved WITH the game every time the gates moved (G9.4 −9.40%, game
catastrophic), while the two handoff pairs went up. **Re-aiming the same decision
points at chain shape should recover the game without a rewrite.**

**The audit — filter is "serves benchmarks AND hurts or is neutral for games."**
Evidence column is the 2026-07-30 firing-rate census (`dba25375c`) unless noted.

| # | mechanism | evidence | disposition |
|---|---|---|---|
| 1 | co-location family (G9.2–G9.7) | game −21 pts of native-relative; serves mutex p99 only | gate on idle capacity |
| 2 | `cake_system_serial()` DELETED by G9.4 | HD2 62% idle vs its 75% threshold → would have DECLINED | **RESTORE** |
| 3 | wake-queue hysteresis 1–2 `SLICE_NS` | 3 ms / 6 ms of vtime margin on a **5.56 ms frame** | re-denominate |
| 4 | `PEER_WAKE_HYSTERESIS` arm | fires 2.94% schbench / 0.19% futex — near-dead | collapse to DEEP |
| 5 | pinned-wake preempt margin | **0% on all three workloads** | dead branch |
| 6 | `WAKE_STARVE_WALL_NS` | **0 of 11,980** evaluations, costs a clock read each | make cheap, keep |
| 7 | neighbour probe (depth 3) | 4.8% effective; 91.6% of work on the miss path | game-neutral (only runs when NO CPU is idle) |
| 8 | `SLEEPER_LAG_NS` gates | **SATURATED 94–99.95%** — a predicate ~always true | not a predicate |
| 9 | vtime/reciprocal fairness ledger | the only 4 multiplies in the object exist to serve it | chain-class ordering instead |

**Not ripped out** (serves handoff *and* protects the render chain):
`cake_preempt_protect_ns`, `COMPUTE_OCCUPANT_MIN_RAN_NS`. Both are load-bearing on
futex (92%, 93.8%) and both are "do not preempt an occupant that just started" — which
is exactly what a renderer wants too.

**BUILD (the five, ranked by game value / cost).**
1. **Idle-first on every concentration path.** Restore `cake_system_serial()` as a
   precondition on the co-location gate and on handoff convergence (`:548`). The
   `WAKE_SYNC` re-steer at `:505` already picks a real idle CPU and is correct.
2. **Re-key routing on mean burst.** `cake_cadence_depth` already computes
   `sum_exec_runtime >> log2(nvcsw)` and **discards the burst**, keeping only the
   sleeper dose. HD2 separates 5–10×: chain 208–415 µs, workers 37–60 µs.
3. **Chain-class outranks worker-class at insert.** At 34% CPU there is nothing to
   arbitrate; the chain must simply never queue behind a worker.
4. **Per-task slice from own burst.** `SLICE_NS` 3000 µs vs HD2's largest burst 415 µs
   = 7.2×, which is why six slice-derived thresholds are meaningless.
5. **Never preempt a chain-class task.** They run 200–415 µs and yield unaided.

### G10 build notes — the narrative relocated out of the hot path

**§G10.2 — the burst class.** `cake_cadence_depth` always computed a storage-free
mean burst (`sum_exec_runtime >> log2(nvcsw)`) and discarded it, keeping only the
sleeper dose. That value is the chain/worker discriminant, and HD2 separates 5–10×
with nothing between: renderer 415 µs, audio 295 µs, main 208 µs against a
14-thread worker pool at 37–60 µs. Routing was keyed on the bare wakeup bit, so
"wakeups global" spread the workers too — the profile measures those 14 threads
churning **70–83k migrations each per 30 s**. Now a stage goes global (worth
placing; any CPU can take it) and a worker takes the continuation arm (its burst is
too short to pay for a migration). The boundary is denominated in the measured cost
of a handoff, not in slice turns: a worker's burst is a small multiple of what
waking it costs, a stage runs orders above. 64× handoff = 93.7 µs here.

**§G10.3 — chain priority, delivered by routing.** The wake arm's key is
`frontier − SLICE_NS − cadence_depth(p)`; the continuation arm uses the plain
`frontier − SLICE_NS`. Since only stages now take the wake arm, stages get the
priority floor and workers do not. This also corrects an inversion: `cadence_depth`
gives a **bigger** dose to a **shorter** burst, so before G10.2 the 37 µs workers
outranked the 415 µs renderer.

**§G10.4 — the slice is a preemption timer, not a vtime grant.** `ops.stopping`
charges `used = sum_exec_runtime − run[cpu].sum`, so an oversized slice costs its
holder nothing in fairness terms — that part of the old claim is true and verified.
What it does cost is exposure: a flat 3 ms lets a worker that stops yielding hold a
CPU for 3 ms while a render stage waits, on a workload whose whole frame budget is
5.56 ms and whose longest thread runs 415 µs. Grant is now `clamp(2 × burst,
chain_burst, SLICE_NS)`.

**§G10.5 — stages are preempt-immune.** A stage yields unaided within a few hundred
microseconds, so preempting it buys the waker a little latency and costs the frame
the rest of the chain. Tested last in `cake_wake_preempt`, on the arm that would
otherwise kick, so the cheap rejections still cost nothing.

**§G10.6 — the dose inversion on the direct path.** `cake_cadence_depth` returns
`3/4 × (SLICE_NS − burst)`, a dose that grows as burst SHRINKS. §G10.3 closed that
inversion for `ops.enqueue` by gating the wake arm on `cake_is_chain()`, but
`cake_direct_clamp() → cake_wake_vtime() → cake_cadence_depth()` was ungated, so
every direct admission still applied the worker-favouring dose. At `SLICE_NS` = 3 ms
against the HD2 profile, ModulePrefetch (1.4 µs burst) outranked the renderer
(415 µs) by **310 µs** of vtime, and the worker pool (37 µs) by 283 µs — and the
direct/idle path is where a game at ~34% CPU spends nearly every wake. The gate now
lives inside `cake_cadence_depth`, shared by every caller, reusing §G10.2's
`cake_chain_burst_ns` rather than adding a constant. It is a no-op for
`ops.enqueue`'s wake arm, which was already chain-gated.

*Registered risk:* a class gate does not degrade gracefully at its boundary — a 90 µs
and a 95 µs renderer get opposite treatment. The pre-change dose was continuous and
merely inverted; the post-change dose is correctly ordered but CLIFFED. If the screen
regresses, the diagnosis is the cliff and the repair is a monotone dose, not a moved
threshold. *Registered residual:* the dose is still inverted **within** the chain
class — a 94 µs stage outranks a 415 µs renderer by ~236 µs. The gate makes it a
class distinction, which is what §G10.3 designed; it does not make the ranking
monotone.

**Cost of 4 + 5, measured:** TOTAL 1214 → 1444 insns (+230), still **zero spills and
zero fills in every function**, zero `/=` and `%=`. `cake_task_slice` inlines a log2
at four insert sites, which is most of the +230 — the first thing to attack if the
endpoint says the instruction cost is not paid for.

**ENDPOINT.** HD2 appsim, 3 arms (native / 1.1.3 / G10), severe-frame ratio as the
screen and 0.1% low as the score, gated through `app_sim_validate` against the real
war-table captures. Target: G10 ≥ 1.1.3 on both, i.e. ≥95% of native.

**STEP BUDGET 6; re-diagnose at 12.** Measure the ENDPOINT, not each step — several of
these remove a compensating mechanism whose replacement lands later.

**KILL CONDITIONS.**
- `app_sim_validate` says NOT VALID → the sim does not track the real signature; stop
  and capture a real `app_profile` before trusting any G10 number.
- Any step fails to build/verify, stalls, or trips the watchdog.
- A per-function spill regression that is not paid for by a decision.
- **NOT a kill condition:** a benchmark regression. That is the registered, accepted
  price of this direction — report it in the ledger, do not revert on it.

### G11.1 — the burst estimator banded, and exact is CHEAPER

`sum_exec_runtime >> log2(nvcsw)` divides by the largest power of two below the wake
count, so it over-reads a mean by a factor in [1, 2) — and the factor **resets to 1.0
every time `nvcsw` crosses a power of two**. A sawtooth, not an offset: the same thread,
doing the same work, sees its reported burst halve at each doubling.

Four live decisions read it (`cake_is_chain`, `cake_cadence_depth`, `cake_task_slice`,
`cake_handoff_yields`), so the error moved routing, the priority dose, the preemption
timer and the co-location gate. Traced from source, not measured:

| effect | magnitude |
|---|---|
| ordering slip between two identical renderers | **311 µs** of vtime (dose 1938 → 1627 µs) |
| slice grant | up to **2×** too long (60 µs worker: 120 → 240 µs) |
| class flip | HD2's 60 µs worker pool reads 60–120 µs and **straddles** the 93.7 µs boundary — becoming preempt-immune |
| co-location gate | `burst − ran` reads high, so it **declines** more than designed |

The renderer (415 µs) and `main` (208 µs) are far enough above the boundary that the 2×
cannot reclassify them; only the worker pool flips. **vtime accounting was never
affected** — `cake_scale_vtime_add` uses the exact nice-indexed reciprocal table, no
log2 anywhere.

**Replaced by one exact `u64` divide (`cake_burst_ns`), and the object got SMALLER:
1530 → 1219 insns (−20%), still zero spills and zero fills in every function.**
`cake_log2_u64`'s ten branches were being inlined at five sites; one `div` is cheaper
than fifty branches. This is the first thing in the tree to break the "zero `/=`"
invariant, deliberately — see §S.2. The two-step mantissa-reciprocal approximation
built earlier the same day (32-entry table, 0.45% error) is deleted: exact is both
more accurate and less code.

### R.22 — the frame clock takes a MODE, and why cake grew its first BPF map

The selector, not the estimator, is what failed in G11 step 1. Taking the **minimum**
in-band cadence assumed the game's render loop is the fastest steady thing on the
machine; on a real desktop the compositor and other apps hold threads waking faster than
the game, so the minimum tracked them (idle desktop 2320–3820 µs) and never locked
(2320–8509 µs against a true 5556 µs). A minimum is also fragile by construction —
**one thread owns the answer, with no averaging and no second opinion.**

The signal that *is* robust: a game drives many threads off one frame loop, so the frame
period is the cadence the **most** threads agree on. HD2 puts `main`, `renderer`, three
vkd3d threads, the swapchain and the worker pool all on 180 Hz; everything else on the
machine is scattered and agrees with nothing. A mode is a vote, a minimum is a dictator,
and one odd fast thread adds one tally instead of taking over.

Three things keep it cheap:
- **The bucket is one shift.** `period >> 17` = 131 µs buckets across the 2–40 ms band.
  No divide, no log2 (which G11.1 deleted).
- **Each bucket also SUMS.** The published period is `sum / count` for the winning
  bucket, so it is exact — bucket width only has to separate distinct cadences, it does
  not set the resolution.
- **Argmax runs in the loader**, once a second, and clears each bucket as it reads it,
  so a stale frame rate cannot keep winning. Derive in the loader, compare in the BPF.

**This is cake's FIRST BPF map**, and the exception is narrow: a mode needs storage that
a per-CPU scalar cannot provide. It is a `PERCPU_ARRAY` with non-atomic local increments
per the GAME_DIAG law, never BSS-plus-atomic, so no cache line is shared. Hot-path cost
is one map lookup and two adds in `cake_frame_observe`: **21 → 26 insns, still zero
spills.**

### G11.2 — the dead-mechanism sweep, and the three that survived it

Filter: delete what the 2026-07-30 firing-rate census (`dba25375c`) measured to decide
nothing. Four candidates went in; **one came out**, and the three refusals are the more
useful record.

**DELETED — the PEER hysteresis arm.** `cake_dispatch_search` chose between a 1-slice
(DEEP) and 2-slice (PEER) vtime margin on `time_before(wv, frontier − SLEEPER_LAG_NS)`.
The census: 97–99.8% took DEEP; PEER fired 420 times on schbench (2.94%) and **11 times
on futex (0.19%)**. Collapsed to DEEP unconditionally. This deletes
`PEER_WAKE_HYSTERESIS_NS` outright and one of the three uses of the saturated
`SLEEPER_LAG_NS`. **Direction is games-positive:** the test is
`wv + margin < own_vtime`, so the larger PEER margin made the global wake queue *less*
likely to be served — and since §G10.2 routes render stages global, the affected
0.19–2.94% now reach a CPU sooner.

**KEPT — `WAKE_STARVE_WALL_NS`, and the audit's own recommendation was wrong.** It reads
"fires 0 of 11,980, costs a clock read each → make it cheap". The rate arithmetic kills
that: 9416 evaluations in 15 s (schbench) is 628/s, or **0.013 ms of clock reads per
second of wall time — 0.0013% of one core.** Never firing is *correct* for a safety net,
and there is nothing here worth the churn. Cost a mechanism by its RATE, not by the
per-call price.

**KEPT — the pinned-wake preempt margin. "0 fires" may be "0 of 0 tries".** It can only
fire for a pinned USER task (kthread wakes return earlier in `cake_enqueue`), and the
three census workloads plausibly have none. Three benchmarks with no pinned user tasks
prove nothing about a game. **Get a denominator before deleting a service path** —
otherwise this is the `4d5b5f96d` error again, deleting decisions to satisfy a count.

**KEPT — the neighbour probe. The magic is the DEPTH, not the mechanism.** It runs only
when no CPU is idle anywhere, so a game at 34% CPU never reaches it — deleting it helps
games nothing and costs futex its 392 kicks. The principled fix is to order the probe by
topology (`cpu_sibling` is already in rodata) instead of by the next 3 ring ids, which
is a change, not a deletion.

## §R — Design rationale, relocated from source comments (2026-07-30)

Kernel coding-style §8 says comments tell WHAT, not HOW, and *"avoid putting comments
inside a function body"*. cake.bpf.c measured **2.00 comment:code** against EEVDF's 0.63
and a 0.15–0.73 scx peer range. The narratives below were moved out of function bodies
so the hot path reads as code; the source keeps a short WHAT/WHY and a `see §R.n`
pointer. **Nothing is deleted — this is the container change the standard asks for.**

### G11.4 / G11.5 — occupant protection and the slice cap, denominated in a FRAME

Both were `SLICE_NS` divisors, i.e. fractions of a number hand-fitted on one 9800X3D.
How long a waker may be made to wait, and how long anything may hold a CPU, are
**display** questions — so both moved onto the observed frame period.
`FRAME_PREEMPT_PROTECT_SHIFT` is a frame sixteenth at the global-wake floor and
`FRAME_PROBE_PROTECT_SHIFT` a quarter for the speculative neighbour probe, which must be
surer before disturbing another CPU. `FRAME_SLICE_CAP_SHIFT` caps any grant at half a
frame: past that a task is eating the budget of the frame in flight.

Near value-neutral where it was measured (at 180 Hz, 347 µs vs the old 375 and 1389 vs
1500 — both 1.080×, which is arithmetic rather than a discovery since they were the same
divisor family) and **correct where it was silently wrong**: at 240 Hz the old flat 3 ms
cap was nearly three quarters of a frame.

### G12 — the starvation predicate, and why the burst class had to go

`cake_chain_burst_ns` (93,696 ns = `handoff_max << 6` = `SLICE_NS/32` in two disguises)
gated routing, the dose, preempt immunity and the slice floor. The live HD2 capture
proved it measured the wrong **axis**, not merely the wrong value: against its 93.7 µs
boundary the whole critical chain reads renderer 64.5 µs, vkd3d_queue 8.5, vkd3d_fence
2.5, main 1.4 — **not one stage of the render path was recognised as a stage.**

Replaced by a question with no magnitude in it: does this task wait longer than it runs?

```
starved  ⟺  run_delay × nvcsw > sum_exec_runtime × pcount
```

Cross-multiplied so no divide is spent; both sides carry the same pre-scale
(`CAKE_RATIO_SHIFT`), which cancels. Threshold 1.0 is a definition, not a tuning. Zero
storage — every term already lives in `task_struct`.

**Portability:** `sched_info` is gated by `CONFIG_SCHED_INFO`, **not** `CONFIG_SCHEDSTATS`
and **not** the `kernel.sched_schedstats` sysctl — which reads 0 on this host while
`run_delay` still advances (three spinners on one CPU: exec +622 ms each, run_delay
+1377 ms each, exactly the 2/3 wait 3-on-1 implies). PSI/TASKSTATS/SCHEDSTATS all select
`SCHED_INFO`.

**Census (2026-07-31):** fires 1.48% on a game, 19.44% saturated — a 13× span, the
signature of a live discriminator. The wait:run distribution is sharply bimodal (96.4%
bottom bucket, 1.02% saturating the top), so the threshold sits in a sparse middle and
the §G10.6 cliff risk does not apply.

### G13 — the cache-warm home claim

`scx_select_cpu_dfl` prefers a **fully idle core** over a merely-idle `prev_cpu`
(`kernel/sched/ext/idle.c:568` then `:578`, reaching `:618` only after), so under SMT
contention it migrates a task off a warm home whose sibling happens to be busy. Right for
a waiter, wrong for an occupant of its own cache. Claim `prev_cpu` directly when idle,
**before** `select_cpu_dfl` (which reserves what it returns), gated on the task being
served rather than starved. `WAKE_SYNC` excluded: there the *waker's* cache holds the
data.

**Measured:** closed the same-CPU gap from 8.9–16.7 points to 0.9–4.5, and cake beats
native on the wake tail for 4 of 5 render-chain roles.

### G17 — anti-collision: never queue behind an equally served peer

§R.15's home rule is depth-blind on purpose. That is right when the occupant is a worker
whose slice is short, and wrong when it is an equally hot **peer** whose entire remaining
slice must be waited out while other CPUs sit idle. Route those to the global wake queue
instead. Reaching the continuation arm already proves `cake_starved(p)` is false, so only
the occupant needs testing; `PF_IDLE` is excluded explicitly because an idle task has huge
`sum_exec` and no `run_delay` and would otherwise read as a well-served peer.

**Measured, interleaved cake-vs-cake in one run:** wake p99 renderer −17.3%, main −21.7%,
vkd3d_queue −49.2%, vkd3d_fence −14.0%, swapchain +2.2% (overlap). First change in the
campaign to move the renderer in a clean comparison.

**The mechanism story is INCOMPLETE and is deliberately not asserted in the source.** The
registered mechanism endpoint failed: the renderer's "blocked by another renderer thread"
share barely moved (58.1% → 56.9%). The rule lives in `ops.enqueue`, which is 0.14% of a
game's dispatches, so it cannot directly cause a 17% tail move. Working hypothesis:
routing other hot continuations global frees CPUs and shortens the queues the renderer
lands behind. Unproven.

### G20 — a kthread wake spends an idle CPU, not an occupant

`cake_enqueue`'s kthread arm inserts into `SCX_DSQ_LOCAL_ON | tcpu`, and a LOCAL_ON insert
**rescheds the target CPU**. Bounded softirq/workqueue service is therefore bought with a
mid-burst eviction of whatever was running there.

Measured on the retained G17/G18 traces with `bench/wake_maxdecomp.py` (no new capture).
Cake preempts the renderer **22,800** times per 22 s against native's **11,383**, at a
median of **50 µs** into its run where native lets it run **216 µs**. Of the short (<200 µs)
preempted runs, **73.7% hand the CPU to `DP-2`** — the display-connector kthread on this
240 Hz VRR panel — **11,518 times under cake against 431 under native, 27×**. That is the
largest behavioural gap found between the two schedulers on this game.

An idle CPU serves the kthread just as promptly and evicts nobody, so try one first and
fall back to the assigned CPU only when the machine is full. One `scx_bpf_pick_idle_cpu`,
the same kfunc with the same arguments already called later in the same callback:
`cake_enqueue` 201 → 211 insns, TOTAL 1433 → 1443, zero spills and zero fills.

**Not established, and neither may be skipped.** Whether those evictions cost *frames* —
cake already wins the >200 µs delay rate 3–5× over native while tying 0.1% low, so the
link from stall rate to frame tail is unproven. And whether serving `DP-2` elsewhere costs
*display* latency: it is vblank-adjacent work, and moving it is not free by assumption.

### G21 — CONFIRMED 2/2: the GPU interrupt owns a CPU, and cake was placing game threads on it

**Registered and measured 2026-08-02. Parent: none — a new trunk, and the first experiment
in the campaign aimed at the QUIET regime.**

**VERDICT — interleaved ABBA on live Helldivers 2, quiet host, 22 s arms, zero stalls,
game confirmed rendering in all four arms (~40k renderer wakes each):**

| metric | A1 | A2 | B1 (G21) | B2 (G21) | delta |
|---|---|---|---|---|---|
| `main` mean wake | 0.79 | 0.79 | **0.47** | **0.48** | **−39.9%** |
| `renderer` mean wake | 0.91 | 0.83 | **0.66** | **0.64** | **−25.3%** |
| `main` p99 | 6.54 | 5.91 | **5.54** | **5.75** | −9.3% |
| `renderer` CPU 13 share | 3.9% | 4.2% | **0.1%** | **0.1%** | 40× |
| `main` wakes served | 357,541 | 358,354 | 363,050 | 363,422 | **+1.5%** |

Zero overlap between arms on both roles. **The mean moves far more than p99 because the
affected wakes are rarer than 1-in-100 for `main` — they sit ABOVE p99 and dominate the
average.** Score this on the mean; p99 understates it by 4×.

**All three kill conditions passed.** Share fell; wakes served ROSE, so work conservation
held; and no CPU inherited a >2× penalty — the worst is **CPU 5 at 1.30×**, everything
else 1.0-1.16×, the expected cost of spreading 2,935 wakes over fifteen CPUs.

**Cost: `main` migrations +9.7%, renderer preemptions +9.4%.** Swapping off the sink is a
migration, so this is the mechanism working, not a side effect — but it is a real charge
and the next experiment on this branch should know it is there.

**NEW, and the obvious child: the SMT sibling of an interrupt sink is itself degraded.**
CPU 5 shares a core with CPU 13 and carries a 1.30× mean penalty while its own load FELL
(19,551 → 13,012). Deprioritising the sibling as well is untested and is the natural G21.1.

`/proc/irq/115/effective_affinity_list` is `13`. The nvidia interrupt has fired
**1,266,319,129** times on CPU 13 and **zero** times on the other fifteen. A latency
thread scheduled there contends with the GPU interrupt stream, and both schedulers place
threads there at close to the fair 1-in-16 rate.

Measured per-CPU wake→run delay, `bench/percpu_wake.py`, no new capture on the cake side
(retained `G20R-20a`, cake, 8-spinner load):

| role | CPUs 0-12,14,15 mean | CPU 13 mean | CPU 13 p99 | CPU 13 share |
|---|---|---|---|---|
| `main` | 1.35-1.75 µs | **40.33 µs** | 520 µs | 1.6% |
| `renderer` | 1.87-3.27 µs | **6.02 µs** | 133 µs | **6.4%** |

Removing CPU 13's excess would cut **28.5%** of `main`'s and **9.8%** of the renderer's
total wake-delay budget. On a fresh quiet-machine native capture the same CPU costs `main`
a **116×** within-CPU penalty (0.25 → 137.23 µs mean).

**An idle-depth theory was falsified on the way here and must not be retried.** The
quiet-machine data first showed delay rising monotonically with how long the target CPU
had been idle — 0.27 µs under 50 µs asleep, 38.66 µs beyond 200 µs, which reads exactly
like C-state exit. It is not: this host has **no cpuidle driver bound**
(`current_driver = none`), and the within-CPU control shows 15 of 16 CPUs at a ratio of
**1.0-1.6×**. The entire effect was CPU 13, which long-idle wakes disproportionately land
on. **Idle duration is a confounder for IRQ residency, not a lever.**

**Steps.** (1) Rust loader samples `/proc/interrupts` over a window and marks any CPU
carrying ≥4× its fair share as IRQ-hot, published as a `const volatile u64` mask so libbpf
freezes it to an immediate. (2) `cake_select_cpu` declines to re-home a task onto a hot
`prev_cpu`. (3) When `select_cpu_dfl` returns a hot idle CPU, try one `scx_bpf_pick_idle_cpu`
for a cool alternative, keeping the hot CPU when none exists.

**Endpoint:** `main` and `renderer` wake p99, interleaved cake-vs-cake, plus the CPU 13
share falling from 6.4% toward zero while total dispatch count holds.

**Kill conditions:** CPU 13 share does not fall (mechanism never fires); total wakes served
drops (work conservation lost — the −19% law from the enqueue-diversion falsification);
any other CPU inherits a >2× penalty (the harm moved rather than went away).

**Step budget: 3 commits.** Re-diagnose at 6.

**Not established.** That wake latency converts to frames — the whole campaign's standing
caveat. And whether an IRQ-hot CPU is bad for *throughput* work too, or only for latency
threads; the change deliberately preserves work conservation so the saturated case is
untouched.

### G23 — BUILT + SMOKE-PASSED 2026-08-02, endpoint pending: every sink, not just the loudest, and no abandoned claims

**Status: P1 CONFIRMED at live smoke (probe flagged {5, 13} with device names, verifier
accepted, zero stalls). P2-P4 await the HD2 ABBA + bench screen. Findings, dispositions
and resume steps: `docs/REVIEW_G21_G23_2026-08-02.md`.**

**Parent: G21 (mechanism confirmed 2/2) + G22 investigation #1 (three sinks, not one).**

**Hypothesis, two clauses.** (a) Detecting sinks per IRQ **line** — rate plus affinity
concentration — finds every pinned interrupt sink independent of relative loudness. G21's
4× fair-share test requires a CPU to carry ≥25% of *total* interrupt volume on a 16-CPU
host, so only the loudest sink can ever qualify: G22 identified three 100%-affine sinks
(nvidia→13, xhci/USB→5, enp10s0→2) and measured CPU 5 as the worst CPU for
`Window & Input` (0.91 µs mean / 15.0 µs p99 vs ~0.50 / 2-3 elsewhere), yet CPU 5 sits at
1.16× fair share (live replication, 2026-08-02) and can never be flagged. (b) Passing the
non-sink mask to `scx_bpf_select_cpu_and` (BTF-verified present on this kernel; the kernel
ANDs it with `p->cpus_ptr` internally) removes sinks from first-choice placement *inside*
dfl's own ranking — deleting the G21 swap block's abandoned idle claim (dfl test-and-clears
what it returns; the bit repairs only on the CPU's next idle re-pick) and its cool-is-hot
sub-leak, rather than patching around them. Same bug class as the 2026-07-09 RT-dodge
idle-claim leak.

**Steps.** (1) Loader: per-line detector — a line whose delta rate is ≥1 kHz AND ≥95%
concentrated on one CPU marks that CPU a sink; columns mapped by header CPU labels
(fixes offline-CPU misattribution); ≥half-machine guard kept. (2) `cake_init` builds a
`nonsink` bpf_cpumask from `cpu_irq_hot`. (3) `cake_select_cpu`: try
`scx_bpf_select_cpu_and(p, prev, wake_flags, nonsink, 0)`, fall back to plain dfl on
negative return — sinks stay last resort, so the saturated regime is untouched (the −19%
enqueue-diversion law). The R.6 sync re-pick goes through `select_cpu_and` with the SYNC
flag stripped, which also upgrades it from a flat idle scan to dfl's topology ranking.
The G21 swap block is deleted; the hot-`prev_cpu` gate stays (it tests before claiming
and leaks nothing).

**Predictions, registered before measurement.**
- P1: at attach with the mouse live the probe flags {5, 13}; CPU 2 only under network
  traffic (live rates 2026-08-02: xhci 2.1 k/s 100%-on-5, nvidia 16.4 k/s 100%-on-13,
  enp quiet).
- P2: `Window & Input` mean wake −4 to −8%, p99 −30% or better — its CPU-5 wakes drop
  from ~15 µs p99 to the 2-3 µs everywhere-else level, at a ~1-in-16 share.
- P3: `main` / `renderer` ≈ neutral (G21 already covers CPU 13); a move >±5% on either
  falsifies "the sink was the whole quiet-regime mechanism".
- P4: throughput bench screen neutral: under saturation `select_cpu_and` returns
  negative and the dfl fallback reproduces today's behavior.

**Endpoint:** HD2 interleaved ABBA, fast config — score mean wake for `Window & Input`,
`main`, `renderer`; screen severe-frame ratio. One `--blocks 2` bench screen for
throughput regressions before the game arms.

**Kill conditions:** total wakes served drops (work conservation lost); any CPU inherits
a >2× penalty (harm moved — if it fires, the next commit ranks sinks worst-first, not a
revert); stall/watchdog; `Window & Input` CPU-5 share fails to fall (mechanism never
fired).

**Step budget: 3 commits. Re-diagnose at 6.**

Note: G21.1 (SMT sibling) was WITHDRAWN by G22 — CPU 5 is its own USB sink and the two
effects are inseparable in that data. G23 flags CPU 5 for its own interrupt stream, which
subsumes the actionable half of G21.1.

### G24 — REGISTERED 2026-08-02: expense-vs-benefit census of every unaudited system, on the sim

**Maintainer direction: audit the systems the game campaign never touched — expense
versus what they bring — and remove what is expensive and does nothing for games.**

**Hypothesis.** The five never-audited paths (dispatch search + steal ring, qmark
maintenance, R.6 re-pick, sibling kick, run-slot learning) plus the three unexecuted
§G10 dispositions (pinned-wake margin at 0%, SLEEPER_LAG at 94-99.95%, G20's null
kthread arm) can be priced by CENSUS × path-cost under a game-shaped sim load —
`helldivers2-mission-fitted.conf`, which needs no game time and no human. The sim is
DISQUALIFIED for latency (2 µs vs 265 µs) and is not asked for any: firing rates and
callback expense only. "Never fires" is removal-grade evidence (three gates died on
rate, never on value); "fires, benefit unproven" routes to a game A/B list instead.

**Steps.** (1) DIAGNOSTIC census commit modeled on `dba25375c` (PERCPU_ARRAY,
non-atomic inc, dump at detach), ~24 counters over select_cpu arms / enqueue arms /
dispatch ring / wake_notify. (2) Attach census build, run mission-fitted sim, collect
rates; repeat on the menu spec for a quiet-regime contrast. (3) Expense side on the
CLEAN build: per-callback ns (cake-bpfstats) + per-arm insn attribution. (4) Verdict
table: ns/sec = rate × path cost vs best existing benefit evidence. (5) Revert the
census commit; deletions (if any) are separate commits with the ledger attached.

**Endpoint:** the expense-vs-benefit table in a committed doc, each row carrying its
evidence class and a disposition (delete / keep / game-A/B).

**Kill conditions:** census build fails verify or stalls (diag commit is discarded);
sim validate fails against its fitted spec (rates would be shape-garbage).

**Step budget: 3 commits (register, census, doc+revert). Re-diagnose at 6.**

### R.23 — the two guards `wake_maxdecomp.py` cannot run without

perf arms its per-CPU ring buffers **sequentially**, so early in a trace a CPU can be
running tasks that are not being recorded, and any wait spanning that hole measures
arbitrarily long. This produced G18's headline 4.78 ms "residual": it sat 5 ms into a
22-second capture and CPU 011 had no recorded events across 4.08 ms of it. Events are
therefore dropped unless they begin after a warmup **and** after the run CPU's own first
recorded switch. The companion rule is to score a **rate**, never a max or a count at one
threshold — the max is a one-sample statistic and inverted the cake-vs-native ordering.

### G14 / G15 / G16 — FALSIFIED, do not retry without new evidence

Three attempts on the renderer's tail, all built, measured and reverted.

| | mechanism | why it died |
|---|---|---|
| **G14** | add a preempt to the continuation arm | fired (locality −2 to −4 points on every role) and never reached the renderer; tail unchanged |
| **G15** | denominate that preempt's guard in the waker's own cycle | correct fix, but it also stripped the neighbour probe's conservative window and cost `vkd3d_fence` and `vkd3d-swapchain` their wins — net worse than G13 |
| **G16** | kick HOME when home is idle | **null**, which proved `cake_enqueue`'s standing claim that the kernel's `activate→wakeup_preempt` rescheds an idle owner. There is no missing notification |

**What G14's diagnostic established and is worth keeping:** over 22,362 hot-waker preempt
attempts the *fairness* arm rejected **18 (0.08%)** while "occupant too young" rejected
**12,827 (57.4%)**. Fairness is not what blocks a game's render thread.

### R.1 — Serial-handoff co-location (the G9.2→G9.6 gate, `cake_select_cpu`)

Co-locating a genuine handoff pair took mutex-handoff p99 **1.442 → 0.625 µs** with the
1.4–1.7 µs second mode fully drained (p50→p99 spread **788 ns → 29 ns**), beating native
at every percentile.

Gate history, each falsified by the next:
- **G9.2** gated on per-CPU queue emptiness. Reverted for **futex −99.4%**: a workload
  with fewer threads than CPUs has empty per-CPU queues *by definition*, so it fired for
  parallel workers too.
- **G9.3** gated on a ≥3/4-of-CPUs-idle census — a global load discriminator.
- **G9.4** replaced the census with a learned per-CPU handoff bit and deleted it,
  removing the gate's only load-sensitive term. This is the defect that cost the games.
- **G9.6** restores it as an occupant test (`cake_handoff_yields`).

**NO PREEMPT is deliberate.** glibc's `pthread_cond_signal` fires while the waker still
HOLDS the mutex, so preempting it is lock-holder preemption: the wakee runs, finds the
mutex held, and blocks a second time — the very mode this drains. The waker is about to
sleep anyway.

**That last assumption is exactly what fails for a GAME.** glibc's cond_signal waker
does sleep; Helldivers 2's `main` wakes `renderer` and keeps working, so co-locating
strands the wakee behind a live occupant. `cake_handoff_yields()` is the only term in
the gate that looks at what is RUNNING rather than what is QUEUED.

**WAKE_SYNC absent keeps pipe out by construction**: pipe uses
`wake_up_interruptible_sync_poll` and DOES set the flag, and it wants spreading
(**+23.1%**), while `futex_wake` sets no `WF_SYNC` at all
(`try_to_wake_up(p, TASK_NORMAL, 0)`, core.c:4545), so a real handoff qualifies.

**Order matters:** the gate must run BEFORE `scx_bpf_select_cpu_dfl()`, because dfl
RESERVES the idle CPU it returns — testing after it and ignoring the answer leaks the
claim (the idle-claim leak of 2026-07-09). Term order within the gate is cost order.

### R.2 — Empty-home carve-out (`cake_enqueue_wake`)

EEVDF's futex locality is prev-CPU stickiness PLUS floor-less eligibility preemption.
Measured 2026-07-02, **each half alone loses**: wake-global = flow without locality;
home routing alone = locality without flow.

An empty home is CLAIMED when any of:
- **curr is idle- or higher-class-owned (vtime 0)** — free or imminently free. Sending
  this case global was the pipe leak: a wake racing its partner's idle transition took a
  WAKE_DSQ insert + pick_idle + kick detour on every message.
- **the wake is LOCAL** (this CPU waking onto itself; curr is the waker, about to block).
  The converged handoff pair. Its raw-vtime sleeper test flaps because the clamp writes
  the wakee back to within one slice of the frontier every insert — routing that flap
  globally collapsed **futex 4.8M → 0.98M** (2026-07-04).
- **the wakee is a sleeper** (raw vtime > half a slice behind the frontier — the handoff
  shape, it accrues nothing between wakes), **or curr is a valve** (live vtime a full
  slice behind — a low-duty dispatcher about to block; the wakee just waits it out).

**When none holds**, wakee and curr are frontier-running compute peers, and homing the
wakee builds a trap: cake has no periodic balancer, so two workers sharing one CPU
mutually preempt forever while other CPUs run one worker each — every affected schbench
request stretched exactly **2×** (p99 9072 µs == 2 × p50 4648 µs vs native 5832 µs,
2026-07-04). Such a peer still QUEUES at the empty home for prev-CPU warmth, but without
an explicit claim it earns no preempt.

### R.3 — Own-first dispatch and the one-slice margin (`cake_dispatch_search`)

Snapshots are point-in-time; a stale read only mis-orders the two consume attempts,
both of which still happen. NULL from an empty queue keeps own-first — the second try
still drains the wake queue, and a blocking CPU picking up the earliest-vtime waker IS
the handoff fast path.

**The one-slice margin is hysteresis, not fairness slack.** Sleeper-clamped wake heads
sit *slightly* earlier than own heads almost always, so a plain earliest-vtime rule sent
every CPU to the global queue first — `move_to_local` takes the DSQ lock and the
wake-storm serialisation came straight back (**futex −49%**, 2026-07-02). Own-first
within the margin keeps the uncontended fast path; a genuinely stranded wake head
freezes while own heads advance past it by more than a slice within a few quanta, after
which every CPU prefers it until it drains. The starvation seal stays structural.

**The head peek republishes qmark with ONE conditional store**, not the former
unconditional clear-then-remark. Every stealer's ring walk reads that line, so on the
common busy dispatch (queue non-empty, mark already set) writing 0 then 1 cost two RFOs
and invalidated every remote copy twice to publish a value that was already there.
Clearing first protected nothing: enqueue and dispatch for the same CPU both hold that
CPU's rq lock, so there is no concurrent insert to hide.

### R.4 — Ordered direct admission (`cake_select_cpu`)

`WAKE_SYNC` is the kernel's explicit handoff hint; qmark is only a cheap indication that
the finalised target's vtime DSQ *may* hold an older claim. Confirm with one lockless
head snapshot plus the sleeper clamp before deciding whether terminal direct dispatch
would jump that claim. Returning without an insert deliberately falls through to
ordinary enqueue; stale qmarks and ties keep today's fast path. The snapshot is
advisory, never a reservation.

**Why it calls `cake_wake_vtime()` instead of re-deriving the clamp inline:** the inline
copy floored at `frontier - SLICE_NS`, but the key this path actually assigns nine lines
later (`cake_direct_clamp` → `cake_wake_vtime`) floors at
`frontier - SLICE_NS - cake_cadence_depth(p)`, which is LOWER. The guard therefore
tested a higher vtime than the one it was about to write, and declined direct dispatch
for short-burst cadence tasks whose real key was older than the head and would never
have jumped it. **Two spellings of "the sleeper clamp" drift; one helper cannot.**

### R.5 — The wake bit as a literal, not the caller's `enq_flags`

Threading the full word through made it span every routing call. VERIFIED against ext.c
(2026-07-27), not assumed: this is a PRIQ insert into a CUSTOM DSQ and the kernel reads
none of the caller's positional bits on that path.

- `SCX_ENQ_HEAD` / `SCX_ENQ_PREEMPT` — consulted only in the FIFO else-branch
  (ext.c:1587); a PRIQ insert takes position from the rbtree, which is the point of a
  vtime queue.
- `SCX_ENQ_NESTED` (bit 58) / `SCX_ENQ_DSQ_PRIQ` (bit 57) — internal bits the kernel
  sets at its own dispatch site, never carried in from `ops.enqueue`.
- `SCX_ENQ_REENQ` / `SCX_ENQ_CPU_SELECTED` — inputs for the scheduler to TEST, not
  insert semantics.
- `SCX_ENQ_IMMED` — meaningful only for `SCX_DSQ_LOCAL`; forwarding it to a custom DSQ
  trips `WARN_ON_ONCE` in `dsq_inc_nr`, so the literal is if anything SAFER.

The kthread arm still forwards real flags, and must: it inserts into
`CAKE_DSQ_LOCAL_ON`, where IMMED does mean something.

### R.6 — Distrusting dfl's WAKE_SYNC waker-affinity return

`scx_bpf_select_cpu_dfl()` reports `is_idle=true` for ANY successful pick — including
"wake @p to the local DSQ of the waker", which returns the waker's still-BUSY CPU and
only requires that idle capacity exist *somewhere* (kernel idle.c).

Direct-dispatching there is a one-way door into co-location for buffered streams: the
wakee's prev aliases the waker's CPU forever after, every message pays a context-switch
round trip, and the pair never re-splits. Measured 2026-07-10: **pipe welded to 196K
ops/s at 5.09 µs/op vs native's split-parallel 1.28M at 0.78 µs/op** — the buffer is
exactly what makes parallelism profitable.

So redirect to the idle capacity dfl just proved exists. A genuinely serial handoff
converges back on the next wake, because its partner's prev is then genuinely idle.
Under real saturation `pick_idle` fails and the wakee queues behind the about-to-sleep
waker — the EEVDF serial-handoff shape. Futex and schbench wakes carry no WAKE_SYNC.

### R.7 — `ops.dispatch`: four stages, and why each is starvation-safe

**1) Earliest eligible vtime of {own queue, wake queue}**, class-aware hysteresis so the
wake head must beat the own head by a margin (see §R.3).

**2) Two lockless peeks** — one RCU load of `first_task` each, no DSQ lock, so the
3M-wakes/s serialisation stays dead. Own-first-always **starved WAKE_DSQ** whenever own
queues never emptied: pure-spinner saturation requeues a continuation every slice,
dispatch always found local work, and a woken task waited out the 5 s watchdog
(stress-ng-futex, 2026-07-02). Vtime comparison seals it at the source — a stranded wake
head's vtime is frozen while every running task's advances past it, so each CPU soon
prefers the wake head. **Starvation-free with no rescue path**, and it doubles as the
latency rule: a blocker that sleeps often carries low vtime and beats the warm backlog
the moment it wakes. Insert and own-queue consume both hold this CPU's rq lock, so the
DSQ spinlock is uncontended except for occasional stealers.

**3) Steal ring**: two constant-start half-loops ascending from `cpu+1`, wraparound
expressed as the second loop — no modulo, no wrap arithmetic, unroll-resistant,
verifier-friendly. The own-offset start is a zero-cost anti-herd stagger. A blind
`move_to_local` takes the victim's min-vtime task, skips tasks whose cpumask excludes
us, and self-corrects cross-queue vtime inversion; an empty victim costs one lockless
list-empty read. Migration happens at the work-conserving minimum: only when this CPU
would otherwise idle or refill (pull model).

**4) Everything visibly empty → keep prev running with a fresh slice** (the +46%
stress-ng-cpu-cache-mem lever). It cannot starve queued work: our own `move_to_local`
returning false proved DSQ[cpu] empty under its lock, and the kernel would otherwise
keep prev anyway with its 20 ms default slice.

Only scalars stay live across the move kfuncs — a successful remote consume drops this
rq's lock mid-call, and we return immediately on success.

## §S — Constant ledger, relocated from `intf.h` and the rodata block (2026-07-30)

`intf.h` measured **2.80 comment:code** — 140 comment lines carrying 50 lines of
enumerators. Every dose-response table, falsification and "was X until" note is below;
the header keeps one WHAT/WHY line per group. Companion to §R, same container change.

### S.1 — `SLICE_NS = 3 ms`, and the 1.5× slice that never existed

A compile-time immediate, replacing the former `const volatile slice_ns` rodata global
and its loader `-s` knob — one policy, no per-use policy load. 3 ms is the
dose-responsed U-curve minimum; **1, 2 and 4 ms all measured worse (2026-07-04)**.

**EVERY task gets exactly this slice.** A claim that "continuations queued behind a
waiter get 1.5×" stood in `intf.h` until 2026-07-30 and was FALSE: that grant was
deleted in the zero-spill work, and all six `cake_dsq_insert*` call sites pass plain
`SLICE_NS`. Verified against the object — the only 4.5 ms constant in the binary is a
SUBTRACTION, `frontier - SLICE_NS - SLEEPER_LAG_NS`, not a slice grant. See S.7 for
what the deletion cost.

### S.2 — Why the scheduling-relative family may keep slice divisors

`SLEEPER_LAG_NS`, `HOME_PREEMPT_YOUNG_NS`, `HOME_PREEMPT_BASE_MARGIN_NS`,
`DEEP_WAKE_HYSTERESIS_NS`, `PEER_WAKE_HYSTERESIS_NS` are the ONLY constants that may be
denominated in slices, because the quantity each measures genuinely IS a turn: *"how
many turns of service behind is this task?"* Scaling them with `SLICE_NS` is correct by
construction.

The divisors are written as literals rather than behind overridable macros — changing
one is a scheduling change, and a scheduling change is a commit.

### S.3 — The five literals named on 2026-07-30

These were UNNAMED literals inline in `cake.bpf.c`, the worst class of magic number: an
audit of `intf.h` found none of them, so they could not be inventoried, dose-responsed
or reasoned about. Values unchanged — proven byte-identical by disassembly.

| name | was | what it measures |
|---|---|---|
| `COMPUTE_OCCUPANT_MIN_RAN_NS` | `ran < SLICE_NS / 2` in `cake_wake_preempt_compute` (since merged into `cake_wake_preempt`) | how much of a turn the occupant has used — genuinely slice-relative |
| `HOME_PREEMPT_RAN_CREDIT_SHIFT` | `(ran >> 1)` | the occupant earns back half its elapsed runtime as margin, so a long-running occupant is harder to preempt than a fresh one |
| `WAKE_STARVE_REFRESH_NS` | `WAKE_STARVE_WALL_NS / 2` inline | see §R.16 |
| `CAKE_NEIGHBOUR_PROBE_DEPTH` | `pi < 3` inline | neighbour probe depth |
| `cake_sleeper_dose` | `(x >> 1) + (x >> 2)` inline | the S1d dose, ¾ of the unused slice fraction |

`COMPUTE_OCCUPANT_MIN_RAN_NS` separates regimes by pure state: a wake-herd peer never
accumulates half a slice (it blocks within microseconds), external compute mid-slice
always has.

### S.4 — `WAKE_STARVE_WALL_NS`: the display-anchored bound that drifted

A WALL-CLOCK starvation bound for the global wake queue, deliberately not denominated in
vtime like everything above it. **The frontier only advances in `cake_running`, which
the kernel skips on keep-running slice refills**, so a vtime-denominated bound advances
at some fraction of wall rate and cannot bound a wall-clock stall: "N slices behind" can
take seconds. Three measured runnable-task stalls under stress-ng-futex ran **5.1 s and
6.2 s against margins nominally worth 6 ms**.

24 ms is ~3 frames at 120 Hz — long enough that it cannot fire inside the band the vtime
hysteresis protects (plain earliest-vtime arbitration stampedes the global lock: **futex
−49%**), short enough to clear the 5 s watchdog by 200×.

**EXTERNALLY ANCHORED, decoupled from `SLICE_NS` on 2026-07-30.** It used to read
`8 * SLICE_NS`, which happens to equal 24 ms only at a 3 ms slice: at 1 ms the same
expression is 8 ms, i.e. ~1 frame, so changing the timeslice silently retargeted a
DISPLAY-anchored safety bound to a third of its intended horizon. Its own comment always
said "3 frames at 120 Hz"; it is now written in the units it actually means.

### S.5 — The hardware-anchored rodata pair, and its dose table

`cake_handoff_max_ns = 1464` and `cake_preempt_protect_ns = 375000` are properties of
this CPU's syscall + switch path, NOT of the timeslice. Written as `SLICE_NS` divisors
they were wrong at any other slice value and on any other host — **at 1 ms the handoff
threshold became 488 ns, below the measured ~606 ns sleep floor, silently disabling the
co-location gate entirely.**

They live in rodata (libbpf freezes it pre-load, so the verifier folds them to
immediates). The loader probes the hardware and LOGS it but does **NOT** drive them —
doing so cost **mutex-handoff −35.66%**, because a boot-time probe measures the wrong
distribution. `probe_handoff_hop_ns()` in `main.rs` is diagnostic only.

**How short a quantum still counts as "woke someone and got out of the way"** — swept
2026-07-29, four counters in parallel, one run per workload, firing rate at conf == 3:

| threshold | | mutex_handoff | schbench | futex |
|---|---|---|---|---|
| `SLICE_NS/32` | 93.75 µs | 99.82% | 11.94% | 0.02% |
| `SLICE_NS/128` | 23.44 µs | 99.81% | 10.96% | 0.02% |
| `SLICE_NS/512` | 5.86 µs | 99.78% | 6.76% | 0.01% |
| `SLICE_NS/2048` | 1.46 µs | 99.67% | 1.51% | 0.00% |

1.46 µs costs mutex_handoff **0.15 points** and cuts schbench's false-positive rate
**EIGHTFOLD**, taking the serial:producer separation from 8.4× to **66×**. That is the
whole point of the term — a genuine handoff quantum here is ~600 ns, so anything looser
was admitting producers that merely happened to block.

1464 ns is therefore not a guess; it is the dose-responsed value, empirical for this
host. **Making it travel to other CPUs needs the runtime `used` distribution, not a
startup measurement** — that is the open work, not a missing probe.

### S.6 — Only topology may come from a cflag

`CAKE_NR_CCDS` / `CAKE_NR_CPUS` / `CAKE_CCD_STEAL_POLICY` are facts about the machine
being built for, not policy: they size the multi-CCD steal-order table and select the
steal loop shape, so they cannot be resolved any later than the build.

They are the ONLY definitions a cflag may set. **Scheduling policy is source-only, on
purpose: a policy A/B has to be two commits, never two cflag values.** The BPF object
cache across cargo fingerprint directories can serve a stale object for a cflag-only
change even after a `touch`, so a flag-toggled arm cannot be trusted to be the code it
claims to be, and a revert cannot be proven byte-identical.

### S.7 — Ids, geometry, and the deleted `OVF_DSQ`

`MAX_CPUS = 1024` is a verifier sizing bound for per-CPU state and the steal loops, a
power of 2 so hot-path indexes reduce to `cpu & (MAX_CPUS - 1)` — a mask, not a modulus,
and provably in bounds. It is NOT the DSQ count: one custom vtime DSQ per possible CPU
is created at init (`dsq_id == cpu`, `nr_cpu_ids` of them).

`WAKE_DSQ = MAX_CPUS` is one id above that range. `MAX_CPUS + 1` **was `OVF_DSQ`,
deleted 2026-07-27 under NO BUCKETS** (§R.15). The id is left unused rather than
recycled.

`STATE_SLOT_BYTES = 128` is the cache-isolation stride — see §R.10 for why 128 and not
64. `RECIP_*` / `STATIC_PRIO_BASE` / `MAX_RECIP_WEIGHT` are representation constants,
not policy knobs: the split high/low product in `cake_scale_vtime()` avoids overflowing
`runtime * reciprocal` on an unusually long uninterrupted run while preserving the exact
former result for ordinary runtimes (§R.13).

### R.8 — Cake-local kfunc bindings, and why the compat ladder was dropped

**MINIMUM KERNEL 7.1.** `compat.bpf.h` resolves each kfunc through a two- or three-way
`bpf_core_type_exists()` / `bpf_ksym_exists()` ladder. Those are LOAD-time relocations,
so at COMPILE time LLVM must emit every arm and keep every argument live across all of
them; the verifier's later dead-code elimination removes the untaken arms but **cannot
undo the register allocation**.

On the hottest callback that is not a rounding error: the shims accounted for **a fifth
of `cake_enqueue`'s instructions and a third of its stack reloads**, and
`__COMPAT_scx_bpf_dsq_peek`'s iterator fallback alone turned a one-call peek into
**four calls — paid twice per dispatch**. `move_to_local` is a THREE-arm macro and
dispatch calls it twice plus once per steal-ring probe, the single largest contributor
to that callback's call count.

Cake binds straight to the modern kfuncs. The loader refuses to attach on an older
kernel with a message naming the reason, rather than letting the verifier report an
unresolved ksym. **The shared `compat.bpf.h` is deliberately untouched** — other
schedulers keep their portability; this is cake's own call.

**NOTE THE NAME.** libbpf strips the trailing `___suffix` before resolving a kfunc, so
declaring `scx_bpf_dsq_move_to_local___v2` asks the kernel for
`scx_bpf_dsq_move_to_local` and fails to load with *"kfunc … is referenced but wasn't
resolved"*. That is exactly why compat.bpf.h has to spell the insert
`scx_bpf_dsq_insert___v2___compat` to reach `scx_bpf_dsq_insert___v2`. Cake needs its
own extern rather than compat's, because compat's is only ever called under a
`bpf_ksym_exists()` guard — the ladder this exists to avoid.

### R.9 — Rebinding the `SCX_*` enumerators to compile-time immediates

`enums.autogen.bpf.h` `#define`s each `SCX_*` name onto a loader-filled
`const volatile u64 __SCX_*` — **a rodata memory load on every hot-path use.** cake
`#undef`s the seven names it uses and rebinds them to `bpf_core_enum_value()`, which
CO-RE resolves to a load-time immediate. Precedent: `compat.bpf.h` uses this mechanism
on `scx_enq_flags`.

**The `#undef` is PERMANENT — deliberately no push/pop.** Macro bodies expand at the use
site, so a pop would silently rebind the enumerator names back to the volatile shadows
inside every `CAKE_*` expansion. It must sit after all scx header includes, and cake
never uses the `SCX_*` macro forms below that point.

### R.10 — `cake_state` cache geometry

All mutable hot state lives in ONE BSS struct built from **128-byte-stride slots**, so
any two accessed words are ≥128 bytes apart in offset: regardless of the struct's base
alignment their 64 B line indexes differ by ≥2 — never the same cache line, and never
the same adjacent-line-prefetcher 128 B pair. This replaced scattered
`__attribute__((aligned(64)))` uses with pure layout.

- **`frontier`** — the global vtime frontier. Written by all CPUs in `ops.running`
  (conditional store), read in enqueue/enable. The sleeper clamp against it is
  load-bearing for futex handoff.
- **`run[cpu]`** — `stamp` is read remotely only by saturated wake preemption; `sum`
  is owner-only and holds the `sum_exec_runtime` snapshot taken in `ops.running`.
  Keeping both lifecycle-coupled values in one isolated slot makes `ops.running` dirty
  **one cache line instead of two** while preserving the 128 B inter-CPU stride.
  `ops.stopping` charges `used = p->se.sum_exec_runtime - sum` with **ZERO clock
  reads**: the kernel calls `update_curr_scx()` immediately before invoking
  `ops.stopping` (both call sites, verified in tree), so `sum_exec` is boundary-exact
  there — unlike mid-slice remote reads, which stay on the ktime stamp.
- **`wake_served`** — ktime the global wake queue was last consumed by ANY CPU. One
  global u64 rather than per-task queue-entry stamps: the question *"is WAKE_DSQ being
  served at all"* is global, and answering it this way needs no per-task state and no
  scan.
- **`qmark[cpu]`** — the "DSQ[i] may hold work" hint gating the steal ring. bpfstats: a
  going-idle dispatch spent **199 ns/call — 30% of the whole pipe benchmark** — walking
  15+ EMPTY queues through hashed kfuncs. Stealers now read these (cached, shared) lines
  and hash only marked queues.

**The qmark concurrency argument.** The only writers of `qmark[c]` are `ops.enqueue` for
a task whose `task_cpu` is `c`, and `ops.dispatch` on CPU `c` — and BOTH run holding CPU
`c`'s rq lock, so they are mutually exclusive and no insert can land between the owner's
peek and its store. A stealer can empty the queue concurrently (it takes the DSQ lock,
not the rq lock), leaving a stale mark; that is benign by construction — **a stale mark
costs one wasted move attempt, a missed mark delays only THEFT**, and the owner serves
its own queue on every dispatch regardless.

**Never re-apply a value already there.** `qmark` is read by every stealer's ring walk,
so a store writing the value already present takes the line Exclusive and invalidates
every remote copy for zero information. The test costs nothing we were not already
paying — a write needs the line Exclusive anyway; skipping the redundant store is what
lets an unchanged line stay Shared.

### R.11 — The subprogram cut points (why each `__noinline` exists)

BPF preserves exactly **r6–r9** across a call, so a hot callback gets FOUR values that
may span a call. Six of seven ops callbacks reach zero spills by cutting where the live
set is ALREADY narrow. **The rule is the CUT POINT, not the technique** — cut after the
task is published or before a tail that re-derives its own state, never through the
middle of a working set. Full law and the ~10 falsified attempts: CLAUDE.md §Design
laws and §G6.

| subprogram | what crosses the cut | measured |
|---|---|---|
| `cake_occupant_live` | `tcpu` in, `(live, ran)` out | FIVE call sites computed `cpu_curr` + clock read + recip index + scaling identically; factoring took six functions to zero |
| `cake_enqueue_wake` | `p`, `tcpu` | a wake and a continuation are two algorithms; one frame allocated against the union of both working sets |
| `cake_wake_notify` | `p`, `tcpu`, `route` | the task is already published, so three values cross; inlined, `route` stayed live THROUGH the insert alongside `p`, `tcpu`, `enq_flags` |
| `cake_home_claim` | `p`, `tcpu` in, one bit out | inlined, `cv`/`lo`/`curr` bracketed the `smp_processor_id()` call while `p`/`tcpu`/`enq_flags`/route waited on the far side — the densest point left in the wake path |
| `cake_wake_vtime` | `p` in, one u64 out | evaluated as an ARGUMENT to the insert, so inlined all its temporaries lived simultaneously with everything the caller still needed |
| `cake_pinned_wake_preempt` | `p`, `tcpu`, `d` | runs entirely AFTER the insert; `d` must be read before the insert rewrites `p->scx.dsq_vtime`, so it is the one value passed; `lo` is re-derived |
| `cake_ring_steal` | `ucpu` | a distinct phase from the own/wake arbitration; ONE value crosses |
| `cake_dispatch_search` | `cpu` in, one bool out | `prev` is read ONLY by the keep-running check at the very end, yet inlined it survived two peeks, four `move_to_local` calls and the whole ring walk |
| `cake_wake_starved` / `cake_wake_serve_stamp` / `cake_wake_idle_stamp` | nothing | the clock read happens where nothing of the caller's is live across it |

**Global (not static) `__noinline` where the verifier needs a signature**: static
subprograms get inlined back or trip E2BIG, and a global one carries its own BTF
signature and its own frame. `p` needs `__arg_trusted` — a global subprogram is verified
independently, so without the tag the verifier reads a bare `struct task_struct *` as a
pointer to `sizeof(task_struct)` bytes of caller memory and rejects the call with
*"access beyond struct task_struct at off 0 size 8240"*.

`cake_wake_notify` is **not** the falsified whole-wake split: that one moved a 200-line
body and relocated its traffic into the prologue. Here the cut is at a point where the
live set is already narrow, so there is little for the prologue to push.

**Route as one value, not two booleans.** `enum cake_route` has three reachable states;
carrying `home` and `queue_home` separately spent two of the four callee-saved registers
to encode three cases, and both had to survive the insert — which is what put
`cake_enqueue_wake` one over budget and spilled `p` itself (12 → 10 spills). This is NOT
the falsified pack-into-a-word: there is no field to extract, so CSE has nothing to
re-materialise, and every read is a single compare against a constant.

**Compute at the point of use.** `cake_scale_vtime_add` folds `base` in rather than
letting the caller add it, because the overflow-safe path is a call and
`base + cake_scale_vtime(...)` would force `base` to live across it — for a branch taken
only above ~51 s of uninterrupted runtime. In `cake_occupant_live` the stamp and weight
index are both read BEFORE the clock read so `tcpu`, the run-slot pointer and `curr` all
die there rather than spanning it; reading the stamp afterwards kept a pointer AND its
index alive for one later subtraction, which with `cv` and `ran_out` was five values
against four registers. In `cake_enqueue` the sleeper clamp is declared after the
kthread arm, and LLVM sinks it into the continuation arm on its own — **forcing it in by
hand measured WORSE, 48 → 53 stack ops.**

### R.12 — `cake_log2_u64`: ten branches, kept on purpose

The log2 is five magnitude tests, and they ARE branches: LLVM emits **ten conditional
jumps** for them (insns 39–65 of `cake_enqueue` in the release object). A comment
claiming "six branch-free conditional shifts" stood here and was never true of the
emitted code.

Left as branches per the branchless law's own limit — *do NOT branchless a predictable
branch; it evaluates both sides and adds a data dependency.* These test the magnitude of
`p->se.sum_exec_runtime`, which for any task that has run more than ~4 s exceeds 2³² ns,
so in a steady-state system every level resolves the same way nearly every time. The
branchless mask-and-shift form would trade ten well-predicted jumps for an unconditional
**~36-instruction serial dependency chain with no early exit**. Which one actually wins
is a measurement, not a reading of the source — it belongs behind an endpoint, not
inside a cleanup sweep.

### R.13 — The sleeper clamp and S1 cadence depth

The clamp is `max(own, frontier - one slice)`, branchless and wrap-safe under
`time_before()` semantics:

```
d = own - lo;  own >= lo => (s64)d >= 0 => mask = ~0 => lo + d = own
               own <  lo => (s64)d <  0 => mask =  0 => lo + 0 = lo
```

**S1 — cadence-proportional sleeper depth (2026-07-19, graph node S1).** The uniform
clamp floor quantizes every sleeper to one key (law N2), so a 100 µs audio burst and a
full-slice compute wake tie and serve FIFO — the audio-under-compile defect and the Rc
schbench deepening. Deepening the floor proportionally to the task's unused slice
fraction means burst ≈ slice keeps exactly today's floor (compute unchanged, implicit
demotion) while short-burst cadence tasks earn extra depth (promotion), bounded so the
peer hysteresis (2 slices) still dominates a fresh storm wake's maximum deficit.

Burst estimate is `sum_exec >> log2(nvcsw)` — within 2× of the true mean, **no map, no
division**. The **S1d dose (2026-07-20) is ¾ of the unused slice fraction**: the bracket
measured futex monotone in depth (`/8` −16%, `/4` +12%, `/2` +39%) with
schbench-light/pipe/lock-pi flat, so take the deepest dose that keeps the peer
hysteresis strictly dominant — max depth 0.75 slice → fresh storm deficit ~1.75 slice.

`cake_wake_vtime()` is computed at each use rather than once per callback: every input
is a plain load, so recomputing costs a few arithmetic instructions while holding it
costs a register across every kfunc between producer and consumer. It reads
`cake.frontier.word`, which another CPU may advance in between — that is already the
documented frontier contract (advisory, monotonic under `time_before`, raced on by
design in `cake_running`), and **a later read is a fresher one**.

**Direct dispatch must pay the same floor.** `scx_bpf_dsq_insert()` takes no vtime, so a
directly-dispatched task never has `p->scx.dsq_vtime` touched — only the insert_vtime
path clamps. A task that sleeps long and is always admitted directly would keep an
arbitrarily old key, and the moment load rises and it finally IS queued it arrives with a
hugely favourable vtime and jumps everything ahead of it: **exactly the sleeper
monopolisation the clamp exists to prevent, entering by the one path that skips the
clamp.** The failure is delayed and load-dependent, so it would surface as an
unexplained fairness bug with nothing pointing at `select_cpu`. Admission is service
whichever door it comes through. The clamp is a max — it only ever RAISES vtime toward
the frontier — so it cannot manufacture credit or penalise anyone; it can only stop
credit accruing for free.

### R.14 — Kthread and pinned-wake service

**All kthread wakes go direct (P0, extended 2026-07-19 from pinned-only).** A
permanently pinned kernel thread has exactly one place where forward progress is
possible, so its wake episode goes to that CPU's local DSQ instead of waiting behind
shared wake/own arbitration. The extension to ALL kthreads is a watchdog fix: the
kernel's scx watchdog rides UNBOUND kworkers, and under a futex storm those queued as
ordinary herd citizens — **wake→run p99 17 ms / max 192 ms measured** — accumulating past
the 5 s check-in and getting cake evicted. `PF_KTHREAD` is scheduling state, not workload
identity. Only the wake takes this path; a runnable continuation returns through the
normal weighted-vtime queue.

**Pinned USER wakes (census-verified 2026-07-19).** A pinned user task's wake lands on
the continuation path — not the wake path — because `nr_cpus_allowed == 1`, and NO other
CPU may steal it, so a wake behind a busy occupant used to wait out the occupant's whole
slice. lock-pi pins its workers: **handoff p99 3.9 ms vs native 171 µs, −86.6%**; census
found **114k of these per arm vs 14 truly flagless**. Native wakeup-preempts by
eligibility, so cake preempts by **RAW sleep depth**: the clamp erases deservingness, and
a pinned wake has no work-conservation alternative, so this cannot collide with the
closed global wake-service frontier — multi-CPU wakes never reach it.

**And the routing rule this sits inside.** *Wakeups are global, continuations are local.*
A woken task must be findable by the FIRST CPU that blocks anywhere — pinning it to one
home CPU's queue strands handoff chains behind a stranger's slice (**futex 20–50×**,
2026-07-01); a slice-expired task wants exactly its home CPU for L1/L2 warmth (the
schbench p99 requeue band). The routing key is the enqueue's own wakeup bit: one
algorithm, no state, no detector. EXCEPT single-CPU tasks, where the global queue's
premise is false — a pinned wake stranded in WAKE_DSQ ate the 5 s runnable-stall watchdog
(stress-ng-futex, 2026-07-02).

**M8 — an RT-owned CPU is not an empty home.** It may burst again (compositor/audio RT
threads), and the kernel preempts SCX unconditionally for it regardless of anything cake
does. CFS/EEVDF discounts a CPU's usable capacity by its RT/DL load average
(`scale_rt_capacity()`, fair.c) before placing a task there; sched_ext's idle tracking
has no equivalent, so cake approximates it — an RT-owned home is never an empty home, and
`&&` short-circuits the queue lookup away when it holds.

**The self-race arm.** Waking the task this CPU is still switching out (sub-slice
block/wake cadence rides the ttwu wakelist and lands with `curr == p` — the pipe/futex
on-cpu shape) is the hottest single wake path, so it runs BEFORE the `nr_queued`
rhashtable lookup: `cpu_curr` is a plain per-cpu rq deref. Home is right even with a
non-empty queue — `p` was current here microseconds ago (continuation-local by
definition) and the vtime order keeps whatever is queued ahead of it fair. Eligibility,
the ktime read and a preempt kick would all be spent against ourselves.

**Post-insert notification is owed even when nothing is idle.** Until 2026-07-27 a
HOME-routed wake got none: the test was `!home && !cake_wake_preempt(...)`, so the one arm
that had already committed the task to `tcpu`'s queue was the one arm excluded from
telling `tcpu` about it. The home rule only fires inside `HOME_PREEMPT_YOUNG_NS`
(3.1% of the occupant's slice), so past that window the wakee sat runnable with nobody
scheduled to re-evaluate — **cake registers no `.tick`, and the only backstop was the 5 s
watchdog.** `cake_wake_preempt` is the complement of the young rule, not a rescue: it
fires only once the occupant has held the CPU for `cake_preempt_protect_ns` and the
wakee's vtime is genuinely older. If the occupant is more deserving the wakee waiting is
fairness, not a stall — but that has to be a DECISION, which is what was missing.

The neighbour hunt is **globally-queued only**. A home-routed wake deliberately does NOT
come there — it was placed on `tcpu` for warmth, and waking a neighbour to steal it back
off that queue would spend the locality the routing just bought.

**Home-arm notification is decided AFTER the insert, and is exact on both inputs it
re-reads.** `curr` cannot have changed: `ops.enqueue` runs holding `tcpu`'s rq lock and
inserting into a DSQ never drops it, so no context switch can have happened on `tcpu`.
`p->scx.dsq_vtime` IS the clamped vtime the insert just used — the kernel assigns
`p->scx.dsq_vtime = vtime` inside `scx_dsq_insert_vtime()` (ext.c:8728) before
committing. **Reloading it from the task is what lets the key stop being a live value.**
A zero curr vtime means an idle- or higher-class-owned home where no preempt exists to
fire; there the answer is whether this callback is running on `tcpu` itself, in which
case core's `activate→wakeup_preempt` has already rescheduled it and a kick would be pure
churn.

### R.15 — NO BUCKETS (2026-07-27): why `OVF_DSQ` is deleted, not better-caught

A deep owner queue used to divert the continuation into `OVF_DSQ` — **and skip
`cake_qmark_set()` while doing it**, which made the task invisible to the steal ring,
because the ring walks per-CPU DSQs by qmark and nothing walked OVF. That is why OVF
needed an aged rescue AND a fallback drain in dispatch: a bucket, then two catchers.
Tasks still sat there **>6 s and tripped the watchdog (three reproductions,
2026-07-11)**.

The ordering rule already conserves work: a continuation left on its owner's queue is
marked, so any CPU going idle finds it on the ring walk. Keeping it there also keeps the
L1/L2 warmth the continuation was queued for. **The bucket and both catchers are deleted
rather than better-caught** — the structural fix, not a third catcher.

### R.16 — Wake-queue starvation escalation, and the stamp that was always armed

**An EMPTY wake queue is a SERVED wake queue.** `wake_served` starts at 0 (BSS) and was
written only on an actual consume, so `cake_wake_starved()` was true from attach onward —
and in any regime where wakes mostly route home, WAKE_DSQ sits empty for seconds at a
time and the stamp never refreshes. The escalation the stamp exists to gate was therefore
**permanently armed**: the first task to land in WAKE_DSQ had every CPU prefer the global
queue at once, which is the plain-earliest-vtime stampede the one-slice hysteresis was
added to prevent (**futex −49%**, 2026-07-02). The bypass meant to fire only rarely was
the default.

It is refreshed only once the stamp is already HALF a window old. Every CPU's dispatch
reads this line, so storing on every empty peek would cost an RFO at context-switch rate
on a line all of them poll — the same reason `cake_running`'s frontier store is
conditional rather than a branchless max. Half a window bounds the residual both ways:
the write rate is at most two per window per CPU, and the escalation can still never fire
sooner than `WAKE_STARVE_WALL_NS/2` after work actually arrives.

The escalation itself exists because **every margin in dispatch is denominated in vtime,
and the frontier only advances in `cake_running` — which the kernel skips on keep-running
refills** — so a vtime bound cannot bound a wall-clock stall. Measured: 5.1 s and 6.2 s
stalls against margins nominally worth 6 ms.

### R.17 — Direct field writes instead of the authority-checked kfuncs

`p->scx.slice` and `p->scx.dsq_vtime` are written directly rather than through
`scx_bpf_task_set_slice()` / `scx_bpf_task_set_dsq_vtime()`. Those kfuncs exist to run a
**sub-scheduler authority check** (`scx_prog_sched()` / `scx_task_on_sched()`, see
internal.h), and cake is a flat root scheduler that will never have sub-schedulers to
authorize against.

In `ops.stopping` — the hottest per-switch callback in the scheduler — that check
measured **+28–36%** on kernels with `CONFIG_EXT_SUB_SCHED=y`: three independent A/Bs,
forensically verified against BPF prog id / `loaded_at` to rule out stale-measurement
contamination (2026-07-08). Still deprecated (emits a kernel log warning, not a
functional break) — an acceptable trade.

For the slice write there is not even a trade: on this kernel the wrapper's ksym does not
exist, so its own fallback is already this exact store, and the two arms it compiles were
pure cost.

`task_cpu(p)` is likewise read directly as `p->thread_info.cpu`. The kfunc body is one
CO-RE-able load (`READ_ONCE(task_thread_info(p)->cpu)`; `THREAD_INFO_IN_TASK` is
unconditional on x86-64), so calling it paid a full kfunc call-clobber — r1–r5 dead,
`tcpu` spilled to stack and reloaded around every later kfunc — on the hottest enqueue
path, for one 4-byte load. `p` is already verified `PTR_TO_BTF_ID` there.

`task_struct::policy`'s `SCHED_*` values are plain uapi macros, not a BTF enum, so
`bpf_core_enum_value` has nothing to hook: guarded local defines, the same convention
`scx_pandemonium` uses for this exact check.

### R.18 — The G9.4 handoff hint and its confidence hysteresis

Per-CPU handoff learning, written only by its own CPU (`select_cpu` on the waker,
`ops.stopping` on the same task) — no sharing, no atomics, and it rides a line
`ops.running`/`ops.stopping` already own. Bit 0 `WOKE` = this CPU's occupant woke someone
during this quantum; the confidence field counts consecutive wake-then-block-quickly
quanta, i.e. serial-handoff partners.

`used` is already exact in `ops.stopping` (the kernel calls `update_curr_scx` immediately
before it), and `runnable` says whether the task BLOCKED or was merely requeued — so the
test **costs no clock read at all**.

**HYSTERESIS.** The plain one-quantum bit censused at **99.85% on mutex_handoff, 1.51% on
stress-ng-futex — but 25.44% on schbench**, whose producer occasionally does
wake-then-block by accident. A saturating confidence counter asks for the pattern to
REPEAT, which a genuine handoff partner does every quantum and a producer does not. A
preempted task (`runnable`) leaves the counter alone: it never got to finish its pattern,
so neither confirming nor clearing is right.

### R.19 — What ZERO STACK SPILLS cost, stated plainly

Two scheduling decisions were deleted in `4d5b5f96d` to reach zero spills. Both are
**UNMEASURED since deletion**, and CLAUDE.md now records the premise they were priced
against as falsified — restoring all three deleted decisions costs exactly **3 spills +
4 fills, seven L1-hot memory ops**. Recorded here so the next session prices them
correctly rather than rediscovering them.

1. **The callback-locality claim in `cake_home_claim`** (`tcpu == bpf_get_smp_processor_id()`).
   It was the only claim needing a helper call, and comparing `tcpu` against that call's
   result forced `tcpu` to outlive it — the one unavoidable spill there. **COST: this was
   the converged-pair signature**, the LOCAL wake where curr is the waker. Its raw-vtime
   sleeper test flaps for exactly that shape, and routing the flap globally collapsed
   **futex 4.8M → 0.98M (2026-07-04)**. The sleeper and valve claims still catch most of
   it; the pair that trips neither now goes global.
2. **The contended-slice choice in `cake_enqueue`** — probing queue depth there put a call
   between `vt` and its use. **COST: continuations behind a waiter lose the 1.5× turn**
   that bought schbench p99 **7368 → 6984, sat +30%, cache +18%** (2026-07-04). A
   continuation landing behind a waiter is one half of an alternating pair — the only
   place turn length is paid for in L2 refills.

### R.20 — `ops.select_cpu`'s saturated convergence arm

When dfl finds no idle CPU the system is saturated on this task's affinity. `select_cpu`
runs in the wake path before an optional queued activation, so the callback CPU is the
waker CPU, and **its queue state is authoritative**: if both queues are empty, returning
it cannot jump queued work and lets enqueue perform the ordinary vtime insert. Process
identity is deliberately irrelevant (see the ALLOW_QUEUED_WAKEUP note below).

Two independent reasons to converge, and both are pure loads:
- **WAKE_SYNC is an explicit handoff signal.** Converge on the empty callback CPU, but do
  not direct-dispatch with `LOCAL_ON`: that would force a preempt and bypass live-vtime
  eligibility.
- **A plain wake has no handoff promise**, so converge only a wakee whose raw vtime proves
  it slept behind the frontier, matching the sleeper class used by enqueue. A
  frontier-running compute peer keeps dfl's prev placement and cache warmth. This
  wakee-only state rule stays valid in queued callback context and is process-agnostic.

**Tested BEFORE the queue state rather than after.** The conjunction is the same either
way — every term is side-effect-free — but this ordering retires `wake_flags` and the
vtime read before the three kfuncs instead of holding them across all of them, and it
skips those three calls outright for the frontier-running compute peer that was never
eligible to converge in the first place.

**`SCX_OPS_ALLOW_QUEUED_WAKEUP`**: after `select_cpu` chooses the target, remote activation
may ride the batched TTWU queue instead of taking the remote rq lock per wake. `enqueue`
therefore treats its callback CPU only as a locality signal; no policy depends on process
identity or assumes enqueue's `current` is the waker.

### R.21 — `ops.init` validates the span rather than storing it

`nr_cpu_span` is rodata, frozen before load, so the hot paths that read it get a
verifier-known constant — which is why it moved out of the 128-byte cache-isolated BSS
slot it used to occupy to hold one never-changing number the verifier had to treat as
unknown.

A span **wider** than `nr_cpu_ids` is harmless: the extra ids own no DSQ, are never
qmarked (qmark is only ever set from a real task's `task_cpu`) and so are never probed. A
span **NARROWER** is not — the steal ring would stop short and work queued on the CPUs
above it would only ever be served by their owners. `ops.init` refuses that rather than
silently under-scanning.

Raw small DSQ ids are safe: the kernel reserves only bit-63 builtin ids.
