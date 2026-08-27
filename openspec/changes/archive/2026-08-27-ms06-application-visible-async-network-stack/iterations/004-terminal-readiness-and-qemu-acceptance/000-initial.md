# Iteration 004 / Cycle 000: terminal readiness and single-hart QEMU acceptance

## Plan Context

- Status: ready
- Approval: pending
- Iteration: 004-terminal-readiness-and-qemu-acceptance
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 3.1–3.4
- Depends on: Iteration 003 accepted
- Stable baseline: normal close, EOF, half-close, connect/listener errors and stable data-plane faults expose matching
  readiness and I/O results; single-hart QEMU proves caller-independent TCP/UDP/listener progress through
  poll/select/epoll.
- Verification boundary: deterministic terminal/fault publication tests and probe seam tests precede the complete
  automatic Gate and the final user-run QEMU batch.
- Diagnostic boundary: terminal snapshot/error mapping, fault registry publication, syscall waiter delivery, guest
  probe decisions, QEMU environment or scheduling chain.
- Deferred tasks: None within MS06

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: Tasks 3.1–3.4 are not implemented; final terminal and application-visible QEMU requirements are
  not yet evidenced.
- Repair items: None
- Inherited scope: R1–R7; D1–D11; unique runner; per-socket readiness registry; bounded stack stages; accepted
  Iterations 000–003; manual-QEMU Runbook policy
- Excluded scope: Linux-compatible destructive `SO_ERROR` consumption, reset/cancellation redesign, scheduler
  changes, SMP, multiqueue, multiple NICs, PCI/DWMAC, physical boards, DMA/cache qualification, performance,
  automatic guest-shell control, global documentation maintenance, archive and commits

**Objective**

Close MS06 by making every terminal transition observable only after its stable state or error is committed, then
prove the full application path with deterministic host/model tests, complete regression Gates and a manually run
single-hart QEMU witness whose scenarios are independently bounded and marked.

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 normal terminal | TCP/UDP reaches EOF, half-close or full close | poll, then perform matching I/O | `IN/RDHUP/HUP` matches data, EOF or the existing close error; no device `ERR` | false `OUT`, normal close reported as fatal, or poll/I/O contradiction outside a documented race |
| S2 connection/listener error | nonblocking connect fails or one hidden listener slot resets | wait through poll/select/epoll, then check completion/accept | connect=`OUT|ERR` with stable error; listener=`IN|ERR` until the reset item is consumed once | missing wake, unstable error, permanent listener `ERR`, or reset delivered twice |
| S3 data-plane fatal | queue owner publishes one fatal `DevError` | observe current and later public sockets and retry network I/O | code commits before wake; all affected bridges report `ERR`; I/O returns one stable mapped category | `WouldBlock`/Full fallback, wake-before-code, missed late socket, duplicate transition or polling fallback |
| S4 application witness | fresh single-hart QEMU image and probe are available | run bounded TCP/UDP/listener, timer/traffic and poll/select/epoll cases | each case emits exactly one PASS/FAIL terminal marker without internal stack polling | timeout, partial markers, active polling, stale artifact or ambiguous exit |
| S5 regression closure | all automatic Gates are GREEN | run MS06 plus affected MS01/MS04/MS05 guest cases | complete marker sets, explicit exit codes and no panic/hang | any compile/test/review failure or incomplete manual batch |

**Current Baseline**

- Branch `net-k3`; HEAD `4396d264787527ed7f158abf9f51f5e8f0cb706a`; the accepted MS06 implementation
  remains in the working tree.
- Iteration 003 Cycle `001-replan` is accepted: ordinary 326/326 and diagnostics 346/346 pass; manual diagnostic
  single/fork and MS01 14/14 including `tcp-adjacent` pass with `MS01_EXIT:0`.
- `ReadinessBridge` currently fans out read/write/terminal wakes but stores no terminal error. The wrapper registry
  has no stable global fault and cannot initialize a socket added after publication.
- `RxRxFuture` observes the concrete `DevError`, but the fatal lifecycle path currently publishes only lifecycle and
  telemetry state. `StackRoundOutcome` reduces another fault path to a boolean. Flush retains a stable numeric error,
  so error identity already has a tested precedent but is not shared with public sockets.
- TCP connect failure currently reports ordinary completion without a socket-local stable error. Listener reset is
  returned by `accept`, but its readiness snapshot cannot distinguish Reset from no ready item. UDP normal close
  already reports `HUP` and must remain non-fatal.
- `GeneralOptions::Error` does not implement Linux `SO_ERROR` consumption. This Cycle may expose the stable value
  without clearing it; adding destructive read/reset semantics requires Plan re-entry.
