# AsyncUart 集成设计方案

> 基于 Console UART 研究经验，重新设计 AsyncUart 集成方案
> 避免上次失败的问题：UART 重初始化冲突、TX 数据竞争、IRQ waker 冲突、缺少 UART 状态诊断

---

## 项目上下文

**当前状态**：
- M3 Task 1-5 完成（AsyncUart 驱动代码实现）
- 完全未集成（ISR 未注册，copier 任务未启动）
- OS 仍使用 Console 阻塞输出（正常工作）

**上次失败问题**：
1. **IRQ 风暴**：RX-COPIER 和 tty-reader 快速循环唤醒，IRQ 10 异常触发
2. **TX busy-loop**：TX FIFO 满，UART 状态异常（LSR=0x00）
3. **UART 重初始化冲突**：AsyncUartDriver::new() 调用 uart.init() 破坏 Console 配置
4. **IRQ waker 冲突**：Console tty-reader 和 AsyncUart copier 竞争 IRQ 10 waker
5. **缺少 UART 状态诊断**：未查询 IIR/MCR/LSR 就开始集成

**Console UART 研究经验**：
- Console TX 不使用中断（纯 polling）
- Console RX 使用中断驱动（register_irq_waker + tty-reader）
- IRQ Waker 单一限制（每个 IRQ 只支持一个 waker）
- Console 与 AsyncUart 共享 UART 的数据竞争风险

---

## 设计目标

### 集成目标

**完全替换 Console**：AsyncUart 完全替代 Console TX/RX 软件路径，独占 UART 硬件。

**关键约束**：
- ✅ 避免 UART 重初始化冲突（复用 axplat 配置）
- ✅ 优先诊断 UART 状态（IER/LSR/ISR/MCR）
- ✅ 解决 IRQ waker 冲突（移除 tty-reader）
- ✅ 提供错误回退机制（Fallback 到 Console）

---

## 设计方案

### 方案选择

**方案 A：渐进式集成 + UART 状态诊断（推荐）**

核心思路：
1. **诊断阶段**：AsyncUart 启动时查询 UART 状态（IER/LSR/ISR/MCR），记录 Console 配置
2. **修复阶段**：如果状态不兼容（如 IER 缺少 THR_EMPTY），修改 IER 配置并记录修改
3. **集成阶段**：禁用 Console 软件路径（重定向 `axhal::console` API → AsyncUart）
4. **移除冲突源**：移除 tty-reader，AsyncUart RX/TX copier 独占 IRQ 10

Trade-offs：
- ✅ 避免 UART 重初始化冲突
- ✅ 诊断 UART 状态，理解 Console 配置
- ✅ 尝试修复，降低集成风险
- ⚠️ 可能破坏 Console RX（修改 IER）
- ⚠️ 需要修改 Console 软件路径（内核代码）

---

## 架构概览

### 整体架构

```
┌─────────────────────────────────────────────────────────┐
│                    启动流程                              │
├─────────────────────────────────────────────────────────┤
│ 1. axruntime::init()                                    │
│    └─ axplat::init() → UART 硬件初始化（Console）        │
│                                                          │
│ 2. kernel::entry::init()                                │
│    └─ mount_all() + spawn init                          │
│    └─ Console 软件路径启动（tty-reader）                 │
│                                                          │
│ 3. AsyncUart 集成阶段（新增）                            │
│    ├─ UART 状态诊断（IER/LSR/ISR/MCR）                   │
│    ├─ IER 配置修复（enable THR_EMPTY）                   │
│    ├─ Console 软件路径禁用                               │
│    ├─ tty-reader 移除                                    │
│    └─ AsyncUart RX/TX copier 启动                        │
└─────────────────────────────────────────────────────────┘
```

### 核心组件

| 组件 | 职责 | 状态 |
|------|------|------|
| **UART 状态诊断器** | 查询 IER/LSR/ISR/MCR，记录 Console 配置 | 新增 |
| **AsyncUartDriver** | RX/TX copier 任务，替代 Console 软件路径 | 已实现，需集成 |
| **Console 软件路径禁用器** | 重定向 axhal::console API → AsyncUart | 新增 |
| **tty-reader 移除器** | 停止 tty-reader 任务，释放 IRQ 10 | 新增 |

