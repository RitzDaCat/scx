#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
BUILD_MODE=""  # Will be set to "release" or "debug"
BIN_PATH=""    # Will be set based on BUILD_MODE

select_build_mode() {
    while true; do
        cat <<'BUILD_MENU'

================================================================================
                         SELECT BUILD MODE
================================================================================

MODE                   DESCRIPTION
--------------------------------------------------------------------------------
1) Release             Production build (optimized, no profiling)
                       - Binary: target/release/scx_gamer
                       - Best performance, smallest binary
                       - Recommended for gaming

2) Debug               Development build with profiling enabled
                       - Binary: target/debug/scx_gamer
                       - Profiling enabled (ENABLE_PROFILING)
                       - Latency histograms active
                       - ~50-150ns overhead per scheduling decision
                       - Use for: Debugging, performance analysis

================================================================================

BUILD_MENU
        read -rp "Select build mode [1-2]: " mode_choice
        case "${mode_choice}" in
            1)
                BUILD_MODE="release"
                BIN_PATH="${REPO_ROOT}/target/release/scx_gamer"
                echo
                echo "Selected: Release build"
                echo "Binary: ${BIN_PATH}"
                echo
                break
                ;;
            2)
                BUILD_MODE="debug"
                BIN_PATH="${REPO_ROOT}/target/debug/scx_gamer"
                echo
                echo "Selected: Debug build (with profiling)"
                echo "Binary: ${BIN_PATH}"
                echo
                break
                ;;
            *)
                echo
                echo "Invalid selection. Please choose 1 or 2."
                echo
                sleep 1
                ;;
        esac
    done
}

build_scx() {
    echo
    if [[ "${BUILD_MODE}" == "debug" ]]; then
        echo "[scx_gamer] Building debug binary with profiling..."
        export SCX_GAMER_ENABLE_PROFILING=1
        cargo -C "${REPO_ROOT}" build -p scx_gamer
    else
        echo "[scx_gamer] Building release binary..."
        cargo -C "${REPO_ROOT}" build -p scx_gamer --release
    fi
}

ensure_binary() {
    if [[ ! -x "${BIN_PATH}" ]]; then
        build_scx
    fi
}

prompt_extra_flags() {
    EXTRA_FLAGS=()
    local line
    read -rp "Additional flags (optional): " line
    if [[ -n "${line}" ]]; then
        read -ra EXTRA_FLAGS <<<"${line}"
    fi
}

launch_scx() {
    ensure_binary
    local mode_desc="$1"
    shift

    local -a env_vars=()
    local -a base_args=()

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --env)
                env_vars+=("$2")
                shift 2
                ;;
            --arg)
                base_args+=("$2")
                shift 2
                ;;
            *)
                echo "Internal error: unknown launch_scx flag '$1'" >&2
                return 1
                ;;
        esac
    done

    echo
    echo "================================================================================"
    echo "                    LAUNCHING SCX_GAMER: ${mode_desc}"
    echo "================================================================================"
    echo
    prompt_extra_flags

    if [[ ${#EXTRA_FLAGS[@]} -gt 0 ]]; then
        echo "Additional flags: ${EXTRA_FLAGS[*]}"
        echo
    fi

    echo "Starting scheduler with root privileges..."
    echo "Press Ctrl+C to stop and return to the menu."
    echo "================================================================================"
    echo

    if (( ${#env_vars[@]} > 0 )); then
        sudo env "${env_vars[@]}" "${BIN_PATH}" "${base_args[@]}" "${EXTRA_FLAGS[@]}"
    else
        sudo "${BIN_PATH}" "${base_args[@]}" "${EXTRA_FLAGS[@]}"
    fi
}

run_standard() {
    while true; do
        cat <<'PROFILE'

================================================================================
                          SCX_GAMER STANDARD PROFILES
================================================================================

PROFILE                DESCRIPTION
--------------------------------------------------------------------------------
1) Esports (DEFAULT)   Competitive gaming with aggressive tuning
                       - Slice: 250us, input-window: 8ms
                       - Keyboard boost: 300ms, mouse boost: 6ms
                       - avoid-smt: ON, prefer-napi: ON
                       - Use for: Valorant, CS2, LoL, 144Hz-240Hz (RECOMMENDED)

2) Baseline            Clean scheduler defaults, minimal tuning
                       - Slice: 1000us (1ms)
                       - No aggressive optimizations
                       - Use for: General desktop use, light gaming

3) Casual Gaming       Balanced performance for general gaming
                       - Slice: 500us
                       - Keyboard boost: 1500ms, mouse boost: 10ms
                       - prefer-napi: ON
                       - Use for: Single-player, RPGs, 60Hz monitors

4) Ultra-Latency       Extreme low-latency for competitive play
                       - Slice: 5us, real-time scheduling (SCHED_FIFO)
                       - Keyboard boost: 100ms, mouse boost: 4ms
                       - wakeup-timer: 100us, mig-max: 2
                       - Use for: Aim trainers, 360Hz+, <5% CPU usage

5) SCHED_DEADLINE      Hard real-time with guaranteed time bounds
                       - Kernel admission control, no starvation risk
                       - Use for: Maximum consistency and stability

PROFILE HIERARCHY:
  Baseline       -> General desktop, light gaming (minimal overhead)
  Casual Gaming  -> Single-player, RPGs, 60Hz (balanced optimizations)
  Esports        -> Competitive FPS/MOBA, 144Hz+ (aggressive tuning)
  Ultra-Latency  -> Aim trainers, 360Hz+ (extreme low-latency)

NOTE: All profiles run WITHOUT monitoring for maximum performance.
      Use TUI Dashboard (option 2 from main menu) if you need visual monitoring.

q) Back to main menu

