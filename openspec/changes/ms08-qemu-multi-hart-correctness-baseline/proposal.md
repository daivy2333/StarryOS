## Why

StarryOS 在 QEMU 默认路径中同时启动异步 UART 和 VirtIO-MMIO 网络。现有资格只证明了单 hart 下的 UART copier、网络 queue owner、stack runner、readiness 和恢复语义，不能证明跨 hart waker、调度、锁和内存序正确。

当前 kernel critical-section 只屏蔽本 hart IRQ，而 UART 与网络都使用 `AtomicWaker`/`PollSet`。后台任务又在首次入队前没有明确 affinity，远端 ready task 可能等到下一次 timer tick 才运行。如果 MS08 只改网卡，同一套全局原语仍可在 UART ISR、copier 和 TTY caller 之间留下 SMP 竞态，也无法建立 StarryOS 的异步 I/O 多 hart 基线。

## What Changes

- 以 `SMP=16` 作为正式 QEMU runtime 资格规模，对应 K3 AP 域静态的 8 × X100 + 8 × A100；启动时仍从实际 online/schedulable 集合计算 placement，不硬编码 hart ID。
- 将 kernel critical-section 修正为“本地 IRQ restore + 全局 Acquire/Release 互斥 + per-hart 嵌套”，使 UART 和网络共享的 waker 在 SMP 下安全。
- 在工作区自有的 `axtask 0.3.0-preview.2` 中增加入队前 affinity、安全 affinity 更新和 remote-ready IPI；驱动不各自模拟远端调度。
- 以工作区自有的 `axtask::AxCpuMask` 安全封装隔离 `cpumask 0.1.0`：release 构建中的越界读取必须返回“不在集合”，越界写入必须返回错误且保持原 mask，不允许 registry crate 的 debug-only 边界检查泄漏到 scheduler、placement 或迁移控制。
- 将 UART RX/TX copier 从 driver 内部立即普通 spawn 调整为由 kernel adapter 在 secondary-ready 后以入队前 affinity 各启动一次，并保存 task handle。
- 保持 UART RX 和 TX 的 SPSC 身份：RX copier 是唯一 producer，TTY reader 是唯一 consumer；TTY writer 是逻辑唯一 producer，TX copier 是唯一 consumer。迁移只改变执行 hart，不创建第二 endpoint。
- 对 UART 增加 QEMU-only placement 与进度观测，验证 IRQ→RX/TX copier、copier→TTY caller、write/readiness 和 `flush/tcdrain` 的跨 hart wake、Full→恢复和 quiet path。
- 保留网络每个硬件 queue 只有一个逻辑 owner，并继续验证 queue owner、stack runner、readiness、迁移以及 reset/I/O 交错。
- UART 和网络分开建立固定 placement Gate，再运行组合压力；任一驱动的普通运行成功不能替代它自身的跨 hart 证据。
- 保留 early console 独立性和 D1 的有界 TX slow-poll workaround。QEMU NS16550 结果不证明 D1/K3 的 MMIO、clock、IRQ 或真板时序。

### Scenario Sketch

#### Happy Path：共享 SMP 原语

- **前置状态**：至少两个 hart online，目标任务在远端 run queue 上阻塞。
- **触发动作**：ISR 或另一 hart 发布状态并 wake 该任务。
- **可观察结果**：发布状态先于观察，远端 hart 因 IPI 获得调度机会，不依赖周期 timer。
- **失败边界**：丢 wake、重复 IPI storm、全局临界区并发进入或 IRQ 状态错误恢复均失败。

#### Happy Path：UART 固定 placement

- **前置状态**：UART ISR、RX copier、TX copier 和 TTY caller 的实际 hart 可观测，copier 以 singleton affinity 各启动一次。
- **触发动作**：host 向 QEMU serial chardev 注入带边界的 RX payload；guest 并发执行 TX、poll/read 和 `tcdrain`。
- **可观察结果**：字节和顺序完整，short write/readiness 与 ring 真实状态一致，`tcdrain` 只在 ring、copier staged bytes 和硬件 transmitter 全部清空后返回。
- **失败边界**：第二 copier/reader/writer、数据丢失或重复、虚假 readiness、提前 drain、永久 Pending 或忙轮询均失败。

#### Happy Path：网络固定 placement

