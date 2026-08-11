## Iteration allocation

后续按单一可诊断目标推进，不再把全部实现、自动 Gate 和手工测试塞进同一轮：

| Iteration | Scope | Manual QEMU |
|---|---|---|
| 000 | T1、T2 与 T3 的主体实现 | No |
| 001 | 收紧 T3 的生产绑定测试见证；保留 D1 基线缺口 | No |
| 002 | iteration 001 Review 修复 + T4.1 one-completion device primitive | No |
| 003 | iteration 002 Review 修复 + T4.2 Router handoff 与 space wake | No |
| 004 | iteration 003 Review 修复 + T5.1 lifecycle/register-recheck 决策层 | No |
| 005 | iteration 004 Review 修复 + T5.2 唯一 RX task 与 budget | No |
| 006 | T6.1 ISR publish/wake 与 telemetry | No |
| 007 | T6.2 probe/stimulus 与自动构建入口 | No |
| 008 | T7 全量自动 Gate、diff Review 与 Evidence 准备 | No |
| 009 | T8 sandbox 复跑和 QEMU runtime 手测 | Yes, user-only |

只有当前一轮通过 Plan Review 后才创建下一轮；编号是当前计划顺序，不预先生成空
iteration。若 Review 发现阻塞问题，优先插入小型修复轮次，后续编号顺延。最终一轮
只包含用户手工测试及其 Evidence，不夹带产品实现。

## 1. 本地化依赖并建立 queue contract

- [x] 1.1 在 `crates/axdriver_net`、`crates/axdriver_virtio` 和
  `crates/virtio-drivers` 放入 registry 当前确切版本，修改根 `Cargo.toml` 的
  workspace `exclude` 与 `[patch.crates-io]`，更新 `Cargo.lock`。WHY 是
  `RING_EVENT_IDX` 的有效控制需要工作区拥有的修改面；HOW 是先在无行为修改时对
  三个 manifest 执行 offline check/test，并确认 QEMU dependency tree 解析到本地
  path；EXPECTED 是版本、feature 和现有 QEMU build 语义不变。禁止修改
  `/home/daivy/.cargo/registry`、本地化 `axdriver` 或关闭 feature。验证命令：
  `cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline`、
  `cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net`、
  `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib` 和
  `cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf`；任一版本
  不匹配、patch 未生效或基线测试失败时停止。

- [x] 1.2 在 `crates/axdriver_net/src/lib.rs::NetDriverOps` 增加默认 `None` 的
  object-safe RX queue-control accessor，并定义 transport-neutral
  `NetQueueControl`；在 `crates/axdriver_virtio/src/net.rs::VirtIoNetDev` 提供
  VirtIO 实现。WHY 是 axnet 必须经唯一设备 owner 控制 completion 可见性和通知，
  但不能看见 ring/token；HOW 是 contract 只表达 has-completion、suppress、
  arm-and-recheck 和 `DevResult`，reap/refill 继续复用 `receive/recycle_rx_buffer`；
  EXPECTED 是 VirtIO 返回 control，其他 driver 通过默认实现保持兼容。RED 是调用
  accessor 或 adapter compile test 缺少接口；GREEN 是两个 driver crate check
  通过且公共 API 搜索不到 VirtIO/DWMAC descriptor 类型。若实现需要 downcast、
  raw 私有字段穿透或第二套 buffer API，停止并返回 Plan。

## 2. 修复 `RING_EVENT_IDX` suppression/rearm

- [x] 2.1 在 `crates/virtio-drivers/src/queue.rs` 的 FakeTransport tests 先增加
  RED cases，覆盖 EVENT_IDX suppression、suppressed 状态下连续 `pop_used` 不重臂、
  arm 空队列、arm 后已有 completion、non-EVENT_IDX flags 和 `u16` wrap；再实现
  design D2 的 notification state、`used_event` 写入和 arm 后 barrier/recheck。
  WHY 是当前 `set_dev_notify` 在 `event_idx=true` 时 no-op；EXPECTED 是 drain 期间
  `used_event` 不随 completion 前移，arm 返回可靠 pending snapshot，原 queue tests
  全部 GREEN。验证：
  `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests -- --nocapture`。
  若必须关闭 EVENT_IDX、无法给 arm 后 recheck 建立单元见证或现有非 net queue
  测试退化，停止并返回 Plan。

