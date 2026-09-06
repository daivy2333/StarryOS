# Iteration 002 / Cycle 001: Prove Coherent Fault Publication Ordering

## Plan Context

- Status: ready
- Iteration: 002-data-stage-deadlines-and-coherent-fault-identity
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 2.2
- Depends on: Iteration 001 accepted；Cycle 000 已满足 A1、A2、A3、A5，并完成 A4 的有界读取与确定性 seam。
- Stable baseline: submit、completion、reclaim 各有独立 1s absolute deadline；fault identity 在弱内存序下也只能读取完整提交。
- Verification boundary: coherent publication source guard、deterministic seam、focused tests、两个 axnet 串行全量和 production check 全部通过。
- Diagnostic boundary: `CoherentFaultSheet` 的 generation/field 原子序与对应 tests。
- Deferred tasks: 2.3–4.2

**Cycle Scope**

- Trigger: rework-required；Cycle 000 的 coherent publication 连续三次实现仍未闭合弱内存序。
- Acceptance gaps: A4 的 RISC-V coherent publication ordering。
- Repair items: 2.2-R1
- Inherited scope: R4、D5、Task 2.2；Cycle 000 已实现的真实 epoch、单 guard identity、有界 `None` defer、确定性 in-progress seam 和 A1–A3/A5 行为。
- Excluded scope: data deadline 行为重写、公开 V1–V3 ABI、driver recovery lifecycle、SMP 扩展、性能优化及后续 Iteration。

**Objective**

用一个方向无歧义、可从 Rust 原子内存模型直接证明的协议关闭 A4：reader 返回 `Some` 时六个字段必定属于同一已完成 publication；writer 被抢占时 reader 在有限尝试后返回 `None`。

**Background**

Cycle 000 的三次实现依次为：字段后单次 generation 更新；opening Relaxed 的无界 odd/even seqlock；有界但使用 opening Release、字段 Relaxed、尾部 Acquire 的 seqlock。第三种组合仍错误依赖 Release 约束后续 store、Acquire 约束此前 load。Gate 6 禁止第四次盲试，因此本 Cycle 固定保守的 SeqCst 设计，不再把 ordering 选择留给 Act。

**Current Baseline**

- Revision：`2a303eaa3d0b2dc3044b32c22eeb5e49a355bbf5`；产品改动尚未提交，审计覆盖 staged + unstaged worktree。
- `CoherentFaultSheet::read` 已以 `READ_BOUND = 2` 有界返回 `Some(identity) | None`。
- `mark_in_progress` 和 `finish_in_progress` 使用 Release RMW；六个字段读写为 Relaxed；两次 generation read 为 Acquire。
- 确定性 test seam 已证明 odd 保持时 reader 返回 `None`，完成 even 后返回新 tuple；stress 每轮在不同 identity 间转换。
- 新鲜 Gate：coherent 3/3、ordinary 412/412、qemu-diagnostics 436/436、两个 production check、scoped rustfmt、diff check 与严格 OpenSpec validation 均通过。通过结果不证明当前弱序协议正确。

**Current-State Evidence**

1. `mark_in_progress` 的 Release 只发布它之前的访问，不能把 odd marker 排在后续字段 store 之前。
2. 第二次 generation Acquire 只约束它之后的访问，不能把此前字段 load 固定在验证点之前。
3. `g1 == g2` 的旧 even 仍可与部分提前可见的新字段组合；host stress 和单 hart QEMU 都不能证明该交错不存在。
4. fault publication/read 是低频诊断路径；使用 SeqCst 不改变公开 ABI、owner 状态或 data deadline 行为，也不需要新增依赖。

**Relevant Code**

- `crates/axnet/src/async_rx.rs::{CoherentFaultSheet,READ_BOUND}`：唯一产品修改面。
- 同文件 tests：`coherent_fault_sheet_in_progress_defer_is_bounded_and_non_blocking`、`coherent_fault_sheet_never_returns_torn_tuple_mid_publication`、`coherent_fault_sheet_reads_only_whole_identities` 及新增 ordering source guard。

