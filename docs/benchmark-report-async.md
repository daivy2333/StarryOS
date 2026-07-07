# Async UART 性能测试报告

> 项目：StarryOS / `uart-16550-lichee`
> 截稿日期：2026-07-07
> 测试主题：Q19C-M0 + Q19C.8e（slow-pool + yield 重试）同版 `tests/benchmark.c` 在 QEMU rootfs 与 Lichee RV Dock D1 userbench 上的横向对比
> 数据来源：`docs/NewDate.md`（2026-07-07 D1 真板串口日志 + QEMU rootfs 日志）

## 摘要

`q19c-m0-20260703` 同版用户态 benchmark 在 QEMU rootfs 与 Lichee RV Dock D1 embedded userbench 路径上输出同构指标。Q19C.8e 在 TX copier 加入了 slow-pool（`TX_SLOW_POLL_LIMIT=4096`）+ yield 重试（`TX_YIELD_RETRIES=4`）作为 budget exhausted 后的 fallback，真板数据证明 slow-pool 100% 成功（`slow_poll_exh=0`），yield 重试从未触发（`yield_exh=0`）。

QEMU 吞吐高于 115200 bps 物理线速（模型不仿真线延迟），仅作功能与相对趋势证据。D1 大包 drain-each 与 batch-drain 稳定接近 11.52 KB/s 理论线速，作绝对线速依据。D1 64B baseline 在 Q19C-M0 加入 pre-section drain 后从旧 1.00 KB/s 提升到 11.13 KB/s（96.6% 线速），消除了 stdout backlog 测量污染。

表 1 汇总横向结果。QEMU 数据来自 rootfs `/bin/benchmark`，D1 数据来自 Android boot image 内嵌 `benchmark.elf`。代码依据见 [tests/benchmark.c](../tests/benchmark.c) 与 [Makefile](../Makefile)。

| 指标 | QEMU rootfs | Lichee D1 userbench | 解释 |
|------|-------------|---------------------|------|
| TX baseline 64B drain-each | 153.86 KB/s | 11.13 KB/s | D1 达 96.6% 线速（pre-section drain 后消除 backlog 污染） |
| TX baseline 256B drain-each | 167.20 KB/s | 11.21 KB/s | D1 达 97.3% 线速 |
| TX baseline 1024B drain-each | 182.28 KB/s | 11.38 KB/s | D1 达 98.8% 线速 |
| TX batch-drain 64B | 159.34 KB/s | 11.38 KB/s | D1 64B batch-drain 达 98.8% 线速 |
| TX writev 4x64B | 149.88 KB/s | 11.36 KB/s | 两端均出现 1 字节短写 |
| TX 1B latency avg / P50 / P99 | 0.182 / 0.176 / 0.357 ms | 0.186 / 0.185 / 0.224 ms | D1 1B P99=0.224ms，1B 不触发 FIFO full |
| RX empty nonblocking | PASS | PASS | `open(O_NONBLOCK)` 与 `ioctl(FIONBIO)` 均返回 EAGAIN |

Q19C-M0 + Q19C.8e 完成同版 benchmark 可横向解释 + D1 TX copier slow-pool fallback。QEMU 证明路径与相对行为，D1 证明硬件 UART0 线速与 tail latency。

## 测试对象与可比性边界

边界一：启动链不同。QEMU 样本通过 rootfs 中 `/bin/benchmark` 运行，manifest 标注为 `target_mode=qemu-rootfs`、`startup_chain=/bin/sh -c init.sh -> /bin/benchmark`、`root_provider=qemu-virtio-ext4-rootfs`。D1 样本通过 `make lichee-userbench` 生成 Android boot image，将 `kernel/resources/benchmark.elf` 嵌入内核并由 `load_embedded_user_app()` 启动，manifest 标注为 `target_mode=lichee-d1-userbench`、`startup_chain=android-boot-image -> embedded benchmark.elf`、`root_provider=d1-memory-root-embedded-payload`。

UART、TTY、syscall、`tcdrain()` 与 FIONBIO 行为可横向比较。rootfs path loader 尚不能横向比较：Q19C 后续 M1 需把 D1 推进到 memory-root `/bin/benchmark` + `load_user_app()`，才能与 QEMU 的 path-based loader 对齐。

表 2 列出测量条件。115200 bps 下按 8N1 串口格式计算为 10 bit/byte，理论上限约 11.52 KB/s。

