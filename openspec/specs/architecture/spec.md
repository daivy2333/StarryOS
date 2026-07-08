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

<!-- tombstone: A014/A015 --> Archived 2026-07-08 in ARC-202607081429 — 被 ADR-025/027 替代；M3 失败（IRQ 风暴 + TX busy-loop），核心架构由 ADR-025/027 继承。

<!-- tombstone: A016/A017 --> Archived 2026-07-08 in ARC-202607081429 — 方向 A 失败教训；"dump 寄存器"教训在 learned L79，stride 根因由 ADR-026 纠正。

<!-- tombstone: A020/A021 --> Archived 2026-07-08 in ARC-202607081429 — 方向 B 已纠正；核心设想由 ADR-025/027 继承，stride 根因由 ADR-026 纠正，MMIO 权限由 ADR-024 澄清。

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

<!-- tombstone: A032 --> Archived 2026-07-08 in ARC-202607081429 — 被 ADR-033（"已接受"）正式化；5-trait 由 ADR-036 缩减为 2-trait。

<!-- A033 -->
### Requirement: ADR-033: uart_16550 成为完整异步 UART crate

uart_16550 MUST provide the complete async UART stack via the `async` feature gate with OS abstraction traits.

**日期**: 2026-06-16
**状态**: 已接受
**决策**: 推翻 ADR-007（D1 决策），将异步串口实现从 StarryOS 提取到 uart_16550 crate。

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

#### Scenario: New OS integrates async UART

- **WHEN** a developer wants to use async UART in a new OS project
- **THEN** they MUST implement the OS abstraction traits and enable the `async` feature gate
- **AND** the async stack SHALL work without modifying uart_16550 internals

<!-- A034 -->
### Requirement: ADR-034: LTO 延期启用 — 已知有效但开发期暂不开

LTO MUST be re-enabled before production release; during active development compile speed SHALL take priority.

**日期**: 2026-06-16
**状态**: 已接受
**决策**: 暂不开启 `lto = true`，记录为已知优化手段，最终发布前再加回。

**背景**:
- 2026-06-16 在 `feat/uart-16550-bench` 分支实测 LTO 效果：
  - Ring buffer TX 385→652 MB/s（↑69%）
  - RX 延迟 P50 200ns→<100ns
  - e2e 延迟不变（瓶颈在调度）
- 本质是消除跨 crate 函数调用开销（embassy_hal_internal → uart_16550）
- 不是代码逻辑优化，纯编译器层面的链接时内联

**决策理由**:
- LTO 使 release build 时间增加 2-3×
- 当前处于活跃开发期，编译速度比这 3% 的 ring buffer 提升更重要
- 最终发布构建时加回一行配置即可，零代码改动

**回滚操作**:
- 从 uart_16550 + StarryOS 的 `Cargo.toml` 中删除 `[profile.release] lto = true`
- 性能文档中 LTO 数据保留为参考（标注为"需在最终构建中开启"）

#### Scenario: Production build preparation

- **WHEN** the project reaches development freeze
- **THEN** LTO MUST be re-enabled in Cargo.toml before the production build
- **AND** the ring buffer throughput regression against LTO baseline SHALL be verified

<!-- tombstone: A035 --> Archived 2026-07-02 in ARC-202607021648 — 被 ADR-036 替代；5-trait 设计仅作为历史 rationale 保留。

<!-- A036 -->
### Requirement: ADR-036: OS abstraction 缩减至 2-trait 最小接口

The OS abstraction layer MUST only include traits actually invoked by driver code; unused YAGNI traits SHALL be removed immediately.

**日期**: 2026-06-19
**状态**: 已接受
**决策**: 从 `uart_16550::os` 中删除 `OsIrq`、`OsMmio`、`OsSpinNoIrq` 三个 trait，保留 `OsRuntime` + `OsWakerSet` 构成**最小可移植接口**。

**背景**:
- ADR-035（2026-06-17）定义了 5 个 OS 抽象 trait，声称是"最小完备接口集"
- 2026-06-19 实际追踪发现：被删的 3 个 trait 在整个 uart_16550 crate 中**从未被 import 或调用**——它们只存在于 trait 定义处
- `cargo build` 报告 3 个 `dead_code` warning：`ArceOsIrq` / `ArceOsMmio` / `ArceOsSpinNoIrq` never constructed

**根因分析 — 驱动架构刻意外部化了这三种职责**:

| 职责 | 实际做法 | 不需要 trait 的原因 |
|------|---------|-------------------|
| IRQ 注册 | `axhal::irq::register_irq_hook()` 直调 | ISR handler 在 OS 层注册，驱动只接收已映射的 `NonNull<u8>` |
| MMIO 映射 | `axmm::iomap()` + `phys_to_virt()` 直调 | 驱动在构造时已拿到映射好的指针，内部直接用 `read_volatile` |
| 关中断锁 | `kspin::SpinNoIrq` 直用 | 锁在 UART 全局实例 `uart_instance()` 层持有，驱动通过 `UartPort` trait 间接访问 |

这三种职责在驱动**外部**完成，驱动代码路径根本不需要走 trait 抽象。保留它们构成 YAGNI 违规。

**决策内容**:
- 从 `uart_16550/src/os/mod.rs` 删除 `OsIrq`（41-47 行）、`OsMmio`（54-71 行）、`OsSpinNoIrq`（81-90 行）
- 从 `kernel/src/drivers/os_arceos.rs` 删除 `ArceOsIrq`（44-53 行）、`ArceOsMmio`（57-80 行）、`ArceOsSpinNoIrq`（85-100 行），清理不再需要的 `NonNull`/`PhysAddr` 导入
- 模块文档更新为 "2 minimum-viable traits"，标注 IRQ/MMIO/锁外部化的架构原因

**影响**:
- uart_16550 `os` 模块从 112 行缩减到 61 行（↓45%）
- StarryOS `os_arceos.rs` 从 123 行缩减到 63 行（↓49%）
- 新 OS 移植接口从 5 trait → 2 trait，认知负担减半
- `OsMmio` 删除后不再需要 `core::ptr::NonNull` / `memory_addr::PhysAddr` 导入
- `OsIrq` 删除后不再需要 `axhal` 依赖声明
- `cargo build` 0 warning（之前 3 个 dead_code 消除）

**ADR-035 修正**:
- ADR-035 的"5 个 trait 构成最小完备接口集"论断不成立——实际最小集是 2
- ADR-035 保留为历史记录（记录了 Q13 提取时的完整思考过程），本 ADR 作为事后修正
- 若未来驱动需要 DMA 管理（`OsDma`），届时追加 trait 不迟

**替代方案**:
- ❌ 保留 + suppress warning（`#[allow(dead_code)]`）：不解决根本问题，死代码持续膨胀认知负担
- ❌ 让驱动实际调用它们：需要重构 isr.rs/driver.rs，把外部职责拉回驱动内部，方向相反
- ✅ 删除 + 文档化：保持接口最小化，ADR 记录决策依据

**参考**:
- ADR-035（原始 5-trait 设计，2026-06-17）
- uart_16550: `src/os/mod.rs`（2 trait 最小接口）
- StarryOS: `kernel/src/drivers/os_arceos.rs`（2 适配器）

#### Scenario: Dead trait detected

- **WHEN** `cargo build` reports dead_code warnings on OS abstraction types
- **THEN** the unused traits MUST be removed from the OS abstraction layer
- **AND** the corresponding adapter impls SHALL be deleted

<!-- A037 -->
### Requirement: ADR-037: TxCompletion 四阶段 drain API 设计

The `flush()` implementation MUST wait for all four drain stages; `tcdrain` SHALL use `driver().tx_completion()` instead of direct MMIO.

**日期**: 2026-06-23
**状态**: 已接受
**决策**: 在 `AsyncUartDriver` 中引入 `TxCompletion { ring_empty, copier_active, staged_bytes, transmitter_empty }` 快照结构体 + `tx_completion()` 方法，供 `flush()` 和 `tcdrain` 轮询四阶段排空完成（ring→copier→FIFO→shift register→wire）。

**背景**:
- Q7 时代 tcdrain 直接读 `uart_instance().lock().lsr()` 判断 TEMT，绕过 driver 架构
- `flush()` 直接返回 `Ok(())` 无任何等待
- 缺少对 copier staging（已从 ring pop 但未 send 到 UART）的可见性
- 真板 NS16550 存在 TEMT 丢唤醒窗口：THRE 中断触发时 TEMT 可能为 0，随后 TEMT→1 不产生新中断

