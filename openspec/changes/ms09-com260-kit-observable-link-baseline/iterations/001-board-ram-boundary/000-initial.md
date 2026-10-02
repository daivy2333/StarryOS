# Iteration 001 / Cycle 000: 当前板资源与可恢复 RAM 边界

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 2.1–2.3（全局 T13 后半）
- Acceptance gaps: None
- Repair items: None
- Inherited scope: Iteration 000 accepted 的参考基线（Task 1.1 来源/字段表）与只读采集程序 `tests/ms09_board_facts.c`；proposal 获批条件（沿用板载固件、首轮只做 RAM/FIT 临时启动、用户协助采集串口日志/运行 FDT/板卡修订并提供 RJ45 对端）。
- Excluded scope: tasks 3.1–5.2（K3 内核构建、FIT 打包、RAM 跳转、MAC/MDIO 寄存器访问、IRQ/DMA/异步网络）、采集程序与任何产品代码修改、持久刷写、`k3` 参考仓库修改。

**Objective**

当前 CoM260 Kit 的板卡修订、启动介质、固件、运行 FDT、boot/可用 hart、UART/MAC/PHY 及内存图经现场只读核对形成记录；payload/FDT 候选 RAM 区间与普通复位恢复路径有现场依据；冲突与 Linux 不可见项显式保留并阻塞对应后端选择。

**Background**

Iteration 000 交付了 host 测试通过、可静态交叉编译的采集程序和官方来源基线，但未取得任何当前板现场事实（能力边界：实物访问）。本 Iteration 由用户作为物理操作者执行板上与固件侧观察，Act 负责给出操作指令包、汇合记录并按基线逐项对照。本 Iteration 无产品代码修改；交付物是 Act Response 记录与 Evidence 现场文件。

**Investigation Facts**

- Current Baseline: StarryOS `k3` @ `160d7967` + 未提交 MS09 文件（`Makefile` +8、`tests/ms09_board_facts.c`、`tests/ms09_board_facts_test.c`、change 目录）。Iteration 000 最终 Act Response（F1/F2 修复后快照）：host 23 项测试 GREEN；`make target/ms09-board-facts` 产出静态 RISC-V Linux ELF（musl 静态链接，无运行时依赖，任意 rootfs 可直接运行）；Plan Review accepted。来源基线关键参考值（对照用）：model `SpacemiT K3 Com260 Kit V02`（Kit V02 DTB）、UART0 base `0xd4017000`/`reg-shift=2`/`reg-io-width=4`、MAC `0xcac82000` 窗口 `0x2000` compatible `spacemit,k3-gmac`+`snps,dwmac-5.10a`、MDIO `phy@1` `ethernet-phy-id001c.c916`、DRAM `0x102000000`+`0x1fe000000`、CMA `0x140000000`/`0x20000000`、UART IRQ 静态 `42`（dtb `interrupts=<0x2a,0x04>`）与历史 Linux 报告 `47` 编号域冲突未裁决。
- Current-State Evidence: 采集程序输出 grammar 见 `tests/ms09_board_facts.c` 文件头注释（`MS09_FACTS_ROOT/MODEL/COMPATIBLE/CHOSEN/MEMORY/RESERVED/CPU/CPU_SYS/UART/MAC/PHY/NET/FIRMWARE_ONLY/ERROR` 行，缺失/权限/ioerr 分类）；`MS09_FACTS_FIRMWARE_ONLY` 三项（U-Boot relocation、safe RAM region、firmware harts）固定标注需启动阶段核对——由本 Iteration 2.2 现场补齐。传输通道：历史 Kit 报告记录 UFS `/dev/sda`、xHCI 与既有网络连通，静态 ELF 经 U 盘或网络任一通道均可传板，最终通道由操作者按现场条件选择并记录。串口：历史报告 UART0 console `ttyS0,115200`；本机 `/dev/serial/by-id` 不存在，串口适配器由用户接入。U-Boot 交互：发行包 env `bootdelay=0`，进入控制台的手法以现场串口提示为准。第三方示例地址（kernel load/entry `0x140000000`、FDT `0x138000000`、fastboot stage `0x180000000`/`0x04000000`）仅为格式参考，禁止作默认候选。
- Code and Critical Path: 本 Iteration 无产品代码修改。Act 的操作对象是板载 Linux shell、U-Boot 控制台与串口终端，命令全部只读（Task Contracts 逐条列出）；RAM 候选推导是现场记录值上的算术核对。

**Implementation Guidance**

