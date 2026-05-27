> ⚠️ 此文档为早期分析，部分内容已过时。
> 最新决策参见 architecture.md ADR-013~ADR-015。

# 异步运行时深度分析

> 基于 StarryOS axtask/axsync 运行时的分析
> 分析日期：2026-05-24

---

## 1. axtask 调度器

### 1.1 核心抽象

axtask 基于等待队列（WaitQueue）的协作式调度器：

```rust
// 核心调度原语
pub fn spawn(future: impl Future<Output = ()> + Send + 'static) -> TaskId;
pub fn block_on<F: Future>(future: F) -> F::Output;
pub fn yield_now();
pub fn schedule(task: Arc<TaskInner>);  // 将任务加入就绪队列
```

### 1.2 任务状态机

```
Created → Ready → Running → Blocked → Ready → ... → Finished
              ↑              ↓
              └── schedule ──┘
                      ↑
              └── wake ──┘
```

- **Ready**：在就绪队列中，等待被调度
- **Running**：正在 CPU 上执行
- **Blocked**：等待某个事件（WaitQueue / Waker）
- **Finished**：任务完成

### 1.3 WaitQueue 机制

```rust
pub struct WaitQueue {
    queue: SpinLock<Deque<Arc<TaskInner>>>,
}

impl WaitQueue {
    pub fn wait_until(&self, condition: impl Fn() -> bool) -> bool;
    pub fn wait_until_timeout(&self, condition: impl Fn() -> bool, timeout: Duration) -> bool;
    pub fn notify_one(&self) -> bool;
    pub fn notify_all(&self) -> bool;
}
```

这是当前内核中最重要的同步原语。pipe.rs、event.rs 等都基于 WaitQueue 实现阻塞。

### 1.4 调度策略

- 单核：就绪队列 FIFO
- 协作式：任务主动 yield 或等待事件时让出 CPU
- **没有抢占式调度**（对内核任务而言）

---

## 2. AxWaker 机制

### 2.1 Waker 实现

```rust
// axtask 中的 Waker
struct AxWaker {
    task: Arc<TaskInner>,
}

impl RawWaker for AxWaker {
    fn wake(self) {
        self.task.schedule();  // 将任务加入就绪队列
    }
    fn wake_by_ref(&self) {
        self.task.schedule();
    }
}
```

Waker 的 `wake()` 本质上是把任务重新放回就绪队列。下次调度器轮转到该任务时，`poll()` 会被再次调用。

### 2.2 Waker 在中断上下文中的使用

`waker.wake_by_ref()` 在中断上下文中是安全的，因为：
1. 只是写入就绪队列（SpinLock 短暂持有）
2. 不涉及内存分配
3. 不阻塞

**关键**：中断上下文中只能用 `wake_by_ref()`，不能用 `wake()`（后者会 drop Waker，涉及内存释放）。

---

## 3. embassy-sync::AtomicWaker

### 3.1 AtomicWaker 的特性

```rust
// embassy-sync::AtomicWaker（no_std 兼容）
pub struct AtomicWaker {
    waker: AtomicPtr<()>,  // 原子指针存储 Waker
}

impl AtomicWaker {
    pub fn register(&self, waker: &Waker);
    pub fn wake(&self);
    pub fn take(&self) -> Option<Waker>;
}
```

**与 AxWaker 的关系**：
- AtomicWaker 是一个存储层，可以存储任意 `Waker`（包括 AxWaker）
- `register()` 将当前任务的 Waker 注册到 AtomicWaker
- `wake()` 唤醒注册的任务（调用 `waker.wake()`）
- **线程安全**：`register` 和 `wake` 可以在不同上下文调用（ISR 和任务上下文）

### 3.2 为什么选择 AtomicWaker 而非 WaitQueue

| 特性 | WaitQueue | AtomicWaker |
|------|-----------|-------------|
| 通知粒度 | notify_one / notify_all | 只唤醒一个 Waker |
| 存储位置 | 内核堆 | 原子变量（栈/静态） |
| 中断上下文安全 | 不安全（涉及 SpinLock） | 安全（原子操作） |
| 多等待者 | 支持（队列） | 不支持（单 Waker） |
| no_std | 是 | 是 |
| 依赖 | axtask 内置 | embassy-sync crate |

**结论**：中断驱动的异步串口需要 AtomicWaker（ISR 中安全唤醒），而非 WaitQueue（ISR 中不安全）。

### 3.3 embassy-sync 依赖可行性

**可行性分析**：
- `embassy-sync` 是 `no_std` crate，不依赖 `embassy-executor`
- 它只提供 `AtomicWaker`、`Channel`、`Signal` 等同步原语
- 不需要完整的 embassy 运行时
- 添加到 `kernel/Cargo.toml` 的依赖成本很低

**当前 kernel/Cargo.toml 的相关依赖**：
```toml
axtask = { ... }
axsync = { ... }  # 提供 WaitQueue 等
```

**需要添加**：
```toml
embassy-sync = "0.7"  # 版本需确认与 Rust nightly-2026-02-25 的兼容性
```

---

## 4. poll 机制

### 4.1 当前 PollSet 实现

```rust
// axpoll / PollSet
pub struct PollSet {
    watchers: BTreeMap<usize, Watcher>,  // fd → watcher
}

struct Watcher {
    waker: Waker,
    events: PollEvents,
}
```

