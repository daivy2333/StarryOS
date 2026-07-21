# Async UART 与 polling Console 性能对比

> 项目：StarryOS / `console-lichee`
> 日期：2026-07-21
> 范围：QEMU 与 D1、async UART 与 polling Console 的 2×2 交叉对比
> 测试程序：[tests/benchmark.c](../tests/benchmark.c)

## 结论

D1 的同步完成吞吐都接近 115200 bps 上限。Console 在 S10 的 64B、256B、1024B 场景分别达到 99.0%、99.3%、99.4% 线速；async UART 为 96.8%、97.3%、98.8%。差值为 2.2、2.0、0.6 个百分点。

async UART 的优势出现在提交与发送解耦。D1 S11 的 64B 和 256B async 入队速度为 3997.04、11280.08 KB/s；Console 同步写为 11.45 KB/s。1024B async 测试受 64 KiB ring backpressure 影响，仍为 31.79 KB/s，高于 Console 的 11.45 KB/s。

Console 的同步完成延迟更低。S20 单字节平均延迟在 QEMU 从 0.176 ms 降到 0.037 ms，在 D1 从 0.192 ms 降到 0.106 ms。D1 async 在 S21 size>=15 时每组出现一次 24-27 ms tail；Console 未出现 `line+10ms` tail。

两种后端的所有 TX section 都完成到 `Done.`，`drain_errors=0`。D1 Console 尚不支持 RX，S30 为 `UNSUPPORTED`；S40 telemetry 仅 D1 async 有效。当前数据不包含 CPU 使用率，也不声明 SMP 正确性。

## 测试环境

| 环境 | 后端 | S11 语义 | 原始日志 | SHA256 |
|---|---|---|---|---|
| QEMU rootfs | async UART | enqueue + final drain | [qemu_out.md](qemu_out.md) | `d2f2486a…15d2` |
| D1 command-entry | async UART | enqueue + final drain | [d1_out.md](d1_out.md) | `b98af673…947cc` |
| QEMU rootfs | polling Console | blocking transmit + final drain | [qemu_console.md](qemu_console.md) | `748f0ad9…37da4` |
| D1 command-entry | polling Console | blocking transmit + final drain | [d1_console.md](d1_console.md) | `46ac67bd…8a91f` |

四组均使用 `q19c-m0-20260703`、`CLOCK_MONOTONIC`、相同 sizes、100 次迭代和相同 drain policy。QEMU 使用 ext4 rootfs；D1 使用 memory-root command-entry。设备均为 `/dev/console`。

Console 日志仍保留程序标题 `UART Async Benchmark`。后端以 S00 的 `backend=polling-console` 为准。async 日志没有 `backend` 字段，由冻结基线和启动日志识别。

QEMU UART 模型不仿真物理线延迟，`line_rate_pct` 大于 100% 是预期现象；QEMU 只作为路径、接口和相对行为证据。D1 数据来自物理 UART0，是绝对线速依据。

统计口径：P50/P95/P99 都来自每组 100 次样本排序后的分位值。P50 代表典型耗时，P99 用来观察少数慢样本；`P99/P50` 越高，说明尾部抖动越明显。`line+10ms` tail 指单次耗时超过该 payload 的理论线时再加 10 ms。

## 吞吐

S10 是 drain-each baseline。测试代码对 64B、256B、1024B 各执行 100 次 `write_full()`，每次写完立即 `tcdrain()`，计入从写入开始到 drain 返回的时间。这个测试测“用户认为已发送完成”的吞吐和尾部延迟。

| Payload | QEMU async KB/s | QEMU Console KB/s | QEMU 差值 | D1 async 线速 | D1 Console 线速 | D1 差值 |
|---|---:|---:|---:|---:|---:|---:|
| 64B | 151.54 | 177.17 | +16.9% | 96.8% | 99.0% | +2.2 pp |
| 256B | 177.84 | 183.12 | +3.0% | 97.3% | 99.3% | +2.0 pp |
| 1024B | 181.59 | 169.08 | -6.9% | 98.8% | 99.4% | +0.6 pp |

