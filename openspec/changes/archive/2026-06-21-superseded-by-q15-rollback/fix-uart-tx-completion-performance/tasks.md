## 1. 根因见证与基准基础设施

- [ ] 1.1 在双 repo 记录 pre-M4（StarryOS `04f8920` / uart_16550 `60c5729`）、M4（`11e60ff` / `2e91e40`）和当前 HEAD 的构建配置、tick=100Hz、FIFO=16、benchmark 命令；验收：记录中包含 406µs 与 29.99ms 原始证据且不把理论线速当 before 基线。
- [ ] 1.2 扩展 `tests/benchmark.c`/脚本以测量 1/15/16/17/31/32/33/48/49/64/256/1024/4096B，输出每尺寸原始样本、P50/P95、阻塞/Async 对照；验收：musl 编译成功，输出可机器解析。
- [ ] 1.3 暴露仅 benchmark 使用的 `tx_poll/tx_hw_bytes/tx_no_progress/irq_storm/accepted/popped/dropped` delta 和 CPU idle 观测；验收：release 默认路径不打印 per-event 日志。
- [ ] 1.4 Verify RED：在 M4 行为上运行 FIFO 边界 benchmark，并仅恢复 `7c1e9f4` TX `Pending→Ready` hunk做 A/B；验收：展示 M4 台阶、selective hunk 性能恢复及其 busy-poll/counter 代价。

## 2. 有界 TX fast-path backpressure

- [ ] 2.1 在 uart_16550 添加 RED tests：`send_bytes` 在第 1/3/32 次恢复时同一 poll 继续；连续 33 次为 0 时 register→enable→recheck→Pending；验收：当前无条件 Pending 实现失败。
- [ ] 2.2 引入命名常量 `TX_FAST_RETRY_LIMIT=32` 和局部 retry 状态，实现无锁、无 await 的有界 `spin_loop` retry；验收：2.1 RED 转 GREEN，cursor/staging 不丢失。
- [ ] 2.3 增加 lost-wake 交错测试：IRQ 在 register 前、enable 后、final recheck 前发生；验收：每种交错最终均继续发送且没有第 33 次同步 retry。
- [ ] 2.4 Verify GREEN：运行 uart_16550 async tests、Clippy，并在 StarryOS 跑 O62 benchmark；验收：64B–4096B 不劣于同环境阻塞基线 10%，空闲 10 秒 counter 稳定，否则返回 Gate 2 调整预算/方案。

## 3. 三阶段 completion 与可靠 drain

- [ ] 3.1 添加 RED tests：ring pop 后首次 UART write 前调用 flush、部分 staging 未提交、THRE=true/TEMT=false 且无后续 IRQ；验收：当前 accepted/popped + drain_waker 实现至少三项失败。
- [ ] 3.2 在 driver 增加 `tx_copier_active` 与 `tx_staged_bytes`，按 Release/Acquire 更新并提供只读 completion snapshot；验收：pop→stage 窗口不存在 ring-empty 提前完成。
- [ ] 3.3 重写 `AsyncUartWriter::flush` 为 ring/staging/TEMT 三阶段 check→register/recheck，并在最后 TEMT 等待使用 `OsRuntime::yield_now()` 协作重查；验收：3.1 RED 转 GREEN，无锁跨 await。
- [ ] 3.4 迁移 StarryOS `TCSBRK/tcdrain` 复用 driver completion API，删除独立的 ring-empty 推断；验收：mock race、QEMU 1B/64B/4KiB tcdrain 无提前返回或 hang。

## 4. IER 单一所有权

- [ ] 4.1 添加 RED tests：RX/TX enable/disable 交错后 IER 不丢位，外部 stale cache 不得覆盖 driver 更新；验收：现有 callback/cache 双路径被测试见证。
- [ ] 4.2 修改 uart_16550 copier 直接调用 `UartPort::update_ier`，移除 `start_rx_copier/start_tx_copier` enable callback 参数；验收：crate 内无 enable callback 调用者，IRQ 方法仍 backend-aware。
- [ ] 4.3 删除 StarryOS `CACHED_IER/write_ier/enable_rx_intr/enable_tx_intr`，初始化和 ISR 统一经过 ArceOsUartPort owner；验收：CodeGraph callers 无旧路径，stride=1 与 ISR 极简保持不变。

