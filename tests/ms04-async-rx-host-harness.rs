//! MS04 host harness: critical-section restore policy bound to production.
//!
//! Compiled and executed by `make host-test`:
//!   rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs \
//!     -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
//!
//! The harness includes the *same* `critical_section_policy.rs` file that the
//! kernel compiles as `crate::critical_section_policy`. The kernel's
//! `critical_impl` delegates its `critical_section::Impl` acquire/release to
//! the seam's `acquire`/`release` through an `axhal` backend; here a fake
//! backend records the simulated IRQ state and call counts. Both paths execute
//! the same two functions, so these tests witness the exact production restore
//! decision logic.
//!
//! RED state: against the pre-iteration seam (dead `IrqRestorePolicy` model,
//! no `IrqOps`/`acquire`/`release` API) this harness fails to compile.
//! GREEN state: all six unique scenarios pass.

#[path = "../kernel/src/critical_section_policy.rs"]
mod critical_section_policy;

/// Stub for the kernel's `axconfig` crate: the placement policy's capacity bound
/// is `axconfig::plat::MAX_CPU_NUM`. The stub value is deliberately 16 (the
/// QEMU SMP=16 qualification platform) and DIFFERS from the old hardcoded 64,
/// so any re-introduction of a hardcoded capacity in `placement.rs` fails the
/// `placement_capacity_matches_axconfig` guard below. `extern crate self`
/// gives the harness crate the `axconfig` name that `placement.rs` references
/// in type position.
pub mod plat {
    pub const MAX_CPU_NUM: usize = 16;
}
extern crate self as axconfig;

#[path = "../kernel/src/drivers/placement.rs"]
mod placement;

#[path = "../kernel/src/drivers/hart_counter.rs"]
mod hart_counter;

#[path = "../kernel/src/drivers/uart_snapshot_types.rs"]
mod uart_snapshot_types;

#[path = "../kernel/src/drivers/virtio_net_irq_logic.rs"]
mod virtio_net_irq_logic;

#[path = "../kernel/src/drivers/net_wake_witness_logic.rs"]
mod net_wake_witness_logic;

#[path = "../kernel/src/drivers/uart_migration_logic.rs"]
mod uart_migration_logic;

use core::cell::Cell;
use std::{
    sync::{
        Arc,
        atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering as StdOrdering},
    },
    thread,
};

use critical_section_policy::{IrqOps, MAX_CPU_NUM, acquire, checked_increment_depth, release};

/// Fake IRQ backend: simulates this hart's IRQ enable state and records
/// disable/enable call counts. Each instance represents one hart (identified
/// by `cpu_id`); the policy's static `NEST_DEPTH`/`GLOBAL_LOCK` are shared
/// across instances/threads, so distinct `cpu_id`s model distinct harts.
#[derive(Default)]
struct FakeIrqOps {
    irqs_enabled: Cell<bool>,
    disable_calls: Cell<u32>,
    enable_calls: Cell<u32>,
    cpu_id: usize,
}

impl FakeIrqOps {
    fn new(irqs_enabled: bool) -> Self {
        Self {
            irqs_enabled: Cell::new(irqs_enabled),
            cpu_id: 0,
            ..Self::default()
        }
    }

    fn with_cpu(cpu_id: usize, irqs_enabled: bool) -> Self {
        Self {
            irqs_enabled: Cell::new(irqs_enabled),
            cpu_id,
            ..Self::default()
        }
    }

    fn cpu_id(&self) -> usize {
        self.cpu_id
    }

    fn enable_calls(&self) -> u32 {
        self.enable_calls.get()
    }

    fn disable_calls(&self) -> u32 {
        self.disable_calls.get()
    }
}

impl IrqOps for FakeIrqOps {
    fn irqs_enabled(&self) -> bool {
        self.irqs_enabled.get()
    }

    fn disable_irqs(&self) {
        self.irqs_enabled.set(false);
        self.disable_calls.set(self.disable_calls.get() + 1);
    }

    fn enable_irqs(&self) {
        self.irqs_enabled.set(true);
        self.enable_calls.set(self.enable_calls.get() + 1);
    }

    fn current_cpu_id(&self) -> usize {
        self.cpu_id
    }
}

#[test]
fn enabled_acquire_disables_and_release_reenables_once() {
    let ops = FakeIrqOps::with_cpu(1, true);
    let was_enabled = acquire(&ops);
    assert!(was_enabled);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.disable_calls(), 1);
    release(&ops, was_enabled);
    assert!(ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 1);
}

#[test]
fn isr_entry_acquire_returns_false_and_release_never_enables() {
    let ops = FakeIrqOps::with_cpu(2, false);
    let was_enabled = acquire(&ops);
    assert!(!was_enabled);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.disable_calls(), 1);
    release(&ops, was_enabled);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 0);
}

#[test]
fn nested_acquire_only_outermost_release_reenables() {
    let ops = FakeIrqOps::with_cpu(3, true);
    let outer = acquire(&ops);
    assert!(outer);
    let inner = acquire(&ops);
    assert!(!inner);
    assert!(!ops.irqs_enabled.get());
    release(&ops, inner);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 0);
    release(&ops, outer);
    assert!(ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 1);
    assert_eq!(ops.disable_calls(), 2);
}

#[test]
fn nested_isr_context_never_enables() {
    let ops = FakeIrqOps::with_cpu(4, false);
    let outer = acquire(&ops);
    assert!(!outer);
    let inner = acquire(&ops);
    assert!(!inner);
    release(&ops, inner);
    release(&ops, outer);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 0);
    assert_eq!(ops.disable_calls(), 2);
}

#[test]
fn release_false_never_enables_irqs() {
    let ops = FakeIrqOps::with_cpu(5, true);
    let was_enabled = acquire(&ops);
    release(&ops, !was_enabled);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 0);
}

#[test]
fn acquire_always_disables_irqs() {
    let ops = FakeIrqOps::with_cpu(6, true);
    let outer = acquire(&ops);
    acquire(&ops);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.disable_calls(), 2);
    assert_eq!(ops.enable_calls(), 0);
    // Balance releases so the shared global lock is not leaked to later tests.
    release(&ops, false);
    release(&ops, outer);
}

/// Two harts (distinct cpu ids) must never simultaneously hold the critical
/// section across threads: the per-hart depth gates through the shared global
/// owner lock, so the observed maximum concurrency over many iterations is 1.
#[test]
fn cross_hart_mutual_exclusion_single_concurrent_owner() {
    const ITERS: usize = 2000;
    let inside = Arc::new(AtomicUsize::new(0));
    let max_concurrent = Arc::new(AtomicUsize::new(0));
    let failed = Arc::new(AtomicU32::new(0));

    let mut handles = Vec::new();
    for cpu in [10usize, 11] {
        let inside = Arc::clone(&inside);
        let max = Arc::clone(&max_concurrent);
        let failed = Arc::clone(&failed);
        handles.push(thread::spawn(move || {
            let ops = FakeIrqOps::with_cpu(cpu, true);
            for _ in 0..ITERS {
                let was_enabled = acquire(&ops);
                debug_assert!(was_enabled);
                let cur = inside.fetch_add(1, StdOrdering::SeqCst) + 1;
                max.fetch_max(cur, StdOrdering::SeqCst);
                if cur > 1 {
                    failed.fetch_add(1, StdOrdering::SeqCst);
                }
                inside.fetch_sub(1, StdOrdering::SeqCst);
                release(&ops, was_enabled);
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        failed.load(StdOrdering::Relaxed),
        0,
        "two harts in critical section"
    );
    assert!(max_concurrent.load(StdOrdering::SeqCst) <= 1);
}

/// cpu 1 (another hart) must remain blocked until cpu 0 releases the global
/// owner lock. The occupant is published *inside* the critical section, so the
/// causal claim (cpu 1 can only enter after cpu 0 cleared the section) has no
/// post-unlock window in which a software flag could disagree with ownership.
#[test]
fn cross_hart_second_acquire_waits_for_first_release() {
    use std::time::Duration;

    let inside = Arc::new(AtomicUsize::new(0));

    let t0 = {
        let inside = Arc::clone(&inside);
        thread::spawn(move || {
            let ops = FakeIrqOps::with_cpu(0, true);
            let was_enabled = acquire(&ops);
            // Publish the occupant while still holding the lock.
            inside.store(1, StdOrdering::SeqCst);
            let nested = acquire(&ops);
            assert!(!nested);
            release(&ops, nested);
            // Sleep while owning the section so cpu 1 is provably blocked.
            thread::sleep(Duration::from_millis(20));
            // Publish "released" *before* releasing ownership (still inside the
            // lock), so there is no window where cpu 1 could enter while this
            // flag still claims occupancy.
            inside.store(0, StdOrdering::SeqCst);
            release(&ops, was_enabled);
        })
    };

    // Wait until cpu 0 provably owns the section before cpu 1 tries.
    while inside.load(StdOrdering::SeqCst) != 1 {
        thread::yield_now();
    }

    let t1 = {
        let inside = Arc::clone(&inside);
        thread::spawn(move || {
            let ops = FakeIrqOps::with_cpu(1, true);
            let was_enabled = acquire(&ops);
            // Mutual exclusion guarantees cpu 0 already cleared the occupant
            // (it set 0 before releasing the global lock that gates this entry).
            assert_eq!(
                inside.load(StdOrdering::SeqCst),
                0,
                "cpu 1 entered while cpu 0 still held the critical section"
            );
            inside.fetch_add(1, StdOrdering::SeqCst);
            release(&ops, was_enabled);
        })
    };

    t0.join().unwrap();
    t1.join().unwrap();
    assert_eq!(
        inside.load(StdOrdering::SeqCst),
        1,
        "cpu 0 released exactly once"
    );
}

/// The nesting-depth transition must reject exactly at the real `u32` ceiling,
/// not at any artificial non-production bound. `checked_increment_depth` is the
/// pure seam `acquire` hydrates through `fetch_update`; exercising it directly
/// at the width boundary witnesses the overflow fail-closed transition without
/// driving any live counter to `u32::MAX`.
#[test]
fn checked_increment_depth_fails_closed_at_u32_overflow() {
    assert_eq!(checked_increment_depth(0), Some(1));
    assert_eq!(checked_increment_depth(u32::MAX - 1), Some(u32::MAX));
    // One past the `u32` width: `None` means the transition is rejected, so
    // `acquire`'s `fetch_update` leaves the counter untouched.
    assert_eq!(checked_increment_depth(u32::MAX), None);
}

/// The exact atomic mutation `acquire` performs must not store on overflow:
/// `fetch_update` with the seam returns `Err(prev)` and leaves the counter (and
/// therefore the global owner and the already-disabled IRQ state) unchanged.
/// This is the reachable equivalent of the fail-closed abort the old depth-cap
/// test drove only at an artificial 64-level bound.
#[test]
fn fetch_update_with_seam_preserves_counter_on_overflow() {
    let depth = std::sync::atomic::AtomicU32::new(u32::MAX);
    let result = depth.fetch_update(
        StdOrdering::AcqRel,
        StdOrdering::Acquire,
        checked_increment_depth,
    );
    assert_eq!(
        result,
        Err(u32::MAX),
        "overflowing nesting must be rejected, not wrapped"
    );
    assert_eq!(
        depth.load(StdOrdering::Acquire),
        u32::MAX,
        "counter untouched on overflow"
    );
}

/// Releasing a depth of zero (no matching acquire on this hart) must fail
/// closed with a panic rather than corrupting another hart's ownership.
#[test]
#[should_panic(expected = "depth underflow")]
fn release_without_acquire_fails_closed() {
    let ops = FakeIrqOps::with_cpu(12, true);
    release(&ops, true);
}

/// A hart id at or above MAX_CPU_NUM must fail closed, never index UB.
#[test]
#[should_panic(expected = "exceeds MAX_CPU_NUM")]
fn out_of_range_cpu_id_fails_closed() {
    let ops = FakeIrqOps::with_cpu(MAX_CPU_NUM, true);
    let _ = acquire(&ops);
}

const LEGACY_DIRECT_CALL_IMPL: &str = r#"
struct KernelCriticalSection;

critical_section::set_impl!(KernelCriticalSection);

unsafe impl critical_section::Impl for KernelCriticalSection {
    unsafe fn acquire() -> critical_section::RawRestoreState {
        let was_enabled = irqs_enabled();
        disable_irqs();
        was_enabled
    }

    unsafe fn release(restore_state: critical_section::RawRestoreState) {
        if restore_state {
            enable_irqs();
        }
    }
}
"#;

const TRUNCATED_IMPL: &str = r#"
unsafe impl critical_section::Impl for KernelCriticalSection {
}
"#;

const PRODUCTION_SOURCE: &str = include_str!("../kernel/src/lib.rs");

#[test]
fn legacy_direct_call_impl_is_rejected() {
    assert!(
        production_guard::check(LEGACY_DIRECT_CALL_IMPL).is_err(),
        "direct axhal restore must be rejected"
    );
    assert!(
        production_guard::check(TRUNCATED_IMPL).is_err(),
        "truncated impl must be rejected"
    );
}

#[test]
fn production_impl_delegates_to_seam() {
    if let Err(reason) = production_guard::check(PRODUCTION_SOURCE) {
        panic!("production critical_impl must delegate to the seam: {reason}");
    }
}

mod production_guard {
    /// Brace-matched block body starting right after `marker`'s `{` (exclusive).
    pub(crate) fn block_after<'a>(source: &'a str, marker: &str) -> Option<&'a str> {
        let start = source.find(marker)? + marker.len();
        let open = source[start..].find('{')? + start + 1;
        let mut depth = 1usize;
        let bytes = source.as_bytes();
        let mut idx = open;
        while idx < bytes.len() {
            match bytes[idx] {
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(&source[open..idx]);
                    }
                }
                _ => {}
            }
            idx += 1;
        }
        Some(&source[open..])
    }

    /// Verifies the Impl methods delegate to the seam, without inlining axhal
    /// IRQ calls or leaving the block empty/truncated.
    pub fn check(source: &str) -> Result<(), String> {
        let impl_block = block_after(
            source,
            "unsafe impl critical_section::Impl for KernelCriticalSection",
        )
        .ok_or("production critical_section impl block not found")?;
        let acquire = block_after(impl_block, "fn acquire")
            .ok_or("acquire method not found in impl block")?;
        let release = block_after(impl_block, "fn release")
            .ok_or("release method not found in impl block")?;

        if !acquire.contains("critical_section_policy::acquire") {
            return Err("acquire does not delegate to critical_section_policy::acquire".into());
        }
        if acquire.contains("disable_irqs") || acquire.contains("irqs_enabled") {
            return Err("acquire inlines axhal IRQ calls instead of the seam".into());
        }
        if !release.contains("critical_section_policy::release") {
            return Err("release does not delegate to critical_section_policy::release".into());
        }
        if release.contains("enable_irqs") {
            return Err("release inlines axhal IRQ calls instead of the seam".into());
        }
        Ok(())
    }
}

/// Production source guard for the VirtIO-net IRQ handler (T6.1a).
///
/// The handler must keep the strict record -> ACK -> publish order, surround
/// the wake with `irqs_enabled()` checks, and never touch the Service,
/// queue-control, descriptors, smoltcp or print loops. This guard reads the
/// actual kernel source so a future edit that breaks the ordering contract
/// fails the host gate immediately.
mod virtio_irq_guard {
    use super::production_guard::block_after;

    const VIRTIO_NET_IRQ_SOURCE: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");

    /// Forbidden data-path tokens that must never appear inside the handler.
    /// `descriptor` is checked separately because the legitimate
    /// `platform::descriptor()` config lookup must be allowed.
    const FORBIDDEN_IN_HANDLER: &[&str] = &[
        "Service",
        "rx_one_step",
        "rx_control",
        "receive",
        "recycle",
        "smoltcp",
        "queue_control",
        "ax_println",
    ];

    /// True when any `descriptor` mention is *not* the legitimate
    /// `platform::descriptor()` config lookup.
    fn has_data_path_descriptor(body: &str) -> bool {
        let mut search_from = 0usize;
        while let Some(pos) = body[search_from..].find("descriptor") {
            let pos = pos + search_from;
            let before = body[..pos].rfind("platform::");
            let in_lookup = before.is_some_and(|p| body[p..pos].trim_end() == "platform::");
            if !in_lookup {
                return true;
            }
            search_from = pos + "descriptor".len();
        }
        false
    }

    pub fn check() -> Result<(), String> {
        check_source(VIRTIO_NET_IRQ_SOURCE)
    }

    pub fn check_source(source: &str) -> Result<(), String> {
        let handler =
            block_after(source, "fn net_irq_handler").ok_or("net_irq_handler body not found")?;

        // record -> ACK -> publish order: each step must appear and the ACK
        // write must precede the publish call.
        let record_pos = handler
            .find("TELEMETRY.record(status)")
            .ok_or("handler must record the raw status variable")?;
        let ack_pos = handler
            .find("write_volatile")
            .ok_or("handler does not write the device ACK register")?;
        let publish_pos = handler
            .find("publish_queue_event")
            .or_else(|| handler.find("publish_rx_event"))
            .ok_or("handler does not publish used-ring queue events")?;
        // Task 3.1 / R6: the config-change bit has its own independent
        // publication seam. Requiring the config publisher keeps a config-only
        // (or combined) cause from being silently swallowed by the used-ring
        // gate.
        let config_publish_pos = handler
            .find("publish_config_event")
            .ok_or("handler does not publish config-change queue events (Task 3.1)")?;
        let restore_pos = handler
            .find("restore_violation")
            .ok_or("handler does not observe restore violations")?;
        let enabled_entry_pos = handler
            .find("irq_enabled_entry")
            .ok_or("handler does not observe IRQ-enabled entry")?;

        if !(record_pos < ack_pos
            && ack_pos < publish_pos
            && publish_pos < config_publish_pos
            && config_publish_pos < enabled_entry_pos
            && enabled_entry_pos < restore_pos)
        {
            return Err(
                "handler order must be record -> ACK -> used -> config -> entry/restore checks"
                    .into(),
            );
        }

        // Wake must be surrounded by IRQ enable-state reads: one read before
        // the publish and a second, later read after it.
        let irq_before_pos = handler
            .find("irqs_enabled")
            .ok_or("no irqs_enabled() read before publish")?;
        let irq_after_pos = handler[irq_before_pos + 1..]
            .find("irqs_enabled")
            .map(|p| p + irq_before_pos + 1)
            .ok_or("no irqs_enabled() read after publish")?;
        if !(irq_before_pos < publish_pos && publish_pos < irq_after_pos) {
            return Err("irqs_enabled() must be read before and after publish".into());
        }

        for token in FORBIDDEN_IN_HANDLER {
            if handler.contains(token) {
                return Err(format!(
                    "handler must not contain data-path token `{token}`"
                ));
            }
        }
        if has_data_path_descriptor(handler) {
            return Err("handler must not touch VirtIO queue descriptors".into());
        }

        // init must start the task only after successful registration.
        let init = block_after(source, "fn init_virtio_net_irq_diag")
            .ok_or("init_virtio_net_irq_diag body not found")?;
        let register_pos = init
            .find("axhal::irq::register")
            .ok_or("init does not register the IRQ handler")?;
        let start_pos = init
            .find("start_rx_task")
            .ok_or("init does not start the async RX task")?;
        if !(register_pos < start_pos) {
            return Err("start_rx_task must be called only after register succeeds".into());
        }
        let registration_failure = block_after(init, "if !axhal::irq::register")
            .ok_or("registration failure branch not found")?;
        if !registration_failure.contains("return;") {
            return Err("registration failure branch must return before start_rx_task".into());
        }

        Ok(())
    }
}

