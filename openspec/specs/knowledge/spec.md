# Spec: knowledge - 项目知识

## Purpose

记录已验证的行为、根因、适用范围和失效边界。条目使用 `Kxx` 编号。不记录单纯文件位置、可从签名读取的 API、未验证猜测或一次性实现细节。对应 Legacy: `openspec/specs/learned/spec.md` (hash: `f09d4cae`)。

## Requirements

<!-- arc: ARC-202607251326 --> 18 K 条目已归档 (2026-07-25) -> openspec/changes/archive/2026-07-25-arc-202607251326/proposal.md
<!-- arc: cleanup-uart-documentation-system --> K09 tightened, K23/K24 archived (2026-07-25) -> openspec/changes/archive/2026-07-25-cleanup-uart-docs/

### Requirement: K01 - ISR 极简原则

ISR MUST 最小化：读 ISR -> 禁用中断 -> AtomicWaker::wake() -> 返回。数据搬运推迟到任务上下文（后台 copier 协程）。

**Legacy**: L12, L107, L128 | **状态**: ✅ 已验证（ISR 最小化原则在全部 UART 驱动验证阶段通过）

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
- **THEN** MUST 评估 waker 数量与动态性：固定少数 -> AtomicWaker；通用动态 -> register_irq_waker

### Requirement: K03 - poll_io 标准模式

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

### Requirement: K04 - AtomicWaker 使用模式

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

### Requirement: K09 — Embassy 选型边界

embassy-sync 子集使用 MUST 严格限定在 `AtomicWaker`（ISR 安全唤醒）。第二套 executor 或调度器 MUST NOT 与 `axtask` 共存。其他 embassy 原语评估 MUST 先证明当前实现有可测问题且不与 axtask 架构冲突 — 不得将全部 embassy 网络原语预设为反模式。

**Legacy**: L10, L81-L84 | **状态**: ✅ 已验证（2026-07-25 收紧为 executor 边界）

- **核心约束**: 不引入第二套 executor。`embassy-sync::AtomicWaker` 是唯一已批准的 embassy 依赖。
- **已排除**: `Mutex` 替换 `SpinNoPreempt`（异步 Mutex 强制走 embassy executor，与 axtask 冲突）。
- **判定原则**: 评估前先回答三问：(1) 当前实现有可测问题吗？(2) 替换方案更快/更简洁吗？(3) 不与 axtask 架构冲突吗？

#### Scenario: 评估 embassy 同步原语替换

- **WHEN** 开发者提议用 embassy 同步原语替换现有实现
- **THEN** MUST 先证明三个条件全部满足，否则保持原状
- **AND** MUST NOT 引入第二套 executor 或调度器

### Requirement: K15 - OpenSpec 变更 tasks.md 漂移

实施期间每完成一个子任务 MUST 同步勾选 change 自己的 `tasks.md`，不能只更新全局文档。

**Legacy**: L156 | **状态**: ✅ 已验证

- **根因**: 实施时仅更新全局 tasks.md/SNAPSHOT.md，未同步勾选 change tasks.md。归档时 `openspec status --change` 报 isComplete: false。
- **预防**: 每个子任务完成 -> change/tasks.md 勾选 -> 主 spec 同步 -> 全局状态文档 -> 提交 -> `openspec validate`。
- **归档前验证**: `openspec status --change <name>`（artifacts 全部 done）、tasks.md 全部勾选、delta spec 存在、`openspec validate` 无 ERROR。

#### Scenario: 实施 OpenSpec 变更

- **WHEN** 开发者按 change tasks.md 实施子任务
- **THEN** MUST 每完成一个子任务同步勾选 change 自己的 tasks.md

### Requirement: K16 - SMP 内存序规则

跨 hart 共享的 async UART 状态 MUST 按同步角色使用 Rust 原子内存序，不按架构分叉。

**Legacy**: L212, L318-L320 | **状态**: ✅ QEMU 验证 / ⚠️ multi-hart 待验证
<!-- arc: cleanup-uart-documentation-system --> Field-level examples (ier_cache, tx_copier_active, tx_staged_bytes) from async UART context. See archived q17-smp-memory-ordering for details.

- **ier_cache RMW 竞争**: load-modify-store 在锁外执行时，两个 hart 同时 load -> modify -> store 导致后写者覆盖。修复：RMW 与 MMIO IER 写入放同一锁/临界区。
- **tx_copier_active / tx_staged_bytes**: store 用 Release，load 用 Acquire；fetch_add/sub 用 AcqRel。
- **QEMU 单核掩盖**: QEMU 单 hart 下所有访问串行化，Relaxed ≈ SeqCst。QEMU max-cpu-num=4 + SMP feature 可提前暴露部分问题。
- **QEMU 验证结果**（single-hart）: 64B TX 153.86 KB/s、1B avg 0.182 ms、FIONBIO PASS。QEMU single-hart 不能替代 multi-hart 证明。

#### Scenario: 真板多核下出现数据丢失或 hang

