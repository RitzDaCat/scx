# scx_gamer Quick Start - CachyOS Integration

## Installation (3 steps)

```bash
# 1. Build
cd /home/ritz/Documents/Repo/Linux/scx
cargo build --release --package scx_gamer

# 2. Install
cd scheds/rust/scx_gamer
sudo ./INSTALL.sh

# 3. Use GUI
scx-manager
# → Select "scx_gamer"
# → Choose "Gaming" profile
# → Click "Apply"
```

## Profiles

| Profile | Use Case | Flags |
|---------|----------|-------|
| **Gaming** | 4K 240Hz / 1080p 480Hz | `--slice-us 10 --mm-affinity --wakeup-timer-us 100` |
| **LowLatency** | Esports/Competitive 480Hz+ | `--slice-us 5 --wakeup-timer-us 50` |
| **PowerSave** | Battery-friendly | `--slice-us 20` |
| **Server** | Background tasks | `--slice-us 15` |

## Custom Flags (Optional)

Add via GUI "Set sched-ext extra scheduler flags":

```bash
--ml-profiles              # Auto-load per-game configs
--ml-collect               # Collect training data
--stats 5.0                # Show stats every 5s
--verbose                  # Debug logging
```

## Verification

```bash
# Check if running
sudo systemctl status scx.service

# View stats
scxstats -s scx_gamer

# Check logs
journalctl -u scx.service -f
```

## Uninstall

```bash
sudo ./UNINSTALL.sh
```

## Troubleshooting

**Not in GUI dropdown?**
```bash
grep scx_gamer /etc/default/scx  # Should appear in line 1
```

**Service won't start?**
```bash
journalctl -u scx.service -n 50  # Check error logs
ls -la /usr/bin/scx_gamer        # Verify binary exists
```

**Game not detected?**
```bash
# Add verbose flag via GUI, then:
journalctl -u scx.service | grep "Game detected"
```

## Files

- Binary: `/usr/bin/scx_gamer`
- Config: `/etc/default/scx`
- Profiles: `/etc/scx_loader.toml`
- Service: `systemctl status scx.service`

## More Info

- [Full Integration Guide](CACHYOS_INTEGRATION.md)
- [Full README](README.md)
