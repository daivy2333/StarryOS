# Spec: architecture — 架构决策记录

## Purpose

汇总 StarryOS 异步串口子系统的所有架构决策（ADR-001~031），按主题分组。每条决策保留**决策内容 / 原因 / 影响 / 替代方案 / 状态**五要素，是后续开发的设计基线。归档（tombstone）的 ADR 已移至 `.claude/docs/archive.md`，不在本规范中保留。

## Requirements

### Requirement: 异步运行时选型 — 复用 axtask::future + embassy-sync::AtomicWaker

异步串口运行时 MUST 基于 `axtask::future`（`block_on` + `poll_io` + `register_irq_waker`），并 MUST 引入 `embassy-sync::AtomicWaker` 用于 ISR 中安全唤醒 Waker，禁止引入完整 Embassy。

**决策详情**（2026-05-24, ADR-001）：

- **原因**：axtask 已有调度器，embassy-executor 会冲突；embassy-sync 无 OS 依赖可单独使用；Pipe / EventFd 已验证 axtask::future 模式可行
- **影响**：保留内核调度器独立性；需要自己定义 `AsyncUart` trait；ISR 唤醒走 `AtomicWaker::wake`，O(1) 复杂度
- **替代方案**：
  - ❌ 完整引入 Embassy（executor + HAL + sync）— 与 axtask 冲突
  - ❌ 仅用 embedded-io-async traits — 仍需自建 IRQ 绑定
  - ✅ axtask::future + AtomicWaker — 最小侵入，复用现有
- **状态**：✅ 已落地（Q1~Q7 全部基于此架构）

#### Scenario: 实现新的 UART 异步原语

- **WHEN** 开发者实现新的 `Future` 或 `Pollable` 用于 UART I/O
- **THEN** 必须基于 `axtask::future::poll_fn` + `embassy_sync::AtomicWaker` 模式编写，禁止引入 `embassy-executor` 或 `embassy-time`

### Requirement: VFS 接口 — DeviceOps trait + Device wrapper

UART 设备 MUST 通过 `DeviceOps` trait + `Device` wrapper 注册到 `/dev`，禁止直接 impl `FileLike`。

**决策详情**（2026-05-24, ADR-003）：

- **原因**：所有现有 `/dev` 设备都通过 DeviceOps 注册；Device struct 自动处理转换链；`as_pollable()` 提供 poll/select/epoll 支持
- **影响**：注册代码与 event/fb 等设备一致；offset 参数对串口无意义（流设备）可忽略
- **替代方案**：
  - ❌ 直接 impl `FileLike` — 需重复实现 fd 管理逻辑，破坏现有模式
  - ✅ DeviceOps — 与现有设备一致，复用转换链
- **状态**：✅ 已落地（`/dev/async_uart` 与 `/dev/console`）

#### Scenario: 注册新串口设备节点

- **WHEN** 开发者要把新串口实例暴露到用户空间
- **THEN** 必须实现 `DeviceOps`（含 `read_at` / `write_at` / `ioctl` / `as_pollable` / `flags`），通过 `Device::new` 包装后挂入 devfs builder

### Requirement: 缓冲策略 — ringbuf::HeapRb + axpoll::PollSet

每个方向（RX / TX）MUST 各自使用一个 `ringbuf::HeapRb<u8>` + `axpoll::PollSet`，硬件 FIFO 与内核 ringbuf 之间的搬运 MUST 由单一后台协程完成。

**决策详情**（2026-05-24, ADR-004）：

- **原因**：`HeapRb` 在 Pipe 中已验证；零额外依赖；Producer/Consumer 分离；默认 64 KiB 与 Pipe 一致
- **影响**：HeapRb 不是中断安全 — 同一时刻只能有单一 reader / writer 操作 ring buffer；每端口 128 KiB 内存
- **替代方案**：
  - ❌ 手写环形缓冲区 — 容易边界 bug
  - ❌ embassy-sync::Channel — 额外依赖，每元素需单独分配
  - ✅ ringbuf::HeapRb + PollSet — 已验证，零额外依赖
