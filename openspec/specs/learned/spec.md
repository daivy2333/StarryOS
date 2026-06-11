# Spec: learned — 项目学习记忆

## Purpose

记录 StarryOS 异步串口项目开发过程中积累的关键知识（API 路径、文件速查、踩坑档案、技巧模式），按主题分组。每条 MUST 能被 `grep` 精确定位，避免重复探索。

## Requirements

### Requirement: API 路径速查表

异步串口相关 API 路径与关键函数 MUST 在本规范中保留可搜索路径，开发者在查询 / 调用时 MUST 用 `grep` 定位而不是重新阅读源码。

**核心 API 路径**（按主题分组）：

**异步任务原语（axtask）**

| 路径 | 用途 | 备注 |
|------|------|------|
| `axtask::future::block_on` | 异步任务阻塞执行 | — |
| `axtask::future::poll_io` | WouldBlock → register → await 标准模式 | — |
| `axtask::future::register_irq_waker` | 连接中断到异步任务唤醒 | Q0~Q7 旧机制，已被 AtomicWaker 替代（见历史决策） |
| `embassy_sync::AtomicWaker::wake` | ISR 中安全唤醒 Waker，无锁中断安全 | 当前方案 |

**uart_16550 crate（本项目本地依赖）**

| 路径 | 用途 |
|------|------|
| `uart_16550/src/spec.rs` | IER/ISR/LSR bitflags + InterruptType 枚举（寄存器定义汇总） |
| `uart_16550/src/backend/mod.rs` | Backend trait 定义（sealed 模式，分发 Mmio/Port I/O 后端） |
| `uart_16550/src/backend/mmio.rs` | read_volatile/write_volatile + 地址计算 |
| `uart_16550/src/lib.rs:406-523` | SerialPort::new_mmio + Config + init |
| `uart_16550/src/config.rs:114-154` | baud_rate/data_bits/interrupts/fifo_trigger_level |
| `uart_16550/src/spec.rs:315-414` | InterruptType 枚举（ReceivedDataReady/THR_EMPTY/ReceptionTimeout/LineStatus） |
| `Uart16550<MmioBackend>::new_mmio(NonNull<u8>, stride)` | RISC-V MMIO 初始化入口；stride MUST 传 1（NS16550 寄存器仅 8 字节） |
| `uart.isr().interrupt_type()` | ISR 中读取 InterruptType 枚举分发：ReceivedDataReady/ReceptionTimeout → RX，THR_EMPTY → TX |

**内核模块关键路径**

| 路径 | 用途 |
|------|------|
| `kernel/src/file/pipe.rs` | poll_io + register_irq_waker 模式参考 |
| `kernel/src/file/event.rs` | 轻量异步通知模式参考 |
| `kernel/src/pseudofs/device.rs:28-55` | DeviceOps trait 核心方法（read_at/write_at/ioctl/as_pollable/flags） |
| `kernel/src/pseudofs/dev/tty/` | TTY/ldisc/Termios 实现 |
| `kernel/src/drivers/isr.rs` | ISR handler 入口（AtomicWaker 唤醒位置） |
| `axpoll/src/lib.rs` | Pollable trait 定义（poll + register，IoEvents 标志） |
| `axmm/src/lib.rs:111-131` | `axmm::iomap()` 设备 MMIO 映射 API |
| `axruntime-0.3.0-preview.2/src/lang_items.rs` | panic handler 实现 → ax_println! → polling TX |

**辅助工具与资源**

| 路径 | 用途 |
|------|------|
| RISC-V musl 工具链 `/opt/musl/riscv64-linux-musl-cross/bin` | 编译 lwext4_rust C 代码 |
| rootfs 下载 `https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz` | 1GB 磁盘镜像 |
| `disk.img` 位置：项目根目录 + `make/disk.img` | `make run` 需要 `make/disk.img` |
| `axhal::mem::phys_to_virt` | 物理地址到虚拟地址转换（返回 VirtAddr） |

#### Scenario: 编写新的串口功能时定位 API

- **WHEN** 开发者要实现某个串口原语（ISR 唤醒、设备注册、MMIO 访问、ring buffer 操作）但不确定具体路径
- **THEN** 必须先 `grep -n "关键词" openspec/specs/learned/spec.md` 定位 API 路径，再去对应源码位置确认

### Requirement: 构建与部署环境踩坑

musl 工具链与 rootfs 部署 MUST 按本规范中的固定流程操作；任何偏离 MUST 先验证环境而非凭直觉调整。

**踩坑 1：musl 工具链缺失导致 `riscv64-linux-musl-cc: command not found`**（L68，2026-05-27）

- **症状**：`make build` 失败，提示 musl 编译器找不到
- **根因**：`lwext4_rust` crate 需要编译 C 代码，依赖 musl 交叉编译工具链
- **解**：
  1. 工具链位于 `/opt/musl/riscv64-linux-musl-cross/bin`
  2. 构建前 `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH`
  3. 系统已有其他 RISC-V 工具链，但 musl 版本是必需的
- **验证**：2026-05-27（T0.4 任务）

**踩坑 2：rootfs 下载与 disk.img 部署**（L69，2026-05-27）

- **症状**：`make rootfs` 下载失败（SSL 连接中断），`make run` 报 `disk.img not found`
- **根因**：GitHub releases 下载不稳定 + Makefile 需要 `disk.img` 在 `make/` 目录
- **解**：
  1. 手动下载 rootfs
  2. `xz -d rootfs-riscv64.img.xz`
  3. `cp rootfs-riscv64.img disk.img && cp disk.img make/disk.img`

