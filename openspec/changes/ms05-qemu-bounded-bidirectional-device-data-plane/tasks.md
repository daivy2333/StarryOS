## Requirement Map

- R1：固定容量的二层 RX/TX packet slots。
- R2：Typed TX handoff 与可恢复背压。
- R3：TX descriptor ownership、completion 与 buffer 守恒。
- R4：Ticketed C4 flush。
- R5：Packet 语义与 telemetry 可解释。
- R6：MS05 QEMU 验证与证据边界。
- R7：RX/TX queue control 与 transport 解耦。
- R8：RX/TX queue owner 唯一且切换有边界。
- R9：ISR 与 AtomicWaker 保持最小和 IRQ-safe。
- R10：Register-recheck 关闭 lost-wakeup 窗口。
- R11：`RING_EVENT_IDX` 通知抑制与重臂有效。
- R12：queue service 以独立有界 budget 推进。
- R13：最终 slots 替换临时 RX handoff。
- R14：兼容性、验证顺序与 Evidence。

## 1. Transport-Neutral Queue Foundation

- [x] 1.1 在 `crates/axdriver_net/src/lib.rs` 的 `NetQueueControl`、`NetDriverOps` 及其 fake/enum-dispatch implementor 中建立 direction mask、单步 TX submit/reclaim 和 opaque completion cookie contract，并用 DWMAC ownership/interrupt fake model 证明 R7 的 RX/TX completion、suppress、arm-and-check 都不需要 transport token。WHY 是当前接口只能控制 RX，且无返回的全量 `recycle_tx_buffers()` 无法承载 reclaim budget 或乱序 flush；HOW 是先写 compile/contract RED tests，再增加 `Rx`/`Tx` direction、明确 `Again`/`Unsupported` 和“submit error 后 driver 已恢复 buffer”的 ownership 文档与默认路径；EXPECTED 是所有 implementor 编译、fake model 分别覆盖两个方向、公共签名不出现 VirtIO/DWMAC ring/token 类型。运行 `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` 与两个 driver checks，任一非目标 driver 无法安全适配、公共 contract 必须泄漏 transport state 或 error ownership 仍有歧义时停止返回 Plan；不得加入 DWMAC 产品代码或修改 Cargo registry。

- [x] 1.2 在 `crates/axdriver_virtio/src/net.rs`、`crates/axdriver_virtio/src/lib.rs` 和 `crates/virtio-drivers/src/device/net/dev_raw.rs` 把 TX 路径改为一 completion 一 reclaim，并在 adapter 内保持 `token → opaque cookie → NetBuf` 一一对应。WHY 是当前 oversize/submit error 会把 buffer 从实际 free list 中逐次丢失，`QueueFull` 还误报 `BadState`；HOW 是为 oversize、真实 QueueFull、submit error、未知/重复 token、token slot overwrite 和乱序 completion 先写 RED model tests，再保证失败 buffer 回到同一可用集合、运行期 exhaustion 映射 `Again`、reclaim 只在 `transmit_complete` 与 buffer 回收都完成后返回 cookie；EXPECTED 是重复错误前后总 buffer 数不变，readiness 与下一次单-owner、单连续-buffer submit 一致，ownership violation 返回稳定 fatal。运行 VirtIO adapter tests、axdriver checks 和完整 `virtio-drivers` lib tests；若底层 API 不能在不暴露 token 的条件下返回 completion identity，或错误后无法确定唯一 buffer owner，停止返回 Plan。

- [x] 1.3 在 `crates/virtio-drivers/src/queue.rs` 和 `crates/virtio-drivers/src/device/net/dev_raw.rs` 为 receive/send used ring 分别实现 EVENT_IDX suppress/arm/recheck，并把 TX device-notify 判定改为 old/new `u16` wrapping event 公式。WHY 是当前 `should_notify()` 的普通 `>=` 比较跨 wrap 不正确，且 MS04 只闭合 RX used-event；HOW 是先加入窗口外、窗口内、equal boundary、`u16::MAX` wrap、双 used queue suppress/rearm 的 RED tests，再让 add/kick 路径把本批 old/new index 传给规范公式；EXPECTED 是两个 used ring 可独立控制，pending recheck 正确，feature 保持启用，普通和 wrap tests 全 GREEN。运行定向 queue tests与完整 34+ lib tests；若实现只能关闭 `RING_EVENT_IDX`、无条件 notify、穿透私有 raw field 或修改 registry，停止返回 Plan。

