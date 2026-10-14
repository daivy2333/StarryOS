# Iteration 000 / Cycle 000：真板 DMA 停止与原地重建底层

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T1（1.1）、T2（1.2）、T3（1.3）。
- Acceptance gaps：None。
- Repair items：None。
- Inherited scope：已批准R1–R8/S01–S20和A1–A4中与底层stop/reset、epoch/carrier、安全失败相关的部分；本轮只交付该稳定底层，不声明全变更完成。
- Excluded scope：共享reset请求门控、runtime PHY/source60、link retry timer、socket最终组合与多hart；这些任务留后续两轮，本轮不得提前实施。禁止修改AGENTS、全局tasks/M/R/I、Runbook/Issue或其他用户工作树改动。

**Objective**

建立同一18页DMA arena的有界stop/SWR/原地重建能力和K3 adapter恢复契约；未确认停止时不写环、不结清device-owned；真板直接观察停止期间无新DMA写回、重建后真实RX/TX恢复。失败保持backing安全。该结果被accepted后，后轮才把它接入常驻owner与PHY控制面。

**Investigation Facts**

- Current Baseline：2026-10-13，分支k3、HEAD d07285c0。实际`git diff --name-only 5f977f1e -- kernel/src/platform crates/axnet crates/axdriver_net tests/k3-async-net-host tests/k3-gmac-host-harness.rs crates/axplat-riscv64-k3 Makefile Cargo.toml kernel/Cargo.toml`无输出；目标产品/测试面在最近accepted后未变。已有AGENTS修改和旧模板删除保持。当前新增文件仅本计划目录，不曾修改产品/测试。
- 采信基线：上一变更最终Act/accepted Review中，`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline` exit0（adapter12、flush1、Service1、stack_socket1）；生产Gmac rustc harness31/31、MS11包17项全绿，axnet三面482/506/514；`make k3-async`、`make k3-async-probe`、`make k3`、`make k3-shell`、QEMU build均exit0。当前覆盖未变，不复跑Plan基线。新增runtime恢复场景尚无PASS，由Act先建立RED。
- 可复用硬件现场：CoM260 Kit、DWMAC 5.10a（version0x1054）、MAC物理窗口0xcac82000/0x2000、source133、RTL8211F MDIO地址1；18页=73728B连续arena、16×64B RX/TX descriptor、每buffer2048B、64B Zicbom cache block、PA在已验收安全RAM窗且>4GiB。地址高字先于低字；TX tail为next-slot排他上界，RX tail为doorbell。只复用本仓已验收用法，不重验平台bring-up。
- 寄存器：`k3_gmac/regs.rs`已有DMA_MODE+0x1000/SWRbit0、TX_CTL+0x1104/STbit0、RX_CTL+0x1108/SRbit0、MAC_CONFIG+0/REbit0/TEbit1、IRQ_EN+0x1134/普通arm0xD041、CH_STATUS+0x1160、ringbase/length/MTL/DSL等。R69 `.../k3_gmac/{core,regs}.rs`明确SWR重启内部fetch与stop清ST/SR/TE/RE；其reset超时继续初始化是反例，不采用。
- 生产`Gmac::init_rx`是同步启动序列，SWR≤1s及PHY≤5s内有spin；不能作为runtime recovery step。`bounded_stop/stop_tx`仅止损，不证明停止；`poll_tx`只在合法OWN=0时回收，1s terminal过期保留TimedOut槽。Gmac没有软件epoch，descriptor也无epoch；新的微步状态归Gmac，唯一QueueEpoch归adapter。
- `ArenaGuard::publish/Drop`：发布前drop释放，发布后drop保留。当前保留设计继续有效；本轮成功恢复复用同一arena，不添加“每轮新分配”或解分配API。只有确认停止后才清/复用旧环，CPU copy carriers也只能按契约结清；不确定时资源留在旧表。
- `K3NetAdapter::Core`含唯一Gmac/env、16个TX CPU copy pool、1个RX staging、16项`Option<TxOwner{cookie,buf}>`、fault/quarantine、completions_seen。`submit_tx` copies→Gmac OWN后绑定实际slot；`reclaim_tx` OWN=0→take owner→返回cookie；Full前拒绝归还CPU pool，postpublish失败保留；目前`recovery_control()`继承None。
- 既有`axdriver_net::{NetRecoveryControl,RecoveryStage,RecoveryProgress,OwnerSummary,QueueEpoch,TxCookie}`足够：begin/step有界，RecoveryStage为Idle/Resetting/Reinitializing/Recovered/Faulted；QueueEpoch.advance checked；cookie含epoch/ticket。Adapter实现该trait，Gmac本地hardware微步转成以上stage；不改trait、不新增第二epoch。
- `K3NetAdapter::read_link_status`本轮继续默认Unsupported，不在本轮接runtimePHY；Service已有初始link读取会接受Unsupported，不要求本轮声称link恢复。下一轮负责完整link接通。
- 原生入口`kernel/src/platform/k3.rs::run_first_byte_park` stages1–10→`k3_async_net::board::run_k3_async_net_service(st10)`：要求WindowEnded，mask60/133/277，退役133观察handler，alloc/zero arena、Gmac::new_async/init_rx/enable_tx，之后才创建adapter→`init_network`→唯一queue→唯一stack。当前Service路径arm_irq_access/register133在adapter创建之后；T3必须将这两项移到Gmac初始化之后、底层probe之前，先arm access再register，并在移交时复用同一handler、不重复注册。T3底层probe放在Gmac创建后、adapter/Service安装前；通过后移动同一Gmac进去，不得和Service并行访问。
- `tests/k3-gmac-host-harness.rs`以path include生产Gmac和IRQ模块，rustc无Cargo feature；`adapter`子模块已有k3-async-net feature门控，不破坏该harness。`tests/k3-async-net-host`默认启k3-async-net、依赖本地axdriver_net/axnet，测试按独立二进制隔离singleton。其FakeEnv当前SWR立即清，缺少runtime reset延迟/stuck/清状态；FakeWire读写同一arena；TrackingBacking有Drop观察。
- 当前`adapter_contract.rs::cookies_keep_owner_epoch_across_the_device`在MIN实例直接提交MIN+1 cookie。新契约要求先真实恢复到MIN+1，再保持原cookie原样回传断言；增加旧/未来cookie拒绝，不删除原断言。MAX/private状态边界在adapter.rs本模块cfg(test)单测设置，不为integration测试新增生产seed API或专用feature。
- host timer事实：axhal宿主无平台backend、wall_time=0。底层新测试只用FakeEnv的advance_clock手动推进；不要在host axtask timeout上等待。本轮不改宿主时钟后端或新增测试专用协议。真实timebase、stop与DMA结果必须上板取得。
- 参考用法：`/home/daivy/projects/serial/work/k3/docs/network/k3-gmac-dma-irq.md` §8–10和DMA/cache主题，`others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/core.rs::reset_dma/stop_dma/init_hardware`，其可溯源寄存器用法按M42默认采信。内部fetch reset有依据，真板停止/复用没有旧PASS，必须通过下述物理验收；不把“参考仓未定义runtime恢复”扩大为否定本仓已有DMA基线。

