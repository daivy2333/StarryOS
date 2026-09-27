# StarryOS 异步高性能网卡初步路线图

> 分析日期：2026-08-09
> 性质：架构探索；已接受的执行顺序以 [tasks](../docs/tasks.md) 为准
> 输入：StarryOS 异步 UART、Embassy 网络模块、ArceOS DWMAC/axnet 工作

## 1. 目标与非目标

目标是在保留 StarryOS 现有 socket 和协议栈能力的前提下，建立：

- 设备中断到 queue task 的可靠通知。
- RX/TX descriptor 和 packet buffer 的显式所有权。
- bounded budget、batching 和 backpressure。
- stack runner 与 socket readiness。
- QEMU、单核真板和 SMP 分层证据。

首阶段非目标：

- 替换 axnet-ng 或 smoltcp。
- 引入 Embassy executor。
- 直接提供用户态 mmap RX/TX ring。
- 首版实现 RSS、多队列和所有 offload。
- 用 QEMU 吞吐替代真板性能结论。

## 2. UART 经验迁移矩阵

| UART 经验 | NIC 对应项 | 迁移方式 |
|-----------|------------|----------|
| RX/TX byte ring | RX/TX descriptor ring | 保留有界队列思想，改为 descriptor ownership |
| copier task | per-queue service task | 批量 reap/refill/reclaim，不逐字节复制 |
| `AtomicWaker` | per-queue RX/TX/error waker | 保留最小 wake 原语 |
| `TxCompletion` 四阶段 drain | TX descriptor completion | 区分提交、doorbell、DMA 完成、descriptor 回收 |
| TTY OUT backpressure | TX ring writable | ring full 返回 Pending/WouldBlock 并注册 waker |
| register-recheck | IRQ/descriptor rearm | 必须保留 |
| QEMU/D1 分证据 | QEMU/目标真板/SMP 分证据 | 必须保留 |
| 单 producer/consumer 契约 | queue owner 契约 | 每个 queue 由唯一任务推进 |

网卡新增的 UART 不具备的问题包括 DMA cache、scatter-gather、MTU、checksum/offload、link state、burst traffic、multiqueue 和 interrupt moderation。

## 3. 分层建议

### 3.1 平台与 DMA 层

职责：

- MMIO、IRQ、clock/reset。
- DMA map/unmap、coherent/streaming allocation。
- CPU address、bus address、IOMMU address。
- cache clean/invalidate 和设备 memory barrier。

该层可参考 ArceOS `axdma` 和 `DwmacHal`，但接口应落在 StarryOS/axhal 边界。

### 3.2 硬件队列层

职责：

- descriptor 初始化、doorbell、head/tail。
- buffer ownership 状态机。
- interrupt cause、mask、ack、rearm。
- error、reset、link 和统计计数。

建议每个 queue 只有一个 owner task。ISR 只发布事件，不直接调用协议栈。

### 3.3 异步 driver adapter

职责：

- 提供类似 `embassy-net-driver::Driver` 的 receive/transmit readiness。
- 通过 token 将 packet buffer 暂时交给 smoltcp。
- 为 RX、TX、link/error 注册 waker。
- 将设备 queue capability 映射成协议栈 capability。

该层可以兼容当前 `axdriver`，避免首版修改全部上游 crate。

### 3.4 stack runner

职责：

- 在任务上下文调用 smoltcp poll。
- 响应设备 readiness、socket waker 和协议栈 timer。
- 形成 bounded poll loop，避免 busy loop。
- 把 socket readiness 继续交给 axpoll/VFS。

### 3.5 执行上下文契约

| 对象 | 唯一修改者 | 可观察者 | 发布点 | 回收点 |
|------|------------|----------|--------|--------|
| IRQ mask/cause shadow | IRQ/control path | queue task | cause snapshot + wake | queue drain/rearm |
| RX descriptor | queue task/device | stack token | DMA completion + sync | token consume/recycle |
| TX descriptor | queue task/device | stack runner | descriptor + barrier + doorbell | completion + DMA sync |
| stack poll state | stack runner | socket/VFS | socket waker/readiness | socket operation |
| reset generation | control path | IRQ、queue、token | reset begin/end | old completion discarded |

关本地 IRQ、禁抢占和跨 hart 互斥是不同机制。per-queue owner task 可以减少锁，但跨 hart IRQ wake、control reset 和 queue service 仍需明确 happens-before。

## 4. RX 和 TX 状态机

### 4.1 RX

```text
Empty
  -> PostedToDevice
  -> DeviceOwned
  -> Completed
  -> CpuSynchronized
  -> StackToken
  -> Recycled
  -> PostedToDevice
```

每次转换必须明确：

- descriptor owner。
- buffer owner。
- cache 操作。
- error/length/checksum metadata。
- 取消或 reset 时的回收责任。

