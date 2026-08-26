# Iteration 003 / Cycle 001: lossless concurrent-SYN listener head repair

## Plan Context

- Status: ready
- Approval: approved by user on 2026-08-26（原话："批准了，你更改gate状态，然后开始实施吧"）
- Iteration: 003-backlog-and-ms01-runtime-compatibility
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: revised Task 2.8
- Depends on: Iteration 002 accepted; Cycle 000 implementation retained
- Stable baseline: adjacent SYNs can consume available backlog in one ingress batch; deterministic overflow terminal,
  exact-512 accept/refill and immediate recovery remain compatible in single-hart QEMU.
- Verification boundary: host/model proves exact head routing and bounded repair before fresh diagnostic single/fork
  and complete MS01 14/14 runtime acceptance.
- Diagnostic boundary: hidden-listener wake, signal queue, ingress micro-step, listener state/refill, guest ordering,
  scheduler delivery and QEMU artifact/runtime compatibility.
- Deferred tasks: 3.1–3.4

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: same-batch adjacent SYN is falsely refused; Cycle 000 Acceptance 5 fails in `tcp-adjacent`.
- Repair items: T2.8-R1
- Inherited scope: R3/R4/R6/R7; D3/D4/D5/D7/D9/D10/D11; backlog 512; unique resident runner; Cycle 000
  overflow-terminal, guest deadline and 14-marker work; manual-QEMU Runbook policy
- Excluded scope: smoltcp wire behavior, idle socket pools, full listener scans, backlog increase, terminal/device
  readiness, scheduler redesign, reset/cancellation, SMP, PCI/DWMAC, physical boards, performance and QEMU automation

**Objective**

Ensure that each SYN which consumes the sole idle hidden listener causes an exact O(1) head transition/refill before
the next packet in the same ingress batch. Preserve the once-per-round bounded pending sweep and all accepted Cycle
000 behavior, then close Task 2.8 with fresh single-hart QEMU evidence.

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 adjacent SYN | one listener has backlog headroom and one idle hidden socket | queue two clients so both SYNs are processed in one ingress batch | both clients establish and can be accepted | second client is refused, delayed until an unrelated round or needs caller polling |
| S2 exact routing | two or more listeners are active | wake one hidden listener during ingress | only its head is repaired; other listener state/cursors remain unchanged | active-port scan, wrong-entry repair, lost or duplicate transition |
| S3 bounded signal | repeated wakes, budget edge and unlisten race occur | deduplicate, consume and drop stale signals | repairs are O(1), at most ingress processed count, quiet path sleeps | allocation in wake, queue overflow, reverse lock, full scan or busy wake |
| S4 overflow/recovery | backlog reaches exact 512 | decide overflow, accept/refill, reconnect immediately | Cycle 000 terminal and recovery witnesses remain GREEN | overflow/recovery ordering regresses or backlog changes |
| S5 QEMU runtime | automatic Gates and artifacts are fresh | run diagnostics and MS01 manually | diagnostics PASS; MS01 START, 14 PASS, no FAIL, END, exit 0 | stale artifact, refusal, hang, missing marker or interruption |

**Current Baseline**

- Branch `net-k3`; HEAD `4396d264787527ed7f158abf9f51f5e8f0cb706a`; Cycle 000 changes remain in the
  working tree and must be preserved.
- Automatic Cycle 000 checks pass: focused tests 3/3 in both profiles, ordinary 319/319, diagnostics retry 339/339,
  validator self-test, fmt, strict OpenSpec and whitespace checks.
- Fresh manual QEMU diagnostics single/fork pass. MS01 reaches START and `PASS: tcp-accept`, then child B in
  `tcp-adjacent` receives `ECONNREFUSED` at t=57.563 s; the parent waits forever for its second accept and emits no END.
- A fresh info-level build reproduced the same failure. The evidence attributes it above the healthy download,
  boot, diagnostics and first TCP accept layers.

**Current-State Evidence**

