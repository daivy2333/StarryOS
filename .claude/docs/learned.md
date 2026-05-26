# learned.md — 项目学习记忆

> 由 project-rules-generator 初始化，由 project-docs-assistant 日常维护。
> 条目格式: <!-- L{编号} --> 标记开头，支持 grep 精确定位。

---

## API 路径

<!-- 添加时格式: <!-- L{编号} --> | 名称 | 路径 | 用途 | 时间 | -->

<!-- L1 --> | axtask::future::block_on | 异步任务阻塞执行 | 2026-05-24 |
<!-- L2 --> | axtask::future::poll_io | WouldBlock → register → await 标准模式 | 2026-05-24 |
<!-- L3 --> | axtask::future::register_irq_waker | 连接中断到异步任务唤醒 | 2026-05-24 |
<!-- L4 --> | embassy_sync::AtomicWaker::wake | ISR 中安全唤醒 Waker，无锁中断安全 | 2026-05-24 |

## 文件速查

<!-- 添加时格式: <!-- L{编号} --> | 名称 | 路径 | 用途 | 时间 | -->

<!-- L5 --> | Pipe 异步管道 | kernel/src/file/pipe.rs | poll_io + register_irq_waker 模式参考 | 2026-05-24 |
<!-- L6 --> | EventFd | kernel/src/file/event.rs | 轻量异步通知模式参考 | 2026-05-24 |
<!-- L7 --> | DeviceOps 设备注册 | kernel/src/pseudofs/device.rs | DeviceOps trait + Device 包装 | 2026-05-24 |
<!-- L8 --> | UART 硬件操作 | axhal/src/platform/riscv64_qemu_virt/uart.rs | MMIO 寄存器定义 | 2026-05-24 |
<!-- L9 --> | PLIC 中断映射 | axhal/src/platform/riscv64_qemu_virt/mod.rs | PLIC 中断号 | 2026-05-24 |

## 踩坑档案

<!-- 添加时格式: <!-- L{编号} --> ### [{问题标题}] 后跟症状→根因→解 -->

<!-- L10 --> ### embassy-executor 与 axtask 冲突
- 症状: 引入完整 Embassy 后调度器冲突
- 根因: embassy-executor 自带调度器，与 axtask 不兼容
- 解: 只引入 embassy-sync::AtomicWaker，异步运行时使用 axtask::future

<!-- L11 --> ### HeapRb 非中断安全
- 症状: 在 ISR 中直接操作 ringbuf 导致数据竞争
- 根因: HeapRb 的 Producer/Consumer 不是中断安全的
- 解: 硬件 FIFO 和内核 ringbuf 之间的搬运由单一后台协程完成

## 技巧模式

<!-- 添加时格式: <!-- L{编号} --> ### [{技巧标题}] 后跟描述 -->

<!-- L12 --> ### ISR 极简原则
中断处理只做三件事：
1. 清中断标志
2. 唤醒 Waker（AtomicWaker::wake）
3. 立即退出
数据搬运推迟到任务上下文（后台协程）。

<!-- L13 --> ### 双缓冲 Ring Buffer
- RX: 硬件 FIFO → ringbuf → 用户空间
- TX: 用户空间 → ringbuf → 硬件 FIFO
- 后台协程是唯一操作硬件的角色，天然无竞态

<!-- L14 --> ### Pollable 模式
参考 Pipe 实现：
- `poll()`: 非阻塞查询当前事件状态
- `register()`: 保存 Waker，等待唤醒
- `PollSet` 容量上限 64 个 Waker

<!-- L15 --> ### VFS 集成转换链
设备实现 `DeviceOps` trait → 包装为 `Device` → 自动获得 `FileLike` 能力。
`Device` → `NodeOps` → `FileNodeOps` → `File` → `FileLike`。
`as_pollable()` 返回 `Some(self)` 以支持 poll/select/epoll。

