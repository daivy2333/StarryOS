# Iteration 001 / Cycle 000: PHY 真实设备 IRQ/status 基线

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 2.1–2.3。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: R2 真实 claim/status 对齐与 MAC 路由、R3 破坏性 cause 保留、R4 设备侧再触发/quiet/持续电平、R5 原生验证/兼容/raw Evidence/reset；Iteration 000 accepted 的 AIA 底座（原子 claim+EOI 先于 handler、固定路由 1/60/133/277、stage1–9、RAM boot 路径）及其全部 Preserve 边界。
- Excluded scope: DMA/descriptor/网络收发、异步 queue task、多 hart、timer/UART 交互 IRQ、性能优化、pinctrl 电气之外的板级变更、MS17/MS18 方向。

**Objective**

把 D5 固定契约（paged MDIO、完整 raw edge 快照、先 cause 后 ack 的固定顺序）接到真板 PHY 链：source60 与 RTL8211F INSR 对齐、破坏性 cause 在 ack 前留存可审计、真实插拔产生对齐 claim 并可再触发可 quiet、MAC 133/277 只读观察，全程 R68 RAM 启动环内可回滚。

**Background**

- 2026-10-03 用户裁决（原话记录于 change tasks.md RTM 行与 proposal）：「拔插计数是形式主义，核心问题是证明拔插有效就行……」——撤销人工插拔计数相等与固定循环门槛；保留真实 link 变化、claim/status 对齐、再触发、快照与 quiet。资料策略为本板 DTB/R69 与相关同位字段/清边沿实现，不等待 vendor 文件。
- D5 固定设备契约（page selector 1f 保存/恢复、INER a42:12 bit4、INSR a43:1d 一次 COR、edge 四字读取、MFPR35 仅 RMW bits4..6）已随 2026-10-03 整体修订计划获用户批准（原话「批准」）。
- Iteration 000 板上事实（第 5 次板测，见 `000-initial/001-replan.md` 第五轮 Blocker Resolution 与 Evidence README）：本硅片 APLIC msiaddrcfg 写后读不回但写生效；EIE 掩码窗口的 MSI 被丢弃而非保留（`eie_retained=0`）——设备轮 ack/再触发设计不得依赖掩码窗口保留 pending，须「触发后置掩码」；handler 执行期间的新 MSI 正常保留（`retrap=2`）。

**Investigation Facts**

Current Baseline（Iteration 000 accepted，2026-10-03 Plan Review）：

- 分支 k3、HEAD `082339ba` + 未提交 MS10 实现（文件清单见 `000-initial/001-replan.md` Act Response 实际改动表；另含不属本 change 的 `.agents/` 规划产物）。
- 真板 `stage=9 pass`：双入口×3、双 mask blocked+fresh、retrap=2、unknown=1、storm=32 止损、各 quiet 全过；reset 已确认。A1–A6 accepted。
- host/构建：`k3-aia-host-harness` 16 passed、`k3-aia-probe-host-harness` 10 passed（双跑）、PHY 30 / MAC 15 回归、`make k3/build/lichee` exit0（采信同一 Review）。
- 遗留 Minor（不阻塞）：`aia.rs` 有 `AplicWindowProbe` 死结构与陈旧注释、`aia_probe.rs` 模块头一行陈旧——本轮触碰这些文件时可顺手清除，不作为任务。

Current-State Evidence / Code and Critical Path：

