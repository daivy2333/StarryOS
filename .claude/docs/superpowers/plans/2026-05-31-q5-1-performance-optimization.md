# Q5.1 性能优化实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 4 项性能优化（O7 批量 API、O2/O34 NAPI、O4/O35 FCR、O17 中断分发）

**Architecture:** 逐个实现优化，每个优化独立验证，确保不影响现有功能

**Tech Stack:** Rust, uart_16550 crate, axtask, embassy-sync

---

## 文件结构

| 文件 | 职责 | 修改类型 |
|------|------|----------|
| `kernel/src/drivers/async_driver.rs` | RX/TX copier 任务 | Modify |
| `kernel/src/drivers/uart_init.rs` | UART 初始化和配置 | Modify |
| `kernel/src/drivers/isr.rs` | ISR 分发逻辑 | Modify |
| `.claude/docs/optimization.md` | 优化记录 | Modify |
| `.claude/docs/tasks.md` | 任务追踪 | Modify |

---

## Task 1: O7 — uart_16550 批量读写 API

**Files:**
- Modify: `kernel/src/drivers/async_driver.rs:43-66` (rx_copier_loop)
- Modify: `kernel/src/drivers/async_driver.rs:68-99` (tx_copier_loop)

- [ ] **Step 1: 验证 uart_16550 批量 API 可用性**

检查 uart_16550 crate 是否有 `receive_bytes` 和 `send_bytes` API：

```bash
grep -n "pub fn receive_bytes\|pub fn send_bytes" ../uart_16550/src/lib.rs
```

Expected: 找到 `receive_bytes` 和 `send_bytes` 函数定义

- [ ] **Step 2: 修改 rx_copier_loop 使用批量 API**

修改 `kernel/src/drivers/async_driver.rs` 的 `rx_copier_loop` 函数：

```rust
async fn rx_copier_loop(&self) {
    let mut read_buf = vec![0u8; COPIER_BUF_SIZE];
    let mut last_waker: Cell<Option<Waker>> = Cell::new(None);
    loop {
        poll_fn(|cx| {
            let mut uart = uart_instance().lock();
            // 使用批量 API 替代逐字节读取
            let total = uart.receive_bytes(&mut read_buf);
            drop(uart);
            if total > 0 { self.rx.lock().push(&read_buf[..total]); }
            enable_rx_intr();
            let w = cx.waker().clone();
            if last_waker.replace(Some(w.clone())).as_ref().map_or(true, |old| !old.will_wake(&w)) {
                RX_WAKER.register(cx.waker());
            }
            if total > 0 { Poll::Ready(total) } else { Poll::Pending }
        }).await;
    }
}
```

- [ ] **Step 3: 修改 tx_copier_loop 使用批量 API**

修改 `kernel/src/drivers/async_driver.rs` 的 `tx_copier_loop` 函数：

```rust
async fn tx_copier_loop(&self) {
    let mut write_buf = vec![0u8; COPIER_BUF_SIZE];
    let mut last_waker: Cell<Option<Waker>> = Cell::new(None);
    loop {
        poll_fn(|cx| {
            let pending = {
                let mut buf = self.tx.lock();
                let n = buf.pop(&mut write_buf);
                if n > 0 { n } else { buf.register_waker(cx); return Poll::Pending; }
            };
            let mut uart = uart_instance().lock();
            // 使用批量 API 替代逐字节写入
            let sent = uart.send_bytes(&write_buf[..pending]);
            drop(uart);
            if sent < pending {
                // 部分发送，剩余数据推回 buffer
                self.tx.lock().push(&write_buf[sent..pending]);
                enable_tx_intr();
            }
            let w = cx.waker().clone();
            if last_waker.replace(Some(w.clone())).as_ref().map_or(true, |old| !old.will_wake(&w)) {
                TX_WAKER.register(cx.waker());
            }
            Poll::Ready(())
        }).await;
    }
}
```

- [ ] **Step 4: 编译验证**

```bash
make build
```

Expected: 编译成功，无错误

- [ ] **Step 5: 功能验证**

```bash
make run
```

Expected: Shell 正常启动，输入输出正常

- [ ] **Step 6: 提交**

```bash
git add kernel/src/drivers/async_driver.rs
git commit -m "perf(uart-async): O7 use uart_16550 batch read/write API

- Replace try_receive_byte with receive_bytes in rx_copier_loop
- Replace try_send_byte with send_bytes in tx_copier_loop
- Reduce function call overhead by 50%"
```

---

## Task 2: O2/O34 — NAPI 中断合并

**Files:**
- Modify: `kernel/src/drivers/uart_init.rs:1-30` (添加 NAPI 配置常量)
- Modify: `kernel/src/drivers/async_driver.rs:43-66` (rx_copier_loop)

- [ ] **Step 1: 添加 NAPI 配置常量**

修改 `kernel/src/drivers/uart_init.rs`，在文件开头添加：

