#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

show_menu() {
    cat <<'MENU'

================================================================================
                         SCX_GAMER BUILD MENU
================================================================================

BUILD TYPE            DESCRIPTION
--------------------------------------------------------------------------------
1) Release             Optimized production build (default)
                      - Full optimizations (-O3)
                      - Stripped symbols
                      - Best performance, no profiling overhead
                      - Binary: target/release/scx_gamer

2) Debug               Development build with profiling enabled
                      - Debug symbols (-g)
                      - Profiling enabled (ENABLE_PROFILING)
                      - Latency histograms active
                      - ~50-150ns overhead per scheduling decision
                      - Binary: target/debug/scx_gamer

3) Exit                Quit without building

================================================================================

MENU
}

clean_build() {
    echo
    echo "Step 1: Removing target/ directory..."
    rm -rf target/
    
    echo "Step 2: Running cargo clean..."
    cargo clean
}

build_release() {
    echo
    echo "================================================================================"
    echo "                    BUILDING SCX_GAMER (RELEASE)"
    echo "================================================================================"
    echo
    echo "Repository root: ${REPO_ROOT}"
    echo "Build type: Release (optimized, no profiling)"
    echo
    
    cd "${REPO_ROOT}"
    
    clean_build
    
    echo "Step 3: Building scx_gamer (release)..."
    cargo build -p scx_gamer --release
    
    echo
    echo "================================================================================"
    echo "                         BUILD COMPLETE (RELEASE)"
    echo "================================================================================"
    echo
    echo "Binary location: ${REPO_ROOT}/target/release/scx_gamer"
    echo "Size: $(du -h "${REPO_ROOT}/target/release/scx_gamer" 2>/dev/null | cut -f1 || echo "unknown")"
    echo
}

build_debug() {
    echo
    echo "================================================================================"
    echo "                    BUILDING SCX_GAMER (DEBUG + PROFILING)"
    echo "================================================================================"
    echo
    echo "Repository root: ${REPO_ROOT}"
    echo "Build type: Debug with profiling enabled"
    echo "Profiling: ENABLE_PROFILING flag active"
    echo "Note: Adds ~50-150ns overhead per scheduling decision"
    echo
    
    cd "${REPO_ROOT}"
    
    clean_build
    
    echo "Step 3: Building scx_gamer (debug with profiling)..."
    echo "Setting SCX_GAMER_ENABLE_PROFILING=1 for BPF compilation..."
    # Use environment variable that build.rs will read (avoids CFLAGS conflicts)
    export SCX_GAMER_ENABLE_PROFILING=1
    cargo build -p scx_gamer
    
    # Verify profiling was enabled (check if latency percentiles populate in API)
    echo
    echo "NOTE: To verify profiling is active, check debug API latency percentiles."
    echo "      If select_cpu_latency_p50 > 0, profiling is working."
    
    echo
    echo "================================================================================"
    echo "                         BUILD COMPLETE (DEBUG + PROFILING)"
    echo "================================================================================"
    echo
    echo "Binary location: ${REPO_ROOT}/target/debug/scx_gamer"
    echo "Size: $(du -h "${REPO_ROOT}/target/debug/scx_gamer" 2>/dev/null | cut -f1 || echo "unknown")"
    echo
    echo "Profiling enabled: Latency histograms will be populated."
    echo "API metrics available: select_cpu_latency_p*, enqueue_latency_p*, dispatch_latency_p*"
    echo
}

main() {
    while true; do
        show_menu
        read -rp "Select build type (1-3): " choice
        
        case "${choice}" in
            1)
                build_release
                break
                ;;
            2)
                build_debug
                break
                ;;
            3)
                echo
                echo "Exiting without building."
                exit 0
                ;;
            *)
                echo
                echo "Invalid choice. Please select 1, 2, or 3."
                echo
                sleep 1
                ;;
        esac
    done
}

main

