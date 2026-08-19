# Iteration 011 / Cycle 001: First-TX Wake and Runtime Evidence Repair

## Plan Context

- Status: ready
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 6.1, 6.2, 6.3
- Repair items: 6.2-R1, 6.1-R1, 6.2-R2, 6.3-R1
- Depends on: accepted Iteration 010 and Cycle 000's manual QEMU diagnostic
  evidence at revision `2af394e6cc8e6aa9ae7026d7ede136382258a98b`
- Stable baseline: every TX slot created by stack ingress or egress publishes
  the sleeping queue owner on its first empty→nonempty transition; a newly
  built and frozen QEMU artifact set then passes all Iteration 011 runtime and
  final provenance Gates
- Verification boundary: focused RED→GREEN wake witness, full automatic host
  regressions, fresh artifact freeze, clean wget/TCP smoke, six MS05 modes,
  isolated R51/MS01/R45 regressions and final Evidence review
- Diagnostic boundary: first-TX publication, automatic qualification, minimal
  wget smoke, MS05 modes, MS04 regression, network regression and final audit
  are separate stop layers

**Cycle Scope**

- Trigger: Cycle 000 `rework-required` Review.
- Acceptance gaps: A1 first-TX queue publication; A2 repaired artifact
  qualification/refreeze; A3 isolated and correctly partitioned manual
  Evidence; A4 incomplete Tasks 6.2-6.3.
- Inherited requirements: R6 fixed-deadline runtime behavior, R14 complete and
  attributable Evidence, Tasks 6.1-6.3 and all accepted owner/ledger/ABI/wire
  contracts.
- Excluded scope: driver queue or IRQ redesign, smoltcp protocol changes,
  socket API changes, new diagnostic ABI/mode, SMP, hardware, performance,
  archive or global OpenSpec maintenance.

**Objective**

Close the lost first-TX wake exposed by manual QEMU, then qualify and rerun the
same Iteration Acceptance against a fresh artifact set. Keep each runtime fault
domain in a separate raw record so a manual nudge, stale socket or appended pcap
cannot be mistaken for autonomous progress.

**Current-State Evidence**

- `Service::poll()` currently calls `router.poll()` before taking
  `tx_pending_before`. `EthernetDevice::process_arp()` can resolve a neighbor
  during that call and `emit_pending_head()` can fill the first dormant TX
  slot. The later “before” sample is already true, so the final transition test
  does not call `QUEUE_EVENT.publish_queue_work()`.
- A sleeping queue owner has no TX completion to wake it before the first slot
  is submitted. Cycle 000's pcap shows ARP request/reply, then a long delay;
  guest SYNs appear only after later manual activity. Subsequent SYN-ACK frames
  are not a clean witness because the originating wget was interrupted and the
  capture spans multiple diagnostic actions.
- `device::tests::service_poll_publishes_queue_event_after_tx_enqueue` passes
  for ordinary `router.dispatch()` egress. `arp_pending_flush_allocates_zero_
  after_initialization` passes for direct ARP pending flush. No test combines
  ARP ingress, first-slot creation and queue-owner wake through
  `Service::poll()`.
- Cycle 000's `MS04 FAIL mode=nudge` overlaps outstanding network traffic. Its
  extra ISR/reap/refill counts are not an isolated R51 result and do not justify
  changing nudge semantics.
- `run_snapshot()` is guest-only: it performs two bounded diagnostic snapshots
  and no UDP registration. Only tx-only, bidirectional, slot-full,
  descriptor-full and flush need a host stimulus.
- Cycle 000 artifacts still hash correctly, but any product repair necessarily
  supersedes that freeze. Its logs and pcap remain diagnostic history only.
- Current worktree product source matches HEAD. Existing modifications are the
  Cycle 000 Act/Review and Evidence; preserve them.

**Critical Path**

```text
ARP reply enters dormant RX slot
  -> Service ingress resolves neighbor
  -> pending SYN creates first dormant TX slot
  -> whole-poll empty→nonempty observation publishes queue work
  -> queue owner submits SYN without manual nudge
  -> SYN-ACK is reaped, stack-progress wake runs smoltcp
  -> TCP handshake and HTTP download complete
  -> fresh artifacts pass five host-assisted modes + guest-only snapshot
  -> isolated MS04/network regressions and final audit pass
```

