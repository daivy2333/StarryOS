## Plan status

- Phase: Iteration 000–002 全部完成（000/001 于 2026-10-03/04、002 于 2026-10-06 accepted）；7/7 tasks 闭合，change 待收尾。
- Gate 1: PASS，用户原话“那就批准当前的工作，开始计划吧”。R1–R6、S1–S11全部保留。
- Gate 2: PASS（000 Cycle）；PASS（001 Cycle 000 与 001-rework）；PASS（002 Cycle，2026-10-04 用户计划审计原话“批准”）。
- Current Iteration: `002-protocol-exchange`（已终结）。
- Current Cycle: `iterations/002-protocol-exchange/000-initial.md`（Act reported 2026-10-06，Plan Review accepted 同日）；无后继 Cycle/Iteration。
- Persisted Evidence: 计划为 `none`；已存证据见 `evidence/{000-initial,001-raw-tx}/` 对应 README（用户要求与 BLOCKED 白名单）。
- tasks 1.1–3.2 全部 done（Iteration 000/001/002 accepted）。

## Iteration Plan

### Iteration 000: DMA 可见性与原始 RX

- Tasks: 1.1, 1.2, 1.3。
- Depends on: None；使用已验收MS09/MS10。
- Stable baseline: 线性PA/DMA、高位地址和cache顺序闭合，controller有界初始化，真实ARP帧RX/reap/refill成立。
- Verification boundary: 生产逻辑host测试、受影响构建、stage11真实帧与peer抓包一致；link-down启动有界失败和reset恢复。
- Diagnostic boundary: 页布局/地址/cache→reset/link/MAC/MTL→RX descriptor/buffer；不混入TX或协议栈。
- Non-goals: TX运行、smoltcp、MAC IRQ async、runtime recovery、SMP。
- Balance: DMA与RX共同产生首个硬件成果，不能以只写寄存器或只测试fake为独立成果；TX/协议各有不同故障域，另行拆分。

- [x] 1.1 DMA arena、地址与可见性。（Iteration 000 accepted）
- [x] 1.2 有界初始化、RX与失败owner。（Iteration 000 accepted）
- [x] 1.3 K3 stage11接入与原始接收验收。（Iteration 000 accepted；V3/V4 板测通过，A5 两项确认按用户豁免）

### Iteration 001: 原始 TX、终态与单次回收

- Tasks: 2.1, 2.2。
- Depends on: Iteration000 accepted。
- Stable baseline: raw双向ARP、TX Full/wrap与错误/超时/取消owner边界，local terminal与peer delivery分别成立。
- Verification boundary: TX生产逻辑host测试、stage12的20帧跨16slot与对端完整抓包、RX应答及reset恢复。
- Diagnostic boundary: buffer→descriptor发布→tail/ST→terminal→reclaim，不调查上层协议。
- Non-goals: smoltcp应用协议、异步、reset恢复、SMP、性能门槛。
- Balance: 发布与回收是同一个TX owner环；不按submit/reclaim拆碎，不承载协议适配。

- [x] 2.1 TX发布、terminal、reclaim和边界。（Iteration 001 accepted；001-rework 修复 doorbell 排他语义后板上 20/20 terminal）
- [x] 2.2 stage12原始双向帧与对端交付。（Iteration 001 accepted；A2 peer 完整性按用户 Wireshark 豁免，A5 reset 已确认）

### Iteration 002: ARP/ICMP/UDP/TCP 轮询交换

- Tasks: 3.1, 3.2。
- Depends on: Iteration001 accepted。
- Stable baseline: 物理peer四类协议完成，数据与抓包一致，原始DMA与协议完成能分别诊断。
- Verification boundary: 生产Device/session的Cargo host测试、K3构建、真实ARP/ICMP/UDP/TCP和关闭、reset。
- Diagnostic boundary: Device token预留/背压→smoltcp polling→有界socket状态/peer；硬件基线复用前轮accepted结果。
- Non-goals: async任务/wake、交互shell、存储、offload、运行时恢复、多hart/soak/性能。
- Balance: 四类协议复用同一Device与poll owner，分成四轮会重复接入和板测；与raw队列分开则能隔离协议和DMA故障。

