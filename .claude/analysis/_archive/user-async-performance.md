# 用户态异步串口性能分析

> Part of StarryOS codebase analysis (branch: asyncuart-dev) | Generated 2026-06-11
> Merged from: `docs/analysis/user-async-perf-analysis.md`, `docs/analysis/nonblocking-mode-analysis.md`
> See also: `openspec/specs/optimization/spec.md`

---

## §1 问题陈述

用户态（Shell/benchmark）下，Async UART 的吞吐量和延迟表现**不优于甚至差于**旧的阻塞 Console UART。异步引入的 ring buffer、copier 任务、ISR 分发等优化，在用户态的性能收益被架构开销抵消。

### 理论预期 vs 实际

| 指标 | 理论预期（Async 应更优） | 实际观察 |
|------|------------------------|---------|
| TX 吞吐量 | 更高（write 立即返回，pipeline） | 相近（受 115200 波特率限制） |
| TX 延迟 | 更低（write 不阻塞） | write() 延迟低，但端到端差异不大 |
| RX 吞吐量 | 更高（ring buffer + copier） | Kernel 内快（588 MB/s），用户态被 TTY 层瓶颈 |
| CPU 占用 | 更低（不 busy-wait） | **更高**（多任务切换 + yield storm） |
| 响应延迟 | 更低（中断驱动） | 相近（TTY ldisc 处理成为瓶颈） |

**核心结论**：Async UART 在原始吞吐量上不可能超过阻塞 Console（115200 bps 线速上限）。异步的真实优势在：(1) 写操作不阻塞调用者，(2) NAPI 中断合并减少 ISR 风暴，(3) 为 DMA/多队列等高性能场景提供架构基础。

---

## §2 数据路径分析 — 完整 read/write 追踪

### TX 路径（写）

```
User write(fd, buf, N)
  → sys_write (syscall 边界)
    → File::write()
      → block_on(poll_io(self, OUT, nonblocking, || inner.write()))
        → inner.write()  (VFS → Device → Tty)
          → Tty::write_at()
            → AsyncUartWriter::write()
              → DRIVER.tx.lock().push(buf)    ← 仅 push ring buffer
              → 返回 (不等待硬件)
        ← poll_io 返回 Ok(N)
  ← sys_write 返回 N
          ↓ (后台)
          TX copier 任务
            → DRIVER.tx.lock().pop()         ← pop ring buffer
            → uart_instance().lock()
            → send_bytes() → 写 UART THR
            → 部分发送 → 使能 TX 中断
            → 等待 ISR (AtomicWaker::wait)
```

**关键发现**：用户态 write() 仅 push ring buffer（~1 µs），**无法反映真实串口吞吐量**。

### RX 路径（读）

```
User read(fd, buf, N)
  → sys_read → File::read()
    → block_on(poll_io(self, IN, nonblocking, || inner.read()))
      LAYER 1: Syscall/File 层等待
      
      → inner.read() → Device::read_at() → Tty::read_at()
        → block_on(poll_io(&JobControl, IN, false, || {
            ldisc.lock().read(buf)
          }))
          LAYER 2: TTY 层等待 (foreground check)

          → ldisc.read()
            → block_on(poll_io(&WaitPollable, IN, false, || {
                buf_rx.pop_slice(buf)       ← 从 ldisc 内部缓冲区读
              }))
              LAYER 3: Ldisc 层等待

              ↓ 当数据到达时
              RX copier 任务
                → uart_instance().lock()
                → receive_bytes() → 读 UART FIFO
                → DRIVER.rx.lock().push()    ← push ring buffer

              ↓ 用户 read() 路径继续
              AsyncUartReader::read()
                → DRIVER.rx.lock().pop()    ← pop ring buffer
                → 返回数据给 ldisc

              ↓ ldisc InputReader.poll() 处理
                → 行规则：echo、信号检测、回显
                → push 到 buf_tx (ldisc 内部缓冲区)
                
              ← ldisc.read() 从 buf_rx pop_slice 成功
            ← LAYER 3 poll_io 返回 Ok(n)
          ← LAYER 2 poll_io 返回 Ok(n)
        ← inner.read() 返回
      ← LAYER 1 poll_io 返回 Ok(n)
  ← sys_read 返回 n
```

**关键发现**：RX 路径经过 **3 层嵌套 `block_on(poll_io(...))`**，每层都有 yield + re-schedule。

---

## §3 根因分析 — 5 层瓶颈分解