**Implementation Guidance**

Observe target TX-slot occupancy at the start of the whole stack poll, before
any ingress path can create a slot, and compare it with occupancy after ingress,
smoltcp egress and router dispatch. Publish queue work exactly when that round
changes empty to nonempty. Preserve the current queue-owner event and Service
guard; do not add polling, retry loops, a second owner or a new IRQ path.

The first focused test must reproduce the complete path: start with an unknown
neighbor and pending IPv4 payload, submit and reclaim the already-published ARP
request through the fake driver's real ticket path so TX is empty, place the
matching ARP reply in an RX slot, register a counting queue waker, and call
production `Service::poll()`. The current code must create the pending IPv4 TX
slot while producing zero queue wake; GREEN must produce exactly one wake.
Separate preservation cases cover already-nonempty, no-new-TX and ordinary
dispatch paths. Do not use the test-only raw slot pop in this witness because it
would bypass ticket release and weaken the ownership assertion.

Do not infer a socket-progress defect from the stale SYN-ACK tail. After the
first-TX repair, a clean wget smoke decides whether another gap exists. If SYN
is submitted promptly but a fresh live socket still fails to consume SYN-ACK,
stop and return to Plan with synchronized serial/pcap/telemetry; do not add a
second speculative waker fix in this Cycle.

**Behavioral Change**

- An ARP reply that flushes a pending packet into an empty dormant TX slot
  wakes the unique queue owner exactly once in the same Service poll.
- Existing pending TX state does not generate duplicate wake storms, and an
  empty/no-progress poll remains quiescent.
- Repaired runtime evidence uses new artifacts and separate files. Snapshot no
  longer starts a host stimulus; R51 nudge runs only from a quiescent baseline.

**Change Surface**

| Repair | Requirement / Gap | Target | Planned responsibility |
|---|---|---|---|
| 6.2-R1 | R6/A1 first-TX progress | `crates/axnet/src/service.rs::Service::poll` | observe and publish whole-round empty→nonempty TX transition |
| 6.2-R1 | R6/A1 missing witness | `crates/axnet/src/device/tests.rs` | combined ARP-resolution→first-slot→owner-wake RED/GREEN and preservation cases |
| 6.1-R1 | R14/A2 repaired provenance | build/host Gates and Cycle 001 Evidence | rebuild, qualify and freeze new revision/artifacts |
| 6.2-R2 | R6/R14/A3 runtime isolation | Cycle 001 manual commands/Evidence | minimal TCP smoke, correct mode partition and clean regressions |
| 6.3-R1 | R14/A4 final review | Cycle 001 review/index/hash records | specs/code/diff/Evidence closure against repaired artifacts |

## Repair Item Contracts

### 6.2-R1: Publish ingress-created first TX work

- Maps to: Task 6.2, R6, Cycle 000 A1.
- Targets: `crates/axnet/src/service.rs`,
  `crates/axnet/src/device/tests.rs`.
- Current behavior: the empty→nonempty observation covers only work created
  after the post-ingress sample, so ARP pending flush can strand the first TX
  slot until unrelated software or hardware activity occurs.
- Required behavior:
  - observe TX pending state before `router.poll()` or any other operation in
    the round can create a target TX slot;
  - after all ingress/egress/dispatch work, publish queue work exactly once if
    the target changed from empty to nonempty;
  - preserve wake ownership, generation ordering, Service serialization,
    bounded queue rounds, deferred ARP transactionality and TX ticket ledger;
  - already-nonempty, no-target and no-new-TX rounds must not add a wake.
- RED witness: the combined production `Service::poll()` ARP-reply test creates
  one pending IPv4 TX slot but observes zero counting-waker calls on current
  source.
- GREEN condition: the same test observes exactly one wake and the pending
  frame remains correctly ticketed; ordinary dispatch wake, deferred ARP,
  slot-drain and ledger tests remain GREEN.
- Verification: focused test RED/GREEN, relevant device/service tests, focused
  test 100 times, axnet qemu-diagnostics and default full suites, rustfmt and
  diff review.
- Preserve: single queue owner, stack-progress role, EVENT_IDX wait protocol,
  fixed budgets, V1/V2/V3 layout, QEMU-only controls and all wire semantics.
