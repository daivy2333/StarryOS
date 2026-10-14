# Iteration 001 / Cycle 000：常驻恢复 owner 与真实 PHY 控制面

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T4（2.1）、T5（2.2）；T5 依赖 T4。
- Acceptance gaps：None。
- Repair items：None。
- Inherited scope：已批准 R1–R8/S01–S20 与 A1–A4 中 resident owner、link 恢复、source60/MDIO 控制面相关部分；Iteration 000 已交付的底层 stop/SWR/原地重建（Gmac 微步）与 adapter epoch/carrier 契约直接复用，不重做 bring-up。
- Excluded scope：T6/T7 的 socket 最终组合矩阵与真板最终验收、S16/S17/S18 的 socket 场景、多 hart、长稳压力、性能优化；禁止修改 AGENTS、全局 tasks/M/R/I、Runbook/Issue 或其他用户工作树改动。

**Objective**

同一常驻 queue owner 驱动实际 reset（请求经 queue-diagnostics 面可达），link Down/Up 不推进 QueueEpoch，Again 有 1 ms one-shot 有界重试，Fault 稳定隔离且记账不重复；K3 侧 runtime PHY 状态经 MFPR35/INSR/source60 与分步 MDIO 接通 `read_link_status`，真板拔插与受控 reset 下 owner/link 语义成立。该结果被 accepted 后，最后一轮才做 socket 组合与最终板测。

**Investigation Facts**

- Current Baseline：Iteration 000 Cycle 000 Act Response（2026-10-13）+ accepted Review。T1–T3 改动全部在当前未提交工作树（HEAD `d07285c0` 之上）：`k3_gmac/mod.rs` runtime 微步机（Resetting 0–9、Rebuild 0–73、SWR 一次性 1 s 绝对 deadline）、`adapter.rs` 唯一 checked QueueEpoch + NetRecoveryControl（begin 先进 epoch、Rebuilding 边界结清旧 carrier 入 quarantine、OwnerSummary 32 carrier 守恒）、`k3_async_net.rs` native probe 与 arm/register 前移。验证基线：harness 36/36、k3-async-net-host 21/21（6 二进制）、k3-polling-net-host 17/17、四个 make 构建门 exit 0、A3 真板停止/保留/重建见证审核 PASS。覆盖未变时直接采信。
- 门控事实：`kernel/Cargo.toml:38` `k3-net-diagnostics = ["k3-async-net", "axnet/queue-diagnostics"]`——K3 probe 构建启 axnet/queue-diagnostics；`qemu-diagnostics` 是其超集（`crates/axnet/Cargo.toml:40`）。普通 `k3-async-net` 构建不含任一诊断面。
- axnet 请求组（当前 qemu-diagnostics 门控，T4 迁移对象）：`crates/axnet/src/lib.rs:264` `pub fn recovery_reset_request()`；`async_rx.rs:236` `RECOVERY_RESET_REQUEST: spin::Mutex<RecoveryRequestState>`、`RecoveryRequestState{pending, owner_claimed}`（~288，请求与 owner 共享 lifecycle 转换锁，自然恢复后请求不存活）、`async_rx.rs:1377` `recovery_reset_request_shared()`（无全局 Service → BadState；request+publish_queue_work）、`async_rx.rs:1388` `claim_recovery_reset_request()`；claim 消费点 `async_rx.rs:1930`（`RxRxFuture::poll_active` 内 `#[cfg(feature = "qemu-diagnostics")]`，claim 后 `drop(service)` → `enter_recovery(&DevError::Io, recover_stage::EXPLICIT_REQUEST)` → `poll_recovery(cx)`）。QEMU-only 诊断打印（`net_link_step_diag`/`ROUND_COUNT`/`TRACE_UNTIL`/`[NET-LINK-COMMIT]`）不是请求组成员，保持 qemu 门控。测试助手 `with_recovery_request_transition`（async_rx.rs:2189/2251）随组迁移。
- resident recovery 既有机器（K3 直接复用，不改语义）：`async_rx.rs:2183` `enter_recovery`、`2311` `poll_recovery`、`2444` `recovery_step`——Recovered 分支次序为 `open_socket_epoch_after_recovery`（service.rs:1046，u64::MAX fail-closed）→ `tx_close_device_owned_target` → `flush_recovery_close`（旧 epoch flush 先于 epoch 推进，Finding 1 次序）→ `tx_advance_epoch_target` → lifecycle `recovery_committed` → `cancel_recovery_timer`。阶段 2 s 绝对 deadline、同 stage Pending 不续期（`recovery_deadline`/`recovery_timer`）。
- link policy 既有机器：`service.rs:920` `link_policy_step_target` 返回 `LinkStep::{NoEvent,Down,Up,Again,Unsupported,Fault}`；seam fail-stop（link_seam_fault/u64::MAX）、Down 关闭当前 SocketEpoch（`commit_socket_epoch_terminal_for` + `socket_epoch_wake`）、Up 经 `open_next_socket_epoch` 开新 epoch、LinkGeneration 每 value change 恰一次、QueueEpoch 永不因 link 推进。`read_link_status_target`（service.rs:894）把 driver `Again` 原样透传。当前 Again 处理（async_rx.rs `poll_active` ~1945）：保留 pending + `publish_config()` 立即自醒——无时距，MDIO busy 期间成为 busy retry；`initial_link_pending`（async_rx.rs:1409）只在非 Again 臂清除。
- one-shot timer 既有模式：`RxRxFuture` 已有 `lease_timer`/`recovery_timer`/`data_stage_timer`（`Option<Pin<Box<dyn Future<Output=()> + Send>>>`，axtask timer，wake-only 无 generation）与 `recovery_progress_wake`（下一有界唤醒 instant）。1 ms Again 重试复用该模式；host 测试用 `RecoveryTestClock`（recovery.rs，per-test 独立 fake clock）与 fakeclock 步进，不宿主 sleep（宿主 axhal 无平台后端、wall_time≡0）。
- Router 缓冲：`router.rs:106` `Router{rx_buffer, tx_buffer: PacketBuffer, tx_fault, tx_drop_counts[..], rx_delivered_delta, rx_consumed_delta, rx_cursor}`；当前 socket epoch 关闭路径不丢弃已复制未分发的旧 RX 帧与 Router rx/tx 缓冲残留。
- fault 记账现状（Iteration 000 遗留，T4 整改）：K3 adapter 暴露 `recovery_control` 后，设备 fatal 产生三次 terminal 记账（fatal 检测 + resident-recovery 进入 + begin 拒绝后 publish），`tests/k3-async-net-host/tests/service_owner.rs` 断言 fault==3；lifecycle 仍稳定 Faulted、fail-stop 语义未变。T4 把 Fault 隔离整改为稳定单次语义并同步该断言。
- K3 PHY 面：`k3_gmac` 内 `phy_probe` 模块（`mdio_read`/`PHY_ADDR`/`BMSR_REG`/`ANA_REG`/`LPA_REG`/`CTRL1000_REG`/`STAT1000_REG`）；`Gmac::decode_link`（mod.rs:575）为 ≤5 s 同步 decode（10 ms 轮询 BMSR link+ANEG_COMPLETE，再 ANA/LPA/CTRL1000/STAT1000 优先序 decode：1000FD→1000HD→100FD→100HD→10FD→10HD→NoCommonAbility），只在 init 使用；`Gmac.link: Option<LinkConfig>` 为最后已知值。runtime 不可调用 decode_link（同步 spin）。
- MS10 source60/PHY IRQ 裁决事实（按归档 change 与 `tests/k3-device-irq-host-harness.rs` 参考实现复用）：pin35 按 DTB 武装 GPIO 下降沿 + MFPR EDGE CLEAR 复位默认置位须 hold-in-clear、INSR COR 前快照、每物理事件一条 INSR=0 回声 claim 须分类器排除、CLEAR 释放后重查。EIE 掩码窗口 MSI 丢弃（掩码只作止损）。`k3_async_net.rs:959` stage10 当前屏蔽 `[60, 133, 277]`——source60 接线即在此改按 DTB 武装。
- `read_link_status` 契约：`axdriver_net` lib.rs:199 trait 默认 `Err(DevError::Unsupported)`，"config-generation race 时返回 retryable 错误"（Again）；axnet `device/mod.rs:379` 经 `recovery_control()` 转发；K3 adapter 本轮覆写它。Service 初始 link 读取已接受 Unsupported（Iteration 000 行为），本轮接通后变为真实快照。
- 测试入口：axnet 定向测试在 `#[cfg(test)]` 模块（async_rx.rs / service.rs / device/tests.rs，`ServiceAccess::Injected` + `RecoveryTestClock` 夹具模式，4366 起有 fake `poll_recovery_step` 先例）；K3 生产面在 `tests/k3-async-net-host`（recovery_contract.rs/adapter_contract.rs/service_owner.rs + `src/fake.rs`）与 `tests/k3-gmac-host-harness.rs`（FakeEnv 已有 access_cost_ns/swr_latency_ns/swr_stuck/cache 计费旋钮）。
- QEMU 回归入口：`.agents/runbooks/ms07-qemu-single-hart-recovery-evidence.md`（编译、peer、qemu-system-riscv64、guest probe、HMP off/on；raw serial + 退出码直接判定，validator 按既有豁免不新增）。

