# Iteration 001 / Cycle 003: Close blocked-task wake placement and UART IPI causality

## Plan Context

- Status: ready
- Iteration: 001-deterministic-uart-placement-and-observability
- Cycle: 003-rework
- Cycle Type: rework
- Parent cycle: `002-rework.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Depends on: Iteration 000
- Stable baseline: RX/TX copier 在 secondary-ready 后各以 singleton affinity 启动一次；QEMU snapshot 安全且一致地报告 placement、实际执行和 UART remote-wake 因果；SPSC、readiness 与四阶段 drain witness 闭合。
- Verification boundary: blocked-task wake placement 与通知、UART remote-wake 因果、普通/`SMP=16`/D1 build、`NET=n` UART smoke 和 `NET=y` scheduler 邻接启动。
- Diagnostic boundary: `block_on` waker、wake target selection、Blocked→Ready transition、local preempt pending、remote S_SOFT IPI 和 UART TX completion。
- Deferred tasks: 3.1–7.3

**Cycle Scope**

- Trigger: rework-required
- Acceptance gaps: A3/A6 的受控 UART TX publication 能唤醒远端 copier，但 `block_on` waker 以 `resched=false` 入队，未发送 remote-ready IPI；smoke 在首次 resume 后立即取样，尚未稳定证明 ring/staged 收敛。
- Repair items: 2.6-R3、2.6-R4
- Inherited scope: Tasks 2.1–2.6；Cycle 001 已通过的 placement、唯一 copier、readiness 与旧 TXDBG ABI；Cycle 002 已完成的 snapshot tuple/wire 安全、标准 SMP feature graph、park 后单字节 publication 和非16 SKIP。
- Excluded scope: 通用负载均衡重写、周期性抢占策略、专用 UART 调度协议、network placement/V5、正式 guest qualification、迁移控制、性能优化和真板资格。

**Objective**

让 `block_on` 挂起任务的 wake path 使用与普通 spawn 分离的目标选择：当前唤醒 hart 合法时优先本地入队，否则选择合法远端队列；仅成功的远端 `Blocked → Ready` 发送一次 reschedule IPI。以 pinned UART TX copier 的启动窗口证明 publication→remote IPI→目标 resume→completion 收敛，同时证明 full-mask network runner 不再因每次唤醒轮询迁移而形成 IPI 风暴。

**Background**

Cycle 002 已关闭首次 snapshot tuple、wire padding 和标准 `SMP=16` feature 传播缺口，并建立了正确 fail-closed 的 UART startup witness。该 witness 显示 TX copier 在 publication 后最终恢复，但 `ipi_sent/ipi_received` 保持零。独立审计确认 `AxWaker::wake_by_ref` 调用 `unblock_task(task, false)`，而 remote IPI 只位于 `resched=true` 分支。Act 曾把所有 wake 直接改为 `true`，现有 `select_run_queue` 又对 full-mask task 做全局 round-robin，导致 `axnet-stack-runner` 在 16 个 hart 间持续迁移并形成 IPI 风暴；该实验已完整回退。

这不是新需求：Task 1.4 和本 Iteration A3/A6 已要求成功的 remote ready transition 获得一次 IPI。缺失的是 blocked-task wake placement 与通知之间的可执行契约。普通 spawn 仍可使用既有 round-robin；wake 应优先唤醒者所在的合法 schedulable hart，从而让 full-mask task保持本地，而 singleton-affinity copier 必然投递到其远端目标。该规则也不禁止后续自然迁移：当唤醒 hart后来进入任务扩展后的 affinity 时，任务可迁移到该唤醒 hart。

**Investigation Facts**

- Current Baseline:
  - Cycle 002 Act Response 的 52/52 host harness、65/65 UART tests、ordinary/`SMP=16`/D1 build 和 feature-symbol 结果按用户授权采信，不重复执行。
  - 2.3-R2 与 2.3-R3 的产品 diff符合契约：tuple只返回严格空或`last ∈ mask`；152字节wire显式序列化且reserved为零；标准`SMP>1`选择root `smp → starry-kernel/smp → axtask/ipi`。
  - `SMP=16 NET=n` 已观察到单字节publication后 TX poll 1→2并最终到达shell，但IPI send/receive均为0；现有smoke因此正确FAIL。
  - 强制所有wake使用现有round-robin目标并传`resched=true`会让full-mask `axnet-stack-runner`跨hart连续迁移和发送IPI；该尝试没有留在产品diff中。
- Current-State Evidence:
  - `future::AxWaker::wake_by_ref` 先调用通用 `select_run_queue(task)`，再调用 `unblock_task(task, false)`。
  - SMP `select_run_queue_index` 使用全局seed在`task affinity ∩ schedulable`中round-robin；它同时服务普通spawn，不能直接承担wake locality语义。
  - `unblock_task` 只有在`put_task_with_state(..., Blocked, resched)`成功且`resched=true`时才处理通知；本地目标设置preempt pending，远端目标在`ipi` feature下发送一次S_SOFT IPI。重复wake不会进入该分支。
  - singleton UART TX copier的affinity只允许其固定hart；startup publisher运行在另一hart，因此local-preferred wake必须选择唯一远端目标并通知。full-mask runner允许当前唤醒hart，因此应本地入队而不产生remote IPI。
  - `tx_polls`在poll入口增长，早于ring drain完成；只等待poll delta后立即读取snapshot会造成`tx_vacancy`的时序偏差，必须另等四阶段completion收敛。
- Code and Critical Path:
  - Wake：`RingBufTx::push → AtomicWaker::wake → AxWaker::wake_by_ref → wake-target selection → AxRunQueueRef::unblock_task`。
  - Remote notification：successful `Blocked → Ready` → target queue identity vs `this_cpu_id` → `send_reschedule_ipi → S_SOFT handler → preempt pending → target copier poll`。
  - Locality guard：full-mask task + current hart schedulable/allowed → local queue；否则从`affinity ∩ schedulable`选择确定目标。
  - Completion：copier poll entry → ring pop/staged send → ring empty、copier inactive、staged zero、transmitter empty；smoke须在独立deadline内观察终态。

**Implementation Guidance**

先以纯policy tests定义wake target：优先`this_cpu_id`与`affinity ∩ schedulable`的交集；当前hart不合法时从有效交集中确定性选择；空交集fail closed。保留普通spawn的round-robin函数，新增或拆分wake专用选择入口。随后让`AxWaker`对成功unblock请求调度：本地目标只设置preempt pending，远端目标只在真实transition后发送一次IPI；不得按task名称或singleton特判。最后把UART smoke分为resume与completion两个有界等待，并以`NET=n`证明远端因果、以`NET=y`证明full-mask邻接任务不再迁移/IPI storm。

**Behavioral Change**

- `block_on`任务被唤醒时，若当前hart同时在task affinity与schedulable集合内，则进入当前run queue；否则进入一个合法目标queue。
- 成功的本地`Blocked → Ready`请求本地preempt检查但不发送IPI；成功的远端transition恰好发送一次IPI；重复或抢先wake不发送。
- 普通spawn继续使用既有round-robin，保存的task affinity不被wake policy缩小。
- UART startup smoke在同一受控publication窗口分别等待copier resume和四阶段completion收敛，不把poll入口误当作drain完成。

**Task Contracts**

### 2.6-R3: Make blocked-task wake placement local-preferred and remotely notifying

- Requirement/Scenario: A3 / Task 1.4 remote-ready基础；A6 UART远端copier唤醒。
- Depends on: None
- Targets: `crates/axtask/src/future/mod.rs::AxWaker::wake_by_ref`；`crates/axtask/src/run_queue.rs` 的wake target选择与`unblock_task`；`crates/axtask/src/tests.rs`；必要的host source guard。
- Current behavior: waker复用普通spawn的全局round-robin目标并传`resched=false`；因此pinned远端任务不发IPI，而直接传`true`会让full-mask高频任务跨hart轮询迁移并产生IPI storm。
- Required behavior: wake target先计算`affinity ∩ schedulable`；当前hart在交集内时优先当前hart，否则从交集中选择合法目标。成功本地transition只触发本地preempt pending；成功远端transition恰好发送一次IPI；重复/非Blocked wake不通知。普通spawn selection保持原行为。
- Required changes: 建立可host测试的wake-target policy与wake专用run-queue入口，令`AxWaker`请求真实reschedule；保持transition成功是唯一通知提交点。若目标集合为空，沿用现有fail-closed策略，不访问未发布run queue。
- Preserve: task affinity值、spawn round-robin、run-queue readiness Acquire/Release、S_SOFT单owner、handler不直接切task、单hart行为和后续扩展mask后向唤醒hart自然迁移的能力。
- Forbidden: 按task名称、UART类型或singleton特判；无条件全局round-robin+`resched=true`；对失败/重复transition发送IPI；在waker中阻塞等待执行；新增第二IPI owner或timer依赖。
- Test witness: pure/model tests在旧实现下先RED，覆盖full-mask当前hart优先、singleton远端选择、稀疏schedulable交集、当前hart不在affinity、空交集、local/remote/duplicate通知；source guard证明`AxWaker`不再调用通用round-robin wake并请求reschedule。
- GREEN condition: full-mask wake稳定选择合法当前hart且无remote IPI；pinned remote wake选择唯一目标并在成功transition后只发一次IPI；重复wake为零；普通spawn round-robin tests不变。
- Verification: axtask focused tests、MS04/MS08 host harness、ordinary/`SMP=16`/D1 build、`NET=y` bounded startup邻接回归。
- Stop when: local-preferred目标无法在transition前安全确定、会破坏readiness publication，或正确通知需要改变公开task/affinity API；返回Plan。

### 2.6-R4: Close the bounded UART wake and completion witness

- Requirement/Scenario: A3、A6；publication→remote-ready IPI→目标copier resume→四阶段completion。
- Depends on: 2.6-R3
- Targets: `kernel/src/drivers/uart_smp_snapshot.rs::snapshot_boot_smoke`；相关host/model witness。
- Current behavior: smoke已等待park、发布一个字节并检查resume/IPI delta，但在poll计数增长后立即读取终态，`tx_vacancy`可能尚未恢复；当前产品因缺失IPI保持RED。
- Required behavior: `SMP=16 NET=n`在独立有界阶段依次观察一个字节成功publication、IPI sent/received delta、pinned目标上的copier resume，以及ring empty、copier inactive、staged zero、transmitter empty和vacancy恢复。非16仍SKIP；任何deadline或零IPI均FAIL。
- Required changes: 在保留单producer窗口的前提下，增加completion收敛的register/recheck或有界yield观察；输出before/after delta与明确失败原因。不得用最终shell、timer tick或非UART流量替代因果。
- Preserve: 一个copier、一个pre-TTY producer、单字节有界payload、旧TXDBG与snapshot ABI、early console、D1 workaround和不panic的FAIL输出。
- Forbidden: sleep-poll、第二producer/copier、无界payload、忽略vacancy差异、仅因poll增长宣称drain、正式guest/run identity协议。
- Test witness: model/source guard覆盖resume已发生但completion未收敛的RED、零IPI、错误hart、重复wake和正确终态GREEN；现有非16SKIP witness保留。
- GREEN condition: bounded `SMP=16 NET=n`输出所有因果检查为ok并PASS，IPI delta至少1、TX poll增加、实际hart保持singleton、四阶段终态与baseline vacancy一致；0 panic/page fault/duplicate。
- Verification: focused witness、UART regression、ordinary/`SMP=16`/D1 build、bounded `NET=n`因果smoke、bounded `NET=y`邻接启动、diff check和strict OpenSpec。
- Stop when: completion无法在不引入正式guest协议的startup窗口内稳定观察，或`NET=y`仍出现wake迁移/IPI storm；返回Plan。

**Invariants**

- wake policy只选择`task affinity ∩ schedulable`；不得缩小保存的affinity或读取未发布run queue。
- 通知只跟随成功`Blocked → Ready`；local与remote互斥，重复wake没有IPI。
- axtask仍是唯一S_SOFT owner；telemetry只观测，不参与调度决定。
- 每方向只有一个UART copier；startup diagnostic publication发生在TTY writer建立前并保持TX SPSC。
- ordinary spawn负载分配、old TXDBG、snapshot 152字节wire、early console、D1 workaround和Iteration Map不变。

**Non-goals**

- 通用scheduler负载均衡、work stealing、CPU hotplug/offline或性能调优。
- 按驱动类型定制wake API、network固定placement/V5或network数据面资格。
- 正式UART guest协议、压力、迁移控制和真板IRQ/timing结论。

**Acceptance**

- A3 / 2.6-R3：blocked-task wake的合法当前hart优先；pinned远端成功transition恰好一次IPI，本地与重复wake不发送；普通spawn round-robin保持。
- A3+A6 / 2.6-R4：`SMP=16 NET=n`单字节窗口直接证明publication、IPI send/receive、目标copier resume和四阶段completion；合法非16配置仍SKIP。
- 邻接保护：`SMP=16 NET=y`不出现`axnet-stack-runner`跨hart wake storm、panic或启动停滞。
- 继承Acceptance：Cycle 002的tuple/wire与feature graph修复，以及Cycle 001的placement、唯一性、readiness和兼容结论在覆盖范围未变时继续采信。

**Verification**

- axtask pure/model：wake target locality、singleton remote、稀疏/空交集、transition通知exactly-once；普通spawn round-robin回归。
- MS04/MS08 host harness：wake source guard、snapshot wire/tuple、smoke状态机与非16SKIP。
- `uart_16550 --features async`：ring/waker/readiness/drain邻接回归。
- ordinary、标准`SMP=16`与D1 build；feature graph仍只有axtask IPI owner。
- bounded `make justrun SMP=16 NET=n`：UART smoke PASS并到达后续启动，禁止panic/page fault/duplicate。
- bounded `make justrun SMP=16 NET=y LOG=info`：无持续`axnet-stack-runner`跨hart迁移/IPI日志风暴，并越过network startup；不把该运行当network placement资格。
- `git diff --check`与`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`。

**Gate 2 Readiness**

- No Missing requirements: PASS — 唯一剩余A3/A6缺口映射到wake placement/notification与UART completion witness。
- Simplified requirements approved: PASS — 没有裁剪remote IPI、completion、placement或兼容要求。
- Investigation complete: PASS — 已定位`AxWaker`、通用round-robin、`unblock_task`通知门和poll-before-drain时序。
- Design closed: PASS — local-preferred wake target、成功transition通知和spawn/wake分离已明确；没有把关键选择留给Act。
- Tasks executable: PASS — 两项repair均给出targets、RED/GREEN、保持/禁止和停止条件。
- Iteration plan ordered and balanced: PASS — 只关闭Iteration 001既有因果缺口，network仅作为全局scheduler改动的邻接回归。
- Traceability complete: PASS — A3/A6、Task 1.4基线、代码路径和runtime witness形成闭环。
- Verification sufficient: PASS — pure policy、transition seam、UART因果与`NET=y`反风暴分别观察目标行为。
- No identity-style evidence engineering: PASS — 不新增revision/run/session协议。
- No material TBD for Act: PASS — wake目标优先级、通知提交点和completion终态均已确定。
- Change/Cycle consistency: PASS — 不修改delta spec、design、tasks或Iteration Map。
- Persisted Evidence mode valid: PASS — 结果可低成本重跑，由Act Response承载。
- User plan approval: PASS — 用户于2026-09-18以“批准”明确批准本Cycle。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、改动文件和符号写入Act Response；不创建Evidence目录。

**Risks and Notes**

- local-preferred wake改变的是blocked-task再入队策略，不改变普通spawn策略；高频full-mask任务的locality与pinned任务的remote notification都必须有直接witness。
- `NET=y`只验证scheduler邻接与无storm，不提前取得network placement或数据面资格。
- QEMU S_SOFT结果只覆盖当前虚拟平台；不外推到D1/K3真板中断路由或timing。

## Act Response

- Status: reported

**Implemented**

- **2.6-R3 (wake routing, `axtask`)**: introduced the pure, host-testable local-preferred wake target policy `select_wake_cpu(task, schedulable, current) -> Option<usize>`: it prefers the current hart when it is in `task ∩ schedulable`, otherwise picks the lowest legal member of the intersection, and returns `None` (fail closed) on an empty intersection — it never selects an uninitialized queue or an offline current hart. Added the wake-specific run-queue entry `select_wake_run_queue` (non-SMP: current queue; SMP: `select_wake_cpu` + `get_run_queue`) and re-exported it. `AxWaker::wake_by_ref` now routes the wake through `select_wake_run_queue` and requests a real `Blocked -> Ready` transition via `unblock_task(task, true)`, so a genuinely remote enqueue sends exactly one remote-ready IPI (the local branch only sets preempt pending). Reverted the earlier global-`resched=true` storm by keeping wake locality separate from the plain-spawn round-robin (`select_run_queue` / `select_schedulable_cpu` unchanged).
- **2.6-R4 (UART completion convergence)**: `snapshot_boot_smoke` now adds a bounded Phase 5 that, in the same single-producer pre-TTY window, waits for the four-stage drain (`TxCompletion::is_drained`: ring empty, copier inactive, staged zero, transmitter empty) *and* the ring vacancy restored to its pre-publication baseline before reading the terminal snapshot. `tx_ring_converged` therefore reflects a converged state instead of sampling right after the poll-count delta.

**Changed Files and Symbols**

- `crates/axtask/src/run_queue.rs` — `select_wake_cpu` (new), `select_wake_run_queue` (new); callers unchanged for `select_run_queue`/`select_schedulable_cpu`.
- `crates/axtask/src/future/mod.rs` — `AxWaker::wake_by_ref`: `select_wake_run_queue` + `unblock_task(task, true)`.
- `crates/axtask/src/api.rs` — re-export `select_wake_run_queue`.
- `crates/axtask/src/tests.rs` — 6 new `wake_target_*` pure tests.
- `kernel/src/drivers/uart_smp_snapshot.rs` — `snapshot_boot_smoke` Phase 5 convergence wait.
- `tests/ms04-async-rx-host-harness.rs` — `axtask_waker_uses_wake_specific_routing_and_requests_reschedule`, `uart_smp_smoke_waits_for_completion_convergence_before_terminal_read`, `completion_guard` module + RED/GREEN fixtures.

**Deviations from Plan**

- None in contract. Process note: `make lichee build` overwrites the shared `.axconfig.toml`/kernel binary with the D1 `max-cpu-num=1` config; `make build SMP=16` must be re-run before a bounded `SMP=16` QEMU run so the booted binary actually has `max-cpu-num=16` (an environmental rebuild-order note, not a code change).
- axtask host unit tests are run against a scratch `AX_CONFIG_PATH=/tmp/axtask-test-config.toml` with `[plat] max-cpu-num=64` (so `one_shot(63)` is constructible), matching the accepted Iteration 000 method.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS — repair items 2.6-R3 and 2.6-R4 implement the full behavioral change: local-preferred wake target, exactly-once IPI on a genuine remote transition, UART completion convergence before the terminal read.
- Full diff reviewed: PASS — changes are confined to the wake-routing seam in `axtask` and the UART smoke/completion witness; no plan-external code touched; the pre-existing Iteration 000/001 uncommitted work is left intact.
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0 (the axtask non-SMP lib build emits the same pre-existing dead-code warning on `select_schedulable_cpu`; `select_wake_cpu` parallels it and is not enabled in non-SMP builds — a warning only, consistent with existing code)

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| axtask 纯策略（RED） | `cargo test --features multitask,test,smp wake_target`（先无 `select_wake_cpu`） | `error[E0425]: cannot find function select_wake_cpu` ×11 | wake 目标策略 seam 不存在 | RED（编译失败见证） |
| axtask 纯策略（GREEN） | 同上（已实现） | `6 passed; 0 failed` | local-prefer、pinned-remote、稀疏、offline-fallback、空交集、不越界不修输入 | PASS |
| axtask 套件回归 | `cargo test --features multitask,test` 与 `...,smp -- --test-threads=1` | 各 `22 passed; 0 failed` + 1 doctest | 基线 16 + 新增 6，无回归 | PASS |
| ms04 host harness | `rustc --test tests/ms04-async-rx-host-harness.rs` + `--test-threads=1` | `56 passed; 0 failed` exit 0 | 旧 guards + 新增 3 个 guard/witness | PASS |
| UART crate | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` | `65 passed; 8 passed; 10 passed` exit 0 | ring/waker/readiness/drain 邻接回归 | PASS |
| 标准 SMP=16 build | `make build SMP=16` | `Finished release ... exit 0` | kernel `smp` + `axtask/ipi` + smoke 编译 | PASS |
| 普通 build | `make build` | `Finished release ... exit 0` | 单 hart 产品路径编译 | PASS |
| D1 build | `make lichee build` | `Finished release ... exit 0` | `lichee-d1` 特性编译 | PASS |
| QEMU UART smoke | bounded `make justrun SMP=16 NET=n` | `[UART-SMP-SMOKE] PASS`；`ipi_sent=1 ipi_received=1`；`tx_ring_converged ok`；`tx_vacancy=65536 == before`；`staged=0`；`tx_still_on_pinned_hart ok`；`starry:~#` | publication→remote-ready IPI→resume→四阶段收敛；到达 shell，0 panic | PASS |
| QEMU 邻接 | bounded `make justrun SMP=16 NET=y LOG=info` | `[UART-SMP-SMOKE] PASS`；0 个 `stack_runner`/panic 引用；`starry:~#` | full-mask network runner 无迁移/IPI 风暴，越过 network startup | PASS |
| diff 卫生 | `git diff --check` | rc=0 | 无空白错误 | PASS |
| strict OpenSpec | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | `Change ... is valid` | change 结构 | PASS |

