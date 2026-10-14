//! MS13 Iteration 000 task T2: production `K3NetAdapter` recovery contract
//! witness over the fake board.
//!
//! Coverage per the Task Contract: epoch binding (stale/future cookie
//! refusal, new-epoch cookies riding the reused slots), confirmed-stop
//! carrier settling (old cookies never return, buffers quarantined), reset
//! failure preserving the conservative device-owned classification and the
//! backing, rebuild failure never publishing the partial queue, and
//! repeated recovery preserving the fixed capacity. Every test drives the
//! production adapter (`k3_gmac::adapter`) over the real `Gmac` + leaked
//! arena through the transport-neutral `NetRecoveryControl` trait.

#![allow(dead_code, unused_imports)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use axdriver_net::{
    DevError, NetDriverOps, NetQueueControl, NetQueueDirection, NetRecoveryControl, NetTxQueue,
    QueueEpoch, RecoveryProgress, RecoveryStage, TxCookie,
};
use memory_addr::{PhysAddr, VirtAddr};

use k3_async_net_host::fake::{Board, FakeEnv, LeakBacking};
use k3_async_net_host::k3_gmac::adapter::K3NetAdapter;
use k3_async_net_host::k3_gmac::{Gmac, DMA_RX_DESC_LL};

/// The MAC_CONFIG register offset (the runtime ability application target).
const MAC_CONFIG_OFF: u32 = 0x000;

// Kernel ABI symbol required by `axklib` (pulled in via `axdriver/dyn`).
#[unsafe(no_mangle)]
unsafe extern "Rust" fn __axklib_0_3_mem_iomap(
    _addr: PhysAddr,
    _size: usize,
) -> axerrno::AxResult<VirtAddr> {
    Err(axerrno::AxError::Unsupported)
}

/// A minimal legal frame.
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

/// The `recovery_control` seam is exposed before anything else — the
/// observable RED of this contract while the adapter still inherits `None`.
#[test]
fn recovery_control_is_exposed() {
    let (mut adapter, _board, _script) = ready_adapter();
    assert!(
        adapter.recovery_control().is_some(),
        "the production adapter exposes NetRecoveryControl"
    );
    let control = adapter.recovery_control().unwrap();
    let prog = control.progress();
    assert_eq!(prog.stage, RecoveryStage::Idle);
    assert_eq!(prog.epoch, QueueEpoch::MIN);
}

/// Drives one full recovery through the trait and asserts the stage path
/// (Resetting → Reinitializing → Recovered) and the target epoch.
fn run_recovery(adapter: &mut K3NetAdapter<LeakBacking, FakeEnv>) -> QueueEpoch {
    let mut saw_reinit = false;
    let control = adapter.recovery_control().expect("recovery control");
    let prog = control.begin_recovery().expect("begin from healthy");
    assert_eq!(prog.stage, RecoveryStage::Resetting);
    let mut steps = 0usize;
    loop {
        match control.poll_recovery_step().expect("in-window step") {
            RecoveryProgress { stage: RecoveryStage::Resetting, .. } => {}
            RecoveryProgress { stage: RecoveryStage::Reinitializing, .. } => saw_reinit = true,
            RecoveryProgress { stage: RecoveryStage::Recovered, epoch } => {
                assert!(saw_reinit, "the confirmed boundary was observed");
                return epoch;
            }
            other => panic!("unexpected recovery stage {other:?}"),
        }
        steps += 1;
        assert!(steps < 300, "recovery is not step-bounded");
    }
}

#[test]
fn recovery_epoch_cookie_and_old_hint_are_isolated() {
    let (mut adapter, mut board, script) = ready_adapter();
    // Pre-recovery, a MIN-epoch cookie round-trips (existing contract).
    let old_cookie = TxCookie::new(7);
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, old_cookie).unwrap();
    board.wire.drain_board_tx();
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(old_cookie));

    // One confirmed recovery: the adapter epoch advances to 1.
    run_recovery(&mut adapter);
    let epoch1 = QueueEpoch::MIN.advance().unwrap();
    assert_eq!(
        adapter.recovery_control().unwrap().progress(),
        RecoveryProgress { stage: RecoveryStage::Idle, epoch: epoch1 },
        "the consumed machine reports Idle under the new epoch"
    );

    // A stale (old-epoch) cookie is refused before any device access.
    let writes_at = script.reg_writes().len();
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    assert!(matches!(
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(9)),
        Err(DevError::BadState)
    ));
    assert_eq!(
        script.reg_writes().len(),
        writes_at,
        "the stale cookie never reached the device"
    );
    // A future-epoch cookie is refused the same way.
    let future = epoch1.advance().unwrap();
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    assert!(matches!(
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::with_epoch(future, 10)),
        Err(DevError::BadState)
    ));
    assert_eq!(script.reg_writes().len(), writes_at);

    // A current-epoch cookie rides the reused slot 0 and returns exactly
    // once; a duplicate reclaim fabricates nothing.
    let cookie = TxCookie::with_epoch(epoch1, 4242);
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, cookie).unwrap();
    board.wire.drain_board_tx();
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(cookie));
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), None);

    // An old work hint after the recovery is only a recheck of the CURRENT
    // heads: no false work, owner table untouched, ledger conserved.
    let control = adapter.queue_control().unwrap();
    assert_eq!(
        control.completion_pending(NetQueueDirection::BOTH).unwrap(),
        NetQueueDirection::NONE
    );
    let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
    assert_eq!(ledger.buffer_available + ledger.buffer_inflight, 16);
    assert_eq!(ledger.descriptor_available + ledger.descriptor_inflight, 16);
}

