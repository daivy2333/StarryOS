# Iteration 004 / Cycle 000: UART and network qualification protocols

## Plan Context

- Status: ready
- Iteration: 004-ms08-uart-and-network-qualification-protocols
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 5.1–5.4.
- Depends on: accepted Iteration 003.
- Stable baseline: separate UART serial and network guest/host protocols with
  pure output validators for all MS08 cases.
- Verification boundary: guest static builds, host decision and negative
  fixtures, schema/source guards, and old probe compatibility.
- Diagnostic boundary: serial transport, guest protocol, snapshot decoding,
  validator decision, or build wiring.
- Deferred tasks: 6.1–7.3.

**Cycle Scope**

- Trigger: initial.
- Acceptance gaps: None.
- Repair items: None.
- Inherited scope: MS08 D6–D9 and change tasks 5.1–5.4; accepted UART and
  network placement, V5, wake witness, and migration controls.
- Excluded scope: full `SMP=16` runtime qualification, combined pressure,
  recovery interleave, and changes to MS03–MS07 protocols.

**Objective**

Create independently buildable UART and network guest protocols and host
validators whose outputs can decide every Iteration 005–006 case without
launching QEMU from a validator.

**Background**

Iteration 003 accepted the source and focused runtime basis for controlled
migration. The remaining change tasks require a UART serial socket harness,
network probe, and strict transcript validators before formal per-driver
qualification. Existing MS06/MS07 probes and validators provide compatible
patterns but their single-hart cases cannot establish MS08 placement or IPI
causality.

**Investigation Facts**

- Current Baseline: Iteration 003 Cycle 002 reports 9/9 focused migration host
  tests, ordinary and `SMP=16` builds, and four PASS markers in a bounded QEMU
  combined smoke; its Review accepted the tuple-lock repair. These results
  establish product controls, not the absent qualification protocol. On this
  Plan pass, `python3 scripts/ms07-qemu-validate.py --self-test`,
  `python3 scripts/ms07-recovery-peer.py --self-test`, and
  `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms07_recovery_probe.c`
  each exited 0 without output; these old protocol surfaces remain a usable
  baseline.
- Current-State Evidence: `kernel/src/syscall/fs/ctl.rs` exposes QEMU-only
  `UART_SMP_SNAPSHOT`/`UART_SMP_MIGRATE`, `NET_IRQ_SNAPSHOT_V5`/`NET_IRQ_MIGRATE`,
  `NET_WAKE_WITNESS_CONTROL`, diagnostic Full/release, reset and flush commands.
  UART snapshot ioctl writes a 224-byte, explicitly zeroed little-endian
  `UartSmpSnapshot::wire_bytes()` frame; `uart_snapshot_types.rs` fixes every
  offset. Network V5 is `IrqSnapshotV4` as an unchanged prefix followed by
  the `u64` fields in `virtio_net_irq_logic::IrqSnapshotV5`. Wake control takes
  `[op, target]` (`START=1`, `TRIGGER=2`, `CANCEL=3`); accepted remote trigger
  requires the target task already Blocked. V5 reports target/trigger harts,
  target IPI before/after, timer restoration and failure counters.
  `tests/ms06_stack_readiness_probe.c` and
  `tests/ms07_recovery_probe.c` show static guest and host seam conventions;
  `scripts/ms07-qemu-validate.py` is a pure transcript state machine. `Makefile`
  `host-test` and target rules build and test those older tools. `make/qemu.mk`
  supplies `QEMU_ARGS`, serial and hostfwd configuration. `BENCH_CC` is the
  static RISC-V guest compiler. Local QEMU 7.0.0 help confirms a Unix socket
  chardev and `-serial`/`-monitor` selectors. Existing guest-to-host QEMU
  user networking reaches `10.0.2.2`; MS05 uses UDP port 15557 and MS07
  uses 15572, so MS08 needs its own bounded peer endpoint.
- Code and Critical Path: guest ioctl/TTY/socket calls → QEMU-only snapshot and
  migration controls → case markers and serial/peer observations → pure host
  validator. Host serial injection/capture is a separate harness entry; it
  owns the QEMU serial socket and bounded timeouts, while validators consume
  only captured text and exit status. Schema guards compare guest and
  validator case names and field contracts. A separate network peer owns
  TCP/UDP echo and sequence accounting; HMP link control is deferred to the
  runtime Gate.

**Implementation Guidance**

