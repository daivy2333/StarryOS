# Iteration 009 / Cycle 000: Probe and Automatic Product Gates

## Plan Context

- Status: ready
- Iteration: 009-probe-and-automatic-product-gates
- Cycle: 000-initial
- Cycle Type: initial

**Iteration Scope**

- Change tasks: 5.1, 5.2
- Depends on: accepted Iteration 008 Service-owned diagnostic lease and all earlier accepted data-plane,
  flush and V3 ABI baselines
- Stable result: deterministic MS05 guest/host probe tooling exists, every automatic product Gate is
  classified from raw output, and fresh image/payload provenance plus full Review is persisted for
  the manual runtime handoff.
- Verification boundary: probe mutation/self-tests, all driver/axnet/UART/host/build/format/source/
  OpenSpec Gates and Evidence audit are complete. Only an R44-qualified environment limitation or
  manual QEMU console execution may remain.
- Diagnostic boundary: probe decision/parser failures, product test/build failures and environment
  capability failures are recorded separately; an unclassified nonzero result is a product failure.
- Deferred tasks: 6.1-6.3

**Objective**

Implement Task 5.1's bounded, non-ambiguous MS05 runtime witness and execute Task 5.2's complete
automatic Gate stack. Produce a self-contained Evidence package that identifies the exact binaries
eligible for Iteration 010 without running or automating the QEMU guest console.

**Authorization and Stop Boundary**

- This Plan authorizes edits only to the MS05 probe, its decision harness, its host stimulus,
  Makefile integration and the required change-local Evidence.
- Do not modify queue ownership, driver, axnet, kernel ABI or diagnostic semantics in this Cycle. A
  probe requirement that cannot be met by the accepted V3/control/flush interfaces returns to Plan.
- Do not start QEMU, drive a guest shell, mount a rootfs or claim runtime PASS. R44 makes those user
  terminal operations part of Iteration 010.
- Stop on the first product failure after preserving its raw command, output and exit status. Only a
  read-only path, unavailable network/tool installation, `EPERM`, `SIGSYS`/`Bad system call`, or a
  user-controlled terminal/privilege boundary proven by the raw log may be `ENV-BLOCKED`.

**Current Baseline**

- Branch `net-k3`; planning HEAD `223f6281d62b6925fa3f830690945dccab424022`, with the accepted
  Iteration 008 implementation and OpenSpec records still in the worktree.
- Iteration 008 Review independently passed axnet qemu-diagnostics `234/234`, default `215/215`,
  axdriver_net `7/7`, axdriver_virtio net `16/16`, virtio-drivers alloc `36/36` plus `8/8`
  doctests, MS03 `33/33`, MS04 `16/16`, kernel QEMU, rustfmt, strict OpenSpec and diff checks.
- The D1 comparison still exits 101 with exactly 25 established axfs/axtask errors; this is a
  comparison Gate, not a successful D1 build and not an environment waiver.
- `tests/ms05_data_plane_probe.c`, its host decision harness and
  `scripts/ms05_data_plane_stimulus.py` do not exist. The Makefile has MS03/MS04 static payload
  targets and host tests but no MS05 target or decision/stimulus Gate.
- V3 is a fixed `72 * u64` ABI. Its first 28 fields preserve V2 and its appended ledger includes
  RX/TX slots, TX buffer/descriptor ownership, stage exhaustions, queue generation/wake, tickets,
  flush, lease/fault and stable drop-reason fields.
- The kernel exposes V3 `0x4e49_4433`, QEMU-only diagnostic control `0x4e49_4331` and QEMU-only
  flush `0x4e49_4631`. Control uses `[op, lease_ms]`; the maximum lease and kernel flush deadline
  are two seconds.
- R51 requires fresh compatibility payloads and retains its manual snapshot/idle/nudge/burst
  process. The MS04 C/Python source and protocol must remain unchanged.

**Relevant Artifacts**

| Area | Files | Responsibility |
|---|---|---|
| Guest decision | `tests/ms05_data_plane_probe.c` | V3/control/flush ABI, bounded modes, phase snapshots and unique terminal marker |
| Host decision tests | `tests/ms05_data_plane_probe_test.c` or an equivalently named C harness | mutate phase/counter/deadline/marker/exit inputs without a guest |
| Host protocol | `scripts/ms05_data_plane_stimulus.py` | bounded traffic/control protocol, sequence validation and deterministic self-tests |
| Build integration | `Makefile` | strict host Gates and static RISC-V MS05 payload target |
| Compatibility | `tests/ms01_socket_baseline.c`, `tests/ms02_guest_service.c`, `tests/ms03_irq_probe.c`, `tests/ms04_rx_probe.c`, `scripts/ms04_rx_stimulus.py` | fresh handoff payloads and unchanged MS04 behavior |
| Evidence | `evidence/009-probe-and-automatic-product-gates/000-initial/` | environment, commands, raw logs, exits, hashes, Review and handoff |

