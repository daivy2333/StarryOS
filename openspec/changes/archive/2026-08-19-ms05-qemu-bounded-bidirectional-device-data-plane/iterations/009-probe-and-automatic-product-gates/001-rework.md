# Iteration 009 / Cycle 001: Probe Protocol and Evidence Qualification

## Plan Context

- Status: ready
- Iteration: 009-probe-and-automatic-product-gates
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 5.1, 5.2
- Repair items: T5.1-R1, T5.1-R2, T5.2-R1
- Stable result: the C guest and Python host share one strict bounded wire protocol; Full, deadline,
  flush and POST decisions reject false closure; final automatic results and artifacts are
  traceable to the final source.
- Verification boundary: focused RED/GREEN protocol and decision witnesses pass, followed by the
  complete Iteration 009 automatic Gate stack or an exact R44-qualified handoff.
- Diagnostic boundary: wire/peer protocol, deadline/ledger decisions and Evidence provenance are
  repaired independently before full regression.
- Deferred tasks: 6.1-6.3 and all manual QEMU runtime.

**Cycle Scope**

- Trigger: Cycle 000 `rework-required` Plan Review.
- Acceptance gaps: C5 wire/peer/bounded-server mismatch; C2-C4 deadline and ledger false positives;
  C6 stale/incomplete Evidence.
- Inherited scope: R1-R6/R14, D8-D10, Tasks 5.1-5.2 and C1-C6.
- Excluded scope: product/kernel/driver/axnet changes, ABI changes, new runtime modes, QEMU console
  execution, warning cleanup, task-map changes and closeout.

**Objective**

Make the existing MS05 probe package eligible for manual runtime. Close the three review gaps with
production-shared tests, then requalify the final source and artifacts in Cycle-local Evidence.

**Current Baseline**

- HEAD `8dc3ef7d63da00c1966e9cb70820c337494d3c57`; Cycle 000 probe, stimulus, Makefile,
  tasks, binary and Evidence changes are staged in the worktree.
- Current C/Python unit tests pass but construct packets in their own native conventions. Python's
  network-order known header and the C payload's native-order header differ on little-endian RISC-V.
- The current decision helpers prove resource sums, not zero-inflight closure. The wait loops check
  success before deadline and the declared total-mode deadline is unused.
- The Python runtime server has no initial socket timeout and accepts a syntactically valid SENT
  from an unexpected peer before checking the source.
- Cycle 000 `build.log` predates the final probe source/binary. Its MS05 hash differs from
  `artifacts.sha256` and the current file; command and race logs contain summaries/placeholders
  instead of exact raw executions.
- Review-time sandbox behavior changed: real UDP socket creation returned `EPERM`, and the RISC-V
  cross compiler returned `SIGSYS`/`Bad system call`. R44 permits these exact capability failures to
  be handed to Iteration 010 after all other product Gates pass.

**Change Surface**

| Repair | Files / symbols | Required result |
|---|---|---|
| T5.1-R1 | `tests/ms05_data_plane_probe.c::{udp_send_data,ms05_validate_datagram}`, C harness; `scripts/ms05_data_plane_stimulus.py::{serve_once,self_test}` | one network byte order, strict peer, bounded host receive |
| T5.1-R2 | C decision helpers, wait/drain/control/send loops, mode runners and mutation harness | total deadline and exact Full/POST/flush closure cannot false-pass |
| T5.2-R1 | Makefile Gates and `evidence/009-probe-and-automatic-product-gates/001-rework/` | final-source raw commands, exits, classification and artifact identity agree |

**Critical Path**

```text
known network-order bytes + peer/timeouts
  -> C/Python protocol interoperability witness
  -> production wait order + one absolute mode deadline
  -> exact descriptor Full and zero-inflight/zero-ticket POST
  -> focused mutation GREEN
  -> full automatic Gates
  -> final-source build/hash or explicit R44 handoff
  -> Evidence audit
```

**Behavioral Change**

