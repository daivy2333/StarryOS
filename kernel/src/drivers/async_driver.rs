use alloc::{sync::Arc, vec};
use core::future::poll_fn;
use core::task::{Poll, Waker};
use core::cell::Cell;

use axtask::{future::block_on, spawn_with_name};
use axsync::Mutex;
use lazy_static::lazy_static;

use crate::drivers::isr::{RX_WAKER, TX_WAKER};
use crate::drivers::ring_buffer::AsyncBuffer;
use crate::drivers::uart_init::{uart_instance, enable_rx_intr, enable_tx_intr};

const COPIER_BUF_SIZE: usize = 1024;
const BATCH_SIZE: usize = 16; // NS16550 FIFO depth

lazy_static! {
    pub static ref DRIVER: Arc<AsyncUartDriver> = AsyncUartDriver::new();
}

pub struct AsyncUartDriver {
    pub buffer: Mutex<AsyncBuffer>,
}

impl AsyncUartDriver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { buffer: Mutex::new(AsyncBuffer::new()) })
    }

    pub fn start_rx_copier(self: &Arc<Self>) {
        let this = Arc::clone(self);
        spawn_with_name(move || { block_on(this.rx_copier_loop()); }, "uart-rx-copier".into());
    }

    pub fn start_tx_copier(self: &Arc<Self>) {
        let this = Arc::clone(self);
        spawn_with_name(move || { block_on(this.tx_copier_loop()); }, "uart-tx-copier".into());
    }

    async fn rx_copier_loop(&self) {
        let mut read_buf = vec![0u8; COPIER_BUF_SIZE];
        let mut last_waker: Cell<Option<Waker>> = Cell::new(None);
        loop {
            poll_fn(|cx| {
                // Batch drain UART FIFO: single SpinNoIrq lock for all reads
                let mut uart = uart_instance().lock();
                let mut total = 0;
                // Read up to BATCH_SIZE at a time (no per-byte LSR check)
                while total < COPIER_BUF_SIZE {
                    match uart.try_receive_byte() {
                        Ok(byte) => { read_buf[total] = byte; total += 1; }
                        Err(_) => break,
                    }
                }
                drop(uart);

                if total > 0 {
                    self.buffer.lock().push_rx(&read_buf[..total]);
                }

                enable_rx_intr();

                // O31: skip AtomicWaker register if waker unchanged
                let new_waker = cx.waker().clone();
                if last_waker.replace(Some(new_waker.clone())).as_ref().map_or(true, |old| !old.will_wake(&new_waker)) {
                    RX_WAKER.register(cx.waker());
                }

                if total > 0 { Poll::Ready(total) } else { Poll::Pending }
            }).await;
        }
    }

    async fn tx_copier_loop(&self) {
        let mut write_buf = vec![0u8; COPIER_BUF_SIZE];
        let mut last_waker: Cell<Option<Waker>> = Cell::new(None);
        loop {
            poll_fn(|cx| {
                // Pop from ring buffer (single lock)
                let pending = {
                    let mut buf = self.buffer.lock();
                    let n = buf.pop_tx(&mut write_buf);
                    if n > 0 { n } else { buf.register_tx_waker(cx); return Poll::Pending; }
                };

                // Batch write to UART FIFO (single SpinNoIrq lock)
                let mut uart = uart_instance().lock();
                let mut sent = 0;
                for &b in &write_buf[..pending] {
                    match uart.try_send_byte(b) {
                        Ok(_) => { sent += 1; }
                        Err(_) => {
                            // FIFO full: push remaining back in ONE buffer lock
                            self.buffer.lock().push_tx(&write_buf[sent..pending]);
                            enable_tx_intr();
                            break;
                        }
                    }
                }
                drop(uart);

                let new_waker = cx.waker().clone();
                if last_waker.replace(Some(new_waker.clone())).as_ref().map_or(true, |old| !old.will_wake(&new_waker)) {
                    TX_WAKER.register(cx.waker());
                }

                Poll::Ready(())
            }).await;
        }
    }
}
