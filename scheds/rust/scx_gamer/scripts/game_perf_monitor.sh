#!/bin/bash
# game_perf_monitor.sh - Delta-Based Game Performance Monitor for scx_gamer
#
# PURPOSE: Measure ACTUAL scheduling performance over a time window
# Unlike cumulative stats, this shows CURRENT scheduler behavior
#
# KEY METRICS (all delta-based):
#   - Wait% per thread (time waiting vs running)
#   - Migrations per second
#   - Context switches per second
#   - PSI (Pressure Stall Information)
#
# USAGE:
#   ./game_perf_monitor.sh                    # Auto-detect game, 10s measurement
#   ./game_perf_monitor.sh --pid 12345        # Specific PID
#   ./game_perf_monitor.sh --duration 30      # 30 second measurement
#   ./game_perf_monitor.sh --continuous       # Continuous monitoring mode
#   ./game_perf_monitor.sh --compare          # Compare two runs (A/B test)

set -euo pipefail

#=============================================================================
# CONFIGURATION
#=============================================================================
DURATION="${DURATION:-10}"
GAME_PID=""
CONTINUOUS=false
COMPARE_MODE=false
BASELINE_FILE="/tmp/scx_gamer_baseline.json"

# Colors for terminal output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

#=============================================================================
# HELPER FUNCTIONS
#=============================================================================

log_info() { echo -e "${BLUE}[INFO]${NC} $*"; }
log_good() { echo -e "${GREEN}[GOOD]${NC} $*"; }
log_warn() { echo -e "${YELLOW}[WARN]${NC} $*"; }
log_bad() { echo -e "${RED}[BAD]${NC} $*"; }

# Find game process automatically
find_game_pid() {
    local patterns=(
        # Blizzard Games (Battle.net)
        "Wow\.exe" "WowClassic\.exe" "WowT\.exe" "WowB\.exe"
        "Overwatch\.exe" "Diablo.*\.exe" "Hearthstone\.exe"
        # Valve Games
        "cs2" "csgo_linux64" "dota2" "hl2_linux"
        # Popular Games
        "Palworld-Win64-Shipping"
        "r5apex" "apex"
        "VALORANT-Win64-Shipping"
        "GenshinImpact" "ZenlessZoneZero"
        "LeagueofLegends\.exe" "League.*\.exe"
        "FFXIV.*\.exe" "ffxiv_dx11\.exe"
        "Elden.*\.exe" "DarkSouls.*\.exe"
        # Generic Wine/Proton
        "wine.*\.exe"
        "proton.*\.exe"
    )
    
    for pattern in "${patterns[@]}"; do
        local pid=$(pgrep -f "$pattern" 2>/dev/null | head -1)
        if [[ -n "$pid" ]]; then
            local comm=$(cat /proc/$pid/comm 2>/dev/null || echo "unknown")
            log_info "Found game: $comm (PID: $pid)"
            echo "$pid"
            return 0
        fi
    done
    
    return 1
}

# Capture thread stats at a point in time
capture_thread_stats() {
    local pid="$1"
    local output_file="$2"
    
    echo "{" > "$output_file"
    echo "  \"timestamp\": $(date +%s%N)," >> "$output_file"
    echo "  \"threads\": {" >> "$output_file"
    
    local first=true
    for tid in $(ls /proc/$pid/task/ 2>/dev/null); do
        local comm=$(cat /proc/$pid/task/$tid/comm 2>/dev/null || continue)
        local schedstat=$(cat /proc/$pid/task/$tid/schedstat 2>/dev/null || continue)
        local sched=$(cat /proc/$pid/task/$tid/sched 2>/dev/null || continue)
        
        local runtime=$(echo $schedstat | awk '{print $1}')
        local wait=$(echo $schedstat | awk '{print $2}')
        local switches=$(echo "$sched" | grep "^nr_switches" | head -1 | awk '{print $3}')
        local migrations=$(echo "$sched" | grep "nr_migrations" | awk '{print $3}')
        local vol_switches=$(echo "$sched" | grep "nr_voluntary_switches" | awk '{print $3}')
        local invol_switches=$(echo "$sched" | grep "nr_involuntary_switches" | awk '{print $3}')
        
        [[ -z "$runtime" ]] && continue
        
        if [[ "$first" == "true" ]]; then
            first=false
        else
            echo "," >> "$output_file"
        fi
        
        cat >> "$output_file" << EOF
    "$tid": {
      "comm": "$comm",
      "runtime_ns": $runtime,
      "wait_ns": ${wait:-0},
      "switches": ${switches:-0},
      "migrations": ${migrations:-0},
      "vol_switches": ${vol_switches:-0},
      "invol_switches": ${invol_switches:-0}
    }
EOF
    done
    
    echo "" >> "$output_file"
    echo "  }" >> "$output_file"
    echo "}" >> "$output_file"
}

