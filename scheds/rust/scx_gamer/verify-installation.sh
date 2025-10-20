#!/bin/bash
# scx_gamer - Installation Verification Script
# Verifies scx_gamer installation and CachyOS integration

set -e  # Exit on error

SCHEDULER_NAME="scx_gamer"
VERSION="1.0.2"

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
    echo -e "${BLUE}║                scx_gamer Installation Verification v${VERSION}               ║${NC}"
    echo -e "${BLUE}║              Ultra-Low Latency Gaming Scheduler                              ║${NC}"
    echo -e "${BLUE}║                    Experimental Research Project                             ║${NC}"
    echo -e "${BLUE}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
}

# Check system requirements
check_system_requirements() {
    echo -e "${CYAN}🔍 Checking system requirements...${NC}"
    
    # Check kernel version
    KERNEL_VERSION=$(uname -r)
    KERNEL_MAJOR=$(echo $KERNEL_VERSION | cut -d. -f1)
    KERNEL_MINOR=$(echo $KERNEL_VERSION | cut -d. -f2)
    
    echo -e "${YELLOW}   Kernel version: ${KERNEL_VERSION}${NC}"
    
    if [ "$KERNEL_MAJOR" -lt 6 ] || ([ "$KERNEL_MAJOR" -eq 6 ] && [ "$KERNEL_MINOR" -lt 12 ]); then
        echo -e "${RED}❌ Kernel $KERNEL_VERSION detected. scx_gamer requires kernel 6.12+ with sched_ext support${NC}"
        return 1
    else
        echo -e "${GREEN}✅ Kernel version compatible${NC}"
    fi
    
    # Check sched_ext support
    if [ -f "/boot/config-$KERNEL_VERSION" ] && grep -q "CONFIG_SCHED_EXT=y" "/boot/config-$KERNEL_VERSION"; then
        echo -e "${GREEN}✅ sched_ext support detected${NC}"
    else
        echo -e "${YELLOW}⚠️  sched_ext support not confirmed in kernel config${NC}"
    fi
    
    # Check CachyOS packages
    if pacman -Q scx-scheds >/dev/null 2>&1; then
        echo -e "${GREEN}✅ scx-scheds package installed${NC}"
    else
        echo -e "${YELLOW}⚠️  scx-scheds package not found${NC}"
    fi
    
    if pacman -Q scx-manager >/dev/null 2>&1; then
        echo -e "${GREEN}✅ scx-manager package installed${NC}"
    else
        echo -e "${YELLOW}⚠️  scx-manager package not found${NC}"
    fi
    
    echo ""
}