- **状态**：✅ 已落地

#### Scenario: 设计新的数据通路

- **WHEN** 开发者实现 UART 与用户空间之间新的数据搬运路径
- **THEN** 必须确保硬件 FIFO 的读取/写入由单一 copier 任务完成，禁止在 ISR 中直接操作 ring buffer，禁止多个任务并发 drain 同一 FIFO

### Requirement: termios 支持 — 默认 raw 模式可切换

UART 默认 MUST 运行在 raw 模式，termios 行规则 MUST 作为可选功能通过 ioctl 动态启用。

**决策详情**（2026-05-24, ADR-005）：

- **原因**：高性能数据通道需要 raw 字节流零开销；终端交互需要 termios 行规则；两者兼得
- **影响**：默认路径零开销；termios 启用时复用现有 `Termios` 和 `ldisc` 逻辑；行规则处理在 `read_at` / `write_at` 中完成
- **替代方案**：始终 raw（无法支持终端应用）/ 始终 termios（所有数据路径都有开销）
- **状态**：✅ 设计保留，已通过 ldisc 集成

#### Scenario: 通过 ioctl 切换 termios

- **WHEN** 用户态程序对 `/dev/console` 调用 `ioctl(TCSETS, ...)` 设置终端属性
- **THEN** 必须保持 raw 数据通路零开销，行规则只在启用时介入

### Requirement: 硬件抽象 — AsyncUart trait

异步串口 MUST 定义 `AsyncUart` trait，初期实现 `Uart16550<MmioBackend>`，为 `DwApbUart` 等其他型号预留实现位。

**决策详情**（2026-05-24, ADR-006）：

- **原因**：StarryOS 支持 RISC-V / LoongArch / AArch64 / x86_64 四架构，初期均用 16550 但未来可能需要其他型号
- **影响**：AsyncUart 使用 axtask::future 异步语义；可在此基础上实现 `embedded-io-async::Read/Write` 获得生态兼容性
- **替代方案**：
  - ❌ 直接用 `embedded-io-async` — 缺少 IRQ/FIFO 信息
  - ❌ 硬编码 16550 — 不支持其他硬件
  - ✅ 自定义 AsyncUart trait — 精确匹配需求，可扩展
- **状态**：✅ 设计保留

#### Scenario: 适配新的 UART 硬件型号

- **WHEN** 项目需要支持非 16550 的 UART 控制器
- **THEN** 必须实现 `AsyncUart` trait 的新后端，复用 ISR → AtomicWaker → copier 任务的上层架构

### Requirement: DMA 策略 — 远期目标，M0~M4 全中断驱动

DMA 探索 MUST 归入远期 M6，M0~M4 MUST 全程基于中断驱动 + NAPI 批量轮询优化。

**决策详情**（2026-05-25, ADR-012）：

- **原因**：QEMU virt 平台没有真正的 16550 DMA 通道；DMA 需要真板或 virtio-console 方案；中断驱动 + NAPI 可覆盖大部分性能需求
- **影响**：高吞吐场景用 NAPI 替代 DMA；性能优化聚焦中断驱动而非 DMA
- **状态**：✅ 设计保留

#### Scenario: 性能优化路径选择

- **WHEN** 开发者考虑提升吞吐量
- **THEN** 必须优先尝试 NAPI / 批量 I/O / IER 缓存等中断驱动优化，DMA 推迟到 M6 真板阶段

### Requirement: 内核日志同步阻塞约束 — axhal::console 外部 crate 不可修改

内核启动日志的同步阻塞开销 MUST 接受为既定约束；用户态 Console 输出 MUST 可异步化，性能优化聚焦用户态路径。

**决策详情**（2026-05-27, ADR-013）：

