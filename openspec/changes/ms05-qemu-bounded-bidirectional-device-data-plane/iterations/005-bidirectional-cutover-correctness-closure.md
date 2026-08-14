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

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

Pending.

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: pending
- Full diff reviewed: pending
- Critical findings unresolved: pending
- Important findings unresolved: pending
- Minor findings unresolved: pending

**Verification Evidence**

Pending.

**Persisted Evidence**

None required.

**Experience Candidates**

Pending.

**Remaining Issues**

Pending.

**Commit or Diff Reference**

Pending.

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Next Iteration**

Pending.
