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
            assert!(axtask::validate_affinity(one, cpu_num), "singleton {idx} among {cpu_num}");
            // Full mask is valid for any non-zero cpu count (all bits < cpu_num).
            let mut full = AxCpuMask::new();
            for c in 0..cpu_num {
                full.set(c, true);
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
    ok.set(0, true);
    ok.set(5, true);
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

/// An invalid (out-of-range) affinity in the pre-enqueue API must fail closed:
/// no task is created and the caller can detect the rejection.
#[test]
fn spawn_with_affinity_rejects_invalid_mask() {
    assert!(
        axtask::spawn_raw_with_affinity(|| {}, "bad".into(), 0x1000, AxCpuMask::one_shot(63))
            .is_none(),
        "out-of-range affinity must fail closed with no enqueued task"
    );
}

/// Schedulable-run-queue selection only ever picks a CPU that is present in
/// *both* the task affinity and the published readiness snapshot (round-robin
/// over the intersection).
#[test]
fn select_schedulable_cpu_intersects_affinity_with_ready() {
    let mut task = AxCpuMask::new();
    for c in 0..4 {
        task.set(c, true);
    }
    let mut sched = AxCpuMask::new();
    sched.set(1, true);
    sched.set(3, true);
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
        before.set(c, true);
    }
    let mut after = before;
    after.set(15, true);

    // cpu 15 absent from the snapshot → not selectable even with affinity.
    assert!(
        crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(15), before, 0).is_none(),
        "unpublished cpu15 must be excluded before publish"
    );
    // Exactly when its bit is published the cpu becomes selectable.
    let (idx, _) =
        crate::run_queue::select_schedulable_cpu(AxCpuMask::one_shot(15), after, 0)
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
    assert!(next < axconfig::plat::MAX_CPU_NUM, "next seed stays in range");
}

/// An explicit affinity update that includes a configured-but-unpublished CPU
/// must fail closed and leave the previous mask unchanged.
#[test]
fn explicit_affinity_rejects_unpublished_cpu_and_preserves_old_mask() {
    let _lock = SERIAL.lock();
    INIT.call_once(axtask::init_scheduler);
    // cpu 14 is configured (16 CPUs) but never published as schedulable here.
    let task = axtask::spawn_raw_with_affinity(|| {}, "adhoc".into(), 0x1000, AxCpuMask::one_shot(0))
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
