# StarryOS 异步串口学习地图

> 范围:Q0~Q32 已完成/归档路径，以及 Q24/Q25/Q30 的证据触发边界。
> 日期:2026-07-30。
> 关联:`docs/async-uart-architecture.md`、`benchmark-report-async.md`、`licheerv-dock-bringup.md`、`docs/manual-qa-report.md`、`.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-d1-tx-optimization.md`、`.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-m1-memory-root-path-loader.md`、`openspec/changes/archive/2026-07-13-q20-benchmark-gap-closure/evidence/`、`openspec/specs/{project-model,decisions,knowledge,references,improvements}/spec.md`。
>
> 当前分支 `uart-lichee`；D1 真板异步 UART 测试与 Q31/Q32 对照已结束，Q24/MS02 SMP 复验仍等待硬件。

## 概览

异步串口已分层为 platform descriptor → UartPort → AsyncUartDriver → Tty → VFS → Shell/用户态。

通用异步栈不绑定 MMIO 模型。板级差异沉到 UartPort 与 platform descriptor。

## 架构分层

| 层 | 职责 |
|---|---|
| L6 启动 | Android boot image / QEMU direct boot |
| L5 用户态 | Shell / embedded benchmark.elf |
| L4 VFS/TTY | Tty<Reader, Writer> + OPOST/ONLCR |
| L3 集成层 | AsyncUartReader / AsyncUartWriter |
| L2 驱动 | AsyncUartDriver + RX/TX copier |
| L1 抽象 | UartPort trait |
| L0 硬件 | QEMU NS16550 / D1 DW APB UART |

## 关键代码入口

| 层 | 类型 | 路径 |
|---|---|---|
| L0 QEMU | NS16550 byte MMIO | `kernel/src/drivers/uart_init.rs` |
| L0 D1 | DW APB UART stride 4 | `kernel/src/drivers/d1_uart.rs:45-176` |
| L1 抽象 | `UartPort` trait | `crates/uart_16550/src/async_/driver.rs` |
| L2 驱动 | `AsyncUartDriver<R, W, U>` | `crates/uart_16550/src/async_/driver.rs` |
| L2 诊断 | `TxDebugSnapshot` + slow-pool 计数器 | `crates/uart_16550/src/async_/driver.rs:95-124` |
| L2 诊断桥接 | `UartTxDebugSnapshot` ioctl | `kernel/src/syscall/fs/ctl.rs:31-69` |
| L3 适配 | `AsyncUartReader/Writer` | `crates/uart_16550/src/async_/device_ops.rs` |
| L4 TTY ONLCR | `kernel/src/pseudofs/dev/tty/mod.rs:107-145` |
| L4 TX backpressure | UART OUT 绑定 TX ring 空间 + writable waker;PTY 保留 always-OUT/short-write(Q27) | `kernel/src/pseudofs/dev/tty/mod.rs` + `crates/uart_16550/src/async_/device_ops.rs` |
| L3 readiness API | `RingBufTx`/`RingBufRx` + `AsyncUartWriter`/`AsyncUartReader` 的 `can_*` + `register_*_waker`(Q27a) | `crates/uart_16550/src/async_/device_ops.rs`、`ring_buffer.rs` |
| L3 writer 契约 | raw `AsyncUartWriter` unsafe 唯一构造 + `&mut self` 提交(Q28) | `crates/uart_16550/src/async_/device_ops.rs` |
| L3 OS 串行化 | `Arc<SpinNoPreempt<RawArceOsWriter>>` 保留 cloneable adapter(Q28) | `kernel/src/drivers/uart_init.rs` |
| L5 用户态 | embedded benchmark ELF | `kernel/src/mm/loader.rs:348-445` |
| L5 诊断 | benchmark.c gated TX debug snapshot | `tests/benchmark.c:64-156` |
| L6 启动 | Android boot image targets | `Makefile:60-82` |
| L6 平台参数 | RAM/UART/PLIC/boot | `kernel/src/platform/lichee_d1.rs:19-47` |

D1 平台记录 RAM `0x40000000`、kernel load `0x40200000`、UART0 `0x02500000`、PLIC `0x10000000`、stride 4 / U32。配置见 `crates/axplat-riscv64-lichee-d1/axconfig.toml:5-28`。

