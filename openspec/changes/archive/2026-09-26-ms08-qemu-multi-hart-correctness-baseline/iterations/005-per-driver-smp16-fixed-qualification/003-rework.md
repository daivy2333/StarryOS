# Iteration 005 / Cycle 003: Replan final qualification as a network-only fixed-placement run

## Plan Context

- Status: ready
- Iteration: 005-per-driver-smp16-fixed-qualification
- Cycle: 003-rework
- Cycle Type: replan
- Parent cycle: `002-rework.md`

**Iteration Scope**

- Change tasks: 6.1–6.3; Tasks 7.1–7.3 are explicitly skipped by the approved scope revision.
- Depends on: accepted Iteration 004 and Iteration 000–003 baselines.
- Stable baseline: QEMU VirtIO-MMIO on `SMP=16` has directly verified fixed owner/runner placement, timer-independent cross-hart wake, TCP/UDP bidirectional traffic, Full→recovery, readiness/quiet behavior and closed descriptor/slot/ticket ledgers.
- Verification boundary: one bounded network-only runtime is accepted independently by the guest probe, host peer and transcript validator. UART output is an environment precondition, not a UART qualification result.
- Diagnostic boundary: guest/peer protocol, IRQ/owner/runner placement, remote-ready IPI, network data path, backpressure, readiness/quiet or resource ledgers.
- Deferred tasks: None.

**Cycle Scope**

- Trigger: user-approved qualification scope and verification-contract change after Cycle 002 Review.
- Acceptance gaps: the existing probe/peer/validator still require migration, reset/link and UART-adjacent choreography that no longer belongs to the final claim; no bounded core-network transcript/validator verdict exists for the revised profile.
- Repair items: None; this is a replan using revised Tasks 6.1–6.3.
- Inherited scope: the implemented SMP synchronization, schedulable run queues, remote-ready IPI, fixed singleton network placement, V5 observation ABI, timer-disabled wake witness, bounded slots/descriptors/tickets and the existing guest/peer/validator framework.
- Excluded scope: product network changes, role migration repair, UART qualification, UART+network combined pressure, reset/link rerun, old milestone-by-milestone regression, physical hardware and performance.

**Objective**

Reduce the existing network qualification tools to the six behaviors that directly support the revised claim, then obtain one `SMP=16` network verdict without changing product networking code.

**Background**

Cycle 002 passed placement, timer-disabled wake, TCP/UDP bidirectional traffic, Full→recovery, readiness/quiet and reset traffic before later qualification choreography failed. Review established that the link failure was an operator-line timeout rather than a product link-up failure. Owner migration timed out intermittently with INFO console logging but did not reproduce at WARN. On 2026-09-26 the user accepted the remaining migration timing risk and explicitly removed UART qualification, migration repair, reset/link rerun, combined pressure and old-stage reruns from the final Gate.

The accepted risk is narrow: no conclusion will be made about controlled migration stability across log levels. The fixed singleton owner/runner path remains the product behavior being qualified.

**Investigation Facts**

