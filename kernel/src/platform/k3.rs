//! SpacemiT K3 Com260 IFX platform descriptor (compile-time only).
//!
//! Facts come from the MS09 Iteration 001 board adjudication (change
//! `ms09-com260-kit-observable-link-baseline`, Iteration 001 Act Response,
//! tasks 2.1–2.3, `evidence/001-board-ram-boundary/`) cross-checked with the
//! shipped `k3_com260_ifx.dtb`:
//! - UART0: 0xd4017000, reg-shift=2 (stride 4), reg-io-width=4 (U32 access);
//!   clock/reset/pinctrl stay in the bootloader handoff state.
//! - Safe RAM window [0x160000000, 0x2f0000000): above the CMA pool
//!   (0x140000000 + 512 MiB), below SWIOTLB (0x2f0e00000), the framebuffer
//!   reservation (0x2fe000000) and the observed U-Boot working area
//!   (>= 0x2f9700000). Kernel payload >= 0x180000000.
//! - Boot: U-Boot FIT from RAM; recovery path = normal reset back to the
//!   stock system.
//! - No PLIC/AIA base is asserted: MS09 keeps IRQ delivery fail-closed
//!   (`irq-if` stub only); APLIC/IMSIC addresses are recorded facts, not
//!   wired.

use super::{
    console::{ConsoleConfig, ConsoleKind, MmioAccessWidth},
    descriptor::{
        BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
        PlatformDescriptor, TimerConfig,
    },
    early_console::{DwApbUart32EarlyConsole, EarlyConsole},
};

/// SpacemiT K3 Com260 IFX platform descriptor.
pub const K3: PlatformDescriptor = PlatformDescriptor {
    name: "spacemit-k3-com260-ifx",
    memory: MemoryLayout {
        base_paddr: 0x1_6000_0000,
        size: 0x1_9000_0000, // adjudicated safe window, not total DRAM
    },
    kernel: KernelImageLayout {
        load_paddr: 0x1_8000_0000,
        link_vaddr: 0xffff_ffc1_8000_0000,
    },
    console: ConsoleConfig {
        kind: ConsoleKind::DwApbUart,
        base_paddr: 0xd401_7000,
        irq: None, // polling-only first-byte path; IRQ delivery stays fail-closed
        reg_stride: 4,
        reg_width: MmioAccessWidth::U32,
        baud: 115200,
    },
    interrupt: InterruptConfig {
        // 0 is the explicit "not brought up" sentinel: no PLIC/AIA base is
        // asserted for MS09 (fail-closed irq-if stub only).
        plic_base_paddr: 0,
    },
    timer: TimerConfig { kind: "sbi" },
    boot: BootImageConfig {
        kind: BootKind::UBootImage,
    },
    virtio_net: None,
};

