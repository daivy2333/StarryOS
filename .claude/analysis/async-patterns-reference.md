# Async Patterns Reference — StarryOS 异步 IO 模式目录

> Part of StarryOS codebase analysis (branch: asyncuart-dev) | Generated 2026-06-11
> Based on: `docs/analysis/reference-implementations.md` (2026-05-24)

---

## §1 Pipe Pattern — 环形缓冲区 + PollSet

Pipe 是 StarryOS 异步 IO 的**基准模式**：单一 `Arc<Shared>` 承载环形缓冲区与三组 PollSet，`block_on(poll_io(...))` 包裹同步尝试，成功即返回、失败则注册 Waker 后挂起。

### 1.1 数据结构

```rust
struct Shared {
    buffer: Mutex<HeapRb<u8>>,    // 环形缓冲区 (64 KiB)
    poll_rx:  PollSet,             // 读取端 Waker 集合
    poll_tx:  PollSet,             // 写入端 Waker 集合
    poll_close: PollSet,           // 关闭事件 Waker 集合
}

pub struct Pipe {
    read_side: bool,
    shared: Arc<Shared>,
    non_blocking: AtomicBool,
}
```

| 设计决策 | 理由 |
|----------|------|
| `Mutex<HeapRb<u8>>` 而非无锁 | `ringbuf` crate 的 split 模式需 `&mut` 引用推进读写指针 |
| 三个 PollSet 分别对应 IN / OUT / HUP | 精确事件分发，避免伪唤醒 |
| `Arc<Shared>` | 读写两端共享同一缓冲区，支持多 fd 引用 |

### 1.2 异步读流程

核心模式：**`block_on(poll_io(self, events, nonblocking, || { 同步尝试 }))`**

1. `poll_io` 先调用闭包尝试同步操作
2. 成功 → 直接返回数据
3. `WouldBlock` → `register(cx, events)` 注册 Waker → 返回 `Pending`
4. 事件到达 → Waker 唤醒 → `poll_io` 重新调用闭包

```rust
fn read(&self, dst: &mut IoDst) -> AxResult<usize> {
    block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
        let read = {
            let cons = self.shared.buffer.lock();
            let (left, right) = cons.as_slices();       // 零拷贝读取
            let mut count = dst.write(left)?;
            if count >= left.len() { count += dst.write(right)?; }
            unsafe { cons.advance_read_index(count) };
            count
        };
        if read > 0 {
            self.shared.poll_tx.wake();   // 通知写端有空间
            Ok(read)
        } else if self.closed() {
            Ok(0)                         // EOF
        } else {
            Err(AxError::WouldBlock)
        }
    }))
}
```

### 1.3 Pollable 实现

```rust
impl Pollable for Pipe {
    fn poll(&self) -> IoEvents {
        let mut events = IoEvents::empty();
        let buf = self.shared.buffer.lock();
        if self.read_side {
            events.set(IoEvents::IN,  buf.occupied_len() > 0);
            events.set(IoEvents::HUP, self.closed());
        } else {
            events.set(IoEvents::OUT, buf.vacant_len() > 0);
        }
        events
    }

    fn register(&self, context: &mut Context<'_>, events: IoEvents) {
        if events.contains(IoEvents::IN)  { self.shared.poll_rx.register(context.waker()); }
        if events.contains(IoEvents::OUT) { self.shared.poll_tx.register(context.waker()); }
        self.shared.poll_close.register(context.waker());
    }
}
```

**关键**：`register()` 将 Waker 注册到对应 PollSet。PollSet 容量 64，超过时替换最旧的 Waker。

### 1.4 对异步串口的适用性

| Pipe 特性 | 串口适用性 | 备注 |
|-----------|-----------|------|
| `Mutex<HeapRb>` | 可用 | ISR 中不能获取 Mutex，需 ISR → Waker → 任务上下文获取锁 |
| 三组 PollSet | 需要 rx + tx | 串口无"关闭"事件，可加 HUP（载波检测） |
| `block_on(poll_io(...))` | 完全适用 | 异步串口 read/write 的标准模式 |
| `Arc<Shared>` | 可用 | 支持多 fd 引用同一设备 |

---

## §2 EventFd Pattern — 原子变量 + PollSet

EventFd 展示了**无锁异步通知**的极简模式：`AtomicU64` 计数器替代 `Mutex<HeapRb>`，`fetch_update` 做 CAS 状态转换，PollSet 仅用于 Waker 管理。

