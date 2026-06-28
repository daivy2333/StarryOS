# tasks.md — 任务追踪

> 由 assistant 维护，feat/uart-16550-async 分支。
> 2026-06-25 Q15 M0~M4 增量重融合 + Manual QA 全部完成（QEMU benchmark 验证无 64B write+tcdrain 退化）。
> 2026-06-28 基于 Lichee RV Dock 与 platform-parameter-decoupling 探索结果，roadmap 二次重排：Q17 不动，新增 Q18 平台参数解耦、Q19 荔枝派 early smoke test，原 VisionFive2/DMA/维护阶段顺延为 Q20~Q23。
> 2026-06-27 Q15 后 roadmap 首次重排：单一 Q6 拆分为原 Q16~Q22，按 Gate 类型分层推进（见 `.claude/analysis/optimization-milestone-replan.md`）。
> 2026-06-21 M4 Sync 已回退到 pre-M4 基线（04f8920/60c5729），原代码保留在 feat/uart-16550-async-temp。
> 2026-06-21 Q15 开启：从 pre-M4 基线增量重融合 M4+ 正确性修复，每步 Manual QA 验证无退化。
> 2026-06-19 OS trait 清理：ADR-036 删除未使用的 OsIrq/OsMmio/OsSpinNoIrq（5→2 trait），消除 3 个 dead_code warning。
> 2026-06-16 Q13 完成：异步串口完整提取到 uart_16550（9 commits, Phase 1 trait 提取 + Phase 2-3 核心逻辑迁移 + 适配层）。
> 2026-06-03 P0 完成，OpenSpec 文档体系建立（核心 5 域 + 后续 capability specs）。
> 条目格式: <!-- Q{编号} --> 或 <!-- P{编号} --> 标记开头，支持 grep 精确定位。

---

## 当前: 方向 C — kernel 层独立实现（feat/uart-16550-async）

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
| **Q17** | SMP / 内存序正确性 | O63：ier_cache RMW + tx completion 原子序 | ⏳ 待做 |
| **Q18** | 平台参数解耦 / early console 基础 | platform descriptor + QEMU 行为保持 + early console 抽象 | ⏳ 待做 |
| **Q19** | Lichee RV Dock early smoke test | Android boot image + D1 platform skeleton + UART0 polling 输出 | ⏳ 待做 |
| **Q20** | VisionFive2 UART 验证 | O66/O64/O65/O71 + O38/O39 + Q15 Manual QA 真板复跑 | ⏳ 等待硬件 |
| **Q21** | DMA / 高波特率决策 | O3/O40/O69 + O41，依赖 Q20 数据 | ⏳ 等待硬件数据 |
| **Q22** | 维护性清理 | O48/O49/O50 + release LTO 检查 | ⏳ 待做 |
| **Q23** | 远期预研池 | O1/O36、O54/O55、O58/O59、O37 | 🧊 按数据触发 |

---

## 最终状态

```
Q0 ✅ Q1 ✅ Q2 ✅ Q3 ✅ Q4 ✅ Q5 ✅ Q5.1 ✅ Q5.2 ✅ Q7 ✅ P0 ✅ Q8 ✅ Q10 ✅ Q9 ✅ Q11 ✅ Q12 ✅ Q13 ✅ Q13-cleanup ✅ LTO ✅ M4 Sync ⟲ Q15 ✅ (2026-06-25 M0~M4 + Manual QA 全部完成) Q16 ✅ → Q17 ⏳ → Q18 ⏳ → Q19 ⏳(Lichee) → Q20 ⏳(VisionFive2 硬件) → Q21 ⏳(硬件数据) → Q22 ⏳ → Q23 🧊

> 2026-06-21 M4 Sync 已回退到 pre-M4 基线 (04f8920/60c5729)，原代码保留在 temp 分支
> 2026-06-21 Q15: M4+ 增量重融合，每步 Manual QA
> 2026-06-25 Q15 完成: M0~M4 全部 commit 落地 + QEMU Manual QA 验证无退化
> 2026-06-27 Roadmap 首次重排: Q6 单一真板桶拆为原 Q16~Q22，Q16 文档/规格收敛完成，下一站 Q17 内存序修复
> 2026-06-28 Roadmap 二次重排: 新增 Q18 平台参数解耦和 Q19 Lichee RV Dock smoke test，VisionFive2 阶段顺延到 Q20
```