- **原因**：外部 crate 层次 `axruntime → axplat-riscv64-qemu-virt → axhal → axtask → axpoll` 均来自 crates.io，不可修改
- **影响**：内核启动日志（`axlog::init`、`ax_println!`）始终走同步 polling TX 路径；用户态 Console 输出可通过 AsyncUart 异步化；性能优化重点在用户态路径
- **状态**：✅ 关键约束，已落地（Console 与 Async 共存）

#### Scenario: 修改内核日志路径

- **WHEN** 开发者想改 `ax_println!` 走异步路径
- **THEN** 不可行 — 该路径依赖外部 crate；必须保留 Console polling TX 作为内核日志通道

### Requirement: 探索方向 A — 渐进式集成 Console（已部分失败，吸取教训）

方向 A 策略（M1/M2 用 Console 验证架构，M3 替换为 AsyncUart）MUST 不再继续使用，但其 M1/M2 验证过的架构（Ring Buffer + ISR + copier 任务 + VFS 集成）MUST 在新方向中复用。

**决策详情**（2026-05-27, ADR-014 / ADR-015）：

- **背景**：M1/M2 阶段 Console 和 AsyncUart 共享 UART 硬件验证上层架构，M3 替换为真正异步引擎
- **结果**：❌ M3 失败 — IRQ 风暴 + TX busy-loop（详见 ADR-016, ADR-017）
- **保留价值**：M1/M2 验证过的 Ring Buffer + ISR + copier + DeviceOps 架构在新方向（kernel 层独立实现）中完整复用

#### Scenario: 复用方向 A 已验证架构

- **WHEN** 在 kernel 层新实现异步串口模块
- **THEN** 必须采用 Ring Buffer + ISR → AtomicWaker → copier 任务模型（M1/M2 已验证），禁止重新发明轮子

### Requirement: 方向 A 失败教训 — 集成前 dump 全部寄存器

任何硬件集成前 MUST 先 dump `IER` / `IIR` / `LSR` / `MCR` 等寄存器状态，禁止假设外部 crate 留下的 UART 状态可用。

**决策详情**（2026-05-27, ADR-016 / ADR-017）：

- **失败 1**：软件路径分离方案（Console RX 禁用 + AsyncUart 独占）— Shell 卡住，根因 AsyncUart RX copier 无法从 UART 正确读数据
- **失败 2**：M3 替换尝试 — IRQ 风暴（RX-COPIER 和 tty-reader 循环唤醒）+ TX busy-loop（FIFO 满，LSR=0x00，THR_EMPTY=false TEMT=false）
- **教训**：
  1. ❌ 未验证硬件状态就开始集成（假设 Console 初始化后的 UART 状态正常）
  2. ❌ 未添加足够调试信息（IIR / MCR / 完整 LSR 状态）
  3. ❌ 战略转向过于激进（未充分验证可行性）

#### Scenario: 集成新驱动

- **WHEN** 开发者准备在已初始化的 UART 上启用新驱动
- **THEN** 必须先调用 `log_uart_state()` 风格的诊断函数，输出全部关键寄存器，验证状态符合预期后才能继续

### Requirement: 探索方向 B — 完全剔除 Console（部分失败，被 stride=4 根因纠正）

方向 B 策略（feat/uart-async-dev2 分支，完全剔除 Console 从零开始）的核心设想（avoid Console 数据竞争 / IRQ 冲突 / 重初始化冲突）MUST 在新方向中保留；但其"必须修改 axplat"的前提 MUST 撤销 — 已被 ADR-026 stride=4 根因纠正。

**决策详情**（2026-05-28, ADR-020 / ADR-021）：

- **背景**：方向 A M3 失败后创建 feat/uart-async-dev2 分支，使用 uart_16550 crate 本地初始化，不依赖 axplat
- **四个关键决策**：
  1. UART 硬件初始化：uart_16550 crate 本地初始化，`IER::DATA_READY | IER::THR_EMPTY`
  2. earlycon 内核日志：复用 axhal::console
  3. AsyncUart 设备注册：DeviceOps + Pollable
  4. IRQ waker 分发：ISR 读 ISR 寄存器判断 InterruptType，唤醒 rx_waker/tx_waker
