# Iteration 000 / Cycle 001: UART42 RX 诊断与参考对齐激活

## Plan Context

- Status: ready
- Cycle Type: rework

**Cycle Scope**

- Change tasks：T1、T2（Iteration 000 既有任务）。
- Acceptance gaps：A3（板上 UART42 两次真实 RX IRQ + 孤立字节）、A4 板上 B4（外部 RESET 回原系统）；连带关闭 A2 的激活序列 native 缺口与 `[ms17]` 诊断截断缺陷。
- Repair items：T2-R1（参考对齐激活序列）、T2-R2（一次性投递自检与故障定位快照）、T2-R3（板测 B3′/B4 重跑）。
- Inherited scope：R1–R8/S1–S28 与五项默认范围；Cycle 000 已交付且经验证的全部表面（timer 底座、route42 opt-in、共享 guard、feature/Make/SMP 拒绝门、raw 等待协议、latch 纪律）原样保留；B1/B2 板上结论随载荷重跑自动复现。
- Excluded scope：T3–T8、copier/tty/用户态、AIA/APLIC/IMSIC 重写、动态波特率、周期 RX 轮询、SMP、性能、MS12、全局文档与 Recorder。

**Objective**

UART42 在板上按真实输入推进：激活序列与参考生产用法对齐（startup 三步 FCR + 运行态 PXAR8 + 陈旧 RBR 排空 + TX 排空后再清 FIFO），板上任何 RX 失败都能用一次性快照定位到 UART 生成 / APLIC 交付 / 输入未达三层之一；B3′ 真实输入验收与 B4 reset 通过。

**Background**

Cycle 000 板测两次同位失败（stage=1 RX 30 s deadline，无任何 RX 推进；B1/B2 PASS）。新证据收敛嫌疑：DTS/寄存器位/路由假设逐项证实（`spacemit-k3-com260-ifx.dts` `interrupts=<0x2a 0x04>`、parent 同一 APLIC）；首要嫌疑为本项目激活后 FCR 停在触发字段 00（1B），而参考生产在 115200 用 PXAR8（0x41）。`[m` 截断根因已定位为激活 FCR clear-TX 清掉未发完的诊断行。详见父 Cycle Plan Review。

**Investigation Facts**

Current Baseline：工作区即 Cycle 000 Act 改动（timer_irq.rs、k3_uart{,_core}.rs、k3-timer/k3-uart harness、aia/console/irq/time/lib/entry/platform-k3/feature/Make 改动），未提交。已采信未失效结论（来源 Cycle 000 Act Response，本 Review 核对工作区一致）：native timer 8 / uart 16 / AIA 17+18 全 GREEN；`make k3`/`make build`/`make lichee`/`make k3-uart-port` exit0；`SMP=2` 拒绝；板 B1/B2 PASS、B3 两次失败。

Current-State Evidence 与关键路径：

- `kernel/src/drivers/k3_uart_core.rs::activate`（249–275 行）：现序列 LCR=3 → IER=UUE(masked) → FCR 0x07（EN|clrRX|clrTX，触发 00）→ MCR=0x08 → 单次 LSR 读 → head_errors 清零 → register_irq → ready。无 TX 排空、无 RBR 排空、无运行态 FCR 写。`FCR_SETUP`/`MCR_SETUP`/`LCR_SETUP` 常量在同文件 85–91 行。
- 截断机制：`console_write`→`console_putbyte_locked` 只等 THRE 即写返回（424–429 行），随后 activate 的 FCR clear-TX 清掉未移出字节。
- 参考实码（R69 `tgoskits/drivers/serial/some-serial/src/pxa_uart/`，本轮 Plan 已 grep 证实）：`mod.rs::startup()`（284–304 行）FCR=EN → EN|clrRX|clrTX → FCR=0，读 LSR 清 sticky、写 MCR、读 LCR 清 SBRK、再一轮 FCR 清+0、`drain_rx()`；`set_config`（408–415 行）写运行态 FCR=`select_fcr_flags_for(clock/divisor)` 并 IER|=UUE。`regs.rs:596`：rate < 2400×16 → PXAR1(0x00)；2400×16 ≤ rate < 230400×16 → PXAR8(0x40)；更高 → PXAR32。本模式固定 115200 8N1、divisor 沿用 bootloader handoff，不运行时计算——运行态 FCR 取常量 **0x41**，推导注释写明分档依据。
- PXAR8 语义下 7 字节输入行（`ms17-a\n`）与孤立单字节不达 8 字节 RDI 触发线，依赖 CTI（RTOIE）中断；参考生产即此形态，Errata-20"低于触发线字节丢 timeout 条件"为 drain 前后 RTOIE off→on 窗口形态（其 irq.rs 注释），本项目 receive 的 Errata-20 顺序 + ISR NO_INT+DR 兜底 + arm 后 LSR 重查已覆盖该窗口（Cycle 000 native 16 用例含 CTI/NO_INT+DR）。协议无需改动。
- 陈旧字节排空参考为 `drain_rx()`（读 RBR 至 DR=0）。本项目 activation 在 FCR=0（FIFO 关）后 DR 至多反映 1 字节 holding register，有界排空 ≤8 次读即可；排空字节不入流。
- AIA 生产入口已存在，返工直接复用不新增 MMIO 知识：`crates/axplat-riscv64-k3/src/aia.rs` 的 `APLIC_SETIP_BASE`（0x1c00，pending 位可读）、`APLIC_SETIPNUM_LE`（0x2000，软件强制 pending 写，MS10 已裁决双入口语义）、`read_eip_snapshot`（IMSIC EIP 快照）、`readback(source)`、`clear_source_pending`，`HardwareBackend` 为真实绑定；`irq.rs` register 已达 AiaChip（板上 activate=ready 证明注册成功）。
- 自检无害性：stage 自检时 RX 组未 arm、DR=0，ISR 读 IIR=NO_INT + LSR DR=0 → 无 wake、无 mask、不消费；route42 host 测试的 `set_pending(42)`+`handle_external` 与此同型。
- 载荷入口 `kernel/src/platform/k3.rs::run_uart_port_bringup`：B2 两段 timer 证明在前，B3 activate→stage1/2→isolated→counts→park。返工在 activate 与 stage1 之间插自检，并在各 RX deadline fault 分支插快照输出后 park。