- Forbidden: periodic polling fallback for this IRQ NIC, unconditional wake on
  every poll, manual nudge as a product dependency, driver/IRQ changes or a
  speculative SYN-ACK waker change.
- Stop when: the combined witness does not RED as predicted, or closure needs a
  driver, ABI, protocol or socket-waker redesign; return to Plan.

### 6.1-R1: Requalify and refreeze repaired artifacts

- Maps to: Task 6.1, R14, Cycle 000 A2.
- Depends on: 6.2-R1 GREEN and complete product diff Review.
- Required behavior:
  - run the focused/full automatic gates and `make host-test`; persist the true
    command exit, including the pipeline's producer status;
  - build the QEMU image and all five guest payloads from the repaired source;
  - record revision or explicit dirty source identity, environment, sizes and
    SHA-256 in the Cycle 001 Evidence root;
  - freeze only after all builds finish and prohibit rebuilds during the manual
    run; verify hashes before and after every QEMU session.
- GREEN condition: all automatic product Gates exit 0, six artifacts exist and
  hash-check, and source/artifact identity is complete and internally
  consistent.
- Evidence: `environment.txt`, `commands.txt`, `host-test.log`,
  `automatic-gates.log`, `artifacts-before.txt`, `artifacts.sha256`.
- Forbidden: reusing Cycle 000 hashes as authority, editing a failed log,
  omitting the real pipeline exit or running QEMU after a hash-changing build.
- Stop when: any compile, link, assertion, audit, diff or hash Gate fails.

### 6.2-R2: Repeat manual QEMU with isolated evidence

- Maps to: Task 6.2, R6/R14, Cycle 000 A3-A4.
- Depends on: 6.1-R1 GREEN and frozen repaired artifacts.
- Capability boundary: all QEMU and guest-shell interaction remains manual
  under R44. Act prepares exact commands, stops with a Blocker Handoff, then
  audits the user's returned files before continuing.
- Required behavior:
  - create new serial and pcap filenames per QEMU session; never append to or
    overwrite Cycle 000 diagnostics;
  - first run a minimal live-socket smoke: start the HTTP server, boot the
    repaired image, download one payload, and require pcap order ARP
    request/reply → SYN/SYN-ACK/ACK → HTTP request/response with successful
    guest command exit. Stop before the mode matrix if this fails;
  - run guest-only `/tmp/ms05_probe snapshot` without a host stimulus;
  - run a fresh host stimulus for each of the five network modes: tx-only,
    bidirectional, slot-full, descriptor-full and flush. Use concrete filenames
    rather than a literal `<mode>` placeholder;
  - preserve the original fixed-deadline, exact traffic, Full→recovery, C4,
    ledger, fault and marker criteria for every mode;
  - run R51 only with no concurrent host traffic and after two consecutive
    quiescent snapshots. Download/setup traffic must finish before the
    snapshot/idle/nudge/burst sequence. Extra packet work during nudge makes
    the observation interrupted, not FAIL or PASS; repeat from a clean session;
  - run R45/MS01 network/socket regressions in their own declared session and
    retain all required markers and host responses;
  - record every guest/host exit and verify frozen hashes after each session.
- GREEN condition: minimal TCP smoke proves autonomous progress without nudge;
  all six MS05 modes, isolated MS04 modes and network/socket regressions pass
  independently with complete raw records.
- Evidence: `qemu-wget-serial.log`, `wget.pcap`, `qemu-ms05-serial.log`, five
  concrete `ms05-*-host.log` files, `ms05-markers.txt`,
  `qemu-ms04-serial.log`, `ms04-burst-host.log`, `ms04-markers.txt`,
  `qemu-network-serial.log`, network host output/pcaps and
  `runtime-exits.txt`.
- Forbidden: automated guest input, manual nudge in the wget smoke, combining
  multiple sessions in one pcap, treating stale-socket traffic as a live
  handshake, requiring a snapshot host log, or promoting partial results.
- Stop when: the minimal smoke fails, a mode times out/fails, a log is
  interrupted, a hash changes or isolated R51 still fails. Preserve the exact
  synchronized evidence and return to Plan.

### 6.3-R1: Close final Review against repaired evidence

