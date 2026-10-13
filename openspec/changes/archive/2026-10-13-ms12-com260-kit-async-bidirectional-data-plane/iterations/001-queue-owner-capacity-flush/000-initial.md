# Iteration 001 / Cycle 000：K3 接入唯一 queue owner 并闭合容量/flush

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T3（2.1）、T4（2.2）、T5（2.3）。
- Acceptance gaps：None（首轮）。
- Repair items：None。
- Inherited scope：2026-10-06 批准、2026-10-08 修订的 R1–R7/S1–S20 中本轮映射（R1/R2/R3/R5/R6/R7 相关行）；Iteration 000 accepted 基线（`k3-async-net` feature、Gmac async 通知面、ISR 核心、寄存器裁决）；RAM-only、单 hart、既有 DMA 安全与公共规则。
- Excluded scope：T6/T7（stack runner、SocketOps 载荷、四协议、quiet、产品侧 snapshot 收尾）、MS13 恢复语义、SMP、性能、UART/shell/rootfs 集成、全局状态同步与经验产物。

**Objective**

既有 axnet Service 以生产 K3 adapter 为唯一 NIC：queue owner 在真实 hart 上 Active，32/32/32 stage budget、64/64 软件 slot、16/16 descriptor 与 CPU 池账本全部成立；target-scoped C4 flush 与两级 TX Full→release→恢复在 host fixture 与真板诊断载荷下可观测。

**Background**

Parent Iteration：`../../iterations/000-initial/001-replan.md`（Review Result: accepted，2026-10-12）。按 Map 展开本 Iteration；本轮的全部代码事实以该 Cycle 最终 Act Response 的改动为 Current Baseline，不再重复其调查。

**Investigation Facts**

Current Baseline：2026-10-12，`k3` 分支，HEAD `cdf94713`；工作树含未提交的 T1/T2 产品与测试改动（本轮直接在其上叠加，不回退、不重排）。可采信结论（来源：001-replan Act Response 与 accepted Review）：普通 axnet event 测试 GREEN、gmac harness 31/31、polling-net-host 17/17、`make k3-async`（含 FIT 390688 B）/`make k3`/`make k3-shell`/QEMU build exit 0、模式互斥守卫生效、V4 run3 板测 A1–A5 全过。修订现场，不作为匹配条件。

Current-State Evidence / Code and Critical Path（本轮新涉及表面，均已在当前实码核对）：

