# Iteration 000 / Cycle 002: 行终止接受、累计可见性与 selftest 撤销

## Plan Context

- Status: ready
- Cycle Type: rework

**Cycle Scope**

- Change tasks：T1、T2（Iteration 000 既有任务）。
- Acceptance gaps：A3（板上真实 RX 阶段完成 + 孤立字节）、A4 板上 B4；Cycle 001 遗留的 selftest 前提错误撤销与 acc/错误计数可见性。
- Repair items：T2-R4（行终止接受与累计可见性）、T2-R5（selftest 撤销与孤儿清理）、T2-R6（板测 B3″/B4 与前置核对）。
- Inherited scope：R1–R8/S1–S28 与默认范围；Cycle 000/001 已交付且验证的全部表面（timer 底座、route42、共享 guard、参考对齐激活序列、diag_snapshot、fault_snapshot、raw 等待协议、latch 纪律）；板上已裁决事实（交付链工作、PXAR8 生效、IIR 掩码照报、SETIP 对 LevelHigh 无效）。
- Excluded scope：T3–T8、tty ICRNL/termios 语义（Iteration 002 的 tty 层拥有 CR/LF 翻译，本轮只改资格载荷的行完成判定）、ISR 8-cause 预算优化、RDI/CTI 分开计数、copier/tty/用户态、AIA 重写、SMP、性能、MS12、全局文档与 Recorder。

**Objective**

资格载荷的行阶段在操作者真实终端（Enter 为 CR、LF 或 CRLF）下完成：stage 行显示实际收到的字节并回显；若仍失败，deadline 快照用 acc 内容与错误计数一行裁决剩余假设；selftest 错误前提撤销；B4 reset 通过。

**Background**

Cycle 001 板测（A3 第 3 次尝试）交付决定性新事实：AIA source42 交付链工作（99/25 次真实 claim→handler→isr）、PXAR8 运行态生效（`iir=0xc1` FIFO-enabled 位）、`[m` 修复成立；但 stage 行永不完成。根因收敛（父 Cycle Plan Review Findings 5/6）：H1——Enter 发送 `\r` 而 `raw_rx_line` 谓词只认 `\n`，累计字节在超时取消的 future 内不可见；H1b（低概率）——字节以线路错误形态被分类丢弃，错误计数未打印无法排除。selftest 的 SETIPNUM 前提对 LevelHigh 源不成立，属 Plan 错误。本 Cycle 为假设确认型运行，非同类盲试；同层再失败即停，无第 5 次板测。

**Investigation Facts**

Current Baseline：工作区 = Cycle 000 + Cycle 001 Act 改动（未提交）。已采信未失效结论（Cycle 001 Act Response，本 Review 核对代码一致）：native uart 21 GREEN（含 5 新用例）、`make k3-uart-port` exit0、FIT strings=17 判据；板 B1/B2 两轮复现、交付链/FCR/`[m` 三项板上裁决。

Current-State Evidence 与关键路径：

- `kernel/src/drivers/k3_uart.rs::raw_rx_line`（209–211 行）：`raw_rx_stage(port, |acc| acc.ends_with(b"\n") || acc.len() >= 512)`——`\n`-only 终止；`acc` 是 `raw_rx_stage` 内 `poll_fn` future 的局部 `Vec`，`block_on(timeout(...))` 超时取消即丢弃，deadline 分支无法打印。
- `kernel/src/platform/k3.rs::run_uart_port_bringup`：stage 期望 `b"ms17-a\n"`/`b"ms17-b\n"`（303 行）；`fault_snapshot` 打印 counts 中 irq/rx_wakes/tx_wakes/unknown/closed 但**不含** oe/pe/fe/bi/fifoerr（`CoreCounts` 已有这些字段，纯格式缺口）；selftest stage 在 activate 与 stage1 之间（283–300 行）。
- 板测数字复核对（父 Cycle Review）：rx_wakes=irq×8 整除关系成立（792=99×8、200=25×8），归因于 IIR 对 IER 掩码照报的每调用 8-cause 预算耗尽；ier_rb=ier_cache=0x55 证明 arm/readback 健康；快照时刻 DR=0/LSR=0x0 只说明 FIFO 已被任务排空，不区分"字节入 acc"与"字节被错误丢弃"。
- selftest 语义：`aia_force_pending42()` 写 `APLIC_SETIPNUM_LE=42`；LevelHigh 源输入 deassert 时 pending 跟随电平，软件强制不生效（MS10 裁决对象为 DETACHED source1）。该函数无其他调用者，撤销 selftest 后为孤儿。
- 终止符语义归属：raw 资格载荷的行完成判定是测试装置行为；产品级 CR/LF 翻译（ICRNL/INLCR/ONLCR 组合）属 Iteration 002 T5 的 tty canonical 层。本轮接受 `\r`/`\n` 任一终止不构成驱动/tty 语义承诺。

