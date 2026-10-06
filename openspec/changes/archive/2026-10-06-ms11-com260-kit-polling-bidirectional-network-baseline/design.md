## Context and current-state evidence

调查现场：2026-10-03，分支 `k3`，HEAD `a504b0de`。本次只创建 change 文档，产品表面无未提交改动。MS09 已闭合 RAM boot、安全 RAM、APMU、GMAC version 与 RTL8211F/link；MS10 已闭合 AIA 软件投递和 PHY source60 事件。MAC source133/277 只有观察路由，不是 DMA IRQ 验收。

| 实际入口或资源 | 当前事实与本次影响 |
|---|---|
| `axruntime::rust_main` → `src/main.rs::main` → `kernel/src/entry.rs::init` | runtime 已初始化 allocator、内存管理、平台与调度器，再进入应用。K3 分支调用 diverging `k3::run_first_byte_park`，不会进入 QEMU axnet/UART 用户程序路径 |
| `kernel/src/platform/k3.rs::run_first_byte_park` | stage1–10 后 WFI park；stage4 两步 APMU、stage6 PHY 硬复位只执行一次；stage8/10 窗口均 30 s；没有 DMA 或网络栈 |
| `mac_probe.rs`、`phy_probe.rs::ProbeEnv/mdio_read/mdio_write` | GMAC `0xcac82000/0x2000`；APMU `0xd4282800+0x3ec`；PHY1、CR5、MDIO busy deadline10 ms；可复用 MMIO/time seam，不再复位 PHY、不改通告 |
| `k3_device_irq_probe.rs::run_stage10/source60_handler` | 窗口结束仍保留 handler/route；handler 使用 MDIO 消费 INSR。前台 DMA 阶段必须先 mask60/133/277，避免前台与 ISR 并行 MDIO |
| K3 `axconfig.toml`、`mem.rs` | allocator RAM `[0x180000000,0x2f0000000)`，线性 VA offset `0xffff_ffc0_0000_0000`，bus offset0；MMIO 沿用既有映射，不将 CPU VA 写入 DMA |
| `axalloc-0.3.0-preview.2/src/page.rs::GlobalPage` | 已有连续页分配、零填充、VA/PA 转换；Drop 自动释放。发布后需要禁止自动 Drop，不能把 park/清 ST/SR 当 stop 证明 |
| `kernel/src/drivers/mod.rs`、`kernel/Cargo.toml` | K3 未接入 axnet；新增 K3 局部后端与独立 polling session，不启动现有异步 queue/stack owner |
| `crates/smoltcp/src/phy/mod.rs::Device/RxToken/TxToken`、`examples/loopback.rs` | 已有 no_std Ethernet、IPv4、ICMP、UDP、TCP；Device receive 要同时提供 RX 与 TX token，consume 无错误返回，因此先保留容量，错误存入设备并由前台消费 |
| `crates/axdriver_net/src/lib.rs` | 现有 transport-neutral queue/reset owner 契约服务异步网络；本次不接入 reset epoch、任务或 waker，也不改变该共享接口 |

基线直接采信：MS10 `iterations/001-phy-device/000-initial.md` accepted Review 的 device harness17 passed、MAC15 passed、`make k3/build/lichee` exit0 和板上 stage1–10、复位回原系统；其后 `f7044e12` 提交记录的 PHY31 passed、device17 passed、`make k3` exit0（stage8 begin 文案修复）。只读检查确认无后续产品改动。Plan 补充编译 `rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/ms11-phy-probe-host-test`，exit0；未重复运行已通过测试。`make host-test` 的 ms04 migration 既有失败、VF2 的 axfs/axnet/axtask 既有导入失败不计通过，也不顺带修复。

## Reference use and limits

本仓 R66 信息价值表、R68 RAM boot Runbook 和 M42 用法对齐约束作为调查输入；默认采信 traceable 官方资料和同板 R69，保留明确 unknown/inference 的边界。

