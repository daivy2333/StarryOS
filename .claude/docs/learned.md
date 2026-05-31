# learned.md — 项目学习记忆（汇总）

> 由 project-docs-assistant 维护，汇总分支整合。
> 条目格式: <!-- L{编号} --> 标记开头，支持 grep 精确定位。
> 汇总两个方向（渐进式集成 + 完全剔除 Console）的全部经验。

---

## API 路径

<!-- L1 --> | axtask::future::block_on | 异步任务阻塞执行 | 2026-05-24 |
<!-- L2 --> | axtask::future::poll_io | WouldBlock → register → await 标准模式 | 2026-05-24 |
<!-- L3 --> | axtask::future::register_irq_waker | 连接中断到异步任务唤醒 | 2026-05-24 |
<!-- L4 --> | embassy_sync::AtomicWaker::wake | ISR 中安全唤醒 Waker，无锁中断安全 | 2026-05-24 |
<!-- L63 --> | register_irq_waker 共存机制 | BTreeMap<usize, PollSet> 支持同一 IRQ 注册多个 waker | 2026-05-27 |
<!-- L65 --> | RISC-V musl 工具链路径 | /opt/musl/riscv64-linux-musl-cross/bin | 编译 lwext4_rust C 代码 | 2026-05-27 |
<!-- L66 --> | rootfs 下载地址 | https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz | 1GB 磁盘镜像 | 2026-05-27 |
<!-- L67 --> | disk.img 位置 | 项目根目录 + make/disk.img | make run 需要后者 | 2026-05-27 |
<!-- L81 --> | uart_16550 寄存器定义 | uart_16550/src/spec.rs | IER/ISR/LSR bitflags + InterruptType 枚举 | 2026-05-28 |
<!-- L82 --> | uart_16550 MMIO 实现 | uart_16550/src/backend/mmio.rs | read_volatile/write_volatile + 地址计算 | 2026-05-28 |
<!-- L94 --> | uart_16550 init API | uart_16550/src/lib.rs:406-523 | SerialPort::new_mmio + Config + init，完整初始化流程 | 2026-05-28 |
<!-- L95 --> | uart_16550 Config 字段 | uart_16550/src/config.rs:114-154 | baud_rate/data_bits/interrupts/fifo_trigger_level | 2026-05-28 |
<!-- L96 --> | UART 中断类型枚举 | uart_16550/src/spec.rs:315-414 | InterruptType（ReceivedDataReady/THR_EMPTY/ReceptionTimeout/LineStatus） | 2026-05-28 |
<!-- L97 --> | register_irq_waker 实现 | axtask/src/future/poll.rs:43-66 | BTreeMap<usize, PollSet>，支持同一 IRQ 注册多个 waker | 2026-05-28 |
<!-- L98 --> | register_irq_hook 全局唯一 | axhal/src/irq.rs:12-28 | AtomicUsize compare_exchange，只能注册一次 | 2026-05-28 |
<!-- L99 --> | AtomicWaker ISR 安全 | embassy-sync/src/waitqueue/atomic_waker.rs:42-63 | CriticalSectionRawMutex + wake_by_ref，ISR 中无阻塞 | 2026-05-28 |
<!-- L100 --> | PollSet 容量上限 | axpoll/src/lib.rs:66-150 | 容量 64，超过唤醒旧 waker（环形缓冲） | 2026-05-28 |
<!-- L101 --> | DeviceOps trait 核心方法 | kernel/src/pseudofs/device.rs:28-55 | read_at/write_at/ioctl/as_pollable/flags | 2026-05-28 |
<!-- L102 --> | Pollable trait 定义 | axpoll/src/lib.rs | poll() + register()，支持 poll/select/epoll | 2026-05-28 |
<!-- L103 --> | IoEvents 标志 | axpoll/src/lib.rs | IN/OUT/HUP/ERR/RDNORM/WRNORM | 2026-05-28 |
<!-- L104 --> | axlog LogIf trait | axlog-0.3.0-preview.2/src/lib.rs | console_write_str() → axhal::console::write_bytes() | 2026-05-28 |
<!-- L105 --> | panic handler 实现 | axruntime-0.3.0-preview.2/src/lang_items.rs | panic() → ax_println! → polling TX | 2026-05-28 |
<!-- L106 --> | uart_16550 retry_until_ok macro | uart_16550-0.4.0/src/lib.rs | loop { if let Ok(ok) = $cond { break ok; } } | 2026-05-28 |
<!-- L116 --> | axhal::mem::phys_to_virt | 物理地址到虚拟地址转换（返回 VirtAddr） | 2026-05-28 |

