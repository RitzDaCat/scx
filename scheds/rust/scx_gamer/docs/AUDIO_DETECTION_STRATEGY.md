# Audio Detection Strategy: Comprehensive Game vs System Audio Classification

## Problem Statement

We need to detect **all audio** happening on the machine and correctly classify:
- **Game Audio**: Audio threads belonging to the foreground game
- **System Audio**: Audio threads from PipeWire, PulseAudio, ALSA (system-wide audio server)
- **USB Audio**: Hardware-specific audio interfaces

## Current Limitations

1. **Name-based detection is incomplete**: Only catches threads with specific names
   - Chromium: Only 2/100+ threads match "chromium" name
   - PipeWire: Threads like "data-loop.0", "module-rt" should match but may not wake frequently
   
2. **Fentry hooks miss PipeWire**: PipeWire doesn't use ALSA directly
   - Fentry hooks on `snd_pcm_*` won't catch PipeWire threads
   - PipeWire uses its own protocol layer

3. **Thread-level vs Process-level**: We check individual thread names, not process membership
   - Need to detect ALL threads in PipeWire/Chromium processes, not just those with specific names

## Proposed Solution: Multi-Layered Detection

### Layer 1: Process-Based Detection (TGID Tracking)

**Detect audio server processes once, classify all their threads:**

```
1. Userspace (Rust): Scan /proc to find PipeWire/PulseAudio TGIDs
   - Match by process name: "pipewire", "pipewire-pulse", "pulseaudio"
   - Store TGIDs in BPF map: system_audio_tgids_map

2. BPF: In gamer_runnable(), check if p->tgid matches known audio server TGID
   - If match: tctx->is_system_audio = 1 (all threads in that process)
   - Benefits: Catches ALL PipeWire threads (data-loop, module-rt, etc.)
```

**Detect game audio by process membership:**
```
- If p->tgid == fg_tgid AND audio pattern matches → game audio
- If p->tgid != fg_tgid AND audio pattern matches → system audio (fallback)
```

### Layer 2: Audio Flow Tracking (Who Calls Whom)

**Track audio API calls to identify source:**
```
1. Game process calls PipeWire → Threads in game process making audio calls = game audio
2. PipeWire server threads → system audio
3. Track via fentry hooks on PipeWire client API calls
```

### Layer 3: Runtime Pattern Detection (Already Working)

**High-frequency, low-exec threads:**
```
- Wakeup: 300-1200Hz (audio callback frequency)
- Exec: <500μs (short audio buffer processing)
- Works for both game and system audio
- Classify as game audio if in game process, else system audio
```

### Layer 4: Name-Based Detection (Current, as Fallback)

**Pattern matching for threads with known names:**
```
- Game audio: "AudioThread", "FMOD", "OpenAL", etc.
- System audio: "pipewire", "pulseaudio", "alsa", etc.
- Keep as fallback for threads not caught by other layers
```

## Implementation Plan

### Phase 1: TGID-Based System Audio Detection

1. **Userspace (Rust)**: Add audio server TGID tracking
   ```rust
   // In game_detect.rs or new audio_detect.rs
   fn detect_audio_servers() -> Vec<u32> {
       // Scan /proc for PipeWire, PulseAudio, etc.
       // Return list of TGIDs
   }
   ```

2. **BPF**: Add system_audio_tgids_map
   ```c
   struct {
       __uint(type, BPF_MAP_TYPE_HASH);
       __uint(max_entries, 16);  // Max 16 audio servers
       __type(key, u32);         // TGID
       __type(value, u8);        // 1 if active audio server
   } system_audio_tgids_map SEC(".maps");
   ```

3. **BPF**: Check TGID in gamer_runnable()
   ```c
   // Check if this thread belongs to known audio server process
   u32 tgid = (u32)p->tgid;
   u8 *is_audio_server = bpf_map_lookup_elem(&system_audio_tgids_map, &tgid);
   if (is_audio_server && *is_audio_server) {
       tctx->is_system_audio = 1;
       __atomic_fetch_add(&nr_system_audio_threads, 1, __ATOMIC_RELAXED);
   }
   ```

### Phase 2: Enhanced Game Audio Detection

1. **Runtime pattern + Process check:**
   ```c
   // In gamer_stopping() audio detection
   if (audio_pattern && p->tgid == fg_tgid) {
       tctx->is_game_audio = 1;  // Game audio
   } else if (audio_pattern && p->tgid != known_audio_server_tgid) {
       tctx->is_system_audio = 1;  // System audio (not in game, not in server)
   }
   ```

### Phase 3: Audio Flow Tracking (Advanced)

1. **Track PipeWire client calls:**
   - Hook PipeWire client library functions
   - When game process calls PipeWire → mark calling thread as game audio
   - PipeWire server threads are already system audio (from Phase 1)

## Detection Priority (Highest to Lowest)

1. **TGID membership** (most reliable)
   - Thread in audio server process → system audio
   - Thread in game process + audio pattern → game audio

2. **Runtime patterns** (fast, works universally)
   - High frequency (300-1200Hz) + low exec (<500μs) = audio thread
   - Classify by process membership (game vs system)

3. **Fentry hooks** (when available)
   - ALSA calls → audio thread
   - USB audio calls → USB audio

4. **Name patterns** (fallback)
   - Specific thread names like "AudioThread", "pipewire"

## Benefits

1. **Complete coverage**: All threads in audio server processes are detected
2. **Process-aware**: Distinguishes game audio vs system audio by process membership
3. **Fast detection**: TGID lookup is O(1) hash lookup
4. **Robust**: Multiple layers ensure detection even if one layer fails

## Edge Cases Handled

1. **PipeWire threads with generic names** (data-loop.0, module-rt)
   - ✅ Caught by TGID membership

2. **Game audio through system server** (game → PipeWire → hardware)
   - ✅ Game threads calling PipeWire = game audio
   - ✅ PipeWire server threads = system audio

3. **Multiple audio servers** (PipeWire + JACK)
   - ✅ Track multiple TGIDs in map

4. **Thread names that don't match process name**
   - ✅ TGID-based detection works regardless of thread name

## Performance Impact

- **TGID lookup**: ~20-40ns per thread wake (hash map lookup)
- **Userspace scan**: ~10-50ms every 5 seconds (only when audio servers start/stop)
- **Overall**: Minimal overhead, significant accuracy improvement

