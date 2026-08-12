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
| 006 | iteration 005 Review 修复 + T6.1 ISR publish/wake 与 telemetry | No |
| 007 | iteration 006 Review 修复 + T6.2 probe/stimulus 与自动构建入口 | No |
| 008 | iteration 007 Review 修复 + T7 全量自动 Gate、diff Review 与 Evidence 准备 | No |
| 009 | iteration 008 Evidence Review 修复 + T8 sandbox 复跑和 QEMU runtime 手测 | Yes, user-only |

只有当前一轮通过 Plan Review 后才创建下一轮；编号是当前计划顺序，不预先生成空
iteration。若 Review 发现可控的小粒度问题，修复并入下一原定 iteration；只有无法安全
合并且会阻塞后续工作的缺口才单独拆轮，后续编号顺延。最终一轮
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

- [x] 3.1 新增可由 `tests/ms04-async-rx-host-harness.rs` 引入的 kernel 纯
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
  <!-- 2026-08-12 iteration 007 Plan Review：相同 D1 命令在当前 HEAD
  `e0fac50ce01527a1c5dea83c36c37616a1a92590` 完整退出 0，并生成新鲜 ELF/bin；结合
  已通过的 policy/host/UART/QEMU compile 见证，本项现已闭合。T7.2 仍会复跑 D1，
  但不再需要产品修复或 waiver。 -->

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

- [x] 5.2R 关闭 iteration 005 Review 的测试隔离、budget 边界见证和定向 warning。
  把 start-once/duplicate 的决策抽成可注入 lifecycle/spawn seam，让测试使用局部
  `RxLifecycle` 和 counter，不再永久推进生产 `RX_LIFECYCLE`；增加 Future 层恰好 31
  completions 后 empty 的直接见证，并把仅产品 spawn 使用的 `ToOwned` import 限定到
  `cfg(not(test))`。WHY 是当前 100×并发 gate 虽通过，但全局 lifecycle 已被测试永久
  留在 Spawned，且 Act 对 0/1/31/32/33 的覆盖声明缺少 31 的直接证据；HOW 是不增加
  test-only reset、不放宽单调状态机、不触碰生产启动语义。EXPECTED 是任意测试顺序
  下 global lifecycle 保持初始状态，31 路径精确执行 31 次进度加一次 empty 检查并
  rearm，axnet test build 不再产生 `ToOwned` warning。若修复需要重置生产 static、
  第二个 task/waker 或改变 budget=32，停止。
  <!-- 2026-08-11 Act：`start_with` crate-private seam + `spawn_rx_task` test binding；
  `future_31_completions_then_empty_registers_once`；`#[cfg(not(test))] ToOwned`；
  axnet 101 tests + 100×16-thread GREEN。 -->

## 6. 接入最小 ISR 与运行观测

- [x] 6.1 修改 `kernel/src/drivers/virtio_net_irq.rs` 及其 pure logic/snapshot seam，
  让 used-ring/combined cause 在设备 ACK 和 telemetry 后调用 axnet 固定 publish+wake；
  config-only、unknown-only、spurious 不发布 RX 事件。WHY 是 descriptor 只能在 task
  context 推进；HOW 是扩展单调 snapshot，包含 lifecycle、ISR publish、task、reap/
  refill、budget/yield、Router wait/wake、fault、last error 和 critical-section restore
  violation，并在 `AtomicWaker::wake()` 前后检查 IRQ 仍 disabled；EXPECTED 是 ISR
  无 Service/queue/descriptor/smoltcp 访问，PLIC EOI 仍在 handler 返回后。先扩展
  MS03/MS04 host harness 为 RED，再实现 GREEN。生产 handler 必须把 raw low byte 交给
  classifier/telemetry，只 ACK known bits，避免把 unknown-only 错记为 spurious；IRQ 注册
  成功后才调用唯一 start entry。append `IrqSnapshot` 时同轮更新现存
  `tests/ms03_irq_probe.c` 的结构与打印，保持 ioctl producer/consumer 尺寸一致。若 ISR
  需要锁 Service、调用 receive、config-only 伪造 completion，或 Rust/C snapshot 尺寸
  分叉，停止。
  <!-- 2026-08-11 Act：axnet `publish_rx_event` + `rx_snapshot` 固定入口；handler raw
  classification + `ack_mask`/`should_publish_rx` seam + record→ACK→publish 顺序 +
  IRQ enabled 检查 + restore violation；register-before-start 唯一 start；
  `IrqSnapshot` 追加 17 字段 + Rust size/offset tests + `ms03_irq_probe.c` 同步；
  MS03 host harness 24 tests、MS04 host harness 9 tests（含 ISR contract source guard）
  GREEN。T6.2 probe/stimulus 留待下一轮。 -->

