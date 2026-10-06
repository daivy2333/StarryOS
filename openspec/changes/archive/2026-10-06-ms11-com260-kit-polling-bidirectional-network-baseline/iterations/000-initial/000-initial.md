# Iteration 000 / Cycle 000: DMA 可见性与原始 RX

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 1.1, 1.2, 1.3。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: 已批准R1–R6、S1–S11；本轮承担R1/R2/R5/R6对应DMA/RX部分，保留完整change的TX和四协议要求。
- Excluded scope: TX运行及协议栈属于001/002；async IRQ/queue owner、runtime reset/recovery、SMP、shell、storage、offload、性能优化与证据工具不在本轮。

**Objective**

K3 RAM boot在旧stage1–10之后运行stage11：DMA地址与cache/descriptor顺序明确，初始化及RX有界，接收物理PC的真实ARP帧并可安全reap/refill。错误、超时或取消不得夺取设备owner；正常结束亦持有DMA页直到外部复位。host GREEN与实际板测分别成立，才能交Review判断本轮完成。

**Investigation Facts**

Current Baseline：2026-10-03，branch k3、HEAD a504b0de，只新增当前change文档，无产品工作树改动。

- MS09/MS10已验收RAM-only boot、安全RAM、APMU两步、GMAC version1054、RTL8211F PHYID001cc916、link以及AIA软件/PHY source60投递；不再把这些事实变成探索前置。MAC133/277未验收DMA IRQ。
- 采信MS10 iterations/001-phy-device/000-initial.md accepted Review：device harness17/MAC15 passed，make k3/build/lichee exit0，真实stage1–10和reset回原系统。后续f7044e12仅修stage8 begin输出并有PHY31/device17 passed、make k3 exit0记录；后续没有产品变更。Plan实际补充编译PHY harness命令 rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/ms11-phy-probe-host-test，exit0；没有重复运行已通过测试。
- make host-test的ms04 migration既有失败与VF2 axfs/axnet/axtask导入失败仅单列；不冒充PASS，不顺带修复。
- 本轮未实施；尚无本仓DMA/真实RX测试结果。

Current-State Evidence与Code and Critical Path：

| 入口/符号 | 实际调用与状态边界 |
|---|---|
| runtime rust_main → src/main.rs::main → kernel/src/entry.rs::init | allocator/内存/平台先初始化，应用再进入K3 diverging分支；不调用QEMU axnet/UART或加载用户程序 |
| platform/k3.rs::run_first_byte_park | stage4 mac_probe::apmu_clock_enable→stage5 MAC probe→gate→stage6 run_pad_gpio→stage7 run_phy_identity→stage8 run_link_observation→stage9 AIA→stage10→park。旧失败分支不绕过；stage10当前返回值未消费，需要用于新阶段gate |
| k3.rs::K3Env | 私有unit struct，已实现phy_probe::ProbeEnv：MMIO u32 volatile、axhal monotonic time/busy_wait。本轮在原调用点传入新probe或按需设crate可见，复用而非复制MDIO访问算法 |
| phy_probe.rs::mdio_read(env,phy,reg) | 返回Result<u16,MdioError>；PHY1，GMAC ADDR+0x200/DATA+0x204，CR5、read GOC3<<2，busybit0，既有10ms idle/completion deadline；paged接口保存原page。新阶段只读协商reg，不读破坏性INSR |
| k3_device_irq_probe::run_k3_stage10 | 返回WindowEnded(summary)/Stopped/SetupFailed。窗口结束保留source60 handler，ISR会读MDIO；只有WindowEnded允许新阶段，先mask60/133/277再使用前台MDIO。无需每次板测重做旧PHY事件验收，旧窗口行为保持 |
| K3 axconfig.toml、mem.rs | allocator只从[0x180000000,0x2f0000000)分配，VA offset 0xffff_ffc0_0000_0000，device bus offset0，GMAC/APMU沿用既有MMIO映射；不向DMA写CPU VA |
| axalloc::GlobalPage | alloc_contiguous(n,align)、zero、start_paddr(axhal::mem::virt_to_phys)可直接复用；Drop自动dealloc，因此发布前后生命周期要分开 |
| platform/mod.rs、drivers/mod.rs | 新模块只在K3 cfg进入内核；host harness可直接path包含core，目标专属分配/CBO实现以riscv64 cfg隔离 |
| tests/Makefile | 已有rustc edition2024直接生产模块harness；追加k3-gmac-host-harness.rs并登记host-test，不建立新的测试判定脚本 |

资料已调查并按M42默认采信：同板tgoskits k3_gmac/{core,desc,regs,mdio,syscon}.rs、axklib/src/dma.rs、someboot RISC-V cache源码，外仓DMA/network文档与同板直连网络经验。必要事实已在本Cycle展开，Act不必回读。适配tgoskits Apache-2.0实现时保留来源；无LICENSE的Rt-Async-AMP主工程不复制。上游Linux只参考寄存器/FCS事实，不复制GPL代码。

