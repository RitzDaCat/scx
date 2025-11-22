#!/bin/bash
# Complete cache line alignment coverage using pahole on compiled BPF code

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
REPO_ROOT="$(cd "$PROJECT_ROOT/../../.." && pwd)"
BPF_OBJ="$REPO_ROOT/target/release/build/scx_gamer-dc757ee635422b33/out/bpf.bpf.o"

# Try to find the BPF object if path changed
if [ ! -f "$BPF_OBJ" ]; then
    BPF_OBJ=$(find "$REPO_ROOT/target" -name "bpf.bpf.o" -type f 2>/dev/null | head -1)
fi

if [ ! -f "$BPF_OBJ" ]; then
    echo "Error: BPF object not found. Please run ./build.sh first."
    exit 1
fi

echo "================================================================================"
echo "COMPLETE CACHE LINE ALIGNMENT COVERAGE REPORT (pahole)"
echo "================================================================================"
echo ""
echo "Cache line size: 64 bytes"
echo "BPF object: $BPF_OBJ"
echo ""

# Priority structs
echo "### PRIORITY STRUCTS (Hot Path) ###"
echo ""
for struct in task_ctx cpu_ctx; do
    if pahole -C "$struct" "$BPF_OBJ" &>/dev/null; then
        size=$(pahole -C "$struct" "$BPF_OBJ" 2>/dev/null | grep -oP 'size: \K\d+')
        cachelines=$(pahole -C "$struct" "$BPF_OBJ" 2>/dev/null | grep -oP 'cachelines: \K\d+')
        members=$(pahole -C "$struct" "$BPF_OBJ" 2>/dev/null | grep -oP 'members: \K\d+')
        
        remainder=$((size % 64))
        if [ $remainder -eq 0 ]; then
            align_status="✓ ALIGNED"
        else
            align_status="✗ NOT ALIGNED (needs +$((64 - remainder)) bytes)"
        fi
        
        printf "%-30s %5d bytes  %2d cache lines  %3d members  %s\n" \
               "$struct" "$size" "$cachelines" "$members" "$align_status"
    fi
done

echo ""
echo "### ALL OTHER STRUCTS ###"
echo ""

# All structs sorted by size
pahole "$BPF_OBJ" 2>/dev/null | grep -E "^struct " | awk '{print $2}' | sort -u | while read struct_name; do
    # Skip priority structs (already shown)
    if [ "$struct_name" = "task_ctx" ] || [ "$struct_name" = "cpu_ctx" ]; then
        continue
    fi
    
    size=$(pahole -C "$struct_name" "$BPF_OBJ" 2>/dev/null | grep -oP 'size: \K\d+')
    
    if [ -n "$size" ] && [ "$size" -gt 0 ]; then
        cachelines=$(pahole -C "$struct_name" "$BPF_OBJ" 2>/dev/null | grep -oP 'cachelines: \K\d+' || echo "0")
        remainder=$((size % 64))
        
        if [ $remainder -eq 0 ]; then
            align_mark="✓"
        else
            align_mark="✗"
        fi
        
        printf "%-40s %5d bytes  %2d lines  [%s]\n" "$struct_name" "$size" "$cachelines" "$align_mark"
    fi
done | sort -k2 -n

echo ""
echo "================================================================================"
echo "SUMMARY"
echo "================================================================================"
echo ""
echo "✓ = Aligned to 64-byte boundary (no straddling risk)"
echo "✗ = Not aligned (may straddle cache lines)"
echo ""
echo "Note: Only CACHE_ALIGNED structs (task_ctx, cpu_ctx) require 64-byte alignment."
echo "Small structs (<64 bytes) don't need alignment unless accessed concurrently."
echo ""