- Current Baseline: Cycle 002 reported PASS for the probe/peer/validator self-tests, ordinary and `SMP=16` builds, and the first six network runtime cases. Its raw transcript also showed resource ledgers closed after reset. The later migration/operator choreography prevented a complete validator exit.
- Current-State Evidence: `tests/ms08_network_smp_probe.c::ms08_net_cases`, `ms08_net_schema`, `ms08_net_peer_cases` and `run_probe` currently freeze ten cases. The first six are exactly the revised profile; `run_probe` then unconditionally executes owner migration, runner migration, reset and link cases.
- Current-State Evidence: `scripts/ms08-network-peer.py::PEER_CASES` accepts eight traffic cases and `PeerLedger` reports all of them. `scripts/ms08-network-validate.py::{EXPECTED_CASES,PEER_CASES,CASE_GRAMMAR,validate}` likewise requires the four removed cases, so the existing validator cannot accept a deliberately shortened transcript.
- Current-State Evidence: `tests/ms08_network_smp_probe_test.c` asserts ten cases and contains focused placement, wake, payload, Full/recovery, readiness/quiet, migration, reset and link decision tests. The implementation may retain pure decision helpers, but the registry/schema expectations and final-profile fixtures must match the six accepted cases.
- Code and Critical Path: the guest reads V5 through the existing ioctl, proves current singleton affinities and actual poll harts, runs the timer-disabled witness, exchanges framed TCP/UDP traffic with the host peer, drives queue capacity to Full and observes recovery, checks readiness/quiet, then emits the terminal marker. The host peer independently enforces per-case sequence order and exact echo. The validator consumes only the captured outputs and native exit lines.
- Compatibility: no product API, network owner/runner, scheduler, UART driver, reset/link control or V5 wire layout needs to change. Existing migration/reset/link helper code may remain because removal is limited to the final profile registry and execution path.

**Implementation Guidance**

Use the current first six cases as the only ordered profile. Keep one protocol rather than adding a second `--profile` CLI unless a focused test demonstrates that retaining the full registry is necessary for another current caller. Remove unused final-profile peer accounting for migration/reset/link, but do not delete product diagnostics or pure decision helpers merely because the final runtime no longer calls them. Keep all waits on existing absolute deadlines and preserve direct native exits.

**Behavioral Change**

The network qualification command terminates after `readiness-quiet`. The guest, peer and validator agree on these cases in order:

1. `placement`
2. `timer-disabled-wake`
3. `tcp-bidirectional`
4. `udp-bidirectional`
5. `full-recovery`
6. `readiness-quiet`

No product behavior changes. The qualification result no longer implies UART asynchronous behavior, controlled migration stability, reset/link interleaving, combined-driver pressure or compatibility with old runtime suites.

**Task Contracts**

### 6.1: Record the UART runtime waiver without changing UART

- Requirement/Scenario: UART as test infrastructure; user-approved qualification simplification.
- Depends on: the user's manual confirmation that UART operates and the accepted Iteration 001–002 implementation baseline.
- Targets: change planning/specification only; no UART source, probe or harness changes.
- Current behavior: Cycle 002 has no complete UART probe transcript because the attempted launch selected the wrong QEMU machine.
- Required behavior: UART console is only required to carry a complete bounded network transcript. The final result explicitly makes no new claim about UART async RX, Full recovery, readiness, drain or migration.
- Required changes: none in Act; retain the `SKIPPED` task reason and risk in the final response.
- Preserve: early console, async UART implementation, SPSC identity, D1 workaround and existing UART tests/tools.
- Forbidden: repairing or deleting UART product/test code, treating console output as comprehensive UART evidence, or adding a UART runtime Gate.
- Test witness: not applicable; this is an explicit user waiver, not an implementation task.
- GREEN condition: final response states the limited role of console and does not report UART qualification.
- Verification: full diff review confirms no Act changes under UART product/probe/harness paths.
- Stop when: the network session cannot produce complete bounded output; report an environment blocker without diagnosing an UART product fault from missing output alone.

### 6.2: Reduce the existing network probe, peer and validator to the approved profile

