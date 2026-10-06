//! K3 GMAC register map facts (MS11 Iteration 000, task 1.1).
//!
//! Offsets are base-relative (the board glue binds `mac_probe::MAC_BASE_PADDR`,
//! inside the DT-declared identity-mapped `0xcac82000/0x2000` window).
//! Per-SoC divergences from the generic Synopsys layout are deliberate
//! (R69 same-board reference, Iteration 000 Plan Context): the MTL queue 0
//! registers sit at `+0xd00`/`+0xd30` (not the generic `0xc00` window), and
//! `CHAN_CTL` carries `DSL` at bit 18.

// ---------------------------------------------------------------------------
// MAC (offset 0x000 window)
// ---------------------------------------------------------------------------

/// MAC configuration: RE bit0, TE bit1, DM bit13, FES bit14, PS bit15.
pub const MAC_CONFIG: u32 = 0x000;
pub const MAC_CONFIG_RE: u32 = 1 << 0;
pub const MAC_CONFIG_TE: u32 = 1 << 1;
pub const MAC_CONFIG_DM: u32 = 1 << 13;
pub const MAC_CONFIG_FES: u32 = 1 << 14;
pub const MAC_CONFIG_PS: u32 = 1 << 15;

/// Packet filter: PR (promiscuous) bit0.
pub const MAC_PKT_FILTER: u32 = 0x008;
pub const MAC_FILTER_PR: u32 = 1 << 0;

/// Hardware feature register 1: address width in bits [15:14].
pub const MAC_HW_FEATURE1: u32 = 0x120;

/// MTL RX queue 0 → DMA channel mapping (queue 0 → channel 0 = 2b10).
pub const MAC_RXQ0_MAP: u32 = 0x0a0;
pub const MAC_RXQ0_MAP_CH0: u32 = 0b10;

/// MAC address 0 high (AE bit31 | MAC[47:32]).
pub const MAC_ADDR0_HIGH: u32 = 0x300;
/// MAC address 0 low (MAC[31:0]).
pub const MAC_ADDR0_LOW: u32 = 0x304;
pub const MAC_ADDR_AE: u32 = 1 << 31;

// ---------------------------------------------------------------------------
// MTL (queue 0 at the per-SoC +0xd00 window, R69)
// ---------------------------------------------------------------------------

/// MTL TX queue 0 operation: FTQ bit0 (kept clear), TSF bit1, TXQEN bit3,
/// TQS in bits [19:16].
pub const MTL_TXQ0_OP: u32 = 0xd00;
pub const MTL_TXQ_TSF: u32 = 1 << 1;
pub const MTL_TXQ_TXQEN: u32 = 1 << 3;
pub const MTL_TXQ_TQS_31: u32 = 31 << 16;

/// MTL RX queue 0 operation: RSF bit5, DIS_TCP_EF bit6, RQS in bits [23:20];
/// flow control (EHFC) deliberately off.
pub const MTL_RXQ0_OP: u32 = 0xd30;
pub const MTL_RXQ_RSF: u32 = 1 << 5;
pub const MTL_RXQ_DIS_TCP_EF: u32 = 1 << 6;
pub const MTL_RXQ_RQS_31: u32 = 31 << 20;

// ---------------------------------------------------------------------------
// DMA (0x1000 window)
// ---------------------------------------------------------------------------

/// System bus mode: BLEN8 bit2, BLEN16 bit3, EAME bit11, RD_OSR_LMT bits
/// [19:16] (value 2).
pub const DMA_SYS_BUS_MODE: u32 = 0x1004;
pub const DMA_SYS_BUS_BLEN8: u32 = 1 << 2;
pub const DMA_SYS_BUS_BLEN16: u32 = 1 << 3;
pub const DMA_SYS_BUS_EAME: u32 = 1 << 11;
pub const DMA_SYS_BUS_RD_OSR_LMT_2: u32 = 2 << 16;

