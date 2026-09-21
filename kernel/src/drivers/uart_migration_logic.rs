// kernel/src/drivers/uart_migration_logic.rs

//! Pure, host-testable decision/state model for the QEMU-only *controlled UART
//! copier migration*（Task 4.2 / design D8）。
//!
//! 该 seam 不含 MMIO、axtask 或 IRQ 依赖，由宿主测试通过 `#[path]` 直接包含。
//! 它只负责两件纯事务：
//!
//! - **目标选择** [`select_second`]：把“第二个允许 hart”的选择与拒绝规则
//!   （自动选择 / 显式容量越界 / 自身 / 不在已发布 schedulable 集合）固定为纯
//!   函数，生产侧只能经由该 seam 构造 two-hart mask，杜绝把 mask 容量当作
//!   schedulable 校验的替代品；
//! - **迁移记录** [`MigrationRecord`]：phase 状态机（None -> Widened ->
//!   Restored，Rejected 只增计数不推进 phase），保存 from/to hart 与
//!   requested/observed poll 计数，供 snapshot 的 migration 字段直接读取。
//!
//! 不变量：任何拒绝路径都不改变任务 mask 与记录 phase（只增 rejects）；
//! observed 只能在 Widened 且 poll 计数推进时提交；restore 只能在 Widened
//! 后到达。计数/状态为纯观测 telemetry，不参与同步决定。

/// 未指定目标 hart 的哨兵（ioctl 自动选择与纯逻辑共享）。
pub const AUTO_HART: usize = usize::MAX;

/// Migration 阶段（observation ABI，判别值稳定，snapshot 字段直接读取）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationPhase {
    /// 尚未发起迁移（初始态；restore 后回到该语义由 Restored 表达）。
    None = 0,
    /// 已把 saved handle 的 affinity 加宽到 {orig, second}，等待 second 上的
    /// 直接 poll 观察。
    Widened = 1,
    /// 已恢复 singleton(orig) 并观察到原 hart 上的后续 poll。
    Restored = 2,
}

impl MigrationPhase {
    /// 该阶段是否允许接收 observed 提交。
    pub fn accepts_observation(self) -> bool {
        matches!(self, Self::Widened)
    }
}

/// [`select_second`] 的拒绝原因（生产侧必须 fail closed 且保留旧状态）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrateReject {
    /// 显式目标达到或超过 mask 容量（安全包装之前的容量防线）。
    OutOfCapacity,
    /// 显式目标就是当前角色所在 hart（加宽无意义）。
    SelfTarget,
    /// 显式目标不在已发布 schedulable 集合。
    NotSchedulable,
    /// 自动选择但集合中没有第二个可用 hart（单 hart 环境）。
    NoSecondHart,
}

/// 从已发布 schedulable 集合中为 `orig` 选择第二个允许 hart。
///
/// `sched` 是按升序、去重、每个 id `< capacity` 的已发布 schedulable hart
/// 列表（生产侧由 `schedulable_cpu_mask()` 推导）。`explicit == AUTO_HART`
/// 时选择集合中第一个不等于 `orig` 的成员；显式目标逐条校验，任何非法输入
/// 返回拒绝且不产生任何状态变化。
pub fn select_second(
    orig: usize,
    explicit: usize,
    sched: &[usize],
    capacity: usize,
) -> Result<usize, MigrateReject> {
    if explicit == AUTO_HART {
        return sched
            .iter()
            .copied()
            .find(|&h| h != orig)
            .ok_or(MigrateReject::NoSecondHart);
    }
    if explicit >= capacity {
        return Err(MigrateReject::OutOfCapacity);
    }
    if explicit == orig {
        return Err(MigrateReject::SelfTarget);
    }
    if !sched.contains(&explicit) {
        return Err(MigrateReject::NotSchedulable);
    }
    Ok(explicit)
}

/// 单方向的迁移记录（纯状态模型；生产侧把字段镜像到 snapshot wire）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationRecord {
    phase: MigrationPhase,
    from: usize,
    to: usize,
    requested_polls: u64,
    observed_polls: u64,
    rejects: u64,
}

