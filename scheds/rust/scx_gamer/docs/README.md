# scx_gamer Documentation

This directory contains technical documentation for the scx_gamer scheduler, a Linux BPF scheduler designed for gaming workloads.

## Quick Navigation

### Getting Started
- **[../README.md](../README.md)** - Main readme with features and usage
- **[../QUICK_START.md](../QUICK_START.md)** - 3-step CachyOS installation
- **[../CACHYOS_INTEGRATION.md](../CACHYOS_INTEGRATION.md)** - Detailed CachyOS guide

### Architecture and Design
- **[../TECHNICAL_ARCHITECTURE.md](../TECHNICAL_ARCHITECTURE.md)** - Comprehensive technical implementation
- **[../CACHYOS_ARCHITECTURE.md](../CACHYOS_ARCHITECTURE.md)** - CachyOS integration architecture
- **[FENTRY_OPTIMIZATIONS.md](FENTRY_OPTIMIZATIONS.md)** - fentry/kprobe/uprobe design
- **[INTEGRATION_COMPLETE.md](INTEGRATION_COMPLETE.md)** - Advanced detection integration status

### Safety and Compatibility
- **[../ANTICHEAT_SAFETY.md](../ANTICHEAT_SAFETY.md)** - Anti-cheat compatibility analysis

### Machine Learning System
- **[ML_AUTOTUNE_GUIDE.md](ML_AUTOTUNE_GUIDE.md)** - Automated parameter tuning (recommended starting point)
- **[ML_README.md](ML_README.md)** - ML pipeline architecture and data collection
- **[ML_AUTOTUNE_NOTE.md](ML_AUTOTUNE_NOTE.md)** - Implementation notes on parameter hot-swapping

### Performance Analysis
- **[PERFORMANCE_ANALYSIS.md](PERFORMANCE_ANALYSIS.md)** - Detailed latency analysis and bottleneck identification
- **[PERFORMANCE_ANALYSIS_OPTIMIZATIONS.md](PERFORMANCE_ANALYSIS_OPTIMIZATIONS.md)** - Optimization strategies and results
- **[BPF_CODE_REVIEW.md](BPF_CODE_REVIEW.md)** - BPF verifier compliance and correctness validation
- **[CHANGELOG_OPTIMIZATIONS.md](CHANGELOG_OPTIMIZATIONS.md)** - Historical optimization changes

### Experimental Features
- **[THREAD_LEARNING.md](THREAD_LEARNING.md)** - Experimental thread pattern learning system (disabled in v1.0.2)

## Documentation Sections

### Core Features

**Game Detection**:
- BPF LSM hooks for kernel-level process tracking (<1ms latency)
- Fallback inotify mode for older kernels
- Wine/Proton/Steam detection
- See: [../TECHNICAL_ARCHITECTURE.md](../TECHNICAL_ARCHITECTURE.md#bpf-lsm-game-detection)

**Advanced Thread Classification**:
- GPU thread detection (fentry hooks on `drm_ioctl`)
- Wine thread priority tracking (uprobe on `NtSetInformationThread`)
- Runtime pattern analysis (sched_switch tracepoint)
- See: [FENTRY_OPTIMIZATIONS.md](FENTRY_OPTIMIZATIONS.md), [INTEGRATION_COMPLETE.md](INTEGRATION_COMPLETE.md)

**ML Auto-Tuning**:
- Bayesian optimization for per-game configs
- Grid search mode for exhaustive exploration
- Hot-reload parameters without restart
- See: [ML_AUTOTUNE_GUIDE.md](ML_AUTOTUNE_GUIDE.md)

**Scheduling**:
- Local-first (per-CPU DSQs) under light load
- Global EDF under heavy load
- Input window boost for low latency
- Migration limiting for cache locality
- See: [../TECHNICAL_ARCHITECTURE.md](../TECHNICAL_ARCHITECTURE.md#component-breakdown)

### Performance

**Overhead** (vs CFS):
- select_cpu(): +50-650ns (200-800ns total)
- enqueue(): +50-300ns (150-400ns total)
- Total CPU: +0.2-0.5%

**Detection Latency**:
- BPF LSM: <1ms
- GPU threads: <1ms (first ioctl)
- Input events: <500μs end-to-end

See: [PERFORMANCE_ANALYSIS.md](PERFORMANCE_ANALYSIS.md), [PERFORMANCE_ANALYSIS_OPTIMIZATIONS.md](PERFORMANCE_ANALYSIS_OPTIMIZATIONS.md)

### Safety

**Anti-Cheat Compatibility**:
- No game memory access
- No input manipulation
- No code injection
- Kernel-sanctioned APIs only
- See: [../ANTICHEAT_SAFETY.md](../ANTICHEAT_SAFETY.md)

**BPF Verifier**:
- Memory safety guarantees
- Bounded execution
- Read-only hooks
- See: [BPF_CODE_REVIEW.md](BPF_CODE_REVIEW.md)

## Research Focus Areas

- **Low-latency scheduling**: Sub-millisecond input-to-boost latency
- **ML-based optimization**: Zero-config per-game tuning via Bayesian optimization
- **BPF scheduler design**: Kernel-level detection and classification
- **Cache-aware placement**: mm-affinity, NUMA, SMT awareness
- **Thread classification**: Multi-source detection (GPU, Wine, runtime patterns)
- **AI-assisted development**: Evaluating AI capabilities in systems programming

## File Organization

```
docs/
├── README.md (this file)
├── ML_AUTOTUNE_GUIDE.md          # Start here for ML features
├── ML_README.md
├── ML_AUTOTUNE_NOTE.md
├── PERFORMANCE_ANALYSIS.md
├── PERFORMANCE_ANALYSIS_OPTIMIZATIONS.md
├── BPF_CODE_REVIEW.md
├── CHANGELOG_OPTIMIZATIONS.md
├── FENTRY_OPTIMIZATIONS.md       # Advanced detection design
├── INTEGRATION_COMPLETE.md       # Implementation status
└── THREAD_LEARNING.md

Root documentation:
../README.md                       # Main entry point
../TECHNICAL_ARCHITECTURE.md      # Comprehensive implementation guide
../ANTICHEAT_SAFETY.md            # Safety analysis
../CACHYOS_ARCHITECTURE.md        # CachyOS integration architecture
../CACHYOS_INTEGRATION.md         # CachyOS installation guide
../QUICK_START.md                 # 3-step quickstart
```

## Contributing to Documentation

When adding new features:
1. Update [../README.md](../README.md) with user-facing changes
2. Update [../TECHNICAL_ARCHITECTURE.md](../TECHNICAL_ARCHITECTURE.md) with implementation details
3. Add performance analysis to [PERFORMANCE_ANALYSIS.md](PERFORMANCE_ANALYSIS.md)
4. Document safety implications in [../ANTICHEAT_SAFETY.md](../ANTICHEAT_SAFETY.md) if relevant
5. Update this index (docs/README.md)

## Version

Documentation for scx_gamer v1.0.2 (2025-10-07)