#[test]
fn virtio_net_irq_handler_guard_passes() {
    if let Err(reason) = virtio_irq_guard::check() {
        panic!("virtio_net_irq handler violates the ISR contract: {reason}");
    }
}

const MUTATED_RECORD_ARGUMENT: &str = r#"
fn net_irq_handler() {
    let status = 1u8;
    let mask = status & 3;
    TELEMETRY.record(mask);
    write_volatile(mask as u32);
    if should_publish_rx(status) {
        let before = irqs_enabled();
        publish_rx_event();
        let after = irqs_enabled();
        if before { irq_enabled_entry += 1; }
        if !before && after { restore_violation += 1; }
    }
}

fn init_virtio_net_irq_diag() {
    if !axhal::irq::register(7, net_irq_handler) { return; }
    start_rx_task();
}
"#;

const MUTATED_EARLY_RETURN_OUTSIDE_REGISTER_BRANCH: &str = r#"
fn net_irq_handler() {
    let status = 1u8;
    TELEMETRY.record(status);
    write_volatile(status as u32);
    let before = irqs_enabled();
    publish_rx_event();
    let after = irqs_enabled();
    if before { irq_enabled_entry += 1; }
    if !before && after { restore_violation += 1; }
}

fn init_virtio_net_irq_diag() {
    if unrelated_failure() { return; }
    if !axhal::irq::register(7, net_irq_handler) { log_failure(); }
    start_rx_task();
}
"#;

#[test]
fn virtio_net_irq_guard_rejects_wrong_record_argument() {
    assert!(virtio_irq_guard::check_source(MUTATED_RECORD_ARGUMENT).is_err());
}

#[test]
fn virtio_net_irq_guard_requires_return_in_registration_failure_branch() {
    assert!(virtio_irq_guard::check_source(MUTATED_EARLY_RETURN_OUTSIDE_REGISTER_BRANCH).is_err());
}

#[test]
fn snapshot_command_consumer_inventory_is_versioned_and_bounded() {
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
    const LOGIC: &str = include_str!("../kernel/src/drivers/virtio_net_irq_logic.rs");
    const MS03: &str = include_str!("ms03_irq_probe.c");
    const MS04: &str = include_str!("ms04_rx_probe.c");
    const MS16: &str = include_str!("network_benchmark_platform.c");

    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V1: u32 = 0x4e49_4431"));
    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V2: u32 = 0x4e49_4432"));
    assert!(CTL.contains("NET_RX_SOFTWARE_NUDGE: u32 = 0x4e49_4e31"));
    assert!(CTL.contains("IrqSnapshotV1).vm_write(snapshot)"));
    assert!(CTL.contains("IrqSnapshotV2).vm_write(snapshot)"));
    assert!(CTL.contains("axnet::software_nudge()"));

    assert!(LOGIC.contains("pub struct IrqSnapshotV1"));
    assert!(LOGIC.contains("pub struct IrqSnapshotV2"));
    assert!(LOGIC.contains("pub struct IrqSnapshotV3"));
    assert!(!LOGIC.contains("type IrqSnapshotV1 = IrqSnapshotV2"));
    assert!(!LOGIC.contains("type IrqSnapshotV3 = IrqSnapshotV2"));

    assert!(MS03.contains("#define NET_IRQ_SNAPSHOT  0x4e494431"));
    assert!(MS03.contains("8 * sizeof(uint64_t)"));
    assert!(!MS03.contains("0x4e494432"));

    assert!(MS16.contains("NB_IOCTL_SNAPSHOT = 0x4e494431"));
    assert!(MS16.contains("uint64_t dummy[8]"));
    assert!(!MS16.contains("0x4e494432"));

    assert!(MS04.contains("#define MS04_SNAPSHOT_V2 0x4e494432"));
    assert!(MS04.contains("#define MS04_SOFTWARE_NUDGE 0x4e494e31"));
    assert!(MS04.contains("28 * sizeof(uint64_t)"));
    assert!(!MS04.contains("0x4e494431"));
    // The MS04 V2 consumer is untouched by V3: it neither knows nor writes
    // the V3 command.
    assert!(!MS04.contains("0x4e494433"));
}

mod probe_terminal_guard {
    use super::production_guard::block_after;

    const SOURCE: &str = include_str!("ms04_rx_probe.c");

    pub fn check(source: &str) -> Result<(), String> {
        if source.matches("MS04 FAIL mode=").count() != 1
            || source.matches("MS04 %s mode=").count() != 1
        {
            return Err("terminal markers must be emitted only by the two report helpers".into());
        }
        for runner in ["run_snapshot", "run_idle", "run_nudge", "run_burst"] {
            let body = block_after(source, &format!("static int {runner}"))
                .ok_or_else(|| format!("{runner} body not found"))?;
            if !body.contains("fail_mode") || !body.contains("finish_mode") {
                return Err(format!("{runner} must terminate through a report helper"));
            }
            if body.contains("MS04 PASS mode=") || body.contains("MS04 FAIL mode=") {
                return Err(format!("{runner} emits a terminal marker directly"));
            }
        }
        Ok(())
    }

    pub fn check_production() -> Result<(), String> {
        check(SOURCE)
    }
}

#[test]
fn probe_modes_have_one_central_terminal_marker_path() {
    if let Err(reason) = probe_terminal_guard::check_production() {
        panic!("MS04 probe terminal marker contract failed: {reason}");
    }
}

#[test]
fn qemu_diagnostics_feature_propagates_only_from_kernel_qemu() {
    // The QEMU-only pressure controls must be reachable only through
    // `starry-kernel/qemu`; ordinary axnet and D1 builds exclude them.
    const AXNET_TOML: &str = include_str!("../crates/axnet/Cargo.toml");
    const KERNEL_TOML: &str = include_str!("../kernel/Cargo.toml");
    const DIAG: &str = include_str!("../crates/axnet/src/diag.rs");
    const LIB: &str = include_str!("../crates/axnet/src/lib.rs");

    assert!(AXNET_TOML.contains("qemu-diagnostics = []"));
    assert!(KERNEL_TOML.contains("axnet/qemu-diagnostics"));
    // The kernel `qemu` feature list carries the propagation.
    let qemu_list = KERNEL_TOML
        .split("qemu = [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("qemu feature list");
    assert!(qemu_list.contains("axnet/qemu-diagnostics"));
    assert!(qemu_list.contains("dep:axnet"));
    // The controls live behind the feature gate in axnet.
    assert!(DIAG.contains("#[cfg(test)]"));
    assert!(LIB.contains("#[cfg(feature = \"qemu-diagnostics\")]"));
    // The default axnet feature set must not enable diagnostics.
    assert!(!AXNET_TOML.contains("default = [\"qemu-diagnostics\"]"));
}

#[test]
fn probe_includes_the_timeval_definition_directly() {
    const SOURCE: &str = include_str!("ms04_rx_probe.c");
    assert!(
        SOURCE.contains("#include <sys/time.h>"),
        "struct timeval must not depend on libc-specific transitive includes"
    );
}

#[test]
fn probe_terminal_guard_rejects_a_missing_failure_path() {
    const MUTATED: &str = r#"
static int finish_mode() { printf("MS04 %s mode="); }
static int fail_mode() { printf("MS04 FAIL mode="); }
static int run_snapshot() { return finish_mode(); }
static int run_idle() { return fail_mode(); }
static int run_nudge() { return fail_mode() + finish_mode(); }
static int run_burst() { return fail_mode() + finish_mode(); }
"#;
    assert!(probe_terminal_guard::check(MUTATED).is_err());
}

/// The scheduler must be the single owner of the reschedule S_SOFT handler.
///
/// `starry-kernel/smp` must enable `axtask/ipi` (our vendor extension that owns
/// the software-interrupt and sends the remote-reschedule IPI) and must never
/// simultaneously enable `axruntime/ipi`/`axipi`, which would register the same
/// S_SOFT slot for an unrelated IPC dispatcher.
#[test]
fn ipi_owner_is_single_and_only_from_kernel_smp() {
    const KERNEL: &str = include_str!("../kernel/Cargo.toml");

    let smp_list = KERNEL
        .split("smp = [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("kernel smp feature list");
    assert!(
        smp_list.contains("axtask/ipi"),
        "kernel smp must enable axtask/ipi (the sole S_SOFT reschedule owner)"
    );

    // The kernel must never enable the competing `axruntime/ipi` / `axipi`
    // features anywhere (SMP or otherwise).
    assert!(
        !KERNEL.contains("axruntime/ipi"),
        "kernel must not enable axruntime/ipi (competing S_SOFT owner)"
    );
    assert!(
        !KERNEL.contains("axipi"),
        "kernel must not enable axipi (competing S_SOFT owner)"
    );
}

/// Design D6 / A3 (Cycle 002, 2.3-R3): the standard `make ... SMP=16` build must
/// actually select the root `smp` feature, which is the only path that propagates
/// `starry-kernel/smp -> axtask/ipi` (real remote-ready IPI telemetry). Prior to
/// the fix only `axfeat/smp` was enabled by `features.mk`, so the kernel `smp`
/// (and with it `axtask/ipi`) was absent and the snapshot IPI counters were always
/// zero, which a `SMP=16` qualification build must not silently accept. Single-hart
/// boots must not enable it, and `axruntime/ipi`/`axipi` stay off.
#[test]
fn standard_smp_build_enables_single_axtask_ipi_owner() {
    const ROOT: &str = include_str!("../Cargo.toml");
    const MAKEFILE: &str = include_str!("../Makefile");

    // Project Makefile: on `SMP > 1` the `smp` feature is appended to
    // APP_FEATURES (the root crate feature set), gated so single-hart never adds it.
    assert!(
        MAKEFILE.contains("GET_SMP") || MAKEFILE.contains("test $(SMP) -gt 1"),
        "Makefile must gate the root smp feature on SMP>1"
    );
    assert!(
        MAKEFILE.contains("APP_FEATURES += smp"),
        "Makefile must append the root `smp` feature for SMP boots"
    );

    // Root crate `smp` is the propagation point for `starry-kernel/smp`.
    let root_smp = ROOT
        .split("\nsmp = [")
        .nth(1)
        .and_then(|s| s.split(']').next())
        .expect("root smp feature list");
    assert!(
        root_smp.contains("starry-kernel/smp"),
        "root smp must propagate starry-kernel/smp"
    );
    // Root `smp` must not introduce a second S_SOFT owner.
    assert!(
        !root_smp.contains("axruntime/ipi") && !root_smp.contains("axipi"),
        "root smp must not enable a competing S_SOFT owner"
    );
}

/// The QEMU config overlay must correct only the PLIC MMIO window to the full
/// `0x0c00_0000 / 0x60_0000` declared by the device tree, and only on the
/// default RISC-V QEMU platform. Non-QEMU platforms must not receive the arg.
#[test]
fn qemu_plic_overlay_only_fixes_plic_window() {
    const CONFIG_MK: &str = include_str!("../make/config.mk");
    const OVERLAY_VAL: &str = "0x0c00_0000, 0x60_0000";

    // The override is applied via the final `-w` write arg (last-applied, so it
    // corrects the platform fact after any EXTRA_CONFIG merge) and only when the
    // platform is the default RISC-V QEMU virt board.
    assert!(
        CONFIG_MK.contains("QEMU_OVERLAY_ARG"),
        "config.mk must define the QEMU overlay write arg"
    );
    assert!(
        CONFIG_MK.contains("riscv64-qemu-virt"),
        "config.mk must gate the overlay on the riscv64-qemu-virt platform"
    );
    assert!(
        CONFIG_MK.contains(OVERLAY_VAL),
        "config.mk QEMU overlay must use the full PLIC window 0x60_0000"
    );

    // Non-QEMU platforms must not receive the arg at all (empty for non-QEMU).
    let branch = CONFIG_MK
        .split("ifeq ($(strip $(PLAT_NAME)), riscv64-qemu-virt)")
        .nth(1)
        .and_then(|s| s.split("endif").next())
        .expect("qemu-gated overlay branch");
    assert!(
        branch.contains("QEMU_OVERLAY_ARG :="),
        "overlay arg must be assigned only inside the QEMU platform guard"
    );
}

/// `axconfig-gen 0.2.1` merges every specification file first and rejects a
/// duplicate key, then applies all `-w`. So `EXTRA_CONFIG` must not be
/// documented as able to override an existing `devices.mmio-ranges`: a second
/// spec defining that key is a deterministic merge-time `Duplicate key` error,
/// independent of CLI argument order. This guard proves that rejection with a
/// minimal fixture rather than re-reading the generator source.
#[test]
fn config_extra_duplicate_mmio_key_is_rejected() {
    use std::process::Command;

    let dir = std::env::temp_dir().join(format!("ms08-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let base = dir.join("base.toml");
    let extra = dir.join("extra.toml");
    let out = dir.join("out.toml");
    let _ = std::fs::remove_file(&out);

    std::fs::write(&base, "devices.mmio-ranges = [[0x0c00_0000, 0x21_0000]]\n").unwrap();
    std::fs::write(&extra, "devices.mmio-ranges = [[0x3000_0000, 0x1000]]\n").unwrap();

    let out_res = Command::new("axconfig-gen")
        .arg(&base)
        .arg(&extra)
        .arg("-o")
        .arg(&out)
        .output()
        .expect("axconfig-gen must be on PATH");

    let stderr = String::from_utf8_lossy(&out_res.stderr);
    assert!(
        !out_res.status.success(),
        "a duplicate devices.mmio-ranges spec must be rejected, got: {stderr}"
    );
    assert!(
        stderr.contains("Duplicate key"),
        "rejection must be the merge-stage duplicate-key error, got: {stderr}"
    );
    assert!(
        !out.exists(),
        "a rejected duplicate-key merge must not produce an output config"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// MS08 shared background-role placement policy (design D4).
///
/// `kernel/src/drivers/placement.rs` assigns a singleton hart to each background
/// logical role (UART RX/TX copier, network owner/runner) from an ordered,
/// de-duplicated *publued-schedulable* hart set plus the current boot hart.
/// Rule: role `i` occupies `schedulable[(anchor_pos + i) % len]`, so roles
/// separate whenever enough harts exist and co-locate only when necessary, and
/// future network roles keep a deterministic occupancy order.
mod placement_tests {
    use super::placement::{
        BackgroundRole, MAX_CPU_NUM, NUM_BACKGROUND_ROLES, RolePlacement, place_role, place_roles,
    };

    fn members(p: &RolePlacement) -> [usize; 4] {
        assert_eq!(
            NUM_BACKGROUND_ROLES, 4,
            "policy roles must match the 4-role enum"
        );
        [p.uart_rx, p.uart_tx, p.net_owner, p.net_runner]
    }

    #[test]
    fn single_hart_colocates_all_roles() {
        let p = place_roles(&[0], 0).expect("single schedulable hart is valid");
        assert_eq!(members(&p), [0, 0, 0, 0]);
    }

    #[test]
    fn placement_capacity_matches_axconfig() {
        // Regression: the policy capacity must be the single-source-of-truth
        // `axconfig::plat::MAX_CPU_NUM`. A hardcoded 64 here once made
        // `sched_hart_ids` probe a 16-bit CpuMask at indices 16..63; the
        // out-of-range `get` (debug_assert only) returned garbage `true`, so
        // nonexistent harts (e.g. 16 under SMP=16) entered the schedulable set
        // and were handed to role placement.
        assert_eq!(
            MAX_CPU_NUM, axconfig::plat::MAX_CPU_NUM,
            "placement capacity must delegate to axconfig::plat::MAX_CPU_NUM"
        );
        assert_eq!(MAX_CPU_NUM, 16, "harness stub models the SMP=16 platform");
    }

    #[test]
    fn sparse_single_hart_colocates() {
        let p = place_roles(&[7], 7).expect("single sparse hart is valid");
        assert_eq!(members(&p), [7, 7, 7, 7]);
    }

    #[test]
    fn two_harts_separate_consecutive_roles() {
        let p = place_roles(&[0, 4], 0).expect("two harts are valid");
        // role i -> schedulable[i % 2], anchor at index 0.
        assert_eq!(members(&p), [0, 4, 0, 4]);
    }

    #[test]
    fn four_harts_give_four_distinct_background_harts() {
        let p = place_roles(&[0, 1, 2, 3], 0).unwrap();
        let m = members(&p);
        let mut sorted: Vec<usize> = m.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            4,
            "all four background roles must be distinct"
        );
    }

    #[test]
    fn sixteen_harts_from_nonzero_anchor_are_distinct_and_in_set() {
        let set: Vec<usize> = (0..16).collect();
        // Boot hart 11 must place the four roles on distinct, in-set harts.
        let p = place_roles(&set, 11).unwrap();
        let m = members(&p);
        let mut sorted: Vec<usize> = m.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(sorted.len(), 4, "SMP=16 must give four distinct roles");
        for h in m {
            assert!(
                set.contains(&h),
                "role hart {h} must be in the schedulable set"
            );
        }
    }

    #[test]
    fn sparse_set_with_nonzero_anchor_uses_in_set_harts_in_order() {
        // anchor=3 is at set index 1, so roles start from that position.
        let p = place_roles(&[1, 3, 5, 7], 3).unwrap();
        assert_eq!(members(&p), [3, 5, 7, 1]);
    }

    #[test]
    fn empty_set_fails_closed() {
        assert!(place_roles(&[], 0).is_none(), "empty set must fail closed");
    }

    #[test]
    fn duplicate_hart_fails_closed() {
        // A repeated hart id is not a valid ordered, de-duplicated set.
        assert!(
            place_roles(&[1, 1], 1).is_none(),
            "duplicate must fail closed"
        );
        assert!(
            place_roles(&[0, 2, 2, 4], 0).is_none(),
            "duplicate must fail closed"
        );
    }

    #[test]
    fn out_of_range_hart_fails_closed() {
        // Values at/above MAX_CPU_NUM exceed the run-queue/readiness capacity.
        assert!(
            place_roles(&[0, MAX_CPU_NUM], 0).is_none(),
            "hart == MAX_CPU_NUM must fail closed"
        );
        assert!(
            place_roles(&[100], 100).is_none(),
            "hart above MAX_CPU_NUM must fail closed"
        );
    }

    #[test]
    fn unordered_set_fails_closed() {
        // The policy requires an ascending, de-duplicated set.
        assert!(
            place_roles(&[4, 2, 0], 0).is_none(),
            "unordered set must fail closed"
        );
        assert!(
            place_roles(&[3, 1], 3).is_none(),
            "unordered set must fail closed"
        );
    }

    #[test]
    fn three_and_eight_hart_sets_place_in_set_singletons() {
        for len in [3_usize, 8] {
            let set: Vec<usize> = (0..len).collect();
            for anchor in &[0_usize, 1, len - 1] {
                let p = place_roles(&set, *anchor).expect("dense len must be valid");
                for h in members(&p) {
                    assert!(set.contains(&h), "{len}-hart role hart {h} must be in set");
                }
                // With >= 4 harts the four background roles stay distinct.
                if len >= members(&p).len() {
                    let mut m = members(&p).to_vec();
                    m.sort_unstable();
                    m.dedup();
                    assert_eq!(m.len(), 4, "{len}-hart set must give 4 distinct roles");
                }
            }
        }
    }

    #[test]
    fn place_role_selects_single_requested_role() {
        assert_eq!(place_role(&[0, 1], 0, BackgroundRole::UartTx), Some(1));
        assert_eq!(place_role(&[0, 1], 0, BackgroundRole::NetOwner), Some(0));
        assert!(place_role(&[], 0, BackgroundRole::UartRx).is_none());
    }

    #[test]
    fn every_role_hart_is_a_member_of_the_input_set() {
        for set in [
            vec![0_usize, 1, 2],
            vec![3_usize],
            vec![0_usize, 2, 4, 6, 8],
        ] {
            for anchor in &[0_usize, 1, 7, set[0]] {
                let Some(p) = place_roles(&set, *anchor) else {
                    continue;
                };
                for h in members(&p) {
                    assert!(
                        set.contains(&h),
                        "role hart {h} must belong to the schedulable set"
                    );
                }
            }
        }
    }
}

/// Design D5: the UART adapter must start each copier exactly once with an
/// affinity committed before first enqueue (`spawn_with_name_affinity`), derive
/// the target hart from the pure placement policy over the published schedulable
/// set, save the task handle, and fail closed on a duplicate start. This guards
/// the kernel boot-path lifecycle that has no runtime unit fixture.
#[test]
fn uart_start_copiers_uses_pre_enqueue_affinity_and_saves_handle() {
    const UART_INIT: &str = include_str!("../kernel/src/drivers/uart_init.rs");

    // Placement comes from the pure policy over the published schedulable mask.
    assert!(UART_INIT.contains("placement::place_roles"));
    assert!(UART_INIT.contains("axtask::schedulable_cpu_mask()"));

    // First enqueue commits the singleton affinity (pre-enqueue API), not a
    // plain spawn followed by a mask update.
    assert!(
        UART_INIT.contains("spawn_with_name_affinity"),
        "start_copiers must spawn with a pre-enqueue affinity API"
    );
    assert!(UART_INIT.contains("singleton_mask"));

    // Both directions expose an explicit, non-spawning copier future.
    assert!(UART_INIT.contains(".rx_copier()"));
    assert!(UART_INIT.contains(".tx_copier()"));

    // Each direction is started at most once and saves its handle.
    for guard in ["RX_COPIER_STARTED.swap(true", "TX_COPIER_STARTED.swap(true"] {
        assert!(
            UART_INIT.contains(guard),
            "a duplicate-start guard {guard:?} must fail closed",
        );
    }
    for handle in [
        "RX_COPIER_TASK.lock() = Some(task)",
        "TX_COPIER_TASK.lock() = Some(task)",
    ] {
        assert!(
            UART_INIT.contains(handle),
            "a saved copier handle {handle:?} must be stored",
        );
    }
}

/// Design D5: `rx_copier`/`tx_copier` must return the copier loop future without
/// spawning, while the compatibility `start_*_copier` wrappers delegate to them
/// via `OsRuntime::spawn`.
#[test]
fn driver_copier_futures_do_not_spawn_and_wrappers_delegate() {
    const DRIVER: &str = include_str!("../crates/uart_16550/src/async_/driver.rs");

    /// Extracts the balanced body of `fn {name}(...` (first occurrence) so the
    /// explicit-future body can be checked independently of the spawn wrapper.
    fn fut_body<'a>(source: &'a str, name: &str) -> &'a str {
        let sig = format!("pub fn {name}(&'static self)");
        let body_start = source.find(&sig).expect("explicit future signature");
        let brace = source[body_start..]
            .find('{')
            .map(|i| body_start + i + 1)
            .expect("future body brace");
        let mut depth = 1usize;
        for (off, ch) in source[brace..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[brace..brace + off];
                    }
                }
                _ => {}
            }
        }
        panic!("unbalanced braces in {name}");
    }

    for f in ["rx_copier", "tx_copier"] {
        // The explicit future returns an opaque Send future (it does not spawn).
        let sig = format!("pub fn {f}(&'static self) -> impl core::future::Future");
        assert!(
            DRIVER.contains(&sig),
            "explicit copier future {f} must return a Future"
        );
        let body = fut_body(DRIVER, f);
        assert!(
            !body.contains("R::spawn"),
            "explicit {f} future must never spawn internally (adapter owns enqueueing + affinity)"
        );
    }

    // The compatibility wrappers delegate to the explicit futures.
    assert!(DRIVER.contains("R::spawn(self.rx_copier(), \"uart-rx-copier\")"));
    assert!(DRIVER.contains("R::spawn(self.tx_copier(), \"uart-tx-copier\")"));
}

