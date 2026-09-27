# xianxw SDMMC 异步驱动参考与迁移边界

> Snapshot: [SNAPSHOT](../docs/SNAPSHOT.md)
> Captured revision: `5d1a22689ed37d657c0ae39251a2e01980b50ec3`
> StarryOS branch: `net-k3`
> External reference: [xianxw/Final-NO-SDMMC](https://github.com/xianxw/Final-NO-SDMMC/tree/f0bdecedf50047a4efee598ee39080e109f2f25e) `f0bdecedf50047a4efee598ee39080e109f2f25e` (`main`)
> Captured at: 2026-08-14
> See also: [异步网卡架构路线](starryos-async-network-roadmap.md) · [网络开发实施探索](starryos-network-development-strategy.md) · [设备专属 IRQ 与任务唤醒分析](starryos-device-specific-irq-waker-architecture.md)

## 目标与范围

本文评估外部仓库 `modules/simple-sdmmc-extended` 的 IDMAC 异步路径，区分可迁移的驱动原则、设备专属机制和未经验证的实现假设。分析回答六个问题：

1. `IdmacCompletion` 保存了什么，是否能替代 register-recheck。
2. generation 能否识别跨请求的 stale completion。
3. 分层超时和终态验证如何映射到 NIC 生命周期。
4. fail-stop 取消适用于哪些 ownership 形态。
5. 同步/异步共用状态机是否允许运行期 fallback。
6. 哪些结论应进入 StarryOS 后续 milestone，哪些不应改变当前 MS05。

范围限于异步等待、IRQ 状态保存、DMA 生命周期和取消路径。本文不验证 SDMMC 初始化、时钟、卡协议或性能，也不把外部实现当作真板压力证据。

## 结论

外部实现提供四类有价值的参考，但不能原样移植到当前 VirtIO NIC：

1. **W1C cause 保留**：ISR 在清除状态前以 OR 累积保存 `RINTSTS/IDSTS`，等待方再合并当前寄存器和快照。该机制保存会被清除的原因位，适合真实设备的 IRQ 诊断和错误账本。
2. **阶段化超时与终态验证**：提交前 idle、command start、command response、DMA terminal、write-busy 和 reset 分别有 deadline；成功还要核对控制器、DMA 和 descriptor ownership。可迁移的是阶段划分，不是 SDMMC 阶段与 NIC 阶段的一对一映射。
3. **单请求 DMA fail-stop**：异步请求被 drop 后停止 IDMAC；若 reset 失败则保留 descriptor 或 DMA buffer，避免硬件继续访问已释放内存。这是 MS07/MS13 设计取消与恢复策略时的保守对照。
4. **同步/异步共用完成谓词**：同步路径自旋，异步路径等待事件，但共享状态读取和终态验证。StarryOS 可以复用完成判定，不能因此恢复第二个 descriptor owner。

这些发现**不要求修改 MS05**。MS05 已明确：flush future 被 drop 只取消等待，不取消 packet；Active/Faulted 保持异步 ownership，不回退 polling；VirtIO used ring 和 cookie/ticket 是 completion 权威账本。新增结论主要属于 MS07，部分作为条件化硬件要求进入 MS10、MS11 和 MS13。

## 外部实现的数据流

外部路径保持单个活动 transfer，由 `ActiveIdmacTransfer<'_>` 独占 `&mut SdMmc`：

```text
prepare
  -> 确认 command/data idle
  -> 清除上一轮 W1C status
  -> 建立 descriptor 并验证 publication
start
  -> 清空 IRQ snapshot
  -> generation + 1
  -> 写 command / doorbell
wait
  -> ISR 读取 status
  -> 清 W1C status
  -> OR 到本轮 snapshot
  -> notify WaitQueue
  -> waiter 合并 register | snapshot
validate
  -> command/auto-stop/DMA/data-over
  -> descriptor OWN/CES
finish
  -> 清 status
  -> 释放 descriptor
```

取消路径是：

```text
drop ActiveIdmacTransfer
  -> driver faulted
  -> mask interrupt
  -> reset IDMAC
  -> reset 成功：释放 descriptor
  -> reset 失败：保留 descriptor，返回 RecoveryFailed
```

写 DMA 已完成但卡仍 busy 时还有独立的 `AsyncWriteBusyGuard`。该 future 被 drop 会把驱动置为 faulted，因为 card programming 的最终状态未知。

## 机制评估

### W1C cause 快照补充 register-recheck，不替代它

[`IdmacCompletion`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/sdmmc.rs#L53-L105) 使用：

- `fetch_or` 累积一次 transfer 内多次 IRQ 的状态位；
- `snapshot_generation` 发布快照归属；
- 读取位域前后两次检查 generation，避免组合明显跨代的读取；
- [`idmac_completion_status`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/sdmmc.rs#L1835-L1847) 合并当前寄存器与 IRQ 快照。

它解决的是 ISR 清除 W1C 状态后，等待方仍能读取 cause。它不负责关闭“检查为空、注册 waker、准备睡眠”之间的竞态。外部路径仍依赖 `WaitQueue::wait_timeout_until_async` 的谓词等待协议；StarryOS 仍需 [QueueEvent generation/register-recheck](../../crates/axnet/src/async_rx.rs)。

对 VirtIO NIC，used ring、used index、descriptor 和 TX cookie 已经构成持久 completion ledger。IRQ cause 本身不包含 packet identity，且 used-ring IRQ 可能同时代表 RX/TX。额外 OR 快照最多保存错误原因或诊断位，不能替代 `completion_pending()`、reclaim 或 ticket tracker。

对未来真实 MAC，如果 cause 是 W1C、clear-on-read 或 ack 后不可恢复，ISR 应先保存 cause 再 ack。快照只作为诊断和唤醒输入；descriptor/queue 状态仍是数据面完成的权威来源。

### generation 不是完整的 stale-completion identity

外部实现的 ISR 在中断发生时读取全局当前 generation，再把 cause 标为该 generation。硬件 completion 本身不携带 software generation。因此：

- 它能拒绝等待者主动读取不匹配的旧 snapshot；
- 它没有证明旧 transfer 的迟到 IRQ 不会在新 transfer 开始后被标成新 generation；
- 正确性还依赖单活动请求、提交前清 W1C 状态以及控制器不会跨请求迟到投递的假设。

StarryOS T11/MS07 需要更强的 operation identity：generation 必须绑定 queue/reset epoch 和 owner ledger，迟到 completion 要通过 descriptor/cookie 所属 epoch 判定，而不是按 ISR 执行时的“当前代”归属。

### 阶段化超时可迁移，原映射不可迁移

外部超时的真实阶段如下：

| 外部阶段 | deadline | 实际含义 | 可迁移原则 |
|---|---:|---|---|
| pre-submit idle | 100 µs | command/data state machine 仍 busy | 新提交前验证设备允许接收工作 |
| start command | 1 ms | controller 未清 `start_cmd` | doorbell/command 接受阶段单独诊断 |
| command response | 2 s | SD command response 未完成 | 控制面阶段不能混为 DMA completion |
| IDMAC terminal | 5 s | DMA、data-over 等终态未闭合 | device-owned 工作必须有 stall 分类 |
| write busy | 5 s | DMA 后 card programming 未完成 | DMA completion 不等于请求最终完成 |
| reset | 100 ms | IDMAC 无法确认停止 | 未确认停止时禁止释放 DMA backing |

NIC 不存在 SD command response 或 card programming 阶段。后续 Plan 应从本项目状态机定义阶段，例如：pre-submit、doorbell publication、device-owned completion、reclaim、quiesce、reset 和 link control。timeout 必须指明 owner、错误传播、是否触发 reset，以及 backing memory 的处理。

MS05 的 2 秒 QEMU diagnostics lease 是测试控制超时，不是数据面 stall timeout；两者不得合并。QEMU 可验证 deadline、错误传播和 VirtIO device-model 行为，不能证明真板 DMA 停止、cache coherency 或物理时序。

### fail-stop 只证明单请求 DMA 的保守取消形态

[`ActiveIdmacTransfer::drop`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/sdmmc.rs#L295-L308) 在活动请求被取消时停止 IDMAC 并永久 fault 驱动。[`finish_idmac_transfer`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/sdmmc.rs#L516-L545) 在 reset 失败时保留 descriptor。这一做法成立的前提是：

- 同时只有一个活动 transfer；
- future 持有该请求的 DMA context；
- `&mut SdMmc` 排除了并行提交；
- 取消请求与停止整个控制器是同一个故障域。

NIC queue task 是多个 packet 的长期 owner。drop 一个 flush waiter、socket future 或 stack waiter不能停止控制器，也不能释放或取消 device-owned packet。MS05 Task 4.1 的“取消只取消等待”必须保持不变。

外部模式对 MS07/MS13 的价值在于建立安全下限：如果 quiesce/reset 无法证明 bus mastering/DMA 已停止，就保留相关 backing memory、保持 faulted owner 并拒绝新提交，不能为了恢复可用性冒 UAF 风险。

### 同步/异步共用谓词，不代表允许双 owner

外部 `wait_transfer_sync` 和 `wait_transfer_async` 共用：

- `idmac_completion_status`；
- command/terminal/error predicates；
- terminal validation；
- finish/abort 资源回收。

差别只是自旋等待或 WaitQueue 等待。这种复用可以减少两条 API 的状态机漂移。

StarryOS 当前采用全有或全无的双向 owner cutover。Active/Faulted 后 polling 不得重新访问 raw queue。可复用的是完成判定和错误分类，不是运行期自动 fallback。

## 对 StarryOS 路线的落点

| Milestone | 纳入内容 | 明确不纳入 |
|---|---|---|
| MS05：QEMU 有界双向设备数据面 | 不新增范围；V3 继续记录已有 fault stage，flush cancellation 只取消 waiter | 不引入 SDMMC snapshot，不增加数据面 timeout，不让 drop waiter 取消 packet |
| MS07：QEMU 单 hart 恢复语义 | 区分 waiter cancel、pre-submit cancel、device-owned quiesce；为 submit/completion/reclaim/quiesce/reset 分阶段 timeout；generation 绑定 owner ledger；reset 失败保持 faulted owner | 不以 ISR 当前 generation 代替 completion identity；不把 QEMU 结果提升为真板 DMA 证据 |
| MS10：目标板可诊断设备中断 | 当目标 MAC cause 为 W1C/clear-on-read 时，先保存 cause 再 ack；组合 cause 可累积审计 | 不把 cause snapshot 当 descriptor completion |
| MS11：目标控制器轮询双向网络 | 分阶段诊断 submit/doorbell/DMA terminal/reclaim；成功同时核对 controller、descriptor 和 buffer ownership | 不照搬 SD command/busy timeout 数值 |
| MS13：目标板单 CPU/hart 恢复语义 | 真板证明 quiesce/reset 后 DMA 已停；不能证明时保留 backing、拒绝新提交；验证 stale completion 不跨 reset epoch | 不用 QEMU reset 或单纯寄存器读回替代真板 DMA/cache 证据 |

这些机制都不足以单独形成新 milestone。它们分别属于已有中断诊断、数据面终态验证和恢复语义的内部要求，共享对应 milestone 的验证与诊断边界。

## UART 参考价值

UART 已有四条件 `TxCompletion`：ring empty、copier inactive、staged bytes 为零和 transmitter empty；其完成语义比 SDMMC generation 类比更贴合 flush/tcdrain。

外部实现只提示一个未承诺议题：是否为 UART flush 增加 deadline 和稳定错误。该选择会改变 API 语义，且 QEMU 不证明真实串行线 drain 时序。本分析不据此修改 UART milestone 或创建改进项。

## 证据边界与未确认项

- 外部仓库固定 commit 与本地检出一致，分支为 `main`；`net-k3` 是 StarryOS 分支。
- 外部模块 README 标记为 `Experimental`。
- 模块中未找到覆盖 `IdmacCompletion`、取消、reset failure 或 generation race 的单元测试。
- `sdmmc-diagnosis.md` 记录的是 VisionFive 2 卡初始化成功，不是异步 DMA 压力、取消、SMP 或长期稳定性证据。
- 单活动 transfer 和 `&mut SdMmc` 降低了并发复杂度；这不能证明多 descriptor、多 packet NIC 的同等行为。
- OR 快照的内存序和跨 transfer 迟到 IRQ 尚无并发测试，不能提升为项目知识结论。
- 外部真板结果不能替代 StarryOS QEMU gate；StarryOS QEMU 结果也不能替代目标板 IRQ、DMA/cache 和 reset 证据。

## 关键文件

StarryOS：

- [QueueEvent 与 register-recheck](../../crates/axnet/src/async_rx.rs)
- [packet slot 与 TX submit/reclaim](../../crates/axnet/src/device/ethernet.rs)
- [transport-neutral queue contract](../../crates/axdriver_net/src/lib.rs)
- [VirtIO used-ring ISR](../../kernel/src/drivers/virtio_net_irq.rs)
- [UART completion 与 copier](../../crates/uart_16550/src/async_/driver.rs)
- [MS05 design](../../openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/design.md)
- [Milestone roadmap](../docs/tasks.md)

外部参考：

- [`sdmmc.rs`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/sdmmc.rs)
- [`dma.rs`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/src/dma.rs)
- [`README.md`](https://github.com/xianxw/Final-NO-SDMMC/blob/f0bdecedf50047a4efee598ee39080e109f2f25e/modules/simple-sdmmc-extended/README.md)

## 后续 Plan 的决策点

- **MS07 决策**：分别定义 waiter cancellation、尚未提交 work 的撤销、device-owned work 的 quiesce，以及 reset 失败后的 fail-stop 行为。
- **MS07/MS13 决策**：generation 如何绑定 reset epoch、descriptor/cookie 和资源 owner，避免 ISR 按当前代误归属迟到 completion。
- **MS10 条件项**：只有目标控制器 cause 具有破坏性读取或 W1C 语义时才增加 OR cause retention；先由目标板事实确认寄存器语义。
- **MS11/MS13 Gate**：每个 timeout 必须记录失败阶段、owner、原始状态和恢复结果；无法证明 DMA 停止时不得释放 backing memory。

这些决策应由对应 change 的 OpenSpec Plan 完成，不在分析文档中预选具体接口或 timeout 数值。
