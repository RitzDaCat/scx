#!/bin/bash
# scx_gamer - Enhanced CachyOS Uninstaller
# Removes scx_gamer with full cleanup of CachyOS integration

set -e  # Exit on error

SCHEDULER_NAME="scx_gamer"
VERSION="1.0.2"

INSTALL_DIR="/usr/bin"
CONFIG_FILE="/etc/default/scx"
LOADER_CONFIG="/etc/scx_loader.toml"
DESKTOP_DIR="/usr/share/applications"
ICON_DIR="/usr/share/icons/hicolor/256x256/apps"
DOC_DIR="/usr/share/doc/scx-gamer"

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
    echo -e "${BLUE}║                  scx_gamer CachyOS Uninstaller v${VERSION}                  ║${NC}"
    echo -e "${BLUE}║              Ultra-Low Latency Gaming Scheduler                              ║${NC}"
    echo -e "${BLUE}║                    Experimental Research Project                             ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

# Check if running as root
check_root() {
    if [ "$EUID" -ne 0 ]; then
        echo -e "${RED}❌ Error: This script must be run as root (use sudo)${NC}"
        exit 1
    fi
}

# Check if scx_gamer is currently active
check_active_scheduler() {
    echo -e "${CYAN}🔍 Checking if scx_gamer is currently active...${NC}"
    
    if systemctl is-active --quiet scx.service; then
        CURRENT_SCHED=$(grep "^SCX_SCHEDULER=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "")
        if [ "$CURRENT_SCHED" = "scx_gamer" ]; then
            echo -e "${YELLOW}⚠️  Warning: scx_gamer is currently active!${NC}"
            echo ""
            read -p "Do you want to stop the scheduler before uninstalling? [Y/n] " -n 1 -r
            echo
            if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
                echo -e "${YELLOW}→${NC} Stopping scx.service..."
                systemctl stop scx.service
                echo -e "${GREEN}✅ Scheduler stopped${NC}"
            else
                echo -e "${RED}❌ Error: Please stop scx_gamer scheduler before uninstalling${NC}"
                echo -e "${YELLOW}💡 Run: ${CYAN}sudo systemctl stop scx.service${NC}"
                exit 1
            fi
        else
            echo -e "${GREEN}✅ scx_gamer is not currently active${NC}"
        fi
    else
        echo -e "${GREEN}✅ scx.service is not running${NC}"
    fi
    
    echo ""
}

# Remove binary
remove_binary() {
    echo -e "${CYAN}📦 Removing scx_gamer binary...${NC}"
    
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${YELLOW}→${NC} Removing ${SCHEDULER_NAME} from ${INSTALL_DIR}/"
        rm -f "${INSTALL_DIR}/${SCHEDULER_NAME}"
        echo -e "${GREEN}✅ Binary removed${NC}"
    else
        echo -e "${YELLOW}⚠️  Binary not found at ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
    fi
    
    echo ""
}

# Remove from scheduler configuration
remove_scheduler_config() {
    echo -e "${CYAN}⚙️  Removing scx_gamer from scheduler configuration...${NC}"
    
    if [ -f "$CONFIG_FILE" ]; then
        echo -e "${YELLOW}→${NC} Removing ${SCHEDULER_NAME} from scheduler list in ${CONFIG_FILE}"
        
        # Backup before modifying
        TIMESTAMP=$(date +%Y%m%d_%H%M%S)
        BACKUP_FILE="${CONFIG_FILE}.uninstall_backup.${TIMESTAMP}"
        cp "$CONFIG_FILE" "$BACKUP_FILE"
        echo -e "${YELLOW}   Backup created: ${BACKUP_FILE}${NC}"
        
        # Remove scx_gamer from the scheduler list
        sed -i 's/ scx_gamer//g' "$CONFIG_FILE"
        
        # If SCX_SCHEDULER was set to scx_gamer, change to scx_rusty (safe default)
        if grep -q "^SCX_SCHEDULER=scx_gamer" "$CONFIG_FILE"; then
            echo -e "${YELLOW}→${NC} Switching default scheduler to scx_rusty"
            sed -i 's/^SCX_SCHEDULER=scx_gamer/SCX_SCHEDULER=scx_rusty/' "$CONFIG_FILE"
        fi
        
        echo -e "${GREEN}✅ Removed from scheduler configuration${NC}"
    else
        echo -e "${YELLOW}⚠️  Configuration file $CONFIG_FILE not found${NC}"
    fi
    
    echo ""
}

# Remove from loader configuration
remove_loader_config() {
    echo -e "${CYAN}⚙️  Removing scx_gamer from loader configuration...${NC}"
    
    if [ -f "$LOADER_CONFIG" ]; then
        echo -e "${YELLOW}→${NC} Removing scx_gamer section from ${LOADER_CONFIG}"
        
        # Backup before modifying
        TIMESTAMP=$(date +%Y%m%d_%H%M%S)
        BACKUP_LOADER="${LOADER_CONFIG}.uninstall_backup.${TIMESTAMP}"
        cp "$LOADER_CONFIG" "$BACKUP_LOADER"
        echo -e "${YELLOW}   Backup created: ${BACKUP_LOADER}${NC}"
        
        # Remove scx_gamer section
        sed -i '/\[scheds\.scx_gamer\]/,/^$/d' "$LOADER_CONFIG"
        
        echo -e "${GREEN}✅ Removed from loader configuration${NC}"
    else
        echo -e "${YELLOW}⚠️  Loader configuration file $LOADER_CONFIG not found${NC}"
    fi
    
    echo ""
}

# Remove desktop entry and icon
remove_desktop_files() {
    echo -e "${CYAN}🖥️  Removing desktop entry and icon...${NC}"
    
    # Remove desktop entry
    if [ -f "${DESKTOP_DIR}/scx-gamer-manager.desktop" ]; then
        echo -e "${YELLOW}→${NC} Removing desktop entry"
        rm -f "${DESKTOP_DIR}/scx-gamer-manager.desktop"
        echo -e "${GREEN}✅ Desktop entry removed${NC}"
    else
        echo -e "${YELLOW}⚠️  Desktop entry not found${NC}"
    fi
    
    # Remove icon
    if [ -f "${ICON_DIR}/scx-gamer.svg" ]; then
        echo -e "${YELLOW}→${NC} Removing icon"
        rm -f "${ICON_DIR}/scx-gamer.svg"
        echo -e "${GREEN}✅ Icon removed${NC}"
    else
        echo -e "${YELLOW}⚠️  Icon not found${NC}"
    fi
    
    # Update icon cache
    if command -v gtk-update-icon-cache >/dev/null 2>&1; then
        echo -e "${YELLOW}→${NC} Updating icon cache"
        gtk-update-icon-cache -f -t /usr/share/icons/hicolor >/dev/null 2>&1 || true
        echo -e "${GREEN}✅ Icon cache updated${NC}"
    fi
    
    echo ""
}

# Restore stable scx_loader
restore_scx_loader() {
    echo -e "${CYAN}🔄 Restoring stable scx_loader from CachyOS repository...${NC}"
    
    # Stop scx_loader service before reinstalling
    if systemctl is-active --quiet scx_loader.service; then
        echo -e "${YELLOW}→${NC} Stopping scx_loader service..."
        systemctl stop scx_loader.service
    fi
    
    # Reinstall scx_loader package (this restores /etc/scx_loader.toml and /usr/bin/scx_loader)
    echo -e "${YELLOW}→${NC} Reinstalling scx-scheds package..."
    if pacman -S --noconfirm scx-scheds; then
        echo -e "${GREEN}✅ scx_loader reinstalled from stable CachyOS repository${NC}"
        
        # Restart scx_loader service
        echo -e "${YELLOW}→${NC} Restarting scx_loader service..."
        systemctl restart scx_loader.service
        echo -e "${GREEN}✅ scx_loader service restarted${NC}"
    else
        echo -e "${RED}❌ Failed to reinstall scx_loader from pacman${NC}"
        echo -e "${YELLOW}💡 You may need to manually reinstall: ${CYAN}sudo pacman -S scx-scheds${NC}"
    fi
    
    echo ""
}

# Clean up any remaining files
cleanup_remaining() {
    echo -e "${CYAN}🧹 Cleaning up remaining files...${NC}"
    
    # Remove any ML data directories (if they exist)
    if [ -d "$HOME/.scx_gamer" ]; then
        echo -e "${YELLOW}→${NC} Found ML data directory: $HOME/.scx_gamer"
        read -p "Do you want to remove ML training data? [y/N] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            rm -rf "$HOME/.scx_gamer"
            echo -e "${GREEN}✅ ML training data removed${NC}"
        else
            echo -e "${YELLOW}⚠️  ML training data preserved${NC}"
        fi
    fi
    
    # Remove any log files
    if [ -f "/var/log/scx_gamer.log" ]; then
        echo -e "${YELLOW}→${NC} Removing log file"
        rm -f "/var/log/scx_gamer.log"
        echo -e "${GREEN}✅ Log file removed${NC}"
    fi
    
    echo ""
}

# Verify uninstallation
verify_uninstallation() {
    echo -e "${CYAN}🔍 Verifying uninstallation...${NC}"
    
    # Check binary
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${RED}❌ Binary still exists: ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
    else
        echo -e "${GREEN}✅ Binary removed${NC}"
    fi
    
    # Check configuration
    if [ -f "$CONFIG_FILE" ] && grep -q "scx_gamer" "$CONFIG_FILE"; then
        echo -e "${YELLOW}⚠️  scx_gamer still referenced in scheduler configuration${NC}"
    else
        echo -e "${GREEN}✅ Scheduler configuration cleaned${NC}"
    fi
    
    # Check loader config
    if [ -f "$LOADER_CONFIG" ] && grep -q "\[scheds\.scx_gamer\]" "$LOADER_CONFIG"; then
        echo -e "${YELLOW}⚠️  scx_gamer still referenced in loader configuration${NC}"
    else
        echo -e "${GREEN}✅ Loader configuration cleaned${NC}"
    fi
    
    # Check desktop entry
    if [ -f "${DESKTOP_DIR}/scx-gamer-manager.desktop" ]; then
        echo -e "${RED}❌ Desktop entry still exists${NC}"
    else
        echo -e "${GREEN}✅ Desktop entry removed${NC}"
    fi
    
    echo ""
}

# Main uninstallation function
main() {
    print_banner
    
    check_root
    check_active_scheduler
    remove_binary
    remove_scheduler_config
    remove_loader_config
    remove_desktop_files
    restore_scx_loader
    cleanup_remaining
    verify_uninstallation
    
    # Success message
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║                      Uninstallation Complete!                                ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    echo -e "${BLUE}📋 Summary:${NC}"
    echo "  • scx_gamer binary removed from ${INSTALL_DIR}/"
    echo "  • Configuration backups created with timestamps"
    echo "  • scx_loader reinstalled from stable CachyOS repository"
    echo "  • Desktop entry and icon removed"
    echo "  • Default configuration restored"
    echo ""
    
    echo -e "${BLUE}🎮 Your system is now back to the stable CachyOS scx configuration${NC}"
    echo ""
    
    echo -e "${BLUE}💡 Next steps:${NC}"
    echo "  • You can select any scheduler via CachyOS kernel manager GUI"
    echo "  • Run ${CYAN}scx-manager${NC} to open the scheduler GUI"
    echo "  • Choose from available schedulers: scx_rusty, scx_bpfland, etc."
    echo ""
    
    echo -e "${PURPLE}⚠️  Note:${NC}"
    echo "  • Configuration backups are preserved with timestamps"
    echo "  • ML training data (if any) may still exist in ~/.scx_gamer/"
    echo "  • You can reinstall scx_gamer anytime using the installer script"
    echo ""
    
    echo -e "${GREEN}🎉 scx_gamer has been successfully removed from your system!${NC}"
}

# Run main function
main "$@"