### 4.2 TX

```text
Free
  -> StackToken
  -> ReadyToSubmit
  -> DeviceOwned
  -> Completed
  -> Reclaimed
  -> Free
```

TX API 返回成功只能表示 packet 已交给驱动或设备，不能表示链路对端已收到。若未来需要 drain/fence，应明确观察的是 descriptor reclaim、设备 FIFO 还是更高层 ACK。

## 5. DMA publication 与设备顺序

首版设计必须把 CPU 原子、MMIO 和 DMA 顺序分开：

```text
TX: map/pin -> write payload/descriptor -> DMA write barrier
    -> publish owner/index -> doorbell -> optional posted-write readback

RX: observe completion -> DMA read barrier/invalidate
    -> validate length/status/metadata -> publish token to stack
```

具体 barrier 由架构和 DMA API 决定，不能机械使用 Rust `Acquire/Release` 替代。descriptor valid/owner bit 必须最后发布；设备释放 ownership 前，CPU 不得读取、复用或释放 buffer。

## 6. 中断与预算协议

建议采用固定协议：

1. ISR 读取 cause。
2. ack 或 mask 对应 queue。
3. 原子发布事件并 wake queue task。
4. queue task 以固定 budget 回收 TX、处理 RX、补 descriptor。
5. 若 budget 耗尽且仍有工作，任务主动重排，不立即 unmask。
6. 若已 drain，先注册/确认 waker 状态，再 recheck descriptor。
7. 确认无工作后 unmask；若边沿期间又有完成，立即继续处理。

首版 budget 应可配置并有计数器，不要直接固化“每次处理到空”。后者在持续流量下可能让其他任务饥饿。

硬中断策略需要与设备成本匹配。初版选择最小 ISR；若后续数据证明调度延迟主导 p99，可评估在 ISR 中处理固定数量 completion，但不得分配、阻塞、await 或形成无界循环。

## 7. 背压和完成语义

### 7.1 TX 背压

- descriptor 或 buffer 不足时，nonblocking 路径返回 partial/`WouldBlock`。
- blocking/async 路径登记 TX writable waker。
- reclaim 从满变为非满时 wake。
- poll/select/epoll 的 writable 必须与可实际取得 TX token 对齐。

### 7.2 RX 压力

- refill buffer 不足必须计数。
- 明确丢弃策略、ring starvation 和协议栈消费过慢的区别。
- 不允许通过无界队列隐藏压力。
- 后续可依据 drop 和 occupancy 数据调整 queue depth、budget 或多队列。

背压必须分层观察，不能只看 NIC ring：

| 层级 | 容量或压力源 | 典型动作 |
|------|--------------|----------|
| NIC RX ring | refill buffer/descriptor | drop、补充、budget poll |
| stack backlog | packet/token backlog | 限制 poll、丢弃或流控 |
| socket receive buffer | 应用消费过慢 | readable、drop、协议窗口 |
| NIC TX ring | descriptor/buffer | WouldBlock、writable waiter |
| socket send buffer | 协议发送窗口 | backpressure、重传 timer |

单槽 `AtomicWaker` 只适合单 waiter。首版建议由每接口一个 stack runner 作为设备层唯一 waiter，再由 socket set/axpoll 管理多 socket waiter；若设备层暴露多 waiter，必须使用 wait queue 或 event counter。

### 7.3 completion

- RX completion：设备释放 descriptor，长度和状态有效，CPU cache 已同步。
- TX completion：设备释放 descriptor，buffer 可安全回收。
- link delivery、TCP ACK 和应用处理不属于 NIC TX completion。

## 8. 取消、复位与设备生命周期

状态建议为：

```text
Discovered -> Probed -> Started -> Quiescing
    -> Resetting -> Started
    -> Suspended -> Started
    -> Removed
```

必须定义：

- future 被丢弃是取消通知，还是撤销尚未提交的 token。
- quiesce 后禁止新提交，分别处理 stack-token、device-owned 和 completed-unreclaimed 对象。
- reset 递增 generation，迟到 completion 只能完成到旧 generation 并返回稳定错误。
- fatal error、suspend 和 remove 唤醒所有 waiter。
- remove/panic 前停止 bus mastering/DMA，避免写入已回收内存。
- probe 任一步失败时按逆序释放 IRQ、DMA mapping、queue、clock/reset。
- 自动重放只允许幂等且确认未执行的操作；普通 packet 不因 completion 丢失而盲目重发。

## 9. 隔离与不可信输入

设备 completion 和 firmware metadata 也属于不可信输入。驱动必须校验：

- descriptor index、ring wrap、length、MTU、segment count 和 checksum/offload flag。
- 地址加法、长度乘法和 queue depth 的整数溢出。
- DMA address 必须来自受控 mapping，不能向设备暴露任意 kernel physical address。
- 交给其他安全域或用户态的 padding/metadata 不得泄漏旧内存。
- pinned memory、outstanding packet、queue depth 和 IRQ rate 必须有资源上限。

