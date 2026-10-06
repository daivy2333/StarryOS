# Iteration 001 / Cycle 000: 原始 TX、终态与单次回收

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: 2.1, 2.2。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: 已批准 R1–R6、S1–S11 全量保留；本轮承担 R3（有界 TX）为主，及 R1/R5/R6 的 TX 侧；R4 四协议属 Iteration 002。
- Excluded scope: smoltcp/协议 session（002）、异步 queue task/wake、运行时 reset/link-flap 恢复、SMP、offload、零拷贝、性能、shell、存储、MAC DMA IRQ、证据工具。

**Objective**

在已验收 RX 基线上交付有界轮询 TX：接受完整帧并拒绝非法长度（不截断）、发布/terminal/单次回收闭环、Full/wrap/错误/超时/取消不破坏 owner；stage12 真板发 20 个标准 ARP who-has，对端抓包与本地 terminal 分别成立。host GREEN 与操作者板测均通过才交 Review。

**Investigation Facts**

Current Baseline（引用 Iteration 000 最终 Act Response 与 accepted Review，2026-10-03）：

- 已交付 RX-only 驱动：`crates/axplat-riscv64-k3/src/dma.rs`（arena 常量/checked 构造/addr width/CBO 编码）、`kernel/src/platform/k3_gmac/{regs,desc,mod}.rs`（寄存器表、RX descriptor 编解码、`Gmac::new/init_rx/poll_rx`、`ArenaGuard` 发布点保留语义、`GmacEnv` seam、bounded_stop/revoke/w1c_status）、`kernel/src/platform/k3_network_probe.rs`（stage11 gate/mask/30s RX 窗）、`tests/k3-gmac-host-harness.rs`（FakeEnv + 14 测试，`#[path]` 直连生产模块）。
- V1 host 14 passed/0 failed、exit0；V2 device-irq 17 passed、`make k3/build/lichee` exit0（phy/mac/aia 生产表面未动，采信不重跑）。
- 板测已裁决事实：Zicbom 实际执行与字节可见性（V3 真实 ARP 逐字节一致）；STAT1000 bits11/10 = 对端 1000 能力位（1000FD 解码成立）；RX tail doorbell 写"本次 refill slot 自身地址"设备持续取帧（8 帧零 drop）；link-down ≤5s 有界失败。TX doorbell 语义尚未经板测裁决。
- 偏差基线：模块实际落位 `platform/k3_gmac/`（k3 构建排除 drivers 树，`kernel/src/lib.rs:39`）；A5 两项操作者确认的豁免仅限 000 轮，不延续。
- 000 Review 非阻塞 finding 3（MAC 地址寄存器内容映射沿 design D3/R69，与通用 DWMAC4 资料相反）：stage12 源 MAC 由软件构建写入帧内、应答经 PR 接收，不受影响；该注记留给依赖名义 MAC 的后续场景。

Current-State Evidence 与 Code and Critical Path（新表面）：

| 入口/符号 | 实际边界 |
|---|---|
| `k3_gmac::Gmac`（mod.rs） | 状态机 Uninit/Running/Stopped/Faulted；`cancel_requested` 检查点模式（帧交付后、OWN 发布前）；TX ring 已在 init_rx 清零并写 base(+0x1110/14 高先低后)与 ringlen(+0x112c=15)，但 ST=0、TE=0、TX tail(+0x1120) 从未写 |
| `dma.rs` 布局 | TX ring 1 页/16 slot、TX buffers 8 页/2048B，`tx_desc_pa/va`、`tx_buf_pa/va`、`check_cache_range` 已就绪 |
| `desc.rs` | 仅 RX 编解码；TX 需新增 TDES 常量（见 2.1 契约） |
| `regs.rs` | `DMA_TX_CTL`(0x1104，已写 OSP|PBL8)、`DMA_TX_DESC_HL/LL`、`DMA_TX_RINGLEN`、`DMA_CHAN_STATUS` TI(0)/TBU(2) 已有；需新增 `DMA_TX_TAIL`(0x1120) |
| `platform/k3.rs::run_first_byte_park` | 消费 stage10 返回值 → `run_k3_stage11` → park；stage12 在此链接 stage11 返回值 |
| R69 同板参考（Apache-2.0，按来源采信） | `k3_gmac/desc.rs`：TDES2 = len[13:0]\|IOC(31)，TDES3 = OWN(31)\|FD(29)\|LD(28)\|ES(15)\|len[14:0]\|CIC[17:16]，prepare 顺序 addr→des2→release fence→des3\|OWN 最后；`core.rs`：submit 后写 TX tail(`dma_chan_tx_end`=0x1100+0x20=0x1120) 并重写 ST 作额外 doorbell（TBU 后恢复 fetch）；初始化不写 TX tail（对照 U-Boot eqos_start）；负面样本：`len.min(BUFFER_SIZE)` 截断不采用、IOC 本轮不启用（轮询） |
| design D2/D4 | TX 发布顺序（写完整 buffer 补零→fence/clean buffer→des 字段→release fence→OWN 最后→clean 64B desc→fence iorw,iorw→tail doorbell/ST）；TX 接口 14..1514、14..59 补零至 60、FCS 硬件生成；submit/doorbell 10ms、TX terminal 每次提交 1s、reclaim ≤16 项/10ms、empty 是 idle |