**Implementation Guidance**

先为三个新行为写 native RED（FCR 序列/最终值、TX 排空先于清 FIFO、陈旧 RBR 排空），再改 `k3_uart_core.rs::activate` 与新增诊断快照方法，然后接 payload 自检/快照，最后 build 与板测。快照/自检是一次性只读观测，不进入数据面路径。

**Task Contracts**

### T2-R1：参考对齐激活序列（含 `[m` 截断修复与陈旧 RX 排空）

- Requirement/Scenario：R2/R8；S4–S5/S7–S8 的低层激活部分、S27。
- Depends on：None。
- Targets：`kernel/src/drivers/k3_uart_core.rs`（`activate`、FCR 常量、必要时 `ActivateError` 新变体）；`tests/k3-uart-host-harness.rs` 新用例。
- Current behavior：激活写一次 FCR 0x07（触发 00），不排空 TX/RBR；诊断行可被 clear-TX 截断。
- Required behavior：激活 prepare 批（单次 `with_lock`）按序：**有界等待 TX TEMT**（轮询上限覆盖 64B FIFO @115200 移出时间；超限返回新 `ActivateError::TxDrain`，不写任何 FCR、状态回 idle）→ LCR=3 → FCR=0x01 → FCR=0x07 → FCR=0x00 → 读 LSR 清 sticky → MCR=0x08 → **有界排空陈旧 RBR**（DR 驱动，≤8 次读，字节丢弃不入流）→ FCR=0x41（EN|PXAR8，注释含 115200 分档依据）→ IER=UUE（masked）。head_errors 清零、register_irq、ready 与回滚语义沿用 Cycle 000。
- Preserve：duplicate 零硬件写、失败回滚与阶段错误、IER RMW 保 UUE、latch 纪律、bootloader clock/pins/divisor、ISR/receive/send/等待协议不变量。
- Forbidden：运行时计算 divisor/波特率、动态触发线选择、激活外任何 FCR 写、清除活动期 FIFO、第二 owner。
- Test witness：新增 RED（对当前实现首跑必失败）：①激活 FCR 写序列断言（值依次 0x01、0x07、0x00，末次 0x41 且无 clear 位）；②fake 保持 TEMT=0 前 K 次时，断言 TEMT=1 的 LSR 读先于任何含 clear 位的 FCR 写，且 TEMT 恒 0 时返回 `TxDrain`、无 FCR 写、状态 idle；③fake 预置陈旧字节，断言激活后 DR=0、陈旧字节被读走且不出现在任何流。命令：`rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-r1` 后运行 `--test-threads=1`。
- GREEN condition：新用例 GREEN，既有 16 用例不回归。
- Verification：上述 harness；`make k3-uart-port` exit0。板上 `[m` 不再出现（T2-R3 观察）。
- Stop when：参考序列在真板仍无 RX 且快照指向 UART 生成层（IIR 无 cause、IER readback 异常）——交 Plan，不做变体盲试。

### T2-R2：一次性投递自检与故障定位快照

