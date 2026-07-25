# StarryOS 网络开发实施探索

> Project: StarryOS
> Branch: net-k3
> Date: 2026-07-25
> Baseline: StarryOS `d9480f54ca8493e4a00ce265e8ddad843940fcac`
> Local smoltcp: `f96a26b5968735d142e6999a016060bc5d3ab2b7`
> Local Embassy: `106dc1952bb681e115037ef97dce1bea31094a93`
> See also: [项目总览](async-network-project-overview.md)、[交付估算](starryos-network-delivery-estimate.md)、[Embassy 评估](embassy-network-module-evaluation.md)、[ArceOS 网卡分析](arceos-async-network-driver-analysis.md)、[初步路线](starryos-async-network-roadmap.md)、[真板验证](arceos-true-board-validation.md)

本文补足现有文档没有落到当前代码版本的部分：依赖兼容、真实调用链、QEMU 设备选择、任务边界、分片顺序和每阶段 Gate。它是实施前探索，不是已批准的 change，也不修改产品代码。

## 结论

建议保留 `axnet-ng + smoltcp + axpoll + axtask` 这条主线，按以下顺序推进：

1. 先把本地 smoltcp 0.13.1 接入问题独立解决，保持当前同步网络行为不变。
2. 再用 QEMU VirtIO PCI 取得可重复的 IRQ、RX、TX completion 证据。
3. 引入 queue task、bounded packet slot 和 stack runner，先接受可测量的复制。
4. 完成 socket readiness、背压、reset generation 和压力测试。
5. 补齐 VirtIO MMIO IRQ parity，再进入 VisionFive2 平台和 DWMAC。
6. 真板稳定后才评估 descriptor token、零拷贝、多队列和中断合并。

不能直接把 `starry-smoltcp` 替换为本地上游 smoltcp。当前 `axnet-ng` 依赖 fork 新增的 `RxToken::preprocess`，用它在 TCP SYN 进入协议栈前动态建立 listen socket。本地 smoltcp 0.13.1 没有该 API。直接替换会先导致编译不兼容；即使机械消除调用，也会破坏 `listen/accept` 语义。

不能只在现有 `NetDriverOps` 外包一层 waker 就得到可靠异步驱动。该 trait 没有 `Context`、IRQ cause/mask/rearm、link、reset generation 等接口。`VirtIoNetDev` 又隐藏了底层 `VirtIONetRaw::enable_interrupts/disable_interrupts`。可靠的 IRQ→queue 协议需要本地化 driver seam，或推动上游扩展。

Embassy 最有价值的是接口语义和 runner 模式，不是第二套运行时。近期可以借鉴：

- `embassy-net-driver::Driver` 的 `Context` 感知 RX/TX/link readiness。
- `embassy-net-driver-channel` 的 runner/device 分离和有界 slot。
- `embassy-net` 的 device wake + stack timer + software wake 合流。
- `embassy-sync::AtomicWaker` 的最小 ISR→task 唤醒。

近期不应引入 `embassy-executor`，也不应整体替换为 `embassy-net`。StarryOS 已有 `axtask`、VFS/socket 和 `axpoll`，整体引入会形成重复 executor、time、socket 管理和协议栈所有权。

## 当前基线与真实调用链

启动顺序是：

```text
axruntime
  -> axdriver::init_drivers()
  -> VirtIO MMIO/PCI probe
  -> AxDeviceContainer<AxNetDevice>
  -> axnet_ng::init_network(all_devices.net)
  -> EthernetDevice + Router + Service + SocketSet
  -> kernel entry
```

`axruntime` 在进入 StarryOS kernel 前完成 driver probe 和 `axnet_ng::init_network`。这意味着只在 `kernel/` 内增加任务还不足以替换设备和协议栈初始化。实施时需要本地化 `axnet-ng`、driver 适配层或 runtime 注入点。

socket 路径是：

```text
syscall/VFS Socket
  -> kernel/src/file/net.rs
  -> axnet-ng TCP/UDP
  -> poll_interfaces()
  -> Service::poll()
  -> Router::poll()
  -> smoltcp Interface::poll()
  -> Router::dispatch()
```