**Implementation Guidance**

按1.1→1.2→1.3推进，每task先建立直接调用生产逻辑的RED，再实现GREEN，最后spec/code自检。core仅需局部MMIO/cache/time seam和传入DMA arena；host fake给内存、寄存器、cache trace与时间，不能另写驱动算法。使用现有页分配/ProbeEnv，不引入通用DMA框架；局部命名、等价辅助函数拆分由Act决定。

**Task Contracts**

### 1.1: 独占 arena、完整 DMA 地址与 cache owner 边界

- Requirement/Scenario: R1/R5/R6；S1/S5/S8/S9/S10/S11。
- Depends on: None。
- Targets: 新crates/axplat-riscv64-k3/src/dma.rs及lib.rs导出；新kernel/src/drivers/k3_gmac/{mod,desc,regs}.rs的arena/descriptor/cache调用；tests/k3-gmac-host-harness.rs。
- Current behavior: 已有allocator/线性地址与MMIO，无K3 DMA分配、CBO适配或GMAC descriptor。
- Required behavior:
  - 18个连续4KiB页、4KiB对齐，总73,728bytes：offset0 RX ring1页、4096 TX ring1页、8192 RX buffers8页、40960 TX buffers8页；双向16slot、buffer2048、descriptor64（16bytes字段+48padding）。ring和buffer分区不重叠；软件owner元数据在arena之外。
  - 使用GlobalPage::alloc_contiguous(18,4096)、zero和axhal virt_to_phys。checked整个span的加法、合法RAM、对齐、每slot范围；ring及next tail高位不跨4GiB。HW_FEATURE1 GMAC+0x120[15:14]编码0/1/2对应32/40/48bit，reserved拒绝；本板RAM高于4GiB，32bit能力明确拒绝，不截断。DMA address为PA，descriptor des0/1分别低/高32bit，ring同样保留高位。
  - Cache block64，CPU VA用于CBO，PA用于设备；checked range仅覆盖独占arena。Zicbom .insn i 15,2,x0,addr,1为clean，immediate2为flush，0为invalidate。flush/clean前fence，invalidate后fence；DMA发布MMIO前使用fence iorw,iorw，不能用单独fence代替cache操作。
  - RX buffer移交前flush整个2048byte，避免旧dirty line覆盖DMA；descriptor地址/控制字段写好→release fence→OWN最后→clean整个64byte descriptor，再full fence/doorbell。reap前invalidate descriptor/fence/volatile读OWN；OWN=1不读buffer；合法OWN=0后acquire fence→invalidate buffer/fence/复制CPU内容。
  - 发布前失败可正常Drop；写任一ring base前将backing转为ManuallyDrop<GlobalPage>或等价持有至复位的结构。之后正常结束/fault/cancel均不释放；不因software清ST/SR就判stop。设备拥有存储通过raw pointer/volatile访问，不保持冲突Rust引用。
- Preserve: 既有安全RAM与allocator/映射，不占用已保留CMA/早期内存；actual CBO仅K3目标运行。
- Forbidden: PMA或NC remap、IOMMU/通用DMA新框架、指纹/身份协议、地址截断、同步设备owner期间的整ring、发布后自动Drop。
- Test witness: V1先RED dma_span_and_cache_order（包括PA高位、32bit拒绝、overflow、分区/slot/check64范围、RX flush→desc clean→doorbell与reap次序）、fatal_keeps_backing的发布前/后生命周期分支。harness直接path生产模块；新增模块缺失的编译RED允许作为首个见证，随后补充行为断言。
- GREEN condition: V1原生0 failed/exit0，所有边界拒绝均不写ring/OWN；按发布点决定释放或保留的生产逻辑有直接见证。1.3的target构建和真板RX验证实际CBO执行，host不宣称DMA硬件通过。
- Verification: V1；随后1.2/1.3验证此基础，不额外建立allocator/cache测试工具。
- Stop when: 分配器不能提供指定安全连续span，或板级地址/cache事实与实际目标配置冲突；记录阻塞交Plan。

### 1.2: 有界 controller 初始化、RX/reap/refill与失败语义

