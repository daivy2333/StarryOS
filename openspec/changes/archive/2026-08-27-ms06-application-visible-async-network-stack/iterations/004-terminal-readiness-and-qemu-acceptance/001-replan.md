# Iteration 004 / Cycle 001: terminal ownership and I/O recheck closure

## Plan Context

- Status: ready
- Approval: accepted — 用户于 2026-08-26 认可 Cycle 000 审计并要求创建下一 Cycle、更新 Iteration Map
- Iteration: 004-terminal-readiness-and-qemu-acceptance（逻辑范围：terminal-readiness-closure）
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 3.1-3.2
- Depends on: Iteration 003 accepted
- Stable baseline: socket-local与global terminal分别first-wins；global先提交再唤醒全部已有bridge，late socket通过effective snapshot观察同一code；blocking/nonblocking send、recv、connect和accept在fatal前后返回同一映射类别。
- Verification boundary: local-before-global、0/1/2/64/65 waiter、add/install并发、fault-during-wait、connect recheck、listener Reset consume-once、normal EOF/HUP和MS05 fatal/flush由host/model tests覆盖；ordinary与qemu-diagnostics full suites和直接产品build通过。
- Diagnostic boundary: terminal state ownership、registry snapshot/wake、TCP/UDP I/O attempt、connect/listener error映射或poll_io register-recheck。
- Deferred tasks: Iteration 005 Tasks 4.1-4.3；Iteration 006 Task 5.1；Iteration 007 Tasks 6.1-6.2

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: Cycle 000的global/local terminal事实源冲突；已有local terminal时global wake可被跳过；UDP blocking recv的poll_io重试不读terminal；connect wake与local error提交的线性化语义未定义；global fatal的0/1/2/64/65和fault-during-wait见证缺失。
- Repair items: None
- Inherited scope: R3-R6；D5-D9；accepted Iterations 000-003；唯一runner；per-socket bridge；Faulted no-fallback；normal EOF/HUP；listener Reset一次性消费；MS05 queue/flush owner契约
- Excluded scope: guest probe、marker validator、完整自动资格、人工QEMU、Linux destructive SO_ERROR、reset/cancellation、scheduler、SMP、真板、性能、全局文档维护、归档和commit

**Objective**

关闭host/model层的terminal ownership和I/O register-recheck缺口，使稳定data-plane fatal在任何global publication wake前提交，并使每个公共socket操作在调用前已存在fatal或等待期间出现fatal时都返回同一稳定映射错误。

**Background**

