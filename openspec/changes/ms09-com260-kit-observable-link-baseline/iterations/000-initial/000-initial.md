# Iteration 000 / Cycle 000: 参考基线与 Linux 板级采集入口

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 1.1–1.2（全局 T13 前半）
- Acceptance gaps: None
- Repair items: None
- Inherited scope: 用户两次「批准」中第二次覆盖修订后的 `proposal.md` 与增量规格；本 Cycle 只建立参考基线和用户态只读采集入口，不修改内核。
- Excluded scope: tasks 2.1–5.2、当前板运行、RAM 跳转、寄存器读写、PHY/link 实测、IRQ/DMA/异步网络、持久刷写。

**Objective**

把可追溯的 K3/CoM260 官方资料和真板成功经验纳入 MS09 的参考基线，并交付能在官方 Linux 只读输出运行 FDT/设备资源、在 host 可测试和交叉编译的采集程序。此轮不声称已经取得当前板事实或安全 RAM 地址。

**Investigation Facts**

- Current Baseline: StarryOS branch `k3` @ `160d7967b585f2319e89188d0644c16f3c79b04b`；MS08 accepted。用户确认实物型号 CoM260 Kit，尚未提供当前板修订、运行 FDT、串口日志和 U-Boot 内存图；本机 `/dev/serial/by-id` 不存在。本 Cycle 无须现场设备。参考仓库 `/home/daivy/projects/serial/work/k3` `main` @ `e6d0557` 有用户原有未提交/未跟踪文件，本轮只读。`docs/reference/source-coverage.md` 以 `observed`/`partially-observed` 分级，G7（Kit 默认 DTS）仍 open；`others/Rt-Async-AMP/README.md` 写有 AP FIT/RAM 成功经验，也含固件/RP 刷写命令，后者不进入本 Cycle。仓库 Buildroot 包 `bootfs.img` 的 Kit V02 DTB 解出 `model = SpacemiT K3 Com260 Kit V02`、UART0 `0xd4017000`、Ethernet `0xcac82000`、PHY@1；历史 Linux 报告也记载这两个 MMIO 与 `carrier=1`。历史 UART IRQ `47` 与静态 DTS source `42` 编号域未核实，均不得选作当前板 IRQ。
- Current-State Evidence: 根 `Makefile` 已有 `BENCH_CC ?= riscv64-linux-musl-gcc`、静态 C 板上诊断程序和 `cc -std=c11 -Wall -Wextra -Werror` host 测试惯例；`tests/` 无 CoM260 板级信息采集程序，`rg` 未找到 `/sys/firmware/fdt`、`/proc/device-tree` 或 `carrier` 的现成采集代码。本机 `riscv64-linux-musl-gcc`、`riscv64-linux-gnu-gcc`、`cc`、`dtc` 均可调用；Rust 已安装 target 不含 Linux RISC-V。根 `Cargo.toml`/kernel 当前仍无 K3 feature，后续 Iteration 才触及。已有 early-console host 六项测试曾通过，只支持后续串口逻辑基线，不是本 Cycle 的采集程序测试。
- Code and Critical Path: `Makefile` 新建最小交叉编译目标 → `tests/ms09_board_facts.c::main` 从 `/sys/firmware/devicetree/base`（可用时）或 `/proc/device-tree` 读取运行树，从 `/sys/class/net` 与设备 `of_node` 只读路径取得 Linux 绑定/link → 直接输出字段或缺失原因；`tests/ms09_board_facts_test.c` 测试复用的属性解码/错误分类函数。程序只读、不修改内核调用链。当前板运行和 bootloader 内存信息留到 Iteration 001。

**Implementation Guidance**

先确认来源与字段的适用范围，再建立程序的纯属性解码/错误分类 host 测试，观察 RED 后实现只读 Linux 遍历和直接文本输出，最后用现有 `BENCH_CC` 静态交叉编译。使用 libc/原生文件接口，不引入 FDT/日志资格依赖、运行身份字段或采集审计器。可输出父 `#address-cells/#size-cells` 解出的 `reg`、phandle 对应路径；未能解析时给出原始属性与明确未知，不猜测地址。Linux 仅能报告其可见 CPU 和设备状态，不能替代 OpenSBI 可用 hart、U-Boot relocation、RAM 安全区间或 StarryOS 首字节。

**Task Contracts**

### 1.1: 官方来源与真板成功经验的参考基线

