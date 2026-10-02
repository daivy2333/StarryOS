# Iteration 004 / Cycle 001: rework — IO-PD 电压域按参考用法写入

## Plan Context

- Status: ready
- Cycle Type: rework

**Cycle Scope**

- Change tasks: 5.1–5.2（Iteration Map 不变）
- Acceptance gaps: 000 阻塞于 stage6 IO-PD 读前置——A1/A4 板上未执行、A2 的 IO-PD 步骤方法需重建（见 Background）
- Repair items: 5.1-R1（stage6 IO-PD 读前置替换为有来源的解锁+写序；含 APBC 映射与 host 测试重建）
- Inherited scope: 000 的 5.1 其余契约（mux RMW、GPIO1_5 恰一次复位、MDIO 有界事务、身份分类、stage6 gate）与 5.2 全部契约（AN restart RMW、reg4/9 只读诊断、120 s/100 ms 观察、latch-low、两次插拔）；delta spec 五条 requirement；D2–D6（D6 已按本 Cycle 修订）；M42（2026-10-02 修订版）。
- Excluded scope: packet I/O、MAC IRQ/DMA、异步 NIC、SMP、吞吐、持久固件写入、RGMII 调相、PHY vendor page、全局状态维护、Runbook/Issue 落账。

**Objective**

以 R69 可溯源用法完成 stage6 电压域设置，取得真实 PHYID、可解释的 Clause 22 状态与两次物理插拔循环（000 原 Objective 不变）。

**Background**

000 真板第一轮停在 stage6 IO-PD 读前置：读 `0xd401e80c` 得 0，`err=voltage-domain stop`。000 的 Plan Review（2026-10-02，`rework-required`）判定该前置无来源依据：R69 的 io_pd 用法是「APBC ASAR 一次性解锁 → 整字写」，全仓无一处读取；RAM 启动路径上无人预置 1.8V，读检查不可满足。按 M42（2026-10-02 修订：参考用法默认采信、禁止发明参考实现从不执行的验证前置）与 Blocker Handoff 候选项 ②，本 Cycle 把 IO-PD 步骤重建为对齐参考用法的有限写。stage1–5 板上基线已在 000 第一轮逐字复现，无需单独重验（本轮 V3 顺带观察其输出）。

**Investigation Facts**

- Current Baseline: 工作树 = 000 Act Response「实际改动」所列内容（未提交）。host：phy harness 32 项、mac harness 15 项 GREEN；`make k3`/`make build`/`make lichee` exit 0；vf2 维持 002 既有失败签名（50 errors，`unresolved imports axfs/axnet/AxTaskExt`）；`openspec validate` valid。板上：stage1–5 复现 accepted 基线（apmu before=0/after1=9/after2=0xb；version=0x1054×2 class=ok；debug=0×2 sem=idle）、stage6 gate=proceed、`pad/gpio begin` 后停在 iopd 前置。
- Current-State Evidence（000 Review 独立核对，未失效）：
  - `kernel/src/platform/phy_probe.rs::run_pad_gpio`——现存 IO-PD 读前置（读 `IOPD_BASE_PADDR+IOPD_GMAC_OFFSET`、all-one/bit2=0 停止、`iopd read=…` 行）位于 mux/GPIO 之前，是本 Cycle 唯一需重建的产品逻辑；其余（mux RMW+写回校验、GPIO 序列、MDIO 事务、stage7/8）保持。
  - `crates/axplat-riscv64-k3/axconfig.toml` mmio-ranges 现含 UART `0xd4017000/0x1000`、MAC `0xcac82000/0x2000`、APMU `0xd4282800/0x400`、GPIO `0xd4019000/0x100`、pad 页 `0xd401e000/0x1000`；无 APBC，需新增。
  - `kernel/src/platform/k3.rs::K3Env::mmio_write` 的 SAFETY 注释列写寄存器清单，需随本 Cycle 扩展。
  - `tests/phy-probe-host-harness.rs` stage6 相关测试：`run_pad_gpio_happy_minimal_sequence`、`run_pad_gpio_voltage_all_one_stops_before_any_write`、`run_pad_gpio_voltage_bit2_clear_stops_before_any_write`、fake board 助手 `set_iopd_1v8_ok`；`tests/mac-probe-host-harness.rs:264` 有 EOF 空行（`git diff --check`）。
