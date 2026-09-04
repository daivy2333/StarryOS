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

- [x] 1.1 在 `crates/virtio-drivers` 建立有界 reset/config primitive：暴露单次 status reset/readback、config generation和一致 net-status snapshot，移除运行时路径对无界 `queue_unset`/Drop wait 的依赖；以 fake MMIO/transport tests证明每步可返回 Pending、generation变化可重试且 reset未确认时 queue backing不释放。（R5、R6；GREEN：transport alloc suite及新增 reset/config tests全绿）
- [x] 1.2 在 `crates/axdriver_net` 定义 transport-neutral recovery contract、checked queue epoch、结构化 stage/progress/ledger和 epoch-aware `TxCookie`，让不支持恢复的 driver稳定返回 `Unsupported`且不暴露 VirtIO token/MMIO；扩展现有 DWMAC model tests证明 API中立、epoch溢出 fail-stop。（R1、R2、R4；Depends on 1.1 contract facts；GREEN：axdriver_net全量 tests通过）
- [x] 1.3 在 `crates/axdriver_virtio`/`VirtIONetRaw` 实现整设备 recovery holder和分步 adapter状态机：停止 submit、status=0 readback后关闭旧 RX/TX owners、重建设备和 refill；reset失败隔离全部 backing，stale/duplicate cookie不命中新 epoch，并提供一致 link snapshot。（R2、R5、R6；Depends on 1.1–1.2；GREEN：真实 adapter fake-transport matrix覆盖成功、延迟、失败、stale、duplicate和资源守恒）

## 2. Iterations 001–003 — Queue Ledger, Deadlines, and Resident Recovery

- [x] 2.1 在 `crates/axnet/src/device` 完成 `(QueueEpoch,ticket)` fixed ledger、`Reclaimed/CancelledPreSubmit/ResetAborted/Fault(stage)` 终结原因与批量 pre-submit cancel；保持 flush drop 只清 waiter，任何非 Reclaimed target 返回稳定错误，并关闭 axnet 全量测试的进程内隔离问题。（R2–R4；Depends on 1.2–1.3；GREEN：cancel/submit 线性化、乱序/stale completion、flush、checked-counter 与 ordinary/diagnostics 串行全量测试通过）
- [x] 2.2 为 submit wait、completion wait、reclaim 分别建立 1s absolute deadline，超时按 D3 取消 Queued 或进入 recovery/fault；以单一提交边界冻结 `{stage,cause,queue_epoch,owner_summary}`，禁止用阶段 code 代替计时器或以分散 relaxed atomics 冒充一致 snapshot。（R3–R4；Depends on 2.1；GREEN：deterministic clock 逐段证明 arm、保持、到期、错误映射与 snapshot 一致性）
- [x] 2.3 在 `crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device`验收并修复唯一常驻 `Active/Quiescing/Resetting/Reinitializing/Faulted` owner，以 1s quiesce、2s reset、2s reinitialize absolute deadline 驱动 bounded recovery step；reset 成功推进 QueueEpoch，失败保留 faulted owner/backing 且不退出或 respawn task。（R1–R5；Depends on 2.2；GREEN：deterministic model 覆盖 driver-stage success/timeout、event interleaving、guard 外 wake 与唯一 owner）

## 3. Iterations 004–005 — Link and Socket Epoch Semantics

- [x] 3.1 在 `kernel/src/drivers/virtio_net_irq.rs` 与 axnet event/control路径分别发布 used-ring/config-change cause，task context读取一致 link snapshot；link down关闭当前 SocketEpoch、取消 Queued、阻止 enqueue/submit但继续回收 DeviceOwned，link up只推进 SocketEpoch并允许新会话，不 reset QueueEpoch。（R3、R6；Depends on 2.3；GREEN：IRQ logic、config register-recheck、combined cause和link off/on model tests全绿）
- [x] 3.2 在 `crates/axnet/src/readiness.rs`、`wrapper.rs`、TCP/UDP/listener/deferred paths引入 epoch-scoped `NetworkTerminal`：旧 epoch映射 `ConnectionReset`，link down映射 `NotConnected`，timeout映射 `TimedOut`，取消映射 `Interrupted`；先提交错误后 wake，恢复后新 socket不继承旧 terminal且旧 socket不复活。（R4、R7；Depends on 3.1；GREEN：多 waiter、handle reuse、listener、deferred retirement、poll后I/O一致性及 ordinary/diagnostics axnet全量 tests通过）

