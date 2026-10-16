//! K3 multi-hart startup decision core (MS14 Iteration 000, task T1).
//!
//! Pure `core` module: the `cpu_boot` acceptance checks consumed by
//! `power.rs` and the per-stage later-init step tables consumed by
//! `init.rs`. The riscv64 layer performs only the SBI/CSR effects these
//! decisions select, and every refusal branch there parks (`-> !`), so a
//! refused start structurally cannot return into the runtime's
//! `ENTERED_CPUS` wait (`hart_start_refusal_stops_before_runtime_wait`).
//! Host harnesses include this module directly and run the real decision
//! logic — never a copy of it.

/// One refused `cpu_boot` request. The production caller reports the
/// refusal and parks: returning would leave
/// `axruntime::start_secondary_cpus` spinning on `ENTERED_CPUS` forever
/// with no diagnosable line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmpBootRefusal {
    /// Target outside the configured AP range (the boot hart itself is
    /// never a `cpu_boot` target).
    TargetOutOfRange { cpu_id: usize, max_cpu_num: usize },
    /// Initial stack top missing or not 16-byte aligned (RISC-V ABI).
    BadStackTop { stack_top_paddr: usize },
    /// The SBI HSM extension is unavailable on this firmware.
    HsmUnavailable,
}

/// Accepts or refuses one `cpu_boot` request BEFORE any SBI call: the
/// target must name a secondary AP and the handed stack top must be a
/// usable 16-byte-aligned physical address.
pub fn check_boot_request(
    cpu_id: usize,
    max_cpu_num: usize,
    stack_top_paddr: usize,
) -> Result<(), SmpBootRefusal> {
    if cpu_id == 0 || cpu_id >= max_cpu_num {
        return Err(SmpBootRefusal::TargetOutOfRange { cpu_id, max_cpu_num });
    }
    if stack_top_paddr == 0 || stack_top_paddr % 16 != 0 {
        return Err(SmpBootRefusal::BadStackTop { stack_top_paddr });
    }
    Ok(())
}

/// Applies the `sbi_rt::probe_extension(Hsm).is_unavailable()` outcome: an
/// unavailable HSM refuses the start (the caller parks).
pub fn check_hsm(hsm_unavailable: bool) -> Result<(), SmpBootRefusal> {
    if hsm_unavailable {
        Err(SmpBootRefusal::HsmUnavailable)
    } else {
        Ok(())
    }
}

/// Platform later-init steps. The global APLIC domain init and the boot
/// hart record run exactly once on the boot hart: a secondary replaying
/// them would clear the fixed routes and retarget registered sources
/// (`secondary_init_never_runs_global_init`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaterStep {
    /// `irq::record_boot_hart` (boot only).
    RecordBootHart,
    /// `irq::init_global` (boot only).
    GlobalAplic,
    /// `irq::init_percpu` (every hart: its own IMSIC file).
    LocalImsicFile,
    /// `time::init_percpu` (every hart: first-tick arm).
    LocalTime,
    /// Local `STIE`/`SSIE` enable (every hart: caller-local CSRs).
    LocalTimerSoftEnable,
}

/// Boot-hart later-init sequence (the existing single-hart order,
/// unchanged): first-tick arm, boot-hart record, global domain, own file.
pub const PRIMARY_LATER_STEPS: [LaterStep; 4] = [
    LaterStep::LocalTime,
    LaterStep::RecordBootHart,
    LaterStep::GlobalAplic,
    LaterStep::LocalImsicFile,
];

/// Secondary later-init sequence: local steps only, in dependency order —
/// first-tick arm, own IMSIC file, then the caller-local timer/soft CSRs.
/// The global slots are already owned by the primary (the scheduler
/// registers S_SOFT before the secondaries start; the runtime timer
/// registration lands before any hart opens `sstatus.SIE`).
pub const SECONDARY_LATER_STEPS: [LaterStep; 3] = [
    LaterStep::LocalTime,
    LaterStep::LocalImsicFile,
    LaterStep::LocalTimerSoftEnable,
];

/// Secondary later-init contract: local steps only — no boot-only step.
pub fn steps_are_local_only(steps: &[LaterStep]) -> bool {
    steps
        .iter()
        .all(|s| !matches!(s, LaterStep::RecordBootHart | LaterStep::GlobalAplic))
}
