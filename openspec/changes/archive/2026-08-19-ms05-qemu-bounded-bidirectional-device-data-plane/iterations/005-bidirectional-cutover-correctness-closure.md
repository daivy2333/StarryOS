# Iteration 005: Bidirectional Cutover Correctness Closure

## Plan Context

- Status: ready
- Round: 005
- Parent: Iteration 004

**Objective**

修复 Iteration 004 Review 发现的 Active 数据面 owner、event/liveness、deferred RX transaction
和 ticket ledger 缺口。完成后，stack 在 slot mode 不访问 raw queue；每个 TX slot、completion、
software nudge、fatal 和 RX-space 变化都有方向正确且不丢失的 wake；资源 Full 时 task 等待外部
进展而不 busy loop；ARP reply 延后时同一 RX frame 每个 Service poll 至多尝试一次。

**Background**

Iteration 004 已报告 Tasks 2.4、3.1-3.3，并通过现有170个axnet tests、driver tests和QEMU
feature check。Plan Review 对实际代码逐项核对 D3-D6 后确认：现有tests覆盖了局部slot、waker
和三阶段动作，却没有覆盖Active stack与真实raw adapter的组合、stack产生首个TX slot后的
queue wake、Again等待、RX Full与TX backlog交错，或deferred ARP经过Router/Service循环。

这些是既有requirements的实现偏差，不改变Gate 1范围，不新增socket readiness、reset、SMP、
真板或性能需求。Tasks 4.1-4.3依赖正确的C4 owner/event基线，必须顺延。

**Current Baseline**

- Branch: `net-k3`
- HEAD: `5d1a22689ed37d657c0ae39251a2e01980b50ec3`
- Worktree: modified；MS05 Iterations 000-004及项目文档尚未commit。
- Change progress before this iteration: 12/23 tasks；Tasks 3.4-3.6未开始。
- Lifecycle: `Polling → Spawned → Active → Faulted/Unavailable`数值与V2 owner解释已存在。
- Active copier: `RxRxFuture::service_round()`按reclaim 32 → RX 32 → submit 32运行，guard不跨
  `Pending`；当前round-end分类仍把blocked TX slot当作可立即推进backlog。
- Event: `QueueEvent`已有共享generation和queue/stack两个`AtomicWaker`，但software producers
  没有完整接入D5来源表。
- Slots: Ethernet RX/TX各64，ticket tracker最多128 live；stack RX从slots消费，queue task在
  raw driver与slots间复制。
- Persisted Evidence: Iteration 004为`none`；Review不要求`evidence/004-*`目录。

Fresh Review baseline：

| Command | Exit | Result |
|---|---:|---|
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | 0 | 170 passed；现有tests未覆盖本轮RED场景 |
| `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | 0 | 7 passed |
| `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | 0 | 11 passed |
| `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | 0 | 36 passed |
| `cargo check --offline -p starry-kernel --features qemu` | 0 | selected QEMU product compiles |
| `make host-test` | 2 | 55 Rust tests、C tests和protocol self-test通过；UDP socket创建`EPERM`，按R44为`ENV-BLOCKED` |
| `cargo check --offline -p starry-kernel --features lichee-d1` | 101 | change外既有25个`axfs`/`axtask` errors；兼容性观察，不是PASS |
| `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | 0 | Change is valid |

Act Response中的`cargo test -p axnet ...`不能从workspace根复现，因为package名为`axnet-ng`；
本轮统一使用上表manifest-path命令。

**Current-State Evidence**

- `EthernetDevice::preflight_send()`在broadcast、resolved neighbor和unknown/expired neighbor路径
  调用`preflight_ready_tx()`；该方法不检查`TxMode`，总是调用
  `inner.recycle_tx_buffers()`/`can_transmit()`。Active stack可因此进入legacy raw TX路径。
- 真实`VirtIoNetDev`明确拒绝legacy recycle观察queue-owned token，并进入stable fault；该
  adapter行为已有`legacy_recycle_rejects_queue_owned_token`测试。
- `EthernetDevice::tx_reclaim_one()`对`tx_tickets.release(cookie.value())`的false结果不处理，
  仍返回`Reclaimed`。
- `Service::poll()`在`Router::dispatch()`之前处理waiting bit，dispatch后没有发布queue event；
  stack把第一个frame放入空TX slot时，已sleep的queue task没有硬件completion可唤醒。