`virtio-mmio-ranges = []` 是 smoke 阶段避免错误探测虚假设备的关键约束。

## 数据流

### RX

UART RBR → ISR → `RX_WAKER.wake` → RX copier → `receive_bytes` → RX ring → Tty read。

D1 通过 `read_reg(offset)` 以 `offset × stride` 的 U32 volatile 访问 RBR/LSR/IIR。证据：`kernel/src/drivers/d1_uart.rs:67-87`。

### TX

`write` / `writev` → `Tty::write_at` → `AsyncUartWriter::write` → TX ring → TX copier → `send_bytes` → THR。

TX copier 四阶段重试:fast retry(32 次 spin)→ final recheck → slow-pool(4096 × 256 spin)→ yield 重试(4 次自唤醒)→ 纯 ISR 等待。证据:`crates/uart_16550/src/async_/driver.rs:460-672`。

阻塞 fd 在 TX ring 满时通过 `Tty::poll()` 等待 `IoEvents::OUT`,`Tty::register()` 对 OUT 注册 TX ring writable waker;nonblocking fd 保持 partial / `WouldBlock`,空写保持 fast path(Q27)。证据:`crates/uart_16550/src/async_/device_ops.rs`、`kernel/src/pseudofs/dev/tty/mod.rs`。

D1 需在 `init_interrupt_mode()` 开 FIFO、清状态、设 `MCR_OUT2_INT_ENABLE`。证据:`kernel/src/drivers/d1_uart.rs:101-117`。

### Drain

`tcdrain` 等四阶段：ring 空、copier 未在 poll、`staged_bytes == 0`、`TEMT == 1`。

QEMU 稳定产生 THRE 中断。D1 THRE 边沿可能丢失，靠 `update_ier(THR_EMPTY)` 后软件 wake 兜底。证据：`kernel/src/drivers/d1_uart.rs:157-175`。

## 关键模式

| 模式 | 当前含义 | 位置 |
|---|---|---|
| ISR 极简 | ISR 只关中断+wake，不搬运 | `crates/uart_16550/src/async_/isr.rs` |
| SPSC ring buffer | RX/TX 各 64 KiB 解耦用户态与硬件 | `crates/uart_16550/src/async_/ring_buffer.rs` |
| AtomicWaker | `RX_WAKER` / `TX_WAKER` / `DRAIN_WAKER` 跨任务事件槽 | `crates/uart_16550/src/async_/isr.rs` |
| 四阶段 drain | ring / copier / staged / TEMT 全满足才完成 | `crates/uart_16550/src/async_/driver.rs` |
| slow-pool + yield 重试 | budget exhausted 后 bounded slow-poll → yield 自唤醒 → 纯 ISR 等待 | `crates/uart_16550/src/async_/driver.rs:578-672` |
| 跨 hart 内存序按角色选序 | 非原子 RMW 用锁隔离；flag Release/Acquire；计数 RMW AcqRel | `uart_init.rs:120`、`d1_uart.rs:157`、`driver.rs:167-171` |
| 硬件能力与运行模式拆分 | `lichee-d1-async-uart` 不等同 kbench/userbench | `kernel/src/entry.rs:145-239` |
| embedded payload | 无 SDMMC/rootfs 时内嵌 `benchmark.elf` | `kernel/src/mm/loader.rs:348-445` |
| TTY ONLCR | 串口 LF 映射为 CRLF;nonblocking 边界停在完整源字符,避免 `a\n` 误提交为 `a\r` | `kernel/src/pseudofs/dev/tty/mod.rs:107-145` |
| Readiness 薄接口(Q27a) | `RingBufTx`/`RingBufRx` 暴露 `vacant_len`/`occupied_len`/`has_space`/`has_data`;`AsyncUartWriter`/`Reader` 暴露 `can_write`/`can_read` + waker register;**readiness hint 不保证后续 push/pop 成功,OS 层必须 register 后 recheck** | `crates/uart_16550/src/async_/device_ops.rs`、`ring_buffer.rs` |
| TX backpressure(Q27) | UART 阻塞 fd 用 `poll_io(OUT)` 累计完成;nonblocking 保持 partial/`WouldBlock`;PTY 保留 always-OUT/short-write | `kernel/src/pseudofs/dev/tty/mod.rs` |
| SPSC writer 契约(Q28) | raw `AsyncUartWriter` 不可 clone、unsafe 唯一构造、`&mut self` 提交,`RingBufTx::push` 收窄为 crate-private | `crates/uart_16550/src/async_/device_ops.rs` |
| OS 串行化 adapter(Q28) | StarryOS `Arc<SpinNoPreempt<RawArceOsWriter>>` 保留 cloneable adapter,锁只覆盖单次 nonblocking push,不跨 `poll_io`/await/等待点 | `kernel/src/drivers/uart_init.rs` |
| SPSC readiness 快照(Q27a) | RX consumer Acquire `end` → `start`;TX producer Acquire `start` → `end`;取模 `2 * capacity` 跨越 wrap-around;**不得通过 `UnsafeCell` 跨角色借 `Reader`/`Writer`** | `crates/uart_16550/src/async_/ring_buffer.rs` |

