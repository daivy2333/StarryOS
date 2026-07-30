# Spec: knowledge — 项目知识

## Purpose

记录已验证的行为、根因、适用范围和失效边界。条目使用 `Kxx` 编号。不记录单纯文件位置、可从签名读取的 API、未验证猜测或一次性实现细节。Legacy learned 原文保存在 `openspec/changes/archive/mig-20260720-legacy-specs/learned-original.md`（hash: `f09d4cae`）。

## Requirements

### Requirement: K01 — ISR 极简原则

ISR MUST 最小化：读 ISR → 禁用中断 → AtomicWaker::wake() → 返回。数据搬运推迟到任务上下文（后台 copier 协程）。

**Legacy**: L12, L107, L128 | **状态**: ✅ 已验证（Q0~Q29 全阶段）

- **模式**: ISR 中读 ISR 寄存器判断 InterruptType，禁用 RX/TX 中断防止重入，分别唤醒 rx_waker/tx_waker。
- **安全约束**: ISR 中无阻塞、无锁、MMIO read/write 安全。
- **选型对比**（L128）：

| 方案 | 数据结构 | ISR 复杂度 | 适用场景 |
|------|---------|-----------|----------|
| **AtomicWaker**（本项目采用）| 静态 `AtomicWaker` 变量 | O(1)，无锁 | 固定数量的 waker（如 RX/TX 各一个）|
| **register_irq_waker**（axtask 通用方案）| `BTreeMap<usize, PollSet>` | O(log n)，需要查找 | 通用场景（如同一 IRQ 注册多个 waker）|

- **选型依据**: UART 驱动是专用场景，只有 RX/TX 两个方向各一个 waker；不需要动态注册/注销 waker；ISR 性能要求高（~1.5 µs），`AtomicWaker::wake()` 是原子操作无分支。

#### Scenario: 设计新的 ISR 唤醒路径

- **WHEN** 开发者要设计新的 ISR 唤醒路径
- **THEN** MUST 评估 waker 数量与动态性：固定少数 → AtomicWaker；通用动态 → register_irq_waker

### Requirement: K02 — 双缓冲 Ring Buffer 模式

硬件 FIFO 与内核 ring buffer 之间的搬运 MUST 由单一后台协程完成。ISR 禁止直接操作 ring buffer。

**Legacy**: L11, L13 | **状态**: ✅ 已验证

- **RX 路径**: 硬件 FIFO → ringbuf → 用户空间（copier 从 FIFO 读入 ring，reader 从 ring 读出到用户）
- **TX 路径**: 用户空间 → ringbuf → 硬件 FIFO（writer 写入 ring，copier 从 ring 读出到 FIFO）
- **安全原因**: ringbuf::HeapRb 的 Producer/Consumer 不是中断安全的；atomic_ring_buffer 也需要 SPSC 契约。
- **教训**: 在 ISR 中直接操作 ringbuf 会导致数据竞争（L11）。

#### Scenario: 决定数据搬运位置

- **WHEN** 开发者需要在硬件 FIFO 与用户缓冲区之间搬运数据
- **THEN** MUST 由单一 copier 任务搬运，ISR 禁止直接操作 ring buffer

### Requirement: K03 — poll_io 标准模式

异步 I/O 等待 MUST 使用 `poll_fn(|cx| { try_operation(); register_waker(); Poll::Pending })` 模式。

**Legacy**: L71 | **状态**: ✅ 已验证

```rust
poll_fn(|cx| {
    match try_operation() {
        Ok(val) => Poll::Ready(val),
        Err(WouldBlock) => {
            register_irq_waker(IRQ_NUM, cx.waker());  // kernel/src/file/pipe.rs 参考
            Poll::Pending
        }
    }
}).await
```

#### Scenario: 实现新的异步等待

- **WHEN** 开发者要写新的异步 I/O 等待代码
- **THEN** MUST 复用 poll_fn + register_waker + recheck 模式

### Requirement: K04 — AtomicWaker 使用模式

静态 waker MUST 用于 ISR 中唤醒任务：ISR 中 `WAKER.wake()`；任务上下文中 `WAKER.register(cx.waker())`。

**Legacy**: L72 | **状态**: ✅ 已验证

```rust
static WAKER: AtomicWaker = AtomicWaker::new();
// 任务上下文
WAKER.register(cx.waker());
// ISR 中
WAKER.wake();
```

#### Scenario: 使用 AtomicWaker

- **WHEN** 开发者需要 ISR 安全唤醒任务
- **THEN** MUST 使用静态 AtomicWaker 变量，禁止在 ISR 中使用锁或动态分配