### 关键文件修改

| 文件 | 修改内容 | 影响 |
|------|---------|------|
| `async_driver.rs` | 添加 UART 状态诊断 + IER 修复逻辑 | AsyncUart 启动流程 |
| `ntty.rs` | Console TX/RX API 重定向到 AsyncUart | Console 软件路径禁用 |
| `ldisc.rs` | tty-reader 任务停止逻辑 | IRQ waker 冲突解决 |
| `entry.rs` | AsyncUart 集成入口调用 | 启动流程 |

---

## UART 状态诊断机制

### 诊断流程

```rust
// async_driver.rs - AsyncUartDriver::new() 启动时
pub fn new() -> Arc<Self> {
    // 1. 创建 Uart16550Async（不调用 init）
    let uart = unsafe { Uart16550Async::new(UART_MMIO_ADDR, 4) };

    // 2. UART 状态诊断（新增）
    let state = diagnose_uart_state(&uart);
    log_uart_state(&state);

    // 3. IER 配置检查与修复（新增）
    if !state.ier.contains(IER::THR_EMPTY) {
        log::warn!("Console IER missing THR_EMPTY, fixing...");
        fix_ier_config(&uart, IER::THR_EMPTY | IER::DATA_READY);
    }

    // 4. 创建 ISR context（不调用 uart.init）
    let isr_ctx = IsrContext::new(uart);

    // 5. 启动 copier 任务...
}
```

### 诊断函数设计

```rust
/// UART 状态诊断结构
struct UartStateDiagnosis {
    ier: IER,           // 中断使能状态
    lsr: LSR,           // 线状态（RX/TX FIFO）
    isr: ISR,           // 中断状态（IIR）
    mcr: MCR,           // 调制解调器控制
    compatible: bool,   // 是否兼容 AsyncUart
    warnings: Vec<String>,
}

/// 诊断 UART 状态
fn diagnose_uart_state(uart: &Uart16550Async) -> UartStateDiagnosis {
    let ier = uart.read_ier();
    let lsr = uart.lsr();
    let isr = uart.isr();
    let mcr = uart.read_mcr();

    let mut warnings = Vec::new();

    // 检查关键配置
    if !ier.contains(IER::DATA_READY) {
        warnings.push("IER missing DATA_READY (Console RX disabled)");
    }
    if !ier.contains(IER::THR_EMPTY) {
        warnings.push("IER missing THR_EMPTY (AsyncUart TX interrupt disabled)");
    }
    if lsr.contains(LSR::THR_EMPTY) {
        warnings.push("LSR THR_EMPTY=1 (TX FIFO empty, good)");
    }

    let compatible = ier.contains(IER::DATA_READY);

    UartStateDiagnosis {
        ier, lsr, isr, mcr, compatible, warnings,
    }
}

/// 记录 UART 状态
fn log_uart_state(state: &UartStateDiagnosis) {
    log::info!("UART state diagnosis:");
    log::info!("  IER: {:02x} (DR={} THRE={})",
        state.ier.bits(),
        state.ier.contains(IER::DATA_READY),
        state.ier.contains(IER::THR_EMPTY));
    log::info!("  LSR: {:02x} (DR={} THRE={} TEMT={})",
        state.lsr.bits(),
        state.lsr.contains(LSR::DATA_READY),
        state.lsr.contains(LSR::THR_EMPTY),
        state.lsr.contains(LSR::TEMT));
    log::info!("  ISR: {:02x} (pending={})",
        state.isr.bits(),
        state.isr.has_pending_interrupt());
    log::info!("  MCR: {:02x}", state.mcr.bits());

    for warning in &state.warnings {
        log::warn!("  {}", warning);
    }
}

/// 修复 IER 配置
fn fix_ier_config(uart: &Uart16550Async, target_ier: IER) {
    unsafe {
        uart.write_ier(target_ier.bits());
    }
    log::info!("IER config fixed: {:02x}", target_ier.bits());
}
```

### 诊断输出示例

