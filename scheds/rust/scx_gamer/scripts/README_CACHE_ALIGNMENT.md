# Cache Line Alignment Analysis Tools

## Overview

This directory contains tools for analyzing 64-byte cache line alignment in BPF structs to prevent:
- **False sharing**: Multiple CPUs invalidating each other's cache lines
- **Memory bus locking**: Fields straddling cache lines can lock the entire memory bus
- **Cache line bouncing**: Performance degradation from unnecessary cache synchronization

## Tools

### 1. `pahole` (Recommended - Analyzes Actual Compiled Code)

`pahole` reads DWARF debug information from compiled binaries to show the **actual** struct layout as the compiler generated it.

#### Installation
```bash
# Arch Linux
sudo pacman -S pahole

# Debian/Ubuntu  
sudo apt install dwarves
```

####Usage for BPF Structs

Since BPF code is compiled separately, you need to analyze the BPF object file:

```bash
# Step 1: Compile BPF code with debug symbols
cd src/bpf
clang -g -O2 -target bpf -c main.bpf.c -o main.bpf.o \
  -I. -I./include -I/usr/include

# Step 2: Analyze structs with pahole
pahole -C task_ctx main.bpf.o
pahole -C cpu_ctx main.bpf.o
pahole -C hot_path_cache main.bpf.o

# Step 3: Show cache line boundaries (hex offsets)
pahole --hex -C task_ctx main.bpf.o

# Step 4: Show holes/padding
pahole --holes -C task_ctx main.bpf.o

# Step 5: Check for cache line straddling
pahole -C task_ctx main.bpf.o | grep -E "\/\* +[0-9]+ +64 \*\/"
# Fields at byte offset 64, 128, 192, 256, 320, 384 cross cache lines
```

#### Key `pahole` Options
- `-C <struct>`: Show specific struct
- `--hex`: Show offsets in hexadecimal
- `--holes`: Highlight padding/holes
- `-E`: Expand type definitions
- `--reorganize`: Suggest optimized layout

### 2. `cache_line_analyzer.py` (Python Static Analyzer)

Analyzes struct definitions from source code (doesn't require compilation).

```bash
python3 scripts/cache_line_analyzer.py
```

**Pros**: Quick, no compilation needed  
**Cons**: May not match actual compiler layout (padding, alignment)

### 3. `analyze_cache_lines.sh` (Automated pahole Wrapper)

Automated script that analyzes all structs.

```bash
./scripts/analyze_cache_lines.sh
```

## Cache Line Alignment Rules

### Critical Structs (Must Be Aligned)
- `task_ctx`: **384 bytes** (6 cache lines) - accessed millions of times/sec
- `cpu_ctx`: **128 bytes** (2 cache lines) - per-CPU hot data
- `hot_path_cache`: **32 bytes** (½ cache line) - stack-allocated, sub-cache-line OK

###Why This Matters

```c
// ❌ BAD: 352 bytes - straddles into 6th cache line
struct task_ctx {
    u64 field1;  // bytes 0-7 (cache line 0)
    // ... 344 more bytes ...
    u64 last_field;  // bytes 344-351 (still in cache line 5)
    // Total: 352 bytes spans 5.5 cache lines
    // Next struct starts at byte 352 (middle of cache line 5)
    // → Both structs share cache line 5 → FALSE SHARING!
};

// ✓ GOOD: 384 bytes - exactly 6 cache lines
struct task_ctx {
    u64 field1;
    // ... fields ...
    u64 last_field;
    u8 _cache_line_padding[32];  // Pad to 384 bytes
    // Total: 384 bytes = exactly 6 cache lines
    // Next struct starts at byte 384 (cache line 6 boundary)
    // → No sharing!
};
```

### Performance Impact

| Issue | Impact | Frequency |
|-------|--------|-----------|
| **Cache line straddling** | 2× memory bus transactions | Every field access |
| **False sharing** | ~100-200ns cache coherency overhead | Per concurrent access |
| **Memory bus locking** | System-wide memory stall | On atomic operations |

**Example**: On a 32-core system @ 1M scheduling decisions/sec:
- Unaligned: ~3.2-6.4ms/sec wasted on false sharing
- Aligned: ~0ns overhead

## Verification Commands

```bash
# Quick check: Are structs aligned?
pahole -C task_ctx main.bpf.o | grep sizeof
# Should show: sizeof: 384 (6 × 64)

# Detailed check: Show cache line boundaries
pahole --hex -C task_ctx main.bpf.o | awk '
  /\/\* *0x[0-9a-f]+ / {
    offset = strtonum("0x" substr($2, 3));
    if (offset % 64 == 0) print "--- CACHE LINE", offset/64, "---";
    print;
  }
'

# Find fields that cross cache line boundaries
pahole -C task_ctx main.bpf.o | awk '
  /\/\* *[0-9]+ / {
    offset = $2;
    size = $3;
    line_start = int(offset / 64);
    line_end = int((offset + size - 1) / 64);
    if (line_start != line_end) 
      print "⚠️  STRADDLE:", $0;
  }
'
```

##Static Assertions in Code

The code includes compile-time checks:

```c
_Static_assert(sizeof(struct task_ctx) % 64 == 0, 
               "task_ctx must be cache-line aligned");
_Static_assert(sizeof(struct cpu_ctx) % 64 == 0,
               "cpu_ctx must be cache-line aligned");
```

If these fail during compilation:
1. Add explicit padding at end of struct
2. Verify with `pahole` after rebuild
3. Update documentation

## AMD Ryzen 9 9800X3D Specific Notes

Your CPU has:
- **L1 Data Cache**: 32 KiB per core, 64-byte lines
- **L2 Cache**: 1 MiB per core, 64-byte lines  
- **L3 Cache (3D V-Cache)**: 96 MiB shared, 64-byte lines

**Critical**:
- Cache line size: **64 bytes** (confirmed)
- False sharing penalty: **~100-200ns** (L3 cache latency)
- Memory bus contention: Can stall all 16 cores

## References

- [LMAX Disruptor: Mechanical Sympathy](https://mechanical-sympathy.blogspot.com/2011/07/false-sharing.html)
- [Intel® 64 and IA-32 Architectures Optimization Reference Manual](https://www.intel.com/content/www/us/en/architecture-and-technology/64-ia-32-architectures-optimization-manual.html)
- [Linux Kernel: Cache Line Padding](https://www.kernel.org/doc/html/latest/core-api/cachetlb.html)
- [pahole Documentation](https://man7.org/linux/man-pages/man1/pahole.1.html)

