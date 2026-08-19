# Iteration 010 / Cycle 002: Host Boundary and Complete Record Audit

## Plan Context

- Status: ready
- Iteration: 010-deadline-closed-probe-and-evidence-pipeline
- Cycle: 002-rework
- Cycle Type: rework
- Parent cycle: `001-rework.md`

**Iteration Scope**

- Change tasks: 5.3, 5.4
- Depends on: accepted protocol/traffic/ledger behavior and Cycle 001's C
  deadline, producer, time-order, index and R44 repairs
- Stable baseline: guest and host operations reject exhausted budgets before
  starting I/O, and every manifest record is bound to content-sensitive source
  identity and independently audited raw output
- Verification boundary: focused RED→GREEN witnesses plus temporary full
  qualification; exact R44 environment blocks remain explicit handoffs
- Diagnostic boundary: Python timeout→I/O sequencing, Git worktree identity and
  artifact-record audit only
- Deferred tasks: 6.1-6.3

**Cycle Scope**

- Trigger: rework-required Review of Cycle 001
- Acceptance gaps: A3 host setter boundary; A5 content-sensitive worktree and
  artifact-record audit; A6/A7 missing damaged-record fixtures and unresolved
  Important findings
- Repair items: 5.3-R3, 5.4-R2
- Inherited scope: R6, R14, D9-D11, Tasks 5.3-5.4, six modes, existing ABI/wire,
  Gate order, six artifacts and the user's Evidence-retention waiver
- Excluded scope: C/kernel/driver/ABI/wire changes, manual QEMU, Iteration 011,
  performance, Runbook updates and global OpenSpec synchronization

**Objective**

Prevent Python host I/O after timeout setup exhausts the exchange deadline.
Make worktree identity sensitive to actual tracked and relevant untracked
content, and require the audit to validate every artifact command record rather
than only the derived artifact entries.

**Background**

Cycle 001 closed most Cycle 000 findings. Independent Review reproduced one
late Python send, showed that porcelain status is not a content identity, and
found that the audit ignores extra artifact command records. These are existing
D11 Acceptance gaps, not new scope. A fresh `make host-test` also hit the known
R44 EPERM boundary at MS04 UDP loopback after earlier harnesses passed.

**Current Baseline**

- Revision: `8dc3ef7d63da00c1966e9cb70820c337494d3c57` on `net-k3` with staged
  change-owned source and OpenSpec files.
- C repair baseline: syntax and 22+18 harness tests PASS.
- Python and Evidence self-tests PASS but do not cover timeout-setter elapsed
  time or artifact-record mutation.
- The full host suite is currently R44-blocked at MS04 loopback socket creation
  with EPERM; this does not waive focused product tests.
- Persisted Evidence remains `none` under the user's large-file retention
  waiver. Cycle 000's artifact index is historical and contains the pre-repair
  MS05 payload hash.

**Current-State Evidence**

- `send_bounded()` and nested `bounded_recv()` compute remaining time and call
  `settimeout()`, but do not re-read the clock before `sendto()`/`recvfrom()`.
  A fake setter reaching the deadline still produces one send call.
- `source_identity()` hashes `git ls-files --stage` for the index and
  `git status --porcelain=v1` for the worktree. The latter does not encode the
  bytes of an already-modified tracked file.
- `audit_manifest()` iterates `GATES` and validates artifact identity entries,
  but extra artifact command records are only stored in `by_id`; no schema,
  log or hash validation is applied to them.

**Relevant Code**

| Area | Files / symbols | Current responsibility |
|---|---|---|
| Host deadline | `scripts/ms05_data_plane_stimulus.py::send_bounded`, `_serve_exchange.bounded_recv` | timeout and host I/O ordering |
| Host tests | stimulus fake sockets and `self_test` | deterministic exchange histories |
| Source freeze | `scripts/ms05_evidence_capture.py::source_identity/freeze_source` | index/worktree identity |
| Artifact records | capture `run_artifact_records`; audit `audit_manifest/audit_record/audit_artifacts` | literal artifact command provenance |
| Fixtures | `scripts/ms05_evidence_audit.py::run_fixtures`; `tests/test_ms05_evidence_tools.py` | exact-code damaged-manifest rejection |

**Critical Path**