**Code and Critical Path**

1. axnet 共享接缝：`crates/axnet/src/lib.rs`（recovery_reset_request 门控）、`async_rx.rs`（请求组迁移、claim 位、Again 1 ms one-shot、fault 记账、旧 RX slot 丢弃）、`service.rs`（link 重读/接单门控、Fault 隔离）、`router.rs`（epoch 关闭时 rx/tx 缓冲 discard）、`device/{mod,ethernet}.rs`（转发面）。
2. K3 driver 面：`k3_gmac/mod.rs`（runtime PHY 状态、分步 MDIO 复用 phy_probe）、`k3_gmac/adapter.rs`（read_link_status 覆写）、`k3_async_net.rs`（source60 handler 武装与 CONFIG cause 路由）。
3. 测试面：axnet `#[cfg(test)]` 新增 RED；`tests/k3-async-net-host/src/fake.rs`（MFPR/INSR/BMSR 脚本扩展）、`tests/recovery_contract.rs`/`adapter_contract.rs`、`tests/service_owner.rs`（fault 断言同步）、必要时 `tests/k3-gmac-host-harness.rs`（Gmac MDIO 分步）。

**Implementation Guidance**

先 T4（axnet 接缝，QEMU/K3 共享）RED→GREEN，再 T5（K3 PHY 面）RED→GREEN，最后 QEMU 回归与真板会话。请求组迁移保持符号与语义不变，只改 cfg 门控；Again 重试的 timer 为 wake-only（stale wake 至多一次有界 poll），不携带 generation。PHY 分步 MDIO 的每次 step 至多一个 MMIO/MDIO 动作，COR 先快照再清，page 恢复后才读能力寄存器。helper 命名与局部拆分留 Act。

**Task Contracts**

### T4：共享恢复请求门控、Again 重试与 epoch 关闭丢弃（axnet resident owner）

