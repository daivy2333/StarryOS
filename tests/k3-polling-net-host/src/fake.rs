//! Fake K3 board, DMA wire and real-smoltcp memory peer for the polling-net
//! host tests (MS11 Iteration 002).
//!
//! `FakeEnv` is the deterministic register/clock/MDIO board fake (same model
//! as the rustc-direct `k3-gmac-host-harness.rs`): the production driver
//! performs real volatile descriptor/buffer accesses against a leaked
//! 18-page arena. `FakeWire` is the test-side view of the same arena — the
//! device half of the DMA: it completes device-owned TX descriptors (the
//! frame travels out) and writes completed frames into device-owned RX
//! slots. `PeerHost` is a second real smoltcp `Interface` over an in-memory
//! device — the "PC" side of the wire.

use std::collections::VecDeque;

use smoltcp::{
    iface::{Config, Interface, SocketHandle, SocketSet, SocketStorage},
    phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken},
    socket::{icmp, tcp, udp},
    time::Instant as SmolInstant,
    wire::{HardwareAddress, IpAddress, IpCidr, Ipv4Address},
};

use crate::k3_dma::{
    ARENA_BYTES, DESC_BYTES, NUM_SLOTS, RX_BUF_OFF, RX_RING_OFF, TX_BUF_OFF, TX_RING_OFF,
};
use crate::k3_gmac::{
    ArenaBacking, ArenaGuard, Gmac, GmacEnv, RDES3_FD, RDES3_LD, RDES3_OWN, TDES3_LEN_MASK,
    TDES3_OWN,
};
use crate::k3_network_probe::Stage11Env;
use crate::mac_probe;
use crate::phy_probe::ProbeEnv;

/// Board addresses on the test wire (matches the kernel session constants).
pub const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];
pub const BOARD_IP: [u8; 4] = [192, 168, 137, 2];
/// Peer ("PC") addresses.
pub const PEER_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x02];
pub const PEER_IP: [u8; 4] = [192, 168, 137, 1];

const GMAC_BASE_PADDR: u64 = mac_probe::MAC_BASE_PADDR;
const MDIO_ADDR_PADDR: u64 = GMAC_BASE_PADDR + 0x200;
const MDIO_DATA_PADDR: u64 = GMAC_BASE_PADDR + 0x204;
const APMU_EBUS_DELAY_PADDR: u64 = 0xd428_2800 + 0x3f0;
const MAC_HW_FEATURE1: u32 = 0x120;
const DMA_MODE: u32 = 0x1000;
/// BMSR autonegotiation-complete (bit5).
const BMSR_ANEG_COMPLETE: u16 = 1 << 5;

// ---------------------------------------------------------------------------
// Fake board environment
// ---------------------------------------------------------------------------

pub struct FakeEnv {
    pub(crate) clock_ns: u64,
    /// Every board access in issue order (masks/registers/MDIO/cache) — the
    /// zero-GMAC-access witness for the stage 13 gate. Shared so tests can
    /// keep a clone after the env is moved into a device or stage entry.
    pub(crate) access_log: std::rc::Rc<std::cell::RefCell<Vec<&'static str>>>,
    regs: std::collections::BTreeMap<u32, u32>,
    arena_va: usize,
    arena_pa: u64,
    mdio_page: u16,
    mdio_regs: [u16; 32],
    mdio_pending: Option<(u32, u32, u64)>,
    mdio_data: u32,
    swr_ready_at: u64,
    apmu_delay: u32,
}

impl Clone for FakeEnv {
    fn clone(&self) -> Self {
        FakeEnv {
            clock_ns: self.clock_ns,
            access_log: self.access_log.clone(),
            regs: self.regs.clone(),
            arena_va: self.arena_va,
            arena_pa: self.arena_pa,
            mdio_page: self.mdio_page,
            mdio_regs: self.mdio_regs,
            mdio_pending: self.mdio_pending,
            mdio_data: self.mdio_data,
            swr_ready_at: self.swr_ready_at,
            apmu_delay: self.apmu_delay,
        }
    }
}

