# 基准测试运行与结果判读

## 适用范围

- QEMU riscv64-virt 与 D1 Lichee RV Dock 真板 benchmark 的运行方式和结果判读
- 不适用于：性能调优方法论、benchmark 代码新增/修改

## 前置条件

- 环境已按 `qemu-build.md` 或 `d1-build-and-flash.md` 准备完毕
- 理解 QEMU 不仿真串口线延迟 → 绝对吞吐量以真板为准

## benchmark 套件总览

`tests/benchmark.c` 包含以下 S 系列测试：

| 编号 | 名称 | 测量内容 | 可信度 |
|---|---|---|---|
| S00 | Manifest | 环境快照 (target mode、line rate、timer) | 信息性 |
| S10 | TX Throughput Baseline | `write + tcdrain` 每次 drain，测试同步完成吞吐 | D1 可信 |
| S11 | TX Enqueue Cost | write loop 不计 drain，测入队速度 vs 线速 | D1 可信 |
| S12 | TX Batch Drain | 每 8 次 write 一次 drain，测试批量提交效率 | D1 可信 |
| S13 | TX writev Fragment | 4×64B `writev` 分片写入路径 | D1 可信 |
| S14 | TX Small-Packet Break-even | 64B/128B/256B drain-each，观察小包线速 | D1 可信 |
| S20 | TX 1B Latency | 单字节 `write + tcdrain`，100 次 | D1 可信 |
| S21 | FIFO Boundary Matrix | 1~49B 跨 FIFO 16B 边界延迟 | D1 可信 |
| S30 | RX Nonblocking | 空缓冲 `O_NONBLOCK` / `FIONBIO` → EAGAIN | 两端可信 |
| S31 | RX Fixed Payload | 固定字节 RX witness (当前配置为 0，跳过) | — |
| S40 | TX Counter Proxy | 发送路径行为计数 (非 CPU 百分比) | D1 可信 |
| Startup | Ring Buffer Raw | 内核态 ring buffer 速度 + RX 延迟 | 相对指标 |

## 操作步骤

### QEMU

```bash
make tests/benchmark                     # 交叉编译 C 测试程序
# 将 benchmark 复制到 disk.img:/bin/benchmark
make run                                 # QEMU 内 ./benchmark
# 或通过 init.sh 启动: /bin/sh -c init.sh
```

### D1

```bash
make lichee-userbench                    # embedded ELF 模式，自动运行
make lichee-fullbench-command            # command-entry 模式，自动运行
make lichee-fullbench-mem                # memory-root 模式，自动运行
```

## 结果判读

### 吞吐量 (S10/S12/S13/S14)

**D1 判据**：
- `line_rate_pct` ≥ 93%：正常。D1 典型值 95.2%-99.1%
- `line_rate_pct` < 90%：可能有退化，对比上次基线
- `short_writes > 0`：S10 不应有短写；S11 是预期行为 (压满 ring buffer)
- `drain_errors > 0`：严重问题，立即排查

**QEMU 判据**：
- QEMU 值远超 115200 bps 线速是预期现象 (不仿真物理延迟)
- QEMU 吞吐量只用于**相对对比**（同环境前后版本差异），不用于绝对声明

### 入队速度 (S11)

- enqueue KB/s 远高于 line rate 说明用户态入队不是瓶颈
- short writes 数量只反映 ring buffer 水位，不影响数据完整性
- `final_drain` 必须收敛到 `ring_empty=1, transmitter_empty=1`

### 延迟 (S20/S21)

**D1 判据**：
- 1B P50 约 0.19ms，P99 < 0.25ms
- `slow_over_line_plus10ms = 0`：关键通过指标
- S21 size≥15 的 P99 tail (24-27ms) 是已知边界，不是退化
- 对比时用 `p99_p50_ratio` 和 `max_p50_ratio`，不要只看绝对值

**QEMU 判据**：
- 延迟比 D1 更小是预期 (无硬件延迟)
- QEMU 延迟仍然可用于相对对比

### 稳定性 (S30)

```
S30 open(O_NONBLOCK): PASS / EAGAIN
S30 ioctl(FIONBIO):   PASS / EAGAIN
```
两者都必须是 PASS，否则非阻塞语义异常。

### Counter Proxy (S40)

**D1**：
- `slow_poll_exh = 0` 且 `yield_exh = 0`：fallback 未耗尽 ✅
- `hw_send_zero` 极高 (1222 万次) 是 slow-poll 探测 FIFO 的正常观测，不是异常

**QEMU**：
- `telemetry_available = 0` → 预期显示 `status=not-available reason=telemetry-counters-are-zero`

### 完整通过条件

| 条件 | QEMU | D1 |
|---|---|---|
| 程序执行到 `Done.` | ✅ | ✅ |
| 进程退出码 0 | ✅ | ✅ |
| S10 line_rate_pct ≥93% | N/A | ✅ |
| S30 双 PASS | ✅ | ✅ |
| S40 无 exh 耗尽 | N/A | ✅ |
| drain_errors 全 0 | ✅ | ✅ |

## 注意事项

- **QEMU 吞吐量 ≠ 物理吞吐量**：QEMU 16550 不仿真串口线延迟 (86.8 µs/byte)。所有声明线速的数据必须来自 D1 真板。
- **内核态 ring buffer benchmark 不等于 UART 吞吐**：Startup 段的 ring buffer 速度是纯内存操作速度，绕过了 UART。不应用于线速声明。
- **S11 S40 不是 CPU 占用率**：S11 enqueue speed 只反映 ring buffer 入队，S40 counter proxy 反映 TX 路径调用模式。两者都不等于 CPU usage。
- **P99 tail 是已知边界**：D1 size≥15 的 24-27ms P99 tail 保留为已知限制，不影响吞吐结论。
- **对比基准必须在同环境**：QEMU vs QEMU，D1 vs D1。不能拿 QEMU 吞吐和 D1 吞吐互比绝对值。
- **S31 RX fixed payload 当前跳过**：`BENCH_RX_FIXED_BYTES=0`，不纳入验证。