- [x] 2.2 在本地 `virtio-drivers::device::net::VirtIONetRaw` 和
  `axdriver_virtio::VirtIoNetDev` 接出 RX-only suppress/arm-and-check，保持 TX queue
  不变。WHY 是现有 `disable_interrupts/enable_interrupts` 同时操作 RX/TX，越过
  MS04 范围；HOW 是 adapter 只委托 receive queue，并把 queue error 映射为
  `DevError`；EXPECTED 是 T1 queue contract 可用且同步 TX API/diff 无行为修改。
  RED 是 adapter check 缺少 RX-only 方法；GREEN 是 T2.1 tests 与两个 driver crate
  checks 通过。若需要暴露 `VirtQueue`、token 或 send queue 状态，停止。

## 3. 修复 shared critical-section 的 IRQ restore

- [ ] 3.1 新增可由 `tests/ms04-async-rx-host-harness.rs` 引入的 kernel 纯
  restore-policy seam，并把它加入 `Makefile::host-test`；先写 enabled、disabled、
  nested RED cases，再在 `kernel/Cargo.toml` 启用 `critical-section 1.2` 的
  `restore-state-bool`，以 `set_impl! + Impl` 替换 `kernel/src/lib.rs` 的手写 ABI。
  WHY 是 ISR 内 `AtomicWaker::wake()` 不得提前开 IRQ；EXPECTED 是进入 disabled
  后 release 仍 disabled，进入 enabled 的最外层 release 才 enable，旧 ABI symbol
  不再手写。GREEN 为新 host cases、`make host-test`、UART 62 unit + 18 doctest 和
  QEMU/D1 target compile 全通过。禁止修改 UART ring/waker、在 release 中无条件
  enable 或用 mock 结果替代 target compile；feature 冲突、官方 ABI 无法链接或
  UART 回归失败时停止。

  Iteration 000 已完成主体实现，但本项保持未完成：Review 发现 host harness 重复
  执行同一组 6 个模型测试，且生产 `KernelCriticalSection` 未复用被测 seam；D1
  target compile 也未通过。Iteration 001 已让生产与 host tests 复用同一 seam；
  D1 未通过仍使本项保持未完成。

- [x] 3.2 为 production glue 增加永久 source guard。WHY 是 iteration 001 的 host
  tests 覆盖 seam 行为，但未来 `KernelCriticalSection` 仍可内联 axhal 调用并绕过
  seam；HOW 是在 MS04 host harness 中加入对真实 `kernel/src/lib.rs` 的结构化断言，
  同时用 legacy direct-call fixture 证明 guard 会 RED；EXPECTED 是 guard 要求
  `critical_section_policy::acquire/release` 委托存在，并拒绝在 Impl 方法体内复制
  restore 决策。若只能匹配整文件中的 axhal backend 合法调用、依赖行号或需要 Rust
  parser 依赖，停止并返回 Plan。

- [x] 3.3 修复当前 kernel manifest 的 rustfmt Gate。WHY 是 iteration 001 fresh
  `cargo fmt --manifest-path kernel/Cargo.toml -- --check` 在 4 个既有文件上退出 1；
  HOW 是只接受 rustfmt 对 `drivers/mod.rs`、`drivers/uart_init.rs`、
  `drivers/virtio_net_irq.rs`、`syscall/fs/ctl.rs` 的机械输出，并用 whitespace-insensitive
  diff 加人工检查确认只有 module/import 排序、换行和缩进；EXPECTED 是 kernel fmt
  check 退出 0。若 rustfmt 触及更多文件、改变 cfg 归属或表达式语义，或掩盖产品
  编译错误，停止。

## 4. 把 axnet RX 收敛为 one-completion 与现有 Router handoff

- [x] 4.1 修改 `crates/axnet/src/device/mod.rs::Device`、
  `device/ethernet.rs::EthernetDevice`、`device/loopback.rs::LoopbackDevice` 和现有
  `router.rs::Router::poll` caller，用携带 `DevError` 的
  `Empty/Consumed/Delivered/Fault` 一次物理进度替代当前 `recv() -> bool`。WHY 是
  Ethernet 当前会在一次调用内无界跳过 ARP/非 IPv4，budget 不可计数；HOW 是每次
  Ethernet 调用最多执行一个 driver receive，并保证已取得的 `NetBufPtr` 在返回前
  恰好 recycle 一次，正常 ARP 保持同步 TX；EXPECTED 是 malformed、非目标、ARP
  算 Consumed，IPv4 算 Delivered，Again 算 Empty，receive/recycle error 算 Fault；
  Router polling 对 Consumed/Delivered 继续、Empty/Fault 停止，保持本轮前的 polling
  行为。axnet tests 通过 test-only `axdriver/dyn` 和本地 `axdriver_net` buffer pool
  建立 fake NIC，覆盖连续两帧每次只 receive 一次、ARP 同步 TX、malformed/非目标、
  IPv4、Again、receive fault、recycle fault、Router buffer enqueue fault 和 loopback。
  任何 enqueue 失败都必须先 recycle 再返回 `Fault(DevError::BadState)`，不得 unwrap。
  若任何路径可持有 buffer 跨返回、需要修改 registry axdriver、recycle 失败被
  unwrap/panic 隐藏、同步 TX 被改为 async，或 caller adaptation 提前实现 T4.2
  owner/space-wake，停止。

