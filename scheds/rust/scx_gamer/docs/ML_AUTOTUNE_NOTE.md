# ML Autotune Implementation Note

## Important: Scheduler Restart Required Per Trial

**Discovery**: BPF `rodata` parameters are **immutable after load**. This means autotune cannot hot-swap parameters in real-time within a single scheduler instance.

### Current Implementation

**Autotune workflow**:
1. Run scheduler with baseline config
2. Collect performance samples for trial duration (e.g., 2 minutes)
3. **Exit scheduler** cleanly
4. **Restart with next config** from trial list
5. Repeat until all trials complete

### Workflow Script

To run autotune, use a shell script to manage restarts:

```bash
#!/bin/bash
# autotune_warframe.sh

GAME_PID=$1
CONFIGS=(
    "--slice-us 5 --input-window-us 1000 --mig-max 1"
    "--slice-us 10 --input-window-us 2000 --mig-max 3"
    "--slice-us 15 --input-window-us 3000 --mig-max 5"
    "--slice-us 7 --input-window-us 1500 --mig-max 2"
)

for config in "${CONFIGS[@]}"; do
    echo "Testing config: $config"
    sudo ./target/release/scx_gamer --stats 1 --ml-collect $config &
    SCHED_PID=$!

    # Run for 2 minutes
    sleep 120

    # Stop scheduler cleanly
    sudo kill -INT $SCHED_PID
    wait $SCHED_PID

    # Brief pause before next trial
    sleep 2
done

echo "Autotune complete! Check ml_data/ for results"
sudo ./target/release/scx_gamer --ml-show-best "$(ps -p $GAME_PID -o comm=)"
```

### Usage

```bash
# 1. Launch game with MangohHUD
mangohud ./game &
GAME_PID=$!

# 2. Run autotune script
./autotune_warframe.sh $GAME_PID

# 3. After 12 minutes (6 configs × 2 min), see best config
sudo ./target/release/scx_gamer --ml-show-best "game.exe"
```

### Why Not Hot-Swap?

**BPF rodata is const after load**:
```c
const volatile u64 slice_ns = 10000ULL;  // ← const = immutable
```

**Alternatives considered**:

1. **BPF global variables** (not `const volatile`):
   - ✅ Mutable from userspace
   - ❌ Slightly slower (not optimized by verifier)
   - ❌ More complex memory model

2. **BPF map for config**:
   - ✅ Fully dynamic
   - ❌ Hash map lookup overhead in hot path (bad for select_cpu)
   - ❌ Complexity increase

3. **Scheduler restart** (chosen):
   - ✅ Simple, uses existing rodata
   - ✅ Clean state between trials
   - ❌ ~2 second downtime per trial
   - ✅ Acceptable for autotune (not running during gameplay)

### Future: Integrated Autotune Mode

Could add `--ml-autotune-integrated` flag that:
- Runs external script automatically
- Manages restarts internally
- No user intervention needed

But for now, the manual script approach works well.

## License

GPL-2.0-only
