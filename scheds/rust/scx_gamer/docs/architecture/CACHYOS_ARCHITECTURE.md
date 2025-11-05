# CachyOS sched-ext Integration Architecture

## System Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                   CachyOS Kernel Manager (GUI)                  │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────────────┐│
│  │  Scheduler   │  │   Profile    │  │   Custom Flags         ││
│  │  Dropdown    │  │   Selector   │  │   Input Box            ││
│  │              │  │              │  │                        ││
│  │ scx_gamer ▼  │  │  Gaming   ▼  │  │ --ml-profiles --stats  ││
│  └──────────────┘  └──────────────┘  └────────────────────────┘│
│                           │                                      │
│                           ▼                                      │
│                      [Apply Button]                              │
└─────────────────────────────│───────────────────────────────────┘
                              │
                              │ Writes to:
                              ▼
         ┌────────────────────────────────────────┐
         │      /etc/default/scx                  │
         │  ┌──────────────────────────────────┐  │
         │  │ SCX_SCHEDULER=scx_gamer          │  │
         │  │ SCX_FLAGS=--slice-us 10 --mm...  │  │
         │  └──────────────────────────────────┘  │
         └────────────────┬───────────────────────┘
                          │
                          │ systemctl restart scx.service
                          ▼
         ┌────────────────────────────────────────┐
         │  /usr/lib/systemd/system/scx.service   │
         │  ┌──────────────────────────────────┐  │
         │  │ EnvironmentFile=/etc/default/scx │  │
         │  │ ExecStart=$SCX_SCHEDULER $FLAGS  │  │
         │  └──────────────────────────────────┘  │
         └────────────────┬───────────────────────┘
                          │
                          │ Starts:
                          ▼
         ┌────────────────────────────────────────┐
         │      /usr/bin/scx_gamer                │
         │  (with flags from profile + custom)    │
         └────────────────────────────────────────┘
```

## Profile Resolution

```
User selects "Gaming" profile
         │
         ▼
GUI reads /etc/scx_loader.toml
         │
         ▼
┌─────────────────────────────────────────────┐
│ [scheds.scx_gamer]                          │
│ gaming_mode = ["--slice-us", "10",          │
│                "--mm-affinity",             │
│                "--wakeup-timer-us", "100"]  │
└─────────────────────────────────────────────┘
         │
         ▼
Combines with custom flags (if any)
         │
         ▼
Writes to /etc/default/scx:
SCX_FLAGS="--slice-us 10 --mm-affinity --wakeup-timer-us 100 --ml-profiles"
         │
         ▼
systemd launches:
/usr/bin/scx_gamer --slice-us 10 --mm-affinity --wakeup-timer-us 100 --ml-profiles
```

## File Hierarchy

```
/usr/bin/
  └── scx_gamer                    # Binary (installed by INSTALL.sh)

/etc/
  ├── default/
  │   └── scx                      # Active scheduler + flags
  └── scx_loader.toml              # Profile definitions

/usr/lib/systemd/system/
  ├── scx.service                  # Main service
  └── scx_loader.service           # Loader service

/var/log/journal/
  └── [system logs]                # Accessible via journalctl -u scx.service
```

## Installation Flow

```
1. cargo build --release
         │
         ▼
2. sudo ./INSTALL.sh
         │
         ├─→ Copy binary to /usr/bin/scx_gamer
         │
         ├─→ Backup /etc/default/scx
         │
         ├─→ Add scx_gamer to scheduler list in /etc/default/scx
         │
         ├─→ Backup /etc/scx_loader.toml
         │
         └─→ Append [scheds.scx_gamer] section to /etc/scx_loader.toml
         │
         ▼
3. Open cachyos-kernel-manager
         │
         ├─→ Select scx_gamer from dropdown
         │
         ├─→ Choose Gaming profile
         │
         └─→ (Optional) Add custom flags
         │
         ▼
4. Click Apply
         │
         └─→ systemctl restart scx.service
         │
         ▼
5. Scheduler running!
```

## Uninstallation Flow

```
1. sudo ./UNINSTALL.sh
         │
         ├─→ Stop scx.service (if scx_gamer is active)
         │
         ├─→ Remove /usr/bin/scx_gamer
         │
         ├─→ Backup /etc/default/scx
         │
         ├─→ Remove scx_gamer from scheduler list
         │
         ├─→ Change SCX_SCHEDULER if set to scx_gamer
         │
         ├─→ Backup /etc/scx_loader.toml
         │
         └─→ Remove [scheds.scx_gamer] section
         │
         ▼