**Implementation Guidance**

先把行完成谓词提取为 core 纯函数并写 native RED，再改 raw stage 的累计所有权与 deadline 打印，随后撤销 selftest/清理孤儿，最后 build 与板测。快照格式细节为非实质选择，但 acc 与错误计数必须在 deadline 行可见。

**Task Contracts**

### T2-R4：行终止接受与累计可见性

- Requirement/Scenario：R2/R8；S4–S5（阶段完成与字节可对应）、S27 的失败可判定性。
- Depends on：None。
- Targets：`kernel/src/drivers/k3_uart_core.rs`（新纯函数 `raw_line_complete` 与行上限常量）；`kernel/src/drivers/k3_uart.rs`（`raw_rx_stage`/`raw_rx_line` 的累计所有权）；`kernel/src/platform/k3.rs`（deadline 打印 acc 与全错误计数、期望内容比较剥离尾部 CR/LF）；`tests/k3-uart-host-harness.rs` 新用例。
- Current behavior：行完成只认 `\n`；acc 在超时取消的 future 内丢失；fault 行无 acc、无 oe/pe/fe/bi/fifoerr。
- Required behavior：①`raw_line_complete(acc)`：末字节为 `\n` 或 `\r` 即完成（覆盖 CR、LF、CRLF），或达到 512 字节上限（常量入 core）。②`raw_rx_stage` 改为调用方持有累计（如 `raw_rx_stage_into(port, acc: &mut Vec<u8>, pred)` 形状，超时后调用方仍持有 acc；现有 owning 接口可基于它实现），等待协议（check→register→arm→recheck、预算）不变。③deadline fault 分支在快照行加打：`acc_len=N`、acc 的可见化内容（转义控制字符，最多打印前 64 字节+N 指示）、oe/pe/fe/bi/fifoerr 全计数；快照其余字段不变。④stage 期望内容比较在剥离尾部 `\r`/`\n` 后进行（`received="ms17-a\r"` 与期望 `ms17-a\n` 不再触发 content-mismatch note）。
- Preserve：raw 等待协议与预算、latch/错误分类纪律、ISR 不变、`raw_rx_stage` 现有对外语义（Ready 返回累计字节）、诊断输出其余格式。
- Forbidden：把 CR/LF 翻译引入驱动或 tty 语义、周期轮询、无界打印（acc 超过 64 字节只打计数与截断标记）、改动 fault 快照的读取次数纪律（各寄存器至多一次，acc 是内存数据不受限）。
- Test witness：`raw_line_complete` native 用例（`\n` 完成、`\r` 完成、`\r\n` 完成、部分行 false、空 false、达 512 上限 true）；RED = 函数缺失编译失败（E0425，项目既定 RED 约定），GREEN 后既有 21 用例不回归。累计所有权与 deadline 打印为 payload/kernel 侧，板上直接观察（B3″ 的 `received=`/`acc_len=` 行）即判定。
- GREEN condition：native 全 GREEN；板上 stage 行完成并显示实际字节。
- Verification：V1/V2/V3 见下。
- Stop when：板上 acc_len>0、错误计数全 0、行仍不完成（谓词实现缺陷）——停，交 Plan。

### T2-R5：selftest 撤销与孤儿清理

- Requirement/Scenario：R2/R8；撤销 Cycle 001 T2-R2 中前提错误的部分（LevelHigh SETIP 无效）。
- Depends on：None。
- Targets：`kernel/src/platform/k3.rs`（selftest stage 删除）；`kernel/src/drivers/k3_uart.rs`（`aia_force_pending42` 孤儿删除；`aia_pending42_and_eip` 保留——fault_snapshot 仍用）。
- Current behavior：activate 后打印 `selftest irq=0 no-delivery`（无信息量）。
- Required behavior：selftest stage 整体移除，板上不再出现 selftest 行；`diag_snapshot`/`aia_pending42_and_eip`/fault_snapshot 保留不变。裁决事实已记 design D2，不在代码中留注释性死代码。
- Preserve：fault 快照路径、激活序列、其余载荷流程。
- Forbidden：以 GENMSI 直发 EID42 等新机制替代（交付链已由真实 IRQ 证实，无剩余诊断价值；不扩表面）。
- Test witness：代码 Review（删除无残留引用）；板上 selftest 行缺席即判定。
- GREEN condition：build exit0、板上无 selftest 行。
- Verification：V2/V3。
- Stop when：无。

