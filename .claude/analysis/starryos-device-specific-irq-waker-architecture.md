# StarryOS 设备专属 IRQ 与任务唤醒分析

> Project: StarryOS
> Branch: net-k3
> Commit: 05dfcfc3ff29401290e666beffcfbe9aeca3267b
> Date: 2026-07-29
> See also: [网络实施探索](starryos-network-development-strategy.md) · [网络知识缺口](starryos-network-knowledge-gaps.md) · [网络项目总览](async-network-project-overview.md)

## 目标与范围

本文回答六个问题：

1. 当前 IRQ 分发有哪两层。
2. QEMU UART 占用了什么。
3. 网络 waker 为什么会静默失效。
4. 设备专属 handler 如何分配所有权。
5. MS03、MS04 和 UART 如何分批。
6. 哪些问题仍由 Plan 决定。

范围是 QEMU RISC-V `virt`。
版本以当前 Cargo.lock 为准。
本文不修改代码和 milestone。
文中的顺序不是获批计划。

## 结论

QEMU UART 没有占用网卡 IRQ。
它占用了唯一的全局 IRQ hook。
该 hook 在设备 handler 和 PLIC EOI 后运行。
它不适合承担设备 cause 和 ack。

`axtask::register_irq_waker` 也注册该 hook。
UART 先注册时，网络 hook 注册失败。
调用方看不到失败，任务可能永久等待。
反向注册时，UART 也可能静默失效。

推荐使用 PLIC 设备 handler 表：

- UART IRQ 10 绑定 UART handler。
- VirtIO-net IRQ 7 绑定网卡 handler。
- UART handler 唤醒 UART copier。
- 网卡 handler 唤醒网卡 queue task。
- 全局 hook 不承担正确性职责。

PLIC handler 表已经提供分发。
不需要再建全局设备 dispatcher。
固定少量 waiter 使用 `AtomicWaker`。
这与 K01 和 M36 的约束一致。

## 当前调用链

当前 QEMU 外部中断顺序如下：

```text
Supervisor External Interrupt
  -> PLIC claim
  -> IRQ_HANDLER_TABLE.handle(claimed_irq)
  -> PLIC complete
  -> axhal global IRQ_HOOK(claimed_irq)
  -> return from trap
```

平台在 claim 后查设备 handler 表。
设备 handler 返回后才执行 EOI。
随后 `axhal` 调用全局 hook。

相关实现：