- `ListenTableEntryInner` owns exactly one `idle: Option<SocketHandle>` plus its pending queue. The first SYN changes
  that idle socket from Listen to SynReceived; no other hidden socket can match the next SYN until head repair.
- `Service::stack_round` processes up to 32 ingress packets and runs `ListenTable::reconcile` only after ingress and
  egress. Two SYNs 0.48 ms apart therefore share a batch, and the second reaches smoltcp's unmatched-packet RST path.
- `reconcile_head` already performs the required transition/refill in O(1). The ordinary reconciliation path must
  retain its cross-round cursor and shared 32-token pending-sweep budget.
- smoltcp's ingress result is not an exact socket signal and currently reports generic state change for nonempty
  frames, so using it to stop every batch or identify a listener is neither precise nor a valid fairness repair.
- Each hidden TCP socket owns 64 KiB RX plus 64 KiB TX buffers. Preallocating 32 idle sockets would cost roughly
  4 MiB per listener and change allocation/backlog semantics; it is outside this Cycle.

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `crates/axnet/src/listen_table.rs::{ListenTableEntryInner,reconcile_head,reconcile,listen_to,unlisten}` | hidden listener lifecycle and bounded sweep | add exact deduplicated head signal, O(1) consume/rearm and stale-signal handling |
| `crates/axnet/src/service.rs::Service::stack_round` | bounded network-stage ordering | consume at most one exact head signal after each processed ingress packet |
| `crates/axnet/src/stack_runner.rs` tests and source guards | runner/listener behavioral witnesses | add same-batch SYN, exact routing, budget, quiet and forbidden-scan guards |
| `crates/axnet/src/readiness.rs` wake patterns | allocation-free staged wake precedent | reuse the pattern only if it preserves listener-local ownership and lock order |
| Cycle 000 guest/tests/validator artifacts | overflow, exact-512 and MS01 compatibility | retain and rerun; change only when the new RED witness proves it necessary |

**Critical Path**

```text
hidden Listen socket receives SYN -> one-shot recv waker records exact deduplicated entry signal
  -> current ingress packet returns -> Service consumes at most one signal
  -> O(1) reconcile_head commits transition/refill/rearm -> next ingress packet
  -> normal once-per-round 32-token pending sweep -> staged application accept wake
```

**Design Decision**

Give each listener an internal head signal whose one-shot hidden-socket recv waker enqueues that exact listener into
a pre-reserved service-visible queue. The waker only performs bounded deduplication and notification: it must not
allocate, mutate the entry or SocketSet, take entry/SocketSet/Service locks, or wake application accept waiters.

After each processed ingress packet, Service consumes at most one queued signal and invokes only the identified
entry's O(1) `reconcile_head` with the existing SocketSet ownership. It commits idle transition/refill and rearms the
hidden recv waker before processing the next packet. The number of micro-repairs is therefore no greater than the
number of processed ingress packets and the ingress budget of 32. The ordinary listener pending sweep remains once
per round with its independent shared 32-token cursor. Application wake remains staged until state is committed and
all guards are released. An unlisten race leaves a stale identifier that is safely discarded.

**Rejected Alternatives**

- Do not restore per-ingress active-port or pending-queue scans; that reintroduces the Task 2.6 latency defect.
- Do not preallocate an idle socket pool; its memory cost and allocation semantics are disproportionate and it only
  masks the missing transition contract.
- Do not stop ingress on generic smoltcp `SocketStateChanged`; it neither identifies the listener nor distinguishes
  the relevant transition and would effectively reduce nonempty ingress to one packet per round.
- Do not change smoltcp's unmatched-packet RST behavior, raise backlog or add sleeps/caller-driven polling.

**Behavioral Change**

- Available backlog headroom remains visible between adjacent packets in one ingress batch.
- The listener pending sweep, backlog 512, accept delivery and Cycle 000 overflow/recovery behavior remain unchanged.
- Internal head repair gains its own ingress-counted bound; it does not make SMP, hardware or performance claims.

**Change Surface**

