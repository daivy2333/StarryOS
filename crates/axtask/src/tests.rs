use core::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, Once};

use crate::{AxCpuMask, WaitQueue, api as axtask, current};

static INIT: Once = Once::new();
static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn test_sched_fifo() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;
    static FINISHED_TASKS: AtomicUsize = AtomicUsize::new(0);

    for i in 0..NUM_TASKS {
        axtask::spawn_raw(
            move || {
                println!("sched-fifo: Hello, task {}! ({})", i, current().id_name());
                axtask::yield_now();
                let order = FINISHED_TASKS.fetch_add(1, Ordering::Release);
                assert_eq!(order, i); // FIFO scheduler
            },
            format!("T{i}"),
            0x1000,
        );
    }

    while FINISHED_TASKS.load(Ordering::Acquire) < NUM_TASKS {
        axtask::yield_now();
    }
}

#[test]
fn test_fp_state_switch() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 5;
    const FLOATS: [f64; NUM_TASKS] = [
        std::f64::consts::PI,
        std::f64::consts::E,
        -std::f64::consts::SQRT_2,
        0.0,
        0.618033988749895,
    ];
    static FINISHED_TASKS: AtomicUsize = AtomicUsize::new(0);

    for (i, float) in FLOATS.iter().enumerate() {
        axtask::spawn(move || {
            let mut value = float + i as f64;
            axtask::yield_now();
            value -= i as f64;

            println!("fp_state_switch: Float {i} = {value}");
            assert!((value - float).abs() < 1e-9);
            FINISHED_TASKS.fetch_add(1, Ordering::Release);
        });
    }
    while FINISHED_TASKS.load(Ordering::Acquire) < NUM_TASKS {
        axtask::yield_now();
    }
}

#[test]
fn test_wait_queue() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;

    static WQ1: WaitQueue = WaitQueue::new();
    static WQ2: WaitQueue = WaitQueue::new();
    static COUNTER: AtomicUsize = AtomicUsize::new(0);

    for _ in 0..NUM_TASKS {
        axtask::spawn(move || {
            COUNTER.fetch_add(1, Ordering::Release);
            println!("wait_queue: task {:?} started", current().id());
            WQ1.notify_one(true); // WQ1.wait_until()
            WQ2.wait();

            COUNTER.fetch_sub(1, Ordering::Release);
            println!("wait_queue: task {:?} finished", current().id());
            WQ1.notify_one(true); // WQ1.wait_until()
        });
    }

    println!("task {:?} is waiting for tasks to start...", current().id());
    WQ1.wait_until(|| COUNTER.load(Ordering::Acquire) == NUM_TASKS);
    axtask::yield_now();
    assert_eq!(COUNTER.load(Ordering::Acquire), NUM_TASKS);
    WQ2.notify_all(true); // WQ2.wait()

    println!(
        "task {:?} is waiting for tasks to finish...",
        current().id()
    );
    WQ1.wait_until(|| COUNTER.load(Ordering::Acquire) == 0);
    assert_eq!(COUNTER.load(Ordering::Acquire), 0);
}

#[test]
fn test_task_join() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);

    const NUM_TASKS: usize = 10;
    let mut tasks = Vec::with_capacity(NUM_TASKS);

    for i in 0..NUM_TASKS {
        tasks.push(axtask::spawn_raw(
            move || {
                println!("task_join: task {}! ({})", i, current().id_name());
                axtask::yield_now();
                axtask::exit(i as _);
            },
            format!("T{i}"),
            0x1000,
        ));
    }

    for (i, task) in tasks.into_iter().enumerate() {
        assert_eq!(task.join(), i as _);
    }
}