### Requirement: K05 — UART 硬件集成铁律

UART 集成前 MUST 验证全部关键寄存器状态（IER / IIR / LSR / MCR），禁止假设外部 crate 初始化后的状态可用。

**Legacy**: L79, L108 | **状态**: ✅ 已验证（Q0 教训）

- **诊断函数**: `uart_init.rs` 中的 `log_uart_state()` 输出 IER/IIR/LSR/MCR。
- **关键差异**: Console 配置 `IER::DATA_READY`（只使能 RX 中断）；AsyncUart 配置 `IER::DATA_READY | IER::THR_EMPTY`（RX + TX）。UART 重新 init 时使能 TX 中断（覆盖 Console 配置）。

#### Scenario: 在已有 UART 实例上启用新驱动

- **WHEN** 开发者准备让新驱动接管 UART
- **THEN** MUST 先调用诊断函数输出全部寄存器状态，验证后才行后续

### Requirement: K06 — THR_EMPTY vs TEMT 区别

`LSR::THR_EMPTY` (bit 5) 与 `LSR::TRANSMITTER_EMPTY` (bit 6) MUST 严格区分：前者表示 THR 可接受新字节，后者表示 THR + 移位寄存器都为空 = 真正 drain。

**Legacy**: L80, L142 | **状态**: ✅ 已验证

- **教训**: uart_16550 crate 的 THR_EMPTY 注释错误（说 "FIFO completely empty"），实际表示 THR 有空位。需仔细阅读 UART 规范，不依赖库注释。
- **tcdrain 陷阱**: 误用 `THR_EMPTY`（bit 5）会导致 tcdrain 过早返回；必须用 `TRANSMITTER_EMPTY`（bit 6）。实现位置: `kernel/src/syscall/fs/ctl.rs:43-58`，ioctl `TCSBRK=0x5409`。。

#### Scenario: 实现 tcdrain 类等待

- **WHEN** 开发者要实现"等待 TX 完成"的逻辑
- **THEN** MUST 用 `LSR::TRANSMITTER_EMPTY`（bit 6）判断

### Requirement: K07 — QEMU 时序欺骗

QEMU NS16550 不仿真真实串口线延迟（86.8 µs/byte），所有基于 tcdrain / 轮询 LSR 的吞吐量测试在 QEMU 上 MUST 标记为不可信。

**Legacy**: L141 | **状态**: ✅ 已验证

- **物理定律**: 真板 NS16550 @ 115200 bps 线速上限 = 11,520 B/s（单字节 86.8 µs）。
- **可靠指标（QEMU 也可测）**: 内核态 ring buffer 速度、write() 延迟、CPU cycles/byte。
- **真板必须**: 吞吐量验证必须在真板上做，QEMU 数据只用于功能/回归验证。

#### Scenario: 声明性能数字

- **WHEN** 开发者要声明某项性能指标
- **THEN** MUST 注明 QEMU 还是真板、测试方法、数据量。禁止用 QEMU 吞吐量冒充真板吞吐

### Requirement: K08 — 跨层状态传播穷举

任何跨层状态（如 O_NONBLOCK、FIONBIO）MUST 穷举所有入口（open / fcntl / ioctl）并逐个验证。

**Legacy**: L140, L137 | **状态**: ✅ 已验证

- **教训**: 最初只在 sys_ioctl(FIONBIO) 做了转发，但 open(O_NONBLOCK) 和 fcntl(F_SETFL) 只在 File 层设置 flag，未传播到 Tty。一个入口遗漏 = 功能不完整。
- **TX 路径**: AsyncUartWriter::write() 天然非阻塞（push ring buffer），不受 FIONBIO 传播影响。
- **三个入口**: `syscall/fs/fd_ops.rs`（open/fcntl）+ `syscall/fs/ctl.rs`（ioctl）。

#### Scenario: 修改 TTY/串口的全局状态

- **WHEN** 开发者添加新的 fd 状态需要跨层传播
- **THEN** MUST 穷举所有入口，逐个验证状态正确性

### Requirement: K09 — Embassy 选型边界

embassy-sync 子集使用 MUST 严格限定在 `AtomicWaker`（ISR 安全唤醒）。任何 embassy 其它原语替换现有实现的提案 MUST 视为反模式。

**Legacy**: L10, L81-L84 | **状态**: ✅ 已验证

- **已排除的反优化**:
  - Channel 替换 HeapRb：失去 lock-free SPSC，MPMC 多余间接
  - Mutex 替换 SpinNoPreempt：异步 Mutex 强制走 embassy executor，与 axtask 冲突
  - Watch 替换 AtomicBool：单 bool 用 Watch 杀鸡用牛刀
  - Semaphore 计数 NAPI：Semaphore 是资源计数，不是事件计数器
  - select! 替换手动 poll：不与 axtask::future 兼容

