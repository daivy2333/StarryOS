# StarryOS 网络开发 — 待收集信息清单

> Project: StarryOS
> Branch: net-k3
> Created: 2026-07-25
> Status: 活跃 — 条目在对应 milestone Plan 前应全部解决；仅管理未决问题，不复制 tasks，不提前做实现决策

本文汇总 T01-T13 每个阶段尚未采集的事实、待确认的边界和缺少的测试见证。条目按 milestone 分组，每个条目必须在对应 change 获批前解决（状态从 open → resolved），并在 `结果落点` 字段指明最终写入位置。

---

## 编号规则

`G{两位数序号}`，按 milestone 分组，不连续编号。状态：`open` / `in-progress` / `resolved` / `deferred`。

---

## G01-G05: T01 — smoltcp/axnet 兼容矩阵

<!-- T01 知识包 -->

### G01 — smoltcp 0.12 fork → 0.13.1 完整 API 差异

- **Milestone**: T01
- **待回答问题**: smoltcp 0.12 fork（`starry-smoltcp 0.12.1-preview.1`）与本地 0.13.1 之间，除了 `RxToken::preprocess` 之外还有哪些 API 差异？`phy::Device` trait、socket API、`Interface` 构造函数签名的变化有哪些？
- **已知事实**:
  - fork 在 `phy::mod.rs:387` 增加了 `RxToken::preprocess(SocketSet)`
  - fork 在 `iface/interface/mod.rs:584` 在 ingress 前调用 preprocess
  - 本地 smoltcp 0.13.1 的 no_std + alloc feature 组合离线检查通过
  - 本地 smoltcp 提供 `poll_ingress_single`、`poll_egress`、`poll_maintenance`、`next_poll_at`
- **要读取的代码**:
  - `crates/smoltcp/` 与 registry `starry-smoltcp-0.12.1-preview.1` 的 diff（全量 API 对比）
  - `crates/smoltcp/src/phy/mod.rs`（trait 定义）
  - `crates/smoltcp/src/socket/tcp.rs`、`udp.rs`、`raw.rs`（socket API）
  - registry `axnet-ng/src/router.rs`、`listen_table.rs`（`preprocess` 调用点）
- **所需测试/日志**: 编译通过、现有 TCP/UDP 测试全绿
- **解决判据**: diff 清单完整（每个差异有文件:行号、变更类型、影响范围）；无遗漏的编译不兼容项
- **状态**: open
- **结果落点**: `.claude/analysis/` 专题分析 或 T01 change Evidence

### G02 — listener/backlog 方案选型

- **Milestone**: T01
- **待回答问题**: 选择哪种方案消除 `RxToken::preprocess` 依赖？
  - 方案 A：把 `preprocess` 补回本地 smoltcp（短期恢复编译，长期维护 fork）
  - 方案 B：在 axnet 层重做 listen/backlog（保持上游 smoltcp 干净）
  - 方案 B 的具体实现：预置 backlog 数量 listening socket + 按需补充，还是 axnet ingress adapter 先检查 TCP SYN 再交给 Interface？
- **已知事实**:
  - `preprocess` 的作用是 SYN 进入协议栈前动态创建 listening socket（`listen_table.rs:126`）
  - 上游 smoltcp 有 `TcpSocket::listen()` 可预置 listening socket
  - 决定（D20）要求保留 axnet-ng、smoltcp、axpoll、axtask
- **要读取的代码**:
  - 上游 smoltcp `TcpSocket::listen()`、`accept()` 语义
  - registry `axnet-ng/src/listen_table.rs`（当前 SYN→listen socket 逻辑）
  - `kernel/src/file/net.rs`（VFS socket 到 axnet 的边界）
- **所需测试/日志**: TCP listen/accept 回归、并发 accept、close 时 listening socket 清理、backlog 满时 SYN 行为
- **解决判据**: 选定方案并在 backlog/close/并发 accept 三个场景下 behavior 与当前基线一致
- **状态**: open
- **结果落点**: T01 change 内的 decision（Dxx）或 change Evidence

### G03 — axnet、axdriver、axruntime 本地化清单

- **Milestone**: T01
- **待回答问题**: 本地化 axnet-ng 需要修改哪些文件？`axdriver_net::NetDriverOps`（同步兼容面）保持不变还是需要适配？`axruntime` 的 `init_drivers()` → `init_network()` 注入点如何本地化？
- **已知事实**:
  - `axnet-ng = 0.3.0-preview.2` 当前在 registry，使用 `RxToken::preprocess`
  - `axruntime` 在 kernel entry 前完成 driver probe 和 `init_network()`（`Cargo.toml:34`）
  - `kernel/src/drivers/os_arceos.rs:25` 已有 `axtask::spawn_with_name + block_on`
  - `axdriver_net` 作为同步兼容面保留，不作为最终异步硬件 contract（决策 D20）