**Critical Path**

```text
single writer
  SeqCst odd generation
  SeqCst six field stores
  SeqCst even generation

bounded reader
  SeqCst g1; reject zero/odd
  SeqCst six field loads
  SeqCst g2
  g1 == g2 && even -> Some(full identity)
  otherwise, bounded retry then None
```

所有相关操作进入同一个 SeqCst total order，并保持线程内程序顺序。若 reader 的两次 generation load 返回同一 even 值，则 writer 的 odd、字段和 even 序列不能插入两次验证之间；因此 `Some` 只能对应一个完整 publication。若 writer 正在发布，reader 观察 odd 或 generation mismatch，并有界返回 `None`。

**Implementation Guidance**

保留当前数据结构、两趟验证和 `READ_BOUND`。把 generation 的 opening/closing RMW、reader 两次 generation load、六字段 store/load 全部改为 `Ordering::SeqCst`，并同步修正文档注释。该路径低频且内部使用，以可证明性优先于更弱 ordering 的微优化。

**Behavioral Change**

外部行为和返回类型不变。变化仅是 `Some(identity)` 在 Rust/RISC-V 弱内存序下具备完整 tuple 保证；in-progress 或竞争 publication 仍有界返回 `None`。

**Change Surface**

| Task/Repair | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.2-R1 | R4/D5/A4 coherent identity | `async_rx.rs::CoherentFaultSheet` 及 tests | 有界 publication/read，但 ordering 方向不足 | 固定全 SeqCst 协议并增加源码见证 |

**Task Contracts**

### 2.2-R1: Close weak-order coherent publication

- Requirement/Scenario: R4、D5、A4；publication 前、publication 中、publication 后读取。
- Depends on: Cycle 000 已完成的单 writer、真实 identity capture、有界 reader 与 test seam。
- Targets: `crates/axnet/src/async_rx.rs::{CoherentFaultSheet,READ_BOUND}` 及同文件 tests。
- Current behavior: reader 有界，但 Release/Acquire/Relaxed 组合不能证明 odd/fields/even 与两次验证的全序关系。
- Required behavior: 所有参与 coherent sheet publication 和 read validation 的 generation 与字段原子操作使用 SeqCst；`Some` 只返回完整 identity，竞争时有限尝试后返回 `None`。
- Required changes: generation opening/closing RMW、reader generation loads、六字段 stores 和 loads 全部改为 `Ordering::SeqCst`；更新错误的 Release/Acquire 注释；保留 `READ_BOUND = 2` 和现有 seam。
- Preserve: 单 writer 假设、`Option` 语义、真实 epoch/owner capture、legacy telemetry、A1–A3/A5、V1–V3 布局、guard 外 wake。
- Forbidden: 使用局部 fence、Acquire/Release/Relaxed 替代本 Cycle 固定的 SeqCst 协议；新增锁或依赖；扩大到 SMP/runtime/后续 Iteration；修改 Plan Context 或父 Cycle。
- Test witness: 在同文件新增 source guard，仅截取 `CoherentFaultSheet` impl，变更前因仍含 `Ordering::{Relaxed,Acquire,Release}` 而 RED；变更后确认 publication/read 路径只使用 `Ordering::SeqCst`。保留确定性 odd pause test 与 a↔b stress。
- GREEN condition: source guard、3 个 coherent behavior tests 均通过；完整 ordinary 与 qemu-diagnostics suites 无回归。
- Verification: focused coherent filter；ordinary 及 qemu-diagnostics `--lib -- --test-threads=1`；两组 production check；scoped rustfmt；`git diff --check`；严格 OpenSpec validation。
- Stop when: SeqCst 在目标上不可用、source guard 无法只覆盖目标 impl、必须改变数据结构/接口或任何 A1–A5 行为；记录 Blocker Handoff 返回 Plan，不选择第四种 ordering 协议。