### 2.1 数据结构

```rust
pub struct EventFd {
    count: AtomicU64,           // 计数器（原子操作，无锁）
    semaphore: bool,            // true = 信号量模式 (每次 -1)；false = 计数器模式 (清零)
    non_blocking: AtomicBool,
    poll_rx: PollSet,           // count > 0 时可读
    poll_tx: PollSet,           // count < u64::MAX 时可写
}
```

### 2.2 异步读流程

```rust
fn read(&self, dst: &mut IoDst) -> axio::Result<usize> {
    block_on(poll_io(self, IoEvents::IN, self.nonblocking(), || {
        let result = self.count.fetch_update(Ordering::Release, Ordering::Acquire, |count| {
            if count > 0 {
                let dec = if self.semaphore { 1 } else { count };
                Some(count - dec)
            } else {
                None  // count == 0 → WouldBlock
            }
        });
        match result {
            Ok(count) => {
                dst.write(&count.to_ne_bytes())?;
                self.poll_tx.wake();  // 通知写端
                Ok(size_of::<u64>())
            }
            Err(_) => Err(AxError::WouldBlock),
        }
    }))
}
```

### 2.3 对比 Pipe 与 EventFd

| 维度 | Pipe | EventFd |
|------|------|---------|
| 共享状态 | `Mutex<HeapRb<u8>>` | `AtomicU64` |
| 锁策略 | 互斥锁保护读写指针 | 无锁 CAS |
| 数据载体 | 环形缓冲区 (64 KiB) | 8 字节计数器 |
| 适用场景 | 字节流传输 | 事件通知 / 信号量 |
| 串口参考价值 | 环形缓冲区结构 | ISR 中可直接原子操作 |

### 2.4 对异步串口的启示

EventFd 证明**纯原子 + PollSet** 的模式可行，无需 Mutex。但串口环形缓冲区需同时修改读/写指针（`HeapRb::advance_read_index` 需 `&mut`），不能纯原子。实际方案是 **ISR 用 AtomicWaker 唤醒 copier 任务，copier 任务在任务上下文中获取 Mutex 操作缓冲区**——这是 §3 讨论的 ISR + 后台任务模式的核心。

> **AtomicWaker 迁移 (Q8.6–9)**：早期设计使用 `embassy_sync::AtomicWaker` 在 ISR 中直接唤醒，后续迭代（Q8.6–9）验证了方案正确性并固化为标准模式。ISR 仅做 (1) 读 ISR (2) 禁中断 (3) `AtomicWaker::wake()` (4) 返回，数据搬运完全在 copier 任务中完成。

---

## §3 Common Patterns — 通用异步原语

### 3.1 PollSet 注册机制

PollSet 是 StarryOS 最基础的 Waker 集合容器，容量 64，溢出时替换最旧条目：

```
注册: poll_set.register(cx.waker())
唤醒: poll_set.wake()  →  遍历所有 Waker → wake_by_ref()
```

**使用位置**：Pipe (rx/tx/close)、EventFd (rx/tx)、TTY ldisc (rx/tx)、Epoll (ready_queue)。

### 3.2 `block_on` + `poll_fn` 执行器

```
用户态系统调用 (read/write/poll/epoll)
         │
         ▼
  FileLike trait: read() / write()
         │
         ▼
  block_on(poll_io(self, events, nonblocking, || {
      同步尝试 → Ok / WouldBlock
  }))
         │ WouldBlock
         ▼
  Pollable trait:
    poll() → IoEvents      (非阻塞查询)
    register(cx, events)   (注册 Waker 到 PollSet / AtomicWaker)
         │
         ▼
  事件源:
    中断 (register_irq_waker)
    超时 (TimerFuture)
    其他任务 (PollSet.wake)
```

### 3.3 Waker 生命周期

| 阶段 | 操作 | 负责组件 |
|------|------|---------|
| 注册 | `register(cx.waker())` 将 Waker 克隆入 PollSet | Pollable::register |
| 挂起 | 闭包返回 `WouldBlock` → `poll_io` 返回 `Pending` | poll_io |
| 唤醒 | 事件源调用 `PollSet::wake()` / `AtomicWaker::wake()` | 中断处理 / 其他任务 |
| 重试 | 执行器重新 poll Future → `poll_io` 再次调用闭包 | block_on |

### 3.4 ISR + 后台 Copier 任务模式 (源自 TTY)

