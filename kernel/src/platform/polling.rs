//! Low-level MMIO UART access trait — raw, no buffering, no LF conversion.
//!
//! [`PollingPort`] is the primitive hardware interface. It does not:
//!   - convert `\n` to `\r\n` (that is the TTY's job)
//!   - buffer bytes in software rings
//!   - handle interrupts
//!   - emit telemetry
//!
//! # Platform gating
//!
//! QEMU (NS16550, stride 1, U8 access) and D1 (DW APB UART, stride 4, U32
//! access) are mutually exclusive at the feature level per
//! `kernel/src/platform/mod.rs` compile_error guard.

use alloc::boxed::Box;
use core::ptr::NonNull;

use axhal::mem::phys_to_virt;
use kspin::SpinNoPreempt;
use memory_addr::PhysAddr;

use super::console::{ConsoleConfig, ConsoleKind};

// ── PollingPort trait ────────────────────────────────────────────────

/// Direct MMIO access to a UART — blocking writes, non-blocking reads.
///
/// Implementors MUST NOT perform LF→CRLF conversion or any buffering.
/// All methods access hardware registers via volatile reads/writes.
pub trait PollingPort: Send {
    /// Blocking write of a single byte.
    ///
    /// Polls the Line Status Register until the Transmitter Holding Register
    /// is empty (THRE, bit 5), then writes the byte to THR.
    fn putchar(&self, ch: u8);

    /// Non-blocking read of a single byte.
    ///
    /// Checks LSR bit 0 (Data Ready). Returns `Some(byte)` if data is
    /// available, `None` otherwise. Does NOT block or spin.
    fn try_getchar(&self) -> Option<u8>;

    /// Check LSR bit 5 — Transmitter Holding Register Empty.
    ///
    /// Returns `true` when the UART can accept another byte for
    /// transmission immediately.
    fn thre(&self) -> bool;

    /// Check LSR bit 6 — Transmitter Empty (shift register done).
    ///
    /// Returns `true` when the transmitter has completed sending all
    /// data including the byte currently in the shift register.
    fn temt(&self) -> bool;
}

// ── NS16550 U8 (QEMU virt) ───────────────────────────────────────────

/// NS16550 UART polling port using 8-bit MMIO access (stride 1).
///
/// Suitable for QEMU virt where the NS16550 is byte-addressed.
/// Register offsets:
///   - THR (write): offset 0 — Transmitter Holding Register
///   - RBR (read):  offset 0 — Receiver Buffer Register
///   - LSR:         offset 5 — Line Status Register
pub struct Ns16550U8PollingPort {
    /// Virtual address of UART MMIO base (already mapped).
    base: NonNull<u8>,
    /// Register stride in bytes (1 for QEMU NS16550).
    stride: u8,
}

impl Ns16550U8PollingPort {
    /// Create from a virtual address and stride.
    ///
    /// # Safety
    ///
    /// `virt_addr` must be a valid, MMIO-mapped virtual address for
    /// an NS16550 UART with DEVICE|READ|WRITE permissions.
    pub unsafe fn new(virt_addr: usize, stride: u8) -> Self {
        // SAFETY: caller guarantees the virtual address is valid and mapped.
        let base = NonNull::new(virt_addr as *mut u8).unwrap();
        Self { base, stride }
    }
}

// SAFETY: The MMIO pointer is device memory, and access is mediated by
// `SpinNoPreempt` in CONSOLE_PORT, so it is safe to share between threads.
unsafe impl Send for Ns16550U8PollingPort {}

impl PollingPort for Ns16550U8PollingPort {
    fn putchar(&self, ch: u8) {
        let lsr_offset = 5 * self.stride as usize;
        let thr_offset = 0;

        // Poll until THR empty (LSR bit 5).
        loop {
            // SAFETY: base and stride valid per constructor contract.
            let lsr: u8 = unsafe { self.base.as_ptr().add(lsr_offset).read_volatile() };
            if lsr & 0x20 != 0 {
                break;
            }
        }

        // SAFETY: THR is ready per LSR check.
        unsafe { self.base.as_ptr().add(thr_offset).write_volatile(ch) };
    }

