# Iteration 000 / Cycle 000：timer 与真实 GMAC completion IRQ

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T1（1.1）、T2（1.2）。
- Acceptance gaps：None。
- Repair items：None。
- Inherited scope：2026-10-06用户“批准”的R1–R7/S1–S20中本轮映射；RAM-only、单hart、既有DMA安全与公共规则。
- Excluded scope：traits adapter、64slots/C4flush、stack/socket、下一轮实施、runtime recovery、SMP、性能、MS17、全局状态与经验产物。

**Objective**

在保留旧K3polling入口的同时，使新增K3异步入口可构建：既有scheduler的timer future真实恢复，GMAC由IOC触发source133真实RI/TI，最小ISR唤醒deferred task完成收发；mask/arm最后事件与fatal ack窗口有确定性host见证。20个真实TX完成和RX通知不能用软件IRQ或host模拟代替。

**Investigation Facts**

Current Baseline：`k3`分支`aca5dfe9`，本次Plan只新增此目录。MS11最终accepted Act Response/Review（2026-10-06）已经运行：GMAC21项、polling-net-host17项通过；`make k3`/`make build`/`make lichee` exit0；真板四协议、20rawARP TX与reset通过。其未变化DMA/cache/PHY/RAM范围直接采信，Act不建立Plan基线。

当前必要新增基线（Plan实际运行）：

```text
env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib async_rx::tests::event_before_register_is_caught_by_arm_recheck -- --exact --test-threads=1
error[E0425]: cannot find function `net_link_step_diag` in this scope
--> src/async_rx.rs:1948:13
error: could not compile `axnet-ng` (lib test) due to 1 previous error
exit 101
```

定义仅 `qemu-diagnostics`，调用无cfg。这是新增K3普通axnet构建的必要阻塞，T1限修该诊断调用隔离。历史ordinary全绿在该范围已经失效。

```text
env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --features qemu-diagnostics async_rx::tests::event_ -- --test-threads=1
event_after_arm_wakes_sleep_decision ... ok
event_before_register_is_caught_by_arm_recheck ... ok
event_during_register_window_retries ... ok
test result: ok. 3 passed; 0 failed; 511 filtered out
exit 0
```

这是现有事件窗口GREEN，不是K3 adapter见证。R57 Issue有STALE标记、现代码有local fixture；不沿用旧反复跑测口径。已知全局`make host-test`与VF2有既有失败，不纳入本轮全量通过承诺。

Current-State Evidence / Code and Critical Path：

| 路径/符号 | 已确认事实与本轮影响 |
|---|---|
| `kernel/src/entry.rs::init` / `platform/k3.rs::run_first_byte_park` | K3分支经过stage1–8 PHY、stage9 AIA、stage10 device window，调用stage11–13 polling后park；新增async分支在正常stage10结束后接入，不运行stage11–13 |
| `k3_device_irq_probe.rs::run_stage10` | 60、133、277 handler注册后仍保留；133 handler只计数mask，不ack GMAC。handoff须先mask三source、unregister133，再装新handler；60/277保持masked防止PHY/MDIO ISR与DMA初始化交错 |
| axruntime `rust_main` / `init_interrupt` | allocator和axtask scheduler在kernel入口前已初始化；runtime注册timer handler，重设SBIdeadline并调用on_timer_tick。无需重新init scheduler或修改registry源码 |
| axdriver `dyn_drivers/{mod,pci}.rs` / axruntime `klib.rs` | dyn模块注册PCI probe回调，引用axklib mem_iomap；runtime仅paging feature提供Klib实现。现K3与uspace不启用paging，所以新增dyn依赖需要K3限定链接入口，实际GMAC不走动态probe/iomap |
| `crates/axplat-riscv64-k3/src/irq.rs` | static HandlerTable512 + AiaChip，register只接固定AIA路由；handle只处理external vectorbit9，timerbit5无handler、STIE off |
| 同crate `time.rs` / `time_math.rs` | 24MHz换算已有overflow-safe mul_div_floor；set_oneshot_timer已用SBI，init_percpu空；复用现有换算，不新增timer wheel |
| `axtask/src/future/{mod,time}.rs` | block_on使用原生waker+blocked_resched，sleep/timeout接percpu TimerRuntime，由on_timer_tick唤醒；这些属于既有执行器 |
| `k3_gmac/mod.rs::{Gmac,ArenaGuard,ArenaBacking}` | Gmac独占arena，18页，16RX/16TX，64Bdesc/2048Bbuf；ArenaGuard在publish后retain backing。新IRQ路径不能释放或重用OWN资源 |
| 同文件 `init_rx/refill_slot/try_submit/poll_rx/poll_tx` | init程序：APMU delay、reset1s、linkMDIO5s、ringbases/highaddr/cache→RX refill，IRQ_EN=0；TX开启TE/ST；submit14..1514、pad60、OWN最后、TX tail=next slot；每submit1s；RX/reclaim10ms；poll消费W1C |
| `raw_poll_rx_desc` / `desc.rs` | invalidate64B→fence→读OWN；合法RX64..1518包含FCS，copy去4B FCS；小scratch可能只copy部分但报告全长，本轮scratch固定2048防止截断成功 |
| `k3_network_probe.rs::{stage12_arp_request,board::BoardBacking,K3Env}` | 已有60B ARP request、GlobalPage18页backing、native MMIO与CBO board绑定。按需要精准开放/复用，不复制DMA分配和cache实现 |
| `axnet/async_rx.rs::publish_queue_event` | ISR-safe，只更新事件generation/原子计数与waker，不碰Service/desc；现有空等待register→arm双方→generation重查无需重写 |
| `axtask/future/poll.rs::register_irq_waker` / axhal IRQ hook | 已有source waker与after-dispatch hook，可给首轮deferred raw任务注册waker；K3 handler返回source133让hook按原逻辑唤醒。最终queue owner使用axnet publisher |