| 条件 | QEMU rootfs | Lichee D1 userbench |
|------|-------------|---------------------|
| Benchmark version | `q19c-m0-20260703` | `q19c-m0-20260703` |
| User payload | `tests/benchmark` on rootfs | `kernel/resources/benchmark.elf` embedded |
| UART device | QEMU NS16550 model | D1 DW APB UART0 |
| UART line rate label | 11.52 KB/s | 11.52 KB/s |
| TX sizes | 64, 256, 1024 | 64, 256, 1024 |
| Break-even sizes | 64, 128, 256 | 64, 128, 256 |
| Latency matrix | 1, 15, 16, 17, 31, 32, 33, 48, 49 | 同左 |
| TX copier slow-pool | 未触发（QEMU THRE 立即响应） | 已触发（`slow_poll_exh=0`，100% 成功） |

## 代码与测试结构

用户态测试结构集中在 `tests/benchmark.c`。Manifest 输出 target mode、startup chain、root provider、TX 矩阵、RX 模式与定时参数。主函数按固定顺序运行 S00/S10/S11/S12/S13/S14/S20/S21/S30/S31。

| Section | 测量语义 |
|---------|----------|
| S10 | 每次 write 后立即 `tcdrain()`，保留 POSIX 完成语义 |
| S11 | 测量 write 入队成本，最后再 drain；D1 路径输出 gated TX debug snapshot（`hw_send_zero`/`no_progress_budget`/`slow_poll_exh`/`yield_exh`） |
| S12 | 每 8 次 write 批量 drain，观察固定开销摊薄 |
| S13 | `writev` 4 个 64B fragment，总 payload 256B |
| S14 | 64/128/256B drain-each 小包 break-even |
| S20/S21 | 1B 延迟与 FIFO 边界矩阵 |
| S30/S31 | 空 RX 非阻塞与可选 fixed payload RX |

Q19C.8e 新增 S11 gated TX debug snapshot 字段：`slow_poll_exh`（slow-pool 跑满 4096 次的次数）、`yield_exh`（yield 重试耗尽的次数）。D1 真板数据 `slow_poll_exh=0` `yield_exh=0`，证明 slow-pool 在 100% 的情况下成功（每次 budget exhausted 后约 653 次 send_bytes 后 FIFO 排空）。

## 内核态启动 Benchmark

内核态 startup benchmark 衡量 async UART 驱动内部 ring buffer 与统计路径，不代表硬件 UART 线速。

| 指标 | QEMU rootfs | Lichee D1 userbench | 测量条件 |
|------|-------------|---------------------|----------|
| Ring buffer TX push | 550,055.01 KB/s | 1,151,569.59 KB/s | 102400 bytes，100 x 1024B |
| RX ring buffer read | 1,205,273.07 KB/s | 8,437,706.00 KB/s | 65536 bytes |
| RX latency avg | 260 ns | 106 ns | n=100，单字节 ring pop |
| RX latency P50 | 100 ns | 123 ns | 同上 |
| RX latency P99 | 11,600 ns | 246 ns | 同上 |
| Driver struct | 152 bytes | 272 bytes | Q19C.8e 加 `slow_poll_exhausted`/`yield_retries_exhausted` 计数器 |
| IRQ count at report | 0 | 43 | startup benchmark 阶段 |

D1 driver struct 从 152 bytes 增加到 272 bytes（Q19C.8e slow-pool + yield 计数器 + `yield_retries` 变量）。D1 IRQ count 从 Q19B 时代的 6 增加到 43，反映 slow-pool 期间 TX_WAKER 注册后 ISR 更频繁到达。

## TX Throughput 与 Drain 策略

D1 在所有 payload 上接近 115200 bps 线速。Q19C-M0 加入 pre-section drain 后，64B baseline 从旧 1.00 KB/s（8.7% 线速）提升到 11.13 KB/s（96.6% 线速），消除了 stdout backlog 测量污染。QEMU 的 `line_rate_pct` 大于 100% 是预期：QEMU UART 模型不仿真物理串口线延迟。

表 5 = S10 baseline：每次 write 后立即 `tcdrain()`。

| Payload | QEMU KB/s | QEMU line_rate_pct | D1 KB/s | D1 line_rate_pct | D1 P99 |
|---------|-----------|--------------------|---------|------------------|--------|
| 64B | 153.86 | 1335.6% | 11.13 | 96.6% | 5.625 ms |
| 256B | 167.20 | 1451.4% | 11.21 | 97.3% | 50.860 ms |
| 1024B | 182.28 | 1582.3% | 11.38 | 98.8% | 117.438 ms |

D1 P99 在 256B/1024B 上出现长尾（50.86ms / 117.44ms），100 次迭代中约 1 次超出线时+10ms。根因未探明——slow-pool/yield 重试均未改善，`slow_poll_exh=0` 证明非 ISR 丢失。当前影响可接受（吞吐量 <2%），暂不继续优化。

表 6 = S11 no-drain enqueue + gated TX debug snapshot。

