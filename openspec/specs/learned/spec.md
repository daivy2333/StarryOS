# learned/spec.md — 项目学习记忆

> 迁移自 .claude/docs/learned.md，2026-06-03
> 条目格式: L{编号} 标记开头，支持 grep 精确定位。

---

## Purpose

记录项目开发过程中学到的知识，避免重复探索，加速问题解决。

## Requirements

### Requirement: API 路径记录

项目中使用的关键 API 路径 MUST 记录，包含用途和使用示例。

#### Scenario: 发现新 API

- **WHEN** 开发者发现或使用了新的 API 端点
- **THEN** 必须记录到 learned/spec.md，包含：API 路径、用途、使用示例

**关键 API 路径**:

| API | 用途 | 示例 |
|-----|------|------|
| `axtask::future::block_on` | 异步任务阻塞执行 | `block_on(async { ... })` |
| `axtask::future::poll_io` | WouldBlock → register → await | `poll_io(\|cx\| ..., false).await` |
| `axtask::future::register_irq_waker` | 连接中断到异步任务唤醒 | `register_irq_waker(IRQ, waker)` |
| `embassy_sync::AtomicWaker::wake` | ISR 中安全唤醒 | `WAKER.wake()` |
| `axmm::iomap` | 设备 MMIO 映射 | `iomap(PhysAddr::from(0x10000000), 0x1000)` |
| `uart_16550::SerialPort::new_mmio` | 创建 UART 实例 | `new_mmio(NonNull::new(ptr), stride)` |

### Requirement: 踩坑经验记录

遇到的技术陷阱和解决方案 MUST 记录，防止重复踩坑。

#### Scenario: 解决棘手问题

- **WHEN** 开发者花费大量时间解决了一个技术问题
- **THEN** 必须记录踩坑档案，包含：症状、根因、解决方案、预防措施

**关键踩坑档案**:

1. **embassy-executor 与 axtask 冲突**
   - 症状: 引入完整 Embassy 后调度器冲突
   - 解: 只引入 embassy-sync::AtomicWaker

2. **HeapRb 非中断安全**
   - 症状: 在 ISR 中直接操作 ringbuf 导致数据竞争
   - 解: 硬件 FIFO 和 ringbuf 之间的搬运由单一后台协程完成

3. **LoadFault 根因：stride=4 错误**
   - 症状: 内核和 ISR 在 `0xffffffc010000008` 处 LoadFault
   - 根因: NS16550 寄存器仅 8 字节，stride=4 导致偏移越界
   - 解: 使用 stride=1

4. **M3 替换失败 — IRQ 风暴 + TX busy-loop**
   - 症状: IRQ 风暴 + TX FIFO 满 LSR=0x00
   - 根因: Console UART 状态不兼容 AsyncUart
   - 教训: 硬件集成前必须 dump 全部寄存器状态

5. **RX copier 与 Console tty-reader FIFO 竞争**
   - 症状: Shell 显示提示符但键盘输入无效
   - 根因: 两个 reader 都读同一个 UART RBR
   - 解: Q2 关闭 copier 让 Console 独占，Q3 替换后再启用

6. **TX copier 与 ax_println! 输出交错**
   - 症状: Shell 输出乱码
   - 根因: TX copier 批发送时 ax_println! 插队写 THR
   - 解: TX copier 用本地 cursor 追踪已发位置

### Requirement: 技巧模式记录

有效的开发技巧和模式 MUST 记录，促进知识共享。

#### Scenario: 发现高效做法

- **WHEN** 开发者发现了一种高效的开发技巧或模式
- **THEN** 必须记录到技巧模式区，包含：技巧名称、适用场景、使用方法

**关键技巧模式**:

1. **ISR 极简原则**: 清中断标志 → 唤醒 Waker → 立即退出
2. **poll_io 标准模式**: `poll_fn(\|cx\| match try_op() { Ok(v) => Ready(v), Err(WouldBlock) => { register(waker); Pending } })`
3. **设备注册到 devfs**: `builder.add_device("name", DeviceId::new(major, minor), Arc::new(Device::new(ops)))`
4. **UART 状态诊断**: 集成前必须 `info!(IIR={:02x} MCR={:02x} LSR={:02x})`
5. **iomap 设备 MMIO**: `axmm::iomap(PhysAddr::from(0x10000000), PAGE_SIZE_4K)`
6. **NAPI 中断合并**: 连续成功 ≥16 次后切轮询模式，batch=64
7. **AtomicWaker vs register_irq_waker**: 专用驱动用 AtomicWaker，通用框架用 register_irq_waker

### Requirement: 文件速查表

关键文件和目录的位置 MUST 记录，加速代码导航。

#### Scenario: 定位关键文件

- **WHEN** 开发者频繁访问某些文件或目录
- **THEN** 必须记录到文件速查表，包含：文件路径、用途、关键内容

**关键文件速查**:

| 文件 | 用途 |
|------|------|
| `kernel/src/drivers/serial/async_driver.rs` | AsyncUart 驱动核心实现 |
| `kernel/src/drivers/serial/isr.rs` | ISR 中断处理 |
| `kernel/src/drivers/serial/ntty_async.rs` | 异步 TTY 设备 |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | tty-reader copier |
| `kernel/src/drivers/benchmark.rs` | 性能测试模块 |
| `uart_16550/src/spec.rs` | 寄存器定义 |
| `uart_16550/src/backend/mmio.rs` | MMIO 后端实现 |