## Milestone 路线

| 阶段 | 目标 | 关键产物 |
|---|---|---|
| Q0~Q7 | 同步串口→用户态可测异步串口 | FIONBIO、tcdrain、benchmark 修正 |
| Q8~Q12 | ISR / NAPI / ring buffer / drain 打磨 | NAPI 退出、AtomicWaker、Embassy SPSC |
| Q13 | `uart_16550` crate 抽取 | OS trait 抽象、`UartPort` |
| Q15 | 增量修复 TX completion 与性能回归 | staged/TEMT 四阶段 drain |
| Q17 | SMP/内存序正确性（2026-07-03 landed，QEMU 验证）| ier_cache 锁内 RMW、tx flag/counter Release/Acquire/AcqRel |
| Q18 | 平台参数解耦、early console | platform descriptor、early console 分层 |
| Q19 | Lichee early smoke | Android boot image、D1 axplat、C906 PTE 修复 |
| Q19B | D1 async UART benchmark | kbench/userbench、PLIC IRQ 18、embedded benchmark |
| Q19C-M0 | benchmark evidence cleanup + TX copier slow-pool | manifest 统一、gated TX debug snapshot、slow-pool + yield 重试、P99 长尾根因未探明 |
| Q19C-M1 | memory-root path loader proof | `FS_CONTEXT.resolve()/read()` + eager ELF mapping，真板 `benchmark exited with code: 0`；lazy file-backed COW SIGILL 记 O80/L277 |
| Q19C-M2 | D1 async UART board benchmark 收尾 | `lichee-memory-root-command` 作为完成目标；M3/rootfs-probe、Q19D SDMMC/rootfs 取消当前规划(ADR-055) |
| Q20 | Benchmark gap closure | QEMU+D1 TX latency / jitter / counter proxy + raw evidence；不改驱动语义；RX fixed payload 排除(ADR-057) |
| Q21/Q22/Q23 | user ring/completion 路线决策 | D1 115200 bps 线速已成瓶颈，无可见收益；当前不实施，保留 batch/writev/tx counter 路径(ADR-058) |
| Q27a | `uart_16550` readiness 薄接口(O83) | RX/TX ring 状态观测 + readable/writable waker 注册，不引入 OS fd 语义；59 unit tests + 8 doctests |
| Q27 | TX backpressure / writable wait MVP(O83) | UART 阻塞 fd 等待 TX ring 空间，非阻塞保持 partial/`WouldBlock`，PTY 保持 short-write；D1 S11 1024B 从 36 short writes/65536B 改善为 0/102400B |
| Q28 | AsyncUartWriter writer 契约收敛(O84) | raw writer 不可 clone、unsafe 唯一构造 + `&mut self` 提交；OS 串行化保留 cloneable adapter；MPSC 后置(ADR-061) |

### 后续(Q29/Q30/Q24/Q25/Q26,登记在 roadmap)

| 阶段 | 目标 | 触发条件 |
|---|---|---|
| Q29 | AsyncUartReader consumer 契约审计(O87) | `openspec-plan` 启动;审计 safe constructor / 共享路径 / SPSC 单 consumer witness |
| Q30 | TX 多 producer 语义决策(O85/O86) | 仅当 Q24 SMP 或真实 workload 提供原子性 / 公平性 / 锁竞争证据 |
| Q24 | VisionFive2 / multi-hart 复验(O63~O71/O38/O39) | 等待硬件 |
| Q25 | DMA / 高波特率决策(O3/O40/O41/O69) | 等待 Q24 或新硬件数据 |
| Q26 | 维护性清理(O48/O49/O50 + ADR-034 LTO) | 待做,不阻塞其他主线 |