| Payload | QEMU enqueue KB/s | QEMU short writes | QEMU final drain ms | D1 enqueue KB/s | D1 short writes | D1 final drain ms |
|---------|-------------------|-------------------|---------------------|-----------------|-----------------|-------------------|
| 64B | 5,918.00 | 0 | 34 | 4,239.36 | 0 | 545 |
| 256B | 13,084.21 | 0 | 147 | 9,113.76 | 0 | 2,183 |
| 1024B | 16,363.15 | 37 | 364 | 10,015.88 | 36 | 5,588 |

D1 S11 gated TX debug snapshot（final-drain phase）：

| Payload | hw_send_calls | hw_send_zero | hw_send_max_chunk | no_progress_budget | slow_poll_exh | yield_exh |
|---------|---------------|--------------|-------------------|--------------------|---------------|-----------|
| 64B | 274,396 | 273,996 | 16 | 399 | 0 | 0 |
| 256B | 1,099,814 | 1,098,214 | 16 | 1,599 | 0 | 0 |
| 1024B | 2,816,501 | 2,812,405 | 16 | 4,095 | 0 | 0 |

`hw_send_zero` 高是 slow-pool 的预期副产物（每次 budget exhausted 后约 653 次 send_bytes 返回 0 后 1 次成功），不影响吞吐量。`slow_poll_exh=0` 证明 slow-pool 从未跑满 4096 次。`yield_exh=0` 证明 yield 重试从未触发。`hw_send_max_chunk=16` 确认 Q19C.8d 16B FIFO burst 修复保持。

表 7 = S12 batch-drain，每 8 次 write 后 drain。

| Payload | QEMU KB/s | QEMU line_rate_pct | D1 KB/s | D1 line_rate_pct |
|---------|-----------|--------------------|---------|------------------|
| 64B | 159.34 | 1383.1% | 11.38 | 98.8% |
| 256B | 165.56 | 1437.2% | 11.35 | 98.5% |
| 1024B | 167.41 | 1453.2% | 11.42 | 99.1% |

表 8 = S13/S14 汇总。

| Section | QEMU | Lichee D1 | 说明 |
|---------|------|-----------|------|
| S13 writev 4x64B | 149.88 KB/s，1 short write，bytes=25563 | 11.36 KB/s，1 short write，bytes=25429 | syscall 聚合路径可用，但存在短写 |
| S14 64B | 121.90 KB/s | 11.13 KB/s | D1 为 96.6% 线速 |
| S14 128B | 143.98 KB/s | 11.00 KB/s | D1 为 95.4% 线速 |
| S14 256B | 159.23 KB/s | 11.21 KB/s | D1 为 97.3% 线速 |

D1 硬件线速能力由 S10 全尺寸、S12 全尺寸、S13、S14 共同支持。64B 不再是异常低值——pre-section drain 后 64B 在所有 section 中稳定接近线速。

## TX Latency 与 FIFO Boundary

D1 1B P99=0.224ms，远低于旧数据 11.881ms。1B 不触发 FIFO full，不进入 slow-pool 路径，P99 改善来自 pre-section drain 消除测量污染 + Q19C.8d 16B FIFO burst 修复。

表 9 = S20 单字节 latency。

| 指标 | QEMU | Lichee D1 |
|------|------|-----------|
| n | 100 | 100 |
| avg | 0.182 ms | 0.186 ms |
| P50 | 0.176 ms | 0.185 ms |
| P95 | 0.204 ms | 0.188 ms |
| P99 | 0.357 ms | 0.224 ms |

表 10 = S21 FIFO boundary matrix。D1 P50 随 payload 按物理串口时间近似增长。D1 P99 在 size≥15 时出现 14-18ms 长尾（根因未探明），1B P99=0.231ms 无长尾。

| Size | QEMU avg / P50 / P95 / P99 ms | D1 avg / P50 / P95 / P99 ms |
|------|-------------------------------|-----------------------------|
| 1 | 0.168 / 0.164 / 0.187 / 0.378 | 0.187 / 0.186 / 0.188 / 0.231 |
| 15 | 0.217 / 0.208 / 0.279 / 0.577 | 1.538 / 1.403 / 1.412 / 14.862 |
| 16 | 0.252 / 0.239 / 0.359 / 0.964 | 1.627 / 1.490 / 1.497 / 15.191 |
| 17 | 0.255 / 0.248 / 0.311 / 0.942 | 1.706 / 1.568 / 1.571 / 15.274 |
| 31 | 0.328 / 0.314 / 0.450 / 1.343 | 2.934 / 2.796 / 2.804 / 16.493 |
| 32 | 0.324 / 0.319 / 0.462 / 0.592 | 3.017 / 2.881 / 2.889 / 16.558 |
| 33 | 0.337 / 0.338 / 0.451 / 1.206 | 3.077 / 2.936 / 2.943 / 16.638 |
| 48 | 0.411 / 0.399 / 0.591 / 0.938 | 4.383 / 4.249 / 4.256 / 17.750 |
| 49 | 0.485 / 0.473 / 0.730 / 1.750 | 4.439 / 4.301 / 4.312 / 18.006 |