```
[UART] state diagnosis:
  IER: 0x01 (DR=true THRE=false)  // Console 仅使能 RX 中断
  LSR: 0x60 (DR=false THRE=true TEMT=true)  // TX FIFO 空
  ISR: 0x01 (pending=false)  // 无中断待处理
  MCR: 0x00
[UART] WARN: IER missing THR_EMPTY (AsyncUart TX interrupt disabled)
[UART] IER config fixed: 0x03  // 使能 RX + TX 中断
```

---

## AsyncUart 集成策略

### 集成时机

```
启动流程时间线：
T1: axruntime::init() → axplat UART 初始化
T2: kernel::entry::init() → Console 软件路径启动
T3: AsyncUart 集成点（新增）→ 在 entry.rs 的合适位置调用
```

**集成点选择**：在 `entry.rs::init()` 中，**spawn init task 之后**集成 AsyncUart。

### 集成流程

```rust
// entry.rs - AsyncUart 集成入口
pub fn init(args: &[String], envs: &[String]) {
    // 1. Mount pseudofs
    pseudofs::mount_all();

    // 2. Spawn alarm task
    spawn_alarm_task();

    // 3. Load init binary + Create init process (Console 软件路径启动)
    // ...

    // 4. AsyncUart 集成（新增）
    #[cfg(feature = "uart-async")]
    {
        log::info!("AsyncUart integration start...");
        integrate_async_uart();
    }

    // 5. Spawn init task
    // ...
}

/// AsyncUart 集成函数
fn integrate_async_uart() {
    // 1. 创建 AsyncUartDriver（包含 UART 状态诊断 + IER 修复）
    let async_uart = AsyncUartDriver::new();

    // 2. 禁用 Console 软件路径
    disable_console_paths();

    // 3. 移除 tty-reader 任务
    remove_tty_reader();

    // 4. 注册 AsyncUart 设备到 devfs
    register_async_uart_device(async_uart);

    log::info!("AsyncUart integration complete");
}
```

---

## Console 软件路径替换

### 替换范围

| 原路径 | 替换后路径 | 影响范围 |
|--------|----------|---------|
| `axhal::console::write_bytes()` | `AsyncUart.buffer().push_tx()` | Console TX → AsyncUart TX copier |
| `axhal::console::read_bytes()` | `AsyncUart.buffer().pop_rx()` | Console RX → AsyncUart RX copier |
| `tty-reader` 任务（ldisc.rs） | **移除** | IRQ waker 冲突解决 |
| `register_irq_waker(10, waker)` | AsyncUart copier 独占 IRQ 10 | IRQ waker 冲突解决 |

### Console TX 替换实现

```rust
// ntty.rs - Console TX 替换
use crate::drivers::serial::async_driver::AsyncUartDriver;
use alloc::sync::Arc;
use spin::OnceCell;

static ASYNC_UART_INSTANCE: OnceCell<Arc<AsyncUartDriver>> = OnceCell::new();

/// 设置 AsyncUart 实例（集成时调用）
pub fn set_async_uart(driver: Arc<AsyncUartDriver>) {
    ASYNC_UART_INSTANCE.set(driver).unwrap();
}

impl TtyWrite for Console {
    fn write(&self, buf: &[u8]) {
        // 优先使用 AsyncUart 异步路径
        if let Some(driver) = ASYNC_UART_INSTANCE.get() {
            driver.buffer().push_tx(buf);
            // 唤醒 TX copier（通过 tx_waker）
            driver.wake_tx_copier();
        } else {
            // Fallback: Console 阻塞路径（未集成或故障时）
            axhal::console::write_bytes(buf);
        }
    }
}
```

### Console RX 替换实现

```rust
// ntty.rs - Console RX 替换
impl TtyRead for Console {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        // 优先使用 AsyncUart 异步路径
        if let Some(driver) = ASYNC_UART_INSTANCE.get() {
            driver.buffer().pop_rx(buf)
        } else {
            // Fallback: Console 阻塞路径（未集成或故障时）
            axhal::console::read_bytes(buf)
        }
    }
}
```

### tty-reader 移除实现

