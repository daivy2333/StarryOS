# M1 架构验证设计文档

> Date: 2026-05-27
> Status: Draft → Approved
> Milestone: M1 (架构验证)
> Bottom Layer: Console 同步引擎（渐进式策略 ADR-015）

---

## 1. 目标

验证基础架构的正确性：
- Ring Buffer + PollSet 缓冲机制
- 中断注册与 copier 任务唤醒流程
- VFS 接口（DeviceOps + poll/epoll）
- 调试能力保留（Console 同步输出始终可用）

**Gate M1 标准**：
- Ring Buffer 单元测试全部通过
- `/dev/async_uart_test` 设备可打开、读写
- 中断触发 → RX copier 唤醒 → 数据到达 rx_buf
- TX 同步发送正常
- Poll/epoll 可监听事件

---

## 2. 模块结构

采用分层模块设计（方案 B）：

```
kernel/src/drivers/serial/
├── mod.rs               # 模块导出
├── ring_buffer.rs       # Ring Buffer + PollSet（可单独测试）
├── console_driver.rs    # ConsoleDriver：封装 Console + ringbuf + copier
└── device_ops.rs        # DeviceOps 实现（VFS 接口层）
```

**职责分离**：
- `ring_buffer.rs`：纯数据结构，可独立单元测试
- `console_driver.rs`：数据收发逻辑，M3 替换为 async_uart_driver.rs
- `device_ops.rs`：VFS 接口，M2/M3 不变

---

## 3. Ring Buffer 模块 (`ring_buffer.rs`)

### 3.1 核心结构

```rust
use axsync::Mutex;
use axpoll::PollSet;
use ringbuf::HeapRb;

pub struct AsyncBuffer {
    rx_buf: Mutex<HeapRb<u8>>,
    tx_buf: Mutex<HeapRb<u8>>,
    rx_wakers: PollSet,
    tx_wakers: PollSet,
}
```

### 3.2 核心 API

| 方法 | 调用者 | 说明 |
|------|--------|------|
| `new(capacity: usize)` | 初始化 | 创建指定容量（默认 64 KiB） |
| `push_rx(data: &[u8]) -> usize` | RX copier | 写入 rx_buf |
| `pop_rx(buf: &mut [u8]) -> usize` | 用户 read | 从 rx_buf 读取 |
| `push_tx(data: &[u8]) -> usize` | 用户 write | 写入 tx_buf |
| `pop_tx(buf: &mut [u8]) -> usize` | TX copier (M1 同步) | 从 tx_buf 读取 |
| `wake_rx()` | RX copier | 唤醒 RX 等待者 |
| `wake_tx()` | TX 完成后 | 唤醒 TX 等待者 |
| `register_rx_waker(waker)` | poll_io | 注册 RX Waker |
| `register_tx_waker(waker)` | poll_io | 注册 TX Waker |

### 3.3 关键设计点

- **HeapRb 非中断安全**：ISR 不直接操作 ringbuf，只唤醒 Waker
- **Producer/Consumer 分离**：copier 和用户操作不同端，天然无竞态
- **PollSet 容量 64**：足够单设备使用（参考 Pipe）

### 3.4 单元测试

```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_buffer_creation() { /* 容量正确，初始为空 */ }
    #[test]
    fn test_push_pop_basic() { /* 单次写入/读取数据完整 */ }
    #[test]
    fn test_buffer_full() { /* 写满后返回 WouldBlock */ }
    #[test]
    fn test_buffer_empty() { /* 空读取返回 WouldBlock */ }
    #[test]
    fn test_wrap_around() { /* 环形缓冲区边界正确 */ }
    #[test]
    fn test_wake_mechanism() { /* 写入后唤醒等待者 */ }
}
```

---

## 4. ConsoleDriver 模块 (`console_driver.rs`)

### 4.1 核心结构

```rust
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use axtask::future::register_irq_waker;

pub struct ConsoleDriver {
    buffer: Arc<AsyncBuffer>,
    rx_irq: usize,  // IRQ 10 (axhal::console::irq_num())
    rx_copier_started: AtomicBool,
}
```

### 4.2 RX copier 任务流程

参考 `ldisc.rs` tty-reader 模式：