## 2. Fixed Slots and Typed Stack Handoff

- [ ] 2.1 在 `crates/axnet/src/device/` 新增专用 `FixedFrameQueue<64>` 与最多 128 个 live ticket 的固定 tracker backing，并在 `EthernetDevice` 初始化时预分配 RX/TX 两组 1514-byte slots。WHY 是 smoltcp `PacketBuffer` 不能保证 64 个最大 frame 的精确容量；HOW 是用 host RED tests 覆盖 0/64/65、最大 frame、oversize、wrap、peek→commit、Full 不部分复制、full→space event、checked ticket exhaustion 和无数据路径分配，再实现普通 frame data+len+ticket metadata；EXPECTED 是两个方向精确容量 64、内存固定、slot 不持有 `NetBufPtr`/descriptor/token。运行 axnet device 定向 tests 和 full lib tests；若需要可变扩容、slot 跨 `Pending` 持有 raw buffer 或 1514-byte 当前 frame 上界与现有 adapter 不一致，停止返回 Plan。

- [ ] 2.2 在 `crates/axnet/src/device/mod.rs`、`loopback.rs` 和 `router.rs` 把 `bool` send 改为 `Accepted { rx_became_ready }`、`Full`、`Dropped(TxDropReason)`、`Fault(DevError)`，并将 Router 改为锁内 peek→全目标 preflight→commit。WHY 是当前先 dequeue 会静默丢包，广播/组播逐设备发送后遇 Full 会在重试时复制已交付 packet；HOW 是为单播、loopback、IPv4 broadcast、IPv6 multicast、missing route、policy drop、任一目标 Full 和 preflight/commit invariant drift 写 RED tests，再让 Router 只在全部 Accepted/明确 Dropped 后 dequeue；EXPECTED 是 Full 保留唯一队首且本轮停止，fanout 不部分交付，drop reason 精确一次，loopback hint 与 disposition 分离。运行 axnet Router/device tests；若多目标 capacity 不能在同一 `Service` guard 内保持稳定，或实现需要 best-effort fanout 简化，停止返回 Plan。

- [ ] 2.3 在 `crates/axnet/src/device/ethernet.rs` 重写 Ethernet frame 生成、ARP request/reply 和 pending flush，使 Active 前保留同步 fallback、Active 后只通过 TX slot typed outcome。WHY 是当前所有 alloc/transmit/ARP Full 都是 warning-only，pending packet 还会在 send 失败后 dequeue；HOW 是先写 unknown-neighbor 双资源 preflight、ARP-request TX Full 保留 RX frame、ARP-reply 更新、pending head Full、expired neighbor retry、oversize/drop 和 fatal 的 RED tests，再按 D3 每次最多提交一个派生 L2 frame并只在 Accepted 后更新/dequeue；EXPECTED 是 ARP pending 容量耗尽为可恢复 Full，reply/pending 不重复，Active 路径不直接碰 descriptor，现有 IPv4/loopback行为回归通过。运行 axnet full lib tests；若一次 ARP 处理必须部分提交多个 frame 且无可持久化进度，或同步 fallback 与 Active owner 无法明确区分，停止返回 Plan。

## 3. Bidirectional Queue Service and Ownership Cutover

- [ ] 3.1 在 `crates/axnet/src/async_rx.rs`、`service.rs` 和 `lib.rs` 将 RX-only lifecycle/notify 演进为双向 data-plane lifecycle 与一个 generation、queue-owner/stack-progress 两个 waker role 的通用 event。WHY 是共享 IRQ、slot producer/consumer 和 socket caller 都需要一致 ordering，但不能互相覆盖 waker；HOW 是保留 V2 lifecycle 数值，先扩展 deterministic interleaving RED tests覆盖 event-before-register、during-register、另一方向 arm 后到达、slot full→space、generation wrap、spurious 和 duplicate start，再实施全有或全无 `Polling→Spawned→Active` preflight；EXPECTED 是 Active/Faulted 都保持双向 AsyncOwned，Unavailable 只发生在切换前，waker 不覆盖且无工作只产生一次 bounded check。运行 axnet async/service tests、100×并发竞态重复和 UART AtomicWaker tests；若 owner 切换需要公开半激活状态、第二 NIC handle 或 guard 跨 await，停止返回 Plan。