<!-- L45 --> ### Embassy 执行器轮询机制
1. 任务被 poll → 执行到阻塞点 → 返回 Poll::Pending
2. 执行器将任务重新加入运行队列末尾，继续执行下一任务
3. 被唤醒的任务重新入队（仅轮询被唤醒的任务，而非全部）
4. 无任务可执行时 CPU 进入休眠（WFE/WFI）——无空转轮询
5. 即使某任务被频繁唤醒，也不会独占 CPU——公平调度保证
与 StarryOS 的关系: 我们用 axtask::future 而非 embassy-executor，但异步调度原理相同。
block_on + poll_io 的模式本质就是执行器轮询 + waker 唤醒。

<!-- L46 --> ### Embassy 中断与异步的配合流程
1. 任务被轮询，尝试取得进展
2. 任务指示外设执行操作，并等待
3. 外设完成操作，发出中断
4. HAL 将中断信号路由到外设，更新外设状态
5. 执行器收到通知，任务可继续被轮询
映射到 StarryOS: ISR → AtomicWaker::wake() → axtask 协程被唤醒 → poll 继续执行。
这就是 ADR-008 ISR → AtomicWaker → copier 任务模型的底层原理。

<!-- L47 --> ### Embassy 异步 vs 中断驱动对比
- 中断驱动: 需要全局 Mutex 保护共享状态，代码复杂度高（70+ 行）
- Embassy 异步: 同样简洁（25 行），但多了 Waker 自动休眠/唤醒
- 关键差异: 异步版本 wait_for_edge().await 使任务挂起，无任务时 CPU 进入睡眠
- 这验证了我们的选择: axtask::future 异步模式比传统中断驱动代码更简洁，且同样零 CPU 空转

<!-- L48 --> ### embassy-sync 与 nightly 兼容性
Embassy 使用 `type_alias_impl_trait` 等 nightly 特性。
embassy-sync 本身可能不需要所有 nightly 特性，但需要验证与 nightly-2026-02-25 的兼容性。
参见存疑 Q8 (L31)。验证方法: 在 Cargo.toml 添加依赖后 `cargo check`。

<!-- L49 --> ### InterruptExecutor 多优先级模式
Embassy 支持创建多个 InterruptExecutor 实例，以不同优先级运行任务。
这对应 optimization.md O5 "优先级调度"——远期若 axtask 不支持优先级，
可考虑在 axtask 之上实现类似的多优先级调度域。

<!-- L60 --> ### TX 同步阻塞实测时间（理论估算）
波特率 115200 bps 下：
- 发送 1 字节: ~86.8 µs CPU 空转
- 发送 1 KB: ~87 ms CPU 空转
- 发送 64 KB: ~5.5 s CPU 空转
波特率 1 Mbps 下：
- 发送 1 KB: ~10 ms CPU 空转
- 发送 64 KB: ~640 ms CPU 穽转
这是当前 Console write_bytes 忙等待循环的阻塞时长，验证了异步化必要性。

<!-- L61 --> ### RX 接收路径多层开销分析
Console (N_TTY) 接收路径虽中断驱动，但经过多层处理：
- UART → Console.read_bytes → N_TTY → ldisc → termios → 用户态
- 即使 raw 模式，数据仍经过 ldisc 处理路径（条件判断、信号检测）
- 多层 buffer 复制：Console buffer → N_TTY buffer → 用户态 buffer
AsyncUart 优化：默认 raw 模式直接读写 rx_buf，跳过 ldisc，零行规则开销。

<!-- L62 --> ### 异步改造预期性能目标
| 指标 | 目标 | 当前状态 | 改进幅度 |
|------|------|---------|---------|
| 最大波特率 | 1 Mbps (可扩展至 2 Mbps) | 受阻于同步阻塞 | 10x ↑ |
| 吞吐量 | > 90% 线速 (115200 bps 下 > 10 KB/s) | 受阻于 CPU 空转 | 估算 5x ↑ |
| RX 延迟 | P50 < 500 µs, P99 < 2 ms | 已中断驱动但多层开销 | 估算 2x ↓ |
| CPU 利用率（空闲） | 0% | TX 阻塞时 100% 穽转 | 100% ↓ |
| 多端口并发 | 4 端口 | 1 端口 | 4x ↑ |
参见 docs/analysis/serial-optimization-preview.md 第 6 节。

## 依赖关系图

<!-- 添加时格式: <!-- L{编号} --> 关键依赖间的调用/依赖关系 -->