| 路径/符号 | 当前事实与本轮责任 |
|---|---|
| `crates/axdriver_net/src/lib.rs::{NetDriverOps, NetTxQueue, NetQueueControl, TxCookie, TxResourceLedger}` | 待实现契约。`NetDriverOps`：`mac_address/can_transmit/can_receive/rx_queue_size/tx_queue_size/queue_control()->Option<&mut dyn NetQueueControl>/tx_queue()->Option<&mut dyn NetTxQueue>/recovery_control()（默认None）/recycle_rx_buffer/recycle_tx_buffers/transmit/receive/alloc_tx_buffer(size)`。`NetTxQueue`：`submit_tx(NetBufPtr, TxCookie)->DevResult`（pre-accept 错误不借走 buffer；post-accept 稳定 fault 保留 buffer）、`reclaim_tx()->DevResult<Option<TxCookie>>`（每次至多一个）、`tx_resource_ledger()->Option<TxResourceLedger>`（默认 None，能观测的驱动必须覆写；`completions_seen` 与 reclaim 分开）。`NetQueueControl`：默认实现仅 RX，双向驱动必须覆写 `completion_pending/suppress_notify/arm_notify_and_check`；`has_rx_completion/suppress_rx_notify/arm_rx_notify_and_check` 必须实现 |
| `axdriver_base 0.1.4-preview.3`（registry）`BaseDriverOps` | `device_name/device_type/irq_num()->Option<usize>`（默认 None）。adapter 返回 `Some(133)`；`EthernetDevice::requires_polling()=irq_num().is_none()`，故 K3 adapter 激活后无 polling fallback |
| `crates/axdriver_net/src/net_buf.rs::{NetBufPool, NetBufPtr, NetBuf}` | `NetBufPool::new(capacity, buf_len)->DevResult<Arc<_>>`、`alloc()->Option<NetBuf>`、`alloc_boxed()`；`NetBuf::{packet,packet_mut,packet_len,into_buf_ptr}`。CPU copy pool 的原生载体，不是 DMA backing |
| `axdriver 0.3.0-preview.2` `structs/mod.rs` | `AxDeviceContainer::from_one(dev)`；dyn 模式下 `AxNetDevice = Box<dyn NetDriverOps>`（axnet `device/tests.rs` 以 `EthernetDevice::new("eth0".into(), Box::new(nic), TEST_IP)` 构造即为该形态） |
| `crates/axnet/src/lib.rs::{init_network, flush, diagnostic_control, poll_interfaces}` | `init_network(AxDeviceContainer<AxNetDevice>)`（lib.rs:93）：Router+loopback+eth0→`Service::new(router, target_dev)`→全局 `SERVICE`，**spawn-free**——由 kernel 侧负责启动 owner。`flush()->DevResult<FlushFuture>`（lib.rs:205）与 `diagnostic_control(op, lease_ms)`（op：HoldTxSubmit=1/HoldTxReclaim=2/Release=3，lease 1..=2000ms）目前都在 `#[cfg(feature="qemu-diagnostics")]` 下，本轮移到中性 `queue-diagnostics`。`poll_interfaces()` 是 polling fallback，K3 数据面禁止调用 |
| `crates/axnet/src/service.rs::{Service, diag_*}` | `Service::new(router, target_dev: Option<usize>)`。hold/lease 状态 `diag_hold_mode/diag_lease_expiry_nanos/diag_auto_release_failure` 在 `#[cfg(feature="qemu-diagnostics")]`（service.rs:207–214），`diag_test_clock` 在 `cfg(all(test, qemu-diagnostics))`；`diag_hold_tick`/`diagnostic_control_shared` 同 feature。本轮仅改 cfg 指向，不改语义/数值/快照 ABI |
| `crates/axnet/src/diag.rs` | HoldMode/lease 上限 2000ms/`DiagTestClock` fake-clock fixture（attach 后 `diag_hold_tick` 与 RX future 的 lease deadline 读 fixture clock）。cfg 随迁；QEMU 专有的 recovery/reset request、link 日志、migration、userspace ioctl 保持仅 `qemu-diagnostics` |
| `crates/axnet/src/async_rx.rs` | 三 budget 常量 `RX_BUDGET/RECLAIM_BUDGET/SUBMIT_BUDGET = 32`（224–230）；`start_rx_task_affinity(mask)->Result<AxTaskRef, StartError>`（3082）；`rx_snapshot()`（840，lifecycle 0 Polling/1 Spawned/2 Active/3 Faulted/4 Unavailable、owner 0 polling/1 async）；`owner_hart_tuple()`（847）；`publish_queue_event/software_nudge` 既有；`mod tests`（3344 起）为既有 local fixture（`start_with/start_affinity_with_checked`、fixture clock、resident-recovery fixture）。budget 31/32/33 与 slot 64/65 的既有见证在此，本轮不改其语义 |
| `crates/axnet/src/device/{ethernet.rs, tests.rs, fixed_queue.rs}` | `EthernetDevice::new(name, inner, ip)`；`rx_slots/tx_slots: FixedFrameQueue<64>`、`tx_tickets: TicketTracker`；完整帧 slot enqueue 走 `emit_frame`（`TxMode::DormantSlots` 下写固定 TX slot 并返回 checked ticket），`send` 的 pending 路径为 `pending_packets.enqueue`；recovery/link hold 拒绝 pre-submit enqueue。`device/tests.rs` 的 `FakeNic/FakeQueueControl/FakeTxQueue → EthernetDevice::new → Service::new(router, Some(eth))`（tests.rs:350）是 Service 级 local fixture 的既有形态，本轮 T4 测试照此注入生产 adapter |
| `crates/axnet/src/flush.rs::{FlushFuture}` | 既有 target-scoped C4 flush（目标 slot+device live 集合、单 waiter、checked ticket）。本轮只迁 cfg 与补 K3 可达性，不重写 |
| `crates/axnet/Cargo.toml [features]` | 现仅 `vsock`、`qemu-diagnostics`。本轮新增 `queue-diagnostics = []` 并令 `qemu-diagnostics = ["queue-diagnostics"]`；hold/lease/flush/最小帧入口的 cfg 全部指向 `queue-diagnostics` |
| `kernel/src/platform/k3_gmac/mod.rs` + `tests/k3-gmac-host-harness.rs` | harness 以 `#[path]` include `k3_gmac` 整模块。**adapter 子模块必须以 kernel cargo feature 门控**（镜像 `k3-polling-net-host` 对 `k3_gmac::polling_net` 的 feature 镜像模式：harness 单文件 rustc 编译不启用任何 cargo feature，`#[cfg(feature=...)]` 模块自动排除，31 项保持 GREEN）；否则 include 断裂 |
| `kernel/src/platform/k3_async_net.rs` | T2 交付：`IrqAccess` seam、`gmac133_handler`、`KNOWN_STATUS_MASK`（含 bit10）、board `run_k3_async_net_session`（raw 会话）。T4 在同文件加 Service 启动路径；按 Migration Plan，raw 会话仅保留为诊断路径，不与网络 owner 并存，普通 `k3-async` 构建进入 Service 路径 |
| `kernel/src/platform/k3_gmac/{mod,regs,desc}.rs`（T2 面） | `Gmac::new_async/arm_notify(tx)/mask_notify/check_completions->CompletionHint{rx_ready,tx_ready}/latch_fault/tx_inflight/diag_head_words`；IOC 位（RX w3 bit30 / TX des2 bit31）、OWN 最后写、`DMA_IRQ_EN_DEFAULT(0xD041)/RX_ONLY`。adapter 的 `NetQueueControl` 直接映射这些原语；`check_completions` 只读不回收，正好承载 `completion_pending` |
| `tests/k3-polling-net-host/` | 既有独立 host 包模式（workspace 之外、path 依赖、feature 镜像 kernel gate）。`tests/k3-async-net-host/` 照此建立：path-include 生产 adapter 与 `k3_gmac`，依赖本仓 `axdriver_net`（host 下 critical-section 用 std 实现），自带最小 FakeEnv/TestBacking（测试设施，不复制生产实现） |
| `kernel/Cargo.toml`、根 `Cargo.toml`、`Makefile` | `k3-async-net` feature 与 `make k3-async` 已存在。T5 新增 `k3-net-diagnostics`（kernel/根 feature，启用 axnet `queue-diagnostics` 并承载诊断载荷）与 `make k3-async-probe`（同 k3-async 参数 + 诊断 feature + 原 k3-fit）；普通 `k3-async` 不含控制入口 |

**Implementation Guidance**

