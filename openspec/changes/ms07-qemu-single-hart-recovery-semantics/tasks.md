## Requirement Map

- R1：唯一常驻 queue owner 承载恢复生命周期
- R2：Reset epoch 绑定 descriptor、cookie 与 ticket owner ledger
- R3：取消按 waiter、pre-submit 与 device-owned 分层
- R4：分阶段 deadline 与错误传播可诊断
- R5：VirtIO-MMIO 整设备 reset 确认停止后再重建
- R6：Link config-change 独立控制面
- R7：Socket 错误按数据面 epoch 隔离
- R8：故障注入、回归与结论边界

## 1. Iteration 000 — Bounded VirtIO Recovery Substrate

- [ ] 1.1 在 `crates/virtio-drivers` 建立有界 reset/config primitive：暴露单次 status reset/readback、config generation和一致 net-status snapshot，移除运行时路径对无界 `queue_unset`/Drop wait 的依赖；以 fake MMIO/transport tests证明每步可返回 Pending、generation变化可重试且 reset未确认时 queue backing不释放。（R5、R6；GREEN：transport alloc suite及新增 reset/config tests全绿）
- [ ] 1.2 在 `crates/axdriver_net` 定义 transport-neutral recovery contract、checked queue epoch、结构化 stage/progress/ledger和 epoch-aware `TxCookie`，让不支持恢复的 driver稳定返回 `Unsupported`且不暴露 VirtIO token/MMIO；扩展现有 DWMAC model tests证明 API中立、epoch溢出 fail-stop。（R1、R2、R4；Depends on 1.1 contract facts；GREEN：axdriver_net全量 tests通过）
- [ ] 1.3 在 `crates/axdriver_virtio`/`VirtIONetRaw` 实现整设备 recovery holder和分步 adapter状态机：停止 submit、status=0 readback后关闭旧 RX/TX owners、重建设备和 refill；reset失败隔离全部 backing，stale/duplicate cookie不命中新 epoch，并提供一致 link snapshot。（R2、R5、R6；Depends on 1.1–1.2；GREEN：真实 adapter fake-transport matrix覆盖成功、延迟、失败、stale、duplicate和资源守恒）

## 2. Iteration 001 — Queue Owner Recovery and Cancellation

- [ ] 2.1 在 `crates/axnet/src/device` 扩展 fixed ticket/slot ledger为 `(QueueEpoch,ticket)` 与 `Reclaimed/CancelledPreSubmit/ResetAborted/Fault`终结原因，建立 submit/completion/reclaim deadline与批量 pre-submit cancel；保持 flush drop只清 waiter，任何非-Reclaimed target返回稳定错误。（R2–R4；Depends on 1.2–1.3；GREEN：cancel/submit线性化、乱序/stale completion、flush和checked-counter tests全绿）
- [ ] 2.2 在 `crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device`接入唯一常驻 `Active/Quiescing/Resetting/Reinitializing/Faulted` owner，按 1s data-stage、1s quiesce、2s reset/reinitialize deadline驱动 bounded recovery step；reset成功推进 QueueEpoch，失败保留 faulted owner/backing且不退出/respawn task。（R1–R5；Depends on 2.1；GREEN：deterministic clock/model tests覆盖每阶段 success/timeout、event interleaving、无 guard跨 Pending和无第二 owner）

## 3. Iteration 002 — Link and Socket Epoch Semantics

- [ ] 3.1 在 `kernel/src/drivers/virtio_net_irq.rs` 与 axnet event/control路径分别发布 used-ring/config-change cause，task context读取一致 link snapshot；link down关闭当前 SocketEpoch、取消 Queued、阻止 enqueue/submit但继续回收 DeviceOwned，link up只推进 SocketEpoch并允许新会话，不 reset QueueEpoch。（R3、R6；Depends on 2.2；GREEN：IRQ logic、config register-recheck、combined cause和link off/on model tests全绿）
- [ ] 3.2 在 `crates/axnet/src/readiness.rs`、`wrapper.rs`、TCP/UDP/listener/deferred paths引入 epoch-scoped `NetworkTerminal`：旧 epoch映射 `ConnectionReset`，link down映射 `NotConnected`，timeout映射 `TimedOut`，取消映射 `Interrupted`；先提交错误后 wake，恢复后新 socket不继承旧 terminal且旧 socket不复活。（R4、R7；Depends on 3.1；GREEN：多 waiter、handle reuse、listener、deferred retirement、poll后I/O一致性及 ordinary/diagnostics axnet全量 tests通过）

