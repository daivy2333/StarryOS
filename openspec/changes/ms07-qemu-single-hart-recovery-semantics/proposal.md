## Why

MS05/MS06 已经建立单 hart QEMU VirtIO-MMIO 的唯一 queue owner、异步双向数据面和应用可见 readiness，但当前 fatal fault、socket terminal 和 task lifecycle 都是 boot-lifetime fail-stop，不能安全处理 reset、取消、阶段超时或 link flap。MS07 必须在进入 SMP 前先固定 owner epoch、quiesce 和恢复失败下的资源保留语义，避免迟到 completion 误归属、重复回收、永久 Pending 和静默丢包。

## What Changes

- 为唯一常驻 queue owner 增加 `Active → Quiescing → Resetting → Reinitializing → Active/Faulted` 恢复状态机，不创建第二 queue task 或 polling fallback。
- 将 reset epoch 绑定到 descriptor/cookie/ticket owner ledger；旧 epoch completion 只能被识别为 stale，不能完成或回收新 epoch 对象。
- 明确 waiter cancellation、pre-submit cancellation 和 device-owned quiesce 三层语义；普通 future drop 不转移 packet ownership。
- 为 submit wait、completion、reclaim、quiesce、reset 和 reinitialize 建立分阶段 deadline、可观察 stage 和稳定错误传播。
- 以 VirtIO-MMIO 整设备 reset 为基线；确认 status 读回 0 后才允许释放或重建旧 queue backing，失败时保留 faulted owner/backing 并拒绝新提交。
- 建立 config-change 到 task context 的 link 控制面；link down/up 与 reset 分开验收，link down 不伪造 completion。
- **BREAKING**：reset 前创建的 public socket 在恢复成功后仍保持终止错误；只有新 epoch 创建的 socket 可以使用恢复后的数据面，既有 TCP 连接不透明续传。
- 增加 host/model fault injection 与单 hart QEMU runtime Gate，分别覆盖不可由真实设备合法产生的 stale completion 和真实 reset/link flap 闭环。

## Requirements Approval and Scenario Sketch

用户于 2026-08-27 以“认可你的分析”批准下列需求和范围，作为 Gate 1 的集中决策：整设备 reset；旧 socket 终止、新 epoch socket 可用；pre-submit packet 取消并报错；device-owned packet 只能 quiesce/reset；link 与 reset 独立验收；结论限定于单 hart QEMU/VirtIO-MMIO。

- **Happy path — reset 后恢复**：前置为 epoch N 的 queue owner 正常服务；触发受控 reset；系统停止新提交、关闭旧 owner ledger、确认 status=0、重建 queue 和 RX refill，进入 epoch N+1；旧 socket 返回终止错误，新 socket 恢复双向流量。任一阶段失败则进入可诊断 `Faulted`，不提前释放 backing。
- **Sad path — reset/quiesce timeout**：前置为 device-owned packet 或设备不确认 reset；触发阶段 deadline；结果必须指出具体 stage、epoch 和未闭合 owner，唤醒 waiter 并拒绝新提交；边界是可能仍被设备访问的 DMA backing 必须保留。
- **Edge case — stale/duplicate completion**：前置为 epoch N 已结束且 N+1 已建立；注入 N 的 completion 或重复 cookie；结果只能记为 stale/fault witness，不得命中 N+1 ticket、释放其 buffer 或成功完成 flush。
- **Cancellation — 三层所有权**：前置分别为只等待、仍在 software queue、已经 device-owned；触发取消；结果分别为清 waiter、移除 queued packet 并返回取消错误、或进入 quiesce/reset；边界是 future drop 不能回收 device-owned buffer。
- **Link flap**：前置为 QEMU `net0` link up；monitor 执行 off/on；config-change 必须唤醒 task context 并发布稳定 link 状态，down 时新发送不得被静默接受，up 后按既定策略恢复；边界是 config IRQ 不搬运 descriptor、不伪造 used-ring completion。
- **Compatibility**：前置为无 reset、无 link flap；运行 MS01、MS04、MS05、MS06 既有 Gate；结果保持 socket、IRQ、budget、quiet path 和数据面行为；边界不扩张到 SMP、PCI、DWMAC、真板或性能资格。

## Capabilities

### New Capabilities

- `qemu-single-hart-network-recovery`: 单 hart QEMU VirtIO-MMIO 网络的分层取消、epoch owner ledger、阶段化 timeout、整设备 reset、link flap、socket epoch 和故障注入语义。

### Modified Capabilities

无。MS07 在既有 MS05/MS06 能力之上新增恢复协议，不削弱既有 queue ownership、flush、readiness 或验证 requirement。

## Impact

- Transport/queue：`crates/virtio-drivers` 的 VirtIO status、config generation、queue teardown/rebuild 和 fake transport seam。
- Driver contract/adapter：`crates/axdriver_net`、`crates/axdriver_virtio` 的 recovery control、cookie epoch、buffer/descriptor ledger 和 link status。
- Queue owner/data plane：`crates/axnet` 的 lifecycle、ticket、cancel、deadline、quiesce、fault/recovery 和 socket epoch。
- Kernel/QEMU control：VirtIO-net IRQ config-change publication、QEMU-only diagnostic ioctl/probe/validator，以及 monitor `set_link net0 off/on` 手工步骤。
- 兼容性：保留单 owner、ISR 最小化、register-recheck、EVENT_IDX、固定容量 slots、C4 flush、stack runner 和 socket readiness；不引入 SMP ordering、自动 polling fallback、真板 reset、multiqueue 或性能优化。