顺序 T3→T4→T5（Map 依赖）。T3 的 adapter 泛型于 backing/env（Gmac 方法本就 `&mut E: GmacEnv` 逐调用传入；board 用 `K3Env`+`BoardBacking`，host 用包内 FakeEnv+TestBacking，参照 MS11 harness 的 arena 泄漏模式）；adapter 内持有 env 的具体泛型形状留给 Act。T4 的板级启动序严格按 design D4：stage10 正常结束 → mask 60/133/277 → unregister 133 观察 handler → 通知关闭下建 Gmac/adapter → 注册 `gmac133_handler` → `init_network(AxDeviceContainer::from_one(...))` → `start_rx_task_affinity(axtask::schedulable_cpu_mask())` → 前台仅 `yield_now`+`rx_snapshot` 在 5s 绝对上限内等 Active（Faulted/Unavailable/超时即停，不 nudge、不调 poll_interfaces）→ Active 后按 feature 进入诊断载荷或 park；通知 arm 由 owner 的 `arm_notify_and_check` 协议负责，启动序不再预置 arm。T5 先迁 cfg（三面测试证明无行为变化），再加最小帧入口与 K3 诊断载荷。诊断 enqueue 复用 `EthernetDevice` 既有完整帧 slot enqueue/checked ticket 路径（`emit_frame` 的 dormant 分支所走），Service 锁内一帧、解锁后 `software_nudge`；辅助函数拆分留 Act。

**Task Contracts**

### T3：NetDriver traits 与 ownership（adapter）

- Requirement/Scenario：R1/R3/R5/R7；S2–S4、S7–S9、S16–S18、S20。
- Depends on：Iteration 000 accepted（已满足）。
- Targets：新 `kernel/src/platform/k3_gmac/adapter.rs`（feature 门控子模块，见 Investigation Facts 的 harness 约束）；`kernel/src/platform/k3_gmac/mod.rs` 的模块声明与再导出；新 `tests/k3-async-net-host/`（Cargo.toml + 测试，path-includes 生产代码，使用本仓 axdriver_net，不复制生产实现）。
- Current behavior：无 trait adapter；Gmac async 面（IOC/arm/check/latch）与 `IrqAccess` ISR 已存在但只被 raw 会话使用；axnet 的 `EthernetDevice` 在 `irq_num()==None` 时走 polling 模式。
- Required behavior：`K3NetAdapter` 持唯一 `Gmac`（泛型 backing/env）、固定 `NetBufPool`（TX 16×2048、RX 1×2048，CPU copy 非 DMA backing）、16 项 TX cookie/buffer→实际 descriptor slot owner 表、≥1514 RX scratch。实现 `BaseDriverOps`（`irq_num=Some(133)`）、`NetDriverOps`（`queue_control/tx_queue` 返回 Some；`recovery_control=None`；queue size 如实报硬件 16/16）、`NetQueueControl`（`completion_pending` 映射 `check_completions` 的 CompletionHint，TX 仅有 live owner 时可报；`suppress_notify→mask_notify`；`arm_notify_and_check→arm_notify+check_completions` 序）、`NetTxQueue`（`submit_tx` 走 `try_submit`：pre-accept Full→`Again`、长度/地址拒绝→`InvalidParam`，成功后 buffer+cookie 绑定实际 slot；`reclaim_tx` 单次回收：Completed 归还 cookie 一次，LocalError 释放已 terminal CPU buffer 但返回 `Io` 并 fault 所属 ticket/flush，TerminalTimeout 保留 descriptor/owner 返回 `TimedOut`，duplicate/缺 cookie/错 epoch→`BadState`；`tx_resource_ledger` 从 pool 实际 owner 与 Gmac descriptor 状态取数，`completions_seen` 计首次同步见到的合法 OWN=0，与 reclaim 分开）。`alloc_tx_buffer` 检查 14..=1514。OWN 发布成功后的调用错误按 post-accept fault 保留（不因返回 Err 误释放），分界以 Gmac 实际发布状态为准。`receive` 每次最多 16 个 descriptor：合法帧 copy→refill→NetBufPtr；分类 drop 有界继续；无工作 `Again`；坏帧非 fatal。进入 fault 后拒绝新 submit、通知关闭、合法软件资源可释放、device-owned backing 全保留。
- Preserve：MS11 DMA/tail/cache 契约与 T2 裁决（0xD041、bit10、OWN-last、IOC 位语义）；已发布 backing 保留；`NetDriverOps/NetTxQueue/NetQueueControl` 公共 trait 及 ABI；既有 31 项 gmac harness 与 17 项 polling-net-host 行为。
- Forbidden：zero-copy、新公共 queue 协议、自动 reset/恢复 control、动态扩容、修改 axdriver/axnet 公共 trait、修改参考仓库、身份型判定器。
- Test witness：新包 `tests/k3-async-net-host/` 以 FakeEnv/TestBacking 驱动生产 adapter，先观察 RED（模块缺失/编译失败或行为断言失败）再 GREEN。覆盖：长度 14/60/1514/1515（1515 拒绝）；pool/descriptor 16/17（第 17 次 Full→Again，ledger available/inflight 守恒）；wrap（slot 15→0）；单次回收不重复；postpublish 错误保留 backing；timeout→TimedOut 且 owner 保留；`completion_pending` 空 TX ring/零 inflight 必为 NONE；fault 后 submit 拒绝且查询传播。
- GREEN condition：上述原生 Cargo 测试全部通过；`tests/k3-gmac-host-harness.rs` 重编译仍 31/31（adapter 门控不破坏 include）；MS11 回归 17/17。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --offline`（新增 package lock 后加 `--locked`）；`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/ms12-k3-gmac-host && /tmp/ms12-k3-gmac-host --test-threads=1`；`cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline`。
- Stop when：需要改公共 trait 语义、CPU/DMA owner 分界或真实容量 16 失效，返回 Plan。

### T4：唯一 queue 与有界双向推进（Service 接入）

- Requirement/Scenario：R1/R2/R3/R5/R7；S2–S9、S12、S16–S17、S20。
- Depends on：T3。
- Targets：`kernel/src/platform/k3_async_net.rs`（Service 启动路径：D4 序，见 Implementation Guidance）；`kernel/src/platform/k3.rs` 异步分支（stage10 后进入 Service 路径）；`crates/axnet/src/async_rx.rs` 既有 local fixture 的最小注入点（若 pub 面已够则零改动）；`tests/k3-async-net-host/` 增加 Service 级测试。
- Current behavior：K3 异步分支进入 raw 会话（T2 形态）；axnet Service 未在 K3 构造；queue owner 未启动。
- Required behavior：普通 `k3-async-net` 构建沿 D4 序进入 `init_network(from_one(Box::new(adapter)))` + `start_rx_task_affinity(schedulable_cpu_mask())`；初始化等待仅 `yield_now`+`rx_snapshot`、5s 绝对上限，Active 才继续，Faulted/Unavailable/超时停（输出 rx_snapshot 与有限快照）；实际运行 hart 由 `owner_hart_tuple()` 读取。数据面激活后不保留初始化等待循环、不调 `poll_interfaces`、无第二 owner/第二 executor；旧 polling stage11–13 与 raw 会话不与 owner 并存（raw 会话仅诊断路径）。任何准备失败不启动第二 owner；Active 后 fault 不回 polling。32/32/32 budget 与 64/64 slot、16/16 descriptor 契约保持不变。
- Preserve：`QueueEvent` 等待协议与 `IrqAccess` ISR（T2 面）；Full 保留帧、slot 完整帧、failure-atomic start；T2 全部已验收行为（ISR 计数、寄存器裁决、backing 保留）。
- Forbidden：第二 owner/executor、periodic polling 补最后 wake、扩大 MS17/UART 面、改 budget/slot 容量、改公共 trait。
- Test witness：`tests/k3-async-net-host/` 以生产 adapter（FakeEnv）+ local Service fixture（`EthernetDevice::new + Service::new` 形态）新增 RED→GREEN：激活失败/重复 start 拒绝、stage 31/32/33（refill 后继续产生 completion 的既有 fixture 手法）、RX 64/65、TX Full 后恢复、通知窗口（event-before/during/after-arm）、稳定 Faulted；既有 axnet 成功测试直接采信。
- GREEN condition：adapter 包与 axnet `--lib` 相关测试通过；`make k3-async` 成功；GREEN 必须是生产路径与原生结果，不是 source 字符串 guard 或 test-only 重新实现。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`；`cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`（相关测试子集）；`make k3-async`。
- Stop when：Service/stack 抢 descriptor、fault 回到 Polling 或需改变 budget/slots，返回 Plan。