================================================================================
PROFILE
        read -rp "Select profile [1-5, q]: " profile_choice
        case "${profile_choice}" in
            1)
                echo
                echo "Profile: Esports (Default)"
                echo "Purpose: Competitive gaming with aggressive optimizations"
                echo
                echo "This is the DEFAULT profile - no flags needed!"
                echo "Effective settings: slice=250us input=8ms kbd=300ms mouse=6ms avoid-smt=on napi=on"
                echo
                launch_scx "Esports" \
                    --env "RUST_LOG=warn"
                return
                ;;
            2)
                echo
                echo "Profile: Baseline"
                echo "Purpose: Clean scheduler defaults for general desktop and light gaming"
                echo
                echo "Active Flags:"
                echo "  --profile baseline             (minimal tuning, slice=1000us)"
                echo
                launch_scx "Baseline" \
                    --env "RUST_LOG=warn" \
                    --arg "--profile" \
                    --arg "baseline"
                return
                ;;
            3)
                echo
                echo "Profile: Casual Gaming"
                echo "Purpose: Balanced performance for general gaming scenarios"
                echo
                echo "Active Flags:"
                echo "  --profile casual               (slice=500us, kbd=1500ms, mouse=10ms)"
                echo
                launch_scx "Casual Gaming" \
                    --env "RUST_LOG=warn" \
                    --arg "--profile" \
                    --arg "casual"
                return
                ;;
            4)
                echo
                echo "================================================================================"
                echo "                         ULTRA-LATENCY MODE (ADVANCED)"
                echo "================================================================================"
                echo
                echo "Profile: Ultra-Latency (Interrupt-Driven)"
                echo "Purpose: Extreme low-latency for competitive gaming and aim training"
                echo
                echo "Technical Details:"
                echo "  - Real-time scheduling: SCHED_FIFO with priority 50"
                echo "  - Input latency: 1-5 microseconds (interrupt-driven, not busy polling)"
                echo "  - CPU usage: <5% (95-98% savings vs old busy polling method)"
                echo "  - Scheduling slice: 5us (extremely aggressive preemption)"
                echo "  - Input boost window: 2ms (sustained for rapid input)"
                echo
                echo "Active Flags:"
                echo "  --profile ultra                (slice=5us, input=2ms, kbd=100ms, mouse=4ms)"
                echo "  --realtime-scheduling          (SCHED_FIFO real-time policy)"
                echo
                echo "Best For:"
                echo "  - Aim trainers (Kovaak's, Aimlab)"
                echo "  - High refresh rate displays (360Hz+)"
                echo "  - Competitive FPS games where every microsecond counts"
                echo "  - Systems with 8+ CPU cores (enough headroom for aggressive scheduling)"
                echo
                echo "WARNING:"
                echo "  Real-time scheduling gives this process maximum priority."
                echo "  Ensure your system has adequate resources (8+ cores recommended)."
                echo
                echo "================================================================================"
                echo
                read -rp "Enable Ultra-Latency mode? (y/N): " confirm
                if [[ "${confirm}" =~ ^[Yy]$ ]]; then
                    launch_scx "Ultra-Latency" \
                        --env "RUST_LOG=warn" \
                        --arg "--profile" \
                        --arg "ultra" \
                        --arg "--realtime-scheduling"
                else
                    echo
                    echo "Ultra-Latency mode cancelled."
                    echo
                fi
                return
                ;;
            5)
                echo
                echo "================================================================================"
                echo "                      SCHED_DEADLINE MODE (HARD REAL-TIME)"
                echo "================================================================================"
                echo
                echo "Profile: SCHED_DEADLINE"
                echo "Purpose: Hard real-time guarantees with kernel admission control"
                echo
                echo "Technical Details:"
                echo "  - Scheduling policy: SCHED_DEADLINE (hard real-time)"
                echo "  - Runtime budget: 800us per 1000us period (80% utilization cap)"
                echo "  - Kernel admission control: Prevents system overload"
                echo "  - No starvation risk: Guaranteed time bounds"
                echo "  - Most consistent latency profile available"
                echo
                echo "Active Flags:"
                echo "  --profile ultra                (aggressive base settings)"
                echo "  --deadline-scheduling          (Enable SCHED_DEADLINE policy)"
                echo "  --deadline-runtime-us 800      (CPU time budget per period)"
                echo
                echo "Best For:"
                echo "  - Maximum latency consistency and stability"
                echo "  - Systems where predictability is critical"
                echo "  - When you need guaranteed response times"
                echo
                echo "Requirements:"
                echo "  - Kernel built with CONFIG_SCHED_DEADLINE support"
                echo "  - Check with: zgrep SCHED_DEADLINE /proc/config.gz"
                echo
                echo "================================================================================"
                echo
                read -rp "Enable SCHED_DEADLINE mode? (y/N): " confirm
                if [[ "${confirm}" =~ ^[Yy]$ ]]; then
                    launch_scx "SCHED_DEADLINE" \
                        --env "RUST_LOG=warn" \
                        --arg "--profile" \
                        --arg "ultra" \
                        --arg "--deadline-scheduling" \
                        --arg "--deadline-runtime-us" \
                        --arg "800"
                else
                    echo
                    echo "SCHED_DEADLINE mode cancelled."
                    echo
                fi
                return
                ;;
            q|Q|0)
                return
                ;;
            *)
                echo "Invalid profile: ${profile_choice}"
                ;;
        esac
    done
}

