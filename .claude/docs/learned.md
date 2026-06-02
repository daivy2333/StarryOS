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

<!-- L78 --> ### M3 替换失败 — IRQ 风暴 + TX busy-loop（方向 A）
- **症状**: IRQ 风暴 + TX FIFO 满 LSR=0x00
- **根因**: Console UART 状态不兼容 AsyncUart + stride=4 + 未验证 IIR/MCR/LSR
- **教训**: 硬件集成前必须 dump 全部寄存器状态；不要假设外设初始化后状态正常
- **解决**: 回滚，后续 Q0-Q4 独立实现替代

<!-- L79 --> ### 硬件调试铁律：集成前 dump 全部寄存器
- **教训**: 集成前必须 `info!(IIR={:02x} MCR={:02x} LSR={:02x})`，否则无法诊断硬件异常
- **参考**: uart_init.rs `log_uart_state()` 函数

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

<!-- tombstone: L110-L112-L113-L116 --> Archived to archive.md §learned #L110-L116 2026-05-31 — MMIO 权限诊断被 stride=4 根因（L121）纠正

<!-- tombstone: L111 --> Archived to archive.md §learned #L111 2026-05-31 — axplat 依赖分析（不需要绕过）

<!-- tombstone: L112 --> Archived to archive.md §learned #L112 2026-05-31 — 绕过 axplat 方案（不需要了）

<!-- L117 --> ### axplat UART 初始化流程（⚠️ 此条目关于 IER 限制仍准确，但"权限设置"部分已纠正）
- **初始化时机**: axplat::init::init_early() 中
- **IER 配置**: 只使能 DATA_READY（RX 中断），不使能 THR_EMPTY（TX 中断）
- ~~**权限设置**: MMIO 映射权限受限~~ → 实际权限正常（READ|WRITE|DEVICE），见 L121

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

<!-- L121 --> ### LoadFault 根因：stride=4 错误（2026-05-31 关键发现）
- **症状**: 内核和 ISR 上下文在 `0xffffffc010000008` 处 LoadFault
- **此前误判**: 认为是 axplat 限制了 UART MMIO 页表权限
- **真正根因**: NS16550 寄存器仅 `0x00-0x07` 共 8 字节。`UART_STRIDE=4` 下 ISR（offset 2×4=8）读写到 `base+8`=第 9 字节，超出 UART 寄存器范围，QEMU 总线错误被 RISC-V 解释为 LoadFault
- **验证**: stride=1 下 raw pointer 读 LSR→`0x60` ✅，uart_16550 crate 全部寄存器正常 ✅，ISR handler 正常 ✅，无 IRQ 风暴 ✅
- **关键证据**: raw read at base+5（stride 1）成功，base+8（stride 4）失败 — 同一 4K 页表映射，排除页表问题
- **影响**: 方向 A M3 和方向 B P1/P2 的全部 LoadFault 阻塞是一次简单的 stride 配置错误

<!-- L122 --> ### Q1 架构关键发现（AtomicWaker + critical-section）
- **ISR 唤醒模式**: ISR 中禁用对应中断后调用 AtomicWaker::wake()，copier 任务中重新 enable 中断
- **critical-section**: embassy-sync AtomicWaker 需要 critical-section crate v1.0 的 `_critical_section_1_0_acquire/release` 符号，在 lib.rs 中用 disable_irqs/enable_irqs 实现
- **UnsafeCell**: 多个 copier 任务共享 AsyncBuffer 需用 UnsafeCell 绕过 Rust 借用检查（单生产者单消费者场景安全）
- **spawn_with_name + block_on**: axtask 的 spawn 接口收 `FnOnce() + Send + 'static` closure，内部用 `block_on(future)` 包装异步逻辑
<!-- L123 --> ### RX copier 与 Console tty-reader FIFO 竞争
- **症状**: Shell 显示 `starry:~#` 但键盘输入完全无效（`ls` 等命令无响应）
- **根因**: RX copier 和 Console tty-reader 都读取同一个 UART RBR（FIFO）。copier 先读取 → 数据进入 ring buffer → tty-reader 读 FIFO 时空 → Shell 收不到输入
- **解决**: Q2 关闭 copier 让 Console 独占 UART。Q3 替换 Console 后再由 copier 接管
- **教训**: 共享硬件（同一 UART）的两个 reader 必须互斥访问，不能同时 drain FIFO