/// The pure affinity validation seam must accept non-empty masks whose every
/// set bit is within the online/schedulable CPU count, and reject empty,
/// offline and out-of-range masks across a full set of representative sizes.
#[test]
fn validate_affinity_accepts_only_online_schedulable_masks() {
    for cpu_num in [1usize, 2, 3, 4, 8, 16] {
        for idx in 0..cpu_num {
            let one = AxCpuMask::one_shot(idx);
            assert!(
                axtask::validate_affinity(one, cpu_num),
                "singleton {idx} among {cpu_num}"
            );
            // Full mask is valid for any non-zero cpu count (all bits < cpu_num).
            let mut full = AxCpuMask::new();
            for c in 0..cpu_num {
                full.set(c, true).expect("index below mask capacity");
            }
            assert!(axtask::validate_affinity(full, cpu_num));
        }
    }
}

#[test]
fn validate_affinity_rejects_empty_mask() {
    for cpu_num in [1usize, 2, 3, 4, 8, 16] {
        assert!(!axtask::validate_affinity(AxCpuMask::new(), cpu_num));
    }
}

#[test]
fn validate_affinity_rejects_out_of_range_and_offline_bits() {
    // A bit at or above the online count is offline/not-yet-initialized: reject.
    // Indices stay below the configured MAX_CPU_NUM (16) so the mask is
    // constructible; the reject decision is about cpu_num, not the mask width.
    for cpu_num in [1usize, 2, 3, 4, 8] {
        // bit at index == cpu_num is the first offline/schedulable-excluded CPU.
        assert!(
            !axtask::validate_affinity(AxCpuMask::one_shot(cpu_num), cpu_num),
            "bit at index cpu_num is offline for count {cpu_num}"
        );
    }
    // A far index within the mask width but >= any online count must be rejected.
    for cpu_num in [1usize, 2, 3, 4, 8, 16] {
        if cpu_num < 15 {
            assert!(
                !axtask::validate_affinity(AxCpuMask::one_shot(15), cpu_num),
                "index 15 is out of range for count {cpu_num}"
            );
        }
    }
}

#[test]
fn validate_affinity_rejects_sparse_mask_with_offline_gap() {
    // cpu_num=8, mask = {cpus 0 and 5} is valid (both < 8).
    let mut ok = AxCpuMask::new();
    ok.set(0, true).expect("index below mask capacity");
    ok.set(5, true).expect("index below mask capacity");
    assert!(axtask::validate_affinity(ok, 8));
    // cpu_num=7 with cpu 7 set: 7 is not < 7 -> offline.
    assert!(!axtask::validate_affinity(AxCpuMask::one_shot(7), 7));
}

/// A task spawned with the pre-enqueue affinity API can run when the singleton
/// mask is schedulable (its run queue has been published by init).
#[test]
fn spawn_with_affinity_runs_when_mask_valid() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);
    static DONE: AtomicUsize = AtomicUsize::new(0);
    DONE.store(0, Ordering::Release);
    let task = axtask::spawn_raw_with_affinity(
        || {
            DONE.fetch_or(1, Ordering::Release);
        },
        "affine".into(),
        0x1000,
        AxCpuMask::one_shot(0),
    );
    let Some(task) = task else {
        panic!("singleton cpu0 mask must be valid when cpu0 is online");
    };
    while DONE.load(Ordering::Acquire) == 0 {
        axtask::yield_now();
    }
    assert_eq!(task.join(), 0);
}

/// An invalid affinity must fail closed at every boundary: an out-of-capacity
/// index is rejected by checked construction, and an in-capacity but
/// not-yet-schedulable mask is rejected by the pre-enqueue spawn with no task
/// created.
#[test]
fn spawn_with_affinity_rejects_invalid_mask() {
    assert!(
        AxCpuMask::try_one_shot(MASK_CAP).is_err(),
        "out-of-capacity index must fail closed at construction"
    );
    assert!(
        AxCpuMask::try_one_shot(usize::MAX).is_err(),
        "usize::MAX index must fail closed at construction"
    );
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);
    assert!(
        axtask::spawn_raw_with_affinity(
            || {},
            "bad".into(),
            0x1000,
            AxCpuMask::one_shot(MASK_CAP - 1)
        )
        .is_none(),
        "in-capacity but unpublished affinity must fail closed with no enqueued task"
    );
}