QEMU 差值是 Console 相对 async 的变化率。D1 使用线速百分点，避免把已接近上限的数据差异放大。

| Payload | QEMU async P99/P50 | QEMU Console P99/P50 | D1 async P99/P50 | D1 Console P99/P50 | D1 async/Console tail |
|---|---:|---:|---:|---:|---:|
| 64B | 2.05 | 2.04 | 1.00 | 1.00 | 0 / 0 |
| 256B | 1.81 | 1.82 | 2.46 | 1.00 | 1 / 0 |
| 1024B | 1.54 | 1.49 | 1.38 | 1.00 | 1 / 0 |

D1 Console 的 S10 P99 接近 P50。D1 async 的 256B 和 1024B 各有一个 `line+10ms` tail。

S12 是 batch-drain。测试代码连续写 100 次，每 8 次调用一次 `tcdrain()`，末尾再补一次 drain。这个测试测批量提交能否摊薄 drain 调用开销，同时仍等待硬件发送完成。

| Payload | QEMU async KB/s | QEMU Console KB/s | QEMU 差值 | D1 async 线速 | D1 Console 线速 | D1 差值 |
|---|---:|---:|---:|---:|---:|---:|
| 64B | 170.50 | 174.38 | +2.3% | 98.8% | 99.4% | +0.6 pp |
| 256B | 185.36 | 179.03 | -3.4% | 98.5% | 99.4% | +0.9 pp |
| 1024B | 191.50 | 165.47 | -13.6% | 99.1% | 99.4% | +0.3 pp |

S13 测 `writev()` fragment aggregation。测试代码构造 4 个 64B `iovec`，每轮一次 `writev()` 后 drain，用来确认分片写入路径和短写行为。S14 测小包 break-even，使用 64B、128B、256B drain-each，观察小包尺寸变化对吞吐和 tail 的影响。

| Section | QEMU async/Console KB/s | QEMU 差值 | D1 async/Console 线速 | D1 差值 |
|---|---:|---:|---:|---:|
| S13 writev 4×64B | 167.85 / 161.37 | -3.9% | 98.7% / 99.3% | +0.6 pp |
| S14 64B | 124.22 / 155.64 | +25.3% | 96.8% / 99.0% | +2.2 pp |
| S14 128B | 143.03 / 159.94 | +11.8% | 95.3% / 99.2% | +3.9 pp |
| S14 256B | 177.31 / 161.91 | -8.7% | 97.2% / 99.3% | +2.1 pp |

D1 async 的 S14 128B、256B 各有一个 `line+10ms` tail；Console 三组均为 0。

S11 在两种后端上的语义不同。async 计时窗口测 ring enqueue，随后 final drain；Console 的 write loop 已同步发送，不能把 `enqueue_kbps` 当作入队速度。表格并列行为，不计算 Console/async 倍率。

| 平台 | Payload | async loop KB/s | async final drain | Console loop KB/s | Console final drain |
|---|---:|---:|---:|---:|---:|
| QEMU | 64B | 5223.13 | 32 ms | 157.67 | 1 ms |
| QEMU | 256B | 23516.13 | 125 ms | 163.09 | 1 ms |
| QEMU | 1024B | 506.81 | 316 ms | 168.87 | 0 ms |
| D1 | 64B | 3997.04 | 545 ms | 11.45 | 10 ms |
| D1 | 256B | 11280.08 | 2183 ms | 11.45 | 10 ms |
| D1 | 1024B | 31.79 | 5590 ms | 11.45 | 10 ms |

四组 `policy=no-drain` 行均报告 `short_writes=0`。D1 async 1024B 的 loop 降到 31.79 KB/s，说明 100 KiB workload 已触发 64 KiB ring backpressure。Console 没有提交队列，loop 始终受物理发送速度限制。

