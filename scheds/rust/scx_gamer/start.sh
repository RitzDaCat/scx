#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
BIN_PATH="${REPO_ROOT}/target/release/scx_gamer"

build_scx() {
    echo
    echo "[scx_gamer] Building release binary..."
    cargo -C "${REPO_ROOT}" build -p scx_gamer --release
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
1) Baseline            Default settings, minimal optimizations
                       - Slice: 1000us (1ms)
                       - Use for: Testing, debugging

2) Casual Gaming       Balanced performance for general gaming
                       - Slice: 500us, MM affinity, NAPI preference
                       - Keyboard boost: 1500ms (ability chains)
                       - Mouse boost: 10ms (forgiving tracking)
                       - Use for: Single-player, RPGs, 60Hz monitors

3) Esports             Competitive gaming with aggressive tuning
                       - Slice: 250us, avoid SMT, max responsiveness
                       - Keyboard boost: 300ms (tight, less background penalty)
                       - Mouse boost: 6ms (covers 8000Hz polling)
                       - Use for: FPS, MOBAs, 144Hz-240Hz (RECOMMENDED)

4) NAPI Preference     Network-aware scheduling testing
                       - Slice: 500us, prefer-napi-on-input
                       - Use for: Online gaming optimization testing

5) Ultra-Latency       Extreme low-latency for competitive play
                       - Slice: 5us, real-time scheduling, 1-5us latency
                       - Keyboard boost: 100ms (minimal overhead)
                       - Mouse boost: 4ms (covers highest polling rates)
                       - Use for: Aim trainers, 360Hz+, <5% CPU usage

6) SCHED_DEADLINE      Hard real-time with guaranteed time bounds
                       - Kernel admission control, no starvation risk
                       - Use for: Maximum consistency and stability

NOTE: Profiles run WITHOUT monitoring (--stats/--monitor/--tui) for maximum
      performance. Ring buffer write is automatically skipped, saving ~20µs per
      event. Use TUI Dashboard (option 3) if you need visual monitoring.

q) Back to main menu