- Requirement/Scenario：R2/R8；S4/S27 的可判定性（不替代 S4 验收）。
- Depends on：T2-R1。
- Targets：`kernel/src/drivers/k3_uart_core.rs`（只读快照方法，如 `diag_snapshot` 返回 IIR/LSR/IER readback/ier_cache/counts）；`kernel/src/drivers/k3_uart.rs` 或平台侧绑定（读 APLIC SETIP 位 42 与 `read_eip_snapshot`，复用 `aia.rs` 生产入口）；`kernel/src/platform/k3.rs::run_uart_port_bringup`（自检 stage 与 fault 快照输出）。
- Current behavior：stage 失败只打一行 fault；counts 不可达；无法定位失败层。
- Required behavior：①activate 成功后、stage1 前，**一次性**写 `APLIC_SETIPNUM_LE=42` 强制 pending 自检：期望 ISR entry（irq_count 增至 ≥1，IIR=NO_INT、DR=0、无 wake），打印一行 `selftest irq=N ok|no-delivery`；等待有界（如 500 ms native timeout 轮询计数）后打印并继续，不 park。②每个 RX deadline fault 分支在 park 前打印**一次性快照**：counts（irq/rx_wakes/tx_wakes/unknown/closed）、IER readback 与 ier_cache、单次 IIR、单次 LSR（DR 位）、APLIC pending 位 42、EIP 快照。所有读取经共享 guard；一次 fault 各寄存器至多读一次。
- Preserve：快照/自检不改变数据面行为；ISR/等待协议不变；诊断输出格式属非实质选择。
- Forbidden：周期轮询 IIR/LSR/pending、把自检 IRQ 计入 B3′ 验收、在 fault 外路径反复强制 pending、为快照新增判定工具/host 协议。
- Test witness：`diag_snapshot` 的 native 用例（fake 提供寄存器值，断言快照只读一次、值正确、不 latch 不清状态）；RED 先行（当前无该 API 编译失败即为 RED）。自检与板上快照为板测观察项，无 native 伪造。
- GREEN condition：native GREEN；板上 selftest 行出现且 irq≥1（若 no-delivery，见 Verification 判读表，属诊断收获不算失败虚报）。
- Verification：板测判读表见 T2-R3；自检本身不设 GREEN 门槛为"必须 ok"——no-delivery 是定位信息，按判读表决定去向。
- Stop when：自检需要重复强制 pending 才能判定、快照需新寄存器语义——交 Plan。

### T2-R3：板测 B3′/B4 重跑

- Requirement/Scenario：R2/R8；S4–S5、S27–S28。
- Depends on：T2-R1、T2-R2。
- Targets：`make k3-uart-port` 产出的 `StarryOS_riscv64-k3.fit`；R68 操作流程。
- Current behavior：Cycle 000 镜像 B3 两次失败。
- Required behavior：新镜像按 R68 启动（FAT U 盘、`fatload usb 0:0 0x2e0000000`、`bootm 0x2e0000000`，不 saveenv）。B2 两行 timer 结果自动复现；随后 selftest 行；stage1/2 依次输入 `ms17-a`+Enter、`ms17-b`+Enter（各 30 s 内），每条看到原文本收回并回显；isolated 阶段只输入 `Z` 后停止，看到单字节返回；counts 行与 complete 行；最后 RESET 直接见 `Bianbu … k3 login:`。
- Preserve：每次 `make k3` 后重跑 `make k3-uart-port` 保证交付 FIT 为本模式镜像（沿用 Cycle 000 判据）。
- Forbidden：把 selftest/软件 pending 算作 S4 真实 RX 验收；用终端本地回显冒充 kernel 回显。
- Test witness：操作者直接输入与所见即判定；结果一句话记入 Act Response。
- GREEN condition：B3′ 三阶段真实 IRQ 推进 + B4 reset 回原系统；任何 fault 行出现即未达成。
- Verification（操作者判读表，fault 快照分支）：
  - `DR=1` 且 IIR 有 RX cause（RDI/CTI）且 pending42=1 且 irq=0 → APLIC→IMSIC→claim 交付层断裂，交 Plan（不再驱动侧盲试）。
  - `DR=1` 且 IIR 无 RX cause → UART 中断生成层（IER/FCR 语义），交 Plan 并携带快照。
  - `DR=0` → 输入未达 FIFO：先与操作者核对输入会话/线序（同一线串口、kernel 输出可见），复核后仍 DR=0 交 Plan。
  - selftest 行 `no-delivery`（强制 pending 无 ISR entry）→ 交付层断裂，同第一分支。
- Stop when：RESET 未回原系统；快照组合超出判读表；同层第二次失败即停（本 Cycle 为该 Acceptance 第 3 次尝试，再失败按三次失败规则整体回 Plan）。

**Invariants**

