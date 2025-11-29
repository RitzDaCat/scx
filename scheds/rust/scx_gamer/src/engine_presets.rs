// SPDX-License-Identifier: GPL-2.0
//
// Engine preset library: seeds engine_profile_map with known thread heuristics.

use crate::BpfSkel;
use anyhow::Result;
use libbpf_rs::{MapCore, MapFlags};
use log::{info, warn};
use std::mem;
use std::os::raw::c_char;
use std::slice;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EngineProfileKey {
    comm: [c_char; 16],
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct EngineProfileEntry {
    avg_exec_ns: u32,
    avg_wakeup_freq: u32,
    last_boost: u8,
    reserved: u8,
    sample_count: u16,
    _pad: u32,
    last_updated_ns: u64,
}

struct ThreadPreset {
    comm: &'static str,
    last_boost: u8,
    avg_exec_ns: u32,
    avg_wakeup_freq: u32,
}

const PRESETS: &[ThreadPreset] = &[
    // Unreal Engine main threads
    // BUG FIX: GameThread was boost=7 (INPUT HANDLER level) - this competes with mice/keyboards!
    // GameThread should be boost=6 (GPU/render level) - it processes game logic, not raw input.
    // Input handlers (boost=7) must have absolute priority for low-latency mouse/keyboard.
    ThreadPreset {
        comm: "GameThread",
        last_boost: 6,  // Was 7 - fixed to not compete with input handlers
        avg_exec_ns: 220_000,
        avg_wakeup_freq: 600,
    },
    ThreadPreset {
        comm: "RenderThread",
        last_boost: 6,
        avg_exec_ns: 200_000,
        avg_wakeup_freq: 600,
    },
    ThreadPreset {
        comm: "RHIThread",
        last_boost: 6,
        avg_exec_ns: 180_000,
        avg_wakeup_freq: 600,
    },
    ThreadPreset {
        comm: "TaskGraphThr",
        last_boost: 5,
        avg_exec_ns: 90_000,
        avg_wakeup_freq: 900,
    },
    ThreadPreset {
        comm: "AudioThread",
        last_boost: 4,
        avg_exec_ns: 75_000,
        avg_wakeup_freq: 1000,
    },
    // Source / Unity style worker threads
    ThreadPreset {
        comm: "MainThrd",
        last_boost: 6,
        avg_exec_ns: 160_000,
        avg_wakeup_freq: 500,
    },
    ThreadPreset {
        comm: "Renderer",
        last_boost: 6,
        avg_exec_ns: 140_000,
        avg_wakeup_freq: 500,
    },
    ThreadPreset {
        comm: "WorkerThd",
        last_boost: 4,
        avg_exec_ns: 80_000,
        avg_wakeup_freq: 900,
    },
    // Audio and input fallbacks
    ThreadPreset {
        comm: "FMODThread",
        last_boost: 4,
        avg_exec_ns: 70_000,
        avg_wakeup_freq: 950,
    },
    ThreadPreset {
        comm: "SDLInput",
        last_boost: 7,
        avg_exec_ns: 40_000,
        avg_wakeup_freq: 1000,
    },
];

pub fn seed_engine_presets(skel: &mut BpfSkel) -> Result<()> {
    let map = &skel.maps.engine_profile_map;

    #[inline(always)]
    fn as_bytes<T>(value: &T) -> &[u8] {
        // SAFETY: EngineProfileKey/Entry are #[repr(C)] POD structs with no padding-sensitive references.
        unsafe { slice::from_raw_parts((value as *const T) as *const u8, mem::size_of::<T>()) }
    }

    for preset in PRESETS {
        let mut key = EngineProfileKey {
            comm: [0 as c_char; 16],
        };
        let comm_bytes = preset.comm.as_bytes();
        let len = comm_bytes.len().min(key.comm.len());
        for i in 0..len {
            key.comm[i] = comm_bytes[i] as c_char;
        }

        let entry = EngineProfileEntry {
            avg_exec_ns: preset.avg_exec_ns,
            avg_wakeup_freq: preset.avg_wakeup_freq,
            last_boost: preset.last_boost,
            reserved: 0,
            sample_count: 1,
            _pad: 0,
            last_updated_ns: 0,
        };

        let key_bytes = as_bytes(&key);
        let entry_bytes = as_bytes(&entry);

        if let Err(err) = map.update(key_bytes, entry_bytes, MapFlags::ANY) {
            warn!(
                "Engine presets: unable to insert preset for '{}': {}",
                preset.comm, err
            );
        }
    }

    info!(
        "Engine presets: seeded {} thread profiles (Unreal, Unity, Source defaults)",
        PRESETS.len()
    );
    Ok(())
}
