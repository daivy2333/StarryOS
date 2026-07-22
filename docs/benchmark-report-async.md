# Async UART 与 polling Console 性能对比

> 项目：StarryOS / `uart-lichee` + `console-lichee`
> 日期：2026-07-22
> 范围：2×2 矩阵对比 — QEMU / D1 × async UART / polling Console，含 S41–S43 CPU 效率
> 测试程序：[tests/benchmark.c](../tests/benchmark.c)（Q31 Async）/ Console 分支同口径移植
> 证据归档：`openspec/changes/archive/2026-07-22-q31-async-uart-cpu-efficiency-benchmark/`、`openspec/changes/archive/2026-07-22-q32-console-cpu-efficiency-benchmark/`

## 结论

D1 两组同步完成吞吐都接近 115200 bps 上限。Console S10 在 64B/256B/1024B 分别为 99.0%/99.3%/99.4% 线速，async UART 为 96.8%/97.3%/98.8%，差值 2.2/2.0/0.6 pp。

CPU 效率差距大。D1 Console S41 `instructions/byte` 在 64/256/1024B 为 1194/1105/1106，async UART 为 32818/32792/44716。Console 同步写路径不需要 copier 任务、ring buffer 和中断唤醒，instret 约为 Async 的 1/30（64/256B）到 1/40（1024B）。`instret` 是 hart-wide CPU-work proxy，不是 CPU utilization。

async UART 的优势在提交与发送解耦。D1 S11 64B/256B async 入队速度 3997.04/11280.08 KB/s，Console 同步写为 11.45 KB/s。1024B async 受 64 KiB ring backpressure 影响降至 31.79 KB/s，仍高于 Console。

S42 展示两条路径的差异。D1 Console synchronous `write()` 在 554.66 ms 返回，UART 已发送完毕，overlap_efficiency=0.0000。D1 async `write()` 在 ring 入队后约 1.6 ms 返回，UART 后台发送，overlap 窗口约 53.5%。

S43 timer overshoot 在 D1 两组相近：Console idle P50=8.42 ms，Async idle P50=9.53 ms。Console loaded 为 `not-applicable`（同步写耗尽发送窗口）。Async loaded P50=25.78 ms，来自发送积压期的唤醒干扰。

所有 TX section `Done.`，`drain_errors=0`。所有 S43 aggregate 标记 `not-independently-recomputed`（每组仅 3/50 raw samples）。`instret` 含同 hart 背景活动。Console S40 telemetry 为 `UNSUPPORTED`。不声明 SMP 正确性。

## 测试环境

| 环境 | 后端 | S11 语义 | 原始日志 | SHA256 |
|---|---|---|---|---|
| QEMU rootfs | async UART | enqueue + final drain | [qemu_out.md](qemu_out.md) | `d2f2486a…15d2` |
| D1 command-entry | async UART | enqueue + final drain | [d1_out.md](d1_out.md) | `b98af673…947cc` |
| QEMU rootfs | polling Console | synchronous-blocking + final drain | [qemu_console.md](qemu_console.md) | `67b7bb02…e227c` |
| D1 command-entry | polling Console | synchronous-blocking + final drain | [d1_console.md](d1_console.md) | `b3f11fce…55aaf` |

两组用相同的 sizes、100 迭代和 drain policy。QEMU 用 ext4 rootfs，D1 用 memory-root command-entry。设备均为 `/dev/console`。Async benchmark version `q31-cpu-efficiency-20260721`，Console `q32-console-cpu-efficiency-20260722`。S41/S42/S43 的 payload、轮数、完成点和 raw 字段一致，经 Q31 iteration 003 Plan Review 和 Q32 iteration 001/002 交叉验证。

Console 日志标题已修正为 `Console Benchmark`，S00 manifest 含 `backend=polling-console`。D1 Console S43 hang（IRQ stub）已修复并在 Q32 iteration 001/002 验证。

