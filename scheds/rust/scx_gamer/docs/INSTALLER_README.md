# scx_gamer CachyOS Installer

This directory contains installation scripts for integrating scx_gamer with CachyOS's SchedEXT GUI Manager.

## Installation Methods

### Method 1: Quick Install Script (Recommended)

**Prerequisites:**
- CachyOS with kernel 6.12+ and sched_ext support
- scx-scheds and scx-manager packages installed
- Built scx_gamer binary

**Installation:**
```bash
# Build scx_gamer first
cd /path/to/scx
cargo build --release --package scx_gamer

# Run installer
cd scheds/rust/scx_gamer
sudo ./install-cachyos.sh
```

**What the installer does:**
- [IMPLEMENTED] Checks system requirements (kernel version, sched_ext support)
- [IMPLEMENTED] Installs scx_gamer binary to `/usr/bin/`
- [IMPLEMENTED] Updates scheduler configuration in `/etc/default/scx`
- [IMPLEMENTED] Generates complete `scx_loader.toml` with all profiles
- [IMPLEMENTED] Builds and installs updated `scx_loader` with scx_gamer support
- [IMPLEMENTED] Creates desktop entry for easy GUI access
- [IMPLEMENTED] Creates custom icon for the scheduler
- [IMPLEMENTED] Verifies installation and provides next steps

### Method 2: PKGBUILD Package (Advanced)

**Build and install package:**
```bash
cd scheds/rust/scx_gamer
makepkg -si
```

**What the package provides:**
- [IMPLEMENTED] Proper Arch/CachyOS package management
- [IMPLEMENTED] Desktop entry and icon
- [IMPLEMENTED] Documentation in `/usr/share/doc/scx-gamer/`
- [IMPLEMENTED] Configuration templates
- [IMPLEMENTED] Post-install and pre-remove hooks
- [IMPLEMENTED] Automatic dependency management

### Method 3: Manual Installation

**Step-by-step manual installation:**
```bash
# 1. Install binary
sudo install -m 755 target/release/scx_gamer /usr/bin/

# 2. Add to scheduler list
sudo nano /etc/default/scx
# Edit line 1 to add scx_gamer:
# List of scx_schedulers: scx_bpfland ... scx_rusty scx_gamer

# 3. Add profiles to loader config
sudo nano /etc/scx_loader.toml
# Add at the end:
[scheds.scx_gamer]
auto_mode = ["--slice-us", "10", "--mm-affinity", "--wakeup-timer-us", "100", "--stats", "0"]
gaming_mode = ["--slice-us", "10", "--mm-affinity", "--wakeup-timer-us", "100", "--stats", "0"]
lowlatency_mode = ["--slice-us", "5", "--mm-affinity", "--wakeup-timer-us", "50", "--stats", "0", "--busy-polling"]
powersave_mode = ["--slice-us", "20", "--stats", "0"]
server_mode = ["--slice-us", "15", "--stats", "0"]

# 4. Use CachyOS GUI to select and start scx_gamer
```

## Usage After Installation

### Using SchedEXT GUI Manager

1. **Launch GUI:**
   ```bash
   scx-manager
   # Or search "Scheduler" in your app menu
   # Or use the new "SchedEXT Gaming Manager" desktop entry
   ```

2. **Select scx_gamer:**
   - Choose `scx_gamer` from the scheduler dropdown
   - Select a profile (Gaming, LowLatency, PowerSave, Server)
   - (Optional) Add custom flags
   - Click "Apply" or "Start"

### Available Profiles

| Profile | Description | Flags |
|---------|-------------|-------|
| **Gaming** | Optimized for 4K 240Hz or 1080p 480Hz gaming | `--slice-us 10 --mm-affinity --wakeup-timer-us 100 --stats 0` |
| **LowLatency** | Ultra-low latency with busy polling (esports) | `--slice-us 5 --mm-affinity --wakeup-timer-us 50 --stats 0 --busy-polling` |
| **PowerSave** | Battery-friendly settings | `--slice-us 20 --stats 0` |
| **Server** | Balanced for background tasks | `--slice-us 15 --stats 0` |

### Custom Flags

You can add custom flags via the GUI's "Set sched-ext extra scheduler flags" option:

**Common flags:**
- `--ml-profiles` - Auto-load per-game configs
- `--ml-collect` - Collect training data
- `--verbose` - Debug logging
- `--busy-polling` - Ultra-low latency (consumes 100% CPU)
- `--realtime-scheduling` - Use SCHED_FIFO for ultra-low latency
- `--deadline-scheduling` - Use SCHED_DEADLINE for hard real-time guarantees

**Performance monitoring:**
- `--stats 5.0` - Show statistics every 5 seconds
- `--tui 1.0` - Launch interactive dashboard
- `--monitor 2.0` - Monitor mode (no scheduler launch)

## Verification

