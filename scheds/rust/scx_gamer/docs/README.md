# scx_gamer Documentation Index

**Date:** 2025-10-29  
**Purpose:** Central index for all scx_gamer documentation  
**Framework:** Organized according to [Diátaxis](https://diataxis.fr/) principles

---

## Documentation Structure (Diátaxis Framework)

Our documentation is organized into four types, each serving a different purpose:

1. **Tutorials** - Learning-oriented, teach concepts step-by-step
2. **How-To Guides** - Goal-oriented, solve specific problems
3. **Reference** - Information-oriented, technical specifications
4. **Explanation** - Understanding-oriented, provide context and insights

---

## Tutorials (Learning-Oriented)

**Purpose:** Guide users through learning scx_gamer concepts step-by-step.

- **[QUICK_START.md](./QUICK_START.md)** - **Start here!** Get up and running in 3 steps
- **[INSTALLER_README.md](./INSTALLER_README.md)** - Detailed installation instructions with all methods

---

## How-To Guides (Goal-Oriented)

**Purpose:** Step-by-step instructions to accomplish specific tasks.

### Installation & Setup
- **[CACHYOS_INTEGRATION.md](./CACHYOS_INTEGRATION.md)** - Integrate scx_gamer with CachyOS GUI manager
- **[INSTALLER_README.md](./INSTALLER_README.md)** - Installation methods and configuration

### Performance Optimization
- **[INPUT_LATENCY_OPTIMIZATIONS.md](./INPUT_LATENCY_OPTIMIZATIONS.md)** - Optimize input latency for gaming
- **[FINAL_INPUT_LATENCY_REVIEW.md](./FINAL_INPUT_LATENCY_REVIEW.md)** - Fine-tune input responsiveness
- **[GPU_FRAME_PERFORMANCE_REVIEW.md](./GPU_FRAME_PERFORMANCE_REVIEW.md)** - Improve GPU and frame presentation

### Troubleshooting
- **[COMPILATION_VERIFICATION.md](./COMPILATION_VERIFICATION.md)** - Fix compilation issues
- **[DEAD_CODE_REVIEW.md](./DEAD_CODE_REVIEW.md)** - Clean up unused code

---

## Reference (Information-Oriented)

**Purpose:** Technical specifications, API details, and factual information.

### Architecture & Design
- **[TECHNICAL_ARCHITECTURE.md](./TECHNICAL_ARCHITECTURE.md)** - Complete system architecture and design
- **[RING_BUFFER_IMPLEMENTATION.md](./RING_BUFFER_IMPLEMENTATION.md)** - Ring buffer implementation details
- **[THREADS.md](./THREADS.md)** - Thread management and scheduling specifications
- **[CACHYOS_ARCHITECTURE.md](./CACHYOS_ARCHITECTURE.md)** - CachyOS-specific architecture details

### Performance & Metrics
- **[PERFORMANCE.md](./PERFORMANCE.md)** - Performance characteristics and benchmarks
- **[COMPREHENSIVE_PERFORMANCE_IMPACT_TABLE.md](./COMPREHENSIVE_PERFORMANCE_IMPACT_TABLE.md)** - Complete performance impact matrix
- **[OPTIMIZATION_IMPLEMENTATION_SUMMARY.md](./OPTIMIZATION_IMPLEMENTATION_SUMMARY.md)** - Optimization details and expected latency changes

### Code Reference
- **[CODE_SAFETY_REVIEW.md](./CODE_SAFETY_REVIEW.md)** - Code safety specifications and unsafe block analysis
- **[SAFETY_REVIEW.md](./SAFETY_REVIEW.md)** - Safety review of error handling and unsafe blocks
- **[INPUT_CHAIN_REVIEW.md](./INPUT_CHAIN_REVIEW.md)** - Input event processing chain specifications

### Platform & Integration
- **[ANTICHEAT_SAFETY.md](./ANTICHEAT_SAFETY.md)** - Anti-cheat safety specifications
- **[ML.md](./ML.md)** - Machine learning integration reference

---

## Explanation (Understanding-Oriented)

**Purpose:** Provide context, insights, and deeper understanding of why and how things work.

### Performance & Optimization Insights
- **[LMAX_PERFORMANCE_OPTIMIZATIONS.md](./LMAX_PERFORMANCE_OPTIMIZATIONS.md)** - Why and how LMAX Disruptor principles improve performance
- **[REALTIME_SCHEDULING_OPTIMIZATIONS.md](./REALTIME_SCHEDULING_OPTIMIZATIONS.md)** - Real-time scheduling algorithms and their application
- **[OPTIMIZATION_STATUS_AND_LEARNINGS.md](./OPTIMIZATION_STATUS_AND_LEARNINGS.md)** - Learnings from optimization research and implementation

### Latency & Timing Analysis
- **[LATENCY_CHAIN_ANALYSIS.md](./LATENCY_CHAIN_ANALYSIS.md)** - Understanding the end-to-end latency chain from game to display
- **[INPUT_CHAIN_REVIEW.md](./INPUT_CHAIN_REVIEW.md)** - How input events flow through the system
- **[RING_BUFFER_DIRECT_BOOST_EXPLAINED.md](./RING_BUFFER_DIRECT_BOOST_EXPLAINED.md)** - Why ring buffer direct boost reduces latency

### GPU & Frame Presentation
- **[PAGE_FLIP_VSYNC_MODE_ANALYSIS.md](./PAGE_FLIP_VSYNC_MODE_ANALYSIS.md)** - How page flip detection works with different VSync modes
- **[PAGE_FLIP_ANTICHEAT_SAFETY.md](./PAGE_FLIP_ANTICHEAT_SAFETY.md)** - Why page flip detection is anti-cheat safe
- **[GPU_FRAME_PERFORMANCE_REVIEW.md](./GPU_FRAME_PERFORMANCE_REVIEW.md)** - Understanding GPU and frame presentation optimization

### Code Quality & Reviews
- **[DEAD_CODE_REVIEW.md](./DEAD_CODE_REVIEW.md)** - Analysis of unused code and optimization opportunities
- **[COMPILATION_VERIFICATION.md](./COMPILATION_VERIFICATION.md)** - Build system and compilation insights

---

## Additional Documentation

### Changelogs
- **[CHANGELOG.md](./CHANGELOG.md)** - Project changelog
- **[CHANGELOG_LMAX_REALTIME_OPTIMIZATIONS.md](./CHANGELOG_LMAX_REALTIME_OPTIMIZATIONS.md)** - Detailed changelog for LMAX/Real-Time optimizations

### Legacy Categories (By Session)
- **LMAX/Real-Time Session:** OPTIMIZATION_*, LMAX_*, REALTIME_*, COMPREHENSIVE_*
- **Input Latency Session:** INPUT_*, FINAL_INPUT_*
- **GPU/Frame Session:** GPU_*, PAGE_FLIP_*, LATENCY_CHAIN_*
- **Code Review Session:** CODE_SAFETY_*, SAFETY_*, DEAD_CODE_*

---

## Document Statistics

**Total Documents:** 33+ markdown files  
**Total Size:** ~500KB+ of documentation  
**Coverage:** Architecture, Performance, Optimizations, Code Quality, Integration

---

## Quick Reference by Topic

**Want to understand performance impact?**
→ Start with `COMPREHENSIVE_PERFORMANCE_IMPACT_TABLE.md`

**Want to understand what was optimized?**
→ Read `OPTIMIZATION_IMPLEMENTATION_SUMMARY.md`

**Want to understand input latency?**
→ Read `INPUT_LATENCY_OPTIMIZATIONS.md` and `FINAL_INPUT_LATENCY_REVIEW.md`

**Want to understand GPU/frame performance?**
→ Read `GPU_FRAME_PERFORMANCE_REVIEW.md` and `LATENCY_CHAIN_ANALYSIS.md`

**Want to understand code safety?**
→ Read `CODE_SAFETY_REVIEW.md` and `SAFETY_REVIEW.md`

**Want to understand architecture?**
→ Read `TECHNICAL_ARCHITECTURE.md`

---

**Last Updated:** 2025-10-29