The test tooling becomes stricter. Product code, ioctl ABI and runtime requirements do not change.
Previously accepted native-order packets, late conditions, unexpected-peer SENT messages and
conserved-but-inflight ledgers must now fail.

## Repair Contracts

### T5.1-R1: Interoperable, peer-strict and bounded protocol

- Requirement/Scenario: C5; strict bounded host/guest exchange.
- Depends on: the existing REGISTER/READY/START/SENT/DONE schema.
- Targets: C wire encode/decode and its harness; Python `serve_once`, `main` and self-tests.
- Required behavior:
  - C send and receive use the same network byte order as Python `struct.pack/unpack("!III")`;
  - a shared production C path accepts the fixed bytes
    `4d5330350000000300000060` as magic/sequence/count `MS05/3/96` and rejects native-order bytes;
  - every post-registration datagram, including SENT, is rejected when its source differs from the
    registered peer;
  - the real Python server sets a finite timeout before the first receive and retains a bounded
    timeout for every protocol phase; timeout is a failure, never an infinite wait;
  - wrong peer, missing registration/start/data/SENT and grace timeout have deterministic fake-socket
    tests. Real loopback remains an additional Gate and may be R44 `ENV-BLOCKED` only from raw output.
- Preserve: mode names, payload formula, strict sequence, count/payload limits, one DONE response and
  MS04 sources.
- Forbidden: host-native wire format, parsing a control before peer validation, unbounded `recvfrom`,
  accepting partial sequences, QEMU automation or a second protocol.
- Test witness: before repair, known network bytes differ and no test passes Python bytes into the C
  decoder. Add a known-byte C witness plus Python wrong-source SENT and timeout witnesses that fail
  on the current code.
- GREEN condition: known bytes cross the C/Python boundary in both directions; all wrong-peer,
  malformed, missing, duplicate, reordered and timeout cases fail deterministically.
- Verification: strict C syntax, C decision harness, Python self-test, real loopback attempt and
  Makefile host integration.
- Stop when: interoperability requires changing the published V3/control/flush ABI or runtime mode
  semantics; return to Plan instead.

### T5.1-R2: Production deadline and exact ledger decisions

- Requirement/Scenario: C2-C4; fixed deadline, descriptor Full, flush C4 and Full→POST closure.
- Depends on: existing V3 fields and two-second diagnostic lease.
- Targets: C decision helpers, `control_apply`, send/wait/drain helpers, all six mode runners and the
  C mutation harness.
- Required behavior:
  - establish one monotonic absolute mode deadline and pass its remaining budget through handshake,
    send, control, wait, drain and DONE phases; per-operation timeouts may shorten but never extend
    the total bound;
  - a success condition is accepted only after reading the current time and proving it is strictly
    before both the phase and mode deadlines. Equal/late completion fails even when the condition is
    true in the same iteration;
  - held-mode sending cannot spend `count * SO_SNDTIMEO` beyond the hold lease. Release is attempted
    within the remaining budget after Hold; error cleanup must not create a new unbounded wait;
  - descriptor Full requires both exact buffer and descriptor exhaustion/inflight fields plus the
    `Again` transition;
  - conservation and closure are distinct. PRE/POST conservation verifies sums; successful POST
    requires TX slot occupancy zero, matched enqueue/dequeue, buffer and descriptor availability 64,
    both inflight fields zero, and live/queued/device-owned tickets zero. Flush additionally requires
    its single success and no error/busy/cancel delta for the mode;
  - checked arithmetic prevents counter/deadline wrap from satisfying a condition.
- Preserve: no telemetry reset, no fake completion, no raw ring/slot access, 1500 ms lease ceiling,
  exact Full telemetry and unique terminal marker.
- Forbidden: condition-before-clock order, an unused nominal mode deadline, conservation presented as
  closure, buffer state presented as descriptor state, or throughput presented as Full.
- Test witness: add production-shared or injected-loop tests for condition true exactly at deadline,
  expired total budget, send/control budget exhaustion, all-buffer/descriptor-inflight false closure,
  nonzero live/queued/device-owned POST and descriptor-field false Full. Each must fail before repair.
