# mutex-handoff: what the −47% is NOT (dose-response record, 2026-07-28)

Status: **mechanism still unidentified.** Two candidates priced and eliminated.
Everything here is `diagnostic_only`, 4–8 blocks, paired ABBA.

## The loss

`mutex-handoff` measures `handoff_p99_usec` — a p99 **latency tail** on a
**2-thread** ping-pong, 1M iterations. Not throughput. That matters: two threads
on 16 CPUs means no contention, so every mechanism cake uses to conserve
throughput under saturation is pure overhead here.

| arm | p99 | vs native |
|---|---|---|
| native EEVDF | 1.0–1.14 µs | — |
| cake HEAD | 1.7 µs | −47% to −56% |
| cake 1.1.3 | 1.7 µs | −53.6% |

**It is architectural, not a regression from the 2026-07-27/28 work.** 1.1.3 —
which predates every commit in that arc — loses by the same margin, across all
8 blocks, with no overlap against native in either run.

The absolute numbers are the useful part: **1.0 µs vs 1.7 µs**. A 0.7 µs delta on
a 2-thread ping-pong is roughly one extra wakeup path. This is not a policy
blunder; it is cake taking a longer route on the handoff.

## The sealed +10.3% is stale

The corpus records `mutex-handoff +10.3%` sealed 2026-07-19. The same July-era
binary (1.1.3) now reads **−53.6%** on this machine. Something environmental
moved — most plausibly the kernel, which is new enough (7.1.5) that it broke
1.1.3's libbpf struct_ops outright and required a rebuild to attach at all.

**Treat that sealed row as superseded.** It describes a machine that no longer
exists.

## Candidate 1: the WAKE_SYNC split-redirect — ELIMINATED

`cake_select_cpu` distrusts `scx_bpf_select_cpu_dfl`'s WAKE_SYNC return: when
dfl hands back the waker's own CPU, cake redirects the wakee to a different idle
CPU. Source rationale is pipe — co-location "welded pipe to 196K ops/s at
5.09 µs/op vs native's split-parallel 1.28M at 0.78 µs/op".

Hypothesis: a 2-thread mutex ping-pong IS the serial pair. The redirect splits
it every handoff, costing a cross-CPU wakeup + IPI, and that is the 0.7 µs.
Prediction: disabling it improves mutex and regresses pipe — the corpus law
("handoff concentrates, parallel spreads; one blunt rule always loses a side")
made visible as a table.

Probe: `if (0 && ...)` on the redirect, 4 blocks per cell, both workloads.

| workload | redirect ON | redirect OFF | effect of disabling |
|---|---|---|---|
| mutex-handoff | −46.7% (1.7 µs) | −57.4% (1.8 µs) | **worse** |
| perf-sched-pipe | +23.1% (0.8 µs) | +20.2% (0.9 µs) | **worse** |

**Falsified in both directions.** The redirect is not the trade point; disabling
it mildly hurts *both* sides. Most likely it never fires for mutex-handoff at
all — mutex wakes may not carry `WAKE_SYNC`, in which case the branch is dead
code for this workload and the source reading, while accurate about what the
code does, said nothing about whether it runs.

Cost to learn this: ~8 minutes of probe versus a day of redesigning around it.

## Candidate 2: the wake-starve escalation — NOT IMPLICATED

Priced separately (same day, different question) at `SLICE_NS/8` vs `8*SLICE_NS`.
Confirmed live via a single collapsed arm at 1.59M — *below* native, which rules
out a dead scheduler — but it fires on the saturated global queue and has no
bearing on a 2-thread idle handoff.

## What is still open

The 0.7 µs is somewhere cake's wake path is longer than EEVDF's for an
uncontended serial handoff. Remaining candidates, all reachable as constants or
narrow branches:

- the **empty-home carve-out** and its sleeper gates in `cake_enqueue_wake`;
- `SLEEPER_LAG_NS` (clamp depth) — a 2-thread pair may be clamped into the
  wrong class every wake;
- the **direct-dispatch exact-head guard** in `cake_select_cpu`, which declines
  the shortcut and falls through to full arbitration;
- the insert path itself: cake always goes through `enqueue` + a DSQ, where
  EEVDF can keep a WAKE_SYNC pair on-CPU with no queue at all.

The last is the one I would price next. It is the only candidate that plausibly
costs a whole wakeup rather than a few hundred nanoseconds, and it is the
structural difference between "insert into a DSQ and let dispatch find it" and
"run it here, now".

## Method note

Both eliminations came from dose-response, not from reading code. The source
reading was *correct* about mechanism and *wrong* about effect, twice. See
`.claude/skills/sched-ext-dev/references/dose-response.md`.
