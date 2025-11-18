# CPU Usage Validation Playbook

This note defines a repeatable workflow for proving that the esports profile
stays under the 3 % CPU envelope while preserving input / frame latency.

## 1. Preparation

1. Build the release artifacts (frame pointers stay enabled via `.cargo/config.toml`):
   ```sh
   cd /home/ritz/Documents/Repo/Linux/scx
   RUSTFLAGS="-C force-frame-pointers=yes" cargo build -p scx_gamer --release
   ```
2. Launch the scheduler in esports mode (detectors + tracing disabled by default):
   ```sh
   cd scheds/rust/scx_gamer
   sudo ./start.sh esports
   ```
3. Start the game or workload you want to profile and let it settle for 30–60 s.

## 2. `bpftop` snapshot (struct_ops share)

Capture the per-program CPU share to double-check that the targeted struct_ops
(`gamer_runnable`, `gamer_select_cp`, `gamer_stopping`, `gamer_dispatch`,
`gamer_enqueue`, `gamer_running`) dropped as expected.

```sh
sudo bpftop --interval 1000 --repeat 15 --sort cpu > /tmp/bpftop.log
```

- The command above runs for ~15 s and produces a logfile you can archive.
- Focus on the following row set:
  - `StructOps` entries for gamer_* functions
  - `Tracing` entries for `track_thread_ru` (should be near 0 unless enabled)
- Record the average and total CPU % in the table below.

| Timestamp | gamer_runnable | gamer_select_cp | gamer_stopping | gamer_dispatch | gamer_enqueue | gamer_running | track_thread_ru | Total scheduler % |
|-----------|----------------|-----------------|----------------|----------------|---------------|---------------|-----------------|-------------------|
|           |                |                 |                |                |               |               |                 |                   |

## 3. `perf` sample (userspace contention)

Use `perf` to make sure the userspace control plane stays well under 0.5 % CPU
and to capture hot stacks if it ever regresses.

```sh
SCX_PID=$(pidof scx_gamer)
sudo perf record -g -p "${SCX_PID}" -- sleep 15
sudo perf report --stdio --sort dso,symbol | head -n 40 > /tmp/perf-scx_gamer.txt
```

Checklist:
- `main::run` should dominate with idle / epoll waits.
- Threads `scx-power-monitor` or `scx-gpu-monitor` should stay <0.1 %.
- No unexpected wakeups (look for `tokio` reactors or busy `std::sync` paths).

## 4. Regression log

Copy `/tmp/bpftop.log` and `/tmp/perf-scx_gamer.txt` into `sessions/` (or paste
the key excerpts into your testing journal) every time we land a CPU-reduction
change. This makes it easy to compare before vs. after when a regression sneaks
in.

> Tip: If a run shows `track_thread_ru` >0.5 %, double-check that
> `set_runtime_trace()` is still disabled in esports mode or that `bpftrace`
> isn’t attached. The new toggles let you flip tracing on/off live without
> touching the scheduler.

