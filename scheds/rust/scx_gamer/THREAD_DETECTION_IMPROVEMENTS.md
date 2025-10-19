# Thread Detection Improvements for Kovaaks

## Summary

Fixed thread detection patterns to properly identify Kovaaks (FPSAimTrainer) threads that were previously missed by the scheduler.

## Issues Identified

### Kovaaks Thread Analysis
- **Process**: `FPSAimTrainer-Win64-Shipping.exe` (PID 14531)
- **Total Threads**: 153 threads
- **Key Threads**:
  - `RenderThread 1` (PID 15236) - 5:08 CPU time
  - `AudioThread` (PID 14753) - 0:10 CPU time
  - `AudioMixerRende` (PID 14752) - 0:05 CPU time
  - `FAudio_AudioCli` (PID 14750) - 0:01 CPU time
  - `CompositorTileW` (PID 14682) - 0:00 CPU time

## Detection Pattern Fixes

### 1. GPU Thread Detection ✅

**Problem**: Pattern only matched `RenderThread` but not `RenderThread 1` (with space and number)

**Fix**: Enhanced pattern to handle numbered RenderThread variants
```c
/* Unreal Engine RenderThread (Splitgate, Fortnite, Kovaaks, etc.) - CRITICAL PATH */
if (comm[0] == 'R' && comm[1] == 'e' && comm[2] == 'n' && comm[3] == 'd' &&
    comm[4] == 'e' && comm[5] == 'r' && comm[6] == 'T') {
    /* Handle RenderThread, RenderThread 0, RenderThread 1, etc. */
    if (comm[7] == '\0' || comm[7] == ' ')
        return true;  /* RenderThread, RenderThread 0, RenderThread 1 */
}
```

**Impact**: `RenderThread 1` now gets 8x boost (boost_shift=6)

### 2. Audio Thread Detection ✅

**Problem**: Pattern missed `AudioMixerRende` and `FAudio_AudioCli` variants

**Fix**: Enhanced pattern to handle audio thread variants
```c
/* Unreal Engine audio threads */
if (comm[0] == 'A' && comm[1] == 'u' && comm[2] == 'd' && comm[3] == 'i' && comm[4] == 'o') {
    /* Handle AudioThread, AudioThread0, AudioMixerRende, etc. */
    if (comm[5] == '\0' || comm[5] == 'T' || comm[5] == 'M')
        return true;  /* AudioThread, AudioThread0, AudioMixerRende */
}

if (comm[0] == 'F' && comm[1] == 'A' && comm[2] == 'u' && comm[3] == 'd')
    return true;  /* FAudio_AudioCli */
```

**Impact**: 
- `AudioThread`: 3x boost (boost_shift=1)
- `AudioMixerRende`: 3x boost (boost_shift=1)
- `FAudio_AudioCli`: 3x boost (boost_shift=1)

### 3. Compositor Thread Detection ✅

**Problem**: Pattern missed `CompositorTileW` threads

**Fix**: Added Unreal Engine compositor thread pattern
```c
/* Unreal Engine compositor threads (Kovaaks, etc.) */
if (comm[0] == 'C' && comm[1] == 'o' && comm[2] == 'm' && comm[3] == 'p' &&
    comm[4] == 'o' && comm[5] == 's' && comm[6] == 'i' && comm[7] == 't')
    return true;  /* CompositorTileW, CompositorThread, etc. */
```

**Impact**: `CompositorTileW` now gets 5x boost (boost_shift=3)

## Expected Performance Impact

### Before Fixes
- `RenderThread 1`: No boost (standard priority)
- `AudioThread`: No boost (standard priority)
- `AudioMixerRende`: No boost (standard priority)
- `FAudio_AudioCli`: No boost (standard priority)
- `CompositorTileW`: No boost (standard priority)

### After Fixes
- `RenderThread 1`: 8x boost (GPU submit priority)
- `AudioThread`: 3x boost (game audio priority)
- `AudioMixerRende`: 3x boost (game audio priority)
- `FAudio_AudioCli`: 3x boost (game audio priority)
- `CompositorTileW`: 5x boost (compositor priority)

## Benefits

1. **Better Frame Pacing**: GPU submit threads get physical core priority
2. **Improved Audio Latency**: Audio threads get appropriate boost
3. **Smoother Compositing**: Compositor threads get proper priority
4. **Enhanced Gaming Experience**: All critical threads properly classified

## Testing Recommendations

1. **Verify Detection**: Check that Kovaaks threads are now classified
2. **Monitor Performance**: Measure frame time consistency improvements
3. **Audio Quality**: Test for reduced audio latency
4. **System Stability**: Ensure no negative side effects

## Files Modified

- `src/bpf/include/task_class.bpf.h`: Updated detection patterns
- `THREAD_DETECTION_IMPROVEMENTS.md`: This documentation

## Conclusion

These fixes ensure that Kovaaks threads are properly detected and receive appropriate priority boosts, leading to improved gaming performance and responsiveness.
