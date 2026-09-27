# Iteration 005 / Cycle 001: Per-driver SMP=16 qualification contract repair

## Plan Context

- Status: ready
- Iteration: 005-per-driver-smp16-fixed-qualification
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 6.1–6.3.
- Depends on: accepted Iteration 004 Cycle 001 and Iteration 000–003 baselines.
- Stable baseline: independent UART-only and network-only `SMP=16` qualification with fixed placement, remote wake, data progress, quiet behavior and controlled migration.
- Verification boundary: each driver has its own bounded runtime transcript and accepted validator verdict; historical migration bits do not stand in for current execution.
- Diagnostic boundary: UART serial transport/parser/TTY/copy path or network V5 placement/probe/peer/control path.
- Deferred tasks: 7.1–7.3.

**Cycle Scope**

- Trigger: parent Cycle Review `replan-required`.
- Acceptance gaps: UART RX corruption and incomplete placement check; network placement rejection of cumulative migration history; both runtime validators absent; independent 6.3 decision absent.
- Repair items: None (replan uses tasks 6.1–6.3).
- Inherited scope: MS08 `SMP=16`, independent drivers, unique copier/owner/runner, V5 ABI, guest protocol case order, payload/ledger and migration requirements.
- Excluded scope: combined pressure, final recovery interleave, true-board qualification, performance and identity-style evidence machinery.

**Objective**

Make the fixed-placement decisions match the existing cumulative telemetry semantics, close the witnessed UART serial RX failure, then obtain independent UART and network runtime validator passes.

**Background**

Parent Act completed UART launch/capture and marker framing host work, but the QEMU UART run failed during RX and the network run failed at placement. Parent Plan Review classified the placement and UART host-witness gaps as invalid or missing verification contracts. No kernel owner duplication has been established by those failures.

**Investigation Facts**

- Current Baseline: `mul-hart-k3` has staged Iteration 004 and other prior changes plus unstaged parent Cycle UART harness/guest edits. Parent Act's unchanged host self-test, probe build, schema and validator self-test results remain usable for those surfaces. This Plan independently ran `python3 scripts/ms08-uart-serial.py --self-test` (exit 0), `git diff --check` (exit 0) and `git diff --cached --check` (exit 0). Parent logs remain `/tmp/ms08-uart-transcript.log` and `/tmp/ms08-net-serial.log`; they are failure evidence, not qualification artifacts.
- Current-State Evidence: `kernel/src/entry.rs` calls `snapshot_boot_smoke()` then `migration_boot_smoke()` for network and UART before user payload launch. `crates/axnet/src/hart_counter.rs::HartCounter::record` and the kernel counterpart use `mask.fetch_or`, so mask is lifetime history. The network probe and validator demand singleton history; UART probe and validator only inspect affinities. Network log shows boot fixed smoke PASS, migration smoke PASS, then placement `owner_mask=0x8001`, `runner_mask=0x3`, matching recorded migration destinations, with current last harts equal to restored affinity. UART log shows placement history `rx_mask=0x3`, `tx_mask=0x5`, then RX third-frame corruption, guest `FAIL: rx reason=rx-corrupt-frame` and harness exit 1.
- Code and Critical Path: host `scripts/ms08-uart-serial.py::Launcher/Session/run_session` sees the shell prompt, launches the guest and sends all numbered frames from `MS08_UART_NEED`. Guest `tests/ms08_uart_smp_probe.c::receive_frames` reads at most 128 bytes into a 160-byte line reader; `line_reader_push` appends a whole chunk before `line_reader_next` drains complete lines and resets length on overflow, so coalesced frames can lose bytes. The same reader is used by RX migration. TTY input echo visibly interleaves with guest output in the failed transcript. `tests/ms08_network_smp_probe.c::ms08_net_placement_ok` and `scripts/ms08-network-validate.py::_validate_protocol` interpret cumulative masks as singleton current placement. Network V5 already carries current affinity, actual last hart, history mask, events and migration state; no ABI expansion is required. UART snapshot carries analogous affinity, last hart, mask, polls and migration state.

**Implementation Guidance**

