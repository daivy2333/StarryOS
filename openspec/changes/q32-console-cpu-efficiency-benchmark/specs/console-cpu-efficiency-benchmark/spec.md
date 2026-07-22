## ADDED Requirements

### Requirement: Q32-R1 D1 时间换算必须使用真实频率

系统 SHALL 按 D1 24 MHz timebase 将 cycle 转换为纳秒，并 MUST 以纯整数 helper 覆盖零值、
截断、余数、秒进位和大数边界。

#### Scenario: 24 MHz 正常换算
- **WHEN** benchmark 在 D1 上读取相隔 24,000,000 cycles 的两个时间点
- **THEN** elapsed time 必须为 1 秒，且 host 单元测试的 12 个边界用例全部通过

#### Scenario: 换算测试暴露旧实现
- **WHEN** RED 测试针对当前 1 MHz 假设运行
- **THEN** 测试必须因约 24 倍误差失败，并保留失败输出作为修复前 witness

### Requirement: Q32-R2 Console 与 Async 必须共享测量契约

Console benchmark SHALL 复用 Q31 的 payload、round count、section 语义、完成点、单位、raw
字段和排除规则，并 MUST 记录 Q31 输入与 Q32 输出的源码和日志 hash。

#### Scenario: 固定契约移植
- **WHEN** 实现者比较 Q32 benchmark 与 Q31 固定提交
- **THEN** 差异必须局限于 Console backend whitelist、标题、能力状态和已批准的 Console 适配

#### Scenario: Q31 基线发生漂移
- **WHEN** Q31 benchmark 或已验收日志 hash 与 Q32 manifest 不一致
- **THEN** comparison gate 必须失败，且不得静默采用新的 Async 输入

### Requirement: Q32-R3 S11 必须区分调用窗口与完成点

S11 SHALL 输出 write 调用和 completion/drain 的 raw timing，并 MUST 只把已完整发送且通过
完成检查的 payload 纳入汇总。

#### Scenario: 同步 Console 完成发送
- **WHEN** Console write 同步发送完整 payload 且 completion check 成功
- **THEN** S11 必须记录 completed bytes、write elapsed 和 completion elapsed，不得跨阶段重复乘轮数

#### Scenario: 短写或完成失败
- **WHEN** write 返回短写、错误或 completion/drain 失败
- **THEN** 当前样本必须标记失败且不得进入 throughput 或 latency 汇总

### Requirement: Q32-R4 S41 必须报告可复算的 CPU-work

S41 SHALL 对固定 payload 执行五轮有效测量，输出 elapsed、completed bytes 与 `instret` delta，
并 MUST 将 `instret` 描述为 hart-wide CPU-work proxy 而不是 CPU utilization。

#### Scenario: 五轮 CPU-work 完成
- **WHEN** `/proc/instret` 可用且五轮 payload 均通过完整性检查
- **THEN** S41 必须输出每轮 raw 值和由相同分母复算的 summary

#### Scenario: instret 不可用
- **WHEN** `/proc/instret` 缺失、读取失败或 delta 不可信
- **THEN** S41 必须输出 `not-available` 或失败状态，不得把缺失 delta 写成零或利用率

### Requirement: Q32-R5 S42 必须保留同步 Console 的 overlap 语义

S42 SHALL 使用与 Q31 相同的发送 workload 和观察窗口，并 MUST 接受同步 Console 的真实零
overlap，而不是通过改变被测路径制造并发。

#### Scenario: 观察窗口仍有 overlap
- **WHEN** write 返回后观察窗口内仍存在可测工作
- **THEN** S42 必须输出每轮 raw overlap、completed bytes 和五轮汇总

#### Scenario: 同步 write 消耗全部窗口
- **WHEN** Console write 已在调用内完成全部发送且剩余 overlap 为零
- **THEN** S42 必须把 `overlap_ns=0` 记录为有效边界结果，不得标成计时错误

### Requirement: Q32-R6 S43 必须区分 idle 与 loaded timer 适用性

S43 SHALL 用相同 request duration 测量 idle timer overshoot；只有存在真实发送 overlap 时才
MUST 计算 loaded timer overshoot 和 loaded aggregate。

#### Scenario: idle timer 测量完成
- **WHEN** timer 在无发送负载时完成固定轮数
- **THEN** S43 必须输出 request、actual、overshoot 与有效样本数

#### Scenario: loaded 窗口不存在
- **WHEN** 同步 Console write 在 timer 观察开始前已经完成且无真实 overlap
- **THEN** loaded 结果必须为 `not-applicable` 并从 aggregate 排除，不得输出伪零干扰

#### Scenario: timer timeout
- **WHEN** timer 超过 section deadline 或返回错误
- **THEN** 当前样本必须失败且不进入 idle 或 loaded summary，日志必须保留 timeout 边界

