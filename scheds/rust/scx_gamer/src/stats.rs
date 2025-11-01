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
    
    /* Diagnostic counters for classification debugging */
    #[stat(desc = "Total classification attempts (gamer_runnable calls)")]
    pub classification_attempts: u64,
    #[stat(desc = "Times is_first_classification was true")]
    pub first_classification_true: u64,
    #[stat(desc = "Times is_exact_game_thread was true")]
    pub is_exact_game_thread_true: u64,
    #[stat(desc = "Times input handler name matched")]
    pub input_handler_name_match: u64,
    #[stat(desc = "Times main thread check matched")]
    pub main_thread_match: u64,
    #[stat(desc = "Times GPU submit name matched")]
    pub gpu_submit_name_match: u64,
    #[stat(desc = "Times GPU submit fentry matched")]
    pub gpu_submit_fentry_match: u64,
    #[stat(desc = "GPU runtime pattern samples collected")]
    pub runtime_pattern_gpu_samples: u64,
    #[stat(desc = "Audio runtime pattern samples collected")]
    pub runtime_pattern_audio_samples: u64,
    #[stat(desc = "Times input handler name check attempted")]
    pub input_handler_name_check_attempts: u64,
    #[stat(desc = "Times input handler name pattern matched (before game thread check)")]
    pub input_handler_name_pattern_match: u64,
    
    /* Diagnostic counters for network/audio/background detection */
    #[stat(desc = "Times is_network_thread() was called")]
    pub network_fentry_checks: u64,
    #[stat(desc = "Times network fentry hook found thread")]
    pub network_fentry_matches: u64,
    #[stat(desc = "Times is_network_name() was called")]
    pub network_name_checks: u64,
    #[stat(desc = "Times network name pattern matched")]
    pub network_name_matches: u64,
    #[stat(desc = "Times is_system_audio_thread() was called")]
    pub system_audio_fentry_checks: u64,
    #[stat(desc = "Times system audio fentry hook found thread")]
    pub system_audio_fentry_matches: u64,
    #[stat(desc = "Times is_system_audio_name() was called")]
    pub system_audio_name_checks: u64,
    #[stat(desc = "Times system audio name pattern matched")]
    pub system_audio_name_matches: u64,
    #[stat(desc = "Times is_background_name() was called")]
    pub background_name_checks: u64,
    #[stat(desc = "Times background name pattern matched")]
    pub background_name_matches: u64,
    #[stat(desc = "Times background runtime pattern was checked")]
    pub background_pattern_checks: u64,
    #[stat(desc = "Times background pattern samples collected")]
    pub background_pattern_samples: u64,
    
    /* Fentry hook call counters (from network_detect.bpf.h and audio_detect.bpf.h) */
    #[stat(desc = "Network fentry send calls")]
    pub network_detect_send_calls: u64,
    #[stat(desc = "Network fentry recv calls")]
    pub network_detect_recv_calls: u64,
    #[stat(desc = "Audio fentry ALSA calls")]
    pub audio_detect_alsa_calls: u64,
    #[stat(desc = "Audio fentry USB calls")]
    pub audio_detect_usb_calls: u64,
    
    #[stat(desc = "Continuous input mode active (1=yes, 0=no)")]
    pub continuous_input_mode: u64,
    #[stat(desc = "Keyboard lane boost active (1=yes, 0=no)")]
    pub continuous_input_lane_keyboard: u64,
    #[stat(desc = "Mouse lane continuous input flag")]
    pub continuous_input_lane_mouse: u64,
    #[stat(desc = "Other lane continuous input flag")]
    pub continuous_input_lane_other: u64,

    /* Fentry Hook Monitoring: Kernel-level input detection */
    #[stat(desc = "fentry total input events seen")]
    pub fentry_total_events: u64,
    #[stat(desc = "fentry boost triggers activated")]
    pub fentry_boost_triggers: u64,
    #[stat(desc = "fentry events from gaming devices")]
    pub fentry_gaming_events: u64,
    #[stat(desc = "fentry events filtered (non-gaming)")]
    pub fentry_filtered_events: u64,
    #[stat(desc = "ring buffer overflow events dropped")]
    pub ringbuf_overflow_events: u64,
    
    /* Ring Buffer Input Latency Tracking: Kernel→Userspace→Processing */
    #[stat(desc = "ring buffer avg latency (ns)")]
    pub ringbuf_latency_avg_ns: u64,
    #[stat(desc = "ring buffer p50 latency (ns)")]
    pub ringbuf_latency_p50_ns: u64,
    #[stat(desc = "ring buffer p95 latency (ns)")]
    pub ringbuf_latency_p95_ns: u64,
    #[stat(desc = "ring buffer p99 latency (ns)")]
    pub ringbuf_latency_p99_ns: u64,
    #[stat(desc = "ring buffer min latency (ns)")]
    pub ringbuf_latency_min_ns: u64,
    #[stat(desc = "ring buffer max latency (ns)")]
    pub ringbuf_latency_max_ns: u64,

    // Userspace ring buffer queue metrics
    #[stat(desc = "rb queue dropped total (userspace)")]
    pub rb_queue_dropped_total: u64,
    #[stat(desc = "rb queue high watermark (userspace)")]
    pub rb_queue_high_watermark: u64,

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
    
    /* P0: CPU Placement Verification */
    #[stat(desc = "GPU threads kept on physical cores (cache affinity)")]
    pub gpu_phys_kept: u64,
    #[stat(desc = "Compositor threads kept on physical cores (cache affinity)")]
    pub compositor_phys_kept: u64,
    #[stat(desc = "GPU preferred core fallback (when preferred core unavailable)")]
    pub gpu_pref_fallback: u64,
    
    /* P0: Deadline Tracking */
    #[stat(desc = "Total deadline misses detected")]
    pub deadline_misses: u64,
    #[stat(desc = "Auto-boost actions taken (self-healing)")]
    pub auto_boosts: u64,
    
    /* P0: Scheduler State */
    #[stat(desc = "Scheduler generation counter (incremented on restart/game change)")]
    pub scheduler_generation: u32,
    #[stat(desc = "Runtime-detected foreground TGID (vs foreground_tgid config)")]
    pub detected_fg_tgid: u32,
    
    /* P0: Window Status */
    #[stat(desc = "Input window currently active (1=yes, 0=no)")]
    pub input_window_active: u64,
    #[stat(desc = "Frame window currently active (1=yes, 0=no)")]
    pub frame_window_active: u64,
    #[stat(desc = "Timestamp when input window expires (ns)")]
    pub input_window_until_ns: u64,
    #[stat(desc = "Timestamp when frame window expires (ns)")]
    pub frame_window_until_ns: u64,
    
    /* P1: Boost Distribution */
    #[stat(desc = "Threads at boost level 0 (no boost)")]
    pub boost_distribution_0: u64,
    #[stat(desc = "Threads at boost level 1")]
    pub boost_distribution_1: u64,
    #[stat(desc = "Threads at boost level 2")]
    pub boost_distribution_2: u64,
    #[stat(desc = "Threads at boost level 3")]
    pub boost_distribution_3: u64,
    #[stat(desc = "Threads at boost level 4")]
    pub boost_distribution_4: u64,
    #[stat(desc = "Threads at boost level 5")]
    pub boost_distribution_5: u64,
    #[stat(desc = "Threads at boost level 6")]
    pub boost_distribution_6: u64,
    #[stat(desc = "Threads at boost level 7 (max boost - input handlers)")]
    pub boost_distribution_7: u64,
    
    /* P1: Migration Cooldown */
    #[stat(desc = "Migrations blocked by cooldown (32ms post-migration)")]
    pub mig_blocked_cooldown: u64,
    
    /* P1: Input Lane Status */
    #[stat(desc = "Keyboard lane trigger rate (events/sec)")]
    pub input_lane_keyboard_rate: u32,
    #[stat(desc = "Mouse lane trigger rate (events/sec)")]
    pub input_lane_mouse_rate: u32,
    #[stat(desc = "Other lane trigger rate (events/sec)")]
    pub input_lane_other_rate: u32,
    
    /* P2: Game Detection Details */
    #[stat(desc = "Game detection method (bpf_lsm, inotify, manual, none)")]
    pub game_detection_method: String,
    #[stat(desc = "Game detection confidence score (0-100)")]
    pub game_detection_score: u8,
    #[stat(desc = "Game detection timestamp (unix seconds)")]
    pub game_detection_timestamp: u64,
    
    /* P2: Frame Timing */
    #[stat(desc = "Estimated frame interval (ns, EMA of inter-frame time)")]
    pub frame_interval_ns: u64,
    #[stat(desc = "Total frames presented")]
    pub frame_count: u64,
    #[stat(desc = "Timestamp of last page flip (ns)")]
    pub last_page_flip_ns: u64,
    
    /* AI Analytics: Latency Percentiles (from histograms) */
    #[stat(desc = "select_cpu latency p10 (ns)")]
    pub select_cpu_latency_p10: u64,
    #[stat(desc = "select_cpu latency p25 (ns)")]
    pub select_cpu_latency_p25: u64,
    #[stat(desc = "select_cpu latency p50 (ns)")]
    pub select_cpu_latency_p50: u64,
    #[stat(desc = "select_cpu latency p75 (ns)")]
    pub select_cpu_latency_p75: u64,
    #[stat(desc = "select_cpu latency p90 (ns)")]
    pub select_cpu_latency_p90: u64,
    #[stat(desc = "select_cpu latency p95 (ns)")]
    pub select_cpu_latency_p95: u64,
    #[stat(desc = "select_cpu latency p99 (ns)")]
    pub select_cpu_latency_p99: u64,
    #[stat(desc = "select_cpu latency p999 (ns)")]
    pub select_cpu_latency_p999: u64,
    
    #[stat(desc = "enqueue latency p10 (ns)")]
    pub enqueue_latency_p10: u64,
    #[stat(desc = "enqueue latency p25 (ns)")]
    pub enqueue_latency_p25: u64,
    #[stat(desc = "enqueue latency p50 (ns)")]
    pub enqueue_latency_p50: u64,
    #[stat(desc = "enqueue latency p75 (ns)")]
    pub enqueue_latency_p75: u64,
    #[stat(desc = "enqueue latency p90 (ns)")]
    pub enqueue_latency_p90: u64,
    #[stat(desc = "enqueue latency p95 (ns)")]
    pub enqueue_latency_p95: u64,
    #[stat(desc = "enqueue latency p99 (ns)")]
    pub enqueue_latency_p99: u64,
    #[stat(desc = "enqueue latency p999 (ns)")]
    pub enqueue_latency_p999: u64,
    
    #[stat(desc = "dispatch latency p10 (ns)")]
    pub dispatch_latency_p10: u64,
    #[stat(desc = "dispatch latency p25 (ns)")]
    pub dispatch_latency_p25: u64,
    #[stat(desc = "dispatch latency p50 (ns)")]
    pub dispatch_latency_p50: u64,
    #[stat(desc = "dispatch latency p75 (ns)")]
    pub dispatch_latency_p75: u64,
    #[stat(desc = "dispatch latency p90 (ns)")]
    pub dispatch_latency_p90: u64,
    #[stat(desc = "dispatch latency p95 (ns)")]
    pub dispatch_latency_p95: u64,
    #[stat(desc = "dispatch latency p99 (ns)")]
    pub dispatch_latency_p99: u64,
    #[stat(desc = "dispatch latency p999 (ns)")]
    pub dispatch_latency_p999: u64,
    
    /* AI Analytics: Temporal Patterns */
    #[stat(desc = "Migrations in last 10 seconds")]
    pub migrations_last_10s: u64,
    #[stat(desc = "Migrations in last 60 seconds")]
    pub migrations_last_60s: u64,
    #[stat(desc = "CPU utilization trend: increasing/decreasing/stable")]
    pub cpu_util_trend: String,
    #[stat(desc = "Frame rate trend: increasing/decreasing/stable")]
    pub frame_rate_trend: String,
    
    /* AI Analytics: Classification Confidence Scores */
    #[stat(desc = "Input handler classification confidence (0-100)")]
    pub input_handler_confidence: u8,
    #[stat(desc = "GPU submit classification confidence (0-100)")]
    pub gpu_submit_confidence: u8,
    #[stat(desc = "Game audio classification confidence (0-100)")]
    pub game_audio_confidence: u8,
    #[stat(desc = "System audio classification confidence (0-100)")]
    pub system_audio_confidence: u8,
    #[stat(desc = "Network classification confidence (0-100)")]
    pub network_confidence: u8,
    #[stat(desc = "Background classification confidence (0-100)")]
    pub background_confidence: u8,
    
    /* AI Analytics: Thread Type Distribution Percentages */
    #[stat(desc = "Input handler threads percentage of total")]
    pub input_handler_pct: f64,
    #[stat(desc = "GPU submit threads percentage of total")]
    pub gpu_submit_pct: f64,
    #[stat(desc = "Game audio threads percentage of total")]
    pub game_audio_pct: f64,
    #[stat(desc = "System audio threads percentage of total")]
    pub system_audio_pct: f64,
    #[stat(desc = "Compositor threads percentage of total")]
    pub compositor_pct: f64,
    #[stat(desc = "Network threads percentage of total")]
    pub network_pct: f64,
    #[stat(desc = "Background threads percentage of total")]
    pub background_pct: f64,
    #[stat(desc = "Total classified threads")]
    pub total_classified_threads: u64,
}

