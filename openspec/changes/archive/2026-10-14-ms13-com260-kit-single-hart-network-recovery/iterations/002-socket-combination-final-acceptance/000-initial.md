# Iteration 002 / Cycle 000：旧新 Socket 组合与真板最终验收

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T6（3.1）、T7（3.2）；T7 依赖 T6。
- Acceptance gaps：None。
- Repair items：None。
- Inherited scope：R1–R8/S01–S20 中 socket 组合与最终验收部分；Iteration 000（stop/SWR/rebuild 底层）与 Iteration 001（resident owner、link 恢复、source60/MDIO、路由修复）的全部已验收成果直接复用，不重做 bring-up。含 001 移交残余：完整 down→up 上升沿与插回后新 SocketEpoch 会话交换进入 T7 板上矩阵。
- Excluded scope：多 hart（MS14）、长稳压力、性能优化（I19）、用户态网络工具；禁止修改 AGENTS、全局 tasks/M/R/I、Runbook/Issue。

**Objective**

生产 `SocketOps` 公共入口下旧 socket 稳定错误、新 socket 真实 I/O 的组合语义有 host 直证；真板一次最终矩阵闭合 R1–R8/S01–S20：受控 reset 前后旧/新 socket、物理 link flap 完整 down→up、reset/link/flush 交错、连续两轮恢复后的容量/C4 与四协议、末尾 fail-stop 有界失败（停止未确认、backing 保留、新提交拒绝）。

**Background**

Iteration 001 收尾的两项用户豁免与移交（原话见 001-rework Plan Review）：A3 QEMU 回归永久豁免——本轮不得声称"采信 001 QEMU 结论"，T6 触碰 axnet 共享面时按覆盖变化定向 host 重验，该缺口随 change 收尾入档；A4 残余（上升沿 + 插回后新会话）移交本 Iteration T7。001 另移交一个设计注意：payload resume 触发条件计数任意 ISR publish，PHY claim 可提前消费 resume 窗——T7 矩阵的 flap/resume 时序设计须避开或细化该触发条件。

**Investigation Facts**

- Current Baseline：Iteration 001 最终 Act Response（001-rework）+ accepted Review。全部改动在当前未提交工作树（HEAD `d07285c0` 之上，+2521/−153）。验证基线：axnet 三面 486/516/519、k3-async-net-host 全包 24/24（recovery 8/8）、harness 36/36、MS11 包 17/17、timer harness 8/8、五个构建门 exit 0、四轮板测（判别/修复确认/S07/S06+S08）。覆盖未变时直接采信。
- host mirror 模式（T6 直接复用）：`tests/k3-async-net-host/tests/stack_socket.rs`（MS12 T6）——生产 K3 adapter + Service + `axnet::SocketOps` + `poll_io`/`timeout` 真实公共入口，测试线程扮演设备半（FakeWire）与 ISR 提示半，宿主显式事件/时钟步进；不宣称真实 timer/DMA（宿主 `wall_time()`≡0，timer wheel 结构性不可触发——板上方可）。`recovery_socket.rs` 尚不存在，按同模式新建。
- 公共入口面：`axnet::{SocketOps, RecvOptions, SendOptions, Shutdown, SocketAddrEx, flush}`；payload 侧 `UdpSocket::new()`/`TcpSocket::new()`（bind/listen/accept 经 `poll_io(&listener, IoEvents::IN, …)`，k3_async_net.rs:1576/1636）。flush 的 socket 级路径：`axnet::flush()` → `FlushFuture`（flush.rs:149），T4 已交付 `flush_recovery_close`（旧 epoch flush 在 epoch 推进前 settle，即 abort 非 C4 的机制基础）。
- socket epoch/terminal 语义（T4 已交付，T6 验收对象）：`commit_socket_epoch_terminal_for`、`open_next_socket_epoch`、`open_socket_epoch_after_recovery`、link Down 关闭当前 epoch + `socket_epoch_wake`、`discard_old_epoch_rx`（旧 epoch queued RX 不交付新 socket 的机制基础）。
- 板上矩阵载体：`k3_async_net.rs::k3_net_diagnostics::run_diagnostics`（:1389，`all(k3, k3-net-diagnostics, riscv64)` 门控）现有 settle/quiet/resume 相位与 UDP/TCP echo/listener 载荷；判别行/`TIMER_DEBUG_TICKS`/AIA 计数器/`snapshot_line` 保留为观察面。受控 reset 请求入口：`axnet::recovery_reset_request()`（queue-diagnostics 面，K3 probe 构建可达——kernel/Cargo.toml:38）。
- T7 fail-stop 注入接缝（新建，诊断门控）：现状 `Gmac::resetting_step` 的 `RESET_STEP_CONFIRM` 每次读 `DMA_MODE` 判 SWR 自清。诚实注入设计：SWR 写真实发出、确认读执行一次（诚实现值）、后续确认步扣留读（诊断 hook），沿既有绝对 1 s deadline 进入 `ResetTimeout`——区分"请求没发出"与"发出未确认"，不改 MMIO 原值、不伪造 SWR 成功、不把写前 0 值当 stuck。host fake（`set_swr_behavior(latency, stuck)`、`set_reg/get_reg`、`advance_clock`）可先为该 hook 建立 RED。
- 001 已知 Minor 延续：`snapshot_line` 的 `[ms12]` 前缀、停止窗在飞 claim ±1 残差、abnormal 良性增长——观察记录，随收尾清理统一处置。