- **判定原则**: 评估前先回答三问：(1) 当前实现有可测问题吗？(2) embassy 方案更快/更简洁吗？(3) 不与 axtask 架构冲突吗？

#### Scenario: 评估 embassy 同步原语替换

- **WHEN** 开发者提议用 embassy 同步原语替换现有实现
- **THEN** MUST 先证明三个条件全部满足，否则保持原状

### Requirement: K10 — MMIO 权限诊断优先级

UART LoadFault/StoreFault MUST 先排查 stride、base 地址等代码 bug，再考虑页表权限。

**Legacy**: L117/L118/L121 | **状态**: ✅ 已验证（Q0 关键纠正）

- **误判纠正**: ADR-022/023 认为 axplat 限制 MMIO 权限，实际根因是 stride=4 导致访问 base+8 超出 NS16550 寄存器范围（0x00-0x07 共 8 字节）。
- **验证方法**: raw read at base+5（stride=1）成功而 base+8（stride=4）失败 — 同一 4K 页表，排除页表问题。
- **axmm::iomap()**: 已有稳定 API，`iomap(PhysAddr, size) → VirtAddr`，自动处理映射+保护+TLB flush。

#### Scenario: UART 操作触发 LoadFault

- **WHEN** UART 读写出现 LoadFault / StoreFault
- **THEN** MUST 先按"stride=1 验证 → base 物理地址核对 → axconfig.toml 设备列表"顺序排查

### Requirement: K11 — benchmark 公平性

Async 与 Console 的 write() 语义不同 MUST 在性能对比中明确标注：Console write() 本身阻塞到发送完成，Async write() 非阻塞 push + 显式 tcdrain()。

**Legacy**: L135, L136, L145 | **状态**: ✅ 已验证

- **公平对比**: 去除 tcdrain，只比 write() 延迟（Async 快 2.2~7.5x）。
- **吞吐量限制**: 115200 bps = 11.52 KB/s，无论同步异步都受此限制。异步优势在不阻塞调用方。
- **benchmark 陷阱**: tests/benchmark.c 的 TX 吞吐量测试可能写入 /dev/null（非 /dev/console），绕过 UART。正确应 write → tcdrain() 等实际发送完成。
- **数据量**: 必须统一数据量对比，Console 3,835 cycles/byte vs Async 268 cycles/byte（效率高 14.3 倍）。

**性能基准框架**:
- 内核态: `kernel/src/drivers/bench.rs`（CPU cycle 计数、NAPI 效果、Ring Buffer 写入）
- 用户态: `tests/benchmark.c`（TX 吞吐 64/256/1024/4096B、write() 延迟 P50/P95/P99）
- 自动化: `scripts/benchmark.sh`
- 测试分支: `feat/uart-async-bench`（Async）、`feat/uart-bench`（Console）

**QEMU benchmark 交叉编译部署**:
1. `export PATH=/opt/musl/riscv64-linux-musl-cross/bin:$PATH && make tests/benchmark`
2. `sudo mount -o loop make/disk.img /mnt && sudo cp tests/benchmark /mnt/bin/benchmark && sudo umount /mnt`
3. `make run` → QEMU 内 `./benchmark`
- 约束: `make/disk.img` 是 rootfs 副本（ext4）；`make rootfs` 会覆盖；`BUS=mmio BLK=y` 必需

**Console 对比基准**（统一数据量 102,400B）: Console 3,835 cycles/byte, Async 268 cycles/byte（效率高 14.3×）。Async TX ~1 µs vs busy-wait ~87 µs/byte。Async RX Ring Buffer 读取 588,776 KB/s, P50 600 ns。

#### Scenario: 测量 I/O 性能

- **WHEN** 开发者要对比 Async vs Console 性能
- **THEN** MUST 在相同数据量、相同测试方法下对比，明确标注测试位置（内核态绕过 TTY / 用户态完整链路）

### Requirement: K12 — 性能优化四方向

性能优化 MUST 集中在 IER 缓存、ISR 合并、批量 I/O、waker skip 四个方向。

**Legacy**: L125, L127, O25-O31 | **状态**: ✅ 已验证

