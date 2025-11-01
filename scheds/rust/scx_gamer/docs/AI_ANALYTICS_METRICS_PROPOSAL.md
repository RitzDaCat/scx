# AI Analytics Metrics Proposal

**Purpose:** Add metrics optimized for AI pattern detection, performance analysis, and scheduler optimization

**Target Use Case:** Enable AI assistants to:
- Detect scheduling patterns and anomalies
- Correlate scheduler decisions with game performance
- Identify optimization opportunities
- Learn from historical data
- Predict performance outcomes

---

## Category 1: Distribution & Variance Metrics (Critical for Pattern Detection)

### **1.1 Latency Distributions**
**Why:** Averages hide outliers. AI needs percentiles to detect frame-time spikes.

**Missing Metrics:**
- `select_cpu_latency_p10`, `p25`, `p50`, `p75`, `p90`, `p95`, `p99`, `p999` (already have histograms)
- `enqueue_latency_p*` (percentiles)
- `dispatch_latency_p*` (percentiles)
- `deadline_latency_p*` (percentiles)
- `ringbuf_latency_p10`, `p25`, `p75`, `p999` (already have p50/p95/p99)

**BPF Location:** Histograms exist in `prof_select_cpu_hist`, `prof_enqueue_hist`, etc.
**Implementation:** Expose histogram percentiles via API (currently only averages exposed)

---

### **1.2 Frame Time Variance**
**Why:** Frame-time consistency is critical for gaming. High variance = stuttering.

**Missing Metrics:**
- `frame_interval_stddev_ns` - Standard deviation of frame intervals
- `frame_interval_variance_ns` - Variance of frame intervals
- `frame_time_spikes` - Count of frames >2x average interval
- `frame_time_jitter_p95_p50_diff` - Difference between p95 and p50 (jitter indicator)

**BPF Location:** Calculate from `frame_interval_ns` EMA and `frame_count`
**Implementation:** Add variance tracking in BPF or calculate from historical samples

---

### **1.3 Boost Distribution Variance**
**Why:** Show distribution spread, not just cumulative counts.

**Missing Metrics:**
- `boost_distribution_mean` - Average boost level
- `boost_distribution_stddev` - Standard deviation of boost levels
- `boost_effectiveness_ratio` - Performance improvement per boost level

**Implementation:** Calculate from existing `boost_distribution_0-7` counters

---

## Category 2: Temporal Patterns & Trends (Critical for Learning)

### **2.1 Historical Time-Series Windows**
**Why:** AI needs temporal context to detect patterns (e.g., "migrations spike during load screens").

**Missing Metrics:**
- `migrations_last_10s` - Migration count in last 10 seconds (rolling window)
- `migrations_last_60s` - Migration count in last 60 seconds
- `cpu_util_trend` - CPU utilization trend (increasing/decreasing/stable)
- `frame_rate_trend` - Frame rate trend (increasing/decreasing/stable)
- `classification_changes_last_10s` - Count of thread reclassifications

**Implementation:** Add rolling-window accumulators in BPF or calculate from historical samples

---

### **2.2 State Transition Tracking**
**Why:** Detect when scheduler state changes (game swap, classification changes, boost triggers).

**Missing Metrics:**
- `last_game_swap_timestamp` - When foreground game changed
- `last_classification_change_timestamp` - When thread classification changed
- `last_boost_trigger_timestamp` - When boost was triggered
- `classification_stability_score` - How stable classifications are (lower = more changes)

**BPF Location:** Track timestamps in BPF, expose via API

---

### **2.3 Pattern Detection Indicators**
**Why:** Pre-computed patterns help AI identify workload characteristics.

**Missing Metrics:**
- `is_load_screen_active` - High I/O, low GPU activity = loading screen
- `is_menu_active` - Low CPU, high input rate = menu
- `is_combat_active` - High input rate, high GPU, high CPU = combat
- `workload_phase` - Enum: "loading", "menu", "combat", "idle", "unknown"