- `software_nudge_impl()`只调用queue waker，不以Release推进generation；event-before-register
  窗口无法由generation recheck关闭。
- `service_round()`把`tx_pending || submit_full`无条件映射为SelfWakeYield；driver仍Full且无
  completion时会持续自唤醒。`rx_full`分支又先于TX completion/slot backlog，可能等待RX
  space并延迟仍可推进的TX。
- `poll_active()`和arm-fault分支进入Faulted后未发布stack-progress，等待caller无法按D4/D5
  立即观察stable fault。
- `Service::poll()`用Router-buffer-space与RX-slot-space的OR清除waiting bit，但当前waiting只
  由RX slot Full发布。
- `recv_dormant()`在保留deferred ARP RX head时返回`Consumed`；`Router::poll()`对Consumed
  继续while，因此同一guard内立即重试同一frame。
- 现有ARP测试直接调用`Device::recv()`，现有TX Again测试明确断言self-wake；两者是缺口的
  test boundary，不是目标行为见证。
- axnet fresh test还报告本轮新增unused import、dead test fixture、unused test method和
  `drop(&ref)` warning；这些由Task 3.6在不触碰change外smoltcp warning的前提下清理。

**Relevant Code**

| File / Symbol | Current Responsibility | Iteration Use |
|---|---|---|
| `crates/axnet/src/device/ethernet.rs::preflight_send/preflight_ready_tx` | stack TX capacity preflight | 按Polling/Slots owner分离，Active不触碰raw queue |
| `ethernet.rs::tx_reclaim_one`、`fixed_queue.rs::TicketTracker` | completion cookie到live ticket的C4删除 | mismatch进入stable ownership fault |
| `ethernet.rs::recv_dormant`、`device/mod.rs::RxStep` | slot RX事务结果 | 表达retained/deferred head并停止本轮 |
| `router.rs::Router::poll/dispatch` | stack RX循环与TX slot生产 | deferred时停止；TX enqueue后报告queue work transition |
| `service.rs::Service::poll` | caller-driven stack入口与resource wake | commit后发布TX event；只按真实RX-slot space唤醒 |
| `async_rx.rs::QueueEvent/software_nudge_impl` | generation与双waker role | 补齐software producer和direction-aware wake |
| `async_rx.rs::service_round/poll_active/poll_register_recheck` | 三阶段budget、等待与fault | 区分progress backlog和blocked Full；fatal wake stack |
| `device/tests.rs`、`async_rx.rs` tests、`service.rs` tests | host model/集成见证 | 先建立本轮RED，再验证GREEN与warning cleanup |
| `kernel/src/drivers/virtio_net_irq.rs`、`tests/ms04-*` | ISR与V1/V2/critical-section回归 | 保持不变，仅回归验证 |

**Critical Path**

```text
stack TX under Service guard
  → slot-mode preflight reads only slot/ticket capacity
  → Router commit enqueues one TX frame
  → commit publishes queue generation + queue-owner wake
  → queue round submit owns raw TX
  → Again with no completion arms/sleeps; completion IRQ wakes retry
  → reclaim validates cookie is exactly one live ticket

slot RX ARP request
  → stack handles head once
  → reply Full retains head and returns deferred/blocked
  → Router stops current RX loop and releases Service guard
  → TX slot space event enables later poll
  → retry accepts one reply and pops one RX head

any fatal
  → record fault under owner
  → lifecycle becomes Faulted without polling fallback
  → stack-progress waiter wakes and re-evaluates state
```

**Implementation Guidance**

1. 先完成Task 3.4 RED。给fake driver分别计数legacy recycle、raw capacity、queue submit和
   reclaim；通过真实`Service::poll`或等价product seam证明Active preflight的raw计数为0。
   Polling mode既有recycle/capacity语义必须保留。
2. slot-mode preflight必须检查下一次commit实际需要的资源：完整L2 frame slot和checked
   ticket。不要使用`inner.can_transmit()`作为slot readiness，也不要在preflight推进completion。
3. reclaim cookie只有在live set中恰好存在时才能发布C4。`release(false)`是ownership invariant
   fault；不得伪造release、重新插入cookie或继续计成功。
4. Task 3.5为软件事件定义明确target：stack TX enqueue唤醒queue owner；queue RX enqueue和
   TX slot full→nonfull唤醒stack role；RX slot full→nonfull唤醒queue owner；fatal唤醒stack
   role并推进generation；software nudge推进generation并唤醒queue owner。状态commit必须先于
   Release generation。