### T2-R6：板测 B3″/B4 与前置核对

- Requirement/Scenario：R2/R8；S4–S5、S27–S28。
- Depends on：T2-R4、T2-R5。
- Targets：`make k3-uart-port` 交付 FIT；R68 流程。
- Current behavior：Cycle 001 镜像 stage 行不完成。
- Required behavior：**前置核对（操作者，装载前）**：确认所用终端 Enter 键发送的字节（CR/LF/CRLF；如 screen/minicom 缺省 CR，可查终端配置或以本地回显判断），一句话记入结果。随后按 R68 启动：B2 两行自动复现（无 selftest 行）；stage1/2 依次输入 `ms17-a`+Enter、`ms17-b`+Enter（各 30 s 内），每条看到 `received="…"` 显示实际字节（含终止符）并回显；isolated 只输入 `Z` 后停止，单字节返回；全字段 counts 行与 complete 行；最后 RESET 直接见 `Bianbu … k3 login:`。
- Preserve：每次 `make k3` 后重跑 `make k3-uart-port` 的交付判据（字符串计数本轮更新：selftest 缺席、`acc_len`/`received` 在镜像内，具体数值由 Act 构建后记录）。
- Forbidden：把终端本地回显冒充 kernel 回显；为通过而临时改终端 Enter 设置再宣称"原配置通过"（前置核对如实记录原配置；若操作者主动改配置属环境事实，同样如实记录）。
- Test witness：操作者直接输入与所见即判定；一句话结果记入 Act Response。
- GREEN condition：B3″ 三阶段完成（含实际字节可见）+ B4 reset 回原系统。
- Verification（操作者判读表，fault 分支）：
  - `acc_len>0` 且错误计数全 0 且行完成 → 正常路径（应无 fault）。
  - fault 且 `acc_len>0`、错误计数 0 → 谓词实现缺陷，停，交 Plan。
  - fault 且 `acc_len=0`、oe/pe/fe/bi 有非零 → H1b 证实：终端发送侧格式/波特不匹配，属环境问题非内核缺陷；停，报告用户裁决（改终端配置或保持并豁免该项）。
  - fault 且 `acc_len=0`、错误计数 0、irq>0 → 中断有 cause 无字节无错误，新硅行为，停，交 Plan。
  - fault 且 irq=0 → 中断生成回归（与 Cycle 001 证据矛盾），停，交 Plan。
- Stop when：任一 fault 分支命中即停（本 Cycle 为 A3 第 4 次板测、同层假设确认型；再失败无第 5 次）；RESET 未回原系统立即停止装载。

**Invariants**

1. Cycle 000/001 全部不变量继续有效。
2. 真实输入 IRQ 仍是 S4 唯一验收；acc/错误计数是观测，不计入验收替代。
3. 行完成接受 `\r`/`\n` 是资格载荷判定，不引入驱动/tty 的 CR/LF 翻译语义（tty 层 Iteration 002 裁决）。
4. 本板裁决事实继续有效：SETIP 对 deassert LevelHigh 无效；IIR 对掩码 RX cause 照报（rx_wakes ÷8 才是中断数口径）。
5. timer 底座、AIA、激活序列、等待协议代码语义零变更（仅载荷累计所有权与打印）。

**Non-goals**

ISR 8-cause 预算优化、RDI/CTI 分计数、tty/canonical CR/LF 翻译、copier/ring、用户态、SMP、性能、经验产物与全局状态同步。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1″ | `raw_line_complete` native 用例 RED→GREEN（CR/LF/CRLF/上限/部分行）；既有 21 用例不回归 | T2-R4 / R2 |
| A2″ | selftest stage 与 `aia_force_pending42` 移除、无残留引用；板上无 selftest 行 | T2-R5 / R8 |
| A3″ | B3″ 两行 + 孤立字节完成，`received=` 显示实际字节（含终止符）并回显；全字段 counts 行 | T2-R4/R6 / R2 / S4–S5 |
| A4″ | B2 复现、B4 RESET 回原系统、`make k3-uart-port` exit0、交付 FIT 判据（数值更新）成立 | T2-R6 / R8 / S27–S28 |

