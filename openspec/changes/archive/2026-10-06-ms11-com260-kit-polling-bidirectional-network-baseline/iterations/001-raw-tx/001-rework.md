# Iteration 001 / Cycle 001: rework — TX tail doorbell 排他语义修复

## Plan Context

- Status: ready
- Cycle Type: rework

**Cycle Scope**

- Change tasks: 2.1, 2.2（既有 Acceptance 修复，不新增范围）。
- Acceptance gaps: A1 板上 terminal/reclaim 分支未达成；A2 板上 20 帧 peer 完整与 local/peer 分列结论未闭合；A5 操作者 reset 一句结论未回报。
- Repair items: 2.1-R1、2.2-R1。
- Inherited scope: 父 Cycle `000-initial.md` 的全部 Task Contract、Invariants、Non-goals、Acceptance 与 Verification 条款继续有效；本文件只修订下述显式条款，冲突处以本文件为准。
- Excluded scope: 同父 Cycle；另明确排除——RX refill doorbell 的排他语义容量问题（真板有效 15/16 槽，见 Investigation Facts）不在本轮修复，留给 Iteration 002 作为契约输入。

**Background**

父 Cycle 板测（2026-10-04）命中 2.2 Stop when：16/16 提交 TerminalTimeout、对端却真实收到请求并回 is-at 应答。Plan Review 分类为 **Plan 错误 + 新证据**：000-initial 契约把 TX tail doorbell 定为"本次提交 slot 自身地址"（从 000 轮 RX 板测裁决外推），偏离了 design D3/R69/Linux 的 next-slot。证据与根因分析见父 Cycle Plan Review 与 `evidence/001-raw-tx/000-initial/`。

**Investigation Facts**

Current Baseline（引用父 Cycle Act Response 与 Plan Review，2026-10-04）：

- host：V1 19 passed/0 failed（14 个 000 witness + 5 个 TX/stage12 witness）、V2 device-irq 17 passed + `make k3` exit0，均新鲜有效；工作树即该基线。
- 板测：link-down 轮实证 stage12 gate 对非 WindowEnded 零 GMAC 访问；链路轮实证 RX 数据面（两个 arena、两次 SWR）正常、TX 有界止损（owner 保留、not-sent=4、无 FBE/ES、backing 保留）按设计工作。

裁决事实（三方一致，本轮修复依据）：**GMAC tail 指针为排他上界——DMA 只 fetch 严格低于 tail 的描述符**。

| 来源 | 事实 |
|---|---|
| 板测时序 | frame 0 的应答在 submit 1 的 doorbell 后到达（此后每帧应答随下一次 doorbell）；≈15/16 请求上线；slot 15 永不被取；每 slot 只在自己的 1s 窗内被轮询，OWN 回写观察必然早于设备取走 |
| R69 同板参考 | `k3_gmac/core.rs` `submit_tx` 写 `dma_chan_tx_end = tx_ring_dma_addr(tx_next)`（递增后的下一空闲槽） |
| Linux `stmmac_main.c` | `stmmac_xmit` 置 OWN 后 `cur_tx` 递增至下一空闲项，`stmmac_flush_tx_descriptors` 写 `tail=addr(cur_tx)`；RX init tail=`buf_alloc_num`（首个未 refill 项）、TX init tail=0，同一排他语义 |

推论：OWN 未清零无需 IOC（H2）解释；H2 保留为 2.2-R1 判读表的分支而非本轮修复变量（单变量裁决）。RX own-slot refill 在排他语义下真板有效容量为 15/16（本轮 15 帧 RX 全落 slot 0..14 吻合）——stage12 重跑 RX 帧数可能接近 15，若出现 RBU/drop 按既有 bounded 语义报告，不判 TX 失败。

Code and Critical Path（当前代码基线）：

- `kernel/src/platform/k3_gmac/mod.rs::try_submit`：doorbell 位于 OWN 发布、descriptor clean、`fence iorw` 之后，当前写 `addr_lo32(desc_pa)`（本次提交槽自身地址，约 L756）——本轮改点。
- `kernel/src/platform/k3_gmac/mod.rs::copy_padded_to_buf`：只复制源字节，不清零 pad 区——本轮改点（父 Review Minor 3）。
- 其余 TX 逻辑（长度拒绝、Full 零接触、10ms submit 检查点、1s terminal、TimedOut 永不复用、cancel/ES/FBE、reclaim phase）已经 host 见证且板上行为符合契约，不动。
- RX 一侧（refill_slot own-slot doorbell）本轮明确不改。