Cycle 000实现了共享DevError编码、wrapper global terminal、bridge terminal、TCP/UDP overlay、listener Reset readiness和具体queue fault传播。用户要求在Task 3.1后停止。Plan Review确认两profile全绿，但原Cycle把四个故障域放在同一Iteration，且Task 3.1的状态所有权和I/O重试仍有Acceptance gap，因此更新Map并在本Cycle只闭合terminal host/model基线。

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3`。
- Task 3.1实现已存在于11个staged axnet文件；Cycle 000 Act Response为`blocked`，原因是用户授权边界。
- 新鲜基线：ordinary 348/348、qemu-diagnostics 368/368，exit 0；`git diff --cached --check` exit 0。
- `.claude/docs/SNAPSHOT.md`和`openspec/specs/knowledge/spec.md`存在与本Cycle无关的unstaged修改；Act不得触碰。
- Cycle 000 Persisted Evidence为`none`；本Cycle不要求补建历史Evidence。

**Current-State Evidence**

- `ReadinessBridge::terminal_code`是单个first-wins原子。`commit_terminal_and_wake`只有在该原子从0提交成功时才wake。
- `SocketSetWrapper::publish_global_fault_code`先提交wrapper global code，再快照registry，随后对每个bridge调用`commit_terminal_and_wake(global)`。如果bridge已有local connect error，该调用不会wake。
- TCP/UDP的`terminal_code()`每次组合`SOCKET_SET.global_terminal_code()`和bridge local code，global优先；因此I/O事实源与bridge内保存值可能不同。
- `UdpSocket::recv`只在进入函数时调用`observe_terminal_error()`；`GeneralOptions::recv_poller`反复调用的闭包不再检查terminal。fatal第一次wake后，闭包仍可返回`WouldBlock`并重新Pending。
- UDP send在解析remote和隐式bind后才检查terminal；TCP connect在状态、route和smoltcp提交后才检查；TCP recv和accept也有terminal前置检查之前的本地状态分支。调用前已存在global fatal可被其他错误或状态修改抢先观察。
- `DirectionNotify`直接wake方向和terminal PollSet。它是smoltcp状态变化hint，无法在回调内推导connect error；`poll_connect`在application recheck时才提交local error并报告`OUT|ERR`。
- 现有global publication测试覆盖单waiter、重复publish、late add/install和add/publish交错；64/65覆盖普通read transition，不覆盖已有local terminal后的global fan-out。

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `readiness.rs::ReadinessBridge` | read/write/terminal PollSet与socket-local code | local code保持first-wins；提供与global wake分离的API和测试seam |
| `wrapper.rs::SocketSetWrapper` | public handle registry与global code | global CAS后快照、解锁、无条件wake；late socket通过wrapper global被观察 |
| `async_rx.rs::RxRxFuture::publish_fatal` | RX/arm fatal lifecycle与publisher入口 | 保持concrete DevError和Faulted-before-global-publication |
| `router.rs`、`service.rs`、`stack_runner.rs` | TX/RX round fault code传播 | 保持code不折叠，并在Service guard释放后调用global publisher |
| `tcp.rs::terminal_code/observe_terminal_error/poll_connect` | TCP effective terminal和连接完成 | 统一API入口与poll_io attempt检查；定义connect hint/recheck线性化 |
| `udp.rs::send/recv` | UDP datagram I/O与poll_io闭包 | 把terminal检查放入每次实际attempt，覆盖fatal-during-wait |
| `listen_table.rs`、`tcp.rs::poll_listener/accept` | queued Ready/Reset与accept | 保持Reset一次性`IN|ERR`，global fatal优先且不消费queue item |
| `general.rs::send_poller/recv_poller` | check-register-recheck调度 | 不改变axtask API；调用方传入的attempt必须每次读取terminal |

**Critical Path**

```text
queue fatal
  -> lifecycle/round保存concrete DevError
  -> wrapper global CAS（publication线性化点）
  -> registry snapshot under lock
  -> unlock
  -> unconditional wake read/write/terminal sets on every snapshotted bridge
  -> current/late socket effective snapshot(global, local)
  -> API entry or poll_io retry returns one stable AxError

connect state transition
  -> smoltcp DirectionNotify hint
  -> application check/register/recheck
  -> poll_connect commits socket-local code
  -> returns OUT|ERR / stable completion error