- Requirement/Scenario: network qualification; capacity/timeout; fixed placement and direct cross-hart wake.
- Depends on: accepted protocol implementation and Cycle 002's passing first-six-case runtime observations.
- Targets: `tests/ms08_network_smp_probe.c::{ms08_net_cases,ms08_net_schema,ms08_net_peer_cases,run_probe}`, `tests/ms08_network_smp_probe_test.c`, `scripts/ms08-network-peer.py::{PEER_CASES,PeerLedger,self_test}`, and `scripts/ms08-network-validate.py::{EXPECTED_CASES,PEER_CASES,CASE_GRAMMAR,validate,self_test}`.
- Current behavior: all three sides require ten ordered cases and the runtime proceeds into migration/reset/link after the six core network cases.
- Required behavior: all three sides require exactly the six ordered cases listed above. The validator still directly rejects missing/reordered/duplicate cases, timer-only wake, invalid fixed placement, corrupt payload/accounting, Full without recovery, readiness/quiet drift, resource imbalance, fatal guest output and nonzero guest/peer outcome.
- Required changes: adjust the existing registries, schemas, runtime tail, peer ledger summary and positive/negative fixtures. Retain reusable migration/reset/link decision helpers unless ordinary compilation requires removing unreachable static functions.
- Preserve: V5 wire ABI, exact framed payload protocol, per-case strictly increasing sequence numbers, absolute deadlines, timer restoration checks, current-affinity versus actual-poll-hart check, descriptor/slot/ticket conservation, validator purity and peer independence from QEMU/HMP control.
- Forbidden: product networking changes, a second owner/runner, sleep polling, unbounded retry, a second test server, revision/run identity, reset/link/operator input, migration control or weakening first-six-case predicates.
- Test witness: before edits, the C test and Python self-tests assert the ten-case registry and a six-case transcript is rejected as incomplete.
- GREEN condition: C probe test, peer self-test, validator self-test, case/schema cross-checks and static guest build all pass with exactly six cases; focused negative fixtures still reject each listed failure class.
- Verification: existing Makefile host-test entry or its constituent native C/Python commands, followed by the existing static guest payload build.
- Stop when: removing the four cases would require a product ABI change, another server/protocol, or weakening placement/wake/resource checks; return to Plan.

### 6.3: Obtain one bounded `SMP=16` network verdict and review the diff

- Requirement/Scenario: network qualification and conclusion boundary.
- Depends on: Task 6.2 GREEN.
- Targets: existing QEMU network-only launch, `tests/ms08_network_smp_probe --run`, `scripts/ms08-network-peer.py`, `scripts/ms08-network-validate.py`, and the complete worktree diff.
- Current behavior: previous sessions prove the six behaviors individually but no transcript terminates after them with mutually successful guest, peer and validator exits.
- Required behavior: one WARN-level `SMP=16` run completes all six cases; V5 reports 16 configured/schedulable harts, singleton owner/runner affinities and matching actual poll harts; the timer-disabled witness shows remote enqueue/IPI/receive/resume with timer restored; TCP/UDP and Full recovery preserve framed payload/accounting; readiness/quiet and terminal resource ledgers pass. Guest, peer and validator exit 0.
- Required changes: run the existing bounded procedure once after Task 6.2; no product change is planned. Record native commands, decisive output and exits in Act Response, then review the full staged and unstaged diff.
- Preserve: QEMU `virt`, `SMP=16`, VirtIO-MMIO, WARN logging, separate host peer and guest output, bounded absolute deadlines and no repeat-until-pass.
- Forbidden: INFO-level migration diagnosis, migration/reset/link/HMP choreography, UART-specific traffic, old-stage runtime reruns, ping-only substitution, accepting partial output or changing product code after the qualifying run without invalidating it.
- Test witness: the current ten-case validator rejects a transcript that intentionally stops after `readiness-quiet`; Task 6.2 makes that revised transcript the direct expected behavior.
- GREEN condition: focused host/tool tests, static guest build and `SMP=16` kernel build exit 0; the single runtime yields guest exit 0, peer exit 0 and validator exit 0; `git diff --check`, strict structure validation and full diff review pass.
- Verification: native focused commands, affected builds, one bounded QEMU session, validator, diff checks and full diff inspection. Old milestone-by-milestone runtime is explicitly skipped.
- Stop when: any placement, IPI, payload, progress, quiet or resource invariant fails; report that network behavior instead of trimming another case or retrying until pass.

**Invariants**