Reference usage facts（已读实码，Act无需沿引用重建）：R69 K3 GMAC `desc.rs`：RX des3 bit30 IOC，TX des2 bit31 IOC；`regs.rs/core.rs`：IRQ_EN0x1134、STATUS0x1160、default mask0xD041，NIS15/AIS14/FBE12/RI6/TI0，RBU7/TBU2为原status观察。参考`queue.rs`在ISR reclaim且try_lock失败等下一IRQ，是本轮避免的策略。当前AIA原子stopei已实现；MS10实测EIE masking丢MSI，completion suppress只在GMAC。TX tail排他next-slot、RX tail doorbell、CBO顺序和RAM地址以MS11已验收用法为准。R68实际RAM路径如下Verification，不要求回读。

**Implementation Guidance**

先修普通feature阻塞并接timer，再启用IOC与最小handler，最后接deferred raw会话。诊断输出在任务上下文，用现有LineBuf/console。模块名和等价helper拆分留Act；状态、所有权、mask位置、错误和测试边界以以下契约为准。

**Task Contracts**

### T1：原生 timer future 可用且 feature 隔离

- Requirement/Scenario：R4/R5/R7；S14、S16、S20。
- Depends on：None。
- Targets：根`Cargo.toml`、`kernel/Cargo.toml`、Makefile K3 build入口；`crates/axplat-riscv64-k3/{Cargo.toml,src/irq.rs,src/time.rs}`与timer handler平台无关core；kernel `platform/mod.rs`与新`platform/k3_klib.rs`最小链接入口；`crates/axnet/src/async_rx.rs::poll_active`的net_link_step_diag调用；新`tests/k3-timer-host-harness.rs`。
- Current behavior：runtime已建立scheduler，但K3拒绝timer注册、STIE off、SBI初始timer未设；普通axnet因诊断call编译失败。
- Required behavior：根/kernel `k3-async-net`蕴含k3并启用平台`runtime-irq`、kernel axdriver(net/dyn)/axnet；不开axruntime/net-ng自动probe。新增`make k3-async`复用原K3/FIT配置，SMP=1、AX_IP=.2/AX_GW=.1。timer vector完整bit5用独立Atomic handler槽，不进AIA表；register CAS成功才开STIE、重复不覆盖；unregister先mask再移除；dispatch调用一次并返回原timer vector，SSIEoff、external原样。runtime-irq下init_percpu SBI set_timer(0)，24MHz数学复用；老feature仍拒绝timer并保持off。diagnostic call只在原QEMU feature调用，无增加日志功能。
- Required linkage behavior：仅K3异步编译`__axklib_0_3_mem_iomap`原生入口，使用已依赖的memory_addr0.4与axerrno0.2，签名`unsafe extern "Rust" fn(PhysAddr, usize)->AxResult<VirtAddr>`，返回AxError::Unsupported；这是dyn已注册PCI回调的链接接缝，GMAC不用它。保持static boot映射，不开启paging/plat-dyn/probe_all_devices，也不新增依赖/动态映射。原生build/link验证该入口；不建立ABI工具/独立测试系统。
- Preserve：root default/原make k3、D1/QEMU选择、时间换算、AIA原子claim、global hook、原axruntime scheduler/tick。
- Forbidden：第二executor/timer wheel、重复scheduler初始化、IPI/SMP、用户态/UART RX/shell、改registry依赖、source字符串测试充当timer行为见证。
- Test witness：普通axnet当前exit101是已观察RED；修复后同一原生命令GREEN。新增timer原生host测试绑定生产handler core、fake CSR/SBI动作：register成功/重复、enable与mask、unregister、timer/external dispatch互不串路及legacy模式，先RED再实现。`rustc --edition=2024 --test tests/k3-timer-host-harness.rs -o /tmp/ms12-k3-timer-host` 后运行该binary。
- GREEN condition：上述native tests通过、ordinary event test通过，make k3-async成功；真实board sleep_until恢复由T2验证，不凭host声称timer真板可用。
- Verification：新增timer suite；ordinary event精确命令见Current Baseline；`make k3-async`原生build/FIT exit0；只重跑受影响现有测试。叠加其他平台/SMP按现有配置拒绝方式报错，不能悄悄进入支持组合。
- Stop when：需改变scheduler/timer public契约、boot hart映射或MS17底座；立即交Plan。普通featurefix以外的axnet缺陷若改变契约同样交Plan。