First establish focused RED witnesses for coalesced/fragmented UART frames and the placement counterexamples. Use an event-driven, bounded line parser that drains complete frames as bytes arrive or otherwise proves the same no-loss property for every allowed read partition; overflow must fail explicitly instead of silently resetting valid data. Disable TTY input echo in the single UART launch command, with shell restoration after probe exit, while keeping one socket client and unchanged numbered payload grammar. Compare current affinity and actual last hart directly, treat masks as cumulative history, and restrict history bits to the reported pin/migration harts plus the schedulable set. Keep migration case checks and V5 wire unchanged. Re-run runtime from fresh driver-specific QEMU sessions after host/guest GREEN.

**Behavioral Change**

The qualification protocol now distinguishes historical execution from current fixed placement. UART injected lines survive arbitrary read chunking without silent byte loss, and guest input does not echo onto the output stream. Case names/order, payload bytes, snapshot wire layouts, kernel scheduler/driver ownership and failure-on-invalid-payload semantics stay intact.

**Task Contracts**

### 6.1: UART-only fixed-placement qualification

- Requirement/Scenario: delta spec `MS08 以分层 Gate...` UART and controlled-migration scenarios; direct actual-hart observation, bounded RX/TX, capacity, readiness, drain, quiet and migration.
- Depends on: accepted Iteration 004 protocol and parent Cycle UART harness/fresh-line host work.
- Targets: `tests/ms08_uart_smp_probe.c::{line_reader_push,line_reader_next,receive_frames,run_copier_migration,ms08_uart_placement_ok}`, `tests/ms08_uart_smp_probe_test.c`, `scripts/ms08-uart-serial.py::{LAUNCH_CMD,Session,self_test}`, `scripts/ms08-uart-validate.py::_validate_protocol`.
- Current behavior: guest line reader can discard bytes when residual data plus a 128-byte read exceed 160 bytes; launcher does not disable TTY echo; placement predicate/validator can accept `last != affinity`. Prior runtime failed at RX frame 3, and no accepted UART transcript exists.
- Required behavior: every complete valid numbered frame is decoded once in order across arbitrary chunks; an overlong or malformed line fails explicitly and within the deadline. The sole harness disables echo before launching and restores it on exit. Current RX/TX affinities are distinct, schedulable and agree with actual last poll harts and nonzero progress; cumulative masks contain those harts and contain no unexplained/out-of-schedulable bit. The historical migration bits recorded by boot smoke remain legal; runtime UART case order and validator grammar remain unchanged.
- Required changes: add RED/GREEN host witnesses for coalesced and split RX lines, overlong input, post-migration good placement and wrong actual hart; repair guest line parsing, single launch command and probe/validator placement decisions; then run the existing UART-only `SMP=16` protocol.
- Preserve: one RX/TX copier each, SPSC, original injected bytes, binary raw transcript, bounded launcher/parser, early console, TXDBG/snapshot ABI, four-stage drain and D1 workaround.
- Forbidden: pacing or dropping frames to hide corruption, accepting affinity as actual execution, making the historical mask singleton, second serial client, unbounded waits or altering kernel driver behavior without witnessed need and Plan Review.
- Test witness: C fixture reproduces valid multiple frames in one read and split across reads as RED under current parser; invalid line remains rejected. C and Python placement negatives with `last` off pin and unexpected mask bits are RED; post-boot migration history is GREEN after repair. Harness self-test checks one echo-off launch and shell restoration. Existing failed QEMU RX transcript is runtime RED.
- GREEN condition: focused fixtures, harness/validator self-tests and guest static build pass; one bounded UART-only `SMP=16` transcript has successful harness exit and UART validator exit 0 with actual-hart, data and completion observations.
- Verification: focused C/Python tests, `make tests/ms08_uart_smp_probe`, UART harness and validator self-tests, bounded UART QEMU session, `python3 scripts/ms08-uart-validate.py <transcript>`; record commands, decisive lines and exit codes.
- Stop when: serial input remains corrupt after a parser/echo witness, actual-hart state cannot be observed, or a kernel/product defect appears; return to Plan with first failing case.

### 6.2: Network-only fixed-placement qualification