顺序 2.1 → 2.2 → 2.3：先 Linux 侧采集与实物标识，再 U-Boot 只读观察与复位验证，最后汇合裁决表。操作者人工步骤按公共规则 › 验证执行：每步写明最短操作与直接可观察结果，所见即判定，结果以一句话或输出片段回传。板上输出回收优先经文件通道（板上运行 `./ms09-board-facts > board-facts.txt` 后取回文件）；无文件通道时经串口终端直接复制。2.1 的对照基准是 Iteration 000 Act Response「Task 1.1 来源基线」表；逐项记录相符/差异/未知，静态值不覆盖现场值。

**Task Contracts**

### 2.1: 板载 Linux 采集与官方基线核对

- Requirement/Scenario: R1「目标板事实先于硬件后端选择」（当前板事实足以选择后端 / FDT 与板卡资料冲突）；R2「官方 Linux 上的只读板级信息采集」（当前板可采集 / 接口缺失、权限不足或固件事实不可见）。
- Depends on: Iteration 000 accepted（采集程序与来源基线）。
- Targets: 操作者在当前 Kit 官方 Linux 运行传板的 `ms09-board-facts` 二进制；实物标识（板卡丝印/标签的型号与修订）；串口启动日志关键行（固件/U-Boot/Linux 版本、启动介质）；Act Response 对照记录 + `evidence/001-board-ram-boundary/000-initial/board-facts.txt`。
- Current behavior: 程序与基线表已交付但从未在当前板运行；当前板修订、运行 FDT、实际设备状态与 link 均无现场记录。
- Required behavior: 完整板上输出（`MS09_FACTS_BEGIN`…`MS09_FACTS_END` 全序列）被记录；Act 将输出与 1.1 基线逐项对照：model/compatible、chosen console、memory/reserved（含 CMA）、CPU_SYS 与 cpu 节点（boot hart 线索）、UART（base/compatible/clock/reset/pinctrl）、MAC（compatible/reg/phy-mode/clock/reset/pinctrl）、PHY（bus/reg/compatible）、NET（iface/driver/carrier）。每项记录相符/差异/未知；Linux 不可见项（U-Boot 自占、安全 RAM、固件 hart）单列留给 2.2；缺关键资源时对应后端标记阻塞，不以静态候选代填。实物修订与运行 model 的关系显式记录（一致、或存疑并说明）。
- Preserve: 板载固件、持久分区、sysfs/网络配置不变；程序只读运行；既有 QEMU/D1/VF2 构建与内核代码不变。
- Forbidden: 写 sysfs/DT/网络配置/MMIO/持久介质；把历史 Linux 报告或 Kit V02 DTB 值当作当前板观测值填入结果；修改采集程序；为采集提权或改系统状态；输出板卡序列号等无关身份信息。
- Test witness: 板上程序运行输出行本身（观测型任务，无 RED 阶段）；对照表每项可回到输出原文与 1.1 基线原文。
- GREEN condition: `board-facts.txt` 含完整 BEGIN/END 序列及运行环境说明行；Act Response 给出逐项对照表；实物修订与运行 model 关系已记录。
- Verification: 操作者最短步骤：传输二进制 → 板上 shell 运行 `./ms09-board-facts`（或重定向到文件后取回）→ 读取串口启动日志关键行与板面标识。直接可观察结果：`MS09_FACTS_*` 输出行与程序退出码 0。
- Stop when: 传输通道与串口均不可用（能力边界，交用户）；程序在板上无法运行且原因指向程序缺陷（返回 Plan，不在本 Cycle 改代码）；关键字段冲突影响地址/接口安全且现场信息无法裁决（记录冲突、阻塞对应后端，交 2.3 汇合与用户决定）。

### 2.2: U-Boot 只读内存图与复位恢复观察

