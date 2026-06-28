//! Minimal IRQ interface for D1 smoke builds.
//!
//! Q19a does not bring up PLIC or device IRQ delivery. However the wider
//! StarryOS build enables the `axplat/irq` interface, so the platform must
//! still provide IrqIf symbols for link-time interface resolution.

use axplat::irq::{IpiTarget, IrqHandler, IrqIf};

struct IrqIfImpl;

#[impl_plat_interface]
impl IrqIf for IrqIfImpl {
    fn set_enable(_irq: usize, _enabled: bool) {}

    fn register(_irq: usize, _handler: IrqHandler) -> bool {
        false
    }

    fn unregister(_irq: usize) -> Option<IrqHandler> {
        None
    }

    fn handle(_irq: usize) -> Option<usize> {
        None
    }

    fn send_ipi(_irq_num: usize, _target: IpiTarget) {}
}
