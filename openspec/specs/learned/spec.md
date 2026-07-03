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

**异步 OS 抽象 trait（Q13 新增到 uart_16550）**

| 路径 | 用途 | 备注 |
|------|------|------|
<!-- tombstone: L161-L164 --> Archived 2026-07-02 in ARC-202607021648 — Q13 旧 5-trait OS abstraction 已被 ADR-036 缩减为 2-trait；当前 API 见 L188-L200。
| <!-- L165 --> | `uart_16550::async_::driver::UartPort` | UART hardware access trait | `receive_bytes(&self, buf)` + `send_bytes(&self, buf)` |
| <!-- L166 --> | `uart_16550::async_::driver::AsyncUartDriver<R,W,U>` | Async UART driver | `new(rx, tx, uart)` + `start_rx/tx_copier(enable_intr)` |
| <!-- L167 --> | `uart_16550::async_::device_ops::AsyncUartReader<R,W,U>` | Async UART reader | `impl TtyRead + embedded_io_async::Read` |
| <!-- L168 --> | `uart_16550::async_::device_ops::AsyncUartWriter<R,W,U>` | Async UART writer | `impl TtyWrite + embedded_io_async::Write + Clone` |

**异步模块文件路径（Q13 新增到 uart_16550 和 StarryOS）**

| 路径 | 用途 |
|------|------|
| <!-- L169 --> | `uart_16550/src/os/mod.rs` | OS abstraction trait definitions: 5 traits for cross-platform async |
| <!-- L170 --> | `uart_16550/src/async_/mod.rs` | Async module root: exports isr, ring_buffer, driver, device_ops |
| <!-- L171 --> | `uart_16550/src/async_/isr.rs` | ISR handler + AtomicWaker: RX_WAKER, TX_WAKER, DRAIN_WAKER |
| <!-- L172 --> | `uart_16550/src/async_/ring_buffer.rs` | Ring buffer with OsWakerSet: RingBufRx<W>, RingBufTx<W> |
| <!-- L173 --> | `uart_16550/src/async_/driver.rs` | Copier driver with NAPI: AsyncUartDriver<R,W,U> + UartPort trait |
| <!-- L174 --> | `uart_16550/src/async_/device_ops.rs` | Device ops with embedded_io_async: AsyncUartReader/Writer |
| <!-- L175 --> | `kernel/src/drivers/os_arceos.rs` | ArceOS adapter layer: 2-trait minimum interface (per ADR-036) |

**内核模块关键路径**

| 路径 | 用途 |
|------|------|
| `kernel/src/file/pipe.rs` | poll_io + register_irq_waker 模式参考 |
| `kernel/src/file/event.rs` | 轻量异步通知模式参考 |
| `kernel/src/pseudofs/device.rs:28-55` | DeviceOps trait 核心方法（read_at/write_at/ioctl/as_pollable/flags） |
| `kernel/src/pseudofs/dev/tty/` | TTY/ldisc/Termios 实现 |
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

- **现象**：QEMU 上 TX 吞吐量测出 150~250 MB/s，远超 115200 bps 物理定律上限 11.5 KB/s
- **根因**：QEMU 的 NS16550 模拟不仿真真实串口线延迟（86.8 µs/byte），UART FIFO 数据处理为瞬时
- **影响**：所有基于 `tcdrain` / 轮询 LSR 的吞吐量测试在 QEMU 上均不可信
- **📐 物理定律**（100% 准确）：真板 NS16550 @ 115200 bps 线速上限 = 11,520 B/s（单字节 86.8 µs），实测值受调度/IRQ 延迟影响可能低于此值
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
- **📐 物理定律**：真板两者受 115200 bps 限制，线速上限 11.52 KB/s；QEMU 的差距是仿真人工产物（Q6 真板实测验证 ⏳）

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

Performance benchmarks MUST be designed separately for kernel-space and user-space, and MUST label the QEMU timing deception boundary. All performance claims MUST specify measurement environment and data size.

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

**Q15-M0 见证层测试经验（2026-06-23）**

M0 见证层（FIFO 边界矩阵 benchmark + telemetry 计数器）实施中积累的测试部署与代码设计经验。

#### QEMU benchmark 交叉编译 → 部署流程

<!-- L201 -->
- **交叉编译**：`export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH && riscv64-linux-musl-gcc -static -o tests/benchmark tests/benchmark.c`
- **挂载部署**：`sudo mount -o loop make/disk.img /mnt && sudo cp tests/benchmark /mnt/bin/benchmark && sudo umount /mnt`
- **运行**：`make run` → QEMU 内 `./benchmark`
- **关键约束**：`make/disk.img` 是 `rootfs-riscv64.img` 的副本（ext4），benchmark 二进制路径为 `/bin/benchmark`

#### benchmark 测试代码设计原则

<!-- L202 -->
- **填充字节用 `\0`**：`memset(buf, 0, sz)`。UART 正常传输，终端零显示。禁止用可见字符（`'A'`）或不可打印字符（`0xFF`），前者刷屏、后者显示乱码 `�`
- **新增测试尺寸不与现有重叠**：`test_tx_throughput` 已覆盖 64/256/1024/4096B，新增矩阵只测 FIFO 边界尺寸 1/15/16/17/31/32/33/48/49
- **排序算法匹配现有风格**：新函数用 bubble sort（与 `test_tx_latency` 一致），不引入 `qsort` + 独立 comparator
- **输出格式对齐**：缩进 + 单位标注（`ms`），匹配 `test_tx_latency` 的 `n=X avg=Y ms P50=Z ms` 格式

<!-- L203 -->
- **数据量意识**：QEMU 115200 bps 下每 KB 数据 ≈ 87ms 传输时间。FIFO 边界矩阵 9 尺寸 × 100 迭代 ≈ 24KB（~2s）。避免尺寸重复导致数据量翻倍（曾因 64/256/1024/4096 重复导致 ~572KB → ~50s）

#### ArceOS 借鉴参考（DMA / HAL / async 模式）

