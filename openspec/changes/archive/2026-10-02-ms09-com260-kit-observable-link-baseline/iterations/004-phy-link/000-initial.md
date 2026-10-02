# Iteration 004 / Cycle 000: PHY 与物理 link

## Plan Context

- Status: ready
- Cycle Type: initial
- Planned: 2026-10-02

**Cycle Scope**

- Change tasks: 5.1–5.2；Iteration Map 不变。
- Acceptance gaps: None（initial）。
- Repair items: None。
- Inherited scope: proposal 已批准的 T15；delta spec「MAC 寄存器与 PHY/link 构成独立可观测链路」「K3 平台选择不改变既有平台」；D4–D6；公共规则及 M42 来源要求。
- Excluded scope: packet I/O、MAC IRQ/DMA、异步 NIC、SMP、吞吐、持久固件写入、全局状态维护、Runbook/Issue 落账。

**Objective**

在已接受的 MAC MMIO 基线上取得实际 PHYID、可解释的 Clause 22 状态与两次物理插拔循环；MDIO 超时、PHY 不可读、协商中和 link down 各有独立结果，结束后停驻。

**Investigation Facts**

### Current Baseline

- 工作树 HEAD `046a519b`；当前产品修改已提交，规划开始时未提交差异仅为 003/001-rework 的记录修复和 accepted Review。revision 只定位历史现场，不参与行为判定。
- Iteration 000–003 accepted。003/001-rework 实际串口：APMU before=0、after1=9、after2=b；version=0x1054×2、debug=0×2。用户接受 debug 零值并豁免该轮第二次启动及 reset 观察；本轮不补跑这些已裁决项，也不把旧豁免延伸为 PHY/link 豁免。
- 采信 003/001-rework Act Response 和独立 Review 中的真实 GREEN：mac harness 11 项、`make k3`、`make build`、`make lichee` exit 0；early-console 6 项未失效。002 的 VF2 为既有构建失败，不得计为通过或在此扩展修复。
- 现状没有 MDIO/PHY/link 探针；没有 PHY 实际读值和本轮物理插拔结果。旧 Linux carrier 不能替代 StarryOS 观察。已有 RAM 启动和返回原系统路径可复用。

### Current-State Evidence / Code and Critical Path

| 位置、入口或来源 | 已读事实与本轮影响 |
|---|---|
| `kernel/src/entry.rs::init` → `platform/k3.rs::run_first_byte_park` | K3 在 axruntime early init 后进入单 hart polling 路径，stage=1/2/3 后 APMU、MAC 探针，最后 wfi；不启动网络任务或 rootfs。 |
| `kernel/src/platform/mac_probe.rs::{apmu_clock_enable,probe_and_report}` | APMU 0x3ec 两步 RMW，仅 clock/mode/reset 位；MAC 0x110/0x114 各读两次，无 MAC 写入。APMU 当前等待是 `us*50` 估算循环；通用分类会把 debug=0 标成 all-zero。 |
| `kernel/src/platform/mod.rs` | k3/mac_probe 当前无条件编译，调用受 K3 分流控制；改动这些模块会影响非 K3 构建面。新 PHY 模块仅接到 K3 入口。 |
| `crates/axplat-riscv64-k3/{axconfig.toml,src/time.rs,src/time_math.rs}` | 已映射 UART/MAC/APMU；没有 GPIO/pad 窗口。TimeIf 读取 RISC-V `time`，DTB 与已采集 Linux 均为 24 MHz，u128 比例换算；early/percpu 初始化无需 IRQ。 |
| 依赖 `axhal-0.3.0-preview.2/src/time.rs`、`axplat-0.3.1-pre.6/src/time.rs` | axhal 重导出 `current_ticks/nanos_to_ticks/busy_wait/Duration`；busy_wait 用已有时钟轮询，不要求定时器中断。可直接复用，禁止新增估算循环频率或校时工具。 |
| `tests/{mac-probe,early-console}-host-harness.rs`、根 `Makefile` | rustc 原生 host harness 可直接包含纯探针模块；host-test 已接入两者。新增测试需驱动实际事务/状态逻辑，不能只测同一份期望常量的副本。 |
| `tools/{k3_com260_ifx.dtb,starry-k3.its}` | 板匹配 DTB 已由 001/002 核实；FIT kernel 0x180000000、DTB 0x181000000，stage 0x2e0000000；本轮保持这些地址和只写 RAM 路径。 |

数据流：K3 polling console → 已有 clock/MAC 基线 → pad/GPIO 受限操作 → DWMAC4 MDIO read/write → PHYID/BMCR/BMSR → 串口状态变化 → 120 s 结束/wfi。只有一个 hart 调用；无中断、异步并发或共享 driver 状态。MMIO 使用现有 U32 volatile 访问方式；访问异常可能直接陷入或静默，最后一个 begin 标记定位故障层，不能声称软件能捕获总线挂死。

### 板级事实与来源等级

参考仓库根为 `/home/daivy/projects/serial/work/k3`，只读。下表将实际使用字段写全，Act 不需沿分析文档回读才能实施。

