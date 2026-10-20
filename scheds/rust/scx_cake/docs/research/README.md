# Research and experimental patches

Verdicts live in the [STATE.md](../../STATE.md) pattern ledger, each with its kernel
need. This page indexes the investigations. Kernel patches now ship per feature in
[experimental-patches](https://github.com/RitzDaCat/experimental-patches)
`kernel/sched-ext/cachyos-<ver>/`; the host runs stock 7.2.10 since 2026-10-15.

**Legend** · ✅ kept / default · ❌ rejected · ➖ no effect · 🧪 open · ⏳ planned · 🐧 stock · 🧩 custom patch

| | Investigation | Status | Start here |
|---|---|---|---|
| ✅ | 🧩 Kernel patch: scalar kfuncs, fastcall, bounds, DSQ index, wake signals | ran on 7.2.9-custom; native CTZ/POPCNT default · fastcall, bounds ➖ · DSQ index untested · signals tag nothing on KWin + NVIDIA | [JIT prototype](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/README.md) · [full patch](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/0001-cake-kernel-full-signals-experimental-cachyos-7.2.8.patch) |
| ✅ | 🧩 All-in-one patch + lean idle claim (K1–K4) | ran from 10-11; `lazy` default since 10-12 | [patch](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/0001-cake-kernel-full-signals-idle-experimental-cachyos-7.2.9.patch) (sections in its header) |
| 🧪 | Cake/kernel caching | source review; DSQ direct index built into the patch | [cache review](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/CACHE_HOTPATH_REVIEW_2026-10-02.md) |
| ➖ | Unary fastcall | no measurable effect on hardware | [fastcall review](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/FASTCALL_KERNEL_REVIEW_2026-10-01.md) |
| 🧪 | BPF / compiler / JIT boundary | compiler probes, JIT inspection | [investigation](jit-20260929/README.md) |
| ➖ | ISA opportunities (10-01) | compiler probes and offline checks only, no timings; recommended fastcall first, which then measured ➖ | [report](isa-opportunities-20261001/README.md) |
| ❌ | Exact reciprocal quotient | removed 10-04 (BPF +5.8 %); sources kept | [implementation](https://github.com/RitzDaCat/experimental-patches/blob/main/kernel/sched-ext/research/jit-bitops-prototype/QUOTIENT_IMPLEMENTATION.md) · [sources](https://github.com/RitzDaCat/experimental-patches/tree/main/kernel/sched-ext/research/jit-bitops-prototype/quotient/) |
| ✅❌ | Counting, classifiers, inclusion–exclusion | spread pick ❌ · wide-host idle count, interlock, exit/census checks ✅ (committed) · K3 premise holds | [report](counting-classifiers-20261005/README.md) |
| ❌ | Block stash: wake classes from one line | no frame effect in KovaaK's; reverted | [report](block-stash-20261012/README.md) |
| ❌ | claimhome: lean `lazy` placement without the patch | placement reproduced; the frame win needs the cheaper claim too | [report](claimhome-20261012/README.md) |
| ❌ | burstkey: cosmos-style requeue key | no gain; cosmos itself loses to cake `lazy` in CPU-limited KovaaK's | [report](burstkey-20261012/README.md) |
| ❌ | sibwake: same-process wake → waker's SMT sibling | idle cores: lows −43 %; saturated: tails +6 % (weak) | [report](sibwake-20261012/README.md) |
| ⏳ | 🐧 CPU-bound tail (Deadlock: nightly worst 5.14 ms vs LAVD 2.84) | planned, nothing built; trace first, then 7 guarded ideas | [plan](cpu-bound-tail-20261014/README.md) |

| | Also |
|---|---|
| 📊 | [meeting page](../meeting/index.html) summarizes the work |
| 📏 | [cost audit](../AUDIT_OPERATION_COST_2026-09-15.md) explains evidence levels |
| ⚠️ | the prototype's `0002-scx-cake-opt-in-native-bitops.patch` is historical: do not apply it to this checkout |

## 📁 Rules for an investigation

| Rule |
|---|
| One directory each; objects, patches, harness and manifests stay together (moving them needs a provenance update) |
| Build outputs → repo `target/` · run outputs → `scx_cake_bench/runs/` |
| README states the **evidence level** (source review → model → verifier → VM → hardware → game) and the **kernel need** |
| A passed build or VM test is not a speedup |
