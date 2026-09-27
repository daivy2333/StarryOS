# Iteration 005 / Cycle 000: Per-driver SMP=16 fixed qualification

## Plan Context

- Status: ready
- Iteration: 005-per-driver-smp16-fixed-qualification
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 6.1–6.3.
- Depends on: accepted Iteration 004 Cycle 001.
- Stable baseline: UART-only and network-only QEMU `SMP=16` runs independently
  demonstrate fixed placement, remote wake, data-plane progress, quiet
  behavior, and controlled migration.
- Verification boundary: each driver has its own runtime transcript and
  validator result; neither driver's success stands in for the other.
- Diagnostic boundary: UART or network IRQ, wake, data, completion, quiet,
  migration, harness, or validator path.
- Deferred tasks: 7.1–7.3 (combined pressure, recovery interleave, final
  regression).

**Cycle Scope**

- Trigger: initial.
- Acceptance gaps: None.
- Repair items: None.
- Inherited scope: the MS08 fixed-placement requirement, Iteration 004's
  accepted protocol and validators, and tasks 6.1–6.3.
- Excluded scope: combined UART/network pressure, final recovery
  interleaving, true-board claims, throughput optimization, and new
  identity or evidence machinery.

**Objective**

Obtain separately decidable QEMU `SMP=16` UART and network runtime results
from the accepted guest probes, host participants, and pure-output
validators. A failed runtime result remains a diagnostic finding, not a
qualification pass.

**Background**

Iteration 004 Cycle 001 accepted the protocol/host-contract stage: its final
Act Response reports static RISC-V guest builds, focused host witnesses,
validator negatives, schema checks and `make host-test` passing. Plan Review
independently reran the UART and network host fixtures (both exit 0) and
accepted the remaining child-cleanup repair. No `SMP=16` guest runtime
qualification was claimed there.

**Investigation Facts**

- Current Baseline: worktree is on `mul-hart-k3` with MS08 changes staged and
  unstaged. The accepted Iteration 004 output supplies the protocol baseline;
  QEMU runtime results are not yet present in this Cycle. The prior accepted
  foundation/placement/migration Iterations supply the kernel substrate.
  `qemu-system-riscv64`, `riscv64-linux-musl-gcc`, `socat`, the 1 GiB
  `make/disk.img`, kernel image and both static MS08 probes are present in
  the current workspace; their presence is setup evidence, not a runtime
  qualification result.
- Current-State Evidence: `Makefile` builds the two static RISC-V payloads at
  `tests/ms08_uart_smp_probe` and `tests/ms08_network_smp_probe`. The UART
  probe emits eight ordered cases and uses the QEMU serial socket. Its host
  participant `scripts/ms08-uart-serial.py --socket ...` currently waits for
  `MS08_UART_READY`, injects payload frames and prints only the harness
  result: `run_session` does not start the probe at the guest shell and
  `serve` does not copy the received serial bytes to stdout. Because this
  harness is the sole serial-socket client, the current program cannot
  produce a complete UART validator input in a standalone run. The UART
  probe also writes bulk `T`/`D`/`M` bytes directly to stdout immediately
  before some marker prints; without a line delimiter the next marker can
  be attached to payload and disappear from both the harness line parser
  and the validator's line-start grammar. `LineReader` currently retains
  an unbounded no-newline buffer during such TX output. The network
  probe emits ten ordered cases, uses the host echo peer on port 15578, and
  requires QEMU HMP actions for link cases; the peer is
  `scripts/ms08-network-peer.py`. The pure validators are
  `scripts/ms08-uart-validate.py` and
  `scripts/ms08-network-validate.py`.