**Critical Path**

```text
accepted V3 + bounded control + C4 flush
  -> C decision model rejects incomplete or synthetic histories
  -> guest probe emits PRE -> HELD -> FULL -> RELEASED -> POST
  -> host stimulus validates bounded control and packet sequence
  -> static RISC-V payload and fresh QEMU image are built
  -> automatic product Gates and full Review pass
  -> hashes + raw logs + ENV-BLOCKED list form Iteration 010 handoff
```

**Behavioral Change**

No kernel or library behavior changes are planned. This Cycle adds test/runtime tooling and build
integration around accepted interfaces. It must not reset telemetry, create another owner, infer
Full from throughput, edit ring/slot state or relax an existing compatibility check.

## BDD and Traceability

| Contract | Requirement / design | Scenario | Automatic witness |
|---|---|---|---|
| C1 | R6, R14, D9 | V3 layout and ioctl commands are exact; V1/V2 consumers remain unchanged | C size/offset/canary and source guards; MS03/MS04 regressions |
| C2 | R6, D9 | each mode records PRE/HELD/FULL/RELEASED/POST and exactly one terminal marker | C decision harness mutations and marker parser tests |
| C3 | R1-R5, D9 | slot/descriptor Full is proven by its exact ledger, then recovery closes ownership | fake-Full and non-closed-ledger mutations fail |
| C4 | R4, D8-D9 | flush is bounded and succeeds only for a closed construction-time target | deadline/equal-boundary, wrong-exit and incomplete-ledger mutations fail |
| C5 | R6, R14, D10 | host traffic/control is bounded, ordered and rejects malformed/duplicate input | Python protocol and loopback self-tests |
| C6 | R14, D10, R44/R51 | every automatic result and fresh artifact is traceable; environment blocks are narrow | Evidence index, raw logs, exits, hashes and full Review |

### Scenario: Missing or reordered phase fails

- **Given** a candidate mode history lacks PRE, skips HELD/FULL/RELEASED, reorders phases or emits a
  second terminal marker
- **When** the host decision harness evaluates it
- **Then** it returns failure and cannot produce `MS05 PASS mode=<mode>`

### Scenario: Full requires exact ledger evidence

- **Given** traffic was sent but the relevant slot or descriptor ledger never reaches its exact Full
  boundary, or ownership conservation does not close after Release
- **When** `slot-full` or `descriptor-full` is evaluated
- **Then** the mode fails even if all packets appeared to make progress

### Scenario: Deadline boundary is inclusive and bounded

- **Given** a phase completes before, exactly at, or after its fixed deadline
- **When** the decision helper evaluates elapsed time
- **Then** only the strictly in-budget completion is eligible for PASS; equal/after deadline,
  overflow or clock regression fails deterministically

### Scenario: Control and protocol input are strict

- **Given** malformed control, wrong peer/mode/count, duplicate sequence, missing packet, wrong exit
  status or a duplicate terminal report
- **When** the C/Python parsers consume it
- **Then** they reject it without silently completing the mode

### Scenario: Automatic Gate classification is evidence-based

- **Given** an automatic command exits nonzero
- **When** its earliest failure layer and final status are reviewed
- **Then** it is `ENV-BLOCKED` only for an R44 capability boundary; otherwise Act stops with a
  product failure and does not enter Iteration 010

## Task Contracts

### T5.1: Deterministic probe, decisions and bounded host protocol

- Requirement/Scenario: R6/R14, D9-D10, C1-C5.
- Depends on: accepted V3 ABI, Service-owned Hold/Release lease, exact slot/descriptor ledgers and C4
  flush.
- Targets: `tests/ms05_data_plane_probe.c`, one C host decision harness,
  `scripts/ms05_data_plane_stimulus.py`, `Makefile`.
- Required behavior:
  - support `snapshot`, `tx-only`, `bidirectional`, `slot-full`, `descriptor-full` and `flush` modes;
  - use normal socket traffic and only the published V3/control/flush ioctls;
  - take and validate PRE/HELD/FULL/RELEASED/POST snapshots as applicable;
  - prove slot Full from 64-slot occupancy/transition telemetry and descriptor Full from the driver
    buffer/descriptor ledger, never from packet count or throughput alone;
  - keep fixed total deadlines within the two-second lease and treat equal-deadline completion as
    expired; bounded retry may handle `EAGAIN`/`EWOULDBLOCK` but may not sleep or retry indefinitely;
  - emit exactly one `MS05 PASS mode=<mode>` or `MS05 FAIL mode=<mode> ...`, and return an exit status
    consistent with that marker;
  - preserve monotonic counters and validate ownership, ticket, fault and safety fields at POST;
  - validate host registration/start/traffic sequence, peer, count, payload and mode with bounded
    socket timeouts.