- [x] 6.1R 关闭 iteration 006 Review 的 telemetry、IRQ witness 和 snapshot ABI 缺口。
  active suppress、completion-query 和 receive/recycle fault 每次只增加一次 fault，并保留
  原始 stage/code；missing Service 记录 PREFLIGHT/BadState。snapshot 对 lifecycle/owner
  使用同一次状态观察，并让 last-error stage/code 作为一个一致 pair 发布；同时记录
  IRQ enabled-on-entry，补强 handler guard 对 raw record 参数和 registration-failure
  branch return 的见证，清理本轮新增的 3 个 test warning。WHY 是当前 common fault
  分支会二次计数并把 stage 覆盖为 RECEIVE_RECYCLE，关联字段也可能被并发 snapshot
  撕裂。旧 `0x4e49_4431` 必须恢复为固定 8-field V1，新增 `0x4e49_4432` V2 承载 MS04
  字段；`ms03_irq_probe.c`、MS16 platform adapter 和既有 binary contract 继续使用 V1，
  不得再扩大原 command 的 kernel write。若只能依赖更新所有旧二进制、保留超长 V1
  write、用两次独立原子发布 last-error pair 或让 fault delta 大于一次，停止。
  <!-- 2026-08-12 Act：active suppress/completion-query/receive faults 精确单计数并保留
  stage/code；missing Service、单次 lifecycle 观察、packed last-error pair、四态 IRQ
  witness 与 handler guards GREEN。旧 command 固定为 64-byte V1，新 command 使用独立
  224-byte V2；V1 canary、全 offset 与 consumer inventory tests GREEN。 -->

- [x] 6.2 新增 `tests/ms04_rx_probe.c`、host RX burst stimulus 及 Makefile targets；
  probe 固定提供 snapshot、idle、single software-nudge、RX burst/fairness 模式，
  host stimulus 只发流量，不驱动 QEMU console。WHY 是运行时要分别观察 IRQ wake、
  task descriptor 进度、budget yield、spurious/no-work、守恒和 socket 回归；HOW 是
  V2 snapshot 用 `0x4e49_4432`，nudge 用独立 `0x4e49_4e31` software-wake command，
  只增加 software-nudge counter，不增加 generation、ISR publish/wake 或 completion。
  每个模式输出固定 PRE/POST/DELTA/PASS/FAIL marker，counter 才相减，lifecycle/owner/
  last-error 等 gauge 输出 POST 值；quiet window 中不打印。host UDP stimulus 在收到 guest
  registration 后发送带 sequence 的有界 burst，不驱动 console，并提供无 QEMU self-test。
  EXPECTED 是 host C syntax、stimulus self-test、Makefile targets 和 RISC-V static probe
  build 通过；运行判据仍为 `reaped_delta == refilled_delta`、fault/restore/IRQ-entry
  violation 为零。若 probe reset counter、复用 ISR publisher 做 nudge、自动输入 guest
  shell、把 gauge 当 delta 或把部分 telemetry 当 PASS，停止。
  <!-- 2026-08-12 Act：独立 software-nudge、V2 guest probe 四模式、两阶段有界 UDP
  stimulus、6 个 host decision tests、strict C11 与 Makefile 入口完成。109 个 axnet tests
  和 100×16-thread stress、host/upstream/kernel/build gates GREEN；RISC-V static probe
  build 在受限沙箱因 SIGSYS/Bad system call 按 R44 记为 ENV-BLOCKED，未用旧 artifact
  代替。本轮未运行 QEMU、未修改 rootfs、未创建 Evidence。 -->

- [x] 6.2R 关闭 iteration 007 Review 的 probe 判定、deadline、marker 与真实 loopback
  缺口。四模式必须在 POST 同时要求 boot-history `fault`、`restore_violation` 和
  `irq_enabled_entry` 绝对值为零；idle 拒绝 ISR/software/descriptor/budget/yield/
  backpressure 进度，nudge 除 software `+1`、task `+1`、empty `+1` 外拒绝所有进度。
  稳定快照先判 deadline，再接受相等 progress；已识别 mode 恰好输出一个终态 marker，
  只在数据实际存在时输出 PRE/POST/DELTA。host 工具保留纯协议 self-test，并新增有界
  real-loopback self-test；当前 sandbox 若以 EPERM/SIGSYS 拒绝，只能按 R44 记录并交给
  8.1。RED 必须通过 C decision mutations 复现“旧 violation 被 delta 掩盖”、idle/nudge
  漏检和 equal-after-deadline；GREEN 为 mutation 全拒绝、strict C11、host-test 和两个
  stimulus self-test 的可执行部分通过。不得扩大固定 V2、重置 counter、伪造 snapshot、
  把缺失 PRE/POST 打印为零或让 self-test 驱动 QEMU console。
  <!-- 2026-08-12 Act：absolute safety、idle/nudge exact matrix、deadline-first stable
  snapshot 和 centralized terminal marker 已由 10 个 C decision tests 与 14 个 host
  harness tests 覆盖。纯协议 self-test PASS；真实 UDP loopback 在 socket 创建处 EPERM，
  按 R44 原命令交给 8.1。 -->

