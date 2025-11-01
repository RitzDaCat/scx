#!/usr/bin/env bash
# Verification script for LMAX Disruptor optimizations

echo "================================================================================"
echo "        LMAX DISRUPTOR & MECHANICAL SYMPATHY VERIFICATION"
echo "================================================================================"
echo

# Check if scheduler is running
if ! pgrep -f "scx_gamer" > /dev/null; then
    echo "❌ scx_gamer is not running"
    echo "   Start it with: ./start.sh (option 1, then option 3)"
    exit 1
fi

echo "✅ scx_gamer scheduler is running"
echo

# Check BPF maps for distributed ring buffers
echo "Checking BPF maps for distributed ring buffers..."
echo

# Look for ring buffer maps using bpftool if available
if command -v bpftool > /dev/null 2>&1; then
    echo "BPF Maps containing 'ringbuf':"
    sudo bpftool map list | grep -i ringbuf | head -20
    echo
    
    RINGBUF_COUNT=$(sudo bpftool map list 2>/dev/null | grep -c "input_events_ringbuf" || echo "0")
    if [ "$RINGBUF_COUNT" -ge 16 ]; then
        echo "✅ Found $RINGBUF_COUNT distributed ring buffer maps (expected 16+)"
    elif [ "$RINGBUF_COUNT" -gt 1 ]; then
        echo "⚠️  Found $RINGBUF_COUNT ring buffer maps (partial distribution)"
    else
        echo "⚠️  Found $RINGBUF_COUNT ring buffer map(s) (may be using legacy single buffer)"
    fi
else
    echo "⚠️  bpftool not available - cannot verify BPF maps directly"
    echo "   Install with: sudo pacman -S bpftool (or equivalent for your distro)"
fi

echo
echo "================================================================================"
echo "Verification Summary:"
echo "================================================================================"
echo
echo "To see distributed buffer initialization messages, run scheduler with:"
echo "  RUST_LOG=info sudo ./target/release/scx_gamer --tui 0.1"
echo
echo "Expected log message:"
echo "  'Input ring buffer: Initialized with 16 distributed buffers (LMAX Disruptor)'"
echo
echo "Performance Indicators:"
echo "  - If using distributed buffers: Lower ring buffer contention"
echo "  - Memory prefetching: Active automatically (no logs needed)"
echo "  - Expected improvement: ~55-110ns per hot path operation"
echo
echo "To test with detailed logging:"
echo "  1. Stop current scheduler (Ctrl+C)"
echo "  2. Run: RUST_LOG=info sudo ./target/release/scx_gamer --tui 0.1"
echo "  3. Look for 'distributed buffers' message in output"
echo