/// Schedulable-run-queue selection only ever picks a CPU that is present in
/// *both* the task affinity and the published readiness snapshot (round-robin
/// over the intersection).
#[test]
fn select_schedulable_cpu_intersects_affinity_with_ready() {
    let mut task = AxCpuMask::new();
    for c in 0..4 {
        task.set(c, true).expect("index below mask capacity");
    }
    let mut sched = AxCpuMask::new();
    sched.set(1, true).expect("index below mask capacity");
    sched.set(3, true).expect("index below mask capacity");
    for seed in [0usize, 1, 3, 7, 16, 999] {
        let (idx, _next) = crate::run_queue::select_schedulable_cpu(task, sched, seed)
            .expect("non-empty intersection must yield a CPU");
        assert!(
            idx == 1 || idx == 3,
            "selected cpu {idx} is not in the ready intersection"
        );
    }
}

/// An empty intersection (or an empty task mask) must fail closed and never
/// return an uninitialized run queue index.
#[test]
fn select_schedulable_cpu_empty_intersection_fails_closed() {
    let sched = AxCpuMask::one_shot(7);
    assert!(
        crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(0), sched, 0).is_none(),
        "task affinity cpu 0 not ready => no candidate"
    );
    assert!(
        crate::run_queue::select_schedulable_cpu(AxCpuMask::new(), sched, 0).is_none(),
        "empty task mask => fail closed"
    );
}

/// A sparse ready set only allows its own (published) members, never a
/// configured-but-unpublished neighbour.
#[test]
fn select_schedulable_cpu_sparse_picks_published_only() {
    let sched = AxCpuMask::one_shot(15);
    let (idx, _next) = crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(15), sched, 0)
        .expect("published singleton is selectable");
    assert_eq!(idx, 15);
    assert!(
        crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(14), sched, 0).is_none(),
        "unpublished neighbour 14 is not selectable"
    );
}

/// Pure model of the `write slot → Release publish → Acquire observe → select`
/// invariant for *one* newly published bit. Both snapshots are ordinary masks
/// built here; the "after" set is the "before" set plus the just-published cpu.
/// This never mutates the real global `SCHEDULABLE` (and so can never fabricate
/// a "published but uninitialized" run queue): the pre-publish snapshot excludes
/// cpu 15, the post-publish snapshot includes it, and selection tracks exactly
/// that transition while leaving already-published members unchanged.
#[test]
fn schedulable_publish_model_flips_only_target_bit() {
    let mut before = AxCpuMask::new();
    for c in 0usize..4 {
        before.set(c, true).expect("index below mask capacity");
    }
    let mut after = before;
    after.set(15, true).expect("index below mask capacity");

    // cpu 15 absent from the snapshot → not selectable even with affinity.
    assert!(
        crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(15), before, 0).is_none(),
        "unpublished cpu15 must be excluded before publish"
    );
    // Exactly when its bit is published the cpu becomes selectable.
    let (idx, _) = crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(15), after, 0)
        .expect("cpu15 selectable after publish");
    assert_eq!(idx, 15);
    // Already-published members are unaffected by publishing another bit.
    for c in 0usize..4 {
        let (b, _) = crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(c), before, 0)
            .expect("published member selectable before");
        assert_eq!(b, c);
        let (a, _) = crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(c), after, 0)
            .expect("published member selectable after");
        assert_eq!(a, c);
    }
}

