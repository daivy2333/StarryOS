//! Console UART configuration — kind, base address, IRQ, MMIO parameters.
//!
//! Separates register stride from MMIO access width because some platforms
//! (D1, VisionFive2) need stride=4 but also require 32-bit volatile access,
//! while QEMU NS16550 uses stride=1 with byte access.

/// Console UART hardware configuration.
pub struct ConsoleConfig {
    /// UART device kind (NS16550, DW APB UART, etc.).
    pub kind: ConsoleKind,
    /// UART MMIO base physical address.
    pub base_paddr: usize,
    /// UART interrupt line number (`None` for polling-only early console).
    pub irq: Option<usize>,
    /// Register stride in bytes (1 for NS16550, 4 for DW APB UART with 32-bit bus).
    pub reg_stride: u8,
    /// MMIO access width — distinct from `reg_stride`.
    ///
    /// DW APB UART on a 32-bit bus needs `U32` access even though registers
    /// are only 8-bit values; the hardware ignores upper bytes on write.
    pub reg_width: MmioAccessWidth,
    /// Baud rate.
    pub baud: u32,
}

/// UART device kind.
pub enum ConsoleKind {
    /// NS16550-compatible UART (QEMU virt, standard PC).
    Ns16550,
    /// DesignWare APB UART (Allwinner D1, StarFive JH7110).
    DwApbUart,
    /// SBI console (no hardware UART access needed).
    SbiConsole,
}

/// MMIO register access width.
///
/// MUST remain separate from register stride. Stride=4 with width=U32
/// is common on 32-bit memory-mapped UARTs; stride alone does not imply
/// the correct volatile access width.
pub enum MmioAccessWidth {
    /// 8-bit access (byte-addressed MMIO).
    U8,
    /// 32-bit access (word-addressed MMIO, upper bytes ignored by hardware).
    U32,
}
