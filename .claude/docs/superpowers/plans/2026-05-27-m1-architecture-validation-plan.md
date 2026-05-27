# M1 架构验证实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 验证异步串口基础架构（Ring Buffer + 中断机制 + copier 任务模型），底层用 Console 同步引擎

**Architecture:** 分层模块设计：ring_buffer.rs（数据结构）→ console_driver.rs（收发逻辑）→ device_ops.rs（VFS 接口），设备注册到 `/dev/async_uart_test`

**Tech Stack:** Rust + ringbuf::HeapRb + axpoll::PollSet + axtask::future + axhal::console

---

## 文件结构

### 新建文件

| 文件 | 职责 |
|------|------|
| `kernel/src/drivers/mod.rs` | drivers 模块导出 |
| `kernel/src/drivers/serial/mod.rs` | serial 子模块导出 |
| `kernel/src/drivers/serial/ring_buffer.rs` | AsyncBuffer 结构 + 单元测试 |
| `kernel/src/drivers/serial/console_driver.rs` | ConsoleDriver + RX copier 任务 |
| `kernel/src/drivers/serial/device_ops.rs` | AsyncUartTestDevice + DeviceOps + Pollable |

### 修改文件

| 文件 | 修改内容 |
|------|---------|
| `kernel/src/lib.rs` | 添加 `mod drivers;` |
| `kernel/src/pseudofs/dev/mod.rs` | builder() 中注册 async_uart_test 设备 |

---

## Task 1: 创建目录结构和模块导出

**Files:**
- Create: `kernel/src/drivers/mod.rs`
- Create: `kernel/src/drivers/serial/mod.rs`
- Modify: `kernel/src/lib.rs`

- [ ] **Step 1: 创建 drivers 目录结构**

```bash
mkdir -p kernel/src/drivers/serial
```

Expected: 目录创建成功

- [ ] **Step 2: 创建 drivers/mod.rs**

```rust
//! Device drivers

pub mod serial;
```

Write to: `kernel/src/drivers/mod.rs`

- [ ] **Step 3: 创建 serial/mod.rs（初始版本）**

```rust
//! Async UART driver (M1 architecture validation)

mod ring_buffer;
mod console_driver;
mod device_ops;

pub use device_ops::AsyncUartTestDevice;
```

Write to: `kernel/src/drivers/serial/mod.rs`

- [ ] **Step 4: 在 lib.rs 中注册 drivers 模块**

```rust
//! The core functionality of a monolithic kernel, including loading user
//! programs and managing processes.

#![no_std]
#![feature(likely_unlikely)]
#![feature(bstr)]
#![allow(missing_docs)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

extern crate alloc;
extern crate axruntime;

#[macro_use]
extern crate axlog;

pub mod entry;

mod config;
mod drivers;  // 新增
mod file;
mod mm;
mod pseudofs;
mod syscall;
mod task;
mod time;
```

Edit: `kernel/src/lib.rs` 第 19 行后添加 `mod drivers;`

- [ ] **Step 5: 创建空的 placeholder 文件（编译通过）**

```rust
// Placeholder for ring_buffer.rs
pub struct AsyncBuffer;
```

Write to: `kernel/src/drivers/serial/ring_buffer.rs`

```rust
// Placeholder for console_driver.rs
pub struct ConsoleDriver;
```

Write to: `kernel/src/drivers/serial/console_driver.rs`

```rust
// Placeholder for device_ops.rs
pub struct AsyncUartTestDevice;
```

Write to: `kernel/src/drivers/serial/device_ops.rs`

- [ ] **Step 6: 验证编译**

```bash
cd /home/daivy/projects/serial/StarryOS
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
cargo check --package starry-kernel
```

Expected: 编译通过（无 errors）

- [ ] **Step 7: 提交**

```bash
git add kernel/src/drivers/ kernel/src/lib.rs
git commit -m "feat(uart-async): 创建 drivers/serial 模块结构"
```

---

## Task 2: 实现 AsyncBuffer（Ring Buffer + PollSet）— TDD

**Files:**
- Modify: `kernel/src/drivers/serial/ring_buffer.rs`

- [ ] **Step 1: 写失败测试 — test_buffer_creation**

