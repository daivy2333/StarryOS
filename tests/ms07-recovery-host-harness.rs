//! MS07 Task 4.1 host-side contract witnesses.
//!
//! These tests deliberately inspect the public seams that the QEMU probe and
//! validator depend on.  They do not boot QEMU or execute a guest.

const IRQ_LOGIC: &str = include_str!("../kernel/src/drivers/virtio_net_irq_logic.rs");
const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
const AXNET: &str = include_str!("../crates/axnet/src/async_rx.rs");

#[test]
fn v4_is_an_independent_append_only_wire_and_control_is_qemu_only() {
    assert!(IRQ_LOGIC.contains("pub struct IrqSnapshotV4"));
    assert!(!IRQ_LOGIC.contains("type IrqSnapshotV4"));
    assert!(IRQ.contains("pub fn irq_snapshot_v4()"));
    assert!(CTL.contains("const NET_IRQ_SNAPSHOT_V4"));
    assert!(CTL.contains("const NET_RECOVERY_RESET_REQUEST"));
    assert!(CTL.contains("#[cfg(feature = \"qemu\")]\n    if cmd == NET_IRQ_SNAPSHOT_V4"));
    assert!(CTL.contains("#[cfg(feature = \"qemu\")]\n    if cmd == NET_RECOVERY_RESET_REQUEST"));
}

#[test]
fn reset_request_is_consumed_only_by_the_resident_owner() {
    assert!(AXNET.contains("pub(crate) fn recovery_reset_request_shared"));
    assert!(AXNET.contains("RecoveryRequestState"));
    assert!(AXNET.contains("claim_recovery_reset_request"));
    assert!(AXNET.contains("clear_for_recovery"));
    assert!(AXNET.contains("self.enter_recovery(&DevError::Io, recover_stage::EXPLICIT_REQUEST)"));
    assert!(!CTL.contains("recovery_begin_target"));
    assert!(!CTL.contains("poll_recovery_step"));
}

#[test]
fn v4_separates_current_observation_from_historical_fault() {
    for field in [
        "current_valid",
        "current_queue_epoch",
        "current_socket_epoch",
        "fault_valid",
        "fault_queue_epoch",
        "fault_owner_available",
    ] {
        assert!(AXNET.contains(field), "missing axnet V4 field {field}");
        assert!(IRQ_LOGIC.contains(field), "missing kernel V4 field {field}");
        assert!(IRQ.contains(field), "missing V4 wire mapping {field}");
    }
    assert!(!AXNET.contains("if fault.queue_epoch == 0"));
}
