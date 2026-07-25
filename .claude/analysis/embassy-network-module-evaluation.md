# Embassy 网络模块对 StarryOS 的适用性评估

> 分析日期：2026-07-18  
> Embassy 基线：`main`，`106dc1952`  
> 目标：识别可复用接口、队列、唤醒和硬件实现模式，不引入第二套内核运行时

## 1. 结论摘要

本地 Embassy 仓库中核对了 12 个网络专用 crate 或模块：11 个独立网络 crate，加 `embassy-stm32::eth`。对 StarryOS 最有价值的不是直接运行完整 `embassy-net`，而是三个可独立吸收的能力：

1. `embassy-net-driver`：带 `Context` 的收发 readiness 和 token 所有权。
2. `embassy-net-driver-channel` + `embassy-sync::zerocopy_channel`：硬件 runner 与协议栈 Device 分离、packet slot 循环回收。
3. `embassy-sync`：`AtomicWaker` 和 `WakerRegistration`，已由异步 UART 验证可用于 no_std 内核。

`embassy-net`、STM32 Ethernet 和 TUN/TAP 适合作为 adapter、descriptor 和 host test 参考。`embassy-executor`、`embassy-time` 不应直接成为 StarryOS 内核依赖；调度和时间能力应映射到 axtask/axhal。

## 2. 十二项模块盘点

| 编号 | crate 或模块 | 主要职责 | 当前建议 |
|------|---------------|----------|----------|
| 1 | `embassy-net-driver` | 网络设备 trait、RxToken、TxToken、capability、link state | 采用语义，评估最小依赖 |
| 2 | `embassy-net-driver-channel` | runner/device 分离、RX/TX packet channel | 优先原型参考 |
| 3 | `embassy-net` | smoltcp stack、driver adapter、runner、socket waker | 参考，不替换 axnet-ng |
| 4 | `embassy-net-tuntap` | Linux TAP host driver | host 仿真参考 |
| 5 | `embassy-stm32::eth` | DMA descriptor、packet queue、IRQ wake | descriptor 实现参考 |
| 6 | `embassy-net-adin1110` | 单对以太网 SPI 设备 | 特定硬件参考 |
| 7 | `embassy-net-enc28j60` | SPI Ethernet controller | 特定硬件参考 |
| 8 | `embassy-net-wiznet` | WIZnet controller | 特定硬件参考 |
| 9 | `cyw43` | Wi-Fi device runner | channel runner 参考 |
| 10 | `embassy-net-esp-hosted` | hosted Wi-Fi | 控制面和 channel 参考 |
| 11 | `embassy-net-nrf91` | 蜂窝网络 | 控制面和 channel 参考 |
| 12 | `embassy-net-ppp` | PPP link runner | 非 Ethernet 链路参考 |

上述 12 项并非 12 个都应引入。通用 `embassy-sync`、`embassy-futures`、`embassy-time` 不计入这 12 个网络专用实现，但也纳入了依赖边界评估。按可迁移能力合并后，有 8 类能提供帮助：driver contract、packet channel、wake、stack adapter、descriptor ring、host test、future/time 协作、硬件 runner。近期采用范围应严格限制在前三类。

## 3. `embassy-net-driver` 的核心价值

`embassy-net-driver/src/lib.rs` 中的 `Driver` 将设备 readiness 与 future 的 `Context` 绑定：

```rust
fn receive(&mut self, cx: &mut Context<'_>)
    -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)>;
fn transmit(&mut self, cx: &mut Context<'_>)
    -> Option<Self::TxToken<'_>>;
fn link_state(&mut self, cx: &mut Context<'_>) -> LinkState;
```

该接口比“`can_receive()` 返回布尔值，ISR 再手动 poll 整个协议栈”更适合异步内核：

- 无 buffer 时 driver 能登记正确 waker。
- packet buffer 通过 token 暂时转交协议栈，consume 后回到设备。
- RX 返回 RX/TX token 对，符合 smoltcp 在接收处理时可能立即应答的需要。
- capability、hardware address 和 link state 保持设备边界清晰。

StarryOS 不一定要直接替换 `axdriver_net` trait，但新异步 adapter 应具备相同语义。尤其不能只添加一个全局网卡 waker，而忽略 RX、TX、link、error/reset 的不同等待条件。

## 4. `driver-channel` 和零拷贝 channel

`embassy-net-driver-channel::new()` 为 RX 和 TX 分别建立 `zerocopy_channel::Channel`，返回：

- `Runner`：硬件任务持有，负责取得 TX slot、提交硬件收到的 RX slot、更新 link 和地址。
- `Device`：协议栈持有，实现 `embassy_net_driver::Driver`。

`zerocopy_channel` 传递的是可循环复用的元素引用，而不是把 payload 再复制进消息队列。它对 StarryOS 的启发是：

```text
RX empty slot -> hardware/DMA -> RX ready slot -> stack token -> RX empty slot
TX empty slot -> stack token -> TX ready slot -> hardware/DMA -> TX empty slot
```

对于 PCIe/virtio/DWMAC，实际实现应让 slot 保存 descriptor/buffer handle，而不是固定二维字节数组。这样可保留 DMA address、cache state、offload metadata 和 queue id。

需要注意：Embassy channel 的静态容量适合嵌入式系统，但 StarryOS 后续可能需要按队列动态配置 depth。应先复用所有权和 wake 规则，不必机械复制泛型布局。

另一个必须显式补足的边界是 waiter 数量。`AtomicWaker` 或单槽 `WakerRegistration` 只适合一个逻辑 waiter。若同一 TX queue 允许多个 socket task 直接等待 writable，后注册者可能覆盖先注册者。StarryOS 必须在以下方案中明确选择一种：