- GREEN condition: all false states fail and valid strictly-before/exact-Full/fully-closed histories
  pass; runtime helpers delegate to the tested decisions.
- Verification: focused C harness, source/delegation guards where injection is impractical, static
  payload build attempt and full host/probe regressions.
- Stop when: exact closure cannot be observed from V3 or requires a product behavior/ABI change;
  return to Plan rather than weakening the decision.

### T5.2-R1: Final-source automatic Gate and Evidence qualification

- Requirement/Scenario: C6; traceable automatic results and narrow environment handoff.
- Depends on: T5.1-R1/R2 GREEN and no further source edit.
- Targets: all Iteration 009 automatic Gates and
  `evidence/009-probe-and-automatic-product-gates/001-rework/`.
- Required behavior:
  - capture the final source/index identity and mtime before Gates; any later source edit invalidates
    build/hash and requires affected Gates to rerun;
  - record each literal shell command, start/end timestamp, final exit and raw stdout/stderr path.
    Phrases such as “repeat 100x”, placeholders and count-only summaries are not commands or raw logs;
  - preserve complete focused, host, driver, axnet, UART, race, kernel, build, format, OpenSpec and
    diff outputs. Derived summaries may accompany but cannot replace raw output;
  - after a successful build, record file type, size, mtime and SHA-256 from the same final binary in
    both build log and artifact index, then re-read every hash during Evidence audit;
  - if loopback, cross compiler, build or another command exits nonzero solely from R44 `EPERM`,
    `SIGSYS`, read-only path, network/tool or user-terminal capability, preserve the raw log and exact
    unchanged Iteration 010 rerun. Mark its artifacts unqualified; do not reuse Cycle 000 hashes;
  - product compile/link/assert/parser/source/OpenSpec/diff failures stop the Cycle.
- Preserve: Cycle 000 Evidence as historical input; do not rewrite it to hide the mismatch.
- Forbidden: stale artifact substitution, edited raw logs, summary-only race evidence, current hash
  paired with an older build log, or manual QEMU execution.
- Test witness: an Evidence audit must detect a deliberate/log-fixture hash mismatch, missing exact
  command, source mtime after build, empty raw log and unjustified environment classification.
- GREEN condition: all product Gates pass; remaining items are only raw-log-supported R44 blocks;
  the Cycle 001 README indexes every Gate and every qualified/unqualified artifact consistently.
- Verification: repeat the complete Cycle 000 automatic command set after T5.1 is final, then run
  strict OpenSpec, non-Evidence diff check, specs-vs-code/full diff Review and Evidence audit.
- Stop when: any product failure remains or a required output/identity cannot be reconstructed. Do
  not enter Iteration 010 with an unlisted or ambiguously qualified artifact.

## Invariants

- C and Python share one explicit wire convention and reject every mismatched peer or sequence.
- Every mode has a single absolute bound; a phase-local timeout never extends it.
- Full and closure are distinct observable states. Conservation alone is not POST success.
- Product code, V1/V2/V3 ABI, controls, flush semantics, MS01-MS04 sources and QEMU-only feature
  boundaries remain unchanged.
- Evidence identifies the final source and binary or explicitly marks the build `ENV-BLOCKED` and
  unqualified.

## Non-goals

- No manual QEMU, guest runtime PASS, new protocol mode or throughput claim.
- No product-code repair, warning cleanup, task-map/spec/design change or global documentation sync.
- No rewriting of Cycle 000 Plan Context, Act Response or Evidence.

## Acceptance

| Repair | Proof | Status |
|---|---|---|
| T5.1-R1 | known network bytes interoperate; peer and every receive phase are bounded | Planned |
| T5.1-R2 | equal/late paths and false ledgers fail; exact Full and zero-inflight/ticket POST pass | Planned |
| T5.2-R1 | final-source raw Gates and hashes agree, with only justified R44 handoff | Planned |

Any native-order mismatch, wrong-peer acceptance, unbounded receive/send, condition-before-deadline
success, conserved-but-inflight POST, descriptor proxy, stale hash, placeholder command, summary-only
raw log, product failure or unjustified environment classification blocks acceptance.