QEMU UART 模型不仿真物理线延迟，`line_rate_pct` > 100% 是预期现象，QEMU 只作为路径和接口行为证据。D1 数据来自物理 UART0，是绝对线速依据。

统计口径：P50/P95/P99 来自每组 N 次样本排序后分位值。P50 代表典型耗时，P99 观察少数慢样本。P99/P50 越高，尾部抖动越明显。`line+10ms` tail 指单次耗时超过理论线时再加 10 ms。

## 同步完成吞吐（S10/S12/S13/S14）

S10 drain-each：64B/256B/1024B 各 100 次 `write_full()` + `tcdrain()`，计入从写入到 drain 返回的时间。测"用户认为已发送完成"的吞吐和尾部延迟。

| Payload | QEMU async KB/s | QEMU Console KB/s | QEMU 差值 | D1 async 线速 | D1 Console 线速 | D1 差值 |
|---|---|---|---:|---:|---:|---:|
| 64B | 151.54 | 177.17 | +16.9% | 96.8% | 99.0% | +2.2 pp |
| 256B | 177.84 | 183.12 | +3.0% | 97.3% | 99.3% | +2.0 pp |
| 1024B | 181.59 | 169.08 | -6.9% | 98.8% | 99.4% | +0.6 pp |

QEMU 差值是 Console 相对 async 的变化率。D1 用线速百分点，避免夸大数据差异。

| Payload | QEMU async P99/P50 | QEMU Console P99/P50 | D1 async P99/P50 | D1 Console P99/P50 | D1 async/Console tail |
|---|---|---|---:|---:|---:|
| 64B | 2.05 | 2.04 | 1.00 | 1.00 | 0 / 0 |
| 256B | 1.81 | 1.82 | 2.46 | 1.00 | 1 / 0 |
| 1024B | 1.54 | 1.49 | 1.38 | 1.00 | 1 / 0 |

D1 Console S10 P99 接近 P50。D1 async 256B/1024B 各有一个 `line+10ms` tail。

S12 batch-drain：连续写 100 次，每 8 次一次 `tcdrain()`，末尾补一次 drain。测批量提交能否摊薄 drain 开销。

| Payload | QEMU async KB/s | QEMU Console KB/s | QEMU 差值 | D1 async 线速 | D1 Console 线速 | D1 差值 |
|---|---|---|---:|---:|---:|---:|
| 64B | 170.50 | 174.38 | +2.3% | 98.8% | 99.4% | +0.6 pp |
| 256B | 185.36 | 179.03 | -3.4% | 98.5% | 99.4% | +0.9 pp |
| 1024B | 191.50 | 165.47 | -13.6% | 99.1% | 99.4% | +0.3 pp |

S13 测 `writev()` fragment aggregation：4 个 64B `iovec`，每轮一次 `writev()` 后 drain。S14 测小包 break-even：64B/128B/256B drain-each。

| Section | QEMU async/Console KB/s | QEMU 差值 | D1 async/Console 线速 | D1 差值 |
|---|---|---|---|
| S13 writev 4×64B | 167.85 / 161.37 | -3.9% | 98.7% / 99.3% | +0.6 pp |
| S14 64B | 124.22 / 155.64 | +25.3% | 96.8% / 99.0% | +2.2 pp |
| S14 128B | 143.03 / 159.94 | +11.8% | 95.3% / 99.2% | +3.9 pp |
| S14 256B | 177.31 / 161.91 | -8.7% | 97.2% / 99.3% | +2.1 pp |

D1 async S14 128B/256B 各有一个 `line+10ms` tail，Console 三组均为 0。

## 提交速度（S11）

S11 两组语义不同。async 计时窗口测 ring enqueue，随后 final drain。Console 的 write loop 已同步发送完毕，`enqueue_kbps` 不适用。表格并列行为，不算倍率。

