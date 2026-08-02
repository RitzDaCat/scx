/* SPDX-License-Identifier: GPL-2.0 */
/*
 * scx_cake — a clean-slate sched_ext scheduler.
 *
 * One release algorithm, eight callbacks, built on kernel primitives. No
 * task-history model, telemetry, attributes, division, or build-variant
 * switches: policy is source constants and derived state, so an A/B is two
 * commits. Wakeups queue on a global DSQ so the first CPU to block finds them;
 * continuations queue at home for cache warmth; an idle CPU pulls work with a
 * staggered ring steal. Fairness is a single dsq_vtime advanced by a
 * reciprocal-weight table.
 *
 * DESIGN.md has the model of operation. HYPOTHESES.md §R holds the design
 * rationale and the measurements behind each decision here; §S the constants.
 *
 * This software may be used and distributed according to the terms of the GNU
 * General Public License version 2.
 */
#include <scx/common.bpf.h>
#include "intf.h"

_Static_assert((MAX_CPUS & (MAX_CPUS - 1)) == 0,
	       "MAX_CPUS must remain a power of two");
_Static_assert((RECIP_TABLE_SIZE & (RECIP_TABLE_SIZE - 1)) == 0,
	       "reciprocal table must remain mask-indexable");
_Static_assert(HOME_PREEMPT_YOUNG_NS < HOME_PREEMPT_BASE_MARGIN_NS,
	       "young-current window must fit inside the base margin");
_Static_assert(CAKE_NR_CPUS <= MAX_CPUS,
	       "build-host CPU span must fit Cake MAX_CPUS");

char _license[] SEC("license") = "GPL";

UEI_DEFINE(uei);

/*
 * Rebind the SCX_* enumerators cake uses from autogen's rodata shadows to
 * bpf_core_enum_value(), i.e. from a load per use to a load-time immediate.
 * The #undef is PERMANENT and must follow every scx header (§R.9).
 */
#undef SCX_DSQ_LOCAL
#undef SCX_DSQ_LOCAL_ON
#undef SCX_ENQ_WAKEUP
#undef SCX_KICK_IDLE
#undef SCX_KICK_PREEMPT
#undef SCX_TASK_QUEUED
#undef SCX_WAKE_SYNC
#define CAKE_DSQ_LOCAL    bpf_core_enum_value(enum scx_dsq_id_flags, SCX_DSQ_LOCAL)
#define CAKE_DSQ_LOCAL_ON bpf_core_enum_value(enum scx_dsq_id_flags, SCX_DSQ_LOCAL_ON)
#define CAKE_ENQ_WAKEUP   bpf_core_enum_value(enum scx_enq_flags,    SCX_ENQ_WAKEUP)
#define CAKE_KICK_IDLE    bpf_core_enum_value(enum scx_kick_flags,   SCX_KICK_IDLE)
#define CAKE_KICK_PREEMPT bpf_core_enum_value(enum scx_kick_flags,   SCX_KICK_PREEMPT)
#define CAKE_TASK_QUEUED  bpf_core_enum_value(enum scx_ent_flags,    SCX_TASK_QUEUED)
#define CAKE_WAKE_SYNC    bpf_core_enum_value(enum scx_wake_flags,   SCX_WAKE_SYNC)

/*
 * Cake-local kfunc bindings. MINIMUM KERNEL 7.1: bind straight to the modern
 * kfuncs rather than through compat.bpf.h's ladder, whose untaken arms cost
 * register allocation the verifier's dead-code elimination cannot undo (§R.8).
 */
static __always_inline bool cake_dsq_insert_vtime(struct task_struct *p, u64 dsq_id,
						  u64 slice, u64 vtime, u64 enq_flags)
{
	struct scx_bpf_dsq_insert_vtime_args args = {
		.dsq_id		= dsq_id,
		.slice		= slice,
		.vtime		= vtime,
		.enq_flags	= enq_flags,
	};

	return __scx_bpf_dsq_insert_vtime(p, &args);
}

static __always_inline bool cake_dsq_insert(struct task_struct *p, u64 dsq_id,
					    u64 slice, u64 enq_flags)
{
	return scx_bpf_dsq_insert___v2___compat(p, dsq_id, slice, enq_flags);
}

/*
 * NOTE THE NAME: libbpf strips the trailing ___suffix before resolving, so the
 * ___cake tail is what makes this ask the kernel for the v2 kfunc rather than
 * the base name. Cake needs its own extern, not compat's (§R.8).
 */
bool scx_bpf_dsq_move_to_local___v2___cake(u64 dsq_id, u64 enq_flags) __ksym __weak;
static __always_inline bool cake_move_to_local(u64 dsq_id)
{
	return scx_bpf_dsq_move_to_local___v2___cake(dsq_id, 0);
}

/*
 * Direct field write, not scx_bpf_task_set_slice(): the kfunc runs a
 * sub-scheduler authority check cake can never need (§R.17).
 */
static __always_inline void cake_set_slice(struct task_struct *p, u64 slice)
{
	p->scx.slice = slice;
}

/* SCHED_* are uapi macros, not a BTF enum, so CO-RE has nothing to hook. */
#ifndef SCHED_FIFO
#define SCHED_FIFO 1
#endif
#ifndef SCHED_RR
#define SCHED_RR   2
#endif
#ifndef SCHED_IDLE
#define SCHED_IDLE 5
#endif
#ifndef SCHED_DEADLINE
#define SCHED_DEADLINE 6
#endif

/*
 * All mutable hot state in ONE BSS struct of 128-byte-stride slots, so any two
 * accessed words land in different 64 B lines AND different adjacent-line
 * prefetcher pairs whatever the struct's base alignment (§R.10).
 */
struct cake_slot {
	u64 word;
	u64 pad[STATE_SLOT_WORDS - 1];
};

struct cake_run_slot {
	u64 stamp;
	u64 sum;
	/*
	 * Per-CPU handoff learning: WOKE plus a saturating confidence count of
	 * consecutive wake-then-block-quickly quanta. Owner-written only, so no
	 * atomics, and it rides a line running/stopping already own (§R.18).
	 */
	u64 hint;
	u64 pad[STATE_SLOT_WORDS - 3];
};

enum {
	CAKE_HINT_WOKE		= 1ULL << 0,
	CAKE_HINT_CONF_SHIFT	= 8,
	CAKE_HINT_CONF_MAX	= 3,
	CAKE_NEIGHBOUR_PROBE_DEPTH = 3,
};

/*
 * HARDWARE-ANCHORED: how short a quantum still counts as "woke someone and got
 * out of the way". A property of this CPU's syscall and switch path, not of
 * the timeslice. In rodata so libbpf's freeze folds it. The loader probes the
 * hardware and LOGS it but must NOT drive it yet (§S.5).
 */
const volatile u64 cake_handoff_max_ns		= 1464;

/*
 * Observed frame period: measured, never inherited from a timeslice. Published
 * by the loader from the votes below; no policy consumes it yet (§G11).
 */
u64 cake_frame_ns __attribute__((aligned(STATE_SLOT_BYTES)));

/*
 * The same estimate filtered PESSIMISTICALLY: a cap must never widen because
 * the estimator wobbled (§G18).
 */
u64 cake_frame_floor_ns __attribute__((aligned(STATE_SLOT_BYTES)));

/*
 * Frame-clock votes: the period is the cadence the MOST threads agree on, and
 * each bucket sums so the published value is exact. Loader takes the argmax
 * once a second and clears (§R.22).
 */
