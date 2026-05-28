# tasks.md — 任务追踪

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。

---

## Milestone 概览

| Milestone | 目标 | 底层引擎 | Gate | 依赖 | 建议 |
|-----------|------|----------|------|------|------|
| **M0** | 基础设施就绪 | — | `make run` + 中断回调触发 | — | 必做 ✅ |
| **M1** | 架构验证 | Console（同步） | Ring Buffer + 中断 + copier 流程正确 | M0 | 必做 ✅ |
| **M2** | VFS 验证 | Console（同步） | DeviceOps + 设备注册 + poll/epoll | M1 | 必做 ✅ |
| **M3** | 异步引擎实现（未集成） | AsyncUart | **⚠️ 替换失败回滚**（IRQ 风暴 + TX busy-loop） | M2 | **待重新设计** |
| **M4** | 性能优化 | AsyncUart | 性能基准达标 | M3 | 优先（暂停） |
| **M5** | 真板验证 | AsyncUart | VisionFive2 实际验证 | M4 | 可并行（暂停） |
| **M6** | DMA 探索 | AsyncUart+DMA | DMA 通道可用（远期） | M4 | 远期 |

---

## M0: 基础设施就绪

> 目标: 让内核具备异步串口所需的所有底层能力——依赖、编译验证、中断机制确认

<!-- T0.1 --> - [x] 添加 uart_16550 本地 path 依赖到 kernel/Cargo.toml
  - 覆盖所有中断控制 API（set_interrupt_enable、interrupt_identification、InterruptType）
  - 路径: `../../uart_16550`（本地 v0.6.0）
  - 验证: cargo check 编译通过 ✅

<!-- T0.2 --> - [x] 添加 embassy-sync 依赖到 kernel/Cargo.toml
  - 仅引入 embassy-sync::AtomicWaker，不引入 executor/time
  - 验证与 nightly-2026-02-25 兼容性
  - 验证: cargo check 编译通过 ✅

<!-- T0.3 --> - [x] 中断机制确认（IRQ 10 共存语义）
  - 确认 register_irq_waker 与现有 Console tty-reader 的共存语义
  - 查看源码或实验验证：同一 IRQ 是否支持多次注册
  - 验证: 明确共存/冲突处理方案 ✅

<!-- T0.4 --> - [x] Gate M0 验证
  - `make run` 编译通过
  - 内核启动正常（Console 调试输出可用）
  - 验证: 基础依赖就绪，可进入 M1 ✅

**Gate M0**: 编译通过 + 内核启动 + 中断共存方案明确

---

## M1: 架构验证（底层用 Console 同步引擎） ✅ 完成

> 目标: 验证基础架构（中断机制、Ring Buffer、copier 任务模型），底层暂时用 Console 同步引擎，调试能力保留
> 完成时间: 2026-05-27

<!-- T1.1 --> - [x] Ring Buffer 实现 ✅
  - rx_buf + tx_buf 各一个 HeapRb<u8>（默认 64 KiB）
  - rx_wakers + tx_wakers 各一个 PollSet
  - 验证: AsyncBuffer 结构正确，编译通过 ✅

<!-- T1.2 --> - [x] 中断机制验证（IRQ 10 共存） ✅
  - 确认 register_irq_waker 与现有 Console tty-reader 的共存语义
  - 验证: RX copier 任务唤醒，数据到达 rx_buf ✅

<!-- T1.3 --> - [x] RX copier 任务模型验证 ✅
  - RX copier: poll_fn 循环，被唤醒后从 Console.read_bytes 读取，写入 rx_buf
  - 底层用 Console（同步），验证 copier 任务流程正确
  - 验证: 中断到来 → copier 任务唤醒 → rx_buf 有数据 ✅

<!-- T1.4 --> - [x] TX 路径模拟验证 ✅
  - TX 暂用 Console.write_bytes（同步阻塞），验证 tx_buf → Console 流程
  - 验证: write → tx_buf → Console 输出正常 ✅

<!-- T1.5 --> - [x] 设备注册到 devfs ✅
  - 在 pseudofs/dev/mod.rs builder() 中注册 async_uart_test 设备
  - 验证: `/dev/async_uart_test` 可打开 ✅

<!-- T1.6 --> - [x] Gate M1 验证 ✅
  - `make run` 编译通过 + 内核启动
  - 设备可打开、读写正常
  - 验证: TX/RX 数据流正常 ✅

**Gate M1**: ✅ 架构验证通过（Ring Buffer + 中断 + copier 任务流程正确）
**已知约束**: Console 共用数据竞争（L74 已记录），M3 解决
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

## M2: VFS 验证（Console 包装） ✅ 验证通过

