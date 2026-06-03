# architecture.md — 架构决策记录（汇总）

> 由 project-docs-assistant 维护，汇总分支整合。
> 条目格式: <!-- A{编号} --> ### {DATE} - {决策标题}
> 每条含决策、原因、影响、替代方案。
> ⚠️ ARCHIVED 标记表示已被后续决策取代或因失败而归档。

---

## 阶段一：基础设计（2026-05-24，两个方向共享）

<!-- A1 --> ### 2026-05-24 - 使用 axtask::future + embassy-sync::AtomicWaker，不引入完整 Embassy

- **决策**: 异步运行时基于 `axtask::future`（block_on + poll_io + register_irq_waker），仅引入 `embassy-sync::AtomicWaker` 用于 ISR 中安全唤醒
- **原因**: axtask 已有调度器，embassy-executor 会冲突；embassy-sync 无 OS 依赖可单独使用；Pipe/EventFd 已验证 axtask::future 模式
- **影响**: 不引入 embassy-executor/time，保持内核调度器独立性；需自己定义 AsyncUart trait
- **替代方案**:
  - ❌ 完整引入 Embassy (executor + HAL + sync) — 与 axtask 冲突
  - ❌ 仅用 embedded-io-async traits — 仍需自建 IRQ 绑定
  - ✅ axtask::future + AtomicWaker — 最小侵入，复用现有
- **风险**: 未来需要 Embassy HAL 高级特性（如 DMA 链）时需重新评估
- **状态**: ✅ 两个方向均采用

<!-- tombstone: A2 --> Archived to archive.md §architecture #A2 2026-05-31 — 独立硬件方案被 ADR-015 取代

<!-- A3 --> ### 2026-05-24 - VFS 接口——DeviceOps trait

- **决策**: 实现 `DeviceOps` trait，通过 `Device` wrapper 注册到 `/dev`
- **原因**: 所有现有 `/dev` 设备都通过 DeviceOps 注册；Device struct 自动处理转换链；as_pollable() 提供 poll/select/epoll 支持
- **影响**: 注册代码与 event/fb 等设备一致；offset 参数对串口无意义（流设备）可忽略
- **替代方案**:
  - ❌ 直接 impl FileLike — 需重复实现 fd 管理逻辑，破坏现有模式
  - ✅ DeviceOps — 与现有设备一致，复用转换链
- **状态**: ✅ 两个方向均采用

<!-- A4 --> ### 2026-05-24 - 缓冲策略——ringbuf::HeapRb + PollSet

- **决策**: 复用 `ringbuf::HeapRb<u8>` + `axpoll::PollSet`，每个方向各一个（rx_buf + rx_wakers / tx_buf + tx_wakers）
- **原因**: HeapRb 在 Pipe 中已验证；零额外依赖；Producer/Consumer 分离；默认 64 KiB 与 Pipe 一致
- **影响**: HeapRb 不是中断安全——硬件 FIFO 和内核 ringbuf 之间的搬运必须由单一后台协程完成；内存占用每端口 128 KiB
- **替代方案**:
  - ❌ 手写环形缓冲区 — 容易边界 bug，重复造轮子
  - ❌ embassy-sync::Channel — 额外依赖，每个元素需单独分配
  - ✅ ringbuf::HeapRb + PollSet — 已验证，零额外依赖
- **状态**: ✅ 两个方向均采用

<!-- A5 --> ### 2026-05-24 - termios 支持——可切换，默认 raw

- **决策**: 默认 raw 模式，termios 作为可选功能通过 ioctl 动态启用
- **原因**: 高性能数据通道需要 raw 字节流零开销；终端交互需要 termios 行规则；两者兼得
- **影响**: 默认路径零开销；termios 启用时复用现有 Termios 和 ldisc 逻辑；行规则处理在 read_at/write_at 中完成
- **替代方案**:
  - ❌ 始终 raw — 无法支持终端应用
  - ❌ 始终 termios — 所有数据路径都有行规则开销
  - ✅ 可切换默认 raw — 高性能与功能兼得
- **状态**: ✅ 设计保留，待实现

