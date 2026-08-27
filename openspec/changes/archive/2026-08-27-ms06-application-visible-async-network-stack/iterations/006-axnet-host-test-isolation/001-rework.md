# Iteration 006 / Cycle 001: pair fixture deferred removal with its socket context

## Plan Context

- Status: ready
- Approval: approved — 用户于 2026-08-27 原话：“批准，更改gate状态然后开始实施”；本 Cycle 已授权 `openspec-act`
- Iteration: 006-axnet-host-test-isolation
- Cycle: 001-rework
- Cycle Type: rework
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: reopened Task 5.1；Task 5.2 保持完成
- Depends on: Iteration 005 accepted；Cycle 000 的直接 socket/listener context 与 diagnostic clock 实现保留
- Stable baseline: fixture socket 的直接访问和 deferred removal 都由同一 context 拥有；相同数值 handle
  不会被 global 或相邻 fixture 的 Service 解释或回收。
- Verification boundary: deferred TCP/UDP RED witnesses、local enqueue/reap、global/neighbor non-interference、
  R57/fixture regressions及两 profile 默认并行 full suites。
- Diagnostic boundary: test fixture context、Service deferred queue、SocketSet 配对、Drop 和 reaper verdict；
  首次越过产品 deferred ownership 语义时停止。
- Deferred tasks: Iteration 007 Task 6.1；Iteration 008 Tasks 7.1-7.2

**Cycle Scope**

- Trigger: Cycle 000 Review Result `rework-required`
- Acceptance gaps: TCP deferred close 与 UDP queued-TX Drop 把 fixture-local handle 提交给 global `SERVICE`
- Repair items: T5.1-R1
- Inherited scope: Task 5.1；R57；Cycle 000 的 `SocketTestContext`、socket/listener routing、accepted child
  context、产品 singleton 与锁序约束
- Excluded scope: Task 5.2 clock/hold 实现；产品 deferred-close/queued-TX 语义；readiness、terminal、
  PollSet、QEMU runtime、automatic qualification、scheduler、reset/SMP、真板、性能和 commit

**Objective**

使 test fixture 的 deferred-removal queue 与创建 handle 的 SocketSet 成对存在。TCP deferred close 和 UDP
queued-TX Drop 必须把 handle 提交给本 fixture 的 Service；产品 socket 仍提交给唯一 global Service。

**Scenario Sketch**

| Scenario | Precondition | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| S1 TCP deferred close | fixture TCP socket 已进入可达的 deferred close 状态 | Drop public socket | 只在本 fixture Service 入队；local reaper 使用同一 SocketSet 完成 verdict/reap | local handle 到达 global/neighbor Service |
| S2 UDP queued TX | fixture UDP raw socket 存在待发送 datagram | Drop public socket | public handle 退役，local Service 恰好入队一次；drain 后 raw handle 被本地回收 | global Service 入队或 datagram 被提前删除 |
| S3 equal numeric handle | global 或另一 fixture 拥有相同数值 handle | fixture 执行 deferred Drop 和 local drain | 其他 context 的 socket 与 backlog 均不变 | 数值碰撞导致误查、误删或 stale-handle panic |
| S4 ordinary Drop | fixture TCP idle、listener 或 UDP empty-TX socket | Drop | 保持 Cycle 000 的直接本地清理路径 | 为修复 deferred 分支而改变普通 Drop |
| S5 production path | socket 没有 test context | deferred Drop | 继续提交 global Service，并由 global SocketSet 回收 | test seam 成为第二个产品 registry |

**Current Baseline**

- Branch `net-k3`；HEAD `1ea51427d8692f5a12b87a0403b940e73d43fed3` 加当前 MS06 工作树。
- Cycle 000 Act Response 为 `reported`，Review Result 为 `rework-required`。Task 5.2 focused tests 与
  ordinary/diagnostics full-suite 历史证据保留。
- `SocketTestContext` 当前只拥有 `SocketSetWrapper` 与 `ListenTable`；没有与该 SocketSet 配对的 Service。
- Cycle 000 新增的 Drop tests 覆盖 idle TCP、listener churn 和 empty-TX UDP，没有执行 deferred branches。

**Current-State Evidence**

