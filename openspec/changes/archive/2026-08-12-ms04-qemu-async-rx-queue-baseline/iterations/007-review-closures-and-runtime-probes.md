# Iteration 007: Review Closures and Runtime Probes

## Plan Context

- Status: awaiting-gate-2
- Round: 007
- Parent: `006-review-closures-and-isr-observability.md`

**Objective**

关闭 iteration 006 的 fault telemetry、关联 snapshot、IRQ witness 和无长度 ioctl ABI
缺口，再完成原定 T6.2：交付固定 V2 snapshot、独立 software nudge、MS04 guest probe、
host UDP RX burst stimulus 与自动构建入口。本轮只构建和 host-test 运行时工具，不启动
QEMU；实际 guest/host 交互和 Evidence 继续留在最终 user-only iteration。

**Background**

Iteration 006 已接通 raw cause→known ACK→fixed publish、register-before-start、唯一 RX
task 和首版 telemetry。Fresh Review 证明 101 个 axnet tests、100×并发、47 个 host tests、
MS16 host suite、kernel check 与 QEMU build 均通过，但发现 active fault 被计数两次、
missing Service/IRQ-enabled entry 不可诊断、关联 snapshot 字段可能撕裂。更严重的是旧
`0x4e49_4431` ioctl 没有长度参数，却从 8 fields 原地扩大到 26 fields；MS16 仍有两个
8-field buffer，已编译的旧 payload 也无法随源码更新。这不是 append-only ABI，必须在
probe 开发前改为固定 V1 + 新 V2。

**Current Baseline**

- Branch/HEAD: `net-k3` / `79ea1f9da7425a388710f7d617eb5d01948c057d`；iteration
  006 产品和 OpenSpec 改动已 staged，Act 必须保留无关内容。
- `service_round` 在具体错误点记录 fault，`poll_active` 又在 common fault branch 统一
  记录 RECEIVE_RECYCLE；ARM 路径只记录一次。
- `rx_snapshot_impl` 两次加载 lifecycle；last-error stage/code 用两个独立 Relaxed
  atomics。`IrqTelemetry` 没有 IRQ-enabled-on-entry counter。
- kernel `NET_IRQ_SNAPSHOT=0x4e49_4431` 写 26×`u64`；更新后的 MS03 probe 匹配 26，
  但 MS16 adapter 的 local struct/dummy 仍为 8，旧 MS03 binary contract 也固定为 8。
- `publish_rx_event` 同时增加 generation 和 ISR counters，不能作为 T6.2 software nudge。
- 没有 `ms04_rx_probe.c`、MS04 stimulus、V2/nudge ioctl 或对应 Makefile targets。
- Fresh baseline：axnet 101、100×stress、host 47、MS16 host tests、kernel check、QEMU
  release ELF/bin、fmt、OpenSpec strict 和 diff checks PASS。axnet test build 有 3 个新增
  test-only warnings。`make build` 最终 exit 0；工具安装探测的只读/网络噪声不是最终
  `ENV-BLOCKED`。

**Current-State Evidence**

| Boundary | Evidence |
|---|---|
| fault path | `service_round::{SUPPRESS,COMPLETION_QUERY,RxOutcome::Fault}` and `poll_active::RoundOutcome::Fault` both call `record_fault` |
| snapshot state | `rx_snapshot_impl` derives owner and lifecycle from separate loads; last error is two atomics |
| old ioctl writer | `sys_ioctl` writes full `virtio_net_irq_logic::IrqSnapshot` for `0x4e49_4431` |
| old consumers | `ms03_irq_probe.c` historical wire ABI and `network_benchmark_platform.c` local raw/dummy are 8 fields |
| IRQ lifecycle | QEMU platform source executes handler before `plic.complete`; current handler does not record enabled entry |
| guard weakness | init guard accepts any earlier `return;`; handler guard does not bind `TELEMETRY.record(status)` argument |
| stimulus topology | QEMU user-net exposes guest host as `10.0.2.2`; MS03 already uses host service port 15555 |
| bounded buffers | Router has 64 slots and RX task budget is 32; a bounded UDP burst can exercise multiple rounds |

