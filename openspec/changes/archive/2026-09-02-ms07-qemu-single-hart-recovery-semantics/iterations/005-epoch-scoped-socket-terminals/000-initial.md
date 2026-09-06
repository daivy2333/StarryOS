# Iteration 005 / Cycle 000: Epoch-scoped Socket Terminals

## Plan Context

- Status: ready
- Iteration: 005-epoch-scoped-socket-terminals
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 3.2
- Depends on: Iteration 004 accepted
- Stable baseline: 旧 SocketEpoch 的 public、listener 与 deferred owners 永久返回同一 terminal；link/recovery 开放新 SocketEpoch 后，新建 socket 不继承旧 terminal，readiness、多 waiter 与清理所有权保持闭合。
- Verification boundary: TCP、UDP、listener、deferred retirement、poll 后 I/O 与错误映射 focused tests，以及 axnet ordinary/qemu-diagnostics 串行全量通过。
- Diagnostic boundary: SocketEpoch registry、`NetworkTerminal` 映射、bridge wake、public/hidden/raw handle identity 与清理线性化点。
- Deferred tasks: 4.1、4.2

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: R4、R7、D1、D5、D7；Iteration 004 接受的 checked SocketEpoch transition seam、link/recovery 复合 gate、唯一 resident owner、first-wins terminal 与 guard 外 wake。
- Excluded scope: QEMU V4 ABI、recovery probe/validator、真实 HMP runtime、重建整个 `SocketSet`、旧连接透明迁移、SMP、PCI/DWMAC runtime、真板与性能。

**Objective**

把当前 boot-global terminal 改造成 SocketEpoch 作用域的稳定 terminal。关闭 epoch 时先提交结构化 `NetworkTerminal`，再唤醒该 epoch 的全部 bridge；旧 handle 的 readiness 与紧随其后的 I/O 永久返回相同错误。新 epoch 开放后只允许新建 handle 正常工作，不清除或复活旧 handle；listener hidden sockets 与 deferred raw sockets随其所属 session epoch 恰好清理一次。

**Background**

Iteration 004 已建立 link down/up 对 SocketEpoch 的 checked transition seam，但 public socket 尚未绑定该 identity。当前 `SocketSetWrapper` 只有一个 first-wins `global_terminal: AtomicU64`：一次 recovery fault 会污染之后创建的所有 handle，若清空它又会让旧 handle 复活。Task 3.2 必须把 terminal identity 与 handle 创建 epoch 绑定，并将 link down、reset/old epoch、deadline、诊断取消和 ownership fault 映射到应用可观察的稳定错误。

**Current Baseline**

- Revision：`596b324b6e7cb78b3a4308b997657b6d0c95d44a`；Iteration 004 产品与测试改动仍在工作树。
- `SocketSetWrapper` 保存单个 `global_terminal: AtomicU64`，`publish_global_fault_code` first-wins 提交后 snapshot 全部 public bridge 并在 guard 外 wake。
- `add_public` 与 `install_readiness` 只登记 `SocketHandle → ReadinessBridge`，未记录创建 SocketEpoch；late-created handle通过 TCP/UDP 的 effective global snapshot继承旧 fault。
- `ReadinessBridge` 已有 per-handle first-wins local terminal、多 waiter fan-out、error-before-wake 与 one-shot rearm，可保留为 epoch publication 的 wake/本地错误基础。
- TCP/UDP 每次 poll 与 I/O 都调用 `effective_terminal_code(global, local)`；映射仍以 `DevError` mirror code 为主，缺少 `ConnectionReset`、`NotConnected`、`TimedOut`、`Interrupted` 的 `NetworkTerminal` 身份。
- stack runner 的 recovery fault publication 直接调用 `SOCKET_SET.publish_global_fault_code(outcome.fault_code)`，尚未携带或关闭目标 SocketEpoch。
- listener hidden handles 不进入 public readiness registry；deferred TCP/UDP raw owners由 Service bounded reaper管理，现有 retire/remove 分工已保证普通路径不会重复移除。
- 审计基线：axnet ordinary 442/442、qemu-diagnostics 466/466，均 exit 0。

**Current-State Evidence**