**Implementation Guidance**

按 2.1→2.2 推进；每 task 先在既有 harness 内建 RED（fake 完成由测试写 TDES3，测试调用真实 try_submit/poll_tx/run_stage12），再实现 GREEN。TX 复用 000 的检查点/bounded_stop/ArenaGuard 模式，不新建第二套 owner 机制；局部命名与辅助拆分由 Act 决定。

**Task Contracts**

### 2.1: TX 发布、terminal、reclaim 和边界

- Requirement/Scenario: R1/R3/R5；S1/S3/S4/S5/S8/S9/S10。
- Depends on: Iteration 000 accepted（已满足）。
- Targets: `kernel/src/platform/k3_gmac/{mod,desc,regs}.rs`（`Gmac::try_submit/poll_tx/enable_tx`、TX slot 状态与 TDES 常量）；`tests/k3-gmac-host-harness.rs` 扩展。
- Current behavior: TX ring 空且 TE/ST 关闭；无 TX 数据面接口。
- Required behavior:
  - TDES（64B 同构）：des0/1 = buffer PA 低/高 32；des2 = len[13:0]，IOC bit31=0；des3 = len[14:0]|FD(29)|LD(28)，OWN(31) 最后写；不启用 CIC。
  - `try_submit(frame)` 返回 Submitted/Full/LengthInvalid/Fault：仅接受 14..=1514 完整 Ethernet frame；<14 或 >1514 拒绝并报告原始长度，不截断、不试写；14..=59 在 TX buffer 内补零至 60 后发布；FCS 由硬件生成，len 字段不含 FCS。
  - 发布顺序：复制 frame（含补零）→ fence → clean 整个 buffer 写入区 → 写 des0..2 → [deadline/cancel 检查点] → release fence → OWN 最后 → fence → clean 整个 64B descriptor → fence iorw,iorw → TX tail(+0x1120) 写本次提交 slot 自身地址低 32 位 → TX_CTL 重写含 ST。
  - slot 状态 Free→Prepared→Device→Completed→Free 加 inflight 计数：Full 不改任何 slot；完成仅回收一次；poll_tx 每次 ≤16 项、reclaim 独立 10ms phase；OWN=1 为 pending，非 timeout。
  - terminal：每次提交独立 1s 窗；OWN 清零读 des3：ES(15)=1 → 本地错误（携带 raw，slot 仍单次回收）；否则 Completed。超窗 OWN=1 → TerminalTimeout{slot,raw}，owner 保留，不软件清 OWN。
  - submit 检查点前 deadline/cancel 撤销（无 doorbell/OWN，slot 回 Free）；发布后取消只结束等待、保留 owner、停止新提交（bounded_stop 同构）；FBE → Fault。状态消费沿用 w1c 掩码（TI/TBU 为摘要/容量事件）。
  - `enable_tx`：MAC_CONFIG RMW 置 TE（保留 RE/DM/PS/FES）、TX_CTL 置 ST（保留 OSP|PBL8），init 成功后调用一次。
- Preserve: RX 行为与既有 14 测试 GREEN、arena 发布后保留、cache/seam/错误语义、stage1–11 输出。
- Forbidden: 截断 len、以 OWN 清零宣称 peer 交付、软件清 OWN 回收、MAC DMA IRQ、自定义帧身份字段、异步/epoch/reset。
- Test witness: 同一 harness 先 RED：`tx_length_no_truncate`、`tx_full_wrap_no_overwrite`、`tx_complete_once`、`tx_deadlines_and_cancel`（submit 撤销/terminal 超时/发布后取消/ES/FBE）。
- GREEN condition: 16 满 + 第 17 次 Full 不改 slot；完成后容量恢复；>16 次提交 wrap 后逐次恰好一次回收；全部 0 failed/exit0。
- Verification: V1；2.2 板测裁决真实 TX。
- Stop when: TDES/寄存器事实与已采信资料矛盾，或需修改已验收 RX/arena/错误契约。

