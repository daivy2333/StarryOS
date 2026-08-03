## Context

MS02 已证明 QEMU VirtIO-MMIO 轮询网络可用。
当前 `axdriver` 在 MMIO probe 时调用
`VirtIoNetDev::try_new(transport, None)`。
`EthernetDevice::requires_polling` 因此返回 `true`，
`Service::register_waker` 使用 10 ms fallback。

`axnet::init_network` 只调用一次 `net_devs.take_one()`，
并把该设备移动进 `EthernetDevice`。
后续 `Service -> Router -> EthernetDevice` 是唯一可变数据面。
`receive`、`recycle_rx_buffer`、`transmit` 和
`recycle_tx_buffers` 都通过该实例推进队列。

QEMU UART 当前调用单槽
`axhal::irq::register_irq_hook`。
设备 handler 表已由 PLIC 路径提供；
D1 UART 也已经使用 `axhal::irq::register`。

当前 revision 是
`05dfcfc3ff29401290e666beffcfbe9aeca3267b`。
调查基线：

- axnet service tests：8 passed，0 failed。
- UART async tests：62 passed，0 failed。
- UART doctests：18 passed，0 failed。
- QEMU target 冷构建在 `lwext4_rust` C 编译处被当前
  sandbox 的 `Bad system call` 阻塞。
- 同 revision 的 MS02 Evidence 已有成功 target build
  和 QEMU MMIO 运行见证。

## Goals / Non-goals

Goals:

- 将 QEMU UART 与 VirtIO-net 绑定到设备专属 handler。
- 固定 QEMU net MMIO `0x10007000` 与 PLIC IRQ 7。
- 诊断 used-ring、config-change、ack 和异常 cause。
- 在保留 `RING_EVENT_IDX` 时证明重复投递。
- 保持一个 `VirtIoNetDev` 和一个 queue owner。
- 为后续 MS04 提供可定位的 IRQ 基线。

Non-goals:

- 网卡 waker、queue task 和 register-recheck。
- descriptor 或 packet 在 ISR 中搬运。
- 删除 MS02 轮询路径。
- 将 IRQ 7 暴露给现有 axnet waker 路径。
- 本地化 `axdriver`、`axdriver_virtio` 或
  `virtio-drivers`。
- PCI、SMP、VF2、热插拔和性能结论。

## Decisions

### D1：诊断控制面不是第二个网卡

新增 QEMU-only VirtIO-MMIO IRQ control。
它只持有平台 MMIO/IRQ 事实和原子 telemetry。
它不实现 `NetDriverOps`，不保存 queue 指针，
也不访问 descriptor、buffer pool 或 axnet `Service`。

唯一 `VirtIoNetDev` 继续由
`axnet::init_network -> EthernetDevice` 持有。
MS03 不复制、不替换该实例。

现有 `VirtIoNetDev::receive` 仍会做兼容性
`ack_interrupt`。
设备 handler 是正常的首个 ack 位置；
轮询侧后续读到零属于兼容 readback。
若受控窗口出现非零 cause 被轮询侧抢先清除、
无法关联的 spurious 激增或重复投递失败，
实施必须停止，不得用宽松阈值掩盖。
该结果将触发后续 Plan 决定是否本地化 driver。

### D2：IRQ 事实与数据面 capability 分开

平台描述增加可选 VirtIO-MMIO net 事实：

- base：`0x10007000`
- size：`0x1000`
- device ID：1
- PLIC IRQ：7

QEMU 提供该事实。
Lichee D1 与 VisionFive2 明确为 `None`。
启动时读取 magic、version 和 device ID，
确认该地址仍是当前 net transport。

IRQ 7 只传给 diagnostic control。
`VirtIoNetDev` 仍以 `irq=None` 构造，
`EthernetDevice::requires_polling` 继续为 `true`。
否则 axnet 会关闭 10 ms fallback，
并进入尚未修复的全局 waker hook 路径。

### D3：UART 使用 IRQ 10 设备 handler

QEMU 增加零参数 UART wrapper。
wrapper 从平台描述取得 IRQ 10，
再调用现有 `uart_isr_wrapper`。

`init_uart_hardware` 使用
`axhal::irq::register` 并检查返回值。
失败时立即用清晰消息 panic。
此时 copier 尚未启动，
early/panic console 仍走轮询输出。

D1 已有设备 handler，保持不变。
UART 的 RX、TX、drain waker 和 copier 不修改。

### D4：ACK、EOI 与 EVENT_IDX rearm 分层

VirtIO-MMIO handler 的顺序：

```text
handler entry
  -> volatile read interrupt status
  -> classify used/config/unknown/spurious
  -> write raw non-zero status to interrupt ACK
  -> read back status for residual witness
  -> update Relaxed telemetry
  -> return
```

QEMU PLIC 在设备 handler 返回后执行 complete。
MS03 不占用全局 hook 记录 EOI。
handler entry 证明 claim 后已分发；
两个独立刺激都进入 handler，
证明前一次 handler 返回和 EOI 已完成。

当前协商包含 `RING_EVENT_IDX`。
`VirtQueue::set_dev_notify` 在该模式下不 rearm。
真正 rearm 是唯一轮询 owner 在 `pop_used`
后更新 `avail.used_event`。
所以完整重复投递链是：

```text
IRQ handler ACK -> PLIC EOI
  -> polling owner consumes used ring
  -> pop_used updates used_event
  -> next device event -> next IRQ
```

MS03 保留该 feature，
不增加伪 rearm counter，
也不通过关闭 feature 获得通过。

### D5：使用按需快照，不在 ISR 打印

Telemetry 使用单调 `AtomicU64`，内存序为
`Relaxed`，因为它只用于观测，不发布队列状态。
快照包含：