struct cake_frame_bucket {
	u64 count;
	u64 sum;
};

struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__uint(max_entries, FRAME_BUCKETS);
	__type(key, u32);
	__type(value, struct cake_frame_bucket);
} cake_frame_hist SEC(".maps");

/*
 * DIAGNOSTIC (to be reverted): G24 expense-vs-benefit census. Firing rates
 * for every path the game campaign never audited, under the fitted mission
 * sim. PERCPU_ARRAY + non-atomic local increment per the 2026-06-17 law.
 * Counters sit in the hot path; this build is not a performance arm.
 */
enum cake_diag_slot {
	DIAG_SELCPU = 0,	/* ops.select_cpu calls   (denominator) */
	DIAG_ENQ,		/* ops.enqueue calls      (denominator) */
	DIAG_DISPATCH,		/* ops.dispatch calls     (denominator) */
	DIAG_NOTIFY,		/* cake_wake_notify calls (denominator) */

	DIAG_SERIAL_TAKEN,	/* serial handoff returned the waker CPU */
	DIAG_HOME_TAKEN,	/* cache-warm prev claim taken		*/
	DIAG_RANKED_IDLE,	/* select_cpu_and claimed an idle CPU	*/
	DIAG_DFL_IDLE,		/* fallback dfl claimed one instead	*/

	DIAG_R6_TRY,		/* sync-distrust condition hit		*/
	DIAG_R6_REPICK,		/*   and the re-pick found an idle CPU	*/
	DIAG_QMARK_TRY,		/* ordered-admission peek taken		*/
	DIAG_QMARK_DEFER,	/*   and it deferred to an older claim	*/

	DIAG_SYNCGATE_TRY,	/* saturated handoff gate entered	*/
	DIAG_SYNCGATE_LAG,	/*   entered via SLEEPER_LAG, not SYNC	*/
	DIAG_SYNCGATE_WAKER,	/*   converged on the waker CPU		*/

	DIAG_KT_IDLE,		/* kthread wake took an idle CPU (G20)	*/
	DIAG_ENQ_STARVE,	/* starved wake routed global (G12)	*/
	DIAG_G17_DIVERT,	/* anti-collision diverted to WAKE_DSQ	*/
	DIAG_PINNED_TRY,	/* pinned-wake preempt evaluated	*/
	DIAG_PINNED_KICK,	/*   and actually fired			*/

	DIAG_QMARK_SETW,	/* qmark stores that actually wrote	*/
	DIAG_QMARK_CLRW,

	DIAG_STEAL_TRY,		/* ring steal entered			*/
	DIAG_STEAL_HIT,		/*   and moved work			*/

	DIAG_SIB_KICK,		/* notify: idle SMT sibling kicked	*/
	DIAG_PICK_KICK,		/* notify: pick_idle kicked		*/
	DIAG_NOTIFY_PREEMPT,	/* notify: target-CPU preempt fired	*/
	DIAG_NEIGH_RUN,		/* neighbour probe loop entered		*/
	DIAG_NEIGH_HIT,		/*   and preempted a neighbour		*/

	DIAG_NR,
};

struct {
	__uint(type, BPF_MAP_TYPE_PERCPU_ARRAY);
	__uint(max_entries, DIAG_NR);
	__type(key, u32);
	__type(value, u64);
} cake_diag SEC(".maps");

static __always_inline void diag_inc(u32 slot)
{
	u64 *v = bpf_map_lookup_elem(&cake_diag, &slot);

	if (v)
		(*v)++;
}

/*
 * This task's mean burst: runtime per voluntary switch, exact to the
 * nanosecond. Supersedes >> log2(nvcsw), which banded (§G11).
 */
static __always_inline u64 cake_burst_ns(const struct task_struct *p)
{
	return p->se.sum_exec_runtime / (p->nvcsw | 1);
}

/*
 * Does this task wait longer than it runs? run_delay is the kernel's lifetime
 * runnable-but-not-running total and pcount its dispatch count, so
 * run_delay/pcount is the mean wait against sum_exec_runtime/nvcsw for the
 * mean burst. Cross-multiplied to spend no divide; both sides carry the same
 * pre-scale, which therefore cancels. The threshold is waiting longer than you
 * run — a definition, not a tuning — and it replaces a burst magnitude that
 * measured the wrong axis entirely (§G12).
 */
static __always_inline bool cake_starved(const struct task_struct *p)
{
	u64 wait = p->sched_info.run_delay >> CAKE_RATIO_SHIFT;
	u64 run = p->se.sum_exec_runtime >> CAKE_RATIO_SHIFT;

	if (!run)
		return false;
	return wait * (p->nvcsw | 1) > run * (p->sched_info.pcount | 1);
}

/*
 * Fold this task's mean wake cadence into the frame clock: a display-coupled
 * thread reports the frame period directly, and casts one vote for it (§G11).
 */
__noinline void cake_frame_observe(struct task_struct *p __arg_trusted, u64 now)
{
	u64 per = (now - p->start_time) / (p->nvcsw | 1);
	struct cake_frame_bucket *b;
	u32 idx;

	if (per < FRAME_PERIOD_MIN_NS || per > FRAME_PERIOD_MAX_NS)
		return;

	idx = (u32)(per >> FRAME_BUCKET_SHIFT) & (FRAME_BUCKETS - 1);
	b = bpf_map_lookup_elem(&cake_frame_hist, &idx);
	if (!b)
		return;

	/* This CPU's own copy: no atomic, no shared line (§R.22). */
	b->count++;
	b->sum += per;
}

_Static_assert(sizeof(struct cake_slot) == STATE_SLOT_BYTES,
	       "cake_slot must preserve cache-isolation stride");
_Static_assert(sizeof(struct cake_run_slot) == STATE_SLOT_BYTES,
	       "cake_run_slot must preserve cache-isolation stride");

struct cake_state {
	/* Global vtime frontier: conditional store from every ops.running. */
	struct cake_slot frontier;
	/*
	 * Per-CPU run accounting. stamp is read remotely by saturated wake
	 * preemption; sum is owner-only and lets ops.stopping charge runtime
	 * with zero clock reads. Both in one slot, so ops.running dirties one
	 * line rather than two (§R.10).
	 */
	struct cake_run_slot run[MAX_CPUS];
	/* ktime the global wake queue was last consumed by ANY CPU (§R.16). */
	struct cake_slot wake_served;
	/*
	 * "DSQ[i] may hold work" hint gating the steal ring, so a going-idle
	 * dispatch stops hashing empty queues. Written only under CPU i's rq
	 * lock; a stale mark is benign by construction (§R.10).
	 */
	struct cake_slot qmark[MAX_CPUS];
};

static struct cake_state cake;


/*
 * Test before writing: every stealer's ring walk reads qmark, so storing the
 * value already there invalidates remote copies for nothing (§R.10).
 */
static __always_inline void cake_qmark_set(u32 cpu)
{
	cpu &= MAX_CPUS - 1;
	if (!cake.qmark[cpu].word) {
		diag_inc(DIAG_QMARK_SETW);
		cake.qmark[cpu].word = 1;
	}
}

static __always_inline void cake_qmark_clear(u32 cpu)
{
	cpu &= MAX_CPUS - 1;
	if (cake.qmark[cpu].word) {
		diag_inc(DIAG_QMARK_CLRW);
		cake.qmark[cpu].word = 0;
	}
}

