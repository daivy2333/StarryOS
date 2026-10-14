# Iteration 001 / Cycle 001：移交后投递断层判别与修复（rework）

## Plan Context

- Status: ready
- Cycle Type: rework
- Parent Cycle: `000-initial.md`（Review Result: rework-required）

**Cycle Scope**

- Acceptance gaps：A4（K3 owner/link 板测）——移交后 RI/TI/timer 投递断层，ping 100% 丢三轮未收敛（Gate 5）；A3（QEMU 回归）——未开始。
- Repair items：T5-R1（判别插桩）、T5-R2（定点修复）、T5-R3（A4 重跑）、T5-R4（A3 回归与收尾镜像）。
- Inherited scope：T4 全部成果与验证（axnet 三面 486/516/519、Gate 3/4）与 T5 host 侧成果（recovery_contract 8/8、全包 24、harness 36、MS11 17、构建门）直接采信，不重做；本轮移交路由修复（移交只留 `mask_notify`）与 `unregister(60)` 前置为已落地事实。
- Excluded scope：T6/T7、socket 组合、多 hart、性能；axruntime/axtask/axplat 通用层的任何修改（判别指向这些层即停，见 Stop when）；禁止修改 AGENTS、全局 tasks/M/R/I、Runbook/Issue。

**Objective**

用不依赖 timer 的判别插桩在单轮板测内钉死断流边界与故障层，按判别树在本 change 表面内定点修复，随后 A4 按父 Cycle 判据全项通过、A3 QEMU 回归一次通过，收尾重建 LOG=off 生产镜像。

**Background**

父 Cycle A4 三轮板测（LOG=info 两轮插桩）钉死的事实链：

1. owner 武装后 `DMA_CHAN_IRQ_EN=0xd041`（info 行直证）；AIA 路由 133 开（移交步已删 `set_enable(133,false)`）。
2. T5 link 分步机板上完成（`runtime link snapshot up=true`，BMSR up）。
3. ping（含 ARP 广播）零 RI：`proto-begin` 行 `status=0x8400`（仅 NIS+BIT10，无 RI/TI 锁存）、`rx-done=0`、`aia-traps=85` 冻结于 session2 最后一次 claim（identity 0x85=133）。
4. 移交后零 TI trap：payload 80 次完成零 trap，`status` 现值 TI=0（被某 poll 路径 W1C 或未置位）。probe session2 的零 TI 增量为探针设计内行为（重建后 `irq-en=0x0`、terminals 走同步 OWN），不是缺陷。
5. payload 卡死在首个 timer 依赖点（`udp_echo_once` 15 s `timeout` 永不触发，四轮 boot 同位置）；所有 `axtask::sleep` 类未来不完成，quiet/resume 行无法打印（测量死角）。
6. feature 图正确（`cargo tree -e features -i axruntime` 证明 axruntime/irq、axhal/irq、axtask/irq 启用）；静态分析与板上事实矛盾到此边界。

Plan Review 裁决与排序（见父 Cycle Plan Review）：timer 槽走 `sie.STIE` 本地中断（axplat `timer_irq.rs::TIMER_IRQ_VECTOR=(1<<63)|5`），与 GMAC MSI 的 IMSIC/SEI `stopei` 路径（aia.rs）结构分离；历史证据（MS12 quiet、父 Iteration 000 probe 的 2 s 停止窗完成）与本轮 probe 阶段均显示 timer 存活——断流边界在 probe 结束→payload 首个 timer 依赖点之间（移交/owner 激活/source60 武装窗口），跨 MS17 的独立 timer 布线缺陷为低概率假说，不作前置。

**Investigation Facts**