- Requirement/Scenario: R1「目标板事实先于硬件后端选择」；官方资料成为默认参考、来源冲突和第三方边界。
- Depends on: None。
- Targets: 只读 `k3/docs/reference/{source-coverage,known-gaps}.md`、`docs/boot/{com260-image-and-dts,k3-ram-boot-and-fastboot}.md`、`docs/serial/com260-uart.md`、`docs/network/com260-gmac-phy.md`、`others/Rt-Async-AMP/README.md`、历史 Kit Linux 报告；结果写入本 Cycle Act Response，不改参考仓库。
- Current behavior: 这些资料已存在，但当前 change 尚无按字段、来源等级、适用范围、已知冲突整理的可执行基线。Kit V02 编译 DTB 与历史 Linux 记录均给出 UART0 `0xd4017000`、MAC `0xcac82000`，但产品修订/实际 DTB 未唯一映射；UART IRQ `42`/`47` 的编号域未厘清。
- Required behavior: 记录启动链、串口、运行 DTB 候选、内存、MAC/PHY、Linux 可采集入口与第三方 AP/FIT 成功路径；每项区分官方已观察正文、部分观察入口、第三方/历史实跑和当前板待核对项。静态资料在其直接支持的范围内默认可用，不需先接板。
- Preserve: 参考仓库既有用户改动；MS09 只做 AP RAM 路线，官方资料的版本和目标板范围不可丢。
- Forbidden: 把第三方 RP/固件刷写序列、FPGA 默认 `dtb_name` 或历史 Linux 设备状态提升为当前板已验证参数；修改 `k3` 仓库。
- Test witness: 当前 change 尚无字段级来源表；直接阅读上述文件、`mtype`/`dtc` 查看发行包 Kit V02 DTB可见来源与编号差异，未核对字段记为未知。
- GREEN condition: Act Response 的来源/字段表能直接回到所列原文，至少覆盖 UART、memory、MAC/PHY、DTB 选择和 AP RAM 路线，并逐项说明当前板仍需采集的部分。
- Verification: 用原文件的字段/访问状态和编译 DTB 直接核对表格；发现文档与原始字段矛盾时报告矛盾并暂停该字段的使用，不因其他字段可用而整体阻塞。
- Stop when: 某必要字段的来源被误判为官方正文，或成功案例含有本变更禁止的持久写入步骤却被列为执行命令。

### 1.2: 可在官方 Linux 运行的只读采集程序

- Requirement/Scenario: R2「官方 Linux 上的只读板级信息采集」；当前板可采集、字段缺失/权限不足/固件不可见、取消/错误。
- Depends on: 1.1 的字段清单与来源边界。
- Targets: 新建 `tests/ms09_board_facts.c`、`tests/ms09_board_facts_test.c`；根 `Makefile` 仅增添独立的 host 测试和 `target/ms09-board-facts` RISC-V Linux 静态构建目标。不得改内核、根 Cargo feature、现有测试语义。
- Current behavior: 本仓有 C 板上 probe 和 `BENCH_CC`/host `cc` 惯例，但无 CoM260 FDT/设备资源采集程序；本机无可访问的当前板 Linux 文件系统。
- Required behavior: 程序在 Linux 用户态从运行设备树 (`/sys/firmware/devicetree/base`，不可用时 `/proc/device-tree`) 和可用的 sysfs/procfs 只读读取 root model/compatible/chosen、`memory`/`reserved-memory`、Linux CPU 节点/online、UART 与 MAC `compatible/reg/status`、MDIO/PHY、关联 phandle 与 clock/reset/pinctrl 引用、驱动绑定及网络 carrier。解析 NUL 分隔字符串、big-endian cell 与父节点地址/大小 cell；能解析的关系给出节点路径，不能解析的原值/缺项明确标注，不猜测。Linux 不可见的 bootloader relocation、安全 RAM 区间和固件可用 hart 固定标为“需启动阶段核对”。输出只用于人直接阅读与下一 Iteration 对照，不承载运行身份或合格判定协议。
- Preserve: 只读、单次有限遍历；无设备状态更改；现有 QEMU/D1/VF2 构建和 host 测试保持。
- Forbidden: `/dev/mem`、任意 MMIO 访问、写 sysfs/FDT/网络/固件、执行 shell 采集链、额外依赖、fixture CLI、hash/revision/run-id/manifest/日志审计或专用资格判定器；不输出板卡序列号、网卡 MAC、随机种子或 `bootargs` 中与 console/启动定位无关的秘密值。
- Test witness: 先在新 C 原生测试中覆盖字符串表、big-endian `reg`/phandle、缺失/权限错误、路径与边界输入；编译/运行观察预期 RED，再实现 GREEN。程序运行于 host 无设备树时允许明确报缺项，不能用 host 结果冒充当前板输出。
- GREEN condition: `cc -std=c11 -Wall -Wextra -Werror tests/ms09_board_facts_test.c -o /tmp/ms09-board-facts-test` 与 `/tmp/ms09-board-facts-test` exit 0；`make target/ms09-board-facts` exit 0，`file target/ms09-board-facts` 显示 RISC-V Linux ELF；源代码可核对无禁止写路径。
- Verification: 直接使用原生测试输出/退出码、交叉编译器退出码和 `file` 输出；程序的当前板完整性留给 Iteration 001，不增加外部输出判定脚本。
- Stop when: 目标 Linux 不提供所需接口且无法只读获取、解析需要改变已批准输出语义，或代码需借权限写入设备状态才可继续；返回 Plan 而非扩大采集权限。