<!-- L204 -->
| `axdma::alloc_coherent` | `others/arceos/modules/axdma/src/lib.rs:46` | DMA 一致性内存分配（返回 `DMAInfo { cpu_addr, bus_addr }` 二元组）。**仅模式参考**：NS16550 16 字节 FIFO 不需要 DMA，引入对齐约束 + cache flush 复杂度反而过度设计。未来做高速外设（NIC/块设备）可强借鉴。 | 2026-06-26 |
<!-- L205 -->
| `axdriver_net::dwmac::DwmacHal` | `others/arceos/axdriver_crates/axdriver_net/src/dwmac/mod.rs:32-54` | DWMAC HAL trait（7 方法：`dma_alloc` / `dma_dealloc` / `mmio_phys_to_virt` / `mmio_virt_to_phys` / `wait_until` / `configure_platform` / `cache_flush_range`）。我们 `UartPort`（4 方法）+ `OsRuntime`/`OsWakerSet`（2 trait）= 6 接口，比 DwmacHal 7 方法**更精简且正交性更好**（ADR-036 已印证）。 | 2026-06-26 |
<!-- L206 -->
| `axasync::waker::wake_at` | `others/arceos/modules/axasync/src/waker.rs:102` | timer-based waker（`BinaryHeap<TimerEventEntry>` + `set_oneshot_timer`）。**等价实现**：我们 `axtask::future::timeout(fut, dur)`（Q9）通过 `axtask::ax_wait_timer` + waker 实现相同语义，不引入 BinaryHeap/oneshot_timer。 | 2026-06-26 |
<!-- L207 -->
| `axnet::smoltcp_impl::RecvFuture::poll` | `others/arceos/modules/axnet/src/smoltcp_impl/future.rs:31-64` | "Init Flag + Waker" Future 模式：首次 poll 做一次性初始化（连接状态检查），主循环注册 waker + Pending。**PIT-006 教训**：AcceptFuture 遗漏 `register_accept_waker` → 永久 Pending。我们 ISR 极简（read_isr + 禁中断 + wake + 返回）+ AtomicWaker 直接 wake（O17 教训）已蕴含此不变量。 | 2026-06-26 |

#### 内存序踩坑（2026-06-26 O63 代码验证）

<!-- L208 -->
**[ier_cache RMW 竞争 — Q6 SMP P0 阻塞]**

- **症状**：真板 4 核下 ISR (hart 0) 和 copier (hart N) 并发调用 `update_ier()` 时，IER 中断使能位可能被对端覆盖丢失，导致 RX 或 TX 彻底停滞
- **根因**：`uart_init.rs:105-111` 的 `ier_cache` load-modify-store 在 `SpinNoIrq::lock()` **外面**执行。两个 hart 同时 load → modify → store 时后写者覆盖先写者
- **解决**：把 `ier_cache` RMW 搬进 `SpinNoIrq::lock()` 内，或改用 `AtomicU8::fetch_or`/`fetch_and` 配合 `AcqRel` 排序
- **预防**：跨 hart 访问的 Atomic 变量做 RMW 时，如用 load+store 两步，必须将两步放在同一个锁临界区内；或直接用 `fetch_*` 原子操作

<!-- L209 -->
**[tx_copier_active / tx_staged_bytes 跨 hart 读写排序 — Q6 SMP P1]**

- **症状**：真板多核下 flush/tcdrain （user task on hart N）可能看到 TX copier (hart M) 的陈旧 active/staged 值，导致 tcdrain 过早返回或 hang
- **根因**：`driver.rs` 中 `tx_copier_active` / `tx_staged_bytes` 的 store/fetch_add 用 `Ordering::Relaxed`，`tx_completion()` 的 load 也用 `Relaxed`。跨 hart 无 happens-before 保证
- **解决**：写端 → `Ordering::Release`；读端 → `Ordering::Acquire`；`fetch_add/sub` → `Ordering::AcqRel`
- **预防**：SMP 场景下，跨 hart 共享的 flag/counter 必须建立 happens-before 边。rule of thumb：store 用 Release，load 用 Acquire

<!-- L210 -->
**[QEMU 单核掩盖 SMP 内存序问题]**

- **现象**：Q15 全部 Relaxed 用法在 QEMU (max-cpu-num=1) 下完全正常，`cargo check` + benchmark 0 问题
- **根因**：QEMU 单 hart 下所有访问串行化，无并发窗口；RISC-V RVWMO 在单 hart 上 Relaxed ≈ SeqCst
- **影响**：`kspin` 当前未启用 `smp` feature（`SpinNoIrq` 是空操作），`critical_section` 仅本地关中断。这些都只在多核下暴露
- **预防**：开发阶段在 QEMU 上跑 `max-cpu-num = 4` + SMP feature 可以提前暴露部分问题（但非全部——QEMU 时序与真板不同）

#### Scenario: 查询 Q13 提取的 API 路径

- **WHEN** 开发者需要定位异步 UART 模块中某个具体类型或 trait
- **THEN** MUST 先查本速查表（L160-L200），再去对应源码文件确认

### Requirement: 2026-06-11 优化审计新发现

审计发现的未记录优化机会与正确性问题 MUST 评估风险后立项落地。本次审计由 openspec-explorer 的 4 个并行 agent 执行，禁止"以后再说"。

#### Scenario: 踩坑 5~7 任意一个未在 Q8~Q11 解决

- **WHEN** 审计发现的 NAPI 退出 / ISR 锁 / IER 裸写三个正确性 Bug 任一未修复
- **THEN** MUST 在下一个里程碑（Q8）优先修复，**禁止**以"低优先级"为由延后
- **AND** 修复后 MUST 写 regression test 防止复发

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

**踩坑 8：OpenSpec 变更的 tasks.md 漂移（2026-06-15）**（L156）

- **症状**：归档 Q12 OpenSpec 变更时发现 `openspec/changes/q12-embassy-path-a/tasks.md` 21 项全部未勾选（`- [ ]`），但实际代码 4 个 git 提交已完成（`e7d93f8` / `04483fe` / `20a243a` / `ac3544d`），全局 `.claude/docs/tasks.md` 与 `SNAPSHOT.md` 也已标 ✅。
- **根因**：
  1. 实施时仅更新全局状态文档（`tasks.md` / `SNAPSHOT.md`），未同步勾选 change 自己的 `tasks.md`
  2. 提交 `ac3544d docs(q12): mark Q12 complete, add OpenSpec change artifacts` 创建了 OpenSpec change 文件，但未把 tasks.md 勾上
  3. 归档时 `openspec status --change` 报 `isComplete: false`，因 tasks.md 仍是初始未勾选
- **影响**：
  1. 归档前需补勾选 21 项（`replace_all` 一次性把 `- [ ]` → `- [x]`）
  2. 需补 `specs/optimization/spec.md` delta spec（归档强制要求至少一个 delta）
  3. 主 `openspec/specs/optimization/spec.md` 第 165 行原"待实现"条目未同步回退到"已完成"（需手动改）
- **预防**：
  1. **实施期间每完成一个子任务 MUST 同步勾选 change 自己的 `tasks.md`**（不能只更新全局文档）
  2. 提交信息包含 `mark X complete` 时 MUST 检查 change 目录下 `tasks.md` 与主 spec 状态
  3. 归档前 MUST 跑 `openspec status --change <name>` + `openspec validate <name>` 双重验证
- **复盘模板**（每次实施完成时）：Code 提交 → **change/tasks.md 勾选** → 主 spec 同步 → 全局状态文档 → 提交 → `openspec validate`

