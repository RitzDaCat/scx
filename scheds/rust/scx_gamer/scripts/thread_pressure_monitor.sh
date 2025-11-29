#!/bin/bash
# thread_pressure_monitor.sh - AI-Friendly Gaming Thread Diagnostics for scx_gamer
#
# PURPOSE: Generate structured diagnostic output for AI analysis (Claude, etc.)
# 
# This tool collects:
# - PSI (Pressure Stall Information) - are threads WAITING vs just busy
# - Per-thread schedstat - wait times, runtime, context switches
# - scx_gamer metrics (if debug API is running)
# - Thread classification hints for AI reasoning
#
# OUTPUT: Structured text report optimized for AI analysis
#
# USAGE:
#   ./thread_pressure_monitor.sh [GAME_NAME]     # One-shot AI report
#   ./thread_pressure_monitor.sh --json [GAME]   # JSON output
#   ./thread_pressure_monitor.sh --watch [GAME]  # Continuous (human mode)

set -euo pipefail

# Configuration
SCX_API_PORT="${SCX_API_PORT:-8080}"
SCX_API_URL="http://127.0.0.1:${SCX_API_PORT}"
MAX_THREADS=30

#=============================================================================
# DATA COLLECTION FUNCTIONS
#=============================================================================

# Get system info
collect_system_info() {
    local cores=$(nproc)
    local model=$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2 | xargs)
    local kernel=$(uname -r)
    local loadavg=$(cat /proc/loadavg | awk '{print $1, $2, $3}')
    local uptime=$(awk '{printf "%.1f hours", $1/3600}' /proc/uptime)
    local mem_total=$(awk '/MemTotal/ {printf "%.1f GB", $2/1024/1024}' /proc/meminfo)
    local mem_avail=$(awk '/MemAvailable/ {printf "%.1f GB", $2/1024/1024}' /proc/meminfo)
    
    cat <<EOF
CPU: $model
Cores: $cores
Kernel: $kernel
Memory: $mem_avail available / $mem_total total
Load Average: $loadavg
Uptime: $uptime
EOF
}

# Collect PSI metrics
collect_psi() {
    echo "=== PRESSURE STALL INFORMATION (PSI) ==="
    echo ""
    echo "INTERPRETATION:"
    echo "  PSI shows % of time tasks were WAITING (stalled), not just CPU busy."
    echo "  - avg10/60/300 = moving averages over 10s/60s/300s"
    echo "  - 'some' = at least one task stalled"
    echo "  - 'full' = ALL tasks stalled (severe)"
    echo ""
    echo "GAMING THRESHOLDS:"
    echo "  CPU:  <1% good, 2-5% concerning, >5% bad (frame drops likely)"
    echo "  MEM:  >0.5% = memory pressure causing stalls"
    echo "  IO:   >1% = storage bottleneck (asset loading)"
    echo ""
    
    echo "--- CPU Pressure ---"
    if [[ -f /proc/pressure/cpu ]]; then
        cat /proc/pressure/cpu
    else
        echo "PSI not available (requires Linux 4.20+ with CONFIG_PSI=y)"
    fi
    echo ""
    
    echo "--- Memory Pressure ---"
    if [[ -f /proc/pressure/memory ]]; then
        cat /proc/pressure/memory
    fi
    echo ""
    
    echo "--- IO Pressure ---"
    if [[ -f /proc/pressure/io ]]; then
        cat /proc/pressure/io
    fi
    echo ""
}

# Find game process
find_game_pid() {
    local game_name="${1:-}"
    local pid=""
    
    local patterns=(
        "Palworld" "PalServer" "cs2" "csgo" "valorant" "VALORANT"
        "overwatch" "apex" "r5apex" "dota2" "hl2_linux"
        "wine.*\.exe" "proton.*\.exe" "steam_app"
        "gamescope" "Xwayland"
    )
    
    if [[ -n "$game_name" ]]; then
        pid=$(pgrep -f "$game_name" 2>/dev/null | head -1)
    else
        for pattern in "${patterns[@]}"; do
            pid=$(pgrep -if "$pattern" 2>/dev/null | head -1)
            [[ -n "$pid" ]] && break
        done
    fi
    
    echo "$pid"
}