### Verify Installation
```bash
sudo ./verify-installation.sh
```

**What it checks:**
- [IMPLEMENTED] System requirements (kernel version, sched_ext support)
- [IMPLEMENTED] Binary installation and permissions
- [IMPLEMENTED] Scheduler configuration
- [IMPLEMENTED] Loader configuration
- [IMPLEMENTED] Desktop integration
- [IMPLEMENTED] Systemd service status
- [IMPLEMENTED] GUI tools availability
- [IMPLEMENTED] Performance tests

### Manual Verification
```bash
# Check binary
ls -la /usr/bin/scx_gamer
scx_gamer --version

# Check configuration
grep scx_gamer /etc/default/scx
grep "\[scheds\.scx_gamer\]" /etc/scx_loader.toml

# Check service status
sudo systemctl status scx.service

# Check scheduler statistics
scxstats -s scx_gamer
```

## Uninstallation

### Using Uninstall Script
```bash
sudo ./uninstall-cachyos.sh
```

**What the uninstaller does:**
- [IMPLEMENTED] Stops scx_gamer if currently running
- [IMPLEMENTED] Removes binary from `/usr/bin/`
- [IMPLEMENTED] Removes from scheduler configuration
- [IMPLEMENTED] Removes from loader configuration
- [IMPLEMENTED] Removes desktop entry and icon
- [IMPLEMENTED] Restores stable scx_loader from CachyOS repository
- [IMPLEMENTED] Cleans up remaining files
- [IMPLEMENTED] Verifies uninstallation

### Using Package Manager
```bash
sudo pacman -R scx-gamer
# Then manually clean /etc/scx_loader.toml if needed
```

### Manual Uninstallation
```bash
# 1. Stop scheduler
sudo systemctl stop scx.service

# 2. Remove binary
sudo rm /usr/bin/scx_gamer

# 3. Edit /etc/default/scx
sudo nano /etc/default/scx
# Remove scx_gamer from scheduler list
# Change SCX_SCHEDULER=scx_gamer to another scheduler

# 4. Edit /etc/scx_loader.toml
sudo nano /etc/scx_loader.toml
# Remove [scheds.scx_gamer] section

# 5. Restart service
sudo systemctl start scx.service
```

## Troubleshooting

### Scheduler doesn't appear in GUI dropdown
- Check `/etc/default/scx` line 1 includes `scx_gamer`
- Restart GUI tool
- Run `sudo ./verify-installation.sh`

### Profile doesn't work
- Verify `/etc/scx_loader.toml` has `[scheds.scx_gamer]` section
- Check for syntax errors (TOML format)
- Run `sudo ./verify-installation.sh`

### Service fails to start
- Check logs: `journalctl -u scx.service -n 50`
- Verify binary exists: `ls -la /usr/bin/scx_gamer`
- Check kernel support: `uname -r` (need 6.12+ with sched_ext)
- Run `sudo ./verify-installation.sh`

### Game not detected
- Check BPF LSM is working: `journalctl -u scx.service | grep "BPF LSM"`
- Verify game is running: `ps aux | grep -i game`
- Enable verbose logging: Add `--verbose` flag via GUI

### Performance issues
- Check CPU usage: `htop` or `top`
- Monitor scheduler statistics: `scxstats -s scx_gamer`
- Try different profiles (Gaming vs LowLatency)
- Check for conflicts with other schedulers

## File Locations

| File | Purpose |
|------|---------|
| `/usr/bin/scx_gamer` | Scheduler binary |
| `/etc/default/scx` | Active scheduler selection |
| `/etc/scx_loader.toml` | Profile definitions |
| `/usr/lib/systemd/system/scx.service` | Systemd service file |
| `/usr/share/applications/scx-gamer-manager.desktop` | Desktop entry |
| `/usr/share/icons/hicolor/256x256/apps/scx-gamer.svg` | Icon |
| `/var/log/journal/` | Service logs (via journalctl) |

## Integration with Other Tools

### scx_loader
If you prefer using `scx_loader` directly:
```bash
scx_loader start scx_gamer --profile gaming
```

### Systemd service override
Create custom override:
```bash
sudo systemctl edit scx.service
```
```ini
[Service]
Environment="SCX_SCHEDULER_OVERRIDE=scx_gamer"
Environment="SCX_FLAGS_OVERRIDE=--ml-profiles --stats 5.0"
```

## Support

For issues specific to:
- **CachyOS integration**: Check CachyOS forums
- **scx_gamer bugs**: Create issue in scx_gamer repo
- **sched-ext kernel**: Check kernel documentation

## Further Reading

- `README.md` - Project overview and features
- `docs/QUICK_START.md` - Quick start guide
- `docs/TECHNICAL_ARCHITECTURE.md` - Technical details
- `docs/CACHYOS_INTEGRATION.md` - CachyOS integration guide
