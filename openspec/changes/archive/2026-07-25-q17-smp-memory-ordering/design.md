## Context

当前 QEMU 配置 `max-cpu-num = 1`，所有 task/ISR 运行在单 hart 上，`Ordering::Relaxed` 足够正确。VisionFive2 是 4 核 RISC-V（U74），一旦启用多核，以下共享字段在无合适内存序时会出现真实 bug：

- `SpinNoIrq` 的 SMP 互斥语义必须以实际 `cargo tree -e features` / 构建输出确认为准；当前顶层 `smp` feature 会转发 `axfeat/smp` 和 `axplat-riscv64-visionfive2?/smp`，但 `kernel/Cargo.toml` 直接依赖 `kspin = "0.1"`
- `critical_section` 仅 `disable_irqs()`（本地中断），不提供 SMP 同步
- `embassy_sync::AtomicWaker` 和 `embassy_hal_internal::atomic_ring_buffer` 已正确使用 `AcqRel`——本次仅需修复我们自己的字段

参考文档：`.claude/analysis/q17-smp-memory-ordering.md`、`openspec/specs/optimization/spec.md` O63

## Goals / Non-Goals

**Goals:**
- 修复 `ier_cache` 跨 hart RMW 竞争（P0 — RX/TX 停滞的根因）
- 修复 `tx_copier_active` 和 `tx_staged_bytes` 内存序（P1 — flush/tcdrain 正确性）
- 全局审计 `Ordering::Relaxed`：保留 telemetry 字段，升级控制流字段
- 明确 Q19B 后新增的 D1 `ArceOsD1UartPort::update_ier()` 边界：同步收敛同一 `UartPort` 契约，或显式记录 D1 单核暂不作为 SMP 风险
- QEMU benchmark 无性能退化（< 5% noise 范围）

**Non-Goals:**
- 不引入 `SeqCst`——当前场景不需要全局总顺序
- 不修改 `embassy_sync::AtomicWaker` 或 `embassy_hal_internal::atomic_ring_buffer`（已是 AcqRel）
- 不修改 telemetry 计数器（纯诊断，保留 Relaxed）
- 不在此变更中启用 QEMU SMP 或多核 stress test——那是验证阶段的可选项

## Decisions

### D1: P0 — `ier_cache` RMW 搬进锁内，而非改用 `fetch_or`/`fetch_and`

**选择**: 将 ier_cache 的 load-modify-store 连同 `set_ier()` 一起放入 `self.uart.lock()` 临界区。

**理由**: 
- `update_ier()` 同时修改 `ier_cache` 和 MMIO IER 寄存器，两者需要原子性——`fetch_or`/`fetch_and` 只解决 cache 的原子性，不保证 cache 和 MMIO 之间的一致性
- 锁内做 RMW + `set_ier()` 是最简洁的方案，不需要额外原子操作
- `SpinNoIrq` 在单 hart 下仍是关中断的锁（不是空操作），不存在性能回退

**替代方案**: `AtomicU8::fetch_or`/`fetch_and` + `AcqRel`——但 cache 和 MMIO 之间的 gap 无法消除，需要额外设计；不如锁方案简洁

**当前分支补充**: `kernel/src/drivers/d1_uart.rs` 的 D1 `ArceOsD1UartPort::update_ier()` 也有 `ier_cache` load/store RMW。D1/C906 是单核平台，不作为 Q17 SMP 证明；但它实现同一 `UartPort::update_ier()` 契约，Phase 3 计划应优先同步收敛该路径，除非实施前明确选择“D1 单核路径暂不改”并记录理由。

### D2: P1 — `tx_copier_active` 用 Release/Acquire

**选择**: store → `Ordering::Release`，load → `Ordering::Acquire`

**理由**:
- copier 在 `store(false, Release)` 之前已完成 ring buffer pop/send 操作——Release 保证这些操作对后续 Acquire load 可见
- flush/tcdrain 的 `load(Acquire)` 读到 `false` 后，保证能看到 copier 退出前的所有 ring buffer 状态
- 语义精确：单 writer（copier）+ 多 reader（flush/tcdrain）的 flag 模式

### D3: P1 — `tx_staged_bytes` 用 AcqRel/Acquire

**选择**: `fetch_add`/`fetch_sub` → `Ordering::AcqRel`，load → `Ordering::Acquire`

**理由**:
- RMW 操作既是读也是写——`AcqRel` 保证"读到最新值"（Acquire）且"写入对后续 Acquire 可见"（Release）
- flush/tcdrain 的 `load(Acquire)` 读到计数后，保证能看到对应的 ring buffer 操作
- `is_drained()` 检查 `staged_bytes == 0` 是控制流判断——必须看到最新值

### D4: 不按架构分叉

**选择**: 所有架构统一使用 Rust 标准 `Ordering::*`，不写 `#[cfg(target_arch = "riscv64")]`

**理由**: Rust 原子内存序是语言级契约，编译器针对各架构生成正确指令。按架构分叉增加维护成本，且容易让未来平台遗漏修复。

## Risks / Trade-offs

- **[RISC-V fence 开销]** → 每条 `fence r,rw` / `fence rw,w` 若干 ns。热点路径（TX copier poll cycle 每轮 2-3 次 fence）总开销在 noise 范围内（134µs baseline < 5%）
- **[QEMU 单 hart 无法验证 SMP 正确性]** → 仅能验证无功能退化；SMP 正确性需真板 Q19 阶段复验。但代码审查可确认 happens-before 关系是否正确
- **[`ier_cache` 进锁后 ISR 路径变长]** → ISR 中关中断后拿锁是安全的，`SpinNoIrq::lock()` 在已关中断时是 trivial 操作（~几条指令），不违反 ISR 极简原则
- **[E2 边界：copier_active 和 staged_bytes 无全局一致性]** → `is_drained()` 不要求事务性快照，独立字段的 Release/Acquire 足够。最坏情况是 flush 多等一个 poll cycle——不是正确性问题

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1: IER cache RMW atomicity | 1.1, 1.2, 1.3 | 100% | None | ✅ |
| R2: D1 `UartPort::update_ier()` boundary | 1.1a, 1.3, 4.2 | 100% | D1 单核不作为 SMP 证明；推荐同步收敛契约 | ✅ |
| R3: TX copier active Release/Acquire | 2.1, 2.2, 2.3 | 100% | None | ✅ |
| R4: TX staged bytes AcqRel/Acquire | 3.1, 3.2, 3.3 | 100% | None | ✅ |
| R5: Telemetry remains Relaxed | 4.1, 4.2 | 100% | None | ✅ |
| R6: Verification before Phase 3 implementation | 5.1, 5.2, 5.3, 5.4 | 100% | QEMU SMP is optional precheck, not mandatory implementation scope | ✅ |

Gate 2 result: no uncovered requirement, no unapproved simplification. Phase 3 remains blocked until the user explicitly approves entering implementation.
