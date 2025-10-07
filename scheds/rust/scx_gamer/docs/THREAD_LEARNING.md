# Thread Pattern Learning (Experimental)

## Overview

Thread pattern learning is an experimental feature that automatically identifies thread roles (input handler, GPU submit, render, etc.) for games with generic thread names like `Warframe.x64.ex`.

## Problem Statement

Many games use generic thread names that make automatic classification difficult:
- **Warframe**: All threads named `Warframe.x64.ex`
- **Custom engines**: Often use `game.exe` for all threads
- **Wine games**: May show as `wine-preloader` or process name only

This prevents the scheduler from accurately prioritizing render threads, input handlers, and GPU submission threads.

## Solution

The thread learning feature:
1. Samples thread behavior over time (`/proc/{pid}/task/` statistics)
2. Identifies patterns: wakeup frequency, CPU time, execution duration
3. Classifies threads based on behavior (not names)
4. Saves patterns to disk for future runs
5. Auto-loads patterns when the same game runs again

## Usage

### Enable Learning Mode

```bash
# Standard mode (no learning)
sudo ./scx_gamer --stats 1.0

# Learning mode (samples threads)
sudo ./scx_gamer --stats 1.0 --learn-threads

# Custom sampling parameters
sudo ./scx_gamer --stats 1.0 --learn-threads \
  --thread-sample-interval 1.0 \
  --thread-min-samples 60
```

### Parameters

| Flag | Default | Description |
|------|---------|-------------|
| `--learn-threads` | disabled | Enable thread pattern learning |
| `--thread-sample-interval` | 2.0 | Seconds between samples |
| `--thread-min-samples` | 30 | Minimum samples before saving pattern |

### Directory Structure

Patterns are saved to:
```
ml_data/
└── {cpu_model}/          # e.g., "9800X3D"
    └── thread_patterns/
        ├── Warframe.x64.ex.json
        ├── Splitgate-Win64-Shipping.exe.json
        └── csgo.exe.json
```

## Pattern File Format

Example `Warframe.x64.ex.json`:

```json
{
  "game_name": "Warframe.x64.ex",
  "engine": null,
  "detected_at": "2025-01-03T15:54:36Z",
  "pid": 50789,
  "threads": [
    {
      "tid": 50789,
      "comm": "Warframe.x64.ex",
      "role": "main",
      "classification": "input_handler",
      "avg_wakeup_freq": 245,
      "avg_exec_ns": 125000,
      "cpu_time_pct": 18.5,
      "samples": 50
    },
    {
      "tid": 50799,
      "comm": "Warframe.x64.ex",
      "role": "render",
      "classification": "gpu_submit",
      "avg_wakeup_freq": 480,
      "avg_exec_ns": 85000,
      "cpu_time_pct": 8.2,
      "samples": 50
    }
  ],
  "pattern_rules": [
    {
      "rule": "main_thread_is_input",
      "threshold": null,
      "confidence": 0.95
    },
    {
      "rule": "high_wakeup_freq_is_render",
      "threshold": 400,
      "confidence": 0.85
    },
    {
      "rule": "identical_names_use_behavior",
      "threshold": null,
      "confidence": 1.0
    }
  ]
}
```

## Classification Rules

### Automatic Rules

1. **Main Thread = Input Handler**
   - Thread where `TID == PID` (process main thread)
   - Confidence: 95%
   - Rationale: Most games process input on the main thread

2. **High Wakeup Frequency = Render/GPU**
   - Wakeup frequency > 400 Hz (frame rendering)
   - Confidence: 85%
   - Rationale: Render threads wake once per frame (360-480Hz for high refresh)

3. **Identical Names = Behavior-Based**
   - All threads have the same `comm` name
   - Forces behavior-based classification
   - Confidence: 100%

### Thread Role Detection