- Current Baseline：父 Cycle Act Response（blocked）+ 本 Review 抽查；工作树含 T4/T5 产品与测试改动 + 诊断脚手架（LOG=info 三处 info 行、payload `status/rx-done/aia-traps/aia-claim` 四字段、axplat `AIA_DEBUG_TRAPS/AIA_DEBUG_LAST_CLAIM`（aia.rs:583-672，含 `last_raw_claim()` 观察口）、board `gmac_irq_en_snapshot/gmac_status_snapshot`）；当前 `StarryOS_riscv64-k3.fit` 为 LOG=info 诊断镜像。
- 关键代码面：
  - 移交路径：`k3_async_net.rs::board::run_k3_async_net_service`——stage10 屏蔽 `[60,133,277]`+`unregister(133)`（:1048/:1182，既有退役步）；`register(133)` 即开路由（CHIP 语义）；`arm_phy_source60` 在 register(133) 后（`unregister(60)` 前置于 :403，paged MDIO 使能 INER、注册 60 并开路由）；probe 移交步只 `mask_notify`。owner 的 arm/recheck 协议只操作设备侧 IRQ_EN，不触 AIA 路由。
  - timer 路径：axplat `timer_irq.rs`——`TIMER_SLOT` 独立 handler 槽、`ProductionCsr` 用本 hart `sie.STIE`；与 aia.rs 的 SEI claim 分离。axtask sleep 依赖 `on_timer_tick` 驱动的 timer wheel。
  - 观察口现状：`proto-begin` 已打印 `status/rx-done/aia-traps/aia-claim`（hang 前可得）；aia.rs 有 trap 计数与 `last_raw_claim`；缺 timer tick 计数、缺 `sie`（STIE/STIP/SEI/SSEP）与 IMSIC EIP/EIE 快照、缺移交窗口（probe 末尾/owner 激活）两侧的打点。
- 判别树（R2 的执行依据，R1 数据落哪个分支走哪支）：
  - (a) tick 在 probe 阶段递增、移交后停跳，且 `sie.STIP=1` 挂起 → STIE 被清或 handler 链断（移交窗口内的代码动作）→ 修复在本 change 表面（k3_async_net 移交/武装序、axplat-k3 被 this-change 触碰的接线）。
  - (b) tick 全程为零（probe 阶段也不递增，且停止窗完成）→ tick 计数插桩点本身错误或 timer handler 未经过该点 → 先修插桩再判别（仍属 R1/R2 预算内一轮），连续两轮无判别力返回 Plan。
  - (c) tick 全程递增、sleep 仍死 → axtask waker/timer wheel 接线（axtask/axruntime 层）→ 停，交用户裁决（范围外）。
  - (d) tick 与 MSI claim 均正常、RI/TI 仍零 → 设备侧（GMAC 移交后 RX/IRQ 输出语义）→ 在 k3_gmac 表面内诊断修复；若证据指向 Iteration 000 rebuild 的中断输出语义缺陷，报回 Plan 裁决是否开 Iteration 000 rework。
- MS10/MS17 裁决事实与 RAM boot 流程沿用父 Cycle 引用。

**Implementation Guidance**

R1 插桩延续既有脚手架形态（计数器 + 快照行 + 无条件打印），不新增协议/判定器；打印点选在 hang 前必经之路（probe 里程碑、payload 各阶段进入点），不依赖任何 sleep future。R2 修复最小化：只动判别树指认的动作点；host 可表达的接缝先 RED（如移交/武装序的契约可测部分），纯板级语义以判别数据为 RED、修复后 A4 为 GREEN。

**Task Contracts**

### T5-R1：投递断层判别插桩（timer 无关观察面）

