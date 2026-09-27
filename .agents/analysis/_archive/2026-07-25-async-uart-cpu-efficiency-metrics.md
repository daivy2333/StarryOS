# 异步 UART CPU 效率指标与测试落地

> Project: StarryOS  
> Branch: `uart-lichee`  
> Commit: `f8819a2f0da205bacfdee80cba276cc278cc452d`  
> Date: 2026-07-21  
> Scope: 评估现有测试已经覆盖的性能指标，以及无需补齐系统 CPU accounting 就能落地的 CPU 效率证据。

相关材料：[异步 UART benchmark 报告](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/docs/benchmark-report-async.md)、[D1 异步结果](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/docs/d1_out.md)、[D1 Console 结果](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/docs/d1_console.md)。

## 结论

StarryOS 当前不能可靠测量“系统 CPU 占用率”。线程 CPU 时间 accounting 尚未覆盖抢占切换，进程与线程 CPU clock 未区分，异步 UART copier 又是独立内核任务；由此得到的 CPU 百分比会混合等待时间、调用者时间和 copier 时间。

这不妨碍验证异步 UART 是否释放 CPU 性能。现有基础设施已经可以构成四类证据：

1. 调用者更早返回：入队时间、最终 drain 时间、提交占比。
2. 释放的时间能做有效工作：通信与计算重叠量、定时唤醒超时量。
3. 完成相同通信消耗多少 CPU work：每字节指令数、每次写入指令数、路径计数密度。
4. 性能没有以正确性为代价：完成吞吐、尾延迟、短写、背压、drain 和退出状态。

优先落地 `submit_fraction`、通信—计算重叠和 `instructions_per_byte`。三者分别回答“是否早返回”“释放时间是否可用”“总 CPU work 是否下降”，比单独展示墙钟吞吐更接近异步化目标。

## 现有证据

当前 benchmark 已覆盖的指标如下。

| 场景 | 已有指标 | 能支持的判断 | 不能支持的判断 |
| --- | --- | --- | --- |
| S10/S12/S13/S14 | 完成吞吐、D1 线速占比 | 数据最终传输能力 | 调用者占用和系统 CPU 占用 |
| S11 | enqueue、final drain、短写、背压 | 提交与完成解耦、容量边界 | 两种路径的总 CPU work |
| S20/S21 | P50/P95/P99/max、P99/P50 | 完成延迟与抖动 | 调度公平性和 CPU 百分比 |
| S12/S13 | batching、writev | 系统调用和批处理摊销 | copier 后台成本 |
| S40 | push/pop/send/zero/no-progress 等计数 | 路径活动、前进性、轮询密度 | 指令成本和真实 CPU 时间 |
| 启动 benchmark | ring push/pop 吞吐、RX latency、配置/IRQ 数 | 局部数据结构和启动检查 | 工作负载期间 IRQ 数；当前计数发生在 copier 启动前 |
| 正确性检查 | drain error、FIONBIO、Done、退出码 | 测试是否完整结束 | 性能收益本身 |

固定字节 RX 场景 S31 当前由 `BENCH_RX_FIXED_BYTES=0` 跳过，不能计入现有覆盖。

D1 上已有 S11 数据可直接派生调用者释放指标：

| 负载 | Async enqueue | Async final drain | Console write loop | `submit_fraction` | `released_window` |
| --- | ---: | ---: | ---: | ---: | ---: |
| 64 B × 100 | 1 ms | 545 ms | 545 ms | 0.18% | 约 544 ms |
| 256 B × 100 | 2 ms | 2183 ms | 2183 ms | 0.09% | 约 2181 ms |
| 1024 B × 100 | 3146 ms | 5590 ms | 8732 ms | 36.0% | 约 5586 ms |

计算口径：

```text
submit_fraction       = enqueue_ms / (enqueue_ms + final_drain_ms)
producer_available    = 1 - submit_fraction
released_window_ms    = console_write_loop_ms - async_enqueue_ms
```

64 B 和 256 B 负载表明调用者几乎立即完成提交；1024 B 负载开始明显受环容量和设备速度背压。由于 Async 的 enqueue/final drain 与 Console 的 write loop/final drain 语义不同，这组结果不应换算成“吞吐加速倍数”。

当前 D1 S40 冻结数据为：`user_calls=2577`、`user_acc=338201`、`ring_pop_calls=1659`、`ring_pop_bytes=338108`、`hw_send_calls=13842121`、`hw_send_bytes=338108`、`hw_send_zero=13820496`、`no_progress_budget=20171`。可派生：