/// Design D6: the QEMU-only UART SMP snapshot is a *new* command with its own
/// wire type; the old TXDBG command values and 16-u64 struct must stay byte
/// identical. The snapshot layout is validated *structurally* (magic prefix at
/// byte 0, explicit per-field offsets and total size) from the shared
/// [`uart_snapshot_types`] source rather than by grepping field names.
#[test]
fn uart_smp_snapshot_layout_is_explicit_and_prefixed() {
    use core::mem::{offset_of, size_of};

    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
    const SNAP: &str = include_str!("../kernel/src/drivers/uart_smp_snapshot.rs");
    const SNAP_TYPE: &str = include_str!("../kernel/src/drivers/uart_snapshot_types.rs");

    // New, independent QEMU-only command + handler; old TXDBG values unchanged.
    assert!(CTL.contains("const UART_SMP_SNAPSHOT: u32 = 0x5553_4d31;"));
    assert!(CTL.contains("#[cfg(feature = \"qemu\")]"));
    assert!(CTL.contains("cmd == UART_SMP_SNAPSHOT"));
    assert!(CTL.contains("UART_TXDBG_SNAPSHOT: u32 = 0x5458_4431"));
    assert!(CTL.contains("UART_TXDBG_RESET: u32 = 0x5458_4432"));

    // The snapshot re-exports the shared wire type so kernel & host share layout.
    assert!(SNAP.contains("pub use super::uart_snapshot_types::UartSmpSnapshot;"));

    // Magic prefix occupies byte 0 and is exactly one u32.
    assert_eq!(offset_of!(uart_snapshot_types::UartSmpSnapshot, magic), 0);
    assert_eq!(uart_snapshot_types::UART_SMP_SNAPSHOT_MAGIC, 0x5553_4d31);
    // Frame-validity: a zeroed snapshot is not a valid frame; the kernel sets the
    // magic prefix, so `is_valid_frame` lets a consumer reject garbage.
    let mut blank = uart_snapshot_types::UartSmpSnapshot::zeroed();
    assert!(!blank.is_valid_frame());
    blank.magic = uart_snapshot_types::UART_SMP_SNAPSHOT_MAGIC;
    assert!(blank.is_valid_frame());
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, configured_harts),
        size_of::<u32>(),
        "magic must be the sole leading u32 prefix"
    );
    assert_eq!(
        uart_snapshot_types::UART_SMP_SNAPSHOT_PREFIX,
        size_of::<u32>(),
        "prefix length must be one u32"
    );

    // Documented field offsets (repr(C), exact). Changing any of these is an ABI
    // break for the ioctl consumer; first canonical test pins them literally.
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, configured_harts),
        4
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, schedulable_mask),
        8
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_affinity),
        16
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_affinity),
        24
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, irq_last_hart),
        32
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, irq_hart_mask),
        40
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_last_hart),
        48
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_hart_mask),
        56
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_last_hart),
        64
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_hart_mask),
        72
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_occupancy),
        80
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_vacancy),
        84
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, ring_empty),
        88
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, copier_active),
        89
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, staged_bytes),
        92
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, transmitter_empty),
        96
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, irq_events),
        104
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_polls),
        112
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_polls),
        120
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, ipi_sent),
        128
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, ipi_received),
        136
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, affinity_rejects),
        144
    );
    // Task 4.2 migration extension (appended; all pre-existing offsets unchanged).
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_migration_state),
        152
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_migration_state),
        153
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_migration_from),
        156
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_migration_to),
        160
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_migration_from),
        164
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_migration_to),
        168
    );
    assert_eq!(
        offset_of!(
            uart_snapshot_types::UartSmpSnapshot,
            rx_migration_requested_polls
        ),
        176
    );
    assert_eq!(
        offset_of!(
            uart_snapshot_types::UartSmpSnapshot,
            rx_migration_observed_polls
        ),
        184
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, rx_migration_rejects),
        192
    );
    assert_eq!(
        offset_of!(
            uart_snapshot_types::UartSmpSnapshot,
            tx_migration_requested_polls
        ),
        200
    );
    assert_eq!(
        offset_of!(
            uart_snapshot_types::UartSmpSnapshot,
            tx_migration_observed_polls
        ),
        208
    );
    assert_eq!(
        offset_of!(uart_snapshot_types::UartSmpSnapshot, tx_migration_rejects),
        216
    );
    assert_eq!(
        size_of::<uart_snapshot_types::UartSmpSnapshot>(),
        224,
        "total snapshot size must match the documented ABI"
    );

    // The kernel snapshot sets the magic prefix and reports attributable per-driver
    // progress; the assembly is gated on QEMU.
    assert!(SNAP.contains("s.magic = UART_SMP_SNAPSHOT_MAGIC;"));
    for f in ["irq_events", "rx_polls", "tx_polls"] {
        assert!(
            SNAP.contains(f),
            "snapshot must expose attributable UART field {f}"
        );
    }
    assert!(SNAP.contains("pub fn snapshot()"));
    assert!(SNAP.contains("#[cfg(feature = \"qemu\")]"));
    // Guest copy of the snapshot must copy a fully-defined wire frame, never the
    // `repr(C)` struct object (which would memcpy implicit padding).
    assert!(CTL.contains("snapshot.wire_bytes()"));
    assert!(CTL.contains("[u8; crate::drivers::uart_snapshot_types::UartSmpSnapshot::WIRE_SIZE]"));
    assert!(CTL.contains("vm_write(wire)?"));
    assert!(!CTL.contains("UartSmpSnapshot).vm_write(snapshot)"));
    // The wire serialization zeroes reserved bytes and never does a whole-object
    // copy of a padded struct.
    assert!(SNAP_TYPE.contains("WIRE_SIZE"));
    assert!(SNAP_TYPE.contains("wire_bytes"));
    assert!(SNAP_TYPE.contains("_r0: [u8; 4]"));
    assert!(SNAP_TYPE.contains("_r4: [u8; 7]"));

    // Old TXDBG consumer inventory stays at 16 u64 counters (unchanged).
    let txdbg_struct = CTL
        .split("struct UartTxDebugSnapshot")
        .nth(1)
        .and_then(|s| s.split('}').next())
        .unwrap_or_default();
    assert_eq!(
        txdbg_struct.matches("u64,").count(),
        16,
        "UART TXDBG ABI must remain byte-identical"
    );
}

/// ABI witness against the *real* wire type: every `repr(C)` offset and the total
/// size are checked a second time via `offset_of!` against the documented ABI, so
/// the constraint is structural, not a field-name string match.
#[test]
fn uart_smp_snapshot_real_offsets_match_documented_abi() {
    use core::mem::offset_of;

    use uart_snapshot_types::UartSmpSnapshot as T;
    assert_eq!(offset_of!(T, magic), 0);
    assert_eq!(offset_of!(T, configured_harts), 4);
    assert_eq!(offset_of!(T, schedulable_mask), 8);
    assert_eq!(offset_of!(T, rx_affinity), 16);
    assert_eq!(offset_of!(T, tx_affinity), 24);
    assert_eq!(offset_of!(T, irq_last_hart), 32);
    assert_eq!(offset_of!(T, irq_hart_mask), 40);
    assert_eq!(offset_of!(T, rx_last_hart), 48);
    assert_eq!(offset_of!(T, rx_hart_mask), 56);
    assert_eq!(offset_of!(T, tx_last_hart), 64);
    assert_eq!(offset_of!(T, tx_hart_mask), 72);
    assert_eq!(offset_of!(T, rx_occupancy), 80);
    assert_eq!(offset_of!(T, tx_vacancy), 84);
    assert_eq!(offset_of!(T, ring_empty), 88);
    assert_eq!(offset_of!(T, copier_active), 89);
    assert_eq!(offset_of!(T, staged_bytes), 92);
    assert_eq!(offset_of!(T, transmitter_empty), 96);
    assert_eq!(offset_of!(T, irq_events), 104);
    assert_eq!(offset_of!(T, rx_polls), 112);
    assert_eq!(offset_of!(T, tx_polls), 120);
    assert_eq!(offset_of!(T, ipi_sent), 128);
    assert_eq!(offset_of!(T, ipi_received), 136);
    assert_eq!(offset_of!(T, affinity_rejects), 144);
    assert_eq!(offset_of!(T, rx_migration_state), 152);
    assert_eq!(offset_of!(T, tx_migration_state), 153);
    assert_eq!(offset_of!(T, rx_migration_from), 156);
    assert_eq!(offset_of!(T, rx_migration_to), 160);
    assert_eq!(offset_of!(T, tx_migration_from), 164);
    assert_eq!(offset_of!(T, tx_migration_to), 168);
    assert_eq!(offset_of!(T, rx_migration_requested_polls), 176);
    assert_eq!(offset_of!(T, rx_migration_observed_polls), 184);
    assert_eq!(offset_of!(T, rx_migration_rejects), 192);
    assert_eq!(offset_of!(T, tx_migration_requested_polls), 200);
    assert_eq!(offset_of!(T, tx_migration_observed_polls), 208);
    assert_eq!(offset_of!(T, tx_migration_rejects), 216);
    assert_eq!(core::mem::size_of::<T>(), 224);
}

/// Design D6 (A3): the self-consistent `(last, mask, events)` read must never
/// expose a torn tuple where `last` names a hart not yet in the cumulative mask.
/// First, a fixture reproduces the *old* two-independent-atomic exposure: an
/// isolated `last.store` / `mask.fetch_or` pair read as `last` then `mask` can
/// yield `last ∉ mask` (RED — proves the torn-read problem is real).
#[test]
fn hart_snapshot_isolated_atomics_are_torn() {
    use crate::hart_counter::UNKNOWN_HART;

    // Simulate the pre-2.3 design: two independent atomics, writer order
    // `last.store` then `mask.fetch_or`, reader order `last`, `mask`.
    let last = AtomicUsize::new(UNKNOWN_HART);
    let mask = AtomicU64::new(0);

    // Writer step 1: publish last hart.
    last.store(3, StdOrdering::Relaxed);
    let l = last.load(StdOrdering::Relaxed);
    // Reader reads mask *after* last but before the writer folds the bit in.
    let m = mask.load(StdOrdering::Relaxed);

    let torn = l != UNKNOWN_HART && (m & (1u64 << l)) == 0;
    assert!(
        torn,
        "isolated last/mask atomics can be read as inconsistent (l={l} m={m:#x})"
    );
}

/// The same reader-side invariant, but driven through the production
/// [`hart_counter::HartCounter`] `record`/`read` under concurrent writers across
/// multiple harts: the bounded-retry read must *never* return `last ∉ mask`.
#[test]
fn hart_snapshot_consistent_read_never_tears_under_concurrency() {
    use std::sync::Arc;

    use crate::hart_counter::{HartCounter, UNKNOWN_HART};

    let counter = Arc::new(HartCounter::new());
    const WRITERS: usize = 8;
    const ITERS: usize = 200_000;

    let mut handles = Vec::new();
    for w in 0..WRITERS {
        let c = Arc::clone(&counter);
        handles.push(std::thread::spawn(move || {
            for i in 0..ITERS {
                let hart = (w * 7 + i) % 16;
                c.record(hart);
            }
        }));
    }
    // A concurrent reader hammers `read` to catch any torn tuple.
    handles.push(std::thread::spawn({
        let c = Arc::clone(&counter);
        move || {
            for _ in 0..(WRITERS * ITERS) {
                let (last, mask, _events) = c.read();
                if last != UNKNOWN_HART {
                    assert!(
                        (mask & (1u64 << last)) != 0,
                        "consistent read returned torn tuple: last={last} mask={mask:#x}"
                    );
                }
            }
        }
    }));
    for h in handles {
        h.join().unwrap();
    }

    // Once all writers finished, the final read must be self-consistent and the
    // cumulative mask must contain every written hart.
    let (last, mask, _events) = counter.read();
    assert!(mask != 0, "mask must have been populated");
    assert!(
        last != UNKNOWN_HART && (mask & (1u64 << last)) != 0,
        "final read must be self-consistent"
    );
}

/// Design D6: affinity/IPI telemetry counters must be monotonic (append-only
/// `fetch_add`), and the composite placement snapshot must read each field from
/// a single atomic/task source rather than assembling a torn tuple.
#[test]
fn uart_smp_telemetry_counters_are_monotonic_and_sources_are_single() {
    const IPI: &str = include_str!("../crates/axtask/src/ipi.rs");
    const API: &str = include_str!("../crates/axtask/src/api.rs");

    // IPI send/receive counters are Relaxed fetch_add observers (monotonic).
    assert!(IPI.contains("IPI_SENT.fetch_add(1, Ordering::Relaxed)"));
    assert!(IPI.contains("IPI_RECEIVED.fetch_add(1, Ordering::Relaxed)"));
    // Invalid-affinity reject counter is a Relaxed fetch_add observer.
    assert!(API.contains("INVALID_AFFINITY_REJECTS.fetch_add(1"));

    // No counter is ever decremented or reset in the product (monotonic across
    // the change). Only the test-only reset exists in ipi.rs for self-tests.
    assert!(!IPI.contains("IPI_SENT.fetch_sub") && !IPI.contains("IPI_RECEIVED.fetch_sub"));
    assert!(!API.contains("INVALID_AFFINITY_REJECTS.fetch_sub"));
}