- [ ] 3.2 在 `crates/axnet/src/service.rs`、`router.rs`、`device/ethernet.rs` 和 evolved queue future 中实现每轮 reclaim 32→RX 32→submit 32 的双向 service。WHY 是 queue task 必须成为 raw driver 与 slots 间唯一 copier，同时保证持续双向负载公平；HOW 是以 fake driver/slots 为 RED 模型，分别覆盖各阶段 31/32/33、earlier-stage exhausted 仍运行 later stage、TX Again 保留 slot、RX Full 不 reap、recycle/submit/reclaim fatal、bidirectional multi-round growth 和 no-work nudge，再让每阶段独立计数并在任一 backlog 后 self-wake/yield；EXPECTED 是 raw descriptor/token/`NetBufPtr` 不跨 `Pending`，RX frame 同轮 refill，TX 成功 submit 后才 dequeue，三方向都有稳定进度。运行 axnet full tests 和 driver integration checks；若一次 poll 无法给后续阶段预算、任何 raw owner 在返回 Pending 时不唯一或只能依赖 10ms fallback，停止返回 Plan。

- [ ] 3.3 在 `kernel/src/drivers/virtio_net_irq.rs`、`virtio_net_irq_logic.rs`、axnet exports 与结构化 host harness 中把 used-ring ISR publish 改为通用 queue event，并将 Active NIC 的 socket waker 注册接到 stack-progress role。WHY 是 ISR 不能判断 RX/TX，也不能让 socket future 覆盖 queue task waker；HOW 是先用 source/logic RED tests约束 ISR 只做 cause/ack/snapshot/counter/wake、不取 `Service` 锁或 descriptor，再保持 config/unknown/zero cause 规则并增加 RX-slot-ready、TX-slot-space、fatal 对 stack caller 的 progress hint；EXPECTED 是 queue task 由通用 event 唤醒，smoltcp 仍决定真实 readiness，MS03 cause/ACK/PLIC 与 MS04 critical-section guards 全部 GREEN。运行 `make host-test` 的可执行部分、kernel QEMU check、UART tests和 axnet tests；若 ISR 必须读取 queue state、stack hint 被当作精确 fd readiness 或旧 V1/V2 字段发生变化，停止返回 Plan。

## 4. Ticketed Flush, V3 Telemetry, and Deterministic Controls

- [ ] 4.1 在 axnet fixed tracker、queue service 和内部 API 中实现 target-scoped C4 flush future。WHY 是 slot empty、queue empty 或最大 completion ticket 都会在新 traffic/乱序 completion 下提前或无限延迟；HOW 是先写 empty、queued+device-owned、post-target accept、乱序 hole、fatal、second waiter、register-recheck、future cancellation 和 `u64` exhaustion RED tests，再让 flush 捕获 `last_accepted`、扫描固定 live set、只清除匹配 waiter registration；EXPECTED 是所有 `<= target` ticket reclaim 后成功，后续 ticket 不阻塞，第二 waiter `ResourceBusy`，cancel 不改变 packet ownership，fatal 稳定唤醒。运行 axnet flush/async full tests和重复竞态；若实现依赖 completion 有序、动态 waiter allocation 或取消 packet，停止返回 Plan。