## 稳定性

四组 benchmark 均执行到 `Done.`。两组 D1 日志记录进程退出码 0；S10/S12/S13/S14/S20/S21 的 `drain_errors` 均为 0。

S30 测空 RX 的非阻塞语义。测试代码先用 `O_NONBLOCK` 打开 `/dev/console` 并读 16B，再用普通 open 后 `ioctl(FIONBIO)` 切到非阻塞并读 16B；空缓冲时预期是 `EAGAIN`。S31 是可选 fixed-payload RX witness，只有 `BENCH_RX_FIXED_BYTES > 0` 时才读固定字节数，本轮配置为 0。

| 测试事项 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---|---|---|---|
| S30 `open(O_NONBLOCK)` | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN | UNSUPPORTED |
| S30 `ioctl(FIONBIO)` | PASS/EAGAIN | PASS/EAGAIN | PASS/EAGAIN | UNSUPPORTED |
| S31 fixed payload RX | SKIPPED | SKIPPED | SKIPPED | SKIPPED |
| S40 telemetry | counters=0 | UNSUPPORTED | available | UNSUPPORTED |
| TX drain errors | 0 | 0 | 0 | 0 |
| 完成状态 | `Done.` | `Done.` | `Done.`, exit 0 | `Done.`, exit 0 |

D1 Console 的 S30 明确标记 `reason=D1-UART-RX-not-implemented`。这不是 async 与 Console RX 性能对比；当前只有 async D1 路径具备 RX 空读语义证据。

## CPU 与计数器代理

当前 benchmark 没有采集 CPU 使用率。S40 只提供 TX counter proxy，用来判断 async 发送路径的调用形态和 fallback 是否耗尽。

S40 在所有用户态 benchmark 结束后读取 `UART_TXDBG_SNAPSHOT`。测试代码输出 user-push、ring-pop、hw-send、no-progress 和 drain-state 计数，再派生每次调用平均字节数、每 KB 空发送次数等 proxy。它测的是 TX 路径行为，不是 CPU 百分比。

| 指标 | QEMU async | D1 async | QEMU/D1 Console |
|---|---:|---:|---|
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

QEMU async counter 为 0，因此 S40 标记 `telemetry-counters-are-zero`。D1 async 的派生值为 `bytes_per_user_call=131.2`、`bytes_per_ring_pop=203.8`、`bytes_per_hw_send=0.024`、`zero_per_kb=41857.0`、`no_progress_per_kb=61.1`。

`hw_send_zero` 高是 slow-poll 路径下频繁探测 TX FIFO 的观测结果，不能当作 CPU 占用率。`slow_poll_exh=0` 与 `yield_exh=0` 表示本轮 fallback 未耗尽。

启动阶段 ring buffer benchmark 测驱动内部能力，不代表 UART 物理线速。它在内核初始化后运行，覆盖 TX ring buffer push、RX ring buffer pop、单字节 RX pop 延迟、buffer 配置、NAPI 配置和 IRQ 计数。

这组测试绕过用户态 `write()/tcdrain()`、syscall、调度等待和物理线速。Console 没有 async ring，因此 S05 标记 `SKIPPED reason=no-async-driver`。

| 指标 | QEMU async | D1 async | QEMU Console | D1 Console |
|---|---:|---:|---|---|
| TX ring write | 321446.51 KB/s | 718020.06 KB/s | SKIPPED | SKIPPED |
| RX ring read | 1019108.28 KB/s | 8303061.75 KB/s | SKIPPED | SKIPPED |
| RX latency P99 | 11600 ns | 123 ns | SKIPPED | SKIPPED |
| 启动时 IRQ count | 0 | 0 | N/A | N/A |

## 延迟

S20 是单字节 `write + tcdrain` 延迟。测试代码循环 100 次，每次写 1B 后立即 drain，并记录单次耗时。这个测试测最小 payload 下的同步完成延迟，通常不触发 FIFO 满路径。