## 4. Iterations 006–007 — Fault Injection and Single-Hart QEMU Qualification

- [x] 4.1 新增 versioned QEMU-only recovery control/snapshot、guest probe和纯输出 validator，复用现有 diagnostic lease制造 queue stall，提供 reset trigger与阶段/epoch/ledger/socket/link marker；保持 V1–V3 ABI、validator不启动QEMU且非QEMU build不暴露控制面。（R4–R8；Depends on 3.2；GREEN：Rust/C/Python host seams覆盖marker顺序、negative fixtures、旧ABI和feature gate）
- [ ] 4.2 清除测试工具中的 revision/run-id pin、hash/source-freeze/time-order manifest 层；在唯一 queue owner 激活后提交首个一致 link snapshot；按 VirtIO 双向 owner 模型校验健康态 `QS` 个常驻 RX owner与空闲 TX capacity，而不把 `device_owned==0` 当作 idle；保留直接行为、环境、阶段顺序、deadline、epoch/ledger、terminal 和 exit 判据。guest probe必须先证明exact ELF可稳定执行：页故障至少记录user PC、fault VA、SP/RA并与program headers和反汇编对齐；peer路径记录socket/connect/send/recv阶段及errno，`EAGAIN/EWOULDBLOCK`只能在同一absolute deadline内通过poll后有界重试，其他errno不得猜测产品修复。自动host/model/build Gate通过后，由用户按R44在single-hart QEMU 7.0.0 VirtIO-MMIO上手工验证reset前流量、queue stall→恢复、旧socket terminal/新socket双向流量及HMP `set_link net0 off/on`，并重跑受影响MS01/MS04/MS05/MS06；一次性FAIL/BLOCKED现场也保存最小raw serial/pcap。（R6、R8；Depends on 4.1；GREEN：初始link为已提交up/down、probe/validator owner判据与真实driver ledger一致、probe无未解释trap且syscall结果可归因、自动产品Gate exit 0、手工workload逐项PASS、无panic/trap/owner drift/永久Pending；环境EPERM必须分层而不得伪记产品PASS）

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
- Required behavior: bounded ledger区分正常、取消、reset和 `Fault(stage)`；recovery只取消Queued；DeviceOwned直到completion或status=0；非Reclaimed target令flush稳定失败；cancel/submit有单一线性化点。
- Preserve: 固定容量64、C4不等于peer delivery、乱序completion、waiter drop语义。
- Forbidden: 自动重发Queued；取消DeviceOwned并提前释放；建立无界history容器。
- Test witness: RED覆盖queued cancel、device-owned cancel拒绝、交错二选一、reset-aborted与带stage fault的flush非成功、stale epoch；隔离复现并关闭ordinary全量进程内SIGSEGV。
- GREEN condition: ticket/buffer/slot守恒，所有target outcome可诊断，无永久Pending；ordinary与diagnostics在 `--test-threads=1` 下均完整退出0。
- Verification: focused device/flush tests后依次运行axnet ordinary与diagnostics串行全量；两条命令不得并发，且均传入 `-- --test-threads=1`。
- Stop when: 上层无法识别driver accept线性化点，返回Plan而非猜测。

### 2.2：Independent data-stage deadlines and coherent recovery fault