## 4. Iteration 003 — Fault Injection and Single-Hart QEMU Qualification

- [ ] 4.1 新增 versioned QEMU-only recovery control/snapshot、guest probe和纯输出 validator，复用现有 diagnostic lease制造 queue stall，提供 reset trigger与阶段/epoch/ledger/socket/link marker；保持 V1–V3 ABI、validator不启动QEMU且非QEMU build不暴露控制面。（R4–R8；Depends on 3.2；GREEN：Rust/C/Python host seams覆盖marker顺序、negative fixtures、旧ABI和feature gate）
- [ ] 4.2 执行自动 host/model/build Gate并冻结 revision/artifact identity，再由用户按 R44 在单 hart QEMU 7.0.0 VirtIO-MMIO 上手工验证 reset前流量、queue stall→恢复、旧socket terminal/新socket双向流量及 HMP `set_link net0 off/on`；重跑受影响 MS01/MS04/MS05/MS06并由validator审计完整raw串口与exit。（R8；Depends on 4.1；GREEN：所有自动产品Gate exit 0、手工workload逐项PASS、无panic/trap/owner drift/永久Pending；环境EPERM必须分层而不得伪记产品PASS）

## Task Contracts

### 1.1：Bounded transport reset/config primitives

- Requirement/Scenario: R5 reset success/failure；R6一致link snapshot。
- Depends on: None。
- Targets: `crates/virtio-drivers/src/transport/{mod.rs,mmio.rs,pci.rs}`、`device/net/{mod.rs,dev_raw.rs}`、transport/queue tests。
- Current behavior: begin_init可写status=0；MMIO Drop只写0，modern queue_unset无界自旋；config generation未进入trait，net status只初始化读取。
- Required behavior: 每次调用只执行有界寄存器操作；reset start和status=0确认分离；一致config snapshot遇generation变化返回可重试结果；运行时reset失败不触发旧queue/backing Drop。
- Preserve: 现有初始化、EVENT_IDX、PCI编译和普通Drop语义；不新增executor依赖。
- Forbidden: 在单次poll自旋到设备响应；暴露MMIO header给axnet；声明PCI已运行验证。
- Test witness: 先新增 fake transport RED，证明缺 reset pending/config retry；运行 `cargo test --manifest-path crates/virtio-drivers/Cargo.toml --locked --offline --lib --features alloc`。
- GREEN condition: 新模型逐步观察 reset pending→complete、generation mismatch→retry；36项既有test及新增项全绿。
- Verification: 上述全量命令 exit 0；source guard确认 recovery path不调用无界queue_unset/Drop wait。
- Stop when: QEMU MMIO无法在不释放旧queue的前提下读写status，或规范要求与D3冲突，返回Plan。

### 1.2：Transport-neutral recovery contract and epoch cookie

- Requirement/Scenario: R1 driver step；R2 current/stale/overflow；R4 stage identity。
- Depends on: 1.1接口事实，但可先写contract RED。
- Targets: `crates/axdriver_net/src/lib.rs`及tests；必要时workspace patch清单。
- Current behavior: `TxCookie(u64)`无epoch；queue control只负责通知；DevError无恢复stage。
- Required behavior: recovery accessor默认Unsupported；typed stage/progress/owner summary；`TxCookie`可无歧义取得epoch/ticket；checked epoch耗尽进入fault。
- Preserve: transport token私有、legacy driver source兼容的default accessor、NetQueueControl原职责。
- Forbidden: patch外部Cargo registry；把MMIO/descriptor类型放入公共contract；silent wrapping。
- Test witness: API/model RED覆盖legacy default、epoch round-trip和overflow；全量axdriver_net命令。
- GREEN condition: DWMAC/legacy model无需实现reset仍编译，typed recovery模型通过且现有7项不退化。
- Verification: `cargo test --manifest-path crates/axdriver_net/Cargo.toml --locked --offline --lib` exit 0。
- Stop when: contract必须泄漏transport token才能表达owner，返回Plan重审分层。

