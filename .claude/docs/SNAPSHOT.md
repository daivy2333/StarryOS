# SNAPSHOT.md - 项目快照

> Last updated: 2026-07-20
> 分支：uart-16550-lichee — Q26 已归档，部分运行时 Gate 为 ENV BLOCK；Q24 等待 SMP 硬件；Q30 维持证据触发

---

## 当前状态

**分支**: `uart-16550-lichee`（Q26/Q27/Q28/Q29 已归档）；历史分支 `asyncuart-dev` / `feat/uart-16550-async`（Q0~Q18）。
**成果**: kernel 异步 UART 适配层 + `uart_16550` 完整异步栈；Shell stdin/stdout 双向异步（`ls`/`cd`/`pwd` 正常）；OpenSpec 21 个 capability spec 已通过校验，rules 位于 `CLAUDE.md`。
**历史压缩**: Q5~Q15 见 tasks/ADR/learned/archived changes；Q18/Q19/Q19B/Q19C 材料在 `.claude/analysis/_archive/`；Q13 旧 5-trait 已由 ADR-036 收敛为 `OsRuntime`+`OsWakerSet`。

**近期完成**:
- Q17：QEMU 修复完成；`ier_cache` RMW 临界区化，TX completion 原子序升级，QEMU rootfs benchmark 通过；多 hart 未实测。
- Q19~Q23：D1 smoke/kbench/userbench/memory-root、Q20 jitter/S40/raw evidence 完成；RX fixed payload 排除，Q20 不证明 SMP；Q21/Q22 user CQ/`mmap` ring 取消，O82 远期保留。
- Q27a：readiness+waker+register-recheck 完成，59 tests+8 doctests、Clippy/rustdoc 通过；Q27：blocking backpressure 完成，nonblocking/ONLCR/PTY 保持，QEMU/D1 通过并归档。
- Q28：raw `AsyncUartWriter` 已收敛为不可 clone、unsafe 唯一构造与 `&mut self` 提交；StarryOS serialized adapter 保留 direct-output/echo clone，compile-fail、并发 accepted-prefix、Q27 回归及静态 Gate 通过。QEMU/D1 单次候选关键指标均未退化超过 3%；已归档 `2026-07-15-q28-async-uart-writer-contract`。
- Q29：raw `AsyncUartReader` 已收敛为 unsafe 唯一构造，RX mutation crate-private，RX/TX copier 启动要求显式单次契约；benchmark-before-copier 消除启动期 SPSC 角色冲突。62 unit + 8 doctest + 10 compile-fail、Clippy/rustdoc/OpenSpec 通过；QEMU build+boot 与 D1 command-entry benchmark 退出码 0；已归档 `2026-07-18-q29-async-uart-reader-contract`。
<!-- Q26 --> - Q26：维护性清理已实施并归档。memtrack feature/API、三态 session 和 8 个 host tests 已完成；Manual、旧 PTY 入口和 ReadOnly mmap 已删除。host/static Gate 通过，部分运行时 Gate 为 ENV BLOCK。

**当前待推进**:
- Q24：VisionFive2 或等价 SMP 环境复验 O63，重点覆盖跨 hart write/flush/tcdrain、read 与 IER enable/disable。
- Q30：仅在 Q24 或真实 workload 提供消息原子性、公平性、锁竞争证据时规划；当前维持 SPSC + producer serialization。

## 最小关键事实

| 主题 | 当前结论 |
|------|----------|
| QEMU benchmark | `BUS=mmio BLK=y make run` 或当前默认 `make run` 可进入 rootfs；`/bin/benchmark` 已通过，适合做功能/回归验证，不适合声明真板线速。 |
| Q17/D1 边界 | Lichee RV Dock userbench 与 QEMU 单 hart路径正常，但不等于 multi-hart stress；O63 跨 hart 结论必须等 Q24 或等价 SMP 环境。 |
| Q19C 数据边界 | 64B 小包旧异常主要是测量污染；D1 FIFO 16B burst 和 TTY short-write 修复已验证有收益；TX P99 长尾接受为 known limitation；M1 eager path 和 M2 command-entry 已通过，lazy file-backed COW 仍待单独修复。Q19C 不再要求 shell、SDMMC、block 或真实 rootfs。 |
| Q20 补测边界 | Q20 只补 TX latency/jitter/counter proxy 和 raw evidence，不改 driver 语义；D1 64B 约 96.7% 线速、1024B 约 98.8% 线速，S40 `slow_poll_exh=0`/`yield_exh=0`；RX fixed payload 不做；SMP 结论仍待 Q24。 |
| Q27 TX backpressure | 阻塞 UART write 会等待 TX ring 空间并完整提交，PTY 保持 short-write；D1 S11 1024B 从 36 short writes/65536B 改善为 0/102400B，S10/S20 性能相对 Q20 持平。 |
| Q28 writer 契约 | raw writer 是唯一 SPSC producer capability；StarryOS clone 共享 task-context producer lock，锁不跨等待。QEMU 关键 P50 改善 7.36%-15.75%，D1 最大退化为 64B P50 +0.107%；单样本不声明统计显著性或 multi-hart 正确性。 |
| Q29 reader 契约 | 每个 driver 仅允许一个 unsafe raw reader，RX producer/consumer mutation 不向 crate 外 safe API 开放；唯一 reader 移入单 `tty-reader`，共享 fd 只消费 ldisc ring。copier 各方向恰好启动一次且晚于 direct-ring benchmark；不扩展为 SMP 证明。 |
<!-- Q26 --> | Q26 维护性清理 | memtrack 三态 session + `axalloc::tracking` API 适配，8 个 host tests 通过；Manual、旧 PTY 入口和 ReadOnly mmap 已删除；`LTO=y` 结果由用户接受。`docs/d1_out.md` 记录 D1 command-entry benchmark 退出码 0，64/256/1024B drain-each 为 96.8%/97.3%/98.8% 线速；该日志不覆盖 memtrack 交互、VTIME、PTY 双向 I/O 和 framebuffer mmap 四项 ENV BLOCK。 |
| Q21/Q22 决策 | 现有异步 UART 已具备 TX ring + copier + `TxCompletion` 的提交/执行分离；D1 115200 bps 线速已成为吞吐瓶颈，user ring/completion queue 当前不实施。 |

<!-- tombstone: SNAPSHOT-history-blocks --> Archived 2026-07-03 in ARC-202607031929 — 旧关键发现表、阶段表、架构图、项目结构、技术栈、文档索引与代码路径速查已压缩归档，active SNAPSHOT 只保留当前态。
<!-- arc: ARC-202607031929 --> SNAPSHOT 历史结构/技术栈/路径表已压缩归档 (2026-07-03) → ../../openspec/changes/archive/2026-07-03-ARC-202607031929/proposal.md