```text
clock -> positive remainder -> settimeout
  -> fresh clock/remainder check
  -> sendto or recvfrom
  -> postcheck -> no next operation after equal/late return

freeze selected files + index bytes + worktree diff/untracked content
  -> run required and artifact commands
  -> enumerate exact artifact record set
  -> audit every record's argv/time/exit/classification/log/hash
  -> audit derived artifact identity and producer
```

**Implementation Guidance**

Extend the existing Python fake socket with independently controlled timeout
setter delay and call counters; drive the real exchange functions. Keep the
outer `serve_once()` timeout restoration as cleanup and do not let it create
protocol progress.

Use read-only Git output that contains content, not only status categories.
A worktree identity may hash the binary unstaged diff plus deterministic
untracked path/content entries; selected frozen source hashes remain a second
check. Fixtures must use a temporary repository or an equivalent isolated
identity input and must not mutate the live index/worktree.

Declare the exact artifact command record IDs or derive them deterministically
from the six artifacts and three commands. Reject missing, duplicate, extra or
damaged artifact records before accepting derived artifact entries.

**Behavioral Change**

- Python timeout setup that reaches/regresses past the deadline prevents the
  associated send/receive from starting.
- Changing tracked worktree bytes invalidates identity even when porcelain
  status text is unchanged; relevant untracked content is also bound.
- Every artifact `file`, `stat` and `sha256sum` record is audited for literal
  argv, ordered time, exit/classification, nonempty raw log and hash.

**Change Surface**

| Repair | Requirement/Scenario | File/Symbol | Planned Change |
|---|---|---|---|
| 5.3-R3 | R6/D11 host deadline | stimulus bounded send/receive + fakes | add post-setter precheck and late-setter witnesses |
| 5.4-R2 | R14/D10/D11 provenance | capture source identity; audit manifest/fixtures | bind content and audit exact artifact record set |

**Task Contracts**

### 5.3-R3: No host I/O after late timeout setup

- Requirement/Scenario: R6 fixed deadline and D11 identical pre/post rule for
  Python `sendto`/`recvfrom`.
- Depends on: Cycle 001 timeout clamp and receive postcheck.
- Targets: `scripts/ms05_data_plane_stimulus.py` self-contained host path and
  self-tests.
- Current behavior: `settimeout()` can consume the final budget, after which
  the host still starts `sendto()` or `recvfrom()`.
- Required behavior: re-read the clock after timeout setup and before I/O;
  zero/equal/regressed remainder fails without invoking I/O. Valid sends and
  receives retain their clamped timeout and postcheck. Cleanup restoration does
  not count as protocol progress.
- Preserve: peer, sequence/count/payload, message order, error text stability
  where already asserted, delayed READY/data/DONE and one absolute deadline.
- Forbidden: deadline renewal, per-packet lifetime, protocol/mode changes or
  swallowing socket timeout/errors.
- Test witness: setter-delay fakes make `settimeout()` land exactly at and past
  the deadline for both send and receive; current code must RED with nonzero I/O
  call counts. Also cover affordable setter delay and no-next-side-effect.
- GREEN condition: late histories invoke zero send/receive calls; valid history
  completes under the original deadline; all prior stimulus tests pass.
- Verification: stimulus self-test, loopback attempt, Evidence-tool tests and
  `make host-test` up to PASS or exact R44 handoff.
- Stop when: closure requires guest or wire changes; return to Plan.

### 5.4-R2: Content-sensitive freeze and exhaustive artifact-record audit

- Requirement/Scenario: R14 automatic Gate and D10/D11 source/log/artifact
  provenance.
- Depends on: Cycle 001 shared capture primitive, producer map and time order.
- Targets: `scripts/ms05_evidence_capture.py`,
  `scripts/ms05_evidence_audit.py`, `tests/test_ms05_evidence_tools.py`.
- Current behavior: worktree identity hashes status text; artifact command
  records are not traversed by the audit.
- Required behavior:
  - worktree identity changes when tracked worktree bytes change without a
    status-category change, and binds relevant untracked paths plus content;
  - identity generation remains read-only and deterministic;
  - audit derives the exact 18 artifact record IDs/argv and rejects missing,
    duplicate, unexpected or damaged records;
  - every artifact record passes the common record checks and has exit 0,
    `pass` classification, ordered timestamps, nonempty hash-matching log and
    cwd consistent with the qualification root contract;
  - fixtures isolate actual worktree-content drift and artifact record
    removal/argv/log/hash/classification damage, each with a stable exact code.
- Preserve: required Gate order, six artifacts and producers, D1/R44 behavior,
  100 child records and qualification binding.
