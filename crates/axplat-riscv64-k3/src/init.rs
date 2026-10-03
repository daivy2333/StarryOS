use axplat::init::InitIf;

struct InitIfImpl;

#[impl_plat_interface]
impl InitIf for InitIfImpl {
    fn init_early(_cpu_id: usize, _mbi: usize) {
        axcpu::init::init_trap();
        crate::console::init_early();
        crate::time::init_early();
    }

    fn init_later(cpu_id: usize, _arg: usize) {
        crate::time::init_percpu();
        // MS10 task 1.2: bring up the AIA stack on the boot hart before any
        // kernel stage runs. The raw hartid from `_start` is stored (never
        // assume 0); global init runs exactly once (single-hart build,
        // max-cpu-num = 1), then the local per-hart file.
        crate::irq::record_boot_hart(cpu_id);
        crate::irq::init_global();
        crate::irq::init_percpu();
    }
}
