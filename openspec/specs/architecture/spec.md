# architecture/spec.md — 架构决策记录

> 迁移自 .claude/docs/architecture.md，2026-06-03
> 条目格式: ADR-{编号} - {决策标题}

---

## Purpose

定义 StarryOS 异步串口驱动的架构决策和设计原则，指导开发过程中的技术选型和系统设计。

## Requirements

### Requirement: 异步运行时选型

所有异步任务 SHALL 基于 axtask::future 运行时，仅引入 embassy-sync::AtomicWaker 用于 ISR 安全唤醒。

#### Scenario: 选择异步框架

- **WHEN** 开发者需要实现异步串口功能
- **THEN** 必须使用 `axtask::future::block_on` + `poll_io` + `register_irq_waker` 模式，不得引入完整 Embassy executor

#### Scenario: ISR 唤醒

- **WHEN** 中断处理需要唤醒异步任务
- **THEN** 必须使用 `embassy_sync::AtomicWaker::wake()`，确保 ISR 安全无阻塞

### Requirement: VFS 接口集成

所有设备 MUST 通过 DeviceOps trait 注册到 VFS，支持 poll/select/epoll。

#### Scenario: 注册新设备

- **WHEN** 开发者需要注册新的异步串口设备
- **THEN** 必须实现 `DeviceOps` trait，通过 `Device` wrapper 注册到 `/dev`，并实现 `Pollable` trait 支持异步 I/O

### Requirement: 缓冲策略

系统 SHALL 使用 ringbuf::HeapRb + PollSet 实现双缓冲，每个方向各一个。

#### Scenario: 配置缓冲区

- **WHEN** 初始化异步串口驱动
- **THEN** 必须为 RX 和 TX 各创建一个 `HeapRb<u8>` ring buffer（默认 64 KiB），由单一后台协程操作硬件 FIFO

### Requirement: 硬件抽象层

系统 SHALL 定义 AsyncUart trait 抽象不同 UART 硬件，初期实现 Uart16550。

#### Scenario: 支持新硬件

- **WHEN** 需要支持新的 UART 硬件型号
- **THEN** 必须实现 `AsyncUart` trait，保持与现有架构兼容

### Requirement: 中断处理架构

ISR MUST 极简：读 ISR 寄存器 + 禁用中断 + 唤醒 waker，数据搬运推迟到任务上下文。

#### Scenario: 处理 UART 中断

- **WHEN** UART 中断触发
- **THEN** ISR 必须在 ~1.5µs 内完成：读 ISR 判断类型 → 禁用对应中断 → 调用 AtomicWaker::wake() → 立即退出

### Requirement: Console 共存策略

内核日志 SHALL 使用 earlycon polling TX，用户态使用 AsyncUart，两者共享 UART THR 互不冲突。

#### Scenario: 内核日志输出

- **WHEN** ax_println! 输出内核日志
- **THEN** 走 `axhal::console::write_bytes()` 同步 polling TX 路径，与 TX copier 共享 THR 无实质冲突

### Requirement: MMIO 访问安全

系统 SHALL 使用 axmm::iomap() 确保设备 MMIO 映射到内核页表，权限为 READ|WRITE|DEVICE。

#### Scenario: 初始化 UART MMIO

- **WHEN** 内核启动时初始化 UART
- **THEN** 必须调用 `axmm::iomap(PhysAddr::from(0x10000000), 0x1000)` 确保映射存在且权限正确

### Requirement: stride 配置

NS16550 寄存器空间仅 8 字节，MUST 使用 stride=1，否则越界访问导致 LoadFault。

#### Scenario: 配置 UART stride

- **WHEN** 初始化 uart_16550 crate
- **THEN** 必须使用 `stride=1`，禁止 `stride=4`（会导致寄存器偏移越界）

### Requirement: NAPI 中断合并

高吞吐场景 SHALL 使用 NAPI 模式减少 IRQ 频率，连续成功读取 ≥16 次后进入轮询模式。

#### Scenario: 高吞吐数据接收

- **WHEN** 连续成功读取字节数达到 NAPI_THRESHOLD (16)
- **THEN** 进入轮询模式，batch size 缩小到 NAPI_BATCH_SIZE (64)，不重新使能 RX 中断

### Requirement: 全异步 TX

TX copier SHALL 接管 UART 发送，AsyncUartWriter 实现 TtyWrite 写入 ring buffer。

#### Scenario: 异步发送数据

- **WHEN** 用户态写入数据到 /dev/console
- **THEN** 数据进入 TX ring buffer → TX copier 发送到 UART THR → FIFO 满时使能 TX 中断 → ISR 唤醒 copier 继续

### Requirement: 三层嵌套优化

用户态 async read 路径有 3 层嵌套 block_on/poll_io，MUST 使用 External 模式消除 yield storm。

#### Scenario: 异步读取优化

- **WHEN** 用户态异步读取串口数据
- **THEN** 使用 `ProcessMode::External` 替代 `Manual`，避免 waker.wake_by_ref() 导致的立即唤醒

### Requirement: FIONBIO 传播

nonblocking 标志 MUST 从 ioctl(FIONBIO) 传播到 TTY 层，确保非阻塞读生效。

#### Scenario: 设置非阻塞模式

- **WHEN** 用户态调用 ioctl(FIONBIO) 或 fcntl(O_NONBLOCK)
- **THEN** Tty struct 的 AtomicBool nonblocking 必须更新，read_at() 使用该标志控制 block_on 行为

### Requirement: 性能测试覆盖

测试 MUST 覆盖吞吐量、延迟、内存消耗、NAPI 效果等指标。

#### Scenario: 运行性能测试

- **WHEN** 开发者需要验证异步串口性能
- **THEN** 必须使用内核态 benchmark 模块测量 TX/RX 吞吐量、延迟 P50/P99、IRQ 频率
