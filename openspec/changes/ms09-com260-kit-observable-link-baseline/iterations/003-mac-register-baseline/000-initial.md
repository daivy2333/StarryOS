# Iteration 003 / Cycle 000: MAC 只读寄存器基线

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 4.1（全局 T14 后半）
- Acceptance gaps: None
- Repair items: None
- Inherited scope: Iteration 002 accepted——K3 平台构建/`make k3`/FIT/RAM 启动/阶段标记/复位恢复全链路可用；R68 Runbook（U-Boot+USB 装载流程）；Iteration 001 裁决表（MAC `0xcac82000/0x2000`、compatible `spacemit,k3-gmac`+`snps,dwmac-5.10a`、clocks `d4282800#79/#81`、resets `#78`、pinctrl `gmac1-cfg`、rgmii）。
- Excluded scope: MDIO/PHY 读取与 link 观察（tasks 5.1–5.2）、MAC IRQ、DMA ring、packet I/O、clock/reset 恢复写序、异步网络、SMP。

**Objective**

在 K3 最小平台探针中对已裁决的目标 MAC 实例只读身份与状态寄存器：读值非全零/全一、身份可解释（与板上 Linux 驱动观测一致）且在重复启动下可重复；访问异常与 clock/reset handoff 问题分层报告，不盲写任何寄存器。

**Background**

Iteration 002 交付了可重复的 K3 RAM 启动与阶段化标记路径；本 Iteration 在同一探针路径上增加 MAC 只读阶段。这是 StarryOS 首次访问目标控制器硬件，clock/reset 保持 bootloader handoff 状态（R5：不得盲写；handoff 不可用则返回 Plan 调整，不让 Act 猜写序——design D4）。

**Investigation Facts**

- Current Baseline: StarryOS `k3` 分支工作树（HEAD `160d7967` + 未提交 MS09 改动：K3 feature/axplat/`k3.rs`/FIT/ITS/DTB/采集程序/change 产物）。Iteration 002 accepted Review：`make k3` exit 0、FIT crc 与板上记录一致、`[k3:pt-mmu]`+stage=1/2/3 两轮板上见证、复位回原系统、qemu/lichee 构建回归通过、early-console host 六项 GREEN。
- Current-State Evidence（板上观测与代码表面，本轮直接核对）：
  - 目标 MAC：`ethernet@cac82000`（唯一启用 GMAC，`phy-mode=rgmii`、`pinctrl-0=gmac1-cfg`、max-speed 1000）；板上 Linux 驱动 `dwmac-spacemit-ethqos` 绑定 `end1`。内核日志（`evidence/001-.../uboot-readonly.txt` 行 564–571）给出对照身份：`User ID: 0x10, Synopsys ID: 0x54`、`DWMAC4/5`、`DMA HW capability register supported`，另有 clock 事实（`rgmii tx clk derived from rx clk`、`phy clk provided by external crystal`、`phy-mode=rgmii val=0x00000008`）——这些是 Linux 运行时状态，仅作身份对照与 handoff 线索，不代表 U-Boot handoff 后的 clock 状态。
  - MMIO 映射：`0xcac82000` 与 UART `0xd4017000` 同在 1G block 3 `[0xc0000000,0x100000000)`，K3 引导页表已恒等映射该 block 并含高半窗 alias（`crates/axplat-riscv64-k3/src/boot.rs`，板上经 UART 输出验证）；探针访问沿用同一映射机制。`axconfig.toml` 的 `mmio-ranges` 是否已含 MAC 窗口由 Act 核对（未含则按 axconfig 既有机制补充该区间，属配置面）。
  - 探针挂点：`kernel/src/platform/k3.rs::run_first_byte_park()` 现输出 stage=1/2/3 后 `wfi` 停留；MAC 探针作为其后的新阶段（stage=4）或等价扩展，先期 stage 语义保持。
  - 寄存器偏移来源：身份/状态寄存器偏移与字段 MUST 来自可追溯正文——板上运行内核（6.18.3）对应 Linux `stmmac` 驱动源码（版本匹配的上游源）或 DWMAC 数据手册正文；`k3` 参考仓库 `docs/network/com260-gmac-phy.md` 无寄存器偏移（本轮查证）。Act Response 须记录来源文件与符号；不得凭记忆填偏移。
- Code and Critical Path: `kernel/src/platform/k3.rs`（探针扩展：MAC 只读 + 读值分类输出）→（如需）`crates/axplat-riscv64-k3/axconfig.toml` mmio 区间 → `make k3`（含 FIT 重打包）→ 板上按 R68 Runbook 两轮启动读值 → 复位。数据流：读值分类（ok / all-zero / all-one / fault 层）以纯函数实现并 host 测试；板上输出经既有串口标记路径。

**Implementation Guidance**

先实现读值分类纯 helper 并 host RED→GREEN，再扩展探针输出 MAC 身份/状态行（读值与期望身份 `Synopsys ID 0x54`/`User ID 0x10`/`DWMAC4/5` 的对照由 Act Response 完成，探针本身只报读值与分类，不在内核里写死「期望值通过/失败」判定）。寄存器读取放在 stage=3 之后、park 之前；每次读取至少两遍并全部输出（同启动内重复可见）。全部访问只读；不触碰 clock/reset/pinctrl 寄存器。偏移来源先查、后写代码。`.gitignore` 顺手补 `*.fit` 一行（Iteration 002 Minor ①，本 change 产物卫生）。

**Task Contracts**

### 4.1: K3 探针只读目标 MAC 身份与状态寄存器

