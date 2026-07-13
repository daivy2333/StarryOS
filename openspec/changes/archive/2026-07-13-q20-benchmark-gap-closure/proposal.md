## Why

Q19C 已完成 D1 async UART 性能验证，但 Q20 仍缺可继续支撑 Q21~Q23 的同版测量证据。
当前 benchmark 已覆盖 TX throughput、latency、FIFO boundary、batch drain、writev、FIONBIO 和 D1 TX debug snapshot；缺口是 jitter 指标不统一、counter 只在部分路径输出、raw evidence 没有 Q20 独立归档口径。

本 change 只做 benchmark evidence closure，不改变 UART 驱动语义。
如果实施中发现必须修改 `tx_copier_loop()`、waker、IER、drain 或 TTY 语义，应退出 Q20 并另开 correctness / optimization change。

## What Changes

- 扩展 TX latency / jitter 输出，让 S10/S14/S20/S21 都有可比较的 p50/p99/max 和 ratio。
- 扩展 TX counter / CPU proxy 输出，让 QEMU 和 D1 至少在关键 TX policy 下输出同格式 counter delta。
- 保留现有 D1 TX debug ioctl 路线，并评估是否补充 QEMU 同格式输出。
- 为 Q20 建立 raw evidence 目录，区分 QEMU rootfs log 和 D1 serial log。
- 更新性能报告，只引用 raw evidence，不用汇总表替代 raw log。

## BDD Gap Scan

`request_user_input` 在当前 Default mode 不可用。用户已手动补充关键 BDD 决策：Q20 不做 RX fixed payload 测试。

### User-confirmed Simplification

RX fixed payload gate 从 Q20 移出。
用户理由：RX 测试复杂度较高，收益不高；TX/RX 结构对称，TX 数据足以作为当前性能代理。

本 change 将 RX fixed payload 明确列为 Non-goal。
这不是遗漏项，后续如需要 RX 专项验证应另开 change。

### Happy Path

- QEMU rootfs benchmark 输出 S10/S14/S20/S21 的 TX latency / jitter summary。
- D1 fullbench command-entry benchmark 输出同版 TX latency / jitter summary。
- QEMU 和 D1 输出同形态 S40 TX counter section；D1 输出有效 counter proxy，QEMU 若 counters 为 0 必须显式标记 not-available。
- Q20 raw evidence 目录保存 QEMU rootfs log、D1 serial log 和 evidence README。
- `docs/benchmark-report-async.md` 追加 Q20 结果摘要，并引用 raw evidence。

### Sad Path

- 不能把 QEMU throughput 当成真板线速证据。
- 不能改 UART driver、TTY、drain、IER 或 waker 语义来让 benchmark 变好。
- 不能把 counter proxy 写成精确 CPU 占用率，除非增加 cycle-level measurement。
- 不能删除或破坏 Q19C 已通过的 D1 fullbench path / command-entry 路线。
- 不能声称 Q20 证明 SMP correctness；O63 multi-hart 仍属于 Q24。

### Edge

- D1 P99 长尾应与 `slow_poll_exh`、`yield_exh`、`hw_send_zero`、throughput 同时解释。
- stdout backlog 会污染小包数据；每节必须保留 pre-section `fflush + tcdrain` 见证。
- 如果 QEMU 的 TX debug counters 为 0，应输出同形态 S40 并退化为可解释的 `not-available`，而不是让整段失败。
- RX fixed payload 可以保留现有代码，但 Q20 不要求开启、验证或记录 PASS。

## Scope

### In Scope

- `tests/benchmark.c` 的 TX jitter / counter 输出。
- `Makefile` 中 benchmark 编译宏和 Q20 专用 target，如实施需要。
- `kernel/src/syscall/fs/ctl.rs` 的只读诊断 ioctl 输出形态，如实施需要。
- `.claude/analysis/q20-evidence/` raw evidence。
- `docs/benchmark-report-async.md` Q20 摘要。

### Out of Scope

- RX fixed payload 测试。
- UART driver 语义优化。
- completion queue / user ring / zero-copy。
- D1 SDMMC/rootfs/shell。
- VisionFive2 / multi-hart stress。

## Impact

- `tests/benchmark.c`
- `Makefile`
- `kernel/src/syscall/fs/ctl.rs`
- `crates/uart_16550/src/async_/driver.rs` only if diagnostics need additional read-only fields
- `docs/benchmark-report-async.md`
- `.claude/analysis/q20-evidence/`
- `.claude/docs/tasks.md`
- `.claude/docs/SNAPSHOT.md`

## Gate Notes

- This change follows R16 / L287 / ADR-057.
- Gate 1 BDD uses the user's manual supplement: RX fixed payload is excluded from Q20.
- Gate 2 completeness is captured in `design.md` and `tasks.md`.