# Format nanoseconds
format_ns() {
    local ns="$1"
    if (( ns >= 1000000000 )); then
        echo "$(echo "scale=2; $ns / 1000000000" | bc)s"
    elif (( ns >= 1000000 )); then
        echo "$(echo "scale=2; $ns / 1000000" | bc)ms"
    elif (( ns >= 1000 )); then
        echo "$(echo "scale=1; $ns / 1000" | bc)us"
    else
        echo "${ns}ns"
    fi
}

# Classify thread by name (for AI context)
classify_thread() {
    local name="$1"
    case "$name" in
        *Render*|*RHI*|*D3D*|*dxvk*|*vkd3d*|*wined3d*|*Vulkan*|*GL*)
            echo "GPU_RENDER"
            ;;
        *GPU*|*gfx*|*amdgpu*|*nvidia*|*radeon*)
            echo "GPU_DRIVER"
            ;;
        *Game*|*Main*|*Tick*|*Update*|*Logic*|*Simulation*)
            echo "GAME_LOGIC"
            ;;
        *Input*|*SDL*|*evdev*|*xinput*|*Mouse*|*Keyboard*)
            echo "INPUT"
            ;;
        *Audio*|*Pulse*|*Pipe*|*ALSA*|*Sound*|*XAudio*)
            echo "AUDIO"
            ;;
        *Network*|*Net*|*Socket*|*Steam*|*HTTP*|*curl*)
            echo "NETWORK"
            ;;
        *Worker*|*Thread*Pool*|*Task*|*Job*|*Async*)
            echo "WORKER_POOL"
            ;;
        *Physics*|*Collision*|*PhysX*|*Havok*)
            echo "PHYSICS"
            ;;
        *AI*|*NPC*|*Pathfind*|*Nav*)
            echo "AI_NPC"
            ;;
        *Load*|*Stream*|*Asset*|*Resource*|*IO*)
            echo "ASSET_LOADING"
            ;;
        wine*|proton*|pressure*)
            echo "WINE_PROTON"
            ;;
        *)
            echo "OTHER"
            ;;
    esac
}

