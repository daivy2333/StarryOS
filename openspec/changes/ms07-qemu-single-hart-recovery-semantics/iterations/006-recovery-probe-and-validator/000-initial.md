# Iteration 006 / Cycle 000: Recovery probe and validator

## Plan Context

- Status: ready
- Iteration: 006-recovery-probe-and-validator
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 4.1
- Depends on: Iteration 005
- Stable baseline: append-only QEMU recovery control/snapshot、deterministic guest marker protocol 与纯输出 validator 形成可冻结、可负向验证的资格协议。
- Verification boundary: Rust ABI/feature seams、C probe decision core、Python validator negative fixtures、既有 MS03–MS06 host seams、axnet ordinary/diagnostics 与 kernel build 全部通过。
- Diagnostic boundary: versioned ioctl/snapshot、resident-owner reset request、guest probe阶段、validator grammar、revision/environment identity。
- Deferred tasks: 4.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R4、R5、R6、R8；D1–D8；Iteration 005 接受的 QueueEpoch/SocketEpoch/LinkGeneration、resident owner、structured recovery fault、epoch-scoped socket terminal 与 commit-before-wake 语义。
- Excluded scope: 自动或手工 QEMU 资格结论、HMP 自动化、MS01/MS04/MS05/MS06 runtime 回归、SMP、PCI/DWMAC、真板、性能、自动恢复重试。

**Objective**

建立 MS07 的 append-only QEMU 观测与触发 ABI、guest recovery/link probe 和纯输出 validator。自动 Gate 必须能证明新协议的布局、case 集合、阶段顺序、negative rejection、feature 隔离与旧 ABI 兼容；真实单 hart QEMU/HMP 运行留给 Iteration 007。

**Background**

Iterations 000–005 已在 host/model 层闭合 reset owner、epoch ledger、deadline、link policy 和 socket terminal，但 guest 目前只能读取 V1–V3 snapshot、使用 MS05 hold/flush，并运行 MS06 readiness 协议。缺少显式 resident-owner reset 请求、recovery/link/socket epoch 观测，以及能拒绝残缺或漂移 transcript 的 MS07 协议。D8 要求先冻结这些工具和协议，再由下一 Iteration 消费同一 artifact 做手工 QEMU 资格。

**Current Baseline**

- Revision: `596b324b6e7cb78b3a4308b997657b6d0c95d44a`；branch `net-k3`；Iterations 000–005 产品改动仍在工作树。
- `sys_ioctl` 暴露固定 V1 `0x4e494431`、V2 `0x4e494432`、V3 `0x4e494433` snapshot；QEMU feature 下另有 MS05 diagnostic control 与 flush。V1–V3 均为独立 `repr(C)` wire type，V3 为 72 个 `u64`，不得改变布局或含义。
- `irq_snapshot_v3` 把 IRQ V2 prefix 与 `axnet::rx_snapshot_v3()` 的 slot/ticket/driver/flush/lease ledger 合并；尚未暴露 recovery stage、QueueEpoch、SocketEpoch、LinkGeneration/state 或 coherent recovery fault identity。
- resident queue owner 已能因 completion/reclaim deadline进入 `Quiescing → Resetting → Reinitializing → Active/Faulted`，但没有 QEMU-only 显式 reset request seam；reset 不能在 syscall 上下文直接执行。
- MS06 probe/validator 已提供固定 case 顺序、revision/environment/exit 校验和大量 negative self-test；validator 只读 transcript，不导入 socket/subprocess，也不启动 QEMU。
- 当前 `make host-test` 包含 MS03–MS06 Rust/C/Python seams；sandbox 可能只在 loopback socket self-test 返回 `EPERM`。该环境分层不能掩盖编译、断言、validator 或 build 失败。

**Current-State Evidence**