- 参考用法（R69 `tgoskits/drivers/ax-driver/src/soc/k3/pinctrl/`，000 Review 直接读取；M42 默认采信）：
  - `regs.rs:64-75`：`IO_PWR_DOMAIN_V18EN = BIT(2)`（置 1 → 1V8，清 0 → 3V3）；`APBC_ASFAR = +0x00`、`APBC_ASSAR = +0x04`（相对 apbc+asar_offset）；魔钥 `APBC_ASFAR_AKEY = 0xbaba`、`APBC_ASSAR_AKEY = 0xeb10`（追溯 Linux `pinctrl-k1.c:75-81`）。
  - `regs.rs:103-118`：GMAC pins 21–41 → io_pd 偏移 `GPIO2_KX = 0x0c`（相对 0xd401e800）。
  - `mod.rs:164-182` `set_io_pwr_domain`：写 ASFAR 魔钥 → 写 ASSAR 魔钥 → io_pd **整字写**目标值（1V8 即 `BIT(2)`，非 RMW）；ASAR 解锁只允许恰好一次紧随的 io_pd 访问，完成后自动重新上锁；全仓 io_pd 仅此一处写、零读取。
  - 板 DTB（`tools/k3_com260_ifx.dtb`，dtc 反编译核对）：`system-controller@d4015000`（`spacemit,k3-syscon-apbc`，reg `0xd4015000/0x1000`）；pinctrl 节点 `spacemit,apbc = <0x82 0x50>` → asar_offset = 0x50；`gmac1-cfg power-source = <1800>` → is_1v8 = true。
- Code and Critical Path: `phy_probe.rs`（删读前置、新增解锁+写序与输出行、APBC/io_pd 写常量）→ `axconfig.toml`（+APBC 窗口）→ `k3.rs`（SAFETY 注释）→ 两 harness（stage6 测试重建 + EOF 修复）→ `make k3` → 板上 V3。数据流不变：单 hart、polling、无 IRQ/异步；新增写面仅 APBC 两个魔钥字与 io_pd 一个字。

**Implementation Guidance**

依赖顺序：5.1-R1 host RED → 实现/GREEN → V2 构建 → V3 真板。删除读前置时同步收敛 `PadGpioStop::Voltage*` 停止分类（写路径故障由 `pad/gpio begin` 后静默的串口形态定位，与 stage4 APMU 同构）；枚举形态与常量命名由 Act 决定。输出行保持 begin → iopd 写观察 → mux → gpio done 的可定位序列。

**Task Contracts**

### 5.1-R1: IO-PD 电压域按 R69 用法设置，stage6 下游解锁

