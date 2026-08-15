# Spec: knowledge - 项目知识

## Purpose

记录已验证的行为、根因、适用范围和失效边界。条目使用 `Kxx` 编号。不记录单纯文件位置、可从签名读取的 API、未验证猜测或一次性实现细节。Legacy 原文：`openspec/changes/archive/mig-20260720-legacy-specs/learned-original.md`（hash: `f09d4cae`）。

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

- **选型依据**: UART 驱动是专用场景，仅 RX/TX 各一个 waker，无需动态注册/注销；ISR 性能要求高（~1.5 µs），`AtomicWaker::wake()` 是原子操作无分支。

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

### Requirement: K42 — 并发测试污染：症状、定位与隔离

`no_std`/内核 crate 的 host 单元测试引入新的生产态全局共享状态（静态 `AtomicXxx` 单例、fake clock 等）时，MUST 同时设计测试隔离边界。生产代码路径（如 queue future 的每轮调度）读取该全局状态时，任何并行测试写它都会污染兄弟测试。诊断特征是**单测单独跑全过、全量并行跑失败、失败集合不稳定**（每次运行失败测试不同）。修复 MUST 优先走"实例注入"（生产用全局引用、测试注入各自独立实例），其次才是共享隔离边界（SERIAL）。

**证据**: `ms05-qemu-bounded-bidirectional-device-data-plane` iter 007 `001-rework.md` Act Response；`crates/axnet/src/async_rx.rs` 的 `RxRxFuture::diag` 注入、`diag.rs` 的 `TEST_NOW`
**状态**: ✅ 已验证，2026-08-15

- **症状判定**: ① 单独跑目标测试通过，`cargo test` 全量并行失败；② 连续多次全量运行失败集合变化（10→4→5→7 这类漂移）；③ 失败集中在"走同一生产调度函数"的测试。三者齐备基本可判定为共享状态污染而非确定性逻辑 bug。
- **根因形态**: 测试 A 写入全局静态（如 `DIAGNOSTIC` 设置 hold、`set_test_now` 推大 fake clock），测试 B 并行运行时生产路径读到被污染值走不同分支。本项目具体案例：`service_round` 无条件调用 `diag_hold_tick` 读全局 `DIAGNOSTIC`，RW-1 测试设置的 hold 泄漏到 telemetry/round 测试。
- **隔离两层级**: ① 加锁串行化（Task 3.7 的 `SERIAL` 边界）只解决"写方之间"互斥，不解决"写方 vs 不持锁读者"——RW-1 初版只给 hold 测试加 SERIAL，telemetry 测试仍并行被污染；② 根本修复是把状态从全局改为**实例注入**：`RxRxFuture` 持有 `diag: &'static DiagnosticState`，生产传 `&DIAGNOSTIC`，每个测试 `Box::leak` 独立实例，写读都作用在同一实例上，跨测试零共享。
- **配套纪律**: ① 测试结束时必须释放全局状态（hold 用 `OP_RELEASE`+`tick(u64::MAX)`，fake clock 复位 `set_test_now(0)`），否则泄漏到后续测试；② 测试断言必须读与生产路径**同一实例**的 telemetry（`fut.telemetry` 而非全局 `RX_TELEMETRY`），本项目曾因此断言误读；③ 引入任何新全局共享状态时，先回答"哪些并行测试会通过生产路径读它"。

#### Scenario: 全量并行测试出现不稳定失败

- **WHEN** 新加测试后全量 `cargo test` 失败集合不稳定，单独跑各自通过
- **THEN** MUST 先对比单独/全量结果确认并行污染
- **AND** MUST 检查测试是否通过生产代码路径读取被其他测试写入的全局静态状态
- **AND** MUST 优先将共享状态改为 per-test 实例注入，而非仅加串行锁
- **AND** MUST 确保每个写全局状态的测试结束时复位状态

### Requirement: K31 - QEMU 终端与网络端口是独立通道

QEMU 终端与 hostfwd MUST 视为独立通道。`make run` 的终端 I/O 走 NS16550 MMIO UART。`-nographic` 将虚拟串口接到宿主标准输入输出。`hostfwd` 只转发网络端口。

