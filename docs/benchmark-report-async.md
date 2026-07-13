# Async UART 性能测试报告

> 项目：StarryOS / `uart-16550-lichee`
> 日期：2026-07-13
> 范围：Q20 benchmark gap closure
> 数据来源：[qemu-rootfs.log](../.claude/analysis/q20-evidence/qemu-rootfs.log)、[d1-fullbench-command.log](../.claude/analysis/q20-evidence/d1-fullbench-command.log)

## 结论

Q20 已补齐同版 benchmark 的 QEMU 与 D1 对比证据。D1 真板在 S10/S12/S13/S14 的 TX 场景中达到 95.2%-99.1% 的 115200 bps 线速，64B、128B、256B、1024B 都不再出现低吞吐异常。

1B 延迟稳定：D1 S20 P99 为 0.221 ms，S21 size=1 P99 为 0.245 ms，均无 `line+10ms` tail。D1 在 size>=15 时仍有每组 1 次 24-27 ms 级 tail，这是保留的已知边界，不影响本次吞吐结论。

D1 的 S40 TX counter proxy 可用，`slow_poll_exh=0`、`yield_exh=0`，说明观测到的 TX forward-progress fallback 没有耗尽。QEMU counter proxy 因 telemetry counter 为 0，标记为 `not-available`。

RX fixed payload 不纳入 Q20 gate，S31 按用户决策保持 `SKIPPED reason=BENCH_RX_FIXED_BYTES=0`。本文不声明 SMP 正确性。

## 测试环境

| 项目 | QEMU rootfs | D1 fullbench command-entry |
|------|-------------|----------------------------|
| Benchmark version | `q19c-m0-20260703` | `q19c-m0-20260703` |
| target_mode | `qemu-rootfs` | `lichee-d1-fullbench` |
| startup_chain | `/bin/sh -c init.sh -> /bin/benchmark` | `android-boot-image -> memory-root /bin/benchmark -> eager_elf_mapping` |
| root_provider | `qemu-virtio-ext4-rootfs` | `d1-memory-root-path` |
| device | `/dev/console` | `/dev/console` |
| UART line rate label | 11.52 KB/s | 11.52 KB/s |
| 原始日志 | [qemu-rootfs.log](../.claude/analysis/q20-evidence/qemu-rootfs.log) | [d1-fullbench-command.log](../.claude/analysis/q20-evidence/d1-fullbench-command.log) |

QEMU UART 模型不仿真物理线延迟，`line_rate_pct` 大于 100% 是预期现象；QEMU 只作为路径、接口和相对行为证据。D1 数据来自物理 UART0，是绝对线速依据。

统计口径：P50/P95/P99 都来自每组 100 次样本排序后的分位值。P50 代表典型耗时，P99 用来观察少数慢样本；`P99/P50` 越高，说明尾部抖动越明显。`line+10ms` tail 指单次耗时超过该 payload 的理论线时再加 10 ms。

## 吞吐

S10 是 drain-each baseline。测试代码对 64B、256B、1024B 各执行 100 次 `write_full()`，每次写完立即 `tcdrain()`，计入从写入开始到 drain 返回的时间。这个测试测“用户认为已发送完成”的吞吐和尾部延迟。

| Payload | QEMU KB/s | QEMU P99/P50 | D1 KB/s | D1 line_rate_pct | D1 P99/P50 | D1 tail |
|---------|-----------|--------------|---------|------------------|------------|---------|
| 64B | 141.04 | 2.20 | 11.14 | 96.7% | 1.00 | 0 |
| 256B | 177.79 | 2.21 | 11.20 | 97.2% | 2.46 | 1 |
| 1024B | 175.03 | 1.56 | 11.38 | 98.8% | 1.38 | 1 |

S12 是 batch-drain。测试代码连续写 100 次，每 8 次调用一次 `tcdrain()`，末尾再补一次 drain。这个测试测批量提交能否摊薄 drain 调用开销，同时仍等待硬件发送完成。

| Payload | QEMU KB/s | D1 KB/s | D1 line_rate_pct |
|---------|-----------|---------|------------------|
| 64B | 165.66 | 11.38 | 98.8% |
| 256B | 181.94 | 11.35 | 98.5% |
| 1024B | 177.43 | 11.42 | 99.1% |

S13 测 `writev()` fragment aggregation。测试代码构造 4 个 64B `iovec`，每轮一次 `writev()` 后 drain，用来确认分片写入路径和短写行为。S14 测小包 break-even，使用 64B、128B、256B drain-each，观察小包尺寸变化对吞吐和 tail 的影响。

