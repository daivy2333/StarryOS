//! K3 multi-hart startup decision core host harness (MS14 Iteration 000, T1).
//!
//! Uses `#[path]` to include the real production module
//! `crates/axplat-riscv64-k3/src/smp_boot.rs` without compiling the platform
//! crate. The module is pure `core`, so the tests drive the actual
//! `cpu_boot` acceptance decisions and the secondary later-init step table
//! — never a copy of them. The riscv64 layer performs only the SBI/CSR
//! effects these decisions select, and every refusal branch there parks
//! (`-> !`), so a refused start structurally cannot return into
//! `axruntime::start_secondary_cpus`'s `ENTERED_CPUS` wait. Runs via
//! `rustc --edition=2024 --test` → /tmp binary (same convention as the
//! MS09/MS10/MS17 probe harnesses).
#![allow(dead_code)]

#[path = "../crates/axplat-riscv64-k3/src/smp_boot.rs"]
mod smp_boot;

use smp_boot::{
    check_boot_request, check_hsm, steps_are_local_only, LaterStep, SmpBootRefusal,
    PRIMARY_LATER_STEPS, SECONDARY_LATER_STEPS,
};

/// 16 KiB aligned fake stack top (the runtime hands a `.bss.stack` end).
const ALIGNED_STACK: usize = 0x1_8001_0000;

#[test]
fn hart_start_refusal_stops_before_runtime_wait() {
    // Target classification: hart 0 (the boot hart itself) and any hart at
    // or beyond the configured maximum are refused BEFORE any SBI call.
    // The production cpu_boot parks on every refusal, so a refused start
    // ends the boot with a diagnosable line instead of hanging the
    // runtime's ENTERED_CPUS wait.
    assert_eq!(
        check_boot_request(0, 16, ALIGNED_STACK),
        Err(SmpBootRefusal::TargetOutOfRange { cpu_id: 0, max_cpu_num: 16 })
    );
    assert_eq!(
        check_boot_request(16, 16, ALIGNED_STACK),
        Err(SmpBootRefusal::TargetOutOfRange { cpu_id: 16, max_cpu_num: 16 })
    );
    assert_eq!(
        check_boot_request(usize::MAX, 16, ALIGNED_STACK),
        Err(SmpBootRefusal::TargetOutOfRange { cpu_id: usize::MAX, max_cpu_num: 16 })
    );
    // HSM refusal: an unavailable SBI HSM extension refuses the start (the
    // board firmware provides it; the fake covers the probe outcome).
    assert_eq!(check_hsm(true), Err(SmpBootRefusal::HsmUnavailable));
    assert_eq!(check_hsm(false), Ok(()));
}

#[test]
fn secondary_target_and_stack_are_checked() {
    // A legal AP request: secondary hart, aligned non-zero stack top.
    assert_eq!(check_boot_request(8, 16, ALIGNED_STACK), Ok(()));
    assert_eq!(check_boot_request(15, 16, ALIGNED_STACK), Ok(()));
    // Stack checks: missing and misaligned tops are refused (RISC-V ABI
    // requires 16-byte SP alignment; the secondary never allocates its own).
    assert_eq!(
        check_boot_request(8, 16, 0),
        Err(SmpBootRefusal::BadStackTop { stack_top_paddr: 0 })
    );
    assert_eq!(
        check_boot_request(8, 16, ALIGNED_STACK + 8),
        Err(SmpBootRefusal::BadStackTop { stack_top_paddr: ALIGNED_STACK + 8 })
    );
    // Order: a bad target refuses with the target error even when the
    // stack is also bad (target check runs first, nothing else proceeds).
    assert!(matches!(
        check_boot_request(0, 16, 0),
        Err(SmpBootRefusal::TargetOutOfRange { .. })
    ));
}

#[test]
fn secondary_init_never_runs_global_init() {
    // The secondary later-init table is local-only: no boot-hart record and
    // no global APLIC domain init — replaying either on a secondary would
    // clear the fixed routes and retarget registered sources.
    assert!(
        steps_are_local_only(&SECONDARY_LATER_STEPS),
        "secondary later-init table contains a boot-only step: {SECONDARY_LATER_STEPS:?}"
    );
    // The table carries exactly the three local steps, in dependency order:
    // first tick arm, own IMSIC file, then the caller-local CSRs.
    assert_eq!(
        SECONDARY_LATER_STEPS,
        [
            LaterStep::LocalTime,
            LaterStep::LocalImsicFile,
            LaterStep::LocalTimerSoftEnable,
        ]
    );
    // The predicate has teeth: the boot-hart table (containing the global
    // steps) is NOT local-only.
    assert!(
        !steps_are_local_only(&PRIMARY_LATER_STEPS),
        "primary table must contain boot-only steps for the predicate to bite"
    );
}
