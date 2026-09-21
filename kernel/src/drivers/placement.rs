// kernel/src/drivers/placement.rs

//! 共享 background-role placement policy（design D4）。
//!
//! 为每个后台逻辑角色（UART RX copier、UART TX copier、network owner、
//! network runner）从一个有序、去重的 *已发布 schedulable* hart 集合加当前
//! boot/registration hart 计算 singleton affinity。policy 保持纯函数、不依赖
//! MMIO、IRQ 路由、hart 数量连续或任何真板 reserved/topology 事实（QEMU 只
//! 具备 CPU count 事实）。
//!
//! 规则：角色 `i`（按固定角色顺序）放置在 `schedulable[(anchor_pos + i) % len]`。
//! 因此当存在足够 hart 时任意两个角色分离，只有单 hart 才必要共置；未来
//! network 角色相对 UART copier 的占用顺序保持确定。空或非法集合 fail closed
//! （返回 `None`）。

use core::fmt;

/// 平台 CPU/hart 容量上限：有效 hart id 必须 `< MAX_CPU_NUM`。**唯一权威是
/// `axconfig::plat::MAX_CPU_NUM`**（axtask `CpuMask<{MAX_CPU_NUM}>` 位图与
/// run-queue readiness bitset 的容量）：本文件不得另行硬编码容量，否则
/// `sched_hart_ids` 越界探测 `CpuMask::get`（其越界仅有 debug_assert，release
/// 下返回垃圾 `true`）会把不存在的 hart（如 SMP=16 下的 hart 16）当成 schedulable
/// 交给角色 placement。与 kernel critical-section 的嵌套深度数组容量一致
/// （见 design D4；QEMU `SMP=16`、D1、VF2 均在此范围内，超出是构建错误而非截断）。
pub const MAX_CPU_NUM: usize = axconfig::plat::MAX_CPU_NUM;

/// 后台逻辑角色的固定顺序；该枚举的判别值（`as usize`）即角色索引 `i`，使未来
/// network 角色相对 UART copier 保持确定的占用顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(usize)]
pub enum BackgroundRole {
    /// UART RX copier：RX ring 的唯一 producer。
    UartRx    = 0,
    /// UART TX copier：TX ring 的唯一 consumer。
    UartTx    = 1,
    /// Network 硬件 queue owner（后续 Iteration 002 使用）。
    NetOwner  = 2,
    /// Network stack runner（后续 Iteration 002 使用）。
    NetRunner = 3,
}

/// policy 分配的后台角色数量。
// Reserved for network placement in Iteration 002 (D4 keeps a deterministic
// role order now); only the host harness currently references it.
#[allow(dead_code)]
pub const NUM_BACKGROUND_ROLES: usize = 4;

/// 所有后台角色的 singleton hart 分配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RolePlacement {
    /// [`BackgroundRole::UartRx`] 的执行 hart。
    pub uart_rx: usize,
    /// [`BackgroundRole::UartTx`] 的执行 hart。
    pub uart_tx: usize,
    /// [`BackgroundRole::NetOwner`] 的执行 hart。
    pub net_owner: usize,
    /// [`BackgroundRole::NetRunner`] 的执行 hart。
    pub net_runner: usize,
}

impl fmt::Display for RolePlacement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "uart_rx={} uart_tx={} net_owner={} net_runner={}",
            self.uart_rx, self.uart_tx, self.net_owner, self.net_runner
        )
    }
}

/// 校验有序、去重、范围有效的 schedulable 集合。
///
/// 返回 `false` 表示输入非法（空、无序、重复或任一 hart id `>= MAX_CPU_NUM`），
/// 调用方必须 fail closed。调用者应传入由已发布 schedulable mask 构造的按序集合
/// （见 `uart_init::start_copiers`）；本函数在纯 policy 边界防御错误输入。
fn valid_schedulable(schedulable: &[usize]) -> bool {
    let mut iter = schedulable.iter().copied();
    match iter.next() {
        None => false,
        Some(first) => {
            if first >= MAX_CPU_NUM {
                return false;
            }
            let mut prev = first;
            for id in iter {
                if id <= prev || id >= MAX_CPU_NUM {
                    return false;
                }
                prev = id;
            }
            true
        }
    }
}

/// 对全部后台角色做纯 placement；`schedulable` 为空、无序、重复或越界时 fail
/// closed 返回 `None`。
///
/// `schedulable` 必须是按升序排列、无重复、每个 hart id `< MAX_CPU_NUM` 的已发布
/// schedulable hart id 列表。`anchor` 是当前 boot/registration hart：placement
/// 从 `anchor` 在集合中的位置开始，保证输入事实被消费且结果对给定输入确定；
/// `anchor` 不在集合中时等价于从索引 0 开始，不做虚构拓扑过滤（见 D4）。
pub fn place_roles(schedulable: &[usize], anchor: usize) -> Option<RolePlacement> {
    if !valid_schedulable(schedulable) {
        return None;
    }
    let len = schedulable.len();
    let start = schedulable.iter().position(|&c| c == anchor).unwrap_or(0);
    let at = |i: usize| schedulable[(start + i) % len];
    Some(RolePlacement {
        uart_rx: at(BackgroundRole::UartRx as usize),
        uart_tx: at(BackgroundRole::UartTx as usize),
        net_owner: at(BackgroundRole::NetOwner as usize),
        net_runner: at(BackgroundRole::NetRunner as usize),
    })
}

/// 读取单个角色对应的 singleton hart；集合为空时 fail closed 返回 `None`。
// Reserved for network placement / migration controls in Iterations 002–003;
// the host harness already exercises it deterministically.
#[allow(dead_code)]
pub fn place_role(schedulable: &[usize], anchor: usize, role: BackgroundRole) -> Option<usize> {
    place_roles(schedulable, anchor).map(|p| match role {
        BackgroundRole::UartRx => p.uart_rx,
        BackgroundRole::UartTx => p.uart_tx,
        BackgroundRole::NetOwner => p.net_owner,
        BackgroundRole::NetRunner => p.net_runner,
    })
}