impl Default for MigrationRecord {
    fn default() -> Self {
        Self::new()
    }
}

impl MigrationRecord {
    pub const fn new() -> Self {
        Self {
            phase: MigrationPhase::None,
            from: AUTO_HART,
            to: AUTO_HART,
            requested_polls: 0,
            observed_polls: 0,
            rejects: 0,
        }
    }

    /// 已加宽：记录 from/to 与请求时刻的 poll 计数。
    pub fn begin(&mut self, from: usize, to: usize, polls_at_request: u64) {
        self.phase = MigrationPhase::Widened;
        self.from = from;
        self.to = to;
        self.requested_polls = polls_at_request;
    }

    /// 提交在 second hart 上的直接 poll 观察；只在 Widened 且计数推进时接受。
    pub fn observe(&mut self, polls_at_observe: u64) -> bool {
        if !self.phase.accepts_observation() || polls_at_observe <= self.requested_polls {
            return false;
        }
        self.observed_polls = polls_at_observe;
        true
    }

    /// 提交恢复：只在 Widened 后合法。
    pub fn restore(&mut self) -> bool {
        if self.phase != MigrationPhase::Widened {
            return false;
        }
        self.phase = MigrationPhase::Restored;
        true
    }

    /// 记录一次拒绝（不改变 phase/mask 语义，纯计数）。
    pub fn reject(&mut self) {
        self.rejects = self.rejects.saturating_add(1);
    }

    pub const fn phase(&self) -> MigrationPhase {
        self.phase
    }
    pub const fn from(&self) -> usize {
        self.from
    }
    pub const fn to(&self) -> usize {
        self.to
    }
    pub const fn requested_polls(&self) -> u64 {
        self.requested_polls
    }
    pub const fn observed_polls(&self) -> u64 {
        self.observed_polls
    }
    pub const fn rejects(&self) -> u64 {
        self.rejects
    }
}

/// 单方向迁移视图（snapshot/V5 直接读取的纯 telemetry）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationView {
    pub phase: u8,
    pub from: usize,
    pub to: usize,
    pub requested_polls: u64,
    pub observed_polls: u64,
    pub rejects: u64,
}

/// 把迁移视图打包进 V5 的单个 reserved u64 字段：phase(0..8) |
/// from(8..32) | to(32..56) | reserved(56..64)。`AUTO_HART` 在 wire 上编码
/// 为 0（phase==None 时 from/to 本就无意义）。req/obs poll 计数经
/// owner/runner 的 events 字段单独观测，不重复进 V5。
pub const fn pack_migration_view(v: &MigrationView) -> u64 {
    let from = if v.from == AUTO_HART { 0 } else { v.from & 0xff_ffff };
    let to = if v.to == AUTO_HART { 0 } else { v.to & 0xff_ffff };
    (v.phase as u64) | ((from as u64) << 8) | ((to as u64) << 32)
}

/// [`pack_migration_view`] 的逆变换（宿主 ABI 测试与诊断消费）。
pub const fn unpack_migration_view(raw: u64) -> (u8, usize, usize) {
    (
        (raw & 0xff) as u8,
        ((raw >> 8) & 0xff_ffff) as usize,
        ((raw >> 32) & 0xff_ffff) as usize,
    )
}

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering};

/// 原子化的单方向迁移视图字段。每个字段都是独立原子，`: Sync` 自动推导，
/// 不再需要 `unsafe impl Sync`，也不对 readers 与 writer 形成数据竞争。
struct MigrationAtomicView {
    phase: AtomicU8,
    from: AtomicUsize,
    to: AtomicUsize,
    requested_polls: AtomicU64,
    observed_polls: AtomicU64,
    rejects: AtomicU64,
}

impl MigrationAtomicView {
    const fn new() -> Self {
        Self {
            phase: AtomicU8::new(MigrationPhase::None as u8),
            from: AtomicUsize::new(AUTO_HART),
            to: AtomicUsize::new(AUTO_HART),
            requested_polls: AtomicU64::new(0),
            observed_polls: AtomicU64::new(0),
            rejects: AtomicU64::new(0),
        }
    }
}