/// Extremely large `seed` values (raw `RUN_QUEUE_INDEX` counts) must probe the
/// intersection deterministically: selection must reduce the seed modulo the
/// CPU count *before* probing, so a `usize`-wide seed that would wrap when
/// `+ offset` still lands on a valid schedulable CPU rather than depending on
/// the wrapped intermediate value. This is load-bearing for non-power-of-two
/// CPU counts (e.g. the `1/2/3/4/8/16` sparse sets the policy must support).
#[test]
fn select_schedulable_cpu_big_wrapped_seed_picks_valid_cpu() {
    let task = AxCpuMask::one_shot(3);
    let sched = AxCpuMask::one_shot(3);
    let (idx, next) = crate::run_queue::select_schedulable_cpu(task, sched, usize::MAX - 1)
        .expect("wrapped seed must still select the published cpu");
    assert_eq!(idx, 3, "wrapped seed must not lose the schedulable bit");
    assert!(
        next < axconfig::plat::MAX_CPU_NUM,
        "next seed stays in range"
    );
}

/// An explicit affinity update that includes a configured-but-unpublished CPU
/// must fail closed and leave the previous mask unchanged.
#[test]
fn explicit_affinity_rejects_unpublished_cpu_and_preserves_old_mask() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);
    // cpu 14 is configured (16 CPUs) but never published as schedulable here.
    let task =
        axtask::spawn_raw_with_affinity(|| {}, "adhoc".into(), 0x1000, AxCpuMask::one_shot(0))
            .expect("cpu0 is published by init_scheduler");
    task.set_cpumask(AxCpuMask::one_shot(0));
    assert!(
        !task.set_cpumask_checked(AxCpuMask::one_shot(14)),
        "unpublished cpu14 must be rejected"
    );
    assert_eq!(
        task.cpumask(),
        AxCpuMask::one_shot(0),
        "old mask preserved on rejected update"
    );
}

// ── Wake-target locality (Cycle 003 / 2.6-R3) ──────────────────────────
//
// A `block_on`-parked task is woken on whatever hart happens to observe the
// publication. To close the remote-ready-IPI gap without an IPI storm, wake
// target selection is *separate* from plain-spawn round-robin: it must prefer
// the current hart when that hart is both in the task's affinity and published
// schedulable (full-mask tasks stay local, so no remote IPI), otherwise pick a
// deterministic legal cushion from `affinity ∩ schedulable` (a pinned copier
// woken from a remote hart is delivered to its one allowed hart, so the remote
// wake triggers exactly one IPI). An empty intersection fails closed.

/// Builds the mask with exactly the given bits set (helper for readability).
fn mask(bits: &[usize]) -> AxCpuMask {
    let mut m = AxCpuMask::new();
    for b in bits {
        m.set(*b, true).expect("index below mask capacity");
    }
    m
}

/// A full-mask task woken on a legal, schedulable hart must be enqueued locally
/// (current hart preferred), so a frequent full-mask task never migrates or
/// raises a remote-reschedule IPI.
#[test]
fn wake_target_prefers_current_hart_when_legal() {
    let range = [0usize, 1, 2, 3, 4, 15];
    let task = mask(&range);
    for cur in range {
        let sched = mask(&range);
        assert_eq!(
            crate::run_queue::select_wake_cpu(task, sched, cur).expect("affinity is schedulable"),
            cur,
            "waking hart {cur} must be chosen locally"
        );
    }
}

/// A singleton-affinity task woken from a remote hart (its affinity hart not
/// being the current one) is delivered to its one allowed hart. This is the
/// pinned TX-copier case: the wake must target the remote singleton.
#[test]
fn wake_target_pinned_task_with_remote_current_selects_its_singleton() {
    let task = AxCpuMask::one_shot(7);
    let sched = mask(&[3, 7]);
    for cur in [0usize, 3, 15] {
        if cur != 7 {
            assert_eq!(
                crate::run_queue::select_wake_cpu(task, sched, cur)
                    .expect("singleton is schedulable"),
                7,
                "pinned task woken from {cur} must target its allowance"
            );
        }
    }
}

