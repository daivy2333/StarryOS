# StarryOS 异步串口子系統架构报告

> **分支**: `feat/uart-async-dev2` | **日期**: 2026-06-02 | **总代码**: ~500 行（7 文件）

---

## 1. 概述

### 1.1 目标

在 StarryOS（基于 ArceOS 的 RISC-V 宏内核）中实现**完整的异步串口栈**：Shell stdin/stdout 通过异步 ring buffer + 中断驱动的 copier 任务进行读写，替代原有的同步阻塞 Console。

### 1.2 核心约束

- **不修改外部 crate**：axplat、axhal、axtask 均保持原始状态。
- **kernel 层独立实现**：所有驱动代码位于 `kernel/src/drivers/`，7 个文件，总计 ~500 行。
- **Tty 泛型兼容**：利用 `Tty<R,W>` 泛型绑定，实现 `TtyRead` / `TtyWrite` trait 即可替换终端栈，无需修改伪终端框架。

### 1.3 历史：为何走到这一步

最初尝试「渐进式集成」方向（复用 Console，逐步替换），因 Console 与 AsyncUart 共享 UART 硬件导致 IRQ 风暴 + TX busy-loop 而失败。随后「完全剔除 Console」方向因 stride=4 导致的 LoadFault 受阻。最终采用当前方案：**kernel 层独立实现**，不修改任何外部 crate，从头控制 UART 初始化和中断分发。

---

## 2. 架构总览

```
                    ┌────────────────────────────────────┐
                    │           用户态 (Shell)            │
                    │   read(fd,buf)     write(fd,buf)   │
                    └────────┬───────────┬───────────────┘
                             │           │
                    ┌────────▼───────────▼───────────────┐
                    │         syscall / VFS / File       │
                    └────────┬───────────┬───────────────┘
                             │           │
                    ┌────────▼───────────▼───────────────┐
                    │      Tty<AsyncUartReader, W>       │
                    │          /dev/console              │
                    │   TtyRead          TtyWrite        │
                    │     │                 │            │
                    │     ▼                 ▼            │
                    │  ldisc (External)  ring buffer     │
                    │  tty-reader task      push         │
                    └────────┬───────────┬───────────────┘
                             │           │
    ┌────────────────────────▼───┐ ┌─────▼───────────────┐
    │     RX copier (task)       │ │   TX copier (task)  │
    │  poll_fn loop:             │ │  poll_fn loop:      │
    │    UART.receive_bytes()    │ │    buf.pop()        │
    │    ring_buf.push()         │ │    UART.send_bytes()│
    │    register RX_WAKER       │ │    register TX_WAKER│
    └────────┬───────────────────┘ └──────┬──────────────┘
             │                            │
    ┌────────▼────────────────────────────▼──────────────┐
    │                  ISR (IRQ 10)                       │
    │  uart_isr_handler:                                  │
    │    RX: disable_rx_intr → RX_WAKER.wake              │
    │    TX: disable_tx_intr → TX_WAKER.wake              │
    │                     → DRAIN_WAKER.wake               │
    └──────────────────────┬──────────────────────────────┘
                           │
                  ┌────────▼────────┐
                  │  NS16550 UART   │
                  │  MMIO 0x1000_0000│
                  │  stride = 1     │
                  └─────────────────┘
```

### 2.1 数据流

**RX 路径**（键盘输入 → Shell 读取）：

```
UART RX FIFO → ISR(RX_WAKER.wake) → RX copier → ring buffer
  → InputReader.poll() → ldisc processing → buf_tx
  → ldisc.read() → user read()
```

**TX 路径**（Shell 输出 → 串口发送）：

```
user write() → TtyWrite → ring buffer push → TX copier
  → UART THR → hardware transmission
  ── tcdrain: PollSet(drain ring) + DRAIN_WAKER(drain UART) → return
```

**内核日志**（ax_println!）：通过 polling TX 路径直接写 UART，与异步路径共存。

---

## 3. 模块详解

### 3.1 `uart_init.rs`（164 行）— UART 硬件初始化

**任务**：替代 axplat 的 UART 初始化，独占控制 NS16550。

| 功能 | 实现 |
|------|------|
| MMIO 映射 | `phys_to_virt(0x10000000)` + `axmm::iomap()` 保证页表权限 |
| 寄存器访问 | `uart_16550` crate (`new_mmio`, stride=1) |
| 全局实例 | `SpinNoIrq<Uart16550<MmioBackend>>`（lazy_static） |
| IER 缓存 | `AtomicU8` 缓存当前 IER 值，enable/disable 函数只需一次 `write_volatile` |
| NAPI 配置 | `NAPI_THRESHOLD=16`, `NAPI_BATCH_SIZE=64` |
| IRQ 计数 | `AtomicU64` 记录 ISR 触发次数，支持性能分析 |

**注意**：stride 必须为 1。NS16550 仅 8 字节寄存器空间（0x00–0x07），stride=4 会导致 LoadFault。

### 3.2 `isr.rs`（24 行）— 中断分发

