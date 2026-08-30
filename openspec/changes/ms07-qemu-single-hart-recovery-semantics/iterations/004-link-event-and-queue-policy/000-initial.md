# Iteration 004 / Cycle 000: Link Event and Queue Policy

## Plan Context

- Status: ready
- Iteration: 004-link-event-and-queue-policy
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 3.1
- Depends on: Iteration 003 accepted
- Stable baseline: used-ring 与 config-change cause 独立到达唯一 queue owner；一致 link snapshot 驱动 link-down/up 的 queue 与 SocketEpoch 边界，不推进 QueueEpoch。
- Verification boundary: IRQ cause、config generation retry、link policy matrix、axnet focused/full、MS03/MS04 host harness 与 kernel build 通过。
- Diagnostic boundary: VirtIO ISR cause publication、QueueEvent flags/recheck、一致 link snapshot、Service enqueue/submit gate 与 SocketEpoch transition seam。
- Deferred tasks: 3.2、4.1、4.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R3、R6、D1、D6；Iteration 003 接受的唯一 resident owner、QueueEpoch、recovery hold、ticket ledger、guard 外 wake 与 bounded poll。
- Excluded scope: socket handle terminal 存储与错误映射、公开 V4/QEMU probe、真实 HMP runtime、透明 TCP 恢复、SMP、PCI/DWMAC runtime、真板与性能。

**Objective**

让 VirtIO config-change IRQ 在 ACK 后独立发布给唯一 queue owner。owner 每 poll 至多进行一次一致 link snapshot 尝试：link down 关闭当前 SocketEpoch 边界、取消 pre-submit 并阻止 enqueue/submit，但继续回收 DeviceOwned；link up 推进 SocketEpoch 并恢复新会话入口。整个过程不得推进 QueueEpoch、伪造 completion、在 ISR 读取 config/descriptor，或新增第二 task。

**Background**

Iteration 000 已提供 config-generation guarded link snapshot，Iteration 003 已接受 resident recovery 与 queue gate。当前 ISR 能分类并 ACK used/config cause，但只为 used-ring 调用 `axnet::publish_queue_event()`；config-only 不唤醒 task。axnet 只有共享 generation/waker，没有独立 cause flags 或 link policy。Task 3.1 需要把现有 driver link accessor 接到 task context，并为 Iteration 005 的 epoch-scoped socket terminal 提供稳定的 SocketEpoch transition seam。

**Current Baseline**

- Revision：`596b324b6e7cb78b3a4308b997657b6d0c95d44a`；Iteration 003 产品与测试改动仍在工作树。
- `virtio_net_irq_logic` 已正确分类 used-only、config-only、combined、unknown 与 zero，并以 `ack_mask(status) == status & 0x03` 只 ACK 已知位。
- `net_irq_handler` 严格执行 telemetry → ACK → used-ring publish；`should_publish_rx` 只检查 bit 0，因此 config-only 不发布，combined 也只产生 generic queue event。
- `QueueEvent` 使用 Release generation + queue waker 的 register/recheck 协议，但没有 cause flags；`publish_queue_event` 也无法区分 used/config。
- `NetRecoveryControl::read_link_status`、VirtIO adapter 与 raw transport 已实现一致 snapshot；generation race 返回 `DevError::Again`，但 Service/Router/owner 尚无 forwarding 或调用点。
- Ethernet TX 的 `recovery_hold` 可阻止 enqueue/submit；当前没有 link-specific state、LinkGeneration 或 SocketEpoch transition seam。
- 新鲜基线：MS03 host harness 33/33、MS04 host harness 16/16、axdriver_virtio link tests 2/2、virtio-drivers link test 1/1，均 exit 0。

**Current-State Evidence**

