# Iteration 003 / Cycle 001: MAC 只读寄存器基线（APMU clock 使能后重验）

## Plan Context

- Status: ready
- Cycle Type: rework
- Parent Cycle: `000-initial`（Review Result: rework-required，2026-10-01）

**Cycle Scope**

- Change tasks: 4.1（全局 T14 后半）
- Acceptance gaps: D1（MAC 读值未取得——clock 门控使读不可达）；D2（两轮独立启动未以有效镜像执行）
- Repair items: 4.1-R1（APMU 最小 clock 使能写序，使既有只读 MAC 探针可达）
- Inherited scope: 000 全部 host 侧成果（`mac_probe.rs` 探针与分类、六项 host 测试、`make k3`/FIT 链、`build.rs` 修复与二进制常量验证机制、上游 v6.18.3 偏移来源核对）；R68 Runbook 板上流程；Iteration 001 裁决表；R69 位/偏移依据；R1/R5/R6 与 design D4。
- Excluded scope: DLINE 写、PHY GPIO 复位（5.1）、MDIO/link（tasks 5.1–5.2）、pinctrl 写、MAC IRQ、DMA ring、packet I/O、异步网络、SMP、性能。

**Objective**

在 K3 探针中先按 R69 可溯源的两步写序使能 GMAC 总线时钟（APMU CTRL `0x3ec`：bit0 开时钟 + RGMII 接口模式 → 100 µs → bit1 释放复位 → 100 µs，RMW 保留其他位），再执行既有只读 MAC 探针；板上两轮独立启动中 APMU 读回 bit0/bit1 置位、MAC version/debug 读值非全零/非全一且身份可解释（对照板上 Linux 驱动 `User ID 0x10`/`Synopsys ID 0x54`，判定在 Act Response），每轮复位回原系统。

**Background**

000 三轮板上执行证明 U-Boot handoff 后 GMAC 总线时钟关闭，读时钟门控外设挂死 AXI 总线且无 trap（info 轮已排除映射组合链、PMP、stvec、日志门控）。000 契约将 clock/reset 写列为 Forbidden，故 Act 正式 Blocker 交接。design D4 预设路径「handoff 不可用则返回 Plan 调整契约」的依据条件现已满足：R69 同板真板验证的两步写序（来源标注 Linux `dwmac-spacemit-ethqos.c` L116-152、U-Boot `dwc_eth_qos_spacemit.c`、ccu-k3.c、reset-spacemit.c、手册 14.3.4.1）与板 DTB 属性、本仓板上现象三方吻合。Plan 遗漏（000 未主动检索 R69）已由 M42 + R69–R73 登记修复。

**Investigation Facts**