impl Metrics {
    /// Calculate percentile from histogram buckets (log scale)
    /// Histogram buckets: 0: <100ns, 1: 100-200ns, 2: 200-400ns, 3: 400-800ns,
    /// 4: 800ns-1.6us, 5: 1.6-3.2us, 6: 3.2-6.4us, 7: 6.4-12.8us,
    /// 8: 12.8-25.6us, 9: 25.6-51.2us, 10: 51.2-102.4us, 11: >102.4us
    pub fn histogram_percentile(hist: &[u64; 12], percentile: f64) -> u64 {
        let total: u64 = hist.iter().sum();
        if total == 0 {
            return 0;
        }
        
        let target_count = (total as f64 * percentile / 100.0) as u64;
        let mut cumulative = 0u64;
        
        // Bucket thresholds (midpoint of each bucket range)
        let thresholds = [
            50,      // bucket 0: <100ns (use 50ns as midpoint)
            150,     // bucket 1: 100-200ns
            300,     // bucket 2: 200-400ns
            600,     // bucket 3: 400-800ns
            1200,    // bucket 4: 800ns-1.6us
            2400,    // bucket 5: 1.6-3.2us
            4800,    // bucket 6: 3.2-6.4us
            9600,    // bucket 7: 6.4-12.8us
            19200,   // bucket 8: 12.8-25.6us
            38400,   // bucket 9: 25.6-51.2us
            76800,   // bucket 10: 51.2-102.4us
            153600,  // bucket 11: >102.4us (use upper bound estimate)
        ];
        
        for (i, &count) in hist.iter().enumerate() {
            cumulative += count;
            if cumulative >= target_count {
                return thresholds[i];
            }
        }
        
        // If we didn't find it (shouldn't happen), return last bucket threshold
        thresholds[11]
    }
    
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
            // cpu_util is a live EMA (0..1024), not a counter; keep as-is
            cpu_util: self.cpu_util,
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
            continuous_input_lane_keyboard: self.continuous_input_lane_keyboard,
            continuous_input_lane_mouse: self.continuous_input_lane_mouse,
            continuous_input_lane_other: self.continuous_input_lane_other,
            