- **要读取的代码**:
  - registry `axnet-ng/src/lib.rs`、`service.rs`、`router.rs`、`listen_table.rs`、`device/ethernet.rs`
  - `Cargo.toml`（workspace members、dependencies、features）
  - `make/features.mk`（feature 选择和传递）
- **所需测试/日志**: 编译通过；本地化后同步网络行为与基线一致
- **解决判据**: 完整的本地化文件清单（新建/修改/保留）；每项有明确的本地化边界和原因
- **状态**: open
- **结果落点**: T01 change proposal 或 change Evidence（文件变更清单）

### G04 — 同步网络基线 payload 和指标

- **Milestone**: T01
- **待回答问题**: 当前同步网络（`starry-smoltcp 0.12.1-preview.1` + registry `axnet-ng`）在 QEMU PCI 下的 TCP/UDP/poll 行为基线是什么？吞吐、IRQ 数、CPU proxy、drop 等指标值？
- **已知事实**:
  - QEMU feature 固定包含 PCI bus（`Cargo.toml:53`）
  - 根 `BUS=mmio` 覆盖子 Makefile 的 PCI fallback（`Makefile:12`）
  - 尚无系统性的同步网络基线数据
- **要读取的代码**:
  - `make/qemu.mk`（QEMU 启动参数）
  - `make/Makefile`（BUS 选择逻辑）
- **所需测试/日志**: TCP ping、UDP echo、TCP listen/accept、poll/select 功能回归；IRQ 计数、吞吐量（Mbps）、CPU busy 比例
- **解决判据**: 同步基线 payload（脚本+输出）可复现；指标数据在本地化后不退化
- **状态**: open
- **结果落点**: T01 change Evidence

### G05 — 本地 smoltcp workspace 集成方案

- **Milestone**: T01
- **待回答问题**: `crates/smoltcp/` 如何纳入 workspace？作为 member、exclude + path dependency 还是 Cargo patch？`edition 2024`、`rust-version 1.91` 与项目 nightly-2026-02-25 的兼容性是否已验证？
- **已知事实**:
  - 本地 smoltcp 是 0.13.1、edition 2024、`rust-version = 1.91`
  - 项目工具链 nightly-2026-02-25（rustc 1.95.0）
  - no_std + alloc feature 组合离线检查通过
  - 直接放入 workspace 目录会触发 "not a member/exclude" 的 Cargo 边界问题
- **要读取的代码**:
  - 根 `Cargo.toml`（workspace members/exclude）
  - `crates/smoltcp/Cargo.toml`（package 元数据）
- **所需测试/日志**: `cargo check` 通过；smoltcp 作为 workspace member 正常编译
- **解决判据**: 选定集成方案（member / patch / path dep）并编译通过；记录选择原因
- **状态**: open
- **结果落点**: T01 change Evidence

---

## G10-G15: T02 — QEMU VirtIO PCI 基线

<!-- T02 知识包 -->

### G10 — QEMU PCI 固定命令和设备参数

- **Milestone**: T02
- **待回答问题**: T02 的固定 QEMU 启动命令是什么？VirtIO PCI 的 device ID、vendor ID、IRQ 号、MAC 地址、queue size、negotiated features 的期望值？
- **已知事实**:
  - `make/qemu.mk` 在 PCI 下使用 `virtio-net-pci`
  - PCI probe 会传入 IRQ 号（区别于 MMIO 的 `None`）
  - 根 Makefile 当前默认 `BUS=mmio`
- **要读取的代码**:
  - `make/qemu.mk`（QEMU 参数生成）
  - `make/Makefile`（BUS 选择逻辑）
  - QEMU VirtIO PCI 设备文档（virtio-net-pci 参数）
- **所需测试/日志**: QEMU 启动日志（bus 类型、device ID、IRQ、MAC、queue size、features）；重复启动结果一致
- **解决判据**: 固定启动命令、设备参数和期望输出文本化；任何人可复现
- **状态**: open
- **结果落点**: T02 change Evidence 或 Runbook

### G11 — PCI IRQ 注册路径和 handler 注入点

