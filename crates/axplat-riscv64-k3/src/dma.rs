//! K3 GMAC DMA address/cache foundation (MS11 Iteration 000, task 1.1).
//!
//! Board facts (MS09/MS10 adjudications + the same-board R69 `k3_gmac`/`dma`
//! references, see the Iteration 000 Plan Context):
//! - Safe RAM window `[0x160000000, 0x2f0000000)`: DMA backing comes from the
//!   kernel allocator, which only hands out this window; the checked layout
//!   below refuses anything outside it (fail closed).
//! - DWMAC address width is read from `HW_FEATURE1[15:14]` (0/1/2 → 32/40/48
//!   bit, 3 reserved). This board's RAM is above 4 GiB, so a 32-bit
//!   capability is rejected outright — never truncated.
//! - Ring bases and the next-tail pointer must not cross a 4 GiB boundary:
//!   the driver only ever writes the low 32 bits of an address, so a
//!   crossing would make the value ambiguous.
//! - Cache block size is 64 bytes (Zicbom `cbo.*` operates per block). CPU
//!   VAs drive the CBO operations, PAs drive the device. The Zicbom
//!   encodings were verified against the shipped toolchain's assembler and
//!   disassembler (nightly-2026-02-25): `.insn i 15, 2, x0, addr, imm` with
//!   imm 0/1/2 = inval/clean/flush.
//!
//! Pure logic is host-testable (`tests/k3-gmac-host-harness.rs`); the actual
//! CBO/fence instructions only execute on the riscv64 target (V2 compile /
//! V3 board run).

/// Cache block size of the K3 cores (Zicbom operates per block).
pub const CACHE_BLOCK_BYTES: usize = 64;
/// Arena size in 4 KiB pages (18 pages = 73,728 bytes).
pub const ARENA_PAGES: usize = 18;
/// Kernel page size for the contiguous allocation.
pub const PAGE_SIZE_BYTES: usize = 4096;
/// Total arena span in bytes.
pub const ARENA_BYTES: usize = ARENA_PAGES * PAGE_SIZE_BYTES;
/// RX and TX ring slots.
pub const NUM_SLOTS: usize = 16;
/// Descriptor size: 16 bytes of fields + 48 bytes of padding.
pub const DESC_BYTES: usize = 64;
/// Packet buffer size per slot.
pub const BUFFER_BYTES: usize = 2048;

/// Arena layout (byte offsets from the arena base):
/// RX ring page 0, TX ring page 1, RX buffers pages 2–9, TX buffers
/// pages 10–17.
pub const RX_RING_OFF: usize = 0;
pub const TX_RING_OFF: usize = PAGE_SIZE_BYTES;
pub const RX_BUF_OFF: usize = 2 * PAGE_SIZE_BYTES;
pub const TX_BUF_OFF: usize = RX_BUF_OFF + 8 * PAGE_SIZE_BYTES;

/// MS09-adjudicated safe RAM window (the kernel allocator's only range).
pub const SAFE_RAM_BASE_PADDR: u64 = 0x1_6000_0000;
pub const SAFE_RAM_END_PADDR: u64 = 0x2_f000_0000;

/// DWMAC address-width capability decoded from `HW_FEATURE1[15:14]`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddrWidth {
    /// 32-bit addressing: this board's RAM is above 4 GiB, so it is always
    /// rejected before any descriptor is built.
    W32,
    /// 40-bit addressing.
    W40,
    /// 48-bit addressing.
    W48,
}

impl AddrWidth {
    /// Addressable width in bits.
    pub const fn bits(&self) -> u32 {
        match self {
            AddrWidth::W32 => 32,
            AddrWidth::W40 => 40,
            AddrWidth::W48 => 48,
        }
    }
}

