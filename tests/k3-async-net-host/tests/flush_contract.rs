//! MS12 Iteration 001 task T5 (2.3): the production K3 adapter under the
//! real target-scoped C4 flush.
//!
//! The QEMU-side flush semantics (Busy second waiter, waiter-drop ownership,
//! stale drop, counters) keep their GREEN witnesses in the axnet
//! queue-diagnostics face; this file adds the production-adapter slice: the
//! flush target set grows with each accepted diagnostic enqueue (new
//! ticket), stays Pending while the tickets are device-owned, survives a
//! waiter drop, completes through C4 exactly when the completions are
//! reclaimed, and reports the stable fault to the waiter instead of
//! hanging. All polling is a manual waker (no scheduler entanglement); the
//! queue owner runs on the host axtask scheduler and is pumped with
//! `yield_now`.

#![allow(dead_code, unused_imports)]

use std::sync::Once;
use std::task::Poll;
use std::time::{Duration, Instant};

use axdriver::AxDeviceContainer;
use axdriver_net::NetDriverOps;
use axnet::{flush, init_network, publish_queue_event, rx_snapshot, start_rx_task_affinity};
use axtask::{init_scheduler, schedulable_cpu_mask, yield_now};

use memory_addr::{PhysAddr, VirtAddr};

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`):
// same stub shape as axnet's own host tests; never called on this path.
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

const _: unsafe extern "Rust" fn(PhysAddr, usize) -> axerrno::AxResult<VirtAddr> =
    __axklib_0_3_mem_iomap;

use k3_async_net_host::fake::{Board, FakeEnv};
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;
use k3_async_net_host::k3_gmac::{DMA_CHAN_STATUS, DMA_STATUS_FBE};

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

fn udp_checksum(src_ip: [u8; 4], dst_ip: [u8; 4], udp: &[u8]) -> u16 {
    let mut sum = 0u32;
    for pair in src_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    for pair in dst_ip.chunks_exact(2) {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    sum += 17u32 + udp.len() as u32;
    let mut chunks = udp.chunks_exact(2);
    for pair in &mut chunks {
        sum += u16::from_be_bytes([pair[0], pair[1]]) as u32;
    }
    if let [last] = chunks.remainder() {
        sum += ((*last as u16) << 8) as u32;
    }
    checksum16(sum)
}

fn udp_frame(sport: u16, dport: u16, payload: &[u8]) -> Vec<u8> {
    let udp_len = 8 + payload.len();
    let mut udp = vec![0u8; udp_len];
    udp[0..2].copy_from_slice(&sport.to_be_bytes());
    udp[2..4].copy_from_slice(&dport.to_be_bytes());
    udp[4..6].copy_from_slice(&(udp_len as u16).to_be_bytes());
    let ck = udp_checksum(peer_ip(), board_ip(), &udp);
    udp[6..8].copy_from_slice(&ck.to_be_bytes());
    udp[8..].copy_from_slice(payload);

    let ip_total = 20 + udp_len;
    let mut ip = vec![0u8; 20];
    ip[0] = 0x45;
    ip[2..4].copy_from_slice(&(ip_total as u16).to_be_bytes());
    ip[6..7].copy_from_slice(&[0x40]);
    ip[8] = 64;
    ip[9] = 17;
    ip[12..16].copy_from_slice(&peer_ip());
    ip[16..20].copy_from_slice(&board_ip());
    let ck = ipv4_checksum(&ip);
    ip[10..12].copy_from_slice(&ck.to_be_bytes());

    let mut frame = vec![0u8; 14 + ip_total];
    frame[0..6].copy_from_slice(&[0x02, 0, 0, 0, 0x26, 0x01]);
    frame[6..12].copy_from_slice(&[0x02, 0, 0, 0, 0x26, 0x02]);
    frame[12] = 0x08;
    frame[13] = 0x00;
    frame[14..34].copy_from_slice(&ip);
    frame[34..].copy_from_slice(&udp);
    frame
}

static SCHED: Once = Once::new();

fn pump(rounds: usize) {
    for _ in 0..rounds {
        yield_now();
    }
}

/// One manual poll of the flush future with a counting waker.
fn poll_flush<F: core::future::Future<Output = Result<(), axdriver_net::DevError>> + Unpin>(
    fut: &mut F,
) -> Poll<Result<(), axdriver_net::DevError>> {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct CountWake(Arc<AtomicUsize>);
    impl std::task::Wake for CountWake {
        fn wake(self: Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    let count = Arc::new(AtomicUsize::new(0));
    let waker = std::task::Waker::from(Arc::new(CountWake(count.clone())));
    let mut cx = std::task::Context::from_waker(&waker);
    core::future::Future::poll(core::pin::Pin::new(fut), &mut cx)
}

#[test]
fn production_adapter_flush_target_set_and_c4() {
    SCHED.call_once(init_scheduler);
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let drv = board.ready_async_driver();
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter construction");
    let nic: Box<dyn NetDriverOps> = Box::new(adapter);
    init_network(AxDeviceContainer::from_one(nic));
    start_rx_task_affinity(schedulable_cpu_mask()).expect("first start wins");
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let snap = rx_snapshot();
        if snap.lifecycle == 2 {
            break;
        }
        assert!(Instant::now() < deadline, "activation timeout: {snap:?}");
        pump(8);
    }

    // Empty target set: the flush completes immediately.
    let mut fut = flush().expect("flush reservation");
    assert!(
        matches!(poll_flush(&mut fut), Poll::Ready(Ok(()))),
        "empty target set completes immediately"
    );

    // New ticket: an accepted enqueue joins the flush target set and the
    // future stays Pending while the ticket is device-owned.
    let report = axnet::diagnostic_enqueue_frame(
        [0xff; 6],
        0x0800,
        b"flush-a",
    )
    .expect("diagnostic enqueue");
    assert!(report.accepted, "first frame took a slot");
    pump(64);
    let mut fut = flush().expect("flush reservation");
    assert!(
        matches!(poll_flush(&mut fut), Poll::Pending),
        "device-owned ticket holds the flush"
    );

    // A later ticket joins the same target set: still Pending.
    let report = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"flush-b")
        .expect("diagnostic enqueue");
    assert!(report.accepted);
    pump(64);
    assert!(
        matches!(poll_flush(&mut fut), Poll::Pending),
        "later ticket joins the target set"
    );

    // Waiter drop cancels this waiter without touching packet ownership; a
    // fresh reservation observes the same set.
    drop(fut);
    let mut fut = flush().expect("fresh flush reservation");
    assert!(matches!(poll_flush(&mut fut), Poll::Pending));

    // The completions land; the owner reclaims them and C4 releases.
    let drained = board.wire.drain_board_tx();
    assert_eq!(drained.len(), 2, "both frames went out");
    publish_queue_event();
    pump(128);
    assert!(
        matches!(poll_flush(&mut fut), Poll::Ready(Ok(()))),
        "flush completes after reclaim (C4 release)"
    );

    // Stable fault: an in-flight ticket plus a device fatal wakes the
    // waiter with the error instead of hanging forever.
    let report = axnet::diagnostic_enqueue_frame([0xff; 6], 0x0800, b"flush-c")
        .expect("diagnostic enqueue before fault");
    assert!(report.accepted);
    pump(64);
    let mut fut = flush().expect("flush reservation");
    assert!(matches!(poll_flush(&mut fut), Poll::Pending));
    board
        .env
        .set_reg(DMA_CHAN_STATUS, DMA_STATUS_FBE);
    publish_queue_event();
    pump(128);
    match poll_flush(&mut fut) {
        Poll::Ready(Ok(())) => panic!("a device fatal must not complete the flush as success"),
        Poll::Ready(Err(_err)) => {}
        Poll::Pending => panic!("the fault must wake the flush waiter"),
    }
}
