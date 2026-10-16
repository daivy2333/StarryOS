use axplat::power::PowerIf;

struct PowerImpl;

#[impl_plat_interface]
impl PowerIf for PowerImpl {
    /// Bootstraps the given CPU core with the given initial stack (in
    /// physical address) via the SBI HSM extension (MS14 Iteration 000,
    /// task T1). Every refusal parks with a diagnosable line: returning
    /// would leave `axruntime::start_secondary_cpus` waiting on
    /// `ENTERED_CPUS` forever, so the refusal branch never returns.
    #[cfg(feature = "smp")]
    fn cpu_boot(cpu_id: usize, stack_top_paddr: usize) {
        use axplat::mem::{va, virt_to_phys};

        let max = crate::config::plat::MAX_CPU_NUM;
        if let Err(e) =
            crate::smp_boot::check_boot_request(cpu_id, max, stack_top_paddr)
        {
            smp_boot_failed(format_args!(
                "refused target={cpu_id} stack={stack_top_paddr:#x}: {e:?}"
            ));
        }
        if let Err(e) =
            crate::smp_boot::check_hsm(sbi_rt::probe_extension(sbi_rt::Hsm).is_unavailable())
        {
            smp_boot_failed(format_args!("HSM probe unavailable: {e:?}"));
        }
        let entry = virt_to_phys(va!(crate::boot::_start_secondary as *const () as usize));
        let ret = sbi_rt::hart_start(cpu_id, entry.as_usize(), stack_top_paddr);
        if ret.is_err() {
            smp_boot_failed(format_args!(
                "hart_start({cpu_id}) error={:#x}",
                ret.error
            ));
        }
    }

    fn system_off() -> ! {
        info!("Shutting down...");
        sbi_rt::system_reset(sbi_rt::Shutdown, sbi_rt::NoReason);
        warn!("It should shutdown!");
        loop {
            axcpu::asm::halt();
        }
    }

    fn cpu_num() -> usize {
        crate::config::plat::MAX_CPU_NUM
    }
}

/// Reports one refused secondary start and parks. The serial line is the
/// diagnostic surface; external RESET is the recovery path.
#[cfg(feature = "smp")]
fn smp_boot_failed(context: core::fmt::Arguments<'_>) -> ! {
    error!("k3-smp: cpu_boot refused ({context}); parking (reset to recover)");
    loop {
        riscv::asm::wfi();
    }
}
