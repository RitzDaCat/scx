// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

include!(concat!(env!("OUT_DIR"), "/bpf_intf.rs"));
use crate::InputLane;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PowerHintArg {
    level: u32,
    duration_ms: u32,
}

#[inline(always)]
pub fn trigger_input_window(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    // ZERO-LATENCY: Direct BPF syscall for immediate input window activation
    // Bypasses timer-based flag processing to eliminate 0-5ms delay
    // This syscall executes fanout_set_input_window() synchronously in BPF
    //
    // MICRO-OPTIMIZATION: Use const Default to avoid heap allocation
    // libbpf_rs v0.24+ optimizes Default::default() to compile-time constant

    let prog = &mut skel.progs.set_input_window;
    match prog.test_run(libbpf_rs::ProgramInput::default()) {
        Ok(out) => {
            if out.return_value == 0 {
                Ok(())
            } else {
                Err(out.return_value)
            }
        }
        Err(_) => Err(1),
    }
}

#[inline(always)]
pub fn trigger_frame_window(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    trigger_input_window(skel)
}

#[inline(always)]
pub fn trigger_napi_softirq_window(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    let prog = &mut skel.progs.set_napi_softirq_window;
    match prog.test_run(libbpf_rs::ProgramInput::default()) {
        Ok(out) => {
            if out.return_value == 0 {
                Ok(())
            } else {
                Err(out.return_value)
            }
        }
        Err(_) => Err(1),
    }
}

#[inline(always)]
pub fn trigger_input_with_napi(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    // ZERO-LATENCY: Execute both syscalls immediately for input + NAPI windows
    // First activate input window
    trigger_input_window(skel)?;

    // Then activate NAPI window
    let prog = &mut skel.progs.set_napi_softirq_window;
    match prog.test_run(libbpf_rs::ProgramInput::default()) {
        Ok(out) => {
            if out.return_value == 0 {
                Ok(())
            } else {
                Err(out.return_value)
            }
        }
        Err(_) => Err(1),
    }
}

#[inline(always)]
pub fn trigger_input_lane(skel: &mut crate::BpfSkel, lane: InputLane) -> Result<(), u32> {
    let prog = &mut skel.progs.set_input_lane;
    let raw: u32 = lane as u32;
    let mut bytes = raw.to_ne_bytes();
    let prog_input = libbpf_rs::ProgramInput {
        context_in: Some(unsafe {
            std::slice::from_raw_parts_mut(bytes.as_mut_ptr(), bytes.len())
        }),
        ..Default::default()
    };
    match prog.test_run(prog_input) {
        Ok(out) => {
            if out.return_value == 0 {
                Ok(())
            } else {
                Err(out.return_value)
            }
        }
        Err(_) => Err(1),
    }
}

#[inline(always)]
pub fn trigger_input_with_napi_lane(skel: &mut crate::BpfSkel, lane: InputLane) -> Result<(), u32> {
    trigger_input_lane(skel, lane)?;
    if matches!(lane, InputLane::Mouse) {
        let prog = &mut skel.progs.set_napi_softirq_window;
        match prog.test_run(libbpf_rs::ProgramInput::default()) {
            Ok(out) => {
                if out.return_value != 0 {
                    return Err(out.return_value);
                }
            }
            Err(_) => return Err(1),
        }
    }
    Ok(())
}

#[inline(always)]
pub fn set_runtime_trace(skel: &mut crate::BpfSkel, enable: bool) -> Result<(), u32> {
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.runtime_trace_enable = if enable { 1 } else { 0 };
        return Ok(());
    }
    Err(1)
}

#[inline(always)]
pub fn set_detector_trace(skel: &mut crate::BpfSkel, enable: bool) -> Result<(), u32> {
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.detector_trace_enable = if enable { 1 } else { 0 };
        return Ok(());
    }
    Err(1)
}

#[inline(always)]
pub fn set_dispatch_events(skel: &mut crate::BpfSkel, enable: bool) -> Result<(), u32> {
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.dispatch_event_enable = if enable { 1 } else { 0 };
        return Ok(());
    }
    Err(1)
}

#[inline(always)]
pub fn set_power_hint(skel: &mut crate::BpfSkel, level: u32, duration_ms: u32) -> Result<(), u32> {
    let prog = &mut skel.progs.set_power_hint;
    let mut args = PowerHintArg { level, duration_ms };
    let prog_input = libbpf_rs::ProgramInput {
        context_in: Some(unsafe {
            std::slice::from_raw_parts_mut(
                &mut args as *mut _ as *mut u8,
                std::mem::size_of_val(&args),
            )
        }),
        ..Default::default()
    };
    match prog.test_run(prog_input) {
        Ok(out) => {
            if out.return_value == 0 {
                Ok(())
            } else {
                Err(out.return_value)
            }
        }
        Err(_) => Err(1),
    }
}

#[repr(C)]
pub struct BssCounters {
    pub rr_enq: u64,
    pub edf_enq: u64,
    pub nr_direct_dispatches: u64,
    pub nr_shared_dispatches: u64,
    pub nr_migrations: u64,
    pub nr_mig_blocked: u64,
}