**Code and Critical Path**

1. core：`kernel/src/platform/k3_gmac/mod.rs::{Gmac,init_rx,clear_rings,refill_slot,try_submit,poll_tx,arm_notify,mask_notify,ArenaGuard}`；必要位定义只在`regs.rs`添加有来源项。
2. adapter：`kernel/src/platform/k3_gmac/adapter.rs::{Core,TxOwner,K3NetAdapter,NetDriverOps,NetTxQueue,NetQueueControl}`；新增NetRecoveryControl impl，existing can_transmit/receive/submit/reclaim/arm按当前恢复状态拒绝不合法访问。
3. native：`kernel/src/platform/k3_async_net.rs::{board::run_k3_async_net_service,board::raw_session,k3_net_diagnostics}`；在已有probe模式接底层流程，普通镜像不跑测试。保持source133最小handler；access/handler在probe前就绪，随后按既有唯一owner移交。
4. tests：`tests/k3-gmac-host-harness.rs`、`tests/k3-async-net-host/{src/fake.rs,tests/recovery_contract.rs,tests/adapter_contract.rs}`；后两个中已有生产回归继续生效。测试直接访问生产接口与真实内存owner，不做源码字符串断言或另起模型替代生产实现。

**Implementation Guidance**

先core RED→GREEN，再adapter RED→GREEN，最后native接线和构建。可以提取已有init配置的等价固定步骤，但保持普通init/polling行为；helper命名/拆分留Act。本地硬件微步是必要的平台状态，不是新通用恢复框架。

stop期间先mask设备通知、清ST/SR与TE/RE，再请求SWR；RMW分成读/写微步，任何已读值只在同一owner状态内使用。每次step至多一个MMIO读/写或一个固定slot的clear/refill单位；返回Pending前不保留锁。SWR确认后固定记录Stopped边界，下一step才可能重建，便于原生probe在保留arena下观察。production常驻owner将由后轮直接推进，不等待操作者。

重建配置沿用现有绝对寄存器写序、64B descriptor和2048B buffer cache维护，先清旧环并同步，再高/低基址和长度，MAC/MTL、RX refill和TX enable；SWR失败不得进入任何步骤。沿用最后已知LinkConfig而非调用5秒同步decode；本轮PHY变化不在验收范围。最终通知保持owner register/arm/recheck可用，不在移交前抢先arm到无waker状态。

**Task Contracts**

### T1：有界停止与同 arena 重建