- **阻塞**：P1/P2 出现 LoadFault，最初误判为 MMIO 权限问题（ADR-022/023，已归档至 archive.md）
- **纠正**：详见 ADR-026，真正根因是 stride=4 配置错误

#### Scenario: 评估新阻塞问题的根因

- **WHEN** UART 操作出现 LoadFault 或类似硬件错误
- **THEN** 必须先排查 stride / 地址 / 寄存器映射等代码 bug，再考虑页表权限等系统级问题

### Requirement: MMIO 权限纠正 — UART 在最终页表中已正确映射

UART MMIO `0x10000000` MUST 视为已通过 `axplat::mem::mmio_ranges` 在最终内核页表中正确映射（`READ | WRITE | DEVICE`），不需要修改 axplat。

**决策详情**（2026-05-31, ADR-024）：

- **背景**：ADR-022/023 认为 UART MMIO 权限被 axplat 限制，导致方向 B P1/P2 阻塞。2026-05-31 经深入代码阅读验证后发现此结论有误
- **验证路径**：
  1. `axconfig.toml` 的 `mmio-ranges` 包含 `[0x1000_0000, 0x1000]`（UART）
  2. `axplat::mem::mmio_ranges()` → `axhal::mem::memory_regions()` 将 UART MMIO 包含在内
  3. `axmm::init_memory_management()` → `new_kernel_aspace()` → `map_linear(phys_to_virt(0x10000000), 0x10000000, 0x1000, READ|WRITE|DEVICE)`
  4. Console 的 `MmioSerialPort` 访问 `0xffffffc010000000` 能正常工作恰好证明了映射有效
- **影响**：
  - ✅ 移除"必须修改 axplat"的阻塞条件
  - ✅ 异步串口可在 kernel 层独立实现，不改任何外部 crate
  - ✅ `axmm::iomap()` 可作为安全保障

#### Scenario: 访问设备 MMIO

- **WHEN** 开发者需要操作非标准设备 MMIO 区域
- **THEN** 可以直接使用经过 `axplat` 注册的虚拟地址（`phys_to_virt(phys_addr)`），或调用 `axmm::iomap(PhysAddr, size)` 作为冗余安全网

### Requirement: 重大根因 — stride=4 配置错误（不是页表权限）

NS16550 寄存器空间仅 8 字节，`UART_STRIDE` MUST 配置为 1。任何 stride 超过 1 的实现 MUST 视为 bug。

**决策详情**（2026-05-31, ADR-026）：

- **背景**：Q0 Spike 中先调用 `axmm::iomap()` 成功，但 `uart.isr()` 仍触发 LoadFault。对比 Console（stride=1，正常）和我们的代码（stride=4，LoadFault），发现根因
- **根因**：NS16550 寄存器空间仅 0x00-0x07 共 8 字节。`UART_STRIDE=4` 下 ISR（register offset 2 × stride 4 = 8）读写到 `base+8`，超出 UART 寄存器范围。QEMU NS16550 设备只响应 0x00-0x07 范围内的访问，越界访问产生总线错误，RISC-V CPU 将其解释为 LoadFault
- **验证**：stride=1 下 raw pointer 读 LSR（base+5）→ `0x60` ✅；同时 stride=4 的 base+8 → LoadFault ❌；同一 4K 页表映射内两个地址不同结果，排除页表问题
- **影响**：方向 A M3 和方向 B P1/P2 的"MMIO 权限阻塞"诊断全部有误。真正阻塞原因：
  - 方向 A M3: stride=4 + Console UART 状态不兼容（IER 冲突 + TX busy-loop）
  - 方向 B P1/P2: stride=4 导致 LoadFault
- **校正**：stride=1 后全部测试通过（uart_16550 crate 读写正常，ISR handler 正常，Console/Shell 正常，无 IRQ 风暴）

