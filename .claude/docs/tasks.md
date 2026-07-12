# tasks.md — 任务追踪

> 由 assistant 维护，uart-16550-lichee 分支。
> 2026-06-25 Q15 M0~M4 增量重融合 + Manual QA 全部完成（QEMU benchmark 验证无 64B write+tcdrain 退化）。
> 2026-06-28 基于 Lichee RV Dock 与 platform-parameter-decoupling 探索结果，roadmap 二次重排：Q17 不动，新增 Q18 平台参数解耦、Q19 荔枝派 early smoke test，原 VisionFive2/DMA/维护阶段顺延为 Q20~Q23。
> 2026-06-29 Q19B 真板 userbench 完成: `starry-lichee-userbench-boot.img` 在 Lichee RV Dock 完整跑完 embedded benchmark，`/dev/console`、TTY、syscall、`tcdrain`、FIONBIO 全链路通过；大包 TX 达 97.7%~99.0% 线速。
> 2026-06-29 Q19 完成：Lichee RV Dock 真板通过官方 U-Boot Android boot image 启动 StarryOS D1 payload，串口输出 `[starry-d1] early boot` 与 `[starry-d1] smoke complete, halting.`。
> 2026-07-03 Q17 QEMU 修复完成：`ier_cache` RMW 纳入临界区，TX completion 控制流内存序升级，QEMU rootfs benchmark 通过；多 hart / 真板 SMP stress 尚未实测，作为 Q20 前置复验项保留。
> 2026-07-04 Q19C review 后曾新增 Q19D 方向：Q19C 只做 memory-root path loader + SDMMC probe-only，真实 D1 SDMMC/block/rootfs 拆到 Q19D。
> 2026-07-07 Q19C-M0 已进入源码与真板数据阶段：统一 benchmark manifest/测试项，移除默认 4096B 长耗时项，隔离 stdout backlog 后 64B 小包恢复接近线速；D1 FIFO 16B burst 与 TTY short-write 修复已验证，TX zero-send/P99 长尾仍待优化，`TX_FAST_RETRY_LIMIT=0` 方案已证伪并回退。
> 2026-07-08 Q19C-M1 已完成：`lichee-fullbench-mem` 通过 memory-root `/bin/benchmark` VFS resolve/read + eager ELF mapping 在 D1 真板完整运行；`load_user_app()` lazy file-backed COW 路径 main 前 SIGILL 已记录为 O80/L277，不作为 async UART gate。
> 2026-07-11 Q19C-M2 真板通过：归档日志完整输出 benchmark sections、`Done.`、`benchmark exited with code: 0` 和 `halting.`。Q19C-M3 真板未通过：日志只到 `d1_sdmmc_controller_base=TBD`，未输出完整 probe table；方向更新后 M3 不再作为 UART gate。
> 2026-07-11 Q19C-M3 旧方案归档：`q19c-m3-polling-console-isolation` 以未实施旧方案归档到 `openspec/changes/archive/2026-07-11-q19c-m3-polling-console-isolation/`，未合入主 spec；M3 后续先重新探索再决定方案。
> 2026-07-11 Q19C 收尾完成：Q19C-M3 代码删除（`lichee-d1-rootfs-probe` feature/Makefile target/entry 分支/cfg 例外），证据表补充到主 spec；`q19c-m2-m3-acceptance-alignment`、`q19c-lichee-full-starryos-benchmark`、`q19c-async-uart-closeout` 三个 change 已归档至 `openspec/changes/archive/`；活跃 change 仅剩 `q17-smp-memory-ordering`。
> 2026-07-11 D1 真板异步 UART 测试正式结束：Q19/Q19B/Q19C 已覆盖 D1 smoke、内核态 benchmark、用户态 benchmark、memory-root path/command 证据；后续不再把 shell、SDMMC、block、rootfs 作为 async UART 验证遗留项。
> 2026-07-12 Q20~Q23 重新规划：基于 ADR-056 / R15 / L286，先推进 QEMU+D1 可验证的 latency+jitter+CPU/RX 补测、UART completion queue、mmap user ring/zero-copy 和性能决策；VisionFive2 / multi-hart O63 复验后置为 Q24。
> 2026-07-04 analysis 文档归档：Q18/Q19/Q19B 历史分析和 Lichee 原始采集日志移至 `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/`；原路径保留 tombstone。
> 2026-07-02 Q19/Q19B OpenSpec changes 已归档：`2026-07-02-q19-lichee-d1-early-smoke`、`2026-07-02-q19b-lichee-d1-benchmark`；活跃 change 仅剩 Q17/Q19C。
> 2026-07-02 状态同步：入口文档、project context、Q19 change tasks 已清理旧分支 / 旧路径 / 已完成但未勾选的状态。
> 2026-07-02 Q19C 规范已完整：原目标是在 Lichee RV Dock 上像 QEMU 一样通过 StarryOS path loader/rootfs 链路运行 benchmark；2026-07-11 已收敛为 D1 async UART 性能验证，rootfs/shell 不再是必达项。
> 2026-06-27 Q15 后 roadmap 首次重排：单一 Q6 拆分为原 Q16~Q22，按 Gate 类型分层推进（见 `.claude/analysis/optimization-milestone-replan.md`）。
> 2026-06-21 M4 Sync 已回退到 pre-M4 基线（04f8920/60c5729），原代码保留在 feat/uart-16550-async-temp。
> 2026-06-21 Q15 开启：从 pre-M4 基线增量重融合 M4+ 正确性修复，每步 Manual QA 验证无退化。
> 2026-06-19 OS trait 清理：ADR-036 删除未使用的 OsIrq/OsMmio/OsSpinNoIrq（5→2 trait），消除 3 个 dead_code warning。
> 2026-06-16 Q13 完成：异步串口完整提取到 uart_16550（9 commits, Phase 1 trait 提取 + Phase 2-3 核心逻辑迁移 + 适配层）。
> 2026-06-03 P0 完成，OpenSpec 文档体系建立（核心 5 域 + 后续 capability specs）。
> 条目格式: <!-- Q{编号} --> 或 <!-- P{编号} --> 标记开头，支持 grep 精确定位。