#[test]
fn reset_failure_preserves_carrier_ledger_and_drop() {
    let mut board = Board::new();
    board.env.set_phy_link_1000fd();
    let published = Arc::new(AtomicBool::new(false));
    let mut drv = Gmac::new_async(board.wire.tracking_backing(published.clone())).expect("arena");
    drv.init_rx(&mut board.env).expect("init");
    drv.enable_tx(&mut board.env);
    let script = board.env.handle();
    let mut adapter = K3NetAdapter::new(drv, board.env.handle()).expect("adapter");

    // One in-flight TX carrier (N=1) when the reset is requested.
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(1)).unwrap();

    // Stuck SWR: the stop never confirms; the test drives the clock past
    // the absolute 1 s window in bounded steps.
    script.set_swr_behavior(0, true);
    let mut pendings = 0usize;
    {
        let control = adapter.recovery_control().unwrap();
        let prog = control.begin_recovery().expect("begin from healthy");
        assert_eq!(prog.stage, RecoveryStage::Resetting);
        loop {
            match control.poll_recovery_step() {
                Ok(RecoveryProgress { stage: RecoveryStage::Resetting, .. }) => {
                    pendings += 1;
                    script.advance_clock(100_000_000);
                }
                Ok(other) => panic!("expected the reset to fail, got {other:?}"),
                Err(_) => break,
            }
            assert!(pendings < 50, "the absolute deadline never fired");
        }
    }
    assert!(pendings >= 1, "the window held Pending before the deadline");

    // The failed machine reports Faulted and keeps the conservative
    // device-owned classification (the stop was never confirmed).
    let prog = adapter.recovery_control().unwrap().progress();
    assert_eq!(prog.stage, RecoveryStage::Faulted);
    let summary = adapter.recovery_control().unwrap().owner_summary();
    assert_eq!(
        (summary.available, summary.device_owned, summary.quarantined),
        (15, 17, 0),
        "unconfirmed stop: the TX carrier stays device-owned"
    );
    // Reclaim and submit refuse with the stable Io.
    assert!(matches!(adapter.tx_queue().unwrap().reclaim_tx(), Err(DevError::Io)));
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    assert!(matches!(
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::with_epoch(prog.epoch, 2)),
        Err(DevError::Io)
    ));
    // The published backing survives the failure and the adapter drop.
    assert!(!published.load(Ordering::SeqCst));
    drop(adapter);
    assert!(
        !published.load(Ordering::SeqCst),
        "a failed reset retains the backing until external reset"
    );
}

#[test]
fn reinit_failure_never_activates_partial_queue() {
    let (mut adapter, _board, script) = ready_adapter();
    // One in-flight carrier: the confirmed stop will quarantine it.
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(1)).unwrap();
    // Stall every cache operation past the 10 ms refill phase bound: the
    // first rebuild refill unit fails its pre-publish checkpoint.
    script.set_cache_cost(11_000_000);

    let mut saw_reinit = false;
    {
        let control = adapter.recovery_control().unwrap();
        control.begin_recovery().expect("begin from healthy");
        let mut steps = 0usize;
        loop {
            match control.poll_recovery_step() {
                Ok(RecoveryProgress { stage: RecoveryStage::Resetting, .. }) => {}
                Ok(RecoveryProgress { stage: RecoveryStage::Reinitializing, .. }) => {
                    saw_reinit = true
                }
                Ok(RecoveryProgress { stage: RecoveryStage::Recovered, .. }) => {
                    panic!("a failed rebuild never publishes Running")
                }
                Ok(other) => panic!("unexpected stage {other:?}"),
                Err(_) => break,
            }
            steps += 1;
            assert!(steps < 300, "rebuild is not step-bounded");
        }
    }
    assert!(saw_reinit, "the confirmed boundary was reached before the failure");

    let prog = adapter.recovery_control().unwrap().progress();
    assert_eq!(prog.stage, RecoveryStage::Faulted);
    let summary = adapter.recovery_control().unwrap().owner_summary();
    assert_eq!(
        (summary.available, summary.device_owned, summary.quarantined),
        (15, 16, 1),
        "confirmed stop: the old carrier is quarantined, RX stays device-owned"
    );
    // The partial queue never became visible: every data-plane entry
    // refuses, reclaim reports the stable Io.
    assert!(!adapter.can_transmit());
    assert!(!adapter.can_receive());
    assert!(matches!(adapter.tx_queue().unwrap().reclaim_tx(), Err(DevError::Io)));
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    assert!(matches!(
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::with_epoch(prog.epoch, 3)),
        Err(DevError::Io)
    ));
}

