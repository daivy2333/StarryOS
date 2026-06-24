# Async 异步串口性能测试报告

> **项目**：StarryOS（[daivy2333/StarryOS](https://github.com/daivy2333/StarryOS)） + uart_16550（[daivy2333/uart_16550](https://github.com/daivy2333/uart_16550)）
> **分支**：`feat/uart-16550-async`（Q0~Q13 + LTO 全部完成） · **测试分支**：`feat/uart-16550-bench`（独立 bench 模块）
> **截稿日期**：2026-06-17（Q13.1 + LTO 完成后最终更新）
> **关联文档**：`docs/async-uart-architecture.md`（架构） · `docs/uart-performance-comparison.md`（Console vs Async 对比） · `.claude/analysis/async-uart-module-boundary.md`（Q13 事后视角）
> **重要声明**：QEMU riscv64-virt 不仿真真实串口线延迟（86.8 µs/byte @115200 bps），吞吐量数值偏高。真板 VisionFive2 @ 115200 bps 收敛至 ~11.5 KB/s（硬件理论上限）。本文 QEMU 实测数据仅供**相对性能对比**，绝对吞吐需以真板为准。

---

## 0. TL;DR

Async 异步串口在 QEMU riscv64-virt 上经过 Q7~Q15 M0~M4 共 12 阶段演进——当前 `feat/uart-16550-bench` 分支状态（**未启用 LTO** per ADR-034）实测：**内核态 ring buffer 吞吐 456,205 KB/s**（TX，两次手动测试平均）/ **1,147,959 KB/s**（RX，**较 Q13+LTO 状态 897,616 KB/s 提升 +27.9%**），**用户态 1B e2e 延迟 134 µs avg / P50 118.5 µs**（n=100，单字节 write+tcdrain），**非阻塞三入口全 PASS**。**Q15 阶段在 RX 路径引入的 lock-free 改进使吞吐显著提升**（Q15-M0 telemetry + Q15-M4 IER single owner 等），TX 路径受 LTO 关闭影响较 Q13+LTO 状态下降 ~30%（符合 ADR-034 预期）。**e2e 延迟瓶颈仍在调度**（Q13 印证未变），Q15 未触及调度层。

| 维度 | 当前成绩（Q15, 无 LTO）| 测量条件 |
|------|---------------------|---------|
| 内核态 Ring Buffer TX | **456,205 KB/s** | [`bench.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/async_/bench.rs) 写入 102,400 字节 × 100 次（两次手动测试平均 455,580.87 + 456,829.60）|
| 内核态 Ring Buffer RX | **1,147,959 KB/s** | [`bench.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/async_/bench.rs) 读取 65,536 字节（两次手动测试平均 1,196,261.68 + 1,099,656.36）|
| 用户态 1B e2e 延迟 | **134 µs avg / P50 118.5 µs** | [`benchmark.c`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-bench/tests/benchmark.c) n=100，write+tcdrain（两次平均 127/141 µs）|
| 用户态 64B TX 吞吐 | **170 KB/s** | `benchmark.c` 100 次迭代（两次平均 169.68 + 170.28）|
| 非阻塞模式 | ✅ 三入口（open / fcntl / ioctl）全 PASS | `EAGAIN` 行为正确 |

**小结**：本节 5 维度最佳成绩是 §2~§7 各章节数据的索引表；详细测试方法、阶段演进、当前 state 综合见后续章节。

---

## 1. 测量条件

所有性能数据 MUST 在统一测量条件下解读，**不同条件数据不可直接比较**。QEMU 16550 模型不仿真真实串口线延迟（86.8 µs/byte）使吞吐数值偏高，真板 VisionFive2 @ 115200 bps 收敛至 ~11.5 KB/s（硬件理论上限）。QEMU RISC-V `monotonic_time_nanos` 计时器分辨率约 100ns，单字节延迟测量下限为 100ns。

**测试环境**（QEMU `qemu-riscv64-virt`，统一基线）：

| 项目 | 配置 | 备注 |
|------|------|------|
| 目标架构 | RISC-V 64-bit | `riscv64gc-unknown-linux-musl` |
| 模拟平台 | QEMU `qemu-riscv64-virt` | **不仿真串口线延迟** |
| 串口硬件 | NS16550 UART | 模拟设备 |
| 波特率 | 115200 bps | 标准串口速率 |
| FIFO 深度 | 16 字节 | FCR（**F**IFO **C**ontrol **R**egister，FIFO 控制寄存器）配置 |
| 构建模式 | `release`（optimized）| LTO on/off 分两个独立构建 |
| 计时器 | `monotonic_time_nanos` | QEMU RISC-V 上**分辨率约 100ns** |

**QEMU 仿真限制**：

- QEMU 16550 模型不仿真真实串口线延迟。`tcdrain()` 的 TCSBRK 实现正确（poll ring buffer + LSR.TRANSMITTER_EMPTY），但 QEMU 内部 UART 数据处理为瞬时。真板 VisionFive2 @ 115200 bps 将产生 ~11.5 KB/s 的准确吞吐量。
- QEMU RISC-V `monotonic_time_nanos` 分辨率约 100ns，单字节延迟测量下限为 100ns（小于 100ns 的值均显示为 `<100ns`）。

**优化阶段对照**（Q7~Q13.1 + LTO 共 9 个阶段）：

| 阶段 | 日期 | 关键变更 | 主要影响 |
|------|------|---------|---------|
| Q7 | 2026-06-01 | yield storm 修复 / FIONBIO 传播 / benchmark 修正 / tcdrain 真异步 | 空闲 CPU 归零，基准建立 |
| **Q8** | 2026-06-11 | NAPI 退出修复 / ISR 去锁 / IER 规范化 / O46 AtomicWaker (8×PollSet) | ISR 延迟 ↓200ns，唤醒延迟 200→50ns |
| **Q9** | 2026-06-11 | VTIME 读超时（axtask::future::timeout）| `todo!()` → `timeout()` |
| **Q10** | 2026-06-11 | BUF_SIZE 80→256 / SimpleReader push_slice / read(&self) | 1B 延迟 ↓16%，256B TX ↓6% |
| **Q11** | 2026-06-11 | tty unwrap / mm/access 批页 / sendfile / close_range / ws_col | 整体稳定优化 |
| **Q12** | 2026-06-11 | Embassy 路径 A：lock-free SPSC ring_buffer (O51) / embedded_io_async (O52) / TC tcdrain (O53) | software overhead ↓31%（53.9→37.1µs），64B 吞吐 ↑24% |
| **Q13** | 2026-06-16 | 异步串口提取到 uart_16550（5 trait 抽象）| overhead +16.2µs（37.1→53.3µs），可移植性 ✅ |
| **Q13.1** | 2026-06-16 | #[inline(always)] + push_batch/pop_batch | overhead ↓20%（53.3→42.6µs），1B avg ↓7.6% |
| **LTO** | 2026-06-16 | `lto = true`，跨 crate 内联（**已 revert**，参见 ADR-034）| 内核态 ring buffer ↑69% (385→652 MB/s)，e2e 不变（瓶颈在调度）|

**小结**：测试环境与构建配置直接影响性能数据可比性。QEMU 实测适用于**阶段间相对对比**与**功能正确性验证**，**绝对吞吐需以真板为准**（Q6 待定）。

---

## 2. 内核态测试结果

内核态测试由 [`bench.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/bench.rs) 模块（仅 `feat/uart-16550-bench` 分支）提供，启动时自动运行，输出至串口日志。**关键观察**：内核态吞吐（MB/s 级）远高于硬件线速（KB/s 级），**瓶颈不在 ring buffer**（56,600× 冗余）。

**Ring Buffer 写入速度（TX）**：向 TX Ring Buffer 写入 102,400 字节（1024 × 100），测量总耗时和 CPU 占用。

| 指标 | 值 | 说明 |
|------|-----|------|
| Ring Buffer 写入 | **456,205 KB/s** | 内核态写入（Q15 M0~M4，无 LTO per ADR-034）|
| 测试数据量 | 102,400 字节 | 100 × 1024 字节 |
| 测试耗时 | 0.15 毫秒 | 纳秒级精度 |
| 硬件线速 | 11.52 KB/s | 115200 bps 理论极限（86.8 µs/byte）|
| 软件 vs 硬件 | 56,600× 冗余 | 内核态快于硬件 4 个数量级 |

> **缩写说明**：SPSC = **S**ingle-**P**roducer **S**ingle-**C**onsumer（单生产者单消费者），lock-free 队列的典型场景。

**Ring Buffer 读取速度（RX）**：从 RX Ring Buffer 读取 65,536 字节数据，测量总耗时。

| 指标 | 值 | 说明 |
|------|-----|------|
| Ring Buffer 读取 | **1,147,959 KB/s** | 内核态读取（Q15 M0~M4，RX 路径 lock-free 改进）|
| 测试数据量 | 65,536 字节 | 64 KB |
| 测试耗时 | 0.07 毫秒 | 纳秒级精度 |

**Ring Buffer 读取延迟（RX，100 次单字节）**：读取 100 个单字节，测量每次读取的延迟。

| 指标 | 值 | 说明 |
|------|-----|------|
| P50 延迟 | <100 ns | 中位数延迟（**低于 `monotonic_time_nanos` 分辨率**）|
| P95 延迟 | 100 ns | 95 分位延迟 |
| P99 延迟 | 14,700 ns | 99 分位延迟 |
| 最小延迟 | <100 ns | 最快一次（计时器分辨率极限）|
| 最大延迟 | 14,700 ns | 最慢一次 |
| 平均延迟 | 195 ns | 平均值（受 P99 拉高）|

> **方法学说明**：P50/最小显示 `<100ns` 而非精确数值，因 QEMU RISC-V `monotonic_time_nanos` 分辨率约 100ns。

**内存占用**：

| 组件 | 大小 | 说明 |
|------|------|------|
| RX Buffer | 64 KB | 接收 Ring Buffer（embassy lock-free SPSC）|
| TX Buffer | 64 KB | 发送 Ring Buffer（embassy lock-free SPSC）|
| 驱动结构体 | 136 字节 | `AsyncUartDriver`（Q13 trait 抽象，无 Mutex）|
| 总计 | 128,136 字节 | 约 125 KB |

**中断处理（NAPI 配置）**：

| 指标 | 值 | 说明 |
|------|-----|------|
| ISR Count | 0（启动时）| 无 UART 流量时 ISR 不被触发 |
| IRQ Frequency | N/A | 无流量时 IRQ 频率无意义 |
| NAPI 阈值 | 16 次 | 连续成功读取后切换轮询模式 |
| NAPI 批量 | 64 字节 | 轮询模式下的批次大小 |

> **缩写说明**：NAPI = **N**ew **API**（Linux 网络子系统的高吞吐中断合并机制），本项目借鉴"连续成功 ≥16 次后切轮询"实现。

**小结**：内核态 ring buffer 吞吐**远超**硬件线速（56,600× 冗余），证明瓶颈不在数据搬运层。Q13 + LTO 使吞吐达到 651 MB/s，但 e2e 延迟未见改善——印证调度瓶颈论（详见 §3 用户态与 §6 趋势）。

---

## 3. 用户态测试结果（Q13.1 + LTO 最新）

用户态测试由 [`tests/benchmark.c`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/tests/benchmark.c)（Q7 修正后，**主分支有效**）提供，测量端到端 write/read/tcdrain 性能。Q13 + Q13.1 + LTO 三个阶段累计优化是当前 state——**用户态性能呈现"内核态快 4 数量级、e2e 受调度瓶颈制约"的双重特征**。

**TX 吞吐量测试**：写 `/dev/console`，每次后 `tcdrain()`，100 次迭代，4 种数据大小。

| 数据大小 | 实测/次（QEMU）| 硬件理论/次 | 真板预测 |
|----------|-------------|------------|----------|
| 64 bytes | 518.0 µs | 5,555.6 µs | 6.07 ms |
| 256 bytes | 1,305.6 µs | 22,222.2 µs | 23.5 ms |
| 1024 bytes | 4,922.5 µs | 88,888.9 µs | 93.8 ms |
| 4096 bytes | 9,852.0 µs | 355,555.6 µs | 365.4 ms |

> **缩写说明**：`tcdrain()` 是 POSIX termios 函数，等待所有输出传输完毕；`/dev/console` 是 Linux 风格的 console 设备节点。
> **Q13 性能说明**：Q12→Q13 引入 trait 抽象（5 个 OS trait），带来约 5.5 µs 软件 overhead 增加（129.5 vs 124 µs，Q12 无 trait 抽象）。这是为可移植性付出的合理代价——`uart_16550` 现在可复用于任何 OS。Q13.1 通过 `#[inline(always)]` + `push_batch`/`pop_batch` 将 overhead 从 53.3 µs 降到 42.6 µs（↓20%）。

**TX 单字节延迟**（write + tcdrain，n=200）：

| 指标 | 值（QEMU）| 说明 |
|------|----------|------|
| P50 | 139.4 µs | 中位数 |
| P95 | 171.2 µs | 95 分位 |
| P99 | 238.8 µs | 99 分位 |
| 平均 | 143.7 µs | 总平均 |
| 软件 overhead | 56.9 µs | 平均减硬件理论 86.8 µs |

> **方法学说明**：硬件理论 86.8 µs/byte @ 115200 bps（8N1 = 10 bit/byte = 86.8 µs）；软件 overhead = 实测 - 硬件理论。

**非阻塞模式（FIONBIO 三入口）**：

| 测试 | 结果 | 说明 |
|------|------|------|
| `open(O_NONBLOCK)` + `read()` | ✅ PASS（`EAGAIN`）| Q7 O43 修复后生效 |
| `ioctl(FIONBIO, 1)` + `read()` | ✅ PASS（`EAGAIN`）| |
| `fcntl(F_SETFL, O_NONBLOCK)` + `read()` | ✅ PASS（`EAGAIN`）| |

> **缩写说明**：FIONBIO = **F**ile **IO**ctl **N**on-**B**locking **I**/O；`EAGAIN` = "再试一次" POSIX 错误码；`O_NONBLOCK` = open 标志；`F_SETFL` = fcntl 设置文件状态标志。

**小结**：当前 Q15 状态（无 LTO）下用户态延迟 134 µs avg / 118.5 µs P50。Q13.1 + LTO 状态 42.6 µs 软件 overhead 是历史最优（已归档），开发期暂不复用 per ADR-034。

---

## 3.6 FIFO 边界延迟矩阵（Q15 当前 state）

按 1/15/16/17/31/32/33/48/49 字节粒度测量单字节 write+tcdrain 延迟（n=100，两次手动测试平均）：

| 字节数 | 相对 FIFO 边界 | avg（ms）| P50（ms）| P95（ms）| 备注 |
|--------|----------------|----------|----------|----------|------|
| 1 | << 1 FIFO（16B）| 0.142 | 0.121 | 0.305 | 1 字节无需边界 |
| 15 | 1 FIFO - 1 | 0.211 | 0.193 | 0.349 | 接近填满 1 FIFO |
| 16 | **正好 1 FIFO** | 0.189 | 0.182 | 0.279 | 1× THR 中断 |
| 17 | 1 FIFO + 1 | 0.183 | 0.182 | 0.237 | 跨 1 字节 |
| 31 | 2 FIFO - 1 | 0.245 | 0.249 | 0.342 | 接近填满 2 FIFO |
| 32 | **正好 2 FIFO** | 0.255 | 0.252 | 0.348 | 2× THR 中断 |
| 33 | 2 FIFO + 1 | 0.265 | 0.269 | 0.364 | 跨 1 字节 |
| 48 | **正好 3 FIFO** | 0.342 | 0.339 | 0.440 | 3× THR 中断 |
| 49 | 3 FIFO + 1 | 0.338 | 0.341 | 0.461 | 跨 1 字节 |

**3 个非显然观察**：

1. **P95 延迟在 1B 最低（0.305 ms）**——单字节无需等待 FIFO 边界中断
2. **17B P95 优于 15-16B**（0.237 < 0.349/0.279）——跨 1 字节可能触发连续 ISR 模式
3. **延迟与字节数近似线性**：1B→49B avg 增长 ~2.4×（0.142→0.338 ms），斜率约 5.3 µs/字节

**小结**：FIFO 边界对延迟的影响是非线性的。NAPI 阈值 16 正好等于 FIFO 深度，存在耦合（详见 §5 演进历史）。

---

## 4. 用户态 RX 测试说明

**当前状态**：用户态 RX 测试在内核 benchmark 模块中完成（直接操作 Ring Buffer），**绕过 TTY 回显问题**——RX Ring Buffer 读取 ~864 MB/s，RX 延迟 P50 200 ns，**Ring Buffer 不是瓶颈**（864 MB/s >> 串口线速 11.52 KB/s）。

**未来方向**：设置终端 raw mode + 禁用 echo，可实现用户态 RX 测试。**Q6 真板验证后**可获得真实 RX 性能数据。

**小结**：用户态 RX 测试当前**未在主分支启用**（依赖 raw mode 终端配置）。Q6 真板验证后可补全。

---

## 5. 测试方法

测试方法分**内核态统计**（QEMU 启动时自动）与**用户态自动化**（[`benchmark.c`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/tests/benchmark.c) + [`scripts/benchmark.sh`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/scripts/benchmark.sh)）两套，前者测吞吐/延迟细节，后者测 e2e 性能与 FIONBIO 行为。

**内核态**（[`bench.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/bench.rs)，`feat/uart-16550-bench` 分支）：

- **Ring Buffer TX**：`push` 102,400 字节（`RingBufTx::push` × 100），测量速度
- **Ring Buffer RX**：`pop` 65,536 字节 + 100 次单字节延迟
- **调用接口**：[`uart_16550::async_::bench`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-async/src/async_/bench.rs) 导出的统计接口（NAPI 常量、IRQ 计数器）
- **运行时机**：启动时自动运行，输出到串口日志
- **分支说明**：内核 benchmark 模块**仅存在于 `feat/uart-16550-bench` 测试分支**，不在主开发分支

**用户态**（[`tests/benchmark.c`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/tests/benchmark.c)，Q7 修正后，主分支有效）：

- **TX 吞吐量**：`write(/dev/console) + tcdrain()`，100 次 × 4 种大小
- **TX 延迟**：单字节 `write + tcdrain`，100 次，计算 P50/P95/P99
- **非阻塞测试**：`open(O_NONBLOCK)` / `ioctl(FIONBIO)` / `fcntl(F_SETFL)` 三种入口
- **编译命令**：`riscv64-linux-musl-gcc -static`

**QEMU 时序说明**：QEMU 16550 模拟不仿真真实串口线延迟。`tcdrain()` 的 TCSBRK 实现正确（poll ring buffer + LSR.TRANSMITTER_EMPTY），但 QEMU 内部 UART 数据处理为瞬时。**真板 VisionFive2 @ 115200 bps 将产生 ~11.5 KB/s 的准确吞吐量**。

**小结**：内核态测吞吐/延迟细节（启动时自动），用户态测 e2e 性能与 FIONBIO 行为（benchmark.sh 自动化）——两套互补。

---

## 6. 性能趋势

性能趋势呈现"**内核态持续提升、e2e 受调度瓶颈制约**"的双重特征。LTO 跨 crate 内联消除函数调用开销但 e2e 不变，**证实调度是当前主要瓶颈**。

**1B e2e 延迟**（QEMU，n=100~200）：

| 阶段 | avg | P50 | P99 | software overhead | 备注 |
|------|-----|-----|-----|-------------------|------|
| Q8 | 144.7 µs | 139.5 µs | 230.4 µs | 57.9 µs | 基线 |
| Q10 | 121.6 µs | 115.8 µs | 244.1 µs | 34.8 µs | 数据路径优化 |
| Q11 | 140.7 µs | 129.2 µs | 320.4 µs | 53.9 µs | 内核通用质量 |
| Q12 | 123.9 µs | 115.7 µs | 294.0 µs | 37.1 µs | Embassy 路径 A（已归档） |
| **Q13** | **140.1 µs** | **138.8 µs** | — | **53.3 µs** | trait 抽象代价 +16.2 µs |
| **Q13.1** | **129.5 µs** | — | — | **42.6 µs** | #[inline] + 批量回收 10.7 µs |
| **LTO** | 129.4 µs | 129.5 µs | — | 42.6 µs | e2e 不变（**瓶颈在调度**） |

> **关键观察**：Q12→Q13 引入 trait 抽象，overhead +16.2 µs。Q13.1 通过内联+批量优化回收 10.7 µs（↓20%），最终 overhead 42.6 µs 仅比 Q12 的 37.1 µs 多 5.5 µs——这是为 `uart_16550` 可移植性付出的合理代价。

**内核态吞吐**（QEMU，bench.rs）：

| 阶段 | Ring Buffer TX | Ring Buffer RX | 备注 |
|------|---------------|----------------|------|
| Q8 | 214,961 KB/s | 588,776 KB/s | 基线 |
| Q11 | 196,850 KB/s | 393,362 KB/s | 内核通用质量 |
| Q12 | 385,000 KB/s | — | atomic_ring_buffer（O51）|
| **Q13.1** | 385,000 KB/s | 864,000 KB/s | trait 抽象 + 批量 |
| **Q13.1 + LTO** | 651,890 KB/s | 897,616 KB/s | 跨 crate 内联 ↑69%（已 revert per ADR-034）|
| **Q15 当前（无 LTO）** | **456,205 KB/s** | **1,147,959 KB/s** | TX -30% / **RX +27.9%**（M0~M4 lock-free 改进）|

**各阶段性能影响汇总**：

| 阶段 | 关键修复/优化 | 性能影响 |
|------|-------------|---------|
| Q7 | yield storm / FIONBIO / benchmark / tcdrain | 空闲 CPU 归零，基准建立 |
| Q8 | NAPI 退出 / ISR 去锁 / IER 规范化 / O46 AtomicWaker (8×PollSet→AtomicWaker) | ISR 延迟 ↓200ns，唤醒延迟 200→50ns |
| Q9 | VTIME 读超时 | `todo!()` → `timeout()` |
| Q10 | BUF_SIZE 80→256 / SimpleReader push_slice / read(&self) | 1B 延迟 ↓16%，256B TX ↓6% |
| Q11 | tty unwrap / mm/access 批页 / sendfile / close_range / ws_col | 整体稳定优化 |
| Q12 | Embassy 路径 A：lock-free RingBuffer (O51) / embedded_io_async (O52) / TC tcdrain (O53) | software overhead ↓31%（53.9→37.1µs），64B 吞吐 ↑24% |
| Q13 | 异步串口提取到 uart_16550（5 trait 抽象）| overhead +16.2µs（37.1→53.3µs），可移植性 ✅ |
| Q13.1 | #[inline(always)] + push_batch/pop_batch | overhead ↓20%（53.3→42.6µs），1B avg ↓7.6% |
| LTO | `lto = true`，跨 crate 内联 | 内核态 ring buffer ↑69% (385→652 MB/s)，e2e 不变（瓶颈在调度）|

**小结**：内核态持续提升（Q8 214 MB/s → Q13+LTO 651 MB/s，3.0×），e2e 受调度瓶颈制约（Q8 144.7 µs → Q13.1 129.5 µs，仅 ↓10.5%）。**LTO 印证调度瓶颈**——若瓶颈在函数调用，LTO 应能改善 e2e，事实不变。

---

## 7. 性能综合（QEMU 最新）

当前 state（Q13.1 + LTO）在 QEMU riscv64-virt 上的综合性能数据如下——**QEMU 实测综合性能达 651 MB/s 内核态 TX 吞吐与 139.4 µs P50 用户态延迟**，e2e 受调度瓶颈制约。Q6 真板验证后将获得真实环境数据。

**综合性能表**（Q13.1 + LTO state）：

| 维度 | 结果 | 测量方法 |
|------|------|---------|
| TX 用户态 @ /dev/console + tcdrain | 518 µs (64B) ~ 9,852 µs (4096B) | [`benchmark.c`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-async/tests/benchmark.c) 100 次迭代 |
| TX 延迟 P50 | 139.4 µs | `benchmark.c` n=200 |
| TX 延迟平均 | 143.7 µs | `benchmark.c` n=200 |
| FIONBIO nonblocking read | ✅ EAGAIN（三入口全 PASS）| `benchmark.c` |
| Ring Buffer TX（Q15 当前）| **456,205 KB/s** | [`bench.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/async_/bench.rs) 102,400 字节 |
| Ring Buffer RX（Q15 当前）| **1,147,959 KB/s** | `bench.rs` 65,536 字节 |
| Ring Buffer RX P50 | <100 ns（计时器分辨率限制）| `bench.rs` 100 次单字节 |

**待验证**（真板 VisionFive2，Q6 待办）：

- 真实串口吞吐量 ~11.5 KB/s @ 115200 bps（硬件理论上限）
- DMA 可行性（O3 / O40）
- 高速波特率支持（230400+，O41）

**小结**：QEMU 实测综合性能达 651 MB/s 内核态 TX 吞吐与 139.4 µs P50 用户态延迟，e2e 受调度瓶颈制约。Q6 真板验证后将获得真实环境数据。

---

## 8. 结论

按新规范要求，本节只列核心判断（3-5 条短句，**不重复 §0 与 §6 数据**）：

1. **Q15 当前 state**（无 LTO per ADR-034）——内核态 TX 456 MB/s / RX 1,148 MB/s，用户态 1B e2e 134 µs avg / P50 118.5 µs（详见 §0 与 §7）
2. **e2e 瓶颈在调度而非函数调用**——LTO 跨 crate 内联使内核态 ↑69% 但 e2e 不变，**调度优化是下一个突破点**（详见 §6）
3. **Q13 提取使 `uart_16550` 跨 OS 可复用**——5 OS trait 抽象 + 5.5 µs 软件 overhead（已被 Q13.1 回收至 Q12 同等水平）
4. **非阻塞三入口全 PASS**——open / fcntl / ioctl 三个入口 FIONBIO 行为正确（Q7 O43 修复，详见 §3）
5. **Q6 真板验证是当前唯一待办**——QEMU 仿真限制决定绝对吞吐需以真板为准

**小结**：Q7~Q13.1 + LTO 9 个阶段累计优化验证了"内核态中断驱动 + ring buffer 中转 + 跨 OS trait 抽象"的技术路线在 QEMU 上可行；下一步突破需在真板（Q6）+ 调度优化两个方向。

**已知排除的反优化方案**（OE1~OE5，避免重复探索）：

| 排除项 | 替代方案 | 反优化原因 |
|--------|---------|----------|
| OE1 | Channel 替换 ring buffer | 增加 copy |
| OE2 | Mutex 替换 SpinNoIrq | 增加 overhead |
| OE3 | Watch 替换 AtomicWaker | 增加 API 复杂度 |
| OE4 | Semaphore 替换 PollSet | 增加无谓唤醒 |
| OE5 | embassy-time 替换 axtask::timeout | 增加依赖 |

---

## 附录 A：术语表

| 术语 | 含义 | 首次出现 |
|------|------|---------|
| FIONBIO | **F**ile **IO**ctl **N**on-**B**locking **I**/O，ioctl 启用非阻塞 | §0 |
| EAGAIN | POSIX 错误码"再试一次"，非阻塞操作无可用数据时返回 | §3 |
| O_NONBLOCK | open 标志：启用非阻塞 I/O | §3 |
| F_SETFL | fcntl 设置文件状态标志 | §3 |
| tcdrain | POSIX 等待所有输出传输完毕 | §3 |
| TCSBRK | **T**erminal **C**ontrol **S**et **BR**ea**K**，tcdrain 对应 ioctl | §5 |
| LSR | **L**ine **S**tatus **R**egister，线状态寄存器 | §5 |
| TRANSMITTER_EMPTY | LSR bit 6：THR + 移位寄存器全空 = 真正 drain | §5 |
| THR | **T**ransmit **H**olding **R**egister，发送保持寄存器 | §5 |
| FCR | **F**IFO **C**ontrol **R**egister，FIFO 控制寄存器 | §1 |
| NAPI | **N**ew **API**（Linux 网络子系统），本项目借鉴 | §2 |
| SPSC | **S**ingle-**P**roducer **S**ingle-**C**onsumer，单生产者单消费者 | §2 |
| ISR | **I**nterrupt **S**ervice **R**outine，中断服务例程 | §2 |
| O-编号 | 项目内部"优化点"编号（O3 / O40 / O41 / O43 / O46 / O51~O53 / OE1~OE5）| §7 |
| Q-编号 | 项目内部"问题/任务"编号（Q0~Q13）| §1 |
| LTO | **L**ink **T**ime **O**ptimization，链接时优化 | §1 |
| monotonic_time_nanos | QEMU RISC-V 单调时钟，分辨率约 100ns | §1 |
| e2e | **E**nd-**t**o-**E**nd，端到端 | §0 |

---

## 附录 B：参考 commit

- `de8cd8b` — `fix(uart-async): RingBufTx::push() 缺少 wake 调用导致 Shell 挂起`
- `7bee89d`（uart_16550）— `feat(uart-async): extract TtyRead/TtyWrite traits for OS integration`
- `1005b71`（uart_16550）— `feat(uart-async): add OS abstraction traits (OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet)`
- `9bed0c7`（StarryOS）— `feat(uart-async): add ArceOS HAL adapter layer`
- `842f8f4`（StarryOS）— `refactor(uart-async): remove migrated local files, finalize StarryOS integration`
- `a0cead0`（uart_16550）— `perf(uart-async): add #[inline(always)] to ring buffer push/pop`
- `73aca5c`（uart_16550）— `perf(uart-async): add batch push/pop to reduce lock overhead`
- `9188c0b`（StarryOS）— `perf(uart-async): add #[inline(always)] to ArceOsUartPort methods`

> 链接模板：`https://github.com/<owner>/<repo>/commit/<hash>`（具体行号以本仓库 `feat/uart-16550-async` 分支当前 state 为准）。

---

**报告版本**：7.0 · **最后更新**：2026-06-17（bettermd 新规范 17 规则全量重写：H1/H2 only + 核心论点=首段 + 小结=**小结**：末段 + 结论=5 条短句 + 信息去重）
**主要变更**：移除 36 处 H3 子节 → 段落合并 + 粗体小节；§8 结论从 5 个 H3 + 3 个表压缩至 5 条核心判断（**禁止复述数据**）；首/末段统一为核心论点/小结；§0 TL;DR 5 维度作为后续章节索引，避免数据在 §6/§7 重复出现。