/// K3 first-byte parking path: staged polling markers, then wfi.
///
/// Runs after axruntime early init. Every marker goes through the polling
/// early console so it does not depend on the log level or the async stack.
/// Combined with the `[k3:pt-mmu]` marker emitted by the axplat boot code,
/// the serial wire shows (3.3 staged observation):
/// - nothing at all: boot did not jump, the boot page table faulted, or
///   UART0 clock/pinmux is dead;
/// - `[k3:pt-mmu]` only: page table + MMU + UART MMIO alive, but axruntime
///   or kernel entry init failed;
/// - stages 1..3: kernel entry reached, K3 descriptor dispatch confirmed
///   (printed facts), controlled park entered;
/// - stage 4 (MS09 Iteration 003 rework): APMU GMAC bus clock-enable
///   sequence lines (begin / raw readbacks / done). `apmu begin` without
///   following lines means the APMU access faulted (mapping layer);
///   readback effectiveness is reported by the stage-6 gate;
/// - stage 5 (MS09 Iteration 003): read-only target-MAC identity/status
///   probe lines (debug zero is reported as `sem=idle`, not a fault);
///   `probe begin` without register lines after it falsifies the
///   clock-gating assumption;
/// - stage 6 (MS09 Iteration 004, task 5.1): a gate line carrying the APMU
///   clock/reset effectiveness and the MAC identity class — identity
///   all-zero/all-one or ineffective APMU bits stop every later stage; then
///   the read-only IO-PD voltage check, the minimal gmac1 pad-mux RMW and
///   exactly one GPIO1_5 PHY hardware reset (`pad/gpio begin` / `done`);
/// - stage 7 (MS09 Iteration 004, task 5.1): bounded MDIO reads of PHYID1/2
///   at address 1 plus BMCR/BMSR status with the anomaly checks
///   (`mdio/phy begin` / `done`; timeouts and identity failures stop);
/// - stage 8 (MS09 Iteration 004, task 5.2): one AN restart, read-only
///   advertisement dump and the fixed link observation window (30 s since
///   the 2026-10-03 operator instruction, originally 120 s); the kernel
///   never judges PASS.
///
/// # Safety
///
/// The caller MUST ensure the UART0 MMIO window is mapped (identity mapping
/// of the `devices.mmio-ranges` window by axruntime) and UART0 clock/pins
/// are in the bootloader handoff state (MS09 Iteration 001 adjudication).
pub unsafe fn run_first_byte_park() -> ! {
    let desc = super::descriptor();
    // SAFETY: K3 UART0 is in the bootloader handoff state (see contract above).
    let console = unsafe { DwApbUart32EarlyConsole::from_config(&desc.console) };

    console.write_str("[starry-k3] stage=1 entry-reached\n");
    console.write_str(
        "[starry-k3] stage=2 console uart=0xd4017000 stride=4 width=u32 baud=115200\n",
    );
    console.write_str("[starry-k3] stage=3 parked, halting (reset to recover)\n");

    let mut env = K3Env;
    let apmu = super::mac_probe::apmu_clock_enable(&mut env, |line| console.write_str(line));
    let mac = super::mac_probe::probe_and_report(&mut env, |line| console.write_str(line));

    // PHY-stage gate (5.1): identity all-zero/all-one or ineffective APMU
    // clock/reset bits stop every later stage; debug=0 (idle) never does.
    let (clk_ok, rst_ok) = super::mac_probe::apmu_bits_effective(apmu.after1, apmu.after2);
    let version_ok = mac.version_identity_ok();
    let version_label = if version_ok {
        "ok"
    } else {
        super::mac_probe::class_label(mac.version_classes[0])
    };
    let mut buf = [0u8; super::mac_probe::READ_LINE_BUF_LEN];
    console.write_str(super::mac_probe::format_gate_line(
        &mut buf,
        clk_ok,
        rst_ok,
        version_label,
        clk_ok && rst_ok && version_ok,
    ));
    if !(clk_ok && rst_ok && version_ok) {
        park();
    }

    match super::phy_probe::run_pad_gpio(&mut env, |line| console.write_str(line)) {
        super::phy_probe::PadGpioOutcome::Done => {}
        super::phy_probe::PadGpioOutcome::Stopped(_) => park(),
    }
    match super::phy_probe::run_phy_identity(&mut env, |line| console.write_str(line)) {
        super::phy_probe::PhyStatus::Ok => {}
        super::phy_probe::PhyStatus::Stopped(_) => park(),
    }
    // Stage 8 (5.2): one AN restart, advertisement dump and the fixed link
    // observation window (30 s); it reports and ends by itself, then we
    // continue to the AIA stages.
    super::phy_probe::run_link_observation(&mut env, |line| console.write_str(line));
    // Stage 9 (MS10 Iter 000, task 1.3): bounded AIA software-injection
    // probe on the same RAM boot path. The RJ45 stays stable — stage 9 is
    // pure software delivery (Detached source 1 + GENMSI) and does not
    // touch the PHY/MDIO state of stages 6–8. Outcome (pass/fail layer) is
    // reported on the serial wire by the probe itself.
    #[cfg(feature = "k3")]
    if let super::aia_probe::Stage9Outcome::Passed(_) =
        super::aia_probe::run_k3_stage9(|line| console.write_str(line))
    {
        // Stage 10 (MS10 Iter 001, tasks 2.1–2.2): 120 s device window after
        // a healthy controller base. The operator unplugs/replugs the RJ45
        // during the window; the handler snapshots every destructive cause
        // before the pad ack and the foreground prints claim lines. The
        // kernel never judges PASS (task 2.3 adjudicates from this wire).
        let st10 = super::k3_device_irq_probe::run_k3_stage10(|line| console.write_str(line));
        // MS12 Iteration 001 (T4): the async-net mode installs the K3
        // adapter as the only NIC and starts the single queue owner (design
        // D4); it parks internally. The raw IRQ session stays the
        // diagnostic-only path and the polling stages 11–13 stay compiled
        // but are never entered.
        #[cfg(feature = "k3-async-net")]
        super::k3_async_net::run_k3_async_net_service(st10, |line| console.write_str(line));
        #[cfg(not(feature = "k3-async-net"))]
        {
            let st11 = super::k3_network_probe::run_k3_stage11(st10, |line| console.write_str(line));
            let st12 = super::k3_network_probe::run_k3_stage12(st11, |line| console.write_str(line));
            super::k3_network_probe::run_k3_stage13(st12, |line| console.write_str(line));
        }
    }
    park();
}

