# Console 性能与测量设计

> Project: StarryOS
> Branch: `console-lichee`
> Commit: `c36544b7281f53b562466809a1a5d9b22479bb7b`
> Date: 2026-07-21
> See also: [Console 基线分析](console-lichee-baseline-branch.md), [Q20 benchmark 缺口](q20-benchmark-gap-closure.md), [当前性能报告](../../docs/benchmark-report-async.md)

## 目标与范围

本文为 I11、I12 提供计划输入。范围包含 Console TX/RX、TTY、
`tcdrain`、CPU、内存、延迟、抖动、完整性和并发影响。

本文回答六个问题：

1. 当前 Console 的时间花在哪里？
2. 现有 CPU 和内存接口是否可信？
3. benchmark 还缺哪些测量边界？
4. 哪些 Console 优化可先做？
5. QEMU 与 D1 各能证明什么？
6. 后续 change 应怎样分阶段？

本文不修改产品代码，也不生成新性能结论。

## 结论

I11 不能从缩短一次循环开始。应先修复测量基础，再采集当前值。
现有结果只证明同步完成速度和功能稳定性。

当前有五个高优先级问题：

| 优先级 | 问题 | 影响 |
|---|---|---|
| P0 | `TCSBRK` 绕过目标 fd | 非 Console fd 会 drain 全局 Console |
| P0 | CPU task time 未跟随调度切换 | CPU 占用率可能包含离 CPU 时间 |
| P0 | TX 全程持有 `SpinNoIrq` | IRQ-off 与物理发送时间成正比 |
| P1 | blocking RX 持续 self-wake | 有等待者时产生调度轮询 |
| P1 | benchmark 无设备和接收端见证 | 不能证明数据到达和内容完整 |

在 115200 bps 下，当前设计存在三项约束：

- 同步轮询到物理完成。
- 整次 `write` 不与日志交错。
- IRQ-off 时间保持短且有界。

当前锁模型不能同时满足三项。保留同步发送和整次写原子性，
IRQ-off 就随 payload 增长。若要求短 IRQ-off，需接受分块交错，
或让日志与 TTY 进入同一缓冲发送器。

建议先完成测量 change，再选择 Console 优化路线。首轮不应同时改锁、
RX 和 benchmark，否则无法区分测量变化与实现变化。

## 当前调用链

用户 TX 路径如下：

```text
write/writev
  -> sys_write/sys_writev
  -> FileLike::write
  -> axfs File::write_at
  -> Tty::write_at
  -> ONLCR 分块（最多 256B 输出）
  -> ConsoleWriter::write
  -> with_console_port_tx
  -> axplat::console::CONSOLE_LOCK (SpinNoIrq)
  -> CONSOLE_PORT (SpinNoPreempt)
  -> PollingPort::putchar
  -> THRE 轮询 -> THR MMIO
```