- Requirement/Scenario：R2/R4/R5/R6/R8；S03/S05/S10/S11/S12/S15，当前轮只覆盖硬件微步。
- Depends on：None。
- Targets：`k3_gmac/mod.rs::Gmac` runtime状态、生产寄存器配置与单槽clear/refill；`regs.rs`仅必要字段；`tests/k3-gmac-host-harness.rs`。
- Current behavior：仅同步启动SWR；运行时stop为止损，无法重建；发布arena永久保留。
- Required behavior：运行时begin在健康/可接纳状态开始有界stop；重复begin稳定Busy/BadState且不重复写SWR。微步到SWR确认前保留旧环、head/tail、TX状态和backing，拒绝新submit/reap/refill。SWR等待绝对deadline1s；同阶段step不续期。确认后才进入Stopped→清环/配置/16-slot重建→Running；同arena重建，RX/TX计数回到正确固定容量；中途错误不发布Running，已重新发布资源继续保留。原rawdescriptor不加epoch字段。
- Preserve：既有同步init、polling IOC=0与async IOC、地址/cache/OWN-last/tail语义；ArenaGuard未发布可释放、已发布保留；RE/TE和SR/ST之外RMW位保留；无真实时钟spin和软件清OWN撤销。
- Forbidden：调用init_rx作为runtime step；全板clock/reset/PHY重置；新arena分配；自动重试直到成功；clear OWN冒充completion；无来源idle位、source/日志身份机制；本轮改共享axnet。
- First test：先在原harness建立 `runtime_reset_is_step_bounded`、`reset_does_not_rewrite_ring_before_confirmation`、`reset_deadline_is_absolute`、`runtime_reset_rebuilds_same_arena`、`stuck_reset_retains_arena_and_owners`；新增测试引用尚不存在的runtime API或断言当前不能满足的行为，运行并观察RED（可为缺符号编译失败，必须记录）。以FakeEnv手动时钟与MMIO/cache trace确认每步行为；原有31项GREEN采信未变，不为证明先行测试另跑一遍。
- GREEN condition：上述生产路径测试全过，delayed/stuck/SWR边界在明确stage返回；确认前无ring-base/内存rewrite或drop，成功后同VA/PA固定18页、16 RX已发布、TX容量16，不依赖下一事件；refill失败owner保留。
- Verification：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host`，再运行该二进制实际新测试filter或最终`--test-threads=1`；exit0。编译/测试失败保留当前task，不继续T2。MS11完整包集中在本轮末T3前，单项新变化可定向运行。
- Stop when：实际controller语义与采用的SWR/配置用法矛盾；需要新bus-idle位/改DMA地址模型/放宽停止条件；或者无法不复用未确认owner而重建。返回Plan，不自行补前置审计框架。

### T2：恢复 adapter 与 epoch/carrier 闭合

- Requirement/Scenario：R1/R2/R4/R5/R6/R7/R8；S03/S04/S05/S11/S12/S13/S15，R1/R7本轮只覆盖drivertrait边界。
- Depends on：T1 Gate3 PASS。
- Targets：`k3_gmac/adapter.rs::Core/TxOwner/K3NetAdapter`，其NetDriverOps/NetTxQueue/NetQueueControl impl与新增NetRecoveryControl；`tests/k3-async-net-host/src/fake.rs`和`tests/recovery_contract.rs`。
- Current behavior：recovery_control=None；cookie未与K3当前实例核对；owner表只支持正常completion或稳定fault。
- Required behavior：唯一checked QueueEpoch由adapter持有；healthy begin先checked advance，exhausted在设备访问之前BadState，重复begin不重复reset；NetRecoveryControl progress包含目标epoch，微步沿T1阶段，Recovered仅在全部重建成立后发布新epoch。未停止不能take/drop旧owner或clear quarantine；可信停止后终结旧drivercarriers并使旧cookie永不被当新完成输出。旧IRQ hint只同步重查当前descriptor，不能修改新表；过期cookie拒绝且不触及设备。复用同pool/arena，成功后固定容量恢复。stuck/reinit/fault返回稳定Io/BadState，不再接受提交或arm不安全状态。
- Required behavior（账本）：OwnerSummary固定32逻辑carrier，健康idle=16 available/16 RX device_owned/0quarantine；TX在途N→16−N/16+N/0。停止未确认即使软件Faulted也保留可能device_owned分类；确认后未重新发布carrier为quarantined。详细TX pool/descriptor ledger各固定16，不与软件slots混加；completions_seen只统计真实合法terminal，reset不能伪造C4。
- Preserve：原typed Full/Length/Io/BadState语义、CPU pool与DMA arena区别、已发布Drop保留、borrow/lock不跨await、IRQ不访问adapter；read_link_status在本轮仍Unsupported，不伪造link。
- Forbidden：新增trait/epoch身份体系、永久丢弃旧arena再分配、reset成功即把flush当成功、照搬VirtIO status=0或quarantine分类掩盖未停止硬件；本轮改public SocketOps和sharedowner。
- First test：在新`tests/recovery_contract.rs`直接调用生产trait建立 `recovery_epoch_cookie_and_old_hint_are_isolated`、`reset_failure_preserves_carrier_ledger_and_drop`、`reinit_failure_never_activates_partial_queue`、`repeated_recovery_preserves_capacity`；recovery_control当前None是可观察RED。`recovery_epoch_exhaustion_is_fail_before_touch`在adapter.rs本模块cfg(test)单测直接设MAX，不扩生产API。现有FakeEnv只增加延迟/stuck/reset清状态与已用寄存器trace，不写独立transport模型或判定CLI。
- GREEN condition：正常、delay/stuck、重复begin、旧hint/过期cookie、slot wrap与两次同arena恢复全部满足owner/epoch结果；失败时TrackingBacking未drop、descriptor不复用；每个真实完成只输出一次cookie；新epoch合法TX/RX完成。测试明确区分软件旧cookie/hint与不能模拟为物理证明的任意旧DMA写。
- Verification：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline --test recovery_contract`；最终完整package仅本轮末一次。host不等待真实timer；deadline由FakeEnv推进。原adapter/flush/Service/socket回归须保持exit0。
- Stop when：既有trait不足以表达已确定控制面、需要改变ticket/flush全局语义、硬件停止与owner表关闭无法配对或pool分类无法守恒；返回Plan。