- **IER 缓存**: 用 AtomicU8 缓存 IER 值，通过 set_ier() API 写入。
- **ISR 合并**: read_isr_unlocked() 无锁读取 ISR 寄存器，配合 disable_rx/tx_intr()。
- **批量 I/O**: RX copier 单次锁内排空 FIFO；TX copier 单次锁内填满 FIFO。
- **waker skip**: Cell<Option<Waker>> + will_wake 避免重复注册。
- **NAPI 中断合并**: 连续成功 ≥16 次 → 轮询模式（batch 64），减少 90%+ IRQ 频率。
- **TX 单锁**: 消除 double buffer lock，一次 pop + send，只 FIFO 满时 push_back。
- **tcdrain 真异步化**: 三段式等待（ring 有数据 → DRAIN_WAKER → TEMT），double-check 模式防丢失唤醒。效果: 64 字节从 9 次切换到 ~6 次，QEMU 延迟 ~300→~200 µs。

**Q13 trait 抽象开销**（L180-L187）: 提取后 +13% avg latency（124→140.1 µs）。`#[inline(always)]` -5~10 µs，批量操作 -10~20 µs，feature gate 特化 -15~25 µs。算法优化（批量）应尽早做，编译器优化（inline/LTO）可等模块化后按需加。

#### Scenario: 优化热路径性能

- **WHEN** 开发者要优化 ISR 或 copier 性能
- **THEN** MUST 优先考虑 IER 缓存 / 批量 I/O / waker skip / 锁合并四个方向

### Requirement: K13 — 编程模式模板

新代码 MUST 复用以下已验证模式，禁止另起炉灶。

**Legacy**: L73-L77, L120 | **状态**: ✅ 已验证

**设备注册模式**:
```rust
builder.add_device("async_uart_test", DeviceId::new(4, 64), Arc::new(Device::new(device)));
```

**UART 状态诊断模式**:
```rust
let (ier, iir, lsr, mcr) = (uart.interrupt_enable(), uart.interrupt_identification(), uart.line_status(), uart.modem_control());
log::info!("UART State: IER={:#x} IIR={:#x} LSR={:#x} MCR={:#x}", ier, iir, lsr, mcr);
```

**条件编译模式**:
```rust
#[cfg(feature = "async_uart")]
pub fn init() { /* AsyncUart */ }
#[cfg(not(feature = "async_uart"))]
pub fn init() { /* Console */ }
```

**UART 重初始化安全模式**: 读取当前 IER → 只修改需要的位 → 验证修改结果。

**iomap 映射模式**: `axmm::iomap(PhysAddr::from(0x10000000), PAGE_SIZE_4K)`。

**模式 5：内核内部测试模式**:
```rust
// 在 kernel/src/drivers/serial/test.rs
pub fn run_tests() {
    test_device_creation();
    test_write_at();
    test_pollable();
}
// 在 entry.rs init() 中调用
drivers::serial::test::run_tests();
```

#### Scenario: 实现新的串口功能

- **WHEN** 开发者要写新的 UART/TTY/copier 代码
- **THEN** MUST 复用上述 5 个模式中的对应模板

### Requirement: K14 — 构建与部署踩坑

musl 工具链与 rootfs 部署 MUST 按固定流程操作。

**Legacy**: L68-L70 | **状态**: ✅ 已验证

- **musl 工具链**: 位于 `/opt/musl/riscv64-linux-musl-cross/bin`，`export PATH=...:$PATH` 必须在构建前执行。
- **rootfs 部署**: 手动下载 → xz 解压 → 双份 disk.img（项目根 + make/disk.img）。
- **警告清理边界**: 项目原有代码的 unused warnings 不清理（Karpathy 原则"只改必须改的代码"）。

#### Scenario: 第一次在新机器构建

- **WHEN** 开发者换机器或重新配置环境
- **THEN** 必须按"musl 工具链 PATH → 手动 rootfs → 双 disk.img"顺序完整执行

### Requirement: K15 — OpenSpec 变更 tasks.md 漂移

实施期间每完成一个子任务 MUST 同步勾选 change 自己的 `tasks.md`，不能只更新全局文档。

**Legacy**: L156 | **状态**: ✅ 已验证

- **根因**: 实施时仅更新全局 tasks.md/SNAPSHOT.md，未同步勾选 change tasks.md。归档时 `openspec status --change` 报 isComplete: false。
- **预防**: 每个子任务完成 → change/tasks.md 勾选 → 主 spec 同步 → 全局状态文档 → 提交 → `openspec validate`。
- **归档前验证**: `openspec status --change <name>`（artifacts 全部 done）、tasks.md 全部勾选、delta spec 存在、`openspec validate` 无 ERROR。

#### Scenario: 实施 OpenSpec 变更

- **WHEN** 开发者按 change tasks.md 实施子任务
- **THEN** MUST 每完成一个子任务同步勾选 change 自己的 tasks.md

### Requirement: K16 — SMP 内存序规则

