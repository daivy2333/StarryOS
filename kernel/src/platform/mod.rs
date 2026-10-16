//! Platform descriptor and early console abstraction.
//!
//! Centralizes board-specific facts (memory layout, console UART config,
//! interrupt routing, boot strategy) behind a build-time descriptor,
//! decoupling driver code from platform constants.

pub mod console;
pub mod descriptor;
pub mod early_console;

pub use console::{ConsoleConfig, ConsoleKind, MmioAccessWidth};
pub use descriptor::{
    BootImageConfig, BootKind, InterruptConfig, KernelImageLayout, MemoryLayout,
    PlatformDescriptor, TimerConfig, VirtioMmioNetConfig,
};
pub use early_console::{DwApbUart32EarlyConsole, EarlyConsole, Ns16550U8EarlyConsole};

pub mod k3;
pub mod lichee_d1;
pub mod mac_probe;
pub mod phy_probe;
pub mod qemu;
#[cfg(all(target_arch = "riscv64", feature = "lichee-d1"))]
pub mod smoke;
pub mod visionfive2;

/// K3 stage 9 AIA software-injection probe (MS10 Iteration 000); the module
/// binds the kernel's optional `axplat-riscv64-k3` dependency for the
/// production backend and chip observation seam.
#[cfg(feature = "k3")]
pub mod aia_probe;

/// K3 stage 10 PHY device IRQ/status probe (MS10 Iteration 001): the D5
/// cause-snapshot-before-ack core and the device window on the RAM boot path.
#[cfg(feature = "k3")]
pub mod k3_device_irq_probe;

/// K3 GMAC polling driver (MS11 Iteration 000): exclusive-arena DMA
/// foundation, bounded RX and the stage 11 foreground integration. Lives in
/// `platform` because the k3 build shape excludes the `drivers` tree (the
/// k3 image is the minimal park-path kernel).
#[cfg(feature = "k3")]
pub mod k3_gmac;

/// K3 stage 11 raw GMAC RX observation window (MS11 Iteration 000, task
/// 1.3): the stage 10 gate, route masks and the polling driver foreground.
#[cfg(feature = "k3")]
pub mod k3_network_probe;

/// MS12 Iteration 000: the K3-limited axklib link seam for the axdriver dyn
/// registry (fail-closed `mem_iomap`); compiled only for the async-net mode
/// without paging — the combined mode (MS14 Iteration 003) enables paging
/// and takes the axruntime provider instead, where a second provider symbol
/// would collide at link time.
#[cfg(all(feature = "k3-async-net", not(feature = "k3-smp-combined")))]
pub mod k3_klib;

/// MS14 Iteration 003 (T10): the minimal native MMIO access over the
/// platform `phys_to_virt` mapping, shared by the K3 probe env, the MAC
/// identity probe and the source-133 IRQ callbacks.
#[cfg(feature = "k3")]
pub mod k3_mmio;

/// K3 async GMAC raw data plane (MS12 Iteration 000, task T2): the
/// host-testable source-133 ISR core and the deferred-owner raw session.
#[cfg(feature = "k3-async-net")]
pub mod k3_async_net;

/// MS14 Iteration 000: the K3 multi-hart native platform entry (`k3-smp`
/// mode): 16-AP singleton timer witnesses, software/source-1 cross-hart
/// wakes with a dead target timer, and the remote global source gate.
#[cfg(all(target_arch = "riscv64", feature = "k3-smp"))]
pub mod k3_smp;

/// MS14 Iteration 001: the `k3-smp-net` role-composition seam — the pure
/// host-testable placement over the shared policy, which it includes from
/// the policy's in-place `drivers` location (the k3 net shapes exclude that
/// tree, so this is the single compiled instance here).
#[cfg(all(target_arch = "riscv64", feature = "k3-smp-net"))]
pub mod k3_smp_net;

/// MS14 Iteration 002: the `k3-smp-uart` role-composition seam and boot
/// entries — the pure host-testable placement over the shared policy (the
/// copier singletons from `place_roles`, the alarm on the boot-hart anchor,
/// the fixed shell hart), plus the shell/raw SMP boot entries (the drivers
/// tree is present in this closure, so the policy is re-exported from it).
#[cfg(all(target_arch = "riscv64", feature = "k3-smp-uart"))]
pub mod k3_smp_uart;

/// MS14 Iteration 003 (T10): the `k3-smp-combined` role-composition seam and
/// the combined boot entry — the four background singletons and the fixed
/// shell over the shared policy, the returnable NIC install (T9) and the
/// shared ordinary RAM-rootfs shell body.
#[cfg(all(target_arch = "riscv64", feature = "k3-smp-combined"))]
pub mod k3_smp_combined;

/// MS14 Iteration 003 (T10): the shared RAM-rootfs ordinary shell body —
/// the MS18-accepted DTB/extent/mount/loader/drain chain extracted once and
/// consumed by the single-hart `k3-rootfs` entry (no fixed roles) and the
/// `k3-smp-combined` entry (fixed shell/alarm singletons).
#[cfg(all(target_arch = "riscv64", feature = "k3-rootfs"))]
pub mod k3_rootfs;

#[cfg(all(
    feature = "qemu",
    any(feature = "lichee-d1", feature = "lichee-d1-async-uart")
))]
compile_error!("features `qemu` and lichee-d1 variants cannot be enabled together");

#[cfg(all(feature = "k3", feature = "qemu"))]
compile_error!("features `k3` and `qemu` cannot be enabled together");

#[cfg(all(
    feature = "k3",
    any(feature = "lichee-d1", feature = "lichee-d1-async-uart")
))]
compile_error!("features `k3` and lichee-d1 variants cannot be enabled together");

/// Returns the build-time platform descriptor for the active target.
pub fn descriptor() -> &'static PlatformDescriptor {
    #[cfg(feature = "k3")]
    {
        &k3::K3
    }
    #[cfg(all(not(feature = "k3"), feature = "lichee-d1"))]
    {
        &lichee_d1::LICHEE_D1
    }
    #[cfg(all(not(feature = "k3"), not(feature = "lichee-d1")))]
    {
        &qemu::QEMU_VIRT
    }
}