### 瓶颈 1：Triple-nested block_on — Yield Storm

每一层 `poll_io` 的行为模式：

```rust
poll_fn(move |cx| {
    match f() {
        Ok(v) => Poll::Ready(Ok(v)),
        Err(WouldBlock) => {
            pollable.register(cx, events);  // 注册 waker
            Poll::Pending                   // 让出 CPU
        }
    }
})
```

**在 `ProcessMode::Manual` 模式下，`register` 导致立即唤醒**：

| 层级 | Pollable | register 行为 | 效果 |
|------|----------|-------------|------|
| L1: File → Tty → ldisc | `ldisc.register_rx_waker()` → Manual → `waker.wake_by_ref()` | **立即 re-schedule** |
| L2: Tty | `JobControl::register()` → `poll_fg.register(waker)` | 正常等待（fg 不变时无唤醒） |
| L3: Ldisc | `WaitPollable::register()` → `waker.wake_by_ref()` | **立即 re-schedule** |

**结果**：L1 和 L3 的 waker 注册后立即触发，形成 **yield storm**（高频 yield-re-schedule 循环）。每次 yield 涉及上下文保存、调度器选择、上下文恢复。在无数据期间，CPU 被空转的 yield 循环消耗。

**定量估计**：一次 yield+reschedule 约 100-500 cycles（QEMU 中更多），无数据时每秒可能数十万次 yield。

### 瓶颈 2：Buffer 拷贝链过长

| 路径 | 拷贝链 | 拷贝次数 |
|------|--------|---------|
| **Console TX（旧）** | User buf → UART THR（逐字节） | 1 |
| **Async TX** | User buf → ring buffer → UART FIFO | 2 |
| **Console RX（旧）** | UART FIFO → ldisc buf → user buf | 2 |
| **Async RX** | UART FIFO → ring buffer → ldisc buf → user buf | **3** |

Async RX 比 Console RX **多一次 ring buffer 拷贝**（64 KB HeapRb）。Q10 数据路径优化已将其中部分拷贝合并。

### 瓶颈 3：锁竞争

| 锁 | 竞争方 | 频率 |
|-----|--------|------|
| `DRIVER.rx.lock()` | RX copier (push) vs AsyncUartReader (pop) vs ldisc.poll_read() | 每个数据包 |
| `DRIVER.tx.lock()` | TX copier (pop) vs AsyncUartWriter (push) vs ax_println! | 每个数据包 |
| `uart_instance().lock()` | RX copier vs TX copier（交替） | 每个数据包 |
| `ldisc.lock()` | Tty::read_at vs Tty::poll | 每次 read/poll |

自旋锁代价在 QEMU 中比真实硬件更昂贵（虚拟化开销放大）。

### 瓶颈 4：协程调度开销 — 额外内核任务

Async UART 引入的额外任务：

| 任务 | 栈 | 调度方式 | 唤醒频率 |
|------|-----|---------|---------|
| RX copier | ~4 KB | ISR AtomicWaker 唤醒 | 每个 RX 数据包 |
| TX copier | ~4 KB | ISR AtomicWaker + TX ring buf waker | 每个 TX 数据包 |
| tty-reader（Q7 External 模式后） | ~4 KB | PollSet 注册 | ldisc 有数据时 |

每次 copier 任务唤醒 = 一次完整上下文切换（~200-1000 cycles）。对比 Console：仅 1 个 tty-reader 任务。

### 瓶颈 5：Manual ProcessMode vs External ProcessMode

`ASYNC_TTY` 初始使用 `ProcessMode::Manual`：

**Manual 模式特点**：
- 无独立 tty-reader 任务（节省一个任务）
- 但 `poll_read()` 在调用者上下文执行（同步）
- `register_rx_waker()` 直接 `wake_by_ref()` — **导致 yield storm**

**External 模式（旧 Console 采用）**：
- 独立 tty-reader 任务，ISR 驱动读取
- `register_rx_waker()` 注册到 PollSet（不会立即唤醒）
- **不会产生 yield storm**

Manual 模式为省一个任务，引入了 yield storm（Q7 O42 已修复）。

### 硬件瓶颈：115200 波特率

串口线速度上限固定：

```
115200 bps = 11.52 KB/s（理论最大值，10 bits/byte × 115200）
```

无论阻塞还是异步，线速度都是同一上限。这意味着 TX 吞吐量差异不会超过线速。异步的优势不在原始吞吐量，而在**不阻塞调用方**（pipeline 能力）。