- [x] 3.1 K3 smoltcp Device与有界session。（Iteration 002 accepted；host 17 测试 + V2 构建全绿）
- [x] 3.2 stage13四协议真板验收。（Iteration 002 accepted；真板四协议交换 + reset 成立，ping 形态按用户原话“接受”豁免）

只展开Iteration000。每个任务唯一归属；后轮初始Cycle在上一轮accepted后，基于实际新增接口和未失效结论形成自包含契约，不预创建目录。

## Task map and contracts

首轮契约的执行权威为当前Cycle；本表用于全change范围、依赖和后轮规划，不给Act添加额外指令。

### 1.1 DMA arena、地址与可见性

- Status: done（Iteration 000 accepted；实际落位 `kernel/src/platform/k3_gmac/` 与 `crates/axplat-riscv64-k3/src/dma.rs`）。
- Requirement/Scenario: R1/R5/R6；S1/S5/S8/S9/S10/S11。
- Depends on: None。
- Targets: 新增K3平台 `dma.rs`、导出 `lib.rs`；新 `kernel/src/drivers/k3_gmac/{mod,desc,regs}.rs` 中arena/descriptor；`tests/k3-gmac-host-harness.rs`。
- Current behavior: 无K3 DMA arena/cache封装/descriptor；已有连续页分配及线性内存映射。
- Required behavior: checked18页arena、16slot/64byte descriptor/2048byte buffer、高位PA/宽度、cache独立同步；发布前可释放，发布后持有至外部复位，错误保留owner。
- Preserve: 既有安全RAM、MMIO与allocator；实际CBO只用于K3。
- Forbidden: 新内存映射/PMA/IOMMU框架、身份型证据、未证明stop即Drop。
- Test witness: 新harness直接生产模块，先RED `dma_span_and_cache_order`、地址边界/overflow/隔离/retention测试；原生rustc测试命令见当前Cycle。
- GREEN condition: 地址、范围、cache trace与保留测试0 failed；K3 CBO编译通过。
- Verification: host GREEN后进入1.2/1.3真实RX，host不宣称DMA硬件已通过。
- Stop when: 板级cache/直接地址依据冲突或分配器无法提供指定连续安全页。

### 1.2 有界初始化、RX 与失败 owner

- Status: done（Iteration 000 accepted）。
- Requirement/Scenario: R1/R2/R5；S1/S2/S4/S5/S7/S8/S9/S10。
- Depends on: 1.1。
- Targets: `k3_gmac::Gmac::init_rx/poll_rx`、局部IO seam、状态/错误；复用 `phy_probe::mdio_read`，不改旧探针。
- Current behavior: 已有clock/PHY/link，MAC尚无DMA队列。
- Required behavior: D3的reset/link、delay/MTL/bus/ring/MAC顺序；16slot RX状态/长度/drop/refill/wrap，fatal/deadline/取消有界止损，TX关闭。
- Preserve: MDIO10ms与已验收单次PHY reset；设备owner不改写；RBU是容量事件。
- Forbidden: reset超时继续、静态速率回退、硬件危险注错、开启MAC DMA IRQ、TX数据面。
- Test witness: RED `reset_timeout_no_start`、`rx_publish_and_reap`、`rx_status_and_length_edges`、`rx_full_wrap_resume`、`phase_deadlines_preserve_owner`、`cancel_before_after_publish`。
- GREEN condition: 正常、异常和owner测试0 failed，reset失败无ring/start写入。
- Verification: 当前Cycle的生产harness，再由1.3板测闭合真实RX。
- Stop when: 所需寄存器/owner行为与已采信资料矛盾，或需要改变错误/取消契约。

### 1.3 K3 stage11 接入与原始接收验收

- Status: done（Iteration 000 accepted；V3 真实 ARP RX / V4 link-down 板测证据）。
- Requirement/Scenario: R1/R2/R5/R6；S1/S2/S7/S10/S11。
- Depends on: 1.2。
- Targets: `platform/k3.rs::run_first_byte_park`、`platform/mod.rs`、新 `k3_network_probe.rs`、`drivers/mod.rs`、host harness与Makefile host-test登记。
- Current behavior: stage10后park，routes/handler仍存在；前台不得并行使用MDIO。
- Required behavior: 只有成功完成旧阶段才mask60/133/277并进入stage11，RX窗30秒，输出实际descriptor/status/ARP字段，原始窗口后持有backing并park；板上观察及reset由操作者执行。
- Preserve: stage1–10功能/输出、QEMU/D1/VF2选择、RAM-only。
- Forbidden: 新shell/串口输入/peer验证器、全局状态同步、stage11未成功就进后轮。
- Test witness: RED `stage11_gate_and_masks_before_mdio`、`stage11_bounded_rx_observation`，既有受影响harness GREEN为兼容见证。
- GREEN condition: 目标harness和受影响构建通过；人工板测通过后才算Iteration accepted。
- Verification: 当前Cycle V1–V4；主机GREEN后等待操作者板测，是能力边界而非可执行等待任务。
- Stop when: 无物理结果或任一前置失败；不宣称whole RX验收通过。