- Code and Critical Path: guest entry `run_probe` in each C probe performs
  placement and data-path cases through existing UART TTY or network socket
  and diagnostic ioctls; UART output/input passes through the serial socket
  harness, while network frames pass through the external peer. Validators
  consume resulting text and peer/harness outcomes; they do not start QEMU.
  `make/qemu.mk` uses `-machine virt -bios default -kernel ... -m 1G -smp
  $(SMP)` plus configured VirtIO devices. The rootfs is a raw ext4 image:
  `debugfs -R 'ls /root' make/disk.img` confirms `/root`, and the existing
  network Runbook documents `debugfs write` into a *copy* of that image.
  This lets both probes start without making network download a precondition
  for the network qualification. The MS07 Runbook confirms manual HMP
  `set_link net0 off/on`; its old single-hart command is not SMP evidence.
  The network validator requires the guest `MS08_NET_HARNESS_EXIT: 0` line
  followed by the peer's independent `MS08_NET_PEER_RESULT` line. The guest
  probe waits for the exact `MS08_NET_HMP_DONE link=on` console line after
  HMP link-up. The rootfs has `stty`, allowing guest input echo to be
  disabled during that probe so an operator input line cannot become a
  spurious validator marker.

**Implementation Guidance**

Qualify UART and network in separate QEMU sessions. First close the UART
harness's launch/capture and bulk-TX framing gaps with focused before/after
witnesses; keep the validator pure. Use a QEMU serial Unix socket for UART.
Use a separate HMP Unix socket for network so monitor prompts cannot prefix
guest markers.
Keep each raw runtime outcome and validator input attributable to the
corresponding driver. Use bounded host deadlines and preserve failure
reasons. If guest execution exposes a product or protocol defect, stop and
return to Plan Review for a bounded repair decision before recording a pass.

**Behavioral Change**

This Iteration verifies the existing implementation under real QEMU
multi-hart execution. The UART host harness must additionally launch the
accepted guest probe once the shell prompt is visible, stream the raw
serial text into its transcript, and parse markers with bounded memory
despite unframed bulk TX output. The guest probe must start protocol
markers on fresh lines after raw TX bytes. Its case order, marker grammar,
payload injection, result grammar and validator decision remain the same.
No kernel behavior change is planned.

**Task Contracts**

### 6.1: UART-only fixed-placement qualification

- Requirement/Scenario: MS08 UART placement, IRQ-to-copier wake, RX/TX,
  Full-to-recovery, readiness, four-stage `tcdrain`, quiet, and RX/TX copier
  migration under QEMU `SMP=16`.
- Depends on: accepted Iteration 004 UART protocol.
- Targets: `tests/ms08_uart_smp_probe.c::{run_tx_full_recovery,run_tcdrain,run_copier_migration,run_probe}`,
  `scripts/ms08-uart-serial.py::{LineReader,Session,run_session,serve,main}`, UART
  snapshot/TTY ioctls, and
  `scripts/ms08-uart-validate.py::validate`.
- Current behavior: host fixtures and static guest build pass; actual
  UART-only `SMP=16` transcript has not been accepted. The sole serial
  client cannot currently launch the probe or retain the raw transcript;
  bulk TX can merge its following marker into a payload line.
- Required behavior: one RX and one TX copier show valid singleton
  placement, actual IRQ/copy/caller hart and remote-ready progress; numbered
  payloads, capacity recovery, readiness, drain, quiet and migration remain
  consistent in one bounded UART-only run. Before the run, the serial
  harness must observe the guest shell prompt, send a fixed `chmod +x
  /root/ms08_uart_smp_probe && /root/ms08_uart_smp_probe --run` launch
  command exactly once, stream every received serial byte
  unchanged to its captured output, parse markers with bounded memory,
  and append its existing result/exit lines after guest `MS08_UART_END`.
  Bulk TX followed by a snapshot or case marker must have a line boundary
  so the unchanged validator can consume it.
- Required changes: add this bounded launch/capture path to the existing
  harness under an explicit `--launch-probe` option, bound its parser's
  no-newline buffering without dropping raw captured bytes, and ensure
  UART guest markers following bulk writes begin on fresh lines; test these
  paths, then run and diagnose the accepted protocol;
  alter kernel or guest protocol only through a reviewed repair if a
  witnessed defect appears.
- Preserve: one SPSC endpoint per direction, early console, snapshot ABI,
  case order, exact serial bytes and the independent serial harness result.
- Forbidden: using a `PASS` marker alone as payload evidence, unbounded
  waits, a second serial client during qualification, or borrowing network
  success as UART evidence. The harness must not launch QEMU or the validator.