### T3：真实停止、原地重建与收发见证

- Requirement/Scenario：R4/R5/R8；S10/S18/S19/S20的底层部分。物理失败注入与socket组合不在本轮。
- Depends on：T1/T2 Gate3 PASS；所有agent构建通过后才交操作者。
- Targets：`k3_async_net.rs::board::run_k3_async_net_service` 中现有probe分支的Gmac→adapter移交前，复用原生raw会话与Gmac观察；仅必要的既有诊断显示。
- Current behavior：现有probe只验证容量/flush与四协议；driver创建后直接移交Service，没有运行时停止/原地重建真板结果。
- Required behavior：在现有probe模式以T1生产微步完成“真实收发基线→stop/SWR确认→Stopped保留arena观察→原地重建→真实收发”并报告直接状态。Gmac初始化后先arm IRQ access、注册唯一source133 handler，再运行probe；移交Service不重复注册。整个流程使用同一Gmac/同一18页arena；Stopped观察期不submit/reap/refill、不修改arena。观察后的重建不得通过新分配掩盖旧owner；正常与失败退出都保留arena。完成后才将同一driver移交adapter/Service；普通k3-async不执行该测试流程。
- Preserve：已有独立feature/单hart拒绝、stage10 WindowEnded门槛、source133真实cause/arm/recheck、source60/277屏蔽、原FIT/RAM启动与普通网络入口；不改变已验收轮询和shell入口。
- Forbidden：在已启动的Service旁边raw访问driver；另建镜像CLI/串口握手协议/hostvalidator；依据SWR读回单项判PASS；把PC不通当DMA停止证明；新增Hash/run-id/源码冻结/manifest；删除用户原改动。
- First test：接线前采信T1/T2刚取得的生产stop/旧资源/rebuild GREEN与已验收原入口构建，作为复用代码接线的先行测试；T3不另实现恢复逻辑。若必须新增不在T1/T2测试覆盖内的行为，先在现有测试入口建立该行为RED，再接线。新增物理见证只能按下列真实入口取得，未执行不声明完成。
- GREEN condition：普通/诊断镜像构建通过；真板停止有真实输入和发布owner前提、保留arena下没有新DMA写回、恢复后新RX/TX/RI/TI和固定容量成立；同一资源没有重复回收，操作者外部RESET可回原系统。
- Verification（agent）：先补完受影响定向测试；本轮末完整生产harness、K3host包与MS11host包各一次exit0。`make k3-async`、`make k3`、`make k3-shell`，最后`make k3-async-probe`，均exit0；未触碰的QEMU/sharedaxnet结果沿用原结论，本轮不做QEMU全量。构建完成后执行Gate3自检与本轮全量diff自检，将具体镜像和下面最短操作交操作者，不先勾选T3。
- Verification（操作者）：依照下方A3最短步骤。失败输出记stage/原始status/owner；未跑或不符保持T3未完成，不通过换新arena或延长窗口补救。
- Stop when：停止后保留内存仍变化、不能保持exclusive观察、恢复要改变clock/reset或队列用法、或真板行为否定SWR停止边界；立即停止，Response blocked并交Plan，不继续后轮。

**Invariants**

- 设备可能可达的backing始终保留；只有SWR停止确认后才允许旧环和slot复用，DMA读写同步仍是MS11既有CPU VA cache/PA device契约。
- 软件标签不证明硬件停止；32个逻辑carrier、16TXdescriptor/pool、64软件slots分别记账；常驻RX owner不是泄漏，不要求device_owned=0。
- 同一队列任一时刻只有一个访问owner：probe移交前是foreground，移交后仅Service常驻queue；无并行raw路径。
- 正常async最后事件仍走register/arm/recheck；stop/rebuild中的one-shot进度读取不变成正常completion轮询fallback。
- runtime stop≤1s；重建可按已固定步数有界完成，后轮resident Reinitializing外部deadline2s。每次step的stage绝对deadline不续期；原板上timebase不修改。
- FBE/unknown/ownership drift保持稳定错误，不能用reset清除未解释归属；恢复成功后才清旧可清状态，历史累计计数不作为身份也不被清成新运行编号。