| Task/Repair | Requirement/Scenario | File/Symbol | Planned change |
|---|---|---|---|
| T2.8-R1 | R3/R4/R6, S1–S3 | `listen_table.rs` | exact pre-reserved signal, deduplication, O(1) head repair/rearm and stale drop |
| T2.8-R1 | R3/R4, S1/S3 | `service.rs::stack_round` | one signal micro-step after each processed ingress packet, bounded by ingress count |
| T2.8-R1 | R3/R4/R6, S1–S4 | `stack_runner.rs` tests/guards | RED same-batch SYN plus routing, budget, quiet, overflow and recovery regression witnesses |
| T2.8-R1 | R7, S5 | fresh artifacts/manual QEMU | rerun diagnostics and full 14-marker MS01 after all automatic Gates |

**Task Contract**

### T2.8-R1: repair adjacent-SYN headroom without restoring listener scans

- Requirement/Scenario: R3/R4/R6/R7; D3/D4/D5/D7/D9/D10/D11; S1–S5.
- Depends on: Iteration 002 accepted; Cycle 000 changes preserved.
- Targets: listener internal signal and head transition; service ingress ordering; stack-runner tests/guards; existing
  overflow/recovery and guest workload; fresh single-hart QEMU artifacts and manual execution.
- Current behavior: only one hidden socket is in Listen, while refill occurs after an ingress batch of up to 32;
  a second same-batch SYN is falsely refused despite backlog headroom.
- Required behavior: the exact listener head is transitioned, refilled and rearmed before the next ingress packet;
  both adjacent clients establish, and all repair work remains explicitly bounded.
- Required changes: first add a RED same-batch two-SYN witness; add a pre-reserved exact signal with per-entry dedup;
  consume at most one signal per ingress packet through O(1) `reconcile_head`; safely discard stale unlisten signals;
  preserve Cycle 000 assertions and rerun all automatic and manual Gates.
- Preserve: unique resident runner; one main listener sweep per round and its 32-token cursor; ingress budget 32;
  backlog 512; atomic accept/refill; staged guard-free application wake; 14 markers and manual-QEMU policy.
- Forbidden: allocation or entry/SocketSet/Service lock acquisition in the waker; signal loss at capacity; reverse
  lock order; active-port/full-queue scan per packet; idle pool; backlog increase; sleep/caller poll; scheduler,
  smoltcp wire, reset/cancellation, SMP or board changes.
- Test witness: two clients must place SYNs in one runner ingress batch; the test is RED only when the second becomes
  refused/Closed and GREEN only when both establish and are accepted. Separate tests must detect wrong-entry routing,
  duplicate repair, more repairs than processed ingress packets, stale-signal misuse and quiet busy-wake.
- GREEN condition: S1–S4 pass in ordinary and qemu-diagnostics profiles; full suites, guards, fmt, strict OpenSpec,
  payload/kernel builds and diff review pass; fresh manual QEMU satisfies S5.
- Verification: focused repeated listener tests, existing 31/32/33/512 cursor and Task 2.8 tests, both full axnet
  profiles, validator/source guards, payload/kernel builds, then the Runbook manual diagnostic/MS01 batch.
- Stop when: the signal cannot be delivered without allocation, loss or reverse locking; exact routing requires a
  full scan; correctness needs an idle pool, larger backlog, periodic polling, smoltcp/scheduler/reset changes; or a
  lower-layer QEMU failure prevents attribution. Return to Plan instead of widening this Cycle.

**Invariants**

- No Service, SocketSet, listener or readiness guard crosses wake, await, Pending or yield.
- The waker records work; only the resident runner mutates listener and smoltcp state.
- Head micro-repair and the normal pending sweep have separate bounds and responsibilities.
- Host/model evidence cannot replace QEMU runtime evidence; single-hart QEMU cannot support SMP or hardware claims.

**Non-goals**

- Tasks 3.1–3.4, terminal fault readiness and the final MS06 application probe.
- smoltcp protocol changes, idle pools, scheduler redesign, reset/cancellation, SMP, physical boards and performance.
- Automated QEMU shell control, global docs, Evidence directory, archive or commit.