---

## 当前: 方向 C — kernel 层独立实现（uart-16550-lichee）

> 2026-06-03 完成文档体系迁移：`.claude/docs/{architecture,learned,references,optimization,rules}.md` → `openspec/specs/`，核心 5 个 spec 域完成迁移；后续 archived changes 追加 capability specs。
> 2026-06-01 完成性能分析：发现 3 层 yield storm、Manual 模式缺陷、benchmark 不测 UART、FIONBIO 不传播。

### Milestone 概览

| Milestone | 目标 | Gate | 状态 |
|-----------|------|------|------|
| **Q0** | Spike 验证 | UART 寄存器可读写，ISR 正常 | ✅ |
| **Q1** | 驱动架构实现 | RX/TX copier + ISR + Ring Buffer | ✅ |
| **Q2** | VFS 集成 | DeviceOps + /dev/async_uart + Console 共存 | ✅ |
| **Q3** | AsyncUart RX 接管 | Tty<AsyncUartReader, ConsoleWriter> → Shell stdin | ✅ |
| **Q4** | 全异步 RX+TX | TX copier + ISR，Shell 双向异步 | ✅ |
| **Q5** | 性能优化 | IER 缓存 + ISR 合并 + batch I/O + waker skip | ✅ |
| **Q5.1** | 性能优化续 | NAPI 中断合并 + 批量 API + FCR 阈值日志 + TX interleave 修复 | ✅ |
| **Q5.2** | 测试补全 | 用户态自动化测试 + 非阻塞模式 | ✅ (O43 已落地) |
| **Q7** | 用户态性能修复 | yield storm + FIONBIO 传播 + benchmark 修正 + tcdrain 真异步 | ✅ |
| **P0** | OpenSpec 文档体系 | 核心 5 域迁移 + 后续 capability specs | ✅ (2026-06-03) |
| **Q8** | 驱动引擎打磨 | 正确性修复（NAPI/ISR/IER）+ 热路径优化 + O46 AtomicWaker 推广 | ✅ (2026-06-11) |
| **Q9** | 超时机制 | VTIME 读超时（复用 axtask::future::timeout，无需 embassy-time） | ✅ (2026-06-11) |
| **Q10** | 数据路径优化 | 减少读路径拷贝 + ldisc 优化 | ✅ (2026-06-11) |
| **Q11** | 内核通用优化 | mm/access + close_range + sendfile + tty unwrap | ✅ (2026-06-11) |
| **Q12** | Embassy 路径 A 优化 | atomic_ring_buffer + embedded_io_async + TC tcdrain | ✅ (2026-06-11) → 🗄️ 已归档 `archive/2026-06-15-q12-embassy-path-a/` |
| **Q13** | 异步串口提取 | uart_16550 成为完整异步 UART crate（三阶段迁移） | ✅ (2026-06-16) |
| **Q13-cleanup** | OS trait 清理 | 删除 OsIrq/OsMmio/OsSpinNoIrq（5→2），ADR-036 | ✅ (2026-06-19) |
| **LTO** | 跨 crate 内联优化 | `lto = true`，ring buffer ↑69%，e2e 不变 | ✅ (2026-06-16) |
| **M4 Sync** | async-uart-1 优化合并 | waker race + TX backpressure + ring/copier 诊断计数器 | ⟲ 已回退 (2026-06-21) |
| **Q15** | M4+ 增量重融合 | 从 pre-M4 基线按最小单元重新 apply，每步 Manual QA | ✅ (2026-06-25 M0~M4 + Manual QA 全部完成) |
| **Q16** | Roadmap / spec rebaseline | 任务重排 + stale spec 标注 + validate 已知噪音记录 | ✅ (2026-06-27) |
| **Q17** | SMP / 内存序正确性 | O63：ier_cache RMW + tx completion 原子序 | ✅ QEMU 修复完成 / ⚠️ 多 hart stress 待验证 |
| **Q18** | 平台参数解耦 / early console 基础 | platform descriptor + QEMU 行为保持 + early console 抽象 | ✅ (2026-06-28) |
| **Q19** | Lichee RV Dock early smoke test | Android boot image + D1 platform skeleton + UART0 polling 输出 | ✅ 真板 smoke complete |
| **Q19B** | Lichee D1 async UART benchmark | kbench/userbench Android boot images + embedded benchmark ELF | ✅ 真板 userbench complete |
| **Q19C** | Lichee async UART board benchmark | benchmark evidence cleanup + memory-root path loader + command-entry + closeout | ✅ D1 async UART 性能验证完成并归档（2026-07-11） |
| **Q19D** | Lichee SDMMC/rootfs implementation | D1 SDMMC/block driver + `AxBlockDevice` + real rootfs path benchmark | 🧊 取消当前规划；需要 storage/rootfs 时重新 propose |
| **Q20** | Benchmark gap closure | QEMU+D1 latency / jitter / CPU 开销 / RX fixed payload 补测 | ⏳ 待做 |
| **Q21** | UART user completion queue MVP | UART 专用 submit/completion queue，先 TX，保留 read/write fallback | ⏳ 待做 |
| **Q22** | User ring + zero-copy prototype | `mmap` user ring + zero-copy RX/TX 原型，明确 fallback | ⏳ 待做 |
| **Q23** | Ring/completion performance decision | 对比 write/tcdrain、no-drain、batch-drain、writev 与 user ring | ⏳ 待做 |
| **Q24** | VisionFive2 / multi-hart revalidation | O63/O64/O65/O66/O71/O38/O39 + Q15 Manual QA 真板复跑 | ⏳ 等待硬件 |
| **Q25** | DMA / 高波特率决策 | O3/O40/O69 + O41，依赖 Q23/Q24 数据 | ⏳ 等待数据 |
| **Q26** | 维护性清理 | O48/O49/O50 + release LTO 检查 | ⏳ 待做 |

