# Iteration 002 / Cycle 000: UDP queued-TX drain ownership

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-25（原话：“批准”；随后明确要求：“但是先不自动调用act”）
- Iteration: 002-udp-queued-tx-drain-ownership
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 2.7
- Depends on: Iteration 001 accepted
- Stable baseline: UDP public handle drop preserves a submitted datagram until the resident runner dispatches it;
  the raw smoltcp handle and deferred entry are then reclaimed exactly once.
- Verification boundary: smoltcp pending-TX observation, axnet drop/reaper decisions, send→drop→peer receive→reap,
  empty drop, stale/retyped safety and quiet convergence pass in ordinary and qemu-diagnostics host profiles.
- Diagnostic boundary: failures are limited to smoltcp UDP TX-buffer observation, public/raw handle ownership,
  deferred verdict ordering, egress/reaper ordering or runner continuation.
- Deferred tasks: 2.8, 3.1–3.4

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R3/R4/R6; D3/D4/D5/D9/D11; unique resident runner; bounded deferred retirement;
  `SERVICE -> SOCKET_SET` lock order; Iteration 001 accepted listener/readiness baseline
- Excluded scope: listener cursor changes, MS01 payload or manual QEMU, terminal faults, scheduler,
  SO_LINGER, reset/cancellation, SMP, physical boards, performance, global docs, Evidence, archive and commits

**Objective**

Replace the inverted use of UDP `can_send()` with a read-only pending-TX observation. A dropped public UDP socket
must defer raw-handle removal only while a datagram remains queued, and the existing runner must dispatch and reap
that handle without a busy wake or second protocol owner.

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 queued datagram | bound UDP socket has one submitted datagram | drop public socket before runner egress | raw handle remains until peer receives the datagram, then is reaped once | close/reset loses data or reaper removes before dispatch |
| S2 empty socket | bound UDP socket has no queued TX | drop public socket | teardown is immediate; no deferred entry remains | empty socket leaks or keeps runner awake |
| S3 drained deferral | `UdpQueued` entry exists and egress empties TX | run bounded stack rounds | reaper removes raw handle and entry in one guarded commit | permanent deferral or duplicate removal |
| S4 stale/retyped | deferred handle is absent or identifies another socket type | run reaper | entry is dropped without touching the unrelated socket | stale lookup panics or removes the replacement |
| S5 backpressure/quiet | queued TX cannot dispatch yet, or all work has drained | poll runner | pending entry is retained while necessary; clean state parks | busy wake, premature reap or hidden periodic poll |

**Current Baseline**

- Branch `net-k3`; HEAD `0acc08137a5df9d3e1ebce709f3760e6d4471d2d`; the working tree contains the
  uncommitted MS06 implementation and OpenSpec records.
- Iteration 001 Cycle 009 is accepted. Task 2.6 listener, boundedness and source-guard tests are GREEN in both
  profiles; Task 2.7 is the only current Iteration scope.
- Fresh ordinary full suite: 316 passed and the three named Task 2.7 tests failed, exit 101.
- Fresh qemu-diagnostics full suite: 335 passed; the same three Task 2.7 tests plus the acknowledged `async_rx`
  flake failed, exit 101. The exact `async_rx` test passed immediately in isolation.

**Current-State Evidence**

- `smoltcp::socket::udp::Socket::can_send()` returns `!tx_buffer.is_full()`. It describes writable capacity, not
  whether a datagram is queued; an empty one-packet buffer returns true and a full queued buffer returns false.
- `UdpSocket::drop` currently names `can_send()` as `has_queued_tx`. This reverses ownership: an empty socket is
  deferred, while a queued single-packet socket falls through to `close()` and raw removal.
- `Service::reap_deferred_removals` uses the same inverted predicate for `CloseKind::UdpQueued`: it keeps an empty
  socket and reaps a queued socket.
- smoltcp UDP `dispatch()` removes one TX-buffer packet with `dequeue_with`; `poll_at()` already reports `Now`
  while TX is non-empty. A read-only `has_pending_tx()` can expose this existing state without changing dequeue,
  wire or readiness semantics.
- `Service::stack_round` runs bounded smoltcp egress before deferred retirement. The reaper can therefore observe
  the post-dispatch TX state in the same round; a retained entry remains bounded by the existing dirty/sweep rules.
- Public UDP send already publishes software work after enqueue. UDP drop can publish the same event after it
  retires public metadata and queues `UdpQueued`; no caller-driven poll or second runner is required.

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `crates/smoltcp/src/socket/udp.rs::Socket` | owns UDP RX/TX packet buffers and dispatch | add read-only pending-TX observation and its state-transition tests |
| `crates/axnet/src/udp.rs::UdpSocket::drop` | retires public metadata and raw handle | defer only when TX is actually pending |
| `crates/axnet/src/service.rs::reap_deferred_removals` | bounded raw-handle retirement | keep pending UDP; reap drained UDP; drop stale/retyped entries |
| `crates/axnet/src/stack_runner.rs` tests | exercises resident egress and public/raw ownership | prove queued echo delivery, exact reclaim and quiet convergence |