**Critical Path**

```text
legacy caller -- ioctl 0x4e49_4431 --> fixed SnapshotV1 (8 u64 only)
MS04 probe   -- ioctl 0x4e49_4432 --> fixed SnapshotV2 (MS03 prefix + MS04 fields)
MS04 probe   -- ioctl 0x4e49_4e31 --> axnet software wake only
                                               -> software_nudge += 1
                                               -> sole AtomicWaker wake
                                               -> no generation/isr counter/completion change

guest probe UDP REGISTER -> host READY
  -> guest takes PRE, sends START
  -> host sends bounded sequenced burst without console control
  -> guest receives/verifies burst, waits for stable quiet snapshot
  -> POST/DELTA/gauge validation -> PASS or FAIL marker
```

**Implementation Guidance**

1. 先收敛 fault owner：每个 active terminal error 只在一个位置提交 fault+stage+code，
   lifecycle 再转 Faulted。增加 suppress、completion query、receive/recycle 三个直接 poll
   tests，逐项断言 `fault_delta=1` 且 stage 不被 common branch 覆盖。missing Service 记录
   PREFLIGHT/BadState，但仍进入 Unavailable、保持 polling owner。
2. snapshot 先 Acquire-load lifecycle 一次，再由该值同时派生 lifecycle/owner。last error
   用一个 atomic packed value或等价单发布机制保存 stage+code，snapshot 再拆成两个 ABI
   fields；不能用锁或让 telemetry 参与调度。删除新增 tests 的无效 mut/变量 warning。
3. IRQ handler 在 publish 前若 `irqs_enabled()==true`，增加独立 `irq_enabled_entry`；
   false→true 仍单独增加 restore violation。host pure tests 覆盖四种 before/after 组合，
   source guard 精确匹配 `TELEMETRY.record(status)`，并在 `if !register` block 内验证 return。
4. ABI 改为两个独立 wire struct。`0x4e49_4431` 只写 V1 8 fields，恢复 MS03 probe 和
   MS16 adapter 的旧尺寸；`0x4e49_4432` 写固定 V2，包含 V1 prefix、现有 MS04 fields、
   `irq_enabled_entry` 和 `software_nudge`。V1/V2 各自有 Rust size/offset、C
   `_Static_assert` 和 all-consumer source guard。不得用 type alias 让 V1 随 V2 增长。
5. axnet 增加固定 software-nudge entry：只增加 software-nudge counter 并调用 sole
   waker，不增 generation、isr_publish/isr_wake。`0x4e49_4e31` 只在 QEMU kernel ioctl
   调用该 entry；local notify/telemetry unit test 证明精确 delta 和无 completion side
   effect。probe 在 lifecycle=Active/owner=AsyncOwned 前置不满足时必须 FAIL。
6. 新 `tests/ms04_rx_probe.c` 提供 `snapshot|idle|nudge|burst`。所有模式输出
   `MS04 PRE`、`MS04 POST`、`MS04 DELTA`、`MS04 PASS|FAIL mode=...`；quiet window 内不
   打印。只对 monotonic counters 求差；lifecycle、owner、last-error stage/code 输出 POST
   gauge，禁止无符号相减。
7. idle 在有界 quiet window 内要求 RX progress/budget/yield/fault delta 为零，task poll/
   empty-check 不超过一次；nudge 要求 software_nudge 精确 +1、task/empty-check 各推进
   一次、ISR/generation/reap/refill/self-yield 不增加。任何 fault、restore violation 或
   IRQ-enabled entry 非零都 FAIL。
