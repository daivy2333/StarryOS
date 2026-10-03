# MS10 change 任务与迭代

Status: approved，Gate 2 PASS。Iteration 000 Cycle `001-replan` Review Result = **accepted**（2026-10-03，A1–A6 全过，两项非阻塞 Minor 记录于 Cycle）。Iteration 001 Cycle `000-initial`：Act Response `reported`（2026-10-03，tasks 2.1–2.3 完成，含三轮板测假设修正），Plan Review = **accepted**（2026-10-03，D1–D6 全过，5 项非阻塞 Minor finding 记录于 Cycle，含 30 s 指示后的陈旧 120 s 文案与 design D5.1 武装假设被板测证伪的记录性差异）。Tasks 1.1–1.4、2.1–2.3 全部闭合，Map 无后继 Iteration；change 达到收尾条件，等待用户指令 docs-maintainer 收尾。

## Planning Progress

| Phase / Step | Status | 结果 |
|---|---|---|
| CLARIFY：需求与 BDD 扫描 | completed | 已并入2026-10-03用户裁决：证明插拔有效，不比较人工动作计数 |
| Gate 1 | PASS | 原需求批准与本次修订裁决；不授权实施 |
| INVESTIGATE：代码、调用链、资料和基线 | completed | 复用有效基线；DTB/R69与K1字段/Marvell相关实现支持固定设备契约 |
| PLAN：设计、任务、Map、当前 Cycle | completed | 两轮Map不变；首轮001-replan保留原执行范围，D5闭合后轮契约 |
| Completeness / Gate 2 | PASS | 无Missing；R4 Simplified及整体修订计划均已批准，原话见proposal |
| 验证基线重跑 | SKIPPED | 目标表面相对 MS09 accepted 基线未变化，直接采信；不是未执行测试冒充通过 |
| Act / 全局状态 / 归档 | SKIPPED | 本次只有 Plan 授权，阶段边界在计划交接 |

## Requirements Traceability Matrix

R1–R5 按 delta specs 的 Requirement 顺序。Covered 表示计划链路闭合，不表示实现完成。

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 AIA 底座与初始化分离 | 1.1–1.4 | `stopei_id_is_high_field`、`claim_precedes_handler`、`global_and_per_hart_init_are_separate`；board stage9 双入口与 handler 内新 MSI | Covered |
| R2 claim/status 对齐与 MAC 路由 | 1.2、2.1–2.3 | paged MDIO/route host tests；board真实插拔→source60/INSR bit4与raw edge，133/277状态 | Covered：D5固定契约，raw四字无需猜bank |
| R3 破坏性 cause 保留 | 2.1、2.3 | `insr_snapshot_survives_clear_on_read`、`all_raw_cause_bits_are_retained`、`phy_cause_precedes_pad_clear_and_rearm`；board原始快照 | Covered |
| R4 重复、disable、quiet、持续电平 | 1.1、1.3、1.4、2.2、2.3 | 软件逐次计数/双mask/quiet；设备再触发/稳定后quiet；持续assert有界止损 | Simplified：2026-10-03用户原话「核心问题是证明拔插有效就行」批准撤销人工计数相等与固定次数，见proposal |
| R5 验证、兼容、失败恢复、Evidence | 1.4、2.3 | 原生 harness、make k3/build/lichee、MS09 stage1–8、各轮 reset 与纯串口日志 | Covered |

## Iteration Plan

### Iteration 000：可诊断 AIA 软件投递底座

- Tasks: 1.1–1.4；Depends on: None。
- Stable baseline: K3 APLIC/IMSIC 初始化、固定路由、claim+EOI/dispatch 和双软件入口已板测；原 stage1–8 保持。
- Verification boundary: 实际生产逻辑 host GREEN、平台构建、stage9 双入口重复/disable/re-enable/handler 新事件/quiet、复位恢复。
- Diagnostic boundary: paging、固件几何、APLIC、IMSIC、source route、平台 dispatch；不把 PHY wiring 混入底座失败。
- Non-goals: pinctrl/PHY 中断使能与真实设备投递，MAC DMA，timer/IPI 接收和多 hart。
- Balance audit: 纯核、平台接入、探针和板测共同形成一个可依赖结果，单独按文件拆轮不会产生稳定底座；设备链属于另一故障域，已经拆出。

