# Performance Hierarchy Pass 2 Review

**Date:** 2025-11-05  
**Status:** Analysis Complete - Critical Issues Identified

---

## Critical Finding: Redundant Operations in Hot Path

After second pass review, identified **3 critical optimizations** that violate performance hierarchy principles:

---

## Issue 1: Redundant `scx_bpf_now()` Call in `preload_hot_path_data()` ⚠️ **CRITICAL**

**Location:** `preload_hot_path_data()` line 686  
**Issue:** Function calls `scx_bpf_now()` even though `select_cpu()` already has `now` from line 2852

**Current Code:**
```c
/* select_cpu() line 2852 */
u64 now = scx_bpf_now();

/* ... fast paths that reuse 'now' ... */

/* select_cpu() line 3096 */
preload_hot_path_data(p, prev_cpu, &cache);
  /* Inside preload_hot_path_data() line 686 */
  cache->now = scx_bpf_now();  /* ⚠️ REDUNDANT - 'now' already available! */
```

**Impact:** ~5-10ns wasted per `select_cpu()` call  
**Tier:** Tier 1 → Tier 1 (avoidable cost)  
**Fix:** Pass `now` as parameter to `preload_hot_path_data()`

---

## Issue 2: Redundant Map Lookups in `preload_hot_path_data()` ⚠️ **HIGH**

**Location:** `preload_hot_path_data()` lines 682-683  
**Issue:** Function performs map lookups even when `tctx`/`cctx` already loaded

**Current Code:**
```c
/* select_cpu() line 2887 (GPU fast path) */
tctx = try_lookup_task_ctx(p);  /* Already loaded! */

/* select_cpu() line 3096 */
preload_hot_path_data(p, prev_cpu, &cache);
  /* Inside preload_hot_path_data() line 682 */
  cache->tctx = try_lookup_task_ctx(p);  /* ⚠️ REDUNDANT - already loaded! */
```

**Impact:** ~20-50ns wasted when fast paths load `tctx`  
**Tier:** Tier 2 (Per-CPU/Task storage)  
**Fix:** Pass `tctx`/`cctx` as optional parameters, only lookup if NULL

---

## Issue 3: Redundant `get_fg_tgid()` Call ⚠️ **MEDIUM**

**Location:** `preload_hot_path_data()` line 687  
**Issue:** `get_fg_tgid()` reads volatile globals, but value rarely changes

**Current Code:**
```c
/* preload_hot_path_data() line 687 */
cache->fg_tgid = get_fg_tgid();  /* Reads volatile globals */

/* But get_fg_tgid() is called MULTIPLE times in same function */
```

**Impact:** ~1-3ns per call (minor, but accumulates)  
**Tier:** Tier 1 (volatile read)  
**Fix:** Cache `fg_tgid` in per-CPU context (updated only when game changes)

---

## Issue 4: Redundant `bpf_get_current_task_btf()` Call ⚠️ **MEDIUM**

**Location:** `select_cpu()` line 3086  
**Issue:** `current` task loaded even when not needed for fast paths

**Current Code:**
```c
/* select_cpu() line 3086 */
const struct task_struct *current = (void *)bpf_get_current_task_btf();

/* But 'current' is only used in sync wake path (line 3114) */
/* Most wakeups don't take sync wake path, so this is wasted */
```

**Impact:** ~3-10ns wasted per call (minor)  
**Tier:** Tier 1 (task_struct read)  
**Fix:** Defer `current` loading until sync wake path

---

## Issue 5: Volatile Global Access Pattern ⚠️ **INFORMATIONAL**

**Locations:** Multiple volatile global reads  
**Issue:** Volatile globals (`input_until_global`, `detected_fg_tgid`, `cpu_util_avg`) are accessed frequently

**Analysis:**
- **Tier:** Tier 1 (memory read, not map lookup)
- **Impact:** ~1-3ns per read (acceptable)
- **Status:** ✅ **Already optimal** - Volatile reads are fastest way to share data

**Note:** These are already Tier 1 operations, but could be cached in per-CPU context if needed.

---

## Recommended Optimizations

### Priority 1: Eliminate Redundant `scx_bpf_now()` (Critical)

**Change:** Pass `now` as parameter to `preload_hot_path_data()`

