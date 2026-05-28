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
<!-- L63 --> | register_irq_waker 共存机制 | BTreeMap<usize, PollSet> 支持同一 IRQ 注册多个 waker | 2026-05-27 |
<!-- L65 --> | RISC-V musl 工具链路径 | /opt/musl/riscv64-linux-musl-cross/bin | 编译 lwext4_rust C 代码 | 2026-05-27 |
<!-- L66 --> | rootfs 下载地址 | https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz | 1GB 磁盘镜像 | 2026-05-27 |
<!-- L67 --> | disk.img 位置 | 项目根目录 + make/disk.img | make run 需要后者 | 2026-05-27 |

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

<!-- L64 --> ### register_irq_waker 共存机制详解
- 场景: Console tty-reader 已使用 IRQ 10，AsyncUart 是否能共用？
- 分析路径: axtask::future::poll.rs → axhal::irq.rs → kernel/pseudofs/dev/tty/ntty.rs
- 结论:
  1. register_irq_waker 内部使用 BTreeMap<usize, PollSet> 存储每个 IRQ 的唤醒器集合
  2. PollSet 支持注册多个 Waker，同一 IRQ 号可共存
  3. register_irq_hook 全局唯一，但 hook 函数根据 IRQ 号查找对应 PollSet
  4. Console 和 AsyncUart 共用 IRQ 10，共存支持
- 源码关键点:
  - Entry::Vacant → register_irq_hook + set_enable + PollSet::new()
  - Entry::Occupied → 获取已有 PollSet
  - PollSet.register(waker) 总是执行（支持多个 waker）
- 验证: 2026-05-27 (T0.3)

<!-- L78 --> ### M3 替换失败 — IRQ 风暴 + TX busy-loop（2026-05-28）
- **症状**:
  1. IRQ 风暴：RX-COPIER 和 tty-reader 快速循环唤醒，`[RX-COPIER] poll` → `[RX-COPIER] returning Pending` → 立即又被唤醒
  2. TX busy-loop：TX FIFO 满，UART 状态异常（LSR=0x00，THR_EMPTY=false TEMT=false）
  3. UART 硬件未正常发送数据：FIFO 满后 retry 无效，LSR 状态不变化
- **根因（未完全明确）**:
  1. UART 硬件配置异常（Console 初始化后的状态不兼容 AsyncUart）
  2. 未验证 UART 状态（IIR、MCR、LSR）就开始集成
  3. THR_EMPTY 状态异常（可能 UART TX 被禁用或硬件卡住）
- **教训**:
  1. ❌ 未验证硬件状态就开始集成（假设 Console 初始化后的 UART 状态正常）
  2. ❌ 未添加足够的调试信息（IIR、MCR、完整 LSR 状态）
  3. ❌ ADR-018 战略转向过于激进（未充分验证可行性）
- **解决**: 回滚到 M3 Task 5（AsyncUart 驱动实现，未集成），重新评估整体方案
- **验证**: 2026-05-28（ADR-019）

<!-- L79 --> ### UART 状态调试缺失教训（2026-05-28）
- **问题**: M3 替换失败时，缺少全面的 UART 硬件状态调试
- **缺失信息**:
  1. IIR 寄存器（Interrupt Identification）— 无法确认 interrupt 类型
  2. MCR 寄存器（Modem Control）— 无法确认 TX 是否被禁用
  3. LSR 完整值（仅输出 THR_EMPTY/TEMT，未输出错误标志）
- **后果**: 无法诊断 UART 硬件为什么卡住，只能猜测根因
- **教训**: 硬件集成前，必须添加全面的寄存器状态调试（IIR/MCR/LSR/IIR）
- **预防**: 下次集成前，先添加 UART 状态诊断代码

<!-- L80 --> ### THR_EMPTY 状态理解错误（2026-05-28）
- **问题**: uart_16550 crate 的 THR_EMPTY 注释说"FIFO completely empty"
- **实际**: THR_EMPTY (Bit 5) 表示 THR 有空位（可以写入），TEMT (Bit 6) 表示完全空闲
- **误解影响**: 以为 THR_EMPTY=false 表示 FIFO 有至少 1 个字节，实际表示 FIFO 满
- **纠正**: THR_EMPTY=1 表示 FIFO 有空位，THR_EMPTY=0 表示 FIFO 满
- **教训**: 需仔细阅读 UART 规范，不要依赖库的注释（可能有错误）

<!-- L68 --> ### 构建环境配置踩坑
- 症状: make build 失败，`riscv64-linux-musl-cc: command not found`
- 根因: lwext4_rust crate 需要编译 C 代码，依赖 musl 交叉编译工具链
- 解:
  1. 工具链位于 `/opt/musl/riscv64-linux-musl-cross/bin`
  2. 构建前设置 `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH`
  3. 系统已有其他 RISC-V 工具链（riscv64-linux-gnu-gcc, riscv64-unknown-elf-gcc），但 musl 版本是必需的
