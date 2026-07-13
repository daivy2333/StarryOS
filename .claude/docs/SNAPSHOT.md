# SNAPSHOT.md - 项目快照

> Last updated: 2026-07-13
> 分支：uart-16550-lichee — Q20 benchmark gap closure 已完成；Q21/Q22 user ring/completion 优化取消当前规划；活跃 change 剩 q17-smp-memory-ordering 多 hart deferred

---

## 当前状态

**分支**: uart-16550-lichee（Q19/Q19B/Q19C D1 真板异步 UART 验证已结束；Q20 TX latency/jitter/counter 补测完成；Q21/Q22 取消当前规划；Q17 QEMU 修复完成，multi-hart 复验待 Q24）
**前分支**: asyncuart-dev / feat/uart-16550-async（Q0~Q18 历史开发与整合分支）
**成果**:
- kernel 层异步串口适配层，uart_16550 提供完整异步栈。
- OpenSpec 文档体系已建立：architecture / learned / references / optimization；rules 已整合到 CLAUDE.md。
- Shell stdin/stdout 双向异步，`ls`/`cd`/`pwd` 正常。

**历史压缩**:
- Q5~Q15 细节见 tasks milestone、ADR、learned 与 archived changes。
- Q18/Q19/Q19B、Q19C/D1 async UART 材料已移至 `.claude/analysis/_archive/`。
- Q13 旧 5-trait OS abstraction 已由 ADR-036 修正为 `OsRuntime` + `OsWakerSet`。

**近期完成**:
- Q17：QEMU 修复完成；`ier_cache` RMW 临界区化，TX completion 原子序升级，QEMU rootfs benchmark 通过；多 hart 未实测。
- Q19/Q19B/Q19C：D1 smoke、kbench/userbench、memory-root path/command 均完成；Q19C closeout 已归档。
- Q20：QEMU+D1 TX jitter ratio、S40 counter proxy 和 raw evidence 已补齐；RX fixed payload 经用户确认排除；Q20 不声明 SMP 正确性。
- Q21/Q22/Q23：基于 Q20 数据和当前架构评估，user completion queue 与 `mmap` user ring / zero-copy 取消当前规划；可借鉴优化降级为 O82 远期候选。

**当前待推进**:
- Q24：VisionFive2 或等价 SMP 环境复验 O63。
- Q25/Q26：仅在 Q24 或新需求提供数据后，再评估 DMA / 高波特率与维护性清理。

## 最小关键事实

| 主题 | 当前结论 |
|------|----------|
| QEMU benchmark | `BUS=mmio BLK=y make run` 或当前默认 `make run` 可进入 rootfs；`/bin/benchmark` 已通过，适合做功能/回归验证，不适合声明真板线速。 |
| D1 userbench | Lichee RV Dock 已正常运行 userbench，说明 Q17 改动未破坏 D1 单板基本路径；但它不是多 hart stress。 |
| Q17 限制 | 当前只证明 QEMU 单 hart与 D1 已运行路径无明显功能/性能问题；O63 跨 hart 结论必须等 Q24 或等价 SMP stress。 |
| Q19C 数据边界 | 64B 小包旧异常主要是测量污染；D1 FIFO 16B burst 和 TTY short-write 修复已验证有收益；TX P99 长尾接受为 known limitation；M1 eager path 和 M2 command-entry 已通过，lazy file-backed COW 仍待单独修复。Q19C 不再要求 shell、SDMMC、block 或真实 rootfs。 |
| Q20 补测边界 | Q20 只补 TX latency/jitter/counter proxy 和 raw evidence，不改 driver 语义；D1 64B 约 96.7% 线速、1024B 约 98.8% 线速，S40 `slow_poll_exh=0`/`yield_exh=0`；RX fixed payload 不做；SMP 结论仍待 Q24。 |
| Q21/Q22 决策 | 现有异步 UART 已具备 TX ring + copier + `TxCompletion` 的提交/执行分离；D1 115200 bps 线速已成为吞吐瓶颈，user ring/completion queue 当前不实施。 |

<!-- tombstone: SNAPSHOT-history-blocks --> Archived 2026-07-03 in ARC-202607031929 — 旧关键发现表、阶段表、架构图、项目结构、技术栈、文档索引与代码路径速查已压缩归档，active SNAPSHOT 只保留当前态。
<!-- arc: ARC-202607031929 --> SNAPSHOT 历史结构/技术栈/路径表已压缩归档 (2026-07-03) → ../../openspec/changes/archive/2026-07-03-ARC-202607031929/proposal.md