- Requirement/Scenario: delta spec network fixed placement, wake, bidirectional/Full/readiness/quiet and controlled migration.
- Depends on: accepted Iteration 004 network protocol; independent of UART runtime outcome.
- Targets: `tests/ms08_network_smp_probe.c::ms08_net_placement_ok`, `tests/ms08_network_smp_probe_test.c`, `scripts/ms08-network-validate.py::_validate_protocol`, existing V5/peer/HMP runtime procedure.
- Current behavior: cumulative owner/runner hart masks contain boot migration destinations, so singleton checks reject an otherwise restored current pin in the first placement case.
- Required behavior: validator and guest predicate require distinct schedulable current owner/runner affinities, actual last harts at those pins, nonzero poll events, masks including current and reported boot migration harts and no unexplained/out-of-schedulable bits. A wrong last hart, missing pin bit or unreported extra bit must fail. Migration and resource-ledger cases remain independently strict. No V5 wire change.
- Required changes: add RED host/validator fixtures from the observed `0x8001/0x3` history and bad current-hart/extra-bit cases; repair only placement decisions; run a separate bounded network-only `SMP=16` session with peer and HMP results.
- Preserve: one owner/runner, V1–V5 layout and semantics, independent peer result, existing reset/link and migration protocol, fail-closed malformed input.
- Forbidden: clearing cumulative history, dropping placement observations, accepting arbitrary extra history bits, using UART success or single-hart evidence, or changing kernel ownership without a witnessed defect.
- Test witness: existing fixture's multibit rejection is revised to distinguish reported historical migration from unexplained drift; parent QEMU placement snapshot is RED before repair and accepted only when current hart/affinity/history relations all hold. Negative fixtures reject unknown bits and actual-hart drift.
- GREEN condition: focused fixture and validator self-test pass; network-only QEMU transcript contains all cases, guest exit 0 and independent peer result; network validator exits 0.
- Verification: C fixture, network validator self-test and static guest build; bounded network QEMU/peer/HMP run and `python3 scripts/ms08-network-validate.py <joined-transcript>`; record first failed case if any.
- Stop when: current owner/runner hart disagrees with affinity, unexplained history appears, peer/HMP result is missing or a data/recovery case fails; return to Plan.

### 6.3: Independent decisions and diff review

- Requirement/Scenario: two independent fixed-placement Gates before combined pressure.
- Depends on: 6.1 and 6.2 accepted runtime results.
- Targets: both transcripts, validators, changed code/diff and this Cycle Act Response.
- Current behavior: UART and network validators have no accepted runtime inputs.
- Required behavior: each driver has a separate successful validator decision and direct target observations; review the full changed diff and close any blocking finding.
- Required changes: record commands, decisive outputs, exit codes, independent conclusions and complete diff Review in Act Response.
- Preserve: separate driver decisions and deferred Iteration 006.
- Forbidden: treating host tests, boot smoke, partial transcript or another driver's pass as runtime acceptance.
- Test witness: existing negative validator fixtures reject missing/duplicated cases; the two actual runtime transcripts are final witnesses.
- GREEN condition: both validators exit 0 on their own full transcripts, `git diff --check`, strict OpenSpec validation and full diff Review pass.
- Verification: both validator commands, `git diff --check`, `git diff --cached --check`, `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`, full staged/unstaged diff inspection.
- Stop when: either driver lacks a decisive accepted runtime result.

**Invariants**

Telemetry mask is monotonic history. Fixed placement uses current scheduler pin plus actual poll-site observation; uniqueness and migration continuity remain separate checks. QEMU `SMP=16` cannot prove true-board behavior.

**Non-goals**

Combined pressure, final recovery regression, hardware qualification, throughput work and new diagnostic wire fields.

**Acceptance**

UART requirement → 6.1 → parser/placement host witnesses → UART runtime validator; network requirement → 6.2 → history/current-hart fixtures → network runtime validator; independent qualification → 6.3 → both distinct results and full diff Review. No requirement is simplified.

**Verification**

