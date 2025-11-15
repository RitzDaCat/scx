## LLVM `objdump` for BPF: Methodology and Lessons Learned

**Date:** 2025-11-15  
**Context:** `scx_gamer` BPF verifier + performance optimization work  

---

### 1. What `llvm-objdump` is (and why we care for BPF)

`llvm-objdump` is the LLVM toolchain’s disassembler/inspector for object files.  
For BPF, it lets us:

- **Disassemble the compiled BPF object** (`.o`) and see *exact* instructions per function.
- **Interleave C source with assembly** to correlate hot-path lines with emitted instructions.
- **Inspect symbol tables and section sizes** to measure code size per function and per section.
- **See relocations** (map accesses, CO-RE field offsets, kfunc calls) and how they’re wired.

This fills a gap between:

- **The BPF verifier log**, which gives symbolic counts like:
  - “processed 497220 insns, 26108 states”
  - but does *not* tell us which function/loop is responsible.
- **Userspace-level profiling**, which sees end‑to‑end latency but cannot isolate specific BPF hot paths.

In `scx_gamer`, we used `llvm-objdump` to answer questions like:

- “How many instructions does `pick_idle_cpu_cached` actually execute per scan iteration?”  
- “Did replacing a map lookup with a BSS load really remove the helper call?”  
- “Is `gamer_enqueue_slowpath` dominated by map ops, kfunc calls, or simple arithmetic?”  

This in turn supported the claims in `BPF_PERFORMANCE_OPTIMIZATION_SUMMARY.md` about **nanosecond‑level savings** from specific transformations.

---

### 2. Where the BPF object lives in this project

The BPF program for `scx_gamer` is built by `build.rs` using `scx_cargo::BpfBuilder::enable_skel("src/bpf/main.bpf.c", "bpf")`.  
That generator writes a BPF object into Cargo’s build directory.

From the **top-level `scx` repo root**:

- **Debug build:**

```bash
cargo build -p scx_gamer
find target/debug/build -maxdepth 4 -path '*scx_gamer-*/out/bpf.bpf.o'
```

- **Release build:**

```bash
cargo build -p scx_gamer --release
find target/release/build -maxdepth 4 -path '*scx_gamer-*/out/bpf.bpf.o'
```

This yields a path like:

```text
target/debug/build/scx_gamer-7ae8cd73c0f9aa39/out/bpf.bpf.o
```

All `llvm-objdump` commands in this document assume that object path.

---

### 3. Basic `llvm-objdump` commands we used

#### 3.1. High-level disassembly with source interleaving

To see C source alongside assembly and relocations:

```bash
llvm-objdump -d -r -source \
  target/debug/build/scx_gamer-*/out/bpf.bpf.o \
  | less
```

- `-d`: disassemble each text section.
- `-r`: show relocations (maps, CO-RE, kfuncs).
- `-source`: interleave the original C source as comments.

This is the “big picture” view we used to:

- Confirm that specific hot-path lines (e.g., in `gamer_enqueue_slowpath`) produced or removed helper calls.
- Verify that BSS accesses (`hotpath_signals.input_ns[idx]`) compiled to plain loads/stores, not hidden helper calls.

#### 3.2. Function-level focused disassembly

For targeted inspection of a specific function:

```bash
llvm-objdump -d target/debug/build/scx_gamer-*/out/bpf.bpf.o \
  | sed -n '/pick_idle_cpu_cached/,/ret/p'
```

We used this to examine:

- `pick_idle_cpu_cached`
- `gamer_select_cpu_slowpath`
- `gamer_enqueue_slowpath`
- `task_dl_with_ctx_cached`

and to visually compare before/after instruction sequences around:

- CPU scanning loops.
- Wakeup-chain checks and dispatch paths.
- Cached flag checks vs previous map accesses.

#### 3.3. Symbol table and code size per function

To obtain code size per function:

```bash
llvm-objdump -t target/debug/build/scx_gamer-*/out/bpf.bpf.o \
  | grep ' F ' \
  | sort -k5,5
```

- This prints function symbols with their sizes.
- For BPF (fixed 8‑byte instructions), **size / 8 ≈ instruction count** for that function.

We used these symbol sizes to:

- Track the impact of refactors on `pick_idle_cpu_cached` and `gamer_select_cpu_slowpath`.
- Verify that some helpers (e.g., `load_preferred_cpu_safe`) remained small and stable.

---

### 4. Case studies from `scx_gamer`

#### 4.1. CPU selection: bounding `pick_idle_cpu_cached`

**Problem:**  
The original `pick_idle_cpu_cached` had:

- Manually unrolled iterations over `preferred_cpus[]`.
- A fallback loop up to `MAX_CPUS` with direct array access.
- More complex index arithmetic (`cpu_count - 1 - i`) in the tail scan.

The BPF verifier could handle it, but:

- The instruction count per iteration was higher than necessary.
- Bounds reasoning around `preferred_cpus[idx]` was fragile.

**Change:**  
We introduced:

```c
#define FRAME_PHYS_SCAN_MAX 64  /* Clamp scan length to avoid verifier blow-up */

static __noinline bool load_preferred_cpu_safe(u32 idx, s32 *out)
{
    if (idx >= MAX_CPUS)
        return false;

    *out = (s32)preferred_cpus[idx];
    return true;
}
```

and rewrote both:

- The unrolled iterations 0–3, and
- The fallback loop (`for i >= 4 && i < FRAME_PHYS_SCAN_MAX`)

to use `load_preferred_cpu_safe(idx, &candidate)` instead of direct array indexing.

**What `llvm-objdump` showed:**

- The body of `load_preferred_cpu_safe` compiled into a tiny, separate subprogram:
  - One compare (`idx >= MAX_CPUS`).
  - One load from `preferred_cpus`.
  - A couple of moves/branches.
- The main loop in `pick_idle_cpu_cached` became:
  - A simple loop with:
    - Call or inlined body of `load_preferred_cpu_safe`.
    - A small branch tree (`candidate < 0`, CPU mask test, CCD class check).
  - Bounded by `FRAME_PHYS_SCAN_MAX`, which we can treat as a small constant (e.g., 64).

**Interpretation:**

- Per-iteration instruction count dropped compared to the previous direct indexing and duplicate bounds checks.
- The verifier sees:
  - A small out-of-line helper for pointer arithmetic and bounds.
  - A bounded loop with clear progress (increment `i` before any `continue`).
- This matches the architectural goal:
  - Keep CPU selection fast and predictable.
  - Avoid verifier state explosion.

#### 4.2. Wakeup-chain hot path: from maps to `hotpath_signals`

**Problem:**  
The original wakeup-chain implementation used:

- `input_arrived_for_game` (BPF map) for input handler wake flags.
- `compositor_woke_for_game` (BPF map) for compositor → game signaling.

Each interaction involved:

- `bpf_map_lookup_elem()` or `bpf_map_update_elem()` helpers.
- Several instructions of key setup and pointer checks per use.

That’s acceptable in warm paths, but it is expensive in ultra‑hot enqueue paths.

**Change:**  
We moved to a shared `.bss` struct:

```c
struct hotpath_signals {
    volatile u64 input_ns[MAX_CPUS];   /* Per-target CPU timestamp for latest input wake */
    volatile u64 compositor_ns;        /* Last compositor wake timestamp */
};
struct hotpath_signals hotpath_signals SEC(".bss");
```

and replaced map ops with:

```c
/* On input event: */
if (fg_tgid != 0 && target_cpu >= 0 && target_cpu < MAX_CPUS)
    hotpath_signals.input_ns[(u32)target_cpu] = now;

/* In enqueue hot path: */
u64 input_time = hotpath_signals.input_ns[idx];
...
u64 compositor_time = hotpath_signals.compositor_ns;
...
hotpath_signals.compositor_ns = now;
```