- `/home/daivy/projects/serial/work/k3/docs/dma/{k3-dma-and-memory-ownership,k3-cache-pma-address-translation}.md` 与 `docs/network/k3-gmac-dma-irq.md`：地址、同步、owner 与通知边界。
- 同仓 `others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/{core,desc,regs,mdio,syscon}.rs`、`components/axklib/src/dma.rs`、`platforms/someboot/src/arch/riscv64/mod.rs`：实际 contiguous DMA、64字节 descriptor/DSL6、完整高位地址、Zicbom 与 tail/ST/SR 用法。tgoskits Apache-2.0，可按来源保留归属；无 LICENSE 的 Rt-Async-AMP 主工程只借鉴机制，不复制代码。
- `tools/k3_com260_ifx.dtb` 实际反编译：X100 `zicbom`、cbom block64；DWMAC5.10a、8 KiB 双向 FIFO、delay-line TX `0x2f47`/RX `0x3553`；MAC 地址全零。本节点没有 `iommus`/`dma-coherent` 属性。同板代码将线性 PA 作为 DMA 地址；这里采用本节点的直接映射，不推广为全 SoC 无 IOMMU。
- 同仓 `.claude/runbooks/K3 CoM260 首次真机接口、通信与固定网络配置报告.md`：Windows 直连 RJ45，PC `192.168.137.1/24`、板 `192.168.137.2/24` 曾实际成功。沿用拓扑，不修改已保存网络配置或持久固件。
- RX FCS 细节补查上游 [stmmac_main.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/net/ethernet/stmicro/stmmac/stmmac_main.c)：ACS 禁用时从完成长度扣除4字节 FCS。寄存器交叉检查 [dwmac4.h](https://raw.githubusercontent.com/torvalds/linux/v6.18/drivers/net/ethernet/stmicro/stmmac/dwmac4.h)。只采用事实，不复制 GPL 实现。

参考中的负面样本：reset 超时后继续、PHY 协商失败后静态回退、TX `min(len,2048)` 截断均不采用。R69 flow-control 位与上游定义有差异，本次关闭 flow-control，因此不消费该位，也不需要新增板测前置。已有 PHY/link 探索不重做为新 Gate。

## D1: 最小接入与单一 owner

新增 `kernel/src/drivers/k3_gmac/{mod,regs,desc}.rs`，纯队列逻辑使用局部 MMIO/cache/time seam；目标实现使用 volatile MMIO、axhal time 和 K3 cache 操作。host harness 直接 `#[path]` 包含生产逻辑，fake 只提供硬件、内存和时间，不复制驱动算法。新增 `kernel/src/platform/k3_network_probe.rs` 连接既有入口；K3 cfg 限定依赖和模块。

stage10 正常 `WindowEnded` 后先 `axhal::irq::set_enable(source,false)` mask60/133/277。失败的 stage9 或 stage10 setup/overflow 不进入 stage11。mask完成后前台是唯一 GMAC/MDIO owner；保持已有 handler 和诊断，不改变已验收阶段。stage11 原始 RX、stage12 原始 TX、stage13 协议 session 顺序运行；首轮只到 stage11。前两轮验证完成再启用后一阶段，不新增运行配置/CLI。

选择局部复制接口而非导入 somehal/rdrive/rd_net 或接入 axnet 全局初始化：本轮结果是硬件轮询基线，引入异步框架会增加未批准 owner/wake/reset 依赖。之后 MS12 可消费此后端事实，当前不预建其接口。

## D2: DMA 内存与 cache

每方向16 slots、每 buffer2048字节、每 descriptor64字节（4个 little-endian u32 +48字节 padding），硬件 DSL6，即16+6×8=64。一个连续18页 arena：RX ring1页、TX ring1页、RX buffers8页、TX buffers8页，总73,728字节，4 KiB对齐。ring 与 buffer 分区无重叠；每 buffer 和 descriptor 独占 cache block，软件 owner 元数据不在 DMA arena 中。两方向 ring base 和下一项 tail 均不得跨4 GiB高位边界。

复用 `GlobalPage::alloc_contiguous(18,4096)`、zero 与 `axhal::mem::virt_to_phys`。检查整个 span 的溢出、安全 RAM 范围、对齐、无重叠及硬件 DMA 宽度（HW_FEATURE1[15:14]，编码0/1/2对应32/40/48bit；reserved拒绝）。支持40/48bit时启用 EAME，descriptor和ring都写PA低/高32bit，不截断。安全 RAM 不包含早期保留区/CMA；不重新映射 NC，不写 PMA，不新增 DMA/IOMMU 框架。

新增 K3 平台 `dma.rs` 提供 checked64字节 cache range 与 Zicbom clean/flush/invalidate；使用同板 `.insn i 15,2,x0,addr,1/2/0` 对应 clean/flush/invalidate。CPU地址用于 CBO，device地址只用于 descriptor/MMIO。range 必须位于独占 arena，checked align/end，不扩展到软件元数据。

| owner 转换 | 必须完成的顺序 |
|---|---|
| TX CPU → device | 写完整 buffer（必要时补零到60字节）→ fence/clean buffer → 写descriptor地址/控制字段 → release fence → OWN最后 → clean整个descriptor → `fence iorw,iorw` → tail doorbell/ST |
| RX CPU → device | 不再持有 slice → flush整个独占buffer，防止旧 dirty line 回写覆盖 DMA → 地址/BUF1V → release fence → OWN最后 → clean descriptor → full fence → RX tail/SR |
| device → CPU | invalidate descriptor → fence → volatile读取OWN/status；OWN仍置位只返回pending，不读buffer。OWN清零且RX合法 → acquire fence → invalidate相应buffer → fence → 读/复制内容；软件单次消费后再补充 |

不在设备 owner 期间写descriptor或buffer，不用整片可变 Rust slice覆盖含设备 owner 的arena。硬件异步修改通过 raw pointer/volatile边界访问，不保留与设备访问冲突的引用。

发布前失败允许正常释放页；写ring base前将 backing 放入 `ManuallyDrop<GlobalPage>`（或等价持有至复位的本地所有权结构）。发布后包括正常 session结束都保留。故障尝试 mask中断并清 TE/RE/ST/SR 是止损，不是 stop proof；不能重新使用或自动释放。真正 reset/recovery/stop proof 留给MS13。

## D3: controller 初始化

沿用已有 APMU clock/reset，不关钟，不再pulse PHY reset。RMW APMU `+0x3f0`，仅修改 TX/RX delay enable(bit16/0)与phase字段[31:24]/[15:8]，写TX0x47、RX0x53，保留其余字段。

SWR `GMAC+0x1000 bit0` 等待自清最长1秒，使用真实 monotonic time，失败禁止写ring/start。复用 MDIO busy10ms；双读BMSR确认link和AN complete，读advertise4/LPA5/CTRL1000reg9/STAT1000reg10，依据本地与对端共同能力按1000FD、1000HD、100FD、100HD、10FD、10HD选择速度/双工；不存在共同能力或MDIO失败明确停止，不默认1000M。复位后 PHY不变，但重新读取当前link。down/AN未完成给最长5秒观察窗，超时输出link阶段。

| GMAC offset | 配置与先后 |
|---|---|
| `0x1004` | 绝对写 EAME(bit11，按宽度)、BLEN8/16(bits2/3)、RD_OSR=2<<16 |
| `0x1100/04/08` | PBLX8 bit16 + DSL6<<18；TX OSP bit4+PBL8<<16，RX RBSZ2048<<1+PBL8<<16；ST/SR初始清零 |
| `0x1134` | channel IRQ enable=0；本轮不接受MAC IRQ作为完成依据 |
| `0xd00/0xd30` | TX TSF bit1、TXQEN bit3、TQS31<<16，FTQ bit0清零；RX RSF bit5、DIS_TCP_EF bit6、RQS31<<20，flow-control关闭 |
| `0x1110/14/18/1c` | TX/RX ring高位先写、低位后写；ringlen `0x112c/30`=15；全部内存同步后才发布 |
| `0x300/304/0xa0/0x08` | MAC `02:00:00:00:26:01`（DT无可用地址，局部管理地址）：LOW0=0x00000002、HIGH0=0x80000126（AE bit31）；RX queue0 DCB=2；filter PR bit0，协议由栈过滤 |
| `0x0` | 软件checksum、无jumbo/TSO/VLAN；ACS关闭，正常RX保留FCS，软件剥离4字节。DM/PS/FES按协商值；stage11 RE=1、TE=0；stage12再开TE/ST |
| `0x1128/0x1120` | RX/TX tail使用next slot低32bit，配合已写base高位；发布后沿R69重写SR/ST恢复fetch，不为doorbell建立读回确认协议 |

先reset成功→bus/MTL→初始空ring及cache→MAC/link→SR/RE→逐slot refill与RX tail。初始空ring整体同步后发布base；随后沿R69逐slot同步并写next tail/SR，不把预填满ring后单次写回base的tail当作等价初始化。设备启动后仅同步当前独占descriptor，不再clean整ring。首轮TX ring保持空、TX引擎关闭。后轮启用TX并沿相同发布顺序提交。所有状态来自实际MMIO和descriptor，不把写入读回一致当包完成。

## D4: 队列、错误与 deadline

固定16 slots，无动态完成队列。RX按顺序 reaping；合法单descriptor需OWN=0、CTXT=0、FD/LD=1、ES=0，原始长度64..1518且不超过2048；剥FCS后交付60..1514。短帧、ES、context、分片或超长帧明确drop并安全refill，不能把丢弃当有效接收。空队列是idle；耗尽不覆盖未消费帧，RBU后补充、tail/SR恢复；每次最多检查16项。

TX接口接受完整Ethernet frame14..1514，14..59补零至60，硬件生成FCS；0..13或1515以上拒绝而非截断。`try_submit` 返回submitted/Full/length/fault；Full不改当前slot。Free→PreparedCpu→DeviceOwned→CompletedCpu→Free，inflight计数和索引共同防止full/wrap混淆，完成仅消费一次。TX ES返回local error并在OWN清零后合法reclaim；OWN=1时禁止软件清除。仅本地终态不证明peer收包。

| 阶段 | 上限与失败语义 |
|---|---|
| controller reset / link | 1秒 /5秒；reset失败不发布，link失败不启动数据面 |
| MDIO事务 | 沿用10ms；超时含寄存器/阶段，不静态回退 |
| submit及doorbell | 同步有界步骤，10ms；发布前检查deadline/取消可撤销；OWN发布后失败保留owner并fault，不能虚构doorbell硬件ack |
| TX terminal | 每次提交1秒，未清OWN则terminal-timeout并保留；poll仍有固定预算，不忙等到无限 |
| RX/TX CPU reclaim/refill | 每次调用最多16项且10ms；只计实际CPU-owned推进，empty是idle。超时保留尚未移交的owner，不把terminal/reclaim混为一阶段 |
| 原始RX/协议服务 | stage11接收窗30秒；stage13接收新会话窗60秒，单个UDP/TCP操作/协议响应5秒。正常等待deadline之外不循环重试取通过 |

channel `0x1160` FBE或非预期停止导致fault；RBU/TBU作为容量/fetch事件处理，摘要位结合具体cause判读，不能仅AIS误判成fatal。消费已读出的可清状态按W1C语义，不RMW写回整寄存器。报告stage、原始des/status、长度、owner与drop/error；不生成hash、run-id或验证身份协议。

取消使用前台测试/session内部取消入口（host可直接调用）；未提交返回资源，提交后结束等待但保留设备owner并停止新提交。物理板操作者复位取消时agent仅采信实际reset结果，不宣称复位前输出；无需新增串口输入或shell。

## D5: 协议复用与真实对端

Iteration002 在 kernel K3 feature添加可选本地 smoltcp0.13.1，`default-features=false`，只启用medium-ethernet/proto-ipv4/socket-icmp/socket-udp/socket-tcp/auto-icmp-echo-reply/socket-tcp-reno。不启用test-seeds、async、DHCP或offload；不修改smoltcp协议实现。新增局部 `polling_net.rs`，静态有界socket storage、UDP4包/4096byte双向buffer、TCP4096byte双向buffer、单连接，无线程/第二executor。

DeviceMTU1514（IP MTU1500），checksum全软件。Device每次poll先reclaim TX；无TX free slot不交付RX token pair，保留RX待下一poll。TX token保留一个CPU-owned slot，consume写完整内容再发布；token未consume则释放保留槽。RX token先拷贝到独立CPU scratch并refill；不同时借用设备仍拥有的buffer。consume无法返回驱动错误时记录fault，前台poll后立即读取并结束session，不把失败当协议成功。

拓扑沿同板经验：PC已有网口192.168.137.1/24、K3临时192.168.137.2/24，静态MAC如上，无gateway/DNS。stage12发20个正常ARP who-has PC的请求（间隔100ms，每个terminal最长1秒），确认TX超过ring容量完成wrap并收到PC应答；这也是原始双向基线，不发自定义探测协议。stage13提供UDP26001和TCP26002 echo（UDP单请求/TCP单连接累计最多1024bytes），PC发ASCII `ms11-udp`/`ms11-tcp`，板回完整相同内容。TCP按流即时echo，用offset推进partial send/recv，peer FIN后核对累计接收/发送内容、板被动close并poll至Closed或5秒失败；不等待请求结束才echo，也不把第一段当全量。PC `ping -n 20 192.168.137.2` 验证ICMP与连续RX/TX wrap；ARP请求/应答在抓包中分别可见。

host测试通过Cargo原生 integration test覆盖生产Device/session以及真实smoltcp+内存对端；不以协议栈自己的loopback测试冒充硬件。新增 `tests/k3-polling-net-host/` 独立Cargo package（workspace排除的必要调整），依赖本地smoltcp最小features，直接path引入生产模块，不复制网络算法。后轮Cycle展开前核实新增后端实际接口与 accepted基线，不让当前Act预调查后轮代码。

## Verification and scenario coverage

| 场景 | 直接见证和真板通过条件 | 逻辑Iteration |
|---|---|---|
| S1 | `dma_span_and_cache_order`、`rx_publish_and_reap`，真板OWN清零+实际ARP字节 | 000 |
| S2 | `rx_refill_after_copy`，串口原始ARP字段与PC抓包一致 | 000 |
| S3 | `tx_complete_once`，PC抓到板ARP完整帧，本地terminal另外报告 | 001 |
| S4 | `rx_full_wrap_resume`、`tx_full_wrap_no_overwrite`；stage12的20帧与stage13 ping20跨16slot | 000–002 |
| S5 | `rx_status_and_length_edges`、`tx_length_no_truncate`覆盖合法边界/异常且无越界；板测验证普通合法帧，不要求破坏PHY或制造CRC故障 | 000–001 |
| S6 | `polling_protocol_roundtrip`及PC ARP/ICMP/UDP/TCP原生观察，串口内容/对端包逐协议对应 | 002 |
| S7 | `link_and_peer_deadlines`与板上未连接启动一次明确link失败、reset回原系统；协议无响应由生产session host测试覆盖 | 000、002 |
| S8 | `reset_timeout_no_start`、`fatal_keeps_backing`、异常descriptor生产逻辑注入；不向真板写非法DMA地址注错 | 000–001 |
| S9 | `phase_deadlines_preserve_owner` fake time分别触发handoff/terminal/reclaim/协议wait，断言阶段和owner | 000–002 |
| S10 | `cancel_before_after_publish`及session cancel；操作者reset恢复原系统一句实际结论 | 000–002 |
| S11 | 相关旧harness、K3/default/D1构建，board stage1–10照常后进入新阶段；既有VF2/ms04失败单列 | 每轮按变更面 |

自动判据使用rustc test或Cargo test原生 `0 failed`+exit0；K3编译/打包使用 `make k3` exit0。仅在共用cfg/依赖表面变化时运行 `make build`、`make lichee`；未变验证直接采信。具体当前轮命令和最短板测步骤自包含于Cycle。

所有帧错误、Full、阶段超时与取消通过生产逻辑可安全注入；本次不承诺真实硬件CRC/fatal注错，也不以模拟宣称该物理故障已发生。这落实proposal的安全注入默认，不删减错误处理要求。运行中出现真实DMA错误仍必须按契约记录并fail closed。

Persisted Evidence默认 `none`：Act Response可保留短descriptor/status、ARP字段、少量payload与人工对端一致结论，足以判断Acceptance。使用现成Wireshark查看，不要求保存完整pcap/串口目录或新增判定工具。若实质blocker需要不可重现现场，由Act按公共规则保留决定性片段并报告。

## Iteration balance, risks and handoff

000的DMA/cache、初始化和RX是一个不可分的真实接收基线；单独拆cache配置没有硬件可验证成果。001以已验收RX定位TX发布/terminal故障，002以原始双向基线定位协议/device适配故障。三轮不按文件数或固定task数拆分；都留出独立构建、板测、Review边界。

没有待Act决定的实质未知项或用户豁免。实际board link能力、DMA完成或报错是本次要验证的目标，不能预先声明PASS；若与同板资料/已批准配置矛盾则当前Cycle阻塞交Plan，不增加试写或身份审计。物理网线、串口与USB需操作者，agent在主机GREEN后到能力边界停止；收到板测输出并Review通过再展开后轮。

2026-10-03用户原话“批准”，当前计划已获审计批准，Gate2 PASS，首轮Cycle ready；本次完成Plan批准登记，后续按Act指令实施。整个MS11完成只证明轮询双向/DMA硬件基础；MAC IRQ async、stop/reset与多hart仍分别属于MS12–14。