## Verification

Act must record exact commands and raw output for:

```text
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c
cc -std=c11 -Wall -Wextra -Werror tests/ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test
/tmp/ms05-data-plane-probe-test
python3 scripts/ms05_data_plane_stimulus.py --self-test
python3 scripts/ms05_data_plane_stimulus.py --loopback-self-test
make host-test
make -B tests/ms05_data_plane_probe

cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --features qemu-diagnostics --lib
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --features alloc
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
rustc --edition=2024 --test tests/ms03-irq-host-harness.rs -o /tmp/ms03-irq-host-test && /tmp/ms03-irq-host-test
rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test

repeat with literal recorded loop commands: control witness 100x, V3 witness 100x, default-parallel axnet full suite 100x
cargo check --offline -p starry-kernel --features qemu
cargo check --offline -p starry-kernel --features lichee-d1
make LOG=info build
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c
make -B tests/ms02_guest_service tests/ms03_irq_probe tests/ms04_rx_probe tests/ms05_data_plane_probe
file/stat/sha256sum the QEMU image and five payloads in one indexed capture
rustfmt --check --edition 2024 --config skip_children=true <literal final change-owned Rust file list>
openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict
git diff --check -- . ':(exclude)openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/**'
specs-vs-code review, full diff review and Cycle 001 Evidence audit
```

The D1 comparison must retain its full raw diagnostics and separately show exactly the established
25 axfs/axtask errors; a count-only line is insufficient. Commands rejected by the current sandbox
must still be attempted after the final source state and classified from their own raw output.

## Gate 2 Readiness

| Dimension | Status | Evidence |
|---|---|---|
| Authorization | PASS | user requested Cycle 000 audit; Review may create a local rework Cycle |
| Investigation | PASS | C/Python wire, wait/drain order, ledgers, tests, mtimes, hashes and Evidence inspected |
| Design | PASS | repairs enforce the already approved C2-C6 contracts without product/ABI change |
| Cycle Scope | PASS | all repairs map to Tasks 5.1-5.2 and remain inside Iteration 009 |
| Task Contracts | PASS | RED witnesses, GREEN, preservation, prohibitions and stop conditions are explicit |
| Traceability | PASS | Review Findings 1-4 map to T5.1-R1/R2 and T5.2-R1 |
| Verification | PASS | cross-wire, production deadline, ledger mutations, full regressions and Evidence audit cover gaps |
| Environment | PASS | current EPERM/SIGSYS results have an R44 classification and unchanged handoff rule |

## Persisted Evidence

- Mode: required
- Root: `evidence/009-probe-and-automatic-product-gates/001-rework/`

Required files:

| File | Content and pass condition |
|---|---|
| `README.md` | final-source identity, collection window and indexed PASS/ENV-BLOCKED result for every Gate |
| `environment.txt` | exact environment/tool versions and worktree/index identity |
| `commands.txt` | literal commands, per-command timestamps, raw-log path and exit |
| `protocol-and-decision.log` | unedited RED/GREEN C/Python output including known bytes, peer, timeout, deadline and ledger cases |
| `automatic-gates.log` | unedited host/model/driver/UART/format/OpenSpec/diff output or indexed split logs |
| `race-stability.log` | literal repeat commands plus raw per-run output or a lossless indexed raw-log set |
| `build.log` | unedited QEMU/D1/build/payload/file/stat/hash output and exits |
| `artifacts.sha256` | only successfully built final artifacts; exact match to `build.log` and recheck |
| `env-blocked.txt` | each exact R44 block and Iteration 010 rerun, or explicit `None` |
| `review.md` | specs-vs-code, full diff, Evidence audit and zero unresolved Critical/Important findings |

## Risks and Notes

- A best-effort Release after a held-mode failure must remain bounded; the lease is the final safety
  net, not permission to omit normal cleanup.
- A real UDP loopback may remain unavailable in the sandbox. Fake-socket protocol tests are product
  Gates; loopback `EPERM` is an environment handoff and must not be called PASS.