### Requirement: Q32-R7 不支持的 Console 诊断必须显式声明

Console benchmark SHALL 对 S40、UART TX 本地计数器和其他 backend-specific diagnostics 做
能力判断，并 MUST 以 `not-available` 表示缺失能力。

#### Scenario: Console 无 TX 计数器
- **WHEN** benchmark 运行在 `polling-console` 且没有 Async UART TX counter 接口
- **THEN** 对应字段必须为 `not-available`，比较报告不得把它解释为计数为零

#### Scenario: 真实零计数
- **WHEN** 某个受支持计数器完成读取且结果确实为零
- **THEN** 输出可以为零，但必须同时保留 capability available 状态以区别缺失能力

### Requirement: Q32-R8 失败样本必须与成功汇总隔离

benchmark MUST 在 byte mismatch、短写、drain/completion error、timeout、无效分母或未完成
section 时拒绝当前样本，并 SHALL 输出可定位失败边界的状态。

#### Scenario: 字节完整性失败
- **WHEN** accepted bytes 与 expected bytes 不一致
- **THEN** 样本必须标记失败且不得增加 valid round count

#### Scenario: 汇总分母为零
- **WHEN** completed bytes、elapsed 或 valid sample count 使派生公式分母为零
- **THEN** 派生指标必须标为不可计算，不得输出 infinity、伪零或沿用前一轮数值

#### Scenario: section 被取消
- **WHEN** 前置致命错误要求取消后续测量
- **THEN** 日志必须标记取消原因，且不得输出看似成功的 terminal summary

### Requirement: Q32-R9 QEMU 与 D1 证据必须分层保存

Q32 SHALL 分别保存 QEMU 协议验证和 D1 实板测量的命令、环境、源码、binary/image hash、raw
日志与 gate 结论，并 MUST 禁止用 QEMU 数值替代 D1 硬件性能证据。

#### Scenario: QEMU smoke validation
- **WHEN** QEMU image 启动并运行 benchmark
- **THEN** 证据必须证明 section、错误处理、summary 和终止标记完整，但结论仅限虚拟环境

#### Scenario: D1 实板采集
- **WHEN** 固定 image 在 D1 上按记录的串口设置和命令完成测量
- **THEN** 证据必须保存原始轮次与 hash，并把硬件结果与 QEMU 结果分开汇报

#### Scenario: 构建环境被 sandbox 阻止
- **WHEN** restricted shell 以 `Bad system call` 或等价策略阻止 compiler
- **THEN** 本次构建必须记为 blocked witness，不得把旧 binary 当作新源码的成功产物

### Requirement: Q32-R10 横向比较必须可审计且结论中立

最终报告 SHALL 只比较 Q31 与 Q32 共同支持、同 workload、同完成点、同单位的指标，MUST
公开公式、raw 来源、适用性和限制，并 MUST NOT 使用预设性能胜负阈值。

#### Scenario: 共同指标可复算
- **WHEN** Q31 与 Q32 的 raw 字段、hash 和有效轮次均通过 gate
- **THEN** 报告必须逐项展示两侧值、公式、单位、分母与差异，不混合 QEMU 和 D1

#### Scenario: backend-specific 字段不同
- **WHEN** Async 有 TX 诊断而 Console 为 `not-available`
- **THEN** 报告必须把该字段列为非共同指标，不得补零、删去限制或推导架构胜负

#### Scenario: 一侧证据不完整
- **WHEN** 任一环境缺少完整日志、hash、有效轮次或完成标记
- **THEN** 对应比较必须标为未通过或不可比较，change 不得宣称最终 comparison gate 通过

### Requirement: Q32-R11 D1 runtime 必须提供可用的 timer interrupt

D1 Console runtime MUST 使用能注册并处理 supervisor timer 的平台 IRQ 实现；smoke 模式
SHALL 保持最小 IRQ stub，polling Console MUST NOT 因此改为 UART interrupt 数据路径。

#### Scenario: Fullbench absolute sleep 被 timer 唤醒
- **WHEN** `lichee-d1-fullbench-command` 运行 `clock_nanosleep` absolute timer smoke
- **THEN** feature graph 必须包含 `axplat-riscv64-lichee-d1/irq`，timer handler 必须注册成功，sleep 必须在 deadline 后返回

#### Scenario: Smoke 保持最小 IRQ 边界
- **WHEN** 构建 `lichee-d1-smoke`
- **THEN** feature graph 必须只选择 `irq-if`/stub，且不得引入 `riscv_plic` 或完整 `irq` feature

#### Scenario: Console 数据路径保持 polling
- **WHEN** Console runtime 启用完整平台 IRQ 并运行 TX benchmark
- **THEN** Console backend 必须仍为 `polling-console`，不得注册 UART IRQ handler，S10/S42 的同步 write 和 drain 语义必须不变