- Requirement/Scenario: R5「MAC 寄存器与 PHY/link 构成独立可观测链路」（控制器和链路均可观察 / 寄存器访问失败）；R1（只使用已裁决字段与可追溯来源）。
- Depends on: Iteration 002 accepted。
- Targets: `kernel/src/platform/k3.rs`（MAC 只读探针阶段 + 读值分类输出）；新 host 测试载体（沿用 `tests/` 纯函数测试惯例，或并入现有 early-console harness 惯例——以 Act 就近选择并在 Response 记录）；`crates/axplat-riscv64-k3/axconfig.toml`（仅当 mmio-ranges 需补 MAC 窗口）；`.gitignore`（+`*.fit`）；`evidence/003-mac-register-baseline/000-initial/board-mac-regs.txt`。
- Current behavior: `run_first_byte_park()` 输出 stage=1/2/3 后停留；无任何 MAC 访问路径；StarryOS 从未访问目标控制器。
- Required behavior: 探针按可追溯偏移只读目标 MAC 身份与至少一个状态寄存器，每次读取 ≥2 遍全部经串口输出（原始读值 + 分类：`ok` / `all-zero` / `all-one` / 读取异常层）；读值分类逻辑为 host 可测纯函数（全零、全一、正常、边界输入先 RED 后 GREEN）；板上至少两轮独立启动中身份读值一致且可解释（对照板上 Linux 驱动的 `User ID 0x10`/`Synopsys ID 0x54`/`DWMAC4/5`），全部读值非全零/非全一；异常时输出所在层（访问异常→MMIO 映射层；全零/全一→clock/reset handoff 层；读值成功但身份不可解释→实例/偏移层）并停在该层，不继续推测。
- Preserve: stage=1/2/3 语义与顺序不变；`make k3`/FIT/RAM 启动/复位路径不变；既有平台构建与 early-console host 测试；MAC/clock/reset/pinctrl 寄存器与持久介质零写入；`k3` 参考仓库只读。
- Forbidden: 任何 MAC/clock/reset/pinctrl 写操作（含「恢复」「使能」「重试」写序）；在内核里硬编码期望值判定或把读值包装成资格结论；MDIO/PHY 访问（5.1 范围）；凭记忆填寄存器偏移；身份型证据机制。
- Test witness: RED——host 分类纯函数测试先于实现（函数不存在/未覆盖分类分支，编译或断言失败记录）；GREEN——分类测试 exit 0。板上见证——两轮启动的 MAC 读值行本身。
- GREEN condition: 分类 host 测试 exit 0；`make k3` exit 0 且 FIT 重打包成功；两轮独立上电/复位循环中 MAC 身份读值一致、非全零/全一、与内核日志身份对照可解释；每轮复位回原系统；qemu/lichee 构建 exit 0、early-console 六项 exit 0（采信未失效结论，仅重跑受影响面）。
- Verification: host——分类测试与构建退出码；板上——按 R68 Runbook 装载启动，串口记录 stage=4 MAC 读值行（≥2 遍读值 + 分类），`reset` 回 `k3 login:`，重复一轮；读值与 `uboot-readonly.txt` 行 569–570 对照写入 Act Response。
- Stop when: 读取产生访问异常（trap/fault）→ 记录现象停 MMIO 映射层，返回 Plan；全部寄存器恒为 `0x00000000` 或 `0xffffffff` → 停 clock/reset handoff 层，返回 Plan（按 D4 调整 003/004 契约，不允许 Act 猜测写序）；读值可读但与 DWMAC 身份无法对应 → 停实例/偏移层，记录读值与来源复核结果返回 Plan。

**Invariants**

只读；handoff 状态保持；失败分层且 fail-closed；偏移来源可追溯；stage 先期语义不变；既有平台与测试不回归；无 IRQ/DMA/MDIO/async。

**Non-goals**

PHY/MDIO（5.1）、link 观察（5.2）、MAC IRQ（MS10）、DMA、收发、clock/reset 恢复写序、吞吐/性能。

**Acceptance**

- D1 / R5 / 4.1：探针只读输出目标 MAC 身份与状态读值，非全零/全一，身份对照板上 Linux 驱动观测可解释。
- D2 / R5 / 4.1：两轮独立启动读值一致；每轮复位回原系统。
- D3 / R1+R5+R6 / 4.1：偏移来源可追溯；异常分层报告且停止；无任何寄存器/持久写入；既有平台构建与 host 测试保持。

**Verification**

- 场景「控制器可观察」（R5）：两轮启动的 stage=4 读值行直接判定。
- 场景「寄存器或 PHY 访问失败」（R5）：分类行（fault/all-zero/all-one）与停止层级直接判定；host 分类测试覆盖四类输入。
- 场景「对端未连接」不适用本轮（无 MDIO/link 读取）；carrier/link 留 5.2。
- 兼容（R6）：`make k3`/qemu/lichee 构建、early-console host 测试退出码。

**Gate 2 Readiness**

PASS：用户于 2026-10-01 审计本 Cycle 计划并回复原话「批准」。板上两轮启动是执行期能力边界（R68 Runbook 已固化流程，单轮成本低），不阻塞 Gate 2；寄存器偏移来源依赖上游 stmmac 源码可得性——不可得时按 Stop when 返回 Plan，不构成 BLOCKED。无 WAIVED 项。

**Persisted Evidence**

- Mode: required