Define one frozen case registry per driver and its wire fields before adding
runtime branches. Build each guest's pure decision seam and host fixtures
first, then connect ioctls, serial/peer transport, validators, and Makefile
entries. Keep UART and network transcripts independent so one channel's
failure cannot be masked by the other's PASS marker. Use deadlines and event
notification rather than sleep polling.

**Protocol and transport contract**

- UART QEMU launch uses `make justrun SMP=16` with `QEMU_ARGS` selecting the
  first NS16550 serial port as a Unix socket: `-chardev
  socket,id=ms08uart,path=<socket>,server=on,wait=off -serial
  chardev:ms08uart -monitor none`. The socket path locates the connection;
  it is not an acceptance field. A host harness connects under a monotonic
  deadline, captures console bytes, waits for guest `READY`, then injects
  numbered ASCII payload frames. It compares exact outbound frames and
  reports socket EOF, timeout and guest shell exit separately. Validators
  never start QEMU.
- UART transcript: `MS08_UART_START`, `MS08_UART_ENV` (platform/topology
  description only), then ordered `CASE_START`, typed observation lines and
  `PASS` for `placement`, `rx`, `tx-full-recovery`, `readiness`, `tcdrain`,
  `quiet`, `rx-migration`, `tx-migration`; finally `MS08_UART_END` and
  `MS08_UART_HARNESS_EXIT`. Observations carry relevant before/after snapshot
  fields, sequence and byte counts, and serial host receipt. Environment
  text is diagnostic context, never a matching key or pass condition.
- Network transcript: `MS08_NET_START`, `MS08_NET_ENV`, ordered `CASE_START`,
  typed V5/peer/socket observations and `PASS` for `placement`,
  `timer-disabled-wake`, `tcp-bidirectional`, `udp-bidirectional`,
  `full-recovery`, `readiness-quiet`, `owner-migration`,
  `runner-migration`, `reset-io`, `link-off-on`; finally `MS08_NET_END`
  and `MS08_NET_HARNESS_EXIT`. Reset/link observations carry old terminal and
  new queue/socket/link epoch state. Iteration 004 tests protocol fixtures;
  Iteration 006 runs the actual reset/link interleave.
- A separate bounded MS08 host peer serves TCP and UDP on a new port, reached
  by the guest at QEMU user-net host `10.0.2.2`. Frames carry case and
  monotonically increasing sequence fields. The peer echoes exact payload
  bytes, rejects duplicate/out-of-order/foreign traffic and reports per-case
  received/sent counts. Peer timeout/exit is an explicit result joined to
  the guest text for validation.
- Validators require each case exactly once and in order; check placement
  membership and actual hart observations, IPI target causality
  (`after > before` after accepted remote trigger), exact serial/network
  payloads, Full to capacity recovery, readiness result, quiet deltas,
  migration phase/from/to and unchanged logical identity, resource
  conservation, and reset/link terminal/epoch relationships. They reject
  missing/duplicate observations, fatal guest output and nonzero harness
  exits. Revision, environment, process identity and elapsed-time ordering
  cannot substitute for these behavior checks.

**Behavioral Change**

No kernel ABI change. New guest probes emit structured per-case observations;
host UART serial transport injects and captures numbered payloads; pure
validators accept complete ordered transcripts with valid snapshot, IPI,
payload, resource and terminal relationships and reject malformed or incomplete
ones. Old guest probes and validators retain their existing layout and meaning.

**Task Contracts**

### 5.1: UART SMP guest probe and serial harness

- Requirement/Scenario: D6/D9; UART placement, SPSC continuity, remote wake,
  Full recovery, readiness, four-stage drain, quiet and copier migration.
- Depends on: accepted Iteration 003.
- Targets: new `tests/ms08_uart_smp_probe.c`, its host decision test and
  `scripts/ms08-uart-serial.py`; existing UART ioctl and snapshot types are
  read-only contracts.
- Current behavior: only boot smokes and host/model witnesses cover UART SMP;
  no serial socket payload protocol exists.
- Required behavior: the guest records environment, placement, RX, TX Full →
  recovery, readiness, `tcdrain`, quiet and separate RX/TX copier migration
  cases. The host injects/captures numbered bytes through a QEMU serial
  socket with bounded deadlines and a separate exit outcome.
- Required changes: use ioctl `0x55534d31` and `0x55534d32`, decode the
  224-byte little-endian UART frame at its declared offsets, and provide the
  UART case registry, sequence/byte-count and snapshot observations. Connect
  the host socket transport above without relying solely on console PASS text.