/// Parks the boot hart until reset (the recovery path is the stock system).
fn park() -> ! {
    loop {
        riscv::asm::wfi();
    }
}

/// One-shot fault snapshot (rework T2-R2; T2-R4 adds the full error counts
/// and the caller-held RX accumulation): the core diagnostic surface in a
/// single shared-guard batch (one read per register) plus the APLIC/EIP
/// observation. Printed once per RX deadline fault branch, right before
/// parking; never runs on the data-plane paths. `acc` is memory data and
/// outside the one-read-per-register discipline.
#[cfg(all(target_arch = "riscv64", feature = "k3-async-uart"))]
fn fault_snapshot(port: &crate::drivers::k3_uart::K3UartPort, stage: &str, acc: &[u8]) {
    let snap = port.core.diag_snapshot(&port.hw);
    crate::drivers::k3_uart::diag_line(&alloc::format!(
        "[ms17] fault-snap stage={stage} irq={} rx_wakes={} tx_wakes={} unknown={} oe={} pe={} fe={} bi={} fifoerr={} closed={} ier_rb={:#x} ier_cache={:#x} iir={:#x} lsr={:#x} dr={} acc_len={} acc=\"{}\"{}",
        snap.counts.irq_count,
        snap.counts.rx_wakes,
        snap.counts.tx_wakes,
        snap.counts.unknown,
        snap.counts.overruns,
        snap.counts.parity_errors,
        snap.counts.framing_errors,
        snap.counts.break_errors,
        snap.counts.fifo_errors,
        snap.notifications_closed,
        snap.ier_readback,
        snap.ier_cache,
        snap.iir,
        snap.lsr,
        snap.lsr_dr,
        acc.len(),
        escape_snapshot_acc(&acc[..acc.len().min(64)]),
        if acc.len() > 64 { " +trunc" } else { "" },
    ));
    let (pending42, eip0) = crate::drivers::k3_uart::aia_pending42_and_eip();
    crate::drivers::k3_uart::diag_line(&alloc::format!(
        "[ms17] fault-snap stage={stage} aplic_pending42={} eip0={eip0:#x}",
        pending42
    ));
}

/// Escapes control and non-ASCII bytes as `\xNN` so the deadline snapshot
/// shows exactly which bytes the terminal sent (rework T2-R4). Printable
/// ASCII passes through; the caller caps the input at 64 bytes.
#[cfg(all(target_arch = "riscv64", feature = "k3-async-uart"))]
fn escape_snapshot_acc(bytes: &[u8]) -> alloc::string::String {
    let mut out = alloc::string::String::new();
    for &b in bytes {
        match b {
            0x20..=0x7e => out.push(b as char),
            _ => out.push_str(&alloc::format!("\\x{b:02x}")),
        }
    }
    out
}

/// Strips trailing `\r`/`\n` bytes for the expected-content comparison
/// (rework T2-R4): the terminator is qualification-payload accepted, not
/// part of the line content.
#[cfg(all(target_arch = "riscv64", feature = "k3-async-uart"))]
fn strip_trailing_crlf(b: &[u8]) -> &[u8] {
    let mut end = b.len();
    while end > 0 && (b[end - 1] == b'\r' || b[end - 1] == b'\n') {
        end -= 1;
    }
    &b[..end]
}

// ---------------------------------------------------------------------------
// MS17 Iteration 000: async UART bring-up (timer proof + raw port
// qualification). Only compiled for the `k3-async-uart` mode; the polling
// network stages above are never entered there.
// ---------------------------------------------------------------------------