5. round-end只对当前无需外部资源即可继续的backlog self-wake。submit `Again`且没有visible
   completion时执行BOTH arm/register/recheck并睡眠；completion pending时retry。达到submit
   budget但尚未尝试下一slot时self-wake。RX Full不能阻止本轮后的可推进TX backlog。
6. `budget_exhausted`只在阶段真实达到budget且仍需边界记录时增加一次；round-end yield不冒充
   第二次exhaustion。后续V3可分字段，但本轮现有counter必须可解释。
7. Task 3.6不要用固定重试次数掩盖deferred head。让Device→Router结果显式区分“本frame已
   consumed”和“保留head、停止本轮”；polling raw path的Deferred仍可recycle并结束该raw
   completion，slot path则保留bytes。
8. 新增测试必须走真实调用边：`Service::poll → Router::dispatch/preflight`、
   `Service::poll → Router::poll → recv_dormant`和future的register/arm/recheck。helper-only
   assertion不能作为owner或liveness GREEN。

**Behavioral Change**

- Active stack TX preflight从raw driver readiness切换为slot/ticket readiness；Polling行为不变。
- stack接受首个TX frame后立即给queue task一个不丢失的软件事件，不再等待无关硬件IRQ。
- TX queue/buffer Full保留slot并等待completion/event，不再持续self-wake；budget backlog仍
  有界self-yield。
- RX slot Full与TX backlog并存时，TX仍按独立budget推进；只有确实无法继续的RX阶段等待space。
- deferred ARP reply保留RX head并结束当前stack RX loop；容量恢复后下一轮精确retry一次。
- unknown/duplicate completion cookie进入stable fault；不再增加成功reclaim。
- fatal和software nudge均遵守generation/register-recheck，并唤醒所需role。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 3.4 | R3/R8；unique raw owner、cookie/ticket conservation | `ethernet::{preflight_send,tx_reclaim_one}`、ticket tests | preflight无mode分支；release失败被忽略 | slot preflight不碰raw；mismatch fail-stop |
| 3.5 | R9/R10/R12；event-before-register、Full wait、fair budgets | `QueueEvent`、`Service::poll`、`service_round` | software producers不全；Full自唤醒 | commit→generation→target wake；blocked与progress分流 |
| 3.6 | R2/R13；ARP reply Full retains head | `recv_dormant`、`RxStep`、`Router::poll` | retained head仍返回Consumed | deferred/blocked停止本轮并按space event重试 |

**Task Contracts**

### Task 3.4 — Restore slot/raw ownership and ticket ledger

- Depends on: Iteration 004；blocks Tasks 3.5-3.6的最终full Gate和全部Task 4。
- RED: Active/slot mode的broadcast、resolved neighbor、unknown neighbor preflight触发legacy
  recycle/raw capacity计数；queue-owned completion可被stack preflight观察；unknown/duplicate
  cookie令`release()`返回false但step仍是Reclaimed。
- GREEN: slot-mode preflight只观察固定slot和ticket capacity；Polling mode仍可recycle同步TX；
  queue task是Active raw RX/TX唯一调用者；每个cookie只删除匹配live ticket一次，mismatch返回
  stable `BadState`并停止后续raw submit/reclaim。
- Must modify: Ethernet owner/preflight/reclaim与对应Device/Service integration tests；必要时为
  无副作用slot/ticket readiness增加crate-private观察方法。
- Must not modify: public socket API、VirtIO token布局、legacy Polling语义、ticket最大容量、
  V1/V2 ABI或Task 4 flush waiter。
- Verify: axnet device/service/full tests；axdriver_net 7、axdriver_virtio 11、virtio-drivers 36；
  source guard证明Active stack路径不含raw recycle/alloc/submit/reclaim。
- Stop: 任一stack preflight必须触碰raw completion才能保证Ready，或cookie无法与唯一live ticket
  对应；保存owner ledger并返回Plan。

### Task 3.5 — Close software-event and round-end liveness gaps

- Depends on: Task 3.4。
- RED: stack在queue task sleep前/注册窗口/睡眠后enqueue TX均没有queue wake；nudge-before-register
  丢失；fatal不唤醒stack；Again无completion重复self-wake；RX Full+33 TX只推进首批或直接等待；
  Router buffer有空间但RX slot仍满会清除waiting。