- Current Baseline: `k3` 分支工作树（HEAD `160d7967` + 未提交 MS09 改动）。000 host 侧全绿且采信：`make k3` exit 0、FIT 打包/区间算术、host 六项测试、`build.rs` 修复使 axconfig 变更触发重编译（板上已验证）。
- Current-State Evidence（本轮 Plan 直接读取核对）：
  - 写序正文（R69 `syscon.rs:242-263`）：step1 `update_bits(CTRL, EMAC_BUS_CLK_EN|PHY_INTF_MODE_MASK, EMAC_BUS_CLK_EN|PHY_INTF_RGMII)` → `delay_us(100)`；step2 `update_bits(CTRL, EMAC_BUS_RST_DEASSERT|WOL_WAKE_IRQ_EN, EMAC_BUS_RST_DEASSERT|0)` → `delay_us(100)`；随后读回打印。注释：「序列严格匹配 U-Boot eqos 驱动（dwc_eth_qos_spacemit.c）：step 1 开总线时钟（bit0=1）+ 接口模式 → udelay；step 2 释放复位（bit1=1）→ udelay」；关键警告「不先关时钟——先关→开→释放的三步脉冲反而让 DMA 子块 SFT_RESET 永不清零」。
  - 位常量（R69 `regs.rs:167-181`）：`EMAC_BUS_CLK_EN=1<<0`、`EMAC_BUS_RST_DEASSERT=1<<1`、`PHY_INTF_MODE_MASK=0b11<<3`、`PHY_INTF_RGMII=0b01<<3`、`WOL_WAKE_IRQ_EN=1<<12`；来源标注 ccu-k3.c（`emacx_bus_clk` BIT(0)）+ reset-spacemit.c（deassert_mask BIT(1)）+ 用户手册 14.3.4.1。
  - RMW 语义（`syscon.rs:307-310`）：`update_bits = (old & !mask) | (value & mask)`，mask 外全部位保持。
  - delay 实现（`syscon.rs:312-319`）：`us×50` 次 `core::hint::spin_loop()` 自旋，无定时器依赖——与本仓最小平台（无 timer 初始化）兼容。
  - 板 DTB 复核（`tools/k3_com260_ifx.dtb`，fdtget）：`/soc/ethernet@cac82000` `phy-mode=rgmii`、`spacemit,ctrl-offset=1004`（=0x3ec）、`spacemit,dline-offset=1008`（=0x3f0）、`spacemit,apmu` = phandle 5；`/soc/system-controller@d4282800`（compatible `spacemit,k3-syscon-apmu`，reg `<0x0 0xd4282800 0x0 0x400>`，phandle 5）。`spacemit,wake-irq-enable` **缺席** → 本板 step2 不触碰 WOL 位（较参考实现更保守：mask 仅 bit1）。
  - 004 前瞻事实（记录不实施）：`snps,reset-gpios=<148 1 5 1>`（GPIO1_5、active-low）与 `snps,reset-delays-us=<0 20000 100000>`；R69 注释：不复位 PHY 则 MDIO 探测 PHYID 读 0xffff。
  - 现实现挂点：`kernel/src/platform/k3.rs::run_first_byte_park()` stage=3 后调 `mac_probe::probe_and_report`；`mac_probe.rs` 自包含（host `#[path]` 直含，无 alloc/format 依赖）；`crates/axplat-riscv64-k3/axconfig.toml` `devices.mmio-ranges` 现含 UART `[0xd401_7000,0x1000]` + MAC `[0xcac8_2000,0x2000]`，APMU 窗口 `[0xd428_2800,0x400]` 待补（与 UART/MAC 同一恒等映射机制，板上已验证）。
- Code and Critical Path: `mac_probe.rs`（APMU 常量 + step 纯函数 + spin delay + stage=4 APMU 序列 + stage=5 MAC 前缀更新）→ `axconfig.toml`（+APMU 窗口）→ `k3.rs`（模块 doc 分层表更新；调用点位置不变）→ `tests/mac-probe-host-harness.rs`（新增 step/常量/格式测试，受前缀影响的既有测试同步更新）→ `make k3`（`build.rs` 已保证 axconfig 变更触发重编译）→ 板上按 R68 两轮 → 复位。

**Implementation Guidance**

顺序：host RED（step 纯函数测试先于实现）→ 实现常量/纯函数/spin delay/序列输出 → host GREEN → axconfig 补窗口与探针序列接线 → `make k3`（构建日志应出现 `Compiling axplat-riscv64-k3`）→ 板上两轮。关键取舍：step2 mask 仅 `EMAC_BUS_RST_DEASSERT`（DT 缺席 wake-irq-enable，WOL 位保持原值，较参考更保守）；DLINE 不写（RGMII 数据面调相，非寄存器读前置，属后续 link/数据通路工作）；delay 自旋不入 host 测试（时序语义在 host 无意义）；APMU 读回行输出原始值，位判定对照在 Act Response，内核不判定期望值。沿用 000 的手工拼行 helper 模式，不引入 alloc/format。

**Task Contracts**

### 4.1-R1: APMU 最小 clock 使能写序使 MAC 只读探针可达

