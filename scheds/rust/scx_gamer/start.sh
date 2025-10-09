#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"
BIN_PATH="${REPO_ROOT}/target/release/scx_gamer"

SCX_EXTRA_ARGS=()

build_scx() {
    echo "[scx_gamer] Building release binary (cargo build -p scx_gamer --release)..."
    cargo -C "${REPO_ROOT}" build -p scx_gamer --release
}

ensure_binary() {
    if [[ ! -x "${BIN_PATH}" ]]; then
        build_scx
    fi
}

read_extra_args() {
    SCX_EXTRA_ARGS=()
    local line
    read -rp "Additional scx_gamer args (optional): " line
    if [[ -n "${line}" ]]; then
        # shellcheck disable=SC2206
        SCX_EXTRA_ARGS=(${line})
    fi
}

run_silent() {
    ensure_binary
    echo
    echo "=== Running scx_gamer (silent baseline) ==="
    echo "Press Ctrl+C to stop the scheduler and return to this menu."
    read_extra_args
    sudo "${BIN_PATH}" "${SCX_EXTRA_ARGS[@]}"
}

run_verbose() {
    ensure_binary
    echo
    echo "=== Running scx_gamer with verbose logging ==="
    echo "Press Ctrl+C to stop the scheduler and return to this menu."
    read_extra_args
    sudo env RUST_LOG=info "${BIN_PATH}" --verbose "${SCX_EXTRA_ARGS[@]}"
}

run_tui() {
    ensure_binary
    echo
    local interval
    read -rp "TUI update interval in seconds [1.0]: " interval
    interval="${interval:-1.0}"
    echo "=== Running scx_gamer TUI (interval ${interval}s) ==="
    echo "Press Ctrl+C to exit the TUI and return to this menu."
    read_extra_args
    sudo "${BIN_PATH}" --tui "${interval}" "${SCX_EXTRA_ARGS[@]}"
}

run_diagnostics() {
    ensure_binary
    echo
    echo "=== Running scx_gamer diagnostics (libbpf + debug logging) ==="
    echo "Press Ctrl+C to stop the scheduler and return to this menu."
    read_extra_args
    sudo env RUST_LOG=debug LIBBPF_LOG=debug "${BIN_PATH}" --verbose --stats 1 "${SCX_EXTRA_ARGS[@]}"
}

show_menu() {
    cat <<'MENU'

scx_gamer launcher
===================
1) Run scheduler (silent)
2) Run scheduler (verbose logging)
3) Run TUI dashboard
4) Diagnostics (verbose + libbpf debug)
5) Build only (cargo build -p scx_gamer --release)
0) Exit
MENU
}

while true; do
    show_menu
    read -rp "Select an option: " choice
    case "${choice}" in
        1) run_silent ;;
        2) run_verbose ;;
        3) run_tui ;;
        4) run_diagnostics ;;
        5) build_scx ;;
        0|q|Q) echo "Exiting."; exit 0 ;;
        *) echo "Invalid choice: ${choice}" ;;
    esac

    echo
    read -rp "Press Enter to return to the menu..." _
    echo
done
