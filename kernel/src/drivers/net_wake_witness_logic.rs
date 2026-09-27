// kernel/src/drivers/net_wake_witness_logic.rs

//! Pure, host-testable state machine for the QEMU-only *timer-disabled
//! remote-wake witness*（design D7、Task 3.4）。
//!
//! 该 seam 不含 MMIO、timer、axtask 或 IRQ 依赖，可由宿主测试通过 `#[path]`
//! 直接包含并驱动全部状态转换。它强制的核心不变量是：
//!
//! - 单次运行（single-flight）：`reserve` 在 spawn 前独占运行；只在
//!   `Idle` 或 terminal 态接受，否则拒绝并发 start；
//! - spawn 失败在真正 enqueue 前由 `reserve_rollback` 完全回滚，运行归 `Idle`，
//!   不产生任何任务；
//! - 每次运行复位 per-run 标志（cancel 请求、restore ack），杜绝跨运行污染；
//! - 触发必须是 remote 且 phase 合法：`trigger` 是**原子提升（promotion）**——
//!   只有 `Starting`（target 已注册 waiter、已由 binding 验证提交 `Blocked`）且
//!   未被监督取消的运行接受来自非目标 hart 的 trigger；提升把
//!   `Starting -> Armed` 并同时记录触发方 hart。`Armed` 的语义因此是
//!   "已接受一次针对真实停放目标的 remote trigger"，而不是"target 自己声明
//!   已 arm"。非法/重复/本地/已取消触发返回拒绝且不改变状态；
//! - 有界 deadline 监督通过 `request_cancel` 请求取消（`Starting`/`Armed`/`Woken`
//!   均接受；`Starting` 取消覆盖 target 尚未运行的迟到调度窗口）；target 必须
//!   `restore_ack` 恢复 timer 后才允许发布 `Completed`/`Failed`（ack-before-
//!   terminal），缺少 ack 的 terminal 发布被拒绝且计入 illegal/非瞬态错误；
//! - Illegal/重复转换不改变机器状态（返回错误），杜绝测试 bless 不变量违例。
//! - 每次 `reserve` 由调用方注入一个**作用域所有权 token**（`scope`）：它只用来
//!   让已入队监督确认自己仍属于当前运行（防止回滚重试后旧监督误取消新运行），
//!   是一个生命周期/持有 token，**永不**作为运行身份进入 V5 或判定运行证据。
//!   旧实现用单调递增的 `run_id` 归因运行，属身份型证据机制，此处不再采用。
//!
//! 计数/状态为纯观测 telemetry，不参与同步决定。

/// 未触发/未知 hart 的哨兵（宿主与 target 共享语义）。
pub const UNKNOWN_HART: usize = usize::MAX;

/// 无存活运行的作用域所有权哨兵（无 run_id 单调计数器；仅作用域 token）。
pub const UNKNOWN_SCOPE: usize = usize::MAX;

/// Witness 生命周期阶段（observation ABI）。判别值稳定，V5 直接读取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessPhase {
    Idle      = 0,
    /// 运行已保留（reserve 成功，target spawn 在途/已入队）：target 正在注册
    /// waiter 并提交 park；尚未接受 remote trigger。监督在 deadline 可取消。
    Starting  = 1,
    /// 已接受 remote trigger（提升）：target 已验证提交 `Blocked` 且 waiter 已
    /// 注册，trigger 记录于此；等待 resume。
    Armed     = 2,
    /// target 已被远端 wake 恢复（woken），尚未确认 timer 恢复。
    Woken     = 3,
    Completed = 4,
    Failed    = 5,
}

impl WitnessPhase {
    /// 是否为一个 terminal 阶段。
    pub fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed)
    }
}

/// 监督的绝对超时时刻（微秒），由**当前单调墙钟 + 相对期限**计算。
///
/// `axtask::future::sleep_until` 把参数当作绝对墙钟 deadline（`deadline <=
/// wall_time()` 立即完成）。把一段*相对*期限误传（如 `sleep_until(from_micros(5s))`）
/// 会让监督在启动超过 5s 后立即过期。此纯函数强制监督只使用相对语义：给出现在
/// 时刻 `now_us`，返回 `now_us + deadline_us` 的绝对 deadline，宿主测试可在非零
/// `now_us` 下断言“必须有偏移”，从而让绝对-5s 的实现失败。
pub const fn supervisor_deadline(now_us: u64, deadline_us: u64) -> u64 {
    now_us.saturating_add(deadline_us)
}

