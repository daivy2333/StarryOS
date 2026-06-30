# StarryOS 异步串口学习地图

> 范围：Q0~Q19B，覆盖 QEMU 异步串口、`uart_16550` 抽取、Lichee RV Dock 真板 smoke、D1 async UART kbench/userbench。
> 日期：2026-06-30
> 关联：`docs/async-uart-architecture.md`、`docs/benchmark-report-async.md`、`docs/licheerv-dock-bringup.md`、`docs/lichee-q19b-benchmark-problems-solutions.md`。

## 架构总览

当前异步串口已经从 QEMU 单平台实现演进为可跨平台迁移的分层结构：上层保持 `Tty<Reader, Writer>`、`AsyncUartDriver`、ring buffer、copier、waker 与 `tcdrain` 状态机；下层按硬件差异替换 `UartPort`。QEMU 使用 NS16550 byte MMIO，Lichee RV Dock / Allwinner D1 使用 DW APB UART stride 4 / 32-bit MMIO，这个差异由 D1 专用 `ArceOsD1UartPort` 吸收。

```text
L6 板级启动      Android boot image / QEMU direct boot
L5 用户态        Shell / embedded benchmark.elf
L4 VFS/TTY       Tty<Reader, Writer> + OPOST/ONLCR
L3 集成层        AsyncUartReader / AsyncUartWriter
L2 驱动层        AsyncUartDriver<R, W, U> + RX/TX copier
L1 硬件抽象      UartPort trait
L0 硬件          QEMU NS16550 U8 / D1 DW APB UART U32 stride 4
```

OS 抽象边界仍然是两个 trait：`OsRuntime` 提供 `spawn` 与 `block_on`，`OsWakerSet` 提供 `register` 与 `wake`。D1 真板没有推翻这个架构，真正变化发生在平台描述、boot image、UART MMIO 访问宽度、PLIC IRQ 以及用户态 payload 交付方式。

**小结**：学习重点已经从“如何写一个异步 UART”扩展为“如何让同一套异步 UART 架构跨 QEMU 与真板工作”。关键判断是：通用异步栈不应绑定具体 MMIO 模型，板级差异必须沉到底层 `UartPort` 与 platform descriptor。

## 分层入口

表 1 给出当前学习地图中每一层的主要代码入口。行号为 2026-06-30 本地工作区快照，后续变动时应优先按文件名和符号名定位。

| 层 | 主要类型或职责 | 路径 |
|---|---|---|
| L0 QEMU UART | NS16550 byte MMIO | `kernel/src/drivers/uart_init.rs` |
| L0 D1 UART | DW APB UART stride 4 / U32 | `kernel/src/drivers/d1_uart.rs:45-176` |
| L1 抽象 | `UartPort` trait | `crates/uart_16550/src/async_/driver.rs` |
| L2 驱动 | `AsyncUartDriver<R, W, U>` | `crates/uart_16550/src/async_/driver.rs` |
| L3 VFS 适配 | `AsyncUartReader` / `AsyncUartWriter` | `crates/uart_16550/src/async_/device_ops.rs` |
| L4 TTY | `/dev/console`、ONLCR、FIONBIO | `kernel/src/pseudofs/dev/tty/mod.rs:90-147` |
| L5 用户态 | embedded benchmark ELF | `kernel/src/mm/loader.rs:348-445` |
| L6 Lichee 启动 | Android boot image targets | `Makefile:60-82` |
| L6 平台参数 | RAM、UART、PLIC、boot kind | `kernel/src/platform/lichee_d1.rs:19-47` |

D1 平台描述记录了 RAM `0x40000000`、kernel load `0x40200000`、UART0 `0x02500000`、PLIC `0x10000000`、UART stride 4 / U32 access。对应配置也落在 `crates/axplat-riscv64-lichee-d1/axconfig.toml:5-28`，其中 `virtio-mmio-ranges = []` 是 smoke 阶段避免错误探测虚假设备的关键约束。

**小结**：读代码时不要从 benchmark 直接跳到 UART 寄存器。正确路径是 platform descriptor → build target → entry mode → UART init → `UartPort` → TTY/syscall/benchmark。

## 三条数据流

RX 路径保持与 QEMU 时代一致：UART RBR → ISR → `RX_WAKER.wake()` → RX copier → `receive_bytes` → RX ring buffer → TTY read。D1 的区别是 RBR/LSR/IIR 均通过 `read_reg(offset)` 以 `offset * stride` 的 U32 volatile 方式访问，代码证据是 `kernel/src/drivers/d1_uart.rs:67-87`。