**踩坑 3：构建警告清理边界**（L70）

- **症状**：编译有 10 个 unused warnings（dead_code）
- **原则**：这些是项目原有代码的未使用函数，**不清理**（遵循 Karpathy 原则"只改必须改的代码"）
- **影响**：不影响功能，编译成功

#### Scenario: 第一次在新机器构建

- **WHEN** 开发者换机器或重新配置环境
- **THEN** 必须按"musl 工具链 PATH → 手动 rootfs → 双 disk.img"顺序完整执行，禁止直接 `make build` 期待成功

### Requirement: 异步运行时与缓冲设计踩坑

异步运行时与 ring buffer 选型 MUST 遵循 `axtask::future + embassy-sync::AtomicWaker` 模式，硬件 FIFO 与内核 ringbuf 之间的搬运 MUST 由单一后台协程完成。

**踩坑 1：embassy-executor 与 axtask 冲突**（L10）

- **症状**：引入完整 Embassy 后调度器冲突
- **根因**：`embassy-executor` 自带调度器，与 `axtask` 不兼容
- **解**：只引入 `embassy-sync::AtomicWaker`，异步运行时使用 `axtask::future`

**踩坑 2：HeapRb 非中断安全**（L11）

- **症状**：在 ISR 中直接操作 `ringbuf` 导致数据竞争
- **根因**：`HeapRb` 的 `Producer` / `Consumer` 不是中断安全的
- **解**：硬件 FIFO 和内核 ringbuf 之间的搬运由单一后台协程完成

#### Scenario: 选型异步运行时

- **WHEN** 开发者要添加新的异步原语
- **THEN** 必须使用 `axtask::future` 调度 + `embassy_sync::AtomicWaker` 唤醒，禁止引入 `embassy-executor`

#### Scenario: 决定数据搬运位置

- **WHEN** 开发者需要在硬件 FIFO 与用户缓冲区之间搬运数据
- **THEN** MUST 由单一 copier 任务（ring buffer Producer/Consumer）搬运，ISR 禁止直接操作 ring buffer

### Requirement: UART 硬件集成踩坑

UART 集成前 MUST 验证全部关键寄存器状态（IER / IIR / LSR / MCR），禁止假设外部 crate 初始化后的状态可用；NS16550 寄存器 stride MUST 配置为 1。

**踩坑 1：M3 替换失败（方向 A 教训）**（L78）

- **核心教训**：stride=4 导致 LoadFault + Console UART 状态不兼容。后 Q0~Q7 独立实现替代。**核心原则：硬件集成前 dump 全部寄存器**

**踩坑 2：硬件调试铁律**（L79）

- **集成前必做**：`info!(IIR={:02x} MCR={:02x} LSR={:02x})`
- **参考函数**：`uart_init.rs` 中的 `log_uart_state()`

**踩坑 3：THR_EMPTY 状态理解错误**（L80）

- **库注释错误**：`uart_16550` crate 的 THR_EMPTY 注释说 "FIFO completely empty"
- **实际含义**：
  - `THR_EMPTY` (Bit 5) 表示 THR 有空位（可以写入）
  - `TEMT` (Bit 6) 表示完全空闲（包括移位寄存器）
- **纠正**：`THR_EMPTY=1` 表示 FIFO 有空位，`THR_EMPTY=0` 表示 FIFO 满
- **教训**：需仔细阅读 UART 规范，不要依赖库的注释

**踩坑 4：UART 初始化配置差异**（L108）

- **Console 配置**：`IER::DATA_READY`（只使能 RX 中断）
- **AsyncUart 配置**：`IER::DATA_READY | IER::THR_EMPTY`（RX + TX 中断）
- **关键差异**：Console 禁用 TX 中断，AsyncUart 必须使能 TX 中断
- **解决方案**：UART 重新 init 时使能 TX 中断（覆盖 Console 配置）

**踩坑 5：LSR::TRANSMITTER_EMPTY vs THR_EMPTY**（L142，2026-06-01）

- `LSR::THR_EMPTY` = bit 5：THR 可接受新字节（FIFO 有空位）
- `LSR::TRANSMITTER_EMPTY` = bit 6：THR + 移位寄存器都为空 = 真正 drain
- **踩坑**：最初误用 `LSR::TEMT`（不存在）编译失败；用 `THR_EMPTY` 会导致 tcdrain 过早返回

**踩坑 6：QEMU 16550 串口模拟的时序欺骗**（L141，2026-06-01）

- **现象**：QEMU 上 TX 吞吐量测出 150~250 MB/s，远超 115200 bps 理论值 11.5 KB/s
- **根因**：QEMU 的 NS16550 模拟不仿真真实串口线延迟（86.8 µs/byte），UART FIFO 数据处理为瞬时
- **影响**：所有基于 `tcdrain` / 轮询 LSR 的吞吐量测试在 QEMU 上均不可信
- **真板预期**：VisionFive2 @ 115200 bps → ~11.5 KB/s（受硬件波特率限制）
- **可靠指标（QEMU 也可测）**：内核态 ring buffer 速度、write() 延迟、CPU cycles/byte

#### Scenario: 在已有 UART 实例上启用新驱动

- **WHEN** 开发者准备让新驱动接管 UART
- **THEN** MUST 先调用 `log_uart_state()` 风格的诊断函数，输出 `IER` / `IIR` / `LSR` / `MCR` 全部状态，验证后才行后续

#### Scenario: 选择配置 stride