- **Milestone**: T02
- **待回答问题**: VirtIO PCI 的 IRQ handler 如何注册？是走 `axhal::register_irq_hook`（全局单次）还是注册设备专用 handler？`axtask::register_irq_waker` 的全局 hook 与 VirtIO 设备 handler 如何共存？
- **已知事实**:
  - `axhal::register_irq_hook` 是全局单次注册
  - QEMU PLIC 先调用 IRQ handler table、完成 PLIC，再调用全局 hook
  - hook 只唤醒 waiter，不读取 device cause、不屏蔽 device queue interrupt
  - `axtask::register_irq_waker` 的 `PollSet` 依赖全局 hook
  - `VirtIoNetDev` 隐藏了 `enable_interrupts/disable_interrupts`
- **要读取的代码**:
  - registry `axhal-0.3.0-preview.2/src/irq.rs`（IRQ hook 机制）
  - registry `axtask-0.3.0-preview.2/src/future/poll.rs`（全局 hook 和 PollSet）
  - registry `axdriver_virtio-0.1.4-preview.3/src/net.rs`（VirtIO net wrapper）
  - registry `virtio-drivers-0.7.5/src/device/net/dev_raw.rs`（底层 interrupt enable/disable）
- **所需测试/日志**: IRQ 到达时 cause 可读；重复触发可复现；handler 路径不与其他子系统冲突
- **解决判据**: 确定的 IRQ 注册方案（全局 hook 扩展 / 设备专用 handler）；重复触发和 cause 读取均可复现
- **状态**: open
- **结果落点**: T02 change Evidence

### G12 — pcap 测试环境和用例

- **Milestone**: T02
- **待回答问题**: T02 的 pcap 测试环境（`NET_DUMP=y` 的 `netdump.pcap`）需要哪些用例来验证包序、重传、重复和静默丢包？user networking 和 TAP 两种环境各用于什么测试？
- **已知事实**:
  - `NET_DUMP=y` 可生成 pcap 文件
  - user networking 适合快速 TCP/UDP 出站和基础回归
  - TAP 适合主机主动入站、双向压力和可控丢包/延迟
- **要读取的代码**:
  - QEMU `netdump.pcap` 生成机制
- **所需测试/日志**: ARP、ICMP echo、TCP handshake、UDP send/recv 的 pcap 记录；包序和内容与预期一致
- **解决判据**: 测试用例清单 + pcap 证据；user networking 和 TAP 两种环境均可复现
- **状态**: open
- **结果落点**: T02 change Evidence

### G13 — 同步吞吐基线建立方法

- **Milestone**: T02
- **待回答问题**: 同步网络（T01 完成后的基线）在 QEMU PCI 下的吞吐、IRQ 数、CPU proxy、drop 等指标的测量方法和基线值？
- **已知事实**:
  - 尚无测量方法和基线数据
  - 最低观测项列表已定义（`starryos-network-development-strategy.md:378`）
- **要读取的代码**: 无 — 需设计 payload 和测量脚本
- **所需测试/日志**: TCP/UDP 吞吐（Mbps）、IRQ 总数/假 IRQ/被屏蔽 IRQ、CPU idle比例、RX drop、TX ring full 次数
- **解决判据**: 测量方法文本化 + 基线数值可复现
- **状态**: open
- **结果落点**: T02 change Evidence

### G14 — VirtIO feature negotiation 基线

- **Milestone**: T02
- **待回答问题**: T02 阶段需要显式 negotiated 哪些 VirtIO features？legacy vs modern 设备模式选哪个？feature negotiation 的当前默认行为是什么？
- **已知事实**:
  - `virtio-drivers 0.7.5` 处理 transport 和 virtqueue
  - 当前未显式固定 feature negotiation 结果
- **要读取的代码**:
  - registry `virtio-drivers-0.7.5/src/device/net/`（feature bit 定义和处理）
  - registry `axdriver_virtio-0.1.4-preview.3/src/net.rs`（wrapper 的 feature 传递）
- **所需测试/日志**: 启动日志中打印 negotiated features 列表；期望 features 全部到位
- **解决判据**: 固定 expected features 清单 + 启动后验证
- **状态**: open
- **结果落点**: T02 change Evidence

### G15 — QEMU idle CPU proxy 基线

- **Milestone**: T02
- **待回答问题**: 同步网络下 QEMU CPU idle 比例是多少？此基线用于证明后续异步化（T03-T06）后 idle 比例不退化。
- **已知事实**:
  - T02 之后引入 queue task 和 stack runner，idle 比例可能变化
  - 当前无 idle CPU proxy 基线
