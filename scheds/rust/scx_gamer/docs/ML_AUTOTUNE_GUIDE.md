# ML Autotune Guide - Automated Parameter Learning

## Overview

**ML Autotune** mode eliminates the need for manual parameter tweaking by automatically exploring different scheduler configurations during gameplay. The scheduler tests multiple parameter combinations and identifies the optimal configuration based on frame timing metrics.

**Key Features:**
- Zero-downtime tuning: Parameters are updated via BPF rodata hot-reload without scheduler restart
- Automated exploration: Tests 10-12 different configurations over 15 minutes
- Performance focused: Optimizes for scheduler latency, cache efficiency, and migration control

---

## Quick Start

### Basic Usage

```bash
# 1. Launch your game
./game  # or via Steam

# 2. Start scheduler in autotune mode
sudo ./target/release/scx_gamer --stats 1 --ml-autotune

# 3. Play the game normally for 15 minutes
#    The scheduler will automatically:
#    - Detect your CPU (e.g., AMD Ryzen 9 9800X3D)
#    - Try different parameter combinations (every 2 minutes)
#    - Measure scheduler performance metrics
#    - Save training data to ./ml_data/9800X3D/{game}.json
#    - Find the best config and apply it

# 4. At the end, you'll see a report like:

ML AUTOTUNE SESSION COMPLETE

Total trials: 12
Session duration: 900.0s

Top 3 Configurations:
-----------------------------------------------------------
1. Score: 87.42  Latency: 850ns  Hit Rate: 68%  Direct Dispatch: 75%
   --slice-us 10 --input-window-us 2000 --mig-max 3
   --mm-affinity

2. Score: 84.15  Latency: 780ns  Hit Rate: 65%  Direct Dispatch: 72%
   --slice-us 7 --input-window-us 1500 --mig-max 2

3. Score: 81.33  Latency: 920ns  Hit Rate: 62%  Direct Dispatch: 70%
   --slice-us 15 --input-window-us 2500 --mig-max 4

RECOMMENDED CONFIGURATION

Performance Score: 87.42

Command:
sudo ./target/release/scx_gamer --stats 1 \
  --slice-us 10 \
  --input-window-us 2000 \
  --mig-max 3 \
  --mm-affinity
```

---

## How It Works

### Phase 1: Grid Search (0-12 minutes)

The scheduler tries different **parameter combinations** automatically:

| Trial | slice_us | input_window_us | mig_max | Duration |
|-------|----------|----------------|---------|----------|
| 1     | 5        | 1000           | 1       | 2 min    |
| 2     | 10       | 2000           | 3       | 2 min    | ← Best
| 3     | 15       | 3000           | 5       | 2 min    |
| 4     | 20       | 2000           | 3       | 2 min    |
| 5     | 10       | 1500           | 2       | 2 min    |
| 6     | 10       | 2500           | 4       | 2 min    |

**During each trial**:
- Scheduler parameters are **hot-swapped** (no restart!)
- Scheduler metrics are collected (latency, cache hits, direct dispatch rate, etc.)
- Performance score is calculated
- Best configurations are tracked

### Phase 2: Convergence (12-15 minutes)

- The **best config** from Phase 1 is applied
- More samples are collected to confirm performance
- Final report is generated

### Phase 3: Continuous Running (Optional)

- After autotune completes, the best config **stays active**
- You can keep playing with the optimized settings
- Or copy the command and restart with optimal params

---

## Configuration Options

### Basic Flags

```bash
# Enable autotune mode (default: 15 min total, 2 min per trial)
--ml-autotune

# Customize trial duration (seconds)
--ml-autotune-trial-duration 180  # 3 minutes per config

# Customize total session duration (seconds)
--ml-autotune-max-duration 1200  # 20 minutes total
```

### Example Workflows

**Fast exploration** (10 minutes, 1 min trials):
```bash
sudo ./target/release/scx_gamer --stats 1 \
  --ml-autotune \
  --ml-autotune-trial-duration 60 \
  --ml-autotune-max-duration 600
```

**Thorough exploration** (30 minutes, 3 min trials):
```bash
sudo ./target/release/scx_gamer --stats 1 \
  --ml-autotune \
  --ml-autotune-trial-duration 180 \
  --ml-autotune-max-duration 1800
```