/// K3 async UART bring-up: B2 timer stage (two real sleeps + one timeout
/// cancellation over native futures) then the B3 raw PXA port qualification
/// (two distinct lines + one isolated byte, each with a 30 s outer deadline).
/// No GMAC/PHY/link stages run; the kernel never judges PASS — the operator
/// adjudicates from this serial wire, and reset returns to the stock system.
#[cfg(all(target_arch = "riscv64", feature = "k3-async-uart"))]
pub fn run_uart_port_bringup() -> ! {
    use core::time::Duration;

    use alloc::boxed::Box;
    use alloc::format;
    use alloc::vec::Vec;
    use axtask::future::{block_on, sleep, timeout};

    use crate::drivers::k3_uart;
    let diag = |line: &str| k3_uart::diag_line(line);

    diag("[ms17] mode=k3-uart-port-test begin uart=0xd4017000 irq=42 smp=1");

    // ── B2 timer stage: two real sleeps under a 1 s outer deadline ──
    let t0 = axhal::time::monotonic_time_nanos();
    let sleeps = block_on(timeout(
        Some(Duration::from_secs(1)),
        async {
            sleep(Duration::from_millis(50)).await;
            sleep(Duration::from_millis(100)).await;
        },
    ));
    let elapsed_ms = (axhal::time::monotonic_time_nanos() - t0) / 1_000_000;
    match sleeps {
        Ok(()) => diag(&format!(
            "[ms17] timer result=sleeps_ok expect_ms=150 elapsed_ms={elapsed_ms}"
        )),
        Err(_) => {
            diag("[ms17] timer fault stage=sleep deadline=1s");
            park();
        }
    }

    // ── B2 timer stage: timeout cancels the inner wait ──────────────
    let t1 = axhal::time::monotonic_time_nanos();
    let cancel = block_on(timeout(
        Some(Duration::from_millis(5)),
        sleep(Duration::from_millis(30)),
    ));
    let cancel_ms = (axhal::time::monotonic_time_nanos() - t1) / 1_000_000;
    match cancel {
        Err(_) if cancel_ms < 30 => diag(&format!(
            "[ms17] timer result=timeout_cancelled elapsed_ms={cancel_ms} inner=30ms-sleep"
        )),
        Ok(()) => {
            diag("[ms17] timer fault stage=timeout inner wait ran to completion");
            park();
        }
        Err(_) => {
            diag("[ms17] timer fault stage=timeout elapsed past the inner wait");
            park();
        }
    }

    // ── B3 port stages ──────────────────────────────────────────────
    let port = k3_uart::port();
    diag("[ms17] uart stage=activate irq=42");
    match k3_uart::activate_port() {
        Ok(()) => diag("[ms17] uart activate=ready (notifications masked; tasks arm next)"),
        Err(e) => {
            diag(&format!("[ms17] uart fault stage=activate err={e:?}"));
            park();
        }
    }

    // Two distinct input lines, each returned and echoed back asynchronously.
    // The caller owns the accumulation (rework T2-R4): a deadline
    // cancellation drops the future but leaves the received bytes visible
    // for the fault snapshot. The line predicate accepts CR, LF or CRLF as
    // the terminator (qualification-payload semantics only; tty CR/LF
    // translation is Iteration 002's).
    for (stage, expect) in [(1usize, &b"ms17-a\n"[..]), (2usize, &b"ms17-b\n"[..])] {
        let mut acc: Vec<u8> = Vec::new();
        let received = block_on(axtask::future::timeout(
            Some(Duration::from_secs(30)),
            k3_uart::raw_rx_stage_into(
                port,
                &mut acc,
                crate::drivers::k3_uart_core::raw_line_complete,
            ),
        ));
        match received {
            Ok(()) => {
                let text = core::str::from_utf8(&acc).unwrap_or("<non-utf8>");
                diag(&format!("[ms17] uart stage={stage} received={text:?}"));
                let echo = block_on(axtask::future::timeout(
                    Some(Duration::from_secs(30)),
                    k3_uart::raw_tx_send(port, &acc),
                ));
                match echo {
                    Ok(()) => diag(&format!("[ms17] uart stage={stage} echo=done bytes={}", acc.len())),
                    Err(_) => {
                        diag(&format!("[ms17] uart fault stage={stage} echo deadline=30s"));
                        park();
                    }
                }
                // The expected content is compared without the trailing
                // CR/LF, so `received="ms17-a\r"` against the expected
                // `ms17-a\n` no longer reports a content mismatch.
                if strip_trailing_crlf(&acc) != strip_trailing_crlf(expect) {
                    diag(&format!(
                        "[ms17] uart stage={stage} note=content-mismatch expected={:?}",
                        core::str::from_utf8(expect).unwrap_or("<bytes>")
                    ));
                }
            }
            Err(_) => {
                diag(&format!("[ms17] uart fault stage={stage} rx deadline=30s"));
                fault_snapshot(port, &format!("{stage}"), &acc);
                park();
            }
        }
    }

    // Isolated-byte stage: exactly one byte, then no further input.
    let isolated = block_on(axtask::future::timeout(
        Some(Duration::from_secs(30)),
        k3_uart::raw_rx_one_byte(port),
    ));
    match isolated {
        Ok(data) if data.len() == 1 => {
            diag(&format!(
                "[ms17] uart stage=isolated received={:?}", data[0] as char
            ));
            let echo = block_on(axtask::future::timeout(
                Some(Duration::from_secs(30)),
                k3_uart::raw_tx_send(port, &data),
            ));
            match echo {
                Ok(()) => diag("[ms17] uart stage=isolated echo=done bytes=1"),
                Err(_) => {
                    diag("[ms17] uart fault stage=isolated echo deadline=30s");
                    park();
                }
            }
        }
        Ok(data) => {
            diag(&format!(
                "[ms17] uart fault stage=isolated received_bytes={} expect=1",
                data.len()
            ));
            park();
        }
        Err(_) => {
            diag("[ms17] uart fault stage=isolated rx deadline=30s");
            // The one-byte predicate completes on the first byte, so a
            // deadline here means zero bytes arrived: the accumulation is
            // empty by construction.
            fault_snapshot(port, "isolated", &[]);
            park();
        }
    }

    let counts = port.core.counts();
    diag(&format!(
        "[ms17] uart counts irq={} rx_wakes={} tx_wakes={} unknown={} oe={} pe={} fe={} bi={} fifoerr={} closed={}",
        counts.irq_count,
        counts.rx_wakes,
        counts.tx_wakes,
        counts.unknown,
        counts.overruns,
        counts.parity_errors,
        counts.framing_errors,
        counts.break_errors,
        counts.fifo_errors,
        port.core.notifications_closed(),
    ));
    diag("[ms17] qualification complete park (reset to recover)");
    park()
}

