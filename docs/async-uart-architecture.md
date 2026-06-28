# StarryOS 异步串口架构（Q15 当前状态）

> 分支：`feat/uart-16550-async`  
> 状态：Q15 M0~M4 增量重融合 + Manual QA 验证通过 ✅（2026-06-25）  
> 下一站：Q6 真板验证（⏳ 等待硬件）  
> 仓库：[StarryOS](https://github.com/daivy2333/StarryOS) · [uart_16550](https://github.com/daivy2333/uart_16550)  
> 关联：`benchmark-report-async.md`（性能）· `manual-qa-report.md`（QA）

## 一句话

kernel 层独立异步串口栈。ISR 极简，copier 搬运数据，ring buffer 解耦硬件与用户。

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

读路径 5 次拷贝定位见 `ldisc.rs`。

## 关键抽象

| 抽象 | 位置 | 作用 |
|------|------|------|
| `OsRuntime` trait | [os_arceos.rs:17-40](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/os_arceos.rs#L17-L40) | 任务生成 + 同步等待 |
| `OsWakerSet` trait | [os_arceos.rs:45-62](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/os_arceos.rs#L45-L62) | 多 waker 集合 |
| `UartPort` trait | [uart_init.rs:81-112](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/uart_init.rs#L81-L112) | IRQ-safe 寄存器访问 |
| `AsyncUartDriver<R, W, P>` | [driver.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs) | 3 泛型驱动主类型 |
| `TtyRead` / `TtyWrite` trait | [ldisc.rs:58](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L58) | 通用 TTY 抽象 |

OS 抽象数量：Q13 提取时是 5 个 trait。Q13-cleanup（ADR-036）删除 3 个（`OsIrq` / `OsMmio` / `OsSpinNoIrq`），剩 2 个。IRQ 注册、MMIO 映射、锁获取由集成层在 driver 外部处理。

## 驱动层

**初始化**：[`uart_init.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/uart_init.rs) 持有 `SpinNoIrq<Uart16550<MmioBackend>>` 单例。MMIO 基址 `0x10000000`，stride 必须传 1（NS16550 仅 8 字节寄存器，stride=4 → LoadFault，见 L122 教训）。NAPI 阈值 16、FIFO 触发 14 字节。

**`UartPort`：Q15-M4 IER 单 owner**：[`update_ier(set, clear)`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/uart_init.rs#L105-L111) 是唯一入口。本地 `ier_cache` 原子更新 + 硬件 `set_ier()` 同步：

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

**ISR 极简**：[`uart_isr_wrapper`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/uart_init.rs#L171-L178) 桥接 axhal IRQ hook → `uart_16550::async_::isr::uart_isr_handler`：

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

ISR 仅做：读 ISR → 禁对应中断 → `AtomicWaker::wake()` → 返回。禁止数据搬运、禁止加锁。

**Copier 任务**：两个独立协程（`uart-rx-copier` / `uart-tx-copier`）搬运 FIFO ↔ ring buffer，循环 `poll_fn(...).await`。

**RX copier**（[`driver.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs)）：`receive_bytes` 批量读 FIFO → `RingBufRx::push` → NAPI 阈值检查 → `RX_WAKER.register` 等 ISR。连续成功 ≥16 次切轮询模式（batch=64），零字节退出轮询（Q8.1 修复）。

**TX copier**（[`driver.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs)）：`RingBufTx::pop_batch` 取数据 → `send_bytes` 批量写 THR。Q15-M1 加 `TX_FAST_RETRY_LIMIT=32` 有界 fast retry，消除 16B refill 的 10ms tick 台阶。Q15-M2 引入 `TxCompletion` API（`driver.tx_completion()`）作为统一 drain 检查点。

**Ring Buffer**（[`ring_buffer.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/ring_buffer.rs)）：

| 缓冲 | 容量 | 同步 | 生产者 | 消费者 |
|------|------|------|--------|--------|
| `RingBufRx` | 64 KiB | `SpinNoIrq` | RX copier | tty-reader |
| `RingBufTx` | 64 KiB | `ArceOsRawMutex` (Q15-M0) | 用户态 + 内核日志 | TX copier |

Q15-M0 引入 `ArceOsRawMutex`（基于 `SpinNoIrq` 的 RawMutex）保护 TX writer，支持 `Clone-safe AsyncUartWriter`（Shell stdout + `ax_println!` 多生产者场景）。

## TTY 集成层

**`Tty<R, W>` 泛型绑定**：[`tty/mod.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/mod.rs) 是 `/dev/console` 的 DeviceOps 实现。`R: TtyRead` / `W: TtyWrite` 是泛型参数。`AsyncUartReader` / `AsyncUartWriter` 实现这两个 trait，`Tty<AsyncUartReader, AsyncUartWriter>` 直接绑定异步串口（[`ntty_async.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/ntty_async.rs#L13)）。

Q15-M3 将 `TtyWrite::write` 改为返回 `usize`（ADR-038）：实际接受字节数穿透到 VFS，避免 silent data loss。

**FIONBIO 三入口**：非阻塞状态必须覆盖三个入口，缺一不可（见 L140 教训）：

| 入口 | 文件 |
|------|------|
| `open(O_NONBLOCK)` | [`fd_ops.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/syscall/fs/fd_ops.rs) |
| `fcntl(F_SETFL)` | [`fd_ops.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/syscall/fs/fd_ops.rs) |
| `ioctl(FIONBIO)` | [`ctl.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/syscall/fs/ctl.rs) |

`Tty` 持有 `nonblocking: AtomicBool`。三个入口都调用同一 setter。

**tcdrain（TCSBRK）** 三段式异步等待（Q15-M2 增强）：

```
1. ring buf 有数据 → 注册 tx.poll（copier pop 时唤醒）
2. ring buf 空 + UART 未排空 → 注册 DRAIN_WAKER（ISR TX 中断时唤醒）
3. ring buf 空 + LSR.TRANSMITTER_EMPTY → 返回
```

Q15-M2 引入 `TxCompletion` API 作为统一 drain 检查点，消除 tcdrain 直接 MMIO 的分层违规。

**VTIME 超时**：Q9 复用 `axtask::future::timeout()` 实现读超时，未引入 embassy-time（OE1~OE5 反优化教训）。

## 性能基线（Q15 Manual QA，2026-06-25）

QEMU `qemu-riscv64-virt`，无 LTO（per ADR-034）：

| 指标 | 值 | 对比 |
|------|-----|------|
| 1B e2e 延迟 avg | 134 µs | Q13.1: 129.5 µs（noise 范围） |
| 1B e2e P50 | 118.5 µs | Q13.1: 125.5 µs |
| 64B TX 吞吐 | 170 KB/s | M4: 184 KB/s（无 backpressure 退化） |
| Ring Buffer TX | 456 MB/s | Q13+LTO: 652 MB/s |
| Ring Buffer RX | 1,148 MB/s | Q13+LTO: 898 MB/s（↑27.9%） |
| 非阻塞三入口 | ✅ | FIONBIO 行为正确 |

e2e 瓶颈在调度，不在内核态吞吐。QEMU 不仿真串口线延迟，真板 ~11.5 KB/s。

## 关键约束

不修改 axplat / axhal / axtask 等外部 crate。所有 MMIO 走 `uart_16550` crate API，禁止裸写硬件地址。NS16550 stride 必须传 1。ISR 极简：只读 ISR / 禁中断 / wake / 返回，禁止数据搬运和加锁。跨层状态（如 FIONBIO）必须穷举所有入口。

## 关键文件

| 层 | 文件 | 说明 |
|----|------|------|
| 通用异步栈 | [driver.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/driver.rs) | AsyncUartDriver + TxCompletion |
| 通用异步栈 | [ring_buffer.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/ring_buffer.rs) | RingBufRx/Tx |
| 通用异步栈 | [isr.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/isr.rs) | ISR handler |
| 通用异步栈 | [device_ops.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/device_ops.rs) | AsyncUartReader/Writer |
| 通用异步栈 | [os/mod.rs](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/os/mod.rs) | 2 个 OS 抽象 trait |
| 平台初始化 | [uart_init.rs](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/uart_init.rs) | Uart16550 单例 + ArceOsUartPort |
| OS 适配 | [os_arceos.rs](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/os_arceos.rs) | ArceOS 适配（2 trait） |
| 类型装配 | [ntty_async.rs](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/drivers/ntty_async.rs) | AsyncTty 别名 |
| TTY 节点 | [tty/mod.rs](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/mod.rs) | Tty + FIONBIO |
| ldisc | [ldisc.rs](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) | 行编辑 + 缓冲 |

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