- Requirement/Scenario: R1（bootloader handoff 事实）；R3「首次启动保持板载持久固件不变」的地址不重叠与恢复路径前置事实。
- Depends on: 2.1（启动介质与固件链确认）。
- Targets: U-Boot 控制台只读命令输出（`version`、`bdinfo`、`printenv` 中内存/加载相关变量、`help fastboot` 等可用 stage 命令确认）；串口启动日志中 OpenSBI/U-Boot 阶段行；一次普通复位观察；Act 的 RAM 候选区间推导；`evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt`。
- Current behavior: 当前板 U-Boot 实际 DRAM bank、relocation 区、可用 stage 命令与普通复位行为均无现场记录；第三方示例地址存在但被禁止作默认。
- Required behavior: 记录 DRAM bank 列表、U-Boot relocation 地址/大小、env 中与加载相关的变量（如 load 地址、bootcmd 涉及的区间）；结合 2.1 的 memory/reserved-memory/CMA 推导 payload 与 FDT 的候选 RAM 区间，逐段核对不与 reserved-memory、CMA、U-Boot 自占及自身展开区重叠（第三方地址仅作格式参考）；执行并观察一次普通复位（U-Boot `reset` 或重新上电）后系统返回原 Linux 登录提示，记录所见。复位后未回原系统或行为异常即记录并阻塞 RAM 路线推进。
- Preserve: 只读命令；不改 env、不写任何分区、不执行任何 stage/boot 跳转（本 Iteration 不执行 RAM stage）。
- Forbidden: `saveenv`、`erase`/`write`/`mtd write`、fastboot 刷写类命令；把示例地址填为候选区间；未做复位观察即宣称恢复路径可用。
- Test witness: U-Boot 控制台输出行与复位后串口序列本身；候选区间推导可回到记录值复算。
- GREEN condition: `uboot-readonly.txt` 含上述命令输出、复位观察记录；Act Response 给出候选区间表及逐段不重叠核对。
- Verification: 操作者最短步骤：复位板卡并按串口提示进入 U-Boot 控制台 → 依序运行只读命令并复制输出 → `reset` → 观察回到 Linux 登录提示。直接可观察结果：命令输出行与复位后串口序列。
- Stop when: 无法进入 U-Boot 控制台（记录现场现象，交用户决定手法）；`bdinfo` 与运行 FDT 内存描述冲突无法裁决（记录冲突，RAM 候选保持未定）；复位后系统未回原状态（立即记录，RAM 路线阻塞，返回 Plan）。

### 2.3: 平台接入字段汇合与裁决表

- Requirement/Scenario: R1「目标板事实先于硬件后端选择」（当前板事实足以选择后端 / FDT 与板卡资料冲突）。
- Depends on: 2.1、2.2。
- Targets: Act Response 中的汇合裁决表（本 Iteration 主要交付物）。
- Current behavior: UART/MAC/PHY/RAM 字段只有官方静态候选与第三方经验值；UART IRQ `42`/`47` 编号域冲突未裁决；G7（修订→DTB 映射）open。
- Required behavior: 对 Iteration 002 所需字段逐项给出「已裁决（现场依据）/ 未裁决（原因与阻塞范围）」：UART 基址/stride/访问宽度/clock/pinmux 状态、MAC compatible/reg/clock/reset、MDIO/PHY/RJ45 对端条件、安全 RAM 候选区间与恢复路径。IRQ/DMA/cache/IOMMU 只记边界，不作为 MS09 数据面资格。`42`/`47` 冲突按现场可见性如实记录（Linux 视图提供什么记什么），不强行裁决。
- Preserve: 每项裁决可回到 2.1/2.2 现场记录或 1.1 基线原文；未裁决项显式阻塞对应后端。
- Forbidden: 以 SoC 最大能力或候选文件名补值；把未裁决项写成已确认。
- Test witness: 裁决表每行的来源列可逐项回溯。
- GREEN condition: 裁决表覆盖上述全部字段，给出 Iteration 002 可直接消费的结论或显式阻塞项。
- Verification: Plan Review 逐行核对来源列与现场记录一致。
- Stop when: 汇合后 Iteration 002 前置字段大面积未裁决（返回 Plan 调整后续 Iteration 契约）。

**Invariants**

板载固件与持久介质不变；板上一切操作只读；冲突/未知 fail-closed 阻塞对应后端；第三方示例地址不作默认；本 Cycle 不修改产品代码与采集程序；`k3` 参考仓库只读；输出不含板卡序列号、网卡 MAC、rng-seed 等无关身份信息。

**Non-goals**

RAM 跳转、FIT 打包、K3 内核构建、MAC/MDIO 寄存器访问、IRQ/DMA/异步网络、多 hart 调度判定、性能观察。

**Acceptance**

- B1 / R1–R2 / 2.1：板上真实运行输出完整记录，与 1.1 基线逐项对照（相符/差异/未知），Linux 不可见项单列；缺关键资源的后端显式阻塞。
- B2 / R3 / 2.2：DRAM/reserved/U-Boot 自占有现场值；RAM 候选区间逐段不重叠核对完成；普通复位回原系统已观察。
- B3 / R1 / 2.3：汇合裁决表覆盖 Iteration 002 全部前置字段，未裁决项显式且带阻塞范围。
- B4 / R1–R3 / 2.1–2.3：无设备写入、无臆测值、程序与产品代码未被修改。

**Verification**

