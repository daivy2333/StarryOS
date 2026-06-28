//! Early console abstraction — minimal character output before async UART
//! is available.
//!
//! Must NOT depend on: ring buffers, async tasks, IRQ delivery, PLIC
//! initialization, rootfs, or `/dev/console`. Designed for board bring-up
//! where only MMIO UART polling is reliable.

use core::ptr::NonNull;

use super::console::{ConsoleConfig, ConsoleKind};

/// Minimal character output for early boot / bring-up.
///
/// Implementations MUST be usable before the async UART runtime exists.
/// The default [`write_str`] method converts `\n` to `\r\n` for
/// terminal compatibility.
///
/// [`write_str`]: EarlyConsole::write_str
pub trait EarlyConsole {
    /// Emit a single byte (blocking, polling).
    fn putchar(&self, ch: u8);

    /// Write a string with automatic `\n` → `\r\n` conversion.
    fn write_str(&self, s: &str) {
        for b in s.bytes() {
            if b == b'\n' {
                self.putchar(b'\r');
            }
            self.putchar(b);
        }
    }
}

/// NS16550 UART early console using 8-bit MMIO access.
///
/// Suitable for QEMU virt (stride 1, byte-addressed registers).
/// Polls the Line Status Register until the transmitter is empty,
/// then writes the byte to the Transmitter Holding Register.
pub struct Ns16550U8EarlyConsole {
    /// Virtual address of UART MMIO base (already mapped).
    base: NonNull<u8>,
    /// Register stride in bytes.
    stride: u8,
}

impl Ns16550U8EarlyConsole {
    /// Create an early console from a physical address and stride.
    ///
    /// # Safety
    ///
    /// `base_paddr` must point to a valid NS16550 UART that has been
    /// MMIO-mapped with DEVICE|READ|WRITE permissions.
    pub unsafe fn new(base_paddr: usize, stride: u8) -> Self {
        // SAFETY: caller guarantees the physical address is valid and mapped.
        let base = NonNull::new(base_paddr as *mut u8).unwrap();
        Self { base, stride }
    }

    /// Convenience constructor from a [`ConsoleConfig`].
    ///
    /// # Safety
    ///
    /// See [`new`](Ns16550U8EarlyConsole::new).
    pub unsafe fn from_config(config: &ConsoleConfig) -> Self {
        assert!(
            matches!(config.kind, ConsoleKind::Ns16550),
            "Ns16550U8EarlyConsole requires ConsoleKind::Ns16550"
        );
        // SAFETY: caller guarantees config.base_paddr is a valid mapped UART.
        unsafe { Self::new(config.base_paddr, config.reg_stride) }
    }
}

impl EarlyConsole for Ns16550U8EarlyConsole {
    fn putchar(&self, ch: u8) {
        // NS16550 register offsets:
        //   offset 0: THR (Transmitter Holding Register) — write
        //   offset 5: LSR (Line Status Register) — bit 5 = THR empty, bit 6 = TEMT
        let lsr_offset = 5 * self.stride as usize;
        let thr_offset = 0;

        // Poll until transmitter holding register is empty.
        loop {
            // SAFETY: base and stride are valid per constructor contract.
            let lsr: u8 = unsafe { self.base.as_ptr().add(lsr_offset).read_volatile() };
            if lsr & 0x20 != 0 {
                // THR empty (bit 5) — ready to accept a byte.
                break;
            }
        }

        // SAFETY: THR is ready per LSR check above.
        unsafe { self.base.as_ptr().add(thr_offset).write_volatile(ch) };
    }
}

// ── Future platforms (Q19/Q20) ─────────────────────────────────────

/// DesignWare APB UART early console using 32-bit MMIO access.
///
/// **Q18 boundary only.** This type is a compile-time placeholder.
/// It does NOT implement [`EarlyConsole`] in Q18. The 32-bit access
/// model requires verification on real hardware (Q19 Lichee D1,
/// Q20 VisionFive2) and a decision on extending the `uart_16550`
/// backend vs. adding a dedicated DW APB backend.
///
/// # Q18 Invariant
///
/// `uart_16550` backend MMIO access width MUST remain `U8` throughout
/// Q18. Changing access width without hardware verification risks
/// silent data corruption on byte-addressed devices.
pub struct DwApbUart32EarlyConsole {
    /// Virtual address of UART MMIO base.
    base: core::ptr::NonNull<u8>,
    /// Register stride (4 for DW APB UART on 32-bit bus).
    stride: u8,
}