## 5. TTY 短写与 backpressure 契约

- [ ] 5.1 添加 RED tests：TX ring 部分空间、满 ring blocking/nonblocking、PTY writer 和 echo 调用；验收：当前 `TtyWrite=()`/`write_at=buf.len()` 的静默成功被见证。
- [ ] 5.2 将 uart_16550 `TtyWrite::write` 改为返回 `usize`，Async writer 返回实际 push 数；验收：短写值与 dropped counter 一致，embedded-io async 非空写不返回 `Ok(0)`。
- [ ] 5.3 迁移 StarryOS TTY `write_at`、PTY writer、ldisc echo 和所有 CodeGraph callers；验收：blocking 路径等待剩余数据，nonblocking 满 ring 返回 `WouldBlock`，echo 明确 best-effort。

## 6. 热路径次要优化与可观测性

- [ ] 6.1 添加测试/插桩证明 `RingBufTx::push` 的 writer mutex 只包围 `Writer::push`；验收：metrics 与 `poll.wake()` 在解锁后执行。
- [ ] 6.2 实施 producer 临界区缩小并 A/B 测量；验收：功能 counters 不变，IRQ-off 时间下降，结果独立于 O63 主修复提交。
- [ ] 6.3 实现 O64 可关闭的 IRQ→wake→copier-run telemetry，默认 release 禁用；验收：启用时可区分 IRQ delivery 与 scheduler wait，禁用时无 per-event timestamp 成本。
- [ ] 6.4 对 O65 做独立 A/B：评估 metrics feature gate、per-task 批量累计或采样，选择满足可观测性且收益可测的最小方案；验收：若收益低于噪声则显式保留现状并记录 SKIPPED，不混入功能修复。

## 7. 两阶段审查与最终验证

- [ ] 7.1 Spec compliance review：逐条核对三个 delta spec 的所有 Scenario 与测试/实现映射；验收：无 Missing/Simplified，`openspec validate fix-uart-tx-completion-performance --strict` 通过。
- [ ] 7.2 Code quality review：检查 ISR 极简、无锁跨 await、atomic ordering、32 次硬上限、unsafe 契约和双 repo API 迁移；验收：无 open Critical/Important。
- [ ] 7.3 uart_16550 验证：运行 fmt、async/all-features tests、Clippy、doc；验收：完整输出 exit 0，测试 0 failed、Clippy 0 warning。
- [ ] 7.4 StarryOS 验证：运行 fmt/check/clippy、QEMU Shell/FIONBIO/write+tcdrain benchmark；验收：启动正常、无丢字节/hang、性能与 idle 门槛全部满足。
- [ ] 7.5 VisionFive2 验证：运行 64B–4096B、持续 TX、tcdrain 与 idle CPU；验收：不劣于同板阻塞基线 10%、数据完整。无硬件时该项保持未完成并阻塞发布归档，不以 QEMU 替代。
- [ ] 7.6 同步 OpenSpec change、`.claude/docs/tasks.md`、`SNAPSHOT.md`、analysis/learned/architecture/optimization；验收：状态与证据一致后才允许 `/opsx:archive`。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|---|---|---:|---|---|
| M4 TX backpressure 回归见证 | 1.1–1.4, 2.4, 7.4 | 100% | None | ✅ |
| 有界 TX fast-path backpressure | 2.1–2.4 | 100% | None | ✅ |
| 三阶段 TX completion | 3.1–3.4 | 100% | None | ✅ |
| TX 短写与 backpressure 可见 | 5.1–5.3 | 100% | None | ✅ |
| 性能诊断成本可独立控制 | 1.3, 6.1–6.4 | 100% | None | ✅ |
| async-uart-core copier/device ops 更新 | 2.1–5.2 | 100% | None | ✅ |
| ArceOS 性能回归防护 | 1.2–1.4, 7.4–7.5 | 100% | None | ✅ |
| 单一 IER 状态所有权 | 4.1–4.3 | 100% | None | ✅ |

## Execution Stop

本计划停在 Phase 3 入口。未收到用户下一次明确执行授权前，不勾选任务、不创建 RED 测试、不修改 StarryOS 或 uart_16550 源码。