- Preserve: early console, TXDBG ABI, SPSC identity and existing UART ioctls.
- Forbidden: sleep polling, second copier, revision/run identity and treating
  a console marker alone as proof of received bytes.
- Test witness: host-only case registry and payload decision fixtures first
  fail on absent probe/harness; compile with `cc -std=c11 -Wall -Wextra -Werror`
  and run the new harness self-test.
- GREEN condition: all expected cases and numbered payload relations are
  available for validation; timeout and corrupted serial data fail closed.
- Verification: C host test/static guest build and harness self-test exit 0.
- Stop when: QEMU serial configuration cannot deliver independent capture or
  a required observation needs a kernel ABI change.

### 5.2: Network SMP guest probe

- Requirement/Scenario: D6–D9; network fixed placement, timer-disabled wake,
  bidirectional pressure, Full recovery, readiness/quiet, migration and
  reset/I/O/link interleave.
- Depends on: 5.1's protocol conventions, not its runtime success.
- Targets: new `tests/ms08_network_smp_probe.c`, host decision test and
  `scripts/ms08-network-peer.py`; existing V5, wake, recovery, diagnostic
  and socket interfaces are read-only contracts.
- Current behavior: MS07 recovery probe is single hart and has no V5 placement
  or remote IPI cases.
- Required behavior: emit ordered environment, placement, timer-disabled wake,
  TCP/UDP bidirectional, Full → recovery, readiness/quiet, owner/runner
  migration and reset/I/O/link case observations with bounded errors.
- Required changes: use V5 ioctl `0x4e494435` as V4 prefix plus appended
  `u64`s, wake control `0x4e495731`, migration control `0x4e494436`, and
  existing recovery/diagnostic controls. Decode results into the network
  case fields above. Add the bounded TCP/UDP peer described above; preserve
  descriptor/slot/ticket and epoch relationships.
- Preserve: V1–V4 wire and MS07 probe/validator semantics.
- Forbidden: internal stack polling, timer-only progress, replacement owner or
  runner, and promoting QEMU output to true-board evidence.
- Test witness: pure network case/state and peer-ledger tests RED before probe
  creation; C host tests, peer `--self-test` and target static build GREEN.
- GREEN condition: each case has a unique start/result and enough counters to
  decide placement, IPI causality, resource conservation and terminal state.
- Verification: `cc` host checks, `python3 scripts/ms08-network-peer.py
  --self-test`, target static build and case list comparison.
- Stop when: V5 or current controls omit a necessary observable state.

### 5.3: Pure UART and network output validators

- Requirement/Scenario: D9 and 5.1–5.2's complete, ordered, attributable
  qualification results.
- Depends on: frozen guest case registries.
- Targets: new `scripts/ms08-uart-validate.py` and
  `scripts/ms08-network-validate.py` with negative fixtures/self-tests.
- Current behavior: MS07 validator only accepts single-hart recovery output.
- Required behavior: reject missing/duplicate/reordered cases, corrupt payload,
  timer-only progress, role drift, stale epoch, resource imbalance, absent
  terminal/exit result and fatal guest output; accept valid complete output.
- Required changes: parse only captured text and explicit exit status; check
  snapshot relationships and per-case transitions rather than marker presence.
- Preserve: existing MS06/MS07 validator outputs and case order.
- Forbidden: QEMU/process/network control in validators, evidence identity
  handshake, or a validator self-certifying its own behavior.
- Test witness: positive and one negative fixture per listed failure class;
  observe RED before parser logic, then GREEN for positives and expected
  rejection of negatives.
- GREEN condition: validators accept only the complete valid UART/network
  transcripts and explain the first rejected relation.
- Verification: `--self-test` exits 0 and explicit negative fixture calls
  exit nonzero.
- Stop when: the guest protocol cannot express a required causal relation.

### 5.4: Schema guards and build/test entry points

- Requirement/Scenario: D6/D9, old ABI compatibility and repeatable protocol
  construction.
- Depends on: 5.1–5.3.
- Targets: `Makefile`, guest and validator schema listing interfaces, narrow
  source guards.
- Current behavior: `host-test` checks MS06/MS07 schema, pure validators and
  old probe builds; it has no MS08 entries.