**2026-06-11 阶段重规划**：基于 4 个并行 agent 的优化审计（`.claude/analysis/optimization-opportunity-audit.md`），将原有 Q8（仅 O46）扩展为驱动引擎打磨（含 3 项正确性修复 + 热路径优化 + O46），新增 Q10（数据路径优化）和 Q11（内核通用优化）。

**已实现**: kernel 层独立异步串口栈，不修改任何外部 crate（axplat/axhal/axtask）。
- Shell stdin: ISR → RX copier → ring buffer → AsyncUartReader → Tty → Shell
- Shell stdout: Shell → Tty → AsyncUartWriter → ring buffer → TX copier → UART
- 内核日志: ax_println! → Console polling TX（共存）
- /dev/async_uart: DeviceOps + Pollable，用户态可 open/read/write/poll
- 性能优化: IER 缓存、ISR 合并、批量 I/O、rx/tx 独立锁、waker skip、NAPI 中断合并、批量 API
- 性能测试: Console vs Async 统一数据量对比，Async CPU 效率高 14.3 倍
- 性能分析: 完成用户态异步效率低下的根因分析（5 层瓶颈），FIONBIO 未传播的详细诊断

<!-- tombstone: Q0-Q15 sub-tasks --> Archived 2026-06-23 — all sub-tasks and verification evidence from Q0 through Q15 collapsed into milestone summary above. Full details preserved in openspec/archive/ and git history.

### Q16: Roadmap / spec rebaseline ✅ (2026-06-27)

<!-- Q16.1 --> - [x] 修正 `openspec/project.md` 当前分支为 `feat/uart-16550-async`
<!-- Q16.2 --> - [x] 生成 `.claude/analysis/optimization-milestone-replan.md`
<!-- Q16.3 --> - [x] 将 `.claude/docs/tasks.md` 从 Q6 单桶改为原 Q16~Q22 roadmap
<!-- Q16.4 --> - [x] 更新 `openspec/specs/optimization/spec.md`，把 O63/O64/O66/O3/O40/O41/O48 等按 Gate 类型分流
<!-- Q16.5 --> - [x] 标注或修订 stale capability specs（`async-uart-traits` / `arceos-adapter`）
<!-- Q16.6 --> - [x] Gate Q16: roadmap 与分析文档一致；`openspec validate --specs` 的已知 parser 噪音不阻塞后续开发

### Q17: SMP / 内存序正确性 ⏳ 待做

<!-- Q17.1 --> - [ ] O63-P0: 修复 `ArceOsUartPort::update_ier()` 的 `ier_cache` RMW 竞争
<!-- Q17.2 --> - [ ] O63-P1: `tx_copier_active` 改为 Release/Acquire 语义
<!-- Q17.3 --> - [ ] O63-P1: `tx_staged_bytes` 改为 AcqRel/Acquire 语义
<!-- Q17.4 --> - [ ] 评估 QEMU SMP 配置是否可作为真板前预检
<!-- Q17.5 --> - [ ] Gate Q17: cargo check + QEMU benchmark 无性能退化；真板到位后复验 SMP stress

### Q18: 平台参数解耦 / early console 基础 ⏳ 待做

> 来源：OpenSpec change `q18-platform-descriptor-early-console`，`.claude/analysis/platform-parameter-decoupling.md`，ADR-044，learned L217-L220。

<!-- Q18.1 --> - [ ] 新增 StarryOS platform descriptor 或等价集中配置，表达 `name / memory / kernel / console / interrupt / timer / boot`
<!-- Q18.2 --> - [ ] 将 QEMU UART facts 从 `kernel/src/drivers/uart_init.rs` 抽出到 QEMU descriptor，QEMU 行为保持不变
<!-- Q18.3 --> - [ ] 新增 early console 抽象：不依赖 ring buffer / async task / IRQ / PLIC / rootfs
<!-- Q18.4 --> - [ ] 实现 `Ns16550U8EarlyConsole` 作为 QEMU baseline，确认 `make ARCH=riscv64 build` 不退化
<!-- Q18.5 --> - [ ] 设计 `DwApbUart32EarlyConsole` 接口，但不要求 Q18 阶段真板启动
<!-- Q18.6 --> - [ ] Gate Q18: QEMU 构建和启动行为保持；驱动初始化路径不再新增板级 base/irq/stride/width 常量