- Requirement/Scenario: R3 submit cancellation与device-owned timeout；R4 submit/completion/reclaim和结构化错误；D3、D4、D5。
- Depends on: 2.1。
- Targets: `crates/axnet/src/async_rx.rs`、`recovery.rs`、`device/{fixed_queue.rs,ethernet.rs,mod.rs}`、`service.rs`及deterministic fixtures。
- Current behavior: `recover_stage`只标记fault来源；唯一`recovery_deadline`仅覆盖quiesce/reset/reinitialize。submit、completion、reclaim没有各自的arm instant或到期判断；fault summary由多个relaxed atomic分次写入且无一致读取接口。
- Required behavior: 三个data stage各自使用进入时arm一次的1s absolute deadline；同阶段Pending不得续期。submit到期取消仍为Queued的ticket并发布timeout，completion/reclaim到期仅在ledger完整时进入recovery，否则直接Faulted。fault以单一可一致读取的有界值携带stage、cause、QueueEpoch和owner summary。
- Preserve: 每阶段budget、QueueEpoch与wake generation分离、V1–V3 ABI、status=0前DeviceOwned/backing所有权、guard外wake。
- Forbidden: 用stage code代替deadline；共用quiesce窗口掩盖data wait；分散字段产生跨fault撕裂snapshot；ownership drift进入reset；新增周期polling。
- Test witness: deterministic clock RED分别覆盖submit/completion/reclaim在deadline前Pending、同阶段不续期、恰好到期、submit cancel、completion/reclaim recovery和coherent snapshot并发读取。
- GREEN condition: 三段deadline可独立触发且错误身份稳定；所有受影响flush/waiter完成或稳定失败；旧ABI不变。
- Verification: focused clock/ledger/fault tests及source guards后，依次运行ordinary与diagnostics串行全量，均 `--test-threads=1`、exit 0。
- Stop when: stage入口无法从现有owner/ledger唯一识别，或一致fault值必须破坏V1–V3 ABI；返回Plan重审内部类型或新version边界。

### 2.3：Resident recovery owner and driver-stage deadlines

- Requirement/Scenario: R1唯一owner；R3 quiesce；R4 quiesce/reset/reinitialize；R5整设备reset；D2、D3、D5。
- Depends on: 2.2。
- Targets: `crates/axnet/src/async_rx.rs`、`service.rs`、`router.rs`、`device/{mod.rs,ethernet.rs}`、`stack_runner.rs`及fixtures。
- Current behavior: 工作树已有常驻恢复状态机、1s quiesce与2s reset/reinitialize deadline，但该代码尚未在独立Iteration中完成验收，并依赖缺失的data-stage触发与fault contract。
- Required behavior: 同一future驻留驱动 `Active/Quiescing/Resetting/Reinitializing/Faulted`；每poll只做bounded ledger工作和至多一个driver step。成功提交新QueueEpoch后才开放I/O并wake；失败先提交Faulted与quarantine再在guard外wake，future不退出。
- Preserve: 唯一spawn seam、ISR不搬packet/descriptor、register-recheck、stack runner无10ms polling fallback、V1–V3 lifecycle code 0–4冻结。
- Forbidden: 第二queue task、caller-driven descriptor progress、blocking sleep/spin、guard跨Pending/wake、用reset掩盖ownership drift、Faulted owner退出。
- Test witness: deterministic model逐项覆盖quiesce/reset/reinitialize success与timeout、driver Pending不续期、event窗口、commit-before-wake、Faulted驻留和spawn始终为1。
- GREEN condition: driver-stage lifecycle、epoch、owner summary和backing归属准确；成功后新epoch继续I/O，失败后稳定拒绝且无永久Pending。
- Verification: focused lifecycle/recovery/source guards；依次运行axnet两个串行全量和三个下层focused suite，全部exit 0。
- Stop when: driver step持锁等待设备、恢复需要第二executor/task，或event协议无法承载deadline wake；返回Plan。

### 3.1：Config IRQ and link policy

- Requirement/Scenario: R3 link-down queued cancel；R6 down/up/combined cause。
- Depends on: 2.3。
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
- Required behavior: 新命令/结构使用新version，不改V1–V3 layout；probe按阶段输出确定marker；validator纯审计并拒绝缺失/乱序/FAIL；非qemu feature不可见。
- Preserve: existing ioctl数字和ABI、2s lease crash safety、R44手工QEMU、raw serial为事实源。
- Forbidden: validator import socket/subprocess或启动QEMU；复用旧V3字段改变含义；probe sleep-poll内部axnet。
- Test witness: C/Python/Rust RED fixtures覆盖所有case和negative protocol。
- GREEN condition: probe/validator case-set一致、所有negative被拒绝、existing MS03–MS06 host seams通过。
- Verification: `make host-test`先尝试；若仅loopback `EPERM`则逐项运行无socket命令并记录，任何其他失败阻塞；kernel build exit 0。
- Stop when: 新ABI无法在append-only version中表达或需要破坏旧probe，返回Plan。

