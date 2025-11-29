# scx_gamer Tools Reference

This document outlines the diagnostic, analysis, and development tools used for scx_gamer scheduler development and performance tuning.

---

## Performance Monitoring Tools

### thread_pressure_monitor.sh

**Location:** `scripts/thread_pressure_monitor.sh`

**Purpose:** AI-friendly diagnostic tool that generates comprehensive thread pressure reports. Designed for pasting into AI assistants (Claude, etc.) to analyze scheduling issues and suggest improvements.

**Why This Tool Exists:**
- CPU% tells you "cores are busy", NOT "tasks are stuck"
- PSI (Pressure Stall Information) shows actual thread wait times
- Provides structured output optimized for AI analysis

**Key Insight:**
- CPU 100% + PSI 0% = Good (doing work, not stuck)
- CPU 40% + PSI 5% = Bad (threads waiting = latency issues!)

**Usage Modes:**

```bash
# AI Report Mode (default) - structured text for AI analysis
./scripts/thread_pressure_monitor.sh              # Auto-detect game
./scripts/thread_pressure_monitor.sh Palworld     # Specific game
./scripts/thread_pressure_monitor.sh --pid 12345  # Specific PID

# JSON Mode - machine-readable output
./scripts/thread_pressure_monitor.sh --json
./scripts/thread_pressure_monitor.sh --json cs2

# Watch Mode - human-friendly live monitoring
./scripts/thread_pressure_monitor.sh --watch
./scripts/thread_pressure_monitor.sh --watch 2 cs2
```

**What the AI Report Includes:**

1. **System Information** - CPU, cores, kernel, memory
2. **CPU Topology** - cores, SMT status, cache hierarchy
3. **PSI Metrics** - CPU/Memory/IO pressure with gaming thresholds
4. **scx_gamer Status** - if running, thread classifications, dispatch stats
5. **Per-Thread Analysis** - wait time, runtime, wait%, context switches
6. **Thread Classifications** - INPUT, GPU_RENDER, GAME_LOGIC, AUDIO, etc.
7. **Analysis Guide** - interpretation hints for AI reasoning

**Thread Classification System:**
```
INPUT       - Mouse/keyboard/controller handling (most latency-critical)
GPU_RENDER  - Render, D3D, Vulkan, DXVK threads
GAME_LOGIC  - Main game loop, simulation, tick
AUDIO       - PulseAudio, PipeWire, audio mixing
NETWORK     - Steam, sockets, HTTP
WORKER_POOL - Thread pools, async workers
PHYSICS     - PhysX, collision detection
AI_NPC      - Pathfinding, NPC behavior
```

**Gaming Thresholds (built into tool):**
| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| CPU PSI | <1% | 2-5% | >5% |
| INPUT Wait% | <2% | 2-5% | >5% |
| GPU_RENDER Wait% | <10% | 10-15% | >15% |

**Example AI Analysis Workflow:**
```bash
# 1. Run the tool while gaming
./scripts/thread_pressure_monitor.sh YourGame > /tmp/thread_report.txt

# 2. Paste output into AI chat:
#    "Analyze this scx_gamer thread pressure report and suggest improvements:"
#    [paste report]

# 3. AI can identify:
#    - Which threads have high wait times
#    - Whether INPUT latency is a problem
#    - If scheduler is detecting threads correctly
#    - Suggested --profile or flag adjustments
```

---

### compare_metrics.sh

**Location:** `compare_metrics.sh`

**Purpose:** Compare scheduler metrics before/after a gaming session via the Debug API.

**Usage:**
```bash
# First, capture baseline (before gaming)
curl -s http://127.0.0.1:8080/metrics | jq '{...}' > /tmp/scx_metrics_before.json

# Then run after gaming session
./compare_metrics.sh
```

**What it shows:**
- Thread classification changes (input, GPU, audio, network, compositor)
- CPU utilization deltas
- Dispatch metrics (direct vs shared)
- Migration statistics
- Fentry event counts

---

### check-performance.sh

**Location:** `check-performance.sh`

**Purpose:** Quick sanity check that scx_gamer is running and detecting games.

**Usage:**
```bash
./check-performance.sh
```

**What it shows:**
- Scheduler service status
- Thread classification stats
- Recent game detection events
- Error/warning summary

---

## Cache Line Analysis Tools

These tools verify that hot-path data structures are properly aligned to 64-byte cache line boundaries. Misalignment causes false sharing and significant performance degradation.

### pahole_full_coverage.sh

**Location:** `scripts/pahole_full_coverage.sh`

**Purpose:** Complete cache line alignment report using pahole on compiled BPF code.

**Usage:**
```bash
./build.sh  # Must build first
./scripts/pahole_full_coverage.sh
```

**What it shows:**
- Size and alignment status for all BPF structs
- Priority structs (task_ctx, cpu_ctx) alignment verification
- Cache line count per struct

