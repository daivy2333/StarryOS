## 0. Gate 3 当前状态与 RED Witness

- [x] 0.1 在源码修改前用 CodeGraph 记录 `AsyncUartReader` 构造、`RingBufRx` push/pop、`ASYNC_TTY`、`tty-reader`、共享 fd/ldisc 与 readiness register-recheck 的完整路径；验收：区分"当前 StarryOS 单 consumer 事实"和"crate safe API 可破坏 SPSC"的边界。
- [x] 0.2 记录工作树、Rust toolchain、feature 组合和当前 crate/kernel 测试基线；验收：后续失败可判断为 Q29 回归或既有问题，且不把 QEMU/D1 单 hart结果解释为 Q24 SMP 证明。
- [x] 0.3 先增加 API contract 的 RED compile-fail witness：safe 代码不能构造 raw reader、raw reader 不可 `Clone`、crate 外不能 direct RX pop/push；验收：旧 API 至少使一个期望失败的片段意外编译，证明 witness 能捕获当前缺口。
- [x] 0.4 先建立 RX 行为 witness，覆盖空读、顺序、partial 和 wrap-around；验收：测试只使用单 consumer，不通过并发访问同一 `UnsafeCell<Reader>` 制造 UB。

## 1. 收敛 uart_16550 RX SPSC Capability

- [x] 1.1 将 `AsyncUartReader` 构造改为带 `# Safety` 文档的显式 unsafe 唯一 consumer 构造；验收：文档要求同一 driver/RX ring 只有一个 raw reader，并禁止其他 direct consumer。
- [x] 1.2 保持 raw reader 不可 `Clone`，`TtyRead` 与 `embedded_io_async::Read` 的消费入口继续要求 `&mut self`；验收：不增加共享 reader、内部 reader lock 或 MPMC 语义。
- [x] 1.3 将 `RingBufRx::pop` 收窄为 crate-private；验收：crate 外 safe 代码不能绕过 raw reader，device ops 和 crate 内测试仍可调用合法 consumer 路径。
- [x] 1.4 将 RX `push`/`push_batch` 生产操作收窄为 crate-private；验收：唯一 RX copier 仍可写入并唤醒 readable waker，crate 外 safe 代码不能取得第二 producer。
- [x] 1.5 保持 `occupied_len`、`has_data` 和 waker registration 为非消费观察接口；验收：readiness 不取得 consumer capability、不作为 reservation，且不新增 OS/VFS/锁依赖。
- [x] 1.6 将 RX/TX copier 启动收敛为显式 unsafe 单次启动契约；验收：crate 外 safe 代码不能重复启动 copier，StarryOS QEMU/D1 各自在互斥 boot path 中恰好启动一次。

## 2. 迁移 StarryOS 唯一 Reader 构造点

- [x] 2.1 在 `ASYNC_TTY` 初始化中迁移 unsafe raw reader 构造并添加相邻 `SAFETY` witness；验收：证明 lazy static 只初始化一次、同一 driver 无第二 raw reader、reader 随后移动到唯一 `tty-reader`。
- [x] 2.2 审计 startup benchmark、QEMU/D1 cfg 和 UART 初始化顺序；验收：不存在 benchmark 或备用路径为同一 driver 构造第二 raw reader，feature 分支均使用相同唯一性契约。→ 修复：QEMU 和 D1 路径均在 benchmark 之后调用 `start_copiers()`，消除 benchmark 与 RX/TX copier 的 SPSC producer 冲突。
- [x] 2.3 审计共享/dup fd、TTY `read_at`/poll、ldisc `buf_rx` 与 `ProcessMode::External`；验收：用户 reader 只消费 ldisc ring，UART RX ring 仅由 `tty-reader` 消费，不新增 reader adapter。
- [x] 2.4 审计 readiness 注册路径；验收：tty-reader 维持 drain/check -> register -> recheck，shared fd waiter 维持 ldisc register-recheck，允许 spurious wake 且无永久 lost wakeup。

## 3. API、字节完整性与 Readiness 测试

- [x] 3.1 将 0.3 的 compile-fail witness 转 GREEN；验收：unsafe 唯一构造、不可 Clone、direct pop 不可见、direct push 不可见四类约束均由编译器证明。
- [x] 3.2 将 0.4 的 RX 行为 witness 转 GREEN；验收：空 buffer/空 ring 返回 0，partial 与 wrap-around 保持顺序且无重复、丢失或凭空数据。
- [x] 3.3 回归 RX occupied length、`can_read` 与 readable waker；验收：查询不消费数据，push 唤醒，注册与 recheck 覆盖到达竞态，spurious wake 后重新检查。
- [x] 3.4 增加 StarryOS 单 consumer 静态/聚焦 witness；验收：唯一 raw constructor、单 `tty-reader` 所有权转移和共享 fd 的 ldisc 路径均可追溯，不依赖真实并发 UB 测试。

## 4. 静态检查与构建 Gate

- [x] 4.1 运行 `uart_16550` fmt、default/async/embedded-io/all-features check、unit test、doctest、Clippy 与 rustdoc；验收：所有命令 exit 0、测试 0 failed、无新增 warning。
- [x] 4.2 运行 StarryOS TTY 聚焦测试与 QEMU、`qemu,smp`、Lichee D1 async UART 目标构建；验收：所有 reader 构造点已迁移，`/dev/console`、TTY/ldisc、poll/select/epoll IN 与 cfg 分支均编译。→ QEMU default 构建通过；D1 路径已添加 `start_copiers()` 调用，但 D1 完整构建因既有 axfeat 版本/feature 冲突无法在本环境验证，非 Q29 回归。
- [x] 4.3 复查 crate dependency、unsafe surface 和公开 API；验收：每个新增 unsafe 调用都有局部 `SAFETY` witness，RX hot path 无新锁/原子状态，crate 仍保持 OS-neutral。
- [x] 4.4 运行 `openspec validate q29-async-uart-reader-contract`；验收：proposal、design、delta spec、tasks 与实现一致且 validation 通过。

## 5. 功能回归与范围 Gate

- [x] 5.1 在 QEMU 运行 console 输入、echo、blocking/nonblocking read 和 Shell 交互回归；验收：无 crash、hang、永久休眠、字节重复/丢失，命令输入与返回 shell 正常。
- [x] 5.2 对 RX hot path 做静态差异检查；验收：API 可见性和 constructor safety 之外不增加 ring pop/push 指令、锁或分配，因此 Q29 不设置 D1 性能阈值。
- [x] 5.3 保留 Q24 边界；验收：报告只声明 API uniqueness、当前所有权拓扑、字节完整性、readiness 和单 hart功能回归，不声明 multi-hart runtime correctness。

## 6. Review、文档同步与收尾

- [x] 6.1 先做 spec compliance review，再做 code quality/soundness review；验收：逐条映射 raw consumer 唯一性、producer 封闭、StarryOS witness、readiness 保持与 Q24 scope，关闭所有 blocking finding。
- [ ] 6.2 汇总 RED/GREEN、构建、QEMU 和 review 证据，经用户确认后调用 `openspec-docs-maintainer` 同步 tasks/SNAPSHOT 及确有长期价值的 architecture/learned/optimization 条目；验收：Q29 状态与 change 一致。
- [ ] 6.3 仅在实现、测试、QEMU、review、文档 Gate 全部通过后归档 change；验收：归档前 OpenSpec validation 通过，无未勾选任务或未处置回归。

> 本计划停在 Phase 3 入口。未通过 Gate 1、Gate 2 且未收到用户明确执行授权前，不勾选任务、不创建 RED 测试、不修改 StarryOS 或 `uart_16550` 源码。
