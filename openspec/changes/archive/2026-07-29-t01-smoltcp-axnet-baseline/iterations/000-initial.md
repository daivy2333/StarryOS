# Iteration 000: Local smoltcp/axnet synchronous baseline

## Plan Context

- Status: ready
- Round: 000
- Parent: None

**Objective**

将 QEMU 同步网络栈切换到仓库内 smoltcp 0.13.1 和本地 axnet，并以
可复现测试证明 TCP listener、TCP bind、UDP、nonblocking 和 poll 行为
满足 MS01 基线；不引入 IRQ 或异步数据路径。

**Background**

MS01 是异步网络路线的行为基线。当前 registry axnet 依赖
`starry-smoltcp 0.12.1-preview.1` 的两个私有扩展：

- `RxToken::preprocess` 在协议栈 ingress 前嗅探 SYN 并创建 socket。
- TCP socket 内的 `get_bound_endpoint`/`set_bound_endpoint` 保存 POSIX
  bind 状态。

仓库内 smoltcp 0.13.1 没有这些接口，但公开
`poll_ingress_single`、`poll_egress`、`poll_maintenance`、
`TcpSocket::listen` 和 connection endpoint。

Gate 1 已于 2026-07-28 由用户回复“同意”通过。

**Current Baseline**

依赖和 workspace：

- 根 `Cargo.toml` 的 axnet 为 registry `axnet-ng 0.3.0-preview.2`。
- axnet 解析到 `starry-smoltcp 0.12.1-preview.1`。
- `crates/smoltcp` 为 0.13.1，但未加入或排除于根 workspace。
- 独立检查该 manifest 退出 101，报
  “current package believes it's in a workspace when it's not”。
- `cargo metadata --offline --format-version 1 --no-deps` 退出 0。

编译替换试验：

- 在 `/tmp` 复制 registry axnet，只把 smoltcp 改为仓库 path。
- `cargo check --offline --manifest-path <temp-axnet>/Cargo.toml` 退出 101。
- 共 10 个 axnet 错误：1 个 E0407 `RxToken::preprocess`，9 个
  `get_bound_endpoint`/`set_bound_endpoint` E0599。
- smoltcp 0.13.1 已在相同试验中完成依赖编译；没有第三类 axnet API
  error。

完整构建：

- 命令：
  `CARGO_TARGET_DIR=/tmp/starryos-ms01-current-target cargo build --offline --release --target riscv64gc-unknown-none-elf --features qemu`
- 退出 101。registry axnet 和 starry-smoltcp 已编译，之后
  `lwext4_rust` 的 C 子构建因当前执行环境 `Bad system call` 失败。
- 该结果是环境 blocker，不是网络 compile PASS。Act 的完整 build Gate
  仍要求退出 0。

运行基线：

- 使用 2026-07-28 构建的现有
  `StarryOS_riscv64-qemu-virt.bin` 和 snapshot rootfs 启动 QEMU。
- guest 到达 BusyBox shell。
- BusyBox `nc` loopback TCP payload `tcp-ms01` 往返成功。
- BusyBox `nc` loopback UDP payload `udp-ms01` 往返成功。
- raw ICMP socket 和 netlink 不受支持；`init.sh` 不启动 5555 服务。
- basic `nc` 不能验证 errno、poll、512 容量和 handle 生命周期。

当前 socket 行为：

- `kernel/src/syscall/net/socket.rs::sys_socket` 创建 axnet TCP/UDP socket。
- `sys_bind`、`sys_connect`、`sys_accept` 委托 axnet。
- `sys_listen` 只拒绝非法 backlog，合法数值不下传。
- `kernel/src/file/net.rs::Socket` 把 read/write/nonblock/poll 委托 axnet。
- `kernel/src/file/epoll.rs` 与
  `kernel/src/syscall/io_mpx/poll.rs` 使用 check-register-recheck。
- 当前仓库没有覆盖这些网络 syscall 的自动测试。

当前 axnet 状态：

- `TcpSocket` 的外部状态为 Idle、Connecting、Connected、Listening、
  Closed；smoltcp handle 存在全局 `SocketSet`。
- `ListenTable` 以 port 索引 entry，`syn_queue` 固定容量 512。
- 每个首 SYN 经 preprocess 动态创建一个 listening socket。
- accept 扫描 queue；close/unlisten 删除剩余 handles。
- `Router` 在 RX 与 TX 使用有界 packet buffers。
- `Service::poll` 依次执行 router receive、`Interface::poll` 和 dispatch。
- `SocketSetWrapper::bind_check` 通过 fork TCP bound field 与 UDP endpoint
  检查冲突。
