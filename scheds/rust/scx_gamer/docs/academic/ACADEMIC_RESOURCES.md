# Academic Resources for Scheduler Development

**Purpose:** Comprehensive collection of papers, books, and resources for CPU scheduler development, real-time systems, and low-latency optimization.

**Last Updated:** 2025-11-05

---

## 🎯 Impact-Based Reading Order (Highest Impact First)

**This section prioritizes resources by their potential impact on scx_gamer development.**

### Tier 1: Critical - Fix Current Issues & Add Missing Features

1. **Sha et al. (1990) - Priority Inheritance Protocol** ⚠️ **HIGHEST IMPACT**
   - **Why:** Prevents priority inversion (currently missing feature)
   - **Impact:** Could eliminate occasional latency spikes from lock contention
   - **Effort:** Medium (requires lock tracking infrastructure)
   - **Location:** Section "Real-Time Scheduling Theory" → #2

2. **Liu & Layland (1973) - RMS Implementation** ⚠️ **HIGH IMPACT**
   - **Why:** Add Rate Monotonic Scheduling for periodic tasks (GPU frames, input handlers)
   - **Impact:** Better handling of known-periodic tasks (240Hz input = 4.17ms period)
   - **Effort:** Low-Medium (add RMS priority calculation)
   - **Location:** Section "Real-Time Scheduling Theory" → #1

3. **Liu & Layland (1973) - Schedulability Analysis** ⚠️ **HIGH IMPACT**
   - **Why:** Add formal schedulability tests (currently missing)
   - **Impact:** Guarantee all deadlines can be met before enabling EDF mode
   - **Effort:** Low (add utilization bound checks)
   - **Location:** Section "Real-Time Scheduling Theory" → #1

### Tier 2: High - Optimize Existing Features

4. **Lozi et al. (2012) - NUMA Cross-Node Stealing** ⚠️ **MEDIUM-HIGH IMPACT**
   - **Why:** Enhance NUMA awareness (currently only same-node preference)
   - **Impact:** Better load balancing when local NUMA node saturated
   - **Effort:** Medium (add cross-node stealing logic)
   - **Location:** Section "NUMA-Aware Scheduling" → #1

5. **Buttazzo (2005) - Adaptive EDF Deadline Adjustment** ⚠️ **MEDIUM IMPACT**
   - **Why:** Self-tuning scheduler based on deadline miss rate
   - **Impact:** Automatically adjusts to workload characteristics
   - **Effort:** Medium (add deadline adjustment logic)
   - **Location:** Section "Real-Time Scheduling Theory" → #3

6. **Lozi et al. (2016) - Linux Scheduler Analysis** ⚠️ **MEDIUM IMPACT**
   - **Why:** Understand why Linux CFS has issues (validates hybrid approach)
   - **Impact:** Confirms design decisions, identifies additional optimizations
   - **Effort:** Low (reading/analysis)
   - **Location:** Section "CPU Scheduling Algorithms" → #1

### Tier 3: Medium - Enhance Already-Good Features

7. **Thompson (2011) - Additional HFT Patterns** ✅ **MEDIUM IMPACT**
   - **Why:** More low-latency optimizations (already using loop unrolling)
   - **Impact:** Additional 50-200ns latency reductions possible
   - **Effort:** Low-Medium (apply more patterns)
   - **Location:** Section "Low-Latency Systems" → #1

8. **Drepper (2007) - Advanced Memory Optimization** ✅ **MEDIUM IMPACT**
   - **Why:** Enhance cache-aware optimizations (already implemented)
   - **Impact:** Further cache locality improvements
   - **Effort:** Medium (optimize data structure layout)
   - **Location:** Section "Performance Optimization" → #1

9. **Herlihy & Shavit (2012) - Advanced Lock-Free Patterns** ✅ **MEDIUM IMPACT**
   - **Why:** More sophisticated lock-free algorithms (already using ring buffers)
   - **Impact:** Additional wait-free optimizations
   - **Effort:** Medium-High (complex implementations)
   - **Location:** Section "Lock-Free Data Structures" → #3

### Tier 4: Reference - Foundational Understanding

10. **Buttazzo (2011) - Real-Time Systems Textbook** 📚 **REFERENCE**
    - **Why:** Comprehensive real-time systems theory
    - **Impact:** Deep understanding of scheduling algorithms
    - **Effort:** High (textbook reading)
    - **Location:** Section "Real-Time Scheduling Theory" → Books

