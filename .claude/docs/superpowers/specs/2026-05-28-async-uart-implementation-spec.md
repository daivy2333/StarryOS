# AsyncUart 异步串口驱动技术规格说明书

> **Spec ID**: ASYNC_UART_SPEC_001
> **Version**: 1.0
> **Date**: 2026-05-28
> **Branch**: feat/uart-async-dev2
> **Goal**: 完全剔除 Console，实现高性能异步串口驱动（AsyncUart）

---

## 1. 项目背景与目标

### 1.1 背景

**当前问题**：
- Console（axhal::console）使用同步阻塞 TX（polling 空转等待 THR_EMPTY）
- Console 与 AsyncUart 共享 UART 硬件导致数据竞争风险
- Console tty-reader 与 AsyncUart copier 共用 IRQ 10 waker 冲突
- Console UART 初始化配置不兼容 AsyncUart（只使能 RX 中断，禁用 TX 中断）

**历史失败**：
- feat/uart-async 分支尝试渐进式集成方案失败（M3 替换失败，IRQ 风暴 + TX busy-loop）
- 根因：UART 状态异常（THR_EMPTY 标志不更新）、数据竞争、IRQ waker 冲突

### 1.2 目标

**完全剔除 Console**：
- 从零开始实现 AsyncUart 异步串口驱动
- 不依赖 axplat UART 初始化，使用本地 uart_16550 crate
- AsyncUart 独占 UART 硬件（IRQ 10 + MMIO 0x10000000）
- 提供用户态异步串口 API（read/write/poll/select/epoll）

**性能目标**：
- 波特率支持：115200 bps（可扩展至 1 Mbps）
- RX 延迟：P50 < 500 µs, P99 < 2 ms
- CPU 利用率（空闲）：0%（中断驱动，无空转）
- 吞吐量：> 90% 线速（115200 bps 下 > 10 KB/s）

**调试安全**：
- earlycon（axhal::console）保留用于内核日志、panic 输出
- panic 时 earlycon 强制输出（禁用 AsyncUart TX 中断）
- 启动早期可用（axruntime::init_early 后立即可用）

---

## 2. 系统架构设计

### 2.1 软件架构层次

```
用户态应用（shell、用户程序）
  ↓ syscall read/write/poll
VFS 层（File → FileLike → Device → DeviceOps）
  ↓
AsyncUartDevice（DeviceOps + Pollable trait）
  ↓
AsyncBuffer（rx_buf + tx_buf + PollSet）
  ↓
RX/TX copier 任务（poll_fn + register_irq_waker）
  ↓
ISR 分发机制（uart_isr_handler → rx_waker/tx_waker）
  ↓
UART 硬件（uart_16550 crate）
  ↓
UART MMIO（0x10000000, IRQ 10）
```

### 2.2 核心组件设计

**组件 1：UART 硬件初始化（uart_init.rs）**
- 职责：替代 axplat UART 初始化，配置 AsyncUart 专用参数
- 关键配置：IER::DATA_READY | IER::THR_EMPTY（使能 RX + TX 中断）
- 初始化时机：kernel entry.rs 早期调用
- 状态验证：log_uart_state() 输出寄存器状态

**组件 2：ISR 分发机制（isr.rs）**
- 职责：IRQ 10 中断分发，精确唤醒 rx_waker/tx_waker
- ISR 执行原则：读 ISR 寄存器 → 判断 InterruptType → 禁用中断 → 唤醒 waker
- ISR 安全约束：无阻塞、无锁、MMIO read/write 安全

**组件 3：AsyncBuffer（ring_buffer.rs）**
- 职责：RX/TX 环形缓冲区 + PollSet 管理
- 数据结构：HeapRb<u8>（4KB RX + 4KB TX）+ PollSet（poll_rx + poll_tx）
- 同步机制：Mutex 保护 ringbuf + PollSet 唤醒等待任务

**组件 4：RX/TX copier 任务（async_driver.rs）**
- 职责：UART 硬件 FIFO ↔ ringbuf 数据搬运
- RX copier：IRQ → read UART FIFO → push rx_buf → 唤醒用户态 read()
- TX copier：pop tx_buf → write UART FIFO → IRQ → 唤醒用户态 write()