当前 TCP/UDP 的 send、recv 和 poll 会主动调用 `poll_interfaces()`。`poll_interfaces()` 以 `while service.poll()` 运行到本轮无进展，因此进展主要由 socket 调用和 smoltcp timer 驱动，不是独立 stack task。

等待路径采用正确的“尝试→注册→复查”：

```text
axtask::future::poll_io()
  -> nonblocking socket operation
  -> Pollable::register()
  -> 再次执行 operation
  -> Pending 或 Ready
```

但 `GeneralOptions::register_waker` 把任务 waker 注册到 Service，再由 Service 根据 `poll_at` 注册 timer，并向设备注册 IRQ waker。它没有直接桥接 smoltcp socket 的单槽 waker。

现有 `EthernetDevice` 和 Router 之间已有中间 `PacketBuffer`。RX 会把 Ethernet payload 复制到 Router RX buffer；TX 会分配 `NetBuf`，再把 IP packet 写入 Ethernet frame。首阶段继续使用有界 packet slot 不会改变“已经存在复制”的事实，但必须记录 copy count、队列占用和 drop，避免把临时设计误称为零拷贝。

关键位置：

| 位置 | 当前作用 |
|---|---|
| `Cargo.toml:3` | workspace 只有 `kernel`；本地 `crates/smoltcp` 尚未纳入 |
| `Cargo.toml:34` | 使用 registry `axnet-ng = 0.3.0-preview.2` |
| `Cargo.toml:53` | QEMU feature 固定包含 PCI bus |
| `Cargo.toml:104` | VF2 只接入平台 crate 和 SDMMC |
| `kernel/src/file/net.rs` | StarryOS socket 到 axnet 的 VFS 边界 |
| `kernel/src/drivers/os_arceos.rs:25` | `axtask::spawn_with_name + block_on`，可承载 queue/stack future |
| `kernel/src/platform/mod.rs:31` | 平台描述符只区分 Lichee D1 和 QEMU |
| `kernel/src/platform/visionfive2.rs:24` | VF2 描述符存在，但尚未被选择 |
| registry `axnet-ng/src/lib.rs:71` | 网络服务初始化 |
| registry `axnet-ng/src/lib.rs:144` | 同步全局 poll loop |
| registry `axnet-ng/src/service.rs:38` | Router、smoltcp、dispatch 串行推进 |
| registry `axnet-ng/src/device/ethernet.rs:336` | 仅 IRQ number 存在时注册 IRQ waker |
| registry `axtask/src/future/poll.rs:43` | 全局 IRQ hook 和 `PollSet` |

## smoltcp 0.13.1 接入边界

本地 `crates/smoltcp` 是 0.13.1、edition 2024、`rust-version = 1.91`。当前项目工具链报告 `rustc 1.95.0-nightly (2026-02-24)`。将仓库复制到临时独立目录、去除只用于测试的 dev-dependencies 后，以下 no_std library feature 组合离线检查通过：

```text
alloc
log
async
medium-ethernet
medium-ip
proto-ipv4
proto-ipv6
socket-raw
socket-icmp
socket-udp
socket-tcp
socket-dns
socket-tcp-reno
```

这个结果只证明 smoltcp library 与本地工具链兼容，不证明它已能替换 `starry-smoltcp`。直接在当前目录检查还会遇到“位于 workspace 下但不是 member/exclude”的 Cargo 边界。

当前链路实际使用 `starry-smoltcp 0.12.1-preview.1`。差异中最关键的是：

```text
starry-smoltcp RxToken::preprocess(SocketSet)
  -> axnet-ng Router::RxToken::preprocess()
  -> LISTEN_TABLE.incoming_tcp_packet()
  -> 在 SYN 被 Interface 消费前创建 listening socket
```

证据位置：

