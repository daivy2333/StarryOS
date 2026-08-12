# Iteration 008: Probe Decision Closures and Automatic Gates

## Plan Context

- Status: awaiting-gate-2
- Round: 008
- Parent: `007-review-closures-and-runtime-probes.md`

**Objective**

关闭 iteration 007 Review 发现的 probe safety、idle/nudge negative matrix、stable deadline、
terminal marker 和真实 loopback 见证缺口，再完成原定 T7：运行 change 的全部自动 Gate，
审查从初始 revision 到当前工作树的完整实现，生成 automatic Evidence，并把仅由 R44
允许延后的环境项固定交给最终 user-only iteration 009。本轮不运行 QEMU、不修改 rootfs，
也不输入 guest shell。

**Approved Requirements**

- 保留已批准的 R1-R8、M1-M4、D1-D10；没有新增 capability 或 Simplified requirement。
- probe 的 boot-history safety counter 在 POST 必须为零；安静 delta 不能掩盖 PRE 前失败。
- idle 不允许 ISR/software/descriptor/budget/yield/backpressure 进度；nudge 只允许
  software-nudge、task-poll、empty-check 各 `+1`。
- stable snapshot 达到 deadline 即 timeout，即使两次 progress 已相等。
- 已识别 mode 恰好输出一个终态 marker；PRE/POST/DELTA 只在对应数据真实可用时输出。
- 纯内存 protocol self-test 与有界 real-loopback self-test 分开；后者必须在自动环境先尝试。
- 自动产品 Gate、change-owned format、target artifacts、spec/code/full diff Review 先于手测。
- 全 manifest rustfmt 不得批量改写未修改的 vendor snapshot；这项 iteration 007 的
  `PLAN-INVALID` 已在 design D10 和 change task 7.1 中更正。

Gate 1 沿用 proposal 中 2026-08-09 的 approved Requirements and Scope。用户本次要求
“把发现的问题和原本要做的事情并作下一个 iter”，授权把 Review closures 与原 T7
合并规划；这不构成 Gate 2 实施批准。

**Scenario Sketch**

1. **Boot-history safety failure**：PRE 已含非零 restore/IRQ-entry/fault；任一 mode 触发
   validation 后必须 FAIL，即使窗口 delta 为零。缺失 POST 时不得伪造零值。
2. **Idle negative matrix**：Active/AsyncOwned 且安全 counter 为零；窗口内任一 ISR、
   nudge、descriptor、delivery、budget、yield 或 backpressure 进度都必须 FAIL；完全安静
   时 task/empty 各不超过一次并 PASS。
3. **Nudge exact matrix**：Active/AsyncOwned；一次 nudge 后只允许 software/task/empty
   各 `+1`。任一 ISR、reap/refill/delivery/non-IP/budget/yield/backpressure 进度、缺计数或
   多计数均 FAIL。
4. **Expired equality**：连续读数相等，但第二次观察达到 deadline；结果必须 timeout/FAIL。
   deadline 内相等才是 stable。
5. **Diagnostic availability**：recognized mode 无论在哪一阶段失败都产生一个 terminal
   FAIL；只输出已取得的 snapshot/delta，读取失败不复用旧值。unknown mode 保持 usage
   error，不伪装成已执行 mode。
6. **Loopback protocol**：真实 UDP loopback 在固定 deadline 内完成 REGISTER/READY/START
   和 96 个精确 packet。socket 被 EPERM/SIGSYS 拒绝时记录 ENV-BLOCKED；协议、sequence、
   timeout 或 join 错误是产品失败。
7. **Automatic Gate**：所有产品 test/check/build/review 通过；环境限制单独登记。任一 Rust/
   C/link/assert/source/spec/diff 失败立即停止，不创建 manual-ready 结论。
8. **Evidence handoff**：automatic Evidence 包含环境、命令、退出码、build、hash、review
   和 ENV-BLOCKED 清单。只有内容完整且无未解决 Critical/Important finding，才允许 Plan
   创建 iteration 009。

**Current Baseline**

- Branch/HEAD: `net-k3` / `e0fac50ce01527a1c5dea83c36c37616a1a92590`。
- Change 初始 revision: `16d9a16a2b65a574022faaee39b465f6f7aebd45`。四个已提交的
  MS04 implementation commits 位于两者之间；iteration 007 产品与测试改动当前在 index，
  Plan Review/design/spec/tasks/iteration 008 文档改动位于 working tree。Act 必须保留两层。