- service 路径可能先锁 SocketSet 再锁 listener entry；accept 扫描可能
  反向锁序。

**Relevant Code**

- `Cargo.toml`：workspace、axnet dependency 和 crates.io patch。
- `Cargo.lock`：registry/local dependency witness。
- `crates/smoltcp/Cargo.toml`：0.13.1 feature surface。
- `crates/smoltcp/src/iface/interface/mod.rs::poll_ingress_single`：单包 ingress。
- `crates/smoltcp/src/socket/tcp.rs::Socket`：标准 listen 和 connection tuple。
- `crates/axnet/Cargo.toml`：实施后本地依赖入口。
- `crates/axnet/src/tcp.rs::TcpSocket`：bind、connect、listen、accept 和 poll。
- `crates/axnet/src/wrapper.rs::SocketSetWrapper`：socket set 与 bind sidecar。
- `crates/axnet/src/listen_table.rs::ListenTable`：listener slot 和 queue。
- `crates/axnet/src/service.rs::Service::poll`：同步协议栈推进。
- `crates/axnet/src/router.rs::RxToken`：实施后只转交 packet。
- `tests/ms01_socket_baseline.c`：guest syscall witness。
- `scripts/ms01-qemu-test.py`：构建、上传、执行和日志判定。

**Critical Path**

```text
guest socket syscall
  -> kernel syscall/net
  -> file/net Socket adapter
  -> local axnet TcpSocket/UdpSocket
  -> SocketSetWrapper + ListenTable
  -> Service::poll
  -> smoltcp 0.13.1 Interface/SocketSet
  -> Router
  -> current axdriver_net transport
```

TCP listener 状态：

```text
listen
  -> reserve endpoint
  -> create idle smoltcp Listen handle
  -> one ingress SYN changes handle state
  -> reconcile moves handle to pending
  -> refill one idle Listen handle if queue < 512
  -> handshake changes pending to ready/reset
  -> accept delivers once or reports reset once
  -> close removes all undelivered handles and endpoint state
```

TCP bind 状态：

```text
external TcpSocket handle
  -> axnet bind sidecar
  -> bind conflict and pre-connect endpoint
  -> smoltcp connect tuple
  -> accepted socket reads local_endpoint from tuple
  -> SocketSetWrapper::remove clears sidecar
```

**Implementation Guidance**

1. 先创建 characterization payload 和 serial harness。对旧镜像确认批准的
   场景可观察；测试不能依赖 hostfwd 或写入 rootfs。
2. 复制并清理 axnet crate，修改根 dependency、exclude 和 smoltcp path。
   保存切换后的精确 compile RED。
3. 迁移 TCP bind sidecar。外部 socket 是 bind owner；pool 和 accepted
   handles 不是重复 bind owners。
4. 重写 listener entry。pending、ready、reset 和 idle handle 都有唯一
   owner；移除后不得再次访问。
5. 将 service 改为细粒度 poll。每个 ingress 后 reconcile；maintenance、
   egress 和 dispatch 都不得遗漏。
6. 删除 Router 的 TCP 解析和 preprocess。smoltcp 保持 upstream-clean。
7. 依次通过 smoltcp、axnet、dependency、kernel build 和 QEMU runtime
   Gate。
8. 保存 required evidence，完成全量 diff review。

任务依赖为：

```text
1.1 -> 2.1 -> 3.1 -> 4.1 -> 4.2 -> 5.1 -> 5.2 -> 6.1
```

停止条件：

- 编译出现 preprocess/bound endpoint 之外的新迁移类别。
- 需要修改 smoltcp public trait 或恢复 fork 字段。
- 无法证明 socket handle 的唯一创建、交付和释放责任。
- 无法保持 `SocketSet -> ListenTable` 锁序。
- 需要修改 syscall backlog、IRQ、transport 或异步层才能通过。
- 完整 build 被环境阻断时，按 blocked handoff 结束，不跳过运行 Gate。

**Invariants**