**决策内容**:
| 决策 | 选择 | 理由 |
|------|------|------|
| TxCompletion 快照语义 | 4 字段 Relaxed 独立读取，不保证原子性 | flush 是 polling 语义，多次调用直到收敛 |
| tx_copier_active 生命周期 | poll 入口 set true，Pending 前 clear false | flush 只需知道 copier 当前是否在处理 |
| tx_staged_bytes 计数 | pop_batch 后 +N，send_bytes>0 后 -S | 追踪 ring→FIFO 在途字节 |
| UartPort 扩展 | 新增 `transmitter_empty() -> bool`，不暴露完整 LSR | 最小接口原则 |
| TEMT corner-case fix | copier 在 send 完最后字节后 bounded spin 256 次等 TEMT | 真板 DRAIN_WAKER 窗口修复；flush 保持纯事件驱动 |
| flush() 实现 | poll_fn + DRAIN_WAKER + register-recheck-Pending | 复用 ISR 已有 DRAIN_WAKER；两路唤醒（ring waker + DRAIN_WAKER） |
| tcdrain 重构 | 改用 `driver().tx_completion()` 替代直接 MMIO | 消除架构分层违规，增加 ring/copier/staged 检查 |

**影响**:
- uart_16550 `driver.rs` 新增 ~65 行（TxCompletion + tx_completion + 状态字段 + TEMT poll）
- uart_16550 `device_ops.rs` flush() 从 3 行 → ~30 行
- StarryOS `ctl.rs` tcdrain 从直接 MMIO → driver API（消除分层违规）
- StarryOS `uart_init.rs` 删除死代码 `tx_is_empty()`
- 性能无退化（M2 QEMU benchmark 验证通过，64B=169KB/s vs M1=156KB/s）

**替代方案**:
- ❌ 在 flush() 中做 TEMT polling（而非 copier）→ 拒绝：flush 应保持纯事件驱动
- ❌ 暴露完整 LSR 寄存器 → 拒绝：增加 trait 耦合
- ❌ 用单一 AtomicU64 打包所有状态 → 拒绝：字段语义不匹配，过度复杂

**参考**:
- Q15-M2 OpenSpec 变更: `openspec/changes/m2-tx-completion-drain/`
- learned/spec.md L201: TEMT corner-case 丢唤醒窗口
- uart_16550: `src/async_/driver.rs`（TxCompletion + tx_completion）
- StarryOS: `kernel/src/syscall/fs/ctl.rs`（tcdrain 新实现）

#### Scenario: Flush must wait for all drain stages

- **WHEN** a caller invokes `flush()` on the async UART writer
- **THEN** the implementation MUST poll until all four stages (ring, copier, FIFO, shift register) are drained
- **AND** the TEMT corner-case wake window SHALL be handled without polling

<!-- A038 -->
### Requirement: ADR-038: TtyWrite 改为返回实际接受字节数 — M3 短写契约

TtyWrite::write MUST return the actual number of bytes accepted by the output sink so VFS callers can observe short writes.

**日期**: 2026-06-23
**状态**: 已实施 ✅ (2026-06-23)
**决策**: 将 `uart_16550::TtyWrite::write(&[u8])` 从无返回值改为 `write(&[u8]) -> usize`，并让 StarryOS `Tty::write_at()` 返回 writer 实际接受的字节数，而不是固定返回 `Ok(buf.len())`。

This contract MUST report the number of bytes accepted by the output sink so VFS callers can observe short writes.

**背景**:
- `RingBufTx::push()` 已返回实际写入数，但 `AsyncUartWriter` 的 `TtyWrite` impl 丢弃该返回值。
- `PtyWriter::write()` 也只记录 short write warning，调用方无法得知丢弃了多少字节。
- `Tty::write_at()` 当前固定返回完整 buffer 长度，导致 TX ring 或 PTY buffer 满时用户态看到“完整写入成功”，形成 silent data loss。
- M1/M2/M4 已分别解决 TX fast retry、TxCompletion drain、IER single owner；M3 可以聚焦契约修正，不需要再改 TX copier。

**决策内容**:
| 决策 | 选择 | 理由 |
|------|------|------|
| trait 返回值 | `usize` | 与 `TtyRead::read` 和 `embedded_io_async::Write::write` 的短 I/O 语义一致 |
| TTY `write_at` | 返回 `Ok(self.writer.write(buf))` | VFS/sys_write 层必须看到真实接受数 |
| 满 ring 语义 | M3 第一阶段返回 `Ok(0)`，不立即引入 `WouldBlock` | 最小 breaking change；blocking exact write 另立后续变更 |
| ldisc echo | 显式 best-effort，忽略返回值 | echo 不是可靠数据写入路径，不能在输入处理循环里阻塞 |
| TX copier | 不改动 | M3 只修正生产者契约，避免影响 M1/M2/M4 已验证状态机 |

**影响**:
- uart_16550 公共 trait breaking change：所有 `TtyWrite` 实现者必须返回实际接受字节数。
- StarryOS 用户态 `write(2)` 可能开始返回短写，benchmark 和测试程序必须循环累计写入。
- PTY 溢出从“只 warn 丢弃”变为“返回短写”，行为更符合 VFS 契约。
- `File::write` 对 `Ok(n)` 不会触发 `poll_io` 重试；若未来需要 blocking exact write，应单独设计 OUT readiness + WouldBlock 语义。

**替代方案**:
- ❌ 保持 void trait，仅在内部日志告警：继续 silent data loss，不能接受。
- ❌ `Tty::write_at` 循环直到写完：会把同步 trait 变成隐式阻塞点，可能重现调度台阶问题。
- ❌ 满 ring 立即返回 `WouldBlock`：语义更强但需要完整验证 blocking/nonblocking/poll OUT，不适合 M3 第一阶段。
- ✅ 返回实际接受数：最小修复，契约清晰，调用方可正确处理短写。

#### Scenario: TX ring cannot accept the full buffer

- **WHEN** a TTY writer accepts fewer bytes than the user buffer length
- **THEN** `TtyWrite::write` MUST return the accepted byte count
- **AND** `Tty::write_at` MUST propagate that count to VFS/sys_write callers

**参考**:
- `.claude/analysis/q15-m3-tty-short-write-contract.md` `[ARCHIVED 2026-07-04 → _archive/2026-06-24-q0-q15-analysis/q15-m3-tty-short-write-contract.md]`
- learned/spec.md L202/L204
- uart_16550: `src/tty.rs`, `src/async_/device_ops.rs`, `src/async_/ring_buffer.rs`
- StarryOS: `kernel/src/pseudofs/dev/tty/mod.rs`, `kernel/src/pseudofs/dev/tty/pty.rs`, `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`

<!-- A039 -->
### Requirement: ADR-039: Q15 M0~M4 增量重融合策略 + Manual QA 验证 — 已完成（2026-06-25）

Q15 阶段 MUST 采用"增量重融合"策略恢复 pre-M4 基线后丢失的 M4+ 正确性修复，并通过 QEMU Manual QA 验证无退化。

**日期**: 2026-06-21（开启）→ 2026-06-25（Manual QA 完成）
**状态**: ✅ 已完成
**决策**: Q15 不一次性 apply `feat/uart-16550-async-temp` 分支的全部 M4+ 代码（避免 73.9x TX backpressure 退化复现），改为按 M0→M1→M2→M4→M3 顺序的 5 个原子 milestone 增量融合，每步 `cargo check` + QEMU benchmark 双重 Gate。

**背景**:
- 2026-06-21: M4 Sync 一次性 apply 全部 M4+ 代码（含 TX backpressure 修复 + 诊断计数器），结果 64B write+tcdrain 退化 73.9x（406µs → 29.99ms）
- 根因：`unblock_task(task, false)` + 100Hz tick → 每 16B FIFO refill 触发 ~10ms 调度台阶
- 决策：回退到 pre-M4 基线（StarryOS `04f8920` / uart_16550 `60c5729`），通过 Q15 阶段增量重新融合
- 原 M4+ 代码保留在 `feat/uart-16550-async-temp` 分支（参考用，未删除）