| 指标 | 当前值 |
| --- | ---: |
| 接受字节 / user call | 131.2 B |
| 弹出字节 / ring pop | 203.8 B |
| 发送字节 / hw send call | 0.024 B |
| zero send / KiB | 约 41,857 |
| no-progress budget / KiB | 约 61.1 |

这些数据证明数据最终前进，但大量空发送探测也说明当前证据不足以宣称“异步路径降低了总 CPU work”。需要用每字节指令数和并发有效工作量区分“调用者早返回”与“系统整体更省 CPU”。

## 测量链

```text
benchmark process
  -> write/writev syscall
  -> TTY output path
  -> async TX ring
  -> kernel copier task
  -> UART FIFO / interrupt

CLOCK_MONOTONIC --------------------> 墙钟、吞吐、延迟、抖动
instret snapshots ------------------> 相同区间内的 CPU work
S40 reset + snapshot --------------> 路径调用与空转密度
fixed compute / absolute sleep -----> 释放时间是否可用、调度干扰
```

墙钟时间通过平台单调时钟读取，不受 100 Hz tick 粒度限制，足以支持毫秒级吞吐、延迟和绝对时间睡眠测试。`/proc/instret` 已提供 RISC-V `instret` 读取，可以在单核或固定 hart 条件下测量区间指令增量。

## 可立即落地的指标

**1. 从 S11 派生提交占比。** 无需修改内核，只需在结果汇总中增加 `submit_fraction`、`producer_available` 和 `released_window_ms`。该组指标最直接地说明同步等待从调用者路径移到后台完成路径。

**2. 每段读取 `/proc/instret`。** 在 workload 前后读取计数，并记录实际完成字节数：

```text
instructions_per_byte  = (instret_end - instret_begin) / completed_bytes
instructions_per_write = (instret_end - instret_begin) / completed_writes
instructions_per_sec   = (instret_end - instret_begin) / elapsed_sec
```

主指标应为 `instructions_per_byte`，因为它能对不同 payload 归一化。测试需保持相同字节数、串口配置、预热、输出去向和完成条件，并同时报告原始 delta。单 hart 的 D1 与单 vCPU QEMU 可以直接比较同环境内的相对变化；SMP 下需要绑核或改成 per-hart 采样。

**3. 增加通信—计算重叠场景。** 固定 UART 字节数和计算内核，在发送理论窗口内执行确定性计算，随后 drain：

```text
t0 = monotonic_now()
write(payload)
t1 = monotonic_now()
run_fixed_compute_until(t0 + theoretical_uart_time)
t2 = monotonic_now()
tcdrain(fd)
t3 = monotonic_now()
```

记录 `write_return_ms`、`useful_work_iterations`、`useful_work_per_ms`、`final_drain_ms`。用同样时间窗口的无 UART 基线计算：

```text
overlap_efficiency = useful_work_with_uart / useful_work_idle_baseline
```

该场景既能证明调用者释放的窗口被有效计算利用，也能暴露 copier 慢轮询对其他任务的抢占。计算内核必须固定、无 I/O、避免被编译器消除，并在 Async 与 Console 两条路径使用同一构建产物。

**4. 对每个测试段重置和快照 S40。** 现有 ioctl 已支持 reset/snapshot。每个 workload 独立采集后派生：

```text
hw_send_calls_per_kib
hw_send_zero_per_kib
ring_pop_calls_per_kib
no_progress_budget_per_kib
bytes_per_ring_pop
bytes_per_hw_send
```

分段数据能把空转归因到具体 payload 和压力区间，避免全套 benchmark 的累计值掩盖问题。

**5. 增加定时唤醒超时量。** 并发 UART workload 下循环调用 `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)`，记录实际唤醒时间减目标时间，分别报告 idle、Async TX 和 Console TX 的 P50/P95/P99/max。该测试使用现有定时器和绝对睡眠接口即可完成，能观察后台 copier 是否损害其他任务的响应性。

**6. 增加并发有效工作公平性。** 在固定 UART 完成窗口内运行纯计算线程，比较 idle 基线的迭代数：

```text
compute_retention = iterations_with_uart / iterations_idle
```

它与 overlap 场景关注点相近，但更强调 UART 完成期间系统留给竞争任务的计算份额。若测试预算有限，先实现 overlap 场景即可。

**7. 小幅扩展遥测可见性。** 驱动内部已有 `tx_poll`、`tx_no_progress`、`tx_hw_bytes`，但当前 `TxDebugSnapshot` 没有全部暴露。将它们以版本化快照输出后，可派生 `polls/KiB`、`bytes/poll` 和 `no_progress_ratio`。工作负载期间的 UART IRQ delta 也应在 copier 启动后采样；启动阶段的零值不能作为轮询或中断模式的证据。