#### Tasks

- [x] 1.1 在 `crates/axplat-riscv64-k3/src/` 增加可 host 测的 AIA 寄存器/路由/claim 核，`tests/k3-aia-host-harness.rs` 建 RED→GREEN，覆盖正确 EID 字段、RV64 selector、source 边界、init/enable/disable/claim 顺序和未知 EID；验证为 rustc 原生测试。
- [x] 1.2 替换 `irq_stub.rs`、接入 `init.rs/lib.rs`、补 APLIC MMIO range；复用 HandlerTable、axcpu、SBI；保持 timer、通用 hook 和非 K3 平台语义。依赖1.1；`make k3` 与实际生产顺序测试为 GREEN。
- [x] 1.3 在 `kernel/src/platform/k3.rs` stage8 后追加 stage9 软件探针，原生 host harness 覆盖成功、错误、1秒超时和 quiet；前台输出快照，handler 不重入 console；接入已有 host-test 方式。依赖1.2。
- [x] 1.4 执行目标 host/兼容构建，再按 R68 上板观察 stage1–9、双入口三次、双 mask、handler 新事件和 quiet，保存用户要求的 raw serial Evidence，reset 回原系统。依赖1.3；上板部分由操作者执行，其结果审核 PASS 前停止下游。

当前 Cycle：`iterations/001-phy-device/000-initial.md`——Plan Context ready（自包含，含后轮任务契约全文与 Iteration 000 accepted 基线），Act Response `reported`，Plan Review `accepted`（2026-10-03）；change 无剩余 Cycle。Iteration 000 已收敛：`000-initial.md` 的 Plan Review 为 replan-required（其 Plan Context 保留），`001-replan.md` 的 Plan Review 为 accepted（2026-10-03）。

### Iteration 001：PHY 真实设备 IRQ/status 基线

- Tasks: 2.1–2.3；Depends on: Iteration000 accepted。
- Stable baseline: source60 与 pin35、INSR 对齐；快照先于破坏性 ack；MAC133/277 路由/状态已观察；重复/quiet/故障恢复有直接板上结果。
- Verification boundary: page/COR/ack 的host测试；拔线与插回真实变化、status对齐、再触发和quiet；持续电平有界止损；MAC raw状态、受影响回归和reset。无固定人工循环次数。
- Diagnostic boundary: pinctrl edge、PHY INER/INSR、MDIO 所有权、板级 INTN；复用前轮控制器结论，不用软件自测替代设备验收。
- Non-goals: DMA/descriptor/network、交互 IRQ 和性能。
- Balance audit: page 操作与 pinctrl ack 必须共同实现一条真实 IRQ 链，拆成单项观测轮不产生可用设备基线；设备压力与恢复同属该链，保留在同轮。故障域较控制器不同，独立一轮。

#### Tasks

- [x] 2.1 实现D5的paged MDIO、完整raw edge快照和固定clear/re-arm顺序，并用原生host测试建立RED→GREEN。依赖1.4 accepted。（2026-10-03：host 7 witness RED→GREEN；板测证实 paged 访问与 INSR COR 真实有效。实现另含板测三次迭代得出的 pin35 武装顺序：GPIO mux+FALL 释放+CLEAR hold 释放，详见 Cycle 2.1 偏差记录）
- [x] 2.2 追加stage10设备窗口、source60 handler和MAC133/277状态；处理quiet、再触发、持续电平/容量满止损和超时。依赖2.1。（2026-10-03：host 10 witness RED→GREEN 共 17/17；板上 stage10 30s 窗口实测：插拔→source60 claim+INSR bit4+link 对齐，quiet=1，无风暴；板测还暴露 stage9 风暴止损关闭 SEXT 需 stage10 入口重开，已实现）
- [x] 2.3 完成受影响回归与真实插拔/status/quiet/reset验证，保存required纯串口片段。依赖2.2；不以人工动作计数判断成败。（2026-10-03：host 全绿（device 17/17、phy 30/30、mac 15/15、make k3/build/lichee exit0）；板上第三次实测 D2/D3/D5 串口判据直接成立（拔线 idx=0 insr=0x0010 link=down aligned=1，插回 idx=2 insr=0x0010 link=up aligned=1，quiet=1，回声 claim aligned=0 不计入）；操作者确认 reset 回原系统；required Evidence `evidence/001-phy-device/000-initial/board-phy-irq.txt`+README）

