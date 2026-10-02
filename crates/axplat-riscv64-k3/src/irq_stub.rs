//! Fail-closed IRQ interface stub for K3 bring-up builds.
//!
//! MS09 does not bring up interrupt delivery (no APLIC/IMSIC wiring). The
//! wider StarryOS build enables the `axplat/irq` interface, so the platform
//! must still provide IrqIf symbols for link-time interface resolution;
//! every operation stays inert (MS09 design D3).

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