**任务**：在 ISR 上下文中最小化工作，仅读 ISR 寄存器 + 禁用中断 + 唤醒对应 Waker。

| 唤醒器 | 触发条件 | 使用者 |
|--------|---------|--------|
| `RX_WAKER` | `ReceivedDataReady` / `ReceptionTimeout` | RX copier |
| `TX_WAKER` | `TransmitterHoldingRegisterEmpty` | TX copier |
| `DRAIN_WAKER` | `TransmitterHoldingRegisterEmpty` | tcdrain (TCSBRK) |

三个 `AtomicWaker`（embassy-sync 提供），ISR 安全（无锁唤醒）。

### 3.3 `ring_buffer.rs`（58 行）— 环形缓冲区

**任务**：提供 RX/TX 共 128 KB 的 `HeapRb<u8>` 缓冲区 + `PollSet` 唤醒机制。

| 结构 | 容量 | 角色 |
|------|------|------|
| `RingBufRx` | 64 KB | RX copier push，user read pop |
| `RingBufTx` | 64 KB | user write push，TX copier pop |

- `push()` 后调用 `poll.wake()` 通知等待者。
- `register_waker()` 在缓冲可读写时立即唤醒，否则注册到 PollSet。

### 3.4 `async_driver.rs`（90 行）— RX/TX Copier 任务

**任务**：两个独立 axtask 协程，负责硬件 FIFO 和 ring buffer 之间的数据搬运。

- **RX copier** (`uart-rx-copier`): ISR 唤醒 → 读 UART FIFO → push ring buffer → 连续 ≥16 次成功后切换轮询模式（NAPI）。
- **TX copier** (`uart-tx-copier`): 从 ring buffer pop 数据 → 批量 `send_bytes()` 到 UART THR → 部分发送时使能 TX 中断。

### 3.5 `device_ops.rs`（25 行）— Tty 接口适配

实现 `TtyRead` / `TtyWrite` trait，通过 `Tty<R,W>` 泛型绑定替换整个终端栈。

| 实现 | Trait | 行为 |
|------|-------|------|
| `AsyncUartReader` | `TtyRead` | `DRIVER.rx.lock().pop()` |
| `AsyncUartWriter` | `TtyWrite` | `DRIVER.tx.lock().push()` |

### 3.6 `ntty_async.rs`（31 行）— AsyncTty 装配

定义 `AsyncTty = Tty<AsyncUartReader, AsyncUartWriter>`，使用 `ProcessMode::External`。

```rust
process_mode: ProcessMode::External(Box::new(move |waker| {
    DRIVER.rx.lock().poll.register(&waker);
})),
```

External 模式自动创建 tty-reader 协程，通过 ring buffer 的 PollSet 精确唤醒（非 Manual 模式的 `wake_by_ref` 自旋）。

### 3.7 系统调用层改动

**TCSBRK / tcdrain**（`ctl.rs`）：

```rust
if cmd == 0x5409 { block_on(poll_fn(|cx| {
    // 1. ring buf 有数据 → 注册 tx.poll（copier pop 时唤醒）
    // 2. ring buf 空但 UART 在发 → 注册 DRAIN_WAKER（ISR 唤醒）
    // 3. ring buf 空 + TEMT → 返回
}))}
```

**FIONBIO 传播**（`ctl.rs` + `fd_ops.rs` + `tty/mod.rs`）：

非阻塞标志从 File 层穿透到 Tty/ldisc，三个入口全部覆盖：
`open(O_NONBLOCK)` / `fcntl(F_SETFL)` / `ioctl(FIONBIO)`。

**Tty struct 改动**（`tty/mod.rs`）：新增 `nonblocking: AtomicBool` 字段，`read_at()` 和 `ldisc.read()` 感知该标志。

---

## 4. 引入的内容

| 类别 | 内容 | 说明 |
|------|------|------|
| **驱动模块** | 7 文件 · ~500 行 | uart_init, isr, ring_buffer, async_driver, device_ops, ntty_async, benchmark |
| **Ring Buffer** | 128 KB (RX+TX) | `ringbuf::HeapRb` + `axpoll::PollSet` |
| **中断分发** | 3 × AtomicWaker | RX_WAKER, TX_WAKER, DRAIN_WAKER |
| **协程任务** | 2 后台任务 | RX copier + TX copier（axtask 调度） |
| **Tty 集成** | `AsyncTty = Tty<R,W>` | TtyRead/TtyWrite trait 实现 |
| **tcdrain** | TCSBRK ioctl | PollSet + DRAIN_WAKER 异步等待 |
| **非阻塞 I/O** | FIONBIO | open/fcntl/ioctl 三入口全传播 |
| **NAPI** | 中断合并 | ≥16 次连续成功 → 轮询模式，减少 90%+ IRQ |
| **IER 缓存** | AtomicU8 | RMW → 单次 write_volatile |
| **测试框架** | benchmark.c + benchmark.rs | 端到端吞吐量/延迟 + FIONBIO 验证 |

---

## 5. 清理的内容