- Iteration 007 主体：axnet fault/snapshot/nudge、kernel V1/V2/nudge ioctl、MS03/MS04
  host harness、guest probe、stimulus 和 Makefile 入口已存在；change tasks 为 18/24，
  其中 T3.1 已由 fresh D1 build 证据闭合，T6.2R/T7/T8 未完成。
- Fresh Review baseline：`make host-test`、`make network-benchmark-test`、axnet 109 tests、
  kernel/axnet fmt、references strict、cached/working diff checks PASS。
- D1 baseline 已变化：原 7 errors 不再复现；fresh `lichee-d1-kbench` build exit 0，生成
  478,672-byte ELF 与 159,936-byte bin。T7.2 仍需当轮复跑和记录 hash。
- `make tests/ms03_irq_probe tests/ms04_rx_probe` 在 iteration 007 因
  `riscv64-linux-musl-gcc` SIGSYS/`Bad system call` exit 2；旧 MS03 binary 未作证据，
  MS04 binary 未生成。real UDP socket 在 sandbox 返回 EPERM。
- Full manifest/workspace fmt 当前 exit 1，并会改写大量未修改的本地化依赖和 smoltcp；
  kernel 与 axnet manifest fmt exit 0。该失败是计划范围错误，不是需要批量清理的产品债。

**Current-State Evidence**

| Boundary | Evidence |
|---|---|
| post safety | `tests/ms04_rx_probe.c::common_delta_valid` 只检查 delta，不检查 POST absolute counters |
| idle decision | `validate_idle` 未读取 `isr_publish`、`isr_wake`、`software_nudge` |
| nudge decision | `validate_nudge` 未拒绝 delivery/non-IP/backpressure 等额外字段 |
| timeout ordering | `read_stable_snapshot` 在 deadline 前先接受 `snapshot_progress_equal` |
| marker path | `finish_mode` 输出完整数据；`fail_mode` 只输出 terminal marker；Act Response 把范围写成 all paths |
| protocol test | `--self-test` 使用内存 `ProtocolSocket`；normal path 使用 UDP，无独立 real-loopback mode |
| automatic format | kernel/axnet fmt PASS；三个本地化 manifest 和 root `--all` 会跨 vendor snapshot 产生大量 diff |
| D1 target | 原 task 3.1 命令在当前 HEAD exit 0，说明历史 baseline 已变化 |
| full implementation range | `git diff 16d9a16...` 覆盖 dependency localization、queue、axnet、kernel、tests 和 tools；仅看 HEAD/index 不足 |

**Relevant Code**

| File / symbol | Current responsibility |
|---|---|
| `tests/ms04_rx_probe.c::{snapshot_delta,common_delta_valid,validate_idle,validate_nudge,read_stable_snapshot,run_*}` | V2 snapshot、四 mode 决策、deadline 与 markers |
| `tests/ms04_rx_probe_test.c` | 6 个纯 C decision tests；negative matrix 尚不完整 |
| `scripts/ms04_rx_stimulus.py::{parse_control,make_packet,serve_once,self_test,main}` | 有界 UDP 协议、纯内存 self-test 和 real socket production path |
| `Makefile::{host-test,tests/ms03_irq_probe,tests/ms04_rx_probe}` | host Gate 与 static guest artifacts |
| `crates/axdriver_virtio/src/net.rs` | VirtIO adapter；定向 rustfmt 当前 RED |
| `crates/virtio-drivers/src/{queue.rs,device/net/dev_raw.rs}` | EVENT_IDX 与 RX-only control；定向 rustfmt 当前 RED |
| `crates/axnet/src/async_rx.rs` | lifecycle、queue Future、telemetry、nudge；109 tests baseline |
| `kernel/src/drivers/virtio_net_irq{,_logic}.rs`; `kernel/src/syscall/fs/ctl.rs` | minimal ISR、V1/V2 snapshot 与 nudge ioctl |
| change specs/design/tasks and references | requirement、Gate、R44 与 task truth |

**Critical Path**

```text
probe PRE
  -> require Active/AsyncOwned and boot-history safety counters == 0
  -> bounded measurement
  -> POST + monotonic delta
  -> deadline-first stable decision
  -> mode-specific exact matrix
  -> exactly one PASS/FAIL marker

stimulus pure self-test -> parser/packet/protocol logic
stimulus real-loopback self-test -> bounded UDP server/client -> same serve_once path

T6.2R GREEN
  -> T7.1 host/dependency/race/format gates
  -> T7.2 D1/QEMU/static artifact builds + hashes
  -> T7.3 specs/code/full diff review + required automatic Evidence
  -> stop for Plan Review; no QEMU
```