- Requirement/Scenario: 可观测链路与平台兼容；000 的 S1–S4、S9–S10 中 IO-PD 相关语义修订为「有来源写入后继续」；D4/D6（修订后）/M42。
- Depends on: 000 host 基线（GREEN 未失效，直接继承）。
- Targets: `kernel/src/platform/phy_probe.rs::{run_pad_gpio, IOPD/APBC 常量区}`；`crates/axplat-riscv64-k3/axconfig.toml`；`kernel/src/platform/k3.rs`（仅 `K3Env::mmio_write` SAFETY 注释）；`tests/phy-probe-host-harness.rs`；`tests/mac-probe-host-harness.rs`（仅 EOF 空行修复）。
- Current behavior: stage6 首步读 `0xd401e80c`，bit2=0 或全一即停止（板上实际触发），下游 mux/GPIO/MDIO/link 全部被阻。
- Required behavior:
  1. 删除 IO-PD 读前置及其停止分类与 `iopd read=…` 行；stage6 首步改为对齐 R69 `set_io_pwr_domain` 的写序：写 APBC `0xd4015050` = `0xbaba` → 写 `0xd4015054` = `0xeb10` → 写 IO-PD `0xd401e80c` = `BIT(2)`（1V8，来自 DTB `power-source=1800`）。三写紧邻成序，之间不插入任何其他 MMIO 访问（含串口输出）；io_pd 为整字写，不做读改写，不做写前/写后读验证（参考实现从不读取该寄存器）。写序前后输出 begin 与 iopd 写观察行（含 written 值）供阶段定位。
  2. `axconfig.toml` mmio-ranges 追加 `[0xd401_5000, 0x1000]`（板 DT `system-controller@d4015000` reg；与现有窗口互不重叠，ASAR 偏移 0x50/0x54 在内）。
  3. mux RMW、GPIO1_5 恰一次复位、stage6 gate、stage7/8 契约全部不变（000 原文继续有效，含 5.2 各项与「BMCR 一次、BMSR 两次」的已裁决读法）。
  4. host 测试重建：改写 `run_pad_gpio_happy_minimal_sequence` 等序列测试——断言解锁两魔钥写的次序与地址、io_pd 写值 = `BIT(2)`、三写之间无其他访问、全程无 io_pd 读；删除两条 preflight 停止测试；fake board 移除 iopd 预置可读语义。mac harness 仅修 EOF 空行，15 项语义不变。
- Preserve: stage1–5 输出形状与 APMU/MAC 基线；GPIO 仅 bank1 bit5；pad 仅低 3 位 RMW；DTB 原文件；既有平台构建与非 K3 分流。
- Forbidden: io_pd 任何读访问；ASAR 解锁重复发送、或解锁写与 io_pd 写之间插入其他访问；电压域以外电气位（drive/pull/slew/schmitt/edge/DLINE/WOL）写入；PHY vendor page、地址扫描、软复位追加、DMA/IRQ；参考仓库写入。
- Test witness: 先改 harness 观察 RED（preflight 测试被删/失败、新序列断言缺失），再实现 GREEN：`rustc --edition=2024 --test tests/phy-probe-host-harness.rs -o /tmp/phy-probe-host-test` 后直接运行；mac harness 同法重跑确认 15 项。
- GREEN condition: 新序列断言全绿（次序/地址/写值/无读/紧邻）；mac harness 15 项不回归；`make k3` exit 0 且 FIT kernel/FDT load 地址不变。
- Verification: V1（两 harness）/V2（四构建，vf2 仅签名比对）/V3（板上：stage6 gate=proceed、iopd 写行、16 个 pad 行、gpio reset done、stage7 真实 PHYID、stage8 两次插拔——步骤同 000 V3）。
- Stop when: iopd 写序后无 mux 行输出（`pad/gpio begin`/iopd 行后静默，总线挂死）→ board-control 层停止返回 Plan；mux/GPIO/MDIO/PHY/link 各层停止条件沿用 000；不得换参数试跑或自行改 divider/时序。

**Invariants**

000 的 Invariants 全部继续有效（单变量、有界事务、直接判定、无身份型证据、早期同步 polling 仅用于有界板级探针）；新增：IO-PD/APBC 访问仅限本契约的三写序列，每轮启动至多一次。

**Non-goals**

同 000（rework 不扩大原 Iteration 范围）；不为 io_pd 增加读诊断能力，不引入 pinctrl/GPIO 框架。

**Acceptance / RTM**

A1–A5 与 000 相同；本 Cycle 关闭 A2 的方法缺口并解锁 A1/A4 的板上执行。RTM 映射不变，仅 5.1 的 IO-PD 步骤实现面变化，无 Simplified/Deferred/Missing。

**Verification**