### T5：实际容量与 target-scoped C4 flush（queue-diagnostics）

- Requirement/Scenario：R2/R3/R5/R6/R7；S6–S12、S16–S20。
- Depends on：T4。
- Targets：`crates/axnet/Cargo.toml`（`queue-diagnostics` feature）；`crates/axnet/src/{lib,diag,service,async_rx,flush}.rs` 的 hold/lease/flush cfg 迁移与最小完整帧诊断入口；`EthernetDevice` 既有完整帧 enqueue/checked ticket 路径的复用点；kernel/根 `k3-net-diagnostics` feature；`make k3-async-probe`；K3 诊断载荷（`k3_async_net.rs` 内，非结果判定器）。
- Current behavior：hold/lease（`diagnostic_control`、Service `diag_*`、`DiagTestClock`）与 `flush()` 都在 `qemu-diagnostics` 下；K3 无容量/flush 板测载荷。
- Required behavior：hold/lease/flush 的 cfg 全部迁至 `queue-diagnostics`，`qemu-diagnostics` 蕴含之；QEMU 专有的 recovery/reset request、link 日志、migration、userspace ioctl 保持原 feature；原 QEMU API/数值/快照 ABI 不变。新增诊断入口：Service 锁内经目标 EthernetDevice 既有完整帧 slot enqueue 提交一帧（最多一帧/调用）、读取既有 ledger（adapter `tx_resource_ledger`、slot enqueue events、`rx_snapshot`），解锁后 `software_nudge`；不直接注入 ticket、不修改 descriptor、不伪造 completion。`k3-net-diagnostics`/`make k3-async-probe`（复用 k3-async 参数+k3-fit）承载 K3 载荷依次测 64/65 软件 slot 与 16/17 硬件资源；普通 `k3-async` 无控制入口。真板容量/flush 观察用 200ms hold（显式 release，lease<1000ms 不触 data deadline；auto-release 计数须为 0）。
- Preserve：QEMU 三面（ordinary/queue-diagnostics/qemu-diagnostics）原有测试语义与快照 ABI；sole owner、checked ticket、flush waiter 语义；正常 socket 路径不变。
- Forbidden：日志判定层、压力协议/CLI、身份字段、改变正常 socket 路径、board reset recovery、暂停 clock 或破坏 OWN。
- Test witness：axnet 三面测试（ordinary 无诊断面编译通过；queue-diagnostics 下 hold/lease/flush 既有测试迁移后 GREEN；qemu-diagnostics 蕴含面 GREEN）+ 生产 adapter flush 测试 RED→GREEN：目标 slot+device 集合、新 ticket、乱序/后续 ticket、empty、Busy（try_lock ResourceBusy）、drop、deadline 取消、FBE 稳定 fault；原 lease/flush 测试作保持 GREEN 见证。
- GREEN condition：三面原生测试通过；`make k3-async` 与 `make k3-async-probe` 成功；V4' 板测（操作者）达 A4 数字。
- Verification：`cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`（依次 default、`--features queue-diagnostics`、`--features qemu-diagnostics`）；`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`；`make k3-async`、`make k3-async-probe`；受影响构建回归（`make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`、`git diff --check`）；V4' 操作者板测（下）。
- Stop when：安全 200ms 窗内不能到达容量边界、需改变 deadline/容量、必须造 DMA 故障或新增控制框架；交 Plan，不隐去未达项。

**Invariants**