- 验证: 2026-05-27 (T0.4)

<!-- L69 --> ### rootfs 下载与部署踩坑
- 症状: make rootfs 下载失败（SSL 连接中断），disk.img not found
- 根因: GitHub releases 下载不稳定 + Makefile 需要 disk.img 在 make/ 目录
- 解:
  1. 手动下载 `https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz`
  2. 解压 `xz -d rootfs-riscv64.img.xz`
  3. 复制到两处：`cp rootfs-riscv64.img disk.img && cp disk.img make/disk.img`
  4. Makefile 会自动复制 rootfs-riscv64.img → make/disk.img（如果 rootfs 下载成功）
- 验证: 2026-05-27 (T0.4)

<!-- L70 --> ### 构建警告清理经验
- 症状: 编译有10个 unused warnings（dead_code）
- 分析: 这些是项目原有代码的未使用函数，不是我们添加依赖导致
- 影响: 不影响功能，编译成功
- 建议: 不清理（遵循"只改必须改的代码"原则，避免引入不必要变更）
- 验证: 2026-05-27 (T0.4)

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

<!-- L24 --> - Q1: QEMU virt 平台是否支持第二个 16550 UART？ — **已解决**：标准 QEMU 不支持（需补丁未合并），决策共用 UART0。参见 ADR-013、ADR-014、ADR-015。
<!-- tombstone: L25 --> Archived to archive.md §learned #L25 2026-05-25 — 已决策 (ADR-009)
<!-- L26 --> - Q3: 上板子时的 UART 型号？是否仍是 16550 兼容？ — AsyncUart trait 设计范围 — 老师
<!-- L27 --> - Q4: register_irq 和 register_irq_waker 同时注册同一 IRQ 时的语义？ — **已解决**: register_irq_waker 支持同一 IRQ 注册多个 waker（BTreeMap<usize, PollSet>），与 Console tty-reader 共存 ✅ 2026-05-27
<!-- tombstone: L28 --> Archived to archive.md §learned #L28 2026-05-25 — 已解决
<!-- L29 --> - Q6: （已分析）N_TTY tty-reader 与 register_irq_waker 的配合方式 — 参见 reference-implementations.md — 代码追踪
<!-- L30 --> - Q7: 多核场景下 PLIC claim/complete 的竞态？ — 当前单核可忽略 — RISC-V PLIC 规范
<!-- L31 --> - Q8: embassy-sync 哪个版本与 nightly-2026-02-25 兼容？ — 依赖选型 — **已解决**: embassy-sync v0.6.2 与 nightly-2026-02-25 兼容，cargo check 通过 ✅ 2026-05-27
<!-- L32 --> - Q9: register_irq_waker 是 per-cpu 还是全局的？ — **已解决**: 全局（static POLL_IRQ: SpinNoIrq<BTreeMap<usize, PollSet>>），多核场景需考虑锁竞争 ✅ 2026-05-27
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

<!-- L60 --> ### 外部 crate 层次结构（不可修改）
StarryOS 依赖的外部 crate（来自 crates.io）：
```
axruntime (启动框架)
  ↓
axplat-riscv64-qemu-virt (平台实现)
  ├─ console.rs: MmioSerialPort + write_bytes (同步阻塞!)
  ├─ irq.rs: PLIC + HandlerTable
  └─ axconfig.toml: UART_PADDR, UART_IRQ
  ↓
axhal (硬件抽象层)
  └─ 导出 axplat::console::* (不可修改!)
  ↓
axtask (任务调度)
  ├─ future/mod.rs: block_on
  ├─ future/poll.rs: poll_io + register_irq_waker
  └─ scheduler
  ↓
kernel (本地项目，可修改)
  └─ pseudofs/dev/tty/ntty.rs: Console, N_TTY
```
**关键约束**: axhal::console 是外部 crate，无法修改其同步阻塞实现。

<!-- L61 --> ### 内核日志与用户态 Console 的软件路径分离
M3 Console 统一后的两条输出路径：
```
路径 A: 内核日志（同步阻塞，不可避免）
  axlog::info!
    → axhal::console::write_bytes (外部 crate)
    → MmioSerialPort::write_bytes (忙等待)
    → UART THR

路径 B: 用户态 Console（异步，性能优化）
  用户态 write("/dev/console")
    → N_TTY.write_at (DeviceOps)
    → Console.write (TtyWrite 替换实现)
    → AsyncUart tx_buf
    → TX copier 任务
    → UART THR
```
**共用同一硬件**: 两条路径写入同一 UART THR，但软件路径独立。
**内核日志始终可用**: 不依赖异步框架是否正常工作。