跨 hart 共享的 async UART 状态 MUST 按同步角色使用 Rust 原子内存序，不按架构分叉。

**Legacy**: L212, L318-L320 | **状态**: ✅ QEMU 验证 / ⚠️ multi-hart 待验证

- **ier_cache RMW 竞争**: load-modify-store 在锁外执行时，两个 hart 同时 load → modify → store 导致后写者覆盖。修复：RMW 与 MMIO IER 写入放同一锁/临界区。
- **tx_copier_active / tx_staged_bytes**: store 用 Release，load 用 Acquire；fetch_add/sub 用 AcqRel。
- **QEMU 单核掩盖**: QEMU 单 hart 下所有访问串行化，Relaxed ≈ SeqCst。QEMU max-cpu-num=4 + SMP feature 可提前暴露部分问题。
- **QEMU 验证结果**（single-hart）: 64B TX 153.86 KB/s、1B avg 0.182 ms、FIONBIO PASS。QEMU single-hart 不能替代 multi-hart 证明。

#### Scenario: 真板多核下出现数据丢失或 hang

- **WHEN** multi-hart stress 显示 UART 数据丢失、flush hang 或 staged_bytes 漂移
- **THEN** O63 字段 MUST 在修改 UART 语义前先检查

### Requirement: K17 — Q15 增量融合铁律

合并多个 async-uart 优化 commit 时 MUST 按"基线能力 → 修复 → 规范化 → 契约"分层。

**Legacy**: L205-L208, L211 | **状态**: ✅ 已验证

- **分层**: M0 见证层（测量基线）→ M1/M2 修复（fast retry 消除 tick 台阶、drain 修正）→ M4 规范化（IER 单 owner）→ M3 契约（VFS 边界）。
- **铁律**: 禁止一次性 apply 多个优化 commit（Q13 M4 Sync 已证伪 73.9x 退化）。
- **Gate 流水线**: 文档/规格收敛 → QEMU 可验证 correctness → 真板观测脚手架 → 真板 bring-up → 数据驱动决策 → 维护性清理 → 远期实验。

#### Scenario: 应用 Q15 增量融合策略

- **WHEN** 开发者需要合并其他分支的多个 commit
- **THEN** MUST 按依赖排序 + 每步 cargo check + QEMU benchmark Gate

### Requirement: K18 — TEMT corner-case 丢唤醒窗口

真板 NS16550 上 THRE 中断触发时 TEMT 可能为 0，随后 TEMT→1 不产生新中断。实现 drain 等待 MUST 覆盖此 corner-case。

**Legacy**: L201 | **状态**: ✅ 已验证（Q15-M2 修复）

- **修复**: copier 在 send 完最后字节后 bounded spin 256 次等 TEMT。
- **tcdrain 三段式**: ring 有数据 → DRAIN_WAKER → 检查 TEMT → double-check 模式。

#### Scenario: 实现 drain 等待

- **WHEN** 开发者实现 tcdrain/flush 等待
- **THEN** MUST 覆盖 TEMT corner-case，确保最后一字节发送后 drain waiter 被唤醒

### Requirement: K19 — D1 平台关键事实

D1/C906 平台关键事实 MUST 作为 Lichee RV Dock bring-up 基线：单核 Sv39、UART0 `0x02500000` IRQ 18 stride 4 32-bit MMIO、RAM `0x40000000+512MiB`、Android boot image `kernel_addr=0x40200000`。

**Legacy**: L213-L216, L231 | **状态**: ✅ 已验证

- **bring-up 顺序**: D1 axplat + DW APB UART polling early console + Android boot image → 首个成功标准 `[starry-d1] early boot`。
- **烧录流程**: 备份 boot → dd starry img → sync → reboot。恢复：dd backup → sync → reboot。
- **构建 Gate**: 正确 D1 镜像必须接入本地 `axplat-riscv64-lichee-d1`，linker base `0xffffffc040200000`，`DWARF=n`。
- **PTE 属性**: `xuantie-c9xx` feature 必须在 early 和 final page table 都启用 T-Head normal-memory flags。

#### Scenario: 启动 Lichee RV Dock 适配

- **WHEN** 开发者开始 StarryOS Lichee RV Dock 适配
- **THEN** MUST 使用 L213-L216 与 L231 作为事实基线，不再重复从官方 Linux 泛采集

### Requirement: K20 — D1 THRE/no-pending 行为

D1 IRQ 18 可进但 IIR 常为 `0xc1` no pending，有效 THRE `0xc2` 偶发。启用 THRE 时若 LSR 已 ready MUST 软件 wake TX/DRAIN。