/// Board bundle: the fake env plus its device-half wire over one arena.
pub struct Board {
    pub env: FakeEnv,
    pub wire: FakeWire,
}

#[repr(align(4096))]
struct AlignedArena([u8; ARENA_BYTES]);

fn fresh_arena() -> (usize, u64, &'static mut [u8; ARENA_BYTES]) {
    let aligned = Box::leak(Box::new(AlignedArena([0u8; ARENA_BYTES])));
    let slice = &mut aligned.0;
    (slice.as_ptr() as usize, 0x2_0000_0000, slice)
}

impl FakeEnv {
    pub fn new() -> (Self, &'static mut [u8; ARENA_BYTES]) {
        let (va, pa, slice) = fresh_arena();
        (
            FakeEnv {
                clock_ns: 0,
                access_log: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
                regs: std::collections::BTreeMap::from([(MAC_HW_FEATURE1, 0b10 << 14)]),
                arena_va: va,
                arena_pa: pa,
                mdio_page: 0,
                mdio_regs: [0; 32],
                mdio_pending: None,
                mdio_data: 0,
                swr_ready_at: 0,
                apmu_delay: 0,
            },
            slice,
        )
    }

    /// Snapshot of every board access so far (the env may have been moved).
    pub fn log_snapshot(&self) -> Vec<&'static str> {
        self.access_log.borrow().clone()
    }

    /// A ready-to-run board: fake env plus the device-half wire.
    pub fn board() -> Board {
        let (env, arena) = FakeEnv::new();
        let pa = env.arena_pa;
        Board { env, wire: FakeWire::new(arena, pa) }
    }

    /// Post-stage8 happy-path PHY state: link up, AN complete, 1000FD.
    pub fn set_phy_link_1000fd(&mut self) {
        self.mdio_regs[1] = 0x0004 | BMSR_ANEG_COMPLETE;
        self.mdio_regs[4] = 0x01e1;
        self.mdio_regs[5] = 0x41e1;
        self.mdio_regs[9] = 0x0300;
        self.mdio_regs[10] = 0x0800;
    }

    /// Sets a GMAC register value the fake controller reports back.
    pub fn set_reg(&mut self, offset: u32, value: u32) {
        self.regs.insert(offset, value);
    }

    fn serve_phy_read(&mut self, reg: u32) -> u16 {
        if reg == crate::phy_probe::PHY_PAGE_SEL_REG {
            return self.mdio_page;
        }
        if self.mdio_page == 0 {
            return self.mdio_regs[reg as usize];
        }
        0
    }
}

impl Stage11Env for FakeEnv {
    fn mask_source(&mut self, source: usize) {
        let tag = match source {
            60 => "mask:60",
            133 => "mask:133",
            277 => "mask:277",
            other => panic!("unexpected mask source {other}"),
        };
        self.access_log.borrow_mut().push(tag);
    }
}

impl Board {
    /// A driver initialized to Running with TX enabled over a fresh guard.
    pub fn ready_driver(&mut self) -> Gmac<LeakBacking> {
        self.env.set_phy_link_1000fd();
        let mut driver = Gmac::new(self.wire.arena_guard()).expect("arena placement");
        driver.init_rx(&mut self.env).expect("init_rx");
        driver.enable_tx(&mut self.env);
        driver
    }
}