            // Diagnostic counters: cumulative totals (show growth over time, not delta)
            classification_attempts: self.classification_attempts,
            first_classification_true: self.first_classification_true,
            is_exact_game_thread_true: self.is_exact_game_thread_true,
            input_handler_name_match: self.input_handler_name_match,
            main_thread_match: self.main_thread_match,
            gpu_submit_name_match: self.gpu_submit_name_match,
            gpu_submit_fentry_match: self.gpu_submit_fentry_match,
            runtime_pattern_gpu_samples: self.runtime_pattern_gpu_samples,
            runtime_pattern_audio_samples: self.runtime_pattern_audio_samples,
            input_handler_name_check_attempts: self.input_handler_name_check_attempts,
            input_handler_name_pattern_match: self.input_handler_name_pattern_match,
            
            // Diagnostic counters for network/audio/background detection: cumulative totals
            network_fentry_checks: self.network_fentry_checks,
            network_fentry_matches: self.network_fentry_matches,
            network_name_checks: self.network_name_checks,
            network_name_matches: self.network_name_matches,
            system_audio_fentry_checks: self.system_audio_fentry_checks,
            system_audio_fentry_matches: self.system_audio_fentry_matches,
            system_audio_name_checks: self.system_audio_name_checks,
            system_audio_name_matches: self.system_audio_name_matches,
            background_name_checks: self.background_name_checks,
            background_name_matches: self.background_name_matches,
            background_pattern_checks: self.background_pattern_checks,
            background_pattern_samples: self.background_pattern_samples,
            