---

## 文件速查

<!-- L5 --> | Pipe 异步管道 | kernel/src/file/pipe.rs | poll_io + register_irq_waker 模式参考 | 2026-05-24 |
<!-- L6 --> | EventFd | kernel/src/file/event.rs | 轻量异步通知模式参考 | 2026-05-24 |
<!-- L7 --> | DeviceOps 设备注册 | kernel/src/pseudofs/device.rs | DeviceOps trait + Device 包装 | 2026-05-24 |
<!-- L8 --> | UART 硬件操作 | axhal/src/platform/riscv64_qemu_virt/uart.rs | MMIO 寄存器定义 | 2026-05-24 |
<!-- L9 --> | PLIC 中断映射 | axhal/src/platform/riscv64_qemu_virt/mod.rs | PLIC 中断号 | 2026-05-24 |
<!-- L83 --> | Console 驱动 | kernel/src/pseudofs/dev/tty/ntty.rs | Console struct + TtyRead/TtyWrite trait | 2026-05-28 |
<!-- L84 --> | tty-reader copier | kernel/src/pseudofs/dev/tty/terminal/ldisc.rs | InputReader + poll_fn + register_irq_waker | 2026-05-28 |
<!-- L85 --> | ConsoleDriver | kernel/src/drivers/serial/console_driver.rs | RX copier + TX sync flush + AsyncBuffer | 2026-05-28 |
<!-- L90 --> | Console 设备注册 | kernel/src/pseudofs/dev/mod.rs:222-230 | /dev/console → DeviceId(5, 1) → N_TTY | 2026-05-28 |
<!-- L91 --> | Console 初始化流程 | kernel/src/pseudofs/dev/tty/ntty.rs:31-44 | new_n_tty() → bind_to(&proc) → session terminal | 2026-05-28 |
<!-- L92 --> | tty-reader 任务名 | kernel/src/pseudofs/dev/tty/terminal/ldisc.rs:276 | spawn_with_name("tty-reader") | 2026-05-28 |
<!-- L93 --> | PTY 与 Console 分离 | kernel/src/pseudofs/dev/tty/pty.rs | PTY 不依赖 Console 硬件，可保留 | 2026-05-28 |

---

## 踩坑档案

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

<!-- L68 --> ### 构建环境配置踩坑
- 症状: make build 失败，`riscv64-linux-musl-cc: command not found`
- 根因: lwext4_rust crate 需要编译 C 代码，依赖 musl 交叉编译工具链
- 解:
  1. 工具链位于 `/opt/musl/riscv64-linux-musl-cross/bin`
  2. 构建前设置 `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH`
  3. 系统已有其他 RISC-V 工具链，但 musl 版本是必需的
- 验证: 2026-05-27 (T0.4)

<!-- L69 --> ### rootfs 下载与部署踩坑
- 症状: make rootfs 下载失败（SSL 连接中断），disk.img not found
- 根因: GitHub releases 下载不稳定 + Makefile 需要 disk.img 在 make/ 目录
- 解:
  1. 手动下载 `https://github.com/Starry-OS/rootfs/releases/download/20260214/rootfs-riscv64.img.xz`
  2. 解压 `xz -d rootfs-riscv64.img.xz`
  3. 复制到两处：`cp rootfs-riscv64.img disk.img && cp disk.img make/disk.img`