8. burst 使用 UDP 两阶段握手：guest REGISTER，host READY；guest 取 PRE 后发 START；host
   立即发送固定数量、固定 payload 上限、带 sequence 的 datagrams。probe 验证完整序列，
   再取得稳定 POST；要求 task/ISR/reap/refill 推进、`reaped_delta==refilled_delta`、
   budget_exhausted/self_yield 都大于零、fault/restore/IRQ-entry 为零。控制 datagram 不
   要求精确 ISR 数量。
9. `scripts/ms04_rx_stimulus.py` 只监听默认 UDP 15556、执行握手和发送流量，不打开 QEMU
   console、不输入 shell。参数有上界；`--self-test` 在 host loopback 验证协议、序列、
   count、payload 和 malformed registration，不依赖 QEMU 或网络下载。
10. Makefile 增加 host syntax/stimulus target 和 `tests/ms04_rx_probe` 静态构建 target；
    `host-test` 纳入不依赖 cross toolchain 的部分。Act 只构建产物，不复制 rootfs、不启动
    QEMU、不创建 Evidence。

**Behavioral Change**

- 旧 MS03/MS16 snapshot binary 继续安全读取固定 64-byte V1；MS04 从 V2 读取扩展字段。
- active fatal 只计一次并保留真实 stage/code；missing Service 和异常 IRQ entry 可诊断。
- lifecycle/owner 与 last-error pair 在单次 snapshot 内保持关联一致。
- software nudge 与硬件 ISR event 分开计数，不改变 generation，也不伪造 RX progress。
- 新 probe/stimulus 可在最终手测时产生机器可判定 marker，但本轮不声称任何 QEMU runtime
  结果。

**Change Surface**

| Task | Requirement/Scenario | File/Symbol | Current Responsibility | Planned Change |
|---|---|---|---|---|
| T6.1R-a | R2,R3,R6 / fault、snapshot、IRQ entry | `axnet::async_rx`; `virtio_net_irq{,_logic}` | first telemetry implementation | once-only fault、coherent pairs、entry counter、strong guards |
| T6.1R-b | compatibility / V1 and V2 | `ctl.rs`; IRQ logic/glue；MS03/MS16 consumers/harness | one enlarged lengthless ioctl | immutable 8-field V1 + fixed MS04 V2 |
| T6.2 | R3,R6,R7 / idle、nudge、burst、fairness | axnet nudge；kernel ioctl；`ms04_rx_probe.c`; stimulus；Makefile | no runtime tooling | separate nudge + deterministic guest/host tools and build gates |

**Task Contracts**

T6.1R-a — Telemetry and permanent witness corrections:

- RED: focused full-Future tests observe suppress/query/receive faults as count 2 or wrong stage；
  missing Service leaves error NONE；snapshot interleaving model permits mismatched lifecycle/owner
  and last-error pairs；before=true is unreported；guard mutations still pass.
- GREEN: every terminal source yields exactly one fault and its original stable stage/code；missing
  Service yields Unavailable + PREFLIGHT/BadState；one lifecycle load drives both fields；one atomic
  publication drives error pair；enabled-entry and restore violation are distinct counters；both
  source mutations fail the guard.
- Preserve: lifecycle ownership、Relaxed observation-only counters、no Service lock in snapshot、
  ACK/publish/EOI ordering and single waker.
- Stop: fix needs telemetry lock in ISR/task, changes owner transitions, merges enabled-entry with
  false→true restore violation, or cannot form mutation-sensitive tests.

T6.1R-b — Memory-safe versioned snapshot ABI:

- Depends on: T6.1R-a GREEN.
- RED: current V1 size is 26×8 while historical/current consumers allocate 8×8；an old-buffer
  canary test or source/type assertion fails.
- GREEN: V1 command/type/write size is exactly 8×8 and first-field offsets match MS03；V2 has an
  independently fixed size/order matching only the MS04 probe；MS03 probe and MS16 adapter remain
  V1；consumer inventory finds no undersized buffer for either command. Old 8-field canaries remain
  unchanged after the V1 write model.
