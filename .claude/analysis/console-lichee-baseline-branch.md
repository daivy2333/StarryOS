# Console Lichee 基线分支分析

> Project: StarryOS  
> Branch at analysis time: `uart-16550-lichee`  
> Candidate async baseline: `1ce95d7128e9c5583fc28628c72fb7c5c5e62db4`  
> Generated: 2026-07-20  
> See also: [Q20 benchmark gap closure](q20-benchmark-gap-closure.md), [QEMU async raw log](../../docs/qemu_out.md), [D1 async raw log](../../docs/d1_out.md)

## 目标

在独立的 `console-lichee` 分支中，把用户态 `/dev/console` 从异步 UART
切换为适配当前代码的 polling Console，然后在 QEMU 和 Lichee D1 上运行
同一组用户态测试，形成同步 Console 与异步 UART 的对照数据。

本分析回答六个问题：

1. 为什么应使用独立分支？
2. 分支应从哪个提交创建？
3. “改回 Console”需要保留和替换哪些代码？
4. 哪些 benchmark 项可以比较？
5. QEMU 和 D1 各自能证明什么？
6. 实施和采集需要哪些 Gate？

本文只分析方案，不创建分支，不修改运行代码，也不生成测试结论。

## 结论

独立分支适合这次基线实验，但不能从旧 Console 提交继续开发，也不能把
旧实现整体回退到当前树。

建议从已完成 Q26 收尾的当前提交创建分支：

```text
async baseline: 1ce95d7128e9c5583fc28628c72fb7c5c5e62db4
                         |
                         +-- console-lichee
```

`console-lichee` 应保留当前 D1 启动、VFS、TTY、用户程序加载和 benchmark
基础设施，只替换 `/dev/console` 的用户态 UART 后端及其生命周期。

历史提交 `b3e9b280172f4822c47c910395ff3c7ddc9f20f1` 可用于理解旧
`Console` TTY，但不能作为新分支基线。该提交早于当前 D1 平台、TTY
短写契约、loader、benchmark 和维护修复。

## 分支用途

独立分支有三个好处：

| 目的 | 收益 |
|------|------|
| 冻结测量对象 | Console 改造不会改变异步分支的运行代码 |
| 保留实验差异 | 每个结果可追溯到一个明确提交 |
| 限制维护影响 | polling 用户态路径只作为性能基线，不改变当前正式架构 |

分支不是长期的第二套产品架构。完成数据采集后，结果可以回到主线文档，
Console 实验代码是否保留应另行决定。

比较时必须使用同一个异步基线。若异步分支继续演进，应先把需要的公共
修复同步到两边，再重新生成两组结果。不能把新异步提交与旧 Console
结果放进同一结论。

## 当前与目标路径

当前用户态输出路径：

```text
benchmark
  -> /dev/console
  -> ASYNC_TTY
  -> TX ring
  -> copier task
  -> UART FIFO
  -> wire
```

目标分支输出路径：

```text
benchmark
  -> /dev/console
  -> adapted Console TTY
  -> axhal::console or board Console backend
  -> polling UART FIFO
  -> wire
```

内核启动、日志和 panic 输出本来就使用 polling Console。目标分支改变的
是用户态 `/dev/console`，不是早期日志路径。

## 适配边界

旧 `Console` 不能原样恢复。当前 `TtyWrite::write` 返回实际写入字节数，
TTY 还要求 `TtyWriteReady`；旧 `ProcessMode::Manual` 已被移除。

建议按下表处理：

| 类别 | 分支内处理 |
|------|------------|
| D1 axplat、启动和页表 | 保留当前实现 |
| D1 stride 4、32-bit MMIO | 保留当前实现 |
| Android boot image 和 command-entry | 保留当前实现 |
| memory-root、eager ELF loader、stdio | 保留当前实现 |
| 当前 TTY、termios、FIONBIO 契约 | 保留并适配 Console 后端 |
| benchmark 程序和构建参数 | 保持同一版本 |
| `/dev/console` 设备绑定 | 从 `ASYNC_TTY` 改为适配后的 Console TTY |
| QEMU/D1 async UART 初始化 | 在 Console 测试模式中移除或跳过 |
| copier task、ring、async ISR | 在 Console 测试模式中移除或跳过 |
| startup ring benchmark | 输出 `SKIPPED backend=console` |
| TX debug ioctl 和 S40 telemetry | 返回不支持，benchmark 输出 `UNSUPPORTED` |
| `tcdrain` | 改为 polling TEMT，不能沿用 async completion |