#### Scenario: 配置新的 UART 实例

- **WHEN** 开发者初始化 `Uart16550<MmioBackend>::new_mmio(NonNull<u8>, stride)`
- **THEN** stride 参数必须传 1（NS16550 寄存器物理布局），禁止传 4 或其他值

### Requirement: 统一方向 — kernel 层独立实现异步串口

异步串口 MUST 在 kernel 层完整实现（约 320 行新代码于 `kernel/src/drivers/`），不修改任何外部 crate。

**决策详情**（2026-05-31, ADR-025 / ADR-027）：

- **核心策略**：
  1. UART 维护一个 `SpinNoIrq<Uart16550<MmioBackend>>` 实例（stride=1）
  2. ISR → AtomicWaker → copier 任务模型（复用方向 A M1/M2 验证过的架构）
  3. RX/TX copier 使用 `poll_fn + register_irq_waker` 模式（参考 Pipe/EventFd）
  4. VFS 集成使用 `DeviceOps + Pollable` trait
  5. Console 共存：earlycon polling TX 用于内核日志，AsyncUart 用于用户态 Shell
- **不再需要**：修改 axplat、页表权限修复、方案 A/B/C 三选一
- **Milestone**：Q0（Spike）→ Q1（driver 架构）→ Q2（VFS 集成）→ Q3（Console 共存/替换）→ Q4（性能优化）→ Q5（真板验证）
- **2026-06-11 Q8 更新**：ISR 已无锁化（`read_isr_unlocked()` 替代 SpinNoIrq），copier 改用 `AtomicWaker` 替代 `register_irq_waker`。详见 ADR-025/027 原始决策上下文。

#### Scenario: 添加新的异步串口功能

- **WHEN** 开发者扩展串口能力（如增加 ioctl、添加新 Pollable 事件）
- **THEN** 必须只在 `kernel/src/drivers/serial/` 范围内修改，禁止改动 `axhal` / `axplat` / `uart_16550` 外部 crate

### Requirement: Q2 共存策略 — copier 与 Console 互斥读 UART

在 Console 仍存在的阶段（Q2），RX copier MUST 不启动，由 Console 独占 UART；Q3 替换 Console 后 copier 才接管。

**决策详情**（2026-05-31, ADR-028）：

- **背景**：Q2 同时运行 Console 和 AsyncUart copier 时，Shell 无法接收键盘输入
- **根因**：RX copier 的 `try_receive_byte()` 和 Console tty-reader 的 `read_bytes()` 都读同一个 UART RBR 寄存器。copier 先启动 → 抢在 tty-reader 之前把 FIFO 数据全部读走放入 ring buffer → tty-reader 看到空 FIFO，Shell 收不到输入
- **影响**：Q2 的 `/dev/async_uart` 只提供设备节点和 DeviceOps 基础架构（read/write 在 ring buffer 上操作），实际数据通路（UART ↔ ring buffer）由 Q3 启用

#### Scenario: 出现 reader 竞争

- **WHEN** 项目中出现多个 reader 任务都想 drain 同一硬件 FIFO
- **THEN** 必须设计互斥访问机制（独占控制 / 临界区 / 阶段切换），禁止并发 drain

### Requirement: Q4 全异步 TX — TX copier 接管 UART 发送

`/dev/console` 注册为 `Tty<AsyncUartReader, AsyncUartWriter>`，TX 流 MUST 经过 ring buffer + copier + ISR 协同完成。

**决策详情**（2026-05-31, ADR-029）：

- **实现**：
  1. `AsyncUartWriter` 实现 `TtyWrite` → 写入 ring buffer
  2. TX copier 从 ring buffer 读取，写入 UART THR
  3. FIFO 满且 buffer 还有数据 → `enable_tx_intr()` → ISR 在 THR_EMPTY 时唤醒
  4. ISR 中 `disable_tx_intr()` + `TX_WAKER.wake()` → copier 继续发送