- Requirement/Scenario: R5「MAC 寄存器与 PHY/link 构成独立可观测链路」（控制器和链路均可观察 / 寄存器访问失败）；R1（只使用已裁决字段与可追溯来源）；design D4 预设调整路径。
- Depends on: 000 host 侧成果（采信继承）；板上流程 R68。
- Targets: `kernel/src/platform/mac_probe.rs`（APMU 常量、step 纯函数、`delay_us` 自旋、stage=4 APMU 序列与读回输出、stage=5 MAC 前缀）；`tests/mac-probe-host-harness.rs`（新增测试 + 前缀更新）；`crates/axplat-riscv64-k3/axconfig.toml`（mmio-ranges += `[0xd428_2800, 0x400]`）；`kernel/src/platform/k3.rs`（模块 doc 分层表补 stage=4/5 语义）；`evidence/003-mac-register-baseline/001-rework/board-mac-regs.txt`。
- Current behavior: stage=4 直接读 MAC → 板上总线挂死（clock 门控），无任何读值。
- Required behavior: stage=4 依序输出 `[starry-k3] stage=4 apmu begin`、写前读回、step1（RMW：`(old & !(CLK_EN|INTF_MASK)) | (CLK_EN|RGMII)`）、step1 后读回、100 µs 自旋、step2（RMW：`(v & !RST_DEASSERT) | RST_DEASSERT`）、step2 后读回、100 µs 自旋、`stage=4 apmu clock enable done`；全部读回为原始 u32 值。stage=5 执行既有 MAC version/debug 只读探针（每寄存器 ≥2 遍，raw + 分类行，行前缀 `stage=5 mac`），begin/done 标记保持。host 可测纯函数覆盖：step1/step2 值计算（mask 外位保持、bit0/bit1/接口模式位置正确）、常量钉扎（`APMU_BASE_PADDR=0xd4282800`、`APMU_CTRL_OFFSET=0x3ec`、`EMAC_BUS_CLK_EN=1`、`EMAC_BUS_RST_DEASSERT=2`、`PHY_INTF_RGMII=0b01<<3`、`PHY_INTF_MODE_MASK=0b11<<3`）、APMU 行与更新后 MAC 行格式。
- Preserve: stage=1/2/3 三行文本与顺序逐字不变；GMAC 寄存器仍零写入（只读）；`make k3`/FIT/RAM 启动/复位路径；既有平台构建（qemu/lichee）与 early-console host 测试；`k3` 参考仓库只读；偏移/位常量来源引注成文（R69 文件:行号 + 板 DTB 属性 + 上游标注）。
- Forbidden: 任何 GMAC 寄存器写；APMU `0x3ec` 以外的任何 APMU 访问写（DLINE `0x3f0` 明确禁写）；「先关→开→释放」三步脉冲或任何先清时钟的变体；pinctrl/GPIO/MDIO/PHY 访问（5.1 范围，含 GPIO1_5 复位——仅记录事实）；内核内期望值判定（读回位与 `0x1054` 对照在 Act Response）；持久介质写入（saveenv/刷写）；身份型证据机制。
- Test witness: RED——新增 host 测试先于实现（step 纯函数不存在 → `rustc --test` 编译失败记录）；GREEN——`/tmp/mac-probe-host-test` 全绿 exit 0。板上见证——两轮启动的 stage=4 读回行与 stage=5 读值行本身。
- GREEN condition: host 测试 exit 0；`make k3` exit 0 且构建日志出现 `Compiling axplat-riscv64-k3`；两轮独立上电/复位循环中 stage=1–5 完整输出、step2 后 CTRL 读回 bit0/bit1 均为 1、MAC version 读值非全零/全一且与 `0x1054`（User ID `0x10`、Synopsys ID `0x54`）对照可解释、debug 读值非全零/全一、两轮读值一致、每轮 `reset` 回 `k3 login:`；qemu/lichee 构建 exit 0、early-console 六项 exit 0（采信未失效结论，仅重跑受影响面）。
- Verification: host——harness 与构建退出码；板上——按 R68 Runbook 装载启动（新镜像 kernel crc 记录于 Act Response），串口记录 stage=1–5 全序列（APMU 三个读回值 + MAC 每寄存器 ≥2 遍读值 + 分类），`reset` 回原系统，重复一轮；读回位与身份对照写入 Act Response。
- Stop when: `stage=4 apmu begin` 后静默（APMU 访问自身挂死或未映射）→ 停 syscon 访问层返回 Plan；step2 后读回 bit0/bit1 未置位（写不生效）→ 停 syscon 写生效层返回 Plan；`stage=4 done` 后 `stage=5 mac probe begin` 静默（时钟已开仍挂死）→ clock 门控假设证伪，返回 Plan（候选：PHY 时钟域、访问宽度、实例差异——不猜测实施）；MAC 读值全零/全一 → 停 clock/reset 更深层返回 Plan；读值可读但身份与 DWMAC 无法对应 → 停实例/偏移层返回 Plan。

**Invariants**

MAC 寄存器只读；APMU 仅 `0x3ec` 两步 RMW（mask 外位保持，无先关时钟动作）；handoff 其余状态保持；失败分层且 fail-closed；偏移/位/序列来源可追溯；stage=1/2/3 语义不变；既有平台与测试不回归；无 IRQ/DMA/MDIO/async/DLINE/pinctrl 写。

