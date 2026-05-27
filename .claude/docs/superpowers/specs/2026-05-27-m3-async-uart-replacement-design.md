# M3 AsyncUart 异步引擎替换设计

> Generated: 2026-05-27
> Status: Approved Design Spec
> Milestone: M3 (Async Engine Replacement)

---

## 1. 目标与范围

### 1.1 核心目标

M3 的核心目标是替换 Console 底层为 AsyncUart 异步引擎，实现：

- **用户态高性能输出**：异步发送，无 CPU 空转
- **内核日志共存**：两条软件路径独立运行
- **调试通道可用**：AsyncUart 故障时 axhal::console 可用
- **Console 共用数据竞争解决**：软件路径分离（L74）

### 1.2 范围边界

**包含**：
- AsyncUart trait 定义与 Uart16550 实现
- ISR 实现（区分 RX/TX 中断）
- RX copier 与 TX copier 任务
- ConsoleDriver 替换为 AsyncUartDriver
- 共用硬件协调方案

**不包含（延后到 M4）**：
- 批量传输优化（NAPI 风格）
- 性能基准建立
- 真板验证（VisionFive2）
- DMA 探索

---

## 2. 架构概览

### 2.1 整体架构

M3 实现后，系统有两条独立的软件路径：

```
路径 A: 内核日志（同步阻塞，earlycon）
  axlog::info! → axhal::console::write_bytes → 直接 MMIO → UART THR
  （始终可用，独立于异步框架）

路径 B: 用户态 Console（异步，高性能）
  用户态 write → N_TTY.write_at → AsyncUart tx_buf → TX copier → UART THR
  用户态 read → N_TTY.read_at → AsyncUart rx_buf ← RX copier ← UART RBR
```

### 2.2 关键组件

| 组件 | 职责 | 位置 |
|------|------|------|
| AsyncUart trait | 高层抽象（try_read/try_write/中断控制） | kernel/src/drivers/serial/async_uart.rs |
| Uart16550 实现 | AsyncUart trait 实现 | kernel/src/drivers/serial/uart16550_impl.rs |
| ISR | 读 IIR 区分 RX/TX，禁用中断，唤醒 waker | kernel/src/drivers/serial/isr.rs |
| RX copier | 硬件 FIFO → rx_buf（任务上下文） | kernel/src/drivers/serial/async_driver.rs |
| TX copier | tx_buf → 硬件 FIFO（任务上下文） | kernel/src/drivers/serial/async_driver.rs |
| AsyncUartDriver | 替换 ConsoleDriver | kernel/src/drivers/serial/async_driver.rs |

### 2.3 共用硬件协调

- TX copier 独占 FIFO 操作权（避免与 axhal::console 竞态）
- axhal::console 逐字节发送，TX copier 批量发送，交错概率低
- 若竞态显著，可在 TX copier 写 FIFO 时临时禁用中断（M4 添加 spinlock）

---

## 3. AsyncUart Trait 设计

### 3.1 Trait 定义

```rust
/// Async UART abstraction for high-performance serial communication
pub trait AsyncUart: Send {
    /// Try to read bytes from hardware FIFO (non-blocking)
    /// Returns number of bytes actually read (0 if no data available)
    fn try_read(&mut self, buf: &mut [u8]) -> usize;

    /// Try to write bytes to hardware FIFO (non-blocking)
    /// Returns number of bytes actually written (0 if FIFO full)
    fn try_write(&mut self, data: &[u8]) -> usize;

    /// Enable RX interrupt (Received Data Available)
    fn enable_rx_intr(&mut self);

    /// Disable RX interrupt
    fn disable_rx_intr(&mut self);

    /// Enable TX interrupt (Transmitter Holding Register Empty)
    fn enable_tx_intr(&mut self);

    /// Disable TX interrupt
    fn disable_tx_intr(&mut self);

    /// Get interrupt identification (IIR register)
    /// Returns None if no interrupt pending
    fn intr_identification(&mut self) -> Option<InterruptType>;

    /// Check if RX has data (LSR::DATA_READY)
    fn rx_ready(&mut self) -> bool;

    /// Check if TX FIFO is empty (LSR::THR_EMPTY)
    fn tx_ready(&mut self) -> bool;
}
```