- Requirement/Scenario：A4 前置（R8/S19 诊断边界）；S09 的观测面。
- Depends on：None。
- Targets：`k3_async_net.rs`（probe 里程碑与 payload 阶段打点）、axplat `timer_irq.rs`（tick 计数，形态随既有 debug 计数器）、必要时 aia.rs（`sie`/EIP/EIE 快照观察口，复用 `last_raw_claim`）。
- Current behavior：`proto-begin` 四字段 hang 前可得；无 tick 计数、无 `sie`/IMSIC 快照、无移交窗口两侧打点；quiet/resume 行 sleep 门控不可达。
- Required behavior：单轮 boot 内可得时间有序的判别行：probe baseline / 停止窗后 / 重建后 / 移交前 / owner 激活后 / payload proto-begin / 首个 timer await 进入点——每行含 tick 计数、`sie`（STIE/STIP/SEI）、aia-traps/last-claim、`gmac_irq_en_snapshot/gmac_status_snapshot`；全部打印不依赖 sleep。
- Preserve：插桩不改任何生产行为；诊断镜像构建 exit 0；普通镜像无新输出。
- Forbidden：判定层/validator/身份字段；为打印新增协议；修改产品控制流（纯观察）。
- First test：插桩为观察脚手架（无行为变更），RED 不适用；构建门为验证（k3-async-probe exit 0）。判别行齐备性由 R3 板测会话直接行使。
- GREEN condition：一轮板测（probe 镜像、RAM boot）走完 payload 进入 hang 前，判别行完整落盘，能按判别树归入唯一分支或明确 (b)。
- Verification：`make k3-async-probe` exit 0 + 一次判别板测会话（A4 boot 步骤到 payload hang 点；操作者照抄判别行）。
- Stop when：判别行与事实链矛盾或连续两轮无判别力；返回 Plan。

### T5-R2：按判别树的定点修复

- Requirement/Scenario：R8（A4 可通过）；S01/S02/S09 的板上闭合。
- Depends on：T5-R1 判别数据。
- Targets：判别树指认的表面——(a)/(d) 分支限 `k3_async_net.rs`、`k3_gmac/*`、axplat-riscv64-k3 中被本 change 触碰的接线；不动 trait。
- Current behavior：见 Background 事实链。
- Required behavior：消除断层——移交后真实 RI/TI 恢复投递、timer 依赖 future 正常完成；修复动作最小（只动指认点）；host 可表达的接缝先 RED 再 GREEN。
- Preserve：父 Cycle 全部 Invariants（唯一 owner、QueueEpoch 语义、flush 次序、IRQ 不触锁）；MS12/Iteration 000 已验收语义；Iteration 000 微步机与 adapter 契约。
- Forbidden：跨表面修复（axruntime/axtask/axplat 通用层）；自动重试到通过；为绕过断层改用轮询替代中断投递。
- First test：host 可表达接缝先 RED（如可测的移交/武装序契约）；纯板级语义以 R1 判别数据为 RED 记录。
- GREEN condition：修复后 host 受影响子集与全包 GREEN；判别行显示断层消除（tick 恢复/claim 递增）。
- Verification：受影响 host 子集 + 轮末全包；`make` 受影响构建门。
- Stop when：判别指向范围外层（(c) 或 (d) 中指向 Iteration 000 rebuild 语义）——停下，写 Blocker Handoff 交用户/Plan 裁决；不自行扩面。

### T5-R3：A4 板测重跑

- Requirement/Scenario：A4 全项（R3/R4；S06/S07/S08/S09/S20）。
- Depends on：T5-R2 GREEN。
- Targets：无新产品面；板测会话按父 Cycle A4 最短步骤原文执行（拔线 ≥3 s、`phy60` events 递增、`lifecycle=2` 保持、插回后 UDP/TCP echo 新会话、`owner=1` 不变、外部 RESET 回官方系统），叠加判别行确认投递恢复。
- First test/GREEN/Verification：同父 Cycle A4 判据；操作者执行、Act 审核 PASS 后 T5 才闭合。
- Stop when：任一判据不满足——保留现场判别行，按 Gate 5 处理。

### T5-R4：A3 QEMU 回归与收尾镜像

- Requirement/Scenario：A3（R1/R3/R8）；S19。
- Depends on：T5-R3 PASS（A4 结论决定其必要性，父 Cycle 预算保留）。
- Targets：QEMU 会话（ms07 runbook 原文命令与六 case）；收尾 `make k3-async`/`make k3-async-probe` LOG=off 生产镜像重建；脚手架处置。
- Required behavior：QEMU 六 case 原生输出与退出码直接判定（恢复成功路径 fault 计数按单次 terminal 语义从 2 变 0 为预期差异，validator 差异行原样回报）；生产镜像 LOG=off 且判别行不再输出；R1/R2 脚手架按“清理由本次改动产生的孤儿”处置——T7 会复用的 cfg 门控计数器可保留，payload 调试字段与无条件 info 行移除（最终清单在本 Cycle Plan Review 定）。
- Verification：runbook 命令 exit 0 + 六 case 通过 + LOG=off 构建门 + 最终 diff 无孤儿脚手架。
- Stop when：QEMU 回归暴露共享面新缺陷——按新证据报回 Plan。