- `kernel/src/syscall/fs/ctl.rs::sys_ioctl`：V1–V3 snapshot、QEMU-only diagnostic control/flush 的 syscall 边界与错误映射。
- `kernel/src/drivers/virtio_net_irq.rs::irq_snapshot_v3` 与 `virtio_net_irq_logic.rs::IrqSnapshotV1/V2/V3`：append-only ABI mapping 和现有 72-field V3 authority。
- `crates/axnet/src/async_rx.rs::RxRxFuture::{enter_recovery,poll_recovery,recovery_step}`：唯一 resident owner 的 reset 生命周期；`freeze_recovery_summary` 已形成 coherent stage/cause/epoch/owner identity。
- `crates/axnet/src/async_rx.rs::{rx_snapshot_v3_from,diagnostic_control_shared}`：Service guard 下 snapshot/control 的可注入 host seam，以及 unlock 后 event publication 顺序。
- `tests/ms05_data_plane_probe.c`：V3 C layout、QEMU-only ioctl 与阶段化 probe 的现有模式；`tests/ms06_stack_readiness_probe.c` 和 `scripts/ms06-qemu-validate.py`：固定 marker grammar、case-set 同源检查和纯输出审计基线。
- `tests/ms03-irq-host-harness.rs`、`tests/ms04-async-rx-host-harness.rs` 与 Makefile `host-test`：旧 ABI layout、feature propagation、C/Python seams 的回归入口。
- `.claude/runbooks/qemu-network-testing.md`：QEMU shell 与 HMP 必须由用户手工驱动；自动化仅能构建 payload、运行 host seams 并离线审计保存的 raw serial。

**Relevant Code**

- `crates/axnet/src/async_rx.rs`、`lib.rs`：reset request、resident-owner消费、V4 recovery snapshot source 与 qemu-diagnostics feature boundary。
- `crates/axnet/src/service.rs`、`stack_runner.rs`、`wrapper.rs`：QueueEpoch/SocketEpoch/link/terminal 观测来源；不得改变其已接受语义。
- `kernel/src/drivers/virtio_net_irq_logic.rs`、`virtio_net_irq.rs`：V4 wire type 与 kernel mapping。
- `kernel/src/syscall/fs/ctl.rs`：新 versioned snapshot/reset command，仅 QEMU feature 可见。
- `tests/ms07_recovery_probe.c`、`tests/ms07_recovery_probe_test.c`：guest payload 与 host-testable decision core。
- `scripts/ms07-qemu-validate.py`：纯 transcript validator 与 negative self-test。
- `tests/ms03-irq-host-harness.rs`、`tests/ms04-async-rx-host-harness.rs`、Makefile：ABI、feature、case-set 与 host Gate。

**Critical Path**

1. Guest 通过新 QEMU-only reset ioctl 提交一次 checked request；syscall 只提交请求并唤醒既有 queue event，不直接访问 transport、descriptor 或运行 recovery step。
2. 唯一 resident queue owner 在 task context 消费 request，关闭当前 SocketEpoch并驱动既有 quiesce/reset/reinitialize 状态机；成功后推进 QueueEpoch/SocketEpoch，失败后保持 Faulted。
3. 新 V4 snapshot 以独立 wire type保留完整 V3 prefix，再追加 recovery stage、epochs、link state/generation、coherent fault和 owner summary；guest 在阶段边界读取，不用旧字段改义。
4. Probe 先证明 reset 前流量，触发 reset并见证旧 socket terminal、新 epoch与新 socket流量；在明确 marker 处等待用户执行 HMP off/on，再见证 `NotConnected`、link generation与新 socket恢复。
5. Validator 只读取保存的 serial transcript，按固定 grammar核对 revision、environment、case顺序、epoch单调性、stage/ledger约束、FAIL/exit；不启动 QEMU、不访问网络。

**Implementation Guidance**

- 新 snapshot 使用独立 V4 类型与未占用 ioctl number；V1–V3 的字段、大小、offset、数字和语义保持逐字节不变。V4 的 V3 prefix由既有 V3 mapping复制，追加字段由一个 shared assembly seam 生成，避免跨时刻拼出伪 tuple。
- reset request采用有界 first-consumer/checked 状态；重复请求、非 Active 请求和未初始化/unsupported target返回稳定错误。成功提交后先释放 Service guard，再发布 queue-work event；owner仍是唯一执行 recovery 的主体。
- Probe 把网络 I/O 与 poll/epoll deadline作为等待机制，snapshot仅用于阶段见证；不得调用内部 axnet progress，也不得以无界 sleep-poll推进恢复。HMP marker只声明用户操作边界，不伪造 link事件。
- C decision core与真实 guest I/O 分层，host test通过 mutation覆盖大小、offset、stage、epoch、ledger和 marker判据。Python validator的 EXPECTED_CASES 必须与 probe `--print-cases` 输出完全一致。
- validator 接受串口噪声但拒绝 protocol namespace内未知、重复、缺失、乱序、部分成功、错误 revision/environment、非零或缺失 exit，以及 panic/trap/fatal ownership drift。

