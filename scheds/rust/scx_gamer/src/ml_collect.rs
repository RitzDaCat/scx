// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Machine Learning Data Collection
// Copyright (c) 2025 RitzDaCat
//
// Collects scheduler performance metrics for machine learning-based per-game tuning.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::stats::Metrics;

/// Single sample of scheduler performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerformanceSample {
    /// Unix timestamp (seconds since epoch)
    pub timestamp: u64,

    /// Scheduler configuration at time of sample
    pub config: SchedulerConfig,

    /// Performance metrics
    pub metrics: MetricsSample,

    /// Game information
    pub game: GameInfo,
}

/// Scheduler configuration parameters
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchedulerConfig {
    pub slice_us: u64,
    pub slice_lag_us: u64,
    pub input_window_us: u64,
    pub mig_window_ms: u64,
    pub mig_max: u32,
    pub mm_affinity: bool,
    pub avoid_smt: bool,
    pub preferred_idle_scan: bool,
    pub enable_numa: bool,
    pub wakeup_timer_us: u64,
}

/// Performance metrics snapshot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSample {
    /// CPU utilization (0-100%)
    pub cpu_util_pct: f64,

    /// Latency measurements (nanoseconds)
    pub latency_select_cpu_avg_ns: u64,
    pub latency_enqueue_avg_ns: u64,
    pub latency_dispatch_avg_ns: u64,
    pub latency_deadline_avg_ns: u64,

    /// Throughput metrics
    pub enqueues_per_sec: f64,
    pub dispatches_per_sec: f64,

    /// Quality metrics
    pub migration_block_rate: f64,  // % of migrations blocked
    pub mm_hint_hit_rate: f64,      // % of mm hints that hit
    pub direct_dispatch_rate: f64,  // % of direct dispatches

    /// Thread classification
    pub input_handler_count: u64,
    pub gpu_submit_count: u64,
    pub compositor_count: u64,
    pub network_count: u64,
}

/// Game identification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameInfo {
    pub tgid: u32,
    pub name: String,
    pub is_wine: bool,
    pub is_steam: bool,
}

/// Aggregated performance data for a specific game
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GamePerformanceData {
    pub game_name: String,
    pub samples: Vec<PerformanceSample>,
    pub best_config: Option<SchedulerConfig>,
    pub best_score: Option<f64>,
}

/// ML data collector
pub struct MLCollector {
    data_dir: PathBuf,
    current_game: Option<GameInfo>,
    samples: Vec<PerformanceSample>,
    config: SchedulerConfig,
    sample_interval: Duration,
    last_sample_time: SystemTime,
    last_save_time: SystemTime,  // Track last save to prevent unbounded memory growth
}

impl MLCollector {
    pub fn new(data_dir: impl AsRef<Path>, config: SchedulerConfig, sample_interval: Duration) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        fs::create_dir_all(&data_dir)?;