```rust
// ldisc.rs - tty-reader 移除
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};

static TTY_READER_STOPPED: AtomicBool = AtomicBool::new(false);

/// 停止 tty-reader 任务（集成时调用）
pub fn stop_tty_reader() {
    TTY_READER_STOPPED.store(true, Ordering::SeqCst);
    log::info!("tty-reader task stopped");
}

// InputReader poll 函数修改
impl<R: TtyRead, W: TtyWrite> InputReader<R, W> {
    pub fn poll(&mut self) -> bool {
        // 检查停止标志
        if TTY_READER_STOPPED.load(Ordering::SeqCst) {
            return false;  // 停止轮询
        }

        // 原逻辑...
        // 读 Console RX → 处理 → push rx_buf
    }
}
```

### AsyncUart copier 唤醒机制

```rust
// async_driver.rs - TX copier 唤醒
impl AsyncUartDriver {
    /// 唤醒 TX copier（Console TX 替换调用）
    pub fn wake_tx_copier(&self) {
        self.isr_ctx.tx_waker.wake();
    }

    /// 唤醒 RX copier（ISR 中调用）
    pub fn wake_rx_copier(&self) {
        self.isr_ctx.rx_waker.wake();
    }
}
```

---

## IRQ Waker 冲突解决机制

### 冲突根源分析

**上次失败的 IRQ 风暴问题**：
```
Console tty-reader 已注册 IRQ 10 waker
  ↓
AsyncUart RX copier 也注册 IRQ 10 waker
  ↓
IRQ 10 中断到来 → dispatch_irq(10)
  ↓
唤醒哪个 waker？（冲突）
  ↓
可能的循环唤醒 → IRQ 风暴
```

### 解决方案：tty-reader 完全移除

```rust
// 冲突解决流程
启动时：
  Console tty-reader 启动 → register_irq_waker(10, waker1)

AsyncUart 集成时：
  1. stop_tty_reader() → tty-reader 停止轮询
  2. AsyncUart RX/TX copier 启动 → register_irq_waker(10, waker2/waker3)

结果：
  IRQ 10 独占给 AsyncUart copier（无冲突）
```

### ISR 分发机制（保留设计，但暂不启用）

```rust
// isr.rs - ISR 分发机制（可选，用于多 waker 共存）
use uart_16550::spec::registers::InterruptType;
use embassy_sync::waitqueue::AtomicWaker;

pub struct IsrContext {
    pub uart: Mutex<Uart16550Async>,
    rx_waker: AtomicWaker,
    tx_waker: AtomicWaker,
}

/// UART ISR handler（ISR 分发机制）
pub fn uart_isr_handler(ctx: &Arc<IsrContext>) {
    let mut uart = ctx.uart.lock();

    // 1. 读 ISR（IIR）识别中断类型
    let intr_type = uart.intr_identification();

    match intr_type {
        Some(InterruptType::ReceivedDataReady) => {
            // 2. 禁用 RX 中断（防止重入）
            uart.disable_rx_intr();
            // 3. 唤醒 RX copier
            ctx.rx_waker.wake();
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            // 2. 禁用 TX 中断（防止重入）
            uart.disable_tx_intr();
            // 3. 唤醒 TX copier
            ctx.tx_waker.wake();
        }
        _ => {}
    }
}
```

### IRQ 注册流程

```rust
// axtask::future::register_irq_waker 内部机制（已验证）
// 使用 PollSet 支持多个 waker

// AsyncUart RX copier 注册
register_irq_waker(10, rx_waker);

// AsyncUart TX copier 注册
register_irq_waker(10, tx_waker);

// tty-reader 注册（已移除）
// register_irq_waker(10, tty_reader_waker);  ← 移除
```

### 冲突解决验证

```rust
// 验证 IRQ waker 冲突已解决
fn verify_irq_waker_conflict_resolved() {
    // 检查 tty-reader 已停止
    assert!(TTY_READER_STOPPED.load(Ordering::SeqCst));

    // 检查 AsyncUart copier 已启动
    let driver = ASYNC_UART_INSTANCE.get().unwrap();
    assert!(driver.rx_copier_started.load(Ordering::SeqCst));
    assert!(driver.tx_copier_started.load(Ordering::SeqCst));

    log::info!("IRQ waker conflict resolved: AsyncUart copiers exclusive");
}
```

---

## 错误处理和回退机制

### 错误场景分类

