## Purpose

规定 CoM260 Kit 单 hart 真板网络在物理链路变化和受控运行时设备 reset 下的生命周期与资源安全行为。通过 epoch 隔离、DMA 停止依据、稳定错误传播和失败时 backing 保留，使恢复后的新网络对象可用，且旧对象不会污染新实例；host 与真板结论分别成立。

## Requirements

### Requirement: 唯一 owner 的有界恢复生命周期

系统 MUST 由同一个常驻 queue owner 串行推进正常服务、停止提交、旧实例隔离、DMA 停止、设备 reset 与队列重建。各阶段 MUST 可区分且有界；新实例只有在全部必要前置成立后才能发布可服务状态。恢复期间 IRQ MUST 继续只承担 cause/ack/mask/wake，MUST NOT 在 ISR 中操作 descriptor ownership、重建队列或执行协议栈。设备、软件与 timer 事件 MUST 通过既有注册/重查协议最终得到处理，不引入第二 owner 或 completion 周期轮询 fallback。

#### Scenario: S01 正常 reset

- **WHEN** Active 单 hart 数据面持有旧 socket 和在途资源，收到合法受控 reset 请求
- **THEN** 同一 owner MUST 隔离旧实例，取得 DMA 停止依据，再完成 reset/重建并以新 epoch 恢复 I/O
- **AND** 任一步失败 MUST NOT 发布半激活实例或创建第二 owner

#### Scenario: S02 恢复事件交错

- **WHEN** IRQ、软件容量事件或 timer 在恢复阶段转换、注册/重臂或返回 Pending 附近到达，且后续不再有事件
- **THEN** owner MUST 按当前阶段处理或保留有界重查机会，使最后必要工作继续推进
- **AND** 事件 MUST NOT 令已关闭实例复活或依赖下一包补救

### Requirement: 新旧 epoch 与资源归属隔离

系统 MUST 区分 queue 实例、link 状态与 socket 生命周期，并将 packet、cookie、descriptor、buffer 和 ticket 的归属绑定到对应实例。只有同步后有效且与当前 ledger 匹配的完成才能单次回收；旧完成 MUST NOT 关闭新 ticket、满足新 flush 或释放新 buffer。重复、未知或不匹配且不能证明安全的完成 MUST 进入稳定隔离错误。epoch 推进 MUST 使用不与活跃归属冲突的有界策略，不以静默 wrapping 复用活跃身份。这里的 epoch/cookie 是产品所有权协议，不是验证身份字段。

#### Scenario: S03 当前完成与 wrap

- **WHEN** 当前 epoch 的合法 TX/RX 完成经过 ring wrap 并回收
- **THEN** descriptor/buffer/ticket MUST 单次闭合，资源账本按实际 owner 角色守恒
- **AND** 常驻 RX device-owned 资源 MUST 单独计入，C4 MUST NOT 等同 peer delivery

#### Scenario: S04 迟到旧完成

- **WHEN** 新 epoch 已开始且位置可能复用，观察或通过生产路径夹具注入旧 completion、cookie 或 ticket
- **THEN** 系统 MUST 识别旧归属，且新实例的资源、flush 与 socket 结果保持不受污染
- **AND** 不能证明归属时 MUST 隔离并报错，不猜测为当前完成

#### Scenario: S05 重复或未知完成及 epoch 边界

- **WHEN** 已回收对象再次完成、完成无法映射合法 owner，或 epoch 已到表示边界
- **THEN** 系统 MUST 避免双重回收和活跃身份复用，输出有界诊断并按安全错误策略处置
- **AND** MUST NOT 静默 wrapping、伪成功或破坏当前实例账本

### Requirement: 真实 PHY link 控制面与新连接恢复

系统 MUST 在任务上下文提交初始真实 PHY link 状态并处理后续变化。重复状态通知 MUST 不重复关闭生命周期。已确认 down MUST 关闭旧 socket 生命周期、撤销未提交 packet、阻止新的 enqueue/driver submit；已提交对象仍由原 owner 通过合法完成或经证明的停止关闭。已确认 up 只有在 queue 可服务时才允许新对象恢复 I/O；MUST NOT 自动等同设备 reset、令旧 socket 复活或重发交付结果不确定的旧 packet。link down MUST NOT 作为 DMA 已停止的证明。

#### Scenario: S06 初始与重复 link