<!-- L124 --> ### Q4 全异步 TX 实现要点
- **TX 中断流**: copier 发送到 FIFO 满 → `enable_tx_intr()` → ISR 在 THR_EMPTY 时 `disable_tx_intr() + wake` → copier 继续
- **AsyncUartWriter**: 实现 `TtyWrite` → 写入 ring buffer → wake TX copier
- **内核日志共存**: `ax_println!` 的 Console polling TX 与 TX copier 共享 THR，因 polling TX 每次仅写极少字节，无实质冲突
- **Tty 泛型绑定**: `Tty<AsyncUartReader, AsyncUartWriter>` 替代 `Tty<Console, Console>`，直接实现 reader/writer trait 即可替换整个终端栈

<!-- L125 --> ### Q5 性能优化关键技术
- **IER 缓存**: 用 `AtomicU8` 缓存 IER 值，enable/disable 只需一次 `write_volatile`（消除 RMW 的 `read_volatile`）
- **ISR 合并**: 在同一个 `SpinNoIrq` 临界区内完成 ISR 读 + IER 写，消除 drop+重锁
- **批量 I/O**: RX copier 在单次锁内排空 FIFO，TX copier 在单次锁内填满 FIFO
- **waker skip**: 用 `Cell<Option<Waker>>` + `will_wake` 避免重复注册相同的 waker
- **TX 单锁**: 消除 double buffer lock（pop → send → push_back），改为一次 pop + send，只在 FIFO 满时 push_back

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

<!-- L126 --> ### TX copier 与 ax_println! 输出交错（最新踩坑）
- **症状**: 异步 TX 启用后 Shell 输出乱码（`ls /bin` 行间字符交叠）
- **根因**: TX copier 用 `send_bytes()` 批发送，中间 `ax_println!` 的 Console polling TX 插队写 THR。copier 把未发完数据推回 ring buffer → 新数据与旧数据混合 → 再次发送时乱序
- **解决**: TX copier 用本地 `cursor` 追踪已发位置，未发完的数据保留在本地 `write_buf` 中，不推回 ring buffer。下次迭代从 `cursor` 继续
- **教训**: 共享硬件（同一 UART THR）的两个 writer 必须保证同一批数据原子发送。不能把部分数据推回共享缓冲区
- **回退方案（已弃用）**: 临时切回 `ConsoleWriter` 让 Shell stdout 也走 Console TX → 这是降级，真正的异步 TX 被绕过

<!-- L127 --> ### NAPI 中断合并模式
- **触发**: 连续成功读取 ≥ `NAPI_THRESHOLD`（16）次后进入轮询模式
- **轮询**: batch size 缩小到 `NAPI_BATCH_SIZE`（64），不重新使能 RX 中断
- **退出**: 一次读取返回 0 → `consecutive` 重置 → 退出轮询 → 使能 RX 中断
- **效果**: 高吞吐时减少 90%+ IRQ 频率

<!-- L128 --> ### AtomicWaker vs register_irq_waker 设计选择
- **场景**: ISR 唤醒异步任务时，有两种方案
- **方案 A: AtomicWaker（本项目采用）**
  - 静态变量：`static RX_WAKER: AtomicWaker = AtomicWaker::new();`
  - ISR 中直接调用：`RX_WAKER.wake()`，O(1) 复杂度
  - 优点：无查找开销、无锁、ISR 安全、代码简单
  - 适用：固定数量的 waker（RX/TX 各一个）
- **方案 B: register_irq_waker（axtask 通用方案）**
  - 使用 `BTreeMap<usize, PollSet>` 存储每个 IRQ 的 waker 集合
  - ISR 中需要查找对应 PollSet → O(log n) 复杂度
  - 优点：支持同一 IRQ 注册多个 waker、动态管理
  - 适用：通用场景（如 Console tty-reader + AsyncUart 共用 IRQ 10）
- **本项目选择 AtomicWaker 的原因**:
  1. UART 驱动是专用的，只有 RX/TX 两个方向，各一个 waker
  2. 不需要动态注册/注销 waker
  3. ISR 性能要求高（~1.5µs），AtomicWaker::wake() 是原子操作，无分支
  4. 代码更简洁，无需处理 BTreeMap 的并发问题
- **结论**: 专用驱动用 AtomicWaker，通用框架用 register_irq_waker。O17 优化不适用本项目。