static __always_inline bool cake_qmark_test(u32 cpu)
{
	cpu &= MAX_CPUS - 1;
	return cake.qmark[cpu].word;
}

/* Loader-filled SMT map; -1 means the CPU has no online sibling. */
const volatile s32 cpu_sibling[MAX_CPUS];

/* Loader-filled: nonzero on a CPU that absorbs an interrupt stream (§G21). */
const volatile u8 cpu_irq_hot[MAX_CPUS];

static __always_inline bool cake_cpu_irq_hot(s32 cpu)
{
	return cpu >= 0 && cpu_irq_hot[(u32)cpu & (MAX_CPUS - 1)];
}

/* Built by ops.init from cpu_irq_hot: the first-choice placement set (§G23). */
private(CAKE_NONSINK) struct bpf_cpumask __kptr *nonsink_cpumask;

/*
 * The CPU id span cake scans, bounding the steal ring and neighbour probe.
 * Rodata, so libbpf's freeze lets the verifier fold it and prune the walk's
 * bound checks; ops.init validates it against nr_cpu_ids (§R.21).
 */
const volatile u32 nr_cpu_span;

#if CAKE_NR_CCDS > 1
/* Loader-sorted: same CCD, same cache-capacity tier, then unrestricted. */
const volatile u16 cpu_steal_order[CAKE_NR_CPUS * CAKE_NR_CPUS];
#endif

/*
 * Reciprocal-weight table for division-free vtime charging:
 *
 *   recip_weight[i] = (1024 << 20) / sched_prio_to_weight[i]
 *
 * so `used * recip_weight[i] >> 20 == used * 1024 / weight`. Indexed by
 * `nice + 20 == static_prio - 100 ∈ [0, 39]`, with SCHED_IDLE's distinct raw
 * weight 3 at IDLE_RECIP_INDEX. Sized to a power of 2 so the index masks.
 */
static const u64 recip_weight[RECIP_TABLE_SIZE] = {
	   12097,    14964,    19009,    23204,    29587, /* nice -20..-16 */
	   36830,    46174,    57404,    71827,    90109, /* nice -15..-11 */
	  112457,   140911,   176023,   218952,   274895, /* nice -10..-6  */
	  344037,   429324,   539297,   677012,   840831, /* nice  -5..-1  */
	 1048576,  1309441,  1639300,  2041334,  2538396, /* nice  +0..+4  */
	 3205199,  3947580,  4994148,  6242685,  7837531, /* nice  +5..+9  */
	 9761289, 12341860, 15339168, 19173961, 23860929, /* nice +10..+14 */
	29826161, 37025580, 46684427, 59652323, 71582788, /* nice +15..+19 */
	/* SCHED_IDLE raw weight 3, then padding [41..63] at nice +19. */
	357913941, 71582788, 71582788, 71582788, 71582788, 71582788,
	71582788, 71582788, 71582788, 71582788, 71582788, 71582788,
	71582788, 71582788, 71582788, 71582788, 71582788, 71582788,
	71582788, 71582788, 71582788, 71582788, 71582788, 71582788,
};

#define CAKE_RECIP_RUNTIME_FAST_MAX (~0ULL / (u64)MAX_RECIP_WEIGHT)

static __noinline u64 cake_scale_vtime_slow(u64 base, u64 runtime, u64 reciprocal)
{
	return base + (runtime >> RECIP_SHIFT) * reciprocal +
	       ((runtime & RECIP_MASK) * reciprocal >> RECIP_SHIFT);
}


/*
 * Return @base plus @runtime scaled by the task's reciprocal weight, without
 * letting the full-width product wrap. Splitting runtime at the fixed-point
 * radix is algebraically identical and stays exact past eight years of
 * uninterrupted nice-19 runtime. @base is folded in here rather than added by
 * the caller so it need not live across the overflow-safe call (§R.11).
 */
static __always_inline u64 cake_scale_vtime_add(u64 base, u64 runtime, u32 idx)
{
	u64 reciprocal = recip_weight[idx & RECIP_INDEX_MASK];

	/* Every ordinary slice takes the original single-multiply fast path. */
	if (runtime <= CAKE_RECIP_RUNTIME_FAST_MAX)
		return base + ((runtime * reciprocal) >> RECIP_SHIFT);

	return cake_scale_vtime_slow(base, runtime, reciprocal);
}

static __always_inline u64 cake_scale_vtime(u64 runtime, u32 idx)
{
	return cake_scale_vtime_add(0, runtime, idx);
}

/* Preserve the exact 40-level nice table while honoring SCHED_IDLE weight 3. */
static __always_inline u32 cake_recip_index(const struct task_struct *p)
{
	if (p->policy == SCHED_IDLE)
		return IDLE_RECIP_INDEX;
	return (u32)(p->static_prio - STATIC_PRIO_BASE);
}

/*
 * The occupant of @tcpu, as the only two numbers any caller wants: how long it
 * has held the CPU, and its live vtime once that runtime is charged. Returns 0
 * when there is no SCX occupant (idle, or a class carrying no vtime); @ran_out
 * is written only when it returns non-zero. Five call sites once computed this
 * identically (§R.11).
 */
static __noinline u64 cake_occupant_live(s32 tcpu, u64 *ran_out)
{
	struct task_struct *curr = scx_bpf_cpu_curr(tcpu);
	u64 cv, ran, stamp;
	u32 cidx;

	if (!curr)
		return 0;
	cv = curr->scx.dsq_vtime;
	if (!cv)
		return 0;

	stamp = cake.run[(u32)tcpu & (MAX_CPUS - 1)].stamp;
	cidx = cake_recip_index(curr);

	ran = bpf_ktime_get_ns() - stamp;
	*ran_out = ran;
	return cake_scale_vtime_add(cv, ran, cidx);
}

/*
 * Wake preemption: kick @tcpu off its occupant for @p, but only once the
 * occupant has run at least @min_ran and @p out-deserves its LIVE vtime.
 *
 * @min_ran is a fraction of the observed FRAME, not of the timeslice: how long
 * a waker may be made to wait is a display question. A sixteenth at the
 * global-wake floor, a quarter for the speculative neighbour probe, which must
 * be surer before disturbing another CPU (§G11.4).
 *
 * LIVE vtime, not the stored one: curr's dsq_vtime is charged only at
 * stopping, so mid-slice it looks eternally deserving. A zero live vtime means
 * RT/DL or idle, which we neither can nor need preempt. §R.1.
 */
static __noinline bool cake_wake_preempt(struct task_struct *p, s32 tcpu,
					 u64 min_ran)
{
	u64 ran = 0;
	u64 live = cake_occupant_live(tcpu, &ran);
	struct task_struct *curr;

	if (!live || ran < min_ran || !time_before(p->scx.dsq_vtime, live))
		return false;

	/*
	 * Never preempt a pipeline stage; tested last so rejections stay
	 * cheap (§G10.5).
	 */
	curr = scx_bpf_cpu_curr(tcpu);
	if (curr && cake_starved(curr))
		return false;

	scx_bpf_kick_cpu(tcpu, CAKE_KICK_PREEMPT);
	return true;
}


/*
 * Is the SYSTEM serial right now? Co-location is a bet that there is nowhere
 * better than the waker's CPU, and that is only true when almost nothing is
 * runnable. Per-CPU emptiness cannot answer this -- a parallel workload with
 * fewer threads than CPUs has empty per-CPU queues by definition -- so the
 * discriminant has to be the global one (§G9.3, §G10).
 *
 * Restored 2026-07-30: G9.4 deleted this term as "cheaper AND sharper" and the
 * game regression dates from exactly there. Helldivers 2 runs ~62% idle
 * against this 75% threshold, so it declines precisely where G9.4 admits.
 */
