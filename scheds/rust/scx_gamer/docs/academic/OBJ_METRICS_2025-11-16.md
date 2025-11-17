# LLVM Objdump Metrics – 2025-11-16

## Build Context
- **Git commit:** `c2c266ef34df4d7bee95ce3e2d08e98f75333698`
- **Build commands:** `cargo build -p scx_gamer` and `cargo build -p scx_gamer --release`
- **Artifacts**
  - Debug object: `target/debug/build/scx_gamer-2ebdf361e38ed70c/out/bpf.bpf.o`
  - Release object: `target/release/build/scx_gamer-4a17e8e6dd4222b7/out/bpf.bpf.o`
- **Tooling:** `llvm-objdump 21.1.5`

## Section Footprint (bytes)
| Section | Debug | Release | Notes |
| --- | ---: | ---: | --- |
| `.text` | 45,432 | 45,432 | ~5,679 BPF instructions across hot + slow paths |
| `.rodata` | 3,919 | 3,919 | Flag caches, wakeup format strings |
| `.bss` | 5,920 | 5,920 | Includes `hotpath_signals` and counters |
| `.maps` | 1,280 | 1,280 | 64 map descriptors (32 B each) |
| `.BTF` | 195,182 | 195,186 | +4 B delta from optimizer metadata |
| `.BTF.ext` | 178,500 | 178,500 | CO-RE relocation records |

Raw dumps: `OBJ_SECTIONS_DEBUG.txt`, `OBJ_SECTIONS_RELEASE.txt`.

## Largest `.text` Symbols (debug/release identical)
| Symbol | Bytes | Est. insns (÷8) | Notes |
| --- | ---: | ---: | --- |
| `wakeup_timerfn` | 11,824 | 1,478 | Tier-0 wakeup orchestration |
| `gamer_select_cpu_slowpath` | 10,376 | 1,297 | CPU selection slowpath (bounded loops) |
| `gamer_enqueue_slowpath` | 7,776 | 972 | Wakeup-chain + dsq routing |
| `pick_idle_cpu_cached` | 6,952 | 869 | Idle scan bounded by `FRAME_PHYS_SCAN_MAX` |
| `task_dl_with_ctx_cached` | 6,760 | 845 | Deadline heuristics with cached ctx |
| `task_slice` | 808 | 101 | Slice computation helper |
| `get_distributed_ringbuf_reserve` | 536 | 67 | Ring buffer dispatcher |
| `is_system_busy` | 304 | 38 | Foreground/system busy check |
| `load_preferred_cpu_safe` | 96 | 12 | Bounds-checked CPU loader |

Reference: `OBJ_SYMBOLS_DEBUG.txt`, `OBJ_SYMBOLS_RELEASE.txt`.

## struct_ops Entrypoint Sizes
| struct_ops section | Hex size | Bytes | Est. insns | Comment |
| --- | --- | ---: | ---: | --- |
| `struct_ops/gamer_select_cpu` | `0x60` | 96 | 12 | Hot head before tailcall |
| `struct_ops/gamer_select_cpu_tail` | `0x30` | 48 | 6 | Tailcall target |
| `struct_ops/gamer_enqueue` | `0x20` | 32 | 4 | Minimal wrapper calling slowpath |
| `struct_ops/gamer_enqueue_tail` | `0x20` | 32 | 4 | (Unused after verifier fix) |
| `struct_ops/gamer_dispatch` | `0x5a0` | 1,440 | 180 | Internal dsq fan-out |
| `struct_ops/gamer_cpu_release` | `0x10` | 16 | 2 | Cleanup |
| `struct_ops/gamer_runnable` | `0x87b8` | 34,744 | 4,343 | Runnable hot loop (Tier-1) |
| `struct_ops/gamer_running` | `0x0be8` | 3,048 | 381 | Runtime accounting |
| `struct_ops/gamer_stopping` | `0x7f08` | 32,520 | 4,065 | Tear-down logic |
| `struct_ops/gamer_enable` | `0x78` | 120 | 15 | Scheduler install |
| `struct_ops/gamer_disable` | `0x588` | 1,416 | 177 | Scheduler removal |
| `struct_ops.s/gamer_init_task` | `0x3e10` | 15,888 | 1,986 | Task bootstrap |
| `struct_ops.s/gamer_init` | `0x608` | 1,544 | 193 | Global init |
| `struct_ops/gamer_exit` | `0x258` | 600 | 75 | Teardown |