| Section | QEMU | D1 | D1 line_rate_pct | 备注 |
|---------|------|----|------------------|------|
| S13 writev 4x64B | 156.50 KB/s | 11.36 KB/s | 98.6% | 两端各 1 次 short write |
| S14 64B | 124.24 KB/s | 11.15 KB/s | 96.8% | 无 `line+10ms` tail |
| S14 128B | 136.58 KB/s | 10.97 KB/s | 95.2% | 1 次 `line+10ms` tail |
| S14 256B | 160.96 KB/s | 11.20 KB/s | 97.2% | 1 次 `line+10ms` tail |

S11 no-drain 测用户态入队成本。测试代码先 reset TX debug counter，再连续写 100 次，计时只覆盖 write loop；随后在计时外执行 final `tcdrain()` 并采集 enqueue/final-drain 两个 snapshot。这个测试把“写入 ring buffer 的速度”和“硬件最终排空时间”分开看。

| Payload | QEMU enqueue KB/s | QEMU short writes | QEMU final drain ms | D1 enqueue KB/s | D1 short writes | D1 final drain ms |
|---------|-------------------|-------------------|---------------------|-----------------|-----------------|-------------------|
| 64B | 6039.23 | 0 | 32 | 4290.67 | 0 | 545 |
| 256B | 14034.69 | 0 | 137 | 8955.02 | 0 | 2183 |
| 1024B | 18170.98 | 36 | 350 | 10310.75 | 36 | 5588 |

## 稳定性

本轮 QEMU 与 D1 benchmark 均执行到 `Done.`，D1 进程退出码为 0。S10/S12/S13/S14/S20/S21 的 `drain_errors` 均为 0。

S30 测空 RX 的非阻塞语义。测试代码先用 `O_NONBLOCK` 打开 `/dev/console` 并读 16B，再用普通 open 后 `ioctl(FIONBIO)` 切到非阻塞并读 16B；空缓冲时预期是 `EAGAIN`。S31 是可选 fixed-payload RX witness，只有 `BENCH_RX_FIXED_BYTES > 0` 时才读固定字节数，本轮配置为 0。

| 测试事项 | QEMU | D1 | 结论 |
|----------|------|----|------|
| S30 `open(O_NONBLOCK)` | PASS / EAGAIN | PASS / EAGAIN | 空 RX 非阻塞语义一致 |
| S30 `ioctl(FIONBIO)` | PASS / EAGAIN | PASS / EAGAIN | ioctl 非阻塞路径一致 |
| S31 fixed payload RX | SKIPPED | SKIPPED | Q20 不纳入 fixed RX |
| S13 writev short write | 1 | 1 | 调用方必须接受短写 |
| S11 1024B short writes | 36 | 36 | no-drain 压满 ring buffer 后的预期边界 |

D1 S11 final-drain 1024B snapshot 显示 `ring_empty=1`、`copier_active=0`、`transmitter_empty=1`，说明最终 drain 后发送路径收敛。对应计数为 `user_acc=65536`、`ring_pop_bytes=65536`、`hw_send_bytes=65536`。

## CPU 与计数器代理

当前 benchmark 没有采集 CPU 使用率。S40 提供的是 TX counter proxy，用来判断发送路径是否大量空转、fallback 是否耗尽，以及 QEMU/D1 telemetry 是否可解释。

S40 在所有用户态 benchmark 结束后读取 `UART_TXDBG_SNAPSHOT`。测试代码输出 user-push、ring-pop、hw-send、no-progress 和 drain-state 计数，再派生每次调用平均字节数、每 KB 空发送次数等 proxy。它测的是 TX 路径行为，不是 CPU 百分比。

| 指标 | QEMU | D1 |
|------|------|----|
| telemetry_available | 0 | 1 |
| user_calls | 0 | 2357 |
| user_acc | 0 | 301251 |
| ring_pop_calls | 0 | 1621 |
| ring_pop_bytes | 0 | 301158 |
| hw_send_calls | 0 | 12248299 |
| hw_send_bytes | 0 | 301158 |
| hw_send_zero | 0 | 12228983 |
| hw_send_max_chunk | 0 | 16 |
| no_progress_budget | 0 | 17861 |
| slow_poll_exh | 0 | 0 |
| yield_exh | 0 | 0 |

QEMU counter 为 0，因此 S40 输出 `proxy=derived status=not-available reason=telemetry-counters-are-zero`。D1 counter 可用：`bytes_per_user_call=127.8`、`bytes_per_ring_pop=185.8`、`bytes_per_hw_send=0.025`、`zero_per_kb=41581.1`、`no_progress_per_kb=60.7`。

