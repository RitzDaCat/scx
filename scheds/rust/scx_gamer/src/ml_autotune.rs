// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Automated ML Parameter Tuning
// Copyright (c) 2025 RitzDaCat
//
// Automatically explores the scheduler parameter space to find optimal configuration
// for the current game without manual intervention.

use anyhow::Result;
use log::{info, warn};
use std::time::{Duration, Instant};

use crate::ml_collect::{SchedulerConfig, PerformanceSample};

/// Parameter combination to test
#[derive(Debug, Clone)]
pub struct ConfigTrial {
    pub config: SchedulerConfig,
    pub duration: Duration,
    pub label: String,  // Human-readable description
}

/// Result of a single trial
#[derive(Debug, Clone)]
pub struct TrialResult {
    pub config: SchedulerConfig,
    pub score: f64,
    // samples removed - takes too much memory, score/avg metrics are sufficient
    pub avg_fps: f64,
    pub avg_jitter_ms: f64,
    pub avg_latency_ns: u64,
}

// AutotuneMode enum removed - mode is implicit in constructor choice
// (new_grid_search vs new_bayesian)

/// Automated tuning orchestrator
pub struct MLAutotuner {
    // mode field removed - determined by which constructor (new_grid_search vs new_bayesian) is called
    // AutotuneMode enum preserved for potential future use

    /// Trials to run (populated at start, only used for GridSearch)
    trials: Vec<ConfigTrial>,

    /// Current trial index
    current_trial: usize,

    /// When the current trial started
    trial_start_time: Option<Instant>,

    /// Results from completed trials
    results: Vec<TrialResult>,

    /// Best configuration found so far
    best_config: Option<SchedulerConfig>,
    best_score: f64,

    /// Current trial samples (accumulated)
    current_samples: Vec<PerformanceSample>,

    /// Total tuning duration limit
    max_duration: Duration,

    /// Start time of entire tuning session
    session_start: Instant,

    /// Bayesian optimizer (if using Bayesian mode)
    bayesian: Option<crate::ml_bayesian::BayesianOptimizer>,

    /// Baseline config for Bayesian mode
    baseline_config: SchedulerConfig,

    // trial_duration removed - stored in trials instead
}

impl MLAutotuner {
    /// Create new autotuner with grid search
    pub fn new_grid_search(
        baseline_config: SchedulerConfig,
        trial_duration: Duration,
        max_duration: Duration,
    ) -> Self {
        let trials = Self::generate_grid_trials(baseline_config.clone(), trial_duration);

        info!("ML Autotune: Grid search with {} trials ({:.0}s each, {:.0}s total)",
              trials.len(),
              trial_duration.as_secs_f64(),
              trials.len() as f64 * trial_duration.as_secs_f64());

        Self {
            trials,
            current_trial: 0,
            trial_start_time: None,
            results: Vec::new(),
            best_config: None,
            best_score: 0.0,
            current_samples: Vec::new(),
            max_duration,
            session_start: Instant::now(),
            bayesian: None,
            baseline_config,
        }
    }

    /// Create new autotuner with Bayesian optimization
    pub fn new_bayesian(
        baseline_config: SchedulerConfig,
        trial_duration: Duration,
        max_duration: Duration,
    ) -> Self {
        let n_iterations = (max_duration.as_secs() / trial_duration.as_secs()) as usize;
        let bayesian = Some(crate::ml_bayesian::BayesianOptimizer::new(
            baseline_config.clone(),
            trial_duration,
            n_iterations,
        ));

        info!("ML Autotune: Bayesian optimization with {} iterations ({:.0}s each)",
              n_iterations,
              trial_duration.as_secs_f64());

        Self {
            trials: Vec::new(),  // Not used in Bayesian mode (trials generated dynamically)
            current_trial: 0,
            trial_start_time: None,
            results: Vec::new(),
            best_config: None,
            best_score: 0.0,
            current_samples: Vec::new(),
            max_duration,
            session_start: Instant::now(),
            bayesian,
            baseline_config,
        }
    }