### 1.3：VirtIO adapter recovery transaction

- Requirement/Scenario: R2正常/stale/duplicate；R5全设备reset、NEEDS_RESET、IRQ交错；R6 link snapshot。
- Depends on: 1.1、1.2。
- Targets: `crates/axdriver_virtio/src/net.rs`、`crates/virtio-drivers/src/device/net/dev_raw.rs`、`queue.rs`及fake device/transport tests。
- Current behavior: inner不可运行时重建；RX全在queue，TX slot/fault ledger稳定但永久；forced token/completion failure seam已存在。
- Required behavior: recovery holder在status=0前完整保留旧对象；成功后关闭old owners并重建/refill；失败后资源计入quarantine；cookie按epoch匹配；link snapshot一致读取。
- Preserve: pre-accept Again归还buffer、post-accept invariant稳定fault、buffer/descriptor真实守恒、QS固定。
- Forbidden: reset未确认即mem::drop旧queue/buffer；把reset-aborted计为completion/reclaimed；自动重试ownership invariant。
- Test witness: 在真实adapter fake transport先写RED：delayed zero、never zero、reinit failure、old cookie after new epoch、link generation race。
- GREEN condition: 每个路径owner ledger精确闭合或隔离；全量`--features net`通过。
- Verification: `cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --locked --offline --lib --features net` exit 0，并复跑1.1/1.2。
- Stop when: Rust所有权布局迫使reset failure提前Drop backing；不得用unsafe绕过，返回Plan。

### 2.1：Epoch ticket outcomes and layered cancellation

- Requirement/Scenario: R2 ticket绑定；R3三层取消及交错；R4 submit/completion/reclaim。
- Depends on: 1.2、1.3。
- Targets: `crates/axnet/src/device/{fixed_queue.rs,ethernet.rs,mod.rs,tests.rs}`、`flush.rs`、`service.rs`。
- Current behavior: live ticket仅Queued/DeviceOwned，删除即被flush视为完成；flush drop只清waiter。
- Required behavior: bounded ledger区分正常/取消/reset/fault；recovery只取消Queued；DeviceOwned直到completion或status=0；非Reclaimed target令flush稳定失败；cancel/submit有单一线性化点。
- Preserve: 固定容量64、C4不等于peer delivery、乱序completion、waiter drop语义。
- Forbidden: 自动重发Queued；取消DeviceOwned并提前释放；建立无界history容器。
- Test witness: RED覆盖queued cancel、device-owned cancel拒绝、交错二选一、reset-aborted flush非成功、stale epoch。
- GREEN condition: ticket/buffer/slot守恒，所有target outcome可诊断，无永久Pending。
- Verification: focused device/flush tests后运行axnet ordinary与diagnostics全量。
- Stop when: 上层无法识别driver accept线性化点，返回Plan而非猜测。

### 2.2：Resident recovery owner and staged deadlines

- Requirement/Scenario: R1全部；R3 device-owned quiesce；R4六阶段；R5调用driver recovery。
- Depends on: 2.1。
- Targets: `crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device/{mod.rs,ethernet.rs}`、`stack_runner.rs`及fixtures。
- Current behavior: lifecycle单向Active→Faulted，fatal后future结束；Service fault/flush terminal不可恢复。
- Required behavior: 同一future在恢复状态驻留；1s data/quiesce和2s reset/reinit deadline；事件register-recheck覆盖状态窗口；成功推进QueueEpoch，失败保持async owner/quarantine。
- Preserve: stage budgets、ISR不搬包、无guard跨Pending/wake、stack runner在fault不回退10ms polling。
- Forbidden: 二次spawn、caller descriptor progress、blocking sleep/spin、在owner mismatch后自动reset。
- Test witness: deterministic clock RED逐项覆盖阶段timeout、quiesce完整ledger冻结/失败、reset success/failure和事件窗口。
- GREEN condition: lifecycle和telemetry给出准确stage/epoch，所有waiter完成或稳定error，owner始终唯一。
- Verification: focused lifecycle/source guards；axnet ordinary 371+和diagnostics393+全量exit 0（数量允许随新增test增长）。
- Stop when: driver step会持锁等待设备，或恢复需要第二executor/task，返回Plan。