/// `reserve` 被拒绝的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartReject {
    /// 上一个 run 尚未 terminal（并发 start 被拒绝）。
    Busy,
}

/// 触发被拒绝的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerReject {
    /// 当前 run 不在可提升状态（不是 `Starting`，或已 terminal/已提升）。
    NotArmed,
    /// 触发方与目标是同一 hart（不构成 remote 触发）。
    SameHart,
    /// 有界监督已请求取消（取消与提升在机器锁下串行，取消先到则触发失败）。
    Cancelled,
}

/// 一次状态转换不被接受的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransitionReject {
    /// 当前 phase 不允许该转换。
    IllegalPhase,
    /// terminal 发布前尚未确认 timer 恢复。
    RestoreNotAcked,
    /// 本 run 已发布过 terminal（重复 terminal）。
    DuplicateTerminal,
}

/// Witness 快照（只读观测）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WitnessSnapshot {
    pub phase: WitnessPhase,
    /// 当前运行的作用域所有权 token（`reserve` 时由调用方注入）。仅用于让
    /// 监督确认自己仍属于当前运行；**不是**运行身份，**不**进入 V5/判定证据。
    pub scope: usize,
    /// 最近一次 run 中目标 hart（`reserve` 时记录）。
    pub target_hart: usize,
    /// 当前 run 的触发方 hart（`trigger` 时记录；尚未触发为 [`UNKNOWN_HART`]）。
    pub trigger_hart: usize,
    /// 上一个已 terminal 的 run 的目标 hart（terminal 后仍保留，用于 V5 归因）。
    pub last_target_hart: usize,
    /// 上一个已 terminal 的 run 的触发方 hart（terminal 后仍保留，用于 V5 归因）。
    pub last_trigger_hart: usize,
    /// 累计成功 run 数。
    pub completed: u64,
    /// 累计失败 run 数。
    pub failed: u64,
    /// 累计 timer 恢复确认数（每次 terminal 前应恰好一次）。
    pub timer_restored: u64,
    /// 被拒绝的并发 start 数。
    pub start_rejects: u64,
    /// 被拒绝的非法/本地/过早 trigger 数。
    pub trigger_rejects: u64,
    /// terminal 缺少 restore ack 而被拒绝的次数（应为 0）。
    pub missing_restore: u64,
    /// 重复 terminal 发布次数（应为 0）。
    pub duplicate_terminal: u64,
    /// 其余非法 phase 转换的拒绝次数。
    pub illegal_transitions: u64,
}

/// 纯 witness 状态机。
#[derive(Debug)]
pub struct WitnessMachine {
    phase: WitnessPhase,
    scope: usize,
    target_hart: usize,
    trigger_hart: usize,
    last_target_hart: usize,
    last_trigger_hart: usize,
    completed: u64,
    failed: u64,
    timer_restored: u64,
    start_rejects: u64,
    trigger_rejects: u64,
    missing_restore: u64,
    duplicate_terminal: u64,
    illegal_transitions: u64,
    /// 当前 run 是否已发布 terminal。
    terminal_published: bool,
    /// 当前 run 是否已确认 timer 恢复。
    restore_acked: bool,
    /// 当前 run 是否被有界监督请求取消。
    cancel_requested: bool,
}

impl WitnessMachine {
    pub const fn new() -> Self {
        Self {
            phase: WitnessPhase::Idle,
            scope: UNKNOWN_SCOPE,
            target_hart: UNKNOWN_HART,
            trigger_hart: UNKNOWN_HART,
            last_target_hart: UNKNOWN_HART,
            last_trigger_hart: UNKNOWN_HART,
            completed: 0,
            failed: 0,
            timer_restored: 0,
            start_rejects: 0,
            trigger_rejects: 0,
            missing_restore: 0,
            duplicate_terminal: 0,
            illegal_transitions: 0,
            terminal_published: false,
            restore_acked: false,
            cancel_requested: false,
        }
    }

    /// 单次运行独占保留（在 spawn 之前调用）。仅 `Idle` 或 terminal 态接受；
    /// 否则拒绝并发 start。`scope` 是调用方注入的作用域所有权 token（不得为
    /// 哨兵值），只用于让监督确认自己仍属于当前运行，**不是**运行身份。成功时
    /// 复位全部 per-run 标志并进入 `Starting`。
    pub fn reserve(&mut self, scope: usize, target_hart: usize) -> Result<(), StartReject> {
        if !(self.phase == WitnessPhase::Idle || self.phase.is_terminal()) {
            self.start_rejects += 1;
            return Err(StartReject::Busy);
        }
        if scope == UNKNOWN_SCOPE {
            self.start_rejects += 1;
            return Err(StartReject::Busy);
        }
        self.scope = scope;
        self.phase = WitnessPhase::Starting;
        self.target_hart = target_hart;
        self.trigger_hart = UNKNOWN_HART;
        self.terminal_published = false;
        self.restore_acked = false;
        self.cancel_requested = false;
        Ok(())
    }