- Preserve: existing command number、first eight values and MS03 marker semantics。V2 is
  `0x4e49_4432`; nudge is not encoded as a snapshot read.
- Stop: requires rebuilding old binaries for safety, changes V1 write size, aliases V1 to growing
  V2, or lacks a complete command→type→consumer witness.

T6.2 — Software nudge, probe, stimulus and build entries:

- Depends on: T6.1R-b GREEN.
- RED: no nudge command/source distinction、V2 consumer、mode markers、host protocol self-test or
  static build target exists.
- GREEN: local nudge test proves software counter +1 and generation/ISR counters unchanged；probe
  parser/decision host tests cover pass/fail, counter wrap rejection, gauge handling, partial
  telemetry and stable-snapshot timeout；stimulus self-test covers handshake/sequence/bounds；C
  syntax and RISC-V static build pass.
- Runtime contracts: snapshot requires Active/AsyncOwned；idle and nudge use bounded deadlines；
  burst validates every sequence and exact offered count before telemetry PASS。Counter regression,
  timeout, malformed packet, missing field, partial receive or nonzero fault/violation returns
  nonzero and emits FAIL.
- Preserve: host stimulus never controls console；probe emits nothing inside measurement window；
  no counter reset、ISR print、fake completion、second executor/task or QEMU launch automation.
- Stop: budget/yield can only be claimed from sent traffic rather than guest telemetry, nudge must
  call ISR publisher, probe depends on interactive parsing, or self-test requires QEMU/network.

**Invariants**

- ISR remains status/ACK/telemetry/fixed wake only；descriptor service remains task-context only。
- V1/V2/nudge command numbers have fixed non-overlapping semantics and fixed write sizes。
- lifecycle is monotonic；fault does not restore polling owner；single RX task/waker remains。
- telemetry does not control scheduling；counter snapshots may be individually non-atomic, so probe
  uses bounded stable reads before evaluating conservation。
- nudge wakes only；it does not increment event generation、ISR counters or RX completion counters。
- host stimulus sends packets only；manual QEMU interaction remains a user boundary。

**Non-goals**

- Running QEMU、editing rootfs、typing guest commands or collecting final Evidence。
- T7 full automatic Gate/diff/Evidence closure；D1 baseline repair；MS01/MS02/MS03 runtime reruns。
- Async TX、MS05 packet slots、stack runner、socket readiness、reset、SMP、PCI/DWMAC or real board。
- A general variable-length ioctl framework or compatibility changes outside these three commands。

**Acceptance**

| Requirement/Scenario | Design | Task | Code/Test Witness | Simplification | Status |
|---|---|---|---|---|---|
| exact active fault observation | D9 | T6.1R-a | suppress/query/receive/arm full-Future deltas | None | Covered |
| coherent lifecycle/error gauges | D4,D9 | T6.1R-a | injected interleaving/pair tests | None | Covered |
| IRQ restore diagnostics | D3,D9 | T6.1R-a | four-state decision + handler guard | None | Covered |
| legacy snapshot safety | D9,D10 | T6.1R-b | V1 canary/size/consumer inventory | None | Covered |
| MS04 expanded snapshot | D9 | T6.1R-b,T6.2 | V2 Rust/C layout and ioctl mapping | None | Covered |
| bounded software nudge | D5,D9 | T6.2 | local wake/generation/counter test + probe mode | None | Covered |
| idle/no busy loop | D7,D9 | T6.2 | host decision tests; final QEMU deferred | None | Covered |
| burst/fairness/conservation | D7,D9 | T6.2 | protocol self-test + guest decision tests; runtime deferred | None | Covered |
| manual boundary | D10 | T6.2 | no QEMU/rootfs/Evidence mutation | None | Covered |

No requirement is Missing or Simplified. QEMU observations remain mapped to final manual iteration;
this round only makes their tooling deterministic and buildable.

**Verification**