1. `evidence/003-mac-register-baseline/000-initial/board-mac-regs.txt` — 两轮启动的装载命令、stage=1–4 串口输出（含每寄存器 ≥2 遍读值与分类行）、两轮复位观察、环境说明行。支持 D1/D2：板上读值是一次性现场事实，身份对照与后续 5.1/MS10 契约设计依赖原始读值；会话后不可低成本重取。通过条件：两轮完整 stage 序列 + MAC 读值行 + 两次复位回原系统记录。

**Risks and Notes**

- U-Boot handoff 后 MAC clock 状态未知：全零/全一是可能结局，属有效诊断而非失败；该情形按 Stop when 返回 Plan 调整（可能需最小 clock 使能写序的专门契约与依据）。
- 偏移来源以版本匹配的 stmmac 源码为准；若上游源与板上二进制行为不符，以板上读值与文档正文并记，返回 Plan。
- 顺手项：板卡丝印修订仍未补记（Iteration 001 Minor），操作者到板边时补一行，不阻塞。

## Act Response

- Status: blocked（info 级诊断轮已回传并定位 fault 层——**clock/reset handoff 层（总线挂死变体）**：MAC 窗口在 axruntime 区域表中（映射组合链正常，PT/PMP 假设排除），info 级下首读仍完全静默（无 panic/warn，CPU 未从 load 返回）。与参考仓库真板经验吻合：handoff 时 GMAC 总线时钟关闭，读门控外设挂死总线。按 Blocker Resolution 约定证据交 Plan 修订契约；4.1 Forbidden clock/reset 写仍生效，Act 不实施 APMU 写序）

**Implemented**

- 新增 `kernel/src/platform/mac_probe.rs`（自包含，仅 core 依赖）：
  - 寄存器常量带来源引注：`MAC_REG_VERSION = 0x110`（`GMAC4_VERSION`，`drivers/net/ethernet/stmicro/stmmac/hwif.h:691` @ v6.18.3，"GMAC4+ CORE Version"；gmac4/xgmac 路径 `hwif.c:323` 经 `stmmac_get_id(priv, GMAC4_VERSION)` 读取；User ID=bits[15:8]、Synopsys ID=bits[7:0]，`hwif.c:15-28`）、`MAC_REG_DEBUG = 0x114`（`GMAC_DEBUG`，`dwmac4.h:33` @ v6.18.3）；`MAC_BASE_PADDR=0xcac8_2000`/`MAC_WINDOW_SIZE=0x2000`（板 DT `ethernet@cac82000` reg，入仓 `tools/k3_com260_ifx.dtb`）。
  - 纯分类逻辑：`MacReadClass{Ok,AllZero,AllOne}` + `classify_mac_read` + `class_label`；`format_read_line` 手工拼行（无 alloc/format 依赖）产出 `[starry-k3] stage=4 mac <reg> read<N>=0xXXXXXXXX class=<label>`；`probe_and_report(emit)`（`unsafe`，只读 volatile）先输出 `probe begin` 行，再对 version/debug 各读 `READS_PER_REG=2` 遍全部输出，最后 `probe done` 行。内核内无期望值判定。
- `kernel/src/platform/k3.rs`：`run_first_byte_park()` 在 stage=3 行后调用 `mac_probe::probe_and_report(|line| console.write_str(line))` 再 wfi；模块 doc 的分层观测表补 stage=4 语义。stage=1/2/3 三行文本逐字未动。
- `kernel/src/platform/mod.rs`：+`pub mod mac_probe;`（沿 `pub mod k3;` 无条件声明先例）。
- `crates/axplat-riscv64-k3/axconfig.toml`：`devices.mmio-ranges` 追加 `[0xcac8_2000, 0x2000]`（原仅有 UART 窗口）——MAC 窗口经 axplat `MemIf::mmio_ranges()` → axhal `memory_regions()` 进入内核地址空间恒等映射，与 UART 已被板上验证的机制一致。
- `crates/axplat-riscv64-k3/build.rs`（第一轮板上后新增，逐字复用 `crates/axplat-riscv64-lichee-d1/build.rs` 模式）：`rerun-if-env-changed=AX_CONFIG_PATH` + `rerun-if-changed=<生成配置路径>`——修复 axconfig 内容变更不触发 axplat crate 重编译的构建陈旧缺陷（详见「第一轮板上结果与修复」）。
- `tests/mac-probe-host-harness.rs`（新增，6 测试）+ `Makefile host-test`（early-console 之后插入两行）：偏移常量钉扎、四类分类输入、标签稳定性、正常/全零/全一行格式。
- `.gitignore`：+`*.fit`（Iteration 002 Review Minor ①，本 change 产物卫生）。

**Deviations from Plan**