## 10. 可观测性基线

首个原型应至少输出：

- RX/TX packet 和 byte。
- IRQ、spurious IRQ、mask/unmask。
- 每轮 budget 使用量、budget exhausted。
- descriptor occupancy 高低水位。
- RX no-buffer、drop、error。
- TX ring full、reclaim、wake。
- stack poll 次数和无进展 poll。
- reset、timeout、link transition。
- QEMU/真板吞吐、p50/p99、CPU proxy。
- queue task 调度延迟、最长 IRQ-off 和最长单轮 poll 时间。
- reset generation、取消、迟到 completion 和 remove 后完成计数。

性能报告必须注明 packet size、queue depth、并发流数、方向、测试时长、CPU/hart、QEMU 或板卡型号。

## 11. 里程碑与 Gate

### N0：基线与契约（已由 MS01-MS03、MS16 建立）

- 梳理 StarryOS 当前 axnet-ng/axdriver 调用路径。
- 固定 QEMU VirtIO-MMIO；真板设备由后续板级事实 Gate 决定。
- 固化同步基线和计数器。
- 写出 descriptor ownership、IRQ rearm、backpressure 契约。
- 写出 probe/quiesce/reset/remove 和 generation 契约。

Gate：路径、指标和失败注入可复现。

### N1：QEMU 单队列异步 MVP

- 建立 queue task。
- 建立 stack runner。
- 用 Context/waker readiness 替代 ISR 全栈 poll。
- 支持基础 TCP/UDP、poll 和持续收发。

Gate：无 busy loop、无 lost wakeup、ring 满/空可恢复。

### N2：压力与恢复

- burst、small packet、bidirectional。
- IRQ 合并/budget。
- descriptor exhaustion、丢包、reset、link flap。
- cancel/completion、timeout/completion、reset/late-completion 竞争。
- 长时间 soak。

Gate：所有权无泄漏/重复回收，remove/reset 后 DMA 不触及已回收内存，指标能解释退化。

### N3：SMP 正确性

- 跨 hart 唤醒。
- queue affinity。
- per-queue lock/owner。
- RSS 和多 queue 只在真板性能数据触发后评估。

Gate：不把单核 QEMU 证据扩大为 SMP 结论。

### N4：目标真板后端

- 先采集板卡、MAC、PHY、IRQ、DMA/cache、clock/reset 和 bootloader handoff 事实。
- 控制器兼容 DWMAC 时审计并参考 ArceOS DWMAC；否则只借 DMA ownership 和 bring-up 方法。
- 先完成目标控制器轮询收发，再接入 QEMU 已验证的异步公共层。
- 验证 cache、barrier、MMIO ordering、中断控制器和 SMP。

Gate：QEMU 与真板报告分开，真板错误计数为零或有解释。

### N5：证据驱动优化

候选包括 batching、interrupt moderation、larger ring、checksum offload、scatter-gather、zero-copy 和 multiqueue。只有现有指标定位到对应瓶颈后才进入。

### 11.1 验证矩阵

| 层级 | 必测竞争或边界 |
|------|----------------|
| unit/model | ring wrap、full/empty、max batch、invalid descriptor、generation |
| QEMU functional | immediate-ready、register 后 ready、IRQ 先到、spurious IRQ、lost edge |
| QEMU stress | bidirectional、queue full、budget fairness、shared IRQ、跨 hart wake |
| fault injection | cancel/completion、timeout/completion、reset/late completion、remove/in-flight |
| 真板 | DMA/cache、posted MMIO、IRQ timing、clock/reset、长时间稳定性 |

## 12. 正式 planning 的建议切片

下一份 OpenSpec change 只覆盖 MS04 的异步 RX 队列基线：

```text
范围：
  QEMU VirtIO-MMIO 单设备、单 RX queue
  NetQueueControl + AtomicWaker
  register-recheck
  RX reap/refill + bounded budget

不含：
  异步 TX
  stack runner 和 socket readiness
  reset 与 SMP
  任何真板后端
  multiqueue
  user mmap ring
  全协议栈替换
```

这样先验证 IRQ 到 RX queue task 的异步边界，再让 TX、stack、恢复、SMP 和真板硬件各自拥有独立 Gate。

## 13. See also

- [异步网卡探索总览](async-network-project-overview.md)
- [Embassy 网络模块评估](embassy-network-module-evaluation.md)
- [ArceOS 异步网卡驱动分析](arceos-async-network-driver-analysis.md)
- [UART backpressure 与 MPSC 规划](_archive/uart-backpressure-mpsc-plan.md)
- [异步 UART 与 io_uring 对比](_archive/async-uart-vs-io_uring.md)