**Legacy**: L255-L258, L265-L266, L275 | **状态**: ✅ 已验证

- **tcdrain 卡因**: 等待者只看 TX ring，未覆盖 staged/TEMT。修复：flush()/TCSBRK 注册 DRAIN_WAKER，TX copier 最后一批 TEMT 后 wake drain。
- **slow-poll 结果**: forward progress 未丢（`slow_poll_exh=0`），但 99.84% hw send 返回 0（以 CPU/MMIO polling 换取 forward progress）。P99 根因未明，影响 <2%。
- **Q19B 基线**: 256B 11.25KB/s、1024B 11.40KB/s；1B avg 0.270ms、P50 0.185ms。
- **Q19C 纠正**: 64B 旧 1KB/s 来自 stdout backlog 污染，隔离后 93-97% 线速。

#### Scenario: 优化 D1 TX 路径

- **WHEN** 开发者继续优化 D1 TX copier、THRE wake 或 retry policy
- **THEN** MUST 区分 D1-specific workaround、QEMU timing model 与其他真板实测结果

### Requirement: K21 — 真板验证分层

真板 bring-up MUST 分层验证：先验证 FIT/串口/MMIO 可访问，再接 DMA/IRQ/workload。

**Legacy**: L281-L285 | **状态**: ✅ 已验证

- **分层**: boot → 寄存器（IER/IIR/LSR/FCR/MCR 原值+写后读回）→ PLIC → waker → drain → stress → userbench。
- **IRQ 验证拆层**: claim → handler → device status → EOI。Q24 UART 记录 claim IRQ、ISR entry、IIR/LSR/IER、RX/TX/DRAIN wake。
- **VF2 hart 拓扑**: Boot HART ID=1、HART Count=5；CPU0 是 S7 小核。

#### Scenario: 新增真板平台适配

- **WHEN** 开发者为 StarryOS 新增真板平台
- **THEN** MUST 先完成 polling early console smoke test，再接 async UART、PLIC、timer、rootfs

### Requirement: K22 — memory-root path API

M1 memory-root benchmark MUST 通过 `FS_CONTEXT.create_dir/write("/bin/benchmark")` 注入，再 `resolve/read` 验证路径。

**Legacy**: L276 | **状态**: ✅ 已验证

- **成功路径**: `load_user_app_eager_from_path()`。
- **lazy COW 问题**: `load_user_app()` lazy tmpfs/COW 可进用户态但在 main 前 RV64C `c.ld` SIGILL — 取指/读字节问题，不是 UART。记为 O80。

#### Scenario: 注入 memory-root benchmark

- **WHEN** 开发者需要在 memory root 中注入可执行文件
- **THEN** MUST 通过 FS_CONTEXT.create_dir/write + resolve 验证路径

### Requirement: K23 — io_uring 设计映射

io_uring 设计映射 MUST 明确：StarryOS 在任务模型 + 批处理 + ISR 极简层面和 io_uring 高度同构；缺少部分（mmap / syscall / 多 op）是架构取舍。

**Legacy**: L291-L295 | **状态**: ✅ 已验证

| io_uring 概念 | StarryOS 等价物 | 文件位置 |
|---|---|---|
| SQ（提交队列）| RingBufTx / RingBufRx（SPSC）| `crates/uart_16550/src/async_/ring_buffer.rs:30` / `:134` |
| completion 观测 | TxCompletion（全局 drain snapshot）| `crates/uart_16550/src/async_/driver.rs:104` |
| io_uring_submit | tx.push(buf) | `crates/uart_16550/src/async_/device_ops.rs:107` |
| io_uring_wait_cqe | poll_fn + register_waker + DRAIN_WAKER | `crates/uart_16550/src/async_/device_ops.rs:131` |
| SQPOLL（内核轮询）| TX copier 任务 loop 轮询 ring，靠 ISR 唤醒 | `crates/uart_16550/src/async_/driver.rs:456` |
| ISR 极简（4 步）| uart_isr_handler | `crates/uart_16550/src/async_/isr.rs:83` |

**TxCompletion 是全局 drain snapshot，不是 per-request CQE**。块设备、网络的 drain/flush 可复用四阶段模式。

#### Scenario: 使用 io_uring 映射表

- **WHEN** async UART 设计工作引用 io_uring
- **THEN** MUST 使用此表作为映射辅助，不作为 StarryOS 有 io_uring 兼容 SQ/CQ 语义的证明

### Requirement: K24 — Q28 后并发边界矩阵

Q28 后并发边界 MUST 按矩阵分流：TX raw producer capability（Q28 ✅）、跨 hart correctness（Q24 ⏳）、syscall 原子性（Q30 🧊）、SPSC vs MPSC（O85）、RX multi-consumer（Q29 ✅）。