## Additional Notes
- Relocation dumps saved as `OBJ_RELOCS_DEBUG.txt` and `OBJ_RELOCS_RELEASE.txt` for CO-RE/helper auditing.
- Re-run the same commands after any refactor to track byte/insn deltas and ensure `.bss`/`.maps` stay within the verified envelope before shipping.

## Helper Call Census (debug build)
- Source table: `OBJ_HELPER_CENSUS_DEBUG.md`.
- Top helpers by invocation count:
  - `ktime_get_ns` (90), `map_lookup_elem` (80), `scx_bpf_now` (56), `map_lookup_percpu_elem` (44), `probe_read_kernel` (41), `map_update_elem` (41).
  - Struct ops taking advantage of storage APIs: `task_storage_get` (25 calls), `map_delete_elem` (12) to flush caches.
- Functions with the highest helper density:
  - `gamer_select_cpu_slowpath` (63 calls) — dominated by task storage + SCX dispatch helpers.
  - `wakeup_timerfn` (63) — heavy use of ring buffer + SCX instrumentation helpers.
  - `gamer_enqueue_slowpath` (59) — mixes DSQ helpers with hotpath telemetry.

## CO-RE Relocation Profile
- Detail file: `OBJ_RELOC_PROFILE.md`.
- Relocation totals: `R_BPF_64_64` (1,386), `R_BPF_64_32` (208), `R_BPF_64_ABS64` (16) — identical for debug/release builds.
- Most referenced symbols:
  - Timing: `scx_bpf_now` (112), `power_hint_level`/`_remaining_ns`/`_expiry_ns` (46/44/23).
  - Detection state: `detected_fg_tgid` (65), `foreground_tgid` (32), `no_stats` (32).
  - Storage/maps: `task_ctx_stor` (25), `cpu_ctx_stor` (41), `scx_bpf_dsq_insert` (27).
- These hotspots indicate where BTF CO-RE stability must be preserved when changing struct layouts.

## Map / BSS Heatmap Highlights
- Derived from relocation trace (`OBJ_MAP_BSS_HEATMAP.md`).
- Heavy hitters:
  - `power_hint_*` fields (46/44/23 refs) — concentrated in `gamer_stopping`, `gamer_runnable`, and hint maintenance helpers.
  - `interrupt_threads_map` (16) — touched by all IRQ detectors, validating that BSS optimizations replaced map churn.
  - `hotpath_signals` (5) — currently touched primarily by `gamer_enqueue_slowpath` and the input ISR, confirming the BSS-based wakeup channel is localized.
- Ring buffer fan-out lines show each `input_events_ringbuf_N` relocation resolves exactly once inside `get_distributed_ringbuf_reserve`, matching expectations for bounded helper usage.

## Function Size Histogram
- Full table: `OBJ_FUNCTION_HISTOGRAM.md`.
- Largest `.text` contributors mirror the earlier top list, validating the instruction budget:
  - `wakeup_timerfn` (11,824 bytes / ~1,478 insns).
  - `gamer_select_cpu_slowpath` (10,376 bytes / ~1,297 insns).
  - `gamer_enqueue_slowpath` (7,776 bytes / ~972 insns).
- Histogram is regenerated automatically to catch regressions >64 instructions in future patches.

## Automation
- Metrics are regenerated via `python scripts/objdump_metrics.py ...` inside `scheds/rust/scx_gamer/`.
- The helper produces:
  - `OBJ_HELPER_CENSUS_DEBUG.md`
  - `OBJ_RELOC_PROFILE.md`
  - `OBJ_MAP_BSS_HEATMAP.md`
  - `OBJ_FUNCTION_HISTOGRAM.md`
- Re-run after any BPF refactor to keep this summary in sync with verifier-facing data.