### 4.2：Single-hart QEMU acceptance and regression

- Requirement/Scenario: R8 host matrix、runtime、compatibility。
- Depends on: 4.1且所有自动产品Gate通过。
- Targets: `kernel/src/syscall/io_mpx/poll.rs`的零`nfds`参数归一化；测试与证据工具精简；guest ELF/页故障与socket syscall诊断；全量Gate、用户手工QEMU与validator。没有fault PC或精确errno时不修改UDP或loader产品语义。
- Current behavior: probe的可信fault与nonblocking send前置已完成；RISC-V musl `poll(NULL, 0, timeout)`实际发出`ppoll` syscall 73，`sys_ppoll`仍对零长度NULL `fds`调用`get_as_mut_slice(0)`并返回`EFAULT`，使runtime停在`wait_for_pre_reset`。x86_64 `sys_poll`存在同构入口；`do_poll`的空集合timer路径已可用。
- Required behavior: `nfds==0`时`sys_ppoll`与x86_64 `sys_poll`都忽略`fds`并把安全空slice交给现有`do_poll`；`nfds>0`继续校验用户范围，timeout与signal路径不变。focused witness通过后，probe完成startup与分阶段socket/send，再在同一次手工session证明reset前流量、stall/reset、旧socket terminal、新socket双向、HMP off/on及MS01/MS04/MS05/MS06回归。
- Preserve: 单hart、MMIO、user-net、LOG=warn、串口与host结果不混淆；现有`do_poll` timer/signal语义、正`nfds`的`EFAULT`、P5/P6实现、历史waiver不提升。
- Forbidden: 修改通用`UserPtr::get_as_mut_slice`以从NULL构造零长度Rust slice；用nanosleep或probe特判绕过产品缺陷；用host/model替代真实MMIO结果；用guest completion声明peer delivery；缺exit/marker仍判PASS。
- Test witness: `003-replan` Evidence中`poll(NULL,0,remaining) -> EFAULT(14)`是修改前target RED；新增focused逻辑/source witness覆盖两个syscall入口，并在QEMU验证零timeout、有限timeout、零`nfds`忽略无效指针及正`nfds` NULL仍`EFAULT`。自动Gate全部GREEN后，用户运行`make ARCH=riscv64 justrun`和HMP协议。
- GREEN condition: validator exit 0，所有case/回归明确PASS，无panic/trap/fatal drift/permanent Pending；old/new epoch与ledger守恒可见。
- Verification: poll focused witness、driver/transport/axnet/smoltcp相关全量、host seams、kernel build、raw QEMU validator；每项输出和exit写Act Response。
- Stop when: 空集合无法由现有timer/signal路径有界唤醒、修复必须改变通用用户内存契约、runtime bytes与exact ELF不匹配、出现未覆盖产品错误、自动产品Gate失败、QEMU环境不满足单hart/MMIO，或用户未提供手工runtime；不得降级结论。

## Iteration Plan

### Iteration 000: Bounded VirtIO recovery substrate

- Tasks: 1.1、1.2、1.3
- Depends on: None
- Stable baseline: transport-neutral bounded recovery contract和VirtIO adapter可在fake transport中安全完成或隔离整设备reset，epoch ledger与link snapshot独立可测。
- Verification boundary: 三个driver/transport crate全量tests通过；reset未确认无Drop/reuse，stale completion不命中新epoch。
- Diagnostic boundary: VirtIO status/config primitive、公共driver contract、adapter queue/buffer ledger。
- Non-goals: axnet task lifecycle、socket/link policy、kernel IRQ、QEMU runtime。

### Iteration 001: Epoch ledger and layered cancellation