**技巧 11：OpenSpec 归档前置验证清单（2026-06-15）**（L157）

| 检查项 | 命令 | 预期 |
|--------|------|------|
| 产物完成度 | `openspec status --change <name>` | artifacts 全部 `done` |
| tasks 状态 | 读 `tasks.md` 统计 `- [ ]` vs `- [x]` | 全部勾选 |
| delta spec | `ls <change>/specs/` | 至少一个 `spec.md`（含 `## ADDED/MODIFIED Requirements`） |
| 验证格式 | `openspec validate <name>` | 无 ERROR |
| 主 spec 同步 | 主 spec 与 delta spec 内容一致 | 主 spec 中相关条目已更新为最终态 |

**技巧 12：异步串口提取的 5 个 OS 抽象 trait（2026-06-15）**（L158）

将 StarryOS 异步串口实现提取为可复用 crate 时，仅需抽象 5 个 OS 特定 API：

| Trait | 替代的 OS API | Linux 等价 | Tock 等价 |
|-------|--------------|------------|-----------|
| `OsRuntime` | `axtask::{block_on, spawn_with_name}` | 手动 poll loop | callback |
| `OsIrq` | `axhal::irq::register_irq_hook` | `request_irq` | `subscribe` |
| `OsMmio` | `axhal::mem::phys_to_virt` + `axmm::iomap` | `ioremap` | 不暴露 |
| `OsSpinNoIrq` | `kspin::SpinNoIrq` | `spinlock_irqsave` | `critical_section` |
| `OsWakerSet` | `axpoll::PollSet` | `wait_queue_head` | `AtomicSubscriptions` |

**关键发现**：
- 核心异步逻辑（isr.rs + ring_buffer.rs + async_driver.rs + device_ops.rs）仅 ~400 行
- 已有依赖（embassy-sync + embassy-hal-internal + embedded-io-async）全部是 `no_std` 可移植的
- `embedded-io-async` 是社区标准 trait，实现后可与任何 async I/O 消费者互操作
- 三阶段迁移：trait 提取 → 核心逻辑 → 适配层，每阶段可独立验证

**踩坑 4：D1 决策需要推翻（2026-06-15）**（L159）

- **原决策**（uart_16550 ADR-7）：异步实现留在 StarryOS wrapper 层，uart_16550 保持 sync-only
- **推翻理由**：
  1. 复用需求 — 其他 OS 项目也需要异步 UART
  2. Q12 已完成基础设施 — `atomic_ring_buffer` + `embedded_io_async` + TC tcdrain
  3. 代码量可控 — 核心异步逻辑仅 ~400 行
  4. trait 抽象成熟 — `embedded_io_async` 是社区标准
- **新决策**：uart_16550 应该成为完整的异步 UART crate，通过 feature gate 支持 async

### Requirement: Q13 uart_16550 异步提取实施经验

将异步串口从 StarryOS 提取到 uart_16550 crate 过程中积累的实现经验。提取后的模块边界 MUST 遵循 5-trait OS 抽象（后精简为 2-trait），所有 async 代码 SHALL 位于 `async` feature gate 后。

| <!-- L176 --> | **UartPort trait for &mut self** | `Uart16550::receive_bytes/send_bytes` 取 `&mut self`，UartPort trait 需要内部可变性；`receive_bytes/send_bytes` 取 `&self`，底层通过 `OsSpinNoIrq::with_lock` 包装 |
| <!-- L177 --> | **Callback pattern for OsSpinNoIrq** | `with_lock<R>(&self, f: impl FnOnce(&mut T) -> R) -> R` 使用回调模式而非返回 guard，避免 guard 生命周期问题 |
| <!-- L178 --> | **&'static Self for no-alloc driver** | `AsyncUartDriver` 使用 `&'static Self` 而非 `Arc<Self>` 兼容 no-alloc 环境 |
| <!-- L179 --> | **StarryOS owns ring buffer statics** | RingBuffer::new() 静态变量保留在 StarryOS，通过 `&'static` 引用传递给 uart_16550 |

#### Scenario: 在新 OS 中实现异步 UART 适配

- **WHEN** 开发者要在新 OS 项目中使用 uart_16550 异步功能
- **THEN** MUST 实现 OsRuntime + OsWakerSet 两个 trait，并按本表 L176-L179 的模式注册静态 ring buffer

### Requirement: Q13 性能优化洞察

Q13 完成后性能测试显示 trait 抽象开销导致 +13% avg latency 退化。热路径函数 MUST 标注 `#[inline(always)]` 并优先采用批量操作减少 trait 调用次数；跨 crate 内联优化 SHALL 通过 LTO 实现。

| <!-- L180 --> | **Trait 抽象开销来源** | 泛型单态化（静态分发）无虚函数表开销，但热路径上 `UartPort::receive_bytes/send_bytes` + `OsWakerSet::wake/register` 每字节增加 ~15-30ns 锁获取开销 |
| <!-- L181 --> | **#[inline(always)] 优化** | 热路径函数添加 `#[inline(always)]` 可消除函数调用开销（-5~10µs），但可能导致代码膨胀影响 I-cache |
| <!-- L182 --> | **批量操作优化** | 减少每字节锁获取次数，改为批量处理（-10~20µs），但增加延迟（批量等待时间） |
| <!-- L183 --> | **Feature gate 条件编译** | 为 ArceOS 提供特化实现绕过 trait 抽象（-15~25µs），但增加维护负担、降低可移植性 |
| <!-- L184 --> | **性能与可移植性权衡** | `#[inline(always)]` + 批量操作 = 最佳性价比（-15~30µs，无可移植性损失）；feature gate 特化 = 中等收益但降低可移植性；DMA = 最佳性能但硬件依赖 |

#### Scenario: 评估 trait 抽象开销优化策略

- **WHEN** Q13 提取后性能出现退化
- **THEN** MUST 优先采用 inline + 批量操作（零可移植性损失），禁止直接实施 feature gate 特化绕过 trait

### Requirement: Q13.1 优化时机选择

| <!-- L185 --> | **优化时机分类** | 算法优化（批量）MUST 尽早实施，收益独立于模块化；编译器优化（inline）可等需要时再加，模块化后才需要显式标注 |
| <!-- L186 --> | **批量操作内嵌收益** | 批量操作在内嵌时就该做（减少锁获取次数），与模块化无关；Q0~Q12 期间未做是遗漏 |
| <!-- L187 --> | **跨 crate 内联必要性** | 同一 crate 内编译器自动内联，跨 crate 需要 `#[inline(always)]` 显式标注；模块化后 inline 注解成为必需 |

#### Scenario: 决定优化实施时机