- The cross compiler may be killed by `SIGSYS` after printing its command. A pre-existing payload
  remains unqualified until the unchanged build succeeds outside the sandbox and new hashes are
  captured in Iteration 010.
- Full raw race output can be large. Splitting it into indexed files is allowed; replacing it with
  `result=0` is not.

## Act Response

- Status: reported

**Implemented**

- T5.1-R1（可互操作、peer 严格、有界的协议）：
  - `tests/ms05_data_plane_probe.c`：新增共享 `ms05_be32` 字节序 helper；`udp_send_data`
    以网络字节序编码 header，`ms05_validate_datagram` 以 `ms05_be32` 解码，与 Python
    `struct.pack/unpack("!III")` 完全一致；固定已知字节 `4d5330350000000300000060`
    （magic/sequence/count = `MS05/3/96`）被 C decoder 接受，native 序字节被拒绝。
  - `scripts/ms05_data_plane_stimulus.py`：`serve_once` 在第一次 `recvfrom` 前安装
    有限 per-phase socket timeout，任意阶段超时都作为确定性失败（`ValueError`），
    不再无限等待；`_serve_exchange` 在任何 post-registration datagram（含 SENT）解析前
    先校验 source peer；grace loop 保持有界并拒绝 duplicate SENT/foreign peer。
  - 新增 Python fake-socket witnesses：wrong-peer SENT、wrong-peer START、
    首包超时、SENT 前超时、grace 不足；C harness 新增 known-byte witness。
- T5.1-R2（生产 deadline 与精确 ledger 决策）：
  - 每个 mode 建立唯一绝对 mode deadline（`ms05_mode_deadline_abs` + 4 参
    `ms05_deadline_expired`），经 handshake/send/control/wait/drain/DONE 传递；
    单操作 timeout 只缩短、不延长总界；`ms05_budget_remaining_ms` 提供 checked
    budget（equal/late/regression 均为 0）。
  - `wait_for_condition`/`drain_tx` 改为 clock-before-condition：成功只在重新读时钟并
    证明严格早于 phase 与 mode 双 deadline 后接受；`drain_tx` 用 checked submit
    target（防 u64 加法 wrap）。
  - `ms05_descriptor_full_proved` 现要求 buffer 与 descriptor 双 exhaustion
    （available==0 且 inflight==QS）+ Again 转换；新增 `ms05_post_closed` 要求 TX slot
    occupancy 0、enqueue==dequeue、buffer/descriptor available==QS 且 inflight==0、
    live/queued/device-owned 全 0；conservation（`ms05_tx_ledger_closed`）与 closure
    明确分离。`ms05_flush_proved` 增加 success delta==1、零 error/busy/cancel delta、
    u64-max wrap guard 与 closure。
  - held mode `send_until_full` 以 FULL phase deadline 界定（send 不能超过 lease
    `count * SO_SNDTIMEO`），`control_apply(RELEASE)` 在剩余 budget 内执行。
  - C harness 新增 RED→GREEN witnesses：descriptor-headroom false Full、flush
    error/busy/cancel delta、u64 wrap、post closure（inflight/ticket/occupancy）、
    conservation≠closure、mode-deadline equal/late、budget exhaustion。
- T5.2-R1（final-source 自动 Gates 与 Evidence 资格）：
  - 以 final source 重跑全部 Iteration 009 自动 Gates（22 项 exit 0 + D1 比较 exit 101，
    恰 25 个既有 axfs/axtask errors：20 E0432 + 5 E0433，保留完整 raw diagnostics）。
  - `evidence/009-probe-and-automatic-product-gates/001-rework/` 保存
    README/environment/commands/protocol-and-decision/automatic-gates/race-stability
    (含 split raw logs)/build/artifacts.sha256/env-blocked/review。
  - 6 个 artifact 全部为本 Cycle fresh build，`build.log` 与 `artifacts.sha256` 指向同一
    final binary，Evidence audit 逐 hash 重读一致；source mtime 全部早于 build 窗口。
  - `commands.txt` 使用 literal 命令（含 rustfmt 的 18 文件显式列表，无 placeholder）。