### 2.2: stage12 原始双向帧与对端交付

- Requirement/Scenario: R2/R3/R5/R6；S2/S3/S4/S7/S10/S11。
- Depends on: 2.1。
- Targets: `kernel/src/platform/k3_network_probe.rs`（stage12 序列）、`kernel/src/platform/k3.rs`（stage11→12 链接）、同一 harness。
- Current behavior: stage11 任一 outcome 后 park。
- Required behavior:
  - gate：仅 stage11 `WindowEnded` 进入 stage12；其余 outcome 保持 park（stage12 零 GMAC 访问）。
  - stage12 在新分配的第二个 18 页 arena 上完整有界 init（复用 `Gmac::new`+`init_rx`；APMU RMW/SWR/MDIO 幂等有界；第一个 arena 按发布后保留语义不释放）。init 成功 → `enable_tx` → 发送 20 个标准 ARP who-has 192.168.137.1（软件构建：dst ff×6、src 02:00:00:00:26:01、ethertype 0806、htype1/ptype0800/hlen6/plen4/oper1、sha=src、spa 192.168.137.2、tha 0×6、tpa 192.168.137.1；42B 补零至 60），间隔 100ms，每包独立 ≤1s terminal，发送期间持续 poll_rx。
  - 发送后 10s 有界 RX 窗：继续 poll_rx；ARP 应答（oper=2）样本打印 raw+前 60B；打印上限沿 stage11（帧样本 8 行 + summary）。
  - summary 分别报告 local terminals、tx 错误、rx frames、是否见到 arp-reply；不以本地 terminal 宣称对端交付。fault/deadline 有界结束并保留 backing，随后 park。
  - 操作者板测：Wireshark 观察 20 个完整 who-has（含 slot 复用 ≥4）；至少 1 条真实 is-at 应答；reset 恢复原系统一句结论（本轮不预设豁免）。
- Preserve: stage1–11 行为、单前台 owner、masks 保持、RAM-only、旧平台构建。
- Forbidden: 自定义探测协议/帧身份字段、pcap 判定工具、以发送返回值替代 peer 抓包、进入 stage13/协议栈。
- Test witness: 先 RED `raw_tx_sequence_requires_rx_and_bounds_wait`：直接调用生产 run_stage12，fake 脚本化 terminal/RX；断言 gate、20 次间隔/terminal 有界、≥16 slot 覆盖、10s RX 窗有界、backing 保留。
- GREEN condition: host 全过；板上 20 帧 peer 完整、跨 16 slot、≥1 真实 reply、无本地 error、reset 恢复。
- Verification: V1/V2/V3（下述）。
- Stop when: 板上 terminal 不推进（doorbell 语义与板不符）、peer 帧缺席或需新数据帧格式/恢复策略；保留现场交 Plan，不试写替代语义。

**Invariants**

单 hart 唯一前台 owner；TX terminal ≠ peer delivery；OWN 不靠软件取消/timeout 清除；完成仅回收一次；Full 不覆盖设备 owner；所有等待/扫描有界；发布后 backing 不 Drop；W1C 只清已消费位；新能力仅 K3 cfg；TDD/三次失败/验证边界生效。

**Non-goals**

协议栈与四协议（002）、异步 wake、运行时恢复、SMP、offload/零拷贝、性能门槛、shell/存储、MAC DMA IRQ、身份型证据。不重做已验收 stage1–11。

**Acceptance**

| Acceptance | Requirement/Scenario/Task | 直接通过条件 |
|---|---|---|
| A1 TX 发布/terminal/reclaim 契约 | R3；S1/S3/S4/S5/S8/S9；2.1 | 长度拒绝不截断、补零、Full 不覆盖、单次回收、wrap、1s terminal、deadline/cancel owner 保留全部 host GREEN |
| A2 stage12 真板原始双向 | R2/R3；S2/S3；2.2 | 20 帧 peer 完整抓包、跨 16 slot、≥1 真实 ARP reply、local 与 peer 分列报告 |
| A3 错误与 owner 安全 | R5；S8/S9/S10；2.1/2.2 | submit 前/后取消、terminal 超时、ES 本地错误、FBE 测试 GREEN；板上无本地 error |
| A4 入口与兼容 | R6；S7/S11；2.2 | stage12 gate 次序、stage1–11 保持、受影响 harness/构建 GREEN |
| A5 RAM boot/reset 验收边界 | R6；S10；2.2 | 操作者板测与 reset 一句结论；无数据不计完成 |