/// Whether a ring of `NUM_SLOTS * DESC_BYTES` bytes starting at
/// `ring_base` — including its next-tail value (`ring_base + span`) —
/// crosses a 4 GiB boundary, which would make a low-32-bit-only tail write
/// ambiguous. Standalone so the guarantee is directly testable; inside
/// [`ArenaLayout::new`] it is defense in depth (a 4 KiB-aligned ring can
/// never cross with the current 1 KiB span).
pub const fn ring_crosses_4g(ring_base: u64) -> bool {
    let span = (NUM_SLOTS * DESC_BYTES) as u64;
    ring_base >> 32 != (ring_base + span - 1) >> 32 || ring_base >> 32 != (ring_base + span) >> 32
}

/// Checked DMA foundation failures. Every variant is a hard refusal: no
/// register is written, no descriptor is built.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DmaAddressError {
    /// Arena base is not 4 KiB aligned (PA or VA).
    Unaligned,
    /// The 18-page span overflows the address space.
    Overflow,
    /// The span is not fully inside the safe RAM window.
    OutsideSafeRam,
    /// A ring (or its next-tail value) crosses a 4 GiB boundary.
    RingCrosses4GiB,
    /// `HW_FEATURE1[15:14]` = 3 (reserved encoding).
    WidthReserved,
    /// The address does not fit the controller's address-width capability.
    AddressTooWide { pa: u64, width_bits: u32 },
    /// Slot index outside `0..NUM_SLOTS`.
    SlotOutOfRange,
    /// A cache range is empty or outside the exclusive arena span.
    CacheRangeOutside,
}

/// Decodes the controller address width from `HW_FEATURE1` (GMAC + 0x120),
/// bits [15:14]: 0 → 32, 1 → 40, 2 → 48, 3 → reserved (rejected).
pub const fn decode_addr_width(hw_feature1: u32) -> Result<AddrWidth, DmaAddressError> {
    match (hw_feature1 >> 14) & 0b11 {
        0b00 => Ok(AddrWidth::W32),
        0b01 => Ok(AddrWidth::W40),
        0b10 => Ok(AddrWidth::W48),
        _ => Err(DmaAddressError::WidthReserved),
    }
}

/// Rejects an address that the controller cannot express. There is no
/// truncation: a 32-bit-capable controller refuses this board's >4 GiB RAM.
pub const fn addr_fits_width(pa: u64, width: &AddrWidth) -> Result<(), DmaAddressError> {
    let bits = width.bits();
    // 2^64-1 upper bound saturates for 64-bit widths; the shift stays in
    // range because `bits <= 48`.
    if pa < (1u64 << bits) {
        Ok(())
    } else {
        Err(DmaAddressError::AddressTooWide { pa, width_bits: bits })
    }
}

/// Low 32 bits of a DMA address (descriptor des0 / ring-base low register).
pub const fn addr_lo32(pa: u64) -> u32 {
    pa as u32
}

/// High 32 bits of a DMA address (descriptor des1 / ring-base high register).
pub const fn addr_hi32(pa: u64) -> u32 {
    (pa >> 32) as u32
}

/// Checks that a cache operation range is non-empty and lies entirely inside
/// the exclusive arena span. Every cache call in the driver is range-checked
/// before it reaches the seam.
pub const fn check_cache_range(
    arena_va: usize,
    va: usize,
    len: usize,
) -> Result<(), DmaAddressError> {
    if len == 0 {
        return Err(DmaAddressError::CacheRangeOutside);
    }
    if va < arena_va {
        return Err(DmaAddressError::CacheRangeOutside);
    }
    match va.checked_sub(arena_va) {
        Some(off) if off + len <= ARENA_BYTES => Ok(()),
        _ => Err(DmaAddressError::CacheRangeOutside),
    }
}

/// Checked view of the exclusive 18-page arena: every accessor validates its
/// slot and the constructor validates the whole span once, before any ring
/// register is written.
#[derive(Clone, Copy, Debug)]
pub struct ArenaLayout {
    base_pa: u64,
    base_va: usize,
}