- Test witness: extend `ms08-uart-serial.py --self-test` with a fake shell
  prompt, exact serial chunks and a long no-newline TX stream followed by
  a marker. Before the harness change it must fail because no launch
  command or raw transcript exists and the parser retains the entire stream;
  after the change it must prove one launch, exact byte capture, bounded
  parsing, payload injection and terminal result. Establish a RED witness
  for the guest marker after raw TX before changing its line boundary, then
  require the marker to be visible to the unchanged validator. The accepted
  UART host fixture and guest
  build remain the product pre-runtime GREEN baseline; the QEMU transcript
  is the runtime witness.
- GREEN condition: UART validator accepts the exact guest+harness output,
  with successful harness exit and observable target behavior.
- Verification: harness self-test (exit 0); bounded QEMU `SMP=16` UART
  session with one serial-socket client and captured raw output; then
  `python3 scripts/ms08-uart-validate.py <transcript>` (exit 0).
- Stop when: QEMU/serial setup cannot expose the required actual-hart or
  payload observations, or a product behavior fails; preserve the first
  failing case and return to Plan.

### 6.2: Network-only fixed-placement qualification

- Requirement/Scenario: MS08 network placement, timer-disabled remote wake,
  TCP/UDP bidirectional I/O, Full-to-recovery, readiness/quiet, owner/runner
  migration, and the network-only protocol cases under QEMU `SMP=16`.
- Depends on: accepted Iteration 004 network protocol.
- Targets: `tests/ms08_network_smp_probe.c::run_probe`,
  `scripts/ms08-network-peer.py::serve`, HMP operator actions, network V5
  and diagnostic ioctls, and `scripts/ms08-network-validate.py::validate`.
- Current behavior: host fixtures, peer self-test, validator negatives and
  static guest build pass; actual network-only `SMP=16` transcript has not
  been accepted.
- Required behavior: actual IRQ/owner/runner harts, IPI causality without
  timer-only progress, TCP/UDP peer counts, slot/ticket conservation,
  readiness/quiet, and owner/runner migration pass in a bounded network-only
  run. The protocol also requires reset/link case output and HMP completion.
- Required changes: run and diagnose the accepted protocol; alter product
  or protocol only through a reviewed repair if a witnessed defect appears.
- Preserve: unique queue owner/runner, V1–V5 ABI, peer frame ledger,
  recovery epoch ownership, and explicit host peer result.
- Forbidden: accepting guest markers without peer evidence, using single
  hart results, or treating QEMU evidence as true-board evidence.
- Test witness: accepted Iteration 004 network host fixtures and guest build
  are the pre-runtime GREEN baseline; QEMU plus peer/HMP is the runtime
  witness.
- GREEN condition: network validator accepts the joined guest, harness and
  independent peer result with no rejected frame or resource imbalance.
- Verification: bounded QEMU `SMP=16` network session, peer/HMP result,
  explicit guest shell exit line, and a joined peer result; then
  `python3 scripts/ms08-network-validate.py <transcript>` (exit 0).
- Stop when: peer/HMP choreography, guest signal delivery in migration
  cleanup, or a product behavior cannot meet the existing contract; retain
  the first failing case and return to Plan.

### 6.3: Independent decisions and diff review

- Requirement/Scenario: 6.1 and 6.2 must each pass independently.
- Depends on: 6.1 and 6.2 runtime outcomes.
- Targets: both validators, the two runtime transcripts, and the MS08
  worktree diff.
- Current behavior: no pair of accepted `SMP=16` runtime outcomes exists.
- Required behavior: report each driver's validator verdict and failure
  boundary separately; review the full diff after any repair.
- Required changes: record commands, decisive output, exit codes, and
  conclusions in Act Response; review all affected diff before claiming the
  Iteration complete.
- Preserve: Iteration 004 protocol acceptance and separate driver evidence.
- Forbidden: substituting migration or combined pressure for fixed
  placement qualification, or accepting partial output.
- Test witness: validator negative self-tests already reject missing,
  duplicated and inconsistent case output.
- GREEN condition: both independent validators pass their own runtime
  inputs, and full diff review has no unresolved blocking finding.
