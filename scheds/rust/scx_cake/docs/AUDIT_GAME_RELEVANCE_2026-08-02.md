# Game-relevance audit of every live system (2026-08-02)

Maintainer question: have all systems been audited for game relevance, and is
there structural bloat to remove? Answer assembled from the three audit passes
of 2026-07-30 → 2026-08-02 cross-referenced against the current design.
Evidence classes per CAMPAIGN_LEDGER.md (FRAME > WAKE > CENSUS > STATIC).

## The three audit passes that exist

| pass | date | scope | instrument |
|---|---|---|---|
| §G10 firing-rate census (`dba25375c`) | 07-30 | 9 benchmark-era mechanisms | counters on 3 workloads |
| G11–G20 campaign | 07-31→08-01 | wake/placement path | WAKE + first FRAME reads |
| G21–G23 + G22 board | 08-02 | quiet regime, IRQ environment, frames | WAKE + FRAME + host |

## System-by-system: best game evidence on record

| system (site) | game evidence | verdict for games |
|---|---|---|
| cache-warm home claim | G13 WAKE, kept | **HELPS** (same-CPU gap 8.9–16.7 → 0.9–4.5 pts) |
| anti-collision admission | G17 WAKE, kept | **HELPS** (renderer −17.3%); mechanism story incomplete |
| slice cap on frame estimate | G18 WAKE, kept | **HELPS** (wake p99 −16% 2/2) |
| frame clock (mode + hysteresis) | G11 direct | **HELPS** (0.05–0.4% error, locked 28/30 s) |
| sink detection + nonsink mask | G21 FRAME 2/2, G23 smoke | **HELPS** (0.1% low +7.7%, p99.9−med −31.6%) |
| serial-handoff co-location | G10 census + gate restored | **INERT-BY-DESIGN in games** (HD2 62% idle < 75% gate); carries mutex +44% |
| enqueue routing tree (all arms) | census umbrella | **~INERT in games** — `ops.enqueue` sees 0.14% of game dispatches (23.6% saturated); carries the cake-ring benchmark wins |
| qmark ordered admission | census | **decoration in games** — head-peek consulted 1 in 42,935 direct dispatches |
| pinned-wake preempt margin (`:900`, `:1011`) | G10 census | **DEAD — 0% on ALL THREE workloads**; disposition "dead branch" never executed |
| SLEEPER_LAG gates (`:715`, `:813`) | G10 census | "SATURATED 94–99.95% — not a predicate"; disposition unexecuted |
| vtime/reciprocal fairness ledger | G10 #9 | the object's only 4 multiplies serve it; "chain-class ordering instead" unbuilt (ties to ledger open item 3, `SLICE_NS`) |
| kthread idle-first arm (G20) | WAKE + FRAME | **NULL, premise unstable** (`DP-2` 45× vs 11,518 on identical code); kept, unproven |
| starve wall (`cake_starved`, 4 sites) | G10 census | 0 of 11,980 evaluations; "make cheap, keep" — safety bound, keep |
| neighbour probe depth 3 | G10 census | 4.8% effective, 91.6% of work on miss path; only runs with zero idle CPUs → rare in games |
| compute-occupant preempt (M) | G10 audit | explicitly NOT bloat — load-bearing on futex AND protects a just-started renderer |
| dispatch search + steal ring | — | **NEVER AUDITED on games** |
| sibling kick (global route) | — | **NEVER AUDITED on games** |
| R.6 sync-distrust re-pick | — | **NEVER AUDITED** (firing rate unknown) |
| run-slot hint learning | — | feeds only the serial-handoff gate → inert in games by that gate |

## Verdict on the bloat thesis

**Partially confirmed, with receipts — but "inert in games" is not "removable".**
The campaign's own census facts say most of the enqueue-side decision tree is
near-inert on a game. The same mechanisms carry the sealed benchmark wins
(mutex-handoff +44.24%, futex modes +57.2, pipe +22.7%), so under GAME-FIRST
their removal is a games-vs-benchmarks trade — the maintainer's call with a
full ledger, per §Running an experiment.

**Three items are different — bloat by every measurement, no trade required:**

1. **Pinned-wake preempt margin: 0% firing on all three census workloads**
   (games AND benchmarks see nothing). §G10 disposition "dead branch" was
   never executed. Still live: `cake_pinned_wake_preempt` (22 insns) + its
   call site in `cake_enqueue`. → re-census once to confirm, then delete.
2. **SLEEPER_LAG as a predicate** — true 94–99.95% under saturation; a
   predicate that always passes is an unconditional path plus a compare.
   §G10 disposition "not a predicate" unexecuted.
3. **G20's kthread idle-first arm** — measured null with an unstable premise;
   +10 insns buying nothing proven. Weakest of the three (null ≠ harmful).

## Coverage gaps — never audited on a game, all CENSUS-tier (no game time needed)

Dispatch search + steal ring firing rates in the quiet regime; qmark
set/clear write traffic per frame; R.6 re-pick rate; sibling-kick hit rate.
Retained traces cannot see these (in-kernel decisions, not sched events) —
they need the GAME_DIAG counter pattern (PERCPU_ARRAY law) for one census
build, run against a retained-scene session.

## Recommended registration (G24, census tier)

One diag build with counters on the five unaudited paths + a fresh census of
the pinned-wake margin; one HD2 session, quiet regime, no A/B needed (census
measures firing, not benefit). Output: firing-rate table → execute §G10 #5
(delete the dead branch) if it re-censuses at 0%, and price #8/#9 with data.
Cost: one diag commit + ~5 min of unattended game time; zero scored arms.