| 位置 / 符号 | 当前行为及本轮影响 |
|---|---|
| `kernel/src/platform/phy_probe.rs::{mdio_read,mdio_write}` | 现有只读标准 C22（`MDIO_ADDR 0x200`/`MDIO_DATA 0x204`、busy bit0、`MDIO_TIMEOUT_NANOS` 10 ms 有界等待、`MdioError` 带 phase/reg/raw 分类）；本轮新增 paged 读写与 vendor 寄存器操作，复用同一 `ProbeEnv` |
| `kernel/src/platform/phy_probe.rs::ProbeEnv` / `K3Env` | u32 volatile MMIO、native `now_nanos`/`wait_nanos` seam；host harness 沿此约定直接包含生产模块 |
| `kernel/src/platform/k3.rs::run_pad_gpio` | MFPR RMW 既有形状（pad 页 `0xd401e000/0x1000` 单页映射、pin21–37 mux 写）；MFPR35 的 CLEAR/读回/清 CLEAR 沿同一 RMW 形状 |
| `crates/axplat-riscv64-k3/axconfig.toml` | GPIO 窗口现 `[0xd401_9000, 0x100]`；按 D5 扩为 `0x1000` 覆盖 DT gpio-edge 子窗口，不新增重叠映射 |
| `crates/axplat-riscv64-k3/src/{aia,irq}.rs` | Iteration 000 交付：固定路由含 source60/133/277 LevelHigh、`AiaChip` claim/dispatch、`irq::probe` 观察接口；本轮只注册 source60 handler 与只读观察，不改控制器语义 |
| `kernel/src/platform/aia_probe.rs` | stage9 探针与 `Stage9Env`/32 预算/前台输出组织方式；stage10 设备窗口沿用同一形状（前台 polling 输出，handler 只记录） |
| `kernel/src/platform/k3.rs::run_first_byte_park` | stage1–9 顺序不变；stage10 追加在 stage9 之后、park 之前 |
| `tests/*-host-harness.rs`、`Makefile::host-test` | `rustc --edition=2024 --test` 直接包含生产模块的既有约定与条目；新 harness `tests/k3-device-irq-host-harness.rs` 沿用 |

寄存器与设备契约（D5；来源：本板 DTB、R69 intc/somehal 同位字段与清边沿实现）：

| 对象 | 值 / 操作 |
|---|---|
| PHY | MDIO 地址 1，RTL8211F（PHYID `0x001cc916`，MS09 已验收） |
| page 选择 | 写 reg `0x1f` = page 号；paged 访问前后保存/恢复原 page |
| INER | page `a42`，reg `0x12`，bit4 = link change 中断使能 |
| INSR | page `a43`，reg `0x1d`，一次 COR（clear-on-read）；任何用途前必须先快照 |
| edge 四字 | GPIO edge 状态寄存器四个字，只读快照全字保留（未确定的 pin-bank 位不解码，只留 raw） |
| MFPR35 | `0xd401e08c`，仅 RMW bits4..6：先保存 cause、读 PHY、置 CLEAR/读回/清 CLEAR 重备；mux/电气位不动 |
| MAC 观察 | source133/277 已在固定路由表；只读展示 APLIC pending/enable/target、GMAC `0x1160` 只读、现有 APMU wake-enable bit12 |

**Implementation Guidance**

先在 `phy_probe.rs` 建立 paged MDIO 与快照/ack 生产函数（host 可测），再建 `k3_device_irq_probe.rs` 设备窗口接 stage10，最后上板。MDIO 所有权单一：设备窗口前台持有时 handler 只记录不触碰 MDIO；handler 的 cause 读取以快照留存为界。沿用 stage9 的有界等待与预算形状。

**Task Contracts**

### 2.1: 可测试的设备 cause 与 ack 顺序