================================================================================
PROFILE
        read -rp "Select profile [1-6, q]: " profile_choice
        case "${profile_choice}" in
            1)
                echo
                echo "Profile: Baseline"
                echo "Purpose: Testing and debugging with default settings"
                echo
                echo "Active Flags:"
                echo "  --slice-us 1000                (1ms scheduling slice)"
                echo
                launch_scx "Baseline" \
                    --env "RUST_LOG=warn" \
                    --arg "--slice-us" \
                    --arg "1000"
                return
                ;;
            2)
                echo
                echo "Profile: Casual Gaming"
                echo "Purpose: Balanced performance for general gaming scenarios"
                echo
                echo "Active Flags:"
                echo "  --slice-us 500                 (500us scheduling slice)"
                echo "  --wakeup-timer-us 500          (Fast BPF timer sampling)"
                echo "  --keyboard-boost-us 1500000    (1500ms - covers ability chains)"
                echo "  --mouse-boost-us 10000         (10ms - forgiving tracking)"
                echo "  --preferred-idle-scan          (Intelligent CPU selection)"
                echo "  --mm-affinity                  (Cache-conscious placement)"
                echo "  --prefer-napi-on-input         (Network-aware scheduling)"
                echo
                launch_scx "Casual Gaming" \
                    --env "RUST_LOG=warn" \
                    --arg "--slice-us" \
                    --arg "500" \
                    --arg "--wakeup-timer-us" \
                    --arg "500" \
                    --arg "--keyboard-boost-us" \
                    --arg "1500000" \
                    --arg "--mouse-boost-us" \
                    --arg "10000" \
                    --arg "--preferred-idle-scan" \
                    --arg "--mm-affinity" \
                    --arg "--prefer-napi-on-input"
                return
                ;;
            3)
                echo
                echo "Profile: Esports"
                echo "Purpose: Competitive gaming with aggressive optimizations"
                echo
                echo "Active Flags:"
                echo "  --slice-us 250                 (250us aggressive preemption)"
                echo "  --wakeup-timer-us 250          (Responsive monitoring)"
                echo "  --input-window-us 8000         (8ms input boost window)"
                echo "  --keyboard-boost-us 300000     (300ms - tight, less background penalty)"
                echo "  --mouse-boost-us 6000          (6ms - covers 8000Hz polling)"
                echo "  --mig-max 4                    (Migration rate limiting)"
                echo "  --preferred-idle-scan          (Smart CPU placement)"
                echo "  --avoid-smt                    (Prevents SMT contention)"
                echo "  --prefer-napi-on-input         (Network interrupt awareness)"
                echo
                launch_scx "Esports" \
                    --env "RUST_LOG=warn" \
                    --arg "--slice-us" \
                    --arg "250" \
                    --arg "--wakeup-timer-us" \
                    --arg "250" \
                    --arg "--input-window-us" \
                    --arg "8000" \
                    --arg "--keyboard-boost-us" \
                    --arg "300000" \
                    --arg "--mouse-boost-us" \
                    --arg "6000" \
                    --arg "--mig-max" \
                    --arg "4" \
                    --arg "--preferred-idle-scan" \
                    --arg "--avoid-smt" \
                    --arg "--prefer-napi-on-input"
                return
                ;;
            4)
                echo
                echo "Profile: NAPI Preference"
                echo "Purpose: Testing network-aware scheduling optimizations"
                echo
                echo "Active Flags:"
                echo "  --slice-us 500                 (500us scheduling slice)"
                echo "  --preferred-idle-scan          (Smart CPU selection)"
                echo "  --prefer-napi-on-input         (Prefer NAPI-handling CPUs)"
                echo
                launch_scx "NAPI Preference" \
                    --env "RUST_LOG=warn" \
                    --arg "--slice-us" \
                    --arg "500" \
                    --arg "--preferred-idle-scan" \
                    --arg "--prefer-napi-on-input"
                return
                ;;
            5)
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
                echo "  - Performance: Ring buffer write skipped (no monitoring) for ~20µs faster latency"
                echo
                echo "Active Flags:"
                echo "  --realtime-scheduling          (SCHED_FIFO real-time policy)"
                echo "  --rt-priority 50               (Mid-range RT priority)"
                echo "  --slice-us 5                   (5us ultra-aggressive slice)"
                echo "  --input-window-us 2000         (2ms input boost window)"
                echo "  --keyboard-boost-us 100000     (100ms - minimal overhead)"
                echo "  --mouse-boost-us 4000          (4ms - covers highest polling rates)"
                echo "  --wakeup-timer-us 100          (100us BPF timer)"
                echo "  --avoid-smt                    (Avoids hyperthread contention)"
                echo "  --mig-max 2                    (Minimal migrations)"
                echo "  --preferred-idle-scan          (Smart CPU selection)"
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
                        --arg "--realtime-scheduling" \
                        --arg "--rt-priority" \
                        --arg "50" \
                        --arg "--slice-us" \
                        --arg "5" \
                        --arg "--input-window-us" \
                        --arg "2000" \
                        --arg "--keyboard-boost-us" \
                        --arg "100000" \
                        --arg "--mouse-boost-us" \
                        --arg "4000" \
                        --arg "--wakeup-timer-us" \
                        --arg "100" \
                        --arg "--avoid-smt" \
                        --arg "--mig-max" \
                        --arg "2" \
                        --arg "--preferred-idle-scan"
                else
                    echo
                    echo "Ultra-Latency mode cancelled."
                    echo
                fi
                return
                ;;
            6)
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
                echo "  --deadline-scheduling          (Enable SCHED_DEADLINE policy)"
                echo "  --deadline-runtime-us 800      (CPU time budget per period)"
                echo "  --deadline-deadline-us 1000    (Relative deadline)"
                echo "  --deadline-period-us 1000      (Scheduling period)"
                echo "  --input-window-us 2000         (2ms input boost window)"
                echo "  --keyboard-boost-us 300000     (300ms - competitive tight window)"
                echo "  --mouse-boost-us 6000          (6ms - covers high-rate polling)"
                echo "  --wakeup-timer-us 250          (250us BPF timer)"
                echo "  --avoid-smt                    (Avoids hyperthread contention)"
                echo "  --mig-max 4                    (Migration rate limiting)"
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
                        --arg "--deadline-scheduling" \
                        --arg "--deadline-runtime-us" \
                        --arg "800" \
                        --arg "--deadline-deadline-us" \
                        --arg "1000" \
                        --arg "--deadline-period-us" \
                        --arg "1000" \
                        --arg "--input-window-us" \
                        --arg "2000" \
                        --arg "--keyboard-boost-us" \
                        --arg "300000" \
                        --arg "--mouse-boost-us" \
                        --arg "6000" \
                        --arg "--wakeup-timer-us" \
                        --arg "250" \
                        --arg "--avoid-smt" \
                        --arg "--mig-max" \
                        --arg "4"
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