1. Cycle 000 Invariants 1–5 全部继续有效（复用现有服务、单 owner IRQ-save、原模式与四 route 保持、RAM-only、raw port 不与后续 copier/tty 并存）。
2. 自检与快照是一次性只读观测：不改变数据面语义、不计入真实 IRQ 验收、不形成周期轮询。
3. 触发线为固定 115200 下的常量裁决（0x41），运行时不读 divisor、不动态选择。
4. B2/timer 底座代码零改动；本轮全部产品改动限定在激活序列、诊断只读方法与载荷观测输出。

**Non-goals**

APLIC/IMSIC 侧任何改动、copier/ring/tty/用户态（Iteration 001+）、多 hart、性能、Wireshark/身份协议/判定器、经验产物与全局状态同步。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1′ | 激活序列/最终 FCR=0x41、TX 排空先于清 FIFO、陈旧 RBR 排空、TxDrain 失败路径的 native 用例 RED→GREEN；既有 16 用例不回归 | T2-R1 / R2 / S7–S8 |
| A2′ | `diag_snapshot` native GREEN；板上 selftest 行出现（ok 或 no-delivery 均为有效观测） | T2-R2 / R2 |
| A3′ | B3′ 两条输入线 + 孤立字节经真实 UART42 IRQ 推进并回显；任何失败有快照行且可按判读表定位 | T2-R3 / R2 / S4–S5 |
| A4′ | B2 复现、B4 RESET 回原系统、`make k3-uart-port` exit0 且交付 FIT 判据成立 | T2-R3 / R8 / S27–S28 |

RTM（本 Cycle）：R2 低层 → T2-R1/R2/R3（native + selftest + B3′）Covered；R8 本轮 → T2-R1/R3（build 门 + B4）Covered。R1/R3–R7 余下场景仍属 Map 后续 Iteration，不在本 Cycle 声明。

**Verification**

V1（agent，native）：T2-R1/R2 的 harness 命令，新用例先 RED（当前实现序列/快照 API 不满足）再 GREEN；`--test-threads=1`。timer harness 8 项、AIA 17/18 项表面未改，采信 Cycle 000 结论不重跑。

V2（agent，build）：`make k3-uart-port` exit0；`make k3-uart-port SMP=2` 拒绝行为未改（Makefile 零改动，采信）；`git diff --check`。`make k3`/`make build`/`make lichee` 覆盖表面零改动（本轮改动全部位于 k3-async-uart 门内模块与 harness），采信 Cycle 000。

V3（操作者，板测 B3′/B4）：按 T2-R3 步骤与判读表执行；无结果则 A3′/A4′ 保持 pending，不展开 Iteration 001。

V4（失败处理）：任一 fault/快照组合按判读表停止并交 Plan；RESET 异常立即停止装载。

**Gate 2 Readiness**

PASS：技术调查（参考实码 + 板上复现 + AIA 现成入口）、契约、追踪与验证计划闭合，无 TBD。2026-10-07 用户原话"批准"——批准本返工计划，并授权 SETIPNUM 软件强制 pending 自检与一次性快照两项诊断观测（不替代 S4 真实 RX 验收）。Status 转 ready；Act 等待明确执行指令。

**Persisted Evidence**

- Mode: none

命令、≤20 行决定性输出、exit 与操作者一句话结果足以判定；板测串口记录按需逐字入 Act Response，不建 Evidence 目录。

**Risks and Notes**

- PXAR8 后短于 8 字节的输入依赖 CTI：参考生产同型，errata 注释为 drain 窗口形态，本项目已有 native 覆盖；若真板 CTI 不触发，快照会落在"DR=1 且 IIR 无 cause"分支，交 Plan 裁决（如评估 PXAR1 是否仅在此硅 <2400 有效之外另有用途），Act 不自行变体。
- harness 全局静态计数需 reset 的既有模式延续（Cycle 000 deviation 2）。
- 自检若引发意外 retrap（level 语义边角），ISR 有界分类 + notifications_closed 止损兜底；快照行会如实显示 counts。
- 快照打印格式、helper 命名、自检等待实现（timeout 轮询计数 vs 单次延迟读）为非实质选择。
- 本 Cycle 为 A3 第 3 次尝试；判读表任一分支失败即回 Plan，不做第 4 次同类板测。

## Act Response

- Status: blocked（2026-10-07 板测 B3′ 第 3 次尝试失败且快照组合超出判读表，Gate 5 阻塞交接；上文"实际改动/验证"为 agent 部分已完成的报告内容，保留不覆盖）

### 实际改动（本 Cycle agent 部分）

T2-R1（参考对齐激活序列，含 `[m` 截断修复与陈旧 RBR 排空）——`kernel/src/drivers/k3_uart_core.rs`：

