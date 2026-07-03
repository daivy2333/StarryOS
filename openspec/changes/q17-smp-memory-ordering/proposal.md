## Why

QEMU 单 hart (`max-cpu-num = 1`) 掩盖了 `ier_cache` 和 TX completion 字段的 Relaxed 内存序问题。VisionFive2 是 4 核 RISC-V，tasks 和 ISR 可能跨 hart 并发——`Relaxed` 在多核下会导致 IER 位丢失（RX/TX 停滞）、flush/tcdrain 看到陈旧状态（hang 或提前返回）。必须在真板 bring-up (Q18/Q19) 前修复，否则 Q19 验证时难以区分 O63 症状和硬件/时钟问题。

## What Changes

- **P0** `ier_cache` RMW 竞争修复：`update_ier()` 中 load-modify-store 搬进 `SpinNoIrq::lock()` 临界区，保证 IER 位操作原子性
- **P1** `tx_copier_active` 内存序升级：store → `Release`，load → `Acquire`，保证 copier 状态对 flush/tcdrain 可见
- **P1** `tx_staged_bytes` 内存序升级：`fetch_add`/`fetch_sub` → `AcqRel`，load → `Acquire`，保证 staged 计数对 `is_drained()` 可见
- 全局 `Ordering::Relaxed` 审计：telemetry/诊断字段保留 `Relaxed`，跨 hart 控制流字段升级

## Capabilities

### New Capabilities

- `smp-memory-ordering`: 定义跨 hart 共享原子字段的内存序契约——哪些字段需要 Release/Acquire/AcqRel，哪些保留 Relaxed，以及验证标准

### Modified Capabilities

<!-- 本变更是正确性修复，不改变现有 spec 级行为需求 -->

## Impact

- `kernel/src/drivers/uart_init.rs` — `ArceOsUartPort::update_ier()` (~6 行改动)
- `kernel/src/drivers/d1_uart.rs` — D1 `ArceOsD1UartPort::update_ier()` 也实现同一 `UartPort` 契约；D1 是单核平台，不作为 SMP 证明，但实施时必须明确同步收敛或显式排除
- `crates/uart_16550/src/async_/driver.rs` — `tx_copier_active` (当前 1 处 true store + 2 处 false store + 1 load) + `tx_staged_bytes` (3 RMW + 1 load)，以当前源码为准替换 Ordering
- `../uart_16550/src/async_/device_ops.rs` — 无直接改动，`flush()` 消费 `tx_completion()` 受益于升级
- 性能：RISC-V `fence r,rw` / `fence rw,w` 几条指令，hot path 增加几 ns，134µs 1B e2e 基线在噪声范围内

## Workflow V5 Phase 1 BDD Gap Scan

> 2026-07-03：交互式 AskUserQuestion 在当前 Default mode 不可用；按 workflow 推荐路径采用“用默认假设补充”，并在本 proposal 中记录场景草图。用户要求在 Phase 3 前停止，因此本次只完成 Phase 1/2，不改源码。

### Happy Path

- QEMU/VisionFive2 多 hart 语义下，并发 `update_ier()` 不丢 RX/TX IER 位，MMIO IER 与 cache 一致。
- TX copier 完成 pop/send 后，`flush()` / `tcdrain` 通过 Acquire 读取 active/staged 状态，不提前返回。
- telemetry 计数仍保持 `Relaxed`，不参与 `tx_completion()` / `is_drained()` 控制流。
- QEMU 单 hart benchmark 与 Shell 行为无性能或功能退化。

### Sad Path

- 若只改 `ier_cache` 的 load/store ordering 而不解决非原子 RMW，必须视为未覆盖 P0。
- 若 D1 路径保留同形态 RMW，必须在实施说明中标注其单核边界，不能用 D1 真板结果证明 SMP 正确性。
- 若 `SpinNoIrq` 在目标 SMP feature 下未提供跨 hart 互斥，必须停止并回到设计，不能继续宣称锁内 RMW 满足 SMP。

### Edge

- `tx_completion()` 是非事务性四字段快照；允许 flush/tcdrain 多等一个 poll cycle，但不允许提前判断 drained。
- 当前源码中 `tx_copier_active` store 数量已不同于旧任务行号，实施时必须以 CodeGraph 当前函数体为准。
- QEMU SMP 预检可作为增强验证，但不作为 Phase 3 前置实现范围。