**Implementation Guidance**

先在既有 harness 建 RED（tail 断言 + 脏 buffer pad 断言），再改两处生产代码至 GREEN；`raw_tx_sequence_requires_rx_and_bounds_wait` 无需修改（fake 不建模 tail 语义）。

**Task Contracts**

### 2.1-R1: TX doorbell next-slot 化与 pad 区真补零

- Requirement/Scenario: R3；S1/S3/S9（2.1 契约中未达成的板上部分）。
- Depends on: 2.1 host 部分（父 Cycle 已 done）。
- Targets: `kernel/src/platform/k3_gmac/mod.rs::try_submit`（doorbell 地址）、`copy_padded_to_buf`、`tests/k3-gmac-host-harness.rs`。
- Current behavior: doorbell 写本次提交 slot 自身地址；14..59 帧的 pad 区在 slot 复用时残留旧字节。
- Required behavior:
  - 提交 slot i 后，TX tail(+0x1120) 写 `addr((i+1) mod 16)` 低 32 位（R69 `tx_next`/Linux `cur_tx`/design D3 同义）；ST 重写不变。提交 slot 15 后 tail=addr(0) 属预期（排他上界覆盖引擎位置到 tail-1）。
  - 每次提交把 buffer 中 `frame.len()..TX_PAD_TO` 区写零（含 slot 复用路径）；clean 范围 `block_round_up(pub_len)` 不变。
  - try_submit 其余语义逐字不变（长度拒绝、Full 零接触、检查点、OWN 最后、cache/fence 序列、1s terminal 起算）。
- Preserve: RX 全部行为（含 own-slot refill）、既有 19 个 witness 中除 tail/pad 断言外的全部断言。
- Forbidden: 同时引入 TDES2 IOC=1（单变量裁决）；改 RX doorbell；软件清 OWN；其他任何 TX 语义变更。
- Test witness: 先 RED：提交 i 后最后一次 `DMA_TX_TAIL` 写值断言为 `addr_lo32(tx_desc_pa(i+1 mod 16))`（含 i=15→addr(0) 用例）；脏 buffer 复用断言（先写脏 60B 到复用槽，再提交 14B 帧，断言 14..60 全零）。
- GREEN condition: harness 全部测试 0 failed/exit0。
- Verification: V1；板载裁决在 2.2-R1。
- Stop when: 修订与已采信三方证据矛盾（不应发生）。

### 2.2-R1: stage12 板测重跑与单变量判读

- Requirement/Scenario: R2/R3/R6；S2/S3/S10。
- Depends on: 2.1-R1 GREEN + `make k3` exit0。
- Targets: 无新代码与打印字段（现有 summary 足够）；操作者板测。
- Required behavior: host GREEN 后由操作者执行 V3' 一轮；Act Response 按判读表分类记录一句话结论与决定性串口行。
- Preserve/Forbidden: 同父 Cycle。
- Test witness: `raw_tx_sequence_requires_rx_and_bounds_wait` 保持 GREEN 即为兼容见证。
- GREEN condition: V3' 判读表第 1 行成立 + reset 一句结论回报。
- Verification: V3'（下述）。
- Stop when: 判读表第 2/3 行——保留现场交 Plan，不试写替代语义。

**Invariants**

同父 Cycle。

**Non-goals**

同父 Cycle；另加：不改 RX doorbell、不引入 IOC、不重建 BLOCKED 证据。

**Acceptance**

同父 Cycle A1–A5；本轮闭合 A1 板上分支、A2、A5。

**Verification**

V1：同父 Cycle 命令，全部 0 failed/exit0（含修订后的 tail/pad 断言）。

V2：`make k3` exit0。本轮产品变化仅 mod.rs 两处；k3.rs/探针表面无新变化，device-irq harness 覆盖未变，采信父 Cycle V2；若 Act 实际触及 k3.rs/探针表面则按父 Cycle V2 条款重跑对应项。

V3'（操作者，能力边界）：