1. 分类纯函数与新 host 载体落在新增自包含模块 `kernel/src/platform/mac_probe.rs`，k3.rs 只保留调用点。契约 Targets 写「`kernel/src/platform/k3.rs`（MAC 只读探针阶段 + 读值分类输出）」并明示「新 host 测试载体……以 Act 就近选择并在 Response 记录」：k3.rs 引用 `super::{console,descriptor,early_console}` 与 `riscv::asm::wfi`，host harness 直接 `#[path]` 包含会引入这些依赖；自包含模块使 harness 与 early-console 惯例一致（单文件 `#[path]`）。
2. 访问异常层不由 trap handler 承载：stvec 保持 fail-closed 不动（D3），「MMIO 映射层」由串口形态定位——`probe begin` 行之后无任何寄存器行即首次读 trap。与既有 stage 标记诊断设计同构，未引入新机制；分类枚举按此不含 fault 变体（trap 读不返回）。
3. stage=3 行文本（`parked, halting (reset to recover)`）保持逐字不变，stage=4 线实际在其后输出——Preserve「stage=1/2/3 语义与顺序不变」的严格读法优先；措辞略超前于实际 park 点，记 Minor。
4. 新增 `crates/axplat-riscv64-k3/build.rs`（Targets 文件清单外）：第一轮板上 trap 后定位出的构建陈旧缺陷的唯一最小修复，使契约 Target「axconfig.toml 补 mmio-ranges」在构建产物中真实生效；逐字复用 lichee-d1 同名文件（公共规则 › 复用已有实现）。lichee-d1 axplat crate 早有该文件而 k3 crate 在 Iteration 002 创建时遗漏。
5. 首轮 trap 未按 Stop when 返回 Plan 的理由（记录供 Plan Review 审计）：Stop when「访问异常→MMIO 映射层」针对映射机制本身的证伪；本轮 trap 发生在**未含契约已规定的 mmio-ranges 条目的制品**上（构建缺陷使 Required behavior 的前提——映射存在——未被真正测试）。Act 自检发现自身 Gate 4 验证缺口（只核对了配置源文件、未核对二进制常量），修复并补二进制级验证属完成当前任务，非新设计。**若修复后镜像板上仍在 `probe begin` 后 trap，即真正命中 Stop when，立即 blocked 返回 Plan。**

**Self-Review（Gate 3）**

- Spec compliance：4.1 Required behavior 逐项核对——按可追溯偏移只读 version+debug 两寄存器 ✓；每寄存器 ≥2 遍且全部输出（raw+分类）✓；分类为 host 可测纯函数且四类输入先 RED 后 GREEN ✓；异常分层（trap→begin-行形态；all-zero/all-one→分类行）并在该层停止（Stop when 返回 Plan，不推测）✓；无任何 MAC/clock/reset/pinctrl 写 ✓；无内核内期望值判定（对照在 Response）✓；无 MDIO/PHY ✓。Invariants：stage 先期语义与顺序不变 ✓；既有平台构建 exit 0 ✓；`k3` 参考仓库零接触 ✓。
- Code quality：新增面无新增警告（k3 最小 feature 集下 2 个既有 unused helper 警告维持 Iteration 002 Minor 记录，非本次引入）；diff 无计划外文件；无身份型证据机制/判定层（`dumpimage -l`/`readelf`/`strings`/字节搜索均为原生命令直读）；`probe_and_report` 的 `unsafe` 契约成文（映射+handoff 前置、只读、窗口内）。
- 自检发现并修复（Important，已闭合）：初版 Gate 4 只验证了配置**源文件**与镜像地址/标记，未验证**二进制内的映射常量**——第一轮板上 trap 暴露该缺口。修复 build.rs 后，二进制级常量检查纳入本轮验证面并命中；该检查为一次性原生命令直读，不新增判定层。
- Critical/Important：无未闭合项。

**Verification Evidence（Gate 4）**

- RED：`rustc --edition=2024 --test tests/mac-probe-host-harness.rs …` → exit 1，`error[E0583]: file not found for module 'mac_probe'`（实现前）。
- GREEN（host 分类）：同命令编译后运行 → `test result: ok. 6 passed; 0 failed`，exit 0。
- GREEN（构建）：`make k3` → exit 0；FIT `StarryOS_riscv64-k3.fit` 280,096 B；`dumpimage -l`：kernel `Load 0x180000000`、127,168 B、crc32 `d80004c4`（新镜像，区别于板上前两轮 `c57cb0f5`/`e4e9f37d`）；FDT `Load 0x181000000`、150,998 B、crc `8711c0d6`（不变）。
- 三方一致：ELF `e_entry 0xffffffc180000000` − `PHYS_VIRT_OFFSET` = `0x180000000` = ITS load/entry = axconfig `kernel-base-paddr`。
- 区间算术（窗口 `[0x160000000, 0x2f0000000)`，payload ≥`0x180000000`）：kernel 物理足迹终点 `0x180020000+0x263000=0x180283000`（ELF LOAD memsz 复算）；FDT `[0x181000000, 0x18124E36)`；staging `[0x2E0000000, 0x2E0044680)`（280,096 B）。两两不重叠，不触 CMA/SWIOTLB（`0x2f0e00000`）/U-Boot reserved（`0x2fbd7c000`）。
- stage=4 标记入 ELF：`strings` 命中 `probe begin`/`probe done` 及行模板。
- 回归：`make ARCH=riscv64 build`（qemu）→ exit 0；`make lichee` → exit 0；early-console host 六项 GREEN 采信 Iteration 002 结论（本轮未触及 console.rs/early_console.rs，harness 编译面独立）。修复 build.rs 后 qemu/lichee 构建面（各自 axplat crate 与 kernel crate）零变化，上述 exit 0 结论采信。
- 偏移来源（写入 Response 的正文依据）：上游 v6.18.3 原文（git.kernel.org stable tree plain，本会话拉取核对）——`hwif.h:690-691`（`GMAC_VERSION 0x20`/`GMAC4_VERSION 0x110`）、`hwif.c:15-28`（UserID/SynopsysID 拆分与读取）、`hwif.c:321-325`（gmac4 路径选 `GMAC4_VERSION`）、`dwmac4.h:33`（`GMAC_DEBUG 0x114`）。板上 6.18.3-generic 驱动 `dwmac-spacemit-ethqos` 的打印行为与该源一致（`User ID 0x10`/`Synopsys ID 0x54` ⇒ version 裸值期望 ≈ `0x00001054`，对照在本 Response 完成，不入内核）。