- RED witnesses: add C mutations for missing PRE, reordered phase, fake Full, counter regression,
  non-closed slot/ticket/buffer/descriptor ledger, before/equal/after deadline, duplicate terminal
  marker and wrong exit. Add Python self-tests for malformed control, wrong peer/mode/count,
  duplicate/missing/out-of-order sequence, timeout and successful bounded exchanges.
- Preserve: all V1/V2/V3 offsets and commands, MS04 source/schema/stimulus, kernel/axnet behavior,
  QEMU-only control scope and unique queue ownership.
- Forbidden: telemetry reset; private/raw ring or slot hook; fake completion; unbounded retry/sleep;
  throughput-as-Full; runtime source rewriting; QEMU automation; a parser that accepts partial output.
- GREEN condition: strict C compilation, all mutation/decision tests, Python self-tests and the static
  RISC-V MS05 payload build pass; each planned mutation is shown to fail before its positive case.
- Stop when: exact Full/recovery or flush closure cannot be decided from the accepted V3/control
  contract, or the probe needs a product ABI/semantic change. Return to Plan instead of modifying
  kernel or libraries under Task 5.1.

### T5.2: Automatic Gates, provenance and independent Review

- Requirement/Scenario: R14, D10, C6.
- Depends on: T5.1 GREEN and accepted Iterations 000-008.
- Targets: the full product/test tree as read by the listed Gates and
  `evidence/009-probe-and-automatic-product-gates/000-initial/` for writes.
- Required behavior:
  - capture environment, revision plus worktree state, every exact command, start/end time, raw
    stdout/stderr and final exit code before summarizing a result;
  - run model/driver Gates before host/probe and target build Gates; do not continue past a product
    failure merely to collect more green output;
  - build a fresh QEMU image, the MS05 payload and the four compatibility payloads MS01-MS04; record
    file type, byte size, mtime and SHA-256. A pre-existing binary is not fresh unless rebuilt in this
    Cycle and tied to its build log;
  - perform specs-vs-code and full-range diff Review with zero unresolved Critical/Important finding,
    no Missing task, no unapproved Simplified behavior and no unresolved design decision;
  - list every `ENV-BLOCKED` command unchanged for Iteration 010, including its exit, earliest
    capability failure and required external rerun. An empty list must be explicit.
- Preserve: raw logs byte-for-byte. Whitespace checks may exclude the Evidence directory so terminal
  escape/output bytes are not normalized, but may not edit raw output to manufacture PASS.
- Forbidden: historical artifact substitution, manual QEMU, environment failure inferred without a
  raw log, product failure relabeled as environment, or a summary without command/exit provenance.
- GREEN condition: every automatic product Gate passes; any remaining item is solely an R44-qualified
  `ENV-BLOCKED` handoff; Evidence index and hashes are complete and self-consistent.
- Stop when: any compile, link, assertion, parser, source, validation or diff Gate fails; preserve the
  failure and return to Plan Review without running downstream manual work.

## Acceptance

| Contract | Proof | Status |
|---|---|---|
| C1 | exact V3 C ABI plus unchanged V1/V2/MS04 source and consumer regressions | Planned |
| C2 | all modes and phase/marker mutations are deterministic and bounded | Planned |
| C3 | slot/descriptor Full and recovery require exact closed ledgers | Planned |
| C4 | flush deadline and C4 closure mutations reject partial success | Planned |
| C5 | Python protocol rejects malformed/duplicate/lost/reordered exchanges | Planned |
| C6 | automatic Gate logs, exits, fresh artifact hashes and full Review are complete | Planned |

Any runtime inference without an exact ledger, missing RED mutation, duplicate/partial terminal
marker, unbounded retry, MS04 source change, stale artifact, uncaptured exit, unjustified
`ENV-BLOCKED`, unresolved Critical/Important finding or manual QEMU action blocks acceptance.

## Verification

Act must preserve exact commands and final exits. At minimum it must run the following groups; the
new Makefile target names may be chosen during implementation but must be recorded literally.