**Non-goals**

DLINE/RGMII 调相、PHY GPIO 复位与 MDIO（5.1）、link 观察（5.2）、MAC IRQ（MS10）、DMA、收发、数据面、性能。

**Acceptance**

- D1 / R5 / 4.1：stage=5 MAC 身份与状态读值非全零/全一，身份对照板上 Linux 驱动观测（version 裸值 ≈ `0x1054`）可解释（对照在 Act Response，内核不判定）。
- D2 / R5 / 4.1：两轮独立启动中 stage=4 读回 bit0/bit1 置位且一致、stage=5 读值一致；每轮复位回原系统。
- D3 / R1+R5+R6 / 4.1：偏移/位/序列来源可追溯（R69 `syscon.rs`/`regs.rs` + 板 DTB 属性 + 上游 Linux/U-Boot 标注）；异常分层报告且停止；除契约 APMU CTRL `0x3ec` 两步写序外零寄存器/持久写入；既有平台构建与 host 测试保持。

**Verification**

- 场景「控制器可观察」（R5）：两轮启动的 stage=4 读回行 + stage=5 读值行直接判定。
- 场景「寄存器或 PHY/link 访问失败」（R5）：Stop when 各层的串口形态（begin 静默 / 读回位未置位 / stage=5 静默 / all-zero、all-one 分类行）直接判定；host 测试覆盖 step 计算、常量与格式。
- 场景「对端未连接」不适用本轮（无 MDIO/link）；carrier/link 留 5.2。
- 兼容（R6）：`make k3`/qemu/lichee 构建、early-console host 测试退出码。

**Gate 2 Readiness**

PASS——调查完整（写序正文、位常量、RMW/delay 语义、板 DTB 属性、现实现挂点与测试面均已直接读取核对）；设计闭合（序列、mask、延时常量、行形态、停止条件无 TBD）；任务可执行且追踪完整。板上两轮为执行期能力边界（R68 Runbook 已固化，单轮成本低），不阻塞 Gate 2。无 BLOCKED / WAIVED 项。用户于 2026-10-01 审计本 Cycle 计划并回复原话「批准」，Status 已置 `ready`。

**Persisted Evidence**

- Mode: required

1. `evidence/003-mac-register-baseline/001-rework/board-mac-regs.txt` — 两轮启动的装载命令、stage=1–5 完整串口输出（含 APMU 三个读回值、MAC 每寄存器 ≥2 遍读值与分类行）、两轮复位观察、环境说明行（含镜像 kernel crc）。支持 D1/D2：APMU 读回与 MAC 读值是一次性板上现场事实，身份对照、004 PHY 契约与 MS10 设计依赖原始读值；会话后不可低成本重取。通过条件：两轮完整 stage 序列 + APMU 读回行 + MAC 读值行 + 两次复位回原系统记录。父 Cycle 000 计划中的同名 000-initial Evidence 未创建，由本 Review 的 Next Cycle 链替代，不再要求补建。

**Risks and Notes**

- CTRL 读回可能是写值镜像而非实际门控状态（SoC 实现差异）：读回位不粘滞时按 Stop when 返回，不现场补写。
- 时钟开启后 MAC 读值仍全零：可能存在 PHY 时钟域或更深的门控——返回 Plan，不猜测写序。
- R69 采集于参考仓库当前 revision（R62 登记）；参考仓库更新时按 M42 重新核对位/偏移。
- 顺手项（沿 000，非阻塞）：操作者到板边时补记板卡丝印修订一行。

## Act Response

- Status: reported
- Progress: repair item 4.1-R1 完成——host 侧全部实现与验证 GREEN；板上 Round 1 采集 APMU 两步写序写生效（`0x0`→`0x9`→`0xb` 读回粘滞）、MAC `version=0x1054` 身份精确命中；`debug=0x00000000` 经用户裁决为非实质形式问题（Blocker Resolution），Round 2 与 reset 观察经用户豁免，Cycle 以 Round 1 证据收口。等待用户审计与 `openspec-plan` Review。

**实际改动（host 侧，4.1-R1 全部代码面）**