static __noinline bool cake_system_serial(void)
{
	const struct cpumask *idle;
	u32 nr;

	idle = scx_bpf_get_idle_cpumask();
	nr = bpf_cpumask_weight(idle);
	scx_bpf_put_idle_cpumask(idle);

	return nr * 4 >= nr_cpu_span * 3;
}

/*
 * The liveness term: scx_bpf_dsq_nr_queued() counts tasks WAITING, never the
 * one EXECUTING, so a CPU busy mid-slice reads as empty and the co-location
 * gate would admit onto it. Admit when there is no SCX occupant, or when the
 * occupant is within one handoff of the end of its OWN typical burst and so
 * about to yield the CPU anyway.
 *
 * Measured RELATIVE to the occupant, not against a fixed age: an absolute
 * window cannot tell a serial partner about to block from a long-running
 * thread that merely started recently, and reading it as the latter is what
 * cost Helldivers 2 its tails (§R.1, §G9.7; game result in STATE.md).
 *
 * Cannot be exact: ALLOW_QUEUED_WAKEUP hides the waker and cake keeps no
 * per-task state, so this refuses a bad bet rather than proving a good one.
 */
static __noinline bool cake_handoff_yields(s32 tcpu)
{
	struct task_struct *curr = scx_bpf_cpu_curr(tcpu);
	u64 ran, burst;

	if (!curr || !curr->scx.dsq_vtime)
		return true;

	ran = bpf_ktime_get_ns() -
	      cake.run[(u32)tcpu & (MAX_CPUS - 1)].stamp;
	burst = cake_burst_ns(curr);

	return burst <= ran || burst - ran < cake_handoff_max_ns;
}


/*
 * The slice this task needs: twice its own burst, floored at the chain
 * boundary, capped at half a frame -- taken from the PESSIMISTIC frame
 * estimate, since a bound must not widen when the estimator wobbles. The grant
 * is a PREEMPTION TIMER, not a vtime question (§G10.4, §G11.5, §G18).
 */
static __always_inline u64 cake_task_slice(const struct task_struct *p)
{
	u64 want = cake_burst_ns(p) << 1;
	u64 cap = cake_frame_floor_ns >> FRAME_SLICE_CAP_SHIFT;

	if (want < cake_handoff_max_ns)
		want = cake_handoff_max_ns;
	if (want > cap)
		want = cap;
	return want;
}

/* The S1d dose: three quarters of the unused slice, as two shifts (§R.13). */
static __always_inline u64 cake_sleeper_dose(u64 unused)
{
	return (unused >> 1) + (unused >> 2);	/* 3/4 */
}

/*
 * How much deeper than the uniform floor this task's sleeper key may sit:
 * proportional to the slice fraction it leaves unused, so a short-burst
 * cadence task outranks a full-slice compute wake instead of tying it and
 * serving FIFO. Burst estimate is sum_exec >> log2(nvcsw) — no map, no
 * division, within 2x of the true mean (§R.13). Stage bursts only: the dose
 * inverts with burst, so ungated it ranks a worker above a render stage
 * (§G10.6).
 */
static __always_inline u64 cake_cadence_depth(const struct task_struct *p)
{
	u64 burst = cake_burst_ns(p);

	if (!cake_starved(p) || burst >= SLICE_NS)
		return 0;
	return cake_sleeper_dose(SLICE_NS - burst);
}

/*
 * The wake arm's insert key: the sleeper clamp taken against a floor deepened
 * by the task's unused slice fraction. Recomputed at each use rather than held
 * — every input is a plain load, and the frontier it reads is advisory, so a
 * later read is simply a fresher one (§R.13).
 */
static __noinline u64 cake_wake_vtime(const struct task_struct *p)
{
	u64 lo = cake.frontier.word - SLICE_NS - cake_cadence_depth(p);
	u64 d  = p->scx.dsq_vtime - lo;

	return lo + (d & ~((u64)((s64)d >> 63)));
}

/*
 * Home-arm notification, decided AFTER the insert rather than carried across
 * it. Returns true when tcpu needs nothing further from us. Both re-read
 * inputs are exact there, not approximations (§R.14). A zero curr vtime means
 * an idle- or higher-class-owned home with no preempt to fire, where the only
 * question left is whether core already rescheduled it for us.
 */
static __noinline bool cake_home_notify(struct task_struct *p, s32 tcpu)
{
	u64 ran = 0;
	u64 live = cake_occupant_live(tcpu, &ran);

	if (!live)
		return (u32)tcpu == bpf_get_smp_processor_id();

	if (ran >= HOME_PREEMPT_YOUNG_NS ||
	    !time_before(p->scx.dsq_vtime + HOME_PREEMPT_BASE_MARGIN_NS -
			 (ran >> HOME_PREEMPT_RAN_CREDIT_SHIFT), live))
		return false;

	scx_bpf_kick_cpu(tcpu, CAKE_KICK_PREEMPT);
	return true;
}

/*
 * Direct dispatch must pay the same vtime floor as a queued admission —
 * scx_bpf_dsq_insert() takes no vtime, so without this a long sleeper admitted
 * directly keeps an arbitrarily old key and monopolises the moment load rises.
 * Admission is service whichever door it comes through (§R.13).
 */
static __always_inline void cake_direct_clamp(struct task_struct *p)
{
	p->scx.dsq_vtime = cake_wake_vtime(p);
}


/*
 * ops.select_cpu — placement plus guarded direct admission.
 *
 * When the ranked pick (nonsink set first, whole machine as fallback) finds an
 * idle CPU we normally direct-dispatch to the local DSQ for lowest latency,
 * unless the qmark guard sees an older visible claim. Otherwise the system is
 * saturated on this task's affinity: return the selected CPU and let
 * ops.enqueue() arbitrate on vtime.
 */