    /// Generate grid of parameter combinations to test
    fn generate_grid_trials(baseline: SchedulerConfig, duration: Duration) -> Vec<ConfigTrial> {
        let mut trials = Vec::new();

        // Phase 1: Coarse grid search (main parameters)
        let slice_values = [5, 10, 15, 20];
        let input_window_values = [1000, 2000, 3000];
        let mig_max_values = [1, 3, 5];

        for &slice in &slice_values {
            for &input_win in &input_window_values {
                for &mig in &mig_max_values {
                    let mut config = baseline.clone();
                    config.slice_us = slice;
                    config.input_window_us = input_win;
                    config.mig_max = mig;

                    trials.push(ConfigTrial {
                        config: config.clone(),
                        duration,
                        label: format!(
                            "slice={}µs, input_win={}µs, mig_max={}",
                            slice, input_win, mig
                        ),
                    });
                }
            }
        }

        // Limit to reasonable number of trials (max 12-15 for a 15-min session)
        if trials.len() > 12 {
            // Sample evenly spaced trials
            let step = trials.len() / 12;
            trials = trials.into_iter().step_by(step).collect();
        }

        trials
    }

    /// Check if current trial should end and switch to next
    pub fn should_switch_trial(&self) -> bool {
        if let Some(start) = self.trial_start_time {
            if self.current_trial >= self.trials.len() {
                return false;  // All trials complete
            }

            let trial = &self.trials[self.current_trial];
            start.elapsed() >= trial.duration
        } else {
            true  // No trial running, should start first one
        }
    }

    /// Get the next configuration to try
    /// Returns None if all trials are complete
    pub fn next_trial(&mut self) -> Option<SchedulerConfig> {
        // Finalize current trial if running
        if self.trial_start_time.is_some() && !self.current_samples.is_empty() {
            self.finalize_current_trial();
        }

        // Check if we've exceeded max duration
        if self.session_start.elapsed() >= self.max_duration {
            info!("ML Autotune: Max duration reached, stopping");
            return None;
        }

        // Bayesian optimization mode
        if let Some(ref mut bay) = self.bayesian {
            if bay.is_complete() {
                info!("ML Autotune: Bayesian optimization complete");
                return None;
            }

            if let Some(trial) = bay.next_trial(&self.baseline_config) {
                self.trial_start_time = Some(Instant::now());
                self.current_samples.clear();

                info!("ML Autotune: {}", trial.label);

                return Some(trial.config);
            } else {
                return None;
            }
        }

        // Grid search mode
        if self.current_trial >= self.trials.len() {
            info!("ML Autotune: All trials complete");
            return None;
        }

        // Start next trial
        let trial = &self.trials[self.current_trial];
        self.trial_start_time = Some(Instant::now());
        self.current_samples.clear();

        info!("ML Autotune: Starting trial {}/{}: {}",
              self.current_trial + 1,
              self.trials.len(),
              trial.label);

        Some(trial.config.clone())
    }

    /// Record a sample for the current trial
    pub fn record_sample(&mut self, sample: PerformanceSample) {
        self.current_samples.push(sample);
    }

    /// Finalize current trial and compute score
    fn finalize_current_trial(&mut self) {
        if self.current_samples.is_empty() {
            warn!("ML Autotune: Trial {} had no samples, skipping", self.current_trial);
            self.current_trial += 1;
            return;
        }

        let trial = &self.trials[self.current_trial];

        // Calculate aggregate metrics
        let avg_latency = self.current_samples.iter()
            .map(|s| s.metrics.latency_select_cpu_avg_ns)
            .sum::<u64>() / self.current_samples.len() as u64;

        // Calculate performance score (scheduler metrics only)
        let score = Self::calculate_trial_score(&self.current_samples);

        let result = TrialResult {
            config: trial.config.clone(),
            score,
            avg_fps: 0.0,  // Unused (frame timing removed)
            avg_jitter_ms: 0.0,  // Unused (frame timing removed)
            avg_latency_ns: avg_latency,
        };

        info!(
            "ML Autotune: Trial {}/{} complete - Score: {:.2}, Latency: {}ns",
            self.current_trial + 1,
            self.trials.len(),
            score,
            avg_latency
        );

        // Update best config if this trial was better
        if score > self.best_score {
            self.best_score = score;
            self.best_config = Some(trial.config.clone());
            info!("ML Autotune: ✓ NEW BEST CONFIG (score: {:.2})", score);
        }

        self.results.push(result.clone());

        // Update Bayesian optimizer if active
        if let Some(ref mut bay) = self.bayesian {
            bay.record_result(result);
        }

        self.current_trial += 1;
    }

    /// Calculate performance score for a set of samples
    /// Uses centralized scoring logic from ml_scoring module
    fn calculate_trial_score(samples: &[PerformanceSample]) -> f64 {
        crate::ml_scoring::calculate_performance_score(samples)
    }