- Requirement/Scenario：R1/R2/R3/R6/R7 的共享接缝部分；S01/S02/S04/S09/S13/S14/S15 的 axnet 面。
- Depends on：Iteration 000 accepted（已满足）。
- Targets：`crates/axnet/src/lib.rs::recovery_reset_request`；`async_rx.rs::{RECOVERY_RESET_REQUEST, RecoveryRequestState, recovery_reset_request_shared, claim_recovery_reset_request, poll_active claim 位, LinkStep::Again 分支, RxRxFuture timer, 旧 RX slot 丢弃, fault 记账}`；`service.rs::{link_policy_step_target 后重读/接单门控, Fault 隔离}`；`router.rs::Router{rx_buffer, tx_buffer}`；`device/{mod,ethernet}.rs` 转发面；`tests/k3-async-net-host/tests/service_owner.rs` fault 断言同步。
- Current behavior：① reset 请求整组在 qemu-diagnostics 下，K3 queue-diagnostics 构建不可达；② Again 立即 publish_config 自醒（busy retry，无时距）；③ 恢复完成不重读 link、up 前无接单门控衔接（initial_link_pending 仅激活期使用）；④ 旧 socket epoch 关闭不丢弃旧 RX slot 与 Router rx/tx 缓冲；⑤ 设备 fatal 三次 terminal 记账（fault==3）。
- Required behavior：① 请求组（状态、入口、claim、消费位、测试助手）整组迁入 queue-diagnostics，qemu-diagnostics 超集面行为不变；② Again 保留 pending 并安排 1 ms one-shot timer 重试，不再立即自醒；timer wake-only；③ resident recovery Recovered 后重读 link，up 确认前不接单（复用 initial_link_pending/接单门控语义），Down/Up 不推进 QueueEpoch（既有 link_policy 语义保持）；④ 关闭旧 socket epoch 时丢弃旧 RX slot 已复制未分发帧与 Router rx/tx 缓冲内容，旧数据不交付新 epoch socket；维持取消/flush/wake 次序（flush_recovery_close 先于 epoch 推进不破坏）；⑤ Fault 稳定隔离：fatal 每事件类单次 terminal 记账，不再三连；lifecycle 稳定 Faulted、不回退轮询。
- Preserve：QEMU 三面既有语义与测试（482/506/514 基线）、flush C4/ticket/cancel 语义、K3 adapter Iteration 000 契约（epoch/OwnerSummary/拒绝语义）、`NetRecoveryControl` trait 不改、IRQ 不访问 adapter/Service 锁。
- Forbidden：引入第二 owner/请求体系、把 K3 MMIO/PHY 语义写进 axnet、本轮触碰 socket 框架（SocketOps 语义留 T6）、多 hart 假设、新增身份型证据/判定层。
- First test：axnet `#[cfg(test)]`（ServiceAccess::Injected + RecoveryTestClock 模式）先 RED：`queue_only_reset_request_is_reachable`（queue-diagnostics-only 面编译并请求可达，Busy/自然恢复吸收交错）、`link_again_waits_without_busy_wake`（Again 后无立即自醒，fake clock 推进 1 ms 才重试）、`closed_epoch_discards_copied_packets`、`cancel_waiter_does_not_cancel_resident_recovery`、`resident_recovery_keeps_one_owner`。
- GREEN condition：上述全部转 GREEN；重复请求 Busy/自然恢复吸收、Again 不 busy wake、link 失败门控、旧 buffer 不交付新 socket、reset/link 交错次序成立；qemu-diagnostics 面既有测试不回归。
- Verification：定向 `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib [--features queue-diagnostics|qemu-diagnostics] -- <filter>`；三 feature 面全量各一次留本轮末（Iteration 预算）；`tests/k3-async-net-host` 全包在 T4/T5 完成后一次。
- Stop when：请求组与 resident 机器的锁/lifecycle 语义冲突无法在既有 RecoveryRequestState 模型内表达、或 epoch 关闭丢弃与 flush/cancel 次序出现既有契约级矛盾；返回 Plan。

### T5：K3 runtime PHY 状态、adapter link 读取与 source60 接线

- Requirement/Scenario：R3/R4/R6；S06/S07/S08/S09/S13 的 K3 面。
- Depends on：T4 Gate3 PASS。
- Targets：`k3_gmac/mod.rs`（runtime PHY 状态与分步 MDIO，复用 phy_probe）；`k3_gmac/adapter.rs::read_link_status`（NetRecoveryControl 覆写）；`k3_async_net.rs`（source60 武装与 CONFIG cause 路由，stage10 屏蔽位改按 DTB 武装 pin35）；`tests/k3-async-net-host/src/fake.rs`（MFPR/INSR/BMSR 脚本）；必要时 `tests/k3-gmac-host-harness.rs`。
- Current behavior：read_link_status 为 trait 默认 Unsupported；source60 被 stage10 屏蔽、无 runtime PHY 事件；Gmac.link 仅 init 时 decode 一次；MDIO 仅有同步 decode_link。
- Required behavior：① 最小 GPIO handler：按 DTB 武装 pin35 下降沿，MFPR EDGE CLEAR hold-in-clear，INSR COR 前快照，每物理事件一条 INSR=0 回声由分类器排除，CLEAR 释放后重查；handler 只留 snapshot/hold/config 提示并 publish CONFIG cause，不在 IRQ 上下文做 MDIO。② 同一 owner 分步 MDIO：COR 先快照、page 恢复、BMSR link 位读取；MDIO busy/超时返回有界 Again/Io，阶段超时不破坏 owner/资源账本；状态去重（同值不重复发布）。③ adapter `read_link_status` 返回一致快照（BMSR link 位），config-race 时 Again；Down/Up 不改 QueueEpoch（adapter 层 guard）。④ link up 后速率/双工应用沿用既有 decode 优先序，写 MAC_CONFIG 沿用 captured-value/RMW 既有契约。
- Preserve：Iteration 000 全部契约（微步机、epoch、OwnerSummary、拒绝语义）、source133 既有 handler 与 arm/recheck、普通镜像不新增 PHY 行为（源经 DTB/路由确认才武装）、MS10 裁决事实。
- Forbidden：IRQ 上下文 MDIO/锁、第二 PHY 轮询框架、自动重试到成功、把 QEMU evidence 冒充 PHY 板测、身份型证据。
- First test：生产 host 夹具先 RED：`initial_and_repeated_phy_link_snapshot`（初始与重复/回声快照去重）、`link_down_up_preserves_queue_epoch`、`mdio_phase_timeout_preserves_owner`；fake 按 MS10 语义脚本化 MFPR/INSR/BMSR/busy/超时。
- GREEN condition：上述转 GREEN；Down/Up 全周期 QueueEpoch 不动、MDIO 阶段超时后 owner/账本无恙、回声事件不产生重复状态变化；既有 k3-async-net-host 测试不回归。
- Verification：定向 host filter；真板由既有独立 probe 入口实际拔插（至少一完整 down→up）、受控 reset 与恢复后新流量验证；一次单 hart QEMU 恢复真实入口回归（T4 共享面变化受影响；见 Verification）。
- Stop when：真板 INSR/MFPR 行为与 MS10 裁决事实矛盾、或 runtime PHY 状态与 Iteration 000 微步机的 owner 边界冲突；返回 Plan。