```

**Implementation Guidance**

1. 先用已有local terminal再发布global的测试使当前实现RED；随后分离local commit与global unconditional wake，不覆盖local code。
2. 再为UDP recv建立两次attempt模型：第一次`WouldBlock`，其间发布fatal，第二次必须返回global error。让真实poll_io闭包调用同一attempt路径。
3. 把send/recv/connect/accept的global terminal入口检查移到会返回其他状态错误或修改协议状态之前，并保留闭包内重复检查。
4. 最后补connect hint/recheck、listener Reset和normal close回归，再运行两profile full suites和产品build。

**Behavioral Change**

- bridge terminal只表示socket-local error；wrapper global只表示data-plane fatal。两者不互相覆盖，effective snapshot由global优先组成。
- global publisher即使发现bridge已有local error也会wake该bridge；duplicate global publish仍不重复wake。
- 调用前已存在的global fatal先于地址、绑定、连接状态和协议提交返回；等待期间出现fatal时，下一次poll_io attempt返回同一error而不重新Pending。
- connect的smoltcp wake明确为recheck hint；local code必须在application-visible `OUT|ERR`或completion error之前提交，不要求hint回调自行推导错误。
- listener Reset保持queue-head一次性结果；global fatal不消费或改写Reset queue。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 3.1 | R4-R6 / stable data-plane fault | `readiness.rs::ReadinessBridge` | local code与PollSet | 分离local commit和unconditional external wake |
| 3.1 | R4-R6 / current-late sockets | `wrapper.rs::publish_global_fault_code` | global CAS与registry | global CAS→snapshot→unlock→wake全部bridge；不复制global到local |
| 3.1 | R3/R6 / exact error propagation | `async_rx.rs`、`router.rs`、`service.rs`、`stack_runner.rs` | concrete fault code链 | 保持现有传播并补global/local precedence见证 |
| 3.2 | R6 / fault during blocking I/O | `tcp.rs`、`udp.rs`、`general.rs` | public I/O与poll_io attempt | API入口及每次attempt读取effective terminal |
| 3.2 | R6 / connect-listener errors | `tcp.rs::poll_connect/poll_listener/accept`、`listen_table.rs` | connect recheck与queued Reset | 明确hint→commit→result；保持Reset consume-once |

**Task Contracts**

### 3.1: separate local terminal state from global fault publication

- Requirement/Scenario: R4-R6；D5/D6/D8/D9；stable data-plane fault。
- Depends on: Cycle 000 staged implementation and accepted Iterations 000-003。
- Targets: `readiness.rs::ReadinessBridge`、`wrapper.rs::SocketSetWrapper`、现有concrete fault propagation chain和focused tests。
- Current behavior: wrapper global CAS正确发生在registry snapshot前，但publisher尝试把global code写入first-wins local slot；已有local code时不会执行global wake。
- Required behavior: local与global分别first-wins；global CAS是唯一publication线性化点；首次global publish在解锁后无条件wake所有snapshot bridge；duplicate publish不重复wake；current和late socket都从effective snapshot观察global优先类别。
- Required changes: 建立local-before-global RED test；分离local commit helper与global wake；删除global-to-local inheritance依赖；补0/1/2/64/65、duplicate、late add/install和add-vs-publish交错见证；wake callback读取wrapper global并观察已提交code。
- Preserve: registry lock外wake；handle生命周期；per-socket PollSet容量64；concrete DevError编码；RX/TX lifecycle和Faulted no-fallback；flush ledger。
- Forbidden: 覆盖socket-local code；持registry/Service/SocketSet guard wake；引入全局共享PollSet、第二registry、周期轮询、reset epoch或新的owner。
- Test witness: 当前实现下“local已提交→global publish仍wake且effective为global”必须RED；修改后0/1/2/64/65全部获得recheck机会，65沿用wake-on-replacement，late socket无需复制local code即可报告global ERR。
- GREEN condition: focused publication tests与100×确定性交错通过；ordinary和qemu-diagnostics full suites保持GREEN；MS05 fatal/flush error不变。
- Verification: 两profile focused tests；两profile完整lib suites；`git diff --check`。任何missed wake、code不一致、重复wake或guard-held callback均失败。
- Stop when: 正确性要求覆盖local error、改变PollSet容量、增加reset/generation或修改queue owner语义。

### 3.2: make every public I/O retry observe the effective terminal

- Requirement/Scenario: R6；D7-D9；connect/listener error、normal close、stable data-plane fault。
- Depends on: Task 3.1 GREEN。
- Targets: `general.rs`、`tcp.rs`、`udp.rs`、`listen_table.rs`和focused/model tests。
- Current behavior: UDP recv只在poller外检查terminal；部分API在terminal前解析地址、隐式bind、检查本地状态或提交connect；connect raw hint与local error commit未在契约中区分。
- Required behavior: send/recv/connect/accept在调用前已存在global fatal时先返回global category且不修改协议状态；blocking attempt每次重试都检查effective terminal；fatal-during-wait的第二次attempt不得返回`WouldBlock`；connect recheck提交local code后才返回`OUT|ERR`；listener Reset只消费一次；normal EOF/HUP无device ERR。
- Required changes: 把实际poll_io闭包复用的单次attempt提取为可测试路径；统一入口和重试检查；补首次WouldBlock→fatal→第二次stable error、global-over-local、connect hint/recheck、listener Reset和normal close tests；删除本Cycle引入的unused imports。
- Preserve: TCP short write、UDP datagram原子性、nonblocking行为、timeouts、check-register-recheck、listener queue/backlog 512、锁序、无caller-driven poll、非消费SO_ERROR现有范围。
- Forbidden: sleep或busy loop修复；同步poll；完整SO_ERROR消费；message passing；scheduler、smoltcp wire、reset/cancellation、backlog或PollSet容量变化。
- Test witness: 当前UDP两次attempt模型先RED；global preexisting不得被NotConnected/InvalidInput/implicit bind遮蔽；connect hint后recheck返回`OUT|ERR`和稳定error；Reset为`IN|ERR`且accept一次后清除；normal EOF/HUP matrix不增加ERR。
- GREEN condition: terminal matrix、fault-during-wait model、100×ordering、MS05 fatal/flush及两profile full suites通过；源代码中每个blocking closure都调用effective terminal-aware attempt。
- Verification: focused host/model tests；ordinary和qemu-diagnostics完整lib suites；QEMU与受支持D1 compile checks；format/source guards。任何永久Pending、错误类别漂移、额外协议提交或正常close误报ERR均失败。
- Stop when: host/model不能约束实际attempt且必须启动host axtask scheduler，或实现需要完整SO_ERROR、scheduler、reset/cancellation或新socket command owner。

**Invariants**

- global terminal先提交，registry snapshot后解锁，再wake；任何guard不跨wake、await、Pending或yield。
- socket-local与global code各自不可变；effective snapshot始终global优先。
- wake只是recheck hint；I/O成功或错误由重检后的当前状态决定。
- Faulted不恢复polling；runner仍是唯一smoltcp progress owner。
- normal EOF、half-close、UDP close和listener Reset不提升为device-wide fault。
- MS05 descriptor、slot、ticket、flush和queue owner契约不变。

**Non-goals**

- Guest probe、marker validator、QEMU runtime或自动全量资格。
- Linux-compatible destructive SO_ERROR、reconnect、reset、cancellation或link flap。
- SMP、multiqueue、多NIC、PCI/DWMAC、真板、DMA/cache和性能。
- 全局tasks/SNAPSHOT/M-D-K-R-I维护、Evidence目录、归档和commit。

**Acceptance**

1. wrapper global与bridge local分别first-wins；首次global publish在code可见且registry lock释放后wake全部已有bridge，已有local error不抑制wake，duplicate publish不重复wake。
2. 0/1/2/64/65 waiter、late add/install和add/publish交错均有确定性见证；current与late socket报告同一global映射类别。
3. queue RX、arm和stack-round TX fault保留具体DevError，global publisher观察的code与MS05 flush error一致，Faulted不启用fallback。
4. TCP/UDP send、recv、connect和accept在调用前已存在fatal时不先返回其他状态错误或提交新协议工作；等待期间fatal使下一attempt返回同一稳定error。
5. TCP connect raw wake只作hint；recheck提交local error后报告`OUT|ERR`并返回匹配completion error。listener Reset报告`IN|ERR`、消费一次后清除。
6. TCP data/EOF/half-close/full close和UDP正常close保持既有IN/OUT/RDHUP/HUP语义，无global fault时不出现device ERR。
7. ordinary与qemu-diagnostics full suites、直接QEMU/D1 compile、format/source guards和diff check通过；无未解决Critical/Important finding。

**Verification**

- TDD顺序：Task 3.1 local-before-global RED→GREEN→focused fan-out；Task 3.2 UDP two-attempt RED→GREEN→connect/listener/normal-close matrix；再运行回归和build。
- 代表命令：
  - `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --lib`
  - 同命令增加`--features qemu-diagnostics`
  - focused terminal/global publication tests在两profile各重复100次
  - `cargo check --locked --offline -p starry-kernel --features qemu`
  - `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
  - `cargo fmt --all -- --check`
  - `openspec validate ms06-application-visible-async-network-stack --strict`
  - `git diff --check`及完整diff review