- [QEMU PLIC 分发](https://github.com/arceos-org/axplat_crates/blob/811837d8c699941f43665510b6e30700faa0e633/platforms/axplat-riscv64-qemu-virt/src/irq.rs#L178-L217)
- [axhal 全局 hook](https://github.com/arceos-org/arceos/blob/6c6765c05df0550e31edb0ca82d468199f108b3f/modules/axhal/src/irq.rs#L12-L48)

QEMU UART 当前走以下路径：

```text
init_uart_hardware
  -> register_irq_hook(uart_isr_wrapper)

任意已处理 IRQ
  -> uart_isr_wrapper(irq)
  -> 读取 UART IIR
  -> 禁用对应 UART 中断
  -> RX_WAKER / TX_WAKER / DRAIN_WAKER
  -> UART copier task
```

`uart_isr_wrapper` 不按 IRQ 号过滤。
因此网卡 IRQ 也会触发一次 UART 检查。
UART IIR 没有 pending 时通常不唤醒任务。
这增加了跨设备耦合和诊断噪声。

QEMU UART 注册位置见
[uart_init.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/kernel/src/drivers/uart_init.rs#L252-L369)。
UART 的固定 waker 见
[isr.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/crates/uart_16550/src/async_/isr.rs#L17-L104)。

网络当前走另一条路径：

```text
EthernetDevice::register_waker
  -> register_irq_waker(irq, waker)
  -> register_irq_hook(axtask::irq_hook)
  -> set_enable(irq, true)
  -> PollSet::register(waker)
```

`register_irq_hook` 只允许成功一次。
`register_irq_waker` 没有检查返回值。
UART 先注册后，网络 PollSet 仍会保存 waker。
但全局 hook 仍指向 UART。
网络 PollSet 因此收不到 IRQ 唤醒。

相关实现：

- [axnet 设备 waker](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/crates/axnet/src/device/ethernet.rs#L336-L343)
- [axtask IRQ waker](https://github.com/arceos-org/arceos/blob/6c6765c05df0550e31edb0ca82d468199f108b3f/modules/axtask/src/future/poll.rs#L41-L65)

当前 MMIO probe 还传入 `irq=None`。
因此 MS02 实际没有触发上述冲突。
它使用 10 ms 轮询回退。
MS03 补入 IRQ 后，冲突才会进入运行路径。

## 推荐架构

目标分发如下：

```text
                    PLIC claim
                        |
            +-----------+-----------+
            |                       |
         IRQ 10                    IRQ 7
            |                       |
  qemu_uart_irq_handler    virtio_net_irq_handler
            |                       |
      UART cause              VirtIO cause
      mask source             ack / mask / snapshot
      wake UART waker         wake queue waker
            |                       |
     UART copier task         NIC queue task
            |                       |
       ring / TTY          descriptor reap / refill
                                    |
                              stack runner
```

两类 handler 都通过
`axhal::irq::register(irq, handler)` 注册。
其 handler 类型是无参数函数。
零参数 wrapper 注入静态 IRQ 和设备实例。

D1 UART 已采用这种形状：

```rust
fn d1_uart_irq_handler() {
    uart_isr_wrapper(UART_IRQ);
}
```

QEMU UART 可采用同样的桥接。
注册结果必须检查。
失败时不能继续声称异步 UART 可用。

网卡 handler 不应锁住 axnet Service。
`VirtIoNetDev` 已被 Service 持有。
任务上下文可能同时持有相关锁。
ISR 获取该锁会引入死锁风险。

网卡需要独立的 IRQ control seam。
它至少提供以下能力：

```text
read_cause
ack
mask
rearm
snapshot counters
wake queue owner
```

该 seam 只持有 IRQ 控制状态。
descriptor 和 packet 不进入 ISR。
具体落点由 Plan 选择：

- 本地 VirtIO adapter。
- 本地化 `axdriver_virtio`。
- 上游接口扩展。

固定 waker 归设备或队列所有。
UART 已有 RX、TX 和 drain waker。
网卡可按 queue task 数量配置。
本文不预设单 waker 或 RX/TX 双 waker。

网络 IRQ 不应先唤醒 stack Service。
M36 要求先唤醒 queue task。
queue task 完成 descriptor 服务后，
再通知 stack runner 和 socket readiness。

## 所有权

| 层 | 持有内容 | 执行职责 |
|---|---|---|
| 平台描述 | MMIO、IRQ、PLIC 事实 | 向驱动提供平台参数 |
| PLIC 平台层 | handler 表、claim、EOI | 分发设备 IRQ |
| 设备 IRQ control | cause、mask、ack、计数、waker | ISR 内最小处理 |
| queue/copier task | buffer、descriptor、budget | 搬运和完成处理 |
| stack/TTY | 协议与字符语义 | 消费任务产出 |
| 全局 hook | 可选观测 | 不参与正确性 |

QEMU UART IRQ 10 已在平台描述中登记：
[qemu.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/kernel/src/platform/qemu.rs#L15-L40)。

VirtIO-net IRQ 7 目前是强推断。
MS02 证明设备位于 `0x10007000`。
QEMU 静态 DTB 将该地址映射到 IRQ 7。
MS03 仍需运行时 claim 证据。

## 任务唤醒协议

设备专属 handler 只解决 IRQ 路由。
可靠异步路径还需要 register-recheck。

任务侧顺序：

```text
检查 completion
  -> 注册 queue waker
  -> rearm 设备通知
  -> 再检查 completion 和 cause
  -> 有工作则继续
  -> 无工作才返回 Pending
```

ISR 侧顺序：

```text
读取 cause
  -> ack / mask
  -> 保存最小 snapshot
  -> wake queue waker
  -> 返回
```

该协议关闭两个窗口：

- 事件发生在第一次检查之后。
- 事件发生在 waker 注册之前。

UART copier 已使用设备私有 waker。
网卡仍需建立 queue 级 control。
MS03 只验证 ISR 控制面。
MS04 再引入 queue task 和该协议。

## 分批边界

推荐按以下依赖顺序调查和实施：

1. 将 QEMU UART 改为设备 handler。
2. 保留现有 UART waker 和 copier。
3. 验证 UART IRQ 10 的 RX/TX 回归。
4. MS03 注册 VirtIO-net 设备 handler。
5. MS03 只记录 cause、ack 和计数。
6. MS04 增加 queue waker 和 register-recheck。
7. 删除正确性路径对全局 hook 的依赖。

UART 迁移和 MS03 可以同一 change 分 iteration。
也可以建立独立的前置 change。
两者必须有分离的测试见证。
Plan 负责选择变更边界。

早期和 panic console 必须保留。
它使用轮询输出，不依赖异步 handler。
UART handler 迁移不能破坏该恢复路径。

## 失败路径

| 失败 | 后果 | 处理要求 |
|---|---|---|
| 忽略 `register` 返回值 | handler 缺失但启动继续 | 初始化失败可见 |
| UART 保留全局 hook | 每个设备 IRQ 都检查 UART | 迁移到 IRQ 10 |
| 用 hook 做设备 ack | ack 晚于 PLIC EOI | 在设备 handler 内 ack |
| ISR 锁住 Service | 中断上下文死锁 | 分离 IRQ control |
| IRQ 只 wake stack | descriptor 无 owner 推进 | 先 wake queue task |
| rearm 后不复查 | 丢失边沿，任务永久 Pending | register-recheck |
| 错误 IRQ 触碰 NIC | 计数污染或错误 ack | handler 按 IRQ 绑定 |
| 注册顺序变化 | UART 或 NIC 静默失效 | 去掉单 hook 依赖 |

设备 handler 迁移不能解决
VirtIO `RING_EVENT_IDX` 的 rearm。
当前 `set_dev_notify` 在该模式下不改 flags。
MS02 已协商 `RING_EVENT_IDX`。
这是独立的 Gate 2 问题。

相关代码见
[VirtQueue::set_dev_notify](https://github.com/rcore-os/virtio-drivers/blob/a9487f2c69826b4caf9830e6d5588f28c27dc24d/src/queue.rs#L324-L338)。

## 验证边界

UART 迁移至少需要：

- IRQ 10 注册返回成功。
- RX 和 TX handler 计数增长。
- 网卡 IRQ 不增加 UART handler 计数。
- 串口输入、输出和 drain 回归通过。
- early/panic 输出仍可用。

MS03 至少需要：

- IRQ 7 claim 可重复出现。
- RX/TX cause 可区分或有明确边界。
- 设备 ack 发生在 PLIC EOI 前。
- 错误 IRQ 不修改 NIC 状态。
- 重复事件不形成中断风暴。

共存验证至少需要：

- UART 与网络 handler 同时注册成功。
- 串口和网络流量并发时各自推进。
- 一个设备的 IRQ 不唤醒另一设备任务。
- 全局 hook 未参与正确性判断。

QEMU 只能证明该设备模型下的行为。
它不能替代真板、SMP 和 DMA 证据。
运行验证仍按 QEMU Runbook 手工执行。

## 未确认项

Plan 仍需决定：

- UART 迁移是前置 change 还是独立 iteration。
- 网卡 IRQ control 放在哪个本地 seam。
- IRQ 7 如何从平台事实传入 MMIO probe。
- VirtIO cause 如何暴露到 wrapper。
- `EVENT_IDX` 下如何 mask 和 rearm。
- 网卡使用一个还是多个 queue waker。
- handler 注册失败采用 panic 还是降级。
- 全局 hook 保留观测用途还是停用。
- EOI 计数如何取得平台级证据。

这些问题影响接口和任务拆分。
未解决前不满足 Gate 2。

## 候选记录

本文形成一个 Decision 候选：

> 固定平台设备使用设备专属 handler。
> handler 唤醒设备私有 waker。
> 全局 IRQ hook 不承担正确性职责。

也形成一个 Knowledge 候选：

> 单槽全局 hook 与忽略注册结果组合后，
> 初始化顺序会使 UART 或 NIC 静默失效。

本文只记录候选。
不自动写入 D 或 K。

## 关键文件

| 文件 | 用途 |
|---|---|
| [kernel/src/drivers/uart_init.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/kernel/src/drivers/uart_init.rs#L252-L393) | QEMU/D1 UART handler 注册与 copier 启动 |
| [crates/uart_16550/src/async_/isr.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/crates/uart_16550/src/async_/isr.rs#L17-L104) | UART cause、mask 和固定 waker |
| [kernel/src/drivers/d1_uart.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/kernel/src/drivers/d1_uart.rs#L192-L238) | D1 设备 handler 范例 |
| [crates/axnet/src/device/ethernet.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/crates/axnet/src/device/ethernet.rs#L336-L343) | 当前网络 IRQ waker 入口 |
| [axhal/src/irq.rs](https://github.com/arceos-org/arceos/blob/6c6765c05df0550e31edb0ca82d468199f108b3f/modules/axhal/src/irq.rs#L12-L48) | 单槽全局 hook |
| [axtask/src/future/poll.rs](https://github.com/arceos-org/arceos/blob/6c6765c05df0550e31edb0ca82d468199f108b3f/modules/axtask/src/future/poll.rs#L41-L65) | PollSet 与全局 hook |
| [QEMU axplat irq.rs](https://github.com/arceos-org/axplat_crates/blob/811837d8c699941f43665510b6e30700faa0e633/platforms/axplat-riscv64-qemu-virt/src/irq.rs#L113-L217) | handler 表、PLIC claim 和 EOI |
| [kernel/src/platform/qemu.rs](https://github.com/daivy2333/StarryOS/blob/05dfcfc3ff29401290e666beffcfbe9aeca3267b/kernel/src/platform/qemu.rs#L15-L40) | QEMU UART 与 PLIC 平台事实 |

