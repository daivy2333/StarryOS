# Iteration 009: Final Sandbox Rerun and QEMU Runtime Evidence

## Plan Context

- Status: awaiting-gate-2
- Round: 009
- Parent: `008-probe-decision-closures-and-automatic-gates.md`

**Objective**

完成 MS04 最后一轮验收：先补齐 iteration 008 的 Evidence provenance 与 raw-log Gate
边界，再由用户在 sandbox 外复跑两项 R44 `ENV-BLOCKED` 命令，最后在单 hart、单
VirtIO-MMIO NIC 的 QEMU 中手工执行 MS04 snapshot/idle/nudge/burst、MS03 IRQ/UART、
MS02 TCP/UDP 和 MS01 socket 回归。所有原始输出进入 change-local Evidence；通过后
T1-T8 全部完成，不再创建常规后继 iteration。

这是计划中的最终 iteration，不含产品实现。若 runtime 暴露产品错误、证据中断或范围
假设失效，本轮停止并返回 Plan；“最终”不构成失败豁免，也不排除故障所必需的后继轮。

**Approved Requirements**

- 沿用 Gate 1 已批准的 R1-R8、M1-M4 与 D1-D10，无 Missing 或 Simplified requirement。
- R44 硬边界不变：用户逐条输入 guest 命令；agent、脚本、pipe、pexpect 不驱动 QEMU
  console。终端录制工具只记录用户交互，不自动发送 guest 输入。
- T8.1 必须先解决 real UDP loopback EPERM 与 static probe SIGSYS 两项交接；失败、
  中断、缺 artifact 或无法区分环境/产品原因时不得进入 QEMU。
- T8.2 只证明当前单 hart、单 VirtIO-MMIO、QEMU user-net 环境；不外推 SMP、PCI、
  DWMAC、真板 DMA/coherency 或物理性能。
- iteration 008 raw logs 保持原样。源码/文档 whitespace 与 raw-log integrity 分开；
  provenance addendum 必须解释采集 HEAD `e0fac50`、Act 基线 `78e1f7a` 与最终 revision。
- 用户本次要求把 Review 问题并入原工作，并在没有大问题时令下一轮成为最终轮；这为
  final scope 提供需求来源，不构成 Gate 2 实施批准。

**Scenario Sketch**

1. **Evidence provenance closure**：008 raw log 已在 index，包含终端空格；路径限定的
   source/document whitespace checks PASS，raw log 以存在、非空、hash、时间范围检查；
   009 addendum 分别记录三个 revision 角色。若仍把 raw 空格报成产品失败，或把不同
   revision 合并为一个 HEAD，FAIL。
2. **Sandbox rerun**：用户环境允许 UDP socket 与 musl compiler；real-loopback 精确完成
   96 packets，MS03/MS04 static probes fresh build 并记录 file/size/hash。EPERM/SIGSYS、
   编译错误、旧 artifact、缺日志或非零退出均停止。
3. **Boot and platform gate**：显式 QEMU 命令以 `virt`、1 GiB、`-smp 1`、单
   `virtio-net-device`、user-net 和现有 ext4 image 启动；完整串口显示 UART IRQ 10、
   VirtIO-MMIO net validation、IRQ 7 registration 和 shell prompt。任一更低层失败时不
   运行 workload。
4. **MS04 quiet and wake paths**：启动后 snapshot 为 Active/AsyncOwned 且 boot-history
   safety counters 为零；idle 无额外 IRQ/software/descriptor/budget/backpressure 进度；
   nudge 只增加 software/task/empty 各一次。任何 FAIL marker、timeout 或额外 delta 停止。
5. **MS04 burst/fairness**：host production stimulus 先监听 `0.0.0.0:15556`，guest 执行
   无参数 `burst`；精确接收 96 packets，reaped=refilled，IRQ/task 推进，budget exhaustion
   与 self-yield 可见。host 或 guest 协议错误、丢包、守恒失败或无 yield 均停止。
6. **MS03 regression**：在 async owner 已激活的同一 boot 中执行 idle、uart、rx2、tx2、
   both、repeat rx2；每个模式有 PASS marker，IRQ ACK/重复投递与 UART 隔离成立。
7. **MS02/MS01 compatibility**：MS02 service 完成两次独立 TCP 和一次 UDP，输出
   `MS02_COMPLETE tcp=2 udp=1`；MS01 输出完整 START/END、14 个 PASS、零 FAIL。任何
   短会话、缺 marker 或端口刺激不完整均失败。
8. **Final safety and evidence**：所有回归后再次运行 MS04 snapshot；仍为 Active、无
   fault/restore/IRQ-entry violation。完整 serial 与分项日志、命令、环境、hash 和判定均
   存在；中断或只保留摘要时本轮未完成。

Gate 1 沿用 proposal 中 2026-08-09 的 approved Requirements and Scope；本轮没有新增
capability、产品接口或需求裁剪。

**Current Baseline**

- Branch/HEAD：`net-k3` / `78e1f7abfa1614c188a24ebe7150ffb7c71e46d0`，实现、测试、
  OpenSpec 与 008 Evidence 位于 index/working tree；change 初始 revision 为
  `16d9a16a2b65a574022faaee39b465f6f7aebd45`。执行者不得 reset、checkout 或覆盖现有层。
- Change progress：22/25 tasks；T1-T7.3 已完成，T7.3R、T8.1、T8.2 待执行。
- Iteration 008 产品 Review：0 unresolved Critical/Important/Minor。独立复验 host 子项、
  MS16、axnet 109、scoped fmt、strict validation 与 artifact hashes 均 PASS。
