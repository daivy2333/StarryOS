//! ArceOS adapter for uart_16550 OS abstraction traits.
//!
//! Implements the 5 platform-independent traits defined in `uart_16550::os`
//! using ArceOS kernel services (axtask, axhal, axmm, kspin, axpoll).

use core::future::Future;
use core::ptr::NonNull;
use core::task::Waker;

use alloc::string::ToString;
use memory_addr::PhysAddr;
use uart_16550::os::{OsIrq, OsMmio, OsRuntime, OsSpinNoIrq, OsWakerSet};

// ── OsRuntime: task spawning and blocking ────────────────────────────

/// ArceOS runtime adapter using `axtask`.
pub struct ArceOsRuntime;

impl OsRuntime for ArceOsRuntime {
    fn spawn<F>(future: F, name: &str)
    where
        F: Future + Send + 'static,
        F::Output: Send,
    {
        let name = name.to_string();
        axtask::spawn_with_name(
            move || {
                axtask::future::block_on(future);
            },
            name,
        );
    }

    fn block_on<F>(future: F) -> F::Output
    where
        F: Future,
    {
        axtask::future::block_on(future)
    }
}

// ── OsIrq: interrupt handler registration ────────────────────────────

/// ArceOS IRQ adapter using `axhal`.
pub struct ArceOsIrq;

impl OsIrq for ArceOsIrq {
    fn register_handler(_irq_number: usize, handler: fn(usize)) {
        // ArceOS provides a single global IRQ hook; the handler receives
        // the IRQ number as its argument and can dispatch internally.
        axhal::irq::register_irq_hook(handler);
    }
}

// ── OsMmio: memory-mapped I/O ────────────────────────────────────────

/// ArceOS MMIO adapter using `axmm` and `axhal`.
pub struct ArceOsMmio;

impl OsMmio for ArceOsMmio {
    unsafe fn map_mmio(phys_addr: usize, size: usize) -> NonNull<u8> {
        // SAFETY: caller guarantees `phys_addr` is a valid MMIO region
        // and `size` is correct. The mapping persists for the kernel lifetime.
        let phys = PhysAddr::from(phys_addr);
        match axmm::iomap(phys, size) {
            Ok(vaddr) => NonNull::new(vaddr.as_mut_ptr()).expect("iomap returned null"),
            Err(_) => {
                // Mapping may already exist (e.g., boot-time identity map).
                // Fall back to phys_to_virt.
                Self::phys_to_virt(phys_addr)
            }
        }
    }

    fn phys_to_virt(phys_addr: usize) -> NonNull<u8> {
        let phys = PhysAddr::from(phys_addr);
        let virt = axhal::mem::phys_to_virt(phys);
        NonNull::new(virt.as_mut_ptr()).expect("phys_to_virt returned null")
    }
}

// ── OsSpinNoIrq: IRQ-safe spinlock ──────────────────────────────────

/// ArceOS spinlock adapter using `kspin`.
pub struct ArceOsSpinNoIrq<T> {
    inner: kspin::SpinNoIrq<T>,
}

impl<T> OsSpinNoIrq<T> for ArceOsSpinNoIrq<T> {
    fn new(val: T) -> Self {
        Self {
            inner: kspin::SpinNoIrq::new(val),
        }
    }

    fn with_lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R {
        let mut guard = self.inner.lock();
        f(&mut *guard)
    }
}

// ── OsWakerSet: waker registration and notification ──────────────────

/// ArceOS waker set adapter using `axpoll::PollSet`.
pub struct ArceOsWakerSet {
    inner: axpoll::PollSet,
}

impl OsWakerSet for ArceOsWakerSet {
    fn new() -> Self {
        Self {
            inner: axpoll::PollSet::new(),
        }
    }

    fn register(&self, waker: &Waker) {
        self.inner.register(waker);
    }

    fn wake(&self) -> u32 {
        self.inner.wake() as u32
    }
}