**Critical Path**

```text
UDP send -> smoltcp TX buffer non-empty -> software wake
  -> public UdpSocket drop reads has_pending_tx
     -> true: retire public metadata -> queue UdpQueued -> software wake
     -> false: close/remove immediately
  -> resident runner egress dispatches the queued datagram
  -> bounded deferred reaper observes TX empty
  -> remove raw handle + deferred entry exactly once
```

**Implementation Guidance**

Add the smoltcp read-only accessor and its direct tests before changing axnet. Use it at both ownership decisions;
do not infer pending work from capacity. Preserve `stack_round` ordering so egress precedes reaping. Existing RED
tests already pin stale/retyped behavior and the full send→drop→receive chain; strengthen source guards to reject a
return to `can_send()` for pending-TX ownership.

**Behavioral Change**

- UDP drop defers only when the raw socket contains queued TX.
- A queued datagram survives public-handle destruction and is dispatched by the unique resident runner.
- Once TX drains, the raw handle and deferred record are reclaimed together; empty drops remain immediate.
- UDP send capacity, datagram atomicity, wire format, socket readiness and public close APIs do not change.

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 2.7 | R3/R6, S1–S5 | smoltcp UDP `Socket` | expose capacity and dispatch | add read-only `has_pending_tx()` and direct tests |
| 2.7 | R4/R6, S1/S2 | `UdpSocket::drop` | destroy public/raw socket | choose deferred versus immediate removal from actual TX occupancy |
| 2.7 | R3/R4/R6, S3–S5 | `Service::reap_deferred_removals` | bounded close retirement | keep pending, reap drained, drop stale/retyped in correct order |
| 2.7 | R3/R6, S1–S5 | service/runner tests | current planned RED witnesses | prove delivery, exact reclaim, bounded continuation and quiet park |

**Task Contract**

### 2.7: close UDP queued-TX drain ownership

- Requirement/Scenario: R3/R4/R6; D3/D4/D5/D9/D11; S1–S5.
- Depends on: Iteration 001 accepted and the existing bounded deferred-reaper baseline.
- Targets: `crates/smoltcp/src/socket/udp.rs::Socket`; `crates/axnet/src/udp.rs::UdpSocket::drop`;
  `crates/axnet/src/service.rs::reap_deferred_removals`; Task 2.7 service/runner tests and source guards.
- Current behavior: `can_send()` is treated as queued-TX state, so empty and queued buffers receive opposite
  ownership decisions; three deterministic Task 2.7 tests fail.
- Required behavior: expose actual TX occupancy without mutation; defer only queued sockets; keep them until egress
  drains TX; then reclaim raw handle and deferred entry exactly once. Empty, stale and retyped paths must converge.
- Required changes: add smoltcp empty/enqueue/dispatch pending-TX RED→GREEN tests; replace both axnet capacity-based
  decisions with the accessor; retain egress-before-reap ordering and bounded dirty/sweep continuation; update source
  guards to assert occupancy use and forbid pending-TX decisions based on `can_send()`.
- Preserve: unique resident runner; UDP datagram atomicity and send-capacity readiness; public/raw handle split;
  `SERVICE -> SOCKET_SET` order; 32-entry deferred budget; TCP deferred retirement; listener/readiness baseline;
  stale/retyped safety; no guard across wake/await/Pending.
- Forbidden: synchronous dispatch from drop; axnet shadow TX ledger; changing smoltcp dequeue or wire semantics;
  caller-driven `poll_interfaces()`; another runner; scheduler, SO_LINGER, reset/cancellation or platform changes.
- Test witness: the three existing Task 2.7 tests are fresh RED. New smoltcp tests must establish empty=false,
  one queued packet=true and successful dispatch=false before axnet product changes.
- GREEN condition: all Task 2.7 direct tests pass in ordinary and qemu-diagnostics profiles; peer receives the queued
  echo; the deferred raw handle disappears once; empty/stale/retyped paths leave no entry; clean runner parks.
- Verification: targeted smoltcp UDP tests; the three named axnet tests 100 times in both profiles; UDP/deferred and
  runner filters; both full axnet suites; fmt, source guards, strict OpenSpec, whitespace and complete diff review.
- Stop when: correctness requires synchronous drop dispatch, an axnet TX ledger, altered smoltcp dequeue/wire
  behavior, unbounded retries, new timeout/cancellation semantics or a second progress owner; return to Plan.

**Invariants**

- The resident runner remains the only smoltcp progress owner.
- Raw UDP ownership moves to the deferred reaper only after public metadata retires.
- Egress precedes UDP deferred verdict evaluation in each stack round.
- A deferred record and its owned raw handle are removed atomically under existing guards.
- TCP retirement, listener readiness, PollSet capacity, backlog 512 and platform scope remain unchanged.

**Non-goals**