- [ ] 4.2 在 axnet snapshot、`kernel/src/drivers/virtio_net_irq_logic.rs`、`virtio_net_irq.rs`、`kernel/src/syscall/fs/ctl.rs` 和 C host ABI tests 中增加 V3 snapshot，同时原样保留 V1/V2 command、size、offset 和写入范围。WHY 是 MS05 需要 slots、budgets、buffer/descriptor、tickets、flush、drop reason 和 fault 的完整账本，又不能破坏 R51 原 consumer；HOW 是先用 Rust/C size/offset/canary RED tests固定 V3 的 28×`u64` V2 前缀，再追加 D9 字段并保持旧 ioctl 只写旧 struct；EXPECTED 是 MS04 probe 继续读取 V2，V3 单一一致 snapshot 可判定 PRE safety 与 Full→recovery。运行 host harness、C syntax/decision tests、kernel check 和 MS04 probe self-tests；若旧字段必须重排/复用或单次 snapshot 会撕裂 owner/fault pair，停止返回 Plan。

- [ ] 4.3 在 `crates/axnet/Cargo.toml` 新增只由 `starry-kernel/qemu` 传递启用的私有 `qemu-diagnostics` feature，并在 axnet queue-service stage seam 与 `sys_ioctl` 中实现最长 2 秒的 `HoldTxSubmit`、`HoldTxReclaim`、`Release` 和内部 flush control。WHY 是 QEMU completion 太快，普通吞吐无法确定达到 slot/descriptor Full；HOW 是先用 fake clock/driver RED tests覆盖 hold 不改 owner、lease timeout auto-release+failure counter、Release event、submit hold 精确 64、reclaim hold 触发真实 `Again`、buffer 守恒和不重置 telemetry，再让 controls 只跳过唯一 owner 的对应 stage，不增加 VirtIO raw ring test hook；EXPECTED 是 controls 不直接写 slot/ring index或伪造 completion，普通 axnet/D1 build不包含入口，probe 异常不会永久停网。运行 axnet model tests、kernel QEMU check、D1 build和 source guards；若控制需要第二 owner、直接 ring mutation、无界 hold 或进入真板 feature，停止返回 Plan。

## 5. Probe and Automatic Product Gates

- [ ] 5.1 在 `tests/ms05_data_plane_probe.c`、host decision harness、`scripts/ms05_data_plane_stimulus.py` 和 Makefile probe target 中实现 snapshot、TX-only、bidirectional、slot-full、descriptor-full、flush 与 bounded network protocol。WHY 是 R6/R14 要求 change-local、非歧义、可确定复现的 runtime witness；HOW 是先用 C decision mutations 和 Python self-tests构造缺 PRE、伪 Full、账本不闭合、deadline/equal boundary、重复 marker、错误 exit、malformed control 等 RED，再实现每个 mode 唯一 `MS05 PASS|FAIL mode=...`、固定 deadline、PRE/HELD/FULL/RELEASED/POST 和 host sequence validation；EXPECTED 是 probe 不能用普通 throughput 代替 Full，不能重置 counter，MS04 V2 probe/stimulus 原样回归。运行 strict C syntax、host harness、script self-tests和 static RISC-V payload build；任何 source/parser/mutation failure 都是产品 Gate，不能交给 QEMU。

- [ ] 5.2 依次运行全部 driver/virtio/axnet/100×竞态/UART/host/probe tests、QEMU 与 D1 checks/build、fmt/source assertions、strict OpenSpec、specs-vs-code 和 full diff review，并在 `evidence/004-automatic-product-gates/` 保存环境、命令、原始输出、退出码、artifact size/hash、review 与 `ENV-BLOCKED` 清单。WHY 是手工 QEMU 前必须关闭所有自动产品和 ownership Gate；HOW 是只把原始日志明确定位的只读路径、禁网、`EPERM`、`SIGSYS` 或用户终端能力记为 `ENV-BLOCKED`，其余任一 compile/link/assert/source/validation/diff 失败立即停止；EXPECTED 是无 Missing、无未批准 Simplified、无未决设计项，Critical/Important finding 为零，fresh QEMU image与四组 guest payload 可追溯。不得清洗 raw log制造 whitespace PASS，也不得用历史 MS04 artifact替代当前产物。

## 6. Independent Manual QEMU Runtime and Closeout