<!-- L16 --> axtask::future ← register_irq_waker ← PLIC ISR → AtomicWaker::wake → 协程唤醒
<!-- L17 --> UartAsyncDriver → ringbuf::HeapRb (rx_buf + tx_buf) → axpoll::PollSet (rx_wakers + tx_wakers)
<!-- L18 --> DeviceOps trait → Device wrapper → FileLike → poll/select/epoll

## 存疑问题

<!-- 添加时格式: <!-- L{编号} --> - {存疑问题} — {影响} — {需要确认对象} -->

<!-- L24 --> - Q1: QEMU virt 平台是否支持第二个 16550 UART？ — 决定是否需要独立硬件还是复用同一 UART — QEMU 文档/实验
<!-- tombstone: L25 --> Archived to archive.md §learned #L25 2026-05-25 — 已决策 (ADR-009)
<!-- L26 --> - Q3: 上板子时的 UART 型号？是否仍是 16550 兼容？ — AsyncUart trait 设计范围 — 老师
<!-- L27 --> - Q4: register_irq 和 register_irq_waker 同时注册同一 IRQ 时的语义？ — 是否需要统一中断分发机制 — 代码审查/实验
<!-- tombstone: L28 --> Archived to archive.md §learned #L28 2026-05-25 — 已解决
<!-- L29 --> - Q6: （已分析）N_TTY tty-reader 与 register_irq_waker 的配合方式 — 参见 reference-implementations.md — 代码追踪
<!-- L30 --> - Q7: 多核场景下 PLIC claim/complete 的竞态？ — 当前单核可忽略 — RISC-V PLIC 规范
<!-- L31 --> - Q8: embassy-sync 哪个版本与 nightly-2026-02-25 兼容？ — 依赖选型 — 实验验证
<!-- L32 --> - Q9: register_irq_waker 是 per-cpu 还是全局的？ — 多核影响 — 代码审查
<!-- L33 --> - Q10: axtask 的 spawn 是否支持 Future？还是只支持闭包？ — 异步任务创建方式 — 代码确认
<!-- L34 --> - Q11: PollSet 是否支持链式 Waker？一个事件唤醒多个等待者？ — poll/epoll 集成 — 代码审查
<!-- L35 --> - Q12: ringbuf::HeapRb 的 advance_read_index 是否需要 &mut？ — ISR 与 copier 分工 — ringbuf 文档
<!-- L36 --> - Q13: PollSet 容量 64 是否足够？ — 多路复用场景 — 使用场景分析
<!-- L37 --> - Q14: block_on 在内核任务上下文中是否可重入？ — 嵌套异步操作的安全性 — axtask 代码确认
<!-- L38 --> - Q15: 项目长期目标是否要替换 Console 底层为 AsyncUart？ — 远期架构方向 — 老师
<!-- L39 --> - Q16: 能否获得真实硬件（如 VisionFive2）进行验证？ — 报告的平台适配章节 — 老师
<!-- L40 --> - Q17: 报告中是否需要用户态测试程序的源码？ — 测试框架交付范围 — 老师
<!-- L41 --> - Q18: 性能基准的最低要求？QEMU 数据是否可接受？ — 性能量化可信度 — 老师
<!-- tombstone: L42 --> Archived to archive.md §learned #L42 2026-05-25 — 已决策 (ADR-009)
<!-- tombstone: L43 --> Archived to archive.md §learned #L43 2026-05-25 — 已确认 (ADR-009)
<!-- tombstone: L44 --> Archived to archive.md §learned #L44 2026-05-25 — 已决策 (ADR-007)

## 待探索

<!-- 添加时格式: <!-- L{编号} --> - {待探索项} -->

<!-- L21 --> - embassy-sync::Channel 是否有优于 ringbuf 的场景
<!-- L22 --> - axtask 协程优先级调度对延迟抖动的影响
<!-- L23 --> - Termios 行规则在 read_at/write_at 中的具体集成方式

---

## 关键代码路径（2026-05-25 补充）

<!-- L50 --> ### axplat-riscv64-qemu-virt（上游 crates.io，不可直接修改）
- `console.rs` — MmioSerialPort 初始化 + write_bytes/read_bytes/irq_num
- `irq.rs` — PLIC claim/complete + HandlerTable + set_enable/register/unregister
- `axconfig.toml` — UART_PADDR=0x10000000, UART_IRQ=0x0a, PLIC_PADDR=0x0c000000