**Verification**

V1：扩展同一 harness（先 RED 后 GREEN）：

~~~sh
rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test
/tmp/k3-gmac-host-test
~~~

判据：编译 exit0、`test result: ok / 0 failed`、exit0；断言覆盖 A1/A3/A4 的 host 部分。CBO 实际执行由 V3 板测裁决，host fake 只证明次序。

V2：k3.rs/探针生产表面变化后：`rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 并运行，0 failed；`make k3` exit0。phy/mac/aia 表面未变时采信 000 V2 基线；共用 cfg/平台导出变化才 `make build`、`make lichee` 各一次。既有 VF2/ms04 失败只报告。

V3（操作者，主机 GREEN 后能力边界）：

1. 沿 000 V3 相同 RAM boot 与直连拓扑（PC 192.168.137.1/24，Wireshark 过滤 arp）。
2. stage1–11 正常运行、stage11 窗口结束后自动进入 stage12 发送阶段；无需 PC 侧操作（PC 自动应答）。
3. 确认 Wireshark 20 个完整 who-has 192.168.137.1（源 02:00:00:00:26:01），描述符 slot 复用 ≥4（ring wrap）；至少 1 条 PC is-at 应答；串口 summary terminals=20、tx 错误=0、arp-reply=1。各观察一句话记入 Act Response。
4. reset 后原系统恢复，一句实际结论。此操作结束本轮，不进入 stage13。

V4：无新增 link-down 轮——S7 已由 000 V4 覆盖（link-down 时 stage11 init 有界失败即 park，stage12 不可达）。

**Gate 2 Readiness**

PASS — 2026-10-03 用户计划审计原话“批准”。调查闭合（000 accepted 基线 + R69 TX 事实）、契约自包含、验证为最简直接判定；无 BLOCKED/WAIVED、无 Missing/Simplified、无待 Act 决定的实质未知项。Plan Context 离开 draft 改为 ready；下一边界为 Act 指令。

**Persisted Evidence**

- Mode: none

Act Response 足以承载 terminal/raw 状态/样本与人工结论；板测输出超过 20 行时按用户明确要求另存（沿 000 先例与白名单第一条），不预创建目录。

**Risks and Notes**

- TX tail doorbell 语义未经板测：契约采用与已裁决 RX 同构的"本次提交 slot 自身地址 + ST 重写"（R69 用 next-slot 作对照）。若板上 terminal 不推进，表现为有界 `TerminalTimeout{raw}`，Stop when 触发交 Plan 裁决，Act 不试写替代语义。
- MAC 地址寄存器内容映射注记（000 Review finding 3）：stage12 源 MAC 由软件构建写入帧内，不受寄存器映射影响；继续留给依赖名义 MAC 的后续场景。
- PC 对重复 who-has 可能不逐条应答；验收只要求至少 1 条真实 is-at + 20 帧 peer 完整对应。
- 非实质选择（fake 完成脚本形态、打印行文案、辅助函数拆分）留给 Act 并记录。
- 000 Review 的 Minor 1/2/4（revocations 死字段、FBE 整字写回、BMSR 注释措辞）可在本轮顺带收敛，不构成本轮义务。

## Act Response

- Status: blocked

**Tasks and actual changes**

2.1 TX 发布、terminal、reclaim 和边界 — done（host 部分）。

- `kernel/src/platform/k3_gmac/regs.rs`（新常量）：`DMA_TX_TAIL`(0x1120)。
- `kernel/src/platform/k3_gmac/desc.rs`（新常量）：TDES2 `IOC`/`LEN_MASK`、TDES3 `OWN`/`FD`/`LD`/`ES`/`LEN_MASK`（R69 位图；CIC 不启用）。
- `kernel/src/platform/k3_gmac/mod.rs`：`Phase::Submit`；`TX_TERMINAL_NANOS`(1s)、`TX_FRAME_MIN/MAX`(14/1514)、`TX_PAD_TO`(60)、`STATUS_CONSUMED_MASK`；`TxSubmit{Submitted{slot},Full}`、`TxSubmitError{LengthInvalid{len},Driver(GmacError)}`、`TxPoll{Pending,Completed,LocalError,TerminalTimeout,Idle}`、私有 `TxSlotState{Free,Device,TimedOut}`；`Gmac` 增加 `tx_head/tx_tail/tx_slots/tx_deadlines/tx_inflight`；`enable_tx`（MAC_CONFIG RMW 置 TE 保留 RE/DM/PS/FES、TX_CTL 置 ST 保留 OSP|PBL8）；`try_submit`（状态 gate → FBE 检查/w1c 消费 → Full 零接触 → 长度拒绝不试写 → 复制补零至 60 → fence → clean 写入区（块粒度取整）→ des0..2 → [10ms deadline/cancel 检查点] → release fence → OWN 最后 → fence → clean 64B descriptor → fence iorw → tail doorbell（本次 slot 自身地址）→ TX_CTL ST 重写）；`poll_tx`（FBE→fault、≤16 项扫描、OWN=0 单次回收并按 ES 分类、OWN=1 时 cancel→stop_tx、超独立 1s 窗→TerminalTimeout 一次且 owner 保留不软件清 OWN、TimedOut slot 后续跳过且永不复用、独立 10ms reclaim phase）；`stop_tx`（TE/ST 清除 + IRQ mask，bounded_stop 同构）；辅助 `block_round_up`/`copy_padded_to_buf`。
- 顺带收敛 000 Review Minor 2（全部 FBE 路径状态写回改为 `status & STATUS_CONSUMED_MASK`，只清已消费位）与 Minor 4（`decode_link` 注释与 10ms 重读实现对齐）；Minor 1（`revocations` 死字段）**保留不动**：2.1 Preserve 列明 stage1–11 输出，本轮不改 wire 格式。
- `tests/k3-gmac-host-harness.rs`：FakeEnv 增加 `pending_tx_complete/pending_tx_es/pending_arp_reply`，ARP 写入重构为 `fake_write_arp(spa,tpa,oper)`，TX 描述符测试辅助；新增 4 个 witness。

2.2 stage12 原始双向帧与对端交付 — done（host 部分）。

- `kernel/src/platform/k3_network_probe.rs`：`STAGE12_TX_FRAMES`(20)/`STAGE12_TX_INTERVAL_NANOS`(100ms)/`STAGE12_RX_WINDOW_NANOS`(10s)、`Stage12Summary`/`Stage12Outcome`、`stage12_arp_request`（标准 who-has 192.168.137.1 tell 192.168.137.2，软件构建，42B 补零至 60）、`is_arp_reply`、`stage12_service_rx`（发送期持续 poll_rx，reply 样本 raw+前 60B，样本帽沿 stage11）、`stage12_stop`（错误→outcome 映射）、`run_stage12`（仅 `WindowEnded` 过 gate 且零访问；第二个 18 页 arena 完整有界 init → `enable_tx` → 20 包×（Full 有界重试 + 提交后独立 ≤1s terminal 等待，每包间隔 100ms，期间持续 poll_rx）→ 10s 有界 RX 窗 → summary 分列 sent/terminals/tx-errors/tx-timeouts/not-sent/rx-frames/rx-drops/arp-reply；fault/deadline 有界结束、backing 全路径保留）；`print_frame` 增加 stage 参数（等价重构）；board glue `run_k3_stage12`（分配第二个 GlobalPage arena；第一个 arena 由其已发布 guard 保留不释放）。
- `kernel/src/platform/k3.rs`：`run_first_byte_park` 消费 `run_k3_stage11` 返回值并传入 `run_k3_stage12`（stage11→12 链接）。
- `tests/k3-gmac-host-harness.rs`：新增 `raw_tx_sequence_requires_rx_and_bounds_wait`。

**Deviations from Plan**

- slot 状态机：契约命名 Free→Prepared→Device→Completed→Free，实现为 Free/Device/TimedOut + inflight 计数——Prepared/Completed 是单次调用内瞬态（`try_submit` 仅在 OWN 发布后记 Device；`poll_tx` 同调用内回收观察到的完成），可观察行为（Full 零接触、单次回收、超时 owner 保留、TimedOut 永不复用）与契约一致。非实质状态机折叠。
- stage12 逐包打印 `tx i=<i> slot=<slot>` 状态行（20 行，无内容转储）：把"打印上限沿 stage11"解释为帧内容样本上限（reply/frame/drop 样本沿 8 行帽），slot 行是操作者判定"slot 复用 ≥4"的必要串口证据。非实质解释选择。
- 发布后 cancel 的观察点：`poll_tx` 在 Pending（设备持有中）与 Idle（无 in-flight）两处执行 stop_tx+Stopped，前者 after_publish=true、后者 false（该语义按"是否存在设备持有 slot"如实报告）；提交检查点 cancel 只撤销当次提交不停车——契约区分 pre/post publish 的两种语义，两条路径均有测试。
- `try_submit` 的 FBE 检查置于容量/长度检查之前：故障 DMA 上不得报 Full 或接受提交。契约未规定顺序，非实质。
- 000 Minor 2/4 顺带收敛、Minor 1 保留（Preserve 优先），在 Review 授权范围内。

**Self-check findings and remaining minors**

- 已修复（RED 阶段）：`tx_full_wrap_no_overwrite` 首版的第二轮回收断言与 FIFO 回收顺序矛盾（head 尚停在未完成 slot 的 Pending 上）——测试构造错误，产品逻辑按序拒绝；改为单循环 wrap 波次（complete→reap→repoll→resubmit）后 GREEN。产品逻辑无缺陷。
- 遗留 Minor：`Stage11Summary.revocations` 仍为死字段（本轮按 Preserve 保留 wire 格式）；stage12 `printed` 计数把 reply 样本计入 8 行帽（沿 stage11 arp 样本同一规则）；stage12 提交阶段 Full 重试路径的 `poll_tx` 结果被忽略（错误经由下一次 `try_submit` 的 Driver(Fault) 浮出，路径有界）。

**Verification**（V1/V2；V3 板测留操作者能力边界）

- V1：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/k3-gmac-host-test` 编译 0 error 0 warning，exit 0；`/tmp/k3-gmac-host-test` 输出 `test result: ok. 19 passed; 0 failed`，exit 0。RED 已见证：2.1 首跑 E0432（TxSubmit/TDES3/DMA_TX_TAIL 等缺失）、2.2 首跑 E0432（Stage12Outcome 缺失），随后补齐行为断言至 GREEN。14 个既有测试与 5 个新 witness 全过。
- V2：`rustc --edition=2024 --test tests/k3-device-irq-host-harness.rs -o /tmp/k3-device-irq-host-test` 编译 0 error 0 warning，`test result: ok. 17 passed; 0 failed`，exit 0。`make k3` exit 0（StarryOS_riscv64-k3.fit 生成，Default Configuration conf-starry-k3）。phy/mac/aia 生产表面未改动，按 000 V2 基线采信不重跑；本轮未触及共享 cfg/平台导出（platform/mod.rs、axplat lib.rs、Makefile 无变化），`make build`/`make lichee` 按契约不适用。
- V3（操作者执行，2026-10-04，两轮）：
  - 第 1 轮 link-down（V4 类，未留档、可复现）：stage8 30s 无 link（`no-link-observed`）→ stage11
    `err=init LinkTimeout { bmsr2: 31113 }`（0x7989，与 000 V4 签名一致）→ stage12
    `err=gate stage11-not-window-ended stop`。真板首次实证 stage12 gate 对非 WindowEnded 输入零 GMAC 访问。
  - 第 2 轮链路正常（S1000 全双工；串口原文见 Evidence `board-stage12-tx-timeout.txt`）：stage11 正常
    收窗（frames=1）→ stage12 `init … rx-running`、`tx-enabled` → 16 个提交（slot 0..15）逐一
    `timeout … raw=0xb000003c`（与 CPU 发布的 OWN|FD|LD|60 逐位相同），ring 耗尽后 i=16..19
    `full-window-expired`；summary `sent=16 terminals=0 tx-errors=0 tx-timeouts=16 not-sent=4
    rx-frames=15 rx-drops=0 arp-reply=1 window-elapsed-ms=10000`。对端证据：`arp-reply slot=0` 为真实
    is-at 应答（dst=02:00:00:00:26:01、src=00:e0:4c:60:2f:60、oper=2、spa=192.168.137.1、tpa=192.168.137.2）
    ——请求确实上线；首个 reply 在 submit i=1 之后才被 RX reap（i=0 整个 terminal 窗内每 1ms 轮询未见）。
    支持 A3 有界止损与 A4 gate；**A1 板上 terminal 分支未达成，A2 需 Plan 裁决定级**。
  - 操作者 reset 恢复结论未回报（可随下一轮板测补一句）；本轮不预设豁免。
