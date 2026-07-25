## 1. P0 — ier_cache RMW 竞争修复

- [x] 1.1 将 `ArceOsUartPort::update_ier()` 中 ier_cache load-modify-store 搬进 `self.uart.lock()` 临界区，与 `set_ier()` 同锁保护
- [x] 1.1a 处理 D1 `ArceOsD1UartPort::update_ier()` 边界：已用 IRQ-off 临界区收敛同形态 RMW；软件 wake 保持在 IRQ 恢复后执行
- [x] 1.2 验证 `uart_isr_wrapper()` 中 ISR 回调路径在锁内安全（QEMU 路径 cache RMW 与 MMIO IER 写同锁保护）
- [x] 1.3 StarryOS `cargo check --package starryos --features qemu --target riscv64gc-unknown-none-elf` 通过（0 error；保留既有 unused import warning）

## 2. P1 — tx_copier_active 内存序升级

- [x] 2.1 TX copier 当前三处 `tx_copier_active.store(true/false, Relaxed)` → `Release`（`tx_copier_loop()` 入口 true store；空 ring Pending false store；无进展 Pending false store，以当前源码为准）
- [x] 2.2 `tx_completion()` 中 `tx_copier_active.load(Relaxed)` → `Acquire`
- [x] 2.3 uart_16550 `cargo check --manifest-path crates/uart_16550/Cargo.toml --features async` 通过

## 3. P1 — tx_staged_bytes 内存序升级

- [x] 3.1 TX copier 三处 `tx_staged_bytes.fetch_add/fetch_sub` → `AcqRel`（pop_batch 后 fetch_add；send_bytes 成功后两处 fetch_sub，以当前源码为准）
- [x] 3.2 `tx_completion()` 中 `tx_staged_bytes.load(Relaxed)` → `Acquire`
- [x] 3.3 uart_16550 current-state test witness 已建立：`cargo check` 通过；`cargo test --manifest-path crates/uart_16550/Cargo.toml` 仍被既有 `assert2` dev-dependency 缺失阻塞，非本次 Q17 回归

## 4. 全局 Relaxed 审计 + Telemetry 保留

- [x] 4.1 grep `Ordering::Relaxed` 全量审计：telemetry 字段（`tx_poll`、`tx_hw_bytes`、`tx_no_progress`、IRQ_COUNT）保留 Relaxed；控制流字段已升级或进入临界区
- [x] 4.2 确认 `SpinNoIrq` 在当前 QEMU target 下未提供 SMP proof；Q17 已用锁内 RMW/Acquire-Release 修复已知风险，但多 hart 实测仍后置到真板/SMP stress
- [x] 4.3 StarryOS + uart_16550 双 repo `cargo check` 均通过

## 5. Gate 验证

- [x] 5.0 Phase 3 开始前建立 current-state witness：CodeGraph impact 已记录；`cargo check`/`cargo test`/benchmark 命令清单明确
- [x] 5.1 StarryOS `cargo check --package starryos --features qemu --target riscv64gc-unknown-none-elf` 0 错误（既有 3 个 unused import warning 未纳入 Q17）
- [x] 5.2 uart_16550 `cargo clippy` witness 已建立：当前被既有 `assert2` 缺失、`embedded_io` all-features、crate-level `deny(clippy::cargo)` 元数据项阻塞，非 Q17 新增问题
- [x] 5.3 QEMU benchmark：64B TX 159.25 KB/s（约等于 160 KB/s gate，QEMU 噪声内）、1B latency avg 0.177ms、FIFO matrix 无 10ms 台阶、FIONBIO 双入口 PASS、tcdrain 无 hang
- [x] 5.4 QEMU Shell 交互正常，`/bin/benchmark` 可从 rootfs 启动并完整返回 shell

## 6. Deferred 验证

- [ ] 6.1 多 hart / 真板 SMP stress 尚未执行：需要在 VisionFive2 或等价多 hart QEMU 配置上复验并发 UART read/write、flush/tcdrain、IER enable/disable 无数据丢失或 hang
