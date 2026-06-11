# StarryOS 异步串口子系统架构报告

> **分支**: asyncuart-dev | **日期**: 2026-06-11（Q0~Q11 完成）
> **仓库**: [daivy2333/StarryOS](https://github.com/daivy2333/StarryOS)

---

## 1. 概述

StarryOS 异步串口子系统是一个 **kernel 层独立实现**的完整异步串口栈，基于 RISC-V 宏内核（ArceOS 架构），将 Shell stdin/stdout 接入异步 ring buffer + 中断驱动的 copier 任务，替代原有的同步阻塞 Console。

**核心约束**：不修改任何外部 crate（axplat/axhal/axtask），所有驱动代码位于 `kernel/src/drivers/`。

**数据流概览**：

```
用户态 read()
  → VFS → File::read → block_on(poll_io(…))
    → Tty::read_at → ldisc::read → buf_rx.pop_slice()
      ↑ ldisc ring buffer (256B StaticRb)
        ↑ tty-reader task (InputReader::poll)
          ↑ AsyncUartReader::read → DRIVER.rx.pop()
            ↑ RingBufRx (64KB HeapRb)
              ↑ RX copier ← ISR(RX_WAKER.wake) ← UART RX FIFO

用户态 write()
  → VFS → File::write
    → Tty::write_at → AsyncUartWriter::write → DRIVER.tx.push()
      ↓ RingBufTx (64KB HeapRb)
        ↓ TX copier → ISR(TX_WAKER.wake) → UART TX THR
          └ tcdrain: DRAIN_WAKER 条件等待
```

**数据拷贝次数**：用户态读路径共 5 次拷贝：

| # | 来源 | 目标 | 位置 |
|---|------|------|------|
| C1 | UART RX FIFO (16B) | copier read_buf (1024B) | [async_driver.rs:48](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L48) |
| C2 | copier read_buf | RingBufRx (64KB) | [async_driver.rs:50](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L50) |
| C3 | RingBufRx | InputReader::read_buf (256B) | [ldisc.rs:83](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L83) |
| C4 | InputReader::read_buf | ldisc StaticRb (256B) | [ldisc.rs:90](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L90) |
| C5 | ldisc StaticRb | 用户 buf | [ldisc.rs:383](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L383) |

> C3/C4 在同一个 `InputReader::poll()` 调用中连续发生。Q10 已将 C4 路径从逐字节 `try_push` 优化为批量 `push_slice`。

---

## 2. 驱动层

驱动层由 4 个文件组成，负责 UART 硬件初始化、中断分发、环形缓冲区管理和数据搬运。源码位于 `kernel/src/drivers/`。

### 2.1 UART 硬件初始化

**文件**: [uart_init.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/uart_init.rs)（182 行）

使用 `uart_16550` crate（本地 path 依赖）替代 axplat 的 UART 初始化，独占控制 NS16550。

| 配置项 | 值 | 说明 |
|--------|-----|------|
| MMIO 基址 | `0x10000000` | QEMU RISC-V virt 平台 |
| 寄存器 stride | **1**（强制） | NS16550 仅 8 字节寄存器，stride=4 → LoadFault |
| 波特率 | 115200 bps | 标准串口速率 |
| FIFO | 使能，触发阈值 14 字节 | FCR 配置 |
| 中断 | RX Data Ready + TX THR Empty | IER 配置 |
| NAPI 阈值 | 16 次 | 连续成功读取后切换轮询模式 |
| NAPI 批量 | 64 字节 | 轮询模式下的批次大小 |

**全局实例**：`SpinNoIrq<Uart16550<MmioBackend>>`（[lazy_static](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/uart_init.rs#L46-L58)），通过 `uart_instance()` 获取。

**IER 管理**（Q8 优化）：使用 `CACHED_IER: AtomicU8` 缓存当前 IER 值，通过 `uart_16550` 的 `set_ier()` API 写入（[Q8.3a 添加的方法](https://github.com/daivy2333/uart_16550/blob/dev/optimize/src/lib.rs)），消除了原先绕过 crate 的裸 `write_volatile`。

```rust
// uart_init.rs — IER 写入路径（Q8 规范化后）
fn write_ier(value: u8) {
    CACHED_IER.store(value, Ordering::Relaxed);
    uart_instance().lock().set_ier(IER::from_bits_truncate(value));
}
pub fn enable_rx_intr()  { write_ier(CACHED_IER.load(Relaxed) | IER::DATA_READY.bits()); }
pub fn disable_rx_intr() { write_ier(CACHED_IER.load(Relaxed) & !IER::DATA_READY.bits()); }
```

**ISR 无锁读取**（Q8 优化，[uart_init.rs:75](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/uart_init.rs#L75)）：
```rust
pub fn read_isr_unlocked() -> ISR {
    unsafe { ISR::from_bits_retain(ptr.add(offsets::ISR as usize).read_volatile()) }
}
```
单 ISR 上下文，无需 SpinNoIrq 保护。

### 2.2 中断分发

**文件**: [isr.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/isr.rs)（27 行）

ISR handler 遵循极简原则：读 ISR → 禁中断 → wake → 返回。**不搬运数据、不获取锁**。

```rust
pub static RX_WAKER:    AtomicWaker = AtomicWaker::new();
pub static TX_WAKER:    AtomicWaker = AtomicWaker::new();
pub static DRAIN_WAKER: AtomicWaker = AtomicWaker::new();
pub static TCDRAIN_ACTIVE: AtomicBool = AtomicBool::new(false); // Q8.5

pub fn uart_isr_handler(_irq: usize) {
    let isr = read_isr_unlocked();                    // Q8.2: 无锁读取
    match isr.interrupt_type() {
        Some(ReceivedDataReady | ReceptionTimeout) => {
            disable_rx_intr(); RX_WAKER.wake();
        }
        Some(TransmitterHoldingRegisterEmpty) => {
            disable_tx_intr(); TX_WAKER.wake();
            if TCDRAIN_ACTIVE.load(Acquire) {         // Q8.5: 条件唤醒
                DRAIN_WAKER.wake();
            }
        }
        _ => {}
    }
}
```

三个 `AtomicWaker` 均来自 `embassy_sync::waitqueue`（lock-free），ISR 安全。Q8 将原有 `SpinNoIrq` 锁保护替换为无锁 MMIO 访问，延迟降低 ~200ns。

### 2.3 环形缓冲区

**文件**: [ring_buffer.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ring_buffer.rs)（58 行）

提供 128 KB 总容量的双缓冲（RX/TX 各 64 KB `HeapRb<u8>`），配合 `AtomicWaker`（Q8 前为 `PollSet`）实现异步唤醒。

| 结构 | 容量 | 操作 | 唤醒 |
|------|------|------|------|
| `RingBufRx` | 64 KB | RX copier `push()` → user `pop()` | `push()` 成功时 `poll.wake()` |
| `RingBufTx` | 64 KB | user `push()` → TX copier `pop()` | `pop()` 成功时 `poll.wake()` |

```rust
// ring_buffer.rs — RX 缓冲区核心逻辑
pub fn push(&mut self, data: &[u8]) -> usize {
    let n = self.buf.push_slice(data);
    if n > 0 { self.poll.wake(); }  // copier 生产数据 → 唤醒 tty-reader
    n
}
pub fn register_waker(&self, cx: &mut Context<'_>) {
    if !self.buf.is_empty() { cx.waker().wake_by_ref(); }
    else { self.poll.register(cx.waker()); }
}
```

### 2.4 Copier 任务

**文件**: [async_driver.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs)（101 行）

两个独立的 axtask 协程，负责硬件 FIFO ↔ ring buffer 的数据搬运。两者均以 `loop { poll_fn(…) }.await` 模式运行。

**RX copier** (`uart-rx-copier`) — ISR 唤醒后从 UART FIFO 读数据推入 ring buffer：

```
poll_fn 每次迭代:
  1. uart.receive_bytes(&mut read_buf[..batch])   // 批量读取 FIFO
  2. self.rx.lock().push(&read_buf[..total])      // 推入 ring buffer
  3. NAPI 逻辑：
     - consecutive < 16: 中断模式，batch=1024，total>0 时 consecutive+1
     - consecutive ≥ 16: 轮询模式，batch=64，total==0 时退出并 enable_rx_intr()
  4. RX_WAKER.register(cx.waker())                // 等待下次 ISR
```

**Q8.1 NAPI 退出修复**（[async_driver.rs:51-57](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L51-L57)）：
```rust
if consecutive >= NAPI_THRESHOLD {
    if total > 0 { consecutive += 1; }
    else { consecutive = 0; enable_rx_intr(); }  // 零字节 → 退出轮询
}
```

**TX copier** (`uart-tx-copier`) — 从 ring buffer 取数据批量写入 UART THR，部分发送时使能 TX 中断：

```
poll_fn 每次迭代:
  1. self.tx.lock().pop(&mut write_buf)           // 从 ring buffer 取出
  2. uart.send_bytes(&write_buf[cursor..pending])  // 批量写 THR
  3. 若 cursor < pending: enable_tx_intr()         // 等待 ISR 继续发送
  4. TX_WAKER.register(cx.waker())                 // 等待下次 ISR
```

**Q8.4 waker 去重简化**（[async_driver.rs:62-66](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs#L62-L66)）：
```rust
let w = cx.waker().clone();
let old = last_waker.replace(Some(w.clone()));
if old.as_ref().map_or(true, |old_w| !old_w.will_wake(&w)) {
    RX_WAKER.register(cx.waker());  // 仅在 waker 变化时 register
}
```

---

## 3. TTY 集成层

TTY 集成层负责将驱动层的 ring buffer 接入 Linux 兼容的伪终端框架（`Tty<R,W>` + ldisc），使 Shell 可以透明地使用异步串口。

### 3.1 TtyRead / TtyWrite 适配

**文件**: [device_ops.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/device_ops.rs)（25 行）  
**文件**: [ntty_async.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ntty_async.rs)（31 行）

通过实现 `TtyRead` / `TtyWrite` trait，泛型 `Tty<R,W>` 无需修改即可绑定异步串口：

```rust
// device_ops.rs — 零拷贝适配层
impl TtyRead for AsyncUartReader {
    fn read(&mut self, buf: &mut [u8]) -> usize {
        DRIVER.rx.lock().pop(buf)   // 直接从 ring buffer pop
    }
}
impl TtyWrite for AsyncUartWriter {
    fn write(&self, buf: &[u8]) {
        DRIVER.tx.lock().push(buf); // 直接 push 到 ring buffer
    }
}
```

**ProcessMode::External**（[ntty_async.rs:20](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ntty_async.rs#L20)）：
```rust
ProcessMode::External(Box::new(move |waker| {
    DRIVER.rx.lock().poll.register(&waker);  // 精确唤醒，无 wake_by_ref 自旋
}))
```
External 模式自动创建 tty-reader 协程，通过 ring buffer 的 waker 精确唤醒。消除 Q7 前 Manual 模式的 yield storm。

### 3.2 Line Discipline（ldisc）

**文件**: [ldisc.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs)（413 行）

`LineDiscipline<R,W>` 负责终端行编辑（canonical mode、echo、信号处理）和缓冲管理。

**核心数据结构**（Q10 优化后）：

```rust
pub struct LineDiscipline<R, W> {
    terminal: Arc<Terminal>,
    buf_rx: UnsafeCell<CachingCons<ReadBuf>>,  // Q10: UnsafeCell 实现 &self 访问
    poll_tx: Arc<PollSet>,         // 通知 tty-reader 有空间
    clear_line_buf: Arc<AtomicBool>,
    processor: Processor<R, W>,    // InputReader / SimpleReader
}
```

**BUF_SIZE = 256**（Q10.2，[ldisc.rs:24](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L24)），3.2× 扩容。影响 `InputReader::read_buf` 和 `StaticRb<u8, 256>` 栈大小。

**InputReader::poll()**（[ldisc.rs:78-155](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L78-L155)）— 从 `AsyncUartReader` 读取数据，逐字节处理行编辑：

```
poll() 每次调用:
  1. reader.read(&mut self.read_buf)      // C3: 从 RingBufRx 读取
  2. 逐字节循环处理:
     - \r → IGNCR/ICRNL 转换
     - canonical mode: VEOF/VERASE/VKILL 处理
     - echo: output_char() 回显
     - 非 canonical: try_push 到 buf_tx  // C4
     - canonical: 累积到 line_buf
  3. 若有完整行，push_slice 到 buf_tx    // C4 (批量)
```

**SimpleReader::poll()**（[ldisc.rs:192-210](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L192-L210)）— Q10.1 优化为批量 `push_slice`：

```rust
pub fn poll(&mut self) {
    let read = self.reader.read(&mut self.read_buf);
    let data = &self.read_buf[..read];
    let mut start = 0;
    for (i, &ch) in data.iter().enumerate() {
        if ch == b'\n' {
            if i > start { self.buf_tx.push_slice(&data[start..i]); }
            self.buf_tx.push_slice(b"\r\n");  // \n → \r\n
            start = i + 1;
        }
    }
    if start < read { self.buf_tx.push_slice(&data[start..read]); }
}
```

**LineDiscipline::read()**（Q10.3 `&self` 化，[ldisc.rs:347-389](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L347-L389)）— 用户态读入口：

```rust
pub fn read(&self, buf: &mut [u8], nonblocking: bool) -> AxResult<usize> {
    // Q10: &self 访问 buf_rx（通过 UnsafeCell 安全访问器）
    let total = self.buf_rx().pop_slice(&mut buf[total_read..]);
    // block_on(poll_io(…)) 等待数据或超时
}
```

**Q9 VTIME 超时**（[ldisc.rs:364-385](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L364-L385)）— VTIME>0 时使用 `axtask::future::timeout()` 包装读等待：
```rust
block_on(axtask::future::timeout(Some(dur), poll_io(&pollable, IN, nonblocking, || {
    total_read += self.buf_rx().pop_slice(&mut buf[total_read..]);
    if total_read > 0 { Ok(total_read) } else { Err(WouldBlock) }
})))
```

### 3.3 Tty 设备节点

**文件**: [tty/mod.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/mod.rs)（234 行）

`Tty<R,W>` 是 `/dev/console` 的 DeviceOps 实现，负责 job control 检查和非阻塞标志传播。

```rust
pub struct Tty<R, W> {
    this: Weak<Self>,
    terminal: Arc<Terminal>,
    ldisc: Mutex<LineDiscipline<R, W>>,
    writer: W,
    is_ptm: bool,
    nonblocking: AtomicBool,  // Q7 O43: FIONBIO 传播
}
```

**read_at()**（[tty/mod.rs:88-102](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/mod.rs#L88-L102)）：
```rust
fn read_at(&self, buf: &mut [u8], _offset: u64) -> AxResult<usize> {
    let nb = self.nonblocking.load(Acquire);
    block_on(poll_io(&self.terminal.job_control, IN, nb, || {
        if self.is_ptm || self.terminal.job_control.current_in_foreground() {
            self.ldisc.lock().read(buf, nb)
        } else { Err(WouldBlock) }
    }))
}
```

**write_at()**（[tty/mod.rs:104-107](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/mod.rs#L104-L107)）— 直接 push ring buffer，天然非阻塞：
```rust
fn write_at(&self, buf: &[u8], _offset: u64) -> AxResult<usize> {
    self.writer.write(buf);  // → AsyncUartWriter::write → DRIVER.tx.push()
    Ok(buf.len())
}
```

**FIONBIO 传播**：非阻塞标志通过三个入口覆盖 — `open(O_NONBLOCK)`（[fd_ops.rs:106](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/fd_ops.rs)）、`fcntl(F_SETFL)`（[fd_ops.rs:254](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/fd_ops.rs)）、`ioctl(FIONBIO)`（[ctl.rs:31](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs#L31)）。Q11 消除了 tty/mod.rs 中 3 处 `.unwrap()` panic 点。

**tcdrain（TCSBRK）**（[ctl.rs:43-72](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs#L43-L72)）— 三段式异步等待：

```
1. ring buf 有数据 → 注册 tx.poll（copier pop 时唤醒）
2. ring buf 空但 UART 未排空 → 注册 DRAIN_WAKER（ISR TX 中断时唤醒）
3. ring buf 空 + LSR.TRANSMITTER_EMPTY → 返回
```

Q8.5 添加了 `TCDRAIN_ACTIVE: AtomicBool` 标志，仅在 tcdrain 活跃时唤醒 `DRAIN_WAKER`。

---

## 4. O46 AtomicWaker 迁移

Q8 将 pipe / signalfd / pidfd / event 共 **8 个 PollSet 实例**替换为 `embassy_sync::waitqueue::AtomicWaker`（lock-free 单槽）。

**迁移矩阵**：

| 文件 | PollSet 数 | 唤醒源 | 风险 | 源码 |
|------|-----------|--------|------|------|
| [signalfd.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/signalfd.rs) | 1 | update_mask + read re-wake | 🟢 低 | 1:1 替换 |
| [event.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/event.rs) | 2 | 交叉唤醒（read↔wakeTX, write↔wakeRX） | 🟡 中 | 独立 AtomicWaker |
| [pipe.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/pipe.rs) | 3 | 交叉唤醒 + Drop 唤醒 | 🟡 中 | 3 个独立 AtomicWaker |
| [pidfd.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/pidfd.rs) | 1 | task/ops.rs exit 路径（Arc 共享） | 🔴 高 | 跨文件重构 |
| [task/mod.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/task/mod.rs) | 3 | Thread/ProcessData 构造 | 🔴 高 | 类型变更 |

**pidfd 特殊处理**：`exit_event` 是 `Arc<PollSet>` 共享于 `Thread` / `ProcessData` 之间。Q8 将其改为 `Arc<AtomicWaker>`，`AtomicWaker` 不实现 `Default` 故使用 `Arc::new(AtomicWaker::new())`。async 模型下始终单 waiter，单槽足够。

**效果**：唤醒延迟 ~200ns（PollSet spinlock）→ ~50ns（AtomicWaker lock-free），8 个唤醒点合计节省 ~1.2µs/lock-cycle。

---

## 5. 关键设计决策

### Q8 — NAPI 退出修复
NAPI 模式（≥16 次连续成功）下 `consecutive` 只增不减导致永不退出轮询。Q8.1 添加零字节重置 + `enable_rx_intr()` 恢复中断驱动。

### Q8 — ISR 无锁化
原 ISR 获取 `SpinNoIrq` 锁调用 `uart.isr()`，违反 ISR 极简原则。Q8.2 实现 `read_isr_unlocked()` 直接 MMIO 读 ISR。

### Q8 — IER 规范 + uart_16550 API
原 `write_ier()` 用裸 `write_volatile` 绕过 crate。Q8.3 向 uart_16550 添加 `set_ier()` 方法，规范化 MMIO 访问。

### Q9 — VTIME 超时（无需 embassy-time）
探索发现 axtask 已有完整 timeout 基础设施（`timeout()` + `select_biased!` + BTreeMap 计时器轮），Q9 直接复用替换 `todo!()`。

### Q10 — 数据路径优化
- BUF_SIZE 80→256：3.2× 扩容 ldisc 环缓冲，提升突发吸收能力
- SimpleReader push_slice：逐字节 `try_push` → 批量写入，减少函数调用
- read(&self)：UnsafeCell 包装 `buf_rx`，消除 ldisc Mutex 的语义依赖

### Q11 — 内核通用质量
- tty unwrap 消除：3 处 panic 点改为 AxError 传播
- mm/access 批量页验证：二进制搜索最大有效范围，减少 aspace 锁获取
- sendfile 栈缓冲区：`vec![0;4096]` → `[0u8;4096]`
- close_range UNSHARE 优化：范围迭代替代全表 clone
- ws_col 110→80：修复 QEMU 控制台显示换行错位

---

## 6. 性能摘要

| 指标 | Q8 基线 | Q11 最新 | 提升 |
|------|---------|----------|------|
| Ring Buffer TX | 214,961 KB/s | 196,850 KB/s | — |
| Ring Buffer RX | 588,776 KB/s | 393,362 KB/s | — |
| 1B 平均延迟 | 144.7 µs | 140.7 µs | ↓2.8% |
| 1B P50 | 139.5 µs | 129.2 µs | ↓7.4% |
| 唤醒延迟 (8点) | ~200ns/次 | ~50ns/次 | ↓75% |
| 空闲 CPU | 0%（External 模式） | 0%（NAPI 退出修复后） | ✅ |

> QEMU 不仿真串口线延迟，真板 VisionFive2 @ 115200 bps 收敛至 ~11.5 KB/s。

---

## 7. 演进历史

| 阶段 | 日期 | 内容 |
|------|------|------|
| Q0~Q4 | 05-31 | 驱动骨架、VFS 集成、全异步 RX+TX |
| Q5 | 05-31 | IER 缓存、ISR 合并、NAPI、rx/tx 独立锁 |
| Q7 | 06-01 | yield storm 修复、FIONBIO 传播、tcdrain 异步化 |
| Q8 | 06-11 | NAPI 退出、ISR 无锁、IER 规范化、O46 AtomicWaker (8处) |
| Q9 | 06-11 | VTIME 读超时 |
| Q10 | 06-11 | BUF_SIZE 256、push_slice、read(&self) |
| Q11 | 06-11 | tty unwrap、mm/access、sendfile、close_range、ws_col |
| Q6 | 待定 | VisionFive2 真板验证 |

---

## 8. 文件索引

| 文件 | 功能 | 行数 |
|------|------|------|
| [kernel/src/drivers/uart_init.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/uart_init.rs) | UART 初始化 + IER 管理 + NAPI 配置 | 182 |
| [kernel/src/drivers/async_driver.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/async_driver.rs) | RX/TX copier 协程 | 101 |
| [kernel/src/drivers/ring_buffer.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ring_buffer.rs) | 128 KB 环形缓冲区 | 58 |
| [kernel/src/drivers/isr.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/isr.rs) | ISR + 3×AtomicWaker | 27 |
| [kernel/src/drivers/device_ops.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/device_ops.rs) | TtyRead/TtyWrite trait 实现 | 25 |
| [kernel/src/drivers/ntty_async.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/drivers/ntty_async.rs) | AsyncTty 装配 (External ProcessMode) | 31 |
| [kernel/src/pseudofs/dev/tty/terminal/ldisc.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) | ldisc 行编辑 + 缓冲（BUF=256, &self） | 413 |
| [kernel/src/pseudofs/dev/tty/mod.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/pseudofs/dev/tty/mod.rs) | Tty 设备节点 + FIONBIO + tcdrain | 234 |
| [kernel/src/file/pipe.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/pipe.rs) | Pipe + 3×AtomicWaker | 236 |
| [kernel/src/file/signalfd.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/signalfd.rs) | Signalfd + AtomicWaker | 182 |
| [kernel/src/file/event.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/event.rs) | EventFd + 2×AtomicWaker | 126 |
| [kernel/src/file/pidfd.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/file/pidfd.rs) | PidFd + Arc\<AtomicWaker\> | 91 |
| [kernel/src/task/mod.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/task/mod.rs) | Thread/ProcessData exit_event 类型 | — |
| [kernel/src/syscall/fs/ctl.rs](https://github.com/daivy2333/StarryOS/blob/asyncuart-dev/kernel/src/syscall/fs/ctl.rs) | TCSBRK + TCDRAIN_ACTIVE + FIONBIO | — |
| [uart_16550/src/lib.rs](https://github.com/daivy2333/uart_16550/blob/dev/optimize/src/lib.rs) | 16550 驱动 + set_ier() | — |