### T2：真实 GMAC IRQ→deferred task 与最后完成

- Requirement/Scenario：R1/R5/R6/R7；S1–S4、S8–S9的原始DMA接缝、S16–S20。
- Depends on：T1。
- Targets：`kernel/src/platform/k3_gmac/{mod,regs,desc}.rs`通知/IOC/只读completion接口；新`platform/k3_async_net.rs`的最小ISR及首轮raw载荷，`platform/{mod,k3}.rs`异步分支；`k3_network_probe.rs`必要共享ARP/BoardBacking绑定；`tests/k3-gmac-host-harness.rs`production fake/MMIO/cache扩展，必要新增native IRQ harness。
- Current behavior：IRQ_EN=0/IOC0，Gmac由polling窗口推进，stage10遗留133观察handler；poll W1C可能清除ISR需保留的fatal。
- Required behavior：正常stage10结束后mask60/133/277、unregister原133观察handler，保持60/277masked；独占一个新arena/Gmac，通知关闭下建立IOC模式，再注册133专用handler。未正常结束或注册失败不进入数据面，device通知不开放。默认旧polling构造/descriptor路径保持IOC0，不改旧行为。
- Required IRQ behavior：ISR只读cause→保存pre-ack完整raw→latch fatal→GMAC侧suppress→W1C observed known位/summary→publish_queue_event；不拿owner锁、不碰desc/MDIO、不alloc/print。RI/TI/异常各计数，组合保留；zero只计spurious无完成；未知有效channel cause mask并稳定fault。fatal latch先于ack，后续poll/query首先传播该fault，无reset/新submit。
- Required notification behavior：single-hart现有IRQ-save保护IRQ_EN整体RMW；BOTH一次提交，维持summary/fatal位。普通arm mask采信0xD041（RI/TI/FBE/NIS/AIS），不加readback验收；方向选择只改其completion enable。arm后fence→invalidate/fence检查RX head和有live owner的TX head OWN→复查latch/status；不回收/advance head。没有TX inflight时不得把未使用descOWN0当completion。通用mask窗口不改APLIC/IMSIC EIE。
- Required deferred behavior：原生axtask任务/block_on作为唯一descriptor owner，用existing IRQ waker或等价原生wake；注册→arm/重查→再查事件后Pending，每poll RX/TX分别最多16步，有可达backlog才yield/selfwake。不得等待下一包/周期timer救最后completion。首轮raw载荷复用MS11的60B ARP request，发送20帧到PC192.168.137.1，前19帧完成并重臂后await原生sleep_until(now+100ms)，再创建最后1帧；该一次性等待不调用descriptor poll，用于独立事件与timer恢复见证。每帧1s terminal deadline，RX scratch2048，会话30s上限；其余timer只用于timeout/结束。结束maskdevice通知、停止新增提交、保留已发布backing；任务输出timer wake/cause/claim/deferred poll/descriptor状态和错误。
- Preserve：18页安全arena、16/16desc、64Bcacheline、RX/TX2048buffer、地址宽度检查、highbase先写、OWN-last、CBO/fence、TX exclusive-next tail、RX tail doorbell、pad60、既有deadline、postpublish backing retention。
- Forbidden：ISR drain、try_lock失败等下一IRQ、软件IRQ冒充DMA、STIE驱动completion polling、second owner/executor、主动清OWN/释放DMA、真板物理FBE注错、改MS11tail裁决、Wireshark/identity/validator、下一轮traits/stack实现。
- Test witness：production fake env扩展RED→GREEN：RX/TX IOC位仅async设置且OWN-last；W1C前raw组合cause/fatal保留；zero/unknown有界；irq期间不触desc/锁/console；mask最后completion、event-before/during/after-arm、Pending前窗口、空TX/ringwrap、fatal ack后查询仍error。fakecache实际描述符OWN变化作判据，不只assert调用日志。已有Gmac harness GREEN覆盖未改行为时直接采信；修改其表面后运行对应回归。
- GREEN condition：新增native tests+受影响MS11 tests通过；make k3-async成功；下面真板A2–A5实测通过并报告结果。host绿色不完成T2板测部分。
- Verification：V1–V5如下。硬件交接前完成所有agent可执行测试/build与task Gate3自检；操作者结果待审核时停止，不执行T3。
- Stop when：IOC/mask真实行为与采信用法冲突、无法观察真实RI/TI、最后完成不推进、满环/ownership不符、timer不恢复，交Plan分层诊断；三次同类失败遵守公共规则，不加第四次盲试。