- Requirement/Scenario: R2 真实对齐，R3 COR 前快照/组合位，R4 再触发，R5 错误停止。
- Depends on: Iteration 000 accepted（已满足）。
- Targets: `kernel/src/platform/phy_probe.rs::{mdio_read,mdio_write}` 及新增 paged 操作，`kernel/src/platform/k3_device_irq_probe.rs`（新），K3 `axconfig.toml`，`tests/k3-device-irq-host-harness.rs`。
- Current behavior: 现有只读标准 C22，无 vendor 页/edge ack。
- Required behavior: 新增 page 保存/恢复（selector `0x1f`）、INER a42:12 bit4 使能、INSR a43:1d 一次 COR、完整 edge 四字读取；MFPR35 `d401e08c` 仅 RMW bits4..6，按 D5 先保存 cause、读 PHY、置 CLEAR/读回/清 CLEAR 重备。GPIO 页 MMIO 描述 `0x100`→`0x1000`，覆盖 DT gpio-edge 子窗口；不新增重叠映射。
- Preserve: 保留 mux/电气位、IO-PD、旧 stage6 一次 reset、MDIO 10 ms 等待。
- Forbidden: 不猜 bitmap、不写 edge syscon、不复制 K1 pin 地址、不新增 PHY reset、无前台/handler 并行 MDIO。
- Test witness: host 直接调用生产模块，先观察 RED 再 GREEN：`paged_access_restores_original_page`、`insr_snapshot_survives_clear_on_read`、`all_raw_cause_bits_are_retained`、`phy_cause_precedes_pad_clear_and_rearm`、`pad_ack_preserves_mux_and_electrical_bits`、`mdio_error_reports_primary_and_restore_failure`、`edge_window_is_read_only`。测试证明固定软件顺序，K3 效果留给 2.3。
- GREEN condition: 同一测试调用生产逻辑全部通过。
- Verification: `rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 后 `/tmp/k3-device-irq-host-test`，原生 0 failed、exit0；`make k3` exit0。
- Stop when: 实际地址/访问语义否定 D5、MDIO 超时或恢复失败；返回 Plan，不改试其它 offset/清法。

### 2.2: 设备窗口与有界 handler

- Requirement/Scenario: R2 设备/MAC 路由，R3 快照，R4 quiet/持续电平，R5 恢复。
- Depends on: 2.1。
- Targets: `kernel/src/platform/{k3.rs,mod.rs,k3_device_irq_probe.rs}`、K3 IRQ 观察接口、同 host harness 与 Makefile 原生条目。
- Current behavior: stage9 后 park，无设备 IRQ 窗口。
- Required behavior: stage9 后追加 120 秒 stage10；唯一 source60 handler 读取 edge 四字、MFPR、INSR 与双读 BMSR 当前 link，先快照后 ack；前台输出 source/raw status/link 与结果。最多 32 条固定快照，容量满或持续 claim 时 mask60/EIE60 保存失败，禁止覆盖尚未输出 cause。source133/277 注册并显示 APLIC pending/enable/target、只读 GMAC `0x1160` 和现有 APMU wake-enable bit12；若出现非预期 MAC 事件记录 raw 后 mask，不伪装 DMA 验收。
- Preserve: 控制器前轮契约和原 stage1–9。
- Forbidden: 不在 IRQ 内 console、不在设备窗口前台并行 MDIO、不新增 DMA/WoL 写入、无人工计数协议。
- Test witness: 生产 probe host RED→GREEN 覆盖 `link_change_matches_source60`、`wrong_source_or_missing_insr_link_bit_fails`、`cause_snapshot_is_not_overwritten`、`quiet_window_has_no_new_claim`、`persistent_level_masks_and_reports`、`device_deadline_stops_without_success`、`mac_routes_are_observed_without_software_trigger`。
- GREEN condition: 原生 harness 全部通过；probe 不打印成功后继续失败步骤；quiet/timeout 用既有 native clock seam 直接判定。
- Verification: 同 harness 原生命令，`make k3`；源60+INSR bit4 是对齐判据，raw edge 全字保留但不要求未确定的 pin-bank 位解码。
- Stop when: 需要改控制器 ownership、DMA 或另一设备路径；持续 assert 或存储满是本契约已定义的运行失败，不继续下游。

### 2.3: 证明真实插拔有效

- Requirement/Scenario: R2–R5。
- Depends on: 2.2。
- Targets: 本轮 Act Response；CoM260 Kit 串口；required `evidence/001-phy-device/000-initial/board-phy-irq.txt` 和 README。
- Current behavior: 无设备 IRQ 运行结果。
- Required behavior: 原生目标 harness 及受影响 PHY/MAC 回归、`make k3` 成功后，按 R68 RAM boot 进入 stage10；操作者拔线观察 source60/INSR bit4/link down，再插回等待 link up 与新的对齐 claim；ack 后的后续投递成立、稳定 1 秒 quiet，reset 回原系统。不设固定循环次数，不要求动作数=claim 数，不另设快速插拔数量验收。
- Preserve: RAM-only、不 saveenv、pure serial required Evidence。
- Forbidden: 不得软件伪造设备事件、输出解析器、身份字段、把 host 结果冒充真板。
- Test witness: 2.1/2.2 实际 RED→GREEN；板上必须实际出现拔线/插回的 link 变化及对应 claim/raw 快照，并在稳定后 quiet，reset 执行人一句话确认。组合位保存 host 证明，raw 设备值如实留档。
- GREEN condition: 上述板上判据全部直接可见且自动目标测试 exit0。
- Verification: 新 harness 与受影响 `tests/phy-probe-host-harness.rs`、`tests/mac-probe-host-harness.rs` 的既有 rustc 原生命令；涉及平台映射时 `make build`、`make lichee`。采信未变范围已有 PASS，不重复全量。板上步骤沿 R68 同地址命令，直接串口判定，deadline 120 秒。
- Stop when: 操作者无结果、raw Evidence 缺失、source/status 错配、无真实 link 变化、quiet 失败或 reset 未恢复；报告实际失败而非缺资料，原控制器结论保留。

**Invariants**

单 hart；RAM-only 不 saveenv；破坏性 cause 先快照后 ack；handler 内不 console、不与前台并行 MDIO；控制器契约与 stage1–9 不变；无人工计数协议、无输出判定器、无身份字段；持续电平/容量满按契约报告并停止，不当缺陷盲试。

**Non-goals**

DMA/descriptor/network、交互 IRQ（timer/UART）、多 hart、性能；不展开 MS17/MS18；不清理与本轮无关的范围外遗留（Iteration 000 的 Minor 注释残留仅在触碰对应文件时顺手处理）。

**Acceptance**

| Acceptance | Requirement / Tasks | 直接判据 |
|---|---|---|
| D1 paged 访问与 cause/ack 顺序 host GREEN | R2/R3；2.1 | 契约 7 测试 RED→GREEN：page 恢复、INSR 快照存活 COR、四字全保留、PHY cause 先于 pad clear/重备、mux/电气位保持、错误报告主操作与恢复失败 |
| D2 真实插拔对齐 | R2/R3；2.2/2.3 | 拔线→source60 claim + INSR bit4 + link down；插回→link up + 新对齐 claim |
| D3 再触发与 quiet | R4；2.2/2.3 | ack 后后续投递成立；稳定 1 秒 quiet 零新 claim |
| D4 持续电平/容量满有界止损 | R4/R5；2.2 | 32 快照上限与持续 claim 的 mask60/EIE60 保存失败，不覆盖未输出 cause |
| D5 MAC 只读观察 | R2；2.2 | source133/277 pending/enable/target、GMAC 0x1160、APMU bit12 只读展示，不伪装 DMA |
| D6 回归、raw Evidence、reset | R5；2.3 | 新旧 harness 0 failed、`make k3/build/lichee` exit0、`board-phy-irq.txt`+README、reset 一句话 |

RTM（本轮链路）：

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R2 | 2.1–2.3 | paged/route host tests；D2/D5 板测 | Covered |
| R3 | 2.1、2.3 | `insr_snapshot_survives_clear_on_read` 等；board 原始快照 | Covered |
| R4 设备部分 | 2.2、2.3 | 再触发/quiet/止损 host；D3/D4 板测 | Covered（Simplified 已批准：不比较人工计数） |
| R5 | 2.3 | 原生 harness、make、MS09/MS10 回归、raw serial、reset | Covered |

**Verification**

V1：`rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 后 `/tmp/k3-device-irq-host-test`（0 failed、exit0；RED 先行如实记录）。

