//! MS14 Iteration 003 (T10): the `k3-smp-combined` role-composition seam and
//! the combined boot entry.
//!
//! Pure, host-testable composition over the shared background-role placement
//! policy (`drivers/placement.rs`, design D4/D5): all four background roles
//! (UART RX/TX copiers, NIC owner/runner) follow the shared policy's actual
//! return and must land on pairwise-distinct harts, the alarm deadline task
//! rides the boot-hart anchor, and the initial shell user task is the fixed
//! A100-cluster hart 8. Fail-closed contract: an invalid schedulable set, a
//! boot hart outside the published set, colliding singletons, or a
//! missing/misplaced shell hart is an error the caller parks on — no
//! fallback, no invented topology.
//!
//! The kernel-only submodule holds the combined boot entry: the main task
//! pins to the boot singleton before its first TimerFuture, the UART
//! copiers start once, the returnable `install_smp_net` (T9) installs the
//! one NIC with the shared policy's owner/runner, and the shared ordinary
//! RAM-rootfs shell body (platform/k3_rootfs.rs) runs with the fixed shell
//! and alarm singletons. The stage-42/60/133 registrations stay on the boot
//! hart; after the copiers start, every diagnostic line rides the
//! serialized async writer, never the lock-free early console.

// Single compiled instance of the shared policy: in kernel builds the
// `drivers` tree is present (the k3-smp-combined closure implies `k3-uart`),
// so the policy is re-exported from there; the host harness (no features)
// includes the same file verbatim as a child module.
#[cfg(feature = "k3-smp-combined")]
pub use crate::drivers::placement;
#[cfg(not(feature = "k3-smp-combined"))]
#[path = "../drivers/placement.rs"]
pub mod placement;

/// The fixed initial-shell user-task hart (design D5: the A100-cluster
/// application hart; the combined qualification shape is SMP=16, so the
/// entry validates membership in the published schedulable set and parks
/// otherwise).
pub const SHELL_HART: usize = 8;

/// The composition result: the four background singletons in shared policy
/// role order (RX = role index 0, TX = 1, NIC owner = 2, NIC runner = 3),
/// plus the initial shell and alarm singletons.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpCombinedRoles {
    /// The UART RX copier singleton hart (`place_roles().uart_rx`).
    pub uart_rx: usize,
    /// The UART TX copier singleton hart (`place_roles().uart_tx`).
    pub uart_tx: usize,
    /// The NIC queue-owner singleton hart (`place_roles().net_owner`).
    pub net_owner: usize,
    /// The NIC stack-runner singleton hart (`place_roles().net_runner`).
    pub net_runner: usize,
    /// The initial shell user task's singleton hart (fixed [`SHELL_HART`]).
    pub shell_hart: usize,
    /// The alarm deadline task's singleton hart (the boot-hart anchor).
    pub alarm_hart: usize,
}

/// Fail-closed composition errors; the caller parks on every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmpCombinedRoleError {
    /// The shared policy rejected the schedulable set (empty, unsorted,
    /// duplicated, or a hart id `>= MAX_CPU_NUM`).
    InvalidSchedulable,
    /// The boot hart is not in the published schedulable set (the combined
    /// entry pins the main task and the alarm to it, so an out-of-set boot
    /// hart has no place in the shape).
    BootHartMissing,
    /// The shared policy placed two of the four background roles on one hart
    /// (only possible below four schedulable harts; the combined mode
    /// requires pairwise-distinct singletons).
    BackgroundCollide,
    /// [`SHELL_HART`] is not in the schedulable set (fewer than nine harts).
    ShellHartMissing,
    /// [`SHELL_HART`] collided with a background singleton or the alarm hart.
    ShellHartCollide,
}

/// Composes the `k3-smp-combined` singletons from the published schedulable
/// list and the boot-hart anchor. The four background roles follow the
/// shared policy (`place_roles`) and must be pairwise distinct; the alarm
/// rides the anchor; the initial shell is the fixed [`SHELL_HART`], which
/// must exist and stay clear of every background singleton and the alarm.
pub fn place_combined_roles(
    schedulable: &[usize],
    anchor: usize,
) -> Result<SmpCombinedRoles, SmpCombinedRoleError> {
    let p =
        placement::place_roles(schedulable, anchor).ok_or(SmpCombinedRoleError::InvalidSchedulable)?;
    if !schedulable.contains(&anchor) {
        return Err(SmpCombinedRoleError::BootHartMissing);
    }
    let background = [p.uart_rx, p.uart_tx, p.net_owner, p.net_runner];
    for i in 0..background.len() {
        for j in (i + 1)..background.len() {
            if background[i] == background[j] {
                return Err(SmpCombinedRoleError::BackgroundCollide);
            }
        }
    }
    if !schedulable.contains(&SHELL_HART) {
        return Err(SmpCombinedRoleError::ShellHartMissing);
    }
    if background.contains(&SHELL_HART) || SHELL_HART == anchor {
        return Err(SmpCombinedRoleError::ShellHartCollide);
    }
    Ok(SmpCombinedRoles {
        uart_rx: p.uart_rx,
        uart_tx: p.uart_tx,
        net_owner: p.net_owner,
        net_runner: p.net_runner,
        shell_hart: SHELL_HART,
        alarm_hart: anchor,
    })
}