**Invariants**

- 父 Cycle Invariants 全部延续；判别与修复不得改变任何已验收语义。
- 修复限本 change 表面；范围外一律停交用户（含 Issue 候选报告：若确认 MS17 timer 布线或 axtask 层缺陷，报告后由 Recorder 按用户指令落账）。
- 板测经 RAM boot runbook，无持久写入；外部 RESET 回官方系统。

**Non-goals**

T6/T7 场景、socket 框架、SMP、长稳、性能；不修改 axnet 共享面（T4 成果已固化，本轮只有 R2 指认时例外，且需 host RED）；不建 Issue/Runbook（候选报告即可）。

**Acceptance**

- A4'（=父 A4）：拔插/echo/events/lifecycle/owner 全项 + 判别行确认投递恢复。
- A3'（=父 A3）：QEMU 六 case 一次通过（预期 fault 计数差异按上述处理）。
- A1/A2 沿用父 Cycle 已采信结论；R2 若触碰 axnet 共享面，仅受影响面定向重验，不重跑全量。

**Verification**

host：受影响子集随时；轮末 `cargo test --manifest-path tests/k3-async-net-host/Cargo.toml --locked --offline` 全包一次。构建：R1/R2 后受影响 k3 镜像；轮末全门 `make k3`、`make k3-shell`、`make ARCH=riscv64 SMP=1 build`、`make k3-async`、`make k3-async-probe`（LOG=off 生产镜像，probe 最后）。板测：判别一轮（R1）+ A4 重跑一轮（R3）；QEMU 一轮（R4）。判别行与板测一句话进 Act Response；人工项最短步骤 + 直接可观察结果。

**Gate 2 Readiness**

PASS：2026-10-13 用户原话“批准”。技术自检结论采信未变（调查完成、判别树穷尽且各分支有 Stop when、无 TBD、repair item 映射 A4/A3；BLOCKED：None；WAIVED：None）。Status 转 `ready`。

**Persisted Evidence**

- Mode: none。判别行、命令、退出码、板测/QEMU 一句话进 Act Response；安全 RAM 板测与 QEMU 均可低成本重跑。

**Risks and Notes**

- 分支 (b)（插桩点无判别力）允许一轮修正，连续两轮无判别力返回 Plan——防诊断无限循环。
- 判别轮 payload 停在 hang 点即判据已足（判别行在 hang 前打印），无需等待 timeout。
- `service_owner.rs` fault==1 断言、AX_IP 约定等既有事实延续。
- 既有 Minor finding 延续（`snapshot_line` 的 `[ms12]` 前缀等）随脚手架处置一并清理或保留，最终清单在 Plan Review。

## Act Response

- Status: reported
- Date: 2026-10-14（R1 判别会话与 R2 修复确认在 2026-10-13 深夜至 2026-10-14 完成）

### 实际改动（文件与符号）

**T5-R1 判别插桩**（observation-only，无生产行为变更）：

- `crates/axplat-riscv64-k3/src/timer_irq.rs`：新增 `TIMER_DEBUG_TICKS` 计数器，在 `handle_irq()` 顶部递增（每个到达平台派发层的 timer trap 必经点，先于 slot 状态判断；形态同 `aia::AIA_DEBUG_TRAPS`）；新增 `TrapCsrSnapshot`/`debug_trap_csr_snapshot()`（`sie`/`sip`/`sstatus` 裸值一次读出，riscv64 cfg 门控，host harness 不编译）。
- `kernel/src/platform/k3_async_net.rs`：`board::discriminator_line(tag, out)` 判别行（ticks、sie/sip/sstatus 裸值 + stie/stip/sext 解码位、aia-traps/last-claim、irq-en/status 快照；全部同步读取，无 sleep 依赖）；七个里程碑打点：`probe-baseline`/`stopped-window`/`rebuilt`/`handover`（`native_recovery_probe` 内）、`owner-active`（`run_k3_async_net_service` 内 cfg 门控）、`snapshot_line` 扩展同组字段（payload 相）、`udp-enter`（`udp_echo_once` 会话块顶部——payload 首个 timer 依赖 await 进入点）。普通镜像全部 cfg 排除，无新输出路径。