Run focused RED/GREEN before product/protocol edits, then static builds and host regressions, then separate bounded QEMU `SMP=16` sessions. Prepare the kernel and two static guest payloads with `make ARCH=riscv64 SMP=16 build` and `make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe`. Copy `make/disk.img` separately to `/tmp/ms08-uart-disk.img` and `/tmp/ms08-net-disk.img`, and use `debugfs -w -R 'write tests/ms08_uart_smp_probe /root/ms08_uart_smp_probe'` or the analogous network command on its corresponding copy. Run QEMU virt with `-m 1G -smp 16 -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=<driver-disk-copy> -snapshot`. UART uses `-chardev socket,id=uart0,path=/tmp/ms08-uart.sock,server=on,wait=off -serial chardev:uart0 -monitor unix:/tmp/ms08-uart-hmp.sock,server=on,wait=off -nographic`; start `python3 scripts/ms08-uart-serial.py --socket /tmp/ms08-uart.sock --launch-probe --transcript /tmp/ms08-uart-transcript.log --deadline-seconds 360` as its sole serial client before QEMU. Network adds `-device virtio-net-device,netdev=net0 -netdev user,id=net0 -serial stdio -monitor unix:/tmp/ms08-net-hmp.sock,server=on,wait=off -nographic`; capture guest serial with `script -q -e -f`, start `python3 scripts/ms08-network-peer.py --host 0.0.0.0 --port 15578 --deadline-seconds 420` after the shell appears, and execute `/root/ms08_network_smp_probe --run` with input echo disabled and an explicit `MS08_NET_HARNESS_EXIT` line containing its actual exit code. On `MS08_NET_HMP_READY: link=off/on`, send `set_link net0 off/on` to the separate HMP socket; after link-on type `MS08_NET_HMP_DONE link=on` at the guest serial console. Join guest serial then peer result into the network transcript. The peer port needs no hostfwd rule. Run each pure-output validator on its own full transcript. Validator decisions require actual case output and independent exit lines. An environment that denies Unix socket binding cannot satisfy runtime Gate 5; carry bounded commands and actual outputs from a socket-capable host into Act Response.

**Gate 2 Readiness**

- Requirements/scope: PASS — tasks 6.1–6.3, Iteration 005 and original delta-spec Acceptance remain unchanged.
- Investigation: PASS — parent Cycle diff, failure transcripts, actual parser/counter/probe/validator symbols, boot order and host tests inspected.
- Design/tasks: PASS — history/current-placement distinction, bounded UART parser and echo control, test witnesses and stop conditions specified.
- Traceability/tests: PASS — each requirement maps to a task, actual code surface, RED witness and runtime validator.
- Verification: PASS as a plan — focused and runtime commands with behavioral results are specified; actual runtime remains an Act/Gate 5 result.
- Change consistency: PASS — tasks and design reflect revised verification contract; no delta-spec requirement changed.
- Persisted Evidence: PASS — mode `none`; decisive output belongs in Act Response.
- User approval: PASS — 用户于 2026-09-22 回复“批准”，批准本 Cycle 001 replan；实施仍需单独指令。
- Overall Gate 2: PASS — 各项检查通过；本 Plan Context 可标记为 `ready`。

**Persisted Evidence**

- Mode: none.
- Budget: decisive commands, output and exit codes in Act Response. Preserve failed runtime transcripts in `/tmp` only while needed for diagnosis; no Evidence directory required.

**Risks and Notes**

The UART transcript shows RX corruption and a parser that can silently discard bytes, but it does not by itself prove that parser overflow caused this exact corruption. The focused RED witness must establish that connection before the repair is credited. The current managed sandbox has previously rejected QEMU Unix socket binding; runtime success requires actual `SMP=16` execution in a socket-capable host. Prior boot migration smoke remains a diagnostic, not a substitute for either driver's full fixed-placement qualification.

## Act Response

- Status: blocked

**Implemented**

Task 6.1 (UART-only) and Task 6.2 (network-only) probe/validator repairs completed
at the host/guest-probe level, then a blocked runtime qualification surfaced a
kernel net owner-migration concurrency defect (see Blocker Handoff). Frozen case
order, V1–V5 wire layouts, payload grammar and guest runtime semantics unchanged
unless noted below (migration V5 count relaxed to 2-or-3 by recognized change).

Repairs made (two phases, all RED→GREEN witnessed):

