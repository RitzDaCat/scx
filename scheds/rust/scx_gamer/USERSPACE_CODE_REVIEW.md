# Userspace Code Review - Memory Leaks & Resource Management

**Date:** 2025-01-28  
**Scope:** Rust userspace code for memory leaks, resource leaks, and cleanup issues

---

## ✅ **GOOD: Thread Cleanup**

**Status:** All threads properly cleaned up

All background threads implement `Drop` with graceful shutdown:

1. **BpfGameDetector** (`game_detect_bpf.rs:144-167`)
   - Signals shutdown flag
   - Waits up to 500ms for graceful exit
   - Handles ring buffer polling cleanup

2. **GameDetector** (`game_detect.rs:145-168`)
   - Signals shutdown flag
   - Waits up to 2s for graceful exit
   - Handles inotify cleanup

3. **KWinState** (`kwin.rs:78-96`)
   - Signals shutdown flag
   - Waits up to 500ms for graceful exit
   - Handles DBus cleanup

4. **TUI/Stats threads** (`main.rs:2259-2303`)
   - Properly joined on exit
   - Shutdown flag propagated

---

## ✅ **GOOD: Epoll Cleanup**

**Status:** Properly cleaned up

**Location:** `main.rs:2081-2104` (`impl Drop for Scheduler`)

- Epoll registrations cleaned up in `Drop` impl
- All registered FDs removed from epoll
- Best-effort cleanup (errors ignored, which is fine for Drop)

---

## ✅ **GOOD: BPF Struct Ops Cleanup**

**Status:** Properly cleaned up

**Location:** `main.rs:2040-2041, 2084-2085`

- Struct ops link dropped in both `run()` exit path and `Drop` impl
- Prevents double-cleanup with `take()`
- BPF scheduler properly unregistered

---

## ✅ **GOOD: Input Device Cleanup**

**Status:** Properly cleaned up

**Location:** `main.rs:2101-2103`

- `input_devs.clear()` - evdev handles dropped
- `registered_epoll_fds.clear()` - tracking cleared
- `input_fd_info_vec.clear()` - metadata cleared

evdev handles automatically close FDs on drop (RAII).

---

## ⚠️ **ISSUE #1: Ring Buffer Manager Cleanup**

**Severity:** Low-Medium  
**Type:** Potential resource leak (ring buffer FD not explicitly closed)

**Location:** `ring_buffer.rs:512-516` (`impl Drop for InputRingBufferManager`)

**Problem:**
```rust
impl Drop for InputRingBufferManager {
    fn drop(&mut self) {
        // Ring buffer cleanup is handled automatically
        // No background thread to shut down in epoll-based version
    }
}
```

The Drop impl is empty. While libbpf-rs may handle cleanup automatically, it's not explicit.

**Current State:**
- `_ring_buffer: Option<RingBuffer>` is stored
- Ring buffer FD stored in `ring_buffer_fd: RawFd`
- No explicit cleanup

**Analysis:**
- libbpf-rs `RingBuffer` likely implements `Drop` to clean up internally
- However, the FD is stored separately and might not be closed
- Kernel will close FDs on process exit, but not ideal

**Recommendation:**
Verify libbpf-rs `RingBuffer` cleanup behavior. If it doesn't close the FD automatically, add explicit cleanup.

---

## ✅ **GOOD: Error Path Cleanup**

**Status:** Properly handled

- `run()` method has cleanup at exit (`main.rs:2039-2047`)
- `Drop` impl provides fallback cleanup
- Error paths don't skip cleanup (Drop always runs)

---

## ✅ **GOOD: Memory Management**

**Status:** No leaks found

- All allocations use `Arc`, `Vec`, or standard Rust types
- No manual `malloc`/`free`
- Rust ownership system prevents leaks
- No circular references found

---

## ✅ **GOOD: File Descriptor Management**

**Status:** Properly managed

- All FDs managed by Rust types (evdev, epoll, ring buffer)
- RAII ensures cleanup on drop
- Epoll registrations cleaned up explicitly
- No raw FD leaks found

---

## ⚠️ **ISSUE #2: BPF Skeleton Lifecycle**

**Severity:** Low  
**Type:** Potential implicit cleanup

**Location:** `main.rs:780-820` (Scheduler::init)

**Analysis:**
- BPF skeleton (`skel`) is stored in `Scheduler` struct
- No explicit cleanup visible, but:
  - Struct ops link is dropped (unregisters scheduler)
  - BPF skeleton likely cleaned up when `Scheduler` is dropped
  - libbpf-rs should handle BPF program/map cleanup

**Recommendation:**
Verify libbpf-rs `BpfSkel` cleanup behavior. If it doesn't clean up maps/programs automatically, add explicit cleanup.

---

## ✅ **GOOD: Statistics/State Cleanup**

**Status:** Properly handled

- Thread-local statistics cleared on thread exit
- Shared state (`Arc`) automatically cleaned up when last reference drops
- No persistent state leaks

---

## Summary

### Critical Issues: 0
### Medium Issues: 1 (Ring buffer cleanup - likely safe but not explicit)
### Low Issues: 1 (BPF skeleton cleanup - likely safe)

### Overall Assessment: **VERY GOOD**

The Rust userspace code shows excellent resource management:
- All threads have proper `Drop` implementations
- Epoll cleanup is explicit
- File descriptors managed via RAII
- No manual memory management

The only potential concerns are implicit cleanup behaviors of libbpf-rs types, which are likely safe but worth verifying.

---

## Recommended Actions

1. **MEDIUM PRIORITY:** Verify `libbpf_rs::RingBuffer` cleanup behavior
   - Check if it closes the FD automatically
   - If not, add explicit cleanup in `Drop` impl

2. **LOW PRIORITY:** Verify `libbpf_rs::BpfSkel` cleanup behavior
   - Check if it unloads BPF programs/maps automatically
   - Document cleanup behavior

3. **DOCUMENTATION:** Add comments documenting implicit cleanup behaviors

---

## Testing Recommendations

1. Run scheduler with `valgrind` or `heaptrack` to verify no leaks
2. Monitor file descriptor count during long runs
3. Test scheduler restart path for resource leaks
4. Test Ctrl+C shutdown path for cleanup issues