impl ArenaLayout {
    /// Validates the arena placement: 4 KiB alignment, non-overflowing span,
    /// fully inside the safe RAM window, and no ring (including its
    /// next-tail value) crossing a 4 GiB boundary.
    pub fn new(base_pa: u64, base_va: usize) -> Result<Self, DmaAddressError> {
        if base_pa % PAGE_SIZE_BYTES as u64 != 0 || base_va % PAGE_SIZE_BYTES != 0 {
            return Err(DmaAddressError::Unaligned);
        }
        let end_pa = match base_pa.checked_add(ARENA_BYTES as u64) {
            Some(v) => v,
            None => return Err(DmaAddressError::Overflow),
        };
        if base_pa < SAFE_RAM_BASE_PADDR || end_pa > SAFE_RAM_END_PADDR {
            return Err(DmaAddressError::OutsideSafeRam);
        }
        // A ring occupies 16 × 64 bytes; the next-tail value written to the
        // 32-bit tail register is ring_base + span. Base, last byte and the
        // tail value must share the same 4 GiB high bits.
        for ring_off in [RX_RING_OFF as u64, TX_RING_OFF as u64] {
            if ring_crosses_4g(base_pa + ring_off) {
                return Err(DmaAddressError::RingCrosses4GiB);
            }
        }
        Ok(ArenaLayout { base_pa, base_va })
    }

    /// Arena base PA (device view).
    pub const fn base_pa(&self) -> u64 {
        self.base_pa
    }

    /// Arena base VA (CPU view, drives CBO operations).
    pub const fn base_va(&self) -> usize {
        self.base_va
    }

    const fn slot(slot: usize) -> Result<usize, DmaAddressError> {
        if slot < NUM_SLOTS {
            Ok(slot)
        } else {
            Err(DmaAddressError::SlotOutOfRange)
        }
    }

    /// RX ring base PA.
    pub const fn rx_ring_pa(&self) -> u64 {
        self.base_pa + RX_RING_OFF as u64
    }

    /// TX ring base PA.
    pub const fn tx_ring_pa(&self) -> u64 {
        self.base_pa + TX_RING_OFF as u64
    }

    /// RX ring base VA.
    pub const fn rx_ring_va(&self) -> usize {
        self.base_va + RX_RING_OFF
    }

    /// TX ring base VA.
    pub const fn tx_ring_va(&self) -> usize {
        self.base_va + TX_RING_OFF
    }

    /// PA of RX descriptor `slot` (64-byte stride).
    pub const fn rx_desc_pa(&self, slot: usize) -> Result<u64, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_pa + RX_RING_OFF as u64 + (s * DESC_BYTES) as u64),
            Err(e) => Err(e),
        }
    }

    /// VA of RX descriptor `slot`.
    pub const fn rx_desc_va(&self, slot: usize) -> Result<usize, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_va + RX_RING_OFF + s * DESC_BYTES),
            Err(e) => Err(e),
        }
    }

    /// PA of TX descriptor `slot`.
    pub const fn tx_desc_pa(&self, slot: usize) -> Result<u64, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_pa + TX_RING_OFF as u64 + (s * DESC_BYTES) as u64),
            Err(e) => Err(e),
        }
    }

    /// VA of TX descriptor `slot`.
    pub const fn tx_desc_va(&self, slot: usize) -> Result<usize, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_va + TX_RING_OFF + s * DESC_BYTES),
            Err(e) => Err(e),
        }
    }

    /// PA of RX buffer `slot`.
    pub const fn rx_buf_pa(&self, slot: usize) -> Result<u64, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_pa + RX_BUF_OFF as u64 + (s * BUFFER_BYTES) as u64),
            Err(e) => Err(e),
        }
    }

    /// VA of RX buffer `slot`.
    pub const fn rx_buf_va(&self, slot: usize) -> Result<usize, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_va + RX_BUF_OFF + s * BUFFER_BYTES),
            Err(e) => Err(e),
        }
    }

    /// PA of TX buffer `slot`.
    pub const fn tx_buf_pa(&self, slot: usize) -> Result<u64, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_pa + TX_BUF_OFF as u64 + (s * BUFFER_BYTES) as u64),
            Err(e) => Err(e),
        }
    }

    /// VA of TX buffer `slot`.
    pub const fn tx_buf_va(&self, slot: usize) -> Result<usize, DmaAddressError> {
        match Self::slot(slot) {
            Ok(s) => Ok(self.base_va + TX_BUF_OFF + s * BUFFER_BYTES),
            Err(e) => Err(e),
        }
    }

    /// The next-tail value of the RX ring (ring base + 16 × 64 bytes): the
    /// 4 GiB-crossing check in [`Self::new`] guarantees its low-32-bit write
    /// is unambiguous.
    pub const fn rx_next_tail_pa(&self) -> Result<u64, DmaAddressError> {
        Ok(self.rx_ring_pa() + (NUM_SLOTS * DESC_BYTES) as u64)
    }
}