            // Fentry hook call counters: cumulative totals
            network_detect_send_calls: self.network_detect_send_calls,
            network_detect_recv_calls: self.network_detect_recv_calls,
            audio_detect_alsa_calls: self.audio_detect_alsa_calls,
            audio_detect_usb_calls: self.audio_detect_usb_calls,

            // Fentry stats: cumulative totals (show growth over time, not delta)
            fentry_total_events: self.fentry_total_events,
            fentry_boost_triggers: self.fentry_boost_triggers,
            fentry_gaming_events: self.fentry_gaming_events,
            fentry_filtered_events: self.fentry_filtered_events,
            ringbuf_overflow_events: self.ringbuf_overflow_events,
            
            // Ring buffer latency: live measurements (not deltas)
            ringbuf_latency_avg_ns: self.ringbuf_latency_avg_ns,
            ringbuf_latency_p50_ns: self.ringbuf_latency_p50_ns,
            ringbuf_latency_p95_ns: self.ringbuf_latency_p95_ns,
            ringbuf_latency_p99_ns: self.ringbuf_latency_p99_ns,
            ringbuf_latency_min_ns: self.ringbuf_latency_min_ns,
            ringbuf_latency_max_ns: self.ringbuf_latency_max_ns,

            // Userspace RB queue metrics are live (not counters)
            rb_queue_dropped_total: self.rb_queue_dropped_total,
            rb_queue_high_watermark: self.rb_queue_high_watermark,

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
            
