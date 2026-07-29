# Iteration 002: manual verification boundary

## Plan Context

- Status: ready
- Round: 002
- Parent: `001-environment-ready`

**Objective**

完成 agent 可执行的实现验证和 diff Review。
把 payload 编译及 QEMU 运行证据交给用户。

**Background**

001 因 agent 终端不能执行 `riscv64-linux-musl-gcc` 而 blocked。
用户确认编译器在其终端可用，并要求手工完成 payload 编译和全部
QEMU 测试。

该事实修正能力边界，不改变需求、设计或产品实现。
000 和 001 的历史与 Evidence 保持不变。

**Current Baseline**

- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1`
- Branch: `net-k3`
- T1 deadline RED 已观察：缺少 `select_wake_deadline`，exit 101。
- Deadline GREEN 已观察：4/4 tests PASS。
- T2 已完成并在 change tasks 中勾选。
- T3 feature tree 已显示 `auto-icmp-echo-reply`。
- Payload 源码和 Makefile target 已存在，目标二进制尚未由用户编译。
- QEMU、pcap、CPU 和 runtime MS01 证据尚未采集。

**Current-State Evidence**

- `Device::requires_polling` 默认返回 `false`。
- `EthernetDevice::requires_polling` 在 `irq_num()` 为 `None` 时返回
  `true`。
- `Service::register_waker` 只为 mask 命中的 polling device 合并
  `now + 10 ms`。
- `select_wake_deadline` 选择 protocol 与 polling deadline 的较早值。
- `tests/ms02_guest_service.c` 使用一个 `poll()` loop 管理 TCP listener、
  active TCP connection 和 UDP socket。
- `crates/axnet/Cargo.toml` 已启用 smoltcp 自动 ICMP echo。

**Critical Path**

`accept/recvfrom/poll`
→ socket waker registration
→ `Service::register_waker`
→ protocol deadline 或 10 ms fallback
→ socket retry
→ `poll_interfaces`
→ VirtIO RX
→ ARP/IP/smoltcp
→ socket readiness 或 ICMP reply。

**Behavioral Change**

无 IRQ Ethernet waiter 最迟 10 ms 被 timer 唤醒。
IRQ device 与 loopback 行为不变。
smoltcp 自动回复 ICMP echo request。

用户负责证明 target payload 和 QEMU 端到端行为。
agent 不执行或自动驱动 QEMU。

**Change Surface**

| Task | Current State | Remaining Work |
|---|---|---|
| T1 | policy GREEN；payload source ready | 用户编译 payload 并提交结果 |
| T2 | complete | agent 在 full diff Review 中复核 |
| T3 | feature graph PASS | agent 完成 target build |
| T4 | not started | agent 完成静态 Gate；用户完成 runtime Gate |

**Task Contracts**

Agent batch:

- 运行 `cargo fmt --all -- --check`。
- 运行 axnet deadline tests。
- 查询 smoltcp feature tree。
- 运行 `make LOG=info build`。
- 运行 `python3 scripts/ms01-qemu-test.py --self-test`。
- 先做 spec review，再做 code quality review。
- 修复计划范围内的 Critical 和 Important 问题。
- 不执行 payload target compiler、QEMU、guest shell、pcap 或 CPU 采样。
- agent Gate 通过后，Act Response 使用 `reported`。
- 用户 Evidence 尚未提交只记录为 external verification pending，
  不写 blocker。

User batch:

- 编译 `tests/ms02_guest_service`。
- 手工执行 no-hostfwd、user-net 和 TAP QEMU。
- 手工输入 guest 和 host 命令。
- 保存 TCP/UDP、ARP/ICMP、CPU 和 MS01 runtime 证据。
- 编译或 runtime 功能失败时提交完整命令、输出和退出码。

**Manual Commands**

编译 payload：

```bash
cd /home/daivy/projects/serial/work/StarryOS
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
sha256sum tests/ms02_guest_service
```

启动 HTTP server：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

无 hostfwd QEMU：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0 \
  -nographic
```

User-net QEMU：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -object filter-dump,id=ms02user,netdev=net0,file=ms02-usernet.pcap \
  -nographic
```

Guest 手工输入：

```sh
wget -q -O /tmp/ms02_service \
  http://10.0.2.2:18765/ms02_guest_service
