# Iteration 000: MS02 MMIO polling baseline

## Plan Context

- Status: ready
- Round: 000
- Parent: None

**Objective**

建立 VirtIO-MMIO 同步轮询基线。
串口、probe、ARP/ICMP、UDP/TCP 和 CPU 分开取证。

**Background**

MS01 主要证明 guest loopback。
MS02 覆盖 roadmap 的 T02-T03。
相关约束为 M41、D22、K31 和 K32。

用户于 2026-07-29 要求：
“你不用测试了，把这个测试任务写入plan”。
因此 Plan 不运行产品测试。
所有验证写入本 iteration。

**Current Baseline**

- Revision:
  `efcf08124294d523ccab4d3569ea97fe31ed96c1`
- Branch: `net-k3`
- QEMU: 7.0.0，`virt`，1 GiB，单 hart。
- Guest IP: `10.0.2.15/24`。
- Gateway: `10.0.2.2`。
- 当前 QEMU 镜像按 K32 解释为 MMIO。
- MS01 已归档，最终 evidence 为 14/14 PASS。
- 当前 smoltcp 未启用 `auto-icmp-echo-reply`。
- 当前 MMIO Ethernet `irq_num()` 为 `None`。
- Plan 阶段运行测试：
  `SKIPPED: user requested tests be written into the plan`。
- 此前由 agent 驱动的 QEMU smoke 不符合 Runbook。
  它们不得作为 Gate 或 Evidence。

**Current-State Evidence**