s32 BPF_STRUCT_OPS(cake_select_cpu, struct task_struct *p, s32 prev_cpu,
		   u64 wake_flags)
{
	bool is_idle = false;
	s32 cpu;

	diag_inc(DIAG_SELCPU);


	/*
	 * Serial-handoff co-location, for a genuine handoff pair only: a
	 * saturated learned handoff bit, no WAKE_SYNC, the wakee allowed here,
	 * both queues empty, and an occupant that will actually yield. No
	 * preempt -- the waker still holds the mutex. MUST run before
	 * select_cpu_dfl(), which RESERVES the idle CPU it returns (§R.1).
	 */
	{
		u32 wc = bpf_get_smp_processor_id() & (MAX_CPUS - 1);
		struct cake_run_slot *wr = &cake.run[wc];
		bool serial = ((wr->hint >> CAKE_HINT_CONF_SHIFT) &
			       CAKE_HINT_CONF_MAX) >= CAKE_HINT_CONF_MAX;

		if (!(wr->hint & CAKE_HINT_WOKE))
			wr->hint |= CAKE_HINT_WOKE;

		if (serial && !(wake_flags & CAKE_WAKE_SYNC) &&
		    bpf_cpumask_test_cpu((s32)wc, p->cpus_ptr) &&
		    cake_system_serial() &&
		    !scx_bpf_dsq_nr_queued((u64)wc) &&
		    !scx_bpf_dsq_nr_queued(CAKE_DSQ_LOCAL_ON | wc) &&
		    cake_handoff_yields((s32)wc)) {
			s32 wcpu = (s32)wc;

			diag_inc(DIAG_SERIAL_TAKEN);
			cake_direct_clamp(p);
			cake_dsq_insert(p, CAKE_DSQ_LOCAL_ON | (u32)wcpu,
					cake_task_slice(p), 0);
			return wcpu;
		}
	}

	/*
	 * Cache-warm home claim for a task that is SERVED rather than waiting:
	 * select_cpu_dfl prefers a fully idle core over a merely idle prev_cpu,
	 * which is wrong for an occupant of its own cache. WAKE_SYNC excluded --
	 * there the waker's cache holds the data. MUST precede select_cpu_dfl(),
	 * which reserves what it returns (§G13).
	 */
	if (!(wake_flags & CAKE_WAKE_SYNC) && prev_cpu >= 0 &&
	    !cake_starved(p) && !cake_cpu_irq_hot(prev_cpu) &&
	    bpf_cpumask_test_cpu(prev_cpu, p->cpus_ptr) &&
	    scx_bpf_test_and_clear_cpu_idle(prev_cpu)) {
		diag_inc(DIAG_HOME_TAKEN);
		cake_direct_clamp(p);
		cake_dsq_insert(p, CAKE_DSQ_LOCAL_ON | (u32)prev_cpu,
				cake_task_slice(p), 0);
		return prev_cpu;
	}

	/*
	 * First-choice placement runs dfl's ranking against the nonsink set,
	 * so a sink is claimed only when nothing quieter is idle -- the
	 * fallback ranks the whole machine and its claim is always the CPU
	 * returned, never abandoned (§G23).
	 */
	{
		const struct cpumask *ns = cast_mask(nonsink_cpumask);

		cpu = -1;
		if (ns && __COMPAT_HAS_scx_bpf_select_cpu_and)
			cpu = scx_bpf_select_cpu_and(p, prev_cpu, wake_flags,
						     ns, 0);
	}
	if (cpu >= 0) {
		is_idle = true;
		diag_inc(DIAG_RANKED_IDLE);
	} else {
		cpu = scx_bpf_select_cpu_dfl(p, prev_cpu, wake_flags, &is_idle);
		if (is_idle)
			diag_inc(DIAG_DFL_IDLE);
	}
	if (is_idle) {
		/*
		 * Distrust a WAKE_SYNC return of the waker's own CPU: it can be
		 * the waker's still-BUSY CPU, and taking it welds buffered
		 * pairs together permanently (§R.6). Re-rank without the SYNC
		 * flag.
		 */
		if ((wake_flags & CAKE_WAKE_SYNC) &&
		    cpu == (s32)bpf_get_smp_processor_id()) {
			const struct cpumask *ns = cast_mask(nonsink_cpumask);
			s32 idle;

			diag_inc(DIAG_R6_TRY);
			if (ns && __COMPAT_HAS_scx_bpf_select_cpu_and)
				idle = scx_bpf_select_cpu_and(p, prev_cpu, 0,
							      ns, 0);
			else
				idle = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
			if (idle >= 0) {
				diag_inc(DIAG_R6_REPICK);
				cpu = idle;
			}
		}

		/*
		 * Ordered direct admission: confirm the qmark hint with one
		 * lockless head snapshot so the shortcut cannot jump an older
		 * claim. Advisory, not a reservation (§R.4).
		 */
		if ((wake_flags & CAKE_WAKE_SYNC) &&
		    cake_qmark_test((u32)cpu)) {
			struct task_struct *head;

			diag_inc(DIAG_QMARK_TRY);
			head = scx_bpf_dsq_peek((u64)(u32)cpu);
			if (head) {
				u64 head_vtime = head->scx.dsq_vtime;

				if (time_before(head_vtime, cake_wake_vtime(p))) {
					diag_inc(DIAG_QMARK_DEFER);
					return cpu;
				}
			}
		}
		cake_direct_clamp(p);
		cake_dsq_insert(p, CAKE_DSQ_LOCAL, cake_task_slice(p), 0);
		return cpu;
	}

	/*
	 * Saturated handoff convergence on the callback CPU, whose queue state
	 * is authoritative: an explicit WAKE_SYNC handoff, or a wakee whose raw
	 * vtime proves it slept behind the frontier. A frontier-running compute
	 * peer keeps dfl's prev placement instead. Returning the CPU rather than
	 * direct-dispatching keeps live-vtime eligibility (§R.20).
	 */
	if ((wake_flags & CAKE_WAKE_SYNC) ||
	    time_before(p->scx.dsq_vtime + SLEEPER_LAG_NS, cake.frontier.word)) {
		s32 waker_cpu = (s32)bpf_get_smp_processor_id();

		diag_inc(DIAG_SYNCGATE_TRY);
		if (!(wake_flags & CAKE_WAKE_SYNC))
			diag_inc(DIAG_SYNCGATE_LAG);
		if (bpf_cpumask_test_cpu(waker_cpu, p->cpus_ptr) &&
		    !scx_bpf_dsq_nr_queued(CAKE_DSQ_LOCAL_ON | (u32)waker_cpu) &&
		    !scx_bpf_dsq_nr_queued((u64)(u32)waker_cpu)) {
			diag_inc(DIAG_SYNCGATE_WAKER);
			return waker_cpu;
		}
	}

	return cpu;
}

/* The wake routing decision: three reachable states, one value (§R.11). */
enum cake_route { ROUTE_GLOBAL = 0, ROUTE_HOME_QUEUE, ROUTE_HOME_CLAIM };

/*
 * Post-insert notification: tell somebody the task is runnable. The task is
 * already published, so only p, tcpu and the route cross this boundary — which
 * is what makes the cut cheap (§R.11).
 */
__noinline void cake_wake_notify(struct task_struct *p __arg_trusted, s32 tcpu,
				 u32 route)
{
	s32 idle;

	diag_inc(DIAG_NOTIFY);
	if (route == ROUTE_HOME_CLAIM && cake_home_notify(p, tcpu))
		return;

	/* Prefer an idle SMT sibling for globally queued cold pickup. */
	if (route == ROUTE_GLOBAL) {
		s32 sib = cpu_sibling[(u32)tcpu & (MAX_CPUS - 1)];

		if (sib >= 0 && bpf_cpumask_test_cpu(sib, p->cpus_ptr) &&
		    scx_bpf_test_and_clear_cpu_idle(sib)) {
			diag_inc(DIAG_SIB_KICK);
			scx_bpf_kick_cpu(sib, CAKE_KICK_IDLE);
			return;
		}
	}

	idle = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
	if (idle >= 0) {
		diag_inc(DIAG_PICK_KICK);
		scx_bpf_kick_cpu(idle, CAKE_KICK_IDLE);
		return;
	}

	/*
	 * No idle CPU anywhere, and every route still owes tcpu a decision:
	 * either the occupant loses the CPU, or the wakee waits because the
	 * occupant genuinely deserves it. Cake registers no .tick, so an arm
	 * that decides neither leaves the wakee on the 5 s watchdog (§R.14).
	 */
	if (cake_wake_preempt(p, tcpu,
			       cake_frame_ns >> FRAME_PREEMPT_PROTECT_SHIFT)) {
		diag_inc(DIAG_NOTIFY_PREEMPT);
		return;
	}

	/*
	 * Globally queued only: hunt a mid-slice compute occupant among the
	 * neighbours. A home-routed wake stays put -- stealing it back off the
	 * queue would spend the locality the routing just bought.
	 */
	if (route != ROUTE_HOME_CLAIM) {
		u32 cand = (u32)tcpu;
		u32 pi;

		diag_inc(DIAG_NEIGH_RUN);
		for (pi = 0; pi < CAKE_NEIGHBOUR_PROBE_DEPTH; pi++) {
			cand++;
			if (cand >= nr_cpu_span)
				cand = 0;
			if (!bpf_cpumask_test_cpu((s32)cand, p->cpus_ptr))
				continue;
			if (cake_wake_preempt(p, (s32)cand,
					       cake_frame_ns >>
					       FRAME_PROBE_PROTECT_SHIFT)) {
				diag_inc(DIAG_NEIGH_HIT);
				break;
			}
		}
	}
}