- **前置状态**：queue owner 和 stack runner 在 secondary-ready 后以 singleton affinity 启动，NIC IRQ 实际执行 hart 可观测。
- **触发动作**：并发执行 TX、RX、poll/select/epoll 和 Full→恢复压力。
- **可观察结果**：唯一 owner 推进 descriptor，stack runner 和应用持续获得进度，slot、descriptor 和 ticket 守恒。
- **失败边界**：第二 owner、重复回收、静默丢包、饥饿或 quiet window 忙轮询均失败。

#### Sad Path：容量、超时与恢复

- **前置状态**：UART ring 或网络 slot/descriptor 达到 Full，或网络处于 reset/link 交错。
- **触发动作**：其他 hart 释放容量、发布 completion，或请求 reset。
- **可观察结果**：等待者由对应事件恢复；UART 保留未接受字节，网络仅由唯一 owner 推进 epoch/recovery。
- **失败边界**：超时必须标记对应 Gate 失败，不得以周期轮询、第二 owner 或无界重试恢复。

#### Edge Case：hart 集合和受控迁移

- **前置状态**：online 集合为 1、2、3、4、8、16 个 hart 或非连续 ID；固定 placement 已通过。
- **触发动作**：计算角色 placement，再将一个现有 copier/owner/runner 的 mask 扩展到两个有效 hart。
- **可观察结果**：单 hart 合法共置，多 hart 尽可能分离角色；迁移后逻辑任务和 SPSC/queue 所有权不变。
- **失败边界**：空、越界、offline 或未初始化 mask 必须 fail closed 并保留旧 placement；不支持 CPU hotplug。

#### Edge Case：CPU mask 越界访问

- **前置状态**：调用方持有 `axtask::AxCpuMask`，并提供等于容量或 `usize::MAX` 的索引。
- **触发动作**：在 debug 或 release 构建中读取或写入该索引。
- **可观察结果**：读取返回“不在集合”；写入返回明确错误且 mask 字节、位数和已设置 bit 不变。
- **失败边界**：不得依赖 `debug_assert`、读取垃圾 bit、改变合法 bit、panic/UB，或让调用方绕过工作区安全封装直接访问 registry mask。

#### Compatibility：单 hart、early console 和真板边界

- **前置状态**：使用单 hart QEMU、D1 特性或 early boot/panic 输出。
- **触发动作**：构建并运行既有 UART/网络回归。
- **可观察结果**：单 hart 语义不变，early console 不依赖 async copier，D1 workaround 保留。
- **失败边界**：QEMU 通过不得被标记为 D1/K3 真板通过；真板硬件事实仍由后续 milestone 重新取证。

## Capabilities

### New Capabilities

- `qemu-multi-hart-async-io-correctness`: 规定 QEMU 多 hart 下共享调度/同步原语、UART 和 VirtIO-MMIO 网络的动态 placement、唯一所有权、跨 hart wake、背压、完成、受控迁移与恢复资格。

### Modified Capabilities

- None.

## Impact

- 影响 kernel critical-section、SMP scheduler/remote wake、QEMU PLIC 映射、UART copier 启动 adapter、网络后台任务启动、QEMU diagnostics、guest probes 和 validator。
- 需要工作区自有的 `axtask` 副本；不修改 Cargo registry，不顺带 vendor 其他 ArceOS crates。
- `cpumask 0.1.0` 继续作为 `axtask` 的私有实现依赖；不 fork、不 patch registry，也不再作为公开 `AxCpuMask` 类型泄漏给调用方。
- 不改变 UART 公开 TTY 语义、网络 socket API、VirtIO descriptor ownership、packet-slot 容量或 MS07 epoch/reset 语义。
- 不包含 CPU hotplug、IRQ 动态负载均衡、multiqueue/RSS、多 NIC、PCI/DWMAC、D1/K3 真板资格或性能优化。

## Gate 1 Approval

- 2026-09-16：用户批准动态 online/schedulable placement，并在读取 K3 静态资料后批准将正式 QEMU runtime 从 `SMP=8` 修订为 `SMP=16`。
- 2026-09-16：用户明确要求将既有异步 UART 与网卡一起纳入多核适配，并以“同意开始重写计划”批准本次范围重规划。
- 2026-09-20：用户明确要求将 `cpumask 0.1` 越界访问风险纳入当前 Cycle 并一并解决，批准 Iteration 003 的范围和验证契约重规划。
- 未豁免 Gate，也未将 QEMU 结果外推为 D1/K3 真板资格。