| 位置 | 证据 |
|---|---|
| registry `starry-smoltcp/src/phy/mod.rs:387` | fork 增加 `RxToken::preprocess` |
| registry `starry-smoltcp/src/iface/interface/mod.rs:584` | ingress 前调用 preprocess |
| registry `axnet-ng/src/router.rs:209` | Router 实现该 hook |
| registry `axnet-ng/src/listen_table.rs:126` | SYN 到来时建立 listen socket |
| `crates/smoltcp/src/iface/interface/mod.rs:545` | 上游提供单包 ingress poll |
| `crates/smoltcp/src/iface/interface/mod.rs:506` | 上游提供 egress poll |
| `crates/smoltcp/src/iface/interface/mod.rs:562` | 上游提供 maintenance poll |
| `crates/smoltcp/src/iface/interface/mod.rs:582` | 上游提供下一次 timer deadline |

推荐保持本地 smoltcp 接近上游，在本地化的 `axnet-ng` 中消除 fork API。可选做法有两种：

| 做法 | 优点 | 代价 | 建议 |
|---|---|---|---|
| 把 `preprocess` 补回本地 smoltcp | 最快恢复编译 | 长期维护协议栈 fork；策略侵入 phy trait | 只作为短期验证分支 |
| 在 axnet 层重做 listen/backlog | 上游 smoltcp 保持干净；socket 策略归 axnet | 要重做 listener 预置、补充和并发测试 | 作为正式方向 |

正式方向可以预置 backlog 数量的 smoltcp listening socket，并在连接进入后补充；也可以在 axnet 的 ingress adapter 中先检查 TCP SYN，再把 packet 交给 `Interface`。无论采用哪种方式，必须先用现有 TCP listen/accept 行为建立回归测试。

本地 smoltcp 的 `async` feature 只给 socket 增加单个 `WakerRegistration`。它没有把 `phy::Device` 改成 `Context` 感知接口。多个 OS waiter 不能直接抢占这个单槽。应由每个 socket/方向的 smoltcp waker 唤醒一个本地 event bridge，再由 `axpoll::PollSet` 唤醒多个 VFS waiter。

`axpoll 0.1.2` 的 `PollSet` 容量为 64，`wake()` 是唤醒并清空。容量溢出时会覆盖 slot，并可能提前唤醒旧 waiter。因此它可以作为首版多 waiter bridge，但必须增加 overflow/registration 计数，并明确超过 64 个 waiter 的语义。

TCP 初期应显式启用 Reno。不要因为本地 smoltcp 提供 Cubic 就默认使用；内核首阶段应避免把浮点或额外算法边界混进基础迁移。

## Embassy 与其他依赖的采用策略

本地 Embassy 基线包含：

| 模块 | 本地版本 | 价值 | 近期策略 |
|---|---:|---|---|
| `embassy-net-driver` | 0.2.0 | no_std `Driver`、RX/TX token、link state、`Context` | 可借 trait 语义；是否直接依赖需单独决策 |
| `embassy-net-driver-channel` | 0.4.0 | runner/device 分离、有界 RX/TX slot | 借结构；首版实现本地 channel |
| `embassy-net` | 0.9.1 | smoltcp adapter、runner、timer 合流 | 只借 runner 模式 |
| `embassy-sync` | 0.8.0 | wake/channel primitives | 项目当前是 0.6.2，不应顺带升级 |
| `embassy-executor` | 本地 workspace | executor | 不引入 |

`embassy-net-driver` 本身依赖很轻，可以成为长期公共 driver contract。不过当前 OpenSpec 只批准了 `embassy-sync::AtomicWaker` 的有限采用。直接新增依赖前，应比较两种选择：

| 选择 | 适用条件 |
|---|---|
| 本地 trait，语义对齐 Embassy | 需要 IRQ control、reset generation、DMA ownership 等 OS 特有能力 |
| 直接实现 `embassy-net-driver::Driver` | 只需要把 packet slot 暴露给 smoltcp，且不把硬件控制塞进该 trait |

推荐拆成两层，避免强迫一个 trait 同时承担硬件和协议栈职责：