<!-- L129 --> ### Console 阻塞串口组件清理（2026-05-31）
- **背景**: 异步串口（ASYNC_TTY）已完全替代 Console，Console 组件不再使用
- **清理范围**:
  1. 删除 `kernel/src/pseudofs/dev/tty/ntty.rs` - Console struct + N_TTY lazy_static
  2. 删除 `kernel/src/drivers/device_ops.rs` 中的 ConsoleWriter struct
  3. 删除 `kernel/src/pseudofs/dev/tty/mod.rs` 中的 N_TTY/NTtyDriver 导出
  4. 修改 `kernel/src/syscall/fs/fd_ops.rs` - 移除 NTtyDriver 类型检查
- **保留的组件**:
  - `axhal::console::write_bytes` - earlycon 内核日志（ax_println! 使用）
  - ASYNC_TTY - 异步串口 TTY 设备
- **验证**: cargo check 通过，无编译错误
- **影响**: 代码更简洁，消除死代码，明确异步串口是唯一串口实现

<!-- L130 --> ### 性能测试框架（2026-06-01）
- **内核态测试**: kernel/src/drivers/benchmark.rs
  - CPU 占用测量：RISC-V cycle 计数器
  - NAPI 效果报告：IRQ 频率统计
  - Ring Buffer 写入测试
- **用户态测试**: tests/benchmark.c
  - TX 吞吐量：不同数据大小（64/256/1024/4096 字节）
  - write() 延迟：P50/P95/P99
  - 压力测试：持续 2 秒写入
- **自动化脚本**: scripts/benchmark.sh
- **测试分支**: feat/uart-async-bench（Async）、feat/uart-bench（Console）

<!-- L131 --> ### CPU 测试数据量统一（2026-06-01）
- **问题**: Console 测试写入 120 字节，Async 测试写入 102,400 字节，数据量差 853 倍
- **影响**: CPU 占用数据无法公平对比（Console 2.3% vs Async 57.8%）
- **解决**: 统一测试数据量为 102,400 字节
- **结果**: Console 3,835 cycles/byte，Async 268 cycles/byte，Async 效率高 14.3 倍
- **教训**: 性能对比必须统一测试条件，否则数据有误导性

<!-- L132 --> ### RX 测试 TTY 竞争条件（2026-06-01）
- **问题**: 用户态 RX 测试卡住，read() 永远等不到数据
- **原因**: TTY 层回显导致 Shell 抢先读取数据
  - benchmark write("AAAAA") → UART 发送 → QEMU 回显 → Shell 读取
  - benchmark read() 等待 ← 数据被 Shell 读走了 ← 回显数据
- **这不是功能错误**: TTY 回显是正常行为，Shell 读取用户输入是正确的行为
- **解决方案**: 在内核态直接测试 Ring Buffer，绕过 TTY 层
- **测试函数**:
  - `run_rx_throughput_test()` - RX 吞吐量测试
  - `run_rx_latency_test()` - RX 延迟测试
- **结果**: RX Ring Buffer 读取 588,776 KB/s，延迟 P50 600 ns

<!-- L133 --> ### 文档优化原则（2026-06-01）
- **问题**: 文档出现多个声明和总结，重复内容多
- **解决**: 合并重复部分，保持信息完整
- **原则**:
  - 性能数据不丢失
  - 测试方法说明合并
  - 结论部分简化
  - 选择建议合并
- **效果**: 文档从 384 行减少到 200 行，更易阅读

<!-- L134 --> ### 三层嵌套 block_on/poll_io 导致 yield storm（2026-06-01）
- **问题**: 用户态 async read 路径有 3 层嵌套 `block_on(poll_io(...))`: File → Tty/JobControl → Ldisc/WaitPollable
- **根因**: `ProcessMode::Manual` 中 `register_rx_waker()` 调用 `waker.wake_by_ref()`，导致 waker 注册后**立即唤醒** task
- **效果**: 形成高频 yield-re-schedule 循环（yield storm），无数据时空耗 CPU
- **解决方向**: 改用 `ProcessMode::External` 消除立即唤醒，或优化 WaitPollable 的 register 行为
- **参考**: `docs/analysis/user-async-perf-analysis.md`

