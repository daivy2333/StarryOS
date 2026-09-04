//! MS07 Task 4.1 host-side contract witnesses.
//!
//! These tests deliberately inspect the public seams that the QEMU probe and
//! validator depend on.  They do not boot QEMU or execute a guest.

const IRQ_LOGIC: &str = include_str!("../kernel/src/drivers/virtio_net_irq_logic.rs");
const IRQ: &str = include_str!("../kernel/src/drivers/virtio_net_irq.rs");
const CTL: &str = include_str!("../kernel/src/syscall/fs/ctl.rs");
const AXNET: &str = include_str!("../crates/axnet/src/async_rx.rs");
const USER_TASK: &str = include_str!("../kernel/src/task/user.rs");
const POLL_SYS: &str = include_str!("../kernel/src/syscall/io_mpx/poll.rs");
const ACCESS: &str = include_str!("../kernel/src/mm/access.rs");

#[test]
fn v4_is_an_independent_append_only_wire_and_control_is_qemu_only() {
    assert!(IRQ_LOGIC.contains("pub struct IrqSnapshotV4"));
    assert!(!IRQ_LOGIC.contains("type IrqSnapshotV4"));
    assert!(IRQ.contains("pub fn irq_snapshot_v4()"));
    assert!(
        IRQ.contains("v3: irq_snapshot_v3()"),
        "V4 must copy V3 as its byte-for-byte prefix"
    );
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
fn unrecoverable_user_fault_log_is_instruction_addressable() {
    // T4.2-P5 / R8: an unrecoverable user page fault must record a distinct
    // faulting PC (from the saved user context) alongside the fault VA, plus
    // SP and RA, so a runtime fault can be aligned to the exact ELF.  The
    // check is scoped to the PageFault branch (the "Enter user space" line
    // already prints an unrelated ip=/sp= pair and must not satisfy it).
    let branch = USER_TASK
        .find("ReturnReason::PageFault")
        .expect("PageFault branch present in user.rs");
    let region = &USER_TASK[branch..];
    let line = region
        .find("segmentation fault")
        .expect("unrecoverable fault log line present");

    // The fault log must separate PC from the fault VA and publish SP/RA.
    for label in ["pc=", "va=", "sp=", "ra="] {
        assert!(
            region[line..].contains(label),
            "unrecoverable fault log must emit label `{label}`"
        );
    }
    // PC and VA come from the same saved uctx but are distinct identities.
    assert!(
        region.contains("user_fault_pc_sp_ra(&uctx)"),
        "fault PC/SP/RA must be read from saved user context"
    );
    assert!(
        USER_TASK.contains("fn user_fault_pc_sp_ra"),
        "a cfg-dispatched RA accessor helper must exist"
    );
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

#[test]
fn zero_nfds_poll_ignores_fds_and_preserves_userptr_boundary() {
    // T4.2-P8 / R8: a timeout-only `poll(NULL, 0, t)` becomes musl's `ppoll`
    // syscall and must ignore the `fds` pointer; the zero-length path must not
    // leak into the generic access.rs region validator.  The normalization
    // lives in the poll syscall layer only.
    assert!(
        POLL_SYS.contains("fn user_poll_fds"),
        "a shared zero-fd normalization helper must exist in poll.rs"
    );
    assert!(
        POLL_SYS.contains("if nfds == 0"),
        "poll.rs must branch on nfds == 0 before touching the user pointer"
    );
    assert!(
        POLL_SYS.contains("Ok(&mut [])"),
        "the zero-fd branch must return a safe empty slice"
    );
    assert!(
        POLL_SYS.contains("user_poll_fds(fds, nfds as usize)"),
        "x86_64 sys_poll must route through the shared helper"
    );
    assert!(
        POLL_SYS.contains("user_poll_fds(fds, nfds)?"),
        "sys_ppoll must route through the shared helper"
    );
    // access.rs must stay a pure region validator: no zero-length shortcut.
    assert!(
        ACCESS.contains("fn get_as_mut_slice"),
        "access.rs get_as_mut_slice must remain"
    );
    assert!(
        ACCESS.contains("Layout::array::<T>(len).unwrap()"),
        "access.rs must not drop the positive-nfds size validation"
    );
}