One logical queue owner and one stack runner remain fixed to valid singleton affinities. IRQ and socket callers publish events; only the owner advances descriptor ownership. The timer-disabled witness must restore the timer on every terminal path. Payload and resource accounting remain exact. QEMU evidence is limited to its VirtIO-MMIO model and 16 homogeneous virtual harts.

**Non-goals**

UART qualification, owner/runner migration stability, reset/link recovery, combined UART/network stress, old milestone runtime regression, physical boards, heterogeneous scheduling, CPU hotplug, multiqueue/RSS and performance.

**Acceptance**

- Fixed placement → Task 6.2/6.3 → V5 singleton affinities, schedulable membership, matching actual owner/runner poll harts and nonzero progress.
- Direct cross-hart wake → Task 6.2/6.3 → timer-disabled witness with remote enqueue, IPI send/receive, resume and timer restoration.
- Network data path → Task 6.2/6.3 → strictly framed TCP and UDP bidirectional exchanges with matching guest/peer accounting.
- Backpressure and completion → Task 6.2/6.3 → Full observed, later capacity and data progress, balanced descriptor/slot/ticket terminal ledger.
- Readiness and idle behavior → Task 6.2/6.3 → poll/select/epoll agree with I/O and quiet window shows no data-path polling progress.
- Scope boundary → Task 6.1/6.3 → final result names all skipped claims and does not use UART console output as UART qualification.

**Verification**

Run the probe decision test, peer self-test, validator self-test, case/schema cross-check and static guest build. Build the affected `SMP=16` kernel. Then run one bounded QEMU `virt` network-only session with the host peer and pass its transcript to the existing pure-output validator. Native exits decide. Finish with diff checks, strict structure validation and full diff review.

Do not run UART qualification, controlled migration, reset/link, combined pressure or old milestone runtime suites. Those are scope decisions, not environment blockers.

**Gate 2 Readiness**

- Requirements/scope: PASS — the user explicitly selected network-only fixed-placement qualification and accepted the migration logging-timing risk.
- BDD/scenarios: PASS — happy path, Full/recovery, timeout/failure classes, quiet behavior and conclusion boundary are explicit.
- Investigation: PASS — the guest registry/runtime, peer registry/ledger, validator grammar and focused tests are located; no product path change is required.
- Design/tasks: PASS — three tasks separate the waiver, test-profile edit and runtime verdict with concrete targets and stop conditions.
- Traceability: PASS — each retained claim maps to an existing direct observation; every removed claim is named as skipped.
- Verification: PASS as a plan — native self-tests/builds and one bounded runtime directly decide the revised claim without old-suite substitution.
- Change consistency: PASS — proposal, delta specification, design decision D11 and Iteration Map carry the same revised boundary.
- Persisted Evidence: PASS — `none`; the six-case run is bounded and its decisive output/exits fit the Act Response.
- User approval: PASS — user explicitly approved this detailed execution plan on 2026-09-26.
- Overall Gate 2: PASS — the revised network-only plan is ready for Act.

**Persisted Evidence**

- Mode: none
- Budget: Act Response records native commands, decisive output, exits and affected files; no separate evidence files are necessary.

**Risks and Notes**

- WAIVED: controlled owner/runner migration may still contain a timing-sensitive defect hidden or exposed by console logging. The final result makes no migration-stability claim.
- WAIVED: UART console output proves only that the chosen output path carried this transcript; it does not independently prove all asynchronous UART semantics.
- WAIVED: reset/link, combined pressure and old runtime regressions are not rerun. The conclusion excludes new claims about them.
- Existing INFO/WARN diagnostic level differences must not be normalized by adding more product logging; the qualifying profile uses WARN and contains no migration case.

## Act Response

- Status: reported

**Summary**

Task 6.1（UART waiver）与 6.3（runtime）为记录/运行型任务；Task 6.2 完成最终 profile 裁剪。一次有界 `SMP=16` network-only 运行完整走完六个 case，guest 全部 PASS 且 exit 0；host peer 独立性由 pcap 客观流量证实。用户明确不再使用 validator 对 transcript 判定。

