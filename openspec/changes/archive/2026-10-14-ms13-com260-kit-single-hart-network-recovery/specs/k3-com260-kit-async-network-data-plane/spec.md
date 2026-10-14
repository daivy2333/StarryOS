## MODIFIED Requirements

### Requirement: 错误、超时与取消保留 DMA 所有权

初始化、提交、terminal/reclaim 与协议等待 MUST 有明确阶段、有限 deadline 和稳定错误语义。地址/cache 前置冲突或初始化 reset 超时 MUST 阻止继续启动。未提交资源可以安全撤销；提交后取消/超时只结束等待、进入既有 fail-stop 边界，或由唯一常驻 owner 进入符合目标板单 hart网络恢复规格的有界恢复控制面，MUST NOT 清 OWN、无控制地自动 reset 或提前释放/复用可能仍由设备访问的 backing。受控恢复 MUST 隔离旧实例、取得 DMA 停止依据后才重建或复用资源；不能确认停止或恢复失败时 MUST 保留 backing、拒绝新提交并稳定报错。普通取消 MUST NOT 隐式请求 reset。stable fatal MUST 拒绝新提交并在发布错误后唤醒受影响 queue/stack/flush/socket waiter；ownership drift MUST NOT 用 reset 掩盖。

#### Scenario: 初始化或运行阶段错误

- **WHEN** 前置冲突、初始化 reset timeout、descriptor/controller fatal 或 submit/reclaim 不变量错误发生
- **THEN** MUST 以阶段、可取得原始状态和 owner 报告稳定错误并停止不安全的后续操作；仅符合恢复控制面契约的可恢复错误允许由原 owner 有界处置
- **AND** MUST NOT 用 warning 后继续、普通 WouldBlock 或第二 owner 掩盖错误；无法确认 DMA 停止时不得重建或释放 backing

#### Scenario: 提交前后取消与 deadline

- **WHEN** 请求分别处于软件待提交或 device-owned 状态并被取消或超时
- **THEN** 软件资源 MUST 可安全撤销，device-owned 资源 MUST 保持 owner 直到合法完成或按恢复控制面确认停止后的安全回收，无法确认时保留 backing
- **AND** MUST NOT 将等待结束解释为 packet 撤回、设备已停止或 C4 成功；普通 waiter drop 不得隐式 reset

#### Scenario: 有界退出和外部恢复

- **WHEN** 板测结束、失败或被操作者取消
- **THEN** MUST 有界停止新增操作、报告阶段并保持 backing 安全，操作者外部复位后确认原系统恢复
- **AND** MUST NOT 将该外部恢复路径声明为真板运行时 reset/link 恢复或 DMA quiesce 已验收；运行时恢复必须另行满足对应目标行为