        let now = SystemTime::now();
        Ok(Self {
            data_dir,
            current_game: None,
            samples: Vec::new(),
            config,
            sample_interval,
            last_sample_time: now,
            last_save_time: now,
        })
    }

    /// Update current game information
    pub fn set_game(&mut self, game: Option<GameInfo>) {
        // Clone prev_game to avoid borrow conflict with mutable self.save_samples()
        if let Some(prev_game) = self.current_game.clone() {
            // Game changed - save accumulated data (drain clears samples automatically)
            if let Err(e) = self.save_samples(&prev_game) {
                log::warn!("Failed to save ML data for {}: {}", prev_game.name, e);
            } else {
                self.last_save_time = SystemTime::now();  // Update save time on successful save
            }
            // No need to clear - drain() already emptied the vec
        }

        self.current_game = game;
    }

    /// Record a performance sample
    pub fn record_sample(&mut self, metrics: &Metrics) -> Result<()> {
        let now = SystemTime::now();
        if now.duration_since(self.last_sample_time).unwrap_or(Duration::ZERO) < self.sample_interval {
            return Ok(());  // Too soon, skip sample
        }

        self.last_sample_time = now;

        // Create a default "system" game if none detected (for general profiling)
        let game = self.current_game.clone().unwrap_or_else(|| GameInfo {
            tgid: 0,
            name: "system".to_string(),
            is_wine: false,
            is_steam: false,
        });

        let sample = PerformanceSample {
            timestamp: now.duration_since(UNIX_EPOCH)?.as_secs(),
            config: self.config.clone(),
            metrics: Self::convert_metrics_static(metrics),
            game: game.clone(),
        };

        self.samples.push(sample);
        log::debug!("ML: Recorded sample #{} for {}", self.samples.len(), game.name);

        // Auto-save strategy to prevent unbounded memory growth:
        // 1. Save every 100 samples (original logic)
        // 2. Save every 5 minutes (time-based flush to prevent multi-hour accumulation)
        const MAX_SAVE_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes
        let time_since_save = now.duration_since(self.last_save_time).unwrap_or(Duration::ZERO);

        if self.samples.len() >= 100 || time_since_save >= MAX_SAVE_INTERVAL {
            self.save_samples(&game)?;
            self.last_save_time = now;
        }

        Ok(())
    }

    /// Convert Metrics to MetricsSample (public for autotuner)
    #[inline]
    pub fn convert_metrics_static(m: &Metrics) -> MetricsSample {
        let total_enq = m.rr_enq + m.edf_enq;
        let total_mig = m.migrations + m.mig_blocked;
        let total_idle_pick = m.idle_pick;

        MetricsSample {
            cpu_util_pct: (m.cpu_util as f64) * 100.0 / 1024.0,
            latency_select_cpu_avg_ns: m.prof_select_cpu_avg_ns,
            latency_enqueue_avg_ns: m.prof_enqueue_avg_ns,
            latency_dispatch_avg_ns: m.prof_dispatch_avg_ns,
            latency_deadline_avg_ns: m.prof_deadline_avg_ns,
            enqueues_per_sec: total_enq as f64,  // Will be divided by interval in analysis
            dispatches_per_sec: (m.direct + m.shared) as f64,
            migration_block_rate: if total_mig > 0 {
                (m.mig_blocked as f64) / (total_mig as f64)
            } else { 0.0 },
            mm_hint_hit_rate: if total_idle_pick > 0 {
                (m.mm_hint_hit as f64) / (total_idle_pick as f64)
            } else { 0.0 },
            direct_dispatch_rate: if total_enq > 0 {
                (m.direct as f64) / (total_enq as f64)
            } else { 0.0 },
            input_handler_count: m.input_handler_threads,
            gpu_submit_count: m.gpu_submit_threads,
            compositor_count: m.compositor_threads,
            network_count: m.network_threads,
        }
    }

    /// Save accumulated samples to disk
    fn save_samples(&mut self, game: &GameInfo) -> Result<()> {
        if self.samples.is_empty() {
            return Ok(());
        }

        let filename = self.get_game_filename(&game.name);
        let mut data = self.load_or_create_game_data(&game.name)?;

        // Calculate score BEFORE draining (score needs the data)
        let current_score = self.calculate_score(&self.samples);
        let sample_count = self.samples.len();

        // PERF: Use drain to move data without cloning (saves allocation for 100+ samples)
        data.samples.extend(self.samples.drain(..));
        if data.best_score.is_none() || current_score > data.best_score.unwrap() {
            data.best_score = Some(current_score);
            data.best_config = Some(self.config.clone());
        }

        let json = serde_json::to_string_pretty(&data)?;
        fs::write(&filename, json)?;

        log::info!("ML: Saved {} samples for {} (score: {:.2})",
                   sample_count, game.name, current_score);

        Ok(())
    }

    /// Load existing game data or create new
    fn load_or_create_game_data(&self, game_name: &str) -> Result<GamePerformanceData> {
        let filename = self.get_game_filename(game_name);

        if filename.exists() {
            let content = fs::read_to_string(&filename)?;
            let data: GamePerformanceData = serde_json::from_str(&content)?;
            Ok(data)
        } else {
            Ok(GamePerformanceData {
                game_name: game_name.to_string(),
                samples: Vec::new(),
                best_config: None,
                best_score: None,
            })
        }
    }

    /// Get filename for game data
    fn get_game_filename(&self, game_name: &str) -> PathBuf {
        // Sanitize game name for filesystem
        let safe_name = game_name.replace(['/', '\\', ' ', ':'], "_");
        self.data_dir.join(format!("{}.json", safe_name))
    }

    /// Calculate performance score (higher is better)
    /// Uses centralized scoring logic from ml_scoring module
    fn calculate_score(&self, samples: &[PerformanceSample]) -> f64 {
        crate::ml_scoring::calculate_performance_score(samples)
    }

    /// Export all data as training dataset (CSV format for pandas/sklearn)
    pub fn export_training_csv(&self, output_path: impl AsRef<Path>) -> Result<()> {
        let mut all_samples = Vec::new();

        // Load all game data files
        for entry in fs::read_dir(&self.data_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let content = fs::read_to_string(&path)?;
                if let Ok(data) = serde_json::from_str::<GamePerformanceData>(&content) {
                    all_samples.extend(data.samples);
                }
            }
        }

        // Write CSV header
        let mut csv = String::from("timestamp,game_name,slice_us,slice_lag_us,input_window_us,mig_window_ms,mig_max,mm_affinity,avoid_smt,preferred_idle_scan,enable_numa,wakeup_timer_us,cpu_util_pct,latency_select_cpu_ns,latency_enqueue_ns,latency_dispatch_ns,latency_deadline_ns,enqueues_per_sec,dispatches_per_sec,migration_block_rate,mm_hint_hit_rate,direct_dispatch_rate,input_handler_count,gpu_submit_count,compositor_count,network_count\n");

        // Write each sample as CSV row
        for sample in &all_samples {
            csv.push_str(&format!(
                "{},{},{},{},{},{},{},{},{},{},{},{},{:.2},{},{},{},{},{},{:.4},{:.4},{:.4},{:.4},{},{},{},{}\n",
                sample.timestamp,
                sample.game.name,
                sample.config.slice_us,
                sample.config.slice_lag_us,
                sample.config.input_window_us,
                sample.config.mig_window_ms,
                sample.config.mig_max,
                sample.config.mm_affinity as u8,
                sample.config.avoid_smt as u8,
                sample.config.preferred_idle_scan as u8,
                sample.config.enable_numa as u8,
                sample.config.wakeup_timer_us,
                sample.metrics.cpu_util_pct,
                sample.metrics.latency_select_cpu_avg_ns,
                sample.metrics.latency_enqueue_avg_ns,
                sample.metrics.latency_dispatch_avg_ns,
                sample.metrics.latency_deadline_avg_ns,
                sample.metrics.enqueues_per_sec,
                sample.metrics.dispatches_per_sec,
                sample.metrics.migration_block_rate,
                sample.metrics.mm_hint_hit_rate,
                sample.metrics.direct_dispatch_rate,
                sample.metrics.input_handler_count,
                sample.metrics.gpu_submit_count,
                sample.metrics.compositor_count,
                sample.metrics.network_count,
            ));
        }

        fs::write(output_path, csv)?;
        log::info!("ML: Exported {} samples to training CSV", all_samples.len());

        Ok(())
    }

    // get_best_config and list_games removed - use ProfileManager and ml_show_best CLI instead

    /// Get summary statistics for a game
    pub fn get_game_summary(&self, game_name: &str) -> Result<GameSummary> {
        let data = self.load_or_create_game_data(game_name)?;

        if data.samples.is_empty() {
            return Ok(GameSummary::default());
        }

        // Aggregate metrics across all samples
        let total_samples = data.samples.len() as f64;
        let mut avg_cpu = 0.0;
        let mut avg_select_latency = 0.0;
        let mut avg_enqueue_latency = 0.0;
        let mut config_perf: HashMap<String, f64> = HashMap::new();

        for sample in &data.samples {
            avg_cpu += sample.metrics.cpu_util_pct;
            avg_select_latency += sample.metrics.latency_select_cpu_avg_ns as f64;
            avg_enqueue_latency += sample.metrics.latency_enqueue_avg_ns as f64;

            // Track performance by configuration
            let config_key = Self::config_to_key(&sample.config);
            let score = self.calculate_score(&[sample.clone()]);
            *config_perf.entry(config_key).or_insert(0.0) += score;
        }

        Ok(GameSummary {
            sample_count: data.samples.len(),
            avg_cpu_util: avg_cpu / total_samples,
            avg_select_cpu_latency_ns: avg_select_latency / total_samples,
            avg_enqueue_latency_ns: avg_enqueue_latency / total_samples,
            best_config: data.best_config,
            best_score: data.best_score,
            config_performances: config_perf,
        })
    }

    /// Convert config to hashable key for grouping
    fn config_to_key(config: &SchedulerConfig) -> String {
        format!(
            "s{}_l{}_i{}_m{}_M{}_a{}_S{}_P{}",
            config.slice_us,
            config.slice_lag_us,
            config.input_window_us,
            config.mig_window_ms,
            config.mig_max,
            config.mm_affinity as u8,
            config.avoid_smt as u8,
            config.preferred_idle_scan as u8,
        )
    }
}

/// Summary statistics for a game
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct GameSummary {
    pub sample_count: usize,
    pub avg_cpu_util: f64,
    pub avg_select_cpu_latency_ns: f64,
    pub avg_enqueue_latency_ns: f64,
    pub best_config: Option<SchedulerConfig>,
    pub best_score: Option<f64>,
    pub config_performances: HashMap<String, f64>,
}

impl Drop for MLCollector {
    fn drop(&mut self) {
        // Save any pending samples on shutdown
        if !self.samples.is_empty() {
            // Use current game or default "system" profile
            let game = self.current_game.clone().unwrap_or_else(|| GameInfo {
                tgid: 0,
                name: "system".to_string(),
                is_wine: false,
                is_steam: false,
            });

            log::info!("ML: Saving {} pending samples on shutdown for '{}'", self.samples.len(), game.name);
            if let Err(e) = self.save_samples(&game) {
                log::warn!("ML: Failed to save final samples: {}", e);
            } else {
                log::info!("ML: Successfully saved samples for {}", game.name);
            }
        } else {
            log::info!("ML: No samples to save on shutdown");
        }
    }
}