/*
 * Does this wake CLAIM its empty home, or merely queue there? A claim earns a
 * preempt; a mere queue takes prev-CPU warmth without one. Two frontier-running
 * compute peers must never claim the same CPU -- cake has no periodic balancer,
 * so they would mutually preempt forever (§R.2).
 *
 * The claims run cheapest first: the wakee is a sleeper (arithmetic on values
 * already in hand), the home is idle-owned, or the occupant is a valve about to
 * block (a clock read plus weight scaling, so it goes last).
 */
static __noinline bool cake_home_claim(struct task_struct *p __arg_trusted,
				       s32 tcpu)
{
	u64 lo = cake.frontier.word - SLICE_NS;
	u64 ran = 0, live;

	/*
	 * The converged-pair claim is DELETED here: routing its traffic global
	 * cost futex 4.8M -> 0.98M, and it is UNMEASURED since (§R.19).
	 */
	if ((s64)(p->scx.dsq_vtime - lo) < (s64)SLEEPER_LAG_NS)
		return true;

	live = cake_occupant_live(tcpu, &ran);
	if (!live)
		return true;

	return time_before(live, cake.frontier.word - SLICE_NS);
}

/*
 * The wake half of ops.enqueue: route the wakee, insert it, then notify.
 *
 * A wake and a continuation are two algorithms, not one algorithm with a
 * shared flag set, so each gets its own subprogram and its own register
 * budget. Global rather than static so it keeps its own BTF signature and
 * frame; @p is __arg_trusted because the verifier checks it independently
 * (§R.11).
 */
__noinline void cake_enqueue_wake(struct task_struct *p __arg_trusted, s32 tcpu)
{

	struct task_struct *curr = scx_bpf_cpu_curr(tcpu);
	bool empty_home;
	u32 route;

	/*
	 * Self-race first: waking the task this CPU is still switching out
	 * (sub-slice block/wake cadence rides the ttwu wakelist and lands here
	 * with curr == p -- the pipe/futex on-cpu shape). Home is right even
	 * behind a non-empty queue, since p ran here microseconds ago and vtime
	 * order keeps the queue fair; eligibility and a kick would both be
	 * spent against ourselves. Hottest wake path, so it precedes the
	 * nr_queued rhashtable lookup (§R.14).
	 */
	if (curr == p) {
		cake_qmark_set((u32)tcpu);
		cake_dsq_insert_vtime(p, (u64)(u32)tcpu, cake_task_slice(p),
				      cake_wake_vtime(p), CAKE_ENQ_WAKEUP);
		return;
	}

	/*
	 * M8: an RT-owned CPU is never an empty home. It may burst again and
	 * the kernel preempts SCX for it unconditionally; EEVDF discounts such
	 * a CPU's capacity by its RT/DL load average and sched_ext's idle
	 * tracking has no equivalent, so approximate it here (§R.14).
	 */
	empty_home = !(curr && (curr->policy == SCHED_FIFO ||
				curr->policy == SCHED_RR ||
				curr->policy == SCHED_DEADLINE)) &&
		     !scx_bpf_dsq_nr_queued((u64)(u32)tcpu);

	route = empty_home ? ROUTE_HOME_QUEUE : ROUTE_GLOBAL;
	if (empty_home && cake_home_claim(p, tcpu))
		route = ROUTE_HOME_CLAIM;

	/*
	 * An already-backlogged global queue is the oversubscription
	 * signature: scattering one more wake buys nothing, it lands cold and
	 * splits its pair (the t32/t64 herd collapse). Probed lazily, so an
	 * empty home never pays for the signal.
	 */
	if (route == ROUTE_GLOBAL && scx_bpf_dsq_nr_queued((u64)WAKE_DSQ))
		route = ROUTE_HOME_QUEUE;

	if (route)
		cake_qmark_set((u32)tcpu);
	/*
	 * The wake bit as a literal, not the caller's enq_flags: a PRIQ insert
	 * into a custom DSQ reads none of the caller's positional bits (§R.5).
	 */
	cake_dsq_insert_vtime(p,
			      route ? (u64)(u32)tcpu : (u64)WAKE_DSQ,
			      cake_task_slice(p), cake_wake_vtime(p),
			      CAKE_ENQ_WAKEUP);

	cake_wake_notify(p, tcpu, route);

}

/*
 * Pinned-wake service. A pinned user task's wake lands on the continuation
 * path because nr_cpus_allowed == 1, and NO other CPU may steal it, so without
 * this it waits out the occupant's whole slice. Preempts by RAW sleep depth
 * @d, which must be read before the insert rewrites p->scx.dsq_vtime (§R.14).
 */
static __noinline void cake_pinned_wake_preempt(struct task_struct *p __arg_trusted,
						s32 tcpu, u64 d)
{
	u64 cran = 0;
	u64 clive = cake_occupant_live(tcpu, &cran);
	u64 lo, dd, pvt;

	diag_inc(DIAG_PINNED_TRY);
	if (!clive)
		return;

	lo = cake.frontier.word - SLICE_NS;
	dd = d + SLICE_NS;
	pvt = lo - SLICE_NS + (dd & ~((u64)((s64)dd >> 63)));

	if (time_before(pvt + HOME_PREEMPT_BASE_MARGIN_NS, clive)) {
		diag_inc(DIAG_PINNED_KICK);
		scx_bpf_kick_cpu(tcpu, CAKE_KICK_PREEMPT);
	}
}

/*
 * ops.enqueue — reached when no idle CPU was claimed, or guarded direct
 * admission found an older visible per-CPU claim.
 *
 * Insert into the OWNER's vtime queue, dsq_id == task_cpu: task_cpu is
 * post-core-validation and always in p->cpus_ptr, and this callback holds that
 * CPU's rq lock, so either the owner scans its own DSQ after the insert or
 * core's activate→wakeup_preempt rescheds it out of idle for us — no insert
 * kick is owed to the owner. On a same-CPU futex handoff the insert and the
 * consume both happen under one rq lock: EEVDF's in-place shape.
 *
 * Then keep the rest of the machine work-conserving by kicking one idle CPU,
 * whose idle bit pick_idle CLAIMS, so concurrent wakers fan out.
 */