RTM（本 Cycle）：R2 → T2-R4/R6 Covered；R8 → T2-R5/R6 Covered。R1/R3–R7 余下场景属后续 Iteration。

**Verification**

V1（agent，native）：`rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-r2` 后 `--test-threads=1`；新用例先 RED（`raw_line_complete` 缺失 E0425）再 GREEN。timer 8 项、AIA 17/18、Cycle 000/001 其余 build 采信（覆盖表面未变——本轮不触碰 timer/AIA/legacy 门内模块；`make k3`/`make build`/`make lichee` 采信）。

V2（agent，build）：`make k3-uart-port` exit0；`git diff --check`；交付 FIT 字符串判据更新记录（selftest 缺席、acc_len/received 在镜像内）。

V3（操作者，B3″/B4）：前置核对 Enter 字节 → 按 T2-R6 步骤与判读表执行；无结果则 A3″/A4″ pending，不展开 Iteration 001。

V4（失败处理）：判读表任一 fault 分支即停并交 Plan；RESET 异常立即停止装载。

**Gate 2 Readiness**

PASS：技术调查（板测快照判读 + 源码核对）、契约、追踪与验证计划闭合，无 TBD。2026-10-07 用户原话"批准"——批准本返工计划。Status 转 ready；Act 等待明确执行指令。

**Persisted Evidence**

- Mode: none

命令、≤20 行决定性输出、exit 与操作者一句话结果足以判定；板测串口记录按需逐字入 Act Response。

**Risks and Notes**

- 若操作者终端实际发送 LF，H1 不成立而 Cycle 001 的失败仍未解释——判读表 `acc_len>0/错误 0/行完成` 或各 fault 分支仍能一行定位；不依赖 H1 为真。
- acc 打印上限 64 字节：512 上限截断时只有计数与截断标记，足够判定。
- `raw_rx_stage` 接口形状调整（owning 包装 vs 双接口）为非实质选择；谓词语义与累计保留是契约。
- harness 全局静态计数 reset 既有模式延续。
- 本 Cycle 后若 A3 仍失败，Iteration 000 需要整体设计复盘（三次失败规则已触发两次同类板测循环，第三次将按规则返回需求/设计层）。

## Act Response

- Status: reported

### 实际改动

**T2-R4（行终止接受与累计可见性）**

- `kernel/src/drivers/k3_uart_core.rs`：新增 `RAW_LINE_LIMIT: usize = 512` 常量与纯函数 `raw_line_complete(acc: &[u8]) -> bool`（末字节为 `\n` 或 `\r` 即完成，覆盖 CR/LF/CRLF；或 `len >= RAW_LINE_LIMIT` 完成）。doc 注明这是资格载荷语义，驱动/tty 的 CR/LF 翻译仍归 Iteration 002。
- `kernel/src/drivers/k3_uart.rs`：新增 `raw_rx_stage_into(port, acc: &mut Vec<u8>, pred)`——调用方持有累计，超时取消后调用方仍持有 acc；等待协议（check→register `RX_WAKER`→arm `RX_GROUP`→recheck）与 256/64 预算逐行不变，Ready 改返回 `()`。`raw_rx_stage(port, pred) -> Vec<u8>` 保留为基于 into 的 owning 包装（对外语义不变）；`raw_rx_one_byte` 不变。
- `kernel/src/platform/k3.rs`：stage1/2 循环改为调用方持有 `acc` + `raw_rx_stage_into` + `crate::drivers::k3_uart_core::raw_line_complete` 谓词；echo 回显累计字节。`fault_snapshot` 增加第三参 `acc: &[u8]`，首行 fault-snap 加打 `oe/pe/fe/bi/fifoerr` 全计数与 `acc_len=N acc="…"`（可见化内容经 `escape_snapshot_acc` 转义控制字符为 `\xNN`，最多前 64 字节，截断时加 `+trunc`）；快照其余字段（irq/rx_wakes/tx_wakes/unknown/closed/ier_rb/ier_cache/iir/lsr/dr 与 aplic_pending42/eip0 行）不变。期望内容比较经 `strip_trailing_crlf` 剥离两侧尾部 CR/LF（`received="ms17-a\r"` 对期望 `ms17-a\n` 不再触发 content-mismatch note）。isolated 超时分支传 `&[]`：一字节谓词下超时 ⇒ 累计为空，构造性成立（代码内注释说明）。
- `tests/k3-uart-host-harness.rs`：新增 3 个 `raw_line_complete` 用例，覆盖契约全部 6 场景：LF/CR/CRLF 完成；空、无终止符、末字节非终止符（`ms17-a\rX`）false；达 512 上限 true 且 511 字节无终止符 false。