---

## 当前执行态

D1 真板异步 UART 测试已正式结束：Q19/Q19B 已完成并归档，Q19C 已完成 D1 async UART 验证并全部归档（`q19c-m2-m3-acceptance-alignment` → `q19c-lichee-full-starryos-benchmark` → `q19c-async-uart-closeout`）。M3/rootfs-probe 代码已删除，历史证据保留在 `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/`；Q19D SDMMC/rootfs 取消当前规划。Q17 已完成 QEMU gate，活跃 change 仅剩 `q17-smp-memory-ordering`。当前主线改为 Q20~Q23：先做 QEMU+D1 可验证的测试补齐和用户态 ring/completion 原型；VisionFive2 或等价多 hart O63 复验后置为 Q24。

<!-- tombstone: Q0-Q15 sub-tasks --> Archived 2026-06-23 — all sub-tasks and verification evidence from Q0 through Q15 collapsed into milestone summary above. Full details preserved in openspec/archive/ and git history.
<!-- tombstone: tasks-final-status/key-experience --> Archived 2026-07-03 in ARC-202607031929 — `最终状态` 与 `关键经验` 长历史已压缩归档，active tasks 只保留 milestone 表和当前/后续任务。

### Q16: Roadmap / spec rebaseline ✅ (2026-06-27)