### 2.1 TX 发布、terminal、reclaim 和边界

- Status: done（Iteration 001 accepted；含 001-rework 的 doorbell next-slot 修复与 pad 真补零）。
- Requirement/Scenario: R1/R3/R5；S1/S3/S4/S5/S8/S9/S10。
- Depends on: Iteration000 accepted。
- Targets: `k3_gmac::try_submit/poll_tx`、TX slot状态、`desc.rs/regs.rs`；同一 `tests/k3-gmac-host-harness.rs`。
- Current behavior: Iteration000形成已验收RX与空TX ring，TX ST/TE关闭。
- Required behavior: 14..1514完整帧、padding至60、完整高位地址；Free/Prepared/Device/Completed/Free单次回收；Full无修改；1秒terminal、10mshandoff/reclaim，原始ES与fatal分类；取消保留设备owner。
- Preserve: RX可持续工作、arena生命周期与既有cache/seam。
- Forbidden: 截断len、以OWN清零宣称交付、异步/epoch/reset恢复。
- Test witness: 扩展harness先RED `tx_length_no_truncate`、`tx_full_wrap_no_overwrite`、`tx_complete_once`、TX各阶段deadline/取消/error测试。
- GREEN condition: full16+第17次Full不覆盖，完成恢复容量，超过16次wrap仍逐次回收；0 failed。
- Verification: rustc编译/运行同harness原生命令，受影响K3构建；既有RX结论覆盖未变则采信。
- Stop when: 需要改变原arena/发布模型或引入停机后释放语义。

### 2.2 stage12 原始双向帧与对端交付

- Status: done（Iteration 001 accepted；板上 20 帧/回绕/真实 ARP 应答，Wireshark 按用户豁免）。
- Requirement/Scenario: R2/R3/R5/R6；S2/S3/S4/S7/S10/S11。
- Depends on: 2.1。
- Targets: `k3_network_probe.rs` rawTX阶段、K3前台顺序；同一harness。
- Current behavior: stage11 RX窗口后park。
- Required behavior: 只有RX硬件基线已accepted才接入stage12；发20个标准ARP who-has192.168.137.1、间隔100ms、单包terminal≤1秒，继续poll RX；串口报告local terminal/ARP应答，peer原生抓包确认完整内容和ring wrap；最后保留arena、reset回原系统。
- Preserve: 同一foreground owner、DMA IRQ保持masked。
- Forbidden: 自定义探测协议/帧身份字段、pcap判定工具、固定性能门槛。
- Test witness: RED `raw_tx_sequence_requires_rx_and_bounds_wait`，host直接调用生产sequence，fake仅模拟完成/时间。
- GREEN condition: host全部通过；20个板发ARP中至少跨16slot的提交/完成及对应peer完整帧、真实ARP reply可见，无本地error。
- Verification: `make k3`；沿本轮相同RAM boot和Wireshark直接观察；reset一句实际结果。不以发送函数返回值替代peer delivery。
- Stop when: terminal/peer任一缺席、意外错误或需要新数据帧格式/恢复策略。

### 3.1 K3 smoltcp Device 与有界 session

