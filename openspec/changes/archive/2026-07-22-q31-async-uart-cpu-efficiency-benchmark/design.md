## Context

Q20 已提供吞吐、延迟、jitter 和累计 TX counter。D1 Async 达到物理线速，但现有数据只能证明提交与完成分离，不能证明释放窗口可用于计算，也不能衡量相同通信量的 CPU work。

当前可复用入口：

| 入口 | 现有能力 |
| --- | --- |
| [benchmark.c](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/tests/benchmark.c) | S10-S40、`CLOCK_MONOTONIC`、write/drain、TX debug ioctl |
| [/proc/instret](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/pseudofs/proc.rs) | 当前 hart 的 64-bit retired instruction counter |
| [clock_nanosleep](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/task/schedule.rs) | `CLOCK_MONOTONIC` 与 `TIMER_ABSTIME` |
| [TX debug ioctl](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/fs/ctl.rs) | reset/snapshot 现有 TX 路径计数 |
| [D1 time.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/axplat-riscv64-lichee-d1/src/time.rs) | 24 MHz tick/ns 换算，现状有整数截断误差 |

`instret` 是 hart-wide counter，不是 task CPU time。D1 是单 hart，区间 delta 会包含 benchmark、syscall、copier 和 IRQ 的总指令活动，也会受无关任务污染。它适合同环境相对比较，不支持 CPU utilization 或跨平台绝对比较。

## Goals / Non-Goals

**Goals**

- 精确修正 D1 time conversion。
- 在同一 benchmark 中测量 caller release、CPU work、useful-work overlap、timer overshoot 和 per-section TX counters。
- 保持 Async 与 Console 测试源码和输出合同一致。
- 先交付 Async 分支实现与证据；Act Review 后再为 Console 分支创建后续 iteration。
- 在覆盖 `docs/*out.md` 前保存其 baseline 副本和 hash。

**Non-Goals**

- CPU accounting、CPU 百分比、RX fixed payload 或 SMP proof。
- UART driver scheduling、IRQ policy、slow-poll、drain 或 ABI 优化。
- 为测量结果设置“必须优于 Console”的阈值。
- 在 `000-initial` 中修改 Console 分支。

## Decisions

**D1：用宽整数频率换算替代 `NANOS_PER_TICK`。**

- 决策：实现 `mul_div_floor(value, multiplier, divisor)`，中间值用 `u128`；超出 `u64` 时饱和。`ticks_to_nanos` 使用 `ticks × 1e9 / frequency`，反向使用 `nanos × frequency / 1e9`。
- 原因：24 MHz 的周期不是整数纳秒。现有 41 ns/tick 会让一秒少 16 ms。
- 影响：D1 所有 wall clock、timer deadline 和吞吐时间口径同时修正。QEMU 不受影响。
- 替代：保留 41 ns/tick 并只用比值。拒绝，因为本 change 包含绝对 latency 和 wakeup overshoot。

**D2：先建立 time conversion RED witness。**

- 决策：测试 24,000,000 ticks→1,000,000,000 ns、反向一秒、零值、边界、单调性和无中间溢出。先观察旧实现不满足一秒精确值，再改实现。
- 原因：时钟修正影响整个平台，必须有独立于 UART 日志的确定性见证。
- 影响：若平台 crate 的 host test 受 RISC-V 依赖阻塞，将纯函数放到可 host-test 的小模块；target `cargo check` 仍为必过 Gate。
- 替代：只从 D1 串口日志推断误差。拒绝，因为 UART 输出不是换算函数的单元测试。

**D3：S41 从 write 开始测到 TEMT 完成。**

- 决策：新增 S41 `TX CPU Work`。每个 payload 在 begin snapshot 后执行完整 write，再 final `tcdrain`，随后读取 end snapshot。
- 原因：只测 enqueue 会漏掉 Async copier 的后台成本；完成区间才能比较相同通信结果。
- 影响：输出 begin/end/delta、逻辑写次数、实际 syscall 次数、completed bytes、instructions/byte 和 instructions/write-call。
- 替代：读取 process CPU clock。拒绝，因为当前 accounting 不能覆盖 copier，也未正确处理抢占。

**D4：保留 raw instret，并单独报告采样开销。**

- 决策：每次读取重新打开 `/proc/instret`，解析单个 `u64`；用相邻读取记录采样开销，但不从主 delta 中扣除。
- 原因：扣除一个噪声估计会制造负值和不可复核修正。两条路径使用同一读取方法，原始 delta 更易审计。
- 影响：比较报告需同时列出 delta 与采样开销，并运行多轮后使用中位数。
- 替代：用估计开销修正每个样本。拒绝，因为误差可能大于短 workload 本身。

**D5：S42 用理论 UART deadline 测量 useful-work overlap。**