#### 后轮任务契约

后轮展开时将以下契约与前轮accepted的新基线写入其自包含Cycle；现在不创建目录，不留设计选择给Act。

**2.1：可测试的设备cause与ack顺序**

- Requirement/Scenario: R2真实对齐，R3 COR前快照/组合位，R4再触发，R5错误停止；Targets: `kernel/src/platform/phy_probe.rs::{mdio_read,mdio_write}`及新增paged操作，`kernel/src/platform/k3_device_irq_probe.rs`，K3 `axconfig.toml`，`tests/k3-device-irq-host-harness.rs`。
- Current → Required: 现有只读标准C22，无vendor页/edge ack；新增page保存/恢复（selector1f）、INER a42:12 bit4、INSR a43:1d一次COR、完整edge四字读取；MFPR35 d401e08c仅RMW bits4..6，按D5先保存cause、读PHY、置CLEAR/读回/清CLEAR重备。GPIO页MMIO描述100→1000，覆盖DT gpio-edge子窗口；不新增重叠映射。
- Preserve / Forbidden: 保留mux/电气、IO-PD、旧stage6一次reset、MDIO10ms等待；不猜bitmap、不写edge syscon、不复制K1 pin地址、不新增PHY reset、无前台/handler并行MDIO。
- Test witness / GREEN: host直接调用生产模块，先观察RED再GREEN：`paged_access_restores_original_page`、`insr_snapshot_survives_clear_on_read`、`all_raw_cause_bits_are_retained`、`phy_cause_precedes_pad_clear_and_rearm`、`pad_ack_preserves_mux_and_electrical_bits`、`mdio_error_reports_primary_and_restore_failure`、`edge_window_is_read_only`。测试证明固定软件顺序，K3效果留给2.3。
- Verification: `rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 后 `/tmp/k3-device-irq-host-test`，原生0 failed、exit0；`make k3` exit0。
- Stop when: 实际地址/访问语义否定D5、MDIO超时或恢复失败；返回Plan，不改试其它offset/清法。

**2.2：设备窗口与有界handler**

- Requirement/Scenario: R2设备/MAC路由，R3快照，R4quiet/持续电平，R5恢复；Targets: `kernel/src/platform/{k3.rs,mod.rs,k3_device_irq_probe.rs}`、K3 IRQ观察接口、同host harness与Makefile原生条目。
- Current → Required: stage9后追加120秒stage10；唯一source60 handler读取edge四字、MFPR、INSR与双读BMSR当前link，先快照后ack；前台输出source/raw status/link与结果。最多32条固定快照，容量满或持续claim时mask60/EIE60保存失败，禁止覆盖尚未输出cause。source133/277注册并显示APLIC pending/enable/target、只读GMAC1160和现有APMU wake-enable bit12；若出现非预期MAC事件记录raw后mask，不伪装DMA验收。
- Preserve / Forbidden: 控制器前轮契约和原stage1–8；不在IRQ内console、不在设备窗口前台MDIO、不新增DMA/WoL写入、无人工计数协议。
- Test witness / GREEN: 生产probe host RED→GREEN覆盖 `link_change_matches_source60`、`wrong_source_or_missing_insr_link_bit_fails`、`cause_snapshot_is_not_overwritten`、`quiet_window_has_no_new_claim`、`persistent_level_masks_and_reports`、`device_deadline_stops_without_success`、`mac_routes_are_observed_without_software_trigger`。
- Verification: 同harness原生命令，make k3；源60+INSR bit4是对齐判据，raw edge全字保留但不要求未确定的pin-bank位解码。
- Stop when: 需要改控制器ownership、DMA或另一设备路径；持续assert或存储满是本契约已定义的运行失败，不继续下游。

**2.3：证明真实插拔有效**

- Requirement/Scenario: R2–R5；Targets: 本轮Act Response、CoM260 Kit串口、required `evidence/001-phy-device/000-initial/board-phy-irq.txt`和README。
- Current → Required: 现无设备IRQ运行结果；原生目标harness及受影响PHY/MAC回归、make k3成功后，按R68 RAM boot进入stage10；操作者拔线观察source60/INSR bit4/link down，再插回等待link up与新的对齐claim；ack后的后续投递成立、稳定1秒quiet，reset回原系统。不设固定循环次数，不要求动作数=claim数，不另设快速插拔数量验收。
- Test witness / GREEN: 2.1/2.2实际RED→GREEN；板上必须实际出现拔线/插回的link变化及对应claim/raw快照，并在稳定后quiet，reset执行人一句话确认。组合位保存host证明，raw设备值如实留档。
- Verification: 新harness与受影响 `tests/phy-probe-host-harness.rs`、`tests/mac-probe-host-harness.rs` 的既有rustc原生命令；涉及平台映射时make build/lichee。采信未变范围已有PASS，不重复全量。板上步骤沿R68同地址命令，直接串口判定，deadline120秒。
- Preserve / Forbidden: RAM-only、不saveenv，pure serial required Evidence；不得软件伪造设备事件、输出解析器、身份字段、把host结果冒充真板。
- Stop when: 操作者无结果、raw Evidence缺失、source/status错配、无真实link变化、quiet失败或reset未恢复；报告实际失败而非缺资料，原控制器结论保留。

本轮目录和 Cycle 已于 Iteration 000 accepted（2026-10-03）后展开：`iterations/001-phy-device/000-initial.md`。Current Baseline 复用 001-replan 最终 Act Response 与 accepted Review，后轮任务契约已完整并入该 Cycle 的 Task Contracts；本节契约文本保留为批准记录。

## Evidence and Boundaries

- 第一轮required：`evidence/000-initial/001-replan/board-aia-irq.txt`与README，支持控制器板上Acceptance；第二轮按Map预期 `evidence/001-phy-device/000-initial/board-phy-irq.txt`。用户明确要求raw串口，摘要会丢失claim/status/ack的决定性结构；实体板+操作者不能由agent低成本重跑，缺日志阻止板上Acceptance接受。host/build仅进Act Response；当前不创建任何Evidence。
- Host/build 普通结果只写 Act Response；无 hash、run-id、manifest、身份字段、资格工具或日志判定脚本。
- 授权边界：计划已批准，后续另行指令Act；能力边界：未来实体板操作和实际串口结果；停止边界：本次Plan交接。资料替代和插拔有效性已由用户决定，不再作为等待事项。

## Planning Checks

- `openspec validate ms10-com260-kit-diagnosable-device-irq-baseline --strict --no-interactive`：输出 `Change 'ms10-com260-kit-diagnosable-device-irq-baseline' is valid`，exit0。该检查证明change格式有效，不证明Gate2或产品验收通过。
- 本次修订后重新执行上述严格校验：`Change 'ms10-com260-kit-diagnosable-device-irq-baseline' is valid`，exit0；`openspec status --change ms10-com260-kit-diagnosable-device-irq-baseline`为4/4规划artifact，exit0。计划现已批准，不声称产品或板上验证通过。
- 人工交叉检查：全部任务各归一轮；仅当前Iteration展开；原draft的Plan Context保留并通过001-replan修订；所有任务有契约、测试、停止条件，R4简化有用户批准；没有新增Evidence目录、产品代码或全局文档写入。