**构建陈旧缺陷修复后的补充验证（Gate 4 追加）**

- `make k3` → exit 0，构建日志可见 `Compiling axplat-riscv64-k3`（build.rs 新增触发重编译）；`.axconfig.toml` 生成物 `mmio-ranges` 含 `[0xd401_7000, 0x1000]` 与 `[0xcac8_2000, 0x2000]` 两项（直接读取）。
- **二进制级常量检查**：`StarryOS_riscv64-k3.bin` 中搜索小端 `(u64,u64)` 对——MAC 窗口 `(0xcac82000, 0x2000)` 命中 offset 93176、UART 窗口 `(0xd4017000, 0x1000)` 命中 offset 93160（相邻数组项，与 MMIO_RANGES 布局一致）。修复前同检查 MAC 命中为 -1（缺陷直接证据）。
- 修复后镜像：kernel crc32 `c5a5f73e`（127,168 B，尺寸与修复前相同）；FIT 280,096 B；`dumpimage -l` load/entry 不变（kernel `0x180000000`、FDT `0x181000000`）；ELF `e_entry 0xffffffc180000000`；stage=4 标记字符串在 ELF 内（3 处）。区间算术与修复前一致（尺寸未变）。

### 4.1 第一轮板上结果与构建陈旧修复（2026-10-01）

**板上现象（操作者执行，第一轮）**：装载/校验正常，`bootm` 后串口完整出现 `[k3:pt-mmu]` → axruntime 横幅（riscv64-k3 / smp=1）→ stage=1/2/3 → `[starry-k3] stage=4 mac probe begin base=0xcac82000 (read-only)`，**此后无任何输出**（version read1 行缺失）。按分层设计判定：MAC MMIO 首次读（`0xcac82000+0x110`）访问异常，停 MMIO 映射层。内核 dead（wfi 未达）；复位回原系统路径不受影响。

**host 侧根因定位（trap 后立即执行，只读检查）**：

- 对第一轮制品 `StarryOS_riscv64-k3.bin`（crc `d80004c4`）做小端 `(u64,u64)` 字节搜索：UART 窗口 `(0xd4017000, 0x1000)` 命中，MAC 窗口 `(0xcac82000, 0x2000)` **未命中（-1）**——板上内核的 `MMIO_RANGES` 仍为 Iteration 002 编译的旧常量（仅 UART），探针读未映射 VA → trap，与串口形态互证。
- 根因：`crates/axplat-riscv64-k3/` 缺 `build.rs`。`axconfig_macros::include_configs!` 在宏展开期读取 `AX_CONFIG_PATH` 指向的生成配置；make 层每轮正确重生成 `.axconfig.toml`（`make/config.mk::oldconfig`）并导出 env（`make/Makefile:136`），但 cargo 无 `rerun-if-changed` 触发，axconfig 内容变化不重编译 axplat crate。`crates/axplat-riscv64-lichee-d1/build.rs` 即该问题的既有解，Iteration 002 创建 k3 crate 时遗漏复制。

**修复与在制品证明**：新增 `crates/axplat-riscv64-k3/build.rs`（逐字复用 lichee 模式）→ `make k3` exit 0 且日志出现 `Compiling axplat-riscv64-k3` → 二进制级常量检查 MAC 窗口命中（offset 93176）→ 新 kernel crc `c5a5f73e`。映射机制（mmio-ranges → 恒等映射）未被本轮证伪——UART 恒等写通仍为其板上证据；MAC 条目此前从未真正进入制品。

**遗留与升级条件**：第一轮已消耗一次板上循环；两轮独立启动要求以修复后镜像重新计。若 `c5a5f73e` 镜像仍在 `probe begin` 后 trap：命中契约 Stop when「访问异常→MMIO 映射层」，Act 立即 blocked 返回 Plan（届时映射机制为真证伪，需 Plan 重审 mmio-ranges → axhal 映射链或 PMP 假设）。构建陈旧缺陷影响面：k3 crate 单点（lichee 已有防护；qemu/vf2 crate 无本地 axconfig 定制路径），不另立 Issue 候选；若 Plan 认为需防御性检查可由 Review 决定。

### 4.1 第二轮板上结果与阻塞交接（2026-10-01）

**板上现象（操作者执行，第二轮，镜像经 iminfo 板上确认 kernel crc `c5a5f73e`）**：装载/校验/`setenv fdt_high`/`bootm` 全过；串口出现 `[k3:pt-mmu]` → axruntime 横幅（smp=1）→ stage=1/2/3 → `stage=4 mac probe begin`，**此后仍无任何输出**——与第一轮完全同型。板载确认制品已含 MAC 常量（host 字节搜索 offset 93176 + iminfo crc 对照），「构建陈旧」解释排除。

**Stop when 判定**：命中契约「读取产生访问异常（trap/fault）→ 停 MMIO 映射层，返回 Plan」。映射机制假设（mmio-ranges → axplat `MemIf::mmio_ranges()` → axhal `ALL_MEM_REGIONS` → 运行时恒等映射）在该环节被板上证伪，或 fault 另有来源（PMP/PMA、stvec 状态、PT 映射缺失）——**host 侧无法进一步区分，Act 不猜测写序/不扩大变更面，正式 Blocker Handoff**。

**Blocker Handoff**：