Q19 完成 boot/early mapping/early console/halt。Q19B 才证明 async UART 与 benchmark 路径。两者不能混为一个验收标准。

## 学习进度

截至 2026-07-16，原 14 站已全部完成(Q19C-M1 后补 Q19C-M2 / Q20 / Q27a / Q27 / Q28 站点)。

| 站 | 主题 | 状态 |
|---|---|---|
| 1 | 入口与全局实例 | ✅ |
| 2 | `AsyncUartDriver` 三泛型结构 | ✅ |
| 3 | ISR 4 步流程 + 3 个 AtomicWaker | ✅ |
| 4 | RX copier + NAPI 状态机 | ✅ (2026-07-04，含两级 waker + spawn 任务答疑) |
| 5 | TX copier + fast retry + TEMT | ✅ (2026-07-04，含四阶段 drain + register-then-recheck) |
| 6 | `ProcessMode::External` 桥接 | ✅ (2026-07-04，含 PTY master/slave 选型 + AsyncUart 真实用法) |
| 7 | OS 抽象具体实现 | ✅ (2026-07-04，5→2 trait 简化 + ArceOS 适配) |
| 8 | VFS 接口 + flush 实现 | ✅ (2026-07-04，5 层 flush + TCSBRK 双份反模式) |
| 9 | Q18 platform descriptor / early console | ✅ |
| 10 | Q19 Lichee Android boot image smoke | ✅ |
| 11 | Q19B D1 `UartPort` + PLIC IRQ 18 | ✅ |
| 12 | Q19B embedded userbench + TTY/ELF | ✅ |
| 13 | Q17 跨 hart 内存序 | ✅ (QEMU 单 hart 验证;多 hart 后置 Q24) |
| 14 | Q19C-M0 slow-pool + yield + gated TX debug | ✅ |
| 15 | Q19C-M1 memory-root path loader proof | ✅ (2026-07-11) |
| 16 | Q19C-M2 board benchmark 收尾 | ✅ (2026-07-11,ADR-055) |
| 17 | Q20 benchmark gap closure | ✅ (2026-07-13,ADR-057) |
| 18 | Q21/Q22/Q23 user ring/completion 决策 | ✅ (2026-07-13,ADR-058) |
| 19 | Q27a `uart_16550` readiness 薄接口 | ✅ (2026-07-15,O83) |
| 20 | Q27 TX backpressure / writable wait | ✅ (2026-07-15 已归档,O83) |
| 21 | Q28 AsyncUartWriter writer 契约收敛 | ✅ (2026-07-15 已归档,O84) |

### 下一轮登记(roadmap,不属“学习站”)

- Q29 → `openspec-plan` 审计 AsyncUartReader consumer 契约(O87,ADR-062)
- Q30 → 仅在 Q24 / workload 证据触发时规划(O85/O86,ADR-062)
- Q24 / Q25 → 等待硬件与真板数据
- Q26 → 维护性 backlog,按需推进

## ADR 与经验索引