- [x] 4.1R 关闭 iteration 002 Review 的 test witness 与 host stub 问题。把
  `__axklib_0_3_mem_iomap` test-only stub 改为 trait-ffi 实际使用的 Rust ABI 和
  `PhysAddr, usize -> AxResult<VirtAddr>` 精确签名，并返回可诊断错误而非依赖
  `unreachable!`；补充 ARP reply 触发 pending IPv4 同步 TX，以及 Router enqueue
  error 与 recycle error 同时发生时 recycle error 优先的 tests。WHY 是当前同名符号
  只能满足链接，不能安全替代 extern contract，且两个获批场景没有永久见证。
  EXPECTED 是新 tests 先 RED，修复后 axnet 全量 tests、QEMU feature tree 和 compile
  回归通过。若需要产品 iomap、修改 axklib/trait-ffi、改变同步 TX 行为或隐藏 recycle
  failure，停止。

- [x] 4.2 修改 `crates/axnet/src/router.rs::Router` 与
  `service.rs::Service`，增加按唯一 target device 的 RX-only one-step service、
  active/faulted owner skip 和 Router-space software wake。WHY 是 queue task 必须在
  buffer full 时先停 reap，而 10ms Service 仍需消费 Router packet；HOW 是 fake
  Router tests 先覆盖 64-slot 满前进度、满时零 reap、释放空间一次 wake、active
  eth skip、loopback 保持推进，再实现状态接口。T4.2 只增加
  `PollingOwned/AsyncOwned` 消费权视图，不提前实现 T5 lifecycle；space signal 使用
  单一 `embassy-sync::AtomicWaker`、非 Relaxed waiting bit 和 host-only
  `critical-section/std`。EXPECTED 是 RX-only 入口不调用
  smoltcp maintenance/ingress/egress，普通 Service poll 仍执行这些阶段。验证：
  `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture`。
  若需要第二个 NIC handle、复制 Service、busy polling 或创建 MS05 packet slot，
  停止。

- [x] 4.2R 关闭 iteration 003 Review 的可达性、竞态见证与定向格式问题。通过
  `Service` 保存的唯一 target index 暴露 crate-private RX one-step seam；把现有
  space signal 收敛为未来 sibling async RX 模块可注册的单 waiter 通知状态，并让
  Router-full 等待在 Service 锁内重新检查空间后才发布。补充不依赖 sleep 的
  register-before-wait、释放发生在 wait 前/后的确定性交错，以及 sibling-module
  可调用性见证；只机械格式化 `tests/ms03-irq-host-harness.rs` 和
  `tests/ms04-async-rx-host-harness.rs`。WHY 是 T4.2 的 Router primitive 与 signal
  当前都被 `service.rs` 私有边界挡住，且现有顺序测试不能证明 lost-wake 窗口关闭。
  EXPECTED 是 future caller 不传 raw device index、不取得第二个 NIC handle，并能按
  “锁外 register、锁内 service/recheck、锁外 Pending”顺序使用 seam。若需要 public
  API、第二个 waker、持有 Service guard 跨调度点，或必须清理 smoltcp/全工作区既有
  fmt 与 warning 基线，停止。

## 5. 建立唯一 RX task、budget 与 register-recheck

- [x] 5.1 在 `crates/axnet` 增加可 host/unit-test 的生命周期和调度决策层，并把
  T4.2R 的单 waiter signal 扩展为 generation/event/space 共用的通知状态；先写
  RED cases 覆盖 `Polling -> Spawned -> Active -> Faulted/Unavailable`、重复 start、
  preflight fail、event-before-register、register-during-event、arm 后事件、空 wake、
  budget=32、backlog self-yield 和 Router-full wait。WHY 是 owner、generation 和
  wake 交错必须在 QEMU 前可重复验证；HOW 是纯决策层不访问 MMIO 或 smoltcp，
  generation publish 使用 Release、观察用 Acquire、状态 CAS 用 AcqRel；EXPECTED
  是所有交错最终为 service/self-wake/sleep/fault 中一个明确结果，无 permanent
  Pending 或双 owner。若测试只能依赖时序 sleep、需要第二 executor 或仍有未决定
  状态，停止。