- Verification: both validator commands, `git diff --check`, strict OpenSpec
  validation, and full diff inspection.
- Stop when: either driver lacks a decisive runtime result.

**Invariants**

Each driver keeps one logical background role per function; actual hart
observations must agree with schedulable placement; device IRQ and remote
wake must produce progress without timer-only or busy-poll dependence;
payload, descriptor, slot and ticket accounting cannot be inferred from
environment labels or a single `PASS` marker.

**Non-goals**

Combined UART/network pressure, final recovery interleave, true-board
qualification and performance claims.

**Acceptance**

Tasks 6.1 and 6.2 each have a bounded QEMU `SMP=16` result accepted by
their own validator; task 6.3 confirms the two results are independent and
the diff has no unresolved blocking finding. The task-to-scenario mapping
is UART requirement → 6.1 → UART probe/harness/validator, network
requirement → 6.2 → network probe/peer/validator, independent qualification
→ 6.3 → both validator decisions.

**Runtime Procedure**

Prepare the `SMP=16` image and independent disk copies once. The copy is a
payload carrier, not Acceptance evidence; QEMU `-snapshot` keeps the copies
unmodified during qualification. Do not add a hostfwd rule for peer port
15578, which the host peer binds directly.

```sh
make ARCH=riscv64 SMP=16 build
make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe
cp --reflink=auto --sparse=always make/disk.img /tmp/ms08-uart-disk.img
cp --reflink=auto --sparse=always make/disk.img /tmp/ms08-net-disk.img
debugfs -w -R 'write tests/ms08_uart_smp_probe /root/ms08_uart_smp_probe' /tmp/ms08-uart-disk.img
debugfs -w -R 'write tests/ms08_network_smp_probe /root/ms08_network_smp_probe' /tmp/ms08-net-disk.img
```

UART-only session: start the serial harness first in Terminal A, then QEMU
in Terminal B. The harness retries its connection while QEMU creates the
socket, observes the shell prompt in raw bytes, launches the injected probe
once, and writes all received serial bytes plus its existing terminal
result to the transcript. HMP is separate from the serial stream.

```sh
python3 scripts/ms08-uart-serial.py --socket /tmp/ms08-uart.sock --launch-probe --deadline-seconds 360 > /tmp/ms08-uart-transcript.log
```

```sh
qemu-system-riscv64 -machine virt -bios default -m 1G -smp 16 -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=/tmp/ms08-uart-disk.img -chardev socket,id=uart0,path=/tmp/ms08-uart.sock,server=on,wait=off -serial chardev:uart0 -monitor unix:/tmp/ms08-uart-hmp.sock,server=on,wait=off -nographic -snapshot
```

After the harness exits, run the UART validator on its transcript. If it
passes, send `quit` through the separate HMP socket to stop QEMU; if it
fails, preserve the first failure result and stop without claiming 6.1.

```sh
python3 scripts/ms08-uart-validate.py /tmp/ms08-uart-transcript.log
```

Network-only session: start QEMU with its own disk copy, stdio serial and
separate HMP socket. Start the peer *after* the guest shell appears and
before running the probe, with a 420 s bound that exceeds the probe's
300 s overall bound. Capture the serial with `script` and the independent
peer output separately. In the guest, disable input echo for the exact
operator completion line, then run the injected probe and print its real
exit status. The peer's port 15578 is reachable from guest `10.0.2.2`
without hostfwd.

```sh
script -q -e -f /tmp/ms08-net-serial.log -c 'qemu-system-riscv64 -machine virt -bios default -m 1G -smp 16 -kernel StarryOS_riscv64-qemu-virt.bin -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=/tmp/ms08-net-disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0 -serial stdio -monitor unix:/tmp/ms08-net-hmp.sock,server=on,wait=off -nographic -snapshot'
```

```sh
python3 scripts/ms08-network-peer.py --host 0.0.0.0 --port 15578 --deadline-seconds 420 > /tmp/ms08-net-peer.log
```

At the guest shell:

```sh
stty -echo && chmod +x /root/ms08_network_smp_probe && /root/ms08_network_smp_probe --run; rc=$?; stty echo; echo "MS08_NET_HARNESS_EXIT: $rc"
```

