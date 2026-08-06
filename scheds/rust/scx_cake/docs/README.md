# scx_cake/docs — what is here, and where the rest went

**2026-08-06 cleanup: 261 MB / 149 files → 2.3 MB / 22 top-level docs. Nothing was
deleted.** Everything removed is in the vault, compressed, and listed below.

`docs/*` is gitignored (`.gitignore:2`); tracked files are force-added with
`git add -f`. That means **an untracked doc exists only on this machine** — if a
doc is worth citing from `STATE.md` or `HYPOTHESES.md`, track it.

## Start here, not in a dated file

| Question | Where |
|---|---|
| Current state, scoreboard, open gaps | [`../STATE.md`](../STATE.md) |
| Which experiment to run next | [`../HYPOTHESES.md`](../HYPOTHESES.md) |
| Rules, design laws, invariants | [`../CLAUDE.md`](../CLAUDE.md) |
| How the scheduler behaves | [`../DESIGN.md`](../DESIGN.md) |
| How to build / bench | the `sched-ext-dev` skill |

## What moved to the vault

Both archives are under
`~/Documents/Repo/scx_cake_bench/history/imported_from_scx_repo/scx_cake_docs_2026-08-06/`,
following the precedent in `scx_cake_bench/COMPACTION_2026-08-01.md`.

| archive | was | now | contents |
|---|---|---|---|
| `docs_analysis_2026-05-23.tar.zst` | 253 MB, 122 files | **8.5 MB** (30×) | `ml_analysis_*`, `benchmark_asset_*`, `perf_helps_hurts_atlas`, `code_pattern_matrix`, `full_suite_mesh`, `positive_code_patterns`, `mixed_cache_memcpy_*`, 4× `frames_*.csv` |
| `docs_session_notes_pre_2026-07.tar.zst` | 1.04 MB, 78 files | **228 KB** | dated session notes older than 30 days, none tracked, none cited |

Restore any of it:

```bash
zstd -dc ~/Documents/Repo/scx_cake_bench/history/imported_from_scx_repo/scx_cake_docs_2026-08-06/docs_analysis_2026-05-23.tar.zst | tar -xf - -C /tmp
```

Seven superseded but *tracked* docs moved to [`archive/`](archive/) via `git mv`, so
their history follows them: `CAMPAIGN_G22_INVESTIGATIONS`, `flight_recorder_telemetry`,
`queue_policy_latency_findings`, `benchmark_capture_workflow`,
`benchmark_checkpoint_2026-05-15_nightly`, `benchmark_debrief_2026-05-14`,
`mutation_campaign_2026-06-10`.

## Selection rule used

A file was archived only if **all three** held: untracked, no inbound citation from
`STATE.md` / `HYPOTHESES.md` / `CLAUDE.md` / `README.md` / `DESIGN.md` / code, and
mtime older than 30 days. Anything failing one test stayed.

`ANALYSIS_DIABLO_CHOP_2026-08-02.md` has zero citations and was **kept anyway** — git
log shows Diablo runs 9–12 with the newest at `HEAD~4`, so it is an active campaign.
Citation count alone is not a liveness test.

## Two integrity bugs this audit found

1. **`CAKE_113_VS_120_DELTA_2026-07-27.md` was cited from `HYPOTHESES.md:108` and
   `STATE.md:2438` while untracked** — the citation resolved only on this machine.
   Fixed: `git add -f`.
2. **`docs/APP_SIMULATION.md` is cited but does not exist here.** The real file is
   `scx_cake_bench/docs/APP_SIMULATION.md` — a cross-repo reference written as a
   local path. Still to fix: qualify the citation with its repo.