- GREEN:所有D5 software source在commit后推进generation并唤醒正确role；wait protocol重查BOTH
  completion、RX capacity、TX backlog和generation；Again无completion睡眠且completion event恢复；
  RX Full不饿死TX；每个阶段budget一次exhaustion、一次self-yield；fatal stack wake可观察。
- Must modify: QueueEvent/software nudge、Service TX/space publication、round-end decision和tests。
- Must not modify: ISR cause/ACK顺序、用一个AtomicWaker覆盖两role、10ms fallback来推进Active raw
  queue、guard跨Pending、精确fd readiness或V3字段。
- Verify:定向event/service/future tests，确定性交错/竞态filter 100×，UART async tests，
  `make host-test`可执行部分，kernel QEMU check。
- Stop:资源Full只能靠周期轮询恢复、Again与budget backlog无法在状态上区分、或修复需要第二
  queue owner；返回Plan并记录最小状态机。

### Task 3.6 — Bound deferred slot RX transactions

- Depends on: Task 3.5的TX-space event语义。
- RED:一个TX-full ARP request进入`Service::poll`后同一`recv_dormant`调用数大于1或poll不返回；
  retained head错误增加Consumed；释放一slot后重试产生重复reply或不弹head。
- GREEN:首次poll尝试一次、保留精确bytes并返回；space release后第二次poll提交恰好一reply、
  解析一次并弹出一次head；polling raw Deferred仍回收buffer；无per-packet allocation。
- Must modify: slot RX结果语义、Router stop条件和真实Service/Router integration tests；清理由本轮
  引入的unused/dead/`drop(&ref)` warning。
- Must not modify: ARP neighbor policy、pending reordering、retry计时器、socket API、loopback行为
  或通过硬编码循环次数吞掉busy loop。
- Verify: axnet device/router/service/full tests、allocation guards、targeted rustfmt、diff check。
- Stop: retained head与Consumed无法在不增加持久化partial-delivery状态时区分，或retry需要在
  同一guard内自旋；返回Plan。

**Invariants**

- Active/Faulted保持双向AsyncOwned；不恢复Polling raw owner，不出现half activation。
- stack只访问普通frame slots、ARP/IP/smoltcp state；raw buffer/token/descriptor只由queue task
  和driver ledger持有，且不跨`Pending`泄漏。
- ISR仍只执行status/cause、ACK、telemetry和wake；不读取Service、slot或descriptor。
- queue-owner与stack-progress使用不同`AtomicWaker`；Release/Acquire只用于控制publication，
  telemetry保持Relaxed。
- frame slot容量仍为每方向64，live ticket容量128，L3 MTU 1500，V1/V2布局和command不变。
- TCP short write、UDP datagram atomicity、loopback RX-ready、MS03 IRQ分类和UART IRQ restore不变。
- QEMU single-hart结果不作为SMP、真板DMA/cache、物理时序或性能证据。

**Non-goals**

- 不实现C4 flush waiter、V3 snapshot、QEMU diagnostics controls或MS05 probe。
- 不运行手工QEMU，不创建Evidence目录，不修改R44/R51 Runbook。
- 不实现stack runner、准确socket readiness、reset/cancel、SMP、PCI、DWMAC或真板路径。
- 不修复change外smoltcp、virtio PCI lifetime warning或lichee-d1 `axfs`/`axtask` baseline。
- 不运行或报告用户已排除的`make LOG=info build`。

**Acceptance**

| Requirement | Scenario | Design | Task | Code/Test | Simplification | Status |
|---|---|---|---|---|---|---|
| R3/R8 | Active唯一raw owner | D4/D6 | 3.4 | slot-mode preflight raw-call guard + real adapter owner test | None | Covered |
| R3 | cookie/ticket一一回收 | D1/D6/D8 | 3.4 | unknown/duplicate cookie RED/GREEN | None | Covered |
| R10 | stack TX与nudge register-recheck | D5 | 3.5 | enqueue/nudge before-during-after register matrix | None | Covered |
| R9/R10 | fatal唤醒stack role | D4/D5 | 3.5 | independent-waker fatal witness | None | Covered |
| R12 | Again等待与三阶段公平 | D6 | 3.5 | Again sleep/recovery + RX Full/TX 33 matrix | None | Covered |
| R13/R2 | deferred ARP保留并有界retry | D3 | 3.6 | Service→Router→Device integration witness | None | Covered |
| R14 | V1/V2、ISR、UART、QEMU build回归 | D10 | 3.4-3.6 | host/source/driver/kernel regression | None | Covered |