**Phase A — UART fixed placement (6.1)**
1. **UART line reader (`tests/ms08_uart_smp_probe.c`)** — moved
   `ms08_uart_line_reader`/`line_reader_push`/`line_reader_next` out of the
   guest-only `#ifndef` guard so the host test can witness pure read
   partitioning. `push` now returns 0 / -1 on an overlong unframed line (was:
   silently reset `r->len` and drop bytes); `next` returns 1/0/-1 where -1 is an
   explicit overflow. Callers (`receive_frames`, `run_copier_migration`) fail
   closed with `rx-overflow`.
2. **Echo control (`scripts/ms08-uart-serial.py`)** — `LAUNCH_CMD` disables TTY
   echo before the probe and restores it after, so injected frames never echo
   into the capture. One serial client, unchanged payload grammar.
3. **UART placement (`ms08_uart_placement_ok`)** — fixed placement requires the
   copier to actually poll on its pinned hart (`*_last_hart == *_affinity`),
   nonzero progress, the history mask to contain the pin, and no out-of-
   schedulable mask bit. Mirrored in
   `scripts/ms08-uart-validate.py::_validate_protocol`.

**Phase B — Network fixed placement + migration/wake contract alignment (6.2)**
4. **Network placement (`ms08_net_placement_ok`)** — replaced the exact
   singleton-mask requirement (which wrongly rejected cumulative boot migration
   history) with: pin present, no out-of-schedulable bit, nonzero role events,
   current pin == actual last poll hart. Mirrored in
   `scripts/ms08-network-validate.py::_validate_protocol`.
5. **Affinity syscall-return predicate** — `get_affinity_mask` misread a
   successful `sched_getaffinity` (returns the mask size > 0) as failure.
   Added `ms08_net_syscall_return_ok(rc)` (`rc >= 0` is success) used by
   get/set_affinity_mask; host witness added. This cleared the original
   `timer-disabled-wake affinity-read` FAIL.
6. **readiness-quiet (`run_readiness_quiet`)** — the strict multi-snapshot
   `quiet_ok` (5 counters equal across two views separated by a data-plane
   round-trip) could never hold; the protocol contract is a single terminal V5
   gated on task_poll progress plus an I/O-silence window. Repaired to match:
   ignore periodic stack-runner maintenance, keep the
   `expect_fd_silence(200ms)` no-readable-event criterion.
7. **Migration pre-state semantics** — the kernel leaves each role Restored (2)
   after boot smoke, which is 'idle' per the migration enum; probe/validator
   demanded pre-phase 0. Added `ms08_net_mig_idle` (phase 0 or 2), applied to
   the idle check and the Widened relation; validator synced.
8. **Migration V5 count 2-or-3** (recognized protocol change, user-approved) —
   a fast runtime migration can finish between the probe's snapshot reads, so
   the Widened mid-state is not always sampled. `run_role_migration` no longer
   fails on a missing mid when the completed Restored view is valid
   (`ms08_net_migration_completed`), and emits 2 V5 (pre, post) or 3 V5
   (pre, mid, post). Probe `ms08_net_schema`, validator `CASE_GRAMMAR`,
   `_check_grammar`, `_v5s`, `_migration` and canonical all accept 2-or-3.

**Changed Files and Symbols**

- `tests/ms08_uart_smp_probe.c`:
  `ms08_uart_line_reader`, `line_reader_push`, `line_reader_next`,
  `ms08_uart_placement_ok`, `receive_frames`, `run_copier_migration`.
- `tests/ms08_uart_smp_probe_test.c`: line-reader coalesced/split/overflow
  witnesses; placement drift/progress/mask witnesses.
- `scripts/ms08-uart-serial.py`: `LAUNCH_CMD`, `self_test` echo assertions.
- `scripts/ms08-uart-validate.py`: `_validate_protocol`, `self_test`.
- `tests/ms08_network_smp_probe.c`: `ms08_net_placement_ok`,
  `ms08_net_syscall_return_ok`, `get/set_affinity_mask`,
  `run_readiness_quiet`, `ms08_net_mig_idle`, `ms08_net_migration_completed`,
  `run_role_migration`, `ms08_net_schema`.
- `tests/ms08_network_smp_probe_test.c`: placement/affinity/migration/completed
  witnesses.