    fn try_getchar(&self) -> Option<u8> {
        let lsr_offset = 5 * self.stride as usize;
        let rbr_offset = 0;

        // SAFETY: base and stride valid per constructor contract.
        let lsr: u8 = unsafe { self.base.as_ptr().add(lsr_offset).read_volatile() };
        if lsr & 0x01 != 0 {
            // Data Ready (LSR bit 0) — read RBR.
            // SAFETY: data is available per LSR check.
            let ch: u8 = unsafe { self.base.as_ptr().add(rbr_offset).read_volatile() };
            Some(ch)
        } else {
            None
        }
    }

    fn thre(&self) -> bool {
        let lsr_offset = 5 * self.stride as usize;
        // SAFETY: base and stride valid.
        let lsr: u8 = unsafe { self.base.as_ptr().add(lsr_offset).read_volatile() };
        lsr & 0x20 != 0
    }

    fn temt(&self) -> bool {
        let lsr_offset = 5 * self.stride as usize;
        // SAFETY: base and stride valid.
        let lsr: u8 = unsafe { self.base.as_ptr().add(lsr_offset).read_volatile() };
        lsr & 0x40 != 0
    }
}

// ── DW APB UART U32 (D1) ─────────────────────────────────────────────

/// DesignWare APB UART polling port using 32-bit MMIO access (stride 4).
///
/// On a 32-bit bus the DW APB UART requires word-sized volatile access
/// even though registers hold only 8-bit values; the hardware ignores
/// upper bytes on write.
///
/// Register offsets (in u32 words):
///   - THR (write): register 0 — Transmitter Holding Register
///   - RBR (read):  register 0 — Receiver Buffer Register
///   - LSR:         register 5 — Line Status Register
pub struct DwApbUart32PollingPort {
    /// Virtual address of UART MMIO base (cast to `*mut u32`).
    base: NonNull<u32>,
    /// Register stride in bytes (4 for DW APB UART on 32-bit bus).
    /// Kept for consistency; actual offset arithmetic uses u32 pointer
    /// addition (register index * sizeof(u32)).
    #[allow(dead_code)]
    stride: u8,
}

impl DwApbUart32PollingPort {
    /// Create from a virtual address and stride.
    ///
    /// # Safety
    ///
    /// `virt_addr` must be a valid, MMIO-mapped virtual address for
    /// a DW APB UART with DEVICE|READ|WRITE permissions.
    pub unsafe fn new(virt_addr: usize, stride: u8) -> Self {
        // SAFETY: caller guarantees the virtual address is valid and mapped.
        let base = NonNull::new(virt_addr as *mut u32).unwrap();
        Self { base, stride }
    }
}

// SAFETY: Device memory pointer, access mediated by SpinNoPreempt.
unsafe impl Send for DwApbUart32PollingPort {}

impl PollingPort for DwApbUart32PollingPort {
    fn putchar(&self, ch: u8) {
        // Register offsets in u32 units: THR=0, LSR=5.
        // Pointer arithmetic with *mut u32 means `add(5)` = 5 * 4 bytes.
        // Poll until THR empty (LSR bit 5).
        loop {
            // SAFETY: base valid per constructor contract.
            let lsr: u32 = unsafe { self.base.as_ptr().add(5).read_volatile() };
            if lsr & 0x20 != 0 {
                break;
            }
        }

        // SAFETY: THR ready per LSR check. Write low byte as u32;
        // upper bytes ignored by DW APB UART hardware.
        unsafe { self.base.as_ptr().add(0).write_volatile(ch as u32) };
    }

    fn try_getchar(&self) -> Option<u8> {
        // SAFETY: base valid per constructor contract.
        let lsr: u32 = unsafe { self.base.as_ptr().add(5).read_volatile() };
        if lsr & 0x01 != 0 {
            // Data Ready — read RBR, return low 8 bits.
            // SAFETY: data available per LSR check.
            let ch: u32 = unsafe { self.base.as_ptr().add(0).read_volatile() };
            Some(ch as u8)
        } else {
            None
        }
    }

    fn thre(&self) -> bool {
        // SAFETY: base valid.
        let lsr: u32 = unsafe { self.base.as_ptr().add(5).read_volatile() };
        lsr & 0x20 != 0
    }

    fn temt(&self) -> bool {
        // SAFETY: base valid.
        let lsr: u32 = unsafe { self.base.as_ptr().add(5).read_volatile() };
        lsr & 0x40 != 0
    }
}

