//! MS12 Iteration 001 task T3 (2.1): production `K3NetAdapter` contract
//! witness over the fake board.
//!
//! Coverage per the Task Contract: length bounds 14/60/1514/1515, pool and
//! descriptor 16/17 conservation, wrap (slot 15→0), single reclaim, post-
//! publish backing retention, terminal-timeout owner preservation,
//! completion_pending without false work, and fault rejection/propagation.
//! Every test drives the production adapter (`k3_gmac::adapter`) against the
//! real `Gmac` + leaked arena — no re-implementation of either side.

#![allow(dead_code, unused_imports)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axdriver_net::{
    BaseDriverOps, DevError, DeviceType, NetBuf, NetDriverOps, NetQueueControl,
    NetQueueDirection, NetTxQueue, QueueEpoch, TxCookie,
};
use memory_addr::{PhysAddr, VirtAddr};

use k3_async_net_host::fake::{Board, FakeEnv, LeakBacking};
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;
use k3_async_net_host::k3_gmac::{
    DMA_CHAN_IRQ_EN, DMA_CHAN_STATUS, DMA_STATUS_FBE, Gmac, TX_TERMINAL_NANOS,
};

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
// Same stub as the other integration tests: never called on this path (the
// K3 adapter performs no dynamic iomap).
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

/// A minimal legal frame: dst=broadcast, src=board, ethertype=0x0806 padding.
fn frame(len: usize) -> Vec<u8> {
    let mut f = vec![0u8; len];
    f[0..6].copy_from_slice(&[0xff; 6]);
    f[6..12].copy_from_slice(&k3_async_net_host::fake::BOARD_MAC);
    if len >= 14 {
        f[12] = 0x08;
        f[13] = 0x06;
    }
    for (i, b) in f.iter_mut().enumerate().skip(14) {
        *b = (i & 0xff) as u8;
    }
    f
}

fn ready_adapter() -> (K3NetAdapter<LeakBacking, FakeEnv>, Board, FakeEnv) {
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let drv = board.ready_async_driver();
    let script = board.env.handle();
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter construction");
    (adapter, board, script)
}

#[test]
fn device_identity_reports_async_irq_and_bounded_queues() {
    let (mut adapter, _board, _script) = ready_adapter();
    assert_eq!(adapter.device_name(), "k3-gmac-async");
    assert_eq!(adapter.device_type(), DeviceType::Net);
    assert_eq!(adapter.irq_num(), Some(133));
    assert_eq!(adapter.mac_address().0, [0x02, 0x00, 0x00, 0x00, 0x26, 0x01]);
    assert_eq!(adapter.rx_queue_size(), 16);
    assert_eq!(adapter.tx_queue_size(), 16);
    assert!(adapter.queue_control().is_some(), "queue control exposed");
    assert!(adapter.tx_queue().is_some(), "tx queue exposed");
    assert!(adapter.recovery_control().is_none(), "no recovery control");
}

#[test]
fn alloc_tx_buffer_enforces_frame_bounds_and_pool_capacity() {
    let (mut adapter, _board, _script) = ready_adapter();
    assert!(matches!(adapter.alloc_tx_buffer(13), Err(DevError::InvalidParam)));
    assert!(matches!(adapter.alloc_tx_buffer(1515), Err(DevError::InvalidParam)));
    // 16×2048 CPU pool: bounds rejects consumed nothing, so 16 concurrent
    // allocations must succeed and the 17th reports pressure as `Again`.
    // The raw pointers are held (never dropped): a NetBufPtr has no Drop —
    // buffers return to the pool only through the driver paths.
    let mut held = Vec::new();
    held.push(adapter.alloc_tx_buffer(14).expect("pool alloc"));
    held.push(adapter.alloc_tx_buffer(60).expect("pool alloc"));
    held.push(adapter.alloc_tx_buffer(1514).expect("pool alloc"));
    for _ in 3..16 {
        held.push(adapter.alloc_tx_buffer(60).expect("pool alloc"));
    }
    assert!(matches!(adapter.alloc_tx_buffer(60), Err(DevError::Again)));
    assert_eq!(held.len(), 16);
}

