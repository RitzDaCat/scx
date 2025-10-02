scx_gamer — sched_ext gaming scheduler

## ⚠️ Experimental / AI-Assisted Development

This scheduler is **experimental** and was developed with significant AI assistance as a test of AI capabilities in producing functional kernel scheduling code. The primary goals are to explore whether AI can help reduce frame latency and input latency in gaming workloads, and to evaluate AI-generated code quality in a complex systems programming context.

**Use at your own risk.** This is a research/testing project and may not be suitable for production use.

## Overview
scx_gamer is a Linux sched_ext (eBPF) scheduler that attempts to minimize input latency and frame-time variance for games. It favors task–CPU locality under light/medium load and switches to a global EDF-like policy under heavy load to sustain responsiveness. It provides input- and frame-aware boost windows, NUMA and SMT-aware placement, and a lightweight userspace control loop with monitoring.

## Goals
- Preserve cache locality and reduce needless migrations when not overloaded
- Deliver consistent frame pacing under load (reduce 1%/0.1% low spikes)
- Optimize input-to-photon latency with input and frame windows
- Stay safe: cleanly detach to CFS on exit or stall
- Test AI capabilities in systems programming and scheduler development

## Architecture at a glance
- **Userspace (Rust)**:
  - CLI argument parsing and topology detection
  - BPF skeleton management and lifecycle
  - Event-driven loop via epoll for input device monitoring
  - Statistics collection and monitoring
  - Watchdog for automatic CFS fallback on stalls

- **BPF (C)**:
  - Per-CPU dispatch queues (DSQs) with round-robin
  - Global EDF fallback when system load increases
  - Migration limiting to preserve cache locality
  - mm-affinity hints (LRU-based) for same-address-space tasks
  - NUMA-aware placement when enabled
  - SMT contention avoidance
  - Input-window boost for low-latency input handling
  - Configurable wakeup timer for idle CPU management

## How it works
1. **Local-first scheduling**: Under light load, tasks dispatch from per-CPU queues (round-robin) to maximize cache locality and minimize contention

2. **Load-aware transitions**: When CPU utilization exceeds configured thresholds, switches to global EDF mode for better load balancing under pressure

3. **Input window boost**: When input device activity is detected (via evdev monitoring), tasks receive shorter slices and relaxed migration limits for responsive input handling

4. **Placement decisions** consider:
   - NUMA boundaries (when enabled)
   - mm-affinity hints for same-address-space tasks
   - SMT contention avoidance
   - Per-task migration rate limiting

5. **Safety mechanisms**: Clean shutdown via SIGINT/SIGTERM, watchdog for automatic CFS fallback on dispatch stalls

## Intended benefits
- Cache locality preservation for game and render threads
- Load-aware scheduling transitions for responsiveness under pressure
- Input-window boost for reduced input latency
- NUMA and SMT awareness to avoid cross-node/sibling contention

**Note**: Results vary significantly based on workload, hardware topology, and configuration.

## Requirements
- Linux kernel with sched_ext enabled (6.12+)
- Root privileges to attach BPF scheduler
- Input devices accessible via `/dev/input/event*` for input monitoring (optional)

## Build
```bash
# From repository root
cargo build -p scx_gamer --release
```

## Usage

### Direct run (recommended)
Foreground execution allows clean shutdown with Ctrl+C:

```bash
sudo ./target/release/scx_gamer --stats 1
```

With input monitoring and optimizations:
```bash
sudo ./target/release/scx_gamer \
  --stats 1 \
  --input-window-us 2000 \
  --mm-affinity \
  --avoid-smt \
  --preferred-idle-scan
```

### Via scx_loader (system service)
```bash
# Using system scx_loader
sudo scx_loader --set scx_gamer

# Check status
scxctl status

# Stop scheduler
sudo systemctl stop scx_loader
```

### Clean shutdown
- **Direct run**: Ctrl+C triggers clean detachment and restores CFS
- **Watchdog**: `--watchdog-secs N` auto-exits if no dispatch progress detected

## CLI Reference

### Core scheduling
- `-s, --slice-us <u64>` (10): Maximum scheduling slice in microseconds
- `-l, --slice-lag-us <u64>` (20000): Maximum vtime debt per task in microseconds
- `-p, --polling-ms <u64>` (0): Deprecated/no-op (in-kernel sampling used)

### CPU topology
- `-m, --primary-domain <list|keyword>`: CPU priority set
  - Accepts: comma-separated list (e.g., `0-3,12-15`)
  - Keywords: `turbo`, `performance`, `powersave`, `all` (default)
- `-n, --enable-numa`: Enable NUMA-aware placement
- `-f, --disable-cpufreq`: Disable CPU frequency control