#[test]
fn repeated_recovery_preserves_capacity() {
    let (mut adapter, mut board, _script) = ready_adapter();
    // Exercise the data plane, then drain it fully before recovering.
    for i in 0..3u64 {
        let buf = adapter.alloc_tx_buffer(60).unwrap();
        adapter.tx_queue().unwrap().submit_tx(buf, TxCookie::new(i)).unwrap();
        board.wire.drain_board_tx();
        assert_eq!(
            adapter.tx_queue().unwrap().reclaim_tx().unwrap(),
            Some(TxCookie::new(i))
        );
    }
    board.wire.inject_rx_frame(&frame(60));
    let rx = adapter.receive().unwrap();
    adapter.recycle_rx_buffer(rx).unwrap();
    let rx_ll0 = board.env.get_reg(DMA_RX_DESC_LL);

    for round in 1..=2u64 {
        let target = run_recovery(&mut adapter);
        assert_eq!(target.current(), round, "each recovery advances one epoch");
        // The fixed capacity is fully conserved after every recovery.
        let summary = adapter.recovery_control().unwrap().owner_summary();
        assert_eq!(
            (summary.available, summary.device_owned, summary.quarantined),
            (16, 16, 0),
            "round {round}: idle recovery quarantines nothing"
        );
        let ledger = adapter.tx_queue().unwrap().tx_resource_ledger().unwrap();
        assert_eq!(ledger.buffer_available, 16);
        assert_eq!(ledger.descriptor_inflight, 0);
        // Same arena: the rebuilt RX ring base names the original PA.
        assert_eq!(
            board.env.get_reg(DMA_RX_DESC_LL),
            rx_ll0,
            "round {round}: rebuilt on the same arena"
        );
        // The reused slots carry new-epoch traffic.
        let cookie = TxCookie::with_epoch(target, 100 + round);
        let buf = adapter.alloc_tx_buffer(60).unwrap();
        adapter.tx_queue().unwrap().submit_tx(buf, cookie).unwrap();
        board.wire.drain_board_tx();
        assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(cookie));
    }
    assert_eq!(
        adapter.recovery_control().unwrap().progress().stage,
        RecoveryStage::Idle,
        "the machine is consumed after every recovery"
    );
}

// ---------------------------------------------------------------------------
// MS13 Iteration 001 T5: runtime PHY link snapshots over the stepped MDIO
// ---------------------------------------------------------------------------

/// Number of MAC_CONFIG writes in the script trace (the ability application
/// witness: a changed-to-up snapshot applies the decode once, a same-value
/// snapshot never does).
fn mac_config_writes(script: &FakeEnv) -> usize {
    script
        .reg_writes()
        .iter()
        .filter(|(off, _)| *off == MAC_CONFIG_OFF)
        .count()
}

/// Drives one stepped link snapshot to completion. The machine's
/// trigger/busy/data steps report `Again` until the MDIO engine completes;
/// the fake clock advances one cadence step between retries.
fn read_link_complete(
    adapter: &mut K3NetAdapter<LeakBacking, FakeEnv>,
    script: &FakeEnv,
) -> Result<bool, DevError> {
    let control = adapter.recovery_control().expect("recovery control");
    for _ in 0..32 {
        let outcome = control.read_link_status();
        if !matches!(outcome, Err(DevError::Again)) {
            return outcome;
        }
        script.advance_clock(1_000_000);
    }
    panic!("the link snapshot is not step-bounded");
}

#[test]
fn initial_and_repeated_phy_link_snapshot() {
    let (mut adapter, mut board, script) = ready_adapter();
    let epoch0 = adapter.recovery_control().unwrap().progress().epoch;

    // The first snapshot reports the boot link; the init decode already
    // applied it, so a same-value runtime snapshot must not re-apply.
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);
    let applies_after_first = mac_config_writes(&script);
    // Repeated snapshots of the same value dedup: no ability re-application.
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);
    assert_eq!(
        mac_config_writes(&script),
        applies_after_first,
        "a same-value snapshot dedups: no repeated MAC_CONFIG application"
    );

    // A down transition is reported and never applies anything.
    board.env.set_phy_link(false);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), false);
    assert_eq!(mac_config_writes(&script), applies_after_first);

    // A changed-to-up snapshot applies the decoded ability exactly once.
    board.env.set_phy_link(true);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);
    assert_eq!(
        mac_config_writes(&script),
        applies_after_first + 1,
        "the up transition applies the decoded ability once"
    );

    // The whole snapshot sequence never touches the QueueEpoch.
    assert_eq!(
        adapter.recovery_control().unwrap().progress().epoch,
        epoch0,
        "link snapshots never advance the QueueEpoch"
    );
}