            // P0: CPU Placement Verification - cumulative totals
            gpu_phys_kept: self.gpu_phys_kept,
            compositor_phys_kept: self.compositor_phys_kept,
            gpu_pref_fallback: self.gpu_pref_fallback,
            
            // P0: Deadline Tracking - cumulative totals
            deadline_misses: self.deadline_misses,
            auto_boosts: self.auto_boosts,
            
            // P0: Scheduler State - live values (not deltas)
            scheduler_generation: self.scheduler_generation,
            detected_fg_tgid: self.detected_fg_tgid,
            
            // P0: Window Status - live values (not deltas)
            input_window_active: self.input_window_active,
            frame_window_active: self.frame_window_active,
            input_window_until_ns: self.input_window_until_ns,
            frame_window_until_ns: self.frame_window_until_ns,
            
            // P1: Boost Distribution - live counts (not deltas)
            boost_distribution_0: self.boost_distribution_0,
            boost_distribution_1: self.boost_distribution_1,
            boost_distribution_2: self.boost_distribution_2,
            boost_distribution_3: self.boost_distribution_3,
            boost_distribution_4: self.boost_distribution_4,
            boost_distribution_5: self.boost_distribution_5,
            boost_distribution_6: self.boost_distribution_6,
            boost_distribution_7: self.boost_distribution_7,
            
            // P1: Migration Cooldown - cumulative total
            mig_blocked_cooldown: self.mig_blocked_cooldown,
            
            // P1: Input Lane Status - live rates (not deltas)
            input_lane_keyboard_rate: self.input_lane_keyboard_rate,
            input_lane_mouse_rate: self.input_lane_mouse_rate,
            input_lane_other_rate: self.input_lane_other_rate,
            
            // P2: Game Detection Details - live values (not deltas)
            game_detection_method: self.game_detection_method.clone(),
            game_detection_score: self.game_detection_score,
            game_detection_timestamp: self.game_detection_timestamp,
            
            // P2: Frame Timing - live values (not deltas)
            frame_interval_ns: self.frame_interval_ns,
            frame_count: self.frame_count,
            last_page_flip_ns: self.last_page_flip_ns,
            