**Code and Critical Path**

1. host 测试：新 `tests/k3-async-net-host/tests/recovery_socket.rs`；`src/fake.rs` 事件/时钟脚本按需扩展；现有 stack_socket/service_owner/recovery_contract/adapter_contract 回归保持 GREEN。
2. 产品（仅限 T4/T5 已确定接缝的必要修复）：`crates/axnet` socket/flush/readiness 面、`k3_gmac` 确认步注入 hook、`k3_async_net.rs::k3_net_diagnostics` 恢复会话与矩阵编排。
3. 判别行/计数器观察面复用；普通镜像无注入、无新输出路径。

**Implementation Guidance**

先 T6（host RED→GREEN），再 T7 注入接缝（host RED→GREEN）与矩阵编排，最后一次最终真板会话。T6 的产品修复限既有接缝——发现需要 socket 框架级新设计即返回 Plan，不能借最终测试扩范围。T7 矩阵编排吸收 001 残余与 resume 触发注意；矩阵各场景的观察直接用现有判别行/`snapshot_line` 字段，不新增判定层。

**Task Contracts**

### T6：旧新 Socket 组合契约（生产 SocketOps host 面）

- Requirement/Scenario：R2/R5/R6/R7 的 socket 面；S04/S05/S13/S16/S17。
- Depends on：Iteration 001 accepted（已满足）。
- Targets：新 `tests/k3-async-net-host/tests/recovery_socket.rs`（stack_socket.rs host-mirror 模式：生产 adapter+Service+`SocketOps`+`poll_io`/`timeout`，FakeWire 设备半 + ISR 提示半）；`tests/k3-async-net-host/src/fake.rs` 按需；必要的产品修复限 T4/T5 已确定接缝（axnet socket/flush/readiness、discard/terminal 既有机制）。
- Current behavior：stack_socket.rs 覆盖正常数据面与稳定 fault 传播；恢复语境（旧 socket terminal 次序、新 epoch 新对象、pending flush abort、多 waiter、cancel/submit 交错、queued RX 隔离）无生产 SocketOps 级 witness。
- Required behavior：① 旧 TCP/UDP/listener 在 reset（受控请求）与 link down 下得到稳定错误，terminal 先于 wake（等待者可见错误而非永久 Pending）；② pending flush 被恢复 abort，不产生伪造 C4 成功；③ 多 waiter poll/select 与实际 I/O readiness 一致（旧错误/新就绪各归其位）；④ cancel/submit 与恢复交错不伪造完成、不重复回收；⑤ 旧 epoch queued RX 不交付新 socket（discard 机制经公共入口可见）；⑥ 新对象在新 epoch 完成真实 UDP/TCP/listener I/O（FakeWire 全字节交换）。
- Preserve：既有 5 个测试文件全部 GREEN；T4/T5 交付语义（单次记账、1 ms Again、epoch 账本）；`NetRecoveryControl` trait 不改。
- Forbidden：另建 socket 框架/测试专用公共 API；宣称真实 timer/DMA（宿主时钟为 0ns stub）；身份型证据。
- First test：先 RED：`old_flush_is_aborted_not_c4`、`old_socket_terminal_precedes_wake_new_socket_is_clean`，加多 waiter/cancel-submit/queued-RX 隔离/新对象 I/O 用例；宿主显式 fake 事件/时钟步进触达真实公共入口。
- GREEN condition：上述全过；恢复后新 socket 全字节 echo/listener accept 成立；旧对象错误稳定重复；全包回归 GREEN。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline --test recovery_socket`；轮末全包一次（Iteration 预算）。
- Stop when：需要 socket 框架级新设计或既有接缝无法表达目标语义；返回 Plan。

### T7：真板最终组合矩阵与 fail-stop 末场景

- Requirement/Scenario：R1–R8 收口；S01–S03/S06–S12/S15/S18/S20 的板上部分（含 001 移交：完整 down→up 上升沿 + 插回后新 SocketEpoch 会话交换）。
- Depends on：T6 Gate3 PASS。
- Targets：`kernel/src/platform/k3_gmac/mod.rs`（确认步诊断注入 hook，`k3-net-diagnostics` 门控）；`kernel/src/platform/k3_async_net.rs::k3_net_diagnostics`（有界恢复会话与最终矩阵编排）；判别行/`snapshot_line` 观察面复用；受控 reset 经 `axnet::recovery_reset_request()` 真实入口。
- Current behavior：诊断载荷有四协议/settle/quiet/resume 相位与 probe 的 stop/rebuild 会话；无受控 reset 的 resident-owner 板上行使、无 socket 级旧新对象矩阵、无诚实 fail-stop 注入。
- Required behavior：① 受控 reset 请求由 resident owner 驱动实际恢复，前后旧/新 socket 行为按 T6 语义板上直证；② 物理 link flap 完整 down→up（上升沿事件、插回后新 SocketEpoch 会话交换——001 移交项）；③ reset/link/flush 交错与连续两轮成功恢复后的容量/C4、四协议；④ 末尾 fail-stop：注入扣留确认读（SWR 写真实发出、首读诚实、后续扣留），绝对 deadline 进入"停止未确认"错误，随后不再提交、不再改环、backing 保留；普通模式无注入、无专用 CLI 参数；⑤ resume 触发条件按 001 Minor 机制细化或避开（PHY claim 不得提前消费 resume 窗）；⑥ 直接记录原生输出、PC 实际字节与错误，不用专用 validator。
- Preserve：普通 k3-async 镜像零行为变化；已验收底层语义；RAM boot 流程与无持久写入；外部 RESET 回官方系统。
- Forbidden：注入改 MMIO 原值/伪造 SWR 成功/把写前 0 值当 stuck；为矩阵新增控制协议或判定层；身份型证据。
- First test：注入接缝先在 `tests/k3-gmac-host-harness.rs` RED（confirm-withhold：一次诚读 + 后续扣留 + deadline `ResetTimeout{raw:…}`，raw 为最后诚读现值；恢复期间拒绝提交/改环断言）；板上矩阵 GREEN 以真实会话判定。
- GREEN condition：注入 host GREEN；最终板测矩阵全场景有直接判定行；末尾 fail-stop 会话显示停止未确认、零写回前提保持、backing retained；操作者外部 RESET 回官方系统。
- Verification（agent）：受影响定向测试 + 轮末全包一次；构建门：普通与 probe 镜像按最终载荷各一次（probe 最后）；axnet/axplat 共享面未再变时采信 001 host 结论（QEMU 无结论可采信——已豁免）。
- Verification（操作者）：一次最终真板组合会话（RAM boot runbook 流程）；各场景最短步骤与直接可观察结果由 Act 在接线后写入 Act Response；未跑或不符合场景保持 T7 未完成。
- Stop when：真板行为否定已 accepted 底层（stop/rebuild、epoch、link 语义）；立即停交 Plan。

**Invariants**

- 唯一 queue owner 驱动恢复与矩阵会话；IRQ 只发布 cause；软件标签不证明硬件状态。
- QueueEpoch 只在 begin 边界推进；socket terminal 先于 wake；flush abort 先于 epoch 推进（Finding 1 次序）。
- fail-stop 注入只在诊断构建存在；普通镜像零差异。
- 板测经 RAM boot runbook、无持久写入；外部 RESET 不算 runtime reset 验收。
- 判定只用原生输出/退出码/直接观察；无新增判定层或身份机制。

**Non-goals**

多 hart、长稳 soak、性能、第二 UART/设备、用户态网络工具；不修改 trait；不重做已 accepted 的 bring-up。

**Acceptance**

- A1（T6；R2/R5/R6/R7 socket 面）：recovery_socket 先 RED 后 GREEN，旧对象稳定错误、flush abort、多 waiter、隔离与新对象 I/O 全过。
- A2（T7 agent 侧；R8/S19）：注入接缝 host RED→GREEN；轮末全包与构建门通过。
- A3（T7；R1–R8 板上收口）：一次最终真板组合矩阵，含 001 移交残余（down→up 上升沿 + 插回新会话）、连续两轮恢复容量/C4、四协议与末尾 fail-stop；操作者执行、Act 审核 PASS 后 T7 才闭合。
- A4（收尾）：最终 diff 自检后仅填写 Act Response；脚手架最终清单（计数器/判别行/`[ms12]` 前缀等）在本 Cycle Plan Review 裁决。

**Verification**

host 定向与全包入口同 Iteration 000/001 命令清单；板测人工项最短步骤 + 每步直接可观察结果，一句话记录；QEMU 已豁免不再列入。昂贵预算：轮末 K3 host 包一次、最终真板会话一次、普通/probe 构建各一次（probe 最后）；T6 产品修复若改 axnet 共享覆盖，仅受影响面定向重验。

**Gate 2 Readiness**

PASS：2026-10-14 用户原话“批准”。技术自检结论采信未变（调查完成、契约自包含、无 TBD、RTM 无 Missing/Simplified；BLOCKED：None；WAIVED：None——001 的两项用户豁免已在案，本轮不重复豁免任何项）。Status 转 `ready`。

**Persisted Evidence**

- Mode: none。命令、退出码、判定行、矩阵一句话结果进 Act Response；安全 RAM 板测可低成本重跑。

**Risks and Notes**

- 矩阵单会话时长受操作者节奏限制——场景编排可跨 boot 分组，但每个场景必须单 boot 内完整（reset 前后同 boot）。
- 上升沿插回窗口的时序（PHY settle 后再发新会话）在编排中给出固定等待，避免与 resume 窗互踩（001 Minor 机制）。
- fail-stop 注入后设备处于 Failed/保留态——矩阵把它固定在末尾，之后仅观察不再恢复。
- AX_IP 编译期约定、`service_owner` fault==1 断言等既有事实延续。

## Act Response

- Status: reported
- Date: 2026-10-14（host 侧实施与真板会话同日完成；板测由操作者执行，Act 依据串口与 PC 原文审核）

### 实际改动（文件与符号）

**T6（host 测试面；axnet 产品代码零改动）**：

- 新 `tests/k3-async-net-host/tests/recovery_socket.rs`：生产 K3 adapter + Service + `SocketOps`/`poll_io` 全公共入口的旧新 socket 组合契约（单测试函数、分相，host mirror 模式）。已行使：旧 epoch 数据面存活（UDP 26011 全字节 echo）；旧对象受控 reset 终态（UDP waiter、TCP listener accepter 均得 `ConnectionReset`，terminal-first 优于其排队旧数据——旧 socket 上的 stale 数据报永不浮出）；raw 第二等待者扇出（`Pollable::register` 计数 waker ≥1）；pending flush 在恢复中 abort 得 Err 非 C4；恢复后同端口新对象真实 I/O（UDP 全字节、TCP listener accept+echo+主动关闭 FIN 无 RST）、新对象只收新数据（queued RX 隔离）；`queue_generation`/owner/fault 账本全程守恒断言。
- `tests/k3-async-net-host/src/fake.rs`：`FakeWire::reset_rx_fetch()`——原地重建后设备半完成游标与驱动 head 一致归零（T6 契约明示的按需 fixture 扩展）。
- `tests/k3-async-net-host/Cargo.toml`：补 `axlog = "=0.3.0-preview.2"`。**基线缺陷修复**：001 的 A4 诊断日志（`adapter.rs` 两处 `axlog::info!`）使 host 包在该工作树状态下无法编译（`E0433`），此前"全包 24/24"结论产生于该日志加入之前；先修才能建立本先行测试。

**T7（注入接缝 + 矩阵编排 + 窗口统一）**：

- `kernel/src/platform/k3_gmac/mod.rs`：`RESET_CONFIRM_WITHHOLD` 静态开关 + `diagnostic_arm_reset_confirm_withhold(bool)` + `Recovery.diag_confirm_withhold` 字段，全部 `#[cfg(any(feature = "k3-net-diagnostics", test))]`——普通镜像（k3-async/k3/k3-shell）整体编译剔除，普通构建零差异；host harness（rustc `--test`）与诊断构建可编译。`resetting_step` 确认步诚实注入：SWR 写真实发出，首次确认读真实执行；若 SWR 仍置位则记录该现值并扣留后续所有确认读（零板访问：无读、无写、无提交），沿既有绝对 1 s deadline 进入 `ResetTimeout { raw = 最后诚读现值 }`。不改 MMIO 原值、不伪造 SWR 成功、不把写前 0 值当 stuck；扣留窗内 `try_submit` 拒绝新提交。
- `kernel/src/platform/k3_async_net.rs`（`k3-net-diagnostics` 面）：
  - **窗口统一 25 s**（2026-10-14 用户指示，原话见 Task Contract 附加指令）：board `RAW_SESSION_NANOS` 30 s→25 s、`STOPPED_WINDOW_NANOS` 2 s→25 s；diagnostics `PHASE_CAP` 10 s→25 s、`SESSION_WAIT` 15 s→25 s、`QUIET_WINDOW` 2 s→25 s、`RESUME_WAIT` 10 s→25 s、`SETTLE_CAP` 10 s→25 s。保持不动：`SETTLE_QUIET` 500 ms（claim 流静默判定阈值，非观察窗）、`HOLD_LEASE` 200 ms、`SESSION_OP` 5 s（既有 T6 op bound 契约）、`SERVICE_START` 5 s（boot 边界）、全部恢复阶段绝对 deadline（R6 设计语义）。
  - **resume 触发细化**（001 Minor 移交）：触发条件从"任意 handler run/任意 ISR publish"改为 ri/ti（MAC cause）增长或 reaped 增长——PHY source-60 claim 不再提前消费 resume 窗。
  - `run_capacity_flush_cycle()`：原 phases 1–3（两级容量边界、显式释放、flush Pending→release→C4）原样抽取为可复用循环，供恢复后两轮容量/C4 复用；`run_diagnostics` 尾部接入 `run_recovery_matrix`。
  - **`run_recovery_matrix()`**（板上最终矩阵，全部 25 s 窗、全部原生判定行、payload 不判 PASS）：R1 旧对象会话（UDP 26011 / TCP 26012 echo）→ parked 旧 waiter（UDP 26013 recv + 26012 listener accept，终态类别以原子码打印）→ 2 s hold + flush Pending → **reset-1**（wait lifecycle 2 → waiter 终态行、flush abort 行、显式 release、容量/C4 第 1 轮、同端口新对象会话）→ **拔线**（flap waiter 期望 NotConnected + snapshot_line）→ **reset-2（link down 交错，恢复第 2 次）**（wait 2）→ **插回**（phy60 上升沿 25 s 窗；001 残余）→ ping+UDP+TCP 新会话（001 残余：插回后新 SocketEpoch 会话交换；四协议合计 ARP/ICMP/UDP/TCP）→ 容量/C4 第 2 轮 → **fail-stop（固定末尾）**：arm 注入 → reset-3 → wait lifecycle 3 → `fail-stop-observed` snapshot 行 + 恢复后 enqueue 拒绝观察行（`backing=retained external-reset-next`）。矩阵 flush 锚定用 2 s hold（覆盖 1 s quiesce 窗，保证板上 abort 可判定），观察后显式 release，auto-release 保持 0。