- queue 只有一个 stack runner waiter，由 runner 再唤醒多个 socket。
- 使用 wait queue 或 event counter 支持多个 waiter。
- 由更高层把多 waiter 串行化成单一设备 waiter。

不能仅因 UART 使用 `AtomicWaker` 成功，就默认 NIC 多流量场景仍满足单 waiter 契约。

## 5. `embassy-net` 的参考边界

`embassy-net/src/driver_util.rs` 使用 `DriverAdapter` 把 Embassy driver 转为 `smoltcp::phy::Device`。`Runner::run()` 则在任务上下文中持续调用 `poll_fn`，由 driver 和 socket waker 决定下一次唤醒。

这正是 StarryOS 当前缺少的中间层：

```text
NIC queue readiness -> stack runner wake -> smoltcp poll -> socket readiness
```

但不建议直接采用完整 `embassy-net`，原因是：

- StarryOS 已有 axnet-ng 的 socket set、路由、VFS 和 syscall 集成。
- 完整替换会扩大回归范围，并制造两套 socket 状态管理。
- Embassy 的 static resource/config 模型与通用 OS 的设备发现、热插拔、多接口需求不同。

推荐移植 `DriverAdapter` 和 runner 的思想，在 axnet-ng 周围建立本地实现。

## 6. STM32 Ethernet 的 descriptor 参考

`embassy-stm32/src/eth` 的 `PacketQueue<TX, RX>` 同时拥有 descriptor 数组和 packet buffer。其 driver token 直接映射 descriptor buffer，consume 后再推进 RX pop 或 TX transmit。

可借鉴点：

- queue depth 明确体现 RAM 与吞吐的权衡。
- descriptor 与 buffer 生命周期绑定。
- IRQ 通过 `AtomicWaker` 唤醒数据面。
- token consume 是所有权转换点，而不是隐式复制点。

不可直接照搬点：

- STM32 DMA descriptor、cache 和 interrupt register 是 MCU 特定实现。
- 通用 OS 需要处理 IOMMU、scatter-gather、动态 MTU、offload 和多队列。
- 静态 singleton queue 不足以表达 PCI/平台总线上的多设备。

## 7. 其他模块的使用方式

### 7.1 TUN/TAP

`embassy-net-tuntap` 依赖 std 和 Linux TAP，不能进入 StarryOS 内核。它可用于：

- 对照 driver readiness 行为。
- 在 host 上做 adapter/token 模型测试。
- 构建故障注入和 packet trace 原型。

### 7.2 硬件 runner 家族

CYW43、ESP-hosted、NRF91、PPP、WIZnet 等实现共同证明了“一个硬件 runner 加一个协议栈 Device”的拆分可跨总线复用。它们对当前 virtio-net/DWMAC 的寄存器代码价值有限，但对控制面状态机、link change 和错误恢复有参考意义。

### 7.3 futures 和 time

设备 runner 常用 `embassy-futures` 的 select/join，以及 `embassy-time` 的 timeout/deadline。StarryOS 应映射到：

- axtask 的 task、yield、block_on 或本地 `poll_fn`。
- axhal/axtask timer 和 deadline。
- 现有 `axpoll` readiness。

不建议引入 `embassy-executor`，否则会出现 executor、timer queue、task wake 和 CPU 亲和性的双重管理。

## 8. 面向 StarryOS 的最小采用方案

建议按以下顺序验证：

1. 定义本地 `AsyncNetDriver` adapter，语义对齐 `embassy-net-driver::Driver`。
2. 建立单 RX queue、单 TX queue 的 descriptor/token 状态机。
3. 为 RX、TX、link/error 分别提供 wake source。
4. 建立 axnet-ng/smoltcp runner，使协议栈 poll 只发生在任务上下文。
5. 在 QEMU virtio-net 上验证 register-recheck、ring full、budget exhausted 和 IRQ rearm。
6. 为 quiesce/reset/remove 增加 generation，使旧 token 和迟到 completion 不能命中新 queue。
7. 明确每个 waker 的单 waiter 或多 waiter 契约。
8. 数据证明 buffer channel 带来额外复制后，再考虑更深层零拷贝。

首个版本可复制接口思想而不新增 Embassy crate 依赖。只有在依赖树、no_std、版本稳定性和 unsafe 审计可接受后，才决定是否直接依赖 `embassy-net-driver` 或 `embassy-net-driver-channel`。

采用评审还应区分三类事实：

- Embassy 的 trait 证明接口模式可行，不证明 StarryOS SMP、热插拔或设备移除正确。
- QEMU virtio-net 证明虚拟设备功能，不证明真实 DMA cache coherence。
- STM32 Ethernet 证明静态 descriptor/token 实现可行，不证明通用 OS 的 IOMMU、多设备和动态生命周期。

## 9. 证据入口

- `../../embassy/embassy-net-driver/src/lib.rs`
- `../../embassy/embassy-net-driver-channel/src/lib.rs`
- `../../embassy/embassy-sync/src/zerocopy_channel.rs`
- `../../embassy/embassy-net/src/driver_util.rs`
- `../../embassy/embassy-net/src/lib.rs`
- `../../embassy/embassy-stm32/src/eth/`
- `../../embassy/embassy-net-tuntap/`

路径以 StarryOS 仓库根目录为参照。

## 10. See also

- [异步网卡探索总览](async-network-project-overview.md)
- [ArceOS 异步网卡驱动分析](arceos-async-network-driver-analysis.md)
- [StarryOS 异步高性能网卡路线图](starryos-async-network-roadmap.md)
- [UART backpressure 与 MPSC 规划](_archive/uart-backpressure-mpsc-plan.md)