没有Missing或Simplified requirement。Tasks 4.1-6.3继续由后续Iterations 006-008覆盖。

**Verification**

Act必须记录每项RED和GREEN命令、关键输出、退出码、修改文件/符号及full diff Review。最终
自动Gate：

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib device:: -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib async_rx -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
repeat the deterministic axnet async/event/round filter 100 times with zero failures
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
make host-test
cargo check --offline -p starry-kernel --features qemu
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- crates/axnet kernel tests openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane
```

对所有本轮修改Rust文件执行`rustfmt --check --edition 2024`。增加source guards：Active stack
preflight不调用raw recycle/alloc/submit/reclaim；slot和future不含raw buffer/token；ISR不访问
Service/descriptor；V1/V2字段数、顺序与commands不变。

`make host-test`必须执行到最终状态。若仍在
`scripts/ms04_rx_stimulus.py --loopback-self-test`创建UDP socket时得到`EPERM`，Act Response记录
前置通过项、原始命令、exit 2与最早环境失败层，并按R44写`ENV-BLOCKED`；不得写PASS，也不得
把产品assert/compile失败交接为环境问题。该环境项的普通终端复跑仍保留给Iteration 008。

`lichee-d1`不是本轮产品Gate；不得把既有25 errors写成PASS。若Act为兼容性观察而执行，必须
保留exit 101并只比较是否出现本轮change surface的新错误。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已追踪Active stack→preflight/raw recycle、Service→dispatch→TX slot、future round-end、Router→deferred RX及cookie→ticket数据流 |
| Design | PASS | D3-D6已固定唯一owner、event source/role、blocked与progress边界、deferred transaction和ledger fail-stop，无新架构选择 |
| Task Contracts | PASS | Tasks 3.4-3.6含依赖、RED/GREEN、文件/符号、禁止项、命令和stop条件 |
| Traceability | PASS | R2/R3/R8/R9/R10/R12/R13/R14映射到D1/D3-D6、tasks、代码和tests；无Missing/Simplified |
| Verification | PASS | host model→driver→ISR/UART→kernel QEMU compile按依赖分层；R44环境分类与退出码规则明确 |

技术Gate 2检查均PASS。执行授权仍等待用户审计本Plan Context；本文件的`ready`不构成自动
调用`openspec-act`的许可。

**Persisted Evidence**

- Mode: none

本轮确定性unit/model/build结果由Act Response承载。不要创建`evidence/005-*`、Runbook、
Incident或全局M/D/K/R/I条目。

**Risks and Notes**

- slot-mode preflight修复会改变现有test seam对raw capacity的假设；必须同时保留Polling
  preflight GREEN，不能用统一slot行为破坏同步fallback。
- `AtomicWaker`不保存event history；software source必须依赖generation关闭注册窗口，单独wake
  不是完整publication。
- `Again`既可能表示等待completion，也可能在同轮已有completion pending；round-end必须以
  fresh completion/slot/generation recheck决定sleep或retry，不能按枚举值静态选择。
- deferred ARP reply保留同一frame；任何Consumed计数或立即while retry都会制造假进度。
- Plan Review发现的Act命令摘要错误不要求改写历史Act Response；本轮使用可复现命令并在新
  Response中记录真实exit。
- Gate 1沿用2026-08-12已批准MS05 requirements；没有用户决策缺口或风险豁免。

**Deferred Tasks**

- Iteration 006: Tasks 4.1-4.3，ticketed C4 flush、V3 snapshot、QEMU-only controls。
- Iteration 007: Tasks 5.1-5.2，probe与自动产品Gate，required Evidence
  `evidence/007-probe-and-automatic-product-gates/`。
- Iteration 008: Tasks 6.1-6.3，普通终端环境复跑、手工QEMU runtime和最终Review，required
  Evidence `evidence/008-independent-manual-qemu-runtime-and-closeout/`。

## Act Response

- Status: reported

**Implemented**

- Task 3.4：`EthernetDevice::preflight_ready_tx()` 按 `TxMode` 分离 — `Polling` 保留 legacy `recycle_tx_buffers()`/`can_transmit()`；`DormantSlots` 只检查 `tx_slots.is_full()` 与新增的 `TicketTracker::can_alloc()`（live set 头寸 + counter headroom），不再触碰 raw queue。`tx_reclaim_one()` 校验 `TicketTracker::release(cookie)` 结果，unknown/duplicate cookie 返回稳定 `Fault(BadState)` 而非 `Reclaimed`。新增 fake driver 的 `recycle_tx_calls`/`can_transmit_calls` 计数器和 scriptable `FakeTxQueue`（Task 3.2/3.4 reclaim ledger），并加 `active_stack_preflight_source_has_no_raw_tx_entry_points` source guard 证明 slot-mode preflight 分支不含 recycle/alloc/capacity/submit/reclaim 入口。
- Task 3.5：`QueueEvent` 新增 `publish_queue_work()`（Release generation + 仅 queue-owner wake），`software_nudge_impl` 改用它以关闭 event-before-register 窗口；`Service::poll` 在 stack TX dispatch 使 TX slot 从空变非空后发布 queue-owner event（Finding 2），并把 space-wake 条件收紧为仅 `rx_slot_has_space_target()`（Finding 6，删除 `router.rx_buffer_has_space()` OR 与孤儿方法）；`service_round` round-end 改为先判 visible completion、再判 `tx_pending && !submit_full` 的可推进 backlog、`submit_full` 无 completion 时 RegisterRecheck 睡眠、最后才 `rx_full` WaitSpace（Again 不 busy loop、RX Full 不饿死 TX）；`budget_exhausted` 只在阶段真实达到 budget 时递增一次，round-end yield 只记 `self_yield`（guidance 6）；fatal 两处（`poll_active` Fault 与 `poll_register_recheck` Fault）发布 `publish_progress()` 唤醒 stack-progress role。
- Task 3.6：新增 `RxStep::Blocked`（deferred RX head 保留、停止本设备本轮 RX 循环），`recv_dormant()` 在 `FrameStep::Deferred` 返回 `Blocked`，`Router::poll` 对 `Blocked` 直接 `break` 且不增加 consumed/delivered delta；Service/Router 集成测试证明满 TX slot 下 ARP request 每轮只尝试一次、释放 slot 后第二轮恰好提交一次 reply 并弹出 head。清理本轮变更产生的 orphan/warning：删除 async_rx 测试的 dead `CountingDevice` fixture、未用 `LoopbackDevice` import、未用 `Service::fill_rx_buffer_for_test`、`drop(&ref)`、`TxSubmitOutcome`/`DevErrorCategory` dead enum 与 `submit_outcomes` 字段、孤儿 `Router::rx_buffer_has_space()`。axnet 自身 warning 清零（仅 change 外 smoltcp 11 个保留）。

**Changed Files and Symbols**

| 文件 | 符号 | 作用 |
|---|---|---|
| `crates/axnet/src/device/ethernet.rs` | `preflight_ready_tx`, `tx_reclaim_one`, `recv_dormant`, `EthernetDevice::recv_dormant_calls` (test), Device impl test observers | mode-separated preflight; ticket-ledger fail-stop; `Blocked` step; retry-count witness |
| `crates/axnet/src/device/fixed_queue.rs` | `TicketTracker::can_alloc` | 无副作用 ticket 分配 preflight |
| `crates/axnet/src/device/mod.rs` | `RxStep::Blocked`, Device trait cfg(test) observers | deferred/blocked 边界与测试见证接口 |
| `crates/axnet/src/router.rs` | `Router::poll` (Blocked break), 删除 `rx_buffer_has_space` | 停止 deferred 循环；清理孤儿 |
| `crates/axnet/src/service.rs` | `Service::poll` (space-wake 条件 + TX enqueue event), 删除 `fill_rx_buffer_for_test` | Finding 2/6 修复 |
| `crates/axnet/src/async_rx.rs` | `QueueEvent::publish_queue_work`, `software_nudge_impl`, `service_round` round-end, `poll_active`/`poll_register_recheck` fatal wake, 测试 | event 发布、round-end 分流、fatal stack wake |
| `crates/axnet/src/device/tests.rs` | FakeStats 计数、FakeTxQueue、Task 3.4/3.5/3.6 RED/GREEN tests | 测试见证 |

**Deviations from Plan**

- **deferred RX 的 retry 计数**：`service_poll_deferred_arp_retries_once_after_tx_space` 中 retry 轮对 `recv_dormant` 调用 2 次（处理 ARP + Empty probe 结束循环），不是恰好 1 次；断言记录为 `attempts_before + 2`，以证明无界重试被消除而非单次调用。
- **consumed delta 断言位置**：`Service::poll` 已在 service.rs 内部把 router delta 折叠进全局 `RX_TELEMETRY.non_ip_consumed`，故集成测试通过该全局 counter 的 delta 断言，而非 `take_rx_consumed_delta()`。
- **新增 cfg(test) Device trait observers**：为让集成测试穿透 `Box<dyn Device>` 观察 slot 状态，在 `Device` trait 增加 5 个 cfg(test) 默认方法（recv 计数、RX/TX slot 长度、RX head peek、TX slot pop），仅测试构建可见，无产品 ABI 影响。
- 其余实现与 Plan 的 Task Contracts RED/GREEN、Verification 命令逐项一致。

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

<details><summary>Self-Review 记录</summary>

逐任务 Gate 4（spec → code quality）与 Response 前全量 diff Review 已执行：

1. **Spec compliance**：三个 Task 的 RED 均先观察到失败（preflight raw 计数=1、unknown/duplicate cookie=Reclaimed、nudge generation 不推进、Again 自唤醒、RX Full 先于 TX backlog、fatal 不唤醒 stack、Router 对 deferred head 无界循环），GREEN 后对应断言翻转。
2. **Code quality**：round-end 四种状态（visible completion / advanceable TX backlog / Again blocked / RX-full wait）顺序与 D5/D6 一致；`publish_queue_work` 仅唤醒 queue role，`publish_progress` 仅唤醒 stack role，ISR `publish_event` 两 role 均唤醒；generation 全部 Release 发布、Acquire 观察。deferred `Blocked` 不计数 consumed/delivered 且保留 head，retry 由容量事件驱动。
3. **回归**：axnet 184/184、100× async_rx 竞态零失败、drivers 7/11/36、UART 62+8+10、host-test exit 0（loopback self-test 通过，本次无 EPERM）、kernel qemu check exit 0、strict validation exit 0、rustfmt 与 diff check 干净。
4. **清理**：change-owned warnings 归零；仅保留 change 外 smoltcp 11 warnings（plan Non-goal 明确不处理）。

</details>

**Verification Evidence**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| axnet device | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib device::` | `107 passed; 0 failed` | PASS |
| axnet async_rx | `cargo test ... --lib async_rx` | `68 passed; 0 failed` | PASS |
| axnet service | `cargo test ... --lib service::` | `9 passed; 0 failed` | PASS |
| axnet full | `cargo test ... --lib` | `184 passed; 0 failed` | PASS |
| 100× 竞态 | `for i in 1..100; cargo test ... async_rx::tests` | 零失败 | PASS |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | `7 passed` | PASS |
| axdriver_virtio | `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net` | `11 passed` | PASS |
| virtio-drivers | `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture` | `36 passed` | PASS |
| uart_16550 | `cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async` | `62 + 8 + 10 passed` | PASS |
| host-test | `make host-test` | exit 0；6+8+26+15 Rust tests、C syntax、10 probe decision、96-packet stimulus self-test 与 loopback self-test 全部 PASS | PASS（本次无 UDP socket EPERM） |
| kernel qemu | `cargo check --offline -p starry-kernel --features qemu` | exit 0 | PASS |
| kernel lichee-d1 | `cargo check --offline -p starry-kernel --features lichee-d1` | exit 101；既有 25 个 axfs/axtask errors，无 change surface 新错误 | 兼容性观察（非 PASS） |
| rustfmt | `rustfmt --check --edition 2024` 7 files | 无 diff | PASS |
| strict OpenSpec | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change is valid` | PASS |
| diff check | `git diff --check -- crates/axnet` | 无 whitespace 错误 | PASS |

**Persisted Evidence**

`None required`（Plan 005 Persisted Evidence mode: none，单元/模型证据写入 Act Response）

**Experience Candidates**

None

**Remaining Issues**

- Iteration 006 未启动：Tasks 4.1-4.3（ticketed C4 flush、V3 snapshot、QEMU-only bounded controls）。
- 本轮结论限于 model/host/build Gate；手工 QEMU runtime 属于 Iteration 008。
- lichee-d1 既有 25 errors 属 change 外基线，未在本轮修复。

**Commit or Diff Reference**

- Working tree（未 commit）：7 files changed, +666/-100 vs HEAD `244803fb`（仅 crates/axnet）。
- 关键行为变更：`preflight_ready_tx` mode-separated；`tx_reclaim_one` ticket fail-stop；`publish_queue_work`/nudge generation；round-end 分流；`Service::poll` TX-enqueue event + RX-slot-only space wake；`RxStep::Blocked` deferred 边界。

## Plan Review

- Status: reviewed

**Review Result**

follow-up-required

Tasks 3.4 与 3.6 的 owner/ledger 和 deferred RX 主体实现与计划一致，定向 tests 也通过；
Task 3.5 仍有一个产品时序错误，且新增 tests 破坏默认并行 full Gate。Iteration 005 因此不能
作为 flush/V3 的稳定前置。Persisted Evidence mode为`none`，未创建Evidence目录不是Finding。
Blocker Handoff为None，本次处理的是正常reported iteration中的Review发现。

**Findings**

1. **Important — fatal wake 发生在 lifecycle commit 之前。**
   `RxRxFuture::poll_active()` 的`RoundOutcome::Fault`分支和
   `poll_register_recheck()`的`WaitDecision::Fault`分支都先调用
   `publish_progress()`，再执行`transition_fatal()`。wake可立即调度stack waiter；该waiter此时
   仍可能以Acquire读到`Active`并重新睡眠，之后的`Faulted`提交没有第二次wake。实现违反本轮
   Task 3.5和Implementation Guidance明确要求的“状态commit先于Release generation”。现有
   `fatal_wakes_stack_progress`只在`poll_once()`返回后检查最终状态，无法见证wake发生时的状态。

2. **Important — 默认并行axnet full Gate不稳定。**
   fresh full suite首次运行得到`183 passed; 1 failed`；随后默认并行重复10次有5次失败，均为
   `future_rx_slot_full_waits_then_service_poll_wakes`期望wake=1、实际=0。同一test单独重复20次和
   `--test-threads=1`全量184 tests均通过。代码审查确认新增
   `service_poll_deferred_arp_attempts_once_and_stops_round`与
   `service_poll_deferred_arp_retries_once_after_tx_space`会调用生产态`Service::poll()`，却没有像
   其他`QUEUE_EVENT` tests一样持有`SERIAL`；并行时它们可通过`wake_if_space(true)`清除另一test
   的共享waiting bit，后者遂收不到预期wake。第二个test还比较共享`RX_TELEMETRY` delta，同样
   缺少隔离。Act Response的`184/184`与“100×零失败”不能证明默认runner稳定。

**Deviation Classification**

- `ACT-DEVIATION`：Finding 1。实现顺序与Task 3.5已固定的commit→publish契约相反。
- `ACT-DEVIATION`：Finding 2。新增Service tests未遵守已有共享`QUEUE_EVENT`/`RX_TELEMETRY`
  隔离边界，导致计划要求的full Gate失败。
- `NEW-EVIDENCE`：默认并行full suite的fresh 10次重复暴露5次可复现失败，与Act摘要不一致。
- `PLAN-OMISSION`、`PLAN-INVALID`、`BASELINE-CHANGED`：None。需求、D3-D6和Iteration 005
  任务契约足以规定正确行为，无需修改spec或design。

**Evidence**

| Evidence | Result |
|---|---|
| `git diff --cached`逐符号审查 | Tasks 3.4/3.6主体符合计划；两条fatal路径均为publish→transition |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | exit 101；183 passed，`future_rx_slot_full_waits_then_service_poll_wakes`失败 |
| 同一full命令默认并行重复10次 | exit 1；5 PASS / 5 FAIL，失败项与断言一致 |
| 单项失败test重复20次 | exit 0；20/20 PASS |
| full命令追加`-- --test-threads=1` | exit 0；184 passed |
| `git diff --cached --check` | exit 0 |
| `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict`（写入前基线） | exit 0；Change is valid |

**Follow-up Decision**

新增Task 3.7并形成独立修复Iteration 006。它只修复fatal commit→publish顺序、wake-time见证和
共享事件/telemetry测试隔离；这是Tasks 4.1-4.3的正确性前置，不能与flush/V3合并。原flush、
自动Gate和手工QEMU轮依次顺延到Iterations 007、008、009，对应required Evidence路径同步为
008和009。Tasks 3.4-3.6保留completed；Review不改写其历史Act Response。

**Next Iteration**

`iterations/006-fatal-publication-and-parallel-gate-closure/000-initial.md`