**Q15 5 个 milestone 增量融合顺序**（非时间顺序，按依赖关系）：
1. **M0** — 见证层（RawMutex / per-port ISR / FIFO 边界矩阵 benchmark / telemetry 计数器），提供基线测量能力
2. **M1** — 有界 TX fast retry（`TX_FAST_RETRY_LIMIT=32`），消除 16B FIFO refill 的 10ms tick 台阶
3. **M2** — TX completion 三阶段 drain（flush / tcdrain 正确等待），TxCompletion API + TEMT corner-case fix
4. **M4** — IER 单 owner（`UartPort::update_ier()` 统一管理），删除 CACHED_IER / write_ier / enable_*
5. **M3** — TtyWrite 短写契约（`write(&[u8]) -> usize`），独立于驱动内部改动，聚焦 VFS 契约修正

**Manual QA Gate 通过（2026-06-25）**：

| Gate | 验证内容 | 通过条件 | 结果 |
|------|---------|---------|------|
| Q15-M0 cargo check | uart_16550 + StarryOS 0 错误/警告 | 0 error + 0 warning | ✅ |
| Q15-M1 benchmark | 64B write+tcdrain 无 10ms 台阶 | ≤ M4 Sync 前基线 | ✅ |
| Q15-M2 benchmark | 64B 吞吐 ≥ 156KB/s | ≥ M1 基线 | ✅ (169KB/s) |
| Q15-M4 benchmark | IER 切换正确，无中断风暴 | cargo check + benchmark | ✅ |
| Q15-M3 benchmark | TtyWrite 短写穿透 5 文件，1B 延迟不退化 | ≤ M4 基线 +129µs | ✅ (134µs) |
| 最终 Manual QA | 全量 QEMU benchmark 综合 | 无 64B write+tcdrain 退化 | ✅ (170KB/s) |

**增量融合策略有效性验证**：

- ✅ **避免了一次性 apply 的退化复现**：Q13 M4 Sync 失败的 73.9x 退化未在 Q15 复现
- ✅ **每步可独立回退**：5 个 milestone 各自独立 commit，任意一项引入 bug 可单独 revert
- ✅ **保留原始参考**：temp 分支完整保留原 M4+ 代码，可对比增量 vs 一次性 apply 的差异
- ✅ **时间效率**：5 天完成 5 个 milestone + Manual QA（原一次性 apply + debug 耗时 4 天仍未恢复）

**影响**:
- uart_16550 + StarryOS 异步栈达到 Q15 设计目标（per-port ISR / ArceOsRawMutex / yield_now / UartPort 扩展 / IER 单 owner / TtyWrite 短写）
- Q15 后待办已在 2026-06-27 重排为 Q16~Q22 roadmap，后续不再使用单一 Q6 真板桶
- `feat/uart-16550-async-temp` 分支保留作为参考，但未来不再直接 merge（增量融合策略是首选）

**替代方案**:
- ❌ 一次性 apply M4+ 全部代码：已证伪（M4 Sync 73.9x 退化）
- ❌ 放弃 M4+ 修复，永久保留 pre-M4 状态：拒绝（IER 单 owner / TtyWrite 短写是正确性必需）
- ❌ 按时间顺序 apply（M4 → M3 → M2 → M1 → M0）：拒绝（M4 依赖 M0 见证层 / M2 依赖 M1 TX fast retry 基线）
- ✅ 按依赖关系增量 + 每步 Gate：当前方案，已验证有效

#### Scenario: 未来 async-uart 优化合并

- **WHEN** 开发者需要合并其他分支（async-uart-1 / future 等）的 async-uart 优化 commit
- **THEN** MUST 遵循 Q15 增量融合策略：按依赖关系排序 → 摘取原子 commit → cargo check → QEMU benchmark → 无退化才继续
- **AND** MUST 保留源分支作为参考（不删除，禁止一次性 merge）
- **AND** 每步 MUST 在 Manual QA Gate 表格新增一行记录验证结果

#### Scenario: Q15 增量融合策略失败

- **WHEN** Q15 任一 milestone 引入性能退化（如 TX backpressure 复现）
- **THEN** MUST 立即停止后续 milestone，定位退化根因
- **AND** 修复后从该 milestone 重新增量融合，禁止跳过或绕过
- **AND** 在 optimization.md 的 Q15 章节追加 incident 记录（含 commit hash / 退化倍数 / 修复方法）

**参考**:
- `openspec/changes/m0-witness-layer/` ... `m4-ier-single-owner/` ... `m3-tty-short-write/`
- optimization.md: Q15 M0~M4 增量重融合 + Manual QA 章节
- learned.md: L201/L202/L204 (Q15-M2/M3 细节)
- SNAPSHOT.md: "Q15 已应用架构" 章节
- `feat/uart-16550-async-temp` 分支：原 M4+ 代码参考

<!-- A040 -->
### Requirement: ADR-040: Q18 真板启动 PLIC / Clock "trust u-boot" 模式（Revised 2026-06-27）

VisionFive2 bring-up MUST preserve U-Boot configured PLIC and Clock state unless diagnostics prove the preserved state is invalid; UART register initialization remains explicitly allowed.

VisionFive2 真板启动时 U-Boot 已配置 PLIC 全局状态和 SoC 时钟树，OS 不应"重新初始化一切"破坏硬件状态。借鉴 arceos ADR-004 决策（`others/arceos/openspec/specs/architecture/spec.md`）及其反复失败教训（PIT-007，7+ 次 `failed attempt`），但**范围收紧为 PLIC + Clock，不包含 UART**。

**证据**: arceos 的 "trust u-boot" 模式仅用于 DWMAC（以太网）驱动（`modules/axdriver/src/dwmac.rs` `set_clocks_uboot()`），不是平台级模式。arceos `riscv64_starfive` 的 UART 走 SBI console 调用，完全不做 UART MMIO 初始化。NS16550 UART 初始化（设波特率/FCR/IER）是简单寄存器写入，重复设置无害——不像 DWMAC 的 PHY 协商那样会破坏已建立的链路状态（2026-06-26 `codegraph_explore` 交叉验证确认）。

**日期**: 2026-06-26（Revised 2026-06-27，Q18 真板观测工具阶段落实）
**状态**: 🟡 Proposed（Q18 bring-up 风险，范围已收紧）
**决策**: Q18 真板启动 PLIC + Clock 初始化采用 "trust u-boot" 模式：
- PLIC init：`init_primary()` 仅 primary CPU 调用，`init_percpu()` 每 CPU 调用（见 ADR-041）
- Clock：检测 U-Boot 已配置的时钟值，跳过重新分频
- UART：**允许正常重新初始化**（设置波特率/FCR/IER），NS16550 寄存器写入无害
- 增加 `print_preserved_status()` dump 当前 PLIC/Clock 状态供真板 debug

**影响**:
- ✅ 避免破坏 U-Boot 已建立的 PLIC 状态和时钟树（arceos PIT-007 教训）
- ✅ 不限制 UART 初始化灵活性（我们仍可自由配置 FCR/IER/波特率用于异步驱动）
- ⚠️ 冷启动场景（无 bootloader）需重新评估 Clock trust 决策（我们场景不涉及）

**替代方案**:
- ❌ trust-u-boot 包含 UART：过度防御。arceos 没有此先例，NS16550 寄存器重写无害
- ❌ 完整自主 PLIC + Clock 协商：U-Boot 接力场景破坏硬件状态（arceos 已证伪）
- ✅ PLIC+Clock trust-u-boot + UART 自由初始化 + print_preserved_status：当前方案

**参考**:
- `others/arceos/modules/axdriver/src/dwmac.rs:101` "U-Boot has already initialized everything"
- `others/arceos/modules/axhal/src/platform/riscv64_starfive/console.rs` — SBI console，无 UART MMIO init
- `others/arceos/openspec/specs/learned/spec.md` PIT-007 / TIP-004
- `.claude/analysis/arceos-borrowable-experience.md` §3.4 `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/arceos-borrowable-experience.md]`（已标注 UART 不适用）
- 2026-06-26 探索验证：bg_27a805e6 确认 arceos starfive 无 UART MMIO init

#### Scenario: VisionFive2 bring-up preserves bootloader state

- **WHEN** StarryOS boots on VisionFive2 through U-Boot
- **THEN** PLIC and Clock setup MUST follow the trust-u-boot policy unless Q18 diagnostics prove the preserved state is invalid
- **AND** UART register initialization MAY still reconfigure FCR, IER, and baud rate for the async driver