- [x] 5.1R 关闭 iteration 004 Review 的共享通知测试隔离和 arm-error 缺口。所有
  触碰全局 `RX_NOTIFY` 的 tests 必须使用同一串行 guard 或改用局部 `RxNotify`，并以
  高并发重复测试证明 waker 不再被测试间覆盖；`wait_decision` 必须接收
  `arm_rx_notify_and_check` 的 `DevResult`，把错误连同 `DevError` 映射为明确 Fault，
  不得伪装为 Quiescent/Sleep 或依赖 side channel。WHY 是 iteration 004 的单次 66
  tests 会通过，但 16 线程重复运行已复现共享 `RX_NOTIFY` waker 被覆盖；现有
  `ArmObservation` 也不能承载 queue-control error。若修复需要第二个 waker、时序
  sleep、全局单线程运行全部测试或丢弃 error category，停止。

- [x] 5.2 在 `crates/axnet/src/lib.rs`、`device/{mod,ethernet}.rs`、`router.rs`、
  `service.rs` 和 async RX 模块接入唯一 named axtask。Device 只通过 transport-neutral
  wrapper 暴露目标 NIC 的 completion/suppress/arm 操作；axnet start 只把生命周期
  CAS 到 Spawned 并创建一次固定名称任务。task 首次
  poll 在 Service 锁内完成 queue-control preflight、suppression 和 Active 发布，
  然后每轮最多服务 32 completion。WHY 是 task 未启动或 preflight 失败时 polling
  必须保持 owner；EXPECTED 是 budget 用尽保持 suppressed、自 wake 并经 axtask
  `block_on` 让出，Router 满等待软件 wake，空队列执行 check/register/arm/recheck，
  fatal 后保持 owner=Faulted 且不回退；所有 Pending/exit/fault 路径先释放 Service
  guard，future 不持锁跨调度点。T5.1 与 axnet full lib tests 必须 GREEN；
  source review 必须证明 active/faulted 下只有 task 调用目标 NIC receive。本轮只
  交付可调用 start entry，不从 kernel 启动；T6.1 在 ISR publish/wake 就绪后才接生产
  caller，避免 task 抑制通知后没有硬件事件入口。若
  `spawn_with_name` 之前必须切 owner、10ms fallback 仍可 reap 或 fatal 会自动启动
  polling，停止。
  <!-- T5.2a 已完成（Device/Router/Service transport-neutral queue-control seam、fake
  Ethernet/control tests、Service target methods、时间戳内部化，2026-08-11 Act）。
  T5.2b 已完成（RxRxFuture + service_round、global RX_LIFECYCLE、dormant pub
  start_rx_task + RX_TASK_NAME、cfg(test) spawn counter seam、poll_interfaces owner
  mapping、ServiceAccess Global/Injected 注入 seam、12 个 future 测试与 guard-release
  witness，90 axnet tests + 100×16-thread gate GREEN，2026-08-11 Act）。 -->

## 6. 接入最小 ISR 与运行观测

- [ ] 6.1 修改 `kernel/src/drivers/virtio_net_irq.rs` 及其 pure logic/snapshot seam，
  让 used-ring/combined cause 在设备 ACK 和 telemetry 后调用 axnet 固定 publish+wake；
  config-only、unknown-only、spurious 不发布 RX 事件。WHY 是 descriptor 只能在 task
  context 推进；HOW 是扩展单调 snapshot，包含 lifecycle、ISR publish、task、reap/
  refill、budget/yield、Router wait/wake、fault、last error 和 critical-section restore
  violation，并在 `AtomicWaker::wake()` 前后检查 IRQ 仍 disabled；EXPECTED 是 ISR
  无 Service/queue/descriptor/smoltcp 访问，PLIC EOI 仍在 handler 返回后。先扩展
  MS03/MS04 host harness 为 RED，再实现 GREEN。若 ISR 需要锁 Service、调用 receive
  或 config-only 伪造 completion，停止。

- [ ] 6.2 新增 `tests/ms04_rx_probe.c`、host RX burst stimulus 及 Makefile targets；
  probe 固定提供 snapshot、idle、single software-nudge、RX burst/fairness 模式，
  host stimulus 只发流量，不驱动 QEMU console。WHY 是运行时要分别观察 IRQ wake、
  task descriptor 进度、budget yield、spurious/no-work、守恒和 socket 回归；HOW 是
  每个模式输出固定 PRE/POST/DELTA/PASS/FAIL marker，quiet window 中不打印，nudge
  只 wake 不伪造 completion。EXPECTED 是 host C syntax、host stimulus self-test 和
  RISC-V static probe build 通过；`reaped_delta == refilled_delta`、fault/restore
  violation 为零是运行判据。若 probe reset counter、在 ISR 打印、自动输入 guest
  shell 或把部分 telemetry 当 PASS，停止。