- V1 原生 host 测试：`rustc --edition=2024 --test tests/{phy,mac}-probe-host-harness.rs` 编译并直接运行，exit 0、原生 `test result: ok`（更新后全量）；early-console 6 项未触碰，采信 002/003 结论；不跑含既有失败的全量 host-test。
- V2 受影响构建：`make k3` exit 0（新映射/代码确有变化，需新构建；FIT load/entry `0x180000000/0x181000000` 不变，kernel 增长远小于 16 MiB 间隔则无需重跑 FIT 资格）；`make build`、`make lichee` exit 0；`make vf2` 原生失败签名与 002 既有基线一致只记未退化。
- V3 真板直接观察（操作者执行，R68 路径）：FIT 入 FAT32 U 盘 → 上电/复位长按 `s` 进 U-Boot → `usb start`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。确认：stage1–5 基线行（顺带观察）；stage6 `gate=proceed`、iopd 写行、pin=21–35/37 mux 行、`gpio reset done`；stage7 `phy id1/id2/phyid=0x001cc916 result=rtl8211f` 与 bmcr/bmsr 行；stage8 120 s 窗口——开始不插线（down 可观察），插上对端见 AN 完成且 up，拔/插各两次共两次完整 up→down→up，每次等串口状态后再动作；`observation-ended` 后停驻，普通 `reset` 回原系统。操作者一句话记录实际顺序与结果，决定性片段（≤20 行）入 Act Response。若 120 s 不足两循环：本轮不 PASS，报告耗时供 Plan 调窗口，不循环到通过。

**Gate 2 Readiness**

PASS：用户于 2026-10-02 审计本 Cycle 计划并回复原话「批准」。调查、来源事实、repair item、测试见证、验证与停止条件已闭合；host/构建基线继承 000 未失效结论。无 BLOCKED/WAIVED 项。板上运行（R68 路径 + 两次插拔）是 Act 批次中的能力边界，由现场操作者执行。

**Persisted Evidence**

- Mode: none

理由同 000：串口行为可经 R68 路径低成本复现，Act Response 摘要承载结论；不创建 Evidence 占位目录。

**Risks and Notes**

- bank 粒度：io_pd `0x0c` 覆盖 pins 21–41（GPIO2_KX bank 共享寄存器），整字写把整个 bank 置 1V8——与 R69/Linux 按 bank 写入的行为一致，非本仓扩大；本板 DTB 中该 bank 由 gmac1 使用。
- 解锁语义：ASAR 一次解锁仅允许恰好一次紧随的 io_pd 访问，失败重试需整段重发解锁序列（本契约不规划重试）。
- CSR 实际频率未实测：divider CR=5 沿用 000 契约，不因 MDIO 异常自行调整。
- 原 UART 单字符等待仍可因硬件故障挂住；以最后阶段标记直接诊断，与 000 一致。

## Act Response

- Status: reported

### 实际改动

- `kernel/src/platform/phy_probe.rs`：删除 stage6 IO-PD 读前置（读 `0xd401e80c`、all-one/bit2=0 停止分支、`iopd read=` 输出行、`PadGpioStop::VoltageAllOne/VoltageDomain` 变体）；按 R69 `set_io_pwr_domain` 用法重建为三写序列——APBC `0xd4015050`=0xbaba → `0xd4015054`=0xeb10 → IO-PD `0xd401e80c`=BIT(2)，整字写、三写紧邻（之间无任何 MMIO 访问或串口输出）、无读回，写后输出 `iopd write=0x00000004 ok` 观察行。新增常量 `APBC_BASE_PADDR/APBC_WINDOW_SIZE/APBC_ASAR_OFFSET/APBC_ASFAR_OFFSET/APBC_ASSAR_OFFSET/APBC_ASFAR_AKEY/APBC_ASSAR_AKEY`。mux RMW、GPIO1_5 恰一次复位、stage6 gate、stage7/8 契约代码未动。
- `crates/axplat-riscv64-k3/axconfig.toml`：mmio-ranges 新增 `[0xd401_5000, 0x1000]`（板 DT `system-controller@d4015000`，与既有窗口无重叠，ASAR 0x50/0x54 在内）；pad 页注释「voltage read」→「voltage write」。
- `kernel/src/platform/k3.rs`：仅扩展 `K3Env::mmio_write` SAFETY 注释（APBC ASAR 解锁键与 IO-PD 电压字）。
- `tests/phy-probe-host-harness.rs`：`run_pad_gpio_happy_minimal_sequence` 重写为写序断言（两魔钥写地址+次序、io_pd 写值 BIT(2)、io_pd 写前零读、全程无 IO-PD/APBC 读、三写相邻）；删除 `run_pad_gpio_voltage_all_one_stops_before_any_write`、`run_pad_gpio_voltage_bit2_clear_stops_before_any_write`；fake board 移除 `set_iopd_1v8_ok` 预置语义，新增 `read_addr_log` 与 io_pd 写时读计数捕获（`reads_before_iopd_write`）。
- `tests/mac-probe-host-harness.rs`：仅修 EOF 空行，15 项语义未动；`git diff --check` 清洁。