#[test]
fn link_down_up_preserves_queue_epoch() {
    let (mut adapter, mut board, script) = ready_adapter();
    let epoch0 = adapter.recovery_control().unwrap().progress().epoch;
    let summary_of = |a: &mut K3NetAdapter<LeakBacking, FakeEnv>| {
        let s = a.recovery_control().unwrap().owner_summary();
        (s.available, s.device_owned, s.quarantined)
    };
    let sum0 = summary_of(&mut adapter);

    // One complete down→up cycle.
    board.env.set_phy_link(false);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), false);
    board.env.set_phy_link(true);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);

    assert_eq!(
        adapter.recovery_control().unwrap().progress().epoch,
        epoch0,
        "a down→up cycle never advances the QueueEpoch"
    );
    assert_eq!(summary_of(&mut adapter), sum0, "the ledger is untouched");

    // The data plane still submits and completes under the same epoch.
    let cookie = TxCookie::with_epoch(epoch0, 77);
    let buf = adapter.alloc_tx_buffer(60).unwrap();
    adapter.tx_queue().unwrap().submit_tx(buf, cookie).unwrap();
    board.wire.drain_board_tx();
    assert_eq!(adapter.tx_queue().unwrap().reclaim_tx().unwrap(), Some(cookie));
}

#[test]
fn mdio_phase_timeout_preserves_owner() {
    let (mut adapter, _board, script) = ready_adapter();
    let sum0 = {
        let s = adapter.recovery_control().unwrap().owner_summary();
        (s.available, s.device_owned, s.quarantined)
    };

    // A stuck MDIO engine: the busy bit never clears. The read holds a
    // bounded `Again` until the absolute phase deadline, then reports the
    // stable `Io`.
    script.set_mdio_behavior(0, true);
    let control = adapter.recovery_control().unwrap();
    let mut agains = 0usize;
    let outcome = loop {
        match control.read_link_status() {
            Err(DevError::Again) => {
                agains += 1;
                script.advance_clock(1_000_000);
            }
            other => break other,
        }
        assert!(agains < 50, "the link-read deadline never fired");
    };
    assert!(matches!(outcome, Err(DevError::Io)));
    assert!(
        agains >= 1,
        "the busy phase held bounded `Again` before the deadline"
    );

    // The failed read left the owner ledger intact.
    let sum_after = {
        let s = adapter.recovery_control().unwrap().owner_summary();
        (s.available, s.device_owned, s.quarantined)
    };
    assert_eq!(sum_after, sum0, "the MDIO phase timeout preserves the owner");

    // The machine is back to Idle: after the engine is repaired the next
    // snapshot completes from a fresh trigger.
    script.set_mdio_behavior(0, false);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);
}

#[test]
fn link_read_consumes_the_latched_insr_cause() {
    // MS13 rework (INTS/INTN ack): the RTL8211F INTN output is a level,
    // deasserted only by an INSR read; the ISR stays MDIO-free (MS10), so
    // the owner's link-read path must consume the latched cause — otherwise
    // INTN holds low after the first event and no later link transition can
    // ever produce a pad edge again (the 2026-10-14 board session's
    // `phy60=events=0` silence).
    let (mut adapter, mut board, script) = ready_adapter();
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);

    // A link-down transition latches INSR bit4 (the pad edge fired and the
    // handler published the CONFIG cause; the owner now re-reads link).
    board.env.set_phy_link(false);
    board.env.set_phy_insr(k3_async_net_host::phy_probe::PHY_INSR_LINK_CHANGE);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), false);
    assert_eq!(
        board.env.phy_insr(),
        0,
        "the owner's link read consumes the latched INSR cause (INTN returns high)"
    );

    // A fresh event re-latches and is consumed by the next read the same
    // way: every link read leaves the event line re-armed.
    board.env.set_phy_link(true);
    board.env.set_phy_insr(k3_async_net_host::phy_probe::PHY_INSR_LINK_CHANGE);
    assert_eq!(read_link_complete(&mut adapter, &script).unwrap(), true);
    assert_eq!(
        board.env.phy_insr(),
        0,
        "every link read leaves INTN re-armed for the next transition"
    );
}
