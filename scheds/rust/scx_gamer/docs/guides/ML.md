# ML Autotune Guide

## Overview

ML Autotune mode eliminates the need for manual parameter tweaking by automatically exploring different scheduler configurations during gameplay. The scheduler tests multiple parameter combinations and identifies the optimal configuration based on frame timing metrics.

Key Features:
- Automated parameter exploration: Tests 10-12 different configurations
- Performance focused: Optimizes for scheduler latency, cache efficiency, and migration control
- Zero-downtime tuning: Parameters updated via BPF rodata hot-reload

## Quick Start

### Basic Usage

```bash
# 1. Launch your game
./game  # or via Steam

# 2. Run autotune
sudo ./start.sh --ml-autotune --ml-autotune-trial-duration 120

# 3. Play for 15 minutes while autotune runs
# 4. Check results in ~/.local/share/scx_gamer/ml_data/
```

### Advanced Usage

```bash
# Custom trial duration and max session time
sudo ./start.sh \
  --ml-autotune \
  --ml-autotune-trial-duration 180 \
  --ml-autotune-max-duration 1200

# Use Bayesian optimization (faster convergence)
sudo ./start.sh \
  --ml-autotune \
  --ml-bayesian \
  --ml-autotune-trial-duration 120
```

## Implementation Details

### Important: Scheduler Restart Required Per Trial

Discovery: BPF `rodata` parameters are immutable after load. This means autotune cannot hot-swap parameters in real-time within a single scheduler instance.

### Current Implementation

Autotune workflow:
1. Run scheduler with baseline config
2. Collect performance samples for trial duration (e.g., 2 minutes)
3. Exit scheduler cleanly
4. Restart with next config from trial list
5. Repeat until all trials complete

### Workflow Script

To run autotune, use a shell script to manage restarts:

```bash
#!/bin/bash
# autotune.sh

GAME_PID=""
TRIAL_DURATION=120
MAX_DURATION=900

# Function to start scheduler
start_scheduler() {
    local config="$1"
    echo "Starting scheduler with config: $config"
    sudo ./start.sh $config --ml-autotune &
    SCHEDULER_PID=$!
}

# Function to stop scheduler
stop_scheduler() {
    if [ ! -z "$SCHEDULER_PID" ]; then
        echo "Stopping scheduler (PID: $SCHEDULER_PID)"
        sudo kill -TERM $SCHEDULER_PID
        wait $SCHEDULER_PID
    fi
}

# Main autotune loop
for config in "${CONFIG_LIST[@]}"; do
    start_scheduler "$config"
    sleep $TRIAL_DURATION
    stop_scheduler
    sleep 5  # Brief pause between trials
done
```

## Parameter Exploration

### Grid Search (Default)

Explored Parameters:
- `input_window_us`: 1ms, 2ms, 4ms
- `boost_duration_us`: 100μs, 200μs, 400μs
- `migration_cost`: 1000, 2000, 4000
- `cache_weight`: 0.5, 1.0, 2.0

Total Configurations: 81 combinations
Estimated Duration: ~3 hours (2 minutes per trial)

### Bayesian Optimization

Advantages:
- Faster convergence to optimal parameters
- Intelligent parameter space exploration
- Reduced total exploration time

Estimated Duration: ~1.5 hours (2 minutes per trial)

## Performance Metrics

### Collected Metrics

Scheduler Performance:
- `select_cpu()` latency (nanoseconds)
- Cache hit ratio (%)
- Migration frequency (per second)
- CPU utilization (%)

Gaming Performance:
- Frame time variance (milliseconds)
- Input latency (nanoseconds)
- GPU utilization (%)
- Memory bandwidth (GB/s)

### Optimization Targets

Primary Objectives:
1. Minimize input latency: Target <100ns per event
2. Reduce frame time variance: Target <1ms variance
3. Optimize cache efficiency: Target >90% hit ratio
4. Control migrations: Target <10 migrations/second

## Results Analysis

### Output Files

Location: `~/.local/share/scx_gamer/ml_data/`

Files:
- `autotune_results.json`: Complete trial results
- `best_config.json`: Optimal configuration
- `performance_samples.csv`: Raw performance data
- `optimization_log.txt`: Detailed optimization log

### Best Configuration Format

```json
{
  "config": {
    "input_window_us": 2000,
    "boost_duration_us": 200,
    "migration_cost": 2000,
    "cache_weight": 1.0
  },
  "performance": {
    "input_latency_ns": 85,
    "frame_variance_ms": 0.8,
    "cache_hit_ratio": 0.92,
    "migrations_per_sec": 8
  },
  "score": 0.94
}
```

## Troubleshooting

### Common Issues

1. Scheduler Restart Failures
```bash
# Check if previous instance is still running
ps aux | grep scx_gamer

# Kill any remaining processes
sudo pkill -f scx_gamer
```

2. Insufficient Performance Data
```bash
# Increase trial duration
--ml-autotune-trial-duration 180

# Check game is actually running
ps aux | grep [game_name]
```

3. BPF Parameter Errors
```bash
# Verify BPF skeleton is valid
sudo ./start.sh --dry-run

# Check kernel logs
dmesg | grep -i bpf
```

### Debug Mode

```bash
# Enable verbose logging
sudo ./start.sh --ml-autotune --verbose

# Check ML data directory
ls -la ~/.local/share/scx_gamer/ml_data/

# View optimization log
tail -f ~/.local/share/scx_gamer/ml_data/optimization_log.txt
```

## Best Practices

### Before Starting Autotune

1. Close unnecessary applications: Reduce system noise
2. Stabilize system: Let system warm up for 5 minutes
3. Prepare game: Have game ready to launch quickly
4. Monitor resources: Ensure sufficient CPU/memory

### During Autotune

1. Play consistently: Maintain similar gameplay patterns
2. Avoid system changes: Don't install/update software
3. Monitor performance: Watch for system instability
4. Take breaks: Autotune can run for hours

### After Autotune

1. Review results: Check optimization log for issues
2. Validate configuration: Test best config manually
3. Backup results: Save optimal configuration
4. Document findings: Note any game-specific optimizations

## Advanced Configuration

### Custom Parameter Ranges

```rust
// In ml_autotune.rs
const PARAMETER_RANGES: &[(&str, Vec<f64>)] = &[
    ("input_window_us", vec![1000.0, 2000.0, 4000.0]),
    ("boost_duration_us", vec![100.0, 200.0, 400.0]),
    ("migration_cost", vec![1000.0, 2000.0, 4000.0]),
    ("cache_weight", vec![0.5, 1.0, 2.0]),
];
```

### Custom Optimization Objectives

```rust
// In ml_scoring.rs
fn calculate_score(metrics: &PerformanceMetrics) -> f64 {
    let latency_score = 1.0 - (metrics.input_latency_ns as f64 / 1000.0);
    let variance_score = 1.0 - (metrics.frame_variance_ms / 10.0);
    let cache_score = metrics.cache_hit_ratio;
    let migration_score = 1.0 - (metrics.migrations_per_sec / 100.0);
    
    (latency_score * 0.4 + variance_score * 0.3 + 
     cache_score * 0.2 + migration_score * 0.1)
}
```

## Conclusion

ML Autotune provides automated parameter optimization for scx_gamer, eliminating the need for manual tuning while ensuring optimal performance for each specific game and hardware configuration.

Key Benefits:
- Automated optimization: No manual parameter tweaking required
- Game-specific tuning: Optimal configuration per game
- Performance validation: Comprehensive performance testing
- Reproducible results: Consistent optimization process