- `tcp.rs::Drop` 先在 `self.sockets()` 退役 public handle，但 FIN-WAIT-1/CLOSING/LAST-ACK 分支随后从
  `crate::SERVICE` 取得进程级 Service 并提交该 handle。
- `udp.rs::Drop` 在 raw socket 有 queued TX 时同样调用 `crate::SERVICE`，而该 handle 来自
  `self.sockets()` 指向的 fixture SocketSet。
- `Service::queue_deferred_removal` 的 entry 只保存 `SocketHandle` 与 close kind；`stack_round` 用调用方传入的
  SocketSet 解释 handle。global runner 传入产品 `SOCKET_SET`，无法识别 fixture 所有权。
- `Service::new_with_listen_table` 已有 test 构造 seam，可为 fixture 建立 local Router/ListenTable/Service；
  修复不需要改变产品 Service 所有权。
- 独立复核的 Cycle 000 focused tests 均通过，说明直接调用面和 clock seam 可保留，但不能证明 deferred path。

**Critical Path**

```text
fixture creates SocketSet + ListenTable + deferred Service as one context
  -> TCP/UDP Drop resolves service from that context
  -> queue stores local handle
  -> local stack_round/reaper receives the same SocketSet
  -> handle retires exactly once; equal numeric handles elsewhere remain untouched

production socket without fixture context
  -> existing global SERVICE
  -> existing global SOCKET_SET
```

**Behavioral Change**

- Host fixture deferred Drop 从 global Service 改为 fixture-local Service；local queue 和 SocketSet 成对。
- 产品 socket、global `SERVICE`/`SOCKET_SET`、deferred verdict、queued datagram 生命周期及锁序不变。
- Task 5.2 的 `DiagTestClock`、Service/Rx future clock 路由和测试不变。

**Change Surface**

| Repair | File/Symbol | Current responsibility | Planned change |
|---|---|---|---|
| T5.1-R1 | `wrapper.rs::SocketTestContext` | local SocketSet + ListenTable | 增加与该 context 配对的 test-only Service owner/accessor |
| T5.1-R1 | `tcp.rs::TcpSocket::drop` | direct access local，deferred enqueue global | 由 socket context 解析 deferred Service |
| T5.1-R1 | `udp.rs::UdpSocket::drop` | direct access local，queued-TX enqueue global | 使用相同 deferred Service 路由 |
| T5.1-R1 | TCP/UDP/Service tests | 只覆盖普通 fixture Drop | 增加 deferred enqueue、local drain 与 equal-handle non-interference witnesses |

**Task Contract**

### T5.1-R1: keep deferred removal inside the fixture context

- Requirement/Scenario: Task 5.1；Cycle 000 Acceptance 1/2/4；S1-S5。
- Targets: `SocketTestContext`、TCP/UDP deferred Drop 路由、local Service/reaper tests 和 source guards。
- Current behavior: fixture-local handle 在 deferred branch 被提交给 global Service，之后由 global SocketSet 解释。
- Required behavior: fixture context 同时提供 sockets、listener table 与 deferred Service；任何 fixture socket 从
  create 到 Drop/reap 只进入该 context。没有 fixture context 的产品 socket 保持 global route。
- RED witness: 分别构造 TCP 可达 deferred-close 状态与 UDP queued-TX，Drop 后断言 local backlog 增一、
  global/neighbor backlog不变；在相同数值 handle 共存时运行 local drain，断言只回收目标 fixture socket。
  若 smoltcp 状态不能通过 public 操作稳定到达，可增加最小 test-only state seed，但不得改变产品状态机。
- Preserve: deferred verdict 与 close kind、UDP datagram 先发送后回收、TCP close progression、exactly-once reap、
  products globals、`SERVICE -> SOCKET_SET -> listener entry` 锁序、Cycle 000 ordinary Drop 和 Task 5.2。
- Forbidden: 用串行化、global reset、不同 handle/port、skip/retry规避碰撞；把 local handle 转换为 global handle；
  修改 smoltcp handle 算法、产品 close/queued-TX 语义或为产品增加第二套 registry。
- GREEN condition: TCP/UDP deferred witnesses在两 profile 重复通过；local queue 可由配对 SocketSet drain；
  global/neighbor socket 与 queue 不变；source review 不再存在 fixture Drop 直达 raw `crate::SERVICE` 的路径。