<!-- A041 -->
### Requirement: ADR-041: Q18 真板 PLIC 防御性设计模式（Revised 2026-06-27）

PLIC initialization MUST keep global one-time setup separate from per-hart setup; `init_percpu()` MUST NOT perform one-time PLIC construction.

借鉴 arceos ADR-002 决策，保持 PLIC init_primary / init_percpu 显式分离作为防御性设计，防止未来回归旧 arceos 的 `LazyInit<Plic>` 反模式。

**2026-06-26 代码验证结论**: 当前 StarryOS 通过 crates.io 的 `axplat-riscv64-qemu-virt-0.3.1-pre.6` / `axplat-riscv64-visionfive2-0.1.0-pre.2` 使用 `static PLIC: SpinNoIrq<Plic>`（编译时初始化），`init_percpu()` 仅做幂等的 `init_by_context(this_context())` 阈值写入。**当前代码在 QEMU 和 VisionFive2 上均不会 panic**（2026-06-26 `codegraph_explore` 逐行验证，bg_c7fd5ae7）。

**真正风险**: 旧 arceos 代码（`others/arceos/modules/axhal/src/platform/riscv64_qemu_virt/irq.rs`）使用 `LazyInit<Plic>`，且 `init_percpu()` 内部调用 `init_plic()` → `PLIC.init_once(Plic::new(regs))`。在 SMP 上第二次调用 `init_once()` 会 panic（`lazyinit-0.2.2/src/lib.rs:50-51` `.expect("Already initialized")`）。此反模式若被重新引入 StarryOS 会导致真板 panic。

**日期**: 2026-06-26（Revised，降为防御性模式）
**状态**: 🟡 防御性保留（当前代码安全，防止回归）
**决策**: 保持 `init_primary` / `init_percpu` 显式分离作为代码审查 checklist：
- `init_primary()` 做全局一次性初始化，`init_percpu()` 做 per-hart 配置
- **禁止**在 `init_percpu()` 内部调用 `init_once()` 或等效的一次性初始化
- 切换到 VisionFive2 平台时需验证 axplat crate 版本是否保持 `static SpinNoIrq<Plic>` 模式

**影响**:
- ✅ 当前 axplat crate 已安全（`static SpinNoIrq<Plic>` + 幂等 `init_by_context`）
- ⚠️ 防御性关注：如未来有人移植旧 arceos 平台代码，需审查 PLIC 初始化路径
- ✅ 无需紧急修改 — Q17 当前优先项是 O63（内存序），PLIC 初始化作为 Q18 防御性检查保留

**替代方案**:
- ❌ 忽略此 ADR：虽然当前代码安全，但旧反模式存在被重新引入的风险
- ✅ 防御性保留 + 代码审查 checklist：当前方案

**参考**:
- 当前安全代码：`axplat-riscv64-visionfive2-0.1.0-pre.2/src/irq.rs:40-57`
- 旧 arceos bug：`others/arceos/modules/axhal/src/platform/riscv64_qemu_virt/irq.rs:127-131`
- `others/arceos/modules/axhal/src/platform/riscv64_starfive/irq.rs:131-149`（正确分离模式）
- 2026-06-26 探索验证：bg_c7fd5ae7 确认当前 crates 安全，bg_27a805e6 确认 arceos 正确模式

#### Scenario: PLIC initialization review

- **WHEN** StarryOS switches or updates the VisionFive2 platform crate
- **THEN** the PLIC initialization path MUST keep global one-time initialization separate from per-hart initialization
- **AND** `init_percpu()` MUST NOT call `init_once()` or equivalent one-time PLIC construction

<!-- A042 -->
### Requirement: ADR-042: Q17 SMP 原子内存序按语义选择，不按架构分叉

Shared async UART state that participates in cross-hart control flow MUST use Rust atomic orderings according to its synchronization role; StarryOS MUST NOT introduce per-architecture memory-ordering branches for Q17.

**日期**: 2026-06-27
**状态**: 已接受
**决策**: Q17/O63 修复采用语言级内存模型：
- 纯 telemetry / 诊断计数可保持 `Relaxed`
- 发布状态、读取方据此决定 `flush()` / `tcdrain` Ready/Pending 时，写端用 `Release`，读端用 `Acquire`
- 参与同步判断的 RMW 计数用 `AcqRel`，snapshot load 用 `Acquire`
- `ier_cache` 属于非原子 RMW 竞争，必须通过锁内 RMW 或原子 RMW 修复，不能只靠更强 load/store 内存序

**原因**:
- Rust 原子内存序是跨架构的并发契约，编译器负责在 RISC-V / x86 / ARM 上生成对应指令。
- 按架构分叉会隐藏真实同步语义，并增加未来平台遗漏修复的风险。
- Q17 当前目标是消除 SMP 正确性风险，不是为某个 CPU 手写 fence 微优化。

**影响**:
- `tx_copier_active`、`tx_staged_bytes` 的内存序选择可在 `uart_16550` crate 内保持平台无关。
- StarryOS `ArceOsUartPort::update_ier()` 的 IER cache 必须和 UART MMIO 写入形成单一同步边界。
- Q19B 后新增的 D1 `ArceOsD1UartPort::update_ier()` 也实现同一 `UartPort` 契约；D1 单核结果不能证明 SMP 正确性，但实施 Q17 时应明确是否同步收敛该路径。
- QEMU 单 hart 通过不再作为 SMP 内存序正确性的充分证据；真板或 QEMU SMP 仍需复验。

**替代方案**:
- ❌ 针对 RISC-V 手写 fence、其他架构保留 Relaxed：语义分裂，维护成本高。
- ❌ 全部改成 `SeqCst`：可读性差，成本更高，且不能修复非原子 RMW 覆盖问题。
- ✅ 按字段同步语义选择最小足够内存序：当前方案。

**参考**:
- `.claude/analysis/q17-smp-memory-ordering.md`
- `openspec/specs/optimization/spec.md` O63

#### Scenario: Q17 atomic ordering review

- **WHEN** Q17 changes an async UART atomic field
- **THEN** the selected ordering MUST be justified by the field's role: telemetry, state publication, RMW progress counter, or compound snapshot
- **AND** per-architecture ordering branches MUST NOT be introduced unless a future ADR proves a platform-specific hardware erratum requires them

<!-- A043 -->
### Requirement: ADR-043: Lichee RV Dock 采用 Android boot image + D1 polling early console 分阶段适配

StarryOS Lichee RV Dock bring-up MUST start from an Android boot image smoke test and a D1 UART0 polling early console before enabling PLIC, timer, async TTY, storage, USB, or benchmark workloads.

**日期**: 2026-06-28
**状态**: 已接受（适配方案阶段）
**决策**:
- 沿用官方启动链 `BOOT0 -> OpenSBI -> U-Boot -> Android boot image`，优先生成可由当前 U-Boot `bootm` 加载的 Android boot image。
- 第一阶段 kernel load/link 基线使用官方 boot header 中的 `0x40200000`。
- 第一阶段 console 使用 D1 UART0 polling early console：base `0x02500000`，stride 4，32-bit MMIO，baud 115200。
- 第一阶段不启用 UART IRQ、async TTY、rootfs、USB、SD/MMC 或 benchmark。
- D1 UART 不能直接复用当前 QEMU NS16550 byte-addressed 假设；完整 async UART 适配需在 smoke test 后再扩展 `uart_16550` backend 或新增 DW APB UART 适配层。

**原因**:
- Lichee RV Dock 官方镜像已确认 boot 分区为 Android boot image，U-Boot 环境变量已确认从 boot 分区加载到内存后 `bootm`。
- D1 UART 是 DesignWare APB UART，公开 DTS 与真板采集均指向 stride 4、32-bit MMIO；当前 StarryOS QEMU UART 初始化路径使用 `0x10000000` 和 stride 1，不能只改 base 地址。
- first-byte serial output 是真板 bring-up 的最小可观测闭环，能把启动链、链接地址和 UART 访问模型问题与后续子系统隔离。

**影响**:
- Lichee RV Dock 后续适配应优先提交平台骨架、boot image 工具链和 early console，而不是先接入 benchmark。
- Q17 SMP 验证不能依赖 Lichee RV Dock；该板仅用于单核真板流程、启动链和串口适配演练。
- 如果 M3 smoke test 失败，排查范围优先限定为 boot image、link/load 地址、UART base/stride/access-width，不应立刻扩大到 async TTY 或文件系统。