run_verbose() {
    echo
    echo "Mode: Verbose Statistics"
    echo "Statistics output interval: 1.0 seconds"
    echo
    launch_scx "Verbose Mode" \
        --env "RUST_LOG=info" \
        --arg "--stats" \
        --arg "1.0"
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

PROFILE                DESCRIPTION
--------------------------------------------------------------------------------
1) Baseline TUI        Default scheduler settings with TUI monitoring
                       - Minimal optimizations, good for testing

2) Casual Gaming TUI   Balanced settings with visual monitoring
                       - Preferred idle scan, MM affinity, NAPI preference

3) Esports TUI         Aggressive competitive tuning with monitoring
                       - Full optimization suite, recommended for gaming

4) NAPI Preference TUI Network-aware scheduling with monitoring
                       - Testing prefer-napi-on-input behavior

q) Back to main menu

================================================================================
TUI_PROFILE
        read -rp "Select TUI profile [1-4, q]: " profile_choice
        case "${profile_choice}" in
            1)
                echo
                echo "Launching: TUI Dashboard - Baseline"
                echo
                launch_scx "TUI Baseline" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            2)
                echo
                echo "Launching: TUI Dashboard - Casual Gaming"
                echo "Active optimizations: Preferred idle scan, MM affinity"
                echo
                launch_scx "TUI Casual Gaming" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}" \
                    --arg "--preferred-idle-scan" \
                    --arg "--mm-affinity"
                return
                ;;
            3)
                echo
                echo "Launching: TUI Dashboard - Esports"
                echo "Active optimizations: Full competitive tuning suite"
                echo
                launch_scx "TUI Esports" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}" \
                    --arg "--preferred-idle-scan" \
                    --arg "--disable-smt" \
                    --arg "--avoid-smt" \
                    --arg "--prefer-napi-on-input" \
                    --arg "--input-window-us" \
                    --arg "8000" \
                    --arg "--keyboard-boost-us" \
                    --arg "300000" \
                    --arg "--mouse-boost-us" \
                    --arg "6000" \
                    --arg "--wakeup-timer-us" \
                    --arg "250" \
                    --arg "--mig-max" \
                    --arg "2"
                return
                ;;
            4)
                echo
                echo "Launching: TUI Dashboard - NAPI Preference"
                echo "Active optimizations: Network-aware scheduling"
                echo
                launch_scx "TUI NAPI Preference" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}" \
                    --arg "--preferred-idle-scan" \
                    --arg "--prefer-napi-on-input"
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

run_ml_collect() {
    echo
    echo "Mode: Machine Learning Data Collection"
    echo "Sample interval: 5.0 seconds"
    echo "Data directory: ml_data/"
    echo
    echo "This mode collects scheduler performance metrics for training."
    echo "Let it run during gaming sessions for best results."
    echo
    launch_scx "ML Data Collection" \
        --env "RUST_LOG=info" \
        --arg "--ml-collect" \
        --arg "--ml-sample-interval" \
        --arg "5.0" \
        --arg "--stats" \
        --arg "2.0"
}