- smoltcp 不新增 `RxToken::preprocess` 或 POSIX bind field。
- kernel socket/VFS/axpoll 调用边界保持不变。
- `listen` 固定队列容量为 512；backlog 数值仍不下传。
- 每个 accepted handle 最多交付一次。
- reset、unlisten 和 Drop 不泄漏或重复移除 handle。
- TCP/UDP nonblocking 和 poll 保持当前 errno/readiness 契约。
- 当前未运行验收的 IPv6、raw、ICMP、DNS 能继续编译。
- 保持同步 poll、VirtIO transport 和 QEMU MMIO 配置。
- 不修改 IRQ、DMA、PCI、VF2、DWMAC、SMP 或异步执行层。

**Non-goals**

- 动态 POSIX backlog。
- raw socket、netlink 或 ICMP runtime 支持。
- hostfwd 5555 服务。
- 性能指标、零拷贝或 buffer tuning。
- 修复无关的 accept address 或 syscall 问题。
- 更新全局 tasks、SNAPSHOT 或项目记忆。

**Acceptance**

- A1：Cargo metadata/tree 只解析本地 axnet 与 smoltcp 0.13.1。
- A2：两个本地 crate 可独立 manifest-path 检查。
- A3：源代码不含 preprocess 或 SYN snoop 实现。
- A4：TCP bind、隐式 bind、冲突和 local endpoint 场景通过。
- A5：TCP accept、相邻连接、512 边界和 close/relisten 通过。
- A6：UDP payload、source address、datagram boundary 和 EAGAIN 通过。
- A7：TCP/UDP poll 与紧随其后的 I/O 结果一致。
- A8：完整 QEMU feature RISC-V build 退出 0。
- A9：compile-only protocol features 保持可编译。
- A10：required evidence 与本轮镜像、payload 和场景一一对应。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R1 local dependency | S1 QEMU build | D1 | 2.1, 5.1 | `Cargo.toml`, manifests | metadata, tree, build | None | Covered |
| R1 local dependency | S2 API mismatch | D1 | 2.1, 3.1, 4.2 | axnet compile surface | compile RED/GREEN | None | Covered |
| R1 local dependency | S3 standalone crates | D1 | 2.1, 5.1 | workspace exclude | manifest checks | None | Covered |
| R2 TCP bind | S4 bind then listen/connect | D2 | 3.1, 5.2 | `tcp.rs`, `wrapper.rs` | guest TCP bind | None | Covered |
| R2 TCP bind | S5 implicit bind | D2 | 3.1, 5.2 | `TcpSocket::connect` | guest local address | None | Covered |
| R2 TCP bind | S6 conflict | D2 | 3.1, 5.2 | bind sidecar/check | guest AddrInUse | None | Covered |
| R3 listener | S7 accept and continue | D3 | 4.1, 4.2, 5.2 | listen table/service | guest accept sequence | None | Covered |
| R3 listener | S8 adjacent connections | D3 | 4.1, 4.2, 5.2 | ingress reconcile | two-client marker | None | Covered |
| R3 listener | S9 queue reaches 512 | D3, D4 | 4.1, 5.2 | queue capacity/refill | 512 boundary marker | None | Covered |
| R3 listener | S10 backlog argument | D4 | 4.1, 5.2 | unchanged `sys_listen` | fixed-capacity marker | None | Covered |
| R3 listener | S11 close/relisten | D3, D5 | 4.1, 5.2 | unlisten/drop | relisten marker | None | Covered |
| R4 UDP | S12 bidirectional datagram | D6 | 1.1, 5.2 | unchanged `udp.rs` | UDP payload/source | None | Covered |
| R4 UDP | S13 no-data nonblock | D6 | 1.1, 5.2 | UDP recv/poller | UDP EAGAIN marker | None | Covered |
| R5 readiness | S14 not ready | D6 | 1.1, 5.2 | axpoll/VFS/axnet | poll-zero + EAGAIN | None | Covered |
| R5 readiness | S15 becomes ready | D3, D6 | 4.2, 5.2 | service/listener/poll | poll-ready + I/O | None | Covered |
| R6 isolation | S16 sync migration | D1-D5 | 2.1-5.2 | full diff | source review/build | None | Covered |
| R6 isolation | S17 compile-only protocols | D1 | 5.1 | axnet feature set | exact-feature checks | None | Covered |

**Verification**

Dependency and source Gate:

```sh
cargo metadata --offline --format-version 1
cargo tree --offline -p starryos --features qemu
rg 'preprocess|snoop_tcp_packet|incoming_tcp_packet' crates/axnet/src
rg 'starry-smoltcp' Cargo.lock
```