- Stop when: local Service 不能在不改变产品 ownership/锁序的情况下配对，或 deferred failure 越出
  fixture/Service/SocketSet 边界；返回 Plan 重新归因。

**Invariants**

- 一个 deferred entry 只能由创建其 handle 的 SocketSet 解释和回收。
- fixture context 不进入非 test 布局；production constructor 仍绑定唯一 global Service/SocketSet。
- queued UDP datagram、TCP close progression和exactly-once removal不因路由修复而改变。
- wake 前释放 registry/listener guard；既有锁序不变。

**Non-goals**

- 重做 diagnostic clock、修复所有 warning、建立通用 dependency-injection framework。
- 产品网络行为、guest probe、automatic qualification、QEMU runtime、scheduler/reset/SMP、真板和性能。

**Acceptance**

1. TCP deferred close 与 UDP queued-TX fixture Drop 只向本 context 的 Service 入队。
2. local Service 使用配对 SocketSet 完成 verdict/reap；global/neighbor 中相同数值 handle 不受影响。
3. ordinary fixture Drop、accepted child context、R57 subset与 Task 5.2 regressions保持通过。
4. 产品 socket 仍使用 global Service/SocketSet；deferred语义、锁序和非 test 布局不变。
5. ordinary与qemu-diagnostics默认并行 full suites各连续三次通过，无 skip/ignore、无限重跑或全局串行。
6. format、source guards、strict OpenSpec、diff check和full diff Review通过，无 Critical/Important finding。

**Verification**

- TCP deferred-close、UDP queued-TX、equal-handle non-interference 和 local drain focused tests，两 profile 各 ×100。
- Cycle 000 local-context/R57 focused subset回归；Task 5.2 two-clock、interleave与目标 flake focused回归。
- `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` 连续三次。
- 同命令增加 `--features qemu-diagnostics` 连续三次；不得增加 `--test-threads=1`。
- axnet范围 format、fixture/global route source guards、strict OpenSpec、`git diff --check` 和 full diff Review。
- host tests 继续使用既有 non-PIE linker wrapper；缺失时只恢复环境，不计产品修复。
- SKIPPED: QEMU、MS01/MS04/MS05/MS06 runtime 与 root 全量产品资格；属于 Iterations 008/007。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | 两个 Drop 分支、Service entry 与 global runner 的 SocketSet 配对已定位 |
| Design | PASS | fixture-local Service 与现有 local SocketSet/ListenTable 组成同一 context；产品路径不变 |
| Iteration Plan | PASS | 只重开 Task 5.1；Task 5.2 保持完成，Iteration Map不变 |
| Cycle Scope | PASS | 单一 Important finding；排除产品语义、clock和后续资格 |
| Task Contract | PASS | T5.1-R1 含 deferred RED、GREEN、保持/禁止、验证和停止条件 |
| Traceability | PASS | Cycle 000 Acceptance 1/2/4 映射到 S1-S5 与三类 ownership witness |
| Verification | PASS | focused ×100、两 profile full ×3、source/full diff/OpenSpec Gate |

Gate 2 技术检查项 PASS；状态保持 `draft`，等待用户审计和明确批准。未授权 `openspec-act`。

**Persisted Evidence**

- Mode: none

结果可低成本重跑；由 Act Response 记录命令、决定性输出、exit、文件和符号，不创建 Evidence 占位目录。

**Risks and Notes**

- TCP deferred 状态可能需要最小 test-only seed；该 seam 只能建立 RED witness，不能绕过产品 close progression。
- Cycle 000 的新增 unused-import/dead-code warning 记录为 Minor；本 Cycle 不为清理 warning 触碰已接受的
  Task 5.2，除非改动 T5.1-R1 直接产生或使其成为编译阻塞。
- full suite 绿只能作为回归；T5.1-R1 必须有确定性 deferred owner witness。

## Act Response

- Status: reported

**Implemented**