TX 路径是用户态 benchmark 的主路径：`write` / `writev` → `Tty::write_at` → `AsyncUartWriter::write` → TX ring buffer → TX copier → `send_bytes` → THR。D1 需要在 `init_interrupt_mode()` 中开启 FIFO、清状态、设置 `MCR_OUT2_INT_ENABLE`，否则异步发送和 IRQ 路径无法稳定工作，代码证据是 `kernel/src/drivers/d1_uart.rs:101-117`。

Drain 路径是 Q19B 真板最关键的学习点：`tcdrain` 不能只等 TX ring buffer 为空，还要覆盖 copier staged buffer 和 UART TEMT。QEMU 稳定产生 THRE 中断，曾掩盖 D1 的 THRE edge-loss/no-pending IIR 行为；D1 backend 因此在 `update_ier(THR_EMPTY)` 后检查当前 LSR，并在 already-ready 时软件唤醒 `TX_WAKER` / `DRAIN_WAKER`，代码证据是 `kernel/src/drivers/d1_uart.rs:157-175`。

**小结**：RX、TX、Drain 三条路径中，Drain 最容易被 QEMU 掩盖。真板适配时必须证明 `tcdrain` 等待的是硬件发送完成，而不是只证明 ring buffer 已清空。

## 关键模式

表 2 总结 Q0~Q19B 后仍然有效的关键模式。新增的 D1 经验没有替代旧模式，而是把旧模式的边界条件补齐。

| 模式 | 当前含义 | 证据位置 |
|---|---|---|
| ISR 极简 | IRQ 只清状态、禁对应中断、wake，不搬运大量数据 | `crates/uart_16550/src/async_/isr.rs` |
| SPSC ring buffer | RX/TX 用 64 KiB ring buffer 解耦用户态和硬件 | `crates/uart_16550/src/async_/ring_buffer.rs` |
| AtomicWaker | `RX_WAKER` / `TX_WAKER` / `DRAIN_WAKER` 作为跨任务事件槽 | `crates/uart_16550/src/async_/isr.rs` |
| 四阶段 drain | ring / copier / staged / TEMT 全部满足才完成 | `crates/uart_16550/src/async_/driver.rs` |
| 硬件能力与运行模式拆分 | `lichee-d1-async-uart` 不能等同于 kbench/userbench | `kernel/src/entry.rs:145-239` |
| embedded payload | 无 SDMMC/rootfs 时先内嵌 `benchmark.elf` | `kernel/src/mm/loader.rs:348-445` |
| TTY ONLCR | 串口终端输出 LF 要映射为 CRLF | `kernel/src/pseudofs/dev/tty/mod.rs:107-145` |

这些模式分别解决不同层级的问题：ring buffer 解决吞吐与解耦，AtomicWaker 解决异步唤醒，四阶段 drain 解决 POSIX 语义，feature 拆分解决多运行模式互相污染，embedded payload 解决真板早期无 rootfs 的测试交付。

**小结**：Q19B 的价值不只是“D1 跑通了 benchmark”，而是把 QEMU 上隐含的假设逐项显式化：硬件访问宽度、IRQ ready-state、rootfs 缺失、TTY 输出格式和 feature 语义都必须可验证。

## Milestone 路线

表 3 将异步串口从 Q0 到 Q19B 的学习路线重新排序。前半段适合理解通用异步驱动，后半段适合理解真板 bring-up。

| 阶段 | 学习目标 | 关键产物 |
|---|---|---|
| Q0~Q7 | 从同步串口走到用户态可测异步串口 | FIONBIO、tcdrain、benchmark 修正 |
| Q8~Q12 | 打磨 ISR、NAPI、ring buffer、drain | NAPI 退出、AtomicWaker、Embassy SPSC |
| Q13 | 抽取 `uart_16550` crate | OS trait 抽象、`UartPort` |
| Q15 | 增量修复 TX completion 与性能回归 | staged/TEMT 四阶段 drain |
| Q18 | 平台参数解耦与 early console | platform descriptor、early console 分层 |
| Q19 | Lichee early smoke | Android boot image、D1 axplat、C906 PTE 修复 |
| Q19B | D1 async UART benchmark | kbench/userbench、PLIC IRQ 18、embedded benchmark |

