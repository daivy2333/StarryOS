# Iteration 004 / Cycle 001: Repair MS08 qualification protocol paths

## Plan Context

- Status: ready
- Iteration: 004-ms08-uart-and-network-qualification-protocols
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 5.1–5.4.
- Depends on: accepted Iteration 003 and Cycle 000's source/build outputs.
- Stable baseline: UART and network guest/host protocols whose output
  validators can decide all MS08 cases before runtime qualification.
- Verification boundary: static guest builds, host decision and negative
  fixtures, schema/source guards, old protocol compatibility, plus focused
  real-interface witnesses for the failures identified by Cycle 000 Review.
- Diagnostic boundary: serial/peer transport, witness trigger, diagnostic
  control, migration observation, or validator decision.
- Deferred tasks: 6.1–7.3.

**Cycle Scope**

- Trigger: rework-required.
- Acceptance gaps: 5.1 UART migration middle view; 5.2 wake/TCP/Full/
  readiness/network migration and peer result; 5.3 behavior-only validation;
  5.4 effective focused guards.
- Repair items: 5.1-R1, 5.2-R1, 5.2-R2, 5.2-R3, 5.2-R4, 5.2-R5, 5.3-R1,
  5.4-R1.
- Inherited scope: D6–D9, original 5.1–5.4 Acceptance, no kernel ABI
  change, no sleep polling, old protocol compatibility and Evidence `none`.
- Excluded scope: full QEMU runtime qualification, new kernel controls,
  product driver changes, identity checks, and Iteration 005–006 work.

**Objective**

Make the existing guest/host protocol executable against its real ioctl,
socket and peer interfaces, and ensure each validator bases acceptance on
independent behavior observations.

**Background**

Cycle 000 produced both static guest binaries, eight source files and
Makefile entries. Its host fixtures passed, but Review found deterministic
interface contradictions in the paths that those fixtures modeled. These
repairs serve the original Iteration 004 goal; the Iteration Map is unchanged.

**Investigation Facts**

- Current Baseline: Cycle 000 Act reports guest static builds, focused host
  fixtures and old `make host-test` passing. Review independently reproduced
  the missing real-socket attribute (`socket.socket` has no `_rxbuf`) and
  inspected the other mismatches directly in source. Existing binaries are
  outputs, not independent proof of guest behavior.
- Current-State Evidence: `net_wake_witness.rs::control` leaves START in
  `Starting`; only accepted TRIGGER after saved target state `Blocked`
  promotes it to `Armed` and wakes it. `migrate_copier` in
  `uart_smp_snapshot.rs` and `migrate_role` in `net_placement.rs` are
  synchronous: each returns after widen/observe/restore. `axnet::diag`
  accepts HOLD_SUBMIT op 1 with a 1..=2000 ms lease and RELEASE op 3 with
  lease 0. QEMU TCP clients use nonblocking sockets; an in-progress connect
  must be completed via writable readiness and `SO_ERROR`. The guest already
  has `sched_setaffinity`/`sched_getaffinity` syscalls.
  `axtask::set_current_affinity` migrates a caller immediately when its new singleton
  excludes the current hart. `spawn_task_with_affinity` puts the witness
  target on its singleton run queue before START returns; the locked RR
  scheduler enqueues at the tail and selects from the head. In
  `witness_target`, timer IRQ is disabled before its `block_on` park, and no
  yield occurs between those operations. A caller migrated to that target
  run queue after START is queued behind the target. When the caller resumes
  on the target hart, the target has reached its Blocked transition. This
  queue-order baton uses existing scheduler behavior, not a new notification
  ABI or a timeout guess.
- Code and Critical Path: guest ioctl and peer socket operations produce
  snapshots/frames; host serial/peer services capture independent bytes and
  counts; pure validators parse joined output. Cycle 000 presently waits for
  `Armed` before TRIGGER, closes `EINPROGRESS` TCP connects, stores a TCP
  buffer on `socket.socket`, uses invalid diagnostic lease/release values,
  consumes readiness echoes before checking readiness, samples migration
  only after synchronous ioctl returns, and ignores the real peer result.
  Its generic `wait_until()` calls `poll(NULL, 0, slice)` repeatedly; that
  is sleep polling even though the Makefile source guard does not match it.

**Implementation Guidance**

First add RED witnesses that exercise each real interface contract, including
a socket-shaped object without a writable attribute dictionary and injected
`EINPROGRESS`/partial TCP I/O. Then repair the guest call order and peer
framing, followed by validators and schema guards. Keep each repair under
its existing task and preserve the case names unless an existing Acceptance
cannot be expressed without a protocol adjustment.

For the wake case, use a single-thread affinity baton: pin the caller to a
chosen controller hart, START on a different target hart, migrate the same
caller to target and back to controller, then call TRIGGER once. The target
is ahead of the caller in the target RR queue. After TRIGGER enqueues the
target again, migrate the caller to target a second time; its return means
the target had its wake turn, so one V5 read decides the terminal result.
No sleep, retry loop, new task or new kernel control is needed. A failed
migration or rejected TRIGGER is a failed case, not a reason to retry.