### 3.2 设计要点

- **高层抽象**：不暴露寄存器细节，封装 UART 操作
- **Non-blocking API**：try_read/try_write 对应 uart_16550 的 receive_bytes/send_bytes
- **中断控制**：enable/disable rx_intr/tx_intr 对应 IER 寄存器操作
- **状态检查**：rx_ready/tx_ready 对应 LSR 寄存器检查

### 3.3 Uart16550 实现

**实现策略**：
- 直接调用 uart_16550 v0.6.0 的现有 API
- `try_read` → `uart.receive_bytes(buf)`
- `try_write` → `uart.send_bytes(data)`
- `enable_rx_intr` → 写 IER 寄存器（设置 Received Data Available Interrupt Enable）
- `enable_tx_intr` → 写 IER 寄存器（设置 Transmitter Holding Register Empty Interrupt Enable）
- `intr_identification` → 读 ISR 寄存器，解析 InterruptType
- `rx_ready` → 读 LSR 寄存器，检查 DATA_READY bit
- `tx_ready` → 读 LSR 寄存器，检查 THR_EMPTY bit

---

## 4. ISR 与 Copier 设计

### 4.1 ISR 设计

**ISR 极简原则（ADR-008）**：
1. 读 IIR → identify interrupt type
2. Disable triggered interrupt (prevent re-entry)
3. Wake corresponding waker
4. Exit immediately

**实现代码**：

```rust
/// UART Interrupt Service Routine
fn uart_isr(irq: usize) {
    // 1. Read IIR
    let intr_type = uart.intr_identification();

    match intr_type {
        Some(InterruptType::RxDataAvailable) => {
            // 2. Disable RX interrupt
            uart.disable_rx_intr();
            // 3. Wake RX waker
            rx_waker.wake();
        }
        Some(InterruptType::TxHoldingRegisterEmpty) => {
            // 2. Disable TX interrupt
            uart.disable_tx_intr();
            // 3. Wake TX waker
            tx_waker.wake();
        }
        // Other interrupt types (ModemStatus, LineStatus) ignored
        _ => {}
    }
    // 4. Exit immediately
}
```

**关键点**：
- ISR 不操作数据（遵循 ADR-008）
- 区分 RX/TX 中断，唤醒对应 waker
- 禁用已触发中断，防止 re-entry
- 使用 AtomicWaker（ISR 安全）

### 4.2 RX Copier 设计

**Poll_fn 模式**：

```rust
/// RX copier: Hardware FIFO → rx_buf
fn rx_copier(driver: &Arc<AsyncUartDriver>) {
    block_on(poll_fn(|cx| {
        let mut tmp_buf = [0u8; 256];

        // 1. Read from hardware FIFO
        let n = driver.uart.try_read(&mut tmp_buf);

        // 2. Write to rx_buf
        if n > 0 {
            driver.buffer.push_rx(&tmp_buf[..n]);
        }

        // 3. Re-enable RX interrupt
        driver.uart.enable_rx_intr();

        // 4. Register IRQ waker for next interrupt
        register_irq_waker(driver.irq, cx.waker());

        // 5. Check again before pending (avoid race)
        let n2 = driver.uart.try_read(&mut tmp_buf);
        if n2 > 0 {
            driver.buffer.push_rx(&tmp_buf[..n2]);
        }

        // 6. Return Pending
        Poll::Pending
    }))
}
```

**关键点**：
- 复用 M1/M2 已验证的 poll_fn 模式
- 数据搬运在任务上下文（安全）
- 中断处理后重新使能 RX 中断
- 避免 race condition 的二次检查

### 4.3 TX Copier 设计