/// Task 2.4: the kernel `tcdrain` path must follow the check→register→recheck
/// ordering and gate on all four TX completion stages (ring empty, copier
/// inactive, staged 0, transmitter empty), backed by the drain waker.
#[test]
fn tcdrain_registers_then_rechecks_all_four_stages() {
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");

    // The tcdrain handler is the last branch in `sys_ioctl`; work on a region
    // that starts at the command and runs to the end of the function body so the
    // ordering checks are not truncated by inner-branch braces.
    let start = CTL
        .find("if cmd == 0x5409")
        .expect("tcdrain ioctl branch present");
    let region = &CTL[start..];

    // The four TX stages are gated through `is_drained()` (which combines ring
    // empty, copier inactive, staged 0 and transmitter empty) plus the explicit
    // ring/copier/staged conditions, backed by the drain waker.
    for cond in ["ring_empty", "copier_active", "staged_bytes"] {
        assert!(
            region.contains(cond),
            "tcdrain must gate on the {cond} completion stage"
        );
    }
    assert!(region.contains("DRAIN_WAKER"));

    // (1) An initial check + (2) a register + (3) a recheck: the register must
    // sit between the two is_drained() probes (register → recheck closes the
    // lost-edge window).
    let first = region.find("is_drained").expect("initial check");
    let last = region.rfind("is_drained").expect("recheck present");
    let between = &region[first..last];
    assert!(
        between.contains("register"),
        "waker registration must occur between check and recheck"
    );

    // The path returns Pending (never busy-polls) after the recheck.
    assert!(
        region[last..].contains("Poll::Pending"),
        "tcdrain must return Pending (never busy-poll) after recheck"
    );
}

/// Task 2.5: the placement refactor must not weaken D1's bounded TX slow-poll
/// workaround or the D1 async adapter, and QEMU early console must stay
/// independent of the copiers. QEMU not firing a path is not a reason to delete
/// the D1 workaround.
#[test]
fn d1_slow_poll_workaround_and_early_console_are_preserved() {
    const DRIVER: &str = include_str!("../crates/uart_16550/src/async_/driver.rs");
    const UART_INIT: &str = include_str!("../kernel/src/drivers/uart_init.rs");

    // The D1 bounded slow-poll + yield-retry workaround constants are intact.
    for c in [
        "TX_SLOW_POLL_SPINS",
        "TX_SLOW_POLL_LIMIT",
        "TX_YIELD_RETRIES",
        "TX_FAST_RETRY_LIMIT",
        "TX_TEMT_POLL_LIMIT",
    ] {
        assert!(
            DRIVER.contains(&format!("const {c}")),
            "D1 workaround constant {c} must be preserved"
        );
    }

    // The D1 async adapter and ISR wrapper are still feature-gated and intact.
    assert!(UART_INIT.contains("ArceOsD1UartPort"));
    assert!(UART_INIT.contains("d1_uart_isr_handler"));
    assert!(UART_INIT.contains("feature = \"lichee-d1-async-uart\""));
    assert!(UART_INIT.contains("init_interrupt_mode()"));

    // Early console (and the ISR path) is independent of copier startup: the
    // hardware + ISR init prints that copiers are *not yet* started, and copiers
    // start only after the startup benchmark (secondary-ready boundary).
    assert!(UART_INIT.contains("copiers not started yet"));
    assert!(UART_INIT.contains("init_uart_hardware"));
}

/// Design D6 / A3 (Cycle 002): during the very first `record`, the cumulative
/// mask bit is folded in *before* `last` is published. A reader that samples
/// that window must NOT get a torn "mask known, last unknown" tuple. This RED
/// fixture drives the production [`HartCounter::read`] through a partial first
/// record (mask set, last still UNKNOWN) via the test-only
/// `record_partial_mask` seam and asserts `read` never returns a synchronously
/// torn tuple (pre-fix it returned `(UNKNOWN_HART, mask≠0)`).
#[test]
fn hart_snapshot_first_record_never_returns_torn_empty() {
    use crate::hart_counter::{HartCounter, UNKNOWN_HART};

    // Simulate the mask-before-last first-record interleave. Pre-fix, sampling
    // here returns `(UNKNOWN_HART, mask≠0, 0)`, which violates the "empty is
    // unknown+zero+zero, else last ∈ mask" contract (A3).
    let c = HartCounter::new();
    c.record_partial_mask(3);

    let (last, mask, events) = c.read();
    let empty = last == UNKNOWN_HART && mask == 0 && events == 0;
    let consistent = last < 64 && (mask & (1u64 << last)) != 0;
    assert!(
        empty || consistent,
        "read returned torn first-record tuple: last={last:?} mask={mask:#x} events={events}"
    );
}

/// The same invariant under real concurrent writers across fresh counters: when
/// `last == UNKNOWN_HART`, `mask` and `events` must both be zero (pre-fix the
/// reset→first-record window could expose `(UNKNOWN, nonzero)` tuples).
#[test]
fn hart_snapshot_concurrent_read_unknown_last_implies_empty_tuple() {
    use std::sync::Arc;

    use crate::hart_counter::{HartCounter, UNKNOWN_HART};

    const ROUNDS: usize = 64;
    const WRITERS: usize = 4;
    const ITERS: usize = 2000;

    for _round in 0..ROUNDS {
        let counter = Arc::new(HartCounter::new());
        let mut handles = Vec::new();
        for w in 0..WRITERS {
            let c = Arc::clone(&counter);
            handles.push(std::thread::spawn(move || {
                for i in 0..ITERS {
                    c.record((w + i) % 16);
                }
            }));
        }
        let c = Arc::clone(&counter);
        handles.push(std::thread::spawn(move || {
            for _ in 0..(WRITERS * ITERS * 2) {
                let (last, mask, events) = c.read();
                if last == UNKNOWN_HART {
                    assert!(
                        mask == 0 && events == 0,
                        "UNKNOWN last must imply a fully-empty tuple, got mask={mask:#x} \
                         events={events}"
                    );
                } else {
                    assert!(
                        (mask & (1u64 << last)) != 0,
                        "torn tuple: last={last} mask={mask:#x}"
                    );
                }
            }
        }));
        for h in handles {
            h.join().unwrap();
        }
    }
}

/// Design D6 / A3 (Cycle 002): the snapshot wire must be *completely defined* —
/// no implicit `#[repr(C)]` padding that a whole-object `vm_write` would copy to
/// guest memory as undefined bytes. Two proofs:
///
/// 1. `size_of` equals the sum of every field's size (any implicit or trailing
///    padding would make `size_of` larger and unaccounted).
/// 2. [`UartSmpSnapshot::wire_bytes`] covers all `WIRE_SIZE` bytes, every
///    documented field encoding lands at its pinned `repr(C)` offset, and the
///    five reserved gaps (`_r0.._r4`) serialize as zero.
#[test]
fn uart_smp_snapshot_wire_has_no_implicit_padding_and_reserved_zero() {
    use uart_snapshot_types::{UART_SMP_SNAPSHOT_MAGIC, UartSmpSnapshot};

    // 1. No implicit padding: total byte size equals the sum of field sizes.
    //    With explicit reserved arrays filling every alignment gap, every struct
    //    byte is a named field (nothing hidden for `vm_write` to leak).
    let field_sizes: usize = [4, 4, 8, 8, 8, 4, 4, 8, 4, 4, 8, 4, 4, 8, 4, 4, 1, 1, 2, 4, 1, 7]
        .iter()
        .sum::<usize>() // magic..(incl. reserved _r0.._r4) == 104
        + 6 * 8 // trailing u64: irq_events, rx_polls, tx_polls, ipi_sent, ipi_received, affinity_rejects
        + [1, 1, 2, 4, 4, 4, 4, 4] // Task 4.2 migration: states, _r5, from/to x2, _r6
            .iter()
            .sum::<usize>()
        + 6 * 8; // migration u64: req/obs/rejects x2
    assert_eq!(
        core::mem::size_of::<UartSmpSnapshot>(),
        UartSmpSnapshot::WIRE_SIZE,
        "WIRE_SIZE must equal the struct size"
    );
    assert_eq!(
        core::mem::size_of::<UartSmpSnapshot>(),
        224,
        "documented ABI total must stay 224 (Task 4.2 migration extension appended)"
    );
    assert_eq!(
        core::mem::size_of::<UartSmpSnapshot>(),
        field_sizes,
        "struct size must equal sum of explicit field sizes (no implicit padding)"
    );

    // 2. Build a representative frame and serialize it.
    let mut s = UartSmpSnapshot::zeroed();
    s.magic = UART_SMP_SNAPSHOT_MAGIC;
    s.configured_harts = 16;
    s.schedulable_mask = 0xffff;
    s.rx_affinity = 14;
    s.tx_affinity = 15;
    s.irq_last_hart = 14;
    s.irq_hart_mask = 0x4000;
    s.rx_last_hart = 14;
    s.rx_hart_mask = 0x4000;
    s.tx_last_hart = 15;
    s.tx_hart_mask = 0x8000;
    s.rx_occupancy = 10;
    s.tx_vacancy = 20;
    s.ring_empty = 1;
    s.copier_active = 1;
    s.staged_bytes = 4;
    s.transmitter_empty = 1;
    s.irq_events = 7;
    s.rx_polls = 3;
    s.tx_polls = 9;
    s.ipi_sent = 2;
    s.ipi_received = 2;
    s.affinity_rejects = 0;
    // Task 4.2 migration extension round-trip values.
    s.rx_migration_state = 2; // Restored
    s.tx_migration_state = 1; // Widened
    s.rx_migration_from = 14;
    s.rx_migration_to = 12;
    s.tx_migration_from = 15;
    s.tx_migration_to = 13;
    s.rx_migration_requested_polls = 100;
    s.rx_migration_observed_polls = 101;
    s.rx_migration_rejects = 1;
    s.tx_migration_requested_polls = 7;
    s.tx_migration_observed_polls = 0;
    s.tx_migration_rejects = 2;

    let b = s.wire_bytes();
    assert_eq!(b.len(), UartSmpSnapshot::WIRE_SIZE, "wire covers all bytes");

    // Field encodings land at pinned offsets (LE).
    assert_eq!(
        u32::from_le_bytes(b[0..4].try_into().unwrap()),
        UART_SMP_SNAPSHOT_MAGIC
    );
    assert_eq!(u32::from_le_bytes(b[4..8].try_into().unwrap()), 16);
    assert_eq!(u64::from_le_bytes(b[8..16].try_into().unwrap()), 0xffff);
    assert_eq!(u64::from_le_bytes(b[16..24].try_into().unwrap()), 14);
    assert_eq!(u64::from_le_bytes(b[24..32].try_into().unwrap()), 15);
    assert_eq!(i32::from_le_bytes(b[32..36].try_into().unwrap()), 14);
    assert_eq!(u64::from_le_bytes(b[40..48].try_into().unwrap()), 0x4000);
    assert_eq!(u64::from_le_bytes(b[72..80].try_into().unwrap()), 0x8000);
    assert_eq!(u64::from_le_bytes(b[104..112].try_into().unwrap()), 7);
    assert_eq!(u64::from_le_bytes(b[144..152].try_into().unwrap()), 0);
    // Migration extension lands at the pinned offsets (LE).
    assert_eq!(b[152], 2, "rx_migration_state");
    assert_eq!(b[153], 1, "tx_migration_state");
    assert_eq!(u32::from_le_bytes(b[156..160].try_into().unwrap()), 14);
    assert_eq!(u32::from_le_bytes(b[160..164].try_into().unwrap()), 12);
    assert_eq!(u32::from_le_bytes(b[164..168].try_into().unwrap()), 15);
    assert_eq!(u32::from_le_bytes(b[168..172].try_into().unwrap()), 13);
    assert_eq!(u64::from_le_bytes(b[176..184].try_into().unwrap()), 100);
    assert_eq!(u64::from_le_bytes(b[184..192].try_into().unwrap()), 101);
    assert_eq!(u64::from_le_bytes(b[192..200].try_into().unwrap()), 1);
    assert_eq!(u64::from_le_bytes(b[200..208].try_into().unwrap()), 7);
    assert_eq!(u64::from_le_bytes(b[208..216].try_into().unwrap()), 0);
    assert_eq!(u64::from_le_bytes(b[216..224].try_into().unwrap()), 2);

    // Reserved gaps must be zero: r0(36..40), r1(52..56), r2(68..72),
    // r3(90..92), r4(97..104), r5(154..156), r6(172..176).
    for range in [
        (36..40),
        (52..56),
        (68..72),
        (90..92),
        (97..104),
        (154..156),
        (172..176),
    ] {
        assert!(
            b[range.clone()].iter().all(|&x| x == 0),
            "reserved byte gap {range:?} must be zero, got {:?}",
            &b[range.clone()]
        );
    }

    // The serialized write path reproduces exactly this byte frame.
    let again = s.wire_bytes();
    assert_eq!(b, again, "wire serialization must be deterministic");
}

/// Design D6 / A3+A6 (Cycle 002, 2.6-R2): the bounded `SMP=16` startup smoke must
/// (a) run the remote-wake causality witness — publish one UART TX byte from the
/// unique pre-TTY producer, require IPI send/receive and copier resume deltas, and
/// convergence of ring/staged — and (b) skip (never FAIL) non-16 legitimate SMP
/// boots, so a valid 2/4/8-hart config does not produce a spurious qualification
/// FAIL. It must also fail closed on a missing-PPI window, i.e. not accept a
/// zero-IPI build as a qualification pass.
#[test]
fn uart_smp_smoke_enforces_remote_wake_causality_and_non16_skip() {
    const SMOKE: &str = include_str!("../kernel/src/drivers/uart_smp_snapshot.rs");

    // (a) The remote-wake publication and the causal delta checks are present.
    assert!(
        SMOKE.contains("bench_tx_push"),
        "smoke must publish the wake byte via the unique pre-TTY TX producer"
    );
    assert!(
        SMOKE.contains("REMOTE_WAKE_BYTE"),
        "smoke must use an explicit wake byte"
    );
    for cond in [
        "tx_resumed_by_wake",
        "ipi_sent_causal",
        "ipi_received_causal",
        "tx_still_on_pinned_hart",
        "tx_ring_converged",
    ] {
        assert!(SMOKE.contains(cond), "smoke must check {cond}");
    }
    // The witness only claims IPI causality when a remote wake is valid: it
    // requires the counters to *increase* (an absent `axtask/ipi` yields zero IPI
    // and is a FAIL), and it never waits on a timer tick.
    assert!(
        SMOKE.contains("snap.ipi_sent > ipi_sent_before")
            && SMOKE.contains("snap.ipi_received > ipi_received_before"),
        "smoke must fail closed when the remote-ready IPI is absent"
    );

    // (b) Non-16 SMP boot must SKIP, not FAIL; single-hart also SKIPs.
    assert!(SMOKE.contains("reason=non-smp-boot"));
    assert!(SMOKE.contains("reason=non-16-smp"), "non-16 SMP must SKIP");
    assert!(
        SMOKE.contains("cpu_num() != 16"),
        "16-hart qualification must be explicit"
    );

    // The smoke must bound its wait with yield (not sleep-poll or busy spin).
    assert!(SMOKE.contains("axtask::yield_now()"));
}

/// Cycle 003 / 2.6-R3 source guard: the `block_on` waker must route a wake
/// through the local-preferred wake entry (`select_wake_run_queue`), never the
/// plain-spawn round-robin (`select_run_queue`), and must request a real
/// `Blocked -> Ready` transition (`resched=true`) so a successful remote enqueue
/// issues exactly one remote-ready IPI. This directly closes the Iteration 000
/// `axtask/ipi` gap where `wake_by_ref` called `unblock_task(task, false)` and
/// so never entered the remote-notification branch for a `block_on`-parked task.
#[test]
fn axtask_waker_uses_wake_specific_routing_and_requests_reschedule() {
    const FUTURE_MOD: &str = include_str!("../crates/axtask/src/future/mod.rs");

    // The waker selects a wake-local run queue, not the spawn round-robin.
    assert!(
        FUTURE_MOD.contains("select_wake_run_queue::<NoPreemptIrqSave>"),
        "wake_by_ref must route through the wake-local run-queue selector"
    );
    assert!(
        !FUTURE_MOD
            .split("select_wake_run_queue")
            .next()
            .unwrap_or("")
            .contains("select_run_queue")
            && !FUTURE_MOD.contains("unblock_task(task, false)"),
        "wake_by_ref must not call the generic round-robin or a no-resched unblock"
    );
    // The wake requests rescheduling so a successful remote `Blocked -> Ready`
    // transition issues the remote-ready IPI.
    assert!(
        FUTURE_MOD.contains("unblock_task(task, true)"),
        "wake_by_ref must request a reschedule (resched=true) on the transition"
    );
}

/// Cycle 003 / 2.6-R4 source guard: the bounded startup smoke must not treat the
/// TX copier *resuming* (poll count advancing at poll entry) as the drained
/// endpoint. It must wait, in the same pre-TTY single-producer window, for the
/// four-stage drain (`TxCompletion::is_drained`: ring empty, copier inactive,
/// staged zero, transmitter empty) *and* the vacancy being restored to its
/// pre-publication baseline before the terminal read evaluates convergence.
#[test]
fn uart_smp_smoke_waits_for_completion_convergence_before_terminal_read() {
    const SMOKE_RS: &str = include_str!("../kernel/src/drivers/uart_smp_snapshot.rs");
    if let Err(reason) = completion_guard::check(SMOKE_RS) {
        panic!("UART smoke does not require completion convergence: {reason}");
    }
}

mod completion_guard {
    use super::production_guard::block_after;

    /// Checks that a smoke body waits for the four-stage drain and vacancy
    /// restoration *before* reading the terminal snapshot, and fails closed when
    /// it would evaluate ring convergence right after resume only.
    pub fn check(source: &str) -> Result<(), String> {
        if !source.contains("is_drained") {
            return Err("smoke must wait on the four-stage drain predicate \
                        TxCompletion::is_drained"
                .into());
        }
        if !source.contains("tx_vacancy_before") {
            return Err(
                "smoke must bind the restored vacancy to the pre-publication baseline".into(),
            );
        }
        let body = block_after(source, "pub fn snapshot_boot_smoke")
            .ok_or("snapshot_boot_smoke body not found")?;
        let snapshot_read = body
            .rfind("let snap = snapshot();")
            .ok_or("smoke must read a converged snapshot")?;
        let drain_wait = body.rfind("is_drained").ok_or("drain wait must exist")?;
        if drain_wait > snapshot_read {
            return Err(
                "smoke must wait for drain/vacancy convergence before reading the terminal \
                 snapshot"
                    .into(),
            );
        }
        Ok(())
    }
}