```rust
pub fn start_rx_copier(self: &Arc<Self>) {
    if self.rx_copier_started.swap(true, Ordering::SeqCst) {
        return; // 已启动
    }
    
    axtask::spawn_with_name({
        let driver = self.clone();
        move || {
            block_on(poll_fn(|cx| {
                let mut tmp_buf = [0u8; 256];
                
                // 1. 从 Console 同步读取
                let n = axhal::console::read_bytes(&mut tmp_buf);
                
                // 2. 写入 rx_buf
                if n > 0 {
                    driver.buffer.push_rx(&tmp_buf[..n]);
                    driver.buffer.wake_rx();
                }
                
                // 3. 注册 IRQ Waker 等待中断
                register_irq_waker(driver.rx_irq, cx.waker());
                
                // 4. 返回 Pending，等待唤醒
                Poll::Pending
            }))
        }
    }, "rx-copier".into());
}
```

### 4.3 中断注册流程

参考 `ntty.rs` ProcessMode::External：

```rust
// ConsoleDriver::new() 时
if let Some(irq) = axhal::console::irq_num() {
    self.rx_irq = irq;
    self.start_rx_copier();
}
```

**共存机制**（ADR-015 已确认）：
- register_irq_waker 使用 BTreeMap<usize, PollSet>
- 同一 IRQ 支持多个 Waker 注册
- Console tty-reader 和 AsyncUart RX copier 共用 IRQ 10

### 4.4 TX 同步发送流程（M1 简化版）

不做真正的 TX copier，直接同步发送：

```rust
pub fn flush_tx_sync(&self) {
    let mut tx_buf = self.buffer.tx_buf.lock();
    while tx_buf.occupied_len() > 0 {
        let (left, right) = tx_buf.as_slices();
        axhal::console::write_bytes(left);
        if left.len() < tx_buf.occupied_len() {
            axhal::console::write_bytes(right);
        }
        unsafe { tx_buf.advance_read_index(left.len() + right.len()) };
    }
    self.buffer.wake_tx();
}
```

**M3 改进**：
- 替换为 TX copier 任务
- 使用 uart_16550.try_send 异步发送
- ISR 中处理 TX 中断

---

## 5. DeviceOps 实现 (`device_ops.rs`)

### 5.1 核心结构

```rust
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use axerrno::{AxError, AxResult};
use axpoll::{IoEvents, Pollable};
use axtask::future::{block_on, poll_io};

pub struct AsyncUartTestDevice {
    driver: Arc<ConsoleDriver>,
    non_blocking: AtomicBool,
}
```

### 5.2 DeviceOps 实现

```rust
impl DeviceOps for AsyncUartTestDevice {
    fn read_at(&self, offset: usize, buf: &mut [u8]) -> AxResult<usize> {
        // offset 对流设备无意义，忽略
        block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
            let n = self.driver.buffer.pop_rx(buf);
            if n > 0 {
                Ok(n)
            } else {
                Err(AxError::WouldBlock)
            }
        }))
    }
    
    fn write_at(&self, offset: usize, buf: &[u8]) -> AxResult<usize> {
        block_on(poll_io(self, IoEvents::OUT, self.nonblocking(), || {
            let n = self.driver.buffer.push_tx(buf);
            if n > 0 {
                self.driver.flush_tx_sync();  // M1 同步发送
                Ok(n)
            } else {
                Err(AxError::WouldBlock)
            }
        }))
    }
    
    fn as_pollable(&self) -> Option<&dyn Pollable> {
        Some(self)
    }
}
```

### 5.3 Pollable 实现

```rust
impl Pollable for AsyncUartTestDevice {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        let rx_buf = self.driver.buffer.rx_buf.lock();
        let tx_buf = self.driver.buffer.tx_buf.lock();
        events.set(IoEvents::IN, rx_buf.occupied_len() > 0);
        events.set(IoEvents::OUT, tx_buf.vacant_len() > 0);
        events
    }
    
    fn register(&self, cx: &mut Context, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.driver.buffer.register_rx_waker(cx.waker());
        }
        if events.contains(IoEvents::OUT) {
            self.driver.buffer.register_tx_waker(cx.waker());
        }
    }
}
```

### 5.4 设备注册入口

在 `pseudofs/dev/mod.rs builder()` 中添加：

```rust
use crate::drivers::serial::device_ops::AsyncUartTestDevice;

// builder() 函数中
Device::new(AsyncUartTestDevice::new()).register("async_uart_test");
```

---

## 6. 验证流程

### 6.1 单元测试