| 平台 | Payload | async loop KB/s | async final drain | Console loop KB/s | Console final drain |
|---|---|---|---:|---:|---:|---:|
| QEMU | 64B | 5223.13 | 32 ms | 157.67 | 1 ms |
| QEMU | 256B | 23516.13 | 125 ms | 163.09 | 1 ms |
| QEMU | 1024B | 506.81 | 316 ms | 168.87 | 0 ms |
| D1 | 64B | 3997.04 | 545 ms | 11.45 | 10 ms |
| D1 | 256B | 11280.08 | 2183 ms | 11.45 | 10 ms |
| D1 | 1024B | 31.79 | 5590 ms | 11.45 | 10 ms |

四组 `policy=no-drain` 行均报告 `short_writes=0`。D1 async 1024B loop 降至 31.79 KB/s — 100 KiB workload 触发 64 KiB ring backpressure。Console 无提交队列，loop 始终受物理发送速度限制。

## 延迟（S20/S21）

S20 单字节 `write + tcdrain` 延迟：100 次，每次 1B 后立即 drain。测最小 payload 同步完成延迟。

| 指标 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|
| n | 100 | 100 | 100 | 100 |
| avg | 0.176 ms | 0.037 ms | 0.192 ms | 0.106 ms |
| P50 | 0.171 ms | 0.037 ms | 0.191 ms | 0.106 ms |
| P95 | 0.213 ms | 0.039 ms | 0.193 ms | 0.106 ms |
| P99 | 0.278 ms | 0.082 ms | 0.238 ms | 0.112 ms |
| P99/P50 | 1.62 | 2.21 | 1.25 | 1.05 |
| `line+10ms` tail | 0 | 0 | 0 | 0 |

Console 相对 async 平均延迟：QEMU 低 79.0%，D1 低 44.8%。P99 分别低 70.5% 和 52.9%。QEMU 百分比只表示同模拟器回归差异。

S21 FIFO boundary matrix：1/15/16/17/31/32/33/48/49B 各 100 次 drain-each，围绕 16B FIFO 边界。所有延迟单位 ms。

| Size | QEMU async P50 | QEMU async P99 | QEMU Console P50 | QEMU Console P99 | D1 async P50 | D1 async P99 | D1 Console P50 | D1 Console P99 |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | 0.186 | 0.486 | 0.036 | 0.084 | 0.193 | 0.233 | 0.106 | 0.118 |
| 15 | 0.226 | 1.731 | 0.105 | 0.485 | 1.421 | 23.992 | 1.300 | 1.398 |
| 16 | 0.201 | 1.515 | 0.118 | 0.235 | 1.507 | 24.676 | 1.385 | 1.483 |
| 17 | 0.237 | 1.340 | 0.120 | 0.269 | 1.585 | 24.758 | 1.470 | 1.569 |
| 31 | 0.305 | 1.539 | 0.204 | 0.374 | 2.783 | 25.966 | 2.665 | 2.761 |
| 32 | 0.323 | 1.589 | 0.208 | 0.645 | 2.868 | 25.784 | 2.750 | 2.847 |
| 33 | 0.303 | 1.530 | 0.220 | 0.391 | 2.941 | 25.872 | 2.836 | 2.932 |
| 48 | 0.365 | 1.162 | 0.297 | 0.610 | 4.233 | 27.153 | 4.115 | 4.212 |
| 49 | 0.412 | 1.644 | 0.321 | 0.640 | 4.311 | 27.240 | 4.201 | 4.297 |

D1 async size≥15 每组各有一个 `line+10ms` tail（24–27 ms），Console 全部为 0。async S40 同时显示 `slow_poll_exh=0`、`yield_exh=0`，这些样本不能归因于 fallback 耗尽。根因未定位。

## CPU 效率（S41/S42/S43）

### S41 TX CPU Work

在 `instret` 区间内完成 100 次 `write_full()` + `tcdrain()` 完整发送链，64B/256B/1024B 三种 payload 各 5 轮。`instret` 是 hart-wide CPU-work proxy。