#[test]
fn submit_length_boundaries_never_truncate() {
    let (mut adapter, mut board, _script) = ready_adapter();
    for len in [14usize, 60, 1514] {
        let ptr = adapter.alloc_tx_buffer(len).expect("alloc");
        // SAFETY: reconstructing the buffer allocated above to fill it —
        // exactly the alloc→copy→submit shape the production tx_submit_one
        // runs before calling into the driver.
        let mut nb = unsafe { NetBuf::from_buf_ptr(ptr) };
        nb.packet_mut().copy_from_slice(&frame(len));
        adapter
            .tx_queue()
            .unwrap()
            .submit_tx(nb.into_buf_ptr(), TxCookie::new(len as u64))
            .expect("submit");
    }
    let sent = board.wire.drain_board_tx();
    assert_eq!(sent.len(), 3, "three terminals");
    // The MAC pads sub-60 frames to 60 (TX_PAD_TO) and appends no FCS to
    // the descriptor length; only the 1514 frame keeps its full length.
    assert_eq!(sent[0].len(), 60);
    assert_eq!(&sent[0][..14], &frame(14)[..]);
    assert_eq!(sent[1].len(), 60);
    assert_eq!(sent[2].len(), 1514);
    for cookie in [14u64, 60, 1514] {
        assert_eq!(
            adapter.tx_queue().unwrap().reclaim_tx().unwrap(),
            Some(TxCookie::new(cookie)),
            "cookie round-trips once"
        );
    }
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), None);
    // 1515 is refused by the fixed-pool allocator gate: nothing published,
    // the wire stays empty and no buffer left the driver pool.
    assert!(matches!(adapter.alloc_tx_buffer(1515), Err(DevError::InvalidParam)));
    assert!(board.wire.drain_board_tx().is_empty());
}

#[test]
fn pool_and_descriptor_ledgers_conserve_at_16_and_refuse_17() {
    let (mut adapter, mut board, _script) = ready_adapter();
    let mut cookies = Vec::new();
    for i in 0..16u64 {
        let buf = adapter.alloc_tx_buffer(60).expect("alloc within capacity");
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(i)).expect("submit within capacity");
        cookies.push(TxCookie::new(i));
    }
    // The 17th buffer is out of the fixed CPU pool: pre-accept pressure.
    assert!(matches!(adapter.alloc_tx_buffer(60), Err(DevError::Again)));
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().expect("observable ledger");
    assert_eq!(ledger.buffer_available + ledger.buffer_inflight, 16);
    assert_eq!(ledger.descriptor_available + ledger.descriptor_inflight, 16);
    assert_eq!(ledger.buffer_available, 0, "all 16 buffers bound to live cookies");
    assert_eq!(ledger.descriptor_inflight, 16);
    assert_eq!(ledger.completions_seen, 0, "no completion observed before drain");
    // The 17th submit with a hand-bound buffer would be driver-Full; the
    // pool gate above already refused it pre-accept.
    board.wire.drain_board_tx();
    // Reclaim exactly one: the ledger moves one buffer and one descriptor.
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(cookies[0]));
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
    assert_eq!(ledger.buffer_available, 1);
    assert_eq!(ledger.descriptor_inflight, 15);
    assert_eq!(ledger.completions_seen, 1);
}

#[test]
fn slots_wrap_from_15_to_0_and_never_double_reclaim() {
    let (mut adapter, mut board, _script) = ready_adapter();
    for i in 0..18u64 {
        let buf = adapter.alloc_tx_buffer(60).expect("alloc");
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(i)).expect("submit");
        assert!(board.wire.drain_board_tx().len() <= 1);
        assert_eq!(
            adapter.tx_queue().unwrap().reclaim_tx().unwrap(),
            Some(TxCookie::new(i)),
            "submit {i}: the same cookie comes back exactly once"
        );
        assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), None, "no duplicate");
    }
    // 18 submits crossed the 16-slot boundary (wrap 15→0) with the ledger
    // fully conserved.
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
    assert_eq!(ledger.buffer_available, 16);
    assert_eq!(ledger.descriptor_inflight, 0);
    assert_eq!(ledger.completions_seen, 18);
}

#[test]
fn cookies_keep_owner_epoch_across_the_device() {
    let (mut adapter, mut board, _script) = ready_adapter();
    let epoch = QueueEpoch::MIN.advance().unwrap();
    let cookie = TxCookie::with_epoch(epoch, 4242);
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, cookie).unwrap();
    board.wire.drain_board_tx();
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(cookie));
}

#[test]
fn postpublish_backing_is_retained_until_external_reset() {
    // Published (init ran): the adapter drop must NOT release the backing.
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let published = Arc::new(AtomicBool::new(false));
    let mut drv = Gmac::new_async(board.wire.tracking_backing(published.clone())).expect("arena");
    drv.init_rx(&mut board.env).expect("init_rx publishes the backing");
    drv.enable_tx(&mut board.env);
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter");
    drop(adapter);
    assert!(
        !published.load(Ordering::SeqCst),
        "published backing retained (no software release)"
    );

    // Pre-publish: an unpublished guard is released by the drop.
    let board = Board::new();
    board.env.set_phy_link_1000fd();
    let dropped = Arc::new(AtomicBool::new(false));
    let drv = Gmac::new_async(board.wire.tracking_backing(dropped.clone())).expect("arena");
    let adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter");
    drop(adapter);
    assert!(dropped.load(Ordering::SeqCst), "unpublished backing released");
}