V2：受影响回归 `tests/phy-probe-host-harness.rs`、`tests/mac-probe-host-harness.rs` 原生命令；2.1/2.2 触碰 aia 表面时补 `k3-aia`/`k3-aia-probe` 两 harness。完全未变表面采信既有 PASS 并注明来源。

V3：`make k3`；涉及平台映射时 `make build`、`make lichee` 各一次，exit0。`make vf2` 既有失败只引用不修复。

板上操作（操作者，非 agent 可执行批次）：按 R68 同地址装载（`fatload usb 0:0 0x2e0000000 … && bootm`，不 saveenv）→ stage1–9 全过 → 进入 stage10 后拔线（观察 source60 claim + INSR bit4 + link down）→ 插回（等待 link up 与新对齐 claim）→ 稳定 1 秒 quiet → reset 回 `k3 login:`，执行人一句话确认。每步直接看串口即判定；deadline 120 秒；无固定插拔次数要求。

**Gate 2 Readiness**

PASS。2026-10-03 用户审计修订计划后回复原话「批准」，覆盖两轮 Iteration Map 与 D5 固定后轮契约（记录于 change tasks.md Planning Progress 与 proposal）；本 Cycle 只物化已批准契约并叠加 Iteration 000 accepted 新基线（板上事实强化 ack 顺序设计），无新增未知项、无 Missing、R4 Simplified 已批准。

