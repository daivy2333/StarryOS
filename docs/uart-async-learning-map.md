# StarryOS 异步串口学习地图

> 范围：Q0~Q19C-M1 + Q17 SMP/内存序。
> 日期：2026-07-08。
> 关联：docs/async-uart-architecture.md、benchmark-report-async.md、licheerv-dock-bringup.md、Q19cM1.md、`.claude/analysis/q19c-d1-tx-optimization.md`、`.claude/analysis/q19c-m1-memory-root-path-loader.md`。

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
| L4 TTY | `/dev/console` + ONLCR + FIONBIO | `kernel/src/pseudofs/dev/tty/mod.rs:90-147` |
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

TX copier 四阶段重试：fast retry（32 次 spin）→ final recheck → slow-pool（4096 × 256 spin）→ yield 重试（4 次自唤醒）→ 纯 ISR 等待。证据：`crates/uart_16550/src/async_/driver.rs:460-672`。

D1 需在 `init_interrupt_mode()` 开 FIFO、清状态、设 `MCR_OUT2_INT_ENABLE`。证据：`kernel/src/drivers/d1_uart.rs:101-117`。

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
| TTY ONLCR | 串口 LF 映射为 CRLF | `kernel/src/pseudofs/dev/tty/mod.rs:107-145` |

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

Q19 完成 boot/early mapping/early console/halt。Q19B 才证明 async UART 与 benchmark 路径。两者不能混为一个验收标准。

## 学习进度

截至 2026-07-04，14 站中已完成 8 站。按 4 → 5 → 6 → 7 → 8 → 9 → 10 → 11 → 12 → 13 → 14 顺序推进。

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
| 9 | Q18 platform descriptor / early console | ⬜ |
| 10 | Q19 Lichee Android boot image smoke | ⬜ |
| 11 | Q19B D1 `UartPort` + PLIC IRQ 18 | ⬜ |
| 12 | Q19B embedded userbench + TTY/ELF | ⬜ |
| 13 | Q17 跨 hart 内存序 | ⬜ |
| 14 | Q19C-M0 slow-pool + yield + gated TX debug | ⬜ |

下一轮按顺序：9 → 10 → 11 → 12 → 13 → 14。

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

QEMU benchmark 仍用于相对优化和回归测试。D1 真板数据用于验证真实 115200 bps 线速。VisionFive2 后续需单独采集多核真板数据。

## 阅读顺序

### 通用栈

`crates/uart_16550/src/async_/driver.rs` → `isr.rs` → `device_ops.rs`。

### StarryOS 接入

`kernel/src/drivers/uart_init.rs` → `ntty_async.rs` → `pseudofs/dev/tty/mod.rs`。

### 真板路径

`kernel/src/platform/lichee_d1.rs` → `crates/axplat-riscv64-lichee-d1/axconfig.toml` → `d1_uart.rs` → `entry.rs` → `mm/loader.rs`。

### 真板文档

`docs/lichee-adaptation-prework.md` → `lichee-smoke-problems.md` → `lichee-smoke-solutions.md` → `lichee-q19b-benchmark-problems-solutions.md`。

硬件事实 → boot 事实 → smoke 最小闭环 → async UART 真板闭环 → 性能解释。不要从性能数据反推全部实现。