| 类别 | 内容 | 替代方案 |
|------|------|----------|
| **Console 组件** | `ntty.rs` (Console struct) | `ntty_async.rs` (AsyncTty) |
| **Console Writer** | `ConsoleWriter` | `AsyncUartWriter` |
| **Console Driver** | `console_driver.rs` | `async_driver.rs` |
| **axplat UART init** | 外部 crate 初始化 | 本地 `uart_init.rs` |
| **register_irq_waker** | BTreeMap 多 waker 分发 | 3 × AtomicWaker 精确唤醒 |
| **Manual ProcessMode** | `wake_by_ref()` 自旋 | External + PollSet 注册 |
| **tcdrain 自旋** | wake_by_ref 协作自旋 | PollSet + DRAIN_WAKER |
| **独立设备节点** | `/dev/async_uart` | 直接替换 `/dev/console` |

---

## 6. 关键设计决策

### 6.1 External ProcessMode（O42）

**问题**：Manual 模式的 `register_rx_waker` 调用 `waker.wake_by_ref()`，每次注册立即唤醒调用者，无数据时产生高频 yield-re-schedule 循环（yield storm）。

**决策**：切换到 External 模式。ldisc 自动创建 tty-reader 协程，通过 ring buffer 的 PollSet 注册 waker。只在 RX copier 实际产生数据时才唤醒。

**代价**：多一个内核协程（与旧 Console 的 tty-reader 成本相同）。

### 6.2 DRAIN_WAKER（O45）

**问题**：tcdrain（TCSBRK）需要等待 ring buffer 清空 + UART FIFO 排空。原实现用 `wake_by_ref()` 协作自旋，每次 poll 失败立即重调度，产生不必要的任务切换。

**决策**：分两阶段等待：
1. Ring buffer 有数据 → 注册 `tx.poll`，copier pop 时唤醒
2. Ring buffer 空但 UART 未排空 → 注册 `DRAIN_WAKER`，TX ISR 唤醒
3. 双检查模式：check → register → double-check → park（防 ISR 竞争）

**效果**：64 字节路径任务切换从 9 次降至 ~6 次。

### 6.3 FIONBIO 三入口传播（O43）

**问题**：最初只在 `sys_ioctl(FIONBIO)` 转发到 Tty，但 `open(O_NONBLOCK)` 和 `fcntl(F_SETFL, O_NONBLOCK)` 只在 File 层设 flag，未传播到 Tty/ldisc。

**决策**：三个入口都加 `f.ioctl(FIONBIO, nb)` 转发。Tty struct 添加 `nonblocking: AtomicBool`，传播到 `read_at()` → `ldisc.read()`。

### 6.4 stride=1（Q0）

NS16550 仅 8 字节寄存器，RISC-V MMIO 标准 stride=4 会导致 LoadFault。强制 stride=1。

---

## 7. 演进里程碑

| 阶段 | 内容 | 关键产出 |
|------|------|---------|
| **Q0** | Spike | stride=1 · 寄存器读写 · ISR 执正 |
| **Q1** | 驱动骨架 | ring buffer + copier + AtomicWaker |
| **Q2** | VFS 集成 | DeviceOps + /dev/console |
| **Q3** | RX 接管 | Tty<AsyncUartReader, ConsoleWriter> → Shell stdin |
| **Q4** | TX 接管 | 全异步 RX+TX，Shell 双向异步 |
| **Q5** | 性能优化 | IER 缓存 · ISR 合并 · 批量 I/O · rx/tx 独立锁 |
| **Q5.1** | 优化续 | NAPI · 批量 API · TX interleave 修复 |
| **Q7** | 用户态修复 | O42（yield storm）· O43（FIONBIO）· O44（TCSBRK）· O45（DRAIN_WAKER） |
| **Q6** | 真板验证 | VisionFive2（等待硬件） |

---

## 8. 文件索引

| 文件 | 行数 | 功能 |
|------|------|------|
| `kernel/src/drivers/uart_init.rs` | 164 | UART 初始化 + IER 缓存 + NAPI 配置 |
| `kernel/src/drivers/async_driver.rs` | 90 | RX/TX copier 协程 |
| `kernel/src/drivers/ring_buffer.rs` | 58 | 128 KB 环形缓冲区 + PollSet |
| `kernel/src/drivers/ntty_async.rs` | 31 | AsyncTty 装配 (External ProcessMode) |
| `kernel/src/drivers/device_ops.rs` | 25 | TtyRead/TtyWrite trait 实现 |
| `kernel/src/drivers/isr.rs` | 24 | ISR + 3×AtomicWaker |
| `kernel/src/drivers/mod.rs` | 20 | 模块声明 |
| `kernel/src/syscall/fs/ctl.rs` | +25 | TCSBRK (tcdrain) + FIONBIO 转发 |
| `kernel/src/syscall/fs/fd_ops.rs` | +6 | O_NONBLOCK open/fcntl 传播 |
| `kernel/src/pseudofs/dev/tty/mod.rs` | +8 | Tty.nonblocking + read_at 感知 |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | +1 | read() 接受 nonblocking 参数 |