```text
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
for review_iter in $(seq 1 100); do cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet -- --test-threads=16; done
make host-test
make network-benchmark-test
python3 scripts/ms04_rx_stimulus.py --self-test
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c tests/ms04_rx_probe.c
make tests/ms03_irq_probe tests/ms04_rx_probe
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib queue::tests
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
rustfmt --edition 2024 --check kernel/src/drivers/virtio_net_irq.rs kernel/src/drivers/virtio_net_irq_logic.rs kernel/src/syscall/fs/ctl.rs tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs
cargo check --offline -p starry-kernel --features qemu
make LOG=info build
openspec validate ms04-qemu-async-rx-queue-baseline --strict
git diff --check
```

Act Response 必须记录：各 fault source 的 exact count/stage/code；missing-Service 状态；
lifecycle/owner 和 error-pair 一致性；四种 IRQ before/after 结果；V1/V2 command、size、offset、
consumer inventory/canary；nudge generation/ISR/software deltas；probe 每个 mode 的 host decision
tests；UDP self-test 包数/sequence/bounds；新增 Make targets、static artifact size/hash；所有
命令退出码与 warning 分类。不得记录 QEMU runtime PASS 或创建最终 Evidence。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R2/R3/R4/R6/R7 + iteration 006 Review closures |
| Investigation | PASS | fault call chain、all ioctl consumers、QEMU EOI、nudge semantics、user-net topology inspected |
| Design | PASS | once-only fault、coherent pairs、fixed V1/new V2、separate nudge and UDP protocol fixed |
| Task Contracts | PASS | T6.1R-a→T6.1R-b→T6.2 each has RED/GREEN/preserve/stop |
| Traceability | PASS | scoped RTM has no Missing/Simplified row |
| Verification | PASS | unit/source/ABI/protocol/static build plus upstream regressions listed |
| Manual boundary | PASS | QEMU/rootfs/Evidence remain final user-only work |
| Persisted Evidence | PASS | mode none; deterministic outputs fit Act Response |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: none

本轮只产生确定性 unit/source/fmt/compile/build 和 host protocol self-test 结果，由 Act
Response 记录；不创建 `evidence/` 目录。

**Risks and Notes**

- UDP burst 在 host self-test 中只能证明协议和序列，不能证明 QEMU 会形成 budget
  exhaustion；最终 guest telemetry 才是 runtime 判据。
- 单次 counter snapshot 不提供跨字段事务一致性；probe 必须在 quiet deadline 内取得
  progress counters 稳定的连续读数，否则 FAIL/timeout，不用不稳定值判断守恒。
- QEMU result 只适用于单 hart VirtIO-MMIO；本轮不产生该类证据。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-12
- User instruction: “批准”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS，iteration 007 可由后续
  明确的 `openspec-act` 请求执行。
- Scope: 批准范围包括 T6.1R-a 的 fault、snapshot pair 和 IRQ witness 修复，T6.1R-b 的
  固定 8-field V1 与独立 V2 snapshot ABI，以及 T6.2 的独立 software nudge、MS04 guest
  probe、host UDP stimulus 和自动构建入口。批准不启动 Act，也不授权 QEMU runtime、
  rootfs 修改、sandbox 外复跑、最终 Evidence、Maintainer、Recorder 或归档工作。

## Act Response

- Status: implemented-with-env-deferred-check
- Completed: 2026-08-12
- Scope: T6.1R-a、T6.1R-b、T6.2

### Implementation

- Fault telemetry now records active suppress `(fault +1, stage 2, code 5)`, completion-query
  `(fault +1, stage 3, code 8)`, and receive/recycle `(fault +1, stage 4, code 5)` exactly once.
  Missing Service records `(stage 1, code 3)` without incrementing `fault`, publishes
  `Unavailable`, and keeps polling ownership.
- RX snapshots derive lifecycle and owner from one lifecycle load. Last-error stage/code use one
  packed `AtomicU64` publication and one snapshot load.
