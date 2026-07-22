## Context

当前分支是 `console-lichee`。Q31 在提交
`7d44cb173a7a5e8e0584c28d7976ded1a4d882f7` 上完成了 Async UART benchmark
优化，并冻结了 workload、完成点、时间换算、raw 字段与 QEMU/D1 证据。当前 Console
版本的 `tests/benchmark.c` 较旧，D1 `time.rs` 仍把 24 MHz cycle 按 1 MHz 换算，直接
复用现有输出会造成约 24 倍时间误差，也缺少 S41/S42/S43 的同口径数据。

Console 和 Async 的产品语义不同。Console write 同步执行，可能把整个发送过程包含在调用
窗口内；Console 也没有 Q31 Async 后端的 UART TX 本地计数器。因此可比较性来自相同
workload、完成点、时钟和 raw 公式，而不是强行让所有诊断字段相同。

Q31 iteration 003 的 Plan Review 已确认职责拆分：Q31 只提供不可变的 Async 基线，Q32
负责 Console 代码、Console 证据和最终比较。

## Goals / Non-Goals

**Goals:**

- 修正并测试 D1 24 MHz 时间换算。
- 在 Console benchmark 中实现与 Q31 相同的 S11、S41、S42、S43 workload 和完成点。
- 对 Console 不支持或不成立的字段做显式 capability negotiation。
- 获取可复现的 QEMU 与 D1 Console 证据。
- 生成可追溯到 Q31 固定 hash 的横向比较，不混合 QEMU 与实板结论。

**Non-Goals:**

- 不修改 Console writer、TTY、polling、锁、flush/drain、调度或 UART 驱动语义。
- 不给 Console 补造 Async UART TX 诊断计数器。
- 不把 `/proc/instret` 解释为 CPU utilization。
- 不以单次结果宣布架构胜负，也不设置 Console 必须优于 Async 的门槛。
- 不重新生成或改写已经验收的 Q31 Async 日志。

## Decisions

### D1：以 Q31 实现为移植基线，按 Console 能力做小范围适配

- **Decision**：以 Q31 提交中的 `tests/benchmark.c` 为结构基线，保留 Console backend
  whitelist、通用标题和 Console section 支持矩阵；通过 diff allowlist 审核移植结果。
- **Reason**：直接复用已验证的计算、输出和错误处理能减少两个 harness 的口径漂移。
- **Impact**：`tests/benchmark.c` 会有较大文本变化，但产品 Console 路径不变。
- **Alternatives**：在旧 Console benchmark 上逐段重写；因容易遗漏 raw 字段和错误边界而拒绝。

### D2：移植 Q31 的纯时间换算 helper，并先做 RED/GREEN 测试

- **Decision**：复用 `time_math.rs` 的 24 MHz 整数换算与 12 个边界测试，再让 D1
  `time.rs` 调用该 helper。
- **Reason**：纯函数测试可以在 host 上固定截断、余数、秒进位和溢出边界，无需启动内核。
- **Impact**：只改变 D1 时间报告的正确性；预期会让旧 benchmark 的错误绝对时间发生变化。
- **Alternatives**：在 `time.rs` 内写浮点或散落的除法；因 `no_std`、精度和不可测试性而拒绝。

### D3：比较基线由不可变 hash 标识

- **Decision**：Q32 报告必须引用 Q31 benchmark、`time.rs`、`time_math.rs` 和当前 QEMU/D1
  日志 hash；Q31 evidence 目录只读。
- **Reason**：分支工作树可能 dirty，只写分支名或 HEAD 不能证明参与比较的真实输入。
- **Impact**：任何 Q31 输入变化都会使比较 gate 失败，必须重新说明基线而不能静默覆盖。
- **Alternatives**：复制 Async 日志到 Q32；因会产生双份真相和来源漂移而拒绝。

### D4：同步 Console 的零 overlap 是有效数据

- **Decision**：S42 中发送调用消耗全部窗口时，`overlap_ns=0` 是有效结果；S43 loaded
  timer 没有实际并发窗口时输出 `not-applicable`，且不进入 loaded aggregate。
- **Reason**：零 overlap 描述同步 Console 的真实行为；没有 overlap 时计算 loaded timer
  interference 则没有物理含义。