<!-- A6 --> ### 2026-05-24 - 硬件抽象——AsyncUart trait

- **决策**: 定义 `AsyncUart` trait，初期实现 `Uart16550`，为 `DwApbUart` 等预留实现位
- **原因**: StarryOS 支持四架构（RISC-V/LoongArch/AArch64/x86_64），初期均用 16550 但未来可能需要其他型号
- **影响**: AsyncUart 使用 axtask::future 异步语义；可在此基础上实现 embedded-io-async::Read/Write 获得生态兼容性
- **替代方案**:
  - ❌ 直接使用 embedded-io-async — 缺少 IRQ/FIFO 信息
  - ❌ 硬编码 16550 — 不支持其他硬件
  - ✅ 自定义 AsyncUart trait — 精确匹配需求，可扩展
- **状态**: ✅ 设计保留

---

## 阶段二：方向 A 渐进式集成（2026-05-25~05-28）

<!-- tombstone: A7 --> Archived to archive.md §architecture #A7 2026-05-31 — Console/AsyncUart 共存策略被 ADR-015 取代

<!-- tombstone: A11 --> Archived to archive.md §architecture #A11 2026-05-31 — QEMU 双串口策略不可用

<!-- A12 --> ### 2026-05-25 - DMA 远期策略——M0-M4 全中断驱动，DMA 归入 M6

- **决策**: DMA 探索归入远期 M6，M0-M4 全程基于中断驱动 + NAPI 批量轮询优化
- **原因**: QEMU virt 平台没有真正的 16550 DMA 通道；DMA 需要真板或 virtio-console 方案；中断驱动 + NAPI 可覆盖大部分性能需求
- **影响**: 高吞吐场景用 NAPI 替代 DMA；M4 性能优化聚焦中断驱动优化而非 DMA
- **状态**: ✅ 设计保留

<!-- A13 --> ### 2026-05-27 - axhal::console 外部 crate 约束——内核日志同步阻塞不可避免

- **决策**: 确认 `axhal::console` 是外部 crate（axplat-riscv64-qemu-virt），不可修改；内核启动日志的同步阻塞开销不可避免，但用户态 Console 输出可异步化
- **原因**: 外部 crate 层次：axruntime → axplat-riscv64-qemu-virt → axhal → axtask → axpoll（均来自 crates.io，不可修改）
- **影响**: 内核启动日志（axlog::init、ax_println!）始终走同步 polling TX 路径；用户态 Console 输出可通过 AsyncUart 异步化；性能优化重点在用户态路径
- **状态**: ✅ 关键约束，两个方向均适用

<!-- A14 --> ### 2026-05-27 - 渐进式开发策略——M1/M2 用 Console 验证，M3 替换 AsyncUart

- **决策**: M1/M2 底层暂时用 Console 同步引擎（read_bytes/write_bytes），验证上层架构（Ring Buffer + 中断 + copier 任务 + VFS 集成）正确后，M3 再替换为真正的 AsyncUart 异步引擎
- **原因**: 降低集成风险；保留调试能力；分步验证每一层
- **影响**: M1/M2 阶段 Console 和 AsyncUart 共享 UART 硬件（数据竞争已知，M3 解决）；M3 替换时需处理 IRQ waker 冲突
- **状态**: 方向 A 的策略

<!-- A15 --> ### 2026-05-27 - 渐进式开发策略——共用 UART0，渐进验证后替换

- **决策**: 放弃独立硬件方案（A2/A7/A11 均已归档），采用共用 UART0 策略——M1/M2 用 Console 验证架构，M3 替换为 AsyncUart
- **原因**: QEMU 第二串口需要补丁未合并；Console RX 已中断驱动（tty-reader），可复用验证架构；降低硬件依赖
- **影响**: M1/M2 阶段 Console 和 AsyncUart 共享 UART 硬件（数据竞争风险已知）；M3 替换时需解决 IRQ waker 冲突和 TX 数据竞争
- **状态**: 方向 A 的核心策略

---

## 阶段三：方向 A 失败经验（2026-05-27~05-28）