- 新常量：`FCR_FIFO_ON=0x01`、`FCR_FIFO_OFF=0x00`、`FCR_PXAR8=0x41`（注释含 115200 分档依据：PXAR1 仅 <2400 波特、PXAR8 为 2400×16≤rate<230400×16、PXAR32 更高；divisor 由 bootloader handoff 固定，运行态 FCR 取常量）、`TX_DRAIN_POLL_LIMIT=1_000_000`（覆盖 64B FIFO @115200 ≈5.6 ms 移出时间，数量级余量）、`STALE_RBR_DRAIN_LIMIT=8`。
- `ActivateError` 新变体 `TxDrain`。
- `activate()` prepare 批（单次 `with_lock`）按契约顺序重写：有界 TX TEMT 轮询（超限返回 `TxDrain`，无任何 FCR 写、状态回 idle）→ LCR=3 → FCR 0x01 → 0x07 → 0x00 → 原始 LSR 读清 sticky → MCR=0x08 → DR 驱动 ≤8 次陈旧 RBR 排空（字节丢弃不入流）→ FCR=0x41（无 clear 位）→ IER=UUE（masked，RMW 保 UUE）。head_errors 清零、register_irq、ready、duplicate 零写与失败回滚语义沿用 Cycle 000。模块文档激活序列描述同步更新。
- `[m` 截断修复机制：激活最先等待 TEMT，console 批末残留在 TX FIFO 的诊断行尾先移出，clear-TX 不再截断（native 见证 `tx_drains_before_any_fifo_clear_write`）。

T2-R2（一次性投递自检与故障定位快照）：

- `kernel/src/drivers/k3_uart_core.rs`：`DiagSnapshot`（counts+closed、IER readback、ier_cache、单次 IIR、单次 LSR 及 DR 位）与 `diag_snapshot()`——单个 `with_lock` 批，刻意绕过 latch 纪律（不 latch head errors、不计数 OE/FIFOERR、零状态写）。
- `kernel/src/drivers/k3_uart.rs`：`aia_force_pending42()`（`SETIPNUM_LE` 写 42 强制 pending，MS10 裁决的软件注入入口，不触碰 UART 寄存器）、`aia_pending42_and_eip()`（读 APLIC SETIP 位 42 + EIP0 快照，只读）。
- `kernel/src/platform/k3.rs`：activate 成功后、stage1 前一次性自检（500 ms native timeout 轮询 irq_count，打印 `selftest irq=N ok|no-delivery`，不 park，no-delivery 为判读表定位信息）；模块级 `fault_snapshot()`（cfg 同 bringup）在 stage1/2 与 isolated 两个 RX deadline fault 分支 park 前各打印一次 core 快照行 + `aplic_pending42/eip0` 行；非 RX deadline fault 分支不打印（契约范围）。

测试 harness（`tests/k3-uart-host-harness.rs`）：新增 5 用例（`activation_fcr_sequence_matches_reference_startup`、`tx_drains_before_any_fifo_clear_write`、`undrained_tx_fails_with_txdrain_and_writes_no_fcr`、`stale_rbr_bytes_are_drained_and_never_enter_the_stream`、`diag_snapshot_reads_once_and_leaves_state_untouched`）；fake 补充 `temt_after_reads`/`lsr_reads`（TEMT 延迟观测）。

### 与计划的偏差

1. fake 缺省 `lsr_level` 由 0 改为 THRE|TEMT：新激活序列首先轮询 TEMT，真实 bootloader handoff 态发射端空闲（THRE|TEMT 置位），原缺省不真实——测试基础设施修正（Cycle 000 deviation 3 同类），生产实现未因此改动；`activation_resets` 用例在 activate 前补 TEMT 置位。
2. `fault_snapshot` 定义为模块级 fn 而非闭包，避免三处重复打印体；诊断输出格式属契约明示非实质选择。

### 自检发现与修复

- Important（已修复，测试代码）：`diag_snapshot` 用例曾以 `let window = &hw.lock().log[..]` 借用临时 MutexGuard 并跨后续硬件调用存活，造成同线程自死锁（`timeout 60` 复现，输出停在 `diag_snapshot_reads_once...`）。修复为单个短锁作用域内完成窗口统计后重跑 GREEN。生产代码无此问题。
- 遗留 Minor：`TX_DRAIN_POLL_LIMIT` 是轮询次数上界而非时间上界，最坏等待时长依 CPU 而定；一次性激活路径，注释已写明推导。
- 遗留 Minor（沿用 Cycle 000，未改）：`with_uart_hardware` 非 panic-safe、激活后 panic 路径无 latch 纪律、激活前 console 轮询可能计入陈旧 OE/FIFOERR 计数。

### 验证

V1（agent，native）：

```text
rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-r1
/tmp/ms17-k3-uart-r1 --test-threads=1
test result: ok. 21 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.08s   exit=0
```