- **要读取的代码**: 无 — 需 QEMU monitor 或 host 侧 CPU 监控
- **所需测试/日志**: 网络空闲时 QEMU 进程 CPU 使用率；轻载/满载下的 CPU 使用率
- **解决判据**: idle CPU proxy 基线数值可复现；后续 T03-T06 不显著退化
- **状态**: open
- **结果落点**: T02 change Evidence

---

## G20-G26: T03-T06 — 异步接口契约

<!-- T03-T06 知识包；可在 T01/T02 代码证据后再做 -->

### G20 — `NetQueueControl` 和 `StackNetDevice` 的方法签名与所有权

- **Milestone**: T03/T04
- **待回答问题**: `NetQueueControl`（硬件/OS contract）需要的完整方法签名？`StackNetDevice`（协议栈 contract）的方法签名？两者的所有权边界在哪里？
- **已知事实**:
  - 推荐两层分离：`NetQueueControl`（IRQ cause/mask/ack/rearm、completion/refill/reclaim、reset generation、link/error、DMA ownership）和 `StackNetDevice`（poll_receive/poll_transmit/poll_link_state、packet token、capabilities）
  - `StackNetDevice` 可对齐 Embassy driver trait 语义
  - M36 要求 ISR → queue task → stack runner → socket readiness 四层分离
- **要读取的代码**:
  - `/home/daivy/projects/serial/work/embassy/embassy-net-driver/src/lib.rs`（Driver trait）
  - `/home/daivy/projects/serial/work/embassy/embassy-net-driver-channel/src/lib.rs`（runner/device 分离）
- **所需测试/日志**: 编译通过；trait 边界可单元测试
- **解决判据**: 两个 trait 的完整方法签名和文档；所有权转移规则明确
- **状态**: open
- **结果落点**: T03/T04 change proposal 或 project-model (Mxx)

### G21 — queue task 启动/停止/reset lifecycle

- **Milestone**: T03
- **待回答问题**: queue task 的完整生命周期：谁创建？谁启动？停止和 reset 时的 in-flight descriptor 和 packet slot 如何处理？与 `NetQueueControl` 的 reset generation 如何联动？
- **已知事实**:
  - 每个 queue 由一个 task 唯一推进
  - reset 必须递增 generation，旧 completion 和 token 不进入新 ring
  - `kernel/src/drivers/os_arceos.rs:25` 已有 task spawn 基础设施
- **要读取的代码**:
  - registry `axtask-0.3.0-preview.2/src/`（task spawn/lifecycle）
  - Embassy `embassy-net-driver-channel` 的 runner lifecycle
- **所需测试/日志**: start→stop→restart 循环；stop 时 in-flight 完成或失败；reset 后旧 descriptor 被正确拒绝
- **解决判据**: lifecycle 状态机文档化 + 每个转换有测试见证
- **状态**: open
- **结果落点**: T03 change Evidence 或 knowledge (Kxx)

### G22 — RX/TX/link/error wake source 清单

- **Milestone**: T04/T05
- **待回答问题**: 所有唤醒源的完整清单（RX completion、TX completion、link/error/reset、RX slot readable、TX slot writable、smoltcp poll_at timer、socket write/config、smoltcp socket waker）中的每一项：谁产生、谁消费、什么条件触发？
- **已知事实**:
  - 唤醒源清单已枚举（`starryos-network-development-strategy.md:243`）
  - 8 类唤醒源：RX completion、TX completion、link/error/reset、RX slot readable、TX slot writable、poll_at timer、socket write/config、socket waker
- **要读取的代码**: 无 — 需设计阶段确认每项的触发条件和消费逻辑
- **所需测试/日志**: 每类唤醒源的独立触发和消费可复现；无 lost wakeup
- **解决判据**: 8 类唤醒源的完整规格（触发条件、消费逻辑、race 处理）
- **状态**: open
- **结果落点**: T04/T05 change Evidence

### G23 — smoltcp 单槽 waker 到 axpoll 事件的映射

- **Milestone**: T06
- **待回答问题**: smoltcp socket 的单槽 `WakerRegistration` 如何桥接到 `axpoll::PollSet`（容量 64）？POLLIN/POLLOUT/POLLERR/POLLHUP 分别对应 smoltcp 的哪些 readiness 状态？overflow（超过 64 waiter）时的语义是什么？
- **已知事实**:
  - 本地 smoltcp 的 `async` feature 只给 socket 增加单个 `WakerRegistration`，不暴露多个 OS waiter
  - `axpoll 0.1.2` 的 `PollSet` 容量 64，`wake()` 唤醒并清空
  - 容量溢出时会覆盖 slot，可能提前唤醒旧 waiter
  - smoltcp 单槽 waker 被多个 VFS waiter 注册时会被覆盖