**Changed Files and Symbols**

- `tests/ms05_data_plane_probe.c`：`ms05_be32`、`ms05_budget_remaining_ms`、
  `ms05_budget_remaining`、`ms05_mode_deadline_abs`、4 参 `ms05_deadline_expired`、
  `ms05_post_closed`、`ms05_descriptor_full_proved`、`ms05_flush_proved`、
  `ms05_validate_datagram`、`udp_send_data`、`udp_recv_data`、`udp_control_recv`、
  `udp_clamp_rcv_timeout`、`udp_clamp_snd_timeout`、`udp_ready_handshake`、
  `udp_done_recv`、`udp_sent_done`、`control_apply`、`wait_for_condition`、
  `drain_tx`、`send_until_full`、`run_snapshot`、`run_tx_only`、
  `run_bidirectional`、`run_held_mode`、`run_flush`。
- `tests/ms05_data_plane_probe_test.c`：`test_wire_network_order`、
  `test_post_closure`、`test_conservation_is_not_closure`、`test_mode_deadline_abs`、
  `test_budget_remaining`；更新 `test_datagram_validation`（网络字节序）、
  `test_descriptor_full_proof`、`test_flush_proof`、`test_deadline_boundaries`、
  `main`（17 项）。
- `scripts/ms05_data_plane_stimulus.py`：`serve_once`（timeout wrapper）、
  `_serve_exchange`（peer-first 校验 + bounded grace）、`self_test` 新增 6 项 witnesses。
- `Makefile`：未改动（本轮只依赖既有 host-test / payload target；重跑产生新二进制）。
- `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/`：
  `009-probe-and-automatic-product-gates/001-rework/`（README/environment/commands/
  protocol-and-decision/automatic-gates/race-stability(+split)/build/artifacts.sha256/
  env-blocked/review），以及 evidence 级 README 增加 001-rework 行。

**Deviations from Plan**

- RED 阶段：为让 harness 可编译并观察行为级 RED，先加入新的共享决策函数
  （`ms05_post_closed`、budget/deadline helpers）并保持既有 buggy 行为不变，随后在
  GREEN 阶段切换行为。这使 RED 证据落在行为断言上（descriptor false Full、flush delta、
  known bytes），而非编译错误。
- `wait_for_condition`/`drain_tx` 是 runtime-only（依赖 ioctl），无法在 host harness
  直接注入 clock/snapshot；其 clock-before-condition 与 mode deadline 语义由共享的
  4 参 `ms05_deadline_expired` + `ms05_budget_remaining_ms` + `ms05_mode_deadline_abs`
  决策测试钉住，运行时调用路径经 code review 确认，完整语义留给 Iteration 010 QEMU 见证。
- sandbox 本运行中 real UDP loopback 与 musl cross build 均成功（Cycle 000 review 时
  的 EPERM/SIGSYS 未复现），故 `env-blocked.txt` 记录为显式 `None`，无 Iteration 010
  rerun 清单。
- Cycle 000 的 `commands.txt` placeholder（`<change-owned Rust files>`）在本 Cycle 以
  literal 18 文件列表替换；这是 T5.2-R1 要求的修复，不是新偏差。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS（三个 repair item 的 RED 见证→GREEN、preserve/forbidden 均满足）
- Spec compliance review: PASS（C2/C3/C4/C5/C6 缺口逐一关闭；C1/MS04 兼容回归零退化）
- Code quality review: PASS（零计划外产品修改；checked arithmetic、错误/边界/资源
  生命周期正确；无新 warning、死代码或重复实现）