### 3.1：Config IRQ and link policy

- Requirement/Scenario: R3 link-down queued cancel；R6 down/up/combined cause。
- Depends on: 2.2。
- Targets: `kernel/src/drivers/virtio_net_irq.rs`、`virtio_net_irq_logic.rs`、`crates/axnet/src/async_rx.rs`、Service/device link preflight和MS03/MS04 host harness。
- Current behavior: config cause仅telemetry，不publish；send path不检查link；combined cause只因used位唤queue。
- Required behavior: cause bits独立publish/recheck；task读取一致snapshot；down关闭socket epoch、取消Queued并阻止新enqueue/submit；DeviceOwned继续reclaim；up不reset queue。
- Preserve: ack known bits、unknown telemetry、ISR IRQ-state恢复、descriptor工作只在task。
- Forbidden: ISR读config/ledger、config伪造completion、link flap触发自动device reset或旧socket复活。
- Test witness: 更新MS03 logic RED覆盖config-only publish、combined双事件、generation retry和link state matrix。
- GREEN condition: config-only最终唤醒控制面，used/config均不丢；link策略匹配D6。
- Verification: MS03/MS04 Rust/C host seams、axnet focused/full tests和kernel build。
- Stop when: platform IRQ入口无法区分或安全发布config cause，返回Plan审查事件contract。

### 3.2：Epoch-scoped socket terminals

- Requirement/Scenario: R4 error映射；R7旧socket、新socket、commit-before-wake。
- Depends on: 3.1。
- Targets: `crates/axnet/src/{readiness.rs,wrapper.rs,tcp.rs,udp.rs,listen_table.rs,stack_runner.rs,service.rs}`。
- Current behavior: boot-global DevError terminal first-wins，late-created handle也继承且无clear。
- Required behavior: NetworkTerminal结构化映射；bridge绑定SocketEpoch；关闭只终结旧epoch，开放新epoch后新handle可用；listener/deferred/raw owner恰好清理一次。
- Preserve: 多waiter、overflow、handle reuse、SERVICE→SOCKET_SET→listener锁序、error-before-wake与poll/I/O一致。
- Forbidden: 清空global code使旧handle复活；重建整个SocketSet；旧TCP/listener透明迁移。
- Test witness: wrapper/TCP/UDP/listener RED覆盖epoch closure、late add、new epoch、multiwaiter、deferred retirement和各AxError映射。
- GREEN condition: 旧handle永久返回原terminal，新handle普通I/O成功，所有existing readiness tests不退化。
- Verification: axnet ordinary/diagnostics全量各exit 0，source lock-order guards通过。
- Stop when: public handle无法稳定保存epoch而只能按当前global判断，返回Plan重新设计registry identity。

### 4.1：Versioned recovery probe and validator