// ── kernel-only combined boot (never compiled by the host harness) ──────

#[cfg(all(target_arch = "riscv64", feature = "k3-smp-combined"))]
mod boot {
    use alloc::format;

    use axtask::AxCpuMask;

    use super::place_combined_roles;
    use crate::platform::k3_rootfs::{self, FixedRootfsRoles};

    /// The published schedulable hart ids — the same construction the other
    /// K3 SMP shapes use, so the composition seam and the spawn/start seams
    /// consume the identical input.
    fn published_schedulable() -> alloc::vec::Vec<usize> {
        let sched = axtask::schedulable_cpu_mask();
        let mut cpus = alloc::vec::Vec::new();
        for hart in 0..super::placement::MAX_CPU_NUM {
            if sched.get(hart) {
                cpus.push(hart);
            }
        }
        cpus
    }

    /// A singleton CPU mask for one hart (fail at compile capacity only;
    /// schedulable membership is validated by the spawn/start seams).
    fn singleton(hart: usize) -> AxCpuMask {
        let mut mask = AxCpuMask::new();
        mask.set(hart, true)
            .expect("hart index within mask capacity");
        mask
    }

    /// Fails the boot, reports, and parks. The recovery path is external
    /// RESET; no retry, no fallback.
    fn fault(out: &dyn Fn(&str), stage: &str, detail: &str) -> ! {
        out(&alloc::format!(
            "[ms14] combined fault stage={stage} detail={detail}"
        ));
        loop {
            riscv::asm::wfi();
        }
    }

    /// The combined boot: 16-hart platform, UART copiers, the one async NIC
    /// and the ordinary RAM-rootfs BusyBox shell on a single entry. Runs
    /// once on the boot hart after the runtime finished init; every failure
    /// parks with the published backing retained.
    pub fn run_smp_combined_boot(args: &[alloc::string::String], envs: &[alloc::string::String]) -> ! {
        use crate::drivers::{k3_uart, uart_init};

        let boot = axhal::percpu::this_cpu_id();
        // Pre-init lines ride the runtime console (the async driver is not
        // up yet); post-init lines ride the serialized async writer.
        let pre = |l: &str| ax_println!("{}", l);
        let diag = |l: &str| k3_uart::diag_line(l);

        // ── Placement: the shared policy decides, fail closed ──
        let schedulable = published_schedulable();
        let roles = match place_combined_roles(&schedulable, boot) {
            Ok(r) => r,
            Err(e) => fault(&pre, "placement", &format!("{e:?}")),
        };
        // The entry additionally requires the full published 16-hart shape
        // (the make target already rejects every other SMP value; this is
        // the kernel-side backstop, not a second policy).
        if schedulable.len() != 16 {
            fault(
                &pre,
                "harts",
                &format!("expect=16 got={}", schedulable.len()),
            );
        }
        pre(&format!(
            "[ms14] combined mode=k3-smp-combined begin harts={} boot_hart={boot}",
            axhal::cpu_num()
        ));
        pre(&format!(
            "[ms14] combined placement rx={} tx={} owner={} runner={} alarm={} shell={} harts={} anchor={boot}",
            roles.uart_rx,
            roles.uart_tx,
            roles.net_owner,
            roles.net_runner,
            roles.alarm_hart,
            roles.shell_hart,
            schedulable.len()
        ));

        uart_init::init_uart_hardware();

        // Main task singleton before any TimerFuture (Invariant 3); the pin
        // contains the current hart, so no migration happens.
        if !axtask::set_current_affinity(singleton(boot)) {
            fault(&pre, "main-pin", "main task boot-hart pin rejected");
        }

        // SAFETY: this boot path runs once; no other ring producer (no
        // benchmark in this mode) ran before the copiers start.
        unsafe { uart_init::start_copiers() };

        // The one returnable NIC install (T9): the shared policy's owner and
        // runner singletons; the install lines ride the serialized async
        // writer (the early console never races the live UART). The retained
        // arena VA has no combined-side consumer; the handles stay held by
        // the install path and the backing stays retained by the Service.
        let _arena_va = crate::platform::k3_async_net::install_smp_net(
            roles.net_owner,
            roles.net_runner,
            |l| diag(l),
        );

        // Shared ordinary rootfs chain with the fixed shell/alarm singletons
        // committed before their first enqueue (the clone descendants inherit
        // the parent mask at the syscall publish point).
        k3_rootfs::run_ram_rootfs_shell(
            args,
            envs,
            Some(FixedRootfsRoles {
                shell: roles.shell_hart,
                alarm: roles.alarm_hart,
            }),
        )
    }
}

#[cfg(all(target_arch = "riscv64", feature = "k3-smp-combined"))]
pub use boot::run_smp_combined_boot;