- 验证: 2026-05-27 (T0.4)

<!-- L70 --> ### 构建警告清理经验
- 症状: 编译有10个 unused warnings（dead_code）
- 分析: 这些是项目原有代码的未使用函数，不是我们添加依赖导致
- 影响: 不影响功能，编译成功
- 建议: 不清理（遵循"只改必须改的代码"原则）

<!-- L78 --> ### M3 替换失败 — IRQ 风暴 + TX busy-loop（方向 A 失败经验）
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
  3. ❌ 战略转向过于激进（未充分验证可行性）
- **解决**: 回滚到 M3 Task 5，重新评估整体方案
- **验证**: 2026-05-28（ADR-019）

<!-- L79 --> ### UART 状态调试缺失教训
- **问题**: M3 替换失败时，缺少全面的 UART 硬件状态调试
- **缺失信息**:
  1. IIR 寄存器（Interrupt Identification）— 无法确认 interrupt 类型
  2. MCR 寄存器（Modem Control）— 无法确认 TX 是否被禁用
  3. LSR 完整值（仅输出 THR_EMPTY/TEMT，未输出错误标志）
- **后果**: 无法诊断 UART 硬件为什么卡住，只能猜测根因
- **教训**: 硬件集成前，必须添加全面的寄存器状态调试（IIR/MCR/LSR/IIR）
- **预防**: 下次集成前，先添加 UART 状态诊断代码

<!-- L80 --> ### THR_EMPTY 状态理解错误
- **问题**: uart_16550 crate 的 THR_EMPTY 注释说"FIFO completely empty"
- **实际**: THR_EMPTY (Bit 5) 表示 THR 有空位（可以写入），TEMT (Bit 6) 表示完全空闲
- **误解影响**: 以为 THR_EMPTY=false 表示 FIFO 有至少 1 个字节，实际表示 FIFO 满
- **纠正**: THR_EMPTY=1 表示 FIFO 有空位，THR_EMPTY=0 表示 FIFO 满
- **教训**: 需仔细阅读 UART 规范，不要依赖库的注释（可能有错误）

<!-- L88 --> ### Console 与 AsyncUart 共享 UART 的数据竞争风险
- **风险类型**:
  1. **TX 数据竞争**: Console TX（同步阻塞）与 AsyncUart TX copier 同时写 THR 寄存器
  2. **IRQ waker 冲突**: Console tty-reader 与 AsyncUart copier 竞争 IRQ 10 waker
  3. **UART 重初始化冲突**: AsyncUart 重初始化可能破坏 Console 配置
- **根因**: Console 是外部 crate (axplat)，无法完全控制其 UART 操作
- **解决**: 完全剔除 Console，使用本地 uart_16550 crate（方向 B 策略）

<!-- L89 --> ### 分支策略变更：完全剔除 Console（方向 B 策略）
- **背景**: feat/uart-async 分支的渐进式集成方案失败（M3 替换失败）
- **决策**: 创建 feat/uart-async-dev2 分支，完全剔除 Console，从零开始
- **原因**: 避免 Console 与 AsyncUart 的数据竞争、IRQ waker 冲突、UART 重初始化冲突
- **新方案**: 使用本地 uart_16550 crate + 自实现 UART 初始化

<!-- L107 --> ### ISR 分发机制设计要点
- **ISR 中读 ISR 寄存器**：判断 InterruptType（ReceivedDataReady/THR_EMPTY/ReceptionTimeout）
- **禁用中断防止重入**：ISR 中临时禁用 RX/TX 中断（IER 操作）
- **AtomicWaker 精确唤醒**：rx_waker/tx_waker 分别唤醒
- **ISR 执行原则**：最小工作（读 ISR + 禁用中断 + 唤醒 waker）
- **ISR 安全约束**：无阻塞、无锁、MMIO read/write 安全