- `tests/k3-gmac-host-harness.rs`：fixture 扩展（`dma_mode_reads` 计数器、`advance_clock`）+ 先行测试 `confirm_withhold_injection_reports_honest_raw_timeout`（RED：`E0425 cannot find diagnostic_arm_reset_confirm_withhold` → GREEN）。

### 与计划的偏差

1. **T6 host 触发面收窄：owner 驱动的 link-down 触发移交 T7 板上矩阵**。原因：owner link 步进首调用必然 `Again`，续跑由 1 ms one-shot retry timer 驱动；host 构建 timer wheel 结构性不触发（`wall_time()`≡0 stub，stack_socket.rs 头注声明的同一证据类），物理 flap 的公共面见证本就属 T7（"真实计时链由 T7 验证"）。LinkDown 公共终态已由 001-rework S07 板测直证（`NotConnected` 有界终态、source-60 端到端），本偏差不减少已验收行为覆盖。T6 host 以受控 reset 触发覆盖同一批公共终态语义（ConnectionReset）。
2. **基线缺陷修复**（超出 T6/T7 文件面、验证前置）：host 包补 axlog 依赖（见上）。此前无任何 axnet 产品代码改动。
3. **用户指示窗口统一 25 s**：测试观察窗参数，非契约语义改动；op bound 与恢复绝对 deadline 不变。