**证据**: `make/qemu.mk:31-55,75-80`；`src/init.sh:12-15`；2026-07-27 无 hostfwd QEMU 启动到 shell
**状态**: ✅ 已验证

- **输出链**: `ax_println!` -> platform console -> UART MMIO -> QEMU serial -> 宿主终端。
- **输入链**: 宿主键盘 -> QEMU serial RX -> UART -> StarryOS TTY。
- **端口边界**: 5555 仅在 guest 程序监听 5555 时可用。`init.sh` 不启动该服务。

#### Scenario: QEMU 有日志但网络端口不可用

- **WHEN** 终端已出现 StarryOS shell，但宿主无法连接 5555
- **THEN** MUST 检查 guest 服务、网卡和 hostfwd
- **AND** MUST NOT 把串口成功当作网络成功

### Requirement: K32 - 当前 QEMU 构建实际选择 VirtIO-MMIO

在纯 PCI 见证通过前，当前 QEMU 构建 MUST 解释为 MMIO。根 `qemu` feature 启用 `bus-pci`，本地 `axfs-ng` 同时启用 `bus-mmio`。`axdriver/build.rs` 在两者并存时优先 MMIO。

**证据**: `Cargo.toml:53-55`；`crates/axfs-ng/Cargo.toml:18`；`axdriver/build.rs:27-32`
**状态**: ✅ 已验证，2026-07-27

- **构建**: `cargo build --offline --release --target riscv64gc-unknown-none-elf --features qemu` 退出码 0。
- **PCI 对照**: 同一镜像挂 PCI net/block 时未发现设备。
- **MMIO 对照**: 改挂 MMIO net/block 后注册两类设备，初始化 `eth0` 并进入 shell。
- **边界**: 该结果证明当前配置选择 MMIO，不证明 QEMU PCI 或上游 PCI 实现不可用。

#### Scenario: 声明 PCI 已可运行

- **WHEN** 开发者准备将 PCI 用作 NIC 基线
- **THEN** MUST 先消除 bus feature 冲突
- **AND** MUST 取得 PCI probe、IRQ、RX 和 TX 的独立运行见证

### Requirement: K33 — fork 版 smoltcp 已知行为偏差

fork 版 smoltcp（`starry-smoltcp 0.12.1`）在 MS01 基线采集中曾暴露两类偏差。UDP 非阻塞 `recvfrom` 无数据时返回 `ENOTCONN(107)`，而非 `EAGAIN(11)`。关闭 TCP listener 后端口不立即释放，需 `sleep(2)` 才能 rebind。当前 smoltcp 0.13.1 + 本地 axnet 基线 MUST 保持这两类偏差已消除。

**证据**: `openspec/changes/t01-smoltcp-axnet-baseline/evidence/000-initial/qemu-socket-baseline.log`；2026-07-28 QEMU 运行见证
**状态**: ✅ 已验证并消除（MS01 完成，2026-07-29，14/14 QEMU PASS on smoltcp 0.13.1 + 本地 axnet）

- **ENOTCONN 偏差**: `recvfrom` on bound-but-not-connected UDP socket 在标准 Linux 返回 EAGAIN，fork 版返回 ENOTCONN。非阻塞语义成立（不返回数据），仅 errno 不同。
- **端口释放延迟**: `close(listen_fd)` 后立即 `bind` 同端口返回 `Address in use`。`listen_table` 未同步清理。
- **迁移预期**: 切换至标准 smoltcp 0.13.1 + 本地 axnet 后两项偏差均应消失。

#### Scenario: MS01 迁移后回归

- **WHEN** 标准 smoltcp + 本地 axnet 替换完成后重跑 `ms01_socket_baseline`
- **THEN** `udp-nonblock` MUST 返回 `EAGAIN(11)` 而非 `ENOTCONN`
- **AND** `tcp-relisten` MUST 无需 `sleep(2)` 也能通过
- **AND** 上述两项已于 2026-07-29 MS01 完成时验证通过（evidence/001 + 002）

### Requirement: K34 — TCP bind 状态属于 kernel sidecar 而非 smoltcp

