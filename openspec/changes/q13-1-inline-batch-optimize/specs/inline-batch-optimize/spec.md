## ADDED Requirements

### Requirement: 热路径函数内联

uart_16550 异步驱动的热路径函数 MUST 使用 `#[inline(always)]` 确保内联，消除函数调用开销。

#### Scenario: RX copier 热路径内联

- **WHEN** RX copier 调用 `uart.receive_bytes(buf)`
- **THEN** 编译器 MUST 内联 `receive_bytes` 调用，无函数调用开销

#### Scenario: TX copier 热路径内联

- **WHEN** TX copier 调用 `uart.send_bytes(buf)`
- **THEN** 编译器 MUST 内联 `send_bytes` 调用，无函数调用开销

#### Scenario: Ring buffer push/pop 内联

- **WHEN** 调用 `RingBufRx::push` 或 `RingBufTx::pop`
- **THEN** 编译器 MUST 内联这些调用，无函数调用开销

### Requirement: 批量操作接口

uart_16550 ring buffer MUST 提供批量 push/pop 接口，减少每字节的锁获取次数。

#### Scenario: RX 批量 push

- **WHEN** RX copier 有多个字节要写入 ring buffer
- **THEN** MUST 使用 `push_batch` 方法一次性写入，减少锁获取次数

#### Scenario: TX 批量 pop

- **WHEN** TX copier 要从 ring buffer 读取多个字节
- **THEN** MUST 使用 `pop_batch` 方法一次性读取，减少锁获取次数

#### Scenario: 批量大小限制

- **WHEN** 批量操作的数据量超过 ring buffer 容量
- **THEN** MUST 返回实际处理的字节数，不阻塞或失败

### Requirement: 性能回归验证

优化后的性能 MUST 不低于 Q12 基线水平。

#### Scenario: 单字节延迟

- **WHEN** 运行 1 字节 benchmark
- **THEN** 平均延迟 MUST ≤ 130µs（Q12 基线 124µs）

#### Scenario: 吞吐量

- **WHEN** 运行 256 字节 benchmark
- **THEN** 吞吐量 MUST 不低于 Q13 优化前水平

#### Scenario: 功能完整性

- **WHEN** 运行 Shell 交互测试
- **THEN** 输入输出 MUST 正常，无数据丢失或乱序

#### Scenario: FIONBIO 测试

- **WHEN** 运行非阻塞模式测试
- **THEN** O_NONBLOCK open 和 ioctl FIONBIO MUST 通过