- **内核日志共存**：`ax_println!` 仍走 `axhal::console::write_bytes()`（polling TX），与 TX copier 共享 UART THR 互不冲突

#### Scenario: 修改 TX 发送路径

- **WHEN** 开发者改 TX 数据通路（如添加优先级、批量发送）
- **THEN** 必须保留 `AsyncUartWriter → ring buffer → TX copier → UART THR` 完整链条，禁止跳过 copier 直接操作硬件

### Requirement: Console 与 Async 共存最终架构 — 各司其职

Console 与 Async 串口 MUST 共存，Console 负责内核日志 / 早期启动 / panic 处理，Async 负责 Shell I/O / 用户态程序 / 高性能数据传输。

**决策详情**（2026-06-01, ADR-030）：

- **原因**：
  1. `ax_println!` 依赖 Console（调用 `LogIf::console_write_str → axhal::console::write_bytes`，外部 crate 无法修改）
  2. 早期启动需要 Console（`ax_println!` 在内核启动早期就使用，Async 驱动在稍后才初始化）
  3. panic 处理需要 Console（panic handler 使用 `ax_println!`，需要可靠输出方式）
  4. 当前方案工作正常（共享 UART THR，互不冲突）
- **影响**：
  - ✅ Console 负责：内核日志、早期启动日志、panic 处理
  - ✅ Async 负责：Shell I/O、用户态程序、高性能数据传输
- **替代方案**：
  - ❌ 完全剔除 Console — `ax_println!` 依赖外部 crate
  - ❌ 修改 `ax_println!` 实现 — 需要修改外部 crate
  - ✅ 保持共存 — 简单可靠

#### Scenario: 增加新的输出路径

- **WHEN** 开发者考虑添加新的日志或数据输出通道
- **THEN** 必须先判断属于"内核态可靠日志"还是"用户态高性能数据"，前者走 Console，后者走 Async

### Requirement: RX 测试方法 — 内核态测 Ring Buffer 绕过 TTY

Async RX 性能测试 MUST 在内核态直接测试 Ring Buffer，Console RX MUST 跳过测试（无 Ring Buffer 且非阻塞），用户态 RX MUST 跳过（TTY 回显竞争）。

**决策详情**（2026-06-01, ADR-031）：

- **原因**：
  1. Async 有 Ring Buffer：64 KB，支持大数据量测试
  2. Console 没有 Ring Buffer：`read_bytes()` 非阻塞，没数据立即返回 0
  3. FIFO 无法直接测试：容量小（16 字节）、非阻塞读取、需外部数据注入、与 Shell 竞争
  4. 用户态 RX 都无法测试：TTY 层回显导致 Shell 抢先读取数据
- **测试位置**：`kernel/src/drivers/benchmark.rs` 中 `run_rx_throughput_test()` / `run_rx_latency_test()`
- **结果**：Async RX Ring Buffer 读取 588,776 KB/s，延迟 P50 600 ns

#### Scenario: 设计新的 I/O 性能测试

- **WHEN** 开发者要测量 I/O 性能
- **THEN** 必须先判断测试对象是"硬件吞吐 / 软件路径 / 端到端延迟"，再选择对应位置（内核态绕过 TTY / 用户态完整链路 / QEMU 时序欺骗需特殊处理）

### Requirement: 异步串口提取 — uart_16550 成为完整异步 UART crate

异步串口实现 MUST 提取到 `uart_16550` crate，通过 `async` feature gate 支持，使其成为可复用的异步 UART crate，适用于任何 Rust RISC-V OS 项目。

**决策详情**（2026-06-15, ADR-032）：

- **背景**：
  - StarryOS 异步串口栈（Q0~Q12）已成熟，核心逻辑 ~400 行
  - 其他 OS 项目（Linux kernel module, Tock capsule, RTIC driver）也需要异步 UART
  - Q12 已完成基础设施迁移（`atomic_ring_buffer` + `embedded_io_async` + TC tcdrain）
  - 现有 D1 决策（uart_16550 ADR-7）说"异步留在 wrapper 层"，需要推翻