**Invariants**

- reader 不等待可能被抢占的 writer；任何 retry 都受 `READ_BOUND` 限制。
- `Some` 不能混合不同 fault 的 stage、cause、epoch 或 owner fields。
- fault identity 仍在一个 Service guard observation 中形成；公开 V1–V3 ABI 不变。
- 本 Cycle 不改变 deadline、ticket、flush、recovery 或 wake ownership。

**Non-goals**

- 不优化 coherent telemetry 的原子开销。
- 不证明 SMP driver runtime 或真实硬件时序；只证明 Rust 原子协议的跨 hart 语义。
- 不清理其他 Relaxed/Acquire/Release 用法或 baseline warning。

**Acceptance**

- A4：source guard 证明 coherent sheet 的 generation 和六字段 publication/read 均使用固定 SeqCst；确定性 seam 证明 odd 时有界 `None`、even 后完整 `Some`；stress 只接受完整 `a` 或 `b`。
- A1、A2、A3、A5：继承 Cycle 000 的已通过证据，并由两个全量 suite 证明无回归。

**Verification**

- `env RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- coherent_fault_sheet --test-threads=1`
- 同一 test 命令移除 filter，ordinary 和 `--features qemu-diagnostics` 各串行运行一次。
- `cargo check --manifest-path crates/axnet/Cargo.toml --locked --offline`，ordinary 与 qemu-diagnostics 各一次。
- `rustfmt --edition 2024 --check crates/axnet/src/async_rx.rs`
- `git diff --check`
- `openspec validate ms07-qemu-single-hart-recovery-semantics --strict`

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 目标 impl、错误 ordering 方向、测试与唯一产品文件已定位。 |
| Design | PASS | 全 SeqCst total order 直接闭合 odd/fields/even 与 reader 验证，不留 ordering 选择。 |
| Iteration Plan | PASS | 仍服务 Task 2.2/A4；Iteration 目标、依赖与边界不变。 |
| Cycle Scope | PASS | 只有本地 repair item 2.2-R1，不扩大 change task。 |
| Task Contracts | PASS | 目标符号、固定修改、RED/GREEN、禁止项和停止条件完整。 |
| Traceability | PASS | R4/D5/A4 → 2.2-R1 → CoherentFaultSheet → source/behavior tests。 |
| Verification | PASS | focused、双 full suite、双 check 与结构校验覆盖该修改。 |

Gate 2 技术检查全部 PASS；状态保持 draft，等待用户批准本 rework Cycle 后才能改为 ready 并交给 Act。

**Persisted Evidence**

- Mode: none
- Budget: 本 Cycle 最多 5 个文件（含 README），整个 change 最多 20 个 Evidence 文件；单个文本文件最多 500 行且不超过 256 KiB；超限需要用户明确批准。

**Risks and Notes**

- SeqCst 比当前 ordering 更保守，但 fault telemetry 为低频路径，本 Iteration 不接受无证据的弱化优化。
- 单 hart QEMU pass 不能单独证明跨 hart ordering；本 Cycle 的结论来自 Rust SeqCst 模型、源码见证和行为测试的组合。

## Act Response

- Status: reported

### Actual Changes

- Repair item 2.2-R1 完整实施：`CoherentFaultSheet` publication/read 协议的全部原子操作固定为 `Ordering::SeqCst`——generation opening/closing RMW（`mark_in_progress`/`finish_in_progress`）、reader 两次 generation load（`read` 的 g1/g2）、六字段 store（`write_fields`）与 load（`snapshot_fields`）、`publish` 内 debug_assert generation load，共 15 处 ordering 修改；`READ_BOUND = 2`、数据结构、控制流、Option 语义与公开 ABI 不变。
- 修正 5 处记录 Release/Acquire 语义的文档注释（struct doc、`publish`、`write_fields`、`finish_in_progress`、`read`）：Ordering argument 重写为 SeqCst total-order 论证（两次 generation load 返回同一 nonzero even 值时，writer 的 ODD/字段/EVEN 序列无法插入其间）。`impl` 外的测试与 telemetry 注释未动。
- 新增 source guard 测试 `coherent_fault_sheet_publication_uses_fixed_seqcst_protocol`：`include_str!` 读取自身源文件，括号配对截取 `impl CoherentFaultSheet` 块，断言其中不含 `Ordering::{Relaxed,Acquire,Release}` 且存在 `Ordering::SeqCst`；guard 只覆盖 impl，文件内其他 Relaxed telemetry 不在范围。