chmod +x /tmp/ms02_service
/tmp/ms02_service
```

Host TCP 手工输入两次：

```bash
timeout 5 nc 127.0.0.1 5555
```

每次输入：

```text
MS02_TCP_REQUEST
```

Host UDP 手工输入：

```bash
timeout 5 nc -u 127.0.0.1 5555
```

输入：

```text
MS02_UDP_REQUEST
```

TAP 准备：

```bash
ip route get 10.0.2.2
sudo ip tuntap add dev tap-ms02 mode tap user "$(id -un)"
sudo ip addr add 10.0.2.2/24 dev tap-ms02
sudo ip link set tap-ms02 up
sudo tcpdump -i tap-ms02 -nn -e -w ms02-tap.pcap
```

TAP QEMU：

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 \
  -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev tap,id=net0,ifname=tap-ms02,script=no,downscript=no \
  -nographic
```

Guest 手工输入：

```sh
nc -u -l -p 5555 >/tmp/ms02-icmp-wait.log 2>&1 &
echo MS02_ICMP_WAITER_READY
```

Host 手工输入：

```bash
ping -c 3 -W 2 10.0.2.15
```

确认设备名后清理：

```bash
ip link show tap-ms02
sudo ip link delete tap-ms02
```

空闲 CPU：

```bash
top -b -d 1 -n 30 -p <QEMU_PID> > idle-cpu.txt
```

**Invariants**

- 保持 M41 transport 边界与 D22 MMIO-first。
- 串口、probe、TCP、UDP、ARP、ICMP 和 CPU 分开取证。
- 不引入 IRQ、AtomicWaker、async queue 或多 waiter。
- 不把 QEMU 结果声明为真板、SMP 或性能优化证据。

**Non-goals**

- PCI、VF2、SMP、IRQ、PLIC 和 DMA。
- 自动 QEMU harness。
- raw ICMP socket。
- CPU、吞吐或延迟优化。

**Acceptance**

| Requirement | Agent Witness | User Witness | Status |
|---|---|---|---|
| R1-R2 | target build、diff review | no-hostfwd log | Covered |
| R3 | deadline 4/4 PASS | TCP/UDP/ICMP runtime | Covered |
| R4、R6 | payload source review | compile、markers、timeout | Covered |
| R5 | feature tree、target build | user-net/tap logs and pcap | Covered |
| R7 | None | 30-second `top` output | Covered |
| R8 | full diff review | separate QEMU evidence | Covered |

**Verification**

Agent:

```bash
cargo fmt --all -- --check
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture
cargo tree --offline -e features \
  -p starryos --features qemu -i smoltcp
make LOG=info build
python3 scripts/ms01-qemu-test.py --self-test
openspec validate ms02-virtio-mmio-polling-baseline
```

User 使用 `Manual Commands`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 001 code diff、RED/GREEN 和 compiler failure |
| Design | PASS | 产品设计不变，只修正能力边界 |
| Task Contracts | PASS | agent 与 user batch 已分离 |
| Traceability | PASS | R1-R8 均映射 agent/user witness |
| Verification | PASS | 命令、通过条件和证据类别已定义 |
| User Approval | PASS | 用户明确要求撤回误阻塞并手工测试 |

**Persisted Evidence**

- Mode: required
- Directory: `evidence/002-manual-verification-boundary/`
- Agent 保存 `README.md`、`build.log`、`review.md`。
- 用户提交后保存 `payload-build.log`、`qemu-no-hostfwd.log`、
  `qemu-usernet.log`、`qemu-usernet.pcap`、`qemu-tap.log`、
  `qemu-tap.pcap`、`idle-cpu.txt` 和 `ms01-regression.log`。
- 用户 Evidence 尚未提交时，Act 记录 external verification pending，
  不将 iteration 标为 blocked。

**Risks and Notes**

- TAP 使用 `10.0.2.2/24` 前必须检查路由冲突。
- 删除 TAP 前必须确认名称为 `tap-ms02`。
- 单 timeout 不证明多 waiter。
- 用户提交失败结果时不得降低验收条件。

## Act Response

- Status: reported

**Implemented**

- Verified the full agent batch for T4 static Gate: axnet fmt PASS,
  deadline policy 4/4 PASS, smoltcp feature tree with `auto-icmp-echo-reply`
  PASS, target kernel build PASS, MS01 harness self-test PASS, and
  `openspec validate` PASS.
- Performed the full-diff spec compliance review and code quality review
  across the five product files and the new C payload.
- Fixed two rustfmt deviations in `crates/axnet/src/service.rs` introduced
  by the prior iteration: collapsed `protocol_deadline` lookup to a single
  line and reordered `use` statements in the test module. axnet crate is
  now fmt-clean.
- Marked T3 (task 3.1) complete because its GREEN conditions (`cargo tree`
  shows the feature, target build passes) are both satisfied by agent
  evidence.