**Invariants**

官方资料在已观察正文和目标范围内默认采信；当前实例待运行核对；第三方经验不覆盖官方/运行冲突。程序只读，缺项 fail closed；固件和持久介质不变。QEMU、D1、VF2、异步 NIC 与内核代码本 Cycle 不变。

**Non-goals**

不在当前板运行、选择安全 RAM 地址、构建 K3 内核、打包 FIT、启动 StarryOS、读取 MAC/MDIO 寄存器或启用 AIA/DMA/网络。

**Acceptance**

- A1 / R1 / 1.1：启动/UART/memory/MAC/PHY/DTB 与 AP RAM 路线的来源、适用范围和当前板待核对项明确；已观察官方字段可作为参考，第三方/历史经验独立标注。
- A2 / R2 / 1.2：只读采集程序在 host 原生测试通过且可静态交叉编译为 RISC-V Linux ELF，包含运行 FDT/资源关系/link 的读取路径与缺失/权限/固件不可见分类。
- A3 / R1–R2 / 1.1–1.2：无当前板参数臆测、无设备状态写入或身份型证据机制；本轮不宣称当前板运行、RAM 安全或 StarryOS 首字节。

**Verification**

- 场景「官方资料成为默认参考」：直接核对 `source-coverage.md` 的访问状态和目标范围、主题文档字段、编译 DTB 属性；Act Response 只陈述各来源实际支持的结论。
- 场景「第三方成功经验」：`others/Rt-Async-AMP/README.md` 的 AP FIT/RAM 路线可说明后续方案，但其 RP/固件刷写和固定地址不写为本轮操作命令；历史 Linux 网络成功只说明曾有通道。
- 场景「Linux 只读采集/错误」：host C 测试覆盖属性解码、缺失/权限/未可见分支；`make target/ms09-board-facts` 和 `file` 直接判定可构建的目标 ELF。板上真实字段不计入本轮通过条件。
- 场景「取消/兼容」：程序不存在状态写路径且有限遍历；仅新增独立 Makefile 目标，不改变现有平台或测试目标的行为。

**Requirements Traceability Matrix**

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 参考与当前板事实分层 | 1.1、2.1–2.3 | 本轮来源/字段直接核对；下一轮实物/固件/FDT 冲突检查 | Covered（当前板现场在后续 Cycle） |
| R2 官方 Linux 只读采集 | 1.2、2.1 | 本轮 C 属性/错误测试与交叉 ELF；下一轮当前 Kit 直接输出 | Covered（板上运行在后续 Cycle） |
| R3 RAM 启动保持持久固件不变 | 2.2、3.2–3.3 | 后续当前内存/恢复、FIT/真板启动 | Covered（后续 Cycle 未展开） |
| R4 早期串口独立 | 2.3、3.1、3.3 | 后续 K3 first byte 与 early-console host 回归 | Covered（后续 Cycle 未展开） |
| R5 MAC 与 PHY/link 分层 | 2.3、4.1、5.1–5.2 | 后续当前板资源、寄存器/PHY/link 真板观察 | Covered（后续 Cycle 未展开） |
| R6 平台兼容 | 3.1–3.3、5.2 | 后续 K3 与 QEMU/D1/VF2 构建/运行回归 | Covered（后续 Cycle 未展开） |

**Gate 2 Readiness**

PASS：用户审定修订后的详细计划并回复原话「批准」。StarryOS HEAD、`k3` 参考仓库 HEAD、`cc`/RISC-V Linux 交叉编译器/`dtc` 入口与计划基线一致；1.1 的来源可只读访问，1.2 的源文件尚不存在，可先建失败测试；任务、设计、场景、RTM 与直接验证覆盖完整。当前板未接入属于 Iteration 001 的能力边界，不阻塞本 Cycle。下一阶段只可按本 ready 契约由用户另行调用 Act。

**Persisted Evidence**

- Mode: none

Act Response 中的字段/来源、原生测试与编译决定性输出足够支持 A1–A3；静态来源和 host 运行均可低成本重取。Plan 不预建 `evidence/`。

**Risks and Notes**

参考仓库 G7 open；Kit V02 编译 DTB和历史 Linux 启动可优先指导采集，但都不等于当前板状态。`dtc` 可用于只读解码参考 DTB，不作为目标板匹配判定；`tests/early-console-host-harness.rs` 的 GREEN 不计入真板首字节。后续 Iteration 的任务依赖见 change `tasks.md`；现场访问在 Iteration 001 才形成边界。