**Invariants**

- 唯一 queue owner 驱动 recovery 与 link policy；IRQ 只发布 cause/snapshot，不触 Service/adapter 锁。
- QueueEpoch 只在 recovery begin 边界推进；link Down/Up 永不推进 QueueEpoch，LinkGeneration 每 value change 恰一次。
- flush 先于 epoch 推进（Finding 1 次序）；取消/唤醒发布次序不因丢弃重构而改变。
- Again 的 1 ms 重试为 wake-only one-shot：stale wake 至多一次有界 poll。
- 软件标签不证明硬件状态；PHY 结论必须来自真板；QEMU 只覆盖共享接缝回归。
- K3 板测经 RAM boot runbook，无持久写入；外部 RESET 回官方系统不算 runtime reset 验收。

**Non-goals**

socket 最终场景矩阵与旧新 socket 公共入口测试（T6）、真板最终组合验收与 fail-stop 末场景（T7）、S16–S18、多 hart、长稳、性能；不修改 NetRecoveryControl trait 与 QEMU validator 豁免范围。

**Acceptance**

本轮追踪：R1→T4（resident owner、单 owner 记账）；R2→T4（旧数据丢弃隔离）；R3→T4（Again 重试）+T5（PHY 快照/去重）；R6→T4（cancel 不取消 recovery、1 ms 有界）+T5（MDIO 阶段超时）；R4 的 owner 语境复用+T5 真板；R7 的 flush 次序部分以既有次序保持与回归覆盖（socket 级留 T6）；R8→本轮回归。R1/R3 的 socket 验收面与 R7 socket 语义留 T6/T7，本轮不标完成。

- A1（T4；R1/R2/R3/R6）：axnet 先 RED 后 GREEN：queue-only 请求可达与交错、Again 1 ms one-shot 不 busy wake、旧 epoch 数据丢弃、cancel 不取消 resident recovery、Fault 单次记账。
- A2（T4/T5；R8/S19）：受影响构建与包通过：axnet 三面全量各一次、k3-async-net-host 全包一次；构建门 `make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`，最后 `make k3-async`、`make k3-async-probe`；D1/VF2 无共享面变化时采信既有结论。
- A3（T4/T5；R1/R3/R8）：一次单 hart QEMU 恢复真实入口回归：复用 `.agents/runbooks/ms07-qemu-single-hart-recovery-evidence.md` 编译/peer/qemu-system-riscv64/guest probe 入口，覆盖 reset 请求与 link flap 两组真实入口，原生输出与 guest 退出码直接判定；QEMU 不冒充 K3 PHY 结论。
- A4（T5；R3/R4）：一次 K3 owner/link 板测：既有 probe 入口 + 实际拔插至少一完整 down→up + 受控 reset + 恢复后新流量；owner 账本、QueueEpoch 不变、link 重查与新会话直接观察。具体输出字段由 Act 接线后在 Act Response 说明，最短操作步骤按 Iteration 000 A3 的 boot 流程（RAM boot runbook，不 saveenv）。

**Verification**

host 定向：axnet 三面 filter、k3-async-net-host filter（同 Iteration 000 命令入口，`RUSTFLAGS='-C linker=scripts/cc-nopie.sh'`）。昂贵验证按上述 A1–A4 预算各一次；板测人工项写最短步骤与每步直接可观察结果，一句话记录，不建判定层。QEMU 会话命令按 runbook 原文执行，不新增封装。

**Gate 2 Readiness**

PASS：2026-10-13 用户原话“批准”。技术自检结论采信未变（调查完成、契约自包含、无 TBD、RTM 无 Missing/Simplified）；BLOCKED：None；WAIVED：None。Status 转 `ready`。

**Persisted Evidence**

- Mode: none。命令、退出码、判定行、QEMU/板测一句话结果进入 Act Response；安全 RAM 板测可低成本重跑。

**Risks and Notes**

- host 无真实时钟：Again/1 ms 与 deadline 测试全部 fakeclock 步进（RecoveryTestClock / FakeEnv 时钟），不宿主 sleep；真实时距结论上板。
- MFPR/INSR/CLEAR 语义按 MS10 裁决事实采信；真板行为若矛盾按 T5 Stop when 处理。
- `service_owner.rs` fault 断言随 T4 整改同步为单次语义；T6 可能再次覆盖，届时按覆盖变化定向验证。
- 既有 `AX_IP`/`AX_GW` 编译期约定延续，非本轮引入。

## Act Response

- Status: blocked
- Date: 2026-10-13（Iteration 001 / Cycle 000，T4 完成、T5 agent 侧完成；A4 板测三轮未收敛，触发 Gate 5 阻塞交接，见 Blocker Handoff）

### 实际改动

**T4（axnet 共享接缝；crates/axnet）**