- 场景「当前板可采集」（R2）：板上运行输出的 `MS09_FACTS_*` 行直接判定；关键类别行齐备或显式缺项。
- 场景「接口缺失/权限不足/固件不可见」（R2）：输出中 missing/perm/ioerr 与 `MS09_FACTS_FIRMWARE_ONLY` 行直接判定分类是否如实。
- 场景「官方资料成为默认参考 / FDT 冲突」（R1）：对照表每项可回溯双方原文；冲突项对应后端保持未选定。
- 场景「安全 RAM 引导前置事实」（R3）：候选区间表与现场内存记录逐段核对；复位观察记录在案。
- 场景「取消/兼容」：全程无持久写入；本 Iteration 无代码变更，QEMU/D1/VF2 与既有测试不受影响。

**Gate 2 Readiness**

PASS：用户于 2026-10-01 审计本 Cycle 计划并回复原话「批准」。实物板在线、串口与文件传输通道是执行期能力边界，不阻塞 Gate 2；UART IRQ `42`/`47` 冲突与 G7 修订映射为已知未裁决项，本 Cycle 按显式记录处理，不构成 BLOCKED。无 WAIVED 项。

**Persisted Evidence**

- Mode: required

1. `evidence/001-board-ram-boundary/000-initial/board-facts.txt` — 板上采集程序完整输出，附运行环境一句话（日期、启动来源、传输通道）。支持 B1：Act Response 只保留 ≤20 行摘录，而完整输出是 Iteration 002 参数裁决与后续冲突回溯的原始事实；板上环境（串口/对端/启动状态）会话结束即不可低成本重取；缺失将阻塞 Iteration 002 的启动参数与字段裁决。通过条件：含完整 `MS09_FACTS_BEGIN`…`MS09_FACTS_END` 序列与环境说明行。
2. `evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt` — U-Boot 只读命令输出、串口启动日志关键行与复位观察一句话记录（可附候选区间推导表）。支持 B2：同上，一次性现场事实，摘要会丢失内存 bank/relocation 等决定性数值。通过条件：含内存 bank、relocation、相关 env 输出与复位结果记录。

**Risks and Notes**

- G7 open：实物修订与运行 FDT 的映射以现场 model/compatible 记录为准，kit_v02 DTB 仅为对照候选。
- 传输与串口手法依赖现场条件；进入 U-Boot 的手法以现场提示为准，未奏效时记录后交用户，不猜测按键序列。
- 板上 Linux 非 root 且关键文件 perm 受限时，perm 分类本身就是合法结果，不为采集提权。
- 本 Cycle 发现采集程序缺陷时不就地修改，返回 Plan（可能形成本 Iteration 的 rework/replan Cycle）。

## Act Response

- Status: reported

**Implemented**

- Task 2.1（完成）：操作者在当前板官方 Linux 真实运行 `ms09-board-facts`，完整输出存入 `evidence/001-board-ram-boundary/000-initial/board-facts.txt`（BEGIN…END 完整 + 环境行）；对照表见下，逐项可回溯本 Response 与 Iteration 000 基线原文。
- Task 2.2（完成；U-Boot 控制台只读命令采集经用户豁免）：两次串口日志（上电 + 复位按钮）存入 `evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt`（serial#/USB SerialNumber 脱敏）。复位观察达成——复位后完整走完 ROM→SPL→U-Boot→内核→系统链并回到原 Linux 登录提示（`Bianbu 4.0.1 k3 ttyS0` / `k3 login:`）。两次启动载荷地址逐一相同（kernel `0x140000000`→`0x102200000`（end `0x1042f7000`）、FDT@`0x138000000`、initrd `0x2f975d000`–`0x2fbd7b6b7`、FDT 终址 `0x2f9736000`）→ 重复启动稳定成立。现场日志 `Autoboot in 0 seconds` 证实无控制台窗口，控制台命令无法进入采集。
- Task 2.3（完成）：汇合裁决表见下，未裁决项显式列出并标注阻塞范围。
- 传输流程沉淀：U 盘 `/dev/sdb`（整盘 FAT32 无分区表）→ `/mnt/usb` → cp `/tmp` 执行；Runbook `.agents/runbooks/com260-usb-transfer-and-run.md` 按用户显式请求创建并登记 R67。
- 用户豁免（原话保留）：「如果没有有用信息没办法了，这个T没办法采集直接豁免掉就好」（2026-10-01，针对 U-Boot 控制台只读命令 version/bdinfo/printenv/help 的采集）。风险与缓解：① U-Boot 自占精确边界未采集——缓解为候选区间上界 `0x2f0000000` 低于全部观测到的 U-Boot 工作地址（initrd/FDT 安置自 `0x2f97xxxxx` 起）；② stage 命令（fastboot/bootm）可用性未采集——不阻塞 MS09 数据面资格，但 Iteration 002 的 RAM 装载交互路径必须现场解决，由该 Iteration 契约覆盖。

