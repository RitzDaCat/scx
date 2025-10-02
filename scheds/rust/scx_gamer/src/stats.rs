// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Gaming-optimized scheduler for low-latency input and frame delivery
// Copyright (c) 2025 RitzDaCat
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2.

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::Local;
use scx_stats::prelude::*;
use scx_stats_derive::stat_doc;
use scx_stats_derive::Stats;
use serde::Deserialize;
use serde::Serialize;

#[stat_doc]
#[derive(Clone, Debug, Default, Serialize, Deserialize, Stats)]
#[serde(default)]
#[stat(top)]
pub struct Metrics {
    #[stat(desc = "Average CPU utilization %")]
    pub cpu_util: u64,
    #[stat(desc = "RR enqueues in interval")]
    pub rr_enq: u64,
    #[stat(desc = "EDF enqueues in interval")]
    pub edf_enq: u64,
    #[stat(desc = "Direct dispatches in interval")]
    pub direct: u64,
    #[stat(desc = "Shared dispatches in interval")]
    pub shared: u64,
    #[stat(desc = "Migrations in interval")]
    pub migrations: u64,
    #[stat(desc = "Migrations blocked by limiter in interval")]
    pub mig_blocked: u64,
    #[stat(desc = "Sync wake kept local in interval")]
    pub sync_local: u64,
    #[stat(desc = "Migrations blocked during frame window in interval")]
    pub frame_mig_block: u64,
    #[stat(desc = "Avg CPU util (BPF EMA)")]
    pub cpu_util_avg: u64,
    #[stat(desc = "Estimated frame rate (Hz via EMA)")]
    pub frame_hz_est: f64,
    #[stat(desc = "Foreground TGID (0=off)")]
    pub fg_pid: u64,
    #[stat(desc = "Focused window identifier")]
    pub fg_app: String,
    #[stat(desc = "Focused window fullscreen flag")]
    pub fg_fullscreen: u64,
    #[stat(desc = "Input window time (ns, total)")]
    pub win_input_ns: u64,
    #[stat(desc = "Frame window time (ns, total)")]
    pub win_frame_ns: u64,
    #[stat(desc = "Wake timer elapsed (ns, total)")]
    pub timer_elapsed_ns: u64,
    #[stat(desc = "Idle CPU pick hits in interval")]
    pub idle_pick: u64,
    #[stat(desc = "Per-mm hint hits in interval")]
    pub mm_hint_hit: u64,
    #[stat(desc = "Foreground runtime share % (0-100)")]
    pub fg_cpu_pct: u64,
    #[stat(desc = "Input triggers in interval")]
    pub input_trig: u64,
    #[stat(desc = "Frame triggers in interval")]
    pub frame_trig: u64,
    #[stat(desc = "SYNC wake fast path hits in interval")]
    pub sync_wake_fast: u64,
    #[stat(desc = "GPU submission threads detected (live count)")]
    pub gpu_submit_threads: u64,
    #[stat(desc = "Background/batch threads detected (live count)")]
    pub background_threads: u64,
    #[stat(desc = "Compositor threads detected (live count)")]
    pub compositor_threads: u64,
    #[stat(desc = "Network/netcode threads detected (live count)")]
    pub network_threads: u64,
    #[stat(desc = "System audio threads detected (live count)")]
    pub system_audio_threads: u64,
    #[stat(desc = "Game audio threads detected (live count)")]
    pub game_audio_threads: u64,
    #[stat(desc = "Input handler threads detected (live count)")]
    pub input_handler_threads: u64,
}

impl Metrics {
    pub fn format<W: Write>(&self, w: &mut W) -> Result<()> {
        let edf_pct = if self.rr_enq + self.edf_enq > 0 {
            (self.edf_enq as f64) * 100.0 / (self.rr_enq + self.edf_enq) as f64
        } else { 0.0 };
        let fg = if !self.fg_app.is_empty() {
            let mut label = format!("  FG {}", self.fg_app);
            if self.fg_fullscreen != 0 {
                label.push_str(" [fs]");
            }
            label
        } else if self.fg_pid != 0 {
            format!("  FG {}", self.fg_pid)
        } else {
            String::new()
        };
        let (in_pct, fr_pct) = if self.timer_elapsed_ns > 0 {
            (
                (self.win_input_ns as f64) * 100.0 / (self.timer_elapsed_ns as f64),
                (self.win_frame_ns as f64) * 100.0 / (self.timer_elapsed_ns as f64),
            )
        } else { (0.0, 0.0) };
        let direct_pct = if self.rr_enq + self.direct > 0 { (self.direct as f64) * 100.0 / (self.rr_enq + self.direct) as f64 } else { 0.0 };

        let now = Local::now();
        writeln!(w, "┌─ {} {} ─", crate::SCHEDULER_NAME, now.format("%H:%M:%S"))?;
        writeln!(w, "│ CPU {:>5.1}% (avg {:>5.1}%)  EDF {:>4.1}%  FPS~ {:>6.1}{}",
                 (self.cpu_util as f64) * 100.0 / 1024.0,
                 (self.cpu_util_avg as f64) * 100.0 / 1024.0,
                 edf_pct, self.frame_hz_est, fg)?;
        writeln!(w, "│ q: rr {:>6}  edf {:>6}  dir {:>6} ({:>4.0}%)  sh {:>6}",
                 self.rr_enq, self.edf_enq, self.direct, direct_pct, self.shared)?;
        writeln!(w, "│ win: in {:>4.0}%  fr {:>4.0}%   hint: idle {:>6}  mm {:>6}   FG {:>3}%   trig i:{:>5} f:{:>5}",
                 in_pct, fr_pct, self.idle_pick, self.mm_hint_hit, self.fg_cpu_pct, self.input_trig, self.frame_trig)?;
        writeln!(w, "│ mig {:>6}  blk {:>6}  sync {:>6}  fblk {:>6}  syncfast {:>6}",
                 self.migrations, self.mig_blocked, self.sync_local, self.frame_mig_block, self.sync_wake_fast)?;
        writeln!(w, "│ threads: input {:>2}  gpu {:>2}  sys_aud {:>2}  gm_aud {:>2}  comp {:>2}  net {:>2}  bg {:>2}",
                 self.input_handler_threads, self.gpu_submit_threads, self.system_audio_threads,
                 self.game_audio_threads, self.compositor_threads, self.network_threads, self.background_threads)?;
        writeln!(w, "└─")?;
        Ok(())
    }