**Behavioral Change**

Existing case names and kernel interfaces remain. UART and network migration
middle states become observable while the synchronous ioctl is in progress;
network wake, TCP, Full and readiness cases can reach their intended
terminal behavior. Validators reject missing host peer results and no longer
use environment text as an acceptance key.

**Task Contracts**

### 5.1-R1: Observe UART migration concurrently with the ioctl

- Requirement/Scenario: 5.1, UART RX/TX migration from Widened to Restored.
- Depends on: Cycle 000 UART guest and serial harness.
- Targets: `tests/ms08_uart_smp_probe.c::run_copier_migration` and focused
  host fixture.
- Current behavior: the caller blocks in `UART_SMP_MIGRATE` until Restored,
  then starts observing; Widened is missed.
- Required behavior: a separate bounded control caller invokes the same
  ioctl while the observer drives RX/TX progress, samples Widened and
  Restored, verifies child completion and preserves one copier per direction.
- Required changes: separate control from stimulus/observation; propagate
  child failure and deadline; drain/compare all injected UART frames.
- Preserve: case names, serial frame ledger, wire layout, SPSC endpoint and
  migration identity.
- Forbidden: second copier, synthetic middle state, unbounded child wait or
  reliance on a marker printed before an actual snapshot.
- Test witness: fake blocking ioctl/observer seam must go RED with the
  current sequential caller, then GREEN with a Widened observation while
  the control call is still outstanding; both RX/TX cases covered.
- GREEN condition: middle and restored snapshots plus child exit form a
  complete bounded migration record.
- Verification: UART host fixture, serial harness self-test and static build.
- Stop when: an observer cannot coexist with this ioctl/TTY path without
  changing the approved ABI.

### 5.2-R1: Make the timer-disabled wake trigger reachable

- Requirement/Scenario: 5.2, blocked target wakes by remote IPI without
  timer-only progress.
- Depends on: existing wake control and V5 witness fields.
- Targets: `tests/ms08_network_smp_probe.c::run_timer_disabled_wake` and
  focused state/call-order fixture.
- Current behavior: guest waits for Armed before calling TRIGGER; kernel
  only enters Armed inside accepted TRIGGER from Starting.
- Required behavior: preserve the caller's original affinity, select two
  distinct schedulable harts (`controller`, `target`), pin the caller to
  controller and invoke START with explicit target. After START returns,
  change the same caller's affinity to singleton target. This migration
  enqueues it behind the already-enqueued target on the target RR run queue.
  On return from `sched_setaffinity`, change affinity back to controller;
  this second migration executes on a hart different from target. Invoke
  TRIGGER exactly once. After its successful remote enqueue, migrate the
  caller to target again so it runs after the woken target, then take one
  V5 snapshot and require Completed, timer restored and the target hart's
  own IPI receive count to increase. Restore the original affinity on every
  exit path and issue CANCEL if a started run fails; after an accepted
  CANCEL, use the same target-hop ordering before checking timer restore.
- Required changes: remove the pre-trigger Armed wait and child retry loop;
  add checked controller/target affinity transitions before and after one
  remote TRIGGER, then a single terminal observation. Require at least two
  schedulable harts; fail
  closed if either affinity syscall fails or TRIGGER is rejected.
- Preserve: kernel witness state machine, single-flight, early/local trigger
  rejection, V5 wire layout and target timer restoration.
- Forbidden: polling `Armed` before TRIGGER, `poll(NULL, 0, ...)`, a retry
  loop, timer-only fallback, a new kernel ABI, or claiming IPI causality
  from global counters alone.
- Test witness: a host scheduling seam models the target's prior enqueue,
  RR `push_back`/`pop_front` order, immediate caller migration and target
  no-yield park. It must reject the old START→wait-Armed→TRIGGER trace and
  accept only controller→START(target)→target-hop→controller-hop→TRIGGER
  once→target-hop→Completed. A source guard confirms axtask ordering and
  absence of a yield before target park; failure cleanup tests require
  original affinity restoration and CANCEL after START.
- GREEN condition: the guest reaches TRIGGER from a non-target hart after
  the target has had its turn to block; after a second target hop, one V5
  read shows Completed, timer restoration and target IPI increase. No
  polling or timing guess determines success.
- Verification: focused order/cleanup fixture, `sched_setaffinity` guest
  static build, source-order guard and later Iteration 005 QEMU runtime case.
- Stop when: scheduler ordering or immediate affinity migration differs
  from the inspected RR/current-affinity implementation, or a valid
  target-hop can resume before the target blocks; return to Plan rather
  than adding delays or retries.

### 5.2-R2: Complete nonblocking TCP and host peer framing

- Requirement/Scenario: 5.2, TCP/UDP bidirectional exact-byte peer exchange.
- Depends on: Cycle 000 peer codec and per-case ledger.
- Targets: `tests/ms08_network_smp_probe.c::{open_peer_socket,
  peer_roundtrip}`, `scripts/ms08-network-peer.py::serve_until_deadline`
  and both focused fixtures.
- Current behavior: normal TCP `EINPROGRESS` is fatal; the Python peer writes
  `_rxbuf` onto a real socket and uses `sendall()` on a nonblocking socket
  without handling partial progress.