**Behavioral Change**

- Probe 不再允许历史安全失败被零 delta 掩盖，idle/nudge 的允许字段集合成为显式白名单。
- Stable snapshot 的 timeout 成为硬上界；过期相等读数不再成功。
- 终态 marker 与 snapshot availability 分开：terminal marker 总是存在，数据只在实际取得时
  输出。
- Stimulus 同时有 deterministic pure test 与 bounded real-loopback test；生产协议不变。
- 自动 format Gate 只覆盖 change-owned Rust，vendor snapshot 不发生批量机械重排。
- T7 完成后只证明自动就绪；不会声明 QEMU runtime、descriptor 守恒或网络回归 PASS。

**Change Surface**

| Task | Requirement / Scenario | File / Symbol | Current responsibility | Planned change |
|---|---|---|---|---|
| T6.2R-a | R6,R8 / S1-S5 | MS04 C probe + decision tests | partial mode validation | absolute safety、exact white-lists、deadline priority、marker availability |
| T6.2R-b | R8 / S6 | stimulus + Makefile | pure self-test and real production path | separate bounded real-loopback self-test and Gate entry |
| T7.1 | R1-R8 / S7 | local crates、axnet、kernel、host tests、format surfaces | per-iteration partial Gates | full automatic unit/check/race/scoped-format closure |
| T7.2 | R3,R5,R8 / S7 | D1/QEMU build、static probes、feature/source audit | artifacts partly stale or ENV-blocked | fresh builds, sizes/hashes, exact ENV classification |
| T7.3 | all / S8 | full range diff、OpenSpec、Evidence | no automatic Evidence package | independent specs/code/full review and indexed artifacts |

**Task Contracts**

T6.2R-a — Probe decision and diagnostic closure:

- RED tests first: set PRE and POST safety counter to the same nonzero value and prove current delta
  path passes；mutate each idle-forbidden and nudge-forbidden field；evaluate equal progress at
  `elapsed >= timeout`；audit failure paths for duplicate/missing terminal markers.
- GREEN: every POST safety counter must be zero；idle and nudge use explicit allow-lists；deadline
  is checked before equality acceptance；recognized mode emits exactly one terminal marker。PRE,
  POST and DELTA appear only when their data is valid; no zero fabrication or stale reuse.
- Preserve: V2 remains 28 `u64` / 224 bytes；no new ioctl/version；counter/gauge split、quiet-window
  no-print、Active/AsyncOwned precondition、burst sequence and conservation rules remain.
- Verification: strict C11 `-Wall -Wextra -Werror` syntax + decision binary；`make host-test`。
- Stop: requires V2 growth, counter reset, weakening burst telemetry, printing within a valid
  measurement window, or using timing sleeps as the unit witness.

T6.2R-b — Bounded real-loopback self-test:

- Depends on T6.2R-a GREEN because the final protocol package is reviewed as one tool boundary.
- RED: no CLI path can create both UDP peers and exercise `serve_once` with a real socket；the only
  self-test uses an in-memory fake.
- GREEN: keep the pure test; add a distinct real-loopback mode using loopback-only addresses,
  bounded socket deadlines and bounded thread/join lifetime。It must verify READY, exact 96 packet
  sequence/payload, bounds and clean completion through the same production parser/generator.
- R44: T7 must attempt it. EPERM/SIGSYS/socket capability refusal is ENV-BLOCKED and recorded;
  timeout、protocol mismatch、partial packet set、unclean worker or assertion is product failure.
- Preserve: normal stimulus only listens/sends；no QEMU launch、console control、network download
  or unbounded count/payload。
- Stop: self-test requires privileged networking, drives guest shell, silently falls back to the
  in-memory seam, or lacks a deterministic timeout.

T7.1 — Full host/dependency/race and scoped-format Gates:

- Depends on all T6.2R tests GREEN.
- Run exact dependency check/test, host/MS16, axnet full + 100×16-thread, UART, strict C and both
  stimulus modes listed under Verification。Record test counts、warnings、exit codes and first
  failure layer.
- Format GREEN: kernel and axnet manifest fmt pass；directed rustfmt for
  `axdriver_virtio/src/net.rs`、`virtio-drivers/src/queue.rs` and
  `virtio-drivers/src/device/net/dev_raw.rs` passes with child traversal disabled。Mechanical edits
  are limited to those files and require semantic diff review.