- Task 2.8 MS01 backlog ordering or manual QEMU runtime.
- Terminal readiness and device fault broadcast.
- Scheduler, SO_LINGER, reset/cancellation, SMP, physical boards and performance.
- Global task/SNAPSHOT synchronization, Evidence, archive or commit.

**Traceability Matrix**

| Requirement / Acceptance | Scenario | Design | Task | Code surface | Witness | Status |
|---|---|---|---|---|---|---|
| R3 bounded caller-independent progress | S1/S3/S5 | D3/D4 | 2.7 | stack round, UDP dispatch and reaper | queued echo, bounded drain, quiet park | Covered |
| R4 ownership and lock order | S1–S4 | D5/D9/D11 | 2.7 | UDP drop and deferred ledger | exact reclaim, stale/retyped source/model tests | Covered |
| R6 UDP close consistency | S1/S2 | D6/D9/D11 | 2.7 | smoltcp occupancy and public drop | empty/enqueue/dispatch states; send→drop→receive | Covered |

No Missing or Simplified requirement exists in this Iteration.

**Acceptance**

1. `has_pending_tx()` is false for an empty TX buffer, true after one enqueue and false after successful dispatch;
   it does not change capacity, dequeue or wire behavior.
2. Dropping a public UDP socket with queued TX preserves the raw handle until the peer receives the datagram; the
   runner then reclaims raw handle and deferred entry exactly once.
3. Dropping an empty UDP socket removes it immediately. Stale or retyped deferred entries are dropped without
   removing an unrelated socket or panicking.
4. Deferred inspection stays within the existing 32-entry stage, unfinished work self-wakes only while necessary,
   and drained/empty state parks without periodic polling.
5. The three planned Task 2.7 RED become GREEN in both profiles; Iteration 001 listener/readiness tests stay GREEN;
   complete diff review has no unresolved Critical or Important finding.
6. No MS01/manual QEMU, terminal, SMP, board or performance claim is made.

**Verification**

- Run new smoltcp UDP pending-TX tests for empty, enqueue and dispatch transitions.
- Run each named Task 2.7 axnet test 100 times in ordinary and qemu-diagnostics profiles.
- Run focused UDP, deferred retirement and stack-runner regressions in both profiles.
- Run ordinary and qemu-diagnostics axnet full suites; the acknowledged `async_rx` flake, if reproduced, must be
  isolated and reported rather than counted as Task 2.7 GREEN.
- Run `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`, strict OpenSpec validation,
  product caller-poll/source guards, `git diff HEAD --check` and complete diff review.
- SKIPPED: Task 2.8, manual QEMU, terminal readiness and later-platform Gates; they belong to later Iterations.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | capacity predicate, TX queue, dispatch, drop, deferred verdict, stack ordering and RED tests inspected |
| Design | PASS | one read-only accessor provides the missing state without a shadow ledger or ownership change |
| Iteration Plan | PASS | Task 2.7 forms one host/model ownership result; Tasks 2.8 and 3.1–3.4 remain separate |
| Cycle Scope | PASS | only UDP queued-TX preservation, drain and reclaim are included |
| Task Contract | PASS | RED, targets, required/preserved/forbidden behavior, GREEN and stop conditions are self-contained |
| Traceability | PASS | R3/R4/R6 map through D3/D4/D5/D9/D11 to Task 2.7, code and deterministic witnesses |
| Verification | PASS | direct state tests, three existing RED, both profiles, regressions and diff Gates are specified |
| User Approval | PASS | 用户于 2026-08-25 批准当前 Cycle，并明确要求暂不自动调用 Act |

Gate 2 全部 PASS，Cycle 已更新为 `ready`。用户本轮只批准计划，并明确要求暂不自动调用 Act；后续实施
仍等待单独的 `$openspec-act` 指令。

**Persisted Evidence**

- Mode: none

All checks are deterministic and inexpensive to rerun. The Act Response can carry decisive output and exit codes.

**Risks and Notes**

- The qemu-diagnostics `async_rx` test is an acknowledged pre-existing flake. Isolate it if it recurs; do not hide
  a Task 2.7 regression behind that classification.
- A successful smoltcp `dispatch()` dequeue is the drain boundary for this Iteration. Peer delivery remains a
  separate end-to-end witness and does not redefine device completion or MS05 flush semantics.
- Existing compiler warnings are outside this Cycle unless the implementation introduces a new warning.

## Act Response

- Status: pending

**Implemented**

Pending.

**Changed Files and Symbols**

Pending.

**Deviations from Plan**

Pending.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: pending
- Full diff reviewed: pending
- Critical findings unresolved: pending
- Important findings unresolved: pending
- Minor findings unresolved: pending

**Verification Evidence**

Pending.

**Persisted Evidence**

None required.

**Experience Candidates**

Pending.

**Remaining Issues**

Pending.

**Commit or Diff Reference**

None.

## Plan Review

- Status: pending

**Review Result**

pending

**Findings**

Pending.

**Deviation Classification**

Pending.

**Acceptance Gaps**

Pending.

**Convergence**

N/A.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

Pending.

**Next Cycle**

None.

**Next Iteration**

None.