/// RED witness for 2.6-R4: a smoke that only waits for the copier to *resume*
/// (poll advance) and reads the terminal state immediately afterwards must be
/// rejected — that is the pre-fix behavior the convergence wait closes.
#[test]
fn completion_guard_rejects_resume_only_smoke() {
    const RESUME_ONLY: &str = r#"
pub fn snapshot_boot_smoke() {
    loop { if tx_polls > before { break; } yield_now(); }
    let snap = snapshot();
    checks.push(("tx_ring_converged", snap.tx_vacancy == before && snap.staged == 0));
}
"#;
    assert!(completion_guard::check(RESUME_ONLY).is_err());
}

/// GREEN fixture counterpart: the same skeleton with a convergence wait before
/// the terminal read is accepted.
#[test]
fn completion_guard_accepts_convergence_wait() {
    const WITH_WAIT: &str = r#"
pub fn snapshot_boot_smoke() {
    loop { if tx_polls > before { break; } yield_now(); }
    loop { let c = tx_completion(); if c.is_drained() && vacancy == tx_vacancy_before { break; } yield_now(); }
    let snap = snapshot();
    checks.push(("tx_ring_converged", snap.tx_vacancy == tx_vacancy_before && snap.staged == 0));
}
"#;
    assert!(
        completion_guard::check(WITH_WAIT).is_ok(),
        "convergence-wait smoke must be accepted"
    );
}

/// ── MS08 Iteration 002 (network placement, V5, timer-disabled witness) ──

/// Task 3.1: the kernel network adapter must consume the shared placement
/// policy (`net_placement::place` over the published schedulable set) and
/// derive singleton owner/runner harts before any network task starts, never
/// hard-coding hart IDs.
#[test]
fn network_adapter_consumes_shared_placement_policy() {
    const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    assert!(IRQ.contains("net_placement::place()"), "init must call place()");
    assert!(IRQ.contains("record_owner_pinned"), "owner pin must be published before enqueue");
    assert!(IRQ.contains("record_runner_pinned"), "runner pin must be published before enqueue");
    // No hard-coded hart number in the init placement path.
    let init = production_guard::block_after(IRQ, "pub fn init_virtio_net_irq_diag")
        .expect("init body");
    assert!(
        !init.contains("pl.net_owner = "),
        "placement must be derived, not overwritten"
    );
}

/// Task 3.2: ordered startup — spawn-free Service install (runner after
/// secondary-ready), pin runner before IRQ registration, pin owner only after
/// successful registration, and save both handles.
#[test]
fn network_startup_order_runner_before_register_owner_after_and_handles() {
    const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    const LIB: &str = include_str!("../crates/axnet/src/lib.rs");
    let init = production_guard::block_after(IRQ, "pub fn init_virtio_net_irq_diag")
        .expect("init body");

    // Service installation in axnet is spawn-free (Task 3.2).
    assert!(
        !LIB.contains("start_stack_runner("),
        "init_network must not spawn a runner early"
    );

    // Pinned runner start before IRQ registration.
    let runner_pos = init
        .find("start_stack_runner_affinity")
        .expect("init must start the pinned runner");
    let register_pos = init
        .find("axhal::irq::register")
        .expect("init must register the IRQ");
    assert!(
        runner_pos < register_pos,
        "pinned runner must start before IRQ registration"
    );

    // Pinned owner start strictly after IRQ registration, and saved.
    let owner_pos = init
        .find("start_rx_task_affinity")
        .expect("init must start the pinned owner after registration");
    assert!(
        register_pos < owner_pos,
        "pinned owner must start only after IRQ registration succeeds"
    );
    assert!(
        init.contains("set_owner_task") && init.contains("set_runner_task"),
        "both role handles must be saved"
    );
}

/// Task 3.2: IRQ-registration failure must return before starting the async
/// owner, keeping the polling fallback (register-before-start).
#[test]
fn network_registration_failure_returns_before_owner_start() {
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
    const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    let init = production_guard::block_after(IRQ, "pub fn init_virtio_net_irq_diag")
        .expect("init body");
    let failure = production_guard::block_after(init, "if !axhal::irq::register")
        .expect("registration failure branch");
    assert!(
        failure.contains("return;"),
        "registration failure must return before owner start"
    );
    assert!(CTL.contains("NET_IRQ_SNAPSHOT_V5"));
}

/// Task 3.2 replan: runner-start failure is a hard stop — the init must not
/// continue to IRQ registration or async-owner startup when no pinned runner
/// exists (a missing runner would leave IRQ/owner without a stack-progress
/// source). The failure branch must return before any owner start.
#[test]
fn network_runner_start_failure_aborts_before_owner() {
    const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    let init = production_guard::block_after(IRQ, "pub fn init_virtio_net_irq_diag")
        .expect("init body");
    let failure = production_guard::block_after(init, "start_stack_runner_affinity")
        .expect("runner start branch");
    // The runner-failure branch must return (abort) and never reach owner start.
    assert!(
        failure.contains("return;"),
        "runner-start failure must abort init (no async owner)"
    );
    // The runner is started before the device-descriptor / MMIO / IRQ checks
    // return, so every no-owner / fallback path retains a pinned runner.
    let runner_pos = init
        .find("start_stack_runner_affinity")
        .expect("pinned runner start");
    let register_pos = init
        .find("axhal::irq::register")
        .expect("IRQ registration");
    let no_virtio_pos = init
        .find("desc.virtio_net")
        .expect("device-descriptor read");
    assert!(
        runner_pos < no_virtio_pos,
        "pinned runner must start before device-descriptor/MMIO validation"
    );
    assert!(
        runner_pos < register_pos,
        "pinned runner must start before IRQ registration"
    );
    // Every post-runner return path leaves the runner active and never starts an
    // owner (no second early return before owner start that drops the runner).
    assert!(
        init.contains("keeping pinned runner, no async owner"),
        "device-missing path must keep the pinned runner"
    );
}

/// Task 3.3: V5 must report DIRECTLY OBSERVED owner/runner execution harts (real
/// poll-site recording via `axnet::owner_hart_tuple` / `runner_hart_tuple`),
/// never a singleton synthesized from the pinned affinity. Affinity and actual
/// execution are separate fields.
#[test]
fn v5_reports_direct_owner_runner_execution_harts() {
    const PLACE: &str = include_str!("../kernel/src/drivers/net_placement.rs");
    const AXNET_LIB: &str = include_str!("../crates/axnet/src/lib.rs");
    const AXNET_ASYNC: &str = include_str!("../crates/axnet/src/async_rx.rs");
    const AXNET_RUNNER: &str = include_str!("../crates/axnet/src/stack_runner.rs");
    // The placement snapshot must populate the execution fields from axnet's
    // real poll-site recording, never from the pinned affinity.
    let snap = production_guard::block_after(PLACE, "pub fn snapshot()").expect("snapshot");
    assert!(
        snap.contains("axnet::owner_hart_tuple()") && snap.contains("axnet::runner_hart_tuple()"),
        "net_placement::snapshot must read the actual execution harts"
    );
    assert!(
        !snap.contains("owner_last_hart: owner_pinned_hart()")
            && !snap.contains("runner_last_hart: runner_pinned_hart()"),
        "execution harts must not be synthesized from pinned affinity"
    );
    // The accessors are wired from axnet and record the hart at every poll entry.
    assert!(
        AXNET_LIB.contains("owner_hart_tuple") && AXNET_LIB.contains("runner_hart_tuple"),
        "axnet must export the real execution-hart accessors"
    );
    assert!(
        AXNET_ASYNC.contains("this.telemetry.hart.record("),
        "owner poll must record its execution hart"
    );
    assert!(
        AXNET_RUNNER.contains("this.telemetry.hart.record("),
        "runner poll must record its execution hart"
    );
}

/// Task 3.3 V5 ABI / Plan Review finding 5: an exact expected-size and
/// appended-field offset table (no modulo-size proxy), plus a byte-for-byte
/// V4-prefix check. V5's leading field is V4 at offset 0 and every appended
/// field is a fixed-order u64, so a V4 consumer reading the prefix observes
/// identical bytes.
#[test]
fn v5_struct_matches_exact_size_and_appended_offsets() {
    use core::mem::{offset_of, size_of};
    use virtio_net_irq_logic::{IrqSnapshotV4, IrqSnapshotV5};

    assert_eq!(offset_of!(IrqSnapshotV5, v4), 0, "V5 must start with V4");
    let base = size_of::<IrqSnapshotV4>();
    assert_eq!(
        base % 8,
        0,
        "V4 size must be whole u64 words (no padding inside the prefix)"
    );
    // The fixed MS08 V5 ABI: 31 appended u64 fields in exactly this order.
    let appended: [(usize, fn() -> usize, &str); 31] = [
        (0, || offset_of!(IrqSnapshotV5, configured_harts), "configured_harts"),
        (1, || offset_of!(IrqSnapshotV5, schedulable_mask), "schedulable_mask"),
        (2, || offset_of!(IrqSnapshotV5, owner_affinity), "owner_affinity"),
        (3, || offset_of!(IrqSnapshotV5, runner_affinity), "runner_affinity"),
        (4, || offset_of!(IrqSnapshotV5, irq_last_hart), "irq_last_hart"),
        (5, || offset_of!(IrqSnapshotV5, irq_hart_mask), "irq_hart_mask"),
        (6, || offset_of!(IrqSnapshotV5, irq_events), "irq_events"),
        (7, || offset_of!(IrqSnapshotV5, owner_last_hart), "owner_last_hart"),
        (8, || offset_of!(IrqSnapshotV5, owner_hart_mask), "owner_hart_mask"),
        (9, || offset_of!(IrqSnapshotV5, owner_events), "owner_events"),
        (10, || offset_of!(IrqSnapshotV5, runner_last_hart), "runner_last_hart"),
        (11, || offset_of!(IrqSnapshotV5, runner_hart_mask), "runner_hart_mask"),
        (12, || offset_of!(IrqSnapshotV5, runner_events), "runner_events"),
        (13, || offset_of!(IrqSnapshotV5, ipi_sent), "ipi_sent"),
        (14, || offset_of!(IrqSnapshotV5, ipi_received), "ipi_received"),
        (15, || offset_of!(IrqSnapshotV5, affinity_rejects), "affinity_rejects"),
        (16, || offset_of!(IrqSnapshotV5, migration_owner_state), "migration_owner_state"),
        (17, || offset_of!(IrqSnapshotV5, migration_runner_state), "migration_runner_state"),
        (18, || offset_of!(IrqSnapshotV5, witness_phase), "witness_phase"),
        (19, || offset_of!(IrqSnapshotV5, witness_completed), "witness_completed"),
        (20, || offset_of!(IrqSnapshotV5, witness_failed), "witness_failed"),
        (21, || offset_of!(IrqSnapshotV5, witness_timer_restored), "witness_timer_restored"),
        (22, || offset_of!(IrqSnapshotV5, witness_start_rejects), "witness_start_rejects"),
        (23, || offset_of!(IrqSnapshotV5, witness_missing_restore), "witness_missing_restore"),
        (24, || offset_of!(IrqSnapshotV5, witness_duplicate_terminal), "witness_duplicate_terminal"),
        (25, || offset_of!(IrqSnapshotV5, witness_trigger_rejects), "witness_trigger_rejects"),
        (26, || offset_of!(IrqSnapshotV5, witness_illegal_transitions), "witness_illegal_transitions"),
        (27, || offset_of!(IrqSnapshotV5, witness_last_target_hart), "witness_last_target_hart"),
        (28, || offset_of!(IrqSnapshotV5, witness_last_trigger_hart), "witness_last_trigger_hart"),
        (29, || offset_of!(IrqSnapshotV5, witness_target_ipi_before), "witness_target_ipi_before"),
        (30, || offset_of!(IrqSnapshotV5, witness_target_ipi_after), "witness_target_ipi_after"),
    ];
    for (index, offset, name) in appended {
        assert_eq!(
            offset(),
            base + index * 8,
            "V5 appended field {name} must sit at word {index} (byte {})",
            base + index * 8
        );
    }
    assert_eq!(
        size_of::<IrqSnapshotV5>(),
        base + appended.len() * 8,
        "V5 total size must be exactly V4 + 31 u64 words (no padding anywhere)"
    );
}

/// A V4 frame with distinctive non-zero prefix values so a byte-prefix
/// comparison cannot pass on two implicitly-zeroed structs.
fn sample_v4() -> virtio_net_irq_logic::IrqSnapshotV4 {
    let mut v4 = zero_v4();
    v4.v3.total = 0x1111_1111_1111_1111;
    v4.v3.ack_count = 7;
    v4.v3.task_poll = 42;
    v4.v3.tx_submit = 5;
    v4.v3.drop_frame_too_large = 9;
    v4.current_queue_epoch = 3;
    v4.fault_stage = 2;
    v4
}

/// Task 3.3 V5 ABI / Plan Review finding 5: the first `size_of::<V4>()` bytes
/// of a V5 must equal the V4's bytes exactly (V4 consumer compatibility).
#[test]
fn v5_prefix_is_byte_for_byte_identical_to_v4() {
    use virtio_net_irq_logic::{IrqSnapshotV4, IrqSnapshotV5};

    let v4 = sample_v4();
    let v5 = IrqSnapshotV5 {
        v4,
        ..zero_v5()
    };
    let prefix_len = core::mem::size_of::<IrqSnapshotV4>();
    // Both structs are #[repr(C)] u64-only sequences with no padding (proven by
    // the exact offset/size table above), so a byte comparison of the prefix is
    // well-defined.
    let v4_bytes = unsafe {
        core::slice::from_raw_parts((&v4 as *const IrqSnapshotV4).cast::<u8>(), prefix_len)
    };
    let v5_bytes = unsafe {
        core::slice::from_raw_parts((&v5 as *const IrqSnapshotV5).cast::<u8>(), prefix_len)
    };
    assert_eq!(
        v4_bytes, v5_bytes,
        "V5 must embed V4 as an exact byte-for-byte prefix"
    );
}

/// Task 4.3 (superseding the Task 3.3 zero-fill guard): the production
/// assembler (`irq_snapshot_v5`) must publish the packed controlled-migration
/// state for both roles — a struct-literal test alone cannot catch a
/// production filler that leaves the fields undefined or omits them.
#[test]
fn v5_production_assembler_initializes_reserved_fields() {
    const V: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    let body = production_guard::block_after(V, "pub fn irq_snapshot_v5").expect("v5 assembler");
    assert!(
        body.contains("pack_migration_view"),
        "production V5 filler must publish the packed migration state"
    );
    assert!(
        body.contains("migration_owner_state,"),
        "production V5 must populate migration_owner_state from the placement view"
    );
    assert!(
        body.contains("migration_runner_state,"),
        "production V5 must populate migration_runner_state from the placement view"
    );
    // The direct Task 3.3 execution fields must be populated from the
    // placement snapshot (never synthesized from affinity).
    for field in [
        "owner_last_hart: place.owner_last_hart",
        "owner_hart_mask: place.owner_hart_mask",
        "owner_events: place.owner_events",
        "runner_last_hart: place.runner_last_hart",
        "runner_hart_mask: place.runner_hart_mask",
        "runner_events: place.runner_events",
    ] {
        assert!(body.contains(field), "production V5 filler must source {field}");
    }
}

/// A fully-zeroed V4 frame (exhaustive struct literal: the type has no hidden
/// reserved padding, so every byte is a defined field).
fn zero_v4() -> virtio_net_irq_logic::IrqSnapshotV4 {
    virtio_net_irq_logic::IrqSnapshotV4 {
        v3: virtio_net_irq_logic::IrqSnapshotV3 {
            total: 0,
            used_ring: 0,
            config_change: 0,
            combined: 0,
            unknown: 0,
            spurious: 0,
            ack_count: 0,
            uart_irq_count: 0,
            restore_violation: 0,
            irq_enabled_entry: 0,
            rx_lifecycle: 0,
            rx_owner: 0,
            isr_publish: 0,
            isr_wake: 0,
            software_nudge: 0,
            task_poll: 0,
            reaped: 0,
            refilled: 0,
            delivered: 0,
            non_ip_consumed: 0,
            budget_exhausted: 0,
            self_yield: 0,
            router_full_wait: 0,
            space_wake: 0,
            empty_check: 0,
            fault: 0,
            last_error_stage: 0,
            last_error_code: 0,
            rx_slot_occupancy: 0,
            rx_slot_high_water: 0,
            rx_slot_full: 0,
            rx_slot_enqueue: 0,
            rx_slot_dequeue: 0,
            rx_slot_space_event: 0,
            tx_slot_occupancy: 0,
            tx_slot_high_water: 0,
            tx_slot_full: 0,
            tx_slot_enqueue: 0,
            tx_slot_dequeue: 0,
            tx_slot_space_event: 0,
            tx_submit: 0,
            tx_again: 0,
            tx_completion: 0,
            tx_reclaim: 0,
            tx_buffer_available: 0,
            tx_buffer_inflight: 0,
            tx_descriptor_available: 0,
            tx_descriptor_inflight: 0,
            reclaim_exhausted: 0,
            rx_exhausted: 0,
            submit_exhausted: 0,
            queue_generation: 0,
            queue_wake: 0,
            last_accepted: 0,
            live: 0,
            queued: 0,
            device_owned: 0,
            flush_target: 0,
            flush_success: 0,
            flush_error: 0,
            flush_busy: 0,
            flush_cancel: 0,
            hold_mode: 0,
            lease_expiry: 0,
            auto_release_failure: 0,
            lifecycle_fault: 0,
            ownership_invariant: 0,
            drop_malformed_ip: 0,
            drop_no_route: 0,
            drop_route_source_mismatch: 0,
            drop_unsupported_address: 0,
            drop_frame_too_large: 0,
        },
        current_valid: 0,
        current_queue_epoch: 0,
        current_socket_epoch: 0,
        current_link_generation: 0,
        current_link_state: 0,
        current_owner_available: 0,
        current_owner_device_owned: 0,
        current_owner_quarantined: 0,
        fault_valid: 0,
        fault_stage: 0,
        fault_cause: 0,
        fault_queue_epoch: 0,
        fault_owner_available: 0,
        fault_owner_device_owned: 0,
        fault_owner_quarantined: 0,
    }
}

/// A fully-zeroed V5 frame: V4 prefix plus every appended field zero
/// (reserved migration fields included).
fn zero_v5() -> virtio_net_irq_logic::IrqSnapshotV5 {
    virtio_net_irq_logic::IrqSnapshotV5 {
        v4: zero_v4(),
        configured_harts: 0,
        schedulable_mask: 0,
        owner_affinity: 0,
        runner_affinity: 0,
        irq_last_hart: 0,
        irq_hart_mask: 0,
        irq_events: 0,
        owner_last_hart: 0,
        owner_hart_mask: 0,
        owner_events: 0,
        runner_last_hart: 0,
        runner_hart_mask: 0,
        runner_events: 0,
        ipi_sent: 0,
        ipi_received: 0,
        affinity_rejects: 0,
        migration_owner_state: 0,
        migration_runner_state: 0,
        witness_phase: 0,
        witness_completed: 0,
        witness_failed: 0,
        witness_timer_restored: 0,
        witness_start_rejects: 0,
        witness_missing_restore: 0,
        witness_duplicate_terminal: 0,
        witness_trigger_rejects: 0,
        witness_illegal_transitions: 0,
        witness_last_target_hart: 0,
        witness_last_trigger_hart: 0,
        witness_target_ipi_before: 0,
        witness_target_ipi_after: 0,
    }
}