            // AI Analytics: Latency Percentiles - live values (not deltas)
            select_cpu_latency_p10: self.select_cpu_latency_p10,
            select_cpu_latency_p25: self.select_cpu_latency_p25,
            select_cpu_latency_p50: self.select_cpu_latency_p50,
            select_cpu_latency_p75: self.select_cpu_latency_p75,
            select_cpu_latency_p90: self.select_cpu_latency_p90,
            select_cpu_latency_p95: self.select_cpu_latency_p95,
            select_cpu_latency_p99: self.select_cpu_latency_p99,
            select_cpu_latency_p999: self.select_cpu_latency_p999,
            enqueue_latency_p10: self.enqueue_latency_p10,
            enqueue_latency_p25: self.enqueue_latency_p25,
            enqueue_latency_p50: self.enqueue_latency_p50,
            enqueue_latency_p75: self.enqueue_latency_p75,
            enqueue_latency_p90: self.enqueue_latency_p90,
            enqueue_latency_p95: self.enqueue_latency_p95,
            enqueue_latency_p99: self.enqueue_latency_p99,
            enqueue_latency_p999: self.enqueue_latency_p999,
            dispatch_latency_p10: self.dispatch_latency_p10,
            dispatch_latency_p25: self.dispatch_latency_p25,
            dispatch_latency_p50: self.dispatch_latency_p50,
            dispatch_latency_p75: self.dispatch_latency_p75,
            dispatch_latency_p90: self.dispatch_latency_p90,
            dispatch_latency_p95: self.dispatch_latency_p95,
            dispatch_latency_p99: self.dispatch_latency_p99,
            dispatch_latency_p999: self.dispatch_latency_p999,
            
            // AI Analytics: Temporal Patterns - live values (not deltas)
            migrations_last_10s: self.migrations_last_10s,
            migrations_last_60s: self.migrations_last_60s,
            cpu_util_trend: self.cpu_util_trend.clone(),
            frame_rate_trend: self.frame_rate_trend.clone(),
            
            // AI Analytics: Classification Confidence - live values (not deltas)
            input_handler_confidence: self.input_handler_confidence,
            gpu_submit_confidence: self.gpu_submit_confidence,
            game_audio_confidence: self.game_audio_confidence,
            system_audio_confidence: self.system_audio_confidence,
            network_confidence: self.network_confidence,
            background_confidence: self.background_confidence,
            
            // AI Analytics: Thread Type Distribution - live values (not deltas)
            input_handler_pct: self.input_handler_pct,
            gpu_submit_pct: self.gpu_submit_pct,
            game_audio_pct: self.game_audio_pct,
            system_audio_pct: self.system_audio_pct,
            compositor_pct: self.compositor_pct,
            network_pct: self.network_pct,
            background_pct: self.background_pct,
            total_classified_threads: self.total_classified_threads,
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

            let mut stdout = std::io::stdout();
            metrics.format(&mut stdout)
        },
    )
}

pub fn monitor_watch_input(intv: Duration, shutdown: Arc<AtomicBool>) -> Result<()> {
    // Reflect scheduler state: show boost (input window) based on per-interval window time
    // and show current lane flags from kernel metrics directly.
    scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        || shutdown.load(Ordering::Relaxed),
        |m| {
            // m.win_input_ns and m.timer_elapsed_ns are deltas for this interval
            let boost_on = m.win_input_ns > 0;
            let boost_pct = if m.timer_elapsed_ns > 0 {
                (m.win_input_ns as f64 * 100.0) / (m.timer_elapsed_ns as f64)
            } else { 0.0 };

            let kbd_lane = if m.continuous_input_lane_keyboard != 0 { "ON" } else { "off" };
            let mouse_lane = if m.continuous_input_lane_mouse != 0 { "ON" } else { "off" };
            let other_lane = if m.continuous_input_lane_other != 0 { "ON" } else { "off" };
            let cont = if m.continuous_input_mode != 0 { "CONT" } else { "edge" };

            println!(
                "[{}] BOOST:{} ({:>3.0}%)  KBD:{}  MOUSE:{}  OTHER:{}  mode:{}  trig_rate:{}/s",
                chrono::Local::now().format("%H:%M:%S"),
                if boost_on { "ON" } else { "off" }, boost_pct,
                kbd_lane, mouse_lane, other_lane, cont, m.input_trigger_rate
            );
            Ok(())
        },
    )
}