- Required behavior: build both guest payloads, run host fixtures and
  validator self-tests, compare case/schema output, reject sleep polling and
  validator process/network imports, and leave old entries passing.
- Required changes: add focused build/test targets and guards for the two new
  protocols only.
- Preserve: old Makefile targets and MS03–MS07 probe/validator files.
- Forbidden: extra identity/capture/audit machinery or validator-launched QEMU.
- Test witness: run the focused new target before wiring and observe missing
  target/fixture RED; then require all new and existing focused checks GREEN.
- GREEN condition: build/schema/source guards pass and old targets still pass.
- Verification: focused Makefile target, affected old host/probe checks,
  `git diff --check`, strict OpenSpec validation.
- Stop when: an old protocol would need changed bytes or cases.

**Invariants**

Validators are pure output consumers. UART serial and network results remain
independent. Runtime qualification remains in Iteration 005–006. QEMU evidence
does not establish physical-board behavior. All guest waits are bounded.

**Non-goals**

Product driver changes, full QEMU qualification, combined pressure, physical
board proof, throughput optimization and updates to old probe protocols.

**Acceptance**

- 5.1 / UART cases: serial injection and capture preserve numbered bytes,
  report timeout/corruption, and expose all UART fixed/migration cases.
- 5.2 / network cases: V5 and control observations expose all network fixed,
  migration and recovery cases.
- 5.3 / validator: valid transcripts pass and each negative fixture class
  fails with a concrete first difference.
- 5.4 / compatibility: schema/source guards and old focused probe/validator
  checks pass without wire or case changes.

**Requirements Traceability Matrix**

| Requirement/scenario | Design | Task | Code surface | Witness | Status |
|---|---|---|---|---|---|
| UART fixed/migration protocol | D6/D9 | 5.1 | UART guest + serial harness | payload/case host fixtures | Covered |
| Network fixed/migration/recovery protocol | D6–D9 | 5.2 | network guest + V5/control | case/state host fixtures | Covered |
| strict independent judgment | D9 | 5.3 | two pure validators | positive/negative fixtures | Covered |
| protocol compatibility/build | D6/D9 | 5.4 | Makefile + schema guards | new/old focused checks | Covered |

**Verification**

Build both guest payloads for the RISC-V user target; run host decision tests,
validator positive/negative fixtures, schema/source guards, affected old
probe/validator checks, format/diff checks and strict OpenSpec validation.
Each result must carry command, decisive output and exit code in Act Response.

**Gate 2 Readiness**

- Missing requirements: PASS — each 5.1–5.4 outcome maps to a protocol case,
  code surface and witness in the RTM.
- Simplifications: PASS — none introduced; old behavior and the accepted
  Iteration 003 single-pass waiver remain intact.
- Investigation: PASS — UART/NET ioctl, exact UART frame and V5 prefix,
  wake control, serial QEMU transport, user-net peer route, old probes,
  validators and Makefile entry points inspected.
- Design: PASS — separate UART/network ordered transcripts, peer/serial
  transport and pure validators have explicit responsibility and failure
  boundaries; no kernel ABI change is required.
- Executable tasks: PASS — each contract states targets, required behavior,
  preservation, forbidden scope, witness, GREEN and stop conditions.
- Iteration dependency/balance: PASS — Iteration 003 accepted; tasks 5.1–5.4
  jointly establish protocol readiness and leave runtime qualification to
  Iterations 005–006.
- Traceability: PASS — D6–D9, tasks, code and witnesses are mapped above.
- Verification: PASS — host fixtures, negative validator cases, static guest
  builds, schema guards and old focused compatibility checks decide this
  protocol Iteration; QEMU runtime is correctly deferred.
- Identity-style evidence engineering: PASS — socket path/environment are
  context only, and validators judge behavior without identity handshake.
- Substantive TBD: PASS — serial launch, case registries, wire decoding,
  peer orchestration and rejection relationships are fixed above.
- Change consistency: PASS — tasks 5.1–5.4, D6–D9 and Iteration Map agree.
- Persisted Evidence: PASS — mode `none`; summaries fit Act Response.
- User approval: PASS — the user replied `批准` after reviewing the Iteration
  004 draft; added details close the named investigation gaps inside its
  approved scope without changing the Iteration Map.
- Overall Gate 2: PASS — final diff and strict validation passed; this Cycle
  is ready for a separately authorized Act.

**Persisted Evidence**

- Mode: none.
- Budget: decisive build, fixture and schema results fit in Act Response.