### 与计划的偏差

无实质偏差。契约内决定三处：`PadGpioStop` 收敛为仅 `MuxWriteback`（Implementation Guidance 授权 Act 收敛 `Voltage*` 停止分类）；iopd 写观察行措辞 `iopd write=0x00000004 ok`（行形态 Act 决定，保持 begin → iopd 写观察 → mux → gpio done 的可定位序列）；axconfig pad 页注释随写语义同步。Persisted Evidence 计划为 `none`，本轮按用户明确要求保存板上日志（白名单第 1 项），见下方 Evidence 引用。

### 自检发现与遗留

- 无 Critical/Important，无遗留 Minor。Gate 3 复核：preflight 符号零残留（`grep -rn "VoltageAllOne|VoltageDomain|iopd read|set_iopd_1v8_ok" kernel/ tests/ src/` 无命中）；本 Cycle 改动面无新增编译警告（make k3 现存警告均为 cargo config 弃用提示、MS08 SMP 死代码与 dtc 警告，不在改动面）。

### 验证

RED（先改 harness）：`rustc --edition=2024 --test tests/phy-probe-host-harness.rs` 9 个编译错误——`APBC_*` 契约面在产品不存在（新序列断言缺失），随后实现转 GREEN。

- V1 phy harness（更新后全量）：`test result: ok. 30 passed; 0 failed`，exit 0（原 32 项 − 删除的 2 条 preflight 测试）。
- V1 mac harness：`test result: ok. 15 passed; 0 failed`，exit 0。early-console 6 项未触碰，采信 002/003 结论。
- V2 `make k3` exit 0；`mkimage -l StarryOS_riscv64-k3.fit`：kernel load/entry `0x180000000`、FDT load `0x181000000`，与已接受基线一致，kernel 147,648 B ≪ 16 MiB 间隔。
- V2 `make build`、`make lichee` exit 0。
- V2 `make vf2` exit 2（预期既有失败）：全部错误为 `axfs`/`axnet`/`axtask` import 解析（E0432 ×26 + E0433 ×23，`kernel/src/file/net.rs` 等），无一本 Cycle 文件——失败签名未退化。错误行计数 49 与 000 基线记录 50 的差异为计数口径（是否含 `error: could not compile` summary 行），非回归。
- V3 真板（操作者执行，R68 路径，2026-10-02，证据 `evidence/004-phy-link/001-rework/board-phy-link.txt`）：stage1–5 逐字复现已接受基线；stage6 `gate=proceed` → `iopd write=0x00000004 ok`（三写后总线未挂，mux 行紧随）→ pin 21–35 `action=write after=0x…441 ok`（mux 位取 1、其余位保持）、pin 37 keep，共 16 pad 行 → `gpio reset done`；stage7 `phyid=0x001cc916 result=rtl8211f` 精确匹配，`link=down an-complete=0`（无对端初始相直接可观察）；stage8 AN restart `0x1040→0x1240` 后，操作者多次插拔产生 6 次 negotiating→up 转换（≥5 个完整 up→down→up 循环，契约要求两次），每次 up 带 `latch-cleared`，`observation-ended last=negotiating up-observed=yes`，窗口未被插拔延长。Stop when 全部未触发；退出 `reset` 回原系统由操作者确认。
- `openspec validate` → valid，exit 0。

