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
    // SAFETY: MAC window is identity-mapped via devices.mmio-ranges;
    // probe_and_report reads only (see mac_probe).
    let mac = unsafe { super::mac_probe::probe_and_report(|line| console.write_str(line)) };

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
        super::k3_device_irq_probe::run_k3_stage10(|line| console.write_str(line));
    }
    park();
}

/// Parks the boot hart until reset (the recovery path is the stock system).
fn park() -> ! {
    loop {
        riscv::asm::wfi();
    }
}

/// Board access + native time source for the Iteration 004 probe stages.
///
/// `mmio_*` addresses come exclusively from `mac_probe`/`phy_probe` constants
/// inside the DT-declared windows identity-mapped via `devices.mmio-ranges`
/// (UART/MAC/APMU plus the Iteration 004 GPIO `0xd4019000/0x100` and the
/// single pad page `0xd401e000/0x1000`). Time reads the 24 MHz `time` CSR via
/// axhal; no timer IRQ is required.
struct K3Env;

impl super::phy_probe::ProbeEnv for K3Env {
    fn mmio_read(&mut self, paddr: u64) -> u32 {
        // SAFETY: paddr is a probe constant inside an identity-mapped
        // DT-declared window (see the type contract above).
        unsafe { core::ptr::read_volatile(paddr as usize as *const u32) }
    }

    fn mmio_write(&mut self, paddr: u64, value: u32) {
        // SAFETY: same window contract as mmio_read; probe code writes only
        // the APMU CTRL register, the APBC ASAR unlock keys and the gmac1
        // IO-PD voltage word, the gmac1 pad mux fields, GPIO1_5 and the
        // MDIO ADDR/DATA registers.
        unsafe { core::ptr::write_volatile(paddr as usize as *mut u32, value) }
    }

    fn now_nanos(&self) -> u64 {
        axhal::time::monotonic_time_nanos()
    }

    fn wait_nanos(&mut self, nanos: u64) {
        axhal::time::busy_wait(core::time::Duration::from_nanos(nanos))
    }
}