**Persisted Evidence**

None required (Cycle Persisted Evidence mode: `none`; every acceptance is observably re-runnable and carried by this response).

**Experience Candidates**

None.

**Remaining Issues**

None.

**Commit or Diff Reference**

None (working tree carries the accumulated Iteration 000/001 + this Cycle's changes; no commit made).

## Plan Review

- Review Result: accepted

**Findings**

No blocking findings.

- `select_wake_cpu` selects only `task affinity ∩ schedulable`, prefers a legal current hart, and returns `None` for an empty intersection. `select_wake_run_queue` is separate from ordinary spawn round-robin and never dereferences an unpublished run queue.
- `AxWaker::wake_by_ref` uses the wake-specific selector and requests `unblock_task(task, true)`. `unblock_task` sends a remote IPI only after `put_task_with_state(..., Blocked, ...)` succeeds; local and duplicate wakes cannot enter the remote-send branch.
- Ordinary `select_run_queue` and `select_schedulable_cpu` behavior is unchanged, so the fix does not reintroduce the full-mask runner migration/IPI storm seen in Cycle 002.
- `snapshot_boot_smoke` now waits separately for copier resume and for `TxCompletion::is_drained()` plus restoration of the pre-publication vacancy before reading the terminal snapshot. The bounded wait preserves the pre-TTY single-producer window and reports failure without panic.
- The implementation preserves the 152-byte UART snapshot ABI, existing TXDBG ABI, early console, D1 workaround, singleton copier identity, and the single S_SOFT owner. QEMU evidence remains limited to the QEMU NS16550/SMP=16 environment.
- Minor, non-blocking: several comments say a successful remote transition “produces exactly one IPI”; this is true in the standard SMP build where `axtask/ipi` is enabled, while builds without that feature compile out the send. The Cycle acceptance and runtime qualification explicitly use the standard SMP feature graph, so this wording does not create an Acceptance gap.

**Deviation Classification**

None. The implementation follows the approved repair contracts; the feature-qualified comment noted above is non-blocking.

**Acceptance Gaps**

None.

**Convergence**

reduced — the parent Cycle 002 gaps (zero remote-ready IPI and premature completion sampling) are both closed.

**Evidence**

- Independent code review: `crates/axtask/src/future/mod.rs::AxWaker::wake_by_ref`, `crates/axtask/src/run_queue.rs::{select_wake_cpu, select_wake_run_queue, AxRunQueueRef::unblock_task}`, `kernel/src/drivers/uart_smp_snapshot.rs::snapshot_boot_smoke`, and the corresponding axtask/MS04 host witnesses.
- Diff review: current Cycle changes are confined to the approved wake-routing, UART completion-witness, and test surfaces; preceding Iteration 000/001 changes remain present in the accumulated worktree.
- User instruction on 2026-09-18 states that the reported test results are trusted. The Cycle 003 Act Response results are therefore adopted without rerun: axtask focused and full suites, 56/56 host harness, UART suites, ordinary/SMP=16/D1 builds, `NET=n` UART smoke, `NET=y` adjacency boot, diff check, and strict OpenSpec validation all report PASS with exit 0 or explicit PASS output.
- Coverage freshness check: the reviewed code is the same accumulated worktree described by the Act Response; no contradictory code or evidence was found during review.

**Follow-up Decision**

Accept Cycle 003 and complete Iteration 001. The implementation satisfies the existing A3/A6 Acceptance without changing requirements, design, verification boundaries, or the Iteration Map. Expand the already-planned Iteration 002 for deterministic network placement and observability; do not begin implementation until its draft plan is approved.

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

`../002-deterministic-network-placement-and-observability/000-initial.md`
