# tasks.md — 任务追踪

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。

---

## Milestone 概览（完全剔除 Console 方案）

| Milestone | 目标 | 底层引擎 | Gate | 依赖 | 建议 |
|-----------|------|----------|------|------|------|
| **P0** | 项目规划与设计 | — | 分支创建 + 文档更新 + Milestone 规划 | — | 必做 ✅ |
| **P1** | UART 硬件初始化替代 | uart_16550（本地） | UART 初始化成功 + 内核启动日志输出 | P0 | 必做 ✅ |
| **P2** | 异步串口架构实现 | uart_16550 + axtask | RX/TX copier + Ring Buffer + ISR | P1 | 必做 ✅ |
| **P3** | Console 软件路径剔除 | AsyncUart | Console 软件路径完全剔除 + AsyncUart 独占 UART | P2 | 必做 ✅ |
| **P4** | VFS 集成验证 | AsyncUart | DeviceOps + 设备注册 + 用户态 API | P3 | 必做 ✅ |
| **P5** | 性能优化 | AsyncUart | 性能基准达标 + IRQ 频率优化 | P4 | 优先 |
| **P6** | 真板验证 | AsyncUart | VisionFive2 实际验证 | P5 | 可并行 |

---

## P0: 项目规划与设计

> 目标: 建立新分支的文档体系，明确完全剔除 Console 的设计方案

<!-- P0.1 --> - [x] 创建 feat/uart-async-dev2 分支 ✅ 2026-05-28
  - 基于 feat/uart-async 创建新分支
  - 回滚所有代码变更（保留文档体系）

<!-- P0.2 --> - [x] 提交 feat/uart-async 分支文档 ✅ 2026-05-28
  - Console UART 研究文档
  - AsyncUart 集成设计方案（归档）

<!-- P0.3 --> - [x] 回滚代码变更 ✅ 2026-05-28
  - 删除 kernel/src/drivers/serial/ 目录
  - 恢复 kernel/Cargo.toml、lib.rs、dev/mod.rs

<!-- P0.4 --> - [ ] 更新文档体系 🔄
  - 更新 SNAPSHOT.md（当前分支状态）
  - 更新 tasks.md（新 Milestone 规划）
  - 更新 architecture.md（新 ADR）
  - 清理 learned.md/references.md（过时信息）

<!-- P0.5 --> - [ ] 设计完全剔除 Console 方案
  - 确定 UART 初始化替代方案
  - 确定 Console 软件路径剔除范围
  - 设计 earlycon（内核启动日志）

**Gate P0**: 文档体系完整 + Milestone 规划明确 + 设计方案初步形成

---

## P1: UART 硬件初始化替代

> 目标: 替代 axplat 的 UART 初始化，实现独立的 UART 硬件配置

<!-- P1.1 --> - [ ] 添加 uart_16550 本地依赖
  - 在 kernel/Cargo.toml 添加 path 依赖：`../../uart_16550`
  - 添加 embassy-sync 依赖（用于 AtomicWaker）
  - 验证：cargo check 编译通过

<!-- P1.2 --> - [ ] 实现 UART 初始化函数
  - 创建 kernel/src/drivers/uart_init.rs
  - 实现 `init_uart_hardware()`：配置波特率、FIFO、中断
  - 配置：BaudRate::Baud115200, FifoTriggerLevel::Fourteen, IER::DATA_READY
  - 验证：UART 寄存器配置正确（IER/LSR/ISR）

<!-- P1.3 --> - [ ] 替代 axplat UART 初始化
  - 在内核启动流程中调用 `init_uart_hardware()`
  - 位置：entry.rs 或 axruntime::init 后
  - 验证：UART 硬件初始化成功

<!-- P1.4 --> - [ ] 实现 earlycon（内核启动日志）
  - 实现简单的 polling TX 输出（用于启动日志）
  - 不依赖 AsyncUart，纯同步阻塞
  - 验证：内核启动日志可见

<!-- P1.5 --> - [ ] Gate P1 验证
  - `make run` 编译通过 + 内核启动
  - UART 初始化成功（IER 配置正确）
  - 内核启动日志输出正常

**Gate P1**: UART 硬件初始化成功 + 内核启动日志输出正常

---

## P2: 异步串口架构实现

> 目标: 实现独立的异步串口架构（不依赖 Console）

<!-- P2.1 --> - [ ] 实现 AsyncUart trait
  - 创建 kernel/src/drivers/async_uart.rs
  - 定义 AsyncUart trait（try_read/try_write + 中断控制）
  - 实现 Uart16550Async（包装 uart_16550 crate）

<!-- P2.2 --> - [ ] 实现 Ring Buffer + PollSet
  - 创建 kernel/src/drivers/ring_buffer.rs
  - 实现 AsyncBuffer（rx_buf + tx_buf + rx_wakers + tx_wakers）
  - 使用 ringbuf::HeapRb<u8> + axpoll::PollSet

<!-- P2.3 --> - [ ] 实现 ISR + AtomicWaker
  - 创建 kernel/src/drivers/isr.rs
  - 实现 IsrContext（Mutex<Uart> + rx_waker + tx_waker）
  - 实现 uart_isr_handler（读 ISR → 禁用中断 → wake waker）

<!-- P2.4 --> - [ ] 实现 RX/TX copier 任务
  - 创建 kernel/src/drivers/async_driver.rs
  - 实现 AsyncUartDriver（RX copier + TX copier）
  - RX copier: IRQ → read UART → push rx_buf
  - TX copier: pop tx_buf → write UART → IRQ