```text
NetQueueControl
  IRQ cause/mask/ack/rearm
  completion/refill/reclaim
  reset generation
  link/error
  DMA ownership

StackNetDevice
  poll_receive(cx)
  poll_transmit(cx)
  poll_link_state(cx)
  packet token
  capabilities
```

`StackNetDevice` 可以对齐或直接实现 Embassy driver trait。`NetQueueControl` 必须是 StarryOS 自己的 OS/硬件 contract。

依赖采用结论：

| 依赖 | 结论 |
|---|---|
| 本地 smoltcp 0.13.1 | 采用目标，但先完成 axnet listen 兼容分片 |
| `axnet-ng` | 保留 socket/VFS 语义；需要本地化以消除 fork API并接 stack runner |
| `axdriver_net::NetDriverOps` | 作为同步兼容面保留，不作为最终异步硬件 contract |
| `virtio-drivers 0.7.5` | 继续复用 transport/virtqueue；本地 wrapper 要暴露 IRQ control 和 completion |
| `axtask` | 唯一 executor/scheduler |
| `axpoll` | VFS 多 waiter bridge；补容量和溢出观测 |
| `embassy-sync::AtomicWaker` | ISR→queue 的最小唤醒原语 |
| `embassy-net-driver-channel` | 参考，不直接引入，避免连带 `embassy-sync 0.8.0` |
| `embassy-net` | 不引入 |
| ArceOS `axdma`/DWMAC | 真板事实和接口参考，不直接复制 |

## 异步数据路径

建议的首版数据流是：

```text
hard IRQ
  -> 读取 cause / 屏蔽 queue source / 发布 event generation
  -> AtomicWaker.wake()
  -> per-queue task
       -> bounded TX reclaim
       -> bounded RX completion
       -> refill descriptors
       -> packet slots
       -> register-recheck
       -> rearm/unmask
  -> stack event
  -> stack runner
       -> bounded poll_ingress_single
       -> bounded poll_egress
       -> poll_maintenance
       -> socket event bridge
       -> 等待 device/link/software/timer
  -> axpoll / VFS / syscall
```

首版 packet slot 应是有界且所有权明确的复制通道。这样可以先把 VirtIO raw descriptor 限制在 queue task 内，避免 smoltcp token 跨 await 持有 DMA buffer。待 completion、reset 和真板 cache 语义稳定后，再把 slot 替换成 descriptor-backed token。

每个 queue 由一个 task 唯一推进。ISR 不调用 smoltcp，不分配，不阻塞，不遍历 socket。stack runner 是 smoltcp `Interface` 和 `SocketSet` 的唯一修改者。

唤醒源必须完整：

| 唤醒源 | 消费者 | 条件 |
|---|---|---|
| RX completion | queue task | 有 descriptor 完成或 error |
| TX completion | queue task | ring 从满变为可用或需要回收 |
| link/error/reset | control/queue task | 状态或 generation 改变 |
| RX packet slot 可读 | stack runner | 从空变为非空 |
| TX packet slot 可写 | stack runner | 从满变为非满 |
| smoltcp `poll_at` | stack runner | timer deadline 到达 |
| socket write/config | stack runner | 新 TX 数据、地址或路由变化 |
| smoltcp socket waker | socket event bridge | read/write/error readiness 改变 |

所有等待路径都要遵守“检查→注册→复查”。IRQ rearm 还需要：

```text
drain work
  -> 记录观察到的 event generation
  -> 注册 waker
  -> recheck descriptor/cause/generation
  -> 无工作才 unmask
  -> 再次检查
```

budget 耗尽但仍有工作时，queue/stack task 自行重排，不立即打开中断。持续流量下禁止“处理到完全为空”的无界循环。初始 budget 应可配置，并记录每次处理量、budget exhaustion 和 reschedule 次数。

reset 必须增加 generation。旧 generation 的 completion、packet token 和 waker 只能丢弃或返回明确错误，不能回收到新 ring。

## QEMU 开发策略

QEMU 第一阶段应显式选择 VirtIO PCI，而不是沿用根 Makefile 的 MMIO 默认值：

