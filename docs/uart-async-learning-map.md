# StarryOS 异步串口学习地图

> 通读：OpenSpec 4 域 + uart_16550 crate + StarryOS 适配层（约 3700 行）
> 范围：Q0~Q15 完整异步串口实现
> 日期：2026-06-25

## 架构总览

5 层结构、2 trait OS 抽象、3 数据流、6 关键模式。

```
L5 用户态        Shell / programs
L4 VFS/TTY       Tty<Reader, Writer>
L3 集成层        AsyncUartReader / AsyncUartWriter
L2 驱动层        AsyncUartDriver<R, W, U> + Rx/Tx Copier
L1 硬件抽象      UartPort trait
L0 硬件          NS16550 @ 0x10000000, stride=1
```

OS 抽象边界仅 2 trait（ADR-036）：

- `OsRuntime` 提供 `spawn` + `block_on`
- `OsWakerSet` 提供 `register` + `wake`

## 五层结构

| 层 | 主要类型 | 路径 |
|---|---|---|
| L0 硬件 | NS16550 UART | `kernel/src/drivers/uart_init.rs:38-43` |
| L1 抽象 | `UartPort` trait | `uart_16550/src/async_/driver.rs:48-74` |
| L2 驱动 | `AsyncUartDriver<R, W, U>` | `uart_16550/src/async_/driver.rs:115-145` |
| L3 集成 | `AsyncUartReader` / `AsyncUartWriter` | `uart_16550/src/async_/device_ops.rs:24-107` |
| L4 VFS | `AsyncTty` | `kernel/src/drivers/ntty_async.rs` |
| L5 用户态 | Shell | — |

## 三个数据流

**RX 路径**：
UART RBR → ISR 读 ISR 寄存器 → `RX_WAKER.wake()` → RX copier poll 唤醒 → `receive_bytes` → `rx.push_batch` → `rx.poll.wake` → `TtyRead::read` pop。

**TX 路径**：
用户 write → `TtyWrite::write` → `tx.push` → `tx.poll.wake` → TX copier poll 唤醒 → `send_bytes` → THRE ISR → `TX_WAKER.wake()`（含 `DRAIN_WAKER.wake()` if TEMT）。

**Drain 路径**：
轮询 `TxCompletion::is_drained()`。四条件全满足返回：

- `ring_empty`
- `!copier_active`
- `staged_bytes == 0`
- `transmitter_empty`

## 六个关键模式

| 模式 | 体现位置 | 思想 |
|---|---|---|
| ISR 极简 | `isr.rs:71-92` | 4 步：读 ISR / 禁中断 / wake / 返回 |
| SPSC 无锁 ring | `ring_buffer.rs:79-88` | `embassy_hal_internal` + `UnsafeCell` |
| AtomicWaker 唤醒 | `isr.rs:14-20` | 3 静态 waker，`O(1)` 复杂度 |
| poll_fn + register | `driver.rs:221-258` | WouldBlock → register → Pending |
| NAPI 中断合并 | `driver.rs:222-248` | 16 次成功 → 轮询模式（batch=64） |
| tcdrain 四阶段 | `driver.rs:81-98` + `device_ops.rs:126-153` | ring / copier / staged / TEMT |

## 关键常量

| 常量 | 值 | 位置 | 含义 |
|---|---|---|---|
| `NAPI_THRESHOLD` | 16 | `driver.rs:27` | 连续成功切换轮询 |
| `NAPI_BATCH_SIZE` | 64 | `driver.rs:29` | 轮询模式批量 |
| `COPIER_BUF_SIZE` | 1024 | `driver.rs:31` | copier 缓冲 |
| `TX_FAST_RETRY_LIMIT` | 32 | `driver.rs:33` | 单 poll TX 重试上限 |
| `TX_TEMT_POLL_LIMIT` | 256 | `driver.rs:35` | TEMT 自旋上限 |
| `BUF_SIZE` | 64 KiB | `uart_init.rs:46` | ring buffer 大小 |

## 五个里程碑方法