### 现有 Benchmark 问题

**TX 吞吐量测试不测 UART** — `benchmark.c` 使用 `/dev/null` 路径：
```c
// benchmark.c:44 — 不经过 UART！
open("/dev/null", O_WRONLY);
write(fd, buf, test_size);  // 测的是 VFS 写入 /dev/null 速度
```

**TX 延迟测试只测 ring buffer push** — 单字节 write 到 `/dev/console` 仅测 ring buffer push 延迟，不反映完整串口发送路径。

**用户态 RX 测试被跳过** — 因 TTY echo loop 问题（数据被 Shell 抢先读取），用户态 RX 测试改为内核态。内核态测试绕过 TTY 层直接测 ring buffer，不能反映真实用户态 RX 性能。

**Kernel benchmark 绕过完整路径** — `run_startup_benchmark()` 在 ring buffer 上直接测 push/pop，不经过 TTY/ldisc 层。

以上问题在 Q7 O44 中已修正（见 §5）。

### 对比总结：Async vs Blocking Console

| 维度 | Console（阻塞） | Async UART | 胜出方 |
|------|---------------|------------|--------|
| TX 吞吐量 | ~11.52 KB/s（线速） | ~11.52 KB/s（线速） | 持平 |
| TX 延迟（write 返回） | 每字节 ~87 µs（busy-wait） | ~1 µs（push ring buf） | **Async** |
| RX 端到端延迟 | ~100 µs（TTY 处理） | ~110 µs（extra copy） | Console（略优） |
| CPU 占用（TX） | 100%（发送期间） | ~1%（push）+ copier 开销 | Async（usr 侧） |
| CPU 占用（空闲等待） | 0%（无 tty-reader） | yield storm（Manual 模式）→ 0%（Q7 修复后） | 持平 |
| 代码复杂度 | 简单（外部 crate） | 复杂（~500 行 + ring buf） | Console |
| 内存占用 | 80 bytes ldisc buf | 128 KB ring buffer | Console |
| 上下文切换 | 无额外任务 | 2 copier 任务 + tty-reader | Console |

---

## §4 非阻塞模式 (FIONBIO) — 性能闭环的关键拼图

### 为什么 FIONBIO 是性能问题

在异步串口栈中，`read()` 经过 3 层嵌套 `block_on(poll_io(...))`（见 §2 RX 路径）。即使 File 层设置了 nonblocking=true（通过 `open(O_NONBLOCK)` / `fcntl(F_SETFL)` / `ioctl(FIONBIO)`），该标志**未能传播到内层 TTY 和 ldisc**，导致：

1. 无数据时 read() 仍然阻塞等待，不返回 `EAGAIN`
2. 性能测试无法区分"等待数据"和"数据处理"的时间
3. 用户态 poll/select 无法正确反映设备可读状态
4. **FIONBIO 对 TTY 设备读取无效**

### 当前实现状态

| 入口 | 位置 | 状态（Q7 前） | Q7 O43 后 |
|------|------|-------------|-----------|
| `open(O_NONBLOCK)` | `fd_ops.rs:106-108` | ✅ `File.set_nonblocking(true)` | ✅ |
| `fcntl(F_SETFL, O_NONBLOCK)` | `fd_ops.rs:253-254` | ✅ `File.set_nonblocking(true)` | ✅ |
| `ioctl(FIONBIO)` | `ctl.rs:28-38` | ✅ `File.set_nonblocking(bool)` | ✅ |
| **File 层 → TTY 层** | `tty/mod.rs:86-104` | ❌ 硬编码 `false` | ✅ 传播 nonblocking |
| **TTY 层 → ldisc 层** | `ldisc.rs:328-370` | ❌ 硬编码 `false` | ✅ 传播 nonblocking |

### 非阻塞标志传播断点

调用链中两个硬编码的 `false` 阻止了 nonblocking 传播：

```
File::read()
  → block_on(poll_io(File, IN, nonblocking=true, || inner.read()))
    → Tty::read_at()
      → block_on(poll_io(JobControl, IN, false, || ldisc.read(buf)))
      //                                   ^^^^^ 硬编码 false!
        → ldisc.read()
          → block_on(poll_io(WaitPollable, IN, false, || buf_rx.pop_slice(buf)))
          //                                   ^^^^^ 硬编码 false!
```

### 为什么 TX 路径不受影响