1. `kernel/src/drivers/virtio_net_irq.rs::net_irq_handler` 在 ACK 后只在 `should_publish_rx(status)` 为真时调用 `axnet::publish_queue_event()`；config-only 明确不 publish。
2. `kernel/src/drivers/virtio_net_irq_logic.rs` 已保留 raw status 的 used/config/combined 信息，适合新增纯逻辑 publication decision；ISR 不需要读取 VirtIO config。
3. `crates/axnet/src/async_rx.rs::QueueEvent` 的 generation/waker 已闭合 event-before/during-register；cause flags 应复用同一同步协议并由 owner 消费，不能另建 waker 或 task。
4. `crates/axdriver_virtio/src/net.rs::read_link_status` 转发到 `VirtIONetRaw::read_link_status`；后者以 config-generation before/status/after 单次读取保证一致性，竞态映射 `Again`。
5. `Service` 已在 guard 下转发 recovery、epoch、cancel、submit/reclaim 与 gate 操作，但没有 `read_link_status_target` 或 link policy commit。
6. `EthernetDevice::send`/submit 已受 recovery hold 控制；Task 3.1 必须区分 link gate 与 reset recovery lifecycle，link up 不得清除仍有效的 recovery hold。
7. 当前 SocketSet readiness 仍是 boot-global terminal；handle epoch/NotConnected 映射属于 Iteration 005。本轮只建立 checked SocketEpoch 状态和关闭/开放 transition seam，不提前改 public handle 语义。

**Relevant Code**

- `kernel/src/drivers/virtio_net_irq.rs::net_irq_handler`：MMIO status、ACK 与 ISR-safe publish。
- `kernel/src/drivers/virtio_net_irq_logic.rs::{classify_mmio_status,ack_mask,should_publish_rx}`：host-testable cause 逻辑。
- `tests/ms03-irq-host-harness.rs`、`tests/ms04-async-rx-host-harness.rs`：cause、ABI、ISR 禁止项与 record→ACK→publish source guard。
- `crates/axnet/src/async_rx.rs::{QueueEvent,publish_queue_event,RxRxFuture}`：event flags/recheck 与唯一 owner。
- `crates/axnet/src/{service.rs,router.rs}`、`device/{mod.rs,ethernet.rs}`：link snapshot forwarding、pre-submit cancel、enqueue/submit gate 与 epoch state。
- `crates/axdriver_net/src/lib.rs::NetRecoveryControl::read_link_status`、`crates/axdriver_virtio/src/net.rs`、`crates/virtio-drivers/src/device/net/dev_raw.rs`：transport-neutral 一致 link snapshot。

**Critical Path**

```text
VirtIO MMIO IRQ status
  telemetry -> ACK known used/config bits
  used bit   -> publish USED cause
  config bit -> publish CONFIG cause
  combined   -> atomically retain both causes, one wake is sufficient
queue owner register/recheck
  atomically take bounded cause flags under the existing generation protocol
  used: run normal RX/TX completion path
  config: one read_link_status attempt
    Again -> retain/re-publish CONFIG + self-wake
    down transition -> close current SocketEpoch seam; gate enqueue/submit;
                       cancel Queued/ARP pending; keep reclaiming DeviceOwned
    up transition   -> checked SocketEpoch advance/open; recheck queue/stack;
                       QueueEpoch unchanged; no reset
```

**Implementation Guidance**

先扩展纯逻辑 cause publication matrix和 host guards，再给 `QueueEvent` 增加有界 cause flags与 take/recheck tests。随后补 Service/Router 的 link snapshot和 policy commit，最后将 owner 每 poll 的 config micro-step接入现有 bounded round。link gate必须与 recovery hold组合，不能由 link up 覆盖正在恢复或 Faulted 的 reset gate。

**Behavioral Change**

- config-only 与 combined IRQ 在 ACK 后唤醒 task context；used/config cause 均不丢失且不互相替代。
- task context 读取一致 link snapshot；`Again` 只触发下一 bounded poll，不在当前 poll 自旋。
- link down 拒绝新 enqueue/submit并取消 pre-submit；DeviceOwned completion/reclaim仍服务资源闭合。
- link up只开放新 SocketEpoch入口，不推进 QueueEpoch、不自动 reset、不复活旧 socket。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 3.1 | R6 config-only/combined | `virtio_net_irq_logic.rs`、`net_irq_handler` | classify/ACK + used publish | 独立 used/config publication decision与 ACK 后双 cause publish |
| 3.1 | R1/R6 event recheck | `async_rx.rs::QueueEvent/RxRxFuture` | generic generation/waker | bounded cause flags、take/recheck与 config micro-step |
| 3.1 | R3/R6 link policy | `service.rs`、`router.rs`、`device/ethernet.rs` | recovery gate与ticket owners | consistent snapshot forwarding、link gate、pre-submit cancel、epoch seam |
| 3.1 | Compatibility | MS03/MS04 harness、axnet/driver tests、kernel build | existing IRQ/queue contracts | config-only、combined、Again、down/up与 no-reset matrix |