impl ProbeEnv for FakeEnv {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        let tag = if paddr == MDIO_ADDR_PADDR || paddr == MDIO_DATA_PADDR { "mdio" } else { "mmio" };
        self.access_log.borrow_mut().push(tag);
        match paddr {
            MDIO_ADDR_PADDR => {
                if let Some((reg, goc, ready)) = self.mdio_pending {
                    if self.clock_ns >= ready {
                        if goc == crate::phy_probe::GOC_READ >> 2 {
                            self.mdio_data = self.serve_phy_read(reg) as u32;
                        }
                        self.mdio_pending = None;
                    }
                }
                if self.mdio_pending.is_some() { 1 } else { 0 }
            }
            MDIO_DATA_PADDR => self.mdio_data,
            APMU_EBUS_DELAY_PADDR => self.apmu_delay,
            _ => 0,
        }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        let tag = if paddr == MDIO_ADDR_PADDR || paddr == MDIO_DATA_PADDR { "mdio" } else { "mmio" };
        self.access_log.borrow_mut().push(tag);
        match paddr {
            MDIO_DATA_PADDR => self.mdio_data = value,
            MDIO_ADDR_PADDR => {
                if value & 1 == 0 {
                    return;
                }
                let goc = (value >> 2) & 3;
                let reg = (value >> 16) & 0x1f;
                self.mdio_pending = Some((reg, goc, self.clock_ns));
            }
            APMU_EBUS_DELAY_PADDR => self.apmu_delay = value,
            _ => {}
        }
    }

    fn now_nanos(&self) -> u64 {
        self.clock_ns
    }

    fn wait_nanos(&mut self, nanos: u64) {
        self.clock_ns += nanos;
    }
}

impl GmacEnv for FakeEnv {
    fn reg_read(&mut self, offset: u32) -> u32 {
        self.access_log.borrow_mut().push("reg");
        if offset == DMA_MODE {
            return if self.clock_ns >= self.swr_ready_at { 0 } else { 1 };
        }
        self.regs.get(&offset).copied().unwrap_or(0)
    }

    fn reg_write(&mut self, offset: u32, value: u32) {
        self.access_log.borrow_mut().push("reg");
        self.regs.insert(offset, value);
        if offset == DMA_MODE && value & 1 != 0 {
            self.swr_ready_at = self.clock_ns;
        }
    }

    fn cache_flush(&mut self, _va: usize, _len: usize) {
        self.access_log.borrow_mut().push("cache");
    }

    fn cache_clean(&mut self, _va: usize, _len: usize) {
        self.access_log.borrow_mut().push("cache");
    }

    fn cache_invalidate(&mut self, _va: usize, _len: usize) {
        self.access_log.borrow_mut().push("cache");
    }

    fn fence_rw(&mut self) {
        self.access_log.borrow_mut().push("fence_rw");
    }

    fn fence_iorw(&mut self) {
        self.access_log.borrow_mut().push("fence_iorw");
    }
}

// ---------------------------------------------------------------------------
// Arena backing + wire (device half of the DMA)
// ---------------------------------------------------------------------------

pub struct LeakBacking {
    va: usize,
    pa: u64,
}

impl ArenaBacking for LeakBacking {
    fn base_va(&self) -> usize {
        self.va
    }
    fn base_pa(&self) -> u64 {
        self.pa
    }
}

/// Test-side device half: injects RX completions and drains TX into wire
/// frames by touching the same arena memory the driver accesses.
pub struct FakeWire {
    arena_va: usize,
    arena_pa: u64,
    arena: &'static mut [u8; ARENA_BYTES],
    /// The device's RX fetch position: completions are DMA-written in ring
    /// order, so injection targets this slot and then advances.
    rx_fetch: usize,
}

impl FakeWire {
    pub fn new(arena: &'static mut [u8; ARENA_BYTES], arena_pa: u64) -> Self {
        FakeWire { arena_va: arena.as_ptr() as usize, arena_pa, arena, rx_fetch: 0 }
    }

    pub fn arena_pa(&self) -> u64 {
        self.arena_pa
    }

    fn rx_desc_word(&self, slot: usize, word: usize) -> u32 {
        let off = RX_RING_OFF + slot * DESC_BYTES + word * 4;
        u32::from_le_bytes(self.arena[off..off + 4].try_into().unwrap())
    }