- 每项在Act Response记录命令、决定性输出与exit。当前Cycle不运行QEMU runtime。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Cycle 000 staged diff、实际TCP/UDP/readiness/wrapper调用链和两profile新鲜基线已审查 |
| Design | PASS | D6/D8已分离global/local所有权，定义global publication与connect hint两种线性化语义 |
| Iteration Plan | PASS | 004只处理terminal host/model；005 witness、006 automatic、007 QEMU按依赖和证据类别拆分 |
| Cycle Scope | PASS | Tasks 3.1-3.2只关闭既有R6与network-stack-baseline Acceptance |
| Task Contracts | PASS | 每项包含目标符号、RED、GREEN、保持/禁止、验证和停止条件，可只读本Cycle执行 |
| Traceability | PASS | R4-R6、D5-D9映射到Tasks 3.1-3.2、代码位置与见证；无Missing/Simplified |
| Verification | PASS | host/model先证明交错与I/O attempt，full suites和产品build证明边界；QEMU明确延后 |
| User Approval | PASS | 用户原话：“认可，那就给出下一轮rework cyc和对iters map进行更新吧”；按规则落为replan Cycle |

**Persisted Evidence**

- Mode: none

Act Response足以保存命令、每项不超过20行的决定性输出、退出码和修改符号；结果可低成本重跑，不创建Evidence占位目录。