**Output example:**
```
task_ctx                         512 bytes   8 cache lines  42 members  ALIGNED
cpu_ctx                          192 bytes   3 cache lines  15 members  ALIGNED
```

---

### analyze_cache_lines.sh

**Location:** `scripts/analyze_cache_lines.sh`

**Purpose:** Detailed cache line boundary analysis showing exactly which fields cross boundaries.

**Usage:**
```bash
./scripts/analyze_cache_lines.sh
```

**What it shows:**
- Per-field offset within cache lines
- Visual cache line boundary markers
- Fields that straddle boundaries (bad for performance)

---

### verify_cache_alignment.sh

**Location:** `scripts/verify_cache_alignment.sh`

**Purpose:** Quick verification of cache alignment by compiling BPF code with debug symbols.

**Usage:**
```bash
./scripts/verify_cache_alignment.sh
```

---

### cache_line_analyzer.py

**Location:** `scripts/cache_line_analyzer.py`

**Purpose:** Python-based analysis for more complex struct layout visualization.

---

## Build and Installation Tools

### build.sh

**Location:** `build.sh`

**Purpose:** Build scx_gamer with optimized release settings.

**Usage:**
```bash
./build.sh
```

---

### start.sh

**Location:** `start.sh`

**Purpose:** Interactive launcher with preset profiles for different gaming scenarios.

**Profiles:**
1. **Baseline** - Clean defaults
2. **Casual Gaming** - Balanced settings
3. **Esports** - Aggressive competitive tuning
4. **NAPI Preference** - Network-aware scheduling

**Usage:**
```bash
./start.sh
```

---

### install-cachyos.sh / uninstall-cachyos.sh

**Location:** Root directory

**Purpose:** CachyOS-specific installation with GUI integration.

**Usage:**
```bash
sudo ./install-cachyos.sh   # Install with CachyOS integration
sudo ./uninstall-cachyos.sh # Clean uninstall
```

---

### verify-installation.sh

**Location:** `verify-installation.sh`

**Purpose:** Comprehensive installation verification and health check.

**Usage:**
```bash
./verify-installation.sh
```

---

## Verification Tools

### verify_lmax.sh

**Location:** `verify_lmax.sh`

**Purpose:** Verify LMAX Disruptor optimizations are active (distributed ring buffers).

**Usage:**
```bash
./verify_lmax.sh
```

**What it shows:**
- BPF ring buffer map count (expecting 16 for distributed buffers)
- Instructions for detailed logging
- Expected performance improvements

---

## External Tools Used

These are standard Linux tools that are useful for scx_gamer development:

### pahole (dwarves package)

Analyze struct layouts from compiled binaries.

```bash
# Install
sudo pacman -S pahole  # Arch
sudo apt install dwarves  # Debian/Ubuntu

# Usage
pahole -C task_ctx ./target/release/build/scx_gamer-*/out/bpf.bpf.o
```

### bpftool

Inspect BPF programs and maps.

```bash
# Install
sudo pacman -S bpftool

# List loaded BPF programs
sudo bpftool prog list

# List BPF maps
sudo bpftool map list
```

### perf

Linux performance profiling.

```bash
# Record scheduler events during gaming
sudo perf record -e sched:* -a sleep 10

# Analyze
sudo perf report
```

### stress-ng

Generate CPU/memory/IO load for testing.

```bash
# CPU stress (useful for PSI testing)
stress-ng --cpu 8 --timeout 30s
```

---

## Quick Reference: PSI Thresholds for Gaming

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| CPU PSI avg10 | <1% | 2-5% | >5% |
| Memory PSI | 0% | >0.5% | >2% |
| IO PSI | <0.5% | >1% | >3% |

**Reading PSI:**
```bash
# Quick check
cat /proc/pressure/cpu
# Output: some avg10=0.00 avg60=5.23 avg300=2.10 total=1234567
#         ^avg60=5.23 means 5.23% of time tasks were waiting for CPU
```

---

## Debug API Endpoints

When running with `--http-port 8080`:

| Endpoint | Description |
|----------|-------------|
| `GET /metrics` | Full scheduler metrics JSON |
| `GET /threads` | Thread classification breakdown |
| `GET /health` | Scheduler health status |

```bash
# Get all metrics
curl -s http://127.0.0.1:8080/metrics | jq .

# Watch metrics live
watch -n 1 'curl -s http://127.0.0.1:8080/metrics | jq ".psi_cpu_some_avg10, .input_handler_threads"'
```

---

## Adding New Tools

When creating new diagnostic tools:

1. Place scripts in `scripts/` directory
2. Make executable: `chmod +x scripts/your_tool.sh`
3. Add documentation to this file
4. Follow existing patterns:
   - Use color output for readability
   - Include usage examples in `--help`
   - Handle missing dependencies gracefully