**Traceability Matrix**

| Requirement / Acceptance | Scenario | Design | Task | Witness | Status |
|---|---|---|---|---|---|
| R3 bounded progress | S1/S3 | D3/D4 | T2.8-R1 | repairs ≤ processed ingress ≤ 32; quiet path sleeps | Covered |
| R4 ownership/order | S1–S3 | D5/D7/D9 | T2.8-R1 | exact waker signal, runner-only mutation, staged wake | Covered |
| R6 listener compatibility | S1/S2/S4 | D7/D11 | T2.8-R1 | same-batch two SYN, multi-listener routing, exact-512 recovery | Covered |
| R7 QEMU boundary | S5 | D10 | T2.8-R1 | fresh diagnostics and MS01 14/14 + END + exit 0 | Covered |

No Missing or Simplified requirement remains. User approval is the only Gate 2 blocker.

**Acceptance**

1. Two clients whose SYNs are processed consecutively in one ingress batch both establish and can be accepted while
   backlog headroom exists; neither needs sleep, caller polling or an unrelated runner round.
2. With multiple listeners, a hidden-socket wake repairs only its exact entry. Duplicate signals coalesce, stale
   unlisten signals are harmless, and application accept wake occurs only after committed state with guards released.
3. Each processed ingress packet causes at most one O(1) head repair, total repairs do not exceed ingress count or 32,
   the main listener sweep remains once per round with an independent 32-token cursor, and quiet path does not spin.
4. Existing 31/32/33/512 listener tests and Cycle 000 terminal-overflow, exact-512, immediate recovery, deadlines and
   exact 14-marker source/validator witnesses remain GREEN in both profiles.
5. Full automatic suites, format, strict OpenSpec, payload/kernel builds and complete diff review pass without an
   unresolved Critical or Important finding.
6. Fresh manual RISC-V `virt`, `-smp 1`, VirtIO-MMIO QEMU reports diagnostic single/fork PASS and MS01 one START,
   14 unique PASS, zero FAIL, one END and exit 0.

**Verification**

- Run focused same-batch SYN, exact-routing, deduplication, stale-unlisten, budget/quiet and overflow/recovery tests
  in ordinary and qemu-diagnostics profiles; repeat deterministic bounded cases where practical.
- Run the existing listener 31/32/33/512 and full axnet suites in both profiles, marker validator self-test, source
  guards, axnet fmt, strict OpenSpec and parent/smoltcp whitespace/diff review.
- Rebuild both RISC-V payloads and `StarryOS_riscv64-qemu-virt.bin`; record revision, command, exit and artifact time.
- Only after automatic Gates pass, follow `.claude/runbooks/qemu-network-testing.md` manually: diagnostic `single`,
  diagnostic `fork`, then MS01. Record decisive markers and exit status.
- SKIPPED: Tasks 3.1–3.4, SMP, board and performance Gates; they do not decide this Cycle.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | manual QEMU was reproduced and mapped to one idle socket plus delayed head refill |
| Design | PASS | exact signal gives O(1) per-packet repair without restoring the removed sweeps or allocating a pool |
| Iteration Plan | PASS | revised Task 2.8 remains the same Iteration's runtime-compatibility result |
| Cycle Scope | PASS | listener head ordering and its direct regression witnesses only |
| Task Contract | PASS | RED, ownership, bounds, preservation, forbidden scope, GREEN and stop conditions are explicit |
| Traceability | PASS | R3/R4/R6/R7 map to host/model and fresh QEMU witnesses |
| Verification | PASS | automatic dependency Gates precede the unchanged manual single-hart acceptance |
| User Approval | PASS | approved by user on 2026-08-26（原话："批准了，你更改gate状态，然后开始实施吧"） |

Gate 2 is now PASS; approval was granted explicitly and does authorize Act per the user's instruction.

**Persisted Evidence**

- Mode: none