    /// Get the best configuration found
    pub fn get_best_config(&self) -> Option<(SchedulerConfig, f64)> {
        self.best_config.as_ref().map(|c| (c.clone(), self.best_score))
    }

    // is_complete() and progress_pct() removed - use should_switch_trial() and generate_report() instead

    /// Generate final report
    pub fn generate_report(&self) -> String {
        let mut report = String::new();

        report.push_str("\n╔═══════════════════════════════════════════════════════════╗\n");
        report.push_str("║        ML AUTOTUNE SESSION COMPLETE                       ║\n");
        report.push_str("╚═══════════════════════════════════════════════════════════╝\n\n");

        report.push_str(&format!(
            "Total trials: {}\n",
            self.results.len()
        ));
        report.push_str(&format!(
            "Session duration: {:.1}s\n\n",
            self.session_start.elapsed().as_secs_f64()
        ));

        // Top 3 configurations
        report.push_str("Top 3 Configurations:\n");
        report.push_str("═══════════════════════════════════════════════════════════\n");

        let mut sorted_results = self.results.clone();
        sorted_results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());

        for (i, result) in sorted_results.iter().take(3).enumerate() {
            report.push_str(&format!(
                "{}. Score: {:.2}  FPS: {:.1}  Jitter: {:.2}ms  Latency: {}ns\n",
                i + 1,
                result.score,
                result.avg_fps,
                result.avg_jitter_ms,
                result.avg_latency_ns
            ));
            report.push_str(&format!(
                "   --slice-us {} --input-window-us {} --mig-max {}\n",
                result.config.slice_us,
                result.config.input_window_us,
                result.config.mig_max
            ));
            if result.config.mm_affinity { report.push_str("   --mm-affinity\n"); }
            if result.config.avoid_smt { report.push_str("   --avoid-smt\n"); }
            report.push('\n');
        }

        // Best config command
        if let Some((best, score)) = self.get_best_config() {
            report.push_str("╔═══════════════════════════════════════════════════════════╗\n");
            report.push_str("║  RECOMMENDED CONFIGURATION                                ║\n");
            report.push_str("╚═══════════════════════════════════════════════════════════╝\n\n");
            report.push_str(&format!("Performance Score: {:.2}\n\n", score));
            report.push_str("Command:\n");
            report.push_str(&format!(
                "sudo ./target/release/scx_gamer --stats 1 \\\n  --slice-us {} \\\n  --input-window-us {} \\\n  --mig-max {}",
                best.slice_us,
                best.input_window_us,
                best.mig_max
            ));
            if best.mm_affinity { report.push_str(" \\\n  --mm-affinity"); }
            if best.avoid_smt { report.push_str(" \\\n  --avoid-smt"); }
            report.push_str("\n\n");
        }

        report
    }
}

/// Apply configuration by requesting scheduler restart
///
/// **NOTE**: BPF rodata is immutable after load, so configs can only be changed via restart.
/// This function signals that a restart is needed with the new config.
///
/// The autotune workflow will:
/// 1. Signal scheduler to exit (via return value)
/// 2. Main loop restarts scheduler with new CLI args
/// 3. New config takes effect on next iteration
pub fn apply_config_hot(
    _skel: &mut crate::BpfSkel,
    config: &SchedulerConfig,
) -> Result<()> {
    // Just log the config - actual application happens via scheduler restart
    // The autotune system will need to update CLI opts and restart
    info!(
        "ML Autotune: Next config - slice={}µs, input_win={}µs, mig_max={} (will restart)",
        config.slice_us,
        config.input_window_us,
        config.mig_max
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grid_generation() {
        let baseline = SchedulerConfig {
            slice_us: 10,
            slice_lag_us: 20000,
            input_window_us: 2000,
            mig_window_ms: 50,
            mig_max: 3,
            mm_affinity: false,
            avoid_smt: false,
            preferred_idle_scan: false,
            enable_numa: false,
            wakeup_timer_us: 500,
        };

        let trials = MLAutotuner::generate_grid_trials(baseline, Duration::from_secs(120));

        // Should generate reasonable number of trials (not too many)
        assert!(trials.len() > 0);
        assert!(trials.len() <= 15, "Too many trials: {}", trials.len());

        // All trials should have valid durations
        for trial in &trials {
            assert_eq!(trial.duration, Duration::from_secs(120));
        }
    }
}