```rust
use axsync::Mutex;
use axpoll::PollSet;
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer};

const DEFAULT_CAPACITY: usize = 65536; // 64 KiB

/// Async buffer with RX and TX ring buffers + waker sets
pub struct AsyncBuffer {
    rx_buf: Mutex<HeapRb<u8>>,
    tx_buf: Mutex<HeapRb<u8>>,
    rx_wakers: PollSet,
    tx_wakers: PollSet,
}

impl AsyncBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            rx_buf: Mutex::new(HeapRb::new(capacity)),
            tx_buf: Mutex::new(HeapRb::new(capacity)),
            rx_wakers: PollSet::new(),
            tx_wakers: PollSet::new(),
        }
    }

    pub fn new_default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    pub fn rx_len(&self) -> usize {
        self.rx_buf.lock().occupied_len()
    }

    pub fn tx_vacant(&self) -> usize {
        self.tx_buf.lock().vacant_len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_buffer_creation() {
        let buf = AsyncBuffer::new_default();
        assert_eq!(buf.rx_len(), 0);
        assert_eq!(buf.tx_vacant(), DEFAULT_CAPACITY);
    }
}
```

Write to: `kernel/src/drivers/serial/ring_buffer.rs`

- [ ] **Step 2: 验证编译和测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer::tests::test_buffer_creation
```

Expected: 测试 PASS

- [ ] **Step 3: 写失败测试 — test_push_pop_basic**

在 `#[cfg(test)] mod tests` 中添加：

```rust
    #[test]
    fn test_push_pop_basic() {
        let buf = AsyncBuffer::new(100);
        
        // Push to RX
        {
            let mut rx = buf.rx_buf.lock();
            let written = rx.push_slice(b"hello");
            assert_eq!(written, 5);
        }
        
        // Pop from RX
        let mut out = [0u8; 10];
        {
            let rx = buf.rx_buf.lock();
            let (left, right) = rx.as_slices();
            out[..left.len()].copy_from_slice(left);
            if !right.is_empty() {
                out[left.len()..left.len()+right.len()].copy_from_slice(right);
            }
            unsafe { rx.advance_read_index(left.len() + right.len()) };
        }
        assert_eq!(&out[..5], b"hello");
    }
```

- [ ] **Step 4: 验证测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer::tests::test_push_pop_basic
```

Expected: 测试 PASS

- [ ] **Step 5: 实现 push_rx/pop_rx/push_tx/pop_tx API**

添加到 `impl AsyncBuffer`：

```rust
    /// Push data to RX buffer (called by RX copier)
    pub fn push_rx(&self, data: &[u8]) -> usize {
        let mut buf = self.rx_buf.lock();
        let n = buf.push_slice(data);
        self.rx_wakers.wake();
        n
    }

    /// Pop data from RX buffer (called by user read)
    pub fn pop_rx(&self, buf: &mut [u8]) -> usize {
        let rx = self.rx_buf.lock();
        let (left, right) = rx.as_slices();
        let mut count = 0;
        if !left.is_empty() {
            let n = left.len().min(buf.len());
            buf[..n].copy_from_slice(&left[..n]);
            count = n;
        }
        if !right.is_empty() && count < buf.len() {
            let n = right.len().min(buf.len() - count);
            buf[count..count+n].copy_from_slice(&right[..n]);
            count += n;
        }
        unsafe { rx.advance_read_index(count) };
        count
    }

    /// Push data to TX buffer (called by user write)
    pub fn push_tx(&self, data: &[u8]) -> usize {
        let mut buf = self.tx_buf.lock();
        let n = buf.push_slice(data);
        self.tx_wakers.wake();
        n
    }

    /// Pop data from TX buffer (called by TX copier/sync flush)
    pub fn pop_tx(&self, buf: &mut [u8]) -> usize {
        let tx = self.tx_buf.lock();
        let (left, right) = tx.as_slices();
        let mut count = 0;
        if !left.is_empty() {
            let n = left.len().min(buf.len());
            buf[..n].copy_from_slice(&left[..n]);
            count = n;
        }
        if !right.is_empty() && count < buf.len() {
            let n = right.len().min(buf.len() - count);
            buf[count..count+n].copy_from_slice(&right[..n]);
            count += n;
        }
        unsafe { tx.advance_read_index(count) };
        count
    }

    /// Wake RX waiters
    pub fn wake_rx(&self) {
        self.rx_wakers.wake();
    }

    /// Wake TX waiters
    pub fn wake_tx(&self) {
        self.tx_wakers.wake();
    }

    /// Register RX waker
    pub fn register_rx_waker(&self, waker: &core::task::Waker) {
        self.rx_wakers.register(waker);
    }

    /// Register TX waker
    pub fn register_tx_waker(&self, waker: &core::task::Waker) {
        self.tx_wakers.register(waker);
    }
