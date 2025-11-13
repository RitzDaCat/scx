# EEXIST Tracepoint Error - Troubleshooting Guide

**Error:** `failed to create BPF link for perf_event FD: -EEXIST`

---

## Problem

The affinity override system fails to attach because the tracepoint/kprobe is already attached by a previous `scx_gamer` instance.

**Symptoms:**
```
libbpf: prog 'affinity_syscall_enter': failed to attach to tracepoint 'syscalls/sys_enter_sched_setaffinity': -EEXIST
[WARN] Failed to initialize affinity override system
```

---

## Root Cause

**BPF hooks can only be attached once per system.** If a previous `scx_gamer` instance is still running, it holds the attachment, preventing new instances from attaching.

**Common Scenarios:**
1. Previous instance crashed without cleanup
2. Multiple instances started simultaneously
3. Instance running in background (forgot to stop)

---

## Solution

### Step 1: Check for Running Instances

```bash
# Check for running scx_gamer processes
ps aux | grep scx_gamer | grep -v grep

# Or use pkill to see what would be killed
sudo pkill -l scx_gamer
```

### Step 2: Kill Existing Instances

```bash
# Kill all scx_gamer processes
sudo pkill scx_gamer

# Verify they're gone
ps aux | grep scx_gamer | grep -v grep
# Should return nothing

# Also check for sched_ext_helper
ps aux | grep sched_ext_helper
sudo pkill sched_ext_helper
```

### Step 3: Verify BPF Hooks Are Detached

```bash
# Check if tracepoint is still attached (requires bpftool)
sudo bpftool prog list | grep affinity

# If you see entries, they should be from the old instance
# They'll be cleaned up when the process exits
```

### Step 4: Restart Scheduler

```bash
# Now start fresh
sudo scx_gamer
```

**Expected Output:**
```
[INFO] Affinity override system: Enabled (proactive detection + reset)
```

---

## Prevention

### Always Stop Scheduler Cleanly

```bash
# Use Ctrl+C in the terminal running scx_gamer
# Or if running in background:
sudo pkill scx_gamer
```

### Check Before Starting

```bash
# Quick check before starting
if pgrep -x scx_gamer > /dev/null; then
    echo "scx_gamer is already running!"
    echo "Kill it with: sudo pkill scx_gamer"
    exit 1
fi
```

---

## Code Behavior

**With EEXIST Error Handling (Current Implementation):**

1. **Both hooks fail (EEXIST):**
   - Returns error, scheduler exits
   - User must kill old instance

2. **Only syscall hook fails:**
   - Warns but continues
   - Detection less accurate (may override kernel-set affinities)
   - Not recommended for production

3. **Both hooks succeed:**
   - Full functionality
   - Proper userspace vs kernel detection

---

## Verification

**After fixing, verify it's working:**

```bash
# 1. Start scheduler
sudo scx_gamer

# 2. In another terminal, test affinity override
sudo taskset -pc 0 <PID>

# 3. Check if override worked
taskset -pc <PID>
# Should show: "pid <PID> current affinity list: 0-15" (full mask)

# 4. Check logs for:
# "Affinity override: first event processed (pid=... comm='...' nr_cpus=1)"
```

---

**Last Updated:** 2025-11-09

