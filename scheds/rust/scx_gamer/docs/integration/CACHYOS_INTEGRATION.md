# CachyOS Integration Guide for scx_gamer

This guide explains how to integrate scx_gamer with CachyOS's sched-ext GUI tool.

## Overview

CachyOS provides a GUI tool for managing sched-ext schedulers with:
- Dropdown scheduler selection
- Profile selection (Gaming, LowLatency, PowerSave, Server, Auto)
- Custom flags input
- Systemd service integration

## Installation Methods

### Method 1: Quick Install Script (Recommended)

1. **Build scx_gamer:**
   ```bash
   cd /path/to/scx  # Your scx repository root
   cargo build --release --package scx_gamer
   ```

2. **Run installer:**
   ```bash
   cd scheds/rust/scx_gamer
   chmod +x INSTALL.sh
   sudo ./INSTALL.sh
   ```

3. **Open CachyOS GUI:**
   - Launch `scx-manager` or search "Scheduler" in app menu
   - Select **scx_gamer** from dropdown
   - Choose profile (e.g., "Gaming")
   - (Optional) Add custom flags
   - Click "Apply" or "Start"

### Method 2: Manual Installation

1. **Install binary:**
   ```bash
   sudo install -m 755 target/release/scx_gamer /usr/bin/
   ```

2. **Add to scheduler list** (`/etc/default/scx`):
   ```bash
   sudo nano /etc/default/scx
   # Edit line 1 to add scx_gamer:
   # List of scx_schedulers: scx_bpfland ... scx_rusty scx_gamer
   ```

3. **Add profiles** (`/etc/scx_loader.toml`):
   ```bash
   sudo nano /etc/scx_loader.toml
   # Add at the end:
   ```
   ```toml
   [scheds.scx_gamer]
   auto_mode = []
   gaming_mode = ["--slice-us", "10", "--mm-affinity", "--wakeup-timer-us", "100", "--stats", "0"]
   lowlatency_mode = ["--slice-us", "5", "--mm-affinity", "--wakeup-timer-us", "50", "--stats", "0"]
   powersave_mode = ["--slice-us", "20", "--stats", "0"]
   server_mode = ["--slice-us", "15", "--stats", "0"]
   ```

4. **Use CachyOS GUI** to select and start scx_gamer

### Method 3: PKGBUILD (Proper Package Management)

For proper Arch/CachyOS package management:

1. **Build package:**
   ```bash
   cd scheds/rust/scx_gamer
   makepkg -si
   ```

2. **Follow post-install instructions** to integrate with GUI

3. **Uninstall cleanly:**
   ```bash
   sudo pacman -R scx-gamer
   ```

## Profile Definitions

### Gaming Mode (Default for 4K 240Hz / 1080p 480Hz)
```
--slice-us 10 --mm-affinity --wakeup-timer-us 100 --input-window-us 2000 --stats 0
```
- **10μs slice**: Fast preemption for low latency
- **MM affinity**: Cache-aware task placement
- **100μs wakeup timer**: Low frame timing variance
- **2ms input window**: Covers Wine/Proton input translation delays
- **No stats**: Silent operation (minimal overhead)

### LowLatency Mode (Esports/Competitive)
```
--slice-us 5 --mm-affinity --wakeup-timer-us 50 --input-window-us 1000 --avoid-smt --stats 0
```
- **5μs slice**: Ultra-fast preemption
- **50μs wakeup timer**: Minimal jitter (best for 480Hz+)
- **1ms input window**: Ultra-low input latency
- **SMT avoidance**: Reduce core contention

### PowerSave Mode (Battery-Friendly)
```
--slice-us 20 --input-window-us 3000 --stats 0
```
- **20μs slice**: Longer slices reduce context switching overhead
- **3ms input window**: Relaxed for power efficiency
- No aggressive optimizations

### Server Mode (Background Tasks)
```
--slice-us 15 --input-window-us 0 --stats 0
```
- **15μs slice**: Balanced for throughput vs latency
- **No input boost**: Focus on throughput

## Custom Flags (Advanced)

You can add these via "Set sched-ext extra scheduler flags" in the GUI:

### ML Auto-tuning (Recommended for New Users)
```
--ml-autotune
```
- **Automatic parameter tuning**: Finds optimal config for your game
- **Duration**: 15 minutes (plays normally, scheduler experiments)
- **Result**: Saves best config as profile, auto-loads next time
- **Zero manual config needed!**