The Cycle and Act Response may retain decisive commands, marker summaries and exit codes. Raw serial logs are not
required unless a future blocker would lose necessary attribution.

**Risks and Notes**

- The internal queue must have a demonstrable capacity invariant. If pre-reservation cannot guarantee lossless
  delivery for all registered listeners under deduplication, stop rather than silently dropping a head transition.
- Source guards should enforce the single main listener sweep and absence of per-packet active/full scans without
  forbidding the exact O(1) head micro-step.
- Cycle 000 product-independent overflow and guest changes are retained; do not revert them while repairing the
  earlier concurrent-SYN regression.

## Act Response

- Status: reported

**Implemented**

T2.8-R1 executed end-to-end with TDD. The RED same-batch two-SYN witness failed first exactly as contracted
(client A `ESTABLISHED`, client B refused/Closed via RST), then turned GREEN after the exact head-signal repair.

1. Exact deduplicated head signal (`listen_table.rs`): new pre-reserved `HeadSignals` shared state — a port-indexed
   dedup bitmap plus a FIFO ring of `AtomicU16` slots sized `PORT_NUM`, lossless by construction (dedup keeps at most
   one queued instance per port). New `HeadSignalWaker` (one per listener entry) records only a bounded signal on its
   one-shot wake: it never allocates, never takes an entry/SocketSet/Service lock and never wakes application accept
   waiters.
2. Idle waker rewiring (`listen_table.rs::{refill,reconcile_head}`): the idle hidden socket's one-shot recv slot now
   carries the exact head-signal waker instead of the accept bridge; application accept wake stays fully staged
   (committed transition → `pending_accept_wakes` → `drain_accept_wakes` after guards release). Queue-slot arming in
   `examine_slot` and all backlog semantics are untouched.
3. Per-packet micro-repair (`service.rs::stack_round`): inside the ingress stage, each processed packet (Processed or
   SocketStateChanged) consumes at most one signal through the new O(1) `ListenTable::consume_head_signal`
   (pop → clear dedup bit before repairing → `reconcile_head` on exactly that entry; stale unlistened identifiers are
   discarded; committed transitions stage their accept wake). Repairs are bounded by processed ingress ≤ 32; the main
   once-per-round listener sweep keeps its independent 32-token cursor.
4. Observability: `StackRoundOutcome.listener_head_repairs` wired through runner telemetry and `StackSnapshot`.
5. Witnesses: RED/GREEN same-batch two-SYN chain test (100×); exact-routing (unsignaled entry untouched), duplicate
   coalescing, stale-unlisten discard, quiet no-op unit tests (100× each); runtime bound test proving 33 signaled
   listeners whose 3 loopback frames are driven until actually ingested yield **exactly 3** repairs — one per
   processed packet, consume↔repair 1:1, zero signal loss (remaining 30 queued) — polled past the dispatch-only
   first round so a vacuous zero-repair pass cannot satisfy it; source guard `task_28_head_micro_step_is_exact_bounded_and_lock_free`
   enforcing the single ingress-stage consume site, `PORT_NUM` ring capacity, lock-free/allocation-free waker code and
   loop-free O(1) consume body; updated idle-wake unit test to the staged-only contract.

**Changed Files and Symbols**

| File | Symbols |
|---|---|
| `crates/axnet/src/listen_table.rs` | `HeadSignals{signal,enqueue,pop,clear_pending,len}`, `HeadSignalWaker{waker,Wake}`, `ListenTableEntryInner{head_signal,new,refill,reconcile_head}`, `ListenTable{head_signals,consume_head_signal,test_signal_head,test_pending_head_signals,test_close_idle}`, tests module |
| `crates/axnet/src/service.rs` | `StackRoundOutcome.listener_head_repairs`, `stack_round` ingress closure |
| `crates/axnet/src/stack_runner.rs` | `StackTelemetry.listener_head_repairs`, `StackSnapshot.listener_head_repairs`, Future telemetry wiring, tests `same_batch_adjacent_syns_both_establish_and_are_accepted`, `task_28_ingress_packet_count_bounds_head_repairs_without_loss`, `task_28_head_micro_step_is_exact_bounded_and_lock_free` |
| `iterations/003-.../001-replan.md` | Gate 2 approval record + this Act Response |