**Poll_fn 模式**：

```rust
/// TX copier: tx_buf → Hardware FIFO
fn tx_copier(driver: &Arc<AsyncUartDriver>) {
    block_on(poll_fn(|cx| {
        let mut tmp_buf = [0u8; 256];

        // 1. Pop from tx_buf
        let n = driver.buffer.pop_tx(&mut tmp_buf);

        if n > 0 {
            // 2. Write to hardware FIFO
            let sent = driver.uart.try_write(&tmp_buf[..n]);

            // 3. If sent < n, FIFO full → enable TX interrupt
            if sent < n {
                driver.uart.enable_tx_intr();
                // Push remaining back to tx_buf
                driver.buffer.push_tx(&tmp_buf[sent..n]);
            }

            // 4. If all sent, check if tx_buf has more
            let remaining = driver.buffer.tx_len();
            if remaining > 0 {
                driver.uart.enable_tx_intr();
            } else {
                // All data sent → disable TX interrupt
                driver.uart.disable_tx_intr();
            }
        }

        // 5. Register IRQ waker (if TX interrupt enabled)
        // Note: register_irq_waker supports multiple wakers (PollSet)
        register_irq_waker(driver.irq, cx.waker());

        // 6. Return Pending
        Poll::Pending
    }))
}
```

**关键点**：
- TX copier 负责使能/禁用 THREI
- 空闲时禁用 THREI（避免无用中断）
- FIFO full 时使能 THREI（等待发送）
- register_irq_waker 支持多个 waker（learned.md L64）

---

## 5. ConsoleDriver 替换方案

### 5.1 AsyncUartDriver 结构

```rust
pub struct AsyncUartDriver {
    uart: Mutex<Uart16550<MmioBackend>>,
    buffer: Arc<AsyncBuffer>,
    irq: usize,
    rx_waker: AtomicWaker,  // ISR 唤醒 RX copier
    tx_waker: AtomicWaker,  // ISR 唤醒 TX copier
    rx_copier_started: AtomicBool,
    tx_copier_started: AtomicBool,
}
```

**与 ConsoleDriver 对比**：

| 组件 | ConsoleDriver (M1/M2) | AsyncUartDriver (M3) |
|------|------------------------|----------------------|
| RX 底层 | axhal::console::read_bytes | uart.try_read |
| TX 底层 | axhal::console::write_bytes | uart.try_write |
| RX copier | Console.read_bytes → rx_buf | uart.try_read → rx_buf |
| TX 机制 | flush_tx_sync（同步阻塞） | TX copier（异步） |
| ISR | Console tty-reader | 自定义 uart_isr |

### 5.2 初始化流程

```rust
impl AsyncUartDriver {
    pub fn new(mmio_addr: usize, irq: usize) -> Arc<Self> {
        // 1. Create Uart16550<MmioBackend>
        let uart = unsafe {
            Uart16550::new_mmio(NonNull::new(mmio_addr as *mut u8).unwrap(), 4)
        };

        // 2. Initialize UART
        uart.init(Config {
            baud_rate: BaudRate::Baud115200,
            fifo_trigger_level: Some(FifoTriggerLevel::TriggerLevel14),
            interrupts: InterruptEnable::RECEIVED_DATA_AVAILABLE,
            ..Default::default()
        });

        // 3. Create driver
        let driver = Arc::new(Self {
            uart: Mutex::new(uart),
            buffer: Arc::new(AsyncBuffer::new_default()),
            irq,
            rx_waker: AtomicWaker::new(),
            tx_waker: AtomicWaker::new(),
            rx_copier_started: AtomicBool::new(false),
            tx_copier_started: AtomicBool::new(false),
        });

        // 4. Register ISR hook
        axhal::register_irq_hook(irq, uart_isr);

        // 5. Start copier tasks
        driver.start_rx_copier();
        driver.start_tx_copier();

        driver
    }
}
```

### 5.3 替换策略