| 指标 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|
| 64B inst/byte median | 14358 | 14779 | 32818 | 1194 |
| 256B inst/byte median | 13504 | 14173 | 32792 | 1105 |
| 1024B inst/byte median | 13843 | 13757 | 44716 | 1106 |

D1 Console 三条 payload `instructions/byte` 稳定在 1100 左右。同步 `write() → polling send → tcdrain()` 路径不需要 copier 任务和中断唤醒。D1 async 64B/256B 约 32800 inst/byte，1024B 升至 44716（ring backpressure + `tcdrain` 卡 TEMT）。

QEMU instret 在 13500–14800，两组差距小。虚拟 UART 无物理线速限制，qemu-system 自身开销主导。QEMU instret 不作为硬件性能证据。

所有 15 轮 valid，`short_writes=0`、`drain_errors=0`、`incomplete_logical=0`。Console S41-local-counters 为 `not-available`。

### S42 TX Compute Overlap

先测纯计算 idle 基线，再在 64B×100 UART write 窗口内执行相同 compute kernel，比较可执行的有效计算量。

| 指标 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|---|
| idle iters | 311639 | 265637 | — | 271928 |
| write_return_us | ~27 ms | ~29 ms | ~1.6 ms | ~554663 μs |
| median useful_iters | — | 278018 | — | 0 |
| median overlap_efficiency | — | 1.0466 | 0.5353 | 0.0000 |
| valid rounds | 5/5 | 5/5 | 5/5 | 5/5 |

D1 Console `write_return_us ≈ 554.66 ms` 已超过 64B×100 理论线时 542.535 ms。同步写把整个发送过程纳入调用窗口，`write()` 返回前数据已发送完毕。与 idle 基线之间无剩余 overlap（efficiency=0.0000）。这是同步写路径的固有语义，不是性能失败。

D1 async `write()` 约 1.6 ms 返回，UART 后台发送，overlap_efficiency=0.5353。QEMU 两组都有 overlap（虚拟 UART 极快）。

### S43 Timer Wakeup Overshoot

`clock_nanosleep(TIMER_ABSTIME)` 按 5ms 间隔绝对时间睡眠。5 组 idle（纯睡眠）+ 5 组 loaded（4096B burst write 期间睡眠），每组 50 次采样。

| 指标 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|---|
| idle P50 | — | 6.28 ms | 9.53 ms | 8.42 ms |
| idle P95 | — | 9.47 ms | 9.82 ms | 8.77 ms |
| idle P99 | — | 9.63 ms | 15.85 ms | 8.77 ms |
| loaded P50 | — | — | 25.78 ms | not-applicable |
| loaded P95 | — | — | 47.33 ms | not-applicable |
| loaded P99 | — | — | 49.63 ms | not-applicable |
| valid groups | 5/5 | 5/5 | 5/5 | 5/5 idle, 5/5 loaded |

D1 Console idle P50=8.42 ms，P99=8.77 ms 无尾部抖动。loaded 组 `not-applicable reason=no-overlap-window`：同步 `write()` 用 355 ms 发送 4096B，耗尽 347 ms 理论线时窗口。D1 async loaded P50=25.78 ms，来自发送积压期的唤醒干扰。

S43 每组仅输出前 3/50 raw samples，无法独立重算全部 percentile。所有 P50/P95/P99/max 以原始日志 hash 锚定，标记为 `reported, hash-anchored, not-independently-recomputed`。

## 计数器与内部能力（S40/S05）

S40 在所有用户态 benchmark 结束后读 `UART_TXDBG_SNAPSHOT`，输出 TX counter proxy。测 TX 路径行为，不测 CPU 百分比。

