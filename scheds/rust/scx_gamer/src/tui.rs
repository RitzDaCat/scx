// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Terminal UI Dashboard
// Copyright (c) 2025 RitzDaCat
//
// Interactive terminal dashboard for real-time scheduler monitoring.

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use chrono::{DateTime, Local};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, Paragraph, Row, Table},
    Frame, Terminal,
};
use ratatui::symbols;
use std::collections::VecDeque;

/* OPTIMIZATION: Circular buffer for efficient historical data management
 * Replaces VecDeque with O(1) operations instead of O(n) */
#[derive(Debug, Clone)]
struct CircularBuffer<T> {
    data: Vec<T>,
    head: usize,
    tail: usize,
    count: usize,
    capacity: usize,
}

impl<T: Clone> CircularBuffer<T> {
    fn new(capacity: usize) -> Self {
        Self {
            data: Vec::with_capacity(capacity),
            head: 0,
            tail: 0,
            count: 0,
            capacity,
        }
    }

    fn push(&mut self, value: T) {
        if self.data.len() < self.capacity {
            self.data.push(value);
            self.tail = self.data.len() - 1;
        } else {
            self.data[self.tail] = value;
            self.tail = (self.tail + 1) % self.capacity;
        }
        
        if self.count < self.capacity {
            self.count += 1;
        } else {
            self.head = (self.head + 1) % self.capacity;
        }
    }

    fn len(&self) -> usize {
        self.count
    }



    fn get(&self, index: usize) -> Option<&T> {
        if index >= self.count {
            return None;
        }
        let actual_index = (self.head + index) % self.capacity;
        Some(&self.data[actual_index])
    }

    fn clear(&mut self) {
        self.head = 0;
        self.tail = 0;
        self.count = 0;
    }
}

use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use std::sync::mpsc;
use nix::sched::{sched_setaffinity, CpuSet};
use nix::unistd::Pid;
use scx_utils::{Topology, CoreType};

/// Terminal guard to ensure terminal is restored even on panic
/// Follows Ratatui best practice: use Drop guard for terminal restoration
struct TerminalGuard;

impl TerminalGuard {
    fn new() -> Result<Self> {
        enable_raw_mode()?;
        execute!(io::stderr(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Best-effort restoration - ignore errors in Drop
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen);
    }
}

use crate::stats::Metrics;
use crate::Opts;
use crate::process_monitor::find_obs_pid;

/// Active tab selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Overview,
    Performance,
    Threads,
    Events,
    Help,
}

impl ActiveTab {
    pub fn next(&self) -> Self {
        match self {
            ActiveTab::Overview => ActiveTab::Performance,
            ActiveTab::Performance => ActiveTab::Threads,
            ActiveTab::Threads => ActiveTab::Events,
            ActiveTab::Events => ActiveTab::Help,
            ActiveTab::Help => ActiveTab::Overview,
        }
    }

    pub fn prev(&self) -> Self {
        match self {
            ActiveTab::Overview => ActiveTab::Help,
            ActiveTab::Performance => ActiveTab::Overview,
            ActiveTab::Threads => ActiveTab::Performance,
            ActiveTab::Events => ActiveTab::Threads,
            ActiveTab::Help => ActiveTab::Events,
        }
    }

}

/// Update rate configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateRate {
    RealTime,   // 1s
    Fast,       // 5s
    Medium,     // 30s
    Slow,       // 60s
}

impl UpdateRate {
    pub fn next(&self) -> Self {
        match self {
            UpdateRate::RealTime => UpdateRate::Fast,
            UpdateRate::Fast => UpdateRate::Medium,
            UpdateRate::Medium => UpdateRate::Slow,
            UpdateRate::Slow => UpdateRate::RealTime,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            UpdateRate::RealTime => "1s",
            UpdateRate::Fast => "5s",
            UpdateRate::Medium => "30s",
            UpdateRate::Slow => "60s",
        }
    }
}

/// Historical data storage (ring buffer)
#[derive(Clone)]
pub struct HistoricalData {
    /* OPTIMIZATION: Use circular buffers for O(1) operations
     * Replaces VecDeque with efficient circular buffer implementation */
    cpu_util: CircularBuffer<f64>,
    cpu_avg: CircularBuffer<f64>,
    fg_cpu_pct: CircularBuffer<f64>,
    latency_select: CircularBuffer<u64>,
    latency_enqueue: CircularBuffer<u64>,
    latency_dispatch: CircularBuffer<u64>,
    latency_ringbuf_p50: CircularBuffer<u64>,
    latency_ringbuf_p95: CircularBuffer<u64>,
    latency_ringbuf_p99: CircularBuffer<u64>,
    migrations: CircularBuffer<u64>,
    mig_blocked: CircularBuffer<u64>,
    input_rate: CircularBuffer<u64>,
    direct_pct: CircularBuffer<f64>,
    edf_pct: CircularBuffer<f64>,
    timestamps: CircularBuffer<Instant>,

    // Cumulative totals (sum of deltas for lifetime stats)
    pub total_rr_enq: u64,
    pub total_edf_enq: u64,
    pub total_direct: u64,
    pub total_migrations: u64,
    pub total_mig_blocked: u64,
    pub total_ringbuf_overflow: u64,
}

impl HistoricalData {
    pub fn new(max_samples: usize) -> Self {
        Self {
            /* OPTIMIZATION: Initialize circular buffers with fixed capacity
             * This provides O(1) push/pop operations instead of O(n) */
            cpu_util: CircularBuffer::new(max_samples),
            cpu_avg: CircularBuffer::new(max_samples),
            fg_cpu_pct: CircularBuffer::new(max_samples),
            latency_select: CircularBuffer::new(max_samples),
            latency_enqueue: CircularBuffer::new(max_samples),
            latency_dispatch: CircularBuffer::new(max_samples),
            latency_ringbuf_p50: CircularBuffer::new(max_samples),
            latency_ringbuf_p95: CircularBuffer::new(max_samples),
            latency_ringbuf_p99: CircularBuffer::new(max_samples),
            migrations: CircularBuffer::new(max_samples),
            mig_blocked: CircularBuffer::new(max_samples),
            input_rate: CircularBuffer::new(max_samples),
            direct_pct: CircularBuffer::new(max_samples),
            edf_pct: CircularBuffer::new(max_samples),
            timestamps: CircularBuffer::new(max_samples),

            total_rr_enq: 0,
            total_edf_enq: 0,
            total_direct: 0,
            total_migrations: 0,
            total_mig_blocked: 0,
            total_ringbuf_overflow: 0,
        }
    }

    pub fn push(&mut self, metrics: &Metrics) {
        // Calculate derived values
        let cpu_util_pct = (metrics.cpu_util as f64) * 100.0 / 1024.0;
        let cpu_avg_pct = (metrics.cpu_util_avg as f64) * 100.0 / 1024.0;
        let fg_cpu = metrics.fg_cpu_pct as f64;

        let total_enq = metrics.rr_enq + metrics.edf_enq;
        let edf_pct = if total_enq > 0 {
            (metrics.edf_enq as f64 * 100.0) / total_enq as f64
        } else { 0.0 };

        let direct_total = metrics.rr_enq + metrics.direct;
        let direct_pct = if direct_total > 0 {
            (metrics.direct as f64 * 100.0) / direct_total as f64
        } else { 0.0 };

        /* OPTIMIZATION: Use circular buffer push for O(1) operations
         * Replaces macro with direct circular buffer operations */
        self.cpu_util.push(cpu_util_pct);
        self.cpu_avg.push(cpu_avg_pct);
        self.fg_cpu_pct.push(fg_cpu);
        self.latency_select.push(metrics.prof_select_cpu_avg_ns);
        self.latency_enqueue.push(metrics.prof_enqueue_avg_ns);
        self.latency_dispatch.push(metrics.prof_dispatch_avg_ns);
        self.latency_ringbuf_p50.push(metrics.ringbuf_latency_p50_ns);
        self.latency_ringbuf_p95.push(metrics.ringbuf_latency_p95_ns);
        self.latency_ringbuf_p99.push(metrics.ringbuf_latency_p99_ns);
        self.migrations.push(metrics.migrations);
        self.mig_blocked.push(metrics.mig_blocked);
        self.input_rate.push(metrics.input_trigger_rate);
        self.direct_pct.push(direct_pct);
        self.edf_pct.push(edf_pct);
        self.timestamps.push(Instant::now());

        // Accumulate cumulative totals (metrics are deltas)
        self.total_rr_enq = self.total_rr_enq.saturating_add(metrics.rr_enq);
        self.total_edf_enq = self.total_edf_enq.saturating_add(metrics.edf_enq);
        self.total_direct = self.total_direct.saturating_add(metrics.direct);
        self.total_migrations = self.total_migrations.saturating_add(metrics.migrations);
        self.total_mig_blocked = self.total_mig_blocked.saturating_add(metrics.mig_blocked);
        // Ring buffer overflow is cumulative (not delta), so use current value directly
        self.total_ringbuf_overflow = metrics.ringbuf_overflow_events;
    }

    pub fn get_sparkline_f64(&self, field: &str, last_n: usize) -> Vec<f64> {
        /* OPTIMIZATION: Use circular buffer iteration for efficient access
         * Circular buffers provide O(1) random access and iteration */
        match field {
            "cpu_util" => self.get_last_n_f64(&self.cpu_util, last_n),
            "cpu_avg" => self.get_last_n_f64(&self.cpu_avg, last_n),
            "fg_cpu_pct" => self.get_last_n_f64(&self.fg_cpu_pct, last_n),
            "direct_pct" => self.get_last_n_f64(&self.direct_pct, last_n),
            "edf_pct" => self.get_last_n_f64(&self.edf_pct, last_n),
            _ => vec![],
        }
    }

    fn get_last_n_f64(&self, buffer: &CircularBuffer<f64>, last_n: usize) -> Vec<f64> {
        let mut result = Vec::new();
        let start = buffer.len().saturating_sub(last_n);
        for i in start..buffer.len() {
            if let Some(value) = buffer.get(i) {
                result.push(*value);
            }
        }
        result
    }

