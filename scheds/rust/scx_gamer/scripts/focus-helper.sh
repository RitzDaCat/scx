#!/usr/bin/env bash
# Focus detection helper - TRUE EVENT-BASED via KWin scripting
#
# ARCHITECTURE:
# 1. Loads a KWin script that listens to windowActivated signal
# 2. KWin script outputs PID to console.log on each focus change
# 3. We monitor KWin's journal logs for these messages
# 4. Write PID to file for scheduler to read
#
# 100% PROOF, TRUE EVENT-BASED:
# - KWin script uses windowActivated signal (not polling)
# - KWin provides PID directly (compositor/kernel authority)
# - No heuristics, no guessing

set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PID_FILE="/tmp/scx_gamer_focused_pid"
LOG_FILE="/tmp/scx_gamer_focus.log"
KWIN_SCRIPT="$SCRIPT_DIR/kwin-focus.js"
SCRIPT_NAME="scx_gamer_focus"

log() {
    echo "[$(date '+%H:%M:%S')] $*" >> "$LOG_FILE"
}

# Cleanup on exit
cleanup() {
    log "Focus helper stopping"
    # Unload KWin script
    qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "$SCRIPT_NAME" 2>/dev/null || true
    rm -f "$PID_FILE"
    exit 0
}
trap cleanup EXIT INT TERM

# Truncate log file
echo "" > "$LOG_FILE"
log "Focus helper starting (EVENT-BASED via KWin scripting)"

# Initialize PID file
echo 0 > "$PID_FILE"

# Check if KWin script exists
if [[ ! -f "$KWIN_SCRIPT" ]]; then
    log "ERROR: KWin script not found: $KWIN_SCRIPT"
    exit 1
fi

# Unload any previous instance
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.unloadScript "$SCRIPT_NAME" 2>/dev/null || true

# Load KWin script
log "Loading KWin script: $KWIN_SCRIPT"
SCRIPT_ID=$(qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.loadScript "$KWIN_SCRIPT" "$SCRIPT_NAME" 2>&1)

# FIX: KWin's loadScript sometimes returns "0" on success or an integer ID.
# Error messages usually contain "Error" text.
# We treat "0" as a valid ID (some KWin versions use it for the first script).
if [[ "$SCRIPT_ID" == *"Error"* ]]; then
    log "ERROR: Failed to load KWin script: $SCRIPT_ID"
    exit 1
fi
log "KWin script loaded (ID: $SCRIPT_ID)"

# Start the script
qdbus6 org.kde.KWin /Scripting org.kde.kwin.Scripting.start 2>/dev/null || true

# Give script time to initialize
sleep 0.5

# Monitor KWin journal for focus events (TRUE EVENT-BASED)
# journalctl --follow blocks until new messages arrive - no polling!
log "Monitoring KWin journal for focus events..."

journalctl --user -u plasma-kwin_wayland --follow --no-pager -o cat 2>/dev/null | \
while IFS= read -r line; do
    if [[ "$line" == *"SCX_GAMER_FOCUS_PID:"* ]]; then
        # Extract PID from log line
        pid="${line##*SCX_GAMER_FOCUS_PID:}"
        pid="${pid%%[^0-9]*}"  # Remove any trailing non-digits
        
        if [[ -n "$pid" ]]; then
            echo "$pid" > "$PID_FILE"
            log "Focus event: PID=$pid"
        fi
    fi
done

log "Journal monitoring ended"