- IRQ diagnostics distinguish all four before/after states. Enabled-on-entry and false-to-true
  restore violations have separate counters; source guards require `TELEMETRY.record(status)` and
  a return inside the registration-failure branch.
- `0x4e49_4431` now writes the fixed 8-field/64-byte V1. Independent command `0x4e49_4432`
  writes the 28-field/224-byte V2. Rust size/offset tests, a V1 adjacent-canary test, and the
  MS03/MS16/MS04 consumer inventory all pass.
- `0x4e49_4e31` performs a software-only nudge: local tests observe waker `+1`, generation
  unchanged, ISR publish/wake `+0`, and software-nudge `+1`.
- Added the four-mode guest probe, bounded two-phase UDP stimulus, strict host decision tests, and
  Make targets. Probe counters reject regression; lifecycle/owner/error remain POST gauges; stable
  reads have a fixed deadline; all execution paths emit a PASS/FAIL marker.
- The 16-thread stress run exposed three older `Service::poll` tests that touched global
  `RX_NOTIFY` outside the existing serial guard. They now share that guard; the repeated stress
  gate is green.

### Gate 4 self-review

| Task | Spec compliance | Code quality | Result |
|---|---|---|---|
| T6.1R-a | Exact fault source, coherent pairs, missing-Service state, and IRQ witness match the approved contract. | Duplicate recording removed; packed-pair and single-load guards are deterministic. | PASS |
| T6.1R-b | V1 remains exactly 64 bytes and V2 is an independent 224-byte wire type with a duplicate prefix. | Typed writes, complete offsets, canary, and consumer inventory prevent accidental growth. | PASS |
| T6.2 | Nudge is separate from hardware publication; probe and stimulus obey bounded/manual boundaries. | Early FAIL paths, timeout arithmetic, protocol bounds, and test isolation were reviewed and corrected. | PASS |

Unresolved product findings: none.

### Verification

| Command or gate | Result |
|---|---|
| axnet full unit suite | PASS, exit 0, 109/109 |
| axnet 100× `--test-threads=16` stress | PASS, exit 0, 100/100 after test-isolation correction |
| `make host-test` | PASS, exit 0: 6 early-console, 8 memtrack, 26 MS03, 12 MS04, 6 probe decisions, stimulus self-test |
| `make network-benchmark-test` | PASS, exit 0: 26 protocol, 20 platform, 21 tool, 9 integration tests |
| stimulus `--self-test` | PASS, exit 0: 96 packets, exact sequence/payload, bounds and malformed controls |
| strict C11 syntax | PASS, exit 0 for MS03 and MS04 probes |
| UART / axdriver-net / virtio queue regressions | PASS, exit 0: 62+18 doctests, 4, and 15 tests |
| axnet fmt and directed rustfmt | PASS, exit 0 |
| `cargo check --offline -p starry-kernel --features qemu` | PASS, exit 0 |
| `make LOG=info build` | PASS, exit 0; attempted cargo-binutils installation emitted read-only/network noise but the cached build completed |
| `openspec validate ms04-qemu-async-rx-queue-baseline --strict` | PASS, exit 0; change valid, 17/23 tasks |
| `git diff --check` | PASS, exit 0 |

Existing warnings are limited to the prior cargo-config deprecation, one `virtio-drivers`
lifetime warning, one `axdriver_virtio` lifetime warning during the full build, and 11 existing
smoltcp warnings. This iteration introduced no remaining Rust warning.

### Environment-deferred checks

- `make tests/ms03_irq_probe tests/ms04_rx_probe` stopped at the first
  `riscv64-linux-musl-gcc` invocation with `Bad system call` (SIGSYS), exit 2. Per R44 this is
  `ENV-BLOCKED`, not a product diagnostic.
- The tracked `tests/ms03_irq_probe` is a pre-existing artifact and was not accepted as current
  evidence; `tests/ms04_rx_probe` was not created. Therefore no static artifact size/hash is
  claimed. Both static targets must be rerun in the final user-controlled environment.