**替代方案**:
- ❌ 直接把完整 StarryOS async UART benchmark 打包烧录：变量过多，失败不可定位。
- ❌ 只修改 QEMU UART base 为 `0x02500000`：忽略 D1 UART 32-bit MMIO 访问宽度，风险高。
- ❌ 改 U-Boot 环境变量加载裸镜像：增加恢复风险，第一阶段无必要。
- ✅ Android boot image + D1 polling early console + milestone gate：当前方案。

**参考**:
- `.claude/analysis/lichee-rv-dock-adaptation-plan.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md]`
- `.claude/analysis/lichee/public-platform-notes.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/lichee/public-platform-notes.md]`
- `docs/licheerv-dock-bringup.md`
- `openspec/specs/learned/spec.md` L213-L216

#### Scenario: Lichee RV Dock early bring-up

- **WHEN** StarryOS starts a Lichee RV Dock bring-up milestone
- **THEN** the first runnable target MUST be a boot-image smoke test with D1 UART0 polling early console output
- **AND** async TTY, UART IRQ, rootfs, USB, SD/MMC, shell, and benchmark workloads MUST remain disabled until polling serial output is confirmed

<!-- A044 -->
### Requirement: ADR-044: 多平台适配采用 build-time platform descriptor 分离平台事实、驱动能力与启动策略

StarryOS board-specific constants MUST be centralized behind a build-time platform descriptor or equivalent platform module before adding Lichee RV Dock or VisionFive2 async UART support.

**日期**: 2026-06-28
**状态**: 已接受（架构重构前置决策）
**决策**:
- StarryOS 复用现有 `MYPLAT` / `PLAT_CONFIG` / `axconfig` / `axplat` 机制选择平台，不另起一套平台选择系统。
- StarryOS 自己新增轻量 platform descriptor，集中表达 async UART 和 bring-up 需要但 axconfig 不完整表达的事实：UART kind、base、irq、register stride、MMIO access width、early console strategy、boot image strategy。
- `kernel/src/drivers/uart_init.rs` 不得继续作为板级常量来源；它只能消费 platform descriptor，并负责 async UART 初始化。
- 真板 bring-up 必须先使用 polling early console。async UART、PLIC、timer、rootfs、USB、benchmark 在 early console 可观测后逐步接回。
- D1 / VisionFive2 等 DW APB UART 平台必须显式表达 32-bit MMIO access width；不能只把 NS16550 stride 从 1 改成 4。

**原因**:
- 当前 `uart_init.rs` 硬编码 QEMU UART base `0x10000000`、stride 1、raw LSR `base+5`，这会让 `make MYPLAT=...` 仍然访问 QEMU 地址。
- 构建系统已经有 `MYPLAT`、`PLAT_CONFIG`、`.axconfig.toml` 和 `axconfig-gen`，应顺势接入，而不是在驱动里堆条件编译常量。
- axconfig 能表达 memory / PLIC / UART base / IRQ，但不能完整表达 `ConsoleKind`、`reg_width` 和 boot image strategy。
- `uart_16550` 当前 MMIO backend 只有 stride 参数，底层 volatile access width 是 `u8`。D1 / VisionFive2 的 DW APB UART 需要 32-bit MMIO 访问模型。

**影响**:
- Lichee RV Dock 和 VisionFive2 适配可共享同一套平台边界：descriptor + early console + async UART backend。
- 上层 TTY、line discipline、`/dev/console` 不需要感知 UART base / stride / width。
- 后续若引入 DTB 解析，也应先解析成同一个 descriptor，而不是让各驱动分散解析 DTB。
- QEMU 现有行为可作为第一个 descriptor 复刻，降低重构风险。

**替代方案**:
- ❌ 在 `uart_init.rs` 内按平台写 `#[cfg(feature = "...")]` 常量：短期快，但会继续污染驱动层。
- ❌ 完整运行时 DTB 解析作为第一步：过重，会推迟 early console 可观测闭环。
- ❌ 仅复用 axplat ConsoleIf：适合 polling console，不足以表达 async UART ring buffer、waker 和 IRQ 控制。
- ✅ build-time platform descriptor + early console + 分阶段接回 async UART：当前方案。

**参考**:
- `.claude/analysis/platform-parameter-decoupling.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/platform-parameter-decoupling.md]`
- `.claude/analysis/lichee-rv-dock-adaptation-plan.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md]`
- `openspec/specs/learned/spec.md` L217-L220

#### Scenario: Board constants are centralized

- **WHEN** a StarryOS driver needs board-specific base addresses, IRQ numbers, register stride, register width, or boot image parameters
- **THEN** it MUST read them from the platform descriptor or equivalent centralized platform module
- **AND** it MUST NOT introduce new board-specific constants directly inside driver initialization code

<!-- A045 -->
### Requirement: ADR-045: D1 真板正路径必须接入 D1 axplat 启动层

StarryOS Lichee RV Dock bring-up MUST replace the QEMU axplat boot path with a D1-specific axplat before expecting StarryOS `entry.rs` smoke code to run.

**日期**: 2026-06-28
**状态**: 已接受（Q19 正路径方案）
**决策**:
- 新增或接入本地 `axplat-riscv64-lichee-d1`，由它负责 `_start`、早期页表、MMU enable、内存布局、D1 polling console 和平台初始化。
- Lichee 构建必须通过 `MYPLAT` / `PLAT_CONFIG` 选择 D1 axplat；禁止继续使用 `axfeat/defplat` 的 QEMU virt 启动层。
- Host-side Gate 必须检查 `readelf` / linker script / `objdump`：entry 与 linker base 应为 D1 高半区地址，boot symbols 必须是 D1 axplat 而不是 QEMU axplat。
- StarryOS `kernel/src/platform/*` descriptor 只表达 StarryOS 驱动层事实；它不能替代 axplat 启动层。

**原因**:
- 最新板测中 U-Boot 已识别 `d1-nezha` Android boot image 并跳转，但没有 StarryOS 输出，说明问题位于 payload 早期执行路径。
- 当前 ELF 仍以 `0xffffffc080200000` 为入口并调用 `axplat_riscv64_qemu_virt::boot`，与 D1 `0x40200000` 物理加载和 `0xffffffc040200000` 高半区预期不一致。
- `axruntime` 会在 StarryOS `entry::init` 前运行；如果 axplat console、页表或 MMU 设置错误，`[starry-d1] early boot` 永远不会打印。

**影响**:
- Q19 后续实现重点从 `entry.rs` smoke 分支转向本地 D1 axplat crate。
- D1 polling console 要放在 axplat 层，至少在 axruntime 早期日志前可用；StarryOS 层 smoke console 只能作为第二层确认。
- `DWARF=n` 仍是 Lichee boot image 打包的强制 Gate，直到 raw binary debug section 控制被正式解决。

**替代方案**:
- ❌ 只覆盖 `KERNEL_BASE_PADDR`：不能替换 QEMU `_start`、页表、console 与 MMIO 范围。
- ❌ 只在 `kernel/src/platform/lichee_d1.rs` 增加常量：该层运行太晚，无法修复 axplat 早期失败。
- ❌ 直接复用 VisionFive2 axplat：RAM 起点相近，但 hart-id、UART、PLIC 和 console access width 都不是 D1。
- ✅ 本地 `axplat-riscv64-lichee-d1` + artifact inspection + Android boot smoke：当前方案。

**参考**:
- `.claude/analysis/d1-axplat-bringup-plan.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/d1-axplat-bringup-plan.md]`
- `.claude/analysis/platform-parameter-decoupling.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/platform-parameter-decoupling.md]`
- `openspec/specs/learned/spec.md` L221-L222

#### Scenario: D1 boot path verification

- **WHEN** StarryOS builds a Lichee RV Dock boot image
- **THEN** host inspection MUST prove the boot path references `axplat_riscv64_lichee_d1`
- **AND** it MUST NOT reference `axplat_riscv64_qemu_virt::boot`
- **AND** board flashing MUST be deferred if the generated linker base, ELF entry, or Android boot image address do not match the D1 contract

<!-- A046 -->
### Requirement: ADR-046: D1/C906 early page table 必须设置 T-Head normal-memory PTE flags

