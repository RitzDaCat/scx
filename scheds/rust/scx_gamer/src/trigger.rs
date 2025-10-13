// SPDX-License-Identifier: GPL-2.0

use crate::{bpf_intf, BpfSkel};

pub trait TriggerOps {
    fn trigger_input_lane(&self, skel: &mut BpfSkel<'_>, lane: super::InputLane);
    fn trigger_input_with_napi_lane(&self, skel: &mut BpfSkel<'_>, lane: super::InputLane);
}

#[derive(Default)]
pub struct BpfTrigger;

impl TriggerOps for BpfTrigger {
    #[inline(always)]
    fn trigger_input_lane(&self, skel: &mut BpfSkel<'_>, lane: super::InputLane) {
        let _ = bpf_intf::trigger_input_lane(skel, lane);
    }
    #[inline(always)]
    fn trigger_input_with_napi_lane(&self, skel: &mut BpfSkel<'_>, lane: super::InputLane) {
        let _ = bpf_intf::trigger_input_with_napi_lane(skel, lane);
    }
}

#[cfg(test)]
pub struct MockTrigger {
    pub input_count: std::sync::atomic::AtomicU64,
}

#[cfg(test)]
impl Default for MockTrigger {
    fn default() -> Self {
        Self {
            input_count: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

#[cfg(test)]
impl TriggerOps for MockTrigger {
    fn trigger_input_lane(&self, _skel: &mut BpfSkel<'_>, _lane: super::InputLane) {
        self.input_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn trigger_input_with_napi_lane(&self, _skel: &mut BpfSkel<'_>, _lane: super::InputLane) {
        self.input_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}


