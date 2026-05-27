//! AsyncUartDriver - Main driver with RX/TX copier tasks
//!
//! This module implements the AsyncUartDriver, which replaces ConsoleDriver
//! with an asynchronous backend. It provides interrupt-driven RX and TX
//! copier tasks that move data between hardware FIFO and ring buffers.

use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use core::future::poll_fn;
use core::task::Poll;

use axtask::future::{block_on, register_irq_waker};

use uart_16550::{Config, BaudRate};
use uart_16550::spec::registers::{FifoTriggerLevel, IER};  // Correct imports from registers

use super::async_uart::AsyncUart;  // Trait methods require this import
use super::uart16550_impl::Uart16550Async;
use super::isr::IsrContext;
use super::ring_buffer::AsyncBuffer;

const UART_MMIO_ADDR: usize = 0x10000000;  // QEMU virt UART base address
const UART_IRQ: usize = 10;                // QEMU virt UART IRQ number

/// AsyncUartDriver: Replaces ConsoleDriver with AsyncUart backend
///
/// This driver provides:
/// - RX copier task: Hardware FIFO → rx_buf (interrupt-driven)
/// - TX copier task: tx_buf → Hardware FIFO (interrupt-driven)
/// - AsyncBuffer: rx_buf + tx_buf + wakers
/// - ISR context: Shared with ISR handler (contains UART hardware access)
pub struct AsyncUartDriver {
    buffer: Arc<AsyncBuffer>,
    isr_ctx: Arc<IsrContext>,
    irq: usize,
    rx_copier_started: AtomicBool,
    tx_copier_started: AtomicBool,
}

impl AsyncUartDriver {
    /// Create a new AsyncUartDriver instance
    ///
    /// Initializes UART hardware, creates ISR context, starts copier tasks.
    pub fn new() -> Arc<Self> {
        // 1. Create Uart16550Async
        // SAFETY: UART MMIO address is valid on QEMU virt platform
        let mut uart = unsafe {
            Uart16550Async::new(UART_MMIO_ADDR, 4)
        };

        // 2. Initialize UART with interrupt configuration
        uart.init(Config {
            baud_rate: BaudRate::Baud115200,
            fifo_trigger_level: Some(FifoTriggerLevel::Fourteen),
            // Enable RX interrupt at init, TX interrupt disabled (idle)
            interrupts: IER::DATA_READY,
            ..Default::default()
        });

        // 3. Create ISR context (UART ownership transferred to IsrContext)
        let isr_ctx = IsrContext::new(uart);

        // 4. Create driver
        let driver = Arc::new(Self {
            buffer: Arc::new(AsyncBuffer::new_default()),
            isr_ctx,
            irq: UART_IRQ,
            rx_copier_started: AtomicBool::new(false),
            tx_copier_started: AtomicBool::new(false),
        });

        // 5. Register ISR hook (TODO: need to integrate with axhal IRQ framework)
        // axhal::register_irq_hook(UART_IRQ, uart_isr_handler);

        // 6. Start copier tasks
        driver.start_rx_copier();
        driver.start_tx_copier();

        driver
    }

    /// Get reference to AsyncBuffer
    pub fn buffer(&self) -> &Arc<AsyncBuffer> {
        &self.buffer
    }

    /// Start RX copier background task
    ///
    /// RX copier: Hardware FIFO → rx_buf
    /// Poll_fn loop: read from UART → write to rx_buf → enable RX intr → register IRQ waker → pending
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

                        // SAFETY: spin::Mutex disables interrupts (ISR-safe, never sleeps)
                        // Access UART through IsrContext's Mutex-protected uart field
                        {
                            let mut uart = driver.isr_ctx.uart.lock();

                            // 1. Read from hardware FIFO
                            let n = uart.try_read(&mut tmp_buf);

                            // 2. Write to rx_buf
                            if n > 0 {
                                driver.buffer.push_rx(&tmp_buf[..n]);
                            }

                            // 3. Re-enable RX interrupt
                            uart.enable_rx_intr();
                        }

                        // 4. Register IRQ waker for next interrupt
                        register_irq_waker(driver.irq, cx.waker());

                        // 5. Check again before pending (avoid race)
                        // SAFETY: spin::Mutex is ISR-safe
                        {
                            let mut uart = driver.isr_ctx.uart.lock();
                            let n2 = uart.try_read(&mut tmp_buf);
                            if n2 > 0 {
                                driver.buffer.push_rx(&tmp_buf[..n2]);
                            }
                        }

                        // 6. Return Pending
                        Poll::Pending
                    }))
                }
            },
            "rx-copier-m3".into(),
        );
    }

    /// Start TX copier background task
    ///
    /// TX copier: tx_buf → Hardware FIFO
    /// Poll_fn loop: pop from tx_buf → write to UART → enable/disable TX intr → register IRQ waker → pending
    fn start_tx_copier(self: &Arc<Self>) {
        if self.tx_copier_started.swap(true, Ordering::SeqCst) {
            return; // Already started
        }

        axtask::spawn_with_name(
            {
                let driver = self.clone();
                move || {
                    block_on(poll_fn(|cx| {
                        let mut tmp_buf = [0u8; 256];

                        // 1. Pop from tx_buf
                        let n = driver.buffer.pop_tx(&mut tmp_buf);

                        if n > 0 {
                            // SAFETY: spin::Mutex is ISR-safe
                            {
                                let mut uart = driver.isr_ctx.uart.lock();

                                // 2. Write to hardware FIFO
                                let sent = uart.try_write(&tmp_buf[..n]);

                                // 3. If sent < n, FIFO full → push remaining back to tx_buf
                                if sent < n {
                                    driver.buffer.push_tx(&tmp_buf[sent..n]);
                                }

                                // 4. Check if more data pending → enable TX interrupt
                                let remaining = driver.buffer.tx_len();
                                if remaining > 0 {
                                    uart.enable_tx_intr();
                                } else {
                                    // All data sent → disable TX interrupt (avoid spurious)
                                    uart.disable_tx_intr();
                                }
                            }
                        } else {
                            // No data to send → ensure TX interrupt disabled
                            // SAFETY: spin::Mutex is ISR-safe
                            let mut uart = driver.isr_ctx.uart.lock();
                            uart.disable_tx_intr();
                        }

                        // 5. Register IRQ waker (supports multiple wakers via PollSet)
                        register_irq_waker(driver.irq, cx.waker());

                        // 6. Return Pending
                        Poll::Pending
                    }))
                }
            },
            "tx-copier-m3".into(),
        );
    }
}

// SAFETY: AsyncUartDriver can be sent to other threads (wrapped in Arc)
unsafe impl Send for AsyncUartDriver {}