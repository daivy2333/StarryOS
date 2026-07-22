## ADDED Requirements

### Requirement: D1 time conversion is frequency-accurate

D1 tick/ns 双向换算 MUST 使用 24 MHz 频率进行宽整数乘除，MUST NOT 先把单 tick 纳秒数截断为整数。

#### Scenario: One second converts exactly

- **WHEN** `ticks_to_nanos` 接收 24,000,000 ticks
- **THEN** 结果 MUST 为 1,000,000,000 ns
- **AND** `nanos_to_ticks(1,000,000,000)` MUST 为 24,000,000 ticks

#### Scenario: Conversion remains monotonic at boundaries

- **WHEN** 输入覆盖 0、1、frequency-1、frequency 和接近 `u64::MAX` 的值
- **THEN** 换算结果 MUST 单调不减
- **AND** 中间乘法 MUST NOT 发生 `u64` 溢出
- **AND** 超过返回类型范围的结果 MUST 饱和到 `u64::MAX`

### Requirement: Submission and completion remain separate

benchmark MUST 分开报告 UART 提交时间和最终 TEMT 完成时间，并输出不混淆完成点的派生指标。

#### Scenario: S11 reports caller release

- **WHEN** S11 完成固定 payload 的 write loop 和 final `tcdrain`
- **THEN** 输出 MUST 包含 bytes、write calls、short writes、enqueue time 和 final drain time
- **AND** 输出 MUST 包含 `submit_fraction` 与 `producer_available`
- **AND** `submit_fraction` MUST 以 enqueue / (enqueue + final drain) 计算

#### Scenario: Completion semantics differ

- **WHEN** Async 与 Console 的 enqueue/write 阶段包含不同完成语义
- **THEN** benchmark MUST 保留分段原始时间
- **AND** 报告 MUST NOT 从不同完成点计算吞吐倍率

### Requirement: CPU work uses instret delta

benchmark MUST 使用 `/proc/instret` 的区间差值报告相同完成字节数的 CPU work，MUST NOT 将该值标注为 CPU utilization。

#### Scenario: Valid instret interval completes

- **WHEN** begin/end 计数读取成功、end 不小于 begin、完成字节数大于零且 `tcdrain` 成功
- **THEN** 输出 MUST 包含 begin、end、delta、completed bytes、logical writes 和实际 write syscall calls
- **AND** 输出 MUST 包含 instructions/byte 与 instructions/write-call
- **AND** 测量区间 MUST 覆盖 write 开始到 final TEMT drain 完成

#### Scenario: instret is unavailable

- **WHEN** proc 文件缺失、读取失败、解析失败、计数倒退或分母为零
- **THEN** 该 CPU-work 样本 MUST 输出 `status=not-available` 和原因
- **AND** benchmark MUST NOT 用零值代替缺失计数
- **AND** 其他墙钟和正确性场景 MUST 可继续执行

### Requirement: UART transmission overlaps useful computation

benchmark MUST 使用固定计算内核和固定 UART workload，比较相同理论串口窗口内的 idle 与 UART useful work。

#### Scenario: Overlap window is measured

- **WHEN** benchmark 完成预热、idle 基线，并在 UART 写入后运行计算直到绝对 deadline
- **THEN** 输出 MUST 包含 write return time、idle iterations、UART iterations、useful work/ms、final drain time 和 `overlap_efficiency`
- **AND** `overlap_efficiency` MUST 以 UART iterations / idle iterations 计算
- **AND** 计算循环结果 MUST 被消费，防止编译器删除 workload

#### Scenario: Write consumes the comparison window

- **WHEN** write 返回时已经到达或超过理论 UART deadline
- **THEN** benchmark MUST 输出零个 UART-window iterations 和实际 write return time
- **AND** 结果 MUST 解释为调用者未获得重叠窗口，而不是测试失败

### Requirement: Timer wakeup overshoot is measured under TX load

benchmark MUST 使用 `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)` 测量 idle 与可建立的 UART TX backlog 下的唤醒超时量。

#### Scenario: Idle and TX-loaded samples are available

- **WHEN** 绝对时间睡眠成功，并且 UART 提交后仍有足够的理论传输窗口
- **THEN** idle 与 loaded 输出 MUST 分别包含样本数、P50、P95、P99 和 max overshoot
- **AND** 采样 deadline MUST 由前一次目标时间递增，MUST NOT 由实际唤醒时间递增