- **WHEN** 开发者初始化 `Uart16550<MmioBackend>::new_mmio(NonNull<u8>, stride)`
- **THEN** stride MUST 传 1（NS16550 寄存器仅 8 字节），禁止传 4 或其他值

### Requirement: ISR 设计踩坑与机制选型

ISR MUST 最小化（读 ISR → 禁用中断 → 唤醒 Waker → 返回），唤醒机制 MUST 在 `AtomicWaker` 与 `register_irq_waker` 之间根据场景选择。

**踩坑 1：ISR 分发机制设计要点**（L107）

- **ISR 中读 ISR 寄存器**：判断 InterruptType（ReceivedDataReady/THR_EMPTY/ReceptionTimeout）
- **禁用中断防止重入**：ISR 中临时禁用 RX/TX 中断（IER 操作）
- **AtomicWaker 精确唤醒**：rx_waker/tx_waker 分别唤醒
- **ISR 执行原则**：最小工作（读 ISR + 禁用中断 + 唤醒 waker）
- **ISR 安全约束**：无阻塞、无锁、MMIO read/write 安全

**踩坑 2：AtomicWaker vs register_irq_waker 设计选择**（L128）

| 方案 | 数据结构 | ISR 复杂度 | 适用场景 |
|------|---------|-----------|----------|
| **AtomicWaker**（本项目采用） | 静态 `AtomicWaker` 变量 | O(1)，无锁 | 固定数量的 waker（如 RX/TX 各一个） |
| **register_irq_waker**（axtask 通用方案） | `BTreeMap<usize, PollSet>` | O(log n)，需要查找 | 通用场景（如同一 IRQ 注册多个 waker） |

**本项目选 AtomicWaker 的原因**：

1. UART 驱动是专用，只有 RX/TX 两个方向，各一个 waker
2. 不需要动态注册/注销 waker
3. ISR 性能要求高（~1.5 µs），`AtomicWaker::wake()` 是原子操作，无分支
4. 代码更简洁，无需处理 BTreeMap 的并发问题

**踩坑 3：Q1 架构关键发现（AtomicWaker + critical-section）**（L122）

- **ISR 唤醒模式**：ISR 中禁用对应中断后调用 `AtomicWaker::wake()`，copier 任务中重新 enable 中断
- **critical-section**：`embassy-sync` AtomicWaker 需要 `critical-section` crate v1.0 的 `_critical_section_1_0_acquire/release` 符号，在 `lib.rs` 中用 `disable_irqs/enable_irqs` 实现
- **UnsafeCell**：多个 copier 任务共享 AsyncBuffer 需用 `UnsafeCell` 绕过 Rust 借用检查（单生产者单消费者场景安全）
- **spawn_with_name + block_on**：axtask 的 spawn 接口收 `FnOnce() + Send + 'static` closure，内部用 `block_on(future)` 包装异步逻辑

#### Scenario: 选型 ISR 唤醒机制

- **WHEN** 开发者要设计新的 ISR 唤醒路径
- **THEN** MUST 评估 waker 数量与动态性：固定少数 → AtomicWaker；通用动态 → register_irq_waker

### Requirement: Embassy 选型边界与反模式

embassy-sync 子集使用 MUST 严格限定在 `AtomicWaker`（ISR 安全唤醒）。任何 embassy 其它原语（Channel / Mutex / Watch / Semaphore / Signal）替换现有 Rust 标准库或项目原语的提案 MUST 视为反模式。

**项目采用 embassy 的实际范围**（2026-06-05 评估）：

| 类型 | 是否使用 | 用途 |
|------|----------|------|
| `embassy_sync::waitqueue::AtomicWaker` | ✅ 核心 | 3 个静态 waker（RX/TX/DRAIN）|
| `embassy_sync::mutex::Mutex` | ❌ 不用 | — |
| `embassy_sync::semaphore::Semaphore` | ❌ 不用 | — |
| `embassy_sync::channel::Channel` | ❌ 不用 | — |
| `embassy_sync::signal::Signal` | ❌ 不用 | — |
| `embassy_sync::watch::Watch` | ❌ 不用 | — |
| `embassy_sync::pipe::Pipe` | ❌ 不用 | — |
| `embassy_time::Timer` | ❌ 不用 | 待 O47 评估 |
| `embassy_futures::select!` | ❌ 不用 | 架构冲突 |
| `embassy-executor` | ❌ 不用 | 与 axtask 冲突（L10 已记录）|

**踩坑 1：Channel 替换 HeapRb 的反优化**（L81，2026-06-05）

- **反优化**：`embassy_sync::Channel<u8, 65536>` 替换 `ringbuf::HeapRb<u8>`
- **现状**：UART RX/TX 用 `ringbuf::HeapRb<u8>`，SPSC 无锁
- **不要做的原因**：
  1. `HeapRb` 是 lock-free SPSC（生产/消费各 1 个），单次 push/pop 约 1-2 ns
  2. `embassy_sync::Channel` 是 MPMC 通用通道，泛型 + critical-section 包装，比 SPSC 多一层间接
  3. 失去 heap 分配灵活性（Channel 编译期 N，HeapRb 运行时 64 KiB）
  4. 项目中只有一个生产者（copier）和一个消费者（reader/writer），**根本不需要 MPMC**
- **教训**：选 embassy 原语前先评估是否真的需要其通用能力。单生产者单消费者场景 MUST 优先 ringbuf

**踩坑 2：Mutex 替换 SpinNoPreempt 的反优化**（L82，2026-06-05）