/// A sparse schedulable intersection whose current hart is present must be
/// selected even when it is not the lowest-bit member.
#[test]
fn wake_target_sparse_with_current_prefers_current_over_lowest_bit() {
    let task = mask(&[2, 5, 9]);
    let sched = mask(&[2, 5, 9]);
    assert_eq!(
        crate::run_queue::select_wake_cpu(task, sched, 5).expect("dense"),
        5
    );
    assert_eq!(
        crate::run_queue::select_wake_cpu(task, sched, 2).expect("dense"),
        2
    );
    assert_eq!(
        crate::run_queue::select_wake_cpu(task, sched, 9).expect("dense"),
        9
    );
    // Current hart not in the (task or schedulable) set → deterministic lowest bit.
    assert_eq!(
        crate::run_queue::select_wake_cpu(task, sched, 0).expect("intersection non-empty"),
        2
    );
}

/// A wake from a hart that is not schedulable (offline / not yet published)
/// must fall back to a deterministic legal target and never return that
/// offline current hart.
#[test]
fn wake_target_offline_current_falls_back_to_schedulable() {
    let task = mask(&[0, 3, 7]);
    let sched = mask(&[3, 7]);
    assert_eq!(
        crate::run_queue::select_wake_cpu(task, sched, 0)
            .expect("allowed schedulable target exists"),
        3,
        "current hart 0 not schedulable; lowest legal member (3) is chosen"
    );
}

/// An empty intersection (empty task affinity, or no schedulable hart in the
/// affinity) must fail closed and never yield an uninitialized run queue index.
#[test]
fn wake_target_empty_intersection_fails_closed() {
    assert!(crate::run_queue::select_wake_cpu(AxCpuMask::new(), AxCpuMask::full(), 0).is_none());
    let sched = AxCpuMask::one_shot(15);
    assert!(
        crate::run_queue::select_wake_cpu(AxCpuMask::one_shot(0), sched, 0).is_none(),
        "cpu 0 in affinity but not schedulable"
    );
}

/// The wake target must never be a flat schedulable bit that is outside the
/// task's affinity, and selecting it never mutates the input masks.
#[test]
fn wake_target_never_selects_outside_affinity_or_mutates_inputs() {
    let task = AxCpuMask::one_shot(4);
    let mut sched = AxCpuMask::new();
    for c in [0usize, 1, 2, 4] {
        sched.set(c, true).expect("index below mask capacity");
    }
    let snap_task = task;
    let snap_sched = sched;
    for cur in [0usize, 1, 2, 7] {
        let got = crate::run_queue::select_wake_cpu(task, sched, cur).expect("affinity in sched");
        assert_eq!(got, 4, "cpu 4 is the only affinity member");
        assert!(
            task.get(got) && sched.get(got),
            "target in affinity ∩ schedulable"
        );
    }
    assert_eq!(task, snap_task, "task mask must not be mutated");
    assert_eq!(sched, snap_sched, "sched mask must not be mutated");
    assert!(
        crate::run_queue::select_wake_cpu(AxCpuMask::one_shot(4), AxCpuMask::one_shot(3), 0)
            .is_none(),
        "affinity member not schedulable"
    );
}

// ===== Task 4.1: AxCpuMask release-safe capacity boundary =====
//
// These witnesses intentionally compile against both the legacy public
// `cpumask::CpuMask` alias and the bounded workspace newtype so the RED run
// demonstrates the *behavioral* release defect (garbage bit / silent
// corruption) instead of a mere signature change.

/// Capacity of the public mask under the repository configuration.
const MASK_CAP: usize = axconfig::plat::MAX_CPU_NUM;

/// A complete observation of every externally visible mask property: backing
/// bytes, bit count and iteration order. An out-of-range mutation must leave
/// all three unchanged.
fn mask_snapshot(m: &AxCpuMask) -> (Vec<u8>, usize, Vec<usize>) {
    (m.as_bytes().to_vec(), m.len(), m.into_iter().collect())
}

#[test]
fn mask_oob_read_is_false_and_non_mutating() {
    let mut m = AxCpuMask::new();
    let _ = m.set(0, true);
    let before = mask_snapshot(&m);
    assert!(
        !m.get(MASK_CAP),
        "membership read at the capacity boundary must be false"
    );
    assert!(
        !m.get(usize::MAX),
        "membership read at usize::MAX must be false"
    );
    assert_eq!(
        mask_snapshot(&m),
        before,
        "a membership read must never mutate the mask"
    );
}