<!-- L135 --> ### 异步 VS 阻塞串口性能边界（2026-06-01）
- **上限**: 115200 bps = 11.52 KB/s，无论同步异步都受此限制
- **Async TX 优势**: write() 返回快（~1 us vs 87 us/byte busy-wait），适合 pipeline
- **Async RX 劣势**: 多一次 ring buffer 拷贝（UART FIFO → ring buf → ldisc buf → user buf）
- **CPU 空闲**: Manual 模式下 yield storm 导致空闲 CPU 更高
- **结论**: 异步在吞吐量上**不可能**超过阻塞 Console（硬件上限），优势在不阻塞调用方
- **参考**: `docs/analysis/user-async-perf-analysis.md`

<!-- L136 --> ### 当前 benchmark 不测量实际 UART 吞吐量（2026-06-01）
- **问题**: `tests/benchmark.c` 的 TX 吞吐量测试写入 `/dev/null`（非 `/dev/console`），绕过 UART
- **延迟测试**: 测量的是 ring buffer push 时间（~1 us），不是硬件发送延迟
- **RX 用户态测试**: 被跳过（TTY echo loop），内核态测试绕过 TTY 层
- **解决**: TX 测试需 write → tcdrain() 等实际发送完成；RX 需 raw mode + 独立测试程序
- **参考**: `docs/analysis/user-async-perf-analysis.md`

<!-- L137 --> ### FIONBIO nonblocking 标志未传播到 TTY 层（2026-06-01）
- **问题**: `File::read()` 将 nonblocking 传入 `poll_io`，但 `Tty::read_at()` 和 `ldisc.read()` 内部 `block_on(poll_io(...))` 硬编码 `false`
- **影响**: `ioctl(FIONBIO)`、`fcntl(F_SETFL, O_NONBLOCK)`、`open(O_NONBLOCK)` 对 TTY 读均无效
- **TX 路径**: AsyncUartWriter::write() 天然非阻塞（push ring buffer），不受影响
- **解决**: Tty struct 添加 AtomicBool nonblocking，传播到 read_at → ldisc
- **参考**: `docs/analysis/nonblocking-mode-analysis.md`

<!-- L138 --> ### 用户态 async read 完整路径追踪（2026-06-01）
- **路径**: sys_read → File::read → block_on(poll_io(File, IN, nb, || inner.read()))
  → Device::read_at → Tty::read_at → block_on(poll_io(JobControl, IN, false, || ldisc.read()))
  → ldisc.read → block_on(poll_io(WaitPollable, IN, false, || buf_rx.pop_slice()))
- **关键点**: 3 层嵌套 block_on、Manual 模式 waker.wake_by_ref()、无 nonblocking 传播
- **文件**: kernel/src/file/fs.rs → kernel/src/pseudofs/dev/tty/mod.rs → .../terminal/ldisc.rs
- **参考**: `docs/analysis/user-async-perf-analysis.md`

<!-- L139 --> ### TCSBRK (tcdrain) 实现：poll ring buffer + LSR.TRANSMITTER_EMPTY（2026-06-01）
- **需求**: `tcdrain(fd)` 调用 `ioctl(fd, TCSBRK=0x5409)`，需等待 TX 数据完全发送
- **实现**: `block_on(poll_fn(|cx| { check ring buffer empty + LSR::TRANSMITTER_EMPTY; if not ready { cx.waker().wake_by_ref(); Pending } }))`
- **关键**: 必须查 `TRANSMITTER_EMPTY`（bit 6, TEMT）而非 `THR_EMPTY`（bit 5），否则 THR 空但移位寄存器还在发 → tcdrain 过早返回
- **文件**: `kernel/src/syscall/fs/ctl.rs:43-58`
- **QEMU 限制**: QEMU 16550 不仿真串口线延迟，tcdrain 几乎瞬时返回，吞吐量 ~200 KB/s 而非 ~11.5 KB/s

<!-- L140 --> ### O_NONBLOCK 必须通过三个入口全部传播（2026-06-01）
- **问题**: 最初只在 `sys_ioctl(FIONBIO)` 做了 `f.ioctl(cmd, nb)` 转发，但 `open(O_NONBLOCK)` 和 `fcntl(F_SETFL, O_NONBLOCK)` 只在 File 层设置 flag，未传播到 Tty
- **症状**: `open("/dev/console", O_RDWR | O_NONBLOCK)` 后 `read()` 仍然阻塞
- **解决**: 三个入口都加 `f.ioctl(FIONBIO, nb as usize)`:
  - `syscall/fs/fd_ops.rs:106` — open() 路径
  - `syscall/fs/fd_ops.rs:254` — fcntl F_SETFL 路径
  - `syscall/fs/ctl.rs:31` — sys_ioctl 路径