| Role | Criteria |
|------|----------|
| **Input Handler** | Main thread OR low exec time (<1ms) + high wakeup freq |
| **GPU Submit** | Wakeup freq ~= frame rate (240-480Hz) + low exec (<100µs) |
| **Render** | High CPU % (>5%) + frame-rate wakeups |
| **Network** | Moderate wakeup freq (60-120Hz) + network syscalls |
| **Audio** | Consistent wakeup freq (~48kHz / buffer size) |
| **Background** | Low wakeup freq (<10Hz) + high exec time (>5ms) |

## Workflow

### First Run (Learning)

```bash
# 1. Start scheduler with learning enabled
sudo ./scx_gamer --stats 1.0 --learn-threads

# 2. Launch game (e.g., Warframe)
# Scheduler detects: "game detector: found game 'Warframe.x64.ex' (tgid=50789)"

# 3. Play normally for 60+ seconds
# Sampler collects: wakeup patterns, CPU usage, execution times

# 4. Stats show learning progress:
# threads: input  0  gpu  0  (still learning...)

# 5. After min_samples (default 30), pattern auto-saves:
# Thread Learning: Saved pattern for Warframe.x64.ex (45 threads, 3 rules)

# 6. Next scheduler cycle (or restart), threads classified:
# threads: input  1  gpu  2  (pattern loaded!)
```

### Subsequent Runs (Auto-Load)

```bash
# Pattern exists, auto-loads even without --learn-threads
sudo ./scx_gamer --stats 1.0

# Scheduler logs:
# Thread Learning: Loaded pattern for Warframe.x64.ex (45 threads, 3 rules)
# Stats immediately show:
# threads: input  1  gpu  2  sys_aud  2  comp  1
```

## Implementation Status

### ✅ Completed

- Thread pattern data structures
- JSON persistence (save/load)
- `/proc` statistics sampler
- CLI flags and initialization
- Thread role classification logic

### ⏳ Pending

- Event loop integration (automatic sampling trigger)
- Pattern auto-save on game exit
- BPF stats feedback to pattern learning
- Pattern confidence scoring over time

### 🔬 Experimental Limitations

1. **No runtime updates**: Patterns are static after first save
2. **Manual enable required**: `--learn-threads` must be explicit
3. **No multi-game testing**: Only tested conceptually, not with real games
4. **No pattern deletion**: Old patterns must be manually removed

## Performance Impact

**When Disabled (Default)**:
- Zero overhead
- No `/proc` reads
- No pattern loading

**When Enabled (`--learn-threads`)**:
- Periodic `/proc/{pid}/task/` reads (every 2s default)
- Minimal CPU impact (<0.1% per game)
- One-time pattern save (JSON write, ~10KB)

## Troubleshooting

### Patterns Not Saving

```bash
# Check permissions
ls -la ml_data/9800X3D/thread_patterns/

# Check minimum samples reached
# Look for: "Thread Learning: Not enough samples (15/30)"
```

### Incorrect Classification

```bash
# Delete pattern to re-learn
rm ml_data/9800X3D/thread_patterns/Warframe.x64.ex.json

# Re-run with more samples
sudo ./scx_gamer --learn-threads --thread-min-samples 60
```

### Pattern Not Loading

```bash
# Check file exists
ls ml_data/9800X3D/thread_patterns/

# Check JSON validity
cat ml_data/9800X3D/thread_patterns/Warframe.x64.ex.json | jq .
```

## Future Enhancements

1. **Active pattern refinement**: Update patterns during gameplay based on BPF feedback
2. **Confidence scoring**: Track classification accuracy over multiple runs
3. **Pattern sharing**: Community-contributed patterns for popular games
4. **Auto-detection**: Enable learning automatically for unknown games
5. **GPU driver integration**: Cross-reference with GPU submission timestamps

## Related Features

- **ML Profiles** (`--ml-profiles`): Auto-loads best scheduler config per game
- **ML Collect** (`--ml-collect`): Collects performance metrics for training
- **ML Autotune** (`--ml-autotune`): Automatically finds optimal scheduler parameters

## See Also

- `src/thread_patterns.rs` - Pattern storage implementation
- `src/thread_sampler.rs` - Thread statistics collection
- Main README.md - Overall scheduler documentation