**Quick test** (6 minutes, 30 sec trials - for testing only!):
```bash
sudo ./target/release/scx_gamer --stats 1 \
  --ml-autotune \
  --ml-autotune-trial-duration 30 \
  --ml-autotune-max-duration 360
```

---

## What Gets Tuned

### Primary Parameters (Grid Search)

The autotuner explores these **3 key parameters**:

1. **`slice_us`** - Scheduling time slice (5, 10, 15, 20 µs)
   - Lower = more responsive but higher overhead
   - Higher = better throughput but less responsive

2. **`input_window_us`** - Input boost duration (1000, 2000, 3000 µs)
   - How long tasks get priority after user input
   - Higher = smoother input response, but may starve other tasks

3. **`mig_max`** - Max migrations per window (1, 3, 5)
   - Controls CPU migration aggressiveness
   - Lower = better cache locality, but less load balancing
   - Higher = better load distribution, but more cache thrashing

### Secondary Parameters (Fixed)

These use sensible defaults from your initial config:
- `slice_lag_us` - Runtime debt ceiling
- `mig_window_ms` - Migration limiter window
- `mm_affinity` - Cache affinity hinting
- `avoid_smt` - SMT contention avoidance
- `preferred_idle_scan` - High-capacity CPU preference

**Future**: Autotuner will explore these too!

---

## Performance Scoring

The autotuner uses this **scoring function** to rank configurations:

```
score = latency × 10.0             # Scheduler latency (select_cpu, etc.)
      + cache_efficiency × 5.0     # Hit rates (mm_hint, direct_dispatch)
      + migration_control × 2.0    # Migration blocking rate
```

### Scoring Components

```
latency = 10000.0 / select_cpu_latency_ns    # Lower latency = higher score
cache_efficiency = mm_hint_hit_rate + direct_dispatch_rate
migration_control = 1.0 - migration_block_rate
```

**Why this works**:
- Optimizes for low scheduling overhead (faster task wakeups)
- Prioritizes cache affinity (better performance)
- Balances migration limiting (prevents cache thrashing)

---

## Interpreting Results

### Score Components

**High Score (>80)**:
- Latency: <1000ns select_cpu average
- Cache hits: >60% mm_hint hit rate
- Direct dispatch: >70% direct dispatch rate
- Migration blocking: <20%

**Medium Score (60-80)**:
- Latency: 1000-1500ns select_cpu
- Cache hits: 50-60% mm_hint hit rate
- Direct dispatch: 60-70% direct dispatch rate
- Migration blocking: 20-30%

**Low Score (<60)**:
- Latency: >1500ns select_cpu
- Cache hits: <50% mm_hint hit rate
- Direct dispatch: <60% direct dispatch rate
- Migration blocking: >30%

### Example Analysis

**Trial 1** (slice=5, input_win=1000, mig=1):
- Score: 85.2
- Latency: 920ns, Cache Hit: 68%, Direct Dispatch: 75%
- **Analysis**: Excellent! Low latency, good cache affinity

**Trial 3** (slice=20, input_win=3000, mig=5):
- Score: 68.4
- Latency: 650ns, Cache Hit: 52%, Direct Dispatch: 58%
- **Analysis**: Good latency but poor cache efficiency

**Result**: Trial 1 wins because it balances low latency with good cache performance

---

## Advanced Usage

### Monitoring Progress

Watch the scheduler logs in real-time:

```bash
# In another terminal:
journalctl -f | grep "ML Autotune"

# You'll see:
# ML Autotune: Starting trial 1/12: slice=5µs, input_win=1000µs, mig_max=1
# ML Autotune: Trial 1/12 complete - Score: 85.2, FPS: 62.1, Jitter: 0.9ms
# ML Autotune: NEW BEST CONFIG (score: 85.2)
# ML Autotune: Starting trial 2/12: slice=10µs, input_win=2000µs, mig_max=3
# ...
```

### Combining with Stats

```bash
# Watch performance in real-time
sudo ./target/release/scx_gamer --stats 1 --ml-autotune

# In another terminal, monitor stats:
watch -n1 'scx_stats scx_gamer'
```

### Per-Game Tuning

The autotuner works best when run **separately for each game**:

```bash
# Tune for Game A
./gameA &
sudo ./target/release/scx_gamer --ml-autotune
# Save the recommended config for Game A

# Tune for Game B
./gameB &
sudo ./target/release/scx_gamer --ml-autotune
# Save the recommended config for Game B
```

**Why?** Different games have different workload characteristics:
- **FPS games**: Low latency > high throughput (prefer slice=5, mig=1)
- **RTS games**: High throughput > low latency (prefer slice=15, mig=5)
- **MMO games**: Balanced (prefer slice=10, mig=3)

---

## Troubleshooting

### Autotune Not Starting

**Problem**: Scheduler runs but doesn't switch configs

**Solutions**:
1. Check logs: `journalctl -f | grep "ML Autotune"`
2. Verify flag: `--ml-autotune` is set
3. Ensure game is running and detected

### All Trials Have Low Scores

**Problem**: Every config scores <50

**Solutions**:
1. **Check scheduler metrics**: Are metrics being collected?
   - Use `--stats 1` to monitor real-time metrics
2. **Game workload**: Autotune works best with games that have consistent workload
3. **System state**: Check if CPU/GPU bottlenecks are affecting performance

### Scores Are Similar Across Trials

**Problem**: All configs score 75-80, hard to pick best

**Solutions**:
1. **Good news!** Your game is not sensitive to these params
2. Use the **highest score** config anyway (marginal gains)
3. Consider expanding the parameter range:
   - Edit `ml_autotune.rs:generate_grid_trials()`
   - Add more extreme values (e.g., slice=3 or slice=30)

### Config Switches Feel Jarring

**Problem**: Noticeable stutter when switching configs

**Solutions**:
1. **Expected**: Parameter changes may cause brief adaptation period
2. **Increase trial duration**: `--ml-autotune-trial-duration 240` (4 min)
3. **Play in a stable scenario**: Avoid loading screens or cutscenes

---

## Technical Details

### Hot Parameter Swapping

The autotuner uses **BPF rodata hot-reload** to change parameters without restarting:

```rust
// In ml_autotune.rs:apply_config_hot()
let rodata = skel.maps.rodata_data.as_mut()?;

rodata.slice_ns = config.slice_us * 1000;           // Update time slice
rodata.input_window_ns = config.input_window_us * 1000;  // Update input window
rodata.mig_max_per_window = config.mig_max;         // Update migration limit

// BPF programs see new values immediately (next scheduling decision)
```

**Why this works**:
- BPF `rodata` maps can be updated from userspace
- Changes take effect within **microseconds** (next BPF hook invocation)
- No scheduler restart, no lost state, no disruption

### Grid Generation Algorithm

The grid is generated to **sample the parameter space** efficiently:

```rust
// In ml_autotune.rs:generate_grid_trials()
let slice_values = [5, 10, 15, 20];           // 4 values
let input_window_values = [1000, 2000, 3000]; // 3 values
let mig_max_values = [1, 3, 5];               // 3 values

// Total combinations: 4 × 3 × 3 = 36 trials
// But we limit to ~12 trials (sample evenly spaced)
```

**Sampling strategy**:
- Try corners of parameter space (e.g., slice=5+mig=1, slice=20+mig=5)
- Sample diagonals (e.g., slice=10+mig=3)
- Skip redundant middle values to save time

### Scoring Formula Rationale

**Frame quality is 3x more important than latency**:
- Players perceive frame drops/stutter directly
- Scheduler latency only matters if it causes frame issues
- Example: 850ns latency with smooth 62 FPS > 600ns latency with stuttery 58 FPS

**Frame pacing bonus**:
- Consistent frame times = smooth experience
- p99 < 1.5x p50 = good pacing (20 point bonus)
- Rewards configs that avoid frame drops

---

## Future Enhancements

### Planned Features

1. **Boolean flag tuning**: Explore `mm_affinity`, `avoid_smt`, etc.
2. **Adaptive grid**: Focus on promising regions after initial sweep
3. **Multi-phase tuning**: Coarse search → fine search → convergence
4. **Per-game profiles**: Save best configs per game, auto-load on launch
5. **Continuous adaptation**: Keep tuning in background during long sessions

### Bayesian Optimization (Future)