- Status: done（Iteration 002 accepted 2026-10-06）。
- Requirement/Scenario: R1/R3/R4/R5/R6；S4/S5/S6/S7/S8/S9/S10/S11。
- Depends on: Iteration001 accepted。
- Targets: 新 `kernel/src/drivers/k3_gmac/polling_net.rs`；kernel可选smoltcp依赖/K3 feature、必要Cargo.lock；新 `tests/k3-polling-net-host/` 原生Cargo integration package及root workspace exclude。
- Current behavior: 已有raw双向基线；K3没有网络栈owner。
- Required behavior: D5最小features、Device token预留和Full背压、软件checksum/MTU、TX错误前台消费；静态socket/UDP TCP echo1024byte上限、partial send/recv有界累计，等待/取消/close5秒。
- Preserve: smoltcp协议实现与其他平台依赖选择，不启动axnet async owner。
- Forbidden: 新协议栈、test-seeds产品feature、第二executor、无限socket队列或scope外API。
- Test witness: `cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml`先RED生产Device token与session测试；真实smoltcp内存peer覆盖ARP/ICMP/UDP/TCP、满队列、partial、timeout/cancel/close，不使用替代session算法。
- GREEN condition: Cargo测试0 failed+exit0，正确buffer内容和协议关闭；Full期间不丢合法owner，fault不伪成功。
- Verification: 原生Cargo test、`make k3`，共用Cargo cfg变更后一次default/D1构建。
- Stop when: smoltcp本地接口无法表达已批准行为，或需要扩展协议/异步/恢复范围。

### 3.2 stage13 四协议真板验收

- Status: done（Iteration 002 accepted 2026-10-06；真板四协议 + reset 成立，ping 形态按用户豁免）。
- Requirement/Scenario: R4/R5/R6；S4/S6/S7/S9/S10/S11。
- Depends on: 3.1。
- Targets: `k3_network_probe.rs`协议session与输出；协议host integration tests。
- Current behavior: stage12后park，只有raw帧结果。
- Required behavior: raw双向accepted后运行60秒session窗；PC ping20、UDP26001 `ms11-udp` echo、TCP26002 `ms11-tcp` echo和正常close；ARP/ICMP/UDP/TCP双向packet与板内容分别成立；失败明确阶段、owner，最终复位恢复原系统。
- Preserve: RAM-only、单hart、backing保留、旧平台。
- Forbidden: 持久网络配置/固件写入、只用栈host测试宣称真板结果、吞吐/soak新门槛。
- Test witness: 原生Cargo测试先RED协议阶段gate及failure/close输出；不是验证工具测试。
- GREEN condition: host/build通过且操作者真实四协议和reset结果满足R4/R6；partial或未执行人工项不得计入完成。
- Verification: 原生Wireshark ARP/ICMP及UDP/TCP payload/FIN观察，Windows ping退出0和echo相同内容；操作步骤在后轮Cycle展开时自包含，设计D5已确定语义与拓扑。
- Stop when: 任一协议缺失、链路或owner错误、需要用户改变拓扑/验收范围。

## Requirements Traceability Matrix

Covered表示计划追踪完整，不表示实现或验收已完成。无Simplified/Missing。

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 地址与cache/owner | 1.1,1.2,1.3,2.1,3.1 | dma_span_and_cache_order、rx_publish_and_reap、tx_complete_once、实际RX/TX帧 | Covered |
| R2 bounded RX | 1.2,1.3,2.2 | rx_status_and_length_edges、rx_full_wrap_resume、stage11真实ARP | Covered |
| R3 bounded TX | 2.1,2.2,3.1 | tx_length_no_truncate、tx_full_wrap_no_overwrite、tx_complete_once、stage12 peer帧 | Covered |
| R4 四协议与抓包 | 3.1,3.2 | polling_protocol_roundtrip、真实ARP/ICMP/UDP/TCP与peer内容 | Covered |
| R5 错误/deadline/cancel | 1.1,1.2,2.1,3.1,3.2 | reset_timeout_no_start、fatal_keeps_backing、phase_deadlines_preserve_owner、cancel_before_after_publish、session timeout/close | Covered |
| R6 临时启动与兼容 | 1.3,2.2,3.1,3.2 | stage11_gate_and_masks_before_mdio、旧harness/构建、stage1–10/reset原系统 | Covered |

场景→设计→任务→目标代码→直接验证的链路分别见proposal Scenario Sketch、design D1–D5/场景表、上列Targets及当前Cycle。后轮不能靠本Map直接实施，必须展开其自包含Cycle。

## Current authorized batch and boundaries