### Task 2.1 对照表（基线 = Iteration 000 Act Response「Task 1.1 来源基线」）

| 字段 | 基线参考值 | 当前板实测 | 判定 |
|---|---|---|---|
| model/compatible | Kit V02 DTB：`SpacemiT K3 Com260 Kit V02` | `SpacemiT K3 Com260 IFX` / `spacemit,k3-com260-ifx`；boot log TLV `product_name=k3_com260_ifx`，DTB 选择 `spacemit/6.18.3-generic/k3_com260_ifx.dtb` | 差异：运行 FDT 不是 kit_v02。G7 部分闭合——固件按 product_name 匹配 IFX 专用 DTB；实物丝印修订待操作者补记，运行 model 与丝印关系暂记「存疑（固件标识=IFX）」 |
| chosen | `console=ttyS0,115200`、stdout-path `serial0:115200` | 相同 | 相符 |
| UART0 | `0xd4017000/0x100`、`k1-uart`+`xscale-uart`、DT IRQ 42（0x2a） | 相同；pinctrl-0=`/soc/pinctrl@d401e000/uart0-0-cfg` 解析成功 | 相符（静态） |
| UART IRQ 编号域 | DT 42 vs 历史 Linux 报告 47，未裁决 | 两次内核日志均为 `ttyS0 at MMIO 0xd4017000 (irq = 47 …)` | 冲突方向明确：47 为 Linux virq 编号域，DT hwirq 42；2.3 按 DT hwirq 记录，Linux 47 不作为内核 IRQ 输入 |
| UART 实例 | 17 实例（AP 11+RCPU 6，uart10@0xd401f000） | 17 节点（d4017xxx×9+f000=10、c0881xxx=6、f0612000=1） | 总数相符；分组口径与基线差 1（f0612000 行基线未计入），非阻塞 |
| MAC | `cac82000/0x2000`、`k3-gmac`+`dwmac-5.10a`、rgmii、max-speed 1000、IRQ 133/277（0x85/0x115） | cac82000 全字段相同且唯一布线（phy-mode=rgmii、pinctrl gmac1-cfg）；另有 `a0000000`/`cac80000`/`cac8e000` 三个无 phy-mode 的 GMAC 节点；NET 行 `end1`↔cac82000、driver `dwmac-spacemit-ethqos` | 相符且唯一启用；IFX DTB 多 3 个未布线 MAC 节点（新事实，2.3 记录） |
| PHY | mdio `phy@1` `ethernet-phy-id001c.c916` | 相同（附 `ieee802.3-c22` 回退串）；输出两条等值 PHY 行（mdio 枚举与 phy 类别两路径各一，程序输出冗余，Minor） | 相符 |
| NET | 历史报告 `end1` `carrier=1` | `end1` carrier=0 | 差异：当前无链路（无对端/未插线），不阻塞 2.1；link 实测留 5.2 |
| DRAM | `0x102000000`+`0x1fe000000`（~8GiB） | FDT/内核 Zone 一致（Normal 0x102000000–0x2ffffffff）；U-Boot `DRAM: 8 GiB`（SPL 行 `Size: 4096MB` 为另一口径，备注不裁决） | 相符 |
| CMA | `0x140000000`/`0x20000000` | FDT RESERVED 无 CMA 行（IFX DTB CMA 经 alloc-ranges 定义，程序按契约只解码 reg）；内核日志 `CMA memory pool at 0x140000000, size 512 MiB` 补齐 | 相符（经内核日志） |
| CPU | Kit V02 DTB 17 节点（16+`cpu@100`）；Linux 16 hart | FDT `cpu@0..15` 全 okay；online/possible/present=0-15；两次日志 `Brought up 1 node, 16 CPUs` | 差异：IFX DTB 无 cpu@100；Linux 可见 16 hart 与产品预期一致 |
| reserved-memory | 基线只关注 CMA | 24 节点（rcpu/mmode/vdev/dpu/framebuffer 等）；内核报告 7 对 OVERLAP DETECTED 仍容忍（官方 DTS 自带重叠） | 新事实：RAM 候选须避开全部节点（含 framebuffer `0x2fe000000`/32MiB）；重叠为静态 DTS 事实，记录不裁决 |
| 启动链 | 来源表候选 | boot ROM `try sd…ERROR CMD8` → NOR（`boot_mode=nor`，GD gd25lq64c 8MiB，MTD 六分区 bootinfo/fsbl/env/esos/opensbi/uboot）→ SPL → U-Boot 2022.10 → env 取自 bootfs（UFS）→ UFS scsi0 启动内核 6.18.3-generic | 新事实：启动介质链闭合（NOR 固件 + UFS 系统），两次启动一致 |
| Linux 不可见项 | FIRMWARE_ONLY 三项 | boot log 已补：U-Boot 载荷地址（kernel `0x140000000`→搬迁 `0x102200000`、FDT@`0x138000000`（终址 `0x2f9736000`）、initrd `0x2f975d000`–`0x2fbd7b6b7`、SWIOTLB `0x2f0e00000`–`0x2f1600000`）、boot hart（`Booting Linux on hartid 0`，SBI v2.0 impl ID=0x1） | 部分补齐；`bdinfo`/`printenv` 权威值按用户豁免 |