- Tasks: 2.1
- Depends on: Iteration 000
- Stable baseline: axnet以有界QueueEpoch ticket ledger区分全部terminal outcome，cancel/submit和flush语义闭合，后续deadline状态机无需猜测packet结局。
- Verification boundary: ledger、ARP/pending gate、cancel/submit、stale/duplicate completion和flush focused tests通过；ordinary/diagnostics以单线程串行全量均exit 0。
- Diagnostic boundary: `fixed_queue`、Ethernet TX slots/pending packets、Service flush waiter及全量test隔离。
- Non-goals: data-stage timer、driver reset lifecycle、link/socket、QEMU ABI。

### Iteration 002: Data-stage deadlines and coherent fault identity

- Tasks: 2.2
- Depends on: Iteration 001
- Stable baseline: submit、completion、reclaim各有独立1s absolute deadline，timeout和ownership fault产生一致可读的stage/epoch/cause/owner summary。
- Verification boundary: deterministic clock逐stage RED/GREEN、同阶段不续期、submit cancel、completion/reclaim recovery、coherent snapshot和两个axnet串行全量通过。
- Diagnostic boundary: queue round stage entry、data deadline/timer、ticket terminal stage、recovery fault publication。
- Non-goals: driver reset状态机的独立验收、link/socket、对外snapshot版本。

### Iteration 003: Resident owner and driver-stage recovery

- Tasks: 2.3
- Depends on: Iteration 002
- Stable baseline: 唯一常驻queue owner按quiesce/reset/reinitialize deadline恢复为新QueueEpoch或驻留Faulted，所有backing和wake顺序可证明。
- Verification boundary: 每个driver stage、reset成功/失败、event窗口、commit-before-wake、唯一owner和下层回归通过。
- Diagnostic boundary: RxRxFuture lifecycle、Service/Router recovery forwarding、driver progress和timer wake。
- Non-goals: config IRQ、SocketEpoch、QEMU ABI/runtime。

### Iteration 004: Link event and queue policy

- Tasks: 3.1
- Depends on: Iteration 003
- Stable baseline: used/config cause独立到达task，link down/up控制enqueue与SocketEpoch边界而不错误推进QueueEpoch。
- Verification boundary: IRQ cause、generation recheck、link policy matrix、axnet focused/full和kernel build通过。
- Diagnostic boundary: VirtIO ISR cause publication、一致link snapshot、Service enqueue gate。
- Non-goals: socket handle terminal存储、QEMU probe/runtime。

### Iteration 005: Epoch-scoped socket terminals

- Tasks: 3.2
- Depends on: Iteration 004
- Stable baseline: 旧SocketEpoch永久返回正确terminal，新epoch socket恢复正常I/O，多waiter/readiness与deferred cleanup不退化。
- Verification boundary: TCP/UDP/listener/deferred/poll后I/O model和两个axnet串行全量通过。
- Diagnostic boundary: SocketEpoch registry、NetworkTerminal映射、bridge wake与handle identity。
- Non-goals: QEMU ABI、真实runtime、透明连接迁移。

### Iteration 006: Recovery probe and validator

- Tasks: 4.1
- Depends on: Iteration 005
- Stable baseline: append-only QEMU recovery控制面、probe marker和纯输出validator形成可冻结、可负向测试的资格协议。
- Verification boundary: Rust/C/Python host seams、negative fixtures、旧ABI、feature gate、host-test可运行部分和kernel build通过。
- Diagnostic boundary: versioned ioctl/snapshot、guest probe协议与validator规则。
- Non-goals: 自动或手工QEMU资格结论。

### Iteration 007: Single-hart QEMU qualification

- Tasks: 4.2
- Depends on: Iteration 006
- Stable baseline: 零`nfds`的poll/ppoll timeout语义不再阻断probe；精简后的行为型测试协议以已初始化link snapshot和真实双向owner ledger，在单hart QEMU VirtIO-MMIO上证明reset、queue stall、link flap、old/new socket和回归结果。
- Verification boundary: focused syscall witness先证明零`nfds`忽略`fds`且正`nfds`仍校验地址；host/model再证明初始link commit、`QS`常驻RX owner与probe/validator契约；自动Gate全绿后，用户手工runtime raw serial由validator判定。
- Diagnostic boundary: RISC-V `poll -> ppoll` ABI、用户指针归一化、空集合timer、queue owner首次link读取、VirtIO RX/TX owner分类、guest exact ELF/fault PC与socket errno、QEMU runtime marker、raw serial/pcap及MS01/MS04/MS05/MS06回归。
- Non-goals: 自动QEMU runner、SMP、PCI/DWMAC/真板、性能。