`hw_send_zero` 高是 slow-poll 路径下频繁探测 TX FIFO 的观测结果，不能当作 CPU 占用率。`slow_poll_exh=0` 与 `yield_exh=0` 表示本轮 fallback 未耗尽。

启动阶段 ring buffer benchmark 测驱动内部能力，不代表 UART 物理线速。它在内核初始化后运行，覆盖 TX ring buffer push、RX ring buffer pop、单字节 RX pop 延迟、buffer 配置、NAPI 配置和 IRQ 计数。

这组测试绕过用户态 `write()/tcdrain()`、syscall、调度等待和串口线速限制。它说明驱动内部队列的处理能力比用户态 115200 bps 表现高得多：D1 TX ring buffer write 为 1155388.15 KB/s，RX ring buffer read 为 8303061.75 KB/s。用户态吞吐仍以物理线速为准。

| 指标 | QEMU | D1 |
|------|------|----|
| TX ring buffer write | 403388.46 KB/s | 1155388.15 KB/s |
| RX ring buffer read | 1052631.58 KB/s | 8303061.75 KB/s |
| RX latency P99 | 11800 ns | 205 ns |
| IRQ count | 0 | 42 |
| IRQ frequency | 未输出 | 485263.02 IRQ/s |

## 延迟

S20 是单字节 `write + tcdrain` 延迟。测试代码循环 100 次，每次写 1B 后立即 drain，并记录单次耗时。这个测试测最小 payload 下的同步完成延迟，通常不触发 FIFO 满路径。

| 指标 | QEMU | D1 |
|------|------|----|
| n | 100 | 100 |
| avg | 0.168 ms | 0.190 ms |
| P50 | 0.162 ms | 0.189 ms |
| P95 | 0.227 ms | 0.197 ms |
| P99 | 0.301 ms | 0.221 ms |
| P99/P50 | 1.86 | 1.17 |
| `line+10ms` tail | 0 | 0 |

S21 是 FIFO boundary matrix。测试代码对 1、15、16、17、31、32、33、48、49B 各写 100 次，每次写完 drain；这些尺寸围绕 16B FIFO 边界和其倍数展开。这个测试测 payload 接近或跨过 FIFO 边界时的延迟增长和 tail。

| Size | QEMU P50/P99 ms | QEMU P99/P50 | D1 P50/P99 ms | D1 P99/P50 | D1 `line+10ms` tail |
|------|-----------------|--------------|---------------|------------|----------------------|
| 1 | 0.166 / 0.353 | 2.13 | 0.190 / 0.245 | 1.29 | 0 |
| 15 | 0.207 / 1.484 | 7.18 | 1.421 / 23.990 | 16.88 | 1 |
| 16 | 0.231 / 1.515 | 6.57 | 1.507 / 24.672 | 16.37 | 1 |
| 17 | 0.225 / 2.290 | 10.16 | 1.581 / 24.758 | 15.66 | 1 |
| 31 | 0.333 / 2.129 | 6.39 | 2.786 / 25.951 | 9.31 | 1 |
| 32 | 0.314 / 1.515 | 4.83 | 2.867 / 25.777 | 8.99 | 1 |
| 33 | 0.306 / 0.969 | 3.17 | 2.937 / 25.858 | 8.80 | 1 |
| 48 | 0.391 / 1.760 | 4.50 | 4.232 / 26.974 | 6.37 | 1 |
| 49 | 0.387 / 1.564 | 4.05 | 4.304 / 27.234 | 6.33 | 1 |

D1 size>=15 的 P99 tail 保留为已知边界。S40 同时显示 `slow_poll_exh=0`、`yield_exh=0`，所以本轮数据不能把 tail 归因于 fallback 耗尽。

## 边界与后续

Q20 已完成 benchmark 证据链：同版 benchmark、QEMU raw evidence、D1 raw evidence、TX jitter ratio、S40 counter proxy、RX empty nonblocking witness。

仍需保留的边界如下：

| 项目 | 状态 | 说明 |
|------|------|------|
| RX fixed payload | 未做 | 不好做，不纳入 Q20 gate |
| D1 size>=15 P99 tail | 保留 | 24-27 ms 级 tail，作为 Q23 输入 |
| QEMU S40 counter | 不可用 | telemetry counters 为 0 |
| CPU 使用率 | 未采集 | 当前只有 counter proxy，不能当作 CPU 百分比 |
| SMP 正确性 | 未声明 | 交给后续 SMP gate |