### Files and Symbols

- `crates/axnet/src/async_rx.rs`：`CoherentFaultSheet::{publish,mark_in_progress,write_fields,finish_in_progress,read,snapshot_fields}`、struct doc、tests 模块新增 1 个 guard 测试；净变化 +86/−39 行。
- `001-rework.md`（本文件）：Plan Context Status draft→ready（用户批准记录）+ 本 Response。
- 其余工作区（Cycle 000 staged 产品改动、000-initial.md）未触碰。

### Deviations from Plan

- `publish` 内 debug_assert 的 generation load 一并改为 SeqCst：契约要求 generation loads 全部 SeqCst 且 source guard 覆盖整个 impl 块，保留 Relaxed 会使 guard 失败；该 load 为同线程 debug 观察，改 SeqCst 无行为影响。
- rustfmt 将 guard 测试的 forbidden 数组与 assert 消息换行展开以满足行宽：仅格式，无语义变化，修正后 focused 测试复跑通过。
- 无其他偏差；全部验证命令与 Plan Verification 逐字一致。

### Self-Review

- Spec review：对照 2.2-R1 契约逐项核对 Required changes/Preserve/Forbidden/GREEN condition，全部满足；A4 三个证明面（source guard、确定性 odd-pause seam、a↔b stress）均有新鲜证据；A1/A2/A3/A5 继承 Cycle 000 证据并由两个全量 suite 证明无回归。
- Code quality review：完整 unstaged diff 只含计划内修改；`cargo check` ordinary/diag 警告计数与 Cycle 000 基线一致（`tx_submit_calls_for_test` dead_code 为既有基线警告，Plan Non-goals 明确不清理）；行为测试未因错误原因通过——行为测试在 host 上无法区分弱序 bug（Cycle 000 三次失败的教训），本 Cycle 由 source guard + SeqCst total-order 论证补上该盲区。
- 已修复发现：rustfmt 行宽（见 Deviations）。
- 遗留 Minor：guard 的括号配对对 impl 块内未来出现的不配对花括号敏感；发生时守卫以 `expect` panic 显式失败而非静默通过，可接受。debug_assert load 语义上可保持 Relaxed，为 guard 一致性统一为 SeqCst，已记录。

### Verification

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| Test witness RED | focused coherent（产品修改前） | `publication_uses_fixed_seqcst_protocol ... FAILED ... found Ordering::Relaxed`；`3 passed; 1 failed` | RED 已见证 |
| Focused GREEN | `env RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- coherent_fault_sheet --test-threads=1` | `test result: ok. 4 passed; 0 failed`，exit 0 | PASS |
| Ordinary 全量 | 同命令去 filter（`-- --test-threads=1`） | `test result: ok. 413 passed; 0 failed`，24.41s，exit 0 | PASS |
| qemu-diagnostics 全量 | `... --features qemu-diagnostics -- --test-threads=1` | `test result: ok. 437 passed; 0 failed`，21.90s，exit 0 | PASS |
| Production check ordinary | `cargo check --manifest-path crates/axnet/Cargo.toml --locked --offline` | `Finished dev profile`，exit 0 | PASS |
| Production check diag | 同上加 `--features qemu-diagnostics` | `Finished dev profile`，exit 0 | PASS |
| 格式化 | `rustfmt --edition 2024 --check crates/axnet/src/async_rx.rs` | 无输出，exit 0（格式化修正后复验） | PASS |
| diff 卫生 | `git diff --check` | 无输出，exit 0 | PASS |
| 结构校验 | `openspec validate ms07-qemu-single-hart-recovery-semantics --strict` | `Change 'ms07-...' is valid`，exit 0 | PASS |