- Marked T1 (task 1.1) and T4 (task 4.1) as agent-side complete but
  user-side pending, per iteration 002 Task Contracts. Act Response is
  `reported` because agent Gate passes; user Evidence is recorded as
  external verification pending, not a blocker.

**Changed Files and Symbols**

- `crates/axnet/src/service.rs`: `protocol_deadline` collapsed to single
  line (line 100); test module `use` order fixed (smoltcp before super).
  No behavior change; fmt compliance only.
- `openspec/changes/ms02-virtio-mmio-polling-baseline/tasks.md`: task 3.1
  marked complete.
- `openspec/changes/ms02-virtio-mmio-polling-baseline/evidence/README.md`:
  added 002 index row.
- `openspec/changes/ms02-virtio-mmio-polling-baseline/evidence/002-manual-verification-boundary/`:
  new directory with `README.md`, `build.log`, `review.md`.

**Deviations from Plan**

- `cargo fmt --all -- --check` reports 341 pre-existing diffs in
  `crates/smoltcp/` (benches, examples, src). `git diff HEAD -- crates/smoltcp/`
  is empty, proving these are baseline state, not introduced by this change.
  Used `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` to
  scope the agent Gate to the crate modified by this change. Surgical
  Changes principle forbids reformatting unrelated files. Recorded in
  `build.log` and `review.md`.
- Worktree contains user-produced artifacts `tests/ms02_guest_service`
  (compiled RISC-V binary) and `ms02-usernet.pcap` (packet capture).
  These exist because the user has begun manual QEMU verification, but
  have not been formally submitted as Evidence. Listed in `build.log` for
  traceability; not claimed as agent evidence.

**Blocker Handoff**

None. Agent Gate passes. User Evidence is pending submission per
iteration 002 Task Contracts; this is not a blocker.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 1

Minor finding: Makefile `tests/ms02_guest_service` target uses
`-static -O2` instead of `$(BENCH_CFLAGS)`, diverging from sibling target
style. Not fixed because it matches the Plan Manual Commands verbatim
and `-O2` is acceptable for a test payload.

Full-diff spec review PASS: all eight requirements (R1-R8) have their
agent-side witnesses covered as specified in iteration 002 Acceptance.
No out-of-scope modifications. No IRQ, PLIC, AtomicWaker, async queue
task, or multi-waiter state was introduced. MS01 socket behavior is
preserved: `register_waker` still uses the protocol deadline for
loopback-only and IRQ-device sockets, and still falls through to
`device.register_waker(waker)` for mask-hit devices.

Full-diff code quality review PASS: `POLLING_FALLBACK` is a named
constant; `select_wake_deadline` is a pure function with exhaustive
match; `register_waker` correctly short-circuits via `any()` and only
emits a polling deadline when a mask-hit device requests polling;
payload uses single `poll()`, ignores SIGPIPE, protects against buffer
overflow, and cleans up fds on failure.

Post-fmt-fix regression: `make LOG=info build` recompiled axnet-ng and
produced `StarryOS_riscv64-qemu-virt.bin` (exit 0); `openspec validate
ms02-virtio-mmio-polling-baseline` still PASS (exit 0). The fmt fixes
do not change behavior, confirmed by GREEN preservation.

**Verification Evidence**

| Verification | Command or operation | Output excerpt | Exit | Conclusion |
|---|---|---|---|---|
| axnet fmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` | (no diff) | 0 | PASS |
| T1 deadline policy | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture` | 4 passed; 0 failed | 0 | PASS |
| T3 feature graph | `cargo tree --offline -e features -p starryos --features qemu -i smoltcp` | `smoltcp feature "auto-icmp-echo-reply"` present | 0 | PASS |
| T4 target build | `make LOG=info build` | `StarryOS_riscv64-qemu-virt.bin` produced | 0 | PASS |
| MS01 self-test | `python3 scripts/ms01-qemu-test.py --self-test` | `PASS: harness-self-test` | 0 | PASS |
| OpenSpec validation | `openspec validate ms02-virtio-mmio-polling-baseline` | `Change 'ms02-virtio-mmio-polling-baseline' is valid` | 0 | PASS |
| Full-diff spec review | manual review of 5 product files + payload | see `review.md` | N/A | PASS |
| Full-diff code quality review | manual review of 5 product files + payload | see `review.md` | N/A | PASS |
| QEMU runtime | SKIPPED: manual-only Runbook and user batch | not run by agent | N/A | external verification pending |

**Persisted Evidence**