**替换 `/dev/console` 的 TtyWrite/TtyRead**：
- N_TTY 绑定的 TtyWrite/TtyRead 实现改为使用 AsyncUartDriver
- `write_at` → AsyncUartDriver.buffer.push_tx + wake tx_wakers
- `read_at` → AsyncUartDriver.buffer.pop_rx
- DeviceOps trait 保持不变

---

## 6. 共用硬件协调方案

### 6.1 竞争风险分析

**两条路径竞争**：
- axhal::console（内核日志）→ 直接 MMIO → THR
- TX copier（用户态）→ uart.try_write → THR

**竞争场景**：
- 内核日志正在逐字节发送 THR
- TX copier 同时批量写入 THR
- 数据交错或丢失

### 6.2 协调策略

**方案 A（推荐）**：TX copier 独占 FIFO 操作权
- TX copier 在写 FIFO 时临时禁用中断（spinlock_irqsave）
- axhal::console 的 write_bytes 是原子循环（逐字节等待 THRE）
- 自然交错，竞态概率低
- **验证方式**：M3 功能验证分支测试竞态是否显著

**方案 B（保守）**：添加 spinlock
- TX copier 和 axhal::console 共享一个 spinlock
- 写 THR 前获取锁，写完释放
- 保证绝对无竞态
- **何时使用**：方案 A 验证时发现竞态显著

### 6.3 实现细节

**方案 A 实现**：

```rust
fn tx_copier(driver: &Arc<AsyncUartDriver>) {
    block_on(poll_fn(|cx| {
        // ...

        // Write to FIFO with interrupt disabled
        let sent = {
            let _guard = spinlock_irqsave();  // 临时禁用中断
            driver.uart.try_write(&tmp_buf[..n])
        };

        // ...
    }))
}
```

**axhal::console 保持不变**：
- write_bytes 已是原子循环，无需修改
- 竞态概率低，方案 A 优先

---

## 7. 测试策略与功能验证

### 7.1 Gate M3 验证清单

| 验证项 | 内容 | 方式 |
|--------|------|------|
| AsyncUart trait | try_read/try_write/中断控制 API | 内核单元测试 |
| ISR | 区分 RX/TX 中断并唤醒 waker | 中断触发测试 |
| RX copier | 硬件 FIFO → rx_buf | 数据接收测试 |
| TX copier | tx_buf → 硬件 FIFO | 数据发送测试 |
| 用户态异步 | read/write 无 CPU 空转 | 性能测量 |
| 内核日志共存 | 两条路径同时运行 | 共存测试 |
| 调试通道可用 | AsyncUart 故障时 axhal::console 可用 | 故障注入测试 |
| Console 共用数据竞争消失 | 软件路径分离（L74） | Shell 竞争测试 |

### 7.2 功能验证分支策略

**创建 feat/uart-async-m3 功能验证分支**：
- 添加内核内部测试代码（`kernel/src/drivers/serial/m3_test.rs`）
- 内核启动时自动执行（无需用户态部署）
- QEMU 环境（暂无真板）

**测试内容**：
1. **AsyncUart API 测试**：
   - try_read/try_write 正确返回字节数
   - 中断控制 API 正确使能/禁用

2. **ISR 测试**：
   - RX 中断触发 → rx_waker 唤醒 → RX copier 执行
   - TX 中断触发 → tx_waker 唤醒 → TX copier 执行

3. **RX copier 测试**：
   - 输入数据 → rx_buf 有数据 → 用户态 read 成功

4. **TX copier 测试**：
   - 用户态 write → tx_buf 有数据 → TX copier 发送 → Console 输出

5. **共存测试**：
   - 内核日志（axlog::info!）+ 用户态输出同时运行
   - 无数据交错或丢失

6. **性能初步测量**：
   - Echo 回环测试 10s 稳定无丢失
   - CPU 利用率测量（无 CPU 空转）

### 7.3 回滚策略

