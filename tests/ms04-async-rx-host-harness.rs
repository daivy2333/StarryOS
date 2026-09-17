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

use core::cell::Cell;

use std::sync::Arc;
use std::thread;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering as StdOrdering};

use critical_section_policy::{
    IrqOps, MAX_CPU_NUM, checked_increment_depth, acquire, release,
};

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
    let ops = FakeIrqOps::with_cpu(1,true);
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
    let ops = FakeIrqOps::with_cpu(2,false);
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
    let ops = FakeIrqOps::with_cpu(3,true);
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
    let ops = FakeIrqOps::with_cpu(4,false);
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
    let ops = FakeIrqOps::with_cpu(5,true);
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
    assert_eq!(failed.load(StdOrdering::Relaxed), 0, "two harts in critical section");
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
    assert_eq!(inside.load(StdOrdering::SeqCst), 1, "cpu 0 released exactly once");
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

    // kernel `smp` propagates `axtask/ipi`.
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
