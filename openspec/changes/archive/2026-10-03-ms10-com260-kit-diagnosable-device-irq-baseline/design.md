# CoM260 Kit 设备 IRQ 设计

Status: approved。2026-10-03用户回复「批准」，批准记录见proposal的Gate 2 Plan Approval；Gate 2 PASS，当前 `iterations/000-initial/001-replan.md` 为ready。实施尚未开始。

## Context

需求来源是 proposal 的 2026-10-02 用户批准与 delta specs 五项 Requirement。本文件用 R1–R5 依次指代这些标题，不新增需求：R1 控制器与初始化分离；R2 设备 claim/status 对齐；R3 破坏性 cause；R4 重复、disable 与无风暴；R5 验证和兼容。SNAPSHOT 仅作项目描述；当前开发约束采用 M41/M42 和公共规则。

现场为 `k3` 分支，HEAD `082339ba`；用户已有 tasks/references 修改、shell Analysis 和本 change 未跟踪文件均保留。HEAD 相对 MS09 收尾 `ddbbf29c` 的目标平台、探针、host harness、Cargo/Makefile 没有代码差异。采信 MS09 Iteration 004 Cycle 001 的 accepted Review：PHY harness 30、MAC harness 15、early-console harness 6 均通过；`make k3`、`make build`、`make lichee` 退出 0；`make vf2` 退出 2，为既有 axfs/axnet/axtask 导入失败，不算成功，也不纳入本 change 修复。真板 stage 1–8、PHYID `001cc916`、至少五次完整插拔和复位恢复已通过。没有重复运行这些未失效基线。

当前调用链与状态：

- K3 `_start` 保留固件 a0 hartid、a1 DTB → `axplat::call_main` → axruntime → `InitIf::init_early/init_later` → kernel entry → `k3::run_first_byte_park`。axruntime 在 paging 后调用 init_later，稍后启用全局 SIE。不能把 boot hart 固定写成 0。
- `irq_stub.rs` 注册总失败，其余操作 inert；`init_later` 只有 time 初始化。axruntime 的 timer 注册失败，K3 未启用 STIE。新增外部 IRQ 不顺带启用 timer/IPI 调度。
- `axhal::irq::irq_handler` 调用平台 `handle(vector)`，只有返回 Some 时才调用现有 IRQ hook。平台处理真实 source，不改变 hook 或调度器。
- stage 6 完成 pad/IO-PD 和一次 PHY reset；stage 7 MDIO identity；stage 8 为 120 秒 link 观察。stage 8 后现为 park，追加新阶段。`ProbeEnv` 已提供 volatile u32 MMIO、原生单调时钟和等待；MDIO 每次 idle/completion 等待分别有 10 ms 上限。
- 现有 pin35 mux=1，PHY reset 是 GPIO1_5；二者不同。不得为 IRQ 再次 reset、改 IO-PD、猜 GPIO-bank 位或重写整字 pad 配置。

## Goals / Non-goals

建立可单独诊断的 AIA 软件投递底座，随后验证 PHY35→pinctrl→APLIC60 的真实设备链；MAC133/277 配置路由并观察状态。按2026-10-03用户裁决，真实插拔只验收有效触发、状态对齐、后续可再触发和无风暴；不要求固定循环次数或人工动作与claim计数相等。

不包含 DMA、descriptor、网络收发、异步任务框架、多 hart 流量、timer/UART IRQ 调度、交互 shell、固件修改、持久烧录、性能或 reset 驱动。使用现有 HandlerTable、axcpu IRQ 控制、SBI 和 ProbeEnv；不引入 rdrive/somehal 框架或新依赖。

## Decisions

### D1：寄存器依据与已发现的参考错误