- Automatic artifacts：D1 ELF/bin 分别 478,672/159,936 bytes；QEMU ELF/bin 分别
  46,894,056/40,046,784 bytes，hash 已在 008 Evidence 中复核。
- R44 handoff：`python3 scripts/ms04_rx_stimulus.py --loopback-self-test` 在 socket creation
  EPERM；`make tests/ms03_irq_probe tests/ms04_rx_probe` 在 musl GCC SIGSYS。旧 MS03
  binary 不合格，MS04 binary 不存在。
- Evidence Review：working-tree `git diff --check` PASS；cached/full-range checks 只因
  staged raw `automatic-gates.log` 的 ANSI/CRLF terminal whitespace exit 2。008
  `environment.txt` 记录 `e0fac50`，README/Act Response 记录 `78e1f7a`。

**Current-State Evidence**

| Boundary | Evidence |
|---|---|
| probe decisions | `tests/ms04_rx_probe.c` + 10 C tests：absolute safety、exact matrices、deadline-first、terminal helpers |
| runtime protocol | guest connects `10.0.2.2:15556`；host `scripts/ms04_rx_stimulus.py` production `serve_once` sends fixed 96×64 |
| MS04 activation | kernel validates VirtIO-MMIO net, registers IRQ 7, then calls unique `start_rx_task`; snapshot lifecycle=2/owner=1 is runtime witness |
| MS03 payload | `tests/ms03_irq_probe.c` uses V1 ioctl and host server `10.0.2.2:15555`; modes idle/uart/rx2/tx2/both |
| MS02 payload | `tests/ms02_guest_service.c` binds TCP+UDP 5555 and exits only after two TCP round trips plus one UDP |
| MS01 payload | `tests/ms01_socket_baseline.c` emits START/END and 14 case markers |
| emulator contract | R44/R48 + current kernel strings：QEMU virt、1 hart、VirtIO-MMIO net、UART IRQ 10、NET IRQ 7、manual shell |
| evidence defect | staged 008 raw log alone triggers cached/range whitespace check；revision labels disagree without invalidating full implementation range |

**Relevant Code and References**

| File / symbol | Responsibility in this iteration |
|---|---|
| `scripts/ms04_rx_stimulus.py::{loopback_self_test,serve_once,main}` | T8.1 real-loopback and T8.2 production burst server |
| `tests/ms04_rx_probe.c::{run_snapshot,run_idle,run_nudge,run_burst}` | guest MS04 runtime verdicts |
| `tests/ms03_irq_probe.c` | V1 IRQ/UART compatibility modes |
| `tests/ms02_guest_service.c` | two TCP + one UDP compatibility ledger |
| `tests/ms01_socket_baseline.c` | 14-case socket regression |
| `kernel/src/drivers/virtio_net_irq.rs::init` | platform validation、IRQ registration、async task start logs |
| `Makefile::tests/ms03_irq_probe,tests/ms04_rx_probe` | fresh RISC-V static payloads |
| R44 `.claude/runbooks/qemu-network-testing.md` | manual-console and ENV classification policy |
| R45/R48 runbooks | MS02/MS03 interaction details；current source/this iteration overrides stale counts or startup wording |
| 008 Evidence + Plan Review | automatic baseline、raw logs、hashes、handoff and provenance discrepancy |

**Critical Path**

```text
T7.3R evidence addendum + source/document diff checks
  -> user T8.1 real-loopback PASS
  -> user T8.1 fresh MS01-MS04 payload builds + hashes PASS
  -> QEMU boot/platform/IRQ lower-layer signatures PASS
  -> MS04 snapshot -> idle -> nudge -> host stimulus + burst PASS
  -> MS03 idle/uart/rx2/tx2/both/repeat PASS
  -> MS02 TCP#1/TCP#2/UDP COMPLETE -> MS01 14/14 PASS
  -> final MS04 snapshot safety PASS
  -> Evidence completeness + scope review
  -> Plan Review; no automatic archive or Maintainer
```

**Behavioral Change**

本轮不改变产品行为。可观察状态只从“自动 Gate 完成、两项 ENV-BLOCKED、无 runtime
Evidence”变为“环境复跑 PASS、QEMU runtime 分层 PASS、required Evidence 完整”。
若任何 runtime 结果失败，产品状态不变，iteration 保持未完成并保存第一失败层。

**Change Surface**

| Task | Requirement / Scenario | Surface | Planned action |
|---|---|---|---|
| T7.3R | R8 / S1 | 009 Evidence index/addendum、diff commands | reconcile revision roles and split raw-log/source whitespace Gates |
| T8.1 | R8 / S2 | host terminal、stimulus、Makefile payload targets | rerun two ENV blockers; build and hash all fresh guest payloads |
| T8.2-a | R3,R6,R8 / S3-S5 | QEMU + MS04 probe/stimulus | boot lower-layer Gate, then quiet/nudge/burst/final safety witnesses |
| T8.2-b | R3,R8 / S6 | MS03 probe + host TCP server | IRQ/UART/repeat compatibility in async-active boot |
| T8.2-c | R7,R8 / S7 | MS02/MS01 payloads + host clients | two TCP + UDP ledger and 14 socket regressions |
| T8.2-d | all / S8 | final Evidence + Review | verify completeness, markers, counters, revisions and claim boundary |

**Task Contracts**