### Persisted Evidence

- 计划 `none`；按用户明确要求保存（白名单第 1 项）：`evidence/004-phy-link/001-rework/board-phy-link.txt`（串口原文）、`evidence/004-phy-link/001-rework/README.md`（来源、结论、文件）。

### Repair item 状态

- 5.1-R1 完成：GREEN 条件全部满足（新序列断言全绿、mac 15 项不回归、`make k3` exit 0 且 FIT load 地址不变、V3 stage6–8 通过条件全部直接观察）。任务 5.1–5.2 的板上验收证据已齐备，勾选与注记留待 Plan Review 接受时处理。

### Experience Candidates

None.

## Plan Review

- Review Result: accepted

**Findings**

1. 独立核对通过：Act 声明与代码一致——`phy_probe.rs` stage6 为三写紧邻序列（APBC `0xd4015050`=0xbaba → `0xd4015054`=0xeb10 → IO-PD `0xd401e80c` 整字写 `BIT(2)`），无任何 IO-PD/APBC 读访问，preflight 符号零残留（`PadGpioStop` 仅余 `MuxWriteback`）；`axconfig.toml` 新增 APBC 窗口与板 DT reg 一致；harness 重写后的序列断言（写次序/地址/值、io_pd 写前零读、两窗口全程零读、15 写 + pin37 keep）与契约逐项对应；mac harness 仅 EOF 修复，`git diff --check` 全仓清洁。
2. 板上验收（Evidence `board-phy-link.txt` 逐字核对）：stage1–5 复现已接受基线；stage6 三写后总线未挂、mux 行紧随（000 阻塞的修复在真板成立）；16 个 pad 行 mux 位取 1 且其余位保持（`0xa440/0xc440→0xa441/0xc441`）；GPIO 复位恰一次；stage7 PHYID=`0x001cc916` 精确匹配 RTL8211F，无对端初相 `link=down an-complete=0` 直接可观察；stage8 6 次 up（≥5 个完整 up→down→up，契约要求两次），每次 up 带 `latch-cleared`（`0x79a9→0x79ad` 双读语义按设计工作），窗口固定未延长，`observation-ended` 后 `reset` 回原系统。Stop when 全部未触发。
3. 非阻塞记录：Evidence 模式由计划 `none` 改为用户明确要求保存（白名单第 1 项），README 注明来源与覆盖理由，体量克制（61 行串口 + 10 行 README）；vf2 错误计数 49 vs 000 记录 50 为计数口径差异（summary 行），失败签名未变化且无一本 Cycle 文件；phy harness 32→30 项源于删除 2 条 preflight 测试，属契约预期。
4. 采信：V1（phy 30 项、mac 15 项 GREEN）、V2 四构建与 FIT 地址、`openspec validate`——覆盖未失效，独立核对代码无出入后采信。

**Acceptance Gaps**

None。A1 真板实读 PHYID 且可解释；A2 IO-PD 步骤按 R69 用法有来源、有限写入并真板生效；A3 无对端/错误分层有 host 测试与板上初相观察；A4 ≥5 个完整 up→down→up 循环；A5 既有平台构建保持。

**Evidence**

- `evidence/004-phy-link/001-rework/board-phy-link.txt` + `README.md`（本轮 Review 逐字读取核对）。
- 代码/diff 独立核对：`phy_probe.rs` stage6 全文、`axconfig.toml` diff、`tests/phy-probe-host-harness.rs` 序列测试、`git diff --check`。
- host/构建基线：Act Response Verification（采信，覆盖未失效）。

**Follow-up Decision**

Acceptance 全部满足且无阻塞 finding → `accepted`。Iteration 004 完成，为 Map 最后一个 Iteration；change 11/11 tasks 闭合，可由用户调用 `openspec-docs-maintainer` 收尾（合并行为规格增量并归档）。Experience Candidates：None。

**Next Cycle**

None

**Next Iteration**

None（Map 最后一个 Iteration；Iteration 004 accepted，change 全部任务完成）