- **要读取的代码**:
  - `crates/smoltcp/src/socket/tcp.rs`（`WakerRegistration` 和 async methods）
  - registry `axpoll-0.1.2/src/lib.rs`（`PollSet` 实现）
- **所需测试/日志**: 1/2/64/65 waiter 并发 poll；overflow 时旧 waiter 被提前唤醒的计数；close/error 时所有 waiter 正确唤醒
- **解决判据**: 映射表完整（smoltcp readiness → axpoll event）；overflow 语义明确且有测试见证
- **状态**: open
- **结果落点**: T06 change Evidence 或 knowledge (Kxx)

### G24 — packet slot 容量和背压测试

- **Milestone**: T04
- **待回答问题**: 首版 packet slot 的容量上界是多少？满时的行为（阻塞 / 非阻塞 / drop）？drop reason 如何记录？背压如何从 slot 传递到 queue task → IRQ rearm？
- **已知事实**:
  - 首版使用有界 packet slot，接受可测量的复制
  - slot 满时背压必须可见
  - 内存必须有上界
  - `embassy-net-driver-channel` 提供有界 RX/TX slot 参考
- **要读取的代码**:
  - `/home/daivy/projects/serial/work/embassy/embassy-net-driver-channel/src/lib.rs`（有界 slot 实现）
- **所需测试/日志**: slot 满时发送者行为（阻塞/返回错误）；drop reason 计数；背压传递链（slot→queue→IRQ）可观测
- **解决判据**: slot 容量确定；满/空/部分写/drop 四个场景有测试见证
- **状态**: open
- **结果落点**: T04 change Evidence

### G25 — stack runner 多唤醒源合流

- **Milestone**: T05
- **待回答问题**: stack runner 如何同时等待 device wake（queue task 通知）、software wake（socket write/config）和 timer wake（smoltcp poll_at）？三路唤醒合流的实现方式？
- **已知事实**:
  - `embassy-net` 的 runner 模式已有 device + software + timer 合流参考
  - stack runner 是 smoltcp Interface 和 SocketSet 的唯一修改者
  - runner 使用 `poll_ingress_single`、`poll_egress`、`poll_maintenance` 细粒度 poll
- **要读取的代码**:
  - `/home/daivy/projects/serial/work/embassy/embassy-net/src/driver_util.rs`（runner 实现）
  - `crates/smoltcp/src/iface/interface/mod.rs`（细粒度 poll 方法）
- **所需测试/日志**: device 唤醒后 ingress 正常推进；timer 到期时 maintenance 正常执行；software 唤醒后 egress 正常发送；空闲时 runner 无 busy loop
- **解决判据**: 三路唤醒合流方案确定；空闲无轮询 + 持续流量不饥饿
- **状态**: open
- **结果落点**: T05 change Evidence

### G26 — embassy-net-driver trait 采用决策

- **Milestone**: T03
- **待回答问题**: 是否直接依赖 `embassy-net-driver` crate（0.2.0）作为 `StackNetDevice` 的基础 trait？还是本地定义语义对齐的 trait？直接依赖会带入什么依赖链？
- **已知事实**:
  - `embassy-net-driver` 本身依赖很轻，可成为长期公共 contract
  - OpenSpec 只批准了 `embassy-sync::AtomicWaker` 的有限采用
  - `embassy-net-driver-channel 0.4.0` 会带入 `embassy-sync 0.8.0`，首版不采用
  - 推荐两层分离：`NetQueueControl`（本地）+ `StackNetDevice`（可对齐 Embassy）
- **要读取的代码**:
  - `/home/daivy/projects/serial/work/embassy/embassy-net-driver/Cargo.toml`（依赖链）
  - `/home/daivy/projects/serial/work/embassy/embassy-net-driver/src/lib.rs`（trait 定义）
- **所需测试/日志**: 无（设计决策，不需要运行时测试）
- **解决判据**: 决策记录（Dxx）明确采用方式、理由和替代方案
- **状态**: open
- **结果落点**: decision (Dxx)

---

## G30-G35: T07-T09 — 恢复语义与平台 parity

<!-- 依赖 T01-T06 代码证据，不适合提前做完 -->

### G30 — reset generation 和 stale completion 丢弃