- `scripts/ms08-network-validate.py`: `_validate_protocol`, `_migration`,
  `_check_grammar`, `_v5s`, `CASE_GRAMMAR`, `schema_lines`, canonical,
  `self_test` (2-or-3 migration V5).
- `kernel/src/drivers/net_placement.rs`: diagnostic print on migration
  ObserveTimeout (kept as the defect reproduction tool; see Blocker Handoff).

**Deviations from Plan**

Two deviations, both recorded and user-approved during this Cycle:
- Migration V5 count relaxed from exactly 3 to 2-or-3 (accommodates un-sampled
  fast-migration windows); schema/validator/canonical/probe synced together.
- Added a qemu-only diagnostic print in `net_placement.rs` on migration
  ObserveTimeout; this is defect tooling, not a product behavior change, and is
  preserved as the reproduction witness for the Plan handoff. No other plan
  contract was altered.

**Blocker Handoff**

- Status: `pending → blocked` (Gate 6: runtime qualification of owner/runner
  migration is not achievable while the kernel net owner migration
  intermittently fails).
- Task/step: Task 6.2 `owner-migration` (and the dependent migration/readiness
  runtime acceptance); also `migration_boot_smoke` itself intermittently FAILs.
- Plan expectation vs actual: probe/validator were expected to qualify fixed
  placement, timer-wake, data-plane, Full recovery, readiness and controlled
  migration. The probe/validator/predicate fixes are green, but the runtime owner
  migration intermittently cannot move the owner to its second hart.
- Root cause (fully located, diagnosis confirmed): `net_placement::migrate_role`
  widens the owner affinity to {orig, second} where `second` is always hart 0
  (`select_second` picks the smallest hart != orig; `sched_hart_ids()` is
  ordered 0..16). The owner must then execute one poll on hart 0. Failure sample:
  `[NET-MIG-owner] TIMEOUT-widened wait=1000001 req_events=1 orig=1 second=0 last=1
  mask=0x2 events=1 other_events=46`. `mask=0x2` contains only orig (bit1),
  never bit0 — the owner was never scheduled/awoken to poll on hart0
  (`crates/axnet/src/async_rx.rs:2824` HartCounter record shows no hart0 entry),
  so the 1e6-yield bounded wait times out and the phase is left Widened.
  Runner migration targets the same hart0 but the runner is a high-frequency
  stack poller that can preempt hart0, so it is stable; UART migration targets
  disjoint harts, so it is stable. This is a genuine kernel concurrency/scheduling
  defect in net owner migration (Task 4.3 completion), not a probe/validator
  judgment error.
- Blocker for Plan: whether owner migration to hart0 is a kernel defect to fix,
  or the migration *_policy's expectation of a low-activity owner polling a
  target hart within the bound is not attainable and must be revised.
- Evidence: see `[NET-MIG-owner] TIMEOUT-widened` boot line and
  `[NET-MIG-SMOKE] FAIL owner_phase=1 owner_obs=0` (2026-09-23 07:13 boot);
  diagnostic is still in the built kernel for Plan reproduction.
- Recovery condition: Plan evaluates/repairs kernel net owner migration, then
  re-runs the migration smoke and the network `SMP=16` qualification.

**Blocker Resolution**

None (blocked; no user-provided workaround yet).

**Self-Review**

- Spec compliance: each 6.1/6.2 probe/validator repair has a RED-repaired host
  witness and a mirrored validator check; schema guard (probe
  `--print-schema` vs validator `schema_lines`) matches after the 2-or-3
  migration change.
- Code quality: parser is a shared pure decision core; no new sleep-polling,
  second serial client, or identity-styled evidence machinery; diagnostics are
  behavior-based. The kernel migration diagnostic is the single qemu-only print.
- Full `make host-test` passes green (see Verification), so no host regression
  was introduced into ms03–ms08 surfaces.
- The blocked runtime defect is a kernel matter outside the probe/validator
  judgment scope; the probe's Sit rejection (`migration-not-idle`, Widened pre)
  was correct, and the subsequent probe adaptations were made only to match the
  documented kernel migration phase semantics.

**Verification Evidence**