- Requirement/Scenario: R1/R2/R5；S1/S2/S4/S5/S7/S8/S9/S10。
- Depends on: 1.1。
- Targets: k3_gmac::Gmac::init_rx/poll_rx及IO seam/错误/owner状态，desc.rs/regs.rs；复用phy_probe::mdio_read；同一host harness。
- Current behavior: clock/reset/PHY/link已存在，GMAC没有DMA数据面；参考代码reset超时后继续和静态speed回退不满足本次需求。
- Required behavior:
  - 已有APMU 0xd4282800+0x3ec两步和PHY reset不重复；RMW同窗口+0x3f0只改TX/RX delay enable bit16/0及phase[31:24]/[15:8]，TX0x47/RX0x53，其余保持。
  - GMAC base 0xcac82000、窗口0x2000；+0x1000 bit0 SWR用真实monotonic time等自清≤1秒。超时不写ring/start，不继续；确认reset成功后才发布arena。
  - 在mask完成、无并行ISR MDIO后，复用10ms事务读取PHY1；BMSR双读确认link/AN(bit2/5)。读取ANA4、LPA5、CTRL1000reg9、STAT1000reg10；共同能力按1000FD/HD、100FD/HD、10FD/HD优先级解码（本地1000FD/HD bits9/8对应对端STAT bits11/10，10/100以ANA&LPA bits8/7/6/5）；最多5秒等待link/AN，无共同能力或MDIO错误停止，不再reset/restartAN或静态回退。
  - reset后绝对配置：SYS_BUS +0x1004 EAME(bit11，支持>32bit时)、BLEN8/16 bits2/3、RD_OSR2<<16；CHAN_CTL +0x1100 PBLX8bit16+DSL6<<18；TX_CTL+0x1104 OSPbit4+PBL8<<16且ST=0；RX_CTL+0x1108 RBSZ2048<<1+PBL8<<16，SR初始0；IRQ_EN+0x1134=0。
  - MTL channel0为+0xd00（不是0xc00）：TX TSFbit1、TXQENbit3、TQS31<<16，FTQbit0=0；RX+0xd30绝对配置RSFbit5、DIS_TCP_EFbit6、RQS31<<20，flow-control关闭（无需采用有冲突的EHFC位）。
  - 空ring清零并同步后，高位先写低位后写：TX+0x1110/14、RX+0x1118/1c；ringlen+0x112c/30=15。MAC02:00:00:00:26:01沿R69先写LOW0+0x304=0x00000002，再HIGH0+0x300=0x80000126（AE31）；RXQ+0xa0=2，filter+0x08 PR bit0=1。MAC config+0x0无jumbo/checksum offload/ACS stripping，DM13、FES14、PS15按共同能力（1000 PS/FES=0、100两者1、10仅PS1），TEbit1=0，REbit0=1。
  - 初始空ring→MAC RE/RX SR→按R69逐slot refill16项：各slot独立buffer flush/desc clean/full fence，再RX tail+0x1128写next slot低32bit并重写SR；不用预填满后一次tail=base替代该顺序。运行后不再整体clean ring。TX始终空且TE/ST关闭。
  - poll_rx使用调用方1514byte CPU scratch，返回idle/完整帧长度及原始status/明确drop或fault。合法完成需OWN31=0、CTXT30=0、FD29/LD28=1、ES15=0，des3[14:0]原始长度64..1518且≤2048；ACS关闭，剥末4byte FCS后复制60..1514byte。ES/runt/超长/context/非完整帧不交付，单次消费后安全refill，不越界。
  - 固定16slot，每poll最多检查16项且CPU reclaim/refill≤10ms；empty是idle。ring满不覆盖未消费CPU frame，按序reap/refill、wrap；RBU7/TBU2是容量/fetch事件，refill后tail/SR恢复。+0x1160 FBE12/非预期停止导致fault，摘要位结合具体cause；W1C只清已消费cause，不RMW写回所有位。
  - submit/refill/doorbell为同步有界步骤≤10ms，分别观察deadline；OWN发布前timeout/cancel可撤销，发布后则保留owner、停止新移交并报告阶段。CPU reclaim超时只保留未移交状态；禁止把空队列当timeout。取消/fatal尝试清RE/SR及mask作为止损，不能当stop proof或软件清OWN回收。报告阶段、raw des/status、owner、长度/原因，TX terminal不在本轮。
- Preserve: 旧PHY reset/AN/window、MDIO原错误语义、DMA独占cache和backing保留。
- Forbidden: 新IRQ handler、MAC DMA IRQ enable、TX启动、危险地址注错、初始化超时继续、静态speed fallback、自动reset/recovery。
- Test witness: V1先RED reset_timeout_no_start、link_and_peer_deadlines（本轮link部分）、rx_publish_and_reap、rx_refill_after_copy、rx_status_and_length_edges、rx_full_wrap_resume、fatal_keeps_backing、phase_deadlines_preserve_owner、cancel_before_after_publish。fake完成由测试写目标descriptor，测试调用真实poll；cache/寄存器trace证明顺序，fake time分别卡住SWR/link/handoff/reclaim，不把全部超时混在一个请求deadline。
- GREEN condition: V1原生0 failed；16slot耗尽、超过16次wrap、非法长度/ES/context/FD-LD、phase/cancel路径均有断言；SWR失败无ring/start，未清OWN不读buffer/释放，合法RX复制后才refill。
- Verification: V1；1.3通过板上descriptor移动与对端对应字节后才计真实RX通过。
- Stop when: 硬件描述符/FCS/cache用法与实际证据矛盾，或需要修改已定状态、错误/取消/范围；保留现场交Plan，禁止猜写。

### 1.3: stage11 集成、兼容和真板原始帧