| 指标 | QEMU async | QEMU Console | D1 async | D1 Console |
|---|---:|---:|---:|---:|
| n | 100 | 100 | 100 | 100 |
| avg | 0.176 ms | 0.037 ms | 0.192 ms | 0.106 ms |
| P50 | 0.171 ms | 0.037 ms | 0.191 ms | 0.106 ms |
| P95 | 0.213 ms | 0.039 ms | 0.193 ms | 0.106 ms |
| P99 | 0.278 ms | 0.082 ms | 0.238 ms | 0.112 ms |
| P99/P50 | 1.62 | 2.21 | 1.25 | 1.05 |
| `line+10ms` tail | 0 | 0 | 0 | 0 |

Console 相对 async 的平均延迟在 QEMU 低 79.0%，在 D1 低 44.8%。P99 分别低 70.5% 和 52.9%。QEMU 百分比只表示同模拟器回归差异。

S21 是 FIFO boundary matrix。测试代码对 1、15、16、17、31、32、33、48、49B 各写 100 次，每次写完 drain；这些尺寸围绕 16B FIFO 边界和其倍数展开。这个测试测 payload 接近或跨过 FIFO 边界时的延迟增长和 tail。

| Size | QEMU async P50/P99 | QEMU Console P50/P99 | D1 async P50/P99 | D1 Console P50/P99 |
|---:|---:|---:|---:|---:|
| 1 | 0.186 / 0.486 | 0.036 / 0.084 | 0.193 / 0.233 | 0.106 / 0.118 |
| 15 | 0.226 / 1.731 | 0.105 / 0.485 | 1.421 / 23.992 | 1.300 / 1.398 |
| 16 | 0.201 / 1.515 | 0.118 / 0.235 | 1.507 / 24.676 | 1.385 / 1.483 |
| 17 | 0.237 / 1.340 | 0.120 / 0.269 | 1.585 / 24.758 | 1.470 / 1.569 |
| 31 | 0.305 / 1.539 | 0.204 / 0.374 | 2.783 / 25.966 | 2.665 / 2.761 |
| 32 | 0.323 / 1.589 | 0.208 / 0.645 | 2.868 / 25.784 | 2.750 / 2.847 |
| 33 | 0.303 / 1.530 | 0.220 / 0.391 | 2.941 / 25.872 | 2.836 / 2.932 |
| 48 | 0.365 / 1.162 | 0.297 / 0.610 | 4.233 / 27.153 | 4.115 / 4.212 |
| 49 | 0.412 / 1.644 | 0.321 / 0.640 | 4.311 / 27.240 | 4.201 / 4.297 |

D1 async 在 size>=15 时每组各有一个 `line+10ms` tail；Console 全部为 0。async S40 同时显示 `slow_poll_exh=0`、`yield_exh=0`，所以这些样本不能归因于 fallback 耗尽。当前数据没有定位 tail 的根因。

## 边界与后续

四份日志覆盖同版 benchmark 的 QEMU/D1 与 async/Console 组合。可比 section 已按同平台比较；能力不一致的 section 保留 `SKIPPED`、`UNSUPPORTED` 或 `N/A`。

仍需保留的边界如下：

| 项目 | 状态 | 说明 |
|---|---|---|
| D1 Console RX | UNSUPPORTED | 不能与 D1 async RX 横向比较 |
| RX fixed payload | SKIPPED | 四组均配置 `BENCH_RX_FIXED_BYTES=0` |
| D1 async size>=15 P99 tail | 保留 | 每组一次 24-27 ms 样本，根因未定位 |
| QEMU async S40 | N/A | telemetry counters 为 0 |
| Console S40 | UNSUPPORTED | polling backend 没有 async telemetry |
| CPU 使用率 | 未采集 | counter proxy 不能替代 CPU 百分比 |
| SMP 正确性 | 未声明 | 四组数据都不构成多 hart 证据 |
