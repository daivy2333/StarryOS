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

<!-- A2 --> ### 2026-05-24 - 串口与控制台关系——独立硬件 /dev/ttyS0 ⚠️ ARCHIVED

> **已归档** (2026-05-27) — 被 ADR-015 取代。决策：共用 UART0，不再使用独立硬件。

- **决策**: 新增独立硬件串口（QEMU `-serial` 多路配置），注册为 `/dev/ttyS0`，不影响现有 `/dev/console`
- **原因**: 隔离风险，独立开发测试；初期不破坏控制台稳定性
- **影响**: 需在 QEMU 启动参数添加第二个 `-serial`；`/dev/console` 和 `/dev/ttyS0` 是两个独立设备
- **替代方案**:
  - ❌ A: 复用同一硬件替换 Console — 可能破坏控制台稳定性
  - ✅ B: 独立硬件独立设备 — 隔离风险
  - ⚠️ C: 替换 axhal::console 底层 — 远期选项，初期影响面太大
- **归档原因**: QEMU 第二串口需要补丁未合并，决策共用 UART0

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

<!-- A7 --> ### 2026-05-25 - Console与AsyncUart共存策略——先独立后统一 ⚠️ ARCHIVED

> **已归档** (2026-05-27) — 被 ADR-015 取代。决策：共用 UART0，渐进式验证后替换。

- **决策**: 采用方案 C"先独立后统一"——AsyncUart 作为独立 `/dev/ttyS0` 设备，Console 保持不变；远期统一
- **原因**: 初期隔离风险，不破坏控制台稳定性；AsyncUart 可独立开发测试；远期框架成熟后可将 axhal::console 底层替换为 AsyncUart 实现
- **影响**: QEMU 需添加第二个 `-serial` 参数；Console 和 AsyncUart 操作不同硬件实例，无竞态
- **归档原因**: QEMU 第二串口不可用，决策共用 UART0

<!-- A8 --> ### 2026-05-25 - 中断分发架构——ISR → AtomicWaker → copier 任务

- **决策**: 采用模型 1"ISR → AtomicWaker → copier 任务"——ISR 极简原则，数据搬运推迟到任务上下文
- **原因**: ISR 中不能持有 Mutex 或做阻塞操作；HeapRb 的 Producer/Consumer 不是中断安全；copier 任务在任务上下文中可安全获取 Mutex；AtomicWaker 是 ISR 安全的（无锁，原子操作）。此模型源自 Embassy 中断→异步配合流程：外设完成→中断→HAL 路由信号→执行器通知→任务继续，我们将"HAL 路由"替换为 AtomicWaker::wake()，"执行器"替换为 axtask::future
- **影响**: ISR 只做三件事：读 IIR → 禁用已触发中断 → AtomicWaker.wake()；copier 任务是唯一操作硬件 FIFO 和 ringbuf 的角色，天然无竞态；需一个 RX copier 任务和一个 TX copier 任务
- **替代方案**:
  - ❌ 模型 2: ISR 直接写 ringbuf — HeapRb 非中断安全，数据竞争风险
  - ❌ 模型 3: 轮询模式 — CPU 空转，高延迟，不符合"高性能"目标
  - ✅ 模型 1: ISR → AtomicWaker → copier — ISR 极简，数据安全，已验证模式
- **参考**: Pipe 的 block_on(poll_io(...)) 模式、N_TTY 的 tty-reader copier 模式
- **状态**: ✅ 设计保留（方向 B 也采用）

<!-- A9 --> ### 2026-05-25 - uart_16550 依赖策略——本地最新版 path 依赖

- **决策**: 使用本地 `/home/daivy/projects/uart_16550` 最新版（v0.6.0），通过 `path = "../../uart_16550"` 依赖
- **原因**: 本地 v0.6.0 完整覆盖所有中断控制 API（set_interrupt_enable、interrupt_identification、InterruptType 枚举、try_send/try_receive、FifoTriggerLevel、Config）；不需要修改 axplat/axhal；便于后续直接对 uart_16550 crate 升级优化
- **影响**: kernel/Cargo.toml 添加 path 依赖；与 axhal 中已有的 uart_16550 v0.4.0 可能版本冲突，需处理；发布时需解决路径依赖
- **替代方案**:
  - ❌ A: 直接依赖 crates.io v0.4.0 — 缺少中断控制 API，需自己封装
  - ❌ B: 发布本地 crate 到 crates.io 后统一升级 — 发布流程耗时
  - ✅ 本地最新版 path 依赖 — 立即可用，便于直接升级优化