## 7. 完成全部自动 Gate 与 Review

- [x] 7.1 运行本地依赖 unit/check、`make host-test`、axnet full lib tests与 100×并发、
  UART unit/doctest、probe host syntax/stimulus self-test，以及 kernel/axnet manifest fmt
  和 change-owned adapter/queue 文件的定向 rustfmt。WHY 是先关闭纯状态、接口、
  EVENT_IDX、critical-section 和兼容性缺口；全 manifest fmt 会批量重排未修改的 vendor
  snapshot，已由 iteration 007 Review 判为无效 Gate，禁止借 T7 清理该范围；
  EXPECTED 是全部退出 0，测试数量和关键输出记录在 Act Response。任何产品编译、
  assertion、source 或格式失败都停止，不能转入任务 8。若某命令最终仅因 R44
  明确的 sandbox 能力拒绝失败，记录原命令、退出码和最早失败层为
  `ENV-BLOCKED`，继续其余自动 Gate，并把同一命令加入 8.1。

- [x] 7.2 依次运行 D1 async-UART compile check、`make LOG=info build`、MS04 guest
  probe static build，并确认 QEMU 镜像存在且记录 size/hash。WHY 是 critical-section
  是跨平台共享实现，QEMU 手测前必须先尝试所有自动 target 产物；EXPECTED 是命令
  退出 0、source/dependency audit 确认未关闭 EVENT_IDX，且产物可供手工批次使用；
  实际协商结果由 8.2 串口日志确认。产品诊断立即停止；仅
  R44 `ENV-BLOCKED` 可按 7.1 规则延后。不得以归档镜像或旧 Evidence 替代本轮产物。

- [x] 7.3 执行 specs-vs-code、完整 code diff 和 full diff review，运行
  `openspec validate ms04-qemu-async-rx-queue-baseline --strict`、references strict
  validation、`git diff --check` 与结构化 source assertions；创建
  对应自动 Gate iteration 的 Evidence 索引，并写入已完成的
  environment/commands/build/hash
  见证或待用户复跑的 `ENV-BLOCKED` 清单。WHY 是用户手工前必须证明没有 Missing、
  未批准 Simplified、TBD 或未解决 Critical/Important finding；EXPECTED 是自动
  Gate 全 PASS 或只有可定位的环境交接，Evidence 索引列出每个最终文件和通过条件。
  任一产品 Gate、追踪或 review 缺口必须停止，不得请求任务 8。
  <!-- 2026-08-12 Act：自动 Gate、D1/QEMU target build、source/dependency audit、
  full-range Review 与 required Evidence 完成。D1/QEMU artifact 已记录 fresh hash；
  static probe compiler 在 SIGSYS 处按 R44 交给 8.1。未运行 QEMU。 -->

- [ ] 7.3R 在最终 iteration 的 Evidence 中补充 iteration 008 revision provenance 和
  raw-log whitespace Gate 说明。WHY 是 008 的 `environment.txt` 记录采集 HEAD
  `e0fac50`，而 README/Act Response 使用 `78e1f7a` 作为 Act HEAD；同时 staged
  `automatic-gates.log` 保留 ANSI/CRLF/终端行尾空格，使不排除 raw Evidence 的
  `git diff --cached --check` 与 full-range `--check` 退出 2。HOW 是保留 008 原始
  Evidence 不改写，在 009 索引逐项记录采集 HEAD、Act 基线、最终 Review revision 和
  worktree/index 层；源码、测试、脚本与 OpenSpec Markdown 使用排除 raw Evidence 的
  路径限定 whitespace check，raw logs 单独检查存在、非空、hash 和时间范围。EXPECTED
  是不存在矛盾的 provenance 声明，所有非 raw-Evidence diff check 退出 0。不得删除、
  截断或“清洗”008 原始日志来制造通过，也不得把 raw-log 空格当作产品源码失败。

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
  当前 MS04 probe 的 burst/fairness 入口是无额外参数的 `ms04_rx_probe burst`，host
  使用 `scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556`；不得沿用 iteration
  000 的旧 `burst 256` 或 `tests/ms04_rx_burst.py` 示例。MS02 guest service 必须观察
  两次独立 TCP `MS02_TCP_PASS` 和一次 UDP PASS 后得到 `MS02_COMPLETE tcp=2 udp=1`。
