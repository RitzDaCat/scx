#!/bin/bash
# scx_gamer - Enhanced CachyOS Installer with SchedEXT GUI Integration
# Installs scx_gamer with full integration into CachyOS's scheduler infrastructure

set -e  # Exit on error

SCHEDULER_NAME="scx_gamer"
VERSION="1.0.2"

# Detect SCX repository root (script is in scheds/rust/scx_gamer/)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCX_REPO="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BINARY_PATH="${SCX_REPO}/target/release/${SCHEDULER_NAME}"

INSTALL_DIR="/usr/bin"
CONFIG_FILE="/etc/default/scx"
LOADER_CONFIG="/etc/scx_loader.toml"
DESKTOP_DIR="/usr/share/applications"
ICON_DIR="/usr/share/icons/hicolor/256x256/apps"

# Colors
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
RED='\033[0;31m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

# ASCII Art Banner
print_banner() {
    echo -e "${BLUE}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${BLUE}║                    scx_gamer CachyOS Installer v${VERSION}                    ║${NC}"
    echo -e "${BLUE}║              Ultra-Low Latency Gaming Scheduler                              ║${NC}"
    echo -e "${BLUE}║                    Experimental Research Project                             ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

# System requirements check
check_requirements() {
    echo -e "${CYAN}🔍 Checking system requirements...${NC}"
    
    # Check if running as root
    if [ "$EUID" -ne 0 ]; then
        echo -e "${RED}❌ Error: This script must be run as root (use sudo)${NC}"
        exit 1
    fi
    
    # Check kernel version (need 6.12+ with sched_ext)
    KERNEL_VERSION=$(uname -r | cut -d. -f1-2)
    KERNEL_MAJOR=$(echo $KERNEL_VERSION | cut -d. -f1)
    KERNEL_MINOR=$(echo $KERNEL_VERSION | cut -d. -f2)
    
    if [ "$KERNEL_MAJOR" -lt 6 ] || ([ "$KERNEL_MAJOR" -eq 6 ] && [ "$KERNEL_MINOR" -lt 12 ]); then
        echo -e "${RED}❌ Error: Kernel $KERNEL_VERSION detected. scx_gamer requires kernel 6.12+ with sched_ext support${NC}"
        echo -e "${YELLOW}💡 Please upgrade to a CachyOS kernel with sched_ext support${NC}"
        exit 1
    fi
    
    # Check if sched_ext is available
    if ! grep -q "CONFIG_SCHED_EXT=y" /boot/config-$(uname -r) 2>/dev/null; then
        echo -e "${YELLOW}⚠️  Warning: sched_ext may not be enabled in kernel config${NC}"
        echo -e "${YELLOW}   Continuing anyway - will verify at runtime${NC}"
    fi
    
    # Check if binary exists
    if [ ! -f "$BINARY_PATH" ]; then
        echo -e "${RED}❌ Error: Binary not found at: ${BINARY_PATH}${NC}"
        echo ""
        echo -e "${YELLOW}💡 Please build first:${NC}"
        echo "  ${CYAN}cd $SCX_REPO${NC}"
        echo "  ${CYAN}cargo build --release --package scx_gamer${NC}"
        exit 1
    fi
    
    # Check if CachyOS scx-scheds is installed
    if ! pacman -Q scx-scheds >/dev/null 2>&1; then
        echo -e "${YELLOW}⚠️  Warning: scx-scheds package not found${NC}"
        echo -e "${YELLOW}   Installing CachyOS scheduler infrastructure...${NC}"
        pacman -S --noconfirm scx-scheds
    fi
    
    echo -e "${GREEN}✅ System requirements check passed${NC}"
    echo ""
}

# Backup existing configuration
backup_config() {
    echo -e "${CYAN}📦 Creating configuration backups...${NC}"
    
    TIMESTAMP=$(date +%Y%m%d_%H%M%S)
    
    if [ -f "$CONFIG_FILE" ]; then
        BACKUP_FILE="${CONFIG_FILE}.backup.${TIMESTAMP}"
        echo -e "${YELLOW}→${NC} Backing up ${CONFIG_FILE} to ${BACKUP_FILE}"
        cp "$CONFIG_FILE" "$BACKUP_FILE"
    fi
    
    if [ -f "$LOADER_CONFIG" ]; then
        BACKUP_LOADER="${LOADER_CONFIG}.backup.${TIMESTAMP}"
        echo -e "${YELLOW}→${NC} Backing up ${LOADER_CONFIG} to ${BACKUP_LOADER}"
        cp "$LOADER_CONFIG" "$BACKUP_LOADER"
    fi
    
    echo -e "${GREEN}✅ Configuration backups created${NC}"
    echo ""
}

# Install binary
install_binary() {
    echo -e "${CYAN}📦 Installing scx_gamer binary...${NC}"
    
    # Install binary with proper permissions
    install -m 755 "$BINARY_PATH" "${INSTALL_DIR}/${SCHEDULER_NAME}"
    
    # Verify installation
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${GREEN}✅ Binary installed successfully${NC}"
        echo -e "${YELLOW}   Location: ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
        echo -e "${YELLOW}   Size: $(du -h "${INSTALL_DIR}/${SCHEDULER_NAME}" | cut -f1)${NC}"
    else
        echo -e "${RED}❌ Failed to install binary${NC}"
        exit 1
    fi
    
    echo ""
}

# Update scheduler configuration
update_scheduler_config() {
    echo -e "${CYAN}⚙️  Updating scheduler configuration...${NC}"
    
    # Add scx_gamer to scheduler list in /etc/default/scx
    if [ -f "$CONFIG_FILE" ]; then
        if grep -q "^# List of scx_schedulers:" "$CONFIG_FILE"; then
            # Add scx_gamer to the list if not already present
            if ! grep "^# List of scx_schedulers:" "$CONFIG_FILE" | grep -q "scx_gamer"; then
                sed -i 's/\(^# List of scx_schedulers:.*\)/\1 scx_gamer/' "$CONFIG_FILE"
                echo -e "${GREEN}✅ Added scx_gamer to scheduler list${NC}"
            else
                echo -e "${YELLOW}⚠️  scx_gamer already in scheduler list${NC}"
            fi
        else
            echo -e "${YELLOW}⚠️  Could not find scheduler list comment in $CONFIG_FILE${NC}"
        fi
    else
        echo -e "${YELLOW}⚠️  Configuration file $CONFIG_FILE not found${NC}"
    fi
    
    echo ""
}

# Generate scx_loader configuration
generate_loader_config() {
    echo -e "${CYAN}⚙️  Generating scx_loader configuration...${NC}"
    
    # Create complete configuration file with all schedulers including scx_gamer
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
lowlatency_mode = ["--slice-us", "5", "--mm-affinity", "--wakeup-timer-us", "50", "--stats", "0", "--busy-polling"]
powersave_mode = ["--slice-us", "20", "--stats", "0"]
server_mode = ["--slice-us", "15", "--stats", "0"]
EOF
    
    echo -e "${GREEN}✅ Generated complete scx_loader configuration${NC}"
    echo ""
}

# Build and install scx_loader
build_scx_loader() {
    echo -e "${CYAN}🔨 Building scx_loader with scx_gamer support...${NC}"
    
    LOADER_BINARY="$SCX_REPO/target/release/scx_loader"
    
    # Verify we're in the scx repo
    if [ ! -f "$SCX_REPO/Cargo.toml" ]; then
        echo -e "${RED}❌ Error: SCX repository not found at $SCX_REPO${NC}"
        echo -e "${YELLOW}💡 scx_gamer installed, but GUI integration requires rebuilding scx_loader${NC}"
        echo -e "${YELLOW}   Run manually: cd <scx_repo> && cargo build --release --package scx_loader${NC}"
        return 1
    fi
    
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
        echo -e "${GREEN}✅ scx_loader updated with scx_gamer support${NC}"
        
        # Restart scx_loader service if it's running
        if systemctl is-active --quiet scx_loader.service; then
            echo -e "${YELLOW}→${NC} Restarting scx_loader service..."
            systemctl restart scx_loader.service
            echo -e "${GREEN}✅ scx_loader service restarted${NC}"
        fi
    else
        echo -e "${RED}❌ Error: Failed to build scx_loader${NC}"
        echo -e "${YELLOW}💡 scx_gamer installed, but may not appear in GUI until scx_loader is rebuilt${NC}"
        return 1
    fi
    
    echo ""
}

# Create desktop entry
create_desktop_entry() {
    echo -e "${CYAN}🖥️  Creating desktop entry...${NC}"
    
    # Create desktop entry for easy access
    cat > "${DESKTOP_DIR}/scx-gamer-manager.desktop" << 'EOF'
[Desktop Entry]
Version=1.0
Type=Application
Name=SchedEXT Gaming Manager
Comment=Manage scx_gamer ultra-low latency gaming scheduler
Exec=scx-manager
Icon=scx-gamer
Categories=System;Settings;
Keywords=scheduler;gaming;performance;latency;
StartupNotify=true
Terminal=false
EOF
    
    # Create simple icon (SVG)
    mkdir -p "$ICON_DIR"
    cat > "${ICON_DIR}/scx-gamer.svg" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<svg width="256" height="256" viewBox="0 0 256 256" xmlns="http://www.w3.org/2000/svg">
  <defs>
    <linearGradient id="grad1" x1="0%" y1="0%" x2="100%" y2="100%">
      <stop offset="0%" style="stop-color:#4A90E2;stop-opacity:1" />
      <stop offset="100%" style="stop-color:#7B68EE;stop-opacity:1" />
    </linearGradient>
  </defs>
  <rect width="256" height="256" rx="32" fill="url(#grad1)"/>
  <text x="128" y="140" font-family="Arial, sans-serif" font-size="48" font-weight="bold" text-anchor="middle" fill="white">G</text>
  <text x="128" y="180" font-family="Arial, sans-serif" font-size="16" text-anchor="middle" fill="white">Gaming</text>
  <text x="128" y="200" font-family="Arial, sans-serif" font-size="12" text-anchor="middle" fill="white">Scheduler</text>
</svg>
EOF
    
    # Update icon cache
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
    fi
    
    echo -e "${GREEN}✅ Desktop entry created${NC}"
    echo ""
}

# Verify installation
verify_installation() {
    echo -e "${CYAN}🔍 Verifying installation...${NC}"
    
    # Check binary
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${GREEN}✅ Binary: ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
    else
        echo -e "${RED}❌ Binary missing${NC}"
    fi
    
    # Check configuration
    if [ -f "$CONFIG_FILE" ] && grep -q "scx_gamer" "$CONFIG_FILE"; then
        echo -e "${GREEN}✅ Scheduler list updated${NC}"
    else
        echo -e "${YELLOW}⚠️  Scheduler list may not be updated${NC}"
    fi
    
    # Check loader config
    if [ -f "$LOADER_CONFIG" ] && grep -q "\[scheds.scx_gamer\]" "$LOADER_CONFIG"; then
        echo -e "${GREEN}✅ Loader configuration updated${NC}"
    else
        echo -e "${YELLOW}⚠️  Loader configuration may not be updated${NC}"
    fi
    
    # Check desktop entry
    if [ -f "${DESKTOP_DIR}/scx-gamer-manager.desktop" ]; then
        echo -e "${GREEN}✅ Desktop entry created${NC}"
    else
        echo -e "${YELLOW}⚠️  Desktop entry may not be created${NC}"
    fi
    
    echo ""
}

# Main installation function
main() {
    print_banner
    
    check_requirements
    backup_config
    install_binary
    update_scheduler_config
    generate_loader_config
    build_scx_loader
    create_desktop_entry
    verify_installation
    
    # Success message
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║                        Installation Complete!                                ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    echo -e "${BLUE}🎮 Next Steps:${NC}"
    echo ""
    echo "1. ${CYAN}Open SchedEXT GUI Manager:${NC}"
    echo "   ${YELLOW}scx-manager${NC} or search 'Scheduler' in your app menu"
    echo "   ${YELLOW}Or use the new 'SchedEXT Gaming Manager' desktop entry${NC}"
    echo ""
    echo "2. ${CYAN}Select scx_gamer:${NC}"
    echo "   - Choose ${GREEN}scx_gamer${NC} from the scheduler dropdown"
    echo ""
    echo "3. ${CYAN}Choose profile:${NC}"
    echo "   - ${GREEN}Gaming${NC} - Optimized for 4K 240Hz or 1080p 480Hz gaming"
    echo "   - ${GREEN}LowLatency${NC} - Ultra-low latency with busy polling (esports)"
    echo "   - ${GREEN}PowerSave${NC} - Battery-friendly settings"
    echo "   - ${GREEN}Server${NC} - Balanced for background tasks"
    echo ""
    echo "4. ${CYAN}(Optional) Add custom flags:${NC}"
    echo "   - ${YELLOW}--ml-profiles${NC} - Auto-load per-game configs"
    echo "   - ${YELLOW}--ml-collect${NC} - Collect training data"
    echo "   - ${YELLOW}--verbose${NC} - Debug logging"
    echo "   - ${YELLOW}--busy-polling${NC} - Ultra-low latency (consumes 100% CPU)"
    echo ""
    echo "5. ${CYAN}Apply and start the scheduler from the GUI${NC}"
    echo ""
    
    echo -e "${BLUE}🔧 Manual Commands:${NC}"
    echo "  ${YELLOW}sudo systemctl status scx.service${NC}  # Check scheduler status"
    echo "  ${YELLOW}scxstats -s scx_gamer${NC}             # View scheduler statistics"
    echo "  ${YELLOW}sudo ./uninstall-cachyos.sh${NC}       # Uninstall scx_gamer"
    echo ""
    
    echo -e "${BLUE}📚 Documentation:${NC}"
    echo "  ${YELLOW}README.md${NC}                          # Project overview and features"
    echo "  ${YELLOW}docs/QUICK_START.md${NC}                # Quick start guide"
    echo "  ${YELLOW}docs/TECHNICAL_ARCHITECTURE.md${NC}    # Technical details"
    echo ""
    
    echo -e "${PURPLE}⚠️  Experimental Research Project${NC}"
    echo "This scheduler is an experimental research project developed with AI assistance."
    echo "Results may vary based on hardware configuration and game engine."
    echo ""
    
    echo -e "${GREEN}🎉 Enjoy ultra-low latency gaming with scx_gamer!${NC}"
}

# Run main function
main "$@"
