use axplat::{console, init::InitIf};

struct InitIfImpl;

#[inline(always)]
fn boot_mark(message: &str) {
    console::write_bytes(message.as_bytes());
}

#[impl_plat_interface]
impl InitIf for InitIfImpl {
    fn init_early(_cpu_id: usize, _mbi: usize) {
        axcpu::init::init_trap();
        crate::console::init_early();
        crate::time::init_early();
    }

    #[cfg(feature = "smp")]
    fn init_early_secondary(_cpu_id: usize) {
        axcpu::init::init_trap();
    }

    fn init_later(_cpu_id: usize, _arg: usize) {
        boot_mark("[d1-init] init_later enter\n");
        #[cfg(feature = "irq")]
        {
            boot_mark("[d1-init] before irq init\n");
            crate::irq::init_percpu();
            boot_mark("[d1-init] after irq init\n");
        }
        boot_mark("[d1-init] before time init\n");
        crate::time::init_percpu();
        boot_mark("[d1-init] after time init\n");
    }

    #[cfg(feature = "smp")]
    fn init_later_secondary(_cpu_id: usize) {
        #[cfg(feature = "irq")]
        crate::irq::init_percpu();
        crate::time::init_percpu();
    }
}