/// single-flight 迁移状态槽：生产侧控制流经 [`MigrationSlot::try_enter`] 独占
/// 写，snapshot/V5 并行只读。
///
/// [`MigrationSlot::store`] 用 seqlock（偶数 seq = 已提交稳定视图，奇数 =
/// 写入进行中）原子化一次整体写入与并发 [`MigrationSlot::load`] 的多次读取：
/// 这是单写多读的简单 seqlock，读者只在两次读到的 seq 一致且为偶数时返回，
/// 保证永远拿不到撕裂的 phase/from/to/poll 元组，同时所有访问都经过原子字段，
/// 不存在未同步的 `Sync` 数据竞争。
pub struct MigrationSlot {
    in_progress: AtomicBool,
    seq: AtomicUsize,
    view: MigrationAtomicView,
}

impl MigrationSlot {
    pub const fn new() -> Self {
        Self {
            in_progress: AtomicBool::new(false),
            seq: AtomicUsize::new(0),
            view: MigrationAtomicView::new(),
        }
    }

    /// 读取当前一致视图。与写并发时自旋重试，直到某次写入完整提交。
    pub fn load(&self) -> MigrationView {
        loop {
            // 奇数 seq 表示 store 正在两次 seq 标记之间写入，跳过而非读撕裂。
            let seq1 = self.seq.load(Ordering::Acquire);
            if seq1 & 1 == 1 {
                continue;
            }
            let v = MigrationView {
                phase: self.view.phase.load(Ordering::Relaxed),
                from: self.view.from.load(Ordering::Relaxed),
                to: self.view.to.load(Ordering::Relaxed),
                requested_polls: self.view.requested_polls.load(Ordering::Relaxed),
                observed_polls: self.view.observed_polls.load(Ordering::Relaxed),
                rejects: self.view.rejects.load(Ordering::Relaxed),
            };
            // seq 一致且偶数：两次读取之间没有 store 运行，所有字段来自同一
            // 次完整提交的视图（Acquire 在 Release 提交后读到偶数 seq）。
            if seq1 == self.seq.load(Ordering::Acquire) {
                return v;
            }
        }
    }

    /// 尝试进入 single-flight 临界区；false 表示已有控制在进行。
    pub fn try_enter(&self) -> bool {
        self.in_progress
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
    }

    /// 离开 single-flight 临界区。
    pub fn exit(&self) {
        self.in_progress.store(false, Ordering::Release);
    }

    /// 整体覆写视图。调用者必须持有 try_enter 成功的临界区（single writer）。
    pub fn store(&self, v: MigrationView) {
        // 奇数：标记写入进行中（reader 在此窗口重试）。该标记本身用 Relaxed 即可，
        // 但紧跟一个 Release fence 把该标记的写入序排在后续所有字段写之前；否则在
        // 弱序目标（RISC-V SMP）上，后续 Relaxed 字段写可能在奇数标记之前可见，读者
        // 会读到旧偶数 seq 两次却采样到新字段（撕裂视图）。
        self.seq.fetch_add(1, Ordering::Relaxed);
        core::sync::atomic::fence(Ordering::Release);
        self.view.phase.store(v.phase, Ordering::Relaxed);
        self.view.from.store(v.from, Ordering::Relaxed);
        self.view.to.store(v.to, Ordering::Relaxed);
        self.view
            .requested_polls
            .store(v.requested_polls, Ordering::Relaxed);
        self.view
            .observed_polls
            .store(v.observed_polls, Ordering::Relaxed);
        self.view.rejects.store(v.rejects, Ordering::Relaxed);
        // 偶数：Release 提交并由 Release fence 之前的所有字段写保证先于它；Acquire
        // reader 读到该偶数 seq 即同步到全部字段写（已知一个写窗口的完整视图）。
        self.seq.store(self.seq.load(Ordering::Relaxed).wrapping_add(1), Ordering::Release);
    }
}