### ML Per-Game Profiles
```
--ml-profiles
```
- Auto-loads saved configurations per game
- Profiles stored in `ml_data/{CPU_MODEL}/`
- Requires prior auto-tune or manual training
- **Example**: Counter-Strike 2 gets different config than Cyberpunk 2077

### ML Data Collection
```
--ml-collect
```
- Collects scheduler metrics for analysis
- Saves to `ml_data/{CPU_MODEL}/{GAME}.json`
- Use with `--ml-autotune` for best results

### Advanced Detection
```
--disable-bpf-lsm --disable-wine-detect
```
- **Fallback mode** for anti-cheat compatibility
- Disables BPF LSM hooks (kernel-level detection)
- Disables Wine priority tracking (uprobe)
- Use if anti-cheat flags advanced features

### Debugging
```
--verbose --stats 1.0
```
- Shows debug logs (BPF verifier, device detection)
- Prints stats every 1 second

### Combined Gaming Setup
```
--ml-profiles --stats 5.0
```
- Auto-load game configs
- Show stats every 5 seconds

### Complete Auto-Tune Example
```
--ml-autotune --stats 1.0
```
- Find optimal config automatically
- Monitor progress with stats

## Using the GUI Tool

1. **Open CachyOS sched-ext Manager:**
   ```bash
   scx-manager
   ```
   Or search "Scheduler" in your application menu

2. **Select Scheduler:**
   - Dropdown: `scx_gamer`

3. **Select Profile:**
   - Gaming (recommended for most)
   - LowLatency (esports/competitive)
   - PowerSave (battery)
   - Server (background tasks)
   - Auto (default profile)

4. **Add Custom Flags (Optional):**
   - Click "Set sched-ext extra scheduler flags"
   - Enter flags like: `--ml-profiles --stats 5.0`

5. **Apply:**
   - Click "Apply" or "Start Scheduler"
   - Scheduler runs via systemd service

## Verification

**Check if running:**
```bash
sudo systemctl status scx.service
```

**View stats:**
```bash
scxstats -s scx_gamer
```

**Check logs:**
```bash
journalctl -u scx.service -f
```

**Verify game detection:**
- Launch a game
- Check logs: `journalctl -u scx.service | grep "Game detected"`

## Uninstallation

### Method 1: Uninstall Script
```bash
cd /home/ritz/Documents/Repo/Linux/scx/scheds/rust/scx_gamer
sudo ./UNINSTALL.sh
```

### Method 2: Manual
1. Stop scheduler (if using scx_gamer):
   ```bash
   sudo systemctl stop scx.service
   ```

2. Remove binary:
   ```bash
   sudo rm /usr/bin/scx_gamer
   ```

3. Edit `/etc/default/scx`:
   - Remove `scx_gamer` from scheduler list
   - Change `SCX_SCHEDULER=scx_gamer` to another scheduler

4. Edit `/etc/scx_loader.toml`:
   - Remove `[scheds.scx_gamer]` section

5. Restart service:
   ```bash
   sudo systemctl start scx.service
   ```

### Method 3: PKGBUILD
```bash
sudo pacman -R scx-gamer
# Then manually clean /etc/scx_loader.toml if needed
```

## Troubleshooting

### Scheduler doesn't appear in GUI dropdown
- Check `/etc/default/scx` line 1 includes `scx_gamer`
- Restart GUI tool

### Profile doesn't work
- Verify `/etc/scx_loader.toml` has `[scheds.scx_gamer]` section
- Check for syntax errors (TOML format)

### Service fails to start
- Check logs: `journalctl -u scx.service -n 50`
- Verify binary exists: `ls -la /usr/bin/scx_gamer`
- Check kernel support: `uname -r` (need 6.12+ with sched_ext)

### Game not detected
- Check BPF LSM is working: `journalctl -u scx.service | grep "BPF LSM"`
- Verify game is running: `ps aux | grep -i game`
- Enable verbose logging: Add `--verbose` flag via GUI

## File Locations

| File | Purpose |
|------|---------|
| `/usr/bin/scx_gamer` | Scheduler binary |
| `/etc/default/scx` | Active scheduler selection |
| `/etc/scx_loader.toml` | Profile definitions |
| `/usr/lib/systemd/system/scx.service` | Systemd service file |
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

- [scx_gamer README](README.md) - Scheduler details
- [CachyOS Wiki](https://wiki.cachyos.org/) - CachyOS documentation
- [sched-ext Documentation](https://docs.kernel.org/scheduler/sched-ext.html) - Kernel docs