| 指标 | QEMU async | D1 async | QEMU/D1 Console |
|---|---|---|---|
| telemetry_available | 0 | 1 | UNSUPPORTED |
| user_calls | 0 | 2577 | N/A |
| user_acc | 0 | 338201 | N/A |
| ring_pop_calls | 0 | 1659 | N/A |
| ring_pop_bytes | 0 | 338108 | N/A |
| hw_send_calls | 0 | 13842121 | N/A |
| hw_send_bytes | 0 | 338108 | N/A |
| hw_send_zero | 0 | 13820496 | N/A |
| hw_send_max_chunk | 0 | 16 | N/A |
| no_progress_budget | 0 | 20171 | N/A |
| slow_poll_exh | 0 | 0 | N/A |
| yield_exh | 0 | 0 | N/A |

D1 async 派生值：`bytes_per_user_call=131.2`、`bytes_per_ring_pop=203.8`、`bytes_per_hw_send=0.024`、`zero_per_kb=41857.0`、`no_progress_per_kb=61.1`。QEMU async counter 为 0，标记 `telemetry-counters-are-zero`。

`hw_send_zero` 高是 slow-poll 路径频繁探测 TX FIFO 的观测结果，不当作 CPU 占用率。`slow_poll_exh=0`、`yield_exh=0` 表示本轮 fallback 未耗尽。

S05 ring buffer benchmark 在内核初始化后运行，测驱动内部能力。绕过用户态 `write()/tcdrain()`、syscall、调度和物理线速。Console 无 async ring，标记 `SKIPPED reason=no-async-driver`。

| 指标 | QEMU async | D1 async |
|---|---|---|
| TX ring write | 321446.51 KB/s | 718020.06 KB/s |
| RX ring read | 1019108.28 KB/s | 8303061.75 KB/s |
| RX latency P99 | 11600 ns | 123 ns |
| 启动时 IRQ count | 0 | 0 |

## 稳定性（S30/S31）

| 测试事项 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|---|
| S30 `open(O_NONBLOCK)` | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN |
| S30 `ioctl(FIONBIO)` | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN |
| S31 fixed payload RX | SKIPPED | SKIPPED | SKIPPED | SKIPPED |
| TX drain errors | 0 | 0 | 0 | 0 |
| 完成状态 | `Done.` | `Done.` | `Done.`, exit 0 | `Done.`, exit 0 |

四组 S30 均为 PASS/EAGAIN。X30 空读行为一致。

## 边界与后续

四份日志覆盖同口径 benchmark 的 QEMU/D1 × async/Console 组合。S41/S42/S43 由 Q31 Async 和 Q32 Console 独立 change 实现，payload、轮数、完成点和 raw 字段经 cross-review 确认一致。可比 section 按同平台比较；能力不一致的 section 保留 `SKIPPED`、`UNSUPPORTED`、`N/A` 或 `not-applicable`。

| 项目 | 状态 | 说明 |
|---|---|---|
| D1 Console RX | 已修复 | S30 四组 PASS/EAGAIN |
| RX fixed payload | SKIPPED | 四组均 `BENCH_RX_FIXED_BYTES=0` |
| D1 async size≥15 P99 tail | 保留 | 每组一次 24–27 ms 样本，根因未定位 |
| QEMU async S40 | N/A | telemetry counters 为 0 |
| Console S40 | UNSUPPORTED | polling backend 无 async telemetry |
| Console S41/S42/S43 local counters | not-available | ioctl 不可用 |
| D1 Console S42 overlap=0 | 合法语义 | 同步写耗尽发送窗口，非性能失败 |
| D1 Console S43 loaded | not-applicable | write 355ms > 347ms 理论线时 |
| S43 percentile | not-independently-recomputed | 每组仅 3/50 raw samples |
| `instret` | CPU-work proxy | hart-wide，不含 CPU utilization |
| CPU 使用率 | 未采集 | counter proxy 与 instret 都不能替代 |
| SMP 正确性 | 未声明 | 不构成多 hart 证据 |