- `Makefile:12` 导出 `BUS=mmio`，覆盖子 Makefile 的 PCI fallback。
- `make/Makefile:67` 的独立 fallback 是 `BUS=pci`。
- `make/qemu.mk` 在 PCI 下使用 `virtio-net-pci`，MMIO 下使用 `virtio-net-device`。
- PCI probe 会把 IRQ 号传给 `VirtIoNetDev`。
- MMIO probe 当前传入 `None`，`EthernetDevice::register_waker` 因此不会注册 IRQ waker。

根 `qemu` feature 同时包含 `bus-pci`。选择 `BUS=mmio` 时，build script 会转到 MMIO probe，但当前并没有补齐设备 IRQ。MMIO 应作为 PCI MVP 后的独立 parity Gate，不能和首个异步 IRQ 分片混做。

当前 `axtask::register_irq_waker` 还有三项限制：

1. `axhal::register_irq_hook` 是全局单次注册接口。
2. QEMU PLIC 先调用 IRQ handler table、完成 PLIC，再调用全局 hook。
3. hook 只唤醒 waiter，不读取设备 cause，也不屏蔽设备 queue interrupt。

因此它可以用于“IRQ 到过”的证据，但不足以独立实现无丢失的 device mask/rearm。正式 VirtIO 路径应注册明确的设备 IRQ handler，或扩展上游 IRQ API，避免多个子系统竞争全局 hook。

QEMU 网络环境分层：

| 环境 | 用途 | 证据 |
|---|---|---|
| user networking | 快速 TCP/UDP 出站和基础回归 | socket 功能 |
| TAP | 主机主动入站、双向压力、可控丢包/延迟 | ingress、backpressure、并发 |
| `NET_DUMP=y` | 生成 `netdump.pcap` | 包序、重传、重复和静默丢包 |
| QEMU IRQ/driver counters | IRQ、completion、budget、wake | 控制流证据 |

每次 QEMU 启动应打印并固定：bus 类型、PCI device ID、IRQ、MAC、queue size、negotiated VirtIO features、RX/TX budget。不要只以 `ping` 成功作为 Gate。

QEMU 可证明：

- VirtIO descriptor/completion 的软件所有权。
- IRQ→task→stack→socket 的控制流。
- register-recheck、背压、timeout/cancel/reset generation。
- 单核和 SMP 下的软件同步。

QEMU 不能证明：

- VisionFive2 clock/reset/PHY 配置。
- DWMAC descriptor 与 JH7110 DMA 地址事实。
- 非一致 cache 的 clean/invalidate。
- 真板 PLIC 路由、电平中断和设备 status/ack 顺序。
- 真板吞吐、p99 和长时间稳定性。

## VisionFive2 过渡策略

当前 VF2 还不是“替换一个网卡驱动”：

- 根 `vf2` feature 没有启用 `axfeat/net-ng` 或 kernel VF2 feature。
- `kernel/Cargo.toml` 没有 VF2 feature。
- `kernel/src/platform/mod.rs::descriptor()` 不会选择现有 VF2 描述符。
- 设备初始化仍发生在 axruntime，DWMAC 注入点尚未建立。

因此真板顺序必须先于 DWMAC 数据面：

| Gate | 目标 | 必须保留的证据 |
|---|---|---|
| B0 | VF2 feature 和 kernel descriptor 一致 | build feature、平台名、内存和 PLIC facts |
| B1 | U-Boot handoff 后稳定启动 | 第一字节、panic、重复启动日志 |
| B2 | DWMAC 寄存器可访问 | ID/version、MAC/DMA status，不是全 0/全 1 |
| B3 | clock/reset/PHY 事实可观测 | U-Boot 状态、link、MDIO/PHY ID |
| B4 | PLIC 中断重复到达 | claim、device cause、handler、ack/EOI 计数 |
| B5 | RX/TX descriptor 移动 | head/tail、owner、DMA bus address、cache 操作 |
| B6 | ARP/ICMP/UDP/TCP | 真板包抓取和 socket 回归 |
| B7 | 压力、reset、SMP | drop、stall、p99、generation、长时间运行 |