通过条件：metadata/tree 显示两个本地 path；两个 `rg` 不出现禁止实现或
旧 dependency。失败表示依赖边界或私有 API 未清除。

Crate Gate:

```sh
cargo test --offline --manifest-path crates/smoltcp/Cargo.toml \
  --no-default-features \
  --features "alloc log async medium-ethernet medium-ip proto-ipv4 proto-ipv6 socket-raw socket-icmp socket-udp socket-tcp socket-dns" \
  --lib
cargo check --offline --manifest-path crates/axnet/Cargo.toml
cargo fmt --manifest-path crates/smoltcp/Cargo.toml -- --check
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
cargo fmt --all -- --check
```

通过条件：全部退出 0。失败先归因到 smoltcp、axnet 或 workspace 层。

Kernel build Gate:

```sh
make ARCH=riscv64 BUS=mmio NET=y build
```

通过条件：退出 0 并生成本轮 kernel image。`lwext4_rust` 的 sandbox
failure 只能记为 blocker。

Runtime Gate:

```sh
python3 scripts/ms01-qemu-test.py
```

通过条件：脚本退出 0，A4-A7 每项都有单一 PASS marker，没有 panic、
timeout、重复交付或 cleanup error。

**Persisted Evidence**

- Mode: required
- Root: `evidence/000-initial/`
- `README.md`：commit/diff、工具链、rootfs hash、kernel build 参数、
  QEMU 参数、payload hash、命令、退出码、场景到文件映射。
- `dependency-tree.txt`：A1-A3；文本；当前 checkout；不得包含 registry
  starry-smoltcp；本地 path 必须可见。
- `crate-gates.log`：A2、A3、A9；文本；精确 feature set；所有命令退出 0。
- `build.log`：A8；文本；RISC-V QEMU MMIO build；退出 0 和 image 路径。
- `qemu-socket-baseline.log`：A4-A7；原始 serial/harness 文本；本轮 image
  与 payload；所有 marker 通过。

Plan 不创建 evidence 文件。Act 采集后在 README 记录每份文件的 hash。

**Risks and Notes**

- 512 边界会临时分配较多 socket buffers；只在边界用例达到上限。
- QEMU shell 到达不代表网络 Gate 通过。
- serial 是控制与日志通道，guest loopback 是网络行为通道；两者结论分开。
- 旧镜像 basic TCP/UDP 已通过，但不能替代新镜像完整 payload。
- compile-only feature 通过不等于对应协议 runtime 已验证。
- 当前工作区有与本 change 无关的用户修改；Act 必须保留并隔离 review。

**Gate 2 Checks**

| Check | Status | Evidence |
|---|---|---|
| No Missing requirement | PASS | RTM 17 rows are Covered |
| Simplifications approved | PASS | No Simplified row; fixed backlog was approved at Gate 1 |
| Current implementation investigated | PASS | Cargo, kernel, axnet, smoltcp, tests and QEMU baseline above |
| Design closed | PASS | D1-D6; Open Questions is None |
| Tasks executable | PASS | tasks 1.1-6.1 include code, RED/GREEN, commands and stop conditions |
| Traceability complete | PASS | RTM links scenario, design, task, code and witness |
| Verification sufficient | PASS | dependency, crate, kernel and runtime Gates are layered |
| No implementation unknown | PASS | API delta is reduced to two resolved categories |
| Artifacts consistent | PASS | proposal, delta spec, design, tasks and this iteration share scope |
| Persisted Evidence defined | PASS | required files map to A1-A9 |
| User approves execution plan | PASS | Gate 2 approved 2026-07-28 by user directive |

Gate 2 is approved. Execution is authorized for this iteration.

## Act Response

- Status: blocked

**Implemented**

- Preserved task 1.1 and its 9/9 characterization witness.
- Switched the direct workspace `axnet-ng` dependency to `crates/axnet`.
- Pointed local axnet at smoltcp 0.13.1.
- Added the planned TCP bind sidecar.
- Added listener slots and fine-grained smoltcp polling.

Tasks 2.1 through 4.2 remain incomplete because task 2.1 is blocked.

**Changed Files and Symbols**

