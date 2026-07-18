# tasks.md — 任务追踪

> 由 assistant 维护，uart-16550-lichee 分支（已同步 origin，领先 0 commits）。
> 当前主线（2026-07-18）：Q27 TX backpressure、Q28 writer 契约与 Q29 reader 契约均已完成并归档；Q24 SMP 复验仍等待硬件；Q30 TX 多 producer 语义保持证据触发。
> 已完成边界：Q15 Manual QA、Q17 QEMU 修复、Q18 platform descriptor、Q19/Q19B/Q19C D1 真板异步 UART 验证均已完成；Q19D SDMMC/rootfs、M3/rootfs-probe 取消当前规划。
> 归档入口：Q0~Q15、Q18/Q19、Q19C 逐项证据分别见 ARC-202607021648、ARC-202607031929、ARC-202607111510 及 `.claude/analysis/_archive/`。
> 条目格式: `<!-- Q{编号} -->` 或 `<!-- P{编号} -->`，支持 grep 精确定位。

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
| **Q20** | Benchmark gap closure | QEMU+D1 TX latency / jitter / CPU proxy 补测，RX fixed payload 明确排除 | ✅ |
| **Q21** | UART user completion queue MVP | 已有 TX ring + copier + TxCompletion 覆盖主要思想；真板线速限制下收益不足 | 🧊 取消当前规划 |
| **Q22** | User ring + zero-copy prototype | `mmap` ring / zero-copy 原型复杂度高，当前 D1 115200 bps 无可见吞吐收益 | 🧊 取消当前规划 |
| **Q23** | Ring/completion performance decision | 基于 Q20 数据决策：不实施 Q21/Q22；保留现有 batch/writev/tx counter 路径 | ✅ 决策完成 |
| **Q27a** | uart_16550 readiness 薄接口 | O83 前置：RX/TX ring 状态观测 + readable/writable waker 注册，不引入 OS 语义 | ✅ (2026-07-15) |
| **Q27** | TX backpressure / writable wait MVP | O83：基于 Q27a，阻塞 fd 等待 TX ring 空间，非阻塞保持 partial/WouldBlock | ✅ 已归档 `2026-07-15-q27-tx-backpressure` |
| **Q28** | AsyncUartWriter writer 契约收敛 | O84：`Clone` 与 `RingBufTx` SPSC 安全边界对齐；MPSC 后置 O85 | ✅ 已归档 `2026-07-15-q28-async-uart-writer-contract` |
| **Q29** | AsyncUartReader consumer 契约审计 | O87：unsafe unique raw reader + crate-private RX mutation + 单次 copier 启动 | ✅ 已归档 `2026-07-18-q29-async-uart-reader-contract` |
| **Q30** | TX 多 producer 语义决策 | O85/O86：syscall 原子性、公平性、跨 write 交错与 MPSC ROI | 🧊 等待真实 workload / Q24 证据 |
| **Q24** | VisionFive2 / multi-hart revalidation | O63/O64/O65/O66/O71/O38/O39 + Q15 Manual QA 真板复跑 | ⏳ 等待硬件 |
| **Q25** | DMA / 高波特率决策 | O3/O40/O69 + O41，依赖 Q24 或新硬件数据 | ⏳ 等待数据 |
| **Q26** | 维护性清理 | O48/O49/O50 + release LTO 检查 | ⏳ 待做 |

---

## 当前执行态

Q19/Q19B/Q19C、Q27a/Q27/Q28/Q29 已完成；Q17 multi-hart 后置 Q24。Q29 已将 raw reader、RX mutation 与 copier startup 收敛到显式唯一性边界，并通过 API/字节完整性/readiness、QEMU 和 D1 单 hart Gate；Q30 TX 原子性/公平性/MPSC 仍由新证据触发。


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

### Q20: Benchmark gap closure ✅ (2026-07-13)

> 来源：ADR-056（A056，已归档于 arc-202607152005）、ADR-057、R15/R16、L286/L287、`.claude/analysis/q20-evidence/`。目标是先补齐 QEMU+D1 当前可验证的 TX latency/jitter/counter 指标，不改驱动语义。RX fixed payload 经用户确认排除在 Q20 scope 外。

