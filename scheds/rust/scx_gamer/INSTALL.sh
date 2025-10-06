#!/bin/bash
# scx_gamer - CachyOS sched-ext Integration Installer
# Installs scx_gamer into CachyOS's scheduler infrastructure

set -e  # Exit on error

SCHEDULER_NAME="scx_gamer"

# Detect SCX repository root (script is in scheds/rust/scx_gamer/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCX_REPO="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BINARY_PATH="${SCX_REPO}/target/release/${SCHEDULER_NAME}"

INSTALL_DIR="/usr/bin"
CONFIG_FILE="/etc/default/scx"
LOADER_CONFIG="/etc/scx_loader.toml"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
NC='\033[0m' # No Color

echo -e "${BLUE}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${BLUE}║       scx_gamer CachyOS Installation Script              ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (use sudo)${NC}"
    exit 1
fi

# Check if binary exists
if [ ! -f "$BINARY_PATH" ]; then
    echo -e "${RED}Error: Binary not found at: ${BINARY_PATH}${NC}"
    echo ""
    echo "Please build first:"
    echo "  cd $SCX_REPO"
    echo "  cargo build --release --package scx_gamer"
    exit 1
fi

# Backup existing config if it exists
if [ -f "$CONFIG_FILE" ]; then
    BACKUP_FILE="${CONFIG_FILE}.backup.$(date +%Y%m%d_%H%M%S)"
    echo -e "${YELLOW}→${NC} Backing up ${CONFIG_FILE} to ${BACKUP_FILE}"
    cp "$CONFIG_FILE" "$BACKUP_FILE"
fi

if [ -f "$LOADER_CONFIG" ]; then
    BACKUP_LOADER="${LOADER_CONFIG}.backup.$(date +%Y%m%d_%H%M%S)"
    echo -e "${YELLOW}→${NC} Backing up ${LOADER_CONFIG} to ${BACKUP_LOADER}"
    cp "$LOADER_CONFIG" "$BACKUP_LOADER"
fi

# Install binary
echo -e "${YELLOW}→${NC} Installing ${SCHEDULER_NAME} to ${INSTALL_DIR}/"
install -m 755 "$BINARY_PATH" "${INSTALL_DIR}/${SCHEDULER_NAME}"

# Update scheduler list in /etc/default/scx
echo -e "${YELLOW}→${NC} Adding ${SCHEDULER_NAME} to scheduler list in ${CONFIG_FILE}"
if grep -q "^# List of scx_schedulers:" "$CONFIG_FILE"; then
    # Add scx_gamer to the list if not already present
    if ! grep "^# List of scx_schedulers:" "$CONFIG_FILE" | grep -q "scx_gamer"; then
        sed -i 's/\(^# List of scx_schedulers:.*\)/\1 scx_gamer/' "$CONFIG_FILE"
    fi
else
    echo "Warning: Could not find scheduler list comment in $CONFIG_FILE"
fi

# Generate complete scx_loader.toml with all schedulers including scx_gamer
echo -e "${YELLOW}→${NC} Generating complete ${LOADER_CONFIG} with ${SCHEDULER_NAME}"

# Create complete configuration file with all schedulers properly configured
cat > "$LOADER_CONFIG" << 'EOF'
default_mode = "Gaming"

[scheds.scx_bpfland]
auto_mode = []
gaming_mode = ["-m", "performance"]
lowlatency_mode = ["-s", "5000", "-S", "500", "-l", "5000", "-m", "performance"]
powersave_mode = ["-m", "powersave"]
server_mode = ["-p"]

[scheds.scx_rusty]
auto_mode = []
gaming_mode = []
lowlatency_mode = []
powersave_mode = []
server_mode = []

[scheds.scx_lavd]
auto_mode = []
gaming_mode = ["--performance"]
lowlatency_mode = ["--performance"]
powersave_mode = ["--powersave"]
server_mode = []

[scheds.scx_flash]
auto_mode = []
gaming_mode = ["-m", "all"]
lowlatency_mode = ["-m", "performance", "-w", "-C", "0"]
powersave_mode = ["-m", "powersave", "-I", "10000", "-t", "10000", "-s", "10000", "-S", "1000"]
server_mode = ["-m", "all", "-s", "20000", "-S", "1000", "-I", "-1", "-D", "-L"]

[scheds.scx_p2dq]
auto_mode = []
gaming_mode = ["--task-slice", "true", "-f", "--sched-mode", "performance"]
lowlatency_mode = ["-y", "-f", "--task-slice", "true"]
powersave_mode = ["--sched-mode", "efficiency"]
server_mode = ["--keep-running"]

[scheds.scx_tickless]
auto_mode = []
gaming_mode = ["-f", "5000", "-s", "5000"]
lowlatency_mode = ["-f", "5000", "-s", "1000"]
powersave_mode = ["-f", "50", "-p"]
server_mode = ["-f", "100"]