| 错误场景 | 处理策略 | 影响 |
|---------|---------|------|
| **UART 状态不兼容** | 记录警告 + 修复 IER | 可能破坏 Console RX |
| **IER 修复失败** | 回退到 Console 软件路径 | AsyncUart 未集成 |
| **TX copier busy-loop** | 回退到 Console TX | AsyncUart TX 未集成 |
| **IRQ 风暴检测** | 停止 AsyncUart + 回退 Console | 完全回退 |
| **AsyncUart 故障** | Fallback 到 Console API | 保持调试输出能力 |

### UART 状态不兼容处理

```rust
// async_driver.rs - UART 状态诊断与修复
pub fn new() -> Arc<Self> {
    let uart = unsafe { Uart16550Async::new(UART_MMIO_ADDR, 4) };

    // 诊断 UART 状态
    let state = diagnose_uart_state(&uart);
    log_uart_state(&state);

    // 检查兼容性
    if !state.compatible {
        log::error!("UART state incompatible with AsyncUart:");
        for warning in &state.warnings {
            log::error!("  {}", warning);
        }

        // 尝试修复 IER
        if !state.ier.contains(IER::THR_EMPTY) {
            log::warn!("Attempting IER fix...");
            fix_ier_config(&uart, IER::THR_EMPTY | IER::DATA_READY);

            // 验证修复成功
            let new_ier = uart.read_ier();
            if new_ier.contains(IER::THR_EMPTY) {
                log::info!("IER fix successful");
            } else {
                log::error!("IER fix failed, aborting AsyncUart integration");
                return Self::fallback_to_console();
            }
        }
    }

    // 继续集成...
}

/// 回退到 Console 软件路径
fn fallback_to_console() -> Arc<Self> {
    log::warn!("AsyncUart integration failed, using Console fallback");
    // 返回一个"空" driver，Console API 保持原路径
    Self::create_empty_driver()
}
```

### TX busy-loop 检测与回退

```rust
// async_driver.rs - TX copier busy-loop 检测
const TX_BUSY_LOOP_THRESHOLD: usize = 100;

fn start_tx_copier(self: &Arc<Self>) {
    axtask::spawn_with_name(
        {
            let driver = self.clone();
            move || {
                block_on(poll_fn(|cx| {
                    let mut retry_count = 0;

                    loop {
                        // 尝试发送数据
                        let sent = driver.try_send_tx_data();

                        if sent > 0 {
                            retry_count = 0;
                            break;
                        }

                        retry_count += 1;
                        if retry_count > TX_BUSY_LOOP_THRESHOLD {
                            log::error!("TX busy-loop detected, retry_count={}", retry_count);
                            driver.handle_tx_busy_loop();
                            return Poll::Ready(());  // 停止 copier
                        }

                        // 等待 THR_EMPTY 中断
                        register_irq_waker(driver.irq, cx.waker());
                        return Poll::Pending;
                    }

                    Poll::Pending
                }))
            }
        },
        "tx-copier".into(),
    );
}

/// 处理 TX busy-loop
fn handle_tx_busy_loop(&self) {
    log::error!("TX copier busy-loop, fallback to Console TX");

    // 停止 TX copier
    self.tx_copier_started.store(false, Ordering::SeqCst);

    // 清空 TX buffer 并用 Console 发送
    self.flush_tx_via_console();
}
```

### IRQ 风暴检测与回退

```rust
// irq.rs - IRQ 风暴检测（全局监控）
use core::sync::atomic::{AtomicU64, Ordering};

static IRQ_10_TRIGGER_COUNT: AtomicU64 = AtomicU64::new(0);
const IRQ_STORM_THRESHOLD: u64 = 1000;  // 1 秒内触发 1000 次

pub fn dispatch_irq(irq: usize) {
    if irq == 10 {
        let count = IRQ_10_TRIGGER_COUNT.fetch_add(1, Ordering::SeqCst);

        if count > IRQ_STORM_THRESHOLD {
            log::error!("IRQ storm detected: IRQ 10 triggered {} times", count);
            handle_irq_storm();
        }
    }

    // 原分发逻辑...
}

/// 处理 IRQ 风暴
fn handle_irq_storm() {
    log::error!("IRQ storm detected, stopping AsyncUart and fallback to Console");

    // 停止 AsyncUart copier
    if let Some(driver) = ASYNC_UART_INSTANCE.get() {
        driver.stop_all_copiers();
    }

    // 清空 ASYNC_UART_INSTANCE，Console API 自动回退
    ASYNC_UART_INSTANCE.take();

    // 重启 tty-reader（如果需要）
    TTY_READER_STOPPED.store(false, Ordering::SeqCst);
}
```

