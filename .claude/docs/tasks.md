# tasks.md — 任务追踪

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。

---

## Milestone 概览

| Milestone | 目标 | 底层引擎 | Gate | 依赖 | 建议 |
|-----------|------|----------|------|------|------|
| **M0** | 基础设施就绪 | — | `make run` + 中断回调触发 | — | 必做 |
| **M1** | 架构验证 | Console（同步） | Ring Buffer + 中断 + copier 流程正确 | M0 | 必做 |
| **M2** | VFS 验证 | Console（同步） | DeviceOps + 设备注册 + poll/epoll | M1 | 必做 |
| **M3** | 异步引擎替换 | AsyncUart | 用户态高性能 + 内核日志共存 | M2 | **关键替换点** |
| **M4** | 性能优化 | AsyncUart | 性能基准达标 | M3 | 优先 |
| **M5** | 真板验证 | AsyncUart | VisionFive2 实际验证 | M4 | 可并行 |
| **M6** | DMA 探索 | AsyncUart+DMA | DMA 通道可用（远期） | M4 | 远期 |

---

## M0: 基础设施就绪

> 目标: 让内核具备异步串口所需的所有底层能力——中断路由、AtomicWaker、uart_16550 本地依赖

<!-- T0.1 --> - [ ] 添加 uart_16550 本地 path 依赖到 kernel/Cargo.toml
  - 覆盖所有中断控制 API（set_interrupt_enable、interrupt_identification、InterruptType）
  - 与 axhal 中 uart_16550 v0.4.0 共存（两者操作不同硬件实例）
  - 验证: cargo check 编译通过

<!-- T0.2 --> - [ ] 添加 embassy-sync 依赖到 kernel/Cargo.toml
  - 仅引入 embassy-sync::AtomicWaker，不引入 executor/time
  - 验证与 nightly-2026-02-25 兼容性
  - 验证: cargo check 编译通过

<!-- T0.3 --> - [ ] QEMU 添加第二个串口
  - Makefile/qemu.mk 添加 `-serial mon:stdio` + 第二 `-serial` 配置
  - axconfig 添加第二 UART 的 MMIO 地址和 IRQ 号
  - 验证: QEMU 启动后两个串口均可见

<!-- T0.4 --> - [ ] UART 中断注册与回调触发验证
  - 通过 register_irq_waker 或 register_irq_hook 注册第二 UART 的 IRQ
  - ISR 读取 IIR 判断中断源（RX/TX），触发 AtomicWaker::wake()
  - 验证: 在 QEMU 中向第二串口发送字符，内核中断回调触发

**Gate M0**: `make run` 编译通过 + 第二串口中断回调可触发

---

## M1: 架构验证（底层用 Console 同步引擎）

> 目标: 验证基础架构（中断机制、Ring Buffer、copier 任务模型），底层暂时用 Console 同步引擎，调试能力保留

<!-- T1.1 --> - [ ] Ring Buffer 实现
  - rx_buf + tx_buf 各一个 HeapRb<u8>（默认 64 KiB）
  - rx_wakers + tx_wakers 各一个 PollSet
  - 验证: 单线程单元测试通过（push/pop/wake 流程）

<!-- T1.2 --> - [ ] 中断机制验证（IRQ 10 共存）
  - 确认 register_irq_waker 与现有 Console tty-reader 的共存语义
  - 若不支持多次注册，需设计统一中断分发机制
  - 验证: 中断回调触发，copier 任务唤醒

<!-- T1.3 --> - [ ] RX copier 任务模型验证
  - RX copier: poll_fn 循环，被唤醒后从 Console.read_bytes 读取，写入 rx_buf
  - 底层用 Console（同步），验证 copier 任务流程正确
  - 验证: 中断到来 → copier 任务唤醒 → rx_buf 有数据

<!-- T1.4 --> - [ ] TX 路径模拟验证
  - TX 暂用 Console.write_bytes（同步阻塞），验证 tx_buf → Console 流程
  - 不实现真正的 TX copier（M3 再做）
  - 验证: write → tx_buf → Console 输出正常