**T2-R5（selftest 撤销与孤儿清理）**

- `kernel/src/platform/k3.rs`：selftest stage（`aia_force_pending42` + 500 ms irq_count 轮询等待 + `[ms17] selftest` 行）整体删除，板上不再出现 selftest 行。
- `kernel/src/drivers/k3_uart.rs`：删除 `aia_force_pending42`；`APLIC_SETIPNUM_LE` import 一并移除；`aia_pending42_and_eip` 保留（fault_snapshot 仍用）。段注释更新为 "One-shot AIA fault observation (rework T2-R2)"。design D2 裁决事实保留在 design，不在代码留注释性死代码。

### 与计划的偏差

1. **删除 `raw_rx_line`**（契约 Targets 写 "`raw_rx_stage`/`raw_rx_line` 的累计所有权"）：按契约 ②，stage 循环必须走 into 形态才能在超时后保留 acc，owning line 包装因此失去全部调用方；谓词职责移入 core `raw_line_complete`。按公共规则"清理由本次改动产生的孤儿"删除而非保留无调用方的 API。Preserve 清单只约束 `raw_rx_stage` 对外语义（已保留）。
2. **RED 报错码**：契约约定 E0425；实际在 import 位报 E0432（`no 'raw_line_complete' in 'k3_uart_core'`）——同一"缺失项编译失败"类，RED 性质不变。
3. **`strip` 闭包改嵌套函数 `strip_trailing_crlf`**：闭包两个输入生命期的省略规则不满足编译；函数形式控制流等价，非实质。

### 自检（Gate 3）

- 残留引用：`raw_rx_line`、`aia_force_pending42`、`selftest` 在 `kernel/src/` 与 harness 中零残留；`APLIC_SETIPNUM_LE` 仅剩 MS10 `aia_probe.rs` 自有使用（不属本轮面）。
- 警告审计：构建输出 4 条 rustc 警告（`PacketAssemblerSet`、`IFACE_MAX_PREFIX_COUNT`/`SIXLOWPAN`、`frag`），符号在本仓改动面与工作区 crates 均不存在，属 registry 依赖既有警告；kernel 无新增警告。
- 契约逐项核对：等待协议/预算、latch 纪律、ISR、`diag_snapshot` 寄存器读取纪律（acc 为内存数据不受限）、诊断输出其余格式均未变；未引入 CR/LF 驱动/tty 语义、周期轮询或无界打印。
- 遗留 Minor：无。

### 验证（Gate 4）

**V1（native，RED→GREEN）**

```text
$ rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-r2
error[E0432]: unresolved imports — no `RAW_LINE_LIMIT` in `k3_uart_core`
              no `raw_line_complete` in `k3_uart_core`
error: aborting due to 1 previous error          # RED（实现前）

$ /tmp/ms17-k3-uart-r2 --test-threads=1
test result: ok. 24 passed; 0 failed; 0 ignored  # GREEN（21 既有 + 3 新）
exit 0
```

**V2（build 与交付判据）**

```text
$ make k3-uart-port
... mkimage -f tools/starry-k3.its StarryOS_riscv64-k3.fit
Default Configuration: 'conf-starry-k3'          # exit 0
$ git diff --check                               # exit 0
$ grep -aoc：selftest=0（缺席）；acc_len=1（offset 114269）；received==2；fault-snap=2
$ strings StarryOS_riscv64-k3.fit | grep '\[ms17\]' | wc -l → 15（Cycle 001 为 17，selftest 行移除所致）
```

**采信（覆盖表面未变化）**：timer 8 项、AIA 17/18、`make k3`/`make build`/`make lichee` 采信 Cycle 000/001 Act Response 结论——本轮不触碰 timer/AIA/legacy 门内模块。

**V3（板测，操作者，2026-10-07 已执行，两轮）**