**Task Contracts**

### 3.1: Config IRQ and link policy

- Requirement/Scenario: R3 link-down layered cancellation；R6 config-only、combined、down/up；D1、D6。
- Depends on: Iteration 003 accepted 的 resident owner、QueueEpoch ledger、recovery hold 与 guard 外 wake。
- Targets: `kernel/src/drivers/virtio_net_irq{,_logic}.rs`、`tests/ms03-irq-host-harness.rs`、`tests/ms04-async-rx-host-harness.rs`、`crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device/{mod.rs,ethernet.rs}`；下层 link accessor 只在 RED 证明契约缺口时修改。
- Current behavior: ISR 记录/ACK config cause但只为 used bit publish；owner无 config cause、link state或epoch seam；enqueue/submit只受 recovery hold控制。
- Required behavior: used/config cause独立保留并复用同一 waker/recheck；config在task context单次读一致snapshot。down关闭当前SocketEpoch seam、取消Queued/ARP pending并阻止新enqueue/submit但继续reclaim；up checked推进/open SocketEpoch且不改QueueEpoch。
- Required changes: 建立 cause flag publication/take；补 link snapshot forwarding和复合 gate；建立 checked LinkGeneration/SocketEpoch transition state；接入 bounded owner micro-step与queue/stack wake。
- Preserve: ISR只做status/ACK/publish；used-ring completion ledger、唯一owner、register-recheck、EVENT_IDX、V1–V3 ABI、Iteration 003 recovery gate/deadline/backing与guard外wake。
- Forbidden: ISR读config/Service/descriptor；config伪造completion；一个poll内循环读config；link flap触发reset或推进QueueEpoch；link up清除有效recovery/fault gate；提前实现per-handle terminal/NotConnected映射。
- Test witness: 先写 RED 覆盖 config-only publish、combined双cause、event-before/during-register、generation `Again` retained+self-wake、down/up matrix、link/recovery gate组合、pre-submit exactly-once、DeviceOwned继续reclaim、QueueEpoch不变与SocketEpoch checked overflow。
- GREEN condition: cause无丢失/替代；每poll至多一次link read；down/up policy与D6一致；既有IRQ/queue/recovery/quiet-path tests不退化。
- Verification: MS03/MS04 host harness；axnet focused和ordinary/qemu-diagnostics串行全量；axdriver_virtio/virtio-drivers link focused；kernel QEMU feature build；manifest-scoped rustfmt、`git diff --check`、full diff Review、strict OpenSpec validation，全部exit 0。
- Stop when: ISR安全发布需要Service锁/descriptor访问，link gate无法与recovery gate组合，SocketEpoch boundary必须依赖Iteration 005 handle registry才能表达，或V1–V3 ABI必须改变；返回Plan。

**Invariants**

- ISR不读取config、descriptor、Service或smoltcp，只在ACK后发布cause。
- used/config cause可合并wake但不可互相覆盖；事件发生在register前/中均由generation recheck捕获。
- link state、QueueEpoch、SocketEpoch与wake generation是不同identity；任一checked epoch耗尽fail-stop。
- link down只阻止新owner产生；DeviceOwned仍由原QueueEpoch completion/reclaim闭合。
- link up不reset queue、不复活旧socket、不覆盖recovery/fault gate。

**Non-goals**

- 不实现Iteration 005的per-handle epoch terminal、`NotConnected` I/O映射、listener/deferred cleanup。
- 不新增公开snapshot/ioctl、probe/validator或真实QEMU HMP资格。
- 不证明SMP、PCI/DWMAC runtime、真板或性能。

**Acceptance**

- A1（R6）：config-only ACK后发布CONFIG cause；used-only发布USED；combined保留两者；unknown/zero不伪造cause。
- A2（R1/R6）：唯一owner以existing register/recheck消费cause；event-before/during-register无lost wakeup，每poll最多一次config snapshot。
- A3（R6/D6）：generation race返回Again并retained/self-wake；稳定down/up只在状态变化时推进LinkGeneration。
- A4（R3/D6）：down取消Queued/ARP pending、阻止enqueue/submit但继续DeviceOwned reclaim；completion不计peer delivery。
- A5（D1/D6）：down关闭、up推进/open SocketEpoch seam；QueueEpoch保持不变，link flap不触发reset或旧socket复活。
- A6（兼容）：MS03/MS04 ISR/ABI、Iteration 003 recovery、quiet path与axnet full tests不退化。