1. `wrapper.rs::SocketSetWrapper` 的 `global_terminal` 是 boot-global first-wins atomic，registry value只有 bridge，没有 epoch或session identity。
2. `wrapper.rs::{add_public,install_readiness}` 创建或采纳 bridge 时不读取 SocketEpoch；`global_terminal_code` 会让旧 fault覆盖所有 late add。
3. `tcp.rs::terminal_code` 与 `udp.rs::terminal_code` 都以 global code优先于 socket-local code，poll 和 I/O 已共享同一检查入口，适合替换为 handle-bound effective terminal。
4. `readiness.rs` 的 terminal codes只表达原 `DevError` 与 connect refused；D5 所需 reset/link/timeout/cancel类别尚无本地结构化类型。
5. `stack_runner.rs` 在 Service round guard释放后发布全局 fault；现有 guard 外 wake 顺序必须保留，但 publication 目标必须改为被关闭的 SocketEpoch。
6. `listen_table.rs` 管理 hidden listener与 accept outcome，`service.rs` 管理 deferred raw retirement；两者需要保存其来源 epoch，不能仅依赖可复用的 numeric `SocketHandle`。

**Relevant Code**

- `crates/axnet/src/readiness.rs::{ReadinessBridge,effective_terminal_code,terminal_ax_error}`：结构化 terminal、稳定映射、多 waiter 与 error-before-wake。
- `crates/axnet/src/wrapper.rs::SocketSetWrapper`：current/open epoch、epoch closure、public handle/bridge identity 与 registry snapshot。
- `crates/axnet/src/{tcp.rs,udp.rs}`：handle 创建/采纳、readiness overlay、poll 后 I/O terminal guard 与 `SO_ERROR`。
- `crates/axnet/src/listen_table.rs`：listener session、hidden handle、accept/reset outcome 与清理所有权。
- `crates/axnet/src/service.rs`：SocketEpoch transition、deferred owner identity与 bounded retirement。
- `crates/axnet/src/stack_runner.rs`：recovery/link terminal publication、Service round之后的 guard 外 wake。

**Critical Path**

```text
new/open SocketEpoch E
  add public socket or listener session -> record E on bridge/session/raw owner

link down or recovery terminal for E
  under the established owner/registry order:
    commit NetworkTerminal(E) first-wins
    mark E closed; snapshot only bridges/owners belonging to E
    stage listener/deferred cleanup exactly once
  release SERVICE / SOCKET_SET / listener guards
  wake every E bridge

old handle from E
  poll -> ERR/HUP overlay from terminal(E)
  immediate I/O retry -> same AxError forever

open SocketEpoch E+1
  new handles bind E+1 and see no terminal from E
  old E handles remain terminal; no migration or revival
```

**Implementation Guidance**

先用模型测试固定 `NetworkTerminal` 编码、AxError映射与 epoch registry 的 close/open/late-add语义，再把 public bridge、listener session和 deferred raw owner绑定到创建 epoch。最后把 stack runner 的 boot-global publication替换为针对明确 epoch 的 commit→snapshot→guard外 wake，并补 poll 后 I/O、handle reuse和 bounded cleanup见证。不要通过清零 global atomic、批量重建 `SocketSet` 或复制 terminal到易复用的 numeric handle来模拟 epoch。

**Behavioral Change**

- reset/old epoch、link down、deadline、诊断取消、ownership invariant与普通 device I/O 分别映射为 D5 规定的稳定 AxError。
- 关闭 SocketEpoch 只终结属于该 epoch 的 public/hidden/raw owners；提交 terminal先于任何 wake。
- 同一旧 handle 的 readiness、`SO_ERROR` 与紧随其后的 TCP/UDP I/O观察同一 terminal，不出现先 ready 后 `WouldBlock`。
- 新 epoch 的新 handle不继承旧 terminal；旧 TCP/listener不自动迁移或恢复。
- listener hidden socket和 deferred raw handle在 epoch closure与普通 drop/reaper交错时仍恰好清理一次。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| 3.2 | R4/D5 error identity | `readiness.rs` | DevError mirror codes + local bridge terminal | 引入有界 `NetworkTerminal` 及完整 AxError映射，保留既有 local terminal兼容 |
| 3.2 | R7/D7 epoch registry | `wrapper.rs::SocketSetWrapper` | boot-global first-wins publication | current/open epoch、per-handle epoch、epoch-scoped commit/snapshot/wake |
| 3.2 | R7 poll/I/O一致 | `tcp.rs`、`udp.rs` | effective global/local terminal | 以 handle epoch解析 terminal，并在每次 retry前稳定检查 |
| 3.2 | R7 owner cleanup | `listen_table.rs`、`service.rs` | hidden listener与 deferred raw retirement | session epoch归属、closure staging与 exactly-once removal |
| 3.2 | R4/R7 publication | `stack_runner.rs` | recovery fault boot-global publish | 将明确 terminal提交给目标 epoch，保留 guard外 wake与 bounded round |