证据位于 [TTY 写路径](../../kernel/src/pseudofs/dev/tty/mod.rs#L145)、
[ONLCR 分块](../../kernel/src/pseudofs/dev/tty/write.rs#L1)、
[Console writer](../../kernel/src/pseudofs/dev/tty/console.rs#L40) 和
[polling port](../../kernel/src/platform/polling.rs#L293)。

`CONSOLE_LOCK` 来自 `axplat 0.3.1-pre.6`。其类型为 `SpinNoIrq`。
锁会关闭本 hart 中断。`CONSOLE_PORT` 只关闭抢占，外层锁决定
TX 的 IRQ-off 边界。

内核日志路径如下：

```text
ax_println!/log
  -> axplat::__simple_print
  -> 同一个 CONSOLE_LOCK
  -> platform ConsoleIf::write_bytes
  -> 平台 UART 锁和 THRE 轮询
```

同一个全局锁避免日志与用户 TX 逐字节交错。代价是等待锁、
发送和 drain 都可能处于本地关中断状态。

默认 ONLCR 路径最多发送 256B 后释放锁。其理论线时约 22.2ms。
关闭 OPOST/ONLCR 后，1024B 可一次进入 writer，理论线时约 88.9ms。
这些数值是上界风险，不是已测 IRQ-off 数据。

`tcdrain` 路径如下：

```text
tcdrain(fd)
  -> ioctl(fd, TCSBRK)
  -> sys_ioctl
  -> 全局 with_console_port_tx
  -> 持锁轮询 TEMT
```

[sys_ioctl](../../kernel/src/syscall/fs/ctl.rs#L28) 先确认 fd 存在，
随后对所有 fd 拦截 `TCSBRK`。它没有确认目标是 Console。
因此 `/dev/null` 等 fd 也会 drain 全局 Console 后返回成功。

RX 路径如下：

```text
read/poll
  -> Tty::read_at / Pollable
  -> LineDiscipline::poll_read
  -> ConsoleReader::read
  -> try_getchar
  -> LSR Data Ready / RBR
```

Polling 模式无外部事件源。阻塞读取注册 waker 时，
[register_rx_waker](../../kernel/src/pseudofs/dev/tty/terminal/ldisc.rs#L384)
调用 `wake_by_ref()`。只有等待者存在时才循环，但等待期间会持续
yield 和重查。D1 benchmark 又用宏把 RX 标成不支持，
没有覆盖 D1 polling RX 能力。

## 现有测量的边界

当前 [benchmark](../../tests/benchmark.c#L1) 使用 `/dev/console`、
`CLOCK_MONOTONIC` 和每组 100 次样本。S10、S14、S20、S21 已输出
P50、P95、P99、max 和尾部比值。

已有测量仍有以下缺口：

| 类别 | 当前状态 | 缺口 |
|---|---|---|
| 设备 | manifest 只打印路径 | 缺 `fstat` 类型、major/minor |
| 时间 | 多数样本是 `write + drain` | 缺 write、drain、总完成三段样本 |
| CPU | S40 是调用计数代理 | 缺 task runtime 与全系统 idle |
| IRQ | 无数据 | 缺锁内 IRQ-off 和 timer 延迟 |
| 内存 | 无数据 | 缺静态、heap delta、peak、RSS 分层 |
| 完整性 | 检查返回值和 drain 错误 | 缺接收长度与 payload hash |
| RX | 空读可测，fixed payload 默认关闭 | 缺内容、超时和 D1 能力见证 |
| 复现 | raw log 有部分环境信息 | 缺统一 commit、工具链、hart、参数字段 |

S11 在后端之间不是同一完成点。async 测 ring 提交，Console 测同步发送。
两列可以并排展示，但不能计算倍率。

当前样本把成功与延迟混在一起。`run_write_drain_iters()` 即使 drain
失败，也会把该次耗时写入分布。后续应分别计数 write 失败、短写、
drain 失败和有效样本。

当前 P99 使用 100 个样本。它只能代表排序后的单个尾部点。
正式结论应包含预热、多轮独立运行和百分位算法标识。
不建议仅靠一次 P99 判断回归。

## CPU 测量设计

CPU 指标应分三层，不能互相替代：

| 层 | 指标 | 用途 |
|---|---|---|
| 任务 | active runtime / wall time | benchmark 线程占用 |
| 系统 | `(hart×wall-idle)/hart×wall` | 整机 busy 比例 |
| 路径 | cycles/byte、cycles/call | 实现效率 |

该缺口属于 StarryOS 的 CPU accounting，不是硬件 timer 精度问题。
[`CLOCK_MONOTONIC`](../../kernel/src/syscall/time.rs#L20) 使用平台单调时钟，
仍可测量墙钟延迟、吞吐和抖动。

现有 `CLOCK_PROCESS_CPUTIME_ID` 与 `getrusage` 都读取 `TimeManager`。
[TimeManager](../../kernel/src/task/timer.rs#L133) 标注抢占不会改变状态。
计时只在 user/kernel 边界和 timer poll 更新。
调度器切走任务时没有结算 active runtime。
任务离开 CPU 的区间仍可能计入该任务。

因此三类时间应分开判断：

| 时间 | 当前可信度 | 原因 |
|---|---|---|
| 单调墙钟 | 可用于延迟和抖动 | 由平台单调时钟提供 |
| task CPU time | 未校准前不可作为 CPU% | 缺 context-switch 结算 |
| system CPU time | 当前不可用 | 缺 per-hart idle/WFI 累计 |

因此首个 CPU Gate 是计时校准：

| 校准 workload | 期望 |
|---|---|
| 固定 wall sleep | task CPU 接近零 |
| 固定 wall busy-loop | task CPU 接近 wall |
| busy 与 sleep 并发 | 两个任务的 runtime 可区分 |
| Console write | task CPU 反映同步轮询 |

校准失败时不得报告 CPU 百分比。修复方向是在调度切换时结算
active runtime，或增加独立的 scheduler runtime 计数。

全系统 CPU 需要 per-hart idle 时间。当前 idle task 会执行 WFI，
但没有 idle 累计值。应在进入和退出 idle/WFI 时用单调时钟计时。
单 hart D1 可计算 busy 比例，多 hart 需先按 hart 报告再汇总。

路径 cycles 应由内核在测量边界读取。用户态能否读取 `cycle/instret`
没有当前项目见证。不能假定 `rdcycle` 一定可用。
cycles 只报告差值、每字节或每调用，不标成 CPU 百分比。

QEMU host CPU 与 guest CPU 必须分列。host 可用外部进程计时，
但它不能替代 guest task runtime。`ICOUNT=y` 可用于确定性回归，
不能与普通 QEMU 或 D1 的墙钟性能混表。

## IRQ 与并发延迟设计

应同时测局部窗口和系统后果：

| 指标 | 测量点 |
|---|---|
| Console lock wait | 请求锁前到取得锁后 |
| Console lock hold | 取得全局锁到释放后 |
| 最大 IRQ-off | 全局 `SpinNoIrq` guard 生命周期 |
| drain hold | TEMT drain 的独立窗口 |
| timer overshoot | 目标唤醒时刻到任务实际运行 |
| 并发任务延迟 | probe 的 P50/P99/max |

锁指标应记录 calls、bytes、total 和 max。只记录平均值会掩盖
1024B raw write。write 与 drain 必须分开计数。

timer probe 应在另一个任务中周期休眠。每次记录实际唤醒减目标时刻。
该指标包含 timer IRQ、IRQ-off、调度和运行队列延迟。
它不是纯硬件 IRQ latency，报告时应命名为 wakeup overshoot。

测试至少包含四种负载：

| 场景 | Console 任务 | 并发任务 |
|---|---|---|
| idle | 无 | timer probe |
| 1B drain-each | S20 | timer probe |
| 256B/1024B | S10/S11 | timer probe |
| log contention | 用户 TX | 固定频率 kernel log |

log contention 场景还需检查输出边界。当前实现承诺同一次
`ConsoleWriter::write` 不被日志穿插。若改成分块锁，
必须明确原子单位是 syscall、TTY chunk 还是物理 burst。

## 内存测量设计

内存数据应分四类：

| 类别 | 口径 |
|---|---|
| 静态 | ELF text/data/bss、静态对象、ring capacity |
| kernel heap | used bytes/pages、UsageKind delta |
| 用户进程 | 映射大小、RSS 或已驻留页 |
| 峰值 | 测量区间内最大 heap/RSS |

`axalloc::global_allocator()` 已提供 `used_bytes()`、`used_pages()` 和
`usages()`。当前 `/proc/meminfo2` 只打印 `usages()` 的 Debug 格式。
`sysinfo` 的内存字段全为零。

`/proc/[pid]/stat` 的 `vsize/rss` 也保持默认零。
因此当前标准用户接口不能给出有效 RSS。

现有 memtrack 会记录分配 backtrace。它适合泄漏分析，
但 feature、DWARF 和记录开销会改变性能。它应单独运行，
不得与正式时延样本同时启用。

首轮内存 Gate 可采用以下分层：

1. 构建产物记录 text/data/bss 与镜像大小。
2. 预热后读取 allocator baseline。
3. 每个 section 前后读取 used bytes/pages/usages。
4. 单独运行采样任务估算区间 peak。
5. RSS 未实现时标成 `UNSUPPORTED`，不填零。

Console 没有 TX/RX ring，不等于内存占用为零。
TTY、line discipline、256B read ring、ONLCR chunk、对象和栈仍占空间。

## benchmark 改进模型

建议给每个 TX 样本记录三个时间点：

```text
t0 -> write -> t1 -> tcdrain -> t2
```

派生字段如下：

| 字段 | 公式 | 含义 |
|---|---|---|
| write_ns | `t1-t0` | 调用接受或同步发送时间 |
| drain_ns | `t2-t1` | 剩余物理完成等待 |
| complete_ns | `t2-t0` | 用户观察到的完成时间 |

async 的 `write_ns` 是提交延迟。Console 的 `write_ns` 包含轮询发送。
两者名称相同，但语义字段必须标明 `completion_point`。

每组输出应包含：

- warmup、samples、runs、payload、drain policy。
- valid、write_errors、short_writes、drain_errors。
- min、P50、P90、P95、P99、max。
- P99-P50、P99/P50、max/P50。
- task CPU、system idle、cycles/byte。
- heap baseline、delta、peak。
- IRQ-off max、wakeup overshoot P99/max。

设备见证应在 open 后执行 `fstat`。输出路径、文件类型、
`st_rdev` major/minor。`/dev/console` 当前注册为 5:1。

完整性测试应使用独立 section。定时区间内不打印诊断文本。
发送固定种子的 payload，并由 QEMU chardev 或 D1 主机端保存原始字节。
接收端校验长度与 hash，测试结束后再打印结果。

串口同时承载日志。完整性帧需可从日志中识别。
若不能可靠分帧，只能声明 write 返回和 drain 成功，
不能声明线端内容完整。

RX fixed payload 需校验字节内容，不只累计长度。
应记录注入方式、目标 hash、超时、读次数和收到的 hash。

## 测量接口选择

有三种可行接口：

| 方案 | 优点 | 缺点 |
|---|---|---|
| 扩展 `/proc` | 易从用户态读取 | reset/区间语义弱，文本解析脆弱 |
| Console 专用 ioctl | 与目标 fd 绑定 | 混入系统 CPU/内存职责 |
| feature-gated bench metrics 设备 | 可版本化、可 reset/snapshot | 新增测试专用内核接口 |

建议使用 feature-gated bench metrics 设备。接口只在 benchmark 构建启用。
结构体应包含 `version`、`size` 和 capability bits。
Console/async 缺失字段通过 capability 标记，不用零伪装可用。

`TCSBRK` 不属于 metrics 接口。它应下沉到目标 TTY/device 的 ioctl，
让 `sys_ioctl` 只做 fd 分发。Console writer drain TEMT，
非 TTY 返回 `ENOTTY` 或对应错误。

## Console 优化路线

应先采集 current-state，再选择路线。

| 路线 | 保持整次写不交错 | IRQ-off 有界 | 提交与发送解耦 | 复杂度 |
|---|---:|---:|---:|---:|
| A 保持当前同步整写 | 是 | 否 | 否 | 低 |
| B 小 burst 分块锁 | 否，除非放宽为 burst | 是 | 否 | 低 |
| C 统一 TX 队列 | 是 | 是 | 是 | 高 |
| D 日志与 TTY 分域 | 取决于输出端合并 | 是 | 可选 | 中高 |

路线 B 只能在用户接受 chunk-boundary interleave 后采用。
路线 C 需要 early/panic fallback，并统一 kernel log 与 TTY 的发送顺序。
它接近一个有缓冲的 Console TX engine，不应伪装成小锁优化。

无论选择哪条路线，都应先做以下低风险工作：

1. 修复 `TCSBRK` 的 fd 归属。
2. 增加 Console 锁、drain 和 MMIO 计数。
3. 校准 task runtime 与 idle 计时。
4. 建立 timer probe 和完整性 capture。
5. 冻结 current-state QEMU/D1 数据。

RX 可单独演进。推荐优先比较 RX-only IRQ 与有界退避。
RX-only IRQ 需采用 register-recheck，并保留 polling fallback。
有界退避需量化输入延迟、CPU 和唤醒次数。

重复 MMIO 对象当前不是首要开销。axplat early Console 与 kernel polling
port 访问同一 UART，但状态很少。应在锁和 RX 问题有数据后再评估合并。

## 对照矩阵

新指标不能从四份旧日志补算。必须把同一测量 harness 带到各分支，
再重新采集。

| 平台 | 实现 | 可证明内容 |
|---|---|---|
| QEMU | main Console | API、锁形状、功能回归 |
| QEMU | polling Console | 功能、指标格式、相对软件开销 |
| QEMU | async UART | 功能、指标格式、相对软件开销 |
| D1 | polling Console | 线速、CPU、IRQ-off、并发影响 |
| D1 | async UART | 线速、CPU、IRQ、并发影响 |

main 当前没有同版 `tests/benchmark.c`。它必须先移植同一 harness，
不能拿旧 main 数据填表。

每个结果必须记录：

- kernel、benchmark 和 harness commit。
- 编译器、flags、LTO、feature 和镜像 hash。
- 平台、hart 数、内存、timer 频率和 UART 参数。
- QEMU 命令、`ICOUNT`、rootfs 和 chardev。
- D1 固件、boot image、串口工具和原始日志 hash。

QEMU 不证明物理线速、FIFO 时序或 D1 IRQ-off 数值。
D1 单 hart也不证明 SMP 正确性。

## 后续 change 拆分

建议拆成四个 change。每个 change 单独建立 TDD witness。

| 顺序 | Change | 主要 Gate |
|---:|---|---|
| 1 | benchmark measurement foundation | 设备、时间点、统计、复现 manifest |
| 2 | kernel metrics foundation | task/idle、allocator、IRQ-off、timer probe |
| 3 | Console correctness and observability | fd-scoped drain、TX/RX telemetry、完整性 |
| 4 | Console optimization experiment | A/B/C/D 路线对照与回归 |

Change 1 不改变 Console 语义。Change 2 不优化 UART。
Change 3 修复 `TCSBRK`，但不缩小锁。
Change 4 必须由前三个 change 的数据选型。

每个优化实验至少通过以下 Gate：

| Gate | 通过条件 |
|---|---|
| 功能 | write/read/FIONBIO/termios/tcdrain 无回归 |
| drain | THRE=1、TEMT=0 时不提前返回 |
| fd 归属 | 非 Console fd 不 drain Console |
| 完整性 | 接收长度与 hash 一致 |
| CPU | 校准通过，task 与 system 指标齐全 |
| 实时性 | IRQ-off 和 wakeup overshoot 有分布 |
| 内存 | 静态、delta、peak 分开报告 |
| 并发 | log 与用户 TX 满足声明的原子单位 |
| 平台 | QEMU 与 D1 证据分开 |

## 边界与失败路径

- `SpinNoIrq` 会让 timer sampling 看不到锁内细节。必须用区间时间补充。
- 修复 task runtime 前，sleep/busy 校准失败属于 Gate BLOCK。
- memtrack 开启后，性能数据只能用于内存诊断。
- Console 与 async 不同完成点禁止计算倍率。
- QEMU `tcdrain` 不代表物理线时。
- D1 Console RX 当前没有正式数据，不能写 PASS。
- 完整性 capture 无法分帧时，不得声明内容完整。
- 分块锁若允许日志插入，必须记录契约变化并取得用户批准。
- 当前 active change `q17-smp-memory-ordering` 仍缺 multi-hart stress。
- I11/I12 不得借单 hart数据关闭 Q17 风险。

项目模型 M07/M10 和决策 D07/D09 仍把用户态路径定义为 async。
当前 `console-lichee` 又有 Console-only capability spec。
若 Console 从实验分支升级为正式架构，后续 plan 必须处理这组状态差异。

## 关键文件

| 文件 | 用途 |
|---|---|
| [polling.rs](../../kernel/src/platform/polling.rs) | MMIO、TX 锁、THRE/TEMT |
| [console.rs](../../kernel/src/pseudofs/dev/tty/console.rs) | Console reader/writer |
| [TTY mod](../../kernel/src/pseudofs/dev/tty/mod.rs) | write、ONLCR、poll/ioctl |
| [ldisc.rs](../../kernel/src/pseudofs/dev/tty/terminal/ldisc.rs) | polling RX 与 self-wake |
| [ctl.rs](../../kernel/src/syscall/fs/ctl.rs) | 当前全局 TCSBRK 拦截 |
| [timer.rs](../../kernel/src/task/timer.rs) | task user/system time |
| [resources.rs](../../kernel/src/syscall/resources.rs) | getrusage |
| [sys.rs](../../kernel/src/syscall/sys.rs) | 当前空内存 sysinfo |
| [proc.rs](../../kernel/src/pseudofs/proc.rs) | allocator usages、timer tick |
| [benchmark.c](../../tests/benchmark.c) | S00-S40 workload |
| [Makefile](../../Makefile) | QEMU/D1 payload 构建 |
| [polling Console spec](../../openspec/specs/polling-console-baseline/spec.md) | 当前 Console 合同 |
| [I11/I12](../../openspec/specs/improvements/spec.md#L166) | 改进范围与证据要求 |

## 未自动登记的候选

- M 候选：性能结论必须区分 task、system 和 path CPU 口径。
- D 候选：Console 的整写原子性、IRQ-off 与缓冲路线选择。
- K 候选：当前 task CPU time 不具备调度切换精度。
- I 候选：`TCSBRK` 对非 Console fd 错误 drain 全局 Console。

这些候选需要用户另行授权。本文只登记分析引用。