// ---------------------------------------------------------------------------
// riscv64 cache-block operations (board execution only; host fakes trace the
// driver's op order instead)
// ---------------------------------------------------------------------------

#[cfg(target_arch = "riscv64")]
pub mod cbo {
    /// Cache-block clean (write back dirty lines). `.insn i 15, 2, x0, addr, 1`
    /// encodes `cbo.clean` (toolchain-verified).
    pub fn block_clean(va: usize) {
        // SAFETY: cbo.clean on an address in the exclusive DMA arena; needs
        // the Zicbom extension (K3 cores).
        unsafe { core::arch::asm!(".insn i 15, 2, x0, {}, 1", in(reg) va, options(nostack)) }
    }

    /// Cache-block flush (clean + invalidate). `.insn i 15, 2, x0, addr, 2`
    /// encodes `cbo.flush` (toolchain-verified).
    pub fn block_flush(va: usize) {
        // SAFETY: cbo.flush on an address in the exclusive DMA arena.
        unsafe { core::arch::asm!(".insn i 15, 2, x0, {}, 2", in(reg) va, options(nostack)) }
    }

    /// Cache-block invalidate (drop clean lines without write-back).
    /// `.insn i 15, 2, x0, addr, 0` encodes `cbo.inval` (toolchain-verified).
    pub fn block_inval(va: usize) {
        // SAFETY: cbo.inval on an address in the exclusive DMA arena; the
        // driver only invalidates lines it has already written back or that
        // the device has completed.
        unsafe { core::arch::asm!(".insn i 15, 2, x0, {}, 0", in(reg) va, options(nostack)) }
    }

    fn for_each_block(va: usize, len: usize, op: fn(usize)) {
        debug_assert!(len > 0 && len % super::CACHE_BLOCK_BYTES == 0);
        let mut off = 0;
        while off < len {
            op(va + off);
            off += super::CACHE_BLOCK_BYTES;
        }
    }

    /// Cleans a block-granular range.
    pub fn clean_range(va: usize, len: usize) {
        for_each_block(va, len, block_clean)
    }

    /// Flushes a block-granular range.
    pub fn flush_range(va: usize, len: usize) {
        for_each_block(va, len, block_flush)
    }

    /// Invalidates a block-granular range.
    pub fn inval_range(va: usize, len: usize) {
        for_each_block(va, len, block_inval)
    }

    /// Inner-shareable read/write fence used around cache operations.
    pub fn fence_rw() {
        // SAFETY: a plain fence; no memory access.
        unsafe { core::arch::asm!("fence rw, rw", options(nostack)) }
    }

    /// Full `fence iorw, iorw` used before the DMA-publishing doorbell:
    /// ordering must include device (I/O) observations, a plain fence is
    /// not a substitute for the cache operations themselves.
    pub fn fence_iorw() {
        // SAFETY: a plain fence; no memory access.
        unsafe { core::arch::asm!("fence iorw, iorw", options(nostack)) }
    }
}