## Act Response

- Status: reported

**Implemented**

- Task 1.1：按 Task Contract 只读核对 `k3` 参考仓库（`main`@`e6d0557` 工作树）的 `docs/reference/source-coverage.md`、`docs/reference/known-gaps.md`、`docs/boot/com260-image-and-dts.md`、`docs/boot/k3-ram-boot-and-fastboot.md`、`docs/serial/com260-uart.md`、`docs/network/com260-gmac-phy.md`、`others/Rt-Async-AMP/README.md` 与历史 Kit Linux 报告 `.claude/runbooks/K3 CoM260 首次真机接口、通信与固定网络配置报告.md`；用 `mtype/mcopy`+`dtc` 直接核对本地发行包 `others/Buildroot-K3-v1.0.7/bootfs.img` 内的 `k3_com260_kit_v02.dtb` 与 `env_k3.txt`。字段/来源表见下「Task 1.1 来源基线」。未修改 `k3` 仓库（`git status` 前后一致，本轮只读）。
- Task 1.2：新建 `tests/ms09_board_facts.c`（815 行，只读采集程序：FDT 树两遍走查——先建 phandle→path 表、再按类别上报 root/chosen/memory/reserved-memory/cpu/uart/mac/mdio-phy，并经 `/sys/class/net/*/device/of_node` 关联驱动与 carrier；fail-closed 白名单只输出、缺失/权限/ioerr 分类、固件可见项固定标「需启动阶段核对」）；新建 `tests/ms09_board_facts_test.c`（23 个纯函数测试：BE32/字符串表/reg 解码（2-cell、1-cell、>4G、畸形、截断）/errno 分类/phandle 查找/路径拼接边界/引用属性 cell 格式化）；根 `Makefile` 仅增 `target/ms09-board-facts` 交叉目标与 `host-test` 末尾三行，既有目标与测试语义未动。本 Response 为含 Plan Review F1/F2 当前 Cycle 修复后的完整当前状态（见下「Repair」）。

**Task 1.1 来源基线（字段 / 来源 / 适用范围 / 待当前板核对）**