- The kernel has poll, select and epoll syscall paths. Existing guest payloads establish static-musl build, fork and
  fixed-deadline patterns. No MS06 guest probe or validator exists yet.

**Relevant Code**

| File / Symbol | Current responsibility | Planned use |
|---|---|---|
| `crates/axnet/src/readiness.rs::ReadinessBridge` | per-socket read/write/terminal PollSets | add stable socket terminal code and commit-before-wake helpers |
| `crates/axnet/src/wrapper.rs::SocketSetWrapper` | public handle→bridge registry | own first global data-plane fault, initialize late sockets, snapshot then wake |
| `crates/axnet/src/async_rx.rs` and `service.rs` | queue fatal/lifecycle and bounded stack outcome | preserve the concrete `DevError` into the single public fault publisher |
| `crates/axnet/src/general.rs` | blocking/nonblocking I/O options and socket error option | read terminal state before retry/register; expose non-consuming stable error |
| `crates/axnet/src/tcp.rs` and `udp.rs` | readiness snapshots and network I/O | commit connect error; apply terminal precedence; preserve normal EOF/HUP behavior |
| `crates/axnet/src/listen_table.rs` | Ready/Reset accept queue | expose one-shot Reset as `IN|ERR` without turning it into listener-global terminal state |
| `tests/ms06_stack_readiness_probe.c` and seam tests | absent application witness | add deterministic marker/deadline decisions and guest socket scenarios |
| `scripts/ms06-qemu-validate.py` | new pure-output validator | validate saved/manual output only; never start QEMU or drive its shell |

**Critical Paths**

```text
queue owner DevError -> first-code CAS -> lifecycle Faulted -> registry global code
  -> snapshot Arc<ReadinessBridge> under registry lock -> unlock
  -> commit the same code to each bridge -> wake terminal/read/write sets
  -> current and later public I/O observes stable mapped AxError

connect/listener transition -> commit socket-local error or Reset queue item
  -> release SocketSet/ListenTable guards -> wake bridge
  -> poll/select/epoll reports ERR with OUT/IN -> completion or accept consumes the matching outcome
```

**Design Decisions**

1. `SocketSetWrapper` owns one first-wins global data-plane fault code. Publication preserves the concrete
   `DevError`; it commits lifecycle/global state before taking a registry snapshot, releases every guard before wake,
   and is idempotent. A bridge installed after publication inherits the same code before it becomes observable.
2. `ReadinessBridge` owns a first-wins socket-local terminal error for connection-level failures. Global data-plane
   fatal takes precedence for subsequent affected network operations, so sockets do not expose different fallback
   categories after one device failure. Normal EOF, half-close and UDP close remain readiness state, not errors.
3. Reuse one stable DevError encoding and add one explicit terminal mapping:
   `AlreadyExists→AlreadyExists`, `BadState→BadState`, `InvalidParam→InvalidInput`, `Io→Io`,
   `NoMemory→NoMemory`, `ResourceBusy→ResourceBusy`, `Unsupported→Unsupported`; fatal `Again` maps to `Io`, not
   `WouldBlock`, because a committed terminal fault is not retryable backpressure. Every variant is table-tested.
4. A listener Reset remains a queued, consumable accept outcome. Readiness inspects the queue head and reports
   `IN|ERR`; successful or reset accept removes exactly that item, so one reset cannot poison the listener forever.
5. `GeneralOptions::Error` may return the saved numeric socket error without clearing it. Full Linux `SO_ERROR`
   consumption, reconnect reset and cancellation semantics remain excluded; if compatibility requires them, stop.
6. The MS06 script is a pure marker validator with self-tests. QEMU launch and every guest-shell command remain
   manual under `.claude/runbooks/qemu-network-testing.md`; no pipe, pexpect or subprocess shell driver is permitted.

**Rejected Alternatives**

- Do not infer a device fault from lifecycle alone or remap it on each I/O; that loses the stable category.
- Do not wake while holding Service, SocketSet, registry, listener or readiness locks.
- Do not store listener Reset as a permanent socket terminal error; it belongs to one pending accept result.
- Do not map a committed fatal to `WouldBlock`, queue Full or a timer fallback.
- Do not replace the exact guest 64/65 boundary with lower practical concurrency. Host/model tests exhaust adversarial
  replacement interleavings, while QEMU must additionally create 64 then 65 distinct waiter tasks/processes on one
  socket and prove the replaced waiter rechecks/re-registers instead of disappearing. If the guest task primitive or
  resource limit cannot support that witness, stop and return to Plan.