ArceOS DWMAC 可以提供 JH7110 PAC、PHY、寄存器和 DMA 地址分离的线索，但当前样例有硬编码 IRQ、默认 MAC、简化 cache 操作，并在 ISR 内调用完整 socket poll。只能借平台事实和验证方法，不能复制为生产实现。

真板 DMA 层必须区分 CPU address 和 bus address，并明确 coherent/streaming allocation、clean/invalidate、DMA barrier 和 descriptor owner bit 的发布顺序。Rust `Acquire/Release` 不能替代设备 memory barrier。

## 可执行分片与 Gate

以下分片每次只改变一个主要边界。每片失败时应停在该片修复，不把未解释问题带到真板。

| 分片 | 代码目标 | 测试目标 | Gate |
|---|---|---|---|
| N0-A | 将 smoltcp 0.13.1 纳入 workspace；本地化 axnet；消除 `preprocess` 依赖 | 现有 TCP listen/accept、UDP、poll 回归 | 同步行为与基线一致 |
| N0-B | 加 counters 和 QEMU PCI IRQ witness | IRQ、RX、TX completion 可重复；pcap 对齐 | 无 busy loop；IRQ 号和 cause 可解释 |
| N1-A | 引入 queue control、AtomicWaker、唯一 owner task | ingress-before-register、register-during-event、spurious IRQ | 无 lost wakeup；ISR 有界 |
| N1-B | bounded RX/TX packet slot 和 stack device | ring full、slot full、partial write、drop | 背压可见；内存有上界 |
| N2-A | 独立 stack runner，使用细粒度 smoltcp poll | device/software/timer 三路 wake | 空闲不轮询；持续流量不饿死 |
| N2-B | smoltcp 单槽 waker 到 `axpoll::PollSet` bridge | 多 waiter、overflow、close/error | poll/select readiness 与实际 I/O 一致 |
| N3 | reset generation、cancel、timeout、fault injection | late completion、link flap、queue stall | 无 UAF、重复回收或永久 Pending |
| N3-MMIO | 给 VirtIO MMIO 补 IRQ facts 和 parity | PCI/MMIO 同一功能集 | MMIO 不依赖 socket 主动 poll |
| N3-SMP | queue affinity 和跨 hart wake | 双 hart 压力、控制面并发 | 无 race；ordering 证据完整 |
| N4-A | VF2 平台、feature、descriptor、启动链 | B0-B4 | 可重复中断证据 |
| N4-B | DWMAC 最小 RX/TX | B5-B6 | 真板包与 descriptor 证据一致 |
| N4-C | 真板压力和恢复 | B7 | 稳定性和性能报告可复现 |
| N5 | descriptor token、batch、多队列优化 | 与 N4 基线 A/B 对比 | 数据证明收益且不退化正确性 |

N0-A 是前置兼容分片，不应与异步重构同一个 commit。N0-B 先建立观测基线。N1 可以先保留 packet copy，N5 再优化所有权传递。

建议的测试集合：

| 层级 | 用例 |
|---|---|
| host/model | ring ownership、event generation、register-recheck、budget、listener backlog |
| QEMU functional | ARP/ICMP、UDP、TCP client/server、nonblocking、poll/select |
| QEMU race | event before register、event during register、spurious IRQ、budget exhausted |
| QEMU pressure | RX flood、TX ring full、socket 慢消费者、并发 close |
| fault | link flap、queue reset、late completion、timeout/cancel、packet corruption/drop |
| SMP | IRQ hart 与 owner task 分离、并发 socket、reset 与 I/O 交错 |
| board | register/IRQ/descriptor/cache/packet 分层证据、长时间 soak |

最低观测项：

```text
irq_total / irq_spurious / irq_masked
rx_completion / rx_drop / rx_refill_fail
tx_submit / tx_completion / tx_ring_full
queue_budget_exhausted / stack_budget_exhausted
wake_sent / wake_consumed / register_recheck_hit
slot_rx_occupancy / slot_tx_occupancy
socket_waiter_overflow
reset_generation / stale_completion
stack_poll_reason / stack_poll_duration
```

