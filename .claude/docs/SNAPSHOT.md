# SNAPSHOT.md - 项目快照

> Last updated: 2026-07-07
> 分支：uart-16550-lichee — Q17 QEMU 修复完成，Q19/Q19B 已完成并归档，Q19C-M0 benchmark evidence/真板 TX 诊断推进中；Q19D 已登记为 D1 SDMMC/rootfs 后续方向

---

## 当前状态

**分支**: uart-16550-lichee（Lichee RV Dock 适配与验证分支；Q17 QEMU 修复完成，Q19/Q19B 真板验证已完成，Q19C fullbench review 后范围收敛，当前聚焦 Q19C-M0 benchmark evidence 与 D1 TX 诊断；Q19D 已登记为 D1 SDMMC/rootfs 后续方向）
**前分支**: asyncuart-dev / feat/uart-16550-async（Q0~Q18 历史开发与整合分支）
**成果**:
- kernel 层异步串口适配层（~50 行），uart_16550 提供完整异步栈（~400 行）
- **OpenSpec 文档体系建立**（2026-06-03）：4 个 spec 域（architecture / learned / references / optimization），全部通过 `openspec validate --specs`；rules 已整合到 CLAUDE.md（迁移墓碑见 `openspec/changes/archive/rules-domain-2026-06-03/`）
- 原 `.claude/docs/{architecture,learned,references,optimization,rules}.md` 已迁移至 `openspec/specs/`，源文件以 `.bak` 保留
**Shell**: stdin/stdout 双向异步，`ls`/`cd`/`pwd` 全部正常。

**历史压缩**:
- Q5~Q15 的详细实现、性能数据与回退历史已压缩到 tasks milestone、architecture ADR、learned 和 archived changes；本快照只保留当前状态。恢复入口见 `openspec/changes/archive/2026-07-02-ARC-202607021648/` 与 `openspec/changes/archive/2026-07-02-ARC-202607021535/`。
- Q13 的 active OS abstraction 已由 ADR-036 修正为 `OsRuntime` + `OsWakerSet` 两个 trait；旧 5-trait 设计归档为历史。
- `.claude/analysis/` 已瘦身：Q18/Q19/Q19B 历史方案、Lichee 原始采集日志和 boot 备份移至 `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/`；原路径保留 tombstone，活跃入口见 `.claude/analysis/README.md`。

**近期完成**:
- **Q17 ✅/⚠️**: QEMU 修复完成；`ier_cache` RMW 临界区化，TX completion 控制流原子序升级，QEMU rootfs benchmark 通过。多 hart / 真板 SMP stress 尚未实测。
- **Q18 ✅**: platform descriptor + early console 分层，QEMU 行为保持。
- **Q19 ✅**: Lichee RV Dock Android boot image + D1 axplat + UART0 polling early console 真板 smoke complete。
- **Q19B ✅**: D1 async UART userbench 真板完成；`/dev/console`、TTY、syscall、`tcdrain`、FIONBIO 全链路通过；大包 TX 达 97.7%~99.0% 线速。
- **Q19/Q19B 归档 ✅**: archived changes 位于 `openspec/changes/archive/2026-07-02-q19-lichee-d1-early-smoke/` 与 `openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/`。
- **Q19C-M0 进行中**: `tests/benchmark.c` 已统一 QEMU/D1 manifest 和测试项，默认移除 4096B 长耗时项；真板数据证明旧 64B 小包约 1KB/s 主要是 section 前 stdout backlog 测量污染，加入 pre-section drain 后 64B 可达约 93%~97% 线速。
- **D1 TX 修复/诊断进展**: D1 `send_bytes()` 已改为 THRE 后最多一次填 16B FIFO，TTY OPOST/ONLCR short-write 计数已修复，S11 1024B 正确发送恢复；`TX_FAST_RETRY_LIMIT=0` + drain 注册 `TX_WAKER` 的优化尝试会在 benchmark 进程启动后卡住，已回退并记录为证伪方案。

**当前待推进**:
- **Q19C 🧪**: OpenSpec change `q19c-lichee-full-starryos-benchmark` 已按 2026-07-04 review 修订；M0 benchmark evidence cleanup 已进入源码与真板验证阶段。下一步不是继续加调试风暴，而是围绕 D1 TX zero-send/P99 长尾设计低风险优化：必须保持启动进展和 Q19B embedded regression，再压低 `hw_send_zero` / `no_progress_budget_exhausted`。
- **Q19D 🧭**: 后续独立方向，承接 Q19C SDMMC probe evidence，目标是真实 D1 SDMMC/block/rootfs 实施和 real rootfs path benchmark；尚未创建 OpenSpec change。
- **Q20 ⏳**: VisionFive2 / 等价多 hart 环境到位后，复验 Q17 O63：并发 UART read/write、flush/tcdrain 与 IER enable/disable 无数据丢失或 hang。

## 最小关键事实

| 主题 | 当前结论 |
|------|----------|
| QEMU benchmark | `BUS=mmio BLK=y make run` 或当前默认 `make run` 可进入 rootfs；`/bin/benchmark` 已通过，适合做功能/回归验证，不适合声明真板线速。 |
| D1 userbench | Lichee RV Dock 已正常运行 userbench，说明 Q17 改动未破坏 D1 单板基本路径；但它不是多 hart stress。 |
| Q17 限制 | 当前只证明 QEMU 单 hart 与 D1 已运行路径无明显功能/性能问题，不能证明跨 hart 内存序彻底关闭。 |
| Q19C-M0 数据边界 | 64B 小包旧异常主要是测量污染；D1 FIFO 16B burst 和 TTY short-write 修复已验证有收益；TX zero-send/P99 长尾仍未解决。 |
| Q19C/Q19D 边界 | Q19C = benchmark manifest + memory-root path loader + SDMMC probe-only；Q19D = 真实 D1 SDMMC/block/rootfs 实施和 real rootfs benchmark。 |

<!-- tombstone: SNAPSHOT-history-blocks --> Archived 2026-07-03 in ARC-202607031929 — 旧关键发现表、阶段表、架构图、项目结构、技术栈、文档索引与代码路径速查已压缩归档，active SNAPSHOT 只保留当前态。
<!-- arc: ARC-202607031929 --> SNAPSHOT 历史结构/技术栈/路径表已压缩归档 (2026-07-03) → ../../openspec/changes/archive/2026-07-03-ARC-202607031929/proposal.md