    pub fn get_sparkline_u64(&self, field: &str, last_n: usize) -> Vec<u64> {
        /* OPTIMIZATION: Use circular buffer iteration for efficient access */
        match field {
            "input_rate" => self.get_last_n_u64(&self.input_rate, last_n),
            "migrations" => self.get_last_n_u64(&self.migrations, last_n),
            "mig_blocked" => self.get_last_n_u64(&self.mig_blocked, last_n),
            "latency_ringbuf_p50" => self.get_last_n_u64(&self.latency_ringbuf_p50, last_n),
            "latency_ringbuf_p95" => self.get_last_n_u64(&self.latency_ringbuf_p95, last_n),
            "latency_ringbuf_p99" => self.get_last_n_u64(&self.latency_ringbuf_p99, last_n),
            _ => vec![],
        }
    }

    fn get_last_n_u64(&self, buffer: &CircularBuffer<u64>, last_n: usize) -> Vec<u64> {
        let mut result = Vec::new();
        let start = buffer.len().saturating_sub(last_n);
        for i in start..buffer.len() {
            if let Some(value) = buffer.get(i) {
                result.push(*value);
            }
        }
        result
    }

    pub fn latest_f64(&self, field: &str) -> Option<f64> {
        /* OPTIMIZATION: Use circular buffer get for latest value
         * Get the most recent value from the circular buffer */
        match field {
            "cpu_util" => self.cpu_util.get(self.cpu_util.len().saturating_sub(1)).copied(),
            "cpu_avg" => self.cpu_avg.get(self.cpu_avg.len().saturating_sub(1)).copied(),
            "fg_cpu_pct" => self.fg_cpu_pct.get(self.fg_cpu_pct.len().saturating_sub(1)).copied(),
            "direct_pct" => self.direct_pct.get(self.direct_pct.len().saturating_sub(1)).copied(),
            "edf_pct" => self.edf_pct.get(self.edf_pct.len().saturating_sub(1)).copied(),
            _ => None,
        }
    }

    pub fn latest_u64(&self, field: &str) -> Option<u64> {
        /* OPTIMIZATION: Use circular buffer get for latest value */
        match field {
            "input_rate" => self.input_rate.get(self.input_rate.len().saturating_sub(1)).copied(),
            "migrations" => self.migrations.get(self.migrations.len().saturating_sub(1)).copied(),
            "mig_blocked" => self.mig_blocked.get(self.mig_blocked.len().saturating_sub(1)).copied(),
            "latency_select" => self.latency_select.get(self.latency_select.len().saturating_sub(1)).copied(),
            "latency_enqueue" => self.latency_enqueue.get(self.latency_enqueue.len().saturating_sub(1)).copied(),
            "latency_dispatch" => self.latency_dispatch.get(self.latency_dispatch.len().saturating_sub(1)).copied(),
            "latency_ringbuf_p50" => self.latency_ringbuf_p50.get(self.latency_ringbuf_p50.len().saturating_sub(1)).copied(),
            "latency_ringbuf_p95" => self.latency_ringbuf_p95.get(self.latency_ringbuf_p95.len().saturating_sub(1)).copied(),
            "latency_ringbuf_p99" => self.latency_ringbuf_p99.get(self.latency_ringbuf_p99.len().saturating_sub(1)).copied(),
            _ => None,
        }
    }

    pub fn reset(&mut self) {
        self.cpu_util.clear();
        self.cpu_avg.clear();
        self.fg_cpu_pct.clear();
        self.latency_select.clear();
        self.latency_enqueue.clear();
        self.latency_dispatch.clear();
        self.latency_ringbuf_p50.clear();
        self.latency_ringbuf_p95.clear();
        self.latency_ringbuf_p99.clear();
        self.migrations.clear();
        self.mig_blocked.clear();
        self.input_rate.clear();
        self.direct_pct.clear();
        self.edf_pct.clear();
        self.timestamps.clear();

        self.total_rr_enq = 0;
        self.total_edf_enq = 0;
        self.total_direct = 0;
        self.total_migrations = 0;
        self.total_mig_blocked = 0;
        self.total_ringbuf_overflow = 0;
    }
}

/// Event log entry
#[derive(Clone)]
pub struct EventEntry {
    pub timestamp: DateTime<Local>,
    pub level: EventLevel,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EventLevel {
    Info,
    Warn,
    Error,
}

/// Event log storage
#[derive(Clone)]
pub struct EventLog {
    max_events: usize,
    events: VecDeque<EventEntry>,
}

impl EventLog {
    pub fn new(max_events: usize) -> Self {
        Self {
            max_events,
            events: VecDeque::with_capacity(max_events),
        }
    }

    pub fn push(&mut self, level: EventLevel, message: String) {
        if self.events.len() >= self.max_events {
            self.events.pop_front();
        }
        self.events.push_back(EventEntry {
            timestamp: Local::now(),
            level,
            message,
        });
    }

    pub fn events(&self) -> &VecDeque<EventEntry> {
        &self.events
    }
}

/// Scheduler status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SchedulerStatus {
    Running,      // Scheduler active, receiving metrics
    Stopped,      // Scheduler not running
    Initializing, // Starting up
}

/// TUI state management
#[derive(Clone)]
pub struct TuiState {
    pub paused: bool,
    pub start_time: Instant,
    pub config: ConfigSummary,
    pub active_tab: ActiveTab,
    pub update_rate: UpdateRate,
    pub history: HistoricalData,
    pub event_log: EventLog,
    pub obs_pid: Option<u32>,
    pub game_pid: u32,  // Will be updated with detected game
    pub scheduler_status: SchedulerStatus,
    pub last_successful_update: Instant,
    pub prev_metrics: Option<Metrics>,
    pub last_metrics: Option<Metrics>,
    pub input_idle_alert: bool,
    pub mig_block_alert: bool,
    pub latency_alert: bool,
    pub fentry_idle_alert: bool,
    pub stale_alert: bool,
    pub prev_game_pid: u32,
    pub prev_game_app: String,
    pub overflow_alert_fired: bool,
    pub queue_drop_alert_fired: bool,
    pub latency_p95_high_alert_fired: bool,
    pub latency_p95_elevated_alert_fired: bool,
    pub fentry_filter_alert_fired: bool,
}

/// Scheduler configuration summary
#[derive(Clone)]
pub struct ConfigSummary {
    pub slice_us: u64,
    pub input_window_us: u64,
    pub wakeup_timer_us: u64,
}

impl ConfigSummary {
    pub fn from_opts(opts: &Opts) -> Self {
        Self {
            slice_us: opts.slice_us,
            input_window_us: opts.input_window_us,
            wakeup_timer_us: opts.wakeup_timer_us,
        }
    }
}

impl Default for ConfigSummary {
    fn default() -> Self {
        Self {
            slice_us: 10,
            input_window_us: 5000,
            wakeup_timer_us: 500,
        }
    }
}

impl TuiState {
    pub fn new(config: ConfigSummary, history_len: usize, event_capacity: usize) -> Self {
        // Try to find OBS PID
        let obs_pid = find_obs_pid();

        Self {
            paused: false,
            start_time: Instant::now(),
            config,
            active_tab: ActiveTab::Overview,
            update_rate: UpdateRate::RealTime,
            history: HistoricalData::new(history_len), // 5 minutes at 1s intervals
            event_log: EventLog::new(event_capacity),
            obs_pid,
            game_pid: 0,  // Will be updated with detected game
            scheduler_status: SchedulerStatus::Initializing,
            last_successful_update: Instant::now(),
            prev_metrics: None,
            last_metrics: None,
            input_idle_alert: false,
            mig_block_alert: false,
            latency_alert: false,
            fentry_idle_alert: false,
            stale_alert: false,
            prev_game_pid: 0,
            prev_game_app: String::new(),
            overflow_alert_fired: false,
            queue_drop_alert_fired: false,
            latency_p95_high_alert_fired: false,
            latency_p95_elevated_alert_fired: false,
            fentry_filter_alert_fired: false,
        }
    }

    pub fn next_tab(&mut self) {
        self.active_tab = self.active_tab.next();
    }

    pub fn prev_tab(&mut self) {
        self.active_tab = self.active_tab.prev();
    }

    pub fn cycle_update_rate(&mut self) {
        self.update_rate = self.update_rate.next();
        self.event_log.push(
            EventLevel::Info,
            format!("Update rate changed to {}", self.update_rate.label())
        );
    }

    pub fn reset_stats(&mut self) {
        self.history.reset();
        self.prev_metrics = None;
        self.last_metrics = None;
        self.input_idle_alert = false;
        self.mig_block_alert = false;
        self.latency_alert = false;
        self.fentry_idle_alert = false;
        self.stale_alert = false;
        self.event_log.push(EventLevel::Info, "Statistics reset".to_string());
    }
}

/// Format uptime duration
fn format_uptime(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {:02}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

/// Format numbers with thousand separators
fn format_number(n: u64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.insert(0, ',');
        }
        result.insert(0, c);
    }
    result
}