- Required behavior: guest waits for writable TCP connect and checks
  `SO_ERROR`; guest and peer both buffer partial stream frames externally
  and send all bytes through bounded writable readiness. UDP remains
  datagram-oriented. Peer ledger still rejects duplicate/out-of-order data.
- Required changes: keyed per-connection receive/send buffers owned by the
  peer service; explicit close cleanup and deadline checks after each I/O.
- Preserve: frame grammar, per-case sequence and exact-byte echo.
- Forbidden: attributes on `socket.socket`, assuming one recv/send is one
  TCP frame, blocking sendall on a nonblocking socket, or silent truncation.
- Test witness: real-object no-dictionary socket check; injected EINPROGRESS,
  split/coalesced TCP frames and short writes/reads all RED then GREEN.
- GREEN condition: complete exact frames reach the guest and peer ledger
  under partial I/O; malformed/late data fails closed.
- Verification: focused C and Python fixtures, peer self-test, static build.
- Stop when: guest networking cannot expose `SO_ERROR` or writable poll.

### 5.2-R3: Use the existing diagnostic and readiness contracts

- Requirement/Scenario: 5.2, Full to recovery and poll/readiness/quiet.
- Depends on: `axnet::diag` op/lease contract and existing socket readiness.
- Targets: `tests/ms08_network_smp_probe.c::{run_full_recovery,
  run_readiness_quiet}` and focused host fixture.
- Current behavior: HOLD_SUBMIT requests 30000 ms (max 2000); RELEASE uses
  op 0 (actual 3). Readiness waits after an echo was already consumed and
  consumes the next echo before its final read.
- Required behavior: hold with valid 1..=2000 ms lease, reach actual Full
  before lease expiry or return a bounded failure, release with op 3/lease
  0 on every path, then prove capacity recovery. Readiness must arm POLLIN
  before sending a frame and consume the reply exactly once after wake;
  quiet observes no spontaneous progress after traffic settles.
- Required changes: correct constants and bounded cleanup; separate
  readiness send/observe/read from roundtrip's consuming helper. Replace
  repeated `poll(NULL, 0, slice)` snapshots with blocking readiness on the
  affected socket or an explicit host serial control byte; the quiet case
  makes one bounded wait on its observed fd and then one counter snapshot.
- Preserve: C4 flush, descriptor/slot/ticket ledger and exact peer sequence.
- Forbidden: fake Full marker, accepting auto-expired hold as deliberate
  release, or waiting for a reply already consumed.
- Test witness: injected diagnostic ioctl asserts op/lease and cleanup on
  every exit; a socket fixture counts send, readable event and single recv.
- GREEN condition: valid controls and one-shot readiness relationship pass.
- Verification: focused host fixture, static build, validator negatives.
- Stop when: current diagnostic hold cannot reach Full within its 2 s lease;
  return to Plan for a scope/acceptance decision.

### 5.2-R4: Observe network migration during the blocking control

- Requirement/Scenario: 5.2, owner and runner natural migration with ledger
  continuity.
- Depends on: Cycle 000 V5 migration views and peer traffic.
- Targets: `tests/ms08_network_smp_probe.c::run_role_migration` and fixture.
- Current behavior: synchronous `NET_IRQ_MIGRATE` returns after Restored;
  observer starts too late to capture Widened.
- Required behavior: a bounded control caller invokes ioctl while a separate
  observer drives traffic and samples pre/mid/post V5, checks peer/owner
  ledger continuity and verifies control-caller exit.
- Required changes: concurrent call/observation with terminal cleanup.
- Preserve: one owner/runner task, original affinity restore and V5 bytes.
- Forbidden: synthetic middle snapshot or accepting Restored alone.
- Test witness: fake blocking ioctl fixture and V5 sequence proof RED/GREEN.
- GREEN condition: Widened and Restored are sampled from the same role
  transition, with no second logical owner and bounded child exit.
- Verification: network host fixture, peer test and guest static build.
- Stop when: approved user-space interfaces cannot support concurrent
  observation without kernel changes.

### 5.2-R5: Replace generic sleep polling with observable events

- Requirement/Scenario: 5.2, network reset/link transitions and other
  bounded guest waits without sleep polling.
- Depends on: 5.2-R1 through 5.2-R4.
- Targets: `tests/ms08_network_smp_probe.c::{wait_until,run_reset_io,
  run_link_off_on}` and network host decision fixture.
- Current behavior: `wait_until()` repeatedly calls
  `poll(NULL, 0, slice)` while looking for snapshot changes, and the
  Makefile guard does not recognize it.
- Required behavior: traffic and migration sample V5 after peer socket
  readiness or completed I/O; reset and link-down observe the old socket's
  terminal event; link-up waits for one explicit operator completion line on
  the existing guest console after the operator changes HMP state, then
  reads V5 and checks the link
  generation. Quiet performs one bounded fd wait and one subsequent
  snapshot. Every path has an absolute deadline and explicit failure result.
