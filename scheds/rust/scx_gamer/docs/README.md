# scx_gamer Documentation

## Overview

This directory contains comprehensive documentation for the scx_gamer scheduler, organized for clarity and ease of use.

## Core Documentation

### Installation & Setup
- **[QUICK_START.md](QUICK_START.md)** - 3-step CachyOS installation
- **[CACHYOS_INTEGRATION.md](CACHYOS_INTEGRATION.md)** - Detailed CachyOS guide
- **[CACHYOS_ARCHITECTURE.md](CACHYOS_ARCHITECTURE.md)** - CachyOS integration architecture

### Technical Documentation
- **[TECHNICAL_ARCHITECTURE.md](TECHNICAL_ARCHITECTURE.md)** - Comprehensive technical implementation
- **[ANTICHEAT_SAFETY.md](ANTICHEAT_SAFETY.md)** - Anti-cheat compatibility analysis
- **[CHANGELOG.md](CHANGELOG.md)** - Version history and changes

### Performance & Optimization
- **[PERFORMANCE.md](PERFORMANCE.md)** - Performance analysis and optimization guide
- **[THREADS.md](THREADS.md)** - Thread detection and classification
- **[ML.md](ML.md)** - Machine learning autotune guide

## Documentation Structure

```
docs/
├── README.md                    # This file
├── QUICK_START.md               # Quick installation guide
├── CACHYOS_INTEGRATION.md       # CachyOS installation
├── CACHYOS_ARCHITECTURE.md      # CachyOS architecture
├── TECHNICAL_ARCHITECTURE.md    # Technical implementation
├── ANTICHEAT_SAFETY.md          # Safety analysis
├── CHANGELOG.md                 # Version history
├── PERFORMANCE.md               # Performance analysis
├── THREADS.md                   # Thread detection
└── ML.md                        # ML autotune guide
```

## Quick Reference

### For Users
1. Installation: Start with [QUICK_START.md](QUICK_START.md)
2. Performance: Check [PERFORMANCE.md](PERFORMANCE.md) for optimization
3. Safety: Review [ANTICHEAT_SAFETY.md](ANTICHEAT_SAFETY.md) for compatibility

### For Developers
1. Architecture: Read [TECHNICAL_ARCHITECTURE.md](TECHNICAL_ARCHITECTURE.md)
2. Thread Detection: Study [THREADS.md](THREADS.md) for implementation details
3. ML Integration: Explore [ML.md](ML.md) for autotune features

### For Contributors
1. Changes: Check [CHANGELOG.md](CHANGELOG.md) for recent updates
2. Performance: Review [PERFORMANCE.md](PERFORMANCE.md) for optimization history
3. Threads: Understand [THREADS.md](THREADS.md) for detection mechanisms

## Key Features

### Performance Optimizations
- Input latency: ~50-100ns per event (hot path optimizations)
- Memory usage: ~75% reduction in DeviceInfo storage
- CPU efficiency: Improved cache utilization and reduced contention
- Gaming performance: Smoother input handling and better responsiveness

### Thread Detection
- BPF hooks: Ultra-low latency detection using eBPF
- Pattern learning: Automatic thread role identification
- Game-specific: Optimized detection for popular games
- Performance impact: <200ns per thread operation

### Machine Learning
- Autotune: Automated parameter optimization
- Bayesian optimization: Faster convergence to optimal parameters
- Performance validation: Comprehensive testing and validation
- Game-specific tuning: Optimal configuration per game

## Getting Started

### Installation
```bash
# Quick start (CachyOS)
git clone https://github.com/RitzDaCat/scx.git
cd scx/scheds/rust/scx_gamer
./start.sh
```

### Basic Usage
```bash
# Standard mode
sudo ./start.sh

# Verbose mode
sudo ./start.sh --verbose

# Ultra-latency mode
sudo ./start.sh --busy-polling --input-window-us 2000
```

### Advanced Usage
```bash
# ML autotune
sudo ./start.sh --ml-autotune --ml-autotune-trial-duration 120

# Thread detection
sudo ./start.sh --thread-detection-all

# Performance monitoring
sudo ./start.sh --verbose --stats-server
```

## Performance Monitoring

### Ring Buffer Statistics
```
RING_BUFFER: Input events processed: 1250, batches: 45, avg_events_per_batch: 27.8
latency: avg=45.2ns min=30ns max=60ns p50=42.1ns p95=55.3ns p99=58.7ns
```

### Scheduler Performance
```
SCHEDULER: select_cpu() latency: avg=650ns min=350ns max=800ns
Fast path: 60% of calls, Slow path: 30% of calls
```

## Troubleshooting

### Common Issues
1. **Compilation errors**: Check kernel version and BPF support
2. **Performance issues**: Review [PERFORMANCE.md](PERFORMANCE.md)
3. **Thread detection**: Check [THREADS.md](THREADS.md) for debugging
4. **ML autotune**: See [ML.md](ML.md) for troubleshooting

### Debug Mode
```bash
# Enable comprehensive debugging
sudo ./start.sh --verbose --debug --bpf-debug
```

## Contributing

### Documentation Updates
1. Update relevant documentation files
2. Update [CHANGELOG.md](CHANGELOG.md) with changes
3. Test documentation accuracy
4. Submit pull request

### Code Changes
1. Follow existing code style
2. Add appropriate documentation
3. Update [CHANGELOG.md](CHANGELOG.md)
4. Test thoroughly
5. Submit pull request

## License

This project is licensed under the GPL-2.0-only license.

## Support

For issues and questions:
- GitHub Issues: [Create an issue](https://github.com/RitzDaCat/scx/issues)
- Documentation: Check relevant documentation files
- Performance: Review [PERFORMANCE.md](PERFORMANCE.md)
- Safety: Check [ANTICHEAT_SAFETY.md](ANTICHEAT_SAFETY.md)