- T5.1-R1a（deferred removal owner 停留 fixture context，Cycle 000 `rework-required` 修复）：
  - `wrapper.rs`：`SocketTestContext` 增加 `service: &'static Mutex<Service>` 字段，`leak_new()` 为每个
    fixture 以同一 `ListenTable` 和 loopback-only Router 构造配对的 `new_with_listen_table` Service；三件套
    （sockets/listen_table/service）成为单一 context。新增 2 个见证：
    `fixture_context_pairs_service_with_its_own_registries`、`fixture_service_constructor_routes_the_local_registries_in_source`。
  - `tcp.rs`：新增 `deferred_service()`（`#[cfg(test)]` ctx 分支优先返回 `ctx.service`，产品回退
    `crate::SERVICE.get()`）；`TcpSocket::drop` 的 FIN-WAIT-1/CLOSING/LAST-ACK 分支改经
    `self.deferred_service()`。新增 3 个见证：S1 `tcp_deferred_close_enqueues_into_fixture_service`、S3+local
    drain `tcp_deferred_close_local_drain_reaps_only_the_owning_fixture`、source guard
    `tcp_deferred_drop_routes_service_through_the_socket_context_in_source`。
  - `udp.rs`：同构 `deferred_service()`；`UdpSocket::drop` 的 queued-TX 分支改经 `self.deferred_service()`。
    新增 3 个见证：S2 `udp_queued_tx_drop_enqueues_into_fixture_service`、S2+local drain
    `udp_queued_tx_local_drain_reaps_only_the_owning_fixture`、source guard
    `udp_queued_tx_drop_routes_service_through_the_socket_context_in_source`。

- T5.1-R1b（Round 1 Important fix：TCP state seed compile-time test-only）：
  - `crates/smoltcp/Cargo.toml`：声明非默认 feature `"test-seeds" = []`（不进 default）。
  - `crates/smoltcp/src/socket/tcp.rs`：`seed_state_for_tests` 加 `#[cfg(feature = "test-seeds")]`——普通
    smoltcp/产品依赖图不编译此方法。
  - `crates/axnet/Cargo.toml`：新增 `[dev-dependencies.smoltcp]`（path/version + `features = ["test-seeds"]`）。
  - `tcp.rs` tests 新增 source guard `seed_state_api_is_compile_time_test_only_across_manifests`。

- T5.1-R1c（Round 2 Important fix：test graph 相对产品 edge 只新增 `test-seeds`）：
  - `crates/axnet/Cargo.toml` `[dev-dependencies.smoltcp]` 显式 `default-features = false`。Cargo 对同
    package 的 normal 与 dev edge 做 feature union；此前 dev edge 未关 defaults，导致 axnet test build 额外
    启用 smoltcp 整套 default（std、raw/TUN-TAP PHY、802.15.4、fragmentation、DHCP/mDNS、multicast 等），
    full suites 因此在产品不等价的 feature graph 上运行。
  - 扩展 `seed_state_api_is_compile_time_test_only_across_manifests`：断言 dev edge 必须同时具备
    `features = ["test-seeds"]` 与 `default-features = false`，把「test graph 只新增 test-seeds」固定在
    manifest。

**Changed Files and Symbols**

- `crates/axnet/src/wrapper.rs`：`SocketTestContext::{service}`、`SocketTestContext::leak_new`；tests。
- `crates/axnet/src/tcp.rs`：`TcpSocket::{deferred_service}`；`impl Drop for TcpSocket` 路由；tests
  （含 `seed_state_api_is_compile_time_test_only_across_manifests`）。
- `crates/axnet/src/udp.rs`：`UdpSocket::{deferred_service}`；`impl Drop for UdpSocket` 路由；tests。
- `crates/axnet/Cargo.toml`：`[dev-dependencies.smoltcp]` = { path, version, default-features = false,
  features=["test-seeds"] }。
- `crates/smoltcp/Cargo.toml`：`"test-seeds" = []` feature。
- `crates/smoltcp/src/socket/tcp.rs`：`Socket::seed_state_for_tests` 加 `#[cfg(feature = "test-seeds")]`。
- `openspec/changes/.../iterations/006-axnet-host-test-isolation/001-rework.md`：Act Response。

**Deviations from Plan**

- D1（见证形态，R1a）：恢复时实现已随工作树就位；RED witness 以临时中和 `deferred_service` 的 fixture
  分支观察——`git diff` 快照记录原始未暂存 diff，还原后逐字节一致。
- D2（环境）：`/tmp/opencode/cc-nopie.sh` 缺失，按 Iteration 004 Cycle 000 D1 记录重建。
- D3（lock 刷新，R1b）：新增 dev-dependencies 条目后 `--locked` 首报需更新 Cargo.lock；`--offline` 刷新
  一次后恢复，Cargo.lock 最终无 diff。