- **WHEN** multi-hart stress 显示 UART 数据丢失、flush hang 或 staged_bytes 漂移
- **THEN** O63 字段 MUST 在修改 UART 语义前先检查

### Requirement: K21 - 真板验证分层

真板 bring-up MUST 分层验证：先验证 FIT/串口/MMIO 可访问，再接 DMA/IRQ/workload。

**Legacy**: L281-L285 | **状态**: ✅ 已验证

- **分层**: boot -> 寄存器（IER/IIR/LSR/FCR/MCR 原值+写后读回）-> PLIC -> waker -> drain -> stress -> userbench。
- **IRQ 验证拆层**: claim -> handler -> device status -> EOI。真板 UART 阶段记录了 claim IRQ、ISR entry、IIR/LSR/IER、RX/TX/DRAIN wake 的完整验证流程。
- **VF2 hart 拓扑**: Boot HART ID=1、HART Count=5；CPU0 是 S7 小核。

#### Scenario: 新增真板平台适配

- **WHEN** 开发者为 StarryOS 新增真板平台
- **THEN** MUST 先完成 polling early console smoke test，再接 async UART、PLIC、timer、rootfs

<!-- arc: cleanup-uart-documentation-system --> K23 (io_uring mapping, UART-specific) archived 2026-07-25.
<!-- arc: cleanup-uart-documentation-system --> K24 (UART concurrency matrix) archived 2026-07-25. SPSC boundary retained in K25.

### Requirement: K25 - SPSC capability 完整边界

完整 SPSC 边界 MUST 包含三要素：unsafe unique constructor + crate-private mutation + exactly-once copier startup。仅标不可 Clone 不足以封闭。

**Legacy**: L300 | **状态**: ✅ 已验证

- **三要素**: (1) unsafe unique raw reader/writer - 阻止 safe constructor 重复取得 consumer；(2) crate-private RX/TX mutation - 阻止绕过角色边界；(3) unsafe exactly-once copier startup - 阻止重复启动创建第二 producer/consumer。
- **StarryOS 额外约束**: direct-ring benchmark 必须在 copier 启动前完成；共享 fd 只消费 ldisc ring。

**SPSC readiness 快照原子序**（L297）：
- `embassy_hal_internal::atomic_ring_buffer` 的 `Reader`/`Writer` 方法要求 `&mut self`。consumer 不能为了查询 RX occupied length 而通过 `UnsafeCell` 借用 producer 的 `Writer`，否则破坏 SPSC 唯一调用方前提。
- 正确做法：直接对底层 ring 原子索引取快照 - RX consumer 先 Acquire 读 `end` 再读 `start`，TX producer 先 Acquire 读 `start` 再读 `end`，用模 `2 * capacity` 的距离得到跨 wrap-around 的总长度。
- 测试 MUST 创建跨存储边界两侧的数据或空闲空间。

#### Scenario: 维护 SPSC adapter

- **WHEN** OS adapter 新增 reader constructor、direct ring benchmark 或 copier startup path
- **THEN** MUST 证明每个 SPSC ring role 恰好一个 producer 和一个 consumer

### Requirement: K26 - UART 经验可迁移到 NIC

UART 已验证经验 MUST 可迁移到 NIC：最小 ISR、register-recheck、显式背压、完成语义。但逐字节 SPSC ring 和 copier MUST NOT 直接迁移 - NIC 必须以 DMA descriptor 和 packet buffer ownership 为基本单位。

**Legacy**: L301-L308 | **状态**: ✅ 已验证

- **可迁移**: ISR 极简、waker 模式、backpressure、completion 分层、QEMU/真板证据分离。
- **不可迁移**: 字节 ring 布局、单一 copier 任务模型。
- **NIC 附加要求**: DMA/cache barrier、generation 隔离 reset 前后对象、单槽 waker 不适用多 waiter、descriptor reclaim ≠ peer delivery。

#### Scenario: 为 NIC 工作复用 async UART 经验

- **WHEN** 未来网络 proposal 引用 async UART 架构
- **THEN** MUST 声明复用哪个 wake、backpressure、completion、ownership 或 validation rule
- **AND** MUST 以 packet buffer 和 DMA descriptor 为模型，不复制字节 ring 布局

### Requirement: K27 - ProcessMode::Manual 删除教训

引入模式枚举时，若某变体无构造路径且超过两个 milestone 未使用 MUST 直接删除，不得保留"预留"。

**Legacy**: L310 | **状态**: ✅ 已验证

- **背景**: ProcessMode::Manual 自引入后从未被构造，其内部 match 分支成为死代码，在后续 API 迁移中累积维护成本。
- **修复**: 删除 Manual 变体与关联分支，TTY/PTY 行为无退化。

#### Scenario: 清理遗留枚举

- **WHEN** 开发者发现 ProcessMode 类有未构造变体的枚举
- **THEN** MUST 先确认变体在所有 cfg 组合下均无构造路径，再删除