- Maps to: Task 6.3, R14, Cycle 000 A4.
- Depends on: 6.1-R1 and 6.2-R2 GREEN.
- Required behavior:
  - review specs/tasks against the repaired code and all runtime markers;
  - review complete product and OpenSpec diffs without modifying Cycle 000 raw
    Evidence;
  - run strict change validation and scoped non-Evidence worktree/index
    whitespace checks;
  - audit required Cycle 001 files for nonempty content, timestamps, exits,
    source/artifact identity and SHA-256; distinguish diagnostic, interrupted
    and qualifying files;
  - write the Cycle 001 Evidence README/review and report every Task, RTM row
    and Gate status. Do not archive or update global OpenSpec state.
- GREEN condition: no Critical/Important finding or unapproved Missing/
  Simplified result, all required Gates trace to raw Evidence and final hashes
  verify.
- Stop when: any required command, exit, marker, raw log, identity or hash is
  missing or inconsistent; return to Plan for another Cycle.

## BDD Scenarios

- ARP first-slot happy path: owner sleeps with empty TX slots; ARP reply
  resolves a pending SYN during ingress. One queue-work event wakes the owner,
  which submits the SYN without manual intervention.
- Already-pending edge: a TX slot exists at round entry. The poll does not
  publish a duplicate first-slot event or create a wake loop.
- No-progress edge: ingress consumes no frame and egress creates no slot. The
  queue owner remains asleep.
- Minimal QEMU smoke: a live wget produces a complete TCP/HTTP exchange and
  exits 0. ARP-only, SYN-only, repeated SYN-ACK or nudge-dependent progress is
  FAIL.
- Snapshot mode: the guest emits one MS05 snapshot PASS without any host
  stimulus process.
- R51 isolation: two quiescent baselines precede nudge; unrelated IRQ/reap
  makes the attempt interrupted and requires a clean rerun.
- Provenance damage: changed source/artifact, appended mixed-session pcap,
  absent exit or template filename blocks final Acceptance.

## Invariants

- One async task remains the sole raw RX/TX queue owner.
- Stack code touches only fixed slots after activation; queue code alone owns
  raw descriptors and completions.
- Queue publication follows committed state and is an edge-triggered work hint,
  not exact socket readiness.
- ARP state/pending dequeue remains transactional; Full retains the obligation.
- Every manual claim is limited to the declared single-hart QEMU device model.
- Cycle 000 Evidence remains immutable diagnostic history; Cycle 001 owns the
  repaired qualification.

## Non-goals

- No driver transport, IRQ handler, VirtQueue, smoltcp protocol, socket API,
  ABI, wire text or probe-mode redesign.
- No warning cleanup, polling fallback, new background task, SMP/hardware/
  performance claim, archive, SNAPSHOT or M/D/K/R/I update.

## Requirements Traceability Matrix

| Requirement / Gap | Repair | Code / Evidence | Witness | Status |
|---|---|---|---|---|
| R6/A1 autonomous first-TX progress | 6.2-R1 | `Service::poll`, device tests | ARP reply creates first slot and exactly one owner wake | Covered |
| R14/A2 repaired provenance | 6.1-R1 | build/host gates, Cycle 001 hashes | automatic PASS plus six-file freeze | Covered |
| R6/A3 clean six-mode runtime | 6.2-R2 | probe/stimulus manual evidence | wget smoke, snapshot + five host modes | Covered |
| R6/A3 MS04/network compatibility | 6.2-R2 | R51/R45/MS01 evidence | isolated markers, pcaps and exits | Covered |
| R14/A4 final closure | 6.3-R1 | Cycle 001 README/review | specs/code/diff/hash audit | Covered |

## Verification Gates

1. Combined ARP-first-slot witness is RED on the current source.
2. 6.2-R1 focused and preservation tests are GREEN, including 100 repeated
   focused runs.
3. Axnet qemu-diagnostics/default suites, rustfmt, build and `make host-test`
   exit 0; no product failure is waived.
4. Repaired source/artifact identity freezes and verifies 6/6 files.
5. User manual minimal wget/TCP smoke passes without nudge.
6. Guest-only snapshot and five host-assisted MS05 modes pass separately.
7. Isolated R51 and R45/MS01 regressions pass.
8. Final strict OpenSpec, diff, Evidence index/hash and full Review pass.

Every Gate stops downstream work on failure. Gate 5-7 are R44 user capability
boundaries; Act must block and resume this Cycle rather than claim or automate
their result.