<!-- L62 --> ### earlycon 调试安全机制
axhal::console 作为"earlycon"提供调试安全保障：
- **独立于异步框架**: axhal::console 直接 MMIO 操作，不依赖 axtask/AsyncUart
- **始终可用**: AsyncUart copier 任务卡死、缓冲区溢出时，内核日志仍能输出
- **panic 信息可靠**: 系统崩溃时仍有输出渠道
- **启动早期可用**: axruntime::init 阶段（异步框架未初始化）就可输出
- **不需要额外实现**: axhal::console 本身就是 earlycon，始终存在
参考: Linux kernel earlycon + 正常 console 的分离模式

<!-- L71 --> ### 构建命令速查
```bash
# 设置环境（每次构建前执行）
export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH

# 编译内核
make build

# 运行内核（需要 disk.img）
make run

# 生成 rootfs（或手动下载）
make rootfs

# QEMU 退出: Ctrl+A 然后 X
```

<!-- L72 --> ### M0 Gate 验证要点
M0 验证内容：
1. `cargo check` 编译通过 → 依赖正确
2. `make build` 编译通过 → 工具链正确
3. `make run` 内核启动 → rootfs 正确
4. 看到 shell 可交互 → Console 正常工作
关键：每一步都可能遇到环境问题，需要逐层排查

<!-- L73 --> ### uart_16550 版本共存说明
Cargo.lock 中存在两个 uart_16550 版本：
- **uart_16550 v0.4.0**：来自 crates.io，被 axplat-riscv64-qemu-virt 等上游 crate 使用
- **uart_16550 v0.6.0**：来自本地 path，被 starry-kernel 使用
原因：
  - axplat-riscv64-qemu-virt（上游 crate）依赖 crates.io 的 v0.4.0，用于其 console 实现
  - kernel（我们的项目）依赖本地最新 v0.6.0，用于 AsyncUart（新增中断控制 API）
  - Cargo 允许不同 crate 使用不同版本的同一依赖
影响：
  - 两者互不影响，各自使用各自版本的 API
  - axplat::console 用 v0.4.0，AsyncUart 用 v0.6.0
统一方案（远期）：
  - 将 uart_16550 v0.6.0 发布到 crates.io
  - 提 PR 给 axplat 项目升级依赖
验证: 2026-05-27 (M0 Gate)

<!-- L74 --> ### M1 Console 共用数据竞争（预期行为）
- 症状: `cat /dev/async_uart_test` 输入数据后，shell 也收到部分数据并尝试执行命令
- 根因: M1 测试设备 `async_uart_test` 和 `/dev/console` 共用 `axhal::console::read_bytes()`，两者竞争读取
- 表现:
  - `echo "hello" > /dev/async_uart_test` → Console 输出 "hello" ✅
  - `cat /dev/async_uart_test` 输入 "world" → cat 显示 "world" ✅
  - shell 同时收到部分数据 → 执行 `world` 命令报错 ⚠️
- 设计决策: ADR-013/014 已记录，M1 用 Console 验证架构，接受共用竞争
- 解决: M3 替换底层为 AsyncUart，路径分离
- 验证: 2026-05-27 (M1 Gate)

<!-- L75 --> ### 非阻塞模式测试延后到 M3/M4（2026-05-27）
- **现象**: M2 只测阻塞模式，不测非阻塞 WouldBlock 场景
- **原因**: 简化 M2 范围，快速验证核心 VFS 集成功能；非阻塞涉及 ioctl 实现，增加复杂度
- **影响**: M2 Gate 验证不覆盖非阻塞场景，但阻塞模式已验证核心流程
- **决策位置**: architecture.md ADR-016, optimization.md O03-O04
- **何时解决**: M3/M4 补充非阻塞测试
- **验证**: M2 内核测试通过 ✅

<!-- L76 --> ### epoll 测试延后到 M3/M4（2026-05-27）
- **现象**: M2 只测 poll，不测 epoll
- **原因**: poll 是 Pollable trait 的直接体现，验证核心实现；epoll 需多 fd 才体现优势，M2 只有单设备
- **影响**: M2 Gate 验证不覆盖 epoll，但 poll 成功 → as_pollable() 正确 → epoll 自然工作
- **决策位置**: architecture.md ADR-016, optimization.md O03-O04
- **何时解决**: M3/M4 补充 epoll 测试（多设备场景）
- **验证**: M2 内核测试通过 ✅

<!-- L77 --> ### M2 测试程序技术选择：内核内部测试（2026-05-27）
- **技术**: 内核内部测试代码（`kernel/src/drivers/serial/test.rs`），启动时自动执行
- **原因**:
  - 用户态测试部署麻烦（ABI 兼容性、rootfs 挂载、手动操作）
  - 内核测试更简单、自动化、无部署复杂度
  - 直接使用内核 API（DeviceOps trait），无需用户态 syscall
- **测试内容**: DeviceOps trait (write_at) + Pollable trait (poll) + TX 路径验证
- **验证**: M2 所有自动化测试通过 ✅
- **分支策略**: feat/uart-async-m2（验证分支）→ feat/uart-async(m1)（主开发分支，记录结果）
- **适用场景**: 内核功能验证，无需用户态交互