**T5-R2 定点修复**：

- `kernel/src/platform/k3_async_net.rs` `native_recovery_probe` 第 8 步（移交）：`mask_notify` 后新增 `axhal::irq::set_enable(GMAC_IRQ_SOURCE, true)`，注释改为与事实一致。单行行为改动，限本 change 表面。

### 根因判定（R1 判别数据 → R2 依据）

1. timer 陷阱投递全程存活：ticks 7463→7903 递增穿过移交窗口，`stip=0`、`stie=1`、全局 SIE=1、无重 trap 风暴——排除朴素分支 (a) 的 "STIE 被清"与分支 (b)；结合修复后 quiet 2 s sleep 正常完成（ticks +206），分支 (c)（timer wheel 断）整体排除。父 Cycle "sleep 类未来不完成"为路由关闭的下游假象（recv 永不返回阻塞在 udp）。
2. 真实断层为 GMAC MSI 路由：移交后零 claim（`irq-runs` 冻结、`aia-traps` 冻结于停止窗在飞 claim），owner 已武装、对端在 ping。
3. 根因：probe 第 2 步（Quiesce）`set_enable(133,false)` 关闭 AIA 路由后无人重开——owner 协议只操作设备侧 IRQ_EN；axtask `register_irq_waker` 的首注开路由仅在 Vacant 分支生效（基线会话已消费）。父 Cycle 将关闭点误归因第 8 步并删除之，第 2 步关闭一直存留；其"路由 133 开"系静态分析结论，被板上判别数据直接推翻。修复后路由与普通镜像不变式一致。

### 板测会话（操作者执行，Act 审核）

- **判别轮（修复前）**：判别行齐备落盘，唯一归入"移交路由关闭"分支；事实链见上。
- **修复确认轮**：`owner-active` 起 aia-traps 恢复递增（86→167→191）；`proto-begin` irq-runs=123、ti=101；UDP echo `ok bytes=6`、TCP echo `ok bytes=8 closed=1`；settle/quiet/resume/proto-end 全到达，quiet 2 s sleep 完成（ticks 9897→10104）；全程 `lifecycle=2 owner=1`；ping 有回复。**R2 板上 GREEN 成立。**
- **S07 有界失败轮**：proto-begin 后拔线 → `proto udp err=NotConnected`、`proto tcp err=NotConnected`（T4 D6 link 门控设计内终态）；link-down 期间 `lifecycle=2 owner=1` 保持；`phy60=events=1`（下降沿）、`aia-claim=0x3c003c`（identity 0x3c=60）、handler MFPR ack `set=0xa460→rearm=0xa420`——source-60 端到端成立。
- **S06+S08 尝试轮**：flap 前 UDP/TCP echo 均 ok；拔线落在 quiet 窗内，拔线 PHY 事件自身触发 resume（quiet-after 后 +0.46 s），payload 直达 proto-end，插回+ping 未获窗口；`phy60=events=1` 仅下降沿。

### A4 结论与残余

已直接行使：link-down 有界失败（NotConnected，S07 字面证据）、link-down 不 fault owner（lifecycle=2/owner=1 两轮保持）、source-60 事件端到端（claim/ack 证据）、修复后全部协议相（echo/quiet/resume，S16 前置）、外部 RESET 回官方系统（多轮 boot 间）。**残余**：完整 down→up 上升沿与插回后新 SocketEpoch 会话交换未在单 boot 内直接观察（resume 窗被拔线事件提前触发，机制已定位）。用户裁决原话："我们已经验证功能正常了，不纠结阻塞在这里，如果没有大问题就填写回复吧"。残余项如实保留，是否豁免或补测交 Plan Review 裁决。