2. Select different scheduler via GUI
         │
         └─→ systemctl start scx.service
```

## Runtime Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      Kernel Space                            │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         BPF sched_ext Program (main.bpf.c)             │  │
│  │  ┌──────────────┐  ┌──────────────┐  ┌─────────────┐  │  │
│  │  │ select_cpu() │→│  enqueue()   │→│ dispatch()  │  │  │
│  │  └──────────────┘  └──────────────┘  └─────────────┘  │  │
│  │                                                        │  │
│  │  ┌──────────────────────────────────────────────────┐ │  │
│  │  │    BPF LSM (game_detect_lsm.bpf.c)               │ │  │
│  │  │    • bprm_committed_creds (exec events)          │ │  │
│  │  │    • task_free (exit events)                     │ │  │
│  │  └──────────────┬───────────────────────────────────┘ │  │
│  └─────────────────┼─────────────────────────────────────┘  │
└────────────────────┼────────────────────────────────────────┘
                     │ Ring Buffer
                     ▼
┌──────────────────────────────────────────────────────────────┐
│                     User Space                               │
│  ┌────────────────────────────────────────────────────────┐  │
│  │         /usr/bin/scx_gamer (main.rs)                   │  │
│  │  ┌──────────────────┐  ┌────────────────────────────┐  │  │
│  │  │ BPF Game Detect  │  │   ML Auto-tuner (opt)      │  │  │
│  │  │  (200ms poll)    │  │   Stats Monitor (opt)      │  │  │
│  │  └──────────────────┘  └────────────────────────────┘  │  │
│  └────────────────────────────────────────────────────────┘  │
└──────────────────────────────────────────────────────────────┘
```

## Profile Customization Workflow

```
User wants custom profile for specific game
         │
         ▼
Option 1: GUI Custom Flags
         │
         ├─→ Select "Gaming" base profile
         │
         ├─→ Add custom flags: "--avoid-smt --stats 1.0"
         │
         └─→ Flags combined: [gaming_mode] + [custom]
         │
         ▼
    Applied immediately

Option 2: ML Profiles (Advanced)
         │
         ├─→ Enable --ml-collect flag
         │
         ├─→ Play game for 30+ minutes
         │
         ├─→ Training data saved to ml_data/
         │
         ├─→ Run ml_train.py (if implementing ML)
         │
         ├─→ Enable --ml-profiles flag
         │
         └─→ Auto-loaded when game detected
```

## Integration Points

| Component | Interface | Purpose |
|-----------|-----------|---------|
| **CachyOS GUI** | Reads `/etc/scx_loader.toml` | Get available profiles |
| **CachyOS GUI** | Writes `/etc/default/scx` | Set active scheduler + flags |
| **systemd** | Reads `/etc/default/scx` | Launch scheduler with flags |
| **scx_gamer** | Reads cmdline args | Apply configuration |
| **scx_gamer** | BPF ring buffer | Receive game events from kernel |
| **scx_gamer** | Stats server | Provide metrics to scxstats tool |

## Security Considerations

1. **Root required**: Installation modifies `/etc/` and `/usr/bin/`
2. **Backups created**: All config changes are backed up with timestamps
3. **Service isolation**: Runs as systemd service with proper permissions
4. **BPF verification**: Kernel verifies BPF programs before loading
5. **No network access**: Scheduler runs entirely offline

## Debugging Flow

```
Issue: Scheduler not starting
         │
         ▼
1. Check service status
   $ systemctl status scx.service
         │
         ▼
2. Check logs
   $ journalctl -u scx.service -n 50
         │
         ▼
3. Verify files
   $ ls -la /usr/bin/scx_gamer
   $ cat /etc/default/scx
   $ cat /etc/scx_loader.toml
         │
         ▼
4. Test manually
   $ sudo scx_gamer --stats 1.0 --verbose
         │
         ▼
5. Check BPF support
   $ uname -r  # Must be 6.12+ with CONFIG_SCHED_CLASS_EXT=y
```