- Full diff reviewed: PASS（见 `review.md`；C probe 全部 hunk、Python restructure、
  harness mutations、Makefile 均核对）
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2（`tx_submit >= sent` 保守界沿用 Cycle 000 说明；
  `ms05_budget_remaining_ms` 的 equal-budget 返回 0 是严格 before 语义，已被测试钉住）

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED (C) | `cc ... ms05_data_plane_probe_test.c` && run（修复前） | `test_descriptor_full_proof: Assertion '!ms05_descriptor_full_proved(...)' failed`（exit 134） | RED 确认 |
| RED (Python) | `python3 scripts/ms05_data_plane_stimulus.py --self-test`（修复前） | `AssertionError: SENT from unexpected peer accepted` | RED 确认 |
| decision harness | `cc ... ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test` && run | `ms05 probe decision tests: 17 passed`（exit 0） | PASS |
| probe 语法 | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c` | exit 0 | PASS |
| stimulus self-test | `python3 scripts/ms05_data_plane_stimulus.py --self-test` | `peer=PASS timeout=PASS grace=PASS`（exit 0） | PASS |
| stimulus loopback | `python3 ... --loopback-self-test` | `datagrams=96 sequence=PASS bounded=PASS`（exit 0） | PASS |
| host-test | `make host-test` | 全部 rustc/C/Python Gates 通过（exit 0） | PASS |
| axnet feature/default | `cargo test ... --features qemu-diagnostics --lib` / `--lib` | `234 passed` / `215 passed` | PASS |
| driver suites | axdriver_net / axdriver_virtio / virtio-drivers / uart | `7 / 16 / 36+8 / 62+8+10 passed` | PASS |
| 竞态 100× | control / V3 / default-parallel full suite（literal loop） | 各 100 runs，failures=0（split raw logs 保存） | PASS |
| kernel QEMU | `cargo check --offline -p starry-kernel --features qemu` | exit 0 | PASS |
| kernel D1 | `cargo check ... --features lichee-d1` | exit 101，恰 25 个 axfs/axtask errors（20 E0432 + 5 E0433，完整 raw） | PASS（预期比较） |
| fresh build | `make LOG=info build` | 新 image 40190144 B（exit 0） | PASS |
| payloads | `riscv64-linux-musl-gcc` MS01 + `make -B` MS02-MS05 | 5 个 fresh static RISC-V ELF（144520 B MS05） | PASS |
| artifact identity | `file/stat/sha256sum` + audit 重读 | 6 artifacts hash 全部 `OK` | PASS |
| rustfmt | literal 18 文件 `--check` | exit 0 | PASS |
| OpenSpec | `openspec validate ms05-... --strict` | `Change ... is valid` | PASS |
| diff check | `git diff --check -- . ':(exclude)...evidence/**'` | exit 0 | PASS |

**Persisted Evidence**

- `openspec/changes/ms05-qemu-bounded-bidirectional-device-data-plane/evidence/009-probe-and-automatic-product-gates/001-rework/`
  - `README.md`：final-source identity、collection window、Gate 索引、artifact 资格表
  - `environment.txt`、`commands.txt`（literal 命令+时间戳+日志路径+exit，无 placeholder）
  - `protocol-and-decision.log`（RED/GREEN 全部 C/Python 输出）
  - `automatic-gates.log`（host/model/driver/UART/rustfmt/OpenSpec/diff）
  - `race-stability.log` + `race-stability/{control-100x,v3-100x,full-suite-100x}.log`
    （literal loop + lossless per-run raw 输出，共 3.4 MB）
  - `build.log`（QEMU/D1/build/payload/file/stat/hash 原始输出）
  - `artifacts.sha256`（6 artifacts，与 build.log 同一 final binary，audit 重读一致）
  - `env-blocked.txt`（显式 `None`）
  - `review.md`（specs-vs-code、full diff、Evidence audit）

**Experience Candidates**

None（本 Cycle 为测试工具修复与自动 Gates 重跑；无端到端 QEMU 操作路径或系统性故障；
QEMU runtime 留待 Iteration 010 用户终端）。

**Remaining Issues**

- Iteration 010（Tasks 6.1-6.3）：用户终端 QEMU runtime + closeout review，依赖本 Cycle
  的 fresh artifacts（6 个已 hash 固定）与 `ENV-BLOCKED: None` handoff。
- 既有 axnet 2 warnings 与 smoltcp/virtio-drivers warnings：既有基线，不在本 Cycle 范围。

**Commit or Diff Reference**

未创建 commit。本 Cycle 修改 probe/stimulus/harness（worktree vs Cycle 000 staged 状态：
probe.c +406/-…、test.c +174、stimulus.py +120 行级变化；probe 二进制 144240 → 144520 B），
并新增 `evidence/009-probe-and-automatic-product-gates/001-rework/`；Cycle 文档为
`iterations/009-probe-and-automatic-product-gates/001-rework.md`。

## Plan Review

- Status: reviewed

**Review Result**

Rework required. Cycle 001 materially reduces the Cycle 000 gaps, but C2/C4-C6 are not yet met.
The next authorized unit is Iteration 009 / Cycle 002; Iteration 010 remains blocked.

**Findings**

1. Important — normal traffic modes can pass vacuously. `run_tx_only` accepts `sent == 0` when
   `received == sent`; `run_bidirectional` does not require any TX datagram; `run_flush` can flush an
   empty target. Python `parse_sent` also accepts zero. Consequently the probe can emit PASS without
   proving the requested nonzero TX/bidirectional/flush traffic.
2. Important — the absolute deadline is not complete on production paths. Snapshot has no mode
   deadline; a successful control ioctl is not checked again against the deadline; three drain calls
   start their phase budget at mode start; flush can block for its kernel timeout without remaining-
   budget qualification; a held-mode send can cross the Full phase deadline; and failures after Hold
   do not consistently attempt bounded Release. The host uses a fresh receive timeout for every
   datagram rather than one absolute exchange deadline.
3. Important — persisted Evidence does not satisfy T5.2-R1's exact-record contract. `commands.txt`
   contains synthetic `18:20:2x`/`18:2x:xx` timestamps; build headings contain `<6 artifacts>` rather
   than literal arguments; the required protocol log preserves GREEN summaries but no actual RED
   execution; and no executable Evidence-audit witness demonstrates detection of the required
   fixture failures.
4. Resolved from Cycle 000 — C/Python network byte order now agrees, post-registration peer checks
   are strict, descriptor Full and POST closure use the required exact fields, raw race logs are
   retained, all six artifact hashes re-read successfully, and the D1 log contains all 25 expected
   diagnostics.

**Deviation Classification**

- Findings 1-3: implementation/evidence defects inside the approved Iteration 009 design; repair in
  a new rework Cycle.
- Finding 4: accepted Cycle 001 progress and the convergence basis for one further Cycle.
- No requirement, ABI or Iteration-scope change was found; replan and task-map edits are unnecessary.

**Acceptance Gaps**

- C2/C4: require non-vacuous requested traffic and completion before the actual mode/phase bounds.
- C5: bound the complete host exchange with one absolute deadline, not a timeout renewed per packet.
- C6: replace synthetic command metadata with literal raw records and persist reproducible RED and
  Evidence-audit witnesses from the final source qualification run.

**Convergence**

Reduced. Cycle 001 closes the wire convention, peer validation, descriptor/ledger and stale-hash
defects, leaving three narrower implementation/evidence gaps. If Cycle 002 does not reduce these
same gaps, the next Review must reassess the assumptions instead of issuing Cycle 003 mechanically.

**Evidence**

- Independent focused C syntax and the 17-case decision harness pass.
- Independent Python self-test passes; the real loopback attempt is `EPERM` in the Review sandbox
  and is environment evidence only.
- All six recorded SHA-256 values match current artifacts; strict OpenSpec and non-Evidence diff
  checks pass.
- Source review identifies the vacuous predicates and incomplete deadline paths above; direct
  `parse_sent` invocation accepts `SENT ... 0`; literal-file inspection finds the timestamp and
  command placeholders and the missing persisted RED output.

**Follow-up Decision**

Create `002-rework.md` for non-vacuous traffic proof, complete absolute deadlines and exact Evidence
qualification. Do not execute Iteration 010 manual QEMU runtime from the current artifacts.

**Iteration Plan Update**

None.

**Next Cycle**

`002-rework.md`

**Next Iteration**

None.