- **Milestone**: T07
- **待回答问题**: reset generation 的具体实现（递增 generation 的触发条件、generation 的存储位置、如何与 descriptor token 和 completion 关联）？stale completion 的检测和丢弃路径？
- **已知事实**:
  - reset 必须递增 generation
  - 旧 generation 的 completion、packet token 和 waker 只能丢弃或返回明确错误
  - 生命周期缺口已在 ArceOS 分析中识别（probe rollback、quiesce、suspend/remove）
- **要读取的代码**: 无 — T01-T06 代码证据建立后再设计
- **所需测试/日志**: fault injection（late completion、link flap、queue stall）；无 UAF、重复回收、永久 Pending 或静默丢包
- **解决判据**: generation 机制设计文档 + fault injection 测试全绿
- **状态**: open
- **结果落点**: T07 change Evidence

### G31 — MMIO IRQ 来源和平台事实

- **Milestone**: T08
- **待回答问题**: VirtIO MMIO 的设备 IRQ 号从哪里获取？当前 `VirtIoNetDev` 在 MMIO probe 时传入 `irq_num=None`，需要什么平台事实才能补齐？MMIO 的 IRQ handler 和 rearm 协议与 PCI 有何不同？
- **已知事实**:
  - MMIO probe 当前传入 `None`，`EthernetDevice::register_waker` 因此不注册 IRQ waker
  - PCI→MMIO parity 在 T08 独立 Gate
  - MMIO 不应作为首个异步 IRQ 路径
- **要读取的代码**:
  - registry `axdriver_virtio-0.1.4-preview.3/src/net.rs`（MMIO probe 路径）
  - registry `axplat-riscv64-qemu-virt-0.3.1-pre.6/src/irq.rs`（PLIC 和 IRQ 号分配）
  - QEMU virt machine device tree 或 platform bus（MMIO device IRQ 来源）
- **所需测试/日志**: MMIO IRQ 可重复到达；PCI/MMIO 功能集一致
- **解决判据**: MMIO IRQ 来源证据（platform facts）+ 可重复触发测试
- **状态**: open
- **结果落点**: T08 change Evidence

### G32 — SMP queue affinity 和跨 hart wake

- **Milestone**: T09
- **待回答问题**: queue task 的 CPU affinity 如何设置？跨 hart 的 AtomicWaker wake 如何保证正确性？控制面（probe/reset）的同步机制？M39（跨 hart ordering）的具体应用？
- **已知事实**:
  - M39 要求按语义选 Ordering，不按架构分叉
  - QEMU 单 hart 结果不能作为 SMP 证据
  - SMP 验证在 T09，QEMU 多 hart 配置
- **要读取的代码**:
  - registry `axtask-0.3.0-preview.2/src/`（task affinity 支持）
  - `embassy-sync::AtomicWaker`（跨 hart 安全性）
- **所需测试/日志**: 双 hart 压力 + reset/I/O 交错；无 race；ordering 理由文档化
- **解决判据**: affinity 设置 + 跨 hart wake 正确性 + ordering 证据
- **状态**: open
- **结果落点**: T09 change Evidence

### G33 — smoltcp runner 粒度（单接口 vs 多接口）

- **Milestone**: T05（设计决策），T09（多 hart 验证）
- **待回答问题**: smoltcp stack runner 是每个网络接口一个独立任务，还是全局一个任务？多接口时如何避免相互饥饿？
- **已知事实**:
  - 当前按单接口设计；多接口方案未定
  - Embassy 参考中 runner 按接口粒度
- **要读取的代码**: 无 — T01-T04 代码证据建立后再决定
- **所需测试/日志**: 多接口场景下各接口独立推进、互不饥饿
- **解决判据**: 颗粒度决策 + 原因记录
- **状态**: open
- **结果落点**: T05 change proposal 或 decision (Dxx)

---

## G40-G44: T10-T12 — VF2 平台与 DWMAC

<!-- 必须由真板验证；当前只枚举问题，不试图用 QEMU 填补 -->

### G40 — VF2 feature、kernel descriptor 和启动链

- **Milestone**: T10
- **待回答问题**: VF2 的 feature 如何启用（根 `vf2` feature 当前未启用 `axfeat/net-ng` 或 kernel VF2 feature）？kernel descriptor 如何选择 VF2 平台？`kernel/Cargo.toml` 需要什么 feature 修改？
- **已知事实**:
  - 根 `vf2` feature 没有启用 `axfeat/net-ng`
  - `kernel/Cargo.toml` 没有 VF2 feature
  - `kernel/src/platform/mod.rs::descriptor()` 不选择现有 VF2 描述符
  - `kernel/src/platform/visionfive2.rs:24` 描述符存在但未被选择
  - M37 要求 trust-u-boot 保留 PLIC+Clock，init_primary/percpu 分离
