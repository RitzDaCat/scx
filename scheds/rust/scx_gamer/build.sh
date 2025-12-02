#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/../../.." && pwd)"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

print_status() {
    local status=$1
    local name=$2
    local detail=${3:-""}
    
    if [ "$status" = "ok" ]; then
        printf "  ${GREEN}[OK]${NC}    %-20s %s\n" "$name" "$detail"
    elif [ "$status" = "missing" ]; then
        printf "  ${RED}[MISSING]${NC} %-20s %s\n" "$name" "$detail"
    elif [ "$status" = "warn" ]; then
        printf "  ${YELLOW}[WARN]${NC}  %-20s %s\n" "$name" "$detail"
    fi
}

check_dependencies() {
    echo
    echo "================================================================================"
    echo "                      CHECKING BUILD DEPENDENCIES"
    echo "================================================================================"
    echo
    
    local missing_pkgs=()
    local all_ok=true
    
    # -------------------------------------------------------------------------
    # Required Commands
    # -------------------------------------------------------------------------
    echo "${BLUE}Required Commands:${NC}"
    
    # Rust toolchain
    if command -v cargo &>/dev/null; then
        local cargo_ver=$(cargo --version 2>/dev/null | head -1)
        print_status "ok" "cargo" "$cargo_ver"
    else
        print_status "missing" "cargo" "(install via rustup)"
        missing_pkgs+=("rustup")
        all_ok=false
    fi
    
    if command -v rustc &>/dev/null; then
        local rustc_ver=$(rustc --version 2>/dev/null | head -1)
        print_status "ok" "rustc" "$rustc_ver"
    else
        print_status "missing" "rustc" "(install via rustup)"
        all_ok=false
    fi
    
    # Clang/LLVM (required for BPF compilation)
    if command -v clang &>/dev/null; then
        local clang_ver=$(clang --version 2>/dev/null | head -1)
        print_status "ok" "clang" "$clang_ver"
    else
        print_status "missing" "clang" "(required for BPF)"
        missing_pkgs+=("clang")
        all_ok=false
    fi
    
    if command -v llvm-strip &>/dev/null; then
        print_status "ok" "llvm-strip" ""
    else
        print_status "missing" "llvm-strip" "(part of llvm)"
        missing_pkgs+=("llvm")
        all_ok=false
    fi
    
    # bpftool
    if command -v bpftool &>/dev/null; then
        local bpftool_ver=$(bpftool version 2>/dev/null | head -1 || echo "installed")
        print_status "ok" "bpftool" "$bpftool_ver"
    else
        print_status "missing" "bpftool" "(required for BPF)"
        missing_pkgs+=("bpf")
        all_ok=false
    fi
    
    # pkg-config
    if command -v pkg-config &>/dev/null || command -v pkgconf &>/dev/null; then
        print_status "ok" "pkg-config" ""
    else
        print_status "missing" "pkg-config" ""
        missing_pkgs+=("pkgconf")
        all_ok=false
    fi
    
    # strip (binutils)
    if command -v strip &>/dev/null; then
        print_status "ok" "strip" ""
    else
        print_status "missing" "strip" "(binutils)"
        missing_pkgs+=("binutils")
        all_ok=false
    fi
    
    # make (often needed)
    if command -v make &>/dev/null; then
        print_status "ok" "make" ""
    else
        print_status "missing" "make" ""
        missing_pkgs+=("make")
        all_ok=false
    fi
    
    echo
    
    # -------------------------------------------------------------------------
    # Required Libraries
    # -------------------------------------------------------------------------
    echo "${BLUE}Required Libraries:${NC}"
    
    # libbpf
    if pkg-config --exists libbpf 2>/dev/null; then
        local libbpf_ver=$(pkg-config --modversion libbpf 2>/dev/null || echo "installed")
        print_status "ok" "libbpf" "v$libbpf_ver"
    else
        print_status "missing" "libbpf" ""
        missing_pkgs+=("libbpf")
        all_ok=false
    fi
    
    # libelf
    if pkg-config --exists libelf 2>/dev/null; then
        local libelf_ver=$(pkg-config --modversion libelf 2>/dev/null || echo "installed")
        print_status "ok" "libelf" "v$libelf_ver"
    else
        print_status "missing" "libelf" ""
        missing_pkgs+=("libelf")
        all_ok=false
    fi
    
    # zlib
    if pkg-config --exists zlib 2>/dev/null; then
        local zlib_ver=$(pkg-config --modversion zlib 2>/dev/null || echo "installed")
        print_status "ok" "zlib" "v$zlib_ver"
    else
        print_status "missing" "zlib" ""
        missing_pkgs+=("zlib")
        all_ok=false
    fi
    
    echo
    
    # -------------------------------------------------------------------------
    # Kernel Headers (for vmlinux.h generation)
    # -------------------------------------------------------------------------
    echo "${BLUE}Kernel Headers:${NC}"
    
    local kernel_ver=$(uname -r)
    local headers_path="/usr/lib/modules/${kernel_ver}/build"
    
    if [ -d "$headers_path" ]; then
        print_status "ok" "linux-headers" "$kernel_ver"
    else
        print_status "missing" "linux-headers" "for kernel $kernel_ver"
        # CachyOS uses linux-cachyos-headers
        if echo "$kernel_ver" | grep -q "cachyos"; then
            missing_pkgs+=("linux-cachyos-headers")
        else
            missing_pkgs+=("linux-headers")
        fi
        all_ok=false
    fi
    
    echo
    
    # -------------------------------------------------------------------------
    # Handle Missing Dependencies
    # -------------------------------------------------------------------------
    if [ "$all_ok" = false ]; then
        # Deduplicate missing packages
        local unique_pkgs=($(printf '%s\n' "${missing_pkgs[@]}" | sort -u))
        
        echo "================================================================================"
        echo -e "  ${RED}MISSING DEPENDENCIES DETECTED${NC}"
        echo "================================================================================"
        echo
        echo "The following packages need to be installed:"
        echo
        printf '  %s\n' "${unique_pkgs[@]}"
        echo
        
        # Check if we're on Arch/pacman
        if command -v pacman &>/dev/null; then
            echo "Detected package manager: pacman (Arch Linux)"
            echo
            
            # Build install command
            local install_cmd="sudo pacman -S --needed"
            for pkg in "${unique_pkgs[@]}"; do
                install_cmd+=" $pkg"
            done
            
            echo "Install command:"
            echo -e "  ${YELLOW}${install_cmd}${NC}"
            echo
            
            read -rp "Would you like to install missing dependencies now? [y/N]: " answer
            case "${answer,,}" in
                y|yes)
                    echo
                    echo "Installing dependencies..."
                    eval "$install_cmd"
                    
                    # Handle rustup separately if needed
                    if [[ " ${unique_pkgs[*]} " =~ " rustup " ]]; then
                        echo
                        echo "Initializing Rust toolchain..."
                        rustup default stable
                    fi
                    
                    echo
                    echo -e "${GREEN}Dependencies installed successfully!${NC}"
                    echo "Re-running dependency check..."
                    echo
                    # Recursive check after install
                    check_dependencies
                    return $?
                    ;;
                *)
                    echo
                    echo "Please install the missing dependencies and try again."
                    echo "Exiting."
                    exit 1
                    ;;
            esac
        else
            echo "Could not detect pacman. Please install the missing packages manually:"
            printf '  - %s\n' "${unique_pkgs[@]}"
            echo
            echo "Exiting."
            exit 1
        fi
    else
        echo -e "${GREEN}All dependencies satisfied!${NC}"
        echo
        return 0
    fi
}

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
    local build_dir="${REPO_ROOT}/target/release/build"
    
    # Only search if the build directory exists (fresh clone won't have it)
    if [ -d "$build_dir" ]; then
        bpf_h_tar=$(find "$build_dir" -path "*/scx_utils-*/out/bpf_h.tar" 2>/dev/null | head -1) || true
    fi
    
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
    if [ -d "${REPO_ROOT}/target/release/build" ]; then
        find "${REPO_ROOT}/target/release/build" -maxdepth 1 -type d -name "scx_gamer-*" -exec rm -rf {} + 2>/dev/null || true
    fi
    if [ -d "${REPO_ROOT}/target/debug/build" ]; then
        find "${REPO_ROOT}/target/debug/build" -maxdepth 1 -type d -name "scx_gamer-*" -exec rm -rf {} + 2>/dev/null || true
    fi
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
    
    echo
    echo "================================================================================"
    echo "                         BUILD COMPLETE (DEBUG + PROFILING)"
    echo "================================================================================"
    echo
    echo "Binary location: ${REPO_ROOT}/target/debug/scx_gamer"
    echo "Size: $(du -h "${REPO_ROOT}/target/debug/scx_gamer" 2>/dev/null | cut -f1 || echo "unknown")"
    echo
    echo "Profiling enabled: Latency histograms will be populated."
    echo "Use --stats <interval> to view scheduler metrics."
    echo
}

