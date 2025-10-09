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
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.cmd_flags |= 1u32 << 1;
        return Ok(());
    }
    Err(1)
}

#[inline(always)]
pub fn trigger_napi_softirq_window(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.cmd_flags |= 1u32 << 2;
        return Ok(());
    }
    Err(1)
}

#[inline(always)]
pub fn trigger_input_with_napi(skel: &mut crate::BpfSkel) -> Result<(), u32> {
    // ZERO-LATENCY: Execute both syscalls immediately for input + NAPI windows
    // First activate input window
    let _ = trigger_input_window(skel)?;

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

#[repr(C)]
pub struct BssCounters {
    pub rr_enq: u64,
    pub edf_enq: u64,
    pub nr_direct_dispatches: u64,
    pub nr_shared_dispatches: u64,
    pub nr_migrations: u64,
    pub nr_mig_blocked: u64,
}