**Task Contracts**

### 3.2: Epoch-scoped socket terminals

- Requirement/Scenario: R4错误映射；R7旧 socket、新 socket、commit-before-wake；D1、D5、D7。
- Depends on: Iteration 004 accepted 的 checked SocketEpoch seam、link/recovery gate、resident owner与 guard 外 wake。
- Targets: `crates/axnet/src/{readiness.rs,wrapper.rs,tcp.rs,udp.rs,listen_table.rs,stack_runner.rs,service.rs}`及对应 fixtures。
- Current behavior: boot-global `DevError` terminal first-wins；late-created handle也继承且无 clear，public/hidden/deferred owner未保存 SocketEpoch。
- Required behavior: 用本地 `NetworkTerminal` 稳定映射终结原因；bridge/session/raw owner绑定创建 SocketEpoch。关闭只终结旧 epoch，开放新 epoch后新 handle正常；旧 handle永久 terminal；listener/deferred/raw ownership恰好清理一次。
- Required changes: 建立 epoch registry与 open/closed terminal；使 handle创建/accept采纳/deferred retirement保留 epoch；把 recovery/link publication接到明确 epoch；让 TCP/UDP readiness、I/O与 `SO_ERROR` 共用同一 effective terminal。
- Preserve: 多 waiter fan-out、PollSet overflow替换语义、handle reuse隔离、local connect-refused terminal、`SERVICE → SOCKET_SET → ListenTable entry`锁序、guard外 wake、bounded listener/deferred stages和 V1–V3 ABI。
- Forbidden: 清空 global code令旧 handle复活；让旧 fault污染新 epoch；重建整个 `SocketSet`；按 numeric handle猜测 epoch；旧 TCP/listener透明迁移；guard内 wake或新增周期 polling。
- Test witness: 先写 RED覆盖 NetworkTerminal全部映射、epoch close commit-before-wake、late add仍属于closed epoch、新 epoch fresh handle、旧/new epoch并存、TCP/UDP poll后I/O、多 waiter/overflow、handle reuse、listener accept/hidden cleanup、deferred retirement与 closure/drop/reaper交错。
- GREEN condition: 旧 handle永久返回正确 terminal，新 epoch handle正常 I/O；同一 terminal在 readiness/`SO_ERROR`/I/O一致；所有 public/hidden/raw owner恰好关闭或回收一次，existing readiness/listener/deferred tests不退化。
- Verification: axnet focused model/source guards；ordinary与qemu-diagnostics完整串行 `--test-threads=1`；manifest rustfmt、`git diff --check`、full diff Review与 strict OpenSpec validation，全部exit 0。
- Stop when: public handle或 listener/deferred raw owner无法持久保存创建 epoch；epoch closure需要反转既有锁序、跨 guard wake、无界遍历或重建 `SocketSet`；稳定错误映射必须改变外部 `DevError` 或冻结的 V1–V3 ABI。返回 Plan 重审 contract。

**Invariants**

- SocketEpoch、QueueEpoch、LinkGeneration与wake generation是独立 identity；socket closure不得推进或重解释 QueueEpoch。
- terminal identity first-wins且不可清除；只通过新 SocketEpoch为新 handle提供 fresh state。
- 关闭 epoch时先提交 terminal，再 snapshot，最后在所有 guard外 wake；waiter醒后立即 I/O观察相同错误。
- public handle、listener session、hidden socket与 deferred raw owner按创建 epoch归属；numeric handle reuse不得继承旧 identity。
- recovery/link gate决定能否创建或使用新 session；link up不复活旧 epoch owner。
- listener/deferred cleanup保持 bounded、exactly-once，且不改变 `SERVICE → SOCKET_SET → ListenTable entry`锁序。

