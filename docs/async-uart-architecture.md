# StarryOS 异步串口架构（Q20 当前状态）

> 分支：`uart-16550-lichee`  
> 状态：Q20 benchmark gap closure 已完成（2026-07-13）  
> 仓库：[StarryOS](https://github.com/daivy2333/StarryOS) · [uart_16550](https://github.com/daivy2333/uart_16550)  
> 关联：`benchmark-report-async.md`（性能）· `manual-qa-report.md`（QA）

## 架构概览

StarryOS 的异步 UART 是提交队列 + 后台 copier 的模型。用户 `write()` 只把数据提交到 TX ring，物理发送由 `uart-tx-copier` 推进；`tcdrain()` / `flush()` 才等待物理完成边界。

这个模型接近 `io_uring` 的提交/完成分离：准备数据和物理发送可以并行。应用继续写入 ring 时，硬件上一批数据仍在移位发送；copier 在 FIFO 可写时批量补充 THR。这样减少“等中断反馈后才准备下一批数据”的空窗。

中断不是数据搬运者，只是“可能有进展”的提示。数据未发完时提前进入 TX ring，不能消除所有中断，但能让 copier 在 THRE 到来或软件探测到 FIFO 空位时马上续写，减少无进展等待。

## 数据流

用户态 ↔ UART 硬件（自上而下读）：

```
用户 read()
  → VFS → File::read → block_on(poll_io(...))
    → Tty::read_at → ldisc → ring_buf.pop
      ↑ tty-reader task (InputReader::poll)
        ↑ AsyncUartReader::read → RingBufRx.pop
          ↑ RX copier ← ISR(RX_WAKER.wake) ← UART RX FIFO

用户 write()
  → VFS → File::write
    → Tty::write_at → AsyncUartWriter::write → RingBufTx.push
      ↓ TX copier → ISR(TX_WAKER.wake) → UART TX THR
        └ tcdrain: DRAIN_WAKER 等 TEMT
```

写路径有三个不同完成层级：

| 层级 | 含义 | 观察者 |
|------|------|--------|
| accepted | 数据进入 TX ring | `write()` / `writev()` 返回值 |
| staged | copier 从 ring 取出，尚未全写入 FIFO | `tx_staged_bytes` |
| drained | ring 空、copier 空、staged=0、TEMT=1 | `tcdrain()` / `flush()` |

普通 `write()` 不等待物理发送完成。`tcdrain()` 等待完整 drain 状态，所以它比 `write()` 慢，但语义更强。

读路径 5 次拷贝定位见 `ldisc.rs`。

## 主要抽象

| 抽象 | 位置 | 作用 |
|------|------|------|
| `OsRuntime` trait | [os_arceos.rs](../kernel/src/drivers/os_arceos.rs) | 任务生成 + 同步等待 |
| `OsWakerSet` trait | [os_arceos.rs](../kernel/src/drivers/os_arceos.rs) | 多 waker 集合 |
| `UartPort` trait | [driver.rs](../crates/uart_16550/src/async_/driver.rs) | IRQ-safe 寄存器访问 |
| `AsyncUartDriver<R, W, U>` | [driver.rs](../crates/uart_16550/src/async_/driver.rs) | 3 泛型驱动主类型 |
| `TxCompletion` | [driver.rs](../crates/uart_16550/src/async_/driver.rs) | `tcdrain()` / `flush()` 的完成快照 |
| `TxDebugSnapshot` | [driver.rs](../crates/uart_16550/src/async_/driver.rs) | D1/QEMU counter proxy |
| `TtyRead` / `TtyWrite` trait | [ldisc.rs](../kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) | 通用 TTY 抽象 |

OS 抽象数量：Q13 提取时是 5 个 trait。Q13-cleanup（ADR-036）删除 3 个（`OsIrq` / `OsMmio` / `OsSpinNoIrq`），剩 2 个。IRQ 注册、MMIO 映射、锁获取由集成层在 driver 外部处理。

## 异步发送模型

TX ring 是发送提交队列。`AsyncUartWriter::write()` 调用 `RingBufTx::push()`，把调用方数据复制到 64 KiB ring，并返回实际接受字节数。ring 满时允许 short write，调用方必须按返回值继续提交。

`uart-tx-copier` 是后台发送者。它用 1024B staging buffer 从 TX ring 批量 `pop_batch()`，再调用 `UartPort::send_bytes()` 写硬件 FIFO。D1 的 `send_bytes()` 每次最多写 16B FIFO；QEMU 路径通常更快返回可写。

流程如下：

```text
producer write()
  -> RingBufTx.push       # 数据先入提交队列
  -> 返回 accepted bytes

uart-tx-copier
  -> RingBufTx.pop_batch  # 一次最多 1024B staged
  -> send_bytes           # 尽量填 FIFO
  -> FIFO 满则有限 retry / slow-poll / 等 TX_WAKER
```

这个设计让“准备下一批数据”和“上一批物理发送”重叠。硬件 115200 bps 发送 16B 约需 1.39 ms，用户态无需在每 16B 后同步等待。copier 负责在 FIFO 出现空间时继续补数据。

THRE 中断仍然有用，但不作为唯一进展来源。TX copier 先做 32 次 fast retry；仍无进展时注册 `TX_WAKER`、打开 `THR_EMPTY` 中断并 recheck；D1 上再进入 bounded slow-poll。这样可以覆盖 THRE 边沿丢失或中断到达前 FIFO 已经可写的情况。

## 批处理与进展策略

| 策略 | 当前值 | 作用 | 边界 |
|------|--------|------|------|
| TX/RX ring | 64 KiB × 2 | 解耦用户态与硬件 FIFO | ring 满时 short write |
| copier buffer | 1024B | 减少 ring pop 次数 | 不改变物理线速 |
| D1 FIFO burst | 16B | 一次填满 TX FIFO | FIFO 满时 `send_bytes=0` |
| TX fast retry | 32 | 避免 16B refill 落到 tick 级等待 | 有界，不无限 busy loop |
| TX slow-poll | 4096 × 256 spins | D1 THRE 边沿丢失 fallback | QEMU 通常不触发 |
| yield retry | 4 | slow-poll 耗尽后让出调度再试 | 耗尽后回到纯 ISR wait |
| RX NAPI threshold | 16 | 连续读到数据后进入 polling | 零字节退出 polling |
| RX NAPI batch | 64B | 高输入率下降低 IRQ 频率 | 不无限扩大 batch |

这些策略的目标是减少反馈等待，而不是绕过物理线速。D1 Q20 数据显示用户态 TX 接近 115200 bps 线速；内核态 ring benchmark 则说明内部队列能力远高于线速。

## 驱动层

**初始化**：[uart_init.rs](../kernel/src/drivers/uart_init.rs) 持有 `SpinNoIrq<Uart16550<MmioBackend>>` 单例。MMIO 基址 `0x10000000`，stride 必须传 1（NS16550 仅 8 字节寄存器，stride=4 → LoadFault，见 L122 教训）。NAPI 阈值 16、FIFO 触发 14 字节。

**`UartPort` IER 单 owner**：`update_ier(set, clear)` 是唯一入口。本地 `ier_cache` 原子更新 + 硬件 `set_ier()` 同步：

```rust
fn update_ier(&self, set: IER, clear: IER) {
    let mut val = self.ier_cache.load(Relaxed);
    val |= set.bits();
    val &= !clear.bits();
    self.ier_cache.store(val, Relaxed);
    self.uart.lock().set_ier(IER::from_bits_truncate(val));
}
```

Q15-M4 前存在 `CACHED_IER` 全局 + `write_ier()` + `enable_rx_intr()` / `disable_tx_intr()` 多入口。Q15-M4 删除，统一通过 `update_ier`。

**ISR 极简**：[uart_init.rs](../kernel/src/drivers/uart_init.rs) 桥接 axhal IRQ hook → `uart_16550::async_::isr::uart_isr_handler`：

```rust
fn uart_isr_wrapper(_irq: usize) {
    let base = NonNull::new(get_uart_mmio_virt().as_mut_ptr()).unwrap();
    uart_16550::async_::isr::uart_isr_handler(
        _irq, base,
        || UART_PORT.update_ier(IER::empty(), IER::DATA_READY),
        || UART_PORT.update_ier(IER::empty(), IER::THR_EMPTY),
    );
}
```

ISR 仅做：读 ISR → 禁对应中断 → `AtomicWaker::wake()` → 返回。禁止数据搬运、禁止加锁。D1 路径还会在 IIR 无 pending 但 LSR 已显示 THRE/TEMT 时主动 wake，避免漏掉已可进展状态。

**Copier 任务**：两个独立协程（`uart-rx-copier` / `uart-tx-copier`）搬运 FIFO ↔ ring buffer，循环 `poll_fn(...).await`。

**RX copier**（[driver.rs](../crates/uart_16550/src/async_/driver.rs)）：`receive_bytes` 批量读 FIFO → `RingBufRx::push` → NAPI 阈值检查 → `RX_WAKER.register` 等 ISR。连续成功 ≥16 次切轮询模式（batch=64），零字节退出轮询（Q8.1 修复）。

**TX copier**（[`driver.rs`](../crates/uart_16550/src/async_/driver.rs)）：`RingBufTx::pop_batch` 取数据 → `send_bytes` 批量写 THR。当前进展链为 fast retry 32 → 注册 `TX_WAKER` + enable THRE + recheck → slow-poll 4096×256 spins → yield retry 4 → 纯 ISR wait。`TxCompletion` API（`driver.tx_completion()`）统一提供 drain 检查点。

**Ring Buffer**（[ring_buffer.rs](../crates/uart_16550/src/async_/ring_buffer.rs)）：

| 缓冲 | 容量 | 同步 | 生产者 | 消费者 |
|------|------|------|--------|--------|
| `RingBufRx` | 64 KiB | `SpinNoIrq` | RX copier | tty-reader |
| `RingBufTx` | 64 KiB | `ArceOsRawMutex` (Q15-M0) | 用户态 + 内核日志 | TX copier |

Q15-M0 引入 `ArceOsRawMutex`（基于 `SpinNoIrq` 的 RawMutex）保护 TX writer，支持 `Clone-safe AsyncUartWriter`（Shell stdout + `ax_println!` 多生产者场景）。

## TTY 集成层

**`Tty<R, W>` 泛型绑定**：[tty/mod.rs](../kernel/src/pseudofs/dev/tty/mod.rs) 是 `/dev/console` 的 DeviceOps 实现。`R: TtyRead` / `W: TtyWrite` 是泛型参数。`AsyncUartReader` / `AsyncUartWriter` 实现这两个 trait，`Tty<AsyncUartReader, AsyncUartWriter>` 绑定到异步串口（[ntty_async.rs](../kernel/src/drivers/ntty_async.rs)）。

Q15-M3 将 `TtyWrite::write` 改为返回 `usize`（ADR-038）：实际接受字节数穿透到 VFS，避免 silent data loss。

**FIONBIO 三入口**：非阻塞状态必须覆盖三个入口，缺一不可（见 L140 教训）：

| 入口 | 文件 |
|------|------|
| `open(O_NONBLOCK)` | [fd_ops.rs](../kernel/src/syscall/fs/fd_ops.rs) |
| `fcntl(F_SETFL)` | [fd_ops.rs](../kernel/src/syscall/fs/fd_ops.rs) |
| `ioctl(FIONBIO)` | [ctl.rs](../kernel/src/syscall/fs/ctl.rs) |

`Tty` 持有 `nonblocking: AtomicBool`。三个入口都调用同一 setter。

**tcdrain（TCSBRK）** 三段式异步等待（Q15-M2 增强）：

```
1. ring buf 有数据 → 注册 tx.poll（copier pop 时唤醒）
2. ring buf 空 + UART 未排空 → 注册 DRAIN_WAKER（ISR TX 中断时唤醒）
3. ring buf 空 + LSR.TRANSMITTER_EMPTY → 返回
```

Q15-M2 引入 `TxCompletion` API 作为统一 drain 检查点，消除 tcdrain 访问 MMIO 的分层违规。

**VTIME 超时**：Q9 复用 `axtask::future::timeout()` 实现读超时，未引入 embassy-time（OE1~OE5 反优化教训）。

## 性能基线（Q20，2026-07-13）

Q20 使用同版 `q19c-m0-20260703` benchmark。QEMU 证明路径和输出形态，D1 证明 115200 bps 物理线速。

| 指标 | 值 | 对比 |
|------|-----|------|
| D1 TX drain-each | 11.14-11.38 KB/s | 96.7%-98.8% 线速 |
| D1 TX batch-drain | 11.35-11.42 KB/s | 98.5%-99.1% 线速 |
| D1 S20 1B P99 | 0.221 ms | 无 `line+10ms` tail |
| D1 S21 size>=15 P99 | 23.990-27.234 ms | 每组 1 次 tail |
| D1 TX ring buffer write | 1155388.15 KB/s | 内部队列能力，不代表线速 |
| D1 RX ring buffer read | 8303061.75 KB/s | 内部队列能力，不代表线速 |
| D1 S40 fallback | `slow_poll_exh=0`, `yield_exh=0` | fallback 未耗尽 |
| RX empty nonblocking | PASS | `open(O_NONBLOCK)` + `ioctl(FIONBIO)` |

用户态吞吐受 UART 物理线速限制。内核态 ring benchmark 说明驱动内部搬运能力远高于用户态线速表现。

## 约束

不修改 axplat / axhal / axtask 等外部 crate。所有 MMIO 走 `uart_16550` crate API，禁止裸写硬件地址。NS16550 stride 必须传 1。ISR 极简：只读 ISR / 禁中断 / wake / 返回，禁止数据搬运和加锁。跨层状态（如 FIONBIO）必须穷举所有入口。

## 文件索引

| 层 | 文件 | 说明 |
|----|------|------|
| 通用异步栈 | [driver.rs](../crates/uart_16550/src/async_/driver.rs) | AsyncUartDriver + TxCompletion |
| 通用异步栈 | [ring_buffer.rs](../crates/uart_16550/src/async_/ring_buffer.rs) | RingBufRx/Tx |
| 通用异步栈 | [isr.rs](../crates/uart_16550/src/async_/isr.rs) | ISR handler |
| 通用异步栈 | [device_ops.rs](../crates/uart_16550/src/async_/device_ops.rs) | AsyncUartReader/Writer |
| 通用异步栈 | [os/mod.rs](../crates/uart_16550/src/os/mod.rs) | 2 个 OS 抽象 trait |
| 平台初始化 | [uart_init.rs](../kernel/src/drivers/uart_init.rs) | QEMU NS16550 初始化 |
| D1 平台初始化 | [d1_uart.rs](../kernel/src/drivers/d1_uart.rs) | D1 stride-4 UART0 + D1 ISR |
| OS 适配 | [os_arceos.rs](../kernel/src/drivers/os_arceos.rs) | ArceOS 适配（2 trait） |
| 类型装配 | [ntty_async.rs](../kernel/src/drivers/ntty_async.rs) | AsyncTty 别名 |
| TTY 节点 | [tty/mod.rs](../kernel/src/pseudofs/dev/tty/mod.rs) | Tty + FIONBIO |
| ldisc | [ldisc.rs](../kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) | 行编辑 + 缓冲 |

## 术语

| 术语 | 含义 |
|------|------|
| ISR | 中断服务例程 |
| MMIO | 内存映射 I/O |
| AtomicWaker | embassy-sync 线程安全 waker 容器 |
| NAPI | 高吞吐中断合并机制 |
| SPSC | 单生产者单消费者 |
| ldisc | 行规程 |
| FIONBIO | ioctl 启用非阻塞 |
| O_NONBLOCK | open 标志启用非阻塞 |
| tcdrain / TCSBRK | POSIX 等待输出完毕 |
| THR | 发送保持寄存器 |
| LSR | 线状态寄存器 |
| TEMT | LSR bit 6（THR + 移位寄存器全空） |
| FCR | FIFO 控制寄存器 |
| IER | 中断使能寄存器 |
| ISR（寄存器）| 中断状态寄存器 |
| Stride | 寄存器地址间隔（NS16550 = 1） |