#### Scenario: Loaded sampling cannot be established

- **WHEN** 同步 Console write 已消耗理论传输窗口，或 `clock_nanosleep` 返回错误
- **THEN** loaded 结果 MUST 标记 `not-applicable` 或 `FAIL` 并给出原因
- **AND** benchmark MUST NOT 把 idle 样本复制为 loaded 样本

### Requirement: TX counters are scoped per workload

Async benchmark MUST 在选定 workload 前 reset、完成后 snapshot TX debug counters，并输出按完成字节数归一化的路径计数。

#### Scenario: Per-section counters are available

- **WHEN** TX debug reset/snapshot ioctl 成功且完成字节数大于零
- **THEN** 输出 MUST 包含原始 counter、reset/snapshot 返回值和完成字节数
- **AND** 输出 MUST 包含 hw-send-calls/KiB、zero-send/KiB、ring-pop/KiB、no-progress/KiB、bytes/ring-pop 和 bytes/hw-send

#### Scenario: Debug ioctl is absent on comparator

- **WHEN** Console 或其他兼容路径不支持 TX debug ioctl
- **THEN** counter section MUST 标记 `status=not-available`
- **AND** write、drain、instret、overlap 和 timer 场景 MUST 可继续运行

### Requirement: Measurement output is isolated and reproducible

benchmark MUST 避免用被测 UART 的逐样本输出污染采样区间，并记录复现实验所需的 manifest。

#### Scenario: Samples are emitted after measurement

- **WHEN** 任一计时、instret 或 counter workload 运行
- **THEN** 逐样本数据 MUST 先保存在内存
- **AND** 结果 MUST 在 workload 完成并 drain 后统一输出
- **AND** manifest MUST 包含 commit、branch/mode、benchmark version、设备路径、设备号、hart 数、payload、迭代数和 timer source

#### Scenario: UART completion is invalid

- **WHEN** 出现未补齐 short write、drain error、超时、完成字节数不符或异常退出
- **THEN** 对应样本 MUST 标记 FAIL
- **AND** 失败样本 MUST NOT 进入效率比较结论

### Requirement: Evidence preserves baselines and branch provenance

change MUST 将可覆盖的 `docs/*out.md` 与不可变 evidence 分离，并为 Async 与 Console 保存同口径原始日志和来源信息。

#### Scenario: Existing docs logs are preserved before overwrite

- **WHEN** 新 benchmark 日志写入任何 `docs/*out.md` 之前
- **THEN** 现有四份 Async/Console、QEMU/D1 日志 MUST 复制到 `.claude/analysis/q31-cpu-efficiency-evidence/baseline/`
- **AND** evidence README MUST 记录源路径、复制日期、commit 和 SHA-256
- **AND** 原 `docs/*out.md` MUST 保留

#### Scenario: Final Async and Console comparison is accepted

- **WHEN** 两条分支的新 D1 日志都已采集
- **THEN** evidence MUST 证明 benchmark version、payload、迭代数、设备、timer conversion 和 drain policy 相同
- **AND** 比较 MUST 分栏报告 caller release、useful work、instructions/byte、line rate、latency 和 correctness
- **AND** QEMU 与 D1 结论 MUST 分开

### Requirement: Existing UART behavior remains unchanged

本 change MUST 保持 UART write、short-write/backpressure、TTY、copier、IER、waker、`TxCompletion` 和 `tcdrain` 语义不变。

#### Scenario: Scope review finds a driver behavior change

- **WHEN** 实施需要修改 `tx_copier_loop`、THRE retry、IER、waker、TTY 或 drain 语义才能得到目标指标
- **THEN** 该修改 MUST 从本 change 移出并另建 correctness 或 optimization change
- **AND** 本 change MUST 只保留测量、平台计时修正和 evidence 工作

#### Scenario: Performance result does not improve

- **WHEN** Async 的某项 CPU-work 或响应性指标不优于 Console
- **THEN** 测量 change 仍可在数据有效且行为正确时完成
- **AND** 结论 MUST 按数据描述，不得把调用者早返回改写成系统 CPU 效率改善