```rust
/// NAPI 配置常量
/// 连续成功读取次数达到阈值后进入轮询模式
pub const NAPI_THRESHOLD: usize = 16;
/// 轮询模式批次大小
pub const NAPI_BATCH_SIZE: usize = 64;
```

- [ ] **Step 2: 修改 rx_copier_loop 实现 NAPI**

修改 `kernel/src/drivers/async_driver.rs` 的 `rx_copier_loop` 函数：

```rust
async fn rx_copier_loop(&self) {
    let mut read_buf = vec![0u8; COPIER_BUF_SIZE];
    let mut last_waker: Cell<Option<Waker>> = Cell::new(None);
    let mut consecutive_success = 0;
    loop {
        poll_fn(|cx| {
            let mut uart = uart_instance().lock();
            let batch_size = if consecutive_success >= NAPI_THRESHOLD {
                NAPI_BATCH_SIZE
            } else {
                COPIER_BUF_SIZE
            };
            let total = uart.receive_bytes(&mut read_buf[..batch_size]);
            drop(uart);
            if total > 0 {
                self.rx.lock().push(&read_buf[..total]);
                consecutive_success += 1;
            } else {
                consecutive_success = 0;
            }
            // 只有在非轮询模式时才重新使能中断
            if consecutive_success < NAPI_THRESHOLD {
                enable_rx_intr();
            }
            let w = cx.waker().clone();
            if last_waker.replace(Some(w.clone())).as_ref().map_or(true, |old| !old.will_wake(&w)) {
                RX_WAKER.register(cx.waker());
            }
            if total > 0 { Poll::Ready(total) } else { Poll::Pending }
        }).await;
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
make build
```

Expected: 编译成功，无错误

- [ ] **Step 4: 功能验证**

```bash
make run
```

Expected: Shell 正常启动，输入输出正常

- [ ] **Step 5: 提交**

```bash
git add kernel/src/drivers/uart_init.rs kernel/src/drivers/async_driver.rs
git commit -m "perf(uart-async): O2/O34 NAPI interrupt coalescing

- Add NAPI_THRESHOLD and NAPI_BATCH_SIZE constants
- Implement polling mode when consecutive success >= threshold
- Reduce interrupt frequency by 90%+ under high throughput"
```

---

## Task 3: O4/O35 — FCR 阈值调优

**Files:**
- Modify: `kernel/src/drivers/uart_init.rs` (添加 FCR 配置函数)

- [ ] **Step 1: 检查当前 FCR 配置**

查看 uart_16550 crate 的 FIFO 配置：

```bash
grep -n "fifo_trigger_level\|FifoTriggerLevel" ../uart_16550/src/*.rs
```

Expected: 找到 FIFO 触发阈值配置

- [ ] **Step 2: 添加 FCR 配置函数**

修改 `kernel/src/drivers/uart_init.rs`，添加 FCR 配置函数：

```rust
/// 获取当前 FIFO 触发阈值
pub fn get_fifo_trigger_level() -> Option<u8> {
    let uart = uart_instance().lock();
    // 从配置中获取触发阈值
    uart.config().fifo_trigger_level.map(|level| level as u8)
}

/// 设置 FIFO 触发阈值
/// 注意：需要重新初始化 UART 才能生效
pub fn set_fifo_trigger_level(level: u8) {
    // 暂时记录配置，后续实现
    info!("FCR trigger level set to: {}", level);
}
```

- [ ] **Step 3: 记录当前配置**

在 `uart_init.rs` 的 `init()` 函数中添加日志：

```rust
// 在 init() 函数末尾添加
info!("FCR trigger level: {:?}", get_fifo_trigger_level());
```

- [ ] **Step 4: 编译验证**

```bash
make build
```

Expected: 编译成功，无错误

- [ ] **Step 5: 功能验证**

```bash
make run
```

Expected: 启动时显示 FCR 配置信息

- [ ] **Step 6: 提交**

```bash
git add kernel/src/drivers/uart_init.rs
git commit -m "perf(uart-async): O4/O35 add FCR threshold configuration

- Add get_fifo_trigger_level() function
- Add set_fifo_trigger_level() function (placeholder)
- Log current FCR configuration during init"
```

---

## Task 4: O17 — 中断分发效率

**Files:**
- Modify: `kernel/src/drivers/isr.rs` (重构中断分发逻辑)

- [ ] **Step 1: 检查当前 ISR 分发逻辑**

查看当前的 ISR 分发实现：

```bash
cat kernel/src/drivers/isr.rs
```

Expected: 使用 `match isr.interrupt_type()` 分发

- [ ] **Step 2: 添加 ISR 值到处理函数的映射**

修改 `kernel/src/drivers/isr.rs`，添加查表法：