- **反优化**：`embassy_sync::Mutex<T>` 替换 `Arc<SpinNoPreempt<T>>`
- **现状**：Tty / RingBufRx / RingBufTx 用 `SpinNoPreempt`（自旋锁 + 中断禁用）
- **不要做的原因**：
  1. 现有临界区都是**同步**的（不进 `.await`），`SpinNoPreempt` 几十 ns 加锁
  2. `embassy_sync::Mutex` 是异步 Mutex，**强制**走 embassy executor 的调度器，与 axtask 冲突
  3. 即便不跨 `.await`，异步 Mutex 也有调度开销（~500 ns+）
  4. 改用 async Mutex 会引入 embassy-executor（与 L10 教训冲突）
- **教训**：临界区不进 `.await` MUST 用同步锁（SpinNoPreempt / kspin），禁止用异步 Mutex

**踩坑 3：Watch / Signal 包装 AtomicBool 的反优化**（L83，2026-06-05）

- **反优化**：`embassy_sync::Watch<bool>` 替换 FIONBIO 标志的 `AtomicBool`
- **不要做的原因**：
  1. FIONBIO 标志是单 bool，AtomicBool 的 load/store + Acquire/Release 语义完全够用
  2. `Watch` 是为"最新值广播"设计（如配置变更），多消费者场景才有价值
  3. `Signal` 是一次性唤醒，AtomicWaker + skip 机制（O31）已实现等价功能
- **教训**：单 bool / 单计数器场景用 `Atomic*`，多消费者"最新值"才考虑 `Watch`

**踩坑 4：Semaphore 计数 NAPI 阈值的反优化**（L84，2026-06-05）

- **反优化**：`embassy_sync::Semaphore` 跟踪"连续成功读取字节数"
- **不要做的原因**：
  1. `Semaphore` 是**资源计数**抽象（最多 N 个资源），不是事件计数器
  2. NAPI 阈值逻辑是"成功 ≥16 后切模式"，语义是阈值触发，不是资源获取
  3. 强行用 Semaphore 需要 acquire/release 配对，破坏状态机结构
  4. 当前实现是 4 行状态机（`u32 consecutive_success` + `if >= NAPI_THRESHOLD`），无抽象必要
- **教训**：用工具前先读文档确认语义，错误工具增加代码量

#### Scenario: 评估 embassy 同步原语替换

- **WHEN** 开发者提议用 embassy 同步原语替换现有 Rust 标准 / 项目原语
- **THEN** MUST 先回答三个问题：(1) 当前实现有可测问题吗？(2) embassy 方案在该场景下更快/更简洁吗？(3) 不与 axtask 架构冲突吗？三个都"是"才考虑，否则保持原状

#### Scenario: 选型 embassy 子集

- **WHEN** 开发者要添加 embassy 依赖
- **THEN** MUST 限定为 `embassy_sync::waitqueue::AtomicWaker`，禁止引入 executor / time / futures，遵循 L10 与 L128 已记录的原则

### Requirement: MMIO 权限诊断误判与纠正

UART MMIO `0x10000000` MUST 视为已正确映射（`READ | WRITE | DEVICE`），LoadFault 等错误 MUST 先排查 stride / 地址等代码 bug，再考虑页表权限。

**重要纠正（关键发现）**（L117 / L118 / L121，2026-05-31）

- **此前误判**：ADR-022/023 认为 axplat 限制 MMIO 权限，导致方向 B P1/P2 阻塞
- **真正根因**：NS16550 寄存器仅 0x00-0x07 共 8 字节。`UART_STRIDE=4` 下 ISR（offset 2×4=8）读写到 `base+8`，超出寄存器范围，QEMU 总线错误被 RISC-V 解释为 LoadFault
- **关键证据**：raw read at base+5（stride 1）成功，base+8（stride 4）失败 — 同一 4K 页表映射，排除页表问题

**Console MMIO 权限验证路径**：

1. `axconfig.toml → [devices].mmio-ranges` 包含 `[0x1000_0000, 0x1000]`
2. `mmio_ranges()` → `axhal::mem::memory_regions()` → `new_kernel_aspace().map_linear()` 将 UART 以 `READ | WRITE | DEVICE` 映射
3. Console 的静态 `MmioSerialPort` 访问 `0xffffffc010000000` 命中有效映射（**与初始化时机无关**）

**axmm::iomap() 现成 API**（L119，关键技术发现）

- **发现**：`axmm` crate 已提供 `iomap()` 函数，专门用于将设备 MMIO 映射到内核页表
- **函数签名**：`pub fn iomap(addr: PhysAddr, size: usize) -> AxResult<VirtAddr>`
- **内部实现**：`kernel_aspace().lock().map_linear()` + `protect()`，使用 `DEVICE | READ | WRITE` 标志
- **关键特性**：
  - 如果映射已存在，静默跳过 `map_linear()` 后仍调用 `protect()` 确保权限正确
  - 自动 flush TLB（cursor drop 时）
  - 不修改任何外部 crate
- **调用方式**：`axmm::iomap(PhysAddr::from(0x10000000), 0x1000)`

#### Scenario: UART 操作触发 LoadFault

- **WHEN** UART 读写出现 LoadFault / StoreFault
- **THEN** MUST 先按"stride=1 验证 → base 物理地址核对 → axconfig.toml 设备列表"顺序排查，**禁止**直接归因为"页表权限问题"

#### Scenario: 访问新设备 MMIO 区域