**组件 5：AsyncUartDevice（device_ops.rs）**
- 职责：DeviceOps trait 实现，VFS 集成
- 接口：read_at/write_at/as_pollable/flags
- 异步支持：返回 WouldBlock → poll_io 自动注册 waker

---

## 3. 功能需求规格

### 3.1 UART 硬件初始化（P1）

**需求 ID**: REQ_UART_INIT_001

**功能描述**：
- 使用 uart_16550 crate 本地初始化 UART 硬件
- 配置波特率：115200 bps
- 配置 FIFO：使能，触发阈值 14 字节
- 配置中断：IER::DATA_READY | IER::THR_EMPTY（RX + TX 中断）
- 配置数据格式：8-N-1（8 数据位，无校验，1 停止位）

**输入约束**：
- UART MMIO 基地址：0x10000000（硬编码）
- 寄存器 stride：4（RISC-V MMIO 标准）
- 时钟频率：1.8432 MHz（uart_16550 标准）

**输出约束**：
- IER 寄存器值：0x03（IER::DATA_READY | IER::THR_EMPTY）
- ISR 寄存器值：FIFO enabled（bits 7:6 = 0b11）
- LSR 寄存器值：TX transmitter empty（bit 6 = 1）

**验证标准**：
- UART 初始化成功：log_uart_state() 输出寄存器状态正确
- 编译验证：cargo check 编译通过
- QEMU 运行验证：make run 内核启动，UART 寄存器配置可见

---

## 3.2 ISR 分发机制（P2）

**需求 ID**: REQ_ISR_DISPATCH_001

**功能描述**：
- 实现 uart_isr_handler 函数，IRQ 10 中断分发
- ISR 读 ISR 寄存器判断 InterruptType
- 根据中断类型唤醒对应的 waker：
  - ReceivedDataReady/ReceptionTimeout → rx_waker.wake()
  - TransmitterHoldingRegisterEmpty → tx_waker.wake()
  - ReceiverLineStatus → 读 LSR 清除错误标志

**ISR 执行约束**：
- 无阻塞：ISR 中不允许获取 Mutex、不允许阻塞等待
- 无锁：使用 AtomicWaker（CriticalSectionRawMutex 保护）
- 最小工作：读 ISR + 禁用中断 + 唤醒 waker（数据搬运推迟到 copier）

**输入约束**：
- IRQ 号：10（PLIC）
- ISR 寄存器：ISR.interrupt_type() 返回 InterruptType 枚举
- UART 实例：全局静态 UART（SpinNoIrq<Mutex<Uart16550<MmioBackend>>）

**输出约束**：
- rx_waker 被唤醒（RX 中断到来时）
- tx_waker 被唤醒（TX 空中断到来时）
- UART IER 寄存器临时禁用对应中断（防止重入）

**验证标准**：
- ISR 注册成功：register_irq_hook(10, uart_isr_handler) 返回 true
- IRQ 10 使能：axhal::irq::set_enable(10, true)
- ISR 分发正确：RX 中断 → rx_waker 唤醒，TX 中断 → tx_waker 唤醒

---

## 3.3 RX/TX copier 任务（P2）

**需求 ID**: REQ_COPIER_TASK_001

**功能描述**：
- RX copier 任务：UART RX FIFO → rx_buf 环形缓冲区
- TX copier 任务：tx_buf 环形缓冲区 → UART TX FIFO
- 任务循环：poll_fn + register_irq_waker + AtomicWaker

**RX copier 工作流程**：
```
1. 尝试读 UART RX FIFO（try_receive_bytes）
2. 如果有数据 → push rx_buf → 唤醒 poll_rx → 返回 Ready
3. 如果无数据 → 使能 RX 中断（IER::DATA_READY） → 注册 rx_waker → 返回 Pending
4. ISR 唤醒 → 重新执行步骤 1
```

**TX copier 工作流程**：
```
1. 尝试从 tx_buf pop 数据
2. 如果有数据 → 检查 THR_EMPTY → write UART TX FIFO → 唤醒 poll_tx → 返回 Ready
3. 如果无数据或 FIFO 满 → 使能 TX 中断（IER::THR_EMPTY） → 注册 tx_waker → 返回 Pending
4. ISR 唤醒 → 重新执行步骤 1
```

**输入约束**：
- UART 实例：全局静态 UART
- rx_buf/tx_buf：HeapRb<u8>（容量 4096）
- rx_waker/tx_waker：AtomicWaker

