# Iteration 002 / Cycle 001：Rework——U1/U2 harness 修复、上升沿武装与最终矩阵重跑

## Plan Context

- Status: ready
- Cycle Type: rework
- Parent Cycle: `000-initial.md`（Act Response `reported`，Plan Review `rework-required` 2026-10-14）

**Cycle Scope**

- Change tasks：T6（3.1）、T7（3.2）——修复以闭合原任务既有 Acceptance。
- Acceptance gaps：A1 尾相 post-recovery C4 探针；A3 四项判定行（reset-1 容量两级边界、插回上升沿 phy60-event、插回后新 SocketEpoch 会话交换、容量/C4 第 2 轮与 fail-stop）。
- Repair items：T6-R1、T7-R2、T7-R3、T7-R4。
- Inherited scope：000 Plan Context 的全部契约面（SocketOps 组合语义、注入接缝、矩阵编排、25 s 窗口、resume 细化、判别行观察面）继续有效；已板上直证的 000 场景不重做 bring-up，随重跑矩阵自然复验。
- Excluded scope：多 hart、长稳 soak、性能、用户态网络工具、第二 UART/设备；不修改 `NetRecoveryControl` trait；runner 自醒风暴 + 自旋锁饿死为**范围外 Issue 候选**（见父 Review Finding 5），本 Cycle 不修；脚手架装饰性清理延迟到收尾 Review（父 Review Finding 6）。

**Objective**

四个 repair item 闭合后：recovery_socket 全绿（尾相 C4 真结算）；`wait_lifecycle` 具备离开-再回护栏且"committed ⟹ 门开"成为调度无关不变量；source-60 双边沿武装使插回上升沿可达；一次完整真板矩阵会话取得 A3 全部判定行（含 001 移交残余与 fail-stop 末场景）。

**Background**

父 Cycle Act 上交 U1/U2/U3，本 Review 已全部根因定位（证据见父 Review Plan Review）：U1=host 尾相探针不驱动 fake TX 完成；U2=payload `wait_lifecycle` 陈旧采样（无 left-Active 护栏）+ 门清除次序加固；U3=pad35 仅武装下降沿，插回上升沿无中断，停摆为其落7。

**Investigation Facts**

- Current Baseline：HEAD `d07285c0` 未提交工作树（父 Cycle Act 全部改动在案）。验证基线：harness 37/37、k3-async-net-host 除 recovery_socket 尾相外全绿（lib 1、adapter 12、flush 1、recovery 8、service_owner 1、stack_socket 1）、axnet 三面 486/516/519（001 结论，本轮 R2 将改动 async_rx 后需重跑）、构建门 k3-async/k3-async-probe exit 0、真板会话 2026-10-14（reset-1/new-1/link-down/reset-2/四协议已直证；A3 四项缺口如上）。
- **U1 事实（host）**：恢复后新 epoch 会话分配票 18–21（UDP echo、SYN-ACK、echo、FIN），尾相 flush target=21=最后一帧 FIN，gdb 显示其 `DeviceOwned` 在途、`first_lost=None`、live_len=1。fake 的设备侧 TX 完成**只在** `drain_board_tx()` 发生（tests/k3-async-net-host/src/fake.rs:424-439：逐 OWN 位置零并回收帧）；尾相循环（tests/recovery_socket.rs:914-935）只 `owner.pump(8)`，无 drain、无 `publish_queue_event`，票 21 完成不可达，flush 判定链（crates/axnet/src/device/fixed_queue.rs:560-587 `flush_state`、crates/axnet/src/service.rs:1189-1243 `flush_recheck`）行为正确。同文件其他循环（`exchange()`，recovery_socket.rs:417-431）即 drain+publish+pump 范式。
- **U2 事实（payload+axnet）**：`wait_lifecycle`（kernel/src/platform/k3_async_net.rs:2175-2199）首次采样即接受 `lifecycle==want`；单 hart 协作下 `recovery_reset_request()` 返回后 owner 尚未 claim，首次采样必然读 Active(2) 陈旧值。host 对照循环（recovery_socket.rs:676-692）已有 left-Active 护栏。次序事实：`recovery_step` 在 guard 内 `tx_advance_epoch` 后提交 `lifecycle.recovery_committed()`（crates/axnet/src/async_rx.rs:2506-2537，commit 于 :2521），`set_recovery_hold(false)` 在调用方 guard 释放后（:2478）；K3 构建 axfeat `sched-rr`（kernel/Cargo.toml:115，axtask preempt + `on_timer_tick`→`scheduler_timer_tick`，crates/axtask/src/api.rs:161）使该 µs 窗口可抢占。矩阵调用序（k3_async_net.rs:2296-2332）：committed 行→waiters→flush 单次 poll→release→`run_capacity_flush_cycle`（:1823-1872 先 HOLD 再逐帧 enqueue，首拒即 break）。
- **U3 事实（platform）**：`pad35_edge_setup`（kernel/src/platform/phy_probe.rs:433-438）仅置 `PAD_EDGE_FALL`（:413），DTB `<35 2>` 注释在 :446-447；`PAD_EDGE_RISE` 已定义（:411，见 :417 mask）。handler `phy_gpio60_handler`（k3_async_net.rs:408-418）边沿无关：pad35_edge_ack + `axnet::publish_config_event()`。owner 的 Up 路径已交付（crates/axnet/src/service.rs:991-1010：Down 后 Up 开下一 SocketEpoch + 清 link 门；:966-967 状态去重吸收同值重读）。恢复后的 initial link 重读一次性消费 `initial_link_pending`（async_rx.rs:2526-2532），此后唯一再读触发是 CONFIG cause（即 source-60 事件）。矩阵 R3 段（k3_async_net.rs:2344-2403）：拔线→waiter NotConnected→reset-2→插回 phy60 计数等待→new-session-2→容量第 2 轮。
- **host RED 入口（R3）**：tests/phy-probe-host-harness.rs 建模 MFPR pad 区（:240-244 fake），运行入口 `rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test && /tmp/phy-probe-host-test --test-threads=1`（Makefile:97）。