RED 见证（变更前实际运行）：补齐 harness 自身 `REG_FCR` 导入缺失后，首跑确认生产侧缺失 `ActivateError::TxDrain` 与 `K3UartCore::diag_snapshot`（E0599 ×2），exit=1。

V2（agent，build）：

```text
make k3-uart-port  → exit=0（SMP=1、LOG=off，FIT 打包 conf-starry-k3）
git diff --check   → exit=0（clean）
```

交付 FIT 判据：`strings StarryOS_riscv64-k3.bin | grep -c ms17` = 17（Cycle 000 为 14，+selftest/fault-snap 共 3 处），`stage=1 entry-reached` 计数 0，`selftest`/`fault-snap` 串在镜像内；bin/fit 均为本轮 `make k3-uart-port` 新鲜产物。

采信与不重复运行：timer harness 8 项、AIA 17+18（两 cfg）、`make k3`/`make build`/`make lichee`、`SMP=2` 拒绝均为 Cycle 000 Act Response 结论；本 Cycle 改动全部位于 k3_uart_core.rs（激活/快照）、k3_uart.rs（新增 AIA 入口）、platform/k3.rs（bringup 接线）与 harness，timer/AIA/legacy 构建覆盖表面未变化，直接采信不重跑。编译零新 warning。

### Persisted Evidence

None required（Mode: none；命令与决定性输出已录本 Response，可低成本重跑）。

### Experience Candidates

None。

### 未解决问题（能力边界，等待操作者）

- T2-R3 板测 B3′/B4 未执行：A3′/A4′ 保持 pending，`tasks.md` 不勾选。操作者按 T2-R3 契约步骤执行（新镜像已就绪：`StarryOS_riscv64-k3.fit`）；无 fault 时把一句话结果记入本 Response 并转 reported；任一判读表分支命中时补 Blocker Handoff 转 blocked。本 Cycle 为 A3 第 3 次尝试，同层第二次失败即停。
- B4（外部 RESET 回原系统）可随时执行，预期不受本轮改动影响。

### Blocker Handoff（2026-10-07 001-rework 板测 B3′，A3 第 3 次尝试）

操作者串口原始记录（两次独立启动，决策性现场逐字保留；两轮 `[ms17]` 行之前内容同构）：

```text
# Run 1
[ms17] mode=k3-uart-port-test begin uart=0xd4017000 irq=42 smp=1
[ms17] timer result=sleeps_ok expect_ms=150 elapsed_ms=164
[ms17] timer result=timeout_cancelled elapsed_ms=7 inner=30ms-sleep
[ms17] uart stage=activate irq=42
[ms17] uart activate=ready (notifications masked; tasks arm next)
[ms17] selftest irq=0 no-delivery
[ms17] uart fault stage=1 rx deadline=30s
[ms17] fault-snap stage=1 irq=99 rx_wakes=792 tx_wakes=0 unknown=0 closed=false ier_rb=0x55 ier_cache=0x55 iir=0xc1 lsr=0x0 dr=false
[ms17] fault-snap stage=1 aplic_pending42=false eip0=0x0

# Run 2（独立重启动）
[ms17] selftest irq=0 no-delivery
[ms17] uart fault stage=1 rx deadline=30s
[ms17] fault-snap stage=1 irq=25 rx_wakes=200 tx_wakes=0 unknown=0 closed=false ier_rb=0x55 ier_cache=0x55 iir=0xc1 lsr=0x0 dr=false
[ms17] fault-snap stage=1 aplic_pending42=false eip0=0x0
```

- 发现位置：Task T2-R3 / 板测 B3′ stage=1 / Gate 4（A3′）。
- Plan 预期 vs 实际：预期 selftest 出现 ISR entry、stage1 两条输入线经真实 IRQ 回显；实际 selftest no-delivery、stage1 30 s deadline，两轮同构。B4 未执行。
- 快照组合超出判读表：DR=0 分支指向"输入未达 FIFO"，但 irq=99/25 次真实 ISR entry 与 rx_wakes=792/200 并存——表内四分支均无法归位，按 T2-R3 Stop when 交 Plan。

判定事实（本轮快照直接支持）：