**What `llvm-objdump` showed:**

- Map helper calls disappeared from those sites entirely.
- Each map lookup/update (~20–40 BPF instructions + helper) was replaced by:
  - A few instructions to compute the BSS offset (index scaling).
  - A direct load or store.

**Interpretation:**

- Replacing helper-based wake flags with `.bss`:
  - Removed both the helper call overhead and associated instruction sequences.
  - Reduced hot-path instruction count and complexity.
- The BPF performance documentation’s claim that this is a **Tier 0/1** operation (~20–50ns) vs a **Tier 3** map access (~100–300ns) is consistent with:
  - Known `bpf_map_*` helper costs from prior measurements.
  - The observed reduction in instructions and helper calls in `llvm-objdump`.

#### 4.3. Cached flags and timestamp reuse

We also used `llvm-objdump` to validate more subtle changes:

- Cached flags (`is_gpu_submit_cached`, `is_input_handler_cached`, etc.):
  - Verified that each check compiles to a **single load + bit test** instead of a map lookup.
- Timestamp reuse:
  - Confirmed that functions like `gamer_enqueue_slowpath` call `scx_bpf_now()` once at the top and re-use the value, not re-emit the helper multiple times.

These kinds of checks are simple to eyeball in disassembly:

- We explicitly looked for:
  - Absence of extra `call` instructions in hot regions.
  - Single `scx_bpf_now` usage per hot function.

---

### 5. How this complements the BPF verifier view

The BPF verifier’s log gives:

- **Total “insns processed”**: symbolic CP-like count over all states and paths.
- **State counts and peak states**: how big the state space is.

But it does *not* show:

- Which functions dominate instruction count.
- Where helpers or complex loops are concentrated.

Our workflow:

1. **Verifier first**:  
   Use the libbpf/BPF log to see that, e.g., `gamer_enqueue` processed ~497k symbolic instructions and hit many states.
2. **Disassemble with `llvm-objdump`**:  
   Inspect `gamer_enqueue_slowpath` and its callees to identify:
   - Large loops.
   - Heavy helper usage.
   - Complex branch trees.
3. **Refactor**:
   - Introduce bounded loops and small helpers (`load_preferred_cpu_safe`).
   - Replace hot-path maps with `.bss` (`hotpath_signals`).
   - Move heavy logic to cold paths and tail programs where appropriate.
4. **Re-check both**:
   - Verifier stats (insns and states).
   - `llvm-objdump` function sizes and instruction patterns.

This “verifier + `llvm-objdump`” two‑view approach was essential to avoid both:

- **Verifier blowup** (too many states, `-EINVAL`), and
- **Over‑optimistic assumptions** about performance that aren’t grounded in instruction counts.

---

### 6. Further exploration ideas

For peers who want to take this further:

- **Automated diffing**:
  - Capture `llvm-objdump -t` output before/after a change and diff function sizes.
  - Use simple scripts to flag functions whose size increased beyond a budget.
- **Helper-call counting**:
  - Grep disassembly for `call` to specific helpers (e.g., ring buffer, maps).
  - Quantify how many helper calls exist in a given hot function.
- **Micro-benchmarks**:
  - Combine instruction counts from `llvm-objdump` with known helper latencies to approximate nanosecond impact per hot path.
  - Validate with runtime measurements (e.g., using BPF histograms or userspace timers).
- **Cross-architecture analysis**:
  - For architectures with different BPF JITs, compare disassembly and JIT stats to see if certain patterns compile better than others.

The key lesson from `scx_gamer` is that **instruction-level visibility** via `llvm-objdump` made it possible to reason about hot paths in terms of tens of nanoseconds, not just milliseconds of frame time, and to align verifier‑friendliness with actual runtime performance. This was especially important for wakeup-chain optimization and CPU selection, where we are operating at the edge of what the BPF verifier and JIT can handle while still meeting aggressive latency goals.