/// Task 3.3: negative fixture — a V5 frame whose reserved migration bytes start
/// nonzero violates the "initialized zero" requirement; a mutable
/// assembly-style write must keep the exact size (no hidden padding).
#[test]
fn v5_reserved_migration_fields_must_start_zero() {
    let mut snap = zero_v5();
    assert_eq!(snap.migration_owner_state, 0);
    assert_eq!(snap.migration_runner_state, 0);
    // A nonzero reserved write is visible and does not change the layout.
    snap.migration_owner_state = 1;
    snap.v4.v3.total = 3; // prefix field still readable/writable
    snap.owner_events = 9;
    assert_eq!(
        core::mem::size_of_val(&snap),
        core::mem::size_of::<virtio_net_irq_logic::IrqSnapshotV5>(),
        "assembly-style writes must not change the exact V5 size"
    );
}

/// Task 3.4: the host state machine closes single-flight reservation, per-run
/// reset, promotion-gated remote trigger (Armed == accepted trigger for a parked
/// target), bounded cancel, and ack-before-terminal.
mod witness_model_tests {
    use super::net_wake_witness_logic::{
        StartReject, TransitionReject, TriggerReject, WitnessMachine, WitnessPhase,
    };

    #[test]
    fn happy_path_promotes_then_completes_exactly_once() {
        // Plan Review finding 2 (final): Armed is reached ONLY by an accepted
        // remote trigger (promotion). The happy path: reserve -> trigger promotes
        // (Starting -> Armed, trigger hart recorded) -> woken -> restore+ack ->
        // Completed exactly once.
        let mut m = WitnessMachine::new();
        m.reserve(1, 7).unwrap();
        m.trigger(3).unwrap();
        m.on_woken().unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        // duplicate terminal is rejected and does not change state
        assert_eq!(m.complete(), Err(TransitionReject::DuplicateTerminal));
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Completed);
        assert_eq!(s.completed, 1);
        assert_eq!(s.timer_restored, 1);
        assert_eq!(s.missing_restore, 0);
        assert_eq!(s.duplicate_terminal, 1);
    }

    #[test]
    fn terminal_without_restore_ack_is_rejected_not_published() {
        let mut m = WitnessMachine::new();
        m.reserve(1, 3).unwrap();
        m.trigger(5).unwrap();
        // complete() before restore_ack must be REJECTED and leave state unchanged.
        assert_eq!(m.complete(), Err(TransitionReject::RestoreNotAcked));
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Armed, "no terminal may be published");
        assert_eq!(s.completed, 0);
        assert_eq!(s.missing_restore, 1);
        assert_eq!(s.timer_restored, 0);
    }

    #[test]
    fn concurrent_start_is_rejected_and_counts() {
        let mut m = WitnessMachine::new();
        m.reserve(10, 1).unwrap();
        assert_eq!(m.reserve(11, 2), Err(StartReject::Busy));
        assert_eq!(m.reserve(12, 3), Err(StartReject::Busy));
        let s = m.snapshot();
        assert_eq!(s.start_rejects, 2);
        assert_eq!(s.target_hart, 1, "reserve must not overwrite the holder");
        assert_eq!(s.scope, 10, "reserve must not overwrite the holder.");
    }

    #[test]
    fn reserve_rejects_unknown_scope_sentinel_as_busy() {
        // An injected UNKNOWN scope is an invalid ownership token: it must be
        // rejected (fail closed) without mutating the machine.
        let mut m = WitnessMachine::new();
        assert_eq!(
            m.reserve(super::net_wake_witness_logic::UNKNOWN_SCOPE, 4),
            Err(StartReject::Busy)
        );
        assert_eq!(m.snapshot().phase, WitnessPhase::Idle);
    }

    #[test]
    fn spawn_failure_rolls_back_reservation_to_idle() {
        let mut m = WitnessMachine::new();
        m.reserve(20, 4).unwrap();
        // spawn failed before enqueue: rollback must free the run for a retry.
        assert_eq!(m.reserve_rollback(), Ok(()));
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Idle);
        assert_eq!(s.target_hart, super::net_wake_witness_logic::UNKNOWN_HART);
        assert_eq!(s.scope, super::net_wake_witness_logic::UNKNOWN_SCOPE);
        // A fresh reserve now succeeds (new scope token).
        m.reserve(21, 5).unwrap();
        assert_eq!(m.snapshot().phase, WitnessPhase::Starting);
    }

    #[test]
    fn trigger_is_atomic_promotion_from_starting() {
        // Plan Review finding 2 (final): `trigger` is the ONLY way to Armed, and
        // it requires the still-Starting (parked, waiter-registered) run.
        // Idle machine: trigger rejected.
        let mut idle = WitnessMachine::new();
        assert_eq!(idle.trigger(0), Err(TriggerReject::NotArmed));
        let mut m = WitnessMachine::new();
        m.reserve(30, 9).unwrap();
        // Same-hart trigger rejected (not remote).
        assert_eq!(m.trigger(9), Err(TriggerReject::SameHart));
        // Remote (different, schedulable) hart accepted: Starting -> Armed and
        // the trigger hart is recorded in the SAME atomic step.
        assert_eq!(m.trigger(2), Ok(()));
        let s = m.snapshot();
        assert_eq!(s.trigger_rejects, 1);
        assert_eq!(s.phase, WitnessPhase::Armed);
        assert_eq!(s.trigger_hart, 2);
        // A second trigger (already promoted) is rejected.
        assert_eq!(m.trigger(4), Err(TriggerReject::NotArmed));
        assert_eq!(m.snapshot().trigger_rejects, 2);
    }

    #[test]
    fn trigger_loses_cleanly_to_cancellation() {
        // Follow-up instruction 4: a timeout may cancel before promotion; the
        // trigger must lose cleanly (reject WITHOUT state change beyond the
        // reject counter) and never produce an Armed run.
        let mut m = WitnessMachine::new();
        m.reserve(31, 9).unwrap();
        m.request_cancel().unwrap();
        assert_eq!(m.trigger(2), Err(TriggerReject::Cancelled));
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Starting, "cancelled run must not promote");
        assert_eq!(s.trigger_rejects, 1);
        // The run then converges through the cancel path to a bounded Failed.
        m.restore_ack().unwrap();
        m.fail().unwrap();
        assert_eq!(m.snapshot().phase, WitnessPhase::Failed);
    }

    #[test]
    fn cancel_requires_restore_ack_before_failed_terminal() {
        let mut m = WitnessMachine::new();
        m.reserve(40, 6).unwrap();
        m.trigger(2).unwrap();
        assert_eq!(m.request_cancel(), Ok(()));
        m.on_woken().unwrap();
        // fail() before restore_ack must be rejected.
        assert_eq!(m.fail(), Err(TransitionReject::RestoreNotAcked));
        assert_eq!(m.snapshot().phase, WitnessPhase::Woken);
        m.restore_ack().unwrap();
        m.fail().unwrap();
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Failed);
        assert_eq!(s.failed, 1);
        assert_eq!(s.timer_restored, 1);
        // The rejected fail()-before-ack is counted as a missing-restore attempt.
        assert_eq!(s.missing_restore, 1);
    }

    #[test]
    fn terminal_releases_the_run_and_allows_a_fresh_start() {
        let mut m = WitnessMachine::new();
        m.reserve(50, 1).unwrap();
        m.trigger(2).unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        m.reserve(51, 2).unwrap(); // must be accepted after terminal
        m.trigger(3).unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        let s = m.snapshot();
        assert_eq!(s.completed, 2);
        assert_eq!(s.phase, WitnessPhase::Completed);
    }

    #[test]
    fn trigger_attribution_is_preserved_through_terminal() {
        // Task 3.3/3.4: the accepted run's target + remote trigger harts must
        // survive published terminal, so a terminal V5 snapshot can attribute
        // the run's remote-wake causality instead of reading transient state.
        let mut m = WitnessMachine::new();
        m.reserve(60, 4).unwrap();
        m.trigger(9).unwrap();
        m.on_woken().unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Completed);
        assert_eq!(s.last_target_hart, 4, "target must survive terminal");
        assert_eq!(s.last_trigger_hart, 9, "remote trigger must survive terminal");
        // The transient target/trigger AND the ownership scope are released.
        assert_eq!(s.target_hart, super::net_wake_witness_logic::UNKNOWN_HART);
        assert_eq!(s.trigger_hart, super::net_wake_witness_logic::UNKNOWN_HART);
        assert_eq!(s.scope, super::net_wake_witness_logic::UNKNOWN_SCOPE);
    }

    #[test]
    fn supervisor_cancel_fires_on_armed_or_starting_for_its_own_scope_and_target() {
        // Task 3.4 / finding 1: the supervisor predicate fires (cancel) exactly when
        // the target is still Armed — or still Starting (delayed target) — on the
        // supervisor's OWN scope; it stays inert for its own run once woken /
        // terminaled, and never touches another target or a different (rolled-back /
        // successor) scope.
        use super::net_wake_witness_logic::supervisor_should_cancel;
        let mut m = WitnessMachine::new();
        m.reserve(70, 3).unwrap();
        let my_scope = m.snapshot().scope;
        // While still Starting (target parking, no trigger yet): supervised.
        assert!(
            supervisor_should_cancel(&m.snapshot(), 3, my_scope),
            "Starting on own target (own scope) must be supervised"
        );
        // Promotion (accepted remote trigger) keeps the run under supervision
        // until the target resumes.
        m.trigger(5).unwrap();
        let s = m.snapshot();
        assert!(
            supervisor_should_cancel(&s, 3, my_scope),
            "Armed on own target (own scope) must request cancel"
        );
        // Different target => never cancel.
        assert!(!supervisor_should_cancel(&s, 7, my_scope));
        // A stale supervisor (a different scope) must NOT cancel this run — the
        // ordinary scoped ownership that replaces the removed run_id counter.
        assert!(!supervisor_should_cancel(&s, 3, my_scope + 99));
        // Once woken (target progressing), no cancel.
        m.on_woken().unwrap();
        assert!(!supervisor_should_cancel(&m.snapshot(), 3, my_scope));
    }

    #[test]
    fn reserve_distinguishes_scopes_without_a_run_counter() {
        // Task 3.4: every START (including a rollback-and-retry) carries a fresh
        // scope token, so an orphaned supervisor from a rolled-back attempt cannot
        // cancel a later run even if placement re-selects the same target hart.
        // The scope is an ordinary ownership/lifetime token, not the removed
        // monotonic run_id evidence identity.
        use super::net_wake_witness_logic::supervisor_should_cancel;
        let mut m = WitnessMachine::new();
        m.reserve(100, 1).unwrap();
        let first_scope = m.snapshot().scope;
        m.reserve_rollback().unwrap();
        m.reserve(101, 1).unwrap();
        let second_scope = m.snapshot().scope;
        // The rollback-and-retry targets the SAME hart; promote it via trigger.
        m.trigger(2).unwrap();
        assert!(
            supervisor_should_cancel(&m.snapshot(), 1, second_scope),
            "the current supervisor (fresh scope) cancels its own run"
        );
        assert!(
            !supervisor_should_cancel(&m.snapshot(), 1, first_scope),
            "the stale supervisor from the rolled-back attempt must not cancel the retry"
        );
    }

    #[test]
    fn reserve_rejects_a_duplicate_scope_as_busy_while_live() {
        // A live run owns exactly one scope; reusing it for a second reserve must
        // be rejected by the single-flight gate (not a run-identity mechanism).
        let mut m = WitnessMachine::new();
        m.reserve(110, 2).unwrap();
        assert_eq!(m.reserve(110, 3), Err(StartReject::Busy));
    }

    #[test]
    fn supervisor_deadline_is_relative_to_now() {
        // Finding 1: the supervisor must use a *relative* deadline. Feeding a bare
        // 5-second duration to `sleep_until` (absolute wall-clock) expires
        // immediately on any boot >5s old. A now + period seam must therefore
        // shift with the supplied now, so a late-starting boot still supervises
        // for a full period.
        use super::net_wake_witness_logic::supervisor_deadline;
        // now = 10s: absolute (buggy) would return 5_000_000; relative must return 15s.
        assert_eq!(supervisor_deadline(10_000_000, 5_000_000), 15_000_000);
        // Zero-clock boot: relative just returns the period.
        assert_eq!(supervisor_deadline(0, 5_000_000), 5_000_000);
        // A later now must yield a correspondingly later deadline.
        assert_eq!(supervisor_deadline(1_000_000_000, 5_000_000), 1_005_000_000);
    }

    #[test]
    fn supervisor_owns_a_delayed_starting_target() {
        // Plan Review finding 1: if the target task is not scheduled before the
        // deadline, the supervisor must NOT exit leaving the run unsupervised. It
        // owns the run while it is still `Starting`: the predicate fires, the
        // machine accepts the cancel, and a delayed target that wakes sees the
        // cancel flag, restores its timer, and converges to a bounded Failed
        // terminal — it can never enter an unsupervised timer-disabled park.
        use super::net_wake_witness_logic::supervisor_should_cancel;
        let mut m = WitnessMachine::new();
        m.reserve(200, 4).unwrap();
        let my_scope = m.snapshot().scope;
        // Deadline arrives while the target has not run: phase is still Starting.
        assert_eq!(m.snapshot().phase, WitnessPhase::Starting);
        let s = m.snapshot();
        assert!(
            supervisor_should_cancel(&s, 4, my_scope),
            "supervisor must own a run still Starting at the deadline"
        );
        // A stale supervisor (different scope) still must not touch this run.
        assert!(!supervisor_should_cancel(&s, 4, my_scope + 99));
        assert!(!supervisor_should_cancel(&s, 5, my_scope));
        // The cancel is accepted in Starting.
        m.request_cancel().unwrap();
        // The delayed target now runs: park entry observes the cancel flag and
        // returns without promotion (a trigger would now be rejected Cancelled);
        // then restore + ack + bounded Failed terminal.
        assert_eq!(m.trigger(2), Err(TriggerReject::Cancelled));
        m.restore_ack().unwrap();
        m.fail().unwrap();
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Failed);
        assert_eq!(s.failed, 1);
        assert_eq!(s.timer_restored, 1);
        assert_eq!(s.missing_restore, 0);
        // A fresh run can start after the terminal.
        m.reserve(201, 6).unwrap();
        assert_eq!(m.snapshot().phase, WitnessPhase::Starting);
    }

    #[test]
    fn supervisor_predicate_does_not_fire_on_wrong_scope_or_terminal() {
        // The widened Starting|Armed predicate must still be scope-gated and must
        // not leak to Woken / terminal phases: a supervisor arriving after
        // completion (or with a stale scope) never cancels.
        use super::net_wake_witness_logic::supervisor_should_cancel;
        let mut m = WitnessMachine::new();
        m.reserve(210, 2).unwrap();
        let scope = m.snapshot().scope;
        assert!(
            !supervisor_should_cancel(&m.snapshot(), 2, scope + 99),
            "a stale scope must not cancel even a Starting run"
        );
        m.trigger(3).unwrap();
        m.on_woken().unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        assert!(
            !supervisor_should_cancel(&m.snapshot(), 2, scope),
            "terminal runs are never cancelled"
        );
    }

    #[test]
    fn restore_failure_path_never_publishes_terminal() {
        // Task 3.4: a run whose timer restore cannot be acknowledged must never
        // publish a terminal; it only counts a missing-restore attempt and stays
        // at Woken/Armed for a bounded supervisor to own. The supervisor can then
        // request cancel and the target restores+acks before Failed.
        let mut m = WitnessMachine::new();
        m.reserve(120, 5).unwrap();
        m.trigger(2).unwrap();
        m.on_woken().unwrap();
        assert_eq!(m.complete(), Err(TransitionReject::RestoreNotAcked));
        assert_eq!(m.snapshot().phase, WitnessPhase::Woken);
        assert_eq!(m.snapshot().missing_restore, 1);
        assert_eq!(m.snapshot().completed, 0);
        // Supervisor requests cancel, then target restores + acks + fails.
        m.request_cancel().unwrap();
        m.restore_ack().unwrap();
        m.fail().unwrap();
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Failed);
        assert_eq!(s.failed, 1);
        assert_eq!(s.timer_restored, 1);
        assert_eq!(s.missing_restore, 1);
    }

    #[test]
    fn per_run_flags_are_reset_on_reserve() {
        let mut m = WitnessMachine::new();
        // Run 1 is cancelled and terminated Failed.
        m.reserve(130, 8).unwrap();
        m.trigger(2).unwrap();
        m.request_cancel().unwrap();
        m.on_woken().unwrap();
        m.restore_ack().unwrap();
        m.fail().unwrap();
        // Run 2 reserves cleanly and reaches a normal Completed terminal; the
        // stale cancel/restore flags from run 1 must not block or corrupt it.
        m.reserve(131, 10).unwrap();
        m.trigger(3).unwrap();
        m.on_woken().unwrap();
        m.restore_ack().unwrap();
        m.complete().unwrap();
        let s = m.snapshot();
        assert_eq!(s.phase, WitnessPhase::Completed);
        assert_eq!(s.completed, 1);
        assert_eq!(s.failed, 1);
        assert_eq!(s.timer_restored, 2);
        assert_eq!(s.missing_restore, 0);
        assert_eq!(s.duplicate_terminal, 0);
    }

    #[test]
    fn timeout_vs_trigger_serialization_is_deterministic() {
        // Follow-up instruction 5: the machine lock serialises promotion and
        // cancellation; both interleavings converge to exactly one terminal with
        // consistent counters (no duplicate, no missing restore, no double
        // trigger attribution).
        // Interleaving A: cancel wins the lock first.
        let mut a = WitnessMachine::new();
        a.reserve(300, 7).unwrap();
        a.request_cancel().unwrap();
        assert_eq!(a.trigger(2), Err(TriggerReject::Cancelled));
        a.restore_ack().unwrap();
        a.fail().unwrap();
        let sa = a.snapshot();
        assert_eq!(sa.phase, WitnessPhase::Failed);
        assert_eq!(sa.failed, 1);
        assert_eq!(sa.completed, 0);
        assert_eq!(sa.trigger_rejects, 1);
        // Interleaving B: promotion wins first; a later cancel still applies
        // (the run resumes and the target observes CANCELLED at cleanup).
        let mut b = WitnessMachine::new();
        b.reserve(301, 7).unwrap();
        b.trigger(2).unwrap();
        assert_eq!(b.request_cancel(), Ok(()));
        b.on_woken().unwrap();
        b.restore_ack().unwrap();
        b.fail().unwrap();
        let sb = b.snapshot();
        assert_eq!(sb.phase, WitnessPhase::Failed);
        assert_eq!(sb.failed, 1);
        assert_eq!(sb.completed, 0);
        assert_eq!(sb.trigger_rejects, 0);
        assert_eq!(sb.last_trigger_hart, 2, "attribution survives either interleaving");
    }
}