- 前置核对（板上直接证实）：`received="…\r"` 显示行尾终止符为 CR——操作者终端 Enter 发送 CR，H1 证实（即 Cycle 001 stage 行不完成的根因）；两条行均以 CR 终止完成。
- 第一轮：B2 两行复现（`sleeps_ok expect_ms=150 elapsed_ms=164`、`timeout_cancelled elapsed_ms=7`）、`activate=ready`、**无 selftest 行**；stage1 `received="dwdasd\r"` echo=done bytes=7；stage2 `received="dwas\r"` echo=done bytes=5；isolated `received='a'` echo=done bytes=1；全字段 counts `irq=26 rx_wakes=104 tx_wakes=13 unknown=0 oe=0 pe=0 fe=0 bi=0 fifoerr=0 closed=false`；`qualification complete park`。
- 第二轮：stage1 `received="i love u\r"` echo bytes=9（空格原样回显）；stage2 `received="ok!\r"` echo bytes=4；isolated `'n'`；counts `irq=28 rx_wakes=112 tx_wakes=14` 错误计数全 0；`qualification complete park`。
- 输入脚本豁免（用户原话，保留）："测试设计的是我输入预期字符串，没必要，我们只需要验证功能正常就好"——操作者以自由文本代替预设 `ms17-a`/`ms17-b`；验收口径以阶段完成、实际字节（含终止符）可见、回显、孤立字节与全字段 counts 为准（R2/S4–S5 成立）。`note=content-mismatch` 为载荷设计行为（输入≠预设常量），信息性输出，保留不改。
- 非阻塞观察：rx_wakes÷irq=4（Cycle 001 满 FIFO 突发下为 8）——IIR 掩码回声深度随任务排空 DR 的时序变化；irq 计数仍是 claim 口径，判读表 irq 分支不受影响。
- B4：两轮之间经复位再次进入 U-Boot 并完成二次装载（reset 恢复路径成立）；最终 `Bianbu … k3 login:` 行未单独粘贴，随操作者"功能正常"整体裁决记录。
- 判定：**A1″–A4″ 全部成立**（A1″ native RED→GREEN；A2″ selftest 缺席 = 镜像 grep 0 + 两轮串口无 selftest 行；A3″ 两轮行/字节/回显/孤立/counts；A4″ B2 复现 + build exit0 + FIT 判据 + reset 回恢复路径）。判读表五分支均未命中，同层第 4 次板测通过，三次失败规则解除。

### Persisted Evidence

None required（模式维持 `none`；命令、决定性输出与操作者一句话结果记录于本 Response）。

### Experience Candidates

None。

### 未解决问题

无 Blocker。Iteration 000 验收面全部有直接证据；等待 Plan Review。

## Plan Review

- Review Result: accepted（2026-10-07；Iteration 000 完成）

**Findings**