1. 单 boot hart、一个 Gmac owner、一个 queue owner、一个 executor；ISR 不碰 Service/descriptor（T2 不变量延续）。
2. 软件资源（pool/cookie/slot/ticket）与硬件资源（descriptor/backing）账本各自守恒；从实际 owner 状态取数，不从软件 slot 推导硬件账本。
3. post-accept 失败保留 device-owned backing；cancel/timeout 不改变 DMA 所有权；fault 后拒绝新提交且通知关闭。
4. K3 数据面不调用 `poll_interfaces`、不 periodic nudge；唤醒只来自 QueueEvent（ISR/software）、descriptor 重查与期限。
5. QEMU 诊断 ABI 与既有测试语义不因 cfg 迁移变化；ordinary 构建无诊断控制入口。
6. 原 `make k3` polling 路径、其他平台构建与已验收 shell/UART 行为不回归。

**Non-goals**

stack runner/SocketOps 载荷/四协议/quiet（Iteration 002）、产品侧 snapshot 收尾（T7）、runtime reset/link-flap 恢复（MS13）、SMP/IPI、性能、用户态网络工具、驱动框架抽离。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | 生产 adapter 经 FakeEnv host 见证：长度边界、pool/descriptor 16/17 守恒、wrap、单次回收、postpublish 保留、timeout、fault 传播、completion_pending 无假工作 | T3，R1/R3/R5，S2–S4/S7–S9/S16–S18 |
| A2 | local Service fixture + 生产 adapter：激活失败/重复拒绝、stage 31/32/33、RX 64/65、TX Full→恢复、通知窗口、稳定 Faulted；`make k3-async` 成功且无 polling fallback | T4，R2/R3/R5，S5–S7/S12/S16–S17/S20 |
| A3 | queue-diagnostics 三面测试通过；flush 目标集合/new ticket/乱序/empty/Busy/drop/deadline/FBE 见证 GREEN；QEMU 原面不回归 | T5，R2/R3/R5/R6，S6–S12/S16–S20 |
| A4 | 真板（操作者，`make k3-async-probe` 镜像）：200ms hold 下 TX 64→65 Full、driver 16→17 Full、release 后原帧继续并达 terminal、flush Pending→release→C4 成功、全部容量与 owner 账本复原、auto-release=0 | T5，R2/R3/R6，S7/S10–S12/S19 |
| A5 | 回归与现场：MS11 host 回归、gmac harness 31 项、受影响构建 exit 0、`git diff --check` exit 0、`openspec validate` valid；板测后外部 RESET 回原系统 | T3–T5，R7，S18–S20 |

**Verification**

V1（agent，T3/T4/T5）：命令见各 Task Contract 的 Verification 行；全部为原生 cargo/rustc/make，判定用退出码与原生输出，无判定器。axnet 相关子集按改动面选择，全量 `--lib` 在三 feature 面各跑一次。

V2（agent，顺序约束）：先完成 agent 可执行测试与全部构建，最后构建待板测镜像（`make k3-async-probe`），避免后续 `make k3` 覆盖 FIT（Migration Plan）。

V3（操作者，agent 项 PASS 后最近能力边界）：

1. 与 Iteration 000 V4 相同的装载流程（U-Boot 五行、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`bootm`），PC 192.168.137.1/24 直连。
2. 等诊断载荷窗口：观察 TX 软件容量 64→第 65 帧 Full、driver 容量 16→第 17 次 Full、200ms 后 release、原帧继续发送并 terminal/reclaim。
3. 观察 flush：hold 下 flush 等待 Pending，release 后 C4 成功；容量与 owner 账本复原行；auto-release=0。
4. 每步直接观察串口原生输出；执行人一句话报告数字与 RESET 回原系统结果；无截图/表格/hash 要求。

V4（失败处理）：任一容量边界不可达、账本不复原、flush 挂起或 reset 未回原系统时停止新增工作，按 phase/owner 输出快照；Act Response 记 blocked，未完成项不勾选。

**Requirements Traceability Matrix（本轮）**

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R2 | T4、T5 | owner_activation、stage_31_32_33、slot_64_65、dual_backlog、真板两级 Full | Covered |
| R3 | T3、T5 | pool_descriptor_16_17、complete_frame_reclaim、flush_target_set/new_ticket/cancel/busy/deadline | Covered |
| R5（本轮部分） | T3、T4、T5 | postpublish_retention、stage_timeout、waiter_drop、backing_drop_witness、stable_fault | Covered |
| R6（本轮部分） | T5 | 真板容量/flush 数字、native ledger/flush snapshot | Covered |
| R7（本轮部分） | T3、T4、T5 | 三面 feature 隔离、MS11/gmac harness 回归、受影响 build、RAM boot/reset | Covered |
| R1 | T3、T4 | completion_pending 无假工作、arm/recheck 窗口（Iteration 000 已验收 IRQ 面直接采信） | Covered |

R4 及 R4–R7 余下、R1 的协议部分属 Iteration 002，不在本轮声称。

**Gate 2 Readiness**

PASS：技术维度自检（调查/设计/契约/分轮/追踪/验证/Evidence）全部通过；2026-10-12 用户原话"批准"，计划获批，Status 转 ready。BLOCKED：None。WAIVED：None。

**Persisted Evidence**

- Mode: none
- native 命令、每项 ≤20 行决定性输出、退出码/操作者一句话结果写 Act Response；不建 Evidence 目录。

**Risks and Notes**

- adapter 泛型/env 注入的具体形状、诊断辅助函数拆分、包内 FakeEnv 的最小实现属非实质选择，留 Act。
- board 上 `start_rx_task_affinity` 首次在 K3 运行；若激活序列与 D4 冲突按 Stop when 返回 Plan，不在板上盲试第四次。
- 真板容量观察依赖 PC 对 64+ 帧的及时回应；若链路速率导致 200ms 窗内不可达，报告观察并交 Plan 裁决，不延长 lease。
- queue-diagnostics cfg 迁移触碰 axnet 多文件；三面测试为回归门，QEMU 恢复专用入口不得泄漏到 K3。
- VF2 既有失败仅按实际影响范围记录；已采信结论不重跑。