# Check binary installation
check_binary() {
    echo -e "${CYAN}📦 Checking binary installation...${NC}"
    
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${GREEN}✅ Binary: ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
        
        # Get binary info
        BINARY_SIZE=$(du -h "${INSTALL_DIR}/${SCHEDULER_NAME}" | cut -f1)
        echo -e "${YELLOW}   Size: ${BINARY_SIZE}${NC}"
        
        # Check binary version
        if BINARY_VERSION=$("${INSTALL_DIR}/${SCHEDULER_NAME}" --version 2>&1 | head -1); then
            echo -e "${YELLOW}   Version: ${BINARY_VERSION}${NC}"
        else
            echo -e "${YELLOW}   Version: Unable to determine${NC}"
        fi
        
        # Check binary permissions
        BINARY_PERMS=$(ls -l "${INSTALL_DIR}/${SCHEDULER_NAME}" | cut -d' ' -f1)
        echo -e "${YELLOW}   Permissions: ${BINARY_PERMS}${NC}"
        
        # Check if binary is executable
        if [ -x "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
            echo -e "${GREEN}✅ Binary is executable${NC}"
        else
            echo -e "${RED}❌ Binary is not executable${NC}"
        fi
    else
        echo -e "${RED}❌ Binary missing: ${INSTALL_DIR}/${SCHEDULER_NAME}${NC}"
        return 1
    fi
    
    echo ""
}

# Check scheduler configuration
check_scheduler_config() {
    echo -e "${CYAN}⚙️  Checking scheduler configuration...${NC}"
    
    if [ -f "$CONFIG_FILE" ]; then
        echo -e "${GREEN}✅ Configuration file: ${CONFIG_FILE}${NC}"
        
        # Check if scx_gamer is in scheduler list
        if grep -q "scx_gamer" "$CONFIG_FILE"; then
            echo -e "${GREEN}✅ scx_gamer found in scheduler list${NC}"
        else
            echo -e "${YELLOW}⚠️  scx_gamer not found in scheduler list${NC}"
        fi
        
        # Check current scheduler
        CURRENT_SCHED=$(grep "^SCX_SCHEDULER=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "")
        if [ -n "$CURRENT_SCHED" ]; then
            echo -e "${YELLOW}   Current scheduler: ${CURRENT_SCHED}${NC}"
        else
            echo -e "${YELLOW}   Current scheduler: Not set${NC}"
        fi
        
        # Check scheduler flags
        SCHED_FLAGS=$(grep "^SCX_FLAGS=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "")
        if [ -n "$SCHED_FLAGS" ]; then
            echo -e "${YELLOW}   Scheduler flags: ${SCHED_FLAGS}${NC}"
        else
            echo -e "${YELLOW}   Scheduler flags: Not set${NC}"
        fi
    else
        echo -e "${RED}❌ Configuration file missing: ${CONFIG_FILE}${NC}"
        return 1
    fi
    
    echo ""
}

# Check loader configuration
check_loader_config() {
    echo -e "${CYAN}⚙️  Checking loader configuration...${NC}"
    
    if [ -f "$LOADER_CONFIG" ]; then
        echo -e "${GREEN}✅ Loader configuration: ${LOADER_CONFIG}${NC}"
        
        # Check if scx_gamer section exists
        if grep -q "\[scheds\.scx_gamer\]" "$LOADER_CONFIG"; then
            echo -e "${GREEN}✅ scx_gamer section found in loader config${NC}"
            
            # Show scx_gamer configuration
            echo -e "${YELLOW}   scx_gamer configuration:${NC}"
            sed -n '/\[scheds\.scx_gamer\]/,/^\[/p' "$LOADER_CONFIG" | head -n -1 | while read line; do
                echo -e "${YELLOW}     ${line}${NC}"
            done
        else
            echo -e "${YELLOW}⚠️  scx_gamer section not found in loader config${NC}"
        fi
        
        # Check default mode
        DEFAULT_MODE=$(grep "^default_mode" "$LOADER_CONFIG" 2>/dev/null | cut -d= -f2 | tr -d ' "' || echo "")
        if [ -n "$DEFAULT_MODE" ]; then
            echo -e "${YELLOW}   Default mode: ${DEFAULT_MODE}${NC}"
        else
            echo -e "${YELLOW}   Default mode: Not set${NC}"
        fi
    else
        echo -e "${RED}❌ Loader configuration missing: ${LOADER_CONFIG}${NC}"
        return 1
    fi
    
    echo ""
}

# Check desktop integration
check_desktop_integration() {
    echo -e "${CYAN}🖥️  Checking desktop integration...${NC}"
    
    # Check desktop entry
    if [ -f "${DESKTOP_DIR}/scx-gamer-manager.desktop" ]; then
        echo -e "${GREEN}✅ Desktop entry: ${DESKTOP_DIR}/scx-gamer-manager.desktop${NC}"
        
        # Check desktop entry content
        DESKTOP_NAME=$(grep "^Name=" "${DESKTOP_DIR}/scx-gamer-manager.desktop" 2>/dev/null | cut -d= -f2 || echo "")
        if [ -n "$DESKTOP_NAME" ]; then
            echo -e "${YELLOW}   Name: ${DESKTOP_NAME}${NC}"
        fi
        
        DESKTOP_EXEC=$(grep "^Exec=" "${DESKTOP_DIR}/scx-gamer-manager.desktop" 2>/dev/null | cut -d= -f2 || echo "")
        if [ -n "$DESKTOP_EXEC" ]; then
            echo -e "${YELLOW}   Exec: ${DESKTOP_EXEC}${NC}"
        fi
    else
        echo -e "${YELLOW}⚠️  Desktop entry not found: ${DESKTOP_DIR}/scx-gamer-manager.desktop${NC}"
    fi
    
    # Check icon
    if [ -f "${ICON_DIR}/scx-gamer.svg" ]; then
        echo -e "${GREEN}✅ Icon: ${ICON_DIR}/scx-gamer.svg${NC}"
        
        # Check icon size
        ICON_SIZE=$(du -h "${ICON_DIR}/scx-gamer.svg" | cut -f1)
        echo -e "${YELLOW}   Size: ${ICON_SIZE}${NC}"
    else
        echo -e "${YELLOW}⚠️  Icon not found: ${ICON_DIR}/scx-gamer.svg${NC}"
    fi
    
    echo ""
}

# Check systemd service
check_systemd_service() {
    echo -e "${CYAN}🔧 Checking systemd service...${NC}"
    
    # Check if scx.service exists
    if systemctl list-unit-files | grep -q "scx.service"; then
        echo -e "${GREEN}✅ scx.service unit file found${NC}"
        
        # Check service status
        if systemctl is-active --quiet scx.service; then
            echo -e "${GREEN}✅ scx.service is active${NC}"
        else
            echo -e "${YELLOW}⚠️  scx.service is not active${NC}"
        fi
        
        # Check service status
        SERVICE_STATUS=$(systemctl is-enabled scx.service 2>/dev/null || echo "unknown")
        echo -e "${YELLOW}   Service status: ${SERVICE_STATUS}${NC}"
        
        # Check if scx_gamer is currently running
        if systemctl is-active --quiet scx.service; then
            CURRENT_SCHED=$(grep "^SCX_SCHEDULER=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "")
            if [ "$CURRENT_SCHED" = "scx_gamer" ]; then
                echo -e "${GREEN}✅ scx_gamer is currently running${NC}"
            else
                echo -e "${YELLOW}⚠️  scx_gamer is not currently running (current: ${CURRENT_SCHED})${NC}"
            fi
        fi
    else
        echo -e "${YELLOW}⚠️  scx.service unit file not found${NC}"
    fi
    
    echo ""
}

# Check scx_loader
check_scx_loader() {
    echo -e "${CYAN}🔧 Checking scx_loader...${NC}"
    
    # Check if scx_loader binary exists
    if [ -f "/usr/bin/scx_loader" ]; then
        echo -e "${GREEN}✅ scx_loader binary found${NC}"
        
        # Check scx_loader version
        if LOADER_VERSION=$(scx_loader --version 2>&1 | head -1); then
            echo -e "${YELLOW}   Version: ${LOADER_VERSION}${NC}"
        else
            echo -e "${YELLOW}   Version: Unable to determine${NC}"
        fi
    else
        echo -e "${YELLOW}⚠️  scx_loader binary not found${NC}"
    fi
    
    # Check scx_loader service
    if systemctl list-unit-files | grep -q "scx_loader.service"; then
        echo -e "${GREEN}✅ scx_loader.service unit file found${NC}"
        
        if systemctl is-active --quiet scx_loader.service; then
            echo -e "${GREEN}✅ scx_loader.service is active${NC}"
        else
            echo -e "${YELLOW}⚠️  scx_loader.service is not active${NC}"
        fi
    else
        echo -e "${YELLOW}⚠️  scx_loader.service unit file not found${NC}"
    fi
    
    echo ""
}

# Check GUI tools
check_gui_tools() {
    echo -e "${CYAN}🖥️  Checking GUI tools...${NC}"
    
    # Check scx-manager
    if command -v scx-manager >/dev/null 2>&1; then
        echo -e "${GREEN}✅ scx-manager command available${NC}"
    else
        echo -e "${YELLOW}⚠️  scx-manager command not found${NC}"
    fi
    
    # Check if GUI can be launched
    if command -v scx-manager >/dev/null 2>&1; then
        echo -e "${YELLOW}   GUI tool: scx-manager${NC}"
        echo -e "${YELLOW}   Launch command: scx-manager${NC}"
    fi
    
    echo ""
}

# Performance test
performance_test() {
    echo -e "${CYAN}⚡ Running performance test...${NC}"
    
    if [ -f "${INSTALL_DIR}/${SCHEDULER_NAME}" ]; then
        echo -e "${YELLOW}   Testing binary execution...${NC}"
        
        # Test version command
        if "${INSTALL_DIR}/${SCHEDULER_NAME}" --version >/dev/null 2>&1; then
            echo -e "${GREEN}✅ Version command works${NC}"
        else
            echo -e "${RED}❌ Version command failed${NC}"
        fi
        
        # Test help command
        if "${INSTALL_DIR}/${SCHEDULER_NAME}" --help >/dev/null 2>&1; then
            echo -e "${GREEN}✅ Help command works${NC}"
        else
            echo -e "${RED}❌ Help command failed${NC}"
        fi
        
        # Test stats command
        if "${INSTALL_DIR}/${SCHEDULER_NAME}" --help-stats >/dev/null 2>&1; then
            echo -e "${GREEN}✅ Stats command works${NC}"
        else
            echo -e "${YELLOW}⚠️  Stats command not available${NC}"
        fi
    else
        echo -e "${RED}❌ Cannot run performance test - binary not found${NC}"
    fi
    
    echo ""
}

# Generate report
generate_report() {
    echo -e "${CYAN}📊 Generating installation report...${NC}"
    
    REPORT_FILE="/tmp/scx_gamer_verification_$(date +%Y%m%d_%H%M%S).txt"
    
    {
        echo "scx_gamer Installation Verification Report"
        echo "Generated: $(date)"
        echo "System: $(uname -a)"
        echo "Kernel: $(uname -r)"
        echo ""
        echo "Binary: ${INSTALL_DIR}/${SCHEDULER_NAME}"
        echo "Config: ${CONFIG_FILE}"
        echo "Loader: ${LOADER_CONFIG}"
        echo ""
        echo "Current Scheduler: $(grep "^SCX_SCHEDULER=" "$CONFIG_FILE" 2>/dev/null | cut -d= -f2 || echo "Not set")"
        echo "Service Status: $(systemctl is-active scx.service 2>/dev/null || echo "Unknown")"
        echo ""
        echo "Desktop Entry: ${DESKTOP_DIR}/scx-gamer-manager.desktop"
        echo "Icon: ${ICON_DIR}/scx-gamer.svg"
        echo ""
        echo "CachyOS Packages:"
        pacman -Q scx-scheds scx-manager 2>/dev/null || echo "Not installed"
        echo ""
        echo "End of Report"
    } > "$REPORT_FILE"
    
    echo -e "${GREEN}✅ Report generated: ${REPORT_FILE}${NC}"
    echo ""
}

# Main verification function
main() {
    print_banner
    
    check_system_requirements
    check_binary
    check_scheduler_config
    check_loader_config
    check_desktop_integration
    check_systemd_service
    check_scx_loader
    check_gui_tools
    performance_test
    generate_report
    
    # Summary
    echo -e "${GREEN}╔══════════════════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${GREEN}║                    Verification Complete!                                   ║${NC}"
    echo -e "${GREEN}╚══════════════════════════════════════════════════════════════════════════════╝${NC}"
    echo ""
    
    echo -e "${BLUE}🎮 Next Steps:${NC}"
    echo ""
    echo "1. ${CYAN}Open SchedEXT GUI Manager:${NC}"
    echo "   ${YELLOW}scx-manager${NC} or search 'Scheduler' in your app menu"
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
    echo "4. ${CYAN}Apply and start the scheduler from the GUI${NC}"
    echo ""
    
    echo -e "${BLUE}🔧 Manual Commands:${NC}"
    echo "  ${YELLOW}sudo systemctl status scx.service${NC}  # Check scheduler status"
    echo "  ${YELLOW}scxstats -s scx_gamer${NC}             # View scheduler statistics"
    echo "  ${YELLOW}sudo ./uninstall-cachyos.sh${NC}       # Uninstall scx_gamer"
    echo ""
    
    echo -e "${PURPLE}⚠️  Experimental Research Project${NC}"
    echo "This scheduler is an experimental research project developed with AI assistance."
    echo "Results may vary based on hardware configuration and game engine."
    echo ""
    
    echo -e "${GREEN}🎉 scx_gamer installation verification complete!${NC}"
}

# Run main function
main "$@"