1. 请求组门控迁移（qemu-diagnostics → queue-diagnostics，qemu 为超集、行为不变）：`async_rx.rs` 的 `RECOVERY_RESET_REQUEST`、`RecoveryRequestState`、`with_recovery_request_transition`、`recovery_reset_request_shared`、`claim_recovery_reset_request`、`poll_active` 消费位、`enter_recovery`/`enter_drift_quarantine`/`transition_fatal` 三处过渡 seam，以及 `lib.rs::recovery_reset_request`；5 个请求组测试与测试锁随组迁移；`net_link_step_diag`/`ROUND_COUNT`/`TRACE_UNTIL`/`[NET-LINK-COMMIT]` 保持 qemu 门控；Cargo.toml 特性注释同步。
2. Again 1 ms one-shot：`LINK_RETRY_CADENCE_NS=1_000_000`；`RxRxFuture` 新增 `link_retry_wake`/`link_retry_timer`（生产 axtask timer，wake-only）；`poll_active` 的 link 读条件改为“未武装时 config/initial_pending，武装时仅 cadence 到期”，Again 臂不再 `publish_config`，Down/Up/NoEvent/Unsupported/Fault 与 `enter_recovery`、Recovered 分支取消武装。
3. epoch 关闭丢弃：`Device::discard_old_epoch_rx`（默认 0；EthernetDevice/测试夹具清空固定 RX slots）、`Router::discard_old_epoch_rx/discard_software_buffers`、`Service::discard_old_epoch_data_target`；调用点为 link Down 提交 terminal 之后与 resident recovery Recovered 分支 `tx_close_device_owned` 之后、`flush_recovery_close` 之前——ticket/flush/wake 次序不变。
4. fault 单次记账：4 个 service_round 检测位点改为先 `classify_fault` 再按结果记账（Recover/Drift → `record_last_error`，其余 → `record_fault`，新 helper `record_stage_fault`）；`enter_recovery` 的记账降为 last_error；terminal 提交者（`publish_recovery_fault`/`enter_drift_quarantine`）持有唯一 `fault` 递增。`last_error` 观察值不变。
5. Recovered 后重读 link：`recovery_step` Recovered 分支置 `initial_link_pending=true`，由既有 link gate 语义决定 Up（不接单窗口由下一 poll 的 link 读先于 service_round 关闭）。
6. `tests/k3-async-net-host/tests/service_owner.rs`：fault 断言 3→1（单 terminal 语义）。

**T5（K3 runtime PHY；kernel/src/platform）**

7. `k3_gmac/mod.rs`：`Phase::LinkRead`、`LinkReadStep{Pending,Done}`、私有 `LinkReg/LinkScratch/LinkRead` 微步机与 `Gmac::read_link_step`（每次至多一个 MMIO 动作：C22 触发写 → busy 观察 → DATA 读；changed-to-up 延伸 ANA/LPA/CTRL1000/STAT1000 尾调度；同一 10 ms 绝对 deadline，超时回 Idle 报 `PhaseTimeout{LinkRead}`）；`decode_abilities` 优先序从 `decode_link` 提取共享；`mac_config_for` 共享 init 与运行时应用；运行时应用为 RMW（仅替换 PS/FES/DM，TE/RE 与其余位保留）；`link_up` 快照去重（init 播种 `Some(true)`，同值快照不重复应用/发布）；恢复期间拒绝读取。
8. `k3_gmac/adapter.rs`：`NetRecoveryControl::read_link_status` 覆写（recovery → Again、fault → Io、Pending → Again、Done → Ok(bool)）；link 快照全程不触 epoch 账本。
9. `k3_async_net.rs`（board 模块）：`phy_gpio60_handler`（IRQ 内仅 MFPR35 pad ack set→readback→rearm + 边沿证据快照 + `axnet::publish_config_event()`，无 MDIO/锁/控制台）；`arm_phy_source60`（pad35 边沿武装释放 hold-in-clear、INER link-change 经 paged MDIO 使能、注册 source60 并开路由；失败仅降级事件路径并打印证据行）；`run_k3_async_net_service` 在 register(133) 之后 diagnostics 门控调用；probe 的 `snapshot_line` 追加 `phy60=events/set/rearm` 字段。普通镜像不新增 PHY 事件行为。

**测试与夹具**

10. axnet 新增 5 个先行测试（`queue_only_reset_request_is_reachable`、`link_again_waits_without_busy_wake`、`closed_epoch_discards_copied_packets`、`cancel_waiter_does_not_cancel_resident_recovery`、`resident_recovery_keeps_one_owner`）+ `RecoveringDevice` 夹具扩展（staged RX slots/discard 实现）。
11. K3 host：`fake.rs` 新增 `set_phy_link`/`set_mdio_behavior`（MDIO 延迟/stuck）；`recovery_contract.rs` 新增 3 个先行测试（`initial_and_repeated_phy_link_snapshot`、`link_down_up_preserves_queue_epoch`、`mdio_phase_timeout_preserves_owner`）。

### 与计划的偏差

1. **2 个旧 Again 语义测试按新契约更新**：`owner_config_cause_again_retains_cause_for_next_poll` 改名为 `owner_config_cause_again_schedules_one_shot_retry`、`owner_activation_initial_link_again_retries_until_commit` 的 cause 断言改为 one-shot wake 断言——契约 ② 明确废除 re-publish 行为，旧断言与目标行为直接冲突，属计划内行为变更的测试同步。
2. **fault 记账的内部形状**：计划只规定“单次 terminal 记账”；实现为分类后记账（Recover/Drift 记观察，terminal 提交者唯一计数），`flush_fault` 移到分类之前（同轮同守卫，无观察差异）。契约内等价实现。
3. **Again 武装期间的新 CONFIG cause 等待 cadence**：武装期间仅 one-shot 到期触发保留读（新 cause 至多多等 1 ms）。契约未规定该交错；有界且消除 busy 回路，非实质选择，记录在案。
4. **④ 的运行时应用落点**：changed-to-up 快照按既有优先序 decode 并以 RMW（PS/FES/DM）应用；同值下行/上行不重复应用。“captured-value”用于 Iteration 000 的重建路径，本轮运行时应用为 RMW 变体（避免运行中 MAC_CONFIG 绝对重写丢 TE），符合“沿用既有契约”的语义。
5. **link-read 机器走 ProbeEnv mmio 路径**（与 init decode 相同的 MDIO 地址事实），不复用 `GmacEnv` u32 寄存器窗口——等价实现细节，fake 与板级行为一致。