| 字段 | 值（已观察） | 来源与状态 | 适用范围 | 待当前板核对 |
|---|---|---|---|---|
| 发行包镜像组成 | bootfs.img 为 FAT16：Image 40MB、`env_k3.txt`、21 个 DTB 含全部 7 个 CoM260 候选 | `others/Buildroot-K3-v1.0.7/bootfs.img`（本地包，`mdir` 直接观察） | K3 Buildroot SDK 产物 | 当前板实际启动介质内容是否一致 |
| U-Boot env | `console=ttyS0,115200`、`bootdelay=0`、`ramdisk_addr=0x130000000`、`dtb_dir=`（空）、`knl_name=Image` | 同上 `env_k3.txt`（mcopy 解出） | 该 SDK 包默认 env | 当前板 env 是否被持久化修改；`dtb_dir=` 空时 U-Boot 选 DTB 的实际逻辑 |
| 运行 DTB 首选候选 | `k3_com260_kit_v02.dtb`：`model = "SpacemiT K3 Com260 Kit V02"`；`chosen` bootargs `console=ttyS0,115200`、stdout-path `serial0:115200` | `dtc -I dtb -O dts` 反编译发行包 DTB（2026-09-30 本轮直接核对） | 发行包默认打包的 Kit 命名 DTB；G7 仍未唯一映射（`v02`≠产品版本 `v03`） | 当前板运行 FDT 的 model/compatible 是否即此 |
| UART0 | base `0xd4017000`、`reg-shift=2`、`reg-io-width=4`、compatible `spacemit,k1-uart`+`intel,xscale-uart`；**IRQ 静态 42（dtb `interrupts=<0x2a,0x04>`）** | 同上 DTB 反编译 + `com260-uart.md`§3（k3.dtsi 交叉验证） | 静态 DTS 链：12Pin→uart0_0_cfg→serial0→chosen 已闭合 | **历史 Linux 报告载 IRQ 47**：42/47 编号域冲突未裁决，两轮均不得选值；实际 console 阶段链 |
| UART 实例矩阵 | 17 实例（AP 11 + RCPU 6），AP base `0xd4017000+0x100` 步进，uart10 单独 `0xd401f000`（G8 open）；fifo 256（dts）vs 64（第三方 IP）冲突不裁决 | `com260-uart.md`§2-§5（k3.dtsi/k3-rdomain.dtsi 直接打开） | SoC 级静态字段 | 当前板启用集合（uart4/uart5 等） |
| DRAM | 起点 `0x102000000`、大小 `0x1fe000000`（约 8GiB） | `k3.dtsi` + 发行包 DTB `memory@102000000` 双方一致 | SoC/该 DTB 内存描述 | 当前板实际装配容量与 firmware 可见顶层 |
| CMA | alloc-ranges 解码后 `0x140000000`、大小 `0x20000000` | `k3_com260.dtsi`（2-cell 解码，父节点 cells=2） | shared DTS 配置输入 | 实际启动日志 CMA 预留 |
| MAC 实例 | `ethernet@cac82000`、窗口 `0x2000`、compatible `spacemit,k3-gmac`+`snps,dwmac-5.10a`、rgmii、max-speed 1000 | 发行包 DTB 反编译 + `com260-gmac-phy.md`（官方 DTS 候选）；与第三方 IFX DTS 一致 | 官方候选 + 第三方交叉一致 | 当前板启用哪个 GMAC、寄存器 handoff（clock/reset）状态 |
| MAC IRQ | `interrupts=<133,277>`（macirq / eth_wake_irq） | 发行包 DTB 反编译；与固定 revision 第三方 IFX DTS「APLIC wired source 133」一致 | 静态输入；source→EID/target hart 未解（G4） | 真板 delivery（MS10+） |
| PHY | MDIO bus `snps,dwmac-mdio`，`phy@1` compatible `ethernet-phy-id001c.c916`（Realtek OUI） | 发行包 DTB 反编译 + 两个官方 DTS 候选一致 | 已观察 DTS 标识 | **实物型号（RTL8211F?）**、RJ45 连接器、link 状态；phase 47/53、reset gpio 1_5 仅 shared DTS 输入 |
| DTB 唯一映射 | 7 个 CoM260 候选；`k3_com260_kit_v02.dts` 名称最近但 v02≠v03；发行包打包 kit_v02.dtb 属强相关而非映射证据 | `com260-image-and-dts.md`§3/§5 + `known-gaps.md` G7（open） | 候选集合 | 原理图下载制品 / defconfig `BR2_TARGET_KERNEL_DTB` / 板上 model 字符串 |
| AP RAM 路线 | U-Boot：`fastboot -l 0x180000000 -s 0x04000000 usb 0` → host `fastboot stage starryos.uimg` → Ctrl+C → `bootm 0x180000000`；第三方示例 kernel load/entry `0x140000000`、FDT load `0x138000000` | `others/Rt-Async-AMP/README.md`（固定 revision `ccb1ff0b…`） | 第三方真板成功经验（AP FIT/RAM 路线可行） | **全部地址为示例，禁止作默认值**；当前板 U-Boot help 语法、实际 FIT 大小、内存图重叠检查 |
| 排除项 | `mtd erase/write esos`、`mtd write opensbi` 等刷写序列 = 持久写入；Pico-ITX 仓库动作 | 同上 README + `k3-ram-boot-and-fastboot.md` | 第三方经验中的禁止部分 | 不进入本轮；MS09 只做 AP RAM 路线 |
| 历史 Kit Linux 报告 | UART0 0xD4017000 IRQ 47；Ethernet 0xCAC82000 `end1` `carrier=1`；UFS 0xC0E00000 /dev/sda KINGSTON TY7B-128 128GB（ESP/bootfs/rootfs）；SDHCI 0xD4280000；xHCI 0xC0A00000/0x81400000；IMSIC 0xE0400000（8160 interrupts）；APLIC 0xE0804000（512）；QSPI 0xD420C000；Pinctrl 0xD401E000；WDT 0xD4014000；Bianbu 4.0.1 / 6.18.3 | `.claude/runbooks/K3 CoM260 首次真机接口、通信与固定网络配置报告.md`（历史实跑） | 历史环境实跑事实，不等于当前板状态 | 当前板逐项复核（尤其 IRQ 47 vs 42、 carrier、容量） |

范围说明：本轮未修改 `k3` 仓库；刷写/RP 固件/OpenSBI 补丁序列只记录不执行；G1-G15 缺口状态未变（G7 仍 open）。

**Repair（Plan Review F1/F2，当前 Cycle 内完成）**