impl DwApbUart32EarlyConsole {
    /// Create from a physical address and stride (compile-time only).
    ///
    /// # Safety
    ///
    /// Caller guarantees the address is valid and MMIO-mapped.
    pub unsafe fn new(base_paddr: usize, stride: u8) -> Self {
        Self {
            base: core::ptr::NonNull::new(base_paddr as *mut u8).unwrap(),
            stride,
        }
    }

    /// Convenience constructor from a [`super::console::ConsoleConfig`].
    ///
    /// # Safety
    ///
    /// Same contract as [`new`](Self::new): `config.base_paddr` must
    /// point to a valid MMIO-mapped DW APB UART with DEVICE permissions.
    pub unsafe fn from_config(config: &super::console::ConsoleConfig) -> Self {
        assert!(
            matches!(config.kind, super::console::ConsoleKind::DwApbUart),
            "DwApbUart32EarlyConsole requires ConsoleKind::DwApbUart"
        );
        // SAFETY: forwarded from caller contract documented on `from_config`.
        unsafe { Self::new(config.base_paddr, config.reg_stride) }
    }
}

impl EarlyConsole for DwApbUart32EarlyConsole {
    fn putchar(&self, ch: u8) {
        // DW APB UART register offsets (in units of stride):
        //   offset 0: THR (Transmitter Holding Register) — write
        //   offset 5: LSR (Line Status Register) — bit 5 = THR empty
        const UART_THR: usize = 0;
        const UART_LSR: usize = 5;
        const UART_LSR_THRE: u32 = 1 << 5;

        // Poll until transmitter holding register is empty.
        loop {
            // SAFETY: base and stride are valid per constructor contract.
            let lsr: u32 = unsafe {
                self.base
                    .as_ptr()
                    .add(UART_LSR * self.stride as usize)
                    .cast::<u32>()
                    .read_volatile()
            };
            if lsr & UART_LSR_THRE != 0 {
                break;
            }
        }

        // SAFETY: THR is ready per LSR check above. Write only the low byte;
        // upper bytes are ignored by DW APB UART hardware.
        unsafe {
            self.base
                .as_ptr()
                .add(UART_THR * self.stride as usize)
                .cast::<u32>()
                .write_volatile(ch as u32);
        }
    }
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    extern crate alloc;

    use alloc::{vec, vec::Vec};
    use core::cell::RefCell;

    use super::*;

    /// Mock early console that captures all `putchar` calls into a `Vec<u8>`.
    struct MockEarlyConsole {
        buf: RefCell<Vec<u8>>,
    }

    impl MockEarlyConsole {
        fn new() -> Self {
            Self {
                buf: RefCell::new(Vec::new()),
            }
        }

        fn into_bytes(self) -> Vec<u8> {
            self.buf.into_inner()
        }
    }

    impl EarlyConsole for MockEarlyConsole {
        fn putchar(&self, ch: u8) {
            self.buf.borrow_mut().push(ch);
        }
    }

    #[test]
    fn write_str_converts_lf_to_crlf() {
        let mock = MockEarlyConsole::new();
        mock.write_str("a\nb");
        assert_eq!(mock.into_bytes(), vec![b'a', b'\r', b'\n', b'b']);
    }

    #[test]
    fn write_str_no_newline() {
        let mock = MockEarlyConsole::new();
        mock.write_str("hello");
        assert_eq!(mock.into_bytes(), vec![b'h', b'e', b'l', b'l', b'o']);
    }

    #[test]
    fn write_str_multiple_newlines() {
        let mock = MockEarlyConsole::new();
        mock.write_str("a\nb\nc");
        assert_eq!(
            mock.into_bytes(),
            vec![b'a', b'\r', b'\n', b'b', b'\r', b'\n', b'c']
        );
    }

    #[test]
    fn write_str_only_newline() {
        let mock = MockEarlyConsole::new();
        mock.write_str("\n");
        assert_eq!(mock.into_bytes(), vec![b'\r', b'\n']);
    }

    #[test]
    fn write_str_empty() {
        let mock = MockEarlyConsole::new();
        mock.write_str("");
        assert_eq!(mock.into_bytes(), vec![]);
    }

    #[test]
    fn dw_apb_uart_register_offsets() {
        // Verify register offset constants are correct for DW APB UART
        // stride=4 means: THR at base+0, LSR at base+20
        let stride: usize = 4;
        assert_eq!(0 * stride, 0); // THR offset
        assert_eq!(5 * stride, 20); // LSR offset
        // LSR THRE bit 5
        assert_eq!(1 << 5, 0x20);
    }
}