**Legacy**: L299 | **状态**: ✅ 已验证（Q28/Q29）

| 问题 | 当前保证 | 证据入口 |
|---|---|---|
| 跨 hart write/flush/tcdrain | QEMU/D1 单 hart 回归通过；无 multi-hart 证明 | Q24 / O63 |
| syscall 原子性/公平性/交错 | 仅每次 raw submission accepted prefix 连续 | Q30 / O86 |
| SPSC vs MPSC | TX ring 仍 SPSC，OS adapter 用 producer lock 串行化 | O85 / Q30 |
| RX multi-consumer | raw reader unsafe unique, shared fd 只消费 ldisc ring | Q29 ✅ |

#### Scenario: 解释 Q28 并发证据

- **WHEN** 报告或 proposal 以 Q28 作为并发证据
- **THEN** MUST 将声明限定在 unique raw TX producer capability 和 accepted-prefix integrity

### Requirement: K25 — SPSC capability 完整边界

完整 SPSC 边界 MUST 包含三要素：unsafe unique constructor + crate-private mutation + exactly-once copier startup。仅标不可 Clone 不足以封闭。

**Legacy**: L300 | **状态**: ✅ 已验证（Q29）

- **三要素**: (1) unsafe unique raw reader/writer — 阻止 safe constructor 重复取得 consumer；(2) crate-private RX/TX mutation — 阻止绕过角色边界；(3) unsafe exactly-once copier startup — 阻止重复启动创建第二 producer/consumer。
- **StarryOS 额外约束**: direct-ring benchmark 必须在 copier 启动前完成；共享 fd 只消费 ldisc ring。

**SPSC readiness 快照原子序**（L297）：
- `embassy_hal_internal::atomic_ring_buffer` 的 `Reader`/`Writer` 方法要求 `&mut self`。consumer 不能为了查询 RX occupied length 而通过 `UnsafeCell` 借用 producer 的 `Writer`，否则破坏 SPSC 唯一调用方前提。
- 正确做法：直接对底层 ring 原子索引取快照 — RX consumer 先 Acquire 读 `end` 再读 `start`，TX producer 先 Acquire 读 `start` 再读 `end`，用模 `2 * capacity` 的距离得到跨 wrap-around 的总长度。
- 测试 MUST 创建跨存储边界两侧的数据或空闲空间。

#### Scenario: 维护 SPSC adapter

- **WHEN** OS adapter 新增 reader constructor、direct ring benchmark 或 copier startup path
- **THEN** MUST 证明每个 SPSC ring role 恰好一个 producer 和一个 consumer

### Requirement: K26 — UART 经验可迁移到 NIC

UART 已验证经验 MUST 可迁移到 NIC：最小 ISR、register-recheck、显式背压、完成语义。但逐字节 SPSC ring 和 copier MUST NOT 直接迁移 — NIC 必须以 DMA descriptor 和 packet buffer ownership 为基本单位。

**Legacy**: L301-L308 | **状态**: ✅ 已验证

- **可迁移**: ISR 极简、waker 模式、backpressure、completion 分层、QEMU/真板证据分离。
- **不可迁移**: 字节 ring 布局、单一 copier 任务模型。
- **NIC 附加要求**: DMA/cache barrier、generation 隔离 reset 前后对象、单槽 waker 不适用多 waiter、descriptor reclaim ≠ peer delivery。

#### Scenario: 为 NIC 工作复用 async UART 经验

- **WHEN** 未来网络 proposal 引用 async UART 架构
- **THEN** MUST 声明复用哪个 wake、backpressure、completion、ownership 或 validation rule
- **AND** MUST 以 packet buffer 和 DMA descriptor 为模型，不复制字节 ring 布局

### Requirement: K27 — Q26 ProcessMode::Manual 删除教训

引入模式枚举时，若某变体无构造路径且超过两个 milestone 未使用 MUST 直接删除，不得保留"预留"。

**Legacy**: L310 | **状态**: ✅ 已验证

- **背景**: ProcessMode::Manual 自 Q7 引入后从未被构造，其内部 match 分支成为死代码，在后续 API 迁移中累积维护成本。
- **修复**: Q26 删除 Manual 变体与关联分支，TTY/PTY 行为无退化。

#### Scenario: 清理遗留枚举

- **WHEN** 开发者发现 ProcessMode 类有未构造变体的枚举
- **THEN** MUST 先确认变体在所有 cfg 组合下均无构造路径，再删除

### Requirement: K28 — D1 构建踩坑集