### A3（QEMU 回归）

用户明确豁免，原话："qemu不重要可以不做，直接填写回复吧"。未执行。风险：T4 axnet 共享面（link 门控/terminal）无 QEMU 真实入口回归；axnet 三面 host 包（486/516/519，父 Cycle 采信未变）覆盖逻辑面，修复后三轮真板 boot 行使了真实数据面。是否在 T7 前补 QEMU 回归交 Plan 裁决。

### 验证命令与实际结果

- `make k3-async-probe` exit 0（R1 后、R2 后、轮末 probe 最后构建，FIT 808480 B 落盘）；`make k3-async` exit 0（普通镜像，判别路径全 cfg 排除）；`make k3` exit 0；`make k3-shell` exit 0；`make ARCH=riscv64 SMP=1 build` exit 0（均 2026-10-14 轮末实测）。
- `rustc --edition=2024 --test tests/k3-timer-host-harness.rs` → /tmp：8/8 通过（`#[path]` 直编 timer_irq.rs 未受影响）。
- 变更 crate 无新增警告（仅 virtio-drivers/smoltcp 既有警告）。
- host 包不受本轮改动影响（board-only 路径），父 Cycle 采信结论（recovery_contract 8/8、全包 24、harness 36、MS11 17）按覆盖未变化沿用。

### 与计划的偏差

1. 判别行在契约字段（tick/sie STIE/STIP/SEI）外增加 `sstatus` 全局 SIE 位与裸值——用于切分分支 (a) 的 "STIE 被清" vs "全局掩"；observation-only，契约意图内。
2. R4 QEMU 部分按用户豁免未执行（原话见上）；LOG=off 收尾镜像重建已由轮末构建门覆盖。
3. R1/R2 脚手架处置：`TIMER_DEBUG_TICKS`/AIA 计数器保留（T7 复用），判别行保留在 probe 构建；最终清理清单按契约留本 Cycle Plan Review 决定。

### 自检发现与遗留 Minor

- resume 触发条件计数任意 ISR publish（含 PHY config 事件）：link 事件与 resume 窗重叠时 resume 被 PHY claim 提前消费（S08 尝试轮机制）。不阻塞本轮；T7 最终矩阵设计时需避开或细化触发条件。
- 停止窗在飞 claim 的 handler-run/trap 计数存在 ±1 残差（判别轮 +2 runs vs +1 trap），不影响结论；如实记录。
- `abnormal`（RBU/TBU）计数在 ping+广播负载下增长，drop 全程为 0、lifecycle 恒 2——良性容量 cause，观察记录。
- `snapshot_line` 的 `[ms12]` 前缀延续（既有 Minor finding，随脚手架清单在 Plan Review 一并处置）。

### Persisted Evidence

None required（mode: none）。判别行与各轮一句话结果已录入本 Response；判别会话与全部构建均可低成本重跑，未创建 Evidence 目录。

### Experience Candidates

None。板测复用既有 RAM boot Runbook；根因与修复属 change 内缺陷（随 closeout 进 specs/design），无范围外实质缺陷证据。

### 未解决问题

- A4 残余（上升沿 + 插回后新会话单 boot 直接行使）——交 Plan Review 裁决豁免或补测。
- 脚手架最终清理清单——本 Cycle Plan Review 决定。
- Iteration 002（T6/T7）待本 Iteration Review accepted 后展开。

## Plan Review

- Review Result: accepted
- Date: 2026-10-14

**Findings**

独立抽查代码与 Act 声明一致：