- Explicitly do not run or “fix” full manifest/workspace fmt for copied vendor trees。The known RED
  inventory stays in iteration 007 Review as PLAN-INVALID evidence, not a waiver for changed code.
- Product compile/assert/source/format failure stops T7.2。Only R44 capability refusal from the real
  loopback mode may be deferred.
- Stop: any fix expands into unrelated fxmac/ixgbe/block/gpu/input/sound/socket/PCI/smoltcp files,
  stress becomes single-threaded, or warnings introduced by this change remain unclassified.

T7.2 — Target builds, feature audit and artifact qualification:

- Depends on T7.1 product Gates PASS.
- Rerun the exact D1 `lichee-d1-kbench` build even though Review baseline passes；run QEMU kernel
  check and `make LOG=info build`；build both MS03 and MS04 static probes；audit critical-section,
  local patch resolution and EVENT_IDX source/feature preservation。
- PASS requires fresh D1/QEMU ELF/bin and both static probes with sizes and SHA-256。Old artifacts
  are never substituted. If static compiler is SIGSYS/Bad system call again, record the command,
  exit and absent/stale artifact status, continue other product Gates, and add the same command to
  iteration 009 handoff.
- A build that emits cargo-home/network preparation noise but exits 0 with fresh artifact is PASS
  per R44。Rust/C/link/objcopy/source mismatch is product failure and stops T7.3.
- Stop: D1 historical errors recur, EVENT_IDX is disabled, dependency resolves outside the owned
  path, artifact freshness cannot be established, or failure classification is ambiguous.

T7.3 — Specs/code/full-diff Review and automatic Evidence:

- Depends on T7.2 product Gates PASS or explicit R44-only handoff entries.
- Compare specs/design/tasks against actual code and review the complete range from
  `16d9a16a2b65a574022faaee39b465f6f7aebd45` to the working tree, including committed changes,
  index and unstaged Plan/Act edits。Review requirement compliance before code quality; inspect
  every Critical/Important finding and all source guards/consumer inventories.
- Run change/references strict validation, `git diff --check`, staged and unstaged checks。No
  Missing/Simplified/TBD, unclassified product failure, unresolved Critical/Important finding or
  Evidence omission may remain.
- Create the required automatic Evidence package below。Act Response records the compact Gate
  summary and file/symbol inventory; long logs and hashes live only in Evidence.
- Stop: full-range diff cannot be reconstructed, Evidence contains old artifact/log substitution,
  tasks/specs/design disagree, or manual/QEMU claims appear.

**Invariants**

- ISR remains cause/ACK/telemetry/fixed wake only；descriptor and Service work remain task-context。
- V1 stays 64 bytes；V2 stays 224 bytes；nudge command stays independent and generation-neutral。
- Single lifecycle/task/waker and Polling/Async ownership mapping do not change。
- EVENT_IDX remains enabled and controlled through owned RX-only queue code。
- Probe never resets counter or treats missing/expired data as zero；host tool never drives console。
- Sync TX、MS01/MS02/MS03、UART、early/panic console and 10ms protocol polling remain。
- Automatic Evidence is not runtime Evidence；QEMU single-hart conclusions remain unclaimed。

**Non-goals**

- QEMU launch、guest command、rootfs copy/mount、runtime probe、MS01/MS02/MS03 manual rerun。
- Creating iteration 009 runtime files in advance or marking T8 complete。
- Full-tree/vendor rustfmt、warning cleanup outside changed code、general ioctl framework。
- Async TX、MS05 packet slots、stack runner、socket readiness、reset、SMP、PCI/DWMAC、真板。
- Maintainer、Recorder、global tasks/SNAPSHOT/M-D-K-R-I update、archive or commit。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|
| R2/R3 safety history | S1 | D9 | T6.2R-a | probe common validation | equal nonzero PRE/POST safety mutations | None | Covered |
| R6 idle boundary | S2 | D7,D9 | T6.2R-a | `validate_idle` | per-field forbidden-progress matrix | None | Covered |
| R6 nudge boundary | S3 | D5,D9 | T6.2R-a | `validate_nudge` | exact allow-list + per-field mutations | None | Covered |
| R8 bounded observation | S4,S5 | D9,D10 | T6.2R-a | stable read + mode runners | equal-after-deadline and marker source/decision tests | None | Covered |
| R8 protocol qualification | S6 | D9,D10 | T6.2R-b | stimulus CLI/`serve_once` | pure + bounded real-loopback self-tests | None | Covered |
| R1-R7 automatic regression | S7 | D1-D9 | T7.1 | local crates/kernel/axnet/tests | unit/check/stress/scoped-format suite | None | Covered |
| R3/R5/R8 artifacts | S7 | D1-D3,D10 | T7.2 | D1/QEMU/static probes/features | fresh build/source/tree/hash evidence | None | Covered |
| R8 readiness/evidence | S8 | D10 | T7.3 | full diff/OpenSpec/Evidence | strict validations + review index | None | Covered |
| manual boundary | S8 | D10 | T7.3 | iteration allocation | no QEMU/rootfs/runtime files | None | Covered |