# Collect thread data for a process
collect_thread_data() {
    local pid="$1"
    local game_name="$2"
    
    echo "=== GAME THREAD ANALYSIS ==="
    echo ""
    echo "Process: $game_name (PID: $pid)"
    echo "Cmdline: $(cat /proc/$pid/cmdline 2>/dev/null | tr '\0' ' ' | head -c 200)"
    echo ""
    
    if [[ ! -d "/proc/$pid/task" ]]; then
        echo "ERROR: Cannot access /proc/$pid/task"
        return
    fi
    
    local thread_count=$(ls -1 /proc/$pid/task/ 2>/dev/null | wc -l)
    echo "Total threads: $thread_count"
    echo ""
    
    echo "INTERPRETATION:"
    echo "  Wait Time = cumulative time thread spent WAITING for CPU (not running)"
    echo "  Runtime   = cumulative time thread spent actually RUNNING on CPU"
    echo "  Wait%     = Wait / (Wait + Runtime) - lower is better"
    echo "  Switches  = context switch count - high = frequent preemption"
    echo ""
    echo "WHAT TO LOOK FOR:"
    echo "  - INPUT threads with high Wait% = input latency issue (BAD for gaming)"
    echo "  - GPU_RENDER threads with high Wait% = GPU bottleneck or scheduler issue"
    echo "  - GAME_LOGIC threads with high Wait% = CPU contention"
    echo "  - Many threads with >10% Wait% = systemic scheduling problem"
    echo ""
    
    # Header
    printf "%-20s %-8s %-10s %-14s %-14s %-7s %-10s %s\n" \
        "THREAD_NAME" "TID" "CLASS" "WAIT_TIME" "RUNTIME" "WAIT%" "SWITCHES" "NOTES"
    printf "%s\n" "$(printf '=%.0s' {1..110})"
    
    # Collect all threads with their wait times
    local threads=()
    for tid_dir in /proc/$pid/task/*/; do
        local tid=$(basename "$tid_dir")
        local comm_file="$tid_dir/comm"
        local stat_file="$tid_dir/schedstat"
        
        [[ ! -f "$comm_file" || ! -f "$stat_file" ]] && continue
        
        local comm=$(cat "$comm_file" 2>/dev/null || echo "unknown")
        local schedstat=$(cat "$stat_file" 2>/dev/null || echo "0 0 0")
        
        local runtime=$(echo "$schedstat" | awk '{print $1}')
        local wait_time=$(echo "$schedstat" | awk '{print $2}')
        local switches=$(echo "$schedstat" | awk '{print $3}')
        
        threads+=("$wait_time:$tid:$comm:$runtime:$switches")
    done
    
    # Sort by wait_time descending and display
    printf '%s\n' "${threads[@]}" | sort -t: -k1 -rn | head -"$MAX_THREADS" | while IFS=: read -r wait tid comm runtime switches; do
        [[ -z "$wait" ]] && continue
        
        local class=$(classify_thread "$comm")
        local wait_fmt=$(format_ns "$wait")
        local runtime_fmt=$(format_ns "$runtime")
        
        local total=$((runtime + wait))
        local wait_pct="0.0"
        if (( total > 0 )); then
            wait_pct=$(echo "scale=1; $wait * 100 / $total" | bc)
        fi
        
        # Generate notes for AI
        local notes=""
        if [[ "$class" == "INPUT" ]] && (( $(echo "$wait_pct > 5" | bc -l) )); then
            notes="[!] INPUT LATENCY RISK"
        elif [[ "$class" == "GPU_RENDER" ]] && (( $(echo "$wait_pct > 15" | bc -l) )); then
            notes="[!] RENDER STALL"
        elif [[ "$class" == "GAME_LOGIC" ]] && (( $(echo "$wait_pct > 20" | bc -l) )); then
            notes="[!] GAME THREAD STARVED"
        elif (( $(echo "$wait_pct > 30" | bc -l) )); then
            notes="[!] HIGH WAIT"
        fi
        
        printf "%-20s %-8s %-10s %-14s %-14s %6.1f%% %-10s %s\n" \
            "${comm:0:20}" "$tid" "$class" "$wait_fmt" "$runtime_fmt" "$wait_pct" "$switches" "$notes"
    done
    
    echo ""
}

# Collect scx_gamer metrics if available
collect_scx_metrics() {
    echo "=== SCX_GAMER SCHEDULER METRICS ==="
    echo ""
    
    # Check if scx_gamer is running
    if ! pgrep -x scx_gamer >/dev/null 2>&1; then
        echo "STATUS: scx_gamer NOT RUNNING"
        echo ""
        echo "The scx_gamer scheduler is not active. Thread scheduling is using"
        echo "the default kernel scheduler (likely CFS or EEVDF)."
        echo ""
        return
    fi
    
    echo "STATUS: scx_gamer RUNNING"
    
    # Try to get metrics from debug API
    if curl -s --connect-timeout 1 "$SCX_API_URL/metrics" >/dev/null 2>&1; then
        echo "Debug API: Available at $SCX_API_URL"
        echo ""
        
        local metrics=$(curl -s "$SCX_API_URL/metrics" 2>/dev/null)
        
        if [[ -n "$metrics" ]] && command -v jq &>/dev/null; then
            echo "--- Thread Classifications ---"
            echo "$metrics" | jq -r '
                "Input Handler Threads:  \(.input_handler_threads // "N/A")",
                "GPU/Render Threads:     \(.gpu_threads // "N/A")",
                "Audio Threads:          \(.audio_threads // "N/A")",
                "Network Threads:        \(.network_threads // "N/A")",
                "Compositor Threads:     \(.compositor_threads // "N/A")"
            ' 2>/dev/null || echo "(Could not parse metrics)"
            echo ""
            
            echo "--- Dispatch Statistics ---"
            echo "$metrics" | jq -r '
                "Direct Dispatch:        \(.dispatch_direct // "N/A")",
                "Shared DSQ Dispatch:    \(.dispatch_shared // "N/A")",
                "Direct Dispatch %:      \(.dispatch_direct_pct // "N/A")%"
            ' 2>/dev/null || true
            echo ""
            
            echo "--- PSI from Scheduler ---"
            echo "$metrics" | jq -r '
                "CPU PSI (avg10):        \(.psi_cpu_some_avg10 // "N/A")%",
                "Memory PSI (avg10):     \(.psi_mem_some_avg10 // "N/A")%"
            ' 2>/dev/null || true
            echo ""
            
            echo "--- Input Boost State ---"
            echo "$metrics" | jq -r '
                "Keyboard Boost Active:  \(.keyboard_boost_active // "N/A")",
                "Mouse Boost Active:     \(.mouse_boost_active // "N/A")",
                "Input Events/sec:       \(.input_events_per_sec // "N/A")"
            ' 2>/dev/null || true
            echo ""
        else
            echo "Raw metrics (jq not available):"
            echo "$metrics" | head -50
            echo ""
        fi
    else
        echo "Debug API: NOT AVAILABLE"
        echo ""
        echo "To enable metrics, run scx_gamer with --debug-api 8080"
        echo "Or use: ./start.sh -> Debug API Mode"
        echo ""
    fi
}

# Collect CPU topology
collect_cpu_topology() {
    echo "=== CPU TOPOLOGY ==="
    echo ""
    
    local cores=$(nproc)
    local sockets=$(lscpu | grep "Socket(s):" | awk '{print $2}')
    local cores_per_socket=$(lscpu | grep "Core(s) per socket:" | awk '{print $4}')
    local threads_per_core=$(lscpu | grep "Thread(s) per core:" | awk '{print $4}')
    local numa_nodes=$(lscpu | grep "NUMA node(s):" | awk '{print $3}' || echo "1")
    
    echo "Total Logical CPUs: $cores"
    echo "Sockets: ${sockets:-1}"
    echo "Cores per Socket: ${cores_per_socket:-$cores}"
    echo "Threads per Core: ${threads_per_core:-1} (SMT/HT: $([ "${threads_per_core:-1}" -gt 1 ] && echo "ENABLED" || echo "DISABLED"))"
    echo "NUMA Nodes: ${numa_nodes:-1}"
    echo ""
    
    # Show cache info
    echo "Cache Hierarchy:"
    lscpu | grep -E "^L[123]" | sed 's/^/  /'
    echo ""
}

# Generate AI analysis hints
generate_analysis_hints() {
    echo "=== ANALYSIS GUIDE FOR AI ==="
    echo ""
    cat <<'EOF'
When analyzing this report, consider:

1. PSI METRICS:
   - CPU PSI >2% during gaming = scheduling problem, threads waiting
   - CPU PSI >5% = critical, expect frame drops and input lag
   - Memory PSI >0.5% = memory pressure, possible swapping
   - IO PSI >1% = storage bottleneck (game loading assets)

2. THREAD WAIT% INTERPRETATION:
   - INPUT threads >5% Wait% = INPUT LATENCY ISSUE (most critical for gaming)
   - GPU_RENDER threads >15% Wait% = GPU starvation, check if GPU-bound
   - GAME_LOGIC threads >20% Wait% = main game thread starved for CPU
   - WORKER_POOL threads high Wait% = thread pool contention

3. THREAD CLASSIFICATION PATTERNS:
   - scx_gamer should detect and prioritize INPUT threads
   - GPU_RENDER threads should get priority during input boost window
   - AUDIO threads need low latency to avoid crackling

4. SCHEDULING IMPROVEMENTS TO SUGGEST:
   - High INPUT Wait% -> check --input-window-us, --mouse-boost-us settings
   - High GPU Wait% -> check --avoid-smt, verify GPU threads detected
   - Many threads >10% Wait% -> check --slice-us (smaller = more responsive)
   - Memory PSI -> not scheduler issue, RAM/swap problem

5. SCX_GAMER SPECIFIC:
   - If "scx_gamer NOT RUNNING", scheduler optimizations not active
   - Check "Thread Classifications" to verify game threads detected
   - "Direct Dispatch %" should be high (>70%) for good cache locality
   - Input boost should be active during gameplay

EOF
}

#=============================================================================
# OUTPUT MODES
#=============================================================================

# Generate full AI-friendly report
generate_ai_report() {
    local game_name="${1:-}"
    
    echo "==============================================================================="
    echo "           SCX_GAMER THREAD PRESSURE DIAGNOSTIC REPORT"
    echo "           Generated: $(date '+%Y-%m-%d %H:%M:%S %Z')"
    echo "==============================================================================="
    echo ""
    
    echo "=== SYSTEM INFORMATION ==="
    echo ""
    collect_system_info
    echo ""
    
    collect_cpu_topology
    collect_psi
    collect_scx_metrics
    
    # Find and analyze game
    local pid=$(find_game_pid "$game_name")
    if [[ -n "$pid" ]]; then
        local comm=$(cat /proc/$pid/comm 2>/dev/null || echo "unknown")
        collect_thread_data "$pid" "$comm"
    else
        echo "=== GAME THREAD ANALYSIS ==="
        echo ""
        echo "NO GAME PROCESS DETECTED"
        echo ""
        echo "Could not find a running game. Specify game name:"
        echo "  $0 Palworld"
        echo "  $0 cs2"
        echo ""
        echo "Or manually provide PID:"
        echo "  $0 --pid 12345"
        echo ""
    fi
    
    generate_analysis_hints
    
    echo "==============================================================================="
    echo "                         END OF DIAGNOSTIC REPORT"
    echo "==============================================================================="
}

# JSON output mode
generate_json_report() {
    local game_name="${1:-}"
    local pid=$(find_game_pid "$game_name")
    
    # Build JSON manually (works without jq for generation)
    echo "{"
    echo "  \"timestamp\": \"$(date -Iseconds)\","
    echo "  \"system\": {"
    echo "    \"cpu_model\": \"$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2 | xargs | sed 's/"/\\"/g')\","
    echo "    \"cores\": $(nproc),"
    echo "    \"kernel\": \"$(uname -r)\","
    echo "    \"load_avg\": \"$(cat /proc/loadavg | awk '{print $1, $2, $3}')\""
    echo "  },"
    
    # PSI
    echo "  \"psi\": {"
    if [[ -f /proc/pressure/cpu ]]; then
        local cpu_line=$(grep "^some" /proc/pressure/cpu)
        echo "    \"cpu_some_avg10\": $(echo "$cpu_line" | grep -oP 'avg10=\K[0-9.]+'),"
        echo "    \"cpu_some_avg60\": $(echo "$cpu_line" | grep -oP 'avg60=\K[0-9.]+'),"
        echo "    \"cpu_some_avg300\": $(echo "$cpu_line" | grep -oP 'avg300=\K[0-9.]+')"
    else
        echo "    \"available\": false"
    fi
    echo "  },"
    
    # scx_gamer status
    echo "  \"scx_gamer\": {"
    if pgrep -x scx_gamer >/dev/null 2>&1; then
        echo "    \"running\": true,"
        if curl -s --connect-timeout 1 "$SCX_API_URL/metrics" >/dev/null 2>&1; then
            echo "    \"api_available\": true,"
            echo "    \"metrics\": $(curl -s "$SCX_API_URL/metrics" 2>/dev/null || echo "null")"
        else
            echo "    \"api_available\": false"
        fi
    else
        echo "    \"running\": false"
    fi
    echo "  },"
    
    # Game threads
    echo "  \"game\": {"
    if [[ -n "$pid" ]]; then
        local comm=$(cat /proc/$pid/comm 2>/dev/null || echo "unknown")
        echo "    \"detected\": true,"
        echo "    \"name\": \"$comm\","
        echo "    \"pid\": $pid,"
        echo "    \"threads\": ["
        
        local first=true
        for tid_dir in /proc/$pid/task/*/; do
            local tid=$(basename "$tid_dir")
            local tcomm=$(cat "$tid_dir/comm" 2>/dev/null || echo "unknown")
            local schedstat=$(cat "$tid_dir/schedstat" 2>/dev/null || echo "0 0 0")
            local runtime=$(echo "$schedstat" | awk '{print $1}')
            local wait_time=$(echo "$schedstat" | awk '{print $2}')
            local switches=$(echo "$schedstat" | awk '{print $3}')
            local class=$(classify_thread "$tcomm")
            
            [[ "$first" == "true" ]] || echo ","
            first=false
            
            echo -n "      {\"tid\": $tid, \"name\": \"$tcomm\", \"class\": \"$class\", \"wait_ns\": $wait_time, \"runtime_ns\": $runtime, \"switches\": $switches}"
        done
        echo ""
        echo "    ]"
    else
        echo "    \"detected\": false"
    fi
    echo "  }"
    echo "}"
}