- V4：第 1 轮板测即覆盖 link-down 分支（stage11 有界失败、stage12 不可达、gate 拒绝零访问），S7 维持
  000 V4 结论。

**Persisted Evidence**: mode none（计划）→ act-added / BLOCKED（板测命中所设 Stop when，真板现场无法低成本复现，白名单「Blocker 需要保留现场」成立）：

- `evidence/001-raw-tx/000-initial/board-stage12-tx-timeout.txt`（操作者串口原文，自 stage8 起）
- `evidence/001-raw-tx/000-initial/blocker.md`（发现位置、预期/实际、影响、候选假设、恢复条件）
- `evidence/001-raw-tx/000-initial/README.md`（来源、结论、文件与限制）

**Experience Candidates**: None。

**Unresolved / capability boundary**

- 2.1/2.2 host 部分完成且 witness 全 GREEN；板测 RX 数据面（两个 arena、两次 SWR）完全正常；TX 本地
  terminal 语义与板不符（16/16 TerminalTimeout、设备未可见回写 OWN、但对端确实收到帧）——命中 2.2
  Stop when，交 Plan 裁决，Act 未试写替代语义。

## Blocker Handoff

- 发现位置: task 2.2 板测 V3 第 2 轮（2026-10-04）；契约条款 2.2「Stop when: 板上 terminal 不推进
  （doorbell 语义与板不符）」。Gate 5：实际硬件行为与契约 terminal 语义实质分歧。