**Implementation:** Heuristic-based detection from existing metrics

---

## Category 3: Per-Thread Type Aggregates (Critical for Correlation)

### **3.1 Average Metrics Per Thread Classification**
**Why:** AI can correlate thread behavior with performance (e.g., "GPU threads with high exec_avg correlate with stuttering").

**Missing Metrics:**
- `avg_exec_avg_per_input_handler` - Average exec_avg for input handlers
- `avg_exec_avg_per_gpu_submit` - Average exec_avg for GPU threads
- `avg_wakeup_freq_per_input_handler` - Average wakeup frequency
- `avg_wakeup_freq_per_gpu_submit` - Average wakeup frequency
- `avg_chain_boost_per_input_handler` - Average chain boost depth
- `avg_preferred_core_hits_per_gpu` - Cache hit rate for GPU threads

**BPF Location:** Aggregate from `task_ctx` entries (requires iteration or separate counters)
**Implementation:** Add per-classification aggregators in BPF or calculate from samples

---

### **3.2 Thread Type Distribution**
**Why:** Show workload composition (e.g., "50% GPU threads, 30% background").

**Missing Metrics:**
- `thread_classification_distribution` - JSON object with percentages:
  ```json
  {
    "input_handler_pct": 5.2,
    "gpu_submit_pct": 45.8,
    "game_audio_pct": 12.1,
    "background_pct": 37.0
  }
  ```
- `total_classified_threads` - Sum of all classification counts
- `unclassified_threads_estimate` - Estimated unclassified threads

**Implementation:** Calculate from existing classification counters

---

## Category 4: CPU-Level Metrics (Critical for Load Balancing Analysis)

### **4.1 Per-CPU Statistics**
**Why:** Detect CPU hot-spots, imbalance, or affinity patterns.

**Missing Metrics:**
- `cpu_utilization_per_cpu` - Array of CPU utilization per CPU (0-1024)
- `cpu_classification_counts` - Per-CPU breakdown:
  ```json
  {
    "cpu_0": {"input_handler": 2, "gpu_submit": 5, ...},
    "cpu_1": {"input_handler": 1, "gpu_submit": 3, ...}
  }
  ```
- `cpu_migration_source_counts` - How many threads migrated FROM each CPU
- `cpu_migration_destination_counts` - How many threads migrated TO each CPU
- `cpu_load_balance_score` - Standard deviation of CPU utilization (lower = better)

**BPF Location:** Aggregate from `cpu_ctx` entries (requires iteration)
**Implementation:** Add per-CPU aggregators or expose `cpu_ctx` map via API

---

### **4.2 CPU Affinity Patterns**
**Why:** Track where threads prefer to run (cache affinity effectiveness).

**Missing Metrics:**
- `avg_threads_per_cpu` - Average threads per CPU
- `cpu_affinity_hit_rate` - % of threads running on preferred CPU
- `smt_contention_rate` - % of time SMT siblings are both busy
- `physical_core_utilization` - Utilization of physical cores vs hyperthreads

**BPF Location:** Calculate from migration patterns and CPU context
**Implementation:** Add affinity tracking counters

---

## Category 5: Correlation & Effectiveness Metrics (Critical for Optimization)

### **5.1 Boost Effectiveness**
**Why:** Measure if boosts actually improve performance.

**Missing Metrics:**
- `boost_effectiveness_by_level` - Performance improvement per boost level:
  ```json
  {
    "boost_0": {"avg_latency_ns": 5000, "p99_latency_ns": 10000},
    "boost_7": {"avg_latency_ns": 2000, "p99_latency_ns": 5000}
  }
  ```
- `deadline_miss_rate_by_boost` - Deadline miss rate per boost level
- `migration_rate_by_boost` - Migration rate per boost level

**Implementation:** Track performance metrics grouped by boost level