// ---------------------------------------------------------------------------
// MS17 Iteration 001: async UART data-plane qualification (copiers + rings).
// Only compiled for the `k3-uart` data-plane mode; tty/userland are
// Iterations 002+ and the GMAC/network stages are never entered here.
// ---------------------------------------------------------------------------

/// Long-line acceptance limit for the qualification echo (the payload's
/// bounded accumulation; the 64 KiB RX ring sits far above it).
#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-uart",
    any(not(feature = "k3-stdio"), feature = "k3-smp-uart-raw")
))]
const PLANE_LINE_LIMIT: usize = 512;

/// TX Full burst size: one full 64 KiB TX ring plus headroom. The wire
/// drains at ~11.5 KiB/s (115200 8N1) while the payload pushes at memory
/// speed, so the ring fills and the short-writes (the Full witness) are
/// guaranteed long before the burst completes.
#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-uart",
    any(not(feature = "k3-stdio"), feature = "k3-smp-uart-raw")
))]
const PLANE_TX_BURST_BYTES: usize = 72 * 1024;

/// Payload line predicate: CR, LF or CRLF terminates; the accumulation also
/// completes at [`PLANE_LINE_LIMIT`].
#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-uart",
    any(not(feature = "k3-stdio"), feature = "k3-smp-uart-raw")
))]
fn plane_line_complete(acc: &[u8]) -> bool {
    matches!(acc.last(), Some(b'\n') | Some(b'\r')) || acc.len() >= PLANE_LINE_LIMIT
}