TTY 的 `tty-reader` 任务展示了 ISR 最小化 + 后台数据搬运的标准模式：

```rust
// 原始模式 (ldisc.rs:258–279)
axtask::spawn_with_name(move || {
    block_on(poll_fn(|cx| {
        // 注册前先尝试处理已有数据（防止丢失事件）
        while reader.poll() { poll_rx.wake(); }
        // 注册 TX Waker + IRQ Waker（同一个 cx.waker()）
        poll_tx.register(cx.waker());
        register(cx.waker().clone());   // register_irq_waker
        // 中断到达后再次尝试
        while reader.poll() { poll_rx.wake(); }
        Poll::Pending  // 永不结束
    }))
}, "tty-reader".into());
```

| 模式要素 | TTY 实现 | 异步串口实现 |
|---------|---------|-------------|
| 后台常驻任务 | `tty-reader` (单任务独占) | `rx_copier` + `tx_copier` (双任务) |
| ISR 职责 | register_irq_waker 触发 | AtomicWaker::wake() (极简 ISR) |
| 双重 poll | 注册前后都 poll，防事件丢失 | 同样适用 |
| 数据搬运 | InputReader::poll() → ldisc buf_rx | copier: FIFO ↔ RingBuffer |
| Waker 分发 | 单 Waker 唤醒 tty-reader | 双 Waker (rx_waker / tx_waker) 精确唤醒 |

### 3.5 Epoll Waker 桥接

Epoll 通过 `InterestWaker` 将底层 Pollable 事件桥接到 Epoll 就绪队列：

```rust
struct InterestWaker {
    epoll: Weak<EpollInner>,
    interest: Weak<EpollInterest>,
}

impl Wake for InterestWaker {
    fn wake_by_ref(self: &Arc<Self>) {
        let Some(epoll) = self.epoll.upgrade() else { return; };
        let Some(interest) = self.interest.upgrade() else { return; };
        if interest.try_mark_in_queue() {   // CAS 避免重复入队
            epoll.ready_queue.lock().push_back(Arc::downgrade(&interest));
            epoll.poll_ready.wake();
        }
    }
}
```

只要串口设备实现 `Pollable` trait（`poll()` + `register()`），Epoll 自动完成桥接，无需额外适配代码。

---

## §4 Key Interfaces — 核心 Trait 与实现者

### 4.1 异步串口需实现的接口层次

| 层次 | Trait | 必须实现的方法 | 模式来源 |
|------|-------|---------------|---------|
| VFS 层 | `DeviceOps` | `read_at(buf)` → `block_on(poll_io(self, IN, ...))` | Pipe |
| | | `write_at(buf)` → `block_on(poll_io(self, OUT, ...))` | Pipe |
| | | `as_pollable()` → `Some(self)` | Pipe |
| 事件层 | `Pollable` | `poll()` → 查询环形缓冲区状态 | Pipe |
| | | `register(cx, events)` → 注册 Waker 到 PollSet | Pipe |
| 事件源 | (中断) | UART IRQ → ISR → AtomicWaker.wake() → copier 任务 | TTY |
| 事件源 | (copier) | 硬件 FIFO ↔ 环形缓冲区 → PollSet.wake() | TTY |

### 4.2 现有组件复用度

| 组件 | 复用度 | 说明 |
|------|--------|------|
| `block_on(poll_io(...))` | 100% | 完全复用，异步 read/write 的标准包装 |
| `PollSet` | 100% | 完全复用，Waker 集合管理 |
| `ringbuf::HeapRb` | 100% | 完全复用，环形缓冲区 |
| `DeviceOps` + `Device` wrapper | 100% | 完全复用，VFS 设备注册框架 |
| `register_irq_waker` | 部分 | 需扩展支持 RX/TX 双 Waker 分别注册 |
| TTY `tty-reader` 模式 | 核心参考 | ISR + copier 任务架构 |
| LineDiscipline | 可选 | raw 模式不需要；termios 模式复用 |

### 4.3 存疑问题

| 编号 | 问题 | 影响范围 |
|------|------|---------|
| Q12 | `ringbuf::HeapRb::advance_read_index` 是否需 `&mut`？ISR 中能否直接操作？ | ISR 与 copier 分工 |
| Q13 | PollSet 容量 64 是否足够？多 epoll 实例监视同一串口 fd？ | 多路复用场景 |
| Q14 | `block_on` 在内核任务上下文中是否可重入？ | 嵌套异步操作安全性 |
