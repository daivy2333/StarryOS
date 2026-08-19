# Iteration 011 / Cycle 005: Reject Out-of-Range DONE Counts

## Plan Context

- Status: ready
- Iteration: 011-independent-manual-qemu-runtime-and-closeout-review
- Cycle: 005-rework
- Cycle Type: rework
- Parent cycle: `004-rework.md`

**Iteration Scope**

- Change tasks: 6.2, 6.3
- Stable baseline: Cycle 004's six accepted single-hart QEMU results and the
  user's recorded Evidence/compatibility waiver
- Verification boundary: DONE parsing and its focused host-side tests
- Deferred tasks: final task/document synchronization and archive decision

**Cycle Scope**

- Trigger: Cycle 004 independent Review.
- Acceptance gap: a DONE count can fit `unsigned long` but overflow or wrap
  when narrowed to `int`; `4294967392` currently becomes `96`.
- Repair item: 6.2-R8.
- Excluded scope: registration, data path, queue/driver/kernel changes, wire
  format changes, new dependencies, automatic capture, QEMU reruns,
  compatibility reruns, global documentation updates and archive.

**Objective**

Reject DONE counts outside the probe's existing `1..4096` protocol bound
before narrowing or ACK, then perform a focused final code Review.

**Current-State Evidence**

- `udp_done_recv()` validates `strtoul()` syntax and `ERANGE`, then returns
  `(int)count` without a range check.
- The probe's command-line contract already limits count to `1..4096`.
- A fresh seam witness using `4294967392` returns `96`; the invalid message can
  therefore masquerade as the normal 96-packet completion.

**Relevant Code**

- `tests/ms05_data_plane_probe.c::{udp_done_recv,udp_sent_done}`
- `tests/ms05_data_plane_probe_test.c`

**Behavioral Change**

- Exact DONE counts from 1 through 4096 remain valid.
- Zero, values above 4096, malformed values and numeric overflow are rejected
  before conversion and before ACK.
- No product kernel, driver, queue or wire behavior changes.

**Task Contract**

### 6.2-R8: Enforce the existing DONE count bound

- Requirement/Scenario: R6/R14; an invalid completion must not produce ACK or
  PASS.
- Depends on: Cycle 004 registration and exact-token repairs.
- Targets: `udp_done_recv()` and the existing C probe test harness.
- Required behavior: after complete numeric parsing, require the count to be
  within `1..4096` before any narrowing conversion; reject otherwise.
- Preserve: exact four-token grammar, mode match, bounded receive, return type,
  ACK text for valid counts and all existing valid-path behavior.
- Forbidden: changing the protocol, public API, host stimulus, QEMU workflow,
  kernel/driver code or introducing a new abstraction/dependency.
- RED witnesses: `4294967392` must fail instead of returning 96; zero and 4097
  must fail. Keep exact 96 and boundary 4096 as valid witnesses.
- GREEN condition: all new and existing DONE-parser cases pass, the complete C
  harness passes, and strict host/RISC-V compilation succeeds.
- Verification: focused C harness, strict C syntax/build probes, diff Review
  of the two allowed files, then Plan Review.
- Stop when: the repair requires a wire/API change or touches the product data
  path.

**Invariants**

- ACK is emitted only for an exact valid DONE from the registered peer.
- The existing count ceiling remains 4096.
- Cycle 004 runtime results and user waivers are not reopened by this local
  parser correction.

**Non-goals**

- No new runtime behavior, feature, hardening sweep or Evidence pipeline work.
- No repeated QEMU, hash audit, automatic capture or compatibility batch.
- No archive or project-wide documentation maintenance in Act.

**Acceptance and Traceability**

| Acceptance | Repair | Witness | Status |
|---|---|---|---|
| Out-of-range DONE cannot wrap into a valid count | 6.2-R8 | 4294967392 seam case | Covered |
| Existing protocol bounds are enforced | 6.2-R8 | 0, 4096 and 4097 cases | Covered |
| Valid completion still produces the shared count | 6.2-R8 | exact 96 case | Covered |

**Verification**

1. Add and run the narrowing/boundary cases as RED.
2. Add the smallest range check before conversion.
3. Run the full existing C harness and strict compilation probes GREEN.
4. Review the focused diff and return to Plan Review.

No Python, network, QEMU or automatic-pipeline rerun is required because this
repair changes only rejection of an invalid control count.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | Deterministic seam witness reproduces wrap-to-96. |
| Design | PASS | Existing `1..4096` bound removes the narrowing ambiguity. |
| Iteration Plan | PASS | One local repair remains within Tasks 6.2-6.3. |
| Cycle count policy | WAIVED | User authorized repeated test-focused Cycles in this Iteration. |
| Task Contract | PASS | Target, RED/GREEN, preserve and stop rules are explicit. |
| Traceability | PASS | The repair closes Cycle 004's remaining R6/R14 gap. |
| Verification | PASS | Focused deterministic tests are sufficient. |
| User authorization | PASS | User requested audit and closeout; prior waiver permits this Cycle. |

**Persisted Evidence**

- Mode: none
- Act Response command/output summaries are sufficient for this local parser
  boundary; do not create another Evidence package.

**Risks and Notes**