**Non-goals**

本轮不把共享reset请求门控从QEMU迁出，不接runtime source60/MDIO，不改socket terminal/readiness/Router缓冲；不创建后轮目录。全部变更范围仍要求后轮覆盖上述内容，本轮不把未做项豁免或标完成。

**Acceptance**

本轮追踪：R2→T1/T2→A1；R4→T1/T2/T3→A1/A3；R5→T1/T2/T3→A1/A4；R6→T1/T2→A1；R8→T1–T3→A2/A4。R1/R7的adapter边界在T2建立，但其常驻owner与socket验收留后轮；R3留后轮，本轮不标完成。所有首测均在上述Task Contract中。

- A1（T1/T2；R2/R4/R5/R6）：生产测试先RED后GREEN，证明stop确认前无新ring/owner复用，timeout/reinit失败保留，成功同arena重建、checkedepoch、旧hint/cookie隔离和两次资源闭合。
- A2（T3；R8/S19）：普通/probe/旧K3/shell受影响构建成功；harness、完整K3host与MS11包通过。未变的axnet/QEMU结论按上方来源采信，不重复验证。
- A3（T3；R4/S10）：必须真板直接见证，最短步骤如下；缺少目标输入、内存或重建后收发任一部分都不通过。
- A4（T3；R5/R8/S20）：失败有界停止并保留不确定backing；操作者按原习惯外部RESET回官方系统，原生一句话结果足够。外部RESET不计runtime reset验收。

**Verification：A3 真板最短操作**