- D4（guard 断言自校正，R1b）：新 guard 的字符串扫描做三处鲁棒化（trim 行尾、引号风格 feature 声明、
  按行首 `[` 定位下一条 section）。均为测试断言修正，不影响产品行为。
- D5（lock 重刷新，R1c）：`default-features = false` 改变解析后 `--locked` 再次报需更新；`--offline`
  刷新一次后恢复，Cargo.lock 仍无 diff（feature/defaults 不进 lock）。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

Spec review（逐条对照 T5.1-R1 契约与两轮 Plan Review finding）：deferred owner 修复——fixture context
同时提供 sockets/listen_table/service；fixture socket 自 create 至 Drop/reap 只进入自身 context；产品
socket 保持 global route；RED witness 覆盖 TCP/UDP deferred 入队、等值 handle local drain
non-interference（S1-S5）；Preserve 全满足（verdict/close kind、UDP 先派发后回收、close progression、
exactly-once reap、锁序、Cycle 000 普通 Drop、Task 5.2）；Forbidden 全未触犯。Round 1 gate 修复——
`seed_state_for_tests` 仅随 smoltcp 非默认 `test-seeds` 编译，normal smoltcp/axnet 产品 edge 未启用
（已关闭）。Round 2 修复——`[dev-dependencies.smoltcp]` 显式 `default-features = false`，`cargo tree`
证据证明 test graph 相对产品 edge 只新增 `test-seeds`、无 `smoltcp feature "default"` 泄漏（`[dev-dependencies]`
来源的 default 计数 0，产品 edge test-seeds 计数 0）；两 profile full suites 已在修正后（产品等价）
graph 上重建并 ×3 通过，Acceptance 3/4/5 回归与产品路径不变要求满足。Code quality review：diff 无计划外
修改且最小；guard 断言覆盖 manifest 两处结构；无新增警告；测试通过原因由 RED 实证（R1b guard 未修复前
失败于定义 cfg；R1c guard 未修复前失败于 dev defaults 缺失，均 exit 101）。遗留 Minor：既有 warning 按
本 Cycle Risks and Notes 保留，不阻塞。

**Verification Evidence**

| 验证项 | 命令或操作 | 输出摘录 | 结论 |
|---|---|---|---|
| RED (R1a) | 中和 `deferred_service` fixture 分支后运行 5 个 deferred 见证 | `0 passed; 5 failed`；`left: 0, right: 1`；`EXIT=101` | RED 命中 |
| RED (R1b) | 未修复源码上先写并运行 seed gate guard | `must be cfg-gated...` panic；`EXIT=101` | RED 命中 |
| RED (R1c) | guard 追加 dev defaults 断言后运行（dev edge 尚缺 `default-features`） | `the dev edge must close smoltcp defaults...` panic；`EXIT=101` | RED 命中（defaults 污染 test graph） |
| Gate 3 还原 | 还原后与 `git diff` 快照 `diff` | `TCP-RESTORED-IDENTICAL`、`UDP-RESTORED-IDENTICAL` | 工作树未受污染 |
| 修正 graph 证据 | `cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i smoltcp` | `smoltcp feature "default"` 计数 **0**；`test-seeds` 来源 `[dev-dependencies] axnet-ng`；产品 edge `--edges normal` 下 test-seeds 计数 0 | PASS |
| focused ×100（两 profile） | 修正 graph 上重建的 binaries：`tcp_deferred_close udp_queued_tx fixture_context_pairs fixture_service_constructor` 循环 100 | ordinary / diag 各 `fails=0` | PASS |
| 回归 ×100（两 profile） | `fresh_fixtures two_fixture_listeners parallel_fixture_churn udp_fixtures listener_reset terminal_commit fatal_between two_fixture_clocks concurrent_fixture_clocks reclaim_hold_drains seed_state_api` 循环 100 | ordinary / diag 各 `fails=0` | PASS |
| ordinary full ×3（修正 graph） | `RUSTFLAGS=…cc-nopie cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib` | `ok. 371 passed; 0 failed` ×3，exit 0 | PASS |
| diagnostics full ×3（修正 graph） | 同上 + `--features qemu-diagnostics` | `ok. 393 passed; 0 failed` ×3，exit 0 | PASS |
| 产品构建 | `cargo check --manifest-path crates/smoltcp/Cargo.toml --offline --lib`；`cargo check --manifest-path crates/axnet/Cargo.toml --offline --lib` | 均 exit 0；无 unexpected_cfgs | PASS |
| fmt / diff | `cargo fmt --check`；`git diff --check`；`git diff --cached --check` | 均 exit 0 | PASS |
| OpenSpec | `openspec validate ms06-application-visible-async-network-stack` | `Change ... is valid` | PASS |