- Requirement/Scenario: R4 telemetry；R5 reset trigger；R6 HMP markers；R8 model/QEMU协议。
- Depends on: 3.2。
- Targets: `kernel/src/syscall/fs/ctl.rs`、IRQ snapshot structs、`tests/ms07_*`、`scripts/ms07-*`、Makefile host-test guards。
- Current behavior: V3 snapshot、diagnostic hold和flush存在；无reset/link/socket epoch ABI；MS06 validator只审计旧协议。
- Required behavior: 新命令/结构使用新version，不改V1–V3 layout；probe按阶段输出确定marker；validator纯审计并拒绝缺失/乱序/错误revision/FAIL；非qemu feature不可见。
- Preserve: existing ioctl数字和ABI、2s lease crash safety、R44手工QEMU、raw serial为事实源。
- Forbidden: validator import socket/subprocess或启动QEMU；复用旧V3字段改变含义；probe sleep-poll内部axnet。
- Test witness: C/Python/Rust RED fixtures覆盖所有case和negative protocol。
- GREEN condition: probe/validator case-set一致、所有negative被拒绝、existing MS03–MS06 host seams通过。
- Verification: `make host-test`先尝试；若仅loopback `EPERM`则逐项运行无socket命令并记录，任何其他失败阻塞；kernel build exit 0。
- Stop when: 新ABI无法在append-only version中表达或需要破坏旧probe，返回Plan。

### 4.2：Single-hart QEMU acceptance and regression

- Requirement/Scenario: R8 host matrix、runtime、compatibility。
- Depends on: 4.1且所有自动产品Gate通过。
- Targets: 无额外产品实现；执行全量Gate、冻结artifact，用户手工QEMU与validator。
- Current behavior: MS06 frozen runtime全过但不覆盖reset/link；R44禁止agent自动驱动QEMU网络测试。
- Required behavior: 同一revision下先证明reset前流量，再触发stall/reset，验证旧socket terminal和新socket双向；HMP off/on分别见证NotConnected与新socket恢复；回归MS01/MS04/MS05/MS06。
- Preserve: 单hart、MMIO、user-net、LOG=warn、串口与host结果不混淆；历史waiver不提升。
- Forbidden: 用host/model替代真实MMIO结果；用guest completion声明peer delivery；缺exit/marker仍判PASS。
- Test witness: 自动Gate全部GREEN后，构建`make ARCH=riscv64 build`并冻结hash；用户以相同配置`make ARCH=riscv64 justrun`和HMP执行协议。
- GREEN condition: validator exit 0，所有case/回归明确PASS，无panic/trap/fatal drift/permanent Pending；old/new epoch与ledger守恒可见。
- Verification: driver/transport/axnet/smoltcp相关全量、host seams、kernel build、raw QEMU validator；每项输出和exit写Act Response。
- Stop when: 自动产品Gate失败、artifact/revision漂移、QEMU环境不满足单hart/MMIO，或用户未提供手工runtime；不得降级结论。

## Iteration Plan

### Iteration 000: Bounded VirtIO recovery substrate

- Tasks: 1.1、1.2、1.3
- Depends on: None
- Stable baseline: transport-neutral bounded recovery contract和VirtIO adapter可在fake transport中安全完成或隔离整设备reset，epoch ledger与link snapshot独立可测。
- Verification boundary: 三个driver/transport crate全量tests通过；reset未确认无Drop/reuse，stale completion不命中新epoch。
- Diagnostic boundary: VirtIO status/config primitive、公共driver contract、adapter queue/buffer ledger。
- Non-goals: axnet task lifecycle、socket/link policy、kernel IRQ、QEMU runtime。

### Iteration 001: Queue owner recovery and cancellation

- Tasks: 2.1、2.2
- Depends on: Iteration 000
- Stable baseline: 唯一常驻queue owner在deterministic model中按deadline执行取消、quiesce、reset和恢复/fault，所有ticket和backing有唯一结局。
- Verification boundary: 每个timeout stage、三层取消、reset成功/失败、event交错和flush均由axnet host tests证明；ordinary/diagnostics全量通过。
- Diagnostic boundary: ticket/slot ledger、Service/Router forwarding、queue future lifecycle和clock/deadline。
- Non-goals: config IRQ、应用socket epoch、guest ABI和真实QEMU。

### Iteration 002: Application-visible link and socket epochs

- Tasks: 3.1、3.2
- Depends on: Iteration 001
- Stable baseline: config-change可达task context，link和reset均以epoch-scoped terminal关闭旧socket且允许合规新socket，不破坏多waiter/readiness。
- Verification boundary: IRQ/link matrix、TCP/UDP/listener/deferred、poll后I/O和full axnet/kernel build通过。
- Diagnostic boundary: IRQ cause publication、link snapshot/policy、SocketEpoch registry和NetworkTerminal映射。
- Non-goals: QEMU probe/runtime、SMP、透明连接迁移。