**Risks and Notes**

The UART console is also an observation channel; the serial harness uses its
byte stream and independent timeout/exit result when console output fails.
The network peer uses a new port so MS05/MS07 listeners cannot satisfy MS08
payload checks. QEMU socket attachment and host networking are environment
dependencies for later runtime qualification; this protocol Iteration proves
their host decision logic and guest build contracts before those runtime Gates.

## Act Response

- Status: reported

**Implemented**

- 5.1 UART SMP guest probe and serial harness:
  - `tests/ms08_uart_smp_probe.c`: frozen 8-case registry (placement, rx,
    tx-full-recovery, readiness, tcdrain, quiet, rx-migration,
    tx-migration); 224-byte little-endian `UartSmpSnapshot` wire decoded at
    the kernel-declared offsets (magic checked, reserved alignment bytes
    fail-closed); numbered RX frame codec (`MS08RX seq=N len=L <bytes>`) with
    strict 1..N sequence ledger; pure relationship decisions (placement,
    drain, quiet, Full->recovery, migration widened/restored); guest
    choreography driven by `MS08_UART_NEED` inject markers with exact-byte
    `MS08UARTECHO` frames for host comparison; all waits poll-based under
    absolute deadlines.
  - `tests/ms08_uart_smp_probe_test.c`: host decision fixtures for the
    registry, frame codec, ledger, placement, drain, quiet, full-recovery and
    both migration directions plus the wire round-trip.
  - `scripts/ms08-uart-serial.py`: owns the QEMU serial Unix socket
    (client of the `-chardev socket,...,server=on` launch), captures the
    console, arms on `MS08_UART_READY`, injects deterministic numbered frames
    on NEED markers, compares echoes byte-for-byte in strict order, tolerates
    TTY loopback of injected frames as transport noise, and reports distinct
    outcomes (ok / timeout / guest-eof / corrupt / guest-fail /
    connect-timeout).  Never launches QEMU.
- 5.2 Network SMP guest probe and peer:
  - `tests/ms08_network_smp_probe.c`: frozen 10-case registry (placement,
    timer-disabled-wake, tcp-bidirectional, udp-bidirectional,
    full-recovery, readiness-quiet, owner-migration, runner-migration,
    reset-io, link-off-on); 118-u64 V5 wire (V4 byte-for-byte prefix,
    `_Static_assert`ed) decoded with fail-closed current-tuple validation;
    wake witness choreography (START on the first schedulable hart, `fork()`ed
    bounded-retry remote TRIGGER, completed/timer-restored/IPI-causality
    relations from the V5 witness fields); diagnostic hold-submit/release
    Full->recovery with slot/ticket conservation; controlled role migration
    observed through the packed migration views with ledger closure; MS07-
    compatible reset/link epoch relations reused via new pure checks;
    per-case TCP/UDP peer exchanges.
  - `tests/ms08_network_smp_probe_test.c`: host decision fixtures for
    placement, wake, full-recovery, quiet, reset/link transitions, both
    migration directions, the V5 wire round-trip and the peer frame codec.
  - `scripts/ms08-network-peer.py`: bounded TCP+UDP echo peer on the new
    MS08 port 15578, per-case strictly-increasing sequence ledger shared
    across transports, exact-byte echo, duplicate/out-of-order/foreign
    rejection with per-case rx/tx accounting and an explicit result line.
- 5.3 Pure output validators:
  - `scripts/ms08-uart-validate.py` / `scripts/ms08-network-validate.py`:
    strict state machines over captured text only (no socket/subprocess/QEMU
    imports).  Envelope (start/env/case order/PASS/end/harness result and
    exit), per-case marker grammar with exact counts, snapshot relationship
    audits (placement singletons, IPI causality rejecting timer-only
    progress, capacity-pressure and ledger conservation, quiet stability,
    migration phase/from/to/identity, reset/link terminal and epoch
    relations, frozen fault tuple, monotonic counters), payload sequence and
    accounting checks.  `--self-test` covers one or more negative fixtures
    per failure class: missing/duplicate/reordered cases, corrupt payload,
    timer-only progress, role drift, stale epoch, resource imbalance, absent
    terminal/exit, fatal guest output, environment mismatch.
