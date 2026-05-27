> ⚠️ 此文档为早期分析，部分内容已过时。
> 最新决策参见 architecture.md ADR-013~ADR-015。

# UART 16550 硬件与驱动深度分析

> 基于 StarryOS 代码库及 axplat-riscv64-qemu-virt 平台实现的分析
> 分析日期：2026-05-24

---

## 1. 硬件配置

### 1.1 QEMU virt 平台 UART 参数

| 参数 | 值 | 来源 |
|------|-----|------|
| UART 型号 | NS16550A 兼容 | QEMU virt 默认 |
| 物理基地址 | 0x1000_0000 | axconfig.toml |
| 虚拟地址 | 0xFFFFFFC0_10000000 (PHYS_VIRT_OFFSET + PADDR) | MMIO 映射 |
| IRQ 号 | 0x0a (PLIC 外部中断号 10) | axconfig.toml |
| 时钟频率 | 由 QEMU 模拟，不影响编程 | — |
| FIFO 深度 | TX=16 bytes, RX=16 bytes | 16550 规范 |

### 1.2 QEMU 串口拓扑

当前 QEMU 启动参数（`make/qemu.mk`）在非图形模式下使用 `-nographic`，将第一个 UART 映射到 stdio。QEMU virt 默认只有一个 UART（0x10000000）。

**添加第二个串口**：需要在 QEMU 参数中添加：
```
-chardev socket,id=serial1,port=4444,host=localhost -serial chardev:serial1
```
或使用 `pty`/`file` 方式：
```
-serial pty -serial mon:stdio
```
第二个 UART 的物理地址和 IRQ 需要确认 QEMU virt 平台是否支持。**存疑：QEMU virt 平台是否真的支持第二个 16550 UART？**

---

## 2. 16550 寄存器映射

基于 `uart_16550` crate（v0.4.0）`MmioSerialPort` 结构体：

| 偏移 | 寄存器 | 读功能 | 写功能 |
|------|--------|--------|--------|
| +0 | RBR/THR | 接收缓冲寄存器 | 发送保持寄存器 |
| +1 | IER | 中断使能寄存器 | 中断使能寄存器 |
| +2 | IIR/FCR | 中断标识寄存器 | FIFO 控制寄存器 |
| +3 | LCR | 线路控制寄存器 | 线路控制寄存器 |
| +4 | MCR | Modem 控制寄存器 | Modem 控制寄存器 |
| +5 | LSR | 线路状态寄存器 | 线路状态寄存器 |
| +6 | MSR | Modem 状态寄存器 | Modem 状态寄存器 |
| +7 | SCR | 暂存寄存器 | 暂存寄存器 |

### 2.1 关键寄存器详解

**IER（Interrupt Enable Register, +1）**：
```
Bit 0: ERBFI - 使能接收数据中断
Bit 1: ETBEI - 使能发送保持寄存器空中断
Bit 2: ELSI  - 使能线路状态中断
Bit 3: EDSSI - 使能 Modem 状态中断
Bit 7-4: 保留
```

**IIR（Interrupt Identification Register, +2，只读）**：
```
Bit 0: 0=有待处理中断, 1=无待处理中断
Bit 3:2: 中断源标识
  000 = Modem 状态变化
  001 = 发送保持寄存器空
  010 = 接收数据就绪
  011 = 接收线状态变化
  110 = 字符超时（FIFO 模式下有数据但未达到触发阈值）
Bit 7:6: FIFO 使能状态（11=FIFO 使能）
```

**LSR（Line Status Register, +5，只读）**：
```
Bit 0: DR   - 数据就绪（RX FIFO 非空）
Bit 1: OE   - 溢出错误
Bit 5: THRE - 发送保持寄存器空
Bit 6: TEMT - 发送器空（THR 和移位寄存器都空）
Bit 7: FIFO 数据错误
```

**FCR（FIFO Control Register, +2，只写）**：
```
Bit 0: FE   - 使能 FIFO
Bit 1: XFR  - 清空 RX FIFO
Bit 2: XFT  - 清空 TX FIFO
Bit 7:6: RX 触发阈值
  00 = 1 字节
  01 = 4 字节
  10 = 8 字节
  11 = 14 字节
```

---

## 3. 当前驱动实现

### 3.1 平台层（axplat-riscv64-qemu-virt）

**初始化**（console.rs）：
```rust
static UART: LazyInit<SpinNoIrq<MmioSerialPort>> = LazyInit::new();

pub(crate) fn init_early() {
    UART.init_once({
        let mut uart = unsafe { MmioSerialPort::new(UART_PADDR + PHYS_VIRT_OFFSET) };
        uart.init();
        SpinNoIrq::new(uart)
    });
}
```

`uart.init()` 执行的操作（uart_16550 crate mmio.rs:50-81）：
1. 禁用所有中断（IER = 0x00）
2. 设置 DLAB，配置分频器（DLL/DLM）
3. 设 8N1（LCR = 0x03）
4. 使能 FIFO，清空 TX/RX 队列（FCR = 0xC7）
5. 使能 Modem 控制（MCR = 0x0B）
6. **使能接收数据中断（IER = 0x01）** ← 关键！初始化时已开启 RX 中断

**Console 接口**：
- `write_bytes(buf)`: 循环写每个字节，`\n` 自动转 `\r\n`，等待 THRE 标志后写入
- `read_bytes(buf)`: 循环读，`try_receive` 返回 Err 时结束
- `irq_num()`: 返回 `Some(UART_IRQ)` 即 `Some(0x0a)`

### 3.2 内核层（N_TTY）