**Gate M1**: 架构验证通过（Ring Buffer + 中断 + copier 任务流程正确），调试输出保留

---

## M2: VFS 验证（Console 包装）

> 目标: 验证 VFS 集成（DeviceOps + 设备注册），设备底层用 Console 同步引擎

<!-- T2.1 --> - [ ] ConsoleDriver 实现 DeviceOps
  - read_at: block_on(poll_io(...)) → 从 rx_buf 读取（M1 已建立）
  - write_at: block_on(poll_io(...)) → 写入 tx_buf → Console.write_bytes
  - as_pollable: 返回 Some(self)，支持 poll/select/epoll
  - 验证: DeviceOps 接口编译通过

<!-- T2.2 --> - [ ] 注册测试设备到 devfs
  - 在 pseudofs/dev/mod.rs 注册测试设备（如 /dev/async_test）
  - 不替换 /dev/console（保留 Console 调试能力）
  - 验证: 内核启动后测试设备可 open

<!-- T2.3 --> - [ ] 用户态验证
  - 用户态程序 open 测试设备 → read → write
  - poll/epoll 监听事件
  - 验证: 用户态读写正常，poll/epoll 可用

<!-- T2.4 --> - [ ] termios 支持框架（可选）
  - 默认 raw 模式零开销
  - ioctl TCGETS/TCSETS 框架预留
  - 验证: raw 模式数据正确

**Gate M2**: VFS 集成验证通过（DeviceOps + 设备注册 + poll/epoll），调试输出保留

---

## M3: 异步引擎替换（关键替换点）

> 目标: 替换 Console 底层为 AsyncUart 异步引擎，实现真正的中断驱动高性能串口

<!-- T3.1 --> - [ ] AsyncUart trait + Uart16550 实现
  - 定义 AsyncUart trait（try_read/try_write/enable_rx_intr/disable_rx_intr/...）
  - Uart16550<MmioBackend> 实现 AsyncUart
  - 验证: cargo check 通过，MMIO 操作封装正确

<!-- T3.2 --> - [ ] ISR → AtomicWaker → copier 任务模型实现
  - ISR: 读 IIR → 禁用已触发中断 → AtomicWaker.wake()
  - RX copier: 真正的硬件 FIFO → rx_buf
  - TX copier: tx_buf → 真正的硬件 FIFO，使能 TX 中断
  - 验证: 中断到来 → copier 任务唤醒 → 数据搬运完成

<!-- T3.3 --> - [ ] 替换 Console 底层
  - ConsoleDriver 底层从 Console 同步引擎 → AsyncUart 异步引擎
  - 内核日志仍通过 axhal::console 输出（earlycon 独立路径）
  - 验证: 用户态 write 异步化，无 CPU 空转

<!-- T3.4 --> - [ ] 确认调试安全通道
  - axhal::console 作为"earlycon"始终可用（独立于异步框架）
  - AsyncUart 故障时内核日志仍能输出 panic/调试信息
  - 验证: 故障场景下仍有输出渠道

<!-- T3.5 --> - [ ] 性能验证
  - Echo 回环测试 10s 稳定无丢失
  - 吞吐量初步测量
  - 验证: 异步引擎工作正常

**Gate M3**: 异步引擎替换完成，用户态高性能输出 + 内核日志共存 + 调试通道可用

---

## M4: 性能优化

> 目标: 性能基准达标，CPU 空闲时零占用，吞吐量接近线速

<!-- T4.1 --> - [ ] 批量传输优化
  - uart_16550 try_receive_batch/try_send_batch API（减少逐字节 MMIO）
  - write coalescing: 多个短 write 合并为一次 tx_buf 写入
  - 验证: 吞吐量提升可测量

<!-- T4.2 --> - [ ] NAPI 风格批量轮询
  - 中断触发后切换到轮询模式处理 FIFO 残留数据
  - 处理完毕后切回中断等待
  - 验证: 高波特率下 IRQ 频率降低，吞吐量不降

<!-- T4.5 --> - [ ] 中断分发效率优化
  - 评估 register_irq_waker BTreeMap 查找开销
  - 若开销显著，考虑 IRQ 号直接映射（数组索引）
  - 与 NAPI 批量处理协同减少 PLIC claim/complete MMIO 延迟
  - 验证: 中断处理延迟降低可测量

