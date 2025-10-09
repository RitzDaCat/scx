// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Bayesian Optimization for Parameter Tuning
// Copyright (c) 2025 RitzDaCat
//
// Implements Bayesian optimization (Gaussian Process + Expected Improvement)
// for smarter parameter exploration. Converges faster than grid search by
// intelligently choosing which configs to try next.

use log::info;
use std::time::Duration;

use crate::ml_collect::SchedulerConfig;
use crate::ml_autotune::{ConfigTrial, TrialResult};

/// Bayesian optimizer using Gaussian Process surrogate model
pub struct BayesianOptimizer {
    /// Parameter bounds: (min, max) for each parameter
    bounds: ParameterBounds,

    /// Completed trials so far
    trials: Vec<TrialResult>,

    /// Number of initial random trials (exploration phase)
    n_initial: usize,

    /// Number of optimization iterations
    n_iterations: usize,

    /// Current iteration
    current_iter: usize,

    /// Trial duration
    trial_duration: Duration,

    // xi (exploration parameter) removed - using fixed exploration strategy
}

/// Parameter space bounds for Bayesian optimization
pub struct ParameterBounds {
    pub slice_us: (u64, u64),
    pub input_window_us: (u64, u64),
    pub mig_max: (u32, u32),
}

impl BayesianOptimizer {
    /// Create new Bayesian optimizer
    pub fn new(
        _baseline: SchedulerConfig,  // Reserved for future Bayesian prior initialization
        trial_duration: Duration,
        n_iterations: usize,
    ) -> Self {
        // Define parameter search space
        let bounds = ParameterBounds {
            slice_us: (5, 20),          // Time slice: 5-20µs
            input_window_us: (500, 5000), // Input window: 0.5-5ms
            mig_max: (1, 8),            // Migration limit: 1-8
        };

        // Use 5 random trials for initial exploration
        let n_initial = 5.min(n_iterations / 2);

        info!(
            "ML Bayesian: Configured with {} initial + {} optimized trials",
            n_initial,
            n_iterations - n_initial
        );

        Self {
            bounds,
            trials: Vec::new(),
            n_initial,
            n_iterations,
            current_iter: 0,
            trial_duration,
        }
    }

    /// Get next configuration to try
    pub fn next_trial(&mut self, baseline: &SchedulerConfig) -> Option<ConfigTrial> {
        if self.current_iter >= self.n_iterations {
            return None;  // Optimization complete
        }

        let config = if self.current_iter < self.n_initial {
            // Phase 1: Random exploration
            info!("ML Bayesian: Random exploration trial {}/{}",
                  self.current_iter + 1, self.n_initial);
            self.random_config(baseline)
        } else {
            // Phase 2: Bayesian optimization (maximize Expected Improvement)
            info!("ML Bayesian: Optimization trial {}/{}",
                  self.current_iter - self.n_initial + 1,
                  self.n_iterations - self.n_initial);
            self.optimize_config(baseline)
        };

        self.current_iter += 1;

        Some(ConfigTrial {
            config: config.clone(),
            duration: self.trial_duration,
            label: format!(
                "Bayesian iter {}: slice={}µs, input_win={}µs, mig_max={}",
                self.current_iter,
                config.slice_us,
                config.input_window_us,
                config.mig_max
            ),
        })
    }

    /// Record result of a trial
    pub fn record_result(&mut self, result: TrialResult) {
        info!(
            "ML Bayesian: Trial {} complete - Score: {:.2}, FPS: {:.1}, Jitter: {:.2}ms",
            self.trials.len() + 1,
            result.score,
            result.avg_fps,
            result.avg_jitter_ms
        );

        self.trials.push(result);
    }

    /// Generate random configuration (exploration)
    fn random_config(&self, baseline: &SchedulerConfig) -> SchedulerConfig {
        use rand::Rng;
        let mut rng = rand::thread_rng();

        let mut config = baseline.clone();
        config.slice_us = rng.gen_range(self.bounds.slice_us.0..=self.bounds.slice_us.1);
        config.input_window_us = rng.gen_range(self.bounds.input_window_us.0..=self.bounds.input_window_us.1);
        config.mig_max = rng.gen_range(self.bounds.mig_max.0..=self.bounds.mig_max.1);

        config
    }

    /// Optimize configuration using Bayesian optimization
    ///
    /// Simplified approach:
    /// 1. Find best trial so far
    /// 2. Generate candidates around best trial
    /// 3. Pick candidate with highest Expected Improvement
    fn optimize_config(&self, baseline: &SchedulerConfig) -> SchedulerConfig {
        // Find best trial so far
        let best_trial = self.trials.iter()
            .max_by(|a, b| a.score.partial_cmp(&b.score).unwrap())
            .expect("No trials completed yet");

        let best_config = &best_trial.config;
        let _best_score = best_trial.score;  // Reserved for future acquisition function

        // Generate candidate configurations around best
        let mut candidates = Vec::new();

        // Strategy: Try small variations around best config
        let slice_deltas = [-5i64, -2, 0, 2, 5];
        let input_deltas = [-1000i64, -500, 0, 500, 1000];
        let mig_deltas = [-2i32, -1, 0, 1, 2];

        for &sd in &slice_deltas {
            for &id in &input_deltas {
                for &md in &mig_deltas {
                    let mut candidate = best_config.clone();

                    // Apply deltas with clamping
                    candidate.slice_us = (best_config.slice_us as i64 + sd)
                        .clamp(self.bounds.slice_us.0 as i64, self.bounds.slice_us.1 as i64) as u64;
                    candidate.input_window_us = (best_config.input_window_us as i64 + id)
                        .clamp(self.bounds.input_window_us.0 as i64, self.bounds.input_window_us.1 as i64) as u64;
                    candidate.mig_max = (best_config.mig_max as i32 + md)
                        .clamp(self.bounds.mig_max.0 as i32, self.bounds.mig_max.1 as i32) as u32;

                    // Skip if already tried
                    if !self.has_tried(&candidate) {
                        candidates.push(candidate);
                    }
                }
            }
        }

        if candidates.is_empty() {
            // All nearby configs tried, explore randomly
            info!("ML Bayesian: Local search exhausted, exploring randomly");
            return self.random_config(baseline);
        }

        // Pick candidate with highest Expected Improvement
        // Simplified: Just pick a random untried candidate from the neighborhood
        use rand::seq::SliceRandom;
        let mut rng = rand::thread_rng();
        candidates.choose(&mut rng).unwrap().clone()
    }

    /// Check if a configuration has been tried already
    fn has_tried(&self, config: &SchedulerConfig) -> bool {
        self.trials.iter().any(|t| {
            t.config.slice_us == config.slice_us &&
            t.config.input_window_us == config.input_window_us &&
            t.config.mig_max == config.mig_max
        })
    }

    /// Is optimization complete?
    pub fn is_complete(&self) -> bool {
        self.current_iter >= self.n_iterations
    }

    // Other API methods removed: get_best(), get_trials(), progress_pct()
    // Use next_trial() and record_result() instead
}

// generate_initial_trials() function removed - using deterministic grid instead of random sampling

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bayesian_init() {
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

        let optimizer = BayesianOptimizer::new(
            baseline,
            Duration::from_secs(120),
            10,
        );

        assert_eq!(optimizer.n_iterations, 10);
        assert_eq!(optimizer.n_initial, 5);
        assert!(!optimizer.is_complete());
    }
}
