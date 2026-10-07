## Purpose

规定 CoM260 Kit 单 hart 上由 GMAC 设备中断驱动的异步双向网络数据面：唯一硬件队列所有者、任务唤醒、有界帧交换、可恢复背压、C4 flush 和稳定错误传播。明确目标板与既有 QEMU 契约的适配边界，并以真实网络交换和直接状态观察验收。

## ADDED Requirements

### Requirement: 最小 IRQ 与最后事件的确定性推进

目标 GMAC IRQ MUST 只执行 cause 分类、清除前有界快照、ack/mask 与 queue wake；descriptor 服务和协议处理 MUST 在任务上下文执行。通知抑制、重臂与重查 MUST 确保最后一次 completion 或软件容量事件即使落在注册/重臂/共享状态占用窗口，也能被观察或触发再次调度，MUST NOT 依赖下一包、下一 IRQ 或固定周期轮询补救。IRQ MUST 只作为工作提示，完成仍以同步后合法 descriptor/status 与 buffer owner 判定；cause 组合、未知和零 cause MUST 可解释且不造成自激风暴。

#### Scenario: 真实 completion 产生可重复设备通知

- **WHEN** 已发布的目标 GMAC RX/TX descriptor 由设备处理并产生中断
- **THEN** MUST 可分别观察真实设备 cause、IRQ claim、任务唤醒与合法完成，ack 后独立事件仍能推进
- **AND** 软件自测或 PHY 事件 MUST NOT 冒充 MAC DMA completion IRQ

#### Scenario: 注册和重臂附近的最后事件

- **WHEN** 最后 completion 或 slot 事件发生在 waker 注册前、期间、重臂后或返回 Pending 前
- **THEN** owner MUST 通过重查观察工作或再次调度并单次消费
- **AND** 无后续流量时 MUST NOT 永久 Pending

#### Scenario: 通知受抑制或共享状态被占用

- **WHEN** 最后 completion 在通知受抑制或任务持有共享状态时到达
- **THEN** 释放/重臂后的路径 MUST 确定性观察并推进该工作
- **AND** ISR MUST NOT 忙等任务锁或把无事件返回当作后续推进保证

#### Scenario: 组合、空和未知 cause

- **WHEN** handler 观察组合 cause、未知 cause 或无 descriptor 工作的提示
- **THEN** MUST 保留已观察 cause 的完整快照并按语义处理，空工作只执行有界重查后休眠
- **AND** MUST NOT 丢弃同次已观察有效位、伪造完成或连续自唤醒

### Requirement: 唯一 owner 与有界公平双向服务

激活后同一 GMAC RX/TX descriptor queue MUST 只由一个常驻 queue owner 推进，协议栈 MUST 只通过固定容量、完整帧原子的 RX/TX slots 与其交换数据。激活 MUST 避免未声明半切换和重复 owner。TX reclaim、RX reap/refill 与 TX submit MUST 有独立固定 budget，backlog 超限 MUST 让出 CPU 并继续调度，双向持续负载 MUST 不饥饿。RX 校验、wrap 和容量耗尽 MUST 保持边界、单次回收与可解释 drop。

#### Scenario: 激活成功、失败和重复启动

- **WHEN** 网络层准备完成后激活，或准备失败，或重复调用启动入口
- **THEN** 成功路径每层 MUST 至多一个推进者，失败路径 MUST 无双向半激活或双重访问，重复调用 MUST 不创建第二 owner
- **AND** 激活后 fatal MUST 保持原 owner 的 fault 边界，不启动 polling fallback

#### Scenario: budget 边界与双向持续工作

- **WHEN** 各阶段工作分别达到 budget−1、budget、budget+1，且两方向持续存在 backlog
- **THEN** 单轮每阶段 MUST 不超过其 budget，剩余工作通过后续调度继续，其他任务和两个方向均能推进
- **AND** exhaustion、处理数与 yield MUST 可观察

#### Scenario: RX slot Full、坏帧与环回绕

- **WHEN** RX 软件容量耗尽、完成帧非法或 ring 索引回绕
- **THEN** MUST 保持未交付对象的明确 owner，合法帧单次交付/refill，非法帧或硬件容量损失有稳定分类
- **AND** 容量恢复 MUST 发布软件事件，MUST NOT 越界、覆盖设备 owner 或动态扩容