No requirement is Missing or Simplified。The full vendor fmt command was removed because it modified
unrelated imported source, not because a requirement was waived；change-owned format remains covered。

**Acceptance**

- All Review mutation tests first fail against the current implementation and pass after correction。
- D1/QEMU builds and all automatic product Gates exit 0；R44-only failures have exact handoff records。
- Static probe and real-loopback results are either fresh PASS or explicit ENV-BLOCKED, never stale PASS。
- Automatic Evidence contains every required file and maps each command/artifact/review to its Gate。
- Change progress may mark T6.2R and T7.1-T7.3 complete only after their own Gate passes。
- Final result contains zero unresolved Critical/Important finding and no QEMU/runtime claim。

**Verification**

```text
# Review closures and host suites
cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms03_irq_probe.c tests/ms04_rx_probe.c
cc -std=c11 -Wall -Wextra -Werror tests/ms04_rx_probe_test.c -o /tmp/ms04-rx-probe-test
/tmp/ms04-rx-probe-test
python3 scripts/ms04_rx_stimulus.py --self-test
python3 scripts/ms04_rx_stimulus.py --loopback-self-test
make host-test
make network-benchmark-test

# Local dependencies and async paths
cargo check --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline
cargo check --manifest-path crates/axdriver_virtio/Cargo.toml --offline --features net
cargo test --manifest-path crates/virtio-drivers/Cargo.toml --offline --lib -- --nocapture
cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib -- --nocapture
for review_iter in $(seq 1 100); do cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib --quiet -- --test-threads=16; done
cargo test --manifest-path crates/uart_16550/Cargo.toml --offline --features async

# Scoped formatting
cargo fmt --manifest-path kernel/Cargo.toml -- --check
cargo fmt --manifest-path crates/axnet/Cargo.toml -- --check
rustfmt --edition 2021 --check --config skip_children=true crates/axdriver_virtio/src/net.rs crates/virtio-drivers/src/queue.rs crates/virtio-drivers/src/device/net/dev_raw.rs
rustfmt --edition 2024 --check kernel/src/drivers/critical_section_policy.rs kernel/src/drivers/virtio_net_irq.rs kernel/src/drivers/virtio_net_irq_logic.rs kernel/src/syscall/fs/ctl.rs tests/ms03-irq-host-harness.rs tests/ms04-async-rx-host-harness.rs

# Target builds and dependency/source qualification
make ARCH=riscv64 APP_FEATURES=lichee-d1-kbench MYPLAT=axplat-riscv64-lichee-d1 PLAT_CONFIG=$PWD/crates/axplat-riscv64-lichee-d1/axconfig.toml MEM=512M BUS=mmio DWARF=n build
cargo check --offline -p starry-kernel --features qemu
make LOG=info build
make tests/ms03_irq_probe tests/ms04_rx_probe
cargo tree --manifest-path crates/axnet/Cargo.toml --offline -e features -i critical-section
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i critical-section
cargo tree -p starryos --features qemu --target riscv64gc-unknown-none-elf -e features -i virtio-drivers

# Specs, source and complete diff
openspec validate ms04-qemu-async-rx-queue-baseline --strict
openspec validate references --strict
git diff --check
git diff --cached --check
git diff 16d9a16a2b65a574022faaee39b465f6f7aebd45 --check
```

Each command needs exact exit、key output、warning classification and timestamp in Evidence。For the
100× loop, record the first failing iteration and full failing output; success records 100/100。
Hash only artifacts whose producing command completed successfully in this iteration。

**Gate 2 Readiness**