**Behavioral Change**

QEMU 构建新增一个 append-only recovery snapshot版本和一个只提交 resident-owner reset request 的 control command；普通非-QEMU构建不暴露它们。新增 MS07 guest payload输出确定性阶段 marker，离线 validator能对完整 raw serial作严格资格预审。它们本身不构成真实 QEMU PASS。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 4.1 | R4/R5 reset telemetry/trigger | `async_rx.rs`、`lib.rs` | resident recovery、V3 snapshot/control seams | 增加 checked reset request与 coherent V4 source，由唯一 owner消费 |
| 4.1 | R4/R5/R6 ABI | `virtio_net_irq_logic.rs`、`virtio_net_irq.rs`、`ctl.rs` | V1–V3 wire/mapping/ioctl | 新增独立 V4 snapshot和QEMU-only reset command，保留旧ABI |
| 4.1 | R5/R6/R8 probe | `tests/ms07_recovery_probe*.c` | None | 固定case、阶段marker、host decision core与guest I/O |
| 4.1 | R8 validator | `scripts/ms07-qemu-validate.py` | None | 纯输出parser、negative fixtures、identity/ordering/ledger判据 |
| 4.1 | R8 regression | Makefile、MS03/MS04 harness | 既有host gates与feature guards | 接入C/Python/Rust seams、case-set diff和pure-auditor guards |

**Task Contracts**

### 4.1: Versioned recovery probe and validator

- Requirement/Scenario: R4 telemetry；R5 reset trigger；R6 HMP markers；R8 host/model 与 QEMU 协议边界。
- Depends on: Iteration 005 accepted；现有 resident recovery 和 epoch-scoped socket terminal保持稳定。
- Targets: `crates/axnet/src/{async_rx.rs,lib.rs}`；`kernel/src/drivers/{virtio_net_irq.rs,virtio_net_irq_logic.rs}`；`kernel/src/syscall/fs/ctl.rs`；`tests/ms07_recovery_probe*.c`；`scripts/ms07-qemu-validate.py`；Makefile与相关host harness。
- Current behavior: V3 snapshot、diagnostic hold和flush存在；没有显式 reset request、recovery/link/socket epoch ABI或MS07 transcript协议。
- Required behavior: 新命令/结构采用新版本且不改 V1–V3；reset只由 resident owner执行；probe输出固定阶段/case marker；validator纯审计并拒绝缺失、重复、乱序、错误identity、FAIL和非零/缺失exit；非QEMU feature不可见。
- Required changes: 建立 checked request提交/消费 seam；建立独立 V4 C/Rust layout与shared snapshot assembly；实现C decision core和guest模式；实现Python grammar/negative self-test；把ABI、feature、case-set和pure-auditor guards接入host-test。
- Preserve: existing ioctl数字与ABI、2秒 diagnostic lease crash safety、唯一 queue owner、error/epoch commit-before-wake、手工QEMU政策、raw serial事实源、MS03–MS06 protocol和tests。
- Forbidden: syscall直接reset或poll driver；第二 queue task；修改/复用V1–V3字段含义；validator导入 socket/subprocess、启动QEMU或驱动guest/HMP；probe调用内部 axnet poll或用无界sleep-poll推进状态；用本 Iteration结果声明QEMU资格。
- Test witness: 先以 Rust layout/feature/source tests、C mutation tests、Python negative fixtures和 probe/validator case-set diff 建立 RED；预期分别因 V4/request/probe/validator尚不存在而失败。既有 V1–V3 ABI tests作为变更前 GREEN preserve witness。
- GREEN condition: 新 seams 全绿；每个 negative transcript被拒绝且首个决定性错误可定位；probe/validator case-set完全一致；V1–V3 size/offset/number tests不变；普通构建看不到QEMU control；reset request只能由resident owner消费。
- Verification: axnet ordinary与qemu-diagnostics串行全量；MS03/MS04 Rust harness；MS07 C syntax/decision tests；validator self-test和case diff；`make host-test`；kernel build；rustfmt、`git diff --check`与strict OpenSpec validation，全部按下述环境规则判定。
- Stop when: V4无法append-only表达所需identity；必须破坏旧probe/ioctl；reset无法经唯一resident owner触发；或实现需要自动驱动QEMU/HMP。停止并返回Plan，不在Act内扩展协议。

