#!/usr/bin/env bash
# Quick script to compare scheduler metrics before/after gaming session

API_URL="http://127.0.0.1:8080/metrics"
BEFORE_FILE="/tmp/scx_metrics_before.json"
AFTER_FILE="/tmp/scx_metrics_after.json"

if [[ ! -f "$BEFORE_FILE" ]]; then
    echo "Error: Before metrics not found. Run this to capture baseline:"
    echo "curl -s $API_URL | jq '{fg_pid, fg_app, fg_cpu_pct, input_handler_threads, gpu_submit_threads, game_audio_threads, system_audio_threads, compositor_threads, network_threads, background_threads, fentry_total_events, fentry_gaming_events, cpu_util, cpu_util_avg, direct, shared, migrations, mig_blocked, sync_wake_fast, idle_pick, mm_hint_hit, ringbuf_overflow_events}' > $BEFORE_FILE"
    exit 1
fi

echo "Capturing current metrics..."
curl -s "$API_URL" | jq '{fg_pid, fg_app, fg_cpu_pct, input_handler_threads, gpu_submit_threads, game_audio_threads, system_audio_threads, compositor_threads, network_threads, background_threads, fentry_total_events, fentry_gaming_events, cpu_util, cpu_util_avg, direct, shared, migrations, mig_blocked, sync_wake_fast, idle_pick, mm_hint_hit, ringbuf_overflow_events}' > "$AFTER_FILE"

echo ""
echo "=================================================================================="
echo "                          METRICS COMPARISON"
echo "=================================================================================="
echo ""

echo "BEFORE (Baseline):"
jq '.' "$BEFORE_FILE"
echo ""
echo "AFTER (After Gaming):"
jq '.' "$AFTER_FILE"
echo ""

echo "=================================================================================="
echo "                          DIFFERENCES"
echo "=================================================================================="
echo ""

# Calculate deltas for numeric fields
echo "Thread Classifications:"
echo "  Input Handler:     $(jq '.input_handler_threads' "$BEFORE_FILE") → $(jq '.input_handler_threads' "$AFTER_FILE")"
echo "  GPU Submit:        $(jq '.gpu_submit_threads' "$BEFORE_FILE") → $(jq '.gpu_submit_threads' "$AFTER_FILE")"
echo "  Game Audio:        $(jq '.game_audio_threads' "$BEFORE_FILE") → $(jq '.game_audio_threads' "$AFTER_FILE")"
echo "  System Audio:      $(jq '.system_audio_threads' "$BEFORE_FILE") → $(jq '.system_audio_threads' "$AFTER_FILE")"
echo "  Compositor:        $(jq '.compositor_threads' "$BEFORE_FILE") → $(jq '.compositor_threads' "$AFTER_FILE")"
echo "  Network:           $(jq '.network_threads' "$BEFORE_FILE") → $(jq '.network_threads' "$AFTER_FILE")"
echo "  Background:        $(jq '.background_threads' "$BEFORE_FILE") → $(jq '.background_threads' "$AFTER_FILE")"
echo ""
echo "Performance Metrics:"
echo "  CPU Util:          $(jq '.cpu_util' "$BEFORE_FILE") → $(jq '.cpu_util' "$AFTER_FILE")"
echo "  CPU Util Avg:      $(jq '.cpu_util_avg' "$BEFORE_FILE") → $(jq '.cpu_util_avg' "$AFTER_FILE")"
echo "  FG CPU %:          $(jq '.fg_cpu_pct' "$BEFORE_FILE")% → $(jq '.fg_cpu_pct' "$AFTER_FILE")%"
echo "  Direct Dispatches: $(jq '.direct' "$BEFORE_FILE") → $(jq '.direct' "$AFTER_FILE")"
echo "  Shared Dispatches: $(jq '.shared' "$BEFORE_FILE") → $(jq '.shared' "$AFTER_FILE")"
echo "  Migrations:        $(jq '.migrations' "$BEFORE_FILE") → $(jq '.migrations' "$AFTER_FILE")"
echo "  Mig Blocked:       $(jq '.mig_blocked' "$BEFORE_FILE") → $(jq '.mig_blocked' "$AFTER_FILE")"
echo ""
echo "Fentry Events:"
echo "  Total:             $(jq '.fentry_total_events' "$BEFORE_FILE") → $(jq '.fentry_total_events' "$AFTER_FILE")"
echo "  Gaming:            $(jq '.fentry_gaming_events' "$BEFORE_FILE") → $(jq '.fentry_gaming_events' "$AFTER_FILE")"
echo ""