```bash
# 不依赖 QEMU
cargo test --package starry-kernel --lib drivers::serial::ring_buffer
```

**预期结果**：全部测试通过

### 6.2 QEMU 手动验证

```bash
# 启动内核
make run

# 在 shell 中检查设备
ls /dev/
# 应看到 async_uart_test

# 写入测试
echo "hello" > /dev/async_uart_test
# Console 应输出 "hello"

# 读取测试（从 QEMU 外部终端输入）
cat /dev/async_uart_test
# 输入字符后应显示

# Poll 测试（需用户态测试程序）
./test_poll /dev/async_uart_test
```

### 6.3 中断触发观察

- 在 QEMU Console 输入字符 → IRQ 10 触发
- RX copier 任务唤醒 → Console.read_bytes 读取 → rx_buf 写入
- poll_io 等待的 read_at 被唤醒 → 用户收到数据

---

## 7. M1 → M3 衔接预留

### 7.1 UartEngine trait（预留）

```rust
/// M1 定义，M3 实现
pub trait UartEngine: Send + Sync + 'static {
    fn try_read(&mut self, buf: &mut [u8]) -> usize;
    fn try_write(&mut self, buf: &[u8]) -> usize;
    fn enable_rx_intr(&mut self);
    fn disable_rx_intr(&mut self);
    fn enable_tx_intr(&mut self);
    fn disable_tx_intr(&mut self);
}
```

### 7.2 M1 实现：ConsoleEngine

```rust
pub struct ConsoleEngine;

impl UartEngine for ConsoleEngine {
    fn try_read(&mut self, buf: &mut [u8]) -> usize {
        axhal::console::read_bytes(buf)
    }
    fn try_write(&mut self, buf: &[u8]) -> usize {
        axhal::console::write_bytes(buf);
        buf.len()
    }
    fn enable_rx_intr(&mut self) { /* Console 已启用 */ }
    fn disable_rx_intr(&mut self) { /* 不支持 */ }
    fn enable_tx_intr(&mut self) { /* 不支持 */ }
    fn disable_tx_intr(&mut self) { /* 不支持 */ }
}
```

### 7.3 M3 实现：AsyncUartEngine

```rust
use uart_16550::SerialPort;

pub struct AsyncUartEngine {
    uart: SerialPort<MmioBackend>,
    rx_waker: embassy_sync::AtomicWaker,
    tx_waker: embassy_sync::AtomicWaker,
}

impl UartEngine for AsyncUartEngine {
    fn try_read(&mut self, buf: &mut [u8]) -> usize {
        self.uart.try_receive(buf)
    }
    fn try_write(&mut self, buf: &[u8]) -> usize {
        self.uart.try_send(buf)
    }
    fn enable_rx_intr(&mut self) {
        self.uart.set_interrupt_enable(InterruptEnable::all_rx());
    }
    // ... 完整中断控制
}
```

---

## 8. 任务清单

| 任务 | 输出文件 | 验证方式 |
|------|---------|---------|
| T1.1 | `ring_buffer.rs` | 单元测试通过 |
| T1.2 | `console_driver.rs` | 中断回调触发 + copier 唤醒 |
| T1.3 | `console_driver.rs` | RX 数据到达 rx_buf |
| T1.4 | `console_driver.rs` + `device_ops.rs` | TX Console 输出正常 |
| 设备注册 | `pseudofs/dev/mod.rs` | `/dev/async_uart_test` 可打开 |
| Poll/epoll | `device_ops.rs` | Pollable 实现正确 |

---

## 9. 风险与缓解

| 风险 | 影响 | 缓解措施 |
|------|------|---------|
| Console 同步阻塞影响调试 | 中等 | 接受启动日志阻塞，用户态异步化（ADR-013） |
| IRQ 10 共存冲突 | 低 | register_irq_waker 已验证支持多个 Waker |
| RX copier 任务卡死 | 中等 | axhal::console 作为 earlycon 始终可用 |
| TX 同步阻塞影响吞吐 | 低 | M1 验证架构正确性，M3 异步化提升性能 |

---

## 10. 参考

- ADR-008: ISR → AtomicWaker → copier 任务模型
- ADR-015: 渐进式开发策略
- `kernel/src/file/pipe.rs`: HeapRb + PollSet + block_on(poll_io) 模式
- `kernel/src/pseudofs/dev/tty/ntty.rs`: Console + register_irq_waker 模式
- `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`: tty-reader copier 模式