**Risks and Notes**

- `/tmp/opencode/cc-nopie.sh`是当前host-test非PIE linker wrapper。若缺失，按Cycle 000记录内容重建到`/tmp`；不得把wrapper缺失误判为产品失败。
- source guard只能补充行为测试，不能替代local-before-global和two-attempt状态见证。
- 当前目录名保留历史`terminal-readiness-and-qemu-acceptance`，逻辑Iteration范围以本Cycle和更新后的Map为准；不得重命名已有目录。

## Act Response

- Status: reported

**Implemented**

Task 3.1 — separate local terminal state from global fault publication:

- `ReadinessBridge::wake_for_global_publication()` added: unconditional wake of all direction sets for a
  fault already committed in the wrapper's global terminal; never touches socket-local code.
- `SocketSetWrapper::publish_global_fault_code()` now CAS-commits the global code (the only publication
  linearization point), snapshots the registry under the lock, unlocks, then wakes every snapshotted bridge
  unconditionally. Duplicate publications return at the failed CAS and do not re-wake.
- Global-to-local inheritance deleted from `add_public()` and `install_readiness()`: bridges keep only
  socket-local state; current and late sockets observe faults through the effective snapshot
  (`effective_terminal_code(global, local)`, global first) already composed by TCP/UDP `terminal_code()`.
- The now production-orphaned combined helper `ReadinessBridge::commit_terminal_and_wake()` was removed
  (its single caller was the old publish path); socket-local commits keep `commit_terminal()` plus their
  existing event-overlay wake propagation.

Task 3.2 — make every public I/O retry observe the effective terminal:

- UDP: hoisted the receive peer-match enum to module level and extracted the poll_io closure bodies into
  testable single-attempt paths `UdpSocket::try_recv_once()` / `try_send_once()` (verbatim bodies,
  behavior-preserving refactor witnessed by the existing suite staying green). `try_recv_once()` now reads
  the effective terminal at every retry, so a fatal landing between attempts returns its stable category
  instead of another `WouldBlock`.
- Entry ordering: preexisting effective fatal now returns before other work in UDP `connect` (before peer
  commit / implicit bind), UDP `send` (before remote-address resolution / implicit bind), TCP `connect`
  (before state transit, route resolution and smoltcp submit), TCP `accept` (before the listening check;
  queued Ready/Reset slots are not consumed), and TCP `recv` (before the rx-closed branch). TCP `send`
  keeps its per-attempt check as sole guard (see Deviations). UDP `recv`'s existing entry check retained.
- Preserved verbatim: TCP short-write semantics, UDP datagram atomicity, nonblocking behavior, timeouts,
  check-register-recheck scheduling in `general.rs` (untouched), listener queue/backlog 512, lock order,
  zero caller-driven poll, non-consuming SO_ERROR view.