- Forbidden: hashing porcelain status as the sole content identity, mutating
  the live Git state in fixtures, accepting arbitrary extra artifact records
  or regenerating Cycle 000 Evidence as current authority.
- Test witness: RED an isolated edit that leaves porcelain status unchanged;
  RED missing and tampered artifact command records that the current audit
  accepts. A valid temporary full manifest remains GREEN.
- GREEN condition: all exact-code fixtures and temporary full qualification
  pass; any tracked-byte or audited artifact-record mutation fails.
- Verification: Evidence unittests, capture/audit self-tests, temporary full
  capture→audit→qualification→verify, strict OpenSpec and both diff checks.
- Stop when: content identity requires a mutating Git command, a literal record
  cannot be captured, or a product Gate fails; return to Plan.

**Invariants**

- Production and fake host paths use the same exchange functions.
- No host operational I/O starts with zero budget or succeeds at/after the
  original absolute deadline.
- Guest C repairs, Hold cleanup, six modes, kernel/driver and ABI/wire remain
  unchanged.
- QEMU results remain separate from hardware/SMP evidence.
- The user waiver covers persistent large Evidence, not code or audit gaps.

**Non-goals**

- C probe changes, kernel/driver changes, manual QEMU, Iteration 011 execution,
  performance, historical Evidence rewrite, Runbook or global state updates.

**Acceptance**

| Contract | Proof | Status |
|---|---|---|
| A3 | late Python timeout setter causes zero associated I/O calls | Planned |
| A5 | same-status content edit changes worktree identity | Planned |
| A6 | exact artifact record set and every record field/log are audited | Planned |
| A7 | focused/full temporary Gates have no product failure or unresolved Important finding | Planned |

**Requirements Traceability Matrix**

| Requirement / Scenario | Design | Repair | Code surface | Test witness | Status |
|---|---|---|---|---|---|
| host fixed deadline | D11 | 5.3-R3 | Python send/receive | timeout-setter delay + zero I/O count | Covered |
| content-sensitive worktree | D11 | 5.4-R2 | capture identity | isolated same-status byte edit | Covered |
| complete artifact records | D10/D11 | 5.4-R2 | capture/audit/fixtures | missing/tampered exact records | Covered |

No requirement is Missing or Simplified. Large temporary qualification
carriers remain non-persistent by explicit user waiver.

**Verification**

Run the focused Python RED/GREEN and Evidence fixtures first, then the existing
C regression, temporary full qualification, artifact re-read generated by that
temporary run, strict OpenSpec and both diff checks. Run `make host-test`; if
MS04/MS05 UDP socket creation is denied, record the unchanged argv, exit and
earliest EPERM layer in the Act Response rather than calling it PASS.

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | late Python send reproduced; identity and audit traversal inspected |
| Design | PASS | post-setter check and content/record binding preserve D10/D11 |
| Iteration Plan | PASS | repairs remain in Iteration 010; 6.1-6.3 stay deferred |
| Cycle Scope | PASS | two repair items map only to residual Cycle 001 gaps |
| Task Contracts | PASS | targets, RED/GREEN, verification and stop rules are explicit |
| Traceability | PASS | R6/R14 → D10/D11 → Tasks 5.3/5.4 → repairs/tests |
| Verification | PASS | focused witnesses precede temporary full qualification |
| Evidence retention | WAIVED | user approved non-retention of large manifest/raw logs after review |

**Persisted Evidence**

- Mode: none

Use a temporary qualification root. Record commands, exits, record/child counts,
artifact hashes, exact R44 handoff and full Review in the Act Response. If a
product failure occurs, keep the temporary root until Plan reviews the blocker;
otherwise remove it without creating a change-local Evidence placeholder.

**Risks and Notes**

- `git status` remains useful metadata but is not a content identity.
- Artifact record IDs must be deterministic and collision-free across six
  artifacts and three commands.
- The current sandbox's EPERM result is expected environment variability; it
  cannot hide a focused test or audit failure.
- This is the second rework Cycle. If the same host/provenance gap does not
  shrink after Act, Plan must re-check D11 assumptions before another Cycle.

## Act Response

- Status: reported

**Implemented**