1. 同一 RAM boot 拓扑（PC 192.168.137.1/24，Wireshark 过滤 arp）。
2. stage11 窗口结束自动进入 stage12；观察 20 个 who-has 完整帧、slot 复用 ≥4。
3. 判读表（结论写入 Act Response）：
   - 第 1 行 `sent=20 terminals=20 tx-errors=0 tx-timeouts=0 not-sent=0` 且 ≥1 条真实 is-at 应答、应答在对应 submit 自己的 1s 窗内到达（不再延迟一个间隔）→ H1 确认，A1/A2 达成，交 Review。
   - 第 2 行 terminals 仍超时但应答不再延迟（对应 submit 自己窗内到达）→ 取走已修复、回写被抑制：H2 新证据，Stop when 交 Plan。
   - 第 3 行 延迟模式不变（应答仍随下一次 doorbell 到达）→ 排他假设不成立或另有机制，Stop when 交 Plan。
4. reset 后原系统恢复，一句实际结论（必填；本轮不预设豁免）。

**Gate 2 Readiness**

PASS — 2026-10-04 用户计划审计原话“批准”。缺口与根因有三方闭合证据、repair item 契约自包含、验证为单变量判读的最简直接判定；无 BLOCKED/WAIVED、无待 Act 决定的实质未知项。父 Cycle 已 PASS 且材料未变的维度（需求映射、未修订的契约语义、harness 基础设施）引用父 Cycle 结论。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

父 Cycle BLOCKED 证据已存于 `evidence/001-raw-tx/000-initial/`，本轮不重复保存；判读结论由 Act Response 承载（≤20 行决定性行）。

**Risks and Notes**

- RX 有效 15 槽事实：stage12 重跑 RX 帧数可能接近 15，若 RBU/drop 出现按 bounded 语义报告且不影响判读表（只看 TX 计数与应答时序）；该事实留 Iteration 002 契约。
- PC 对重复 who-has 可能抑制应答（上轮 8 条后停止）：判读只要求 ≥1 条；若 0 条应答且 Wireshark 20 帧完整，按第 1 行 TX 计数判定并记录 PC 行为。
- 同一问题当前失败 1 次；按公共规则，连续两个 rework 未缩小同一 gap 或同一问题第三次失败时停下反思。

## Act Response

- Status: reported

**Tasks and actual changes**

2.1-R1 TX doorbell next-slot 化与 pad 区真补零 — done（host 全部）。

- `kernel/src/platform/k3_gmac/mod.rs::try_submit`：tail doorbell 由"本次提交槽自身地址"改为下一空闲槽地址 `addr_lo32(tx_desc_pa((slot+1) mod 16))`（排他上界语义；提交 slot 15 写 addr(0)）；`self.tx_tail = next` 复用同一计算，取值不变。ST 重写、OWN 最后序、cache/fence 序列、deadline/cancel 检查点、长度拒绝、Full 零接触、1s terminal 起算逐字未动。
- `kernel/src/platform/k3_gmac/mod.rs::copy_padded_to_buf`：新增 `src.len()..TX_PAD_TO` 清零循环（每次提交执行，含 slot 复用路径）；clean 范围 `block_round_up(pub_len)` 不变。
- `tests/k3-gmac-host-harness.rs`：新增 2 个 witness——`tx_tail_doorbell_names_next_slot`（16 次提交逐一断言每次恰好一条 `DMA_TX_TAIL` 写且值为 `addr((i+1) mod 16)`，覆盖 i=15→addr(0)）、`tx_pad_zeroed_on_reuse`（pad 区预写脏 0xA5 后提交 14B 帧，断言帧原样复制且 14..60 全零）。

2.2-R1 stage12 板测重跑 — host 前置已满足；V3' 板测在操作者能力边界。

- 无新代码与打印字段（按契约现有 summary 足够）；`raw_tx_sequence_requires_rx_and_bounds_wait` 保持 GREEN 即兼容见证。

**Deviations from Plan**

- 无契约偏差。自检修正一处非实质：`tx_pad_zeroed_on_reuse` 首版 `mut arena` 绑定触发 unused_mut 警告（父 Cycle V1 基线 0 warning），改为不可变绑定后 0 warning，重跑 GREEN。