N_TTY 是当前唯一使用 UART 的内核组件：
- `Console` 结构体实现 `TtyRead`/`TtyWrite`
- 读操作直接调用 `axhal::console::read_bytes()`
- 写操作直接调用 `axhal::console::write_bytes()`
- 中断模式：`ProcessMode::External`，使用 `register_irq_waker(irq, &waker)` 注册

---

## 4. 当前驱动的问题与约束

### 4.1 发送路径是同步阻塞的

`write_bytes()` 是一个忙等待循环：
```rust
for &byte in bytes {
    while !uart.line_sts().contains(LineStsFlags::OUTPUT_EMPTY) {}
    uart.send_raw(byte);
}
```
这意味着每次 `printf` 或日志输出都会阻塞当前 CPU，直到所有字节写入 TX FIFO。

### 4.2 接收路径依赖 tty-reader 后台任务

N_TTY 的 `ProcessMode::External` 模式会 spawn 一个 `tty-reader` 内核任务：
- 该任务 `block_on(poll_fn(|cx| { ... }))` 永久运行
- 通过 `register_irq_waker(irq, waker)` 注册中断唤醒
- 中断到来时唤醒任务，任务调用 `reader.poll()` 从硬件 FIFO 读取

### 4.3 无独立的硬件抽象层

当前 UART 硬件操作完全封装在 `axhal::console` 三个函数中，无法：
- 分别使能/禁用 RX/TX 中断
- 读取 IIR 获取中断源
- 配置 FIFO 触发阈值
- 直接访问 MMIO 寄存器做更细粒度控制

---

## 5. 异步串口需要的硬件操作

### 5.1 ISR 需要的能力

ISR（中断服务例程）需要：
1. 读取 IIR 判断中断源（RX 就绪 / TX 空 / 其他）
2. 清除中断标志（对 16550，读取 IIR 或读写 RBR/THR 即自动清除）
3. **禁用已触发的中断**（避免中断风暴）—— 特别是 TX 空中断
4. 唤醒对应的 Waker

### 5.2 Copier 任务需要的能力

后台数据搬运任务需要：
1. `try_read(buf) -> usize`：非阻塞从 RX FIFO 读取
2. `try_write(buf) -> usize`：非阻塞写入 TX FIFO
3. `enable_rx_intr()` / `disable_rx_intr()`：控制 RX 中断
4. `enable_tx_intr()` / `disable_tx_intr()`：控制 TX 中断
5. `read_iir() -> u8`：读取中断标识

### 5.3 当前 API 缺口

| 需要的操作 | axhal::console 现有 | 差距 |
|-----------|-------------------|------|
| 非阻塞读 | `read_bytes` (批量) | 需返回实际读取数 |
| 非阻塞写 | `write_bytes` (忙等) | 需非阻塞版本 |
| 使能 RX 中断 | 无 | 需新增 |
| 禁用 RX 中断 | 无 | 需新增 |
| 使能 TX 中断 | 无 | 需新增 |
| 禁用 TX 中断 | 无 | 需新增 |
| 读取 IIR | 无 | 需新增 |
| 配置 FIFO 触发 | 无 | 需新增 |

---

## 6. 两种扩展路径

### 6.1 路径 A：扩展 axhal::console API

在 `axplat::ConsoleIf` trait 中增加方法：
```rust
trait ConsoleIf {
    // 现有
    fn write_bytes(buf: &[u8]);
    fn read_bytes(buf: &mut [u8]) -> usize;
    fn irq_num() -> Option<u32>;

    // 新增
    fn try_read(buf: &mut [u8]) -> usize;       // 非阻塞读
    fn try_write(buf: &[u8]) -> usize;          // 非阻塞写
    fn enable_rx_intr();
    fn disable_rx_intr();
    fn enable_tx_intr();
    fn disable_tx_intr();
    fn read_iir() -> u8;
}
```

**优点**：保持平台抽象层的一致性
**缺点**：需要修改 axplat crate（外部依赖，在 crates.io 上）

### 6.2 路径 B：内核直接操作 MMIO

在内核中直接映射 UART MMIO 寄存器，绕过 axhal console：
```rust
// kernel/src/drivers/uart_async/uart_16550.rs
pub struct Uart16550 {
    base: AtomicPtr<u8>,  // MMIO 基地址
}
```

**优点**：不需要修改外部依赖，完全自主控制
**缺点**：绕过 HAL 层，硬件耦合，移植性差

### 6.3 路径 C：混合方案（推荐）

在 axhal 中新增一个独立于 console 的 UART 操作模块：
```rust
// axhal::uart_async
pub mod uart_async {
    pub fn try_read(base: usize, buf: &mut [u8]) -> usize;
    pub fn try_write(base: usize, buf: &[u8]) -> usize;
    pub fn enable_rx_intr(base: usize);
    pub fn disable_rx_intr(base: usize);
    pub fn enable_tx_intr(base: usize);
    pub fn disable_tx_intr(base: usize);
    pub fn read_iir(base: usize) -> u8;
    pub fn irq_num() -> u32;
}
```

**优点**：保持 HAL 抽象，不影响现有 console；新增模块不影响已有接口
**缺点**：需要修改 axhal crate

---

## 7. 存疑问题

| 编号 | 问题 | 影响 | 需要确认对象 |
|------|------|------|-------------|
| Q1 | QEMU virt 平台是否支持第二个 16550 UART？ | 决定是否需要独立硬件还是复用同一 UART | QEMU 文档/实验 |
| Q2 | 修改 axplat/axhal crate 的方式？crates.io 上的包是直接 fork 还是提 PR？ | 决定硬件抽象层扩展路径 | 项目维护者/老师 |
| Q3 | 上板子时的 UART 型号？是否仍是 16550 兼容？ | AsyncUart trait 设计范围 | 老师 |