show_help() {
    cat <<'HELP'
Usage: build.sh [OPTIONS]

Options:
    --skip-check    Skip dependency checking
    --release       Build release directly (no menu)
    --debug         Build debug directly (no menu)
    -h, --help      Show this help message

Examples:
    ./build.sh                  # Interactive menu with dep check
    ./build.sh --skip-check     # Interactive menu, skip dep check
    ./build.sh --release        # Direct release build
    ./build.sh --debug          # Direct debug build

HELP
}

main() {
    local skip_check=false
    local direct_build=""
    
    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --skip-check)
                skip_check=true
                shift
                ;;
            --release)
                direct_build="release"
                shift
                ;;
            --debug)
                direct_build="debug"
                shift
                ;;
            -h|--help)
                show_help
                exit 0
                ;;
            *)
                echo "Unknown option: $1"
                show_help
                exit 1
                ;;
        esac
    done
    
    # Check dependencies unless skipped
    if [ "$skip_check" = false ]; then
        check_dependencies
    fi
    
    # Direct build mode (non-interactive)
    if [ -n "$direct_build" ]; then
        case "$direct_build" in
            release)
                build_release
                ;;
            debug)
                build_debug
                ;;
        esac
        exit 0
    fi
    
    # Interactive menu
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

main "$@"