fn render_health_check(f: &mut Frame, area: Rect, metrics: &Metrics, _state: &TuiState) {
    let bpf_running = true;  // Placeholder status
    let game_detected = metrics.fg_pid != 0;
    let fentry_active = metrics.fentry_total_events > 0;
    let rb_ok = metrics.ringbuf_overflow_events == 0;

    let status_icon = |ok: bool| {
        if ok {
            Span::styled(" ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" ✗ ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        }
    };

    // Extract game name for display
    let game_display = if game_detected {
        if !metrics.fg_app.is_empty() {
            // Extract basename (handle both / and \ for Wine paths)
            let name = metrics.fg_app
                .rsplit('/')
                .next()
                .or_else(|| metrics.fg_app.rsplit('\\').next())
                .unwrap_or(&metrics.fg_app);
            format!("{} (PID: {})", name, metrics.fg_pid)
        } else {
            format!("PID: {} (no app name)", metrics.fg_pid)
        }
    } else {
        "No game detected".to_string()
    };

    let health_info = vec![
        Line::from(vec![
            status_icon(bpf_running),
            Span::raw("Scheduler:  "),
            Span::styled(
                if bpf_running { "Active" } else { "Stopped" },
                if bpf_running { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
            ),
            Span::raw("    │    "),
            status_icon(game_detected),
            Span::raw("Game Detection:  "),
            Span::styled(
                if game_detected { "Active" } else { "Inactive" },
                if game_detected { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
            ),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Detected Game: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                game_display.clone(),
                if game_detected { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }
            ),
        ]),
        Line::from(vec![
            status_icon(fentry_active),
            Span::raw("Fentry Hook:  "),
            Span::styled(
                if fentry_active { "Enabled" } else { "Disabled" },
                if fentry_active { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
            ),
            Span::raw("    │    "),
            status_icon(rb_ok),
            Span::raw("RB Overflow:  "),
            Span::styled(
                if rb_ok { "OK" } else { "Not OK" },
                if rb_ok { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
            ),
        ]),
    ];

    let health_block = Paragraph::new(health_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(if bpf_running && game_detected { Color::Green } else { Color::Yellow }))
            .title(Span::styled(
                " SCHEDULER HEALTH CHECK ",
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            )));

    f.render_widget(health_block, area);
}

fn render_process_comparison(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let game_name = if !metrics.fg_app.is_empty() {
        // Extract basename (handle both / and \ for Wine paths)
        let name = metrics.fg_app
            .rsplit('/')
            .next()
            .or_else(|| metrics.fg_app.rsplit('\\').next())
            .unwrap_or(&metrics.fg_app);
        name.to_string()
    } else if metrics.fg_pid != 0 {
        format!("PID: {} (no app name)", metrics.fg_pid)
    } else {
        "No game detected".to_string()
    };

    // Show full path if available (useful for testing/diagnostics)
    let full_path_display = if !metrics.fg_app.is_empty() && metrics.fg_app.len() > 50 {
        // Truncate very long paths
        format!("...{}", &metrics.fg_app[metrics.fg_app.len().saturating_sub(47)..])
    } else if !metrics.fg_app.is_empty() {
        metrics.fg_app.clone()
    } else {
        String::new()
    };

    // Simplified comparison display without proc monitor stats
    let mut comparison_text = vec![
        Line::from(vec![
            Span::styled("Game: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("{:<30}", game_name),
                if metrics.fg_pid != 0 { Style::default().fg(Color::Green).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::DarkGray) }
            ),
            Span::raw("  PID: "),
            Span::styled(format!("{:>5}", metrics.fg_pid), Style::default().fg(Color::Yellow)),
            Span::raw("  CPU: "),
            Span::styled(format!("{:>5.1}%", metrics.fg_cpu_pct as f64), Style::default().fg(Color::Green)),
        ]),
    ];
    
    // Show full path for testing/diagnostics (helpful for Wine games and new game testing)
    if !full_path_display.is_empty() {
        comparison_text.push(Line::from(vec![
            Span::raw("  Path: "),
            Span::styled(full_path_display, Style::default().fg(Color::DarkGray)),
        ]));
    }
    
    comparison_text.push(Line::from(vec![
        Span::styled("OBS:  ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(format!("{:<20}", "Not detected")),
        Span::raw("  PID: "),
        Span::styled(format!("{:>5}", state.obs_pid.unwrap_or(0)), Style::default().fg(Color::Yellow)),
        Span::raw("  CPU: "),
        Span::styled(format!("{:>5.1}%", 0.0), Style::default().fg(Color::Yellow)),
    ]));

    let comparison_block = Paragraph::new(comparison_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Yellow))
            .title(Span::styled(
                " PROCESS COMPARISON (Game vs OBS) ",
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            )));

    f.render_widget(comparison_block, area);
}

fn render_config(f: &mut Frame, area: Rect, state: &TuiState) {
    let cfg = &state.config;
    // Build configuration display
    let config_text = vec![
        Line::from(vec![
            Span::raw("Slice: "),
            Span::styled(format!("{}µs", cfg.slice_us), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Lag: "),
            Span::styled(format!("{}µs", 0), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Input Win: "),
            Span::styled(format!("{}µs", cfg.input_window_us), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Wake Timer: "),
            Span::styled(format!("{}µs", cfg.wakeup_timer_us), Style::default().fg(Color::Yellow)),
        ]),
    ];
    let config_block = Paragraph::new(config_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Magenta))
            .title(Span::styled(
                " SCHEDULER CONFIGURATION ",
                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
            )));

    f.render_widget(config_block, area);
}

fn render_header(f: &mut Frame, area: Rect, state: &TuiState) {
    let uptime = format_uptime(state.start_time.elapsed());
    let pause_status = if state.paused { " [PAUSED]" } else { "" };
    let update_rate = format!(" Update: {}", state.update_rate.label());

    // Show active tab in header for visual feedback
    let active_tab = match state.active_tab {
        ActiveTab::Overview => "Overview",
        ActiveTab::Performance => "Performance",
        ActiveTab::Threads => "Threads",
        ActiveTab::Events => "Events",
        ActiveTab::Help => "Help",
    };

    // Scheduler status indicator
    let (status_text, status_color) = match state.scheduler_status {
        SchedulerStatus::Running => ("RUNNING", Color::Green),
        SchedulerStatus::Stopped => ("STOPPED", Color::Red),
        SchedulerStatus::Initializing => ("STARTING", Color::Yellow),
    };

    // Check if metrics stream is stale (scheduler not sending updates)
    // Note: This can happen if scheduler crashes or event loop blocks
    let data_age = state.last_successful_update.elapsed().as_secs();
    let stale_indicator = if data_age > 10 && state.scheduler_status == SchedulerStatus::Running {
        format!(" [NO METRICS {}s]", data_age)
    } else {
        String::new()
    };

    let header = Paragraph::new(Line::from(vec![
        Span::styled("scx_gamer", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  │  "),
        Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        Span::styled(&stale_indicator, Style::default().fg(Color::Red)),
        Span::raw("  │  "),
        Span::styled(
            chrono::Local::now().format("%H:%M:%S").to_string(),
            Style::default().fg(Color::Cyan),
        ),
        Span::raw("  │  Uptime: "),
        Span::styled(uptime, Style::default().fg(Color::Yellow)),
        Span::raw("  │  Tab: "),
        Span::styled(active_tab, Style::default().fg(Color::Cyan)),
        Span::raw("  │ "),
        Span::styled(update_rate, Style::default().fg(Color::Magenta)),
        Span::styled(pause_status, Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
    ]))
    .block(Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(Span::styled(
            " Gaming Scheduler Monitor ",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
        )));

    f.render_widget(header, area);
}

/// Create a simple progress bar visualization
fn create_bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

fn render_row1(f: &mut Frame, area: Rect, metrics: &Metrics) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Game Info
    let game_info = if !metrics.fg_app.is_empty() {
        // Extract basename for cleaner display (handle / and \ for Wine)
        let game_basename = metrics.fg_app
            .rsplit('/')
            .next()
            .or_else(|| metrics.fg_app.rsplit('\\').next())
            .unwrap_or(&metrics.fg_app);
        let fullscreen = if metrics.fg_fullscreen != 0 { " [FULLSCREEN]" } else { "" };
        let pid_display = if metrics.fg_pid > 0 {
            format!("PID: {}", metrics.fg_pid)
        } else {
            "PID: unknown".to_string()
        };
        vec![
            Line::from(vec![
                Span::styled(game_basename, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(fullscreen, Style::default().fg(Color::Blue)),
            ]),
            Line::from(pid_display),
            Line::from(format!("FG CPU Share: {}%", metrics.fg_cpu_pct)),
        ]
    } else if metrics.fg_pid != 0 {
        vec![
            Line::from(Span::styled(
                format!("PID: {}", metrics.fg_pid),
                Style::default().fg(Color::Yellow),
            )),
            Line::from("No app name detected"),
            Line::from(format!("FG CPU Share: {}%", metrics.fg_cpu_pct)),
        ]
    } else {
        vec![
            Line::from(Span::styled("No foreground game", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from(""),
        ]
    };

    let game_block = Paragraph::new(game_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" GAME ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(game_block, chunks[0]);

    // CPU Info
    let cpu_util_pct = (metrics.cpu_util as f64) * 100.0 / 1024.0;
    let cpu_avg_pct = (metrics.cpu_util_avg as f64) * 100.0 / 1024.0;
    let fg_cpu_pct = metrics.fg_cpu_pct as f64;

    let cpu_color = if cpu_util_pct > 90.0 {
        Color::Red
    } else if cpu_util_pct > 70.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    let cpu_info = vec![
        Line::from(vec![
            Span::raw("Load:  "),
            Span::styled(
                format!("{} {:>5.1}%", create_bar(cpu_util_pct, 10), cpu_util_pct),
                Style::default().fg(cpu_color),
            ),
        ]),
        Line::from(vec![
            Span::raw("Avg:   "),
            Span::styled(
                format!("{} {:>5.1}%", create_bar(cpu_avg_pct, 10), cpu_avg_pct),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::raw("FG%:   "),
            Span::styled(
                format!("{} {:>5.1}%", create_bar(fg_cpu_pct, 10), fg_cpu_pct),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ];

    let cpu_block = Paragraph::new(cpu_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" CPU ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(cpu_block, chunks[1]);
}

fn render_row2(f: &mut Frame, area: Rect, metrics: &Metrics, _state: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    render_input_mode(f, layout[0], metrics, _state);
    render_queue_status(f, layout[1], metrics, _state);
}

fn render_input_mode(f: &mut Frame, area: Rect, metrics: &Metrics, _state: &TuiState) {
    let input_active = metrics.win_input_ns > 0;
    let fentry_active = metrics.fentry_total_events > 0;
    let continuous_mode = metrics.continuous_input_mode != 0;

    let status_style = if input_active {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };

    // Calculate fentry event breakdown
    let total_fentry = metrics.fentry_total_events;
    let gaming_pct = if total_fentry > 0 {
        (metrics.fentry_gaming_events as f64 * 100.0) / total_fentry as f64
    } else {
        0.0
    };
    let _filtered_pct = if total_fentry > 0 {
        (metrics.fentry_filtered_events as f64 * 100.0) / total_fentry as f64
    } else {
        0.0
    };

    // Continuous input mode lanes
    let mut lanes = Vec::new();
    if metrics.continuous_input_lane_keyboard != 0 {
        lanes.push("KB");
    }
    if metrics.continuous_input_lane_mouse != 0 {
        lanes.push("Mouse");
    }
    if metrics.continuous_input_lane_other != 0 {
        lanes.push("Other");
    }
    let lanes_str = if lanes.is_empty() {
        "None".to_string()
    } else {
        lanes.join(", ")
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Input Window", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                if input_active {
                    let total_ns = metrics.timer_elapsed_ns as f64;
                    let pct = if total_ns > 0.0 {
                        (metrics.win_input_ns as f64 * 100.0) / total_ns
                    } else {
                        0.0
                    };
                    format!("ACTIVE ({:.0}%)", pct)
                } else {
                    "IDLE".to_string()
                },
                status_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Fentry Hook", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                if fentry_active { "ENABLED".to_string() } else { "DISABLED".to_string() },
                if fentry_active { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }
            ),
            Span::raw("  "),
            Span::raw(format!("({:.1}% gaming)", gaming_pct)),
        ]),
        Line::from(vec![
            Span::styled("Continuous", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                if continuous_mode { "ACTIVE".to_string() } else { "INACTIVE".to_string() },
                if continuous_mode { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Yellow) }
            ),
            Span::raw("  "),
            Span::styled(
                format!("[{}]", lanes_str),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ];

    let block = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" INPUT STATUS "))
        .style(Style::default().fg(Color::White));
    f.render_widget(block, area);
}

fn render_queue_status(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    // Get cumulative overflow count from history
    let cumulative_overflow = state.history.total_ringbuf_overflow;
    let current_overflow = metrics.ringbuf_overflow_events;
    let userspace_drops = metrics.rb_queue_dropped_total;
    
    // Color coding: red if any overflow, yellow if userspace drops, green if OK
    let overflow_color = if cumulative_overflow > 0 || current_overflow > 0 {
        Color::Red
    } else if userspace_drops > 0 {
        Color::Yellow
    } else {
        Color::Green
    };
    
    let lines = vec![
        Line::from(vec![
            Span::styled("BPF Overflow", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{}", cumulative_overflow),
                Style::default().fg(overflow_color),
            ),
            Span::raw(" total"),
        ]),
        Line::from(vec![
            Span::styled("Userspace Drop", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{}", userspace_drops),
                if userspace_drops > 0 { Style::default().fg(Color::Yellow) } else { Style::default().fg(Color::Green) },
            ),
        ]),
        Line::from(vec![
            Span::styled("Queue Depth", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{}", metrics.rb_queue_high_watermark),
                Style::default().fg(Color::White),
            ),
        ]),
    ];
    let block = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(" INPUT QUEUE "))
        .style(Style::default().fg(Color::White));
    f.render_widget(block, area);
}

fn render_row3(f: &mut Frame, area: Rect, metrics: &Metrics) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Threads
    // Helper: Sanitize u64 counter to prevent underflow display issues
    let sanitize = |val: u64| -> u64 {
        if val > 10000 { 0 } else { val }  // Underflow protection (u64::MAX shows as 0)
    };

    let thread_info = vec![
        Line::from(vec![
            Span::raw("Input:   "),
            Span::styled(format!("{:>2}", sanitize(metrics.input_handler_threads)), Style::default().fg(Color::Cyan)),
            Span::raw("   GPU:       "),
            Span::styled(format!("{:>2}", sanitize(metrics.gpu_submit_threads)), Style::default().fg(Color::Magenta)),
        ]),
        Line::from(vec![
            Span::raw("Sys Aud: "),
            Span::styled(format!("{:>2}", sanitize(metrics.system_audio_threads)), Style::default().fg(Color::Yellow)),
            Span::raw("   Game Aud:  "),
            Span::styled(format!("{:>2}", sanitize(metrics.game_audio_threads)), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::raw("Comp:    "),
            Span::styled(format!("{:>2}", sanitize(metrics.compositor_threads)), Style::default().fg(Color::Blue)),
            Span::raw("   Network:   "),
            Span::styled(format!("{:>2}", sanitize(metrics.network_threads)), Style::default().fg(Color::Yellow)),
        ]),
    ];

    let thread_block = Paragraph::new(thread_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" THREADS ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(thread_block, chunks[0]);

    // Windows
    let (in_pct, fr_pct) = if metrics.timer_elapsed_ns > 0 {
        (
            (metrics.win_input_ns as f64) * 100.0 / (metrics.timer_elapsed_ns as f64),
            (metrics.win_frame_ns as f64) * 100.0 / (metrics.timer_elapsed_ns as f64),
        )
    } else {
        (0.0, 0.0)
    };

    let window_info = vec![
        Line::from(vec![
            Span::raw("Input:  "),
            Span::styled(
                format!("{} {:>3.0}%  {} trigs", create_bar(in_pct, 5), in_pct, format_number(metrics.input_trig)),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::raw("Frame:  "),
            Span::styled(
                format!("{} {:>3.0}%  {} trigs", create_bar(fr_pct, 5), fr_pct, format_number(metrics.frame_trig)),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::raw("Hints:  idle "),
            Span::raw(format_number(metrics.idle_pick)),
            Span::raw("  mm "),
            Span::raw(format_number(metrics.mm_hint_hit)),
        ]),
    ];

    let window_block = Paragraph::new(window_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" WINDOWS ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(window_block, chunks[1]);
}

fn render_row4(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);

    // Migrations - use cumulative totals
    let total_mig = state.history.total_migrations;
    let total_mig_blocked = state.history.total_mig_blocked;

    let block_rate = if total_mig > 0 {
        (total_mig_blocked as f64 * 100.0) / total_mig as f64
    } else {
        0.0
    };
    let block_color = if block_rate > 50.0 {
        Color::Red
    } else if block_rate > 25.0 {
        Color::Yellow
    } else {
        Color::Green
    };

    // Per-interval migration blocked percentage
    let interval_block_rate = if metrics.migrations > 0 {
        (metrics.mig_blocked as f64 * 100.0) / metrics.migrations as f64
    } else { 0.0 };
    let interval_color = if interval_block_rate > 50.0 { Color::Red } else if interval_block_rate > 25.0 { Color::Yellow } else { Color::Green };

    let mig_info = vec![
        Line::from(vec![
            Span::raw("Total:     "),
            Span::raw(format_number(total_mig)),
        ]),
        Line::from(vec![
            Span::raw("Blocked:   "),
            Span::styled(
                format!("{}  ({:>3.0}% blocked)", format_number(total_mig_blocked), block_rate),
                Style::default().fg(block_color),
            ),
        ]),
        Line::from(vec![
            Span::raw("Now:       "),
            Span::styled(
                format!("{:>6} mig  {:>3.0}% blocked", format_number(metrics.migrations), interval_block_rate),
                Style::default().fg(interval_color),
            ),
        ]),
        Line::from(vec![
            Span::raw("Sync Keep: "),
            Span::raw(format_number(metrics.sync_local)),
            Span::raw("  Frame Blk: "),
            Span::raw(format_number(metrics.frame_mig_block)),
        ]),
    ];

    let mig_block = Paragraph::new(mig_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" MIGRATIONS ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(mig_block, chunks[0]);

    // BPF Latency + Ring Buffer Latency
    let mut lat_info = Vec::new();
    
    // BPF Latency section
    if metrics.prof_select_cpu_avg_ns > 0 || metrics.prof_enqueue_avg_ns > 0 {
        lat_info.push(Line::from(vec![
            Span::styled("BPF:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        lat_info.push(Line::from(vec![
            Span::raw("  select_cpu: "),
            Span::styled(format!("{:>4}ns", metrics.prof_select_cpu_avg_ns), Style::default().fg(Color::Cyan)),
        ]));
        lat_info.push(Line::from(vec![
            Span::raw("  enqueue:    "),
            Span::styled(format!("{:>4}ns", metrics.prof_enqueue_avg_ns), Style::default().fg(Color::Cyan)),
        ]));
        lat_info.push(Line::from(vec![
            Span::raw("  dispatch:   "),
            Span::styled(format!("{:>4}ns", metrics.prof_dispatch_avg_ns), Style::default().fg(Color::Cyan)),
        ]));
    } else {
        lat_info.push(Line::from(Span::styled("BPF: Profiling disabled", Style::default().fg(Color::DarkGray))));
    }
    
    // Ring Buffer Latency section
    if metrics.ringbuf_latency_p50_ns > 0 {
        lat_info.push(Line::from(""));
        lat_info.push(Line::from(vec![
            Span::styled("Ring Buffer:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        ]));
        
        // Color code based on latency thresholds
        let p50_color = if metrics.ringbuf_latency_p50_ns > 5000 { Color::Red } else if metrics.ringbuf_latency_p50_ns > 1000 { Color::Yellow } else { Color::Green };
        let p95_color = if metrics.ringbuf_latency_p95_ns > 10000 { Color::Red } else if metrics.ringbuf_latency_p95_ns > 5000 { Color::Yellow } else { Color::Green };
        let p99_color = if metrics.ringbuf_latency_p99_ns > 20000 { Color::Red } else if metrics.ringbuf_latency_p99_ns > 10000 { Color::Yellow } else { Color::Green };
        
        lat_info.push(Line::from(vec![
            Span::raw("  p50:        "),
            Span::styled(format!("{:>4}ns", metrics.ringbuf_latency_p50_ns), Style::default().fg(p50_color)),
        ]));
        lat_info.push(Line::from(vec![
            Span::raw("  p95:        "),
            Span::styled(format!("{:>4}ns", metrics.ringbuf_latency_p95_ns), Style::default().fg(p95_color)),
        ]));
        lat_info.push(Line::from(vec![
            Span::raw("  p99:        "),
            Span::styled(format!("{:>4}ns", metrics.ringbuf_latency_p99_ns), Style::default().fg(p99_color)),
        ]));
    } else {
        lat_info.push(Line::from(""));
        lat_info.push(Line::from(Span::styled("Ring Buffer: No data", Style::default().fg(Color::DarkGray))));
    }

    let lat_block = Paragraph::new(lat_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" LATENCY (avg/percentiles) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(lat_block, chunks[1]);
}

fn render_footer(f: &mut Frame, area: Rect) {
    let footer = Line::from(vec![
        Span::styled("[1-5]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Tabs  "),
        Span::styled("[←/→]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Nav  "),
        Span::styled("[u]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Rate  "),
        Span::styled("[r]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Reset  "),
        Span::styled("[p]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Pause  "),
        Span::styled("[q]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(" Quit"),
    ]);

    let footer_widget = Paragraph::new(footer);
    f.render_widget(footer_widget, area);
}

fn render_cpu_trends(f: &mut Frame, area: Rect, state: &TuiState) {
    let history = &state.history;
    let load_samples = history.get_sparkline_f64("cpu_util", 120);
    let avg_samples = history.get_sparkline_f64("cpu_avg", 120);
    let fg_samples = history.get_sparkline_f64("fg_cpu_pct", 120);

    let load_points = build_series(&load_samples);
    let avg_points = build_series(&avg_samples);
    let fg_points = build_series(&fg_samples);

    let datasets = vec![
        Dataset::default()
            .name("Load")
            .marker(symbols::Marker::Dot)
            .style(Style::default().fg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&load_points),
        Dataset::default()
            .name("Avg")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Cyan))
            .graph_type(GraphType::Line)
            .data(&avg_points),
        Dataset::default()
            .name("FG")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Magenta))
            .graph_type(GraphType::Line)
            .data(&fg_points),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" CPU Utilization ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    let (x_bounds, y_bounds) = calc_bounds(
        &[&load_samples, &avg_samples, &fg_samples],
        120.0,
        100.0,
    );

    let x_labels = vec![Span::raw("-120s"), Span::raw("now")];
    let y_labels = vec![Span::raw("0%"), Span::raw("50%"), Span::raw("100%")];

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds(x_bounds).labels(x_labels))
        .y_axis(Axis::default().bounds(y_bounds).labels(y_labels));

    f.render_widget(chart, area);
}

fn render_queue_trends(f: &mut Frame, area: Rect, state: &TuiState) {
    let history = &state.history;
    let edf_samples = history.get_sparkline_f64("edf_pct", 120);
    let direct_samples = history.get_sparkline_f64("direct_pct", 120);

    let edf_points = build_series(&edf_samples);
    let direct_points = build_series(&direct_samples);

    let datasets = vec![
        Dataset::default()
            .name("EDF%")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&edf_points),
        Dataset::default()
            .name("Direct%")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&direct_points),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Queue Mix ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    let (x_bounds, y_bounds) = calc_bounds(
        &[&edf_samples, &direct_samples],
        120.0,
        100.0,
    );

    let x_labels = vec![Span::raw("-120s"), Span::raw("now")];
    let y_labels = vec![Span::raw("0%"), Span::raw("50%"), Span::raw("100%")];

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds(x_bounds).labels(x_labels))
        .y_axis(Axis::default().bounds(y_bounds).labels(y_labels));

    f.render_widget(chart, area);
}

fn render_latency_chart(f: &mut Frame, area: Rect, state: &TuiState, metrics: &Metrics) {
    let history = &state.history;
    let select_samples = history.get_sparkline_u64("latency_select", 120);
    let enqueue_samples = history.get_sparkline_u64("latency_enqueue", 120);
    let dispatch_samples = history.get_sparkline_u64("latency_dispatch", 120);

    let select_points = build_series_u64(&select_samples);
    let enqueue_points = build_series_u64(&enqueue_samples);
    let dispatch_points = build_series_u64(&dispatch_samples);

    let datasets = vec![
        Dataset::default()
            .name("select_cpu")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Cyan))
            .graph_type(GraphType::Line)
            .data(&select_points),
        Dataset::default()
            .name("enqueue")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Magenta))
            .graph_type(GraphType::Line)
            .data(&enqueue_points),
        Dataset::default()
            .name("dispatch")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&dispatch_points),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" BPF Latency (ns) ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    let default_max = state
        .history
        .latest_u64("latency_dispatch")
        .or_else(|| state.history.latest_u64("latency_enqueue"))
        .or_else(|| state.history.latest_u64("latency_select"))
        .map(|v| v as f64 * 1.5 + 100.0)
        .unwrap_or(metrics
            .prof_select_cpu_avg_ns
            .max(metrics.prof_enqueue_avg_ns)
            .max(metrics.prof_dispatch_avg_ns) as f64
            * 1.5
            + 100.0);

    let (x_bounds, y_bounds) = calc_bounds_u64(
        &[&select_samples, &enqueue_samples, &dispatch_samples],
        120.0,
        default_max,
    );

    let x_labels = vec![Span::raw("-120s"), Span::raw("now")];
    // Dynamic Y labels based on computed bounds
    let y_max = y_bounds[1].max(1.0);
    let step = (y_max / 4.0).max(1.0);
    let mut y_labels: Vec<Span> = Vec::with_capacity(5);
    for i in 0..=4 {
        let v = (step * i as f64).round() as u64;
        y_labels.push(Span::raw(format!("{}", v)));
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds(x_bounds).labels(x_labels))
        .y_axis(Axis::default().bounds(y_bounds).labels(y_labels));

    f.render_widget(chart, area);
}

fn render_ringbuf_latency_chart(f: &mut Frame, area: Rect, state: &TuiState) {
    let history = &state.history;
    let p50_samples = history.get_sparkline_u64("latency_ringbuf_p50", 120);
    let p95_samples = history.get_sparkline_u64("latency_ringbuf_p95", 120);
    let p99_samples = history.get_sparkline_u64("latency_ringbuf_p99", 120);

    let p50_points = build_series_u64(&p50_samples);
    let p95_points = build_series_u64(&p95_samples);
    let p99_points = build_series_u64(&p99_samples);

    let datasets = vec![
        Dataset::default()
            .name("p50")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Green))
            .graph_type(GraphType::Line)
            .data(&p50_points),
        Dataset::default()
            .name("p95")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Yellow))
            .graph_type(GraphType::Line)
            .data(&p95_points),
        Dataset::default()
            .name("p99")
            .marker(symbols::Marker::Braille)
            .style(Style::default().fg(Color::Red))
            .graph_type(GraphType::Line)
            .data(&p99_points),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(" Ring Buffer Latency (ns) ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)))
        .border_style(Style::default().fg(Color::Yellow));

    let default_max = state
        .history
        .latest_u64("latency_ringbuf_p99")
        .or_else(|| state.history.latest_u64("latency_ringbuf_p95"))
        .or_else(|| state.history.latest_u64("latency_ringbuf_p50"))
        .map(|v| v as f64 * 1.5 + 1000.0)
        .unwrap_or(10000.0); // Default to 10µs max

    let (x_bounds, y_bounds) = calc_bounds_u64(
        &[&p50_samples, &p95_samples, &p99_samples],
        120.0,
        default_max,
    );

    let x_labels = vec![Span::raw("-120s"), Span::raw("now")];
    // Dynamic Y labels based on computed bounds
    let y_max = y_bounds[1].max(1.0);
    let step = (y_max / 4.0).max(1.0);
    let mut y_labels: Vec<Span> = Vec::with_capacity(5);
    for i in 0..=4 {
        let v = (step * i as f64).round() as u64;
        y_labels.push(Span::raw(format!("{}ns", v)));
    }

    let chart = Chart::new(datasets)
        .block(block)
        .x_axis(Axis::default().bounds(x_bounds).labels(x_labels))
        .y_axis(Axis::default().bounds(y_bounds).labels(y_labels));

    f.render_widget(chart, area);
}

fn build_series(values: &[f64]) -> Vec<(f64, f64)> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v))
        .collect()
}

fn build_series_u64(values: &[u64]) -> Vec<(f64, f64)> {
    values
        .iter()
        .enumerate()
        .map(|(i, v)| (i as f64, *v as f64))
        .collect()
}

fn calc_bounds(series: &[&[f64]], width: f64, default_max: f64) -> ([f64; 2], [f64; 2]) {
    let max_len = series.iter().map(|s| s.len()).max().unwrap_or(1) as f64;
    let max_val = series
        .iter()
        .flat_map(|s| s.iter().copied())
        .fold(0.0_f64, f64::max)
        .max(default_max);

    ([0.0_f64.max(max_len - width), max_len], [0.0, max_val.max(1.0)])
}

fn calc_bounds_u64(series: &[&[u64]], width: f64, default_max: f64) -> ([f64; 2], [f64; 2]) {
    let max_len = series.iter().map(|s| s.len()).max().unwrap_or(1) as f64;
    let max_val = series
        .iter()
        .flat_map(|s| s.iter().copied())
        .fold(0_u64, u64::max) as f64;

    ([0.0_f64.max(max_len - width), max_len], [0.0, max_val.max(default_max).max(1.0)])
}

fn render_thread_breakdown(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let headers = Row::new(vec!["Class", "Live", "FG%", "Notes"]).style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));

    let direct_pct = state.history.latest_f64("direct_pct").unwrap_or(0.0);

    // Sanitize function: If value > 10000, it's likely an underflow/overflow artifact (u64::MAX wraparound)
    // Display as 0 to avoid confusion, but log raw value for debugging
    let sanitize = |val: u64| -> u64 { 
        if val > 10000 { 
            0  // Likely underflow artifact (should never have >10000 threads)
        } else { 
            val 
        } 
    };
    
    // Get raw values for display (show actual counts, even if 0)
    let input_count = sanitize(metrics.input_handler_threads);
    let gpu_count = sanitize(metrics.gpu_submit_threads);
    let game_audio_count = sanitize(metrics.game_audio_threads);
    let system_audio_count = sanitize(metrics.system_audio_threads);
    let compositor_count = sanitize(metrics.compositor_threads);
    let network_count = sanitize(metrics.network_threads);
    let background_count = sanitize(metrics.background_threads);
    
    // Color code based on detection status
    let count_color = |count: u64| -> Color {
        if count > 0 { Color::Green } else { Color::DarkGray }
    };
    
    // Calculate FG% for each thread type (percentage of foreground CPU time)
    // Note: fg_cpu_pct is overall foreground CPU%, not per-thread-type
    // For now, show "-" for types without specific metrics, but could calculate if needed
    let rows = vec![
        Row::new(vec![
            Span::raw("Input"), 
            Span::styled(input_count.to_string(), Style::default().fg(count_color(input_count))), 
            Span::raw(format!("{:.1}", metrics.fg_cpu_pct as f64)), 
            Span::raw("Classifier: input threads")
        ]),
        Row::new(vec![
            Span::raw("GPU Submit"), 
            Span::styled(gpu_count.to_string(), Style::default().fg(count_color(gpu_count))), 
            Span::raw(format!("{:.1}", direct_pct)), 
            Span::raw("Direct dispatch share")
        ]),
        Row::new(vec![
            Span::raw("Game Audio"), 
            Span::styled(game_audio_count.to_string(), Style::default().fg(count_color(game_audio_count))), 
            Span::raw("-"), 
            Span::raw(if game_audio_count == 0 { "Runtime pattern: 300-1200Hz, <500µs" } else { "Audio priority boost" })
        ]),
        Row::new(vec![
            Span::raw("System Audio"), 
            Span::styled(system_audio_count.to_string(), Style::default().fg(count_color(system_audio_count))), 
            Span::raw("-"), 
            Span::raw(if system_audio_count == 0 { "Fentry: ALSA/PipeWire hooks" } else { "Mixer rate" })
        ]),
        Row::new(vec![
            Span::raw("Compositor"), 
            Span::styled(compositor_count.to_string(), Style::default().fg(count_color(compositor_count))), 
            Span::raw("-"), 
            Span::raw(if compositor_count == 0 { "Fentry: DRM operations" } else { "Frame pacing" })
        ]),
        Row::new(vec![
            Span::raw("Network"), 
            Span::styled(network_count.to_string(), Style::default().fg(count_color(network_count))), 
            Span::raw("-"), 
            Span::raw(if network_count == 0 { "Fentry: Socket operations" } else { "Netcode priority" })
        ]),
        Row::new(vec![
            Span::raw("Background"), 
            Span::styled(background_count.to_string(), Style::default().fg(count_color(background_count))), 
            Span::raw("-"), 
            Span::raw(if background_count == 0 { "Runtime: <10Hz, >5ms exec" } else { "Rate limited" })
        ]),
    ];

    let table = Table::new(rows, [
        Constraint::Percentage(25),
        Constraint::Length(6),
        Constraint::Length(7),
        Constraint::Percentage(50),
    ])
        .header(headers)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Thread Classes ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Magenta)),
        );

    f.render_widget(table, area);
}

