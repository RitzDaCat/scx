# scx_gamer v2.0: Technical Design Document

**Version:** 2.0.0  
**Status:** Active Development  
**Last Updated:** 2025-12-04

---

## Table of Contents

### Part I: Foundation
1. [Mission and Goals](#1-mission-and-goals)
2. [Architecture Overview](#2-architecture-overview)
3. [Code Organization Rules](#3-code-organization-rules)

### Part II: Specification
4. [Data Structures](#4-data-structures)
5. [Detection Hooks](#5-detection-hooks)
6. [Priority System](#6-priority-system)
7. [CPU Selection](#7-cpu-selection)
8. [Dispatch Logic](#8-dispatch-logic)
9. [Userspace Components](#9-userspace-components)

### Part III: Implementation
10. [File Manifest](#10-file-manifest)
11. [Implementation Phases](#11-implementation-phases)
12. [Testing and Validation](#12-testing-and-validation)

### Part IV: Reference
13. [Research Background](#13-research-background)
14. [Glossary](#14-glossary)

---

# Part I: Foundation

## 1. Mission and Goals

### 1.1 Mission Statement

scx_gamer is a Linux sched_ext scheduler optimized for competitive gaming. It prioritizes **input latency**, **frametime consistency**, and **audio stability** over fairness.

### 1.2 Target Metrics

| Metric | Target | Measurement |
|--------|--------|-------------|
| Input-to-frame latency | <1ms | From `hid_irq_in` to `drm_atomic_commit` |
| 99th percentile frametime | ≤4.17ms (240Hz) | `scripts/game_perf_monitor.sh` |
| Frames >5ms | <0.5% | Frametime histogram |
| FPS standard deviation | <50 | Over 60-second windows |
| Audio underruns | 0 | PulseAudio/PipeWire stats |

### 1.3 Target Hardware

```
CPU:     AMD Ryzen 9800X3D (8C/16T, 3D V-Cache)
GPU:     NVIDIA RTX 4090
Input:   8kHz mouse, 8kHz keyboard (Wooting)
Display: 480Hz (1080p) / 240Hz (4K)
OS:      Arch Linux, CachyOS kernel 6.17+
```

### 1.4 Design Principles

#### P1: 100% Proof, Zero Guesswork
Every scheduling decision must be traceable to a kernel hook. No behavioral heuristics.

```c
// CORRECT: Hook-based classification
SEC("fentry/drm_ioctl")
int detect_gpu() {
    tctx->flags |= FLAG_GPU;  // Kernel proof
}

// WRONG: Behavioral guess
if (runtime < 100us && wakeups > 1000/sec)
    tctx->is_interactive = true;  // Heuristic - forbidden
```

#### P2: A.B.C. - Always Be Casting
When we detect an event, prepare CPUs for the work that WILL follow.

```c
// Input detected → kick low-priority task → CPU ready BEFORE game thread wakes
SEC("fentry/hid_irq_in")
int input_detected() {
    kick_lowest_priority_cpu();  // Proactive preparation
}
```

#### P3: Radical Simplicity
Prefer 10 lines of obvious code over 5 lines of clever code. Every line must justify its existence.

```c
// CORRECT: Obvious
u64 deadline = vtime + (runtime >> boost_shift);

// WRONG: Clever but obscure
u64 deadline = vtime + ((runtime * weight_table[nice]) >> (boost_shift + SCALE_FACTOR));
```

#### P4: DRY - Don't Repeat Yourself
Common patterns go in helpers. If you're copying code, make a function.

#### P5: Fail Loudly
Errors should be visible, not hidden. Debug stats should expose all decisions.

---

## 2. Architecture Overview

### 2.1 System Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              USERSPACE                                       │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐        │
│  │   main.rs   │  │  focus.rs   │  │  stats.rs   │  │   args.rs   │        │
│  │  BPF loader │  │  D-Bus/KWin │  │  Display    │  │    CLI      │        │
│  │  Event loop │  │  Focus tgid │  │  Metrics    │  │   Parsing   │        │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘  └─────────────┘        │
│         │                │                │                                  │
│         └────────────────┼────────────────┘                                  │
│                          │ BPF Maps                                          │
└──────────────────────────┼───────────────────────────────────────────────────┘
                           │
┌──────────────────────────┼───────────────────────────────────────────────────┐
│                     KERNEL (BPF)                                             │
│                          │                                                   │
│  ┌───────────────────────┴───────────────────────┐                          │
│  │              main.bpf.c (~400 lines)           │                          │
│  │  struct_ops callbacks only:                    │                          │
│  │  - ops.select_cpu()  → cpu_select.bpf.h       │                          │
│  │  - ops.enqueue()     → enqueue.bpf.h          │                          │
│  │  - ops.dispatch()    → dispatch.bpf.h         │                          │
│  │  - ops.running()     → (inline)               │                          │
│  │  - ops.stopping()    → (inline)               │                          │
│  │  - ops.init_task()   → (inline)               │                          │
│  │  - ops.init()        → (inline)               │                          │
│  └───────────────────────────────────────────────┘                          │
│                          │                                                   │
│         ┌────────────────┼────────────────┐                                  │
│         │                │                │                                  │
│         ▼                ▼                ▼                                  │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐                          │
│  │  detection/ │  │  priority/  │  │    core/    │                          │
│  │             │  │             │  │             │                          │
│  │ input.bpf.h │  │ boost.bpf.h │  │enqueue.bpf.h│                          │
│  │   gpu.bpf.h │  │             │  │dispatch.bpf.│                          │
│  │ audio.bpf.h │  │             │  │cpu_sel.bpf.h│                          │
│  │  sync.bpf.h │  │             │  │             │                          │
│  │   net.bpf.h │  │             │  │             │                          │
│  └─────────────┘  └─────────────┘  └─────────────┘                          │
│         │                │                │                                  │
│         └────────────────┼────────────────┘                                  │
│                          │                                                   │
│  ┌───────────────────────┴───────────────────────┐                          │
│  │           Shared Headers                       │                          │
│  │  types.bpf.h   - task_ctx, cpu_ctx, maps      │                          │
│  │  config.bpf.h  - tunables, constants          │                          │
│  │  helpers.bpf.h - utility functions            │                          │
│  └───────────────────────────────────────────────┘                          │
│                                                                              │
└──────────────────────────────────────────────────────────────────────────────┘
```

### 2.2 Data Flow

```
Input Event Flow:
─────────────────
HID IRQ → fentry/hid_irq_in → set FLAG_INPUT_PENDING → kick CPU
                                                           │
Game Thread Wakes → select_cpu() → check FLAG_INPUT_PENDING
                                           │
                         ┌─────────────────┴─────────────────┐
                         │ FLAG set? Use prepared CPU        │
                         │ FLAG not set? Normal selection    │
                         └───────────────────────────────────┘

GPU Submit Flow:
────────────────
Game calls drm_ioctl() → fentry/drm_ioctl → set FLAG_GPU on task_ctx
                                                    │
                              ┌─────────────────────┴─────────────────────┐
                              │ boost_shift = 6 (64x priority)            │
                              │ Prefer physical core                      │
                              │ Avoid SMT sibling if possible             │
                              └───────────────────────────────────────────┘
```

---

## 3. Code Organization Rules

### 3.1 File Structure Requirements

```
src/
├── bpf/
│   ├── main.bpf.c              # ONLY struct_ops callbacks + includes (~400 lines max)
│   ├── intf.h                  # Userspace ↔ BPF interface (shared structs)
│   │
│   └── include/
│       ├── config.bpf.h        # ALL tunables and constants (single source of truth)
│       ├── types.bpf.h         # ALL data structures and maps
│       ├── helpers.bpf.h       # Shared utility functions (lookups, macros)
│       │
│       ├── detection/          # Fentry hooks (one file per category)
│       │   ├── input.bpf.h     # input_event, hid_irq_in, hid_input_report
│       │   ├── gpu.bpf.h       # drm_ioctl, dma_fence_signal, drm_atomic_commit
│       │   ├── audio.bpf.h     # ALSA ioctl detection
│       │   ├── sync.bpf.h      # eventfd, futex, ntsync (Wine/Proton)
│       │   ├── net.bpf.h       # UDP/TCP gaming traffic
│       │   └── game.bpf.h      # LSM hooks for game process detection
│       │
│       ├── priority/
│       │   └── boost.bpf.h     # boost_shift calculation, deadline formula
│       │
│       └── core/
│           ├── cpu_select.bpf.h  # CPU selection logic
│           ├── enqueue.bpf.h     # Task enqueue decisions
│           └── dispatch.bpf.h    # DSQ → CPU dispatch
│
├── main.rs                      # Entry point, BPF loading, event loop
├── args.rs                      # CLI argument parsing (clap)
├── focus.rs                     # D-Bus focus detection
├── stats.rs                     # Statistics display
└── topology.rs                  # CPU topology helpers
```

**Note:** This is a binary crate, not a library. No `lib.rs` needed.

### 3.2 File Size Limits

| File Type | Max Lines | Rationale |
|-----------|-----------|-----------|
| `main.bpf.c` | 400 | Callbacks only, all logic in headers |
| `config.bpf.h` | 150 | Constants and tunables only |
| `types.bpf.h` | 250 | Data structures and maps |
| `helpers.bpf.h` | 200 | Shared utility functions |
| Detection headers | 300 each | One responsibility per file |
| Core headers | 400 each | May have more logic |
| `priority/boost.bpf.h` | 200 | Priority calculation only |
| Rust files | 400 each | Split if larger |
| `main.rs` | 300 | Entry point, delegates to modules |

**If a file exceeds its limit, it MUST be split.** No exceptions.

**Why strict limits matter:**
- Forces modular design
- Makes code reviewable
- Easier for AI to understand in context windows
- Prevents "god files" that accumulate complexity

### 3.3 Naming Conventions

#### BPF Files
```c
// Files: lowercase_with_underscores.bpf.h
// Functions: lowercase_with_underscores
// Structs: lowercase_with_underscores (no typedef wrappers)
// Macros/Constants: UPPERCASE_WITH_UNDERSCORES
// Flag bits: FLAG_NAME pattern

// Function naming:
static __always_inline s32 select_idle_cpu(struct task_struct *p);
static __always_inline struct task_ctx *lookup_task_ctx(struct task_struct *p);

// Struct naming (no typedefs - use struct keyword):
struct task_ctx { ... };   // CORRECT
typedef struct { } task_ctx_t;  // WRONG - don't use typedefs

// Constants and flags:
#define FLAG_INPUT          (1 << 1)
#define BOOST_INPUT         7
#define SLICE_NS_DEFAULT    10000ULL
```

#### Rust Files
```rust
// Files: lowercase_with_underscores.rs
// Functions: snake_case
// Types/Structs: PascalCase
// Constants: SCREAMING_SNAKE_CASE
// Enum variants: PascalCase

fn load_bpf_program() -> Result<Scheduler>;
struct SchedulerStats { ... }
const MAX_CPUS: usize = 512;
enum TaskClass { Input, Gpu, Audio, Background }
```

#### Consistency Rules
- Use the same name in BPF and Rust when referring to the same concept
- BPF: `boost_shift` → Rust: `boost_shift` (not `boostShift` or `BoostShift`)
- BPF: `FLAG_INPUT` → Rust: `FLAG_INPUT` (constants match exactly)

### 3.4 Include Order

**Rule:** Dependencies must be included before dependents. Within a category, use alphabetical order.

```c
// In main.bpf.c:

// === LAYER 1: External dependencies ===
#include <scx/common.bpf.h>      // sched_ext BPF helpers

// === LAYER 2: Interface definitions ===
#include "intf.h"                // Shared userspace ↔ BPF structs

// === LAYER 3: Foundation (order matters!) ===
#include "include/config.bpf.h"  // Must be first - defines constants
#include "include/types.bpf.h"   // Depends on config - defines structs
#include "include/helpers.bpf.h" // Depends on types - utility functions

// === LAYER 4: Detection hooks (alphabetical, no interdependencies) ===
#include "include/detection/audio.bpf.h"
#include "include/detection/game.bpf.h"
#include "include/detection/gpu.bpf.h"
#include "include/detection/input.bpf.h"
#include "include/detection/net.bpf.h"
#include "include/detection/sync.bpf.h"

// === LAYER 5: Priority (depends on detection flags) ===
#include "include/priority/boost.bpf.h"

// === LAYER 6: Core scheduling (order matters!) ===
#include "include/core/cpu_select.bpf.h"  // CPU selection helpers
#include "include/core/enqueue.bpf.h"     // Uses cpu_select
#include "include/core/dispatch.bpf.h"    // Uses enqueue
```

**Key principle:** If file A uses something from file B, then B must be included before A.

### 3.5 Comment Standards

```c
// FILE HEADER (required for all .bpf.h files):
/* SPDX-License-Identifier: GPL-2.0
 *
 * input.bpf.h - Input device detection hooks
 *
 * Detects input handler threads via fentry hooks on:
 * - input_event(): Keyboard/mouse events
 * - hid_irq_in(): USB HID interrupt (earliest detection)
 * - hid_input_report(): HID report processing
 *
 * Sets FLAG_INPUT on task_ctx and triggers A.B.C. preemption.
 */

// FUNCTION COMMENTS (only for non-obvious logic):
/*
 * Why we check both FLAG_INPUT and FLAG_GPU:
 * Input handlers may also submit GPU work (e.g., cursor rendering).
 * We want the highest applicable boost.
 */
static __always_inline u8 calculate_boost(struct task_ctx *tctx) { ... }

// INLINE COMMENTS (sparingly, for "why" not "what"):
if (tctx->flags & FLAG_GPU) {
    // Prefer physical cores for GPU - reduces cache thrashing on SMT siblings
    return pick_physical_core(p);
}
```

**Inlining rules for BPF:**
- `static __always_inline` - Default for helpers. Forces inlining, required for BPF verifier.
- `static __noinline` - Use when function is called from multiple places AND inlining would bloat code.
- Never use plain `static inline` - BPF verifier may not inline it.

### 3.6 Forbidden Patterns

```c
// FORBIDDEN: Magic numbers
slice = 10000;  // What unit? Why 10000?

// CORRECT: Named constants
slice = SLICE_NS_DEFAULT;  // Defined in config.bpf.h with comment

// FORBIDDEN: Commented-out code
// if (old_feature) { ... }  // Delete it, git has history

// FORBIDDEN: Dead code paths
if (0) { debug_print(); }  // Use #ifdef DEBUG or remove

// FORBIDDEN: Duplicate logic
// If you're copying a code block, make it a function

// FORBIDDEN: Behavioral heuristics
if (runtime < 100000 && wakeups > threshold)  // NO - use hooks instead
```

### 3.7 Error Handling Patterns

#### BPF Error Handling
```c
// Pattern: Early return on lookup failure (no exceptions in BPF)
struct task_ctx *tctx = bpf_task_storage_get(&task_ctxs, p, 0, 0);
if (!tctx)
    return 0;  // Silent fail - task not tracked yet

// Pattern: Fallback for map operations
struct cpu_ctx *cctx = bpf_map_lookup_elem(&cpu_ctxs, &cpu);
if (!cctx) {
    // Fallback: use default behavior, don't crash
    return prev_cpu;
}

// Pattern: Bounds checking for arrays
if (cpu >= MAX_CPUS)
    return -EINVAL;
```

#### Rust Error Handling
```rust
// Pattern: Use Result<T, E> with context
fn load_bpf() -> Result<Skel> {
    let skel = SkelBuilder::default()
        .open()
        .context("Failed to open BPF skeleton")?;
    
    skel.load().context("Failed to load BPF program")?;
    Ok(skel)
}

// Pattern: Log and continue for non-fatal errors
if let Err(e) = update_focus(tgid) {
    warn!("Focus update failed: {}, continuing", e);
}

// FORBIDDEN: unwrap() in production code
let value = map.get(&key).unwrap();  // NO - will panic

// CORRECT: Handle the None/Err case explicitly
let value = map.get(&key).copied().unwrap_or(0);           // For Copy types
let value = map.get(&key).cloned().unwrap_or_default();    // For Clone + Default
let value = match map.get(&key) {                           // For complex handling
    Some(v) => *v,
    None => {
        warn!("Key not found, using default");
        DEFAULT_VALUE
    }
};

// ALLOWED: expect() with descriptive message (for truly impossible cases)
let cpu = skel.maps.rodata.nr_cpus;  // Safe - set at load time
```

### 3.8 Statistics Patterns

#### When to Use Atomic vs Per-CPU
```c
// USE ATOMIC: Low-frequency counters (< 1000/sec)
// Simple, but cache line bouncing at high frequency
__sync_fetch_and_add(&stats->nr_errors, 1);

// USE PER-CPU: High-frequency counters (hot path)
// No cache line bouncing, aggregate on read
struct {
    __uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, struct per_cpu_stats);
} percpu_stats SEC(".maps");

// In hot path:
struct per_cpu_stats *pcs = bpf_map_lookup_elem(&percpu_stats, &zero);
if (pcs)
    pcs->nr_dispatches++;  // No atomic needed - per-CPU
```

#### Adding a New Statistic

1. **Add field to `struct gamer_stats`** (in `types.bpf.h`, see Section 4.4):
```c
u64 nr_new_counter;  // Description of what this counts
```

2. **Increment in BPF code** (in relevant `.bpf.h`):
```c
STAT_INC(nr_new_counter);  // Uses helper macro from helpers.bpf.h
```

3. **Display in userspace** (in `stats.rs`):
```rust
println!("New Counter: {}", stats.nr_new_counter);
```

**Naming convention for stats:**
- `nr_*` - Count of events (e.g., `nr_input_detected`)
- `*_ns` - Time in nanoseconds (e.g., `max_wait_ns`)
- `*_histogram` - Distribution array (e.g., `boost_histogram[8]`)

### 3.9 DRY Patterns and Helper Extraction

#### When to Extract a Helper
```c
// EXTRACT when you see the same pattern 2+ times:

// BAD: Duplicate lookup pattern
struct task_ctx *tctx = bpf_task_storage_get(&task_ctxs, p, 0, 0);
if (!tctx) return 0;
// ... (repeated in 10 places)

// GOOD: Helper function
static __always_inline struct task_ctx *lookup_task_ctx(struct task_struct *p)
{
    return bpf_task_storage_get(&task_ctxs, p, 0, 0);
}

// Usage:
struct task_ctx *tctx = lookup_task_ctx(p);
if (!tctx) return 0;
```

#### Macro vs Function
```c
// USE MACRO: When you need compile-time string/token manipulation
#define STAT_INC(field) \
    do { \
        struct gamer_stats *s = get_stats(); \
        if (s) __sync_fetch_and_add(&s->field, 1); \
    } while (0)

// USE FUNCTION: For everything else (type safety, debugging)
static __always_inline u64 task_deadline(u64 vtime, u64 runtime, u8 boost) {
    return vtime + (runtime >> boost);
}
```

#### Helper Location Rules
| Helper Type | Location | Example |
|-------------|----------|---------|
| Task context ops | `helpers.bpf.h` | `lookup_task_ctx()` |
| CPU context ops | `helpers.bpf.h` | `lookup_cpu_ctx()` |
| Statistics | `helpers.bpf.h` | `STAT_INC()` macro |
| Detection-specific | `detection/*.bpf.h` | `is_gpu_submit_ioctl()` |
| CPU selection | `core/cpu_select.bpf.h` | `pick_idle_physical_core()` |
| Priority calc | `priority/boost.bpf.h` | `calculate_boost_shift()` |

### 3.10 Adding New Features Checklist

When adding a new feature (e.g., new detection hook):

- [ ] **Design first**: Document in design.md (what, why, how)
- [ ] **Flags**: Add any new flags to `types.bpf.h` FLAG definitions
- [ ] **Config**: Add any tunables to `config.bpf.h` with comments
- [ ] **Stats**: Add counters to `struct gamer_stats` in `types.bpf.h`
- [ ] **Implementation**: Create/modify appropriate .bpf.h file
- [ ] **Include**: Add include to `main.bpf.c` (correct layer, see 3.4)
- [ ] **Helpers**: Extract reusable code to `helpers.bpf.h`
- [ ] **Userspace**: Update stats display in `stats.rs` if needed
- [ ] **CLI**: Add any new arguments to `args.rs`
- [ ] **Test**: Verify with `./start.sh` and check `--stats` output
- [ ] **README**: Update if user-visible behavior changed
- [ ] **design.md**: Update architecture sections if structure changed

### 3.11 Code Review Checklist

Before committing any change, verify:

**Code Quality:**
- [ ] No magic numbers (all constants in `config.bpf.h`)
- [ ] No commented-out code (delete it, git has history)
- [ ] No duplicate logic (extract to helpers if pattern repeats)
- [ ] No behavioral heuristics (use hooks for 100% proof)
- [ ] File under size limit (see Section 3.2)

**Style:**
- [ ] Include order correct (see Section 3.4)
- [ ] Naming conventions followed (see Section 3.3)
- [ ] Comments explain "why", not "what"
- [ ] `__always_inline` used for BPF helpers (not plain `inline`)

**Safety:**
- [ ] Error cases handled (no bare `unwrap()` in Rust)
- [ ] Bounds checking for array access in BPF
- [ ] Null checks for map lookups

**Testing:**
- [ ] Compiles without warnings: `./build.sh`
- [ ] Loads successfully: `./start.sh`
- [ ] Stats show expected values: `./start.sh --stats 1`
- [ ] No kernel warnings in `dmesg`

### 3.12 Build and Run Scripts

#### build.sh

```bash
#!/bin/bash
# Build scx_gamer scheduler
# Usage: ./build.sh [--release|--debug]

set -e

MODE="${1:---release}"

if [ "$MODE" = "--debug" ]; then
    cargo build
    echo "Debug build complete: ./target/debug/scx_gamer"
else
    cargo build --release
    echo "Release build complete: ./target/release/scx_gamer"
fi
```

#### start.sh

Simple flat menu to start the scheduler:

```bash
#!/usr/bin/env bash
# scx_gamer launcher - Simple menu to start the scheduler

════════════════════════════════════════════════════════════════════
                         SCX_GAMER LAUNCHER
════════════════════════════════════════════════════════════════════

  1) Baseline
     General desktop and light gaming (slice: 1ms)

  2) Esports
     Competitive gaming, low latency (slice: 10µs)

  3) Baseline Debug
     Baseline + stats display for troubleshooting

  4) Esports Debug
     Esports + stats display for troubleshooting

  q) Quit

────────────────────────────────────────────────────────────────────

Select [1-4, q]:
```

**Profile Settings:**

| Profile | Slice | SMT Avoidance | Stats | Use Case |
|---------|-------|---------------|-------|----------|
| Baseline | 1000µs | Default | ❌ Off | Desktop, light gaming |
| Esports | 10µs | ✅ On | ❌ Off | Competitive gaming |
| Baseline Debug | 1000µs | Default | ✅ 2s | Troubleshooting |
| Esports Debug | 10µs | ✅ On | ✅ 2s | Troubleshooting |

**Key Design:**

1. **Flat menu** - No nested submenus, pick and go
2. **Gaming = No Stats** - Stats disabled for zero overhead
3. **Debug = Full Stats** - Stats every 2s + BPF debug events
4. **D-Bus passthrough** - Focus detection works under sudo

### 3.13 Debugging Guide

scx_gamer includes a comprehensive debugging system for troubleshooting gaming performance issues.
The system provides multiple levels of visibility into scheduler decisions.

#### Debug System Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                        DEBUGGING LAYERS                              │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  Level 1: Quick Stats (--stats N)                                   │
│  ├── Scheduling counters (enqueue, dispatch, shared)                │
│  ├── Detection hook counts (input, GPU, audio, sync)                │
│  ├── Priority distribution histogram                                 │
│  └── Health metrics (max wait, starvation)                          │
│                                                                      │
│  Level 2: Full Stats Display (--stats 2)                            │
│  ├── Visual bar charts for boost distribution                       │
│  ├── Wait time histogram (8 buckets: <1µs to >1s)                   │
│  ├── CPU selection breakdown (physical vs SMT)                       │
│  ├── Preemption analysis (kicks vs avoided)                         │
│  └── Window boost tracking (input/sync windows)                      │
│                                                                      │
│  Level 3: Event Tracing (--debug)                                    │
│  ├── Ring buffer with real-time events                              │
│  ├── Per-task: PID, TGID, boost, flags, CPU, wait time              │
│  ├── Event types: ENQUEUE, DISPATCH, DETECT_*, STARVATION           │
│  └── BPF trace_pipe output                                           │
│                                                                      │
│  Level 4: System Analysis (external tools)                           │
│  ├── bpftool for map/program inspection                             │
│  ├── perf for context switch analysis                               │
│  └── dmesg for kernel errors                                         │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

#### Level 1: Quick Health Check

```bash
# Is the scheduler running?
cat /sys/kernel/sched_ext/root/ops 2>/dev/null || echo "No sched_ext scheduler active"

# Check for errors
dmesg | tail -20 | grep -i "scx\|bpf\|sched"

# Quick stats view (updates every 5 seconds)
./start.sh
# Select: 1) Standard Profiles → 1) Esports
# Then check output for stats
```

#### Level 2: Full Stats Display

Run with `--stats 2` to get comprehensive stats every 2 seconds:

```bash
# Via start.sh
./start.sh
# Select: 3) Custom Flags
# Enter: --stats 2

# Or directly
sudo ./target/release/scx_gamer --stats 2
```

**Stats Display Sections:**

```
╔═══════════════════════════════════════════════════════════════╗
║               scx_gamer v2.0 - SCHEDULER STATS                ║
╚═══════════════════════════════════════════════════════════════╝

📊 SCHEDULING (Dispatch Path Breakdown)
  Direct dispatch (fast):    896282 ( 89.9%)   ← Fast path from select_cpu
  Enqueue fallback:          100565 ( 10.1%)   ← No idle CPU, fallback path
  Shared DSQ (deadline):     100565 (100.0% of enqueue)  ← All fallback uses deadline
  Dispatched (running):     1005706

🎮 DETECTION HOOKS (v2.2: Per-hook breakdown)
  [INPUT]  hid_irq:     5464  input_event:    16392  hid_report:        0  │ Total:    21856
  [GPU]    drm_ioctl:       0  atomic_commit:     0  dma_fence:   11964  │ Total:    11964
  [AUDIO]  alsa_ioctl:      0  pcm_period:    21050                      │ Total:    21050
  [SYNC]   esync:       4101  fsync:       1695  ntsync:   491876  │ Total:   497672
  Window boosts → Input:   7372   Sync:  22492

⚡ PRIORITY DISTRIBUTION
  BG(0):     472730  47.0% ████████████████      ← Background tasks
  GM(3):      84729   8.4% ███                   ← Game main thread
  GP(6):      72282   7.2% ███                   ← GPU tasks
  IN(7):     375965  37.4% █████████████         ← Input/high priority

⏱️  WAIT TIME (task queue latency) - 11 buckets
      <1µs:     876672  87.2% ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓  ← Excellent
    1-10µs:      48189   4.8% ▓▓
  10-100µs:      66664   6.6% ▓▓▓
   0.1-1ms:      10594   1.1% ▓
    1-10ms:       3551   0.4% ▓
  10-100ms:         36   0.0% ▓                       ← Acceptable
    0.1-1s:          0   0.0%                         ← Should be 0
      1-3s:          0   0.0%                         ← MUST be 0
      3-5s:          0   0.0%                         ← MUST be 0
     5-10s:          0   0.0%                         ← MUST be 0
      >10s:          0   0.0%                         ← MUST be 0

🖥️  CPU SELECTION (v2.2: prev_cpu preference)
  Physical:   919651 (100.0%)   SMT:        0 (  0.0%)
  Migrations:  94403   Same CPU:   801746   ← High same-CPU = good cache locality

🔄 PREEMPTION DECISIONS
  Preempt:    79986 ( 85.2%)  Avoided:    13937 ( 14.8%)  Idle:        0

❤️  HEALTH
  Max wait: 24.77ms   Starvation rescues:      0   Errors: 0
```

#### Key Metrics to Watch

| Section | Metric | Healthy | Problem Indicator | Meaning |
|---------|--------|---------|-------------------|---------|
| **Scheduling** | `Direct dispatch` | 85-95% | <70% | Fast path from select_cpu |
| **Scheduling** | `Enqueue fallback` | 5-15% | >30% | No idle CPUs, fallback path |
| **Scheduling** | `Same CPU` | >80% | <50% | Cache locality - high is good |
| **Detection** | `[INPUT] Total` | >0 during gameplay | 0 | Input hooks not firing |
| **Detection** | `[GPU] dma_fence` | >0 for NVIDIA | 0 | NVIDIA GPU not detected |
| **Detection** | `[SYNC] ntsync` | >0 for Proton | 0 | ntsync not working (best sync) |
| **Detection** | `Window boosts` | >0 | 0 | Game tasks not getting priority |
| **Priority** | `IN(7)` | 30-40% during gaming | <10% | Input handlers not classified |
| **Priority** | `GP(6)` | 5-10% during gaming | 0% | GPU threads not classified |
| **Priority** | `BG(0)` | 40-50% | >80% | Too many background = poor detection |
| **Wait Time** | `<1µs` bucket | >80% | <50% | Fast dispatch rate |
| **Wait Time** | `<100µs` total | >95% | <80% | Overall latency health |
| **Wait Time** | `>10ms` buckets | <0.1% | >1% | Scheduling delays |
| **Wait Time** | `>1s` buckets | **0%** | >0% | 🔴 CRITICAL STARVATION |
| **CPU** | `Physical` | 100% | <80% | SMT avoidance for latency tasks |
| **Preemption** | `Preempt` | 80-90% | <50% | Priority preemption working |
| **Health** | `Max wait` | <30ms | >100ms | Worst-case latency |
| **Health** | `Starvation rescues` | 0 | >100 | Starvation detection triggered |

#### Level 3: Event Tracing (--debug)

For detailed per-task event tracing:

```bash
# Start with debug mode
./start.sh
# Select: 2) Debug Mode

# Or directly
sudo RUST_LOG=debug ./target/release/scx_gamer --stats 1 --debug
```

**Debug Events Captured (Ring Buffer):**

| Event Type | When Emitted | Data Captured |
|------------|--------------|---------------|
| `EVENT_ENQUEUE` | Task becomes runnable | PID, TGID, boost, flags, wait_ns, CPU |
| `EVENT_DISPATCH` | Task dispatched to CPU | PID, CPU from/to, runtime |
| `EVENT_DETECT_INPUT` | Input hook fires | PID, timestamp |
| `EVENT_DETECT_GPU` | GPU submit detected | PID, timestamp |
| `EVENT_DETECT_AUDIO` | Audio callback detected | PID, timestamp |
| `EVENT_DETECT_SYNC` | Wine/Proton sync | PID, TGID, timestamp |
| `EVENT_BOOST_CHANGE` | Priority changed | PID, old_boost, new_boost |
| `EVENT_STARVATION` | Task rescued | PID, wait_ns |
| `EVENT_PREEMPT` | Preemption occurred | CPU, kicked task |
| `EVENT_MIGRATE` | Task moved CPUs | PID, from_cpu, to_cpu |

**BPF Trace Pipe (additional debug):**

```bash
# In a separate terminal, filter specific events:
sudo cat /sys/kernel/debug/tracing/trace_pipe | grep "scx_gamer"

# Filter by category:
sudo cat /sys/kernel/debug/tracing/trace_pipe | grep "input"
sudo cat /sys/kernel/debug/tracing/trace_pipe | grep "gpu"
sudo cat /sys/kernel/debug/tracing/trace_pipe | grep "starvation"
```

#### Level 4: System Analysis

```bash
# Check if fentry hooks are attached
sudo bpftool prog list | grep scx_gamer

# Verify maps are created and sizes
sudo bpftool map list | grep -A5 task_ctxs
sudo bpftool map list | grep -A5 stats_map

# Dump stats map directly
sudo bpftool map dump name stats_map

# Check CPU topology detection
cat /sys/devices/system/cpu/cpu*/topology/core_id

# Monitor context switches for game
perf stat -e context-switches,cpu-migrations -p $(pgrep -f "game_name") sleep 10

# Trace scheduling events
sudo perf sched record -p $(pgrep -f "game_name") sleep 5
sudo perf sched latency
```

#### Troubleshooting Common Gaming Issues

##### Issue: Game feels laggy despite scheduler running

**Diagnosis:**
```bash
sudo ./target/release/scx_gamer --stats 2
```

**Check:**
1. **Detection hooks firing?** Look at `Input:` and `GPU:` counters
2. **Boost working?** Check `IN(7)` and `GP(6)` in priority distribution
3. **Wait times reasonable?** Most should be in `<1µs` to `10-100µs` buckets

**Solutions:**
| Finding | Cause | Fix |
|---------|-------|-----|
| Input: 0 | Hooks not attached | Check kernel version, rebuild |
| IN(7): 0 | Input not classified | Game input going through different path |
| Wait >10ms | Tasks starving | Check for CPU-bound background work |
| Max wait >50ms | Severe starvation | Reduce background load, check affinity |

##### Issue: Game stutters/microfreezes

**Diagnosis:**
```bash
sudo ./target/release/scx_gamer --stats 1 --debug
# Watch the wait time histogram
```

**Likely causes:**
1. **Preemption storms**: Check `Preempt:` percentage - if >50%, too aggressive
2. **CPU contention**: Check `Physical:` percentage - should be >50%
3. **Starvation**: Watch for spikes in `10-100ms` or `>1s` buckets

**Solutions:**
| Symptom | Cause | Fix |
|---------|-------|-----|
| High Preempt % | Too aggressive | Scheduler is working correctly |
| Low Physical % | SMT contention | Enable `--avoid-smt` |
| Starvation rescues | System overload | Close background apps |

##### Issue: Scheduler crashes/exits immediately

**Diagnosis:**
```bash
# Check dmesg for BPF errors
dmesg | tail -50 | grep -i "bpf\|scx\|verifier"

# Run with debug
./start.sh --debug
```

**Common causes:**
| Error | Cause | Fix |
|-------|-------|-----|
| "Invalid DSQ ID" | DSQ not created | Check gamer_init() |
| "Verifier error" | BPF code issue | Check array bounds |
| "Permission denied" | CAP_BPF missing | Run with sudo |
| "Another scheduler running" | scx conflict | Stop other scheduler |

##### Issue: Input feels delayed

**Diagnosis:**
```bash
sudo ./target/release/scx_gamer --stats 2
# Focus on:
# - Input detection count
# - IN(7) boost histogram
# - Wait time histogram
```

**Check input chain:**
```bash
# Is input being detected?
# Look for "Input: X" incrementing during mouse/keyboard use

# Are input tasks getting boosted?
# Look for "IN(7):" with non-zero count

# Is wait time low for boosted tasks?
# Most waits should be <10µs
```

##### Issue: GPU/game frametime inconsistent

**Diagnosis:**
```bash
# Watch GPU detection
sudo ./target/release/scx_gamer --stats 2
# Check "GPU:" counter and "GP(6):" boost histogram

# Check for CPU migrations
# High migrations can hurt cache locality
```

**Solutions:**
| Finding | Cause | Fix |
|---------|-------|-----|
| GPU: 0 | DRM hooks not firing | May need different GPU path detection |
| GP(6): 0 | GPU not classified | Check drm_ioctl detection |
| High migrations | Poor affinity | Tasks bouncing between CPUs |

#### Adding Custom Debug Points

For development, add debug output to BPF code:

```c
// In any .bpf.h file:

// Method 1: Conditional debug print (requires --debug)
if (debug) {
    bpf_printk("select_cpu: task=%s boost=%d cpu=%d", 
               p->comm, tctx->boost_shift, selected_cpu);
}

// Method 2: Emit to ring buffer (captured in userspace)
emit_debug_event(EVENT_ENQUEUE, p, tctx, prev_cpu, target_cpu);

// Method 3: Stats counter (always available, no overhead with --no-stats)
STAT_INC(nr_custom_event);
```

#### Performance Mode vs Debug Mode

| Feature | `--no-stats` (Perf) | `--stats N` (Normal) | `--debug` (Full) |
|---------|---------------------|----------------------|------------------|
| Stats collection | ❌ Disabled | ✅ Enabled | ✅ Enabled |
| Wait histogram | ❌ Disabled | ✅ Enabled | ✅ Enabled |
| Ring buffer events | ❌ Disabled | ❌ Disabled | ✅ Enabled |
| BPF trace prints | ❌ Disabled | ❌ Disabled | ✅ Enabled |
| Overhead | Minimal | ~1-2% | ~5-10% |
| Use case | Competitive gaming | Normal gaming | Troubleshooting |

### 3.14 A.B.C. Pattern (Always Be Casting)

The core scheduling principle: **When we detect an event, prepare for the work that WILL follow.**

```c
// A.B.C. Pattern: Proactive CPU preparation

// WRONG: Reactive - wait for task to wake, then find CPU
SEC("fentry/hid_irq_in")
int detect_input(void *urb) {
    // Just record that input happened
    last_input_ns = bpf_ktime_get_ns();
    return 0;
}
// Problem: When input thread wakes, it may wait for CPU

// CORRECT: Proactive - prepare CPU immediately
SEC("fentry/hid_irq_in")
int detect_input(void *urb) {
    u64 now = bpf_ktime_get_ns();
    s32 cpu = bpf_get_smp_processor_id();
    
    // 1. Record timing
    last_input_ns = now;
    
    // 2. Mark CPU as expecting input work
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (cctx)
        cctx->flags |= CPU_INPUT_RESERVED;
    
    // 3. KICK: Preempt low-priority task to free CPU NOW
    kick_if_lower_priority(cpu, BOOST_INPUT);
    
    return 0;
}
// Result: CPU is FREE when input thread wakes - zero wait time
```

**A.B.C. applies to:**

| Event | Preparation Action |
|-------|-------------------|
| HID interrupt (`hid_irq_in`) | Kick CPU, reserve for input |
| GPU submit (`drm_ioctl`) | Prefer physical core for GPU thread |
| Audio period (`snd_pcm_period_elapsed`) | Boost audio thread |
| Wine sync signal | Prepare for game thread wake |

**When NOT to use A.B.C.:**
- Background tasks (no urgency)
- Unknown/unclassified tasks
- When system is idle (nothing to kick)

---

# Part II: Specification

## 4. Data Structures

### 4.1 task_ctx (Per-Task Context)

**Size:** 64 bytes (single cache line)  
**Alignment:** 64-byte aligned (CACHE_ALIGNED)

```c
struct task_ctx {
    /* === CACHE LINE 1 (64 bytes) === */
    
    /* Byte 0: Classification flags (hot - read every select_cpu) */
    u8 flags;
    /*
     * Bit layout:
     *   0: FLAG_GAME       - Thread belongs to foreground game
     *   1: FLAG_INPUT      - Input handler (mouse/keyboard/controller)
     *   2: FLAG_GPU        - GPU command submission
     *   3: FLAG_AUDIO      - Audio thread (ALSA/PipeWire)
     *   4: FLAG_COMPOSITOR - Window compositor (KWin)
     *   5: FLAG_NETWORK    - Network I/O (UDP gaming traffic)
     *   6: FLAG_SYNC       - Wine/Proton sync primitive
     *   7: FLAG_STALE      - Needs reclassification
     */
    
    /* Byte 1: Current priority boost (0-7) */
    u8 boost_shift;
    /*
     * Priority multiplier = 2^boost_shift
     *   7 = 128x (input handlers)
     *   6 = 64x  (GPU submit)
     *   5 = 32x  (audio)
     *   4 = 16x  (compositor)
     *   3 = 8x   (game main thread)
     *   2 = 4x   (game worker)
     *   1 = 2x   (foreground app)
     *   0 = 1x   (background)
     */
    
    /* Bytes 2-3: Reserved for future flags */
    u16 _reserved_flags;
    
    /* Bytes 4-7: Preferred CPU cache */
    s32 preferred_cpu;  /* -1 = no preference */
    
    /* Bytes 8-15: Virtual deadline */
    u64 deadline;
    
    /* Bytes 16-23: Last dispatch timestamp */
    u64 last_run_ns;
    
    /* Bytes 24-31: Cumulative runtime (for starvation detection) */
    u64 sum_runtime_ns;
    
    /* Bytes 32-35: Last CPU ran on */
    s32 last_cpu;
    
    /* Bytes 36-39: Process group ID (for game detection) */
    u32 tgid;
    
    /* Bytes 40-47: Timestamp of last classification */
    u64 classified_at_ns;
    
    /* Bytes 48-63: Reserved for future use */
    u64 _reserved[2];
    
} __attribute__((aligned(64)));

/* Flag definitions */
#define FLAG_GAME       (1 << 0)
#define FLAG_INPUT      (1 << 1)
#define FLAG_GPU        (1 << 2)
#define FLAG_AUDIO      (1 << 3)
#define FLAG_COMPOSITOR (1 << 4)
#define FLAG_NETWORK    (1 << 5)
#define FLAG_SYNC       (1 << 6)
#define FLAG_STALE      (1 << 7)

/* Flag test helpers */
#define IS_GAME(tctx)       ((tctx)->flags & FLAG_GAME)
#define IS_INPUT(tctx)      ((tctx)->flags & FLAG_INPUT)
#define IS_GPU(tctx)        ((tctx)->flags & FLAG_GPU)
#define IS_LATENCY_CRITICAL(tctx) \
    ((tctx)->flags & (FLAG_INPUT | FLAG_GPU | FLAG_AUDIO | FLAG_COMPOSITOR))
```

### 4.2 cpu_ctx (Per-CPU Context)

**Size:** 64 bytes (single cache line)  
**Alignment:** 64-byte aligned

```c
struct cpu_ctx {
    /* === CACHE LINE 1 (64 bytes) === */
    
    /* Bytes 0-7: Timestamp of last input event routed here */
    u64 last_input_ns;
    
    /* Bytes 8-11: Boost level of currently running task */
    u32 current_boost;
    
    /* Bytes 12-15: CPU flags */
    u32 flags;
    /*
     * Bit layout:
     *   0: CPU_IDLE          - Currently idle
     *   1: CPU_SMT           - Is an SMT sibling (hyperthread)
     *   2: CPU_PREFERRED     - In preferred/high-perf set
     *   3: CPU_INPUT_RESERVED - Reserved for imminent input processing
     */
    
    /* Bytes 16-19: Physical core ID (for SMT pairing) */
    s32 core_id;
    
    /* Bytes 20-23: SMT sibling CPU ID (-1 if no sibling) */
    s32 sibling_cpu;
    
    /* Bytes 24-27: NUMA node ID */
    u32 node_id;
    
    /* Bytes 28-31: Reserved */
    u32 _reserved0;
    
    /* Bytes 32-63: Reserved for future use */
    u64 _reserved[4];
    
} __attribute__((aligned(64)));

/* CPU flag definitions */
#define CPU_IDLE            (1 << 0)
#define CPU_SMT             (1 << 1)
#define CPU_PREFERRED       (1 << 2)
#define CPU_INPUT_RESERVED  (1 << 3)
```

### 4.3 BPF Maps

```c
/* Per-task context storage */
struct {
    __uint(type, BPF_MAP_TYPE_TASK_STORAGE);
    __uint(map_flags, BPF_F_NO_PREALLOC);
    __type(key, int);
    __type(value, struct task_ctx);
} task_ctxs SEC(".maps");

/* Per-CPU context (array for O(1) access) */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, MAX_CPUS);
    __type(key, u32);
    __type(value, struct cpu_ctx);
} cpu_ctxs SEC(".maps");

/* Global scheduler statistics */
struct {
    __uint(type, BPF_MAP_TYPE_ARRAY);
    __uint(max_entries, 1);
    __type(key, u32);
    __type(value, struct gamer_stats);
} stats_map SEC(".maps");
```

### 4.4 Statistics Structure

**Size:** 448 bytes (7 sections × 64 bytes each, cache-aligned)

The stats structure is organized into 7 sections, each 64 bytes for cache alignment:

```c
struct gamer_stats {
    /* === SECTION 1: Core Scheduling (64 bytes) === */
    u64 nr_enqueued;           /* Tasks going through enqueue() fallback */
    u64 nr_dispatched;         /* Tasks dispatched (started running) */
    u64 nr_direct_dispatch;    /* Direct dispatch from select_cpu (fast path) */
    u64 nr_shared_dispatch;    /* Shared DSQ dispatch (slow path) */
    u64 nr_wakeups;            /* Total task wakeups */
    u64 nr_yields;             /* Voluntary yields */
    u64 nr_slice_expiry;       /* Slice exhaustion (preempted by time) */
    u64 _sched_reserved;
    
    /* === SECTION 2: Detection Events - Per-Hook Stats (128 bytes) === */
    /* Input hooks (3) */
    u64 nr_hid_irq_in;         /* fentry/hid_irq_in - USB HID interrupt */
    u64 nr_input_event;        /* fentry/input_event - Input subsystem event */
    u64 nr_hid_input_report;   /* fentry/hid_input_report - HID report */
    
    /* GPU hooks (3) */
    u64 nr_drm_ioctl;          /* fentry/drm_ioctl - DRM command submit (AMD/Intel) */
    u64 nr_drm_atomic_commit;  /* fentry/drm_atomic_commit - Frame submit */
    u64 nr_dma_fence_signal;   /* fentry/dma_fence_signal - GPU fence (NVIDIA!) */
    
    /* Audio hooks (2) */
    u64 nr_audio_ioctl;        /* fentry/do_vfs_ioctl - ALSA ioctl */
    u64 nr_pcm_period;         /* fentry/snd_pcm_period_elapsed - Audio period */
    
    /* Sync hooks (3) */
    u64 nr_esync;              /* fentry/eventfd_signal_mask - Wine esync */
    u64 nr_fsync;              /* fentry/do_futex - Wine fsync */
    u64 nr_ntsync;             /* fentry/ntsync_char_ioctl - Wine ntsync (best!) */
    
    /* Aggregate totals (computed in userspace from per-hook stats) */
    u64 nr_input_detected;     /* Total input hooks fired */
    u64 nr_gpu_detected;       /* Total GPU hooks fired */
    u64 nr_audio_detected;     /* Total audio hooks fired */
    u64 nr_sync_detected;      /* Total sync hooks fired */
    
    /* Window boost stats */
    u64 nr_input_window_boosts;/* Tasks boosted by input window */
    u64 nr_sync_window_boosts; /* Tasks boosted by sync window */
    
    /* === SECTION 3: Priority Distribution (64 bytes) === */
    u64 boost_histogram[8];    /* Count per boost level (0-7) */
    
    /* === SECTION 4: CPU Selection (64 bytes) === */
    u64 nr_physical_selected;  /* Physical core selections */
    u64 nr_smt_selected;       /* SMT sibling selections */
    u64 nr_migrations;         /* CPU migrations */
    u64 nr_same_cpu;           /* Task stayed on same CPU */
    u64 nr_idle_found;         /* Found idle CPU */
    u64 nr_preempt_needed;     /* No idle CPU, will preempt */
    u64 _cpu_reserved[2];
    
    /* === SECTION 5: Preemption (64 bytes) === */
    u64 nr_preempt_kick;       /* Preemptive kicks (SCX_KICK_PREEMPT) */
    u64 nr_preempt_avoided;    /* Kicks avoided (equal/higher priority) */
    u64 nr_idle_kick;          /* Idle kicks (SCX_KICK_IDLE) */
    u64 nr_latency_critical_kicks; /* Kicks for latency-critical tasks */
    u64 _preempt_reserved[4];
    
    /* === SECTION 6: Wait Time Histogram (88 bytes) === */
    /* 11 buckets: <1us, 1-10us, 10-100us, 100us-1ms, 1-10ms, 10-100ms, 100ms-1s, 1-3s, 3-5s, 5-10s, >10s */
    u64 wait_histogram[11];
    
    /* === SECTION 7: Health & Debugging (64 bytes) === */
    u64 max_wait_ns;           /* Longest task wait time seen */
    u64 nr_starvation_rescues; /* Tasks rescued from starvation */
    u64 nr_errors;             /* Error count */
    u64 nr_affinity_failures;  /* CPU affinity violations prevented */
    u64 total_runtime_ns;      /* Sum of all task runtimes */
    u64 total_wait_ns;         /* Sum of all task wait times */
    u64 _health_reserved[2];
};
```

**Key Statistics Explained:**

| Section | Field | Description | Use for Debugging |
|---------|-------|-------------|-------------------|
| Core | `nr_shared_dispatch` | Tasks to shared DSQ | Should match nr_enqueued |
| Detection | `nr_input_window_boosts` | Tasks boosted during input window | Game responsiveness |
| Priority | `boost_histogram[7]` | Input handlers count | Should be >0 during gameplay |
| Priority | `boost_histogram[6]` | GPU threads count | Should be >0 during gameplay |
| CPU | `nr_physical_selected` | Physical core usage | Higher = better for latency |
| Preemption | `nr_preempt_avoided` | Smart kicks saved | Higher = less IPI overhead |
| Wait Time | `wait_histogram[0-2]` | Fast dispatches (<100µs) | Should be >90% |
| Wait Time | `wait_histogram[5-6]` | Slow dispatches (10ms-1s) | Should be <1% |
| Wait Time | `wait_histogram[7-10]` | Starvation (>1s) | MUST be 0% - indicates severe bug |
| Health | `max_wait_ns` | Worst-case latency | <5ms ideal, >20ms = problem |

### 4.5 Debug Event Structure

For real-time event tracing (only when `--debug` enabled):

```c
/* Event types */
enum debug_event_type {
    EVENT_ENQUEUE = 1,         /* Task enqueued */
    EVENT_DISPATCH = 2,        /* Task dispatched to CPU */
    EVENT_DETECT_INPUT = 3,    /* Input detected */
    EVENT_DETECT_GPU = 4,      /* GPU submit detected */
    EVENT_DETECT_AUDIO = 5,    /* Audio callback detected */
    EVENT_DETECT_SYNC = 6,     /* Wine/Proton sync detected */
    EVENT_BOOST_CHANGE = 7,    /* Task boost level changed */
    EVENT_STARVATION = 8,      /* Task starved, getting rescue */
    EVENT_PREEMPT = 9,         /* Preemption occurred */
    EVENT_MIGRATE = 10,        /* Task migrated CPUs */
};

/* Debug event structure (64 bytes, cache-aligned) */
struct debug_event {
    u64 timestamp_ns;          /* Event timestamp */
    u32 event_type;            /* enum debug_event_type */
    u32 pid;                   /* Task PID */
    u32 tgid;                  /* Task TGID (process) */
    u8 boost_old;              /* Previous boost level */
    u8 boost_new;              /* New boost level */
    u8 flags;                  /* Task flags */
    u8 _pad0;
    s32 cpu_from;              /* Source CPU (-1 if N/A) */
    s32 cpu_to;                /* Target CPU (-1 if N/A) */
    u64 wait_ns;               /* Time spent waiting */
    u64 runtime_ns;            /* Runtime so far */
    char comm[16];             /* Task name (first 16 chars) */
};
```

**Ring Buffer:** 256KB = ~4000 events, FIFO, userspace consumes via `libbpf_rs`

---

## 5. Detection Hooks

### 5.1 Hook Inventory

**Current Implemented Hooks (12 total):**

| Category | Hook | File | Stat Counter | Status |
|----------|------|------|--------------|--------|
| **INPUT** | `fentry/hid_irq_in` | input.bpf.h | `nr_hid_irq_in` | ✅ Active |
| **INPUT** | `fentry/input_event` | input.bpf.h | `nr_input_event` | ✅ Active |
| **INPUT** | `fentry/hid_input_report` | input.bpf.h | `nr_hid_input_report` | ✅ Active |
| **GPU** | `fentry/drm_ioctl` | gpu.bpf.h | `nr_drm_ioctl` | ✅ AMD/Intel |
| **GPU** | `fentry/drm_atomic_commit` | gpu.bpf.h | `nr_drm_atomic_commit` | ✅ All GPUs |
| **GPU** | `fentry/dma_fence_signal` | gpu.bpf.h | `nr_dma_fence_signal` | ✅ **NVIDIA!** |
| **AUDIO** | `fentry/do_vfs_ioctl` | audio.bpf.h | `nr_audio_ioctl` | ✅ ALSA |
| **AUDIO** | `fentry/snd_pcm_period_elapsed` | audio.bpf.h | `nr_pcm_period` | ✅ Active |
| **SYNC** | `fentry/eventfd_signal_mask` | sync.bpf.h | `nr_esync` | ✅ Wine esync |
| **SYNC** | `fentry/do_futex` | sync.bpf.h | `nr_fsync` | ✅ Wine fsync |
| **SYNC** | `fentry/ntsync_char_ioctl` | sync.bpf.h | `nr_ntsync` | ✅ Wine ntsync |

**Planned Hooks (not yet implemented):**

| Category | Hook | File | Purpose |
|----------|------|------|---------|
| **NET** | `fentry/udp_recvmsg` | net.bpf.h | Game UDP receive |
| **NET** | `fentry/udp_sendmsg` | net.bpf.h | Game UDP send |

**Debug Display Color Coding:**
- 🟢 Green: Hook is firing (working)
- 🟡 Yellow: Partial detection (some hooks firing)
- 🔴 Red: No hooks firing (not working for your setup)

### 5.2 Detection Flow Template

```c
/* Template for detection hooks (in detection/*.bpf.h) */

SEC("fentry/kernel_function")
int BPF_PROG(hook_name, /* kernel function args */)
{
    /* 1. Early exit if detection disabled */
    if (!detection_enabled)
        return 0;
    
    /* 2. Get task context */
    struct task_struct *p = bpf_get_current_task_btf();
    struct task_ctx *tctx = bpf_task_storage_get(&task_ctxs, p, 0, 0);
    if (!tctx)
        return 0;
    
    /* 3. Check if already classified (avoid redundant work) */
    if (tctx->flags & FLAG_ALREADY_SET)
        return 0;
    
    /* 4. Validate this is a relevant call (kernel-specific checks) */
    if (!is_relevant_call(/* args */))
        return 0;
    
    /* 5. Apply classification */
    tctx->flags |= FLAG_TO_SET;
    tctx->boost_shift = APPROPRIATE_BOOST;
    tctx->classified_at_ns = bpf_ktime_get_ns();
    
    /* 6. Update statistics */
    struct gamer_stats *stats = get_stats();
    if (stats)
        __sync_fetch_and_add(&stats->nr_xxx_detected, 1);
    
    /* 7. A.B.C.: Proactive CPU preparation (if applicable) */
    if (SHOULD_KICK)
        kick_for_latency_critical();
    
    return 0;
}
```

### 5.3 Input Detection (input.bpf.h)

```c
/* SPDX-License-Identifier: GPL-2.0
 *
 * input.bpf.h - Input device detection hooks
 *
 * Priority: HIGHEST (boost_shift = 7)
 * Hooks: hid_irq_in (earliest), input_event, hid_input_report
 */

/* hid_irq_in: Fires when USB HID device sends interrupt
 * This is the EARLIEST point we can detect input - before any processing.
 * Used for A.B.C.: kick a CPU to be ready for the input thread.
 */
SEC("fentry/hid_irq_in")
int BPF_PROG(detect_hid_irq, void *urb)
{
    u64 now = bpf_ktime_get_ns();
    s32 cpu = bpf_get_smp_processor_id();
    
    /* A.B.C.: Mark CPU as expecting input, kick lowest priority task */
    struct cpu_ctx *cctx = bpf_map_lookup_elem(&cpu_ctxs, &cpu);
    if (cctx) {
        cctx->last_input_ns = now;
        cctx->flags |= CPU_INPUT_RESERVED;
    }
    
    /* Speculatively preempt if a low-priority task is running */
    kick_if_lower_priority(cpu, BOOST_INPUT);
    
    return 0;
}

/* input_event: Fires when input subsystem processes an event
 * This identifies the actual input handler thread.
 */
SEC("fentry/input_event")
int BPF_PROG(detect_input_event, 
             struct input_dev *dev, unsigned int type, 
             unsigned int code, int value)
{
    /* Only care about actual input (not sync events) */
    if (type == EV_SYN)
        return 0;
    
    struct task_struct *p = bpf_get_current_task_btf();
    struct task_ctx *tctx = bpf_task_storage_get(&task_ctxs, p, 0, 0);
    if (!tctx)
        return 0;
    
    /* Mark as input handler with maximum boost */
    tctx->flags |= FLAG_INPUT;
    tctx->boost_shift = BOOST_INPUT;  /* 7 = 128x priority */
    
    STAT_INC(nr_input_detected);
    return 0;
}
```

### 5.4 GPU Detection (gpu.bpf.h)

```c
/* SPDX-License-Identifier: GPL-2.0
 *
 * gpu.bpf.h - GPU thread detection hooks
 *
 * Priority: HIGH (boost_shift = 6)
 * Hooks: drm_ioctl (command submit), drm_atomic_commit (frame timing)
 */

/* Identify GPU vendor from ioctl command */
static __always_inline bool is_gpu_submit_ioctl(unsigned int cmd)
{
    /* AMD: DRM_AMDGPU_CS = 0x44 */
    if ((cmd & 0xFF) == 0x44)
        return true;
    
    /* Intel: DRM_I915_GEM_EXECBUFFER2 = 0x29 */
    if ((cmd & 0xFF) == 0x29)
        return true;
    
    /* NVIDIA uses different path (nv_drm_ioctl) */
    return false;
}

SEC("fentry/drm_ioctl")
int BPF_PROG(detect_gpu_submit, struct file *filp, unsigned int cmd, unsigned long arg)
{
    if (!is_gpu_submit_ioctl(cmd))
        return 0;
    
    struct task_struct *p = bpf_get_current_task_btf();
    struct task_ctx *tctx = bpf_task_storage_get(&task_ctxs, p, 0, 0);
    if (!tctx)
        return 0;
    
    tctx->flags |= FLAG_GPU;
    tctx->boost_shift = MAX(tctx->boost_shift, BOOST_GPU);  /* Don't reduce existing boost */
    
    STAT_INC(nr_gpu_detected);
    return 0;
}
```

### 5.5 GPU Detection by Vendor

The challenge with GPU detection is that different vendors use different kernel paths:

| Vendor | Driver | Submit Path | Hook Strategy |
|--------|--------|-------------|---------------|
| **AMD** | amdgpu | `drm_ioctl` → `DRM_AMDGPU_CS` | ✅ `drm_ioctl` hook works |
| **Intel** | i915 | `drm_ioctl` → `DRM_I915_GEM_EXECBUFFER2` | ✅ `drm_ioctl` hook works |
| **NVIDIA** | nvidia-drm | `nvidia_drm` module ioctls | ⚠️ Different path! |
| **Universal** | All | `dma_fence_signal` | ✅ **Works for ALL GPUs** |

#### NVIDIA Detection Challenge

NVIDIA's proprietary driver doesn't use standard DRM ioctls for command submission. The GPU work goes through:
1. `nvidia-drm` kernel module
2. Proprietary ioctls (`nv_drm_*_ioctl`)
3. Internal NVIDIA paths

**Solution: `dma_fence_signal` hook**

ALL GPU drivers (including NVIDIA) use DMA fences for GPU completion signaling:

```c
/*
 * Universal GPU detection via DMA fence
 *
 * When GPU work completes, dma_fence_signal is called.
 * This works for AMD, Intel, AND NVIDIA.
 *
 * Benefits:
 * - Universal (all GPU vendors)
 * - Signals actual GPU completion (not just submission)
 * - Can be used for A.B.C. (GPU done → game thread about to wake)
 */
SEC("fentry/dma_fence_signal")
int BPF_PROG(detect_dma_fence_signal, struct dma_fence *fence)
{
    u64 now = bpf_ktime_get_ns();
    struct task_struct *p = (void *)bpf_get_current_task_btf();
    struct task_ctx *tctx = lookup_task_ctx(p);
    
    if (!tctx)
        return 0;
    
    /* Task is interacting with GPU fences */
    tctx->flags |= FLAG_GPU;
    if (tctx->boost_shift < BOOST_GPU)
        tctx->boost_shift = BOOST_GPU;
    tctx->classified_at_ns = now;
    
    STAT_INC(nr_gpu_detected);
    
    /* A.B.C.: GPU completion often precedes game thread wakeup */
    /* Could trigger speculative kick here */
    
    return 0;
}
```

### 5.6 Wine/Proton Sync Primitives

Proton games use Windows synchronization primitives emulated through various mechanisms:

| Sync Type | Kernel Path | Hook | Status |
|-----------|-------------|------|--------|
| **esync** | `eventfd_signal_mask` | `fentry/eventfd_signal_mask` | ✅ Implemented |
| **fsync** | `do_futex` | `fentry/do_futex` | ✅ Implemented |
| **ntsync** | `ntsync_char_ioctl` | `fentry/ntsync_char_ioctl` | ✅ Implemented |

#### v2.2 CRITICAL FIX: No foreground_tgid Check

**Bug:** Sync detection required `foreground_tgid` to be set manually.
This violated the 100% hook-based detection design philosophy.

**Symptom:** Window boosts = 0 despite 250k+ sync events firing.

**Fix:** Removed `foreground_tgid` check from `handle_sync_signal()`.
These sync primitives (esync/fsync/ntsync) are Wine/Proton-specific.
Any task using them is automatically game-related.

```c
/* BEFORE (broken) */
static __always_inline void handle_sync_signal(struct task_struct *p, u64 now)
{
    if (!foreground_tgid) return;  // BAD: Requires manual config!
    if (p->tgid != foreground_tgid) return;
    // ... never reached ...
}

/* AFTER (fixed) */
static __always_inline void handle_sync_signal(struct task_struct *p, u64 now)
{
    /* No check needed - ntsync/esync/fsync ARE Wine/Proton-specific */
    last_sync_signal_ns = now;  // Enable window boosts
    tctx->flags |= FLAG_SYNC;   // Mark as game-related
    // ...
}
```

#### esync (Event-based Synchronization)

The default for most Proton games. Uses Linux eventfd:

```c
SEC("fentry/eventfd_signal_mask")
int BPF_PROG(detect_esync_signal, void *eventfd_ctx, __u64 mask)
{
    STAT_INC(nr_esync);
    handle_sync_signal(p, now);
    return 0;
}
```

#### fsync (Futex-based Synchronization)

More efficient than esync, requires kernel support:

```c
SEC("fentry/do_futex")
int BPF_PROG(detect_fsync_futex, u32 __user *uaddr, int op, ...)
{
    /* Filter for FUTEX_WAKE operations */
    if ((op & 0x7F) == 1 || (op & 0x7F) == 5 || (op & 0x7F) == 10) {
        STAT_INC(nr_fsync);
        handle_sync_signal(p, now);
    }
    return 0;
}
```

#### ntsync (Native NT Sync - BEST!)

The newest and most efficient sync mechanism. Uses a dedicated kernel driver:

```c
SEC("fentry/ntsync_char_ioctl")
int BPF_PROG(detect_ntsync, struct file *file, unsigned int cmd, unsigned long arg)
{
    STAT_INC(nr_ntsync);
    handle_sync_signal(p, now);
    return 0;
}
```

**Availability check:**
```bash
# Check if ntsync is available
cat /proc/kallsyms | grep ntsync_char_ioctl
# Example output: ffffffffc0a2f0d0 t ntsync_char_ioctl [ntsync]
```

### 5.7 Detection Requirements by Setup

| Setup | Required Hooks | Notes |
|-------|----------------|-------|
| **Native Linux game** | `drm_ioctl`, `input_event` | Standard DRM path |
| **Proton + AMD GPU** | `drm_ioctl`, `eventfd_signal_mask` | esync/fsync for sync |
| **Proton + Intel GPU** | `drm_ioctl`, `eventfd_signal_mask` | Same as AMD |
| **Proton + NVIDIA GPU** | `dma_fence_signal`, `eventfd_signal_mask` | DRM ioctls don't work! |
| **Proton + ntsync** | `ntsync_char_ioctl`, `dma_fence_signal` | Best performance |

### 5.8 Checking Available Hooks

To verify what hooks are available on your system:

```bash
# Check GPU hooks
cat /proc/kallsyms | grep -E "drm_ioctl|dma_fence_signal|nvidia_drm"

# Check sync hooks
cat /proc/kallsyms | grep -E "eventfd_signal|do_futex|ntsync"

# Check if ntsync module is loaded
lsmod | grep ntsync
```

---

## 6. Priority System

### 6.1 Boost Levels

```c
/* Priority boost levels (in config.bpf.h) */
#define BOOST_INPUT       7   /* 128x - Input handlers (mouse, keyboard) */
#define BOOST_GPU         6   /* 64x  - GPU command submission */
#define BOOST_AUDIO       5   /* 32x  - Audio threads */
#define BOOST_COMPOSITOR  4   /* 16x  - Window compositor */
#define BOOST_GAME_MAIN   3   /* 8x   - Game main thread */
#define BOOST_GAME_WORKER 2   /* 4x   - Game worker threads */
#define BOOST_FOREGROUND  1   /* 2x   - Foreground non-game */
#define BOOST_BACKGROUND  0   /* 1x   - Background tasks */
```

### 6.2 Boost Calculation

```c
/* Calculate boost from flags (in priority/boost.bpf.h) */
static __always_inline u8 calculate_boost_shift(struct task_ctx *tctx, u32 fg_tgid)
{
    /* Priority order: input > gpu > audio > compositor > game > foreground > background */
    
    if (tctx->flags & FLAG_INPUT)
        return BOOST_INPUT;
    
    if (tctx->flags & FLAG_GPU)
        return BOOST_GPU;
    
    if (tctx->flags & FLAG_AUDIO)
        return BOOST_AUDIO;
    
    if (tctx->flags & FLAG_COMPOSITOR)
        return BOOST_COMPOSITOR;
    
    /* Game thread detection */
    if (tctx->flags & FLAG_GAME) {
        /* Main thread gets higher boost than workers */
        if (tctx->tgid == tctx->pid)  /* pid == tgid means main thread */
            return BOOST_GAME_MAIN;
        return BOOST_GAME_WORKER;
    }
    
    /* Foreground app (non-game) */
    if (fg_tgid && tctx->tgid == fg_tgid)
        return BOOST_FOREGROUND;
    
    return BOOST_BACKGROUND;
}
```

### 6.3 Deadline-Ordered Dispatch

#### Why Deadline Ordering Matters

The BPF scheduler has two primary dispatch functions:

| Function | Ordering | Use Case |
|----------|----------|----------|
| `scx_bpf_dsq_insert()` | **FIFO** | Simple insertion, no priority |
| `scx_bpf_dsq_insert_vtime()` | **Deadline-ordered** | Tasks run in deadline order |

**The Problem with FIFO:**
If we calculate a deadline but use `scx_bpf_dsq_insert()`, tasks are dispatched in arrival order, not deadline order. This means:
- A task waiting 6ms has no priority over one waiting 1ms
- Worst-case latency is unbounded
- Priority boosts have no effect on dispatch order

**scx_cosmos achieves better worst-case frametimes (5ms vs 6ms+) because it uses deadline-ordered dispatch.**

#### Deadline Calculation Formula

```c
/* Virtual deadline formula (EEVDF-inspired) */
static __always_inline u64 calculate_deadline(u64 vtime, u64 runtime, u8 boost_shift)
{
    /*
     * deadline = vtime + (runtime >> boost_shift)
     *
     * Higher boost_shift = smaller deadline increment = runs sooner
     *
     * Example at runtime = 1ms (1,000,000 ns):
     *   boost=0: deadline = vtime + 1,000,000 ns  (background)
     *   boost=7: deadline = vtime + 7,812 ns       (input: 128x faster)
     */
    return vtime + (runtime >> boost_shift);
}
```

#### Deadline-Ordered Dispatch Implementation

```c
/* CORRECT: Deadline-ordered dispatch */
void enqueue_with_deadline(struct task_struct *p, struct task_ctx *tctx, 
                           u64 slice, u64 enq_flags)
{
    u64 deadline = task_deadline(p, tctx);
    
    /* scx_bpf_dsq_insert_vtime orders tasks by the vtime parameter */
    scx_bpf_dsq_insert_vtime(p, SHARED_DSQ, slice, deadline, enq_flags);
}

/* WRONG: FIFO dispatch ignores deadline */
void enqueue_fifo(struct task_struct *p, u64 slice, u64 enq_flags)
{
    /* This is FIFO - deadline calculation is wasted */
    scx_bpf_dsq_insert(p, SHARED_DSQ, slice, enq_flags);
}
```

#### Slice Lag: Preventing Unbounded Credit

**The Problem:** Tasks that sleep for long periods accumulate "credit" (their vtime falls behind global vtime). Without a cap, a task sleeping for 10 seconds could wake up and dominate the CPU.

**The Solution:** Cap how far behind global vtime a task can be.

```c
/* Maximum time a task can be "behind" - prevents starvation */
const volatile u64 slice_lag = 20000000ULL;  /* 20ms */

static u64 task_deadline(struct task_struct *p, struct task_ctx *tctx)
{
    u64 vtime_min = vtime_now - slice_lag;
    
    /* Cap: task can't be more than slice_lag behind */
    if (time_before(p->scx.dsq_vtime, vtime_min))
        p->scx.dsq_vtime = vtime_min;
    
    /* Deadline = current vtime + execution cost */
    return p->scx.dsq_vtime + scale_by_weight(p, tctx->exec_runtime);
}
```

**Why 20ms?** This is ~1.2 frames at 60Hz. A task can accumulate enough credit for one "free" frame, but not enough to starve others.

#### v2.4 FIX: Absolute Time Deadlines (CRITICAL)

**The Problem:** v2.3's deadline increment cap wasn't enough! Vtime-based deadlines had a fatal flaw:

```c
// v2.3 approach - BROKEN!
vtime_min = vtime_now - 20ms;
if (p->scx.dsq_vtime < vtime_min)
    p->scx.dsq_vtime = vtime_min;  // Cap to same base!
    
deadline = p->scx.dsq_vtime + exec_cost;
```

**Why this failed:**
1. ALL waking tasks get dsq_vtime capped to `vtime_now - 20ms`
2. This means ALL tasks have the SAME base vtime
3. Priority is ONLY determined by exec_cost (boost level)
4. A task arriving LATER with higher priority beats older tasks!

```
BG task at T=0:  dsq_vtime = vtime(0) - 20ms, exec_cost = 5ms  → deadline = X - 15ms
IN task at T=1s: dsq_vtime = vtime(1s) - 20ms, exec_cost = 78µs → deadline = Y - 19.9ms

If Y > X + 5ms, the INPUT task wins despite arriving 1 SECOND later!
```

**Stats showing the problem:**
```
      1-3s:       3,113 tasks
      3-5s:         822 tasks
Max wait: 4177.77ms  ← 4.2 SECONDS STARVATION!
```

**v2.4 Solution: Absolute time-based deadlines**
```c
static __always_inline u64 task_deadline(...)
{
    u64 now = bpf_ktime_get_ns();  // Use REAL time!
    
    exec_cost = tctx->exec_runtime >> boost;
    if (exec_cost > MAX_DEADLINE_INCREMENT_NS)
        exec_cost = MAX_DEADLINE_INCREMENT_NS;
    
    return now + exec_cost;  // Deadline is absolute!
}
```

**Why this works:**
```
BG task at T=0: deadline = 0 + 5ms = 5ms
IN task at T=1s: deadline = 1s + 78µs = 1000.078ms

BG runs first (5ms < 1000ms) even though IN has higher priority!
```

**Priority still works within same arrival time:**
```
IN task at T=0: deadline = 0 + 78µs = 78µs
BG task at T=0: deadline = 0 + 5ms = 5ms

IN runs first (78µs < 5ms) ✓
```

**Guaranteed bounds:**
- No task waits more than ~5ms after a task that arrived LATER
- Priority ordering preserved for tasks arriving at the same time
- Earlier tasks ALWAYS beat later tasks of equal or lower priority

#### Execution Runtime Tracking

**Key Insight:** Track runtime *since last sleep*, not cumulative total.

```c
struct task_ctx {
    u64 exec_runtime;     /* Runtime since last sleep (reset on wake) */
    u64 sum_runtime_ns;   /* Total runtime (for statistics only) */
    u64 last_run_at;      /* Timestamp when task started running */
};

/* In ops.runnable() - task waking up */
void BPF_STRUCT_OPS(gamer_runnable, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    if (tctx)
        tctx->exec_runtime = 0;  /* Reset on wake */
}

/* In ops.stopping() - task yielding CPU */
void BPF_STRUCT_OPS(gamer_stopping, struct task_struct *p, bool runnable)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    u64 delta = bpf_ktime_get_ns() - tctx->last_run_at;
    
    /* Accumulate runtime since last sleep */
    tctx->exec_runtime = MIN(tctx->exec_runtime + delta, slice_lag);
    
    /* Update vtime for fairness */
    p->scx.dsq_vtime += scale_by_weight_inverse(p, delta);
}
```

**Why this matters for gaming:**
- Short-burst tasks (input handlers, audio) have low `exec_runtime` → earlier deadline
- Long-running tasks (background compile) have high `exec_runtime` → later deadline
- Game threads that sleep between frames get priority over CPU hogs

#### scx_cosmos Reference Implementation

scx_cosmos achieves excellent worst-case latency through this approach:

```c
/* From scx_cosmos/src/bpf/main.bpf.c */

/* Deadline calculation with all features */
static u64 task_dl(struct task_struct *p, struct task_ctx *tctx)
{
    /* Wakeup frequency scaling - frequent wakers get more credit */
    u64 lag_scale = MAX(tctx->wakeup_freq, 1);
    u64 vsleep_max = scale_by_task_weight(p, slice_lag * lag_scale);
    u64 vtime_min = vtime_now - vsleep_max;

    /* Cap how far behind task can be */
    if (time_before(p->scx.dsq_vtime, vtime_min))
        p->scx.dsq_vtime = vtime_min;

    /* Deadline = vtime + weighted exec_runtime */
    return p->scx.dsq_vtime + scale_by_task_weight_inverse(p, tctx->exec_runtime);
}

/* Dispatch with deadline ordering */
void BPF_STRUCT_OPS(cosmos_enqueue, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = try_lookup_task_ctx(p);
    
    /* When system is busy, use deadline-ordered shared DSQ */
    if (is_system_busy()) {
        scx_bpf_dsq_insert_vtime(p, SHARED_DSQ,
                                 task_slice(p), task_dl(p, tctx), enq_flags);
        return;
    }
    
    /* When idle, local DSQ is fine (no contention) */
    scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL, task_slice(p), enq_flags);
}
```

#### Comparison: FIFO vs Deadline-Ordered

| Metric | FIFO Dispatch | Deadline-Ordered |
|--------|--------------|------------------|
| Worst-case frametime | Unbounded (6ms+) | Bounded (~5ms) |
| Priority enforcement | None (ignored) | Strict ordering |
| Starvation | Possible | Prevented by slice_lag |
| Complexity | Simple | Moderate |
| Overhead | Minimal | Slight (vtime tracking) |

#### Integration with boost_shift

Our priority system integrates with deadline ordering:

```c
static u64 task_deadline_with_boost(struct task_struct *p, struct task_ctx *tctx)
{
    u64 vtime_min = vtime_now - slice_lag;
    
    /* Cap vtime */
    if (time_before(p->scx.dsq_vtime, vtime_min))
        p->scx.dsq_vtime = vtime_min;
    
    /* Apply boost to deadline calculation */
    u64 weighted_runtime = tctx->exec_runtime >> tctx->boost_shift;
    
    return p->scx.dsq_vtime + weighted_runtime;
}
```

This gives us:
- **EEVDF fairness** via vtime tracking
- **Priority control** via boost_shift
- **Bounded latency** via slice_lag cap
- **Burst-friendly** via exec_runtime reset on wake

### 6.4 Slice Duration

```c
/* Time slice calculation (in config.bpf.h) */
const volatile u64 slice_base_ns = 10000;  /* 10µs base slice */

/* Slice by task type */
#define SLICE_INPUT       (slice_base_ns >> 2)   /* 2.5µs - yield quickly after input */
#define SLICE_GPU         (slice_base_ns)        /* 10µs  - standard for GPU work */
#define SLICE_BACKGROUND  (slice_base_ns << 1)   /* 20µs  - longer for batch work */

static __always_inline u64 task_slice(struct task_ctx *tctx)
{
    if (tctx->flags & FLAG_INPUT)
        return SLICE_INPUT;
    
    if (tctx->flags & (FLAG_GPU | FLAG_AUDIO | FLAG_COMPOSITOR | FLAG_GAME))
        return SLICE_GPU;
    
    return SLICE_BACKGROUND;
}
```

### 6.5 Starvation Prevention & Rescue

Even with deadline-ordered dispatch, edge cases can cause long waits:
- CPU-pinned kernel tasks (kworker/N, ksoftirqd/N)
- Non-preemptible kernel code (spinlocks, interrupt handlers)
- Temporary system overload

**v2.5: Aggressive Starvation Rescue**

```c
/* In priority/boost.bpf.h */

/* Lowered from 100ms to 20ms for faster intervention */
#define STARVATION_THRESH_NS (20ULL * 1000 * 1000)  /* 20ms */

/*
 * Check if task has been waiting too long since becoming runnable.
 * Uses last_woke_at (not last_run_ns) to catch DSQ starvation.
 */
static __always_inline bool task_is_starved(struct task_ctx *tctx, u64 now)
{
    if (!tctx || tctx->last_woke_at == 0)
        return false;
    
    u64 wait_time = now - tctx->last_woke_at;
    
    if (wait_time > STARVATION_THRESH_NS) {
        STAT_INC(nr_starvation_rescues);
        return true;
    }
    return false;
}

/*
 * Rescue boost - elevate to maximum priority.
 * Must be INPUT level to beat all game tasks.
 */
static __always_inline u8 get_starvation_rescue_boost(struct task_ctx *tctx)
{
    return BOOST_INPUT;  /* Highest priority */
}
```

**Why 20ms instead of 100ms?**
- 100ms allowed 19ms outliers without intervention
- 20ms catches outliers before they become noticeable
- ~1.2 frames at 60Hz, ~4.8 frames at 240Hz
- Matches `MAX_DEADLINE_INCREMENT_NS` for consistency

**v2.5: Rescued Task Logging**

When a task is rescued, we emit a debug event with the task name:

```c
/* In enqueue() when starvation detected */
if (task_is_starved(tctx, now)) {
    boost = get_starvation_rescue_boost(tctx);
    tctx->boost_shift = boost;
    is_critical = true;
    
    /* Log the rescue event (only in debug mode) */
    emit_debug_event(EVENT_STARVATION, p, tctx, tctx->last_cpu, -1);
}
```

**Debug output:**
```
🚨 RESCUED TASKS (last 5)
  pipewire-pulse (pid: 1234) waited 21.45ms
        kworker/0:1 (pid:  456) waited 19.23ms
```

This helps identify:
- Which tasks are hitting outliers
- Whether they're kernel (kworker) or userspace
- If specific CPUs have pinned-task issues

---

## 7. CPU Selection

### 7.1 Selection Priority

```
CPU Selection Priority (highest to lowest):
1. Affinity-constrained CPU (honor cpus_allowed)
2. Previous CPU if idle (cache locality)
3. Physical core if latency-critical (avoid SMT thrashing)
4. Any idle CPU in preferred set
5. Any idle CPU
6. Least-loaded CPU (fallback)
```

### 7.2 Implementation

```c
/* In core/cpu_select.bpf.h */

static __always_inline s32 select_cpu_for_task(
    struct task_struct *p,
    struct task_ctx *tctx,
    s32 prev_cpu,
    u64 wake_flags)
{
    const struct cpumask *allowed = p->cpus_ptr;
    
    /* 1. Check affinity constraint */
    if (!bpf_cpumask_test_cpu(prev_cpu, allowed))
        prev_cpu = bpf_cpumask_first(allowed);
    
    /* 2. Latency-critical tasks prefer physical cores */
    if (IS_LATENCY_CRITICAL(tctx)) {
        s32 phys = pick_idle_physical_core(allowed);
        if (phys >= 0)
            return phys;
    }
    
    /* 3. Check if previous CPU is idle */
    if (scx_bpf_test_and_clear_cpu_idle(prev_cpu))
        return prev_cpu;
    
    /* 4. Find any idle CPU */
    s32 idle = scx_bpf_pick_idle_cpu(allowed, 0);
    if (idle >= 0)
        return idle;
    
    /* 5. Fallback to previous CPU */
    return prev_cpu;
}

/* Find idle physical core (non-SMT or both siblings idle) */
static __always_inline s32 pick_idle_physical_core(const struct cpumask *allowed)
{
    s32 cpu;
    
    bpf_for(cpu, 0, nr_cpu_ids) {
        if (!bpf_cpumask_test_cpu(cpu, allowed))
            continue;
        
        struct cpu_ctx *cctx = bpf_map_lookup_elem(&cpu_ctxs, &cpu);
        if (!cctx)
            continue;
        
        /* Skip SMT siblings */
        if (cctx->flags & CPU_SMT)
            continue;
        
        if (scx_bpf_test_and_clear_cpu_idle(cpu))
            return cpu;
    }
    
    return -1;
}
```

---

## 8. Dispatch Logic

### 8.1 Dispatch Methods in sched_ext

sched_ext provides several dispatch mechanisms, each with different trade-offs:

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    DISPATCH METHOD SPECTRUM                              │
│                                                                          │
│  FASTEST ───────────────────────────────────────────────────► FAIREST   │
│                                                                          │
│  Direct         Local DSQ      Local DSQ       Shared DSQ    Shared DSQ │
│  Dispatch       (current)      (specific)      (FIFO)        (vtime)    │
│                                                                          │
│  Skip enqueue   Low overhead   Medium          Cross-CPU     Deadline   │
│  callback       Per-CPU lock   overhead        contention    ordering   │
└─────────────────────────────────────────────────────────────────────────┘
```

| Method | Function | Overhead | Use Case |
|--------|----------|----------|----------|
| **Direct Dispatch** | `scx_bpf_dsq_insert()` in `select_cpu` | Lowest | Idle CPU available |
| **Local DSQ** | `SCX_DSQ_LOCAL` | Low | Keep on current CPU |
| **Local DSQ (specific)** | `SCX_DSQ_LOCAL_ON \| cpu` | Low-Medium | Target specific CPU |
| **Shared DSQ (FIFO)** | `scx_bpf_dsq_insert(SHARED_DSQ)` | Medium | Load balancing |
| **Shared DSQ (vtime)** | `scx_bpf_dsq_insert_vtime()` | Highest | Fair/deadline ordering |

#### Why Direct Dispatch is Fastest

```
Normal path (with enqueue):
  Task wakes → select_cpu() → enqueue() → dispatch() → Task runs
              [3 callback invocations, DSQ operations]

Direct dispatch path:
  Task wakes → select_cpu() + insert → Task runs
              [1 callback, task goes directly to CPU]
```

Direct dispatch skips the enqueue() and dispatch() callbacks entirely.

### 8.1.1 CRITICAL: Idle CPU Claiming Behavior

**WARNING:** Understanding idle CPU claiming is essential to avoid 0% direct dispatch bugs!

```
scx_bpf_pick_idle_cpu()      → FINDS AND CLAIMS (clears idle bit)
scx_bpf_test_and_clear_cpu_idle() → TESTS AND CLAIMS (clears idle bit)
```

**Both functions CLAIM the CPU by clearing the idle bit.** You cannot call both on the same CPU - the second call will always fail!

**Correct Pattern:**
```c
// Option A: Use pick_idle_cpu (finds AND claims)
cpu = scx_bpf_pick_idle_cpu(cpumask, 0);
if (cpu >= 0) {
    // CPU is already claimed - dispatch directly!
    scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, slice, 0);
}

// Option B: For specific CPU, use test_and_clear
if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
    // prev_cpu was idle and is now claimed
    scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | prev_cpu, slice, 0);
}
```

**WRONG Pattern (causes 0% direct dispatch):**
```c
// BROKEN: Double-claim always fails!
cpu = scx_bpf_pick_idle_cpu(cpumask, 0);  // Claims CPU
if (scx_bpf_test_and_clear_cpu_idle(cpu)) {  // ALWAYS FAILS - already claimed!
    // Never reaches here
}
```

### 8.2 Hybrid Dispatch Strategy for Gaming (v2.1)

**Design Decision:** Use direct dispatch as the PRIMARY path, shared DSQ for ALL fallbacks.

**Rationale:**
- Gaming workloads typically use 30-70% CPU (not saturated)
- Idle CPUs are usually available
- Direct dispatch gives lowest latency for input/GPU/audio
- **v2.1 CRITICAL FIX:** Enqueue fallback MUST use shared DSQ to prevent starvation

#### Why NOT Use Local DSQ in Enqueue?

Using `SCX_DSQ_LOCAL_ON | cpu` in enqueue() caused **severe starvation**:
- CPU-pinned kernel tasks (kworkers, ksoftirqd) sit in SHARED_DSQ
- They can ONLY run on their pinned CPU
- If that CPU's local DSQ always has work, `dispatch()` is never called
- Result: 40+ second starvation → system crash

**v2.1 Fix:** ALL enqueue() tasks go to SHARED_DSQ with deadline ordering.
Critical tasks get an early deadline + CPU kick for timely execution.

#### Expected Distribution During Gaming

| Scenario | Direct Dispatch | Shared DSQ (Critical + Kick) | Shared DSQ (Background) |
|----------|----------------|------------------------------|------------------------|
| Light gaming (CS2, Valorant) | ~90% | ~8% | ~2% |
| Heavy gaming (Cyberpunk) | ~70% | ~20% | ~10% |
| Gaming + compile | ~50% | ~25% | ~25% |

### 8.3 Dispatch Decision Tree

```
Task Wakes Up
      │
      ▼
┌─────────────────────────────────────────────────────────────────────┐
│                        SELECT_CPU CALLBACK                           │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ┌─────────────────────────┐                                        │
│  │ Is there an idle CPU?   │                                        │
│  └───────────┬─────────────┘                                        │
│              │                                                       │
│       ┌──────┴──────┐                                               │
│       │             │                                               │
│      YES           NO                                               │
│       │             │                                               │
│       ▼             │                                               │
│  ┌─────────────┐    │                                               │
│  │   DIRECT    │    │    Return prev_cpu, fall through to enqueue   │
│  │  DISPATCH   │    └──────────────────────────────────────────────►│
│  │             │                                                     │
│  │ Insert to   │                                                     │
│  │ local DSQ   │                                                     │
│  │ Skip enqueue│                                                     │
│  └─────────────┘                                                     │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
                              │
                              │ (only if no idle CPU)
                              ▼
┌─────────────────────────────────────────────────────────────────────┐
│                     ENQUEUE CALLBACK (v2.1)                          │
├─────────────────────────────────────────────────────────────────────┤
│                                                                      │
│  ALL tasks → SHARED_DSQ with deadline ordering                       │
│  (v2.1 fix: prevents starvation of CPU-pinned kernel tasks)         │
│                                                                      │
│  ┌───────────────────────────────┐                                  │
│  │ Is task latency-critical?     │                                  │
│  │ (input/gpu/audio/compositor)  │                                  │
│  └───────────────┬───────────────┘                                  │
│                  │                                                   │
│           ┌──────┴──────┐                                           │
│           │             │                                           │
│          YES           NO                                           │
│           │             │                                           │
│           ▼             ▼                                           │
│    ┌────────────┐  ┌─────────────────┐                              │
│    │ Shared DSQ │  │ Shared DSQ      │                              │
│    │ + early    │  │ + normal        │                              │
│    │   deadline │  │   deadline      │                              │
│    │ + KICK CPU │  │                 │                              │
│    │            │  │ Fair queuing    │                              │
│    │ Timely     │  │ for background  │                              │
│    │ pickup     │  │                 │                              │
│    └────────────┘  └─────────────────┘                              │
│                                                                      │
└─────────────────────────────────────────────────────────────────────┘
```

### 8.4 Implementation: select_cpu with Direct Dispatch

```c
/* Primary dispatch path - handles ~70-90% of tasks during gaming */

s32 BPF_STRUCT_OPS(gamer_select_cpu, struct task_struct *p, s32 prev_cpu, u64 wake_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    s32 cpu;
    
    /*
     * Strategy 1: Prefer prev_cpu for cache locality.
     * Try to claim prev_cpu FIRST before looking for other CPUs.
     * This dramatically improves cache hit rates and reduces migrations.
     */
    if (scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
        u64 slice = task_slice(tctx);
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | prev_cpu, slice, 0);
        
        STAT_INC(nr_direct_dispatch);
        STAT_INC(nr_same_cpu);
        return prev_cpu;
    }
    
    /*
     * Strategy 2: Find another idle CPU.
     * pick_idle_cpu() both FINDS and CLAIMS (clears idle bit).
     */
    cpu = select_cpu_for_task(p, tctx, prev_cpu, wake_flags);
    
    if (cpu >= 0 && cpu != prev_cpu) {
        /* Direct dispatch to idle CPU */
        u64 slice = task_slice(tctx);
        scx_bpf_dsq_insert(p, SCX_DSQ_LOCAL_ON | cpu, slice, 0);
        
        STAT_INC(nr_direct_dispatch);
        return cpu;
    }
    
    /*
     * SLOW PATH: No idle CPU found.
     * Fall through to enqueue() for priority-based handling.
     */
    return prev_cpu;
}
```

**v2.2 Cache Locality Fix:**
- **Strategy 1:** Try prev_cpu FIRST via `test_and_clear_cpu_idle`
- **Strategy 2:** Only look for other idle CPUs if prev_cpu is busy
- **Result:** Same CPU reuse went from 0.002% → 50-80%

**Why this matters for gaming:**
- L1/L2 caches are per-core (~12 cycles to access)
- L3 is shared (~40 cycles)
- RAM is slow (~150+ cycles)
- Keeping tasks on prev_cpu means cache warm data stays accessible

### 8.5 Implementation: enqueue with Fallback (v2.1)

```c
/* Fallback path - only called when no idle CPU was found (~10-30% of tasks) */

void BPF_STRUCT_OPS(gamer_enqueue, struct task_struct *p, u64 enq_flags)
{
    struct task_ctx *tctx = lookup_task_ctx(p);
    u64 slice = task_slice(tctx);
    s32 cpu = scx_bpf_task_cpu(p);
    bool is_critical = tctx && task_is_latency_critical(tctx);
    u8 boost = tctx ? tctx->boost_shift : BOOST_BACKGROUND;
    
    STAT_INC(nr_enqueued);
    
    /*
     * v2.1 CRITICAL FIX: ALL tasks go to SHARED_DSQ.
     *
     * Using local DSQ here caused starvation of CPU-pinned tasks:
     * - kworkers/ksoftirqd pinned to specific CPUs sit in SHARED_DSQ
     * - If local DSQ always has work, dispatch() is never called
     * - Result: 40+ second starvation → system crash
     *
     * Solution: Use SHARED_DSQ for everything. Critical tasks get
     * early deadlines + CPU kick for timely execution.
     */
    u64 deadline = task_deadline(p, tctx, vtime_now);
    scx_bpf_dsq_insert_vtime(p, SHARED_DSQ, slice, deadline, enq_flags);
    STAT_INC(nr_shared_dispatch);
    
    if (is_critical) {
        /* Kick CPU to ensure timely pickup of critical task */
        smart_kick_cpu(cpu, boost);
    }
}
```

**Why ALL tasks use SHARED_DSQ in enqueue (v2.1):**
- Prevents starvation of CPU-pinned kernel tasks
- dispatch() is always called, serving both critical and pinned tasks
- Critical tasks get early deadlines (boost_shift) for priority
- CPU kick ensures timely execution for latency-critical tasks

### 8.6 Implementation: dispatch Callback

```c
/* Called when a CPU needs work */

void BPF_STRUCT_OPS(gamer_dispatch, s32 cpu, struct task_struct *prev)
{
    /*
     * Local DSQ is automatically consumed by the kernel.
     * We only need to check the shared DSQ here.
     */
    scx_bpf_dsq_move_to_local(SHARED_DSQ);
}
```

### 8.7 Preemption Decision (Smart Kick)

```c
/*
 * Priority-aware preemption.
 * Only send expensive IPI if waking task has HIGHER priority.
 */
static __always_inline void smart_kick_cpu(s32 cpu, u8 waking_boost)
{
    struct cpu_ctx *cctx = lookup_cpu_ctx(cpu);
    if (!cctx)
        return;
    
    u8 running_boost = cctx->current_boost;
    
    if (waking_boost > running_boost) {
        /* Higher priority: preempt immediately */
        scx_bpf_kick_cpu(cpu, SCX_KICK_PREEMPT);
        STAT_INC(nr_preempt_kick);
    } else {
        /* Equal or lower: signal without forced preemption */
        scx_bpf_kick_cpu(cpu, SCX_KICK_IDLE);
        STAT_INC(nr_preempt_avoided);
    }
}
```

**Why smart kick matters:**
- `SCX_KICK_PREEMPT`: Sends IPI, forces immediate context switch (~1-5µs)
- `SCX_KICK_IDLE`: Signals CPU, task runs after current slice (~10-1000µs)
- Avoiding unnecessary IPIs reduces frametime variance

### 8.8 Dispatch Path Summary (v2.1)

| Path | When Used | Latency | Overhead |
|------|-----------|---------|----------|
| **Direct dispatch** | Idle CPU found (select_cpu) | ~100ns | Minimal |
| **Shared DSQ + kick** | No idle CPU, latency-critical (enqueue) | ~500ns-5µs | Low |
| **Shared DSQ (deadline)** | No idle CPU, background task (enqueue) | ~10-100µs | Medium |

**v2.1 Note:** "Local DSQ + kick" path removed to prevent starvation of CPU-pinned tasks.

### 8.9 Why Not Always Use Direct Dispatch?

Direct dispatch requires an idle CPU. When none exists:

```
Problem scenarios:
┌─────────────────────────────────────────────────────────────────────┐
│ 1. All CPUs busy with game threads                                  │
│    → Input handler has nowhere to direct dispatch                   │
│    → Need preemptive kick fallback                                  │
├─────────────────────────────────────────────────────────────────────┤
│ 2. Background tasks accumulating                                    │
│    → If always direct dispatch, they starve                         │
│    → Need fair scheduling fallback (deadline ordering)              │
├─────────────────────────────────────────────────────────────────────┤
│ 3. Gaming + compile (CPU saturated)                                 │
│    → Multiple tasks competing for same CPUs                         │
│    → Need deadline ordering to prevent game starvation              │
└─────────────────────────────────────────────────────────────────────┘
```

**The hybrid approach handles all scenarios optimally.**

### 8.10 Comparison with scx_cosmos

scx_cosmos uses a similar hybrid approach:

| Aspect | scx_cosmos | scx_gamer |
|--------|------------|-----------|
| Primary path | Direct dispatch | Direct dispatch |
| Fallback trigger | `is_system_busy()` | No idle CPU found |
| Latency-critical handling | Same as background | Separate path with kick |
| Priority system | Kernel nice weights | Hook-based boost (0-7) |
| Detection | Behavioral (wakeup freq) | Kernel hooks (100% proof) |

**scx_gamer advantage:** We KNOW which tasks are latency-critical via hooks, so we can give them the preemptive kick path even when CPUs are busy.

### 8.11 Expected Results

#### Performance Impact

| Metric | Before (Always Shared DSQ) | After (Hybrid) | Expected Change |
|--------|---------------------------|----------------|-----------------|
| Input latency | ~10-50µs | ~1-10µs | **5-10x better** |
| Average frametime | ~4.0ms | ~3.8ms | ~5% better |
| 1% low frametime | ~6.0ms | ~4.5ms | **25% better** |
| Scheduler overhead | High | Low | **50-70% reduction** |
| Scheduler CPU usage | ~8% | ~3% | **60% reduction** |

#### Why These Improvements?

```
Current (Always Shared DSQ):
  - Every task: select_cpu → enqueue → deadline calc → vtime insert → dispatch
  - Cross-CPU lock contention on shared DSQ
  - Cache line bouncing between CPUs
  - ~1,100ns overhead per task

Hybrid (Direct Dispatch Primary):
  - 70-90% of tasks: select_cpu + direct insert → done
  - No lock contention (per-CPU local DSQ)
  - Cache-local operations
  - ~300ns overhead per task (70% reduction)
```

#### Validation Metrics

After implementation, statistics should show:

```
nr_direct_dispatch:   70-90% of total dispatches (was 0%)
nr_shared_dispatch:   2-10% of total dispatches (was 100%)

Wait time histogram:
  <1µs bucket:        Significant increase
  >1ms bucket:        Significant decrease

Worst-case frametime: ~4.5ms (was ~6ms)
Max wait time:        ~3ms (was ~6ms)
```

#### Risk Assessment

| Risk | Likelihood | Mitigation |
|------|------------|------------|
| Task starvation | Low | Deadline-ordered fallback for background |
| Load imbalance | Low | `pick_idle_cpu` distributes work |
| Regression | Low | Can revert to shared DSQ if issues |

---

## 9. Userspace Components

### 9.1 main.rs Structure

```rust
// main.rs - Entry point (~300 lines max)

mod args;
mod focus;
mod stats;
mod topology;

fn main() -> Result<()> {
    // 1. Parse arguments
    let args = args::parse();
    
    // 2. Initialize logging
    init_logging(args.verbose);
    
    // 3. Load and configure BPF
    let mut skel = load_bpf(&args)?;
    configure_tunables(&mut skel, &args)?;
    
    // 4. Initialize topology
    let topo = topology::detect()?;
    configure_cpu_contexts(&mut skel, &topo)?;
    
    // 5. Attach scheduler
    let _link = skel.attach()?;
    info!("scx_gamer started");
    
    // 6. Start focus detection (background thread)
    let focus_rx = focus::start_detection()?;
    
    // 7. Main event loop
    run_event_loop(&mut skel, focus_rx, &args)?;
    
    Ok(())
}

fn run_event_loop(skel: &mut Skel, focus_rx: Receiver<u32>, args: &Args) -> Result<()> {
    let mut last_stats = Instant::now();
    
    loop {
        // Check for exit signal
        if should_exit() {
            break;
        }
        
        // Handle focus changes
        if let Ok(tgid) = focus_rx.try_recv() {
            skel.maps.rodata.foreground_tgid = tgid;
            info!("Focus changed to tgid {}", tgid);
        }
        
        // Print stats periodically
        if args.stats_interval > 0 && last_stats.elapsed() > Duration::from_secs(args.stats_interval) {
            stats::print(&skel);
            last_stats = Instant::now();
        }
        
        // Sleep to avoid busy-waiting
        std::thread::sleep(Duration::from_millis(100));
    }
    
    Ok(())
}
```

### 9.2 CLI Arguments (args.rs)

```rust
// args.rs - CLI parsing (~150 lines)

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "scx_gamer")]
#[command(about = "Gaming-optimized sched_ext scheduler")]
#[command(version)]
pub struct Args {
    // === Performance Tuning ===
    
    /// Base time slice in microseconds (default: 10)
    /// Lower = more responsive but more overhead
    /// Range: 1-1000, recommend 5-20 for gaming
    #[arg(long, default_value = "10")]
    pub slice_us: u64,
    
    /// Avoid SMT siblings for latency-critical tasks
    /// Recommended: true for gaming (reduces cache thrashing)
    #[arg(long, default_value = "true")]
    pub avoid_smt: bool,
    
    // === Statistics & Monitoring ===
    
    /// Statistics display interval in seconds (0 = disabled)
    /// Used by: ./start.sh --stats
    #[arg(long, short = 's', default_value = "0")]
    pub stats: u64,
    
    /// Disable ALL statistics collection for maximum performance
    /// Removes atomic operations from hot path
    /// Used by: ./start.sh --perf
    #[arg(long)]
    pub no_stats: bool,
    
    // === Debugging ===
    
    /// Enable verbose logging
    #[arg(short, long)]
    pub verbose: bool,
    
    /// Enable BPF debug prints (requires trace_pipe)
    /// Used by: ./start.sh --debug
    #[arg(long)]
    pub debug: bool,
    
    // === Advanced ===
    
    /// Number of high-performance CPUs to prefer (0 = auto-detect)
    #[arg(long, default_value = "0")]
    pub preferred_cpus: u32,
    
    /// Manually set foreground game PID (0 = auto-detect via D-Bus)
    #[arg(long, default_value = "0")]
    pub foreground_pid: u32,
}

impl Args {
    /// Validate arguments and return errors
    pub fn validate(&self) -> Result<(), String> {
        if self.slice_us == 0 || self.slice_us > 1000 {
            return Err("slice_us must be between 1 and 1000".into());
        }
        if self.no_stats && self.stats > 0 {
            return Err("Cannot use --no-stats with --stats".into());
        }
        Ok(())
    }
}
```

**CLI Usage Examples:**

```bash
# Normal gaming (auto stats every 5s via start.sh)
./start.sh

# Maximum performance (benchmarking, competitive)
./start.sh --perf
# Equivalent to: scx_gamer --no-stats

# Detailed monitoring
./start.sh --stats
# Equivalent to: scx_gamer --stats 1

# Debug mode (troubleshooting)
./start.sh --debug
# Equivalent to: scx_gamer --stats 1 --debug (with trace_pipe)

# Custom tuning
sudo ./target/release/scx_gamer --slice-us 5 --stats 2

# Force specific game PID
sudo ./target/release/scx_gamer --foreground-pid 12345
```

### 9.3 Focus Detection (focus.rs)

```rust
// focus.rs - Window focus detection (~200 lines)

use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use zbus::Connection;

pub fn start_detection() -> Result<Receiver<u32>> {
    let (tx, rx) = mpsc::channel();
    
    thread::spawn(move || {
        if let Err(e) = run_focus_loop(tx) {
            eprintln!("Focus detection error: {}", e);
        }
    });
    
    Ok(rx)
}

fn run_focus_loop(tx: Sender<u32>) -> Result<()> {
    let conn = Connection::session()?;
    
    // Subscribe to KWin focus changes via D-Bus
    // Implementation details...
    
    loop {
        // Wait for focus change signal
        // Extract PID of focused window
        // Send to main thread
        tx.send(focused_pid)?;
    }
}
```

---

# Part III: Implementation

## 10. File Manifest

### 10.1 BPF Files

| File | Lines | Purpose | Dependencies |
|------|-------|---------|--------------|
| `main.bpf.c` | ~400 | struct_ops callbacks | All headers |
| `intf.h` | ~50 | Userspace interface | None |
| `config.bpf.h` | ~100 | Tunables, constants | None |
| `types.bpf.h` | ~200 | Data structures, maps | config.bpf.h |
| `helpers.bpf.h` | ~100 | Utility functions | types.bpf.h |
| `detection/input.bpf.h` | ~150 | Input hooks | helpers.bpf.h |
| `detection/gpu.bpf.h` | ~200 | GPU hooks | helpers.bpf.h |
| `detection/audio.bpf.h` | ~100 | Audio hooks | helpers.bpf.h |
| `detection/sync.bpf.h` | ~150 | Wine/Proton hooks | helpers.bpf.h |
| `detection/net.bpf.h` | ~100 | Network hooks | helpers.bpf.h |
| `priority/boost.bpf.h` | ~100 | Priority calculation | types.bpf.h |
| `core/cpu_select.bpf.h` | ~200 | CPU selection | boost.bpf.h |
| `core/enqueue.bpf.h` | ~150 | Enqueue logic | cpu_select.bpf.h |
| `core/dispatch.bpf.h` | ~100 | Dispatch logic | types.bpf.h |

**Total BPF: ~2,100 lines** (down from 8,478)

### 10.2 Rust Files

| File | Lines | Purpose |
|------|-------|---------|
| `main.rs` | ~300 | Entry point, event loop |
| `args.rs` | ~100 | CLI parsing |
| `focus.rs` | ~200 | Focus detection |
| `stats.rs` | ~150 | Statistics display |
| `topology.rs` | ~100 | CPU topology |
| `lib.rs` | ~50 | Module exports |

**Total Rust: ~900 lines** (down from 3,159)

---

## 11. Implementation Phases

### Phase 1: Skeleton (Est: 4 hours)

**Goal:** Minimal scheduler that loads and runs without crashing.

**Tasks:**
- [ ] Create `src/bpf/v2/` directory structure
- [ ] Write `config.bpf.h` with essential constants
- [ ] Write `types.bpf.h` with task_ctx, cpu_ctx, maps
- [ ] Write `main.bpf.c` with stub callbacks
- [ ] Verify compilation with `./build.sh`
- [ ] Verify loading with `sudo ./target/release/scx_gamer`

**Validation:** Scheduler loads, system remains stable, `scx_bpf_ksym_id` shows it's active.

### Phase 2: Priority System (Est: 4 hours)

**Goal:** Tasks get correct boost levels.

**Tasks:**
- [ ] Implement `priority/boost.bpf.h`
- [ ] Add deadline calculation
- [ ] Add slice calculation
- [ ] Test: manually set flags, verify boost in stats

**Validation:** Stats show boost distribution across 8 levels.

### Phase 3: Detection Hooks (Est: 8 hours)

**Goal:** Automatic thread classification via fentry.

**Tasks:**
- [ ] Port `detection/input.bpf.h` (3 hooks)
- [ ] Port `detection/gpu.bpf.h` (3 hooks)
- [ ] Port `detection/audio.bpf.h` (2 hooks)
- [ ] Port `detection/sync.bpf.h` (4 hooks)
- [ ] Port `detection/net.bpf.h` (4 hooks)
- [ ] Test: run game, check detection counters

**Validation:** Running CS2/Overwatch shows detection hits in stats.

### Phase 4: CPU Selection (Est: 4 hours)

**Goal:** Intelligent CPU placement.

**Tasks:**
- [ ] Implement `core/cpu_select.bpf.h`
- [ ] Add physical core preference for GPU/input
- [ ] Add SMT avoidance logic
- [ ] Test: verify GPU threads on physical cores

**Validation:** `nr_physical_selected` > `nr_smt_selected` for latency-critical tasks.

### Phase 5: Dispatch (Est: 4 hours)

**Goal:** Complete scheduling loop.

**Tasks:**
- [ ] Implement `core/enqueue.bpf.h`
- [ ] Implement `core/dispatch.bpf.h`
- [ ] Add smart preemption (kick only if higher priority)
- [ ] Test: full game session

**Validation:** Game runs smoothly, stats show direct dispatch for critical tasks.

### Phase 6: Userspace (Est: 4 hours)

**Goal:** Clean Rust implementation.

**Tasks:**
- [ ] Write `args.rs`
- [ ] Write `focus.rs`
- [ ] Write `stats.rs`
- [ ] Write `topology.rs`
- [ ] Integrate in slim `main.rs`

**Validation:** `--help` works, focus changes update foreground_tgid.

### Phase 7: Benchmarking (Est: 8 hours)

**Goal:** Prove performance improvements.

**Tasks:**
- [ ] Run `scripts/game_perf_monitor.sh` on v1 and v2
- [ ] Compare against scx_cosmos
- [ ] Profile hot paths with BPF profiling
- [ ] Document results

**Validation:**
- 99th percentile frametime ≤4.17ms
- Frames >5ms <0.5%
- FPS stdev <50

---

## 12. Testing and Validation

### 12.1 Unit Tests (BPF)

```c
// In-code assertions for development
#ifdef DEBUG
#define BPF_ASSERT(cond, msg) \
    if (!(cond)) { \
        bpf_printk("ASSERT FAILED: %s", msg); \
        return -EINVAL; \
    }
#else
#define BPF_ASSERT(cond, msg)
#endif
```

### 12.2 Integration Tests

| Test | Command | Expected Result |
|------|---------|-----------------|
| Load test | `sudo ./scx_gamer` | Loads without error |
| Stability test | Run for 1 hour | No crashes |
| Game test | Play CS2 30 min | No stutters |
| Detection test | Check stats | Input/GPU hooks fire |

### 12.3 Benchmark Protocol

```bash
# 1. Start monitoring
./scripts/game_perf_monitor.sh &

# 2. Run game for 5 minutes (controlled scenario)
# - CS2: Workshop aim training map
# - Fixed settings: 1080p, low, uncapped FPS

# 3. Stop monitoring, save results
kill %1
mv perf_results.csv benchmarks/v2_$(date +%Y%m%d).csv

# 4. Compare
./scripts/compare_metrics.sh benchmarks/v1_baseline.csv benchmarks/v2_*.csv
```

---

# Part IV: Reference

## 13. Research Background

### 13.1 Why Not EEVDF?

EEVDF (Linux 6.6+) improved on CFS but still has gaming limitations:

| Issue | EEVDF Behavior | scx_gamer Solution |
|-------|----------------|-------------------|
| No semantic awareness | Treats all SCHED_OTHER equally | fentry hooks identify thread roles |
| Nice=20 penalty | Wine games get 68x vruntime penalty | Ignore nice, use hook-based boost |
| Fairness over responsiveness | Input thread waits its turn | Input gets 128x priority boost |
| No frame deadline awareness | Virtual deadlines unrelated to vsync | Track frame_interval_ns |
| Reactive | Schedule after wake | A.B.C.: prepare CPU before wake |

### 13.2 Key Concepts from Other Schedulers

**From scx_lavd:**
- Latency criticality measurement (we use simpler flag-based)
- Adaptive power management (future consideration)

**From scx_bpfland:**
- vruntime-based fairness (we use deadline-based)
- Interactive detection (we use hooks instead)

**From scx_cosmos:**
- Short time slices (we adopt similar ~10µs)
- SMT awareness (we prioritize physical cores)

**From ADIOS I/O scheduler:**
- Adaptive learning (future: frame timing adaptation)
- Tiered priority (we use 8 boost levels)

### 13.3 References

| Topic | Source |
|-------|--------|
| sched_ext | [kernel.org/doc/html/latest/scheduler/sched-ext.html](https://kernel.org/doc/html/latest/scheduler/sched-ext.html) |
| EEVDF | [kernel.org/doc/html/latest/scheduler/sched-eevdf.html](https://kernel.org/doc/html/latest/scheduler/sched-eevdf.html) |
| scx_lavd | [github.com/sched-ext/scx](https://github.com/sched-ext/scx) |
| Low latency | [github.com/penberg/awesome-low-latency](https://github.com/penberg/awesome-low-latency) |

### 13.4 Linux Scheduler Evolution

Understanding how Linux scheduling evolved helps explain why scx_gamer takes its approach.

```
┌─────────────────────────────────────────────────────────────────────────┐
│                    Linux Scheduler Evolution                            │
│                                                                         │
│  ┌────────┐   ┌────────┐     ┌────────┐          ┌────────┐            │
│  │  O(n)  │──▶│  O(1)  │────▶│  CFS   │─────────▶│ EEVDF  │            │
│  │Scheduler│   │Scheduler│    │        │          │        │            │
│  └────────┘   └────────┘     └────────┘          └────────┘            │
│                                                                         │
│  Linux 2.4    Linux 2.6.0    Linux 2.6.23        Linux 6.6             │
└─────────────────────────────────────────────────────────────────────────┘
```

#### O(n) Scheduler (Linux 2.4)

**Mechanism:**
- Divided CPU time into "epochs"
- Each task received a time slice per epoch
- Selected next task by scanning ALL runnable tasks
- Unused time carried forward to next epoch

**Problems:**
```c
// O(n) task selection - scales poorly!
struct task_struct *pick_next_task(void) {
    struct task_struct *best = NULL;
    int best_goodness = -1000;
    
    // Scan every task - O(n) complexity
    for_each_runnable_task(p) {
        int goodness = calculate_goodness(p);
        if (goodness > best_goodness) {
            best = p;
            best_goodness = goodness;
        }
    }
    return best;
}
```

With thousands of tasks, this became a bottleneck on servers.

#### O(1) Scheduler (Linux 2.6.0-2.6.22)

**Innovation:** Constant-time task selection regardless of task count.

**Mechanism:**
```c
// Two priority arrays: active and expired
struct prio_array {
    int nr_active;
    unsigned long bitmap[BITMAP_SIZE];  // One bit per priority
    struct list_head queue[MAX_PRIO];   // 140 priority levels
};

// O(1) selection using bitmap
struct task_struct *pick_next_task(void) {
    // Find first set bit in bitmap - O(1) with hardware instruction
    int idx = sched_find_first_bit(active->bitmap);
    return list_first_entry(&active->queue[idx]);
}
```

**Problems:**
- Complex heuristics for interactive vs batch detection
- Interactivity scoring was fragile and game-able
- Priority arrays caused unfairness under certain workloads

#### Completely Fair Scheduler - CFS (Linux 2.6.23-6.5)

**Philosophy:** Model an "ideal, precise multi-tasking CPU" where each task runs simultaneously.

**Core Concept: Virtual Runtime (vruntime)**

```c
// Virtual runtime tracks "fair share" of CPU
// Higher weight = vruntime increases slower = more CPU time

// Weight table (nice 0 = 1024)
static const int prio_to_weight[40] = {
    /* -20 */ 88761, 71755, 56483, 46273, 36291,
    /* -15 */ 29154, 23254, 18705, 14949, 11916,
    /* -10 */  9548,  7620,  6100,  4904,  3906,
    /*  -5 */  3121,  2501,  1991,  1586,  1277,
    /*   0 */  1024,   820,   655,   526,   423,  // nice 0 = 1024
    /*   5 */   335,   272,   215,   172,   137,
    /*  10 */   110,    87,    70,    56,    45,
    /*  15 */    36,    29,    23,    18,    15,  // nice 19 = 15
};

// vruntime calculation
vruntime += (actual_runtime * NICE_0_WEIGHT) / task_weight;

// Nice 0:  1ms actual = 1ms vruntime
// Nice -20: 1ms actual = 0.012ms vruntime (88x more CPU)
// Nice +19: 1ms actual = 68ms vruntime (68x less CPU)
```

**The Nice=20 Problem for Wine/Proton:**

```c
// Wine sets nice=20 for Windows NORMAL_PRIORITY_CLASS
// This causes 68x vruntime penalty!

// Example: CS2 thread runs for 1ms
// Nice 0:   vruntime += 1ms
// Nice 20:  vruntime += 68ms  // Severely penalized!

// Result: Game threads fall behind in red-black tree
// They wait while background tasks (nice 0) run first
```

**Red-Black Tree Structure:**

```
CFS Run Queue (rb-tree ordered by vruntime):
                    ┌──────────────────┐
                    │   vruntime: 500  │
                    │   (background)   │
                    └────────┬─────────┘
                   ┌─────────┴─────────┐
         ┌─────────┴───────┐ ┌─────────┴───────┐
         │  vruntime: 300  │ │  vruntime: 700  │
         │  (game thread)  │ │  (compiler)     │
         └────────┬────────┘ └─────────────────┘
                  │
    Leftmost = runs next (smallest vruntime)
```

**CFS Problems for Gaming:**

1. **No semantic awareness** - All SCHED_OTHER tasks treated equally
2. **Nice abuse** - Wine's nice=20 causes severe penalty
3. **Heuristic-based interactivity** - Sleep/wake patterns guessed
4. **No deadline concept** - Frame deadlines invisible to scheduler
5. **Fairness over responsiveness** - Mouse thread waits its turn

#### EEVDF Scheduler (Linux 6.6+)

**Source:** Based on academic paper "A Proportional Share Resource Allocation Algorithm" (Stoica et al., 1996)

**Key Innovation:** Virtual Deadlines + Eligibility

**Core Concepts:**

```c
// 1. VIRTUAL RUNTIME (inherited from CFS)
vruntime += (actual_runtime * NICE_0_WEIGHT) / task_weight;

// 2. LAG: Difference between ideal and actual CPU time
// Positive lag = task is owed CPU time
// Negative lag = task has exceeded its share
lag = ideal_runtime - actual_runtime;

// 3. ELIGIBLE TIME: When task becomes eligible to run
// Task must have lag >= 0 to be eligible
eligible = (lag >= 0);

// 4. VIRTUAL DEADLINE: When task's time slice should complete
// Shorter slice = earlier deadline = higher priority
virtual_deadline = eligible_time + (slice_length / weight);
```

**EEVDF Selection Algorithm:**

```c
struct task_struct *pick_next_task_eevdf(struct rq *rq) {
    struct sched_entity *best = NULL;
    u64 best_deadline = U64_MAX;
    
    // Find eligible task with earliest virtual deadline
    for_each_task_in_rbtree(se) {
        if (se->lag < 0)
            continue;  // Not eligible
        
        if (se->deadline < best_deadline) {
            best = se;
            best_deadline = se->deadline;
        }
    }
    return task_of(best);
}
```

**Why EEVDF Became Standard:**

| Aspect | CFS | EEVDF |
|--------|-----|-------|
| Selection criterion | Smallest vruntime | Earliest deadline among eligible |
| Latency guarantee | None (statistical) | Bounded by slice length |
| Fairness enforcement | vruntime only | lag + deadline |
| Gaming prevention | Weak | Deferred dequeue |
| Tunability | Many knobs | Slice length via `sched_setattr()` |

**EEVDF `sched_setattr()` API:**

```c
// Applications can request specific time slices
struct sched_attr {
    __u32 size;
    __u32 sched_policy;     // SCHED_OTHER
    __u64 sched_runtime;    // Requested slice (ns)
};

// Example: Request 1ms slice for latency-sensitive task
attr.sched_runtime = 1000000;  // 1ms
sched_setattr(pid, &attr, 0);

// Result: Task gets earlier virtual deadline
// Shorter slice = earlier deadline = scheduled sooner
```

#### Linux Scheduling Classes Hierarchy

```
Priority (highest to lowest):
┌─────────────────────────────────────────────────────────────────────────┐
│                                                                         │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ SCHED_DEADLINE (Earliest Deadline First + CBS)                  │   │
│  │ - Real-time with bandwidth reservation                          │   │
│  │ - Specify: runtime, deadline, period                            │   │
│  │ - Highest priority, preempts everything                         │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                                          │
│                              ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ SCHED_FIFO / SCHED_RR (Real-time)                               │   │
│  │ - Fixed priority (1-99)                                         │   │
│  │ - FIFO: Run until yield/block                                   │   │
│  │ - RR: Round-robin with time quantum                             │   │
│  │ - Preempts SCHED_OTHER                                          │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                                          │
│                              ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ SCHED_OTHER / SCHED_NORMAL (Default - EEVDF)                    │   │
│  │ - Nice values (-20 to +19)                                      │   │
│  │ - Where most processes run                                      │   │
│  │ - Games, desktop apps, servers                                  │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                                          │
│                              ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ SCHED_BATCH                                                     │   │
│  │ - Like SCHED_OTHER but never considered "interactive"           │   │
│  │ - For throughput-oriented batch jobs                            │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                              │                                          │
│                              ▼                                          │
│  ┌─────────────────────────────────────────────────────────────────┐   │
│  │ SCHED_IDLE                                                      │   │
│  │ - Lowest priority                                               │   │
│  │ - Only runs when nothing else wants CPU                         │   │
│  └─────────────────────────────────────────────────────────────────┘   │
│                                                                         │
└─────────────────────────────────────────────────────────────────────────┘
```

### 13.5 ADIOS I/O Scheduler Deep Dive

[ADIOS](https://github.com/firelzrd/adios) (Adaptive Deadline I/O Scheduler) provides insights applicable to CPU scheduling.

#### Adaptive Latency Learning

ADIOS learns optimal latency targets instead of using fixed thresholds:

```c
// ADIOS maintains a global latency window (GLW)
struct adaptive_model {
    u64 latency_sum;        // Sum of observed latencies
    u64 latency_count;      // Number of samples
    u64 latency_target;     // Learned target (adaptive)
    u64 model_age;          // For decay/shrinkage
};

// Update model with new observation
void update_latency_model(struct adaptive_model *m, u64 observed_latency) {
    // Exponential moving average
    m->latency_sum = (m->latency_sum * 7 + observed_latency) / 8;
    m->latency_count++;
    
    // Derive target from observations
    m->latency_target = m->latency_sum / max(m->latency_count, 1);
}
```

**Application to scx_gamer:**
- Learn optimal frame intervals per game
- Adapt slice durations based on observed patterns
- Track input→render latency distribution

#### Priority Tiers

ADIOS uses tiered priority with clear boundaries:

```
┌─────────────────────────────────────────────────┐
│ Tier 0: REALTIME                                │
│   - Deadline I/O, synchronous reads             │
│   - Always processed first                      │
├─────────────────────────────────────────────────┤
│ Tier 1: INTERACTIVE                             │
│   - User-initiated I/O                          │
│   - Short deadline from observed patterns       │
├─────────────────────────────────────────────────┤
│ Tier 2: BATCH                                   │
│   - Background I/O                              │
│   - Longer deadline, opportunistic              │
├─────────────────────────────────────────────────┤
│ Tier 3: IDLE                                    │
│   - Only when system is idle                    │
│   - No deadline guarantee                       │
└─────────────────────────────────────────────────┘
```

**Mapping to scx_gamer boost_shift:**

| ADIOS Tier | scx_gamer boost_shift | Task Type |
|------------|----------------------|-----------|
| Tier 0 | 7 | Input handlers |
| Tier 1 | 5-6 | GPU, Audio |
| Tier 2 | 2-4 | Game workers, Compositor |
| Tier 3 | 0-1 | Background |

#### Model Shrinkage

ADIOS implements model shrinkage to prevent stale data from dominating:

```c
// Models decay over time - recent data weighted more heavily
void shrink_model(struct adaptive_model *m, u64 now) {
    u64 age = now - m->last_update;
    
    if (age > MODEL_HALF_LIFE) {
        // Halve the sample count (reduces confidence in old data)
        m->latency_count >>= 1;
        m->latency_sum >>= 1;
        m->last_update = now;
    }
}
```

**Application to scx_gamer:**
- Frame timing models should decay when game phase changes
- Thread classification confidence decays without re-confirmation
- Prevents "stale boost" from holding after behavior changes

### 13.6 scx_lavd Deep Dive

[scx_lavd](https://github.com/sched-ext/scx/tree/main/scheds/rust/scx_lavd) (Latency-Aware Virtual Deadline) provides behavioral analysis we can learn from.

#### Latency Criticality Formula

```c
// scx_lavd's latency criticality calculation
lat_cri = (1000 * wait_freq * wake_freq) / (run_time_ns + 1);

// Where:
// - wait_freq: How often task waits for I/O or sync
// - wake_freq: How often task wakes up  
// - run_time_ns: Average runtime per wake

// High lat_cri = frequently waking, short bursts = interactive
// Low lat_cri = long running, infrequent wakes = batch
```

**Why scx_gamer uses hooks instead:**

| Aspect | scx_lavd (Behavioral) | scx_gamer (Hook-based) |
|--------|----------------------|------------------------|
| Detection time | Multiple wakes needed | First syscall |
| Accuracy | ~80% (patterns can mislead) | 100% (kernel proof) |
| Warmup period | 5-10 frames | Zero |
| False positives | Possible | Impossible |

**What we borrow:**
- Virtual deadline formula structure
- Per-task runtime tracking
- Starvation detection concept

#### Greedy Ratio

scx_lavd tracks how "greedy" tasks are:

```c
// Greedy ratio = actual runtime / fair share
greedy_ratio = task_runtime / (total_runtime / nr_tasks);

// greedy_ratio > 1.0: Task is taking more than fair share
// greedy_ratio < 1.0: Task is taking less than fair share
```

**Application to scx_gamer:**
- Use greedy_ratio for starvation detection
- Rescue tasks with greedy_ratio << 1.0 that haven't run in 500ms+

### 13.7 scx_bpfland Deep Dive

[scx_bpfland](https://github.com/sched-ext/scx/tree/main/scheds/rust/scx_bpfland) focuses on interactive desktop workloads.

#### Interactive Detection Heuristics

```c
// scx_bpfland's interactive classification
bool is_interactive(struct task_ctx *tctx) {
    // Short average runtime suggests interactive
    if (tctx->avg_runtime_ns < INTERACTIVE_RUNTIME_THRESH)
        return true;
    
    // High voluntary context switch rate suggests I/O-bound interactive
    if (tctx->voluntary_ctx_switches > INTERACTIVE_VCS_THRESH)
        return true;
    
    return false;
}
```

**Why this is fragile for gaming:**
- Games have variable runtime (menu vs combat)
- GPU threads may have long runtimes but are latency-critical
- Wine/Proton threads have unusual patterns

**What we borrow:**
- NUMA awareness for large systems
- CPU topology handling
- DSQ organization patterns

#### Voluntary Preemption

scx_bpfland implements voluntary preemption for fairness:

```c
// Check if current task should yield
bool should_yield(struct task_struct *curr) {
    // Yielded already
    if (curr->scx.slice == 0)
        return true;
    
    // Higher priority task waiting
    if (dsq_has_higher_priority_task())
        return true;
    
    return false;
}
```

**Application to scx_gamer:**
- Use slice exhaustion as yield signal
- Preempt only when boost_shift difference warrants it

### 13.8 scx_cosmos Deep Dive

[scx_cosmos](https://github.com/sched-ext/scx/tree/main/scheds/rust/scx_cosmos) achieves excellent gaming performance through simplicity.

#### Why cosmos performs well

1. **Short time slices** (~10µs) - Quick response to new work
2. **Simple priority** - No complex heuristics
3. **Aggressive idle CPU finding** - Minimizes contention

**scx_cosmos baseline metrics (our target to beat):**
- 99th percentile: 3.254ms
- Frames >5ms: 0.34%
- FPS stdev: 38

#### What cosmos lacks (our opportunity)

| Feature | scx_cosmos | scx_gamer v2 |
|---------|------------|--------------|
| Input prioritization | None | 128x boost via fentry |
| GPU awareness | None | drm_ioctl detection |
| A.B.C. preparation | None | Speculative preemption |
| Wine/Proton optimization | None | esync/fsync/ntsync hooks |

### 13.9 Low-Latency Programming Principles

#### LMAX Disruptor Patterns

The [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) achieves sub-microsecond latency through:

**Single Writer Principle:**
```c
// BAD: Multiple writers contend on atomic
__sync_fetch_and_add(&shared_counter, 1);  // Cache line ping-pong

// GOOD: Per-CPU counters, aggregate on read
struct per_cpu_stats {
    u64 counter;
    u64 _pad[7];  // Pad to cache line
} __attribute__((aligned(64)));
```

**Application to scx_gamer:**
- Per-CPU statistics instead of global atomics
- Cache-line aligned structures (64 bytes)
- Avoid false sharing in hot paths

#### Mechanical Sympathy

[Mechanical Sympathy](https://mechanical-sympathy.blogspot.com/) emphasizes hardware awareness:

**Cache Line Awareness:**
```c
// task_ctx is exactly 64 bytes = 1 cache line
// Hot fields (flags, boost_shift) at offset 0
// Cold fields (reserved) at end

struct task_ctx {
    u8 flags;           // Byte 0 - always accessed
    u8 boost_shift;     // Byte 1 - always accessed
    // ... hot fields ...
    u64 _reserved[2];   // Bytes 48-63 - rarely accessed
} __attribute__((aligned(64)));
```

**Branch Prediction:**
```c
// Use likely/unlikely hints for predictable branches
if (likely(tctx != NULL)) {
    // Normal path - branch predictor optimizes for this
}

if (unlikely(error_condition)) {
    // Rare path - branch predictor skips this
}
```

### 13.10 Research References

| Reference | Key Contribution | scx_gamer Application |
|-----------|------------------|----------------------|
| [ADIOS I/O Scheduler](https://github.com/firelzrd/adios) | Adaptive latency learning, tiered scheduling | Deadline learning, frame batching |
| [Linux EEVDF Docs](https://docs.kernel.org/scheduler/sched-eevdf.html) | Virtual deadline, lag management | Deadline calculation, fairness |
| [CFS Design](https://docs.kernel.org/scheduler/sched-design-CFS.html) | vruntime, weight system | Nice value handling |
| [SCHED_DEADLINE](https://docs.kernel.org/scheduler/sched-deadline.html) | EDF + CBS real-time scheduling | Bandwidth concepts |
| [scx_lavd](https://github.com/sched-ext/scx) | Behavioral latency criticality | Fallback classification |
| Liu & Layland 1973 | Rate Monotonic Scheduling theory | Period-based priority |
| Stoica et al. 1996 | Original EEVDF algorithm paper | Theoretical foundation |
| [LMAX Disruptor](https://lmax-exchange.github.io/disruptor/) | Lock-free, cache-efficient patterns | Ring buffers, per-CPU data |
| [Mechanical Sympathy](https://mechanical-sympathy.blogspot.com/) | Hardware-aware programming | Cache optimization |
| [Awesome Low Latency](https://github.com/penberg/awesome-low-latency) | Low-latency techniques catalog | Best practices reference |

---

## 14. Glossary

| Term | Definition |
|------|------------|
| **A.B.C.** | Always Be Casting - proactive CPU preparation |
| **boost_shift** | Priority multiplier (0-7, where 2^N is the multiplier) |
| **CBS** | Constant Bandwidth Server - bandwidth reservation for SCHED_DEADLINE |
| **CFS** | Completely Fair Scheduler - Linux scheduler before EEVDF |
| **DSQ** | Dispatch Queue - BPF queue for runnable tasks |
| **EDF** | Earliest Deadline First - scheduling algorithm |
| **EEVDF** | Earliest Eligible Virtual Deadline First - current Linux scheduler |
| **esync/fsync** | Wine synchronization primitives (eventfd/futex based) |
| **fentry** | BPF hook that fires on function entry |
| **GLW** | Global Latency Window - ADIOS concept for adaptive learning |
| **lat_cri** | Latency criticality - scx_lavd metric for task urgency |
| **ntsync** | NT synchronization - Wine kernel driver for Windows sync |
| **sched_ext** | Linux scheduler extension framework |
| **SMT** | Simultaneous Multi-Threading (hyperthreading) |
| **task_ctx** | Per-task scheduler context |
| **tgid** | Thread Group ID (process ID) |
| **vruntime** | Virtual runtime (EEVDF fairness metric) |

---

**Document Version:** 2.4.0  
**Created:** 2025-12-04  
**Updated:** 2025-12-05  
**Author:** AI-assisted (Cursor Claude)

**Changelog:**
- v2.5.0: **Starvation threshold reduced to 20ms + rescued task logging**
  - Lowered `STARVATION_THRESH_NS` from 100ms to 20ms for faster intervention on outliers
  - Added ring buffer consumer in userspace to display rescued task names in debug view
  - Fixed debug event `wait_ns` calculation to use `last_woke_at` for starvation events
  - Debug output now shows: `🚨 RESCUED TASKS (last N)` with task name, PID, and wait time
- v2.4.0: **CRITICAL: Absolute time deadlines + wait time fix**
  - Switched from vtime-based to absolute time-based deadlines. The vtime approach capped ALL tasks to the same base vtime, so priority was only determined by boost level.
  - Fixed wait time measurement for preempted tasks. Previously, `last_woke_at` was only set in `runnable()` (on wakeup from sleep). Preempted tasks (which stay runnable) would accumulate "wait time" from their original wakeup, causing false 2-3 second readings. Now `stopping()` resets `last_woke_at` when `runnable=true`.
- v2.3.0: **Starvation fix** - Added deadline increment cap (5ms) to prevent background task starvation (was 2.4s max wait, now <30ms). **Note: Insufficient - see v2.4.0**
- v2.2.0: **Detection & locality fixes**:
  - Removed `foreground_tgid` check from sync detection (100% hook-based now)
  - Added prev_cpu preference in select_cpu (Same CPU went from 0.002% to 87%)
  - Added per-hook stat counters (nr_hid_irq_in, nr_dma_fence_signal, nr_ntsync, etc.)
  - Updated debug output format with per-hook breakdown
  - Extended wait time histogram to 11 buckets (added 1-3s, 3-5s, 5-10s, >10s)
- v2.1.0: **Dispatch fixes**:
  - All enqueue() tasks go to SHARED_DSQ to prevent CPU-pinned task starvation
  - Fixed idle CPU double-claiming bug (was causing 0% direct dispatch)
  - Added boost decay (5ms) to prevent sticky priorities
- v2.0.0: Complete rewrite for clean-slate refactor. Added concrete v2 architecture, code organization rules, implementation phases. Preserved all research from v1.3 including scheduler history, ADIOS, scx_lavd/bpfland/cosmos analysis, and low-latency programming principles.