POSIX bind/getsockname/local_addr 状态 MUST 由 kernel 侧 `HashMap<SocketHandle, IpListenEndpoint>` 维护，smoltcp socket 保持上游干净。外部 socket handle 拥有 bind 记录；accepted handle 从 smoltcp connection tuple 取 local endpoint；`SocketSetWrapper::remove` 统一清理。

**证据**: `t01-smoltcp-axnet-baseline` iter 000-002；bind sidecar 位于 `crates/axnet/src/wrapper.rs:73-78`，14/14 QEMU PASS 含 4 个 bind 专项见证
**状态**: ✅ 已验证

- **存储格式**: INADDR_ANY 存为 `addr: None`。冲突检测 MUST 使用端口级匹配（`endpoint.port == port`），不得用 `endpoint.addr == Some(addr)` 比较——`None` 代表通配，与任何 `Some(...)` 不相等，导致 wildcard bind 后重复 bind 检测失效。
- **生命周期**: bind → listen/connect → close/remove。每次 remove 同时清理 `tcp_bound` HashMap 和 `SocketSet`。
- **冲突检测**: `bind_check` 遍历 `tcp_bound` 做端口匹配；UDP 额外检查 SocketSet 中的 endpoint。

#### Scenario: TCP bind 冲突误报或漏报

- **WHEN** bind 同一端口两次，第二次未被拒绝或错误拒绝
- **THEN** MUST 检查 `tcp_bound` HashMap 的冲突比较是否正确处理 `addr: None`（wildcard）

### Requirement: K35 — Device mask × capability 单元测试模式

device selection / routing 逻辑 MUST 提取为纯策略 helper，输入 `mask: u32` 与按设备顺序排列的 capability 迭代器，输出组合结果。单元测试可在不构造真实 `Box<dyn Device>` 的前提下覆盖 mask × capability 全组合。

**证据**: `ms02-virtio-mmio-polling-baseline` iter 003；`any_masked_device_requires_polling` 位于 `crates/axnet/src/service.rs:37-45`，4 个 mask×polling eligibility 单元测试 + 4 个 deadline min 选择测试 = 8/8 PASS
**状态**: ✅ 已验证

- **签名模式**: `fn helper(mask: u32, capabilities: impl IntoIterator<Item = bool>) -> bool`。`IntoIterator` 接受 `[bool]`、`Vec<bool>`、`map(...).iter()` 等多种形态，便于测试与运行调用。
- **位序约定**: bit `i` in `mask` selects device `i`。helper 内部 `mask & (1 << i) != 0` 命中判断必须与 router 的 `device_mask_for` 使用同一惯例。
- **覆盖矩阵**: 至少四组合 — mask 外 polling、mask 内非 polling、mask 内 polling、mixed devices。mixed 测试同时验证 true 和 false 两种结果以确认决策边界。
- **重构不变性**: helper 提取前后，`register_waker` 等调用点的运行行为 MUST 保持等价。原有 deadline 4/4 测试作为 refactor witness 不得退化。

#### Scenario: 新增 device selection 逻辑

- **WHEN** 引入新的 device 选择或 routing 决策（如未来 IRQn / 多队列 affinity）
- **THEN** MUST 先提取为 `fn(mask, impl IntoIterator<Item=Capability>)` 纯 helper；运行调用通过 `devices.iter().map(|d| d.capability())` 注入；单元测试覆盖 mask × capability 全组合

### Requirement: K37 — 网卡基准分轴与资格层级

网卡基准 MUST 分开记录 environment、driver treatment 和 test。QEMU、真板是 environment；polling、async 是 treatment；协议、方向、payload、flow、profile 和 N00-N54 是 test。

**证据**: R47、R49；MS16 EV-005-07 user-net 六方向运行记录
**状态**: ✅ 已验证，2026-08-06

- **执行资格**: 双端启动，并产生 manifest 和 reason-coded round。invalid 可以通过执行资格。
- **正确性资格**: fingerprint 一致，C6 账本闭合，异常分类满足测试要求。
- **性能资格**: round valid，采样覆盖流量，所需 capability 可用。
- **缺口分类**: 命令可表达但未取得 Evidence 记 `not-run`。CLI、采集器或 telemetry 无法表达测试口径记 `infrastructure-unavailable`。两者均不得记为网卡失败。
- **比较边界**: 同一 environment 内只改变 treatment 才能生成 A/B。QEMU 与真板分别建基线。