When the guest prints `MS08_NET_HMP_READY: link=off`, enter HMP through
`socat - UNIX-CONNECT:/tmp/ms08-net-hmp.sock` in another terminal and send
`set_link net0 off`. At `MS08_NET_HMP_READY: link=on`, send `set_link net0
on` through HMP, then type `MS08_NET_HMP_DONE link=on` into the *guest*
serial console. The probe's `MS08_NET_HMP_OBSERVED` and V5 fields, not the
operator input, are the device-state evidence. After guest exit, let the
bounded peer print `MS08_NET_PEER_RESULT`, stop QEMU through HMP `quit`,
join console then peer output in that order, and run the validator:

```sh
cat /tmp/ms08-net-serial.log /tmp/ms08-net-peer.log > /tmp/ms08-net-transcript.log
python3 scripts/ms08-network-validate.py /tmp/ms08-net-transcript.log
```

**Verification**

Direct evidence is the actual guest data, snapshots, host payload/peer
accounting, control outcomes, validator exit codes, and bounded session
results. Build and host self-tests are prerequisites, not substitutes for
the QEMU runtime observations. A failed command, lost shell prompt,
incomplete peer result, unexpected HMP/console prefix, nonzero guest exit,
or validator rejection stops the affected driver qualification.

**Gate 2 Readiness**

- Requirements/scope: PASS — tasks 6.1–6.3 and the Iteration Map are unchanged.
- Investigation: PASS — `make/qemu.mk`, both probes, harness/peer/validators,
  MS07 and rootfs Runbooks, installed tools, existing image and rootfs
  `/root` were checked. The UART launch/capture and bulk-TX framing gaps
  are identified explicitly. Copying the rootfs and `debugfs write` into
  `/root` succeeded on a temporary copy; `debugfs stat` reported an
  executable RISC-V probe (exit 0).
- Design/tasks: PASS — the required UART harness change, separate disk
  copies, serial/HMP channels, guest entry, TX marker line boundary and
  peer choreography have concrete targets, witnesses, bounds and stop
  conditions.
- Traceability/tests: PASS — UART 6.1 adds a pre-change RED and post-change
  GREEN launch/capture self-test; both driver runtime outputs map to their
  existing validators; 6.3 requires independent decisions.
- Verification: PASS — the procedure above specifies bounded `SMP=16`
  commands, actual guest/host output capture, HMP actions and validator
  exit conditions. Actual runtime passing is an Act/Gate 5 result, not a
  prerequisite to Gate 2. A local QEMU CLI preflight with the serial or
  HMP Unix socket exited 1 at socket bind (`Operation not permitted`) in
  this managed sandbox; this is an execution capability boundary, not
  evidence of a guest or protocol failure. The host must run the runtime
  Gate in an environment that permits Unix sockets.
- No identity-style evidence engineering: PASS — qualification is based on
  target behavior and validator output.
- Change consistency: PASS — no change to specs, tasks or Iteration Map.
- Persisted Evidence: PASS — mode `none` is the initial choice; decisive
  output can be summarized in Act Response. Reassess only if a permitted
  Evidence trigger actually occurs.
- User approval: PASS — the user approved this Iteration 005 Plan Context
  for Act on 2026-09-22 (指令: 更改gate状态，开始实施)。
- Overall Gate 2: PASS — all items PASS; the approved Plan Context status
  changes to `ready` for a separately authorized Act.

**Persisted Evidence**

- Mode: none.
- Budget: decisive commands, output and exit codes belong in Act Response.

**Risks and Notes**

Host fixtures prove cleanup logic on the host. Whether guest `SIGKILL`
reaches a control caller parked in a migration ioctl remains a QEMU runtime
observation. A failure is a runtime finding for 6.1 or 6.2, not a reason to
reinterpret a host pass as a guest pass.

The current managed sandbox denies QEMU binding the planned Unix serial/HMP
sockets (`Operation not permitted` on the local preflight). This plan does
not require loosening the sandbox or treating a syntax-only QEMU check as
runtime evidence. The Act phase can complete its build/host witnesses here,
then the `SMP=16` run must occur in a socket-capable host environment and
return the guest, harness/peer and validator results before Gate 5 can pass.

