# Async UART 性能测试报告

> 项目：StarryOS / `uart-16550-lichee`
> 截稿日期：2026-07-07
> 测试主题：Q19C-M0 同版 `tests/benchmark.c` 在 QEMU rootfs 与 Lichee RV Dock D1 userbench 上的横向对比
> 数据来源：本文末次重写前追加在本文件末尾的 2026-07-07 QEMU 与真板串口日志

## 摘要

`q19c-m0-20260703` 同版用户态 benchmark 在 QEMU rootfs 与 Lichee RV Dock D1 embedded userbench 路径上输出同构指标。可区分：软件入队成本、批量 drain 策略、硬件 UART 线速、文件/启动路径差异。

QEMU 吞吐高于 115200 bps 物理线速（模型不仿真线延迟），仅作功能与相对趋势证据。D1 大包 drain-each 与 batch-drain 稳定接近 11.52 KB/s 理论线速，作绝对线速依据。

表 1 汇总横向结果。QEMU 数据来自 rootfs `/bin/benchmark`，D1 数据来自 Android boot image 内嵌 `benchmark.elf`。代码依据见 [tests/benchmark.c:122-152](../tests/benchmark.c#L122-L152)、[tests/benchmark.c:525-544](../tests/benchmark.c#L525-L544) 与 [Makefile:44-56](../Makefile#L44-L56)。

| 指标 | QEMU rootfs | Lichee D1 userbench | 解释 |
|------|-------------|---------------------|------|
| TX baseline 64B drain-each | 133.33 KB/s | 1.00 KB/s | 小包对固定开销高度敏感，D1 首个 64B baseline 明显偏低 |
| TX baseline 256B drain-each | 165.25 KB/s | 11.33 KB/s | D1 达到 98.4% 线速 |
| TX baseline 1024B drain-each | 177.55 KB/s | 11.38 KB/s | D1 达到 98.8% 线速 |
| TX batch-drain 64B | 170.77 KB/s | 11.08 KB/s | D1 64B 通过批量 drain 达到 96.2% 线速 |
| TX writev 4x64B | 162.20 KB/s | 11.26 KB/s | 两端均出现 1 字节短写，统计按实际写入字节数计算 |
| TX 1B latency avg / P50 | 0.159 ms / 0.148 ms | 0.306 ms / 0.188 ms | D1 P99 受调度或中断尾延迟拉高 |
| RX empty nonblocking | PASS | PASS | `open(O_NONBLOCK)` 与 `ioctl(FIONBIO)` 均返回 EAGAIN |

Q19C-M0 完成同版 benchmark 可横向解释的第一步。本报告不再用 QEMU 吞吐声明物理性能：QEMU 证明路径与相对行为，D1 证明硬件 UART0 线速与 tail latency。

## 测试对象与可比性边界

边界一：启动链不同。QEMU 样本通过 rootfs 中 `/bin/benchmark` 运行，manifest 标注为 `target_mode=qemu-rootfs`、`startup_chain=/bin/sh -c init.sh -> /bin/benchmark`、`root_provider=qemu-virtio-ext4-rootfs`。D1 样本通过 `make lichee-userbench` 生成 Android boot image，将 `kernel/resources/benchmark.elf` 嵌入内核并由 `load_embedded_user_app()` 启动，manifest 标注为 `target_mode=lichee-d1-userbench`、`startup_chain=android-boot-image -> embedded benchmark.elf`、`root_provider=d1-memory-root-embedded-payload`。

UART、TTY、syscall、`tcdrain()` 与 FIONBIO 行为可横向比较。rootfs path loader 尚不能横向比较：D1 embedded userbench 启动路径由 [kernel/src/entry.rs:163-226](../kernel/src/entry.rs#L163-L226) 实现；Q19C 后续 M1 需把 D1 推进到 memory-root `/bin/benchmark` + `load_user_app()`，才能与 QEMU 的 path-based loader 对齐。

表 2 列出测量条件。115200 bps 下按 8N1 串口格式计算为 10 bit/byte，理论上限约 11.52 KB/s。该值是物理上限，不是 QEMU 模型的限制。

| 条件 | QEMU rootfs | Lichee D1 userbench |
|------|-------------|---------------------|
| Benchmark version | `q19c-m0-20260703` | `q19c-m0-20260703` |
| User payload | `tests/benchmark` on rootfs | `kernel/resources/benchmark.elf` embedded |
| UART device | QEMU NS16550 model | D1 DW APB UART0 |
| UART line rate label | 11.52 KB/s | 11.52 KB/s |
| TX sizes | 64, 256, 1024 | 64, 256, 1024 |
| Break-even sizes | 64, 128, 256 | 64, 128, 256 |
| Latency matrix | 1, 15, 16, 17, 31, 32, 33, 48, 49 | 同左 |

横向结论只在同一测试 section 内比较，不跨启动路径推断 rootfs 能力。D1 embedded userbench 是硬件 UART 性能证据；D1 path loader 证据属 Q19C M1。

## 代码与测试结构

用户态测试结构集中在 `tests/benchmark.c`。Manifest 由 [tests/benchmark.c:122-152](../tests/benchmark.c#L122-L152) 输出，携带 target mode、startup chain、root provider、TX 矩阵、RX 模式与定时参数。主函数按固定顺序运行 S00/S10/S11/S12/S13/S14/S20/S21/S30/S31，见 [tests/benchmark.c:525-544](../tests/benchmark.c#L525-L544)。

表 3 对应测量语义。S10 是 baseline；S11/S12 拆分软件入队成本与 drain 策略；S13 检查 `writev` 聚合路径；S14 专门观察 64/128/256 小包 break-even；S20/S21 观察 `tcdrain()` 延迟；S30/S31 覆盖 RX 非阻塞行为与可选 fixed payload witness。

| Section | 代码位置 | 测量语义 |
|---------|----------|----------|
| S10 | [tests/benchmark.c:156-193](../tests/benchmark.c#L156-L193) | 每次 write 后立即 `tcdrain()`，保留 POSIX 完成语义 |
| S11 | [tests/benchmark.c:195-236](../tests/benchmark.c#L195-L236) | 测量 write 入队成本，最后再 drain |
| S12 | [tests/benchmark.c:238-284](../tests/benchmark.c#L238-L284) | 每 8 次 write 批量 drain，观察固定开销摊薄 |
| S13 | [tests/benchmark.c:286-331](../tests/benchmark.c#L286-L331) | `writev` 4 个 64B fragment，总 payload 256B |
| S14 | [tests/benchmark.c:333-370](../tests/benchmark.c#L333-L370) | 64/128/256B drain-each 小包 break-even |
| S20/S21 | [tests/benchmark.c:372-433](../tests/benchmark.c#L372-L433) | 1B 延迟与 FIFO 边界矩阵 |
| S30/S31 | [tests/benchmark.c:435-522](../tests/benchmark.c#L435-L522) | 空 RX 非阻塞与可选 fixed payload RX |

内核态启动 benchmark 由 [kernel/src/drivers/bench.rs:15-85](../kernel/src/drivers/bench.rs#L15-L85) 输出，包括 ring buffer TX、内存占用、NAPI 配置与 IRQ 统计；RX throughput 与 latency 分别由 [kernel/src/drivers/bench.rs:90-117](../kernel/src/drivers/bench.rs#L90-L117) 和 [kernel/src/drivers/bench.rs:119-163](../kernel/src/drivers/bench.rs#L119-L163) 输出。构建路径由 [Makefile:44-56](../Makefile#L44-L56) 注入 QEMU/D1 manifest 宏，D1 boot image 打包由 [Makefile:93-99](../Makefile#L93-L99) 完成。

数据已具备可审计的代码来源：测试矩阵来自 `tests/benchmark.c`，内核 ring buffer 指标来自 `kernel/src/drivers/bench.rs`，QEMU/D1 模式差异来自 Makefile 与 `entry.rs`。

## 内核态启动 Benchmark

内核态 startup benchmark 衡量 async UART 驱动内部 ring buffer 与统计路径，不代表硬件 UART 线速。QEMU 与 D1 结果均达数十万到数百万 KB/s，远高于 11.52 KB/s 物理线速；该数字只说明软件 ring buffer 不是当前端到端瓶颈。TX 数据量固定 102400 bytes，RX throughput 读取 65536 bytes，见 [kernel/src/drivers/bench.rs:20-38](../kernel/src/drivers/bench.rs#L20-L38) 与 [kernel/src/drivers/bench.rs:90-117](../kernel/src/drivers/bench.rs#L90-L117)。

表 4 列出数据。D1 ring buffer 读写数字高于 QEMU，但二者都属于内存路径测量；不能推出 D1 物理 UART 比 QEMU 更快。

| 指标 | QEMU rootfs | Lichee D1 userbench | 测量条件 |
|------|-------------|---------------------|----------|
| Ring buffer TX push | 525486.07 KB/s | 1127611.83 KB/s | 102400 bytes，100 x 1024B |
| RX ring buffer read | 1075630.25 KB/s | 8172647.17 KB/s | 65536 bytes |
| RX latency avg | 313 ns | 102 ns | n=100，单字节 ring pop |
| RX latency P50 | 100 ns | 82 ns | 同上 |
| RX latency P99 | 14600 ns | 205 ns | 同上 |
| Driver struct | 152 bytes | 152 bytes | `size_of_val(driver.as_ref())` |
| IRQ count at report | 0 | 6 | startup benchmark 阶段 |

端到端性能问题不应优先归因于 ring buffer 本身。D1 串口吞吐在后续 S10/S12/S13 中被 11.52 KB/s 线速约束，内核态 ring buffer 有多个数量级余量；优化重点在 drain 策略、TTY/syscall 路径、调度尾延迟与 Q19C path loader 对齐。Ring buffer 不是端到端瓶颈，指标用于确认软件队列容量与统计路径健康，不声明硬件 UART 速度。

## TX Throughput 与 Drain 策略

D1 在 256B 及以上 payload 上接近 115200 bps 线速；64B 表现取决于 drain 策略与测试 section 顺序，不能仅凭单个 S10 结果判断。QEMU 的 `line_rate_pct` 大于 100% 是预期：QEMU UART 模型不仿真物理串口线延迟。该列在 QEMU 上只保留为日志一致性字段。

表 5 = S10 baseline：每次 write 后立即 `tcdrain()`。D1 的 256B/1024B 分别达 98.4%/98.8% 线速；64B 为 8.7% 线速，小包固定成本在该 section 中主导总耗时。

| Payload | QEMU KB/s | QEMU line_rate_pct | D1 KB/s | D1 line_rate_pct |
|---------|-----------|--------------------|---------|------------------|
| 64B | 133.33 | 1157.4% | 1.00 | 8.7% |
| 256B | 165.25 | 1434.5% | 11.33 | 98.4% |
| 1024B | 177.55 | 1541.2% | 11.38 | 98.8% |

表 6 = S11 no-drain enqueue。它不是物理吞吐测试，用来分离 write 入队速度与最终 drain 等待；应重点看 `short_writes` 与 `final_drain_ms`。两端在 1024B 下都出现 36 次短写：benchmark 必须按实际写入字节数统计，不能假设每次 write 完整接受 payload。

| Payload | QEMU enqueue KB/s | QEMU short writes | QEMU final drain ms | D1 enqueue KB/s | D1 short writes | D1 final drain ms |
|---------|-------------------|-------------------|---------------------|-----------------|-----------------|-------------------|
| 64B | 5584.35 | 0 | 37 | 5157.46 | 0 | 569 |
| 256B | 13791.58 | 0 | 143 | 10519.93 | 0 | 2202 |
| 1024B | 19137.52 | 36 | 353 | 12343.52 | 36 | 5628 |

表 7 = S12 batch-drain，每 8 次 write 后 drain。D1 64B 从 S10 的 1.00 KB/s 提升到 11.08 KB/s：64B 小包并非无法接近线速，per-iteration drain 固定开销在特定路径上放大。256B/1024B batch-drain 继续保持约 99% 线速。

| Payload | QEMU KB/s | QEMU line_rate_pct | D1 KB/s | D1 line_rate_pct |
|---------|-----------|--------------------|---------|------------------|
| 64B | 170.77 | 1482.4% | 11.08 | 96.2% |
| 256B | 174.54 | 1515.1% | 11.40 | 99.0% |
| 1024B | 174.61 | 1515.7% | 11.39 | 98.9% |

表 8 = S13/S14 汇总。`writev` 4 x 64B 在两端都出现 1 字节短写，报告使用 `bytes=25599` 而非理论 25600。S14 的 D1 64/128/256B 均达 93.7% 以上线速，与 S10 64B 的低值形成对照：64B 需重复采样或隔离 warm-up/state effect。

| Section | QEMU | Lichee D1 | 说明 |
|---------|------|-----------|------|
| S13 writev 4x64B | 162.20 KB/s，1 short write | 11.26 KB/s，1 short write | syscall 聚合路径可用，但存在短写 |
| S14 64B | 128.66 KB/s | 10.79 KB/s | D1 为 93.7% 线速 |
| S14 128B | 146.26 KB/s | 11.22 KB/s | D1 为 97.4% 线速 |
| S14 256B | 159.20 KB/s | 11.33 KB/s | D1 为 98.3% 线速 |

D1 硬件线速能力由 S10 256/1024、S12 全尺寸、S13、S14 共同支持。后续优化不应只追更高 enqueue KB/s，应优先减少 64B drain-each 的固定开销波动，并解释 1024B 短写出现的具体边界。

## TX Latency 与 FIFO Boundary

D1 P50/P95 接近物理发送时间加软件固定开销，P99 远高于 QEMU：尾延迟受调度、中断或 drain wake 时序影响。QEMU 1B avg 0.159 ms，D1 0.306 ms；D1 P99 达 11.881 ms，QEMU P99 0.788 ms。

表 9 = S20 单字节 latency。每次写 1 字节并 `tcdrain()`，D1 理论发送时间约 0.0868 ms，其余为软件、调度与等待成本。

| 指标 | QEMU | Lichee D1 |
|------|------|-----------|
| n | 100 | 100 |
| avg | 0.159 ms | 0.306 ms |
| P50 | 0.148 ms | 0.188 ms |
| P95 | 0.180 ms | 0.194 ms |
| P99 | 0.788 ms | 11.881 ms |

表 10 = S21 FIFO boundary matrix。D1 P50 随 payload 按物理串口时间近似增长：16B P50 = 1.520 ms，32B P50 = 2.885 ms，48B P50 = 4.250 ms。QEMU 同样随 payload 增长，绝对值远低于物理 UART 时间。

| Size | QEMU avg / P50 / P95 / P99 ms | D1 avg / P50 / P95 / P99 ms |
|------|-------------------------------|-----------------------------|
| 1 | 0.151 / 0.142 / 0.170 / 0.680 | 0.307 / 0.188 / 0.190 / 12.053 |
| 15 | 0.212 / 0.205 / 0.271 / 0.481 | 1.506 / 1.434 / 1.442 / 8.876 |
| 16 | 0.215 / 0.205 / 0.276 / 0.824 | 1.591 / 1.520 / 1.528 / 8.982 |
| 17 | 0.238 / 0.238 / 0.282 / 0.587 | 1.679 / 1.604 / 1.611 / 9.047 |
| 31 | 0.323 / 0.308 / 0.473 / 1.105 | 2.875 / 2.800 / 2.807 / 10.262 |
| 32 | 0.287 / 0.278 / 0.421 / 1.035 | 2.959 / 2.885 / 2.892 / 10.438 |
| 33 | 0.320 / 0.309 / 0.468 / 0.898 | 3.046 / 2.970 / 2.978 / 10.524 |
| 48 | 0.396 / 0.398 / 0.567 / 1.232 | 4.324 / 4.250 / 4.259 / 11.775 |
| 49 | 0.414 / 0.414 / 0.646 / 0.853 | 4.406 / 4.335 / 4.344 / 11.804 |

D1 P99 在所有 size 上都接近 9-12 ms，P50/P95 稳定。常态路径接近物理发送时间，异常尾延迟是另一问题域。优化用户可感知交互延迟时，应优先采集调度点、DRAIN_WAKER 唤醒点、IRQ no-pending 次数与 task migration 信息，不应先调 FIFO size。

D1 P50/P95 证明 drain 完成路径常态稳定；P99 暴露尾延迟问题。QEMU 帮助发现相对趋势，不替代 D1 的实测 tail latency 证据。

## RX 与非阻塞行为

RX 只覆盖 empty nonblocking witness，fixed-payload RX 未启用。QEMU 与 D1 在 S30 中均显示 `open(O_NONBLOCK)` 和 `ioctl(FIONBIO)` 返回 EAGAIN；S31 均为 `SKIPPED reason=BENCH_RX_FIXED_BYTES=0`。非阻塞空读语义一致，但不能证明 RX 数据吞吐。

表 11 = RX 汇总。`FIONBIO` 是 File IOctl Non-Blocking I/O，`EAGAIN` 表示当前无数据且调用方可稍后重试。

| 测试 | QEMU | Lichee D1 | 结论 |
|------|------|-----------|------|
| `open(O_NONBLOCK)` read | PASS / EAGAIN | PASS / EAGAIN | 空读非阻塞语义一致 |
| `ioctl(FIONBIO)` read | PASS / EAGAIN | PASS / EAGAIN | ioctl 路径一致 |
| fixed payload RX | SKIPPED | SKIPPED | 未设置 `BENCH_RX_FIXED_BYTES` |

声明 RX 性能需启用 `BENCH_RX_FIXED_BYTES` 并提供可重复的输入源（人工固定 payload、主机串口注入或 loopback）。本报告只声明 RX 空读语义，不声明 RX throughput。

RX 语义 gate 已通过，RX 性能 gate 尚未执行。区分该点：empty EAGAIN 只能证明非阻塞控制流，不能证明接收数据路径性能。

## 工程判断

数据把优化方向拆成三个层次。

1. **物理线速**：D1 256B/1024B baseline、batch-drain、writev 与 break-even 共同证明硬件 UART0 可达约 98%-99% 的 115200 bps 线速。
2. **短包固定成本**：S10 64B 与 S14 64B 在 D1 上差异很大，64B 结果对测试顺序、drain 状态或 warm-up 敏感，需重复采样后再决定是否改驱动。
3. **尾延迟**：D1 P99 接近 9-12 ms，P50/P95 稳定，尾延迟应独立追踪。

下一步把 Q19C M1 memory-root path loader 接上，让 D1 从 `/bin/benchmark` 通过 `load_user_app()` 运行同一 payload。该步骤不是性能优化，是可比性补齐。没有它，D1 与 QEMU 的 rootfs/path loading 仍不是同一启动链。可选优化项：64B drain-each 多轮重复采样、`writev` 短写根因定位、DRAIN_WAKER 事件计数、固定 payload RX witness。

表 12 按必要性排序。复杂度是工程判断，不包含工时估计。

| 优先级 | 项目 | 必要性 | 复杂度 | 依赖 |
|--------|------|--------|--------|------|
| P0 | D1 memory-root `/bin/benchmark` path loader | 必要 | 中 | Q19C M1 |
| P0 | 保存 QEMU/D1 raw serial log 与 manifest 表 | 必要 | 低 | 当前数据 |
| P1 | 64B S10/S14 重复采样并固定运行顺序 | 必要 | 低 | 当前 benchmark |
| P1 | 定位 1024B no-drain 与 writev 短写 | 可选但建议 | 中 | syscall/TTY 写路径审计 |
| P2 | 启用 fixed-payload RX witness | 可选 | 中 | 可重复输入源 |
| P2 | D1 P99 tail latency tracing | 可选 | 中 | IRQ/waker/scheduler trace |

性能优化不应先扩大到 SDMMC/rootfs。当前最有价值的工作：补齐 D1 path-loader 可比性，用重复采样确认 64B 与 P99 是否稳定复现。

## 结论

1. Q19C-M0 已经实现 QEMU 与 D1 同版 benchmark 横向对比，证据见 §测试对象与可比性边界、§代码与测试结构。
2. D1 真板在 256B 及以上 TX 场景稳定接近 115200 bps 线速，证据见 §TX Throughput 与 Drain 策略。
3. QEMU 吞吐不能用于物理线速结论，只能用于功能和相对趋势判断，证据见 §测试对象与可比性边界。
4. RX 空读非阻塞语义已通过，但 RX 数据吞吐尚未测试，证据见 §RX 与非阻塞行为。
5. 后续主线应先推进 Q19C M1 memory-root path loader，再讨论 SDMMC/rootfs 或更大范围性能优化。

## 术语表

`tcdrain()`：POSIX termios 接口，等待输出队列中的数据发送完成。本文中用于区分“write 已接受数据”和“UART 物理发送完成”。

FIONBIO：File IOctl Non-Blocking I/O，通过 ioctl 设置文件描述符非阻塞模式。

EAGAIN：POSIX 错误码，表示当前没有可读数据或资源，调用方可稍后重试。

FIFO：First-In First-Out，UART 硬件发送/接收缓冲队列。本文 D1/QEMU 观察均按 16B FIFO 边界分析。

NAPI：New API，Linux 网络子系统中用于降低高频中断开销的轮询/中断混合思想；本文用于描述 async UART copier 的批处理配置。

术语是本文指标解释的必要上下文，避免把软件入队、TTY 完成语义与硬件物理发送混为同一类性能数据。