**Persisted Evidence**

- Mode: required
- Source: proposal 中用户要求设备轮板上 raw 串口留档；与 Iteration 000 同一理由——实体板 + 操作者插拔会话无法由 agent 低成本重跑，摘要会丢失 source/INSR/快照/ack 先后与 quiet 的决定性结构。
- Files: `evidence/001-phy-device/000-initial/board-phy-irq.txt` 和同目录 README；Act 创建，Plan 不创建。
- Acceptance / Gate: 支持 D2–D6 的板上判据，供 Gate 4 与 Plan 独立 Review 直接检查。
- Why Act Response is insufficient: 摘要丢失逐事件 claim/raw 快照与 ack 先后关系。
- Why not cheaply reproducible: 需要操作者实体板与物理插拔。
- Missing blocks: 无原始片段不得接受设备轮板测或宣布 change 完成。
- Pass condition: 纯串口片段直接呈现 stage10 拔/插的真实 claim、INSR bit4、raw 快照、link 变化、quiet 与最终结果；README 只列文件、Acceptance 映射与 reset 一句话；无身份字段、Hash、manifest 或判定脚本。

**Risks and Notes**

- 持续 assert 或快照存储满是本契约已定义的运行失败（报告并停止），不是缺陷也不是重试理由。
- EIE 掩码窗口 MSI 可能丢失（板上事实）：ack/再触发设计不得依赖掩码窗口保留 pending，须触发后置掩码。
- MDIO 超时或 page 恢复失败按 2.1 Stop when 返回 Plan，不改试其它 offset/清法。
- 内部命名与辅助拆分为非实质选择；fake 建模 COR/edge 语义须与生产顺序测试对齐，不得生成伪板上 PASS。

## Act Response

- Status: reported（2026-10-03，Iteration 001 Cycle 000-initial 全部 task 完成）

**实际改动**

| 文件 | 内容 |
|---|---|
| `kernel/src/platform/phy_probe.rs` | 2.1 设备 cause/ack 原语：paged MDIO（`mdio_page`/`with_page`/`paged_read`/`paged_write`，页保存/恢复含失败路径，`PagedError::{SaveFailed,Primary,RestoreOnly}`）、`iner_enable_link_change`（RMW+写后读回，`InerEnableEvidence.stuck()`）、INER/INSR 页与位常量（a42:12 bit4 / a43:1d COR）、edge 四字只读快照（`GPIO_EDGE_BASE_PADDR=0xd4019800`，4 词）、`pad35_edge_setup`（GPIO mux+FALL 置位+CLEAR hold 释放+读回）与既有 ack 对 `pad_edge_ack_set/rearm`；stage8 窗口常量 120 s→30 s（用户指示） |
| `kernel/src/platform/k3_device_irq_probe.rs`（新） | 2.1 `service_one_device_event` 固定 D5 顺序（edge→MFPR→INSR COR→BMSR×2→ack）；2.2 stage10：`Stage10Env` seam、write-once 快照环（cap 32，满则 handler 内 mask60/EIE60 止损且不覆盖未输出 cause）、`source60_handler`/`mac133_handler`/`mac277_handler`、30 s 窗口+1 s quiet、SetupFailed 分类（iner-stuck/insr-drain/pad35-setup/register）、window-close raw dump、事实性结果行（kernel 不判 PASS）；board 环境 `K3Stage10Env`（raw MMIO/axhal 时钟）、IRQ 侧 fn 表（`HandlerAccess` 静态武装）、dispatch 路径 emergency mask、入口重开 `sie.SEXT` |
| `kernel/src/platform/mod.rs` | 挂载 `k3_device_irq_probe`（`cfg(feature="k3")`） |
| `kernel/src/platform/k3.rs` | stage9 `Passed` 后调用 `run_k3_stage10`；stage8 注释同步 30 s |
| `crates/axplat-riscv64-k3/axconfig.toml` | GPIO 窗口 `0xd4019000/0x100`→`0x1000`（覆盖 `syscon-gpio-edge 0xd4019800/0x10`，不新增重叠映射） |
| `tests/k3-device-irq-host-harness.rs`（新） | 17 witness：2.1 七条 + 2.2 七条 + pad35 武装 + INSR 排空 + window-close dump；fake 板含页感知 MDIO/COR INSR/pad CLEAR 锁存/边缘武装门控与 W1C 位图语义 |
| `Makefile` | `host-test` 追加 device harness 条目 |
| `tests/phy-probe-host-harness.rs` | 3 处 120 s 硬编码样本数断言改 `LINK_WINDOW_NANOS` 常量推导 |
| change `tasks.md` | 2.1/2.2/2.3 状态与结果注记 |