## 边界与失败路径

| 风险 | 触发方式 | 处理 |
|---|---|---|
| smoltcp 替换破坏 listen | 直接 patch `starry-smoltcp` 为上游 | 先做 N0-A 和 TCP server 回归 |
| IRQ 到达但 cause 未清 | 只使用全局 IRQ hook | 增加设备 handler 和 cause/mask/rearm |
| MMIO 永久 Pending | `irq_num=None` 且去掉主动 poll | N3-MMIO 前保留明确兼容路径 |
| smoltcp waker 覆盖 | 多 waiter 注册同一 socket 单槽 | 单槽→PollSet bridge |
| PollSet 超过 64 | 高并发 waiter | overflow 计数、提前 wake、后续可换动态结构 |
| stack runner 饥饿其他任务 | 无界 `Interface::poll` | 用 `poll_ingress_single` 和 budget |
| packet slot 隐藏背压 | 无界 channel 或静默 drop | 固定容量、occupancy、drop reason |
| reset 后旧 token 回收 | 无 generation | token/completion 携带 generation |
| QEMU 成功误判真板完成 | 直接进入性能结论 | B0-B7 分层取证 |
| 复制 ArceOS 样例缺陷 | 硬编码 IRQ/cache no-op/ISR poll | 只借平台事实，重新实现 ownership |
| Embassy 版本联动 | 直接引入 driver-channel 0.4.0 | 本地 slot channel，避免顺带升级 sync |

## 关键文件

StarryOS：

- `Cargo.toml`
- `make/Makefile`
- `make/features.mk`
- `make/qemu.mk`
- `kernel/src/file/net.rs`
- `kernel/src/drivers/os_arceos.rs`
- `kernel/src/platform/mod.rs`
- `kernel/src/platform/visionfive2.rs`

本地 smoltcp：

- `crates/smoltcp/Cargo.toml`
- `crates/smoltcp/src/phy/mod.rs`
- `crates/smoltcp/src/iface/interface/mod.rs`
- `crates/smoltcp/src/socket/tcp.rs`
- `crates/smoltcp/src/socket/udp.rs`

本地 Embassy：

- `/home/daivy/projects/serial/work/embassy/embassy-net-driver/src/lib.rs`
- `/home/daivy/projects/serial/work/embassy/embassy-net-driver-channel/src/lib.rs`
- `/home/daivy/projects/serial/work/embassy/embassy-net/src/driver_util.rs`
- `/home/daivy/projects/serial/work/embassy/embassy-net/src/lib.rs`

当前 registry 依赖：

- `axnet-ng-0.3.0-preview.2/src/lib.rs`
- `axnet-ng-0.3.0-preview.2/src/service.rs`
- `axnet-ng-0.3.0-preview.2/src/router.rs`
- `axnet-ng-0.3.0-preview.2/src/listen_table.rs`
- `axnet-ng-0.3.0-preview.2/src/device/ethernet.rs`
- `axdriver_net-0.1.4-preview.3/src/lib.rs`
- `axdriver_virtio-0.1.4-preview.3/src/net.rs`
- `virtio-drivers-0.7.5/src/device/net/dev_raw.rs`
- `axtask-0.3.0-preview.2/src/future/poll.rs`
- `axpoll-0.1.2/src/lib.rs`
- `axhal-0.3.0-preview.2/src/irq.rs`
- `axplat-riscv64-qemu-virt-0.3.1-pre.6/src/irq.rs`

本地 ArceOS 参考：

- `/home/daivy/projects/serial/work/arceos/modules/axdriver/src/lib.rs`
- `/home/daivy/projects/serial/work/arceos/modules/axnet/src/lib.rs`
- `/home/daivy/projects/serial/work/arceos/axdriver_crates/axdriver_net/src/dwmac/mod.rs`

下一步若进入实施，应先为 N0-A 建立独立 OpenSpec change。该 change 只处理本地 smoltcp、axnet listen 兼容和同步回归，不同时引入 IRQ/queue task。
