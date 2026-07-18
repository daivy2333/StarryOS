# ArceOS 异步网卡驱动工作分析

> 分析日期：2026-07-18  
> ArceOS 基线：`main`，`68bda6d`  
> 目标：提取可复用的驱动、DMA、smoltcp 和真板经验，并识别不能照搬的异步边界

## 1. 总体判断

`work/arceos` 对 StarryOS 最有价值的是“网卡硬件工程样本”，而不是完整的异步执行模型。它已经打通 `NetDriverOps`、buffer pool、DWMAC descriptor、DMA coherent memory、smoltcp token adapter 和 VisionFive2 平台控制；但设备层仍以同步 nonblocking 操作为主，部分硬中断直接触发完整协议栈 poll。

因此应按以下原则借鉴：

- 复用硬件寄存器、DMA、descriptor、cache 和板级初始化证据。
- 复用 `NetBufPtr` 与 smoltcp token 的所有权轮廓。
- 重做 IRQ、queue task、stack runner 和 socket wake 的连接方式。
- 不引入第二套 executor，不复制全局锁和硬编码 IRQ。

## 2. 驱动接口与 buffer 所有权

`axdriver_crates/axdriver_net/src/lib.rs` 的 `NetDriverOps` 提供：

- `can_transmit()`、`can_receive()`。
- `transmit()`、`receive()`。
- `alloc_tx_buffer()`。
- `recycle_rx_buffer()`、`recycle_tx_buffers()`。
- queue size、MAC address 和 interrupt status 清理。

`NetBufPtr` 用裸指针、长度和 packet slice 表达 buffer；`NetBufPool` 预分配 buffer，并用 free list 循环回收。

这些接口已经接近高性能数据面的基本需求，但缺少两项异步信息：

1. readiness 查询没有 `Context` 或 waker 注册点。
2. buffer handle 没有显式携带 queue、DMA mapping、cache state 和 offload metadata。

StarryOS 可以保留兼容层，但新异步队列接口应把“无 descriptor 时如何被唤醒”纳入 trait，而不是让 ISR 知道协议栈全局对象。

## 3. smoltcp adapter

`modules/axnet/src/smoltcp_impl/mod.rs` 的 `DeviceWrapper` 将 `AxNetDevice` 转为 smoltcp `Device`：

- receive 前先回收 TX。
- RX token 持有收到的 `NetBufPtr`，consume 后回收。
- TX token 先分配 buffer，协议栈写入后调用 transmit。
- `InterfaceWrapper::poll()` 同时操作 device、interface 和 socket set。

该结构说明 axdriver 与 smoltcp 之间可以维持 token 化所有权。StarryOS 应继续沿用此边界，但把 poll 的触发从硬中断改为 stack runner：

```text
device IRQ -> queue task -> driver readiness waker -> stack runner -> smoltcp poll
```

这样中断层不需要依赖 socket set，也不需要在禁止抢占或持设备锁时处理 TCP/IP。

## 4. 当前异步实现的边界

### 4.1 socket future

`modules/axnet/src/smoltcp_impl/future.rs` 中：

- `RecvFuture`、`SendFuture`、`ConnectFuture` 会向 socket 注册 waker。
- `AcceptFuture` 在 `WouldBlock` 后直接返回 `Pending`，未注册 accept waker。

这会产生典型 lost wakeup：连接已经到达，但没有事件能再次 poll accept future。StarryOS 设计任何 socket 或设备 future 时，都必须执行“检查状态、注册 waker、再次检查状态”。

### 4.2 硬中断轮询协议栈

当前网络初始化对部分平台 IRQ 使用硬编码，并在 handler 中：

- 打印中断日志。
- 锁定设备。
- 清理设备状态。
- 调用全局接口集合的 `poll_interfaces()`。

这种路径功能上能推进网络，但不适合作为高性能目标：

- ISR 执行时间受协议栈和 socket 数量影响。
- 日志和锁放大 tail latency。
- smoltcp poll 可能触发复杂状态转换和更多锁。
- 多队列时难以保持 queue locality。
- 设备驱动和协议栈产生反向依赖。

建议只保留“读取 cause、ack/mask、唤醒 queue task、返回”。

### 4.3 `axasync`

`modules/axasync` 提供自定义 executor、`MmioEvent` 和动态 `MmioWakerSet`。它证明 ArceOS 上层 async socket 示例可以运行，但对 StarryOS 不宜直接复用：

- StarryOS 已有 axtask。
- 动态 keyed waker 和锁不适合作为每包热路径默认结构。
- 两套 executor 会使 CPU 亲和性、timer 和取消语义复杂化。

可借鉴的是事件分发概念，而不是运行时本身。

## 5. DWMAC 和 DMA 的高价值证据

ArceOS DWMAC 代码已经覆盖：

- RX/TX descriptor ring 的 head/tail 推进。
- descriptor 与 packet buffer 的回收。
- `DwmacHal` 平台抽象。
- DMA coherent allocation。
- CPU address 与 bus address 分离。
- cache flush 和设备可见性处理。
- ring 索引的 Acquire/Release 使用。
- DMA interrupt status 读取与清除。

`modules/axdma` 的 `DMAInfo { cpu_addr, bus_addr }` 和 `alloc_coherent()` 对 StarryOS 真板接入尤其有价值。它提醒我们不能把 UART 的 MMIO + CPU ring 模型直接扩展到网卡：