1. **AIA source42 交付链路工作**：99/25 次 stopei claim→handler→`core.isr` 真实发生（Cycle 000 起的首要嫌疑排除）。
2. **selftest no-delivery 是工具假象而非交付断裂**：SETIPNUM 强制 pending 对输入已 deassert 的 **LevelHigh** 源无效（电平型源 pending 跟随输入线）；MS10 的 SETIPNUM 裁决对象是 **DETACHED** source1（软件 pending 即其定义行为）。判读表"no-delivery→交付层断裂"分支前提不成立，属 Plan 错误（判读表设计假设）。
3. **IIR 对已掩码 RX cause 仍照报**：每 invocation 恰 8 cause 预算满（792=99×8、200=25×8，零例外、无一次 NO_INT 早退、无 THRI/RLS/MSI/unknown）——本硅 IIR 的 RX cause 报告不受 IER 掩码约束；预算有界、无风暴、无通知关闭。新裁决硅行为。
4. **数据面零字节进入完成路径**：stage1 deadline 时 `raw_rx_line` 的 pred 从未满足；快照时刻 DR=0、LSR=0x0（FIFO 空且干净）、ier_rb=ier_cache=0x55（UUE|RX_GROUP，任务 arm/recheck 循环健康）。触发节奏与操作者键入相关（两轮 99/25 次）。
5. **`[m` 截断修复板上验证成立**：诊断行完整（对比 Cycle 000 `[m` 残片）；激活序列（TEMT 排空+三步 FCR+PXAR8+双排空）按设计执行，中断行为较 Cycle 000（零 ISR entry）发生实质变化。

根因假设（供 Plan 裁决，按可能性排序）：

- **H1（最可能）**：操作者输入已到达并被 `raw_rx_stage` 累计，但 stage1 行谓词等待 `\n`（`raw_rx_line` pred `ends_with(b"\n")`），而操作者终端 Enter 发送 `\r`（CR）——行永不终止 → 30 s deadline；deadline 分支不打印 `acc`，累计字节全程不可见。与"键入相关节奏 + 快照时刻 FIFO 空且干净 + 零完成"吻合。若证实，行终止符/CRLF 策略属行为契约变更，须 Plan 修订。
- **H2（已证实硅行为，非根因）**：判定事实 3 的 IIR-vs-IER 行为，使每个 RX 条件以 8 cause 满预算报告。
- **H3（需排除，可能性低）**：RX 数据真未达 FIFO，124 次 RX 条件为空 FIFO 下 arm 触发的假条件——但该假设定量上要求 ~kHz 级陷阱率，与 ~3.3 次/s 观测不符。

关键缺口（Act 未采集，成因如实记录）：fault-snap 行格式（Act 非实质格式选择）未打印 `acc` 字节数/内容、pe/fe/bi/oe 计数与 RDI/CTI 分开计数——这三项可一行裁决 H1/H3。

- 工作区状态：本轮 agent 改动全部保留（见上文"实际改动"）；无阻塞后计划外修改；交付 FIT 为 11:31 产物（`strings StarryOS_riscv64-k3.bin | grep -c ms17` = 17）。
- 恢复条件：用户调用 `openspec-plan`。建议下一诊断轮最小集合（由 Plan 裁决，Act 不自行实施）：① deadline fault 分支打印 `acc` 字节数与可见化内容（一行裁决 H1/H3）；② ISR 分类把 RDI/CTI 分开计数并入快照；③ selftest 对 LevelHigh 源的处理（移除，或改用 GENMSI 直发 EID42——只测 IMSIC→claim 段——或改为仅真实条件后判读）；④ 操作者核对终端 Enter 发送的字节（CR/LF/CRLF）。本 Acceptance 已 3 次尝试，无第 4 次同类盲试。
- B4（RESET 回原系统）随时可安全单独执行，不受本阻塞影响。

## Plan Review

- Review Result: rework-required

**Findings**

