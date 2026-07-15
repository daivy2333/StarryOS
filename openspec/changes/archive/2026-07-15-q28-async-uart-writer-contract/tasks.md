## 0. Gate 3 当前状态与 RED witness

- [x] 0.1 在修改源码前用 CodeGraph 与定点搜索记录 `AsyncUartWriter` 构造、复制、`TtyWrite::write(&self)`、`RingBufTx::push` 以及 StarryOS direct-output/ldisc-echo 的完整调用路径；验收：证据明确当前存在两个可并发 producer，且没有把 RX 单 consumer 或 MPSC 改造纳入 Q28。
- [x] 0.2 记录工作树、工具链、feature、Q27 测试矩阵，以及 QEMU/D1 同环境性能基线与手测命令；验收：用户各执行一次 candidate 后能与同一配置和负载下的 Q27 baseline 比较，单次结果不声明统计显著性。
- [x] 0.3 先增加裸 writer API contract 的 RED compile-fail witness：`AsyncUartWriter` 不可 `Clone`、安全代码不能绕过唯一 producer 构造、外部调用者不能直接 `RingBufTx::push`；验收：当前 API 至少使一个期望失败的片段意外编译，证明测试能捕获契约缺口。
- [x] 0.4 先增加 StarryOS writer wrapper 的 RED 行为测试，使用 barrier 与 test-only 临界区探针制造 direct-output/echo 重叠；验收：测试在生产改动前因包装器尚不存在或检测到重入而失败，不通过并发访问真实 `UnsafeCell` 制造 UB。

## 1. 收敛 uart_16550 裸 TX producer capability

- [x] 1.1 将 `RingBufTx::push` 收窄为 crate-private，保留 ISR consumer、readiness、waker 与 telemetry 路径；验收：crate 外无法直接写 TX ring，crate 内现有 consumer/测试仍可编译。
- [x] 1.2 移除 `AsyncUartWriter: Clone` 与其直接 `TtyWrite` 实现，提供接收 `&mut self` 的同步 nonblocking `try_write(&mut self, &[u8]) -> usize`；验收：每次调用只返回本次实际接受的输入前缀长度，空写、partial、full 和 wrap-around 语义与 Q27 一致。
- [x] 1.3 将裸 writer 构造改为带 `# Safety` 文档的显式 `unsafe` 唯一 producer 构造；验收：文档说明调用方必须保证同一 TX ring 在 writer 生命周期内只存在一个 producer capability，并覆盖重复构造和直接 ring push 的禁止条件。
- [x] 1.4 让 `embedded_io_async::Write` 通过 `&mut self` 委托 `try_write`，保持 flush/drain、writable hint、waker register 和非空写无进展处理不变；验收：不引入 OS/VFS/syscall 类型或语义，crate 仍可独立使用。
- [x] 1.5 更新导出文档、doctest 与 API contract 测试；验收：RED compile-fail witness 转 GREEN，并证明裸 writer 的唯一性由类型/可见性/unsafe 构造共同表达，而非只靠注释。

## 2. 增加 StarryOS 可复制的串行化 writer adapter

- [x] 2.1 在 async TTY 集成层为 crate 裸 writer 建立 `RawArceOsWriter` cfg alias，并定义本地 `ArceOsWriter` newtype，内部使用 `Arc<SpinNoPreempt<RawArceOsWriter>>`；验收：`Tty::new()` 可继续分别持有 direct-output 与 ldisc-echo clone，PTY writer 模型与公共 `DeviceOps` 签名不变。
- [x] 2.2 为 `ArceOsWriter` 实现 `Clone`、`TtyWrite` 与 `TtyWriteReady`，只在一次 `try_write` producer push 期间持锁；验收：readiness 查询/注册保持 Q27 的 hint + register/recheck 契约，锁不跨 `poll_io`、`.await`、flush/drain 或重试等待。
- [x] 2.3 在 `ASYNC_TTY` 初始化路径只构造一次裸 writer，并为 unsafe block 写出可审计的 `SAFETY` 证明；验收：证明覆盖静态实例唯一初始化、无第二 producer capability、ISR 仅为 TX consumer，仓库中没有其他安全旁路。
- [x] 2.4 审计中断、panic/console、echo 与普通 fd write 的锁顺序和 no-preempt 约束；验收：没有递归获取 producer lock、没有在锁内等待 TX 空间，也不声称 Q28 已完成 Q24 的真实 multi-hart 验证。

## 3. 并发正确性与 Q27 回归测试

- [x] 3.1 将 0.4 的并发探针转 GREEN，覆盖两个 clone 同时提交不同标记 payload、多轮 barrier 交错与 short write；验收：每次 writer 调用的 accepted prefix 内部连续，不发生 producer 临界区重入、丢失、重复或跨调用字节交织。
- [x] 3.2 增加 direct-output 与 ldisc-echo 共享同一 adapter 的聚焦测试；验收：两条真实 TTY 调用路径都经过同一把 producer lock，测试不要求不同 write 调用之间的全局公平性或 syscall 级原子性。
- [x] 3.3 回归 Q27 blocking/nonblocking、OUT readiness、register-then-recheck、ONLCR 源字符边界、PTY short write 与 `tcdrain` 语义；验收：既有 6 个聚焦场景和新增 writer contract 场景全部通过。
- [x] 3.4 检查 RX reader 的 clone/consumer 风险并只记录为独立 follow-up；验收：Q28 不修改 RX ring、不实现 MPSC ring，也不借机扩大 UART/TTY API 范围。