- 发现位置：task 4.1 / 探针 stage=4 首读 / Gate 4 板上验证。
- Plan 预期：mmio-ranges 含 MAC 窗口后，运行时内核可恒等读 `0xcac82110`。
- 实际情况：两轮独立板上执行（两镜像、一轮未含常量、一轮含常量）均在 `probe begin` 行后静默；axcpu 对不可分派异常应 `panic!("Unhandled trap … stval=…")`、对未注册页错误应 `warn!`——两者均未现形（warn 级可见而未见），静默本身成为新疑点（stvec 状态、panic 打印路径或 fault 形态待区分）。
- 影响：task 4.1 板上验证无法达成；Mapping-layer 假设需 Plan 重新调查（候选：axhal riscv64 是否真的把 mmio-ranges 映射进活动 satp；boot PT 是否已被替换；PMP/PMA 差异；trap 打印为何静默）。
- 已完成：host 全部（探针/分类/harness/构建/FIT/build.rs 修复）；板上两轮记录。
- 工作区：本 Cycle 改动面 = Deviations 1–4 清单 + 探针代码；无计划外修改。
- 恢复条件：Plan 对映射链重新调查并给出新契约；或板上 info 轮证据定位 fault 层后由 Plan 修订。

**Blocker 上下文补充——参考仓库真板经验（用户指令采集，2026-10-01）**：用户指令「去 k3/others 里面别人的仓库看看别人的真板经验……可以进行借鉴」。对 `../k3/others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/`（同板 COM260 真板验证的 GMAC 驱动，AP 侧 StarryOS 同为 FIT uimg + bootm 启动）只读调查得到决定性事实：

- `syscon.rs:3-6`：**「K3 的 GMAC 没有独立的时钟/复位寄存器，而是借用 APMU 系统控制器的 CTRL/DLINE 两个 32 位寄存器」**；`regs.rs:174-180` 注释：「StarryOS 无 CCF，必须在 glue 里显式置 1（`EMAC_BUS_CLK_EN`），否则 GMAC DMA 寄存器无时钟」。位定义来源标注为 ccu-k3.c（`emacx_bus_clk` BIT(0)）+ reset-spacemit.c（deassert_mask BIT(1)）+ 用户手册 14.3.4.1，序列来源为 Linux `dwmac-spacemit-ethqos.c` L116-152 与 U-Boot `dwc_eth_qos_spacemit.c`。
- `syscon.rs:209-241`：probe 阶段**先于任何 GMAC 寄存器访问**执行 APMU CTRL 两步写（`syscon.rs:212-213`：「CTRL 必须先置 BUS_CLK_EN | BUS_RST_DEASSERT，否则 GMAC DMA 寄存器无时钟」；step1 置 bit0=1 + PHY_INTF_MODE（RGMII=`0b01<<3`）后 udelay(100)，step2 置 bit1=1（释放复位）再 udelay(100)；警告「不先关时钟——先关→开→释放的三步脉冲反而让 DMA 子块 SFT_RESET 永不清零」）。DLINE（offset `0x3f0`）为 RGMII 延迟线，寄存器可读不需要。
- 板级事实闭环：APMU 即板上 DTB `system-controller@d4282800`（`spacemit,k3-syscon-apmu`，reg `<0x0 0xd4282800 0x0 0x400>`，phandle `0x05` = ethernet@cac82000 节点 `spacemit,apmu=<0x05>`）；CTRL offset `0x3ec`、DLINE offset `0x3f0`（节点 `spacemit,ctrl-offset`/`spacemit,dline-offset`，与 Iteration 001 裁决 clocks `d4282800` 一致）。
- 与两轮板上静默的契合：真板驱动明确 handoff 默认态为「时钟未开、复位未释放」；读时钟门控外设在本 SoC 上表现为总线访问无限停滞（无异常、无 trap 打印、无 warn）——与「`probe begin` 后完全静默」精确吻合，且解释了 axcpu 不可分派异常 `panic!` 未现形。_mapping 假设由此改判：PT/PMP 层待 info 轮区域表佐证，但首要假设转为 **clock/reset handoff 层（总线挂死变体）**_。
- 对契约的影响：4.1 Forbidden「任何 clock/reset 写操作」+ R5/D4「handoff 不可用则返回 Plan 调整，不让 Act 猜写序」——「最小 clock 使能写序的专门契约与依据」现已存在（上述可溯源、真板验证的 CTRL 两步序列），按预设路径由 Plan 修订 003（并前瞻 004：真板驱动注明 PHY MDIO 前需 GPIO1_5 复位 RTL8211F，否则 PHYID 读 0xffff；pinctrl mux 由框架在 probe 前应用）。
- 边界申明：Act 未实施任何 APMU/GMAC 写序（4.1 Forbidden 仍生效）；以上为 Blocker 上下文与 Plan 修订依据。

**Blocker Resolution（用户指令，2026-10-01）**：用户原话「我们可以编译info级别用info来定位问题，如果不知道如何进行可以查看runbook」。按此恢复 Act：`make k3 LOG=info` exit 0（`features.mk` 校验 LOG 并映射 axfeat log-level feature）；MAC 常量复验命中（offset 93176）；新 kernel crc `66677180`。info 级预期新增可见信息：axruntime `info!("Found physical memory regions:")` 完整区域表（含 mmio 项——直接验证 MMIO_RANGES→`ALL_MEM_REGIONS` 组合链）、platform/interrupt 初始化日志，以及 trap 打印（若其输出此前被日志门控）。本轮为**单轮诊断采集**，不计入 4.1 的两轮独立启动验收；采集结果回传后：若定位出 fault 层，证据交 Plan 修订契约（Act 不实施新设计）；若信息仍不足，Blocker Handoff 重新生效返回 Plan。