```

- [ ] **Step 6: 写测试 — test_buffer_full**

```rust
    #[test]
    fn test_buffer_full() {
        let buf = AsyncBuffer::new(10);
        
        // Fill RX buffer
        let n = buf.push_rx(b"0123456789");  // 10 bytes
        assert_eq!(n, 10);
        
        // Try push more
        let n = buf.push_rx(b"extra");
        assert_eq!(n, 0);  // Should be 0 (full)
    }
```

- [ ] **Step 7: 验证测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer::tests::test_buffer_full
```

Expected: 测试 PASS

- [ ] **Step 8: 写测试 — test_buffer_empty**

```rust
    #[test]
    fn test_buffer_empty() {
        let buf = AsyncBuffer::new(100);
        
        let mut out = [0u8; 10];
        let n = buf.pop_rx(&mut out);
        assert_eq!(n, 0);  // Should be 0 (empty)
        
        let n = buf.pop_tx(&mut out);
        assert_eq!(n, 0);  // Should be 0 (empty)
    }
```

- [ ] **Step 9: 验证测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer::tests::test_buffer_empty
```

Expected: 测试 PASS

- [ ] **Step 10: 写测试 — test_wrap_around**

```rust
    #[test]
    fn test_wrap_around() {
        let buf = AsyncBuffer::new(8);  // Small buffer to force wrap
        
        // Fill and drain multiple times
        buf.push_rx(b"abcd");
        let mut out = [0u8; 4];
        let n = buf.pop_rx(&mut out);
        assert_eq!(n, 4);
        assert_eq!(&out, b"abcd");
        
        // Push more (should wrap around)
        buf.push_rx(b"efgh");
        let n = buf.pop_rx(&mut out);
        assert_eq!(n, 4);
        assert_eq!(&out, b"efgh");
    }
```

- [ ] **Step 11: 验证所有测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer
```

Expected: 所有测试 PASS

- [ ] **Step 12: 提交**

```bash
git add kernel/src/drivers/serial/ring_buffer.rs
git commit -m "feat(uart-async): 实现 AsyncBuffer (Ring Buffer + PollSet)"
```

---

## Task 3: 实现 ConsoleDriver

**Files:**
- Modify: `kernel/src/drivers/serial/console_driver.rs`

- [ ] **Step 1: 定义 ConsoleDriver 结构**

```rust
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use core::future::poll_fn;
use core::task::Poll;

use axtask::future::{block_on, register_irq_waker};

use super::ring_buffer::AsyncBuffer;

/// Console driver with RX copier task
pub struct ConsoleDriver {
    buffer: Arc<AsyncBuffer>,
    rx_irq: usize,
    rx_copier_started: AtomicBool,
}
```

Write to: `kernel/src/drivers/serial/console_driver.rs`

- [ ] **Step 2: 实现 ConsoleDriver::new()**

```rust
impl ConsoleDriver {
    pub fn new() -> Arc<Self> {
        let driver = Arc::new(Self {
            buffer: Arc::new(AsyncBuffer::new_default()),
            rx_irq: axhal::console::irq_num().unwrap_or(10),
            rx_copier_started: AtomicBool::new(false),
        });
        
        // Start RX copier task
        driver.start_rx_copier();
        
        driver
    }
    
    pub fn buffer(&self) -> &Arc<AsyncBuffer> {
        &self.buffer
    }
}
```

- [ ] **Step 3: 实现 RX copier 任务**

```rust
impl ConsoleDriver {
    fn start_rx_copier(self: &Arc<Self>) {
        if self.rx_copier_started.swap(true, Ordering::SeqCst) {
            return;  // Already started
        }
        
        axtask::spawn_with_name({
            let driver = self.clone();
            move || {
                block_on(poll_fn(|cx| {
                    let mut tmp_buf = [0u8; 256];
                    
                    // 1. Read from Console
                    let n = axhal::console::read_bytes(&mut tmp_buf);
                    
                    // 2. Write to rx_buf
                    if n > 0 {
                        driver.buffer.push_rx(&tmp_buf[..n]);
                        driver.buffer.wake_rx();
                    }
                    
                    // 3. Register IRQ waker
                    register_irq_waker(driver.rx_irq, cx.waker());
                    
                    // 4. Check again before pending
                    let n2 = axhal::console::read_bytes(&mut tmp_buf);
                    if n2 > 0 {
                        driver.buffer.push_rx(&tmp_buf[..n2]);
                        driver.buffer.wake_rx();
                    }
                    
                    // 5. Return Pending (wait for next IRQ)
                    Poll::Pending
                }))
            }
        }, "rx-copier".into());
    }
}
```

