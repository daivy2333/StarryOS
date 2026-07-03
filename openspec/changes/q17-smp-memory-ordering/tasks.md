## 1. P0 — ier_cache RMW 竞争修复

- [ ] 1.1 将 `ArceOsUartPort::update_ier()` 中 ier_cache load-modify-store 搬进 `self.uart.lock()` 临界区，与 `set_ier()` 同锁保护
- [ ] 1.1a 处理 D1 `ArceOsD1UartPort::update_ier()` 边界：优先同步收敛同一 `UartPort` 契约；若选择不改，必须在实现记录中说明 D1/C906 单核路径不作为 Q17 SMP 风险
- [ ] 1.2 验证 `uart_isr_wrapper()` 中 ISR 回调路径在锁内安全（`SpinNoIrq` 不会 re-enable 中断）
- [ ] 1.3 StarryOS `cargo check --package starry` 通过

## 2. P1 — tx_copier_active 内存序升级

- [ ] 2.1 TX copier 当前三处 `tx_copier_active.store(true/false, Relaxed)` → `Release`（`tx_copier_loop()` 入口 true store；空 ring Pending false store；无进展 Pending false store，以当前源码为准）
- [ ] 2.2 `tx_completion()` 中 `tx_copier_active.load(Relaxed)` → `Acquire`
- [ ] 2.3 uart_16550 `cargo check` 通过

## 3. P1 — tx_staged_bytes 内存序升级

- [ ] 3.1 TX copier 三处 `tx_staged_bytes.fetch_add/fetch_sub` → `AcqRel`（pop_batch 后 fetch_add；send_bytes 成功后两处 fetch_sub，以当前源码为准）
- [ ] 3.2 `tx_completion()` 中 `tx_staged_bytes.load(Relaxed)` → `Acquire`
- [ ] 3.3 uart_16550 `cargo test` 通过

## 4. 全局 Relaxed 审计 + Telemetry 保留

- [ ] 4.1 grep `Ordering::Relaxed` 全量审计：确认 telemetry 字段（`tx_poll`、`tx_hw_bytes`、`tx_no_progress`）保留 Relaxed，无遗漏跨 hart 控制流字段
- [ ] 4.2 确认 `SpinNoIrq` 在目标 SMP feature 下的实际互斥语义；若无法确认，停止并回 Phase 2 调整设计
- [ ] 4.3 StarryOS + uart_16550 双 repo `cargo check` 均通过

## 5. Gate 验证

- [ ] 5.0 Phase 3 开始前建立 current-state witness：CodeGraph impact 已记录；`cargo check`/`cargo test`/benchmark 命令清单明确
- [ ] 5.1 StarryOS `cargo check --package starry` 0 错误/警告（含 path 依赖 uart_16550）
- [ ] 5.2 uart_16550 `cargo clippy` 0 警告
- [ ] 5.3 QEMU benchmark：1B latency ≤ 140µs（基线 134µs，允许 < 5% noise），64B TX ≥ 160 KB/s（基线 170 KB/s），FIONBIO PASS，tcdrain 无 hang
- [ ] 5.4 QEMU Shell 交互正常（`ls`/`cd`/`echo` 无卡顿）