- **决策**：
  1. uart_16550 新增 `async` feature gate，包含完整异步串口实现
  2. 定义 5 个 OS 抽象 trait（`OsRuntime`, `OsIrq`, `OsMmio`, `OsSpinNoIrq`, `OsWakerSet`）
  3. StarryOS 实现 ArceOS 适配层，从 uart_16550 导入异步实现
  4. 删除 StarryOS 本地 drivers/ 中已迁移的代码
- **原因**：
  1. 复用需求 — 其他 OS 项目也需要异步 UART
  2. 代码量可控 — 核心异步逻辑仅 ~400 行，不增加维护负担
  3. trait 抽象成熟 — `embedded_io_async` 是社区标准，`embassy-sync` 是 ISR 安全的
  4. Q12 已完成基础设施 — 可直接复用
- **影响**：
  - ✅ uart_16550 成为完整的异步 UART crate
  - ✅ 其他 OS 项目可通过实现 5 个 trait 快速集成
  - ✅ StarryOS 消除 ~400 行本地代码
  - ⚠️ 需要推翻 D1 决策（uart_16550 ADR-7）
  - ⚠️ 需要处理全局状态（`UART`, `DRIVER`, `ASYNC_TTY`）的泛型化
- **替代方案**：
  - ❌ 保持 D1 决策（wrapper 方案）— 无法复用
  - ❌ 创建独立 crate（`portable-async-uart`）— 增加维护负担
  - ✅ 提取到 uart_16550（本决策）— 最小侵入，复用现有
- **状态**：📋 待实施

#### Scenario: 实现异步串口的 OS 适配层

- **WHEN** 开发者要在新 OS 项目中使用 uart_16550 的异步功能
- **THEN** 必须实现 5 个 OS 抽象 trait（`OsRuntime`, `OsIrq`, `OsMmio`, `OsSpinNoIrq`, `OsWakerSet`），然后启用 `uart_16550` 的 `async` feature

#### Scenario: 推翻 D1 决策

- **WHEN** 开发者需要修改 uart_16550 的异步集成策略
- **THEN** 必须参考本 ADR-032 的决策理由，确保不破坏跨平台复用目标

<!-- A033 -->
### ADR-033: uart_16550 成为完整异步 UART crate

**日期**: 2026-06-16
**状态**: 已接受
**决策**: 推翻 ADR-007（D1 决策），将异步串口实现从 StarryOS 提取到 uart_16550 crate

**背景**:
- StarryOS Q0~Q12 积累 ~618 行异步串口实现
- 其他 OS 项目（Linux kernel module, Tock capsule, RTIC driver）也需要异步 UART
- Q12 已完成基础设施迁移（atomic_ring_buffer + embedded_io_async + TC tcdrain）

**决策内容**:
1. uart_16550 新增 `async` feature gate
2. 定义 5 个 OS 抽象 trait（OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet）
3. 迁移 ISR handler, ring buffer, copier driver, device_ops 到 uart_16550
4. StarryOS 实现 ArceOS 适配层（os_arceos.rs）

**替代方案**:
- 保持异步在 StarryOS（原 D1 决策）→ 复用性差
- 独立 crate（uart_16550-async）→ 维护负担

**影响**:
- uart_16550 代码量增加 ~400 行
- StarryOS 删除 ~370 行本地代码
- 其他 OS 只需实现 5 个 trait 即可使用异步 UART

**关键设计**:
- `UartPort` trait 解决 `&mut self` 问题（Uart16550 方法需要 &mut self）
- `OsSpinNoIrq` 使用回调模式（`with_lock`）避免 guard 生命周期问题
- Ring buffer 静态变量由 OS 拥有（`&'static RingBuffer` 传入 uart_16550）
- 驱动使用 `&'static Self` 而非 `Arc<Self>`（兼容 no-alloc）