- [ ] **Step 4: 实现 TX 同步发送（flush_tx_sync）**

```rust
impl ConsoleDriver {
    /// Flush TX buffer to Console (synchronous, M1 simplified)
    pub fn flush_tx_sync(&self) {
        let mut tmp_buf = [0u8; 256];
        loop {
            let n = self.buffer.pop_tx(&mut tmp_buf);
            if n == 0 {
                break;
            }
            axhal::console::write_bytes(&tmp_buf[..n]);
        }
        self.buffer.wake_tx();
    }
}
```

- [ ] **Step 5: 验证编译**

```bash
cargo check --package starry-kernel
```

Expected: 编译通过

- [ ] **Step 6: 提交**

```bash
git add kernel/src/drivers/serial/console_driver.rs
git commit -m "feat(uart-async): 实现 ConsoleDriver + RX copier 任务"
```

---

## Task 4: 实现 AsyncUartTestDevice（DeviceOps + Pollable）

**Files:**
- Modify: `kernel/src/drivers/serial/device_ops.rs`

- [ ] **Step 1: 定义 AsyncUartTestDevice 结构**

```rust
use alloc::sync::Arc;
use core::sync::atomic::{AtomicBool, Ordering};
use core::task::Context;

use axerrno::{AxError, AxResult};
use axpoll::{IoEvents, Pollable};
use axtask::future::{block_on, poll_io};

use crate::pseudofs::DeviceOps;

use super::console_driver::ConsoleDriver;

/// Test device for async UART architecture validation
pub struct AsyncUartTestDevice {
    driver: Arc<ConsoleDriver>,
    non_blocking: AtomicBool,
}
```

Write to: `kernel/src/drivers/serial/device_ops.rs`

- [ ] **Step 2: 实现 AsyncUartTestDevice::new()**

```rust
impl AsyncUartTestDevice {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            driver: ConsoleDriver::new(),
            non_blocking: AtomicBool::new(false),
        })
    }
    
    fn nonblocking(&self) -> bool {
        self.non_blocking.load(Ordering::Acquire)
    }
}
```

- [ ] **Step 3: 实现 DeviceOps trait**

```rust
impl DeviceOps for AsyncUartTestDevice {
    fn read_at(&self, buf: &mut [u8], _offset: u64) -> AxResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        
        block_on(poll_io(
            self,
            IoEvents::IN,
            self.nonblocking(),
            || {
                let n = self.driver.buffer().pop_rx(buf);
                if n > 0 {
                    Ok(n)
                } else {
                    Err(AxError::WouldBlock)
                }
            },
        ))
    }

    fn write_at(&self, buf: &[u8], _offset: u64) -> AxResult<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        
        block_on(poll_io(
            self,
            IoEvents::OUT,
            self.nonblocking(),
            || {
                let n = self.driver.buffer().push_tx(buf);
                if n > 0 {
                    self.driver.flush_tx_sync();
                    Ok(n)
                } else {
                    Err(AxError::WouldBlock)
                }
            },
        ))
    }

    fn as_pollable(&self) -> Option<&dyn Pollable> {
        Some(self)
    }
}
```

- [ ] **Step 4: 实现 Pollable trait**

```rust
impl Pollable for AsyncUartTestDevice {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        
        let rx_buf = self.driver.buffer().rx_buf.lock();
        let tx_buf = self.driver.buffer().tx_buf.lock();
        
        events.set(IoEvents::IN, rx_buf.occupied_len() > 0);
        events.set(IoEvents::OUT, tx_buf.vacant_len() > 0);
        
        events
    }

    fn register(&self, cx: &mut Context, events: IoEvents) {
        if events.contains(IoEvents::IN) {
            self.driver.buffer().register_rx_waker(cx.waker());
        }
        if events.contains(IoEvents::OUT) {
            self.driver.buffer().register_tx_waker(cx.waker());
        }
    }
}
```

- [ ] **Step 5: 验证编译**

```bash
cargo check --package starry-kernel
```

Expected: 编译通过

- [ ] **Step 6: 更新 serial/mod.rs 导出**

```rust
//! Async UART driver (M1 architecture validation)

mod ring_buffer;
mod console_driver;
mod device_ops;

pub use device_ops::AsyncUartTestDevice;
pub use ring_buffer::AsyncBuffer;
pub use console_driver::ConsoleDriver;
```

Edit: `kernel/src/drivers/serial/mod.rs`

