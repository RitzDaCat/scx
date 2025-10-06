#!/bin/bash
# scx_gamer - CachyOS sched-ext Uninstaller
# Removes scx_gamer from CachyOS's scheduler infrastructure

set -e  # Exit on error

SCHEDULER_NAME="scx_gamer"
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
echo -e "${BLUE}║       scx_gamer CachyOS Uninstall Script                 ║${NC}"
echo -e "${BLUE}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then
    echo -e "${RED}Error: This script must be run as root (use sudo)${NC}"
    exit 1
fi

# Check if scx_gamer is currently active
if systemctl is-active --quiet scx.service; then
    CURRENT_SCHED=$(grep "^SCX_SCHEDULER=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "")
    if [ "$CURRENT_SCHED" = "scx_gamer" ]; then
        echo -e "${YELLOW}⚠ Warning: scx_gamer is currently active!${NC}"
        echo ""
        read -p "Do you want to stop the scheduler before uninstalling? [Y/n] " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]] || [[ -z $REPLY ]]; then
            echo -e "${YELLOW}→${NC} Stopping scx.service..."
            systemctl stop scx.service
            echo -e "${GREEN}✓${NC} Scheduler stopped"
        else
            echo -e "${RED}Error: Please stop scx_gamer scheduler before uninstalling${NC}"
            echo "Run: ${YELLOW}sudo systemctl stop scx.service${NC}"
            exit 1
        fi
    fi
fi

# Remove binary
if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
    echo -e "${YELLOW}→${NC} Removing ${SCHEDULER_NAME} from ${INSTALL_DIR}/"
    rm -f "${INSTALL_DIR}/${SCHEDULER_NAME}"
    echo -e "${GREEN}✓${NC} Binary removed"
else
    echo -e "${YELLOW}⚠${NC} Binary not found at ${INSTALL_DIR}/${SCHEDULER_NAME}"
fi

# Remove from scheduler list in /etc/default/scx
if [ -f "$CONFIG_FILE" ]; then
    echo -e "${YELLOW}→${NC} Removing ${SCHEDULER_NAME} from scheduler list in ${CONFIG_FILE}"

    # Backup before modifying
    BACKUP_FILE="${CONFIG_FILE}.uninstall_backup.$(date +%Y%m%d_%H%M%S)"
    cp "$CONFIG_FILE" "$BACKUP_FILE"

    # Remove scx_gamer from the scheduler list
    sed -i 's/ scx_gamer//g' "$CONFIG_FILE"

    # If SCX_SCHEDULER was set to scx_gamer, change to scx_rusty (safe default)
    if grep -q "^SCX_SCHEDULER=scx_gamer" "$CONFIG_FILE"; then
        echo -e "${YELLOW}→${NC} Switching default scheduler to scx_rusty"
        sed -i 's/^SCX_SCHEDULER=scx_gamer/SCX_SCHEDULER=scx_rusty/' "$CONFIG_FILE"
    fi

    echo -e "${GREEN}✓${NC} Removed from config (backup: ${BACKUP_FILE})"
fi

# Reinstall stable scx_loader from pacman to restore defaults
echo ""
echo -e "${YELLOW}→${NC} Reinstalling stable scx_loader from CachyOS repository..."
echo -e "${BLUE}This will restore default configuration and scx_loader binary${NC}"

# Stop scx_loader service before reinstalling
if systemctl is-active --quiet scx_loader.service; then
    echo -e "${YELLOW}→${NC} Stopping scx_loader service..."
    systemctl stop scx_loader.service
fi

# Reinstall scx_loader package (this restores /etc/scx_loader.toml and /usr/bin/scx_loader)
echo -e "${YELLOW}→${NC} Running: pacman -S --noconfirm scx-scheds"
pacman -S --noconfirm scx-scheds

if [ $? -eq 0 ]; then
    echo -e "${GREEN}✓${NC} scx_loader reinstalled from stable CachyOS repository"

    # Restart scx_loader service
    echo -e "${YELLOW}→${NC} Restarting scx_loader service..."
    systemctl restart scx_loader.service
    echo -e "${GREEN}✓${NC} scx_loader service restarted"
else
    echo -e "${RED}✗${NC} Failed to reinstall scx_loader from pacman"
    echo -e "${YELLOW}→${NC} You may need to manually reinstall: sudo pacman -S scx-scheds"
fi

echo ""
echo -e "${GREEN}╔═══════════════════════════════════════════════════════════╗${NC}"
echo -e "${GREEN}║              Uninstallation Complete!                    ║${NC}"
echo -e "${GREEN}╚═══════════════════════════════════════════════════════════╝${NC}"
echo ""
echo -e "${BLUE}Summary:${NC}"
echo "  • scx_gamer binary removed from ${INSTALL_DIR}/"
echo "  • Config backups created with timestamps"
echo "  • scx_loader reinstalled from stable CachyOS repository"
echo "  • Default configuration restored"
echo ""
echo -e "${BLUE}Your system is now back to the stable CachyOS scx configuration${NC}"
echo ""
echo -e "${YELLOW}Note:${NC} You can select any scheduler via CachyOS kernel manager GUI"
echo ""