**输出约束**：
- RX 数据从 UART FIFO 正确搬运到 rx_buf
- TX 数据从 tx_buf 正确搬运到 UART FIFO
- 无数据丢失、无数据竞争

**验证标准**：
- copier 任务启动成功：spawn_with_name("rx-copier") + spawn_with_name("tx-copier")
- 数据搬运正确：发送 1KB 数据 → 接收 1KB 数据，无丢失
- 延迟测量：RX 数据到达延迟 < 500 µs

---

## 3.4 DeviceOps trait 实现（P4）

**需求 ID**: REQ_DEVICEOPS_001

**功能描述**：
- 实现 AsyncUartDevice 结构体，实现 DeviceOps trait
- 实现 Pollable trait，支持 poll/select/epoll
- 注册设备到 /dev/async_uart（DeviceId::new(4, 64））

**DeviceOps trait 方法**：
- `read_at(buf, offset)`：从 rx_buf pop 数据，返回 WouldBlock 如果缓冲区空
- `write_at(buf, offset)`：向 tx_buf push 数据，返回 WouldBlock 如果缓冲区满
- `as_pollable()`：返回 Some(self)（支持 poll）
- `flags()`：返回 NodeFlags::NON_CACHEABLE | NodeFlags::STREAM

**Pollable trait 方法**：
- `poll()`：检查 rx_buf/tx_buf 状态，返回 IoEvents（IN/OUT）
- `register(context, events)`：注册 waker 到 poll_rx/poll_tx

**输入约束**：
- rx_buf/tx_buf：AsyncBuffer 实例
- poll_rx/poll_tx：PollSet 实例

**输出约束**：
- 用户态 open("/dev/async_uart") → 成功返回 fd
- 用户态 read(fd, buf) → 正确读取数据
- 用户态 write(fd, buf) → 正确写入数据
- 用户态 poll(&pollfd) → 正确返回 IoEvents

**验证标准**：
- 设备注册成功：/dev/async_uart 节点存在
- VFS 集成正确：Device → File → FD_TABLE 流程正确
- 用户态 API 可用：read/write/poll 功能正常

---

## 3.5 Console 软件路径剔除（P3）

**需求 ID**: REQ_CONSOLE_REMOVE_001

**功能描述**：
- 完全剔除 Console 软件路径（不依赖 axhal::console）
- 删除 Console struct、N_TTY 全局变量、tty-reader copier
- 移除 /dev/console 设备注册
- AsyncUart 独占 UART 硬件（IRQ 10 + MMIO 0x10000000）

**剔除范围**：
- `kernel/src/pseudofs/dev/tty/ntty.rs`：删除 Console struct + N_TTY + new_n_tty()
- `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`：删除 tty-reader copier（InputReader）
- `kernel/src/pseudofs/dev/mod.rs`：移除 /dev/console 设备注册
- `kernel/src/entry.rs`：移除 N_TTY.bind_to(&proc) 调用

**保留内容**：
- PTY 子系统（不依赖 Console 硬件）
- termios 行规则（可迁移到 AsyncUart）
- job_control（可迁移到 AsyncUart）

**约束**：
- axhal::console 保留用于 earlycon（内核日志）
- earlycon 不与 AsyncUart 共享 UART 硬件（软件路径分离）

**验证标准**：
- Console 代码完全剔除：grep "Console" 无结果
- /dev/console 设备不存在：ls /dev/console 失败
- AsyncUart 独占 UART：IRQ 10 只唤醒 AsyncUart copier

---

## 3.6 earlycon 内核日志（P1）

**需求 ID**: REQ_EARLYCON_001

**功能描述**：
- 复用 axhal::console 作为 earlycon（无需额外实现）
- Polling TX 输出（同步阻塞，调试安全）
- Panic 时强制输出（禁用 AsyncUart TX 中断）

**earlycon 特性**：
- 启动早期可用：axruntime::init_early 后立即可用（比 AsyncUart 早 10-20 ms）
- Panic 安全：panic handler 使用 ax_println! → axhal::console::write_bytes()
- 独立路径：不依赖异步框架

**共存策略**：
- AtomicBool 标记：EARLYCON_DISABLED（AsyncUart 使用时禁用 earlycon）
- 自旋锁保护：UART 硬件访问使用 SpinNoIrq
- Panic 强制输出：忽略 EARLYCON_DISABLED 标记

**输入约束**：
- axhal::console：外部 crate（不可修改）
- UART 硬件：与 AsyncUart 共享（MMIO 0x10000000）

**输出约束**：
- 内核启动日志可见：ax_println! 输出到 UART
- Panic 信息输出：panic handler 强制输出
- 性能影响：启动时间增加 100-200 ms（Polling TX）

**验证标准**：
- 启动日志输出：make run 可见内核启动信息
- Panic 输出：触发 panic 可见 panic 信息
- earlycon 与 AsyncUart 不冲突：EARLYCON_DISABLED 标记正确工作

---

## 4. 接口定义规格

### 4.1 UART 硬件接口

**uart_init.rs 公共接口**：

```rust
/// 初始化 UART 硬件（AsyncUart 专用配置）
pub fn init_uart_hardware() {
    // 配置波特率、FIFO、中断、数据格式
    // 验证 UART 寄存器状态
}

/// 全局 UART 实例（AsyncUart 独占访问）
pub static UART: Mutex<Uart16550<MmioBackend>> = ...;
```

**uart_16550 crate API 使用**：
- `SerialPort::new_mmio(base, stride)` — 创建 UART 实例
- `uart.init(&Config)` — 初始化 UART 硬件
- `uart.ier()` — 读 IER 寄存器
- `uart.isr()` — 读 ISR 寄存器
- `uart.lsr()` — 读 LSR 寄存器
- `uart.try_send_bytes(buf)` — 非阻塞写 TX
- `uart.try_receive_bytes(buf)` — 非阻塞读 RX
- `uart.set_interrupt_enable(IER)` — 设置中断使能

---

## 4.2 ISR 分发接口

**isr.rs 公共接口**：

```rust
/// RX waker（唤醒 RX copier）
pub static RX_WAKER: AtomicWaker = AtomicWaker::new();

/// TX waker（唤醒 TX copier）
pub static TX_WAKER: AtomicWaker = AtomicWaker::new();

/// UART ISR handler（IRQ 10 分发）
pub fn uart_isr_handler() {
    // 读 ISR 寄存器判断中断类型
    // 禁用对应中断（防止重入）
    // 唤醒 rx_waker/tx_waker
}

/// 注册 UART ISR 到 IRQ 10
pub fn register_uart_isr() {
    // register_irq_hook(10, uart_isr_handler)
    // axhal::irq::set_enable(10, true)
}
```

---

## 4.3 AsyncBuffer 接口

**ring_buffer.rs 公共接口**：

```rust
/// AsyncBuffer：RX/TX 环形缓冲区 + PollSet
pub struct AsyncBuffer {
    rx_buf: Mutex<HeapRb<u8>>,   // RX 环形缓冲区（4KB）
    tx_buf: Mutex<HeapRb<u8>>,   // TX 环形缓冲区（4KB）
    poll_rx: PollSet,            // RX waker 集合
    poll_tx: PollSet,            // TX waker 集合
}

impl AsyncBuffer {
    pub fn new_default() -> Self { ... }
    
    /// 从 RX buffer pop 数据（用户态 read）
    pub fn pop_rx(&self, buf: &mut [u8]) -> usize { ... }
    
    /// 向 TX buffer push 数据（用户态 write）
    pub fn push_tx(&self, buf: &[u8]) -> usize { ... }
    
    /// 向 RX buffer push 数据（RX copier）
    pub fn push_rx_from_uart(&self, buf: &[u8]) -> usize { ... }
    
    /// 从 TX buffer pop 数据（TX copier）
    pub fn pop_tx_to_uart(&self, buf: &mut [u8]) -> usize { ... }
}
```

---

## 4.4 DeviceOps trait 接口

**device_ops.rs 公共接口**：

```rust
/// AsyncUartDevice：DeviceOps + Pollable 实现
pub struct AsyncUartDevice {
    buffer: Arc<AsyncBuffer>,
    uart: Arc<Uart16550Async>,  // AsyncUart trait 实现
}

impl DeviceOps for AsyncUartDevice {
    fn read_at(&self, buf: &mut [u8], offset: u64) -> AxResult<usize> { ... }
    fn write_at(&self, buf: &[u8], offset: u64) -> AxResult<usize> { ... }
    fn as_pollable(&self) -> Option<&dyn Pollable> { Some(self) }
    fn flags(&self) -> NodeFlags { NodeFlags::NON_CACHEABLE | NodeFlags::STREAM }
}

impl Pollable for AsyncUartDevice {
    fn poll(&self) -> IoEvents { ... }
    fn register(&self, context: &mut Context, events: IoEvents) { ... }
}
```

---

## 5. 实现约束规格

### 5.1 编码规范约束

**遵循三大规则**（见 .claude/docs/rules.md）：
- Iron Law：禁止假设不明确、禁止过度复杂
- Karpathy Guidelines：Think Before Coding、Implementation Simplicity、Surgical Changes
- Workflow Violation：禁止 Gate BLOCK 不记录

**Rust 特定规范**：
- unsafe 块必须有 SAFETY 注释
- 核心逻辑必须有测试覆盖
- 函数 < 20 行，单一职责
- 命名清晰，揭示意图

---

## 5.2 硬件约束

**UART 硬件约束**：
- UART 型号：NS16550（QEMU virt 平台）
- MMIO 基地址：0x10000000（硬编码）
- IRQ 号：10（PLIC）
- 寄存器 stride：4（RISC-V MMIO 标准）
- FIFO 大小：16 字节

**中断约束**：
- ISR 在中断上下文中执行（不允许阻塞）
- register_irq_hook 全局唯一（只能注册一次）
- register_irq_waker 支持多 waker（BTreeMap<usize, PollSet>）

---

## 5.3 依赖约束

**外部 crate 约束**：
- uart_16550：本地 path 依赖（../../uart_16550）
- embassy-sync：v0.6.2（用于 AtomicWaker）
- axtask：0.3.0-preview.2（用于 poll_io + register_irq_waker）
- axhal：外部 crate（axhal::console 保留用于 earlycon，不可修改）
- axplat：外部 crate（axplat-riscv64-qemu-virt UART 初始化不可修改）

---

## 6. 测试标准规格

### 6.1 UART 硬件初始化测试（P1）

**测试 ID**: TEST_UART_INIT_001

**测试场景**：
- UART 初始化成功：IER/ISR/LSR 寄存器值正确
- UART 状态验证：log_uart_state() 输出可见

**测试方法**：
- 内核启动验证：make run 内核启动，UART 寄存器配置日志可见
- 编译验证：cargo check 编译通过

**验证标准**：
- IER 寄存器值：0x03（IER::DATA_READY | IER::THR_EMPTY）
- ISR 寄存器值：bits 7:6 = 0b11（FIFO enabled）
- LSR 寄存器值：bit 6 = 1（TX transmitter empty）

---

## 6.2 ISR 分发机制测试（P2）

**测试 ID**: TEST_ISR_DISPATCH_001

**测试场景**：
- ISR 注册成功：register_irq_hook(10, uart_isr_handler) 返回 true
- ISR 分发正确：RX 中断 → rx_waker 唤醒，TX 中断 → tx_waker 唤醒

**测试方法**：
- 内核启动验证：ISR 注册日志可见
- 中断触发验证：发送数据触发 RX 中断，ISR 唤醒 rx_waker

**验证标准**：
- ISR 注册日志："[UART ISR] Registered UART ISR to IRQ 10"
- RX 中断日志："[UART ISR] RX interrupt, woke RX copier"
- TX 中断日志："[UART ISR] THR empty, woke TX copier"

---

## 6.3 RX/TX copier 任务测试（P2）

**测试 ID**: TEST_COPIER_TASK_001

**测试场景**：
- copier 任务启动成功：spawn_with_name("rx-copier") + spawn_with_name("tx-copier")
- 数据搬运正确：发送 1KB 数据 → 接收 1KB 数据

**测试方法**：
- 内核内部测试：kernel/src/drivers/test.rs（启动时自动执行）
- 用户态测试：用户态程序打开 /dev/async_uart，read/write 数据

**验证标准**：
- copier 任务启动日志："[RX COPIER] started" + "[TX COPIER] started"
- 数据搬运正确：发送 1024 字节 → 接收 1024 字节，无丢失
- 延迟测量：RX 数据到达延迟 < 500 µs

---

## 6.4 DeviceOps trait 测试（P4）

**测试 ID**: TEST_DEVICEOPS_001

**测试场景**：
- 设备注册成功：/dev/async_uart 节点存在
- 用户态 API 可用：open/read/write/poll 功能正常

**测试方法**：
- 用户态测试程序：打开 /dev/async_uart，read/write 数据，poll 监听事件

**验证标准**：
- 设备注册成功：ls /dev/async_uart 成功
- 用户态 read：read(fd, buf) 正确读取数据
- 用户态 write：write(fd, buf) 正确写入数据
- 用户态 poll：poll(&pollfd) 正确返回 IoEvents

---

## 7. 性能指标规格

### 7.1 吞吐量指标

**指标 ID**: PERF_THROUGHPUT_001

**目标**：
- 波特率 115200 bps：吞吐量 > 10 KB/s（> 90% 线速）
- 波特率 1 Mbps：吞吐量 > 100 KB/s（可扩展）

**测量方法**：
- 发送 1MB 数据，测量时间，计算吞吐量

**验证标准**：
- 115200 bps：吞吐量 > 10 KB/s ✅
- 1 Mbps：吞吐量 > 100 KB/s（可选）

---

## 7.2 延迟指标

**指标 ID**: PERF_LATENCY_001

**目标**：
- RX 延迟：P50 < 500 µs, P99 < 2 ms
- TX 延延：P50 < 500 µs, P99 < 2 ms

**测量方法**：
- 发送单字节，测量从 UART TX 到 UART RX 的时间

**验证标准**：
- RX 延迟 P50 < 500 µs ✅
- RX 延迟 P99 < 2 ms ✅

---

## 7.3 CPU 利用率指标

**指标 ID**: PERF_CPU_UTIL_001

**目标**：
- CPU 利用率（空闲）：0%（中断驱动，无空转）
- CPU 利用率（吞吐）：< 10%（数据搬运）

**测量方法**：
- 无数据传输时测量 CPU 利用率

**验证标准**：
- CPU 利用率（空闲）= 0% ✅

---

## 8. 风险评估规格

### 8.1 技术风险

| 风险类型 | 风险描述 | 影响等级 | 缓解措施 |
|---------|---------|---------|---------|
| **UART 重初始化冲突** | axplat UART 初始化状态可能不兼容 AsyncUart | 高 | kernel entry 早期覆盖 axplat 配置 |
| **ISR 分发机制首次使用** | ISR 读 ISR 寄存器判断中断类型，首次验证 | 中 | 充足调试信息，ISR 状态日志 |
| **earlycon 与 AsyncUart 共存** | 共享 UART 硬件可能数据竞争 | 中 | AtomicBool 标记 + 自旋锁保护 |
| **Console 剔除范围过大** | 可能误删 PTY 或 termios 相关代码 | 低 | 参考 docs/analysis/console-removal-scope-analysis.md |

---

## 8.2 依赖风险

| 风险类型 | 风险描述 | 影响等级 | 缓解措施 |
|---------|---------|---------|---------|
| **uart_16550 crate API 变化** | 本地 v0.6.0 与 crates.io v0.4.0 API 不同 | 低 | 使用本地版本，完整 API 分析 |
| **axhal::console 不可修改** | 外部 crate，无法修改 polling TX 实现 | 中 | 复用现有实现，无需修改 |
| **register_irq_hook 全局唯一** | 只能注册一次，可能与其他 ISR 冲突 | 中 | 验证 ISR 注册成功，无冲突 |

---

## 9. 参考文档索引

**设计分析文档**（docs/analysis/）：
- Console UART 研究：console-uart-mechanism.md
- Console 软件路径剔除范围：console-removal-scope-analysis.md
- UART 硬件初始化替代方案：uart-init-design.md
- earlycon 内核日志设计：earlycon-design.md
- AsyncUart 设备注册方案：async-uart-device-registration.md
- IRQ waker 分发机制验证：irq-waker-mechanism-verification.md

**架构决策记录**（.claude/docs/architecture.md）：
- ADR-020：分支策略变更（完全剔除 Console）
- ADR-021：四个关键架构决策

**知识积累**（.claude/docs/learned.md）：
- L94-L112：uart_16550 API、ISR 分发机制、DeviceOps trait 等

---

**Spec 文档完成** — 待生成详细执行计划（Plan 文档）