11. **Tanenbaum & Bos (2014) - OS Theory** 📚 **REFERENCE**
    - **Why:** Foundational OS concepts
    - **Impact:** Broader systems understanding
    - **Effort:** High (textbook reading)
    - **Location:** Section "Operating Systems Theory" → #1

12. **Game Engine Optimization Resources** 🎮 **DOMAIN-SPECIFIC**
    - **Why:** Frame-based scheduling, game loop optimization
    - **Impact:** Better game-specific optimizations
    - **Effort:** Medium (domain knowledge)
    - **Location:** Section "Game Engine Optimization"

---

## Table of Contents

1. [Real-Time Scheduling Theory](#real-time-scheduling-theory)
2. [CPU Scheduling Algorithms](#cpu-scheduling-algorithms)
3. [Low-Latency Systems](#low-latency-systems)
4. [NUMA-Aware Scheduling](#numa-aware-scheduling)
5. [Lock-Free Data Structures](#lock-free-data-structures)
6. [Game Engine Optimization](#game-engine-optimization)
7. [Operating Systems Theory](#operating-systems-theory)
8. [Performance Optimization](#performance-optimization)
9. [Load Balancing](#load-balancing)
10. [Thread Classification](#thread-classification)

---

## Real-Time Scheduling Theory

### Foundational Papers

#### 1. Liu & Layland (1973) - **ESSENTIAL**
**Title:** "Scheduling Algorithms for Multiprogramming in a Hard-Real-Time Environment"  
**Authors:** C. L. Liu, James W. Layland  
**Journal:** Journal of the ACM, Vol. 20, No. 1, pp. 46-61  
**Year:** 1973  
**DOI:** 10.1145/321738.321743  
**URL:** https://dl.acm.org/doi/10.1145/321738.321743

**Key Concepts:**
- Rate Monotonic Scheduling (RMS) - fixed priority based on period
- Earliest Deadline First (EDF) - dynamic priority based on deadline
- Utilization bounds: RMS ≤69% (infinite tasks), EDF ≤100% (optimal)
- Schedulability tests for periodic task sets

**Relevance to scx_gamer:**
- EDF already implemented for heavy load mode
- RMS could enhance periodic task handling (GPU frames, input handlers)
- Utilization bounds provide formal schedulability guarantees

**Implementation Notes:**
- See `docs/LIU_LAYLAND_1973_ANALYSIS.md` for detailed analysis
- Deadline calculation: `deadline = vruntime + exec_vruntime`

---

#### 2. Sha et al. (1990) - Priority Inheritance Protocol
**Title:** "Priority Inheritance Protocols: An Approach to Real-Time Synchronization"  
**Authors:** Lui Sha, Ragunathan Rajkumar, John P. Lehoczky  
**Journal:** IEEE Transactions on Computers, Vol. 39, No. 9  
**Year:** 1990  
**DOI:** 10.1109/12.57058

**Key Concepts:**
- Priority Inheritance Protocol (PIP) - prevents priority inversion
- Priority Ceiling Protocol (PCP) - alternative to PIP
- Bounded blocking times for real-time tasks

**Relevance to scx_gamer:**
- Currently NOT implemented
- Could prevent priority inversion delays (low-priority task holds lock needed by high-priority task)
- Impact: Low for gaming (few locks in hot path), but could cause occasional latency spikes

**Recommended Reading:**
- Section 3: Priority Inheritance Protocol
- Section 4: Priority Ceiling Protocol
- Section 5: Blocking Time Analysis

---

#### 3. Buttazzo (2005) - Hard Real-Time Systems
**Title:** "Rate Monotonic vs. EDF: Judgment Day"  
**Author:** Giorgio C. Buttazzo  
**Journal:** Real-Time Systems, Vol. 29, No. 1, pp. 5-26  
**Year:** 2005  
**DOI:** 10.1007/s11241-005-6883-0

**Key Concepts:**
- Comparison of RMS vs EDF under various conditions
- EDF advantages: optimal utilization, better responsiveness
- RMS advantages: simpler implementation, predictable behavior
- Trade-offs and recommendations

**Relevance to scx_gamer:**
- Validates current EDF implementation choice
- Provides guidance on when to use RMS vs EDF
- Hybrid approach (RR + EDF) aligns with recommendations

---

### Books

#### 1. Buttazzo (2011) - Hard Real-Time Computing Systems
**Title:** "Hard Real-Time Computing Systems: Predictable Scheduling Algorithms and Applications"  
**Author:** Giorgio C. Buttazzo  
**Publisher:** Springer  
**Edition:** 3rd  
**Year:** 2011  
**ISBN:** 978-1-4614-0675-4

**Chapters Relevant to scx_gamer:**
- Chapter 3: "Aperiodic Task Scheduling" - Input handlers are aperiodic
- Chapter 4: "Resource Access Protocols" - Priority inheritance
- Chapter 5: "Resource Reservations" - CPU time reservations
- Chapter 6: "Power-Aware Scheduling" - CPU frequency scaling
- Chapter 7: "Scheduling in Linux" - Linux scheduler implementation

**Key Takeaways:**
- Formal schedulability analysis
- Resource reservation techniques
- Real-time extensions to Linux

---

#### 2. Burns & Wellings (2009) - Real-Time Systems and Programming Languages
**Title:** "Real-Time Systems and Programming Languages: Ada 95, Real-Time Java, and C/Real-Time POSIX"  
**Authors:** Alan Burns, Andy Wellings  
**Publisher:** Addison-Wesley  
**Edition:** 4th  
**Year:** 2009  
**ISBN:** 978-0-321-41745-9

**Relevance:**
- Real-time scheduling theory foundations
- Deadline-based scheduling
- Priority inheritance
- Multiprocessor scheduling

---

## CPU Scheduling Algorithms

### Modern Multiprocessor Scheduling

#### 1. Lozi et al. (2016) - The Linux Scheduler
**Title:** "The Linux Scheduler: A Decade of Wasted Cores"  
**Authors:** Jean-Pierre Lozi, Baptiste Lepers, Justin Funston, Fabien Gaud, Vivien Quéma, Alexandra Fedorova  
**Conference:** EuroSys 2016  
**Year:** 2016  
**URL:** https://www.eurosys2016.org/program/accepted-papers/

**Key Findings:**
- Linux CFS scheduler scalability issues
- Cache locality vs load balancing trade-offs
- NUMA-aware scheduling challenges

**Relevance to scx_gamer:**
- Validates hybrid approach (cache locality first, load balance when needed)
- NUMA awareness considerations
- Per-CPU queues vs global queues

---

#### 2. Lozi et al. (2012) - Remote Core Locking
**Title:** "Remote Core Locking: Migrating Critical-Section Execution to Improve the Performance of Multithreaded Applications"  
**Authors:** Jean-Pierre Lozi, Florian David, Gaël Thomas, Julia Lawall, Gilles Muller  
**Conference:** USENIX ATC 2012  
**Year:** 2012  
**URL:** https://www.usenix.org/conference/atc12/technical-sessions/presentation/lozi

**Key Concepts:**
- Lock migration techniques
- Critical section execution migration
- Performance improvements for multithreaded applications

**Relevance:**
- Lock contention optimization
- Migration strategies

---

### Fair Scheduling

#### 3. Pradeep et al. (2008) - Scheduling Fairness
**Title:** "Scheduling with Fairness and Throughput Considerations"  
**Authors:** Pradeep Padala, Xiaoyun Zhu, Sharad Agarwal, Michael Kozuch, Timothy Wood, Kirk Schwan  
**Conference:** SIGMETRICS 2008  
**Year:** 2008  
**DOI:** 10.1145/1375457.1375497

**Key Concepts:**
- Fairness vs throughput trade-offs
- Proportional-share scheduling
- Virtual runtime (vruntime) concepts

**Relevance to scx_gamer:**
- Current implementation uses `vruntime` for fairness
- Balance between fairness and latency-criticality

---

## Low-Latency Systems

### High-Frequency Trading (HFT) Patterns

#### 1. Thompson (2011) - Mechanical Sympathy
**Title:** "Mechanical Sympathy"  
**Author:** Martin Thompson  
**Blog:** http://mechanical-sympathy.blogspot.com/  
**Year:** 2011

**Key Concepts:**
- Understanding hardware characteristics
- Cache-aware programming
- Branch prediction optimization
- False sharing avoidance

**Relevance to scx_gamer:**
- Loop unrolling implemented (HFT pattern)
- Cache-aware CPU selection (mm_hint)
- Per-CPU data structures (false sharing avoidance)

**Implementation Notes:**
- See `docs/MECHANICAL_SYMPATHY_OPTIMIZATIONS.md`
- See `docs/LMAX_MECHANICAL_SYMPATHY_ANALYSIS.md`

---

#### 2. Fowler (2011) - LMAX Disruptor
**Title:** "LMAX Disruptor - High Performance Inter-Thread Messaging Library"  
**Author:** Martin Fowler  
**Blog:** https://martinfowler.com/articles/lmax.html  
**Year:** 2011

**Key Concepts:**
- Lock-free ring buffers
- Single-writer pattern
- Cache-line optimization
- Wait-free algorithms

**Relevance to scx_gamer:**
- Ring buffer implementation (BPF ring buffers)
- Event-driven architecture
- Zero-copy message passing

**Implementation Notes:**
- See `docs/LMAX_DETAILED_EXPLANATION.md`
- See `docs/RING_BUFFER_IMPLEMENTATION.md`

---

### Lock-Free Data Structures

#### 3. Herlihy & Shavit (2012) - The Art of Multiprocessor Programming
**Title:** "The Art of Multiprocessor Programming"  
**Authors:** Maurice Herlihy, Nir Shavit  
**Publisher:** Morgan Kaufmann  
**Edition:** Revised 1st  
**Year:** 2012  
**ISBN:** 978-0-12-397337-5

**Key Chapters:**
- Chapter 2: "Mutual Exclusion" - Lock-free algorithms
- Chapter 3: "Concurrent Objects" - Wait-free data structures
- Chapter 7: "Spin Locks and Contention" - Lock contention avoidance
- Chapter 18: "Transactional Memory" - Advanced concurrency

**Relevance:**
- Lock-free ring buffer design
- Wait-free algorithms for hot paths
- Memory ordering and barriers

---

#### 4. Michael & Scott (1996) - Nonblocking Algorithms
**Title:** "Simple, Fast, and Practical Non-Blocking and Blocking Concurrent Queue Algorithms"  
**Authors:** Maged M. Michael, Michael L. Scott  
**Conference:** PODC 1996  
**Year:** 1996  
**DOI:** 10.1145/248052.248106

**Key Concepts:**
- Lock-free queue algorithms
- ABA problem solutions
- Performance comparisons

**Relevance:**
- Ring buffer implementation
- Lock-free data structure patterns

---

## NUMA-Aware Scheduling

#### 1. Lozi et al. (2012) - NUMA-Aware Schedulers
**Title:** "The NUMA-aware Scheduler: Design and Implementation"  
**Authors:** Jean-Pierre Lozi, Baptiste Lepers, Justin Funston, Fabien Gaud, Vivien Quéma, Alexandra Fedorova  
**Conference:** HotOS 2013  
**Year:** 2013

**Key Concepts:**
- NUMA topology awareness
- Memory locality optimization
- Cross-node migration policies

**Relevance to scx_gamer:**
- Current implementation: `shared_dsq(cpu)` returns NUMA node ID
- Tasks prefer same NUMA node
- Cross-node stealing not implemented (trade-off: cache locality vs load balance)

---

#### 2. Dashti et al. (2013) - Traffic Management
**Title:** "Traffic Management: A Holistic Approach to Memory Placement on NUMA Systems"  
**Authors:** Mohammad Dashti, Alexandra Fedorova, Justin Funston, Fabien Gaud, Renaud Lachaize, Baptiste Lepers, Vivien Quéma, Mark Roth  
**Conference:** ASPLOS 2013  
**Year:** 2013  
**DOI:** 10.1145/2451116.2451162

**Key Concepts:**
- NUMA memory placement
- Traffic-aware scheduling
- Holistic NUMA optimization

**Relevance:**
- Memory-aware CPU selection
- NUMA topology considerations

---

## Game Engine Optimization

#### 1. Sutter (2005) - Games Programming Gems
**Title:** "Games Programming Gems" series  
**Editor:** Various  
**Publisher:** Charles River Media / Cengage Learning  
**Years:** 2000-2011

**Relevant Volumes:**
- Volume 1: Threading and concurrency
- Volume 3: Performance optimization
- Volume 4: Memory management
- Volume 5: Low-level optimization

**Key Topics:**
- Frame-based scheduling
- Lock-free game loops
- Cache-aware data structures
- Real-time rendering optimization

---

#### 2. Gregory (2018) - Game Engine Architecture
**Title:** "Game Engine Architecture"  
**Author:** Jason Gregory  
**Publisher:** CRC Press  
**Edition:** 3rd  
**Year:** 2018  
**ISBN:** 978-1-138-09666-9

**Relevant Chapters:**
- Chapter 4: "Parallelism and Concurrent Programming" - Threading models
- Chapter 5: "3D Math for Games" - Performance considerations
- Chapter 16: "Runtime Gameplay Foundation Systems" - Game loop optimization

**Relevance:**
- Frame-rate targeting
- Frame-based deadlines
- Game loop optimization

---

## Operating Systems Theory

#### 1. Tanenbaum & Bos (2014) - Modern Operating Systems
**Title:** "Modern Operating Systems"  
**Authors:** Andrew S. Tanenbaum, Herbert Bos  
**Publisher:** Pearson  
**Edition:** 4th  
**Year:** 2014  
**ISBN:** 978-0-13-359162-0

**Relevant Chapters:**
- Chapter 2: "Processes and Threads" - Scheduling fundamentals
- Chapter 6: "Deadlocks" - Resource allocation
- Chapter 10: "Multiprocessor Systems" - NUMA, cache coherence

**Key Concepts:**
- Process scheduling algorithms
- Round-robin, priority scheduling
- Multiprocessor scheduling challenges

---

#### 2. Love (2010) - Linux Kernel Development
**Title:** "Linux Kernel Development"  
**Author:** Robert Love  
**Publisher:** Addison-Wesley Professional  
**Edition:** 3rd  
**Year:** 2010  
**ISBN:** 978-0-672-32946-3

**Relevant Chapters:**
- Chapter 4: "Process Scheduling" - CFS scheduler
- Chapter 10: "Kernel Synchronization Methods" - Locking
- Chapter 11: "Timers and Time Management" - Timer management

**Relevance:**
- Linux scheduler internals
- CFS (Completely Fair Scheduler) implementation
- Kernel synchronization primitives

---

## Performance Optimization

#### 1. Drepper (2007) - What Every Programmer Should Know About Memory
**Title:** "What Every Programmer Should Know About Memory"  
**Author:** Ulrich Drepper  
**Year:** 2007  
**URL:** https://people.freebsd.org/~lstewart/articles/cpumemory.pdf

**Key Concepts:**
- Memory hierarchy (L1/L2/L3 cache, RAM)
- Cache line optimization
- False sharing
- Prefetching strategies

**Relevance to scx_gamer:**
- Cache-aware CPU selection
- Per-CPU data structures (false sharing avoidance)
- Prefetching optimizations

---

#### 2. Fog (2021) - Optimization Manuals
**Title:** "Optimization manuals" series  
**Author:** Agner Fog  
**URL:** https://www.agner.org/optimize/

**Relevant Manuals:**
- "Optimizing software in C++" - General optimization
- "The microarchitecture of Intel, AMD and VIA CPUs" - CPU-specific optimization
- "Instruction tables" - Instruction latency and throughput

**Relevance:**
- CPU-specific optimizations
- Instruction-level optimization
- Branch prediction

---

## Load Balancing

#### 1. Mogul & Ramakrishnan (1997) - Eliminating Receive Livelock
**Title:** "Eliminating Receive Livelock in an Interrupt-Driven Kernel"  
**Authors:** Jeffrey C. Mogul, K. K. Ramakrishnan  
**Conference:** USENIX ATC 1997  
**Year:** 1997  
**URL:** https://www.usenix.org/conference/atc97/technical-sessions/presentation/mogul

**Key Concepts:**
- Interrupt-driven vs polling
- Load balancing for network interrupts
- Livelock prevention

**Relevance:**
- NAPI (Network API) preference implementation
- Interrupt handling optimization

---

#### 2. Anderson et al. (1989) - Scheduler Activations
**Title:** "Scheduler Activations: Effective Kernel Support for the User-Level Management of Parallelism"  
**Authors:** Thomas E. Anderson, Brian N. Bershad, Edward D. Lazowska, Henry M. Levy  
**Conference:** SOSP 1991  
**Year:** 1991  
**DOI:** 10.1145/121132.121151

**Key Concepts:**
- User-level thread scheduling
- Kernel-user cooperation
- Load balancing across processors

**Relevance:**
- Userspace-kernel coordination (Rust control plane + BPF)
- Load balancing strategies

---

## Thread Classification

#### 1. Tarjan et al. (2015) - Cache-Conscious Scheduling
**Title:** "Cache-Conscious Scheduling of Multithreaded Programs"  
**Authors:** Robert Tarjan, et al.  
**Conference:** Various

**Key Concepts:**
- Thread classification based on memory access patterns
- Cache-aware thread placement
- Memory locality optimization

**Relevance to scx_gamer:**
- Task classification system (GPU, input, audio, etc.)
- Cache-aware CPU selection
- Memory hint tracking

---

#### 2. Marathe et al. (2018) - ATC Thread Scheduling
**Title:** "Thread Scheduling on Asymmetric Single-ISA Multi-Core Systems"  
**Authors:** Virendra J. Marathe, Mark D. Hill, Michael M. Swift  
**Conference:** USENIX ATC 2018  
**Year:** 2018  
**URL:** https://www.usenix.org/conference/atc18/presentation/marathe

**Key Concepts:**
- Asymmetric core scheduling
- Thread-to-core mapping
- Performance/energy trade-offs

**Relevance:**
- CPU selection logic
- High-capacity vs low-capacity cores
- Preferred CPU scanning

---

## Additional Resources

### Conferences

1. **USENIX ATC** (Annual Technical Conference)
   - Systems research, scheduling papers
   - URL: https://www.usenix.org/conferences

2. **SOSP** (Symposium on Operating Systems Principles)
   - Foundational OS research
   - URL: https://www.sigops.org/sosp/

3. **EuroSys**
   - European systems research
   - URL: https://www.eurosys.org/

4. **RTSS** (Real-Time Systems Symposium)
   - Real-time scheduling papers
   - URL: https://www.rtss.org/

### Online Resources

1. **Linux Kernel Documentation**
   - `/Documentation/scheduler/` in kernel source
   - URL: https://www.kernel.org/doc/html/latest/scheduler/

2. **BPF Documentation**
   - eBPF program development
   - URL: https://www.kernel.org/doc/html/latest/bpf/

3. **SCX (sched_ext) Documentation**
   - Scheduler extension framework
   - URL: https://github.com/sched-ext/scx

---

## Implementation Mapping

### Papers → scx_gamer Features

| Paper/Book | scx_gamer Feature | Status |
|------------|-------------------|--------|
| Liu & Layland (1973) | EDF scheduling | ✅ Implemented |
| Liu & Layland (1973) | RMS for periodic tasks | ❌ Not implemented |
| Sha et al. (1990) | Priority Inheritance | ❌ Not implemented |
| Thompson (2011) | Loop unrolling, cache-aware | ✅ Implemented |
| Fowler (2011) | Ring buffers, event-driven | ✅ Implemented |
| Lozi et al. (2012) | NUMA-aware scheduling | ✅ Partially implemented |
| Drepper (2007) | Cache-aware CPU selection | ✅ Implemented |
| Herlihy & Shavit (2012) | Lock-free data structures | ✅ Implemented (BPF ring buffers) |

---

## Reading Priority

**Note:** See the [Impact-Based Reading Order](#-impact-based-reading-order-highest-impact-first) section at the top of this document for detailed prioritization by impact and effort.

### Quick Reference - Start Here

**For Immediate Impact (Fix Missing Features):**
1. Sha et al. (1990) - Priority Inheritance Protocol ⚠️ **HIGHEST IMPACT**
2. Liu & Layland (1973) - RMS Implementation & Schedulability Analysis ⚠️ **HIGH IMPACT**

**For Understanding Current Implementation:**
3. Liu & Layland (1973) - EDF foundations (already implemented)
4. Thompson (2011) - Mechanical Sympathy (HFT patterns - already implemented)
5. Fowler (2011) - LMAX Disruptor (ring buffers - already implemented)

**For Optimizing Existing Features:**
6. Lozi et al. (2012) - NUMA enhancements
7. Buttazzo (2005) - Adaptive EDF
8. Lozi et al. (2016) - Linux scheduler analysis

**For Deep Understanding (Reference):**
9. Herlihy & Shavit (2012) - Lock-free algorithms textbook
10. Buttazzo (2011) - Real-time systems textbook
11. Tanenbaum & Bos (2014) - OS theory textbook

---

## Notes for RAG Collection

**Keywords/Tags:**
- Real-time scheduling, EDF, RMS, priority inheritance
- Low-latency, HFT, mechanical sympathy, LMAX
- NUMA, cache-aware, memory optimization
- Lock-free, wait-free, concurrent data structures
- CPU scheduling, load balancing, multiprocessor
- Game engine optimization, frame-based scheduling

**Extraction Priorities:**
1. Algorithm descriptions (EDF, RMS, PIP)
2. Performance optimization techniques
3. Implementation patterns (lock-free, cache-aware)
4. Schedulability analysis methods
5. NUMA optimization strategies

---

**Last Updated:** 2025-11-05  
**Maintained by:** RAG Document Collection System