#[test]
fn mask_oob_write_preserves_all_legal_state() {
    let mut m = AxCpuMask::new();
    let _ = m.set(0, true);
    let _ = m.set(5, true);
    let before = mask_snapshot(&m);

    // Setting an out-of-range bit must not alias onto an in-range bit.
    let _ = m.set(MASK_CAP, true);
    assert_eq!(
        mask_snapshot(&m),
        before,
        "out-of-range set at capacity corrupted legal mask state"
    );
    let _ = m.set(usize::MAX, true);
    assert_eq!(
        mask_snapshot(&m),
        before,
        "out-of-range set at usize::MAX corrupted legal mask state"
    );

    // Clearing an out-of-range bit must not alias onto an in-range bit.
    let mut m2 = AxCpuMask::new();
    let _ = m2.set(0, true);
    let _ = m2.set(5, true);
    let before2 = mask_snapshot(&m2);
    let _ = m2.set(MASK_CAP, false);
    assert_eq!(
        mask_snapshot(&m2),
        before2,
        "out-of-range clear at capacity corrupted legal mask state"
    );
    let _ = m2.set(usize::MAX, false);
    assert_eq!(
        mask_snapshot(&m2),
        before2,
        "out-of-range clear at usize::MAX corrupted legal mask state"
    );
}

/// Legal operations must behave exactly as the registry type did: construction,
/// read/write round-trip, length, iteration order and set algebra.
#[test]
fn mask_legal_operations_remain_compatible() {
    // Construction.
    assert_eq!(AxCpuMask::new().len(), 0);
    assert!(AxCpuMask::new().is_empty());
    assert!(AxCpuMask::full().is_full());
    let single = AxCpuMask::one_shot(0);
    assert!(single.get(0));
    assert_eq!(single.len(), 1);
    let top = AxCpuMask::one_shot(MASK_CAP - 1);
    assert!(top.get(MASK_CAP - 1));
    assert_eq!(top.into_iter().collect::<Vec<_>>(), vec![MASK_CAP - 1]);

    // Read/write round-trip and previous-value reporting.
    let mut m = AxCpuMask::new();
    let _ = m.set(3, true);
    assert!(m.get(3));
    assert_eq!(m.len(), 1);

    // Iteration order is ascending.
    let mut it = AxCpuMask::new();
    for i in [1usize, 5, 9] {
        let _ = it.set(i, true);
    }
    assert_eq!(it.into_iter().collect::<Vec<_>>(), vec![1, 5, 9]);
    assert_eq!(it.last_index(), Some(9));
    assert_eq!(it.first_index(), Some(1));

    // Set algebra matches bit semantics across the whole capacity.
    let mut a = AxCpuMask::new();
    let _ = a.set(1, true);
    let _ = a.set(5, true);
    let mut b = AxCpuMask::new();
    let _ = b.set(5, true);
    let _ = b.set(9, true);
    let union = a | b;
    let inter = a & b;
    let xor = a ^ b;
    let not_a = !a;
    for i in 0..MASK_CAP {
        assert_eq!(union.get(i), i == 1 || i == 5 || i == 9);
        assert_eq!(inter.get(i), i == 5);
        assert_eq!(xor.get(i), i == 1 || i == 9);
        assert_eq!(not_a.get(i), !(i == 1 || i == 5));
    }
    let mut acc = a;
    acc |= b;
    assert_eq!(acc, union);
    acc &= a;
    assert_eq!(acc, a);
    acc ^= b;
    assert_eq!(acc, xor);

    // Copy/equality snapshots used by the scheduler and snapshots.
    let copied = a;
    assert_eq!(copied, a);
    assert_ne!(a, b);

    // Wire representation is one bit per capacity slot.
    assert_eq!(AxCpuMask::new().as_bytes().len(), MASK_CAP / 8);
}