### Requirement: 完整 TX、typed 背压与单次 C4 reclaim

TX MUST 以完整帧为单位返回接受、可恢复 Full、带原因的 drop 或稳定 fatal。Full MUST 保留上游 packet；超长 MUST 拒绝，MUST NOT 静默截断。每个已接受 packet MUST 从软件 slot、准备、device-owned、合法 terminal 到 C4 reclaim 保持唯一 owner，并单次回收。C4 MUST 表示 driver buffer 可安全复用，MUST NOT 等同于对端、TCP 或应用交付。

#### Scenario: 合法 TX 与跨环回收

- **WHEN** 有容量接受合法完整帧并完成跨 ring wrap 的多次发送
- **THEN** frame MUST 保持内容完整，每个 descriptor/buffer/接受 ticket 的状态转移和回收只发生一次
- **AND** local C4 与 peer delivery MUST 分别判定

#### Scenario: 软件 slot 或 descriptor Full 后恢复

- **WHEN** 任一级达到容量上限，额外提交后再释放空间
- **THEN** MUST 返回可恢复 Full 并保持原 packet，恢复后经软件事件继续提交，容量与 owner 账本一致
- **AND** MUST NOT 伪成功、覆盖已提交对象或以新分配掩盖压力

#### Scenario: 非法长度与提交/回收失败

- **WHEN** packet 长度不合法，或提交/回收出现错误
- **THEN** 未交给设备的资源 MUST 安全退回可用集合；可能仍被设备访问的资源 MUST 保留并进入稳定错误边界
- **AND** MUST NOT 截断、逐次泄漏容量、重复回收或把 fatal 映射为普通 Full

### Requirement: 有界 ticketed C4 flush 与 waiter 生命周期

内部 flush MUST 捕获调用时已接受 packet 的目标集合，仅在全部目标达到合法 C4 时成功。后续接受 packet MUST 不阻塞旧 flush，完成顺序 MUST 不改变目标集合判定，空集合 MUST 立即成功。waiter 竞争 MUST 沿用既有稳定 Busy 语义而不覆盖 waker；取消只停止等待。timeout/fault MUST 有界返回稳定错误且不夺取设备 ownership。

#### Scenario: slot 与 device-owned 目标，及调用后的新提交

- **WHEN** flush 目标同时覆盖尚在软件 slot 和已经 device-owned 的 packet，调用后又接受新 packet
- **THEN** MUST 等待所有原目标达到 C4 后完成，新 packet 不延迟旧目标结果
- **AND** 任意完成顺序 MUST 不掩盖未完成的小 ticket，空目标立即返回

#### Scenario: waiter 取消与第二 waiter

- **WHEN** flush waiter 被取消或第二个内部 waiter 竞争注册
- **THEN** 取消 MUST 只结束该次等待，第二 waiter MUST 返回稳定 Busy，原 waiter 的 waker 不被覆盖
- **AND** 已接受 packet MUST 保持原所有权并继续合法推进

#### Scenario: flush 超时或数据面 fault

- **WHEN** 目标未完成且 deadline 到期，或 owner 提交稳定 fault
- **THEN** flush MUST 有界返回带失败阶段/owner 的稳定错误，fault MUST 唤醒受影响 waiter
- **AND** MUST NOT 把 timeout/cancel 当作 DMA 已停止、C4 或 backing 可释放证明

### Requirement: 既有 stack 契约适配与无轮询 quiet

目标后端 MUST 接通既有独立常驻 stack 推进和 socket 接口契约。device progress、software mutation 与真实协议 deadline MUST 能唤醒对应层，数据推进 MUST 不依赖调用者主动 poll。无立即工作、无到期协议 timer 时 queue/stack MUST 休眠，MUST NOT 以固定周期 completion 轮询或 busy self-wake 维持进度。必要的单 hart task/timer 底座只服务本能力，不扩张为交互 shell。

#### Scenario: 无主动 poll 的四协议交换

- **WHEN** queue/stack 已激活，物理对端发起 ARP/ICMP/UDP/TCP 请求
- **THEN** MUST 完成双向真实包交换、UDP/TCP 内容一致与 TCP 正常关闭，无需额外网络 API 调用推进
- **AND** MUST NOT 用旧轮询 session 或 host 模型冒充真板异步结果