- [ ] 6.1 仅在任务 5.2 全部产品 Gate 通过后，由用户在 R44 允许的普通终端中复跑 `evidence/004-automatic-product-gates/` 列出的原始 `ENV-BLOCKED` 命令，并把完整首次/最终输出、exit、环境差异、payload/image size 与 SHA-256 保存到 `evidence/005-manual-qemu-runtime/`。WHY 是当前 sandbox 已确认 `make host-test` UDP socket 创建 `EPERM`，但环境豁免不能掩盖产品失败；HOW 是逐项使用同一命令，不扩大权限后改用不同测试；EXPECTED 是所有项最终 PASS 或任务保持未完成，任何 Rust/C/link/assert 诊断都返回对应产品任务，不进入 QEMU。

- [ ] 6.2 用户按 R44 在单 hart、单 VirtIO-MMIO NIC QEMU 中运行 MS05 TX-only、bidirectional、slot Full→recovery、descriptor Full→recovery、flush C4、ARP、ICMP、UDP、TCP 5555、nonblocking 和 poll，并按 R51 重跑 MS04 snapshot/idle/nudge/burst；保存完整 serial、probe、host stimulus、commands、environment、exit、marker、revision 与 artifact hashes。WHY 是 model tests不能证明真实 VirtIO device-model IRQ/descriptor progression；HOW 是每个 mode 使用任务 5.1 的 fixed deadline和唯一 marker，检查 slot/ticket/buffer/descriptor账本、三个 budget/yield、fault/restore/IRQ-entry 为零，并保留 MS04 历史 waiver；EXPECTED 是所有本 change 必需 mode 分项 PASS，缺日志、中断、超时、partial telemetry 或单协议成功都不能提升为 PASS。结论必须限定于当前 QEMU 软件/设备模型，不外推 SMP、DWMAC、真板或性能。

- [ ] 6.3 在同一 manual iteration 中执行最终 specs-vs-code、完整 code/full diff Review、strict change validation、non-Evidence whitespace checks和 Evidence index/hash audit，记录所有 task/RTM/Gate 的最终状态。WHY 是 runtime 成功不能覆盖实现偏差或不完整 provenance；HOW 是核对 V1/V2 compatibility、QEMU-only controls feature边界、历史 WAIVED/SKIPPED、每个 required Evidence 文件和唯一 marker，并对 raw logs只做非空/hash/时间范围检查；EXPECTED 是 Critical/Important finding 为零、没有 Missing 或未批准 Simplified、所有 required task有可追溯证据，之后才可由用户决定 change 收尾。不得在本任务归档 change、刷新 SNAPSHOT 或修改 M/D/K/R/I。

## Iteration Plan

### Iteration 000: Transport-Neutral Queue Foundation

- Tasks: 1.1, 1.2, 1.3
- Depends on: None
- Stable baseline: direction-aware contract、opaque cookie、one-completion reclaim、错误后 buffer 守恒，以及双 used-event/old-new EVENT_IDX tests 全部可供 axnet 使用。
- Verification boundary: axdriver_net tests/check、axdriver_virtio net check、完整 virtio-drivers lib tests、相关 fmt/source guard 全部退出 0；公共接口不泄漏 transport token。
- Diagnostic boundary: 失败只定位在公共 queue contract、VirtIO adapter、VirtQueue notification 或非目标 implementor 编译迁移，不涉及 Router、ARP、async task和 runtime probe。
- Non-goals: 不创建 frame slots，不修改 axnet owner/task，不实现 flush、snapshot 或 QEMU control。

### Iteration 001: Fixed Slots and Typed Stack Handoff

- Tasks: 2.1, 2.2, 2.3
- Depends on: Iteration 000
- Stable baseline: 精确 64-capacity slots、ticket acceptance、Router/device typed disposition、fanout preflight 与 ARP peek→commit 在 host tests 中闭合，Active 前同步 fallback仍可用。
- Verification boundary: axnet device/router/ARP model与 full lib tests退出 0，TCP short write、UDP atomicity、loopback 和 MS04 RX tests无回归。
- Diagnostic boundary: 失败只定位在 stack-side slots、routing/fanout、Ethernet/ARP state；不启用双向 descriptor owner。
- Non-goals: 不切换 queue task，不接 ISR，不实现 C4 waiter或 runtime ABI。