格式化修正后 focused 测试与 `git diff --check` 已复跑确认仍通过；两个全量 suite 的 413/437 计数含新增 guard 测试（基线 412/436 + 1）。

### Persisted Evidence

- None required（Plan Mode: none；全部验证可低成本重跑，无白名单触发项；未创建 `evidence/` 目录）。

### Experience Candidates

- None。单一 ordering 修复无可复用的高风险端到端操作路径或故障事件；Cycle 000 三次弱序失败的诊断过程已由 Plan Context Background 与本 Cycle 记录保留。

### Open Issues

- 无。Task 2.2 整体验收（A1–A5）与 Iteration 002 完成判定留待 Plan Review；本 rework Cycle 仅闭合 2.2-R1/A4。

### Diff Reference

- 未提交。本 Cycle 审计范围 = unstaged delta（`git diff`：`async_rx.rs` +86/−39、本文件状态与 Response）叠加 Cycle 000 staged 基线（revision `2a303eaa`）。

## Plan Review

- Review Result: accepted

**Findings**

None blocking。

1. **Minor — Act Response 的机械统计不准确。** `CoherentFaultSheet` impl 当前共有 17 处
   `Ordering::SeqCst`，不是 Response 所写的 15 处；本 Cycle unstaged `async_rx.rs` diff 为
   `+85/-38`，不是 `+86/-39`。实际代码、source guard 和验收不受影响。
2. **Minor — source guard 是协议回归护栏，不是独立内存模型证明。** 它能阻止 impl 内重新出现
   Relaxed/Acquire/Release；正确性仍来自所有 generation/field 操作进入同一 SeqCst total
   order，并由确定性 seam 验证 bounded defer。当前代码满足该组合证据。

**Deviation Classification**

- `NEW-EVIDENCE`：独立代码 Review 与新鲜 Gate 证明 2.2-R1 已关闭父 Cycle 的 A4 缺口。
- 两项统计/证据措辞属于非阻塞 Minor，不构成实现偏差。

**Acceptance Gaps**

None。

- A4 满足：generation opening/closing、reader 两次验证和六字段读写均为 SeqCst；reader 至多
  两次尝试，odd/mismatch 返回 `None`，`Some` 只能对应同一完整 publication。
- A1、A2、A3、A5 保持满足，完整 ordinary 与 qemu-diagnostics suite 未见回归。

**Convergence**

Closed。父 Cycle 唯一剩余的弱内存序缺口已由固定 SeqCst 契约关闭；Iteration 002 达到稳定
baseline。

**Evidence**

- 源码：`crates/axnet/src/async_rx.rs:285-410`；完整 staged + unstaged worktree 已审查。
- focused coherent：4 passed、0 failed，exit 0。
- focused recovery baseline：13 passed、0 failed；唯一 spawn seam：1 passed、0 failed。
- ordinary：413 passed、0 failed；qemu-diagnostics：437 passed、0 failed，均串行 exit 0。
- ordinary 与 qemu-diagnostics production check 均 exit 0。
- scoped rustfmt、`git diff --check` 和严格 OpenSpec validation 均 exit 0。
- Persisted Evidence：None required；没有 Evidence 目录不是 finding。

**Follow-up Decision**

无需当前 Cycle 修复。Iteration 002 accepted；按既有 Iteration Map 展开 Iteration 003 draft，
只验收常驻 owner 与 quiesce/reset/reinitialize driver stages，不提前实施或接受 link/socket/QEMU
runtime 工作。

**Iteration Plan Update**

None。目标、依赖、验证契约和 Acceptance 保持既有 Map。

**Next Cycle**

None。

**Next Iteration**

`../003-resident-owner-and-driver-stage-recovery/000-initial.md`