- Do not automate the QEMU guest shell or reuse historical runtime output as current evidence.

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Planned change |
|---|---|---|---|
| 3.1 | R6, S1–S3 | readiness/wrapper | socket-local terminal state, global first-fault inheritance, snapshot-before-wake |
| 3.1 | R3/R4/R6, S3 | async_rx/service/flush mapping | carry exact DevError through all fatal paths and centralize stable encoding/mapping |
| 3.1 | R6, S1/S2 | general/TCP/UDP/listener | terminal-first I/O, connect `OUT|ERR`, Reset `IN|ERR`, normal EOF/HUP preservation |
| 3.2 | R5–R7, S4 | new C probe, host seam and validator | fixed deadlines, unique markers, no internal poll, poll/select/epoll application cases |
| 3.3 | R1–R7, S5 | repository automatic Gates | complete tests/builds/regressions/source assertions/diff review |
| 3.4 | R7, S4/S5 | manual single-hart QEMU batch | MS06 and affected MS01/MS04/MS05 commands, markers and exits |

## Task Contracts

### 3.1: publish terminal readiness and stable errors before wake

- Requirement/Scenario: R3/R4/R6; D5/D6/D8/D9; S1–S3.
- Depends on: accepted per-socket bridge/registry and queue lifecycle from Iterations 000–003.
- Targets: `readiness.rs`, `wrapper.rs`, `async_rx.rs`, `service.rs`, shared DevError encoding, `general.rs`,
  `tcp.rs`, `udp.rs`, `listen_table.rs` and focused tests/source guards.
- Current behavior: bridges have no error payload; late sockets miss a prior fatal; concrete queue errors are reduced
  to lifecycle/boolean state; connect/listener readiness does not preserve the matching error outcome.
- Required behavior: error identity commits once before all wakes; current and late sockets report `ERR`; all
  subsequent affected I/O returns the same mapped category. Connection failure and listener Reset expose the exact
  completion events, while normal close remains EOF/RDHUP/HUP.
- Required changes: add RED terminal matrix and publication-order tests; centralize the stable DevError code; carry
  the concrete error from every fatal path; add global and per-bridge state; implement snapshot/unlock/wake and late
  inheritance; make connect completion, accept and network I/O read the same state; add source/order guards.
- Preserve: unique runner and Faulted no-fallback; check-register-recheck; per-socket PollSet capacity; normal TCP
  buffered data/EOF/half-close and UDP datagram atomicity; listener unique accept; lock order and staged wakes.
- Forbidden: wake-before-code; lock-held wake; best-effort registry traversal; late-socket gap; `WouldBlock`/Full
  terminal mapping; permanent listener poisoning; inline stack polling; complete SO_ERROR consume/reset semantics.
- Test witness: normal-state matrix; connect failure and Reset poll→I/O; zero/1/2/64/65 waiter fatal delivery;
  publish-before-register/during-register/duplicate-publish/late-add races; a wake callback that immediately observes
  committed code; all DevError variants and 100× deterministic publication ordering.
- GREEN condition: all terminal tests pass in ordinary and qemu-diagnostics profiles, MS05 fatal/flush regressions
  retain their exact error, and no source path can publish a terminal wake before state.
- Stop when: error identity cannot survive a stack round; correctness needs registry lock across callbacks, full
  SO_ERROR consumption, reconnect/reset/cancellation redesign, a second protocol owner or periodic polling.

### 3.2: build a bounded application-visible guest witness

- Requirement/Scenario: R5–R7; D6/D8/D10; S4.
- Depends on: Task 3.1 GREEN host/model baseline.
- Targets: new `tests/ms06_stack_readiness_probe.c`, its C seam test, source assertions and new
  `scripts/ms06-qemu-validate.py` pure validator.
- Current behavior: existing payloads cover MS01 compatibility and MS04/MS05 data plane, but no payload joins
  caller-independent TCP/UDP/listener progress with poll/select/epoll and terminal readiness.
- Required behavior: each independent guest mode has a monotonic fixed deadline, one START, unique PASS/FAIL cases,
  one END and explicit exit. The source contains no axnet-internal poll and no unbounded wait.
- Required changes: first make seam/validator tests RED for missing, duplicate, reordered, partial, timed-out and
  exit-inconsistent markers; add TCP timer/traffic, UDP, listener, nonblocking connect, close/error, poll, select,
  epoll, multiwaiter and overflow decision modes; reuse static-musl and fork patterns already supported by MS01.
- Marker contract: one `MS06_STACK_READINESS_START`, then exactly one `MS06 PASS case=<name>` for each of
  `tcp-timer`, `udp-progress`, `listener`, `nonblock-connect-error`, `poll-multiwaiter`, `select-multiwaiter`,
  `epoll-multiwaiter`, `waiter-64`, `waiter-65-reregister`, `quiet`, `continuous-traffic` and `close-error`, followed
  by one `MS06_STACK_READINESS_END`; any `MS06 FAIL`, duplicate/missing case or nonzero exit fails the run.