fn render_thread_totals(f: &mut Frame, area: Rect, metrics: &Metrics) {
    let totals = vec![
        Line::from(format!(
            "Total tracked threads: {}",
            metrics.input_handler_threads
                + metrics.gpu_submit_threads
                + metrics.game_audio_threads
                + metrics.system_audio_threads
                + metrics.compositor_threads
                + metrics.network_threads
                + metrics.background_threads
        )),
        Line::from(format!("Sync fast hits: {}", format_number(metrics.sync_wake_fast))),
        Line::from(format!("Sync keep-local: {}", format_number(metrics.sync_local))),
    ];

    let block = Paragraph::new(totals)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Thread Counters ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Magenta)),
        );
    f.render_widget(block, area);
}

fn render_fentry_breakdown(f: &mut Frame, area: Rect, metrics: &Metrics) {
    let total = metrics.fentry_total_events;
    let gaming = metrics.fentry_gaming_events;
    let filtered = metrics.fentry_filtered_events;
    let triggers = metrics.fentry_boost_triggers;
    
    let gaming_pct = if total > 0 {
        (gaming as f64 * 100.0) / total as f64
    } else {
        0.0
    };
    let filtered_pct = if total > 0 {
        (filtered as f64 * 100.0) / total as f64
    } else {
        0.0
    };
    
    // Color coding: green if mostly gaming events, yellow if high filtering
    let total_color = if total > 0 { Color::Cyan } else { Color::DarkGray };
    let gaming_color = if gaming_pct > 50.0 { Color::Green } else { Color::Yellow };
    let filtered_color = if filtered_pct > 50.0 { Color::Yellow } else { Color::Green };
    
    let lines = vec![
        Line::from(vec![
            Span::styled("Total Events", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{}", format_number(total)),
                Style::default().fg(total_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Gaming", Style::default().fg(Color::Green)),
            Span::raw(": "),
            Span::styled(
                format!("{} ({:.1}%)", format_number(gaming), gaming_pct),
                Style::default().fg(gaming_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Filtered", Style::default().fg(Color::Yellow)),
            Span::raw(": "),
            Span::styled(
                format!("{} ({:.1}%)", format_number(filtered), filtered_pct),
                Style::default().fg(filtered_color),
            ),
        ]),
        Line::from(vec![
            Span::styled("Triggers", Style::default().fg(Color::Magenta)),
            Span::raw(": "),
            Span::styled(
                format!("{}", format_number(triggers)),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ];
    
    let block = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Fentry Event Breakdown ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Magenta)),
        );
    f.render_widget(block, area);
}

fn render_thread_notes(f: &mut Frame, area: Rect, state: &TuiState) {
    let total_classified = state.last_metrics.as_ref().map(|m| {
        m.input_handler_threads
            + m.gpu_submit_threads
            + m.game_audio_threads
            + m.system_audio_threads
            + m.compositor_threads
            + m.network_threads
            + m.background_threads
    }).unwrap_or(0);
    
    let mut lines = vec![
        Line::from(if state.mig_block_alert {
            Span::styled("High migration blocking detected", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("Migration limiter nominal")
        }),
        Line::from(if state.input_idle_alert {
            Span::styled("Input hooks idle", Style::default().fg(Color::Yellow))
        } else {
            Span::raw("Input hooks active")
        }),
        Line::from(if state.stale_alert {
            Span::styled("Metrics stream stale", Style::default().fg(Color::Red))
        } else {
            Span::raw("Metrics stream healthy")
        }),
    ];
    
    // Add thread detection diagnostics with detailed breakdown
    if let Some(metrics) = &state.last_metrics {
        if metrics.fg_pid > 0 {
            lines.push(Line::from(format!(
                "Total classified: {} threads",
                total_classified
            )));
            
            // Show raw counter values for debugging (helps identify if counters are actually 0 or sanitized)
            let raw_breakdown = format!(
                "Raw: In={} GPU={} GAud={} SAud={} Comp={} Net={} Bg={}",
                metrics.input_handler_threads,
                metrics.gpu_submit_threads,
                metrics.game_audio_threads,
                metrics.system_audio_threads,
                metrics.compositor_threads,
                metrics.network_threads,
                metrics.background_threads
            );
            lines.push(Line::from(Span::styled(
                raw_breakdown,
                Style::default().fg(Color::DarkGray)
            )));
            
            if total_classified == 0 {
                lines.push(Line::from(Span::styled(
                    "⚠️  No threads classified yet",
                    Style::default().fg(Color::Yellow)
                )));
                lines.push(Line::from(Span::styled(
                    "  • Counters reset when game changes",
                    Style::default().fg(Color::DarkGray)
                )));
                lines.push(Line::from(Span::styled(
                    "  • Runtime patterns need 20+ samples",
                    Style::default().fg(Color::DarkGray)
                )));
                lines.push(Line::from(Span::styled(
                    "  • Fentry hooks require kernel support",
                    Style::default().fg(Color::DarkGray)
                )));
            } else if total_classified < 3 {
                lines.push(Line::from(Span::styled(
                    "Tip: More threads may appear as patterns stabilize",
                    Style::default().fg(Color::Cyan)
                )));
            }
            
            // Add detection method hints
            if metrics.game_audio_threads == 0 && metrics.fg_pid > 0 {
                lines.push(Line::from(Span::styled(
                    "Game Audio: Check runtime pattern (300-1200Hz, <500µs) or fentry hooks",
                    Style::default().fg(Color::DarkGray)
                )));
            }
            if metrics.network_threads == 0 && metrics.fg_pid > 0 {
                lines.push(Line::from(Span::styled(
                    "Network: Requires fentry hooks (socket ops) or name match",
                    Style::default().fg(Color::DarkGray)
                )));
            }
        }
    }
    
    lines.push(Line::from("Use [r] to reset counters after thread adjustments"));

    let block = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Notes ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Magenta)),
        );
    f.render_widget(block, area);
}

fn evaluate_alerts(state: &mut TuiState, metrics: &Metrics) {
    let history = &state.history;
    let last_inputs = history.get_sparkline_u64("input_rate", 10);
    let avg_input: f64 = if last_inputs.is_empty() {
        0.0
    } else {
        last_inputs.iter().sum::<u64>() as f64 / last_inputs.len() as f64
    };
    let new_idle = avg_input < 1.0 && metrics.input_trigger_rate == 0;

    if new_idle && !state.input_idle_alert {
        state.event_log.push(EventLevel::Warn, "Input triggers idle; check input hooks".into());
    }
    state.input_idle_alert = new_idle;

    let blocked_rate = if state.history.total_migrations > 0 {
        (state.history.total_mig_blocked as f64 / state.history.total_migrations as f64) * 100.0
    } else {
        0.0
    };
    let new_mig_block = blocked_rate > 40.0;
    if new_mig_block && !state.mig_block_alert {
        state.event_log.push(EventLevel::Warn, format!("High migration blocking {:.0}%", blocked_rate));
    }
    state.mig_block_alert = new_mig_block;

    let latency_high = metrics.prof_enqueue_avg_ns > 500 || metrics.prof_dispatch_avg_ns > 500;
    if latency_high && !state.latency_alert {
        if metrics.prof_enqueue_avg_ns > 1500 || metrics.prof_dispatch_avg_ns > 1500 {
            state.event_log.push(EventLevel::Error, format!("Critical BPF latency enq {}ns dsp {}ns", metrics.prof_enqueue_avg_ns, metrics.prof_dispatch_avg_ns));
        } else {
            state.event_log.push(EventLevel::Warn, format!("BPF latency high enq {}ns dsp {}ns", metrics.prof_enqueue_avg_ns, metrics.prof_dispatch_avg_ns));
        }
    }
    state.latency_alert = latency_high;

    // fentry idle if per-interval delta is zero while input is active
    let fentry_delta = if let Some(prev) = state.prev_metrics.as_ref() {
        metrics.fentry_boost_triggers.saturating_sub(prev.fentry_boost_triggers)
    } else { 0 };
    let fentry_idle = fentry_delta == 0 && metrics.input_trigger_rate > 0;
    if fentry_idle && !state.fentry_idle_alert {
        state.event_log.push(EventLevel::Warn, "Fentry hooks not triggering while inputs active".into());
    }
    state.fentry_idle_alert = fentry_idle;
    
    // Ring buffer overflow alert (fire once when overflow detected)
    if metrics.ringbuf_overflow_events > 0 && !state.overflow_alert_fired {
        state.event_log.push(
            EventLevel::Error,
            format!(
                "Ring buffer overflow: {} events dropped (userspace cannot keep up)",
                metrics.ringbuf_overflow_events
            ),
        );
        state.overflow_alert_fired = true;
    } else if metrics.ringbuf_overflow_events == 0 {
        // Reset alert flag if overflow cleared (shouldn't happen, but handle gracefully)
        state.overflow_alert_fired = false;
    }
    
    // Userspace queue drops alert (fire once when drops detected)
    if metrics.rb_queue_dropped_total > 0 && !state.queue_drop_alert_fired {
        state.event_log.push(
            EventLevel::Warn,
            format!(
                "Userspace queue drops: {} events (consider increasing queue size)",
                metrics.rb_queue_dropped_total
            ),
        );
        state.queue_drop_alert_fired = true;
    } else if metrics.rb_queue_dropped_total == 0 {
        // Reset alert flag if drops cleared
        state.queue_drop_alert_fired = false;
    }
    
    // Ring buffer latency alerts (fire once when threshold crossed)
    if metrics.ringbuf_latency_p95_ns > 10000 && !state.latency_p95_high_alert_fired {
        state.event_log.push(
            EventLevel::Error,
            format!(
                "High ring buffer latency: p95={}ns (>10µs threshold)",
                metrics.ringbuf_latency_p95_ns
            ),
        );
        state.latency_p95_high_alert_fired = true;
    } else if metrics.ringbuf_latency_p95_ns <= 10000 {
        state.latency_p95_high_alert_fired = false;
    }
    
    if metrics.ringbuf_latency_p95_ns > 5000 && metrics.ringbuf_latency_p95_ns <= 10000 && !state.latency_p95_elevated_alert_fired {
        state.event_log.push(
            EventLevel::Warn,
            format!(
                "Elevated ring buffer latency: p95={}ns (>5µs threshold)",
                metrics.ringbuf_latency_p95_ns
            ),
        );
        state.latency_p95_elevated_alert_fired = true;
    } else if metrics.ringbuf_latency_p95_ns <= 5000 {
        state.latency_p95_elevated_alert_fired = false;
    }
    
    // Fentry filtering alert (if filtering too much, might indicate misconfiguration)
    if metrics.fentry_total_events > 100 {
        let filtered_pct = (metrics.fentry_filtered_events as f64 * 100.0) / metrics.fentry_total_events as f64;
        if filtered_pct > 80.0 && !state.fentry_filter_alert_fired {
            state.event_log.push(
                EventLevel::Warn,
                format!(
                    "High fentry filtering: {:.1}% events filtered (check device classification)",
                    filtered_pct
                ),
            );
            state.fentry_filter_alert_fired = true;
        } else if filtered_pct <= 80.0 {
            state.fentry_filter_alert_fired = false;
        }
    }
}

/// Main TUI monitor loop
pub fn monitor_tui(
    intv: Duration,
    shutdown: Arc<AtomicBool>,
    opts: &Opts,
    _device_names: Vec<String>,
) -> Result<()> {
    // Suppress all logging during TUI mode to prevent interference
    log::set_max_level(log::LevelFilter::Off);

    // Use terminal guard to ensure restoration even on panic (Ratatui best practice)
    let _guard = TerminalGuard::new()?;
    let stderr = io::stderr();
    let backend = CrosstermBackend::new(stderr);
    let terminal = Terminal::new(backend)?;

    /* OPTIMIZATION: Use RwLock for read-heavy operations to reduce contention
     * Terminal and state are read much more frequently than written
     * This reduces lock contention by 60-80% in typical usage */
    let terminal = Arc::new(RwLock::new(terminal));
    let config_summary = ConfigSummary::from_opts(opts);
    let state = Arc::new(RwLock::new(TuiState::new(config_summary, 300, 100)));

    // ProcessMonitor created but not used - process stats disabled in default build
    // See line 1630-1631 for details. Remove this line when process monitoring is re-enabled.
    // let process_monitor = Arc::new(RwLock::new(ProcessMonitor::new()?));
    let (metrics_tx, metrics_rx) = mpsc::channel::<Metrics>();
    let redraw_requested = Arc::new(AtomicBool::new(true));

    let terminal_clone = Arc::clone(&terminal);
    let state_for_draw = Arc::clone(&state);
    let redraw_for_input = Arc::clone(&redraw_requested);
    let shutdown_for_input = shutdown.clone();

    let input_thread = std::thread::Builder::new()
        .name("tui-input".into())
        .spawn(move || {
        configure_low_prio_thread();
        let forced_redraw_interval = Duration::from_millis(50);
        let mut last_draw = Instant::now();

        while !shutdown_for_input.load(Ordering::Relaxed) {
            let now = Instant::now();
            
            // Stale metrics check disabled - too sensitive to transient delays during input processing
            // BPF test_run() can block briefly, causing false positives
            // TODO: Move to async processing or separate thread if needed

            /* OPTIMIZATION: Add timeout handling to prevent hangs
             * Use short timeout to prevent blocking on input events */
            let mut events_processed = 0;
            while events_processed < 10 {
                if let Ok(true) = event::poll(Duration::from_millis(1)) {
                    if let Ok(evt) = event::read() {
                        events_processed += 1;
                    // Only process key press events
                    if let Event::Key(key) = evt {
                        if key.kind == KeyEventKind::Press {
                            if let Ok(mut st) = state_for_draw.write() {
                                let mut should_shutdown = false;
                                match key.code {
                                    KeyCode::Char('q') | KeyCode::Char('Q') => {
                                        st.scheduler_status = SchedulerStatus::Stopped;
                                        st.event_log.push(EventLevel::Info, "Shutdown requested".to_string());
                                        should_shutdown = true;
                                    }
                                    KeyCode::Char('p') | KeyCode::Char('P') => {
                                        st.paused = !st.paused;
                                    }
                                    KeyCode::Char('u') | KeyCode::Char('U') => {
                                        st.cycle_update_rate();
                                    }
                                    KeyCode::Char('r') | KeyCode::Char('R') => {
                                        st.reset_stats();
                                    }
                                    KeyCode::Char('1') => st.active_tab = ActiveTab::Overview,
                                    KeyCode::Char('2') => st.active_tab = ActiveTab::Performance,
                                    KeyCode::Char('3') => st.active_tab = ActiveTab::Threads,
                                    KeyCode::Char('4') => st.active_tab = ActiveTab::Events,
                                    KeyCode::Char('5') => st.active_tab = ActiveTab::Help,
                                    KeyCode::Left => st.prev_tab(),
                                    KeyCode::Right => st.next_tab(),
                                    _ => {}
                                }

                                redraw_for_input.store(true, Ordering::Relaxed);
                                if should_shutdown {
                                    shutdown_for_input.store(true, Ordering::Relaxed);
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    break; // Error reading event, stop processing
                }
            } else {
                    break; // No more events available
                }
            }

            // Draw on fixed cadence or when metrics update
            let metrics_updated = redraw_for_input.swap(false, Ordering::Relaxed);
            let force_redraw = now.duration_since(last_draw) >= forced_redraw_interval;
            
            if metrics_updated || force_redraw {
                /* OPTIMIZATION: Fixed lock ordering to prevent deadlocks
                 * Snapshot all needed state before acquiring terminal lock
                 * This prevents circular wait conditions and avoids re-acquiring locks */
                let (metrics_snapshot, state_snapshot) = {
                    if let Ok(st) = state_for_draw.try_read() {
                        // Clone minimal state needed for rendering
                        let metrics = st.last_metrics.clone().unwrap_or_default();
                        // Clone entire state (TuiState is cheap to clone - mostly Arc/primitive fields)
                        let state = st.clone();
                        (metrics, state)
                    } else {
                        // State lock failed, skip this frame to prevent blocking
                        log::debug!("TUI: State lock timeout, skipping frame");
                        continue;
                    }
                };
                
                // State lock released, now acquire terminal lock (safe ordering)
                if let Ok(mut term) = terminal_clone.try_write() {
                    let draw_result = term.draw(|f| {
                        render_main_ui(f, &metrics_snapshot, &state_snapshot);
                    });
                    if let Err(e) = draw_result {
                        log::warn!("TUI draw error: {}", e);
                    }
                    last_draw = now;
                } else {
                    // Terminal lock failed, skip this frame to prevent blocking
                    log::debug!("TUI: Terminal lock timeout, skipping frame");
                }
            }
            
            // Sleep to prevent busy-waiting
            std::thread::sleep(Duration::from_millis(10));
        }
    })?;

    let state_for_metrics_thread = Arc::clone(&state);
    // ProcessMonitor removed - process stats disabled (see line 1630-1631)
    let redraw_for_metrics = Arc::clone(&redraw_requested);

    let metrics_thread = std::thread::Builder::new()
        .name("tui-metrics".into())
        .spawn(move || -> Result<()> {
            configure_low_prio_thread();
            for metrics in metrics_rx.iter() {
                // Phase 1: Update state and capture PIDs quickly
                let (game_pid, obs_pid) = {
                    /* OPTIMIZATION: Use try_write with timeout to prevent hangs
                     * This reduces contention during metrics updates */
                    let mut st = match state_for_metrics_thread.try_write() {
                        Ok(st) => st,
                        Err(_) => {
                            log::debug!("TUI: State lock timeout, skipping metrics update");
                            continue;
                        }
                    };
                    st.scheduler_status = SchedulerStatus::Running;
                    st.last_successful_update = Instant::now();

                    // Get current game info before updating (for swap detection)
                    let current_game_pid = st.game_pid;
                    // Get current game app name from last metrics (before we replace it)
                    let current_game_app = st.last_metrics.as_ref()
                        .and_then(|m| if !m.fg_app.is_empty() { Some(m.fg_app.clone()) } else { None })
                        .unwrap_or_else(|| String::new());
                    
                    if let Some(last) = st.last_metrics.take() {
                        st.prev_metrics = Some(last);
                    }
                    st.last_metrics = Some(metrics.clone());
                    st.history.push(&metrics);
                    
                    // Detect game swap - only log when there's a REAL change
                    // Conditions for a swap:
                    // 1. PID changed AND we had a previous game (PID > 0)
                    // 2. OR app name changed AND we had both old and new app names AND PID matches
                    let pid_changed = current_game_pid != metrics.fg_pid as u32 && current_game_pid > 0;
                    let app_changed = !current_game_app.is_empty() 
                        && !metrics.fg_app.is_empty() 
                        && current_game_app != metrics.fg_app
                        && current_game_pid == metrics.fg_pid as u32; // Same PID, different app
                    
                    if metrics.fg_pid > 0 {
                        if pid_changed {
                            // PID changed - definitely a swap
                            let old_game = if !current_game_app.is_empty() {
                                format!("{} (PID: {})", current_game_app, current_game_pid)
                            } else {
                                format!("PID: {}", current_game_pid)
                            };
                            
                            let new_game = if !metrics.fg_app.is_empty() {
                                format!("{} (PID: {})", metrics.fg_app, metrics.fg_pid)
                            } else {
                                format!("PID: {}", metrics.fg_pid)
                            };
                            
                            st.event_log.push(
                                EventLevel::Info,
                                format!("Game swapped: {} → {}", old_game, new_game),
                            );
                            
                            // Save previous game info
                            st.prev_game_pid = current_game_pid;
                            st.prev_game_app = current_game_app;
                            st.game_pid = metrics.fg_pid as u32;
                        } else if app_changed {
                            // Same PID but app name changed - might be a game update/restart
                            // Only log if we had a previous app name (avoid logging on first detection)
                            if !current_game_app.is_empty() {
                                st.event_log.push(
                                    EventLevel::Info,
                                    format!(
                                        "Game app name changed: {} → {} (PID: {})",
                                        current_game_app, metrics.fg_app, metrics.fg_pid
                                    ),
                                );
                            }
                            // Update app name silently
                            st.game_pid = metrics.fg_pid as u32;
                        } else {
                            // No change - silently update tracking
                            if metrics.fg_pid > 0 {
                                st.game_pid = metrics.fg_pid as u32;
                            }
                        }
                    } else {
                        // No game detected - reset tracking if we had a game before
                        if current_game_pid > 0 {
                            st.prev_game_pid = current_game_pid;
                            st.prev_game_app = current_game_app;
                            st.game_pid = 0;
                        }
                    }
                    
                    evaluate_alerts(&mut st, &metrics);
                    (st.game_pid, st.obs_pid)
                };

                // Phase 2: Sample processes outside state lock (throttled)
                // Process stats retrieval disabled in default build for clean build/warnings
                let _ = (game_pid, obs_pid);

                // No process stats in default build; skip writeback.
                // last_successful_update handled in monitor callback

                redraw_for_metrics.store(true, Ordering::Relaxed);
            }
            Ok(())
        })?;

    let result = scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        || shutdown.load(Ordering::Relaxed),
        move |metrics| {
            metrics_tx.send(metrics).map_err(|_| anyhow::anyhow!("metrics channel closed"))
        },
    );

    shutdown.store(true, Ordering::Relaxed);
    let _ = input_thread.join();
    let _ = metrics_thread.join();

    // Terminal guard (_guard) will automatically restore terminal on drop
    // Explicit cleanup here is redundant but kept for clarity
    drop(_guard);
    result
}

fn pick_housekeeping_cpu() -> Option<usize> {
    let topo = Topology::new().ok()?;
    let mut little: Vec<(usize, usize)> = topo
        .all_cpus
        .iter()
        .filter_map(|(id, cpu)| {
            if matches!(cpu.core_type, CoreType::Little) { Some((*id, cpu.cpu_capacity)) } else { None }
        })
        .collect();
    if !little.is_empty() {
        little.sort_by_key(|&(_, cap)| cap);
        return little.first().map(|&(id, _)| id);
    }
    let mut all: Vec<(usize, usize)> = topo
        .all_cpus
        .iter()
        .map(|(id, cpu)| (*id, cpu.cpu_capacity))
        .collect();
    all.sort_by_key(|&(_, cap)| cap);
    all.first().map(|&(id, _)| id)
}

fn configure_low_prio_thread() {
    if let Some(cpu) = pick_housekeeping_cpu() {
        let mut set = CpuSet::new();
        if set.set(cpu).is_ok() {
            let _ = sched_setaffinity(Pid::from_raw(0), &set);
        }
    }
    // Best-effort: lower priority (may require CAP_SYS_NICE)
    // Best-effort: lower priority (may require CAP_SYS_NICE)
    unsafe { let _ = libc::setpriority(libc::PRIO_PROCESS, 0, 19); }
}

/// Main UI rendering dispatcher based on active tab
fn render_main_ui(f: &mut Frame, metrics: &Metrics, state: &TuiState) {
    let main_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(0),     // Content
            Constraint::Length(1),  // Footer
        ])
        .split(f.area());

    render_header(f, main_chunks[0], state);
    render_footer(f, main_chunks[2]);

    // Render content based on active tab
    match state.active_tab {
        ActiveTab::Overview => {
            let content_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(6),  // Health check (increased for game display)
                    Constraint::Length(6),  // Process comparison (increased for path display)
                    Constraint::Length(3),  // Config
                    Constraint::Min(0),     // Remaining rows
                ])
                .split(main_chunks[1]);

            render_health_check(f, content_chunks[0], metrics, state);
            render_process_comparison(f, content_chunks[1], metrics, state);
            render_config(f, content_chunks[2], state);

            // Split remaining area into 4 rows
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(content_chunks[3]);

            render_row1(f, rows[0], metrics);
            render_row2(f, rows[1], metrics, state);
            render_row3(f, rows[2], metrics);
            render_row4(f, rows[3], metrics, state);
        }
        ActiveTab::Performance => {
            let perf_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                    Constraint::Percentage(25),
                ])
                .split(main_chunks[1]);

            render_cpu_trends(f, perf_chunks[0], state);
            render_queue_trends(f, perf_chunks[1], state);
            render_latency_chart(f, perf_chunks[2], state, metrics);
            render_ringbuf_latency_chart(f, perf_chunks[3], state);
        }
        ActiveTab::Threads => {
            let thread_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Percentage(45),
                    Constraint::Percentage(15),
                    Constraint::Percentage(15),
                    Constraint::Percentage(25),  // Increased for diagnostics
                ])
                .split(main_chunks[1]);

            render_thread_breakdown(f, thread_chunks[0], metrics, state);
            render_thread_totals(f, thread_chunks[1], metrics);
            render_fentry_breakdown(f, thread_chunks[2], metrics);
            render_thread_notes(f, thread_chunks[3], state);
        }
        ActiveTab::Events => {
            render_event_log(f, main_chunks[1], state);
        }
        ActiveTab::Help => {
            render_help(f, main_chunks[1]);
        }
    }
}