### 未解决问题（交 Plan Review 裁决）

**U1：recovery_socket 尾相 post-recovery C4 探针有界失败（host）**。现象：受控恢复成功后（lifecycle=2、owner=1、fault 守恒；旧对象终态/flush abort/多 waiter/隔离/新对象 I/O 全部先过），恢复后新 flush 持续 Pending 至 20 s 有界失败。失败快照：`lifecycle=2 flush-target=21 flush-ok=0 flush-err=1 stack-rounds=30 stack-work=27 task-poll=30`（21 = pre-reset 被 quiesce cancel 的 B 票）。读码核对：`advance_epoch` 清 `first_lost`（fixed_queue.rs:411）、live 集在 advance 时为空（debug assert 未触发）、`has_live_at_or_before(21)` 按票号不过滤 epoch——Pending 与判定链预期（Done 或 Lost）不符；且 `flush_target=21` 意味着 flush 时刻 `last_accepted` 仍为 21，而新 epoch 会话已发生多次成功发送，两者并置未解释。用户指示"不阻塞在这里，直接进行后续的测试"，已将探针改为有界失败（不再挂死 harness）。候选：(a) 测试期望错误（跨 advance 的旧 target flush 语义）；(b) 产品 flush 判定链缺陷（D8/C4 语义）；(c) 尚未定位的中间机制。