    fn delta(&self, prev: &Self) -> Self {
        Self {
            cpu_util: self.cpu_util.saturating_sub(prev.cpu_util),
            rr_enq: self.rr_enq.saturating_sub(prev.rr_enq),
            edf_enq: self.edf_enq.saturating_sub(prev.edf_enq),
            direct: self.direct.saturating_sub(prev.direct),
            shared: self.shared.saturating_sub(prev.shared),
            migrations: self.migrations.saturating_sub(prev.migrations),
            mig_blocked: self.mig_blocked.saturating_sub(prev.mig_blocked),
            sync_local: self.sync_local.saturating_sub(prev.sync_local),
            frame_mig_block: self.frame_mig_block.saturating_sub(prev.frame_mig_block),
            cpu_util_avg: self.cpu_util_avg,
            frame_hz_est: self.frame_hz_est,
            fg_pid: self.fg_pid,
            fg_app: self.fg_app.clone(),
            fg_fullscreen: self.fg_fullscreen,
            win_input_ns: self.win_input_ns.saturating_sub(prev.win_input_ns),
            win_frame_ns: self.win_frame_ns.saturating_sub(prev.win_frame_ns),
            timer_elapsed_ns: self.timer_elapsed_ns.saturating_sub(prev.timer_elapsed_ns),
            idle_pick: self.idle_pick.saturating_sub(prev.idle_pick),
            mm_hint_hit: self.mm_hint_hit.saturating_sub(prev.mm_hint_hit),
            fg_cpu_pct: self.fg_cpu_pct,
            input_trig: self.input_trig.saturating_sub(prev.input_trig),
            frame_trig: self.frame_trig.saturating_sub(prev.frame_trig),
            sync_wake_fast: self.sync_wake_fast.saturating_sub(prev.sync_wake_fast),
            gpu_submit_threads: self.gpu_submit_threads,  // live count, not delta
            background_threads: self.background_threads,  // live count, not delta
            compositor_threads: self.compositor_threads,  // live count, not delta
            network_threads: self.network_threads,  // live count, not delta
            system_audio_threads: self.system_audio_threads,  // live count, not delta
            game_audio_threads: self.game_audio_threads,  // live count, not delta
            input_handler_threads: self.input_handler_threads,  // live count, not delta
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_includes_numbers() {
        let m = Metrics {
            cpu_util: 512,
            rr_enq: 10,
            edf_enq: 5,
            direct: 2,
            shared: 1,
            migrations: 3,
            mig_blocked: 1,
            sync_local: 7,
            frame_mig_block: 0,
            cpu_util_avg: 256,
            frame_hz_est: 240.0,
            fg_pid: 1234,
            ..Default::default()
        };
        let mut out = Vec::new();
        m.format(&mut out).unwrap();
        let s = String::from_utf8(out).unwrap();
        assert!(s.contains("EDF"));
        assert!(s.contains("FG 1234"));
    }
}

pub fn server_data() -> StatsServerData<(), Metrics> {
    let open: Box<dyn StatsOpener<(), Metrics>> = Box::new(move |(req_ch, res_ch)| {
        req_ch.send(())?;
        let mut prev = res_ch.recv()?;

        let read: Box<dyn StatsReader<(), Metrics>> = Box::new(move |_args, (req_ch, res_ch)| {
            req_ch.send(())?;
            let cur = res_ch.recv()?;
            let delta = cur.delta(&prev);
            prev = cur;
            delta.to_json()
        });

        Ok(read)
    });

    StatsServerData::new()
        .add_meta(Metrics::meta())
        .add_ops("top", StatsOps { open, close: None })
}

pub fn monitor(intv: Duration, shutdown: Arc<AtomicBool>) -> Result<()> {
    scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        || shutdown.load(Ordering::Relaxed),
        |metrics| metrics.format(&mut std::io::stdout()),
    )
}
