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
    # Cleaning is now handled by check_and_setup_workspace()
    # This function is kept for compatibility but does nothing
    :
}

check_and_setup_workspace() {
    # Check if scx_utils has been built with bpf_h.tar (contains vmlinux.h)
    # Different RUSTFLAGS/environments create different build hashes, so we check
    # for the actual bpf_h.tar file which contains vmlinux.h
    
    local bpf_h_tar=""
    bpf_h_tar=$(find "${REPO_ROOT}/target/release/build/scx_utils-"*/out -name "bpf_h.tar" 2>/dev/null | head -1)
    
    if [ -z "$bpf_h_tar" ]; then
        echo
        echo "================================================================================"
        echo "  FIRST-TIME SETUP: Building workspace dependencies (vmlinux.h)"
        echo "================================================================================"
        echo
        echo "This is required on first build or after 'cargo clean'."
        echo "Subsequent builds will be much faster (incremental)."
        echo
        
        # Build scx_utils first to generate vmlinux.h
        echo "Building scx_utils (contains vmlinux.h for BPF)..."
        cargo build -p scx_utils --release
        
        echo
        echo "Workspace setup complete. Continuing with scx_gamer build..."
        echo
    fi
    
    # Clean stale scx_gamer build artifacts to ensure fresh BPF compilation
    # Different environments (terminal vs IDE) create different build hashes
    # This ensures vmlinux.h is properly extracted for this environment
    echo "Step 2: Cleaning stale scx_gamer build artifacts..."
    rm -rf "${REPO_ROOT}/target/release/build/scx_gamer-"* 2>/dev/null || true
    rm -rf "${REPO_ROOT}/target/debug/build/scx_gamer-"* 2>/dev/null || true
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
    
    # Detect CPU architecture for optimal flags
    local cpu_model
    cpu_model=$(grep -m1 "model name" /proc/cpuinfo | cut -d: -f2 | xargs)
    echo "Detected CPU: ${cpu_model}"
    
    # Check if Zen 5 (9800X3D, 9900X, 9950X, etc.)
    if echo "${cpu_model}" | grep -qiE "9[0-9]{3}X"; then
        echo "Architecture: AMD Zen 5 (znver5) - Using znver5 optimizations"
        local TARGET_CPU="znver5"
    elif echo "${cpu_model}" | grep -qiE "7[0-9]{3}X|5[0-9]{3}X"; then
        echo "Architecture: AMD Zen 4 (znver4) - Using znver4 optimizations"
        local TARGET_CPU="znver4"
    else
        echo "Architecture: Unknown - Using native auto-detection"
        local TARGET_CPU="native"
    fi
    echo
    
    cd "${REPO_ROOT}"
    
    # Check if workspace needs initial setup (vmlinux.h generation)
    check_and_setup_workspace
    
    clean_build
    
    echo "Step 3: Building scx_gamer (release)..."
    echo "Target CPU: ${TARGET_CPU}"
    echo "LTO: fat (from workspace Cargo.toml)"
    echo "Codegen units: 1 (from workspace Cargo.toml)"
    echo
    
    # Set RUSTFLAGS for maximum performance on your CPU architecture
    # - target-cpu: Enables znver5 (Zen 5) specific optimizations for 9800X3D
    # - embed-bitcode: Required for LTO to work across crates
    #
    # Note: This creates a different build hash than IDE builds (without these flags)
    # The check_and_setup_workspace() function cleans stale artifacts to handle this.
    export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=${TARGET_CPU} -C embed-bitcode=yes"
    
    cargo build -p scx_gamer --release
    
    # Strip debug symbols for smaller, faster binary
    local BINARY="${REPO_ROOT}/target/release/scx_gamer"
    local SIZE_BEFORE=$(du -h "${BINARY}" 2>/dev/null | cut -f1 || echo "unknown")
    
    echo
    echo "Step 4: Stripping debug symbols..."
    strip --strip-all "${BINARY}"
    
    local SIZE_AFTER=$(du -h "${BINARY}" 2>/dev/null | cut -f1 || echo "unknown")
    
    echo
    echo "================================================================================"
    echo "                         BUILD COMPLETE (RELEASE)"
    echo "================================================================================"
    echo
    echo "Binary location: ${BINARY}"
    echo "Size before strip: ${SIZE_BEFORE}"
    echo "Size after strip:  ${SIZE_AFTER}"
    echo "Target CPU: ${TARGET_CPU}"
    echo
    
    # Show optimization summary
    echo "Optimizations applied:"
    echo "  - Target CPU: ${TARGET_CPU} (architecture-specific instructions)"
    echo "  - LTO: fat (cross-crate optimization)"
    echo "  - Codegen units: 1 (maximum inlining)"
    echo "  - Strip: --strip-all (minimal binary size)"
    echo
    
    # Verify binary is stripped
    if file "${BINARY}" | grep -q "not stripped"; then
        echo "WARNING: Binary still contains debug info"
    else
        echo "Binary successfully stripped"
    fi
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
    
    # Check if workspace needs initial setup (vmlinux.h generation)
    check_and_setup_workspace
    
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