- Plan 预期 vs 实际: 预期 20 提交 terminals=20 + 对端完整抓包；实际 `sent=16 terminals=0
  tx-timeouts=16 not-sent=4 rx-frames=15 arp-reply=1`，raw 一律 `0xb000003c`（设备未可见清 OWN），
  但真实 ARP is-at 应答证明请求上线；首个 reply 迟一个提交间隔出现。
- 影响: A1 板上 terminal/reclaim 未达成（阻塞本轮 Acceptance）；A2 对端交付有强间接支持待裁决；
  A3/A4 板上行为符合契约；A5 的 reset 结论未回报。
- 已完成 / 部分完成 / 未开始: 2.1、2.2 host 部分 done；2.1/2.2 板测部分完成（RX 链路全过，TX terminal
  未达成）；Iteration 002 未开始（依赖本轮 accepted）。
- 工作区与已通过 Gate: 产品改动限于 k3_gmac 三文件、k3_network_probe.rs、k3.rs、harness；V1（19
  passed）/V2（device-irq 17 passed + make k3 exit 0）新鲜有效；无计划外修改。
- Evidence: `evidence/001-raw-tx/000-initial/`（README、board-stage12-tx-timeout.txt、blocker.md，来源
  act-added）。
- 恢复条件: Plan 审查 blocker，对照 R69 参考裁决 H1（tail doorbell 排他/包容语义——解释传输延迟与
  ≈15/16 帧）与 H2（IOC=0 可能抑制本板 DWMAC 描述符回写——解释发送成功但 OWN 不清零；R69 参考置位
  IOC），创建 replan Cycle 修订 TX 契约与验证（建议把 IOC 与 tail=next 作为可分别裁决的板测变量）；
  Act 按新契约继续当前 Iteration。同一问题本轮首败，未触发三次失败反思。