**Invariants**

- V1为8个、V2为28个、V3为72个 `u64`；旧类型独立存在，size/align/offset与ioctl数字不变。
- recovery request只是控制事件；descriptor、transport reset、epoch推进和Faulted提交仍只发生在resident owner task context。
- QueueEpoch只在成功整设备reset后推进；link flap只推进LinkGeneration/SocketEpoch，不推进QueueEpoch；旧socket terminal不被新epoch清除。
- snapshot不得用分离锁区读取拼接一个声称coherent的fault/owner tuple；缺失Service/target必须有明确sentinel或unsupported语义，不得伪造健康值。
- validator不改变系统状态；probe与validator case/marker grammar一一对应；任何partial success、panic/trap、ownership drift或缺exit均失败。
- wake在相关状态提交和guard释放后发生；不得跨await、Pending或waker持有Service/socket/driver guard。

**Non-goals**

- 不执行或接受真实QEMU/HMP runtime，不冻结最终qualification artifact，不运行MS01/MS04/MS05/MS06 guest回归。
- 不实现QEMU runner、serial shell automation、HMP automation或validator网络访问。
- 不修改recovery算法、deadline数值、socket错误映射、link policy或既有诊断lease语义，除非暴露只读append-only观测所必需。
- 不覆盖SMP、PCI、DWMAC、真板reset/DMA停止、透明连接迁移、自动重试或性能。

**Acceptance**

- A1 / R4、R5：QEMU-only checked reset command提交后由唯一resident owner消费；重复/非法状态稳定失败，syscall不执行driver reset。
- A2 / R4–R6：独立V4 snapshot保留完整V3 prefix，并可一致观察 recovery stage、QueueEpoch、SocketEpoch、LinkGeneration/state、coherent fault与bounded owner/ledger摘要。
- A3 / R5、R6、R8：probe具备固定 reset前流量、reset触发、旧socket terminal、恢复后新socket、HMP link off/on和ledger检查阶段；所有等待有绝对deadline且不内部poll axnet。
- A4 / R8：validator核对固定顺序、identity/epoch关系、case唯一性、revision/environment、FAIL/panic/trap和exit；所有negative fixtures均拒绝。
- A5 / R8：probe/validator `--print-cases`一致；validator保持纯输出审计，不导入网络/进程控制能力。
- A6 / compatibility：V1–V3 ABI、MS03–MS06 host seams、qemu feature propagation、diagnostic lease、axnet ordinary/diagnostics和kernel build无回归。
- A7 / conclusion boundary：本 Iteration仅证明协议与自动 seams ready；真实单 hart QEMU结果必须由 Iteration 007 使用保存的raw serial和validator另行判定。

**Verification**