**Actual changes**

- `tests/ms08_network_smp_probe.c`：`ms08_net_cases`/`ms08_net_schema`/`ms08_net_peer_cases` 裁剪为六个最终 case（placement、timer-disabled-wake、tcp-bidirectional、udp-bidirectional、full-recovery、readiness-quiet）；`run_probe` 移除 migration/reset/link 执行路径；删除不可达 static case runners（`run_role_migration`、`run_reset_io`、`run_link_off_on`、`expect_terminal*`、`wait_fd_revents`、`read_console_line`）与专用宏；`ms08_net_probe_self_test` 断言 6 case；`terminate_control_caller`/`reap_control_caller` 保留并限定 host-harness（`#ifdef MS08_NET_PROBE_TESTING`）。非 static 决策谓词（migration/reset/link 关系）全部保留供 host decision tests 使用。
- `tests/ms08_network_smp_probe_test.c`：registry/schema 断言从 10 改为 6 case；既有 migration/reset/link 决策谓词测试保持（谓词仍在）。
- `scripts/ms08-network-peer.py`：`PEER_CASES` 从 8 收敛为 4 个 traffic case。
- `scripts/ms08-network-validate.py`：`EXPECTED_CASES`/`PEER_CASES`/`CASE_GRAMMAR`/`schema_lines` 收敛为六 case；移除 `_migration`、migration/reset/link 协议校验与对应负向 fixtures；`canonical()` 与 self-test 重建为六 case；fault-drift/counter-rewind fixtures 改指 full-recovery 快照。
- `openspec/changes/ms08-qemu-multi-hart-correctness-baseline/evidence/005-per-driver-smp16-fixed-qualification/003-rework/`：`guest-transcript.log`、`diag.pcap`、`README.md`（user-required，见 Evidence）。

**Deviations from Plan**

- 用户指令："直接检测日志输出，validator是违反规范的，算是历史遗留，这里我们不做"；随后确认"只要证实正常在跑就行，不用管那么多"。最终判定不使用 `ms08-network-validate.py`，改为 guest 原生 marker/退出码 + pcap 客观流量直接判定；validator 代码仍按 Task 6.2 契约裁剪并自测通过（保持一致性），但不作为本 Cycle 通过判定工具。
- `peer.log` 未采集到结果行：peer 在 420s 超时前被停止，Python 管道输出块缓冲未落盘；peer 独立性由 `diag.pcap` 双向 echo 帧承担（tcp 8/udp 8/readiness 1/full-recovery 301 帧一一对应，与 guest SOCK 计数一致）。
- Evidence 模式由 Plan 的 `none` 改为 user-required 采集（用户明确要求日志采集到证据目录）。

**Self-Review**

- Gate 4 spec review：registry/schema/run_probe/peer/validator 三端一致收敛为六 case；V5 线协议、帧协议、严格递增 seq、绝对 deadline、validator 纯度（无 subprocess/socket/QEMU）与 probe 无 sleep-poll 全部保持。
- Gate 4 code review：`git diff --check` 干净；无计划外修改；判定谓词保留未删（契约要求）；无死代码（validator unused-function 检查为空；guest `-Wall -Wextra -Werror` 干净）；无身份型证据机制。
- Gate 5：host 全部门、case/schema cross-check、RISC-V static guest build、`SMP=16` kernel build、单一 network-only runtime 均有新鲜输出与退出码（见 Verification）。
- 全量 diff Review：本轮改动只涉及 4 个测试/工具文件 + evidence；工作树其余 staged 改动为 Iteration 000–004 既有 WIP（未在本 Cycle 修改）。

**Verification**