- **WHEN** owner 启动后读取初始 PHY 状态，或收到与已提交状态相同的通知
- **THEN** 系统 MUST 提交真实初始状态，重复通知不重复关闭或重建对象
- **AND** PHY 读取失败 MUST 有界报告，不猜测 link up 或无限自旋

#### Scenario: S07 link down

- **WHEN** 已有 socket、软件待提交和 device-owned packet 时，真实拔线且任务观察到 down
- **THEN** 旧 socket MUST 终止，新提交 MUST 拒绝，未提交资源 MUST 单次撤销
- **AND** device-owned backing MUST 保留到合法回收或已证明停止，不因拔线清 OWN 或提前复用

#### Scenario: S08 link up

- **WHEN** down 已提交，当前 queue 未隔离失败，真实插线后观察到 up
- **THEN** 新建 socket MUST 可经异步网络完成双向 I/O，旧 socket 仍保持 terminal
- **AND** MUST NOT 自动 reset、透明续传或跨生命周期重发不确定 packet

#### Scenario: S09 link、reset 与 completion 组合

- **WHEN** 在途 packet、flush 和旧 socket 存在时，link 变化、受控 reset 请求与完成交错
- **THEN** 系统 MUST 按明确控制面顺序关闭或恢复正确生命周期，完成只处置所属 owner
- **AND** link up MUST NOT 越过尚未结束的恢复阶段或稳定 fault

### Requirement: 真实 DMA 停止后才能复用与重建

运行时 reset MUST 先停止新提交并隔离旧实例，按目标控制器与板级依据完成必要 stop/reset；旧 descriptor、queue 和 buffer backing 只有在能够证明设备不再访问它们后才能释放或复用。新实例 MUST 完成必要 DMA/cache ownership、队列初始化、RX refill 与通知注册/重臂后才对外可服务。DMA 停止结论 MUST 由目标硬件语义与本板直接行为支撑，单纯寄存器读回、清 OWN、IRQ 安静、PHY down、QEMU reset 或操作者外部复位均不能单独使本项通过。

#### Scenario: S10 DMA 停止后复用

- **WHEN** 旧实例已隔离且停止新提交，执行控制器规定的 stop/reset 并获得本板停止依据
- **THEN** 系统 MUST 在停止成立后才回收或复用旧 backing，再发布完整的新实例
- **AND** 恢复后的真实双向交换与资源账本 MUST 成立；缺少停止依据时本场景不通过

### Requirement: 恢复失败的稳定 fault 与 backing 保留

停止、reset、重建或 ownership 不变量失败 MUST 进入可观察的稳定 fault，拒绝新提交并唤醒受影响等待者。可能仍被设备访问的 backing MUST 保留；只有已证明不可达的资源才允许回收。MUST NOT 以 warning 后继续、第二 owner、无界自动重试、新分配覆盖泄漏或普通 Full 隐藏失败。失败边界内不承诺再次软件自动恢复，外部安全恢复路径保留。

#### Scenario: S11 停止或 reset 失败

- **WHEN** 设备可能仍持有 descriptor/buffer，停止不能证明或 reset 超时/返回错误
- **THEN** owner MUST 保持 Faulted、保留不确定 backing、拒绝新提交并发布稳定错误
- **AND** MUST NOT 释放或复用该 backing，也不能虚报 reset 成功

#### Scenario: S12 重建失败

- **WHEN** 旧 DMA 已证明停止，但新队列、RX refill 或通知准备失败
- **THEN** 系统 MUST 不发布半激活实例，并按新旧资源真实可达性保留或回收 backing
- **AND** 已重新可能被设备访问的资源 MUST NOT 因此前停止结论而被提前释放

### Requirement: 分层取消与分阶段 deadline

取消 MUST 区分 waiter、pre-submit 和 device-owned：撤销 waiter 只结束等待，关闭生命周期时未提交 packet 可单次撤销，已交给设备的 packet 只有合法完成或已证明停止后才能处置。常驻 owner MUST 不依赖请求 future 存活来完成必要资源闭合。submit wait、terminal、reclaim、quiesce/stop、reset 与重建 MUST 有有限 deadline 和可观察阶段错误；同步 MMIO 调用本身 MUST NOT 被伪装为可抢占异步 timeout。等待结束 MUST NOT 当作 DMA 停止或 C4 成功。

#### Scenario: S13 waiter 取消