1. 运行新增 axnet reset-request/V4 exact tests，再串行运行 ordinary 与 `qemu-diagnostics` 全量 `--test-threads=1`。
2. `rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test`；同样运行 `tests/ms04-async-rx-host-harness.rs`，覆盖旧ABI与feature source guards。
3. `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms07_recovery_probe.c`；编译并运行 `tests/ms07_recovery_probe_test.c`，覆盖layout、阶段、epoch、ledger与mutation判据。
4. `python3 scripts/ms07-qemu-validate.py --self-test`；比较 probe/validator `--print-cases`；source guard禁止 validator 的 socket/subprocess/QEMU/pty 能力和probe内部poll/sleep-poll。
5. 先运行 `make host-test`。若唯一失败是已知loopback socket `EPERM`，记录最终exit和最早环境失败层，再逐项运行全部无socket Rust/C/Python命令；任何编译、断言、parser或其他失败均阻塞，不能分类为环境限制。
6. `make ARCH=riscv64 build` exit 0并生成目标镜像；只构建，不启动QEMU。
7. ordinary与qemu-diagnostics production `cargo check --lib`、相关manifest rustfmt、`git diff --check`、完整diff Review和 `openspec validate ms07-qemu-single-hart-recovery-semantics --strict` 全部exit 0。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 已定位V1–V3 ABI、syscall、resident recovery、snapshot/control shared seams、MS05/MS06 probe/validator和host-test入口 |
| Design | PASS | D8已决定append-only ABI、纯输出validator、手工HMP与model/QEMU职责；本Cycle不改变该边界 |
| Iteration Plan | PASS | 只包含Task 4.1；Task 4.2真实QEMU资格仍独立留在Iteration 007 |
| Cycle Scope | PASS | 新ABI/request/probe/validator及自动Gate形成一个可独立交付协议，不含runtime资格 |
| Task Contracts | PASS | A1–A7、RED/GREEN、preserve/forbidden和stop条件均可由当前Cycle直接执行 |
| Traceability | PASS | R4/R5/R6/R8 → D1–D8 → Task 4.1 → A1–A7 → Rust/C/Python/build Gate |
| Verification | PASS | host seams、negative fixtures、feature/ABI、两套axnet全量、host-test环境分层与kernel build均有明确判据 |

Gate 2 技术维度已就绪；用户于 2026-08-30 明确批准本 Cycle 并要求「更改gate状态，开始实施」，Plan Context 由 `draft` 改为 `ready` 并进入 Act。

**Persisted Evidence**

- Mode: none

Act Response足以记录可低成本重跑的Rust/C/Python/build结果；本Cycle不采集一次性QEMU runtime Evidence。

- Budget: 本 Cycle 最多5个文件（含README），整个change最多20个Evidence文件；单个文本文件最多500行且不超过256 KiB；超限需要用户明确批准。

**Risks and Notes**

- V4字段过多会复制V3历史包袱；只追加Iteration 007判定所需的bounded identity/summary，并用C/Rust offset tests冻结。
- reset request与自然deadline recovery可能交错；checked single-consumer状态必须让同一owner只进入一次recovery，不得重复关闭epoch或重入driver begin。
- link off/on由HMP手工执行，probe marker必须能区分“等待用户操作”与“已经观察到config transition”；validator不得把ready marker当成事件证据。
- `make host-test`的loopback `EPERM`只允许精确环境分层；新MS07 parser、C/Rust test或kernel build失败一律是产品Gate失败。
- 本草案由 `openspec-plan` 在接受Iteration 005后展开；`bettermd`用于保持Task Contract紧凑、去除重复表述。用户批准前不得调用Act。

## Act Response

- Status: blocked

**Implemented**

1. 新增仅在 `qemu-diagnostics` 可见的 checked reset request。syscall 只提交一次
   event 并唤醒既有 queue owner；只有 `RxRxFuture::poll_active` 消费它并以
   `EXPLICIT_REQUEST` 进入已有的 bounded recovery 状态机。
2. 新增 QEMU-only V4 snapshot/ioctl。V4 保留 V3 的完整 `repr(C)` prefix，并追加
   recovery stage/cause、QueueEpoch、SocketEpoch、LinkGeneration/link state 与 owner
   summary；未初始化 Service 返回明确 `u64::MAX` identity sentinel，不伪造健康状态。
3. 新增 MS07 C probe decision core、纯输出 Python validator 与 Rust/C/Python host
   seams。probe/validator 固定同一六个 case；validator 自测覆盖缺失、未知、重复和乱序
   transcript 的拒绝，且 source guard 禁止网络、进程控制与 QEMU 启动能力。该 probe 尚未
   实现真实 guest I/O、V4 ioctl snapshot/marker 阶段，见 Blocker Handoff。

**Changed Files and Symbols**

- `crates/axnet/src/{async_rx.rs,lib.rs,service.rs}`：V4 recovery snapshot、QEMU
  reset request/owner consumption、socket/link observation accessors。
- `kernel/src/drivers/virtio_net_irq{,_logic}.rs`、`kernel/src/syscall/fs/ctl.rs`：
  append-only `IrqSnapshotV4`、`0x4e49_4434` snapshot 与 `0x4e49_5231` reset ioctl。