# Calculate deltas between two captures
calculate_deltas() {
    local before_file="$1"
    local after_file="$2"
    local duration="$3"
    
    # Parse JSON with basic tools (no jq dependency)
    local before_ts=$(grep '"timestamp"' "$before_file" | grep -o '[0-9]*')
    local after_ts=$(grep '"timestamp"' "$after_file" | grep -o '[0-9]*')
    local actual_duration_ns=$((after_ts - before_ts))
    local actual_duration_s=$(echo "scale=2; $actual_duration_ns / 1000000000" | bc)
    
    echo ""
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                    GAME PERFORMANCE REPORT (${actual_duration_s}s window)                    ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    echo ""
    
    # PSI Snapshot
    echo "┌─────────────────────────────────────────────────────────────────────────────┐"
    echo "│ PRESSURE STALL INFORMATION (PSI)                                            │"
    echo "└─────────────────────────────────────────────────────────────────────────────┘"
    if [[ -f /proc/pressure/cpu ]]; then
        local psi_cpu=$(cat /proc/pressure/cpu | head -1)
        local psi_avg10=$(echo "$psi_cpu" | grep -o 'avg10=[0-9.]*' | cut -d= -f2)
        if (( $(echo "$psi_avg10 < 1" | bc -l) )); then
            echo -e "  CPU:  ${GREEN}${psi_avg10}%${NC} (Good - minimal stalls)"
        elif (( $(echo "$psi_avg10 < 5" | bc -l) )); then
            echo -e "  CPU:  ${YELLOW}${psi_avg10}%${NC} (Warning - some thread stalls)"
        else
            echo -e "  CPU:  ${RED}${psi_avg10}%${NC} (Bad - significant stalls)"
        fi
    fi
    echo ""
    
    # Thread Analysis Header
    echo "┌─────────────────────────────────────────────────────────────────────────────┐"
    echo "│ PER-THREAD ANALYSIS (Delta over ${actual_duration_s}s)                                      │"
    echo "└─────────────────────────────────────────────────────────────────────────────┘"
    echo ""
    printf "%-20s %10s %10s %10s %10s %s\n" "THREAD" "WAIT%" "MIG/s" "SW/s" "INVOL/s" "STATUS"
    printf "%-20s %10s %10s %10s %10s %s\n" "────────────────────" "──────────" "──────────" "──────────" "──────────" "──────────"
    
    # Extract thread data and calculate deltas
    # Using grep/awk since we can't depend on jq
    local tids=$(grep -o '"[0-9]*":' "$before_file" | tr -d '":')
    
    local critical_threads=""
    local gpu_threads=""
    local worker_threads=""
    local other_threads=""
    
    for tid in $tids; do
        # Extract before values
        local before_block=$(sed -n "/$tid/,/}/p" "$before_file" | head -10)
        local after_block=$(sed -n "/$tid/,/}/p" "$after_file" | head -10)
        
        [[ -z "$after_block" ]] && continue
        
        local comm=$(echo "$before_block" | grep '"comm"' | cut -d'"' -f4)
        
        local before_runtime=$(echo "$before_block" | grep '"runtime_ns"' | grep -o '[0-9]*')
        local before_wait=$(echo "$before_block" | grep '"wait_ns"' | grep -o '[0-9]*')
        local before_mig=$(echo "$before_block" | grep '"migrations"' | grep -o '[0-9]*')
        local before_sw=$(echo "$before_block" | grep '"switches"' | grep -o '[0-9]*' | head -1)
        local before_invol=$(echo "$before_block" | grep '"invol_switches"' | grep -o '[0-9]*')
        
        local after_runtime=$(echo "$after_block" | grep '"runtime_ns"' | grep -o '[0-9]*')
        local after_wait=$(echo "$after_block" | grep '"wait_ns"' | grep -o '[0-9]*')
        local after_mig=$(echo "$after_block" | grep '"migrations"' | grep -o '[0-9]*')
        local after_sw=$(echo "$after_block" | grep '"switches"' | grep -o '[0-9]*' | head -1)
        local after_invol=$(echo "$after_block" | grep '"invol_switches"' | grep -o '[0-9]*')
        
        # Calculate deltas
        local delta_runtime=$((after_runtime - before_runtime))
        local delta_wait=$((after_wait - before_wait))
        local delta_mig=$((after_mig - before_mig))
        local delta_sw=$((after_sw - before_sw))
        local delta_invol=$((after_invol - before_invol))
        
        # Skip threads with no activity
        [[ $delta_runtime -lt 1000000 ]] && continue  # < 1ms runtime
        
        # Calculate rates
        local total=$((delta_runtime + delta_wait))
        local wait_pct=$(echo "scale=1; $delta_wait * 100 / $total" | bc 2>/dev/null || echo "0")
        local mig_per_sec=$(echo "scale=1; $delta_mig / $duration" | bc 2>/dev/null || echo "0")
        local sw_per_sec=$(echo "scale=1; $delta_sw / $duration" | bc 2>/dev/null || echo "0")
        local invol_per_sec=$(echo "scale=1; $delta_invol / $duration" | bc 2>/dev/null || echo "0")
        
        # Determine status
        local status=""
        if (( $(echo "$wait_pct > 30" | bc -l) )); then
            status="${RED}HIGH WAIT${NC}"
        elif (( $(echo "$wait_pct > 10" | bc -l) )); then
            status="${YELLOW}MODERATE${NC}"
        elif (( $(echo "$mig_per_sec > 500" | bc -l) )); then
            status="${YELLOW}HIGH MIG${NC}"
        else
            status="${GREEN}GOOD${NC}"
        fi
        
        # Categorize thread
        local line=$(printf "%-20s %9.1f%% %10.1f %10.1f %10.1f %b\n" \
            "${comm:0:20}" "$wait_pct" "$mig_per_sec" "$sw_per_sec" "$invol_per_sec" "$status")
        
        case "$comm" in
            # Critical game logic threads (input + main loop)
            GameThread*|MainThread*|Wow|Wow.exe|WowClassic*|Main*)
                critical_threads+="$line\n"
                ;;
            # GPU/Rendering threads
            dxvk*|RHI*|Render*|RenderThread*|D3D*|Vulkan*|wined3d*)
                gpu_threads+="$line\n"
                ;;
            # Worker/background threads
            *Worker*|Background*|Pool*|Async*|ThreadPool*|Wow-[0-9]*)
                worker_threads+="$line\n"
                ;;
            *)
                other_threads+="$line\n"
                ;;
        esac
    done
    
    # Print categorized threads
    if [[ -n "$critical_threads" ]]; then
        echo -e "${BLUE}[CRITICAL - Game Logic]${NC}"
        echo -e "$critical_threads" | head -5
    fi
    
    if [[ -n "$gpu_threads" ]]; then
        echo -e "\n${BLUE}[GPU - Rendering]${NC}"
        echo -e "$gpu_threads" | head -10
    fi
    
    if [[ -n "$worker_threads" ]]; then
        echo -e "\n${BLUE}[WORKERS - Background]${NC}"
        echo -e "$worker_threads" | head -5
    fi
    
    echo ""
    echo "┌─────────────────────────────────────────────────────────────────────────────┐"
    echo "│ LEGEND                                                                      │"
    echo "└─────────────────────────────────────────────────────────────────────────────┘"
    echo "  WAIT%:   Time waiting for CPU (lower = better, <5% ideal for critical threads)"
    echo "  MIG/s:   Migrations per second (lower = better cache locality, <100 ideal)"
    echo "  SW/s:    Context switches per second"
    echo "  INVOL/s: Involuntary preemptions per second (high = contention)"
    echo ""
    echo "  STATUS: ${GREEN}GOOD${NC} = Healthy | ${YELLOW}MODERATE${NC} = Acceptable | ${RED}HIGH WAIT${NC} = Problem"
    echo ""
}