void BPF_STRUCT_OPS(cake_enqueue, struct task_struct *p, u64 enq_flags)
{
	/* task_cpu(p) read directly; the kfunc is one load behind a call (§R.17). */
	s32 tcpu = (s32)p->thread_info.cpu;
	u64 lo, d;
	s32 idle;

	diag_inc(DIAG_ENQ);

	/*
	 * Kernel-thread wakes go straight to the selected CPU's local DSQ, so
	 * essential softirq/workqueue service is bounded by one occupant slice
	 * rather than herd order — the scx watchdog itself rides unbound
	 * kworkers. PF_KTHREAD is scheduling state, not workload identity. Only
	 * the wake takes this path; a continuation falls through (§R.14). An idle
	 * CPU serves it as promptly and evicts nobody (§G20).
	 */
	if ((enq_flags & CAKE_ENQ_WAKEUP) && (p->flags & PF_KTHREAD)) {
		s32 kcpu = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);

		if (kcpu >= 0)
			diag_inc(DIAG_KT_IDLE);

		cake_direct_clamp(p);
		cake_dsq_insert(p, CAKE_DSQ_LOCAL_ON |
				   (u32)(kcpu >= 0 ? kcpu : tcpu),
				   SLICE_NS, enq_flags);
		return;
	}

	/*
	 * Sleeper clamp max(own, frontier - one slice), branchless and wrap-safe
	 * under time_before() semantics:
	 *   d = own - lo; own >= lo => (s64)d >= 0 => mask = ~0 => lo + d = own
	 *                 own <  lo => (s64)d <  0 => mask =  0 => lo + 0 = lo
	 * Only the continuation arm consumes it; the wake arm derives its own
	 * cadence-deep floor (§R.13).
	 */
	lo = cake.frontier.word - SLICE_NS;
	d  = p->scx.dsq_vtime - lo;

	/*
	 * STAGE wakeups are global, everything else is local -- the routing key
	 * is the wakeup bit AND the burst class (§G10.2). Single-CPU tasks take
	 * the continuation arm regardless (§R.14).
	 */
	if ((enq_flags & CAKE_ENQ_WAKEUP) && p->nr_cpus_allowed > 1 &&
	    cake_starved(p)) {
		diag_inc(DIAG_ENQ_STARVE);
		cake_enqueue_wake(p, tcpu);
		return;
	}

	/*
	 * The continuation arm. It stays on its owner's queue whatever the
	 * depth there: the mark makes it visible to the steal ring, so leaving
	 * it keeps the L1/L2 warmth it was queued for and still conserves work.
	 * A separate overflow bucket needed two catchers and stalled anyway
	 * (§R.15). The slice is the task's own, not a flat grant (§G10).
	 */
	{
		u64 vt = lo + (d & ~((u64)((s64)d >> 63)));
		struct task_struct *hc = scx_bpf_cpu_curr(tcpu);

		/*
		 * Anti-collision: home held by an equally well-served PEER. The
		 * depth-blind home rule above is right for a worker occupant,
		 * whose slice is short, and wrong for a peer, whose whole slice
		 * must be waited out while other CPUs sit idle. Reaching this
		 * arm already proves p is not starved, so only the occupant
		 * needs testing (§G17).
		 */
		if ((enq_flags & CAKE_ENQ_WAKEUP) && p->nr_cpus_allowed > 1 &&
		    hc && !(hc->flags & PF_IDLE) && !cake_starved(hc)) {
			diag_inc(DIAG_G17_DIVERT);
			cake_dsq_insert_vtime(p, (u64)WAKE_DSQ,
					      cake_task_slice(p),
					      cake_wake_vtime(p), enq_flags);
			goto kick_idle;
		}

		cake_qmark_set((u32)tcpu);
		cake_dsq_insert_vtime(p, (u64)(u32)tcpu, cake_task_slice(p),
					 vt, enq_flags);

		if ((enq_flags & CAKE_ENQ_WAKEUP) && p->nr_cpus_allowed == 1)
			cake_pinned_wake_preempt(p, tcpu, d);
	}

kick_idle:

	idle = scx_bpf_pick_idle_cpu(p->cpus_ptr, 0);
	if (idle >= 0)
		scx_bpf_kick_cpu(idle, CAKE_KICK_IDLE);
}

/*
 * Staggered ring steal: walk the other CPUs' queues, marked ones first, and
 * move the first task found. Two constant-start half-loops from cpu+1 rather
 * than a modulo — no wrap arithmetic, verifier-friendly — with the own-offset
 * start as a zero-cost anti-herd stagger (§R.7).
 */
static __noinline bool cake_ring_steal(u32 ucpu)
{
	u32 nr = nr_cpu_span;
	u32 i, idx;


#if CAKE_NR_CCDS > 1 && CAKE_CCD_STEAL_POLICY > 0
	/* One precomputed locality order avoids verifier-multiplying scan loops. */
	if (ucpu >= CAKE_NR_CPUS)
		return true;
	for (i = 0; i < MAX_CPUS; i++) {
		if (i >= CAKE_NR_CPUS || i + 1 >= nr)
			break;
		idx = cpu_steal_order[ucpu * CAKE_NR_CPUS + i];
		if (cake_qmark_test(idx) &&
		    cake_move_to_local((u64)idx))
			return true;
	}
#else
	for (i = 0; i < MAX_CPUS; i++) {	/* upper half: cpu+1 .. nr-1 */
		idx = ucpu + 1 + i;
		if (idx >= nr)
			break;
		if (cake_qmark_test(idx) &&
		    cake_move_to_local((u64)idx))
			return true;
	}
	for (i = 0; i < MAX_CPUS; i++) {	/* lower half: 0 .. cpu-1 */
		if (i >= ucpu)
			break;
		if (cake_qmark_test(i) &&
		    cake_move_to_local((u64)i))
			return true;
	}
#endif

	return false;
}

/* Has the global wake queue gone unserved for a full WALL-clock window? */
static __noinline bool cake_wake_starved(void)
{
	return time_before(cake.wake_served.word + WAKE_STARVE_WALL_NS,
			   bpf_ktime_get_ns());
}

/* Record that someone served the global wake queue. */
static __noinline void cake_wake_serve_stamp(void)
{
	cake.wake_served.word = bpf_ktime_get_ns();
}

/*
 * An EMPTY wake queue is a SERVED wake queue — without this the escalation is
 * permanently armed in any regime where wakes mostly route home. Refreshed
 * only once the stamp is already half a window old, because every CPU's
 * dispatch polls this line and an unconditional store would cost an RFO at
 * context-switch rate (§R.16).
 */
static __noinline void cake_wake_idle_stamp(void)
{
	u64 now = bpf_ktime_get_ns();

	if (time_before(cake.wake_served.word + WAKE_STARVE_REFRESH_NS, now))
		cake.wake_served.word = now;
}

/*
 * The dispatch search: earliest eligible vtime of {own queue, wake queue},
 * then the staggered ring steal. Returns true when it moved work local.
 *
 * Two lockless head peeks pick the earlier vtime and the other is the
 * immediate fallback. The vtime comparison is what makes this starvation-free
 * with no rescue path — a stranded wake head's vtime is frozen while running
 * tasks advance past it (§R.3, §R.7).
 */