- Preserve: host tests remain the exhaustive interleaving proof for 64/65 PollSet replacement; the guest additionally
  proves the exact capacity boundary through distinct waiters and an application-visible re-registration outcome.
- Forbidden: direct/internal `poll_interfaces`, sleeps as correctness, infinite wait, benchmark expansion, SMP,
  shell automation, QEMU subprocess launch, stale markers or one aggregate PASS masking a partial scenario.
- Test witness: C decision/seam test and validator self-test reject every incomplete/ambiguous output fixture; source
  guard rejects internal poll and unbounded waits; the 64/65 modes verify distinct waiter identities, replacement wake
  and eventual completion after re-registration; static RISC-V compilation succeeds.
- GREEN condition: syntax, seam, validator, source and cross-build tests pass; a fresh QEMU image can carry the probe.
- Stop when: the guest ABI lacks a required syscall or practical concurrency primitive, fixed deadlines cannot
  distinguish state, or the witness would need QEMU automation, scheduler changes or I16 performance machinery.

### 3.3: close automatic integration and regression Gates

- Requirement/Scenario: R1–R7; D10; S5.
- Depends on: Tasks 3.1 and 3.2 GREEN.
- Targets: all affected axnet, smoltcp, kernel, probe and OpenSpec surfaces.
- Required behavior: no runtime attempt begins until product, compatibility, ownership and review Gates are GREEN.
- Required changes: run focused tests first, then ordinary/qemu-diagnostics full suites sequentially; run repeated
  lost-wakeup/lock-order tests, MS01 self-tests, MS04 harness/probe, MS05 data-plane/Full/flush tests, MS06 seam and
  validator, QEMU kernel and supported root D1 checks, format/source assertions, strict OpenSpec and full diff review.
- Preserve: use `--manifest-path crates/axnet/Cargo.toml` for axnet; use the RISC-V target for root D1; do not treat
  the known invalid kernel-only D1 feature combination as a product Gate.
- Forbidden: parallel full suites when their leak-heavy test processes cause memory pressure; historical artifacts;
  ignored failures; global doc/archive writes; unresolved Critical or Important findings.
- GREEN condition: every command and final exit is recorded in Act Response, fresh payload/image artifacts exist,
  and the complete diff has no unresolved Critical/Important finding.
- Stop when: any compile, assertion, ownership, timeout, source or review Gate fails; classify genuine environment
  limits separately but never convert an ambiguous/product failure to `ENV-BLOCKED`.

### 3.4: execute final manual single-hart QEMU acceptance

- Requirement/Scenario: R7; D10; S4/S5.
- Depends on: Task 3.3 GREEN and fresh artifacts from the same working tree.
- Targets: user-run RISC-V `virt`, `-smp 1`, one VirtIO-MMIO NIC; MS06 probe and affected MS01/MS04/MS05 cases.
- Required behavior: runner device/software/timer progress, Active quiet, bounded continuous traffic, TCP/UDP,
  listener, nonblocking, poll/select/epoll, multiwaiter/overflow and close/error are individually decided; regression
  marker sets and explicit exits are complete.
- Required changes: Act prints the exact Runbook command batch and stops for the user to run it; then records the
  supplied decisive output, environment, revision and exits and performs one final full-diff review.
- Preserve: QEMU is manual; evidence is concise; results apply only to single-hart QEMU VirtIO-MMIO.
- Forbidden: automatic guest input, inferred exit status, partial success, timeout-as-pass, history-as-current,
  SMP/board/performance claims, archive or docs-maintainer invocation.
- GREEN condition: every required mode has one START, all unique PASS markers, zero FAIL, one END and exit 0; no
  panic/hang occurs; final diff review remains clean.
- Stop when: any marker/exit is missing, the user interrupts, an environment issue prevents attribution, or a product
  case fails. Report incomplete/blocked or return to Plan; never promote partial evidence.

**Invariants**

- No Service, SocketSet, registry, listener or readiness guard crosses wake, await, Pending or yield.
- A terminal error is immutable once observable; a wake is only a hint to recheck committed state.
- Device fatal is visible to every affected public socket, including handles installed after publication.
- Normal close and transient listener Reset are not rewritten as device-wide fault.
- The resident runner remains the sole smoltcp progress owner; Faulted never activates polling fallback.
- Host/model establishes adversarial interleavings; QEMU establishes the application-visible scheduling chain only.

**Non-goals**