### Iteration 002: Bidirectional Queue Service Cutover

- Tasks: 3.1, 3.2, 3.3
- Depends on: Iteration 001
- Stable baseline: 单个长驻 task成为 RX/TX hardware queue 唯一 owner，通用 generation+双 waker role、三阶段 budget和 slots copier 完成，fault不回退第二 owner。
- Verification boundary: deterministic interleavings、100×竞态、bidirectional budget、ISR source guards、MS03/MS04 host harness、UART、axnet full tests和 kernel check全部通过。
- Diagnostic boundary: 失败定位在 lifecycle/event、bounded service、ISR/stack progress wiring；不混入 flush ABI或 QEMU diagnostic控制。
- Non-goals: 不声明 fd readiness、不运行 QEMU、不实现 reset/SMP。

### Iteration 003: Ticketed Flush and V3 Diagnostics

- Tasks: 4.1, 4.2, 4.3
- Depends on: Iteration 002
- Stable baseline: 乱序安全的 target C4 flush、V1/V2-compatible V3账本和 QEMU-only bounded pressure controls在 model/build Gate中闭合。
- Verification boundary: flush/cancel/fatal tests、Rust/C ABI canary、lease controls、QEMU与D1 checks/build、MS04 V2 consumer回归全部退出 0。
- Diagnostic boundary: 失败定位在 ticket tracker、waiter、snapshot映射或 test-control feature boundary；不涉及 guest/host runtime orchestration。
- Non-goals: 不采集 runtime PASS，不推广 controls到真板。

### Iteration 004: Probe and Automatic Product Gates

- Tasks: 5.1, 5.2
- Depends on: Iteration 003
- Stable baseline: 所有自动产品 Gate、probe parser/decision、fresh artifacts与 full diff review完成，手工边界只剩明确 `ENV-BLOCKED` 和 QEMU runtime。
- Verification boundary: task 5.2 明列的全套命令全部 PASS或只有R44原始日志支持的环境阻塞；required Evidence完整、Critical/Important为零。
- Diagnostic boundary: probe/harness失败与产品 test/build/review失败分别记录；任何无法归类的非零结果按产品失败处理并停止。
- Non-goals: 不手工操作 QEMU console，不用历史日志补证。

### Iteration 005: Independent Manual QEMU Runtime and Closeout Review

- Tasks: 6.1, 6.2, 6.3
- Depends on: Iteration 004
- Stable baseline: 当前产物的环境阻塞复跑、MS05全部runtime modes、R51回归、网络功能与最终 provenance/review形成 change-local证据。
- Verification boundary: 所有 required marker、账本、raw logs、exit、hash、revision和最终review满足R6/R14；历史waiver保持原状态。
- Diagnostic boundary: 环境复跑、每个QEMU mode、协议回归和最终Evidence审计分项判定，partial结果不能互相替代。
- Non-goals: 不归档、不刷新全局OpenSpec状态、不声明SMP/真板/性能/fd readiness。

## Balance Audit

| Iteration | Cohesive Result | Independent Verification | Diagnostic Scope | Audit |
|---|---|---|---|---|
| 000 | 可被上层消费的双向 transport contract | driver/queue unit+compile | contract/adapter/VirtQueue | Balanced |
| 001 | 不启用新 owner 的完整 stack handoff | axnet model/full tests | slot/Router/Ethernet/ARP | Balanced |
| 002 | 唯一双向 owner 与有界 copier | race/ISR/UART/kernel checks | lifecycle/event/service wiring | Balanced |
| 003 | flush、ABI 与确定性控制 | model/ABI/multi-platform build | tracker/snapshot/control | Balanced |
| 004 | 可交给用户的 fresh runtime package | automatic full Gate+Evidence | probe/product/review | Balanced |
| 005 | 独立 QEMU 运行与最终审计 | per-mode raw Evidence | environment/runtime/provenance | Balanced |

所有任务只属于一个 iteration，依赖只指向同轮或更早轮次。首轮没有承载 axnet、kernel
runtime 或 Evidence 工作；后续每轮都有前一轮可复用的稳定接口和独立停止边界。