**Verification**

1. `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test` 的等价分步命令。
2. `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` 的等价分步命令。
3. axnet link/cause focused tests；ordinary与qemu-diagnostics完整串行、`--test-threads=1`。
4. axdriver_virtio(net)与virtio-drivers(alloc) link focused tests；触碰下层时跑对应全量。
5. `make ARCH=riscv64 build` 或项目当前QEMU feature等价kernel build；不要求真实HMP runtime。
6. `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`、kernel相关rustfmt、`git diff --check`、完整diff Review、`openspec validate ms07-qemu-single-hart-recovery-semantics --strict`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | ISR、pure logic、event协议、link accessor、Service gate和缺失policy seam已定位。 |
| Design | PASS | cause flags、bounded snapshot、复合gate、epoch identity与down/up语义由R6/D1/D6固定。 |
| Iteration Plan | PASS | Task 3.1独立形成link event/policy baseline；handle terminal与runtime留后续Iteration。 |
| Cycle Scope | PASS | 只展开3.1；不前移3.2或QEMU资格。 |
| Task Contracts | PASS | 入口、行为、测试见证、禁止项和stop条件足以直接建立RED。 |
| Traceability | PASS | R3/R6、D1/D6、Task 3.1、代码面与A1–A6闭合映射。 |
| Verification | PASS | host IRQ、axnet model/full、driver link与kernel build覆盖本轮边界。 |

技术 Gate 2 检查通过；用户已审计并批准本计划（「更改gate状态，开始实施」），Plan Context 改为 `ready`，Act 开始执行 Task 3.1。

**Persisted Evidence**

- Mode: none

命令与决定性输出可低成本重跑，Act Response 足以保存 Gate 结果。

- Budget: 本 Cycle 最多 5 个文件（含 README），整个 change 最多 20 个 Evidence 文件；当前不创建 Evidence。

**Risks and Notes**

- link gate与recovery gate必须独立保存并组合判定；复用单一布尔值会让link up错误开放正在reset/faulted的数据面。
- combined IRQ可以只wake一次，但cause flags必须同时保留；测试应检查语义而非wake次数固定为二。
- SocketEpoch本轮只建立state/transition seam；Iteration 005负责把public handle绑定到epoch并映射terminal。
- 单hart QEMU host/model结果不能证明SMP或真实板级config IRQ时序。

## Act Response

- Status: reported

**Implemented**

Task 3.1（Config IRQ 与 link policy）完整实施，含首次报告与按 Plan Review 两处 Blocking ACT-DEVIATION 的当前 Cycle 修复，分层如下：