**Non-goals**

- 不新增 QEMU V4 ABI、recovery probe/validator或执行真实 QEMU/HMP资格。
- 不重建全局 `SocketSet`，不透明迁移旧 TCP连接、listener或 deferred owner。
- 不改变 driver QueueEpoch/completion语义，不证明 SMP、PCI/DWMAC runtime、真板或性能。

**Acceptance**

- A1（R4/D5）：`NetworkTerminal` 稳定映射 reset/old epoch→`ConnectionReset`、link down→`NotConnected`、deadline→`TimedOut`、诊断取消→`Interrupted`、ownership invariant→`BadState`、其他 device I/O→`Io`；既有 local connect-refused保持正确。
- A2（R7/D7）：每个 public bridge/handle记录创建 SocketEpoch；关闭 epoch只终结该 epoch，commit-before-snapshot-before-wake且 first-wins。
- A3（R7）：旧 handle在 readiness、`SO_ERROR`和每次 TCP/UDP I/O retry中永久返回同一 terminal；多 waiter与 poll 后 I/O无 lost wakeup或 `WouldBlock`漂移。
- A4（R7）：新 SocketEpoch开放后新建 socket正常使用且不继承旧 terminal；旧/new handle并存与 numeric handle reuse不发生 identity污染。
- A5（R7）：listener public session、hidden accept socket与 deferred raw owner保留来源 epoch；closure、drop、accept和 bounded reaper交错下恰好清理一次。
- A6（兼容）：existing readiness overflow/fan-out、TCP/UDP/listener/deferred tests、lock-order source guards及 axnet ordinary/qemu-diagnostics全量不退化。

**Verification**

1. `NetworkTerminal` encode/decode/AxError mapping focused tests，覆盖每个 D5类别与 local connect-refused兼容。
2. wrapper epoch registry focused tests：close/open、late add、old/new并存、commit-before-wake、multiwaiter/overflow和 handle reuse。
3. TCP/UDP model tests：terminal-before-register、event-before/during-register、poll报告 ERR 后立即 I/O、旧 epoch永久错误与新 epoch普通 I/O。
4. listener/deferred tests：hidden handle/session epoch、accept/drop/closure/reaper交错、exactly-once与 bounded convergence；现有 lock-order/source guards。
5. `env RUSTFLAGS="-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh" cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --test-threads=1`。
6. 同上增加 `--features qemu-diagnostics`。
7. `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check`、`git diff --check`、完整 diff Review、`openspec validate ms07-qemu-single-hart-recovery-semantics --strict`。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Investigation | PASS | boot-global terminal、TCP/UDP effective lookup、runner publication与 listener/deferred ownership边界已定位。 |
| Design | PASS | D5/D7固定结构化映射、epoch binding、commit-before-wake与 fresh epoch语义。 |
| Iteration Plan | PASS | Task 3.2独立形成 socket terminal baseline；QEMU ABI/runtime留给 Iteration 006/007。 |
| Cycle Scope | PASS | 只展开3.2，不前移4.1/4.2或真实 QEMU资格。 |
| Task Contracts | PASS | handle/session/raw owner、映射、锁序、测试见证与 stop条件足以建立 RED。 |
| Traceability | PASS | R4/R7、D1/D5/D7、Task 3.2、代码面与 A1–A6闭合映射。 |
| Verification | PASS | mapping、registry、TCP/UDP、listener/deferred与两个 axnet全量覆盖本轮边界。 |

技术 Gate 2 检查通过；用户已于 2026-08-30 明确批准本计划并要求「更改gate状态，开始实施」，Plan Context 由 `draft` 改为 `ready`，进入 Act。

**Persisted Evidence**

- Mode: none

命令与决定性输出可低成本重跑，Act Response足以保存 Gate结果。