### Idle CPU selection
- `-i, --flat-idle-scan`: Simple idle scan (lower overhead)
- `-P, --preferred-idle-scan`: Prioritize higher-capacity CPUs
- `--disable-smt`: Disable SMT placement (requires idle scan mode)
- `-S, --avoid-smt`: Aggressively avoid SMT sibling contention

### Task placement
- `-w, --no-wake-sync`: Disable direct dispatch on sync wakeups
- `-d, --no-deferred-wakeup`: Disable deferred wakeups (may reduce power)
- `-a, --mm-affinity`: Keep same-address-space tasks on same CPU

### Migration control
- `--mig-window-ms <u64>` (50): Migration limiter window in milliseconds
- `--mig-max <u32>` (3): Max migrations per task per window

### Input boost
- `--input-window-us <u64>` (2000): Input-active boost window (µs). 0=disabled
- `--prefer-napi-on-input`: Prefer NAPI/softirq CPUs during input
- `--foreground-pid <u32>` (0): Restrict boost to this TGID. 0=global

### Memory affinity
- `--disable-mm-hint`: Disable per-mm cache affinity hints (enabled by default)
- `--mm-hint-size <u32>` (8192): mm hint LRU size (128-65536)

### System
- `--wakeup-timer-us <u64>` (500): Wakeup timer period (min 250µs)
- `--event-loop-cpu <usize>`: Pin event loop to specific CPU (auto-selected by default)
- `--watchdog-secs <u64>` (0): Auto-exit to CFS after N seconds of stall. 0=disabled

### Monitoring
- `--stats <sec>`: Print statistics every N seconds
- `--monitor <sec>`: Monitor-only mode (don't attach scheduler)
- `--help-stats`: Show metric descriptions
- `-v, --verbose`: Enable verbose output
- `-V, --version`: Print version

### Debug
- `--exit-dump-len <u32>` (0): BPF exit dump buffer length

## Configuration Examples

### Balanced gaming setup
```bash
sudo ./target/release/scx_gamer \
  --stats 1 \
  --input-window-us 2000 \
  --mm-affinity \
  --avoid-smt \
  --preferred-idle-scan
```

### Low-latency competitive gaming
```bash
sudo ./target/release/scx_gamer \
  --stats 1 \
  --input-window-us 1000 \
  --slice-us 5 \
  --preferred-idle-scan \
  --avoid-smt \
  --mig-max 1
```

### Power-efficient gaming
```bash
sudo ./target/release/scx_gamer \
  --stats 1 \
  --primary-domain powersave \
  --no-deferred-wakeup
```

## Monitoring
The `--stats` option prints periodic statistics including:
- Enqueue/dispatch counts (local vs shared)
- Migration statistics and blocks
- CPU utilization (instant and EMA)
- Input event counts

Use `--help-stats` for detailed metric descriptions.

## Design Notes
- **Load detection**: In-kernel CPU utilization sampling with EMA-based mode transitions
- **Migration limiting**: Per-task rate limiting to preserve cache affinity
- **Input boost**: evdev-based input detection triggers short-lived boost windows
- **mm-affinity**: LRU-based hints to keep same-address-space tasks co-located
- **SMT awareness**: Configurable sibling avoidance to reduce contention
- **Safety**: Clean SIGINT/SIGTERM handling and optional watchdog for stall detection

## Troubleshooting

**Can't stop with Ctrl+C**
- Ensure running in foreground (not via scx_loader/systemd)
- If using scx_loader: `sudo systemctl stop scx_loader`

**Input monitoring not working**
- Check `/dev/input/event*` permissions
- Run with `-v` for verbose device detection logs

**High CPU usage from event loop**
- Event loop auto-pins to low-capacity CPU by default
- Manually specify with `--event-loop-cpu N` if needed

**Performance worse than CFS**
- Try different flag combinations (see Configuration Examples)
- Some games may not benefit from this scheduling approach
- Compare with established schedulers (scx_lavd, scx_bpfland)

## Testing and Validation

This scheduler is experimental. When evaluating:
- **Baseline**: Compare against CFS and established sched_ext schedulers
- **Metrics**: Measure frametime percentiles (P99, P99.9), input latency, stutters
- **Scenarios**: Test with/without OBS capture, various game engines, different CPU loads
- **Hardware**: Results vary significantly by CPU topology (E/P-cores, SMT, NUMA)

## Glossary
- **DSQ**: Dispatch queue
- **EDF**: Earliest-deadline-first scheduling
- **EMA**: Exponential moving average
- **CFS**: Completely Fair Scheduler (Linux default)
- **SMT**: Simultaneous multithreading (HyperThreading)

## License
GPL-2.0-only

## Author
RitzDaCat