**U2（板 A1）：reset-1 后诊断 enqueue 首帧即拒**。板上现象：`reset-1 committed` 后容量第 1 轮 `enqueue i=0 accepted=false tx-slot=0 high-water=64 tx-full=1`（第 1 帧被拒，循环按设计断开）→ `accepted=0` → 25 s 窗超时 → `auto-release=1`（200 ms lease 在 25 s 等待内自然过期，此点无害）；随后的 `flush c4 ok` 与 **new-1 会话全通**证明数据面健康，被拒的只是诊断 enqueue 路径的恢复后首帧。拒绝点在 `emit_frame_dormant`（ethernet.rs:235 门 / 238 preflight / 243 alloc）三者之一：`recovery_hold` 在 Recovered 已清、`link_held` 在 reset-1 前从未置位（无 down 转换），preflight 时 slot 全空——静态核对与首帧拒绝矛盾，机制未定位。恢复后容量边界第 1 轮 witness 因此未取得。

**U3（板 A2+A3）：插回上升沿不可达 + 门关闭流量下串口停摆（实质，001 残余无法达成）**。链路：source-60 按 MS10/MS13-T5 仅武装下降沿（拔线）→ 插回上升沿无中断 → owner 无触发重读 link（非 Again 结束后 retry 已取消、空闲不轮询）→ link TX 门与 socket epoch 持续关闭。板上证据：`link-up phy60-event timeout`；PC 侧 ping 显示"无法访问目标主机"（连 ARP 应答都被门拦截）、随后 UDP/TCP 双 5 s 超时——**插回后新 SocketEpoch 会话交换（001 移交残余）按当前实现无法达成，设计缺口**。并发症（A3）：`new-session-2` 指令行后串口完全停摆——连 terminal-first 应立即返回的错误行都没有，payload 卡在 `UdpSocket::new()`/bind/首次 poll 对 `SOCKET_SET` 自旋锁的等待上；插回流量（操作者 ping + Windows 后台 ARP/ND）驱动 runner 自醒风暴（stack_runner.rs:544-561 的 socket_changed/rx_ready 自醒条件）反复持锁，boot 任务自旋等锁（axsync::Mutex 纯自旋、单 hart）、低优先级 runner 得不到调度释放锁——优先级反转/饿死停摆，**payload 的"有界失败并打印"设计承诺被破坏**。容量/C4 第 2 轮与 fail-stop 判定行因此未取得。

