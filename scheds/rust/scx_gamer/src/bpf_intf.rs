// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer v2.0: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]

// Include auto-generated bindings from intf.h
include!(concat!(env!("OUT_DIR"), "/bpf_intf.rs"));