### 自检发现与修复

**A4 板测发现并修复的移交路由缺陷（跨 Iteration，交 Plan Review 裁决）**：首轮 A4 会话暴露 probe 移交后数据面死亡——`native_recovery_probe` 第 8 步 `set_enable(133, false)` 关闭了 AIA 路由，而 owner 的 arm/recheck 协议只打开设备侧 IRQ_EN、从不操作 AIA 路由，导致移交后 RI/TI 永久静默（板上证据：`irq-runs`/`ri` 自重建会话后冻结，ping 100% 丢）。该缺陷属 Iteration 000 T3 交付代码：MS12 验收时 native probe 尚不存在（路由保持 register 时的开启），Iteration 000 A3 的验收步到移交为止、未行使移交后流量。修复：移交只保留 `mask_notify`（设备通知关闭，owner 协议自行打开），删除 `set_enable(133, false)`，使路由状态与普通镜像路径不变式一致。同会话还修复 `arm_phy_source60` 未先 `unregister(60)` 导致 stage10 残留 handler 占位、注册被拒（板上证据 `registered=false`）。两修复后 k3-async/k3-async-probe/k3/k3-shell 重建 exit 0。**Iteration 000 的 A3 验收盲区（移交后流量不在最短步骤内）与本修复的归属请 Plan Review 独立裁决。**

- `record_stage_fault` 初版在 `classify_fault` 按值移动 `DevError`（非 Copy）后借用原值，E0382；改为从 `RoundOutcome` 提取错误。
- `flush_fault(&err)` 初版位于 classify 之后（同 move 冲突），移到分类之前。
- `RecoveringDevice` 夹具初版未实现 `discard_old_epoch_rx`，`closed_epoch_discards_copied_packets` RED 暴露后补齐。
- `link_read_due` 初版未在武装期间门控 `initial_link_pending`，`link_again_waits_without_busy_wake` 早轮询断言暴露后改为“武装期间仅 cadence 触发”。
- `LinkRead::Next` 初版携带多余 `head` 字段（宿主包编译 E0027/E0063），修正为仅 `deadline`。
- 遗留 Minor：`snapshot_line` 的 phy60 字段复用 `[ms12]` 前缀（延续既有文案约定，同首轮 Review finding 4）。

### 验证命令与实际结果