<!-- Q20.1 --> - [x] 增加 jitter summary：S10/S14/S20/S21 输出 `p99_p50_ratio`、`max_p50_ratio`、`slow_over_line_plus10ms`
<!-- Q20.2 --> - [x] 增加 CPU/counter proxy summary：S40 输出 user/ring/hw/no-progress/drain counters；D1 输出有效派生 proxy，QEMU 明确 not-available
<!-- Q20.3 --> - [x] RX fixed payload scope 决策：经用户确认不做；S31 保持 `SKIPPED reason=BENCH_RX_FIXED_BYTES=0`
<!-- Q20.4 --> - [x] 保存 raw evidence：QEMU rootfs log 与 D1 serial log 分别归档到 `.claude/analysis/q20-evidence/`
<!-- Q20.5 --> - [x] Gate Q20: QEMU+D1 输出 TX latency、jitter、counter proxy 证据；D1 fullbench command-entry 正常退出，Q20 不声明 SMP 正确性

### Q21/Q22/Q23: user ring / completion queue 路线取消当前规划 ✅ 决策完成（2026-07-13）

> Q20 证明 D1 TX 达 95.2%-99.1% 线速；现有 TX ring+copier+`TxCompletion` 已覆盖提交/执行分离，115200 bps 下 Q21 CQ 与 Q22 `mmap` ring/zero-copy 无可见收益。

<!-- Q21.1 --> - [x] 决策：不实施 UART user completion queue MVP；保留当前 `write()` / `writev()` / `tcdrain()` / `TxCompletion` 路径
<!-- Q22.1 --> - [x] 决策：不实施 `mmap` user ring / zero-copy prototype；O1/O36 保留为远期候选，不进入当前 roadmap
<!-- Q23.1 --> - [x] 决策输入：Q20 QEMU+D1 TX jitter/counter 证据、D1 线速数据、S40 fallback 未耗尽
<!-- Q23.2 --> - [x] 决策结果：user ring/completion 路线取消当前规划；可借鉴优化记录到 `openspec/specs/optimization/spec.md` O82

### Q27a: uart_16550 readiness 薄接口 ✅ (2026-07-15)

> 来源：R19/ADR-061/L295/O83；目标：补齐 crate readiness+waker，不下沉 StarryOS VFS/poll/syscall 语义。

<!-- Q27a.1 --> - [x] `RingBufTx` 增加 `vacant_len()` / `has_space()`；`RingBufRx` 增加 `occupied_len()` / `has_data()`
<!-- Q27a.2 --> - [x] `AsyncUartWriter` 增加 `can_write()` / `register_writable_waker()`；`AsyncUartReader` 增加 `can_read()` / `register_readable_waker()`
<!-- Q27a.3 --> - [x] 文档标明 readiness hint 不保证后续 push/pop 成功，OS 层必须 register 后 recheck
<!-- Q27a.4 --> - [x] Gate Q27a: crate fmt/check/test/clippy/rustdoc 通过（59 unit tests + 8 doctests）；用户手动 QEMU `make run` 并确认正常工作与测试

### Q27: TX backpressure / writable wait MVP ✅ (2026-07-15)

> 来源：`q27-tx-backpressure`、R19/ADR-061/L295/O83；目标：基于 Q27a 修复 blocking short write，不重启 user ring/CQ。

<!-- Q27.1 --> - [x] 接入 Q27a writer readiness，UART OUT 绑定 TX ring 空间并注册 writable waker；PTY 保留 always-OUT/short-write
<!-- Q27.2 --> - [x] 阻塞 UART short write 通过 `poll_io(... OUT ...)` 累计完成；非阻塞保持 partial/`WouldBlock`，空写保持 fast path
<!-- Q27.3 --> - [x] ONLCR 以完整源字符边界映射，覆盖 0/1/2B 空间、混合换行、255/256B chunk 与 retry 无重复/丢失
<!-- Q27.4 --> - [x] Gate Q27: 6 个聚焦测试、uart crate 62 tests + 8 doctests、fmt/clippy/kernel build/OpenSpec/QEMU/D1 通过；D1 64B 96.8%、1024B 98.8% 线速且无性能退化

### Q28: AsyncUartWriter writer 契约收敛 ✅ 已归档（2026-07-15）

> 来源：R19/ADR-061/L296/O84、归档 `2026-07-15-q28-async-uart-writer-contract`；目标：writer API 对齐 SPSC，MPSC 非默认。