#### Scenario: 基准项目没有结果

- **WHEN** 某个 Nxx 项目没有可用结果
- **THEN** MUST 先按 R49 判断它是 `not-run`、`infrastructure-unavailable`、execution failure、correctness invalid 或 performance invalid
- **AND** MUST NOT 把测试设施缺失归因于被测网卡

### Requirement: K38 — 工作区 exclude crate 的测试工具链由项目根 rust-toolchain 决定

`--manifest-path` 运行 workspace-exclude 的本地化 crate 时，工具链解析 MUST 以 rustup
从**当前目录**向上查找的 `rust-toolchain.toml` 为准，不得按 manifest 所在目录推断。crate
目录内自带的 `rust-toolchain.toml` 只有 `cd` 进该目录才生效；从项目根用
`--manifest-path` 运行时实际使用项目根 nightly 工具链。

**证据**: `ms04-qemu-async-rx-queue-baseline` iter 000 T2.1；`crates/virtio-drivers` 在项目根 nightly 下 sound 测试挂起、目录内（stable 1.90）28/28 通过
**状态**: ✅ 已验证，2026-08-10

- **触发条件**: workspace `exclude` 列表中的本地化依赖（patch 到工作区的 registry crate）与项目根工具链存在行为差异（lint 严格度、UB 暴露、feature 行为）。
- **诊断方法**: 从项目根 `cargo test --manifest-path <crate>/Cargo.toml` 与 `cd <crate> && cargo test` 对比；两者工具链不同会给出不同结果。
- **处理原则**: 目标是让 crate 在**项目工具链**下健康（它是项目依赖），不是用 crate 自带工具链回避；必要时修复上游缺陷而非降级工具链。

#### Scenario: 本地化依赖测试结果与基线不一致

- **WHEN** 本地化 crate 的测试在项目根运行与目录内运行结果不同
- **THEN** MUST 确认 rustup 工具链解析（当前目录向上，非 manifest 目录）
- **AND** MUST 修复 crate 使其在项目工具链下通过，或用项目工具链的过滤测试作为 MS04 相关见证

### Requirement: K39 — virtio-drivers 0.7.5 FakeSoundDevice config_space 悬垂指针

`FakeSoundDevice::new` 把栈局部 `config_space` 的 `NonNull` 存入返回的 `FakeTransport`；函数返回后指针悬垂（dangling）。这是 UB：stable 下栈残留恰好读出正确值，nightly 下读垃圾（如 32586）导致断言失败、越界 panic 和子线程 join 永久挂起。根因是所有权设计缺陷，非工具链 bug。

**证据**: `ms04-qemu-async-rx-queue-baseline` iter 000 T2.1；修复前 nightly 下 `empty_info` 断言 `left: 32586, right: 0`、`play`/`stream_info` 越界挂起；修复（`Box` 持有 config_space）后 sound 4/4、全量 34/34 通过
**状态**: ✅ 已验证，2026-08-10

- **修复模式**: 设备结构体持有 `Box<VirtIOSoundConfig>`（堆上稳定地址），transport 引用 `&*box`，生命周期与设备一致；配套给 `VirtIOSoundConfig`/`ReadOnly` 补 `Clone/Debug` derive。
- **通用教训**: fake 设备把 config space 指针存入 transport 时 MUST 保证指针所有者与 transport 同生命周期；`NonNull::from(&mut stack_var)` 跨函数返回是悬垂。
- **诊断特征**: "stable 过 / nightly 挂起" 的差异通常指向未定义行为而非工具链 bug；垃圾值、越界 panic、join 死等组合是悬垂指针典型症状。

#### Scenario: fake 设备测试在新工具链挂起

- **WHEN** fake transport 测试在某工具链下挂起或读垃圾值
- **THEN** MUST 检查 config space / DMA buffer 指针是否由栈变量提供并跨返回逃逸
- **AND** MUST 用 `Box` 或 `Arc` 持有使地址稳定，再判定是否为真实工具链问题

### Requirement: K40 — EVENT_IDX used_event suppression 语义

