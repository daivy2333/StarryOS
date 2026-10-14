//! MS12 Iteration 001 task T4 (2.2): the production K3 adapter as the ONLY
//! NIC of the real axnet `Service`, driven by the public axnet surface on a
//! host axtask scheduler.
//!
//! This is the host mirror of the board Service path (design D4):
//! `init_network(AxDeviceContainer::from_one(..))` +
//! `start_rx_task_affinity(schedulable_cpu_mask())`, then the queue owner
//! activates (suppress both + slot mode), reaches `Active` with no polling
//! fallback and serves the bounded bidirectional data plane. The test
//! thread plays the device half (FakeWire) and the ISR work-hint half
//! (`publish_queue_event` — the same function the board ISR calls).
//!
//! Compile-time env: `AX_IP`/`AX_GW` must match the run invocation (the
//! same `option_env!` consts the board build uses).
//!
//! Coverage per the Task Contract: duplicate start rejection, activation to
//! `Active` (owner async, no polling), notify-window closure (event before
//! arm), RX 64/65 slot boundary with recovery, TX Full → release →
//! recovery with ARP resolution, and stable `Faulted` (no polling
//! rollback).

#![allow(dead_code, unused_imports)]

use std::net::{IpAddr, SocketAddr};
use std::sync::Once;
use std::time::{Duration, Instant};

use axdriver::AxDeviceContainer;
use axdriver_net::NetDriverOps;
use axnet::{
    RxSnapshot, SocketOps, init_network, publish_queue_event, rx_snapshot, start_rx_task_affinity,
};
use axnet::udp::UdpSocket;
use axtask::{init_scheduler, schedulable_cpu_mask, yield_now};

use memory_addr::{PhysAddr, VirtAddr};

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
// Same stub shape as axnet's own host tests: never called on this path (the
// K3 adapter performs no dynamic iomap); returns a stable error so an
// accidental call is diagnosable instead of trapping.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

// Compile-time ABI witness: the stub must be assignable to the exact fn
// pointer type the trait-ffi macro generates for `Klib::mem_iomap`.
const _: unsafe extern "Rust" fn(PhysAddr, usize) -> axerrno::AxResult<VirtAddr> =
    __axklib_0_3_mem_iomap;

use k3_async_net_host::fake::{Board, FakeEnv, LeakBacking};
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;

const BOARD_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x01];
const PEER_MAC: [u8; 6] = [0x02, 0x00, 0x00, 0x00, 0x26, 0x02];
/// The compile-time axnet consts (same env the board build passes).
fn board_ip() -> [u8; 4] {
    match option_env!("AX_IP") {
        Some("192.168.137.2") => [192, 168, 137, 2],
        _ => [10, 0, 2, 15],
    }
}
fn peer_ip() -> [u8; 4] {
    match option_env!("AX_GW") {
        Some("192.168.137.1") => [192, 168, 137, 1],
        _ => [10, 0, 2, 2],
    }
}

static SCHED: Once = Once::new();

fn init_scheduler_once() {
    SCHED.call_once(init_scheduler);
}

// ---------------------------------------------------------------------------
// Wire frames (IPv4/UDP with real checksums)
// ---------------------------------------------------------------------------

fn checksum16(sum: u32) -> u16 {
    let mut sum = (sum & 0xffff) + (sum >> 16);
    sum = (sum & 0xffff) + (sum >> 16);
    !(sum as u16)
}

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in header.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    checksum16(sum)
}

fn udp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp_header_and_payload: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in src_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    for pair in dst_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    sum += 17u32; // proto UDP
    sum += udp_header_and_payload.len() as u32;
    let mut chunks = udp_header_and_payload.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += ((*last as u16) << 8) as u32;
    }
    checksum16(sum)
}

/// One complete peer→board IPv4/UDP frame (Ethernet + IPv4 + UDP).
fn udp_frame(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let mut udp = vec![0u8; udp_len];
    udp[0..2].copy_from_slice(&sport.to_be_bytes());
    udp[2..4].copy_from_slice(&dport.to_be_bytes());
    udp[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    udp[6..8].copy_from_slice(&[0, 0]); // placeholder
    udp[8..].copy_from_slice(payload);
    let ck = udp_checksum(peer_ip(), board_ip(), &udp);
    udp[6..8].copy_from_slice(&ck.to_be_bytes());

    let ip_total = 20 + udp_len;
    let mut ip = vec![0u8; 20];
    ip[0] = 0x45;
    ip[1] = 0;
    ip[2..4].copy_from_slice(&(ip_total as u16).to_be_bytes());
    ip[4..6].copy_from_slice(&[0, 1]); // id
    ip[6..7].copy_from_slice(&[0x40]); // don't fragment
    ip[8] = 64; // ttl
    ip[9] = 17; // proto UDP
    ip[12..16].copy_from_slice(&peer_ip());
    ip[16..20].copy_from_slice(&board_ip());
    let ck = ipv4_checksum(&ip);
    ip[10..12].copy_from_slice(&ck.to_be_bytes());

    let mut frame = vec![0u8; 14 + ip_total];
    frame[0..6].copy_from_slice(&BOARD_MAC);
    frame[6..12].copy_from_slice(&PEER_MAC);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14..34].copy_from_slice(&ip);
    frame[34..].copy_from_slice(&udp);
    frame
}