- F1（`fact_print_ref_prop` 对存在属性误报 `<missing>`）：解码逻辑提取为纯 helper `fact_format_ref_cells`（host 可测；经 `lookup` 函数指针注入 `fact_resolve_phandle`，复用既有纯函数/探针分区惯例且避免二维表强转），`fact_print_ref_prop` 改为调用它并输出到 4 KiB 缓冲。语义：前导 (phandle,arg) 双 cell 对按原样解析（`<path>#<arg>` / `phandle=<hex>#<arg>`）；剩余单个/奇数尾 cell 按裸 phandle 解析（`<path>` / `phandle=<hex>`）；0 cell 的存在属性输出 `<empty>`；非整 cell 长度输出 `<malformed>`（沿用 reg 解码先例）；格式化超 4 KiB 输出 `<overflow>`（fail-closed 标注）。Kit V02 形状（`pinctrl-0 = <0x87>` 单 phandle）在 UART/MAC 行均解析为 `/soc/pinctrl@d401e000`。
- F2（`fact_report_netdevs` 用 `readlink` 原始相对目标与绝对路径 `strcmp`）：改为 `lstat` 区分「无 of_node 属性的虚拟接口（ENOENT，按契约静默跳过）」与「of_node 存在但解析失败」，后者以 `MS09_FACTS_ERROR: item=netdev_of_node:<iface>` 按缺失/权限/ioerr 分类显式报告；`of_node` 统一经 `realpath` 解析为绝对路径后再与 MAC 路径比对；比对根一次 `realpath` 规范化（`/proc/device-tree` 别名归一到 sysfs 规范位置，procfs 回退模式下比对不失效）。driver/carrier 输出逻辑未动。
- 新增 host 测试 8 项：单 phandle（Kit 形状）、单 phandle 不可解析、clocks 六 cell 三对、双 cell 对不可解析、奇数尾裸 phandle、0 cell `<empty>`、非整 cell `<malformed>`、输出溢出拒绝。

**Verification Evidence**

1. Task 1.1 直接核对（mtools + dtc，exit 0）：
   ```
   $ mdir -i k3/others/Buildroot-K3-v1.0.7/bootfs.img ::
   K3_COM~5 DTB 147139 k3_com260_kit_v02.dtb   env_k3 txt 125 env_k3.txt
   $ dtc -I dtb -O dts k3_com260_kit_v02.dtb -o kit_v02.dts
   model = "SpacemiT K3 Com260 Kit V02";
   serial@d4017000 { interrupts = <0x2a 0x04>; status = "okay"; }  // 0x2a = 42
   ethernet@cac82000 { compatible = "spacemit,k3-gmac\0snps,dwmac-5.10a";
     reg = <0x00 0xcac82000 0x00 0x2000>; interrupts = <0x85 0x04 0x115 0x04>; ... }
   phy@1 { compatible = "ethernet-phy-id001c.c916"; reg = <0x01>; }
   memory@102000000 { reg = <0x01 0x2000000 0x01 0xfe000000>; }
   cpu@0..cpu@15 + cpu@100（共 17 个 cpu 节点）
   ```
   `cat env_k3.txt`: `console=ttyS0,115200 / bootdelay=0 / ramdisk_addr=0x130000000 / dtb_dir=（空）`。