### 真板会话（2026-10-14，操作者执行，Act 依据串口与 PC 原文审核）

已见证（判定行逐条对应）：

1. 原地重建后基线：`rebuilt-session sent=20 terminals=20 rx-frames=19 irq-runs=40 ri=21 ti=20`；`probe ok handover-next backing=retained`；`owner-active irq-en=0xd041`（owner 协议武装）、`stack-active started=1`。
2. 恢复前容量/flush：81 帧两级边界（80 accepted、第 81 拒 `tx-slot=64 tx-full=1`）→ release 排空 → `flush c4 ok` → `auto-release=0`。
3. 协议相：`proto udp ok bytes=8`、`proto tcp ok bytes=8 closed=1`；settle 后 quiet 窗实跑 25 s（ticks 11965→14471）；quiet 窗内 ri 35→39（对端后台流量进入，"计数冻结"前提本轮不成立；lifecycle=2/fallback=0 保持，机制无异常）；`resume` 在 ri +1 触发——细化后的 MAC-cause 触发工作正常（001 Minor 修复生效）。
4. R1 旧会话：`matrix old udp ok bytes=6`、`old tcp ok bytes=10 closed=1`（PC 原文 `b'mx-old'` / `b'mx-old-tcp'` 逐字节一致）。
5. **reset-1**：`committed lifecycle=2 owner=1 queue-gen=331 fault=0`；`reset-1-udp-waiter waiter=connection-reset`、`reset-1-listener-waiter waiter=connection-reset`（旧对象受控 reset 终态板上直证，双对象一致）；`reset-1 flush aborted err=BadState`（恢复中 flush abort 非 C4）。
6. new-1（新 epoch 同端口新对象）：`new-1 udp ok bytes=6`、`new-1 tcp ok bytes=10 closed=1`（PC 原文逐字节一致；此前操作者 ping 3/3 回复补上 ICMP 见证）。
7. link-down：`link-down waiter=not-connected`（有界终态）+ `phy60=events=1 aia-claim=0x3c003c`（0x3c=60，source-60 下降沿端到端）。
8. **reset-2（link down 交错，第 2 次成功恢复）**：`committed lifecycle=2 owner=1 queue-gen=403 fault=0`。

四协议覆盖：ARP（全程会话）+ ICMP（ping 3/3）+ UDP + TCP，四类均有真实交换。

未取得（原因见"未解决问题"）：插回后新 SocketEpoch 会话交换（U3 设计缺口）；容量/C4 第 2 轮与 fail-stop 判定行（U3 停摆挡住）。操作者已外部 RESET 回官方 Linux（用户原话"已恢复"）。

### 验证命令与实际结果（2026-10-14 实测）

- `rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host && /tmp/k3-gmac-recovery-host --test-threads=1`：**37/37 通过**（36 既有 + 新增 confirm_withhold 注入测试；先行 RED 已观察）。
- `AX_IP=10.0.2.15 AX_GW=10.0.2.2 cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`：lib 1、adapter_contract 12、flush_contract 1、recovery_contract 8 全绿；service_owner 1、stack_socket 1 全绿（单独补跑）；**recovery_socket 1 failed**（U1 尾相有界失败，其余相全过）。
- `make k3-async` exit 0（普通镜像；注入面 cfg 剔除，零行为变化）。
- `make k3-async-probe` exit 0（诊断镜像，**probe 最后构建**，FIT 打包落盘；含矩阵编排与 25 s 窗口）。
- axnet 三面 `--lib`（486/516/519，001 结论）：本轮 axnet 产品代码零改动，覆盖未变，采信；QEMU 维持 001 用户永久豁免。

### Persisted Evidence