- descriptor 和 payload 可能由设备并发访问。
- CPU 原子顺序不能替代 DMA memory barrier。
- QEMU coherent 行为不能证明真板 cache 正确。
- 回收 descriptor 之前必须确认设备已经释放 ownership。

这些内容应成为 VisionFive2 DWMAC 阶段的主要代码和验证参考。

按 CPU 与设备的 ownership publication 协议，复用前必须逐项确认：

```text
CPU 写 payload/descriptor
  -> DMA write barrier
  -> 发布 owner/producer index
  -> MMIO doorbell

设备完成
  -> CPU 观察 completion
  -> DMA read barrier/cache invalidate
  -> 校验 length/status
  -> 归还 descriptor 和 buffer ownership
```

Rust Acquire/Release 只能组织 CPU 之间的状态发布，不能替代 DMA barrier、cache maintenance 或 posted MMIO write 的 readback。ArceOS 的具体实现是证据入口，不应在未核对 StarryOS axhal 和 RISC-V 平台语义时直接复制 ordering。

### 5.1 生命周期缺口

当前样本更侧重 probe/start 和持续收发。StarryOS 还需要单独补齐：

- probe 失败时 descriptor、mapping、IRQ 和 clock 的逆序回滚。
- quiesce 后阻止新 token，等待或失败全部 in-flight buffer。
- reset generation，拒绝旧 queue 的迟到 completion。
- suspend/remove 前停止 bus mastering 和 DMA。
- fatal error 时先保存 cause，再 reset 并唤醒所有 waiter。
- link flap 与 device reset 的错误传播边界。

这些不是 DWMAC 收发成功自动证明的能力。

## 6. 可复用与不可复制清单

| 范围 | 建议 |
|------|------|
| `NetDriverOps`/`NetBufPtr` | 复用接口轮廓，补 Context、queue id、DMA metadata |
| `NetBufPool` | 复用预分配与回收思想，评估 per-queue pool |
| smoltcp token adapter | 复用，改为由 stack runner 调用 |
| DWMAC descriptor | 高优先级参考，重新审计 ownership 和 barrier |
| `axdma`/`DwmacHal` | 高优先级参考，适配 StarryOS axhal |
| VisionFive2 初始化 | 复用日志、时钟、复位、地址和 IRQ 证据 |
| IRQ handler 中 poll stack | 不复制 |
| 平台 IRQ 硬编码在 axnet | 不复制 |
| `AcceptFuture` 当前实现 | 不复制，先修正 waker 协议 |
| 全局 device/interface/socket 锁 | 不作为 multiqueue 目标 |
| `axasync` executor | 不引入 StarryOS |
| probe/start 样本 | 复用；quiesce/reset/remove 另建状态机 |
| CPU Acquire/Release | 复用协议意图；不得替代 DMA/MMIO barrier |

## 7. 对 StarryOS 首个原型的输入

ArceOS 最适合为两个阶段提供输入：

### 7.1 QEMU virtio-net 阶段

- 参考 smoltcp token adapter。
- 参考 `NetDriverOps` buffer 回收语义。
- 不使用其平台 IRQ handler 组织方式。
- 先验证 queue task、stack runner 和 socket readiness。

### 7.2 VisionFive2 DWMAC 阶段

- 参考 DWMAC register、descriptor 和 interrupt status。
- 参考 `axdma` coherent mapping 和 bus address。
- 参考已有真板 bring-up 日志。
- 重新验证 StarryOS 下的 cache、barrier、PLIC 和 SMP。

## 8. 需要在正式规划前回答的问题

1. StarryOS 当前依赖的 preview.2 `axdriver` 是否允许扩展异步 trait，还是需要本地 adapter。
2. QEMU 首发设备选择 virtio-net legacy、modern MMIO 或 PCI。
3. smoltcp runner 是每接口一个任务，还是全局一个任务。
4. queue task 与 stack runner 的 packet handoff 使用 descriptor handle 还是中间 packet slot。
5. VisionFive2 DWMAC 是否作为首个真板 Gate。
6. NAPI 类 budget 和中断重开协议如何与 axtask 调度结合。
7. 当前 axdriver device object 如何表达 probe rollback、remove 和 generation。
8. 单接口 stack runner 如何扩展到多接口、namespace 或设备热移除。

这些问题应进入后续 `openspec-plan`，本分析不直接决定实现。

## 9. 证据入口

- `../arceos/axdriver_crates/axdriver_net/src/lib.rs`
- `../arceos/axdriver_crates/axdriver_net/src/net_buf.rs`
- `../arceos/modules/axnet/src/smoltcp_impl/mod.rs`
- `../arceos/modules/axnet/src/smoltcp_impl/future.rs`
- `../arceos/modules/axasync/`
- `../arceos/modules/axdma/`
- `../arceos/axdriver_crates/axdriver_net/src/dwmac/`

路径以 StarryOS 仓库根目录为参照。

## 10. See also

- [异步网卡探索总览](async-network-project-overview.md)
- [Embassy 网络模块评估](embassy-network-module-evaluation.md)
- [StarryOS 异步高性能网卡路线图](starryos-async-network-roadmap.md)
- [ArceOS 真板验证方法](arceos-true-board-validation.md)