- Full Linux SO_ERROR destructive consumption, reconnect/reset/cancellation redesign or new ABI.
- Scheduler, smoltcp wire behavior, queue ownership, slot capacity, listener backlog or PollSet capacity changes.
- Automated QEMU shell control, SMP, multiqueue, multi-NIC, PCI/DWMAC, physical boards, DMA/cache and performance.
- Global tasks/SNAPSHOT/M-D-K-R-I updates, Evidence directory by default, archive and commit.

**Traceability Matrix**

| Requirement / Acceptance | Scenario | Design | Task | Witness | Status |
|---|---|---|---|---|---|
| R3 bounded progress/fault stop | S3/S4 | D3/D4/D8 | 3.1,3.2 | fatal no-fallback, fixed deadlines, continuous/quiet markers | Covered |
| R4 ownership and publication order | S2/S3 | D5/D9 | 3.1 | state-before-wake, registry snapshot, 100× races | Covered |
| R5 multiwaiter bridge | S2–S4 | D6 | 3.1,3.2 | 1/2/64/65 host tests plus guest poll/select/epoll progress | Covered |
| R6 terminal readiness | S1–S3 | D6/D8/D9 | 3.1 | EOF/HUP, connect `OUT|ERR`, Reset `IN|ERR`, stable fatal matrix | Covered |
| R7 validation boundary | S4/S5 | D10 | 3.2–3.4 | automatic Gates and manual single-hart marker/exit batch | Covered |
| MS01/MS04/MS05 compatibility | S5 | D10/D11 | 3.3,3.4 | socket, snapshot/quiet/nudge/burst, data-plane/Full/flush regressions | Covered |

No Missing or Simplified requirement remains. User approval is the only Gate 2 blocker.

**Acceptance**

1. TCP data, EOF, half-close and full close and UDP data/close expose the specified `IN/OUT/RDHUP/HUP` snapshots;
   the next nonblocking I/O matches or records an explicit concurrent winner/state race, and normal close has no
   device `ERR`.
2. Failed nonblocking connect reports `OUT|ERR` and a stable completion error. Listener Reset reports `IN|ERR`,
   returns `ConnectionReset` once and clears when that queued item is consumed.
3. Every queue fatal path preserves one concrete `DevError`, commits the stable global/socket code before wake,
   wakes all current bridges, initializes later public sockets with the same fault and makes send/recv/connect/accept
   return the mapped category rather than `WouldBlock`, Full or fallback.
4. Terminal host/model tests cover zero/1/2/64/65 waiters, register races, duplicate publication, late socket,
   wake-observes-code, all error mappings and 100× deterministic ordering in both feature profiles.
5. The MS06 probe, C seam and pure validator implement fixed-deadline TCP/UDP/listener, nonblocking,
   poll/select/epoll, timer/traffic, quiet, multiwaiter/overflow and close/error decisions without internal polling or
   automated QEMU control.
6. All automatic Task 3.3 Gates pass sequentially, fresh artifacts are built, and full diff review has no unresolved
   Critical or Important finding.
7. The user-run single-hart VirtIO-MMIO QEMU batch reports complete MS06 and affected MS01/MS04/MS05 marker sets,
   zero FAIL/panic/hang and explicit exit 0 for every required command.

**Verification**

- TDD order: terminal RED matrix and publication races → Task 3.1 GREEN → probe/validator negative fixtures → Task
  3.2 GREEN → complete automatic Gate → fresh manual QEMU batch.
- Representative automatic commands include:
  - `cargo test --manifest-path crates/axnet/Cargo.toml --lib`
  - `cargo test --manifest-path crates/axnet/Cargo.toml --lib --features qemu-diagnostics`
  - focused terminal/listener/fatal tests repeated 100× in each profile
  - MS04 host harness/probe and MS05 data-plane/Full/flush seam suites
  - `python3 scripts/ms01-qemu-test.py --self-test` and `python3 scripts/ms06-qemu-validate.py --self-test`
  - host C syntax/seam builds and `riscv64-linux-musl-gcc -static -O2` for the MS06 payload
  - `cargo check --locked --offline -p starry-kernel --features qemu`
  - `cargo check --locked --offline --target riscv64gc-unknown-none-elf --features lichee-d1`
  - `make ARCH=riscv64 build`, format/source guards, `openspec validate ... --strict`, `git diff --check` and full diff review
- Run the two full axnet profiles sequentially. A parallel reviewer/test process killed by host memory pressure is an
  execution artifact only after the identical command passes alone; an assertion or product crash remains failure.
- After automatic GREEN, Act must provide the complete manual Runbook batch. User output must include environment,
  revision, decisive markers and explicit exits; missing evidence is incomplete, not inferred PASS.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | current bridge, registry, fatal, TCP/UDP/listener and syscall paths inspected |
