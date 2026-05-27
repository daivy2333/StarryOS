use alloc::sync::Arc;
use core::future::poll_fn;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Poll;

use axtask::future::{block_on, register_irq_waker};

use super::ring_buffer::AsyncBuffer;

/// Console driver with RX copier task and TX sync flush
///
/// This driver spawns a background task (RX copier) that:
/// 1. Reads from Console HAL when IRQ arrives
/// 2. Copies data to the shared ring buffer
/// 3. Wakes waiting readers
///
/// TX is handled synchronously for M1 simplicity (direct console write).
pub struct ConsoleDriver {
    buffer: Arc<AsyncBuffer>,
    rx_irq: usize,
    rx_copier_started: AtomicBool,
}

impl ConsoleDriver {
    /// Create a new ConsoleDriver and start the RX copier task
    pub fn new() -> Arc<Self> {
        let driver = Arc::new(Self {
            buffer: Arc::new(AsyncBuffer::new_default()),
            rx_irq: axhal::console::irq_num().unwrap_or(10),
            rx_copier_started: AtomicBool::new(false),
        });

        // Start RX copier task
        driver.start_rx_copier();

        driver
    }

    /// Get a reference to the async buffer
    pub fn buffer(&self) -> &Arc<AsyncBuffer> {
        &self.buffer
    }

    /// Start the RX copier background task
    ///
    /// The task waits for RX IRQs, reads available bytes from console,
    /// and pushes them to the ring buffer.
    fn start_rx_copier(self: &Arc<Self>) {
        if self.rx_copier_started.swap(true, Ordering::SeqCst) {
            return; // Already started
        }

        axtask::spawn_with_name(
            {
                let driver = self.clone();
                move || {
                    block_on(poll_fn(|cx| {
                        let mut tmp_buf = [0u8; 256];

                        // 1. Read from Console HAL
                        let n = axhal::console::read_bytes(&mut tmp_buf);

                        // 2. Write to rx_buf and wake readers
                        if n > 0 {
                            driver.buffer.push_rx(&tmp_buf[..n]);
                        }

                        // 3. Register IRQ waker for next IRQ
                        register_irq_waker(driver.rx_irq, cx.waker());

                        // 4. Check again before pending (avoid race condition)
                        let n2 = axhal::console::read_bytes(&mut tmp_buf);
                        if n2 > 0 {
                            driver.buffer.push_rx(&tmp_buf[..n2]);
                        }

                        // 5. Return Pending - wait for next IRQ
                        Poll::Pending
                    }))
                }
            },
            "rx-copier".into(),
        );
    }

    /// Flush TX buffer to Console (synchronous, M1 simplified)
    ///
    /// Pops all data from TX buffer and writes to console directly.
    /// This is a synchronous operation for M1 validation.
    pub fn flush_tx_sync(&self) {
        let mut tmp_buf = [0u8; 256];
        loop {
            let n = self.buffer.pop_tx(&mut tmp_buf);
            if n == 0 {
                break;
            }
            axhal::console::write_bytes(&tmp_buf[..n]);
        }
    }
}