Console 写入是同步写。正常情况下应返回完整长度。若底层接口不能报告
错误，TTY 层也不能伪造异步短写或可写等待。

## tcdrain 要求

当前 `TCSBRK` 实现无条件读取 async driver 的 `tx_completion()`，因此
仅替换 `/dev/console` 绑定会导致错误依赖，甚至在未初始化 async driver
时失败。

Console 分支必须提供独立的 drain 实现：

```text
wait until LSR.TEMT == 1
```

必须检查 TEMT，也就是发送保持寄存器和移位寄存器都为空。只检查 THRE
会让 `tcdrain()` 在最后一个字节仍在线上发送时提前返回，吞吐和延迟数据
会失真。

QEMU 16550 不模拟真实串口线速。即使 TEMT 语义正确，QEMU 结果仍只适合
比较软件路径和虚拟设备开销。

## D1 Console 限制

当前 D1 polling Console 使用 UART0 的 stride 4、32-bit MMIO，并在每个
字节前轮询 LSR bit 5。它的 `read_bytes()` 固定返回 0。

因此：

| 能力 | D1 Console 基线状态 |
|------|---------------------|
| TX throughput / latency | 可测，需补 TEMT drain |
| termios 基础 ioctl | 可做兼容 smoke |
| nonblocking RX | 不支持 |
| S30 RX 结果 | 必须标记 `UNSUPPORTED` |
| async TX telemetry | 不支持 |

不能把 `read()` 返回 0 或 `EAGAIN` 解释成 D1 Console RX 通过。若本次目标
只比较 TX 性能，不需要为了 S30 新增 polling RX。

## Benchmark 可比性

应复用当前 `tests/benchmark.c`，但在 manifest 中增加后端标识：

```text
backend=async-uart
backend=polling-console
```

各测试项的解释如下：

| Section | Console 分支处理 | 可比结论 |
|---------|------------------|----------|
| S10 | 保留 write + tcdrain | drain-each 吞吐和延迟 |
| S11 | 保留，但改名或注明 blocking transmit | write 返回时机，不再是 enqueue 性能 |
| S12/S13/S14 | 保留 | 批量策略和小包表现 |
| S20/S21 | 保留 | 单字节和 FIFO 边界延迟 |
| S30 | QEMU 可测；D1 标记不支持 | 不能给出统一双平台 RX 结论 |
| S40 | 标记不支持 | 不能与 async telemetry 对比 |

S11 在两个后端测量的语义不同：

```text
async write:    copy into ring, later drain
console write:  poll and transmit before write returns
```

可以同时展示两者，但报告必须分别列出 `write elapsed` 和 `final drain`，
不能把 Console 的阻塞发送时间称为 enqueue 时间。

## 测量矩阵

最终对照至少包含四个单元：

| 平台 | 后端 | 证据 |
|------|------|------|
| QEMU | async UART | `docs/qemu_out.md` 对应冻结提交 |
| QEMU | polling Console | 新采集日志 |
| D1 | async UART | `docs/d1_out.md` 对应冻结提交 |
| D1 | polling Console | 新采集日志 |

只在同一平台内比较后端。QEMU 和 D1 之间不能比较绝对吞吐，因为 QEMU
不模拟 115200 波特率的物理发送时间。

建议 Console 原始日志使用：

```text
docs/qemu_console_out.md
docs/d1_console_out.md
```

每份日志应记录：

| 字段 | 要求 |
|------|------|
| async base commit | 固定为本轮异步基线 |
| console commit | 实际构建提交 |
| image | 完整镜像文件名和校验值 |
| build | 编译器、feature、LTO 和构建命令 |
| run | QEMU 命令或 D1 烧录、启动命令 |
| board | 固件版本、波特率、串口工具 |
| benchmark | 版本、迭代数、buffer、drain policy、timer |
| result | 完整原始输出，不只保留汇总表 |

## CPU 数据

polling Console 的主要预期代价是 CPU 忙等，因此只看吞吐和墙钟时间不够。
建议追加进程 CPU 时间与墙钟时间：

```text
cpu_ratio = process_cpu_time / wall_time
```

StarryOS 已提供 process/thread CPU clock 和 `getrusage`，但任务计时在抢占
场景仍有已知限制。正式采集前先做两个校准：