- **WHEN** 开发者计划实施性能优化
- **THEN** MUST 区分算法优化（批量，尽早做）与编译器优化（inline/LTO，模块化后按需加），禁止颠倒顺序

### Requirement: Q13 模块分离后的 API 路径速查

API path quick-reference for post-Q13 module separation. All new async types and traits MUST be registered in this lookup table.

| 编号 | 名称 | 路径 | 用途 |
|------|------|------|------|
| <!-- L188 --> | `OsRuntime` trait | `uart_16550::os::OsRuntime` | 任务生成 `spawn(future, name)` + 同步等待 `block_on(future)` |
| <!-- L192 --> | `OsWakerSet` trait | `uart_16550::os::OsWakerSet` | waker 集合 `new()` + `register(waker)` + `wake() -> u32` |
| <!-- L193 --> | `ArceOsRuntime` 适配 | `StarryOS::drivers::os_arceos::ArceOsRuntime` | 桥接 `axtask::spawn_with_name` + `axtask::future::block_on` |
| <!-- L196 --> | `ArceOsWakerSet` 适配 | `StarryOS::drivers::os_arceos::ArceOsWakerSet` | 桥接 `axpoll::PollSet`（register/wake） |
<!-- tombstone: L189-L191/L194-L195 --> Archived 2026-07-02 in ARC-202607021648 — 旧 5-trait API/adapters 已删除，当前保留 OsRuntime + OsWakerSet。
| <!-- L197 --> | 异步栈模块入口 | `uart_16550::async_::*` | `isr` / `ring_buffer` / `driver` / `device_ops` 4 子模块 |
| <!-- L198 --> | `AsyncUartDriver<R, W, P>` | `uart_16550::async_::driver::AsyncUartDriver` | 异步驱动主类型，3 泛型参数：Runtime / WakerSet / UartPort |
| <!-- L199 --> | `TtyRead` / `TtyWrite` | `uart_16550::tty::{TtyRead, TtyWrite}` | 通用 TTY 抽象 trait（Q13 Phase 1 提取） |
| <!-- L200 --> | StarryOS 类型别名 | `StarryOS::drivers::{ArceOsDriver, ArceOsReader, ArceOsWriter}` | `pub type` 简化泛型，绑定 ArceOS 5 适配 |
| <!-- L201 --> | TEMT corner-case 丢唤醒窗口 | Q15-M2: 真板 NS16550 上 THRE 中断触发时 TEMT 可能为 0... | Q15-M2 driver.rs tx_copier_loop TEMT poll |
| <!-- L202 --> | M3 TtyWrite 短写契约 | Q15-M3 ✅ (2026-06-23): `TtyWrite::write(&[u8]) -> usize`，穿透 uart_16550（tty.rs + device_ops.rs）和 StarryOS（mod.rs + pty.rs + ldisc.rs）共 5 文件。QEMU benchmark 验证无退化（1B 0.134ms, 64B 210KB/s, FIONBIO PASS）。ADR-038 记录完整设计决策。 | Q15-M3 change |
| <!-- L204 --> | M3 短写影响面速查 | `../uart_16550/src/tty.rs`, `../uart_16550/src/async_/device_ops.rs`, `kernel/src/pseudofs/dev/tty/mod.rs`, `kernel/src/pseudofs/dev/tty/pty.rs`, `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`, `kernel/src/file/fs.rs`, `kernel/src/syscall/fs/io.rs` | `TtyWrite::write -> usize` 会让 `Tty::write_at` 把实际接受字节数穿透到 `sys_write`；`File::write` 对 `Ok(n)` 不重试，benchmark 必须循环处理短写。详见 `.claude/analysis/q15-m3-tty-short-write-contract.md` |
| <!-- L205 --> | Q15 增量融合策略（核心元经验）| Q15 (2026-06-25): M13 M4 Sync 一次性 apply 全部 M4+ 代码 → 64B write+tcdrain 退化 73.9x (406µs→29.99ms)；Q15 改为按依赖关系排序的 5 个原子 milestone（M0→M1→M2→M4→M3）增量融合 + 每步 cargo check + QEMU benchmark Gate → 5 天完成 + 无退化。**铁律**：禁止一次性 apply 多个 async-uart 优化 commit，必须按依赖排序 + 每步 Gate | `architecture.md` ADR-039 + `optimization.md` Q15 章节 |
| <!-- L206 --> | Q15 Manual QA 性能基线（2026-06-25）| QEMU 不带 LTO（per ADR-034）：用户态 1B e2e 134µs avg / P50 118.5µs / 64B TX 170KB/s / FIONBIO 全 PASS；内核态 Ring Buffer TX 456,205 KB/s / RX 1,147,959 KB/s（RX 较 Q13+LTO ↑27.9%）。**与 Q13.1 基线对比**：1B 延迟 +3.5% 在 noise 范围内；64B 吞吐 -7.6% 无 TX backpressure 退化（关键 Gate 通过） | `docs/benchmark-report-async.md` §0 |
| <!-- L207 --> | Q15 后禁止的操作 | ❌ 一次性 merge `feat/uart-16550-async-temp` 或其他临时分支的多个 commit（Q13 M4 Sync 已证伪 73.9x 退化）；❌ 删除 temp 分支（保留作为增量融合的参考基线）；❌ 在 QEMU 上声称绝对吞吐（不仿真串口线延迟，真板验证必须等 Q6） | `architecture.md` ADR-039 替代方案章节 |
| <!-- L208 --> | 增量融合的"依赖排序"启发式 | 按"基线能力 → 修复 → 契约"分层：M0 见证层（提供测量基线）→ M1/M2 修复（M1 fast retry 消除 tick 台阶，M2 drain 修正 flush）→ M4 规范化（IER 单 owner 整合）→ M3 契约（VFS 边界，独立于驱动内部）。M3 放最后因为它只改 trait 签名不碰驱动内部，依赖前 4 个 milestone 提供稳定的内部行为 | `architecture.md` ADR-039 |
| <!-- L211 --> | Q15 后 milestone 重排启发式 | 不按 O 编号顺序排期，按 Gate 类型分层：文档/规格收敛 → QEMU 可验证 correctness → 真板观测脚手架 → 真板 bring-up → 数据驱动决策 → 维护性清理 → 远期实验。避免把 O63/O64/O66/O3/O40/O41/O48 等不同触发条件的项继续塞进单一 Q6 | `.claude/analysis/optimization-milestone-replan.md` |
| <!-- L212 --> | Q17 内存序选型速查 | 不按架构分叉实现内存序；按 Rust 语言级并发契约选序。纯 telemetry 保持 `Relaxed`；跨 hart 发布/观察状态用 `Release`/`Acquire`；参与同步判断的 RMW 计数用 `AcqRel`；多字段一致性优先用锁或重新设计快照。`ier_cache` 是非原子 RMW 竞争，不能只靠 Acquire/Release 修复 | `.claude/analysis/q17-smp-memory-ordering.md` |
| <!-- L213 --> | Lichee RV Dock 采集完成边界 | 2026-06-28: 官方 Linux 采集已足够支撑 StarryOS early console smoke test，后续不再泛采集。已确认：D1/C906 单核 Sv39，RAM `0x40000000 + 512MiB`，OpenSBI v0.6 + U-Boot 2018.05，boot 分区 `/dev/by-name/boot` 是 Android boot image，U-Boot `sunxi_flash read 45000000 boot; bootm 45000000`，kernel_addr `0x40200000`。 | `.claude/analysis/lichee/public-platform-notes.md` + `docs/licheerv-dock-bringup.md` |
| <!-- L214 --> | Lichee RV Dock UART 事实 | D1 UART0: base `0x02500000`, size `0x400`, IRQ `18`, console `ttyS0,115200`。mainline DTS 是 `snps,dw-apb-uart`，`reg-shift = 2`、`reg-io-width = 4`，即 register stride 4 + 32-bit MMIO；不能沿用 QEMU NS16550 stride=1 假设。 | mainline `sunxi-d1s-t113.dtsi` + 真板 `probe`/explorer |
| <!-- L215 --> | Lichee RV Dock boot image 事实 | `/dev/mmcblk0p4` / `boot` header magic `ANDROID!`，name `d1-nezha`，page_size `2048`，kernel_size `9783580`，kernel_addr `0x40200000`，ramdisk_addr `0x41200000`，tags_addr `0x40200100`。StarryOS smoke test 优先生成 Android boot image 写入 boot 分区；写入前必须备份原 boot。 | `tests/boot_probe.c` 输出 `.claude/analysis/lichee/probe` |
| <!-- L216 --> | Lichee RV Dock early smoke test 路线 | 下一步工程目标不是继续采集，而是新增 D1 platform + DW APB UART early console + Android boot image 打包。成功标准：串口输出 `[starry-d1] early boot`。初期 timer 优先走 SBI timer，rootfs/USB/SD/Shell/async benchmark 后置。 | `docs/licheerv-dock-bringup.md` |
| <!-- L217 --> | 平台参数耦合点速查 | `kernel/src/drivers/uart_init.rs` 当前硬编码 QEMU UART：base `0x10000000`、stride 1、raw LSR `base+5`、`iomap(..., 0x1000)`；换 Lichee / VisionFive2 时不能只改构建目标，必须把 UART facts 从 driver init 中抽出。 | `.claude/analysis/platform-parameter-decoupling.md` |
| <!-- L218 --> | axconfig 可复用边界 | `make/platform.mk` 已支持 `MYPLAT` / `PLAT_CONFIG`，`make/config.mk` 生成 `.axconfig.toml`，`make/build.mk` 读取 `plat.kernel-base-paddr`。这些可复用为多平台入口，但 axconfig 还不能完整表达 UART kind / reg width / boot image strategy。 | `.claude/analysis/platform-parameter-decoupling.md` |
| <!-- L219 --> | 32-bit MMIO access width 缺口 | `../uart_16550/src/backend/mmio.rs` 支持 stride，但 volatile read/write 当前是 `u8`；D1 / VisionFive2 的 DW APB UART 需要 stride 4 + 32-bit MMIO。后续不能只把 stride 改成 4，必须显式处理 access width。 | `.claude/analysis/platform-parameter-decoupling.md` |
| <!-- L220 --> | early console 分层经验 | 真板 bring-up 应先用不依赖 IRQ / async task / rootfs 的 polling early console 输出首字节；async UART `/dev/console` 在 early console 和平台 descriptor 稳定后再接入。 | `.claude/analysis/platform-parameter-decoupling.md` |
| <!-- L221 --> | D1 当前无输出根因 | 2026-06-28 板测已确认 U-Boot 能识别 Android boot image 并打印 `Starting kernel ...`，但 ELF 仍链接到 `0xffffffc080200000` 且 `_stext` 调用 `axplat_riscv64_qemu_virt::boot`；`lichee-d1` feature 只影响 StarryOS entry 层，不会替换 axplat 启动层。 | `.claude/analysis/d1-axplat-bringup-plan.md` |
| <!-- L222 --> | D1 axplat 构建 Gate | 正确 D1 镜像必须通过 `MYPLAT` / `PLAT_CONFIG` 接入本地 `axplat-riscv64-lichee-d1`，保持 `DWARF=n`，linker base 应为 `0xffffffc040200000`，boot image `kernel_addr` 应为 `0x40200000`，`objdump` 必须显示 D1 axplat boot symbols 而不是 QEMU。 | `.claude/analysis/d1-axplat-bringup-plan.md` |
| <!-- L223 --> | axplat 版本对齐铁律 | 创建新 axplat crate 时 MUST 以 `make build` 输出中实际编译的版本为准（如 `v0.3.1-pre.6`），不可用 cargo registry 中找到的最新版（如 `v0.4.1`）。`#[impl_interface]` vs `#[impl_plat_interface]` 宏名因版本而异。 | Q19 构建踩坑 |
| <!-- L224 --> | axconfig_macros 类型标注 | `axconfig_macros` v0.2 依赖 TOML 中的 `# [(uint, uint)]` 类型注释来区分数组与元组生成。缺少此注释时生成 `&[&[usize]]`，`axdriver` 期望 `&[(usize, usize)]` 导致编译失败。单元素数组 `[[0,0]]` 无法通过类型推断补齐。 | Q19 构建踩坑 |
| <!-- L225 --> | BUS=mmio 对无 PCI 平台必需 | D1 无 PCI，`make lichee` 必须传 `BUS=mmio`，否则 `axdriver` 默认编译 PCI bus 代码并查找不存在的 `PCI_RANGES`/`PCI_BUS_END`。 | Q19 构建踩坑 |
| <!-- L226 --> | Cargo.lock 版本污染 | 对 workspace 内 path dependency 执行 `cargo check --manifest-path` 会升级 Cargo.lock 中未锁死的依赖（如 `axcpu 0.3` 松约束 → 0.3.1）。修复：用 `=0.3.0-preview.8` 精确锁死，然后 `git restore Cargo.lock` 恢复。 | Q19 构建踩坑 |
| <!-- L227 --> | D1 IRQ interface 分层 | StarryOS `lichee-d1` 构建会通过全局 feature 触发 `axplat/irq` 接口符号；如果 D1 axplat 不提供 `IrqIf`，链接会报 undefined `__IrqIf_register` / `__IrqIf_set_enable` / `__IrqIf_handle`。Q19a 采用 `irq-if = ["axplat/irq"]` + `irq_stub.rs` no-op `IrqIf`，先满足运行时符号，不提前启用 PLIC；完整 PLIC 放到后续 `irq` feature。 | Q19 构建踩坑 |
| <!-- L228 --> | D1 boot image 尺寸 gate | 未传 `DWARF=n` 时 raw binary / Android boot image 曾达到 `25.6M`，超过当前 boot 分区约 `10.1M` 容量。`rust-objcopy --strip-debug` 对该产物无效，因为调试相关段在当前链接布局中仍进入 raw binary。Lichee 构建必须在 `make lichee` 中强制 `DWARF=n`，或后续改 linker 明确丢弃 debug sections。 | Q19 烧录踩坑 |
| <!-- L229 --> | D1/C906 Store/AMO fault 根因 | 板测日志 `Store/AMO access fault EPC ffffffc040244648 TVAL ffffffc0402c6908` 已符号化：EPC 位于 `percpu::imp::init` 的 `amoor.w.aqrl`，TVAL 是 `.bss` 中 `percpu::imp::IS_INIT`。这不是 USB/SD/MMC 问题，而是 D1/C906 早期页表 DDR 映射缺少 T-Head C9xx normal-memory 属性。早期 DDR PTE 必须设置 `SH\|B\|C`（bits 60/61/62）。 | Q19 板测定位 |
| <!-- L230 --> | D1 final page table 风险 | Q19 当前修复覆盖 early boot page table；后续若在 `axmm` / `new_kernel_aspace` 切换最终页表后再次出现 AMO / load / store fault，应优先检查最终页表是否也带 `xuantie-c9xx` / T-Head C9xx PTE 属性，而不是先怀疑 UART 或 boot image。 | Q19 后续风险 |
| <!-- L231 --> | D1 smoke test 完成事实 | 2026-06-29 真板验证通过：官方 U-Boot Android boot image 成功加载 StarryOS，串口输出 `platform = riscv64-lichee-d1`、`sbi_version: 0.2`、`[starry-d1] early boot`、`[starry-d1] smoke complete, halting.`。这证明 D1 axplat、load/link 地址、Android boot image 打包、UART0 polling early console 已完成最小闭环。 | Q19 真板验收 |
| <!-- L232 --> | D1 final PTE 修复 | 早期 PTE 修复后仍可能在最终页表阶段 fault；`lichee-d1` feature 必须启用 `page_table_entry/xuantie-c9xx`，让 final page table 也带 T-Head C9xx memory attributes。 | Q19 final page table 修复 |
| <!-- L233 --> | D1 virtio 空 MMIO 修复 | D1 没有 virtio-mmio。`virtio-mmio-ranges` 必须写成空数组 `[]`，不能写成 `[[0,0]]` 占位；后者会让 `axdriver_virtio::probe_mmio_device` 访问 `phys_to_virt(0)`，fault VA 表现为 `0xffffffc000000000`。 | Q19 runtime fault 修复 |
| <!-- L234 --> | Lichee smoke feature gate | Lichee Q19a 只验证 boot + early console，必须把 fs/net/display/axdriver/PCI/task-ext 从 smoke 路径隔离。否则会出现 `No block device found!`、`PCI_ECAM_BASE`/`PCI_RANGES`/`PCI_BUS_END` 缺失，或 `TaskExt` extern_trait 链接符号缺失。QEMU 完整用户态路径通过 `starry-kernel/qemu` 保持这些特性。 | Q19 feature gate 修复 |
| <!-- L235 --> | D1 最小启动后扩展顺序 | Q19a 完成后不要回到官方 Linux 泛采集；后续按 PLIC/timer → SDMMC/block → rootfs/VFS → TTY/async UART → benchmark 顺序单独立项，避免把 benchmark 或 rootfs 问题误当成 early boot 阻塞。 | Q19 后续路线 |
<!-- tombstone: L236-L239 --> Archived 2026-07-02 in ARC-202607021648 — Q19B 计划期路线已执行；最终事实见 L240-L258、ADR-047~051、Q19B archived change。
| <!-- L240 --> | Q19B 三模式 feature 拆分方案 | Q19B 落地了 `lichee-d1`（smoke 回归，保持向后兼容）+ `lichee-d1-kbench`（内核 benchmark）+ `lichee-d1-userbench`（用户态 benchmark）三层 feature。`lichee-d1-kbench` 在 root Cargo.toml 中启用 `axplat-riscv64-lichee-d1/irq`（真 PLIC）替代 `irq-if`（no-op stub）；`lichee-d1-kbench` 通过 kernel Cargo.toml 的 `lichee-d1-kbench = []` 独立 feature gate 控制。模式拆分在 `kernel/src/entry.rs` 中通过 `#[cfg(feature = "lichee-d1-kbench")]` 条件编译实现，`lichee_d1_init()` 函数负责 kbench/userbench 共用初始化路径。 | Q19B Phase 1 |
| <!-- L241 --> | D1 DW APB UART 32-bit MMIO UartPort 实现 | `kernel/src/drivers/d1_uart.rs`（162 行新文件）实现 `ArceOsD1UartPort`：内部通过 stride-aware `read_reg(offset)`/`write_reg(offset, val)` 做 `base_ptr.add(offset * stride).cast::<u32>().read_volatile()`，实现了 `UartPort` trait 的 `receive_bytes`/`send_bytes`/`transmitter_empty`/`update_ier`。寄存器偏移：RBR/THR=0, IER=1, IIR=2, LSR=5（物理字节偏移 = offset × stride）。LSR 位定义与 NS16550 相同（bit0=DR, bit5=THRE, bit6=TEMT），但必须通过 u32 volatile 访问。`NonNull<u8>` 需要 `unsafe impl Send + Sync` 才能放入 `lazy_static!`。D1 ISR handler (`d1_uart_isr_handler`) 通过 IIR 的 stride-aware 32-bit 读取分派中断，复用 `uart_16550::async_::isr` 的 `RX_WAKER`/`TX_WAKER`/`DRAIN_WAKER`。 | Q19B Phase 2 |
| <!-- L242 --> | uart_init.rs 双路径 feature gate 模式 | `kernel/src/drivers/uart_init.rs` 通过 `#[cfg(not(feature = "lichee-d1-kbench"))]` 和 `#[cfg(feature = "lichee-d1-kbench")]` 维护 QEMU/D1 两条完全独立的硬件访问路径：不同的 `UartPort` 实现、不同的类型别名（`ArceOsDriver`/`ArceOsReader`/`ArceOsWriter` 各自做 cfg）和不同的 ISR wrapper。`init_uart_hardware()` 中的 raw `base+5` byte probe 被 `#[cfg(not(feature = "lichee-d1-kbench"))]` gate 排除在 D1 路径之外。common 部分（ring buffer 初始化、AsyncUartDriver 创建、ISR 注册、copier 启动）不做 feature gate。这是在不修改外部 crate 的前提下支持异构 UART 硬件的最小侵入模式。 | Q19B Phase 2 |
<!-- tombstone: L243 --> Archived 2026-07-02 in ARC-202607021648 — Q19B axfs 阻塞已解，最终约束见 ADR-049 与 L249/L252/L253。
| <!-- L244 --> | Q19B cargo check 三模式验证 | Q19B 开发中建立了 `cargo check --features lichee-d1 --target riscv64gc-unknown-none-elf`（smoke）、`cargo check --features lichee-d1-kbench --target riscv64gc-unknown-none-elf`（kbench）、`cargo check --features qemu --target riscv64gc-unknown-none-elf`（QEMU）三模式并行验证工作流。三模式全部通过 `cargo check` + `cargo clippy` 后才能声明 Phase 完成。`Makefile` 新增 `make lichee-kbench` 和 `make lichee-userbench` 目标，输出独立命名的 Android boot image。 | Q19B 验证模式 |
| <!-- L245 --> | Q19B userbench feature 继承陷阱 | 当前 `lichee-d1-userbench = ["lichee-d1-kbench"]`，而 `kernel/src/lib.rs` / `kernel/src/drivers/mod.rs` 又用 `feature = "lichee-d1-kbench"` 排除 `file`/`mm`/`pseudofs`/`task`/`ASYNC_TTY`。因此 userbench 编译会报 unresolved imports。经验：硬件能力 feature（D1 async UART/PLIC）和运行模式 feature（kbench-only halt）必须拆开，不能让 userbench 继承会排除用户态 runtime 的 kbench gate。 | `.claude/analysis/q19b-current-blockers.md` |
<!-- tombstone: L246-L247 --> Archived 2026-07-02 in ARC-202607021648 — Q19B 阻塞图与 Next 路线已执行；最终结果见 L248-L258。
| <!-- L248 --> | Q19B feature 规范化实战 | 实现后的 feature 布局：`lichee-d1-async-uart`（DW APB UART stride 4 + 真 PLIC — 硬件能力）→ `lichee-d1-kbench`（内核 benchmark 后 halt — 运行模式）和 `lichee-d1-userbench`（含 axfs/pseudofs/syscall 的最小白用户态 runtime — 运行模式）各自独立继承。关键约束：kbench 的模块排除（`file`/`mm`/`pseudofs`/`task`/`ASYNC_TTY`）绝不能影响 userbench。net socket/fb/axdisplay 模块通过 `#[cfg(not(feature = "lichee-d1"))]` 从 D1 路径完全排除。四模式 `cargo check` 全通过。 | Q19B-Next.1 实现 |
| <!-- L249 --> | D1 userbench 最小依赖集 | `lichee-d1-userbench` kernel feature 需要 `dep:axfs`（FS_CONTEXT 与伪文件系统操作）+ `axfeat/task-ext`（AxTaskExt 用户任务扩展）。明确不需要的：`axdisplay`（无 framebuffer）、`axdriver`（无 virtio block）、`axnet`（无网络）、PCI 相关常量。net/fb 子模块在父 mod.rs 中通过 `#[cfg(not(feature = "lichee-d1"))]` gate 排除；syscall 中的 socket 分派函数逐项加 `#[cfg(not(feature = "lichee-d1"))]`。 | Q19B-Next.2 实现 |
| <!-- L250 --> | D1 嵌入式 benchmark ELF 加载 | `kernel/resources/benchmark.elf`（约 38KB）通过 `include_bytes!` 嵌入 kernel binary。新函数 `load_embedded_user_app()` 在 `kernel/src/mm/loader.rs` 中实现（`#[cfg(feature = "lichee-d1-userbench")]`），绕过文件系统直接从 `&[u8]` 解析 ELF → 分配用户映射 → `uspace.write()` 复制段数据，并复用与 `load_user_app()` 相同的 AUXV、堆栈、heap 初始化模式。benchmark 进程以 `Process::new_init()` + `ASYNC_TTY::bind_to()` + `add_stdio()` 启动。重要限制：当前 loader 不处理 relocation，embedded ELF 必须是 `ET_EXEC` / no relocation，不能是 static PIE (`DYN`)。 | Q19B-Next.4 实现 |
| <!-- L251 --> | Q19B 最终 host gate 状态 | 四模式 `cargo check` 全部通过：`lichee-d1`（smoke, irq-if no-op stub）、`lichee-d1-kbench`（async UART + 真 PLIC + kernel benchmark → halt）、`lichee-d1-userbench`（async UART + axfs/pseudofs/syscall + embedded ELF user process）、`qemu`。真实 `make` gate 也通过：`make lichee-kbench` 生成 `starry-lichee-kbench-boot.img` (`kernel_size=188608`)，`make lichee-userbench` 生成 `starry-lichee-userbench-boot.img` (`kernel_size=876736`)。下一站是真板烧录验证（Next.5）。 | Q19B session 收尾 |
| <!-- L252 --> | axdriver `cfg(bus)` 不是普通 feature | `axdriver` 的 `build.rs` 会根据自身是否启用 `bus-mmio` feature 输出自定义 cfg：有 `bus-mmio` → `cargo:rustc-cfg=bus="mmio"`；否则默认 `cargo:rustc-cfg=bus="pci"`。因此 `cargo tree` 看不到 `bus-pci` feature 时，仍可能因为缺少 `bus-mmio` 而编译 `src/bus/pci.rs`。定位命令：查看 `target/*/build/axdriver-*/output`。 | Q19B axfs-ng patch 踩坑 |
| <!-- L253 --> | axfeat 弱转发不能修复间接 axdriver | `axfeat/bus-mmio = ["axdriver?/bus-mmio"]` 只会转发给 `axfeat` 自己的可选依赖 `axdriver`，不会自动影响 `axfs-ng` 间接拉进来的 `axdriver`。D1 userbench 通过 `dep:axfs` 引入 `axfs-ng -> axdriver` 时，必须在本地 patch 的 `crates/axfs-ng/Cargo.toml` 中显式写 `axdriver = { default-features = false, features = ["block", "bus-mmio"] }`，否则 axdriver build.rs 会 fallback 到 PCI。 | Q19B axfs-ng patch 踩坑 |
| <!-- L254 --> | embedded RISC-V benchmark 必须禁用 PIE | `riscv64-linux-musl-gcc -static` 可能产出 `DYN` / static PIE，并带 `R_RISCV_RELATIVE` relocation；当前 `load_embedded_user_app()` 不做 relocation。D1 embedded benchmark 必须用 `-static -no-pie -fno-pie -s` 生成 `ET_EXEC`，并用 `file`、`readelf -h`、`readelf -r` 验证 `Executable file` 与 `There are no relocations in this file.`。 | Q19B embedded ELF 踩坑 |
| <!-- L255 --> | D1 THRE 边沿丢失与 no-pending IRQ | 真板日志出现 PLIC 进入 UART IRQ 18 但 IIR=`0xc1`（bit0=1 no pending），同时有效 THRE 仅偶发 `0xc2`。QEMU 会稳定触发 THRE，掩盖了这个边界。D1 `ArceOsD1UartPort::update_ier(THR_EMPTY)` 在 LSR 已 THRE/TEMT 时必须软件 wake `TX_WAKER/DRAIN_WAKER`；ISR 中 no-pending 不能当成有效 modem/status，只能基于 LSR 补 wake。 | Q19B 真板 tcdrain 修复 |
| <!-- L256 --> | tcdrain/flush 必须覆盖 staged/TEMT 状态变化 | Q19B 真板第一次卡在 64B write 后的 `tcdrain`，根因是等待者只注册 TX ring waker，未覆盖 TX copier 已 pop 到 staged buffer 后的 `staged_bytes -> 0` 和 UART TEMT 变化。修复：`flush()`/`sys_ioctl(TCSBRK)` 始终注册 `DRAIN_WAKER`，TX copier 在最后一批数据送完且 TEMT 后主动 wake drain。 | Q19B 真板 tcdrain 修复 |
| <!-- L257 --> | D1 async UART 真板性能基线 | Lichee RV Dock UART0 @115200bps：256B TX 11.25KB/s (97.7% line rate)、1024B TX 11.40KB/s (98.9%)、4096B TX 11.41KB/s (99.0%)；64B TX `size=64 / iters=100 / 1.01KB/s / 8.8% line rate`（每轮 tcdrain，小包固定开销主导）；1B tcdrain latency avg 0.270ms / P50 0.185ms / P95 0.187ms / P99 8.547ms；FIONBIO open/ioctl 双入口 PASS。 | `docs/licheerv-dock-bringup.md` Q19B |
| <!-- L258 --> | 串口 benchmark 输出必须 CRLF + exit drain | 真板串口终端只输出 LF 会出现“斜行”：光标下移但不回行首；用户态 stdout 未 drain 前内核打印退出日志会插队。内核 `Tty::write_at()` 应按默认 termios `OPOST\|ONLCR` 做 LF→CRLF；`tests/benchmark.c` 也应使用 `\r\n`，并在 main 末尾执行 `fflush(stdout); tcdrain(STDOUT_FILENO);`。如果修改 C 源后要更新 embedded payload，需在正常环境用 musl 工具链重编 `kernel/resources/benchmark.elf`；当前 Codex 沙箱运行该 gcc 会 `Bad system call`。 | Q19B 输出清理 |
| <!-- L259 --> | Q19C 完整 benchmark 路径差异 | QEMU 完整路径通过 `entry::init -> pseudofs::mount_all -> FS_CONTEXT.resolve(args[0]) -> load_user_app -> add_stdio(/dev/console)` 启动；D1 Q19B userbench 当前通过 `init_memory_root -> include_bytes!(benchmark.elf) -> load_embedded_user_app` 绕过 rootfs/path loading。Q19C 应先让 D1 从 memory root 的 `/bin/benchmark` 走 `load_user_app()`，再做 SDMMC/rootfs parity。 | `.claude/analysis/q19c-lichee-full-starryos-benchmark.md` |
| <!-- L260 --> | D1 fullbench feature 边界 | 后续完整 StarryOS benchmark 不应启用 `qemu` feature；应新增独立 `lichee-d1-fullbench` 或等价 runtime mode，继承 D1 async UART/PLIC、paging、task-ext、axfs 和选定 rootfs provider，同时继续排除 QEMU PCI/virtio/display 假设。 | Q19C 探究 |
| <!-- L261 --> | D1 rootfs provider 分层 | D1 完整 benchmark 有两层 rootfs gate：先用 populated memory root 提供 `/bin/benchmark` 证明 VFS path loader，再实现真实 SDMMC/block rootfs 让 `axfs::init_filesystems(block_devs)` 接管。不要把 SDMMC bring-up 作为 path loading 的第一 blocker。 | Q19C 探究 |
| <!-- L262 --> | Q19C benchmark 证据口径 | Q19C 进入源码变更前应先规划 `benchmark.c` manifest：benchmark version、mode/startup chain、root provider、payload sizes、iteration counts、drain policy、timer source、nonblocking entries、RX test mode。QEMU、Q19B embedded、memory-root、shell/rootfs 的参数不同就分组解释，不能直接混成一条横向性能曲线。 | Q19C 重新探索 |

