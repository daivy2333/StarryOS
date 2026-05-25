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

## 依赖关系图

<!-- 添加时格式: <!-- L{编号} --> 关键依赖间的调用/依赖关系 -->

<!-- L16 --> axtask::future ← register_irq_waker ← PLIC ISR → AtomicWaker::wake → 协程唤醒
<!-- L17 --> UartAsyncDriver → ringbuf::HeapRb (rx_buf + tx_buf) → axpoll::PollSet (rx_wakers + tx_wakers)
<!-- L18 --> DeviceOps trait → Device wrapper → FileLike → poll/select/epoll

## 存疑问题

<!-- 添加时格式: <!-- L{编号} --> - {存疑问题} — {影响} — {需要确认对象} -->

<!-- L24 --> - Q1: QEMU virt 平台是否支持第二个 16550 UART？ — 决定是否需要独立硬件还是复用同一 UART — QEMU 文档/实验
<!-- L25 --> - Q2: 修改 axplat/axhal crate 的方式？~~fork 还是提 PR？~~ **已决策：不修改，内核直接用本地最新 uart_16550 crate** — ADR-009
<!-- L26 --> - Q3: 上板子时的 UART 型号？是否仍是 16550 兼容？ — AsyncUart trait 设计范围 — 老师
<!-- L27 --> - Q4: register_irq 和 register_irq_waker 同时注册同一 IRQ 时的语义？ — 是否需要统一中断分发机制 — 代码审查/实验
<!-- L28 --> - Q5: trap 上下文中读 MMIO 是否安全？~~是否有内存序问题？~~ **已解决：uart_16550 crate 封装了 volatile read，ISR 中安全**
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
<!-- L42 --> - Q19: 本地 uart_16550 crate 是否可发布到 crates.io？ — **已决策：使用本地 path 依赖，暂不发布** — ADR-009
<!-- L43 --> - Q20: StarryOS 的 uart_16550 v0.4.0 和本地版本是否有 API 兼容性？ — **已确认：本地 v0.6.0 完整覆盖 v0.4.0 API，额外增加中断控制** — ADR-009
<!-- L44 --> - Q21: Console 和 AsyncUart 同时操作同一 UART 的协调方案？ — **已决策：先独立后统一（方案 C），QEMU 配第二个 -serial** — ADR-007

## 待探索

<!-- 添加时格式: <!-- L{编号} --> - {待探索项} -->

<!-- L21 --> - embassy-sync::Channel 是否有优于 ringbuf 的场景
<!-- L22 --> - axtask 协程优先级调度对延迟抖动的影响
<!-- L23 --> - Termios 行规则在 read_at/write_at 中的具体集成方式