- `Cargo.toml`: workspace excludes and direct axnet path.
- `Cargo.lock`: local axnet and smoltcp resolution.
- `crates/axnet/Cargo.toml`: local smoltcp dependency.
- `crates/axnet/src/wrapper.rs`: TCP bind sidecar.
- `crates/axnet/src/tcp.rs`: sidecar and standard endpoint APIs.
- `crates/axnet/src/listen_table.rs`: listener slot lifecycle.
- `crates/axnet/src/service.rs`: maintenance, ingress, egress, and reconcile.
- `crates/axnet/src/router.rs`: removed SYN snooping and `preprocess`.

**Deviations from Plan**

The read-only Cargo registry prevented unpacking cached crates. Isolated
checks used a copied Cargo home under `/tmp` and the root lockfile. This did
not change product scope.

**Blocker Handoff**

- Discovered at: task 2.1 dependency source Gate
- Expected: path dependencies remove registry axnet-ng and `starry-smoltcp`
- Actual: `axruntime` still resolves registry axnet-ng; `axfeat` and
  `axruntime` also resolve `axnet`
- Impact: the approved path-only design cannot satisfy A1-A3
- Completed work: task 1.1
- Partial work: tasks 2.1, 3.1, 4.1, and 4.2
- Unstarted work: tasks 5.1, 5.2, and 6.1
- Worktree state: partial product changes are preserved; unrelated staged
  user changes were not reverted
- Gates: Gate 3 RED passed; isolated axnet compile passed; dependency source
  Gate blocked
- Evidence: EV-000-02, `../evidence/000-initial/blocker.md`
- Plan decision needed: select patching, feature changes, or another
  localization boundary
- Resume condition: a new approved iteration defines that dependency strategy

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 1
- Important findings unresolved: 0
- Minor findings unresolved: 0

The unresolved Critical finding is the dependency graph mismatch. The new
unused `with_socket` helper warning was removed and the isolated check was
rerun before the blocker was found.

**Verification Evidence**

| Verification | Command | Result | Conclusion |
|---|---|---|---|
| Root baseline | `cargo metadata --offline --format-version 1 --no-deps` | exit 0 | PASS |
| Task 2.1 RED | isolated `cargo check` with root lockfile | exit 101; 1 E0407 and 9 E0599 | PASS |
| Task 3.1 intermediate | same isolated check | exit 101; only E0407 remained | PASS |
| Partial tasks 4.1/4.2 | same isolated check | exit 0 | PASS |
| Dependency source | `cargo tree --offline -p starryos --features qemu -i starry-smoltcp` | exit 0; fork still present | BLOCKED |

**Persisted Evidence**

EV-000-02: `../evidence/000-initial/blocker.md`.

**Experience Candidates**

None.

**Remaining Issues**

The root dependency graph needs a Plan decision. Required files
`dependency-tree.txt`, `crate-gates.log`, and `build.log` are absent because
their Gates did not pass.

**Commit or Diff Reference**

Current worktree diff; no commit created.

## Plan Review

- Status: complete

**Review Result**

follow-up-required

**Findings**

1. **Critical — PLAN-OMISSION：依赖切换没有覆盖 transitive feature graph。**
   根 workspace 的直接 path dependency 只替换了 kernel 的直接边。
   `axfeat/net-ng` 同时启用 `axfeat/net`，后者继续让 `axruntime/net`
   解析 registry `axnet`；`axruntime/net-ng` 又解析 registry
   `axnet-ng`。因此当前图同时包含本地 `axnet-ng`、registry
   `axnet-ng`、legacy `axnet` 和 `starry-smoltcp`，task 2.1 的来源
   验收不可能按原方案通过。
2. **Important — ACT-DEVIATION：characterization witness 未按批准的
   harness 契约实现。** task 1.1 要求动态 serial port、自动上传、marker
   判定、timeout 和 cleanup；实际使用固定 HTTP 端口加人工 QEMU 操作。
   该证据可保留为旧 fork 行为记录，但不能作为新栈 Gate。
3. **Important — ACT-DEVIATION：512 容量断言弱于任务契约。**
   `test_tcp_512_capacity` 允许仅连接和接受 256 个 socket 就通过。现有
   日志碰巧记录 512/512，但测试本身不能阻止容量回退，也没有覆盖第 513
   个连接或释放一个 slot 后的即时恢复。
4. **Important — PLAN-OMISSION：满队列恢复缺少时序契约。** 当前
   `accept` 消费 slot 后不补空闲 listener；`Service::poll` 又在 ingress
   后才 reconcile。队列曾满 512 时，释放容量后的第一个新 SYN 可能在
   listener 补位前到达。必须在 ingress 前恢复 listener 容量，并以
   “512 满载 → accept 一个 → 立即新建连接成功”作为 RED/GREEN。