1. IRQ cause 发布（R6/A1）：`virtio_net_irq_logic` 新增 `should_publish_config`（bit 1 独立判定）；`net_irq_handler` 在 ACK 后分别按 used/config 位调用 `axnet::publish_queue_event()`（used）与新增的 `axnet::publish_config_event()`（config），combined 时两者都发布、互不覆盖；unknown/zero 不伪造 cause。
2. QueueEvent 有界 cause flags（A1/A2/A3）：`QueueEvent` 新增 `cause_used`/`cause_config`；`publish_event` 置 used、新增 `publish_config` 置 config（均 Release 升 generation + wake）；新增 `take_causes()`（AcqRel 单次清除两个 flag）。公开入口 `publish_config_event()` 已在 `lib.rs` 导出。
3. Owner config 微步（A2/A3/A5）：`poll_active` 在 register 后 take cause，每 poll 至多一次 config snapshot；`Again` → 重新 publish_config（保留 cause，gen 升使后续 sleep 路径自动 retry）；Down/Up → 在 round 后发布 stack progress。
4. Device/Router/Service link 链：`Device` trait 新增默认 `read_link_status`（经 `recovery_control()`）与 `tx_set_link_hold`；`EthernetDevice` 新增 `link_held` 并纳入四处 send/preflight gate（与 `recovery_hold` 复合，任一挡住即 `Full`）；`Router` 新增 `read_link_status`/`tx_set_link_hold`；`Service` 新增 `read_link_status_target`/`tx_set_link_hold_target`/`link_policy_step_target`（R6/D6/A3/A5）。
5. Link/SocketEpoch 边界（D1/A5）：`Service` 持有 `link_state`/`link_generation`/`socket_epoch`/`link_seam_fault`；`link_policy_step_target` 在状态变化时：down → 设 link hold + `tx_cancel_queued_target` + `tx_cancel_pending_target`（不关 DeviceOwned）、推进 LinkGeneration 与 SocketEpoch（关闭旧边界）；up → 清 link hold（不覆盖 recovery hold）、推进 SocketEpoch（开放新边界）；`QueueEpoch` 永不前进。
6. **当前 Cycle 修复（Finding 1，Blocking，A5/D1）**：`link_policy_step_target` 在一切 transition 工作之前做 seam fail-stop 守卫。任一 checked identity（`link_generation` 或 `socket_epoch`）等于 `u64::MAX`（或 `link_seam_fault` 已置位）时：持久化 `link_seam_fault`、强制 `tx_set_link_hold_target(true)` 保持数据面关闭、立即返回 `LinkStep::Fault`，不推进任何 identity、不提交 `link_state`、不允许后续 link-up/event 重新开放数据面。原 `checked_seam_epoch` 辅助函数（先逐 counter 推进各自的 fail-stop，导致溢出时部分提交另一 identity）已删除；两个 counter 改为在守卫之后一致提交（`link_generation += 1; socket_epoch += 1;`）。
7. **当前 Cycle 修复（Finding 2，Blocking，A2/A3/A4/A5 边界见证）**：补齐缺失的 witness。
   - CONFIG cause 穿过 register 窗口：新增 `config_event_before_register_is_caught_by_arm_recheck`、`config_event_during_register_window_retries`、`config_event_after_arm_wakes_sleep_decision`，证明 CONFIG 复用与 USED 相同的 arm/recheck 协议，event-before/during-register 无 lost wakeup。
   - SocketEpoch 溢出一致 fail-stop：改写原 `link_policy_checked_socket_epoch_overflow_fail_stops`（其旧断言只查 counter 不 wrapping 且承认返回 `Down`）为 `link_policy_socket_epoch_overflow_fail_stops_consistently`：断言返回 `Fault`、`link_seam_fault` 置位、`socket_epoch` 与 `link_generation` 都不推进、link_hold 保持关闭，且后续 link-up 仍返回 `Fault` 且 gate 不被重新打开。
   - LinkGeneration 溢出 + 溢出后永久 fail-stop：新增 `link_policy_link_generation_overflow_fail_stops_and_stays_closed`，覆盖 A3 identity 溢出；断言 `Fault`、另一 identity 与 `QueueEpoch` 不变、gate 不打开、后续事件保持 fail-stop。配合 `Service::set_link_generation_for_test` 观察器。
   - stable-down exactly-once：新增 `link_policy_stable_down_cancels_each_owner_once`，证明 down 只取消 Queued/ARP-pending 各一次，后续 stable-down 返回 `NoEvent` 不重复取消、不推进 seam、gate 保持。
   - link gate 下 DeviceOwned 仍可 reclaim：新增 `crates/axnet/src/device/tests.rs::link_hold_gates_new_send_but_not_device_owned_reclaim`，证明 `tx_set_link_hold(true)` 只挡住新 dormant-slot enqueue（`Full`），已 DeviceOwned 的 ticket 仍可 `Reclaimed`，不 strand completion/资源。