- The sandbox also rejects host UDP socket creation with `EPERM`. The stimulus self-test therefore
  exercised the same parser, handshake, packet generator, bounds, malformed inputs, and exact
  96-packet sequence through an in-memory datagram seam. The production path still uses a real
  UDP socket; real loopback remains part of the final environment rerun.

### Boundaries and handoff

- Persisted Evidence: none, as approved.
- QEMU runtime: not run and not claimed.
- Rootfs and guest console: unchanged and untouched.
- Blocker handoff: none; the two environment-only checks are explicitly deferred by R44.
- Experience candidates: none.
- Base revision: `e0fac50ce01527a1c5dea83c36c37616a1a92590`; implementation remains in the working tree for
  Plan Review.

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 007 的 fault once-only 修复、coherent lifecycle/error pair、四态 IRQ witness、
固定 V1/独立 V2、software nudge 和协议工具主体可以保留。独立复验的 axnet 109 tests、
host 64 tests、MS16 host suite、kernel/axnet fmt、D1 target build、references/change strict
validation 与 staged/unstaged diff checks 均通过。Persisted Evidence 模式为 none，未创建
Evidence 目录符合计划。

1. **PASS — telemetry、IRQ 与 snapshot ABI 闭合。** 三个 active fault source 的完整
   Future tests 分别得到一次 fault 和原 stage/code；missing Service、单次 lifecycle load、
   packed error pair、enabled-entry/restore 分离、raw record 与 registration-failure mutation
   guards 均有见证。V1 command 只写 64 bytes，V2 独立写 224 bytes；canary、offset 和
   MS03/MS16/MS04 consumer inventory 通过。
2. **IMPORTANT — probe 只检查安全 counter delta，会掩盖 PRE 前的 IRQ 安全失败。**
   `common_delta_valid` 只要求本窗口 `fault/restore_violation/irq_enabled_entry` delta 为零，
   不要求 POST 绝对值为零。若 IRQ-enabled entry 或 false→true restore violation 在 PRE
   前已经发生，四个模式仍可能 PASS，违反 D9 和 runtime Gate 的“同次启动安全计数为零”。
3. **IMPORTANT — idle/nudge 的负向判定不完整。** `validate_idle` 不检查 ISR publish/wake
   或 software-nudge；`validate_nudge` 不检查 delivered、non-IP、Router-full、space-wake
   等额外进度。现有 6 个 C tests 只用 `task_poll=2` 使 idle 失败，并只给 nudge 注入
   `isr_publish=1`，无法证明其他不允许的字段会被拒绝。
4. **IMPORTANT — stable deadline 可以被过期的相等读数绕过。** `read_stable_snapshot`
   在第二次读取后先判断 progress 相等并返回成功，之后才检查 deadline。若 sleep、ioctl
   或调度延迟跨过 1000 ms，但两次值恰好相等，过期 snapshot 会被接受。现有 deadline
   helper test 没有覆盖“equal 且 expired”的决策优先级。
5. **IMPORTANT — 原计划的输出与 loopback 见证只部分实现。** 已识别 mode 的成功路径
   输出 PRE/POST/DELTA/terminal marker；读取失败、inactive 和 protocol failure 只输出
   FAIL，Act Response 的“all execution paths”表述过宽。原计划要求 `--self-test` 使用
   host loopback，当前因 sandbox EPERM 改成纯内存 seam；该偏差已诚实记录，但没有一个
   可在 sandbox 外原样复跑的有界 real-loopback self-test 入口。下一轮保留纯协议测试，
   另加真实 loopback 入口，并明确只有实际取得的数据才允许输出。