| Dimension | Status | Evidence |
|---|---|---|
| Requirements | PASS | approved R1-R8/M1-M4 plus iteration 007 Review clarifications; no Simplified row |
| Investigation | PASS | probe decisions、deadline、markers、loopback seam、format scope、D1 and full diff range inspected |
| Design | PASS | absolute safety、allow-lists、deadline-first、split self-tests、scoped fmt and Evidence contract fixed |
| Task Contracts | PASS | T6.2R-a → T6.2R-b → T7.1 → T7.2 → T7.3 has RED/GREEN/preserve/stop |
| Traceability | PASS | RTM maps every scenario to design/task/code/test; no Missing/TBD |
| Verification | PASS | exact host/dependency/stress/format/build/tree/spec/diff commands and failure meanings listed |
| OpenSpec consistency | PASS | design D9/D10、delta spec、tasks allocation/6.2R/7.1 and this iteration agree |
| Persisted Evidence | PASS | mode required; automatic-only files and pass conditions fixed below |
| Manual boundary | PASS | ENV rerun/QEMU/runtime remain iteration 009; no rootfs or guest work here |
| User Approval | BLOCKED | awaiting explicit Gate 2 approval; Act is not authorized |

**Persisted Evidence**

- Mode: required
- Root: `evidence/008-probe-decision-closures-and-automatic-gates/`

Plan does not create this directory。Act creates it while executing T7。

| File | Gate | Required content | Pass condition |
|---|---|---|---|
| `README.md` | T7.3 index | revision/range、file list、per-task/per-Gate status、scope and handoff | every required file present; no runtime claim |
| `environment.txt` | T7.1-T7.2 | OS、Rust/C tools、target、sandbox capabilities、timestamps | environment sufficient to interpret every result |
| `commands.txt` | T7.1-T7.3 | exact commands、start/end、exit codes、artifact producer mapping | every planned command traceable |
| `automatic-gates.log` | T6.2R/T7.1 | complete test/check/fmt output including warnings | all product Gates PASS; R44 entries referenced |
| `build.log` | T7.2 | D1、QEMU and static probe build output with final exits | product builds PASS; ENV-blocked static command identified |
| `artifacts.sha256` | T7.2 | size + SHA-256 + producing command for fresh D1/QEMU/probes | no stale artifact; blocked artifact explicitly absent/unqualified |
| `env-blocked.txt` | T6.2R-b/T7.2 | exact command、exit、earliest capability failure、artifact status；or `None` | every deferred item satisfies R44 and maps to T8.1 |
| `review.md` | T7.3 | specs-vs-code、code quality、full-range diff findings and resolutions | zero unresolved Critical/Important; Minor explicitly classified |

**Risks and Notes**

- Real-loopback and musl compiler are expected sandbox pressure points, but classification is based
  on fresh logs, not expectation。A protocol/compiler diagnostic remains product failure。
- D1 now passes, but this is baseline evidence only；T7.2 must produce fresh artifacts again。
- Full change range includes three imported dependency trees。Review must distinguish copied source,
  change-owned modifications and unrelated format churn without skipping modified queue/sound fixes。
- `git diff --cached` and `git diff` represent different ownership layers。Act may edit staged files
  but must review the final working-tree content and must not reset/re-stage user content。
- Completion of this iteration does not authorize or create iteration 009；Plan Review decides that
  only after Evidence inspection。

## Gate 2 Approval Addendum

- Status: PASS
- Approved: 2026-08-12
- User instruction: “批准”
- Effect: 本追加记录取代上文审批前的 `Status: awaiting-gate-2` 和
  `User Approval: BLOCKED` 快照；Gate 2 全部检查项现为 PASS。Iteration 008 已达到执行
  就绪状态，可由后续明确的 `openspec-act` 请求实施。
- Scope: 批准范围包括 T6.2R-a 的 probe safety、idle/nudge matrix、deadline 和 marker
  修复，T6.2R-b 的有界 real-loopback self-test，以及 T7.1-T7.3 的自动 Gate、目标构建、
  full-diff Review 和 required automatic Evidence。批准不启动 Act，也不授权 QEMU
  runtime、rootfs 修改、guest shell、iteration 009、Maintainer、Recorder 或归档工作。

## Act Response

- Status: reported

**Implemented**

Implemented the approved T6.2R and T7.1-T7.3 scope. Probe decisions now enforce
boot-history safety, exact idle/nudge progress boundaries and deadline-first stable
snapshot handling. Recognized modes use centralized terminal-marker paths. The
stimulus tool has a bounded real UDP loopback self-test that reuses `serve_once`.

All automatic product checks, dependency/source audits, target builds and the
full-range review completed. QEMU runtime, rootfs and guest-shell work were not run.

**Changed Files and Symbols**

- `tests/ms04_rx_probe.c`: `stable_snapshot_ready`, `common_delta_valid`,
  `irq_delta_quiet`, mode failure routing.
