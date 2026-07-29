# Iteration 001: MS02 environment ready

## Plan Context

- Status: ready
- Round: 001
- Parent: `000-initial`

**Objective**

完成 VirtIO-MMIO 同步轮询基线。
分别取证串口、设备探测、ARP/ICMP、UDP/TCP 和空闲 CPU。

**Background**

Iteration 000 在 Gate 3 阻塞。
当时 Cargo 缺少锁定的 `libc 0.2.182`，且无法访问
`static.crates.io`。Act 未修改产品代码。

用户已补齐依赖。Plan Review 使用离线命令复验，
axnet 测试二进制成功启动。001 继续原 T1-T4，
不改需求、设计或验证边界。

**Current Baseline**

- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1`
- Branch: `net-k3`
- QEMU: 7.0.0，`virt`，1 GiB，单 hart。
- Guest IP: `10.0.2.15/24`。
- Gateway: `10.0.2.2`。
- 当前 MMIO Ethernet `irq_num()` 为 `None`。
- smoltcp 未启用 `auto-icmp-echo-reply`。
- 产品 diff 为空；当前 change 未跟踪。
- 000 的 blocker 与 Evidence 保留，不改写。

新鲜基线：

```text
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture
exit 0
running 0 tests
test result: ok
```

该结果只证明 host test 可进入测试二进制。
它不是 T1 的 RED，也不证明 MS02 功能。

**Current-State Evidence**

- `axnet::init_network` 创建 loopback、eth0、router 和全局
  `Service`。
- `poll_interfaces` 在 socket 调用线程推进协议栈。
- `TcpSocket::accept` 在尝试前调用 `poll_interfaces()`。
- `GeneralOptions::recv_poller` 通过 `poll_io` 注册 waker。
- `Service::register_waker` 只组合 smoltcp deadline 与设备 waker。
- `EthernetDevice::register_waker` 只在存在 IRQ 时注册。
- smoltcp 未启用 feature 时忽略 ICMPv4 echo request。
- kernel socket syscall 只支持 TCP 与 UDP。
- `.claude/runbooks/qemu-network-testing.md` 要求人工操作 guest shell。

**Relevant Code**

- [Device](../../../../crates/axnet/src/device/mod.rs)
- [EthernetDevice](../../../../crates/axnet/src/device/ethernet.rs)
- [Service](../../../../crates/axnet/src/service.rs)
- [GeneralOptions](../../../../crates/axnet/src/general.rs)
- [TcpSocket](../../../../crates/axnet/src/tcp.rs)
- [axnet Cargo features](../../../../crates/axnet/Cargo.toml)
- [tests](../../../../tests/)
- [Makefile](../../../../Makefile)

**Critical Path**

`accept/recvfrom/poll`
→ `poll_io`
→ socket waker registration
→ `Service::register_waker`
→ smoltcp timer 或 device waker
→ socket 操作重试
→ `poll_interfaces`
→ VirtIO RX
→ ARP/IP/smoltcp
→ socket readiness 或 ICMP reply。

无 IRQ Ethernet 当前缺少外部 RX 唤醒来源。
目标是在现有 waiter 上增加 10 ms timer fallback。
状态仍由全局 `Service` mutex 所有。

**Implementation Guidance**

1. 先加入 deadline policy tests 并观察 RED。
2. 增加单 `poll()` 的 TCP/UDP guest payload。
3. 增加设备 polling capability。
4. 只为无 IRQ Ethernet 合并 10 ms deadline。
5. 启用 smoltcp 自动 ICMP echo。
6. 完成 agent 可执行回归和完整 diff Review。
7. 到达 QEMU 人工验证边界后停止并交接。

**Behavioral Change**

当前无 IRQ Ethernet 不注册 waker。
阻塞 socket 可能无法观察外部 RX。
ICMP echo request 被协议栈忽略。

修改后，无 IRQ Ethernet waiter 最迟 10 ms 被 timer 唤醒。
重试仍由同步 `poll_interfaces()` 推进。
smoltcp 自动回复 ICMP echo request。
IRQ 设备、loopback、errno、timeout 和 close 语义不变。

**Change Surface**

| Task | Requirement | File/Symbol | Planned Change |
|---|---|---|---|
| T1 | R3-R4、R6 | `service.rs` tests；guest payload；`Makefile` | 建立 RED/GREEN policy test 与单 waiter payload |
| T2 | R3、R8 | `Device`；`EthernetDevice`；`Service::register_waker` | 无 IRQ Ethernet 使用 10 ms fallback |
| T3 | R5、R8 | `crates/axnet/Cargo.toml` | 启用自动 ICMP echo |
| T4 | R1-R8 | build、MS01、Runbook | 完成回归并交接人工 QEMU |

**Task Contracts**

T1 — 测试见证：

- Depends on: None.
- 先只加入 deadline tests，观察 RED。
- RED 原因必须是 fallback 策略缺失。
- payload 使用单一 `poll()` 处理 TCP/UDP 5555。
- READY、TCP PASS、UDP PASS 和 FAIL 标记必须唯一。
- payload 不修改 guest init。
- 禁止创建 QEMU 驱动脚本。
- 测试再次无法进入主体时停止。

T2 — timer fallback：

- Depends on: T1 RED.
- `Device` 默认不要求 polling。
- Ethernet 仅在 IRQ 为 `None` 时要求 polling。
- deadline 为 smoltcp 与 `now + 10 ms` 的较早值。
- mask 外设备不得触发 fallback。
- 不增加后台任务、IRQ、AtomicWaker 或多 waiter 状态。
- T1 policy tests 是 GREEN witness。
- 单一 `poll()` 无法推进时停止。

T3 — ICMP feature：

- Depends on: T1 RED；可在 T2 后执行。
- 只修改 axnet 的 smoltcp feature。
- 不修改 smoltcp echo 实现。
- 不修改 raw/ICMP socket syscall。
- feature tree 是 RED/GREEN witness。
- feature 解析失败时停止。

T4 — 回归与交接：

- Depends on: T2、T3 GREEN.
- 依次执行 fmt、unit、payload、feature tree、build。
- MS01 非 QEMU检查必须通过。
- 完整 diff 先做 spec review，再做 code review。
- QEMU 和 guest shell 只由用户手工操作。
- agent Gate 失败时保留当前任务并写 blocker。
- 人工 Evidence 不全时 Gate 5 保持 blocked。

**Manual QEMU Boundary**

遵守 `.claude/runbooks/qemu-network-testing.md`。
不得使用脚本、pipe 或 pexpect 驱动 guest shell。

用户手工完成：

1. 无 hostfwd 启动，确认串口 shell、MMIO net/block 与 `eth0`。
2. user-net TCP/UDP 5555，分别记录 payload 与响应。
3. TAP `tap-ms02` 的 ARP/ICMP，保存 `tcpdump` pcap。
4. user-net 空闲 30 秒，保存 QEMU PID 的 `top` 原始输出。
5. MS01 运行时回归。

TAP 使用 `10.0.2.2/24` 前必须检查路由冲突。
删除前必须确认设备名为 `tap-ms02`。
QEMU 结果只计单 hart 模拟环境证据。

**Invariants**

- 保持 M41 transport 边界与 D22 MMIO-first。
- 保持 K31 的串口、网络和 hostfwd 分证据。
- 保持 MS01 socket 行为。
- 不引入 Embassy executor、IRQ 或 async queue。
- 不声明真板、SMP 或性能优化结论。

**Non-goals**

- PCI、VF2、SMP、IRQ、PLIC 和 DMA。
- stack runner、socket readiness bridge 和多 waiter。
- raw ICMP socket 与 BusyBox `ping` 兼容。
- 自动 QEMU harness。
- CPU、吞吐或延迟优化。

**Acceptance**

| Requirement | Scenario | Design | Task | Test Witness | Status |
|---|---|---|---|---|---|
| R1 通道分证据 | no-hostfwd；端口失败 | D5 | T4 | 启动与网络日志 | Covered |
| R2 MMIO 启动 | probe；transport 不一致 | D5 | T4 | LOG=info 启动日志 | Covered |
| R3 同步进度 | RX；早到流量；空闲 | D1-D2 | T1-T2 | deadline tests；协议用例 | Covered |
| R4 guest 服务 | TCP；UDP；未就绪 | D4 | T1,T4 | payload markers；timeout | Covered |
| R5 协议见证 | ARP；ICMP；UDP；TCP | D3-D5 | T1,T3,T4 | pcap 与终端日志 | Covered |
| R6 timeout 诊断 | timeout；payload；中断 | D4-D5 | T1,T4 | FAIL 与 blocker | Covered |
| R7 CPU 基线 | 30 秒空闲采样 | D1,D5 | T4 | `top` 原始输出 | Covered |
| R8 范围隔离 | 同步基线；范围扩大 | D1-D5 | T2-T4 | diff review；build | Covered |

**Verification**

Agent 执行：

```bash
cargo fmt --all -- --check
cargo test --manifest-path crates/axnet/Cargo.toml \
  --locked --offline --lib service::tests -- --nocapture
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
cargo tree --offline -e features \
  -p starryos --features qemu -i smoltcp