- Budget: 本 Cycle 最多 5 个文件（含 README），整个 change最多 20 个 Evidence文件；当前不创建 Evidence。

**Risks and Notes**

- epoch terminal registry若只按 numeric handle索引，会在 smoltcp handle复用时污染新 owner；identity必须随 bridge/session/raw owner持久保存。
- 同一 recovery fault可能从 lifecycle、stack outcome或 link closure到达；必须明确单一 first-wins epoch commit，避免重复 cleanup/wake。
- listener hidden handle与 deferred raw handle没有 public bridge，仍必须通过 session/owner epoch参加 closure和 exactly-once清理。
- epoch closure不得为唤醒方便在 registry guard内调用 waiter；继续使用 snapshot 后 guard外 wake。

## Act Response

- Status: reported

当前 Cycle 已完成原 Task 3.2 及最新 Plan Review 要求的有限修复；Plan Context、Iteration Map 与 Plan Review 保持不变。

**Implemented**

1. `NetworkTerminal` 建立有界、稳定的应用错误映射：reset、link down、deadline、取消、ownership fault 和普通 device I/O 分别映射为 `ConnectionReset`、`NotConnected`、`TimedOut`、`Interrupted`、`BadState` 和 `Io`；既有 local `ConnectionRefused` 保持不变。
2. `SocketSetWrapper` 以 current/open epoch 管理 public/adopted bridge，关闭时按 epoch first-wins 提交 terminal、snapshot matching bridges，再在所有 registry guard 外 wake；旧 epoch terminal 保留在 bridge，late add/adoption 不会污染新 epoch。
3. TCP/UDP public handle 保存来源 epoch；readiness、`SO_ERROR` 和每次 I/O retry 共用 bridge 的 epoch terminal。新 epoch 可创建 fresh handle，旧 handle 不会复活或继承新状态。
4. listener hidden session 与 accepted handle 保存来源 epoch；bounded reconcile 可按关闭 epoch回收 hidden raw handles。deferred TCP/UDP owner 保存 Drop 时的 epoch，仍由 resident reaper exactly once 处理。
5. link、resident recovery、fatal/recovery fault 和 stack-runner publication 均接入 epoch closure；Service 与 listener marker 保持既有锁序，bridge wake 继续在 guard 外执行。

**Audit Repairs**

1. 配对 Service 与 Registry 现在共享 Registry 的 checked SocketEpoch identity：down 只关闭当前 epoch，成功 up/recovery 只通过 Registry 推进一次；未配对 test Service 保留原兼容 seam，`QueueEpoch` 不随 flap 改变。
2. TCP/UDP `SO_ERROR` 在读取时直接刷新 effective terminal，再由 `GeneralOptions` 返回缓存值；读取不消费状态，重复读取保持同一错误。
3. TCP `bind`/`listen` 与 UDP `bind` 入口执行 terminal-first；`ListenTable` 保存关闭 epoch marker 与 terminal，在 late old-epoch listener 插入前拒绝，并让 bounded reconcile 回收已存在的旧 session。
4. 删除按关闭 epoch增长的无界 `terminals` 表，改为一个 `last_closed` 快照；accept adoption 在 SocketSet 临界区结束前登记 bounded `pending` owner，closure 将 terminal 写入该 owner，adoption/remove 后立即移除。
5. 统一 `SocketSetWrapper` 相关路径为 `inner -> epoch_state -> readiness` 的无环顺序：`add_public`、`remove_raw` 与 accept pending registration 不形成反向等待；新增 source/order guard，并验证 pending owner 在 adoption/remove 后归零。
6. 新增显式 `publish_socket_epoch_fault_code(epoch, code)`；stack round、fatal、recovery fault 与 drift quarantine 将 captured epoch 一直带到 registry commit，listener marker、matching snapshot 与 wake 不再因重读 current 而分裂。
7. `remove_raw` 现在持有 raw `SocketSet` guard，直到对应 pending owner metadata 删除完成后才释放 numeric slot；新增 source-order 与 handle-reuse 模型，关闭旧 remover 删除复用 handle 新 metadata 的 ABA 窗口。
8. 新增 `SocketEpochTerminalCommit` 与 Service 配对提交路径：registry 先产生唯一 first-wins terminal，listener marker 使用同一 winner，只有 commit winner 在 Service guard 释放后唤醒 matching bridges。stack、link、recoverable reset、fatal、drift 与 recovery fault 共用该顺序。
9. A6 publication source guard 改为沿 `publish -> commit helper -> wake helper` 调用链检查：允许 epoch-scoped `commit_network_terminal`，继续禁止 socket-local `commit_terminal`，并明确验证 commit 先于 guard 外 `wake_for_global_publication`。
10. `Service::stack_round` 在产生 terminal 的同一 Service 临界区中将 `fault_epoch` 写入 outcome；无 fault 的 round 保持 `None`。`StackAccess::publish_terminal` 只接收 outcome 携带的 epoch，不再于发布时读取 current epoch；E 的迟到 fault 在 E+1 打开后稳定 no-op。