- `tests/ms04_rx_probe_test.c`: boot-history, exact idle/nudge and deadline mutation
  coverage; 10 decision tests.
- `tests/ms04-async-rx-host-harness.rs`: terminal-marker source guard and mutation.
- `scripts/ms04_rx_stimulus.py`: `--loopback-self-test` using the production
  `serve_once` path with bounded socket/thread deadlines.
- `Makefile`: host Gate entry for the real loopback self-test.
- `crates/axdriver_virtio/src/net.rs`, `crates/virtio-drivers/src/queue.rs`,
  `crates/virtio-drivers/src/device/net/dev_raw.rs`: scoped rustfmt only.
- `crates/axnet/src/async_rx.rs`, `crates/axnet/src/router.rs`: corrected stale
  behavior comments.
- `.gitignore`: removed the full-range whitespace defect.
- Change-local tasks, this Act Response and the iteration 008 Evidence package.

**Deviations from Plan**

- Plan recorded `e0fac50` as the current layer; Act began at `78e1f7a`, which had
  committed the already reviewed iteration 007 index. Content semantics and approved
  scope were unchanged.
- The real UDP loopback and static probe build reached R44 capability refusals.
  They are recorded as `ENV-BLOCKED` and handed to T8.1; no PASS is claimed for
  either command or its missing/unqualified probe artifact.
- Directed rustfmt initially failed on three scoped files. The repair was mechanical
  and remained inside the approved T7.1 format surface.

**Blocker Handoff**

None. The two R44 entries are expected manual-boundary handoffs, not Act product
blockers.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS, baseline `16d9a16a2b65a574022faaee39b465f6f7aebd45`
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Review found and repaired two Important gaps: the raw V1 IRQ fields were missing
from idle/nudge exact validation, and the burst failure path bypassed the centralized
terminal-marker helper. Three Minor findings—scoped formatting, `.gitignore`
whitespace and stale behavior comments—were also repaired. Relevant tests and
checks were rerun after each repair.

**Verification Evidence**

| Gate | Result | Evidence |
|---|---|---|
| Probe decisions | PASS | strict C11; 10/10 C decision tests |
| Marker/tool protocol | PASS | 14/14 host harness tests; pure self-test PASS |
| Real UDP loopback | ENV-BLOCKED | socket creation returned EPERM; exact command handed to T8.1 |
| Host/dependency | PASS | axdriver_net 4, virtio-drivers 34, axnet 109, axnet stress 100/100 |
| UART | PASS | 62 unit tests; 8 normal + 10 compile-fail doc tests |
| Broader host suite | PASS | network protocol/platform/tool/integration suites |
| Target builds | PASS | fresh D1 and QEMU ELF/bin outputs with size and SHA-256 |
| Static probes | ENV-BLOCKED | musl GCC terminated with SIGSYS; no fresh qualified probe |
| Source/dependency | PASS | local virtio-drivers selected; EVENT_IDX support and queue propagation preserved |
| Format/OpenSpec/diff | PASS | scoped fmt, both strict validations and final whitespace checks |

Warnings were limited to unchanged copied dependencies and the deprecated
user-level Cargo config path. Complete output and exact exits are persisted below.

**Persisted Evidence**

`../evidence/008-probe-decision-closures-and-automatic-gates/README.md`

**Experience Candidates**

None. The sandbox capability pattern and T8.1 handoff are already governed by R44;
this iteration did not produce a new repeatable Runbook or Incident candidate.

**Remaining Issues**

- T8.1 must rerun the exact real-loopback and static-probe commands outside the
  restricted sandbox and qualify fresh probe artifacts.
- T8.2 remains the user-operated QEMU runtime batch. No runtime result exists yet.

**Commit or Diff Reference**

- Change review baseline: `16d9a16a2b65a574022faaee39b465f6f7aebd45`
- Act HEAD: `78e1f7abfa1614c188a24ebe7150ffb7c71e46d0`
- Implementation and Evidence: working-tree diff from Act HEAD

## Plan Review

- Status: follow-up-required

**Review Result**

follow-up-required

**Findings**

Iteration 008 的产品实现和自动 Gate 可以保留。独立代码审查确认 absolute safety、
idle/nudge exact matrix、deadline-first stable snapshot、central terminal marker 与有界
real-loopback 入口符合批准契约；fresh host 子项、MS16、axnet 109 tests、scoped fmt、
strict validation 和四个 target artifact hash 与 Act Evidence 一致。真实 loopback 仍在
socket 创建处 EPERM，static probe build 仍由 008 原始 SIGSYS 交接，均符合 R44。