D1 构建中积累的关键踩坑 MUST 在后续 D1 或其他真板适配时作为检查清单复用。

**Legacy**: L223, L226, L227, L233, L234, L254 | **状态**: ✅ 已验证

**axplat 版本对齐铁律**（L223）：
- 创建新 axplat crate 时 MUST 以 `make build` 输出中实际编译的版本为准（如 `v0.3.1-pre.6`），不可用 cargo registry 中找到的最新版（如 `v0.4.1`）。
- `#[impl_interface]` vs `#[impl_plat_interface]` 宏名因版本而异。

**Cargo.lock 版本污染**（L226）：
- 对 workspace 内 path dependency 执行 `cargo check --manifest-path` 会升级 Cargo.lock 中未锁死的依赖。修复：用 `=0.3.0-preview.8` 精确锁死，然后 `git restore Cargo.lock` 恢复。

**D1 IRQ interface 分层**（L227）：
- `lichee-d1` 构建通过全局 feature 触发 `axplat/irq` 接口符号；D1 axplat 不提供 `IrqIf` 时链接报 undefined `__IrqIf_register` / `__IrqIf_set_enable` / `__IrqIf_handle`。
- 修复：`irq-if = ["axplat/irq"]` + `irq_stub.rs` no-op `IrqIf`，先满足运行时符号，不提前启用 PLIC。

**D1 virtio 空 MMIO 修复**（L233）：
- D1 没有 virtio-mmio。`virtio-mmio-ranges` 必须写成空数组 `[]`，不能写成 `[[0,0]]` 占位；后者会让 `axdriver_virtio::probe_mmio_device` 访问 `phys_to_virt(0)`，fault VA 表现为 `0xffffffc000000000`。

**Lichee smoke feature gate**（L234）：
- Lichee Q19a 只验证 boot + early console，必须把 fs/net/display/axdriver/PCI/task-ext 从 smoke 路径隔离。否则出现 `No block device found!`、`PCI_ECAM_BASE`/`PCI_RANGES`/`PCI_BUS_END` 缺失，或 `TaskExt` extern_trait 链接符号缺失。

**embedded ELF 禁 PIE**（L254）：
- D1 embedded benchmark MUST 用 `-static -no-pie -fno-pie -s` 编译。用 `file`、`readelf -h`、`readelf -r` 验证 `ET_EXEC` 且无 relocation。payload 必须是 `ET_EXEC`、no relocation、no static PIE。

#### Scenario: D1 构建排障

- **WHEN** D1 构建出现链接错误、运行时 fault 或 image 尺寸异常
- **THEN** MUST 按本清单逐项排查：axplat 版本 → Cargo.lock → IRQ stub → virtio-mmio → feature gate → ELF PIE

### Requirement: K29 — D1 userbench feature 继承陷阱

`lichee-d1-userbench = ["lichee-d1-kbench"]` 会导致 userbench 继承 kbench 的排除项（`file`/`mm`/`pseudofs`/`task`/`ASYNC_TTY`），编译报 unresolved imports。硬件能力 feature（D1 async UART/PLIC）和运行模式 feature（kbench-only halt）MUST 拆开，不能让 userbench 继承会排除用户态 runtime 的 kbench gate。

**Legacy**: L245 | **状态**: ✅ 已验证

#### Scenario: 新增 D1 运行模式 feature

- **WHEN** 开发者为 D1 新增运行模式 feature
- **THEN** MUST 确认不会通过 feature 继承链排除用户态/文件系统/bencmark runtime 模块

### Requirement: K30 — 用户态 async read 完整调用链

用户态 async read MUST 理解其经过 3 层嵌套 `block_on` + `poll_io` 的完整路径，才能正确诊断阻塞/非阻塞行为。

**Legacy**: L138 | **状态**: ✅ 已验证

```
sys_read → File::read → block_on(poll_io(File, IN, nb, || inner.read()))
  → Device::read_at → Tty::read_at → block_on(poll_io(JobControl, IN, false, || ldisc.read()))
  → ldisc.read → block_on(poll_io(WaitPollable, IN, false, || buf_rx.pop_slice()))
```

- **关键点**：3 层嵌套 block_on、Manual 模式 `waker.wake_by_ref()`、无 nonblocking 传播。
- **文件路径**：`kernel/src/file/fs.rs → kernel/src/pseudofs/dev/tty/mod.rs → .../terminal/ldisc.rs`

#### Scenario: 诊断 async read 阻塞

- **WHEN** async read 出现意外阻塞或 nonblocking 行为异常
- **THEN** MUST 沿此调用链逐层检查 poll_io 的 nonblocking 参数传播