<!-- P2.5 --> - [ ] 注册 ISR hook
  - 调用 axhal::register_irq_hook(10, uart_isr_handler)
  - 验证：ISR 触发正常

<!-- P2.6 --> - [ ] Gate P2 验证
  - RX/TX copier 任务启动正常
  - IRQ 触发 → ISR → copier 唤醒流程正确
  - 数据收发正常（环形缓冲区）

**Gate P2**: 异步串口架构完整 + RX/TX copier 任务正常

---

## P3: Console 软件路径剔除

> 目标: 完全剔除 Console 软件路径，AsyncUart 独占 UART

<!-- P3.1 --> - [ ] 分析 Console 软件路径
  - 查找所有使用 axhal::console 的代码
  - 确定剔除范围：ntty.rs、ldisc.rs、entry.rs

<!-- P3.2 --> - [ ] 剔除 tty-reader 任务
  - 移除 tty-reader copier 任务（ldisc.rs）
  - 移除 register_irq_waker(10, tty_reader_waker)

<!-- P3.3 --> - [ ] 替代 Console TX/RX API
  - 重定向 axhal::console::write_bytes → AsyncUart tx_buf
  - 重定向 axhal::console::read_bytes → AsyncUart rx_buf
  - 或完全删除 Console API

<!-- P3.4 --> - [ ] 修改 Console 相关代码
  - ntty.rs: Console TtyWrite/TtyRead trait 实现
  - ldisc.rs: InputReader 移除或修改
  - entry.rs: Console 初始化代码

<!-- P3.5 --> - [ ] Gate P3 验证
  - Console 软件路径完全剔除
  - AsyncUart 独占 UART 硬件
  - IRQ 10 独占给 AsyncUart copier

**Gate P3**: Console 软件路径剔除完成 + AsyncUart 独占 UART

---

## P4: VFS 集成验证

> 目标: AsyncUart 设备注册到 VFS，提供用户态 API

<!-- P4.1 --> - [ ] 实现 DeviceOps trait
  - 创建 kernel/src/drivers/device_ops.rs
  - 实现 AsyncUartDevice（read_at/write_at/as_pollable）
  - 实现 Pollable trait（poll/select/epoll 支持）

<!-- P4.2 --> - [ ] 注册设备到 devfs
  - 在 pseudofs/dev/mod.rs builder() 中注册 async_uart 设备
  - 设备类型：CharacterDevice
  - 设备 ID：实验性 ID（如 4, 64）

<!-- P4.3 --> - [ ] 用户态 API 验证
  - 用户态程序打开 /dev/async_uart
  - read/write 数据收发正常
  - poll/select/epoll 事件通知正常

<!-- P4.4 --> - [ ] Gate P4 验证
  - DeviceOps trait 实现正确
  - 设备注册成功
  - 用户态 API 可用

**Gate P4**: VFS 集成完成 + 用户态 API 可用

---

## P5: 性能优化

> 目标: 性能基准达标，IRQ 频率优化

<!-- P5.1 --> - [ ] IRQ 频率优化
  - 监控 IRQ 10 触发频率
  - 验证无 IRQ 风暴（频率 < 100 Hz）

<!-- P5.2 --> - [ ] TX 吞吐量测试
  - 发送 1MB 数据，测量吞吐量
  - 目标：> 10 KB/s @115200

<!-- P5.3 --> - [ ] RX 延迟测试
  - 测量 RX 数据到达延迟
  - 目标：< 500 µs

<!-- P5.4 --> - [ ] CPU 利用率测试
  - 无数据时 CPU 利用率
  - 目标：0%（空闲）

**Gate P5**: 性能基准达标 + IRQ 频率正常

---

## P6: 真板验证

> 目标: 在 VisionFive2 真实硬件上验证

<!-- P6.1 --> - [ ] VisionFive2 平台适配
  - 确认 UART 型号（是否 16550 兼容）
  - 适配 UART MMIO 地址和 IRQ 号

<!-- P6.2 --> - [ ] 真板串口验证
  - 交叉编译
  - 真板串口收发测试

**Gate P6**: 真板串口收发正常

---

## 依赖关系

```
P0（项目规划）
  ↓
P1（UART 初始化替代）
  ↓
P2（异步串口架构）
  ↓
P3（Console 剔除）
  ↓
P4（VFS 集成）
  ↓
P5（性能优化）
  ↓
P6（真板验证）
```

---

## 阻塞项

<!-- 添加时格式: <!-- T{编号} --> - {阻塞描述} - {原因} -->

<!-- PB1 --> - 无

---

## 已完成 Milestone（feat/uart-async 分支，归档）

**原 feat/uart-async 分支的 Milestone（渐进式集成方案）已归档**：
- M0: 基础设施就绪 ✅
- M1: 架构验证 ✅
- M2: VFS 验证 ✅
- M3: 异步引擎实现（未集成）⚠️ 回滚
- M4: 性能优化（暂停）
- M5: 真板验证（暂停）

**归档原因**：分支策略变更，从渐进式集成改为完全剔除 Console 方案。

**相关文档**：
- 渐进式集成设计文档：`.claude/docs/superpowers/specs/2026-05-28-async-uart-integration-design.md`
- Console UART 研究文档：`docs/analysis/console-uart-mechanism.md`（保留作为参考）