- [ ] **Step 7: 提交**

```bash
git add kernel/src/drivers/serial/device_ops.rs kernel/src/drivers/serial/mod.rs
git commit -m "feat(uart-async): 实现 AsyncUartTestDevice (DeviceOps + Pollable)"
```

---

## Task 5: 设备注册到 devfs

**Files:**
- Modify: `kernel/src/pseudofs/dev/mod.rs`

- [ ] **Step 1: 在 dev/mod.rs 中添加 import**

在文件顶部添加：

```rust
#[cfg(feature = "uart-async")]
use crate::drivers::serial::AsyncUartTestDevice;
```

Edit: `kernel/src/pseudofs/dev/mod.rs` 第 24 行后

- [ ] **Step 2: 在 builder() 中注册设备**

在 `builder()` 函数的 `root.add("cpu_dma_latency", ...)` 之后添加：

```rust
    // Async UART test device (M1 architecture validation)
    #[cfg(feature = "uart-async")]
    root.add(
        "async_uart_test",
        Device::new(
            fs.clone(),
            NodeType::CharacterDevice,
            DeviceId::new(4, 64),  // Experimental device ID
            AsyncUartTestDevice::new(),
        ),
    );
```

Edit: `kernel/src/pseudofs/dev/mod.rs` 第 270 行后

- [ ] **Step 3: 添加 feature 到 Cargo.toml**

在 `kernel/Cargo.toml` 的 `[features]` 部分添加：

```toml
[features]
# ... existing features ...
uart-async = []  # Async UART architecture validation (M1)
```

检查 `kernel/Cargo.toml` 确认 features 部分

- [ ] **Step 4: 验证编译**

```bash
cargo check --package starry-kernel --features uart-async
```

Expected: 编译通过

- [ ] **Step 5: 提交**

```bash
git add kernel/src/pseudofs/dev/mod.rs kernel/Cargo.toml
git commit -m "feat(uart-async): 注册 async_uart_test 设备到 devfs"
```

---

## Task 6: Gate M1 验证

- [ ] **Step 1: 运行单元测试**

```bash
cargo test --package starry-kernel --lib drivers::serial::ring_buffer
```

Expected: 所有测试 PASS

- [ ] **Step 2: 编译内核**

```bash
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH
make build
```

Expected: 编译成功

- [ ] **Step 3: 运行内核**

```bash
make run
```

Expected: 内核启动，进入 shell

- [ ] **Step 4: 手动验证设备存在**

在 shell 中：
```bash
ls /dev/
```

Expected: 看到 `async_uart_test`

- [ ] **Step 5: 手动验证写入**

在 shell 中：
```bash
echo "hello" > /dev/async_uart_test
```

Expected: Console 输出 "hello"

- [ ] **Step 6: 手动验证读取**

在 shell 中：
```bash
cat /dev/async_uart_test
```

然后在 QEMU 外部终端输入字符。

Expected: 输入的字符在 shell 中显示

- [ ] **Step 7: 最终提交**

```bash
git add -A
git commit -m "feat(uart-async): M1 架构验证完成 (Gate M1)"
```

---

## Self-Review

### Spec Coverage Check

| Spec 章节 | 对应任务 |
|----------|---------|
| §3 Ring Buffer | Task 2 |
| §4 ConsoleDriver | Task 3 |
| §5 DeviceOps | Task 4 |
| §5.4 设备注册 | Task 5 |
| §6 验证流程 | Task 6 |

✅ 所有 spec 要求已覆盖

### Placeholder Scan

✅ 无 TBD、TODO、"implement later" 等占位符
✅ 所有代码步骤包含完整实现代码
✅ 所有验证步骤包含具体命令和预期结果

### Type Consistency

- `AsyncBuffer::push_rx` → `ConsoleDriver` 使用
- `AsyncBuffer::pop_rx` → `AsyncUartTestDevice::read_at` 使用
- `ConsoleDriver::buffer()` → `Arc<AsyncBuffer>` 返回类型一致
- `AsyncUartTestDevice::new()` → `Arc<Self>` 返回类型一致

✅ 类型和方法签名一致

---

## 依赖关系

```
Task 1 (目录结构) → Task 2 (ring_buffer) → Task 3 (console_driver)
                                              ↓
                                         Task 4 (device_ops)
                                              ↓
                                         Task 5 (设备注册)
                                              ↓
                                         Task 6 (Gate验证)
```

**推荐执行顺序**: T1 → T2 → T3 → T4 → T5 → T6（串行依赖）