- 决策：固定 64 B × 100 workload。先测相同理论线速窗口的 idle 计算迭代，再从 `t0` 写 payload；write 返回后只计算到 `t0 + line_time`，随后 drain。
- 原因：Async 的收益是调用者在串口传输期间继续计算。Console write 若占满窗口，会自然得到零个 overlap iterations。
- 影响：固定计算内核必须无 I/O、无动态分配，并将 accumulator 写入 `volatile` sink。执行一轮预热和至少五轮采样，输出中位数及范围。
- 替代：固定计算次数后比较总时间。拒绝，因为 UART 和计算串行/重叠的解释不如固定窗口明确。

**D6：S43 不依赖 pthread 建立 TX load。**

- 决策：先提交足够覆盖采样窗口的 UART workload，再在理论剩余传输窗口内执行绝对时间睡眠序列。deadline 由计划时间递增，结果先缓存在内存。
- 原因：当前 benchmark 没有 pthread 见证；引入线程会扩大 syscall、调度和兼容性范围。
- 影响：Async 可在 copier drain 期间得到 loaded 样本。Console 若 write 已耗尽窗口，输出 `not-applicable reason=no-overlap-window`，不伪造并发结果。
- 替代：增加 writer thread。后置；只有单进程窗口无法满足需求时才另行规划。

**D7：S40 保持 ABI，新增 workload-local reset/snapshot。**

- 决策：S41/S42/S43 各自在测量前 reset、完成后 snapshot，并用本段 completed bytes 归一化。保留现有全程 S40 汇总。
- 原因：已有 snapshot 字段足以计算调用密度；无需暴露另一组 telemetry counter。
- 影响：Console 不支持 ioctl 时只缺 counter proxy，其他指标继续。
- 替代：扩展 `TxDebugSnapshot` 加 `tx_poll`、IRQ delta。后置，因为本轮主指标不依赖它们。

**D8：输出期间不采样。**

- 决策：每节先 `fflush(stdout)` 与 `tcdrain(STDOUT_FILENO)`，采样值存入固定数组，完成并 drain 后统一打印。
- 原因：stdout 与被测 `/dev/console` 共用 UART，逐样本输出会把日志成本计入 workload。
- 影响：数组大小使用编译期上限，分配失败或样本溢出时该节 FAIL。
- 替代：逐次打印并在报告中估算污染。拒绝，因为无法稳定扣除。

**D9：evidence 与临时 docs 日志分离。**

- 决策：在任何覆盖前复制四份 `docs/*out.md` 到 `.claude/analysis/q31-cpu-efficiency-evidence/baseline/`，README 记录 SHA-256、commit 和来源。新日志按 `async/`、`console/`、`comparison/` 分目录保存。
- 原因：用户会用新日志覆盖 docs 文件；旧数据仍需可追溯。
- 影响：`docs/*out.md` 不删除，也不作为不可变 Gate；evidence 副本才是本 change 的冻结输入。
- 替代：只记录 Git commit。拒绝，因为临时日志可能未进入历史或与工作树版本不同。

**D10：跨分支分两个 iteration。**

- 决策：`000-initial` 只做 baseline 快照、timer 修正、benchmark 实现、Async QEMU/D1 证据。Plan Review 后创建 Console iteration，复用同版测试并完成 comparison。
- 原因：Plan Context 交接后不可改写，且一次 Act 不应在两个分支间切换和混合 diff。
- 影响：Async Act 完成不等于整个 change 完成；Console 与 comparison tasks 保持 pending。
- 替代：当前 iteration 同时修改两分支。拒绝，因为证据与代码来源难以审计。

**D11：Async 诊断缺口以声明收口，不重新采集。**

- 决策：iteration 002 的 QEMU/D1 数据作为 Async 当前证据。README 补充来源 hash、推导公式和诊断限制，不修改 benchmark 或重采集。
- 原因：现有日志已证明固定字节完成、drain 正常、五轮样本有效，并覆盖 S11/S41/S42/S43。缺失项不改变主指标。
- 影响：`completed_bytes` 与 `hw_send_calls_per_kb` 可从同 section 的完成字节和 raw counter 推导。日志不能细分 partial、zero-progress 和 errno，也不能声明 CPU utilization。
- 用户批准：2026-07-21，批准不再补测；同时批准保留 Runbook 和 S41 `line_time × 100` deadline。

## Measurement Contracts

| Section | Window | Primary output | Completion condition |
| --- | --- | --- | --- |
| S11 | write loop、final drain 分段 | submit fraction、producer available | final TEMT drain |
| S41 | write 开始至 final drain | instructions/byte | exact completed bytes + TEMT |
| S42 | theoretical line-time window | overlap efficiency | compute deadline + TEMT |
| S43 | absolute sleep sequence | wakeup overshoot P50/P95/P99/max | all samples or explicit status |
| S40/local diag | section reset→snapshot | calls/KiB、zero/KiB、bytes/call | ioctl success or not-available |