- Requirement/Scenario: R1/R2/R5/R6；S1/S2/S7/S10/S11。
- Depends on: 1.2。
- Targets: platform/k3.rs::run_first_byte_park/K3Env、platform/mod.rs、新platform/k3_network_probe.rs、drivers/mod.rs，harness/Makefile host-test条目。
- Current behavior: stage9 Passed才运行stage10，未检查stage10 outcome随后park；K3Env私有，routes60/133/277仍可能消费MDIO。
- Required behavior: stage9非Passed或stage10非WindowEnded保持park；正常窗口结束后先通过axhal::irq::set_enable(60/133/277,false) mask，再进入唯一前台GMAC/MDIO owner。按需传入现有K3Env并实现cache seam，不复制旧PHY算法；新板专属模块由K3 cfg隔离。
  - stage11打印初始化进度、真实raw descriptor/status、长度、首个匹配PC ARP的前60byte（或等价ARP原始字段），计数/drop原因与window结束。窗口30秒有界；若有多个帧，每次固定poll预算，打印只保留足以识别内容的样本和摘要，不建立判定器。
  - 原始stage11不回复ARP/ICMP、不运行协议栈；PC ping只作ARP刺激，其timeout不判本轮失败，真实帧缺席则不能验收RX。窗口结束或fault后保留arena并park；打印drop不等于成功接收。
  - 新harness纳入既有host-test命令，但本轮只定向运行受影响测试，不修已有全量host-test失败。触及公共cfg/平台导出则必要default/D1构建。
  - 操作者完成V3真实接收、V4无link启动和reset恢复；agent主机GREEN后到能力边界停止，人工结果未提交或失败不能完成本轮。
- Preserve: stage1–10功能和输出、单hart、RAM-only、旧平台/owner/wake路径；仅本次相关调用点注释可随接入修正。
- Forbidden: shell/串口输入/固件写入、新peer CLI/packet身份字段/输出判定脚本、下一阶段提前执行、全局状态/Runbook/Issue写入。
- Test witness: V1先RED stage11_gate_and_masks_before_mdio、stage11_bounded_rx_observation，直接生产foreground函数验证gate/mask在MDIO和DMA前、window/fault有界。修改旧表面前采信上述对应GREEN；新调用边发生变化后V2定向回归。
- GREEN condition: V1/V2通过，V3实际ARP字节对应对端抓包、至少一帧成功复制/refill、无owner/cache/DMA错误，V4明确link失败且reset回原系统。无板测只记录主机部分完成。
- Verification: 下列V1–V4；成功条件分别判定，不用ping退出码替代rawRX。
- Stop when: 任一前置失败，物理结果缺席、实际内容不符或原系统无法恢复；不继续001/002。

**Invariants**

单hart唯一前台owner；mask后才前台MDIO；独立descriptor/buffer同步；OWN不靠软件取消/timeout清除；所有等待/扫描有界；发布后backing不Drop；local状态不等于peer交付；新能力仅K3；公共规则中的TDD、三次失败与验证边界均生效。

**Non-goals**

TX/四协议本轮不执行但需求保留；不重做已验收板级探索，不写PMA、firmware、分区，不引入async/wake/第二executor、恢复或多hart，不产出测试工具/身份协议或性能结果。

**Acceptance**

| Acceptance | Requirement/Scenario/Task | 直接通过条件 |
|---|---|---|
| A1 地址与cache发布/回收 | R1；S1/S5；1.1/1.2 | 合法高位地址、边界拒绝、独占layout与cache次序测试GREEN，实际RX字节闭合 |
| A2 bounded RX正确性 | R2；S2/S4/S5；1.2/1.3 | 正常复制/refill，error/len/drop/full/wrap直接生产测试通过；物理ARP内容与peer一致 |
| A3 error/deadline/cancel安全 | R5；S7/S8/S9/S10；1.1/1.2 | reset/link/handoff/reclaim/fatal/cancel分别测试，拒绝后续操作且owner/backing保留；board link-down有界结束 |
| A4 入口和旧平台 | R6；S11；1.3 | stage gate/mask次序，相关旧harness/构建通过，新板阶段在旧阶段之后 |
| A5 RAM boot/reset与验收边界 | R6；S2/S7/S10；1.3 | 操作者V3/V4实际结果和reset一句结论；没有数据不计完成/不展开下一轮 |

映射代码和测试由上列Targets/Test witness给出；R3/R4及TX terminal/protocolwait等后轮验收不被本轮结果冒充。

**Verification**

V1：新增目标harness（先RED后GREEN）。原生命令分别执行：

~~~sh
rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test
/tmp/k3-gmac-host-test
~~~

判据：编译exit0、运行原生test result: ok /0 failed、exit0，实际production断言覆盖A1–A4。新测试必须调用生产逻辑，不只比较常量。目标实现CBO在V2编译、V3运行验证，host fake只证明调用次序。

V2：受影响边界。与新增前台gate接入直接相关的现有harness：

~~~sh
rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test
/tmp/k3-device-irq-host-test
~~~