<!-- Q16.1 --> - [x] 修正 `openspec/project.md` 当前分支（2026-07-02 已同步为 `uart-16550-lichee`）
<!-- Q16.2 --> - [x] 生成 `.claude/analysis/optimization-milestone-replan.md`
<!-- Q16.3 --> - [x] 将 `.claude/docs/tasks.md` 从 Q6 单桶改为原 Q16~Q22 roadmap
<!-- Q16.4 --> - [x] 更新 `openspec/specs/optimization/spec.md`，把 O63/O64/O66/O3/O40/O41/O48 等按 Gate 类型分流
<!-- Q16.5 --> - [x] 标注或修订 stale capability specs（`async-uart-traits` / `arceos-adapter`）
<!-- Q16.6 --> - [x] Gate Q16: roadmap 与分析文档一致；`openspec validate --specs` 的已知 parser 噪音不阻塞后续开发

### Q17: SMP / 内存序正确性 ✅ QEMU 修复完成 / ⚠️ 多 hart stress 待验证

<!-- Q17.1 --> - [x] O63-P0: 修复 `ArceOsUartPort::update_ier()` 的 `ier_cache` RMW 竞争，cache RMW 与 MMIO IER 写入同锁保护
<!-- Q17.1a --> - [x] Q17 当前分支补充：D1 `ArceOsD1UartPort::update_ier()` 同形态 RMW 已纳入 IRQ-off 临界区；软件 wake 在 IRQ 恢复后执行
<!-- Q17.2 --> - [x] O63-P1: `tx_copier_active` 改为 Release/Acquire 语义
<!-- Q17.3 --> - [x] O63-P1: `tx_staged_bytes` 改为 AcqRel/Acquire 语义
<!-- Q17.4 --> - [x] 评估 QEMU SMP 配置是否可作为真板前预检：当前 QEMU 默认单 hart，只能作为功能/性能回归；多 hart stress 仍需后置
<!-- Q17.5 --> - [x] Gate Q17: current-state witness、cargo check、QEMU benchmark 已完成；`cargo test/clippy` 的既有阻塞已记录为非 Q17 回归
<!-- Q17.6 --> - [ ] Deferred: VisionFive2 或等价多 hart 环境复验 UART 并发读写、flush/tcdrain 与 IER enable/disable，无数据丢失或 hang

### Q18: 平台参数解耦 / early console 基础 ✅ (2026-06-28)

> 来源：OpenSpec change `q18-platform-descriptor-early-console`（已归档 `openspec/changes/archive/2026-06-28-q18-platform-descriptor-early-console/`），`.claude/analysis/platform-parameter-decoupling.md`，ADR-044，learned L217-L220。

<!-- tombstone: Q18.1-Q18.6 --> Archived 2026-07-02 in ARC-202607021648 — Q18 子任务已收敛为摘要；完整任务见 Q18 archived change 和 carrier spec。
<!-- Q18.summary --> - [x] 完成 platform descriptor + early console 分层，QEMU 行为保持，板级 base/irq/stride/width 不再散落在驱动初始化路径。

### Q19: Lichee RV Dock D1 axplat bring-up ✅ 真板 smoke complete (2026-06-29)