- handler、used-ring、config-change。
- ack、ack 后仍 pending。
- spurious、unknown 和 last raw status。
- 注册状态、MMIO base、IRQ。
- 现有 UART handler count。

`sys_ioctl` 增加只读诊断命令
`0x4e49_4431`。
它把固定 `repr(C)` 快照复制给用户态。
不提供 reset，证据使用前后单调差值。

新增 `tests/ms03_irq_probe.c`。
payload 在输出 READY 并 `tcdrain` 后取前快照，
在窗口内不打印，再取后快照。
它提供以下手工模式：

- 两次独立 UDP RX，不发送应用响应。
- 两次独立 UDP TX，第一次用于 warm-up。
- UART-only 单字节输入。
- UART 与 UDP RX 并发。
- 有界 idle 窗口。

payload 只产生刺激和打印快照，
不驱动 QEMU 或 guest shell。

### D6：失败语义保持分层

UART IRQ 10 注册失败是启动配置错误。
系统 panic，copier 不启动。

Net 地址验证、IRQ 映射或 handler 注册失败时：

- 输出 transport、地址、IRQ 和失败层。
- telemetry 标记未注册。
- MS03 IRQ Gate 失败。
- MS02 轮询数据面继续可用。

unknown cause、ack 后同位仍 pending、
空闲窗口计数持续增长或两次刺激只有一次 handler
都使对应 Gate 失败。

### D7：QEMU Evidence 由用户手工取得

QEMU 和交叉 C 编译是当前能力边界。
不新增自动 QEMU harness。

运行证据至少包含：

- 单网卡 QEMU 命令与启动注册日志。
- RX 两次、TX 两次、UART-only、并发和 idle 快照。
- `RING_EVENT_IDX` 协商记录。
- MS01 14/14 回归。
- MS02 TCP/UDP 与 MMIO probe 回归。
- 环境、命令、退出结果和有界观察窗口。

证据只支持 QEMU 单 hart。

## Current-State Call Paths

Network ownership:

```text
axruntime::rust_main
  -> axdriver::init_drivers
  -> axnet::init_network
  -> net_devs.take_one
  -> EthernetDevice { inner: AxNetDevice }
  -> Service::poll
  -> Router::poll
  -> EthernetDevice::recv/send
  -> VirtIoNetDev queue operations
```

Interrupt dispatch:

```text
supervisor external interrupt
  -> QEMU PLIC claim
  -> IRQ_HANDLER_TABLE.handle(irq)
  -> device handler
  -> PLIC complete
  -> optional global hook
```

Current QEMU UART:

```text
entry::init
  -> uart_init::init_uart_hardware
  -> register_irq_hook(uart_isr_wrapper)
```

Target QEMU UART:

```text
entry::init
  -> uart_init::init_uart_hardware
  -> register(10, qemu_uart_irq_handler)
  -> uart_isr_wrapper(10)
```

## Risks / Trade-offs

- Registry net driver retains a compatibility ack in `receive`.
  Controlled single-hart windows and residual/spurious telemetry
  are required to expose interference.
- Platform base depends on QEMU device ordering.
  Header validation prevents a wrong slot from passing silently.
- Snapshot ioctl is diagnostic ABI, not a stable public API.
  It is QEMU-only and read-only.
- UART registration now fails fast.
  This changes silent degradation into an observable boot failure.
- Current sandbox cannot complete the ext4 C cold build.
  Target build and runtime stay at the user capability boundary.

## Migration Plan

1. 建立纯逻辑 RED/GREEN 测试和平台事实。
2. 迁移 QEMU UART handler，保持 UART tests GREEN。
3. 增加 net handler、ACK 和 telemetry。
4. 增加只读快照与 guest probe payload。
5. 完成 agent 可执行的格式、host tests 和 diff Review。
6. 用户完成 target build 与手工 QEMU Evidence。
7. Plan Review 审计实现和运行证据。

回滚时先移除 net handler 与 snapshot ioctl，
再移除平台 net 事实，最后恢复 UART 注册方式。
任何回滚都必须保留 MS02 轮询网络和 early console。

## Requirements Traceability Matrix

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R1 平台事实 | 地址解析；映射缺失 | D2,D6 | T1,T3 | `platform::descriptor`; `virtio_net_irq::init` | host config tests；启动日志 | None | Covered |
| R2 设备 handler | 双注册；冲突；网卡到达 | D3,D4,D6 | T2,T3 | `uart_init`; `virtio_net_irq` | UART GREEN；QEMU 注册与隔离快照 | None | Covered |
| R3 ISR 边界 | used；config；spurious | D1,D4,D5 | T1,T3 | `virtio_net_irq_logic`; handler | cause/telemetry host tests；QEMU 快照 | None | Covered |
| R4 单实例/owner | 启动；IRQ control；证据分类 | D1,D2 | T1,T3,T5 | `axnet::init_network`; diagnostic control | source audit；MS02 回归 | None | Covered |
| R5 ACK/EOI/rearm | 重复投递；EVENT_IDX；风暴 | D4,D6 | T1,T3,T4 | handler；`VirtQueue::pop_used` | RX2/TX2；idle；source audit | None | Covered |
| R6 分层计数 | RX；TX；共享 cause | D4,D5 | T1,T3,T4 | telemetry；snapshot ioctl；probe | snapshot delta markers | None | Covered |
| R7 兼容性 | net fallback；UART fail；功能回归 | D2,D3,D6 | T2,T3,T5 | `entry::init`; `uart_init`; axnet | UART tests；MS01/MS02 runtime | None | Covered |
| R8 有界 Evidence | 并发；中断；范围 | D5,D7 | T4,T5 | guest probe；change Evidence | concurrent/idle logs；Evidence index | None | Covered |

## Open Questions

None.