| 事实 | 已读取依据 | 设计采用 |
|---|---|---|
| APLIC `e0804000/4000`，512 sources；IMSIC `e0400000/400000`，511 IDs，guest bits 6、hart bits 4 | `tools/k3_com260_ifx.dtb` 经 dtc 解码；R69 intc 模块 | APLIC 加入 MMIO ranges；IMSIC 通过 CSR 管理本地文件，MSI 地址几何沿用固件预配 |
| source60/133/277 level-high，PHY pin35 falling | 同 DTB pinctrl/GMAC/PHY 节点 | 平台外部源采用 60/133/277，设备下降沿归 pinctrl 配置 |
| claim+EOI 单条 `csrrw stopei, zero` 在 handler 前 | R69 somehal 包装及 intc | 保留操作顺序，禁止锁跨 handler |
| `stopei` EID 在高位 `[26:16]`，priority 在低 11 位 | R69 同仓 OpenSBI `lib/utils/irqchip/imsic.c` 与 `riscv_encoding.h`；[Linux IMSIC handler](https://kernel.googlesource.com/pub/scm/linux/kernel/git/johan/usb-serial/+/refs/tags/usb-serial-6.17-rc1/drivers/irqchip/irq-riscv-imsic-early.c) | 修正 R69 Rust `parse_stopei` 的低位 EID 解释；测试用 EID≠priority 检出反转 |
| INER `a42:12` bit4，INSR `a43:1d` clear-on-read，page selector `1f` | [Linux RTL8211F 驱动](https://code.googlesource.com/linux/torvalds/linux/+/21e4675d9305f6ccd20b95d943882d607c8ae288/drivers/net/phy/realtek.c)，PHYID 精确匹配 | 修正 proposal 的 page3；保存/恢复原 page，不读第二次 INSR 来否定第一次快照 |
| pad falling/rising/clear 位 5/4/6；gpio-edge syscon `d4019800/10` | R69 pinctrl regs 与本板 DTB | 保留K3地址/位布局，采用D5相关资料支持的clear/re-arm固定顺序；读取完整syscon窗口，不猜pin35 bitmap索引 |

R69 路径根为 `/home/daivy/projects/serial/work/k3/others/Rt-Async-AMP/tgoskits/`，重点为 `drivers/intc/riscv_{imsic,aplic}/src/lib.rs`、`platforms/somehal/src/arch/riscv64/imsic_aplic.rs`、`drivers/ax-driver/src/soc/k3/pinctrl/regs.rs`。OpenSBI 位于同参考仓的 `others/Rt-Async-AMP/opensbi-k3/opensbi-spacemit/`。另已消费 R66 信息价值分析、R68 RAM boot Runbook、k3 interrupt/time 与 GMAC DMA/IRQ 文档；参考工程的框架和取证习惯不替代本项目规则。

### D2：分离初始化，固定当前所需路由

global APLIC 配置与本地 IMSIC 初始化分别为独立函数，由 boot hart init_later 各执行一次；本轮只有单 hart，禁止把 per-hart 函数变成重新初始化 APLIC 的入口。先禁用 APLIC sources、清 pending，再设置 MSI 地址，最后 domaincfg IE|DM。IMSIC 清 EIE、threshold=0、delivery=1，最后打开 SEXT；不打开 STIE/SSIE。

S-mode 写 `smsicfgaddrh` 不证明全部几何位生效：readback 展示实际字段，仅按 S-mode 可控制字段判定；固件几何由现有参考默认采信，实际投递由自测裁决。不加固件资格预检。

最小固定路由为软件 source/EID1（Detached）、PHY60、MAC133、wake277（后者三者 LevelHigh），EID=source。其他 register 请求返回 false、不触碰硬件；EID0、source0 和 source512 拒绝。初始化仍覆盖 DT 声明的全部 source bitmap，包括 source512 所在第16字，避免复制参考 ceil(512/32) 漏掉最后一位的问题。target hart 字段来自真实 boot hart，超出16个 hart 的几何即停止，不猜路由。

使用现有 `axplat::irq::HandlerTable<512>`：register 成功后自动 enable，失败不 enable；unregister 先 disable 再移除。enable 顺序是 sourcecfg/target → clear source pending → local EIE → APLIC enable，避免 sourcecfg 改变电平解释时产生的旧 pending；disable 先 APLIC 再本地 EIE。被 mask 的 pending 可保留，只有明确的新一轮软件自测清场清除对应旧 pending，然后再使能，不能以 stale pending 冒充新触发。

### D3：运行时所有权与投递

本地 siselect/sireg 两条访问组成临界段，保存原 SIE、临时关闭、恢复原值，复用 axcpu 原生能力，防止 foreground 被 handler 打断后 selector 混用。无需单 hart 锁；不持锁执行 handler、串口输出或 MDIO。

外部 vector 为 interrupt-bit|9。每次 trap 原子 claim 一次、解析 EID、再查表分发并返回实际 source；再次 pending 经 sret 后再次 trap，禁止无界 drain。零 claim 和未知/未注册 EID 只累计诊断、返回 None、不 panic、不触发通用 hook。没有 try_lock 丢事件路径。控制器探针通过最小 K3 内部观察接口读取原始 claim/计数/readback，接口不带 CLI、身份字段或独立判定协议。send_ipi 使用现有 SBI HartMask；不以发送动作声称 IPI 接收/调度已验收。

探针输出走 polling console；handler 内只记录原始字段/计数，前台读取稳定快照再输出，避免串口重入。设备 handler 为唯一 INSR 消费者；设备窗口期间前台不得操作 MDIO，BMSR 对齐读取如需执行由同一 handler 完成。错误只记录并停止当前阶段，不继续输出设备成功。

### D4：两轮验证与失效边界

第一轮 host harness 直接调用生产 AIA 纯核与实际软件探针，用 fake MMIO/CSR 观察真实顺序。第二轮复用 MDIO host fixture 验证 page 恢复、COR 前保存、全部原始位保留、错误和D5固定ack顺序；host测试只证明软件按契约执行，K3实际触发/清除/再次投递由真板裁决。

板上 stage9：SETIPNUM_LE 和 GENMSI 各三次，逐次等待到达后再触发；每次 1 秒截止；APLIC disable 和 EIE disable 分别再触发并观察 1 秒无分发；清理旧 pending 后 re-enable 新触发成功；handler 内再触发一次 GENMSI，得到两个独立 claim；未知 EID 和零 claim 的边界由 host 测试，未知 EID 另以受控 GENMSI 板测；最后 1 秒 quiet、所有计数不变。首次访问前输出初始化层标记，运行异常保留最后层标记/native trap 输出并复位；读回异常、等待超时则 mask SEXT、报告当前层并 park。

第二轮 stage10：按D5配置 PHY/pinctrl/source60，在120秒观察窗口内执行拔线与插回，观察 source60、原始边沿状态、INSR bit4与handler读取的当前link变化，证明ack后再次投递；稳定后观察1秒quiet，不比较人工动作数。启用133/277并观察 raw source pending/enable/target，以及 GMAC channel0 status（MAC base+`0x1160`，R69 `regs::dma_chan_status(0)`、core只读诊断用法）和已有APMU控制字的wake使能bit12。只读这些状态，不写DMA/WoL、不伪造MAC事件，不以wake使能位冒充wake cause或真实投递。组合cause全部位保存由host覆盖，真板只报告实际值。失败/取消通过 reset 回原系统，未执行人工项不计完成。

沿用 `make k3`、目标 rustc host harness、受影响的既有 PHY/MAC harness；平台变动完成后 `make build`、`make lichee`。VF2 与已知 unrelated host-test 失败采信基线，除涉及其代码或新矛盾外不重复全量测试。新 harness 作为 tests 下原生 rustc 测试，复用现有 Makefile host-test 接入方式，无结果解析器。

### D5：用相关资料闭合 pinctrl 操作，不依赖特定源码文件

依据分层：本板DTB给出 pinctrl→source60、PHY pin35 falling 与gpio-edge窗口；R69给出K3 MFPR35=`d401e08c`及edge位布局。[SpacemiT K1字段说明](https://github.com/spacemit-com/docs-chip/blob/main/en/key_stone/k1/k1_docs/k1_ds.md)解释同位置bit6为edge逻辑禁用/重新准备控制，bit5为falling使能；[Marvell MMP清边沿实现](https://git.zx2c4.com/linux-rng/diff/arch/arm?h=jd%2Fvdso-test-harness&id=c9494727cf293ae2ec66af57547a3e79c724fec2)有同bit6置1后恢复原字的用法。[Marvell DTS](https://android.googlesource.com/platform/hardware/bsp/kernel/marvell/pxa-v3.14/+/refs/heads/master/arch/arm64/boot/dts/pxa1908.dtsi)也定义相同地址/长度的edge-wakeup窗口。后两者是相关实现依据，**K3沿用clear/re-arm语义是推导**；不复制K1 pin地址表、GPIO58寄存器或其它SoC的bank映射。

固定设备契约：

1. 保持source60 masked，保存MFPR35原字；只RMW bits4..6，不改mux/电气/IO-PD。先置CLEAR=1且RISE/FALL=0；PHY paged INSR读一次清旧cause后，在INER只更新LINK bit4。保存page selector并在成功/失败路径尝试恢复，沿用每次MDIO等待10ms上限；恢复失败保留原错误和恢复错误并停止。
2. 读取gpio-edge `d4019800 + {0,4,8,12}`四个u32原始字，完整保存，不写syscon、不解释为W1C、不猜pin35在哪个bank/bit。MMIO描述将既有GPIO页范围 `d4019000/100` 扩为覆盖该DT子窗口的单一 `d4019000/1000` 页，避免重叠映射；实际访问仍只落在已知GPIO/reset或gpio-edge窗口。
3. 设置MFPR35 FALL=1、RISE=0、CLEAR=0，保存前后原字，最后启用source60。稳定链路时1秒无新claim；真实事件期间由唯一设备handler消费INSR，前台不操作MDIO。
4. 每次claim先保存四个edge原始字与MFPR35；再读一次INSR并保存全部16位（COR），读BMSR两次取当前link；随后以当前MFPR35字仅置CLEAR=1、读回作MMIO完成，再清CLEAR回0，保持FALL=1。原始cause快照先于这组清除动作，不用后读值否定快照。该顺序先释放PHY INTN，再clear/re-arm pad，减少旧电平重复投递；允许未处理的同类边沿合并。
5. 输出source60、edge四字、MFPR、INSR、当前link、ack后edge四字；静默不再投递且后续独立插拔仍产生新对齐claim，才接受K3实际clear/re-arm效果。原始edge快照可观察，不以未确定的bank位解释作为拒绝条件；claim必须为60、INSR link bit4必须存在，真实link变化必须可见。
6. MDIO错误、claim/status不一致或持续level-high导致反复trap时，mask60及EIE60，保存原始状态和失败原因，前台报告并park。事件记录使用最多32条固定快照容量，满时停止而非覆盖未输出cause；host覆盖持续assert时止损，真板以ack后quiet与再触发证明正常路径。停止后不改试其它pad/offset/清法。

这套固定方案允许Act按直接契约实现，不要求Act调查或选择ack语义；只有实际行为否定契约时才按运行失败返回Plan。没有新增独立硬件资格预检。

## Risks

- **U1，已解除资料阻塞**：采用用户授权的替代资料策略与D5固定操作；K3具体clear/re-arm效果仍待实际设备验证，host模拟不证明它。原始syscon四字保存不依赖pin35 bitmap索引；无需取得vendor文件或补证其它bank全集。
- **U2，已由用户裁决关闭**：取消人工插拔次数相等及固定循环门槛，验收真实变化、对齐、再触发和quiet；RTM标Simplified并引用2026-10-03原话。受控软件自测仍逐次验证。
- 非阻塞选择：内部辅助函数和文件的精确拆分可由 Act 选择，不能改变上述所有权、接口语义和责任边界。

## Gates and Handoff

Gate 1/2 PASS：需求、修订裁决与整体计划均获批准。R2/R3为Covered；R4为用户已批准的Simplified，没有Missing。当前Cycle为ready。旧Cycle的Plan Context保持历史状态，通过同Iteration的001-replan承载修订。

Persisted Evidence 按用户明确要求为 required，只保存各轮决定性原始串口片段及 README；Plan 不创建 evidence。host/build 结果进 Act Response。Plan 后停止，不调用 Act、Recorder 或 Maintainer，不同步全局 tasks/M/R/I。