<!-- L108 --> ### UART 初始化配置差异
- **Console 配置**：IER::DATA_READY（只使能 RX 中断）
- **AsyncUart 配置**：IER::DATA_READY | IER::THR_EMPTY（RX + TX 中断）
- **关键差异**：Console 禁用 TX 中断，AsyncUart 必须使能 TX 中断
- **解决方案**：UART 重新 init 时使能 TX 中断（覆盖 Console 配置）

<!-- L109 --> ### earlycon 启动时机分析
- **earlycon 可用时间点**: axruntime::rust_main 中 axplat::init::init_early() 后（T0-T2）
- **axlog::init()**: 第 160 行，启用内核日志框架
- **ax_println!**: 启动 LOGO 输出（约 17 ms polling TX）
- **AsyncUart 启动时间点**: kernel::entry::init() 中（T4-T5）
- **时间差**: earlycon 比 AsyncUart 早约 10-20 ms

<!-- L110 --> ### MMIO 权限问题根因分析（方向 B 关键发现）
- **问题**: 内核上下文和 ISR 上下文都无法访问 UART MMIO 寄存器
- **现象**:
  - 物理地址 0x1000001c → Page Fault（未映射）
  - 虚拟地址 0xffffffc01000001c → StoreFault（无写入权限）
  - 虚拟地址 0xffffffc010000008 → LoadFault（无读取权限）
- **根因**: axplat 在 boot 阶段映射 UART MMIO，权限被限制（只读或禁止）
- **影响**: 无法在内核/ISR 上下文中访问 UART 寄存器，无法验证/修改 UART 配置
- **结论**: 不彻底更改底层支持（axplat）就无法使用异步串口
- **验证**: 2026-05-29 (ADR-023)

<!-- L111 --> ### axplat 外部 crate 依赖全景图
- **依赖链**: axruntime → axplat-riscv64-qemu-virt → axhal → axtask → axpoll
- **不可修改**: 所有 crate 均来自 crates.io，无法直接修改
- **UART 相关**: axplat 负责 UART MMIO 映射和初始化，axhal 提供 console API
- **关键约束**: UART MMIO 权限由 axplat 控制，kernel 层无法绕过

<!-- L112 --> ### 绕过 axplat 的可能路径
- **方案 1**: 修改 axplat 源码（fork 或 PR）— 需上游协调
- **方案 2**: 在 boot 阶段修改页表权限 — 需深入理解 RISC-V 页表机制
- **方案 3**: 使用 QEMU 命令行参数修改 MMIO 映射 — 可能不可行
- **方案 4**: 完全自实现 UART 驱动（不依赖 axplat）— 工作量大
- **评估**: 方案 1 最合理，但需评估上游接受度

<!-- L117 --> ### axplat UART 初始化流程
- **初始化时机**: axplat::init::init_early() 中
- **初始化内容**: MMIO 映射 + 波特率配置 + FIFO 使能 + IER 配置
- **IER 配置**: 只使能 DATA_READY（RX 中断），不使能 THR_EMPTY（TX 中断）
- **权限设置**: MMIO 映射权限受限（可能是只读或禁止用户态访问）
- **影响**: AsyncUart 需要 TX 中断，但无法修改 IER 配置

---

<!-- L118 --> ### Console MMIO 权限分析纠正（关键发现）
- **背景**: 此前分析文档认为"Console 因在页表切换前初始化而能访问 UART MMIO，新代码因在页表切换后无法访问"，进而得出"MMIO 权限阻塞，必须修改 axplat"的结论。
- **纠正**: 此分析有误。Console 能访问 UART MMIO 的真正原因是：
  1. UART MMIO 地址 `0x10000000` 明确列在 `axconfig.toml → [devices].mmio-ranges` 中
  2. `mmio_ranges()` → `axhal::mem::memory_regions()` → `new_kernel_aspace().map_linear()` 将 UART 以 `READ | WRITE | DEVICE` 标志映射进最终内核页表
  3. Console 的静态 `MmioSerialPort` 访问 `0xffffffc010000000` 命中有效映射，**与初始化时机无关**
