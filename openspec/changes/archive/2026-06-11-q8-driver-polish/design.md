# Q8 技术方案

## Wave 1: 正确性修复

### Q8.1 NAPI 退出修复

**当前 bug**（`async_driver.rs:51`）：
```rust
if consecutive >= NAPI_THRESHOLD { consecutive += 1; } // ← 永远不会退出！
else { consecutive = if total > 0 { consecutive + 1 } else { 0 }; }
```

**修复**：在 NAPI 模式添加零字节退出逻辑：
```rust
if consecutive >= NAPI_THRESHOLD {
    if total > 0 {
        consecutive += 1;
    } else {
        consecutive = 0;          // 零字节 → 退出 NAPI
        enable_rx_intr();         // 重新使能 RX 中断
    }
} else {
    consecutive = if total > 0 { consecutive + 1 } else { 0 };
}
```

**影响文件**：仅 `kernel/src/drivers/async_driver.rs`

### Q8.2 ISR 去锁化

**当前问题**（`isr.rs:9-10`）：
```rust
pub fn uart_isr_handler(_irq: usize) {
    let mut uart = uart_instance().lock(); // ← ISR 中获取 SpinNoIrq
```

**方案**：将 `isr()` 调用移出锁范围。由于 uart_16550 的 `isr()` 方法内部只是 `read_volatile`，在 ISR 上下文中（单 ISR、无并发）安全。

```rust
pub fn uart_isr_handler(_irq: usize) {
    // 无锁读取 ISR（read_volatile on MMIO，单 ISR 安全）
    let isr = unsafe { uart_instance_unlocked().isr_unchecked() };
    match isr.interrupt_type() {
        Some(InterruptType::ReceivedDataReady) | Some(InterruptType::ReceptionTimeout) => {
            disable_rx_intr();
            RX_WAKER.wake();
        }
        Some(InterruptType::TransmitterHoldingRegisterEmpty) => {
            disable_tx_intr();
            TX_WAKER.wake();
            DRAIN_WAKER.wake();
        }
        _ => {}
    }
}
```

**备选方案**：在 uart_16550 添加无锁 `isr_unchecked(&self)` 方法

**影响文件**：`isr.rs`（+ 可能需要 `uart_init.rs` 添加无锁访问方法）

### Q8.3 IER 写路径规范化

**当前问题**（`uart_init.rs:72`）：
```rust
unsafe { core::ptr::write_volatile(ptr.add(offsets::IER as usize), value) };
```

**方案**：向 uart_16550 添加 `set_ier()` 公共方法：
```rust
// uart_16550/src/lib.rs
pub fn set_ier(&mut self, ier: IER) {
    // SAFETY: IER offset is within valid register range
    unsafe { self.backend.write_byte(offsets::IER, ier.bits()); }
}
```

StarOS 端改为：
```rust
fn write_ier(value: u8) {
    CACHED_IER.store(value, Ordering::Relaxed);
    uart_instance().lock().set_ier(IER::from_bits_truncate(value));
}
```

**注意**：此方案在 `write_ier` 中获取了 `uart_instance().lock()`，与 Q8.2 的目标（ISR 去锁）形成矛盾。Wave 1 先做 Q8.3，Q8.2 在 Wave 1 中评估备选方案（直接添加无锁 IER 写方法）或推迟到 Wave 2。

**影响文件**：`uart_16550/src/lib.rs`、`kernel/src/drivers/uart_init.rs`

## Wave 2: 热路径优化

### Q8.4 copier waker 去重简化

**当前**（`async_driver.rs:53-55`）：
```rust
let w = cx.waker().clone();  // ← 总是 clone
if last_waker.replace(Some(w.clone())).as_ref().map_or(true, |old| !old.will_wake(&w)) {
    RX_WAKER.register(cx.waker());
}
```

**简化**：
```rust
let w = cx.waker();
if last_waker.get().map_or(true, |old| !old.will_wake(w)) {
    last_waker.set(Some(w.clone()));
    RX_WAKER.register(w);
}
```

**收益**：仅在 waker 变化时 clone，节省 ~20-40ns/poll

### Q8.5 DRAIN_WAKER 条件唤醒

**当前**（`isr.rs:20`）：每次 TX ISR 无条件 `DRAIN_WAKER.wake()`

**方案**：添加 `AtomicBool` 标志位
```rust
// isr.rs
use core::sync::atomic::{AtomicBool, Ordering};
static TCDRAIN_ACTIVE: AtomicBool = AtomicBool::new(false);

// tcdrain 路径设置
TCDRAIN_ACTIVE.store(true, Ordering::Release);
DRAIN_WAKER.register(cx.waker());
// ... double-check pattern ...
TCDRAIN_ACTIVE.store(false, Ordering::Release);

// ISR 中条件唤醒
if TCDRAIN_ACTIVE.load(Ordering::Acquire) {
    DRAIN_WAKER.wake();
}
```

## Wave 3: O46 AtomicWaker 推广

### 通用模式

所有 PollSet 替换为 AtomicWaker 遵循同一模式：

```rust
// Before:
struct Shared {
    poll_rx: PollSet,  // spinlock + 64 slots
}

// After:
struct Shared {
    rx_waker: AtomicWaker,  // lock-free, single slot
}
```

**风险排序**（低→高）：
1. **signalfd**（1 PollSet，2 个唤醒源）— 最低风险
2. **event**（2 PollSet，交叉唤醒）— 中低风险
3. **pipe**（3 PollSet，跨操作唤醒 + Drop 唤醒）— 中风险
4. **pidfd**（1 Arc\<PollSet\>，共享于 task 结构体）— 高风险

### pidfd 共享问题

pidfd 的 `exit_event` 是 `Arc<PollSet>`，克隆自 `Thread.exit_event` / `ProcessData.exit_event`。替换为 `Arc<AtomicWaker>` 需要：
- 修改 `task/mod.rs` 中的 `Thread` 和 `ProcessData` 结构体定义
- 修改 `task/ops.rs` 中的退出唤醒路径
- 修改 `syscall/task/wait.rs` 中的 child_exit_event 注册

**默认假设**：async 模型下同一 pidfd 最多 1 个 waiter，单槽 AtomicWaker 足够。