<!-- A16 --> ### 2026-05-27 - 软件路径分离方案失败（Console RX 禁用 + AsyncUart 独占）

- **决策**: 尝试通过条件编译禁用 Console RX（read() 返回 0），让 AsyncUart 独占 UART 硬件 RX 数据，Shell stdin 重定向到 `/dev/async_uart_test`
- **实施结果**: ❌ **失败** - Shell 启动后完全卡住，无法输入任何内容
- **根因分析**:
  - Console.read() 返回 0 → Shell stdin 无法接收输入（预期行为）
  - Shell stdin 改为 `/dev/async_uart_test` → AsyncUart RX buffer 空的 → 阻塞等待数据（未预期）
  - **根本问题**: AsyncUart RX copier 任务无法从 UART 硬件正确读取数据
- **影响**: 已回滚所有修改；ADR-015 软件路径分离方案暂时不可行
- **教训**: 需先验证 AsyncUart RX 能独立工作，再尝试分离

<!-- A17 --> ### 2026-05-27 - M3 替换尝试失败（IRQ 风暴 + TX busy-loop）

- **决策**: 尝试将 Console 底层替换为 AsyncUart（M3 核心任务）
- **实施结果**: ❌ **失败** - IRQ 风暴 + TX busy-loop
- **问题详情**:
  1. **IRQ 风暴**: RX-COPIER 和 tty-reader 快速循环唤醒，IRQ 10 异常触发
  2. **TX busy-loop**: TX FIFO 满，UART 状态异常（LSR=0x00，THR_EMPTY=false TEMT=false）
  3. **UART 硬件未正常发送数据**: FIFO 满后 retry 无效，LSR 状态不变化
- **根因（未完全明确）**:
  1. UART 硬件配置异常（Console 初始化后的状态不兼容 AsyncUart）
  2. 未验证 UART 状态（IIR、MCR、LSR）就开始集成
  3. THR_EMPTY 状态异常（可能 UART TX 被禁用或硬件卡住）
- **教训**:
  1. ❌ 未验证硬件状态就开始集成（假设 Console 初始化后的 UART 状态正常）
  2. ❌ 未添加足够的调试信息（IIR、MCR、完整 LSR 状态）
  3. ❌ 战略转向过于激进（未充分验证可行性）
- **影响**: 回滚到 M3 Task 5，重新评估整体方案

<!-- tombstone: A18-A19 --> Archived to archive.md §architecture #A18-A19 2026-05-31 — 方向 A 战略转向失败，被 stride=4 根因发现纠正

---

## 阶段四：方向 B 完全剔除 Console（2026-05-28~05-29）

<!-- A20 --> ### 2026-05-28 - 分支策略变更：完全剔除 Console（feat/uart-async-dev2）

- **背景**: feat/uart-async 分支尝试渐进式集成方案（复用 Console UART 初始化），M3 替换失败
- **决策**: 创建新分支 feat/uart-async-dev2，完全剔除 Console，从零开始实现
- **策略变更**: 从"渐进式集成"改为"完全替代 Console"
- **目标**: 使用本地 uart_16550 crate + 自实现 UART 初始化，不依赖 axplat
- **原因**:
  - ✅ 避免 Console 与 AsyncUart 的数据竞争（TX 同时写 THR）
  - ✅ 避免 IRQ waker 冲突（tty-reader 与 AsyncUart copier）
  - ✅ 避免 UART 重初始化冲突（Console 初始化状态不明确）
  - ✅ 完全控制 UART 硬件配置（从零开始，状态明确）
- **影响**: 新 Milestone 规划：P0（规划）→ P1（UART 初始化替代）→ P2（异步架构）→ P3（Console 剔除）→ P4（VFS 集成）→ P5（性能优化）→ P6（真板验证）
- **替代方案（已评估）**:
  - ❌ 方案 A：添加 UART 状态调试 + 修复 IER — 未验证可行性
  - ❌ 方案 B：AsyncUart 重新初始化 UART — 可能破坏 Console RX
  - ❌ 方案 C：纯 polling TX — 性能不如预期
  - ❌ 方案 D：Console 和 AsyncUart 共存 — 数据竞争风险
  - ✅ 方案 E（新分支）：完全剔除 Console — 从零开始，避免冲突