StarryOS Lichee RV Dock early boot page table MUST mark DDR mappings with T-Head C9xx normal-memory attributes before entering `axruntime` / `percpu` initialization.

**日期**: 2026-06-28
**状态**: 已接受（Q19 板测修复）
**决策**:
- D1 axplat early boot 的 DDR identity mapping 与 high-half mapping 使用 `PTE_DDR = 0xef | (1 << 60) | (1 << 61) | (1 << 62)`。
- 低地址 / MMIO bootstrap mapping 暂不套用 cacheable normal-memory 属性，避免把 UART / PLIC / timer 等设备区错误标记为普通内存。
- `Store/AMO access fault` 如果发生在 `.bss` / percpu 原子操作附近，默认按 D1/C906 memory attribute 问题优先排查。
- 后续最终页表也必须继承等价的 `xuantie-c9xx` / T-Head C9xx 属性；early page table 修复不代表最终页表已经安全。

**原因**:
- 真板日志已经进入 StarryOS payload：U-Boot 识别 `d1-nezha`，打印 `Starting kernel ...` 后触发 `Store/AMO access fault`。
- `EPC ffffffc040244648` 符号化为 `percpu::imp::init` 中的 `amoor.w.aqrl`，`TVAL ffffffc0402c6908` 对应 `.bss` 符号 `percpu::imp::IS_INIT`。
- 项目依赖中的 `ax-page-table-entry` 对 `xuantie-c9xx` 明确使用 `SH = 1<<60`、`B = 1<<61`、`C = 1<<62` 表示 normal memory。

**影响**:
- Q19 下一次板测应重编带该 PTE 修复的镜像并重新写入 boot 分区。
- 如果修复后串口能继续输出，说明 D1 axplat 已跨过 `axruntime`/`percpu` 早期障碍。
- 如果后续在内存管理初始化后再次 fault，应检查最终 kernel address space 的 PTE 属性，而不是回退到 boot image 格式或官方 Linux 采集。

**替代方案**:
- ❌ 禁用 percpu / atomics：规避症状，不解决 D1/C906 内存属性要求。
- ❌ 把整个 0..1G 都标记为 normal memory：可能误标设备 MMIO。
- ✅ 仅对 DDR `0x40000000..0x80000000` 和高半区 DDR 镜像设置 `SH|B|C`：当前方案。

**参考**:
- `crates/axplat-riscv64-lichee-d1/src/boot.rs`
- `openspec/specs/learned/spec.md` L229-L230
- `.claude/analysis/d1-axplat-bringup-plan.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/d1-axplat-bringup-plan.md]`

#### Scenario: D1/C906 AMO fault diagnosis

- **WHEN** Lichee RV Dock 在 `Starting kernel ...` 后报告 `Store/AMO access fault`
- **THEN** MUST 先符号化 EPC/TVAL，确认是否落在 DDR `.bss` / percpu / atomic 路径
- **AND** MUST 检查 early page table 和 final page table 是否设置 T-Head C9xx normal-memory PTE flags

<!-- A047 -->
### Requirement: ADR-047: Q19B 先嵌入 benchmark payload，再追求 SDMMC/rootfs parity

StarryOS Lichee RV Dock benchmark bring-up MUST first reach async UART benchmark data through staged platform modes and an embedded benchmark payload; SDMMC/rootfs parity MUST NOT block the first Q19B benchmark dataset.

**日期**: 2026-06-29
**状态**: 已接受（Q19B 探索结论）
**决策**:
- Q19B 从 Q19A smoke-only 路径拆出显式模式：smoke、kernel benchmark、user benchmark。
- 先实现 D1-safe async UART path（DW APB UART stride 4 / 32-bit MMIO）与真实 PLIC UART IRQ 18，再进入 `/dev/console` 和用户态 benchmark。
- 第一个用户态 benchmark 数据优先通过 embedded benchmark ELF 或小 initramfs 获取，复用现有 `load_user_app`/ELF loader 能力，避免先实现完整 SDMMC/rootfs。
- SDMMC/rootfs 从 TF 卡运行 `/bin/benchmark` 是后续 parity 阶段，不作为 Q19B 首个成功标准。
- QEMU benchmark 数据和 D1 真板数据必须分栏记录；QEMU 不仿真物理串口线延迟，不能覆盖 D1 结果。

**原因**:
- 当前 `entry.rs` 在 `feature = "lichee-d1"` 时直接进入 `run_lichee_d1_smoke() -> !`，完整 QEMU 用户路径被绕开。
- QEMU 用户 benchmark 依赖 `/dev/console`、`tcdrain`、`FIONBIO`、`clock_gettime` 和用户 ELF 加载；这些比 Q19A smoke 多出多个可独立失败的子系统。
- D1 当前没有 StarryOS SDMMC/rootfs 路径。若把 SDMMC 作为前置，无法区分 block bring-up 失败和 async UART/TTY 失败。
- `crates/axplat-riscv64-lichee-d1/src/irq.rs` 已有真实 PLIC 实现，但顶层 Lichee feature 当前只启用 `irq-if` stub；这适合单独作为 Q19B-M2 Gate。

**影响**:
- Q19B milestone 必须按 mode split → D1 async UART backend → PLIC/UART IRQ → kernel benchmark → `/dev/console` → embedded user benchmark → optional SDMMC/rootfs 的顺序推进。
- `tests/benchmark.c` 应保持与 QEMU source-compatible；差异应在 payload delivery 和平台 feature 中处理。
- 文档和结果保存路径应区分 `lichee-kbench` 与 `lichee-userbench`，原始串口日志建议保存到 `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee/q19b-*.txt`（当前为归档目录，活跃采集可走此路径或新建 `_archive/<日期>-新批次>/lichee/`）。

**替代方案**:
- ❌ 直接启用完整 qemu feature set：会重新引入 block/PCI/virtio 假设，已经导致 `No block device found` 与 PCI 常量缺失。
- ❌ 先做 SDMMC/rootfs：工程价值高，但会延迟第一个 async UART 真板数据，并扩大排障面。
- ❌ 只跑 `drivers::bench::run_startup_benchmark()` 就声明完成：它只覆盖 ring buffer/driver 层，不证明用户态 syscall、TTY、tcdrain、FIONBIO。
- ✅ 嵌入 benchmark ELF，先得到 D1 用户态 `/dev/console` 数据，再补 SDMMC/rootfs parity：当前方案。

**参考**:
- `.claude/analysis/q19b-lichee-benchmark-plan.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/q19b-lichee-benchmark-plan.md]`
- `openspec/specs/learned/spec.md` L236-L239
- `tests/benchmark.c`
- `kernel/src/entry.rs`

#### Scenario: Q19B first benchmark dataset

- **WHEN** Q19B aims to collect the first Lichee RV Dock async UART benchmark data
- **THEN** it MUST run through a staged mode that has D1 async UART, PLIC IRQ, `/dev/console`, and a user benchmark payload
- **AND** it SHOULD use an embedded benchmark ELF before SDMMC/rootfs is available
- **AND** it MUST NOT count kernel ring benchmark alone as the final Q19B user benchmark gate

<!-- A048 -->
### Requirement: ADR-048: D1 先做平台专用 UartPort，后考虑 uart_16550 width-aware backend

D1 DW APB UART (stride 4, 32-bit MMIO) 的异步栈入口 MUST 先通过 D1 专用 `ArceOsD1UartPort` 实现，禁止在当前阶段修改 `uart_16550::MmioBackend`（外部 crate 约束）。

**日期**: 2026-06-29
**状态**: 已落地（Q19B Phase 2）
**决策**:
- 在 `kernel/src/drivers/d1_uart.rs` 中实现 `ArceOsD1UartPort`，直接通过 32-bit `read_volatile`/`write_volatile` 访问 DW APB UART 寄存器（stride 4），实现 `uart_16550::async_::driver::UartPort` trait。
- 同时提供 D1 专用 ISR handler (`d1_uart_isr_handler`)，在 IIR 读取时使用 stride-aware 32-bit access，复用 `uart_16550::async_::isr` 的全局 waker (`RX_WAKER`/`TX_WAKER`/`DRAIN_WAKER`)。
- QEMU 路径的 `ArceOsUartPort`（封装 `Uart16550<MmioBackend>`，U8 byte access）完全保留不变。
- D1 路径和 QEMU 路径通过 `#[cfg(feature = "lichee-d1-kbench")]` 条件编译互斥，共享相同的 `AsyncUartDriver` 类型系统但使用不同的 `UartPort` 实现。
- 长期方案：VisionFive2 也有类似 access-width 需求，可在 D1 benchmark 验证后提取 width-aware backend 到 `uart_16550` crate。