#[test]
fn terminal_timeout_keeps_owner_and_never_reuses_the_slot() {
    let (mut adapter, mut board, script) = ready_adapter();
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(7)).unwrap();
    // Let the independent 1 s terminal window lapse with OWN still set.
    script.advance_clock(TX_TERMINAL_NANOS + 1);
    let err = adapter.tx_queue().unwrap().reclaim_tx().unwrap_err();
    assert!(
        matches!(err, DevError::Io),
        "deadline reports Io (platform has no TimedOut variant)"
    );
    // The owner is preserved: the ledger keeps one inflight descriptor and
    // one bound buffer; a later reclaim never fabricates a completion.
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
    assert_eq!(ledger.descriptor_inflight, 1);
    assert_eq!(ledger.buffer_inflight, 1);
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), None);
    // Even after the device finally completes the descriptor, the timed-out
    // submit is never re-attributed: the slot stays retired.
    board.wire.drain_board_tx();
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), None);
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
    assert_eq!(ledger.descriptor_inflight, 1);
    assert_eq!(ledger.buffer_inflight, 1);
}

#[test]
fn completion_pending_reports_no_false_work_on_empty_ring() {
    let (mut adapter, mut board, _script) = ready_adapter();
    let control = adapter.queue_control().unwrap();
    assert_eq!(
        control.completion_pending(NetQueueDirection::BOTH).unwrap(),
        NetQueueDirection::NONE,
        "empty RX ring and zero live TX owners: nothing pending"
    );
    assert_eq!(
        control.completion_pending(NetQueueDirection::TX).unwrap(),
        NetQueueDirection::NONE,
        "zero inflight: TX can never report work"
    );
    // A real RX completion becomes visible without any TX false positive.
    board.wire.inject_rx_frame(&frame(60));
    assert_eq!(
        control.completion_pending(NetQueueDirection::BOTH).unwrap(),
        NetQueueDirection::RX
    );
}

#[test]
fn fault_rejects_submits_and_propagates_through_queries() {
    let (mut adapter, _board, script) = ready_adapter();
    // The adapter holds one env clone; the test scripts FBE through its own.
    script.set_reg(DMA_CHAN_STATUS, DMA_STATUS_FBE);
    let buf = adapter.alloc_tx_buffer(60).expect("software resources stay legal");
    let err = adapter
        .tx_queue()
        .unwrap()
        .submit_tx(buf, TxCookie::new(1))
        .expect_err("faulted driver refuses submit");
    assert!(matches!(err, DevError::Io));
    // The fault closed the device notification (IRQ_EN = 0).
    assert_eq!(script.get_reg(DMA_CHAN_IRQ_EN), 0, "notify suppressed on fault");
    // Queries propagate the fault instead of fabricating quiet.
    assert!(adapter.queue_control().unwrap().completion_pending(NetQueueDirection::BOTH).is_err());
    // The refused submit released its buffer: the pool still serves 16.
    let mut held = Vec::new();
    for _ in 0..16 {
        held.push(adapter.alloc_tx_buffer(60).expect("pool intact after fault"));
    }
    assert!(matches!(adapter.alloc_tx_buffer(60), Err(DevError::Again)));
}

#[test]
fn receive_delivers_refills_wraps_and_drops_classified_frames() {
    let (mut adapter, mut board, _script) = ready_adapter();
    // One delivered frame with byte-exact content and recycling.
    board.wire.inject_rx_frame(&frame(60));
    let rx = adapter.receive().expect("frame delivered");
    assert_eq!(rx.packet_len(), 60);
    assert_eq!(rx.packet(), &frame(60)[..]);
    adapter.recycle_rx_buffer(rx).expect("recycle");

    // A classified non-deliverable descriptor drops with bounded continue:
    // the next receive reports Again (idle), never a fault.
    board.wire.inject_rx_reject();
    assert!(matches!(adapter.receive(), Err(DevError::Again)));

    // 16 more delivered frames walk the wrap (slot 15 → 0) with refill.
    for i in 0..16u32 {
        let payload = frame(60 + i as usize);
        board.wire.inject_rx_frame(&payload);
        let rx = adapter.receive().unwrap_or_else(|e| panic!("wrap rx {i}: {e:?}"));
        assert_eq!(rx.packet(), &payload[..]);
        adapter.recycle_rx_buffer(rx).unwrap();
    }
    assert!(matches!(adapter.receive(), Err(DevError::Again)), "drained ring is idle");
}

#[test]
fn arm_recheck_reports_pending_after_suppress_cycle() {
    let (mut adapter, mut board, _script) = ready_adapter();
    let control = adapter.queue_control().unwrap();
    control.suppress_notify(NetQueueDirection::BOTH).expect("suppress both");
    board.wire.inject_rx_frame(&frame(60));
    let pending = control.arm_notify_and_check(NetQueueDirection::BOTH).expect("arm both");
    assert_eq!(pending, NetQueueDirection::RX, "arm/recheck sees the landed completion");
}
