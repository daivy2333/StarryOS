# ArceOS 网卡工作对 StarryOS 的可复用性分析

> Snapshot: [SNAPSHOT](../docs/SNAPSHOT.md)
> Captured revision: `2ccb836a6541bfcf13fd134b5b321fb31c9be52d`
> Observed branch: `net-k3`
> Captured at: 2026-08-09
> ArceOS baseline: main repository `68bda6d`; local `axdriver_crates` checkout `42c1d8c`，parent gitlink records `68397cad`
> See also: [异步网卡探索总览](async-network-project-overview.md)、[实施探索](starryos-network-development-strategy.md)、[异步路线](starryos-async-network-roadmap.md)

## 1. 总体判断

`work/arceos` 对 StarryOS 最有价值的是网卡硬件工程样本，而不是现成的异步驱动。它已经打通 `NetDriverOps`、buffer pool、DWMAC descriptor、DMA coherent memory、smoltcp token adapter 和 VisionFive2 平台控制；但设备层仍以同步 nonblocking 操作为主，部分硬中断直接触发完整协议栈 poll。

它对当前 QEMU/VirtIO-MMIO 开发的直接代码帮助很小。DWMAC 寄存器、descriptor、DMA 初始化和板级 IRQ 不能进入 VirtIO 后端；QEMU 阶段只能借鉴 buffer 生命周期、token 边界，并用 DWMAC 作为第二种设备模型审查公共接口。`AtomicWaker`、register-recheck、queue task、budget、packet slot、stack runner、socket readiness、reset generation 和 SMP 状态机仍由 StarryOS 实现。

ArceOS 的主要收益出现在 QEMU 异步基线完成后的真板阶段。若目标板使用兼容 DWMAC，寄存器、descriptor ring 和 DMA 代码可作为移植输入；若目标板使用其他控制器，具体代码价值下降，但 DMA ownership、CPU/bus 地址分离、cache、PHY、clock/reset、IRQ 和 bring-up 排障方法仍可借鉴。任何情况下都必须按目标板事实重新验证。

因此应按以下原则借鉴：

- QEMU 阶段只参考 `NetBufPtr`、buffer recycle 和 smoltcp token 的所有权轮廓，不移植 DWMAC 数据面代码。
- 用 DWMAC 检查 `NetQueueControl` 是否泄漏 VirtIO available/used ring、descriptor chain 或 ISR status 等 transport 细节。
- 重做 IRQ、queue task、stack runner 和 socket wake 的连接方式。
- 不引入第二套 executor，不复制全局锁和硬编码 IRQ。
- 真板阶段先通过板级事实 Gate，再按控制器类型决定 DWMAC 代码是移植输入还是仅作工程参考。

### 1.1 价值分级

下表是代码适用范围，不是已测量的工期或代码行占比。

| ArceOS 成果 | QEMU/VirtIO-MMIO | 异步公共层 | 目标真板 |
|---|---|---|---|
| `NetDriverOps`、`NetBufPtr`、buffer pool | 参考接口轮廓 | 参考 ownership 和 recycle 语义 | 可作为同步兼容面参考 |
| smoltcp token adapter | 可参考调用边界 | 可由 stack runner 复用同类语义 | 与控制器无关 |
| DWMAC descriptor/register | 不可直接使用 | 仅用于审查接口是否 transport-neutral | 仅在兼容 DWMAC 时具有代码移植价值 |
| `axdma`、CPU/bus 地址、cache | QEMU 不能形成真板证据 | 提供 DMA contract 约束 | 高价值，但必须按平台重新验证 |
| IRQ handler | 不复制 | 作为 ISR 反例和 cause/ack 输入 | 重新实现路由、ack/rearm 和 EOI |
| socket Future、`axasync` | 不复制 | 暴露 lost-wakeup 和双 executor 风险 | 不作为真板驱动基础 |
| VF2 clock/reset/PHY/PLIC | 不适用 | 不进入公共层 | 只作为 bring-up 方法；目标板事实全部重采 |

因此，ArceOS 不改变 QEMU 异步主线的工作范围，只降低部分接口设计和后续真板排障的试错成本。

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

这些内容应成为目标板 DMA/cache Gate 的参考。只有目标控制器属于兼容 DWMAC 时，DWMAC descriptor 和寄存器实现才进入代码移植候选。

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