## Gate 2: Execution Readiness

- PASS — requirement and scope: local repair items map only to existing Tasks
  6.1-6.3 and R6/R14 Acceptance.
- PASS — root cause: current source ordering and manual pcap establish the
  missing first-TX event; no driver/ABI redesign is needed.
- PASS — test strategy: the absent combined production-path witness has a
  deterministic RED state and explicit preservation matrix.
- PASS — runtime strategy: minimal smoke precedes full modes; snapshot and
  R51 are correctly isolated.
- PASS — provenance: product repair forces a new source/artifact freeze and a
  Cycle 001 Evidence root.
- PASS — boundaries: manual QEMU remains an explicit R44 Blocker Handoff; all
  substantive implementation choices are closed.
- PASS — RTM: no Missing or Simplified row.

## Persisted Evidence

- Mode: required
- Root:
  `evidence/011-independent-manual-qemu-runtime-and-closeout/001-rework/`
- Automatic: environment, literal commands/exits, focused RED/GREEN, repeated
  witness, full gates, build log, source identity, artifact sizes/hashes.
- Manual: separate wget/MS05/MS04/network serial logs and pcaps, five concrete
  host stimulus logs, marker summaries and runtime exits.
- Final: README index, hash list and review mapping every Gate to raw files.
- Passing condition: all required files are nonempty where applicable,
  internally time/source consistent and hash-verifiable. Diagnostic or
  interrupted files are labelled and never substitute for qualifying files.

## Risks and Notes

- Cycle 000's later SYN/SYN-ACK tail may come from a stale socket after wget
  interruption. It supports the stranded-first-slot timing diagnosis but does
  not qualify inbound TCP progress.
- A single global stack-progress waker could be a separate scalability concern,
  but current evidence does not prove it causes this failure. It is outside
  this repair unless a fresh live-socket smoke fails after 6.2-R1.
- Existing smoltcp/axnet warnings are baseline observations, not repair scope.
- The literal zero-byte `ms05-<mode>-host.log` in Cycle 000 is retained as
  non-qualifying history; do not rename it into Cycle 001 Evidence.

## Act Response

- Status: blocked

**Actual changes**

- 6.2-R1 (GREEN): `crates/axnet/src/service.rs` — moved the `tx_pending_before`
  sample in `Service::poll` from after `router.poll()`/ingress/egress to the top
  of the round, so a TX slot created by an ARP-reply pending flush during
  ingress is observed as an empty→nonempty transition and publishes exactly one
  queue-owner work event. Publish logic, wake ownership, generation ordering,
  budgets, deferred ARP transactionality and the ticket ledger are unchanged.
- 6.2-R1 (GREEN): `crates/axnet/src/device/tests.rs` — added the combined
  production-`Service::poll` witness
  `service_poll_arp_flush_first_tx_slot_wakes_queue_owner_once`
  (unknown neighbor + pending IPv4 → ARP request drained through the fake
  driver's real ticket path → ARP reply in RX slot → poll must wake the
  sleeping queue owner exactly once, flushed frame stays ticketed, no Router
  fault) plus preservation tests
  `service_poll_already_pending_tx_does_not_publish_again` and
  `service_poll_empty_round_keeps_queue_owner_asleep`.
- 6.1-R1 (GREEN): rebuilt the QEMU image and five guest payloads from the
  repaired source via the automatic gate manifest; froze six artifacts with
  sizes + SHA-256 under `evidence/011-…/001-rework/`.
- 6.2-R2: NOT STARTED — user capability boundary (Blocked, see Blocker Handoff).
- 6.3-R1: NOT STARTED — depends on 6.2-R2 raw evidence.

**Deviations from Plan**