## Act Response

- Status: reported

**实际改动**

T3（adapter）：

- 新增 `kernel/src/platform/k3_gmac/adapter.rs`：`K3NetAdapter<B: ArenaBacking, E: GmacEnv>`（`core: spin::Mutex<Core>`，Core 持唯一 `Gmac`+env+16×2048 TX 池+1×2048 RX 暂存池+16 项 cookie→slot owner 表+quarantine+`completions_seen`）；实现 `BaseDriverOps`（`irq_num=Some(133)`）、`NetDriverOps`（queue 16/16、`queue_control/tx_queue=Some(self)`、`recovery_control=None`、`transmit=Unsupported` fail-loud、`recycle_tx_buffers` 无副作用）、`NetTxQueue`（submit 走 `try_submit`：Full→Again、长度/地址→InvalidParam、其余 pre-publish→Io，OWN 发布后错误按实际 `tx_inflight` 判 post-accept——quarantine 保留 buffer 并锁存稳定 TX fault；reclaim 单次回收：Completed 一次归还 cookie、LocalError 释放 terminal buffer 返回 Io、TerminalTimeout 保留 descriptor/owner 返回 Io、缺 cookie→BadState）、`NetQueueControl`（`completion_pending/arm_notify_and_check`→`check_completions` 映射、`suppress_notify`→`mask_notify`、arm 序=arm_notify+recheck）、`tx_resource_ledger` 从 owner 表+`Gmac::tx_inflight` 取数。本地 trait `AdapterEnv/AdapterBacking`（GmacEnv/ArenaBacking+Send+Sync）。
- `kernel/src/platform/k3_gmac/mod.rs`：`#[cfg(feature = "k3-async-net")] pub mod adapter;`（rustc harness 无 feature 编译时自动排除，31 项不受影响）。
- `kernel/Cargo.toml`：`axdriver_net = 0.1.4-preview.3` optional dep，纳入 `k3-async-net` feature。
- 新增 `tests/k3-async-net-host/`（workspace exclude；feature 镜像 `k3-async-net`+声明未启用的 `k3-polling-net`/`k3`；path-include k3_dma/mac_probe/phy_probe/k3_gmac；`src/fake.rs` 最小 FakeEnv（Arc<Mutex> 共享态可脚本）/LeakBacking/TrackingBacking/FakeWire/Board）。

T4（Service 接入）：

- `kernel/src/platform/k3_async_net.rs` board 模块新增 `run_k3_async_net_service`（D4 序：stage10 gate→mask 60/133/277→unregister 133→通知关闭下 `Gmac::new_async`+`init_rx`+`enable_tx`→`K3NetAdapter::new`→`arm_irq_access`→`register(gmac133_handler)`→`init_network(from_one)`→`start_rx_task_affinity(schedulable_cpu_mask())`→前台仅 `yield_now`+`rx_snapshot` 5s 绝对上限等 Active（Faulted/Unavailable/超时即停，不 nudge、不 poll_interfaces）→Active 后按 feature 进诊断载荷或 park）；`board::line` 提为 `pub(super)`。raw 会话 `run_k3_async_net_session` 原样保留为诊断专用路径，不再被调用。
- `kernel/src/platform/k3.rs`：k3-async-net 分支改调 `run_k3_async_net_service`。
- `tests/k3-async-net-host/` 增加 `tests/service_owner.rs`（生产 adapter+axnet pub API+host axtask 调度器：重复 start 拒绝、Active/owner=1、通知窗口 event-before-arm、RX 64/65 slot 背压不覆盖、稳定 Faulted 不回 polling）与依赖面（axnet/axdriver dyn/axtask(irq+multitask+test)/critical-section std/memory_addr+axklib mem_iomap stub+镜像 [patch.crates-io]）。

T5（queue-diagnostics）：

- `crates/axnet/Cargo.toml`：新增 `queue-diagnostics = []`，`qemu-diagnostics = ["queue-diagnostics"]`。
- cfg 迁移（仅 hold/lease/flush 面，共 79 个 gate）：`lib.rs`（mod diag、`diagnostic_control`、`flush`）、`service.rs`（diag 字段/init/`diag_*` 方法/lease 测试子模块）、`diag.rs`（整模块+文档）、`async_rx.rs`（`diagnostic_control_shared`、lease 字段/timer/`SleepUntil`、hold 读取、v3 lease 元组、`RxRxFuture` 全部初始化点、lease/hold/diag-control/flush 测试与 LedgerDevice 夹具及其 not-partner gate）。QEMU 专有（recovery/reset request、ROUND_COUNT/link 日志、RecoverySnapshotV4、migration、对应测试）保持 `qemu-diagnostics` 不动。
- 新增最小完整帧诊断入口：`device/mod.rs` `Device::diagnostic_enqueue_complete_frame`（默认 Unsupported）+ `ethernet.rs` 实现（复用 `emit_frame` dormant 路径：preflight+checked ticket+slot fill）+ `service.rs` `diagnostic_enqueue_target`（Service 锁内一帧+同 guard 读 v3 slot/tx ledger）+ `lib.rs` `diagnostic_enqueue_frame`/`DiagnosticEnqueueReport`（解锁后一次 `software_nudge`）。
- `kernel/src/platform/k3_async_net.rs` 新增 `k3_net_diagnostics` 载荷模块（`cfg(all(k3, k3-net-diagnostics, riscv64))`）：200ms reclaim hold 下 65 次 enqueue（64/65 软件 Full+16/17 硬件 Full 观察）→显式 release→账本复原等待→第二段 hold 内 flush Pending→显式 release→C4 成功→`auto-release=0` 终行；每相 10s 墙钟上限，只打印原生观测不判定。
- feature/构建：kernel+根 `k3-net-diagnostics`；`Makefile` 新增 `k3-async-probe`（同 k3-async 参数，SMP>1 拒绝）。