```text
# Probe/product RED-GREEN
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c
cc -std=c11 -Wall -Wextra -Werror tests/ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test
/tmp/ms05-data-plane-probe-test
python3 scripts/ms05_data_plane_stimulus.py --self-test
python3 scripts/ms05_data_plane_stimulus.py --loopback-self-test
make -B tests/ms05_data_plane_probe

# Existing host and model compatibility
make host-test
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test

# Required race stability
repeat diagnostic_control_shared_path_is_bounded_and_publishes_after_unlock 100 times
repeat v3_shared_snapshot_path_returns_only_committed_tuples_under_control_and_tick 100 times
repeat the accepted default-parallel axnet full-suite Gate 100 times

# Target checks/builds and fresh artifacts
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
make LOG=info build
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c
make -B tests/ms02_guest_service tests/ms03_irq_probe tests/ms04_rx_probe tests/ms05_data_plane_probe
file StarryOS_riscv64-qemu-virt.bin tests/ms01_socket_baseline tests/ms02_guest_service tests/ms03_irq_probe tests/ms04_rx_probe tests/ms05_data_plane_probe
stat -c '%y %s %n' StarryOS_riscv64-qemu-virt.bin tests/ms01_socket_baseline tests/ms02_guest_service tests/ms03_irq_probe tests/ms04_rx_probe tests/ms05_data_plane_probe
sha256sum StarryOS_riscv64-qemu-virt.bin tests/ms01_socket_baseline tests/ms02_guest_service tests/ms03_irq_probe tests/ms04_rx_probe tests/ms05_data_plane_probe

# Quality and Review
rustfmt --check --edition 2024 --config skip_children=true <all change-owned Rust files>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- . ':(exclude)openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/**'
specs-vs-code review for R1-R15 and D1-D10
full git diff review from the recorded baseline through worktree state
Evidence index/hash/completeness audit
```

The D1 command must still be classified by exact comparison: only the established 25 axfs/axtask
errors satisfy the current expected boundary. `make host-test` must run to its final status; if its
UDP loopback or any static cross-build is denied by `EPERM`/`SIGSYS`, retain the raw output and apply
R44 rather than reporting the command as PASS.

## Gate 2 Readiness

| Dimension | Status | Evidence |
|---|---|---|
| Authorization | PASS | accepted Iteration 008 Review requires expansion of the next planned logical Iteration |
| Investigation | PASS | V3/control/flush ABI, current probe/Makefile patterns, R44/R51 and Evidence precedent inspected |
| Design | PASS | D9 defines deterministic control/snapshot phases; D10 fixes Gate order and runtime boundary |
| Iteration Plan | PASS | Tasks 5.1-5.2 form the existing Iteration 009 map and leave manual runtime in Iteration 010 |
| Task Contracts | PASS | probe behavior, RED mutations, provenance, classifications and stop rules are explicit |
| Traceability | PASS | R1-R6/R14, D8-D10 map to C1-C6, T5.1/T5.2 and named witnesses |
| Verification | PASS | decision/protocol, model/driver/UART, races, target builds, hashes and Review are layered |
| Evidence | PASS | required directory, files, raw-output policy and Iteration 010 handoff are defined |

## Persisted Evidence

- Mode: required
- Root: `evidence/009-probe-and-automatic-product-gates/000-initial/`

Act must create at least:

| File | Required content |
|---|---|
| `README.md` | indexed Gate/result table, collection window, revision/worktree identity and scope limits |
| `environment.txt` | host, sandbox, toolchains and relevant compiler/QEMU versions |
| `commands.txt` | ordered exact commands, start/end timestamps, raw-log path and final exit |
| `probe-tests.log` | unedited C/Python decision, mutation, parser and protocol output |
| `automatic-gates.log` | unedited Rust/host/100x/format/source/OpenSpec Gate output |
| `build.log` | unedited QEMU/D1 checks, image and static payload build output |
| `artifacts.sha256` | file type, mtime, byte size and SHA-256 for the fresh image and five payload binaries |
| `env-blocked.txt` | each R44-qualified item and exact Iteration 010 rerun, or explicit `None` |
| `review.md` | specs-vs-code matrix, full diff findings/severity/disposition and scope exclusions |

Additional split raw logs are allowed if `README.md` and `commands.txt` index them. Do not hash or
normalize a derived replacement while discarding the original log.

## Risks and Notes

- The accepted diagnostic lease is at most two seconds. Host scheduling and network timeouts must
  leave margin for Release and POST rather than treating lease expiry as successful recovery.
- V3 gauges and monotonic counters need different delta rules. Occupancy/availability are state;
  enqueue/dequeue/full/reclaim/error fields are monotonic and counter regression is failure.
- QEMU user networking can reorder/drop UDP. The protocol must identify sequence and fail boundedly;
  it must not turn retransmission or timeout into an infinite wait.
- `make build` may emit intermediate Cargo-home/network warnings yet finish successfully. R44 uses the
  final exit and produced artifact, while the earliest layer controls only an actual nonzero result.
- Iteration 009 prepares but does not validate runtime behavior. Artifact hashes establish identity,
  not TX/RX/flush correctness; those claims require Iteration 010 raw serial evidence.

## Act Response

- Status: pending

## Plan Review

- Status: pending

**Review Result**

Pending.

**Findings**

Pending.

**Deviation Classification**

Pending.

**Acceptance Gaps**

Pending.

**Convergence**

Pending.

**Evidence**

Pending.

**Follow-up Decision**

Pending.

**Iteration Plan Update**

None.

**Next Cycle**

Pending.

**Next Iteration**

Pending.