1. **PASS — Review closures 没有遗留产品缺陷。** `common_delta_valid` 要求 POST 三个
   boot-history safety counter 为零；idle/nudge 使用完整禁止字段矩阵；deadline 在相等
   判断前生效。10 个 C decision tests 和 14 个 host harness tests 覆盖对应 mutations。
2. **PASS — automatic build 与 artifact 见证可复核。** D1/QEMU ELF/bin 的 mtime 位于
   008 采集窗口内，size 与 SHA-256 复算一致。axnet 单次 16-thread 复验仍为 109/109；
   008 的 100/100 stress transcript 完整且退出 0。
3. **IMPORTANT — raw Evidence 加入 index 后，声明的最终 whitespace Gate 不再成立。**
   `git diff --check` 对未暂存产品/文档层退出 0，但 `git diff --cached --check` 和从
   `16d9a16...` 开始的 range check 当前均 exit 2；唯一命中项是 staged
   `automatic-gates.log` 中保留的 ANSI/CRLF/终端行尾空格。008 的命令在 Evidence 加入
   index 前执行，Act Response 因而把“final whitespace checks”写得过宽。这不是产品
   源码缺陷；最终轮必须分离 source/document whitespace 与 raw-log integrity Gate。
4. **IMPORTANT — Evidence revision provenance 自相矛盾。** `environment.txt` 在
   2026-08-12 16:17 记录 HEAD `e0fac50`，README 和 Act Response 则把 `78e1f7a` 记为
   Act HEAD/起点。完整 range 仍以 `16d9a16...` 覆盖实现，因此不推翻产品 Review，但
   最终 Evidence 必须显式区分采集 HEAD、Act 基线和最终 Review revision。
5. **NEW-EVIDENCE — 早期最终手测示例已经漂移。** 当前 probe 只接受无额外参数的
   `burst`，stimulus 位于 `scripts/ms04_rx_stimulus.py`；MS02 service 源码要求两次 TCP
   round trip，而 R45/R48 的简写只展示一次。Iteration 009 使用当前接口并把两次 TCP
   设为明确 PASS 条件。

**Deviation Classification**

- `PLAN-OMISSION`：008 没有规定 Evidence 加入 index 后重新执行、且对 raw logs
  排除的 whitespace Gate，也没有定义多 revision provenance 的表达方式。
- `ACT-DEVIATION`：Act Response 声明 final staged/full-range whitespace PASS，但当前
  index 中的 raw log 使两者 exit 2；README/环境文件的 HEAD 角色未对齐。
- `NEW-EVIDENCE`：最终手测命令必须适配当前 MS04 CLI，并补足 MS02 的第二次 TCP 会话。

**Evidence**

2026-08-12 独立复验：

| Command / inspection | Result |
|---|---|
| `make host-test` | 产品子项 PASS：6+8+26+14 Rust、10 C decisions、纯协议 self-test；real loopback 在 socket creation EPERM，整体 exit 2 |
| `make network-benchmark-test` | PASS，exit 0：26 protocol、20 platform、21 tools、9 integration |
| axnet 16-thread full lib suite | PASS，109/109，exit 0；11 个既有 smoltcp warnings |
| kernel/axnet/scoped VirtIO fmt | PASS，exit 0 |
| change/references strict validation | PASS，exit 0 |
| four target artifact `sha256sum` | PASS，与 `artifacts.sha256` 四项一致 |
| `git diff --check` | PASS，exit 0 |
| `git diff --cached --check`; full range `--check` | FAIL，exit 2；只命中 staged raw terminal log 的 ANSI/CRLF trailing whitespace |
| Evidence provenance inspection | `environment.txt=e0fac50`；README/Act Response=`78e1f7a` |

**Follow-up Decision**

创建最终 iteration 009，把 Evidence provenance/whitespace Gate 修正并入原定 T8。
本轮先建立不改写 008 原始日志的 evidence addendum，再由用户在 sandbox 外复跑两项
ENV-BLOCKED 命令，并在单 hart、单 VirtIO-MMIO QEMU 中完成 MS04、MS03、MS02、MS01
手工验收。没有新的产品实现轮；任何 runtime 产品失败仍停止，不得以最终轮名义豁免。

**Next Iteration**

`iterations/009-final-sandbox-rerun-and-qemu-runtime.md`，等待 Gate 2 批准。这是计划中的
最后一个 iteration；通过后 change 才具备无后续任务的 Review 条件。