# Continuous watch mode (for humans)
watch_mode() {
    local game_name="${1:-}"
    local interval="${2:-1}"
    
    while true; do
        clear
        echo "=== LIVE THREAD PRESSURE MONITOR === ($(date '+%H:%M:%S')) Press Ctrl+C to exit"
        echo ""
        
        # Quick PSI summary
        if [[ -f /proc/pressure/cpu ]]; then
            local cpu_psi=$(grep "^some" /proc/pressure/cpu | grep -oP 'avg10=\K[0-9.]+')
            local mem_psi=$(grep "^some" /proc/pressure/memory | grep -oP 'avg10=\K[0-9.]+')
            
            local cpu_color="\033[32m"  # green
            (( $(echo "$cpu_psi >= 2" | bc -l) )) && cpu_color="\033[33m"  # yellow
            (( $(echo "$cpu_psi >= 5" | bc -l) )) && cpu_color="\033[31m"  # red
            
            echo -e "PSI: CPU ${cpu_color}${cpu_psi}%\033[0m | MEM ${mem_psi}%"
        fi
        echo ""
        
        # Thread summary
        local pid=$(find_game_pid "$game_name")
        if [[ -n "$pid" ]]; then
            local comm=$(cat /proc/$pid/comm 2>/dev/null || echo "unknown")
            echo "Game: $comm (PID: $pid)"
            echo ""
            printf "%-18s %-10s %-12s %-12s %s\n" "THREAD" "CLASS" "WAIT" "RUNTIME" "WAIT%"
            printf "%s\n" "$(printf '-%.0s' {1..70})"
            
            for tid_dir in /proc/$pid/task/*/; do
                local tid=$(basename "$tid_dir")
                local tcomm=$(cat "$tid_dir/comm" 2>/dev/null || continue)
                local schedstat=$(cat "$tid_dir/schedstat" 2>/dev/null || continue)
                local runtime=$(echo "$schedstat" | awk '{print $1}')
                local wait=$(echo "$schedstat" | awk '{print $2}')
                local class=$(classify_thread "$tcomm")
                
                local total=$((runtime + wait))
                local wait_pct="0.0"
                (( total > 0 )) && wait_pct=$(echo "scale=1; $wait * 100 / $total" | bc)
                
                # Only show threads with some activity
                (( wait > 1000000 || runtime > 1000000 )) || continue
                
                printf "%-18s %-10s %-12s %-12s %5.1f%%\n" \
                    "${tcomm:0:18}" "$class" "$(format_ns $wait)" "$(format_ns $runtime)" "$wait_pct"
            done | sort -t'%' -k5 -rn | head -15
        else
            echo "No game detected. Specify: $0 --watch GameName"
        fi
        
        sleep "$interval"
    done
}

#=============================================================================
# MAIN
#=============================================================================

usage() {
    cat <<EOF
Usage: $0 [OPTIONS] [GAME_NAME]

AI-Friendly Gaming Thread Diagnostics for scx_gamer

OPTIONS:
  (default)           Generate full diagnostic report for AI analysis
  --json              Output in JSON format
  --watch [SEC]       Continuous monitoring (human mode)
  --pid PID           Analyze specific process ID
  -h, --help          Show this help

EXAMPLES:
  $0                  # Full AI report, auto-detect game
  $0 Palworld         # Full AI report for Palworld
  $0 --json cs2       # JSON output for CS2
  $0 --watch 2        # Live monitor, 2 second refresh
  $0 --pid 12345      # Analyze specific PID

OUTPUT MODES:
  Default: Structured text report optimized for pasting into AI chat
  JSON:    Machine-readable format for programmatic analysis
  Watch:   Human-friendly live monitoring

EOF
}

main() {
    local mode="report"
    local game_name=""
    local watch_interval=1
    local target_pid=""
    
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --json)
                mode="json"
                shift
                ;;
            -w|--watch)
                mode="watch"
                if [[ "${2:-}" =~ ^[0-9]+$ ]]; then
                    watch_interval="$2"
                    shift
                fi
                shift
                ;;
            --pid)
                target_pid="$2"
                shift 2
                ;;
            -h|--help)
                usage
                exit 0
                ;;
            *)
                game_name="$1"
                shift
                ;;
        esac
    done
    
    # Override game detection if PID specified
    if [[ -n "$target_pid" ]]; then
        game_name="$target_pid"
    fi
    
    case "$mode" in
        report)
            generate_ai_report "$game_name"
            ;;
        json)
            generate_json_report "$game_name"
            ;;
        watch)
            watch_mode "$game_name" "$watch_interval"
            ;;
    esac
}

main "$@"