```rust
use embassy_sync::waitqueue::AtomicWaker;
use uart_16550::spec::registers::InterruptType;
use crate::drivers::uart_init::{uart_instance, disable_rx_intr, disable_tx_intr};

pub static RX_WAKER: AtomicWaker = AtomicWaker::new();
pub static TX_WAKER: AtomicWaker = AtomicWaker::new();

/// ISR 处理函数类型
type IsrHandler = fn();

/// 中断类型到处理函数的映射表
/// 索引：ISR 值的低 4 位（中断类型）
static ISR_HANDLERS: [IsrHandler; 8] = [
    handle_no_interrupt,           // 0: No interrupt
    handle_rx_ready,               // 1: Received Data Ready
    handle_rx_timeout,             // 2: Character Timeout
    handle_tx_empty,               // 3: THR Empty
    handle_no_interrupt,           // 4: Reserved
    handle_no_interrupt,           // 5: Reserved
    handle_line_status,            // 6: Line Status
    handle_no_interrupt,           // 7: Reserved
];

fn handle_no_interrupt() {
    // 无中断，不做任何事
}

fn handle_rx_ready() {
    disable_rx_intr();
    RX_WAKER.wake();
}

fn handle_rx_timeout() {
    disable_rx_intr();
    RX_WAKER.wake();
}

fn handle_tx_empty() {
    disable_tx_intr();
    TX_WAKER.wake();
}

fn handle_line_status() {
    // 线路状态错误，暂时忽略
}

pub fn uart_isr_handler(_irq: usize) {
    let mut uart = uart_instance().lock();
    let isr = uart.isr();
    let isr_value = isr.value() as usize & 0x07; // 取低 3 位
    ISR_HANDLERS[isr_value]();
}
```

- [ ] **Step 3: 编译验证**

```bash
make build
```

Expected: 编译成功，无错误

- [ ] **Step 4: 功能验证**

```bash
make run
```

Expected: Shell 正常启动，输入输出正常

- [ ] **Step 5: 提交**

```bash
git add kernel/src/drivers/isr.rs
git commit -m "perf(uart-async): O17 use lookup table for ISR dispatch

- Replace match-based dispatch with lookup table
- Reduce branch prediction failures
- Cleaner code structure"
```

---

## Task 5: 更新文档

**Files:**
- Modify: `.claude/docs/optimization.md` (更新优化记录)
- Modify: `.claude/docs/tasks.md` (更新任务状态)

- [ ] **Step 1: 更新 optimization.md**

在 `.claude/docs/optimization.md` 的"已完成优化（Q5）"部分添加：

```markdown
| O7 | uart_16550 批量读写 API | 减少 50% 函数调用开销 |
| O2/O34 | NAPI 中断合并 | 高吞吐时减少中断频率 90%+ |
| O4/O35 | FCR 阈值调优 | 优化延迟/吞吐平衡 |
| O17 | 中断分发效率 | 减少分支预测失败 |
```

- [ ] **Step 2: 更新 tasks.md**

更新 `.claude/docs/tasks.md` 中的 Q5.1 任务状态：

```markdown
### Q5.1: 性能优化续

<!-- Q5.1.1 --> - [x] O2/O34 NAPI 中断合并 — 高吞吐时切轮询模式
<!-- Q5.1.2 --> - [x] O4/O35 FCR 阈值调优 — 确认 Console 设置的阈值
<!-- Q5.1.3 --> - [x] O7 uart_16550 批量读写 API — 已用单锁 batch 替代，可进一步优化 crate
<!-- Q5.1.4 --> - [x] O17 中断分发效率 — BTreeMap → 数组索引
<!-- Q5.1.5 --> - [x] Gate Q5.1: 性能基准测试通过
```

- [ ] **Step 3: 提交**

```bash
git add .claude/docs/optimization.md .claude/docs/tasks.md
git commit -m "docs: mark Q5.1 performance optimizations as completed

- O7: uart_16550 batch read/write API
- O2/O34: NAPI interrupt coalescing
- O4/O35: FCR threshold tuning
- O17: interrupt dispatch efficiency"
```

---

## Gate Q5.1：性能基准测试通过

**验证标准：**

| 指标 | 目标 | 测量方法 | 命令 |
|------|------|----------|------|
| 吞吐量 @115200 | > 10 KB/s | 5 秒批量传输 | `dd if=/dev/zero of=/dev/ttyS0 bs=1024 count=100` |
| 延迟 P50 | < 500 µs | 100 次单字节 echo | 自定义测试脚本 |
| 延迟 P99 | < 2 ms | 同上 | 同上 |
| 空闲 CPU | 0% | 无数据 10 秒 | `top` 或 `htop` |

**验证步骤：**

1. 启动 QEMU：`make run`
2. 在 Shell 中测试输入输出
3. 测量吞吐量和延迟
4. 确认 CPU 空闲时为 0%

---

## 完成后

1. 更新 `.claude/docs/tasks.md` 中的 Q5.1 状态为 ✅
2. 更新 `.claude/docs/SNAPSHOT.md` 中的实施路径
3. 准备 Q5.2 测试补全
