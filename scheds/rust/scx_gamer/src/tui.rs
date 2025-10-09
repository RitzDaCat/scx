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
    widgets::{Axis, Block, Borders, Chart, Dataset, GraphType, List, ListItem, Paragraph, Row, Table, Tabs},
    Frame, Terminal,
};
use ratatui::symbols;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::stats::Metrics;
use crate::Opts;
use crate::process_monitor::{ProcessMonitor, ProcessStats};

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
    max_samples: usize,
    cpu_util: VecDeque<f64>,
    cpu_avg: VecDeque<f64>,
    fg_cpu_pct: VecDeque<f64>,
    latency_select: VecDeque<u64>,
    latency_enqueue: VecDeque<u64>,
    latency_dispatch: VecDeque<u64>,
    migrations: VecDeque<u64>,
    mig_blocked: VecDeque<u64>,
    input_rate: VecDeque<u64>,
    direct_pct: VecDeque<f64>,
    edf_pct: VecDeque<f64>,
    timestamps: VecDeque<Instant>,

    // Cumulative totals (sum of deltas for lifetime stats)
    pub total_rr_enq: u64,
    pub total_edf_enq: u64,
    pub total_direct: u64,
    pub total_migrations: u64,
    pub total_mig_blocked: u64,
}

impl HistoricalData {
    pub fn new(max_samples: usize) -> Self {
        Self {
            max_samples,
            cpu_util: VecDeque::with_capacity(max_samples),
            cpu_avg: VecDeque::with_capacity(max_samples),
            fg_cpu_pct: VecDeque::with_capacity(max_samples),
            latency_select: VecDeque::with_capacity(max_samples),
            latency_enqueue: VecDeque::with_capacity(max_samples),
            latency_dispatch: VecDeque::with_capacity(max_samples),
            migrations: VecDeque::with_capacity(max_samples),
            mig_blocked: VecDeque::with_capacity(max_samples),
            input_rate: VecDeque::with_capacity(max_samples),
            direct_pct: VecDeque::with_capacity(max_samples),
            edf_pct: VecDeque::with_capacity(max_samples),
            timestamps: VecDeque::with_capacity(max_samples),

            total_rr_enq: 0,
            total_edf_enq: 0,
            total_direct: 0,
            total_migrations: 0,
            total_mig_blocked: 0,
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

        // Push new data (pop oldest if at capacity)
        macro_rules! push_value {
            ($field:ident, $value:expr) => {
                if self.$field.len() >= self.max_samples {
                    self.$field.pop_front();
                }
                self.$field.push_back($value);
            };
        }

        push_value!(cpu_util, cpu_util_pct);
        push_value!(cpu_avg, cpu_avg_pct);
        push_value!(fg_cpu_pct, fg_cpu);
        push_value!(latency_select, metrics.prof_select_cpu_avg_ns);
        push_value!(latency_enqueue, metrics.prof_enqueue_avg_ns);
        push_value!(latency_dispatch, metrics.prof_dispatch_avg_ns);
        push_value!(migrations, metrics.migrations);
        push_value!(mig_blocked, metrics.mig_blocked);
        push_value!(input_rate, metrics.input_trigger_rate as u64);
        push_value!(direct_pct, direct_pct);
        push_value!(edf_pct, edf_pct);
        push_value!(timestamps, Instant::now());

        // Accumulate cumulative totals (metrics are deltas)
        self.total_rr_enq = self.total_rr_enq.saturating_add(metrics.rr_enq);
        self.total_edf_enq = self.total_edf_enq.saturating_add(metrics.edf_enq);
        self.total_direct = self.total_direct.saturating_add(metrics.direct);
        self.total_migrations = self.total_migrations.saturating_add(metrics.migrations);
        self.total_mig_blocked = self.total_mig_blocked.saturating_add(metrics.mig_blocked);
    }

    pub fn get_sparkline_f64(&self, field: &str, last_n: usize) -> Vec<f64> {
        let start_idx = |len: usize| len.saturating_sub(last_n);

        match field {
            "cpu_util" => {
                let start = start_idx(self.cpu_util.len());
                self.cpu_util.iter().skip(start).copied().collect()
            }
            "cpu_avg" => {
                let start = start_idx(self.cpu_avg.len());
                self.cpu_avg.iter().skip(start).copied().collect()
            }
            "fg_cpu_pct" => {
                let start = start_idx(self.fg_cpu_pct.len());
                self.fg_cpu_pct.iter().skip(start).copied().collect()
            }
            "direct_pct" => {
                let start = start_idx(self.direct_pct.len());
                self.direct_pct.iter().skip(start).copied().collect()
            }
            "edf_pct" => {
                let start = start_idx(self.edf_pct.len());
                self.edf_pct.iter().skip(start).copied().collect()
            }
            _ => vec![],
        }
    }

    pub fn get_sparkline_u64(&self, field: &str, last_n: usize) -> Vec<u64> {
        let start_idx = |len: usize| len.saturating_sub(last_n);

        match field {
            "input_rate" => {
                let start = start_idx(self.input_rate.len());
                self.input_rate.iter().skip(start).copied().collect()
            }
            "migrations" => {
                let start = start_idx(self.migrations.len());
                self.migrations.iter().skip(start).copied().collect()
            }
            "mig_blocked" => {
                let start = start_idx(self.mig_blocked.len());
                self.mig_blocked.iter().skip(start).copied().collect()
            }
            _ => vec![],
        }
    }

    pub fn latest_f64(&self, field: &str) -> Option<f64> {
        match field {
            "cpu_util" => self.cpu_util.back().copied(),
            "cpu_avg" => self.cpu_avg.back().copied(),
            "fg_cpu_pct" => self.fg_cpu_pct.back().copied(),
            "direct_pct" => self.direct_pct.back().copied(),
            "edf_pct" => self.edf_pct.back().copied(),
            _ => None,
        }
    }

    pub fn latest_u64(&self, field: &str) -> Option<u64> {
        match field {
            "input_rate" => self.input_rate.back().copied(),
            "migrations" => self.migrations.back().copied(),
            "mig_blocked" => self.mig_blocked.back().copied(),
            "latency_select" => self.latency_select.back().copied(),
            "latency_enqueue" => self.latency_enqueue.back().copied(),
            "latency_dispatch" => self.latency_dispatch.back().copied(),
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
    Error,        // Scheduler crashed or failed
    Initializing, // Starting up
}

/// TUI state management
pub struct TuiState {
    pub paused: bool,
    pub start_time: Instant,
    pub config: ConfigSummary,
    pub active_tab: ActiveTab,
    pub update_rate: UpdateRate,
    pub history: HistoricalData,
    pub event_log: EventLog,
    pub obs_pid: Option<u32>,
    pub game_pid: u32,
    pub game_stats: Option<ProcessStats>,
    pub obs_stats: Option<ProcessStats>,
    pub scheduler_status: SchedulerStatus,
    pub last_successful_update: Instant,
    pub prev_metrics: Option<Metrics>,
    pub last_metrics: Option<Metrics>,
    pub input_idle_alert: bool,
    pub mig_block_alert: bool,
    pub latency_alert: bool,
    pub fentry_idle_alert: bool,
    pub stale_alert: bool,
    pub input_devices: Vec<String>,
}

/// Scheduler configuration summary
#[derive(Clone)]
pub struct ConfigSummary {
    pub slice_us: u64,
    pub slice_lag_us: u64,
    pub input_window_us: u64,
    pub mig_window_ms: u64,
    pub mig_max: u32,
    pub wakeup_timer_us: u64,
    pub mm_affinity: bool,
    pub avoid_smt: bool,
    pub preferred_idle_scan: bool,
    pub enable_numa: bool,
}

impl ConfigSummary {
    pub fn from_opts(opts: &Opts) -> Self {
        Self {
            slice_us: opts.slice_us,
            slice_lag_us: opts.slice_lag_us,
            input_window_us: opts.input_window_us,
            mig_window_ms: opts.mig_window_ms,
            mig_max: opts.mig_max,
            wakeup_timer_us: opts.wakeup_timer_us,
            mm_affinity: opts.mm_affinity,
            avoid_smt: opts.avoid_smt,
            preferred_idle_scan: opts.preferred_idle_scan,
            enable_numa: opts.enable_numa,
        }
    }
}

impl Default for ConfigSummary {
    fn default() -> Self {
        Self {
            slice_us: 10,
            slice_lag_us: 20000,
            input_window_us: 5000,
            mig_window_ms: 50,
            mig_max: 3,
            wakeup_timer_us: 500,
            mm_affinity: false,
            avoid_smt: false,
            preferred_idle_scan: false,
            enable_numa: false,
        }
    }
}

impl TuiState {
    pub fn new(config: ConfigSummary, history_len: usize, event_capacity: usize, input_devices: Vec<String>) -> Self {
        // Try to find OBS PID
        let obs_pid = crate::process_monitor::find_obs_pid();

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
            game_stats: None,
            obs_stats: None,
            scheduler_status: SchedulerStatus::Initializing,
            last_successful_update: Instant::now(),
            prev_metrics: None,
            last_metrics: None,
            input_idle_alert: false,
            mig_block_alert: false,
            latency_alert: false,
            fentry_idle_alert: false,
            stale_alert: false,
            input_devices,
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

/// Create a horizontal bar for percentage visualization
fn create_bar(pct: f64, width: usize) -> String {
    let filled = ((pct / 100.0) * width as f64) as usize;
    let filled = filled.min(width);
    let empty = width.saturating_sub(filled);
    format!("{}{}", "█".repeat(filled), "░".repeat(empty))
}

/// Render the TUI dashboard
pub fn render_ui(f: &mut Frame, metrics: &Metrics, state: &TuiState) {
    let size = f.area();

    // Main layout: header + tabs + content + footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Length(3),  // Tab bar
            Constraint::Min(0),     // Content
            Constraint::Length(1),  // Footer
        ])
        .split(size);

    // Header
    render_header(f, chunks[0], state);

    // Tab bar
    render_tabs(f, chunks[1], state);

    // Content (tab-specific)
    match state.active_tab {
        ActiveTab::Overview => render_overview_tab(f, chunks[2], metrics, state),
        ActiveTab::Performance => render_performance_tab(f, chunks[2], metrics, state),
        ActiveTab::Threads => render_threads_tab(f, chunks[2], metrics, state),
        ActiveTab::Events => render_events_tab(f, chunks[2], state),
        ActiveTab::Help => render_help_tab(f, chunks[2]),
    }

    // Footer
    render_footer(f, chunks[3]);
}

fn render_tabs(f: &mut Frame, area: Rect, state: &TuiState) {
    let tab_titles = vec!["[1] Overview", "[2] Performance", "[3] Threads", "[4] Events", "[5] Help"];
    let tabs = Tabs::new(tab_titles)
        .block(Block::default().borders(Borders::ALL).title(" Navigation "))
        .select(state.active_tab as usize)
        .style(Style::default().fg(Color::White))
        .highlight_style(
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD)
        );
    f.render_widget(tabs, area);
}

// ============================================================================
// TAB RENDERING FUNCTIONS
// ============================================================================

fn render_overview_tab(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),  // Configuration
            Constraint::Length(8),  // Health Check (4 rows + borders)
            Constraint::Length(5),  // Process Comparison
            Constraint::Length(6),  // Game + CPU
            Constraint::Length(6),  // Input Mode + Queues
            Constraint::Length(6),  // Threads + Windows
            Constraint::Length(6),  // Migrations + BPF Latency
            Constraint::Min(0),     // Extra space
        ])
        .split(area);

    render_config(f, chunks[0], state);
    render_health_check(f, chunks[1], metrics, state);
    render_process_comparison(f, chunks[2], metrics, state);
    render_row1(f, chunks[3], metrics);
    render_row2(f, chunks[4], metrics, state);
    render_row3(f, chunks[5], metrics);
    render_row4(f, chunks[6], metrics, state);
}