run_tui() {
    ensure_binary
    echo
    local interval="0.1"
    while true; do
        cat <<'TUI_PROFILE'

================================================================================
                         SCX_GAMER TUI DASHBOARD PROFILES
================================================================================

TUI profiles include real-time visual monitoring with your selected settings.
Update interval: 0.1 seconds (100ms refresh rate)
Note: --monitoring is automatically enabled with --tui

PROFILE                DESCRIPTION
--------------------------------------------------------------------------------
1) Esports TUI         Competitive gaming profile with monitoring (DEFAULT)
                       - Full optimization suite, recommended for gaming

2) Baseline TUI        Clean scheduler defaults with TUI monitoring
                       - Basic scheduling, good for debugging/testing

3) Casual Gaming TUI   Balanced settings with visual monitoring
                       - Good for single-player and RPG gaming

4) Ultra TUI           Ultra low-latency with monitoring
                       - For extreme performance analysis

q) Back to main menu

================================================================================
TUI_PROFILE
        read -rp "Select TUI profile [1-4, q]: " profile_choice
        case "${profile_choice}" in
            1)
                echo
                echo "Launching: TUI Dashboard - Esports (Default profile)"
                echo
                launch_scx "TUI Esports" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            2)
                echo
                echo "Launching: TUI Dashboard - Baseline"
                echo
                launch_scx "TUI Baseline" \
                    --env "RUST_LOG=info" \
                    --arg "--profile" \
                    --arg "baseline" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            3)
                echo
                echo "Launching: TUI Dashboard - Casual Gaming"
                echo
                launch_scx "TUI Casual Gaming" \
                    --env "RUST_LOG=info" \
                    --arg "--profile" \
                    --arg "casual" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            4)
                echo
                echo "Launching: TUI Dashboard - Ultra"
                echo
                launch_scx "TUI Ultra" \
                    --env "RUST_LOG=info" \
                    --arg "--profile" \
                    --arg "ultra" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            q|Q|0)
                return
                ;;
            *)
                echo "Invalid profile: ${profile_choice}"
                ;;
        esac
    done
}

