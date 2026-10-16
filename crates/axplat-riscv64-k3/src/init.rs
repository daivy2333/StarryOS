use axplat::init::InitIf;

struct InitIfImpl;

#[impl_plat_interface]
impl InitIf for InitIfImpl {
    fn init_early(_cpu_id: usize, _mbi: usize) {
        axcpu::init::init_trap();
        crate::console::init_early();
        crate::time::init_early();
    }

    /// Secondary early init (MS14 Iteration 000, task T1): trap vector
    /// only. The console/time early stages are boot-hart one-shot setup.
    #[cfg(feature = "smp")]
    fn init_early_secondary(_cpu_id: usize) {
        axcpu::init::init_trap();
    }

    fn init_later(_cpu_id: usize, _arg: usize) {
        // MS14 Iteration 000: the boot-hart sequence is the primary step
        // table (host-witnessed in `smp_boot`); the single-hart order is
        // unchanged. `RecordBootHart` reads the hart from per-CPU state,
        // which `rust_main` initialized with the same id.
        for step in crate::smp_boot::PRIMARY_LATER_STEPS {
            run_later_step(step);
        }
    }

    /// Secondary later init (MS14 Iteration 000, tasks T1/T2): local steps
    /// only (`smp_boot::SECONDARY_LATER_STEPS`) — the global APLIC domain
    /// and the boot-hart record stay boot-hart-only, so the fixed routes
    /// survive secondary boot.
    #[cfg(feature = "smp")]
    fn init_later_secondary(_cpu_id: usize) {
        for step in crate::smp_boot::SECONDARY_LATER_STEPS {
            match step {
                // The secondary table is local-only by contract
                // (`steps_are_local_only`); a boot-only step here would
                // replay global init — fail closed instead.
                crate::smp_boot::LaterStep::RecordBootHart
                | crate::smp_boot::LaterStep::GlobalAplic => {
                    secondary_boot_step_reached(step)
                }
                other => run_later_step(other),
            }
        }
    }
}

/// Executes one later-init step. Shared by the boot-hart and secondary
/// tables; the boot-only arms only exist so the match stays exhaustive and
/// are never dispatched to from the secondary table.
fn run_later_step(step: crate::smp_boot::LaterStep) {
    match step {
        crate::smp_boot::LaterStep::LocalTime => crate::time::init_percpu(),
        crate::smp_boot::LaterStep::RecordBootHart => crate::irq::record_boot_hart(
            axplat::percpu::this_cpu_id(),
        ),
        crate::smp_boot::LaterStep::GlobalAplic => crate::irq::init_global(),
        crate::smp_boot::LaterStep::LocalImsicFile => crate::irq::init_percpu(),
        crate::smp_boot::LaterStep::LocalTimerSoftEnable => {
            // Caller-local CSRs only (MS14 task T2). Both are compiled in
            // the shapes that carry this step (`smp` + `runtime-irq` via
            // the `k3-smp` closure).
            #[cfg(feature = "runtime-irq")]
            crate::timer_irq::set_local_timer(true);
            #[cfg(feature = "smp")]
            crate::ipi_irq::set_local_soft(true);
        }
    }
}

/// A boot-only step reached the secondary table: report and park (external
/// RESET is the recovery path). Never silently replay global init.
#[cfg(feature = "smp")]
fn secondary_boot_step_reached(step: crate::smp_boot::LaterStep) -> ! {
    error!("k3-smp: secondary later-init reached boot-only step {step:?}; parking");
    loop {
        riscv::asm::wfi();
    }
}
