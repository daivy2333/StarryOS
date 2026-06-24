# Q13+ 异步串口演进报告

> **时间窗**：2026-06-16 ~ 2026-06-23
> **范围**：Q13 模块分离、Q13.1 性能回收、Q15 M0~M4 五阶段深化
> **关联分支**：`feat/uart-16550-bench`（双 repo）
> **关联文档**：`async-uart-architecture.md`、`uart-performance-comparison.md`、`benchmark-report-async.md`、`.claude/analysis/async-uart-module-boundary.md`

---

## §1 背景

Q0~Q12 在 StarryOS 内核累计 ~618 行异步串口栈。栈耦合 axplat 抽象层，跨 OS 复用困难。Q13 解决复用问题，Q15 解决性能与诊断问题。

## §2 Q13：模块分离（2026-06-16）

9 个原子提交。逻辑分层为：

- **通用层 ~400 行**（[`uart_16550/src/async_/`](https://github.com/daivy2333/uart_16550/tree/feat/uart-16550-bench/src/async_)）：isr / ring_buffer / driver / device_ops / mod
- **OS 适配层 ~123 行**（[`os_arceos.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-bench/kernel/src/drivers/os_arceos.rs)）：5 个 trait 的 ArceOS 实现
- **OS 集成层 ~155 行**（[`uart_init.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-bench/kernel/src/drivers/uart_init.rs) + [`ntty_async.rs`](https://github.com/daivy2333/StarryOS/blob/feat/uart-16550-bench/kernel/src/pseudofs/dev/tty/ntty_async.rs)）：平台硬件初始化 + TTY 进程绑定

5 个 OS 抽象 trait（[`os/mod.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/os/mod.rs)）：`OsRuntime` / `OsIrq` / `OsMmio` / `OsSpinNoIrq` / `OsWakerSet`。每个 trait 对应一种不可绕过的 OS 能力。Linux / Zephyr / Embassy / FreeRTOS / RT-Thread / 裸机 6 种目标 OS 接入成本约 40~150 行 trait 实现。

通用层 65% / OS 层 35% 的拆分与 Linux 子系统、Zephyr 驱动模型接近。

## §3 Q13.1：性能回收（2026-06-16）

Q13 引入 trait 抽象带来 +5.5 µs 软件 overhead（53.3 µs）。Q13.1 通过 3 个 commit 回收 10.7 µs 至 42.6 µs：

- [`a0cead0`](https://github.com/daivy2333/uart_16550/commit/a0cead0)：`#[inline(always)]` 加在 ring buffer push/pop
- [`73aca5c`](https://github.com/daivy2333/uart_16550/commit/73aca5c)：批量 push/pop 减少锁获取次数
- [`9188c0b`](https://github.com/daivy2333/StarryOS/commit/9188c0b)：`#[inline(always)]` 加在 ArceOsUartPort 方法

LTO 跨 crate 内联（已 revert per ADR-034）使内核态 ring buffer TX 385→652 MB/s（+69%）。e2e 延迟不变，瓶颈在调度。

## §4 Q15 M0：诊断 telemetry（2026-06-23）

[`telemetry.rs`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/async_/telemetry.rs) 新增 TX copier 计数器，通过 `feature = "telemetry"` gate 控制。三个计数器：

- `tx_poll`：poll_fn 调用次数
- `tx_no_progress`：`send_bytes()` 返回 0 的次数（THR 满）
- `tx_hw_bytes`：成功写入 FIFO 的总字节数

计数器使用 `Ordering::Relaxed`，是信息性的，不参与同步。feature 关闭时全部编译为 0 开销。

## §5 Q15 M1：bounded 快速重试（2026-06-23）

`tx-bounded-fast-retry` spec。替换 Q12 归档的 embassy-time 路径。bounded 语义避免无界重试导致调度饿死。

## §6 Q15 M2：完成追踪（2026-06-23）

两个 spec 域：

- **tx-completion-tracking**：用 LSR.TEMT 硬件位追踪 TX 完成，替代软件 `TCDRAIN_ACTIVE` 标志
- **uart-temt-query**：暴露 `LSR::TRANSMITTER_EMPTY` 查询接口

缩短 tcdrain 三段等待时间（详见 [async-uart-architecture.md §3.4](../docs/async-uart-architecture.md)）。

## §7 Q15 M3：TtyWrite 短写契约（2026-06-23）

`async-uart-traits` spec 扩展。`TtyWrite::write()` 在 ring buffer 满时返回短写长度而非阻塞。Tty 层与 ring buffer 边界清晰化。

## §8 Q15 M4：IER 单所有权（2026-06-23）

三个 spec 域：

- **ier-isr-refactor**：IER 写入从 ISR 路径移出，移至 copier 任务
- **ier-port-ownership**：IER 状态归 copier 任务单一所有权
- **ier-port-ownership**：port-level IER 抽象

`set_ier()` 公共 API（[`lib.rs:830`](https://github.com/daivy2333/uart_16550/blob/feat/uart-16550-bench/src/lib.rs#L830)）暴露给 OS 集成层。ISR 只读 IER，不写。

## §9 量化效果

`feat/uart-16550-bench` 分支两次手动测试（QEMU，间隔 2 分钟）：

| 指标 | Q13+LTO | Q15 当前（无 LTO）| 变化 |
|------|---------|-------------------|------|
| 内核态 TX | 651,890 KB/s | 456,205 KB/s | -30%（LTO 关闭所致）|
| 内核态 RX | 897,616 KB/s | **1,147,959 KB/s** | **+27.9%**（M0~M4 lock-free 改进）|
| 用户态 1B e2e avg | 129.5 µs | 134 µs | +3.5%（调度瓶颈未变）|
| 用户态 1B P50 | 129.5 µs | 118.5 µs | -8.5% |
| 用户态 64B TX | 124 KB/s | 170 KB/s | +37% |
| FIONBIO 三入口 | 全 PASS | 全 PASS | 一致 |

LTO 跨 crate 内联状态：455 → 652 MB/s（+69%，已 revert per ADR-034）。TX 下降 30% 是 LTO 关闭的预期结果，RX 提升 27.9% 是 Q15 M0~M4 路径改进的真实效果。

## §10 代码变更统计

- **Q13**（9 commits）：3 阶段迁移（trait → 栈代码 → 适配层）
- **Q13.1**（3 commits）：inline + batch 回收
- **Q15**（5 commits）：M0~M4 五阶段
- **LTO**（1 commit，reverted）：跨 crate 内联
- **总计**：~18 commits 涉及双 crate

新增 OpenSpec spec 域：tx-bounded-fast-retry、tx-completion-tracking、uart-temt-query、ier-isr-refactor、ier-port-ownership、benchmark-fifo-matrix、uart-telemetry（7 个）。

## §11 后续

- **Q6**：VisionFive2 真板验证（DMA、波特率扩展）
- **Q15 进一步深化**：OsIrq/OsMmio/OsSpinNoIrq 已 remove（commit `60c5729`），可观察是否需要
- **DMA**：新增 `OsDma` trait（Q6 启用 DMA 提升吞吐时）
- **PM**：suspend/resume 支持（新增 `OsPm` trait）

---

**报告版本**：1.0 · **生成日期**：2026-06-23
**数据来源**：[`benchmark-report-async.md`](../docs/benchmark-report-async.md) §3.6 FIFO 矩阵、两次手动测试（2026-06-24）
**commit 索引**：[Q13 9 commits](https://github.com/daivy2333/StarryOS/commits/feat/uart-16550-bench) · [Q13.1 3 commits](https://github.com/daivy2333/StarryOS/commits/feat/uart-16550-bench) · [Q15 M0~M4 5 commits](https://github.com/daivy2333/uart_16550/commits/feat/uart-16550-bench)
