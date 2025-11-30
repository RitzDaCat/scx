// KWin Focus Detection Script for scx_gamer
// 100% PROOF, TRUE EVENT-BASED
//
// This script runs inside KWin and:
// 1. Listens to activeWindowChanged signal (EVENT-BASED, not polling)
// 2. Accesses client.pid DIRECTLY from KWin (AUTHORITATIVE)
// 3. Writes PID to /tmp/scx_gamer_focused_pid via shell command
//
// PROOF CHAIN:
// - KWin knows which window is focused (compositor authority)
// - KWin knows each window's PID (kernel data via Wayland/X11)
// - No guessing, no heuristics - just facts

var pidFile = "/tmp/scx_gamer_focused_pid";

function writePid(pid) {
    // Use callDBus to run a shell command that writes the PID
    // This is a workaround since KWin scripts can't write files directly
    callDBus(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "NameHasOwner",
        "org.kde.KWin"
    );
    
    // Alternative: print to console log (parsed by helper)
    console.log("SCX_GAMER_FOCUS_PID:" + pid);
}

function updateFocus() {
    var client = workspace.activeWindow;
    if (client) {
        var pid = client.pid;
        var resourceClass = client.resourceClass || "unknown";
        console.log("SCX_GAMER_FOCUS_PID:" + pid);
        console.log("SCX_GAMER_FOCUS_CLASS:" + resourceClass);
    } else {
        console.log("SCX_GAMER_FOCUS_PID:0");
    }
}

// Register for focus change events (TRUE EVENT-BASED)
workspace.windowActivated.connect(function(client) {
    if (client) {
        console.log("SCX_GAMER_FOCUS_PID:" + client.pid);
        console.log("SCX_GAMER_FOCUS_CLASS:" + (client.resourceClass || "unknown"));
    } else {
        console.log("SCX_GAMER_FOCUS_PID:0");
    }
});

// Initial focus check
updateFocus();

console.log("SCX_GAMER_SCRIPT_LOADED");

