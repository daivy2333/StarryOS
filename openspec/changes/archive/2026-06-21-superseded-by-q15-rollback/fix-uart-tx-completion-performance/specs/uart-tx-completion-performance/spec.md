## ADDED Requirements

### Requirement: M4 TX backpressure 回归见证

系统 MUST 用同配置 benchmark 持续见证 M4 `Poll::Ready→Poll::Pending` 引入的 FIFO refill 延迟，性能声明必须包含 before/after commit、原始延迟和 CPU 指标。

#### Scenario: 运行 FIFO 边界基准
- **WHEN** benchmark 测试 1/15/16/17/31/32/33/48/49/64B
- **THEN** 输出 MUST 包含每个尺寸的原始延迟、P50/P95、tx poll/no-progress 和 CPU idle
- **AND** M4 RED 基线 MUST 见证跨 16B 边界的延迟台阶

#### Scenario: 声明回归已修复
- **WHEN** 开发者声明 M4 TX 性能恢复
- **THEN** 64B–4096B Async `write+tcdrain` MUST 不劣于同环境阻塞基线 10%
- **AND** 1B 延迟 MUST 不劣于 pre-M4 同方法基线 10%

### Requirement: 有界 TX fast-path backpressure

TX copier MUST 在 FIFO 暂时不可写时使用有界 fast-path retry，并在预算耗尽后回落到 IRQ 驱动的 Pending；实现 MUST 禁止无限 busy-poll。

#### Scenario: 快速恢复可写
- **WHEN** `send_bytes()` 在 32 次 fast retry 预算内从 0 变为正数
- **THEN** copier MUST 在当前 poll 中继续提交 staged 数据
- **AND** MUST NOT 等待下一个 scheduler tick

#### Scenario: 预算内仍不可写
- **WHEN** `send_bytes()` 连续 32 次返回 0
- **THEN** copier MUST register TX waker、enable THRE、recheck 后返回 `Poll::Pending`
- **AND** 后续 IRQ MUST 能恢复同一 staging cursor

#### Scenario: UART 空闲
- **WHEN** TX ring 和 staging 连续 10 秒无数据
- **THEN** tx poll/no-progress counter MUST NOT 持续增长
- **AND** 系统 MUST 不存在 TX busy-poll CPU 占用

### Requirement: 三阶段 TX completion

`flush` 和 `tcdrain` MUST 同时等待 ring queued、copier staged 与 UART hardware 三阶段完成，禁止仅以 ring empty 或 accepted/popped 相等作为完成条件。

#### Scenario: copier 已 pop 但尚未写 UART
- **WHEN** ring 已空而 copier 仍持有 staged bytes
- **THEN** `flush` 和 `tcdrain` MUST 保持 Pending

#### Scenario: FIFO 空但 shift register 忙
- **WHEN** ring/staging 均空且 THRE=true、TEMT=false
- **THEN** drain MUST 协作 yield 并重查 TEMT
- **AND** MUST NOT 假设会出现独立 TEMT IRQ

#### Scenario: 全部阶段完成
- **WHEN** ring 无待处理数据、copier inactive、staged bytes=0 且 TEMT=true
- **THEN** `flush` 和 `tcdrain` MUST 返回成功

### Requirement: TX 短写与 backpressure 可见

非空 TX 写入 MUST 返回实际进入 ring 的字节数；ring 满时系统 MUST 返回短写、等待或 `WouldBlock`，禁止报告未接收字节写入成功。

#### Scenario: ring 有部分空间
- **WHEN** writer 输入长度大于 TX ring 当前可用空间
- **THEN** write MUST 返回实际接收字节数
- **AND** dropped counter MUST 与未接收字节一致

#### Scenario: nonblocking ring 满
- **WHEN** nonblocking writer 对满 ring 写入非空数据
- **THEN** VFS MUST 返回 `WouldBlock` 或零进展对应错误
- **AND** MUST NOT 返回输入总长度

### Requirement: 性能诊断成本可独立控制

ring/copier metrics MUST 保留 accepted/dropped/high-water/hw-bytes/no-progress 语义，同时 MUST 可独立量化和关闭高频采集成本。

#### Scenario: 禁用详细 telemetry
- **WHEN** 发布构建关闭详细 TX telemetry
- **THEN** 数据路径 MUST 不记录 per-event timestamp
- **AND** 基础正确性 counters MUST 继续可用或由文档明确 feature 行为