[scheds.scx_rustland]
auto_mode = []
gaming_mode = []
lowlatency_mode = []
powersave_mode = []
server_mode = []

[scheds.scx_cosmos]
gaming_mode = ["-m", "all"]

[scheds.scx_gamer]
auto_mode = ["--slice-us", "10", "--mm-affinity", "--wakeup-timer-us", "100", "--stats", "0"]
gaming_mode = ["--slice-us", "10", "--mm-affinity", "--wakeup-timer-us", "100", "--stats", "0"]
lowlatency_mode = ["--slice-us", "5", "--mm-affinity", "--wakeup-timer-us", "50", "--stats", "0"]
powersave_mode = ["--slice-us", "20", "--stats", "0"]
server_mode = ["--slice-us", "15", "--stats", "0"]
EOF
echo -e "${GREEN}✓${NC} Generated complete scx_loader configuration with all schedulers"

# Rebuild scx_loader to include scx_gamer in GUI dropdown
echo ""
echo -e "${YELLOW}→${NC} Rebuilding scx_loader to add scx_gamer to GUI..."
LOADER_BINARY="$SCX_REPO/target/release/scx_loader"

# Verify we're in the scx repo
if [ ! -f "$SCX_REPO/Cargo.toml" ]; then
    echo -e "${RED}Error: SCX repository not found at $SCX_REPO${NC}"
    echo "scx_gamer installed, but GUI integration requires rebuilding scx_loader"
    echo "Run manually: cd <scx_repo> && cargo build --release --package scx_loader"
else
    # Build scx_loader (this includes scx_gamer in the supported scheduler list)
    echo -e "${YELLOW}→${NC} Building scx_loader (this may take 1-2 minutes)..."

    # Run build as the original user (not root) to avoid permission issues
    ORIGINAL_USER=$(who am i | awk '{print $1}')
    if [ -n "$ORIGINAL_USER" ] && [ "$ORIGINAL_USER" != "root" ]; then
        sudo -u "$ORIGINAL_USER" bash -c "cd '$SCX_REPO' && cargo build --release --package scx_loader 2>&1 | grep -E 'Compiling|Finished|error|warning' || true"
    else
        (cd "$SCX_REPO" && cargo build --release --package scx_loader 2>&1 | grep -E "Compiling|Finished|error|warning" || true)
    fi

    # Install updated scx_loader
    if [ -f "$LOADER_BINARY" ]; then
        echo -e "${YELLOW}→${NC} Installing updated scx_loader to /usr/bin/"
        install -m 755 "$LOADER_BINARY" /usr/bin/scx_loader
        echo -e "${GREEN}✓${NC} scx_loader updated with scx_gamer support"

        # Restart scx_loader service if it's running
        if systemctl is-active --quiet scx_loader.service; then
            echo -e "${YELLOW}→${NC} Restarting scx_loader service..."
            systemctl restart scx_loader.service
            echo -e "${GREEN}✓${NC} scx_loader service restarted"
        fi
    else
        echo -e "${RED}Error: Failed to build scx_loader${NC}"
        echo "scx_gamer installed, but may not appear in GUI until scx_loader is rebuilt"
    fi
fi

echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║              Installation Complete!                       ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${BLUE}Next steps:${NC}"
echo ""
echo "1. Open CachyOS sched-ext GUI:"
echo "   ${YELLOW}scx-manager${NC} or search 'Scheduler' in your app menu"
echo ""
echo "2. Select scheduler:"
echo "   - Choose ${GREEN}scx_gamer${NC} from the dropdown (now available!)"
echo ""
echo "3. Choose profile:"
echo "   - ${GREEN}Gaming${NC} - Optimized for 4K 240Hz or 1080p 480Hz gaming"
echo "   - ${GREEN}LowLatency${NC} - Ultra-low latency (esports, competitive)"
echo "   - ${GREEN}PowerSave${NC} - Battery-friendly settings"
echo "   - ${GREEN}Server${NC} - Balanced for background tasks"
echo ""
echo "4. (Optional) Add custom flags via 'Set sched-ext extra scheduler flags':"
echo "   - ${YELLOW}--ml-profiles${NC} - Auto-load per-game configs"
echo "   - ${YELLOW}--ml-collect${NC} - Collect training data"
echo "   - ${YELLOW}--verbose${NC} - Debug logging"
echo ""
echo "5. Apply and start the scheduler from the GUI"
echo ""
echo -e "${BLUE}Manual verification:${NC}"
echo "  ${YELLOW}sudo systemctl status scx.service${NC}"
echo "  ${YELLOW}scxstats -s scx_gamer${NC}"
echo ""
echo -e "${BLUE}To uninstall:${NC}"
echo "  ${YELLOW}sudo ./UNINSTALL.sh${NC}"
echo ""
