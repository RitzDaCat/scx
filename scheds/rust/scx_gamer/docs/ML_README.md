# scx_gamer Machine Learning Pipeline

## Overview

scx_gamer includes a comprehensive ML data collection and training pipeline to automatically discover optimal scheduler configurations for different games. The system collects performance metrics during gameplay, exports them as training data, and uses machine learning to predict the best configuration parameters.

## Architecture

```
┌──────────────┐    ┌─────────────┐    ┌──────────────┐    ┌──────────────┐
│   scx_gamer  │───>│  ML Collector│───>│  JSON Files  │───>│ CSV Export   │
│  (BPF sched) │    │  (Rust)      │    │ (~/.scx_gamer)│    │ (training.csv)│
└──────────────┘    └─────────────┘    └──────────────┘    └──────────────┘
       │                                                              │
       │ Profiling                                                    │
       │ Metrics                                                      ▼
       v                                                     ┌──────────────┐
┌──────────────┐                                            │  ML Training │
│ BPF Hot-Path │                                            │  (Python)    │
│  Profiling   │                                            └──────────────┘
└──────────────┘                                                     │
                                                                     ▼
                                                            ┌──────────────┐
                                                            │ Trained Model│
                                                            │  (.pkl)      │
                                                            └──────────────┘
                                                                     │
                                                                     ▼
                                                            ┌──────────────┐
                                                            │ Optimal Config│
                                                            │   (JSON)     │
                                                            └──────────────┘
```

## Quick Start

### 1. Collect Training Data

Run scx_gamer with ML collection enabled while playing games:

```bash
# Start scheduler with ML collection
sudo ./target/release/scx_gamer \
  --stats 1 \
  --ml-collect \
  --ml-sample-interval 5.0

# Play different games with various settings
# Try different configurations to explore the parameter space:

# Configuration A: Low latency
sudo ./target/release/scx_gamer --ml-collect --slice-us 5 --input-window-us 1000 --mig-max 1

# Configuration B: Balanced
sudo ./target/release/scx_gamer --ml-collect --slice-us 10 --input-window-us 2000 --mig-max 3

# Configuration C: High throughput
sudo ./target/release/scx_gamer --ml-collect --slice-us 20 --input-window-us 3000 --mig-max 5
```

**Data is automatically saved to:** `./ml_data/{cpu_model}/{game_name}.json` (CPU-specific, git-shareable)

### 2. Export Training Dataset

After collecting data from multiple gaming sessions:

```bash
sudo ./target/release/scx_gamer --ml-export-csv training_data.csv
```

This exports ALL collected data from all games into a single CSV file ready for ML training.

### 3. Train ML Model

```bash
# Install dependencies
pip install scikit-learn pandas numpy joblib

# Train global model (all games)
python3 ml_train.py training_data.csv

# Train game-specific model
python3 ml_train.py training_data.csv --game "game.exe"

# Analyze feature importance
python3 ml_train.py training_data.csv --analyze
```

### 4. Use Optimal Configuration

The training script outputs the best configuration found:

```bash
# Example output:
=== Best Configuration Found ===
Performance Score: 87.42

Command line:
sudo ./target/release/scx_gamer --stats 1 --slice-us 7 --input-window-us 1500 --mig-max 2 --mm-affinity --avoid-smt
```

## Data Collection Strategy

### Sampling Parameters

- **Sample interval:** 5 seconds (configurable with `--ml-sample-interval`)
- **Auto-save:** Every 100 samples (prevents data loss)
- **Storage:** JSON per-game + aggregated CSV export

### Collected Metrics

#### Configuration Parameters (Inputs)
- `slice_us` - Scheduling time slice
- `slice_lag_us` - Max runtime debt
- `input_window_us` - Input boost window
- `mig_window_ms` - Migration limiter window
- `mig_max` - Max migrations per window
- `mm_affinity` - Address space affinity
- `avoid_smt` - SMT contention avoidance
- `preferred_idle_scan` - Prefer high-capacity CPUs
- `enable_numa` - NUMA awareness
- `wakeup_timer_us` - Wakeup timer period

#### Performance Metrics (Outputs)
- **Latency:** select_cpu, enqueue, dispatch, deadline (nanoseconds)
- **Throughput:** enqueues/sec, dispatches/sec
- **Quality:** migration_block_rate, mm_hint_hit_rate, direct_dispatch_rate
- **System:** cpu_util_pct
- **Thread classification:** input_handler_count, gpu_submit_count, etc.

## ML Model Details

### Algorithm: Random Forest Regression

**Why Random Forest?**
- Handles non-linear relationships between config and performance
- Resistant to overfitting with small datasets
- Provides feature importance rankings
- No assumptions about data distribution

### Training Process

1. **Feature Engineering:**
   - Game characteristics (thread counts, CPU usage)
   - Runtime behavior (latency measurements)

2. **Target Prediction:**
   - Predicts optimal `slice_us`, `input_window_us`, `mig_max`
   - Other parameters use heuristics (mm_affinity, avoid_smt based on topology)

3. **Scoring Function:**
   ```
   score = (10000 / select_cpu_latency) * 10.0
         + (mm_hint_hit_rate + direct_dispatch_rate) * 5.0
         + (1 - migration_block_rate) * 2.0
   ```

### Model Validation