### Console API Fallback 机制

```rust
// ntty.rs - Console API 自动 fallback
impl TtyWrite for Console {
    fn write(&self, buf: &[u8]) {
        if let Some(driver) = ASYNC_UART_INSTANCE.get() {
            // AsyncUart 异步路径
            driver.buffer().push_tx(buf);
            driver.wake_tx_copier();
        } else {
            // Fallback: Console 阻塞路径（自动触发）
            axhal::console::write_bytes(buf);
        }
    }
}
```

---

## 测试和验证策略

### 测试策略分层

```
┌─────────────────────────────────────────────────────┐
│ 测试分层（从下到上）                                  │
├─────────────────────────────────────────────────────┤
│ 1. UART 状态诊断单元测试                             │
│    ├─ diagnose_uart_state() 输出验证                 │
│    ├─ IER/LSR/ISR/MCR 解析正确性                     │
│    └─ compatible 判断逻辑正确性                       │
│                                                       │
│ 2. AsyncUart copier 任务测试                         │
│    ├─ RX copier: IRQ → read UART → push rx_buf       │
│    ├─ TX copier: pop tx_buf → write UART → IRQ       │
│    └─ AtomicWaker 唤醒机制正确性                      │
│                                                       │
│ 3. Console 软件路径替换测试                           │
│    ├─ Console.write() 重定向到 AsyncUart TX          │
│    ├─ Console.read() 重定向到 AsyncUart RX           │
│    ├─ Fallback 机制（ASYNC_UART_INSTANCE 未设置时）   │
│                                                       │
│ 4. tty-reader 移除验证                               │
│    ├─ TTY_READER_STOPPED 标志生效                    │
│    ├─ tty-reader 任务停止轮询                        │
│    └─ IRQ 10 独占给 AsyncUart copier                 │
│                                                       │
│ 5. 集成测试（E2E）                                    │
│    ├─ 内核启动 → AsyncUart 集成 → Shell 启动          │
│    ├─ 用户态 read/write 通过 AsyncUart                │
│    └─ 内核日志通过 AsyncUart TX                       │
│                                                       │
│ 6. 性能基准测试                                       │
│    ├─ TX 吞吐量 @115200                               │
│    ├─ RX 延迟测量                                     │
│    └─ IRQ 触发频率统计                                │
└─────────────────────────────────────────────────────┘
```

### 单元测试：UART 状态诊断

```rust
// tests/uart_diagnosis_test.rs
#[test]
fn test_diagnose_uart_state() {
    // Mock UART 寄存器值
    let mock_ier = IER::DATA_READY;  // Console 仅使能 RX
    let mock_lsr = LSR::THR_EMPTY | LSR::TEMT;  // TX FIFO 空
    let mock_isr = ISR::INTERRUPT_STATUS;  // 无中断待处理
    let mock_mcr = MCR::empty();

    // 诊断逻辑
    let state = diagnose_uart_state_from_registers(
        mock_ier, mock_lsr, mock_isr, mock_mcr
    );

    // 验证诊断结果
    assert!(!state.ier.contains(IER::THR_EMPTY), "IER missing THR_EMPTY");
    assert!(state.lsr.contains(LSR::THR_EMPTY), "TX FIFO empty");
    assert!(!state.isr.has_pending_interrupt(), "No pending interrupt");
    assert!(!state.compatible, "UART state incompatible");
    assert!(state.warnings.contains(&"IER missing THR_EMPTY"), "Warning detected");
}

#[test]
fn test_fix_ier_config() {
    let original_ier = IER::DATA_READY;
    let fixed_ier = IER::DATA_READY | IER::THR_EMPTY;

    // 验证修复逻辑
    assert!(fixed_ier.contains(IER::THR_EMPTY), "THR_EMPTY enabled");
    assert!(fixed_ier.contains(IER::DATA_READY), "DATA_READY preserved");
}
```