/// Task 3.4: the QEMU binding must disable the target timer before parking and
/// restore it before publishing any terminal (cleanup-before-terminal
/// symmetry), so the missing-restore path cannot silently leave a hung timer.
#[test]
fn witness_binding_preserves_timer_restore_symmetry() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    let body = production_guard::block_after(W, "fn witness_target").expect("witness body");
    // The timer is disabled exactly once, at target entry.
    assert_eq!(
        body.matches("set_enable(axhal::time::irq_num(), false)").count(),
        1,
        "timer must be disabled exactly once"
    );
    // The target never publishes Armed itself (finding 2 final): promotion is
    // the TRIGGER path's job, gated on the saved task being Blocked.
    assert!(
        !body.contains("on_armed"),
        "witness_target must never publish Armed (no on_armed)"
    );
    // Every terminal edge must be preceded by a timer restore + ack.
    let restore = body.find("set_enable(axhal::time::irq_num(), true)");
    let restore_ack = body.find("restore_ack()");
    let complete = body.find("complete()");
    let fail = body.find("fail()");
    assert!(
        restore.is_some()
            && restore_ack.is_some()
            && complete.is_some()
            && fail.is_some()
            && restore.unwrap() < restore_ack.unwrap()
            && restore_ack.unwrap() < complete.unwrap()
            && restore_ack.unwrap() < fail.unwrap(),
        "timer must be restored + acked before any terminal edge"
    );
}

/// Plan Review finding 2 (final): the target registers its waiter and parks
/// WITHOUT touching the machine; `Armed` can only be published by the TRIGGER
/// path after it verifies the saved target task committed `TaskState::Blocked`.
#[test]
fn witness_target_parks_without_publishing_armed() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    let body = production_guard::block_after(W, "fn witness_target").expect("witness body");
    // Waiter registration happens inside the poll closure (before Pending).
    assert!(
        body.contains("WAKER.register(cx.waker())"),
        "target must register its waiter before parking"
    );
    // The poll closure is pure register+check+Pending: no machine interaction.
    let closure = body
        .split("poll_fn(|cx|")
        .nth(1)
        .expect("poll closure")
        .split("}))")
        .next()
        .expect("poll closure body");
    assert!(
        !closure.contains("MACHINE"),
        "the target poll must not touch the witness machine (no self-arming)"
    );
    assert!(
        !closure.contains("on_armed"),
        "the target poll must not publish Armed"
    );
}

/// Plan Review finding 2 (final), follow-up instruction 2/3: TRIGGER must gate
/// on the saved target task being `TaskState::Blocked` (reject Running/Ready/
/// missing handle), verify the saved task still covers the current run's target
/// hart, then promote under the machine lock and only then set TRIGGERED and
/// wake. Rejected paths must not wake.
#[test]
fn witness_trigger_gates_blocked_then_promotes() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    let body = production_guard::block_after(W, "op::TRIGGER").expect("TRIGGER branch");
    let handle = body.find("WITNESS_TASK.lock()").expect("saved handle check");
    let blocked = body
        .find("task.state() != axtask::TaskState::Blocked")
        .expect("Blocked scheduler-state gate");
    let promote = body
        .find("machine.trigger(anchor)")
        .or_else(|| body.find("MACHINE.lock().trigger(anchor)"))
        .expect("machine promotion");
    let wake = body.find("WAKER.wake()").expect("wake after promotion");
    assert!(
        handle < blocked,
        "the saved-handle + Blocked gate must precede any state change"
    );
    assert!(
        blocked < promote,
        "promotion must be gated on the committed Blocked state"
    );
    assert!(
        promote < wake,
        "the blocked task may only be woken after the atomic promotion"
    );
    // The machine promotion itself rejects cancelled/terminal/again-promoted
    // runs without waking (the TRIGGERED store sits after the promotion check).
    let triggered = body.find("TRIGGERED.store(true").expect("trigger flag");
    assert!(
        promote < triggered && triggered < wake,
        "TRIGGERED is set only after the promotion succeeds"
    );
}

/// Task 3.4: the QEMU binding must reserve single-flight before spawn, roll back
/// a failed spawn, reset per-run flags on START, and gate trigger/cancel through
/// the machine (remote-only / phase-valid).
#[test]
fn witness_binding_enforces_singleflight_rollback_and_remote_trigger() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    assert!(
        W.contains("reserve(scope, target)") && W.contains("reserve_rollback()"),
        "START must reserve (with a per-attempt scope ownership token) before spawn \
         and roll back a failed spawn"
    );
    assert!(
        !W.contains("snapshot().run_id") && !W.contains(".run_id ="),
        "the forbidden identity-style run_id counter must be gone from the binding"
    );
    assert!(
        W.contains("TRIGGERED.store(false") && W.contains("CANCELLED.store(false"),
        "START must reset per-run flags"
    );
    assert!(
        W.contains("machine.trigger(anchor)") || W.contains("MACHINE.lock().trigger(anchor)"),
        "TRIGGER must be gated through the machine promotion"
    );
    assert!(
        W.contains("MACHINE.lock().request_cancel()") || W.contains("machine.request_cancel()"),
        "CANCEL must be gated through the machine"
    );
    // Reserve happens before the spawn call; rollback is on the spawn-failure edge.
    let body = production_guard::block_after(W, "op::START").expect("START branch");
    let reserve_pos = body.find("reserve(scope, target)").unwrap();
    let spawn_pos = body.find("spawn_with_name_affinity").unwrap();
    let rollback_pos = body.find("reserve_rollback()").unwrap();
    assert!(reserve_pos < spawn_pos, "reserve must precede spawn");
    assert!(spawn_pos < rollback_pos, "spawn failure must roll back the reserve");
    // The target spawn is the SECOND affinity spawn (supervisor first) and its
    // failure still rolls back through the same reserve.
    assert!(
        body.matches("spawn_with_name_affinity").count() >= 2,
        "START must spawn a supervisor before the target"
    );
}

/// Task 3.4: a timer-enabled remote supervisor owns a fixed deadline. START spawns
/// it on a non-target hart (single-flight), and a failed target spawn sets an abort
/// flag so the already-enqueued supervisor expires without acting on a new run.
#[test]
fn witness_binding_spawns_bounded_remote_supervisor() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    assert!(
        W.contains("fn witness_supervisor") && W.contains("WITNESS_DEADLINE_US"),
        "binding must define a bounded supervisor with a fixed deadline"
    );
    assert!(
        W.contains("move || witness_supervisor(") && W.contains("spawn_with_name_affinity"),
        "the supervisor must be spawned through the affinity seam"
    );
    let body = production_guard::block_after(W, "op::START").expect("START branch");
    let sup_pos = body.find("witness_supervisor").unwrap();
    let tgt_pos = body.find("witness_target").unwrap();
    assert!(
        sup_pos < tgt_pos,
        "supervisor must be enqueued before the target so a target spawn failure can be aborted"
    );
    assert!(
        W.contains("SUPERVISOR_ABORT.store(true") && W.contains("SUPERVISOR_ABORT.store(false"),
        "START must reset and target-spawn-failure set the supervisor abort flag"
    );
    // The supervisor must gate cancellation on its OWN run via the pure predicate.
    assert!(
        W.contains("supervisor_should_cancel(&s"),
        "supervisor must consult the pure supervisor_should_cancel predicate"
    );
}

/// Task 3.4: the target must restore its local timer on EVERY function-exit path
/// after disabling it — the `on_armed` failure and `restore_ack` failure edges
/// included — so a target can never exit while its hart timer stays disabled.
/// Only *bare* `return;` exits are checked: the poll-closure's value returns
/// (`return Poll::Ready(..)`) merely hand control back to `block_on`, they do
/// not exit `witness_target`, and the closure is re-entered before any exit.
#[test]
fn witness_target_restores_timer_on_all_return_paths() {
    const W: &str = include_str!("../kernel/src/drivers/net_wake_witness.rs");
    let body = production_guard::block_after(W, "fn witness_target").expect("witness body");
    // The disable happens once, before arm.
    assert_eq!(
        body.matches("set_enable(axhal::time::irq_num(), false)").count(),
        1,
        "timer must be disabled exactly once"
    );
    let disable = body.find("set_enable(axhal::time::irq_num(), false)").unwrap();
    // Every function-level bare `return;` must already have restored the timer on
    // its own path (finding 2: no post-disable path may escape with the timer
    // disabled). Closure value-returns are not function exits and are excluded.
    let mut from = disable;
    while let Some(rel) = body[from..].find("return;") {
        let ret = from + rel;
        let between = &body[disable..ret];
        assert!(
            between.contains("set_enable(axhal::time::irq_num(), true)"),
            "a function-exit path escapes with the timer disabled (window before return;)"
        );
        from = ret + "return;".len();
    }
    // The unified post-park restore serves both the on_armed-failure exit and the
    // main terminal path (single shared restore preceding every bare `return;`),
    // and restore strictly follows disable.
    assert!(
        body.matches("set_enable(axhal::time::irq_num(), true)").count() >= 1,
        "the post-park unified path must restore the timer before any exit"
    );
    let first_restore = body
        .find("set_enable(axhal::time::irq_num(), true)")
        .unwrap();
    assert!(disable < first_restore, "restore must follow disable");
}

/// Task 3.2 replan fix: `init_virtio_net_irq_diag` must record the pinned owner and
/// runner telemetry AFTER a successful affinity spawn, never before — a rejected
/// spawn rolls the lifecycle back but must leave the pinned field unset (no fake
/// role in V5), and it must be retryable.
#[test]
fn init_records_pinned_hart_only_after_successful_spawn() {
    const V: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    let body = production_guard::block_after(V, "fn init_virtio_net_irq_diag")
        .expect("init_virtio_net_irq_diag body");
    // Both record_*_pinned calls must appear strictly after their spawn match's
    // Ok(task) arm, i.e. textually after the corresponding start_*_affinity call.
    for (record, start) in [
        ("record_runner_pinned", "start_stack_runner_affinity"),
        ("record_owner_pinned", "start_rx_task_affinity"),
    ] {
        let rec = body.find(record).unwrap_or_else(|| panic!("no {record}"));
        let start = body.find(start).unwrap_or_else(|| panic!("no {start}"));
        assert!(
            start < rec,
            "{record} must be recorded only after a successful {start}"
        );
    }
}

// ===== Task 4.2: controlled UART copier migration (pure decision/state model) =====

/// Target selection must fail closed for every invalid dynamic input and keep
/// the published-schedulable validation distinct from capacity validation.
#[test]
fn uart_migration_target_selection_rejects_invalid_and_preserves() {
    use uart_migration_logic::{MigrateReject, select_second, AUTO_HART};

    const CAP: usize = 16;
    let sched = [1usize, 3, 5, 9]; // sorted, deduped, all < CAP

    // Auto: first schedulable member different from orig.
    assert_eq!(select_second(1, AUTO_HART, &sched, CAP), Ok(3));
    assert_eq!(select_second(9, AUTO_HART, &sched, CAP), Ok(1));
    // Auto with no alternative fails closed.
    assert_eq!(
        select_second(7, AUTO_HART, &[7], CAP),
        Err(MigrateReject::NoSecondHart)
    );
    // Capacity boundary: at/above capacity rejected before schedulable check.
    // (`usize::MAX` is the AUTO sentinel, i.e. "no explicit target"; a dynamic
    // out-of-capacity input is represented by any other index >= capacity.)
    assert_eq!(
        select_second(1, CAP, &sched, CAP),
        Err(MigrateReject::OutOfCapacity)
    );
    assert_eq!(
        select_second(1, CAP + 1, &sched, CAP),
        Err(MigrateReject::OutOfCapacity)
    );
    // Self target rejected.
    assert_eq!(
        select_second(3, 3, &sched, CAP),
        Err(MigrateReject::SelfTarget)
    );
    // In-capacity but not published schedulable rejected.
    assert_eq!(
        select_second(1, 7, &sched, CAP),
        Err(MigrateReject::NotSchedulable)
    );
    assert_eq!(
        select_second(1, 0, &sched, CAP),
        Err(MigrateReject::NotSchedulable)
    );
    // Legal explicit target accepted.
    assert_eq!(select_second(1, 5, &sched, CAP), Ok(5));
}

/// The migration record state machine: rejects never advance the phase, the
/// observation requires a real poll-count advance in the Widened phase, and
/// restore closes the cycle in order.
#[test]
fn uart_migration_record_state_model() {
    use uart_migration_logic::{MigrationPhase, MigrationRecord};

    let mut rec = MigrationRecord::new();
    assert_eq!(rec.phase(), MigrationPhase::None);
    assert_eq!(rec.rejects(), 0);

    // Rejects preserve phase and every other field.
    rec.reject();
    rec.reject();
    assert_eq!(rec.rejects(), 2);
    assert_eq!(rec.phase(), MigrationPhase::None);
    assert_eq!(rec.from(), usize::MAX);
    assert_eq!(rec.to(), usize::MAX);

    // Observation is impossible before widening.
    assert!(!rec.observe(10));

    rec.begin(1, 3, 100);
    assert_eq!(rec.phase(), MigrationPhase::Widened);
    assert_eq!(rec.from(), 1);
    assert_eq!(rec.to(), 3);
    assert_eq!(rec.requested_polls(), 100);

    // A stale (non-advancing) poll count is not an observation.
    assert!(!rec.observe(100));
    assert_eq!(rec.observed_polls(), 0);
    // A genuine later poll on the second hart commits.
    assert!(rec.observe(101));
    assert_eq!(rec.observed_polls(), 101);

    // Restore only valid from Widened.
    assert!(rec.restore());
    assert_eq!(rec.phase(), MigrationPhase::Restored);
    // Double restore / late observation are illegal and change nothing.
    assert!(!rec.restore());
    assert!(!rec.observe(102));
    assert_eq!(rec.observed_polls(), 101);
}

/// Structural guard: the production control must route target selection and
/// the two-hart mask through the safe wrapper seams (no ad-hoc mask building,
/// no schedulable bypass, wake issued by a task pinned to the second hart).
#[test]
fn uart_migration_control_uses_safe_seams_in_source() {
    const SNAP: &str = include_str!("../kernel/src/drivers/uart_smp_snapshot.rs");
    const INIT: &str = include_str!("../kernel/src/drivers/uart_init.rs");
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");

    let body = production_guard::block_after(SNAP, "fn migrate_inner")
        .expect("migrate_inner must implement the QEMU-only control flow");
    assert!(
        body.contains("uart_migration_logic::select_second"),
        "target selection must go through the pure fail-closed seam"
    );
    assert!(
        body.contains("set_cpumask_checked"),
        "affinity commit must use the schedulable-validating update"
    );
    assert!(
        body.contains("spawn_with_name_affinity"),
        "the wake stimulus must be a task pinned to the second hart"
    );
    assert!(
        body.contains("wake_copier(dir)"),
        "the stimulus must issue the driver's genuine copier wake"
    );
    let wake_helper = production_guard::block_after(SNAP, "fn wake_copier")
        .expect("wake_copier helper must route to the driver waker seam");
    assert!(
        wake_helper.contains("wake_rx_copier") && wake_helper.contains("wake_tx_copier"),
        "the wake helper must use the driver's RX/TX waker seams"
    );
    // Never fall back to a full mask or force immediate migration.
    for forbidden in ["cpu_mask_full", "set_cpumask("] {
        assert!(
            !body.contains(forbidden),
            "migration must not use {forbidden}"
        );
    }

    // Saved handles are the only migration target source.
    assert!(
        INIT.contains("pub(crate) fn copier_task"),
        "uart_init must expose the saved copier handles to the control"
    );

    // The ioctl command is a new, QEMU-only value; TXDBG stays unchanged.
    assert!(CTL.contains("const UART_SMP_MIGRATE: u32 = 0x5553_4d32;"));
    assert!(CTL.contains("cmd == UART_SMP_MIGRATE"));
    assert!(CTL.contains("UART_TXDBG_SNAPSHOT: u32 = 0x5458_4431"));
}

// ===== Task 4.3: controlled network role migration =====

/// V5 迁移字段的打包/解包往返：phase/from/to 位域稳定，AUTO 哨兵与
/// 判别值在 wire 上可复原。
#[test]
fn net_migration_v5_packing_round_trips() {
    use uart_migration_logic::{pack_migration_view, unpack_migration_view, AUTO_HART};

    // 未迁移初始态打包为 0（V5 原有 reserved 语义）。
    let fresh = uart_migration_logic::MigrationView {
        phase: 0,
        from: AUTO_HART,
        to: AUTO_HART,
        requested_polls: 0,
        observed_polls: 0,
        rejects: 0,
    };
    assert_eq!(pack_migration_view(&fresh), 0);

    for (phase, from, to) in [(1u8, 2usize, 5usize), (2, 15, 0), (1, 0, 15)] {
        let v = uart_migration_logic::MigrationView {
            phase,
            from,
            to,
            requested_polls: 10,
            observed_polls: 11,
            rejects: 3,
        };
        let (p, f, t) = unpack_migration_view(pack_migration_view(&v));
        assert_eq!((p, f, t), (phase, from, to));
    }
    // AUTO 哨兵（usize::MAX）经 24 位位域截断不可表示；生产侧只在 phase=None
    // 时携带 AUTO，打包结果须等于 0（fresh 已断言）。
}

// ===== Cycle 001 repair / finding 1: MigrationSlot is data-race-free & coherent =====