- **WHEN** 开发者要访问不在 `axconfig.toml` 的设备 MMIO
- **THEN** MUST 先在 axconfig 中注册；如已注册但仍报错，调用 `axmm::iomap(PhysAddr, size)` 作为冗余安全网

### Requirement: 共存 / 竞争发现与解决方案

共享硬件（同一 UART）的 reader/writer MUST 互斥访问，TX 共享 THR 时 MUST 保证同一批数据原子发送。

**踩坑 1：copier/Console FIFO 竞争**（L123，2026-05-31）

- **症状**：Shell 显示 `starry:~#` 但键盘输入完全无效（`ls` 等命令无响应）
- **根因**：RX copier 和 Console tty-reader 都读取同一个 UART RBR（FIFO）。copier 先读取 → 数据进入 ring buffer → tty-reader 读 FIFO 时空 → Shell 收不到输入
- **解决**：Q2 关闭 copier 让 Console 独占 UART。Q3 替换 Console 后再由 copier 接管
- **教训**：共享硬件（同一 UART）的两个 reader MUST 互斥访问，不能同时 drain FIFO

**踩坑 2：TX copier 与 ax_println! 输出交错**（L126，2026-05-31）

- **症状**：异步 TX 启用后 Shell 输出乱码（`ls /bin` 行间字符交叠）
- **根因**：TX copier 用 `send_bytes()` 批发送，中间 `ax_println!` 的 Console polling TX 插队写 THR。copier 把未发完数据推回 ring buffer → 新数据与旧数据混合 → 再次发送时乱序
- **解决**：TX copier 用本地 `cursor` 追踪已发位置，未发完的数据保留在本地 `write_buf` 中，不推回 ring buffer。下次迭代从 `cursor` 继续
- **教训**：共享硬件（同一 UART THR）的两个 writer MUST 保证同一批数据原子发送。**禁止**把部分数据推回共享缓冲区
- **回退方案（已弃用）**：临时切回 `ConsoleWriter` 让 Shell stdout 也走 Console TX — 这是降级，真正的异步 TX 被绕过

**踩坑 3：O_NONBLOCK 必须通过三个入口全部传播**（L140，2026-06-01）

- **问题**：最初只在 `sys_ioctl(FIONBIO)` 做了 `f.ioctl(cmd, nb)` 转发，但 `open(O_NONBLOCK)` 和 `fcntl(F_SETFL, O_NONBLOCK)` 只在 File 层设置 flag，未传播到 Tty
- **症状**：`open("/dev/console", O_RDWR | O_NONBLOCK)` 后 `read()` 仍然阻塞
- **解决**：三个入口都加 `f.ioctl(FIONBIO, nb as usize)`：
  - `syscall/fs/fd_ops.rs:106` — open() 路径
  - `syscall/fs/fd_ops.rs:254` — fcntl F_SETFL 路径
  - `syscall/fs/ctl.rs:31` — sys_ioctl 路径
- **教训**：任何跨层状态传播 MUST 穷举所有入口，一个遗漏 = 功能不完整

**踩坑 4：FIONBIO nonblocking 标志未传播到 TTY 层**（L137）

- **问题**：`File::read()` 将 nonblocking 传入 `poll_io`，但 `Tty::read_at()` 和 `ldisc.read()` 内部 `block_on(poll_io(...))` 硬编码 `false`
- **影响**：`ioctl(FIONBIO)`、`fcntl(F_SETFL, O_NONBLOCK)`、`open(O_NONBLOCK)` 对 TTY 读均无效
- **TX 路径**：`AsyncUartWriter::write()` 天然非阻塞（push ring buffer），不受影响
- **解决**：Tty struct 添加 `AtomicBool` nonblocking，传播到 `read_at → ldisc`

#### Scenario: 多个 reader 抢占同一硬件 FIFO

- **WHEN** 出现两个 reader 想 drain 同一硬件 FIFO
- **THEN** MUST 设计互斥（独占控制 / 临界区 / 阶段切换），禁止并发 drain

#### Scenario: 修改 TTY/串口的全局状态

- **WHEN** 开发者添加新的 fd 状态（nonblocking、raw 等）需要跨层传播
- **THEN** MUST 穷举所有入口（open / fcntl / ioctl），逐个验证状态正确性

### Requirement: 用户态性能与 tcdrain 实现

`tcdrain` (TCSBRK) MUST 用 `TRANSMITTER_EMPTY`（TEMT, bit 6）而非 `THR_EMPTY`（bit 5）判断"已 drain"；高吞吐场景的 QEMU 数据 MUST 用真板验证。

**踩坑 1：tcdrain 实现要点**（L139，2026-06-01）

- **需求**：`tcdrain(fd)` 调用 `ioctl(fd, TCSBRK=0x5409)`，需等待 TX 数据完全发送
- **实现位置**：`kernel/src/syscall/fs/ctl.rs:43-58`
- **关键**：MUST 查 `TRANSMITTER_EMPTY`（bit 6, TEMT）而非 `THR_EMPTY`（bit 5），否则 THR 空但移位寄存器还在发 → tcdrain 过早返回
- **QEMU 限制**：QEMU 16550 不仿真串口线延迟，tcdrain 几乎瞬时返回，吞吐量 ~200 KB/s 而非 ~11.5 KB/s

**踩坑 2：三层嵌套 block_on/poll_io 导致 yield storm**（L134，2026-06-01）