run_verbose() {
    echo
    echo "Mode: Verbose Statistics"
    echo "Statistics output interval: 1.0 seconds"
    echo "Note: --monitoring is automatically enabled with --stats"
    echo
    launch_scx "Verbose Mode" \
        --env "RUST_LOG=info" \
        --arg "--stats" \
        --arg "1.0"
}

run_debug() {
    echo
    echo "Mode: Debug (Maximum Logging)"
    echo
    echo "Environment:"
    echo "  RUST_LOG=debug"
    echo "  LIBBPF_LOG=debug"
    echo "  SCX_BPF_LOG=trace"
    echo
    echo "Use this mode for troubleshooting scheduler issues."
    echo "Output will be very verbose."
    echo
    launch_scx "Debug Mode" \
        --env "RUST_LOG=debug" \
        --env "LIBBPF_LOG=debug" \
        --env "SCX_BPF_LOG=trace" \
        --arg "--stats" \
        --arg "1.0" \
        --arg "--verbose"
}

run_debug_api() {
    ensure_binary
    echo
    local port="8080"
    while true; do
        cat <<'DEBUG_API_MENU'

================================================================================
                         DEBUG API MODE
================================================================================

The Debug API exposes scheduler metrics via HTTP for debugging and monitoring.
Metrics update every 1 second and are available as JSON.
Note: --monitoring is automatically enabled with --debug-api

API Endpoints:
  - http://127.0.0.1:PORT/metrics  - Get current scheduler metrics
  - http://127.0.0.1:PORT/health    - Health check
  - http://127.0.0.1:PORT/          - API information

You can query metrics using curl:
  curl http://127.0.0.1:PORT/metrics | jq .

PROFILE                DESCRIPTION
--------------------------------------------------------------------------------
1) Esports + API        Competitive gaming profile with API (DEFAULT)
                       - Full optimizations + metric access

2) Baseline + API       Default settings with debug API enabled
                       - Good for testing and debugging

3) TUI + API            TUI Dashboard with debug API enabled
                       - Visual monitoring + HTTP access

4) Custom + API         Custom flags with debug API enabled
                       - Use your own flags + API

q) Back to main menu

================================================================================
DEBUG_API_MENU
        read -rp "Select profile [1-4, q]: " profile_choice
        case "${profile_choice}" in
            1)
                echo
                echo "Launching: Esports Profile with Debug API (port ${port})"
                echo
                echo "Access metrics at: http://127.0.0.1:${port}/metrics"
                echo
                launch_scx "Esports + Debug API" \
                    --env "RUST_LOG=info" \
                    --arg "--debug-api" \
                    --arg "${port}"
                return
                ;;
            2)
                echo
                echo "Launching: Baseline Profile with Debug API (port ${port})"
                echo
                echo "Access metrics at: http://127.0.0.1:${port}/metrics"
                echo
                launch_scx "Baseline + Debug API" \
                    --env "RUST_LOG=info" \
                    --arg "--profile" \
                    --arg "baseline" \
                    --arg "--debug-api" \
                    --arg "${port}"
                return
                ;;
            3)
                echo
                echo "Launching: TUI Dashboard with Debug API (port ${port})"
                echo
                echo "Access metrics at: http://127.0.0.1:${port}/metrics"
                echo
                launch_scx "TUI + Debug API" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "0.1" \
                    --arg "--debug-api" \
                    --arg "${port}"
                return
                ;;
            4)
                echo
                echo "================================================================================"
                echo "                         CUSTOM FLAGS + DEBUG API"
                echo "================================================================================"
                echo
                echo "Enter your custom scx_gamer command-line arguments."
                echo "The --debug-api ${port} flag will be added automatically."
                echo
                echo "Example: --profile ultra --realtime-scheduling"
                echo
                local line
                read -rp "Custom flags: " line
                if [[ -z "${line}" ]]; then
                    echo
                    echo "No flags provided. Cancelled."
                    echo
                    return
                fi
                read -ra CUSTOM_ARGS <<<"${line}"
                echo
                echo "Launching with custom arguments + Debug API:"
                echo "  Custom: ${CUSTOM_ARGS[*]}"
                echo "  API: --debug-api ${port}"
                echo
                echo "Access metrics at: http://127.0.0.1:${port}/metrics"
                echo "================================================================================"
                echo
                sudo "${BIN_PATH}" "${CUSTOM_ARGS[@]}" --debug-api "${port}"
                return
                ;;
            q|Q|0)
                return
                ;;
            *)
                echo "Invalid profile: ${profile_choice}"
                ;;
        esac
    done
}