8. **当前 Cycle 修复（Rework，Blocking，A4/A5 fail-stop Queued 隔离）**：修复审计暴露的“fail-stop 后同一 round 仍把 Queued 转为 DeviceOwned”路径。
   - `Service::link_policy_step_target` 的 seam fail-stop 守卫在进入时（`link_seam_fault` 首次置位）额外调用 `tx_cancel_queued_target()` 与 `tx_cancel_pending_target()`，在同一个 Service guard 内恰好一次关闭全部 pre-submit/Queued ownership，镜像 down 分支的单一取消线性化点；已 fault 的后续事件不重复取消。DeviceOwned 保持可 reclaim，`QueueEpoch` 不推进。
   - `EthernetDevice::tx_submit_one` 在 peek slot 前新增复合 gate：`recovery_hold || link_held` 时返回 `TxSubmitStep::Full`，让 submit path 直接服从 link/recovery hold，堵死任何残余 Queued 在同一 round 进入 DeviceOwned 的路径。恢复/降速 hold 外单调推进不变，DeviceOwned reclaim 不受 gate 影响。
   - 测试 fixture `LedgerRecoveryDevice`（真实 `FixedFrameQueue`+`TicketTracker`）补齐 `send`/`tx_submit_one` 的 link_hold 复合 gate 与 `tx_set_link_hold`，使其忠实现-model 产品 submit 行为；新增 `leaked_service_ledger_link`（Active 非 faulting 真 ledger）。
   - 新增 RED-first witness：`link_policy_socket_epoch_overflow_closes_queued_and_blocks_submit` 与 `link_policy_link_generation_overflow_closes_queued_and_blocks_submit`。两者预置真实 Queued owner（`Device::send` 入队真 ticket），分别触发 SocketEpoch/LinkGeneration overflow，驱动 owner round，断言：driver submit 次数不增（`submitted_ticket` 不变）、ticket 不进入 DeviceOwned（`device_owned_len()==0`）、Queued/ARP-pending 恰好各取消一次、`QueueEpoch` 不变、link gate 永久保持、Queued slot 被 drain，后续 link-up 仍返回 `Fault` 且不重开。修复前这两项测试 RED（守卫无取消时 `cancel_queued_calls != 1` 且 submit 可达），修复后 GREEN。既有 DeviceOwned completion 的 reclaim 由 `link_hold_gates_new_send_but_not_device_owned_reclaim` 继续见证（link gate 下仍 `Reclaimed`）。

**Changed Files and Symbols**

- `kernel/src/drivers/virtio_net_irq_logic.rs`：`should_publish_config`。
- `kernel/src/drivers/virtio_net_irq.rs`：`net_irq_handler` 双 cause 发布。
- `crates/axnet/src/async_rx.rs`：`QueueCauses`；`QueueEvent::{cause_used,cause_config,publish_config,take_causes}`；`publish_config_event`；`RxRxFuture::poll_active` config 微步；测试 fake `RecoveryDriverStats{link,link_reads,link_again,link_hold}`、`ScriptedRecovery::read_link_status`、`RecoveringDevice::tx_set_link_hold`；新增 `config_event_*`、`link_policy_socket_epoch_overflow_fail_stops_consistently`、`link_policy_link_generation_overflow_fail_stops_and_stays_closed`、`link_policy_stable_down_cancels_each_owner_once`。
- `crates/axnet/src/lib.rs`：导出 `publish_config_event`。
- `crates/axnet/src/router.rs`：`Router::{read_link_status,tx_set_link_hold}`。
- `crates/axnet/src/device/mod.rs`：`Device::{read_link_status,tx_set_link_hold}` 默认实现。
- `crates/axnet/src/device/ethernet.rs`：`EthernetDevice::{link_held,tx_set_link_hold}`，send/preflight 四处复合 gate；`tx_submit_one` 的 `recovery_hold || link_held` 复合 submit gate（rework）。
- `crates/axnet/src/service.rs`：`LinkStep`；Service `link_state/link_generation/socket_epoch/link_seam_fault`；`read_link_status_target`/`tx_set_link_hold_target`/`link_policy_step_target`（seam fail-stop 守卫，rework 加 Queued/ARP-pending 恰好一次取消）；删除 `checked_seam_epoch`；测试观察器 `set_link_generation_for_test`（新增）、`socket_epoch`/`link_generation`/`link_seam_fault`。
- `crates/axnet/src/async_rx.rs`（rework）：测试 fixture `LedgerRecoveryDevice` 补 `tx_set_link_hold` + `send`/`tx_submit_one` 的 link_hold 复合 gate；新增 `leaked_service_ledger_link`、`preset_queued_owner`、`drive_link_fault_owner_round`、`link_policy_socket_epoch_overflow_closes_queued_and_blocks_submit`、`link_policy_link_generation_overflow_closes_queued_and_blocks_submit`。
- `crates/axnet/src/device/tests.rs`：`link_hold_gates_new_send_but_not_device_owned_reclaim`（新增）。
- `tests/ms03-irq-host-harness.rs`：`should_publish_config` 与 combined 双发布 witness。
- `tests/ms04-async-rx-host-harness.rs`：IRQ source guard 要求 config publisher 与双发布顺序。

**Deviations from Plan**