- Required changes: remove the generic repeated sleep helper; use relevant
  fd readiness and one-shot console input for transitions. Specify the
  completion line as `MS08_NET_HMP_DONE link=on\n`; accept it only after
  `MS08_NET_HMP_READY: link=on`, using a bounded `poll`/read on stdin.
  The operator sends the line through the existing console after HMP link-up.
  The line permits the next observation; V5 and socket behavior remain the
  Acceptance evidence.
- Preserve: existing reset/link epoch and old-socket terminal checks,
  case order and manual HMP operator boundary.
- Forbidden: repeated `poll(NULL, 0, ...)`, timing-only acceptance,
  fabricated link status or new kernel control.
- Test witness: old helper and link-up without a host action RED; bounded
  socket/serial event traces GREEN, including timeout and wrong-link V5.
- GREEN condition: no repeated sleep polling remains and reset/link cases
  advance only after their corresponding observable event and V5 check.
- Verification: focused host fixture, source guard, static build and
  schema/validator self-tests.
- Stop when: the existing serial/TTY path cannot carry the explicit host
  control byte without changing the approved interface.

### 5.3-R1: Base validators on behavior and independent peer output

- Requirement/Scenario: 5.3, pure output judgment, environment as context,
  peer accounting independent of guest claims.
- Depends on: repaired guest/host transcript fields.
- Targets: both `scripts/ms08-*-validate.py`, network peer output grammar
  and negative fixtures.
- Current behavior: validators can reject solely on `--expect-environment`;
  network validator accepts guest-authored `MS08_NET_PEER: result=ok` without
  reading the host peer's `MS08_NET_PEER_RESULT` counts/exit.
- Required behavior: accept any nonempty environment description when
  behavioral fields pass; require one independent host peer terminal result
  with successful exit and per-case rx/tx counts equal to guest successful
  exchanges. Reject zero traffic, rejects and missing/duplicate peer result.
- Required changes: remove environment matching option and its fixture;
  join host peer output with guest transcript in the validator's text input;
  enforce exact counts, not a guest assertion.
- Preserve: pure-output validators, frozen case order, old tools untouched.
- Forbidden: revision/run/host identity checks, validator-launched QEMU,
  or accepting guest-authored peer success as host evidence.
- Test witness: mutate valid transcripts to change only environment (must
  still pass), then remove/change peer result or counts (must fail).
- GREEN condition: every accepted network transcript has matching host and
  guest behavior counts and a successful independent peer terminal result.
- Verification: both validator self-tests and explicit negative fixtures.
- Stop when: peer output cannot be joined without adding identity machinery.

### 5.4-R1: Make protocol guards exercise the repaired interfaces

- Requirement/Scenario: 5.4, focused host tests and compatibility.
- Depends on: 5.1-R1 through 5.3-R1.
- Targets: `Makefile` MS08 host-test entries and new focused fixtures.
- Current behavior: fake peer sockets allow attributes a real socket rejects;
  existing fixtures pass despite invalid call order/diagnostic values.
- Required behavior: focused tests must fail on the old code for each
  confirmed gap, pass on the repair, and keep old MS03–MS07 checks passing.
- Required changes: wire no-dictionary socket, nonblocking TCP,
  wake/migration order, diagnostic op/lease, readiness and peer-result cases
  into the focused host target; preserve schema/source guards and add a
  guard against repeated `poll(NULL, 0, ...)` in both new probes.
- Preserve: old Makefile targets and existing probe/validator bytes.
- Forbidden: test-only product branches, identity tooling or treating build
  success as proof of runtime call order.
- Test witness: old behavior RED for each class, including the existing
  `wait_until()` sleep-poll helper; new focused target GREEN.
- GREEN condition: all focused cases pass and old host/test entries remain.
- Verification: focused `make host-test` components, both guest static
  builds, schema diffs, diff checks and strict OpenSpec validation.
- Stop when: a new test requires a changed product ABI or new Acceptance.

**Invariants**

No kernel ABI or driver change. No second UART copier or network owner.
Validators stay pure output consumers and environment stays diagnostic.
Every guest/peer operation is bounded; no sleep polling or fake success
markers. Existing V1–V4, UART TXDBG and MS03–MS07 protocols remain unchanged.

**Non-goals**

Iteration 005/006 runtime qualification, QEMU/HMP automation, physical
board claims, benchmark work and new evidence identity mechanisms.

**Acceptance**

- 5.1: both UART migration cases expose observed Widened and Restored views
  while a separate control caller completes successfully.
- 5.2: timer-disabled remote trigger reaches Completed; TCP/UDP peer frames
  survive real nonblocking I/O; Full and readiness use valid controls; owner
  and runner middle views are observed; reset/link waits follow observable
  events without repeated sleep polling; host peer counts match guest traffic.
- 5.3: validators reject missing/incorrect host peer results and accept a
  behaviorally valid transcript regardless of environment text.
- 5.4: the old buggy behaviors are RED in focused witnesses, repaired source
  and old protocol checks GREEN, static guest builds succeed.

**Requirements Traceability Matrix**