### Q19: Lichee RV Dock early smoke test ⏳ 待做

> 来源：`.claude/analysis/lichee-rv-dock-adaptation-plan.md`、`.claude/analysis/lichee/public-platform-notes.md`。

<!-- Q19.1 --> - [ ] boot image 工具链：备份/解析官方 Android boot image，复现 `page_size=2048`、`kernel_addr=0x40200000`
<!-- Q19.2 --> - [ ] 新增 Lichee RV Dock / Allwinner D1 descriptor：RAM `0x40000000+512MiB`，kernel `0x40200000`，UART0 `0x02500000`，IRQ 18，stride 4，width 32
<!-- Q19.3 --> - [ ] 生成 D1 最小平台骨架和链接配置，构建产物不依赖 rootfs / USB / SDMMC / async TTY
<!-- Q19.4 --> - [ ] 实现 D1 UART0 polling early console，串口输出 `[starry-d1] early boot`
<!-- Q19.5 --> - [ ] early console 成功后打印 hart id / SBI version / timebase，timer 初期优先 SBI
<!-- Q19.6 --> - [ ] Gate Q19: Lichee RV Dock 串口看到 smoke test 输出；失败排查限定在 boot image、link/load 地址、UART base/stride/access width

### Q20: VisionFive2 UART 验证 ⏳ 等待硬件

<!-- Q20.1 --> - [ ] O66 `print_preserved_status()`：UART / PLIC / Clock 状态 dump
<!-- Q20.2 --> - [ ] O64 trust-u-boot 脚手架：明确 PLIC/Clock 只观察或最小补丁，UART 可正常 re-init
<!-- Q20.3 --> - [ ] O65 PLIC init_primary/init_percpu 防御性验证
<!-- Q20.4 --> - [ ] O71 PAC 类型安全寄存器访问评估（只做决策，不强行引入依赖）
<!-- Q20.5 --> - [ ] O38 VisionFive2 UART 时钟适配
<!-- Q20.6 --> - [ ] O39 真实硬件 FIFO 深度验证
<!-- Q20.7 --> - [ ] 真板复跑 Q15 Manual QA：1B latency / 64B TX / FIONBIO / tcdrain / Shell 交互
<!-- Q20.8 --> - [ ] 更新 `SNAPSHOT.md` / `optimization/spec.md` 真板数据，明确 QEMU vs 真板可信度
<!-- Q20.9 --> - [ ] Gate Q20: VisionFive2 串口稳定运行，真板基线数据落档

### Q21: DMA / 高波特率决策 ⏳ 等待硬件数据

<!-- Q21.1 --> - [ ] O3/O40/O69 DMA 决策树：JH7110 DMA 控制器是否存在、是否可达 UART FIFO、PIO vs DMA ROI
<!-- Q21.2 --> - [ ] O41 高速波特率支持（230400+），仅在 Q20 稳定后实施
<!-- Q21.3 --> - [ ] Gate Q21: 用真板数据决定实施 / 拒绝 DMA 与高波特率扩展

### Q22: 维护性清理 ⏳ 待做

<!-- Q22.1 --> - [ ] O48 memtrack 是否集成：Q20/Q21 调试需要则启用，否则记录保留/移除决策
<!-- Q22.2 --> - [ ] O49 `ProcessMode::Manual` 移除评估
<!-- Q22.3 --> - [ ] O50 预留接口评估（超过 90 天未用则移除或留明确注释）
<!-- Q22.4 --> - [ ] ADR-034 发布前 LTO 检查：开发期不启用，release 前恢复
<!-- Q22.5 --> - [ ] Gate Q22: 维护性债务有明确处理结论，不阻塞 Q17~Q21

### Q23: 远期预研池 🧊 按数据触发