- **要读取的代码**:
  - `kernel/src/platform/mod.rs`（平台选择逻辑）
  - `kernel/src/platform/visionfive2.rs`（VF2 描述符）
  - `kernel/Cargo.toml`（feature 定义）
  - `make/features.mk`（feature 传递）
- **所需测试/日志**: B0-B1: build feature 一致、平台名正确、第一字节输出、重复启动
- **解决判据**: B0+B1 Gate 通过（feature + descriptor + 稳定启动）
- **状态**: open
- **结果落点**: T10 change Evidence

### G41 — VF2 clock/reset/PHY 事实

- **Milestone**: T10/T11
- **待回答问题**: VF2 JH7110 的 DWMAC clock、reset、PHY（MDIO/PHY ID）当前状态（U-Boot handoff 后）？哪些由 U-Boot 配置、哪些需要 OS 重设？ArceOS 的 DWMAC 代码提供了哪些可复用的平台事实？
- **已知事实**:
  - M37 要求 trust-u-boot 保留 PLIC+Clock 状态
  - M38 要求 init_primary/percpu 分离
  - ArceOS DWMAC 提供了寄存器、PHY 和 DMA 地址分离的线索
  - I06 中 O64（trust-u-boot PLIC+Clock）和 O66（print_preserved_status）在 VF2 硬件到位时触发
- **要读取的代码**:
  - ArceOS `axdriver_crates/axdriver_net/src/dwmac/`（DWMAC 寄存器、PHY、clock 初始化）
  - JH7110 技术参考手册（clock/reset/PHY 章节）
- **所需测试/日志**: B2-B3: 寄存器可访问（非全 0/全 1）、link 状态、MDIO/PHY ID
- **解决判据**: B2+B3 Gate 通过（寄存器 + PHY 事实）；QEMU 结果不计入
- **状态**: open
- **结果落点**: T10/T11 change Evidence

### G42 — DWMAC DMA 地址、cache 和 barrier

- **Milestone**: T11
- **待回答问题**: DWMAC descriptor 的 DMA bus address 与 CPU virtual address 如何映射？cache 操作（clean/invalidate/coherent allocation）的具体实现？DMA barrier 与 Rust atomic ordering 的关系？
- **已知事实**:
  - QEMU 不能证明非一致 cache 正确性
  - Rust Acquire/Release 不能替代 DMA barrier、cache maintenance 或 posted MMIO write readback
  - ArceOS `axdma` 提供 `DMAInfo { cpu_addr, bus_addr }` 和 `alloc_coherent()`
  - I06 中 O69（DMA 一致性内存抽象）在 N4 DWMAC 真板时触发
- **要读取的代码**:
  - ArceOS `modules/axdma/`（coherent mapping、bus address）
  - StarryOS `axhal` 的 MMU/DMA 支持
- **所需测试/日志**: B5-B6: descriptor head/tail 移动正确、owner bit 状态正确、ARP/ICMP/UDP/TCP 包内容与抓取一致
- **解决判据**: B5+B6 Gate 通过（descriptor + cache + 协议包一致性）
- **状态**: open
- **结果落点**: T11 change Evidence 或 knowledge (Kxx)

### G43 — VF2 PLIC 中断路由和电平中断

- **Milestone**: T10
- **待回答问题**: VF2 JH7110 的 PLIC 中断路由（DWMAC IRQ 到哪个 hart？电平中断的 claim/status/ack/EOI 顺序？与 QEMU PLIC 的差异？
- **已知事实**:
  - QEMU PLIC 行为可能与真板不同
  - I06 中 O65（PLIC 防御性分离验证）在 N3 SMP 或 VF2 平台切换时触发
  - M38 要求分离 primary/per-hart 初始化
- **要读取的代码**:
  - JH7110 技术参考手册（PLIC 章节）
  - ArceOS VisionFive2 bring-up 日志（IRQ 路由证据）
- **所需测试/日志**: B4: PLIC claim、device cause、handler、ack/EOI 可重复
- **解决判据**: B4 Gate 通过（中断重复到达证据）；QEMU 结果不计入
- **状态**: open
- **结果落点**: T10 change Evidence

### G44 — VF2 压力测试和 soak 方案