Linux 不可见项单列与阻塞：无关键资源缺失，UART/MAC/PHY 资源齐备，无后端因资源缺失阻塞；唯一启用 MAC 为 cac82000（RJ45 经 gmac1-cfg）。

### Task 2.2 RAM 候选区间推导（现场值复算，可回溯）

观测占用（来源：board-facts.txt + 两次串口日志）：

| 区间 | 来源 |
|---|---|
| DRAM `0x102000000`–`0x2ffffffff` | FDT memory + 内核 Zone Normal（两次一致） |
| reserved-memory 全集（rcpu/mmode/vdev/rpmi/dpu：`0xc086c000`–`0xc087cfff`、`0x100000000`–`0x101ffffff`） | board-facts RESERVED 行 + 内核 nomap 行；均低于 DRAM 起点，不进入候选空间 |
| 内核镜像（U-Boot 搬迁后）`0x102200000`–`0x1042f7000` | U-Boot `Moving Image` 行 |
| CMA `0x140000000`–`0x160000000` | 内核日志 512 MiB @`0x140000000` |
| SWIOTLB `0x2f0e00000`–`0x2f1600000` | 内核日志 |
| initrd `0x2f975d000`–`0x2fbd7b6b7`；FDT 终址 `0x2f9736000`–`0x2f975c116` | U-Boot `Loading` 行 |
| framebuffer `0x2fe000000`–`0x2ffffffff` | FDT + 内核 |
| U-Boot 自占 | 未采集（豁免）；观测下界：U-Boot 安置区自 `0x2f97xxxxx` 起（initrd/FDT），即 ≥`0x2f9700000` |

**候选区间：`0x160000000`–`0x2f0000000`**（CMA 上界以上、U-Boot 观测工作区以下，约 6.4 GiB 连续空间）。逐段核对：reserved-memory 全集不落入该区间（或在区间外独立列出）；与 CMA 在 `0x160000000` 相接不重叠；与 U-Boot 观测工作区（≥`0x2f9700000`）留 `0x9700000` 以上间隔；与 framebuffer（≥`0x2fe000000`）、SWIOTLB（`0x2f0e00000` 起）不重叠。建议 payload+FDT 自 ≥`0x180000000` 起布局并预留自身展开区。第三方示例地址仅作格式参考、未作默认；`0x140000000` 为 U-Boot kernel 加载址且落入 CMA，禁用作 payload。

### Task 2.3 汇合裁决表（Iteration 002 前置字段）

| 字段 | 裁决 | 依据 |
|---|---|---|
| UART base/stride/访问宽度 | 已裁决：`0xd4017000`、reg-shift=2、reg-io-width=4 | 现场 FDT compatible/reg + Iteration 000 基线交叉一致 |
| UART clock/reset/pinmux | 已裁决：clocks `sysctrl@d4015000#0`/`#10` + `d4050000#25`；resets `sysctrl@d4015000#0`；pinctrl `uart0-0-cfg` | 现场 FDT |
| UART IRQ | 已裁决（按 DT hwirq）：42（APLIC 路由）；Linux virq 47 为编号域差异，不作内核 IRQ 输入 | 现场 FDT `interrupts=0x2a` + 两次内核日志 `irq=47` |
| MAC compatible/reg | 已裁决：`spacemit,k3-gmac`+`snps,dwmac-5.10a`、`0xcac82000/0x2000`、唯一布线实例 | 现场 FDT + 驱动 probe 日志（User ID 0x10 / Synopsys ID 0x54） |
| MAC clock/reset/pinctrl/phy-mode | 已裁决：clocks `sysctrl@d4282800#79`/`#81`、resets `#78`、pinctrl `gmac1-cfg`、rgmii、max-speed 1000 | 现场 FDT |
| MAC IRQ | 只记边界：hwirq 133/277；MS10 前不作数据面资格 | 现场 FDT |
| MDIO/PHY/RJ45 | 已裁决：mdio@cac82000 内 `phy@1`（Realtek `001c.c916`）；对端未接（carrier=0），link 观察属 5.2 | 现场 FDT + NET 行 |
| 安全 RAM 候选与恢复路径 | 已裁决：候选 `0x160000000`–`0x2f0000000`（payload ≥`0x180000000`）；恢复路径=复位→NOR/UFS 全新启动（现场两次观察回原系统） | 2.2 推导 + 复位日志 |
| U-Boot 自占精确边界 | 未裁决（用户豁免采集）；缓解：候选上界留距；Iteration 002 若需 ≥`0x2f0000000` 地址须先取 bdinfo | 本 Response 豁免段 |
| U-Boot 控制台手法 / stage 命令 | 未裁决（用户豁免采集）；阻塞范围：Iteration 002 的 RAM 装载交互路径（3.3 契约的重复观察必须现场解决） | 本 Response 豁免段 + `Autoboot in 0 seconds` 现场记录 |
| 启动链 | 已裁决：ROM（SD 失败）→ NOR（gd25lq64c 六分区）→ SPL → U-Boot → env/bootfs → UFS scsi0 内核；boot_mode=nor | 两次日志 |
| IRQ/DMA/cache/IOMMU | 只记边界：APLIC `0xe0804000`（512）/IMSIC `0xe0400000`（8160）现场在案；DMA/cache/IOMMU 不作 MS09 数据面资格 | 内核日志 |