- **WHEN** flush/socket 或恢复请求等待者被 drop 或取消
- **THEN** 系统 MUST 仅撤销该等待注册，必要资源闭合仍由常驻 owner 推进
- **AND** MUST NOT 回滚设备 ownership 或把取消当停止证明

#### Scenario: S14 pre-submit 与 submit 交错

- **WHEN** packet 正从软件队列转交 driver，同时发生取消或生命周期关闭
- **THEN** 系统 MUST 在明确提交点只选择未提交撤销或 device-owned 处置中的一条路径
- **AND** 同一 packet MUST NOT 同时提交和回收，也不跨新生命周期自动重发

#### Scenario: S15 分阶段 deadline

- **WHEN** submit wait、terminal、reclaim、停止、reset 或重建未在对应 deadline 内完成，或发生 fatal
- **THEN** 等待 MUST 有限结束，错误 MUST 区分失败阶段及资源 owner，并保持必要 backing
- **AND** MUST NOT 永久 Pending、以普通 Full 掩盖 fault 或谎报取消即 C4

### Requirement: Socket 与 flush terminal 一致且不污染新对象

link down、reset 或 fatal 关闭旧生命周期时，旧 TCP、UDP、listener、flush 与 readiness 等待者 MUST 在 wake 之前得到稳定 terminal/error。readiness 与紧随其后的 I/O MUST 一致，恢复后的旧对象 MUST 不使用新 queue 继续原连接。新实例和 link 可服务后，新 socket MUST 不继承旧 terminal 污染。已取消/失败的旧 packet MUST 不使 flush 虚报 C4；本能力 MUST 保持既有多 waiter 与稳定竞争语义。

#### Scenario: S16 旧 waiter 错误一致

- **WHEN** link/reset/fault 关闭持有旧 socket、listener、flush 和多个 readiness waiter 的生命周期
- **THEN** 系统 MUST 先提交错误再 wake，poll 与随后的 read/write/connect/accept/flush 返回匹配结果
- **AND** MUST NOT 把错误伪装 EOF 成功、C4 或普通 Full

#### Scenario: S17 新对象恢复

- **WHEN** 新 queue 和 link 已可服务，应用新建 TCP/UDP/listener 并通信
- **THEN** 新对象 MUST 完成真实交换，旧对象仍返回稳定 terminal
- **AND** MUST NOT 沿用旧队列状态或继承旧 registry 错误

### Requirement: 真板行为、重复恢复与兼容边界

验收 MUST 分别覆盖 host 生产路径故障注入与 CoM260 Kit 单 hart 真板 link/reset 行为；旧完成或难以稳定制造的失败可用生产测试夹具注入，但不能代替物理 DMA 停止和恢复后真实网络交换。重复恢复 MUST 保持 owner 账本和容量，不以新分配掩盖泄漏；正常 IRQ/异步唤醒、quiet、C4、旧平台隔离和安全外部恢复路径 MUST 保持。观测 MUST 直接呈现目标状态、阶段错误、对端输出及资源归属，遵守公共规则，不新增验证身份协议或专用判定层；既有 Wireshark 永久豁免继续适用。

#### Scenario: S18 重复恢复与容量

- **WHEN** 正常流量后重复执行成功 link flap/reset，并再次 RX/TX、Full→恢复与 flush
- **THEN** 资源账本 MUST 按角色闭合，长期可用容量不逐轮缩小，双向 I/O 与 C4 均成立
- **AND** 稳定 fault MUST NOT 算作成功恢复，新分配 MUST NOT 掩盖未关闭旧 owner

#### Scenario: S19 正常兼容

- **WHEN** 不注入故障，运行受影响生产路径测试、构建与正常异步网络验证
- **THEN** 既有单 owner、IRQ/quiet、C4 和平台隔离 MUST 保持；pre-existing 失败单独报告
- **AND** host/QEMU MUST NOT 代替本板 DMA、link、reset 结果，不扩张为多 hart、组合长稳或性能结论

#### Scenario: S20 退出与外部回滚

- **WHEN** 板测结束、操作者取消或进入 fail-stop，停止新增操作并沿既有外部 RESET 路径恢复
- **THEN** 系统 MUST 有界报告阶段、保留不确定 backing，操作者能够恢复原系统
- **AND** 该外部回滚 MUST NOT 替代运行时 reset 或 DMA 停止验收
