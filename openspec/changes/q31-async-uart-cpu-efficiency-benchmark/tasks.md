## 1. Baseline and RED Witness

- [x] 1.1 在覆盖任何 `docs/*out.md` 前，将 `docs/d1_out.md`、`docs/qemu_out.md`、`docs/d1_console.md`、`docs/qemu_console.md` 复制到 `.claude/analysis/q31-cpu-efficiency-evidence/baseline/`；在 README 记录源路径、branch、commit、复制日期和 `sha256sum`，并验证源文件仍存在。[R8]
- [x] 1.2 为 [D1 time conversion](https://github.com/daivy2333/StarryOS/blob/f8819a2f0da205bacfdee80cba276cc278cc452d/crates/axplat-riscv64-lichee-d1/src/time.rs) 建立 RED witness：24,000,000 ticks 必须等于 1,000,000,000 ns；保存旧实现得到 984,000,000 ns 的失败证据。[R1]
- [ ] 1.3 用现有 Async QEMU/D1 日志建立 benchmark RED witness，确认缺少 S41/S42/S43、`instructions_per_byte`、`overlap_efficiency` 和 wakeup overshoot 字段；将检查命令和结果写入 evidence README。[R2-R7]
- [ ] 1.4 记录实施前 diff 边界：允许 `tests/benchmark.c`、D1 `time.rs`、测试见证和 q31 evidence；禁止 UART copier、IER、waker、TTY、drain 和 debug ABI 改动。[R9]

## 2. D1 Time Conversion

- [x] 2.1 在 D1 platform crate 中抽取可测试的宽整数 `mul_div_floor` helper，覆盖 0、1、24 MHz 一秒、frequency±1、单调性、round-trip 不超过一 tick和饱和边界；先运行测试并观察 1.2 的 RED。[R1]
- [x] 2.2 将 `ticks_to_nanos` 改为 `ticks × 1e9 / frequency`，将 `nanos_to_ticks` 改为 `nanos × frequency / 1e9`，中间值使用 `u128`，返回溢出时饱和到 `u64::MAX`。[R1]
- [x] 2.3 运行 time helper 测试、`cargo fmt --check` 和 `cargo check --package starryos --features lichee-d1 --target riscv64gc-unknown-none-elf`；记录命令、关键输出和退出码。[R1]
- [x] 2.4 构建 `lichee-d1-fullbench-command`，确认 timer deadline、boot image 打包和现有启动链无编译退化；不得用构建成功代替真板时间验证。[R1,R9]

## 3. Shared Benchmark Measurement Helpers

- [ ] 3.1 在 `tests/benchmark.c` 增加严格的 `/proc/instret` `u64` 读取函数，区分 open/read/parse/counter-regression 错误；用相邻读取报告采样开销，不从主 delta 扣除。[R3]
- [x] 3.2 扩展 write helper，分别累计 logical writes、实际 write syscall calls、accepted/completed bytes、short writes 和 errno；保持 blocking backpressure 与 `write_full` 语义不变。[R2,R3,R9]
- [x] 3.3 增加固定计算内核和 `volatile` sink，提供按绝对 deadline 运行并返回 iterations 的 helper；加入预热，禁止 I/O 和动态分配进入计算循环。[R4]
- [x] 3.4 增加绝对时间睡眠采样 helper，使用 `CLOCK_MONOTONIC | TIMER_ABSTIME`，保存原始 overshoot 后统一计算 P50/P95/P99/max；任一 syscall 错误保留 errno。[R5,R7]
- [ ] 3.5 扩展 manifest，输出 benchmark version、target mode、startup chain、root provider、device path、`fstat` 设备号、hart 数、payload/iterations、timer source 和相关 feature；验证 Async/Console 不支持字段可显式降级。[R7,R8]

## 4. Async Benchmark Sections

- [x] 4.1 扩展 S11 输出 `submit_fraction` 和 `producer_available`，保留 enqueue/final-drain 原始时间、短写、完成字节和 drain 错误；零分母输出 `not-available`。[R2,R7]
- [x] 4.2 新增 S41 `TX CPU Work`，让 instret 窗口覆盖 write 开始至 final TEMT drain；对 64/256/1024 B payload 输出 raw counters、delta、instructions/byte 和 instructions/write-call。[R3,R7]
- [x] 4.3 新增 S42 `TX Compute Overlap`，使用 64 B × 100、一次预热和至少五轮采样；输出 write-return、idle/UART iterations、useful-work/ms、drain 和 overlap efficiency。[R4,R7]
- [x] 4.4 新增 S43 `Timer Wakeup Overshoot`，分别采集 idle 与 Async TX backlog 窗口；deadline 从计划时间递增，窗口不足时输出 `not-applicable reason=no-overlap-window`。[R5,R7]
- [ ] 4.5 为 S41/S42/S43 加入 workload-local TX debug reset/snapshot，输出原始 counter 和 hw-send-calls/KiB、zero-send/KiB、ring-pop/KiB、no-progress/KiB、bytes/ring-pop、bytes/hw-send；ioctl 不可用时不阻塞主场景。[R6]
- [x] 4.6 审查所有新 section：测量前 `fflush+tcdrain`，采样期间不打印，完成后统一输出；未补齐短写、drain error、超时或完成字节错误必须 FAIL。[R2-R7,R9]

## 5. Async Static and QEMU Gates

- [x] 5.1 交叉编译 `tests/benchmark` 和 `benchmark-fullbench-elf`，确认无 warning/error、静态 `ET_EXEC`、无 relocation，并记录 benchmark binary SHA-256。[R7,R8]
- [x] 5.2 运行 `cargo fmt --all --check`、D1/QEMU 相关 `cargo check` 与受影响 crate tests；已有环境阻塞必须记录命令和边界，不得写成 PASS。[R1,R9]
- [x] 5.3 运行 QEMU rootfs benchmark，将完整输出保存为 `.claude/analysis/q31-cpu-efficiency-evidence/async/qemu-rootfs.log`；验证 S11、S41-S43、S40/local diag、Done 和 exit 0。[R2-R8]
- [x] 5.4 核对 QEMU 证据只用于功能和同环境相对行为；任何 QEMU `line_rate_pct` 不得进入 D1 物理线速结论。[R8,R9]

## 6. Async D1 Gate

- [ ] 6.1 构建并检查 `starry-lichee-fullbench-command-boot.img`，记录 commit、feature、toolchain、image SHA-256、烧录命令和串口配置。[R1,R7,R8]
- [x] 6.2 在 Lichee RV Dock 运行 Async benchmark，将原始串口输出保存为 `.claude/analysis/q31-cpu-efficiency-evidence/async/d1-fullbench-command.log`；验证 timer manifest、S11、S41-S43 和分段 counter 完整。[R1-R8]
- [x] 6.3 验证每个有效样本完成字节正确、未补齐 short write 为 0、drain error 为 0、Done、exit 0；与冻结 Async D1 baseline 比较既有 S10/S20/S40，超过 5% 的退化必须解释或阻塞 Gate。[R2-R9]
- [x] 6.4 运行至少五轮 S41/S42/S43，保留 raw samples、中位数和范围；确认 instret delta 非零且 sampling overhead 明显小于 workload delta，否则扩大 workload 后重测。[R3-R7]
- [x] 6.5 在 evidence README 标记 Async Gate 状态。真板不可用时写 `ENV BLOCK`，不得以 QEMU 代替 D1 完成。[R8]

## 7. Plan Review Boundary

- [x] 7.1 `000-initial` Act Response 完成后，由 `openspec-plan` 检查实际代码、diff、time RED/GREEN、QEMU/D1 日志和 driver-scope 边界，并记录 follow-up decision。[R1-R9]
- [ ] 7.2 只有 Async review 通过后才创建 Console iteration；不得在 `000-initial` 中切换到 `console-lichee` 或改写其代码。[R8,R9]
- [x] 7.3 在覆盖首版 Async 日志前，将其保存到 `async/iteration-000-invalid/`，记录原 hash、`byte_ok=0` 和不得进入 comparison 的原因。[R3,R7,R8]
- [x] 7.4 按 iteration 001 修正 time test、S11、S41-S43、counter、manifest 和 evidence metadata，并用新 QEMU/D1 日志重新判定 tasks 1.2-6.5。[R1-R9]
- [x] 7.5 按用户批准保留 `.claude/runbooks/qemu-build.md` 的 benchmark 注入说明；在收尾 Act Response 中完整列出 Runbook、tracked binary、被覆盖的 docs 日志和 evidence 日志。[R8,R9]
- [x] 7.6 iteration 003 通过 Plan Review 后才勾选 7.2 并创建 Console iteration；Async 诊断字段不再触发重测。[R1-R9]

## 7A. Async Evidence Declaration Closeout

- [x] 7.7 更新 evidence README：12/12 time tests、iteration 000/001/002 状态、完整 SHA-256、构建与运行环境、Async Gate 结果。[R1-R9]
- [x] 7.8 在 README 记录 derivation：每个 section 的 completed bytes、`hw_send_calls_per_kb = hw_send_calls / (completed_bytes / 1024)`，并给出当前 D1 可复算值。[R6-R8]
- [x] 7.9 声明限制：instret 是 hart-wide CPU-work proxy；当前日志不能细分 partial/zero-progress/errno；counter regression reason 不独立；manifest revision/dirty 由外部 Git 状态和 hash 补足；不得声明 CPU utilization。[R3,R6-R9]
- [x] 7.10 只检查现有日志和 hash，不修改 benchmark、不重新构建、不覆盖或重采集 Async 日志；运行 OpenSpec 与 Markdown/diff 检查并记录结果。[R1-R9]

## 8. Console Follow-up

- [ ] 8.1 在后续 iteration 中将同一 benchmark 与 D1 time conversion 更新应用到 `console-lichee`；记录源码/binary hash，任何差异必须限于 Console 适配并写明原因。[R1-R9]
- [ ] 8.2 采集 Console QEMU 日志到 `.claude/analysis/q31-cpu-efficiency-evidence/console/qemu-rootfs.log`；debug counters 与 loaded timer 无法建立时按 spec 标记不可用。[R2-R8]
- [ ] 8.3 采集 Console D1 日志到 `.claude/analysis/q31-cpu-efficiency-evidence/console/d1-fullbench-command.log`；验证 payload、迭代数、设备、timer conversion、drain policy、Done 和 exit 0 与 Async 相同。[R1-R9]
- [ ] 8.4 用户可用新日志覆盖 `docs/d1_console.md` 和 `docs/qemu_console.md`；覆盖前后均验证 q31 baseline evidence 的 SHA-256 未变化，且不删除 docs 源文件。[R8]

## 9. Comparison and Change Gates

- [ ] 9.1 在 `comparison/result.md` 按 caller release、useful work、instructions/byte、timer response、path counters、line rate、latency 和 correctness 比较 Async/Console；不同完成点不得计算倍率。[R2-R9]
- [ ] 9.2 若 Async 只降低 submit fraction，但 instructions/byte、overlap 或 wakeup tail 未改善，结论必须写成“等待转移到后台”，不得写成系统 CPU 效率改善。[R3-R5,R9]
- [ ] 9.3 运行 `openspec validate q31-async-uart-cpu-efficiency-benchmark --strict`、`openspec validate --changes`、`openspec validate --specs` 和 `git diff --check`，记录输出与退出码。[R1-R9]
- [ ] 9.4 审查最终 diff 不包含 UART copier、THRE retry、IER、waker、TTY、drain 或 debug ABI 语义修改；发现后移出本 change。[R9]
- [ ] 9.5 在全部 Async/Console evidence 和 comparison 完成前保持 change active；本 Plan 不更新全局 tasks、SNAPSHOT、I01/I12 状态，也不归档 change。[R8,R9]