- **关键发现**: 之前分析中认为"缺失"的 API 全部已在本地 v0.6.0 中提供
- **状态**: ✅ 两个方向均采用

<!-- A10 --> ### 2026-05-25 - Milestone 分期策略——6+1 期，M4 优先，M3 延后

- **决策**: M0~M6 七期规划，M0-M2 串行推进，M4 优先于 M3，M3 建议在 M4 稳定后推进
- **原因**: M3（Console 统一）涉及修改内核启动核心路径（entry.rs），风险最高；M4（性能优化）在稳定独立串口上推进，有清晰基准；降低集成风险，先验证性能再做统一
- **影响**: M0→M1→M2 串行必做；M2 完成后 M4 优先推进，M5 可并行（依赖 M2 基础）；M3 建议在 M4 稳定后再做
- **依赖关系**:
  ```
  M0 → M1 → M2
               ├→ M4（优先）→ M5 → M6
               └→ M3（延后，风险高）
  ```
- **状态**: 方向 A 的规划

<!-- A11 --> ### 2026-05-25 - QEMU 双串口开发策略——独立硬件隔离风险 ⚠️ ARCHIVED

> **已归档** (2026-05-27) — 已不适用。QEMU 第二串口需要补丁未合并，决策共用 UART0。

- **决策**: M0-M2 阶段 QEMU 配置第二个 `-serial`，AsyncUart 操作第二 UART 硬件实例
- **原因**: Console 串口用于内核日志和 shell 交互，如果直接在上面测试中断驱动可能破坏调试信息输出
- **归档原因**: QEMU 第二串口不可用

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

<!-- A18 --> ### 2026-05-28 - AsyncUart 完全替代 Console（战略转向）⚠️ ARCHIVED

> **已归档** (2026-05-28) — 战略转向失败，详见 ADR-019。

- **决策**: 完全剔除 Console，AsyncUart 独占 UART 硬件
- **实施结果**: ❌ **失败** - M3 替换失败，IRQ 风暴 + TX busy-loop

<!-- A19 --> ### 2026-05-28 - M3 替换失败回滚 ⚠️ ARCHIVED

> **已归档** (2026-05-28) — 详细内容见 ADR-017/018。方向 A 的渐进式集成方案失败。

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

<!-- A22 --> ### 2026-05-28 - UART MMIO 权限问题发现

- **背景**: P1 UART 初始化遇到 MMIO 权限阻塞（内核上下文无法访问 UART）
- **问题**:
  - Page Fault @ 0x1000001c（物理地址未映射）
  - StoreFault @ 0xffffffc01000001c（虚拟地址无写入权限）
  - LoadFault @ 0xffffffc010000008（虚拟地址无读取权限）
- **根因**: axplat 在 boot 阶段映射 UART MMIO，内核启动后权限被限制
- **影响**: 无法在内核上下文中访问 UART 寄存器，无法验证/修改 UART 配置
- **策略调整**: 完全放弃访问 UART 寄存器，依赖 axplat 配置；提出测试 ISR 上下文访问权限

<!-- A23 --> ### 2026-05-29 - ISR UART MMIO 权限测试失败：证明不彻底更改底层支持无法使用异步串口

- **背景**: ADR-022 提出测试 ISR 上下文访问权限的策略
- **测试结果**:
  - ✅ ISR handler 成功注册并执行
  - ❌ ISR 尝试读 UART ISR 寄存器时触发 LoadFault（`stval=0xffffffc010000008`）
- **关键结论**: ISR 上下文也无法访问 UART 寄存器（MMIO 权限限制仍然存在）
- **根因分析**:
  - axplat 在 boot 阶段映射 UART MMIO，权限被限制（只读或禁止）
  - MMIO 权限限制对**所有上下文**都生效（内核 + ISR）
  - 外部 crate（axplat）的架构约束无法在 kernel 层绕过
- **影响**:
  - ❌ 原设计（ISR 使能 TX 中断）完全不可行
  - ❌ AsyncUart 异步 TX 路径无法实现（依赖 IER::THR_EMPTY 中断）
  - ✅ 证明了不彻底更改底层支持就无法使用异步串口
- **后续策略**:
  - 方案 A：Polling TX（同步阻塞）— 简单可行，牺牲性能
  - 方案 B：Boot 阶段修改 UART 配置 — 需修改 axplat，复杂度高
  - 方案 C：完全依赖 Console — 放弃 AsyncUart 独占目标
- **决策**: 暂缓 AsyncUart 实现，记录关键发现，等待架构层面决策

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
- **状态**: ✅ Q0/Q1 完成（2026-05-31）
