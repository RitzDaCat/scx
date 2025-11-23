#!/bin/bash
# Cache Line Analysis using pahole
# Analyzes actual struct layouts from compiled binary

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$PROJECT_ROOT/../../.." && pwd)"
BINARY="$REPO_ROOT/target/release/scx_gamer"

if [ ! -f "$BINARY" ]; then
    echo "Error: Binary not found at $BINARY"
    echo "Please build the project first with: ./build.sh"
    exit 1
fi

if ! command -v pahole &> /dev/null; then
    echo "Error: pahole not found"
    echo "Install with: sudo pacman -S pahole (Arch) or sudo apt install dwarves (Debian/Ubuntu)"
    exit 1
fi

echo "================================================================================"
echo "Cache Line Alignment Analysis (using pahole)"
echo "================================================================================"
echo "Binary: $BINARY"
echo "Cache line size: 64 bytes"
echo ""

# Analyze priority structs
echo "################################################################################"
echo "# PRIORITY STRUCTS (Hot Path)"
echo "################################################################################"
echo ""

for struct in task_ctx cpu_ctx hot_path_cache; do
    echo "--------------------------------------------------------------------------------"
    echo "Struct: $struct"
    echo "--------------------------------------------------------------------------------"
    
    if pahole -C "$struct" "$BINARY" 2>/dev/null; then
        size=$(pahole -C "$struct" "$BINARY" 2>/dev/null | grep -oP 'sizeof:\K\d+' || echo "unknown")
        if [ "$size" != "unknown" ]; then
            cache_lines=$((size / 64))
            remainder=$((size % 64))
            aligned=""
            if [ $remainder -eq 0 ]; then
                aligned="✓ YES"
            else
                aligned="✗ NO (needs $((64 - remainder)) more bytes)"
            fi
            
            echo ""
            echo "Size: $size bytes ($cache_lines full cache lines + $remainder bytes)"
            echo "Aligned to 64 bytes: $aligned"
            echo ""
            echo "--- Cache Line Boundaries ---"
            pahole --hex -C "$struct" "$BINARY" 2>/dev/null | while IFS= read -r line; do
                # Extract offset if present
                if echo "$line" | grep -qP '\/\*\s+0x[0-9a-f]+'; then
                    offset=$(echo "$line" | grep -oP '\/\*\s+0x\K[0-9a-f]+' | head -1)
                    offset_dec=$((16#$offset))
                    cache_line=$((offset_dec / 64))
                    
                    # Print cache line header when crossing boundary
                    if [ $offset_dec -gt 0 ] && [ $((offset_dec % 64)) -eq 0 ]; then
                        echo "    --- Cache Line $cache_line (bytes $((cache_line * 64))-$((cache_line * 64 + 63))) ---"
                    fi
                fi
                echo "    $line"
            done
        fi
    else
        echo "Struct not found in binary"
    fi
    echo ""
done

# Analyze all other structs
echo ""
echo "################################################################################"
echo "# ALL OTHER STRUCTS"
echo "################################################################################"
echo ""

# Get list of all structs
all_structs=$(pahole "$BINARY" 2>/dev/null | grep '^struct ' | awk '{print $2}' | sort -u)

for struct in $all_structs; do
    # Skip priority structs (already analyzed)
    if [ "$struct" = "task_ctx" ] || [ "$struct" = "cpu_ctx" ] || [ "$struct" = "hot_path_cache" ]; then
        continue
    fi
    
    # Skip kernel/system structs (not ours)
    if echo "$struct" | grep -qE '^(task_struct|sched_|rq|kernel|mm_|fs_|inode|dentry|file|path|sock|sk_|tcp_|udp_|ip_|net_|dev_)'; then
        continue
    fi
    
    size=$(pahole -C "$struct" "$BINARY" 2>/dev/null | grep -oP 'sizeof:\K\d+' || continue)
    
    # Only show structs relevant to scx_gamer (from our source)
    if [ "$size" -lt 1000 ] && [ "$size" -gt 0 ]; then
        cache_lines=$((size / 64))
        remainder=$((size % 64))
        aligned=""
        if [ $remainder -eq 0 ]; then
            aligned="✓"
        else
            aligned="✗"
        fi
        
        echo "[$aligned] $struct: $size bytes ($cache_lines lines + $remainder bytes)"
    fi
done

echo ""
echo "================================================================================"
echo "Analysis complete!"
echo "================================================================================"
echo ""
echo "Legend:"
echo "  ✓ = Aligned to 64-byte cache line boundary"
echo "  ✗ = NOT aligned (may cause cache line straddling)"
echo ""
echo "Fields that straddle cache line boundaries can cause:"
echo "  - False sharing (CPUs invalidating each other's caches)"
echo "  - Memory bus locking"
echo "  - Significant performance degradation"