## Plan Review

- Review Result: rework-required（2026-10-04）

**Findings**

独立检查：完整阅读本轮 diff（k3.rs stage12 链接、k3_gmac/{regs,desc,mod}.rs TX 数据面、k3_network_probe.rs stage12、harness 5 个新 witness）与 act-added BLOCKED Evidence 三件；Act 声明与代码逐项一致。采信 V1（19 passed/0 failed、exit0）与 V2（device-irq 17 passed、make k3 exit0）未失效结论，未重跑；只读 git 检查确认工作树与 Act 声明一致。

1. **阻塞（A1/A2）— TX tail doorbell 语义与板不符**。板上 16/16 提交 TerminalTimeout（raw=0xb000003c 逐位等于 CPU 写入值）、对端却真实收到请求并回 is-at 应答；frame 0 的应答在 submit 1 的 doorbell 之后才被 RX 收到、后续每帧应答紧随下一次 doorbell、第 16 槽提交后 4 次提交 Full 有界放弃。分类：**Plan 错误 + 新证据**——本 Cycle 契约采用"本次提交 slot 自身地址"，偏离了 design D3 与 R69（`core.rs:212` 写 `tx_next`）的 next-slot；把 000 轮 RX own-slot 的板测裁决错误外推到 TX。三方证据闭合"tail 为排他上界（fetch 严格低于 tail 的描述符）"：① 板测时序（释放延迟一个 doorbell、≈15/16 上线、slot 15 永不被取）；② R69 同板参考；③ Linux `stmmac_main.c`（`stmmac_xmit` 置 OWN 后 `cur_tx` 递增至下一空闲项，`stmmac_flush_tx_descriptors` 写 `tail=addr(cur_tx)`；RX init tail=`buf_alloc_num`、TX init tail=0，同一排他语义）。OWN 未清零不需要 H2 解释：每 slot 只在自己的 1s 窗内被轮询，而该描述符要到下一次 doorbell 才被取走，观察必然早于回写；H2（IOC 抑制回写）保留为下一轮的判读分支而非本轮修复变量。H3（cache 可见性）排除——RX 同序列 15 帧全对。
2. **Minor（新证据）— RX own-slot doorbell 使真板 RX ring 有效容量为 15/16**：排他语义下初始 walk 后 slot 15 不可取，本轮 15 帧 RX 全落 slot 0..14 与此吻合。000 轮 Acceptance 按其验收条件仍成立（真实帧接收、refill/wrap host 语义），不构成本 Iteration 缺口；作为基线事实记录，留给 Iteration 002（stage13 持续 RX 需要 16 槽容量时）作为契约输入，不在本轮修复。
3. **Minor — `copy_padded_to_buf` 未把补零区清零**：只复制源字节，slot 复用后 14..59 帧的 pad 区残留旧帧字节（以太网 pad 内容不限、stage12 帧恰为 60B，线上无害），但契约写明"补零"。并入 rework 的 2.1-R1 一并修复并加脏 buffer 复用的 host 断言。
4. **非实质偏差均接受**：状态机折叠（Free/Device/TimedOut+inflight，可观察行为与契约一致）、逐包 `tx i= slot=` 状态行（slot 复用判读的必要串口证据）、cancel 观察点与 FBE 先于容量检查（故障 DMA 不报 Full）、Full 重试路径 poll_tx 结果经由下次 try_submit 浮出（路径有界）。000 Review Minor 2/4 收敛已核实（`STATUS_CONSUMED_MASK` 于全部 FBE 路径；注释改为 10ms re-read）；Minor 1 按 Preserve 保留，合理。
5. Act 的 Blocker 处置符合契约：Stop when 命中即停、未试写替代语义、BLOCKED Evidence 白名单成立（一次性真板现场、README/日志/blocker 三件完整）。