<!-- Q23.1 --> - [ ] O1/O36 零拷贝 RX：仅当 Q20 证明 RX 拷贝是瓶颈时启动
<!-- Q23.2 --> - [ ] O54 ISR 直接搬运：需重新评估 ISR 延迟与极简原则冲突
<!-- Q23.3 --> - [ ] O55 半满/IDLE 唤醒：当前 NAPI 已覆盖相近收益，低优先级
<!-- Q23.4 --> - [ ] O58 ArceOS feature gate 特化：仅当性能不达标且可接受可移植性损失时启动
<!-- Q23.5 --> - [ ] O59 MaybeUninit ring buffer：unsafe 成本高，需单独安全分析
<!-- Q23.6 --> - [ ] O37 kernel log TX 合并：外部 crate 约束强，收益低

### Q8: 驱动引擎打磨 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q8-driver-polish/`

### Q9: 超时机制 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q9-timeout/`

### Q10: 数据路径优化 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q10-data-path-optimize/`

### Q11: 内核通用优化 ✅ (2026-06-11) → 已归档 `openspec/changes/archive/2026-06-11-q11-kernel-optimize/`

---

## 关键经验

### 已验证的模式

1. Ring Buffer + 中断 + copier 任务模型 ✅
2. DeviceOps + 设备注册 + poll/epoll 支持 ✅
3. uart_16550 本地 path 依赖 + embassy-sync 集成 ✅
4. Tty<R,W> 泛型绑定：实现 reader/writer trait 即可替换终端栈 ✅
5. NAPI 中断合并：连续成功 ≥16 次后切轮询模式，高吞吐时减少 90%+ IRQ ✅
6. 批量 API：receive_bytes/send_bytes 替代逐字节操作 ✅
7. TX interleave 修复：本地 cursor 追踪已发位置，避免与 ax_println! 输出交错 ✅
8. AtomicWaker 直接唤醒：ISR 中 O(1) 唤醒，无需 BTreeMap 分发（O17 不需要） ✅
9. Console 组件清理：删除 ntty.rs + ConsoleWriter，ASYNC_TTY 成为唯一串口实现 ✅

### 已解决的问题（Q7 修复）

1. ~~三重 yield storm~~ → Q7 O42 修复（Manual→External）
2. ~~Manual 模式缺陷~~ → Q7 O42 修复
3. ~~Benchmark 不测 UART~~ → Q7 O44 修正
4. ~~FIONBIO 不传播到 TTY~~ → Q7 O43 修复

### 新发现的待解决问题（2026-06-11 审计）

1. **NAPI 模式永不退出** — consecutive 在 NAPI 模式只增不减，零字节时无重置 → Q8.1
2. **ISR 获取 SpinNoIrq 锁** — 违反 ISR 极简原则 → Q8.2
3. **IER 裸 write_volatile** — 绕过 uart_16550 API → Q8.3
4. **读路径 5 次拷贝** — UART FIFO→copier→driver→InputReader→ldisc→user → Q10
5. **ldisc 锁跨 async wait 持有** — 阻塞并发 poll/select → Q10.3
6. **copier waker 去重过度** — 每 poll 周期 2 次 Waker::clone() → Q8.4
7. **PollSet→AtomicWaker** — pipe/signalfd/pidfd/event 共 8 个 PollSet 替换 → Q8.6~9

### 已修正的误判

1. **LoadFault 根因**: stride=4 越界，非"MMIO 权限阻塞"
2. **Console 能访问的原因**: 页表映射正常（mmio-ranges 中），非"初始化时机"
3. **无需修改 axplat**: kernel 层独立实现完全可行
4. **copier/Console 竞争**: RX copier 不能与 Console tty-reader 共用 FIFO

### 方向 A M3 的真正失败原因

IRQ 风暴 + TX busy-loop — Console + AsyncUart 共享 UART 时的 IER 冲突和 stride=4 错误

### 新发现的架构问题（2026-06-01 性能分析）— 全部已解决

> 💡 以下问题已于 Q7~Q11 全部修复，保留为历史参考。

1. **用户态性能上限是波特率**：115200 bps = 11.52 KB/s（硬件约束，非软件问题）
2. ~~**Async RX 多一次拷贝**~~ → Q10 合并 C3/C4 拷贝
3. ~~**ProcessMode::Manual yield storm**~~ → Q7 O42 External 模式修复
4. ~~**FIONBIO 对 TTY 不生效**~~ → Q7 O43 三入口传播
5. ~~**benchmark.c 不测真实 UART 吞吐量**~~ → Q7 O44 修正
