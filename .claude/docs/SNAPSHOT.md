# SNAPSHOT.md - 项目快照

> Last updated: 2026-07-11
> 分支：uart-16550-lichee — D1 真板异步 UART 测试正式结束，Q19C 已归档，活跃 change 仅剩 q17-smp-memory-ordering

---

## 当前状态

**分支**: uart-16550-lichee（Lichee RV Dock 适配与验证分支；Q17 QEMU 修复完成，Q19/Q19B/Q19C 真板验证已完成；D1 真板内核态 + 用户态异步 UART 测试正式结束；M3/rootfs-probe 与 Q19D SDMMC/rootfs 已取消为当前规划）
**前分支**: asyncuart-dev / feat/uart-16550-async（Q0~Q18 历史开发与整合分支）
**成果**:
- kernel 层异步串口适配层（~50 行），uart_16550 提供完整异步栈（~400 行）
- **OpenSpec 文档体系建立**（2026-06-03）：4 个 spec 域（architecture / learned / references / optimization），全部通过 `openspec validate --specs`；rules 已整合到 CLAUDE.md（迁移墓碑见 `openspec/changes/archive/rules-domain-2026-06-03/`）
- 原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，源文件以 `.bak` 保留
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常。

**历史压缩**:
- Q5~Q15 的详细实现、性能数据与回退历史已压缩到 tasks milestone、architecture ADR、learned 和 archived changes；本快照只保留当前状态。恢复入口见 `openspec/changes/archive/2026-07-02-ARC-202607021648/` 与 `openspec/changes/archive/2026-07-02-ARC-202607021535/`。
- Q13 的 active OS abstraction 已由 ADR-036 修正为 `OsRuntime` + `OsWakerSet` 两个 trait；旧 5-trait 设计归档为历史。
- `.claude/analysis/` 已瘦身：Q18/Q19/Q19B 历史方案移至 `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/`；Q19C/D1 async UART 收尾材料移至 `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/`。活跃入口见 `.claude/analysis/README.md`。

**近期完成**:
- **Q17 ✅/⚠️**: QEMU 修复完成；`ier_cache` RMW 临界区化，TX completion 控制流原子序升级，QEMU rootfs benchmark 通过。多 hart / 真板 SMP stress 尚未实测。
- **Q18 ✅**: platform descriptor + early console 分层，QEMU 行为保持。
- **Q19 ✅**: Lichee RV Dock Android boot image + D1 axplat + UART0 polling early console 真板 smoke complete。
- **Q19B ✅**: D1 async UART userbench 真板完成；`/dev/console`、TTY、syscall、`tcdrain`、FIONBIO 全链路通过；大包 TX 达 97.7%~99.0% 线速。
- **Q19/Q19B 归档 ✅**: archived changes 位于 `openspec/changes/archive/2026-07-02-q19-lichee-d1-early-smoke/` 与 `openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/`。
- **Q19C-M0 ✅**: `tests/benchmark.c` 统一 QEMU/D1 manifest；64B 小包 pre-section drain 后达 93%~97% 线速；Q19C.8e slow-pool 实施，`slow_poll_exh=0`。
- **Q19C-M1 ✅**: `lichee-fullbench-mem` 通过 memory-root `/bin/benchmark` 在 D1 真板完整运行。
- **Q19C-M2 ✅**: `lichee-memory-root-command` 在 D1 真板完整运行。归档日志记录完整 benchmark sections + exit code 0。
- **Q19C 收尾 ✅ (2026-07-11)**: D1 真板异步 UART 测试正式结束；M3/rootfs-probe 代码删除（feature/Makefile/entry/cfg），证据表补入主 spec；`q19c-m2-m3-acceptance-alignment`、`q19c-lichee-full-starryos-benchmark`、`q19c-async-uart-closeout` 三个 change 已归档。活跃 change 仅剩 `q17-smp-memory-ordering`。

**当前待推进**:
- **Q19D 🧊**: 取消当前规划。只有在项目目标转向 D1 storage/rootfs bring-up 时，才重新提出独立 change。
- **Q20 ⏳**: VisionFive2 / 等价多 hart 环境到位后，复验 Q17 O63：并发 UART read/write、flush/tcdrain 与 IER enable/disable 无数据丢失或 hang。

## 最小关键事实

| 主题 | 当前结论 |
|------|----------|
| QEMU benchmark | `BUS=mmio BLK=y make run` 或当前默认 `make run` 可进入 rootfs；`/bin/benchmark` 已通过，适合做功能/回归验证，不适合声明真板线速。 |
| D1 userbench | Lichee RV Dock 已正常运行 userbench，说明 Q17 改动未破坏 D1 单板基本路径；但它不是多 hart stress。 |
| Q17 限制 | 当前只证明 QEMU 单 hart 与 D1 已运行路径无明显功能/性能问题，不能证明跨 hart 内存序彻底关闭。 |
| Q19C 数据边界 | 64B 小包旧异常主要是测量污染；D1 FIFO 16B burst 和 TTY short-write 修复已验证有收益；TX P99 长尾接受为 known limitation；M1 eager path 和 M2 command-entry 已通过，lazy file-backed COW 仍待单独修复。Q19C 不再要求 shell、SDMMC、block 或真实 rootfs。 |
| Q19C 收尾标准 | 已满足。真板内核态 benchmark + 用户态 benchmark 已覆盖 async UART、`/dev/console`、TTY、syscall、`tcdrain` 和 FIONBIO。后续只保留 Q20 多 hart / 其他真板复验。 |
| Q19D 边界 | Q19D SDMMC/rootfs 不再是当前 roadmap。若以后需要真实 storage/rootfs bring-up，应重新 propose，不能作为 async UART 性能验证的遗留 gate。 |

<!-- tombstone: SNAPSHOT-history-blocks --> Archived 2026-07-03 in ARC-202607031929 — 旧关键发现表、阶段表、架构图、项目结构、技术栈、文档索引与代码路径速查已压缩归档，active SNAPSHOT 只保留当前态。
<!-- arc: ARC-202607031929 --> SNAPSHOT 历史结构/技术栈/路径表已压缩归档 (2026-07-03) → ../../openspec/changes/archive/2026-07-03-ARC-202607031929/proposal.md
