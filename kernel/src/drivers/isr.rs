use embassy_sync::waitqueue::AtomicWaker;
use uart_16550::spec::registers::InterruptType;
use crate::drivers::uart_init::{uart_instance, disable_rx_intr, disable_tx_intr};

pub static RX_WAKER: AtomicWaker = AtomicWaker::new();
pub static TX_WAKER: AtomicWaker = AtomicWaker::new();

pub fn uart_isr_handler(_irq: usize) {
    let mut uart = uart_instance().lock();
    let isr = uart.isr();
    match isr.interrupt_type() {
        Some(InterruptType::ReceivedDataReady) | Some(InterruptType::ReceptionTimeout) => {
            disable_rx_intr();
            RX_WAKER.wake();
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            disable_tx_intr();
            TX_WAKER.wake();
        }
        _ => {}
    }
}