fn render_performance_tab(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10),
            Constraint::Length(9),
            Constraint::Length(11),
            Constraint::Min(0),
        ])
        .split(area);

    render_cpu_trends(f, layout[0], state);
    render_queue_trends(f, layout[1], state);
    render_latency_chart(f, layout[2], state, metrics);

    let footer = Paragraph::new(vec![
        Line::from(Span::styled(
            "Charts show real scheduler samples gathered at runtime; scale adapts to history window.",
            Style::default().fg(Color::Yellow),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Diagnostics Notes ", Style::default().fg(Color::Yellow)))
            .border_style(Style::default().fg(Color::Yellow)),
    );
    f.render_widget(footer, layout[3]);
}

fn render_threads_tab(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),
            Constraint::Length(7),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(area);

    render_thread_breakdown(f, layout[0], metrics, state);
    render_thread_totals(f, layout[1], metrics);
    render_thread_notes(f, layout[2], state);
}

fn render_events_tab(f: &mut Frame, area: Rect, state: &TuiState) {
    let events = state.event_log.events();
    let max_items = area.height.saturating_sub(2) as usize; // account for borders
    let width = area.width.saturating_sub(4) as usize;

    let items: Vec<ListItem> = events
        .iter()
        .rev()
        .take(max_items.max(1))
        .map(|entry| {
            let ts = entry.timestamp.format("%H:%M:%S").to_string();
            let (label, color, prefix) = match entry.level {
                EventLevel::Info => ("INFO", Color::Cyan, " "),
                EventLevel::Warn => ("WARN", Color::Yellow, "!"),
                EventLevel::Error => ("ERR", Color::Red, "!"),
            };

            let mut message = entry.message.clone();
            if message.len() > width.saturating_sub(18) {
                message.truncate(width.saturating_sub(21));
                message.push_str("...");
            }

            let line = Line::from(vec![
                Span::styled(ts, Style::default().fg(Color::DarkGray)),
                Span::raw("  "),
                Span::styled(format!("{}[{}]", prefix, label), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::raw("  "),
                Span::raw(message),
            ]);

            ListItem::new(line)
        })
        .collect();

    let list = if items.is_empty() {
        List::new(vec![ListItem::new(Line::from("No events yet"))])
    } else {
        List::new(items)
    };

    let list = list.block(
        Block::default()
            .borders(Borders::ALL)
            .title(Span::styled(" Event Log ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)))
            .border_style(Style::default().fg(Color::Red)),
    );

    f.render_widget(list, area);
}

fn render_help_tab(f: &mut Frame, area: Rect) {
    let help_text = vec![
        Line::from(vec![
            Span::styled("KEYBOARD SHORTCUTS", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("[1-5]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("      Switch tabs directly"),
        ]),
        Line::from(vec![
            Span::styled("[←/→]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("      Navigate between tabs"),
        ]),
        Line::from(vec![
            Span::styled("[u]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("        Cycle update rate (1s → 5s → 30s → 60s)"),
        ]),
        Line::from(vec![
            Span::styled("[r]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("        Reset statistics"),
        ]),
        Line::from(vec![
            Span::styled("[p]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("        Pause/Resume updates"),
        ]),
        Line::from(vec![
            Span::styled("[q]", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw("        Quit"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled("METRICS LEGEND", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from("CPU%       - CPU utilization percentage"),
        Line::from("EDF%       - Earliest-Deadline-First queue usage"),
        Line::from("Direct%    - Direct dispatch rate (bypass queue)"),
        Line::from("Mig Blocked- Migrations blocked by rate limiter"),
        Line::from("MM Hint    - Memory affinity hint hits"),
        Line::from("Input Trig - Input window activations/sec"),
    ];

    let help_widget = Paragraph::new(help_text)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(" Help & Legend ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));

    f.render_widget(help_widget, area);
}

// ============================================================================
// HELPER RENDERING FUNCTIONS
// ============================================================================

fn render_health_check(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    // Extract game basename for display (handle both Unix / and Wine \ paths)
    let game_name = if !metrics.fg_app.is_empty() {
        let name = metrics.fg_app
            .rsplit('/')
            .next()
            .or_else(|| metrics.fg_app.rsplit('\\').next())
            .unwrap_or(&metrics.fg_app);
        name.to_string()
    } else if metrics.fg_pid > 0 {
        format!("PID {}", metrics.fg_pid)
    } else {
        "None".to_string()
    };

    // Check status of each subsystem
    let game_detected = metrics.fg_pid > 0;

    // Sanitize input rate (cap at 10000 Hz, anything higher is likely a bug)
    let input_rate = if metrics.input_trigger_rate > 10000 {
        0  // Clearly buggy value, show 0
    } else {
        metrics.input_trigger_rate
    };
    let input_active = input_rate > 0 || metrics.continuous_input_mode != 0;
    let input_rate_style = if input_active {
        Style::default().fg(Color::Green)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    // Sanitize thread counts
    let sanitize = |val: u64| if val > 1000 { 0 } else { val };
    let threads_classified = sanitize(metrics.input_handler_threads) > 0 ||
                            sanitize(metrics.gpu_submit_threads) > 0 ||
                            sanitize(metrics.game_audio_threads) > 0;

    let windows_active = metrics.win_input_ns > 0 || metrics.win_frame_ns > 0;
    let migrations_working = metrics.migrations > 0 || metrics.mig_blocked > 0;
    let bpf_running = state.scheduler_status == SchedulerStatus::Running;

    // Helper: status indicator
    let status_icon = |ok: bool| {
        if ok {
            Span::styled(" ✓ ", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD))
        } else {
            Span::styled(" ✗ ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD))
        }
    };

    // Check fentry hook status
    let fentry_working = metrics.fentry_boost_triggers > 0;
    let evdev_count = metrics.input_trig.saturating_sub(metrics.fentry_boost_triggers);
    let evdev_working = evdev_count > 0;

    // Calculate input path ratio
    let fentry_pct = if metrics.input_trig > 0 {
        (metrics.fentry_boost_triggers as f64 / metrics.input_trig as f64) * 100.0
    } else {
        0.0
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
                &game_name,
                if game_detected { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }
            ),
        ]),
        Line::from(vec![
            status_icon(fentry_working),
            Span::raw("Input (fentry):  "),
            Span::styled(
                if fentry_working {
                    format!("{} events ({:.0}%)", format_number(metrics.fentry_boost_triggers), fentry_pct)
                } else {
                    "Not active".to_string()
                },
                if fentry_working { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Yellow) }
            ),
            Span::raw("    │    "),
            status_icon(evdev_working),
            Span::raw("Input (evdev):  "),
            Span::styled(
                if evdev_working {
                    format!("{} events ({:.0}%)", format_number(evdev_count), 100.0 - fentry_pct)
                } else {
                    "Fallback idle".to_string()
                },
                if evdev_working { Style::default().fg(Color::Cyan) } else { Style::default().fg(Color::DarkGray) }
            ),
            Span::raw("    │    Input Rate:  "),
            Span::styled(format!("{} /sec", input_rate), input_rate_style),
        ]),
        Line::from(vec![
            status_icon(threads_classified),
            Span::raw("Threads Classified:  "),
            Span::styled(
                format!("{} total",
                    sanitize(metrics.input_handler_threads) + sanitize(metrics.gpu_submit_threads) +
                    sanitize(metrics.game_audio_threads) + sanitize(metrics.network_threads)),
                if threads_classified { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Yellow) }
            ),
            Span::raw("    │    "),
            status_icon(windows_active),
            Span::raw("Boost Windows:  "),
            Span::styled(
                if windows_active { "Active" } else { "Idle" },
                if windows_active { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }
            ),
        ]),
        Line::from(vec![
            Span::raw("Fentry Efficiency:  "),
            Span::styled(
                if metrics.fentry_total_events > 0 {
                    format!("{}/{} gaming ({:.0}% filtered)",
                        format_number(metrics.fentry_gaming_events),
                        format_number(metrics.fentry_total_events),
                        if metrics.fentry_total_events > 0 {
                            (metrics.fentry_filtered_events as f64 / metrics.fentry_total_events as f64) * 100.0
                        } else { 0.0 }
                    )
                } else {
                    "Not tracking".to_string()
                },
                Style::default().fg(Color::Cyan)
            ),
            Span::raw("    │    "),
            status_icon(migrations_working),
            Span::raw("Task Migration:  "),
            Span::styled(
                if migrations_working { "Working" } else { "Idle" },
                if migrations_working { Style::default().fg(Color::Green) } else { Style::default().fg(Color::DarkGray) }
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
    } else {
        "No game detected".to_string()
    };

    // Build comparison display using collected stats
    let comparison_text = vec![
        Line::from(vec![
            Span::styled("Game: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:<20}", game_name)),
            Span::raw("  PID: "),
            Span::styled(
                format!("{:>5}", if let Some(ref gs) = state.game_stats { gs.pid } else { metrics.fg_pid as u32 }),
                Style::default().fg(Color::Yellow)
            ),
            Span::raw("  CPU: "),
            Span::styled(
                format!("{:>5.1}%", if let Some(ref gs) = state.game_stats { gs.cpu_percent } else { metrics.fg_cpu_pct as f64 }),
                Style::default().fg(Color::Green)
            ),
            Span::raw("  GPU: "),
            Span::styled(
                format!("{:>5.1}%", if let Some(ref gs) = state.game_stats { gs.gpu_percent } else { 0.0 }),
                Style::default().fg(Color::Cyan)
            ),
            Span::raw("  Thr: "),
            Span::styled(
                format!("{:>3}", if let Some(ref gs) = state.game_stats { gs.threads } else { 0 }),
                Style::default().fg(Color::Magenta)
            ),
            Span::raw("  Mem: "),
            Span::styled(
                format!("{:>4} MB", if let Some(ref gs) = state.game_stats { gs.memory_mb } else { 0 }),
                Style::default().fg(Color::Yellow)
            ),
        ]),
        Line::from(vec![
            Span::styled("OBS:  ", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::raw(format!("{:<20}",
                if let Some(ref obs) = state.obs_stats {
                    obs.name.clone()
                } else if state.obs_pid.is_some() {
                    "tracking...".to_string()
                } else {
                    "Not detected".to_string()
                }
            )),
            Span::raw("  PID: "),
            Span::styled(
                format!("{:>5}", if let Some(ref obs) = state.obs_stats { obs.pid } else { state.obs_pid.unwrap_or(0) }),
                Style::default().fg(Color::Yellow)
            ),
            Span::raw("  CPU: "),
            Span::styled(
                format!("{:>5.1}%", if let Some(ref obs) = state.obs_stats { obs.cpu_percent } else { 0.0 }),
                if let Some(ref obs) = state.obs_stats {
                    if obs.cpu_percent > 15.0 { Style::default().fg(Color::Red) }
                    else if obs.cpu_percent > 8.0 { Style::default().fg(Color::Yellow) }
                    else { Style::default().fg(Color::Green) }
                } else {
                    Style::default().fg(Color::DarkGray)
                }
            ),
            Span::raw("  GPU: "),
            Span::styled(
                format!("{:>5.1}%", if let Some(ref obs) = state.obs_stats { obs.gpu_percent } else { 0.0 }),
                Style::default().fg(Color::Cyan)
            ),
            Span::raw("  Thr: "),
            Span::styled(
                format!("{:>3}", if let Some(ref obs) = state.obs_stats { obs.threads } else { 0 }),
                Style::default().fg(Color::Magenta)
            ),
            Span::raw("  Mem: "),
            Span::styled(
                format!("{:>4} MB", if let Some(ref obs) = state.obs_stats { obs.memory_mb } else { 0 }),
                Style::default().fg(Color::Yellow)
            ),
        ]),
    ];

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
            Span::styled(format!("{}µs", cfg.slice_lag_us), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Input Win: "),
            Span::styled(format!("{}µs", cfg.input_window_us), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Wake Timer: "),
            Span::styled(format!("{}µs", cfg.wakeup_timer_us), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("Migration: "),
            Span::styled(format!("{} max/{} ms", cfg.mig_max, cfg.mig_window_ms), Style::default().fg(Color::Yellow)),
            Span::raw("  │  Flags: "),
            Span::styled(
                format!("{}{}{}{}",
                    if cfg.mm_affinity { "MM " } else { "" },
                    if cfg.avoid_smt { "AVOID-SMT " } else { "" },
                    if cfg.preferred_idle_scan { "PREF-IDLE " } else { "" },
                    if cfg.enable_numa { "NUMA" } else { "" }
                ),
                Style::default().fg(Color::Green)
            ),
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
        SchedulerStatus::Error => ("ERROR", Color::Red),
        SchedulerStatus::Initializing => ("STARTING", Color::Yellow),
    };

    // Check if data is stale (no update in 5+ seconds = likely crashed)
    let data_age = state.last_successful_update.elapsed().as_secs();
    let stale_indicator = if data_age > 5 && state.scheduler_status == SchedulerStatus::Running {
        format!(" [STALE {}s]", data_age)
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

fn render_row2(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let layout = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50),
            Constraint::Percentage(50),
        ])
        .split(area);

    render_input_mode(f, layout[0], metrics, state);
    render_queue_status(f, layout[1], metrics, state);
}

fn render_input_mode(f: &mut Frame, area: Rect, metrics: &Metrics, state: &TuiState) {
    let input_active = metrics.win_input_ns > 0;
    let fentry_active = metrics.fentry_boost_triggers > 0;

    let status_style = if input_active {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Yellow)
    };

    let mut lines = vec![
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
                    "idle".to_string()
                },
                status_style,
            ),
        ]),
        Line::from(vec![
            Span::styled("Raw Input (fentry)", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                if fentry_active {
                    format!("OK ({} events)", metrics.fentry_boost_triggers)
                } else {
                    "waiting".to_string()
                },
                Style::default().fg(if fentry_active { Color::Green } else { Color::Yellow }),
            ),
        ]),
        Line::from(vec![
            Span::styled("Trigger Rate", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{} Hz", metrics.input_trigger_rate),
                Style::default().fg(Color::Magenta),
            ),
        ]),
        Line::from(vec![
            Span::styled("Input Triggers", Style::default().fg(Color::Cyan)),
            Span::raw(": "),
            Span::styled(
                format!("{}", metrics.input_trig),
                Style::default().fg(Color::Magenta),
            ),
        ]),
    ];

    if !state.input_devices.is_empty() {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled("Devices", Style::default().fg(Color::Cyan))));
        for dev in state.input_devices.iter().take(4) {
            lines.push(Line::from(vec![
                Span::raw("  • "),
                Span::styled(dev, Style::default().fg(Color::Gray)),
            ]));
        }
        if state.input_devices.len() > 4 {
            lines.push(Line::from(vec![
                Span::raw("  • "),
                Span::styled(
                    format!("… {} more", state.input_devices.len() - 4),
                    Style::default().fg(Color::DarkGray),
                ),
            ]));
        }
    } else {
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(
            "No input devices detected",
            Style::default().fg(Color::Yellow),
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(Span::styled(
            " Input Mode ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(ratatui::widgets::Wrap { trim: true });

    f.render_widget(paragraph, area);
}

fn render_queue_status(f: &mut Frame, area: Rect, _metrics: &Metrics, state: &TuiState) {
    let total_rr = state.history.total_rr_enq;
    let total_edf = state.history.total_edf_enq;
    let total_direct = state.history.total_direct;

    let total_enq = total_rr + total_edf;
    let edf_pct = if total_enq > 0 {
        (total_edf as f64 * 100.0) / total_enq as f64
    } else {
        0.0
    };
    let direct_total = total_rr + total_direct;
    let direct_pct = if direct_total > 0 {
        (total_direct as f64 * 100.0) / direct_total as f64
    } else {
        0.0
    };

    let queue_info = vec![
        Line::from(vec![
            Span::raw("RR:     "),
            Span::raw(format!("{:>8}  {}", format_number(total_rr), create_bar(50.0, 8))),
        ]),
        Line::from(vec![
            Span::raw("EDF:    "),
            Span::styled(
                format!("{:>8}  {} {:>3.0}%", format_number(total_edf), create_bar(edf_pct, 8), edf_pct),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::raw("Direct: "),
            Span::styled(
                format!("{:>8}  {:>3.0}%", format_number(total_direct), direct_pct),
                Style::default().fg(if direct_pct > 40.0 { Color::Green } else { Color::Yellow }),
            ),
        ]),
    ];

    let queue_block = Paragraph::new(queue_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" QUEUES ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
    f.render_widget(queue_block, area);
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

    // BPF Latency
    let lat_info = if metrics.prof_select_cpu_avg_ns > 0 || metrics.prof_enqueue_avg_ns > 0 {
        vec![
            Line::from(vec![
                Span::raw("select_cpu:  "),
                Span::styled(format!("{:>4}ns", metrics.prof_select_cpu_avg_ns), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("enqueue:     "),
                Span::styled(format!("{:>4}ns", metrics.prof_enqueue_avg_ns), Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::raw("dispatch:    "),
                Span::styled(format!("{:>4}ns", metrics.prof_dispatch_avg_ns), Style::default().fg(Color::Cyan)),
                Span::raw("  deadline: "),
                Span::styled(format!("{:>3}ns", metrics.prof_deadline_avg_ns), Style::default().fg(Color::Cyan)),
            ]),
        ]
    } else {
        vec![
            Line::from(Span::styled("Profiling not enabled", Style::default().fg(Color::DarkGray))),
            Line::from(""),
            Line::from("Use --verbose flag to enable"),
        ]
    };

    let lat_block = Paragraph::new(lat_info)
        .block(Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Blue))
            .title(Span::styled(" BPF LATENCY (avg) ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))));
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
    let y_labels = vec![Span::raw("0"), Span::raw("250"), Span::raw("500"), Span::raw("750"), Span::raw("1k")];

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

    let rows = vec![
        Row::new(vec![Span::raw("Input"), Span::raw(metrics.input_handler_threads.to_string()), Span::raw(format!("{:.1}", metrics.fg_cpu_pct as f64)), Span::raw("Classifier: input threads")]),
        Row::new(vec![Span::raw("GPU Submit"), Span::raw(metrics.gpu_submit_threads.to_string()), Span::raw(format!("{:.1}", direct_pct)), Span::raw("Direct dispatch share")]),
        Row::new(vec![Span::raw("Game Audio"), Span::raw(metrics.game_audio_threads.to_string()), Span::raw("-"), Span::raw("Audio priority boost")]),
        Row::new(vec![Span::raw("System Audio"), Span::raw(metrics.system_audio_threads.to_string()), Span::raw("-"), Span::raw("Mixer rate")]),
        Row::new(vec![Span::raw("Compositor"), Span::raw(metrics.compositor_threads.to_string()), Span::raw("-"), Span::raw("Frame pacing")]),
        Row::new(vec![Span::raw("Network"), Span::raw(metrics.network_threads.to_string()), Span::raw("-"), Span::raw("Netcode priority")]),
        Row::new(vec![Span::raw("Background"), Span::raw(metrics.background_threads.to_string()), Span::raw("-"), Span::raw("Rate limited")]),
    ];

    let table = Table::new(rows.into_iter(), [
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

fn render_thread_notes(f: &mut Frame, area: Rect, state: &TuiState) {
    let lines = vec![
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
        Line::from("Use [r] to reset counters after thread adjustments"),
    ];

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

    let fentry_idle = metrics.fentry_boost_triggers == 0 && metrics.input_trigger_rate > 0;
    if fentry_idle && !state.fentry_idle_alert {
        state.event_log.push(EventLevel::Warn, "Fentry hooks not triggering while inputs active".into());
    }
    state.fentry_idle_alert = fentry_idle;
}

/// Main TUI monitor loop
pub fn monitor_tui(
    intv: Duration,
    shutdown: Arc<AtomicBool>,
    opts: &Opts,
    device_names: Vec<String>,
) -> Result<()> {
    // Suppress all logging during TUI mode to prevent interference
    log::set_max_level(log::LevelFilter::Off);

    enable_raw_mode()?;
    let stdout = io::stdout();
    execute!(io::stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;

    let terminal = Rc::new(RefCell::new(terminal));
    let config_summary = ConfigSummary::from_opts(opts);
    let state = Rc::new(RefCell::new(TuiState::new(config_summary, 300, 100, device_names)));

    let process_monitor = Rc::new(RefCell::new(ProcessMonitor::new()?));
    let input_poll_interval = Duration::from_millis(20);
    let last_poll = Rc::new(RefCell::new(Instant::now()));

    let terminal_clone = terminal.clone();
    let state_clone = state.clone();
    let shutdown_clone = shutdown.clone();
    let state_for_metrics = state.clone();
    let process_monitor_clone = process_monitor.clone();

    let result = scx_utils::monitor_stats::<Metrics>(
        &[],
        intv,
        move || {
            if shutdown_clone.load(Ordering::Relaxed) {
                return true;
            }

            let now = Instant::now();
            if now.duration_since(last_poll.borrow().clone()) >= input_poll_interval {
                *last_poll.borrow_mut() = now;

                if let Ok(true) = event::poll(Duration::from_millis(1)) {
                    if let Ok(Event::Key(key)) = event::read() {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Char('q') | KeyCode::Char('Q') => {
                                    shutdown_clone.store(true, Ordering::Relaxed);
                                    if let Ok(mut st) = state_clone.try_borrow_mut() {
                                        st.scheduler_status = SchedulerStatus::Stopped;
                                        st.event_log.push(EventLevel::Info, "Shutdown requested".to_string());
                                    }
                                    return true;
                                }
                                KeyCode::Char('p') | KeyCode::Char('P') => {
                                    if let Ok(mut st) = state_clone.try_borrow_mut() {
                                        st.paused = !st.paused;
                                    }
                                }
                                KeyCode::Char('u') | KeyCode::Char('U') => {
                                    state_clone.borrow_mut().cycle_update_rate();
                                }
                                KeyCode::Char('r') | KeyCode::Char('R') => {
                                    state_clone.borrow_mut().reset_stats();
                                }
                                KeyCode::Char('1') => state_clone.borrow_mut().active_tab = ActiveTab::Overview,
                                KeyCode::Char('2') => state_clone.borrow_mut().active_tab = ActiveTab::Performance,
                                KeyCode::Char('3') => state_clone.borrow_mut().active_tab = ActiveTab::Threads,
                                KeyCode::Char('4') => state_clone.borrow_mut().active_tab = ActiveTab::Events,
                                KeyCode::Char('5') => state_clone.borrow_mut().active_tab = ActiveTab::Help,
                                KeyCode::Left => state_clone.borrow_mut().prev_tab(),
                                KeyCode::Right => state_clone.borrow_mut().next_tab(),
                                _ => {}
                            }
                        }
                    }
                }

                if let Ok(mut st) = state_clone.try_borrow_mut() {
                    if st.scheduler_status == SchedulerStatus::Running {
                        let stale = st.last_successful_update.elapsed() > Duration::from_secs(5);
                        if stale && !st.stale_alert {
                            st.stale_alert = true;
                            st.scheduler_status = SchedulerStatus::Error;
                            let age = st.last_successful_update.elapsed().as_secs();
                            st.event_log.push(EventLevel::Error, format!("No metrics for {}s; scheduler stalled?", age));
                        } else if !stale && st.stale_alert {
                            st.stale_alert = false;
                            st.scheduler_status = SchedulerStatus::Running;
                            st.event_log.push(EventLevel::Info, "Metrics stream resumed".to_string());
                        }
                    }
                }

                if let Ok(mut term) = terminal_clone.try_borrow_mut() {
                    let draw_result = term.draw(|f| {
                        let st = state_clone.borrow();
                        let fallback = Metrics::default();
                        let metrics_ref = st.last_metrics.as_ref().unwrap_or(&fallback);
                        render_ui(f, metrics_ref, &st);
                    });
                    if let Err(e) = draw_result {
                        log::warn!("TUI draw error: {}", e);
                    }
                }
            }

            false
        },
        move |metrics| {
            {
                let mut st = state_for_metrics.borrow_mut();
                st.scheduler_status = SchedulerStatus::Running;
                st.last_successful_update = Instant::now();

                if let Some(last) = st.last_metrics.take() {
                    st.prev_metrics = Some(last);
                }
                st.last_metrics = Some(metrics.clone());
                st.history.push(&metrics);
                evaluate_alerts(&mut st, &metrics);

                if metrics.fg_pid > 0 && st.game_pid != metrics.fg_pid as u32 {
                    st.game_pid = metrics.fg_pid as u32;
                }
            }

            let game_pid = state_for_metrics.borrow().game_pid;
            let obs_pid = state_for_metrics.borrow().obs_pid;

            {
                let mut monitor = process_monitor_clone.borrow_mut();
                if game_pid > 0 {
                    if let Some(stat) = monitor.get_process_stats(game_pid) {
                        state_for_metrics.borrow_mut().game_stats = Some(stat);
                    }
                }
                if let Some(obs_pid) = obs_pid {
                    if let Some(stat) = monitor.get_process_stats(obs_pid) {
                        state_for_metrics.borrow_mut().obs_stats = Some(stat);
                    }
                }
            }

            Ok(())
        },
    );

    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen)?;
    result
}