T7.3R — Evidence boundary and provenance addendum:

- Agent-executable before the user boundary. Do not modify product source or rewrite iteration 008
  raw logs. Record the captured environment HEAD `e0fac50`, Act/final implementation base
  `78e1f7a`, initial range `16d9a16...`, final working-tree/index state and timestamps as separate
  fields in 009 Evidence.
- Run strict OpenSpec validation and whitespace checks over source, tests, scripts and Markdown while
  excluding `openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/**` raw payloads. Check every
  008 Evidence file for existence/nonzero size and SHA-256 separately.
- GREEN: path-limited checks exit 0, every raw file is indexed and provenance has no overloaded HEAD
  label. Record the unfiltered cached/range exit 2 as reviewed raw-log behavior, not PASS.
- Stop: any non-Evidence path fails whitespace validation, an 008 file is missing/empty, hash cannot
  be computed, or revision range cannot be reconstructed.

T8.1 — User sandbox rerun and fresh payload qualification:

- Depends on T7.3R GREEN. This is an authorization/capability boundary: Act must provide the exact
  handoff and wait for user-supplied command logs; it must not emulate success inside the sandbox.
- User runs the exact commands under Verification outside the restricted sandbox. The loopback test
  must exit 0 with 96-packet/protocol/sequence/bounded PASS. Static MS03/MS04 builds must exit 0 and
  produce fresh RISC-V static executables. MS01/MS02 payloads are rebuilt in the same batch.
- Record environment differences, command timestamps/exits, `file`, byte size and SHA-256 for all
  four payloads. Old `tests/ms03_irq_probe` is never reused as a PASS witness.
- Stop: EPERM/SIGSYS persists, any compiler diagnostic appears, output is stale/absent/non-RISC-V,
  loopback protocol fails, or user output is partial. Do not start QEMU.

T8.2-a — Manual QEMU lower layers and MS04 runtime:

- Depends on T8.1 PASS. User starts one explicit `qemu-system-riscv64` session with `virt`, 1 GiB,
  `-smp 1`, existing kernel/image, one VirtIO-MMIO NIC and user-net. A terminal recorder may capture
  bytes, but the user enters every guest command one at a time after the prompt.
- Lower-layer Gate: boot reaches `starry:~#`; serial contains UART IRQ 10 registration, VirtIO-MMIO
  net validation at the selected descriptor and IRQ 7 handler registration/start message. Failure
  here stops all guest workloads.
- Download fresh payloads over the host HTTP server; no rootfs mount/copy is required. Run snapshot,
  idle and nudge. Start the production host stimulus before guest `burst`, then run `burst` with no
  extra argument. Run a final snapshot after all MS03/MS02/MS01 regressions.
- GREEN: every recognized mode has one PASS terminal marker; lifecycle=2, owner=1; safety counters
  are zero; idle/nudge exact matrices pass; burst receives 96, reaped=refilled, IRQ/task/budget/yield
  progress is visible. The final snapshot proves no later boot-history fault.
- Stop: duplicate/missing marker, timeout, inactive owner, safety counter, conservation failure,
  no budget/yield, protocol error, dropped packet or interrupted serial capture.

T8.2-b — MS03 IRQ/UART regression:

- Depends on the initial MS04 batch PASS and uses the same boot. Start a host TCP server on 15555.
  User runs `idle`, `uart`, `rx2`, `tx2`, `both`, then a second `rx2`, supplying the two response
  lines required by receive modes.
- GREEN: every mode emits its PASS marker; repeat rx2 again shows used-ring and ACK progress; UART
  mode does not fabricate net used progress; no MS04 fault appears in the later final snapshot.
- Stop: old R48 “polling fallback active” startup string is required, server/port is wrong, any mode
  fails, or repeat delivery cannot be observed. MS04 runtime uses current async-start wording.

T8.2-c — MS02 and MS01 compatibility:

- Depends on MS03 PASS. Run the fresh MS02 service in guest. User performs two separate host TCP
  sessions, entering `MS02_TCP_REQUEST` in each, plus one UDP request. Do not use a timeout that kills
  an interactive TCP connection before the response.
- GREEN: both `MS02_TCP_PASS connection=1|2`, `MS02_UDP_PASS datagrams=1`, and
  `MS02_COMPLETE tcp=2 udp=1` appear. Then MS01 emits START/END, exactly 14 PASS and zero FAIL.
- Stop: only one TCP round trip, missing host response, guest service remains blocked, any FAIL,
  reused old log or incomplete marker set.

T8.2-d — Final Evidence and scope review:

- Depends on every manual case PASS. Preserve the complete serial recording; derive separate MS04,
  MS03 and MS01/MS02 logs without deleting context from the raw serial. Record commands, terminal
  roles, timestamps, exits/interruption status, hashes and per-case decisions.
- Validate required file list, OpenSpec strict checks, source/document path-limited whitespace and
  complete range review. Runtime evidence is checked before code quality; no unresolved
  Critical/Important finding or partial case may remain.
- GREEN: all required Evidence files exist and map to T7.3R/T8.1/T8.2; README limits conclusions to
  this QEMU contract. Act Response may mark `reported` only after user logs are supplied and checked.
- Stop: QEMU console was automated, raw serial is missing/truncated, extraction disagrees with raw
  log, scope is overclaimed, or any pass condition relies only on a summary.

**Invariants**