**与计划的偏差及原因**

1. **`TerminalTimeout`→`DevError::Io`（计划文字为 `TimedOut`）**：axdriver_base 0.1.4-preview.3 的 `DevError` 无 `TimedOut` 变体（registry 源核实）。行为要件全部保留：descriptor/owner 保留、slot 永不复用、不冒充成功、非 BadState（不计 ownership_invariant）；对不可恢复 K3 目标，任何 reclaim Err 同路径进入 Fault（`classify_fault` 已核实），错误值区分只余 telemetry 面。host 见证：`terminal_timeout_keeps_owner_and_never_reuses_the_slot`。
2. **host Service 级测试不含 socket 端到端 TX**：调查发现 queue owner round 只含 reclaim/RX-copy/TX-submit 三 stage，Router/dispatch 属 stack runner（Iteration 002 范围）；`EthernetDevice/Service` 为 axnet 私有模块，契约"local Service fixture（EthernetDevice::new+Service::new 形态）"以 axnet pub API（init_network+start_rx_task_affinity）承载。TX Full→恢复的 Service 层见证按契约由既有测试采信（axnet queue 面 506 项内 submit-Full 恢复）+ T3 adapter 16/17 wrap→reclaim→恢复直证 + T5 板测载荷将覆盖两级 Full→release→恢复。
3. **验证命令等价调整**：host 测试运行需编译期 `AX_IP=192.168.137.2 AX_GW=192.168.137.1` env（axnet consts 为 `option_env!`）；axnet 三面测试需 `RUSTFLAGS='-C linker=scripts/cc-nopie.sh'`（001-replan V1 既有环境解法沿用）。
4. **T5 迁移实际 79 个 gate**：契约未给数目；按"hold/lease/flush 迁、QEMU 专有留"逐点甄别，含 13 个 `not(qemu)` 伴生 gate 与全部 `RxRxFuture` 初始化点同步翻转（首次漏翻由 queue 面编译失败暴露后补齐）。

**自检发现、已修复内容和遗留 Minor 问题**

- 迁移首轮 queue 面编译失败（`not(qemu)` 伴生块 + 2 处未迁移 future 初始化点）→ 补迁后三面 GREEN；无行为变化（qemu 面 514=506+8 recovery 专有，default 482 三面一致）。
- host 测试三处预期错误（NetBufPtr drop 泄漏属测试语义、14 帧按 TX_PAD_TO pad 到 60、cookie 期望序列）与一处线帧构造 bug（ip 头切片）→ 测试侧修复，产品代码未动。
- Service 启动路径首次实现即通过 host 等价序列见证；板级路径真板验证随 T5 V3 进行。
- **V3 板测 run1（2026-10-12 操作者）两项载荷缺陷（Act 修复，非产品 axnet/adapter 缺陷）**：(a) 65 帧 enqueue 逐行打印 ×~10ms/行（115200）≈650ms 耗尽 200ms lease——hold 实际生效（high-water=4、tx-again=3 为窗口内积压），但尾部排空后第 65 帧不被拒、边界不可观测；修复：打印降频至每 25 帧+被拒帧，循环 <1ms 使全程落于 hold 窗口。(b) flush 段 `drop(pin!(f))` 只 drop `Pin<&mut>` 不 drop 底层 future——唯一 waiter 位未释放，二次 `flush()` 报 `ResourceBusy`（板测原话 `probe err=flush-2 ResourceBusy stop`）；修复：单 waiter 覆盖 Pending→release→C4 全程。修复后 `make k3-async-probe` 重建 exit0（480800B）。host flush_contract 测试不受影响（其 drop 的是 future 值本身）。操作者提示 run 后第二次 `bootm` 未断电复位即重跑出现异常序列——按启动契约重测须外部 RESET 后再装载。
- **V3 板测 run2（2026-10-12 操作者）A4 六项观察五项直证达标**：hold 全程有效（i=25/i=50 行 completions=0 冻结、desc-inflight=16 满）、driver 16→17 Full（tx-submit=16、其后 Again）、release 后原帧全部 terminal（completions=65）、账本精确复原（buf/desc 16/16 归零回满）、flush Pending→release→C4 同 waiter 三行齐全、`auto-release=0`、owner 恒 Active。未达一项：软件 64→65 slot Full 未触发——run2 载荷固定 65 帧，而 hold 期间前 16 帧被硬件 ring 吸收（16+49=65），slot 最高 49<64，属载荷计数设计错误（Act 契约内修复）：改为入队至被拒为止（结构上限 slots+descs+1=81），重建 exit0（480800B）。
- **V3 板测 run3（2026-10-12 操作者）A4 全项直证达标 + A5 板侧确认**：操作者原样输出——`enqueue i=80 accepted=false tx-slot=64 high-water=64 tx-full=1 desc-inflight=16 completions=0`（软件 64→65 Full）；`capacity accepted=80 tx-slot-full=1 tx-again=64 tx-submit=16 desc-inflight=16 slot-backlog=64 lifecycle=2`（driver 16→17 Full，17 次起全部 Again）；`release ok` → `restored tx-slot=0 desc-inflight=0 buf-available=16 desc-available=16 completions=80 lifecycle=2`（release 后原帧全部 terminal、账本精确复原）；`flush pending` → `release-2 ok` → `flush c4 ok`（同 waiter Pending→release→C4）；`probe end auto-release=0 hold-mode=0 lease-expiry=0 flush-success=1 flush-busy=0 flush-cancel=0 lifecycle=2 owner=1 backing=retained`。操作者一句话确认："reset可以恢复"（外部 RESET 回原系统）。
- 遗留 Minor：无未解决项（host 包自身 warning 清零；smoltcp 本地副本既有 warnings 非本轮引入）。