- T4 RED：默认面 4 项运行时失败（`link_again` 早轮询重试 2≠1；`closed_epoch` slots 1≠0；`resident_recovery` fault 2≠0；`cancel_waiter` 构造修正后以 fault 断言为 RED）；`--features queue-diagnostics` 面 E0425/E0433（请求组符号不可达）。
- T4 GREEN：axnet 三面全量 `--lib` 各一次——默认 **486 passed**、queue-diagnostics **516 passed**、qemu-diagnostics **519 passed**，0 failed。
- T5 RED：recovery_contract 3 项失败（`read_link_status` 为 trait 默认 `Unsupported`）。
- T5 GREEN：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline` 全包 **lib 1 + adapter 12 + flush 1 + recovery 8 + service 1 + stack_socket 1 = 24 passed; 0 failed**（service_owner 单次记账断言在内）。
- `rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host && /tmp/k3-gmac-recovery-host --test-threads=1` → **36 passed; 0 failed**。
- MS11 包（k3-polling-net-host）：**17 passed; 0 failed**。
- 构建门（按预算顺序，probe 最后）：`make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`、`make k3-async`、`make k3-async-probe` 全部 **exit 0**（snapshot_line 追加字段后 probe/async 复验 exit 0）。
- 警告核对：probe/async 构建日志与本轮符号无新增警告（既有 smoltcp/axnet 未用项警告维持原状）。
- host 包需编译期 `AX_IP`/`AX_GW`（既有 10.0.2.x 约定，非本轮引入）。

### A3（QEMU 恢复回归）与 A4（K3 板测）操作者交接

Persisted Evidence mode=none，无 Evidence 目录；以下会话的一句话结果与决定性串口行由操作者回报后记入本 Response 追加节，Plan Review 审核后 T5 才闭合。

**A3 — 单 hart QEMU 恢复真实入口回归（R44 手工；复用 `.agents/runbooks/ms07-qemu-single-hart-recovery-evidence.md` 原文命令）**：

1. `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH && make ARCH=riscv64 build && make tests/ms07_recovery_probe`（本轮改动后重建）。
2. 按 runbook 步骤 1–6 跑 MS07 六 case（peer/HTTP/QEMU/monitor 手工），validator exit 0。本轮共享面改动正是 reset 请求（`recovery_reset_request` 迁移后超集面行为不变）与 hmp link flap（link_policy + Down/Up 丢弃/重读）两组 case 行使的路径。
3. 预期行为差异（非回归）：恢复成功路径的 `fault` 计数按单次 terminal 语义从 2 变 0（V4/validator 若对 fatal 计数有具体期望，请原样回报差异行）；link `Again` 在 VirtIO 不行使（无 MDIO），QEMU 不覆盖该路径。

**A4 — K3 owner/link 板测（probe 镜像 `StarryOS_riscv64-k3.fit`，RAM boot 按 Iteration 000 A3 流程，不 saveenv）**：

1. 上电/RESET 长按 `s` 入 U-Boot → `usb start` → `fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit` → `setenv fdt_high 0xffffffffffffffff` → `bootm 0x2e0000000`。
2. PC 保持 `ping -t -w 100 192.168.137.2`；owner Active 后确认新判定行 `[ms13] phy60 arm mfpr=<before>-><readback> iner=true registered=true`（武装成功）。
3. 实际拔线 ≥3 s：UDP 26001 echo 会话停止应答；`[ms12] probe` 行的 `phy60=events=` 递增（每次物理事件 +1，回声不计状态变化）；`lifecycle=2` 保持（link down 不得 fault owner）。
4. 插回：至少一个完整 down→up；重新发起 UDP/TCP echo（26001/26002）成功应答（新会话在未重启的数据面上工作）；`phy60=events` 再次递增；全程 `lifecycle=2`、`owner=1` 不变。
5. 外部 RESET 回官方 Linux，一句话记录步骤 2–4 实际所见。受控 reset 请求的有界会话属 T7 范围，本轮不做板上 reset 注入。

### Persisted Evidence

- Mode: none。无 Evidence 目录；命令、退出码、判定行进入本 Response（host 全部可低成本重跑；QEMU/板测一句话结果待操作者回报后补录）。

### Experience Candidates

- None（板测复用既有 RAM boot Runbook 与首轮 A3 流程；QEMU 按 MS07 runbook 原文执行，无新增可重复高风险操作或范围外缺陷）。

### 未解决问题

- A3/A4 未执行前，T5 不勾选完成、Review Result 保持 pending；QEMU validator 对恢复成功路径 fault 计数的具体期望未知，若出现计数差异按失败原样回报交 Plan 裁决。
- `service_owner.rs` fault 断言本轮已同步为 1；T6 若扩展 socket 组合场景可能再次覆盖，届时按覆盖变化定向验证。

## Plan Review

- Review Result: rework-required
- Date: 2026-10-13

**Findings**

Blocked 复核（按公共规则，审查集中于 Blocker Handoff、部分实现与工作区状态；已完成且验证未失效的任务直接采信）：

- 工作区与交接声明一致（抽查）：移交路由修复已落——`native_recovery_probe` 移交步只保留 `mask_notify`，现存两处 `set_enable(133,false)` 分别在 probe 第 2 步 quiesce（k3_async_net.rs:850，探针独占期合法）与 MS12 诊断会话收尾（:1110，另一模式），均不在移交路径；stage10 两处 `[60,133,277]` 屏蔽+`unregister(133)`（:1048/:1182）为既有退役步；`unregister(60)` 前置（:403）、`AIA_DEBUG_TRAPS/LAST_CLAIM`（aia.rs:583-672）、T4 请求组门控（queue-diagnostics）、`LINK_RETRY_CADENCE_NS=1ms` 与 `link_retry_timer`、`discard_old_epoch_rx/discard_software_buffers` 接缝均在位。
- 采信：T4 全部契约验证（axnet 三面 486/516/519、Gate 3/4）；T5 host 侧（recovery_contract 8/8、全包 24、harness 36、MS11 17、构建门）。A3 未开始、A4 阻塞。

**裁决一（Act 请求：移交路由缺陷的归属与 Iteration 000 盲区）**：`set_enable(133,false)` 缺陷由 Iteration 000 T3 新增的 probe 移交步引入；MS12 验收时 probe 不存在、路由保持 register 开启，MS12 的 RI/TI 与 quiet 结论不受该缺陷影响。Iteration 000 A3 的书面验收步骤到移交为止（“本轮无需用socket会话代替该硬件结果”），移交后流量不在判据内，故 accepted 结论按其书面范围维持、不重开；移交后 Service 诊断“正常”与路由关闭并存也不矛盾——容量/flush 证据不依赖 IRQ 投递。盲区（移交后流量未行使）记录在案。修复（移交只留 `mask_notify`）归属本 Iteration A4 范围，本轮已落地并保留，作为 rework Cycle R2 的既有事实。

**裁决二（Act 请求：TI-after-rebuild 是否开 Iteration 000 rework）**：不开。probe session2 的零 TI 增量与探针设计一致——重建事实行打印 `irq-en=0x0` 后才跑 session2（通知关闭至移交），terminals 按设计来自同步 OWN 观察；Iteration 000 报告的 `ti=20` 不变与该设计自洽。真正的异常是移交后（payload 80 次完成、IRQ_EN=0xd041、路由开、零 trap），属本 Iteration A4 范围。

**根因排序（Plan 判断，修正 Act 的“先修 timer”前置）**：timer 槽走 `sie.STIE` 本地中断（axplat `timer_irq.rs`，`TIMER_IRQ_VECTOR=(1<<63)|5`），与 GMAC MSI 的 IMSIC/SEI `stopei` claim 路径（aia.rs）结构性分离——“单一 AIA 断层同时杀死 timer 与 MSI”不成立。历史证据表明 timer 投递在本镜像族一直工作到本轮 probe 阶段（MS12 板测 quiet/2 s 窗；Iteration 000 probe 的 `stopped-window window-ms=2000` 由 `sleep_until` 完成直证 timer 存活），且本轮 probe 的 sleep 依赖步骤（会话超时、停止窗）同样完成——断流边界在 probe 结束→payload 首个 timer 依赖点之间（移交/owner 激活/source60 武装窗口），独立于跨里程碑的 timer 布线缺陷假说。因此下一 Cycle 不得把跨 MS17 timer 修复当作前置，而应以不依赖 timer 的判别插桩先钉死边界；判别结果指向 axruntime/axtask/axplat 通用层（本 change 表面之外）时才停下交用户裁决。

**Acceptance Gaps**

- A4（T5 板测）：移交后 RI/TI/timer 投递断层未定位未修复，ping 100% 丢，三轮未收敛（Gate 5）。
- A3（QEMU 回归）：未开始（依赖 A4 结论，预算保留）。

A1/A2 的 host 证据按上采信，无需重验。当前契约没有覆盖“跨层投递断层诊断”的 repair item 与判别插桩，需新的 Current-State Evidence 与执行上下文 → rework-required。Iteration Map 不变。

**Evidence**

- Act Response 事实链（三轮板测 + LOG=info 两轮插桩）与本 Review 的代码抽查（上述行号）。
- 验证结论采信来源：本 Cycle Act Response 验证节（覆盖未失效）。

**Follow-up Decision**

创建同目录 rework Cycle `001-rework.md`（repair items R1–R4），先判别后修复，修复限本 change 表面；出表面即停交用户。本 Cycle 到此冻结。

**Iteration Plan Update**

None。

**Next Cycle**

`iterations/001-resident-owner-link-recovery/001-rework.md`（draft，待 Gate 2 批准）。

**Next Iteration**

None（同一 Iteration 001）。

## Blocker Handoff

- 触发：Gate 5——A4 板测同一问题连续三轮未收敛（ping 100% 丢），且继续诊断必须先修复一个跨里程碑缺陷（timer tick 断流），修复归属超出当前 Cycle 契约。
- 位置：T5 的 A4 板测会话（`iterations/001-resident-owner-link-recovery/000-initial.md` › Acceptance › A4）；T4 已完成不受影响。

### 已钉死的事实链（三轮板测 + LOG=info 两轮插桩）

1. 设备通知已武装：owner `arm_notify_and_check` 后 `DMA_CHAN_IRQ_EN=0xd041`（info 行直证）。
2. AIA 路由 133 开启（本轮修复删除了 probe 移交的 `set_enable(133,false)`；`CHIP.register` 注册即开路由）。
3. T5 link 分步机在板上完成（`runtime link snapshot up=true`），BMSR link=up。
4. ping（含 ARP 广播）不产生任何 RI：`proto-begin` 行 `status=0x8400`（仅 NIS+BIT10，无 RI/TI 锁存）、`rx-done=0`、`aia-traps=85` 且最后 claim 为 identity 0x85(133)（=session2 的最后一次）。
5. **TI 投递在 SWR+rebuild 之后死亡**：session1 ti=20（rebuild 前），session2 的 20 个 TX 完成 ti=0 增量、terminals=20 全部由同步 OWN 观察得来；payload 的 80 次完成同样零 TI trap，status 现值 TI=0（已被某条 poll 路径 W1C 或从未到达可投递状态）。
6. **payload 卡死在第一个 timer 依赖点**：`udp_echo_once` 的 15 s `timeout` 永不触发（四轮 boot 同一位置），`axtask::sleep` 类未来全部不完成——quiet/resume 行永远打不出来，导致 ping 之后的 `rx-done/status/aia-traps` 增量无法观测（测量死角）。
7. feature 图已验证正确：`cargo tree -e features -i axruntime` 证明 axruntime/irq、axhal/irq、axtask/irq 全部启用，`init_interrupt`（注册 timer handler + `axtask::on_timer_tick` + re-arm + enable_irqs）理论上在编译内。板上事实（6）与之矛盾，静态分析到此边界。

### 候选根因（交 Plan 排序，非 Act 结论）

1. **timer tick 断流**：FBE 自定义启动路径上 axruntime `init_interrupt` 是否真正运行、timer 槽是否被注册、`set_timer(0)` 首 tick 后 re-arm 链是否闭环——需要板上 tick 计数验证。修复归属涉及 MS17 timer 槽设计与 k3-async 镜像族的接线决策（kernel 自接线 vs axruntime 通用层），跨 MS17。
2. **TI 投递死亡**（仅 rebuild 后）：指向 Iteration 000 已 accepted 的 rebuild 机（写序列与 init"一致"的判定未覆盖中断输出语义）或 poll 路径 `w1c_status` 消费与投递的竞态——可能构成 Iteration 000 的 rework 候选。
3. **ping/RI 归因**：依赖 1 修复后的重测（v2 插桩已就位：`status/rx-done/aia-traps/aia-claim` 四字段 + `owner armed`/`link snapshot` info 行，timer 修复后一轮板测即可出判别数据）。

### 已完成 / 部分完成 / 未开始

- completed：T4（全部契约验证过 Gate 3/Gate 4）；T5 的 host 侧（RED/GREEN、recovery_contract 8/8、全量包、构建门）。
- 部分完成：T5 的 source60 接线（两处缺陷已修：`unregister(60)` 前置、移交路由保持开启；`registered=true` 板上直证）。
- 未完成：A4 板测（本轮目标）；A3 QEMU 回归（未开始，A4 的结论会影响其必要性判断）。

### 工作区状态与已过 Gate

- 工作树：T4+T5 产品与测试改动 + 本轮诊断脚手架（LOG=info 三处 info 行、payload `status/rx-done/aia-traps/aia-claim` 四字段、axplat `AIA_DEBUG_TRAPS/LAST_CLAIM` 计数器、board `gmac_irq_en_snapshot/gmac_status_snapshot`）——脚手架去留交 Plan 裁决。`StarryOS_riscv64-k3.fit` 当前为 LOG=info 诊断镜像；收尾前需重建 LOG=off 生产镜像。
- 已过 Gate：T4 Gate 3/Gate 4；T5 host 侧 Gate 3（A4 属 Gate 4 的板测段，阻塞）。
- 恢复条件：用户/Plan 对候选根因 1 给出修复归属与方案（并裁定 2 是否开 Iteration 000 rework）后，调用 `openspec-plan` 生成修复 Cycle（或授权当前 Cycle 有限修复），Act 恢复后重跑 A4；timer 修复成立后 v2 插桩一轮板测即可完成 ping 归因。