### 4.1 操作者指令包（板上步骤，用户执行；串口 115200 8N1 无流控）

前置：新 FIT 已在仓库根 `StarryOS_riscv64-k3.fit`（**info 级诊断镜像**，kernel crc32 `66677180`——勿与前两轮 `d80004c4`/`c5a5f73e` 混淆）。拷入 FAT32 U 盘（整盘无分区表）后插板 USB-A。本轮为单轮诊断采集，重点抓 stage=4 之前的全部 info 日志。

1. 给板上电/按 RESET 的同时长按 `s` 键进 `U-Boot>`（已验证手法）。
2. 装载到 staging：
   ```
   usb start
   fatls usb 0:0 /
   fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
   ```
   （文件名大小写敏感；不用 `${kernel_addr_r}`——stock 值落 CMA 禁用区。）
3. 只读校验并启动：
   ```
   md.b 0x2e0000000 0x10        # 首字节 27 05 19 56（FIT magic）
   iminfo 0x2e0000000           # kernel crc 应为 66677180（info 级诊断镜像）
   setenv fdt_high 0xffffffffffffffff   # 易失，必须；阻止 FDT 被重定位出映射
   bootm 0x2e0000000
   ```
4. 串口期望与采集重点（info 级新增内容，逐行记录）：
   - 横幅 `log_level = info`（确认诊断镜像生效）。
   - **`Found physical memory regions:` 后的区域表**——重点看是否有 `mmio` 名目、`0xcac82000`/`0x2000` 与 `0xd4017000`/`0x1000` 两项（验证常量→region 组合链）。
   - `Initialize platform devices...`/`Initialize interrupt handlers...` 等初始化日志。
   - stage=1/2/3 → `probe begin` 之后的一切输出——**任何 panic/trap 行（含 scause/stval）都是本轮最关键证据**；仍静默也照实记录。
5. 恢复：`reset` → 应回 `k3 login:`，记录一句话。
6. 本轮为单轮诊断，**不需要**重复循环（两轮独立启动要求留待诊断结论落地后的正式验收轮）。
7. 结果回传：命令序列 + **完整串口输出原文**（尤其 `Found physical memory regions:` 区域表与 stage=4 之后的一切）粘贴回会话（Evidence `board-mac-regs.txt` 由 Act 依格式落盘）。
8. 禁止：`saveenv`、任何刷写/持久写入、盲写 clock/reset/pinctrl 寄存器、把静默计为成功。
9. 顺手项（非阻塞）：补记板卡丝印修订一行。

### 4.1 info 级诊断轮结果与层定位（2026-10-01，操作者执行，镜像 crc `66677180`）

板上串口决定性输出（原文摘录，操作者回传）：

- axruntime 区域表含 MAC 窗口：`[PA:0xcac82000, PA:0xcac84000) mmio (READ | WRITE | DEVICE | RESERVED)`，UART 窗口同列（`[PA:0xd4017000, PA:0xd4018000) mmio …`）——`MMIO_RANGES → MemIf::mmio_ranges() → axhal ALL_MEM_REGIONS` 组合链**板上证实正常**；`kernel aspace: [VA:0xffffffc000000000, …)`、`Primary CPU 0 init OK`，初始化全程无异常。
- 随后 stage=1/2/3 → `stage=4 mac probe begin` → **完全静默**：info 级下无 `Unhandled trap` panic、无 `warn!`、无任何后续日志——CPU 未从首读 `0xcac82110` 返回且未产生可捕获异常。

**层判定**：clock/reset handoff 层（总线挂死变体）。PT 映射/PMP/stvec/日志门控假设全部排除（区域表在列 + 全级别静默 + axcpu panic 路径未触发）。与 Blocker 上下文补充中的真板经验唯一自洽：APMU CTRL（`0xd4282800+0x3ec`）bit0/bit1 均为 0（时钟关、复位保持），读时钟门控的 GMAC 挂死 AXI 总线。

**Act 处置（按 Blocker Resolution 约定）**：诊断目标达成，证据交 Plan。4.1 现行契约下无法完成（读值前置的 APMU CTRL 写属 Forbidden），需 Plan 按 D4 修订：加入最小 clock 使能写序契约（可溯源依据已在 Blocker 上下文补充中列全——APMU 基址/窗口、CTRL `0x3ec`、bit0+RGMII 模式 → udelay(100) → bit1 → udelay(100)；来源 ccu-k3.c/reset-spacemit.c/用户手册 14.3.4.1/U-Boot `dwc_eth_qos_spacemit.c`/Linux `dwmac-spacemit-ethqos.c` L116-152，经 Rt-Async-AMP 同板真板验证）。GMAC 寄存器读保持只读。诊断轮不计入两轮独立启动验收。

**Persisted Evidence**

- Mode: required
- `evidence/003-mac-register-baseline/000-initial/board-mac-regs.txt`：**待创建**——板上两轮回传后由 Act 按 evidence-format 落盘（含 Cycle README）；通过条件：两轮完整 stage=1–4 序列 + 每寄存器 ≥2 遍读值与分类行 + 两次复位回原系统记录。当前未创建（板上采集未发生，无占位目录）。

**Experience Candidates**

- None（本轮无新增可复用操作路径或范围外实质缺陷发现；vf2 pre-existing 构建失败候选已在 Iteration 002 Response 在案）。