Q19 完成前只证明 boot image、early mapping、early console 和 halt 能工作。Q19B 才证明 async UART、`/dev/console`、syscall、`tcdrain` 与用户态 benchmark 路径成立。两者不能混为一个验收标准。

**小结**：学习路线应先读通用异步栈，再读平台适配；先理解 smoke，再理解 benchmark。否则容易把 rootfs、PCI、ELF、TTY 或 UART 中断问题误判成同一个“板子跑不起来”问题。

## 学习进度

截至 2026-06-30，原 8 站学习地图扩展为 12 站。前 8 站对应通用异步串口，后 4 站对应 Lichee 真板适配。

| 站 | 主题 | 状态 |
|---|---|---|
| 1 | 入口与全局实例 | ✅ |
| 2 | `AsyncUartDriver` 三泛型结构 | ✅ |
| 3 | ISR 4 步流程 + 3 个 AtomicWaker | ✅ |
| 4 | RX copier + NAPI 状态机 | ⬜ |
| 5 | TX copier + fast retry + TEMT | ⬜ |
| 6 | `ProcessMode::External` 桥接 | ⬜ |
| 7 | OS 抽象具体实现 | ⬜ |
| 8 | VFS 接口 + flush 实现 | ⬜ |
| 9 | Q18 platform descriptor / early console | ✅ |
| 10 | Q19 Lichee Android boot image smoke | ✅ |
| 11 | Q19B D1 `UartPort` + PLIC IRQ 18 | ✅ |
| 12 | Q19B embedded userbench + TTY/ELF | ✅ |

下一轮学习若继续补笔记，应优先写第 4、5、8 站，因为这些是 Q19B 真板问题暴露最多的共享机制：NAPI、TX copier、flush/drain。

**小结**：当前学习进度不再只按“读了多少异步驱动源码”衡量，还要按“能否解释真板上为何失败、为何修复”衡量。Q19B 已经把第 9~12 站补成可复用经验。

## 阅读顺序

推荐阅读顺序分为三段。第一段读通用栈：`crates/uart_16550/src/async_/driver.rs`、`isr.rs`、`device_ops.rs`。第二段读 StarryOS 接入：`kernel/src/drivers/uart_init.rs`、`kernel/src/drivers/ntty_async.rs`、`kernel/src/pseudofs/dev/tty/mod.rs`。第三段读真板路径：`kernel/src/platform/lichee_d1.rs`、`crates/axplat-riscv64-lichee-d1/axconfig.toml`、`kernel/src/drivers/d1_uart.rs`、`kernel/src/entry.rs`、`kernel/src/mm/loader.rs`。

真板文档建议按下面顺序读：

1. `docs/lichee-adaptation-prework.md`：先知道需要采集什么信息。
2. `docs/lichee-smoke-problems.md`：理解 smoke 前遇到的失败类型。
3. `docs/lichee-smoke-solutions.md`：理解 smoke 如何被拆解并修复。
4. `docs/lichee-q19b-benchmark-problems-solutions.md`：理解 kbench/userbench 和真实性能。

**小结**：不要从最后的性能数据反推全部实现。正确的学习顺序是：硬件事实 → boot 事实 → smoke 最小闭环 → async UART 真板闭环 → 性能解释。

## ADR 与经验索引

表 4 汇总当前最值得反复查阅的 ADR 与 learned 条目。它们是学习地图的事实来源。

| 编号 | 主题 |
|---|---|
| ADR-037 | TxCompletion 四阶段 |
| ADR-038 | TtyWrite 短写契约 |
| ADR-039 | Q15 增量融合策略 |
| ADR-044 | 平台参数解耦 |
| ADR-047 | Q19B 先嵌入 benchmark payload |
| ADR-048 | D1 先做平台专用 `UartPort` |
| ADR-049 | D1 userbench 最小 axfs-ng patch |
| ADR-050 | 硬件能力 feature 与运行模式 feature 拆分 |
| ADR-051 | D1 THRE 边沿丢失与 drain wake |
| L231-L235 | Q19 smoke 阶段经验 |
| L236-L258 | Q19B benchmark 阶段经验 |

这些条目共同形成一个边界：QEMU benchmark 仍用于相对优化和回归测试；D1 真板数据用于验证真实 115200 bps 串口线速；VisionFive2 后续需要单独采集多核真板数据。

**小结**：学习地图的最新结论是分层复用而不是平台复制。D1 的成功经验可以复用到 VisionFive2 的 bring-up 流程，但不能假设 UART 地址、IRQ、MMIO width、boot image 或 SMP 能力相同。