**原因**:
- `uart_16550::Uart16550<MmioBackend>` 内部做 U8 `read_volatile`/`write_volatile`，这是 NS16550 的标准行为。D1 的 DW APB UART 要求 stride 4 + 32-bit MMIO，不兼容当前 backend。
- 项目规则：「不修改任何外部 crate」— `uart_16550` 是外部 crate。当前阶段不适合改动 `MmioBackend`。
- D1 专用实现风险最小：162 行自包含代码，不碰 uart_16550 内部，`UartPort` trait 只有 4 个方法（`receive_bytes`/`send_bytes`/`transmitter_empty`/`update_ier`），接口简洁。

**影响**:
- `kernel/src/drivers/uart_init.rs` 通过 feature gate 维护双路径（QEMU `ArceOsUartPort` + D1 `ArceOsD1UartPort`），各自有独立的类型别名 (`ArceOsDriver`/`ArceOsReader`/`ArceOsWriter`) 和 ISR wrapper。
- `kernel/src/lib.rs` 和 `kernel/src/drivers/mod.rs` 的 feature gate 需要精确控制哪些模块在 kbench 模式下可用（排除 `file`/`pseudofs`/`mm`/`syscall`/`task`/`time`/`ntty_async`，因为它们依赖 `axfs`/`axdisplay`）。
- `NonNull<u8>` 需要 `unsafe impl Send + Sync` 才能放入 `lazy_static!`（内核态下 MMIO base pointer 是 immutable constant）。

**替代方案**:
- ❌ 扩展 `uart_16550::MmioBackend` 支持 access width：违反「不修改外部 crate」规则。
- ❌ 通过 uart_16550 的 `Backend` trait 添加 width-aware 实现：Backend trait 是 sealed，无法外部实现。
- ❌ 在 QEMU 路径中也使用 32-bit MMIO：NS16550 在 QEMU 上 stride 1 / U8 access 已验证且性能良好，没有必要。
- ✅ D1 专用 `UartPort` + 复用 `AsyncUartDriver`/waker 体系：当前方案。

**参考**:
- `kernel/src/drivers/d1_uart.rs`（新增，162 行）
- `kernel/src/drivers/uart_init.rs`（重构，双路径 feature gate）
- `kernel/src/platform/early_console.rs` — `DwApbUart32EarlyConsole::putchar`（DW APB UART 32-bit MMIO 参考）
- `kernel/src/platform/lichee_d1.rs` — `LICHEE_D1` descriptor（stride 4 / U32）

#### Scenario: D1 async UART register access

- **WHEN** Lichee D1 benchmark mode initializes async UART
- **THEN** it MUST use the D1-specific `ArceOsD1UartPort` for register access
- **AND** it MUST access DW APB UART registers through stride-aware 32-bit volatile MMIO
- **AND** it MUST NOT route D1 UART access through the QEMU `Uart16550<MmioBackend>` byte-MMIO path

<!-- A049 -->
### Requirement: ADR-049: Q19B Phase 5-6 通过最小 axfs-ng patch 解阻

StarryOS D1 userbench 模式 (`lichee-d1-userbench`) 的 `/dev/console` TTY gate 和 embedded user benchmark payload 阶段 MUST 使用最小 D1 runtime 和 patched `axfs-ng`，避免重新引入 QEMU PCI/virtio/display/rootfs 假设。

**日期**: 2026-06-29
**状态**: ✅ 已落地（Q19B Host Gate）
**决策**:
- Q19B userbench 启用 `dep:axfs` 和 `axfeat/task-ext`，恢复 `pseudofs::mount_all()`、`ASYNC_TTY`、`FD_TABLE`、用户任务与 syscall 最小路径。
- 不启用 QEMU `qemu` feature；D1 路径继续排除 net socket、fb/axdisplay、virtio/display 等硬件无关模块。
- 通过 `[patch.crates-io] axfs-ng = { path = "crates/axfs-ng" }` 本地化 `axfs-ng`，仅修改其 `axdriver` 依赖为 `default-features = false, features = ["block", "bus-mmio"]`。
- D1 userbench 通过 embedded `benchmark.elf` 获取首个用户态 benchmark 路径，不要求 SDMMC/rootfs parity。
- `make lichee-userbench` 必须作为烧录前 gate，而不仅是 `cargo check`。

**原因**:
- `pseudofs::mount_all()` 与 `add_stdio()` 需要 `axfs::FS_CONTEXT`，完全绕开 `axfs` 会扩大重构面。
- 原始 `axfs-ng` 依赖 `axdriver` 时未关闭默认 feature，而 `axdriver` default 是 `bus-pci`；D1 没有 `PCI_ECAM_BASE` / `PCI_RANGES` / `PCI_BUS_END`。
- `axdriver` 的 `build.rs` 在未启用自身 `bus-mmio` feature 时会默认输出 `cfg(bus="pci")`，所以只在 root feature 添加 `axfeat/bus-mmio` 不足以影响 `axfs-ng` 的间接 `axdriver`。

**影响**:
- `make lichee-userbench` 已生成可写入 boot 分区的 `starry-lichee-userbench-boot.img` (`kernel_size=876736`)。
- 本地 `crates/axfs-ng` 是 deliberate patch，不是无意 vendor 膨胀；后续升级 axfs-ng 时必须重新核对 `axdriver` feature。
- 真板仍需验证 `/dev/console`、`tcdrain`、FIONBIO、用户态 benchmark 输出；host gate 通过不等于 Q19B 最终完成。

**替代方案**:
- ❌ 直接启用 `qemu` feature：会带入 PCI/virtio/display/net 假设，扩大排障面。
- ❌ 给 D1 axconfig 填假 `PCI_*` 常量：只欺骗编译器，运行时可能访问不存在的 PCI ECAM。
- ❌ 完全重写最小 devfs：短期可行但重构面大，偏离 benchmark 数据目标。
- ✅ patch `axfs-ng` 的 `axdriver` 依赖并保持 embedded benchmark：当前方案。

**参考**:
- `crates/axfs-ng/Cargo.toml` — patched `axdriver` dependency
- `kernel/src/pseudofs/mod.rs:61` — `mount_all()` 依赖 `axfs::FS_CONTEXT`
- `kernel/src/entry.rs` — `lichee_d1_init()` 中 Phase 5-6 TODO
- `openspec/changes/q19b-lichee-d1-benchmark/tasks.md` — Phase 5-6 deferred 标记

#### Scenario: D1 userbench reaches devfs work

- **WHEN** Q19B proceeds from kbench to userbench
- **THEN** it MUST use a D1 feature set that includes `axfs` and `task-ext` without enabling QEMU PCI/virtio/display assumptions
- **AND** patched `axfs-ng` MUST enable `axdriver/block` and `axdriver/bus-mmio` with `default-features = false`
- **AND** `make lichee-userbench` MUST pass before board flashing

<!-- A050 -->
### Requirement: ADR-050: Q19B feature 必须区分硬件能力与运行模式

StarryOS Lichee D1 benchmark features MUST NOT use a kbench-only runtime feature as the parent of userbench if that feature also excludes user/process/filesystem modules.

**日期**: 2026-06-29
**状态**: ✅ 已落地（Q19B-Next.1）
**决策**:
- 后续 Q19B 继续推进前，必须重新梳理 Lichee feature 语义，把 D1 async UART / PLIC 这类硬件能力与 smoke/kbench/userbench 这类运行模式分开。
- `lichee-d1-userbench` 可以复用 D1 async UART 和 PLIC 能力，但不能继承会排除 `ASYNC_TTY`、`file`、`mm`、`pseudofs`、`task`、`syscall`、`time` 的 kbench-only feature。
- userbench 的第一 host gate 是 `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench` 通过，且不得通过直接启用完整 QEMU PCI/virtio/display 假设来绕过。
- `/dev/console` gate 必须先于 embedded benchmark ELF；只有 `/dev/console` 和 async TTY 通路成立后，才加载完整 `tests/benchmark.c` payload。