// ── CONSOLE_PORT global ───────────────────────────────────────────────

/// Global console UART port.
///
/// Initialized once by [`init_console_port`] during early boot.
/// Uses `Box<dyn PollingPort + Send>` to support all UART kinds
/// without `#[cfg]` gating.
///
/// Protected by [`SpinNoPreempt`] — disables preemption but does NOT
/// mask interrupts, so it must only be used in contexts where IRQ
/// delivery does not re-enter the lock.
static CONSOLE_PORT: SpinNoPreempt<Option<Box<dyn PollingPort + Send>>> = SpinNoPreempt::new(None);

/// Initialize the global console port from a platform [`ConsoleConfig`].
///
/// Maps the UART MMIO via [`phys_to_virt`] and creates the appropriate
/// [`PollingPort`] implementation. Does NOT rewrite baud-rate divisor,
/// LCR, FCR, or MCR — the UART must already be configured (by firmware
/// or QEMU device model).
///
/// After attaching, disables IER (width-correct) since this is a
/// polling-only driver with no IRQ handler.
///
/// # Safety
///
/// Must be called exactly once, before any other thread can call
/// [`with_console_port`]. `config` must describe a valid, mapped UART.
pub unsafe fn init_console_port(config: &ConsoleConfig) {
    let virt = phys_to_virt(PhysAddr::from(config.base_paddr)).as_usize();
    // Only disable IER — do NOT rewrite divisor, LCR, FCR, or MCR.
    // SAFETY: caller guarantees virt is a valid mapped UART.
    unsafe { disable_ier(virt, config) };
    let port: Box<dyn PollingPort + Send> = match config.kind {
        ConsoleKind::Ns16550 => unsafe {
            Box::new(Ns16550U8PollingPort::new(virt, config.reg_stride))
        },
        ConsoleKind::DwApbUart => unsafe {
            Box::new(DwApbUart32PollingPort::new(virt, config.reg_stride))
        },
        _ => panic!("unsupported console kind for polling port"),
    };
    *CONSOLE_PORT.lock() = Some(port);
}

/// Width-correct IER write of zero — no divisor, LCR, FCR, or MCR access.
unsafe fn disable_ier(virt: usize, config: &ConsoleConfig) {
    match config.kind {
        ConsoleKind::Ns16550 => {
            let p = virt as *mut u8;
            // IER at offset 1 × stride, write 0 as u8.
            unsafe { p.add(config.reg_stride as usize).write_volatile(0u8) };
        }
        ConsoleKind::DwApbUart => {
            let p = virt as *mut u32;
            // IER at register 1, write 0 as u32.
            unsafe { p.add(1).write_volatile(0u32) };
        }
        _ => {}
    }
}

// ── TX lock (axplat::console::CONSOLE_LOCK → local port lock) ────

/// Acquire the global console lock then the local port lock.
///
/// Used for TX write and drain to prevent byte-level interleaving with
/// kernel log output (`ax_println!`, `info!`, etc). Lock order is always
/// `axplat::console::CONSOLE_LOCK` → `CONSOLE_PORT`.
///
/// RX (non-blocking, short) uses [`with_console_port`] without the global
/// lock to avoid blocking UART input during kernel log bursts.
pub fn with_console_port_tx<F, R>(f: F) -> R
where
    F: FnOnce(&dyn PollingPort) -> R,
{
    let _global = axplat::console::CONSOLE_LOCK.lock();
    let guard = CONSOLE_PORT.lock();
    let port = guard.as_ref().expect("console port not initialized");
    f(port.as_ref())
}

// ── Public helpers ─────────────────────────────────────────────────

/// Lock the global console port and call `f` with a `&dyn PollingPort`.
///
/// RX-only — does NOT acquire [`axplat::console::CONSOLE_LOCK`], so
/// kernel log and UART input may interleave. For TX write and drain,
/// use [`with_console_port_tx`] instead.
///
/// # Panics
///
/// Panics if [`init_console_port`] has not been called yet (port is `None`).
pub fn with_console_port<F, R>(f: F) -> R
where
    F: FnOnce(&dyn PollingPort) -> R,
{
    let guard = CONSOLE_PORT.lock();
    let port = guard.as_ref().expect("console port not initialized");
    f(port.as_ref())
}