- None required（模式 none）：命令与判定行已录入本 Response；安全 RAM 板测可低成本重跑。

### Experience Candidates

- Issue 候选：**U1**（若 Plan Review 判定为产品缺陷，则为本 change 范围内缺陷，随 rework 处理；若判定为既有 D8 语义问题则可能构成范围外 Issue）——证据见上"未解决问题"与 `/tmp/hostpkg.log`。**U2/U3** 均在本 change 表面内（link 门/source-60 武装/runner 自醒/自旋锁为 T4/T5/既有共享面），随 rework 处理，不另立 Issue；U3 若被 Plan 判定涉及 axnet 既有调度/锁设计（而非本 change 引入），可能构成范围外 Issue 候选。Runbook 候选：None（本轮无新增可重复高风险操作；板测沿用既有 RAM boot Runbook）。

## Plan Review

- Review Result: rework-required
- Date: 2026-10-14

**Findings**

1. **U1 定位（host harness 缺陷，产品 flush 判定链正确）**。独立重跑复现同一失败快照后，用 gdb 在第二次 `flush_begin` 与 `has_live_at_or_before` 断点取票账本真值：恢复前 tracker `next=18`、`last_accepted=17`（pre-reset 17 帧即票 17）；恢复后 `next=22`、`last_accepted=Some(21)`、`live_len=1`、`first_lost=None`、live 数组残留一张 `Some((当前epoch, 21, DeviceOwned))`。即恢复后新 epoch 会话共分配了票 18–21（UDP echo、SYN-ACK、echo、FIN 四帧），尾相 flush 的 target=21 是**新会话最后一帧（FIN）**而非 Act 推断的"pre-reset 被 quiesce cancel 的 B 票"（该票为 17）。`Pending` 是判定链的正确答案：票 21 仍 DeviceOwned 在途（fixed_queue.rs:560-587 `has_live_at_or_before` 为真）。在途原因：fake 的 TX 完成只在 `drain_board_tx()` 中发生（fake.rs:424-439 逐 OWN 位清零），而尾相探针循环（recovery_socket.rs:917-933）只调 `owner.pump(8)`，既不 drain 也不 publish——最后一帧的完成永远不发生，flush 正确地不结算。偏差分类：Act 实现缺陷（harness 循环未驱动设备半）+ Plan 遗漏（契约未规定探针循环的驱动机制）；Act 报告的候选 (b)"产品 flush 判定链缺陷"、(c)"未定位中间机制"排除。
2. **U2 定位（payload harness 缺陷，产品门行为正确；确定性复现，非竞态）**。`wait_lifecycle`（k3_async_net.rs:2175-2199）在 `recovery_reset_request()` 之后首次采样 lifecycle：单 hart 协作调度下 payload 未让出即采样，queue owner 尚未 claim 请求，lifecycle 仍为 Active(2) → **立即返回恢复前的陈旧快照**，"reset-1 committed" 行在恢复开始前打印。此后整列车厢（waiter 等待、flush 单次 poll、release、容量轮）在恢复进行中执行：容量轮 HOLD+首帧 enqueue 撞上合法保持的 `recovery_hold` 门（async_rx.rs:2239 `enter_recovery` 置位）→ 首帧被拒、循环按设计断开 → 25 s 等待超时（期间 200 ms hold lease 自然过期 → auto-release=1）→ 而恢复在 25 s 等待内真正完成，门打开，故后续 flush c4 ok 与 new-1 全通。板上每一行均吻合；host 对照循环（recovery_socket.rs:676-692）本身就有 left-Active 护栏，payload 未沿用。偏差分类：Plan 遗漏（000 Investigation Facts 引用了 host 循环的离开-再回语义，但未将护栏写入 T7 契约）。Act 对三门（235/238/243）的静态排除成立，但"机制未定位"由本条闭合。
3. **U2 附带产品加固发现（Minor，随 rework 一并处理）**：`recovery_step` 在 guard 内提交 `lifecycle:=2`（async_rx.rs:2521），而 `set_recovery_hold(false)` 在 guard 释放后的调用方分支（2478）才执行——两行之间存在 µs 级窗口。K3 构建 axfeat 启用 `sched-rr`（kernel/Cargo.toml:115，经 axtask `preempt` 特性、`on_timer_tick`→`scheduler_timer_tick`，api.rs:161），timer tick 抢占使该窗口可被观察者击中：观察者见 committed 后立即 enqueue 会被门拒绝。协作语义下不可达，属加固而非缺陷；随 R2 的次序重排消除（committed ⟹ 门开的调度无关不变量）。
4. **U3 上升沿定位（产品平台设计缺口，001 移交项结构性不可达）**：`pad35_edge_setup`（phy_probe.rs:433-438）仅置 `PAD_EDGE_FALL`（沿用 DTB `<35 2>` 下降沿声明，phy_probe.rs:446-447）。插回产生上升沿 → pad 无中断 → 无 CONFIG cause → owner 在恢复后的 initial link 重读（async_rx.rs:2526-2532，一次性消费 `initial_link_pending`）之后无任何触发再读 link → `link_held` 与 SocketEpoch 维持关闭——板上 `link-up phy60-event timeout`、PC ping"无法访问"（ARP 应答也被 TX 门拦）、new-session-2 无法达成，全部吻合。handler（k3_async_net.rs:408-418）边沿无关（ack+publish_config_event），owner 的 Up 路径（service.rs:991-1010：开下一 SocketEpoch + 清 link 门 + 状态去重吸收每 flap 双沿）已具备，缺的只是双边沿武装。偏差分类：Plan 错误/新证据（000 契约把"上升沿事件+插回新会话"列为 T7 required behavior，但未调查 pad 武装位的可达性）。
5. **U3 停摆定位（U3 上升沿缺口的落7，非独立缺陷）**：门持续关闭时，操作者 ping 与 Windows 后台 ARP/ND 持续入流，stack runner 的自醒条件（stack_runner.rs:540-561：`rx_ready`/`socket_changed` 等不满足即重臂）驱动 runner 反复持 `SOCKET_SET`；boot 任务在 `axsync::Mutex`（纯自旋）上等待，`sched-rr` 抢占下形成优先级反转/饿死，new-session-2 连有界失败打印都无法输出。上升沿修复后插回即重开门、入流正常交付、自醒风暴平息，该场景消失。**残余风险**（长时间门关闭 + runner 自醒风暴 + 自旋锁饿死为 axnet/axsync 既有结构性属性，非本 change 引入）：**范围外 Issue 候选，只报告不落账**（Recorder 按用户指令处理）。
6. **A4 脚手架裁决（用户交本 Review 决定）**：判别行/`[ms12]` 前缀/计数器/001 的 ±1 claim 残差与 abnormal 良性增长——为保持与 001/002-000 板测日志的可比性，rework 板测重跑期间观察面零改动；装饰性清理（前缀更名等）延迟到 rework accepted 后的收尾 Review 统一处置，本次不作为修复项。
7. **偏差与验证采信**：Act 声明的三项偏差（T6 host 触发面收窄并交 T7、host 包 axlog 依赖基线修复、25 s 窗口统一）与代码一致，窗口统一已有用户原话授权，均接受。A2 采信：harness 37/37、adapter/recovery/flush/service/stack 各面、构建门 exit 0（recovery_socket 失败除外——我独立重跑复现了同一 panic 快照，命令见 Evidence）。axnet 三面本轮无产品改动，采信 001 结论。