| 阶段 | 关键经验 |
|---|---|
| Q0~Q7 | kernel 层独立实现，不改外部 crate |
| Q8 打磨 | NAPI 退出修复、ISR 去锁、IER 单 owner |
| Q12 Embassy A | atomic_ring_buffer + embedded_io_async + TC tcdrain |
| Q13 提取 | 异步栈搬到 uart_16550，5 trait → 2 trait |
| Q15 增量融合 | M4 Sync 一次性失败 73.9x → 5 个原子 milestone |

## 学习进度

截至 2026-06-25：3/8 完成（37.5%）

| 站 | 主题 | 状态 |
|---|---|---|
| 1 | 入口与全局实例 | ✅ |
| 2 | `AsyncUartDriver` 三泛型结构 | ✅ |
| 3 | ISR 4 步流程 + 3 个 AtomicWaker | ✅ |
| 4 | RX copier + NAPI 状态机 | ⬜ |
| 5 | TX copier + fast retry + TEMT | ⬜ |
| 6 | `ProcessMode::External` 桥接 | ⬜ |
| 7 | OS 抽象具体实现 | ⬜ |
| 8 | VFS 接口 + flush 实现 | ⬜ |

**已写笔记**（7 篇）：

- `async-uart-entry.md`（第 1 站）
- `isr-disable-level-trigger.md`（第 1 站 Q&A）
- `async-driver-generics.md`（第 2 站）
- `memory-ordering-smp.md`（第 2 站 Q&A + O63）
- `async-driver-thread-safety.md`（第 2 站 + 第 3 站）
- `isr-minimal-4-step.md`（第 3 站）
- `rust-async-driver-basics.md`（Rust 基础复习）

## 学习路径

按以下顺序读，逐步深入。

1. ✅ `kernel/src/drivers/mod.rs` + `uart_init.rs:38-73`
   入口与全局实例。
2. ✅ `uart_16550/src/async_/driver.rs:115-145`
   `AsyncUartDriver` 结构与 3 泛型。
3. ✅ `uart_16550/src/async_/isr.rs:14-92`
   3 个 AtomicWaker + ISR 4 步。
4. `driver.rs:215-261`
   RX copier 与 NAPI 状态机。
5. `driver.rs:263-380`
   TX copier 与 fast retry + TEMT。
6. `kernel/src/drivers/ntty_async.rs`
   `ProcessMode::External` 桥接（Q7 O42 修复 yield storm）。
7. `kernel/src/drivers/os_arceos.rs`
   OS 抽象具体实现。
8. `uart_16550/src/async_/device_ops.rs`
   VFS 接口与 flush 实现。

## 阅读入口速查

| 文件 | 行数 | 主题 |
|---|---|---|
| `uart_16550/src/async_/mod.rs` | 13 | 模块入口 |
| `uart_16550/src/async_/driver.rs` | 381 | 异步驱动主逻辑 |
| `uart_16550/src/async_/isr.rs` | 92 | ISR handler |
| `uart_16550/src/async_/ring_buffer.rs` | 240 | ring buffer + waker |
| `uart_16550/src/async_/device_ops.rs` | 154 | VFS 接口 |
| `uart_16550/src/os/mod.rs` | 61 | OS 抽象 trait |
| `kernel/src/drivers/uart_init.rs` | 301 | 初始化 + UartPort 实现 |
| `kernel/src/drivers/ntty_async.rs` | 29 | AsyncTty |
| `kernel/src/drivers/os_arceos.rs` | 63 | ArceOS 适配 |

## 主要 ADR 索引

| 编号 | 主题 |
|---|---|
| ADR-001 | 异步运行时选型 |
| ADR-004 | 缓冲策略 HeapRb + PollSet |
| ADR-006 | 硬件抽象 AsyncUart trait |
| ADR-025/027 | kernel 层独立实现 |
| ADR-026 | stride=4 根因 |
| ADR-032/033 | uart_16550 提取 |
| ADR-035/036 | OS 抽象 5 trait → 2 trait |
| ADR-037 | TxCompletion 四阶段 |
| ADR-038 | TtyWrite 短写契约 |
| ADR-039 | Q15 增量融合策略 |

详细见 `openspec/specs/architecture/spec.md`。