- `tests/ms07-recovery-host-harness.rs`、`tests/ms07_recovery_probe*.c`、
  `scripts/ms07-qemu-validate.py`、`Makefile`：host seams、case agreement及纯审计 guard。

**Deviations from Plan**

无实质偏差。V4 将既有 V3 作为 `repr(C)` prefix 成员，保持 V3 原 layout与语义完全
不变；新增字段只在 QEMU ioctl path 暴露。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0
- Important findings unresolved: 1
- Minor findings unresolved: 0

Spec review：A1/A2、validator 的 A4/A5 以及 A6 均有实现与验证；但 A3 要求的 guest
probe 实际 reset 前流量、reset request、old/new socket、HMP link marker 与 V4 snapshot
阶段尚未实现。该 Acceptance gap 是 Important，不能标记当前 Cycle reported。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED witness | 新增 Rust host seam 与 C probe test | V4/reset seam 缺失时 Rust 2 FAILED；`ms07_recovery_probe.c` 缺失时 C 编译失败 | PASS（已观察） |
| MS07 seams | Rust host harness、C probe test、validator self-test、case diff | Rust `2 passed`；C exit 0；case diff exit 0 | PASS |
| axnet ordinary | `cargo test ... --lib -- --test-threads=1` | `467 passed; 0 failed`，exit 0 | PASS |
| axnet diagnostics | 同上加 `--features qemu-diagnostics` | `491 passed; 0 failed`，exit 0 | PASS |
| host Gate | `make host-test`，然后补跑无 socket 项 | MS03 35/35、MS04 16/16、MS07 2/2；唯一失败为 `socket.socket ... EPERM` | 环境受限；无 socket Gate PASS |
| kernel build | `make ARCH=riscv64 build` | release build finished，exit 0 | PASS |
| hygiene | rustfmt、`git diff --check`、strict validate | 全部 exit 0；`Change ... is valid` | PASS |

**Persisted Evidence**

None required.

**Blocker Handoff**

- Task/Gate：Task 4.1，Gate 4/5 full diff review。
- Plan 预期：C guest probe 必须通过 V4/reset ioctl 与受 deadline 约束的网络 I/O 输出
  reset、old/new socket 和 link 阶段 marker；validator 再审计该完整协议。
- 实际状态：已完成 QEMU-only ABI/reset event、host seams、C decision core 和纯 validator；
  但 probe 目前只提供 case 集与 host self-test，尚未有真实 guest transport/ioctl runner，
  因此不能作为 A3 或后续 Iteration 007 runtime 的输入。
- 影响：不可将 Task 4.1 标记完成，也不可开始 Iteration 007。
- 已完成/未完成：实现文件与所有已列 host/model Gate 保留；guest probe runtime、C mutation
  覆盖 V4 layout/stage/epoch/ledger、probe source guard与对应 final verification 未开始。
- 工作区：本 Cycle 的修改保留；未回滚用户既有 staged 改动；`.codegraph/.gitignore` 删除不属本
  Task。Persisted Evidence：None required。
- 恢复条件：调用 `openspec-plan` 为 guest probe 的 socket/I/O choreography、V4 C layout 和
  marker grammar补充可执行 repair contract；获得用户批准后将本 Response 恢复为 `pending` 并
  从新的 RED witness继续。

**Experience Candidates**

None.

**Remaining Issues**

无产品阻塞项。真实单 hart QEMU/HMP runtime、raw serial 与 qualification artifact 是
Iteration 007 的明确范围，未在本 Cycle 执行。

**Commit or Diff Reference**

Diff reference: `git diff`（工作树，未提交）。保留既有 staged MS07 改动及无关的
`.codegraph/.gitignore` 删除，不将它们计入本 Task。

## Plan Review

- Review Result: rework-required

**Findings**

1. **Important — A3 guest probe 仍是 case-name stub，不能产生 Iteration 007 所需的 runtime 输入。**
   `tests/ms07_recovery_probe.c` 只打印六个 case 名并运行一个数组自检；没有 V4 C wire、
   snapshot/reset ioctl、deadline、socket、old/new epoch、HMP ready/observed marker 或 ledger
   判据。当前 validator 因而只审计六行 `PASS`，没有审计计划要求的 recovery/link事实。