## 4. 静态检查与构建 Gate

- [x] 4.1 运行仓库格式检查以及 `uart_16550` default/async/embedded-io/all-features 的 check、unit test、doctest、Clippy 和 rustdoc 矩阵；验收：所有命令 exit 0、测试 0 failed、无新增 warning。
- [x] 4.2 运行 StarryOS 目标构建与 TTY 聚焦测试；验收：`/dev/console`、PTY、poll/select/epoll OUT、echo 和 cfg 分支全部编译，裸 writer API 迁移没有遗漏调用点。
- [x] 4.3 复查 crate 依赖边界、unsafe surface 和锁域；验收：`uart_16550` 不依赖 StarryOS 同步原语，所有新增 unsafe 均有局部 `SAFETY` 证明，producer lock 不跨等待点。
- [x] 4.4 运行 `openspec validate q28-async-uart-writer-contract`；验收：proposal、design、delta spec、tasks 与实现保持一致且校验通过。

## 5. QEMU、D1 与性能 Gate

- [x] 5.1 用户在 QEMU 手动运行一次 console/echo 与 candidate 基准，自动化 Gate 覆盖并发 writer stress 和 Q27 功能回归；验收：无 crash、hang、drain error、字节损坏或前缀破坏，candidate 相对 Q27 baseline 不超过 3% 退化。
- [x] 5.2 由用户在 D1 真板用同一镜像配置、串口参数与负载手动运行一次功能回归和 candidate 性能采样；验收：结果满足 5.1 的正确性与 3% 阈值，超过阈值则阻断 Gate 并回到设计评估锁粒度。
- [x] 5.3 对照 Q27 baseline 记录 QEMU/D1 单次结果并解释锁成本、吞吐和延迟变化；验收：保存原始输出和配置，不用理论线速替代 before baseline，不声明统计显著性，也不把单 hart 证据外推为 Q24 multi-hart 证明。

## 6. Review、文档同步与收尾

- [x] 6.1 先做 spec compliance review，再做 code quality/soundness review；验收：逐条映射唯一 producer、共享 adapter、锁生命周期、accepted-prefix、Q27 保持和性能要求，并关闭所有 blocking finding。
- [x] 6.2 汇总 Gate 3/4/5 证据及已知边界，经用户确认后同步 `.claude/docs/tasks.md`、`SNAPSHOT.md`、architecture/learned/optimization 中确有长期价值的条目；验收：Q28 状态与 OpenSpec change 一致，RX follow-up、MPSC 远期候选和 Q24 边界可追溯。
- [x] 6.3 仅在实现、测试、QEMU/D1、review 和文档 Gate 全部通过后归档 change；验收：归档前 `openspec validate q28-async-uart-writer-contract` 再次通过，无未勾选实施任务或未处置的回归。

> 本计划停在 Phase 3 入口。未通过 Gate 1、Gate 2 且未收到用户明确执行授权前，不勾选任务、不创建 RED 测试、不修改 StarryOS 或 `uart_16550` 源码。


## Gate 证据（2026-07-15）

- RED：safe benchmark bypass compile-fail doctest 在旧 API 上意外编译；wrapper 集成测试在生产 helper 不存在时编译失败。GREEN 后 4 个 API compile-fail doctest、QEMU feature 1 个共享流测试及 `qemu,smp` 2 个并发 accepted-prefix 测试全部通过。
- crate：default `45 tests + 8 doctests`；all-features `62 tests + 8 doctests + 4 compile-fail doctests`；fmt、embedded-io check、Clippy `-D warnings`、rustdoc 全部 exit 0。
- kernel：Q27 `tty_write_logic` 6/6；Q28 wrapper `qemu` 1/1、`qemu,smp` 2/2；kernel `qemu` 与 workspace `qemu,smp` check exit 0。kernel 全量 Clippy 仍有 6 个与 Q28 无关的既有 baseline finding，本 change 未扩大处理范围。
- QEMU raw：`docs/qemu_out.md` 完整运行至 `Done.`，short-write/drain-error/10ms tail 为 0。相对 Q27，S10 64B P50 `0.426 -> 0.384 ms`（-9.86%）、S10 1024B P50 `5.327 -> 4.488 ms`（-15.75%）、S20 1B P50 `0.163 -> 0.151 ms`（-7.36%）。startup ring 修正旧版 102400B 误计数后为 `258168.61 -> 275150.47 KB/s`（+6.58%）。候选使用 `NET=n` 避开宿主 UDP 5555 冲突；idle network device 不参与 benchmark，单样本不声明统计显著性。
- D1 raw：`docs/d1_out.md` 完整运行至 `benchmark exited with code: 0`。相对 Q27 tracked baseline，S10 64B P50 `5.597 -> 5.603 ms`（+0.107%）、S10 1024B P50 `87.524 -> 87.520 ms`（-0.005%）、S20 1B P50 `0.187 -> 0.185 ms`（-1.070%）；64B/1024B 分别为 96.8%/98.8% 线速，`slow_poll_exh=0`、`yield_exh=0`。startup ring 按实际 65536B 归一化后 `709534.37 -> 731136.12 KB/s`（+3.04%）。
- Review：spec compliance 后再做 code quality/soundness；producer lock 仅覆盖短暂 raw access，不跨 `poll_io`、await、flush/drain 或调度等待，ISR 不获取该锁。RX clone 风险、MPSC 公平性和真实 multi-hart 证明分别保留给独立 follow-up、O85 与 Q24。