Repair 5.3-R3 — no host I/O after late timeout setup:
`scripts/ms05_data_plane_stimulus.py::send_bounded()` and the nested
`_serve_exchange.bounded_recv()` now re-read the clock after the
`settimeout()` clamp and before the blocking `sendto()`/`recvfrom()`; an
equal or regressed remainder raises `"exchange deadline exceeded before
send/receive"` without invoking the I/O. Valid operations keep the clamped
timeout and the existing after-I/O postcheck; the `serve_once()` finally
cleanup restoration still does not count as protocol progress. A new
`SetterDelaySocket` fake advances the fake clock by one configured delay per
`settimeout()` call with send/recv call counters, driving five witnesses:
send setter landing exactly at the deadline, send setter past the deadline,
receive setter exactly at the deadline, receive setter past the deadline
(all assert zero I/O calls and an empty outgoing queue), and an affordable
0.05 s setter delay that must still complete the full 96-datagram exchange
under the original absolute deadline. Pre-fix the late send witness observed
`send_calls == 1` (RED); post-fix all witnesses pass (GREEN).

Repair 5.4-R2 — content-sensitive freeze and exhaustive artifact-record
audit:
- `scripts/ms05_evidence_capture.py::source_identity()` now builds the
  worktree identity from the binary unstaged diff (`git diff --binary`) plus
  deterministic untracked path/content entries (`git ls-files --others
  --exclude-standard`, each path hashed with its content, sorted), so a byte
  edit inside an already-modified tracked file invalidates the freeze even
  when `git status --porcelain` category text is unchanged. `git ls-files
  --stage` remains the index identity. All Git reads stay read-only.
- `scripts/ms05_evidence_capture.py::artifact_record_specs()` deterministically
  derives the exact 18 artifact command records `(gate_id, argv)` from the
  six artifacts and the file/stat/sha256sum command triple, and
  `run_artifact_records()` is rewired through it so the capture runner and
  the audit validator cannot drift.
- `scripts/ms05_evidence_audit.py::audit_artifact_records()` validates the
  exact 18 artifact records: missing → `ARTIFACT_RECORD_MISSING`, wrong
  literal argv → `ARTIFACT_RECORD_ARGV`, non-zero exit or non-`pass`
  classification → `ARTIFACT_RECORD_FAILED`, cwd not equal to the
  qualification root → `ARTIFACT_RECORD_CWD`, and any artifact-named record
  outside the exact set → `ARTIFACT_RECORD_UNEXPECTED`; every record must
  also pass the common `audit_record()` checks (times, exit, classification,
  nonempty hash-matching log). Duplicates are already rejected by the global
  `DUPLICATE_GATE_ID` check. It runs in `audit_manifest()` before the
  derived artifact entries are accepted.
- `tests/test_ms05_evidence_tools.py` adds `TestArtifactRecordSpec`
  (exact 18 records, unique IDs, command prefixes) and
  `TestContentSensitiveIdentity` (temporary git repositories prove a same-
  status byte edit and an untracked content edit both change the worktree
  identity). `scripts/ms05_evidence_audit.py` adds five exact-code negative
  fixtures for missing/unexpected/argv/exit/duplicate artifact records and
  regenerates the valid fixture template through `artifact_record_specs()`.

**Changed Files and Symbols**

- `scripts/ms05_data_plane_stimulus.py`: `send_bounded` post-setter fresh
  precheck; `_serve_exchange.bounded_recv` post-setter fresh precheck;
  `SetterDelaySocket` fake; five late/affordable-setter witnesses; new
  self-test summary line.
- `scripts/ms05_evidence_capture.py`: `artifact_record_specs()`; rewired
  `run_artifact_records`; content-sensitive `source_identity`.
- `scripts/ms05_evidence_audit.py`: `audit_artifact_records()`; hook in
  `audit_manifest`; `build_valid_fixture` artifact records via
  `artifact_record_specs()` with qualification-root cwd; five new negative
  fixtures.
- `tests/test_ms05_evidence_tools.py`: `TestArtifactRecordSpec`,
  `TestContentSensitiveIdentity`, `subprocess`/`tempfile`/`Path` imports.

**Deviations from Plan**

- The late-setter witnesses use a single `SetterDelaySocket` whose
  `settimeout()` consumes one configured delay per call instead of separate
  fake classes per direction; this matches the Plan's "setter-delay fakes"
  while keeping the call-index convention (2nd call = registration receive,
  3rd call = READY send) explicit in comments.
- The duplicate-artifact fixture expects the pre-existing global
  `DUPLICATE_GATE_ID` code rather than a new artifact-specific code, because
  `audit_manifest()` already rejects duplicate gate IDs before
  `audit_artifact_records()` runs; the repair contract "rejects duplicate
  records" is satisfied by that existing check.