**与计划的偏差**（均有板测/DTB 依据；无实质范围外改动）

1. **stage10 入口重开 sie.SEXT**：板测前自检发现 stage9 风暴止损关闭 SEXT 且路由注册不重开，不修正则设备窗口收不到任何 trap。
2. **pin35 武装 setup（计划假设被板测证伪）**：计划假设边沿检测交接已武装、MFPR35 仅做 ack RMW。三轮板测证伪：交接/stage6 后 pin35=0xa441（mux=1 GMAC、RISE/FALL=0、CLEAR=1 为全 pad 复位默认）。DTB `ethernet-phy@1 interrupts=<35 2>`（pin35 下降沿 GPIO 中断）证实选点，交接态 mux=0 证实 GPIO 功能。最终 setup：mux→0、FALL(bit5) 置位、CLEAR(bit6) hold 释放、其余位保留+读回校验——全部落在已批准的 MFPR bits4..6 RMW 与 DTB 事实内，D5 ack 顺序不变。
3. **INSR 排空读**：RTL8211F INTB 电平保持，窗口前 COR 排空（板上 `insr-drain=0x0020`），否则武装下降沿抓不到首个真实沿。
4. **INER 写后读回**（`err=iner-stuck` stop；板上 `written=rb=0x0030` 证实生效）。
5. **window-close raw dump**（edge 四字+MFPR35+setip60，只读）：第三轮板测的判别诊断，保留为常规收尾输出。
6. **窗口 120 s→30 s ×2**（stage10 与 stage8，用户指示，原话：「稍微修改一下窗口上限，设置为30秒就行」「stage 8的时间也缩减到30秒，记得」）。MS09 规格无 120 s 数值硬编码（语义为固定窗口+结束报告），不受影响。
7. AIA 表面（`aia.rs`/`aia_probe.rs`/`irq.rs`）本轮未触碰；`k3-aia`/`k3-aia-probe` harness 沿用 Iteration 000 accepted 结论（2026-10-03 Plan Review 采信）。

**板上迭代记录**（同板三轮，R68 RAM boot 环，操作者执行）

- 第 1 轮（120 s，无 setup）：stage1–9 pass，`result=no-device-event`。根因：MFPR35 边沿未武装且 mux=GMAC，pinctrl 中断路径断。
- 第 2 轮（30 s，mux+FALL 武装、CLEAR=1 保留）：仍零 claim，`pad35 setup ok=1` 证明写生效。得出 bit6 hold-in-clear 假设。
- 第 3 轮（CLEAR hold 释放 + INER 读回 + window-close dump）：D2/D3/D5 判据成立。三轮为同一验收点的假设修正链，每轮携带上一轮证伪的修正与判别诊断，非同类盲试（未触发三次失败反思的停止条件：第 3 轮按修正假设成功）。

**自检发现与修复**

- fake APLIC 位图未建模 W1C/WTS（CLRIE 写未清 SETIE）导致 persistent 测试假失败——修 fake，非生产问题。
- `window_close` 测试断言遗漏 `{:#010x}` 的 `0x` 前缀——修测试。
- 串口 Evidence 首次转写混入第 1 轮 U-Boot 头部时间戳——发现后按第 3 轮原样重写（逐字保真）。
- Gate 3 全量 diff 复核：变更面与契约一致，无计划外文件；Forbidden 逐条核对（不写 edge syscon、不猜 bitmap、无人工计数、无前台-IRQ 并行 MDIO、无 DMA/WoL 写入）未违反。
- 遗留 Minor：`aia.rs::AplicWindowProbe` 死结构与 `aia_probe.rs` 模块头一行陈旧注释（Iteration 000 遗留；本轮未触碰该表面，按 Cycle 约定保留）。
- 遗留观察（非缺陷，不计入证据）：每物理事件一条 INSR=0 的回声 claim（INTB 释放沿在 pad 的附加锁存），分类器正确排除，每事件恰 2 条且无风暴；如需降噪属后续 Plan 层评估。