---

### **5.2 Classification Accuracy**
**Why:** Measure detection reliability (false positives/negatives).

**Missing Metrics:**
- `classification_confidence_scores` - Per-classification confidence:
  ```json
  {
    "input_handler": {"detected": 5, "fentry_matches": 3, "name_matches": 2, "confidence": 0.85},
    "gpu_submit": {"detected": 12, "fentry_matches": 10, "name_matches": 2, "confidence": 0.92}
  }
  ```
- `false_positive_rate` - Incorrect classifications / total classifications
- `detection_method_distribution` - % detected via fentry vs name vs runtime pattern

**BPF Location:** Use existing diagnostic counters (`nr_*_fentry_matches`, `nr_*_name_matches`)
**Implementation:** Calculate from existing diagnostic counters

---

### **5.3 Migration Effectiveness**
**Why:** Measure if migrations improve or hurt performance.

**Missing Metrics:**
- `migration_performance_delta` - Performance before vs after migration
- `migration_cache_miss_rate` - Cache miss rate after migration
- `migration_success_rate` - % of migrations that improved performance
- `migration_cooldown_effectiveness` - Performance impact of cooldown

**Implementation:** Track performance metrics before/after migrations

---

## Category 6: Workload Characteristics (Critical for Pattern Recognition)

### **6.1 Workload Signature**
**Why:** AI can identify game/workload type from characteristics.

**Missing Metrics:**
- `workload_thread_density` - Threads per CPU core
- `workload_cpu_intensity` - Average CPU usage per thread
- `workload_io_intensity` - I/O operations per second
- `workload_network_intensity` - Network operations per second
- `workload_gpu_intensity` - GPU submit frequency
- `workload_input_intensity` - Input events per second

**Implementation:** Calculate from existing metrics

---

### **6.2 Resource Utilization Patterns**
**Why:** Identify bottlenecks (CPU-bound, GPU-bound, I/O-bound).

**Missing Metrics:**
- `cpu_bound_score` - 0-100, how CPU-bound the workload is
- `gpu_bound_score` - 0-100, how GPU-bound the workload is
- `io_bound_score` - 0-100, how I/O-bound the workload is
- `bottleneck_indicator` - Primary bottleneck: "cpu", "gpu", "io", "network", "none"

**Implementation:** Heuristic-based calculation from existing metrics

---

## Category 7: Anomaly Detection Indicators (Critical for Troubleshooting)

### **7.1 Anomaly Scores**
**Why:** Pre-computed anomaly scores help AI identify issues quickly.

**Missing Metrics:**
- `stutter_score` - 0-100, likelihood of stuttering (based on frame time variance)
- `input_lag_score` - 0-100, likelihood of input lag (based on input window + latency)
- `migration_chaos_score` - 0-100, excessive migrations (based on migration rate)
- `classification_instability_score` - 0-100, unstable classifications
- `cpu_contention_score` - 0-100, CPU saturation/contention

**Implementation:** Heuristic-based calculation from existing metrics

---

### **7.2 Alert Conditions**
**Why:** Structured alerts help AI prioritize issues.

**Missing Metrics:**
- `active_alerts` - Array of active alerts:
  ```json
  [
    {"type": "ring_buffer_overflow", "severity": "high", "count": 150},
    {"type": "deadline_misses", "severity": "medium", "count": 25},
    {"type": "classification_instability", "severity": "low", "count": 3}
  ]
  ```
- `alert_history` - Recent alerts with timestamps

**Implementation:** Calculate from existing metrics (already have some in TUI)

---

## Category 8: Performance Predictors (Critical for Proactive Optimization)

### **8.1 Predictive Metrics**
**Why:** Help AI predict performance issues before they occur.

**Missing Metrics:**
- `predicted_frame_time_ns` - Predicted next frame time (based on trends)
- `predicted_cpu_util` - Predicted CPU utilization (based on trends)
- `predicted_migration_rate` - Predicted migration rate
- `performance_trend` - "improving", "degrading", "stable"

