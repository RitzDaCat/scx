# Arithmetic census — release object, 2026-08-03

**Object:** `target/release/build/scx_cake-fb5cb1b0f8a481a5/out/bpf.bpf.o`, built
2026-08-03 10:29 from clean HEAD `bacf0a817` (binary sha256 `61b976c1…185bcb`).
Census tools: `bench/fnspills.py` + per-function objdump sweep (div/mod/mul/call/
load/store/branch/atomic). Static counts; CO-RE compat alternates DCE at load, so
compat-shim calls overstate — noted where it matters.

## Closed axes (do not re-open without new evidence)

| axis | state |
|---|---|
| stack spills | **0 across all 26 functions, 1575 insns** — campaign complete (was "1-2/subprogram floor"; now literally zero) |
| atomics | 0 on every hot path |
| modulo | 0 |
| multiplies | 15, all reciprocal-table / scaling pattern — already the house style |
| line packing | done (§R.10); enqueue regions 3→2 |

## The 11 divides, mapped and classified

| site | fn (insn) | origin | quotient consumed as | dynamic rate |
|---|---|---|---|---|
| 1 | frame_observe (5) | `(now−start_time)/nvcsw` | **compared first** (2–40 ms gate), magnitude only if in range | **every ops.running ≈ ctx-switch rate (~500k/s mission, 4–5M/s futex)** |
| 2 | handoff_yields (407) | burst | **comparison only** (`burst < ran+H`) | per serial-handoff gate pass (rare in games, hot in mutex benches) |
| 3–9 | select_cpu ×3, enqueue ×2, enqueue_wake ×2, dispatch ×1 | task_slice | magnitude (slice grant, clamped) | ≤1 per event path; dispatch site only on keep-running |
| 10 | wake_vtime (341) | cadence_depth | magnitude (dose) | **already sunk behind the starved test by LLVM** — common path pays nothing |
| 11 | (counted in 3–9) | | | |

## Ranked targets

1. **frame_observe range gate, cross-multiplied (R.24a).** The divide runs on
   EVERY context switch, then the range gate discards the quotient for every
   non-frame-cadence task — the overwhelming majority. `per < MIN ⟺ delta <
   MIN·n`; `per > MAX ⟺ delta ≥ (MAX+1)·n`. Exact for `n < 2^32`; wide-`n`
   falls back to the original divide-first path (a 99-day-uptime 500 Hz
   compositor crosses 2^32 nvcsw — the fallback keeps it observed). Common
   case: ~15–25-cycle div → ~6–8 cycles of mul+cmp. At futex switch rates
   that is ~0.7–1.6% of one core.
2. **handoff_yields cross-multiply (R.24b).** `S/n ≤ ran || S/n − ran < H ⟺
   S < (ran+H)·n` — proven exact, one multiply replaces the divide.
   Fast arm gated on `(lim|n) < 2^32` (occupant on-CPU < 4.3 s and nvcsw
   < 4.3e9); slow arm keeps the original arithmetic verbatim.
3. **task_slice cap-arm pre-check** — SPECULATIVE, unregistered. `2S > cap·n ⟹
   want=cap` would skip the div for long-burst tasks but adds a mul+branch to
   the middle arm (the frame-cadence tasks). Needs burst-distribution data
   before it is worth a commit.
4. **Steal-ring walk** (93–98% of dispatches, ~1% hit) — the biggest dynamic
   expense in the object, but it is G25's registered territory (policy
   structure, not arithmetic). Not touched here.
5. **frame_observe call overhead from ops.running** — the `__noinline` call
   (r1–r5 clobber) is paid per switch even when the pre-check would exit
   immediately. Hoisting the pre-check inline into `cake_running` would
   duplicate the gate (drift risk per CLAUDE.md §comments). Follow-up only if
   R.24a's measured win says the remaining call overhead matters.

## Non-targets, with reasons

- `smp_processor_id` ×3 in select_cpu — JIT-inlined percpu load on this kernel.
- Repeated `ktime_get_ns` (handoff_yields, occupant_live) — freshness is
  deliberate (§R.13/§R.14) and holding `now` across kfuncs re-opens the spill
  war for ~5 ns.
- Branch census: 213 total; the unpredictable data-dependent ones (vtime,
  qmark) are already branchless house-style; the rest predict. No named target.

## Experiment registration

§R.24 in HYPOTHESES.md; endpoint = `--blocks 2` cake-vs-cake screen on
context-switch-heavy workloads (futex/pipe are the sensitive instruments),
plus byte-diff proof that both transforms leave every scheduling decision
bit-identical. bpf_stats per-callback ns unavailable (sysctl off, enable
retired as kernel-mutating — sudoless).