若实际修改phy/mac/aia生产表面或其调用语义，按同一原生rustc命令执行对应tests/phy-probe-host-harness.rs、mac-probe-host-harness.rs、k3-aia-host-harness.rs；完全未变覆盖采信基线，不逐项重跑。make k3 exit0（生成临时FIT）；模块cfg/平台导出变化后make build、make lichee各一次exit0。VF2/ms04既有失败只报告，不虚报PASS。一次通过即采信，不重跑增强信心。

V3：操作者最短真实接收步骤（主机GREEN后，能力边界）。

1. 使用现有直连网口/网线，PC已有192.168.137.1/24。Wireshark在该物理网口查看arp；不改持久网络配置。串口115200 8N1无流控，把make k3生成的FIT复制到既有FAT32 USB。
2. 上电长按s进U-Boot，执行以下原有RAM命令（不saveenv）：

~~~text
usb start
fatls usb 0:0 /
fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
setenv fdt_high 0xffffffffffffffff
bootm 0x2e0000000
~~~

3. 旧stage1–10运行正常；保持/恢复网线连接，看到stage11 rx window开始后，在Windows执行原生arp -d 192.168.137.2清此条临时neighbor，再ping -n 1 -w 1000 192.168.137.2。确认Wireshark实际发出who-has192.168.137.2的ARP request；若未发出，刺激未成立，不能据此判RX故障。此阶段ping失败为预期，因为尚无协议回应。
4. 在30秒窗内串口看到合法descriptor OWN清零、原始len≥64、去FCS后ARP frame与PC抓包的source MAC、SPA192.168.137.1、TPA192.168.137.2、opcode1和内容对应；driver完成copy/refill、无fault。直接人工观察结果一句记入Act Response，保留不超过20行决定性输出，不要求截图/保存pcap。
5. 窗口有界结束并park；按reset确认原系统恢复，用一句实际结果记录。此操作结束本轮，不进入stage12。

V4：独立link-down场景，用同一临时FIT在RJ45拔开时启动一次。stage11在最长5秒link/AN观察后明确link阶段失败且不启动RX/TX；reset确认原系统恢复。不重试直到通过；这是另一目标场景，不是重复V3取信心。unsafe DMA/fatal/CRC真实注错不做，生产host注入验证错误处理，不宣称物理错误已发生。

取消人工验证仅采信实际复位/恢复；不得虚构复位前软件取消输出。软件提交前/后cancel与保留语义由V1直接验证。Act Response报告命令/操作、≤20行决定性输出、退出码或明确人工结果、所支持Acceptance。

**Gate 2 Readiness**

PASS — 调查、设计、任务/追踪、场景与验证契约完整；2026-10-03用户原话“批准”，计划获批，无豁免。Plan Context为ready；本次仅完成批准登记。

**Persisted Evidence**

- Mode: none

Act Response足以承载少量raw des/status、ARP内容及人工peer/reset结论；不创建Evidence占位目录、不要求完整日志或pcap。发现实质blocker或一次性决定性现场时按公共规则处理，不能新增判定/身份机制。

**Risks and Notes**

无待Act选择的实质未知项/TBD。真实DMA移动是本轮验收目标，尚未宣称；若CBO异常、地址/FCS/owner事实与实际输出冲突即交Plan。物理网口/USB/串口操作由操作者执行，主机GREEN不越过该能力边界。局部名称/辅助函数等非实质选择可由Act处理并记录。

非阻塞既有元数据漂移：OpenSpec CLI的project context仍输出旧branch net-k3及.claude/docs/SNAPSHOT.md。当前项目以AGENTS.md文档地图、.agents体系与上述实际代码/Cycle为准；本次不修改全局OpenSpec配置，不按CLI旧上下文重建实施基线。

## Act Response

- Status: reported

**Tasks and actual changes**

1.1 DMA arena、地址与 cache 可见性 — done（host 部分）。

- `crates/axplat-riscv64-k3/src/dma.rs`（新）：arena 常量与分区（18 页/73,728B、RX ring/TX ring/RX buf/TX buf）、`ArenaLayout::new` checked 构造（4KiB 对齐、span 加法溢出、安全 RAM 窗口、ring+next-tail 4GiB 跨界拒绝）、slot 级 checked 访问器、`decode_addr_width`（HW_FEATURE1[15:14]→32/40/48，reserved 拒绝）、`addr_fits_width`（32bit 能力对本板 >4GiB RAM 直接拒绝、不截断）、`addr_lo32/hi32`、`check_cache_range`、`ring_crosses_4g`；riscv64 `cbo` 模块（clean/flush/inval 的 `.insn i 15,2,x0,addr,{0,1,2}` 编码、`fence rw,rw`、`fence iorw,iorw`）。编码经本机 nightly-2026-02-25 工具链汇编+反汇编实测证实与契约一致。
- `platform/k3_gmac/{regs,desc,mod}.rs`（新）：寄存器/字段常量（含 MTL +0xd00 窗口、CHAN_CTL DSL<<18 等本板分叉）、RX 描述符编解码（OWN/CTXT/FD/LD/BUF1V/ES/len、runt/超长/context/非完整分类）、发布序列（fence→flush 2048B→地址/控制→release fence→OWN 最后→fence→clean 64B→fence iorw→tail doorbell）、回收序列（inval desc→fence→volatile OWN→OWN=0 acquire fence→inval buffer→fence→复制）、`ArenaGuard`（发布前 Drop 释放/发布后持有至外部复位）。

