// SPDX-License-Identifier: GPL-2.0
//
// scx_gamer: Unified ML Performance Scoring
// Copyright (c) 2025 RitzDaCat
//
// Centralized performance scoring logic for ML-based parameter optimization.
// Used by both ml_collect and ml_autotune to ensure consistent evaluation.

use crate::ml_collect::{PerformanceSample, MetricsSample};

/// Calculate performance score for ML optimization (higher is better).
///
/// **Scoring Formula** (Frame timing removed - focus on scheduler metrics):
/// - **Primary (10x weight)**: Scheduler latency (select_cpu, enqueue, dispatch)
/// - **Secondary (5x weight)**: Cache efficiency (mm_hint hits, direct dispatch)
/// - **Tertiary (2x weight)**: Migration control (blocking rate)
///
/// **Total Score:**
/// ```
/// total = latency_score * 10.0     // Scheduler efficiency is critical
///       + cache_score * 5.0         // Cache locality matters
///       + migration_score * 2.0     // Migration control
/// ```
///
/// Lower latency + better cache hits + fewer migration blocks = higher score
pub fn calculate_performance_score(samples: &[PerformanceSample]) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }

    let mut total_score = 0.0;

    for sample in samples {
        let m = &sample.metrics;

        // **PRIMARY METRIC**: Scheduler latency (internal efficiency)
        let latency_score = calculate_latency_score(m);

        // **SECONDARY METRICS**: Cache efficiency and migration control
        let cache_score = calculate_cache_score(m);
        let migration_score = calculate_migration_score(m);

        // **COMBINED SCORE** with reweighted priorities (no frame data)
        total_score += latency_score * 10.0     // Latency is now primary
                     + cache_score * 5.0        // Cache efficiency
                     + migration_score * 2.0;   // Migration control
    }

    total_score / samples.len() as f64
}

/// Calculate scheduler latency score (0-100, higher is better)
fn calculate_latency_score(m: &MetricsSample) -> f64 {
    if m.latency_select_cpu_avg_ns == 0 {
        return 0.0;
    }

    // Inverse latency: Lower is better, so invert (capped at 10µs)
    10000.0 / (m.latency_select_cpu_avg_ns as f64).max(100.0)
}

/// Calculate cache efficiency score (0-2, higher is better)
fn calculate_cache_score(m: &MetricsSample) -> f64 {
    m.mm_hint_hit_rate + m.direct_dispatch_rate
}

/// Calculate migration control score (0-1, higher is better)
fn calculate_migration_score(m: &MetricsSample) -> f64 {
    1.0 - m.migration_block_rate
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ml_collect::{PerformanceSample, SchedulerConfig, GameInfo};

    #[test]
    fn test_scoring_consistency() {
        // Create sample
        let config = SchedulerConfig {
            slice_us: 10,
            slice_lag_us: 20000,
            input_window_us: 2000,
            mig_window_ms: 50,
            mig_max: 3,
            mm_affinity: true,
            avoid_smt: false,
            preferred_idle_scan: true,
            enable_numa: false,
            wakeup_timer_us: 500,
        };

        let metrics = MetricsSample {
            cpu_util_pct: 50.0,
            latency_select_cpu_avg_ns: 1000,
            latency_enqueue_avg_ns: 500,
            latency_dispatch_avg_ns: 300,
            latency_deadline_avg_ns: 100,
            enqueues_per_sec: 10000.0,
            dispatches_per_sec: 9500.0,
            migration_block_rate: 0.2,
            mm_hint_hit_rate: 0.6,
            direct_dispatch_rate: 0.7,
            input_handler_count: 4,
            gpu_submit_count: 2,
            compositor_count: 1,
            network_count: 3,
        };

        let sample = PerformanceSample {
            timestamp: 1234567890,
            config,
            metrics,
            game: GameInfo {
                tgid: 12345,
                name: "test.exe".to_string(),
                is_wine: true,
                is_steam: true,
            },
        };

        let score = calculate_performance_score(&[sample]);

        // Reasonable config should score > 0 (scheduler metrics only now)
        assert!(score > 0.0, "Score should be positive for valid config: {}", score);
    }
}