**Self-check findings and remaining minors**

- Gate 3 独立核对：改动限于两处生产改点与两个新 witness；RX refill own-slot doorbell 未触碰（`refill_slot`/`raw_refill_rx_desc` 无改动）；未引入 TDES2 IOC=1；无软件 OWN 清除；既有 19 个 witness 全 GREEN（Preserve 成立）。
- 遗留 Minor：无新增。父 Cycle 遗留项（`revocations` 死字段、stage12 `printed` 计数含 reply 样本、Full 重试路径 poll_tx 结果被忽略）维持原状，非本轮范围。
- RX 15/16 有效容量事实按本轮排除条款未改，留 Iteration 002 契约输入。

**Verification**（V1/V2；V3' 留操作者能力边界）

- V1：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test` 编译 0 error 0 warning，exit 0；运行输出 `test result: ok. 21 passed; 0 failed`，exit 0（19 既有 + 2 新 witness）。RED 先行见证：tail 断言首跑 `left: [4096] / right: [4160]`（自身槽地址 vs 下一槽地址）、pad 断言首跑 panic `stale pad bytes zeroed on the submit path`，恰 2 failed；修改生产代码后转 GREEN。
- V2：`make k3` exit 0（`Default Configuration: 'conf-starry-k3'`，FIT 镜像生成）。k3.rs/探针表面本轮无改动，device-irq harness 覆盖未变，按 rework Cycle 契约采信父 Cycle V2 不重跑。
- V3'（操作者执行，2026-10-04，串口判读）：**判读表第 1 行成立，H1 确认**。决定性行：
  `stage=12 result sent=20 terminals=20 tx-errors=0 tx-timeouts=0 not-sent=0 rx-frames=20 rx-drops=0 arp-reply=1 window-elapsed-ms=10000`。
  20 条 `tx i=0..19` 跨 slot 0..15 后回绕复用 0..3（恰 4 slot 复用，零 timeout 零 full）；`arp-reply slot=0 len=60 raw=0x30010040 data=…00e04c602f60…0002…c0a88901…c0a88902…` 为真实 is-at 应答（oper=2、spa=192.168.137.1、tpa=192.168.137.2）。上一轮 16×TerminalTimeout（raw=0xb000003c）与"应答延迟一个 doorbell"病态完全消失：20/20 提交均在各自 1s 窗内达成 terminal，证明设备即时取走了每个描述符（排他 tail 语义修复生效）。
  操作者收尾（2026-10-04，原话保留）：Wireshark 豁免——"Wireshark不做，后续也不用做"；reset——"reset早就做完了"（未报告异常）。A2 peer 完整性按豁免以串口侧证据闭合：真实 is-at 应答本身即 PC 收到并解析请求的对端交付证据，与 rx-frames=20 零 drop、20/20 local terminal 互证；Wireshark 帧级计数缺席的风险由用户原话明示接受。A5 以 reset 已执行且无异常报告闭合。A1 板上分支、A2、A5 全部闭合。

**Persisted Evidence**: None required（mode none；本轮判定由 Act Response 与 harness 原生输出承载，无 BLOCKED、无一次性现场）。

**Experience Candidates**: None。

**Unresolved / capability boundary**

- 无未解决问题。V3' 判读表第 1 行成立（H1 确认，2.1-R1 修复生效）；A1 板上分支、A2、A5 均闭合（A2 按用户原话豁免 Wireshark，A5 reset 由操作者确认已执行无异常）。本 Cycle Acceptance 材料齐备，交 Plan Review。
- 控制台可读性 Minor（本轮操作者报告的新证据）：stage 9/10/11/12 输出行在串口上首尾相连。根因：stage≤8 的 emit 显式携带 `"\n"`（如 `phy_probe::run_link_observation`），而 stage9+ 的 `stage9_line!/stage10_line!/stage11_line!/stage12_line!` 宏经 `aia::LineBuf` 输出无换行符。纯输出格式问题，不影响判读与任何行为语义。用户已批准（2026-10-04 原话"同意下一批优化输出"）将该修复并入 Iteration 002 Cycle 契约（stage13 使用同一组宏，下轮板测自然生效），由 Plan 展开时落实；当前 Cycle 契约外，Act 未改动。
- 同一问题（TX terminal/doorbell）失败计数：本轮第 1 行成立，不再累计。

## Plan Review

- Review Result: accepted（2026-10-04；Iteration 001 完成）

**Findings**

独立核验：`try_submit` doorbell 改点（`next = (slot+1) % 16`，排他上界注释，ST 重写与既有序列未动）、`copy_padded_to_buf` 补零循环（含复用理由）与两个新 witness（16 次逐一断言恰一条 tail 写且值为 next-slot、覆盖 i=15→addr(0)；脏 pad 区 0xA5 预写后 14B 帧断言逐字节补零）逐项与 2.1-R1 契约一致；采信 V1（21 passed/0 failed、RED 先行恰 2 failed）与 V2（make k3 exit0；device-irq 覆盖未变采信父 Cycle）。

1. **主判定：V3' 判读表第 1 行成立，H1 双侧确认**。`sent=20 terminals=20 tx-errors=0 tx-timeouts=0 not-sent=0 rx-frames=20 rx-drops=0 arp-reply=1`，20 条 tx 行跨 slot 0..15 回绕复用 0..3，真实 is-at 应答且不再延迟一个间隔。上一轮 16×TerminalTimeout（raw=0xb000003c）与应答延迟病态在唯一变量（doorbell next-slot）下完全消失——TX tail 为排他 fetch 上界由板测病理出现/消除双侧证据与 R69/Linux 用法三方闭合，成为语料库候选事实（随 change 收尾合并）。
2. **新证据修正（Plan 推断错误）**：000-initial Review 的"RX own-slot 使真板有效容量 15/16"推断被本轮 `rx-frames=20 rx-drops=0`（单 arena 16 槽上 >15 帧、含整环回绕、零 drop）推翻。修正后的语义事实：**TX tail=排他 fetch 上界；RX tail 写为唤醒 doorbell、fetch 由 OWN 驱动**，两侧非对称。RX doorbell 维持现状，原计划留给 002 的"RX next-slot 修复"输入撤销。
3. **豁免登记核对**：A2 peer 帧级完整性按用户原话"Wireshark不做，后续也不用做"（2026-10-04）豁免，以真实 is-at 应答（PC 只应答已收请求）+ rx-frames=20 零 drop + 20/20 本地 terminal 互证闭合；该豁免延伸至后续轮次——Iteration 002 的板测验证不再以 Wireshark 为条件（改 PC 原生工具 + 串口分列证据）。A5 reset 由操作者原话"reset早就做完了"（无异常）闭合。两项原话已留存 Act Response。
4. **Minor（用户已批准处置）**：stage9+ 输出行缺尾随换行（宏经 `aia::LineBuf` 无 `\n`，stage≤8 显式带）——纯输出格式，不影响判读；按用户原话"同意下一批优化输出"并入 Iteration 002 Cycle 3.2 契约。
5. 既有遗留 Minor 维持原状（`revocations` 死字段、stage12 printed 计数、Full 重试路径 poll_tx 结果忽略），非本轮范围。

**Acceptance Gaps**

None。A1（板上 terminal/reclaim 分支 20/20 + wrap 单次回收）、A2（20 帧、4 slot 复用、真实应答、local/peer 分列）、A5（reset 一句结论）本轮闭合；A3/A4 已由父 Cycle 与本轮 link-down/正常两轮共同闭合。

**Evidence**

代码与 witness 独立阅读核对；Act Response 的 V3' 决定性行与 ARP 应答样本；V1/V2 采信（来源注明）。本轮无 Evidence 文件——mode none、无 blocker、板测判定行未超 20 行上限，合规。

**Follow-up Decision**

既有 Acceptance 全部满足，修复精准落在 2.1-R1/2.2-R1 契约内且单变量裁决成立；Iteration 001 按 Map 完成，tasks 2.1/2.2 勾选。展开 Iteration 002（含用户批准并入的输出换行修复与 Wireshark 豁免后的验证重设计）。

**Iteration Plan Update**

None（Map 不变）。

**Next Cycle**

None（accepted，无后继 Cycle）。

**Next Iteration**

`iterations/002-protocol-exchange/000-initial.md`（本轮展开，Plan Context 为 draft，等待用户 Gate 2 审计）。
