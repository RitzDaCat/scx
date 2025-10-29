#!/bin/bash
# Quick performance check script for scx_gamer while gaming

echo "=== scx_gamer Performance Check ==="
echo ""

# Check if scheduler is running
echo "1. Scheduler Status:"
if systemctl is-active --quiet scx.service; then
    CURRENT_SCHED=$(grep "^SCX_SCHEDULER=" /etc/default/scx 2>/dev/null | cut -d= -f2 || echo "unknown")
    if [ "$CURRENT_SCHED" = "scx_gamer" ]; then
        echo "   [OK] scx_gamer is running"
    else
        echo "   [WARN] scx_gamer not active (current: $CURRENT_SCHED)"
    fi
else
    echo "   [ERROR] scx.service is not running"
    exit 1
fi
echo ""

# Show thread classification stats
echo "2. Thread Classification (should show active threads):"
scxstats -s scx_gamer 2>/dev/null | grep "threads" || echo "   [WARN] Unable to get stats"
echo ""

# Check game detection
echo "3. Game Detection (last 5 minutes):"
GAME_DETECTED=$(journalctl -u scx.service --since "5 minutes ago" 2>/dev/null | grep -i "game detected" | tail -1)
if [ -n "$GAME_DETECTED" ]; then
    echo "   [OK] Game detected:"
    echo "   $GAME_DETECTED"
else
    echo "   [INFO] No recent game detection entries"
fi
echo ""

# Check for errors
echo "4. Recent Errors/Warnings:"
ERRORS=$(journalctl -u scx.service --since "10 minutes ago" 2>/dev/null | grep -iE "error|warn|fail" | tail -5)
if [ -n "$ERRORS" ]; then
    echo "   [WARN] Found issues:"
    echo "$ERRORS" | sed 's/^/   /'
else
    echo "   [OK] No errors found"
fi
echo ""

# Performance indicators
echo "5. Performance Indicators:"
echo "   Key things to watch:"
echo "   - input= threads (should be >0 during gameplay)"
echo "   - gpu= threads (should be >0 with GPU activity)"
echo "   - network= threads (should be >0 in multiplayer)"
echo "   - compositor= threads (should be >0)"
echo ""
echo "6. Real-time monitoring:"
echo "   Run in separate terminal: watch -n 1 'scxstats -s scx_gamer'"
echo ""

