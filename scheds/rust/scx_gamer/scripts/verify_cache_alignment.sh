#!/bin/bash
# Quick cache line alignment verification using pahole
# Compiles BPF code and analyzes struct layouts

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BPF_DIR="$PROJECT_ROOT/src/bpf"
TEMP_OBJ="/tmp/scx_gamer_cache_check.bpf.o"

echo "================================================================================"
echo "Cache Line Alignment Verification (64-byte boundaries)"
echo "================================================================================"
echo ""

# Check dependencies
if ! command -v clang &> /dev/null; then
    echo "Error: clang not found. Install with: sudo pacman -S clang"
    exit 1
fi

if ! command -v pahole &> /dev/null; then
    echo "Error: pahole not found. Install with: sudo pacman -S pahole"
    exit 1
fi

# Compile BPF code with debug symbols for pahole
echo "[1/3] Compiling BPF code with debug symbols..."
cd "$BPF_DIR"
clang -g -O2 -target bpf -c main.bpf.c -o "$TEMP_OBJ" \
  -I. -I./include -I/usr/include \
  -Wno-unknown-attributes \
  -D__BPF__ \
  2>&1 | grep -v "warning:" || true

if [ ! -f "$TEMP_OBJ" ]; then
    echo "Error: Failed to compile BPF code"
    exit 1
fi

echo "✓ Compiled: $TEMP_OBJ"
echo ""

# Analyze priority structs
echo "[2/3] Analyzing hot-path structs with pahole..."
echo ""

analyze_struct() {
    local struct_name=$1
    local expected_size=$2
    
    echo "--------------------------------------------------------------------------------"
    echo "Struct: $struct_name"
    echo "--------------------------------------------------------------------------------"
    
    if ! pahole -C "$struct_name" "$TEMP_OBJ" &>/dev/null; then
        echo "⚠️  Struct not found in compiled code"
        echo ""
        return
    fi
    
    local actual_size=$(pahole -C "$struct_name" "$TEMP_OBJ" 2>/dev/null | \
                       grep -oP 'sizeof:\K\d+' || echo "0")
    
    if [ "$actual_size" -eq 0 ]; then
        echo "⚠️  Could not determine size"
        echo ""
        return
    fi
    
    local cache_lines=$((actual_size / 64))
    local remainder=$((actual_size % 64))
    
    echo "Size: $actual_size bytes"
    echo "Layout: $cache_lines full cache lines + $remainder bytes"
    
    if [ "$expected_size" != "0" ]; then
        if [ "$actual_size" -eq "$expected_size" ]; then
            echo "Expected: $expected_size bytes ✓ MATCH"
        else
            echo "Expected: $expected_size bytes ✗ MISMATCH"
        fi
    fi
    
    if [ $remainder -eq 0 ]; then
        echo "Alignment: ✓ ALIGNED to 64-byte boundary"
    else
        echo "Alignment: ✗ NOT ALIGNED (needs $((64 - remainder)) more bytes)"
        echo ""
        echo "⚠️  WARNING: This struct will cause cache line straddling!"
        echo "   Add padding: u8 _cache_line_padding[$((64 - remainder))];"
    fi
    
    echo ""
    echo "Field layout:"
    pahole -C "$struct_name" "$TEMP_OBJ" 2>/dev/null | grep -v "^struct" | head -20
    
    echo ""
    echo "Cache line boundaries (fields at offsets 64, 128, 192, etc.):"
    pahole --hex -C "$struct_name" "$TEMP_OBJ" 2>/dev/null | \
      awk '/\/\* *0x[0-9a-f]+/ {
        offset_hex = substr($2, 3);
        offset_dec = strtonum("0x" offset_hex);
        if (offset_dec % 64 == 0 && offset_dec > 0) {
          cache_line = offset_dec / 64;
          printf "    --- Cache Line %d (bytes %d-%d) ---\n", cache_line, offset_dec, offset_dec + 63;
        }
        print "   ", $0;
      }' | head -30
    
    echo ""
}

# Analyze hot-path structs
analyze_struct "task_ctx" 384
analyze_struct "cpu_ctx" 128
analyze_struct "hot_path_cache" 32

# Cleanup
echo "[3/3] Cleanup..."
rm -f "$TEMP_OBJ"
echo "✓ Removed temporary object file"
echo ""

echo "================================================================================"
echo "Verification Complete!"
echo "================================================================================"
echo ""
echo "Summary:"
echo "  - task_ctx should be 384 bytes (6 × 64-byte cache lines)"
echo "  - cpu_ctx should be 128 bytes (2 × 64-byte cache lines)"  
echo "  - hot_path_cache should be 32 bytes (½ cache line, stack-allocated OK)"
echo ""
echo "If any struct shows ✗ NOT ALIGNED:"
echo "  1. Add explicit padding at the end of the struct definition"
echo "  2. Rebuild and re-run this script"
echo "  3. Verify the _Static_assert() passes during compilation"
echo ""