**Invariants**

1. 单boot hart、一个Gmac owner；handler不借用Gmac/arena/Service，descriptor与临时借用不跨await。
2. 所有初始化/提交后的资源按实际OWN和publish状态处置；timeout/cancel/park不是quiesce，不reset/reuse可能device-owned backing。
3. cause、claim、wake、同步后descriptor terminal各有产品观察；一个不能替代另一个。RX/TX吞吐或软件MSI不替代真实IRQ。
4. 周期scheduler tick不发布网络work；仅deadline timer可恢复对应等待。foreground初始化有界轮询沿用既有，不用于数据面fallback。
5. 原make k3与MS11polling路径、其他平台接口保持；本轮不实施后续Map。

**Non-goals**

NetDriver traits、slots、flush、stack/socket四协议、SMP/IPI/迁移、runtime reset/link-flap、性能/soak、UART RX/tty/shell、存储、框架抽离、全局文档同步/经验产物。首轮raw不是MS12完成。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | runtime feature下timer register/重复/unregister/mask/dispatch与普通feature隔离的native tests通过；普通axnet编译缺陷修复；异步build/FIT成功 | T1，R4/R5/R7，S14/S16/S20 |
| A2 | IOC与最小ISR、组合/零/未知/fatal pre-ack latch、mask/arm/Pending窗口与最后completion的生产host见证通过 | T2，R1/R5，S2–S4/S16–S17 |
| A3 | 真板source133 claim中出现真实RI和TI，ack/rearm后至少各一次独立后续cause；deferred任务处理；20TX合法terminal/reclaim、RX至少1合法完整帧、跨16wrap，最后TX无后续包也完成 | T2，R1/R6，S1/S8/S9/S19 |
| A4 | 原生sleep_until在板上真实唤醒任务；30s会话有界、无新traffic时不靠completion polling推进，无TX错误/timeout；故障输出阶段/owner，backing安全retain | T1/T2，R4/R5，S14/S16–S18 |
| A5 | 原polling回归未被async IOC/status路径破坏；仅RAM启动、外部RESET回原Linux；未把PHY/softwareIRQ算DMA、未追加资格工具 | T1/T2，R6/R7，S18–S20 |

**Verification**

V1（agent，T1）：native rustc timer harness；Current Baseline ordinary event精确命令，第一次修复后GREEN；make k3-async。命令退出0与测试原生结果通过，不加runner/validator。