- **R2 修复**：`native_recovery_probe` 第 8 步 `mask_notify` 后 `axhal::irq::set_enable(GMAC_IRQ_SOURCE, true)`（k3_async_net.rs:1069–1070），注释如实记录真因（第 2 步 quiesce 关闭、owner 协议只开设备侧 IRQ_EN、首注开路由已被基线会话消费）。
- **R1 插桩门控**：`discriminator_line`（:376）与七个打点全部限诊断构建——probe 内四点在 probe 函数内、`owner-active` 有显式 cfg（:1339）、`udp-enter`/`snapshot_line` 位于 `mod k3_net_diagnostics`（:1388 `all(k3, k3-net-diagnostics, riscv64)` 门控）内；普通镜像确无新输出路径。`TIMER_DEBUG_TICKS`/`TrapCsrSnapshot`（timer_irq.rs:144–184）形态同既有 debug 计数器；`tests/k3-timer-host-harness.rs` 为 MS17 既有文件（f93c03f6），8/8 采信。
- **根因判定采信**：R1 判别数据（ticks 7463→7903 穿移交窗递增、stie=1/stip=0、修复后 quiet 2 s 完成）推翻父 Cycle "路由已开"的静态结论——真正关闭点是第 2 步 quiesce 而非第 8 步；父 Cycle 删除的是一次冗余关闭，本轮补上重开。判别先行的排程达到目的，"sleep 全死"确认为路由关闭的下游假象。
- 板测四轮（判别/修复确认/S07 有界失败/S06+S08 尝试）的关键判定行（aia-traps 恢复递增、NotConnected 终态、claim identity 60、MFPR ack、lifecycle=2/owner=1）与代码输出面一致；构建门 5/5、timer harness 8/8 采信（覆盖未变）。

**豁免（原话在案）**：

- A3 QEMU 回归：用户原话 "qemu不重要可以不做，直接填写回复吧"。风险边界：T4 axnet 共享面无 QEMU 真实入口回归，逻辑面由三面 host 包（486/516/519）与三轮真板数据面行使覆盖；后续 Iteration 不得声称"采信 001 QEMU 结论"，T6 触碰共享面时按覆盖变化定向 host 重验；该缺口随 change 收尾入档。
- A4 残余：用户原话 "我们已经验证功能正常了，不纠结阻塞在这里，如果没有大问题就填写回复吧"。裁决：完整 down→up 上升沿与插回后新 SocketEpoch 会话交换**不作为缺口消失，移交 T7 板上矩阵**（其范围本含物理 link flap 与旧/新 socket），Iteration 002 契约显式列入。受控 reset 的板上注入按 001/000-initial A4 步 5 原文即属 T7，非本轮缺口。A4 其余判据均已直接行使（down 有界失败、owner 稳定、source-60 端到端、修复后新会话与 quiet/resume、QueueEpoch link 语义 host 直证）。

**脚手架处置**：`TIMER_DEBUG_TICKS`/AIA 计数器/判别行保留（T7 最终矩阵复用其观察面）；`[ms12]` 前缀延续既有文案约定；最终清理清单在 T7 Plan Review/收尾统一裁决。

**Minor findings（记录，随 T7 设计吸收）**：resume 触发条件计数任意 ISR publish，PHY claim 可提前消费 resume 窗（S06+S08 尝试轮机制）——T7 矩阵设计须避开或细化；停止窗在飞 claim 计数 ±1 残差（不影响结论）；abnormal（RBU/TBU）良性增长（drop=0、lifecycle 恒 2）。

**Acceptance Gaps**

None 阻塞：A4 按用户裁决接受、残余移交 T7；A3 豁免在案；A1/A2 型 host 证据沿用父 Cycle 采信。Iteration 001 稳定基线成立——resident owner 语义（请求门控、Again 1 ms、epoch 丢弃、单次记账）host 全绿，真板 owner/link/协议相与路由修复直证。

**Evidence**

代码抽查行号如上；验证运行采信本 Cycle Act Response（判别行、四轮板测一句话结果、构建门、timer harness 8/8）；mode=none，无 Evidence 目录，符合契约。

**Follow-up Decision**

修复最小且在判别树 (a)′ 分支内（移交窗口代码动作）；两项用户豁免原话在案；残余场景有明确下落（T7）。接受本轮，Iteration 001 完成，按 Map 展开 Iteration 002。

**Iteration Plan Update**

None（Map 不变；T7 矩阵吸收 001 残余属既有范围细化，非契约变更）。

**Next Cycle**

None。

**Next Iteration**

`iterations/002-socket-combination-final-acceptance/000-initial.md`（已展开，draft，待 Gate 2 批准）。