- **Cross-validation:** 5-fold CV to prevent overfitting
- **Train/test split:** 80/20
- **Feature scaling:** StandardScaler for numerical stability

## Advanced Usage

### Explore Parameter Space

To build a comprehensive dataset, systematically vary parameters:

```bash
# Generate grid of configurations
for slice in 5 10 15 20; do
  for input_win in 1000 2000 3000; do
    for mig_max in 1 3 5; do
      sudo ./target/release/scx_gamer \
        --ml-collect \
        --slice-us $slice \
        --input-window-us $input_win \
        --mig-max $mig_max &

      # Play game for 5 minutes
      sleep 300
      sudo pkill scx_gamer
      sleep 5
    done
  done
done
```

### View Collected Data

```bash
# Show summary for a game
sudo ./target/release/scx_gamer --ml-show-best "game.exe"

# Inspect raw JSON data
cat ~/.scx_gamer/ml_data/game_exe.json | jq '.best_config'

# List all games with data
ls ~/.scx_gamer/ml_data/
```

### Batch Analysis

```python
import pandas as pd

# Load exported CSV
df = pd.read_csv('training_data.csv')

# Group by game
for game in df['game_name'].unique():
    game_df = df[df['game_name'] == game]
    print(f"{game}: {len(game_df)} samples")
    print(f"  Avg select_cpu latency: {game_df['latency_select_cpu_ns'].mean():.0f}ns")
    print(f"  Best config: slice={game_df.loc[game_df['latency_select_cpu_ns'].idxmin(), 'slice_us']}")
```

## Performance Metrics Explained

### Latency Measurements (Lower is Better)

- **select_cpu_ns:** Time to select which CPU should run a task
  - Target: <1000ns average, <2000ns p99
  - Critical path: called on every wakeup (~100k times/sec during gameplay)

- **enqueue_ns:** Time to add task to run queue
  - Target: <500ns average
  - Includes deadline calculation and queue insertion

- **dispatch_ns:** Time to move task from queue to CPU
  - Target: <300ns average
  - Fast path: usually just a queue pop

- **deadline_ns:** Time to calculate task deadline/priority
  - Target: <100ns average (with our boost_shift optimization)
  - Critical for EDF scheduling accuracy

### Quality Metrics (Higher is Better)

- **mm_hint_hit_rate:** % of times mm_last_cpu hint resulted in idle CPU
  - Target: >60%
  - Indicates cache affinity preservation quality

- **direct_dispatch_rate:** % of enqueues that went directly to local CPU
  - Target: >70%
  - Higher = better cache locality

- **migration_block_rate:** % of migrations blocked by rate limiter
  - Target: <20%
  - Too high = limiting useful migrations
  - Too low = allowing cache thrashing

## Interpreting Results

### Good Configuration Indicators

- ✅ select_cpu latency <1000ns
- ✅ mm_hint hit rate >60%
- ✅ direct dispatch rate >70%
- ✅ Low migration blocking (<20%)
- ✅ Detected all critical threads (input, GPU, compositor)

### Bad Configuration Indicators

- ❌ select_cpu latency >2000ns (too much overhead)
- ❌ mm_hint hit rate <40% (poor cache affinity)
- ❌ direct dispatch rate <50% (excessive migrations)
- ❌ High migration blocking (>40%) - limiter too aggressive
- ❌ Missing thread classifications (check thread naming patterns)

## Troubleshooting

### No data collected

- Ensure `--ml-collect` flag is set
- Check `~/.scx_gamer/ml_data/` directory exists and is writable
- Run with `--verbose` to see ML log messages

### Model training fails

- Need at least 20+ samples per game for meaningful training
- Ensure CSV export includes all required columns
- Check Python dependencies are installed

### Poor predictions

- Collect more diverse configurations (vary parameters widely)
- Train game-specific models instead of global model
- Check for outliers in data (extreme latency spikes during loading screens)

## Future Enhancements

- [ ] Real-time adaptive tuning (adjust params during gameplay)
- [ ] Deep learning models (LSTM for temporal patterns)
- [ ] Transfer learning (pre-trained model on similar games)
- [ ] Automatic A/B testing framework
- [ ] Integration with game launchers (Steam, Lutris)
- [ ] Cluster analysis to group games by workload characteristics

## Technical Details

### BPF Profiling Implementation

The scheduler uses compile-time-optimized profiling macros:

```c
PROF_START_HIST(select_cpu);
// ... hot path code ...
PROF_END_HIST(select_cpu);
```

When `--stats` is disabled, these macros compile to **zero overhead** (eliminated by optimizer).

### Histogram Buckets

Latency distribution tracked in logarithmic buckets:
- Bucket 0: <100ns
- Bucket 1: 100-200ns
- Bucket 2: 200-400ns
- ...
- Bucket 11: >102.4μs

This allows calculating p50, p99, p99.9 percentiles for latency analysis.

### Data Storage Format

Each game's data is stored as:
```json
{
  "game_name": "game.exe",
  "samples": [
    {
      "timestamp": 1735776000,
      "config": { "slice_us": 10, ... },
      "metrics": { "latency_select_cpu_avg_ns": 850, ... },
      "game": { "tgid": 12345, "name": "game.exe", ... }
    }
  ],
  "best_config": { ... },
  "best_score": 87.42
}
```

## License

GPL-2.0-only

## Author

RitzDaCat
