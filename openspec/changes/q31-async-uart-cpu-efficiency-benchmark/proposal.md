## Why

现有 UART benchmark 能证明 Async 路径达到 D1 线速，并能观察提交、完成和路径计数。它不能证明调用者释放的时间可用于计算，也不能比较相同通信量的 CPU work。

本 change 补齐可复核的 CPU 效率证据。实施先在 `uart-lichee` 完成；用户随后在 `console-lichee` 复用同版测试，形成 D1 同口径对照。

## Approved Scope

用户于 2026-07-21 批准推荐方案：

- 完整证据组：提交占比、`instret/byte`、通信—计算重叠、定时唤醒超时、分段 S40。
- 先修复 D1 tick→ns 的整数换算误差，再采集绝对时间指标。
- 最终结论使用 D1 Async 与 Console 同版 benchmark 对照。
- 先在 Async 分支更新测试；Console 分支更新由用户在 Async 工作完成后执行。
- 将 `docs/d1_out.md`、`docs/qemu_out.md`、`docs/d1_console.md`、`docs/qemu_console.md` 的既有日志复制并归纳到 change evidence 目录。源文件不删除，后续可被新日志覆盖。

### Scope Split (2026-07-22)

用户批准不再向 Q31 增加 Console iteration。Q31 以已验收的 Async 实现、QEMU/D1 日志和
测量合同收口；`q32-console-cpu-efficiency-benchmark` 独立负责 Console 代码、Console
QEMU/D1 证据和最终 comparison。Q32 以 hash 引用 Q31，不改写 Q31 evidence。

## What Changes

- 修正 D1 `ticks_to_nanos` 与 `nanos_to_ticks`，避免 24 MHz 时钟先截断为 41 ns/tick。
- 为 S11 输出 `submit_fraction`、`producer_available` 和 `released_window_ms` 所需的原始字段与派生字段。
- 为 UART workload 增加 `/proc/instret` 区间采样，输出 delta、instructions/byte 和 instructions/write。
- 增加通信—计算重叠场景，以 idle 基线计算 `overlap_efficiency`。
- 增加绝对时间睡眠 overshoot 场景，输出 idle 与 UART TX 下的 P50/P95/P99/max。
- 对选定 workload 分段 reset/snapshot S40，输出归一化路径计数。
- 建立 `.claude/analysis/q31-cpu-efficiency-evidence/`，保存旧日志快照、Async 新日志和环境信息；Console 新日志与比较结果由 Q32 独立保存。
- 保持 QEMU 与 D1 结论分离。QEMU 只作为软件路径和回归证据，D1 作为物理 UART 与最终对照证据。

## Scenario Sketch

**Happy Path — Async 完整测量**

- 前置：D1 使用修正后的单调时钟，`/dev/console`、`/proc/instret` 和 TX debug ioctl 可用。
- 动作：运行同一版本 benchmark，完成预热、idle 基线、UART workload、最终 drain 和分段快照。
- 结果：输出完整原始值与派生值，`completed_bytes` 与预期一致，`drain_errors=0`，进程正常退出。
- 失败边界：任一计数源或完成条件失败时，该指标标记失败或不可用，不以零值代替。

**Happy Path — Async 与 Console 对照**

- 前置：两条分支使用同一 benchmark 源码、payload、迭代数、串口配置、预热和 drain policy。
- 动作：分别采集 D1 日志，并保存构建信息和日志 hash。
- 结果：按提交释放、有效计算、CPU work、吞吐、延迟和正确性分栏比较。
- 失败边界：完成点或 workload 不同则拒绝计算倍率，并标记为不可比较。

**Sad Path — instret 不可用**

- 前置：`/proc/instret` 缺失、读取失败、解析失败或 end 小于 begin。
- 动作：执行 CPU-work 测量。
- 结果：输出 `status=not-available` 和原因；其他墙钟与正确性场景继续运行。
- 失败边界：不得输出 `instructions_per_byte=0` 或 CPU 百分比。

**Sad Path — UART 未完整完成**

- 前置：出现 short write 未补齐、`tcdrain` 失败、超时或完成字节数不符。
- 动作：汇总 workload。
- 结果：该场景失败，并保留 errno、短写数、已完成字节和 drain 状态。
- 失败边界：失败样本不得进入效率结论。

**Edge — 背压与零分母**

- 前置：payload 超过 TX ring 可用容量，或测试字节数、耗时、idle iterations 为零。
- 动作：计算提交占比和归一化指标。
- 结果：背压仍按实际 enqueue/drain 分段报告；零分母字段标记不可用。
- 失败边界：不得发生除零、溢出或将背压写成吞吐退化。

**Edge — 计时与采样污染**

- 前置：被测 UART 同时承担 benchmark 输出，或后台任务污染 hart-wide `instret`。
- 动作：开始采样。
- 结果：采样区间内结果缓存在内存，drain 后统一打印；记录单 hart、环境和重复轮次。
- 失败边界：无法隔离时只保留诊断数据，不宣称系统 CPU work 改善。

**Timeout、取消与兼容性**

- 定时唤醒使用 `CLOCK_MONOTONIC | TIMER_ABSTIME`，系统调用错误必须终止该场景。
- 测试进程被中断时不得生成 PASS 汇总；已有原始输出保留用于诊断。
- QEMU、D1 Async 和 D1 Console 使用相同字段名；不支持的 debug ioctl 输出 `not-available`。
- 保持现有 S00-S40 字段兼容，只新增 section 或字段，不修改 write、TTY、copier、IER、waker 和 drain 语义。

## Non-goals

- 不报告 process、thread 或 system CPU utilization。
- 不重做任务 CPU accounting。
- 不启用 RX fixed payload S31。
- 不证明 SMP memory ordering。
- 不优化 `tx_copier_loop()`、THRE retry、IER、waker 或 drain。
- 不扩展 `TxDebugSnapshot` ABI；先复用现有 reset/snapshot 字段。
- 不删除 `docs/*out.md`，也不把这些临时文件当成不可变证据。

## Capabilities

**New Capabilities**

- `uart-cpu-efficiency-benchmark`：定义 UART 提交释放、CPU work、通信—计算重叠、唤醒响应、分段路径计数和跨分支证据的测量合同。

**Modified Capabilities**

None。主规格 M15/M30/M31、I01/I12 的现有边界保持不变。

## Impact

- [tests/benchmark.c](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/tests/benchmark.c)：新增测量场景、采样帮助函数和输出字段。
- [D1 time.rs](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/axplat-riscv64-lichee-d1/src/time.rs)：修正 tick/ns 双向换算并增加测试见证。
- [clock_nanosleep](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/task/schedule.rs)：复用已有绝对时间睡眠，不修改接口。
- [TX debug ioctl](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/kernel/src/syscall/fs/ctl.rs)：复用已有 reset/snapshot，不修改 ABI。
- `.claude/analysis/q31-cpu-efficiency-evidence/`：新增可追溯日志与比较入口。
- `docs/d1_out.md`、`docs/qemu_out.md`、`docs/d1_console.md`、`docs/qemu_console.md`：保留为可覆盖的临时日志，不在本 change 删除。

本 change 涉及平台时间和性能测量，不使用轻量模式。