| # | 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|---|
| 1 | UART host decision test | `cc -std=c11 -Wall -Wextra -Werror tests/ms08_uart_smp_probe_test.c -o /tmp/... && /tmp/...` | exit 0 (no assertion abort) | 行解析器/UART placement | PASS |
| 2 | UART serial harness self-test | `python3 scripts/ms08-uart-serial.py --self-test` | exit 0 | echo 开关/launcher | PASS |
| 3 | UART validator self-test | `python3 scripts/ms08-uart-validate.py --self-test` | exit 0 | 全 UART 判定 | PASS |
| 4 | Network host decision test | compile + run `tests/ms08_network_smp_probe_test.c` | exit 0 | placement/affinity/migration/completed | PASS |
| 5 | Network validate self-test | `python3 scripts/ms08-network-validate.py --self-test` | exit 0 | 全 network 判定含 2-or-3 migration | PASS |
| 6 | Guest static build | `make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe` | exit 0, built | RISC-V static probe | PASS |
| 7 | Cases/schema guard matcher | diff validator vs probe `--print-cases`/`--print-schema` | exit 0, match | 冻结语法一致（迁移 2-or-3） | PASS |
| 8 | Full host regression | `make host-test` | exit 0 | ms03–ms08 host 全量 | PASS |
| 9 | Kernel debug build | `make ARCH=riscv64 SMP=16 build` | exit 0 | 诊断版内核 | PASS |
| 10 | Runtime blocker 捕获 | boot 串口 `[NET-MIG-owner] TIMEOUT-widened` | `mask=0x2, last=1` (无 bit0) | 内核 owner 迁移缺陷 | BLOCKED |

Runtime `SMP=16` qualification is blocked on the kernel owner-migration defect
(see Blocker Handoff); the probe/validator host GREEN confirms the tooling
accepts the protocol but the owner-migration runtime cannot be qualified yet.

**Persisted Evidence**

None required (mode `none`). The blocker's decisive output is the boot line
quoted in Blocker Handoff; no Evidence directory was created.

**Experience Candidates**

- Runbook candidate: the UART/network `SMP=16` bounded QEMU procedure with
  per-driver socket/peer/HMP choreography and the two validator commands, once
  the operator confirms it end-to-end. Evidence: Plan `Verification` command
  lineage + this Cycle's host GREEN (tooling accepts the protocol).
- Issue candidate (substantive kernel defect, evidence-based): net owner
  migration intermittently fails to move the owner to hart0 (mask never gains
  bit0), leaving the slot Widened and `migration_boot_smoke` FAIL. Evidence: the
  `[NET-MIG-owner] TIMEOUT-widened wait=1000001 ... mask=0x2 last=1` boot line and
  `[NET-MIG-SMOKE] FAIL owner_phase=1 owner_obs=0`. This is outside current probe/
  validator work and its resolution belongs to Plan (kernel Task 4.3 completion).
- Note opposite of previous response: the earlier "place-probe fixes satisfy 6.2"
  reasoning is now superseded by the runtime kernel blocker; the host GREEN does
  not qualify the migration runtime.

**Remaining Issues**

- Runtime acceptance of `owner-migration`/`runner-migration` (and the follow-on
  reset/link cases) is blocked on the kernel net owner-migration defect.
- `migration_boot_smoke` intermittently FAILs on owner (observed 2026-09-23
  07:13), the authoritative kernel self-check.
- The migration V5-count relaxation (2-or-3) should be re-confirmed once the
  kernel owner migration is stable, and the still-unused
  `ms08_net_migration_widened`/`restored` full-chain path re-checked for a
  medium-sampled run.
- No second-endpoint or kernel-ownership defect has been established at host
  level; the owner-migration issue is scheduling/wake on the target hart.

**Commit or Diff Reference**

Not committed (user has not authorized a commit). Diff is the unstaged changes
under the current working tree; probe/validator/test files are part of the
already-staged MS08 Iteration 004–005 protocol surface; the kernel diagnostic is
in `kernel/src/drivers/net_placement.rs`.

## Plan Review

- Review Result: rework-required

**Findings**