#### Scenario: 新增 Q13 层级 API

- **WHEN** 开发者在 uart_16550 async 栈中添加新类型或 trait
- **THEN** MUST 同时更新本速查表（上方 L176-L200 区域），标注文件路径与用途

<!-- arc: ARC-202607021648 --> 5 组 learned 条目已归档/压缩 (2026-07-02) → ../changes/archive/2026-07-02-ARC-202607021648/proposal.md

#### Scenario: 应用 Q15 增量融合策略

- **WHEN** 开发者需要合并 async-uart 相关分支（async-uart-1 / future / temp）的多个 commit
- **THEN** MUST 按 L208 启发式分层排序（基线 → 修复 → 规范化 → 契约）
- **AND** MUST 每步 cargo check + QEMU benchmark Gate（参见 ADR-039 Manual QA Gate 表）
- **AND** MUST 保留源分支作为参考，禁止一次性 merge 或删除
- **AND** 退化时立即停止，定位根因后从该 milestone 重新融合（L205 铁律）

#### Scenario: 启动 Lichee RV Dock 适配

- **WHEN** 开发者开始 StarryOS Lichee RV Dock 适配
- **THEN** MUST 使用 L213-L216 与 L231-L239 作为事实基线，不再重复从官方 Linux 泛采集
- **AND** MUST 先做 early console smoke test，再扩展 PLIC / rootfs / Shell / async benchmark
- **AND** MUST 将 UART 按 DW APB UART stride 4 / 32-bit MMIO 处理，禁止套用 QEMU stride=1 配置

#### Scenario: 新增真板平台适配

- **WHEN** 开发者为 StarryOS 新增 Lichee RV Dock、VisionFive2 或其他真板平台
- **THEN** MUST 先把平台事实记录到 platform descriptor 或等价集中配置中
- **AND** MUST 禁止在 `uart_init.rs` 等驱动初始化路径继续散落板级 base / irq / stride / access width 常量
- **AND** MUST 先完成 polling early console smoke test，再接 async UART、PLIC、timer、rootfs 或 benchmark