- Scoped rustfmt applied to axnet (`cargo fmt --manifest-path crates/axnet/Cargo.toml --`); it also
  whitespace-only reformatted staged `async_rx.rs` / `listen_table.rs` hunks (no token changes).

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/readiness.rs` | `ReadinessBridge::wake_for_global_publication` (new); `commit_terminal_and_wake` (removed); tests rewritten (`wake_callback_observes_committed_code`) and guard test replaced |
| `crates/axnet/src/wrapper.rs` | `add_public`, `install_readiness`, `publish_global_fault_code`, `global_terminal` field docs; new tests: local-before-global witness, 0/1/2/64/65 fan-out despite local codes, source guards (publish never commits local; install paths never read global), effective-snapshot late-add/install witnesses, adjusted 100× deterministic cycles and add-vs-publish interleaving assertions |
| `crates/axnet/src/tcp.rs` | `TcpSocket::connect/accept/recv` entry checks; new tests: connect-entry no-submit, accept-entry-before-state-checks, recv-entry-before-rx-closed, terminal-leaves-listener-reset-intact |
| `crates/axnet/src/udp.rs` | `ExpectedRemote` (hoisted), `try_recv_once`/`try_send_once` (new attempt paths), `send`/`recv` delegation, `send`/`connect` entry checks, per-attempt check in `try_recv_once`; new tests: fatal-between-attempts two-attempt model, send/connect entry-order witnesses |
| `crates/axnet/src/async_rx.rs`, `listen_table.rs` | whitespace-only (scoped rustfmt of staged content) |

**Deviations from Plan**

1. **TCP `send` entry check not added**: nothing precedes the poller, whose first attempt executes
   immediately in both blocking and nonblocking modes and already reads the effective terminal, so an entry
   duplicate is observationally identical; omitted per minimal-change discipline. The contract's observable
   requirement (preexisting fatal returns its category without other errors or protocol work) still holds
   and is covered by the sibling entry witnesses.
2. **`listen_table.rs` unchanged although listed as target**: Reset consume-once reporting (`IN|ERR`,
   cleared after one accept) and publisher-never-touches-queue were already implemented by the parent
   Cycle 000; this Cycle adds the missing preservation witness instead of code.
3. **Hermetic witnesses use socket-local commits instead of global publications** for socket-level tests:
   publishing on the process-global `SOCKET_SET` would poison every parallel unit test (first-wins, no
   reset API). Local commits exercise the identical `observe_terminal_error()` read path; global-slot
   semantics (CAS linearization, late-socket composition, fan-out) are covered by wrapper-level tests on
   leaked local instances.
4. **Focused ×100 executed with a race-free harness split**: parallel focused runs that mix these tests
   with heavy process-global `SOCKET_SET` churn reproduce a PRE-EXISTING instability (attribution below);
   Task 3.1's publication subset ran ×100 in parallel (100/100 pass) and Task 3.2's terminal set plus the
   threaded interleave witness ran ×100 single-threaded (100/100 pass).
5. **`cargo fmt --all -- --check` replaced by axnet-scoped fmt**: the workspace-wide check fails on the
   untouched vendored `smoltcp` crate at baseline (hundreds of pre-existing diffs); previous Cycles used
   the same scoped command.
6. **RED-witness integrity note**: the first version of the local-before-global test had a self-bug (the
   counting waker was never registered), so its initial "RED" was invalid. The corrected witness was
   re-established against the temporarily restored one-line old behavior (`commit_terminal_and_wake` in
   the publish loop): true RED observed (exit 101, wake count 0 ≠ 1), then the fix reapplied for GREEN.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Specification review: PASS — Tasks 3.1/3.2 re-checked against their contracts: local/global each
  first-wins with global-priority effective snapshots; unconditional post-unlock fan-out; duplicates silent;
  0/1/2/64/65 + late add/install + add-vs-publish interleavings witnessed; per-attempt and entry terminal
  reads unified; connect hint→recheck-commit→`OUT|ERR` and listener Reset consume-once witnesses retained
  from parent cycle plus new precedence witness; normal EOF/HUP matrix untouched and green. Invariants hold
  (no guard across wake; Faulted no-fallback untouched; MS05 flush/queue contracts untouched). Forbidden
  items absent (no local overwrite, no registry-held wakes, no second registry/PollSet/polling/epoch).
- Plan compliance: PASS — TDD order honored per task (true RED observed before each product change;
  extraction step separately witnessed by the suite staying green); change surface confined to
  readiness/wrapper/tcp/udp (+whitespace-only fmt in two staged files); Iteration Map untouched;
  Deferred Iterations 005–007 not started.
- Code quality review: PASS — no out-of-plan edits; no new warnings (axnet lib warnings 22 → 21, one dead
  helper removed, none added); no dead code introduced; naming and test style match file conventions;
  witnesses fail for the right reasons (corrected RED documented above).
- Full diff reviewed: PASS — complete working-tree diff inspected against both task contracts including
  cross-task interactions (publication wake vs I/O recheck share the effective-snapshot read path).
- Critical findings unresolved: none.
- Important findings unresolved: none.
- Minor findings introduced by this Cycle: none.

**Verification Evidence**

| Verification | Command / result | Conclusion |
|---|---|---|
| Task 3.1 true RED | corrected witness vs one-line restored old publish path: `assertion left == right failed ... left: 0, right: 1` | exit 101; expected RED |
| Task 3.1 GREEN focused | `cargo test --lib -- wrapper:: readiness::tests::...` | exit 0; 17 passed |
| Task 3.2 RED set | five new witnesses individually: WouldBlock/Io, NotConnected/Io, panics at entry asserts, connect reaches `lib.rs:80` service panic | exit 101 each; expected RED |
| Task 3.2 GREEN focused | same filters post-fix | exit 0; 23 passed (udp::+tcp::+general::) |
| Ordinary full lib | `RUSTFLAGS=... cc-nopie cargo test --manifest-path crates/axnet/Cargo.toml --lib` | exit 0; **357 passed; 0 failed** (baseline 348 + 9 new); reproduced ≥5 times incl. consecutive runs |
| qemu-diagnostics full lib | same + `--features qemu-diagnostics` | 376 passed; 1 failed = acknowledged pre-existing `async_rx` flake; isolated rerun of the exact test: exit 0 |
| Focused publication ×100 | loop ×100, parallel | pass=100 fail=0 |
| Focused terminal+interleave ×100 | loop ×100, `--test-threads=1` | pass=100 fail=0 |
| QEMU product build | `cargo check --locked --offline -p starry-kernel --features qemu` | Finished dev profile; exit 0 |
| D1 product build | `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1` | Finished dev profile; exit 0 |
| Format | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | clean; exit 0 |
| OpenSpec | `openspec validate ms06-application-visible-async-network-stack --strict` | valid; exit 0 |
| Whitespace | `git diff --check` | clean; exit 0 |
| Warnings | `cargo check` axnet lib | 21 warnings (baseline 22; −1 removed, 0 new) |

**Pre-existing instability attribution** (recorded so Review is not misled):

- Parallel focused runs mixing these tests with heavy process-global `SOCKET_SET` churn intermittently
  panic inside `smoltcp` socket_set ("handle does not refer to a valid socket") / hashbrown debug asserts,
  occasionally SIGSEGV/SIGABRT.
- Attribution experiments: (a) my product + my tests skipped → flake reproduces (diagnostics profile);
  (b) untouched pre-existing test subset sharing the global set, MY product → 17/40 runs fail; (c) same
  subset, product reverted byte-for-byte to the staged Cycle-000 index version → 10/25 fail. Rates are
  statistically indistinguishable: the race predates this Cycle and is independent of this diff.
- The diagnostics-profile `async_rx::reclaim_hold_...` flake likewise reproduces without any of this
  Cycle's tests present, matches the flake acknowledged in the parent Cycle's review, and always passes on
  isolated rerun.

**Persisted Evidence**

None required (Plan mode `none`): all decisive commands rerun cheaply; outputs summarized above within
budget.

**Experience Candidates**

One candidate for `openspec-experience-recorder` (user decision pending; Act creates no artifacts):

- Incident candidate: "parallel axnet unit tests intermittently fail with invalid-handle panics /
  SIGSEGV due to shared process-global SOCKET_SET/LISTEN_TABLE" — significant diagnostic effort,
  probabilistic reproduction, systematic attribution data in the Pre-existing-instability block above and
  the Verification Evidence table (experiments a/b/c). Reference: this Act Response.

No Runbook candidate: no newly verified repeatable operational procedure emerged.

**Remaining Issues**

1. Pre-existing parallel-test UB around the process-global socket state (see attribution block): outside
   plan scope, not introduced here; recommend a future dedicated change (e.g., per-test isolated wrappers
   or serialized access) routed through `openspec-plan`.
2. Acknowledged diagnostics-profile `async_rx` flake (predates this Cycle; isolated reruns pass).
3. 21 pre-existing axnet lib compiler warnings and repo-wide `cargo fmt --all` failure on vendored smoltcp
   (both pre-existing, untouched).

**Commit or Diff Reference**

None; no commit requested. Implementation remains unstaged on top of the user's staged Cycle-000 tree
(`MM` state preserved for readiness/tcp/udp/wrapper; fmt also touched async_rx/listen_table worktree
content only).

## Plan Review

- Review Result: accepted

**Findings**

- Critical: 0。
- Important: 0。
- Minor: 0（本 Cycle 引入）。
- 非阻塞既有问题：qemu-diagnostics 的
  `async_rx::tests::reclaim_hold_drains_to_real_driver_full_without_observing_again` 仍会在并行全量套件中偶发失败；
  本次 Review 首轮为 376/377，精确隔离重跑通过，第二轮完整套件 377/377 通过。并行共享全局
  `SOCKET_SET`/`LISTEN_TABLE` 的另一既有竞态已登记 R57 Incident，不归因于本 Cycle。

**Deviation Classification**

None。Act 所列六项偏差均未改变可观察契约：TCP send 的首次 poller attempt 已在任何协议工作前检查 terminal；
listener Reset 只需保持既有实现；socket-level hermetic seam 与 wrapper global publication witnesses 共同覆盖组合语义；
focused 重复测试的串行化和 axnet 范围 fmt 是对已证实基线不稳定性的验证口径调整。

**Acceptance Gaps**

None。

**Convergence**

reduced：父 Cycle 的 global/local 所有权冲突、已有 local error 抑制 global wake、UDP retry 漏读 terminal、
connect recheck 线性化和规模化 fan-out 见证缺口均已关闭。

**Evidence**

- 完整 staged diff Review：`readiness.rs`、`wrapper.rs`、`async_rx.rs`、`router.rs`、`service.rs`、
  `stack_runner.rs`、`tcp.rs`、`udp.rs`、`general.rs`、`listen_table.rs` 和 `flush.rs`；global CAS→snapshot→unlock→wake、
  concrete DevError 传播、entry/per-attempt terminal check、connect commit-before-result 与 Reset consume-once 均符合契约。
- Review 新鲜 ordinary：357 passed，0 failed，exit 0。
- Review 新鲜 qemu-diagnostics：首轮 376 passed、1 个已知 flake；精确隔离重跑 1/1 通过；第二轮完整套件
  377 passed、0 failed，exit 0。
- Review 新鲜产品检查：QEMU kernel check 与 root D1 check 均 exit 0。
- `openspec validate ms06-application-visible-async-network-stack --strict`、`git diff --cached --check` 均 exit 0。
- Act 的 true RED→GREEN、publication ×100、terminal/interleave ×100 和 pre-existing instability 对照数据成立；
  R57 Incident 保存其归因与适用限制。

**Follow-up Decision**

接收。本 Cycle 的 Acceptance 1–7 均有产品、host/model、build 与 Review 证据；没有需要留在当前 Cycle
直接修复的有限问题，也没有需要新执行契约的 Acceptance gap，因此不创建 rework/replan Cycle。
既有并行测试竞态和 diagnostics flake 不扩大本 Iteration 范围，后续应作为独立 change 处理。

**Iteration Plan Update**

None；Iteration Map 不变。

**Next Cycle**

None.

**Next Iteration**

`../005-application-witness-construction/000-initial.md`（已展开为 draft，等待用户批准；不自动调用 Act）。