/// Soft reset: SWR bit0, self-clearing; bounded by real monotonic time.
pub const DMA_MODE: u32 = 0x1000;
pub const DMA_MODE_SWR: u32 = 1 << 0;

/// Channel 0 control: PBLX8 bit16, DSL at bit 18 (value 6, per-SoC position).
pub const DMA_CHAN_CTL: u32 = 0x1100;
pub const DMA_CHAN_PBLX8: u32 = 1 << 16;
pub const DMA_CHAN_DSL_6: u32 = 6 << 18;

/// Channel TX control: ST bit0 (kept 0), OSP bit4, TXPBL in bits [21:16].
pub const DMA_TX_CTL: u32 = 0x1104;
pub const DMA_TX_CTL_ST: u32 = 1 << 0;
pub const DMA_TX_CTL_OSP: u32 = 1 << 4;
pub const DMA_TX_CTL_PBL_8: u32 = 8 << 16;

/// Channel RX control: SR bit0, RBSZ in bits [14:1], RXRBL/PBL in bits
/// [21:16].
pub const DMA_RX_CTL: u32 = 0x1108;
pub const DMA_RX_CTL_SR: u32 = 1 << 0;
pub const DMA_RX_CTL_RBSZ_2048: u32 = 2048 << 1;
pub const DMA_RX_CTL_PBL_8: u32 = 8 << 16;

/// TX descriptor ring base (high, low). The TX data plane stays off in
/// Iteration 000; only the base registers of the cleared empty ring are set.
pub const DMA_TX_DESC_HL: u32 = 0x1110;
pub const DMA_TX_DESC_LL: u32 = 0x1114;
/// RX descriptor ring base (high, low) — high word written first.
pub const DMA_RX_DESC_HL: u32 = 0x1118;
pub const DMA_RX_DESC_LL: u32 = 0x111c;
/// RX descriptor tail pointer (doorbell; low 32 bits only).
pub const DMA_RX_TAIL: u32 = 0x1128;
/// TX descriptor tail pointer (doorbell; low 32 bits only). R69 same-board
/// reference: `dma_chan_tx_end` = channel base (0x1100) + 0x20.
pub const DMA_TX_TAIL: u32 = 0x1120;
/// Ring lengths (N-1 encoding): 15 slots.
pub const DMA_TX_RINGLEN: u32 = 0x112c;
pub const DMA_RX_RINGLEN: u32 = 0x1130;
pub const DMA_RINGLEN_15: u32 = 15;

/// Per-channel interrupt enable (kept 0: polling RX only).
pub const DMA_CHAN_IRQ_EN: u32 = 0x1134;

/// Per-channel status (W1C per consumed bit; never RMW the whole word).
pub const DMA_CHAN_STATUS: u32 = 0x1160;
/// Transmit buffer unavailable (capacity/fetch event).
pub const DMA_STATUS_TBU: u32 = 1 << 2;
/// Receive buffer unavailable (capacity event; refill and re-doorbell).
pub const DMA_STATUS_RBU: u32 = 1 << 7;
/// Receive interrupt (completion summary bit).
pub const DMA_STATUS_RI: u32 = 1 << 6;
/// Transmit interrupt (completion summary bit).
pub const DMA_STATUS_TI: u32 = 1 << 0;
/// Fatal bus error (fault).
pub const DMA_STATUS_FBE: u32 = 1 << 12;

// ---------------------------------------------------------------------------
// APMU (board glue base-relative fact, R69: `apmu + 0x3f0`)
// ---------------------------------------------------------------------------

/// APMU EMAC delay control: TX enable bit16 with phase [31:24], RX enable
/// bit0 with phase [15:8]. The RMW keeps every other bit.
pub const APMU_EBUS_DELAY_TX_EN: u32 = 1 << 16;
pub const APMU_EBUS_DELAY_RX_EN: u32 = 1 << 0;
pub const APMU_EBUS_DELAY_TX_PHASE: u32 = 0x47 << 24;
pub const APMU_EBUS_DELAY_RX_PHASE: u32 = 0x53 << 8;