**验证命令、输出和退出码（全部原生，无判定器）**

| 验证 | 命令 | 结果 |
|---|---|---|
| T3 RED | `cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --offline`（adapter 模块缺失时） | E0432 unresolved import，exit 101 |
| T3/T4/T5 host | 同上（带 `AX_IP/AX_GW` env） | 12+1+1 passed 0 failed，exit 0 |
| gmac harness | `rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/ms12-k3-gmac-host && /tmp/ms12-k3-gmac-host --test-threads=1` | 31 passed，exit 0 |
| MS11 回归 | `cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline` | 5 个 target 全 ok（17 项），exit 0 |
| axnet 三面 | `RUSTFLAGS='-C linker=scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib`（依次 default/`--features queue-diagnostics`/`--features qemu-diagnostics`） | 482 / 506 / 514 passed 0 failed，exit 0 |
| 异步构建 | `make k3-async` | exit 0（FIT 468512B） |
| 探针构建 | `make k3-async-probe` | exit 0（FIT 480800B，最终收尾镜像） |
| 受影响回归 | `make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build` | 全部 exit 0 |
| 现场 | `git diff --check`；`openspec validate ms12-com260-kit-async-bidirectional-data-plane` | exit 0；valid |

**Persisted Evidence**

None required（Plan Mode: none；命令与决定性输出已入本 Response）。

**Experience Candidates**

- Runbook 候选（来源：本 Response 验证段）：axnet 组件 host 测试的 no-PIE 链接解法（`RUSTFLAGS='-C linker=scripts/cc-nopie.sh'`，percpu x86_64 32S 重定位在 PIE 测试二进制下失败）与新建 workspace-excluded host 包的依赖三件套（镜像 `[patch.crates-io]` axdriver_net/axtask、axnet dep 需 `dyn` feature 统一、lock 需预置 yanked spin 0.9.8/0.10.0 与 axcpu 0.3.0-preview.8 钉子）。已在本轮端到端验证成功、不照做即链接失败，可重复且高风险。

**未解决问题**

- T5 真板板测（V3，A4）待操作者执行（用户已声明真板操作由其采集）：`make k3-async-probe` 镜像已就绪（480800B）。板测未执行前 tasks 2.3 保持未勾选；A4 达标后由 Plan Review 复核。

## Plan Review

- Review Result: accepted（2026-10-12 用户审计 PASS 后独立 Review；Iteration 001 完成）

**Findings**

独立读取本轮全部新增/修改代码（adapter.rs、k3_async_net.rs Service 路径与诊断载荷、axnet cfg 迁移与诊断入口链、feature/Makefile、k3-async-net-host 三测试文件）并核对 Act Response 与 tasks 勾选；A1–A5 全部有新鲜证据（host 12+1+1、axnet 三面 482/506/514、gmac harness 31、MS11 回归 17、构建矩阵 exit 0、V3 run3 板测全项 + 操作者"reset可以恢复"）。采信 Act Response 记录的验证结论（产生后覆盖表面未再变化；FIT 480800B、`DevError` 无 `TimedOut` 变体、测试计数 12+1+1、`SERVICE_START_NANOS`=5s 均经独立复核）。

非阻塞 findings：

1. **偏差 1（TerminalTimeout→Io）根因为 Plan 契约文字错误**：本 Cycle 契约照抄 future contract 的"返回 TimedOut"，未核查 `axdriver_base` 枚举；Act 的 `Io` 映射是唯一可行落点且行为要件（owner 保留、slot 永不复用、不冒充成功）完整，host 见证 `terminal_timeout_keeps_owner_and_never_reuses_the_slot` 在案。不改代码。
2. **Act Response"未解决问题"节陈旧（Minor）**：仍写"板测待执行、2.3 未勾选"，与同 Response 自检节 run3 记录及 tasks.md 勾选状态矛盾；属板测结果到达后未覆盖该节。权威状态无歧义，不作返工；归档前收尾同步时以 tasks/本 Review 为准。
3. 偏差 2（Service 级测试走 pub API）与偏差 3（env/RUSTFLAGS）经核实为事实必需：`Service`/`EthernetDevice` 为 axnet 私有模块；pub API 即生产路径，见证力不降。偏差 4（79 gate）为披露细节。
4. V3 run1 的两次载荷缺陷（打印耗时耗尽 lease、`drop(pin!(&mut))` 不释放 waiter）与 run2 的帧数设计错误均为载荷侧、契约内修复且留有代码注释；run3 数字内部自洽（80=64+16、第 81 帧被拒、release 后 completions=80 账本复原、同 waiter flush 三行齐全）。

**Acceptance Gaps**

None。A1–A5 全部满足；R4 及 R4–R7 余下部分按 RTM 属 Iteration 002。

**Evidence**

代码核对见 Findings；板测 V3 run3 原样输出在 Act Response 自检节（enqueue i=80 accepted=false tx-slot=64、capacity tx-submit=16 tx-again=64、restored completions=80、flush pending→release-2→c4 ok、probe end auto-release=0 lifecycle=2 owner=1）；验证结论采信来源：Act Response 验证表。

**Follow-up Decision**

Acceptance 已满足且无阻塞项；三条偏差均有合理裁决，Finding 1 记录 Plan 根因、Finding 2 为 Minor 不返工。接受本 Cycle，Iteration 001（T3、T4、T5）完成；按 Map 展开 Iteration 002（T6–T7）。Experience Candidate（host 测试 no-PIE 链接与新 host 包依赖三件套 Runbook）留待用户按需交 Recorder。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

[../../iterations/002-stack-socket-protocol/000-initial.md](../../iterations/002-stack-socket-protocol/000-initial.md)（T6、T7；创建时 draft，待用户批准后 ready）。