1.2 有界初始化、RX 与失败 owner — done（host 部分）。

- `platform/k3_gmac/mod.rs`：`Gmac::init_rx`（能力检查先于一切写、APMU +0x3f0 RMW 只改 TX/RX delay 域、SWR ≤1s 超时不写 ring/start、超时前发布点、复用 `phy_probe::mdio_read` 的 BMSR 双读 + ANA/LPA/CTRL1000/STAT1000 ≤5s 链路解码与优先级、SYS_BUS/CHAN/TX/RX/IRQ_EN/MTL 绝对配置、ring 清零同步→ring base 高字先写→ringlen=15→MAC 02:00:00:00:26:01 LOW→HIGH(AE)→RXQ map→PR→MAC config（RE，TE=0，PS/FES/DM 按能力）→RX SR→逐 slot refill（tail+SR 重写×16））；`Gmac::poll_rx`（每 poll 单帧、状态 W1C 只清已消费位、RBU 容量事件恢复、FBE=fault、空队列=Idle 永非 timeout）；失败语义（发布前 deadline/cancel 撤销移交，发布后保留 owner、revoke 走 bounded_stop 关闭扫描、fault/stop/cancel 均不清 OWN 不释放 backing）。
- `tests/k3-gmac-host-harness.rs`（新）：FakeEnv（MDIO C22 引擎、SWR 时序/APMU RMW 模型、cache 代价、窗口内设备完成脚本）+ 14 个 witness 测试（契约 V1 全清单：dma_span_and_cache_order、fatal_keeps_backing、reset_timeout_no_start、init_rejects_32bit_before_any_write、link_and_peer_deadlines、rx_publish_and_reap、rx_refill_after_copy、rx_status_and_length_edges、rx_full_wrap_resume、phase_deadlines_preserve_owner、cancel_before_after_publish、fatal_keeps_backing_on_failure_paths、stage11_gate_and_masks_before_mdio、stage11_bounded_rx_observation）。

1.3 stage11 接入 — done（host 部分；板测留操作者）。

- `platform/k3_network_probe.rs`（新）：`Stage11Env`（GmacEnv+路由掩码）、`run_stage11`（stage10 非 WindowEnded 即 gate 拒绝且零访问；mask 60/133/277 先于一切前台 MDIO/DMA 访问；30s 有界窗口、样本打印上限 8、首个 ARP 请求打印 raw descriptor/status/长度/前 60 字节、drop 计数与 summary 行、FBE=fault、deadline/cancel 有界终止、backing 全路径保留）；board glue（GlobalPage 18 页零化 backing、K3Env 实现 GmacEnv/Stage11Env，CBO 走 k3 crate dma::cbo，mask 走 axhal::irq::set_enable）。
- `platform/k3.rs`：`run_k3_stage10` 返回值改为消费并传入 `run_k3_stage11`（gate）；stage9 非 Passed 分支保持 park。`platform/mod.rs`：登记 k3_gmac/k3_network_probe（均 cfg k3）。`crates/axplat-riscv64-k3/src/lib.rs`：导出 `pub mod dma`。`Makefile`：host-test 登记新 harness。

**Deviations from Plan**

- 模块位置：契约 Targets 写 `kernel/src/drivers/k3_gmac/`，实际落位 `kernel/src/platform/k3_gmac/`。原因：k3 构建形态（kernel/src/lib.rs cfg）在 feature k3 下整体排除 `drivers` 树（MS09/MS10 起的 minimal park 内核），驱动挂在 platform 才能进入 make k3 编译。文件内容、测试与 harness `#[path]` 均等价；drivers/mod.rs 注册相应移至 platform/mod.rs。非实质、可直接定位的等价搬移。
- refill 尾指针语义：契约"RX tail写next slot低32bit"按 DWMAC 恢复语义实现为"tail := 本次 refill 的 slot 自身地址"（初始化逐 slot 递进、消费后 refill 同 slot 再 doorbell）；板测 V3 可裁决。
- STAT1000 对端 1000 能力位按契约取 bits11/10（CTRL1000 本地 bits9/8 对应）；与 IEEE 802.3ab reg10 常见 bits15/14 布局存在出入，Plan 契约为准、未擅自改位。若板测 1000M 链路被解码为无共同能力，`NoCommonAbility` 错误携带 ANA/LPA/CTRL1000/STAT1000 原始值可直接裁决。
- 闭环内自检修复：poll 的取消/超时撤销后 head 描述符处于半发布状态，若继续扫描会误报 NotComplete——revoke 统一走 bounded_stop 关闭驱动（Stopped），设备持有 slot 不动；取消检查点放在帧交付之后、OWN 发布之前（已完成的 CPU 帧不被取消吞掉）。
- 过程记录：`make k3` 首次通过时输出被管道过滤丢失，重跑一次并保留完整 FIT 输出（两次均 exit0）；采信第二次为记录输出。