**Self-Review（Gate 3）**

- 对照表/推导/裁决表每行可回溯 board-facts.txt、uboot-readonly.txt 或 Iteration 000 基线原文；区间推导为记录值上的算术核对，无臆测地址；第三方地址未作默认。
- 豁免仅覆盖 U-Boot 控制台命令采集；复位观察、重复启动稳定、RAM 推导、2.3 裁决均有现场证据。B4 保持：全程只读、无设备写入、无臆测值、程序与产品代码未修改。
- Minor（记录不处理）：采集程序 PHY 行重复输出（两条等值行）；SPL DDR `4096MB` 与 U-Boot `8 GiB` 口径差异；板上 RTC 无电（内核时钟 2000-01-01 起）；日志二 `rsvd_phy_addr` 与日志一不同（动态分配）。
- Critical/Important：无。本 Iteration 无产品代码修改。

**Verification Evidence**

- 2.1 GREEN：`board-facts.txt` 含完整 `MS09_FACTS_BEGIN`…`MS09_FACTS_END` 序列与环境行；对照表逐项相符/差异/未知；实物修订关系按「存疑（固件标识=IFX）」显式记录。契约通过条件全部满足。
- 2.2 GREEN（控制台命令部分按用户豁免）：复位观察在案（日志二末尾 `k3 login:`）；候选区间表与逐段不重叠核对完成；`uboot-readonly.txt` 1714 行、脱敏 14 处（`grep -c REDACTED` = 15，含 1 处说明行）。
- 2.3 GREEN：裁决表覆盖 Iteration 002 前置字段；未裁决项（U-Boot 自占边界、控制台手法/stage 命令）显式带阻塞范围，无臆测补值。
- `openspec validate ms09-com260-kit-observable-link-baseline` → 见本 Response 下方验证输出（收尾时运行）。

**Deviations from Plan**

- 控制台命令采集：命中 Task 2.2 Stop when「无法进入 U-Boot 控制台」，用户裁决豁免（原话保留）；复位观察实际已由复位按钮日志达成，豁免范围仅 console 命令输出。
- 启动日志保存范围由用户要求从「关键行」扩展为两次全文，Evidence README 已注明。
- 无其他偏差：板上操作全部只读；采集程序未修改；exit code 未单独回显，按输出序列完整性判定（契约允许）。

**Persisted Evidence**

- `evidence/001-board-ram-boundary/000-initial/board-facts.txt`（complete，支持 B1）
- `evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt`（两次启动日志全文 + 复位观察，支持 B2；控制台命令部分经用户豁免）

**Experience Candidates**

- Runbook 候选：无新增（USB 传输与运行 Runbook 已按用户显式指令创建，R67 已登记）。
- Issue 候选：无（官方 DTS reserved-memory 7 对重叠为上游静态事实而非本仓缺陷，2.3 已记录；`Autoboot in 0 seconds` 无控制台窗口为固件 env 事实，留 Iteration 002 现场解决）。

**未解决问题**

- U-Boot 控制台进入手法与 stage 命令可用性留待 Iteration 002 现场（其契约已含 RAM 启动重复观察）；不阻塞本 Cycle 收口，等待 Plan Review。

## Plan Review

