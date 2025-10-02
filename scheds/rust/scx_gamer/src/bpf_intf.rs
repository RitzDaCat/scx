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
    // Set CMD_INPUT flag; wakeup_timerfn will fan-out.
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.cmd_flags |= 1u32 << 0;
        return Ok(());
    }
    Err(1)
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
    if let Some(bss) = skel.maps.bss_data.as_mut() {
        bss.cmd_flags |= (1u32 << 0) | (1u32 << 2);
        return Ok(());
    }
    Err(1)
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