**未解决问题**

- task 4.1 被阻塞：板上两轮 + info 级诊断轮定位 fault 层为 **clock/reset handoff（总线挂死）**——读值前置的 APMU CTRL 写序属现行契约 Forbidden，Act 不实施。需 Plan 按 D4 修订 003 契约（最小 clock 使能写序的可溯源、真板验证依据已在 Blocker 上下文补充列全），修订后由 Act 继续。寄存器偏移来源、映射组合链、构建陈旧修复三项已闭合不受影响。

## Plan Review

- Review Result: rework-required

**Findings**

1. 阻塞（新证据 + Plan 遗漏）：000 契约前提「handoff 后 MAC 寄存器可读」被板上证伪。三轮板上执行（镜像 kernel crc `d80004c4` / `c5a5f73e` / `66677180`）一致显示 `stage=4 mac probe begin` 后完全静默；info 诊断轮区域表含 MAC 窗口（`[PA:0xcac82000, PA:0xcac84000) mmio (READ|WRITE|DEVICE|RESERVED)`），排除映射组合链；全级别无 trap panic / warn 输出，排除 PT 缺失、PMP、stvec 与日志门控假设。结论：U-Boot handoff 后 GMAC 总线时钟关闭（APMU CTRL bit0/bit1=0），读时钟门控外设挂死 AXI 总线且不产生可捕获异常。R69（Rt-Async-AMP `syscon.rs`）同板真板注释与该现象精确互证（「CTRL 必须先置 BUS_CLK_EN | BUS_RST_DEASSERT，否则 GMAC DMA 寄存器无时钟」）。Plan 遗漏：000 调查未主动检索该参考——其价值 2026-09-29 起已在 R62/R66 摘要在案（现已登记 R69–R73 并立 M42 主动引用规范）；若 Plan 阶段检索过，clock 门控风险本可预先进入契约。
2. 非阻塞：Act 五项 Deviations 复核接受——自包含模块与 host 载体选择、trap 分层由串口形态承载（stvec fail-closed 不动）、stage=3 措辞 Minor、`build.rs` 构建陈旧修复（逐字复用 lichee 模式，有配置源/二进制两级证据）、首轮 trap 后修复继续（契约前提未被测试 vs 前提证伪的区分成立，且已按约定在第二轮同型时升级为正式 Blocker）。构建陈旧缺陷定界 k3 crate 单点，同意不另立 Issue 候选。
3. 非阻塞（沿 000 在案）：vf2 pre-existing 构建失败 Issue 候选维持未落账；`make host-test` ms04 harness pre-existing 失败维持。
4. host 侧结论采信（覆盖未变化）：分类函数与探针实现、六项 host 测试、`make k3`/FIT/区间算术、上游 v6.18.3 偏移来源核对均未失效，`001-rework` 直接继承，不重复实施。

**Acceptance Gaps**

- D1（MAC 读值非全零/全一、身份可解释）：板上读值未取得——clock 门控使读不可达。
- D2（两轮独立启动读值一致 + 每轮复位回原系统）：前三轮均为诊断消耗且未产生读值，需以含 clock 写序的新镜像重新执行两轮。
- D3：host 侧成立（来源可追溯、零寄存器写入、既有构建与测试保持）；自 `001-rework` 起 4.1 需要契约化 APMU 写序，D3 写入边界由 `001-rework` 更新为「除契约 APMU CTRL 0x3ec 两步写序外零寄存器写入」。

**Evidence**

- 板上三轮串口现象与层定位：000 Act Response「第一轮板上结果与构建陈旧修复」「第二轮板上结果与阻塞交接」「info 级诊断轮结果与层定位」三节（操作者回传原文摘录在案）。
- info 轮区域表输出（MAC 窗口在列、初始化无异常）：同上节摘录。
- 决定性参考（本轮 Plan 直接读取核对）：R69 `tgoskits/drivers/ax-driver/src/net/k3_gmac/syscon.rs:209-263`（两步写序、三步脉冲警告、RMW 语义、spin delay）与 `regs.rs:167-181`（位常量与来源标注）。
- 板 DTB 独立复核（`tools/k3_com260_ifx.dtb`，fdtget）：`/soc/ethernet@cac82000` `phy-mode=rgmii`、`spacemit,ctrl-offset=1004(0x3ec)`、`spacemit,dline-offset=1008(0x3f0)`、`spacemit,apmu` phandle 5；`/soc/system-controller@d4282800`（`spacemit,k3-syscon-apmu`，reg `0xd4282800/0x400`，phandle 5）；`spacemit,wake-irq-enable` 缺席。
- host 基线：000 Act Response Verification Evidence 各项（未失效，采信）。

**Follow-up Decision**

修复 4.1 需要解除 000 Forbidden（clock/reset 零写入）并建立新的自包含执行契约（APMU 最小 clock 使能写序、stage 观测扩展、更新后的零写入边界与停止条件），超出「当前 Cycle 有限修复」边界；design D4 预设调整路径的依据条件已满足（写序可溯源且同板真板验证：R69 + 板 DTB + 上游 Linux/U-Boot 标注三方吻合）。Iteration 目标、Acceptance 语义与 Iteration Map 不变 → 创建 rework Cycle，repair item `4.1-R1`。

**Next Cycle**

`001-rework.md`（同目录；Cycle Type: rework；Plan Context 自 `draft` 起，待用户批准后置 `ready`）

**Next Iteration**

None（Iteration 003 未完成；Iteration 004 保持未展开）
