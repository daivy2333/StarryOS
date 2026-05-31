use alloc::{sync::Arc, vec};
use core::future::poll_fn;
use core::task::Poll;

use axtask::{future::block_on, spawn_with_name};
use axsync::Mutex;
use lazy_static::lazy_static;

use crate::drivers::isr::{RX_WAKER, TX_WAKER};
use crate::drivers::ring_buffer::AsyncBuffer;
use crate::drivers::uart_init::{uart_instance, enable_rx_intr, enable_tx_intr};

const COPIER_BUF_SIZE: usize = 256;

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
        loop {
            poll_fn(|cx| {
                let mut uart = uart_instance().lock();
                let mut total = 0;
                while total < COPIER_BUF_SIZE {
                    match uart.try_receive_byte() { Ok(b) => { read_buf[total]=b; total+=1; } Err(_)=>break }
                }
                drop(uart);
                if total > 0 { self.buffer.lock().push_rx(&read_buf[..total]); }
                enable_rx_intr();
                RX_WAKER.register(cx.waker());
                if total > 0 { Poll::Ready(total) } else { Poll::Pending }
            }).await;
        }
    }

    async fn tx_copier_loop(&self) {
        let mut write_buf = vec![0u8; COPIER_BUF_SIZE];
        loop {
            poll_fn(|cx| {
                let pending = {
                    let mut buf = self.buffer.lock();
                    let n = buf.pop_tx(&mut write_buf);
                    if n > 0 { n } else { buf.register_tx_waker(cx); return Poll::Pending; }
                };
                let mut uart = uart_instance().lock();
                let mut sent = 0;
                for &b in &write_buf[..pending] {
                    match uart.try_send_byte(b) { Ok(_)=>sent+=1, Err(_)=>break }
                }
                if sent < pending {
                    self.buffer.lock().push_tx(&write_buf[sent..pending]);
                    enable_tx_intr(); // FIFO full, wait for THR_EMPTY IRQ
                }
                drop(uart);
                TX_WAKER.register(cx.waker());
                Poll::Ready(())
            }).await;
        }
    }
}