| 验证项 | 命令 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| C host test | `cc -std=c11 -Wall -Wextra -Werror tests/ms08_network_smp_probe_test.c -o /tmp/ms08-net-probe-test && /tmp/ms08-net-probe-test` | `C TEST: PASS` | registry=6、schema=6、决策谓词 | PASS |
| peer self-test | `python3 scripts/ms08-network-peer.py --self-test` | `PEER: PASS` | PEER_CASES=4、帧协议、ledger | PASS |
| validator self-test | `python3 scripts/ms08-network-validate.py --self-test` | `VALIDATOR: PASS` | 六 case grammar/负向 fixtures | PASS |
| case/schema cross-check | `--print-cases/--print-schema` + `diff -u` | `CASES MATCH: PASS` `SCHEMA MATCH: PASS` | C 与 Python 三端一致 | PASS |
| guest syntax | `cc ... -fsyntax-only tests/ms08_network_smp_probe.c` | `GUEST SYNTAX: PASS` | 全部函数可达、无 unused static | PASS |
| guest static build | `riscv64-linux-musl-gcc ... -o /tmp/ms08-net-probe-rv && --self-test` | `RV GUEST: PASS`；6 case 列表 | RISC-V 静态载荷 | PASS |
| kernel build | `make ARCH=riscv64 SMP=16 build` | `Finished release profile`；生成 `.bin` | SMP=16 内核 | PASS |
| 6-case transcript | `python3 scripts/ms08-network-validate.py /tmp/ms08-net-sixcase-transcript.txt` | `SIXCASE TRANSCRIPT: PASS` | 新 profile 判定契约（自测用） | PASS |
| Makefile purity guards | grep 守卫 6 组 | 全部 PASS | sleep-poll/validator/peer 纯度 | PASS |
| runtime（用户手动） | QEMU `SMP=16` + guest probe + host peer | 六 case `PASS` + `MS08_NET_HARNESS_EXIT: 0`；pcap 双向 echo 帧对应 | 见 Evidence EV-005-003-01..05 | PASS |

- OpenSpec validate：`openspec validate ms08-qemu-multi-hart-correctness-baseline` → `valid`（exit 0）。

**Persisted Evidence**

- `openspec/changes/ms08-qemu-multi-hart-correctness-baseline/evidence/005-per-driver-smp16-fixed-qualification/003-rework/`（user-required；README.md + guest-transcript.log + diag.pcap，3 文件，各文件行数/体积在预算内）。
- Plan 原模式为 `none`；用户明确要求采集到证据目录，按 user-required 记录并说明白名单理由（README）。

**Experience Candidates**

- Runbook 候选：`ms08-qemu-smp16-qualification.md` 需按最终 profile 修订——六 case 流程、无 HMP/migration/reset/link 步骤、validator 不再作判定工具、peer 结果行需等 peer 自然超时退出或以 pcap 佐证。证据：本 Act Response + Evidence README。
- Issue 候选：无（boot `[NET-MIG-SMOKE] FAIL` 为已知间歇竞态 B1，用户已豁免迁移结论，不构成新缺陷登记）。

**Unresolved issues**

- `make host-test` 中 `tests/ms04-async-rx-host-harness.rs::net_migration_stimulus_is_blocked_gated_in_source` 失败：已验证为 pre-existing（stash 本 Cycle 改动后同样失败），源于 Iteration 001–003 已暂存的 `net_placement.rs` 与 harness 的源码结构断言失配；属本 Cycle 明确排除的 migration 范围，不影响本 Cycle 通过判定；遗留待后续 Iteration 或 Plan Review 处理。
- 受控迁移稳定性、UART 专项、reset/link 交错、组合压力按用户决定不作新结论。

## Plan Review

- Review Result: accepted

**Findings**

