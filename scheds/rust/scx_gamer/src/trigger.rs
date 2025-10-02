// SPDX-License-Identifier: GPL-2.0

use crate::{bpf_intf, BpfSkel};

pub trait TriggerOps {
    fn trigger_input(&self, skel: &mut BpfSkel<'_>);
    fn trigger_input_with_napi(&self, skel: &mut BpfSkel<'_>);
}

#[derive(Default)]
pub struct BpfTrigger;

impl TriggerOps for BpfTrigger {
    #[inline(always)]
    fn trigger_input(&self, skel: &mut BpfSkel<'_>) {
        let _ = bpf_intf::trigger_input_window(skel);
    }
    #[inline(always)]
    fn trigger_input_with_napi(&self, skel: &mut BpfSkel<'_>) {
        let _ = bpf_intf::trigger_input_with_napi(skel);
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
    fn trigger_input(&self, _skel: &mut BpfSkel<'_>) {
        self.input_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    fn trigger_input_with_napi(&self, _skel: &mut BpfSkel<'_>) {
        self.input_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
}


