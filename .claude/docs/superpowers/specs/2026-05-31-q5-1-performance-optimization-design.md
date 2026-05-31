# Q5.1 性能优化设计

> 日期：2026-05-31
> 分支：feat/uart-async-dev2
> 状态：设计批准

---

## 背景

Q5 已完成基础性能优化（IER 缓存、ISR 合并、批量 I/O、waker skip、rx/tx 独立锁）。Q5.1 继续优化剩余项。

## 优化清单

| 编号 | 内容 | 优先级 | 预期效果 |
|------|------|--------|----------|
| O7 | uart_16550 批量读写 API | 🔴 高 | 减少 50% 函数调用开销 |
| O2/O34 | NAPI 中断合并 | 🟡 中 | 高吞吐时减少中断频率 |
| O4/O35 | FCR 阈值调优 | 🟢 低 | 优化延迟/吞吐平衡 |
| O17 | 中断分发效率 | 🟢 低 | 减少分支预测失败 |

---

## 优化 1：uart_16550 批量读写 API（O7）

### 当前状态

- `async_driver.rs` 中 RX/TX copier 使用 `try_receive_byte`/`try_send_byte` 逐字节读写
- 已用单锁 batch（COPIER_BUF_SIZE=1024），但函数调用开销大

### 优化方案

使用 `uart_16550` 的 `receive_bytes`/`send_bytes` API：

```rust
// rx_copier_loop 中
let total = uart.receive_bytes(&mut read_buf);

// tx_copier_loop 中
let sent = uart.send_bytes(&write_buf[..pending]);
```

### 预期效果

- 减少函数调用次数（从 N 次降到 1 次）
- 减少 MMIO 访问开销
- 代码更简洁

### 实现位置

- `kernel/src/drivers/async_driver.rs`：`rx_copier_loop` 和 `tx_copier_loop`

---

## 优化 2：NAPI 中断合并（O2/O34）

### 当前状态

- 每次中断唤醒 copier
- copier 读完 FIFO 后重新使能中断
- 高吞吐时中断频繁，影响性能

### 优化方案

高吞吐时切轮询模式：

```rust
const NAPI_THRESHOLD: usize = 16; // 连续成功次数
const NAPI_BATCH_SIZE: usize = 64; // 轮询批次大小

let mut consecutive_success = 0;
loop {
    let n = uart.receive_bytes(&mut read_buf[..NAPI_BATCH_SIZE]);
    if n > 0 {
        consecutive_success += 1;
        if consecutive_success >= NAPI_THRESHOLD {
            // 轮询模式：不重新使能中断，继续读
            continue;
        }
    } else {
        consecutive_success = 0;
        enable_rx_intr(); // 恢复中断驱动
    }
}
```

### 预期效果

- 高吞吐时中断频率降低 90%+
- 低吞吐时保持中断驱动（低延迟）

### 实现位置

- `kernel/src/drivers/async_driver.rs`：`rx_copier_loop`
- `kernel/src/drivers/uart_init.rs`：添加 NAPI 配置常量

---

## 优化 3：FCR 阈值调优（O4/O35）

### 当前状态

- FIFO 触发阈值 14 字节（默认）
- 未验证是否最优

### 优化方案

1. 读取当前 FCR 配置
2. 测试不同阈值（4/8/14）的性能影响
3. 选择最优阈值

### 预期效果

- 优化延迟/吞吐平衡
- 减少中断频率（高阈值）或减少延迟（低阈值）

### 实现位置

- `kernel/src/drivers/uart_init.rs`：添加 FCR 配置函数
- `.claude/docs/optimization.md`：记录测试结果

---

## 优化 4：中断分发效率（O17）

### 当前状态

- ISR 中使用 `match isr.interrupt_type()` 分发
- 分支预测可能失败

### 优化方案

使用查表法：

```rust
// 使用查表法
static ISR_HANDLER: [fn(); 8] = [
    handle_no_intr,      // 0
    handle_rx_ready,     // 1
    handle_rx_timeout,   // 2
    handle_tx_empty,     // 3
    // ...
];

let isr_val = isr.value() as usize;
ISR_HANDLER[isr_val]();
```

### 预期效果

- 减少分支预测失败
- 代码更清晰

### 实现位置

- `kernel/src/drivers/isr.rs`：重构中断分发逻辑

---

## 实现顺序

1. **O7：批量读写 API**（最简单，效果明显）
2. **O2/O34：NAPI 中断合并**（中等复杂度，效果显著）
3. **O4/O35：FCR 阈值调优**（需要测试）
4. **O17：中断分发效率**（最后实现）

---

## 验证标准

### Gate Q5.1：性能基准测试通过

| 指标 | 目标 | 测量方法 |
|------|------|----------|
| 吞吐量 @115200 | > 10 KB/s (90% 线速) | 5 秒批量传输 |
| 延迟 P50 | < 500 µs | 100 次单字节 echo |
| 延迟 P99 | < 2 ms | 同上 |
| 空闲 CPU | 0% (完全挂起) | 无数据 10 秒 |

---

## 风险与缓解

| 风险 | 缓解措施 |
|------|----------|
| NAPI 阈值不合适 | 提供配置接口，运行时调整 |
| FCR 阈值影响稳定性 | 先在 QEMU 测试，再上真板 |
| 批量 API 兼容性 | 已验证 uart_16550 API，无风险 |

---

## 依赖

- uart_16550 v0.6.0（已有批量 API）
- 当前 Q5 优化已落地（IER 缓存、ISR 合并等）

---

## 参考

- [uart_16550 API](../uart_16550/src/lib.rs)
- [NAPI Wikipedia](https://en.wikipedia.org/wiki/New_API)
- [Linux NAPI](https://www.kernel.org/doc/html/latest/networking/napi.html)