**Changed Files and Symbols**

- `crates/axnet/src/readiness.rs`：`NetworkTerminal`、稳定 terminal codes、`ReadinessBridge::effective_terminal_code` 与 epoch terminal commit。
- `crates/axnet/src/wrapper.rs`：`SocketEpochState`、per-registration epoch、close/open/late-adoption、snapshot/wake 与 registry tests。
- `crates/axnet/src/tcp.rs`、`crates/axnet/src/udp.rs`：public handle epoch、epoch-aware adoption/drop、poll 后 I/O terminal tests；TCP accept 在持有 `inner` 时登记 pending owner。
- `crates/axnet/src/listen_table.rs`：listener session epoch、`accept_with_epoch`、closed-epoch bounded cleanup 与 late old-epoch listener rejection 见证。
- `crates/axnet/src/service.rs`：deferred owner epoch、Service/registry pairing、link close/open、hidden listener closure marker，以及 stack round 的 fault-origin epoch outcome。
- `crates/axnet/src/stack_runner.rs`：传递 outcome 的显式 fault epoch，完成 paired registry terminal publication 与 guard 外 wake；更新 ownership/source guards。
- `crates/axnet/src/async_rx.rs`：recovery 成功后的 fresh epoch、fault closure 的 listener marker 与 link-down wake handoff。

**Deviations from Plan**

无 Acceptance 偏差。最新 repair items 均受原 Task 3.2 A2、A4–A6 契约约束，没有新增设计、后继 Cycle 或范围外产品能力。stack late-fault 模型首次编译因测试夹具缺少 `RxOwnerView` import 而失败；补入 import 后 exact test 通过，未因此改动产品语义。先前 OS-thread 夹具的握手问题仍按已记录方式处理，不计为产品结果。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Spec review：PASS。A1–A6 与最新 A2/A4 gaps 全部闭合：raw slot/pending metadata 是同一 owner 临界区；registry winner 同时决定 bridge、late listener 与 hidden cleanup 错误；stack fault 在 round 内捕获目标 epoch，旧 epoch publisher 不重定向 E+1；wake 均在 Service/registry guard 外。
- Code review：PASS。完整 diff 未发现未解决的 Critical/Important。保持 `Service -> inner -> epoch_state -> readiness/ListenTable` 的既有无环方向；loser 不预写或覆盖 marker；pending/late-adoption 容器仍有界；没有新增 executor、轮询、ABI 或 warning 来源。
- RED/GREEN：新 fault-origin source guard 修复前 `0 passed; 1 failed`、exit 101，修复后 1/1 GREEN；E 产生 fault、E 关闭、E+1 打开、再发布旧 outcome 的确定性模型 1/1 GREEN，Service fault/no-fault outcome 3/3 GREEN。既有 ABA、winner-before-marker 与 A6 regressions 仍全绿。
- Warning review：本 Cycle 未增加 axnet warning；生产 check 仍报告既有 smoltcp、diagnostic/test-only dead-code/import warnings，不影响构建。

**Verification Evidence**