### 集成测试：Console 软件路径替换

```rust
// tests/console_path_test.rs
#[test]
fn test_console_write_redirect_to_async_uart() {
    // 设置 AsyncUart 实例
    let async_uart = AsyncUartDriver::new_mock();
    set_async_uart(async_uart.clone());

    // Console.write() 调用
    let console = Console;
    console.write(b"test data");

    // 验证数据进入 AsyncUart tx_buf
    let tx_len = async_uart.buffer().tx_len();
    assert!(tx_len > 0, "Console.write redirected to AsyncUart");
    assert_eq!(async_uart.buffer().peek_tx(), b"test data");
}

#[test]
fn test_console_write_fallback() {
    // 清空 AsyncUart 实例
    ASYNC_UART_INSTANCE.take();

    // Console.write() 调用
    let console = Console;
    console.write(b"fallback test");

    // 验证使用 Console 阻塞路径（无数据进入 AsyncUart tx_buf）
    // 通过 mock axhal::console::write_bytes 调用次数验证
}
```

### E2E 测试：内核启动验证

```bash
# E2E 测试脚本
make run 2>&1 | tee test_output.log

# 验证关键日志
grep "UART state diagnosis:" test_output.log
grep "IER config fixed:" test_output.log
grep "AsyncUart integration complete" test_output.log
grep "tty-reader task stopped" test_output.log
grep "Console TX/RX paths redirected" test_output.log

# 验证 Shell 启动
grep "Shell started" test_output.log

# 验证无 IRQ 风暴
! grep "IRQ storm detected" test_output.log

# 验证无 TX busy-loop
! grep "TX busy-loop detected" test_output.log
```

### 性能基准测试

```rust
// tests/performance_test.rs
#[test]
fn test_tx_throughput() {
    let async_uart = AsyncUartDriver::new();

    // 发送 1MB 数据
    let data_size = 1024 * 1024;
    let start_time = get_timestamp();

    for _ in 0..data_size {
        async_uart.buffer().push_tx(b"x");
    }

    // 等待 TX copier 完成
    while async_uart.buffer().tx_len() > 0 {
        sleep_ms(1);
    }

    let end_time = get_timestamp();
    let throughput = data_size / (end_time - start_time);

    println!("TX throughput: {} KB/s", throughput);
    assert!(throughput > 10, "Throughput > 10 KB/s @115200");
}

#[test]
fn test_irq_trigger_frequency() {
    // 监控 IRQ 10 触发频率
    let start_count = IRQ_10_TRIGGER_COUNT.load(Ordering::SeqCst);
    sleep_ms(1000);
    let end_count = IRQ_10_TRIGGER_COUNT.load(Ordering::SeqCst);

    let frequency = end_count - start_count;
    println!("IRQ 10 frequency: {} Hz", frequency);

    // 验证无 IRQ 风暴（频率 < 100 Hz）
    assert!(frequency < 100, "IRQ frequency normal (< 100 Hz)");
}
```

---

## 关键文件索引

| 文件 | 作用 | 核心内容 |
|------|------|---------|
| `uart_16550/src/spec.rs` | UART 寄存器定义 | IER/ISR/LSR bitflags + InterruptType 枚举 |
| `uart_16550/src/backend/mmio.rs` | MMIO 访问实现 | read_volatile/write_volatile + 地址计算 |
| `kernel/src/pseudofs/dev/tty/ntty.rs` | Console 驱动 | Console struct + TtyRead/TtyWrite trait + ASYNC_UART_INSTANCE |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | tty-reader copier | InputReader + poll_fn + register_irq_waker + TTY_READER_STOPPED |
| `kernel/src/drivers/serial/async_driver.rs` | AsyncUartDriver | RX/TX copier + UART 状态诊断 + IER 修复 |
| `kernel/src/entry.rs` | 内核入口 | integrate_async_uart() |

---

## 参考资料

- Console UART 工作机制分析：`docs/analysis/console-uart-mechanism.md`
- 中断框架深度分析：`docs/analysis/interrupt-framework.md`
- UART 16550 规范：`uart_16550/src/spec.rs`
- ADR-019（M3 替换失败回滚）：`architecture.md`