## Context

Q20 starts from an existing benchmark surface, not a blank test harness.
The key implementation points are:

| Area | Existing entry |
|------|----------------|
| TX throughput baseline | `tests/benchmark.c` S10 |
| no-drain enqueue + D1 TX debug | `tests/benchmark.c` S11 |
| batch drain | `tests/benchmark.c` S12 |
| writev aggregation | `tests/benchmark.c` S13 |
| small packet break-even | `tests/benchmark.c` S14 |
| 1B latency | `tests/benchmark.c` S20 |
| FIFO boundary matrix | `tests/benchmark.c` S21 |
| TX debug snapshot | `AsyncUartDriver::tx_debug_snapshot()` |
| user ioctl bridge | `kernel/src/syscall/fs/ctl.rs` |
| kernel IRQ count | `IRQ_COUNT` / `irq_count()` |

Q20 should make these outputs comparable between QEMU and D1.
It must not change the underlying TX path semantics.

## Goals / Non-Goals

**Goals:**

- Add uniform jitter fields for S10/S14/S20/S21.
- Add a stable S40 TX counter section for QEMU and D1; D1 provides effective counter proxy, QEMU may explicitly report not-available when counters are zero.
- Preserve Q19C benchmark manifest and pre-section drain protection.
- Save raw QEMU and D1 evidence under a Q20-specific path.
- Update the benchmark report after raw evidence exists.

**Non-Goals:**

- RX fixed payload testing.
- Any change to `tx_copier_loop()` scheduling, slow-poll, waker, IER, TTY, `tcdrain`, or driver completion semantics.
- Precise CPU utilization unless cycle-level measurement is explicitly added.
- SMP correctness proof.

## Decisions

### D1: RX fixed payload is excluded from Q20

**选择**: 不启用 `BENCH_RX_FIXED_BYTES`，不把 S31 PASS 作为 Q20 gate。

**理由**:
- 当前目标是为 Q21~Q23 的 TX completion / ring / zero-copy 决策建立 TX 基线。
- RX fixed payload 需要稳定输入注入和日志对齐，复杂度高。
- 当前异步 UART 的 TX/RX 数据面均使用 copier + ring + waker 模式，TX counter 足以作为当前性能代理。

**影响**:
- `tests/benchmark.c` 可以保留 S31 代码。
- Q20 报告必须说明 RX fixed payload 不属于本 change。
- 后续需要 RX 专项性能时另开 change。

### D2: Jitter fields are benchmark output, not report-only math

**选择**: 在 benchmark stdout 中直接输出 `p99_p50_ratio` 和 `max_p50_ratio`。

**理由**:
- raw log 应能独立支持结论。
- 后处理脚本或人工表格不能替代 gate evidence。
- Q19C 的 P99 长尾需要在原始 section 下定位。

### D3: CPU data is counter proxy first

**选择**: Q20 优先输出 `bytes/call`、`zero/kb`、`no-progress/kb` 等 proxy，不先引入 cycle accounting。

**理由**:
- 现有 TX debug snapshot 已有足够字段解释 D1 slow-poll 和 FIFO fill 行为。
- cycle-level measurement 会扩大改动面，可能涉及时间源、用户态 ABI 或 per-section kernel counter。
- D1 proxy 足以支持 Q23 的“是否明显恶化”判断；QEMU 只要求保留同形态 S40 section，并在 counters 为 0 时显式输出 not-available。

### D4: Evidence before report

**选择**: 先保存 raw log，再更新 `docs/benchmark-report-async.md`。

**理由**:
- 报告是解释层，raw log 是 Gate 证据。
- QEMU 和 D1 的证据类别不同，必须分文件保存。

## Risks / Trade-offs

| Risk | Handling |
|------|----------|
| RX exclusion hides RX-only regressions | Q20 明确只建立 TX baseline；RX 专项另开 change |
| Counter proxy is overclaimed | 文档和输出均标注 proxy，不称 CPU utilization |
| QEMU/D1 output diverges | QEMU 允许 derived proxy 输出 `not-available`，但 section 名和字段名保持稳定；D1 必须提供有效 counter proxy |
| D1 P99 tail remains unexplained | Q20 只复验和标注，不把根因修复作为 gate |
| Evidence collection needs true board | 代码和 QEMU gate 可先完成；D1 raw log 是最终 Q20 gate |

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1: TX jitter summary | 2.1, 2.2, 5.1, 5.2 | 100% | None | Covered |
| R2: TX counter / CPU proxy | 3.1, 3.2, 5.1, 5.2 | 100% | CPU cycles simplified to D1 counter proxy; QEMU may report not-available | User-approved |
| R3: raw evidence | 4.1, 4.2, 5.1, 5.2 | 100% | None | Covered |
| R4: report update | 6.1 | 100% | None | Covered |
| R5: RX fixed payload | - | 0% by design | Removed from Q20 scope by user | User-approved |
| R6: no driver semantics change | 1.3, 5.3 | 100% | None | Covered |

Gate 2 result: no uncovered in-scope requirement. The only simplification is RX fixed payload removal, explicitly approved by the user before planning.