- `audit_artifact_records()` records are `by_id`-keyed with
  `record.get("gate_id")` (not `record["gate_id"]`) to keep the same
  missing-field safety style as the global duplicate check.
- No product kernel/driver/ABI/wire change was needed; the repairs stay
  inside the probe host path, the Evidence scripts and their tests as the
  Plan scoped.

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: pass — 5.3-R3 (post-setter recheck before send and
  receive, zero I/O at/after the deadline, clamped timeout and postcheck
  preserved, cleanup not counted as progress) and 5.4-R2 (content-sensitive
  worktree identity binding tracked bytes and untracked content, exact 18
  artifact record set audited for argv/time/exit/classification/log/hash/cwd)
  are fully covered with the specified RED→GREEN witnesses and fixtures.
- Full diff reviewed: pass — staged + unstaged diff of the four changed
  source files reviewed; no out-of-scope edits (the 001-rework.md unstaged
  change is the Plan Review written by Plan, not this Act); cross-task
  interaction checked (content-sensitive identity keeps the temporary
  qualification root stable because payload rebuilds are byte-identical, and
  the fixture template now matches the capture runner's exact artifact IDs).
- Critical findings unresolved: none
- Important findings unresolved: none
- Minor findings unresolved: none

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED witness 5.3-R3 | `python3 scripts/ms05_data_plane_stimulus.py --self-test` (pre-fix) | `AssertionError: 1` on `late_send.send_calls == 0` | RED CONFIRMED |
| GREEN 5.3-R3 | `python3 scripts/ms05_data_plane_stimulus.py --self-test` | `late-send-setter=PASS past-send-setter=PASS late-recv-setter=PASS past-recv-setter=PASS affordable-setter=PASS`, exit 0 | PASS |
| RED witness 5.4-R2 | `python3 -m unittest tests.test_ms05_evidence_tools` (pre-fix identity) | 3 failures in `TestContentSensitiveIdentity` | RED CONFIRMED |
| RED witness 5.4-R2 | monkeypatched old audit on fixture with missing/tampered artifact record | old audit accepts both | RED CONFIRMED |
| Evidence unittests | `python3 -m unittest tests.test_ms05_evidence_tools -v` | Ran 9 tests, OK, exit 0 | PASS |
| Capture self-test | `python3 scripts/ms05_evidence_capture.py --self-test` | `schema=PASS hash=PASS classify=PASS d1=PASS`, exit 0 | PASS |
| Audit self-test | `python3 scripts/ms05_evidence_audit.py --self-test` | 25 fixtures FAILED AS EXPECTED, `negative fixtures PASS`, exit 0 | PASS |
| C regression | `cc -fsyntax-only tests/ms05_data_plane_probe.c` + harness | `22 passed` + `18 passed`, exit 0 | PASS |
| Stimulus loopback | `python3 scripts/ms05_data_plane_stimulus.py --loopback-self-test` | `protocol=PASS datagrams=96 sequence=PASS bounded=PASS`, exit 0 | PASS |
| Host suite | `make host-test` | all harnesses exit 0 (MS04/MS05 loopback PASS this run; no R44 EPERM) | PASS |
| Full qualification | `ms05_evidence_capture.py --run automatic --root /tmp/ms05-qual-010-c2` | 44 records, 44 pass, 18 artifact records, 0 non-pass | PASS |
| Audit qualification | `ms05_evidence_audit.py --root … --write-qualification/--verify-qualification` | positive audit PASS; `qualification binding VERIFIED`, verdict PASS | PASS |
| Artifacts | `sha256sum -c artifacts.sha256` | 6/6 OK | PASS |
| 100x children | manifest inspection | race-control/v3/full-suite each 100 children, exit 0 | PASS |
| OpenSpec | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | `Change … is valid`, exit 0 | PASS |
| Diff checks | `git diff --check` + `git diff --cached --check` (evidence-excluded) | both exit 0 | PASS |
| Py compile | `python3 -m py_compile` on 4 files | exit 0 | PASS |

**Persisted Evidence**

None required. Per the Plan's `Persisted Evidence mode: none` and the user's
large-file retention waiver, the qualification pipeline ran against the
temporary root `/tmp/ms05-qual-010-c2` and that root was removed after the
artifact re-read; no change-local Evidence directory was created for this
Cycle.

**Experience Candidates**

- Runbook candidate: the content-sensitive worktree identity
  (`git diff --binary` + sorted untracked path/content entries) plus the
  exact-18 artifact-record audit is now validated end-to-end in a temporary
  qualification root: payload rebuilds (`make -B`) and the gcc `build-ms01`
  step are byte-identical, so a full automatic run keeps the frozen identity
  stable. Referenced by this Act Response.
- Incident candidate: none.

**Remaining Issues**

None — both repair-item contracts, RED→GREEN witnesses, exact-code fixtures,
full verification, temporary qualification binding and both diff checks
pass; no manual QEMU was run and Iteration 011 tasks 6.1-6.3 remain deferred.

**Commit or Diff Reference**

Worktree diff on `net-k3` for the four changed files; no commit created.
Iteration 010 Cycle 002 files staged/modified in the shared worktree as
described; the 001-rework.md unstaged diff is the Plan Review, not this Act.

## Plan Review

- Status: reviewed

**Review Result**

Accepted.

**Findings**

- No blocking finding. Independent source review and fresh tests confirm that
  Python rechecks the absolute exchange deadline after timeout installation and
  before both `sendto()` and `recvfrom()`. Exact-boundary and past-boundary
  setter-delay witnesses prevent the I/O call; the affordable path preserves
  the installed timeout and completes normally.
- No blocking finding. Worktree identity now hashes the binary tracked diff and
  sorted untracked paths plus contents, while index identity remains bound to
  `git ls-files --stage`. Temporary-repository tests reject same-status tracked
  edits and untracked-content drift.
- No blocking finding. The capture and audit paths derive the same exact set of
  18 artifact command records and verify argv, common record fields, raw-log
  hash/time, exit, PASS result, cwd, uniqueness and unexpected IDs.
- Fresh `make host-test` reached the MS04 loopback check after the earlier Rust
  and C harnesses passed, then failed to create a UDP socket with exact
  `PermissionError: [Errno 1] Operation not permitted`. This is the R44
  capability boundary and not a product failure; Iteration 011 Task 6.1 owns
  the unchanged-argv ordinary-terminal rerun.
- The user-authorized deletion of the large temporary Cycle 010 manifest and
  logs is accepted as a persistence waiver. Review did not infer their content
  from absent files: it independently reran the focused validators and treats
  the Act Response only as the retained summary of the full temporary run.
- A final unrestricted `git diff --check` still reports pre-existing EOF blank
  lines in Iteration 009 `red-fixtures/probe_red.c` and the Iteration 010
  Evidence `README.md`. They are outside this Cycle's product/Plan edits and do
  not reopen 5.3-R3 or 5.4-R2; Iteration 011 final review must retain their
  historical/Evidence classification rather than describe the unrestricted
  worktree check as GREEN.

**Deviation Classification**

None for the implementation. The fresh Review-only `make host-test` result is
NEW-EVIDENCE for the already-defined R44 capability branch; it neither changes
the Cycle contract nor authorizes a product-failure waiver.

**Acceptance Gaps**

None. Repair items 5.3-R3 and 5.4-R2 close the two inherited Cycle 001 gaps.

**Convergence**

Closed. The second rework Cycle resolves the remaining Python timeout-to-I/O
race and content-insensitive qualification identity/record-audit gap without
opening another requirement, design issue or adjacent fault domain.

**Evidence**

- Fresh stimulus self-test passed, including exact/past delayed timeout setters
  for send and receive plus the affordable setter path.
- Nine Evidence-tool unit tests, capture self-test and all 25 exact-code audit
  fixtures passed.
- Strict C syntax and the production-runner harness passed all 22 decision and
  18 operation-seam tests. `py_compile` passed for the four Python tools.
- An independent setter-delay demonstration raised before `sendto()` with
  `send_calls=0` and retained the one-second installed timeout.
- Strict OpenSpec validation and the Cycle's Evidence-excluded worktree/index
  diff checks exited 0. The final unrestricted check produced only the two
  historical Evidence EOF findings listed above.
- Fresh `make host-test` produced the exact R44 `EPERM` handoff described above;
  no compile, link, assertion, audit or diff failure preceded it.

**Follow-up Decision**

Accept Cycle 002 and Iteration 010. Expand Iteration 011 for Tasks 6.1-6.3; do
not create Cycle 003.

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../011-independent-manual-qemu-runtime-and-closeout-review/000-initial.md`
