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

/// 专用的短生命周期视图锁：只串行化一次六字段视图的 load/store 拷贝，
/// 与 `MigrationSlot::in_progress` 的迁移 single-flight 完全独立（pinned
/// stimulus 读者在迁移持有 `in_progress` 期间仍会读视图，复用它会死锁）。
///
/// 获取使用 Acquire compare-exchange，保证锁内 Relaxed 字段访问不会被重排
/// 到锁边界之外；释放经由 RAII guard 的 Release store，任何 early return
/// 都不会把读者永久楔住。Rust 的 Acquire/Release 模型无法为自研 seqlock
/// 提供本锁所需的双向边界（Release fence 只能约束其之前的操作），因此这
/// 里用互斥短临界区替代 fence 序列协议，在所有支持的内存模型上读者都只
/// 返回旧完整视图或新完整视图之一。
struct ViewLock(AtomicBool);

impl ViewLock {
    const fn new() -> Self {
        Self(AtomicBool::new(false))
    }

    /// 自旋获取；返回持有期间独占视图拷贝的 RAII guard。
    fn acquire(&self) -> ViewGuard<'_> {
        while self
            .0
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            core::hint::spin_loop();
        }
        ViewGuard { lock: self }
    }
}

/// [`ViewLock::acquire`] 的 RAII 释放 guard：drop 时以 Release store 解锁，
/// 锁内字段写因此对下一个获取者完整可见。
struct ViewGuard<'a> {
    lock: &'a ViewLock,
}

impl Drop for ViewGuard<'_> {
    fn drop(&mut self) {
        self.lock.0.store(false, Ordering::Release);
    }
}

/// single-flight 迁移状态槽：生产侧控制流经 [`MigrationSlot::try_enter`] 独占
/// 写，snapshot/V5 并行只读。
///
/// [`MigrationSlot::load`] 与 [`MigrationSlot::store`] 的每次六字段视图拷贝
/// 都经由私有的短生命周期 [`ViewLock`] 串行化（见其上文档）：读者与 writer
/// 并发时，每次 `load` 都返回某次完整提交的 phase/from/to/poll 元组，永不
/// 撕裂；所有字段访问都在锁内 Relaxed 执行，锁边界提供唯一的同步点。
/// `in_progress` 只负责迁移 single-flight，不参与视图访问。
pub struct MigrationSlot {
    in_progress: AtomicBool,
    view_lock: ViewLock,
    view: MigrationAtomicView,
}

impl MigrationSlot {
    pub const fn new() -> Self {
        Self {
            in_progress: AtomicBool::new(false),
            view_lock: ViewLock::new(),
            view: MigrationAtomicView::new(),
        }
    }

    /// 读取当前一致视图。与写并发时经由短视图锁互斥，返回某次完整提交
    /// 的元组（旧视图或新视图之一）。
    pub fn load(&self) -> MigrationView {
        let _guard = self.view_lock.acquire();
        MigrationView {
            phase: self.view.phase.load(Ordering::Relaxed),
            from: self.view.from.load(Ordering::Relaxed),
            to: self.view.to.load(Ordering::Relaxed),
            requested_polls: self.view.requested_polls.load(Ordering::Relaxed),
            observed_polls: self.view.observed_polls.load(Ordering::Relaxed),
            rejects: self.view.rejects.load(Ordering::Relaxed),
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

    /// 整体覆写视图。调用者必须持有 try_enter 成功的临界区（single writer），
    /// 六字段替换本身与并发 `load` 经由短视图锁互斥。
    pub fn store(&self, v: MigrationView) {
        let _guard = self.view_lock.acquire();
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
    }
}