1. agent完成上述构建并说明probe分支已接入的实际输出字段；按既有RAM Runbook复制`StarryOS_riscv64-k3.fit`到U盘。操作者上电/RESET同时长按`s`入U-Boot，执行`usb start`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`；不saveenv，不刷写。
2. 进入底层恢复probe前，Windows PC开始`ping -t -w 100 192.168.137.2`并保持；缩短单次等待以免默认长timeout使2s停止窗没有输入。native raw会话发实际ARP并报告已完成TX/RX，作为PHY/peer真实输入前提。此处还没有ICMP协议栈，应答不作为前置要求；必须实际观察RX帧/ARP回应，PC命令启动本身不算收到帧。
3. probe执行有界stop/SWR，在Stopped打印原始mode、SR/ST/TE/RE、同步的旧descriptor/owner摘要。保留旧arena，使用真实timer的2s观察窗；不清/重写任何DMA可达内存、不release backing。直接报告全ring与buffer同步前后新增写回字节数及若非零的首处原值/新值，须为零；这只是目标内存观察，不输出“资格PASS”或做日志判定协议。PC持续输入和此前RX真实有效必须同时成立，不能拿拔线/无输入造成的不变值充数。
4. probe推进同arena原地重建，重新真实ARP RX/TX（至少一次双向完成、真实RI/TI、TX完整terminal）、报告固定16槽容量；观察重建失败则不启动Service。成功后才移交同一Gmac给adapter/Service；本轮无需用socket会话代替该硬件结果。
5. 停止PC ping，操作者外部RESET回原系统；以一句话加决定性串口输出报告步骤2–4实际所见。每段输出不要求截图、完整日志目录或身份表。

2s保留观察是目标DMA状态的有界行为测试，不是运行时间顺序/身份核验；单项SWR、IRQ静止或PC退出码均不能代替A3。它结合R69内部fetch reset用法、旧资源同步观察与同arena恢复后真实交换支持本轮停止/复用结论；发现任何反证即不接受，不宣称已证明所有未测故障下无限期总线静止。

**Gate 2 Readiness**

PASS：技术维度自检结论采信未变；2026-10-13 用户审计详细计划后原话“批准”，计划获批，Status转ready。BLOCKED：None。WAIVED：None。

**Persisted Evidence**

- Mode: none
- 命令、实际退出码、每项决定性输出与操作者一句话直接结果进入Act Response。安全RAM板测可重复，Response足够支持是否接受；不创建evidence占位目录、不增加采集或判定工具。

**Risks and Notes**

- 正常板上PHY/参考clock变化可能使SWR不自清；保留资源并报stop/resetstage，不能照搬参考仓warning继续。
- native观察必须保持旧arena同步读且没有CPU修改；若需要违反exclusivecache规则才能观察，立即返回Plan调整，不改写为“寄存器够了”。
- payload字段排版、helper命名和等价局部拆分不影响契约，留Act；以上资源、阶段、deadline、观察面与移交顺序不可自行改变。
- 已有Wireshark永久豁免适用；无本轮恢复行为豁免。第一轮技术/板测结果只支持本轮底层，不能标R1–R8全部完成。

## Act Response

- Status: reported
- Date: 2026-10-13（Iteration 000 / Cycle 000，T1→T2→T3 全部完成，A3 真板会话由操作者执行）

### 实际改动

1. `kernel/src/platform/k3_gmac/mod.rs`：新增 `RuntimeRecovery{Resetting,Rebuilding,Failed}`、`RecoveryRefusal{Busy,BadState}`、`RecoveryStep{Pending,Stepped,Recovered}` 公共类型与私有 `Recovery` 微步机器（固定 cursor + 一次性 SWR 绝对 deadline + RMW scratch + 捕获的 teardown 前 MAC_CONFIG 原值）；`Gmac` 新增 `runtime_recovery()/runtime_begin_reset()/runtime_poll_step()` 及私有 `resetting_step()`（10 步：mask→RE/TE、SR/ST RMW 拆步→SWR 请求→确认/超时）与 `rebuild_step()`（74 步：软件账本重启→32 个单槽 clear→配置/基址/MAC/MTL 绝对写序→16 个 refill 单位→TX enable，全部复用既有 `refill_slot`/cache/tail 契约）。`poll_rx/poll_tx/try_submit/check_completions` 增加 recovery 拒绝 guard；`arm_notify` 在 recovery 期间为 no-op。原始 descriptor 未加任何 epoch 字段。
2. `kernel/src/platform/k3_gmac/adapter.rs`：`Core` 新增 `epoch: QueueEpoch` 与 `recovery: Option<QueueEpoch>`；`NetDriverOps::recovery_control()` 改为 `Some(self)`；实现 `NetRecoveryControl`（begin 先 checked advance 再触设备；确认边界结清旧 TX carrier 入 quarantine；stage 映射 Gmac 微步）；`submit_tx` 增加 recovery guard 与 cookie epoch 绑定（旧/未来 cookie 设备访问前 BadState）；`reclaim_tx`/`arm_notify_and_check` 增加 recovery guard；`owner_summary()` 按 32 逻辑 carrier 守恒（available=16−N−Q / device_owned=16+N / quarantined=Q，确认后 N 归零入 Q）。`read_link_status` 保持 trait 默认 Unsupported。cfg(test) 单测 `recovery_epoch_exhaustion_is_fail_before_touch` 直接设 MAX。
3. `kernel/src/platform/k3_async_net.rs`：`arm_irq_access`+`register(133)` 前移到 Gmac 初始化后、probe 前，Service 移交复用同一 handler 不重复注册；新增 `native_recovery_probe`（仅 `k3-net-diagnostics`）：基线真实 ARP 会话→quiesce（mask+路由关闭）→有界 stop 步进→Stopped 边界寄存器/owner 摘要打印+同步 arena 快照→2 s 保留观察（写回非零即 park 不移交）→原地重建步进→重建事实打印→第二次真实 ARP 会话→关 notify/路由移交。`sync_arena_read` 为整 arena 块粒度 invalidate+fence+volatile 只读。
4. `tests/k3-gmac-host-harness.rs`：新增 5 个先行测试（`runtime_reset_is_step_bounded`、`reset_does_not_rewrite_ring_before_confirmation`、`reset_deadline_is_absolute`、`runtime_reset_rebuilds_same_arena`、`stuck_reset_retains_arena_and_owners`），先 RED（E0433 缺符号）后 GREEN。
5. `tests/k3-async-net-host/src/fake.rs`：`BoardState` 增加 `swr_latency_ns/swr_stuck/cache_cost_ns/reg_writes`；SWR 触发按脚本延迟或 stuck，并按真板语义清除驱动已写寄存器（保留 HW_FEATURE1）；cache 操作推进时钟。
6. `tests/k3-async-net-host/tests/recovery_contract.rs`（新）：5 个测试（契约 4 个具名 + `recovery_control_is_exposed` RED 种子）。
7. `tests/k3-async-net-host/tests/adapter_contract.rs`：identity 测试改断言 recovery_control 为 Some；`cookies_keep_owner_epoch_across_the_device` 按计划 Investigation Facts 先真实恢复到 epoch 1 再保留原样回传断言。
8. `tests/k3-async-net-host/tests/service_owner.rs`：`fault` 精确计数断言 1→3（见偏差 2）。

### 与计划的偏差

1. **MAC_CONFIG 重建值来源**：计划写“沿用最后已知 LinkConfig”；实现改为捕获 teardown 前读到的 MAC_CONFIG 寄存器原值并在重建时原样写回。等价（该值即 init 按 LinkConfig 写入的结果），且完全避免同步 decode。契约内等价局部实现。
2. **`service_owner.rs` fault 计数同步（1→3）**：T2 暴露 `recovery_control` 后，axnet Service 对设备 fatal 改走 resident-recovery 路径（design ›Risks 已预判：“adapter控制面接通后，原generic owner会对可恢复timeout尝试恢复……FBE/unknown/drift与故障恢复阶段失败仍fail-stop，正常初始数据面契约增量同步”）。设备 fatal 现在产生三次 terminal 记账：fatal 检测（COMPLETION_QUERY）、resident-recovery 进入（enter_recovery）、begin 拒绝后 publish_recovery_fault（Gmac Faulted 状态按“FBE 保持稳定错误、不可被 reset 清除”拒绝 begin）。lifecycle 仍稳定 Faulted、不再服务、不回退轮询——fail-stop 语义未变，仅精确计数断言按新拓扑同步。Iter 001 T4 整改共享接缝时会再次覆盖此处。
3. `recovery_contract.rs` 除契约 4 个具名测试外增加 `recovery_control_is_exposed`（计划指定“recovery_control 当前 None 是可观察 RED”的落点）。

### 自检发现与修复

- 首版 `resetting_step` 缺 cursor 7（SWR 请求前 DMA_MODE 读取），step-bounded 测试以 unreachable panic 暴露，已补。
- `runtime_begin_reset` 对 failed 机器曾返回 Busy；契约要求稳定 BadState，已按 `failed.is_some()` 区分。
- `rebuild_step` 内两处 `?`（clear 单元的 cache range 检查、refill 单元）会绕过 `fail_recovery`，改为错误流入统一失败路径。
- harness 步进边界测试最初把 fence 计入 cache 操作上限，改为只计 flush/clean/inval（fence 是屏障标记）。
- 全部在 GREEN 前修复；遗留 Minor：probe 的逻辑不可达臂使用 `unreachable!`（机器状态转换保证，接受）。

### 验证命令与实际结果

- T1 RED：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-recovery-host` → E0433（`RuntimeRecovery`/`RecoveryStep` 不存在）。
- T1 GREEN：同命令构建 exit 0；`/tmp/k3-gmac-recovery-host --test-threads=1` → **36 passed; 0 failed**（31 既有 + 5 新增）。
- T2 RED：`cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline --test recovery_contract` → 0 passed; 5 failed（recovery_control 为 None）。
- T2 GREEN：同命令 → **5 passed**；`--test adapter_contract` → **12 passed**；`--lib` → **1 passed**（epoch 耗尽）。
- 本轮末全量（`AX_IP=10.0.2.15 AX_GW=10.0.2.2 cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline`）：lib 1 + adapter 12 + flush 1 + recovery 5 + service 1 + stack_socket 1 = **21 passed; 0 failed**（6 个二进制）。
- MS11 包（`cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline`）：0+6+6+5 = **17 passed; 0 failed**。
- 构建门：`make k3-async`、`make k3`、`make k3-shell`、`make k3-async-probe`（最后）全部 exit 0；`StarryOS_riscv64-k3.fit` 为 probe 镜像。
- host 包需编译期 `AX_IP`/`AX_GW`（axnet `IP.parse()` 在缺失时空串 panic），沿用既有测试的 10.0.2.x 约定；非本次引入。
- 未触碰的 axnet 三面与 QEMU 结论按计划采信（覆盖未变化）。