    /// spawn 失败（在 enqueue 前）完全回滚保留，运行归还 `Idle`，不产生任务。
    /// 仅 `Starting` 态合法；其他态拒绝且不改变状态。
    pub fn reserve_rollback(&mut self) -> Result<(), TransitionReject> {
        if self.phase != WitnessPhase::Starting {
            self.illegal_transitions += 1;
            return Err(TransitionReject::IllegalPhase);
        }
        self.phase = WitnessPhase::Idle;
        self.scope = UNKNOWN_SCOPE;
        self.target_hart = UNKNOWN_HART;
        self.trigger_hart = UNKNOWN_HART;
        self.terminal_published = false;
        self.restore_acked = false;
        self.cancel_requested = false;
        Ok(())
    }

    /// 远程触发 = **原子提升**（Plan Review finding 2 的最终闭合）：只有
    /// `Starting`（binding 已验证 saved target task 提交 `Blocked`、waiter 已注册）
    /// 且未被监督取消的运行接受 trigger。提升在同一调用内完成
    /// `Starting -> Armed` 并记录触发方 hart，因此 `Armed` 必然意味着
    /// "对已停放目标的一次已接受 remote trigger"，其后的 wake 必然命中
    /// 已注册 waiter：真实 `Blocked -> Ready`、目标 hart 收到 reschedule IPI。
    /// 取消与提升都持有机器锁，串行裁定：监督先到则 `Cancelled`，提升先到则
    /// 监督看到 `Armed`。返回拒绝且不改变状态：phase 不是 `Starting`
    /// （含 Idle/terminal/已提升/已 woken）、本地触发、已取消。
    pub fn trigger(&mut self, source_hart: usize) -> Result<(), TriggerReject> {
        if self.phase != WitnessPhase::Starting {
            self.trigger_rejects += 1;
            return Err(TriggerReject::NotArmed);
        }
        if source_hart == self.target_hart {
            self.trigger_rejects += 1;
            return Err(TriggerReject::SameHart);
        }
        if self.cancel_requested {
            self.trigger_rejects += 1;
            return Err(TriggerReject::Cancelled);
        }
        self.phase = WitnessPhase::Armed;
        self.trigger_hart = source_hart;
        Ok(())
    }

    /// 目标 hart 被远端 wake 恢复：`Armed -> Woken`。取消路径（target 在 `Starting`
    /// 被监督取消后未经提升即醒来）不经过本转换，由 `fail()` 从 `Starting` 直接收敛。
    pub fn on_woken(&mut self) -> Result<(), TransitionReject> {
        if self.phase != WitnessPhase::Armed {
            self.illegal_transitions += 1;
            return Err(TransitionReject::IllegalPhase);
        }
        self.phase = WitnessPhase::Woken;
        Ok(())
    }

    /// 有界监督请求取消当前 run（timer 仍由目标负责恢复）。`Starting`/`Armed`/
    /// `Woken` 接受：`Starting` 取消在 target 尚未 arm（甚至尚未运行、尚未关闭
    /// timer）时由监督在 deadline 发起，杜绝"监督在 Starting 过期、迟到的 target
    /// 随后进入无人监督的 Armed"窗口（Plan Review finding 1）。非法请求记录但不
    /// 改变 phase。与 `trigger` 提升在同一机器锁下串行：先请求取消则后续提升
    /// 被拒绝（`Cancelled`），先提升则监督看到 `Armed`。
    pub fn request_cancel(&mut self) -> Result<(), TransitionReject> {
        if !matches!(
            self.phase,
            WitnessPhase::Starting | WitnessPhase::Armed | WitnessPhase::Woken
        ) {
            self.illegal_transitions += 1;
            return Err(TransitionReject::IllegalPhase);
        }
        self.cancel_requested = true;
        Ok(())
    }

