use core::sync::atomic::{AtomicBool, Ordering};
use embassy_sync::waitqueue::AtomicWaker;
use uart_16550::spec::registers::InterruptType;
use crate::drivers::uart_init::{read_isr_unlocked, disable_rx_intr, disable_tx_intr};

pub static RX_WAKER: AtomicWaker = AtomicWaker::new();
pub static TX_WAKER: AtomicWaker = AtomicWaker::new();
pub static DRAIN_WAKER: AtomicWaker = AtomicWaker::new();
pub static TCDRAIN_ACTIVE: AtomicBool = AtomicBool::new(false);

pub fn uart_isr_handler(_irq: usize) {
    let isr = read_isr_unlocked();
    match isr.interrupt_type() {
        Some(InterruptType::ReceivedDataReady) | Some(InterruptType::ReceptionTimeout) => {
            disable_rx_intr();
            RX_WAKER.wake();
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            disable_tx_intr();
            TX_WAKER.wake();
            if TCDRAIN_ACTIVE.load(Ordering::Acquire) {
                DRAIN_WAKER.wake();
            }
        }
        _ => {}
    }
}