**失败时回滚到 M2 状态**：
- AsyncUartDriver 替换失败 → 回退到 ConsoleDriver
- ISR 注册失败 → 回退到 Console tty-reader
- copier 任务失败 → 回退到 flush_tx_sync
- Git 分支策略：feat/uart-async(m3) → 验证失败 → 回退到 feat/uart-async(m1)

---

## 8. M4 优化基础

### 8.1 M3 完成后的优化基础

| 基础 | 内容 | M4 优化方向 |
|------|------|-------------|
| AsyncUart trait | 已定义，Uart16550 实现 | 扩展 DwApbUart 等硬件 |
| ISR/copier 模型 | 已验证 | NAPI 批量处理优化 |
| 性能基准 | Echo 回环初步测量 | 吞吐量/延迟精确测量 |
| 共用硬件协调 | 方案 A 验证竞态 | 若显著，添加 spinlock（方案 B） |

### 8.2 M4 优化清单

| 优化项 | 内容 | 预期提升 |
|--------|------|----------|
| 批量传输 | uart_16550 try_receive_batch/try_send_batch | 减少 MMIO 次数 |
| NAPI 风格 | 中断触发后批量轮询 FIFO 残留 | 高波特率 IRQ 频率降低 |
| TX coalescing | 多个短 write 合并为一次 tx_buf 写入 | 减少唤醒频率 |
| 空闲 CPU 零占用 | 无数据时 copier 任务挂起，CPU 进入 WFI | CPU 利用率 0% |
| 性能基准建立 | 吞吐量 @115200 > 10 KB/s, 延迟 P50 < 500 µs | 量化验证 |

---

## 9. 架构决策更新

### 9.1 新增 ADR

**ADR-017 (2026-05-27)**: M3 实现方案选择
- **决策**：采用方案 A"一步替换"，ISR 区分 RX/TX + 双 copier
- **原因**：功能完整，一步到位，Console 共用数据竞争立即解决
- **影响**：实现复杂度较高，但风险可控（M1/M2 已验证基础架构）

**ADR-018 (2026-05-27)**: 共用硬件协调策略
- **决策**：方案 A"TX copier 独占 FIFO 操作权"，临时禁用中断
- **原因**：竞态概率低（axhal::console 逐字节，TX copier 批量），实验验证
- **影响**：若竞态显著，M4 添加 spinlock（方案 B）

---

## 10. 附录

### 10.1 uart_16550 v0.6.0 API 参考

| API | 用途 | AsyncUart 对应 |
|------|------|------|
| `receive_bytes(buf)` | Non-blocking read | `try_read(buf)` |
| `send_bytes(data)` | Non-blocking write | `try_write(data)` |
| `ier()` | Read IER register | `enable/disable_rx_intr/tx_intr` 实现 |
| `isr()` | Read ISR register | `intr_identification()` |
| `lsr()` | Read LSR register | `rx_ready()` / `tx_ready()` |

### 10.2 参考文档

- **ADR-008**: ISR → AtomicWaker → copier 任务模型
- **ADR-014**: Console 统一策略（内核同步 + 用户态异步）
- **ADR-015**: 渐进式开发策略
- **L64**: register_irq_waker 共存机制（PollSet 支持多个 waker）
- **L74**: Console 共用数据竞争现象

---

## 11. 验证通过条件

**Gate M3 通过条件**：
1. AsyncUart trait 实现正确（编译通过）
2. ISR 正确区分 RX/TX 中断（中断触发测试）
3. RX/TX copier 任务正常运行（数据搬运测试）
4. 用户态 read/write 异步化（无 CPU 空转）
5. 内核日志与用户态输出共存（无数据交错）
6. 调试通道可用（AsyncUart 故障时 axhal::console 可用）
7. Console 共用数据竞争消失（Shell 竞争测试）
8. Echo 回环测试 10s 稳定无丢失

**失败条件**：
- 任一验证项未通过 → STOP → 分析原因 → 回滚或修复

---

**End of Spec Document**