/// MigrationSlot 是单写多读的原子 seqlock：读者与 writer 并发时，每次
/// `load` 都必须返回一个完整提交的视图，永不读到把两次不同写入混合在一起的
/// 撕裂元组。
#[test]
fn migration_slot_concurrent_load_is_always_coherent() {
    use std::sync::Arc;
    use std::thread;
    use uart_migration_logic::{MigrationSlot, MigrationView};

const ROUNDS: u64 = 4000;

    let slot = Arc::new(MigrationSlot::new());

    // Builder 使每个 epoch 的所有字段都严格由 `from` 决定；任意两个字段一旦
    // 来自不同 epoch（撕裂）就会违反下面 reader 的一致性断言。
    fn coherent_view(f: usize) -> MigrationView {
        MigrationView {
            phase: (f % 3) as u8,
            from: f,
            to: f + 1,
            requested_polls: (f as u64) * 1000,
            observed_polls: (f as u64) * 1000 + 1,
            rejects: f as u64,
        }
    }

    // 先以满足一致性 oracle 的初始 tuple 打底（epoch 0），再启线程：否则排在首个
    // writer store 之前的 reader 会读到 `MigrationSlot::new()` 的初始
    // from=AUTO_HART/to=AUTO_HART，无法通过 `to == from + 1` 等断言（AUTO 时 from+1
    // 溢出）。
    slot.store(coherent_view(0));

    let writer = {
        let slot = Arc::clone(&slot);
        thread::spawn(move || {
            for round in 0..ROUNDS {
                slot.store(coherent_view((round % 64) as usize));
            }
        })
    };

    let reader = thread::spawn(move || {
        for _ in 0..ROUNDS {
            let v = slot.load();
            // Coherence: every field must derive from the same epoch `from`.
            let e = v.from as u64;
            assert_eq!(v.phase as u64, e % 3, "torn tuple: phase drawn from a different epoch");
            assert_eq!(v.to as u64, e + 1, "torn tuple: to drifted");
            assert_eq!(v.requested_polls, e * 1000, "torn tuple: requested drifted");
            assert_eq!(v.observed_polls, e * 1000 + 1, "torn tuple: observed drifted");
            assert_eq!(v.rejects, e, "torn tuple: rejects drifted");
        }
    });

    writer.join().expect("writer thread panicked");
    reader.join().expect("reader thread panicked");
}

/// 迁移 stress 串行单次通过（用户豁免多轮：各项串行测试且只测试一次通过即
/// 可）。这不重复 `uart_migration_record_state_model`，而是把整个迁移生命周期
/// — 合法 select、begin/observe/restore、无效操作 fail-closed、end-state
/// identity — 作为一次确定性的完整 pass 串行执行，覆盖 UART 与 network 共用的
/// 同一套 seam。
#[test]
fn migration_single_cycle_serial_pass() {
    use uart_migration_logic::{select_second, MigrationPhase, MigrationRecord, AUTO_HART};

    const CAP: usize = 16;
    let sched = [0usize, 3, 7, 9];

    // 1) select: auto 与显式合法目标。
    let orig = 3;
    let second = select_second(orig, AUTO_HART, &sched, CAP).expect("auto second hart");
    assert_eq!(second, 0, "auto must pick the first schedulable member != orig");
    let second = select_second(orig, 9, &sched, CAP).expect("explicit second hart");

    // 2) 完整生命周期（record 记录 from/to/observed/restore）。
    let mut rec = MigrationRecord::new();
    let poll_before: u64 = 500;
    rec.begin(orig, second, poll_before);
    assert_eq!(rec.phase(), MigrationPhase::Widened);
    assert!(!rec.observe(poll_before), "stale poll-count must not be accepted");
    assert!(rec.observe(poll_before + 1), "genuine poll advance must commit");
    assert!(rec.restore(), "restore only valid from Widened");

    // 3) 无效操作 fail-closed：restore/observe 之后不再改变任何字段。
    assert!(!rec.restore(), "double restore must be rejected");
    assert!(!rec.observe(poll_before + 2), "late observation must be rejected");
    assert_eq!(rec.phase(), MigrationPhase::Restored);
    assert_eq!((rec.from(), rec.to()), (orig, second), "role identity preserved");
    assert_eq!(rec.observed_polls(), poll_before + 1);
}

/// Structural guard: cycle-001 repair must delegate the mask's `Ord`/`Hash` to
/// the private registry inner value (never a native byte-slice comparison).
#[test]
fn mask_ord_hash_delegate_to_inner_in_source() {
    const MASK: &str = include_str!("../crates/axtask/src/cpumask.rs");
    assert!(
        MASK.contains("self.inner.cmp(&other.inner)"),
        "Ord must delegate to the registry numeric backing store"
    );
    assert!(
        MASK.contains("self.inner.hash(state)"),
        "Hash must delegate to the registry backing store"
    );
    assert!(
        !MASK.contains("self.as_bytes().cmp"),
        "byte-slice ordering would mis-order across the byte boundary"
    );
    assert!(
        !MASK.contains("self.as_bytes().hash"),
        "hashing must not go through a native byte slice"
    );
}

/// Structural guard: cycle-002 repair must replace the incomplete custom
/// seqlock with a dedicated short-lived Acquire/Release view lock that is
/// RAII-released. The sequence counter and its Release fences must be gone: a
/// Release fence after the relaxed odd marker cannot keep later field stores
/// below it, and the second Acquire sequence load cannot keep earlier field
/// loads above it, so readers could still accept a torn tuple on RISC-V SMP.
#[test]
fn migration_slot_has_no_unsafe_cell_sync() {
    const LOGIC: &str = include_str!("../kernel/src/drivers/uart_migration_logic.rs");
    for forbidden in ["UnsafeCell", "unsafe impl Sync for MigrationSlot"] {
        assert!(
            !LOGIC.contains(forbidden),
            "MigrationSlot must not use raw {forbidden}"
        );
    }
    // The incomplete seqlock publication must not remain in any form.
    for forbidden in ["self.seq", "fence(Ordering::Release)", "fetch_add"] {
        assert!(
            !LOGIC.contains(forbidden),
            "MigrationSlot must not keep the incomplete seqlock ({forbidden})"
        );
    }
    assert!(
        LOGIC.contains("struct MigrationAtomicView"),
        "MigrationSlot must store each field as an atomic"
    );
    // A distinct short-lived view lock must serialize every six-field copy.
    assert!(
        LOGIC.contains("struct ViewLock"),
        "MigrationSlot must own a dedicated short-lived view lock"
    );
    assert!(
        LOGIC.contains("compare_exchange(false, true, Ordering::Acquire"),
        "the view lock must be acquired with an Acquire compare-exchange"
    );
    assert!(
        LOGIC.matches("self.view_lock.acquire()").count() >= 2,
        "both load() and store() must route their six-field copy through the view lock"
    );
    assert!(
        LOGIC.contains("impl Drop for ViewGuard"),
        "the view lock must be released through an RAII guard"
    );
    let guard = production_guard::block_after(LOGIC, "impl Drop for ViewGuard")
        .expect("ViewGuard drop impl must exist");
    assert!(
        guard.contains("store(false, Ordering::Release)"),
        "the RAII guard must release the view lock with a Release store"
    );

    // The view lock must stay independent of the long-lived single-flight flag:
    // neither load() nor store() may touch `in_progress`.
    for method in ["pub fn load", "pub fn store"] {
        let body = production_guard::block_after(LOGIC, method)
            .unwrap_or_else(|| panic!("{method} body not found"));
        assert!(
            !body.contains("in_progress"),
            "{method} must not reuse the migration single-flight flag"
        );
    }
}

/// Structural guard: the network migration control must route target
/// selection, the affinity commit and the wake through the same safe seams as
/// the UART control, and the V5 assembler must publish the packed migration
/// state (never leave the reserved fields hard-zero).
#[test]
fn net_migration_control_uses_safe_seams_in_source() {
    const PLACE: &str = include_str!("../kernel/src/drivers/net_placement.rs");
    const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
    const AXNET_STACK: &str = include_str!("../crates/axnet/src/stack_runner.rs");
    const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");

    let body = production_guard::block_after(PLACE, "fn migrate_role_inner")
        .expect("migrate_role_inner must implement the QEMU-only control flow");
    assert!(
        body.contains("select_second"),
        "target selection must go through the pure fail-closed seam"
    );
    assert!(
        body.contains("set_cpumask_checked"),
        "affinity commit must use the schedulable-validating update"
    );
    assert!(
        body.contains("spawn_with_name_affinity"),
        "the wake stimulus must be a task pinned to the second hart"
    );
    assert!(
        body.contains("wake_role(role)"),
        "the stimulus must issue the role's genuine wake"
    );
    for forbidden in ["cpu_mask_full", "set_cpumask("] {
        assert!(
            !body.contains(forbidden),
            "migration must not use {forbidden}"
        );
    }

    // The role wake seam must use axnet's own nudge primitives.
    let wake_helper = production_guard::block_after(PLACE, "fn wake_role")
        .expect("wake_role must route to the axnet nudge seams");
    assert!(wake_helper.contains("software_nudge"));
    assert!(wake_helper.contains("runner_software_nudge"));
    assert!(
        AXNET_STACK.contains("pub fn runner_software_nudge()"),
        "axnet must expose the runner nudge publicly"
    );

    // V5 assembler publishes the packed migration views.
    assert!(IRQ.contains("pack_migration_view"));
    assert!(IRQ.contains("migration_owner_state,"));
    assert!(IRQ.contains("migration_runner_state,"));

    // The ioctl command is a new, QEMU-only value.
    assert!(CTL.contains("const NET_IRQ_MIGRATE: u32 = 0x4e49_4436;"));
    assert!(CTL.contains("cmd == NET_IRQ_MIGRATE"));
}

/// Cycle 002 (6.2-R2): the second-hart migration stimulus must be
/// state-driven — it may call the role's genuine wake only for a saved target
/// observed in `TaskState::Blocked`.  A blind wake against Running/Ready only
/// pre-sets `AxWaker.woke` and suppresses the natural park without any
/// `Blocked -> Ready`, which is the runtime owner-migration timeout this
/// repair closes.  Termination must cover second-hart observation, singleton
/// rollback and the bounded iteration count.
#[test]
fn net_migration_stimulus_is_blocked_gated_in_source() {
    const PLACE: &str = include_str!("../kernel/src/drivers/net_placement.rs");
    let body = production_guard::block_after(PLACE, "fn migrate_role_inner")
        .expect("migrate_role_inner must implement the QEMU-only control flow");
    let stimulus = production_guard::block_after(body, "move ||")
        .expect("the pinned second-hart stimulus closure must exist");

    // The stimulus must gate on the saved target's public Blocked state ...
    let gate = stimulus
        .find("task.state() == axtask::TaskState::Blocked")
        .expect("stimulus must verify TaskState::Blocked before waking");
    // ... and its first (and only) wake must sit inside that gated branch:
    // Running/Ready is never nudged.
    let wake = stimulus
        .find("wake_role(role)")
        .expect("stimulus must issue the role's genuine wake");
    assert!(
        gate < wake,
        "the wake must follow the Blocked-state gate (no ungated wake path)"
    );
    assert!(
        !stimulus[gate..wake].contains('}'),
        "the wake must stay inside the Blocked-gated branch"
    );

    // Bounded, state-driven termination: observation, rollback, iteration cap.
    assert!(
        stimulus.contains("observed_polls > v.requested_polls"),
        "stimulus must exit once the control observes the second-hart poll"
    );
    assert!(
        stimulus.contains("cpumask().get(second)"),
        "stimulus must stop when the widened mask is rolled back"
    );
    assert!(
        stimulus.contains("for _ in 0..64"),
        "stimulation stays bounded; no enlarged blind wake count"
    );
    // The control-side timeout diagnostic must distinguish the failing stage.
    assert!(
        body.contains("parked="),
        "the QEMU-only timeout print must record whether the target parked"
    );
}

// ===== Task 4.4: cross-hart ordering audit — concurrent IER RMW witness =====

use std::sync::Mutex as StdMutex;

/// QEMU `ArceOsUartPort::update_ier` 的精确模型：同一个串行化锁同时保护
/// `ier_cache` 的读-改-写与 MMIO `set_ier` 写出。生产不变量是“cache 与
/// MMIO 在任何交错后收敛到同一合并位集”。
#[derive(Default)]
struct LockedIerModel {
    inner: StdMutex<IerState>,
}

#[derive(Default)]
struct IerState {
    /// 模拟 `ier_cache: AtomicU8`（在锁内访问）。
    cache: u8,
    /// 模拟 16550 的 IER 寄存器（MMIO 写出目标）。
    mmio: u8,
}

impl LockedIerModel {
    fn update_ier(&self, set: u8, clear: u8) {
        let mut s = self.inner.lock().unwrap();
        // 生产顺序：lock -> cache load(RMW) -> cache store -> set_ier。
        let mut val = s.cache;
        val |= set;
        val &= !clear;
        s.cache = val;
        s.mmio = val;
    }

    fn state(&self) -> (u8, u8) {
        let s = self.inner.lock().unwrap();
        (s.cache, s.mmio)
    }
}

/// RED 孪生：无锁 RMW（两个独立“原子”上非原子地读-改-写）证明见证有
/// 判别力——同一交错下它确实会丢更新。
struct UnlockedIerModel {
    cache: core::sync::atomic::AtomicU8,
    mmio: core::sync::atomic::AtomicU8,
}

impl UnlockedIerModel {
    fn update_ier(&self, set: u8, clear: u8) {
        let mut val = self.cache.load(core::sync::atomic::Ordering::Relaxed);
        val |= set;
        val &= !clear;
        self.cache.store(val, core::sync::atomic::Ordering::Relaxed);
        // 模拟调度点：另一线程可在此处改写 cache，使本次 MMIO 写出过期值。
        std::thread::yield_now();
        self.mmio.store(val, core::sync::atomic::Ordering::Relaxed);
    }
}

#[test]
fn uart_ier_cache_rmw_is_serialized_with_mmio_write() {
    const ROUNDS: usize = 2000;
    // 两个发布者做冲突的位更新：A 开 RX+THRE、清 THRE；B 只开 THRE。
    // 任何交错下，锁内 RMW 使 cache 与 MMIO 收敛到同一最终位集。
    let model = std::sync::Arc::new(LockedIerModel::default());
    let m1 = model.clone();
    let m2 = model.clone();
    let a = std::thread::spawn(move || {
        for _ in 0..ROUNDS {
            m1.update_ier(0b11, 0b010); // 开 RX|THRE，清 THRE → 0b001
        }
    });
    let b = std::thread::spawn(move || {
        for _ in 0..ROUNDS {
            m2.update_ier(0b010, 0); // 开 THRE
        }
    });
    a.join().unwrap();
    b.join().unwrap();

    let (cache, mmio) = model.state();
    assert_eq!(
        cache & !0b011,
        0,
        "no IER bit outside the merged bitset may be set: {cache:#x}"
    );
    assert_eq!(
        cache, mmio,
        "cache and MMIO must converge to the same serialized value: cache={cache:#x} mmio={mmio:#x}"
    );
    // RX 位只能由 A 设置且从未被清除：必须恒为 1。
    assert_eq!(cache & 0b001, 0b001, "RX bit must survive every interleaving");
}

#[test]
fn uart_ier_unlocked_model_loses_updates_proving_witness_power() {
    // 无锁孪生在强制交错下确定性丢更新：A 完成 cache RMW 后让 B 完整跑完
    // 一次更新，A 再用自己缓存的旧值写 MMIO —— cache 与 MMIO 发散。这证明
    // 上面的锁化见证有判别力（不是恒真）。
    use std::sync::{Barrier, Condvar, Mutex};

    struct Handoff {
        ready: Mutex<bool>,
        condvar: Condvar,
    }

    let model = std::sync::Arc::new(UnlockedIerModel {
        cache: core::sync::atomic::AtomicU8::new(0),
        mmio: core::sync::atomic::AtomicU8::new(0),
    });
    let a_done = std::sync::Arc::new(StdMutex::new(false));
    let handoff = std::sync::Arc::new(Handoff {
        ready: Mutex::new(false),
        condvar: Condvar::new(),
    });
    let start = std::sync::Arc::new(Barrier::new(2));

    let m1 = model.clone();
    let a_done1 = a_done.clone();
    let handoff1 = handoff.clone();
    let start1 = start.clone();
    let a = std::thread::spawn(move || {
        start1.wait();
        // A 的 RMW（只到 cache store，推迟 MMIO 写出）。
        let mut val = m1.cache.load(core::sync::atomic::Ordering::Relaxed);
        val |= 0b11;
        val &= !0b010;
        m1.cache.store(val, core::sync::atomic::Ordering::Relaxed);
        *a_done1.lock().unwrap() = true;
        // 通知 B 完整跑一次更新，然后等 B 完成。
        *handoff1.ready.lock().unwrap() = true;
        handoff1.condvar.notify_one();
        let mut done = handoff1.ready.lock().unwrap();
        while *done {
            done = handoff1.condvar.wait(done).unwrap();
        }
        // A 用缓存的旧值写 MMIO（生产 BUG：RMW 与写出之间无串行化）。
        m1.mmio
            .store(val, core::sync::atomic::Ordering::Relaxed);
    });

    let m2 = model.clone();
    let a_done2 = a_done.clone();
    let handoff2 = handoff.clone();
    let start2 = start.clone();
    let b = std::thread::spawn(move || {
        start2.wait();
        // 等 A 完成 cache RMW。
        while !*a_done2.lock().unwrap() {
            std::thread::yield_now();
        }
        // B 完整跑一次更新（cache 与 MMIO 都写）。
        let mut val = m2.cache.load(core::sync::atomic::Ordering::Relaxed);
        val |= 0b010;
        m2.cache.store(val, core::sync::atomic::Ordering::Relaxed);
        m2.mmio
            .store(val, core::sync::atomic::Ordering::Relaxed);
        // 通知 A 继续写 MMIO。
        let mut ready = handoff2.ready.lock().unwrap();
        *ready = false;
        handoff2.condvar.notify_one();
    });

    a.join().unwrap();
    b.join().unwrap();

    let cache = model.cache.load(core::sync::atomic::Ordering::Relaxed);
    let mmio = model.mmio.load(core::sync::atomic::Ordering::Relaxed);
    assert_ne!(
        cache, mmio,
        "the unlocked model must diverge under the forced interleaving (witness power)"
    );
}

#[test]
fn qemu_uart_update_ier_holds_one_guard_across_cache_rmw_and_mmio() {
    // Structural guard: the production QEMU port must perform the cache RMW
    // and the MMIO write under a single `SpinNoIrq` guard (Task 4.4 / D8
    // ordering audit). The D1 local-IRQ-only adapter is explicitly out of the
    // SMP claim and is not guarded here.
    const INIT: &str = include_str!("../kernel/src/drivers/uart_init.rs");
    let body = production_guard::block_after(INIT, "fn update_ier")
        .expect("ArceOsUartPort::update_ier body");
    assert!(
        body.contains("self.uart.lock()"),
        "update_ier must hold the UART lock"
    );
    assert!(
        body.contains("ier_cache"),
        "update_ier must update the cached bitset"
    );
    assert!(
        body.contains("set_ier"),
        "update_ier must write the MMIO register"
    );
    // 锁必须在 RMW 之前取得、在写出之后释放：lock 出现在两者之前。
    let lock_at = body.find("self.uart.lock()").unwrap();
    let first_touched = body
        .find("ier_cache")
        .unwrap()
        .min(body.find("set_ier").unwrap());
    assert!(
        lock_at < first_touched,
        "the guard must be acquired before the cache RMW and the MMIO write"
    );
}