**Acceptance Gaps**

- A1：host 契约分支全 GREEN；板上 terminal/reclaim 分支未达成（0 观测）——本 Iteration 未完成。
- A2：对端有强间接支持（真实 is-at 应答证明请求上线），但 20 帧 peer 完整抓包未采集、local/peer 分列结论依赖修复后重跑——待 rework 裁决。
- A5：操作者 reset 恢复结论未回报（不预设豁免）。
- A3/A4：板上行为符合契约（有界止损、gate 零访问、两次 SWR 后 RX 正常）。

**Evidence**

- 代码与 harness 独立阅读核对（上文）；V1/V2 采信 Act Response（来源注明，未重跑）。
- `evidence/001-raw-tx/000-initial/{README.md,board-stage12-tx-timeout.txt,blocker.md}`：act-added/BLOCKED 白名单成立，内容与 Act 声明逐项一致（16×timeout raw、reply 时序、is-at 帧内容、summary 行）。
- 外部交叉：R69 `k3_gmac/core.rs` submit_tx；Linux `stmmac_main.c` `stmmac_flush_tx_descriptors`/`stmmac_xmit`/init tail 用法。

**Follow-up Decision**

A1/A2 为本 Iteration 既有 Acceptance，修复需要修订 Task Contract 的 doorbell 语义（原契约明确写 own-slot，Act 无法在现契约下修复）并补充 Current-State Evidence——需要新的自包含执行契约，且无需改变目标、范围、requirement、设计（design D3 本就是 next-slot）或验收边界，Iteration Plan 不变。按 Review 分类创建同目录 rework Cycle；不采用双变量（tail=next + IOC=1 同改）以保持单变量板测裁决，H2 作为下一轮结果判读分支。

**Iteration Plan Update**

None（Map 不变；repair item 不新增全局 task）。

**Next Cycle**

`iterations/001-raw-tx/001-rework.md`（本轮创建；repair items 2.1-R1/2.2-R1，Plan Context draft 等待用户 Gate 2 审计）。

**Next Iteration**

None（当前 Iteration 未完成，002 不展开）。