**Acceptance Gaps**

- A1（T6）：recovery_socket 尾相 post-recovery C4 探针失败（其余相全过）。
- A3（T7 板上）：四项判定行未取得——reset-1 容量第 1 轮两级边界 witness（U2 假 committed 致首帧被拒）；插回上升沿 phy60-event 与插回后新 SocketEpoch 会话交换（U3 设计缺口）；容量/C4 第 2 轮与 fail-stop 判定行（被 U3 停摆挡在矩阵中途）。
- A2、A4：无缺口（A4 裁决见 Finding 6）。

**Evidence**

- 复现：`AX_IP=10.0.2.15 AX_GW=10.0.2.2 cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline --test recovery_socket -- --test-threads=1` → 同一 panic（`flush-target=21 flush-ok=0 flush-err=1`，2026-10-14 本 Review 重跑）。
- gdb 真值（同 binary，断点 `Service::flush_begin` 第二命中 + `TicketTracker::has_live_at_or_before`）：pre `next=18 last=17`；post `next=22 last=Some(21) live_len=1 first_lost=None`；live[0]=`Some((当前epoch, 21, DeviceOwned))`。
- 代码位置：recovery_socket.rs:914-935（尾相循环）、fake.rs:424-439（TX 完成仅在 drain）、fixed_queue.rs:560-587、service.rs:1146-1243、k3_async_net.rs:2175-2199/2296-2332、async_rx.rs:2239/2472-2537、phy_probe.rs:413/433-447、stack_runner.rs:540-561、kernel/Cargo.toml:115。

**Follow-up Decision**

U1/U2 为 harness 缺陷（host 测试循环 + payload 等待护栏），修复在既有文件面内；U3 上升沿修复需修改 Iteration 001 已交付的 `phy_probe` 平台武装位——超出 002 既有 Task Contract 的 Targets，且需要新的先行测试入口（`tests/phy-probe-host-harness.rs` 建模 MFPR pad，可先 RED）与一次新的真板矩阵会话。属"需要新的 repair item 与重新通过 Gate 2"，创建同目录 rework Cycle；Iteration Map 与 RTM 不变（修复服务于原 A1/A3）。

**Iteration Plan Update**

None。

**Next Cycle**

`iterations/002-socket-combination-final-acceptance/001-rework.md`（draft；技术自检 PASS，等用户批准转 ready 后交 Act）。

**Next Iteration**

None（待 rework Review accepted 后收口本 change）。