V2（agent，T2）：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/ms12-k3-gmac-host`，运行`/tmp/ms12-k3-gmac-host --test-threads=1`；若为IRQ新增独立harness，同样原生rustc/test binary。现有MS11生产回归：`cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline`；只有其覆盖表面发生变化时补跑。所有新test先观察RED再改生产路径。

V3（agent）：先`make k3`确保old feature兼容；共享axnet调用cfg修复后`make build`为受影响QEMU编译检查。D1 kernel feature受改则`make lichee`；未变化D1采信MS11 accepted build，不重复。最后`make k3-async`生成实际待板测FIT（与make k3共用文件名，后者会覆盖它）。`git diff --check`。build成功只证明编译，不能通过A3/A4。

V4（操作者，前述agent项PASS后最近能力边界）：

1. PC网卡192.168.137.1/24，直连且保持link up；串口1152008N1无流控。构建FIT `StarryOS_riscv64-k3.fit`拷入FAT32 U盘。上电/RESET同时长按s进入U-Boot。
2. 逐行执行 `usb start`、`fatls usb 0:0 /`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。不saveenv；kernel load/entry0x180000000、FDT0x181000000保持。
3. stage1–10按既有链路前置自动运行，30s link窗口保持插线；等新增async raw窗口begin。PC原生ARP响应供RX，不做抓包。serial任务行显示timer await确实返回、source133的真实RI/TI pre-ack cause、后续独立cause、deferred wake以及实际OWN terminal/reclaim。
4. 观察30s结束行：sent/terminal/reclaimed均20，TX error/timeout为0，RX完整帧≥1，真实RI/TI分别至少两次可区分事件；最后一帧不用PC追加流量仍达terminal/reclaim。若新cause仅TBU/RBU、PHY60、软件source1或只看OWN均不通过A3。
5. RESET，直接见到原系统`Bianbu … k3 login:`。执行人一句话报告本轮网络与reset直接结果；原生关键串口输出用于审核，无截图/表格/日志hash要求。

V5（失败处理）：任意err/fault、无RI/TI、无法deadline结束时停止新增工作并按phase/owner报告，保留DMA资源；操作者reset。reset未回原系统则停止一切装载并交用户。未执行板测或不满足A3/A4时，Act Response应为blocked，T2不勾选完成；审核PASS后仅恢复本Cycle剩余验证/反馈，不自动展开后轮。

**Requirements Traceability Matrix（本轮）**

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 | T2 | async_ioc、irq_cause、mask_last_completion、arm_windows、真实RI/TI | Covered |
| R4（timer部分） | T1 | timer_handler_lifecycle、真实timer future恢复 | Covered |
| R5（底层部分） | T1/T2 | feature RED→GREEN、fatal_after_ack、postpublish保留、deadline | Covered |
| R6（底层部分） | T2 | 真板20TX+RX、cause/claim/wake/terminal直接观察 | Covered |
| R7（底层部分） | T1/T2 | legacy feature、MS11回归、RAM boot/reset | Covered |

R2/R3及R4–R7余下Acceptance属于Map001/002，不是本轮Missing，也不能在本轮声称完成。

**Gate 2 Readiness**

PASS：技术调查、契约、追踪与验证计划已闭合；2026-10-06用户原话“批准当前的工作”，设计、三轮Map与本Cycle获批，Status由draft转为ready。仅更新批准状态，执行契约、Acceptance和Verification不变；全局路线调整不扩展本Cycle范围。

**Persisted Evidence**

- Mode: none
- native命令、每项≤20行决定性输出、退出码/人工结果、修改文件/符号与Gate写Act Response；本轮可重跑且无需独立持久化结构。不建Evidence占位目录。

**Risks and Notes**

- 真实DMA IRQ/timer await仍待本轮执行，不是Plan未决接口；若物理结果冲突按Stop when交Plan。
- 首轮host tests必须覆盖生产核心。raw载荷、快照和tests只证明目标行为，不添加身份协议/判定器；原生Cargotest是判据。
- Wireshark既有用户原话“Wireshark不做，后续也不用做”继续有效。MS11历史ping数/reset描述豁免不延伸；本轮reset直接观察必须执行。
- 当前普通axnet feature defect为Issue候选，已有exit101依据；T1修复，本Plan只报告，未调用Recorder。
- root/Kernel K3 feature编译隔离可等价实现；helper命名、输出字段本地格式与LineBuf拆分属非实质选择，不改变Acceptance。
- 已通过且覆盖未变的验证按公共规则采信；不重复测试增强信心。若同一问题三次失败按Gate5停。

## Act Response

- Status: pending

尚未获实施指令，未建立新测试、未修改产品或运行板测。

## Plan Review

- Review Result: pending

未进入实施Review；没有后继Cycle/Iteration产物。

**Next Cycle**

None。

**Next Iteration**

None；001只在Map，待本轮accepted后由Plan展开。
