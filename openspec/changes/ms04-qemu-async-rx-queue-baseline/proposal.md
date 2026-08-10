## Why

MS03 只证明 VirtIO-MMIO IRQ 7 可以被识别、确认并重复投递，RX descriptor 仍由 10ms 轮询路径推进。MS04 需要在不引入 stack runner、异步 TX 或第二套 executor 的前提下，建立第一条由最小 ISR 唤醒、有 budget 且不会丢失唤醒的 RX queue task 路径。

## What Changes

- 将 VirtIO-net ISR 从纯诊断入口演进为 cause/ack/snapshot/wake 入口；ISR 不读取或回收 descriptor，不进入 axnet `Service` 或 smoltcp。
- 建立 transport-neutral `NetQueueControl`，表达 RX completion、reap/refill、通知抑制与重臂，不向公共接口暴露 VirtIO used ring 或 DWMAC descriptor。
- 使用 `embassy-sync::AtomicWaker` 和 register-recheck 关闭 event-before-register、register-during-event 与 rearm 窗口。
- 修正共享 critical-section 的 IRQ 状态恢复语义，使 ISR 中的 `AtomicWaker::wake()` 不会提前开启中断；保留 UART 回归。
- 保留 `RING_EVENT_IDX`，通过工作区拥有的适配层控制 `used_event` 通知；不修改 Cargo registry，不关闭 feature 规避问题。
- 创建唯一、长驻、单 hart 的 RX queue task，以固定 budget 服务 RX completion；budget 用尽时保持通知抑制、自调度并让出 CPU。
- queue task 通过 axnet RX-only 入口把帧送入现有 Router 有界 RX 缓冲，并在同次服务中回收 descriptor；不创建 MS05 的最终 packet slot，不运行 smoltcp ingress/egress。
- Router 缓冲满时停止继续 reap，待现有协议栈轮询释放空间后由软件事件重新唤醒；不忙等、不丢失 descriptor 所有权。
- 保留同步 TX，包括当前 RX 路径触发的 ARP 等基线发送；不引入异步 TX completion。
- 初始化或 task 启动在所有权切换前失败时保留 MS02 轮询；异步 RX 激活后，轮询路径不得并发消费同一 RX queue。
- 将需要用户操作的 QEMU 手动测试放在 iteration 末尾；所有可在当前环境执行的 host、build、静态检查与 QEMU 准备 Gate 必须先尝试并通过。只有按 R44 可明确归类为 `ENV-BLOCKED` 的项目才可延后到同一手工批次，产品失败不得延后。
- 持久化 QEMU 串口原始日志、probe 输出、环境信息和计数快照，避免只保留结论而缺少运行时证据。

## BDD Scenario Sketch

### Happy Path

- **前置状态**：单一 VirtIO-MMIO NIC 已完成 MS03 IRQ 注册，唯一 RX queue task 已取得数据面所有权。
- **触发动作**：QEMU 向 guest 注入 RX burst。
- **可观察结果**：IRQ 7 handler 只 ack、记录并唤醒；queue task 在 budget 内 reap、送入现有 Router 缓冲并 refill，descriptor 数量守恒。
- **失败边界**：任何 completion 只能被一个 owner 消费；ISR 读取 descriptor、10ms fallback 并发 reap、丢失唤醒、忙等或 descriptor 泄漏均失败。

### Register-Recheck 竞态

- **前置状态**：queue task 正在从空队列转为 `Pending`。
- **触发动作**：事件分别发生在 waker 注册前、注册期间和通知重臂后。
- **可观察结果**：任务通过事件代次、注册、重臂和再次检查观察到每个事件。
- **失败边界**：任务在队列已有 completion 时永久 `Pending`，或依赖无界轮询恢复，均失败。

### Budget 与 Router 满

- **前置状态**：RX completion 数量大于单轮 budget，或现有 Router RX 缓冲已满。
- **触发动作**：queue task 被 IRQ 或软件事件唤醒。
- **可观察结果**：budget 用尽时任务自调度并让出；Router 满时不再 reap，空间释放后继续推进。
- **失败边界**：单轮无界 drain、CPU 饥饿、满缓冲下继续占有未交付 buffer、静默 drop 或 busy loop 均失败。

### Spurious 与 Config-Only IRQ

- **前置状态**：RX queue 没有 completion。
- **触发动作**：handler 收到无 pending cause 或仅 config-change 的 IRQ。
- **可观察结果**：cause 被正确分类和 ack；RX task 至多进行一次无工作检查，不形成循环。
- **失败边界**：伪造 RX completion、修改 descriptor 或持续自唤醒均失败。

### 初始化失败与运行时故障

