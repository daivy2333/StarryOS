## Why

MS01 需要一个可复现的同步网络栈基线。当前项目使用 registry
`axnet-ng` 和 fork `starry-smoltcp`。后者增加了
`RxToken::preprocess`，用于在 TCP SYN 进入协议栈前创建监听 socket。
仓库内的 smoltcp 0.13.1 没有该接口。

本轮先消除这项私有依赖，并保持同步 socket 行为。IRQ、异步队列和平台
bring-up 由后续 milestone 处理。

## What Changes

- 将仓库内 smoltcp 0.13.1 作为目标协议栈依赖。
- 本地化 `axnet-ng`，移除其对 `RxToken::preprocess` 的依赖。
- 将 fork 中的 TCP bound endpoint 状态迁回 axnet。
- 保持 TCP listen/accept、UDP、nonblocking 和 poll 行为。
- 覆盖 listener backlog、并发连接、close/relisten 和无数据边界。
- 保持当前 QEMU transport 和同步轮询路径，不引入 IRQ 或异步执行层。

## BDD Scenario Sketch

以下范围已通过 Gate 1。

### Happy Path

- 当前支持的 QEMU 配置可以构建，并解析到仓库内 smoltcp 和本地 axnet。
- TCP listener 接受连接后继续补足可用监听能力。
- UDP 可以双向收发，数据和源地址语义不变。
- 数据到达或发送可用时，poll 与实际 socket 操作一致。

### Sad Path

- 无待接连接或数据时，nonblocking 操作返回当前兼容错误，不得永久等待。
- backlog 满时不得破坏 listener 状态；释放容量后可以继续接受连接。
- listener 关闭后不得遗留可被新 listener 错误接收的旧连接状态。
- 依赖切换出现 API 或 feature 不兼容时，构建 Gate 必须失败，不得把
  `RxToken::preprocess` 补回 smoltcp 隐藏问题。

### Edge Case

- 多个连接相邻到达时，每个已接受连接只归属一个 socket。
- close 后重新 bind/listen 时，不复用已释放的 smoltcp handle。
- poll 注册前后 readiness 改变时，结果仍与后续 I/O 一致。
- 当前 QEMU feature 启用的协议能力继续编译；本轮不新增 IPv6 运行验收。

### Error, Timeout, Cancellation, and Compatibility

- 保持现有 socket errno、短 I/O、close 和 poll 兼容语义。
- 本轮不新增 timeout、取消或异步 future 语义。
- 不修改 VirtIO transport、IRQ、DMA、VF2 或 PCI 配置。
- 不设吞吐或延迟目标，也不声明性能改善。

## Capabilities

### New Capabilities

- `network-stack-baseline`: 定义本地 smoltcp/axnet 同步兼容、listener
  生命周期、UDP 和 readiness 的 MS01 基线。

### Modified Capabilities

- None.

## Impact

- 根 Cargo workspace、依赖和 feature 解析。
- 仓库内 `crates/smoltcp`。
- 待本地化的 axnet crate 及其 listener、router、service 和 device adapter。
- 现有 kernel VFS/socket 调用者和 QEMU 同步网络回归入口。

## Non-goals

- IRQ handler、AtomicWaker、queue task、stack runner 或 socket async bridge。
- PCI 兼容、VF2/DWMAC、SMP、零拷贝或性能优化。
- 修改 registry 源码或把 `RxToken::preprocess` 加回 smoltcp。
- 同步全局 tasks、SNAPSHOT 或项目记忆。

## Gate 1

- Status: approved
- Decision: 用户于 2026-07-28 回复“同意”，批准上述场景、兼容范围和
  Non-goals。