/// K3 async UART data-plane qualification (task T4; operator-adjudicated):
/// one-shot and what-it-prints-is-the-verdict. Every interactive output
/// flows through the sole `SerializedWriter` producer into the TX ring, the
/// copier and the wire (never a raw THR write); every input is read from
/// the RX ring through the sole reader with the check → register → recheck
/// discipline. Stages: duplicate-activate rejection, ready banner, two
/// echoed lines, one ≥256 B long line, a >64 KiB TX burst (Full →
/// recovery), a bounded flush (tail drain completed through the native
/// timer hook), a 10 s quiet window with post-quiet input, and the
/// error-class counters. The kernel never judges PASS; reset returns to the
/// stock system. The body is verbatim MS17; MS14 Iteration 002 (T7) widens
/// only this cfg so the `k3-smp-uart-raw` sub-form — whose closure implies
/// `k3-stdio` — compiles the payload as its sole RX consumer.
#[cfg(all(
    target_arch = "riscv64",
    feature = "k3-uart",
    any(not(feature = "k3-stdio"), feature = "k3-smp-uart-raw")
))]
pub fn run_uart_data_plane() -> ! {
    use core::future::poll_fn;
    use core::sync::atomic::Ordering;
    use core::task::Poll;
    use core::time::Duration;

    use alloc::boxed::Box;
    use alloc::format;
    use alloc::vec::Vec;
    use axtask::future::{block_on, sleep, timeout};
    use uart_16550::TtyRead;

    use crate::drivers::k3_uart;
    use crate::drivers::k3_uart_core::ActivateError;
    use crate::drivers::uart_init::{
        ArceOsReader, ArceOsWriter, RawArceOsWriter, driver, wait_tx_drain,
    };

    /// Reads one line into `acc` through the ring reader with the
    /// check → register → recheck discipline; completes on CR/LF or at the
    /// payload line limit.
    async fn read_line(reader: &mut ArceOsReader, acc: &mut Vec<u8>) {
        // 单个 poll_fn 即可：Pending 后执行器会带着已注册的 waker 重新轮询
        // 同一 poll_fn。行完成即返回 Ready——不得包外层 loop，否则 future
        // 永不完成（板测实测：acc 无限累积、60s 超时）。
        poll_fn(|cx| {
            let mut chunk = [0u8; 64];
            // check
            let n = reader.read(&mut chunk);
            if n > 0 {
                acc.extend_from_slice(&chunk[..n]);
            }
            if plane_line_complete(acc) {
                return Poll::Ready(());
            }
            // register → recheck
            reader.register_readable_waker(cx.waker());
            let n = reader.read(&mut chunk);
            if n > 0 {
                acc.extend_from_slice(&chunk[..n]);
            }
            if plane_line_complete(acc) {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }

    /// LF → CRLF：数据面 writer 输出的是裸字节流（不经过 diag_line 的翻译
    /// 层），终端需要 CR 才会回到行首，否则出现阶梯状显示（板测实测发现，
    /// Iteration 001 Cycle 内修复）。
    fn emit(writer: &ArceOsWriter, line: &[u8]) -> usize {
        let mut out = Vec::with_capacity(line.len() + 8);
        for &b in line {
            if b == b'\n' {
                out.push(b'\r');
            }
            out.push(b);
        }
        writer.push(&out)
    }

    let port = k3_uart::port();
    let diag = |line: &str| k3_uart::diag_line(line);

    diag("[ms17] mode=k3-uart-dataplane begin uart=0xd4017000 irq=42 smp=1");

    // The sole RX consumer and TX producer for this boot: neither type is
    // Clone, and the payload holds the only instances (owner semantics).
    let reader: &'static mut ArceOsReader = Box::leak(Box::new(unsafe {
        uart_16550::async_::device_ops::AsyncUartReader::new(driver())
    }));
    let writer = ArceOsWriter::new(RawArceOsWriter(unsafe {
        uart_16550::async_::device_ops::AsyncUartWriter::new(driver())
    }));

    // Owner semantics: a duplicate activation must be rejected explicitly
    // and touch nothing (the core CAS fails without a hardware batch).
    match k3_uart::activate_port() {
        Err(ActivateError::AlreadyActive) => {
            emit(&writer, b"[ms17] plane stage=duplicate-activate result=rejected\n");
        }
        other => {
            diag(&format!("[ms17] plane fault stage=duplicate-activate result={other:?}"));
            fault_snapshot(port, "duplicate-activate", &[]);
            park();
        }
    }

    // Ready banner: the first output through ring → copier → wire.
    emit(&writer, b"[ms17] plane stage=ready ring-output=live\n");

    // Two interactive echo lines: input through the RX ring reader, output
    // through the TX ring writer.
    for stage in 1..=2 {
        emit(&writer, format!("[ms17] plane stage=echo{stage} send-one-line\n").as_bytes());
        let mut acc: Vec<u8> = Vec::new();
        match block_on(timeout(Some(Duration::from_secs(60)), read_line(reader, &mut acc))) {
            Ok(()) => {
                let line_len = strip_trailing_crlf(&acc).len();
                acc.extend_from_slice(b"\r\n");
                emit(&writer, &acc);
                emit(&writer,
                    format!("[ms17] plane stage=echo{stage} echo=done len={line_len}\n").as_bytes(),
                );
            }
            Err(_) => {
                diag(&format!("[ms17] plane fault stage=echo{stage} rx deadline=60s"));
                fault_snapshot(port, &format!("echo{stage}"), &acc);
                park();
            }
        }
    }

    // Long input: one line of at least 256 bytes crosses the 64-byte
    // hardware call budget over multiple copier polls, then echoes back.
    emit(&writer, b"[ms17] plane stage=long send-one-line-of-256B-plus\n");
    let mut acc: Vec<u8> = Vec::new();
    match block_on(timeout(Some(Duration::from_secs(60)), read_line(reader, &mut acc))) {
        Ok(()) => {
            let line_len = strip_trailing_crlf(&acc).len();
            acc.extend_from_slice(b"\r\n");
            emit(&writer, &acc);
            if line_len < 256 {
                emit(
                    &writer,
                    format!(
                        "[ms17] plane stage=long received_len={line_len} note=line-too-short expect>=256\n"
                    )
                    .as_bytes(),
                );
            } else {
                emit(
                    &writer,
                    format!("[ms17] plane stage=long received_len={line_len} echo=done\n").as_bytes(),
                );
            }
        }
        Err(_) => {
            diag("[ms17] plane fault stage=long rx deadline=60s");
            fault_snapshot(port, "long", &acc);
            park();
        }
    }

    // TX Full burst: push 72 KiB of NUL bytes through the sole writer.
    // The ring (64 KiB) fills long before the wire drains, so the short
    // writes are the Full witness; the copier drains and the burst recovers.
    emit(&writer,
        format!(
            "[ms17] plane stage=txfull begin bytes={PLANE_TX_BURST_BYTES} pattern=nul-0x00\n"
        )
        .as_bytes(),
    );
    let burst = block_on(timeout(Some(Duration::from_secs(180)), async {
        let mut off = 0usize;
        let mut accepted_total = 0usize;
        let mut short_writes = 0u32;
        let mut chunk = [0u8; 4096];
        while off < PLANE_TX_BURST_BYTES {
            let len = chunk.len().min(PLANE_TX_BURST_BYTES - off);
            for (i, b) in chunk.iter_mut().enumerate().take(len) {
                // NUL(0x00)：与 QEMU 串口基准（bench.rs `vec![0u8;1024]`）
                // 同一形态——终端完全不可见、不滚屏，线路上真实搬运 72 KiB；
                // Full/背压见证由内核账本 requested/accepted/short_writes
                // 承担（用户 2026-10-07 指示）。
                *b = 0u8;
            }
            let accepted = emit(&writer, &chunk[..len]);
            accepted_total += accepted;
            if accepted < len {
                short_writes += 1;
                // Full: back off so the copier can drain the ring.
                sleep(Duration::from_millis(20)).await;
            }
            off += accepted;
        }
        (accepted_total, short_writes)
    }));
    match burst {
        Ok((accepted_total, short_writes)) => {
            if accepted_total != PLANE_TX_BURST_BYTES {
                diag(&format!(
                    "[ms17] plane fault stage=txfull accepted={accepted_total} requested={PLANE_TX_BURST_BYTES}"
                ));
                fault_snapshot(port, "txfull", &[]);
                park();
            }
            emit(&writer, b"\r\n");
            emit(
                &writer,
                format!(
                    "[ms17] plane stage=txfull drained requested={PLANE_TX_BURST_BYTES} accepted={accepted_total} short_writes={short_writes}\n"
                )
                .as_bytes(),
            );
        }
        Err(_) => {
            diag("[ms17] plane fault stage=txfull deadline=180s");
            fault_snapshot(port, "txfull", &[]);
            park();
        }
    }

    // Tail drain: the flush completes only on the four TxCompletion
    // conditions; on K3 the final TEMT wait rides the native 1 ms timer
    // hook, so this marker appears only after the wire is physically silent.
    match block_on(timeout(Some(Duration::from_secs(60)), wait_tx_drain(&driver()))) {
        Ok(()) => {
            emit(&writer, b"[ms17] plane stage=drain done (TxCompletion four conditions)\n");
        }
        Err(_) => {
            diag("[ms17] plane fault stage=drain deadline=60s");
            fault_snapshot(port, "drain", &[]);
            park();
        }
    }

    // Quiet window: 10 s with no input, then a fresh line must wake the RX
    // copier through the armed IRQ immediately.
    emit(&writer, b"[ms17] plane stage=quiet begin=10s (send nothing)\n");
    if block_on(timeout(Some(Duration::from_secs(15)), sleep(Duration::from_secs(10)))).is_err() {
        diag("[ms17] plane fault stage=quiet sleep deadline");
        fault_snapshot(port, "quiet", &[]);
        park();
    }
    emit(&writer, b"[ms17] plane stage=quiet done - send one line now\n");
    let mut acc: Vec<u8> = Vec::new();
    match block_on(timeout(Some(Duration::from_secs(60)), read_line(reader, &mut acc))) {
        Ok(()) => {
            let line_len = strip_trailing_crlf(&acc).len();
            acc.extend_from_slice(b"\r\n");
            emit(&writer, &acc);
            emit(&writer,
                format!("[ms17] plane stage=post-quiet echo=done len={line_len}\n").as_bytes(),
            );
        }
        Err(_) => {
            diag("[ms17] plane fault stage=post-quiet rx deadline=60s");
            fault_snapshot(port, "post-quiet", &acc);
            park();
        }
    }

    // Error classification: the drop/telemetry and core counters are part
    // of the verdict surface (operator records them with the run).
    let drv = driver();
    let counts = port.core.counts();
    emit(&writer,
        format!(
            "[ms17] plane stage=errors rx_dropped_bytes={} tx_no_progress={} tx_hw_bytes={} irq={} rx_wakes={} tx_wakes={} unknown={} oe={} pe={} fe={} bi={} fifoerr={} closed={}\n",
            drv.telemetry.rx_dropped_bytes.load(Ordering::Relaxed),
            drv.telemetry.tx_no_progress.load(Ordering::Relaxed),
            drv.telemetry.tx_hw_bytes.load(Ordering::Relaxed),
            counts.irq_count,
            counts.rx_wakes,
            counts.tx_wakes,
            counts.unknown,
            counts.overruns,
            counts.parity_errors,
            counts.framing_errors,
            counts.break_errors,
            counts.fifo_errors,
            port.core.notifications_closed(),
        )
        .as_bytes(),
    );
    let flush_done = block_on(timeout(Some(Duration::from_secs(30)), wait_tx_drain(&driver())));
    if flush_done.is_err() {
        diag("[ms17] plane fault stage=errors drain deadline=30s");
        fault_snapshot(port, "errors", &[]);
        park();
    }

    diag("[ms17] data-plane qualification complete park (reset to recover)");
    park()
}