/// Structural guard: the public mask must be the bounded workspace struct, not
/// the registry alias, and no raw-inner escape may exist.
#[test]
fn ax_cpu_mask_is_bounded_struct_without_raw_escape_in_source() {
    let api = include_str!("api.rs");
    assert!(
        !api.contains("pub type AxCpuMask"),
        "the public registry alias must be replaced by the bounded newtype"
    );
    let mask_mod = include_str!("cpumask.rs");
    assert!(
        mask_mod.contains("pub struct AxCpuMask"),
        "AxCpuMask must be a workspace-owned struct"
    );
    assert!(
        mask_mod.contains("pub enum AxCpuMaskError"),
        "out-of-range writes must have a distinct error type"
    );
    for forbidden in [
        "impl Deref",
        "impl AsRef",
        "From<AxCpuMask> for cpumask",
        "pub fn as_inner",
        "pub fn into_inner",
    ] {
        assert!(
            !mask_mod.contains(forbidden),
            "raw registry escape must not exist: {forbidden}"
        );
    }
    // Capacity checks must be unconditional, not debug-only assertions.
    assert!(
        mask_mod.contains("if index >= AX_CPU_MASK_CAPACITY"),
        "indexed access must test the capacity at runtime in every build"
    );
}

/// GREEN-only witnesses (Task 4.1): the distinct-error write contract and the
/// checked construction surface that the alias could not express.
#[test]
fn mask_oob_write_returns_distinct_error_without_mutation() {
    use crate::AxCpuMaskError;

    let mut m = mask(&[1, 5]);
    let before = mask_snapshot(&m);
    assert!(
        matches!(
            m.set(MASK_CAP, true),
            Err(AxCpuMaskError::IndexOutOfRange { index, capacity })
                if index == MASK_CAP && capacity == MASK_CAP
        ),
        "write at capacity must return a distinct out-of-range error"
    );
    assert!(
        matches!(
            m.set(usize::MAX, true),
            Err(AxCpuMaskError::IndexOutOfRange { index, capacity })
                if index == usize::MAX && capacity == MASK_CAP
        ),
        "write at usize::MAX must return a distinct out-of-range error"
    );
    assert!(
        matches!(
            m.set(MASK_CAP, false),
            Err(AxCpuMaskError::IndexOutOfRange { .. })
        ),
        "clear at capacity must also be rejected"
    );
    assert_eq!(
        mask_snapshot(&m),
        before,
        "a rejected write must leave every legal bit, length and byte unchanged"
    );

    // Legal writes keep the previous-bit contract of the legacy API.
    assert_eq!(m.set(2, true), Ok(false), "first set reports previous false");
    assert_eq!(m.set(2, true), Ok(true), "repeat set reports previous true");
    assert_eq!(m.set(2, false), Ok(true), "clear reports previous true");
    assert_eq!(m.len(), 2);
}

#[test]
fn mask_checked_construction_covers_dynamic_inputs() {
    for idx in [MASK_CAP, MASK_CAP + 1, usize::MAX] {
        assert!(
            AxCpuMask::try_one_shot(idx).is_err(),
            "dynamic index {idx} must fail closed"
        );
    }
    let m = AxCpuMask::try_one_shot(0).expect("index 0 is always legal");
    assert_eq!(m.into_iter().collect::<Vec<_>>(), vec![0]);
    let m = AxCpuMask::try_one_shot(MASK_CAP - 1).expect("top legal index");
    assert!(m.get(MASK_CAP - 1));
    assert!(!m.get(MASK_CAP));
}

/// The newtype stays transparent in size so scheduler/snapshot storage layout
/// is unchanged, while the raw registry type is not nameable through it.
#[test]
fn mask_newtype_is_transparent_in_size() {
    assert_eq!(
        core::mem::size_of::<AxCpuMask>(),
        core::mem::size_of::<cpumask::CpuMask<MASK_CAP>>(),
        "the wrapper must not change mask storage layout"
    );
}
