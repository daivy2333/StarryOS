use axsync::Mutex;
use axpoll::PollSet;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer};

const DEFAULT_CAPACITY: usize = 65536; // 64 KiB

/// Async buffer with RX and TX ring buffers + waker sets
pub struct AsyncBuffer {
    pub rx_buf: Mutex<HeapRb<u8>>,
    pub tx_buf: Mutex<HeapRb<u8>>,
    rx_wakers: PollSet,
    tx_wakers: PollSet,
}

impl AsyncBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            rx_buf: Mutex::new(HeapRb::new(capacity)),
            tx_buf: Mutex::new(HeapRb::new(capacity)),
            rx_wakers: PollSet::new(),
            tx_wakers: PollSet::new(),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    pub fn rx_len(&self) -> usize {
        self.rx_buf.lock().occupied_len()
    }

    pub fn tx_len(&self) -> usize {
        self.tx_buf.lock().occupied_len()
    }

    pub fn tx_vacant(&self) -> usize {
        self.tx_buf.lock().vacant_len()
    }

    /// Push data to RX buffer (called by RX copier)
    pub fn push_rx(&self, data: &[u8]) -> usize {
        let mut buf = self.rx_buf.lock();
        let n = buf.push_slice(data);
        self.rx_wakers.wake();
        n
    }

    /// Pop data from RX buffer (called by user read)
    pub fn pop_rx(&self, buf: &mut [u8]) -> usize {
        let rx = self.rx_buf.lock();
        let (left, right) = rx.as_slices();
        let mut count = 0;
        if !left.is_empty() {
            let n = left.len().min(buf.len());
            buf[..n].copy_from_slice(&left[..n]);
            count = n;
        }
        if !right.is_empty() && count < buf.len() {
            let n = right.len().min(buf.len() - count);
            buf[count..count+n].copy_from_slice(&right[..n]);
            count += n;
        }
        unsafe { rx.advance_read_index(count) };
        count
    }

    /// Push data to TX buffer (called by user write)
    pub fn push_tx(&self, data: &[u8]) -> usize {
        let mut buf = self.tx_buf.lock();
        let n = buf.push_slice(data);
        self.tx_wakers.wake();
        n
    }

    /// Pop data from TX buffer (called by TX copier/sync flush)
    pub fn pop_tx(&self, buf: &mut [u8]) -> usize {
        let tx = self.tx_buf.lock();
        let (left, right) = tx.as_slices();
        let mut count = 0;
        if !left.is_empty() {
            let n = left.len().min(buf.len());
            buf[..n].copy_from_slice(&left[..n]);
            count = n;
        }
        if !right.is_empty() && count < buf.len() {
            let n = right.len().min(buf.len() - count);
            buf[count..count+n].copy_from_slice(&right[..n]);
            count += n;
        }
        unsafe { tx.advance_read_index(count) };
        count
    }

    /// Wake RX waiters
    pub fn wake_rx(&self) {
        self.rx_wakers.wake();
    }

    /// Wake TX waiters
    pub fn wake_tx(&self) {
        self.tx_wakers.wake();
    }

    /// Register RX waker
    pub fn register_rx_waker(&self, waker: &core::task::Waker) {
        self.rx_wakers.register(waker);
    }

    /// Register TX waker
    pub fn register_tx_waker(&self, waker: &core::task::Waker) {
        self.tx_wakers.register(waker);
    }
}