| Design | PASS | error ownership, precedence, late-socket inheritance, listener Reset and QEMU policy are explicit |
| Iteration Plan | PASS | Tasks 3.1–3.4 form one terminal implementation-to-runtime result |
| Cycle Scope | PASS | terminal behavior, its probe and final MS06 Gates only |
| Task Contracts | PASS | RED/GREEN, preserve/forbid, exact witnesses and stop boundaries are defined |
| Traceability | PASS | R3–R7 and affected MS01/MS04/MS05 paths map to host and QEMU evidence |
| Verification | PASS | deterministic automatic Gates precede the manual single-hart batch |
| User Approval | BLOCKED | awaiting explicit approval; this plan does not authorize Act |

Gate 2 remains BLOCKED until explicit user approval. Do not invoke `openspec-act` automatically after approval unless
the user separately requests implementation.

**Persisted Evidence**

- Mode: none

Act Response may retain decisive commands, marker summaries and exit codes. Create compact Evidence only if the user
explicitly requests it or a later Plan decision establishes that necessary attribution cannot be reproduced cheaply.

**Risks and Notes**

- The global fatal state must be installed early enough that a concurrent `add_public` cannot escape without either
  inheriting it or appearing in the publication snapshot. Tests must pin this linearization point.
- `StackRoundOutcome` must not collapse a concrete terminal error to a boolean before the public publisher sees it.
- Listener Reset readiness must follow queue-head/consumption semantics; a permanent bit would create a busy wake.
- If a required guest syscall or concurrency pattern is absent, return to Plan rather than weakening the marker.
- Manual QEMU is the final batch and cannot repair an automatic Gate failure.

## Act Response

- Status: blocked

### Completed

Task 3.1 only. The user directed, mid-execution (2026-08-26), that this Act invocation
close Task 3.1 and stop; Tasks 3.2–3.4 remain pending inside this Cycle.

**Actual changes**

1. Single stable DevError encoding centralized in `readiness.rs`
   (`dev_error_code` / `dev_error_from_code`, codes 1–8) plus one explicit terminal
   mapping (`terminal_ax_error`): fatal `Again→Io` (never `WouldBlock`),
   `InvalidParam→InvalidInput`, others identity; `TERMINAL_CONNECT_REFUSED=9` is the
   socket-local connect-failure category. `flush.rs::error_code/error_from_code` and
   `async_rx.rs::rx_error_code` now delegate to it (no duplicate tables).
2. `ReadinessBridge` owns a first-wins socket-local terminal code
   (`commit_terminal`) and a commit-before-wake fan-out helper
   (`commit_terminal_and_wake`: commit → wake IN|OUT|RDHUP|HUP|ERR).
3. `SocketSetWrapper` owns the first-wins global data-plane fault
   (`publish_global_fault[_code]`): first-wins CAS is the linearization point,
   then registry snapshot under the lock, lock released, then per-bridge
   commit+wake. `add_public` / `install_readiness` inherit the published fault
   while holding the registry lock (no late-socket gap).
4. Concrete errors reach the single publisher uncollapsed: `RxRxFuture`
   gained an injected `fault_sink` (production: global registry; tests: leaked
   local wrappers for hermeticity); both fatal paths
   (`RoundOutcome::Fault(err)` and arm `WaitDecision::Fault(err)`) call
   `publish_fatal(&err)` = lifecycle CAS → sink publish → internal wakes.
   `RouterDispatchOutcome.faulted:bool` became `fault_code:u64`;
   `StackRoundOutcome.faulted:bool` became `fault_code:u64`; the stack runner
   publishes the concrete code after `round()` returns (guards already
   dropped). MS05 flush ledger semantics untouched.
5. Terminal-first I/O surfaces: TCP connect completion commits
   `CONNECT_REFUSED` + records SO_ERROR before reporting `OUT`; `poll_connect`
   reports `OUT|ERR` on failure; stream/listener/UDP poll and register overlay
   `ERR` whenever a terminal state exists; `send`/`recv`/`accept`/UDP
   send/recv closures check `observe_terminal_error()` first and return the
   mapped stable category. Listener Reset stays a queued consumable outcome:
   readiness reports `IN|ERR` from the queue head via
   `ListenTable::accept_head_is_reset`; consumption clears it; no permanent
   listener poisoning. Normal EOF/HUP/UDP-close never gain device `ERR`.
6. `GeneralOptions` saves the Linux errno of the stable socket error;
   `GetSocketOption::Error` exposes it WITHOUT consuming (D5 scope boundary
   respected).