### Iteration 003: Single-hart QEMU qualification

- Tasks: 4.1、4.2
- Depends on: Iteration 002
- Stable baseline: MS07 single-hart QEMU VirtIO-MMIO恢复语义具有可复跑host/model和手工runtime协议，MS01/MS04/MS05/MS06不退化。
- Verification boundary: 自动Gate、kernel artifact、真实reset、queue stall、link off/on、old/new socket和全回归逐项判定。
- Diagnostic boundary: versioned ABI/probe/validator、QEMU命令/环境、runtime stage marker与host/guest evidence分层。
- Non-goals: 自动QEMU runner、SMP、PCI/DWMAC/真板、性能。

## Balance Audit

- Iteration 000聚合transport contract与唯一首个backend，因为三者共同形成可独立host验证的安全reset底座；若拆开，单独primitive没有可交付owner闭环。
- Iteration 001只处理queue owner和packet生命周期，完成后即使没有socket恢复也能独立证明无UAF/双重回收/永久Pending。
- Iteration 002把link与socket合并，因为link-down的可观察契约就是SocketEpoch关闭，但与设备reset实现故障域不同，符合MS07 split signal。
- Iteration 003聚合ABI工具和runtime资格；probe单独完成不构成产品成果，必须与真实QEMU及回归共同验收。
- 每个task只归属一个Iteration，依赖均指向同轮更早task或前序Iteration；没有以测试数量、文件数量或编号机械拆分。

## Requirements Traceability Matrix

| Requirement | Scenario group | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| R1 | owner成功恢复、事件交错、阶段失败 | D2、D3 | 1.2、2.2 | 000、001 | `axdriver_net::NetRecoveryControl`；`async_rx::RxRxFuture/RxLifecycle` | contract model；deterministic lifecycle/deadline matrix | None | Covered |
| R2 | 正常、stale、duplicate、epoch耗尽 | D1、D3、D4 | 1.2、1.3、2.1 | 000、001 | `TxCookie`；`VirtIoNetDev::tx_slots`；`TicketTracker` | fake transport old-cookie/duplicate；ticket overflow/ledger conservation | None | Covered |
| R3 | waiter drop、queued cancel、device-owned cancel、submit交错 | D4 | 2.1、3.1 | 001、002 | `flush::FlushFuture`；`TicketTracker`；`EthernetDevice::tx_submit_one` | cancel/submit linearization；reset/link bulk cancel | None | Covered |
| R4 | 六阶段timeout和稳定错误 | D2、D3、D5 | 1.2、2.1、2.2、3.2、4.1 | 000–003 | recovery progress；queue clock；`readiness::NetworkTerminal`；snapshot ABI | deterministic clock每stage；poll后I/O；validator marker | None | Covered |
| R5 | reset成功/未确认、NEEDS_RESET、IRQ交错 | D2、D3 | 1.1、1.3、2.2 | 000、001 | `Transport`；`VirtIONetRaw`；`VirtIoNetDev`；queue owner | delayed/never-zero、reinit failure、backing quarantine | None | Covered |
| R6 | link down/up、combined cause | D1、D6 | 1.1、1.3、3.1 | 000、002 | config generation/status；VirtIO ISR；axnet link state | generation retry；IRQ cause matrix；QEMU HMP off/on | None | Covered |
| R7 | 旧socket、新socket、terminal-before-wake | D1、D5、D7 | 3.2 | 002 | `SocketSetWrapper`；TCP/UDP/listener/readiness | epoch closure、late add、multiwaiter、handle reuse、deferred cleanup | None | Covered |
| R8 | host fault matrix、QEMU、兼容回归 | D8 | 4.1、4.2 | 003 | fake transports；QEMU ioctl/probe/validator；Makefile gates | crate/full host tests；single-hart raw serial；MS01/04/05/06 | None | Covered |