**Implementation Guidance**

顺序：R1（host 快速闭合）→ R2（护栏+次序）→ R3（双边沿）→ 构建门 → R4（板测）。R1/R2/R3 相互独立、无共享文件冲突；R4 依赖三者全部完成。R2 的 axnet 改动是行为保持的次序重排，由既有 GREEN 套件保护（TDD 重构规则：先 GREEN 保持 GREEN）；R1 与 R3 的先行测试在各自 harness 内先观察 RED。

**Task Contracts**

### T6-R1：recovery_socket 尾相 C4 探针驱动设备半（闭合 A1）

- Requirement/Scenario：R7/R8 socket 面；S17 尾相。
- Depends on：None。
- Targets：`tests/k3-async-net-host/tests/recovery_socket.rs`（尾相循环 :914-935 及其前提注释）。
- Current behavior：尾相循环只 `owner.pump(8)`；票 21（新会话 FIN）的 fake TX 完成不可达；flush 20 s 有界失败 panic。
- Required behavior：循环每轮驱动设备半——`owner.board.wire.drain_board_tx()` + `publish_queue_event()`（同 `exchange()` 范式），使最后一帧完成、queue owner 回收、flush 经 `flush_progress` 唤醒后结算 `Ok`（真实 C4）；前提注释更正为"最后一帧回收后结算"（删去"nothing in flight"错误前提）。20 s 有界与计数器快照 panic 消息保留。
- Preserve：其余相（旧对象终态/flush abort/多 waiter/隔离/新对象 I/O）零改动；fake.rs 不改（其 TX 完成语义正确）；产品 axnet 不改。
- Forbidden：为驱动循环新增判定层/专用 fixture API；宣称真实 timer/DMA（宿主时钟 stub 不变）。
- First test：现有失败即 RED（本 Review 2026-10-14 已复现，panic 快照在案）；修复后同命令 GREEN。
- GREEN condition：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline --test recovery_socket` 全过；随后全包一次全绿。
- Verification：上述命令；轮末全包一次（Iteration 预算内）。
- Stop when：驱动循环后 flush 仍不结算（说明存在真实产品缺陷）——停，回 Plan。

### T7-R2：wait_lifecycle 离开-再回护栏 + 恢复门清除次序加固（闭合 A3 容量轮前提）

- Requirement/Scenario：R1/R2；S01/S02（committed ⟹ 门开）。
- Depends on：None。
- Targets：`kernel/src/platform/k3_async_net.rs::wait_lifecycle`（:2175-2199）；`crates/axnet/src/async_rx.rs::recovery_step`（:2506-2537）与 `poll_recovery` Finished 分支（:2472-2480）。
- Current behavior：wait_lifecycle 首采即接受 Active(2) 陈旧值，整列后续操作在恢复进行中执行（板上容量首帧被拒的确定性根因）；门清除（:2478）晚于 lifecycle 提交（:2521）一个 guard 释放，`sched-rr` 抢占下存在 µs 观察窗口。
- Required behavior：① wait_lifecycle 记录是否已观察到 `lifecycle != want`（`left` 见证），仅在其后接受 `== want` 返回；fail-stop 的 want=3 不受影响（初值 2 即置 left）。② async_rx：在 `recovery_step` Recovered 分支内、`tx_advance_epoch_target` 之后 `lifecycle.recovery_committed()` 之前，于同一 Service guard 下调用 `service.tx_set_recovery_hold_target(false)`；删除 Finished 分支中 guard 释放后的 `self.set_recovery_hold(false)`（:2478）。"lifecycle==2 ⟹ 门开"成为调度无关不变量。
- Preserve：恢复阶段机、各绝对 deadline、flush 结算次序（close→advance→commit 的既有 F5 序）、quiesce/drift 路径的置位语义全部不变；`set_recovery_hold` 方法保留（enter_recovery/drift 仍在用）。
- Forbidden：新增公共 API；调度器/锁改动；改变 lifecycle 编码或等待窗口长度。
- First test：①为 payload 代码，RED/GREEN 由 R4 板测承载（reset-1 容量轮 i=0 必须 accepted、81 帧两级边界复现）；②为行为保持次序重排——先运行 axnet 三面 + host 全包确认 GREEN，改动后保持 GREEN（TDD 重构路径）。
- GREEN condition：axnet 三面 `--lib`（默认/queue-diagnostics/qemu-diagnostics）全绿；k3-async-net-host 全包全绿；harness 37/37；板测 R4 见证容量轮第 1 帧接受。
- Verification：axnet 三面命令（同父 Cycle 清单）+ host 全包 + harness；板测归 R4。
- Stop when：次序重排导致任一既有套件失败（说明非行为保持）——停，回 Plan。

### T7-R3：source-60 双边沿武装（闭合 001 移交残余的可达性）

- Requirement/Scenario：R3；S06/S08（完整 down→up 与插回后新会话）。
- Depends on：None。
- Targets：`kernel/src/platform/phy_probe.rs::pad35_edge_setup`（:433-438 边沿位写入；`PadEdgeSetupEvidence` 读回随寄存器真值）。
- Current behavior：仅 `PAD_EDGE_FALL`（DTB `<35 2>` 用法）；插回上升沿无中断，owner 无触发再读 link，link 门与 SocketEpoch 持续关闭。
- Required behavior：武装位改为 `PAD_EDGE_RISE | PAD_EDGE_FALL`（CLEAR hold 释放与其余位保留语义不变）；handler、INER、owner link 步骤零改动（Up 路径 service.rs:991-1010 已交付；状态去重 :966-967 吸收每 flap 双沿）。DTB 注释同步更正为双边沿运行时武装的事实陈述。
- Preserve：MS10 板裁决事实（EIE 掩码窗口、hold-in-clear、INSR 回声分类）不受影响；arm 证据行格式不变（读回值随新武装位变化）；MDIO/锁/控制台不进 IRQ handler（现状保持）。
- Forbidden：轮询 fallback（空闲不轮询不变量）；新控制协议/专用 CLI 参数；修改 BMSR/MDIO 状态机。
- First test：`tests/phy-probe-host-harness.rs` 先 RED：`pad35_edge_setup` 的 readback 证据须含 RISE|FALL 双位（现读回缺 RISE）；再 GREEN。命令：`rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test && /tmp/phy-probe-host-test --test-threads=1`。
- GREEN condition：该 harness 全绿；axnet link 既有套件（service_owner/recovery_contract 的 down→up 面）不回归（无 axnet 改动，采信+定向确认）；板测 R4：插回 25 s 窗内 phy60 计数增长、link up commit、new-session-2 交换成立。
- Verification：harness 命令 + 受影响 host 面；板测归 R4。
- Stop when：板上上升沿仍不可达（读回与 INSR/MFPR 证据显示 pad/PHY INTN 脉冲形态与预期不符，如 INTN 为保持电平）——停，回 Plan 并附 arm 行与 phy60 证据。

### T7-R4：真板最终矩阵重跑（A3 收口）

- Requirement/Scenario：R1–R8 板上面；S01–S12/S15/S18/S20。
- Depends on：T6-R1、T7-R2、T7-R3 全部 GREEN。
- Targets：无代码改动；`run_recovery_matrix`（k3_async_net.rs:2231-2430）按现状执行；构建门普通/probe 各一次（probe 最后）。
- Current behavior：000 板测中途受阻（U3 停摆在 new-session-2，容量第 2 轮与 fail-stop 未取得）。
- Required behavior：一次单 boot 完整会话取得全部判定行：重建基线→R1 旧会话→waiter→2 s hold+flush anchor→reset-1（committed、waiter connection-reset ×2、flush abort、release、**容量第 1 轮 81 帧两级边界（i=0 必须接受）**、new-1）→拔线（waiter not-connected + phy60）→reset-2→**插回（25 s 窗内 phy60-event、link up、new-session-2 ping/UDP/TCP 交换——001 移交残余）**→容量/C4 第 2 轮→fail-stop 末场景（confirm-withhold→`ResetTimeout{raw:…}` 诚实现值→停止未确认→enqueue-after 拒绝→backing=retained）→操作者外部 RESET 回官方系统一句话。lifecycle/owner/fault/queue-gen 账本守恒断言与 000 相同。
- Preserve：RAM boot 流程、无持久写入；普通镜像零行为差异（注入面 cfg 剔除）；原生输出直判、无 validator。
- Forbidden：注入改 MMIO 原值/伪造 SWR 成功；失败重试到 PASS；场景跨 boot 拆分（单场景须单 boot 内完整）。
- First test：R2/R3 的板面 RED 已由 000 板测（假 committed 首帧被拒、phy60-event timeout）构成；重跑即 GREEN 判定。
- GREEN condition：矩阵全场景判定行取得且无一否定已 accepted 底层；操作者一句话外部 RESET 见证。
- Verification（操作者）：RAM boot Runbook 流程 + Act 在接线后写入的最短步骤；Act 依据串口与 PC 原文审核。
- Stop when：任一场景否定已 accepted 底层语义（stop/rebuild、epoch、link）——立即停交 Plan。

**Invariants**

- 唯一 queue owner 驱动恢复与矩阵；IRQ 只发布 cause；软件标签不证明硬件状态。
- QueueEpoch 只在 begin 边界推进；socket terminal 先于 wake；flush abort 先于 epoch 推进。
- 恢复各阶段绝对 deadline 不变（R6 语义）；fail-stop 注入只在诊断构建存在。
- 板测经 RAM boot、无持久写入；外部 RESET 不算 runtime reset 验收。
- 判定只用原生输出/退出码/直接观察；无新增判定层或身份机制。

**Non-goals**

与父 Cycle 相同（多 hart、长稳、性能、用户态网络工具）；另明确：runner 自醒风暴 + 自旋锁饿死（范围外 Issue 候选）与脚手架装饰清理（延迟收尾 Review）不在本 Cycle 处理。

**Acceptance**

- A1-R（T6-R1）：recovery_socket 全绿，尾相 C4 真结算。
- A2-R（T7-R2/R3 agent 侧）：axnet 三面、host 全包、phy-probe harness、k3-gmac harness 全绿；构建门普通/probe 各一次通过。
- A3-R（T7-R4）：完整真板矩阵取得 A3 全部判定行，含 001 移交残余（上升沿+插回新会话）、容量第 1 轮边界、第 2 轮容量/C4 与 fail-stop；操作者执行、Act 审核 PASS 后闭合。
- A4：收尾清单裁决维持父 Review Finding 6 的延迟处置；最终 diff 自检后仅填写 Act Response。

**Verification**

host 命令（在所属 repair item 使用；已通过且覆盖未变化时直接采信）：

```sh
cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline
rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test
/tmp/phy-probe-host-test --test-threads=1
rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host
/tmp/k3-gmac-recovery-host --test-threads=1
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features queue-diagnostics
RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics
```

昂贵预算：本轮末 host 全包一次、axnet 三面一次（R2 改动后必跑）、真板矩阵会话一次、构建门各一次（probe 最后）。QEMU 维持用户永久豁免，不列入。

**Gate 2 Readiness**

技术自检 PASS：调查完成（三项根因均有直接证据，代码位置到行）、契约自包含、无 TBD、RTM 无 Missing/Simplified、BLOCKED: None、WAIVED: None。**Gate 2 PASS：2026-10-14 用户原话“批准直接实施，翻转状态，真板操作交由我手动运行”——Status 已转 `ready`，Act 执行 T6-R1→T7-R2→T7-R3→构建门，T7-R4 板测停在能力边界交操作者。**

**Persisted Evidence**

- Mode: none。命令、退出码、判定行与板测一句话结果入 Act Response；安全 RAM 板测可低成本重跑。

**Risks and Notes**

- 双边沿武装后每 flap 可产生两条 claim（INTN assert+deassert）：owner 状态去重吸收，矩阵 phy60-event 行显示 events 计数增长即为达成判据（不要求恰为 1）。
- 若 RTL8211F INTN 为保持电平（非脉冲），上升沿可能不可达——R3 的 Stop when 覆盖；arm 读回行与 000 板测 phy60 证据是排查入口。
- R2 次序重排触碰 F5 结算序附近代码：任何既有套件失败即停，不解释性地继续。
- 矩阵单会话时长受操作者节奏限制：R3/R4 场景各须单 boot 完整（窗 25 s 不变）。

## Act Response

- Status: reported
- Date: 2026-10-14
- Gate 2 批准：用户原话"批准直接实施，翻转状态，真板操作交由我手动运行"。
- **收口裁决：用户原话"算了，这些问题都不重要，取消相关调试语句和更改，我们直接填写回复，本轮change就做这么多"**——诊断插桩全部撤销（保留交付修复），真板矩阵的残余缺口按用户裁决止步本轮，如实记录如下，交 Plan Review 裁决。

### 实际改动（最终保留态）

1. **T6-R1**：`tests/k3-async-net-host/tests/recovery_socket.rs` 尾相循环每轮 `drain_board_tx()` + `publish_queue_event()` + `pump(8)`（同 `exchange()` 范式），闭会话最后一帧回收后 flush 真结算；20 s 有界与 panic 快照保留。
2. **T7-R2①**：`kernel/src/platform/k3_async_net.rs::wait_lifecycle` left-Active 护栏（观察到 `!= want` 后才接受 `== want`；fail-stop want=3 不受影响）。
3. **T7-R2②**：`crates/axnet/src/async_rx.rs::recovery_step` 在 `tx_advance_epoch_target` 后、`lifecycle.recovery_committed()` 前同 guard 清恢复门；删除 `poll_recovery` Finished 分支 guard 外的旧清除点。"lifecycle==2 ⟹ 门开"调度无关。
4. **T7-R3**：`kernel/src/platform/phy_probe.rs::pad35_edge_setup` 武装 `RISE|FALL` + `ok()` 谓词同步 + DTB 注释更正（板上证实写入幂等——pad 本就双沿，寄存器契约自此明确双沿事实）。
5. **Blocker 1 修复（裁决 (a)）**：`recovery_socket.rs` 尾相前取 `active_tail` baseline，最终断言改 `fallback_poll == active_tail.fallback_poll`（Active 尾相零回退；恢复窗口 runner 安全网回退计入 baseline）。
6. **Blocker 2 修复（裁决 (a)，INTS/INTN ack）**：`arm_phy_source60` 在 INER 使能后一次 `paged_read(INSR)` 清 boot 期 latch（arm 行新增 `insr=` 字段）；`k3_gmac/adapter.rs` 新增 `consume_link_cause`（有界 3 轮 INSR 清读，latched 则双读 BMSR 重采样至 0；paged 失败报 `Io`），`read_link_status` Done 后调用——owner 上下文 MDIO，ISR 保持无 MDIO（MS10 裁决）；axnet 与 trait 零改动。先行测试 `link_read_consumes_the_latched_insr_cause` RED→GREEN。

### 已撤销（收口裁决）

判别插桩全部移除并恢复原实现：`spawn_session_traced`/echo 会话 spawn 改造/`wd` 看门狗/`phy-truth` 行/模块级原子导入。撤销后 probe 镜像重建 `REAL_MAKE_EXIT=0`（FIT 837152 B）。插桩期间的过程发现（插桩构建首版缺导入被管道化退出码误报成功等）留档于 git 历史与本 Response 的教训记录，不再保留代码。

### 与计划的偏差

1. 两项 Blocker（fallback 断言语义、source-60 静默）属 Plan 遗漏/归因错误，经用户裁决在当前 Cycle 内修复。
2. axnet `--lib` 三面 linker 用绝对路径（相对路径冷 target 必报 `linker not found`；MS12 先例）。
3. host 包沿用编译期 `AX_IP=10.0.2.15 AX_GW=10.0.2.2` 约定。

### 验证命令与实际结果（2026-10-14 实测）

- host 全包（`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`，AX_IP/AX_GW 编译期设置）：lib 1 + adapter 12 + flush 1 + recovery_contract **9**（含 INSR 消费新测试 RED→GREEN）+ **recovery_socket 1（首次全过）** + service_owner 1 + stack_socket 1 = **全绿**。
- phy-probe harness **32/32**（双沿测试 RED→GREEN）；k3-gmac harness **37/37**（覆盖面变化后重跑）。
- axnet 三面 **486 / 516 / 519** 全绿（R2 次序重排行为保持）。
- 构建门：`make k3-async` exit 0、`make k3-async-probe`（最后）exit 0（真实退出码直接捕获；插桩期间的管道化误报教训见下）。

### 真板会话三轮汇总（2026-10-14，操作者执行，Act 依串口原文审核）

**已直证（板上取得）：**
- **U2 闭合**：reset-1 后容量第 1 轮 `i=0 accepted=true`、81 帧两级边界（i=80 rejected）、release→restored→`flush c4 ok`、`auto-release=0`（第 1/2/3 轮一致复现）。
- **R1/R2 全链**：old-session、reset-1（committed lifecycle=2 owner=1 + 双 waiter `connection-reset` + `flush aborted err=BadState`）、new-1 会话、reset-2（link down 交错）committed fault=0——三轮一致无回归。
- **INTS/INTN ack 修复板上成立**：拔线 `link-down waiter=not-connected` + `phy60=events set=0xa470 rearm=0xa430 aia-claim=0x3c003c`（source-60 下降沿端到端）；**插回 25 s 窗 `phy60-event observed events=3→5`**（上升沿首次可达，001 移交残余的证据项取得）；插回后 new-2 UDP/TCP 会话真实交换成功（操作者 PC 原文逐字节回显）。

**未取得（用户裁决止步本轮）：**
- 容量/C4 第 2 轮、fail-stop 末场景（confirm-withhold→停止未确认→enqueue 拒绝→backing=retained）未走到。
- **插回后 link 不稳定（未定位，留档）**：第三轮带判别 trace 的会话显示——插回后 new-2 窗口内先出现"RX 活（ri/work 每秒攀升）而 TX 冻结（ti 不动、rx-slot 积压至 5）"的门关闭形态，继而**全部计数器冻结**（ri/irq-runs/aia-traps/occupancy 全停）而 owner 始终 `lifecycle=2 fault=0` 无感知，`phy60 events` 冻结 5 不再增长，网络沉寂直到外部 RESET；期间操作者 UDP/TCP 各成功一次后再次超时。trace 同时证明 boot 任务全程存活、会话有界——饥饿/死锁（Finding 5）与 `consume` 锁嫌疑排除。精化三候选（未验证）：① INTN 电平型事件通路"事件→读→消费"单次失手即永久哑掉（无兜底读取）；② 末次 link 判读为 Down 后门永久关闭（无事件不重读）；③ GMAC DMA 静默停摆（owner 无感）。诊断插桩按裁决撤销，三候选留档给后继工作。
- 插回新会话交换的"矩阵判定行"形态未取得（操作者手动会话已证明交换能力，但矩阵自身的 new-2 判定行因 link 窗口错过未打印）。

### 自检与教训

- `set_recovery_hold` 保留后仅剩 `enter_recovery` 调用点，无死代码；gate-clear 与 commit 同 guard 后无"提交 Active 而门关"路径。
- 插桩构建曾因 `k3_net_diagnostics` 模块缺导入失败、且被 Act 的管道化退出码读取（`exit=$?` 取到 grep 退出码）误报成功——操作者实跑暴露后更正；本轮两次踩同一管道坑，教训：验证退出码必须直接捕获（`cmd > log; echo $?`），管道只作展示。登记 Runbook 候选。
- k3-gmac harness（k3_dma/mac_probe/phy_probe/k3_gmac mod 编译面）与 phy_probe、axnet 在最终态改动核验为零重叠或已重跑：37/37、32/32、486/516/519 采信有效。

### Persisted Evidence

None required（Mode: none；命令、退出码、判定行已入本 Response）。

### Experience Candidates

- Runbook 候选：axnet host 测试 no-PIE 链接需绝对路径 linker（相对路径冷 target 必报 `linker not found`）；MS12 与本轮两次端到端验证。
- Runbook 候选：RTL8211F INTN 电平保持语义与 INSR 读清除应答设计（handler 无 MDIO → owner link 读路径消费 latch；arm 时清 pending）——本轮板上端到端验证成立（拔线/插回事件恢复 + 会话交换成功）。
- Runbook 候选：构建/验证退出码禁止经管道读取（`cmd | grep` 后 `exit=$?` 取到的是过滤器退出码）；本轮两次实错。
- Issue 候选（交 Recorder 按用户指令处理）：插回后 link 不稳定的三候选机制（事件通路单次失败永久哑 / 末次 Down 门永久关 / DMA 静默停摆）——有第三轮 trace 证据，未定位；范围外 runner 自醒风暴+自旋锁饿死沿用父 Review Finding 5。

### 未解决问题（交 Plan Review 裁决）

- A3-R 残余：容量/C4 第 2 轮、fail-stop 末场景、矩阵形态的 new-2 判定行——用户裁决本轮止步；是否以"插回后真实会话交换成功 + 三轮已取得判定行"折价收口，由 Review 决定。
- 插回后 link 不稳定三候选未定位（用户裁决不追）。
- A4 收尾清单裁决维持父 Review Finding 6 延迟处置。

## Plan Review

- Review Result: accepted
- Date: 2026-10-14

**Findings**

1. **代码独立核对通过**。六个改动面逐项与 Act 声明一致：①尾相循环每轮 `drain_board_tx()+publish_queue_event()+pump`、前提注释更正为"闭会话尾帧先回收再结算"（recovery_socket.rs 尾相段）；②`wait_lifecycle` left-Active 护栏（先见证 `!= want` 再接受 `== want`，fail-stop 语义不变）；③`recovery_step` 在 `tx_advance_epoch_target` 后、`recovery_committed()` 前同 guard 清恢复门（async_rx.rs:2525 附近），`poll_recovery` Finished 分支旧清除点已删，`set_recovery_hold` 仍被 `enter_recovery`/drift 使用无死代码；④`pad35_edge_setup` 武装 `RISE|FALL`、`ok()` 谓词同步、DTB 注释如实陈述运行时超集武装；⑤`active_tail` 基线断言（恢复窗回退计入基线、Active 稳态零回退）语义与用户裁决 (a) 一致；⑥`arm_phy_source60` arm 行新增 `insr=` 清 boot latch、adapter `consume_link_cause` 有界 3 轮 INSR 清读 + latch-low 双读 BMSR、`read_link_status` Done 后调用，owner 上下文 MDIO、ISR 无 MDIO 不变量保持（MS10 裁决），"边界本身不丢事件"的论证与实现一致。诊断插桩（`spawn_session_traced`/看门狗/phy-truth）零残留。
2. **验证采信**。host 全包（recovery_socket 首次全过、recovery_contract 9 含 INSR 新测试）、phy-probe harness 32/32（双沿 RED→GREEN）、k3-gmac harness 37/37、axnet 三面 486/516/519（R2 后重跑，行为保持证实）、构建门 exit 0（退出码直捕）。覆盖面与代码一致，无矛盾触发重跑；recovery_socket 的失败态我在父 Review 已独立复现，本轮 GREEN 与修复代码一致。
3. **两项过程 Blocker 的偏差分类**：Blocker 1（fallback 断言基线语义）与 Blocker 2（RTL8211F INTN 为电平型、INSR 读清除需 owner 侧消费）均为 **Plan 遗漏/新证据**——两事实在 000/rework 计划期未知。修复经用户裁决 (a) 在当前 Cycle 内完成；Blocker 2 触及 `adapter.rs`（T5 已交付面，超出本 rework Targets），属用户授权的范围内扩展，已记录且有 RED→GREEN 与板测直证。非阻塞。
4. **A3-R 折价收口裁决（本 Review 决定）**：接受"插回后真实会话交换成功 + 三轮已取得判定行"折价。依据：①U2 闭合三轮一致（i=0 accepted、81 帧两级边界、flush c4 ok、auto-release=0）；②上升沿首次可达（events 3→5）+ 插回后 new-2 UDP/TCP 真实逐字节交换（操作者 PC 原文审核）——001 移交残余的**实质**（插回后新 SocketEpoch 会话交换）已板上取得，缺的只是矩阵行形态；③残余三项（容量/C4 第 2 轮、fail-stop 末场景、矩阵行形态）按用户收口原话止步本轮，风险如 Acceptance Gaps 记录。
5. **插回后 link 不稳定（三候选，未定位）**：第三轮 trace 证明 boot 任务存活、会话有界（饥饿/死锁与 `consume` 锁嫌疑排除），但计数器全冻结而 owner `lifecycle=2 fault=0` 无感——候选①INTN 电平"事件→读→消费"单次失手永久哑、②末次 Down 判读后无事件不重读、③GMAC DMA 静默停摆。诊断插桩已按用户裁决撤销，trace 证据留档于本 Response。登记为 Issue 候选（交 Recorder 按用户指令处理）；与父 Review Finding 5 的 runner 风暴/饿死候选并列。

**Acceptance Gaps**

- 用户豁免项（原话在 Act Response 收口裁决，风险在此记录）：容量/C4 第 2 轮（连续两轮成功恢复后的容量/C4 板上判定行）未取得——第 1 轮已三轮一致，重复恢复后的容量/C4 无板上结论；fail-stop 末场景（confirm-withhold→停止未确认→enqueue 拒绝→backing retained 的板上判定行）未取得——host 注入面 37/37 GREEN，R5/S11/S12 的板上 fail-stop 与 DMA 停止证明**不在本 change 结论内**；矩阵行形态的 new-2 判定行未打印（实质已由手动会话见证）。
- 插回后 link 不稳定未定位（Finding 5）。
- 上述缺口随 change 收尾入档，不得在规格或 SNAPSHOT 中表述为已板上验证。

**Evidence**

- 代码位置：recovery_socket.rs 尾相段与 `active_tail` 断言、k3_async_net.rs `wait_lifecycle`/`arm_phy_source60`、async_rx.rs `recovery_step` Recovered 分支、phy_probe.rs `pad35_edge_setup`/`ok()`、adapter.rs `consume_link_cause`(:72-86)/`read_link_status`(:512-)、recovery_contract.rs:477 新测试。
- 验证结果：Act Response 验证命令与实际结果段（2026-10-14 实测）；recovery_socket 失败态独立复现见父 Cycle Plan Review Evidence。
- 板测三轮串口与 PC 原文：Act Response 真板会话段（操作者执行，Act 审核原话在案）。

**Follow-up Decision**

A1-R/A2-R 全量满足；A3-R 的残余按用户收口原话豁免且实质见证成立（Finding 4）；A4 维持延迟处置。两项过程 Blocker 经用户裁决修复并验证。无阻塞 Acceptance 的发现，无需后续 Cycle——接受，Iteration 002 与本 change 的执行面完成。收尾（规格合并、tasks/SNAPSHOT 同步、归档）按阶段边界由用户决定后交 `openspec-docs-maintainer`。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

None（002 为 Map 中最后一个 Iteration；change 待收尾）。

## Plan Review

- Review Result: accepted
- Date: 2026-10-14
- Acceptance authority：用户审计接受，原话"工作被接收，归档收尾，然后提交保存当前工作"。
- 裁决范围：本 rework Cycle 全部交付（T6-R1、T7-R2、T7-R3、Blocker 1 baseline 断言、Blocker 2 INSR/INTN ack）连同三轮真板会话证据整体接收；A3-R 残余（容量/C4 第 2 轮、fail-stop 末场景、矩阵形态 new-2 判定行）与插回后 link 不稳定三候选按用户收口裁决（"本轮change就做这么多"）留档不追，不计入本 change 完成判定的阻塞项；后继工作（link 不稳定定位、fail-stop 场景）可由未来 change/Issue 承接。
- Evidence：Persisted Evidence `none`（三轮判定行与 PC 原文已录入 Act Response）。
- Next Cycle: None
- Next Iteration: None