<!-- A21 --> ### 2026-05-28 - 完全剔除 Console 方案的四个关键架构决策

- **决策 1：UART 硬件初始化替代方案**
  - 使用 uart_16550 crate 本地初始化（替代 axplat UART init）
  - 关键配置：IER::DATA_READY | IER::THR_EMPTY（Console 只使能 RX，AsyncUart 必须使能 TX）
  - 初始化时机：kernel entry.rs 早期调用，覆盖 axplat 配置

- **决策 2：earlycon 内核日志方案**
  - 复用 axhal::console（已有 polling TX 实现，无需额外开发）
  - 可用时机：axruntime::init_early 后立即可用（比 AsyncUart 早 10-20 ms）
  - UART 共存策略：AtomicBool 标记 + 自旋锁保护（AsyncUart 运行时禁用 earlycon）

- **决策 3：AsyncUart 设备注册架构**
  - VFS 集成路径：DeviceOps trait → Device wrapper → File → FD_TABLE → 用户态 API
  - 设备节点：/dev/async_uart（DeviceId::new(4, 64））
  - 异步支持：Pollable trait 实现（poll() + register()）支持 poll/select/epoll

- **决策 4：IRQ waker 分发机制**
  - ISR 分发架构：uart_isr_handler 读 ISR 寄存器判断 InterruptType，唤醒 rx_waker/tx_waker
  - 多 waker 支持：register_irq_waker 使用 BTreeMap<usize, PollSet>，同一 IRQ 可共存多个 waker
  - ISR 安全约束：禁用中断防止重入（IER 操作）+ AtomicWaker ISR 安全唤醒

<!-- tombstone: A22-A23 --> Archived to archive.md §architecture #A22-A23 2026-05-31 — MMIO 权限诊断有误，被 ADR-026 stride=4 根因纠正

---

## 阶段五：基于新发现的重新出发（2026-05-31）

<!-- A24 --> ### 2026-05-31 - MMIO 权限重新分析：此前结论有误，UART 在最终页表中正确映射

- **背景**: ADR-022/023 认为 UART MMIO 权限被 axplat 限制，导致方向 B P1/P2 阻塞。2026-05-31 经深入代码阅读验证后发现此结论有误。
- **验证结果**:
  1. `axconfig.toml` 的 `mmio-ranges` 明确包含 `[0x1000_0000, 0x1000]`（UART）
  2. `axplat::mem::mmio_ranges()` → `axhal::mem::memory_regions()` 将 UART MMIO 包含在内
  3. `axmm::init_memory_management()` → `new_kernel_aspace()` → `map_linear(phys_to_virt(0x10000000), 0x10000000, 0x1000, READ|WRITE|DEVICE)` 将 UART 正确映射
  4. Console 的 `MmioSerialPort` 访问 `0xffffffc010000000` 能正常工作恰好证明了映射有效——与"初始化时机"无关
- **纠正结论**: 此前方向 B P1/P2 的 LoadFault/StoreFault **不是页表权限问题**，是测试代码 bug（地址计算、stride 匹配、或实现错误）
- **影响**:
  - ✅ 移除"必须修改 axplat"的阻塞条件
  - ✅ 异步串口可在 kernel 层独立实现，不改任何外部 crate
  - ✅ `axmm::iomap()` 可作为安全保障（已存在 API，调用即可确保权限）
- **状态**: ✅ 归档 ADR-022/023 的结论为已纠正

<!-- A25 --> ### 2026-05-31 - 基于 dev2 分支重新出发：Q0→Q5 新规划

- **背景**: MMIO 权限分析纠正后，方向 B 的 P1/P2 阻塞解除。基于 dev2 分支重新出发，使用 axmm::iomap() 作为安全网。
- **决策**:
  1. **Spike 优先（Q0）**: 先在 entry.rs 调用 `axmm::iomap(PhysAddr::from(0x10000000), 0x1000)` 验证 UART 寄存器可读写，再推进后续
  2. **不改外部 crate**: 所有实现均在 `kernel/src/drivers/` 下，~320 行新代码
  3. **复用方向 A 已验证架构**: Ring Buffer + ISR → AtomicWaker → copier 任务 + DeviceOps + VFS 集成
  4. **方向 A 教训吸取**: M3 失败根因是 Console UART 状态不兼容（IER 冲突 + TX busy-loop），需在 Q1 完成前验证 UART 配置
- **新 Milestone**: Q0（Spike）→ Q1（driver 架构）→ Q2（VFS 集成）→ Q3（Console 共存/替换）→ Q4（性能优化）→ Q5（真板验证）
- **替代方案**: 无。此方案是当前唯一不修改外部 crate 的可行路径。
- **状态**: ✅ Q0 完成（2026-05-31）

<!-- A26 --> ### 2026-05-31 - LoadFault 根因确认：stride=4 错误，非页表权限问题

- **背景**: Q0 Spike 中先调用 `axmm::iomap()` 成功，但 `uart.isr()` 仍触发 LoadFault。对比 Console（stride=1，正常工作）和我们的代码（stride=4，LoadFault），发现根因。
- **根因**: NS16550 寄存器空间仅 0x00-0x07 共 8 字节。`UART_STRIDE=4` 下 ISR（register offset 2 × stride 4 = 8）读写到 `base+8`，超出 UART 寄存器范围。QEMU NS16550 设备只响应 0x00-0x07 范围内的访问，越界访问产生总线错误，RISC-V CPU 将其解释为 LoadFault。
- **验证**: stride=1 下 raw pointer 读 LSR（base+5）→ `0x60` ✅。同时 stride=4 的 base+8 → LoadFault ❌。同一 4K 页表映射内两个地址不同结果，排除了页表问题。
- **影响**: 方向 A M3 和方向 B P1/P2 的"MMIO 权限阻塞"诊断全部有误。真正阻塞原因：
  - 方向 A M3: stride=4 + Console UART 状态不兼容（IER 冲突 + TX busy-loop）
  - 方向 B P1/P2: stride=4 导致 LoadFault
- **校正**: ADR-022/023 的"页表权限"结论作废。stride=1 后全部测试通过：
  - ✅ uart_16550 crate 读写 IER/ISR/LSR
  - ✅ ISR handler 正常执行（读 ISR 寄存器 + drain RX FIFO）
  - ✅ Console/Shell 正常运行
  - ✅ 无 IRQ 风暴
- **状态**: ✅ 阻塞解除，方向确定

<!-- A27 --> ### 2026-05-31 - 统一方向：kernel 层独立实现异步串口

- **背景**: 经两个方向探索（A: 渐进式集成, B: 完全剔除 Console）+ 根因发现（stride=4），方向已明确。
- **决策**: 在 dev2 分支，kernel 层独立实现完整异步串口栈，不修改任何外部 crate。
- **核心策略**:
  1. UART 维护一个 `SpinNoIrq<Uart16550<MmioBackend>>` 实例（stride=1）
  2. ISR → AtomicWaker → copier 任务模型（复用方向 A M1/M2 验证过的架构）
  3. RX/TX copier 使用 poll_fn + register_irq_waker 模式（参考 Pipe/EventFd）
  4. VFS 集成使用 DeviceOps + Pollable trait（参考方向 A M2 经验）
  5. Console 共存：earlycon polling TX 用于内核日志，AsyncUart 用于用户态 Shell
- **不再需要**: ~~修改 axplat~~、~~页表权限修复~~、~~方案 A/B/C 三选一~~
<!-- A28 --> ### 2026-05-31 - copier/Console FIFO 竞争发现：Q2 共存策略

- **背景**: Q2 阶段同时运行 Console 和 AsyncUart copier 时，Shell 无法接收键盘输入
- **根因**: RX copier 的 `try_receive_byte()` 和 Console tty-reader 的 `read_bytes()` 都读同一个 UART RBR 寄存器。copier 先启动、先执行，抢在 tty-reader 之前把 FIFO 数据全部读走放入 ring buffer，tty-reader 看到空 FIFO，Shell 收不到输入
- **解决**: Q2 阶段关闭 copier，Console 独占 UART。Q3 替换 Console 后再启用 copier，届时 copier 是唯一 UART 读写者
- **影响**: Q2 的 /dev/async_uart 只提供设备节点和 DeviceOps 基础架构（read/write 在 ring buffer 上操作），实际数据通路（UART ↔ ring buffer）由 Q3 启用
- **状态**: ✅ Q2 共存验证通过

<!-- A29 --> ### 2026-05-31 - Q4 全异步 TX：TX copier 接管 UART 发送

- **背景**: Q3 实现了 RX 异步但 TX 仍用 Console polling。Q4 将 TX 也切换到异步。
- **实现**:
  1. `AsyncUartWriter` 实现 `TtyWrite`——写入 ring buffer
  2. TX copier 从 ring buffer 读取，写入 UART THR
  3. 若 FIFO 满且 buffer 有剩余数据 → `enable_tx_intr()` → ISR 在 THR_EMPTY 时唤醒
  4. ISR 中 `disable_tx_intr()` + `TX_WAKER.wake()` → copier 继续发送
  5. `Tty<AsyncUartReader, AsyncUartWriter>` 注册为 `/dev/console`
- **内核日志共存**: `ax_println!` 仍走 `axhal::console::write_bytes()`（Console polling TX），与 TX copier 共享 UART THR，互不冲突
- **状态**: ✅ Q4 完成

<!-- A30 --> ### 2026-06-01 - Console 与 Async 共存架构决策

- **背景**: 异步串口（Async）已完成，性能测试显示 Async 在 CPU 效率、延迟等方面优于 Console。是否可以完全剔除 Console，让 Async 也负责内核日志输出？
- **决策**: 保持 Console 与 Async 共存，各司其职
- **原因**:
  1. **ax_println! 依赖 Console**：ax_println! 调用 LogIf::console_write_str → axhal::console::write_bytes，这是外部 crate（axplat-riscv64-qemu-virt），无法修改
  2. **早期启动需要 Console**：ax_println! 在内核启动早期就使用，Async 驱动在稍后才初始化
  3. **panic 处理需要 Console**：panic handler 使用 ax_println!，需要可靠的输出方式
  4. **当前方案工作正常**：Console 和 Async 共享 UART THR，互不冲突
- **影响**:
  - ✅ Console 负责：内核日志（ax_println!）、早期启动日志、panic 处理
  - ✅ Async 负责：Shell I/O、用户态程序、高性能数据传输
  - ✅ 两者共存，各取所长
- **替代方案**:
  - ❌ 完全剔除 Console — ax_println! 依赖外部 crate，无法修改
  - ❌ 修改 ax_println! 实现 — 需要修改外部 crate，早期启动时 Async 未初始化
  - ✅ 保持共存 — 简单可靠，工作正常
- **状态**: ✅ 当前方案

<!-- A31 --> ### 2026-06-01 - RX 测试方法决策

- **背景**: Async 异步串口的 RX 测试可以在内核态直接测试 Ring Buffer，但 Console 阻塞串口无法测试 RX。
- **决策**: Async 使用内核态 RX 测试，Console 跳过 RX 测试
- **原因**:
  1. **Async 有 Ring Buffer**：可以存储大量数据（64 KB），支持大数据量测试
  2. **Console 没有 Ring Buffer**：read_bytes() 是非阻塞的（try_receive），没有数据立即返回 0
  3. **FIFO 无法直接测试**：容量小（16 字节）、非阻塞读取、需要外部数据注入、与 Shell 竞争
  4. **用户态 RX 都无法测试**：TTY 层回显导致 Shell 抢先读取数据
- **影响**:
  - ✅ Async RX 测试：Ring Buffer 读取 588,776 KB/s，延迟 P50 600 ns
  - ❌ Console RX 测试：跳过（无 Ring Buffer，非阻塞读取）
  - ✅ 用户态 RX 测试：跳过（TTY 竞争条件）
- **状态**: ✅ 当前方案