Instead of exhaustive grid search, use **Bayesian optimization** to converge faster:

```
Trial 1: Try random config A - Score 75
Trial 2: Try random config B - Score 82  (Best so far)
Trial 3: Try config near B (exploit) - Score 85  (New best)
Trial 4: Try distant config (explore) - Score 70
Trial 5: Try config between B and Trial 3 - Score 87  (Best)
...
Converged after 8 trials instead of 12
```

**Benefits**:
- Finds optimal config in fewer trials
- Smarter exploration vs exploitation tradeoff
- Adapts to each game's unique parameter sensitivity

---

## Comparison: Manual vs Autotune

| Aspect | Manual Tuning | Autotune Mode |
|--------|---------------|---------------|
| **Time investment** | 30-60 min (restart each time) | 15 min (hands-off) |
| **Number of configs tested** | 3-5 (user patience limited) | 10-12 (automated) |
| **Downtime** | Restart scheduler each test | Zero downtime (hot-swap) |
| **Objectivity** | Subjective "feel" | Quantitative scores |
| **Reproducibility** | Hard to replicate | Consistent methodology |
| **Data collection** | Manual CSV export | Automatic logging |

---

## Best Practices

### 1. Stable Gameplay Scenario

**Recommended**:
- Play normal gameplay (combat, exploration, etc.)
- Avoid loading screens, cutscenes, menus during trials
- Keep graphics settings consistent

**Avoid**:
- Switching games mid-session
- Alt-tabbing frequently
- Changing graphics settings during tuning

### 2. Representative Workload

**Recommended**:
- Play the most performance-critical part of the game
- Example: Busy multiplayer match, not tutorial

**Avoid**:
- Tuning during idle menu screens (unrealistic)
- Tuning during loading screens (no meaningful data)

### 3. Sufficient Trial Duration

**Recommended**:
- Use 2-3 minute trials minimum (default: 2 min)
- Longer for games with variable workload

**Avoid**:
- Using <1 minute trials (insufficient samples)
- Using >5 minute trials (diminishing returns)

---

## Example Session

```bash
# 1. Launch game
$ ./Warframe.x64 &

# 2. Start autotune (15 min, 2 min trials)
$ sudo ./target/release/scx_gamer --stats 1 --ml-autotune

# Log output:
# ML Autotune: Enabled (trial: 120s, max: 900s)
# ML Autotune: Grid search with 12 trials (120.0s each, 1440.0s total)
# ML Autotune: Starting trial 1/12: slice=5µs, input_win=1000µs, mig_max=1
# ... play for 2 minutes ...
# ML Autotune: Trial 1/12 complete - Score: 82.5, Latency: 950ns, Hit Rate: 62%
# ML Autotune: NEW BEST CONFIG (score: 82.5)
# ML Autotune: Starting trial 2/12: slice=10µs, input_win=2000µs, mig_max=3
# ... play for 2 minutes ...
# ML Autotune: Trial 2/12 complete - Score: 87.2, Latency: 870ns, Hit Rate: 68%
# ML Autotune: NEW BEST CONFIG (score: 87.2)
# ... continue for 10 more trials ...

# Final report:

ML AUTOTUNE SESSION COMPLETE

Total trials: 12
Session duration: 1440.0s

Top 3 Configurations:
-----------------------------------------------------------
1. Score: 87.2  Latency: 870ns  Hit Rate: 68%  Direct Dispatch: 75%
   --slice-us 10 --input-window-us 2000 --mig-max 3
   --mm-affinity

2. Score: 85.4  Latency: 820ns  Hit Rate: 65%  Direct Dispatch: 72%
   --slice-us 7 --input-window-us 1500 --mig-max 2

3. Score: 82.5  Latency: 950ns  Hit Rate: 62%  Direct Dispatch: 70%
   --slice-us 5 --input-window-us 1000 --mig-max 1

RECOMMENDED CONFIGURATION

Performance Score: 87.2

Command:
sudo ./target/release/scx_gamer --stats 1 \
  --slice-us 10 \
  --input-window-us 2000 \
  --mig-max 3 \
  --mm-affinity

# 3. Use the optimal config (or keep playing with current best config)
```

---

## License

GPL-2.0-only

## Author

RitzDaCat - 2025