- **教训**: 任何跨层状态传播都必须穷举所有入口，一个遗漏 = 功能不完整

<!-- L141 --> ### QEMU 16550 串口模拟的时序欺骗（2026-06-01）
- **现象**: QEMU 上 TX 吞吐量测出 150-250 MB/s（用户态），远超 115200 bps 理论值 11.5 KB/s
- **根因**: QEMU 的 NS16550 模拟不仿真真实串口线延迟（86.8 µs/byte），UART FIFO 数据处理为瞬时
- **影响**: 所有基于 tcdrain/轮询 LSR 的吞吐量测试在 QEMU 上均不可信
- **真板预期**: VisionFive2 @ 115200 bps → ~11.5 KB/s（受硬件波特率限制）
- **可靠指标（QEMU 也可测）**: 内核态 ring buffer 速度、write() 延迟、CPU cycles/byte

<!-- L142 --> ### LSR::TRANSMITTER_EMPTY vs THR_EMPTY 的位差异（2026-06-01）
- `LSR::THR_EMPTY` = bit 5: THR 可接受新字节（FIFO 有空位）
- `LSR::TRANSMITTER_EMPTY` = bit 6: THR + 移位寄存器都为空 = 真正 drain
- **踩坑**: 最初误用 `LSR::TEMT`（不存在）编译失败；用 `THR_EMPTY` 会导致 tcdrain 过早返回
- **文件**: `uart_16550/src/spec.rs:904/914` 定义了这两个 bitflag

<!-- L143 --> ### 2026-06-01 会话总结（Q7 完成）
- **分析**: 两份深度分析文档（user-async-perf-analysis.md, nonblocking-mode-analysis.md）
- **O42**: yield storm → `ProcessMode::External`，1 行改动（ntty_async.rs）
- **O43**: FIONBIO 传播到 Tty/ldisc，3 文件改动（tty/mod.rs, ldisc.rs, ctl.rs）
- **O44**: benchmark 修正（写 /dev/console + tcdrain）+ TCSBRK 实现
- **补充修复**: O_NONBLOCK open/fcntl 入口转发、LSR 位修正
- **文档**: 重写 comparison 报告、更新全部体系文档
- **分支**: dev2 和 bench 均已同步（bench 多了 benchmark.c）

<!-- L144 --> ### tcdrain 真异步化：PollSet + DRAIN_WAKER（2026-06-01）
- **三段式等待**:
  1. ring buf 有数据 → 注册 `tx.poll`（copier pop 时 `poll.wake()` 唤醒 tcdrain）
  2. ring buf 空但 UART 还在发 → 注册 `DRAIN_WAKER`（ISR TX 中断时唤醒）
  3. ring buf 空 + UART TEMT → 返回
- **double-check 模式**: `check TEMT → register DRAIN_WAKER → check TEMT again → park`，防止 ISR 在检查与注册之间触发而丢失唤醒
- **DRAIN_WAKER**: 独立 AtomicWaker（不覆盖 TX_WAKER），ISR 中 `TX_WAKER.wake()` + `DRAIN_WAKER.wake()` 同时调用
- **效果**: 64 字节 tcdrain 从 9 次切换降至 ~6 次，QEMU 延迟 ~300→~200 µs
- **文件**: `kernel/src/drivers/isr.rs:8`, `kernel/src/syscall/fs/ctl.rs:44-65`

<!-- L145 --> ### benchmark 测试公平性诊断（2026-06-01）
- **问题**: Console `write()` 本身阻塞到发送完成，Async `write()` 非阻塞 push + 显式 `tcdrain()`。测的不是同一个时间点
- **Console QEMU**: 纯 VFS+MMIO 速度（~5 µs/64B），因为 QEMU LSR 永远 THR_EMPTY
- **Async QEMU**: VFS + 任务切换（~300 µs/64B），因为 tcdrain 需要多次 poll → yield
- **公平对比**: 去除 tcdrain，只比 write() 延迟（Async 快 2.2-7.5x，§3 已验证）
- **真板**: 两者受 115200 bps 限制，收敛到 ~11.5 KB/s；QEMU 的差距是人工产物