run_custom() {
    ensure_binary
    echo
    echo "================================================================================"
    echo "                              CUSTOM FLAGS MODE"
    echo "================================================================================"
    echo
    echo "Enter your custom scx_gamer command-line arguments."
    echo
    echo "Available profiles: --profile [esports|baseline|casual|ultra]"
    echo "  esports (default): slice=250us, input=8ms, kbd=300ms, mouse=6ms, avoid-smt, napi"
    echo "  baseline:          slice=1000us, minimal tuning"
    echo "  casual:            slice=500us, kbd=1500ms, mouse=10ms, napi"
    echo "  ultra:             slice=5us, input=2ms, kbd=100ms, mouse=4ms, mig-max=2"
    echo
    echo "Monitoring: --monitoring (enables stats/detectors/tracing)"
    echo "            Or use --stats/--tui/--debug-api which auto-enable monitoring"
    echo
    echo "Override any profile setting with explicit flags:"
    echo "  --slice-us <us>           Override slice duration"
    echo "  --input-window-us <us>    Override input window"
    echo "  --keyboard-boost-us <us>  Override keyboard boost"
    echo "  --mouse-boost-us <us>     Override mouse boost"
    echo "  --avoid-smt <true|false>  Override SMT avoidance"
    echo "  --prefer-napi-on-input <true|false>  Override NAPI preference"
    echo
    echo "Example: --profile ultra --realtime-scheduling"
    echo "Example: --profile casual --slice-us 250 --avoid-smt true"
    echo
    local line
    read -rp "Custom flags: " line
    if [[ -z "${line}" ]]; then
        echo
        echo "No flags provided. Cancelled."
        echo
        return
    fi
    read -ra CUSTOM_ARGS <<<"${line}"
    echo
    echo "Launching with custom arguments: ${CUSTOM_ARGS[*]}"
    echo "================================================================================"
    echo
    sudo "${BIN_PATH}" "${CUSTOM_ARGS[@]}"
}

show_menu() {
    cat <<'MENU'

================================================================================
                              SCX_GAMER LAUNCHER
================================================================================

Default profile is ESPORTS - competitive gaming with minimal overhead.
Stats/monitoring are DISABLED by default for maximum performance.

MODE                   DESCRIPTION
--------------------------------------------------------------------------------
1) Standard Profiles   Choose from preset gaming configurations
                       - Esports (default), Baseline, Casual, Ultra-Latency

2) TUI Dashboard       Interactive terminal UI with real-time stats
                       - Visual performance monitoring (recommended)

3) Verbose Mode        Run with statistics output every 1 second
                       - Clean output, monitoring auto-enabled

4) Debug Mode          Maximum logging for troubleshooting
                       - RUST_LOG=debug, LIBBPF_LOG=debug

5) Debug API Mode      Start with HTTP API for metric access
                       - Exposes metrics at http://127.0.0.1:8080/metrics
                       - Useful for debugging with AI assistants

6) Custom Flags        Manually enter scheduler command-line arguments
                       - For advanced users and testing

q) Quit                Exit launcher

================================================================================
MENU
}

# Select build mode first (release or debug)
select_build_mode

# Main menu loop
while true; do
    show_menu
    echo
    # Display build mode (capitalize first letter)
    if [[ "${BUILD_MODE}" == "release" ]]; then
        echo "Current build mode: Release (${BIN_PATH})"
    else
        echo "Current build mode: Debug (${BIN_PATH})"
    fi
    echo
    read -rp "Select mode [1-6, q]: " choice
    case "${choice}" in
        1) run_standard ;;
        2) run_tui ;;
        3) run_verbose ;;
        4) run_debug ;;
        5) run_debug_api ;;
        6) run_custom ;;
        q|Q|0) 
            echo
            echo "Exiting scx_gamer launcher."
            echo
            exit 0
            ;;
        *) 
            echo
            echo "Invalid selection: ${choice}"
            echo "Please choose 1-6 or q to quit."
            echo
            ;;
    esac
    echo
    echo "================================================================================"
    read -rp "Press Enter to return to the main menu..." _
    clear
done