> 目标: 验证 VFS 集成（DeviceOps + 设备注册 + poll），设备底层用 Console 同步引擎
> 验证方式: 内核内部测试（feat/uart-async-m2 分支）
> 验证时间: 2026-05-27

**验证策略**：
- 在 feat/uart-async-m2 分支添加内核内部测试代码
- 测试文件: `kernel/src/drivers/serial/test.rs` (M2 验证测试模块)
- 测试方式: 内核启动时自动执行，无需用户态程序
- 测试结果: 所有自动化测试通过 ✅

**验证结果**：
- ✅ Device creation (AsyncUartTestDevice 创建成功)
- ✅ write_at (TX path) (write_at 返回 Ok(36)，Console 输出可见)
- ✅ Pollable trait (as_pollable() 返回 Some，poll 返回 IoEvents(OUT))
- ℹ️ read_at (RX) 跳过（需手动输入触发）
- ℹ️ devfs registration 提示手动检查

<!-- T2.1 --> - [x] ConsoleDriver 实现 DeviceOps ✅ (已在 M1 实现)
  - read_at/write_at 实现（已在 M1 实现）
  - as_pollable 支持 poll/select/epoll（已在 M1 实现）
  - 验证: DeviceOps 接口编译通过 ✅
  - M2 验证: write_at 成功，Console 输出正常 ✅

<!-- T2.2 --> - [x] 注册测试设备到 devfs ✅ (已在 M1 实现)
  - 在 pseudofs/dev/mod.rs builder() 中注册 async_uart_test 设备（已在 M1 实现）
  - 不替换 /dev/console（保留 Console 调试能力）✅
  - 验证: `/dev/async_uart_test` 设备注册成功 ✅

<!-- T2.3 --> - [x] 功能验证 ✅ (内核内部测试完成)
  - 验证 DeviceOps trait 实现正确 ✅
  - 验证 Pollable trait 实现正确 ✅
  - 验证 TX 路径正常（Console 输出可见）✅
  - 验证 poll IN/OUT 事件正确返回 ✅

<!-- T2.4 --> - [ ] termios 支持框架（可选，延后到 M3）
  - 默认 raw 模式零开销
  - ioctl TCGETS/TCSETS 框架预留
  - 延后原因: Console 共用问题（L74）待解决；M3 替换 AsyncUart 后统一实现（ADR-016）

**Gate M2**: ✅ VFS 集成验证通过（DeviceOps + 设备注册 + poll），调试输出保留
**验证分支**: feat/uart-async-m2 (内核内部测试代码)
**下一步**: 进入 M3 异步引擎替换

---

## M3: 异步引擎实现（⚠️ 替换失败回滚）

> **状态**：M3 Task 1-5 完成（AsyncUart 驱动代码实现），**Task 6+ 替换失败**
> **回滚原因**：IRQ 风暴 + TX busy-loop，UART 硬件状态异常（详见 ADR-019）
> **回滚点**：d29a28f（M3 Task 5 - module exports 完成）

### 已完成（Task 1-5） ✅

<!-- T3.1 --> - [x] AsyncUart trait + Uart16550 实现 ✅
<!-- T3.2 --> - [x] ISR（IsrContext + AtomicWaker）实现 ✅
<!-- T3.3 --> - [x] AsyncBuffer（Ring Buffer + PollSet）实现 ✅
<!-- T3.4 --> - [x] AsyncUartDriver（RX/TX copier）实现 ✅
<!-- T3.5 --> - [x] Module exports + 编译验证 ✅

### 替换失败（Task 6+） ❌ 2026-05-28

**失败症状**：
- **IRQ 风暴**：RX-COPIER 和 tty-reader 快速循环唤醒，IRQ 10 异常触发
- **TX busy-loop**：TX FIFO 满，UART 状态异常（LSR=0x00，THR_EMPTY=false TEMT=false）
- **UART 硕件未正常发送数据**：FIFO 满后 retry 无效

**根本问题（未完全明确）**：
- UART 硕件配置异常（Console 初始化后的状态不兼容 AsyncUart）
- 未验证 UART 状态（IIR、MCR、LSR）就开始集成
- 缺少全面的硬件状态调试信息

**教训**：见 ADR-019（architecture.md）

### 待重新设计

**替代方案（待评估）**：
1. 方案 A：添加全面的 UART 状态调试（IIR/MCR/LSR），诊断硬件问题
2. 方案 B：AsyncUart 启动时重新初始化 UART（uart.init()）
3. 方案 C：放弃 THRE interrupt，使用纯 polling TX
4. 方案 D：回到"软件路径分离"方案（Console 和 AsyncUart 共存）

**Gate M3**：⚠️ **阻塞** — 需重新设计整体方案

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