run_ml_profiles() {
    echo
    echo "Mode: Machine Learning Profile Manager"
    echo
    echo "This mode automatically loads optimized configurations based on"
    echo "the detected game. Requires previously collected ML data."
    echo
    launch_scx "ML Profile Manager" \
        --env "RUST_LOG=info" \
        --arg "--ml-profiles"
}

run_ml_full() {
    echo
    echo "Mode: Complete ML Pipeline"
    echo
    echo "Enables all ML features simultaneously:"
    echo "  - Data collection (saves to ml_data/)"
    echo "  - Auto-load per-game profiles"
    echo "  - Bayesian optimization"
    echo "  - Verbose statistics output"
    echo
    launch_scx "ML Full Pipeline" \
        --env "RUST_LOG=info" \
        --arg "--ml-collect" \
        --arg "--ml-profiles" \
        --arg "--ml-autotune" \
        --arg "--ml-bayesian" \
        --arg "--stats" \
        --arg "2.0" \
        --arg "--verbose"
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

run_custom() {
    ensure_binary
    echo
    echo "================================================================================"
    echo "                              CUSTOM FLAGS MODE"
    echo "================================================================================"
    echo
    echo "Enter your custom scx_gamer command-line arguments."
    echo "Example: --slice-us 100 --input-window-us 1000 --keyboard-boost-us 300000 --mouse-boost-us 6000 --verbose"
    echo
    echo "Available flags: Run 'scx_gamer --help' for complete list"
    echo "Key options:"
    echo "  --keyboard-boost-us <us>   Keyboard boost duration (default: 1000000 = 1000ms)"
    echo "                            Lower values (200-500ms) reduce background penalty"
    echo "  --mouse-boost-us <us>      Mouse boost duration (default: 8000 = 8ms)"
    echo "                            Lower values (4-6ms) reduce latency variance"
    echo "  --input-window-us <us>      Global input window (default: 5000 = 5ms)"
    echo ""
    echo "Performance Note:"
    echo "  When running WITHOUT --stats/--monitor/--tui, ring buffer write is"
    echo "  automatically skipped, saving ~20µs per event for maximum performance."
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

MODE                   DESCRIPTION
--------------------------------------------------------------------------------
1) Standard Profiles   Choose from preset gaming configurations
                       - Baseline, Casual, Esports, Ultra-Latency, etc.

2) Verbose Mode        Run with statistics output every 1 second
                       - Clean output, no extra logging

3) TUI Dashboard       Interactive terminal UI with real-time stats
                       - Visual performance monitoring (recommended)

4) ML Data Collection  Collect scheduler metrics for machine learning
                       - Saves performance data to ml_data/ directory

5) ML Profile Manager  Auto-load optimized configs per game
                       - Uses previously trained ML data

6) ML Full Pipeline    Complete ML workflow (collect + profiles + stats)
                       - Comprehensive machine learning optimization

7) Debug Mode          Maximum logging for troubleshooting
                       - RUST_LOG=debug, LIBBPF_LOG=debug

8) Custom Flags        Manually enter scheduler command-line arguments
                       - For advanced users and testing

q) Quit                Exit launcher

================================================================================
MENU
}

while true; do
    show_menu
    echo
    read -rp "Select mode [1-8, q]: " choice
    case "${choice}" in
        1) run_standard ;;
        2) run_verbose ;;
        3) run_tui ;;
        4) run_ml_collect ;;
        5) run_ml_profiles ;;
        6) run_ml_full ;;
        7) run_debug ;;
        8) run_custom ;;
        q|Q|0) 
            echo
            echo "Exiting scx_gamer launcher."
            echo
            exit 0
            ;;
        *) 
            echo
            echo "Invalid selection: ${choice}"
            echo "Please choose 1-8 or q to quit."
            echo
            ;;
    esac
    echo
    echo "================================================================================"
    read -rp "Press Enter to return to the main menu..." _
    clear
done