- ISR remains cause/ACK/telemetry/fixed wake only; descriptor service stays in the unique task.
- V1 is 64 bytes, V2 is 224 bytes, nudge is generation-neutral, budget is 32.
- Active/Faulted retains async ownership; no polling/async double consumer.
- EVENT_IDX remains enabled; synchronous TX、10 ms protocol polling、UART and early/panic console
  remain available.
- Guest commands are manual and one per prompt. Host stimulus may be scripted but never drives the
  guest console.
- No old artifact/log is substituted. QEMU evidence is not hardware, SMP or performance evidence.

**Non-goals**

- Product code、test implementation、new ioctl/ABI、rootfs mutation、automatic QEMU harness。
- TAP/pcap、MS02 full TAP/ICMP baseline、performance benchmark、multi-hart、PCI、DWMAC、真板。
- Cleaning vendor warnings, raw terminal whitespace or unrelated staged/worktree content。
- Creating iteration 010 in advance、Maintainer、Recorder、SNAPSHOT/tasks global sync、archive、commit。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Runtime / Evidence Witness | Simplification | Status |
|---|---|---|---|---|---|---|
| R8 evidence integrity | S1,S8 | D10 | T7.3R,T8.2-d | provenance addendum + path-limited diff + raw hashes | None | Covered |
| R8 environment boundary | S2 | D10/R44 | T8.1 | loopback PASS + fresh four payload hashes | None | Covered |
| R3 platform/IRQ | S3,S6 | D3,D5,D9 | T8.2-a,b | boot signatures + MS03 repeat/ACK/UART | None | Covered |
| R2/R6 quiet wake | S4 | D5,D7,D9 | T8.2-a | snapshot/idle/nudge exact markers and deltas | None | Covered |
| R4/R5/R6 RX service | S5 | D2,D4-D7,D9 | T8.2-a | 96-packet burst、reaped=refilled、budget/yield | None | Covered |
| R7 compatibility | S7 | D8,D9 | T8.2-c | MS02 two TCP + UDP；MS01 14/14 | None | Covered |
| R1 transport boundary | S3,S5 | D1,D2,D9 | T8.2-a | VirtIO-MMIO boot + EVENT_IDX-backed runtime progress | None | Covered |
| M1-M4 recovery/scope | S3-S8 | D3,D4,D8-D10 | T8.2-d | final safety snapshot + scope-limited README | None | Covered |

No requirement is Missing or Simplified. R45 的 TAP/ICMP、pcap 和 idle CPU characterization
不属于当前 T8 批准范围，`SKIPPED: MS04 change 只要求 MS02 TCP/UDP compatibility`。
自动 QEMU harness `SKIPPED: R44 prohibits automated guest-console driving`。

**Acceptance**

- T7.3R 解释 008 revision/whitespace 偏差，非 raw-Evidence diff checks 全部 exit 0。
- T8.1 两个 ENV-BLOCKED 原命令在用户环境 exit 0；四个 payload fresh、static、hashed。
- QEMU lower-layer signatures、所有 MS04/MS03 modes、MS02 2×TCP+UDP、MS01 14/14 PASS。
- Burst 96 packets、reaped=refilled、budget exhaustion/self-yield 可见；所有 safety counters
  在最终 snapshot 仍为零。
- Required Evidence 文件完整、可追踪、无旧日志替代；结论仅限单 hart VirtIO-MMIO QEMU。
- 失败时记录第一失败层并保持任务未完成，不创建完成或归档结论。

**Verification**

Agent/Act 在用户边界前执行：

```text
openspec validate ms04-qemu-async-rx-queue-baseline --strict
openspec validate references --strict
git diff --check -- . ':(exclude,glob)openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/**'
git diff --cached --check -- . ':(exclude,glob)openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/**'
git diff 16d9a16a2b65a574022faaee39b465f6f7aebd45 --check -- . ':(exclude,glob)openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/**'
find openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/008-probe-decision-closures-and-automatic-gates -type f -size 0 -print
sha256sum openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/008-probe-decision-closures-and-automatic-gates/*
```

用户在 sandbox 外执行 T8.1，并把完整输出交回：

```text
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/ms04_rx_stimulus.py --loopback-self-test
make tests/ms03_irq_probe tests/ms04_rx_probe
riscv64-linux-musl-gcc -static -O2 -o tests/ms02_guest_service tests/ms02_guest_service.c
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c
file tests/ms03_irq_probe tests/ms04_rx_probe tests/ms02_guest_service tests/ms01_socket_baseline
stat -c '%y %s %n' tests/ms03_irq_probe tests/ms04_rx_probe tests/ms02_guest_service tests/ms01_socket_baseline
sha256sum tests/ms03_irq_probe tests/ms04_rx_probe tests/ms02_guest_service tests/ms01_socket_baseline
```

Terminal A — HTTP server，先启动并保持运行：