### A3 真板会话（操作者 2026-10-13 执行，Act 审核 PASS）

- 操作者确认全程保持 `ping -t -w 100 192.168.137.2`（超时为预期：probe 阶段无 ICMP 协议栈）；外部 RESET 正常回官方 Linux（操作者原话“正常恢复”）。
- 决定性串口行：基线 `sent=20 terminals=20 rx-frames=19 irq-runs=38 ri=19 ti=20`（真实 RI/TI + 19 个真实 RX 帧）；`probe stopped mode/rxctl/txctl/mac/status=0x00000000 sr=0 st=0 te=0 re=0 tx-inflight=0`（SWR 确认、控制面全零）；**`probe stopped-window window-ms=2000 writeback-bytes=0`（持续输入前提下零 DMA 写回）**；`probe rebuilt state=running rx-published=16 tx-capacity=16 irq-en=0x0`（同 arena 重建）；`probe rebuilt-session sent=20 terminals=20 rx-frames=19 irq-runs=39 ri=20 ti=20`；`probe ok handover-next backing=retained`；随后 Service owner-active/stack-active 与容量（80+slot-full）/flush C4 诊断正常。
- A1/A2（host RED→GREEN 与构建门）、A3（真板停止/保留/重建见证）、A4（失败安全边界由 host fixture 覆盖；外部 RESET 回原系统经操作者确认）全部满足。

### Persisted Evidence

- Mode: none。无 Evidence 目录；命令、退出码、判定行与操作者一句话已录本 Response（安全 RAM 板测可低成本重跑）。

### Experience Candidates

- None（板测流程复用既有 RAM boot Runbook 与 Cycle 内 A3 步骤；AX_IP 编译期约定为既有已知项，无新增可重复高风险操作或范围外缺陷）。

### 未解决问题

- axnet resident-recovery 的 terminal-fault 记账（fatal+进入+begin 拒绝共 3 次）属 Iteration 001 T4“Fault 稳定隔离”范围，届时随共享接缝整改；本轮不修改 axnet。

## Plan Review

- Review Result: accepted
- Date: 2026-10-13

**Findings**

独立阅读了全部 8 个文件的工作树 diff 与关联未跟踪文件，并对照三个 Task Contract 逐项核对 Act Response 声明，全部与代码一致：