VirtIO 协商 `RING_EVENT_IDX` 后，`set_dev_notify(bool)` 的 flags 路径失效，必须通过 `used_event` 字段控制 used-buffer 通知。有效 suppression/arm 状态机：suppress 写 `used_event = last_used_idx.wrapping_sub(1)`（设备在写入下一 completion 前 used idx 等于该值才通知，因此不通知）；arm 写 `used_event = last_used_idx` 后执行 strong fence 再 recheck `can_pop()`。suppressed 期间 `pop_used` 只推进 `last_used_idx`，不得覆盖 `used_event`，否则 drain 中自动 rearm。

**证据**: `ms04-qemu-async-rx-queue-baseline` iter 000 T2.1；`crates/virtio-drivers/src/queue.rs` 新增 `suppress_dev_notify`/`arm_dev_notify_and_check`，FakeTransport 6 个新测试（suppress 写入、suppressed-pop 不重臂、arm 空队列、arm 后 pending、flags 模式、u16 wrap）+ queue 15/15 通过
**状态**: ✅ 已验证，2026-08-10

- **arm 后 recheck 的必要性**: arm 写 `used_event` 与设备并发写 used idx 存在竞争；strong fence（`SeqCst`）保证 rearm 先于 recheck 被观测，否则可能错过 arm 与 completion 之间的事件。
- **wrap 处理**: `last_used_idx`、`used_event` 均为 `u16`，suppress 的 `wrapping_sub(1)` 与 arm 的当前值在边界处必须用 `wrapping_*`，测试覆盖 `u16::MAX` 场景。
- **RX-only 约束**: 设备层接口 MUST 只操作 receive queue（`VirtIONetRaw` 的 RX-only suppress/arm），不得复用同时操作 RX/TX 的 `disable_interrupts/enable_interrupts`。

#### Scenario: 有界 drain 期间需要抑制 used 通知

- **WHEN** queue task 要在不超过 budget 的有界循环中服务 completion，且不希望每 completion 触发 IRQ
- **THEN** MUST 先 `suppress_dev_notify()`，drain 中 `pop_used` 不重臂，预算用尽保持 suppressed
- **AND** MUST 恢复等待前 `arm_dev_notify_and_check()` 并以返回值决定是否自唤醒

### Requirement: K41 — MS04 核心异步 RX 的运行时判定边界

单 hart QEMU VirtIO-MMIO 的 MS04 核心基线 MUST 同时证明唯一 RX queue task、空闲无进度、
软件唤醒不搬 descriptor、有界 burst 的回收守恒，以及 budget/self-yield。网络功能成功本身
不能替代 owner、wakeup 和 descriptor 证据。

**证据**: R51；`openspec/changes/archive/2026-08-12-ms04-qemu-async-rx-queue-baseline/`
iteration 009
**状态**: ✅ 核心基线已验证，2026-08-12；⚠ 完整 compatibility 与 exact-binary
重放未声明

- **生命周期**: snapshot 见证 `lifecycle=2`、`owner=1`，且 safety/fault 计数为 0。
- **quiet/wake**: idle 的 IRQ、软件事件、task、descriptor 和 budget delta 全为 0；nudge
  只允许 software/task/empty 各推进一次，descriptor delta 必须为 0。
- **有界 burst**: iteration 009 的 96 包运行满足 reaped/refilled/delivered 守恒，且
  budget exhausted、self-yield、Router full/space wake 均出现。
- **证据边界**: 用户豁免的 MS01/MS02/MS03 重复兼容项、boot raw signature、终止元数据
  和 post-regression safety 仍为 `WAIVED/SKIPPED`，不得提升为 PASS。
- **可复现性边界**: Evidence 中记录的 kernel hash 与后续工作树重建产物不同，因此本次
  结论只绑定已保存的核心运行证据，不声明当前重建镜像的 exact-binary replay。

#### Scenario: 后继 change 复用 MS04 基线

- **WHEN** MS05 或后续 change 依赖 MS04 异步 RX
- **THEN** MUST 按 R51 重跑其受影响的核心模式，或明确引用未受影响的既有证据
- **AND** MUST 将完整 compatibility、SMP、真板和性能资格作为独立 Gate 处理