| 验证项 | 决定性输出 | 结论 |
|---|---|---|
| Gate 3 RED — raw ABA | exact source-order test：`FAILED`，`0 passed; 1 failed`，exit 101 | PASS（预期 RED） |
| Gate 3 RED — terminal winner | exact publisher source test：`FAILED`，`0 passed; 1 failed`，exit 101 | PASS（预期 RED） |
| Gate 3 RED — stack fault origin | exact source guard：`FAILED`，`0 passed; 1 failed`，exit 101 | PASS（预期 RED） |
| Repair focused GREEN | raw removal `2 passed`；winner order model、publisher source、captured epoch、paired flap、recovery guard/wake 各 `1 passed; 0 failed` | PASS |
| Stack fault focused GREEN | source guard `1 passed`；late E outcome/E+1 fresh 模型 `1 passed`；Service round outcome `3 passed`，均 `0 failed` | PASS |
| 原 A6 失败 exact | `source_global_publication_never_touches_socket_local_commit ... ok`；`1 passed; 0 failed` | PASS |
| axnet ordinary aggregate | `467 passed; 0 failed`，`--test-threads=1`，exit 0 | PASS |
| axnet qemu-diagnostics aggregate | `491 passed; 0 failed`，`--test-threads=1`，exit 0 | PASS |
| production check | ordinary 与 qemu-diagnostics `cargo check --lib` 均 exit 0 | PASS |
| rustfmt | `cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check` exit 0 | PASS |
| whitespace | `git diff --check` exit 0 | PASS |
| OpenSpec | `openspec validate ms07-qemu-single-hart-recovery-semantics --strict`：`Change ... is valid` | PASS |

**Persisted Evidence**

- Mode: none；按本 Cycle 预算不创建 Evidence 文件。

**Experience Candidates**

None.

**Current Path**

- 最新 fault-origin epoch handoff 修复、任务级 Gate 4/5、完整 diff Review 与全量 Gate 均完成；Act Response 为 `reported`，Plan Review 保持 `pending` 等待独立审计。未归档 change，未同步全局文档，未清理分支。

## Plan Review

- Review Result: accepted

**Findings**

None.

**Deviation Classification**

None.

**Acceptance Gaps**

None. A1–A6 均满足。

**Convergence**

closed。上一版剩余的 fault-origin epoch handoff 已闭合；没有未解决的 Critical、
Important 或 Minor finding。

**Evidence**

- `Service::stack_round` 在产生 terminal 的同一 `Service` 临界区把
  `self.socket_epoch` 写入 `StackRoundOutcome::fault_epoch`；quiet round 为 `None`。
- `StackAccess::publish_terminal(epoch, code)` 不再读取
  `current_socket_epoch()`，并继续通过配对 Service 完成 registry first-wins commit、
  listener marker 和 guard 外 wake。
- 确定性交错模型 `late_stack_fault_uses_round_epoch_and_leaves_new_epoch_fresh`：E 中
  生成 fault，先以 `LinkDown` 关闭 E 并打开 E+1，再发布旧 outcome；E 保留 winner，
  E+1 bridge、wake 与 listener 均保持 fresh。source guard 与 quiet-round 模型也通过。
- 新鲜聚焦验证 3/3 通过；ordinary 全量 `467 passed; 0 failed`，qemu-diagnostics
  全量 `491 passed; 0 failed`，两个命令均 exit 0。
- ordinary 与 qemu-diagnostics `cargo check --lib`、manifest rustfmt、
  `git diff --check` 和 strict OpenSpec validation 均 exit 0。warning 为既有 smoltcp、
  diagnostic/test-only dead code/import，不构成 Acceptance gap。
- QEMU/HMP runtime 未执行：它属于 Iteration 006–007 的明确延后边界，不影响本
  Iteration 的 host/model Acceptance。Persisted Evidence 为 `none`；Blocker Handoff
  与 Blocker Resolution 均为 `None`。

**Follow-up Decision**

接受当前 Cycle 与 Iteration 005。Task 3.2 的 epoch-scoped terminal、稳定应用错误、
first-wins publication、late ownership、bounded cleanup 与 lock/wake 约束均有实现和新鲜
验证支撑；下一步只展开 Iteration 006 的 Task 4.1 草案，不启动实现。

**Iteration Plan Update**

None.

**Next Cycle**

None.

**Next Iteration**

`../006-recovery-probe-and-validator/000-initial.md`
