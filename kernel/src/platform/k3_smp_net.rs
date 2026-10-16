//! MS14 Iteration 001 (T5): the `k3-smp-net` role-composition seam.
//!
//! Pure, host-testable composition over the shared background-role placement
//! policy. The policy file stays at its in-place `drivers/placement.rs`
//! location (design D5) and is included verbatim as a child module here —
//! the k3 network shapes exclude the `drivers` tree, so this is the single
//! compiled instance in `k3-smp-net` builds.
//!
//! Fail-closed contract (A5): an invalid schedulable set, colliding
//! owner/runner singletons, or a missing/misplaced fixed application hart
//! is an error the caller must park on — no fallback, no invented topology.
//! Every hart in a successful result comes from the shared policy's actual
//! return, never a hardcode.

#[path = "../drivers/placement.rs"]
pub mod placement;

/// The fixed application hart for the `k3-smp-net` mode (design D5: the
/// A100-cluster witness hart; the qualification shape is SMP=16, so the
/// mode validates membership in the published schedulable set and parks
/// otherwise).
pub const APP_HART: usize = 9;

/// The composition result: the two NIC background singletons, in shared
/// policy role order (owner = role index 2, runner = role index 3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SmpNetRoles {
    /// The queue-owner singleton hart (`place_roles().net_owner`).
    pub net_owner: usize,
    /// The stack-runner singleton hart (`place_roles().net_runner`).
    pub net_runner: usize,
}

impl SmpNetRoles {
    /// The application singleton hart (the fixed [`APP_HART`], validated by
    /// [`place_smp_net_roles`]).
    pub fn app_hart(&self) -> usize {
        APP_HART
    }
}

/// Fail-closed composition errors (A5); the caller parks on every variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmpNetRoleError {
    /// The shared policy rejected the schedulable set (empty, unsorted,
    /// duplicated, or a hart id `>= MAX_CPU_NUM`).
    InvalidSchedulable,
    /// The policy placed both NIC roles on one hart (only possible below
    /// four schedulable harts; this mode requires distinct singletons).
    OwnerRunnerCollide,
    /// [`APP_HART`] is not in the schedulable set.
    AppHartMissing,
    /// [`APP_HART`] collided with a NIC role singleton.
    AppHartCollide,
}

/// Composes the `k3-smp-net` singletons from the published schedulable list
/// and the boot-hart anchor. The two NIC roles follow the shared policy
/// (`place_roles`); the application role is the fixed [`APP_HART`].
pub fn place_smp_net_roles(
    schedulable: &[usize],
    anchor: usize,
) -> Result<SmpNetRoles, SmpNetRoleError> {
    let p =
        placement::place_roles(schedulable, anchor).ok_or(SmpNetRoleError::InvalidSchedulable)?;
    if p.net_owner == p.net_runner {
        return Err(SmpNetRoleError::OwnerRunnerCollide);
    }
    if !schedulable.contains(&APP_HART) {
        return Err(SmpNetRoleError::AppHartMissing);
    }
    if APP_HART == p.net_owner || APP_HART == p.net_runner {
        return Err(SmpNetRoleError::AppHartCollide);
    }
    Ok(SmpNetRoles {
        net_owner: p.net_owner,
        net_runner: p.net_runner,
    })
}