- **问题**：用户态 async read 路径有 3 层嵌套 `block_on(poll_io(...))`：`File → Tty/JobControl → Ldisc/WaitPollable`
- **根因**：`ProcessMode::Manual` 中 `register_rx_waker()` 调用 `waker.wake_by_ref()`，导致 waker 注册后**立即唤醒** task
- **效果**：形成高频 yield-re-schedule 循环（yield storm），无数据时空耗 CPU
- **解决方向**：改用 `ProcessMode::External` 消除立即唤醒，或优化 WaitPollable 的 register 行为

**踩坑 3：异步 VS 阻塞串口性能边界**（L135，2026-06-01）

- **上限**：115200 bps = 11.52 KB/s，无论同步异步都受此限制
- **Async TX 优势**：`write()` 返回快（~1 µs vs 87 µs/byte busy-wait），适合 pipeline
- **Async RX 劣势**：多一次 ring buffer 拷贝（UART FIFO → ring buf → ldisc buf → user buf）
- **CPU 空闲**：Manual 模式下 yield storm 导致空闲 CPU 更高
- **结论**：异步在吞吐量上**不可能**超过阻塞 Console（硬件上限），优势在不阻塞调用方

**踩坑 4：benchmark 公平性诊断**（L145，2026-06-01）

- **问题**：Console `write()` 本身阻塞到发送完成，Async `write()` 非阻塞 push + 显式 `tcdrain()`。测的不是同一个时间点
- **Console QEMU**：纯 VFS+MMIO 速度（~5 µs/64B），因为 QEMU LSR 永远 THR_EMPTY
- **Async QEMU**：VFS + 任务切换（~300 µs/64B），因为 tcdrain 需要多次 poll → yield
- **公平对比**：去除 tcdrain，只比 `write()` 延迟（Async 快 2.2~7.5x）
- **真板**：两者受 115200 bps 限制，收敛到 ~11.5 KB/s；QEMU 的差距是人工产物

**踩坑 5：当前 benchmark 不测量实际 UART 吞吐量**（L136）

- **问题**：`tests/benchmark.c` 的 TX 吞吐量测试写入 `/dev/null`（非 `/dev/console`），绕过 UART
- **延迟测试**：测量的是 ring buffer push 时间（~1 µs），不是硬件发送延迟
- **RX 用户态测试**：被跳过（TTY echo loop），内核态测试绕过 TTY 层
- **解决**：TX 测试需 `write → tcdrain()` 等实际发送完成；RX 需 raw mode + 独立测试程序

**踩坑 6：CPU 测试数据量统一**（L131）

- **问题**：Console 测试写入 120 字节，Async 测试写入 102,400 字节，数据量差 853 倍
- **影响**：CPU 占用数据无法公平对比（Console 2.3% vs Async 57.8%）
- **解决**：统一测试数据量为 102,400 字节
- **结果**：Console 3,835 cycles/byte，Async 268 cycles/byte，Async 效率高 14.3 倍

#### Scenario: 实现新的 tcdrain 类等待

- **WHEN** 开发者要实现"等待 TX 完成"的逻辑
- **THEN** MUST 用 `LSR::TRANSMITTER_EMPTY`（bit 6）判断，禁止用 `THR_EMPTY`（bit 5）

#### Scenario: 测量 I/O 性能

- **WHEN** 开发者要对比 Async vs Console 性能
- **THEN** MUST 在相同数据量、相同测试方法下对比，QEMU 吞吐量不可信时需明示并辅以真板验证

### Requirement: 性能优化技术模式（Q5 已完成）

性能优化 MUST 集中在 IER 缓存、ISR 合并、批量 I/O、waker skip 四个方向。

**技巧 1：IER 缓存**（O27 / L125）

- 用 `AtomicU8` 缓存 IER 值，enable/disable 通过 `uart_16550` 的 `set_ier()` API 写入（Q8 规范化，消除裸 `write_volatile`）

**技巧 2：ISR 合并**（O28）

- ISR 通过 `read_isr_unlocked()` 无锁读取 ISR 寄存器，配合 `disable_rx_intr()`/`disable_tx_intr()` 禁用中断（Q8 无锁化，消除 SpinNoIrq）

**技巧 3：批量 I/O**（O25-O26）

- RX copier 在单次锁内排空 FIFO
- TX copier 在单次锁内填满 FIFO

**技巧 4：waker skip**（O31）

- 用 `Cell<Option<Waker>>` + `will_wake` 避免重复注册相同的 waker

**技巧 5：TX 单锁**（O30）

- 消除 double buffer lock（pop → send → push_back）
- 改为一次 pop + send，只在 FIFO 满时 push_back

**技巧 6：NAPI 中断合并**（O2 / O34，L127）

- **触发**：连续成功读取 ≥ `NAPI_THRESHOLD`（16）次后进入轮询模式
- **轮询**：batch size 缩小到 `NAPI_BATCH_SIZE`（64），不重新使能 RX 中断
- **退出**：一次读取返回 0 → `consecutive` 重置 → 退出轮询 → 使能 RX 中断
- **效果**：高吞吐时减少 90%+ IRQ 频率

**技巧 7：tcdrain 真异步化（O45）**（L144）

- **三段式等待**：
  1. ring buf 有数据 → 注册 `tx.poll`（copier pop 时 `poll.wake()` 唤醒 tcdrain）
  2. ring buf 空但 UART 还在发 → 注册 `DRAIN_WAKER`（ISR TX 中断时唤醒）
  3. ring buf 空 + UART TEMT → 返回