2. F1 修复前单元 RED（先提取 pair-only 语义，Kit 形状用例对现有语义见证缺陷）：
   ```
   $ cc -std=c11 -Wall -Wextra -Werror tests/ms09_board_facts_test.c -o /tmp/ms09-board-facts-test && /tmp/ms09-board-facts-test
   ms09-board-facts-test: tests/ms09_board_facts_test.c:179: test_ref_cells_single_phandle_kit_shape:
     Assertion `strcmp(out, "/soc/pinctrl@d401e000") == 0' failed.          # exit 134 (SIGABRT)
   ```
3. F1/F2 修复后 GREEN 三命令（Task Contract 1.2 原命令）：
   ```
   $ cc -std=c11 -Wall -Wextra -Werror tests/ms09_board_facts_test.c -o /tmp/ms09-board-facts-test && /tmp/ms09-board-facts-test
   ms09-board-facts: all host tests passed                                  # exit 0（23 项）
   $ make target/ms09-board-facts                                           # exit 0（riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os）
   $ file target/ms09-board-facts
   ELF 64-bit LSB executable, UCB RISC-V, RVC, double-float ABI, version 1 (SYSV), statically linked, with debug_info, not stripped
   ```
4. 伪 DT/伪 sysfs 冒烟（`unshare -rm` + tmpfs；夹具复刻 Kit V02 字段形状：单 phandle pinctrl-0、相对 `device/of_node`/`driver` 符号链接、自环 of_node（eth9）、无 of_node 虚拟接口（eth7））。修复前现场：UART/MAC 行 `pinctrl-0=<missing>`、无任何 `MS09_FACTS_NET` 行（F1/F2 缺陷直接可见）。修复后决定性输出：
   ```
   MS09_FACTS_MAC: ... reg=0xcac82000/0x2000 ... pinctrl-0=/soc/pinctrl@d401e000
   MS09_FACTS_UART: ... pinctrl-0=/soc/pinctrl@d401e000
   MS09_FACTS_ERROR: item=netdev_of_node:eth9 result=ioerr:Too many levels of symbolic links
   MS09_FACTS_NET: iface=eth0 of_node=/soc/ethernet@cac82000 driver=spacemit-gmac carrier=1
   ```
   eth7 无输出行（虚拟接口静默跳过保持）；程序 exit 0。host 无 FDT 的 fail-closed 路径未被本轮修复触及，采信首轮 Verification 4 结论。
5. 禁止项与结构核对（修复后重跑）：`grep -nE '/dev/mem|fopen.*"w|O_WRONLY|system\(|popen\(' tests/ms09_board_facts.c` 仅命中文件头注释；`openspec validate ms09-com260-kit-observable-link-baseline` → `Change 'ms09-com260-kit-observable-link-baseline' is valid`。

**Deviations from Plan**

- 无范围偏差。首轮两处契约内细化并记录：(a) reg 属性存在但解码失败时输出 `reg=<malformed>`（契约允许「未能解析时给出原始属性与明确未知」）；(b) 增加计划未点名的伪 DT 命名空间冒烟验证（只读、本机、无产品代码改动），用于在无板条件下走查遍历逻辑；板端完整性仍留给 Iteration 001。
- Repair 在 Follow-up Decision 指令内的四处细化并记录：(a) 非整 cell 长度（0<len<4）输出 `<malformed>` 而非按 0 cell 归入 `<empty>`——沿用 reg 解码先例，避免静默截断字节；(b) 格式化超 4 KiB 时输出 `<overflow>` 显式标注（Follow-up 未枚举的超长 fail-closed 行为）；(c) F2 比对根经 `realpath` 规范化，使 `/proc/device-tree` 别名在 procfs 回退模式下仍可比对（sysfs 主路径有冒烟见证；该别角未单独见证，失效方向与修复前一致，不产生假事实）；(d) 错误项缓冲扩至 272 字节容纳 NAME_MAX 接口名（`-Wformat-truncation` 要求）。伪 DT 冒烟夹具相应扩展相对符号链接、自环与虚拟接口三类形状。
- 观察（非本轮引入）：未设 `SMP` 时根 Makefile 第 31 行 `test: -gt: unary operator expected` 在任何 make 目标都会出现，属 pre-existing。

**Self-Review（Gate 3）**

- 首轮实现期发现并已修复（均有测试/冒烟见证）：(1) `fread` 读目录返回 0 而非 -1 导致遍历空转——改判 `ferror` 取 `EISDIR`；(2) `char[512][512]` 二维表强转 `const char *const *` 的类型双关 SEGV——`fact_resolve_phandle` 改为正确索引；(3) compatible 字符串指针指向共享 `g_prop`、后续 reg 读取覆写造成首串空——改调用方私有缓冲 `fact_string_list_prop_buf`；(4) CPU status 先读入 `g_prop` 后被 reg 覆写——改局部缓冲并调整读取顺序；(5) `MS09_FACTS_CPU_SYS` 未剥离换行——已修。
- Repair 期新增发现并已修复：编译期 `-Wformat-truncation`（`err_item` 64 字节不足以容纳 255 字节接口名）→ 扩至 272 字节；重编译后 `-Werror` 全绿。
- 遗留 Minor：CPU_SYS 三个文件按设计以空格连接单行输出（语法已冻结于文件头注释）；`fact_report_error` 的 ioerr 分支随 `strerror` 文本变化属预期；`make target/ms09-board-facts` 依赖 `target/` 目录已存在（Plan Review M1，未变化）。
- Critical/Important：无。diff 全量核对：本轮改动仅限 `tests/ms09_board_facts.c`（include 区、`fact_format_ref_cells` 新增、`fact_print_ref_prop` 与 `fact_report_netdevs` 重写）与 `tests/ms09_board_facts_test.c`（新增 8 测试及调用），无新增写路径、无计划外文件；`git status` 其余条目（references.md、Makefile、analysis/、change 目录）均为本轮前已存在。`k3` 仓库未修改。

**Persisted Evidence**

None required（Plan 设为 none；来源文件与 host 运行均可低成本重取）。

**Experience Candidates**

- Runbook 候选 1：「从 Buildroot `bootfs.img` 提取并反编译 DTB 的只读流程」（`mdir` 定位 → `mcopy` 解出 → `dtc -I dtb -O dts` 核对 model/reg/interrupts），端到端验证两次（kit_v02.dtb 字段核对 + env_k3.txt）。证据：本 Response §Verification 1。
- Runbook 候选 2：「无板条件下用 unshare 用户命名空间 + tmpfs 伪 `/sys`/伪 DT 冒烟只读采集程序」（脚本构建夹具 → `unshare -rm` 内 `mount -t tmpfs none /sys && cp -a` → 运行 host 编译产物核对输出行），端到端验证三次（修复前 / 修复后 / 含虚拟接口最终形态）。证据：本 Response §Verification 4。
- Issue 候选：无（未发现 change 范围外实质缺陷；SNAPSHOT 已记录的 `make host-test` ms04 既有失败未复验也未新增证据，维持原登记）。

**未解决问题**

无阻塞。Plan Review F1/F2 已在当前 Cycle 修复并复验，等待 Plan 重新 Review；procfs 别名比对的规范化（Deviations (c)）为未单独见证的边角，失效方向安全。Iteration 001 的能力边界（实物访问）按计划不阻塞本 Cycle；板上采集命令将在 Review 通过后按 Iteration 001 契约交付操作者执行。

## Plan Review

- Review Result: accepted

**Findings**

本 Review 为本 Cycle 的最终完整审查，覆盖首轮实现与 F1/F2 当前 Cycle 修复。独立通读修复后 `tests/ms09_board_facts.c` 全文与 `tests/ms09_board_facts_test.c` 新增测试，逐项核对前轮 Follow-up Decision 1–3：

- F1 已闭合：解码提取为纯 helper `fact_format_ref_cells`（经 lookup 函数指针注入 `fact_resolve_phandle`，host 可测）。存在的属性不再输出 `<missing>`：0 cell → `<empty>`，非整 cell → `<malformed>`，双 cell 对按 `<path>#<arg>`/`phandle=<hex>#<arg>`，奇数尾 cell 按裸 phandle 解析，格式化超长输出 `<overflow>` fail-closed；`fact_print_ref_prop` 改走该 helper（`MS09_REF_OUT_CAP` 4 KiB）。新增 8 项测试覆盖 Follow-up 要求的全部形状（单 phandle Kit 形状、clocks 六 cell 三对、奇数尾、零 cell）另加不可解析、畸形、溢出，合计 23 项；RED 记录（`test_ref_cells_single_phandle_kit_shape` exit 134）与该断言位置一致。
- F2 已闭合：`fact_report_netdevs` 以 `lstat` 区分 ENOENT（无 of_node 属性的虚拟接口，静默跳过）与其他错误（`MS09_FACTS_ERROR: item=netdev_of_node:<iface>` 显式分类）；of_node 经 `realpath` 解析为绝对路径后再比对，比对根同样经 `realpath` 归一（`/proc/device-tree` 回退模式不失效，失败时回退原值，方向安全）；`<sys/stat.h>` 已补；`err_item` 272 字节容纳 NAME_MAX。冒烟决定性输出（UART/MAC 行 `pinctrl-0=/soc/pinctrl@d401e000`、`MS09_FACTS_NET` 行出现且 of_node/driver/carrier 正确、自环 eth9 报 `ioerr:Too many levels of symbolic links`、虚拟 eth7 静默）与代码路径逐条对应。
- 修复偏差 (a)–(d) 均在契约语义内：(a)(b) 是「不猜测、显式标注」原则的直接延伸；(c) 归一化边角未单独见证但失效方向与修复前一致、不产生假事实；(d) 为 `-Wformat-truncation` 必要扩容。`realpath`/`lstat` 只读，无新写路径。
- 遗留非阻塞 note：偶数 cell 的纯多 phandle 列表（如 `pinctrl-0 = <&a &b>`）仍按 (handle,arg) 对解码——无 provider binding 无法消歧，属 DT 固有歧义；Kit 形状为单 phandle 不受影响，超出本轮修复范围，不要求处理。