`AsyncUartWriter::write()` 总是立即返回（push ring buffer，不等待硬件），TX 天然非阻塞。所以 FIONBIO 对写路径没有影响——问题只在读路径。

### 跨层状态传播教训

> **任何跨层状态（如 O_NONBLOCK）MUST 穷举所有入口（open / fcntl / ioctl）并逐个验证。一个入口遗漏 = 功能不完整。**（FIONBIO 教训，参见 `learned` L140）

### 修复方案（O43，Q7 已落地）

| 文件 | 修改内容 |
|------|---------|
| `tty/mod.rs` | Tty struct 添加 `nonblocking: AtomicBool` 字段；`read_at()` 内使用 `self.nonblocking.load(Acquire)` 替代硬编码 false；DeviceOps ioctl 处理 FIONBIO → set nonblocking |
| `ldisc.rs` | `read()` 方法接受 `nonblocking: bool` 参数 → `block_on(poll_io(...))` 使用该参数 |
| `ctl.rs` | TTY ioctl 路径正确分发 FIONBIO |

**预期行为**（修复后）：

- 无数据时 `read()` 立即返回 `EAGAIN`（三种设置方式均生效）
- `poll()` / `select()` 在无数据时正确返回 0
- 与 Pipe、Socket、EventFd 等 FileLike 实现的 nonblocking 行为一致

---

## §5 修复方案（Q7 已落地，2026-06-01）

基于上述根因分析，Q7 实现了四项关键修复：

### O42 — 修复 yield storm：ProcessMode::Manual → External

**根因**：Manual 模式下 `register_rx_waker()` 直接 `wake_by_ref()`，导致 L1/L3 双层立即唤醒，形成 yield storm。

**修复**：

- `ntty_async.rs`：创建 `Arc<PollSet>`，传入 `ProcessMode::External(Box::new(move |waker| poll_rx.register(waker)))`
- `ldisc.rs`：External 模式自动创建 tty-reader 任务，`register_rx_waker` 使用 PollSet 注册（不再 `wake_by_ref`）

| 指标 | 修复前 | 修复后 |
|------|--------|--------|
| 空闲 CPU | 高频 yield-re-schedule，CPU 空转 | **0%**（无数据时休眠） |
| 任务数 | RX copier + TX copier | RX copier + TX copier + tty-reader（与旧 Console 代价相同） |
| 响应延迟 | 受 yield storm 影响波动 | 稳定 |

### O43 — 传播 FIONBIO nonblocking：TTY/ldisc 层感知标志

见 §4 详细分析。关键改动：Tty struct 加 `nonblocking: AtomicBool`，从 File 层 → Tty → ldisc 三入口（open/fcntl/ioctl）完整传播。

### O44 — 修正 benchmark：测量真实 UART 吞吐量

| 问题 | 旧实现 | 修复后 |
|------|--------|--------|
| TX 吞吐量 | `/dev/null`（绕过 UART） | `/dev/console` + `tcdrain()` 等待硬件发送完成 |
| TX 延迟 | 测 ring buffer push（~1 µs） | 端到端 `write+tcdrain` 测完整链路 |
| RX 测试 | 被跳过（echo loop） | raw mode + 独立测试程序 |
| 非阻塞测试 | 无 | 新增 `ioctl(FIONBIO)` + 无数据 read → EAGAIN 验证 |

### O45 — tcdrain 真异步化：消除协作自旋

**问题**：TCSBRK 实现使用 `wake_by_ref()` + `Pending` 协作式自旋。64 字节数据需 TX copier 发 4 批（每批 16 字节 FIFO），tcdrain 与 copier 交替重调度 9 次（~300 µs QEMU）。

**修复**：三段式 PollSet + DRAIN_WAKER 等待。

```
tcdrain 等待逻辑：
1. ring buffer 有数据 → 注册到 tx.poll，等 copier pop 唤醒
2. ring buffer 为空但 UART 还在发 → 注册到 DRAIN_WAKER，等 TX ISR 唤醒
3. ring buffer 空 + LSR TRANSMITTER_EMPTY → 返回
```

**关键实现**：`isr.rs` 新增 DRAIN_WAKER，TX 中断时同时唤醒；`ctl.rs` TCSBRK 三段式等待。

| 指标 | 修复前 | 修复后 |
|------|--------|--------|
| 64B tcdrain 任务切换次数 | 9 次 | ~6 次 |
| 64B tcdrain 延迟（QEMU） | ~300 µs | ~200 µs |
| 机制 | 协作自旋（wake_by_ref） | 事件驱动（PollSet + DRAIN_WAKER） |