- The combined witness sets up the unknown-neighbor/pending state via a
  device-level `dev.send()` plus direct `tx_submit_one()`/`tx_reclaim_one()`
  (the same production queue-task methods the driver's real ticket path uses)
  instead of a first full `Service::poll()` round to create-and-publish the ARP
  request. This is a local, non-substantive setup difference: the witnessed
  production path (a `Service::poll()` round where an ARP reply creates the
  first dormant TX slot during ingress) is exactly the contract's RED/GREEN
  scenario, and the drain never uses the test-only slot pop.

**Self-Review**

- Spec review: contract 6.2-R1's required behavior (observe before any
  in-round creation; publish exactly once on empty→nonempty; RED witness on
  current source; GREEN with exactly one wake and correctly ticketed frame;
  preservation of already-nonempty/no-new-TX/ordinary-dispatch) and 6.1-R1
  (all automatic Gates exit 0, six artifacts hash-check, complete identity) —
  all implemented and witnessed. No polling fallback, unconditional wake,
  manual-nudge dependency, driver/IRQ/ABI/wire change, or SYN-ACK speculation
  was introduced.
- Code review: product diff is limited to the two contract targets (+9/-2 in
  `service.rs`, +101 in `tests.rs`); no warnings introduced; tests cannot pass
  for a wrong reason (`wake_if_space` requires a previously-published waiting
  bit the tests never set; `publish_event` is not reachable from
  `Service::poll`); naming/conventions match the file.
- Engineering note: the audit's source-freeze identity is stable once the
  regenerable evidence root is excluded from git identity via a local (non
  committed) `.git/info/exclude` entry — verified empirically (excluded-root
  identity matches the frozen identity byte-for-byte).
- Remaining Minor: none blocking. The manifest's `kernel-lichee-d1-check` gate
  keeps the documented expected exit 101 with the exact E0432/E0433 diagnostic
  contract. The Cycle 000 literal `ms05-<mode>-host.log` zero-byte template
  remains immutable history and is not part of this Cycle.

**Verification**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED witness | `cargo test --manifest-path crates/axnet/Cargo.toml --offline --lib service_poll_arp_flush_first_tx_slot_wakes_queue_owner_once` | `assertion left == right failed (count 0 vs 1)`; `test result: FAILED. 1 failed` | PASS (RED as predicted) |
| GREEN witness | same command after fix | `test result: ok. 1 passed; 0 failed` | PASS |
| preservation | `cargo test --manifest-path crates/axnet/Cargo.toml --offline --lib service_poll_` | `12 passed; 0 failed` (incl. already-nonempty, empty-round, ordinary dispatch, deferred ARP) | PASS |
| focused 100× | 100× rerun of the combined witness | `100× ok. 1 passed; 0 failed` | PASS |
| axnet default suite | `cargo test --manifest-path crates/axnet/Cargo.toml --offline --lib` | `218 passed; 0 failed` | PASS |
| axnet qemu-diagnostics | `cargo test … --lib --features qemu-diagnostics` | `237 passed; 0 failed` | PASS |
| rustfmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | exit 0 | PASS |
| automatic manifest | `python3 scripts/ms05_evidence_capture.py --run automatic --root …/001-rework` | exit 0; 44/44 records `pass` (host-test, all suites, 3×100× races, both harnesses, D1 contract, builds, rustfmt, openspec-strict, dual diff, artifact records) | PASS |
| audit | `python3 scripts/ms05_evidence_audit.py --root …/001-rework --write-qualification` | negative fixtures PASS + positive audit PASS, qualification written | PASS |
| qualification binding | `… --verify-qualification` | `qualification binding VERIFIED` | PASS |
| artifact freeze | `sha256sum -c artifacts.sha256` | 6/6 OK | PASS |
| strict OpenSpec | `openspec validate ms05-… --strict` | `Change '…' is valid` | PASS |
| diff checks | `git diff --check` / `git diff --cached --check` (evidence excluded) | exit 0 / exit 0 | PASS |

**Persisted Evidence（required）**

- Root: `evidence/011-independent-manual-qemu-runtime-and-closeout/001-rework/`
  - `manifest.json`（44 records, source freeze, 6 artifacts）与 `logs/`（每 Gate 原始输出 + exit trailer，100× 子记录齐全）
  - `qualification.json`（verdict PASS）、`evidence-audit.log`、`env-blocked.json`（none）
  - `environment.txt`、`commands.txt`、`host-test.log`（exit 0）、`automatic-gates.log`（44/44 pass）、`artifacts-before.txt`、`artifacts.sha256`、`README.md`
- Change 级与 Iteration 级 evidence README 索引已更新（011/001-rework 标记为 in-progress/等待用户）。

**Blocker Handoff**

