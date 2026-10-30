# scx_cake

scx_cake is a Linux sched_ext CPU scheduler tuned for games and fine for
everyday use. It aims for three things: no task waits while a CPU sits
free, tasks stay near their warm cache, and CPU time stays fair. It
decides from how tasks behave and how the CPUs are laid out, never from a
game name or a profile. The name comes from the CAKE network queue
manager.

This README describes this checkout. An installed package may differ, so
check `scx_cake --version` and `scx_cake --help`.

## Build and run

Cake needs a kernel with `CONFIG_SCHED_CLASS_EXT=y`, BPF and BTF (see
[kernel.config](../../../kernel.config)), the repository's
[Rust toolchain](../../../rust-toolchain.toml), Clang with BPF support
and the [build dependencies](../../../README.md#build--install). Loading
a scheduler needs permission from your scheduler service or an admin.

```sh
cargo build --locked --release -p scx_cake
./target/release/scx_cake --version          # version and binary hash
./target/release/scx_cake --print-topology   # CPU layout
./target/release/scx_cake                    # run the default policy
```

`./scheds/rust/scx_cake/cake build` builds it too. `--version` and
`--print-topology` attach nothing.

While cake runs, `cat /sys/kernel/sched_ext/root/ops` starts with
`cake`. Ctrl+C stops it and gives the CPUs back to the kernel's default
scheduler. The kernel removes cake if a runnable task waits 5 s. That
watchdog is a safety net, not a latency promise.

## Stock and patched kernels

Cake runs on any kernel with sched_ext. The
[experimental-patches](https://github.com/RitzDaCat/experimental-patches)
kernel adds speedups. Cake checks the running kernel at start, never the
build machine, and falls back when a feature is missing.

| Feature | Needs | Without it |
|---|---|---|
| Cached rq clock in `ops.running` | stock 6.14 or newer | `scx_bpf_now()` |
| Native CTZ/POPCNT | patch 1, and x86 BMI1 and POPCNT | portable bit math |
| Lean idle claim, `lazy` by default | patch 2 | regular idle claim, no warning |
| Direct DSQ index | patch 3, kernel side only | hash lookup |
| `--kernel-signals` (input and fence wakes, IRQ time) | patch 4 | reports it unavailable and runs normally |

Patch numbers follow the `kernel/sched-ext/cachyos-<ver>/` series in
experimental-patches. Measured results are in the pattern ledger in
[STATE.md](./STATE.md).

## Flags

The default needs no flags. Apart from the inspect flags, every flag is
for testing.

| Flag | Default | Does |
|---|---|---|
| `-h`, `--help` | | list flags and toggles, then exit |
| `-V`, `--version` | | print the version and binary SHA-256, then exit |
| `--print-topology` | | print the CPU layout and core preferences, then exit |
| `-v`, `--verbose` | off | libbpf output, startup detail, IRQ-sink changes, kernel event counts at exit |
| `--inspect-bpf NEW_DIR` | off | load the BPF, save its instructions, exit without attaching |
| `--toggle NAME=0\|1` | | set a toggle; repeatable, the last one wins |
| `--handoff-ns NS` | 1464 | change the handoff floor: the slice floor, serial handoff, sub-handoff and deep-idle cutoffs |
| `--lean-idle off\|on\|lazy` | `lazy` | lean idle claim (patch 2); warns only when asked for explicitly and missing |
| `--native-bitops` | auto | require native CTZ/POPCNT; fail without them |
| `--portable-bitops` | off | force portable CTZ/POPCNT |
| `--native-ring` | off | native-CTZ queue scan; only for hosts with one LLC and at most 64 CPU IDs |
| `--native-hist` | off | native CLZ for the probe histograms, used only with `probe=1` |
| `--kernel-signals off\|observe\|on` | off | patch 4: `observe` counts the signals, `on` also uses them for placement |
| `--signal-report NEW.csv` | off | save the signal counters at exit; needs `observe` or `on` |

The `probe` toggle defaults to `0`. Set to `1`, it prints placement and
delay diagnostics at exit, separate from `-v`.

Unknown options, such as the old `--profile gaming` or `-p powersave`
from `scx_loader`, and bad toggles are skipped with a warning. Cake still
starts.

## How scheduling works

LLC is the last-level cache that a group of CPUs shares. A DSQ is a
sched_ext dispatch queue. Each CPU owns one queue, and each LLC has one
shared wake pool.

When a task wakes, cake tries to claim an idle CPU for it. If it gets one,
the task runs there next. If not, enqueue preempts a task that used up
its turn, or puts the task in its LLC's wake pool or its CPU's queue. A
task that uses up its slice goes back to its CPU's queue. A CPU that needs
work takes offers from other LLCs first, then its own queue or the pool,
oldest first, and steals last.

Placement weighs a warm cache, whole idle cores, SMT siblings, big cores
first on hybrid CPUs, the platform core ranking and IRQ load. IRQ load
counts the measured share, a handler running now and the next tick.

A seat is a CPU that a pipeline-stage task, one with a mean burst of
64 us or more, returns to. It is a preference, not a reservation.

Tasks are ordered by virtual runtime: CPU time scaled by nice weight.

The slice comes from the task's own run time, age and voluntary switches.
The floor is 1464 ns and the cap is 1.5 ms. Kernel threads placed
directly get a flat 3 ms.

The full rules are in [DESIGN.md](./DESIGN.md).

## Limits

| Limit | Value | Past it |
|---|---|---|
| Fast idle claim, seats, one pool per LLC | CPU IDs 0 to 63 | kernel idle picker, one shared pool |
| Wake pools | 16 LLCs | one shared pool |
| Cache-aware steal order | CPU IDs 0 to 127 | plain ring walk |
| CPU ID span | 1024, possible CPUs included | refuses to start |
| Starvation fallback | fixed 24 ms | dispatch serves the waiting pool first ([DESIGN.md](./DESIGN.md#-starvation-fallback-24-ms)) |

## Tests and bug reports

```sh
cargo test --locked -p scx_cake
```

The tests never attach. The verifier-load test is ignored because it
needs BPF permission.

A bug report needs the version or commit, the kernel version, the CPU,
the launch arguments, and the scheduler and kernel logs.

## Source

| File | Purpose |
|---|---|
| [src/main.rs](./src/main.rs) | loader: flags, topology, attach, IRQ-sink monitor |
| [src/core_performance.rs](./src/core_performance.rs) | platform core ranking, read once at start |
| [src/bpf/cake.bpf.c](./src/bpf/cake.bpf.c) | scheduling policy |
| [src/bpf/wake-signals.bpf.h](./src/bpf/wake-signals.bpf.h) | `--kernel-signals` reads |
| [src/bpf/intf.h](./src/bpf/intf.h) | shared constants and limits |
| [DESIGN.md](./DESIGN.md) | policy and known limits |
| [STATE.md](./STATE.md) | status, pattern ledger, owed work |
| [docs/README.md](./docs/README.md) | documentation map |
| [bench/README.md](./bench/README.md) | bench tools |

License: GPL-2.0-only.