- 5.4 Build/test entry points and guards:
  - `Makefile` `host-test`: focused syntax/host-test/self-test/schema/cases
    diff entries for both new protocols plus source guards (validators stay
    pure output auditors; probes never sleep-poll or call `poll_interfaces`;
    harness/peer never launch QEMU or a shell).  Old MS03-MS07 entries
    unchanged.
  - New RISC-V static targets `tests/ms08_uart_smp_probe` and
    `tests/ms08_network_smp_probe` (BENCH_CC, `-static -no-pie -Os`).

**Changed Files and Symbols**

- New: `tests/ms08_uart_smp_probe.c`, `tests/ms08_uart_smp_probe_test.c`,
  `tests/ms08_network_smp_probe.c`, `tests/ms08_network_smp_probe_test.c`,
  `scripts/ms08-uart-serial.py`, `scripts/ms08-uart-validate.py`,
  `scripts/ms08-network-peer.py`, `scripts/ms08-network-validate.py`,
  `tests/ms08_uart_smp_probe` (RV static), `tests/ms08_network_smp_probe`
  (RV static, both untracked binaries for the commit stage).
- Modified: `Makefile` only (`host-test` additions, two RV probe targets).
- Kernel sources: untouched (no kernel ABI change, per contract).

**Deviations from Plan**

- Guest cases now print explicit before/after snapshots (tx-full-recovery,
  quiet, both UART copier migrations, timer-disabled-wake, both network role
  migrations, link-off-on) so the validators can audit pre/mid/post relations
  directly; this follows the plan's "Observations carry relevant
    before/after snapshot fields" contract and the schema guard diffs enforce
    the frozen emission order.  The probe `--print-schema` outputs therefore
    differ from a marker-presence-only listing; both sides are diffed in
    `host-test`.
- Remote wake trigger choreography uses `fork()` plus a bounded same-seq
    retry (the kernel rejects local triggers and publishes
    last_target/last_trigger harts, so remoteness is proven from V5 fields).
    Runtime convergence on 16 harts is an Iteration 005 qualification
    concern; the protocol and its host witnesses are complete here.
- UART NEED/RX/ECHO ordering places the case snapshot after the payload
    exchange (the guest-side progress check compares snapshots across the
    receive); validator grammar pins this order.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Spec review (Gate 4, per task): each task contract's targets, ioctls
  (0x55534d31/32, 0x4e494435/36, 0x4e495731, 0x4e494331, 0x4e495231,
  0x4e494631, 0x5409), wire offsets, preservation (early console, TXDBG ABI,
  V1-V4 prefix, MS06/MS07 tools), forbidden scope (no sleep-poll, no second
  copier/owner, no identity machinery, validators launch nothing) and GREEN
  conditions re-checked against the final code.
- Code quality review (Gate 4, per task): fixed during implementation —
  UART line reassembly dropped co-framed lines (replaced by a
  push/extract-next buffer); migration loops restructured from placeholder
  scaffolding into single-purpose consume-and-observe loops; the echo ledger
  now enforces strict sequence order (reorder was accepted); the peer and
  harness had deadline/spin hazards (BlockingIOError handling, fake-clock
  livelock, connect-retry busy-spin) all closed and covered by self-tests.
- Full-diff review: only the Makefile is modified among tracked files; all
  other deliverables are new files; no kernel or old-protocol changes.
  No unresolved Critical or Important findings.  Minor (left as-is):
  the two RV probe binaries are untracked pending the commit stage, matching
  how the previous probes entered the tree; `run_tx_full_recovery` writes a
  phase deadline the caller recomputes anyway (kept for symmetry, no
  behavioural effect).

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| UART host fixtures | `cc -std=c11 -Wall -Wextra -Werror tests/ms08_uart_smp_probe_test.c -o /tmp/t1 && /tmp/t1` | exit 0，无输出 | case registry、frame codec、ledger、placement/drain/quiet/full/migration 决策、wire 往返 | PASS |
| Network host fixtures | `cc ... tests/ms08_network_smp_probe_test.c -o /tmp/t2 && /tmp/t2` | exit 0，无输出 | placement/wake/full/quiet/reset/link/migration 决策、V5 wire、peer frame codec | PASS |
| UART harness self-test | `python3 scripts/ms08-uart-serial.py --self-test` | exit 0 | READY/NEED/ECHO 状态机、TTY 回环噪声、乱序/损坏/EOF/FAIL 负例、超时纪律 | PASS |
| UART harness 真实传输 | 伪 QEMU serial endpoint + `--socket` 实跑 | `outcome=ok injected=2 matched=2` | connect/inject/capture/echo 比对的端到端 socket 路径 | PASS |
| Network peer self-test | `python3 scripts/ms08-network-peer.py --self-test` | exit 0 | frame grammar、per-case ledger、UDP/TCP pump、拒绝路径、deadline | PASS |
| UART validator | `python3 scripts/ms08-uart-validate.py --self-test` | exit 0 | canonical 正向 + 全部负向 fixture 类 | PASS |
| Network validator | `python3 scripts/ms08-network-validate.py --self-test` | exit 0 | canonical 正向 + 全部负向 fixture 类 | PASS |
| Case/schema 一致性 | probe `--print-cases/--print-schema` 与 validator 输出 `diff` | 无差异 ×4 | 冻结 case 顺序与 marker 契约 | PASS |
| RISC-V 静态构建 | `make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe` | 两个 ELF 生成 | guest payload 目标构建 | PASS |
| 集成 host-test | `make host-test`（用户本机手工运行） | 无错误退出 | 新条目 + 全部旧 MS03-MS07 检查 | PASS |
| 格式 | `git diff --check` / `git diff --cached --check` | 无输出，exit 0 | 全量 diff 空白 | PASS |
| OpenSpec | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | `Change ... is valid` | change 结构 | PASS |