> 来源：OpenSpec change `q19-lichee-d1-early-smoke`，`.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/d1-axplat-bringup-plan.md`、`.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md`。
> 2026-06-29：D1 axplat、Android boot image、DW APB UART0 polling early console、C906 PTE 属性和 Lichee smoke feature gate 已通过真板最小验证。串口输出 `platform = riscv64-lichee-d1`、`sbi_version: 0.2`、`[starry-d1] early boot`、`[starry-d1] smoke complete, halting.`。Q19a 到此完成；PLIC/Timer/SDMMC/rootfs/TTY/benchmark 属于后续独立阶段。

<!-- tombstone: Q19.1-Q19.13 --> Archived 2026-07-02 in ARC-202607021648 — Q19 子任务已收敛为摘要；完整任务见 Q19 archived change、`lichee-d1-early-smoke` spec 和 carrier spec。
<!-- Q19.summary --> - [x] 完成 D1 axplat、Android boot image、DW APB UART0 polling early console、C906 PTE 属性、feature gate 与真板 smoke 输出 `[starry-d1] smoke complete, halting.`

### Q19C/Q19D: D1 async UART board benchmark ✅ 正式结束

> Q19C-M0/M1/M2 已在 D1 真板通过，D1 异步 UART 测试正式结束。M2 以 `lichee-memory-root-command` 作为完成目标，true shell deferred；M3/rootfs-probe、Q19D SDMMC/rootfs 与 real rootfs 不再是当前 gate。逐项历史已归档到 ARC-202607111510。

### Q20: Benchmark gap closure ⏳ 待做

> 来源：ADR-056、R15 `.claude/analysis/uart-async-qemu-d1-first-replan.md`、L286。目标是先补齐 QEMU+D1 当前可验证的指标，不改驱动语义。

<!-- Q20.1 --> - [ ] 增加 jitter summary：S10/S14/S21 输出 `p99/p50`、`max/p50`、`slow_over_line_plus10ms`
<!-- Q20.2 --> - [ ] 增加 CPU/counter summary：输出 IRQ count、TX poll、no-progress、hw_send_zero、bytes/call
<!-- Q20.3 --> - [ ] 启用 RX fixed payload：`BENCH_RX_FIXED_BYTES > 0` 时 QEMU 与 D1 都有 PASS 样本
<!-- Q20.4 --> - [ ] 保存 raw evidence：QEMU rootfs log 与 D1 serial log 分开归档
<!-- Q20.5 --> - [ ] Gate Q20: QEMU+D1 输出 latency、jitter、CPU/IRQ/copy 开销与 RX fixed payload 证据

### Q21: UART user completion queue MVP ⏳ 待做

> 只做 UART 专用 completion queue，不先实现完整 `io_uring`。MVP 先覆盖 TX。

<!-- Q21.1 --> - [ ] 定义 submit ring / completion ring entry：request id、opcode、buffer、len、status、bytes、timestamp
<!-- Q21.2 --> - [ ] 实现 doorbell：用 `ioctl` 或轻量 syscall 通知 kernel 消费 submit ring
<!-- Q21.3 --> - [ ] 实现 TX completion：completion 不弱于 write + `tcdrain` 语义，仍以 `TxCompletion` 四阶段为完成依据
<!-- Q21.4 --> - [ ] 接入 wait path：复用 poll/select 或现有 waker，禁止新增 busy loop
<!-- Q21.5 --> - [ ] Gate Q21: 用户态能提交 N 个 TX request 并读取 completion；read/write fallback 不退化

### Q22: User ring + zero-copy prototype ⏳ 待做

> 基于现有 `DeviceOps::mmap()` / `sys_mmap()` 路线做 UART 专用 user ring 原型。

<!-- Q22.1 --> - [ ] 设计 mmap ring 所有权：head/tail、wrap、full/empty、用户/内核写权限
<!-- Q22.2 --> - [ ] 实现 TX shared ring：用户写 descriptor/payload，kernel copier 消费
<!-- Q22.3 --> - [ ] 实现 RX shared ring：kernel 写 payload，用户读；防止覆盖未消费数据
<!-- Q22.4 --> - [ ] 实现 completion ring release/acquire 语义，记录 status/bytes/timestamp
<!-- Q22.5 --> - [ ] 保留 fallback：mmap 不可用时回到 read/write
<!-- Q22.6 --> - [ ] Gate Q22: `mmap` ring 在 QEMU+D1 可用，并明确减少了哪些 user/kernel 拷贝