#### Scenario: 三种事件分别恢复推进

- **WHEN** 休眠层分别收到 device progress、软件状态变更或到期协议 timer
- **THEN** 对应层 MUST 获得调度并继续其工作，锁和临时 descriptor 借用不跨等待点
- **AND** timer MUST NOT 被用作补救设备 completion lost wakeup 的周期 fallback

#### Scenario: quiet 后重新到达流量

- **WHEN** 无立即工作和到期协议 timer 的安静窗口结束后出现新流量
- **THEN** 安静窗口 MUST 无持续 queue/stack poll 或 self-wake，新流量仍经真实 IRQ 恢复推进
- **AND** 已发生 fault 或未区分的背景流量 MUST NOT 被隐藏为 quiet 成功

### Requirement: 错误、超时与取消保留 DMA 所有权

初始化、提交、terminal/reclaim 与协议等待 MUST 有明确阶段、有限 deadline 和稳定错误语义。地址/cache 前置冲突或初始化 reset 超时 MUST 阻止继续启动。未提交资源可以安全撤销；提交后取消/超时只结束等待或进入既有 fail-stop 边界，MUST NOT 清 OWN、自动 reset 或提前释放/复用可能仍由设备访问的 backing。stable fatal MUST 拒绝新提交并在发布错误后唤醒受影响 queue/stack/flush/socket waiter。

#### Scenario: 初始化或运行阶段错误

- **WHEN** 前置冲突、reset timeout、descriptor/controller fatal 或 submit/reclaim 不变量错误发生
- **THEN** MUST 以阶段、可取得原始状态和 owner 报告稳定错误并停止不安全的后续操作
- **AND** MUST NOT 用 warning 后继续、普通 WouldBlock 或第二 owner 掩盖错误

#### Scenario: 提交前后取消与 deadline

- **WHEN** 请求分别处于软件待提交或 device-owned 状态并被取消或超时
- **THEN** 软件资源 MUST 可安全撤销，device-owned 资源 MUST 保持 owner 直到合法完成/安全回收，无法确认时保留 backing
- **AND** MUST NOT 将等待结束解释为 packet 撤回或设备已停止

#### Scenario: 有界退出和外部恢复

- **WHEN** 板测结束、失败或被操作者取消
- **THEN** MUST 有界停止新增操作、报告阶段并保持 backing 安全，操作者外部复位后确认原系统恢复
- **AND** MUST NOT 将该恢复路径声明为真板运行时 reset/link 恢复或 DMA quiesce 已验收

### Requirement: 真板直接验证、可解释观测与兼容边界

本能力 MUST 限于 CoM260 Kit 单 boot hart RAM-only，沿用已验收 DMA/cache、tail/doorbell 和 AIA 用法，并保持旧平台隔离与受影响异步契约。IRQ/wake、预算、slot occupancy/high-water/full、分类 drop、submit/terminal/reclaim 与 flush 目标/结果 MUST 可直接观察。host 逻辑和真板 IRQ/DMA/peer 结果 MUST 分别成立；Wireshark 沿用既有用户永久豁免，PC 原生输出与板上目标状态直接判定。验证与参考使用遵守公共规则及 M42，历史失败/豁免 MUST 单独说明。

#### Scenario: 确定性容量边界与真实流量

- **WHEN** 执行 RX/TX burst、Full→恢复、wrap、flush 和四协议真板测试
- **THEN** MUST 分别观察直接目标状态和对端内容，计数能解释 accepted/完成/回收/损失/待处理对象
- **AND** 普通 ping 成功或只有 IRQ/wake 计数 MUST NOT 替代容量、ownership 和 C4 结论

#### Scenario: 兼容构建与资料适用性

- **WHEN** 变更触及平台选择或共享 queue/stack 接缝并执行受影响验证
- **THEN** MUST 保持 QEMU/D1/VF2 与已验收 K3 行为边界，pre-existing 失败分别报告；板级参考寄存器与用法按 M42 留来源
- **AND** MUST NOT 复制第三方框架、扩写历史证据为本板资格或引入验证身份机制/专用判定层