- `kernel/src/platform/mac_probe.rs`：新增 APMU 常量（`APMU_BASE_PADDR=0xd4282800`、`APMU_CTRL_OFFSET=0x3ec`、`EMAC_BUS_CLK_EN=1<<0`、`EMAC_BUS_RST_DEASSERT=1<<1`、`PHY_INTF_MODE_MASK=0b11<<3`、`PHY_INTF_RGMII=0b01<<3`、`APMU_STEP_DELAY_US=100`；引注 R69 `syscon.rs`/`regs.rs`、ccu-k3.c、reset-spacemit.c、手册 14.3.4.1、板 DTB `spacemit,ctrl-offset=1004`/`phy-mode="rgmii"`）；新增 step 纯函数 `apmu_step1`/`apmu_step2`（RMW，mask 外位保持；step2 mask 仅 bit1，不触碰 WOL 位）；新增 `delay_us`（us×50 `spin_loop`，无定时器依赖）；新增 `format_apmu_line` 与 `apmu_clock_enable`（stage=4：begin → before 读回 → step1 写 → after1 读回 → 100 µs → step2 写 → after2 读回 → 100 µs → done，全部行为原始 u32 值）；`format_read_line` 与 MAC begin/done 标记前缀 `stage=4 mac` → `stage=5 mac`；模块 doc 改写为 APMU 写序来源、只读边界与串口失败形态分层。
- `tests/mac-probe-host-harness.rs`：新增 5 测试（APMU 常量钉扎、step1/step2 值计算与 mask 外位保持、零起写序链、APMU 行格式）；受前缀影响的两项既有格式测试期望更新为 stage=5；import 块 rustfmt 排序。
- `crates/axplat-riscv64-k3/axconfig.toml`：`devices.mmio-ranges` += `[0xd428_2800, 0x400]`（APMU 窗口，与 UART/MAC 同一恒等映射机制）。
- `kernel/src/platform/k3.rs`：模块 doc 分层表补 stage=4（APMU 访问层/写生效层两种失败形态）与 stage=5（MAC 读值层）语义；调用点位置不变（stage=3 行之后），先 `apmu_clock_enable` 后 `probe_and_report`。

**Deviations from Plan**

无实质偏差。两处契约内记录：(1) APMU 读回行标签钉扎为 `before/step1/after1/step2/after2`（契约未钉扎标签字面；行形态与「全部读回为原始 u32 值」一致，host 测试覆盖）；(2) GREEN condition 允许 qemu/lichee「采信未失效结论」，但 `k3`/`mac_probe` 模块经 `platform/mod.rs` 无条件参与全平台编译，覆盖面已变化，故本轮实际重跑两构建（均 exit 0）——比采信更保守。

**自检（Gate 3）**

stage=1/2/3 三行文本与顺序逐字未动；GMAC 寄存器零写；APMU 仅 CTRL 0x3ec 读/写，DLINE/WOL/pinctrl/GPIO/MDIO 未触碰；无「先关时钟」变体；无内核内期望值判定（after2 仅打印原始值，位对照留本 Response）；FIT load/entry 地址与 ITS 未变。发现并修复：harness import 未按 rustfmt 排序（已格式化并重跑 harness 保持 GREEN）。遗留 Minor：k3.rs stage=2 行存在 pre-existing 换行风格与 rustfmt 建议不一致（本轮未触碰该行，按 Surgical Changes 不清理）。

**验证（Gate 4）**

- RED（实现前）：`rustc --edition=2024 --test tests/mac-probe-host-harness.rs -o /tmp/mac-probe-host-test` → exit 1，`unresolved imports mac_probe::apmu_step1 …`（step 纯函数与常量不存在，契约见证形态）。
- GREEN：同命令编译 + 运行 → `test result: ok. 11 passed; 0 failed`，exit 0。
- `make k3` → exit 0，日志含 `Compiling axplat-riscv64-k3` 与 `Compiling starry-kernel`；FIT 重打包成功，`dumpimage -l`：kernel `Hash value: 87ffd369`、Load/Entry `0x180000000`、FDT load `0x181000000`（地址面与 000/002 一致）。
- 受影响面构建重跑：`make build`（qemu）→ exit 0；`make lichee` → exit 0。
- early-console host 六项：采信 Cycle 000 Act Response 结论（其采信 Iteration 002），本轮未触及 `console.rs`/`early_console.rs`，覆盖未变化。
- `openspec validate ms09-com260-kit-observable-link-baseline` → valid，exit 0。

**Persisted Evidence**