1. sleep 测试的 CPU 时间应接近零。
2. busy-loop 测试的 CPU 时间应接近墙钟时间。

校准失败时，CPU 指标只能标记为实验数据，不能用于后端优劣结论。

## 预期假设

以下只是待验证假设：

| 假设 | 需要的数据 |
|------|------------|
| D1 两个后端 drain-each 吞吐都接近线速 | S10/S14 + TEMT drain |
| polling Console 的 CPU 占用更高 | 校准后的 CPU/wall ratio |
| polling 单字节延迟可能更低 | S20/S21 分布 |
| async S11 write 更早返回 | write elapsed 与 final drain 分列 |
| QEMU 差异主要来自软件和设备模型 | QEMU 同平台相对数据 |

没有采集数据前，不应把这些假设写成性能结论。

## 实施 Gate

| Gate | PASS 条件 |
|------|-----------|
| G0 分支冻结 | 从记录的 async base 创建 `console-lichee`，工作树干净 |
| G1 host build | QEMU 和 D1 Console 模式均通过格式化、检查和链接 |
| G2 路径 smoke | `/dev/console` 使用 Console TTY，async copier 未启动 |
| G3 drain | QEMU 与 D1 都证明 `tcdrain` 等待 TEMT |
| G4 QEMU | 有完整 manifest、启动命令和 Console raw log |
| G5 D1 | 有镜像、校验值、烧录命令、固件信息和 Console raw log |
| G6 compare | 只比较同平台、同 benchmark 版本和同参数结果 |

G3 不能只靠 benchmark 数值判断。应增加一个最小的 Console drain 测试，
验证发送最后一个字节后，THRE 已置位但 TEMT 未置位时仍继续等待。

## 风险

| 风险 | 处理 |
|------|------|
| 从旧提交恢复导致平台能力丢失 | 只在当前基线上选择性适配 |
| 两个分支继续演进造成漂移 | 冻结提交并记录两个 commit |
| Console 仍调用 async `tcdrain` | 分离 ioctl 后端并做未初始化 smoke |
| THRE 被误当成 TEMT | 用寄存器状态测试作为 Gate |
| D1 RX 假通过 | 明确输出 `UNSUPPORTED` |
| QEMU 高吞吐被当成线速 | 只作同平台软件路径比较 |
| S11 名称掩盖语义变化 | 分列 write elapsed 和 final drain |
| benchmark 版本不同 | 两分支共用相同源码和宏 |
| 日志输出干扰小包测试 | 每节前后 drain，并保留完整顺序 |

## 影响文件

预计后续计划至少检查这些位置：

| 文件 | 原因 |
|------|------|
| `kernel/src/pseudofs/dev/mod.rs` | `/dev/console` 当前绑定 `ASYNC_TTY` |
| `kernel/src/entry.rs` | async init、startup benchmark、copier 和 TTY bind |
| `kernel/src/syscall/fs/ctl.rs` | TX debug ioctl 和 async `tcdrain` |
| `kernel/src/pseudofs/dev/tty/` | 适配当前 TTY trait 和 termios |
| `crates/axplat-riscv64-lichee-d1/src/console.rs` | D1 polling TX 和 RX 限制 |
| `kernel/src/drivers/uart_init.rs` | Console 模式不得依赖 async driver 生命周期 |
| `tests/benchmark.c` | backend manifest、S11 标签和 unsupported 输出 |
| `Makefile` | QEMU/D1 Console 构建入口和参数一致性 |

## 后续计划输入

后续 `openspec-plan` 应把工作拆成四组：

1. 建立 `console-lichee` 分支和可选的 Console 构建模式。
2. 适配当前 TTY，并替换 `/dev/console` 绑定和生命周期。
3. 实现 polling TEMT drain，收敛不支持的 telemetry/RX 行为。
4. 用同一 benchmark 依次采集 QEMU、D1 日志，再生成对照报告。

建议先完成 G0-G3，再决定是否调整 benchmark 输出。这样可以先证明
Console 路径语义正确，避免性能数据掩盖功能错误。

## 持久化候选

- R 候选：登记本文，作为 Console 性能基线分支的探索入口。
- L 候选：Console 基线必须从当前异步提交选择性适配，不能整体回退旧实现。
- A 候选：`console-lichee` 仅作为测量分支，正式架构仍保持 Console 日志与
  async 用户态路径共存。

L/A 候选应在后续计划获批后再提升，避免把探索建议提前写成项目决定。