- **前置状态**：异步 RX 尚未切换 owner，或已经完成切换。
- **触发动作**：IRQ/waker/task 初始化失败，或激活后出现 descriptor/fatal error。
- **可观察结果**：切换前失败保留轮询基线并报告原因；切换后故障进入可观察 fault 状态，不自动创建第二 owner。
- **失败边界**：半切换状态、轮询与 queue task 并发 reap、无界重试或静默恢复均失败。

### 手动 QEMU 验收顺序

- **前置状态**：本 iteration 的 host tests、静态检查和自动竞态测试均已通过；target build 与 QEMU 准备已经在当前环境尝试，结果为通过或按 R44 明确标记 `ENV-BLOCKED`。
- **触发动作**：任务执行到 iteration 最后的用户手动验证批次。
- **可观察结果**：用户先复跑延后的环境阻塞项，再运行已固定的 QEMU 命令或操作步骤；原始构建输出、串口日志、probe 输出和环境信息写入要求的 Evidence。
- **失败边界**：产品 Gate 未通过时不得请求用户手测；无法证明是环境限制的失败不得延后；手测中断或日志缺失不得计为通过。

### Compatibility, Timeout, and Cancellation

- **前置状态**：现有 MS01/MS02 socket、轮询 TX、UART 和 early/panic console 基线可用。
- **触发动作**：启用 MS04 或在有界观察窗口内终止验证。
- **可观察结果**：同步 TX、socket 行为、UART 和 console 不退化；中止的运行见证标记未完成。
- **失败边界**：MS04 不提供 task 注销、热插拔、reset、link flap、跨 hart 或异步 TX；任何相关成功声明均越界。

## Capabilities

### New Capabilities

- `qemu-async-rx-queue-baseline`: 定义 MS04 的 transport-neutral RX queue control、AtomicWaker/register-recheck、`RING_EVENT_IDX` 通知重臂、有界 queue task、临时 Router handoff、故障边界和 QEMU 验收。

### Modified Capabilities

- `qemu-mmio-diagnostic-irq-baseline`: 将 MS03 的诊断-only ISR 和轮询 owner 约束演进为 MS04 的最小 ISR 唤醒与唯一异步 RX owner，同时保留 cause/ack/EOI、UART 隔离和初始化失败回退。

## Impact

- QEMU VirtIO-net IRQ 7 handler、平台 IRQ 到驱动的绑定和诊断计数。
- 本地 axnet 的设备所有权、Router RX-only 服务入口和轮询 capability。
- VirtIO RX queue 的 completion、recycle 与 `EVENT_IDX` 通知控制适配。
- `embassy-sync::AtomicWaker` 使用点和 kernel critical-section IRQ 恢复语义。
- axtask 中唯一 RX queue task 的启动、让出和软件重唤醒。
- MS01/MS02/MS03、UART、host tests、target build 和 QEMU runtime 回归。
- 用户手动测试排序与 change-local Evidence。

## Non-goals

- 异步 TX、TX completion、flush 或 peer delivery 语义。
- 新建最终 RX/TX packet slot、occupancy/drop 契约或用户态共享 ring。
- smoltcp stack runner、socket readiness、多 waiter 或移除协议栈的 10ms 推进机制。
- reset generation、取消、link flap、热插拔或运行时自动恢复。
- SMP、跨 hart wake、multiqueue、RSS、PCI 或真板 transport。
- DWMAC 代码、DMA/cache 真板结论或性能优化。
- 修改全局 tasks、SNAPSHOT、M/D/K/R/I 或归档其他 change。

## Gate 1

- Status: approved.
- 用户于 2026-08-09 接受集中决策的全部默认值。
- 用户新增约束：需要用户手动测试的任务必须集中到 iteration 末尾，并以前置自动 Gate 通过为条件。
- 用户审计 proposal 与 delta specs 后，于 2026-08-09 回复“同意，继续吧”，正式批准 Requirements and Scope。
- 用户于 2026-08-09 补充：“对于这个沙箱问题，你可以记到runbook里面，有类似沙箱问题的，都当作需要我来手动进行的测试，写在qemu需要手测的runbook就好”。R44 已据此增加 `ENV-BLOCKED` 分类；该补充不豁免产品失败，也不允许跳过自动尝试。

## Gate 2

- Status: approved.
- Requirements、调查、设计、任务契约、RTM、验证方法、OpenSpec 一致性与 Persisted Evidence 均为 PASS。
- 用户审计完整计划后，于 2026-08-10 回复“批准”，正式批准 Execution Readiness。
- 本批准授权后续显式调用 `openspec-act` 执行 iteration 000；本次 Plan 不自动进入实施。