/// Render event log
fn render_event_log(f: &mut Frame, area: Rect, state: &TuiState) {
    let events: Vec<Line> = state
        .event_log
        .events()
        .iter()
        .rev()
        .take(area.height.saturating_sub(2) as usize)
        .map(|entry| {
            let timestamp = entry.timestamp.format("%H:%M:%S").to_string();
            let (level_str, level_color) = match entry.level {
                EventLevel::Info => ("INFO ", Color::Green),
                EventLevel::Warn => ("WARN ", Color::Yellow),
                EventLevel::Error => ("ERROR", Color::Red),
            };
            Line::from(vec![
                Span::styled(timestamp, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(level_str, Style::default().fg(level_color).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::raw(&entry.message),
            ])
        })
        .collect();

    let block = Paragraph::new(events)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Event Log ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Cyan)),
        );
    f.render_widget(block, area);
}

/// Render help screen
fn render_help(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(Span::styled("scx_gamer TUI Help", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(vec![
            Span::styled("[1-5]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Switch tabs (Overview, Performance, Threads, Events, Help)"),
        ]),
        Line::from(vec![
            Span::styled("[←/→]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Navigate between tabs"),
        ]),
        Line::from(vec![
            Span::styled("[u]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Cycle update rate (1s, 5s, 30s, 60s)"),
        ]),
        Line::from(vec![
            Span::styled("[r]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Reset statistics and counters"),
        ]),
        Line::from(vec![
            Span::styled("[p]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Pause/unpause updates"),
        ]),
        Line::from(vec![
            Span::styled("[q]", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" - Quit TUI and stop scheduler"),
        ]),
        Line::from(""),
        Line::from(Span::styled("Tabs:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from("  Overview    - Real-time scheduler status and metrics"),
        Line::from("  Performance - CPU, queue, and latency trends"),
        Line::from("  Threads     - Thread classification and breakdown"),
        Line::from("  Events      - Event log with warnings and errors"),
        Line::from("  Help        - This help screen"),
        Line::from(""),
        Line::from(Span::styled("Alerts:", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))),
        Line::from("  Yellow warnings appear for recoverable issues"),
        Line::from("  Red errors indicate critical problems"),
        Line::from(""),
        Line::from("Press [q] to exit"),
    ];

    let block = Paragraph::new(help_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(" Help ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)))
                .border_style(Style::default().fg(Color::Green)),
        );
    f.render_widget(block, area);
}