    fn set_rx_desc_word(&mut self, slot: usize, word: usize, v: u32) {
        let off = RX_RING_OFF + slot * DESC_BYTES + word * 4;
        self.arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    fn tx_desc_word(&self, slot: usize, word: usize) -> u32 {
        let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
        u32::from_le_bytes(self.arena[off..off + 4].try_into().unwrap())
    }

    fn set_tx_desc_word(&mut self, slot: usize, word: usize, v: u32) {
        let off = TX_RING_OFF + slot * DESC_BYTES + word * 4;
        self.arena[off..off + 4].copy_from_slice(&v.to_le_bytes());
    }

    /// Device-side RX completion: the frame is DMA-written into the fetch
    /// slot in ring order (zero-padded to the 60-byte minimum; the raw
    /// descriptor length includes the 4-byte FCS the MAC appends).
    pub fn inject_rx_frame(&mut self, frame: &[u8]) {
        assert!(frame.len() <= 1514, "frame longer than the driver MTU");
        let slot = self.rx_fetch;
        assert!(
            self.rx_desc_word(slot, 3) & RDES3_OWN != 0,
            "RX fetch slot {slot} is not device-owned (driver fell behind)"
        );
        let wire_len = frame.len().max(60);
        let off = RX_BUF_OFF + slot * crate::k3_dma::BUFFER_BYTES;
        self.arena[off..off + frame.len()].copy_from_slice(frame);
        let raw_len = (wire_len + 4) as u32;
        self.set_rx_desc_word(slot, 3, RDES3_FD | RDES3_LD | raw_len);
        self.rx_fetch = (slot + 1) % NUM_SLOTS;
    }

    /// Device-side TX completion: every OWN-set TX descriptor is "sent" and
    /// cleared; the wire frames (length-field bytes, FCS never included) are
    /// returned in slot order.
    pub fn drain_board_tx(&mut self) -> Vec<Vec<u8>> {
        let mut out = Vec::new();
        for slot in 0..NUM_SLOTS {
            let w3 = self.tx_desc_word(slot, 3);
            if w3 & TDES3_OWN != 0 {
                let len = (w3 & TDES3_LEN_MASK) as usize;
                let off = TX_BUF_OFF + slot * crate::k3_dma::BUFFER_BYTES;
                out.push(self.arena[off..off + len].to_vec());
                self.set_tx_desc_word(slot, 3, w3 & !TDES3_OWN);
            }
        }
        out
    }

    /// A fresh published-guard driver over this wire's arena.
    pub fn backing(&self) -> LeakBacking {
        LeakBacking { va: self.arena_va, pa: self.arena_pa }
    }

    /// ArenaGuard over this wire's arena (for the stage 13 core entry).
    pub fn arena_guard(&self) -> ArenaGuard<LeakBacking> {
        ArenaGuard::new(self.backing())
    }
}

// ---------------------------------------------------------------------------
// Memory peer (the "PC" side): a real smoltcp Interface over Vec queues
// ---------------------------------------------------------------------------

pub struct PeerDev {
    rx: VecDeque<Vec<u8>>,
    tx: Vec<Vec<u8>>,
}

pub struct PeerRxToken {
    frame: Vec<u8>,
}

impl RxToken for PeerRxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame)
    }
}

pub struct PeerTxToken<'a> {
    dev: &'a mut PeerDev,
}

impl TxToken for PeerTxToken<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = vec![0u8; len];
        let r = f(&mut buf);
        self.dev.tx.push(buf);
        r
    }
}

impl Device for PeerDev {
    type RxToken<'a>
        = PeerRxToken
    where
        Self: 'a;
    type TxToken<'a>
        = PeerTxToken<'a>
    where
        Self: 'a;

    fn receive(&mut self, _timestamp: SmolInstant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let frame = self.rx.pop_front()?;
        Some((PeerRxToken { frame }, PeerTxToken { dev: self }))
    }