- `evidence/003-mac-register-baseline/001-rework/board-mac-regs.txt` + 同目录 `README.md` — 已创建，含 Round 1 串口采集原文（TLV serial# 脱敏，其余逐字保留）；第 2 轮与两轮 `reset` 观察待补齐。`required` 模式维持。host 侧验证结果按「普通验证结果写 Act Response」不入 Evidence。

**板上执行协议（Round 1 已按此执行，Round 2 待补）**

镜像：仓库根 `StarryOS_riscv64-k3.fit`（上文验证段产出，已含 APMU 序列）。板上 U-Boot `iminfo`/`bootm` 应显示 kernel crc32 `87ffd369`——不一致即板上载入旧镜像，停止并重新拷贝。

按 R68 Runbook 执行两轮独立上电/复位循环，每轮：

1. 上电/复位同时长按 `s` 进 `U-Boot>`；`usb start` → `fatls usb 0:0 /` → `fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit` → `iminfo 0x2e0000000`（crc 对照 `87ffd369`）。
2. `setenv fdt_high 0xffffffffffffffff`（必做）→ `bootm 0x2e0000000`。
3. 串口完整记录 stage=1–5：`[k3:pt-mmu]`、stage=1/2/3 三行、`stage=4 apmu begin`、`before/step1/after1/step2/after2` 五行、`stage=4 apmu clock enable done`、`stage=5 mac probe begin`、version/debug 各 ≥2 遍读值行、`stage=5 mac probe done`。
4. `reset` → 确认回 `k3 login:`。

GREEN 判定（对应 Acceptance）：两轮 stage 序列完整且读值一致；after1/after2 读回 bit0（0x1）与 bit1（0x2）均置位；MAC version/debug 非全零/全一，version 与 `0x1054`（User ID `0x10` / Synopsys ID `0x54`）对照可解释（对照在本 Response，内核不判定）；每轮复位回原系统。

Stop when（命中即停，串口原文交回，不现场补写）：`apmu begin` 后静默；读回位未置位；`apmu … done` 后 `stage=5 mac probe begin` 静默；MAC 读值全零/全一；身份无法对应 DWMAC。

串口全文（两轮装载 + stage 序列 + 复位观察 + 一行环境说明含 kernel crc）贴回会话；Act 采集入 Evidence 并按 GREEN 判定 / Stop when 处置——Round 1 已采集，处置结果见 Blocker Handoff。

**板上结果（Round 1/2，2026-10-01）**

- 镜像核对：板上 `iminfo`/`bootm` kernel crc32 `87ffd369` 与 host `dumpimage -l` 一致（FIT 280,096 B，`fatload` 63 ms 成功），确认为本轮新镜像。
- APMU（stage=4）：`before=0x00000000`——证实 U-Boot handoff 后 GMAC 总线时钟门控（父 Cycle 000 阻塞根因成立）；`step1=0x00000009`（= CLK_EN | RGMII，与纯函数预期一致）读回 `0x00000009`；`step2=0x0000000b`（= `0x9 | RST_DEASSERT`）读回 `0x0000000b`。bit0/bit1 均置位、读回粘滞、两步写序写生效。
- MAC（stage=5）：`version read1/read2 = 0x00001054 class=ok`——与板上 Linux 驱动观测 `User ID: 0x10, Synopsys ID: 0x54`（`evidence/001-board-ram-boundary/000-initial/uboot-readonly.txt:569`）精确一致，非全零/全一，身份可解释。`debug read1/read2 = 0x00000000 class=all-zero`——见 Blocker Handoff。
- 复位观察：本轮未见 `reset`→`k3 login:` 记录（用户贴回范围止于 `stage=5 mac probe done`）。

**Blocker Handoff**