<!-- L51 --> ### axhal（上游 crates.io，不可直接修改）
- `irq.rs` — register_irq_hook(全局唯一) + irq_handler(分发到 axplat + hook)

<!-- L52 --> ### axtask（上游 crates.io，不可直接修改）
- `future/mod.rs` — block_on 实现（AxWaker → unblock_task）
- `future/poll.rs` — poll_io 实现 + register_irq_waker 实现（POLL_IRQ BTreeMap → irq_hook → PollSet.wake）

<!-- L53 --> ### kernel（可修改）
- `file/pipe.rs` — 异步管道参考：Shared { buffer: Mutex<HeapRb>, poll_rx/poll_tx: PollSet }
- `file/event.rs` — EventFd 参考：AtomicU64 + PollSet
- `pseudofs/device.rs` — DeviceOps trait + Device 包装
- `pseudofs/dev/mod.rs` — builder() 注册 /dev 设备（添加新设备入口）
- `pseudofs/dev/tty/mod.rs` — Tty<R,W> 实现 DeviceOps + Pollable
- `pseudofs/dev/tty/ntty.rs` — Console + register_irq_waker 使用
- `pseudofs/dev/tty/terminal/ldisc.rs` — tty-reader copier: spawn_with_name + poll_fn 循环
- `entry.rs` — 内核入口：mount_all → spawn init → N_TTY.bind_to

<!-- L54 --> ### uart_16550 本地项目（可修改）
- `src/spec.rs` — bitflags: InterruptEnable, InterruptIdentification, LineStatus, FifoControl
- `src/backend/mmio.rs` — MmioBackend
- `src/lib.rs` — SerialPort<M>: set_interrupt_enable, interrupt_identification, try_send/try_receive, set_fifo_trigger_level, Config

<!-- L55 --> ### 中断完整路径
```
UART 硬件信号 → PLIC → S_EXT trap → axhal::irq_handler
  → axplat::handle (PLIC claim → HandlerTable.handle → PLIC complete)
  → IRQ_HOOK (register_irq_waker 注册的 irq_hook)
  → POLL_IRQ[irq].wake() → PollSet.wake()
  → axtask scheduler 唤醒等待任务
```

<!-- L56 --> ### Console 中断驱动模式（ntty.rs 参考）
Console (N_TTY) 已实现中断驱动：
- `ProcessMode::External` 使用 `register_irq_waker(irq, &waker)`
- tty-reader copier 任务：`spawn_with_name + poll_fn` 循环
- 这与 AsyncUart 设计的 ISR → AtomicWaker → copier 模型完全一致
- 参考：`kernel/src/pseudofs/dev/tty/ntty.rs:37-42` + `ldisc.rs:256-278`

<!-- L57 --> ### PTY 纯软件终端（pty.rs 参考）
PTY 不涉及硬件，使用 ringbuf 作为数据通道：
- master_to_slave + slave_to_master 两个 HeapRb<u8>
- PtyWriter 使用 `SpinNoPreempt<Prod<Buffer>>` + PollSet
- 与 AsyncUart 的 rx_buf/tx_buf + PollSet 设计一致
- 参考：`kernel/src/pseudofs/dev/tty/pty.rs`

<!-- L58 --> ### vsock ≠串口（virtio socket）
vsock 是 virtio socket 设备，用于虚拟机与主机通信：
- 不是传统串口，是 socket API (AF_VSOCK)
- 基于 virtqueue DMA 传输，性能远高于模拟 UART
- 可参考用于 M6 DMA 探索，但不属于 AsyncUart 设计范围
- 实现：`axnet-ng/src/vsock/` + `axdriver_virtio/src/socket.rs`

<!-- L59 --> ### 串口接口总结
StarryOS 中串口相关接口：
| 类型 | 硬件 |位置 | 用途 |
|------|------|------|------|
| Console | UART | ntty.rs | 系统控制台 |
| PTY | 无 | pty.rs | 终端模拟器 |
| vsock | virtio | axnet-ng/vsock | VM通信（非串口）|