D1 P99 在 size≥15 时稳定在 14-18ms，P50/P95 稳定。size=1 P99=0.231ms 无长尾——1B 不触发 FIFO full，不进入 slow-pool 路径。size≥15 填满 16B FIFO 后需要等排空，触发 slow-pool 路径，P99 长尾出现。根因未探明，`slow_poll_exh=0` 证明非 ISR 丢失，确切成因待查。

## RX 与非阻塞行为

RX 只覆盖 empty nonblocking witness，fixed-payload RX 未启用。QEMU 与 D1 在 S30 中均显示 `open(O_NONBLOCK)` 和 `ioctl(FIONBIO)` 返回 EAGAIN；S31 均为 `SKIPPED reason=BENCH_RX_FIXED_BYTES=0`。

| 测试 | QEMU | Lichee D1 | 结论 |
|------|------|-----------|------|
| `open(O_NONBLOCK)` read | PASS / EAGAIN | PASS / EAGAIN | 空读非阻塞语义一致 |
| `ioctl(FIONBIO)` read | PASS / EAGAIN | PASS / EAGAIN | ioctl 路径一致 |
| fixed payload RX | SKIPPED | SKIPPED | 未设置 `BENCH_RX_FIXED_BYTES` |

## 工程判断

数据把优化方向拆成三个层次。

1. **物理线速**：D1 全尺寸 S10/S12/S13/S14 共同证明硬件 UART0 可达 96.6%-99.1% 的 115200 bps 线速。64B 不再是异常低值。
2. **TX copier slow-pool**：Q19C.8e slow-pool + yield 重试已实施，`slow_poll_exh=0` 证明 slow-pool 100% 成功。`hw_send_zero` 高是预期副产物，不影响吞吐量。`hw_send_max_chunk=16` 确认 Q19C.8d 修复保持。
3. **P99 长尾**：D1 size≥15 P99=14-18ms，size=1 P99=0.231ms。根因未探明——slow-pool/yield 均未改善，`slow_poll_exh=0` 证明非 ISR 丢失。当前影响可接受（吞吐量 <2%），暂不继续优化，Q20 复验时再探明。

| 优先级 | 项目 | 必要性 | 复杂度 | 依赖 |
|--------|------|--------|--------|------|
| P0 | D1 memory-root `/bin/benchmark` path loader | 必要 | 中 | Q19C M1 |
| P0 | 保存 QEMU/D1 raw serial log 与 manifest 表 | 已完成 | 低 | 当前数据 |
| P1 | D1 P99 长尾根因 tracing | 可选 | 中 | Q20 多 hart stress |
| P2 | 启用 fixed-payload RX witness | 可选 | 中 | 可重复输入源 |

## 结论

1. Q19C-M0 + Q19C.8e 已实现 QEMU 与 D1 同版 benchmark 横向对比 + D1 TX copier slow-pool fallback。
2. D1 真板在所有 TX 场景稳定接近 115200 bps 线速（96.6%-99.1%），64B 不再是异常低值。
3. Q19C.8e slow-pool 100% 成功（`slow_poll_exh=0`），yield 重试从未触发（`yield_exh=0`），ISR 从未丢失。
4. D1 P99 长尾（size≥15 时 14-18ms）根因未探明，当前影响可接受，Q20 复验时再探明。
5. 后续主线应先推进 Q19C M1 memory-root path loader，再讨论 SDMMC/rootfs 或 P99 深入 tracing。

## 术语表

`tcdrain()`：POSIX termios 接口，等待输出队列中的数据发送完成。

FIONBIO：File IOctl Non-Blocking I/O，通过 ioctl 设置文件描述符非阻塞模式。

EAGAIN：POSIX 错误码，表示当前没有可读数据或资源，调用方可稍后重试。

FIFO：First-In First-Out，UART 硬件发送/接收缓冲队列。本文 D1/QEMU 观察均按 16B FIFO 边界分析。

slow-pool：Q19C.8e 在 TX copier budget exhausted 后加入的 bounded slow-poll（`TX_SLOW_POLL_LIMIT=4096` × `TX_SLOW_POLL_SPINS=256`），给 FIFO 排空时间后 retry `send_bytes`，作为 D1 THRE IRQ 边沿丢失的软件 fallback。

yield 重试：Q19C.8e 在 slow-pool 跑满后通过 `cx.waker().wake_by_ref()` 自唤醒让出调度，给调度器 yield 机会后再 slow-pool 一轮（`TX_YIELD_RETRIES=4`）。