## Balance Audit

- Iteration 000聚合transport contract与唯一首个backend，因为三者共同形成可独立host验证的安全reset底座；若拆开，单独primitive没有可交付owner闭环。
- Iteration 001只闭合ticket outcome、分层取消、flush和test隔离；它独立提供后续deadline可依赖的owner事实。
- Iteration 002单独完成三个data wait deadline和coherent fault identity；这些计时语义必须先于driver reset生命周期，避免再次用stage label代替timer。
- Iteration 003只验收常驻owner与driver stages；工作树中已有实现可以保留，但必须在2.2稳定后独立证明，而不能据“代码已存在”提前接受。
- Iteration 004与005分开，因为IRQ/link cause和socket registry是不同锁域、不同失败模式，任一都可形成独立host稳定基线。
- Iteration 006先形成可负向验证的ABI/probe/validator协议；Iteration 007移除不直接证明行为的身份/指纹层，修正首次link与双向owner的runtime契约，再执行一次性真实QEMU资格，避免把证据工程或错误的零owner假设当成产品结果。
- 每个task只归属一个Iteration，依赖均指向前序稳定结果；拆分依据是状态、所有权和验证边界，不是测试或文件数量。

## Requirements Traceability Matrix

| Requirement | Scenario group | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| R1 | owner成功恢复、事件交错、阶段失败 | D2、D3 | 1.2、2.3 | 000、003 | `axdriver_net::NetRecoveryControl`；`async_rx::RxRxFuture/RxLifecycle` | contract model；deterministic lifecycle/deadline matrix | None | Covered |
| R2 | 正常、stale、duplicate、epoch耗尽 | D1、D3、D4 | 1.2、1.3、2.1 | 000、001 | `TxCookie`；`VirtIoNetDev::tx_slots`；`TicketTracker` | fake transport old-cookie/duplicate；ticket overflow/ledger conservation | None | Covered |
| R3 | waiter drop、queued cancel、device-owned cancel、submit交错 | D4 | 2.1、2.2、3.1 | 001、002、004 | `flush::FlushFuture`；`TicketTracker`；`EthernetDevice::tx_submit_one` | cancel/submit linearization；data timeout；reset/link bulk cancel | None | Covered |
| R4 | 六阶段timeout和稳定错误 | D2、D3、D5 | 1.2、2.1、2.2、2.3、3.2、4.1 | 000–006 | recovery progress；queue clock；`readiness::NetworkTerminal`；snapshot ABI | deterministic clock每stage；poll后I/O；validator marker | None | Covered |
| R5 | reset成功/未确认、NEEDS_RESET、IRQ交错 | D2、D3 | 1.1、1.3、2.3 | 000、003 | `Transport`；`VirtIONetRaw`；`VirtIoNetDev`；queue owner | delayed/never-zero、reinit failure、backing quarantine | None | Covered |
| R6 | 初始link、link down/up、combined cause | D1、D6 | 1.1、1.3、3.1、4.2 | 000、004、007 | config generation/status；VirtIO ISR；queue owner初始link commit；axnet link state | generation retry；首次snapshot；IRQ cause matrix；QEMU HMP off/on | None | Covered |
| R7 | 旧socket、新socket、terminal-before-wake | D1、D5、D7 | 3.2 | 005 | `SocketSetWrapper`；TCP/UDP/listener/readiness | epoch closure、late add、multiwaiter、handle reuse、deferred cleanup | None | Covered |
| R8 | host fault matrix、零nfds poll timeout、双向owner语义、QEMU、兼容回归 | D8 | 4.1、4.2 | 006、007 | fake transports；`io_mpx::poll`；VirtIO owner summary；QEMU ioctl/probe/validator；Makefile gates | poll focused witness；owner model + C/Python fixtures；single-hart raw serial；MS01/04/05/06 | None | Covered |