```text
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

Terminal B — 用户手工 QEMU，会话必须完整录制；以下 `script` 只录制，不提供输入：

```text
cd /home/daivy/projects/serial/work/StarryOS
script -q -f openspec/changes/ms04-qemu-async-rx-queue-baseline/evidence/009-final-sandbox-rerun-and-qemu-runtime/qemu-serial.log -c 'qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin -m 1G -smp 1 -device virtio-blk-device,drive=disk0 -drive id=disk0,if=none,format=raw,file=make/disk.img -device virtio-net-device,netdev=net0 -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 -nographic'
```

Guest shell — 每条等 prompt 后由用户逐条输入：

```text
wget -q -O /tmp/ms04_rx_probe http://10.0.2.2:18765/ms04_rx_probe
wget -q -O /tmp/ms03_irq_probe http://10.0.2.2:18765/ms03_irq_probe
wget -q -O /tmp/ms02_guest_service http://10.0.2.2:18765/ms02_guest_service
wget -q -O /tmp/ms01_socket_baseline http://10.0.2.2:18765/ms01_socket_baseline
chmod +x /tmp/ms04_rx_probe /tmp/ms03_irq_probe /tmp/ms02_guest_service /tmp/ms01_socket_baseline
/tmp/ms04_rx_probe snapshot
/tmp/ms04_rx_probe idle
/tmp/ms04_rx_probe nudge
```

Terminal C — 在 guest burst 前启动，看到监听进程等待后再回 guest：

```text
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556
```

Guest shell：

```text
/tmp/ms04_rx_probe burst
```

MS03 的 Terminal C 改为 `nc -l -p 15555 -k`；guest 逐条运行，rx2/both 时用户在 host
server 提供所需回应：

```text
/tmp/ms03_irq_probe idle
/tmp/ms03_irq_probe uart
/tmp/ms03_irq_probe rx2
/tmp/ms03_irq_probe tx2
/tmp/ms03_irq_probe both
/tmp/ms03_irq_probe rx2
```

MS02 guest 启动 `/tmp/ms02_guest_service`。Terminal C 先后打开两次独立
`nc 127.0.0.1 5555`，每次手工输入 `MS02_TCP_REQUEST` 并确认响应；再运行：

```text
echo 'MS02_UDP_REQUEST' | nc -u -w1 127.0.0.1 5555
```

MS02 完成后，guest 逐条运行：

```text
/tmp/ms01_socket_baseline
/tmp/ms04_rx_probe snapshot
```

用户以 `Ctrl-A X` 退出 QEMU、`Ctrl-C` 停止 host servers。任何命令中断必须记录，不能
以重跑后的摘要覆盖第一次失败；允许开始全新、有独立日志的 clean rerun。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R1-R8/M1-M4；无新增 capability 或 Simplified row |
| Investigation | PASS | current probe/stimulus/MS03/MS02/MS01 source、kernel logs、R44/R45/R48、008 Evidence inspected |
| Design | PASS | evidence-only closure、ENV-first、lower-layer-first、manual console、single-session runtime order fixed |
| Task Contracts | PASS | T7.3R → T8.1 → T8.2-a/b/c/d has dependencies, pass/fail and stop conditions |
| Traceability | PASS | RTM maps final scenarios to design/tasks/runtime witnesses；no Missing/TBD |
| Verification | PASS | exact host/QEMU/guest commands、terminal roles、markers、counters and artifact checks listed |
| OpenSpec consistency | PASS | design D10、delta Evidence scenario、tasks 7.3R/8.1/8.2 and this iteration agree |
| Persisted Evidence | PASS | required files、sources and pass conditions fixed below |
| Manual boundary | PASS | agent stops before user T8.1/T8.2 and resumes only to inspect supplied evidence |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval；Act and manual execution are not authorized |

**Persisted Evidence**

- Mode: required
- Root: `evidence/009-final-sandbox-rerun-and-qemu-runtime/`

Plan does not create this directory. Act may create the index/provenance skeleton for T7.3R；用户
提供 T8.1/T8.2 raw outputs；Act 在恢复后核验并填写最终判定。

| File | Gate | Required content | Pass condition |
|---|---|---|---|
| `README.md` | T8.2-d | revision/scope、file index、per-task/case result、interruption status | every case final PASS；single-hart QEMU claim only |
| `environment.txt` | T8.1/T8.2 | host/QEMU/Rust/C tools、sandbox difference、machine/memory/hart/NIC/rootfs | enough to reproduce and classify each result |
| `provenance.txt` | T7.3R | initial range、008 capture HEAD、Act base、final revision/index/worktree、raw-log Gate rule | every revision role and timestamp unambiguous |
| `commands.txt` | all | exact terminal role、command、start/end、exit or manual completion | no missing command or automated guest input |
| `sandbox-rerun.log` | T8.1 | real-loopback and original static-probe rerun complete output | both original ENV blockers exit 0 |
| `build.log` | T8.1 | fresh MS01-MS04 payload builds、file/stat output | four fresh static RISC-V payloads qualified |
| `artifacts.sha256` | T8.1/T8.2 | QEMU kernel + four payload size/hash and producer | files used by QEMU match recorded hashes |
| `qemu-serial.log` | T8.2 | raw boot through final snapshot and session termination | uninterrupted lower layers + all workload markers |
| `ms04-probe.log` | T8.2-a | initial/final snapshot、idle、nudge、burst PRE/POST/DELTA/terminal | exact matrices、96 packets、conservation、budget/yield、zero safety fault |
| `ms03-regression.log` | T8.2-b | six ordered mode outputs | every PASS；repeat IRQ/ACK and UART isolation |
| `ms01-ms02-regression.log` | T8.2-c | MS02 two TCP+UDP ledger；MS01 START/END 14 PASS | complete markers；zero FAIL |
| `final-review.md` | T8.2-d | evidence completeness、spec/runtime、full-range/scope findings | zero unresolved Critical/Important；Minor classified |

**Risks and Notes**

- User-net UDP burst may expose packet loss or timing behavior absent from the memory test；that is a
  runtime product result, not an environment waiver after QEMU starts successfully。
- MS03 R48 startup wording predates async activation。Use current kernel strings and V1 probe markers;
  do not require the obsolete “polling fallback active” success line。
- MS02 source requires two TCP sessions。A single successful nc plus UDP leaves the guest blocked and
  is incomplete even if the older runbook summary appears satisfied。
- `script` may add terminal control bytes。Preserve them in raw serial；source/document whitespace
  checks exclude Evidence, and derived logs remain traceable to the raw file。
- Completion is a QEMU functional baseline, not throughput, physical timing, true DMA coherency or SMP
  proof。QEMU/TAP and true-board evidence classes remain distinct。
- After Act reports, Plan must independently review code/evidence before `no-follow-up`。Archive and
  global status updates require a separate `openspec-docs-maintainer` request。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-12
- User instruction: “批准”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS。Iteration 009 已达到执行
  就绪状态，可由后续明确的 `openspec-act` 请求开始 T7.3R。
- Scope: 批准范围包括 T7.3R 的 Evidence provenance/raw-log Gate 修正、T8.1 的用户
  sandbox 外复跑与 fresh payload qualification，以及 T8.2 的单 hart VirtIO-MMIO QEMU
  手工 MS04/MS03/MS02/MS01 runtime Evidence。批准不启动 Act 或 QEMU，不代替用户
  T8.1/T8.2 操作，也不授权产品修复、iteration 010、Maintainer、Recorder 或归档。

## Act Response

- Status: reported

**Implemented**

- T7.3R：重建 `16d9a16a`、`e0fac50c`、`78e1f7ab` 与当前 `8f5b5228` 的 revision
  角色；分离源码/文档 whitespace Gate 与原始 Evidence 完整性 Gate。
- 创建全部 12 个 iteration 009 required Evidence 文件。T7.3R 文件已写入新鲜结果；
  T8.1/T8.2 文件包含可直接替换或由 `script -f` 覆盖的 `PENDING` 采集位置。
- 更新 change Evidence 索引和 change-local task 7.3R 状态；T7.3R 本身未修改产品代码。
- 按用户限量豁免修复 T8.1 暴露的 musl 可移植性错误：`ms04_rx_probe.c` 显式包含
  `<sys/time.h>`；host harness 增加禁止依赖 libc 间接包含的永久 guard。
- 审核用户提供的 QEMU serial：提取 MS04、MS03、MS02 分项 Evidence；确认 MS04
  snapshot/idle/nudge/burst 核心 runtime Gate PASS，并按用户授权记录未运行项和声明边界。

**Changed Files and Symbols**

- `evidence/README.md`：登记 iteration 009 Evidence。
- `evidence/009-final-sandbox-rerun-and-qemu-runtime/*`：T7.3R provenance、命令和环境，
  以及 T8.1/T8.2 原始日志、hash、分项日志和最终 Review 的预创建采集文件。
- `tasks.md`：勾选 7.3R，并记录新鲜 Gate 结果和未过滤 cached/range 的实际差异。
- 本 iteration 的 `Act Response`：记录手工测试能力边界和恢复条件。
- `tests/ms04_rx_probe.c`：补充 `struct timeval` 的权威定义头文件。
- `tests/ms04-async-rx-host-harness.rs`：增加显式头文件 source guard。

**Deviations from Plan**

- Plan 预期未过滤 cached/range whitespace checks 均 exit 2。执行时 008 Evidence 已进入
  commit `8f5b5228`，所以 cached check exit 0，完整 initial revision range 仍 exit 2。
  该基线变化不改变 raw-log Gate，也未改写 008 Evidence。
- 用户要求在手测前预创建全部文件。计划已要求这些文件且模式为 `required`，因此只提前
  创建采集载体；其中的 `PENDING` 不计 Evidence PASS。
- 用户在最终审核时明确授权跳过未运行的重复/兼容性用例。Act 未把这些项目标记 PASS，
  而是记录为 `WAIVED/SKIPPED`，并把完成结论收窄到 MS04 核心单 hart VirtIO-MMIO
  异步 RX runtime baseline。

**Blocker Handoff**

- Discovered at: T8.1 / authorization and capability boundary after T7.3R Gate 5.
- Expected: T7.3R PASS 后由用户在 unrestricted environment 复跑 loopback/static build，
  再手工运行单 hart QEMU T8.2。
- Actual: T7.3R PASS；当前 agent sandbox 的既知 UDP `EPERM` 和 musl GCC `SIGSYS` 不能
  产生 T8.1 PASS，R44 又禁止 agent 驱动 guest console。
- Impact: T8.1/T8.2 required Evidence 尚未采集，8.1、8.2 和本 iteration 不得完成。
- Completed work: T7.3R；全部 required Evidence 文件已预创建。
- Partial work: T8.1/T8.2 只有采集说明和 `PENDING` 文件，无运行结果。
- Unstarted work: 用户 T8.1、T8.2-a/b/c；agent T8.2-d 最终 Evidence/diff Review。
- Worktree state: 只有 change-local tasks、iteration 和 Evidence 修改；产品源未改。
- Gates: Gate 3/4/5 for T7.3R PASS；T8.1 Gate 3 BLOCKED at user capability boundary。
- Evidence: EV-009-01、EV-009-04 至 EV-009-06 PASS；EV-009-02/03 PARTIAL；
  EV-009-07 至 EV-009-11 BLOCKED。
- Plan decision needed: None；这是获批计划中的预期用户边界。
- Resume condition: 用户完成 T8.1 并把完整输出写入预创建文件后明确要求继续；Act 先审核
  T8.1。只有 PASS 才进入 T8.2；T8.2 日志齐全后再次恢复完成 T8.2-d。
- Update after limited repair: 首次用户复跑中 loopback exit 0；static build exit 2，首个产品
  错误是 `struct timeval` incomplete type。修复后本地 host Gates 全部 PASS；sandbox 内
  musl build 已不再输出该诊断，只在既知 `Bad system call` 处 exit 2。等待用户环境 GREEN。
- Update after user clean rerun: 2026-08-12 17:14 外部 static probe rerun exit 0；MS01/MS02
  builds exit 0；四个 payload 的 ELF 类型、mtime、size 与 SHA-256 完整。长路径换行产生的
  `sandbox-` 意外文件已在保留原文后合并并删除。T8.1 Gate 5 PASS，阻塞点推进到 T8.2。

**Blocker Resolution**

- User instruction: “不用，如果这是一个小问题的话，请你直接解决，解决然后再给我命令行，
  没必要进行阻塞，你可以把这句话写入回复当授权记录”。
- Resolution: 用户授权在 iteration 009 内直接修复 T8.1 暴露的小型 guest probe
  可移植性缺口。范围限定为给 `tests/ms04_rx_probe.c` 补充 `struct timeval` 的权威头文件，
  并增加永久 source guard；不改变 probe 协议、runtime 判定、内核或异步 RX 产品行为。
- Accepted risk: 豁免本轮“runtime 产品错误返回 Plan”的停止边界，仅覆盖上述显式头文件
  修复。其他编译、产品或 QEMU runtime 失败仍按原停止条件处理。
- Resume point: T8.1 static payload build。
- Required verification: 复用用户 musl build exit 2 作为 RED；修复后运行 host syntax、
  probe decision tests、source guard、host-test 和可用环境中的交叉构建。若 sandbox 再次
  SIGSYS，则保留为用户同命令复跑边界，不计 GREEN。
- User instruction: “当前没有遇到fatal，至于少的几个测试我都觉得没必要重复进行了，我
  授权的，你看看，没有问题就填写回复”。
- Resolution: 审核现有 serial 后未发现 FAIL、fatal、panic 或安全计数异常；将缺失项逐项
  标为 `WAIVED/SKIPPED`，不扩大 PASS 声明，并完成 T8.2 Evidence Review。
- Accepted risk: 不取得 boot 原始签名、MS03 both/repeat、完整 MS01/MS02 compatibility、
  post-regression safety snapshot 或 termination metadata；这些能力不在本轮完成结论中。
- Resume point: T8.2-d Evidence and full-range review。
- Required verification: serial marker/counter review、derived-log traceability、完整 diff Review、
  strict validation 和 non-Evidence whitespace checks。

**Self-Review**

- Plan compliance: PASS WITH EXPLICIT USER WAIVER
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Spec review 先确认了 revision 角色、non-Evidence whitespace、raw file nonzero/hash 和手工
边界均符合 T7.3R contract；随后 code-quality/document review 确认没有产品修改、没有改写
008 raw log，且所有 `PENDING` 状态明确不构成 PASS。

限量修复的 Spec Review 确认只改变 guest probe 的编译可移植性，不改变 wire protocol、
timeout 值或 runtime verdict。Code Review 确认 `<sys/time.h>` 是 `struct timeval` 的直接
定义来源，source guard 会阻止回归。未解决 Critical/Important/Minor finding 仍为 0。

最终 Spec Review 逐项核对 serial：MS04 四模式 PASS；idle 与 nudge 精确矩阵成立；burst
96 个 completion 守恒且 budget/yield 可见；已记录的 fault/restore/IRQ-entry 均为零。
Code/Evidence Review 确认分项日志可追溯到 raw serial，缺失项目明确标记 waiver，结论未
外推到完整 compatibility、真板、SMP、PCI、DMA coherency 或性能。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 退出码 / 结论 |
|---|---|---|---|
| Change strict | `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | `Change ... is valid` | 0 / PASS |
| References strict | `openspec validate references --strict` | `Specification 'references' is valid` | 0 / PASS |
| Non-Evidence whitespace | working tree、cached、`16d9a16a..` 三项 path-limited checks | 无输出 | 0 / PASS |
| 008 completeness | zero-size find + SHA-256 | 无空文件；8 个 hash | 0 / PASS |
| Raw full range | 未过滤 `git diff 16d9a16a... --check` | 只报告 `automatic-gates.log` preserved terminal whitespace | 2 / REVIEWED，不计 PASS |
| User RED | sandbox 外 `make tests/ms03_irq_probe tests/ms04_rx_probe` | `struct timeval` incomplete type | 2 / RED |
| Timeval guard RED | 定向 Rust source guard | 1 failed，14 filtered | 101 / RED |
| Timeval guard GREEN | MS04 host harness | 15 passed，0 failed | 0 / PASS |
| Probe decisions | C decision suite | `10 passed` | 0 / PASS |
| Host syntax | 严格 C syntax check | 无输出 | 0 / PASS |
| Sandbox cross-build | `make tests/ms03_irq_probe tests/ms04_rx_probe` | 原 C 诊断消失；compiler `Bad system call` | 2 / ENV-BLOCKED |
| External clean build | 用户 `make -B tests/ms03_irq_probe tests/ms04_rx_probe` | `STATIC_PROBE_RERUN_EXIT=0` | 0 / PASS |
| Payload qualification | `file`、`stat`、`sha256sum` | 四个 fresh static RISC-V ELF；五个运行产物 hash 完整 | 0 / PASS |
| MS04 runtime | `qemu-serial.log` snapshot/idle/nudge/burst | 4 PASS；96/96/96；budget=2，yield=2；safety/fault=0 | PASS |
| MS03 partial regression | idle/uart/rx2/tx2 | 4 PASS；无 FAIL | PASS WITH WAIVER |
| MS02 partial regression | TCP connection 1 | `MS02_TCP_PASS connection=1` | PASS WITH WAIVER |
| Missing runtime cases | 用户明确授权跳过 | 逐项记录于 `final-review.md` | WAIVED/SKIPPED |

**Persisted Evidence**

`../evidence/009-final-sandbox-rerun-and-qemu-runtime/README.md`。EV-009-01、EV-009-04 至
EV-009-06、EV-009-08、EV-009-11 PASS；EV-009-02/03/07/09/10 PASS WITH WAIVER。

**Experience Candidates**

None。现有 QEMU 流程包含显式 waiver，不满足端到端 Runbook 候选门槛；普通 probe
可移植性修复不构成 Incident。

**Remaining Issues**

- Accepted scope 内无未解决问题。
- 未运行的完整 MS01/MS02/MS03 compatibility 与 post-regression safety 仍无 PASS 证据；
  这是显式接受的 Evidence 缺口，不应在后续文档中改写为已验证。

**Commit or Diff Reference**

T7.3R start HEAD: `8f5b5228747dc817a5a9de7a3461dccdf06e0c24`；本批为其上的
change-local worktree diff。

## Plan Review

- Status: completed — PASS WITH EXPLICIT USER WAIVER

**Review Result**

Plan 独立复核通过。实际产品 diff 只有 `ms04_rx_probe.c` 的直接 `<sys/time.h>` include
和 host harness 的永久 source guard；未发现 ownership、IRQ、descriptor、wire protocol 或
runtime verdict 变化。OpenSpec strict、15 个 MS04 Rust host tests、严格 C syntax、10 个
probe decision tests、96-packet stimulus self-test 和三层 non-Evidence whitespace checks
均 exit 0。

QEMU raw serial 与三个派生日志逐行一致：MS04 snapshot/idle/nudge/burst 四模式 PASS，burst
为 96 reaped = 96 refilled = 96 delivered，budget/yield 各为 2，已记录的 safety/fault 计数
为零；MS03 idle/uart/rx2/tx2 和 MS02 TCP#1 的已执行结果也与摘要一致。没有发现产品失败、
fatal、panic 或范围外推。

**Findings**

- Critical: 0。
- Important: 0。
- Minor: 0 个需要修复或后续执行的问题。
- Bounded observation: `artifacts.sha256` 在 17:16 记录 iteration 008 QEMU kernel hash
  `84b7b5c8...`，工作区 kernel 随后于 17:22 被重建，审计时 hash 为 `1fa4a11c...`；由于
  Terminal B 启动时间未记录，无法事后对已加载 binary 做 contemporaneous re-hash。这是
  已明确接受的 terminal timing metadata 缺口，不反证 raw serial，也不支持 exact-binary
  reproducibility 声明。当前结论仅保留为同一 revision/worktree 上的单 hart VirtIO-MMIO
  MS04 核心功能基线。

**Deviation Classification**

- `ACT-DEVIATION`：T8.1 首次外部构建暴露缺少 `<sys/time.h>`；用户明确授权限量修复，
  TDD RED/GREEN、外部 clean rebuild 和独立复核均通过，偏差已闭合。
- `ACT-DEVIATION`：T8.2 未完成原计划全部 compatibility/repeat/final-snapshot cases；用户
  明确承担风险并授权跳过。缺项均记为 `WAIVED/SKIPPED`，没有改写为 PASS。
- `NEW-EVIDENCE`：审计发现 kernel artifact 在 hash 采集后被重建；按上面的 bounded
  observation 收窄可复现性结论，不构成产品或核心 runtime Gate 失败。
- `PLAN-OMISSION`、`PLAN-INVALID`、`BASELINE-CHANGED`：None。

**Evidence**

- `evidence/009-final-sandbox-rerun-and-qemu-runtime/` 的 12 个 required 文件均存在且非空；
  raw serial SHA-256 复算为 `186ad7c39dc13831892faf2459a5071c7455244bcb66a8662c288062e859a4e7`。
- `qemu-serial.log` lines 9-28 对应 MS04 派生记录，lines 29-58 对应 MS03，lines 59-62
  对应 MS02 TCP#1；marker、delta 和 waiver 边界一致。
- 当前四个 guest payload 的 size/type/hash 与 qualified artifact set 一致；QEMU kernel
  的当前 hash 变化按 Findings 限制使用，不改写历史 Evidence。
- `openspec list` 显示 change 为 `Complete`；change-local tasks 为 25/25。

**Follow-up Decision**

`no-follow-up`。用户已明确说明本轮是测试轮次，并授权跳过剩余重复/兼容性项目；独立审计
未发现 unresolved Critical/Important、未获授权的 required Evidence 缺口、产品失败或范围
夸大。bounded artifact observation 已通过声明收窄处理，不值得单独创建 iteration 010。

**Next Iteration**

None。iteration 009 是本 change 的最后一个 implementation iteration；本次未调用 Act、
Maintainer、Recorder 或归档流程。