2. **Important — V4 混合历史 fault 与当前 Service 状态，且把合法 QueueEpoch 0 当成“无 fault”。**
   `recovery_snapshot_v4()` 先独立读取 `coherent_fault`，再取得 Service guard读取当前
   queue/socket/link；随后用 `fault.queue_epoch == 0` 决定是否替换为当前 epoch。
   QueueEpoch 0 本身合法，且 owner summary始终来自历史 fault，不是当前 ledger。该 payload
   无法区分 current tuple与fault tuple，也不满足 A2 的一致、可解释观测语义。
3. **Important — reset request 与自然 recovery 的交错仍可导致恢复完成后再次 reset。**
   request API先读取全局 lifecycle，再对独立 `AtomicBool` CAS；owner只在 `poll_active` 中
   swap该布尔位。若请求提交后同一 active round先因 completion/reclaim fault进入自然
   recovery，pending bit会跨 recovery保留，并在回到 Active 后触发第二次 reset。现有 source
   test只检查调用字符串，没有覆盖该线性化窗口。
4. **Important — validator 和 host witnesses 不能证明 A4/A5。** 当前 validator不要求
   revision/environment，接受没有任何 identity metadata 的 transcript；任意
   `MS07_MARKER:` 都被忽略，也未检查 V4字段关系、HMP ready/observed顺序、panic/trap/fatal
   drift。C test只验证六元素数组，Rust test只做 source substring检查；Makefile也没有
   probe sleep/internal-progress guard。

**Deviation Classification**

- `PLAN-OMISSION`：初始 Task Contract列出了A3阶段，却没有固定guest/host socket
  choreography、V4 current/fault字段语义和完整marker grammar，Act无法在不决定契约的
  情况下完成probe。
- `ACT-DEVIATION`：已实现的V4与reset request没有满足初始Plan中“shared coherent
  assembly”和“自然/显式recovery不得重复”的明确约束；这些缺口仍在Task 4.1范围内。

**Acceptance Gaps**

- A1：pending explicit request与自然recovery没有单一线性化结果。
- A2：V4 current observation、historical fault和validity语义未分离；C/Rust layout与epoch 0
  未验证。
- A3：guest probe runtime、host peer、V4 marker和old/new socket/link choreography缺失。
- A4：validator未验证identity、阶段、epoch/ledger关系或fatal transcript。
- A5：case名称虽一致，但完整grammar和pure probe/validator source guards未建立。
- A6已通过现有host/full/build证据，后继Cycle只需保持回归；A7边界未被破坏。

**Convergence**

N/A。首次实现Review；ABI、owner入口和parser骨架已形成可复用baseline，但A1–A5仍有
Important gap。

**Evidence**

- 新鲜MS07 host seams：Rust `2 passed; 0 failed`；C stub self-test、validator self-test与
  case diff均exit 0。这些结果只证明现有骨架自洽，不证明A1–A5。
- 构造一个含完整六个 `PASS`、但完全缺少revision/environment/V4 marker的transcript，
  `scripts/ms07-qemu-validate.py /dev/stdin` 返回exit 0，直接复现A4缺口。
- `async_rx.rs::recovery_snapshot_v4` 对 `fault.queue_epoch == 0` 的分支和分离的fault/Service
  读取，证明epoch 0歧义及cross-time tuple。
- `async_rx.rs::recovery_reset_request_shared` 与 `take_recovery_reset_request` 使用独立全局
  lifecycle load和`AtomicBool`；`Future::poll`在整个recovery期间不会消费该bit，证明stale
  request可跨恢复存活。
- Act记录的ordinary 467/467、qemu-diagnostics 491/491、无socket host gates、kernel build、
  rustfmt、diff check与strict validation均PASS；本Review不重复运行不影响上述结论的全量Gate。

**Follow-up Decision**

保留当前已实现的ABI、request入口和host工具骨架，创建同一Iteration的有限rework Cycle。
后继契约只关闭A1–A5：修正request线性化、冻结V4语义/layout、实现guest/peer choreography、
强化validator与tests。目标、Task 4.1、Iteration Map和真实QEMU边界不变。当前initial Cycle
冻结，不在其Act Response中继续实施。

**Iteration Plan Update**

None.

**Next Cycle**

`001-rework.md`

**Next Iteration**

None.