No other product or guest file changed; Cycle 000 artifacts (payload sources/binary, validator, spec delta) preserved.

**Deviations from Plan**

- Replacing the idle socket's registered waker necessarily changed the Task 2.3-era unit witness
  `hidden_socket_creation_arms_accept_bridge_recv_slot`; it was rewritten as
  `closing_idle_records_head_signal_and_stages_accept_wake` asserting the new staged-only contract (no direct accept
  wake before commit; signal recorded; staged drain wakes). This witnesses the replaced mechanism itself, consistent
  with the Design Decision and the Preserve list (guest/runtime markers untouched).
- The runtime budget witness drives three real loopback frames instead of synthetic device counts, so "processed
  ingress packet" is observed end-to-end rather than inferred (equivalent adjustment within the Verification clause).
  After the interim Plan Review flagged the single-poll form as vacuously satisfiable (dispatch-only first round),
  the witness was strengthened to poll until signals are consumed and to require exactly 3 repairs; test-only change,
  product code untouched.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Specification review: PASS — every Required change implemented in order (RED first), Preserve list verified intact
  (unique runner; one sweep/round + 32-token cursor; ingress budget 32; backlog 512; atomic accept/refill; staged
  guard-free wake; 14-marker source/validator guards), Forbidden list enforced by construction and by the new source
  guard (no allocation/locks in waker; no capacity loss — proven invariant; no reverse lock order — consume takes
  entry lock strictly under the caller's SocketSet guard like `reconcile`; no per-packet scans; no idle pool/backlog/
  sleep/poll/scheduler/smoltcp/reset/SMP/board changes).
- Code quality review: PASS — unstaged working-tree delta limited to the four files above; no new compiler warnings
  (the two remaining axnet warnings are the pre-existing unused `flush_*` items); lock scopes statement-local; memory
  ordering conservative (AcqRel/Release/Acquire); no dead code introduced.
- Full diff reviewed: PASS — reviewed `git diff HEAD` for both product files and the unstaged stack_runner hunks,
  separating Cycle 000 staged records from this Cycle's additions; cross-task interaction checked against the sweep,
  deferred retirement and accept paths.
- Critical findings unresolved: none.
- Important findings unresolved: none.
- Minor findings unresolved: pre-existing compiler warnings and the acknowledged qemu-diagnostics `async_rx` flake
  (Cycle 009 record) remain outside this Cycle; neither reproduced in either full-suite run here.

**Verification Evidence**