### Q23: Ring/completion performance decision ⏳ 待做

> Q23 是保留/收窄/回滚决策，不继续堆功能。

<!-- Q23.1 --> - [ ] 建立对照矩阵：write+tcdrain、no-drain enqueue、batch-drain、writev、user ring+completion
<!-- Q23.2 --> - [ ] 正确性对照：FIONBIO、short write、tcdrain、FIFO boundary 均不退化
<!-- Q23.3 --> - [ ] 延迟对照：1B 与 15/16/17B 不退化，P99 长尾有解释
<!-- Q23.4 --> - [ ] CPU 对照：poll/send/IRQ/copy counters 不明显恶化
<!-- Q23.5 --> - [ ] Gate Q23: 决定 ring/completion 保留、收窄或回滚；若收益只来自 batch-drain，则只保留 batch-drain

### Q24: VisionFive2 / multi-hart revalidation ⏳ 等待硬件

<!-- Q24.1 --> - [ ] O66 `print_preserved_status()`：UART / PLIC / Clock 状态 dump
<!-- Q24.2 --> - [ ] O64 trust-u-boot 脚手架：明确 PLIC/Clock 只观察或最小补丁，UART 可正常 re-init
<!-- Q24.3 --> - [ ] O65 PLIC init_primary/init_percpu 防御性验证
<!-- Q24.4 --> - [ ] O71 PAC 类型安全寄存器访问评估（只做决策，不强行引入依赖）
<!-- Q24.5 --> - [ ] O38 VisionFive2 UART 时钟适配
<!-- Q24.6 --> - [ ] O39 真实硬件 FIFO 深度验证
<!-- Q24.7 --> - [ ] O63 multi-hart stress：并发 read/write、flush/tcdrain、IER enable/disable 无数据丢失或 hang
<!-- Q24.8 --> - [ ] 真板复跑 Q15 Manual QA：1B latency / 64B TX / FIONBIO / tcdrain / Shell 交互
<!-- Q24.9 --> - [ ] Gate Q24: VisionFive2 或等价 SMP 环境串口稳定运行，multi-hart 风险有实测结论

### Q25: DMA / 高波特率决策 ⏳ 等待数据

<!-- Q25.1 --> - [ ] O3/O40/O69 DMA 决策树：JH7110 DMA 控制器是否存在、是否可达 UART FIFO、PIO vs DMA ROI
<!-- Q25.2 --> - [ ] O41 高速波特率支持（230400+），仅在 Q23/Q24 数据证明需要后实施
<!-- Q25.3 --> - [ ] Gate Q25: 用 Q23/Q24 数据决定实施 / 拒绝 DMA 与高波特率扩展

### Q26: 维护性清理 ⏳ 待做

<!-- Q26.1 --> - [ ] O48 memtrack 是否集成：调试需要则启用，否则记录保留/移除决策
<!-- Q26.2 --> - [ ] O49 `ProcessMode::Manual` 移除评估
<!-- Q26.3 --> - [ ] O50 预留接口评估（超过 90 天未用则移除或留明确注释）
<!-- Q26.4 --> - [ ] ADR-034 发布前 LTO 检查：开发期不启用，release 前恢复
<!-- Q26.5 --> - [ ] Gate Q26: 维护性债务有明确处理结论，不阻塞 Q20~Q25

<!-- tombstone: Q8-Q11 archive pointers --> Archived 2026-07-02 in ARC-202607021648 — Q8~Q11 已在 Milestone 表与 archive 目录中可定位，删除重复小节。

<!-- arc: ARC-202607021648 --> Q18/Q19 详细任务与 Q8-Q11 重复指针已归档/压缩 (2026-07-02) → ../../openspec/changes/archive/2026-07-02-ARC-202607021648/proposal.md
<!-- arc: ARC-202607031929 --> tasks `最终状态` / `关键经验` 历史小节已压缩归档 (2026-07-03) → ../../openspec/changes/archive/2026-07-03-ARC-202607031929/proposal.md
<!-- arc: ARC-202607111510 --> Q19C/Q19D 逐项收尾任务已归档 (2026-07-11) → ../../openspec/changes/archive/2026-07-11-ARC-202607111510/proposal.md