| 编号 | 主题 |
|---|---|
| ADR-037 | TxCompletion 四阶段 |
| ADR-038 | TtyWrite 短写契约 |
| ADR-039 | Q15 增量融合策略 |
| ADR-042 | Q17 内存序按语义选择，不按架构分叉 |
| ADR-044 | 平台参数解耦 |
| ADR-047 | Q19B 先嵌入 benchmark payload |
| ADR-048 | D1 先做平台专用 `UartPort` |
| ADR-049 | D1 userbench 最小 axfs-ng patch |
| ADR-050 | 硬件能力 feature 与运行模式 feature 拆分 |
| ADR-051 | D1 THRE 边沿丢失与 drain wake |
| ADR-052 | Q19C 完整 StarryOS benchmark 先走 memory-root path loader |
| ADR-055 | Q19C-M2 command-entry 是收尾 gate，M3/rootfs-probe 取消 |
| ADR-056(A056,已归档 arc-202607152005) | Q21/Q22/Q23 排期历史背景；被 ADR-058 取代 |
| ADR-057 | Q20 只收敛 benchmark 证据，不改变 UART 驱动语义 |
| ADR-058 | 取消 Q21/Q22 user ring/completion 当前规划 |
| ADR-059 | lint 与测试 Gate 按 artifact / feature / target 分层 |
| ADR-060 | async UART 与 io_uring 同构点识别 + 借鉴方向定档 |
| ADR-061 | UART backpressure 与 writer 并发边界分阶段处理(Q27/Q28) |
| ADR-062 | Q28 后 TX/RX 并发契约分流(Q29/Q30) |
| L212 | Q17 内存序选型速查 |
| L231-L235 | Q19 smoke 阶段经验 |
| L236-L258 | Q19B benchmark 阶段经验 |
| L263 | Q17 当前分支复核边界 |
| L264 | Q17 收尾验证边界 |
| L265 | Q19C 64B 小包测量污染边界 |
| L266 | Q19C TX drain/THRE 长尾排查经验 |
| L275 | Q19C.8e slow-pool + yield 真板验证结果 |
| L276 | Q19C-M1 memory-root path loader API 速查 |
| L277 | Q19C-M1 lazy file-backed loader 踩坑（O80） |
| L286 | QEMU/D1-first 重排；Q20 完成、Q21/Q22 取消当前规划、O82 保留可借鉴项 |
| L287 | Q20 benchmark gap 入口(`tests/benchmark.c` + QEMU/D1 target + TX debug ioctl + raw log) |
| L288 | 内嵌 `uart` manifest parity(Cargo.toml 缺 `resolver="3"` / `embedded-io` / `assert2`) |
| L289 | workspace lint 边界(path dependency 自动纳入 members,`deny(clippy::cargo)` 跨包报告) |
| L290 | kernel 测试 Gate 分层(host vs target+feature vs QEMU/真板) |
| L291 | io_uring 设计思想映射表 |
| L292 | `TxCompletion` 是全局 drain snapshot，不是 CQE 超集 |
| L293 | Q28 前 `AsyncUartWriter::Clone` 潜在的 MPSC 隐患 |
| L294 | TX ring push 保持 short-write 原语，阻塞策略由 OS 层补齐 |
| L295 | UART TX backpressure 应复用 Pollable OUT 模式(Q27) |
| L296 | `AsyncUartWriter::Clone` 与 `RingBufTx` SPSC 契约必须收敛(Q28) |
| L297 | SPSC readiness 快照必须保持 reader/writer 角色归属(Q27a) |
| L298 | TTY backpressure 必须同时区分字符映射边界与 writer 等待策略(Q27) |
| L299 | Q28 后 UART 并发边界必须按证据类型拆分(跨 hart / syscall 原子性 / SPSC-MPSC) |

QEMU benchmark 仍用于相对优化和回归测试(Q17/Q27/Q28 都以 QEMU 作为功能/性能 Gate)。D1 真板数据用于验证真实 115200 bps 线速:Q19C 64B 96.7%、1024B 98.8% 线速;Q20 jitter S40 `slow_poll_exh=0`/`yield_exh=0`;Q27 S11 1024B 从 36 short writes/65536B 改善为 0/102400B。VisionFive2(Q24)后续需单独采集多核真板数据,验证跨 hart write/flush/tcdrain、read 与 IER enable/disable。

## 阅读顺序

### 通用栈

`crates/uart_16550/src/async_/driver.rs` → `isr.rs` → `ring_buffer.rs`(readiness 快照) → `device_ops.rs`(readiness API + writer 契约)。

### StarryOS 接入

`kernel/src/drivers/uart_init.rs`(OS 串行化 adapter)→ `ntty_async.rs` → `pseudofs/dev/tty/mod.rs`(ONLCR + TX backpressure)。

### 真板路径

`kernel/src/platform/lichee_d1.rs` → `crates/axplat-riscv64-lichee-d1/axconfig.toml` → `d1_uart.rs` → `entry.rs` → `mm/loader.rs`。

### 真板文档

`.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/lichee-rv-dock-adaptation-plan.md` → `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/q19b-current-blockers.md` → `.claude/analysis/_archive/2026-07-04-q19-lichee-analysis/q19b-lichee-benchmark-plan.md` → `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/q19c-lichee-full-starryos-benchmark.md`。

硬件事实 → boot 事实 → smoke 最小验证 → async UART 真板验证 → 性能解释。不要从性能数据反推全部实现。
