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
    echo "=== Launching scx_gamer (${mode_desc}) ==="
    echo "Press Ctrl+C to stop and return to the menu."
    prompt_extra_flags

    if (( ${#env_vars[@]} > 0 )); then
        sudo env "${env_vars[@]}" "${BIN_PATH}" "${base_args[@]}" "${EXTRA_FLAGS[@]}"
    else
        sudo "${BIN_PATH}" "${base_args[@]}" "${EXTRA_FLAGS[@]}"
    fi
}

run_standard() {
    while true; do
        cat <<'PROFILE'

Standard Profiles:
  1) Baseline      - Minimal changes (no additional flags)
  2) Casual        - Balanced responsiveness + locality
  3) Esports       - Maximum responsiveness, aggressive tuning
  4) NAPI Prefer   - Test prefer-napi-on-input bias
  5) Ultra-Latency - Busy polling ultra-low latency (9800X3D)
  6) Deadline-Mode - SCHED_DEADLINE hard real-time guarantees
  q) Back
PROFILE
        read -rp "Select profile: " profile_choice
        case "${profile_choice}" in
            1)
                launch_scx "Standard - Baseline" \
                    --env "RUST_LOG=warn"
                return
                ;;
            2)
                launch_scx "Standard - Casual" \
                    --env "RUST_LOG=warn" \
                    --arg "--preferred-idle-scan" \
                    --arg "--mm-affinity" \
                    --arg "--prefer-napi-on-input" \
                    --arg "--wakeup-timer-us" \
                    --arg "400"
                return
                ;;
            3)
                launch_scx "Standard - Esports" \
                    --env "RUST_LOG=warn" \
                    --arg "--preferred-idle-scan" \
                    --arg "--disable-smt" \
                    --arg "--avoid-smt" \
                    --arg "--prefer-napi-on-input" \
                    --arg "--input-window-us" \
                    --arg "8000" \
                    --arg "--wakeup-timer-us" \
                    --arg "250" \
                    --arg "--mig-max" \
                    --arg "2"
                return
                ;;
            4)
                launch_scx "Standard - NAPI Prefer" \
                    --env "RUST_LOG=warn" \
                    --arg "--preferred-idle-scan" \
                    --arg "--prefer-napi-on-input"
                return
                ;;
            5)
                echo
                echo "✅ Ultra-Latency Mode (Busy Polling)"
                echo "   • Consumes 100% of CPU core 7 for input handling"
                echo "   • Busy polling for ultra-low latency"
                echo "   • No real-time scheduling (safer)"
                echo "   • Recommended for competitive gaming"
                echo
                launch_scx "Standard - Ultra-Latency (9800X3D)" \
                    --env "RUST_LOG=warn" \
                    --arg "--event-loop-cpu" \
                    --arg "7" \
                    --arg "--busy-polling" \
                    --arg "--slice-us" \
                    --arg "5" \
                    --arg "--input-window-us" \
                    --arg "1000" \
                    --arg "--wakeup-timer-us" \
                    --arg "50" \
                    --arg "--avoid-smt" \
                    --arg "--mig-max" \
                    --arg "2"
                return
                ;;
            6)
                echo
                echo "🚀 SCHED_DEADLINE Mode (Hard Real-Time)"
                echo "   • Ultra-low latency with time guarantees"
                echo "   • No starvation risk (kernel admission control)"
                echo "   • Hard real-time guarantees"
                echo "   • Requires CONFIG_SCHED_DEADLINE kernel support"
                echo
                read -rp "Continue? (y/N): " confirm
                if [[ "${confirm}" =~ ^[Yy]$ ]]; then
                    launch_scx "Standard - SCHED_DEADLINE (9800X3D)" \
                        --env "RUST_LOG=warn" \
                        --arg "--event-loop-cpu" \
                        --arg "7" \
                        --arg "--busy-polling" \
                        --arg "--deadline-scheduling" \
                        --arg "--deadline-runtime-us" \
                        --arg "500" \
                        --arg "--deadline-deadline-us" \
                        --arg "1000" \
                        --arg "--deadline-period-us" \
                        --arg "1000" \
                        --arg "--slice-us" \
                        --arg "5" \
                        --arg "--input-window-us" \
                        --arg "1000" \
                        --arg "--wakeup-timer-us" \
                        --arg "50" \
                        --arg "--avoid-smt" \
                        --arg "--mig-max" \
                        --arg "2"
                else
                    echo "SCHED_DEADLINE mode cancelled."
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
    launch_scx "Verbose" \
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