## 最小证据组合

一次可以支持“异步 UART 释放 CPU 性能”的对比测试至少应同时报告：

| 证据问题 | 主指标 | 配套指标 |
| --- | --- | --- |
| 调用者是否更早返回 | `submit_fraction` | enqueue、final drain、released window |
| 释放时间是否能被利用 | `overlap_efficiency` | useful work、timer wakeup P99 |
| 相同通信的 CPU work 是否下降 | `instructions_per_byte` | instret delta、zero/KiB、no-progress/KiB |
| 传输能力是否保持 | D1 `line_rate_pct` | completed bytes、总完成时间 |
| 响应性是否退化 | completion P50/P99 | P99/P50、max |
| 语义是否保持 | drain error = 0 | 短写、背压、Done、exit code 0 |

结论应按证据拆分：`submit_fraction` 只支持调用者释放，`overlap_efficiency` 支持释放时间可用，`instructions_per_byte` 才支持总 CPU work 的相对比较。只有三者方向一致，才适合表述为 CPU 效率改善。

## 边界与失败路径

**CPU accounting 暂不可用。** [`TimeManager`](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/task/timer.rs) 明确留有“抢占不改变 timer state”的待办；[time syscall](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/time.rs) 对 process/thread CPU clock 使用相同路径；[proc task stats](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/pseudofs/proc.rs) 的 `utime/stime` 没有形成有效工作负载计数；[copier 创建路径](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/drivers/os_arceos.rs) 又将后台工作放入独立内核任务。因此当前不要报告 process CPU%、thread CPU% 或系统 CPU%。

**D1 绝对时间存在换算误差。** [D1 time 实现](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/axplat-riscv64-lichee-d1/src/time.rs) 先计算 `1_000_000_000 / 24_000_000 = 41 ns/tick`，相对真实值约少 1.6%。同一时钟内的比例指标多数会抵消该误差，绝对吞吐和延迟仍会受影响。严谨比较前应改为宽整数的“tick × 1e9 / frequency”换算。

**`instret` 不是 CPU 时间。** 它衡量退休指令数，适合在同平台、同构建、同工作负载下比较 CPU work；不同微架构、编译选项或模拟器之间不能直接比较。中断和其他任务会污染全局计数，因此需要单 hart、绑核或空载基线，并报告重复运行的中位数和尾部范围。

**QEMU 与 D1 的证据用途不同。** QEMU 可用于功能回归和同环境的相对 `instret/byte`，不能代表物理 UART 线速或真实硬件 CPU 成本。D1 可验证线速与真实设备路径，但当前结果是单 hart 证据，不能证明 SMP memory ordering。

**输出会污染被测路径。** 采样期间不要把逐次结果写到同一个 UART。计数应先保存在内存，workload 结束并 drain 后统一打印。Async 与 Console 必须使用相同 payload、轮次、UART 配置、完成条件、预热次数和统计口径。

**失败判据需要先固定。** 若 Async 仅降低 `submit_fraction`，但 `instructions_per_byte`、zero/KiB 或 timer wakeup P99 明显恶化，应表述为“调用者等待转移到后台”，不能表述为“系统 CPU 效率改善”。

## 关键接口与文件

| 位置 | 用途 |
| --- | --- |
| [tests/benchmark.c](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/tests/benchmark.c) | S00-S40 场景、时钟、统计和输出入口 |
| [kernel/src/syscall/time.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/time.rs) | monotonic clock、sleep、CPU clock 语义 |
| [kernel/src/task/timer.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/task/timer.rs) | 任务 CPU accounting 状态与限制 |
| [kernel/src/pseudofs/proc.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/pseudofs/proc.rs) | `/proc/instret` 和 task stats |
| [kernel/src/drivers/os_arceos.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/drivers/os_arceos.rs) | UART copier 内核任务创建 |
| [crates/uart_16550/src/async_/driver.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/uart_16550/src/async_/driver.rs) | TX ring、poll、drain 和 debug snapshot |
| [crates/uart_16550/src/async_/telemetry.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/uart_16550/src/async_/telemetry.rs) | poll、no-progress、硬件字节等内部遥测 |
| [crates/axplat-riscv64-lichee-d1/src/time.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/axplat-riscv64-lichee-d1/src/time.rs) | D1 tick 到纳秒换算 |

这份分析只定义可验证的指标与证据边界，不替代 q17 的 SMP ordering 验证，也不改变现有 benchmark Gate。