# Main monitoring function
run_monitor() {
    local pid="$1"
    local duration="$2"
    
    local before_file="/tmp/scx_perf_before_$$.json"
    local after_file="/tmp/scx_perf_after_$$.json"
    
    log_info "Capturing baseline..."
    capture_thread_stats "$pid" "$before_file"
    
    log_info "Waiting ${duration} seconds..."
    sleep "$duration"
    
    log_info "Capturing final state..."
    capture_thread_stats "$pid" "$after_file"
    
    calculate_deltas "$before_file" "$after_file" "$duration"
    
    # Cleanup
    rm -f "$before_file" "$after_file"
}

# Continuous monitoring mode
run_continuous() {
    local pid="$1"
    local interval="${2:-5}"
    
    log_info "Continuous monitoring (Ctrl+C to stop)"
    log_info "Interval: ${interval}s"
    echo ""
    
    while true; do
        clear
        run_monitor "$pid" "$interval"
        echo ""
        echo "Next update in ${interval}s... (Ctrl+C to stop)"
        sleep 1
    done
}

# Save baseline for comparison
save_baseline() {
    local pid="$1"
    log_info "Saving baseline to $BASELINE_FILE"
    capture_thread_stats "$pid" "$BASELINE_FILE"
    log_good "Baseline saved. Run with --compare to measure against it."
}