- **Milestone**: T12
- **待回答问题**: 真板压力测试的具体场景（burst、双向、ring full、link flap、reset、长时间 soak）和通过判据（drop/stall/p99/generation）？
- **已知事实**:
  - B7 Gate 要求原始日志、指标可复现
  - 性能阈值和优化目标必须由基线数据产生（T13 触发条件）
- **要读取的代码**: 无 — T10-T11 证据建立后再设计
- **所需测试/日志**: B7: drop rate、p99 latency、generation 变化、长时间运行 stability
- **解决判据**: B7 Gate 通过（压力和 soak 日志完整可复现）
- **状态**: open
- **结果落点**: T12 change Evidence

---

## G50-G52: T13 — 数据驱动优化

<!-- 必须由指标触发；无数据则 SKIPPED -->

### G50 — 性能基线与优化目标

- **Milestone**: T13
- **待回答问题**: T12 真板压力测试后的性能基线（吞吐、延迟 p99、CPU 利用率、IRQ 率）？优化目标（batching、moderation、zero-copy、multiqueue、offload）每项的触发阈值？
- **已知事实**:
  - T13 每项独立 A/B 对比
  - 无数据则 SKIPPED 并记录原因
  - descriptor-backed token、零拷贝、多队列属于 T13，不作为异步 MVP 前置条件
- **要读取的代码**: 无 — T12 完成后按基线数据决策
- **所需测试/日志**: T12 基线数据 + 每项优化的独立 A/B 测试
- **解决判据**: 有数据 → 逐项 A/B 对比；无数据 → SKIPPED 并记录原因
- **状态**: open
- **结果落点**: T13 change Evidence

### G51 — descriptor token（零拷贝）的 unsafe 边界

- **Milestone**: T13（按需触发）
- **待回答问题**: 从有界 packet slot（复制）切换到 descriptor-backed token（零拷贝）时，unsafe 范围扩大多少？DMA buffer 的生命周期保证？与 cache coherence 的交互？
- **已知事实**:
  - 首版使用有界 packet slot + 可观测复制
  - 过早零拷贝会使 unsafe 范围扩大但收益不明
  - 需 token 化所有权后以指标决定 mmap/zero-copy
- **要读取的代码**: 无 — T13 触发后再设计
- **所需测试/日志**: 性能 A/B 对比 + 正确性不退化
- **解决判据**: 数据证明收益 + unsafe 边界文档化 + 正确性 Gate 通过；无数据则 SKIPPED
- **状态**: open
- **结果落点**: T13 change Evidence

### G52 — multiqueue 和 interrupt moderation

- **Milestone**: T13（按需触发）
- **待回答问题**: 多队列（multiqueue）的 queue-to-hart 映射策略？中断合并（interrupt moderation）的参数（timeout、packet count）和调优方法？与 T09 SMP 验证的关系？
- **已知事实**:
  - T09 SMP 完成后再评估 multiqueue
  - 无性能数据不实施
- **要读取的代码**: 无 — T12 完成后按数据决策
- **所需测试/日志**: 多队列吞吐 vs 单队列；中断合并对延迟的影响；A/B 对比
- **解决判据**: 数据证明收益；无数据则 SKIPPED
- **状态**: open
- **结果落点**: T13 change Evidence

---

## 条目统计

| Milestone | 条目 | 状态 |
|-----------|------|------|
| T01 | G01-G05 (5) | 全部 open |
| T02 | G10-G15 (6) | 全部 open |
| T03-T06 | G20-G26 + G33 (8) | 全部 open |
| T07-T09 | G30-G32 (3) | 全部 open |
| T10-T12 | G40-G44 (5) | 全部 open |
| T13 | G50-G52 (3) | 全部 open |
| **合计** | **30** | **全部 open** |

## 读取和使用

- 新 session 先读 `async-network-project-overview.md`（R23），再读本文档。
- 进入某 milestone 的 Plan 阶段前，将对应条目标记为 `in-progress`，通过 openspec-explorer 逐项调查。
- 调查结果写入 `结果落点`（analysis / change Evidence / decision）。
- 条目标记为 `resolved` 后，不删除——保留为已完成调查的审计轨迹。

## 关联文档

- [R23] `.claude/analysis/async-network-project-overview.md` — 网络开发总览
- [R41] `.claude/analysis/starryos-network-development-strategy.md` — 实施探索
- [R25] `.claude/analysis/arceos-async-network-driver-analysis.md` — ArceOS 网卡分析
- [R42] `.claude/analysis/starryos-network-delivery-estimate.md` — 交付估算
- [tasks] `../docs/tasks.md` — T01-T13 milestones 和 Gate