- **double-check 模式**：`check TEMT → register DRAIN_WAKER → check TEMT again → park`，防止 ISR 在检查与注册之间触发而丢失唤醒
- **DRAIN_WAKER**：独立 AtomicWaker（不覆盖 TX_WAKER），ISR 中 `TX_WAKER.wake()` + `DRAIN_WAKER.wake()` 同时调用
- **效果**：64 字节 tcdrain 从 9 次切换降至 ~6 次，QEMU 延迟 ~300→~200 µs

#### Scenario: 优化热路径性能

- **WHEN** 开发者要优化 ISR 或 copier 性能
- **THEN** MUST 优先考虑 IER 缓存 / 批量 I/O / waker skip / 锁合并四个方向，**禁止**简单加锁粒度

### Requirement: 常用编程模式（代码模板）

新代码 MUST 复用以下已验证模式，禁止另起炉灶。

**模式 1：ISR 极简原则**（L12）

中断处理只做三件事：清中断标志 → 唤醒 Waker（AtomicWaker::wake）→ 立即退出。数据搬运推迟到任务上下文（后台协程）。

**模式 2：双缓冲 Ring Buffer**（L13）

- RX：硬件 FIFO → ringbuf → 用户空间
- TX：用户空间 → ringbuf → 硬件 FIFO
- 后台协程是唯一操作硬件的角色，天然无竞态

**模式 3：poll_io 标准模式**（L71）

```rust
poll_fn(|cx| {
    match try_operation() {
        Ok(val) => Poll::Ready(val),
        Err(WouldBlock) => {
            register_irq_waker(IRQ_NUM, cx.waker());
            Poll::Pending
        }
    }
}).await
```

**模式 4：AtomicWaker 使用模式**（L72）

```rust
static WAKER: AtomicWaker = AtomicWaker::new();
// 任务上下文注册
WAKER.register(cx.waker());
// ISR 中唤醒
WAKER.wake();
```

**模式 5：设备注册到 devfs 模式**（L73）

```rust
// 在 pseudofs/dev/mod.rs builder() 中
builder.add_device(
    "async_uart_test",
    DeviceId::new(4, 64),
    Arc::new(Device::new(async_uart_test_device)),
);
```

**模式 6：UART 状态诊断模式**（L74）

```rust
// 集成前必须诊断
let ier = uart.interrupt_enable();
let iir = uart.interrupt_identification();
let lsr = uart.line_status();
let mcr = uart.modem_control();
log::info!("UART State: IER={:#x} IIR={:#x} LSR={:#x} MCR={:#x}", ier, iir, lsr, mcr);
```

**模式 7：内核内部测试模式**（L75）

```rust
// 在 kernel/src/drivers/serial/test.rs
pub fn run_tests() {
    test_device_creation();
    test_write_at();
    test_pollable();
    // ...
}
// 在 entry.rs init() 中调用
drivers::serial::test::run_tests();
```

**模式 8：条件编译开关模式**（L76）

```rust
#[cfg(feature = "async_uart")]
pub fn init() { /* AsyncUart 初始化 */ }
#[cfg(not(feature = "async_uart"))]
pub fn init() { /* Console 初始化（默认） */ }
```

**模式 9：UART 重初始化安全模式**（L77）

```rust
// 1. 读取当前配置
let current_ier = uart.interrupt_enable();
// 2. 只修改需要的位
uart.set_interrupt_enable(current_ier | IER::THR_EMPTY);
// 3. 验证修改结果
let new_ier = uart.interrupt_enable();
assert!(new_ier.contains(IER::THR_EMPTY));
```

**模式 10：iomap 设备 MMIO 映射模式**（L120）

```rust
// 在 entry.rs::init() 中，确保设备 MMIO 可访问
use axmm::iomap;
use memory_addr::{PhysAddr, PAGE_SIZE_4K};

// 映射 UART MMIO (addr=0x10000000, size=4K)
let vaddr = iomap(PhysAddr::from(0x10000000), PAGE_SIZE_4K)
    .expect("Failed to map device MMIO");

// 现在可以安全访问
let ptr = vaddr.as_mut_ptr() as *mut u8;
unsafe { ptr.add(5).read_volatile() }; // 读 LSR
```

- **原理**：锁定 `kernel_aspace()` 全局内核页表，调用 `map_linear()` + `protect()` 保证 `DEVICE | READ | WRITE` 权限
- **优势**：不修改任何外部 crate，已有稳定 API

#### Scenario: 实现新的串口功能

- **WHEN** 开发者要写新的 UART/TTY/copier 代码
- **THEN** MUST 复用上述 10 个模式中的对应模板，不允许另起炉灶或省略关键注释

### Requirement: 测试方法与性能基准

性能测试 MUST 在内核态和用户态分开设计，并标明 QEMU 时序欺骗边界。

**性能测试框架**（L130，2026-06-01）

- **内核态测试**：`kernel/src/drivers/benchmark.rs`
  - CPU 占用测量：RISC-V cycle 计数器
  - NAPI 效果报告：IRQ 频率统计
  - Ring Buffer 写入测试
- **用户态测试**：`tests/benchmark.c`
  - TX 吞吐量：不同数据大小（64/256/1024/4096 字节）
  - `write()` 延迟：P50/P95/P99
  - 压力测试：持续 2 秒写入
- **自动化脚本**：`scripts/benchmark.sh`
- **测试分支**：`feat/uart-async-bench`（Async）、`feat/uart-bench`（Console）

**RX 测试方法决策**（ADR-031 / L132，2026-06-01）