| 字段 | 本轮采用事实 | 来源与等级 |
|---|---|---|
| 实例 | MAC `0xcac82000/0x2000`，APMU `0xd4282800/0x400`，CTRL 0x3ec；mode=rgmii，PHY 外部晶振 | 当前板 001 采集、003 真板读值；DTB 交叉验证；不是从候选板名推断。 |
| PHY | MDIO 地址 1；DT compatible `ethernet-phy-id001c.c916` + ieee802.3-c22 | 当前板采集与 DTB；预期身份，不冒充已经读出的 ID。上游 Realtek 注册 0x001cc916 为 RTL8211F，见来源下文。 |
| GPIO reset | 控制器 `0xd4019000/0x100`，bank=1、pin=5、active-low；pre/assert/post=0/20000/100000 μs | 当前板匹配 DTB。R69 `net/k3_gmac/syscon.rs::GpioReset` 给出同板复位路径，源注追溯 U-Boot/Linux。 |
| GPIO 访问 | bank stride=0x40；GPSR=0x08、GPCR=0x0c、GSDR=0x1c；只写 bank1 的 bit5 | R69 `net/k3_gmac/regs.rs` K3 常量，明确追溯 gpio-spacemit-k1.c 的 K3 布局；符合 M42 的同板实现来源，不推定 K1 全集。 |
| pad | MFPR base `0xd401e000/0x400`；pins 21–35 mux1，pin37 mux0；offset=pin*4，MUX mask=7 | DTB gmac1-cfg 与 R69 `soc/k3/pinctrl/{regs,mod}.rs`；参考源注追溯 Linux K3 pin offset/mux。 |
| 电气 | gmac1-cfg power-source=1800；IO PD base `0xd401e800/0x34`，pins21–41 对应 offset0x0c、1.8 V bit2 | DTB + R69 pinctrl 布局。只读取该位和 pad 原值；不重设电压、drive、pull、slew、schmitt 或 edge 位。 |
| MDIO | ADDR 0x200、DATA 0x204；busy bit0，C45 bit1=0；GOC read=3<<2/write=1<<2；PHY[25:21]、reg[20:16]、CR[11:8]；DATA低16位 | R69 mdio/regs 与 Linux v6.18 上游 DWMAC4 交叉验证。 |
| divider | CR=5，R69 同板路径注明 U-Boot eqos 配置，定义适用 CSR 250–300 MHz | 可追溯同板配置依据；没有本会话 CSR 时钟实测，不宣称已测得该频率。出现 MDIO 不可读不自行改 divider/时钟。 |
| Clause 22 | BMCR=0、BMSR=1、PHYID1=2、PHYID2=3；BMCR reset=0x8000、loopback=0x4000、AN enable=0x1000、powerdown=0x0800、isolate=0x0400、AN restart=0x0200；BMSR link=0x0004、AN capable=0x0008、remote fault=0x0010、AN complete=0x0020 | Linux v6.18 `include/uapi/linux/mii.h`；标准字段，非 vendor page。 |

本轮阅读并采信的范围：R61 网络/GPIO资料说明静态资源和缺口；R69 提供寄存器操作；R70 的板匹配 DTB 给当前实例；R71 draft 网络操作资料只提供排障背景，不采信其中未验证步骤；R72 网络 baseline 是文档来源契约，不作为 StarryOS 已运行证据。官网网络正文及 SpacemiT 源码的直接访问本次未成功，未把缺失正文升级为事实；已取得的上游通用代码只支持通用字段。

原始来源链接：

