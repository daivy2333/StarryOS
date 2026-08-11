//! MS04 host harness: critical-section restore policy bound to production.
//!
//! Compiled and executed by `make host-test`:
//!   rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs \
//!     -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test
//!
//! The harness includes the *same* `critical_section_policy.rs` file that the
//! kernel compiles as `crate::drivers::critical_section_policy`. The kernel's
//! `critical_impl` delegates its `critical_section::Impl` acquire/release to
//! the seam's `acquire`/`release` through an `axhal` backend; here a fake
//! backend records the simulated IRQ state and call counts. Both paths execute
//! the same two functions, so these tests witness the exact production restore
//! decision logic.
//!
//! RED state: against the pre-iteration seam (dead `IrqRestorePolicy` model,
//! no `IrqOps`/`acquire`/`release` API) this harness fails to compile.
//! GREEN state: all six unique scenarios pass.

#[path = "../kernel/src/drivers/critical_section_policy.rs"]
mod critical_section_policy;

use core::cell::Cell;

use critical_section_policy::{IrqOps, acquire, release};

/// Fake IRQ backend: simulates the global IRQ enable state and records
/// disable/enable call counts.
#[derive(Default)]
struct FakeIrqOps {
    irqs_enabled: Cell<bool>,
    disable_calls: Cell<u32>,
    enable_calls: Cell<u32>,
}

impl FakeIrqOps {
    fn new(irqs_enabled: bool) -> Self {
        Self {
            irqs_enabled: Cell::new(irqs_enabled),
            ..Self::default()
        }
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
}

#[test]
fn enabled_acquire_disables_and_release_reenables_once() {
    let ops = FakeIrqOps::new(true);
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
    let ops = FakeIrqOps::new(false);
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
    let ops = FakeIrqOps::new(true);
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
    let ops = FakeIrqOps::new(false);
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
    let ops = FakeIrqOps::new(true);
    let was_enabled = acquire(&ops);
    release(&ops, !was_enabled);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.enable_calls(), 0);
}

#[test]
fn acquire_always_disables_irqs() {
    let ops = FakeIrqOps::new(true);
    acquire(&ops);
    acquire(&ops);
    assert!(!ops.irqs_enabled.get());
    assert_eq!(ops.disable_calls(), 2);
    assert_eq!(ops.enable_calls(), 0);
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
        let handler = block_after(VIRTIO_NET_IRQ_SOURCE, "fn net_irq_handler")
            .ok_or("net_irq_handler body not found")?;

        // record -> ACK -> publish order: each step must appear and the ACK
        // write must precede the publish call.
        let record_pos = handler
            .find("TELEMETRY.record")
            .ok_or("handler does not record raw status")?;
        let ack_pos = handler
            .find("write_volatile")
            .ok_or("handler does not write the device ACK register")?;
        let publish_pos = handler
            .find("publish_rx_event")
            .ok_or("handler does not publish used-ring RX events")?;
        let restore_pos = handler
            .find("restore_violation")
            .ok_or("handler does not observe restore violations")?;

        if !(record_pos < ack_pos && ack_pos < publish_pos && publish_pos < restore_pos) {
            return Err("handler order must be record -> ACK -> publish -> restore check".into());
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
        let init = block_after(VIRTIO_NET_IRQ_SOURCE, "fn init_virtio_net_irq_diag")
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
        let fail_return = init.find("return;").ok_or("init lacks a failure return")?;
        if !(fail_return < start_pos) {
            return Err("registration failure must return before start_rx_task".into());
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