| Requirement | Repair | Code surface | Witness | Status |
|---|---|---|---|---|
| UART migration middle view | 5.1-R1 | UART guest | concurrent-control fixture | Covered |
| wake causality | 5.2-R1 | network guest + existing scheduler | affinity-baton order/cleanup fixture | Covered |
| exact TCP/UDP peer I/O | 5.2-R2 | network guest/peer | partial I/O and real-object fixtures | Covered |
| Full/readiness/quiet | 5.2-R3 | network guest | op/lease and one-read fixtures | Covered |
| network role middle view | 5.2-R4 | network guest | concurrent-control fixture | Covered |
| event-driven reset/link waits | 5.2-R5 | network guest | socket/serial event fixtures | Covered |
| behavior-only decision | 5.3-R1 | validators/peer output | environment/peer negatives | Covered |
| effective host gates | 5.4-R1 | Makefile/fixtures | old RED/new GREEN | Covered |

**Verification**

Run focused C/Python witnesses, peer/harness/validator self-tests, both
RISC-V static builds, case/schema diffs, affected old host checks,
`git diff --check`, `git diff --cached --check`, full repair diff review and
strict OpenSpec validation. Runtime qualification stays in Iteration 005.

**Gate 2 Readiness**

- Requirements and scope: PASS — every Cycle 000 Acceptance gap maps to a
  repair item without changing the Iteration Map.
- Investigation: PASS — synchronous migration, wake promotion, RR queue
  order, immediate affinity migration, diagnostic op/lease, TCP buffering,
  pipe-free baton and validator inputs are documented above.
- Design and executable tasks: PASS — the single-thread affinity baton
  arranges Blocked before remote TRIGGER and terminal observation after
  wake using existing scheduler order. It requires no polling, timer guess,
  new kernel interface or user-space child. Each other repair item has a
  concrete path, witness and stop condition.
- Traceability and tests: PASS — every original Acceptance gap maps to a
  repair item and an old-RED/new-GREEN focused witness.
- Verification: PASS — host order/interface fixtures and static builds
  prove the protocol contract in this Iteration; actual QEMU IPI causality
  remains the explicit Iteration 005 runtime Gate.
- No identity-style evidence engineering: PASS — no new run, environment,
  revision or peer-identity acceptance mechanism is introduced.
- Change consistency: PASS — original 5.1–5.4 and Iteration Map are unchanged.
- Persisted Evidence: PASS — mode `none`; focused results fit Act Response.
- User approval: PASS — the user approved the rewritten Cycle in this
  conversation on 2026-09-21.
- Overall Gate 2: PASS — all checks passed and the approved Plan Context is
  `ready`. Implementation remains a separate Act stage.

**Persisted Evidence**

- Mode: none.
- Budget: decisive focused output belongs in Act Response.

**Risks and Notes**

The current host sandbox rejects live sockets (`EPERM`), so focused tests
may use no-OS socket objects and injected operations to model stream
behavior. This does not replace Iteration 005's QEMU network runtime Gate.
The affinity baton depends on the existing RR queue order and immediate
`sched_setaffinity` migration. Focused source and host witnesses guard that
assumption. If actual scheduling violates it, Act must stop and return to
Plan; adding delays or retries would undermine the no-polling contract.
Iteration 005 still supplies the actual QEMU witness for IPI causality.

## Act Response

- Status: reported

**Implemented**

Round 1 (Cycle repair items per Plan Context):

- 5.1-R1 / 5.2-R4 (concurrent migration control callers): both migration
  runners spawn the synchronous widen/observe/restore ioctl in a `fork()`ed
  bounded control caller whose result travels a completion pipe.  All
  post-fork exits now route through a single `fail_cleanup` path that closes
  the socket/pipe and runs the new `reap_control_caller` (drain the pipe to
  EOF — the child's write end closes only at exit — then reap, all within
  the remaining deadline); the original failure reason stands.  `fork`/
  `pipe` failures close both pipe ends before failing.  The happy path also
  closes the pipe read end (previously leaked).