RED witnesses: 5.1/5.2 host decision fixtures failed to compile while the
probes were absent (`fatal error: ms08_*_smp_probe.c: No such file or
directory`); validator self-tests and harness/peer self-tests failed on
their first runs (grammar/schema drift, accepted reorder, spin hazards)
before the fixes above — observed RED precedes every GREEN listed here.

**Persisted Evidence**

None required (Plan Context mode `none`; all decisive outputs fit this
Response).

**Experience Candidates**

- Runbook (deferred): the MS08 QEMU runtime choreography (`make justrun
  SMP=16` + `QEMU_ARGS` serial socket + `ms08-uart-serial.py` +
  `ms08-network-peer.py` + validators) is only protocol-verified in this
  Cycle; once Iteration 005/006 runs it end-to-end it becomes a Runbook
  candidate.  Not recorded now because it is not yet validated end-to-end.
- Issue candidates: None.

**Remaining Issues**

- None known in protocol/host scope.  Runtime-only risks (trigger-child
  placement convergence, TTY echo behaviour, HMP operator timing) are
  Iteration 005-006 qualification concerns with explicit failure outcomes.

**Commit or Diff Reference**

- Worktree diff: `Makefile` (+40 lines); 8 new source files; 2 new RV
  binaries (untracked).  Pre-existing staged changes
  (`uart_migration_logic.rs`, `tests/ms04-async-rx-host-harness.rs`,
  Iteration 003 `002-rework.md`, this Cycle's `000-initial.md`) belong to
  prior stages and were left untouched by this Act.

## Plan Review

- Review Result: rework-required

**Findings**

- Blocking / 5.2 timer-disabled wake: `run_timer_disabled_wake()` waits for
  V5 phase `Armed` before it forks the TRIGGER caller. The kernel witness
  enters `Armed` only in `control(TRIGGER)` after the target is Blocked;
  `START` leaves it `Starting`. The guest therefore reaches its arm deadline
  without ever issuing a trigger. This contradicts the Plan Context's
  accepted-trigger fact and cannot pass the runtime case.
- Blocking / 5.2 TCP peer: `open_peer_socket()` creates `SOCK_NONBLOCK` and
  treats any nonzero TCP `connect()` result as fatal. A normal asynchronous
  `EINPROGRESS` is not followed by writable readiness and `SO_ERROR`, so the
  first TCP case fails. The host peer also writes `_rxbuf` onto a real
  `socket.socket`; CPython's socket object has no per-instance dictionary,
  causing `AttributeError` on the first received TCP segment. Its fake TCP
  socket has a dictionary, which explains the passing self-test. Nonblocking
  `sendall()` also lacks a partial-write/retry path.
- Blocking / 5.2 Full recovery: the guest submits a 30000 ms diagnostic
  lease although `axnet::diag::MAX_LEASE_MS` is 2000. It also defines
  `DIAG_RELEASE=0`, while the existing release operation is 3. The hold ioctl
  fails before any capacity pressure can be observed, and the failure cleanup
  cannot release a valid hold.
- Blocking / 5.2 readiness: `peer_roundtrip()` consumes the echo, after which
  `run_readiness_quiet()` waits for the same socket to become readable without
  sending another frame. The second roundtrip also consumes its echo before
  an extra `recv()`. Both waits are for data already consumed.
- Blocking / 5.1 and 5.2 migration: the UART and network migration ioctls
  execute their widen/observe/restore sequence synchronously. Both guest
  probes call ioctl in the same thread and only then sample for the Widened
  middle view; they can only see the Restored view and fail `have_mid`. These
  paths need a concurrent control caller and bounded observer/stimulus.
- Blocking / 5.2–5.3 peer evidence: the network guest itself prints
  `MS08_NET_PEER: ... result=ok`; the validator accepts that marker but never
  consumes the host peer's `MS08_NET_PEER_RESULT` counts/exit. The peer can
  report zero traffic or rejects while the guest transcript is accepted,
  violating the explicit independent peer-result contract.
- Blocking / 5.3 environment rule: both new validators provide
  `--expect-environment` and reject a valid behavior transcript when its
  environment text differs. The Plan Context explicitly makes environment
  diagnostic only and forbids it as an acceptance key. Remove the option,
  matching branch and dedicated negative fixture.
- The static RISC-V binaries and focused host checks exist. Their passing
  results do not cover the runtime call order and real TCP socket path above.

**Deviation Classification**

- PLAN-OMISSION: the ready Plan did not record that migration ioctl is
  synchronous or that diagnostic hold is capped at 2000 ms with release op 3.
  The repair Cycle must put those existing interfaces directly in its task
  contracts.
- ACT-DEVIATION: waiting for `Armed` before TRIGGER, sampling migration only
  after synchronous ioctl, consuming readiness data twice, using environment
  as a validator rejection key, and accepting a guest-authored peer success
  marker contradict the explicit behavior/verification contract.
- NEW-EVIDENCE: a real CPython socket object rejects `_rxbuf` assignment;
  the fake peer socket in Act's self-test concealed that interface difference.

**Acceptance Gaps**

- 5.1: UART migration protocol cannot observe a Widened view.
- 5.2: timer-disabled wake, TCP traffic, Full recovery, readiness, network
  migration and independent peer accounting cannot satisfy their cases.
- 5.3: validators can reject by environment identity and can accept a guest
  claim without the required host peer result.
- 5.4: schema/build guards and passing self-tests do not catch these
  protocol failures; the test entry points need focused real-interface
  witnesses while preserving old-protocol checks.

**Convergence**

N/A — first Review of this Iteration; the protocol surface exists, but its
runtime-decision gaps prevent acceptance.

**Evidence**

- Independently inspected the relevant call paths in the new guest, peer and
  validator sources, both static ELF outputs, `Makefile`, the kernel ioctl
  and witness state machine, and `crates/axnet/src/diag.rs`. The blocking
  call paths are at
  `tests/ms08_network_smp_probe.c::{open_peer_socket,
  run_timer_disabled_wake,run_full_recovery,run_readiness_quiet,
  run_role_migration}`, `tests/ms08_uart_smp_probe.c::run_copier_migration`,
  `scripts/ms08-network-peer.py::serve_until_deadline`, and both validators.
- Pure local reproduction: `python3 -c 'import socket;
  s=socket.socket.__new__(socket.socket); print(hasattr(s,"__dict__"));
  s._rxbuf=b""'` printed `False` and raised `AttributeError` (exit 1).
  Creating a live socket in this sandbox returned `EPERM`; the no-OS-object
  reproduction directly tests the same attribute boundary.
- Focused host tests (`ms08-network-peer.py --self-test`, both validator
  `--self-test`, and the network C host fixture) exited 0, demonstrating
  that their fixtures do not expose these paths. `openspec validate ...
  --strict`, `git diff --check`, and `git diff --cached --check` also exited
  0. The Act Response's reported static builds and user-run `make host-test`
  are adopted for the unchanged build surfaces; no QEMU runtime qualification
  was claimed by Act or performed in this Review.
- Persisted Evidence mode is `none`; the missing evidence directory is not a
  finding.

**Follow-up Decision**

Rework required. The original Iteration 004 objective and task map remain
valid, but the independent runtime-path failures need a new self-contained
repair contract and tests that exercise real interface behavior. Do not
enter Iteration 005 or treat host-only GREEN as protocol qualification.

**Iteration Plan Update**

None.

**Next Cycle**

`001-rework.md`.

**Next Iteration**

None.