<!-- Q28.1 --> - [x] 确认 `Tty::new()` 的 direct-output/ldisc-echo clone 与共享 fd 会形成真实多 producer；RX/MPSC 不纳入本 change
<!-- Q28.2 --> - [x] raw `AsyncUartWriter` 移除 `Clone`/共享 `TtyWrite`，改为 unsafe 唯一构造、`&mut self` 提交，`RingBufTx::push` 收窄为 crate-private
<!-- Q28.3 --> - [x] StarryOS 以 `Arc<SpinNoPreempt<RawArceOsWriter>>` 保留 cloneable adapter，锁不跨等待点；SMP feature 显式传播
<!-- Q28.4 --> - [x] Gate Q28：4 compile-fail、Q28 并发 2/2、Q27 回归 6/6、crate/kernel 构建与 OpenSpec 通过；QEMU/D1 单次关键指标均未退化超过 3%

### Q29: AsyncUartReader consumer 契约审计 ✅ 已归档（2026-07-18）

> 来源：Q28 review、ADR-062/L299/O87、归档 `2026-07-18-q29-async-uart-reader-contract`；RX 保持 SPSC，不引入 MPMC。

<!-- Q29.1 --> - [x] 审计 reader 构造/移动、共享 fd、TTY/ldisc 与全部 RX pop；确认唯一 raw reader 移入单 `tty-reader`，共享 fd 只消费 ldisc ring
<!-- Q29.2 --> - [x] `AsyncUartReader::new` 改为 unsafe unique constructor，RX `push`/`push_batch`/`pop` 收窄为 crate-private，copier startup 改为 unsafe 单次启动
<!-- Q29.3 --> - [x] 10 个 compile-fail + RX 空读/partial/wrap-around/字节顺序与 readiness register-recheck witness 通过
<!-- Q29.4 --> - [x] Gate Q29：62 unit + 8 doctest + 10 compile-fail、Clippy/rustdoc/OpenSpec、QEMU build+boot 与 D1 `/dev/console` benchmark 退出码 0；不声明 multi-hart

### Q30: TX 多 producer 语义决策 🧊 证据触发

> 来源：Q28、ADR-062/L299/O85/O86；仅保证每次 accepted prefix 连续，blocking retry 间可插入其他 producer，不保证 syscall 原子性/公平性/跨 write 不交错。

<!-- Q30.1 --> - [ ] 仅当 Q24/新 workload 发现消息边界、饥饿、交互延迟或 producer-lock 吞吐问题时启动 `openspec-plan`
<!-- Q30.2 --> - [ ] 区分 atomicity、fairness、锁竞争与吞吐目标，禁止用 MPSC 一次性代替全部问题
<!-- Q30.3 --> - [ ] 比较 SPSC+串行化、提交粒度、调度队列、MPSC，并量化内存序、公平性、延迟、复杂度
<!-- Q30.4 --> - [ ] Gate Q30：并发 stress 证明目标语义；未满足触发 Gate 时维持当前 accepted-prefix 契约和 O85 远期状态

### Q24: VisionFive2 / multi-hart revalidation ⏳ 等待硬件

<!-- Q24.1 --> - [ ] O66 `print_preserved_status()`：UART / PLIC / Clock 状态 dump
<!-- Q24.2 --> - [ ] O64 trust-u-boot 脚手架：明确 PLIC/Clock 只观察或最小补丁，UART 可正常 re-init
<!-- Q24.3 --> - [ ] O65 PLIC init_primary/init_percpu 防御性验证
<!-- Q24.4 --> - [ ] O71 PAC 类型安全寄存器访问评估（只做决策，不强行引入依赖）
<!-- Q24.5 --> - [ ] O38 VisionFive2 UART 时钟适配
<!-- Q24.6 --> - [ ] O39 真实硬件 FIFO 深度验证
<!-- Q24.7 --> - [ ] O63 multi-hart stress：至少两个 hart 跨 hart 并发 write/flush/tcdrain，并覆盖 read 与 IER enable/disable；验证无数据丢失、重复、staged_bytes 漂移或 hang
<!-- Q24.8 --> - [ ] 真板复跑 Q15 Manual QA：1B latency / 64B TX / FIONBIO / tcdrain / Shell 交互
<!-- Q24.9 --> - [ ] Gate Q24: VisionFive2 或等价 SMP 环境串口稳定运行，multi-hart 风险有实测结论

### Q25: DMA / 高波特率决策 ⏳ 等待数据

<!-- Q25.1 --> - [ ] O3/O40/O69 DMA 决策树：JH7110 DMA 控制器是否存在、是否可达 UART FIFO、PIO vs DMA ROI
<!-- Q25.2 --> - [ ] O41 高速波特率支持（230400+），仅在 Q24 或新硬件数据证明需要后实施
<!-- Q25.3 --> - [ ] Gate Q25: 用 Q24 或新硬件数据决定实施 / 拒绝 DMA 与高波特率扩展

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