1. 代码/diff 独立核对通过，四文件与契约逐项一致：①`k3_uart_core.rs:125-135` `RAW_LINE_LIMIT=512` 与 `raw_line_complete`（末字节 `\n`/`\r` 或达上限），doc 明确资格载荷语义、CR/LF 翻译留给 Iteration 002 tty 层；②`k3_uart.rs:174-243` `raw_rx_stage_into` 调用方持有 `acc`，等待协议（check→register→arm→recheck）与 256/64 预算结构不变，owning `raw_rx_stage` 基于它实现且对外语义保留，`raw_rx_one_byte` 不变；③`k3.rs:176-226` `fault_snapshot` 第三参 acc、首行加打 oe/pe/fe/bi/fifoerr 全计数与 `acc_len`/转义内容（`escape_snapshot_acc` 0x20–0x7e 透传、其余 `\xNN`、64 字节上限 +trunc），其余字段与第二行 aplic/eip 不变；寄存器读取纪律保持（`diag_snapshot` 单批每寄存器一次，acc 为内存数据）；④`k3.rs:232-238,350` `strip_trailing_crlf` 双侧剥离后比较。stage 循环 echo 回显累计字节原样（无翻译引入）。harness 24 项 = 21 既有 + 3 新，覆盖契约全部 6 场景（LF/CR/CRLF 完成、空/无终止/`\rX` false、512 上限 true 与 511 false）。
2. T2-R5 独立证实：`k3.rs` activate 后直接进入 stage 循环，selftest stage 无残留；`aia_force_pending42` 与 `APLIC_SETIPNUM_LE` import 已删，`aia_pending42_and_eip` 保留且仍被 `fault_snapshot` 调用，段注释更新为 T2-R2 归属。孤儿 grep（本 Review 只读复跑）：`raw_rx_line`/`aia_force_pending42`/`selftest` 在 `kernel/src/` 与 `tests/` 零匹配；`APLIC_SETIPNUM_LE` 仅剩 `aia.rs:127` 定义与 `aia_probe.rs` 三处 MS10 自有使用。`git diff --check` exit 0。
3. 周期边界证实（mtime 只读检查）：本 Cycle 窗口（10-07 12:16–12:19）仅改动四个契约目标文件，FIT 构建于 12:32 晚于全部源码修改；timer/AIA/legacy 门内模块最后修改均在此前周期窗口——Act"本轮不触碰"成立，timer 8 项、AIA 17/18 与 `make k3`/`make build`/`make lichee` 采信合法（覆盖未变）。
4. V3 板测采信（操作者两轮，2026-10-07）：前置核对直接证实 Enter 发送 CR——H1 根因板上成立（Cycle 001 stage 行不完成由此解释）；两轮 B2 复现、`activate=ready`、无 selftest 行、stage1/2 行完成且 `received=` 显示实际字节含终止符并回显、孤立字节单字节返回、全字段 counts 错误计数全 0、`qualification complete park`；两轮间复位回 U-Boot 并二次装载（reset 恢复路径直接证据）。判读表五分支均未命中；A3 第 4 次板测为假设确认型（带 acc/错误计数决定性观测 + 前置核对，非盲试）通过，三次失败规则解除。输入脚本按用户原话豁免预设字符串，`note=content-mismatch` 为载荷设计信息性输出，保留正确。
5. 偏差分类（均非阻塞，Act 已如实记录）：deviation 1（删除 `raw_rx_line`）为 Act 偏离 Targets 文本——契约 ② 要求 stage 循环走 into 形态后 owning line 包装零调用方，按"清理由本次改动产生的孤儿"删除正确且引用零残留（本 Review 证实），语义由 core 谓词承接；deviation 2（E0432 vs 约定 E0425）同属"缺失项编译失败"RED 类；deviation 3（strip 闭包改嵌套函数）为生命期省略规则下的等价控制流。
6. 非阻塞 Minor（新证据，记录）：最终 B4 的 `Bianbu … k3 login:` 行未单独粘贴，随操作者整体裁决记录；reset 恢复路径本身有直接证据（两轮间复位 + 二次装载），按人工验证一句话记录规则不阻塞 A4″。
7. 非阻塞 Minor（观察，记录）：rx_wakes÷irq=4（Cycle 001 满 FIFO 突发下为 8）——IIR 掩码回声深度随任务排空 DR 时序变化，irq 计数仍为 claim 口径；诊断口径失真属 Cycle 001 已裁决硅行为，预算优化在本 change Non-goals。

**Acceptance Gaps**

无。A1″（native RED→GREEN 24/24，6 场景全落码）、A2″（代码移除 + FIT grep 0 + 两轮板上无 selftest 行）、A3″（两轮行/字节/回显/孤立/全字段 counts）、A4″（B2 复现、build exit0、FIT 判据、reset 恢复）全部成立。Iteration 000 验收面（T1 timer 底座 + T2 PXA port 低层双向资格）全部有直接证据。

**Evidence**

- 采信本 Cycle Act Response：V1 native（RED E0432 见证 → GREEN 24 passed exit 0；覆盖未变，本 Review 核对代码一致）、V2 build/FIT 判据（FIT mtime 晚于最后源码修改）、V3 操作者两轮一句话记录。
- 本 Review 只读新检查：四文件独立阅读（结论见 F1/F2）、孤儿 grep、`git diff --check`、mtime 周期边界、FIT 产物存在性与新鲜度。

**Follow-up Decision**

accepted。全部 Acceptance 有直接证据，无阻塞 finding；三项偏差均非实质且按规则记录。本 Cycle 为 Iteration 000 最终 Cycle，Iteration 000 完成；按 Map 展开 Iteration001（T3/T4，UART 数据面与物理 drain），新 Cycle 以 draft 交付，待用户计划审计批准后转 ready。T1、T2 勾选随本次 Review 生效。

**Next Cycle**

None。

**Next Iteration**

`iterations/001-uart-dataplane-drain/000-initial.md`（draft，待用户批准后转 ready）。