本 change 不定义性能改善阈值。Gate 检查测量是否有效、可比较、可复现，以及既有正确性与线速是否退化；数据优劣写入 comparison，不改变测试 PASS 条件。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
| --- | --- | ---: | --- | --- |
| R1 D1 frequency-accurate time conversion | 1.2, 2.1-2.4, 6.1-6.2, 8.1, 9.3 | 100% | None | Covered |
| R2 submission/completion separation | 1.3, 3.2, 4.1, 4.6, 5.3, 6.2-6.3, 8.1-8.3, 9.1 | 100% | None | Covered |
| R3 instret CPU work | 1.3, 3.1-3.2, 4.2, 4.6, 5.3, 6.2-6.4, 7.7-7.10, 8.1-8.3, 9.1-9.2 | 100% | 当前有效样本以 raw begin/end 证明无 regression；失败路径不补测，README 声明诊断限制。用户已批准 | Simplified |
| R4 useful-work overlap | 1.3, 3.3, 4.3, 4.6, 5.3, 6.2-6.4, 8.1-8.3, 9.1-9.2 | 100% | None | Covered |
| R5 timer wakeup overshoot | 1.3, 3.4, 4.4, 4.6, 5.3, 6.2-6.4, 8.1-8.3, 9.1-9.2 | 100% | None | Covered |
| R6 per-workload TX counters | 1.3, 4.5-4.6, 5.3, 6.2-6.4, 7.7-7.10, 8.1-8.3, 9.1 | 100% | `completed_bytes` 与 hw-send-calls/KiB 从同 section 数据推导，不重测。用户已批准 | Simplified |
| R7 isolated/reproducible output | 1.3, 3.4-3.5, 4.1-4.6, 5.1-5.3, 6.1-6.4, 7.7-7.10, 8.1-8.3, 9.1 | 100% | revision/dirty 使用 Git 状态与源码/binary hash 外部固定；成功日志不补 partial/zero/errno 字段。用户已批准 | Simplified |
| R8 baseline and cross-branch evidence | 1.1, 3.5, 5.1, 5.3-5.4, 6.1-6.5, 7.1-7.6, 8.1-8.4, 9.1, 9.3, 9.5 | 100% | Console work placed in a user-requested follow-up iteration; requirement unchanged | Covered |
| R9 existing UART behavior unchanged | 1.4, 2.4, 3.2, 4.6, 5.2, 6.3, 7.1-7.6, 8.1-8.3, 9.1-9.5 | 100% | None | Covered |

Console 分支顺序是实施分段，不是需求裁剪。全部 requirement 均有任务、验证方法和失败边界。

## Evidence Layout

```text
.claude/analysis/q31-cpu-efficiency-evidence/
  README.md
  baseline/
    async-d1.md
    async-qemu.md
    console-d1.md
    console-qemu.md
  async/
    qemu-rootfs.log
    d1-fullbench-command.log
  console/
    qemu-rootfs.log
    d1-fullbench-command.log
  comparison/
    result.md
```

`README.md` 记录每份文件的 source branch、commit、benchmark hash、build/run command、target、hart 数、串口配置和 SHA-256。Console 目录在后续 iteration 填充。

## Risks / Trade-offs

- [hart-wide instret 被其他任务污染] → D1 保持单 hart、采样前 drain、固定启动路径、重复至少五轮，并保留 raw delta。
- [Console 没有 TX debug ioctl] → counter 标记不可用；不阻塞跨分支主指标。
- [Console 无并发 loaded timer window] → S43 loaded 标记不适用；不引入未验证线程基础设施。
- [D1 time 修正改变全部时间输出] → 先做确定性换算测试，再跑 QEMU/D1 回归；旧新日志不得混表。
- [payload 输出包含零字节] → 保持 raw UART，记录完成字节和 hash；终端展示不是完整性来源。
- [现有工作树含用户文档改动] → 只修改本 change 文件；Act 复制 baseline 时先记录源 hash，不覆盖分析文档与 R43。
- [真板暂不可用] → QEMU 结果只完成软件 Gate；D1 task 保持 ENV BLOCK，不能完成 Async evidence Gate。

## Migration Plan

1. 冻结四份现有 docs 日志到 baseline evidence，并记录 hash。
2. 在 Async 分支建立 time conversion 与 benchmark RED witness。
3. 修正 D1 time conversion，完成 benchmark 和静态/构建 Gate。
4. 采集 Async QEMU 与 D1 新日志；Plan Review 审查实际 diff 和证据。
5. 新建 Console iteration；用户切换分支后复用相同 benchmark 版本和计时修正。
6. 保存 Console 新日志并生成 comparison。

回滚只撤销 time conversion 和 benchmark 代码；evidence 保留并标记失败或 superseded，不删除原始日志。

## Open Questions

None。范围、计时修正、D1 对照和跨分支顺序已由用户批准。