6. **PLAN-INVALID — T7 的全 manifest/workspace fmt Gate 会制造无关 vendor churn。**
   `cargo fmt` 对三个本地化依赖和根 workspace 分别失败；axdriver-net 至少触及未修改的
   fxmac/ixgbe，axdriver-virtio 触及 blk/gpu/input/socket，virtio-drivers 与根命令会重排
   大量 block/sound/socket/PCI/smoltcp 源码。kernel 和 axnet manifest fmt 单独通过。
   T7 已改为定向格式 change-owned adapter/queue 文件，并用 dependency tests、source
   audit 和 full diff Review 覆盖 vendor snapshot，禁止批量格式化无关源码。
7. **BASELINE-CHANGED — D1 compile debt 已消失。** 原 `lichee-d1-kbench` 命令在当前
   HEAD 完整退出 0，生成 478,672-byte ELF 与 159,936-byte bin；本轮只把这条新鲜结果
   用于关闭长期未完成的 T3.1。T7.2 仍需再次运行并把当次产物写入 Evidence。
8. **MINOR — Act Response 使用非标准状态。** `implemented-with-env-deferred-check` 不在
   `pending/reported/blocked` 状态集合中。内容、环境交接和工作树足以按 reported 结果
   审查；旧 Act Response 保持不可改，本 Review 记录该流程偏差。

**Deviation Classification**

- `ACT-DEVIATION`：probe 没有拒绝 PRE 前已存在的安全失败，idle/nudge negative matrix
  不完整，stable deadline 判定顺序不符合批准 contract。
- `ACT-DEVIATION`：host loopback self-test 被纯内存 seam 替代，且 Act Response 对所有
  failure path 的固定输出作了过宽声明；环境原因成立，但仍需可复跑入口。
- `PLAN-INVALID`：全 manifest/workspace fmt 会批量改写未修改的本地化依赖和 smoltcp，
  不符合 Surgical Changes，已收紧为 change-owned format Gate。
- `BASELINE-CHANGED`：D1 七个历史编译错误在当前 HEAD 不再复现。
- `NEW-EVIDENCE`：发现 post-absolute safety、idle/nudge mutation 和 equal-after-deadline
  缺口；发现完整 fmt Gate 的实际影响面。

**Evidence**

2026-08-12 独立复验：

| Command / inspection | Result |
|---|---|
| `make host-test` | PASS，exit 0：6 early-console + 8 memtrack + 26 MS03 + 12 MS04 + 6 probe decisions + stimulus self-test |
| `make network-benchmark-test` | PASS，exit 0：26 protocol + 20 platform + 21 tools + 9 integration |
| axnet 16-thread full suite | PASS，109/109，exit 0；仅 11 个既有 smoltcp warnings |
| D1 `lichee-d1-kbench` build | PASS，exit 0；ELF 478,672 bytes，bin 159,936 bytes |
| kernel/axnet manifest fmt | PASS，exit 0 |
| axdriver-net/axdriver-virtio/virtio-drivers/root full fmt | FAIL，exit 1；均为跨未修改 vendor 源码的大范围 rustfmt diff，不是环境失败 |
| `openspec validate references --strict` | PASS，exit 0 |
| `git diff --cached --check`; `git diff --check` | PASS，exit 0 |
| `ms04_rx_probe.c` source review | 复现 absolute safety、idle/nudge matrix 与 deadline ordering 缺口 |
| stimulus/Act Response review | 纯内存 self-test PASS；真实 loopback 仅有 EPERM 叙述，无独立复跑 mode |

**Follow-up Decision**

创建 iteration 008，把上述 probe/loopback 修复并入原定 T7 自动 Gate，不再单独拆轮。
先用 C decision mutations 和有界 loopback mode 关闭 Review findings，再执行所有自动
unit/check/build/source/spec/full-diff Gate，创建 change-local automatic Evidence，并把
仍按 R44 归类的 static probe/real-loopback 环境项交给 iteration 009。QEMU、rootfs、
guest shell 和 runtime Evidence 不进入 iteration 008。

**Next Iteration**

`iterations/008-probe-decision-closures-and-automatic-gates.md`，等待 Gate 2 批准。