- [Linux v6.18 DWMAC4 offsets](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/ethernet/stmicro/stmmac/dwmac4.h)、[MDIO fields setup](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/ethernet/stmicro/stmmac/dwmac4_core.c)、[MDIO transactions](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/ethernet/stmicro/stmmac/stmmac_mdio.c)：本次已直接阅读，支持字段与事务次序；10 ms deadline 与其 wait 上限相同。
- [Clause 22 定义](https://raw.githubusercontent.com/torvalds/linux/v6.18/include/uapi/linux/mii.h)、[genphy_update_link](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/phy/phy_device.c)：支持 BMCR/BMSR 及 latch-low 处理；上游在 polling 且旧状态为 up 时保留短暂 drop，本轮保留首次读值供诊断，第二次用于当前电平，不复制完整 phylib。
- [Realtek PHY 注册](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/phy/realtek/realtek_main.c)：0x001cc916 以 exact match 注册 RTL8211F；不由该名称推定内部延时、strap、电气或数据通路已验证。

**BDD 场景扫描**

| ID / 类型 | 前置状态 → 动作 → 可观察结果；失败边界 |
|---|---|
| S1 Happy | MAC 身份已可读、板资源符合契约 → 最小 mux/reset + 读地址1 ID → 输出 raw ID1/ID2、可解释身份与 BMCR；不把 DT 的 ID当成实读。 |
| S2 Sad | 初始或提交事务后 busy 持续 → 等待10 ms → 报 idle/completion timeout 与 raw ADDR；没有 DATA有效结果或后继协商。 |
| S3 Sad/Edge | 事务完成但身份0/全一或其他型号 → 报 raw ID 与 unavailable/unexpected-phy → 停止，无地址扫描、无 BMCR写。 |
| S4 Sad/Edge | 电压域非1.8 V、mux写回不符、MAC身份不可读 → 报 board-control/MAC层 → 停止，未执行依赖它的 GPIO/MDIO写；不试电压或 clock新配方。 |
| S5 Happy | 已识别 PHY、正常 AN capable 状态，对端启用AN → 启AN/restart一次并观察 → AN完成且link=1时输出 up；AN restart未清除或AN未完成时不误报up。 |
| S6 Sad | 无线缆/无启用对端 → 观察 PHY始终可达但link=0 → 输出down/协商中；窗口结束说明未见up，PHY验收与link验收分别判定。 |
| S7 Edge | BMSR首次读为down、第二次为up → 当前电平取第二次，保留两次raw值/latched drop；不把旧锁存值作为当前down。 |
| S8 Happy/Edge | 同一对端 → 断开/重连两次 → 两次up→down→up可见，AN完成才up；不通过重置PHY制造down。 |
| S9 Error/Timeout | 观察中MDIO失败、BMSR全一/全零、remote fault，或启动时reset/powerdown/isolate/loopback异常 → 分别报告访问/PHY状态；访问及异常控制状态停止，remote fault报告down/fault并继续有界观察；不回用旧up。 |
| S10 Cancel/Compatibility | 操作者取消或观察结束 → 停驻或按已有路径复位；持久介质不写。非K3构建 → 不调用PHY入口，既有平台行为保持。 |

**Implementation Guidance**

依赖顺序为 5.1 host RED→最小实现/GREEN→5.2 host RED→实现/GREEN→受影响构建→真板操作。测试只为实际探针逻辑提供可替换的 MMIO/时钟访问；不引入 GPIO/PHY 框架、通用 CLI、独立判定脚本或运行归属协议。变量与内部函数拆分由 Act决定。

**Task Contracts**

### 5.1: 有界 MDIO 与可解释 PHY 身份

- Requirement/Scenario: 可观测链路与平台兼容；S1–S4、S9 的初始异常、S10；D4/D6。
- Depends on: 003/001-rework accepted。
- Targets: `kernel/src/platform/{k3.rs::run_first_byte_park,mac_probe.rs,mod.rs}`；新增 `kernel/src/platform/phy_probe.rs`；`crates/axplat-riscv64-k3/axconfig.toml`；新增 `tests/phy-probe-host-harness.rs`、受影响的 mac harness 和根 `Makefile::host-test`。
- Current behavior: stage5 后wfi，无GPIO/pad/MDIO；估算APMU延时，debug0仍被粗分类为all-zero。
- Required behavior:
  1. 保留stage1–5既有原始观察和APMU两步掩码/次序；APMU等待100 μs改用已有时间接口。身份0/全一或APMU clock/reset位未生效时报告并停止后续PHY阶段；debug0保留raw值并按idle状态解释，不作为身份失败。分类API若保留原粗分类，则必须额外明确寄存器语义，不能输出debug0故障结论。
  2. axconfig增加GPIO `0xd4019000/0x100` 和覆盖两个pad资源的单一页 `0xd401e000/0x1000`；避免两个资源分别映射同一页。访问范围仍限DT声明资源内，不扩大为任意MMIO接口。
  3. stage6先读IO PD `0xd401e80c`并确认bit2；全一或bit2=0停止且不改电压。pins21–35和37只按表做必要MUX RMW，原MUX正确不写；逐项写后读取确认低3位，其他位保持。访问前输出begin；读值/失败定位到pin或阶段。
  4. GPIO按R69序列仅写bit5：GSDR bank1 offset0x1c设output → GPSR置高 → 等2 μs（DT pre0，沿同板参考最短pre）→ GPCR置低 → 等20000 μs → GPSR置高 → 等100000 μs；每次启动至多一个硬件reset序列，GMACclock先就绪，完成标记在post等待后输出。禁止自动追加软复位或循环复位。
  5. stage7 Clause22每事务先等待idle最多10 ms；read仅写ADDR，完成后读DATA低16位；write先写DATA再ADDR，完成最多10 ms。CMD使用地址1、寄存器0..31、CR5及相应opcode，C45=0；非法寄存器拒绝且不写。超时报idle/completion、reg、rawADDR，停止；不返回伪0/ffff作为成功，也不读取未完成DATA。
  6. 读取PHYID1/2并输出原始值。合成0/0xffffffff或任一word=0xffff视为unavailable；0x001cc916识别为RTL8211F；其他非空ID报unexpected-phy后返回Plan，禁止访问新地址或继续配置。BMCR和BMSR可读后报告状态，BMCR reset/PDOWN/isolate/loopback置位、BMSR不支持AN或全零/全一时停止并报告，不推测vendor配置。
- Preserve: 原FIT和安全RAM地址、单hart/polling/非K3选择、GPIO其他line与pad非MUX位、电压域及APMU非目标位、DTB原文件。
- Forbidden: clock provider移植、GPIO/pad电压或电气配置重写、DLINE/WOL变更、PHY扩展page、全地址扫描、DMA/IRQ/packet配置、参考仓库写入。
- Test witness: 原生新harness先建立S1–S4和初始异常测试，执行 `rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test` 后直接运行二进制，观察缺失实现或行为不符的RED；行为变化的mac语义测试也必须先观察RED。APMU延时复用以测试中的时钟/等待调用与真实参数见证，禁止以真实睡眠速度校准。
- GREEN condition: 对实际事务和板级序列的host测试全绿；超时前后访问边界、DATA/ADDR顺序、mux仅低3位、GPIO仅bit5/一次复位、未识别PHY禁止BMCR写均受测试；`make k3`成功且真板实读ID/BMCR/BMSR符合S1。
- Verification: 下文V1–V3；host模拟失败不冒充真板故障注入。
- Stop when: 寄存器布局/时钟divider依据被实际现象否定、mux电气状态不符、GPIO访问故障、超时或ID不符；保留最后原生输出，按层报告并返回Plan。不得换参数试到成功。

### 5.2: 协商与两次物理插拔可观察

- Requirement/Scenario: 可观测链路和兼容；S5–S10；D4/D6。
- Depends on: 5.1 hostGREEN及正常PHY身份/控制状态路径；板上PHY失败时不执行本任务下游。
- Targets: `kernel/src/platform/{phy_probe.rs,k3.rs::run_first_byte_park}`、`tests/phy-probe-host-harness.rs`；不改通用网络栈。
- Current behavior: 无协商/link状态读取。
- Required behavior:
  1. 通过5.1后，BMCR只以保留其他位的RMW置ANENABLE|ANRESTART（0x1200）一次；读取并输出现有advertisement寄存器4/9供诊断，但不重写advertisement、不强制speed/duplex、不软复位。
  2. stage8在120 s窗口每100 ms取样；使用已有current_ticks/nanos_to_ticks的真实经过时间，单个MDIOdeadline不被重读busy重置，窗口也不因插拔延长。每次读BMCR及BMSR两次；保留两次BMSR原值，第二次决定当前link位；BMCR ANRESTART=1或AN尚未完成时结果为negotiating/down，不报up。首次down后二次up可报告latch已清；首次down的短暂drop不能被描述为持续当前down。
  3. 初次状态和状态变化必输出，至少包含raw BMCR/BMSR两读、link/AN状态；稳定状态可不重复打印。MDIO错误或无效PHY状态立即输出并退出观察，禁止复用上次up；remote fault明确输出并视为非up，允许继续到窗口截止。
  4. 120 s结束输出observation-ended与最后状态后wfi；若全程未up则明确no-link-observed。到时不自动判本轮PASS，也不自动boot/reset。线缆断开是正常PHY状态，不触发PHY重置。
- Preserve: 5.1错误分层和有界事务、已识别实例、无对端下MDIO仍可访问、原平台行为。
- Forbidden: 通过LED/ping替代BMSR结论、只看AN complete忽略link、把等待到时写成成功、自动重试故障或改PHY地址、扩展到RGMII调相或收发包。
- Test witness: 新harness先为S5–S9建立RED：两次BMSR含latch-low、restart未清/AN未完成、120 s无up、插拔两循环、观察中超时/无效值/remote fault。假时钟驱动实际deadline和采样逻辑，覆盖恰在截止点退出；不能仅测枚举格式化。
- GREEN condition: 全部host测试exit0；V3真板同一启AN对端两次up→down→up均可见，无对端和MDIO错误分层；V2受影响平台构建保持基线。
- Verification: V1/V2/V3；两次插拔是既有重复link Acceptance，不是重复运行已PASS命令以增强信心。
- Stop when: 正常对端在窗口内未能完成两次循环、持续不能AN/up、访问失败、实际PHY需vendor配置或MAC数据面才能解释；报告已观察结论和未满足Acceptance，返回Plan，不裁剪重复要求。

**Invariants / Non-goals**

保持公共规则的范围、测试见证、直接判定和单一权威位置。早期同步polling仅用于有界板级探针，不修改async设备所有权/waker/恢复语义。不建立NIC驱动或包通路；link up只证明PHY对端物理协商。不给验证新增Hash、run-id、匹配协议或外部判定器。

**Acceptance / RTM**

| Acceptance | Requirement / Scenario | Design / Task | Code / test / 判定 | Coverage |
|---|---|---|---|---|
| A1 实读且可解释PHY身份 | 可观测链路 / S1、S3 | D4/D6 / 5.1 | phy_probe + host事务测试 + V3真实ID | Implemented（计划覆盖） |
| A2 必要板控制有来源、有限写入 | 可观测链路 / S4 | D6 / 5.1 | axconfig、pad/GPIO/APMU实际序列测试 + V3阶段 | Implemented（计划覆盖） |
| A3 错误和无对端不混淆 | 可观测链路 / S2–S4、S6、S9 | D4/D6 / 5.1–5.2 | timeout/invalid/state测试 + V3down | Implemented（计划覆盖） |
| A4 同一对端重复up/down | 可观测链路 / S5、S7、S8 | D6 / 5.2 | latch/deadline/transition测试 + V3两次插拔 | Implemented（计划覆盖） |
| A5 临时启动和既有平台保持 | RAM启动、平台兼容 / S10 | D2/D3/D6 / 5.1–5.2 | K3入口、构建、原RAM操作与已有恢复结论 | Implemented（计划覆盖） |

此处Coverage描述规划映射，实施状态仍pending；没有Simplified、Deferred或Missing requirement。已接受debug裁决只修正文义，不降低本轮PHY/link Acceptance。

**Verification**

### V1 原生 host 测试

- 新PHY：`rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test`，随后 `/tmp/phy-probe-host-test`；编译与测试exit0、原生`test result: ok`，对应S1–S9。
- 受影响MAC：`rustc --edition=2024 --test tests/mac-probe-host-harness.rs -o /tmp/mac-probe-host-test`，随后 `/tmp/mac-probe-host-test`；debug语义变化、APMU保留位及延时调用GREEN。
- 不改early-console，其6项既有GREEN直接采信；不跑含无关既有失败的全量host-test。仅把新harness接入该入口，方便后续按需运行。

### V2 受影响构建

- `make k3`：exit0，FIT原生输出kernel/FDT load地址保持；因新映射/逻辑确有变化，需要新的K3构建。
- `make build`、`make lichee`：新模块及k3/mac_probe接线会参与编译，分别exit0。无需另启QEMU网络测试，本轮未改其运行入口。
- VF2若非K3共用编译表面变化则执行`make vf2`，原生失败若与002既有签名相同只记未退化，不能计为PASS；出现新签名则阻塞兼容验收。不为本轮修复既有平台缺陷。
- 本轮未改FIT布局/地址，无需重新运行已接受的完整ELF/FIT资格检查。若产物大小增长达到16 MiB kernel/DTB间隔，返回Plan处理，不能忽略实际重叠。

### V3 真板直接观察与能力边界

前置：使用当前已确认CoM260 Kit目标RJ45、可靠线缆和一个已启自动协商的以太网对端；该物理连接沿当前Linux目标接口的已知连接关系使用，不通过试写其他MAC选择网口。若接口与物理口仍有歧义，先由操作者用原系统确认连接口再启动；未确认时板上步骤不可执行。host测试和构建由agent完成；搬运USB、操作U-Boot和插拔由现场操作者执行，未收到实际结果不得标记5.1/5.2完成。

1. 将V2产出的FIT放USB根目录，开始时线缆不插。上电/复位长按`s`进入U-Boot；依次执行 `usb start`、`fatls usb 0:0 /`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。确认各命令成功和stage1–5原基线、stage6复位完成、stage7真实PHYID及控制状态；不执行saveenv或其他持久写。
2. 等stage8窗口开始，未插线状态可观察为PHY可读且down。插上同一对端，见AN完成且up；拔线见down；重新插上见AN完成且up。再拔线见down、再插上见up，共两次完整up→down→up。每次等待串口状态后才做下一动作；若120 s不足，当前观察不PASS，报告操作耗时供Plan调整窗口，不循环到通过。
3. 见observation-ended后停驻；操作者取消时也可用既有普通复位返回原系统。恢复路径已有真实证据，本轮不把再次验证reset作为额外Gate；若实际恢复失败则如实报告并停止。操作者用一句话记录实际插拔顺序、状态和结果，保留不超过20行决定性串口片段于Act Response即可；无需填表、截图或新增证据工具。

通过条件：A1/A2真实输出成立；开始无对端下down可解释；两次循环A4全部观察；本轮故障模拟A3 hostGREEN；V2兼容结论成立。只有PHY通过而未观察重复link时，5.1可完成，5.2保持未完成，Cycle不能reported为整体完成。

**Gate 2 Readiness**

Gate 1：沿获批proposal及已接受用户裁决PASS；Gate 2：PASS，调查、设计、场景、任务、追踪和验证契约已闭合，用户于2026-10-02明确批准本次计划，原话：“批准”。板上运行是Act批次中的能力边界；本次未执行任何实施或真板验收。

规划状态：资料与代码调查 completed；BDD/设计/任务契约/RTM completed；文档一致性检查 completed；实施5.1/5.2 pending；实施后的Gate 3/4与独立Review pending。未跳过本轮要求；既有结果采信和旧豁免的适用范围见Current Baseline。

文档验证：`openspec validate ms09-com260-kit-observable-link-baseline` → `Change 'ms09-com260-kit-observable-link-baseline' is valid`，exit0。完整读取新增Cycle与本轮design/spec/tasks diff，确认5.1/5.2仍未完成；批准后当前Cycle为ready/pending/pending，未改冻结的003 Plan Context。

**Persisted Evidence**

- Mode: none

可重复的串口行为及原生host/build结果由Act Response摘要承载；不存在必须保存结构化现场才能决定的Acceptance。本轮不创建Evidence占位目录。若发生不可重取的实质故障现场，按公共规则交接，而非预建完整日志收集。

**Risks and Notes**

- CSR实际频率、GPIO/pad handoff和PHY strap未在本轮现场实测：采用M42允许的同板可追溯来源与已确认实例，执行契约已经选定；失败时有明确停止条件，不授权Act选择另一配方。
- RGMII tx/rx phase、内部延时和PHY vendor电源策略不影响本轮所声称的PHY对端协商边界。若实际证明其为当前目标的必要前置，返回Plan调查，不在Act临时扩展。
- 原UART单字符等待仍可因硬件故障挂住；本轮不声称总线访问或输出具有可捕获的软件超时，最后阶段标记用于直接诊断。
- 原有reset/重复启动豁免仅按对应Cycle范围采信；本轮重复物理link要求未豁免。

## Act Response

- Status: blocked

5.1/5.2 的 host 实现、测试见证与构建验证已完成；2026-10-02 真板第一轮（V3 步骤 1）停在 stage6 IO-PD 电压域 preflight——读值 0x00000000 触发按契约设计的停止。经对 R69 参考实现的补充核对，该 preflight 所依赖的 Plan 事实被实际现象否定，属实质问题，按阻塞交接返回 Plan。

### Blocker Handoff

**发现位置**：task 5.1 Required behavior item 3（stage6 IO-PD preflight）；V3 步骤 1，Gate 4 板上判定。

**Plan 预期 vs 实际**：

- Plan 预期（Contract 5.1.3 / 板级事实表）：读 `0xd401e80c` 应见 1.8 V 域 bit2=1（依据标注"DTB + R69 pinctrl 布局"），bit2=0 或全一则停止。
- 实际串口（用户现场采集）：`stage=6 iopd read=0x00000000 err=voltage-domain stop`——读访问本身成功（非全一、无总线挂死），值为 0。
- 补充核对 R69（`tgoskits/drivers/ax-driver/src/soc/k3/pinctrl/{regs,mod}.rs`，本轮 Act 完成）：
  - `pin_to_io_pd_offset`：pins 21–41 → `GPIO2_KX = 0x0c` ✓，偏移与 bit 位置（`IO_PWR_DOMAIN_V18EN = BIT(2)`）无误；
  - 但参考实现**从不读取 io_pd**（全仓无 `io_pd.read`）：该寄存器的唯一用法是 `set_io_pwr_domain`——先向 APBC（`0xd4015000`，asar offset 0x50）写 `ASFAR=0xbaba`、`ASSAR=0xeb10` 做 AIB 安全解锁（解锁后允许恰好一次 io_pd 访问，之后自动重新上锁），再**写** `1V8=BIT(2)` / `3V3=0`（pinctrl-k1.c:479-509 追溯）。
  - 即"读 bit2 应为 1"在参考实现中没有依据；Linux 对该寄存器的行为是 GMAC init 时**设置**而非验证。复位后读到 0 与 3V3 复位默认（或锁定读零）一致，不能区分，也与 DTB `power-source=1800` 的配置意图（由软件设置）不矛盾。

**影响**：stage6 的电压域 preflight 以一个不可满足的读值条件拒绝了后续 GPIO 复位与 MDIO/PHY/link 全部下游；Task Contract 5.1.3 的"只读取该位"与"不符预期则停在板级控制边界"本身被正确执行，但该检查无法在当前契约下通过，5.1/5.2 的板上验收无法继续。

**任务状态**：

- 已完成：5.1/5.2 host 实现、RED（5.1）、两 harness GREEN（32+15 项）、V2 四构建、openspec validate。
- 部分完成：5.1 板上部分——stage1–5 原基线逐字复现（apmu before=0/after1=9/after2=0xb；version=0x1054×2 class=ok；debug=0×2 sem=idle）、stage6 gate=proceed、`pad/gpio begin` 正常输出，随后停在 iopd preflight。
- 未开始：GPIO 复位、stage7、stage8（被 gate 正确阻止）；5.2 板上观察全未执行（无需插拔线缆）。

**工作区状态与已过 Gate**：修改文件 = Act Response「实际改动」所列 7 个产品/测试文件 + 2 个 change 文档，无计划外文件；工作树未提交（与进入本 Cycle 时一致）。Gate 1/2（Plan 已批）、Gate 3（逐任务自检完成）、Gate 4 host/build 部分全部通过；Gate 5 本轮命中阻塞交接。当前行为证据：用户现场串口（本轮对话完整保留）+ Act Response 摘要。

**Evidence**：None required——串口输出可由既定 RAM 启动路径低成本复现，决定性片段已内嵌本 Handoff，未建 Evidence 目录。

**恢复条件与候选项（由 Plan 裁决，Act 不预选）**：

1. 修订 stage6 契约：以 DTB `power-source=1800` + R69 写序为来源等级允许的依据，取消"读 bit2=1"preflight（可保留读值作为 raw 观测输出，不作为停止条件）；
2. 或将"AIB 解锁 + 写 BIT(2)"纳入契约——这是对电压域控制面的写入，超出当前 change"不重设电压"的边界，需要明确的设计决策与用户批准；
3. 或维持读检查并接受其阻塞（视为板级事实未证实，返回更早的调查阶段）。

任一选项确定后按公共规则创建 replan/rework Cycle；板上操作从 stage6 重启即可，stage1–5 无需重验（本轮串口已复现 accepted 基线）。

### 实际改动

- 新增 `kernel/src/platform/phy_probe.rs`：`ProbeEnv` 探针缝（MMIO/原生时钟）、MDIO 事务核心（10 ms 单次 deadline；读仅写 ADDR 后取 DATA 低 16 位；写先 DATA 后 ADDR；非法寄存器拒绝且零访问；不读未完成 DATA）、PHY 身份/BMCR/BMSR 纯分类、stage6 `run_pad_gpio`（IO-PD 只读电压检查 → gmac1 pad mux 最小 RMW 且写回校验 → 恰一次 GPIO1_5 硬复位，仅写 bank1 bit5）、stage7 `run_phy_identity`、stage8 `run_link_observation`（一次保留其他位的 AN restart RMW、reg4/9 只读诊断、120 s 固定窗口每 100 ms 采样 BMCR+BMSR×2，第二次 BMSR 决定当前电平）。
- `kernel/src/platform/mac_probe.rs`：`apmu_clock_enable` 改经 `ProbeEnv` 原生时间接口等待（100 μs×2，删除估算自旋 `delay_us`），返回 `ApmuOutcome`；新增 `apmu_bits_effective`、`MacProbeSummary::version_identity_ok`、`debug_sem_label`/`format_debug_line`（debug=0 报 `sem=idle`，不作故障结论）、`format_gate_line`；`probe_and_report` 返回 summary。stage4/5 原始行形状、写序与读数不变。
- `kernel/src/platform/k3.rs::run_first_byte_park`：stage4/5 后新增 stage6 gate（APMU clock/reset 位与 MAC 身份不全零/全一才放行，否则停驻）、stage6/7/8 调用；新增 `K3Env`（volatile 访问限已裁决 DT 窗口 + axhal 单调时钟/busy_wait）。stage1–3 输出不变。
- `kernel/src/platform/mod.rs` 注册 `pub mod phy_probe`；`crates/axplat-riscv64-k3/axconfig.toml` `mmio-ranges` 增加 GPIO `0xd4019000/0x100` 与单一 pad 页 `0xd401e000/0x1000`（避免同页双重映射）；根 `Makefile::host-test` 接入 `tests/phy-probe-host-harness.rs`（新增），`tests/mac-probe-host-harness.rs` 扩展（debug sem、env 化 APMU 序列与真实 100 μs 参数见证、有效性、gate 行、summary）。

### 与计划的偏差

- TDD 顺序偏差（非阻塞，如实记录）：`phy_probe.rs` 首版将 5.1 与 5.2 实现一次写入，5.2 的 host 测试后于实现产生，未形成严格 RED。补救证据：5.2 测试仍抓到一个真实实现缺陷——`observe()` 在最后一个采样槽与固定 deadline 之间不等待，零成本假时钟下死循环、真板上会在窗口尾部违反 100 ms 采样节奏；已修复（尾部睡满到 deadline 再退出）并全量重跑 GREEN。5.1 的 RED（两 harness E0583 编译失败，exit 1）与最终 GREEN 见证完整。
- 契约解释（非阻塞）：「每次读BMCR及BMSR两次」按"BMCR 一次、BMSR 两次"实现，依据其操作子句「保留两次BMSR原值，第二次决定当前link位」；采样行携带 raw BMCR 与两读 BMSR，满足 5.2.3「raw BMCR/BMSR两读」下限。Plan Review 如按另一读法要求 BMCR 亦两读，按 rework 处理。
- 等价调整：`format_gate_line` 增加 `proceed` 参数（调用方以同一事实推导）；`bmcr_anomaly_labels` 返回 `([label; 4], count)`（no_std 无 Vec）；max 复用 `push_u16_hex4`/`push_u32_hex8` 共享格式助手（`pub(crate)`）。

### 自检发现（Gate 3）

- 已修复（均测试脚手架，不涉产品代码）：假板 GOC 字段与未移位常量比较导致读事务被当写、pad 写损坏钩子误在正常路径生效、强制 BMSR 值缺 AN-capable 位、两循环 latch-cleared 计数（三次插入各自清除一次）、MDIO DATA 写断言把读触发误计、`access_cost=0` 下挂死场景需非零访问成本。
- 产品代码无遗留 Critical/Important；vf2 日志 17 条 warning 均为既有表面预存，无一涉及本次改动文件。

### 验证（Gate 4）

- RED（5.1）：`rustc --edition=2024 --test tests/{mac,phy}-probe-host-harness.rs` → 均 E0583（`phy_probe.rs` 不存在），exit 1，无产物。
- V1 GREEN：phy harness `test result: ok. 32 passed` exit 0；mac harness `test result: ok. 15 passed` exit 0（含 APMU env 序列复现 board 形状 before=0/after1=9/after2=b 与真实 100_000 ns×2 等待参数、debug sem、有效性、gate 行）。early-console 六项未触碰，按 002/003 既有 GREEN 采信；未跑含既有失败的全量 host-test。
- V2：`make k3` exit 0（FIT kernel/FDT load/entry 0x180000000/0x181000000 不变，kernel 144 KiB ≪ 16 MiB 间隔，无需重跑已接受的 ELF/FIT 资格）；`make build` exit 0；`make lichee` exit 0；`make vf2` exit 2 —— 50 errors、签名 `unresolved imports axfs/axnet/AxTaskExt` 与 002 既有基线一致，pre-existing 未退化，不计通过。
- `openspec validate ms09-com260-kit-observable-link-baseline` → valid，exit 0。

### Persisted Evidence

None required（模式 none；未创建占位目录，可重复结论由本 Response 与两个 harness 承载）。

### Experience Candidates

None（本次发现的实质缺陷均在当前 change 范围内并已修复；`make vf2` 既有失败已随 002 记录为 Issue 候选，无新增范围外缺陷）。

### 等待事项（恢复条件）

已被上方 Blocker Handoff 取代：V3 步骤 1 已执行并停在 stage6 IO-PD preflight，本 Response 状态为 `blocked`。恢复由用户调用 `openspec-plan` 按 Blocker Handoff 的候选项裁决后进行；Plan/Act 不得在当前契约下换参数重试（Task Contract 5.1 停止条件）。

## Plan Review

- Review Result: rework-required

**Findings**

1. 阻塞（Plan 错误 + 新证据）：Task Contract 5.1 item 3 的 IO-PD 读前置（「读 `0xd401e80c` 应见 bit2=1」）无来源依据——板级事实表把「电气」行标注为「DTB + R69 pinctrl 布局」，但两个来源只支撑布局（偏移/位），不支撑行为预期。2026-10-02 真板第一轮 `stage=6 iopd read=0x00000000 err=voltage-domain stop`（读访问本身成功，非总线挂死）。本 Review 独立核对 R69：`pinctrl/regs.rs:64-75`（`IO_PWR_DOMAIN_V18EN=BIT(2)`、ASAR 偏移 `+0x00/+0x04`、魔钥 `0xbaba/0xeb10`，追溯 pinctrl-k1.c:75-81）与 `pinctrl/mod.rs:164-182`（`set_io_pwr_domain`：ASFAR→ASSAR→io_pd **整字写**，解锁后恰好一次访问即写，非 RMW）；全仓 io_pd 仅 `mod.rs:181` 一处 write、无任何 read。RAM 启动路径上无人预置 1.8V（Linux 未运行、U-Boot 网络未用），该读前置在当前契约下不可满足；M42（2026-10-02 修订）明文禁止发明参考实现从不执行的验证前置。000 的 Stop when 被正确执行，Act 无偏离。
2. 非阻塞（Act 偏离，已记录并补救）：5.2 host 测试后于实现，未形成严格 RED；补救中测试捕获真实缺陷（观察窗尾部不等待导致假时钟死循环）并修复后全量重跑 GREEN（32+15 项）。
3. 非阻塞（Plan 遗漏，措辞歧义裁决）：5.2.2「每次读BMCR及BMSR两次」按「BMCR 一次、BMSR 两次」实现，采纳该读法——操作子句明确「保留两次BMSR原值，第二次决定当前link位」，latch-low 是 BMSR 专属语义；`001-rework` 契约写清。
4. 非阻塞 Minor（并入 rework 顺手修复）：`tests/mac-probe-host-harness.rs:264` EOF 空行（`git diff --check` 命中）；`K3Env::mmio_write` 的 SAFETY 注释写寄存器清单需随 5.1-R1 扩展。
5. 采信（独立核对后）：Act 声明与代码一致——`phy_probe.rs` 全文、`k3.rs`/`mod.rs`/`axconfig.toml`/`Makefile`/`mac_probe.rs` diff、两 harness 测试数（32/15）逐项核对无出入；000 的 V1/V2 GREEN 与 `openspec validate` 结论未失效。

**Acceptance Gaps**

- A1（实读且可解释 PHY 身份）：板上 stage7 未执行——被 stage6 前置正确阻止。
- A2（必要板控制有来源、有限写入）：IO-PD 步骤方法错误（无来源的读验证）；需按 R69 用法重建为有来源的有限写。
- A4（同一对端两次 up→down→up）：stage8 板上未执行。
- A3/A5：host 层成立；板上面待 A1/A4 解锁后一并观察。

**Evidence**

- 板上现象与层定位：000 Act Response「Blocker Handoff」（用户现场串口 `stage=6 iopd read=0x00000000 err=voltage-domain stop`）。
- R69 用法语义（本 Review 直接读取）：`tgoskits/drivers/ax-driver/src/soc/k3/pinctrl/regs.rs:60-118`、`mod.rs:132-182`；io_pd 全仓唯一访问点为 `mod.rs:181` 的写。
- 板 DTB 独立复核（`dtc` 反编译 `tools/k3_com260_ifx.dtb`）：`system-controller@d4015000`（`spacemit,k3-syscon-apbc`，reg `0xd4015000/0x1000`，phandle 0x82）；pinctrl 节点 `spacemit,apbc = <0x82 0x50>`——asar offset 0x50 与 R69 一致；`gmac1-cfg power-source = <1800>`。
- 代码/diff 独立核对：Findings 第 5 条所列文件（本会话完整阅读）。
- host 基线：000 Act Response Verification（RED→GREEN 32+15、`make k3`/`build`/`lichee` exit 0、vf2 既有签名、validate valid）。

**Follow-up Decision**

修复需解除 000 Forbidden（「GPIO/pad 电压或电气配置重写」）并把 IO-PD 步骤重建为有来源写序（新增 APBC 窗口映射），超出当前 Cycle 有限修复边界；Iteration 目标、tasks 5.1–5.2、Acceptance A1–A5 与 Iteration Map 均不变，属 design D4 预设的「资源与 handoff 证实需要时修订阶段契约」路径 → 创建 rework Cycle，repair item `5.1-R1`。恢复方向按 M42（2026-10-02 修订）对齐 R69 用法（即 Blocker 候选项 ②），design D6 已同步修订。

**Next Cycle**

`iterations/004-phy-link/001-rework.md`（同目录；Cycle Type: rework；Plan Context 自 `draft` 起，待用户批准后置 `ready`）

**Next Iteration**

None（当前 Map 最后一个 Iteration；Iteration 004 未完成）
