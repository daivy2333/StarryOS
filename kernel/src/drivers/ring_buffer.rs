use core::task::Context;

use axpoll::PollSet;
use ringbuf::{HeapRb, traits::{Consumer, Observer, Producer}};

pub const BUF_SIZE: usize = 64 * 1024;

pub struct RingBufRx {
    pub buf: HeapRb<u8>,
    pub poll: PollSet,
}

impl RingBufRx {
    pub fn new() -> Self {
        Self { buf: HeapRb::new(BUF_SIZE), poll: PollSet::new() }
    }
    pub fn push(&mut self, data: &[u8]) -> usize {
        let n = self.buf.push_slice(data);
        if n > 0 { self.poll.wake(); }
        n
    }
    pub fn pop(&mut self, buf: &mut [u8]) -> usize {
        self.buf.pop_slice(buf)
    }
}

pub struct RingBufTx {
    pub buf: HeapRb<u8>,
    pub poll: PollSet,
}

impl RingBufTx {
    pub fn new() -> Self {
        Self { buf: HeapRb::new(BUF_SIZE), poll: PollSet::new() }
    }
    pub fn push(&mut self, data: &[u8]) -> usize {
        self.buf.push_slice(data)
    }
    pub fn pop(&mut self, buf: &mut [u8]) -> usize {
        let n = self.buf.pop_slice(buf);
        if n > 0 { self.poll.wake(); }
        n
    }
    pub fn is_empty(&self) -> bool { self.buf.is_empty() }
    pub fn register_waker(&self, cx: &mut Context<'_>) {
        if self.buf.vacant_len() > 0 { cx.waker().wake_by_ref(); }
        else { self.poll.register(cx.waker()); }
    }
}