**Files/symbols**: `readiness.rs` (encoding/mapping/effective_terminal_code,
bridge terminal state), `wrapper.rs` (global fault + inheritance),
`general.rs` (saved_error, non-consuming Error), `flush.rs`,
`async_rx.rs` (`rx_error_code` delegate, `fault_sink`, `publish_fatal(&err)`),
`router.rs` (`RouterDispatchOutcome.fault_code`), `service.rs`
(`StackRoundOutcome.fault_code`), `stack_runner.rs` (publisher consumer),
`tcp.rs`, `udp.rs`, `listen_table.rs` (`accept_head_is_reset`, test seam
`test_push_reset_slot`). No product behavior outside the contract touched.

**Test witness (RED→GREEN)**: 22 new tests — encoding roundtrip table,
mapping table (incl. Again→Io), first-wins commit, wake-callback observes
committed code, source guard on commit-before-wake ordering,
global-precedence table, publication commits before waking waiters,
duplicate-publish idempotence, late `add_public`/`install_readiness`
inheritance, 100-cycle deterministic publication ordering, threaded
add-vs-publish interleaving pin, connect failure commits before `OUT|ERR`,
listener Reset `IN|ERR` consumed once, guard-mapping witnesses for TCP/UDP,
UDP ERR overlay, normal-close-no-ERR regressions (TCP+UDP), stack-round
RX/TX fault codes surfaced exactly (strengthened to exact `DevError::Io`),
receive/arm fault publishes concrete code to injected sink.

**Deviations from Plan**

- D1 (environment): cold rebuild exposed that `percpu` (via axtask) cannot
  link into PIE host-test binaries (`R_X86_64_32S` under rust-lld). Resolved
  by running axnet host suites with
  `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh"`, a wrapper that appends
  `-no-pie` to executable links and passes shared-object links through
  (distro gcc defaults to PIE; stripping `-pie` alone was insufficient).
  Wrapper content (recreate as needed):
  ```bash
  #!/usr/bin/env bash
  shared=false
  for a in "$@"; do [ "$a" = "-shared" ] && { shared=true; break; }; done
  if [ "$shared" = true ]; then exec cc "$@"; else exec cc "$@" -no-pie; fi
  ```
  Kernel/D1/RISC-V builds are unaffected (root workspace target dir).
- D2 (test isolation): pre-existing `leaked_future*` fixtures would have
  published faults into the shared global registry once publication went
  live; they now default to fresh local sinks. Tests needing the sink use
  `leaked_future_with_sink`. Production spawn path keeps the global registry.
- D3 (witness shape): public `send`/`recv` e2e calls require an axtask task
  context (`block_on` panics without one in host unit tests). The mapped-
  category-through-I/O witness is carried by the guard-mapping tests plus
  the connect-completion path; real application-level delivery is witnessed
  later by Tasks 3.2–3.4 exactly as planned. Source-level coverage of all
  four I/O closures is present.

**Self-Review**: spec compliance re-checked against Contract 3.1 line by line
(required behavior, preserve list, forbidden list) — no unresolved Critical
or Important findings. Minor findings fixed during execution: stale
`publish_fatal()` source-guard strings updated to the `(&err)` call shape;
unused test helpers/imports removed. Remaining Minor: none open.

**Verification (Gate 5)**

| 验证项 | 命令 | 输出摘录 | 结论 |
|---|---|---|---|
| ordinary full suite | `RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --lib` | `test result: ok. 348 passed; 0 failed` | PASS |
| diagnostics full suite | same + `--features qemu-diagnostics` | `test result: ok. 368 passed; 0 failed` | PASS |

Both include the MS01 socket-baseline self-tests, MS04/MS05 harness seams,
MS05 fatal/flush exact-error regressions and the 100× deterministic
publication-ordering cycles. Exit code 0 for both runs.

**Persisted Evidence**

None required (mode `none`); decisive outputs retained above.

**Experience Candidates**

- Candidate (Runbook): "axnet host unit suites need the non-PIE link wrapper
  after any cold rebuild" — verified end-to-end twice today (both profiles),
  repeatable, high-friction if lost. Evidence: Deviations D1 and the
  verification table above. Not persisted; Recorder decides.

**Unresolved / remaining in this Cycle**

Tasks 3.2, 3.3, 3.4 — not started.

**Blocker Handoff**

- Found at: Cycle scope execution, after Task 3.1 Gate 4/5.
- Plan expectation vs actual: Plan expects 3.1–3.4 in one Cycle; the user
  explicitly redirected mid-Cycle to close 3.1 only ("先把3.1做完就行").
  This is a user authorization boundary, not a technical failure; no product
  work is blocked.
- Done: Task 3.1 fully GREEN in both profiles with evidence above; change
  `tasks.md` marks 3.1 complete.