**Self-check findings and remaining minors**

- 已修复：见上条撤销语义；另有三处 harness 侧构造错误（arena 未对齐、overflow 用例未对齐、40 位越界地址取值）在 RED 阶段被生产逻辑正确拒绝后修正——均属测试构造，非产品缺陷。
- 遗留 Minor：FakeEnv 不建模 cache 数据面（cache op 只计时间与踪迹），发布/回收的字节级可见性由 V3 板测裁决；ring 清零+clean 的次序踪迹未单独断言（由代码路径与 V1 发布序列断言覆盖）。

**Verification**（V1/V2；V3/V4 板测未执行，见能力边界）

- V1：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test` 编译 0 error 0 warning，exit 0；`/tmp/k3-gmac-host-test` 输出 `test result: ok. 14 passed; 0 failed`，exit 0。RED 已见证：缺模块编译错（首见证）+ dma_span 首跑 FAILED（overflow 用例构造错误）后修正 GREEN。
- V2：`k3-device-irq-host-harness` 编译 0 error 0 warning，`test result: ok. 17 passed; 0 failed`，exit 0（新增 stage11 对 stage10 契约的消费不改变其覆盖面）。phy/mac/aia 生产表面未改动，按基线采信不重跑。`make k3` exit 0（生成 StarryOS_riscv64-k3.fit，FDT 150,998B @0x181000000）；`make build` exit 0（qemu-virt release 完成）；`make lichee` exit 0（android boot image pack/inspect 完成）。
- V3（操作者执行，2026-10-03 第 3 轮板测，串口原文见 Evidence `board-stage11-v3-rx-arp.txt`）：`stage=11 begin … masks=60/133/277` → `init LinkConfig { speed: S1000, full_duplex: true } rx-running` → 真实 ARP 请求帧 `arp-req slot=1 len=60 raw=0x30010040 data=ffffffffffff…0806…0001…c0a88901…c0a88902…（60 字节逐字节与刺激一致，原始长度 64 去 FCS）` → `result frames=8 drops=0 revocations=0 arp-sample=1 window-elapsed-ms=30000`。附证：STAT1000 bit11 位图实证成立（1000FD 解码）；PC 后台广播帧（280/120B）同样被正确接收分类。支持 A1/A2/A4。
- V4（操作者执行，同日，串口原文见 Evidence `board-stage11-v4-linkdown.txt`）：拔线启动，stage10 `result=no-device-event` 正常结束，stage11 在 ≤5 s 内 `err=init LinkTimeout { bmsr2: 31113 }`（0x7989，link/AN 位均 0）有界终止；无 `rx-running`、无 RX 窗口、无帧行。支持 A3/A5 有界失败分支。
- A5 的两项操作者一句话确认（两轮 reset 后原系统恢复、V3 轮 Wireshark 对端同帧一致性）由用户显式豁免，原话保留："不阻塞在这里，想验证的都拿到了，给出豁免，只管回复就好"（2026-10-03）。风险：上述两项未单独确认即计入本轮验收；缓解事实——RAM-only 启动不写持久状态（reset 回原系统为 MS09 已验收路径），且串口 ARP 帧内容与刺激逐字节自洽。任务 1.1–1.3 的 Map 复选框保持未勾选，Iteration 验收由 Plan Review 依据上述证据与豁免裁决。

**Persisted Evidence**: mode=none（计划），另存 user-required 证据（用户在 reported 后明确要求保存板测输出；白名单与结论见 `evidence/000-initial/000-initial/README.md`）：

- `evidence/000-initial/000-initial/board-stage11-v3-rx-arp.txt`（V3 串口原文节选）
- `evidence/000-initial/000-initial/board-stage11-v4-linkdown.txt`（V4 串口原文节选）

**Experience Candidates**: None。

**Unresolved / capability boundary**

- 无未决项。A5 的 reset/对端一致性确认按上述用户豁免（原话已保留）闭合。
- 已由板测裁决、不再存疑的事实：Zicbom 实际执行与可见性（真实帧字节一致）、STAT1000 bits11/10 布局（1000FD 解码成立）、refill tail 语义（逐 slot doorbell 后设备持续取帧）、真实 RX descriptor 移动（8 帧零 drop）。
- 板测结果已提交，下一步由 Plan 核对与 Review：PASS 且 accepted 才展开 Iteration 001。

## Plan Review

- Review Result: accepted（2026-10-03；Iteration 000 完成）

**Findings**

独立检查：完整阅读 tracked diff（Makefile、axplat-riscv64-k3 lib.rs、k3.rs、platform/mod.rs）与全部新增文件（dma.rs、k3_gmac/{regs,desc,mod}.rs、k3_network_probe.rs、tests/k3-gmac-host-harness.rs 共 14 测试）及 Evidence 三件；Act 声明与代码逐项一致；采信 Act Response 的 V1（14 passed/0 failed、exit 0）与 V2（device-irq 17 passed、make k3/build/lichee exit 0）未失效结论，未重跑。只读 git 检查确认工作树文件集与 Act 声明完全一致，结论产生后覆盖面未变化。

偏差裁决（分类）：

- 模块落位 `platform/k3_gmac/`（契约写 drivers/）：Act 偏离，理由核实成立——`kernel/src/lib.rs:39` 在 feature k3 下确实排除 `drivers` 树。非实质等价搬移，接受。
- RX tail 写"本次 refill slot 自身地址"（design D3 写 next slot）：Act 偏离，已被 V3 板测裁决（逐 slot doorbell 后设备持续取帧，8 帧零 drop）。按新证据接受，该语义成为后续轮次的已裁决基线事实。
- STAT1000 bits11/10：契约按计划实现；V3 实证 1000FD 解码成立，Act 的 IEEE 位布局疑虑已由板测消除。
- 撤销语义闭环修复（revoke 走 bounded_stop、取消检查点在帧交付后 OWN 发布前）：契约范围内的非实质完善，有直接测试见证（cancel_before_after_publish、phase_deadlines_preserve_owner），接受。

非阻塞 Minor findings（不要求返工）：

1. `Stage11Summary.revocations` 永不递增：revocation 路径直接以 Stopped 结束、不产 summary，字段恒为 0（V3 日志即 revocations=0）。死字段，无行为影响；可在后轮删除或填充。
2. poll_rx 的 FBE 路径把整读出的 status 原字写回 `0x1160`，比 w1c_status 的"只清已消费位"更粗（读与写之间新置位的位会被未观察清除）。驱动随即进入终态 Faulted，后果有界；无板测触发。后轮收敛为同 w1c 掩码即可。
3. MAC 地址寄存器内容映射沿 design D3 与 R69（LOW@0x304=mac[0..4] LE、HIGH@0x300=mac[4..6]|AE）；通用 DWMAC4 资料（Linux stmmac）把寄存器内容映射相反。本轮 PR+广播 ARP 无法区分且 A1–A5 不依赖；Iteration 001/002 依赖名义 MAC（单播过滤、期望源 MAC）前应在板上核实有效语义。stage12 的 ARP 请求源 MAC 由软件构建、应答经 PR 接收，不受影响。已写入 001 Cycle Risks。
4. `decode_link` 注释写"BMSR double-read"，实现是 10ms 循环重读、首读即 up 即确认；latch-low 语义下单次 up 读可靠，V4 证实 down 锁存报告正确（bmsr2=0x7989 保留）。注释与实现的措辞差异，非实质。

**Acceptance Gaps**

None。A1/A2/A4 由 V3 支持（真实 ARP 帧逐字节一致、raw=0x30010040 解码正确、8 帧零 drop、masks 先行、1000FD 解码）；A3 有界失败分支由 V4 支持（≤5s LinkTimeout、无 rx-running）；A5 的两项操作者一句话确认（两轮 reset 恢复、V3 轮 Wireshark 对端一致）按用户显式豁免闭合——原话"不阻塞在这里，想验证的都拿到了，给出豁免，只管回复就好"（2026-10-03）已保留于 Act Response，风险与缓解（RAM-only 不写持久状态、reset 路径为 MS09 已验收路径、ARP 帧逐字节自洽）记录在案，豁免仅覆盖该两项，不延续到后轮。

**Evidence**

- 代码：上述全部新增/修改文件，独立阅读核对。
- Act Response V1/V2 结论（来源注明，未重跑）；用户要求另存的 Evidence：`evidence/000-initial/000-initial/{README.md, board-stage11-v3-rx-arp.txt, board-stage11-v4-linkdown.txt}`，白名单第一条（用户明确要求）成立，内容与 Act 声明一致。
- 参考交叉核实：R69 `tgoskits/.../net/k3_gmac/core.rs:469-475`（MAC 地址写法与实现逐字一致）、`regs.rs`（GMAC_ADDR_HIGH0/LOW0=0x300/0x304）；`kernel/src/lib.rs:39`（drivers 排除）。

**Follow-up Decision**

既有 Acceptance A1–A5 全部满足，偏差均有正当理由或板测裁决，仅余非阻塞 Minor（已记录，其中第 1/2/4 条可在后轮顺带收敛，不构成本轮修复义务；第 3 条转为 Iteration 001 Risks 注记）。Iteration 000 按 Map 完成，tasks 1.1–1.3 勾选。

**Iteration Plan Update**

None（Map 不变）。

**Next Cycle**

None（accepted，无 rework/replan）。

**Next Iteration**

`iterations/001-raw-tx/000-initial.md`（本轮展开，Plan Context 为 draft，等待用户 Gate 2 审计）。