5. **Important — ACT-DEVIATION：egress 只推进一次。** design D3 和
   iteration 000 要求循环 `poll_egress` 直到 `PollResult::None`；当前
   `Service::poll` 只调用一次，不能证明一次 poll 会排空当前可推进的
   egress 工作。
6. **Important — ACT-DEVIATION：lockfile 带入无关 registry 升级。**
   当前 `Cargo.lock` 除本地 axnet/smoltcp 外还升级了 `addr2line`、
   `either`、`regex`、`rand`、`zerocopy` 等包。MS01 不应把依赖来源切换
   扩展成全局依赖刷新。
7. **Minor — NEW-EVIDENCE：旧 fork 与目标标准栈的验收语义需要分离。**
   旧证据允许 UDP nonblocking 返回 `ENOTCONN`，并在 close/relisten
   前等待 2 秒。它们适合作为 K33 characterization，不是迁移后的
   acceptance；新栈必须只接受 `EAGAIN/EWOULDBLOCK`，且移除人为等待。
8. **Important — BASELINE-CHANGED：全局 K33 spec 当前 strict invalid。**
   已暂存的 `knowledge/K33` 正文没有规范性 `MUST`，因此
   `openspec validate --specs` 失败。Plan 和 Act 均无权修改全局项目
   knowledge；它必须由 docs maintainer 在执行新迭代前修复。

**Deviation Classification**

`PLAN-OMISSION`, `ACT-DEVIATION`, `BASELINE-CHANGED`, `NEW-EVIDENCE`

**Evidence**

- `cargo tree --offline -p starryos --features qemu -i starry-smoltcp`
  显示 `axfeat/net-ng -> axfeat/net -> axruntime/net -> axnet ->
  starry-smoltcp`。
- registry `axfeat 0.3.0-preview.2` 声明
  `net-ng = ["net", "irq", "multitask", "axruntime/net-ng"]`；
  `axruntime 0.3.0-preview.2` 分别以 `net` 和 `net-ng` 激活 legacy
  `axnet` 与 `axnet-ng`。
- 在当前源码副本上仅加入根
  `[patch.crates-io] axnet-ng = { path = "crates/axnet" }`，并把
  kernel QEMU feature 的 `axfeat/net-ng` 拆为
  `axdriver/virtio-net` 与 `axruntime/net-ng` 后，
  `cargo metadata --offline` 通过；反向 tree 中没有
  `starry-smoltcp` 或 legacy `axnet`，且只有一个本地 `axnet-ng`。
- `crates/axnet/src/service.rs` 当前只有一次 `poll_egress`；
  `listen_table.rs` 的 `accept` 消费 slot 后没有 refill。
- `tests/ms01_socket_baseline.c` 的容量通过阈值是 256；脚本入口是
  `scripts/ms01-prepare.sh`，不是批准的自动化
  `scripts/ms01-qemu-test.py`。
- `git diff -- Cargo.lock` 显示本地包变化之外的多项 registry
  version/checksum 漂移。
- `evidence/000-initial/qemu-socket-baseline.log` 记录旧镜像 9/9 PASS；
  `blocker.md` 记录 dependency source Gate blocked。
- `openspec validate knowledge --type spec`：K33 requirement 缺少
  `SHALL` 或 `MUST`，exit 1。

**Follow-up Decision**

保留已完成的 fork characterization 和当前未完成的产品 diff，不回退也不
继续按旧 task 2.1 执行。下一轮采用最小 feature 图修正：

1. 根 `[patch.crates-io]` 将所有 `axnet-ng` 解析到 `crates/axnet`。
2. kernel QEMU feature 不再启用聚合的 `axfeat/net-ng`，改为直接启用
   `axdriver/virtio-net` 与 `axruntime/net-ng`；保留 registry
   `axfeat`/`axruntime`，不本地化这两个 crate。
3. 收紧 lockfile、自动化 harness、512 容量恢复、pre-ingress refill 和
   egress-until-none 契约，再完成原有 bind/listener/build/QEMU Gates。
4. iteration 001 进入 Act 前，先由 docs maintainer 修复 K33 的规范性
   requirement 正文并通过全局 spec validation。

**Next Iteration**

`openspec/changes/t01-smoltcp-axnet-baseline/iterations/001-dependency-recovery.md`
