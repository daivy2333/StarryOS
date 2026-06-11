use alloc::{sync::Arc, vec};
use core::future::poll_fn;
use core::task::{Poll, Waker};
use core::cell::Cell;

use axtask::{future::block_on, spawn_with_name};
use lazy_static::lazy_static;

use crate::drivers::isr::{RX_WAKER, TX_WAKER};
use crate::drivers::ring_buffer::{RingBufRx, RingBufTx};
use crate::drivers::uart_init::{uart_instance, enable_rx_intr, enable_tx_intr, NAPI_THRESHOLD, NAPI_BATCH_SIZE};

const COPIER_BUF_SIZE: usize = 1024;

lazy_static! {
    pub static ref DRIVER: Arc<AsyncUartDriver> = AsyncUartDriver::new();
}

pub struct AsyncUartDriver {
    pub rx: RingBufRx,
    pub tx: RingBufTx,
}

impl AsyncUartDriver {
    pub fn new() -> Arc<Self> {
        Arc::new(Self { rx: RingBufRx::new(), tx: RingBufTx::new() })
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
        let last_waker: Cell<Option<Waker>> = Cell::new(None);
        let mut consecutive = 0u32;
        loop {
            poll_fn(|cx| {
                let mut uart = uart_instance().lock();
                let batch = if consecutive >= NAPI_THRESHOLD { NAPI_BATCH_SIZE } else { COPIER_BUF_SIZE };
                let total = uart.receive_bytes(&mut read_buf[..batch]);
                drop(uart);
                if total > 0 { self.rx.push(&read_buf[..total]); }
                if consecutive >= NAPI_THRESHOLD {
                    if total > 0 {
                        consecutive += 1;
                    } else {
                        consecutive = 0;
                        enable_rx_intr();
                    }
                } else {
                    consecutive = if total > 0 { consecutive + 1 } else { 0 };
                }
                if consecutive < NAPI_THRESHOLD { enable_rx_intr(); }
                let w = cx.waker().clone();
                let old = last_waker.replace(Some(w.clone()));
                if old.as_ref().map_or(true, |old_w| !old_w.will_wake(&w)) {
                    RX_WAKER.register(cx.waker());
                }
                if total > 0 { Poll::Ready(total) } else { Poll::Pending }
            }).await;
        }
    }

    async fn tx_copier_loop(&self) {
        let mut write_buf = vec![0u8; COPIER_BUF_SIZE];
        let mut pending = 0usize;
        let mut cursor = 0usize;
        let last_waker: Cell<Option<Waker>> = Cell::new(None);
        loop {
            poll_fn(|cx| {
                if cursor >= pending {
                    pending = self.tx.pop(&mut write_buf);
                    cursor = 0;
                    if pending == 0 { self.tx.register_waker(cx); return Poll::Pending; }
                }
                let mut uart = uart_instance().lock();
                let sent = uart.send_bytes(&write_buf[cursor..pending]);
                drop(uart);
                cursor += sent;
                if cursor < pending {
                    enable_tx_intr();
                }
                let w = cx.waker().clone();
                let old = last_waker.replace(Some(w.clone()));
                if old.as_ref().map_or(true, |old_w| !old_w.will_wake(&w)) {
                    TX_WAKER.register(cx.waker());
                }
                Poll::Ready(())
            }).await;
        }
    }
}
