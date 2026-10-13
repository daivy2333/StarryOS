//! K3 GMAC DMA descriptor word facts (MS11 Iteration 000, task 1.1).
//!
//! 64-byte descriptors: four 32-bit words of fields + 48 bytes of padding.
//! RX normal descriptor (DWMAC4 layout, per the Iteration 000 contract):
//! - word 0: buffer address low 32
//! - word 1: buffer address high 32
//! - word 2: control (interrupt-on-completion deliberately 0: polling RX)
//! - word 3: OWN bit31 (published last), CTXT bit30, FD bit29, LD bit28,
//!   BUF1V bit24, ES bit15, raw packet length bits [14:0] (FCS included).
//!
//! A legal completion has OWN=0, CTXT=0, FD=1, LD=1, ES=0 and a raw length
//! in 64..=1518 (≤ the 2048-byte buffer). Anything else is a classified
//! drop — the raw word 3 is always retained for the report.

/// RX descriptor word 3: ownership (1 = device-owned). Written last.
pub const RDES3_OWN: u32 = 1 << 31;
/// RX descriptor word 3 bit30: interrupt-on-completion when PUBLISHED by
/// software; the same bit reads back as CTXT on the device's completion
/// writeback (which is 0 for every normal descriptor). MS12 async mode sets
/// it at publish time only; the completion decode keeps rejecting CTXT != 0
/// (the IOC value never bypasses [`decode_rx`]).
pub const RDES3_IOC: u32 = 1 << 30;
/// RX descriptor word 3: context descriptor marker (legal completions: 0).
pub const RDES3_CTXT: u32 = 1 << 30;
/// RX descriptor word 3: first descriptor of the frame.
pub const RDES3_FD: u32 = 1 << 29;
/// RX descriptor word 3: last descriptor of the frame.
pub const RDES3_LD: u32 = 1 << 28;
/// RX descriptor word 3: buffer-1 address valid (set at publish).
pub const RDES3_BUF1V: u32 = 1 << 24;
/// RX descriptor word 3: error summary.
pub const RDES3_ES: u32 = 1 << 15;
/// RX descriptor word 3: raw packet length mask (bits [14:0], FCS included).
pub const RDES3_LEN_MASK: u32 = 0x7fff;

/// Legal raw length window: 64..=1518 (and always ≤ the buffer size).
pub const RX_RAW_LEN_MIN: u32 = 64;
pub const RX_RAW_LEN_MAX: u32 = 1518;
/// FCS length stripped after the copy (the MAC has no checksum offload and
/// no ACS stripping; the 4-byte FCS is removed in software).
pub const FCS_LEN: usize = 4;

// ---------------------------------------------------------------------------
// TX normal descriptor (task 2.1). Same 64-byte shape as RX; the MAC
// generates the FCS, so every TX length field excludes it.
// ---------------------------------------------------------------------------

/// TX descriptor word 2: frame length bits [13:0]; IOC bit31 stays 0
/// (polling TX, no completion interrupts).
pub const TDES2_IOC: u32 = 1 << 31;
pub const TDES2_LEN_MASK: u32 = 0x3fff;
/// TX descriptor word 3: ownership (1 = device-owned). Written last.
pub const TDES3_OWN: u32 = 1 << 31;
/// TX descriptor word 3: first descriptor of the frame (single-buffer ring:
/// always set).
pub const TDES3_FD: u32 = 1 << 29;
/// TX descriptor word 3: last descriptor of the frame (single-buffer ring:
/// always set).
pub const TDES3_LD: u32 = 1 << 28;
/// TX descriptor word 3: error summary (device-reported on completion).
pub const TDES3_ES: u32 = 1 << 15;
/// TX descriptor word 3: frame length bits [14:0] (FCS excluded).
pub const TDES3_LEN_MASK: u32 = 0x7fff;

/// Drop classification of a non-deliverable descriptor. The raw word 3
/// travels with every variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxDrop {
    /// ES set: the device flagged an error (raw status carries the detail).
    ErrorSummary,
    /// Context descriptor: not a packet.
    ContextDescriptor,
    /// FD/LD not both set: partial or spanning frame (single-buffer ring).
    NotComplete { fd: bool, ld: bool },
    /// Raw length outside the legal window (runt below 64, oversized above
    /// 1518).
    LengthOutOfBounds { len: u32 },
}

/// Outcome of decoding a device-owned RX descriptor word 3.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RxCompletion {
    /// Legal frame: raw length (FCS included) with the unmodified word 3.
    Frame { raw_len: u16, w3: u32 },
    /// Classified drop with the unmodified word 3.
    Drop { reason: RxDrop, w3: u32 },
}

/// Decodes a device-owned (OWN=0) RX descriptor word 3. No fake success:
/// every malformed shape is classified, never delivered.
pub const fn decode_rx(w3: u32) -> RxCompletion {
    if w3 & RDES3_CTXT != 0 {
        return RxCompletion::Drop { reason: RxDrop::ContextDescriptor, w3 };
    }
    if w3 & RDES3_ES != 0 {
        return RxCompletion::Drop { reason: RxDrop::ErrorSummary, w3 };
    }
    let fd = w3 & RDES3_FD != 0;
    let ld = w3 & RDES3_LD != 0;
    if !fd || !ld {
        return RxCompletion::Drop { reason: RxDrop::NotComplete { fd, ld }, w3 };
    }
    let len = w3 & RDES3_LEN_MASK;
    if len < RX_RAW_LEN_MIN || len > RX_RAW_LEN_MAX {
        return RxCompletion::Drop { reason: RxDrop::LengthOutOfBounds { len }, w3 };
    }
    RxCompletion::Frame { raw_len: len as u16, w3 }
}