static __noinline bool cake_dispatch_search(s32 cpu)
{
	u32 ucpu = (u32)cpu;
	u64 first = (u64)ucpu, second = (u64)WAKE_DSQ;
	struct task_struct *own, *wake;

	/*
	 * Own queue first, global wake queue second, with a one-slice margin.
	 * The margin is HYSTERESIS, not fairness slack: without it every CPU
	 * takes the global lock first and the wake-storm serialisation returns.
	 * The head peek republishes qmark with one conditional store (§R.3).
	 */
	own = scx_bpf_dsq_peek((u64)ucpu);
	if (own)
		cake_qmark_set(ucpu);
	else
		cake_qmark_clear(ucpu);
	wake = scx_bpf_dsq_peek((u64)WAKE_DSQ);
	if (wake) {
		u64 wv = wake->scx.dsq_vtime;

		/*
		 * Either the vtime margin favours the wake head, or nobody
		 * served that queue in a wall-clock window (§R.16, §G11.2).
		 */
		if (!own ||
		    time_before(wv + DEEP_WAKE_HYSTERESIS_NS,
				own->scx.dsq_vtime) ||
		    cake_wake_starved()) {
			first  = (u64)WAKE_DSQ;
			second = (u64)ucpu;
		}
	} else {
		cake_wake_idle_stamp();
	}
	if (cake_move_to_local(first)) {
		if (first == (u64)WAKE_DSQ)
			cake_wake_serve_stamp();
		return true;
	}
	if (cake_move_to_local(second)) {
		if (second == (u64)WAKE_DSQ)
			cake_wake_serve_stamp();
		return true;
	}

	diag_inc(DIAG_STEAL_TRY);
	if (cake_ring_steal(ucpu)) {
		diag_inc(DIAG_STEAL_HIT);
		return true;
	}
	return false;
}

/*
 * ops.dispatch — run the search, and if it finds nothing anywhere keep prev
 * running with a fresh slice rather than idling.
 */
void BPF_STRUCT_OPS(cake_dispatch, s32 cpu, struct task_struct *prev)
{
	diag_inc(DIAG_DISPATCH);
	if (cake_dispatch_search(cpu))
		return;

	if (prev && (prev->scx.flags & CAKE_TASK_QUEUED))
		cake_set_slice(prev, cake_task_slice(prev));
}

/*
 * ops.running — stamp the per-CPU run start and advance the global vtime
 * frontier to this task's deadline.
 *
 * The frontier store is deliberately conditional, NOT a branchless max: this
 * is the hottest shared line in the scheduler, and a select would dirty it
 * every quantum on every CPU even when it does not move. The racy
 * read-check-write is fine — the frontier is advisory and monotonic enough
 * under time_before() semantics.
 */
void BPF_STRUCT_OPS(cake_running, struct task_struct *p)
{
	u64 task_vtime = p->scx.dsq_vtime;
	u32 cpu = bpf_get_smp_processor_id();
	struct cake_run_slot *run = &cake.run[cpu & (MAX_CPUS - 1)];
	u64 now = bpf_ktime_get_ns();

	run->stamp = now;
	run->sum = p->se.sum_exec_runtime;

	cake_frame_observe(p, now);

	if (time_before(cake.frontier.word, task_vtime))
		cake.frontier.word = task_vtime;
}

/*
 * ops.stopping — charge the wall time used to the task's vtime, weighted by
 * the reciprocal table (no division on the hot path).
 */
void BPF_STRUCT_OPS(cake_stopping, struct task_struct *p, bool runnable)
{
	u32 cpu = bpf_get_smp_processor_id();
	u64 used = p->se.sum_exec_runtime -
		   cake.run[cpu & (MAX_CPUS - 1)].sum;
	u32 idx = cake_recip_index(p);
	struct cake_run_slot *rs = &cake.run[cpu & (MAX_CPUS - 1)];
	u64 hint = 0;

	/*
	 * Count consecutive wake-then-block-quickly quanta. `used` is already
	 * exact here and `runnable` distinguishes blocking from a requeue, so
	 * the test costs no clock read. The counter saturates rather than
	 * latching a single quantum, because a producer trips that by accident
	 * and a real handoff partner repeats it; a preempted task never
	 * finished its pattern, so it leaves the count alone (§R.18).
	 */
	hint = (rs->hint >> CAKE_HINT_CONF_SHIFT) & CAKE_HINT_CONF_MAX;
	if (!runnable) {
		if ((rs->hint & CAKE_HINT_WOKE)) {
			if (used < cake_handoff_max_ns) {
				if (hint < CAKE_HINT_CONF_MAX)
					hint++;
			} else {
				hint = 0;
			}
		} else {
			hint = 0;
		}
	}
	hint <<= CAKE_HINT_CONF_SHIFT;
	if (rs->hint != hint)
		rs->hint = hint;

	/*
	 * Direct write, not scx_bpf_task_set_dsq_vtime(): the kfunc's
	 * sub-scheduler authority check measured +28-36% on this, the hottest
	 * per-switch callback in the scheduler (§R.17).
	 */
	p->scx.dsq_vtime += cake_scale_vtime(used, idx);
}

/*
 * ops.enable — a freshly enabled task starts at the current vtime frontier so
 * it is neither starved nor granted a windfall of credit.
 */
void BPF_STRUCT_OPS(cake_enable, struct task_struct *p)
{
	scx_bpf_task_set_dsq_vtime(p, cake.frontier.word);
}

/*
 * ops.init (sleepable, one-shot): confirm the loader's frozen CPU span covers
 * the kernel's own nr_cpu_ids, then create one custom vtime DSQ per possible
 * CPU, dsq_id == cpu. A span narrower than nr_cpu_ids would stop the steal
 * ring short, so refuse it rather than silently under-scan (§R.21).
 */
s32 BPF_STRUCT_OPS_SLEEPABLE(cake_init)
{
	u32 nr = scx_bpf_nr_cpu_ids();
	s32 i, ret;

	if (nr > MAX_CPUS) {
		scx_bpf_error("nr_cpu_ids %u exceeds Cake MAX_CPUS %u", nr,
			      MAX_CPUS);
		return -EINVAL;
	}
	if (nr_cpu_span < nr) {
		scx_bpf_error("loader CPU span %u is narrower than nr_cpu_ids %u",
			      nr_cpu_span, nr);
		return -EINVAL;
	}

	bpf_for(i, 0, nr) {
		ret = scx_bpf_create_dsq((u64)(u32)i, -1);
		if (ret)
			return ret;
	}
	ret = scx_bpf_create_dsq(WAKE_DSQ, -1);
	if (ret)
		return ret;

	{
		struct bpf_cpumask *mask = bpf_cpumask_create();

		if (!mask)
			return -ENOMEM;
		bpf_for(i, 0, nr) {
			if (!cpu_irq_hot[(u32)i & (MAX_CPUS - 1)])
				bpf_cpumask_set_cpu((u32)i, mask);
		}
		mask = bpf_kptr_xchg(&nonsink_cpumask, mask);
		if (mask)
			bpf_cpumask_release(mask);
	}
	return 0;
}

void BPF_STRUCT_OPS(cake_exit, struct scx_exit_info *ei)
{
	UEI_RECORD(uei, ei);
}

/*
 * ALLOW_QUEUED_WAKEUP lets remote activation ride the batched TTWU queue
 * instead of taking the remote rq lock per wake, so no policy here may depend
 * on process identity or assume enqueue's current is the waker (§R.20).
 */
SCX_OPS_DEFINE(cake_ops,
	       .select_cpu	= (void *)cake_select_cpu,
	       .enqueue		= (void *)cake_enqueue,
	       .dispatch	= (void *)cake_dispatch,
	       .running		= (void *)cake_running,
	       .stopping	= (void *)cake_stopping,
	       .enable		= (void *)cake_enable,
	       .init		= (void *)cake_init,
	       .exit		= (void *)cake_exit,
	       .flags		= SCX_OPS_ALLOW_QUEUED_WAKEUP,
	       .timeout_ms	= WATCHDOG_TIMEOUT_MS,
	       .name		= "cake");