| Verification | Command / result | Conclusion |
|---|---|---|
| RED witness (pre-change) | `cargo test --manifest-path crates/axnet/Cargo.toml --lib same_batch_adjacent_syns` | FAILED at `assert_ne!(state(clients[1]), Closed)` — client A `ESTABLISHED`, client B `Closed`; exit 101 |
| Same-batch GREEN (post-change) | same command | `ok`; exit 0 (100×) |
| listen_table unit set | `cargo test --manifest-path crates/axnet/Cargo.toml --lib listen_table::` | 23 passed, exit 0 (routing/coalesce/stale/quiet included) |
| task_28 guards, ordinary + diagnostics | `cargo test --manifest-path crates/axnet/Cargo.toml --lib [--features qemu-diagnostics] task_28` | 2 passed each, exit 0 |
| Full ordinary suite | `cargo test --manifest-path crates/axnet/Cargo.toml --lib` | **326 passed / 0 failed**, exit 0 (rerun after the strengthened runtime test: 326/326 again) |
| Full diagnostics suite | `cargo test --manifest-path crates/axnet/Cargo.toml --lib --features qemu-diagnostics` | **346 passed / 0 failed**, exit 0 (rerun after the strengthened runtime test: 346/346 again) |
| Format | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | clean, exit 0 |
| OpenSpec strict | `openspec validate ms06-application-visible-async-network-stack --strict` | valid, exit 0 |
| Validator self-test | `python3 scripts/ms01-qemu-test.py --self-test` | `PASS: harness-self-test`, exit 0 |
| Payload builds | `riscv64-linux-musl-gcc -static -O2 -o ...` for `ms01_socket_baseline` and `ms01_loopback_diagnostic` | exit 0 each; fresh static RISC-V ELFs, 2026-08-26 |
| Fresh kernel image | `make ARCH=riscv64 build` | exit 0; `StarryOS_riscv64-qemu-virt.bin` timestamp 2026-08-26 12:16 |
| Whitespace | `git diff HEAD --check` | exit 0 |
| Diagnostic single (user-run) | guest `/tmp/ms01_diag single`, Runbook HTTP path | `PASS: single-loopback`, START/END markers, exit 0 |
| Diagnostic fork (user-run) | guest `/tmp/ms01_diag fork` | `PASS: fork-loopback`, START/END markers, exit 0 |
| MS01 runtime (user-run) | guest `/tmp/ms01_test; echo MS01_EXIT:$?` on image 2026-08-26 12:16 | one START, **14 unique PASS incl. `tcp-adjacent`**, zero FAIL, one END, **explicit `MS01_EXIT:0`** |

**Manual QEMU Results (Acceptance 6 / scenario S5 — CLOSED)**

Executed manually by the user on 2026-08-26 following `.claude/runbooks/qemu-network-testing.md`
with this Cycle's fresh artifacts (HTTP server `--bind 0.0.0.0`:18765; payloads served from `tests/`;
kernel image `StarryOS_riscv64-qemu-virt.bin` built 2026-08-26 12:16). Final fresh batch:

1. diagnostic `single`: `MS01_LOOPBACK_DIAGNOSTIC_START single` … `PASS: single-loopback` …
   `MS01_LOOPBACK_DIAGNOSTIC_END single` — PASS.
2. diagnostic `fork`: `MS01_LOOPBACK_DIAGNOSTIC_START fork` … `PASS: fork-loopback` …
   `MS01_LOOPBACK_DIAGNOSTIC_END fork` — PASS.
3. MS01: one `MS01_SOCKET_BASELINE_START`, 14 unique PASS (`tcp-accept`, `tcp-adjacent`,
   `tcp-512cap: accepted 512 of 512 initial connections`, `tcp-512-recovery`, `tcp-relisten`,
   `udp-bidi`, `tcp-nonblock-accept`, `udp-nonblock`, `poll-readiness`, `udp-source`,
   `bind-getsockname`, `bind-ephemeral`, `bind-conflict`, `bind-close-cleanup`), zero FAIL,
   one `MS01_SOCKET_BASELINE_END`, explicit **`MS01_EXIT:0`** — PASS. **`tcp-adjacent` established
   both adjacent clients**: the same-batch defect this Cycle repairs is confirmed fixed on the real
   VirtIO-MMIO stack; overflow terminal (`tcp-512cap`) and immediate recovery (`tcp-512-recovery`)
   remain GREEN.

All six Acceptance items are now evidenced with observed runtime output including the required
explicit exit status; scenario S5 is satisfied and Task 2.8 is closed.

**Persisted Evidence**

User explicitly requested preservation of the runtime batch ("创建证据文件保存这个证据"), meeting the
whitelist; saved as `EV-003-001-replan-01`:

- `openspec/changes/ms06-application-visible-async-network-stack/evidence/003-backlog-and-ms01-runtime-compatibility/001-replan/README.md`
- `…/001-replan/ms06-qemu-runtime-markers.md`（三次运行的决定性 marker 序列 + 显式退出码；2 文件，预算内）

Scope limit: single-hart QEMU VirtIO-MMIO software device model only; no SMP, board, DMA/cache or
performance claims.

**Experience Candidates**

None. Standard TDD path; QEMU operation is already covered by the existing Runbook (R55/R48 policy references).