- Partial: none. Not started: 3.2 (guest probe + validator), 3.3 (automatic
  Gates incl. focused 100× reruns, kernel/D1 checks, fmt, OpenSpec strict,
  full diff review), 3.4 (manual single-hart QEMU batch).
- Workspace: clean except the 11 axnet files of Task 3.1 plus unrelated
  user edits to `CLAUDE.md` and `.claude/docs/templates/change-cycle.md`
  (present before Act started; untouched).
- Gates passed: Gate 3 (per-task witnesses), Gate 4 (spec then quality),
  Gate 5 (table above). Gate 6 not triggered (the linker issue was solved,
  not bypassed).
- Recovery condition: user resumes implementation (invoke `openspec-act`
  again; status returns `pending` via Blocker Resolution, then continue at
  Task 3.2), or redirects scope via `openspec-plan` (e.g., split Iteration
  004 so 3.2–3.4 form their own Cycle).

## Plan Review

- Review Result: replan-required

**Review Result**

replan-required

**Findings**

1. Important：`UdpSocket::recv`只在进入函数时读取terminal；`poll_io`重试闭包没有重复读取。blocking recv在注册IN waiter后遇到global fatal时会被首次wake，但重试仍可返回`WouldBlock`并再次Pending，违反stable fault后的I/O终态。
2. Important：Plan把bridge terminal同时当作socket-local first-wins状态和global fault副本。已有local connect error时，`commit_terminal_and_wake(global)`提交失败并跳过wake；实际I/O又从wrapper global取优先值，形成两个未闭合的事实源。
3. Important：smoltcp `DirectionNotify`可在`poll_connect`提交local error前发出recheck wake。Cycle没有区分状态变化hint与terminal publication wake，现有测试只证明`poll_connect`返回`OUT|ERR`前已提交错误。
4. Important：Task 3.1要求global fatal覆盖0/1/2/64/65 waiter与真实blocking I/O。现有64/65测试只覆盖普通read transition，global fatal只覆盖单waiter，TCP/UDP I/O测试主要直接调用helper；Task 3.1不能按原Contract判定GREEN。
5. Minor：Task 3.1新增数个unused imports；不单独阻塞Acceptance，但与Act Response的`Remaining Minor: none`不一致。

**Deviation Classification**

PLAN-INVALID；ACT-DEVIATION

**Acceptance Gaps**

- global与socket-local terminal的独立所有权、优先级和wake规则未闭合。
- UDP blocking recv及其他公共I/O缺少“入口检查+每次poll_io重试检查”的一致terminal路径。
- connect transition hint、local error提交和application-visible `OUT|ERR`的线性化点未定义。
- global fatal的0/1/2/64/65 fan-out、local-before-global、fault-during-wait与真实I/O见证缺失。
- Tasks 3.2-3.4未开始；按用户要求不得继续留在同一过重Iteration。

**Convergence**

reduced。Task 3.1已建立共享DevError编码、global publisher、listener Reset readiness和大部分terminal映射，两profile全量测试通过；上述Acceptance仍未闭合。

**Evidence**

- 代码：`crates/axnet/src/udp.rs::recv`、`tcp.rs::poll_connect`、`readiness.rs::DirectionNotify/ReadinessBridge`、`wrapper.rs::publish_global_fault_code`。
- 规范：`specs/qemu-application-visible-async-network-stack/spec.md`的“稳定数据面 fault”和`specs/network-stack-baseline/spec.md`的“稳定网络 fault”。
- 新鲜验证：`RUSTFLAGS="-C linker=/tmp/opencode/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --lib`，348 passed，exit 0；同命令增加`--features qemu-diagnostics`，368 passed，exit 0；`git diff --cached --check`，exit 0。
- Persisted Evidence模式为`none`；没有Evidence目录不构成finding。

**Follow-up Decision**

原Cycle同时承载terminal语义、guest witness、自动资格和人工QEMU四个故障域，且terminal所有权契约需要修订。有限当前Cycle修复不足以约束后续Act；更新未完成Iteration Map并创建同目录`001-replan.md`。用户于2026-08-26认可审计结果并授权该replan。

**Iteration Plan Update**

- Iteration 004只闭合Tasks 3.1-3.2的terminal ownership、I/O recheck和host/model witness。
- Iteration 005执行Tasks 4.1-4.3，构建validator与guest probe，不启动QEMU。
- Iteration 006执行Task 5.1，关闭自动产品、兼容、build与Review Gate并生成新鲜artifact。
- Iteration 007执行Tasks 6.1-6.2，完成人工single-hart QEMU MS06及MS01/MS04/MS05 runtime验收。

**Next Cycle**

`001-replan.md`

**Next Iteration**

None。Iteration 004必须先由`001-replan.md`取得accepted；Iteration 005只保留在Map中。