    /// 确认目标 hart 的 timer 已恢复。任何 terminal 发布前必须恰好调用一次。
    pub fn restore_ack(&mut self) -> Result<(), TransitionReject> {
        if self.terminal_published {
            self.duplicate_terminal += 1;
            return Err(TransitionReject::DuplicateTerminal);
        }
        if self.restore_acked {
            // 重复 ack 在同一 run 内只计一次；不改变状态。
            return Ok(());
        }
        self.restore_acked = true;
        self.timer_restored += 1;
        Ok(())
    }

    /// 目标 hart 已恢复到 `Completed`。前置：已确认 timer 恢复；每个 run 至多
    /// 一次。缺少 ack 或重复发布被拒绝且不改变状态。
    pub fn complete(&mut self) -> Result<(), TransitionReject> {
        self.commit_terminal(WitnessPhase::Completed)
    }

    /// 从任一非 terminal 态进入 `Failed`（含取消路径）。前置：已确认 timer
    /// 恢复；每个 run 至多一次。
    pub fn fail(&mut self) -> Result<(), TransitionReject> {
        // 取消路径可从 Armed/Woken/Starting 进入；其余情况由 commit_terminal 的
        // phase 检查兜底。Starting 取消发生在 target 尚未 arm 时（监督在 arm 前
        // 超时），target 无需恢复已被禁用的 timer。
        if self.phase.is_terminal() {
            self.illegal_transitions += 1;
            return Err(TransitionReject::IllegalPhase);
        }
        self.commit_terminal(WitnessPhase::Failed)
    }

    /// 每个 run 至多发布一个 terminal；缺少 restore ack 或重复发布被拒绝且不
    /// 改变 phase。成功后释放 run 供下一次 reserve。
    fn commit_terminal(&mut self, phase: WitnessPhase) -> Result<(), TransitionReject> {
        if self.terminal_published {
            self.duplicate_terminal += 1;
            return Err(TransitionReject::DuplicateTerminal);
        }
        if !self.restore_acked {
            self.missing_restore += 1;
            return Err(TransitionReject::RestoreNotAcked);
        }
        // Preserve the accepted run's attribution (target + remote trigger) before
        // releasing the run, so a terminal V5 snapshot can still attribute this
        // run's remote-wake causality instead of reading transient state.
        self.last_target_hart = self.target_hart;
        self.last_trigger_hart = self.trigger_hart;
        self.terminal_published = true;
        self.phase = phase;
        self.restore_acked = false;
        self.cancel_requested = false;
        self.scope = UNKNOWN_SCOPE;
        self.target_hart = UNKNOWN_HART;
        self.trigger_hart = UNKNOWN_HART;
        match phase {
            WitnessPhase::Completed => self.completed += 1,
            WitnessPhase::Failed => self.failed += 1,
            _ => {}
        }
        Ok(())
    }

    pub fn snapshot(&self) -> WitnessSnapshot {
        WitnessSnapshot {
            phase: self.phase,
            scope: self.scope,
            target_hart: self.target_hart,
            trigger_hart: self.trigger_hart,
            last_target_hart: self.last_target_hart,
            last_trigger_hart: self.last_trigger_hart,
            completed: self.completed,
            failed: self.failed,
            timer_restored: self.timer_restored,
            start_rejects: self.start_rejects,
            trigger_rejects: self.trigger_rejects,
            missing_restore: self.missing_restore,
            duplicate_terminal: self.duplicate_terminal,
            illegal_transitions: self.illegal_transitions,
        }
    }
}

/// 监督决策谓词（Task 3.4）：目标仍是本监督所属作用域且处于 `Starting` 或
/// `Armed` 时，监督在 deadline 到达后应请求取消。`Starting` 纳入谓词是 Plan
/// Review finding 1 的修复：若 target 在 5s 期限内未被调度，监督不得对
/// `Starting` 视而不见后退出——否则迟到的 target 会关闭本地 timer、进入
/// `Armed` 并永久停放而无人清理。`Starting` 取消发生在 timer disable/park
/// 之前，target 恢复 timer 是有界空操作。目标不同、phase 已离开
/// `Starting`/`Armed`、或作用域 token 已不是本监督所属运行时不干预（杜绝
/// 回滚重试后旧 supervisor 误取消新运行）。作用域 token 只用于所有权，不进入
/// V5/判定证据。纯函数，宿主测试可直接覆盖。
pub fn supervisor_should_cancel(s: &WitnessSnapshot, my_target: usize, my_scope: usize) -> bool {
    matches!(s.phase, WitnessPhase::Starting | WitnessPhase::Armed)
        && s.target_hart == my_target
        && s.scope == my_scope
}

impl Default for WitnessMachine {
    fn default() -> Self {
        Self::new()
    }
}