- 发现位置：task 4.1-R1 板上执行，契约 Stop when「MAC 读值全零/全一 → 停 clock/reset 更深层返回 Plan」；Gate 4。
- Plan 预期 vs 实际：GREEN condition 要求 version 非全零/全一且与 `0x1054` 对照可解释、**debug 读值非全零/全一**。实际 version=0x1054 精确命中、debug=0x00000000×2。该 Stop when 的层解释（clock/reset 更深层、总线仍不可达）被同一窗口的 version 精确读值证伪：MAC 寄存器总线已可达且身份正确；GMAC_DEBUG 为硬件活动/状态类寄存器，仓库内无可查证的 idle 期望值来源（参考仓库 `k3_gmac` 七文件无此寄存器，上游头文件不在仓），Act 不裁决其 0 值是合法 idle 还是异常。
- 影响：Acceptance D1 的「状态读值非全零/全一」子项字面未满足，且重复启动下该读值可预期恒为同态，当前契约无法判 GREEN；D1 身份子项、D2 的 APMU 半边（读回置位）、D3 可追溯性已在 Round 1 满足。
- 已完成：host 全部（含全部验证）；板上 Round 1 装载、stage=1–5 全序列、APMU 写生效、version 身份命中；Evidence Round 1 落盘。
- 部分完成：D1 debug 子项（待裁决）；D2（Round 2 与两轮一致性待补）。
- 未开始：Round 2、两轮 `reset`→`k3 login:` 确认——对任何裁决结果均有证据价值，可按用户安排补采；`blocked` 状态下 Act 仅接收记录，不再推进判定。
- 工作区状态：本轮代码改动即上文「实际改动」四文件，Blocker 期间无新增代码改动；Evidence 目录 `evidence/003-mac-register-baseline/001-rework/`（README + board-mac-regs.txt）。
- 恢复条件：用户/Plan 对 debug 读值语义裁决——(a) 认定 version 身份已覆盖 D1「身份与状态可访问、可解释」意图、debug=0 为合法 idle 状态：由 Plan 修订 D1/GREEN wording（rework/replan Cycle 或 Review 解释），补齐 Round 2 + reset 观察后判 GREEN；(b) 认定 debug=0 为异常：返回 Plan 调查 GMAC_DEBUG 位域语义与预期 idle 值（上游/手册来源）后另立执行契约。
- Evidence 引用：`evidence/003-mac-register-baseline/001-rework/board-mac-regs.txt`（Round 1 全文）。

**Blocker Resolution（2026-10-01，用户裁决）**

用户原话：「剩下的reset不用管，这个都是验证可行的，后续的iter也没必要有，debug这个仅仅只是形式报错的化那其实可以接受没必要报block，可以修改为reported然后额外说明相关问题，并在做下一个iter的事情的时候顺便就修复了，这就是我的裁决，你可以进行相关更改回复」

裁决落点：

- GMAC_DEBUG=0：认定为非实质的形式问题，Round 1 读值作为 D1 状态子项证据被接受；身份判定以 `version=0x1054` 与板上 Linux 驱动观测精确对照为准。语义澄清（位域 idle 期望值）不另立 Cycle，随 Iteration 004（PHY 与 link）的计划工作顺带覆盖——004 的 MDIO/PHY 路径依赖对控制器状态寄存器的解释，届时写入该 Iteration 的 Investigation Facts。
- Round 2 与两轮 `reset`→`k3 login:` 观察：用户豁免。理由（原话）「这个都是验证可行的」——复位回原系统路径已在 Iteration 002 两轮板上验证，本轮镜像仅改 stage=4/5 探针代码、启动链与地址面未变。风险记录：D2 的「两轮读值一致」退化为单轮自一致（每寄存器 ≥2 遍读值一致）加既有两轮启动经验外推；APMU/MAC 冷启动差异未被本轮直接覆盖，若 004 上板出现读值漂移，按当时现场重新取证。
- 状态流转：`blocked` → `pending`（用户解决阻塞并指示继续）→ `reported`（本 Response 即最终完整状态；原 Blocker Handoff 保留于上方）。

最终 Acceptance 判定：

- D1：满足——version 身份精确命中；debug 状态子项经用户裁决以形式问题接受，语义澄清移交 Iteration 004。
- D2：APMU 写生效与读值自一致在 Round 1 满足；「两轮一致」与复位观察经用户豁免（原话与风险如上）。
- D3：满足——来源引注成文；除契约 APMU CTRL `0x3ec` 两步写序外零寄存器/持久写入；异常分层报告且停止（Stop when 触发并按规则交接，未现场补写）。

**Experience Candidates**

None——R68 流程未变化；GMAC_DEBUG 语义问题经用户裁决为非实质并移交 Iteration 004 计划，不立 Issue、不立 Runbook。

**未解决问题**

无阻塞项。移交事项：GMAC_DEBUG 位域语义澄清随 Iteration 004 计划顺带覆盖（用户裁决）；Iteration 004 展开时以本 Response 与 Evidence 为 Current Baseline 输入。