<!-- T4.3 --> - [ ] 空闲 CPU 零占用
  - 无数据时 copier 任务挂起（poll_fn 返回 Pending）
  - CPU 进入 WFI 休眠
  - 验证: 无数据 10s，CPU 统计显示 0% 串口占用

<!-- T4.4 --> - [ ] 性能基准建立
  - 吞吐量 @115200: > 10 KB/s (90% 线速)
  - 延迟 P50 < 500 µs, P99 < 2 ms
  - 数据完整性: 1 MB 随机数据 MD5 校验
  - 验证: 基准指标达标

<!-- T4.6 --> - [ ] PTY ringbuf 性能优化（可选）
  - PTY_BUF_SIZE 从 4096 增大到 64 KiB（与 AsyncUart 对齐）
  - 减少高频读写时的唤醒频率
  - 验证: SSH/tmux 场景吞吐量提升可测量
  - 参考: kernel/src/pseudofs/dev/tty/pty.rs PTY_BUF_SIZE

**Gate M4**: 性能基准达标 + 与原轮询驱动对比有明显提升

---

## M5: 真板验证

> 目标: 在 VisionFive2 等真实硬件上验证异步串口驱动

<!-- T5.1 --> - [ ] VisionFive2 平台适配
  - 确认板上 UART 型号（是否 16550 兼容）
  - AsyncUart trait 支持 DwApbUart 等不同硬件
  - 验证: 交叉编译通过

<!-- T5.2 --> - [ ] 真板中断验证
  - PLIC 中断号与 QEMU 不同，需适配
  - 验证: 板上中断回调触发

<!-- T5.3 --> - [ ] 真板串口收发验证
  - echo 回环测试
  - 性能基准测试
  - 验证: 板上串口工作正常

**Gate M5**: 真板串口收发正常 + 性能数据可信

---

## M6: DMA 探索（远期）

> 目标: 探索 DMA 传输通道，为极高吞吐量场景做准备

<!-- T6.1 --> - [ ] DMA 通道可行性评估
  - 评估 virtio-console DMA 模式是否可用于串口数据传输
  - 评估 StarryOS 内存管理是否支持 PageBox 对齐分配
  - 验证: 可行性报告输出

<!-- T6.2 --> - [ ] DMA 缓冲区管理（如果可行）
  - PageBox 对齐分配
  - 物理地址映射
  - 验证: 分配/释放正确

<!-- T6.3 --> - [ ] 流式 DMA 收发（如果可行）
  - 零拷贝读取路径
  - 大数据块传输校验
  - 验证: 数据校验通过

<!-- T6.4 --> - [ ] DMA + 中断混合策略
  - 小数据走中断，大数据走 DMA
  - 阈值可配置
  - 验证: 混合模式切换正确

**Gate M6**: 1 MB 数据 DMA 传输校验通过 + 与纯中断模式性能对比

---

## 依赖关系

```
M0 → M1 → M2（架构 + VFS 验证，底层用 Console 同步引擎）
               │
               └→ M3（异步引擎替换，关键替换点）
                     │
                     ├→ M4（性能优化）
                     │      ├→ M5（真板验证）
                     │      └→ M6（DMA 远期）
```

**渐进式策略（ADR-015）**：

1. **M0 → M1 → M2**：验证基础架构（底层用 Console 同步引擎，调试能力保留）
2. **M3**：异步引擎替换（一步到位，风险集中但可控）
3. **M3 完成后**：
   - **M4 优先**：性能优化，基于 AsyncUart
   - **M5 可并行**：真板验证
   - **M6 远期**：DMA 探索

**风险控制**：
- M1/M2 验证时调试能力保留（Console 同步输出）
- M3 是关键替换点，但前置验证已完成，失败可回滚
- 异步引擎 bug 不影响内核日志输出（earlycon 独立路径）

---

## 阻塞项

<!-- 添加时格式: <!-- T{编号} --> - {阻塞描述} - {原因} -->

<!-- TB1 --> - 无