make LOG=info build
python3 scripts/ms01-qemu-test.py --self-test
```

用户按 Runbook 手工执行 QEMU。
失败时保留命令、关键输出和退出码。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 000 调用链、timer、IRQ 与 feature 调查仍有效 |
| Design | PASS | D1-D5 未变，10 ms 与单 waiter 边界明确 |
| Task Contracts | PASS | T1-T4 含 RED、GREEN、约束和停止条件 |
| Traceability | PASS | R1-R8 均为 Covered |
| Verification | PASS | host test 离线 exit 0；agent 与人工 Gate 已定义 |
| User Approval | PASS | 用户确认环境恢复并要求继续，随后调用 Act |

**Persisted Evidence**

- Mode: required
- Directory: `evidence/001-environment-ready/`
- `README.md`：环境、revision、artifact hash 和索引。
- `tdd-policy.log`：deadline test RED/GREEN。
- `build.log`：fmt、payload、feature tree 和 target build。
- `qemu-no-hostfwd.log`：串口、MMIO probe 和 eth0。
- `qemu-usernet.log`、`qemu-usernet.pcap`：TCP/UDP。
- `qemu-tap.log`、`qemu-tap.pcap`：ARP/ICMP。
- `idle-cpu.txt`：30 秒采样环境、方法和原始结果。
- `ms01-regression.log`：MS01 回归摘要。

Gate 3 使用 `tdd-policy.log`。
Gate 5 要求其余 build、QEMU、pcap、CPU 和 MS01 文件。
blocked 时保存 README 与 Blocker Handoff。

**Risks and Notes**

- smoltcp 当前 11 个 warning 不阻塞基线。
- TAP 需要 sudo；地址冲突时停止。
- 10 ms 只作为功能 fallback。
- 单 timeout 不证明多 waiter。
- 同一问题三次失败后返回 Plan。

## Act Response

- Status: blocked

**Implemented**

- Added four deadline policy tests and observed the planned missing-policy RED.
- Added `Device::requires_polling` with a default of `false`.
- Made Ethernet request polling only when its underlying device has no IRQ.
- Merged the smoltcp protocol deadline with a masked-device 10 ms polling
  fallback in `Service::register_waker`.
- Enabled smoltcp `auto-icmp-echo-reply`.
- Added the single-`poll()` TCP/UDP guest service source and Makefile target.
- Stopped before downstream verification when the target payload compiler was
  terminated by the environment.

**Changed Files and Symbols**

- `crates/axnet/src/service.rs`: `POLLING_FALLBACK`,
  `select_wake_deadline`, `Service::register_waker`, and four unit tests.
- `crates/axnet/src/device/mod.rs`: default `Device::requires_polling`.
- `crates/axnet/src/device/ethernet.rs`:
  `EthernetDevice::requires_polling`.
- `crates/axnet/Cargo.toml`: smoltcp `auto-icmp-echo-reply`.
- `tests/ms02_guest_service.c`: manual TCP/UDP 5555 payload using one
  `poll()` loop.
- `Makefile`: `tests/ms02_guest_service` target.
- Change-local Evidence, task 2.1 status, and this Act Response.

**Deviations from Plan**

None. The partial implementation follows T1-T3 contracts, and stopping after
the payload compiler failure follows the T4 agent-Gate rule.

**Blocker Handoff**

- Discovered at: T1 payload GREEN verification, before T4, Gate 5.
- Expected: `make tests/ms02_guest_service` produces the static RISC-V guest
  payload.
- Actual: `riscv64-linux-musl-gcc` was terminated with `Bad system call`;
  Make exited 2 and produced no binary.
- Impact: T1 cannot complete, T3 lacks its target-build witness, and T4 cannot
  start.
- Completed work: T2, including RED/GREEN policy tests and two-stage review.
- Partial work: T1 policy tests, payload source and Makefile target; T3 feature
  change and feature-tree witness.
- Unstarted work: fmt Gate, target build, MS01 self-test, complete T4/full-diff
  completion review, and all manual QEMU verification.
- Worktree state: six product files modified or added; no generated payload
  binary or core file remains; current change is untracked.
- Gates: Gate 1 PASS; Gate 2 PASS; Gate 3 PASS for T1 RED; Gate 4 PASS for T2;
  Gate 5 BLOCKED for T1; Gate 6 applied.
- Evidence: EV-001-01 through EV-001-03 under
  `evidence/001-environment-ready/README.md`.
- Plan decision needed: review the compiler execution blocker and carry the
  partial implementation into a new iteration after the toolchain works.
- Resume condition: a working `riscv64-linux-musl-gcc` can compile the declared
  payload command, and `openspec-plan` creates a new `ready` iteration. This
  blocked iteration must not be resumed.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

T2 spec review PASS: only masked devices can request fallback; the Device
default preserves loopback and IRQ-device behavior; no IRQ, task, AtomicWaker,
or multi-waiter state was introduced.

T2 code quality review PASS: deadline selection is isolated and covered by
four deterministic tests; axnet host compilation produced no new axnet
warnings.

T1 and T3 completion reviews remain blocked because the payload compiler and
target-build witnesses are missing. The partial diff was inspected for scope
and no Critical, Important, or Minor finding was identified, but the final
completion diff review was not reached.

**Verification Evidence**

| Verification | Command or operation | Output excerpt | Exit | Conclusion |
|---|---|---|---|---|
| T1 RED | `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture` | E0432: no `select_wake_deadline` in `service` | 101 | PASS RED |
| T2 GREEN | same command after implementation | 4 passed; 0 failed | 0 | PASS |
| T3 feature graph | `cargo tree --offline -e features -p starryos --features qemu -i smoltcp` | `smoltcp feature "auto-icmp-echo-reply"` | 0 | PASS partial |
| Guest payload | `make tests/ms02_guest_service` | `Bad system call (core dumped)` | 2 | BLOCKED |
| OpenSpec validation | `openspec validate ms02-virtio-mmio-polling-baseline` | change is valid | 0 | PASS |
| Remaining agent suite | SKIPPED: payload compiler Gate failed | not run | N/A | NOT REACHED |
| QEMU and guest shell | SKIPPED: manual-only Runbook and earlier Gate blocker | not run | N/A | NOT REACHED |

**Persisted Evidence**

- `../evidence/001-environment-ready/README.md`
- EV-001-01: `tdd-policy.log`
- EV-001-02: `build.log`
- EV-001-03: `blocker.md`

The remaining plan-required QEMU, pcap, CPU, and MS01 files do not exist
because their tasks were not reached. Gate 5 remains blocked.

**Experience Candidates**

None. The compiler termination is an environment prerequisite failure, not a
verified reusable procedure or significant runtime incident.

**Remaining Issues**

- T1 guest payload compilation is blocked.
- T3 target-build witness is missing.
- T4 and all manual QEMU evidence are unstarted.
- Task 2.1 is complete; tasks 1.1, 3.1, and 4.1 remain open.

**Commit or Diff Reference**

Worktree only; no commit created.

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

- T1 RED 和 T2 GREEN 证据有效。四个 deadline policy tests 通过。
- T2 实现符合 mask、无 IRQ 和 10 ms fallback 契约。
- T3 feature tree 已显示 `auto-icmp-echo-reply`。
- `riscv64-linux-musl-gcc` 的 `Bad system call` 只说明 agent 终端不能
  执行该工具，不说明工具或 payload 无效。
- 用户确认该编译器在其终端可用，并要求自行完成 payload 编译和全部
  QEMU 测试。
- QEMU 从未由 agent 执行。001 将外部工具失败当作实施 blocker，
  能力边界划分过严。

**Deviation Classification**

`PLAN-INVALID`、`NEW-EVIDENCE`

**Evidence**

- `evidence/001-environment-ready/tdd-policy.log`：RED exit 101，
  GREEN 4/4 PASS。
- `evidence/001-environment-ready/build.log`：feature tree PASS；
  payload compiler 在 agent 终端 exit 2。
- 用户原话：“我允许进行阻塞撤回，这个小问题造成的误判而已，取消之后
  把命令行给我我来手动测试”。
- 当前产品 diff 包含计划内的 deadline、Device capability、ICMP feature、
  guest payload 和 Makefile target。

**Follow-up Decision**

保留 001 的 blocked 历史。002 将 payload 编译、QEMU、pcap、CPU 采样
和 MS01 runtime 回归设为用户能力边界。

Agent 继续执行 fmt、Rust unit、feature tree、target kernel build、
MS01 harness self-test 和完整 diff Review。外部证据待用户提交，不把
“尚未提交”记为 agent blocker。

**Next Iteration**

`openspec/changes/ms02-virtio-mmio-polling-baseline/iterations/002-manual-verification-boundary.md`
