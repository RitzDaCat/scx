#!/usr/bin/env bash
# scx_gamer launcher - Simple menu to start the scheduler
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
BINARY="${REPO_ROOT}/target/release/scx_gamer"

# Colors
GREEN=$'\033[0;32m'
YELLOW=$'\033[0;33m'
BLUE=$'\033[0;34m'
CYAN=$'\033[0;36m'
BOLD=$'\033[1m'
NC=$'\033[0m'

check_binary() {
    if [[ ! -x "${BINARY}" ]]; then
        echo "${YELLOW}Binary not found. Building...${NC}"
        (cd "${SCRIPT_DIR}" && ./build.sh --skip-check --release) || {
            echo "Build failed. Run ./build.sh manually."
            exit 1
        }
    fi
}

run_scheduler() {
    local mode_name="$1"
    shift
    
    check_binary
    
    echo
    echo "${GREEN}Starting scx_gamer: ${mode_name}${NC}"
    echo "Press Ctrl+C to stop."
    echo "${BLUE}────────────────────────────────────────────────────────────────────${NC}"
    echo
    
    # Pass D-Bus environment for focus detection
    local -a env_cmd=()
    [[ -n "${DBUS_SESSION_BUS_ADDRESS:-}" ]] && env_cmd+=("DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS}")
    [[ -n "${XDG_RUNTIME_DIR:-}" ]] && env_cmd+=("XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR}")
    
    if [[ ${#env_cmd[@]} -gt 0 ]]; then
        sudo env "${env_cmd[@]}" "${BINARY}" "$@"
    else
        sudo "${BINARY}" "$@"
    fi
}

show_menu() {
    clear 2>/dev/null || true
    echo
    echo "${BLUE}════════════════════════════════════════════════════════════════════${NC}"
    echo "${BOLD}                         SCX_GAMER LAUNCHER${NC}"
    echo "${BLUE}════════════════════════════════════════════════════════════════════${NC}"
    echo
    echo "  ${GREEN}1)${NC} Baseline"
    echo "     General desktop and light gaming (slice: 1ms)"
    echo
    echo "  ${GREEN}2)${NC} Esports"
    echo "     Competitive gaming, low latency (slice: 10µs)"
    echo
    echo "  ${YELLOW}3)${NC} Baseline Debug"
    echo "     Baseline + stats display for troubleshooting"
    echo
    echo "  ${YELLOW}4)${NC} Esports Debug"
    echo "     Esports + stats display for troubleshooting"
    echo
    echo "  ${CYAN}q)${NC} Quit"
    echo
    echo "${BLUE}────────────────────────────────────────────────────────────────────${NC}"
    echo
}

main() {
    while true; do
        show_menu
        read -rp "Select [1-4, q]: " choice
        
        case "${choice}" in
            1)
                run_scheduler "Baseline" \
                    --slice-us 1000 \
                    --no-stats
                ;;
            2)
                run_scheduler "Esports" \
                    --slice-us 10 \
                    --no-stats
                ;;
            3)
                run_scheduler "Baseline Debug" \
                    --slice-us 1000 \
                    --stats 2 \
                    --debug \
                    --verbose
                ;;
            4)
                run_scheduler "Esports Debug" \
                    --slice-us 10 \
                    --stats 2 \
                    --debug \
                    --verbose
                ;;
            q|Q)
                echo
                echo "Goodbye!"
                exit 0
                ;;
            *)
                echo "Invalid selection"
                sleep 1
                ;;
        esac
        
        echo
        read -rp "Press Enter to return to menu..." _
    done
}

main