- [axnet::init_network](../../../../crates/axnet/src/lib.rs#L70)
  建立 loopback、eth0、router 与全局 `Service`。
- [poll_interfaces](../../../../crates/axnet/src/lib.rs#L143)
  在调用线程中推进 ingress、egress 与 dispatch。
- [TcpSocket::accept](../../../../crates/axnet/src/tcp.rs#L340)
  每次尝试先调用 `poll_interfaces()`。
- [GeneralOptions::recv_poller](../../../../crates/axnet/src/general.rs#L87)
  使用 `poll_io` 注册 socket waker。
- [Service::register_waker](../../../../crates/axnet/src/service.rs#L84)
  只组合 smoltcp deadline 与设备 waker。
- [EthernetDevice::register_waker](../../../../crates/axnet/src/device/ethernet.rs#L336)
  只在存在 IRQ 时注册。
- [ICMPv4 处理](../../../../crates/smoltcp/src/iface/interface/ipv4.rs#L320)
  在 feature 关闭时忽略 echo request。
- [socket syscall](../../../../kernel/src/syscall/net/socket.rs#L26)
  只支持 TCP 与 UDP。
- [QEMU 参数](../../../../make/qemu.mk#L52)
  已区分 user、tap 和 bridge backend。
- [Runbook](../../../../.claude/runbooks/qemu-network-testing.md)
  禁止自动驱动 QEMU 和 guest shell。

只读基线命令：

- `python3 scripts/ms01-qemu-test.py --self-test`
  退出码 0，输出 `PASS: harness-self-test`。
- `cargo tree --offline -e features -p starryos --features qemu -i axdriver`
  退出码 0，显示 `bus-mmio` 与 `bus-pci`。
- `cargo tree --offline -e features -p starryos --features qemu -i smoltcp`
  退出码 0，未显示 `auto-icmp-echo-reply`。
- QEMU runtime baseline：
  `SKIPPED: manual-only Runbook and user instruction`。

**Relevant Code**

- `crates/axnet/src/device/mod.rs::Device`：
  设备接收、发送与 waker 契约。
- `crates/axnet/src/device/ethernet.rs::EthernetDevice`：
  Ethernet RX/TX、ARP 和 IRQ waker。
- `crates/axnet/src/service.rs::Service`：
  协议栈推进、timer 与设备注册。
- `crates/axnet/Cargo.toml`：
  smoltcp feature 选择。
- `tests/ms02_guest_service.c`：
  新增的 TCP/UDP 手工 payload。
- `Makefile`：
  新增 payload 编译 target。

**Critical Path**

`accept/recvfrom/poll`
→ `poll_io`
→ socket `register`
→ `GeneralOptions::register_waker`
→ `Service::register_waker`
→ smoltcp timer 或 device waker
→ waiter 被唤醒
→ socket 操作重试
→ `poll_interfaces`
→ VirtIO RX
→ ARP/IP/smoltcp
→ socket readiness 或 ICMP reply。

无 IRQ 时，当前路径停在 device waker。
目标路径加入 10 ms timer fallback。
状态仍由全局 `Service` mutex 所有。

**Implementation Guidance**

1. 测试先约束 deadline policy。
2. 建立 guest TCP/UDP payload。
3. 只扩展设备 polling capability。
4. 只在无 IRQ Ethernet 启用 timer。
5. 启用 smoltcp 自动 ICMP echo。
6. 完成 agent 可执行的回归。
7. 到达手工 QEMU 边界后停止。

**Behavioral Change**

当前：

- 无 IRQ Ethernet 不注册 waker。
- 外部 RX 可能无法唤醒阻塞 socket。
- ICMP echo request 被协议栈忽略。

目标：

- 无 IRQ Ethernet waiter 最迟 10 ms 被 timer 唤醒。
- 重试仍由同步 `poll_interfaces()` 推进。
- smoltcp 自动回复 ICMP echo request。
- IRQ 设备与 loopback 行为不变。

错误语义：

- socket errno、timeout 和 close 语义不变。
- 手工用例超时视为失败。
- 环境或权限失败写 blocked，不计功能失败。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T1 | R3/S3.1-S3.3, R4, R6 | `service.rs` tests; `tests/ms02_guest_service.c`; `Makefile` | timer 与测试构建 | 增加 RED/GREEN 策略测试和单 waiter payload |
| T2 | R3, R8 | `Device`; `EthernetDevice`; `Service::register_waker` | 设备通知与 timer | 无 IRQ Ethernet 使用 10 ms fallback |
| T3 | R5/S5.2, R8 | `crates/axnet/Cargo.toml` | smoltcp feature | 启用自动 ICMP echo |
| T4 | R1-R8 | build、MS01、Runbook 手工步骤 | 回归与取证 | 完成 agent Gate 并交接用户验证 |

**Task Contracts**

T1 — 测试见证：

- Depends on: None.
- 先只加入 deadline tests，观察 RED。
- RED 原因必须是 fallback 策略缺失。
- payload 用单一 `poll()` 处理 TCP/UDP。
- READY、PASS、FAIL 标记必须唯一。
- payload 不得修改 guest init。
- 禁止创建 QEMU 驱动脚本。
- RED 无法进入测试主体时停止。

T2 — timer fallback：

- Depends on: T1 RED.
- `Device` 默认不要求 polling。
- Ethernet 仅在 IRQ 为 `None` 时要求 polling。
- deadline 为 smoltcp 与 10 ms fallback 的较早值。
- mask 外设备不得触发 fallback。
- 不增加后台任务或多 waiter 状态。
- T1 policy tests 是 GREEN witness。
- 单一 `poll()` 无法推进时停止。

T3 — ICMP feature：

- Depends on: T1 RED；可在 T2 后执行。
- 只修改 axnet 的 smoltcp feature。
- 不修改 smoltcp echo 代码。
- 不修改 raw/ICMP socket syscall。
- feature tree 是 RED/GREEN witness。
- feature 解析失败时停止。

T4 — 回归与交接：

- Depends on: T2、T3 GREEN.
- 先 fmt，再 unit，再 payload，再 build。
- MS01 非 QEMU检查必须通过。
- 完整 diff 先做 spec review，再做 code review。
- 准备下列手工命令，不执行 QEMU。
- agent Gate 失败时保留当前任务。
- 用户 Evidence 不全时 Gate 5 blocked。

**Manual QEMU Boundary**

以下操作由用户手工执行。
不得通过脚本、pipe 或 pexpect 驱动 guest shell。

环境准备：

```bash
cd /home/daivy/projects/serial/work/StarryOS
make LOG=info build
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
```

无 hostfwd 启动：

```bash
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

通过条件：

- 串口进入 `starry:~#`。
- 日志标出 MMIO net/block probe。
- 日志标出 `eth0`。
- 该 run 不证明 hostfwd。

user-net TCP/UDP：

```bash
cd /home/daivy/projects/serial/work/StarryOS/tests
python3 -m http.server 18765 --bind 0.0.0.0
```

另一个终端手工启动：

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

guest 手工输入：

```sh
wget -q -O /tmp/ms02_service \
  http://10.0.2.2:18765/ms02_guest_service
chmod +x /tmp/ms02_service
/tmp/ms02_service
```

宿主分别运行 TCP 与 UDP `nc`。
连接后手工输入 payload。
不得用 pipe 代替手工输入。

通过条件：

- guest READY 标记唯一。
- TCP 连接、payload 与响应通过。
- TCP 关闭后第二次连接通过。
- UDP datagram 与响应通过。
- TCP、UDP PASS 标记分开。
- 每个宿主命令有 5 秒 timeout。

TAP ARP/ICMP：

```bash
cd /home/daivy/projects/serial/work/StarryOS
sudo ip tuntap add dev tap-ms02 mode tap user "$(id -un)"
sudo ip addr add 10.0.2.2/24 dev tap-ms02
sudo ip link set tap-ms02 up
sudo tcpdump -i tap-ms02 -nn -e -w ms02-tap.pcap
```

另一个终端手工启动：

```bash
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

guest 手工输入：

```sh
nc -u -l -p 5555 >/tmp/ms02-icmp-wait.log 2>&1 &
echo MS02_ICMP_WAITER_READY
```

宿主手工输入：

```bash
ping -c 3 -W 2 10.0.2.15
```

通过条件：

- pcap 含 ARP request 与 reply。
- pcap 含三组 ICMP request 与 reply。
- echo ident、sequence 和 payload 匹配。
- ICMP 不依赖 guest raw socket。

清理 TAP：

```bash
sudo ip link delete tap-ms02
```

删除前必须确认目标名称是 `tap-ms02`。

空闲 CPU：

- 使用 user-net run。
- guest 服务处于等待状态。
- 网络保持空闲 30 秒。
- 用户记录 QEMU PID。
- 运行 `top -b -d 1 -n 30 -p <QEMU_PID>`。
- 记录宿主、QEMU 版本、参数和输出。
- 不设置通过阈值。

**Invariants**

- 保持 M41 的 transport 边界。
- 保持 D22 的 MMIO-first。
- 保持 K31 的通道分证据。
- 保持 MS01 socket 行为。
- 不引入 Embassy executor。
- 不引入 IRQ 或 async queue。
- 不把 QEMU 结果声明为硬件证据。
- 不把单 hart 结果声明为 SMP 证据。

**Non-goals**

- PCI、VF2、SMP、IRQ 和 DMA。
- stack runner 与 socket readiness bridge。
- raw ICMP socket 与 BusyBox `ping`。
- 自动 QEMU harness。
- CPU、吞吐或延迟优化。

**Acceptance**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R1 通道分证据 | no-hostfwd；串口成功端口失败 | D5 | T4 | `make/qemu.mk`；Runbook | 手工启动日志 | None | Covered |
| R2 MMIO 启动 | probe；transport 不一致 | D5 | T4 | axruntime；axdriver；`init_network` | LOG=info 启动日志 | None | Covered |
| R3 同步进度 | 等待 RX；早到流量；空闲 | D1-D2 | T1-T2 | `Device`；`EthernetDevice`；`Service` | deadline tests；TCP/UDP/ICMP | None | Covered |
| R4 guest 服务 | TCP；UDP；未就绪 | D4 | T1,T4 | `ms02_guest_service.c` | payload markers；host timeout | None | Covered |
| R5 协议见证 | ARP；ICMP；UDP；TCP | D3-D5 | T1,T3,T4 | Ethernet；smoltcp feature；payload | 两份 pcap 与终端日志 | None | Covered |
| R6 timeout 诊断 | timeout；payload；中断 | D4-D5 | T1,T4 | payload；手工步骤 | FAIL 标记与 Blocker Handoff | None | Covered |
| R7 CPU 基线 | 30 秒空闲采样 | D1,D5 | T4 | `Service`；QEMU process | `top` 原始输出 | None | Covered |
| R8 范围隔离 | 完成基线；自动化扩大 | D1-D5 | T2-T4 | 完整 diff；feature tree | diff review；build | None | Covered |

**Verification**

Agent 可执行：

```bash
cargo fmt --all -- --check
cargo test --manifest-path crates/axnet/Cargo.toml \
  --lib service::tests -- --nocapture
riscv64-linux-musl-gcc -static -O2 \
  -o tests/ms02_guest_service tests/ms02_guest_service.c
cargo tree --offline -e features \
  -p starryos --features qemu -i smoltcp
make LOG=info build
python3 scripts/ms01-qemu-test.py --self-test
```

QEMU 与 guest shell 只由用户手工执行。
若 agent 环境阻止 build 或 unit test，
Act Response 写 blocked。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 实际入口、调用链、timer、IRQ、feature 与 Runbook 已定位 |
| Design | PASS | D1-D5 的 10 ms、单 waiter、TAP 与 ICMP 语义已固定 |
| Task Contracts | PASS | T1-T4 含 RED、GREEN、约束、命令与停止条件 |
| Traceability | PASS | R1-R8 全部 Covered，无 Simplified 或 Missing |
| Verification | PASS | agent Gate、三组手工 QEMU 和判定规则已定义 |
| User Approval | PASS | 用户要求写入测试任务并按流程继续；失败写 blocked response |

**Persisted Evidence**

- Mode: required
- Directory: `evidence/000-initial/`
- `README.md`：环境、revision、artifact hash 与 Evidence 索引。
- `tdd-policy.log`：deadline test RED/GREEN、命令与退出码。
- `build.log`：fmt、payload、feature tree、target build。
- `qemu-no-hostfwd.log`：串口、MMIO probe 和 eth0。
- `qemu-usernet.log`：guest 服务、TCP 与 UDP 标记。
- `qemu-usernet.pcap`：user-net TCP/UDP 包。
- `qemu-tap.log`：waiter 与 ICMP 手工结果。
- `qemu-tap.pcap`：ARP 与 ICMP 包。
- `idle-cpu.txt`：30 秒采样环境、方法和原始结果。
- `ms01-regression.log`：MS01 手工回归摘要。

关联 Gate：

- Gate 3：`tdd-policy.log`。
- Gate 4：Act Response 的 spec/code review。
- Gate 5：全部 build、QEMU、pcap、CPU 和 MS01 文件。
- Gate 6：blocked 时的 README 与 Blocker Handoff。

**Risks and Notes**

- TAP 需要 sudo。
- `10.0.2.2/24` 冲突时必须停止。
- 单 timeout 不覆盖多 waiter。
- 10 ms 只作为功能 fallback。
- 手工结果失败时不得修改验收条件。
- 同一问题三次失败后返回 Plan。

## Act Response

- Status: blocked

**Implemented**

No product implementation was performed.

Act completed the rules, Runbook, change, code-path, revision, feature, and
toolchain baseline checks. Gate 3 stopped execution before the planned RED
test or any product edit.

**Changed Files and Symbols**

- `evidence/README.md`: change-level Evidence index.
- `evidence/000-initial/README.md`: iteration Evidence index and limits.
- `evidence/000-initial/tdd-policy.log`: Gate 3 command, output, exit code,
  and the successful feature/compiler baseline checks.
- `evidence/000-initial/blocker.md`: structured blocker handoff.
- This `Act Response`.
- Product files and symbols: None.

**Deviations from Plan**

None. Stopping when the axnet host test cannot enter the test body is the
explicit T1 contract.

**Blocker Handoff**

- Discovered at: T1, current-state validation, Gate 3.
- Expected: the declared axnet host test reaches `service::tests`, after which
  Act adds and observes the deadline-policy RED witness.
- Actual: Cargo could not obtain missing `libc 0.2.182`; DNS resolution for
  `static.crates.io` failed and the command exited 101 before the test body.
- Impact: T1 cannot establish its mandatory RED witness. The TDD rule forbids
  starting T2 or modifying product code.
- Completed work: rules and Runbook review; change/iteration readiness;
  actual call-path and HEAD checks; smoltcp feature RED baseline; musl compiler
  availability check.
- Partial work: None.
- Unstarted work: T1 implementation through T4, including all manual QEMU
  verification.
- Worktree state: no product changes; only the active untracked change contains
  this Response and Evidence.
- Gates: Gate 1 PASS; Gate 2 PASS; Gate 3 BLOCKED; Gates 4-5 not reached;
  Gate 6 applied.
- Evidence: EV-000-01 and EV-000-02 under
  `evidence/000-initial/README.md`.
- Plan decision needed: review the environment blocker and create a new
  iteration after dependencies are made available.
- Resume condition: the locked Cargo dependencies are locally cached or
  resolvable, and `openspec-plan` creates a new `ready` iteration. This blocked
  iteration must not be resumed.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

The stop condition was followed before product edits. Evidence contains the
failing command and exit code. No QEMU automation, global document update,
Plan Review edit, lifecycle operation, or scope expansion was performed.

**Verification Evidence**

| Verification | Command or operation | Output excerpt | Exit | Conclusion |
|---|---|---|---|---|
| Gate 3 host test baseline | `cargo test --manifest-path crates/axnet/Cargo.toml --lib service::tests -- --nocapture` | failed to download `libc 0.2.182`; could not resolve `static.crates.io` | 101 | BLOCKED before test body |
| T3 feature baseline | `cargo tree --offline -e features -p starryos --features qemu -i smoltcp` | current feature graph has `socket-icmp` and lacks `auto-icmp-echo-reply` | 0 | PASS baseline only |
| Payload compiler prerequisite | `command -v riscv64-linux-musl-gcc` | `/opt/musl/riscv64-linux-musl-cross/bin/riscv64-linux-musl-gcc` | 0 | PASS |
| Revision | `git rev-parse HEAD` | `efcf08124294d523ccab4d3569ea97fe31ed96c1` | 0 | PASS |
| QEMU and guest shell | SKIPPED: manual-only Runbook, user instruction, and T1 Gate 3 blocker | not run | N/A | NOT REACHED |

**Persisted Evidence**

- `../evidence/000-initial/README.md`
- EV-000-01: `tdd-policy.log`
- EV-000-02: `blocker.md`

The remaining plan-required files do not exist because their tasks were not
reached. Their Gate 5 claims remain blocked.

**Experience Candidates**

None. This is an environment prerequisite failure, not a verified reusable
operation or significant runtime incident.

**Remaining Issues**

- The axnet host test cannot run until the missing locked Cargo dependencies
  are available.
- T1-T4 are not implemented.
- Manual no-hostfwd, user-net, TAP/ICMP, packet capture, idle CPU, and MS01
  runtime regression evidence are not collected.

**Commit or Diff Reference**

Worktree only; no commit created. Product diff is empty.

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

- PASS：原环境 blocker 已解除。用户在同一工作区运行 axnet host
  test，测试二进制成功启动。
- PASS：Plan Review 使用 `--locked --offline` 复验同一入口，
  exit 0，输出 `running 0 tests`。
- `0 tests` 只证明测试入口可用。T1 的 deadline RED test 尚未添加，
  T1-T4 仍未实施。
- 000 的 Act Response 与 BLOCKED Evidence 在当时有效。未发现
  Plan 错误或 Act 偏离。
- 工作区仍无产品代码改动。现有 smoltcp warning 不影响该基线。

**Deviation Classification**

`BASELINE-CHANGED`、`NEW-EVIDENCE`

**Evidence**

- 用户命令完成 `axnet-ng` 编译并启动
  `axnet_ng-cbdd639128f1b2f8`，exit 0。
- Review 命令：
  `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline
  --lib service::tests -- --nocapture`。
- Review 结果：exit 0；`running 0 tests`；`0 passed; 0 failed`。
- `git status --short` 只显示未跟踪的当前 change。

**Follow-up Decision**

依赖缓存恢复后，原停止条件不再成立。保留 000 的 blocked 历史，
由 iteration 001 执行原 T1-T4。001 必须先加入 deadline tests
并取得 RED，不得把本次 0-test 基线计为测试见证。

**Next Iteration**

`openspec/changes/ms02-virtio-mmio-polling-baseline/iterations/001-environment-ready.md`