/// Board access + native time source for the Iteration 004 probe stages.
///
/// `mmio_*` addresses come exclusively from `mac_probe`/`phy_probe` constants
/// inside the DT-declared windows identity-mapped via `devices.mmio-ranges`
/// (UART/MAC/APMU plus the Iteration 004 GPIO `0xd4019000/0x100` and the
/// single pad page `0xd401e000/0x1000`). Time reads the 24 MHz `time` CSR via
/// axhal; no timer IRQ is required.
pub(crate) struct K3Env;

impl super::phy_probe::ProbeEnv for K3Env {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        // MS14 Iteration 003 (T10): the device windows are reachable only
        // through the platform phys_to_virt mapping under the full paging
        // stage; the translation point is `platform::k3_mmio`.
        super::k3_mmio::read32(paddr)
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        // SAFETY: same window contract as mmio_read; probe code writes only
        // the APMU CTRL register, the APBC ASAR unlock keys and the gmac1
        // IO-PD voltage word, the gmac1 pad mux fields, GPIO1_5 and the
        // MDIO ADDR/DATA registers.
        super::k3_mmio::write32(paddr, value)
    }

    fn now_nanos(&self) -> u64 {
        axhal::time::monotonic_time_nanos()
    }

    fn wait_nanos(&mut self, nanos: u64) {
        axhal::time::busy_wait(core::time::Duration::from_nanos(nanos))
    }
}