| 范围 | QEMU 阶段 | 真板阶段 |
|------|------|------|
| `NetDriverOps`/`NetBufPtr` | 参考接口轮廓，补 Context、queue id 和 completion 语义 | 作为同步兼容面或后端 adapter 输入 |
| `NetBufPool` | 参考有界预分配与回收思想，评估 per-queue pool | 按 DMA/cache 属性重新实现或适配 |
| smoltcp token adapter | 参考，由 stack runner 调用 | 继续复用设备无关语义 |
| DWMAC descriptor | 不复制；用于检查 queue contract 是否 VirtIO-specific | 目标控制器兼容时高优先级参考，否则只借 ownership 方法 |
| `axdma`/`DwmacHal` | 只提炼 DMA/platform contract | 适配 StarryOS axhal，并用目标板证据验证 |
| VisionFive2 初始化 | 不使用 | 保留为 bring-up 案例，不作为目标板配置 |
| IRQ handler 中 poll stack | 不复制 | 不复制；真板 ISR 保持 cause/ack/wake |
| 平台 IRQ 硬编码在 axnet | 不复制 | 按目标板路由注册 |
| `AcceptFuture` 当前实现 | 不复制，先修正 waker 协议 | 不作为后端输入 |
| 全局 device/interface/socket 锁 | 不作为 multiqueue 目标 | 不作为 SMP 设计基础 |
| `axasync` executor | 不引入 StarryOS | 不引入 StarryOS |
| probe/start 样本 | 只参考生命周期入口 | 参考；quiesce/reset/remove 另建状态机 |
| CPU Acquire/Release | 复用协议意图 | 不得替代 DMA/MMIO barrier |

## 7. 对 StarryOS 开发阶段的输入

ArceOS 为两个阶段提供的输入并不对称：QEMU 阶段用于约束设计，真板阶段才可能提供硬件代码和排障经验。

### 7.1 QEMU virtio-net 阶段

- 参考 smoltcp token adapter。
- 参考 `NetDriverOps` buffer 回收语义。
- 把 DWMAC 作为第二种设备模型，审查 `NetQueueControl` 是否只表达 completion、submit、reclaim、refill、mask/ack/rearm 和 ownership。
- 不使用其平台 IRQ handler 组织方式。
- 不把 DWMAC descriptor、DMA、PHY 或 clock/reset 代码编译进 VirtIO 后端。
- 由 StarryOS 自行实现并验证 queue task、waker、budget、backpressure、stack runner、socket readiness、reset 和 SMP。

### 7.2 目标真板阶段

- 先记录启动介质、bootloader handoff、DTS/ACPI、MAC 控制器、PHY、MMIO、IRQ 控制器、DMA 地址、cache 属性和 hart/CPU 拓扑。
- 控制器兼容 DWMAC 时，审计后参考 DWMAC register、descriptor、interrupt status 和 `DwmacHal`。
- 控制器不兼容 DWMAC 时，不移植其寄存器和 descriptor；只参考 DMA ownership、PHY/clock/reset 分层、IRQ 诊断和 bring-up 日志结构。
- 无论控制器类型，都重新验证 StarryOS 下的 DMA/cache barrier、MMIO ordering、IRQ/EOI、reset 和 SMP。
- 轮询收发先形成独立基线，再接入 QEMU 已验证的 queue task、stack runner、backpressure 和恢复契约。

## 8. 需要在正式规划前回答的问题

1. StarryOS 当前依赖的 preview.2 `axdriver` 是否允许扩展异步 trait，还是需要本地 adapter。
2. smoltcp runner 是每接口一个任务，还是全局一个任务。
3. queue task 与 stack runner 的 packet handoff 使用 descriptor handle 还是中间 packet slot。
4. NAPI 类 budget 和中断重开协议如何与 axtask 调度结合。
5. 当前 axdriver device object 如何表达 probe rollback、remove 和 generation。
6. 单接口 stack runner 如何扩展到多接口、namespace 或设备热移除。
7. 目标板的 MAC 控制器、DMA/cache、PHY、IRQ、clock/reset 和 bootloader handoff 事实是什么。
8. 目标控制器是否兼容 DWMAC；若不兼容，需要采用哪个已有后端或从哪个最小轮询驱动起步。

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
