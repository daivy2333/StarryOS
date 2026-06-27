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
- `../uart_16550/src/async_/driver.rs` — `tx_copier_active` (4 store + 1 load) + `tx_staged_bytes` (3 RMW + 1 load)，~12 处 Ordering 替换
- `../uart_16550/src/async_/device_ops.rs` — 无直接改动，`flush()` 消费 `tx_completion()` 受益于升级
- 性能：RISC-V `fence r,rw` / `fence rw,w` 几条指令，hot path 增加几 ns，134µs 1B e2e 基线在噪声范围内