PollSet 的工作方式：
1. 用户态调用 `poll(syscall)` → 内核创建 PollSet
2. PollSet 注册多个 fd 的 Waker
3. 当 fd 有事件时唤醒 Waker
4. 返回就绪的 fd 列表

### 4.2 pipe.rs 的异步模式

pipe.rs 使用 WaitQueue 实现读写阻塞：

```rust
pub struct PipeReader {
    buf: SpinLock<RingBuffer>,
    waker: WaitQueue,  // 写端唤醒读端
}

pub struct PipeWriter {
    buf: Arc<SpinLock<RingBuffer>>,
    waker: WaitQueue,  // 读端唤醒写端
}

impl Read for PipeReader {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut ring = self.buf.lock();
        loop {
            let n = ring.read(buf);
            if n > 0 { return Ok(n); }
            // 缓冲区空，阻塞等待
            drop(ring);
            self.waker.wait_until(|| self.buf.lock().len() > 0);
            ring = self.buf.lock();
        }
    }
}
```

**关键观察**：pipe 的阻塞通过 WaitQueue + SpinLock 实现，不是真正的 async/await。

### 4.3 EventFd 的异步模式

```rust
pub struct EventFd {
    count: AtomicU64,
    waker: WaitQueue,
}

impl Read for EventFd {
    fn read(&self, buf: &mut [u8]) -> Result<usize> {
        loop {
            match self.count.compare_exchange(...) {
                Ok(val) => { return Ok(val.to_ne_bytes()); }
                Err(_) => { self.waker.wait_until(|| self.count.load() > 0); }
            }
        }
    }
}
```

同样是 WaitQueue + 原子变量的模式。

---

## 5. N_TTY 的 tty-reader 模式

### 5.1 完整异步流程

这是当前项目中**唯一的异步串口 IO 模式**，需要深入理解：

```
1. N_TTY 初始化:
   - register_irq_waker(UART_IRQ, &waker)
   - spawn(tty_reader_task)

2. tty_reader_task (永久运行):
   loop {
       let result = poll_fn(|cx| {
           // 注册 waker
           register_irq_waker(UART_IRQ, cx.waker());
           // 尝试读取
           let n = console::read_bytes(buf);
           if n > 0 { return Poll::Ready(n); }
           // 没数据，挂起
           Poll::Pending
       }).await;

       // 有数据，处理输入
       process_input(buf, n);
   }

3. UART 中断到来:
   → PLIC claim → dispatch_irq(10) → waker.wake_by_ref()
   → tty_reader_task 被调度恢复 → poll_fn 再次 poll → read_bytes 有数据 → Ready
   → 处理输入 → 回到循环
```

### 5.2 这个模式的局限性

1. **单任务独占**：只有一个 tty-reader，所有输入都经过它
2. **RX 中断从不禁用**：每次中断都唤醒 tty-reader，即使没人读
3. **无 TX 异步**：写操作仍然是同步阻塞的
4. **无缓冲区**：从硬件 FIFO 直接读，如果 tty-reader 来不及处理，硬件 FIFO 可能溢出
5. **耦合 TTY 语义**：和 line discipline 绑定，不是通用的异步串口原语

---

## 6. 异步串口需要的运行时支持

### 6.1 核心 Future 设计

```rust
// 异步读 Future
pub struct UartRx {
    _private: (),
}

impl Future for UartRx {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // 1. 注册 Waker
        RX_WAKER.register(cx.waker());
        // 2. 检查是否有数据
        if has_rx_data() {
            Poll::Ready(())
        } else {
            // 3. 使能 RX 中断，等待 ISR 唤醒
            enable_rx_intr();
            Poll::Pending
        }
    }
}
```

### 6.2 与现有运行时的集成点

| 组件 | 现有接口 | 异步串口需要 |
|------|---------|-------------|
| axtask | spawn, block_on, yield_now | 不需要修改 |
| AxWaker | wake, wake_by_ref | 不需要修改 |
| WaitQueue | wait_until, notify | 不使用（改用 AtomicWaker） |
| PollSet | register, poll | 可能需要扩展（见下） |
| register_irq_waker | 单 Waker | 需要支持双 Waker（RX/TX） |

### 6.3 PollSet 与异步串口的集成

**用户态 poll() 场景**：
1. 用户进程打开 /dev/ttyS0
2. 调用 poll(fd, POLLIN) 等待数据
3. 内核需要将 PollSet 的 Waker 注册到 UART 的 RX Waker 链
4. RX 中断到来 → 唤醒 PollSet → poll() 返回

**实现方式**：
- 方案 A：在 DeviceOps trait 中添加 `poll` 方法
- 方案 B：在内核 tty 设备中实现 `AxPoll` trait

**存疑：PollSet 的 Waker 是否支持链式注册（多个 Waker 同时等待同一事件）？**

---

## 7. 存疑问题

| 编号 | 问题 | 影响 | 需要确认 |
|------|------|------|---------|
| Q8 | `embassy-sync` 的哪个版本与 `nightly-2026-02-25` 兼容？ | 依赖选型 | 实验验证 |
| Q9 | `register_irq_waker` 是 per-cpu 还是全局的？ | 多核场景影响 | 代码审查 |
| Q10 | axtask 的 `spawn` 创建的任务是否支持 Future？还是只支持闭包？ | 异步任务创建方式 | 代码确认 |
| Q11 | PollSet 是否支持链式 Waker？一个事件唤醒多个等待者？ | poll/epoll 集成 | 代码审查 |