- **Impact**：Console 与 Async 的字段集合一致，但部分派生量的适用性不同。
- **Alternatives**：给 Console 人为创建后台发送线程；这会改变被测路径和完成点，故拒绝。

### D5：不可用诊断使用 capability state，不使用哨兵零

- **Decision**：Console 不支持的 S40/local TX counters 输出 `not-available`；零值只在测量
  确实执行且零有语义时使用。
- **Reason**：缺失能力与真实计数为零不能混为一谈，否则横向表会产生假结论。
- **Impact**：比较报告按 common fields 与 backend-specific fields 分栏。
- **Alternatives**：修改 Console 产品路径增加计数器；超出本 change 的测量范围，故拒绝。

### D6：Q32 使用独立证据目录

- **Decision**：Console 日志与比较报告写入
  `.claude/analysis/q32-console-cpu-efficiency-evidence/`，目录包含 `console/`、
  `comparison/` 和 provenance README；Q31 路径只作为输入引用。
- **Reason**：新的 change 需要独立生命周期，不能让 Console 采集继续修改 Q31 已验收证据。
- **Impact**：最终报告通过 hash 和相对路径连接两组证据。
- **Alternatives**：继续写 Q31 evidence 的 `console/` 子目录；因职责和归档边界模糊而拒绝。

### D7：QEMU 与 D1 是两个证据层级

- **Decision**：QEMU 用于构建、启动、输出协议、错误路径和 smoke validation；D1 用于真实
  UART timing 与 CPU-work 数据。报告不得把两类数值合并为一个性能结论。
- **Reason**：QEMU 设备模型和 host 调度不能代表真实串口硬件时序。
- **Impact**：两个环境分别保存命令、日志、hash 和 gate 结果。
- **Alternatives**：只跑 QEMU 或将 QEMU 当硬件性能证据；均无法满足横向比较目的。

### D8：比较基于 raw 字段与固定公式，不基于展示字符串

- **Decision**：共同指标只从 completed bytes、elapsed ns、instret delta、timer request/actual、
  round count 等 raw 字段派生；公式、单位、分母和排除规则写入报告。
- **Reason**：展示文字可因 backend 不同而变化，raw contract 才是可复算的比较接口。
- **Impact**：每项汇总都能由日志复算，零分母或无有效样本时拒绝派生。
- **Alternatives**：人工抄录最终 summary；因无法审计和易受格式差异影响而拒绝。

### D9：性能没有预设胜负阈值

- **Decision**：Gate 判断测量完整性与可比性，不判断 Console 或 Async 谁必须更快、更省 CPU。
- **Reason**：本 change 的目标是建立可信数据，不是用未校准门槛证明既定结论。
- **Impact**：任何方向的结果都可验收，只要 provenance、完整性和公式通过。
- **Alternatives**：设置提升百分比；缺少稳定历史分布和硬件重复性依据，故拒绝。

### D10：D1 runtime 启用真实 IRQ，smoke 保留 stub

- **Decision**：新增 D1 runtime IRQ 组合，让 `lichee-d1-userbench`、`lichee-d1-fullbench` 和
  `lichee-d1-fullbench-command` 启用 `axplat-riscv64-lichee-d1/irq`。`lichee-d1-smoke`
  继续只启用 `irq-if`。
- **Reason**：当前 Console runtime 编译了 `irq_stub.rs`。该 stub 的 `register()` 恒为
  `false`，无法安装 supervisor timer handler。Async runtime 因继承 `/irq` 使用
  `irq.rs`，所以 `clock_nanosleep` 能被 timer interrupt 唤醒。
- **Impact**：Console runtime 会初始化现有 D1 PLIC 和 supervisor timer。polling Console
  不注册 UART IRQ handler，write、drain 和数据路径不变。
- **Alternatives**：修改 `clock_nanosleep` 忙等；会掩盖平台 IRQ 缺失并改变调度语义，拒绝。
  让 smoke 也启用完整 IRQ；会破坏最小 bring-up 边界，拒绝。拆出 timer-only IRQ backend；
  当前没有独立需求，改动大于复用已验证的 `irq.rs`，拒绝。

## Requirement Traceability Matrix