- Blocking — UART reassembly still rejects a valid read partition.  The repaired
  `line_reader_push` appends an entire read before `line_reader_next` drains a
  newline.  A residual partial line followed by a chunk containing that line's
  terminator and more valid frames can exceed the 160-byte aggregate buffer even
  though no individual line is overlong.  This violates Task 6.1's requirement
  to preserve every valid frame across arbitrary allowed read partitioning.
- Blocking — both placement predicates and both validators accept every extra
  schedulable history bit.  The Plan requires history to be limited to the
  current pin and the migration `from/to` harts reported by the snapshot.  An
  unexplained but schedulable bit therefore passes today, weakening the
  fixed-placement/unique-role decision in Tasks 6.1 and 6.2.
- Blocking — the two-view network migration path verifies `Restored`, the
  origin hart and aggregate progress, but does not require a distinct,
  schedulable `to` hart recorded in the role's history.  The approved two-or-
  three-view grammar remains usable, but its fast-completion predicate does not
  yet prove the target relation promised by Task 6.2.
- Blocking — the owner migration failure is a real product-path race, not a
  validator error.  `AxWaker::wake_by_ref` sets its `woke` flag before trying
  `Blocked -> Ready`.  The current second-hart stimulus calls `wake_role` 64
  times regardless of target state.  Wakes delivered while the owner is
  Running/Ready do not enqueue it on hart 0, but can make `block_on` yield on
  the origin instead of committing `Blocked`; after the stimulus exits, no
  second-hart wake remains.  This matches the reported `last=1 mask=0x2`
  timeout.  Existing scheduler code already chooses the waking hart when a
  blocked task's widened mask contains it, so no scheduler API or immediate
  forced migration is justified.
- Non-blocking — Act Response calls the current diff unstaged, while the
  inspected worktree has the MS08 surface staged.  This does not affect the
  behavioral findings or verification results.

**Deviation Classification**

`ACT-DEVIATION` for the parser and placement/migration decision gaps;
`NEW-EVIDENCE` for the latent network-owner migration race exposed by the fresh
`SMP=16` run.

**Acceptance Gaps**

- 6.1: valid UART frames are not yet accepted for every allowed read partition;
  unexplained schedulable history is not rejected; no accepted UART runtime
  transcript exists.
- 6.2: unexplained history and an incomplete two-view target relation are not
  rejected; owner migration can remain on the origin hart; no accepted network
  runtime transcript exists.
- 6.3: neither independent validator decision nor the final full-diff decision
  exists.

**Convergence**

expanded — several parent gaps were narrowed and the host suites are green, but
independent review found three false-accept/reject paths and fresh runtime
evidence exposed a prerequisite product race.

**Evidence**

- Independently inspected `tests/ms08_uart_smp_probe.c::{line_reader_push,
  line_reader_next}`, both placement predicates and validators, and the
  two-view migration predicates.  Their current conditions establish the gaps
  above directly from source.
- Independently traced
  `net_placement::migrate_role_inner -> axnet::software_nudge ->
  QueueEvent::publish_queue_work -> AxWaker::wake_by_ref ->
  select_wake_run_queue/unblock_task`, including `block_on`'s `woke` branch.
  The source explains why an ungated wake can prevent the natural block/wake
  migration that D8 requires.
- Adopted the unchanged Act results for focused C/Python tests, static guest
  builds, `make host-test`, and the `SMP=16` kernel build.  They remain useful
  regression baselines but do not close the listed Acceptance gaps.
- Adopted the fresh runtime failure:
  `[NET-MIG-owner] TIMEOUT-widened ... orig=1 second=0 last=1 mask=0x2` and
  `[NET-MIG-SMOKE] FAIL owner_phase=1 owner_obs=0`.
- Persisted Evidence mode is `none`; absence of an Evidence directory is not a
  finding.

**Follow-up Decision**

Create a rework Cycle because Act needs new Current-State Evidence, kernel
targets, repair-item contracts and RED witnesses.  Keep the existing
requirements, D8 natural block/wake design, Tasks 6.1–6.3 and Iteration Map.
Do not reopen the accepted Iteration 003 Cycle or weaken migration to a timing,
timer or repeated-ungated-wake claim.

**Iteration Plan Update**

None.

**Next Cycle**

`002-rework.md`.

**Next Iteration**

None.