- Check the range while the value is still `unsigned long`; checking after the
  cast would preserve the defect.
- Do not broaden this into generic parser hardening.

## Act Response

- Status: reported

**Implemented**

- Added the protocol bound check in `udp_done_recv()`: after complete numeric
  parsing (`strtoul` with `ERANGE`/endptr checks), a DONE count outside
  `1..4096` is rejected before the `(int)` narrowing conversion. This closes the
  6.2-R8 gap where `4294967392` (= 2^32 + 96) fit `unsigned long` but wrapped to
  a valid-looking `96` when narrowed, letting an out-of-range completion
  masquerade as the normal 96-packet result.
- Added RED-witness tests covering the wrap-into-valid case (`4294967392` must
  fail), zero, the 4097 over-bound case, and preserved both the exact `96`
  result and the `4096` upper boundary as valid.

**Changed Files and Symbols**

- `tests/ms05_data_plane_probe.c`: `udp_done_recv()` — range guard before the
  `(int)` narrowing.
- `tests/ms05_data_plane_probe_test.c`: `test_udp_done_rejects_wrap_into_valid`,
  `test_udp_done_rejects_zero`, `test_udp_done_rejects_above_max`,
  `test_udp_done_accepts_max_boundary` + registration in `main` (5 → 9
  done-parsing cases).

**Deviations from Plan**

None. The repair matches the 6.2-R8 Task Contract exactly; only the two allowed
files were modified.

**Blocker Handoff**

None.

**Self-Review**

- Spec compliance: requirement (reject outside `1..4096` before narrowing),
  targets (only `udp_done_recv` + existing C harness), preserved grammar/mode/
  return type/ACK for valid counts, RED and valid witnesses all satisfied; no
  forbidden change (no protocol/API/kernel/QEMU change, no new abstraction).
- Code quality: range checked while still `unsigned long` (per the Plan's risk
  note — checking after the cast would preserve the defect); diff confined to
  the two allowed files; no new warnings; no Critical/Important finding.
- Full-diff review across the Cycle confirms only the two source files changed.

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| C harness (RED then GREEN) | `cc -std=c11 -Wall -Wextra -Werror tests/ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test && /tmp/ms05-data-plane-probe-test` | RED: `Assertion ... != 96' failed` at `test_udp_done_rejects_wrap_into_valid`; after fix: `ms05 probe seam tests: 9 done-parsing passed`, RUN_EXIT=0 | PASS |
| Strict C syntax probe | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c` | exit 0 | PASS |
| Static RISC-V payload build | `riscv64-linux-musl-gcc -std=c11 -Wall -Wextra -Werror -static -no-pie -Os -o /tmp/ms05_data_plane_probe tests/ms05_data_plane_probe.c` | exit 0 | PASS |
| OpenSpec change validation | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane` | `Change ... is valid`, exit 0 | PASS |

**Persisted Evidence**

None required — Cycle Persisted Evidence mode is `none`; command/output
summaries above are sufficient for this local parser boundary.

**Experience Candidates**

None. This is a one-time local parser guard fix; no end-to-end path or
repeated/high-risk operation that would qualify as a Runbook or Incident
candidate.

**Remaining Issues**

None.

**Commit or Diff Reference**

Working-tree diff of the two allowed files:
`tests/ms05_data_plane_probe.c` (+4 lines) and
`tests/ms05_data_plane_probe_test.c` (+32 lines) only; no other file touched by
this Cycle.

## Plan Review

- Status: reviewed

**Review Result**

Accepted. Repair 6.2-R8 satisfies the remaining DONE-parser Acceptance gap.
The change's planned implementation and verification work is complete under
the Evidence and compatibility waivers recorded in Cycle 004.

**Findings**

- No blocking finding.
- `udp_done_recv()` now checks the parsed `unsigned long` against the existing
  `1..4096` protocol bound before converting it to `int`; invalid values cannot
  wrap into a valid ACK count.
- The focused diff contains only the planned parser guard and four boundary
  tests. It does not change the wire format, host stimulus, kernel, driver,
  queue ownership or QEMU workflow.

**Deviation Classification**

None for Cycle 005. The current managed sandbox terminates the RISC-V compiler
with `SIGSYS`; this is an environment limitation, not an implementation
deviation. Act's ordinary-environment static build exited zero.

**Acceptance Gaps**

None.

**Convergence**

Complete. The deterministic wrap-to-96 gap is closed, all Cycle 005
Acceptance is satisfied, and no further Cycle or Iteration is justified.

**Evidence**

- Fresh host harness: 22 decision, 18 seam and 9 DONE-parser tests PASS; exit
  0.
- Fresh strict host C syntax: exit 0.
- Act static RISC-V payload build: exit 0.
- Fresh strict OpenSpec validation: `Change
  'ms05-qemu-bounded-bidirectional-device-data-plane' is valid`; exit 0.
- Focused `git diff --check`: exit 0.
- Code Review confirms zero and values above 4096 return `-1`, while 96 and
  4096 remain valid.

**Follow-up Decision**

Implementation complete. Hand the accepted change to
`openspec-docs-maintainer` for Tasks 6.1-6.3, SNAPSHOT/reference and change
closeout synchronization; archive only after that state is consistent.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

None.