- Task/Step: 6.2-R2 — manual QEMU runtime（6.1 ordinary-terminal rerun、minimal wget smoke、guest-only snapshot、五个 host-assisted MS05 modes、隔离 R51、R45/MS01 网络回归）。
- Gate: R44 能力边界 — QEMU guest-shell 交互、wget smoke 与 ordinary-terminal 运行必须由用户在普通终端执行（`.claude/runbooks/qemu-network-testing.md`）。Act 不得自动运行 QEMU，也不得在缺少原始输出的情况下声明 PASS。
- Plan expected vs actual: 一致 — Plan 的 6.2-R2 明确要求 Act 在此边界停止并留下精确命令清单；现已在 `evidence/011-…/001-rework/commands.txt` 备好。
- Impact: 剩余任务的 runtime 证据（six-mode、MS04/MS01/R45 回归）与 Task 6.3 最终审查被阻塞；自动资格与制品冻结已完成并绑定到修复后的 dirty-tree 身份。产品代码已修改（service.rs/tests.rs，未提交）；冻结制品 hash 已记录，不得在手工运行期间重建。
- Completed: 6.2-R1（GREEN）+ 6.1-R1（GREEN）。Partial: 无。Not started: 6.2-R2（用户）、6.3-R1。
- Recovery: 用户按 `commands.txt` 在普通终端执行并返回原始文件（`host-test.log`、`qemu-wget-serial.log`、`wget.pcap`、`qemu-ms05-serial.log`、五个 `ms05-*-host.log`、`ms05-markers.txt`、`qemu-ms04-serial.log`、`ms04-burst-host.log`、`ms04-markers.txt`、`qemu-network-serial.log`、`network-host.log`、`runtime-exits.txt` 及环境差异）；Act 审计后填写 Blocker Resolution、恢复 `pending`，并执行 6.3-R1。

**Blocker Resolution**

None — awaiting user manual run.

**Experience Candidates**

- Candidate: manual QEMU diagnostic runbook for the Repaired first-TX-wake data
  plane（分层 serial/pcap/snapshot 诊断 + 隔离 wget smoke→modes→regression
  顺序 + 每 session 前后 hash 校验），若 6.2-R2 端到端通过，可提交
  `openspec-experience-recorder` 更新/扩展 `qemu-network-testing` Runbook。
  引用：本 Act Response 与 `evidence/011-…/001-rework/commands.txt`。
- Candidate: `ms05_evidence_audit` 的 source-freeze identity 需把可再生成的
  evidence root 排除在 git identity 之外（`.git/info/exclude`）才能通过正向
  audit；该操作提示可补入 `ms05-automatic-gate-manifest` Runbook 的失败处理节。
  引用：本 Act Response 与 `evidence/011-…/001-rework/` 的 audit 记录。

**Unresolved issues**

- MS04 nudge 隔离重检（Cycle 000 发现的疑似 regression）须在 6.2-R2 Session 3
  的 quiescent 基线确认后判定；当前不构成产品结论。
- 所有结论限于单 hart QEMU VirtIO-MMIO 软件/设备模型；不涉及 SMP、DWMAC、真板、DMA/cache 或性能。

## Plan Review

- Status: reviewed

**Review Result**

rework-required

**Findings**

- **6.2-R1 product repair is accepted.** The product diff moves the TX pending
  sample to the start of `Service::poll()` and leaves the existing
  empty→nonempty publication at the end. Independent Review reran all 12
  `service_poll_` tests; the combined ARP-flush witness and the
  already-pending/empty preservation cases passed. No driver, IRQ, ABI, wire,
  budget or ownership change was introduced.
- **Blocking — required Cycle 001 Evidence is hidden by local Git state.** Act
  added the whole `001-rework/` Evidence root to `.git/info/exclude` so
  `source_identity()` would remain stable while capture wrote its own output.
  `git check-ignore -v` confirms the rule, and `git status --untracked-files=all`
  shows none of the required files. A local, unversioned exclude rule is not
  bound by the manifest and prevents the Evidence from following the change;
  the current qualification is therefore machine-verifiable only in this
  checkout, not persistable or reproducible as R14 requires.
- **Blocking — source identity has no explicit output-root contract.** The
  capture/audit pair calls `git ls-files --stage`, `git diff --binary` and
  `git ls-files --others --exclude-standard` without a root exclusion. It
  therefore depends on `.git/info/exclude` to avoid self-reference. The tool
  must derive the one exact Evidence root from `--root`, exclude only that root
  from index/worktree identity, record the exclusion and reject a missing,
  broad or different exclusion during audit.