## 7. 完成全部自动 Gate 与 Review

- [ ] 7.1 运行本地依赖 unit/check、`make host-test`、axnet full lib tests、UART
  unit/doctest、probe host syntax/stimulus self-test、各 manifest 与 workspace fmt
  check。WHY 是先关闭纯状态、接口、EVENT_IDX、critical-section 和兼容性缺口；
  EXPECTED 是全部退出 0，测试数量和关键输出记录在 Act Response。任何产品编译、
  assertion、source 或格式失败都停止，不能转入任务 8。若某命令最终仅因 R44
  明确的 sandbox 能力拒绝失败，记录原命令、退出码和最早失败层为
  `ENV-BLOCKED`，继续其余自动 Gate，并把同一命令加入 8.1。

- [ ] 7.2 依次运行 D1 async-UART compile check、`make LOG=info build`、MS04 guest
  probe static build，并确认 QEMU 镜像存在且记录 size/hash。WHY 是 critical-section
  是跨平台共享实现，QEMU 手测前必须先尝试所有自动 target 产物；EXPECTED 是命令
  退出 0、source/dependency audit 确认未关闭 EVENT_IDX，且产物可供手工批次使用；
  实际协商结果由 8.2 串口日志确认。产品诊断立即停止；仅
  R44 `ENV-BLOCKED` 可按 7.1 规则延后。不得以归档镜像或旧 Evidence 替代本轮产物。

- [ ] 7.3 执行 specs-vs-code、完整 code diff 和 full diff review，运行
  `openspec validate ms04-qemu-async-rx-queue-baseline --strict`、references strict
  validation、`git diff --check` 与结构化 source assertions；创建
  对应自动 Gate iteration 的 Evidence 索引，并写入已完成的
  environment/commands/build/hash
  见证或待用户复跑的 `ENV-BLOCKED` 清单。WHY 是用户手工前必须证明没有 Missing、
  未批准 Simplified、TBD 或未解决 Critical/Important finding；EXPECTED 是自动
  Gate 全 PASS 或只有可定位的环境交接，Evidence 索引列出每个最终文件和通过条件。
  任一产品 Gate、追踪或 review 缺口必须停止，不得请求任务 8。

## 8. 最终独立 iteration 的用户手工批次

- [ ] 8.1 仅在 1-7 的产品 Gate 全部通过后，在最终独立 manual iteration 中由用户
  在 sandbox 外复跑 7.1/7.2 记录的 `ENV-BLOCKED` 原命令；若没有环境阻塞则明确
  记录 None。WHY 是 R44 要求
  环境能力问题由用户边界完成，但不能掩盖产品失败；HOW 是保存环境差异、完整输出、
  最终退出码、镜像/probe size 与 SHA-256 到该 manual iteration 的 Evidence；
  EXPECTED 是
  每项最终 PASS 后才进入 8.2。出现 Rust/C/link/test/source 错误、中断、缺日志或
  缺产物时本任务保持未完成并停止。

- [ ] 8.2 用户按 R44 在终端中手工提供 guest payload、启动单 hart/单
  VirtIO-MMIO NIC QEMU，并逐条运行 MS04 `idle`、`nudge`、`burst/fairness` 与
  snapshot 模式，再运行 MS03 IRQ/UART、MS02 TCP/UDP 和 MS01 socket 回归；host
  burst 工具只负责流量刺激，不输入 guest shell。WHY 是最终证明 IRQ 唤醒、唯一
  task、budget/yield、no-work 不 busy-loop、descriptor 守恒和兼容性；EXPECTED 是
  `reaped_delta == refilled_delta`、budget exhaustion 与 yield 在 burst 中可见、
  idle/nudge 有界、fault/restore violation 为零、MS01/MS02/MS03 全部通过。保存
  `environment.txt`、`commands.txt`、`artifacts.sha256`、`build.log`、完整
  `qemu-serial.log`、`ms04-probe.log`、`ms03-regression.log`、
  `ms01-ms02-regression.log` 和 README 判定。任何中断、文件缺失、旧日志复用或范围
  超出单 hart VirtIO-MMIO 的声明都不能计为 MS04 通过。