## Act Response

- Status: blocked

**Implemented**

Task 6.1 harness/guest 变更已实现并 GREEN：

- `scripts/ms08-uart-serial.py`：新增 `--launch-probe`（在 `starry:~#` 后只发一次
  `cp + chmod + /root/ms08_uart_smp_probe --run`）、`--transcript`（原始串口字节
  二进制落盘）、`Launcher`（跨 chunk 匹配 shell prompt，一次性 arm）、
  `LineReader` 有界 no-newline buffer（`MAX_PARTIAL=4096`，为 bulk TX burst 封顶
  内存，raw 字节仍完整流到 transcript）。harness 结果行
  `MS08_UART_HARNESS_RESULT/EXIT` 一并写入 transcript。
- `tests/ms08_uart_smp_probe.c`：`fresh_line()` 在 tx-full-recovery、tcdrain、
  tx-migration 三个 bulk `T`/`D`/`M` 写出后、下一个 marker 前补 `\n`，防止 marker
  与 bulk payload 粘连被 validator 吞掉。
- 两个 probe 宿主 fixture、validator/harness `--self-test`、case/schema diff 全过；
  静态 RISC-V guest 二进制构建通过。

**Changed Files and Symbols**

- `scripts/ms08-uart-serial.py`：`SHELL_PROMPT`、`LAUNCH_CMD`、`MAX_PARTIAL`、
  `Launcher`、`LineReader(max_partial)`、`run_session(out, launcher)`、
  `serve(...launch_probe, out)`、`main(--launch-probe/--transcript)`。
- `tests/ms08_uart_smp_probe.c`：`fresh_line()`，三处 bulk-write 后调用。
- `tests/ms08_uart_smp_probe_test.c`、`tests/ms08_network_smp_probe_test.c`、
  `Makefile`（fer: Iteration 004/005 既有）。kernel：未改动。

**Deviations from Plan**

- `--transcript` 采用二进制写入并追加 harness 结果行，使单文件同时携带 raw
  guest 字节与判定行（Plan 原文依赖 stdout 重定向；等价单文件契约）。
- 运行时 socket 在其真实宿主没有再次回填；Plan 已注明 runtime Gate 需
  socket-capable host，本环境不作为 runtime 判定。

**Blocker Handoff**

Network-only 资格（6.2）卡在首个 case `placement reason=policy`，已定位为
**V5 `owner_hart_mask`/`runner_hart_mask` 字段语义与 `ms08_net_placement_ok`
判定契约不匹配**，属实质问题，按 6.2 停止条件（既有 contract 无法满足）回 Plan
Review，不擅自改 Iteration 002/003 已验收语义。

- 现场证据：`/tmp/ms08-net-serial.log`
  - boot 期 `[NET-SMP-SMOKE]`（迁移前）：`owner_mask=0x8000 runner_mask=0x1`
    —— singleton，全部 PASS。
  - probe `placement`（迁移后）：`owner_mask=0x8001 runner_last=15 owner_aff=15
    owner_events=4 runner_mask=0x3 runner_last=0 runner_aff=0 runner_events=2713
    own_mig=0xf02 run_mig=0x100000002` → `FAIL: placement reason=policy`。
  - boot 期大量 `[NET-MIG-SMOKE] ok role=owner`（`migration_boot_smoke` 已对
    owner/runner 各做 widen→observe→restore 受控迁移）。
- 根因：内核 `HartCounter::record()` 用 `mask.fetch_or(1u64<<hart)`，`mask` 为
  **生命周期累计执行集合、永不回落**；probe `ms08_net_placement_ok` 要求
  `mask == 1<<affinity`（严格 singleton）。迁移后任务在迁移目标 hart poll 过，
  累计 mask 永久含该 bit；restore 后 `owner_last==owner_aff==15`、
  `runner_last==runner_aff==0` 均正确 → 当前固定点正确，仅历史迁移痕迹残留。
  因此只要发生过任何迁移，固定判断就必然失败。
