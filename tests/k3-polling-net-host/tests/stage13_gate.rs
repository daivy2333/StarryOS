//! Task 3.2 witness: the stage 13 gate (only a normally ended stage 12
//! window opens the session; every other outcome keeps the board untouched),
//! the failure output semantics (stage + owner) and the trailing-newline
//! output fix for stages 11–13.

use k3_polling_net_host::fake::FakeEnv;
use k3_polling_net_host::k3_device_irq_probe::Stage10Outcome;
use k3_polling_net_host::k3_gmac::polling_net::SessionStorage;
use k3_polling_net_host::k3_gmac::{DMA_CHAN_STATUS, DMA_STATUS_FBE};
use k3_polling_net_host::k3_network_probe::{
    run_stage11, run_stage12, run_stage13, Stage11Outcome, Stage12Outcome, Stage13Outcome,
};

#[test]
fn stage11_gate_line_ends_with_newline() {
    let mut b = FakeEnv::board();
    let mut lines: Vec<String> = Vec::new();
    let outcome = run_stage11(
        &mut b.env,
        Stage10Outcome::Stopped { reason: "x" },
        b.wire.arena_guard(),
        &mut |l| lines.push(l.to_string()),
    );
    assert!(matches!(outcome, Stage11Outcome::SetupFailed { phase: "gate" }));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("stage=11 err=gate"), "{:?}", lines);
    assert!(lines[0].ends_with('\n'), "output lines carry a trailing newline");
}

#[test]
fn stage12_gate_line_ends_with_newline() {
    let mut b = FakeEnv::board();
    let mut lines: Vec<String> = Vec::new();
    let outcome = run_stage12(
        &mut b.env,
        Stage11Outcome::SetupFailed { phase: "gate" },
        b.wire.arena_guard(),
        &mut |l| lines.push(l.to_string()),
    );
    assert!(matches!(outcome, Stage12Outcome::SetupFailed { phase: "gate" }));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("stage=12 err=gate"), "{:?}", lines);
    assert!(lines[0].ends_with('\n'));
}

#[test]
fn stage13_gate_blocks_a_stopped_stage12_and_touches_nothing() {
    let mut b = FakeEnv::board();
    let env_copy = b.env.clone();
    let storage = Box::leak(Box::new(SessionStorage::new()));
    let mut lines: Vec<String> = Vec::new();
    let outcome = run_stage13(
        b.env,
        Stage12Outcome::Stopped { reason: "x" },
        b.wire.arena_guard(),
        storage,
        &mut |l| lines.push(l.to_string()),
    );
    assert!(matches!(outcome, Stage13Outcome::SetupFailed { phase: "gate" }));
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("stage=13 err=gate"), "{:?}", lines);
    assert!(lines[0].ends_with('\n'));
    assert!(
        env_copy.log_snapshot().is_empty(),
        "the gate keeps every GMAC access away: {:?}",
        env_copy.log_snapshot()
    );
}

#[test]
fn stage13_fatal_error_names_the_stage_and_owner() {
    let mut b = FakeEnv::board();
    b.env.set_phy_link_1000fd();
    // FBE armed before the run: the first device entry must surface it.
    b.env.set_reg(DMA_CHAN_STATUS, DMA_STATUS_FBE);
    let storage = Box::leak(Box::new(SessionStorage::new()));
    let mut lines: Vec<String> = Vec::new();
    let outcome = run_stage13(
        b.env,
        Stage12Outcome::WindowEnded(Default::default()),
        b.wire.arena_guard(),
        storage,
        &mut |l| lines.push(l.to_string()),
    );
    assert!(matches!(outcome, Stage13Outcome::Fault { .. }), "{outcome:?}");
    let fault_line = lines
        .iter()
        .find(|l| l.contains("stage=13") && l.contains("err=fbe"))
        .expect("fault line names the fatal stage");
    assert!(fault_line.contains("owner="), "the fault line names its owner: {fault_line}");
    assert!(lines.iter().all(|l| l.ends_with('\n')), "{lines:?}");
}

#[test]
fn stage13_idle_window_reports_a_zero_summary() {
    let mut b = FakeEnv::board();
    b.env.set_phy_link_1000fd();
    let storage = Box::leak(Box::new(SessionStorage::new()));
    let mut lines: Vec<String> = Vec::new();
    let outcome = run_stage13(
        b.env,
        Stage12Outcome::WindowEnded(Default::default()),
        b.wire.arena_guard(),
        storage,
        &mut |l| lines.push(l.to_string()),
    );
    let summary = match outcome {
        Stage13Outcome::WindowEnded(summary) => summary,
        other => panic!("expected WindowEnded, got {other:?}"),
    };
    assert!(summary.window_elapsed_nanos >= 59_000_000_000, "the fixed 60 s window ran");
    assert_eq!(summary.counters.rx_frames, 0);
    assert_eq!(summary.counters.tx_completed, 0);
    assert_eq!(summary.counters.icmp_echo_replies, 0);
    assert_eq!(summary.counters.udp_echoed, 0);
    assert_eq!(summary.counters.tcp_echoed, 0);
    let result = lines.iter().find(|l| l.contains("stage=13 result")).expect("summary line");
    assert!(result.contains("icmp-replies=0"), "{result}");
    assert!(result.contains("udp-echo=0"), "{result}");
    assert!(result.contains("tcp-echo=0"), "{result}");
    assert!(lines.iter().all(|l| l.ends_with('\n')), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("stage=13 init")), "{lines:?}");
}