**原因**:
- 当前 `kernel/Cargo.toml` 定义 `lichee-d1-userbench = ["lichee-d1-kbench"]`。
- 当前 `kernel/src/lib.rs` 和 `kernel/src/drivers/mod.rs` 用 `#[cfg(not(any(feature = "lichee-d1-smoke", feature = "lichee-d1-kbench")))]` 排除用户态路径模块。
- 实测 `cargo check --target riscv64gc-unknown-none-elf --features lichee-d1-userbench` 失败，缺失 `crate::drivers::ASYNC_TTY`、`crate::file`、`crate::mm`、`crate::pseudofs`、`crate::task`、`axfs`、`axtask::AxTaskExt`。

**影响**:
- Q19B 当前 Phases 0-4 可以作为 kbench 交付继续保留；Phases 5-6 需要单独做 feature/runtime 边界修正。
- 后续实现应优先新增或重命名 feature，使 kbench-only exclusion 不影响 userbench。
- 这条 ADR 不要求立即实现 D1 SDMMC/rootfs；embedded benchmark 仍然是首个用户态数据路径的推荐方式。

**替代方案**:
- ❌ 继续让 userbench 继承 kbench-only feature：会持续排除 userbench 必需模块。
- ❌ 直接启用 `qemu` feature：会重新带入 PCI/virtio/display/rootfs 假设，扩大排障面。
- ✅ 拆分硬件能力 feature 与运行模式 feature，再构建最小 D1 userbench runtime：当前方案。

**参考**:
- `.claude/analysis/q19b-current-blockers.md` `[ARCHIVED 2026-07-04 → _archive/2026-07-04-q19-lichee-analysis/q19b-current-blockers.md]`
- `kernel/Cargo.toml`
- `kernel/src/lib.rs`
- `kernel/src/drivers/mod.rs`

#### Scenario: D1 userbench feature selection

- **WHEN** `lichee-d1-userbench` is enabled
- **THEN** it MUST include D1 async UART and PLIC capability
- **AND** it MUST keep the user/process/filesystem modules required by the benchmark runtime visible
- **AND** it MUST NOT inherit a kbench-only feature that excludes `ASYNC_TTY`, `file`, `mm`, `pseudofs`, `task`, `syscall`, or `time`

<!-- A051 -->
### Requirement: ADR-051: D1 async UART drain 必须兼容 THRE 边沿丢失

D1 DW APB UART async backend MUST treat THRE/TEMT readiness as both interrupt-driven and state-driven; drain waiters MUST NOT rely solely on future THRE interrupts.

**日期**: 2026-06-29
**状态**: ✅ 已落地（Q19B 真板 userbench）
**决策**:
- D1 `ArceOsD1UartPort` 在启用 `IER::THR_EMPTY` 后，如果 LSR 已显示 THRE/TEMT，必须立即软件 wake `TX_WAKER` / `DRAIN_WAKER`。
- D1 ISR 读取 IIR 时必须识别 bit0=1 的 no-pending 状态；no-pending 不是有效中断类型，但可基于 LSR 的 THRE/TEMT 补一次 TX/drain wake。
- `flush()` 与 `sys_ioctl(TCSBRK/tcdrain)` 必须注册 `DRAIN_WAKER`，不能只注册 TX ring waker；因为数据被 TX copier pop 出 ring 后，后续状态变化发生在 staged buffer 和 UART FIFO/TEMT。
- TX copier 在最后一批数据送入 UART 且确认 transmitter empty 后必须 wake `DRAIN_WAKER`。

**原因**:
- QEMU NS16550 模型稳定产生 THRE 中断，曾掩盖“启用 THRE 时硬件已经 ready 但不再产生新边沿”的真板窗口。
- Lichee RV Dock 真板日志显示 IRQ 18 可进入，但 IIR 多次为 `0xc1`（no pending），有效 THRE `0xc2` 只偶发。
- userbench 首次卡在 64B write 后的 `tcdrain`，说明 `/dev/console`、write、TX ring 都成立，卡点在 staged/TEMT drain 唤醒。

**影响**:
- D1 userbench 已完整跑完 embedded benchmark，`tcdrain` 不再卡住。
- 真板大包 TX 达 97.7%~99.0% 115200bps 线速，证明 drain 等待的是实际串口发送完成。
- `uart_16550` 被本地化到 `crates/uart_16550`，以便 StarryOS 分支保存 async drain 修复；后续若回推上游，需要拆分为通用 drain 修复和 D1 backend 修复。

**替代方案**:
- ❌ 在 `tcdrain` 内轮询硬件：破坏原异步架构，且把硬件细节泄漏到 syscall 层。
- ❌ 只依赖 PLIC/UART 中断：D1 no-pending/edge-loss 窗口已由真板日志证伪。
- ✅ 保持 copier + waker 架构，在硬件 backend 和 drain 状态机补齐 ready-state wake：当前方案。

#### Scenario: Porting async UART to a real board

- **WHEN** a real UART backend enables THRE interrupts
- **THEN** it MUST also check current LSR readiness and wake TX/drain waiters if THRE/TEMT is already true
- **AND** `tcdrain` / `flush` waiters MUST be registered on the drain completion path, not only on the TX ring space path

<!-- A052 -->
### Requirement: ADR-052: Q19C 完整 StarryOS benchmark 先走 memory-root path loader，再做 SDMMC/rootfs parity

Lichee RV Dock 上的完整 StarryOS benchmark MUST first prove the normal VFS/path-based user loading path with a populated memory root before treating real SDMMC/rootfs bring-up as the blocking requirement.

**日期**: 2026-07-02
**状态**: 🔍 探究结论（待 Q19C change 落地）
**决策**:
- 保留 Q19B embedded `benchmark.elf` 作为 D1 async UART/userland regression gate。
- 新增 Q19C fullbench runtime mode 时，先在 D1 memory root 中提供 `/bin/benchmark`，通过 `load_user_app()` 和 `FS_CONTEXT.resolve()` 启动 benchmark，而不是继续调用 `load_embedded_user_app()`。
- 只有 memory-root path loading 通过后，才进入真实 SDMMC/block rootfs parity；SDMMC bring-up 失败不得回归为 UART/TTY benchmark 阻塞。
- D1 fullbench 不得启用 `qemu` feature；必须保持独立 feature set，继承 D1 async UART/PLIC 能力但排除 QEMU PCI/virtio/display 假设。

**原因**:
- Q19B 已证明 `/dev/console`、TTY、syscall、`tcdrain` 和 FIONBIO，但 embedded ELF 绕过了 rootfs/path loader。
- QEMU 完整路径依赖 `load_user_app()` 从 `FS_CONTEXT` 解析 `/bin/sh` 或 benchmark 路径；这是 Q19B 尚未覆盖的差距。
- 真实 SDMMC/rootfs 需要 D1 block driver、clock/reset/pinmux/cache 等硬件 bring-up，排障面大，不能作为验证 normal loader path 的第一步。

**影响**:
- Q19C 可以拆成一个前置证据清理 gate 加两个可验证工程阶段：benchmark evidence cleanup、memory-root fullbench 和 SDMMC/rootfs parity。
- benchmark 数据仍按 QEMU / D1 embedded / D1 fullbench 分栏记录，避免覆盖不同测试条件。
- 后续 OpenSpec change 应新增 `lichee-d1-fullbench` 或等价 make target，并保留 `make lichee-userbench` 不退化。

**替代方案**:
- ❌ 直接做 SDMMC/rootfs：工程价值高，但会把 block bring-up 和 user loader parity 绑在一起，降低定位效率。
- ❌ 复用 QEMU feature：会重新引入 PCI/virtio/display/rootfs 假设，不符合 D1 硬件事实。
- ✅ benchmark evidence cleanup -> memory-root path loader -> shell/script optional -> SDMMC/rootfs parity：当前推荐路线。

#### Scenario: D1 fullbench path loading

- **WHEN** Q19C starts full StarryOS benchmark work on Lichee RV Dock
- **THEN** the first fullbench gate MUST run benchmark through `load_user_app()` from a VFS-visible `/bin/benchmark`
- **AND** `load_embedded_user_app()` MUST remain only the Q19B regression path
- **AND** real SDMMC/rootfs parity MUST be a later gate after memory-root path loading succeeds

<!-- arc: ARC-202607081429 --> 4 条已归档 (2026-07-08) → ../changes/archive/2026-07-08-ARC-202607081429/proposal.md