**Acceptance Gaps**

None。前轮 A2 的两个缺口（F1 pinctrl 假缺失、F2 netdev 关联失效）已闭合；A1、A3 维持满足（修复未引入写路径或臆测值，来源基线未变）。

**Evidence**

- F1：`tests/ms09_board_facts.c` `fact_format_ref_cells` / `fact_print_ref_prop`；`tests/ms09_board_facts_test.c` 新增 8 测试（Kit 单 phandle 断言解析为 `/soc/pinctrl@d401e000`）。
- F2：`tests/ms09_board_facts.c` `fact_report_netdevs`（lstat / realpath / 归一比对根 / `err_item` 272 字节）。
- 采信来源：Act Response（修复后完整快照）Verification 2–4——RED exit 134、GREEN 三命令（23 项 host 测试、交叉构建、静态 RISC-V Linux ELF）、伪 sysfs 冒烟输出。覆盖未失效：本轮 Review 未改动产品代码，交付文件与 Act 声明一致。

**Follow-up Decision**

修复满足前轮 Follow-up Decision 1–3 的全部要求且验证充分；遗留事项均为非阻塞 Minor/note。Iteration 000 Acceptance A1–A3 满足，本 Iteration 完成。按 Map 展开 Iteration 001，不创建后继 Cycle。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/001-board-ram-boundary/000-initial.md`（当前板资源与可恢复 RAM 边界，tasks 2.1–2.3）。
