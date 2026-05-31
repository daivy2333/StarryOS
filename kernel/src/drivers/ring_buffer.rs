// AsyncBuffer — double ring buffer with poll support

use alloc::vec::Vec;
use core::task::Context;

use axpoll::{IoEvents, PollSet, Pollable};
use ringbuf::{HeapRb, traits::{Consumer, Observer, Producer}};

const BUF_SIZE: usize = 64 * 1024; // 64 KiB

pub struct AsyncBuffer {
    rx: HeapRb<u8>,
    tx: HeapRb<u8>,
    rx_poll: PollSet,
    tx_poll: PollSet,
}

impl AsyncBuffer {
    pub fn new() -> Self {
        Self {
            rx: HeapRb::new(BUF_SIZE),
            tx: HeapRb::new(BUF_SIZE),
            rx_poll: PollSet::new(),
            tx_poll: PollSet::new(),
        }
    }

    pub fn push_rx(&mut self, data: &[u8]) -> usize {
        let n = self.rx.push_slice(data);
        if n > 0 {
            self.rx_poll.wake();
        }
        n
    }

    pub fn pop_rx(&mut self, buf: &mut [u8]) -> usize {
        self.rx.pop_slice(buf)
    }

    pub fn rx_available(&self) -> usize {
        self.rx.occupied_len()
    }

    pub fn rx_is_empty(&self) -> bool {
        self.rx.is_empty()
    }

    pub fn rx_capacity(&self) -> usize {
        self.rx.vacant_len()
    }

    pub fn register_rx_waker(&self, cx: &mut Context<'_>) {
        if !self.rx.is_empty() {
            cx.waker().wake_by_ref();
        } else {
        self.rx_poll.register(cx.waker());
        }
    }

    pub fn push_tx(&mut self, data: &[u8]) -> usize {
        self.tx.push_slice(data)
    }

    pub fn pop_tx(&mut self, buf: &mut [u8]) -> usize {
        let n = self.tx.pop_slice(buf);
        if n > 0 {
            self.tx_poll.wake();
        }
        n
    }

    pub fn tx_pending(&self) -> usize {
        self.tx.occupied_len()
    }

    pub fn tx_is_empty(&self) -> bool {
        self.tx.is_empty()
    }

    pub fn tx_capacity(&self) -> usize {
        self.tx.vacant_len()
    }

    pub fn register_tx_waker(&self, cx: &mut Context<'_>) {
        if self.tx.vacant_len() > 0 {
            cx.waker().wake_by_ref();
        } else {
            self.tx_poll.register(cx.waker());
        }
    }

    pub fn drain_rx(&mut self) -> Vec<u8> {
        let mut v = alloc::vec![0u8; self.rx.occupied_len()];
        let n = self.pop_rx(&mut v);
        v.truncate(n);
        v
    }
}

impl Pollable for AsyncBuffer {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        if self.rx_available() > 0 {
            events |= IoEvents::IN;
        }
        if self.tx_capacity() > 0 {
            events |= IoEvents::OUT;
        }
        events
    }

    fn register(&self, cx: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.register_rx_waker(cx);
        }
        if events.contains(IoEvents::OUT) {
            self.register_tx_waker(cx);
        }
    }
}