1. 代码/diff 一致性核对通过：`activate()` prepare 批严格按 T2-R1 契约顺序（有界 TEMT 轮询→LCR→FCR 0x01/0x07/0x00→LSR→MCR→≤8 次 RBR 排空→FCR 0x41→IER UUE masked，`TxDrain` 超限无 FCR 写、状态回 idle）；`diag_snapshot` 单锁批只读绕过 latch；`aia_force_pending42`/`aia_pending42_and_eip` 复用 `aia.rs` 生产入口；payload 自检与 fault_snapshot 接线如声明；harness 5 个新用例名与契约逐字一致、共 21 项。`git diff --check` clean，工作区与本 Cycle"实际改动"一一对应。Act 验证结论（native 21 GREEN、RED E0599 见证、`make k3-uart-port` exit0、FIT strings=17 判据）覆盖未变，直接采信。
2. 阻塞（新证据，正面进展）：A3′ 第 3 次尝试仍 stage=1 deadline（两轮同构），但快照实质性缩小了缺口——**AIA source42 交付链工作**（99/25 次真实 claim→handler→isr）、**FCR/PXAR8 修复有效**（Cycle 000 零中断迹象 → 现真实 RX 条件中断以键入相关节奏到达；`iir=0xc1` 的 FIFO-enabled 位交叉证实运行态 FCR=0x41 已生效）、**`[m` 截断修复板上成立**（诊断行完整）。按 iteration-planning"连续两个 rework 未缩小 gap"检查：本 Cycle 明确缩小，允许再一个 rework。
3. Plan 错误（本轮 Review 裁定）：selftest 的 SETIPNUM 前提不成立——对输入已 deassert 的 **LevelHigh** 源，pending 跟随输入电平，SETIP 软件强制不生效；MS10 的 SETIPNUM 裁决对象是 DETACHED source1。判读表"no-delivery→交付层断裂"分支作废，`selftest irq=0 no-delivery` 无信息量（与 2 的真实 99 次 claim 并存自证）。
4. 新裁决硅行为（本轮板上事实）：**IIR 对 IER 已掩码的 RX cause 照报**——每次 invocation 恰好 8 cause 预算满（792=99×8、200=25×8，零 NO_INT 早退、零 THRI/RLS/MSI/unknown），ISR 掩码后循环内 IIR 仍报 cause 直至预算耗尽；有界、无风暴、无通知关闭。后果仅 rx_wakes 计数 ×8 膨胀（诊断口径失真）与每中断 8 轮分类（有界浪费，优化属后续范围，不进本 Cycle）。
5. 阻塞（Plan 遗漏，本轮主要缺口）：deadline 分支丢失最决定性的可观测量——`raw_rx_stage` 的累计字节 `acc` 存在于被 timeout 取消的 future 内部，fault 分支不可见；且 fault 行未打印 `CoreCounts` 已有的 oe/pe/fe/bi 计数。因此当前快照无法区分两个都与证据相容的根因：
   - **H1**（Act 提出，最可能）：输入已到达并累计，操作者终端 Enter 发送 `\r`，而 `raw_rx_line` 谓词只认 `\n` → 行永不终止；30 s deadline；快照时刻 FIFO 空（字节已入 acc）。
   - **H1b**（本 Review 补充，低概率残留）：字节以 PE/FE/BI 错误形态到达（终端发送侧格式/波特不匹配），被 head-error 纪律分类丢弃 → acc 恒空；CTI 对错误字节照常触发。错误计数未打印故无法排除。
6. 需求假设缺口（Plan 层，Cycle 000 起遗留）：契约写"输入 `ms17-a`+Enter"但从未定义 Enter 的字节（CR/LF/CRLF 随终端而异）；`raw_rx_line` 的 `\n`-only 谓词与期望内容 `ms17-a\n` 把该未验证假设固化进代码。U-Boot 阶段可正常键入不构成区分（U-Boot CLI 接受 CR 与 LF）。
7. 三次失败规则遵守说明：A3 已 3 次板测失败（Cycle 000×2 + Cycle 001×2 轮）。Cycle 002 不是同类盲试——根因假设已收敛到行终止/可见性缺口，且下一轮带决定性观测（acc+错误计数）与操作者前置核对（终端 Enter 字节），属假设确认型运行；若同层再失败即停，无第 5 次板测。
8. 非阻塞 Minor（记录）：`TX_DRAIN_POLL_LIMIT` 为轮询次数上界非时间上界（注释已写推导）；Cycle 000 三项遗留 Minor 维持；Act deviation 1（fake 缺省 LSR 改 THRE|TEMT，与真实 handoff 态一致）与 deviation 2（fault_snapshot 用 fn）成立，属测试基础设施/非实质选择。

**Acceptance Gaps**

- A3′ 未达成：stage 行未完成（根因收敛见 F5/F6），B4 仍未执行。
- A1′（native 21 GREEN 含 5 新用例）、A2′ 的 `diag_snapshot` native 部分已闭合；A2′ selftest 部分因 F3 前提错误作废，随 Cycle 002 移除；A4′ 的 build 门已闭合。

**Evidence**

- 采信本 Cycle Act Response：native/build 输出与 RED 见证（覆盖未变，本 Review 核对代码一致）。
- 本 Review 只读新分析：`raw_rx_line` 谓词与 stage 期望源码核对（k3_uart.rs:209-211、platform/k3.rs:303）；`iir=0xc1` 的 0xC0 FIFO-enabled 位解码；792/200 与 ×8 整除关系复核；SETIPNUM 对 LevelHigh 源失效的 AIC 语义复核（MS10 source1 DETACHED 对照）。

**Follow-up Decision**

rework-required，创建同目录 Cycle 002。理由：行终止接受语义是 Cycle 000 已交付契约表面的行为变更（`raw_rx_line` 与 stage 期望），selftest 的强制行为须按 Plan 错误撤销，acc 保留需要 payload API 形状变更——三者都超出本轮 Task Contract（含变更面）可约束的有限修复，Act 需新自包含契约并重新过 Gate 2。Iteration Plan 不变。

**Next Cycle**

`iterations/000-initial/002-rework.md`（rework；draft，待用户批准后转 ready）。

**Next Iteration**

None；须本 Iteration accepted 后按 Map 展开 Iteration001。
