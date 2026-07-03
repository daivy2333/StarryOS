## ADDED Requirements

### Requirement: IER cache RMW atomicity

`update_ier()` 中对 `ier_cache` 的 load-modify-store MUST 与 `set_ier()` MMIO 写入在同一临界区内完成，保证跨 hart 并发调用时 IER 位不丢失。

#### Scenario: ISR 和 copier 并发修改 IER

- **WHEN** ISR(hart 0) 调用 `update_ier(empty, DATA_READY)` 和 TX copier(hart N) 调用 `update_ier(THR_EMPTY, empty)` 并发执行
- **THEN** `ier_cache` 最终值 MUST 同时反映 `DATA_READY` 清除和 `THR_EMPTY` 设置，两个操作不互相覆盖

#### Scenario: IER 写入后 MMIO 寄存器一致性

- **WHEN** `update_ier()` 完成
- **THEN** MMIO IER 寄存器值 MUST 等于 `ier_cache` 最终值

### Requirement: TX copier active state ordering

TX copier 对 `tx_copier_active` 的 store MUST 使用 `Release` 语义，`tx_completion()` 的 load MUST 使用 `Acquire` 语义，保证 copier 退出前对 ring buffer 的修改对 flush/tcdrain 可见。

#### Scenario: flush 等待 copier 退出

- **WHEN** TX copier 在 hart N 上完成最后一轮 pop/send 后 store `copier_active = false` (Release)
- **AND** flush 在 hart M 上 Acquire-load `copier_active`
- **THEN** flush MUST 看到 copier 退出前的所有 ring buffer 变更，不提前返回

### Requirement: TX staged bytes counting ordering

TX copier 对 `tx_staged_bytes` 的 `fetch_add`/`fetch_sub` MUST 使用 `AcqRel` 语义，`tx_completion()` 的 load MUST 使用 `Acquire` 语义，保证 staged 计数变更对 `is_drained()` 可见。

#### Scenario: is_drained 看到最新 staged 计数

- **WHEN** TX copier 在 hart N 上 `fetch_sub(sent, AcqRel)` 降低 `staged_bytes`
- **AND** `tx_completion()` 在 hart M 上 `load(Acquire)` 读取 `staged_bytes`
- **THEN** `is_drained()` MUST 看到最新的 staged 计数值，不因陈旧值误判 drain 完成

### Requirement: Telemetry fields remain Relaxed

纯诊断/统计字段（`tx_poll`、`tx_hw_bytes`、`tx_no_progress` 等 telemetry 计数器）MUST 保持 `Ordering::Relaxed`，不参与控制流正确性。

#### Scenario: Telemetry 不阻塞控制流

- **WHEN** telemetry 计数器使用 Relaxed
- **THEN** `tx_completion()` 和 `is_drained()` 的控制流判断 MUST 不依赖任何 telemetry 字段的值

### Requirement: UartPort implementations have explicit Q17 boundary

所有实现 `UartPort::update_ier()` 的平台路径 MUST 在 Q17 中有明确处理结论：同步满足 IER cache/MMIO 原子更新契约，或记录该平台路径不属于当前 SMP 风险的原因。

#### Scenario: D1 单核路径不被误用为 SMP 证据

- **WHEN** Q17 评估 `kernel/src/drivers/d1_uart.rs` 的 `ArceOsD1UartPort::update_ier()`
- **THEN** Q17 MUST NOT use Lichee RV Dock / D1 single-core board results as evidence for SMP correctness
- **AND** Q17 MUST either update this implementation to the same `UartPort` contract or document why it is excluded from the SMP fix scope

### Requirement: Phase 3 requires current-state witness

进入源码实现前 MUST 建立 fresh current-state witness，至少包含 CodeGraph impact、当前 Relaxed/control-flow 字段清单、以及将要运行的 cargo/check/benchmark 验证命令。

#### Scenario: Implementation starts only after witness

- **WHEN** Phase 3 begins
- **THEN** the implementer MUST have current-state evidence for `update_ier`, `tx_completion`, and `tx_copier_loop`
- **AND** MUST NOT rely on stale line numbers from older Q17 analysis