| ID | Requirement | Design | Planned verification | Tasks |
|---|---|---|---|---|
| Q32-R1 | D1 时间换算正确 | D2 | 12 个 host 单元测试 + D1 时间 sanity | 2.1–2.4, 6.3 |
| Q32-R2 | workload 与 provenance 固定 | D1, D3 | hash、diff allowlist、manifest | 1.1–1.4, 3.1, 7.1 |
| Q32-R3 | S11 完成点可比较 | D1, D8 | raw write/drain 校验与失败样本排除 | 3.2, 4.1, 5.3, 6.4 |
| Q32-R4 | S41 CPU-work 可复算 | D8 | 五轮 raw/summary 复算 | 3.3, 4.2, 5.3, 6.4 |
| Q32-R5 | S42 overlap 语义正确 | D4, D8 | 零 overlap 边界与五轮复算 | 3.4, 4.3, 5.3, 6.4 |
| Q32-R6 | S43 timer 适用性正确 | D4, D8 | idle/loaded raw 与排除规则 | 3.5, 4.4, 5.3, 6.4 |
| Q32-R7 | 不可用能力不伪造 | D5 | capability-state 静态与运行检查 | 3.6, 4.5, 5.3, 6.4 |
| Q32-R8 | 错误样本不污染汇总 | D1, D8 | fault-path assertions、日志扫描 | 3.7, 4.6, 5.3, 6.4 |
| Q32-R9 | QEMU/D1 证据分层 | D6, D7 | 两套 manifest、日志与 gate | 5.1–5.4, 6.1–6.5 |
| Q32-R10 | 横向比较可信且中立 | D3, D8, D9 | S41/S42 公式复算；S43 reported aggregate 限制待用户批准；无胜负阈值 | 7.1–7.5, 8.1–8.3 |
| Q32-R11 | D1 runtime timer IRQ 可唤醒 | D10 | feature matrix、timer smoke、D1 S43 | 6A.1–6A.7 |

## Risks / Trade-offs

- **[同步 write 让 loaded overlap 不成立]** → 输出 `not-applicable`，保留 idle timer 结果，
  不制造后台并发。
- **[D1 24 MHz 修复改变旧数值]** → 保留修复前 RED witness，并只比较修复后的同口径数据。
- **[restricted shell 阻止 musl compiler]** → 记录 `Bad system call` 证据；Act 阶段在普通 host
  shell 执行构建，不把失败产物当 gate 结果。
- **[工作树已有用户修改]** → 每次写入前检查目标 diff，只修改 Q32 计划列出的文件，不清理
  无关变更。
- **[D1 串口日志受外部负载干扰]** → 固定 image、波特率、轮数和命令，保存 raw rounds，不以
  单个 summary 替代原始证据。
- **[instret 是 hart-wide proxy]** → 报告统一写 CPU-work，不写 CPU utilization，并记录
  hart 与背景负载限制。
- **[启用完整 D1 IRQ 也会初始化 PLIC]** → 只对 runtime modes 启用；smoke 保持 stub；
  验证 polling Console 未注册 UART IRQ，S10/S42 行为不变。

## Migration Plan

1. 固定当前 Console witness 与 Q31 输入 hash，不修改 Q31 evidence。
2. 先完成 `time_math.rs` RED/GREEN，再接入 D1 `time.rs`。
3. 移植 benchmark 并通过静态 contract、host 编译和错误路径检查。
4. 验证 D1 runtime 使用真实 IRQ、smoke 保持 stub，并先通过 timer wake smoke。
5. 生成 Console QEMU 产物与日志，确认协议完整后再制作 D1 image。
6. 在 D1 采集固定轮数，复算每项 summary。
7. 生成 comparison 与 provenance README，执行 strict validate 和 scope review。

若任一 gate 失败，回滚仅限 Q32 对 `tests/benchmark.c` 和 D1 时间文件的变更，保留失败日志
作为 witness；不得改动 Console 产品路径来规避 benchmark 失败。

## Open Questions

Q31 冻结的 S43 日志每组只保存 3/50 个 sample 和最终 percentile，无法从日志独立重算
P50/P95/P99。推荐保留该 aggregate 作为 hash 锚定的 reported metric，并在 comparison 标明
`not-independently-recomputed`；S41/S42 仍从完整 raw rows 复算。此口径属于 Q32-R10 的
受限实现，须由用户批准后才能执行 iteration 001 的 comparison。