Iteration 000 已 accepted（2026-10-03）：host V1/V2 GREEN + V3/V4 板测通过，A5 两项操作者确认按用户原话豁免。Iteration 001 Cycle 已展开为 draft，等待用户 Gate 2 审计；批准后下一 Act 批次为 2.1→2.2 的主机实现/验证，主机 GREEN 后到操作者板测能力边界（stage12 + reset）。板测输出提交后由 Plan 核对，PASS 且 Review accepted 才展开 Iteration 002；无数据不推进，错误保留当前工作。上轮豁免不延续到本轮。

| Phase / Step | Status | 说明 |
|---|---|---|
| 需求/BDD与Gate1 | completed | 六项需求、十一场景批准，无豁免 |
| 实现调查/基线复用 | completed | design与首轮Cycle记录实际入口/调用链/代码/负面经验 |
| 设计/任务契约/RTM/平衡审计 | completed | 七任务唯一分配到三轮 |
| Iteration 000 实施/板测/Review | completed | 2026-10-03 accepted；V1/V2 GREEN、V3/V4 板测、豁免与偏差记录于 000 Cycle |
| Iteration 001 计划（本轮展开） | completed | `iterations/001-raw-tx/000-initial.md`，Plan Context ready |
| 用户审计 Iteration 001 Cycle / Gate 2 | completed | 2026-10-03 用户原话“批准”；Gate 2 PASS，无豁免 |
| 产品实施（001 2.1→2.2 host）、V1/V2 | completed | 2026-10-04 Act blocked：V1 19 passed、V2 通过；板测命中 2.2 Stop when |
| Plan Review（001 Cycle 000） | completed | rework-required；根因为 Plan 契约 doorbell 语义错误（三方证据闭合），A1 板上/A2/A5 缺口登记 |
| rework Cycle 创建（本轮） | completed | `iterations/001-raw-tx/001-rework.md`（2.1-R1/2.2-R1），Plan Context ready |
| 用户审计 rework Cycle / Gate 2 | completed | 2026-10-04 用户原话“批准”；Gate 2 PASS，无豁免 |
| rework 实施（V1/V2）、板测 V3' 判读 | completed | 2026-10-04 Act reported：21 passed、V3' 判读表第 1 行成立（20/20 terminal、应答不再延迟、20 RX 零 drop） |
| Plan Review（001-rework） | completed | accepted；H1 板测双侧确认；RX 15/16 推断被新证据修正；A1/A2/A5 闭合（豁免原话留存）；Iteration 001 完成 |
| Iteration 002 计划（本轮展开） | completed | `iterations/002-protocol-exchange/000-initial.md`（3.1/3.2 + 用户并入的换行修复；Wireshark 豁免后验证重设计），Plan Context ready |
| 用户审计 Iteration 002 Cycle / Gate 2 | completed | 2026-10-04 用户原话“批准”；Gate 2 PASS，无豁免 |
| 002 实施（V1/V2）、板测 V3' | completed | 2026-10-06 Act reported：host 全绿 + 真板四协议成立；ping 形态与 reset 按用户原话豁免 |
| Plan Review（002 Cycle 000） | completed | 2026-10-06 accepted；独立代码审查与声明一致，2 项非阻塞 Minor finding 记录于 Cycle；无后继 Cycle/Iteration，change 待收尾 |
| 重跑已采信基线全量测试 | SKIPPED | 覆盖未变已有真实运行；不逐项重跑 |
| 全局状态/经验登记/归档 | SKIPPED | 当前未授权Maintainer/Recorder；公共规则阶段边界 |

## Plan validation

- 命令：`openspec validate ms11-com260-kit-polling-bidirectional-network-baseline --strict --no-interactive`；输出 `Change 'ms11-com260-kit-polling-bidirectional-network-baseline' is valid`，exit0。
- 命令：`openspec status --change ms11-com260-kit-polling-bidirectional-network-baseline`；输出 `Progress: 4/4 artifacts complete`、proposal/specs/design/tasks完成，exit0。
- 只读命令：`openspec instructions apply --change ms11-com260-kit-polling-bidirectional-network-baseline`；输出 `0/7 complete`，七个Map task全部未完成，exit0。只核对原生任务识别，不执行apply或授权后轮。
- 人工自检：R1–R6全部Covered，S1–S11有直接验证，七任务各归一轮；只展开000，Plan ready、Act pending、Review pending；无产品/全局状态/Evidence目录修改。
- 这些检查只证明计划结构与追踪，不证明网络实现或板测通过。用户已批准计划，Gate2 PASS；没有省略需求或测试场景。
