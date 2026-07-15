# Spec Delta: architecture — carrier arc-202607152005

## REMOVED Requirements

### Requirement: A063 — legacy ADR-025/027 kernel-only UART implementation

The legacy kernel-only implementation boundary MUST be treated as archived history because the reusable crate and two-trait architecture superseded it.

#### Scenario: Looking up the current UART implementation boundary

- **WHEN** developers plan async UART implementation or portability work
- **THEN** they MUST use ADR-033 and ADR-036 instead of legacy ADR-025/027

### Requirement: A064 — legacy ADR-028 Q2 copier/Console exclusion

The Q2/Q3 stage-specific exclusion rule MUST be treated as archived history while its single-drainer safety lesson remains applicable.

#### Scenario: Investigating UART RX consumer competition

- **WHEN** developers investigate multiple readers draining one hardware FIFO
- **THEN** they MUST use A062/Q29 for the active contract and MAY consult A064 for the historical Q2 failure

### Requirement: A056 — original Q21/Q22/Q23 user-ring schedule

The original Q21/Q22/Q23 schedule MUST be treated as archived history because A058 explicitly canceled that active plan.

#### Scenario: Planning post-Q20 UART work

- **WHEN** developers consider user completion queues or mmap user rings
- **THEN** they MUST use A058 and preserve the A062/Q24 multi-hart gate

---

## 完整保留（Archive 区）

### A063 (Archive, legacy ADR-025/027, 2026-07-15)

```markdown
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
```

### A064 (Archive, legacy ADR-028, 2026-07-15)

```markdown
### Requirement: Q2 共存策略 — copier 与 Console 互斥读 UART

在 Console 仍存在的阶段（Q2），RX copier MUST 不启动，由 Console 独占 UART；Q3 替换 Console 后 copier 才接管。

**决策详情**（2026-05-31, ADR-028）：

- **背景**：Q2 同时运行 Console 和 AsyncUart copier 时，Shell 无法接收键盘输入
- **根因**：RX copier 的 `try_receive_byte()` 和 Console tty-reader 的 `read_bytes()` 都读同一个 UART RBR 寄存器。copier 先启动 → 抢在 tty-reader 之前把 FIFO 数据全部读走放入 ring buffer → tty-reader 看到空 FIFO，Shell 收不到输入
- **影响**：Q2 的 `/dev/async_uart` 只提供设备节点和 DeviceOps 基础架构（read/write 在 ring buffer 上操作），实际数据通路（UART ↔ ring buffer）由 Q3 启用

#### Scenario: 出现 reader 竞争

- **WHEN** 项目中出现多个 reader 任务都想 drain 同一硬件 FIFO
- **THEN** 必须设计互斥访问机制（独占控制 / 临界区 / 阶段切换），禁止并发 drain
```

### A056 (Archive, 2026-07-15)

```markdown
<!-- A056 -->
### Requirement: ADR-056: QEMU/D1 可验证 UART 工作前移，multi-hart 真板复验后置

**日期**: 2026-07-12
**状态**: 已接受；Q21/Q22/Q23 当前排期由 ADR-058 取代。
**约束**: 本 ADR 的决定 MUST 作为对应阶段 gate；涉及 Q21/Q22/Q23 时 MUST 以 ADR-058 为准。
**决定**: 原计划为 Q20 先补 latency / jitter / CPU 开销 / RX fixed payload；Q21 做 UART user completion queue MVP；Q22 做 mmap user ring / zero-copy prototype；Q23 做 ring/completion 性能决策；Q24 再做 VisionFive2 / 等价 SMP 的 O63 复验。2026-07-13 后，Q21/Q22/Q23 当前排期由 ADR-058 收敛。
**原因**: QEMU 和 D1 已能跑同版 benchmark；`tests/benchmark.c` 覆盖 write+tcdrain、no-drain、batch drain、writev、FIFO boundary、FIONBIO、optional RX fixed payload；`DeviceOps::mmap()` / `sys_mmap()` 可承接 user ring 原型；D1 单 hart不能证明 O63，但可证明 UART 语义、tail latency、CPU/counter 和 user ring 原型。
**影响**: optimization milestone 从 Q20 重排到 Q26；Q23 成为 ring/completion 保留、收窄或回滚 gate；Q24 仍必须覆盖并发 read/write、flush/tcdrain、IER enable/disable、waker、release/acquire。
**恢复入口**: R15、L286、`.claude/analysis/uart-async-qemu-d1-first-replan.md`。

#### Scenario: Planning UART async work after Q19C

- **WHEN** 使用 ADR-056 规划 async UART 后续 milestone
- **THEN** ADR-058 MUST be checked first for Q21/Q22/Q23 scope
- **AND** multi-hart O63 proof MUST remain required before claiming SMP correctness
- **AND** generic `io_uring` semantics MUST NOT be required unless a later ADR proves UART-specific rings are insufficient
```