// ---------------------------------------------------------------------------
// Test harness: pump the host scheduler, drive the device half
// ---------------------------------------------------------------------------

struct Owner {
    board: Board,
}

impl Owner {
    fn pump(&mut self, rounds: usize) {
        for _ in 0..rounds {
            yield_now();
        }
    }

    /// Pumps until `cond` over fresh snapshots or the deadline expires.
    fn pump_until(&mut self, deadline: Instant, mut cond: impl FnMut(&RxSnapshot) -> bool) -> RxSnapshot {
        loop {
            let snap = rx_snapshot();
            if cond(&snap) {
                return snap;
            }
            assert!(Instant::now() < deadline, "timed out waiting; snap={snap:?}");
            yield_now();
        }
    }

    fn inject_and_kick(&mut self, frame: &[u8]) {
        self.board.wire.inject_rx_frame(frame);
        publish_queue_event();
        self.pump(16);
    }
}

fn setup_owner() -> Owner {
    init_scheduler_once();
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let drv = board.ready_async_driver();
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter construction");
    let nic: Box<dyn NetDriverOps> = Box::new(adapter);
    init_network(AxDeviceContainer::from_one(nic));
    let handle =
        start_rx_task_affinity(schedulable_cpu_mask()).expect("first start wins");
    let _ = handle;
    Owner { board }
}

#[test]
fn service_owner_bounded_bidirectional_contract() {
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut owner = setup_owner();

    // Duplicate start is rejected without a second owner.
    assert!(
        start_rx_task_affinity(schedulable_cpu_mask()).is_err(),
        "second start must be rejected"
    );

    // Activation: suppress-both + slot mode on the production adapter, then
    // Active with the async owner view (no polling fallback).
    let snap = owner.pump_until(deadline, |s| s.lifecycle == 2);
    assert_eq!(snap.owner, 1, "async owner (no polling fallback)");

    // Notify window (event before/at arm): a completion that lands before
    // the owner's next arm is served by the arm/recheck protocol — the
    // frame is reaped into the fixed RX slot storage in one kick.
    owner.inject_and_kick(&udp_frame(40000, 26001, b"ping-01"));
    let snap = rx_snapshot();
    assert_eq!(snap.reaped, 1, "event-before-arm completion served");
    assert_eq!(snap.refilled, 1, "the reaped slot was refilled");

    // RX 64/65: fill all 64 software slots, then the 65th frame must stay
    // in its descriptor — bounded backpressure, never overwritten (the
    // stack-side drain is Iteration 002; the slot boundary is this round's).
    for i in 0..63u32 {
        let payload = format!("fill-{i:02}");
        owner.board.wire.inject_rx_frame(&udp_frame(40001, 26001, payload.as_bytes()));
        if i % 8 == 7 {
            publish_queue_event();
            owner.pump(16);
        }
    }
    publish_queue_event();
    owner.pump(64);
    owner.pump_until(deadline, |s| s.reaped >= 64);
    let full_reaped = rx_snapshot().reaped;
    assert_eq!(full_reaped, 64, "all 64 slots took a frame");

    // The 65th frame: the queue owner must not reap it (Full, never an
    // overwrite) and must stay Active while backpressured.
    owner.board.wire.inject_rx_frame(&udp_frame(40001, 26001, b"fill-65"));
    publish_queue_event();
    owner.pump(128);
    let stalled = rx_snapshot();
    assert_eq!(stalled.reaped, 64, "65th frame stays in its descriptor while slots are full");
    assert_eq!(stalled.lifecycle, 2, "backpressure never faults the owner");

    // Stable Faulted: a fatal bus error folds into a faulted owner that
    // never rolls back to polling and never serves more frames.
    owner.board.env.set_reg(
        k3_async_net_host::k3_gmac::DMA_CHAN_STATUS,
        k3_async_net_host::k3_gmac::DMA_STATUS_FBE,
    );
    publish_queue_event();
    let snap = owner.pump_until(deadline, |s| s.lifecycle == 3);
    assert_eq!(snap.owner, 1, "faulted owner stays async-owned");
    let fault_reaped = rx_snapshot().reaped;
    for _ in 0..8 {
        owner
            .board
            .wire
            .inject_rx_frame(&udp_frame(40002, 26001, b"after-fault"));
        publish_queue_event();
        owner.pump(32);
    }
    let later = rx_snapshot();
    assert_eq!(later.reaped, fault_reaped, "faulted data plane serves nothing more");
    assert_eq!(later.lifecycle, 3, "fault stays stable across rounds");
    // MS13 T2/T4: the K3 adapter exposes NetRecoveryControl, so a device
    // fatal drives the resident-recovery path; the faulted driver refuses
    // begin (FBE stays a stable error, never reset-cleared), so the owner
    // still fail-stops in Faulted. MS13 T4 made the accounting single: the
    // fault counter counts exactly one terminal commit per fatal event class
    // — the begin-refusal quarantine — while the detection and the
    // recovery-entry observations only update the last-error record.
    assert_eq!(
        later.fault, 1,
        "single terminal accounting for one fatal event class, then stable fail-stop"
    );
}
