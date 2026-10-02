use axplat::power::PowerIf;

struct PowerImpl;

#[impl_plat_interface]
impl PowerIf for PowerImpl {
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