**Implementation:** Simple trend analysis from historical samples

---

### **8.2 Optimization Recommendations**
**Why:** Pre-computed recommendations help AI suggest improvements.

**Missing Metrics:**
- `recommended_slice_us` - Optimal slice based on workload
- `recommended_mig_max` - Optimal migration limit
- `recommended_boost_levels` - Optimal boost distribution
- `optimization_opportunities` - Array of optimization suggestions

**Implementation:** Heuristic-based recommendations from metrics

---

## Implementation Priority

### **P0 - Critical for AI Analysis (Implement First)**
1. ✅ Distribution metrics (latency percentiles) - Histograms exist, need to expose
2. ✅ Temporal patterns (rolling windows) - Easy to add
3. ✅ Per-thread type aggregates - Moderate complexity
4. ✅ Classification confidence scores - Use existing diagnostic counters

### **P1 - High Value for Pattern Detection**
5. CPU-level metrics - Requires per-CPU aggregation
6. Boost effectiveness - Requires performance tracking per boost level
7. Workload characteristics - Heuristic-based calculations
8. Anomaly detection indicators - Heuristic-based calculations

### **P2 - Nice to Have**
9. Migration effectiveness - Requires before/after tracking
10. Predictive metrics - Trend analysis
11. Optimization recommendations - Heuristic-based

---

## API Design Considerations

### **Efficient Aggregation**
- Pre-aggregate in BPF where possible (avoid iterating task_ctx in userspace)
- Use rolling windows for temporal metrics (O(1) updates)
- Cache expensive calculations (percentiles, distributions)

### **Historical Data**
- Store last N samples in userspace (circular buffer)
- Expose via `/api/metrics/history?window=60s` endpoint
- Enable AI to query historical patterns

### **Structured Output**
- Use nested JSON for related metrics (e.g., `cpu_stats: {cpu_0: {...}, cpu_1: {...}}`)
- Include metadata (timestamps, confidence scores, units)
- Provide normalized values (0-1, percentages) for easier ML processing

---

## Example API Response Enhancement

```json
{
  "current": { /* existing metrics */ },
  "distributions": {
    "select_cpu_latency": {"p10": 150, "p50": 200, "p95": 500, "p99": 800},
    "frame_interval": {"mean": 16666666, "stddev": 500000, "variance": 250000000000}
  },
  "temporal": {
    "migrations_last_10s": 45,
    "cpu_util_trend": "increasing",
    "frame_rate_trend": "stable"
  },
  "per_thread_type": {
    "input_handler": {"avg_exec_avg": 500000, "avg_wakeup_freq": 1000},
    "gpu_submit": {"avg_exec_avg": 2000000, "avg_wakeup_freq": 144}
  },
  "cpu_level": {
    "cpu_0": {"util": 850, "threads": {"input_handler": 1, "gpu_submit": 3}},
    "cpu_1": {"util": 720, "threads": {"input_handler": 0, "gpu_submit": 2}}
  },
  "correlations": {
    "boost_effectiveness": {"boost_7": {"latency_reduction_pct": 60}},
    "classification_confidence": {"input_handler": 0.85, "gpu_submit": 0.92}
  },
  "anomalies": {
    "stutter_score": 25,
    "input_lag_score": 10,
    "active_alerts": [
      {"type": "high_migration_rate", "severity": "medium"}
    ]
  }
}
```

---

## Next Steps

1. **Expose existing histogram percentiles** (P0 - easy win)
2. **Add rolling-window temporal metrics** (P0 - moderate effort)
3. **Calculate per-thread-type aggregates** (P0 - requires BPF changes)
4. **Add CPU-level aggregation** (P1 - requires BPF changes)
5. **Implement anomaly detection heuristics** (P1 - userspace calculation)