- No blocking finding. The current guest probe, peer and optional validator all expose the approved six-case profile. `run_probe` terminates after `readiness-quiet`; no migration, reset or link choreography remains in the final runtime path.
- Fixed placement is directly supported by the runtime V5 snapshots: `configured=16`, `schedulable=0xffff`, owner affinity/last hart `15/15`, runner affinity/last hart `0/0`, with nonzero role progress. Cumulative extra hart bits are explained by Restored diagnostic migration records and do not change the current singleton affinities.
- The timer-disabled witness directly records remote trigger `0`, target `1`, `ipi_before=0`, `ipi_after=1`, one completion, one timer restoration and no missing restore.
- TCP and UDP each completed eight framed round trips. Full recovery observed `tx_occ=64`, then converged to `tx_occ=0`, `live=0`, zero buffer/descriptor inflight and `tx_submit=tx_completion=tx_reclaim=322`. Readiness/quiet completed one framed exchange and the guest's bounded silent-window check returned PASS.
- Independent pcap inspection found TCP sequences 1–8, UDP sequences 1–8, Full-recovery sequences 1–301 and readiness sequence 1; every sequence appeared exactly twice, once in each direction, with no non-paired sequence.
- Accepted deviation: the user explicitly waived validator-based transcript acceptance, peer natural-exit output, migration qualification and old regression reruns. Guest-native predicates/exit plus pcap are the governing evidence. The missing peer result and the boot migration-smoke failure therefore do not block the revised network-only claim.
- Accepted deviation: persisted evidence changed from `none` to user-required. The three files are within the repository budget: README 26 lines, guest transcript 276 lines/85,480 bytes and pcap 54,358 bytes.
- Minor, non-blocking: `tests/ms08_network_smp_probe` is `AM`; the working-tree RISC-V executable is the rebuilt final profile while its staged copy predates that rebuild. A future commit must stage the working-tree executable together with the final source. No commit was requested in this Cycle.

**Deviation Classification**

`ACT-DEVIATION` for replacing validator/peer terminal verdicts with the user-approved guest+pcap decision; `NEW-EVIDENCE` for the user-required evidence directory. Both are explicitly authorized and do not leave an Acceptance gap.

**Acceptance Gaps**

None within the approved network-only boundary. UART qualification, controlled migration stability, reset/link interleave, combined pressure and old runtime regressions remain explicitly waived and are not part of the conclusion.

**Convergence**

Reduced to zero relative to Cycle 002 and the Cycle 003 Plan Context: the final profile now terminates after the six retained network cases and has a complete bounded runtime result.

**Evidence**

- Code: `tests/ms08_network_smp_probe.c::{ms08_net_cases,ms08_net_schema,ms08_net_peer_cases,run_probe}`, `tests/ms08_network_smp_probe_test.c`, `scripts/ms08-network-peer.py::PEER_CASES`, and `scripts/ms08-network-validate.py::{EXPECTED_CASES,PEER_CASES,CASE_GRAMMAR}`.
- Runtime: `evidence/005-per-driver-smp16-fixed-qualification/003-rework/guest-transcript.log` contains the six ordered PASS markers, `MS08_NET_END` and `MS08_NET_HARNESS_EXIT: 0`, together with the decisive V5 relations.
- Peer traffic: `evidence/005-per-driver-smp16-fixed-qualification/003-rework/diag.pcap`; independent `tcpdump` inspection produced `tcp 8/16`, `udp 8/16`, `full 301/602`, `readiness 1/2`, with zero non-paired sequences.
- Review checks: `git diff --check`, `git diff --cached --check`, and `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` exited 0.
- Act verification results remain applicable because the four source/tool surfaces are staged with no subsequent working-tree modification; only the Cycle response and rebuilt guest executable are unstaged.

**Follow-up Decision**

Accept this Cycle. The approved network-only QEMU `SMP=16` claim is supported, all removed claims remain explicitly bounded out, and no product or test repair is required.

**Iteration Plan Update**

None. The revised Iteration 005 remains the final Iteration; Tasks 7.1–7.3 stay explicitly skipped by user decision.

**Next Cycle**

None.

**Next Iteration**

None.