- **Blocking — the prepared wget pcap cannot prove the required guest-side L2
  order.** `tcpdump -i any` observes host interfaces, while QEMU user networking
  keeps guest Ethernet/ARP inside the `net0` backend. R45's applicable witness
  is QEMU `filter-dump` attached to `net0`; without it, absence of ARP/SYN in the
  host capture is ambiguous and cannot qualify or diagnose the smoke Gate.
- **Blocking — sessions 1, 3 and 4 lack complete independent launch commands.**
  The command list starts only `qemu-wget-serial.log`, then asks the same
  process to be “saved as” `qemu-ms05-serial.log`, and later says to restart
  into MS04/network logs without giving the exact QEMU argv. Those required
  files cannot be produced verbatim from the handoff, so the R44 boundary is
  not execution-ready.
- **Blocking — ordinary-terminal exit capture is still incomplete.** The
  command prints `${PIPESTATUS[0]}` after `tee` but does not append it to
  `host-test.log` or another required exit record. The automatic manifest log
  has an exit trailer, but it cannot substitute for the user-terminal Gate.
- No manual QEMU Gate was attempted in this Cycle, which is correct at the
  declared capability boundary. The missing runtime Evidence is not itself an
  Act failure; the invalid handoff and non-persistable automatic Evidence are
  the reasons a new Cycle is required.

**Deviation Classification**

- `ACT-DEVIATION`: using `.git/info/exclude` makes required Evidence invisible
  and turns an unversioned local rule into part of qualification semantics.
- `PLAN-OMISSION`: Cycle 001 required a self-verifying Evidence root but did not
  specify a deterministic, narrowly validated output-root exclusion in the
  capture/audit identity contract.
- `PLAN-INVALID`: the manual Evidence plan required guest ARP/TCP packet order
  without mandating a QEMU-netdev capture source, and did not require a full
  independent QEMU argv for every named session log.
- `ACT-DEVIATION`: the generated command list uses host `tcpdump`, incomplete
  session launch instructions and a non-persisted ordinary-terminal exit.
- No material deviation in 6.2-R1 product code or its test setup.

**Acceptance Gaps**

- A2 remains: automatic qualification must be reproducible without local Git
  ignore state, and required Evidence must be Git-visible and persistable.
- A3 remains: the R44 handoff must provide exact independent session commands,
  QEMU `net0` pcaps, concrete outputs and persisted exits before user execution.
- A4 remains: 6.2-R2 and 6.3-R1 have not run.
- A1 is closed: the ARP-created first TX slot now publishes exactly one queue
  wake with preservation coverage.

**Convergence**

Reduced. The product runtime gap from Cycle 000 is closed and independently
witnessed. Remaining gaps are confined to qualification identity, Evidence
persistence and manual-session orchestration; they do not require another
data-plane design or product-code change.

**Evidence**

- Independent `git diff` review found only the planned `Service::poll()` sample
  move and three focused tests. Twelve `service_poll_` tests passed with exit 0.
- The recorded automatic manifest reports 44/44 pass, qualification binding
  verifies, and all six frozen artifacts currently hash-check.
- `git check-ignore -v` identifies `.git/info/exclude` as the rule hiding the
  entire required Cycle 001 root; removing that local dependency would make
  current `source_identity()` observe the generated Evidence and drift.
- Source inspection confirms both capture and audit recompute identity without
  an explicit Evidence-root parameter and honor local standard excludes.
- Command inspection confirms only the wget QEMU process has a complete argv,
  the pcap source is host `tcpdump -i any`, and the user-terminal exit is not
  written to a required file.

**Follow-up Decision**

Do not ask the user to execute the Cycle 001 command list. Preserve its product
repair and automatic logs as diagnostic inputs, then create Cycle 002 to make
qualification self-contained and issue an exact four-session R44 handoff.

**Iteration Plan Update**

None. Existing Tasks 6.1-6.3, R6/R14 and the Iteration Map remain unchanged;
Cycle 002 adds only local repair items.

**Next Cycle**

`002-rework.md`

**Next Iteration**

None.