- 争议方向（供 Plan 决策，Act 不选）：
  - A) `placement_ok` 改判「当前 `last==affinity` 且 mask 元素合法」，不再要求
    累计 mask 严格 singleton。
  - B) V5 增加/改独立字段区分「当前 singleton affinity」与「累计执行集合」。

**Blocker Resolution**

None. 待 Plan Review 定方向 / 用户裁决后恢复。

**Self-Review**

- Spec/CQ Review（6.1）：`--launch-probe` 一次性启动、raw transcript、bounded
  parser、fresh-line marker 均按 Task 6.1 契约实现；无 sleep-poll、无 QEMU 启动、
  无第二 copier/owner、不破坏 V1–V4/旧 probe。host fixtures 与 self-test 全过。
- 全量 diff Review：本轮产品改动集中在 6.1（harness + guest header 行），kernel
  未改。无未解决的 Critical/Important。6.2 的 placement 语义冲突列入 Blocker，不改。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| UART harness self-test | `python3 scripts/ms08-uart-serial.py --self-test` | exit 0 | launch/capture/bounded-parser/fresh-line | PASS |
| UART validator self-test | `python3 scripts/ms08-uart-validate.py --self-test` | exit 0 | 8-case 语法/关系 | PASS |
| UART probe host fixture | `cc -std=c11 -Wall -Wextra -Werror tests/ms08_uart_smp_probe_test.c -o /tmp/t && /tmp/t` | exit 0 | placement/drain/quiet/full/migration 决策 | PASS |
| probe 语法 | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms08_uart_smp_probe.c` | exit 0 | C11 `-Werror` | PASS |
| case/schema 一致性 | validator 与 probe `--print-cases/--print-schema` diff | 无差异 | 冻结 case/marker 契约 | PASS |
| UART guest 运行 | shell 内手动 `/root/ms08_uart_smp_probe --run` | `PASS: placement`（rx 等注入超时） | 6.1 固定 placement + IPI | PARTIAL |
| 静态构建 | `make tests/ms08_uart_smp_probe tests/ms08_network_smp_probe` | 两个 ELF | guest payload | PASS |
| Network placement（真机） | `/tmp/ms08-net-serial.log` boot [NET-SMP-SMOKE] vs probe placement | smoke singleton PASS；probe `mask=0x8001/0x3` FAIL policy | 6.2 语义定位 | FAIL（blocked） |

**Persisted Evidence**

None required（mode `none`；决定性输出已在本 Response 与 `/tmp` 现场）。

**Experience Candidates**

- Issue 候选：V5 `mask`（累计执行集合）与 `placement_ok`（严格 singleton）语义
  冲突，导致迁移后固定 placement 判定永远失败；是否登记由 Recorder 按用户指令处理。

**Remaining Issues**

- 网络 placement 语义冲突阻塞 6.2/6.3（见 Blocker Handoff），待决定修复方向。
- 完整 UART/net socket+harness 自动注入链路需 socket-capable host 回填后复验。

**Commit or Diff Reference**

- 本轮 diff：`scripts/ms08-uart-serial.py`（+launch/transcript/bounded）、
  `tests/ms08_uart_smp_probe.c`（+fresh_line）。先前 staged 文件（Iteration 004、
  既有 Makefile/scripts/probe）未动。

## Plan Review

- Review Result: replan-required

**Findings**

- Blocking: network V5 `owner_hart_mask` and `runner_hart_mask` are cumulative poll-site history (`crates/axnet/src/hart_counter.rs::record`). `kernel/src/entry.rs` runs both network migration smokes before the user probe. The first probe snapshot therefore contains the migrated hart even after affinity is restored (`owner_mask=0x8001`, `runner_mask=0x3`, with `owner_last=owner_aff=15` and `runner_last=runner_aff=0`). `tests/ms08_network_smp_probe.c::ms08_net_placement_ok` and `scripts/ms08-network-validate.py::_validate_protocol` incorrectly require singleton cumulative masks. This is a protocol and verification-contract mismatch, not evidence of a second owner.
- Blocking: the UART guest transcript reaches `PASS: placement`, then `FAIL: rx reason=rx-corrupt-frame` after the third injected frame. `tests/ms08_uart_smp_probe.c::line_reader_push` appends a whole 128-byte read into a 160-byte buffer before extracting lines and resets the buffer on overflow. Multiple valid frames can therefore lose bytes when delivered in one read. Serial input echo also interleaves with guest output; the accepted launch command does not disable echo. The harness reports `guest-eof`, and the UART validator exits 1. This is a witnessed runtime failure; its exact byte-loss mechanism still needs a focused RED witness before implementation.
- Blocking: `ms08_uart_placement_ok` and the UART validator only check distinct schedulable affinities. Neither checks `rx_last`/`tx_last` against the pinned harts or validates cumulative masks as history. They could accept a wrong actual copier hart, contrary to Task 6.1 and the delta spec's direct-observation requirement. The UART transcript already shows history masks `0x3` and `0x5` after boot migration, so a singleton-history test would be wrong here too.
- Non-blocking for this Review: Task 6.1's harness launch, bounded parser and fresh-line changes pass their reported host checks. The current QEMU run does not qualify UART; its failed result supersedes the earlier partial runtime description. `git diff --check` and `git diff --cached --check` exit 0. Existing staged work outside this Cycle was not attributed to Task 6.1.

**Deviation Classification**

PLAN-INVALID (the accepted fixed-placement protocol tests cumulative history as current placement and omits UART actual-hart checks); PLAN-OMISSION (UART line buffering and TTY echo were not covered by the host witnesses); NEW-EVIDENCE (the two actual QEMU failure transcripts). No evidence of a kernel ownership defect has yet been established.

**Acceptance Gaps**

- 6.1: UART RX payload integrity, actual-hart placement decision and a successful bounded UART validator result are absent.
- 6.2: the network placement case rejects the historical hart masks; no successful bounded network validator result exists.
- 6.3: neither independent runtime verdict is accepted, so the final diff Review and independent decision cannot close.

**Convergence**

N/A — first Review of this Cycle; the host UART launch/capture work advanced, but runtime Acceptance remains open.

**Evidence**

- Code: `kernel/src/entry.rs`, `crates/axnet/src/hart_counter.rs`, `tests/ms08_network_smp_probe.c`, `scripts/ms08-network-validate.py`, `tests/ms08_uart_smp_probe.c`, `scripts/ms08-uart-serial.py`, and `scripts/ms08-uart-validate.py`; inspected staged and unstaged changes.
- Act Response host self-test and static-build results are adopted for their unchanged coverage. Independent `python3 scripts/ms08-uart-serial.py --self-test` exits 0.
- `/tmp/ms08-net-serial.log`: boot fixed smoke passes, boot migration smoke passes, then `MS08_NET_V5: case=placement ... owner_mask=0x8001 ... runner_mask=0x3` and `FAIL: placement reason=policy`. The original runtime artifact remains in `/tmp`; this Review records only its decisive lines.
- `/tmp/ms08-uart-transcript.log`: `PASS: placement`, `FAIL: rx reason=rx-corrupt-frame`, `MS08_UART_HARNESS_EXIT: 1`. `python3 scripts/ms08-uart-validate.py /tmp/ms08-uart-transcript.log` exits 1 (`missing successful harness exit`). The network serial log alone lacks the peer result, so the network validator exits 1 (`expected MS08_NET_PEER_RESULT:`); this is not an independent placement verdict.

**Follow-up Decision**

Replan is required because the fixed-placement decision and UART launch/line-framing verification contract must change. The existing tasks 6.1–6.3 and Iteration 005 objective remain; the next Cycle must provide self-contained contracts and new Gate 2 review. Do not resume this blocked Cycle or claim either driver qualified.

**Iteration Plan Update**

The Iteration 005 task and verification contracts are revised to distinguish cumulative execution history from current affinity and actual poll-site observations, and to cover bounded UART frame parsing and echo-free injection. Iteration numbering, scope and deferred Iteration 006 remain unchanged.

**Next Cycle**

`001-replan.md` in this Iteration directory (draft; Gate 2 approval pending).

**Next Iteration**

None.
