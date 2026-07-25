# Q31 Console CPU 效率测试移植分析

> Project: StarryOS  
> Branch: `console-lichee`  
> Commit: `73b8973ad5ae198a07ce730f830b6d6e1db93718`  
> Date: 2026-07-22  
> See also: [Async CPU 效率指标](async-uart-cpu-efficiency-metrics.md), [Console 测量设计](console-performance-measurement-design.md), [Q31 evidence](q31-cpu-efficiency-evidence/README.md)

## 目标与范围

本文为 Q31 的 Console iteration 提供代码输入。目标是把 Async 分支的 Q31 测量带到 `console-lichee`，得到可横向比较的 QEMU 与 D1 数据。

本文回答五个问题：

1. Async 分支改了哪些测试与计时代码？
2. 当前 Console 分支缺少哪些改动？
3. 哪些代码可复用，哪些必须适配？
4. 如何保持 payload、完成点和输出口径一致？
5. 后续 Plan 与 Act 应按什么顺序验证？

范围只包含 benchmark、D1 时间换算和证据采集。不修改 Console writer、TTY、polling port、`TCSBRK`、UART MMIO 或内核调度语义。

## 结论

Async Q31 优化已进入提交 [`7d44cb1`](https://github.com/daivy2333/StarryOS/commit/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7)。产品代码改动只有三处：

- `tests/benchmark.c`：新增 742 行、删除 9 行。
- D1 `time.rs`：新增 14 行、删除 3 行。
- 新建 `time_math.rs`：169 行，含 12 个测试。

当前 Console 分支仍使用旧 benchmark 和截断时间换算。当前文件与 Async Q31 版本的差异如下：

| 文件 | Console 当前状态 | Async Q31 状态 | Console iteration 动作 |
| --- | --- | --- | --- |
| `tests/benchmark.c` | 824 行，SHA-256 `cf26ff3d...` | 1545 行，SHA-256 `4ad658f3...` | 以 Async 文件为基线，重放 Console 适配 |
| D1 `time.rs` | 40 行，SHA-256 `eecaf202...` | 51 行，SHA-256 `c821367e...` | 使用宽整数换算 |
| D1 `time_math.rs` | 不存在 | 169 行，SHA-256 `78399919...` | 原样引入并先跑测试 |

当前内核已提供 `/proc/instret` 和绝对时间睡眠。两处无需改动：[`proc.rs`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/kernel/src/pseudofs/proc.rs#L379-L389) 暴露 hart-wide `instret`，[`schedule.rs`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/kernel/src/syscall/task/schedule.rs#L54-L88) 支持 `CLOCK_MONOTONIC | TIMER_ABSTIME`。

不应整体 cherry-pick `7d44cb1`。该提交还包含 Async 日志、二进制、Runbook、Q31 change 和 references。Console iteration 只需移植三处产品代码，再采集 Console evidence。

## Async 改动清单

Async Q31 的 benchmark 入口位于 [`tests/benchmark.c`](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/tests/benchmark.c)。改动可分为六组。

| 组 | 符号或 section | 作用 |
| --- | --- | --- |
| 复现信息 | `BENCH_HART_COUNT`、`BENCH_SOURCE_REVISION`、`BENCH_SOURCE_DIRTY`、`print_manifest` | 输出设备号、hart、源码状态与测量能力 |
| 严格完成 | `counted_write_stats_t`、`counted_write_full` | 统计 logical write、syscall、partial、retry、errno、timeout 和完成字节 |
| CPU work | `read_instret_strict`、`report_instret_overhead`、S41 | 测 write 开始到 final TEMT drain 的 `instret/byte` |
| 有效计算 | `fixed_compute`、S42 | 在理论线时窗口内比较 idle 与 UART 计算迭代数 |
| 唤醒响应 | `collect_abs_sleep_samples`、`print_timer_stats`、S43 | 测 idle 与 TX load 下的绝对睡眠 overshoot |
| 路径计数 | `print_workload_tx_counters`、S40 | 分段 reset/snapshot Async TX debug counters |

S11 还新增：

```text
submit_fraction    = enqueue_time / (enqueue_time + final_drain_time)
producer_available = 1 - submit_fraction
```

S41 使用 64、256、1024 B，每组 100 次、5 轮。S42 使用 64 B × 100、1 轮预热、5 轮采样。S43 使用 5 组 idle、5 组 loaded，每组 50 个 5 ms 绝对 deadline，loaded burst 为 4096 B。

D1 时间修正位于 [`time.rs`](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/crates/axplat-riscv64-lichee-d1/src/time.rs) 与 [`time_math.rs`](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/crates/axplat-riscv64-lichee-d1/src/time_math.rs)。公式为：

```text
ticks_to_nanos = ticks × 1_000_000_000 / 24_000_000
nanos_to_ticks = nanos × 24_000_000 / 1_000_000_000
```

乘法使用 `u128`，结果向下取整，超出 `u64` 时饱和。旧实现把单 tick 截断为 41 ns，导致 24,000,000 ticks 被换算为 984,000,000 ns。

## 当前 Console 数据流

Console TX 调用链保持不变：

```text
benchmark write/writev
  -> TTY write_at / ONLCR
  -> ConsoleWriter::write
  -> with_console_port_tx
  -> CONSOLE_LOCK + CONSOLE_PORT
  -> PollingPort::putchar
  -> THRE polling + THR MMIO

benchmark tcdrain
  -> ioctl(TCSBRK)
  -> with_console_port_tx
  -> TEMT polling
```

[`ConsoleWriter::write`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/kernel/src/pseudofs/dev/tty/console.rs#L40-L48) 同步写完整 buffer。[`with_console_port_tx`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/kernel/src/platform/polling.rs#L283-L301) 持有全局 Console 锁和本地 port 锁。[`TCSBRK`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/kernel/src/syscall/fs/ctl.rs#L42-L50) 再等待 TEMT。

这使相同 harness 产生预期差异：

| Section | Async | Console | 比较规则 |
| --- | --- | --- | --- |
| S11 | write 主要提交到 ring | write 同步轮询 THR | 保留分段原值，不计算 write 阶段倍率 |
| S41 | 包含调用者与 copier 的 hart-wide 指令 | 包含同步 polling 指令 | 完成字节、完成点相同，可比较 `instret/byte` |
| S42 | write 后仍可能有计算窗口 | write 通常耗尽理论窗口 | Console 的零迭代是有效结果 |
| S43 loaded | Async backlog 可与 sleep 重叠 | 同步 write 通常无 overlap window | Console 输出 `not-applicable`，不得复制 idle 数据 |
| TX counters | Async ioctl 可用 | Console ioctl 不可用 | Console 必须输出 `not-available` |

## Console 代码方案

`tests/benchmark.c` 应以 `7d44cb1` 的文件为基线。保留全部常量、payload、轮次、deadline、完成判据和字段名，仅允许以下 Console 差异。

| 适配点 | 处理 |
| --- | --- |
| backend | 保留 `BENCH_BACKEND="polling-console"`，manifest 输出该字段 |
| 标题 | 使用通用 `UART CPU Efficiency Benchmark`，不声称 Async |
| S05 | 保留 `SKIPPED reason=no-async-driver` |
| D1 RX | 保留当前 `BENCH_D1_DIAG` 的 RX unsupported 标记 |
| S40 | ioctl 失败输出 `not-available reason=backend-polling-console-no-telemetry` |
| S11/local counters | ioctl 失败不得打印全零 snapshot；统一输出 `not-available` |

`BENCH_VERSION`、S41/S42/S43 常量和所有比较字段必须与 Async Q31 相同。Console 源码 hash会因上述适配不同，evidence 必须保存完整 diff 与两个 hash。

S42 不需要 Console 专用短路。同步 `write_full` 返回后，`fixed_compute(deadline)` 会自然得到零次或极少迭代。该行为就是调用者没有获得重叠窗口的证据。

S43 应保留 Async 的 overlap 检查。若 4096 B write 已超过理论线时，loaded group 输出 `not-applicable reason=no-overlap-window`，不进入 aggregate。后续实现可跳过该组过期 deadline 的采样以缩短运行时间，但若这样做，必须同时保持输出合同，并在 Q31 Plan 中批准该 Console 适配。

D1 平台代码应原样引入 `time_math.rs`，并把 `time.rs` 的两个换算函数改为 `mul_div_floor`。该修正影响所有 D1 wall-clock 与 timer deadline，是 Console 与 Async 绝对时间可比的前置条件。

以下文件不应在 Q31 Console iteration 修改：

- `kernel/src/pseudofs/proc.rs`
- `kernel/src/syscall/task/schedule.rs`
- `kernel/src/syscall/fs/ctl.rs`
- `kernel/src/pseudofs/dev/tty/console.rs`
- `kernel/src/platform/polling.rs`
- `Makefile`，除非 Plan 决定注入 provenance 宏

当前 Makefile 已有 QEMU payload、D1 ELF 和 command-entry image 入口：[`tests/benchmark`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/Makefile#L44-L49)、[`benchmark-fullbench-elf`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/Makefile#L61-L67) 和 [`lichee-fullbench-command`](https://github.com/daivy2333/StarryOS/blob/73b8973ad5ae198a07ce730f830b6d6e1db93718/Makefile#L121-L127)。

## 实施与验证顺序

后续 Plan 应先 Review iteration 003。其 `Plan Review` 当前仍为 `pending`，Q31 tasks 7.2 要求 Review 通过后才能创建 Console iteration。

Console iteration 建议按以下顺序执行：

1. 冻结当前 Console benchmark、D1 time 文件和既有 Console 日志 hash。
2. 为旧 D1 换算建立 RED：24,000,000 ticks 得到 984,000,000 ns。
3. 引入 `time_math.rs`，运行 12 个 host tests，观察 GREEN。
4. 更新 `time.rs`，执行 D1 target `cargo check`。
5. 以 Async Q31 benchmark 为基线重放 Console 适配白名单。
6. 编译 QEMU 与 D1 静态 ELF，检查 `ET_EXEC`、无 relocation 和字符串标签。
7. 运行 QEMU，保存 `console/qemu-rootfs.log`。
8. 构建、检查并人工烧录 D1 image，保存 `console/d1-fullbench-command.log`。
9. 校验 payload、轮次、设备 5:1、timer、drain policy、完成字节、Done 与 exit 0。
10. 生成 `comparison/result.md`，再执行 Q31 最终 Gates。

TDD witness 应覆盖：

| Gate | RED 或 current witness | GREEN 或完成条件 |
| --- | --- | --- |
| D1 time | 一秒换算为 984 ms | 12/12 test，换算为 1 s |
| benchmark fields | 当前无 S41/S42/S43 | binary 含三组 section 与 Q31 version |
| Console adapter | 当前 S40 unsupported | S40 与 local counters 均显式 not-available |
| QEMU | 旧日志无 Q31 section | 新日志完整、Done、exit 0、无 short/drain error |
| D1 | 旧日志无 Q31 section | 新日志完整、Done、exit 0、时间换算已修正 |

QEMU 只证明构建、启动、rootfs payload、字段格式和回归。QEMU 的 UART timing 与 `tcdrain` 不形成物理线速证据。D1 日志用于线速、绝对时间与最终横向比较。D1 单 hart结果不关闭 Q17/Q24 的 SMP 风险。

## 横向比较合同

比较前必须逐项相等：

- benchmark version 与测量公式。
- 64/256/1024 B payload 和 100 次迭代。
- S41 五轮、S42 一轮预热加五轮、S43 五组配置。
- `/dev/console` 与设备号 5:1。
- `CLOCK_MONOTONIC` 和修正后的 24 MHz D1 换算。
- S41 从 write 开始到 final TEMT drain 的完成点。
- 115200 8N1、同一板卡、启动链和 command-entry。

最终报告分栏展示：

| 问题 | 指标 |
| --- | --- |
| 调用者何时返回 | S11 write/enqueue、final drain、submit fraction |
| 相同通信的 CPU work | S41 raw instret、instructions/byte、syscall writes |
| 释放窗口能否计算 | S42 useful iterations、overlap efficiency |
| 定时响应 | S43 idle 与 loaded P50/P95/P99/max |
| 完成能力 | D1 line rate、总完成时间、completed bytes |
| 正确性 | short writes、drain errors、timeout、Done、exit code |

`instret` 是 hart-wide CPU-work proxy，不是 task CPU time 或 CPU utilization。若 Async 只更早返回，但 `instret/byte`、S42 或 S43 没有改善，结论只能写“等待转移到后台”。

## 边界与失败路径

- 当前受限环境中的 musl 交叉编译器以 `Bad system call` 退出，退出码 159。本次未建立 Console 编译 PASS；后续 Act 应在普通 host shell 执行。
- `time_math.rs` 已在当前环境重新运行，12/12 通过，退出码 0。
- Console 没有 Async TX debug ioctl。返回错误是能力缺失，不是 S41/S42/S43 失败。
- S11 的 Console write 与 Async enqueue 语义不同。不得从两者计算吞吐倍率。
- S42 的 Console 零 overlap 是有效结果，不得标成 benchmark 故障。
- S43 loaded 无 overlap window 时必须排除，不得进入 loaded aggregate。
- `TCSBRK` 当前在 syscall 层全局处理 fd 归属。Q31 只对 `/dev/console` 调用，不受该缺陷阻塞；修复属于独立 correctness change。
- 采样期间 stdout 与被测设备相同。每个 section 必须先 drain，样本保存在内存，完成后再打印。
- 不得修改 UART/Console 语义来改善测量结果。发现需要改锁、polling、TTY 或 drain 时，退出 Q31 并另建 change。

## 关键文件

| 文件 | 用途 |
| --- | --- |
| [Async Q31 benchmark](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/tests/benchmark.c) | Console 移植基线 |
| [Console benchmark](../../tests/benchmark.c) | 当前旧 harness 与 Console 适配 |
| [Async D1 time](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/crates/axplat-riscv64-lichee-d1/src/time.rs) | 频率精确换算基线 |
| [Async time math](https://github.com/daivy2333/StarryOS/blob/7d44cb173a7a5e8e0584c28d7976ded1a4d882f7/crates/axplat-riscv64-lichee-d1/src/time_math.rs) | 宽整数 helper 与测试 |
| [Q31 tasks](../../openspec/changes/q31-async-uart-cpu-efficiency-benchmark/tasks.md) | Console tasks 8.1-8.4 与最终 Gate |
| [Q31 spec](../../openspec/changes/q31-async-uart-cpu-efficiency-benchmark/specs/uart-cpu-efficiency-benchmark/spec.md) | 横向测量合同 |
| [Console Runbook](../runbooks/console-benchmark-qemu-d1.md) | QEMU 与 D1 操作步骤 |

本次没有发现需要自动登记的 M、D、K 或 I 候选。既有 I11、I12 和 Q31 spec 已覆盖测量边界。