```c
/* BEFORE */
static __always_inline void preload_hot_path_data(
	struct task_struct *p,
	s32 cpu,
	struct hot_path_cache *cache)
{
	cache->now = scx_bpf_now();  /* REDUNDANT */
	/* ... */
}

/* AFTER */
static __always_inline void preload_hot_path_data(
	struct task_struct *p,
	s32 cpu,
	u64 now,  /* Pass timestamp from caller */
	struct hot_path_cache *cache)
{
	cache->now = now;  /* Reuse caller's timestamp */
	/* ... */
}
```

**Impact:** ~5-10ns saved per `select_cpu()` call  
**Risk:** Low - Simple parameter addition

---

### Priority 2: Eliminate Redundant Map Lookups (High)

**Change:** Pass `tctx`/`cctx` as optional parameters

```c
/* BEFORE */
static __always_inline void preload_hot_path_data(
	struct task_struct *p,
	s32 cpu,
	struct hot_path_cache *cache)
{
	cache->tctx = try_lookup_task_ctx(p);  /* Always lookup */
	cache->cctx = try_lookup_cpu_ctx(cpu);  /* Always lookup */
	/* ... */
}

/* AFTER */
static __always_inline void preload_hot_path_data(
	struct task_struct *p,
	s32 cpu,
	u64 now,
	struct task_ctx *tctx_opt,  /* Optional - NULL = lookup */
	struct cpu_ctx *cctx_opt,    /* Optional - NULL = lookup */
	struct hot_path_cache *cache)
{
	cache->tctx = tctx_opt ? tctx_opt : try_lookup_task_ctx(p);
	cache->cctx = cctx_opt ? cctx_opt : try_lookup_cpu_ctx(cpu);
	cache->now = now;
	/* ... */
}
```

**Impact:** ~20-50ns saved when fast paths already loaded context  
**Risk:** Low - Backward compatible (NULL = lookup)

---

### Priority 3: Defer `current` Task Loading (Medium)

**Change:** Only load `current` when sync wake path taken

```c
/* BEFORE */
const struct task_struct *current = (void *)bpf_get_current_task_btf();
/* ... */
if (sync_wake_path) {
    /* Use current */
}

/* AFTER */
/* ... */
if (sync_wake_path) {
    const struct task_struct *current = (void *)bpf_get_current_task_btf();
    /* Use current */
}
```

**Impact:** ~3-10ns saved per non-sync wakeup  
**Risk:** Low - Simple code movement

---

## Performance Impact Estimate

### Before Optimizations
- **Redundant timestamp:** ~5-10ns per call
- **Redundant map lookups:** ~20-50ns per call (when fast paths succeed)
- **Redundant current task:** ~3-10ns per call (non-sync wakeups)

**Total:** ~28-70ns wasted per `select_cpu()` call

### After Optimizations
- **No redundant operations**
- **Context reused from fast paths**
- **Deferred loading for rare paths**

**Expected Improvement:** ~25-60ns per `select_cpu()` call

---

## Implementation Plan

### Phase 1: Eliminate Redundant Timestamp (Immediate)
1. Add `now` parameter to `preload_hot_path_data()`
2. Update all call sites to pass `now`
3. Remove `scx_bpf_now()` call inside function

**Estimated Impact:** ~5-10ns per call  
**Estimated Time:** 15 minutes

### Phase 2: Eliminate Redundant Map Lookups (Next)
1. Add optional `tctx`/`cctx` parameters to `preload_hot_path_data()`
2. Update fast paths to pass loaded context
3. Update function to only lookup if NULL

**Estimated Impact:** ~20-50ns per call (when fast paths succeed)  
**Estimated Time:** 30 minutes

### Phase 3: Defer Current Task Loading (Future)
1. Move `bpf_get_current_task_btf()` into sync wake path
2. Only call when needed

**Estimated Impact:** ~3-10ns per call  
**Estimated Time:** 10 minutes

---

## Conclusion

**Critical Issues Found:** 3 redundant operations in hot path  
**Expected Total Improvement:** ~25-60ns per `select_cpu()` call  
**Priority:** High - These are easy wins with significant impact

**Status:** Ready for implementation

---

**Last Updated:** 2025-11-05