- `../evidence/002-manual-verification-boundary/README.md`
- EV-002-01: `build.log` (agent batch verification)
- EV-002-02: `review.md` (spec and code quality review)

Plan-required user Evidence files (`payload-build.log`, `qemu-no-hostfwd.log`,
`qemu-usernet.log`, `qemu-usernet.pcap`, `qemu-tap.log`, `qemu-tap.pcap`,
`idle-cpu.txt`, `ms01-regression.log`) are pending user submission per
iteration 002 Task Contracts. Their absence does not block the agent-side
`reported` status.

**Experience Candidates**

| Type | Candidate | Evidence | Reason |
|---|---|---|---|
| Runbook | `cargo fmt` workspace vs crate scope | `build.log` | When the workspace contains a vendored upstream fork (here: smoltcp) with pre-existing fmt drift, `cargo fmt --all -- --check` fails on baseline state. Scoping to `--manifest-path <crate>` isolates the change's responsibility. May merit a Runbook after user confirms this pattern recurs. |

**Remaining Issues**

- T1 (task 1.1) user-side GREEN (payload compile, markers, timeout) is
  pending formal user submission. Worktree contains a compiled binary,
  but the user has not yet submitted the result as Evidence.
- T4 (task 4.1) user-side runtime Gate (no-hostfwd, user-net TCP/UDP,
  TAP ARP/ICMP, idle CPU, MS01 runtime regression) is pending formal
  user submission. Worktree contains `ms02-usernet.pcap`, suggesting the
  user has begun this batch.
- Pre-existing smoltcp fmt drift (341 diffs in `crates/smoltcp/`) is
  out of scope for this change and not fixed.

**Commit or Diff Reference**

Worktree only; no commit created. Product diff is the five files listed
above plus the fmt fixes in `service.rs`. HEAD remains at
`efcf08124294d523ccab4d3569ea97fe31ed96c1`.

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

- PASS：产品 diff 保持 MS02 范围。未引入 IRQ、async queue、
  多 waiter、PCI 或 raw ICMP socket。
- PASS：Review 复验 axnet fmt、deadline 4/4、feature tree、
  target build、MS01 self-test 和 OpenSpec strict validation。
- IMPORTANT：四个 deadline test 只验证两个 `Option<Instant>` 的选择。
  它们未覆盖 device mask、mask 外 polling device、mask 内 IRQ device
  和 mask 内无 IRQ device。T2 的 mask 契约缺少测试见证。
- Gate 5 未通过。`payload-build.log`、no-hostfwd、5555 TCP/UDP、
  TAP ICMP、空闲 CPU 和 MS01 runtime Evidence 尚未提交。
- 当前 `ms02-usernet.pcap` 只有 2 个 ARP 包和 9 个 18765/TCP 包。
  它只证明 payload 下载路径，不证明 5555 TCP/UDP 或 ICMP。
- `tests/ms02_guest_service` 是 RISC-V static PIE。项目 loader 支持
  ET_DYN load bias，MS01 payload 也是同类格式。ELF 类型不构成问题。
- Fresh build exit 0 并生成镜像。期间 cargo-binutils 安装探测因
  只读 Cargo home 和网络限制报错，但已有 `rust-objcopy` 完成产物。

**Deviation Classification**

`PLAN-OMISSION`、`ACT-DEVIATION`、`NEW-EVIDENCE`

**Evidence**

- [Service deadline tests](../../../../crates/axnet/src/service.rs)：
  只调用 `select_wake_deadline`。
- [Device polling capability](../../../../crates/axnet/src/device/mod.rs)
  与 [Ethernet implementation](../../../../crates/axnet/src/device/ethernet.rs)：
  Codegraph 未发现 `requires_polling` 的覆盖测试。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked
  --offline --lib service::tests -- --nocapture`：4 passed，exit 0。
- `make LOG=info build`：镜像生成，exit 0。
- `python3 scripts/ms01-qemu-test.py --self-test`：PASS，exit 0。
- `openspec validate ms02-virtio-mmio-polling-baseline --strict`：
  PASS，exit 0。
- `tcpdump -nn -r ms02-usernet.pcap`：ARP 2、TCP 9、UDP 0、
  ICMP 0；TCP 仅使用 18765。

**Follow-up Decision**

创建 iteration 003。先补齐 device mask 与 polling eligibility
的纯策略测试，不改变运行行为。随后复验静态 Gate，并审核用户手工
提交的 QEMU、pcap、CPU 和 MS01 runtime Evidence。

**Next Iteration**

`openspec/changes/ms02-virtio-mmio-polling-baseline/iterations/003-policy-coverage-and-runtime-evidence.md`