- T1：`Recovery` 微步机结构、Resetting 0–9（10 步）与 Rebuild 0–73（74 步）游标、SWR 一次性绝对 deadline（`RESET_BOUND_NANOS`=1e9，既有常量）、RMW 读/写拆步、teardown 前 MAC_CONFIG 捕获均如实；`poll_rx`/`try_submit`/`poll_tx`/`check_completions` 四处 recovery guard 与 `arm_notify` no-op 落位正确；重建写序与 `init_rx` 第 5–11 步一致（APMU/MDIO 按 Forbidden 不重做）；确认前不触碰环内存与 head/tail/owner。
- T2：epoch 先 checked advance 再触设备（MAX 时零寄存器写入由 cfg(test) 单测直证）；Rebuilding 边界才结清 owner 入 quarantine，quarantine 无清除路径；`owner_summary` 公式与声明逐字一致（16−N−Q / 16+N / Q，确认后 N 归零入 Q）；stale/future cookie 在设备访问前 BadState；`recovery_control()=Some`；`read_link_status` 未动（Unsupported）。
- T3：probe 仅 `k3-net-diagnostics` 门控；arm access→register(133) 前移到 Gmac 初始化后、probe 前，旧注册点删除、无重复注册；基线会话要求 `rx_frames>=1` 作为真实输入前提；2 s 窗口为真实 timer，写回非零即 park 不移交；重建步数 200 护栏；OWN 位直读 16 槽；移交前关通知。A3 判定行与代码 format 字段逐项对应（操作者记录为压缩转写，字段一致）。
- 测试面：harness 5 个新测与契约 First test 清单同名同断言；`recovery_contract.rs` 5 测试覆盖 15/17/0（未确认保守分类）、15/16/1（重建失败不激活部分队列）、16/16/0 两轮容量守恒与同基址、重复 reclaim 无伪造；fake.rs SWR 真板语义（清驱动寄存器、保留 HW_FEATURE1）与 cache 计费如实；`adapter_contract` 两处同步（identity 翻转、epoch-1 真实恢复后回传）保留原断言；`service_owner` fault 1→3 附根因注释。
- 验证结论按公共规则采信 Act Response（本 Review 只读 diff，未见结论产生后覆盖面变化；测试计数与测试文件逐项对账相符：31+5=36、1+12+1+5+1+1=21）。

偏差核对：Act 记录的偏差 1（MAC_CONFIG 捕获原值替代 LinkConfig，等价）、偏差 2（fault 1→3，design Risks 已预判，T4 整改）、偏差 3（`recovery_control_is_exposed` RED 种子）均准确且在契约等价范围内，分类为合理的局部实现选择。

非阻塞 Minor finding（记录，不要求返工）：

1. 捕获的 `mac_config_orig` 含 TE（捕获时 TX 已使能），`REBUILD_MAC_CONFIG` 单元使 TE 早于专用 TX-enable RMW（70/71 步对其幂等）；终态与 init 序等价，中间窗 TE=1 且 ST=0 无 TX 可能。
2. 重建不重跑 `init_rx` 步 0 的 `addr_fits_width` 复验；同 arena 构造时已验，宽度仍参与 EAME 判定。
3. adapter 层重复 begin 的 `ResourceBusy` 拒绝无直接断言（Gmac 层 Busy/BadState 由 harness 覆盖）。
4. 前移的 register-133 失败串仍标 `[ms12]`（纯文案）。

**Acceptance Gaps**

None。A1（harness/recovery 契约 RED→GREEN）、A2（构建门与全量包）、A3（真板停止/2 s 零写回/同 arena 重建/重建后真实收发/移交）、A4（host fixture 失败保留 + 操作者外部 RESET 回原系统确认）全部满足，证据见 Act Response 验证节与 A3 节。

**Evidence**

- 代码：工作树 diff（8 文件，+1369/−50）与 `tests/recovery_contract.rs`（新），逐文件独立阅读；关键符号 `Gmac::{runtime_begin_reset, runtime_poll_step, resetting_step, rebuild_step, fail_recovery}`、`Core::{epoch, recovery}`、`NetRecoveryControl for K3NetAdapter`、`native_recovery_probe`。
- 验证运行：采信 Act Response（覆盖未变）；harness 36/36、k3-async-net-host 21/21、k3-polling-net-host 17/17、四 make exit 0、A3 板测判定行。
- Persisted Evidence mode=none，无 Evidence 目录，符合契约。

**Follow-up Decision**

Iteration 000 的 Stable baseline（SWR 未确认不复用、同 arena 重建、epoch/carrier 账本可复用）已由 host GREEN 与真板直接见证共同成立；四项 Minor finding 均不影响 Acceptance（前两项为等价实现细节，第三项由机器层覆盖，第四项纯文案），不构成当前 Cycle 修复或 rework 依据。按 Iteration Map 推进。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/001-resident-owner-link-recovery/000-initial.md`（已按既有 Map 展开，Plan Context 为 `draft`，待用户 Gate 2 批准后转 `ready`）。