- Review Result: accepted

**Findings**

本 Review 独立核对 Act Response、两份 Evidence 原文与产品代码状态；推导算术全部复算。

- B1 满足：`board-facts.txt` 含完整 `MS09_FACTS_BEGIN`…`MS09_FACTS_END` 序列与环境说明行（root shell、U 盘通道、Bianbu 6.18.3-generic）。对照表逐项与原文一致——model `SpacemiT K3 Com260 IFX`（运行 FDT 非 kit_v02，product_name→`k3_com260_ifx.dtb` 的固件匹配链有日志行 78–80 直接支撑）、UART0/MAC/PHY/MEMORY/CPU/NET 各行核对无误。Iteration 000 的两处修复在真板环境生效：`pinctrl-0=/soc/pinctrl@d401e000/uart0-0-cfg`、`gmac1-cfg` 解析成功，`MS09_FACTS_NET` 行（`end1`↔`/soc/ethernet@cac82000`、driver `dwmac-spacemit-ethqos`、carrier=0）正常输出——carrier=0 与无对端接线一致，正确区分「link down」与「设备不可见」。
- B2 满足（控制台命令部分按用户豁免）：复位观察在案（第 1714 行 `Bianbu 4.0.1 k3 ttyS0` / `k3 login:`，另两次启动的载荷地址逐一相同）。候选区间 `[0x160000000, 0x2f0000000)` 的不重叠由本 Review 对证据全集复算：reserved-memory 全部条目（evidence 内 26 行）要么低于 DRAM 起点（最高 `dpu_reserved1` 止于 `0x102000000`）、要么为 framebuffer `0x2fe000000` 在区间上方；CMA `[0x140000000,0x160000000)` 与区间相接不重叠；内核镜像 `0x102200000–0x1042f7000`、SWIOTLB `0x2f0e00000–0x2f1600000`（日志第 192 行 mapped 区间与 Response 一致）、initrd/FDT `≥0x2f9736000` 均在区间外。用户豁免原话、风险与缓解已记录（候选上界低于全部观测固件工作地址；Iteration 002 用 `≥0x2f0000000` 地址前须先取 bdinfo），处置符合规则。
- B3 满足：2.3 裁决表覆盖 Iteration 002 全部前置字段；UART IRQ `42`/`47` 的裁决（DT hwirq 42 为 APLIC source，47 为 Linux virq 编号域，后者不作内核 IRQ 输入）有 FDT `interrupts=0x2a` 与两次日志 `irq = 47`（第 478 行等）支撑；未裁决项（U-Boot 自占精确边界、控制台进入手法/stage 命令）显式带阻塞范围。
- B4 满足：`tests/ms09_board_facts.c`（815 行）/`_test.c`（283 行）与 accepted 版本一致未再修改；`k3` 参考仓库只读；脱敏 15 处（14 替换 + 1 说明）；全程无设备写入。
- Minor（记录，不阻塞）：① Response 叙述计数两处笔误——reserved-memory 实为 26 条（非 24）、OVERLAP DETECTED 实为 9 行/次启动共 18（非「7 对」）；推导按全集成立，不受影响。② 实物丝印修订未采集，2.1 GREEN 以「存疑（固件标识=IFX）」满足；G7 经 product_name 链部分闭合，丝印补记留给 Iteration 002 现场顺手完成。③ PHY 双行输出冗余为已登记程序 Minor，维持。

**Acceptance Gaps**

None。B1–B4 全部满足；豁免范围（U-Boot 控制台命令）有原话、风险与缓解记录，不扩大到复位观察与内存推导。

**Evidence**

- `evidence/001-board-ram-boundary/000-initial/board-facts.txt`（79 行完整序列 + 环境行）、`uboot-readonly.txt`（1714 行，两次启动全文，关键行：30/884 `DRAM: 8 GiB`、42 `gd25lq64c`、76 `Autoboot in 0 seconds`、79 dtb 匹配、91 `Moving Image … end=1042f7000`、94–95 ramdisk/FDT 终址、101 `hartid 0`、115 CMA pool、192 SWIOTLB mapped、240 `16 CPUs`、478 `irq = 47`、1714 `k3 login:`）。
- 采信来源：Act Response Verification Evidence（openspec validate 通过）；本 Review 未改动产品代码与 Evidence。

**Follow-up Decision**

现场采集、对照、推导与裁决均达到本 Iteration Acceptance；遗留为非阻塞 Minor。Iteration 001 完成，按 Map 展开 Iteration 002；不创建后继 Cycle。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/002-k3-boot-first-byte/000-initial.md`（K3 临时启动与首字节，tasks 3.1–3.3）。
