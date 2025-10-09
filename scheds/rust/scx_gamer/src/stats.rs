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
    #[stat(desc = "Input trigger rate (events/sec, EMA)")]
    pub input_trigger_rate: u64,
    #[stat(desc = "Continuous input mode active (1=yes, 0=no)")]
    pub continuous_input_mode: u64,

    /* Fentry Hook Monitoring: Kernel-level input detection */
    #[stat(desc = "fentry total input events seen")]
    pub fentry_total_events: u64,
    #[stat(desc = "fentry boost triggers activated")]
    pub fentry_boost_triggers: u64,
    #[stat(desc = "fentry events from gaming devices")]
    pub fentry_gaming_events: u64,
    #[stat(desc = "fentry events filtered (non-gaming)")]
    pub fentry_filtered_events: u64,

    /* BPF Profiling: Hot-path latency measurements */
    #[stat(desc = "select_cpu avg latency (ns)")]
    pub prof_select_cpu_avg_ns: u64,
    #[stat(desc = "enqueue avg latency (ns)")]
    pub prof_enqueue_avg_ns: u64,
    #[stat(desc = "dispatch avg latency (ns)")]
    pub prof_dispatch_avg_ns: u64,
    #[stat(desc = "deadline calc avg latency (ns)")]
    pub prof_deadline_avg_ns: u64,

    /* Raw profiling counters (for calculating averages) */
    #[stat(desc = "select_cpu total ns")]
    pub prof_select_cpu_ns: u64,
    #[stat(desc = "select_cpu calls")]
    pub prof_select_cpu_calls: u64,
    #[stat(desc = "enqueue total ns")]
    pub prof_enqueue_ns: u64,
    #[stat(desc = "enqueue calls")]
    pub prof_enqueue_calls: u64,
    #[stat(desc = "dispatch total ns")]
    pub prof_dispatch_ns: u64,
    #[stat(desc = "dispatch calls")]
    pub prof_dispatch_calls: u64,
    #[stat(desc = "deadline total ns")]
    pub prof_deadline_ns: u64,
    #[stat(desc = "deadline calls")]
    pub prof_deadline_calls: u64,
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
        writeln!(w, "│ CPU {:>5.1}% (avg {:>5.1}%)  EDF {:>4.1}%{}",
                 (self.cpu_util as f64) * 100.0 / 1024.0,
                 (self.cpu_util_avg as f64) * 100.0 / 1024.0,
                 edf_pct, fg)?;
        writeln!(w, "│ q: rr {:>6}  edf {:>6}  dir {:>6} ({:>4.0}%)  sh {:>6}",
                 self.rr_enq, self.edf_enq, self.direct, direct_pct, self.shared)?;
        let input_mode_indicator = if self.continuous_input_mode != 0 {
            format!("CONT@{}/s", self.input_trigger_rate)
        } else {
            format!("i:{:>5}", self.input_trig)
        };
        writeln!(w, "│ win: in {:>4.0}%  fr {:>4.0}%   hint: idle {:>6}  mm {:>6}   FG {:>3}%   trig {} f:{:>5}",
                 in_pct, fr_pct, self.idle_pick, self.mm_hint_hit, self.fg_cpu_pct, input_mode_indicator, self.frame_trig)?;
        writeln!(w, "│ mig {:>6}  blk {:>6}  sync {:>6}  fblk {:>6}  syncfast {:>6}",
                 self.migrations, self.mig_blocked, self.sync_local, self.frame_mig_block, self.sync_wake_fast)?;
        writeln!(w, "│ threads: input {:>2}  gpu {:>2}  sys_aud {:>2}  gm_aud {:>2}  comp {:>2}  net {:>2}  bg {:>2}",
                 self.input_handler_threads, self.gpu_submit_threads, self.system_audio_threads,
                 self.game_audio_threads, self.compositor_threads, self.network_threads, self.background_threads)?;

        // Show profiling data if available
        if self.prof_select_cpu_avg_ns > 0 || self.prof_enqueue_avg_ns > 0 {
            writeln!(w, "│ prof: sel {:>4}ns  enq {:>4}ns  dsp {:>4}ns  dl {:>3}ns",
                     self.prof_select_cpu_avg_ns,
                     self.prof_enqueue_avg_ns,
                     self.prof_dispatch_avg_ns,
                     self.prof_deadline_avg_ns)?;
        }
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
            fg_app: self.fg_app.clone(),  // String clone needed for delta (not in hot path)
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
            input_trigger_rate: self.input_trigger_rate,  // live rate (EMA), not delta
            continuous_input_mode: self.continuous_input_mode,  // live flag, not delta

            // Fentry stats: cumulative totals (show growth over time, not delta)
            fentry_total_events: self.fentry_total_events,
            fentry_boost_triggers: self.fentry_boost_triggers,
            fentry_gaming_events: self.fentry_gaming_events,
            fentry_filtered_events: self.fentry_filtered_events,

            // Profiling: calculate averages from deltas
            prof_select_cpu_avg_ns: if self.prof_select_cpu_calls > prev.prof_select_cpu_calls {
                (self.prof_select_cpu_ns - prev.prof_select_cpu_ns) / (self.prof_select_cpu_calls - prev.prof_select_cpu_calls)
            } else { 0 },
            prof_enqueue_avg_ns: if self.prof_enqueue_calls > prev.prof_enqueue_calls {
                (self.prof_enqueue_ns - prev.prof_enqueue_ns) / (self.prof_enqueue_calls - prev.prof_enqueue_calls)
            } else { 0 },
            prof_dispatch_avg_ns: if self.prof_dispatch_calls > prev.prof_dispatch_calls {
                (self.prof_dispatch_ns - prev.prof_dispatch_ns) / (self.prof_dispatch_calls - prev.prof_dispatch_calls)
            } else { 0 },
            prof_deadline_avg_ns: if self.prof_deadline_calls > prev.prof_deadline_calls {
                (self.prof_deadline_ns - prev.prof_deadline_ns) / (self.prof_deadline_calls - prev.prof_deadline_calls)
            } else { 0 },

            // Raw counters (deltas for monitoring, absolute for export)
            prof_select_cpu_ns: self.prof_select_cpu_ns,
            prof_select_cpu_calls: self.prof_select_cpu_calls,
            prof_enqueue_ns: self.prof_enqueue_ns,
            prof_enqueue_calls: self.prof_enqueue_calls,
            prof_dispatch_ns: self.prof_dispatch_ns,
            prof_dispatch_calls: self.prof_dispatch_calls,
            prof_deadline_ns: self.prof_deadline_ns,
            prof_deadline_calls: self.prof_deadline_calls,
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
    // Custom monitor with terminal clearing to prevent endless scrolling
    // Clears screen every 20 outputs (~100 seconds at 5s interval)
    let mut iteration_count = 0u32;
    const CLEAR_INTERVAL: u32 = 20;  // Clear every 20 stats outputs

    scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        || shutdown.load(Ordering::Relaxed),
        |metrics| {
            iteration_count += 1;

            // Clear terminal every CLEAR_INTERVAL outputs
            // ANSI escape codes: \x1b[2J (clear screen) + \x1b[H (move cursor to top)
            if iteration_count % CLEAR_INTERVAL == 1 {
                print!("\x1b[2J\x1b[H");  // Clear screen and move to top
                println!("─────────────────────────────────────────────────────────");
                println!("scx_gamer stats (screen cleared every {} outputs)", CLEAR_INTERVAL);
                println!("─────────────────────────────────────────────────────────");
                println!();
            }

            metrics.format(&mut std::io::stdout())
        },
    )
}