**验证**（原生命令，全部 exit0）

- `rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 后运行：`17 passed; 0 failed`。RED 先行：2.1 首编译失败（生产模块缺失）、2.2 追加编译失败（Stage10 API 缺失）、CLEAR-release 修正后一次 fake 语义假失败，均如实记录。
- `tests/phy-probe-host-harness.rs`：`30 passed`（stage8 常量化后重跑）；`tests/mac-probe-host-harness.rs`：`15 passed`。
- `make k3` exit0（FIT 最终 18:51）、`make build` exit0、`make lichee` exit0；`make vf2` 未触碰（pre-existing 失败，按计划只引用）。
- 板上第 3 轮：`stage=9 pass` → setup 行（`iner written=0x0030 rb=0x0030`、`insr-drain=0x0020`、`pad35 0xa441→0xa420 ok=1`）→ 30 s 窗口 4 条 claim（对齐 ×2、回声 ×2）→ `window-close edge 全零 setip60=0` → `result=link-down-and-up-observed claims=4 down=1 up=1 unaligned=2 mdio-err=0 quiet=1 quiet-claims=0` → reset 回原系统（操作者确认）。

**Persisted Evidence**

- `evidence/001-phy-device/000-initial/board-phy-irq.txt`（第 3 轮完整 raw 串口）+ 同目录 `README.md`（来源/结论/文件/Acceptance 映射/reset 确认）。支持 D2/D3/D5/D6 板上判据。

**Experience Candidates**

- Runbook 候选：R68（`com260-uboot-usb-starryos-ram-boot.md`）扩展 stage10 观察节——setup 行判读（`pad35 setup ok=1`、`iner written=rb`）、claim 对齐判据（source60+INSR bit4+link）、回声 claim 形态（每事件恰一条 aligned=0）、30 s 窗口与 reset。已端到端验证成功且可重复（三轮），引用本 Response 与 Evidence；是否落账由用户指令 Recorder。
- Issue：None（两轮零 claim 属 change 范围内的假设证伪与修正，非范围外实质缺陷）。

**未解决问题**

- None blocking。`make host-test` 中 ms04 pre-existing 失败与本轮无关，未触碰。

## Plan Review

- Review Result: accepted
- Reviewed: 2026-10-03。独立阅读实际代码（`phy_probe.rs`、`k3_device_irq_probe.rs`、`k3.rs`、`mod.rs`、`axconfig.toml`、`Makefile`、两处 harness）、完整工作树 diff 与 required Evidence；Act 报告的 host/构建/板测结论按覆盖未变化采信（来源：本 Cycle Act Response 验证节）。

**Findings**

全部为非阻塞 Minor；无阻塞 Acceptance 项，无计划外产品改动（其余 diff 表面均属 Iteration 000 accepted 范围或 Current Baseline 已记录的 `.agents/` 规划产物；`Cargo.lock` +1 行为 kernel 可选依赖的必要后果）。Forbidden 逐条核对通过：不写 edge syscon、不猜 bitmap（edge 全字保留不解码）、无人工计数协议、窗口内前台无 MDIO（handler 为唯一 INSR 消费者）、无 DMA/WoL 写入、handler 不 console、证据纯串口无身份字段/解析器。

1. [Minor｜Act 偏离] 陈旧 "120 s" 文案三处未随 30 s 用户指示同步：`phy_probe.rs::run_link_observation` begin 行硬编码 `window-ms=120000`（板上日志实际打印 120000 而窗口为 30 s）、`k3.rs` stage10 注释、`k3_device_irq_probe.rs` 模块 doc。判定依据是行为与 result 行，不影响本 Acceptance；后续触碰对应文件时宜改为常量推导。
2. [Minor｜Act 偏离] Act Response 写 phy-probe harness 断言常量化 "3 处"，diff 实际 6 处；描述性偏差，无行为影响。
3. [Minor｜新证据（板测证伪），已完整记录] design.md D5.1 的武装假设（不改 mux、先置 CLEAR=1 且 RISE/FALL=0）被三轮板测证伪；Act 偏差 2 以 DTB `ethernet-phy@1 interrupts=<35 2>` 与交接态事实替代为单写武装（mux→GPIO、FALL 置位、CLEAR hold 释放+读回校验，电气位全保留），最终武装态与 D5.3 一致（FALL=1/RISE=0/CLEAR=0），claim 顺序与 D5.4 逐字一致（edge→MFPR→INSR COR→BMSR×2→set/读回/rearm）。偏差链记录于 Act Response 偏差 2、change tasks 2.1 注记与 Evidence README；design.md 保留批准时原文，收尾合并行为规格时由 docs-maintainer 知悉该差异（delta spec 无 mux 细节，无冲突）。
4. [Minor｜Plan 错误] design D5.5 输出清单列有逐 claim "ack后edge四字"；实现为逐 claim ack 前快照 + MFPR set/rearm readback，ack 后 edge 四字仅在 window-close 输出一次。Cycle 契约 2.2 未要求逐 claim post-ack edge，D2/D3 判定结构完整。
5. [Minor｜遗留，Iteration 000 Review 已记录] `aia.rs::AplicWindowProbe` 死结构与 `aia_probe.rs` 模块头一行陈旧注释；本轮未触碰该表面，按 Cycle 约定保留。

**Acceptance Gaps**

None。D1：host 17/17 witness 名称与契约逐一对应，测试驱动生产顺序（op-log 顺序断言、RMW 位级断言、错误分类断言），fake 建模页感知 MDIO/COR/锁存与生产语义一致。D2：拔线 claim idx=0（source=60 insr=0x0010 link=down aligned=1）与插回 idx=2（insr=0x0010 link=up aligned=1）板上直接可见。D3：ack 后 idx=2 再投递成立、quiet=1 quiet-claims=0。D4：cap 32 write-once 环与 overflow/持续 claim 的 mask60/EIE60 止损由 host witness 与代码核实（不覆盖未输出 cause）。D5：route 60/133/277 pending/enable/target、gmac1160、apmu-wake-bit12 只读展示，无软件触发、不伪装 DMA。D6：回归与构建采信 Act（覆盖未变化）、`board-phy-irq.txt`+README 齐备、reset 操作者一句话确认（README）。

**Evidence**

- `evidence/001-phy-device/000-initial/board-phy-irq.txt`（140 行，U-Boot usb start 至 result 行）：stage1–9 完整（`stage=9 pass` 含 storm=32、quiet）、stage10 setup 行（`iner written=rb=0x0030`、`insr-drain=0x0020`、`pad35 0xa441→0xa420 ok=1`）、4 条 claim（raw edge word1=0x8 全字保留；每物理事件恰一条 INSR=0 回声 aligned=0，分类器正确排除）、window-close edge 全零 setip60=0、result 行。纯串口，无身份字段。
- 代码核实：`service_one_device_event` D5 顺序；`source60_handler` cap/止损；`run_stage10` 窗口/quiet/window-close/事实性 result；`K3Stage10Env::handler_access` 静态武装；`run_k3_stage10` 入口重开 sie.SEXT；GPIO 窗口 `0xd4019000/0x1000` 覆盖 edge 子窗口且无重叠映射；`Makefile host-test` 追加 device harness 条目。
- 采信（本 Cycle Act Response，覆盖未变化、与代码/diff 无矛盾）：device harness 17 passed、phy 30 passed、mac 15 passed、`make k3`/`make build`/`make lichee` exit0；`make host-test` ms04 pre-existing 失败与本轮无关；`make vf2` pre-existing 只引用。

**Follow-up Decision**

D1–D6 全部满足；7 项偏差均有板测/DTB/用户指示依据且记录完整；全部 finding 非阻塞，无需当前 Cycle 修复——接受。Iteration Map 无后继 Iteration。

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

None（Iteration 000、001 全部 accepted；change 全部 task 闭合，达到收尾条件；收尾与全局同步由 `openspec-docs-maintainer` 按用户指令执行）