TUI Profiles:
  1) Baseline      - Launch TUI with default scheduler settings
  2) Casual        - TUI + preferred idle scan, mm affinity
  3) Esports       - TUI + aggressive competitive tuning
  4) NAPI Prefer   - TUI + prefer-napi-on-input testing
  q) Back
TUI_PROFILE
        read -rp "Select TUI profile: " profile_choice
        case "${profile_choice}" in
            1)
                launch_scx "TUI - Baseline" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}"
                return
                ;;
            2)
                launch_scx "TUI - Casual" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}" \
                    --arg "--preferred-idle-scan" \
                    --arg "--mm-affinity"
                return
                ;;
            3)
                launch_scx "TUI - Esports" \
                    --env "RUST_LOG=info" \
                    --arg "--tui" \
                    --arg "${interval}" \
                    --arg "--preferred-idle-scan" \
                    --arg "--disable-smt" \
                    --arg "--avoid-smt" \
                    --arg "--prefer-napi-on-input" \
                    --arg "--input-window-us" \
                    --arg "8000" \
                    --arg "--wakeup-timer-us" \
                    --arg "250" \
                    --arg "--mig-max" \
                    --arg "2"
                return
                ;;
            4)
                launch_scx "TUI - NAPI Prefer" \
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
    launch_scx "ML Collect" \
        --env "RUST_LOG=info" \
        --arg "--ml-collect" \
        --arg "--ml-sample-interval" \
        --arg "5.0" \
        --arg "--stats" \
        --arg "2.0"
}

run_ml_profiles() {
    launch_scx "ML Profiles" \
        --env "RUST_LOG=info" \
        --arg "--ml-profiles"
}

run_ml_full() {
    launch_scx "ML Full" \
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
    launch_scx "Debug" \
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
    local line
    read -rp "Enter custom scx_gamer flags: " line
    if [[ -z "${line}" ]]; then
        echo "No flags provided."
        return
    fi
    read -ra CUSTOM_ARGS <<<"${line}"
    echo "=== Launching scx_gamer (custom flags) ==="
    sudo "${BIN_PATH}" "${CUSTOM_ARGS[@]}"
}

show_menu() {
    cat <<'MENU'
Select launch mode:

  1) Standard        - Silent operation (no output)
  2) Verbose         - Show stats every 1s (clean output)
  3) TUI Dashboard   - Interactive terminal UI (recommended)
  4) ML Collect      - Collect training data (saves to ml_data/)
  5) ML Profiles     - Auto-load per-game configs
  6) ML Full         - Collect + Profiles + Verbose stats
  7) Debug           - Maximum logging (for troubleshooting)
  8) Custom          - Enter your own flags

  q) Quit
MENU
}

while true; do
    show_menu
    echo
    read -rp "Choice: " choice
    case "${choice}" in
        1) run_standard ;;
        2) run_verbose ;;
        3) run_tui ;;
        4) run_ml_collect ;;
        5) run_ml_profiles ;;
        6) run_ml_full ;;
        7) run_debug ;;
        8) run_custom ;;
        q|Q|0) echo "Exiting."; exit 0 ;;
        *) echo "Invalid choice: ${choice}" ;;
    esac
    echo
    read -rp "Press Enter to return to the menu..." _
    echo
done