- **结论**: 页表权限不是阻塞原因。若测试代码在同一虚拟地址上 LoadFault，根因可能是地址计算错误、stride 不匹配、PMP 配置等，**而非页表权限**。
- **验证路径**: `axplat-riscv64-qemu-virt/axconfig.toml` → `src/mem.rs mmio_ranges()` → `axhal/src/mem.rs ALL_MEM_REGIONS` → `axmm/src/lib.rs new_kernel_aspace()` → `KERNEL_ASPACE`
- **影响**: 移除了"必须修改 axplat"的前提条件，异步串口实现可在 kernel 层独立解决

<!-- L119 --> ### axmm::iomap() 现成 API（关键技术发现）
- **发现**: `axmm` crate 已提供 `iomap()` 函数，专门用于将设备 MMIO 映射到内核页表
- **函数签名**: `pub fn iomap(addr: PhysAddr, size: usize) -> AxResult<VirtAddr>`
- **内部实现**: `kernel_aspace().lock().map_linear()` + `protect()`，使用 `DEVICE | READ | WRITE` 标志
- **调用方式**: `axmm::iomap(PhysAddr::from(0x10000000), 0x1000)`
- **关键特性**:
  - 如果映射已存在，静默跳过 `map_linear()` 后仍调用 `protect()` 确保权限正确
  - 自动 flush TLB（cursor drop 时）
  - 不修改任何外部 crate
- **验证**: axmm-0.3.0-preview.2/src/lib.rs:111-131
- **影响**: 方案 D 无需开发新 API，直接调用 `iomap()` 即可解除 MMIO 访问阻塞

## 技巧模式

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

<!-- L71 --> ### poll_io 标准模式
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

<!-- L72 --> ### AtomicWaker 使用模式
```rust
static WAKER: AtomicWaker = AtomicWaker::new();
// 任务上下文注册
WAKER.register(cx.waker());
// ISR 中唤醒
WAKER.wake();
```

<!-- L73 --> ### 设备注册到 devfs 模式
```rust
// 在 pseudofs/dev/mod.rs builder() 中
builder.add_device(
    "async_uart_test",
    DeviceId::new(4, 64),
    Arc::new(Device::new(async_uart_test_device)),
);
```

<!-- L74 --> ### UART 状态诊断模式
```rust
// 集成前必须诊断
let ier = uart.interrupt_enable();
let iir = uart.interrupt_identification();
let lsr = uart.line_status();
let mcr = uart.modem_control();
log::info!("UART State: IER={:#x} IIR={:#x} LSR={:#x} MCR={:#x}", ier, iir, lsr, mcr);
```

<!-- L75 --> ### 内核内部测试模式
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

<!-- L76 --> ### 条件编译开关模式
```rust
#[cfg(feature = "async_uart")]
pub fn init() {
    // AsyncUart 初始化
}
#[cfg(not(feature = "async_uart"))]
pub fn init() {
    // Console 初始化（默认）
}
```

<!-- L77 --> ### UART 重初始化安全模式
```rust
// 1. 读取当前配置
let current_ier = uart.interrupt_enable();
// 2. 只修改需要的位
uart.set_interrupt_enable(current_ier | IER::THR_EMPTY);
// 3. 验证修改结果
let new_ier = uart.interrupt_enable();
assert!(new_ier.contains(IER::THR_EMPTY));
```

<!-- L120 --> ### iomap 设备 MMIO 映射模式
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
- 原理：锁定 `kernel_aspace()` 全局内核页表，调用 `map_linear()` + `protect()` 保证 `DEVICE | READ | WRITE` 权限
- 优势：不修改任何外部 crate，已有稳定 API