- 无实质偏差。符合 Gates 的非实质记录：
  - `read_link_status_target`/`tx_set_link_hold_target` 由 `link_policy_step_target` 内部直接调用（而非 owner 单独调用），作为向前足够、避免 product dead-code 的实现选择；`socket_epoch()`/`link_generation()`/`link_seam_fault()`/`set_socket_epoch_for_test`/`set_link_generation_for_test` 观察器限定 `#[cfg(test)]`。
  - 原 `checked_seam_epoch` 删除：其逐 counter 的 store-and-advance fail-stop split 正是 Finding 1 部分提交的根因，改为守卫+一致提交后不再需要。
  - axnet host 全量测试需要 `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`（复用 MS06 Iteration 004 记录的环境 wrapper，`/tmp/opencode/cc-nopie.sh` 是既有环境前提，属 `scripts/cc-nopie.sh` staged 的外部改动，本 Cycle 未将其计入实现 diff）。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2（观测计数语义 + Iteration 005 预留，见下）

逐任务与全量 diff 均已复核。A1-A6 逐一核对：A1 双 cause 独立发布不覆盖；A2 每 poll 至多一次 config snapshot + register/take 闭 window（新增 CONFIG before/during/after 三个 register-window witness）；A3 `Again` 保留 + gen self-wake、seam 仅状态变化时推进（新增 LinkGeneration 溢出 fail-stop witness）；A4 down 取消 Queued/ARP pending exactly-once（新增 stable-down witness）且不关 DeviceOwned（新增 link-hold 下 reclaim witness）；A5 down/up 只推进 SocketEpoch、QueueEpoch 不变、link flap 不触发 reset，且 checked epoch 耗尽以一致 fail-stop 结束、gate 永久保持、后续 link-up 不能重新开放（新增 SocketEpoch/LinkGeneration 溢出一致 fail-stop witness），且 fail-stop 在同一个 Service guard 内恰好一次关闭 Queued/ARP-pending、submit path 服从 link/recovery hold，同一 round 不能把 Queued 转为 DeviceOwned（新增两个真 ledger Queued-preset witness，修复前 RED）；A6 全量回归全绿。V1-V3 快照 ABI 未改动。ISR 全程只 status/ACK/publish，不触 config/Service/descriptor。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| ms03 host harness | `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test` | `test result: ok. 35 passed; 0 failed` | PASS |
| ms04 host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test` | `test result: ok. 16 passed; 0 failed` | PASS |
| axnet ordinary 全量 | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1` | `test result: ok. 442 passed; 0 failed`，exit 0 | PASS |
| axnet qemu-diagnostics 全量 | 同命令 + `--features qemu-diagnostics` | `test result: ok. 466 passed; 0 failed`，exit 0 | PASS |
| link 模型测试 | `cargo test ... 'link_policy_\|link_generation_\|config_event_\|closes_queued'` | 11 passed；0 failed（含新增 3 config_event、4 overflow、2 stable/held） | PASS |
| fail-stop Queued 隔离（RED→GREEN） | `cargo test ... closes_queued` | 守卫取消禁用时 2 FAILED；修复后 2 passed（RED-first 见证） | PASS |
| device/reclaim | `cargo test ... 'device::\|link_hold_\|owned'` | 143 passed；0 failed（含 `link_hold_gates_new_send_but_not_device_owned_reclaim`） | PASS |
| driver link | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --features net -- --test-threads=1` | `36 passed; 0 failed`（含 2 link tests） | PASS |
| driver link | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc -- --test-threads=1` | `43 passed; 0 failed`（含 link_status 测试） | PASS |
| kernel QEMU build | `make ARCH=riscv64 build` | starry-kernel release 构建成功，`.bin` 生成，exit 0 | PASS |
| rustfmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` + `rustfmt --edition 2024` 三个改动文件 | exit 0 | PASS |
| diff 白测 | `git diff --check` | exit 0 | PASS |
| OpenSpec validate | `openspec validate ms07-qemu-single-hart-recovery-semantics --strict` | `Change ... is valid`，exit 0 | PASS |

**Persisted Evidence**

None required. Plan Context `Persisted Evidence` 模式为 `none`；命令与决定性输出均可低成本重跑，Act Response 足以保存 Gate 结果。

**Experience Candidates**

None. axnet host 测试所需的 `cc-nopie.sh` wrapper 属既有环境前提（MS06 Iteration 004 已记录），不是本次新验证出的可重复高风险操作路径。

**Remaining Issues**

无阻塞项。遗留 Minor：
- combined 中断在 handler 中调用 used 与 config 两个 publisher，`isr_publish`/`isr_wake` 计数器会为一次 combined 中断计两次——属观测计数语义（两次发布），非控制流问题。
- Iteration 005 需把本轮的 `socket_epoch` 边界与 public handle 绑定并映射 terminal（本轮已按 Non-goals 只建 seam，不提前实现）。

**Commit or Diff Reference**

Diff reference: `git diff`（工作树，未提交）——Task 3.1（含两轮当前 Cycle 修复）变更跨 12 文件。commit 未建（未获提交授权）。staged 的 `scripts/cc-nopie.sh`（`A`）属外部改动，不在 Task 3.1 Change Surface 内，未随本 Response 声明实现，予以保留。

## Plan Review

- Review Result: accepted

**Findings**

1. **Minor — link-held 空队列走 backpressure 观测路径。**
   `EthernetDevice::tx_submit_one` 在检查 queue slot 前检查
   `recovery_hold || link_held`，因此 link-held 且空队列时返回 `Full` 而不是
   `Empty`，会增加一次 `tx_again` 并选择 register/recheck 路径。fail-stop/down
   已先取消 Queued/ARP-pending，使 `tx_pending == false`；deadline只在
   `(submit_full || submit_held) && tx_pending` 时 arm，所以该细节不会触发
   reset、延长 deadline、形成 busy loop或破坏 quiet-path acceptance。
2. **Non-blocking — BASELINE-CHANGED：`scripts/cc-nopie.sh` 仍是 staged 的
   外部改动。** Act Response 已明确把它排除在 Task 3.1 产品实现 diff 外；
   本轮继续保留，不影响 Acceptance。

**Deviation Classification**

- None。上一版 ACT-DEVIATION 已由当前 Cycle 的局部修复与真实 ledger witness
  闭合；未改变 Plan Context、需求或 task contract。
- BASELINE-CHANGED：`scripts/cc-nopie.sh` 不属于 Task 3.1，实现报告已正确排除。

**Acceptance Gaps**

None。A1–A6 均满足。

**Convergence**

achieved。SocketEpoch/LinkGeneration fail-stop在同一 Service guard内 first-entry
取消 Queued/ARP-pending，产品 submit path同时服从 link/recovery hold；同一 owner
round不再能把旧 Queued ticket转成 DeviceOwned，既有 DeviceOwned仍可 reclaim，
QueueEpoch保持不变。

**Evidence**

- 独立复核 `service.rs::link_policy_step_target`、
  `async_rx.rs::poll_active/service_round/arm_and_handle_data_deadlines`、
  `device/ethernet.rs::tx_submit_one` 与真实 ledger fixture的调用顺序；取消、hold、
  submit和deadline条件闭合。
- 真实 Queued owner fail-stop witness：`closes_queued` 2 passed，0 failed；两类
  checked identity overflow均证明无 driver submit、无 DeviceOwned迁移、Queued
  slot被清空、取消各一次、QueueEpoch不变且后续 link-up不重开。
- link policy focused：9 passed，0 failed；link-hold focused：1 passed，0 failed。
- 本轮独立重跑 axnet ordinary：442 passed，0 failed；`qemu-diagnostics`：
  466 passed，0 failed；两条命令均 exit 0。
- Act Response记录 MS03 host harness 35/35、MS04 host harness 16/16、
  axdriver_virtio 36/36、virtio-drivers 43/43与 kernel build均通过。
- strict OpenSpec validation exit 0。
- Persisted Evidence 模式为 `none`；缺少 Evidence 目录不是 finding。

**Follow-up Decision**

接受 Iteration 004 / Cycle 000。Task 3.1形成稳定 baseline；按既有 Iteration
Map展开 Task 3.2。下一 Iteration保持 `draft`，须经用户明确批准后才能进入 Act。

**Iteration Plan Update**

已按 `tasks.md` 展开 Iteration 005：Epoch-scoped socket terminals。未修改 change
级 task、requirements或design。

**Next Cycle**

None.

**Next Iteration**

`../005-epoch-scoped-socket-terminals/000-initial.md`