**Persisted Evidence**

None required（结果可低成本重跑；命令、决定性输出、exit 与符号均在 Act Response 记录）。本 Cycle 未创建
Evidence 目录。

**Experience Candidates**

None（R57 既有 Incident 已登记相关知识；T5.1-R1 a/b/c 均为按 Review 修复的常规工程过程，无新增可重复高
风险操作或系统性故障诊断待记录）。

**Remaining Issues**

- 未运行 QEMU runtime / MS01/MS04/MS05/MS06 runtime / root 全量产品资格：按 Iteration 006 Deferred scope
  分别属于 Iteration 007（Task 6.1）与 008（Tasks 7.1-7.2）。
- 既有 Minor 警告（unused import、`set_test_now` dead code 等）按本 Cycle Risks and Notes 保留，不阻塞
  Acceptance；未触碰已接受的 Task 5.2 实现。

**Commit or Diff Reference**

未提交；改动在工作树。本 Cycle 未暂存 diff 概览：`wrapper.rs` +92/-19、`tcp.rs` +224/-4、`udp.rs`
+187/-3、`axnet/Cargo.toml` +10、`smoltcp/src/socket/tcp.rs` +14、`smoltcp/Cargo.toml` +6；另含
`001-rework.md` 本 Act Response。

## Plan Review

- Review Result: accepted

**Findings**

- No Critical/Important findings。
- 上一版 default-feature finding 已关闭：dev smoltcp edge 显式 `default-features = false`；测试图不含
  smoltcp `default`，normal产品图不含 `test-seeds`。
- deferred owner 与 seed gate 通过独立源码复核：fixture Service/SocketSet配对，TCP/UDP Drop不再把local
  handle交给global Service；状态seed只在非默认test feature下编译。
- [Minor] Cycle 000 已记录的 unused import 与 `set_test_now` dead-code warning 仍存在，不阻塞本次
  Acceptance；其余既有axnet/smoltcp warning同样不扩大为本Cycle清理。

**Deviation Classification**

ACT-DEVIATION resolved。R1b/R1c分别关闭公开状态seed与dev default-feature泄漏；未改变Iteration目标、产品
socket语义或验证边界。

**Acceptance Gaps**

None。Acceptance 1-6全部满足。

**Convergence**

Closed。Cycle 000的直接context与clock隔离、Cycle 001的deferred owner配对、状态seed gate及产品等价测试图
共同关闭Tasks 5.1-5.2；Iteration 006形成可供自动资格依赖的稳定host-test基线。

**Evidence**

- 独立`cargo tree -e features -i smoltcp`：`smoltcp feature "default"` absent；`test-seeds`只来自axnet
  dev-dependency。`cargo tree -e normal -i smoltcp`不含`test-seeds`。
- 独立执行修正graph生成的371-test ordinary与393-test diagnostics binaries：seed manifest guard、TCP/UDP
  local drain均PASS；diagnostics two-clock witness PASS。
- Act在修正graph记录focused/regression各×100、ordinary 371/371 ×3、diagnostics 393/393 ×3，均exit 0。
- 独立`cargo check`：smoltcp lib、axnet lib与root `starry-kernel --features qemu` exit 0。D1仍为既有
  20×E0432/5×E0433负基线，由后续automatic auditor判定，不归因于本Cycle。
- `git diff HEAD --check`、strict OpenSpec和full diff Review通过。

**Follow-up Decision**

接受Cycle 001与Iteration 006；按既有Map展开Iteration 007 `automatic-integration-qualification/000-initial.md`。

**Iteration Plan Update**

None；Iteration Map 保持不变。

**Next Cycle**

None.

**Next Iteration**

`007-automatic-integration-qualification/000-initial.md`。