- 5.1-R1 TX record gate (Review finding 1):
  `ms08_uart_migration_record_ok` is now direction-neutral: its last
  argument is `progress_ok` computed per direction.  The RX branch passes
  drained-frame conservation; the TX branch accumulates real written bytes
  and passes the new pure `ms08_uart_tx_migration_progress_ok(pre, post,
  tx, written)` (bytes > 0 and the migrated copier's poll counter moved) —
  the RX frame count is never TX evidence, so `tx-migration` no longer
  fails its own record gate.
- 5.2-R2 (Review finding 2): `peer_drain` is deleted.  Every readable byte
  is validated: UDP datagrams and TCP-extracted frames go through the new
  pure `ms08_net_stream_frame_classify` (0 = expected, 1 = well-formed but
  unexpected — duplicate/late/wrong case — , -1 = malformed); anything not
  the exact expected frame fails closed instead of being discarded.
- 5.2-R5 (Review finding 3 of round 1): `read_v5_stable` (up to 1000
  back-to-back ioctls) is deleted.  Every call site now uses
  `read_v5_once`: one snapshot read after the preceding observable event; a
  transiently invalid current tuple fails closed.  A Makefile source guard
  pins the absence of the retry-loop helper.
- 5.4-R1 witnesses (round 1): the updated host fixtures fail to compile
  against the pre-repair probes (observed RED: 9 errors UART, 2 errors
  network), then pass (GREEN); new coverage: TX progress evidence, stream
  frame classification and the new Makefile guards.

Round 2 (latest Plan Review follow-up, this Act):

- Finding 3 (blocking cleanup bound): `reap_control_caller` in both probes
  now sets `O_NONBLOCK` on the pipe read end at entry (`fcntl`
  F_GETFL/F_SETFL); on `fcntl` failure it closes the fd and returns -1.
  The existing EAGAIN/deadline branch therefore actually runs: no `read`
  can block past the deadline while the control caller holds the write end
  inside the migration ioctl.  The bounded wait now includes `POLLHUP` —
  the new witness exposed that a pipe whose writer exited reports EOF as
  POLLHUP-only, which spun in `wait_fd(POLLIN)` until the deadline instead
  of draining.
- Structural enabler (behavior-preserving): `now_ms`, `wait_fd` and
  `reap_control_caller` moved out of the `#ifndef MS08_*_PROBE_TESTING`
  guard into the shared section of both probes, so the host fixtures
  witness the real cleanup path against real pipes and processes.  The move
  changes no code.
- Finding 4 (TCP coalesced tail): new pure `ms08_net_stream_rx_audit` —
  after the single expected echo frame is accepted, every remaining
  complete frame (always unexpected under the one-echo-per-request grammar,
  however well-formed) and any partial tail fail closed before the exchange
  is claimed, including the final exchange.  Wired into `peer_roundtrip`'s
  TCP branch; a Makefile source guard pins the wiring.
- 5.4-R1 runtime-path witnesses: both fixtures now exercise
  `reap_control_caller` with a real child holding the pipe write end open
  (must return -1 within the deadline; pre-repair code hangs — observed RED
  `timeout` exit 124) and a completing child (drained to EOF, reaped, 0).
  The network fixture adds audit cases: exact exchange accepted, coalesced
  duplicate after the final frame rejected, partial tail rejected, empty
  buffer accepted, NULL rejected (pre-repair RED: implicit-declaration
  compile error under `-Werror`).

Round 3 (latest Plan Review follow-up, this Act):

- Child-lifetime gap: new `terminate_control_caller` in both probes —
  `SIGKILL` plus a bounded `waitpid(WNOHANG)` reap inside a 1000 ms grace
  window (`MS08_*_REAP_GRACE_MS`) — following the `kill(pid, SIGKILL)` +
  waitpid convention of the MS01/MS06 guest probes.  It is invoked on every
  `reap_control_caller` failure exit (fcntl failure, deadline reached,
  bounded-wait failure, read error), so a control caller still parked
  inside the migration ioctl is terminated and reaped instead of outliving
  the case with the inherited TTY and pipe write end (5.1-R1/5.2-R4
  terminal cleanup).  `reap_control_caller` still returns -1 with the
  original failure reason; the termination is bounded and its outcome is
  observable.  `#include <signal.h>` added to both probes; the guest kernel
  implements `sys_kill` (`kernel/src/syscall/signal.rs`), and real SIGKILL
  delivery to a syscall-blocked task remains part of the Iteration 005
  runtime Gate.
- 5.4-R1 witness update: both fixtures no longer kill the child themselves
  after the deadline-path reap.  They now assert `kill(pid, 0) == -1 &&
  errno == ESRCH` after both the deadline path (helper itself terminated
  and reaped the still-alive child) and the happy path (child reaped), so a
  missing production cleanup fails the fixture instead of being masked
  (pre-repair RED: assertion abort, exit 134, child confirmed alive).

**Changed Files and Symbols**

- `tests/ms08_uart_smp_probe.c`: `#include <signal.h>`;
  `MS08_UART_REAP_GRACE_MS`; new `terminate_control_caller`;
  `now_ms`/`wait_fd`/`reap_control_caller` moved above the testing guard;
  `reap_control_caller` sets `O_NONBLOCK` at entry, waits
  `POLLIN|POLLHUP`, and terminates a still-alive control caller on every
  failure exit; round-1/2 changes retained (`ms08_uart_migration_record_ok`
  direction-neutral signature, `ms08_uart_tx_migration_progress_ok`,
  `run_copier_migration` rewrite, `fail_cleanup` on every post-fork exit).
- `tests/ms08_network_smp_probe.c`: `#include <signal.h>`;
  `MS08_NET_REAP_GRACE_MS`; same `terminate_control_caller` and
  `reap_control_caller` fixes; new `ms08_net_stream_rx_audit`;
  `peer_roundtrip` audits the reassembly buffer before accepting a TCP
  exchange; round-1/2 changes retained (`ms08_net_stream_frame_classify`,
  `peer_drain` deletion, `read_v5_stable` deletion at 16 call sites,
  `run_role_migration` rewrite, `fail_cleanup` on every post-fork exit).
- `tests/ms08_uart_smp_probe_test.c`: round-1 record/progress fixtures;
  bounded-cleanup witness now asserts the helper itself terminates and
  reaps the child (`kill(pid, 0)` → ESRCH) on both the deadline and happy
  paths — no fixture-side kill.
- `tests/ms08_network_smp_probe_test.c`: round-1 classify fixtures; audit
  fixtures; same helper-owned-termination witness as the UART fixture.
- `Makefile`: round-1 guards (`poll(NULL`, `read_v5_stable` absence); new
  guard pinning `ms08_net_stream_rx_audit` inside the TCP roundtrip
  extraction window.
- Rebuilt guest binaries `tests/ms08_uart_smp_probe`,
  `tests/ms08_network_smp_probe` (RISC-V static, musl `-Werror`).
- Kernel sources: untouched.

**Deviations from Plan**

- Round-2 deviation retained and completed in round 3: the round-1
  "zombie torn down with user-process exit" record is replaced by explicit
  termination — `reap_control_caller` now kills and reaps a still-alive
  control caller on every failure exit, within a 1000 ms grace window.
  The grace window (rather than the already-passed case deadline) bounds
  the SIGKILL-to-reap wait so the cleanup outcome is achievable and
  observable; the parent's total wait stays bounded.
- `reap_control_caller` waits on `POLLIN|POLLHUP`, not the literal
  `POLLIN`: the round-2 behavioral witness proved a writer-exited pipe
  reports EOF as POLLHUP-only, so a `POLLIN`-only wait spun until the
  deadline — still an unbounded cleanup.  The deviation is required by the
  Review's "no blocking operation can bypass the deadline" and is itself
  witness-covered.
- The helper move outside the testing guard is a behavior-preserving
  refactor recorded here; it exists solely so the host fixtures can
  witness the real cleanup path.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Spec review (Gate 4): every latest-Review item maps to a repair under its
  original Task Contract (5.1-R1/5.2-R4 bounded and terminal cleanup;
  5.2-R2 exact-byte accounting; 5.4-R1 helper-owned-termination witness).
  No Acceptance, invariant or forbidden item changed: no kernel ABI change,
  no second copier/owner, validators stay pure output consumers, no sleep
  polling (`poll(NULL`/`sleep`/`usleep` guards still clean), old protocols
  untouched, no identity machinery.
- Code quality review (Gate 4): `terminate_control_caller` follows the
  existing `kill(pid, SIGKILL)` + waitpid convention of the MS01/MS06
  probes, but with a bounded WNOHANG reap instead of a blocking waitpid,
  per the Review's bounded-cleanup requirement; ESRCH/ECHILD are treated as
  already-gone.  Round-2 witness-caught fix (POLLHUP-only EOF spin) stands.
  Full-diff review: repairs are confined to the two probes, the two
  fixtures and the Makefile guard lines; rounds 1–2 repairs (baton order,
  diag op/lease, EINPROGRESS completion, single-consumption readiness,
  peer-result validation, TX record, V5 single-shot, TCP audit) are
  untouched and still GREEN.  No unresolved Critical or Important findings.
  Minor (left as-is): the waitpid WNOHANG tail loops busy-spin (pre-existing
  pattern, bounded by construction); real SIGKILL delivery to a task parked
  in the migration ioctl is guest-runtime behavior qualified in Iteration
  005 — the host contract (kill issued, bounded reap attempted, outcome
  observable) is what this Cycle proves.

**Verification Evidence**

Round-3 RED witness (observed before the fix):

- UART fixture with the helper-owned-termination assertion but the
  pre-repair reap (no termination): assertion `kill(pid, 0) == -1 &&
  errno == ESRCH` failed (exit 134, core dumped); the `pause()` child was
  confirmed still alive and cleaned up manually — exactly the gap the
  Review flagged.

Round-2 RED witnesses (observed before the round-2 fixes):

- UART fixture against the pre-repair (blocking-pipe) reap:
  `timeout 5 /tmp/t-uart` → exit 124 (hung in `read`).
- Network fixture before `ms08_net_stream_rx_audit` existed:
  `cc -std=c11 -Wall -Wextra -Werror` → error: implicit declaration of
  `ms08_net_stream_rx_audit`.
- Isolated repro of the happy-path cleanup (moved-but-unfixed reap):
  `r1=-1 elapsed=201` (deadline branch works) but `r2=-1 elapsed=5000` —
  POLLHUP-only EOF spun to the deadline; `strace` showed repeated
  `poll([{fd=3, events=POLLIN}], 1, N) = 1 ([{fd=3, revents=POLLHUP}])`.

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| UART host fixtures | `cc -std=c11 -Wall -Wextra -Werror tests/ms08_uart_smp_probe_test.c -o /tmp/t-uart && timeout 30 /tmp/t-uart` | exit 0，无输出 | 全部原断言 + TX progress + cleanup 见证（holding child 限时 −1 且已被 helper 终止回收 `kill(pid,0)`→ESRCH；completing child reaped 0） | PASS |
| Network host fixtures | `cc ... tests/ms08_network_smp_probe_test.c -o /tmp/t-net && timeout 30 /tmp/t-net` | exit 0，无输出 | 全部原断言 + classify + audit + 同上 helper-owned-termination 见证 | PASS |
| Probe 语法/宿主构建 | `cc -fsyntax-only` ×2 + `cc -Werror` 宿主构建 ×2 | exit 0 | 两个 probe guest 与宿主模式全量编译 | PASS |
| Peer/validators/harness | `ms08-network-peer.py --self-test`、`ms08-uart-validate.py --self-test`、`ms08-network-validate.py --self-test`、`ms08-uart-serial.py --self-test` | 全部 exit 0 | peer 帧泵、纯 validator 正/负例、串口 harness 状态机 | PASS |
| Case/schema 一致性 | validator 与 probe `--print-cases/--print-schema` diff ×4 | 无差异 | 冻结 case 顺序与 marker 契约 | PASS |
| Makefile guards | `poll(NULL` / `sleep` / `read_v5_stable` / audit-wiring guards | 全部 clean；wiring hit `1003: if (ms08_net_stream_rx_audit(rx)...` | 无 sleep-poll、无 V5 重试、TCP 审计接线 | PASS |
| RISC-V 静态构建 | `make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe` | 两个 ELF 重新生成（musl `-Werror`） | guest payload 目标构建 | PASS |
| 全量 host-test | `make host-test`（用户本机手动运行） | 通过（用户确认；同前轮采纳方式，覆盖范围未变化） | 含新 guards 与全部旧 MS03–MS07 检查 | PASS |
| 格式 | `git diff --check` / `git diff --cached --check` | 无输出，exit 0 | 全量 diff 空白 | PASS |
| OpenSpec | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | `Change ... is valid` | change 结构 | PASS |

**Persisted Evidence**

None required (Plan Context mode `none`; all decisive outputs fit this
Response).

**Experience Candidates**

- Runbook (deferred, unchanged): the MS08 QEMU runtime choreography awaits
  Iteration 005/006 end-to-end validation.
- Issue candidates: None.

**Remaining Issues**

- None known in protocol/host scope.  Runtime convergence (baton
  scheduling, Widened sampling probability, slirp timing) and real SIGKILL
  delivery to a task parked in the migration ioctl remain the explicit
  Iteration 005 runtime Gate with bounded failure outcomes.

**Commit or Diff Reference**

- Worktree diff (this round): 2 probes (new `terminate_control_caller`
  wired into every `reap_control_caller` failure exit, `signal.h`,
  reap-grace define) and 2 fixtures (helper-owned-termination assertions
  replacing the fixture-side kill); the cumulative Cycle diff also includes
  rounds 1–2 repairs and the earlier validator/peer/cycle-file changes.
  Pre-existing staged files from prior stages were left untouched.

## Plan Review

- Review Result: accepted

**Findings**

- Resolved — both `reap_control_caller` implementations make pipe reads
  nonblocking, wait for `POLLIN|POLLHUP`, and call
  `terminate_control_caller` on the live-writer deadline and helper-error
  paths. The latter sends `SIGKILL` and attempts a bounded nonblocking reap.
  Both focused fixtures now require the helper itself to leave no child;
  their earlier fixture-side `kill` is gone. Independent reruns of both
  fixtures pass.
- Resolved — `peer_roundtrip` audits the complete TCP receive buffer before
  accepting an exchange. Coalesced extra frames and partial tails are
  rejected; UART TX progress and one-shot V5 reading remain intact.
- Minor — the `fail_cleanup` callers discard the cleanup helper's return,
  so a guest failure to deliver `SIGKILL` is reported as the original case
  failure without a separate cleanup result. The host witness proves the
  helper's termination behavior; actual guest signal delivery and runtime
  convergence are reserved for Iteration 005. This does not block the
  Iteration 004 protocol and host-contract Acceptance.

**Deviation Classification**

None blocking. The prior ACT-DEVIATION in terminal cleanup is repaired.

**Acceptance Gaps**

None within Iteration 004. Runtime `SMP=16` qualification remains assigned
to Iteration 005 tasks 6.1–6.3.

**Convergence**

Reduced to zero from the previous Review: the child-lifetime gap is closed
by the termination path and an unmasked real-process witness.

**Evidence**

Independent source and diff review: both `terminate_control_caller` and
`reap_control_caller` implementations, their `fail_cleanup` callers, and
the real-process UART/network fixtures. The latest Act Response reports
RED before termination, then static guest builds, validators, schema checks
and `make host-test` passing; these conclusions are adopted for unchanged
protocol scope. Fresh review commands: `cc -std=c11 -Wall -Wextra -Werror
tests/ms08_uart_smp_probe_test.c -o /tmp/ms08-uart-review && timeout 30
/tmp/ms08-uart-review` and its network counterpart both exited 0 with no
output; `git diff --check` and `git diff --cached --check` exited 0;
`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`
exited 0 with `Change ... is valid`. Persisted Evidence mode is `none`.

**Follow-up Decision**

Accept Cycle 001 and complete Iteration 004. Expand the existing Iteration
Map's Iteration 005 as a draft Plan Context; runtime UART-only and
network-only `SMP=16` qualification must be decided there before Act.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../005-per-driver-smp16-fixed-qualification/000-initial.md`.
