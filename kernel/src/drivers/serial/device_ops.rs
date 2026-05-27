use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Context;

use axfs_ng_vfs::VfsResult;
use axpoll::{IoEvents, Pollable};
use axtask::future::{block_on, poll_io};
use ringbuf::traits::Observer;

use crate::pseudofs::DeviceOps;

use super::console_driver::ConsoleDriver;

/// Test device for async UART architecture validation
///
/// This device implements DeviceOps and Pollable traits, providing:
/// - Async read/write operations via block_on + poll_io pattern
/// - Non-blocking mode support
/// - Integration with the RX copier and TX flush mechanism
pub struct AsyncUartTestDevice {
    driver: Arc<ConsoleDriver>,
    non_blocking: AtomicBool,
}

impl AsyncUartTestDevice {
    /// Create a new AsyncUartTestDevice
    ///
    /// This creates a new device with:
    /// - A ConsoleDriver instance (spawns RX copier task)
    /// - Non-blocking mode disabled by default
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            driver: ConsoleDriver::new(),
            non_blocking: AtomicBool::new(false),
        })
    }

    /// Check if non-blocking mode is enabled
    fn nonblocking(&self) -> bool {
        self.non_blocking.load(Ordering::Acquire)
    }

    /// Set non-blocking mode
    pub fn set_nonblocking(&self, nonblocking: bool) {
        self.non_blocking.store(nonblocking, Ordering::Release);
    }
}

impl DeviceOps for AsyncUartTestDevice {
    /// Read data from the UART device
    ///
    /// Uses async I/O with poll_io to wait for data availability.
    /// Returns WouldBlock error if non-blocking mode is enabled and no data is available.
    fn read_at(&self, buf: &mut [u8], _offset: u64) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        block_on(poll_io(
            self,
            IoEvents::IN,
            self.nonblocking(),
            || {
                let n = self.driver.buffer().pop_rx(buf);
                if n > 0 {
                    Ok(n)
                } else {
                    Err(axerrno::AxError::WouldBlock)
                }
            },
        ))
    }

    /// Write data to the UART device
    ///
    /// Uses async I/O with poll_io to wait for buffer space.
    /// Data is flushed synchronously to console after being buffered.
    fn write_at(&self, buf: &[u8], _offset: u64) -> VfsResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        block_on(poll_io(
            self,
            IoEvents::OUT,
            self.nonblocking(),
            || {
                let n = self.driver.buffer().push_tx(buf);
                if n > 0 {
                    self.driver.flush_tx_sync();
                    Ok(n)
                } else {
                    Err(axerrno::AxError::WouldBlock)
                }
            },
        ))
    }

    fn as_any(&self) -> &dyn core::any::Any {
        self
    }

    /// Return this device as a Pollable object
    fn as_pollable(&self) -> Option<&dyn Pollable> {
        Some(self)
    }
}

impl Pollable for AsyncUartTestDevice {
    /// Poll the device for I/O events
    ///
    /// Returns:
    /// - IoEvents::IN if RX buffer has data
    /// - IoEvents::OUT if TX buffer has space
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();

        let rx_buf = self.driver.buffer().rx_buf.lock();
        let tx_buf = self.driver.buffer().tx_buf.lock();

        events.set(IoEvents::IN, rx_buf.occupied_len() > 0);
        events.set(IoEvents::OUT, tx_buf.vacant_len() > 0);

        events
    }

    /// Register wakers for I/O events
    ///
    /// Registers:
    /// - RX waker if IN events are requested
    /// - TX waker if OUT events are requested
    fn register(&self, cx: &mut Context, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.driver.buffer().register_rx_waker(cx.waker());
        }
        if events.contains(IoEvents::OUT) {
            self.driver.buffer().register_tx_waker(cx.waker());
        }
    }
}