- **Async**：内核态直接测 Ring Buffer（`run_rx_throughput_test()` / `run_rx_latency_test()`）
- **Console**：跳过（无 Ring Buffer，非阻塞读取）
- **用户态 RX**：跳过（TTY 回显竞争）
- **结果**：Async RX Ring Buffer 读取 588,776 KB/s，延迟 P50 600 ns

**用户态 async read 完整路径**（L138）

```
sys_read → File::read → block_on(poll_io(File, IN, nb, || inner.read()))
  → Device::read_at → Tty::read_at → block_on(poll_io(JobControl, IN, false, || ldisc.read()))
  → ldisc.read → block_on(poll_io(WaitPollable, IN, false, || buf_rx.pop_slice()))
```

- **关键点**：3 层嵌套 block_on、Manual 模式 `waker.wake_by_ref()`、无 nonblocking 传播
- **文件路径**：`kernel/src/file/fs.rs → kernel/src/pseudofs/dev/tty/mod.rs → .../terminal/ldisc.rs`

#### Scenario: 设计新的性能基准

- **WHEN** 开发者要测量新优化效果
- **THEN** MUST 同时提供内核态（绕过 TTY）和用户态（完整链路）两个测试，并标明 QEMU vs 真板的可信度差异

---

### Requirement: 2026-06-11 优化审计新发现

本次审计（openspec-explorer，4 个并行 agent）揭示的未记录优化机会与正确性问题。

**踩坑 5：NAPI 模式永不退出（2026-06-11）**（L150）

- **症状**：`async_driver.rs:51` 中 `consecutive` 在 NAPI 模式（≥16）下只增不减（`consecutive += 1`），零字节读取时无重置逻辑。RX 中断永久禁用，CPU 空转轮询空 FIFO。
- **根因**：NAPI 退出逻辑缺失 — 缺少 `total == 0` 时重置 `consecutive = 0` + 调用 `enable_rx_intr()` 的分支。
- **解决**：添加 `if total == 0 { self.consecutive = 0; enable_rx_intr(); }` 退出分支。
- **预防**：状态机转换 MUST 穷举所有转移条件，特别是"退出"路径。

**踩坑 6：ISR 中获取 SpinNoIrq 锁（2026-06-11）**（L151）

- **症状**：`isr.rs:10` 中 `uart_instance().lock()` 在 ISR 上下文获取 SpinNoIrq 锁。违反 ISR 极简原则第 4 条。
- **根因**：`uart.isr()` 和 `disable_*_intr()` 需要 `&mut self`，而 uart_16550 的 MUTEX 要求 SpinNoIrq 保护。
- **解决方向**：实现无锁的 ISR 读取路径（`unsafe fn isr_unchecked(&self)`），或拆分锁范围。
- **预防**：ISR 路径 MUST 逐行审计锁获取，禁止任何形式的锁操作。

**踩坑 7：IER 裸 write_volatile 绕过 uart_16550 API（2026-06-11）**（L152）

- **症状**：`uart_init.rs:72` 使用 `unsafe { core::ptr::write_volatile(ptr.add(offsets::IER), value) }`，违反 MMIO 封装规则。
- **根因**：uart_16550 v0.6.0 无 `set_ier()` 公共方法，只能通过 `new_mmio()` 构造时的配置。
- **解决方向**：向 uart_16550 添加 `pub fn set_ier(&mut self, ier: IER)` 方法，或使用 `ier_mut()` 返回引用。
- **预防**：任何 `unsafe write_volatile` MUST 附 SAFETY 注释并证明无法通过 crate API 实现。

**技巧 8：读路径数据拷贝分析（2026-06-11）**（L153）

- **发现**：用户态串口读路径经过 5 次数据拷贝：UART FIFO → copier buf → driver ringbuf → InputReader buf → ldisc ringbuf → user buf
- **可优化点**：C3/C4（InputReader buf → ldisc ringbuf）在同一个 `InputReader::poll()` 中立即完成，可合并为一次直接 push。
- **关键文件**：`ldisc.rs:83-90`（InputReader::poll）
- **量化**：每字节减少 1 次 memcpy，115200 bps 下节省 ~11.5 KB/s 的 CPU。

**技巧 9：PollSet→AtomicWaker 迁移风险矩阵（2026-06-11）**（L154）

- **pipe.rs**（3 PollSet）：HIGH — 跨操作唤醒（read→wakeTX, write→wakeRX, drop→wakeClose），需 3 个独立 AtomicWaker
- **signalfd.rs**（1 PollSet）：LOW — 最简单的 1:1 替换，两个唤醒源（update_mask + read re-wake）
- **pidfd.rs**（1 Arc\<PollSet\>）：HIGH — exit_event 是共享的（Arc 克隆自 Thread/ProcessData），修改影响 task/mod.rs 和 task/ops.rs 的唤醒路径
- **event.rs**（2 PollSet）：MEDIUM — 同 pipe 的跨操作模式
- **关键前提**：AtomicWaker 仅支持单 waker。需验证这些场景中是否最多 1 个 task 同时 poll（async 模型下通常是）。

**技巧 10：copier waker 去重可简化（2026-06-11）**（L155）

- **发现**：`async_driver.rs:53-55,82-84` 每个 poll_fn 迭代执行 2×Waker::clone() + will_wake() + register()
- **原因**：单线程 copier 的 waker 几乎不变，但代码仍每次 clone+检查
- **简化方向**：`if !last_waker.get().map_or(false, |old| old.will_wake(&cx.waker())) { RX_WAKER.register(cx.waker()); }` — 仅在变化时 clone
- **量化**：每 poll 周期节省 ~2 次 Arc 原子操作（~20-40ns）