    fn transmit(&mut self, _timestamp: SmolInstant) -> Option<Self::TxToken<'_>> {
        Some(PeerTxToken { dev: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ethernet;
        caps.max_transmission_unit = 1514;
        caps
    }
}

/// One listening peer (UDP echo client, TCP client, ICMP ping source).
pub struct PeerHost {
    pub iface: Interface,
    pub dev: PeerDev,
    pub sockets: SocketSet<'static>,
    pub udp: SocketHandle,
    pub tcp: SocketHandle,
    pub icmp: SocketHandle,
}

impl PeerHost {
    pub fn new(now: SmolInstant) -> Self {
        let mut dev = PeerDev { rx: VecDeque::new(), tx: Vec::new() };
        let mut config = Config::new(HardwareAddress::Ethernet(
            smoltcp::wire::EthernetAddress::from_bytes(&PEER_MAC),
        ));
        config.random_seed = 0x1234_5678;
        let mut iface = Interface::new(config, &mut dev, now);
        iface.update_ip_addrs(|v| {
            v.push(IpCidr::new(
                IpAddress::v4(PEER_IP[0], PEER_IP[1], PEER_IP[2], PEER_IP[3]),
                24,
            ))
            .expect("peer address slot");
        });

        let sockets = Box::leak(Box::new([SocketStorage::EMPTY; 3]));
        let mut set = SocketSet::new(&mut sockets[..]);

        let udp_rx_meta = Box::leak(Box::new([udp::PacketMetadata::EMPTY; 4]));
        let udp_rx = Box::leak(Box::new([0u8; 4096]));
        let udp_tx_meta = Box::leak(Box::new([udp::PacketMetadata::EMPTY; 4]));
        let udp_tx = Box::leak(Box::new([0u8; 4096]));
        let mut udp_sock = udp::Socket::new(
            udp::PacketBuffer::new(&mut udp_rx_meta[..], &mut udp_rx[..]),
            udp::PacketBuffer::new(&mut udp_tx_meta[..], &mut udp_tx[..]),
        );
        udp_sock.bind(61001).expect("peer udp bind");
        let udp = set.add(udp_sock);

        let tcp_rx = Box::leak(Box::new([0u8; 4096]));
        let tcp_tx = Box::leak(Box::new([0u8; 4096]));
        let tcp_sock = tcp::Socket::new(
            tcp::SocketBuffer::new(&mut tcp_rx[..]),
            tcp::SocketBuffer::new(&mut tcp_tx[..]),
        );
        let tcp = set.add(tcp_sock);

        let icmp_rx_meta = Box::leak(Box::new([icmp::PacketMetadata::EMPTY; 4]));
        let icmp_rx = Box::leak(Box::new([0u8; 2048]));
        let icmp_tx_meta = Box::leak(Box::new([icmp::PacketMetadata::EMPTY; 4]));
        let icmp_tx = Box::leak(Box::new([0u8; 2048]));
        let mut icmp_sock = icmp::Socket::new(
            icmp::PacketBuffer::new(&mut icmp_rx_meta[..], &mut icmp_rx[..]),
            icmp::PacketBuffer::new(&mut icmp_tx_meta[..], &mut icmp_tx[..]),
        );
        // Ident-scoped endpoint: matches the echo replies for our ident.
        icmp_sock.bind(icmp::Endpoint::Ident(0x1234)).expect("peer icmp bind");
        let icmp = set.add(icmp_sock);

        PeerHost { iface, dev, sockets: set, udp, tcp, icmp }
    }

    /// One peer stack poll at `now`.
    pub fn poll(&mut self, now: SmolInstant) {
        let Self { iface, dev, sockets, .. } = self;
        iface.poll(now, dev, sockets);
    }

    /// Frames the peer transmitted (drained into the board RX ring).
    pub fn take_tx(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.dev.tx)
    }

    /// Delivers a board wire frame to the peer's ingress queue.
    pub fn push_rx(&mut self, frame: Vec<u8>) {
        self.dev.rx.push_back(frame);
    }
}