**Remaining Issues**

- None blocking. Pre-existing compiler warnings (`flush_*`) and the acknowledged diagnostics-profile `async_rx` flake
  remain outside this Cycle (the flake did not reproduce in either full-suite run here).

**Commit or Diff Reference**

None; no commit was requested. Implementation remains in the modified working tree on top of HEAD `4396d264`.

## Plan Review

- Status: completed

**Review Result**

accepted

**Findings**

- The implementation matches the approved ownership and boundedness design. The fixed `PORT_NUM` ring and per-port
  bitmap make signals lossless under the one-pending-per-listener invariant; the hidden waker allocates no memory and
  takes no listener, SocketSet or Service lock. Service consumes through one O(1) site after each processed ingress
  packet, while application accept wake remains staged until committed state and released guards.
- The interim evidence gap is closed. The Act Response now records executable `--manifest-path` Cargo commands and
  the matching fresh QEMU `MS01_EXIT:0`; diagnostic single/fork and MS01 emit their complete START/PASS/END sets,
  including `tcp-adjacent`, with no FAIL.
- The interim test-quality concern is also closed. The strengthened runtime-bound witness advances beyond the
  dispatch-only first round until three frames enter ingress, then requires exactly three repairs and thirty retained
  signals. It can no longer pass vacuously with zero repairs.
- Reviewer inspection found no Critical or Important defect in the signal queue, lock order, stale-unlisten path,
  staged wake or interaction with the independent 32-token listener sweep. The accepted backlog 512, overflow and
  recovery semantics remain intact.
- One reviewer attempt ran the two leak-heavy full profiles concurrently and the diagnostics process received an
  external SIGKILL near completion. The same diagnostics command then passed alone at 346/346, with no prior
  assertion failure; this is classified as reviewer execution/memory-pressure noise, not a product finding.
- The two pre-existing unused `flush_*` warnings remain Minor and outside Task 2.8. They do not affect acceptance.

**Deviation Classification**

The interim `ACT-DEVIATION` is resolved by the corrected command context and explicit runtime exit. The user-requested
compact Evidence is permitted by the Evidence whitelist and remains within its two-file budget. No unresolved product,
plan or evidence deviation remains.

**Acceptance Gaps**

None. Acceptance 1–6 are satisfied.

**Convergence**

closed: the parent Cycle's same-batch refusal and this Review's evidence/test-quality gaps are all closed without
widening Task 2.8.

**Evidence**

- `cargo test --manifest-path crates/axnet/Cargo.toml --lib same_batch_adjacent_syns_both_establish_and_are_accepted`:
  1 passed, exit 0.
- Focused listener and strengthened Task 2.8 commands: 23 passed and 2 passed in each applicable profile, exit 0.
- Full ordinary and isolated qemu-diagnostics suites: 326/326 and 346/346, exit 0.
- `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`, validator self-test, strict OpenSpec and
  `git diff HEAD --check`: exit 0.
- Manual QEMU: diagnostic single/fork PASS; MS01 one START, 14 unique PASS, zero FAIL, one END and `MS01_EXIT:0`.
- User-requested compact evidence:
  `evidence/003-backlog-and-ms01-runtime-compatibility/001-replan/{README.md,ms06-qemu-runtime-markers.md}`.

**Follow-up Decision**

Accept Iteration 003 and close Task 2.8. No rework Cycle is needed. Expand the dependency-ready final Iteration 004
for Tasks 3.1–3.4; its Gate 2 remains blocked on explicit user approval and it must not invoke Act automatically.

**Iteration Plan Update**

Iteration 003 is accepted. Iteration 004 is expanded as
`../004-terminal-readiness-and-qemu-acceptance/000-initial.md`; roadmap scope and requirement allocation are unchanged.

**Next Cycle**

None for Iteration 003.

**Next Iteration**

`004-terminal-readiness-and-qemu-acceptance/000-initial.md`（Gate 2 BLOCKED，等待用户批准；不自动调用 Act）。