---

## §6 性能基线与硬件约束

### NS16550 @ 115200 bps 硬件理论极限

| 参数 | 值 |
|------|-----|
| 线速 | 11,520 B/s（10 bits/byte × 115200） |
| 单字节传输时间 | 86.8 µs |
| FIFO 深度 | 16 字节 |
| IRQ 频率（阈值 14） | ~823/秒，间隔 1.22 ms |
| ISR 总延迟 | ~1.5 µs（< 0.1% 线时间） |
| MMIO 单次访问 | ~100~200 ns |

### 当前指标（QEMU，Q7 修复后）

| 指标 | 测量方法 | 当前值 |
|------|---------|--------|
| TX 吞吐量 @115200 | `write → tcdrain()`，5 秒批量 | ~11.5 KB/s（线速） |
| TX 延迟 P50 | 单字节 `write+tcdrain` | ~1 µs（ring buf push），端到端 ~87 µs/byte |
| RX 吞吐量（内核态） | Ring buffer 直接测，绕过 TTY | 588 MB/s |
| RX 延迟（内核态） | Ring buffer P50 | 600 ns |
| 空闲 CPU | 无数据 10 秒 | **0%**（O42 yield storm 修复后） |
| CPU 效率 | 统一 102,400 字节写 | 268 cycles/byte（Console: 3,835 cycles/byte，快 14.3×）|
| 非阻塞读 | `ioctl(FIONBIO)` + `read()` | 无数据立即 EAGAIN（O43 修复后） |
| tcdrain 延迟（64B） | QEMU | ~200 µs（O45 修复后） |

### QEMU vs 真板可信度

| 测试类型 | QEMU 可信度 | 说明 |
|---------|------------|------|
| 内核态 ring buffer 速度 | ✅ 可信 | 纯 CPU/Memory 操作，不受 UART 仿真影响 |
| write() 延迟 | ✅ 可信 | 只测 ring buf push |
| CPU cycles/byte | ✅ 可信 | 纯软件度量 |
| **串口吞吐量** | ❌ **不可信** | QEMU 16550 不仿真串口线延迟 |
| **tcdrain/LSR 轮询延迟** | ❌ **不可信** | 同上 |
| 用户态 RX 延迟 | ⚠️ 部分可信 | 受 ldisc/TTY 影响，但线延迟缺失 |

**真板预期**：VisionFive2 @ 115200 bps → TX ~11.5 KB/s（硬上限）。Q6 真板验证待 VisionFive2 硬件到位后执行。

---

## 关键文件索引

| 文件 | 作用 | 核心内容 |
|------|------|---------|
| `kernel/src/drivers/async_driver.rs` | AsyncUartDriver + RX/TX copier | copier 循环、NAPI 中断合并 |
| `kernel/src/drivers/device_ops.rs` | AsyncUartReader/Writer | TtyRead/TtyWrite trait 实现 |
| `kernel/src/drivers/ring_buffer.rs` | 环形缓冲区 | HeapRb + PollSet 封装 |
| `kernel/src/drivers/isr.rs` | UART 中断处理 | AtomicWaker 唤醒（RX/TX/DRAIN） |
| `kernel/src/pseudofs/dev/tty/mod.rs` | Tty 设备 | DeviceOps + nonblocking + FIONBIO |
| `kernel/src/pseudofs/dev/tty/ntty_async.rs` | Async TTY 初始化 | ProcessMode + PollSet |
| `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs` | 行规则 | InputReader + nonblocking 传播 |
| `kernel/src/pseudofs/dev/tty/terminal/job.rs` | JobControl | 前台检测 + PollSet |
| `kernel/src/file/fs.rs` | File 封装 | block_on + poll_io 非阻塞路径 |
| `kernel/src/syscall/fs/ctl.rs` | ioctl 系统调用 | FIONBIO + TCSBRK(tcdrain) |
| `kernel/src/syscall/fs/fd_ops.rs` | 文件描述符操作 | open/fcntl O_NONBLOCK 传播 |
| `tests/benchmark.c` | 用户态基准测试 | TX/RX 吞吐量、延迟、FIONBIO 测试 |
| `openspec/specs/optimization/spec.md` | 优化记录 | Q5/Q7/Q8 完整优化清单与性能基线 |