# Compare against saved baseline
run_compare() {
    local pid="$1"
    
    if [[ ! -f "$BASELINE_FILE" ]]; then
        log_bad "No baseline found. Run with --baseline first."
        exit 1
    fi
    
    local after_file="/tmp/scx_perf_compare_$$.json"
    capture_thread_stats "$pid" "$after_file"
    
    echo ""
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                         A/B COMPARISON REPORT                                ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    echo ""
    echo "Baseline: $(stat -c %y "$BASELINE_FILE" | cut -d. -f1)"
    echo "Current:  $(date '+%Y-%m-%d %H:%M:%S')"
    echo ""
    
    # TODO: Implement detailed comparison
    log_info "Comparison analysis..."
    
    rm -f "$after_file"
}

#=============================================================================
# MAIN
#=============================================================================

main() {
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --pid)
                GAME_PID="$2"
                shift 2
                ;;
            --duration|-d)
                DURATION="$2"
                shift 2
                ;;
            --continuous|-c)
                CONTINUOUS=true
                shift
                ;;
            --baseline|-b)
                local pid="${GAME_PID:-$(find_game_pid)}"
                if [[ -z "$pid" ]]; then
                    log_bad "No game found. Use --pid to specify."
                    exit 1
                fi
                save_baseline "$pid"
                exit 0
                ;;
            --compare)
                COMPARE_MODE=true
                shift
                ;;
            --help|-h)
                cat << EOF
game_perf_monitor.sh - Delta-Based Game Performance Monitor

USAGE:
  ./game_perf_monitor.sh [OPTIONS]

OPTIONS:
  --pid PID         Monitor specific process ID
  --duration N      Measurement window in seconds (default: 10)
  --continuous      Continuous monitoring mode
  --baseline        Save current state as baseline for comparison
  --compare         Compare current state against saved baseline
  --help            Show this help

EXAMPLES:
  ./game_perf_monitor.sh                    # Auto-detect game, 10s measurement
  ./game_perf_monitor.sh --duration 30      # 30 second measurement
  ./game_perf_monitor.sh --pid 12345        # Monitor specific PID
  ./game_perf_monitor.sh --continuous       # Live monitoring
  ./game_perf_monitor.sh --baseline         # Save baseline
  ./game_perf_monitor.sh --compare          # Compare to baseline

KEY METRICS:
  WAIT%    - Time thread spent waiting for CPU (lower = better)
  MIG/s    - CPU migrations per second (lower = better cache locality)
  SW/s     - Context switches per second
  INVOL/s  - Involuntary preemptions (scheduler forced switch)

THRESHOLDS:
  Critical threads (GameThread): WAIT% < 5%, MIG/s < 100
  GPU threads (dxvk, Render):    WAIT% < 10%, MIG/s < 200
  Workers (Background, Pool):   WAIT% < 30%, MIG/s < 500
EOF
                exit 0
                ;;
            *)
                shift
                ;;
        esac
    done
    
    # Find game PID
    if [[ -z "$GAME_PID" ]]; then
        GAME_PID=$(find_game_pid) || {
            log_bad "No game found. Use --pid to specify."
            exit 1
        }
    fi
    
    # Verify PID exists
    if [[ ! -d "/proc/$GAME_PID" ]]; then
        log_bad "Process $GAME_PID not found."
        exit 1
    fi
    
    local game_name=$(cat /proc/$GAME_PID/comm 2>/dev/null || echo "unknown")
    local thread_count=$(ls /proc/$GAME_PID/task/ 2>/dev/null | wc -l)
    
    echo ""
    echo "╔══════════════════════════════════════════════════════════════════════════════╗"
    echo "║                    SCX_GAMER PERFORMANCE MONITOR                             ║"
    echo "╚══════════════════════════════════════════════════════════════════════════════╝"
    echo ""
    echo "  Game:    $game_name (PID: $GAME_PID)"
    echo "  Threads: $thread_count"
    echo "  Window:  ${DURATION}s"
    echo ""
    
    if [[ "$COMPARE_MODE" == "true" ]]; then
        run_compare "$GAME_PID"
    elif [[ "$CONTINUOUS" == "true" ]]; then
        run_continuous "$GAME_PID" "$DURATION"
    else
        run_monitor "$GAME_PID" "$DURATION"
    fi
}

main "$@"

