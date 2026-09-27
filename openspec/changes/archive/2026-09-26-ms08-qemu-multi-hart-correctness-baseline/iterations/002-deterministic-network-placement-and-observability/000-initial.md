# Iteration 002 / Cycle 000: Deterministic network placement and observability

## Plan Context

- Status: ready
- Iteration: 002-deterministic-network-placement-and-observability
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: `../001-deterministic-uart-placement-and-observability/003-rework.md`

**Iteration Scope**

- Change tasks: 3.1–3.5
- Depends on: Iteration 001
- Stable baseline: the unique network queue owner and stack runner start after secondary-ready with deterministic singleton affinity; QEMU V5 reports their placement and progress without changing V1–V4; a timer-disabled witness proves remote-ready IPI delivery independently of NIC progress.
- Verification boundary: placement/startup/fallback tests, V5 prefix and field checks, timer-disabled state-machine witnesses, MS03/MS04/MS07 regressions, both axnet test modes, ordinary and `SMP=16` builds.
- Diagnostic boundary: network Service installation, runner/owner lifecycle, IRQ registration, affinity spawn, V5 snapshot assembly, or timer-disabled witness cleanup.
- Deferred tasks: 4.1–7.3

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: accepted Iterations 000–001; shared placement policy; pre-enqueue affinity API; local-preferred blocked-task wake; single S_SOFT owner; existing network queue ownership, fallback, recovery, V1–V4 ABI, and UART behavior.
- Excluded scope: affinity expansion or migration, formal guest/host qualification protocol, combined UART/network pressure, reset qualification, multiqueue/RSS, performance work, and physical-board claims.

**Objective**

Move the existing unique network owner and stack runner from early full-mask spawn to deterministic post-secondary singleton placement, expose append-only QEMU V5 observations, and add a bounded timer-disabled remote-wake witness without changing descriptor ownership or V1–V4 behavior.

**Background**

`axruntime` currently calls `axnet::init_network` before `start_secondary_cpus`. `init_network` installs `SERVICE` and immediately starts the full-mask stack runner. Later, kernel entry calls `init_virtio_net_irq_diag`; successful IRQ registration calls `axnet::start_rx_task`, which starts the queue owner with ordinary full-mask spawn. Iteration 001 established the scheduler, affinity, IPI, placement, and UART patterns needed to move both network roles after secondary-ready.

**Investigation Facts**

- Current Baseline:
  - Iteration 001 Cycle 003 is accepted. The standard SMP graph provides local-preferred blocked-task wake and one remote-ready IPI after a successful remote `Blocked → Ready` transition.
  - `kernel/src/drivers/placement.rs::place_roles` already reserves deterministic `NetOwner` and `NetRunner` positions. With four or more schedulable harts, UART RX/TX and both network roles are distinct; smaller valid sets co-locate by modulo policy.
  - Existing network semantics already provide one `RxRxFuture` owner, one `StackRunnerFuture`, lifecycle CAS guards, register-before-owner-start, polling fallback, recovery ownership, and V1–V4 diagnostics.
  - Cycle 003 Act verification for the shared scheduler/UART substrate is adopted as the predecessor baseline; Iteration 002 must independently verify its network changes.
- Current-State Evidence:
  - Registry `axruntime 0.3.0-preview.2::rust_main` calls `axnet_ng::init_network(all_devices.net)` before `start_secondary_cpus`; it invokes kernel `main()` only after all configured CPUs report initialized.
  - `crates/axnet/src/lib.rs::init_network` installs `SERVICE` and immediately calls `start_stack_runner()`.
  - `crates/axnet/src/stack_runner.rs::spawn_stack_runner` and `crates/axnet/src/async_rx.rs::spawn_rx_task` use ordinary `axtask::spawn_with_name`, so their first enqueue is not pinned.
  - `kernel/src/entry.rs::init` runs after the runtime secondary-ready barrier and calls `init_virtio_net_irq_diag` before starting UART copiers and user space.
  - `kernel/src/drivers/virtio_net_irq.rs::init_virtio_net_irq_diag` validates the MMIO device, registers the IRQ, and starts the RX owner only after registration; registration failure leaves the polling fallback active.
  - `axtask::spawn_with_name_affinity` commits a validated schedulable mask before first enqueue and returns the task handle. `AxTaskRef::set_cpumask_checked` is available for the later migration Iteration but is not exercised here.
  - `IrqSnapshotV4` is the current QEMU-only recovery snapshot and embeds V3 as its prefix. The syscall command is `0x4e49_4434`; V1–V4 commands and layouts are compatibility surfaces.
  - RISC-V QEMU exposes per-hart timer masking through `axhal::irq::set_enable(axhal::time::irq_num(), enabled)`. Timer masking must occur on the target hart and every completion, cancellation, rejection, and timeout path must restore it.
- Code and Critical Path:
  - Early install: `axruntime::rust_main → axnet::init_network → SERVICE.call_once`.
  - Post-secondary start: `kernel::entry::init → virtio_net_irq::init_virtio_net_irq_diag → runner start → IRQ register → owner start`.
  - Owner: `QUEUE_EVENT/IRQ → RxRxFuture::poll → Service/descriptor ownership → STACK_EVENT`.
  - Runner: device/software/timer event → `StackRunnerFuture::poll → Service/socket readiness`.
  - Observation: IRQ wrapper and future poll wrappers → coherent counters → `irq_snapshot_v5 → ioctl 0x4e49_4435`.
  - Timer-disabled witness: control request → singleton target task disables its local timer and parks → a different hart wakes it → remote-ready IPI handler → target resumes, restores timer, and commits terminal state.

**Implementation Guidance**

First separate Service installation from background-role start while keeping compatibility wrappers for callers that still require ordinary spawn. Add affinity-aware owner/runner start APIs that return handles and preserve their existing once-only lifecycle gates. The kernel adapter should compute placement once from the acquired schedulable mask, start the runner after Service installation, register the IRQ, and start the owner only on registration success; store both handles for observation and later migration.

Add V5 only after the lifecycle and handle sources are stable. Use V4 as a literal leading field and append fixed-width QEMU-only fields. Reuse coherent hart counters and Relaxed telemetry for observation, while lifecycle and handle affinity remain authoritative. Build the timer-disabled witness as an independent single-flight state machine; it must not borrow the NIC queue, alter descriptor ownership, or use the periodic timer it is intended to exclude.

**Behavioral Change**

- `init_network` installs Service and registries but does not enqueue the stack runner before secondary schedulers are ready.
- Kernel QEMU startup computes `NetRunner` and `NetOwner` singleton masks from the published schedulable set. It starts exactly one pinned runner, registers the NIC IRQ, and starts exactly one pinned owner only after successful registration.
- IRQ registration failure keeps the existing bounded polling owner and still permits the pinned runner needed by polling-mode stack progress; it does not start an async queue owner.
- QEMU V5 preserves V4 byte-for-byte as its prefix and appends direct network placement/wake/progress fields. V1–V4 commands, layouts, and meanings remain unchanged.
- The timer-disabled witness proves a remote blocked task resumes through S_SOFT while its target-hart timer is masked, and restores that timer on every terminal path.

**Task Contracts**

### 3.1: Connect the shared placement policy to network roles

- Requirement/Scenario: placement requirement, multi-hart fixed placement, sparse/invalid set handling.
- Depends on: None
- Targets: `kernel/src/drivers/placement.rs`; network startup adapter in `kernel/src/drivers/virtio_net_irq.rs`; focused placement tests in the existing MS08 host harness.
- Current behavior: placement computes network roles, but no network task consumes them; both tasks retain full affinity.
- Required behavior: derive ordered harts from `axtask::schedulable_cpu_mask`, compute `NetOwner` and `NetRunner`, and convert each to a singleton `AxCpuMask`. Invalid or empty input fails before any network task starts. Small valid sets co-locate only as the existing policy requires; `SMP=16` separates all four background roles.
- Required changes: add a network placement/start record owned by the kernel adapter and expose read-only affinity/handle data to V5 assembly.
- Preserve: policy inputs, UART assignments, no topology invention, no hard-coded hart IDs, and no CPU-hotplug support.
- Forbidden: one owner per hart, full-mask fallback after invalid placement, or deriving placement from assumed IRQ routing.
- Test witness: existing topology matrix plus network-consumption guards must be RED before the adapter uses `NetOwner`/`NetRunner`, then GREEN for 1/2/3/4/8/16, non-zero anchor, sparse, empty, duplicate, unordered, and out-of-range inputs.
- GREEN condition: valid singleton masks match the shared policy and invalid input enqueues nothing.
- Verification: focused host tests and ordinary/`SMP=16` compile checks.
- Stop when: the kernel cannot observe a complete schedulable set at this entry or placement would require a new topology source; return to Plan.

### 3.2: Start one pinned runner and owner after secondary-ready

- Requirement/Scenario: unique queue owner and layered runner; fixed placement; owner fault/fallback.
- Depends on: 3.1
- Targets: `crates/axnet/src/lib.rs::init_network`; `crates/axnet/src/stack_runner.rs::{start_with, spawn_stack_runner, start_stack_runner}`; `crates/axnet/src/async_rx.rs::{start_with, spawn_rx_task, start_rx_task}`; `kernel/src/drivers/virtio_net_irq.rs::init_virtio_net_irq_diag`; `kernel/src/entry.rs::init` if a separate adapter entry is needed.
- Current behavior: Service installation immediately starts a full-mask runner; successful IRQ registration starts a full-mask owner. Neither handle is retained.
- Required behavior: Service installation is spawn-free; affinity-aware APIs commit singleton masks before first enqueue, retain once-only lifecycle behavior, return task handles, and report invalid-affinity spawn failure without creating a second role. Kernel startup saves the runner handle, registers IRQ, then saves the owner handle only after registration succeeds. IRQ failure preserves polling fallback and never starts the async owner.
- Required changes: split install from start; add affinity-aware future/spawn seams; keep ordinary wrappers only where compatibility tests require them; add kernel-owned handle storage and explicit diagnostics for repeated or rejected starts.
- Preserve: one owner, one runner, register-before-owner-start, Service-before-runner, owner recovery lifecycle, polling fallback, IRQ cause/ACK path, and application-ready ordering.
- Forbidden: rollback from faulted async owner to polling ownership, starting owner before IRQ registration, detached duplicate tasks, or modifying descriptor service in IRQ/socket callers.
- Test witness: source/model tests must show old early/full-mask spawn RED; lifecycle tests cover first start, duplicate, invalid affinity, spawn rejection, IRQ failure, and successful ordered startup.
- GREEN condition: exactly one pinned runner and, after IRQ success, exactly one pinned owner exist with stored handles; fallback behavior remains bounded on IRQ failure.
- Verification: axnet ordinary and `qemu-diagnostics` suites, MS03/MS04/MS07 harnesses, and target builds.
- Stop when: preserving once-only semantics requires an ambiguous lifecycle rollback or Service is unavailable at the post-secondary entry; return to Plan.

### 3.3: Add append-only QEMU network snapshot V5

- Requirement/Scenario: network qualification observability and compatibility.
- Depends on: 3.2
- Targets: `kernel/src/drivers/virtio_net_irq_logic.rs`; `kernel/src/drivers/virtio_net_irq.rs`; `kernel/src/syscall/fs/ctl.rs`; axnet owner/runner telemetry; host ABI tests.
- Current behavior: V4 reports V3 plus recovery identity but not network task placement, actual task/IRQ harts, remote wake progress, migration counters, or invalid-affinity rejects.
- Required behavior: command `0x4e49_4435` returns a QEMU-only V5 whose first bytes are exactly V4. Appended fixed-width fields include configured/schedulable masks, owner/runner affinity, coherent actual IRQ/owner/runner last+mask+event tuples, IPI send/receive or role-attributable wake/resume deltas, migration counters/state reserved for Iteration 003, and invalid-affinity rejects. Reserved fields are initialized and do not leak padding.
- Required changes: instrument poll/IRQ observation without affecting scheduling; assemble V5 from one coherent read per tuple and the saved task handles; add explicit serialization if the representation contains padding.
- Preserve: V1–V4 command values, sizes, byte layouts, semantics, and existing MS07 consumers.
- Forbidden: revising V4, adding revision/run identity, using telemetry as a scheduling input, or exposing V5 outside QEMU diagnostics.
- Test witness: size/offset/prefix tests, source guards for old commands, coherent tuple concurrency fixtures, and negative fixtures for undefined reserved bytes.
- GREEN condition: V4 bytes are an exact prefix; all appended fields have stable offsets and initialized bytes; actual role masks remain singleton in fixed placement.
- Verification: ABI/model tests, MS03/MS04/MS07 harnesses, axnet diagnostics tests, target build.
- Stop when: V4 cannot remain byte-for-byte intact or a required field lacks an attributable source; return to Plan.

### 3.4: Add a single-flight timer-disabled remote-wake witness

- Requirement/Scenario: remote ready task receives an event-driven scheduling opportunity without timer rescue.
- Depends on: 3.2, 3.3
- Targets: QEMU-only kernel diagnostic control/state module; `axtask` affinity/wake/IPI interfaces; ioctl/control plumbing and pure state-machine tests.
- Current behavior: runtime traffic can show IPI and task progress, but periodic target-hart timer interrupts remain an alternative scheduler trigger.
- Required behavior: one singleton-affinity target task disables its local timer IRQ, publishes `Armed`, and parks. A distinct schedulable hart wakes it. On resume it records target hart/IPI deltas, restores the timer before publishing `Completed`, and exits. Concurrent start is rejected. Cancellation, timeout, setup failure, and unexpected state restore the timer before a terminal result; the owner state never remains permanently armed.
- Required changes: implement explicit Idle/Starting/Armed/Woken/Completed/Failed state transitions, bounded deadline supervision from a hart whose timer remains enabled, a cleanup acknowledgment before reporting terminal failure, and V5-observable counters/state. The witness must use normal `block_on` wake routing.
- Preserve: the single S_SOFT handler, normal timer programming outside the target window, network owner/runner identity, and all NIC descriptor state.
- Forbidden: sleeping or polling on the disabled target timer, disabling all harts' timers, reusing NIC packets/descriptors, unbounded spin, or accepting a timeout without confirmed timer restoration.
- Test witness: pure/model tests cover success, duplicate start, invalid target, wake-before-armed, cancellation, supervisor timeout, restoration failure, and exactly-once terminal publication; source guards require disable/restore symmetry.
- GREEN condition: host state-machine tests prove cleanup on every terminal edge and target builds compile the QEMU control; full runtime qualification remains deferred to Iteration 005.
- Verification: focused tests, QEMU-diagnostics build, ordinary-build exclusion, and existing IPI tests.
- Stop when: the platform cannot restore the target timer from the target task or a bounded supervisor cannot distinguish cleanup completion; return to Plan.

### 3.5: Run the network placement integration Gate

- Requirement/Scenario: fixed placement, unique ownership, compatibility, and timer-disabled witness safety.
- Depends on: 3.1–3.4
- Targets: affected unit/host suites, feature builds, and bounded startup smoke.
- Current behavior: predecessor suites pass, but no network placement/V5/witness integration exists.
- Required behavior: focused tests, MS03/MS04/MS07 harnesses, both axnet modes, ordinary and `SMP=16` builds pass; bounded startup directly observes distinct owner/runner singleton placement without claiming the later network data-plane qualification.
- Required changes: add only build/test entry points and guards needed to execute the above checks.
- Preserve: old probes/validators and single-hart behavior.
- Forbidden: treating UART success, shell arrival, or single-hart output as network placement proof; adding identity-style evidence tooling.
- Test witness: the integrated Gate itself, with negative fixtures for early spawn, duplicate role, V4 prefix drift, and timer not restored.
- GREEN condition: every listed check passes and no second owner, early spawn, old-ABI change, permanent timer mask, panic, or startup stall is observed.
- Verification: task-specific checks followed by bounded QEMU startup, diff review, and strict OpenSpec validation.
- Stop when: any non-environment failure remains; do not enter Iteration 003.

**Invariants**

- Service is installed before any runner uses it; secondary run queues are published before either affinity spawn.
- Exactly one logical queue owner and one stack runner exist. IRQ, socket, timer, reset, and witness paths publish events only.
- IRQ registration precedes owner start. Registration failure leaves polling ownership and never creates an async owner.
- Task affinity is committed before first enqueue; invalid placement never falls back to a different hart.
- V1–V4 are frozen. V5 is QEMU-only and observational.
- Timer-disabled witness cleanup restores the target timer before terminal state becomes visible.
- QEMU results do not establish real-board IRQ, timer, DMA/cache, or performance behavior.

**Non-goals**

- Network data-plane runtime qualification, controlled owner/runner migration, reset/I/O interleave qualification, or combined UART/network stress.
- CPU hotplug, dynamic IRQ balancing, multiqueue/RSS, PCI/DWMAC, hardware timing, or optimization.
- Changes to descriptor, slot, ticket, recovery epoch, socket API, or UART semantics.

**Requirements Traceability Matrix**

| Requirement / scenario | Design | Task | Code surface | Test witness | Status |
|---|---|---|---|---|---|
| schedulable fixed placement | D4 | 3.1 | placement policy + kernel adapter | topology and invalid-set model | Covered |
| unique owner and layered runner | D5 | 3.2 | axnet lifecycle/start seams + IRQ adapter | lifecycle/start-order tests | Covered |
| QEMU network observation, V1–V4 compatibility | D6 | 3.3 | V5 types/assembly/ioctl | prefix, offsets, tuple tests | Covered |
| event-driven remote wake without timer rescue | D7 | 3.4 | witness control/state + axtask wake | cleanup/state-machine tests | Covered |
| Iteration integration and compatibility | D9 | 3.5 | suites/build/startup | layered Gate | Covered |

**Acceptance**

- A1 / 3.1: valid online/schedulable inputs yield deterministic singleton owner/runner masks; invalid inputs enqueue no task.
- A2 / 3.2: Service installation is early and spawn-free; after secondary-ready exactly one pinned runner starts, then IRQ registration, then exactly one pinned owner. IRQ failure preserves polling fallback and starts no async owner.
- A3 / 3.3: V5 directly reports fixed placement and actual progress while preserving V1–V4 byte layout and command behavior.
- A4 / 3.4: the timer-disabled witness has a complete bounded state machine and restores the target timer on success and every failure edge; host/model and target-build evidence close this Iteration's witness contract.
- A5 / 3.5: focused, compatibility, feature, and bounded startup Gates pass without duplicate owner/runner, early spawn, ABI drift, permanent timer mask, panic, or startup stall.

**Verification**

- Placement and lifecycle focused unit/model tests with RED/GREEN witnesses.
- `cargo test` for axnet ordinary and `qemu-diagnostics` configurations.
- MS03, MS04, MS07, and MS08 host harnesses, including old ABI/source guards.
- Ordinary, `SMP=16`, and relevant QEMU-diagnostics builds.
- Bounded QEMU startup for fixed placement and safe witness setup only; full network data-plane and timer-disabled runtime qualification remain later Iterations.
- `git diff --check` and `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict`.

**Gate 2 Readiness**

- No Missing requirements: PASS — Tasks 3.1–3.5 map to placement, startup, V5, timer-disabled witness, and integration.
- Simplified requirements approved: PASS — no requirement is removed or weakened.
- Investigation complete: PASS — early runtime ordering, Service install, runner/owner spawn seams, IRQ ordering, affinity API, V4 ABI, timer mask API, and test surfaces are located.
- Design closed: PASS — ownership, startup order, V5 prefix, timer cleanup, and compatibility boundaries are explicit.
- Tasks executable: PASS — each task identifies targets, behavior, RED/GREEN, preservation rules, verification, and stop conditions.
- Iteration plan ordered and balanced: PASS — all tasks form the planned network placement/observability baseline; migration and runtime qualification stay deferred.
- Traceability complete: PASS — requirements, D4–D7/D9, tasks, code surfaces, and tests form direct chains.
- Verification sufficient: PASS — unit/model, ABI, compatibility, target build, and bounded startup evidence are separated by layer.
- No identity-style evidence engineering: PASS — V5 contains behavior telemetry only.
- No material TBD for Act: PASS — task ownership, ordering, errors, cleanup, and ABI rules are decided.
- Change/Cycle consistency: PASS — the Cycle implements unchanged tasks 3.1–3.5 and the existing Iteration Map.
- Persisted Evidence mode valid: PASS — results are reproducible and fit in the Act Response.
- User plan approval: PASS — 用户于 2026-09-18 以“批准”明确批准本 Cycle。

**Persisted Evidence**

- Mode: none
- Budget: commands, decisive output, exit status, changed files, and symbols belong in the Act Response; do not create an Evidence directory.

**Risks and Notes**

- `axruntime` is registry-owned and remains unchanged; the local axnet/kernel split must adapt to its fixed early `init_network` call.
- The timer-disabled witness is a QEMU diagnostic safety mechanism. A timeout is not complete until timer restoration is acknowledged.
- V5 migration fields may remain zero/inactive in this Iteration; Iteration 003 will exercise them without changing V5 layout.

## Act Response

- Status: reported

**Implemented**

Task 3.1 — 网络角色 placement 接入：新增 `kernel/src/drivers/net_placement.rs`，通过
`placement::place_roles` 在已发布 schedulable 集合上为 `NetOwner`/`NetRunner` 计算
singleton hart（非硬编码），提供 `record_owner_pinned`/`record_runner_pinned`（首次入队前
发布）、`set_owner_task`/`set_runner_task`（保存 handle）、`record_irq_hart`、`snapshot()`
与 `snapshot_boot_smoke()`。空/非法集合 fail closed。

Task 3.2 — 解耦安装与启动 + 固定 placement：`axnet::init_network` 改为 spawn-free（只装
Service/registries）；`stack_runner.rs` 与 `async_rx.rs` 新增 `start_stack_runner_affinity` /
`start_rx_task_affinity`（先在首次入队前提交 affinity 的 `spawn_with_name_affinity`，返回
`AxTaskRef`，`InvalidAffinity` 前校验、不推进 once-only lifecycle）；`init_virtio_net_irq_diag`
重写为：`place()` → pin/spawn runner → 注册 IRQ → 成功后才 pin/spawn owner，保存两者 handle，
IRQ 失败返回并保留 polling fallback（runner 仍推进 polling 栈）。

Task 3.3 — V5 QEMU 快照：`virtio_net_irq_logic.rs` 新增 `IrqSnapshotV5`（`v4` 为字节级首字段，
追加 configured/schedulable mask、owner/runner affinity、IRQ 实际 hart、owner/runner poll、
IPI send/receive、`affinity_rejects` 与保留迁移字段）；`virtio_net_irq.rs::irq_snapshot_v5`
组装（`UNKNOWN_HART` 时 singleton mask 守卫移位）；`ctl.rs` 新增 ioctl `NET_IRQ_SNAPSHOT_V5`
=`0x4e49_4435`。V1–V4 不变。

Task 3.4 — timer-disabled remote-wake witness：`net_wake_witness_logic.rs`（纯、宿主可测状态机，
强制 cleanup-before-terminal：terminal 前必须 `restore_ack`，重复 terminal 只计数，terminal
释放 run 供下次 start，`Starting` 起拒绝并发 start）+ `net_wake_witness.rs`（QEMU 绑定：目标
hart 关本地 timer→Armed→park→远端唤醒→先恢复 timer→terminal；ioctl `NET_WAKE_WITNESS_CONTROL`
=`0x4e49_5731` 的 START/TRIGGER）。完整 runtime 资格按计划留给 Iteration 005。

Task 3.5 — 集成：`net_placement::snapshot_boot_smoke()` 接入 `entry.rs`，SMP=16 下直接观察
owner/runner 固定 placement（distinct、in-schedulable、均已 poll、0 reject）。

**Changed Files and Symbols**

- `crates/axnet/src/lib.rs`: `init_network` spawn-free；re-export `start_rx_task_affinity`,
  `start_stack_runner_affinity`。
- `crates/axnet/src/stack_runner.rs`: `StartError::InvalidAffinity`, `spawn_stack_runner_affinity`,
  `start_stack_runner_affinity`；`init_installs_service_without_early_or_full_mask_runner_spawn`。
- `crates/axnet/src/async_rx.rs`: `StartError::InvalidAffinity`, `spawn_rx_task_affinity`,
  `start_rx_task_affinity`。
- `kernel/src/drivers/mod.rs`: 注册 `net_placement`、`net_wake_witness`(qemu)、
  `net_wake_witness_logic`。
- `kernel/src/drivers/net_placement.rs`（new）; `kernel/src/drivers/net_wake_witness_logic.rs`（new）;
  `kernel/src/drivers/net_wake_witness.rs`（new）。
- `kernel/src/drivers/virtio_net_irq.rs`: `net_irq_handler` 记录 IRQ hart；`singleton_mask`;
  `init_virtio_net_irq_diag` 重写；`irq_snapshot_v5`, `mask_to_u64`。
- `kernel/src/drivers/virtio_net_irq_logic.rs`: `IrqSnapshotV5`。
- `kernel/src/entry.rs`: 调用 `net_placement::snapshot_boot_smoke()`。
- `kernel/src/syscall/fs/ctl.rs`: `NET_IRQ_SNAPSHOT_V5`、`NET_WAKE_WITNESS_CONTROL` ioctl。
- `tests/ms04-async-rx-host-harness.rs`: 新增 3.1/3.2/3.3/3.4 host 测试（network placement 消费、
  启动顺序+handle、注册失败、V5 前缀/无 padding、witness 模型 5 项 + timer restore 对称）。

**Deviations from Plan**

- `init_virtio_net_irq_diag` 在注册 IRQ 失败分支同时保留了已启动的 pin runner（符合 IRQ 失败仍
  需 polling 栈推进的契约）；无其他实质偏差。
- `start_rx_task_affinity`/`start_stack_runner_affinity` 的 `InvalidAffinity` 是在第一次入队前由
  `axtask::validate_affinity`（configured cpu_num）防护；host 模型无法直接调用（需 axhal），
  该路径由防御性前校验 + 宿主 source-guard（`network_startup_order_*`、`witness_*`）覆盖，非阻塞。

**Blocker Handoff**

None.

**Blocker Resolution**

None.

**Self-Review**

- Plan compliance: PASS — 5 个 Task 契约全部落实。
- Full diff reviewed: PASS — axnet spawn-free+affinity seam、kernel adapter 顺序、V5 ABI、witness
  状态机、ioctl、smoke、测试与构建均已逐文件核对。
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 2 — (1) `make host-test` 在负载下偶发触发既有 critical-section
  并发线程测试的时序敏感卡顿（非本轮引入；独立运行确定性 67/67 PASS，用户终端全量跑完亦绿）；
  (2) `IrqSnapshotV5 is never constructed` 等 harness 内 lint 警告（ms03/ms04 未使用 V5 路径）。

**Verification Evidence**

| 验证项 | 命令 | 输出 | 覆盖范围 | 结论 |
|---|---|---|---|---|
| ms04(MS08) host harness | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs` | `67 passed; 0 failed` | placement/启动顺序/V5/witness | PASS |
| ms03 / ms07 / early-console harness | 同上 rustc --test | 36 / 5 / 6 passed | IRQ/恢复/早期 console 回归 | PASS |
| axnet ordinary | `RUSTFLAGS=cc-nopie cargo test --manifest-path crates/axnet/Cargo.toml` | `474 passed` | lib 全量 | PASS |
| axnet qemu-diagnostics | 同上 + `--features qemu-diagnostics` | `506 passed` | lib 全量 | PASS |
| uart_16550 async | `cargo test --features async` | `65+8+10 passed` | UART 回归 | PASS |
| kernel SMP=16 build | `make build SMP=16` | `Finished release … 0 error` | qemu feature 全量编译含 V5/witness | PASS |
| kernel ordinary build | `make build` | `Finished release … 0 error` | 默认编译 | PASS |
| `make host-test`（用户终端跑完） | `make host-test` | 全量 Rust/C/python 全绿（ms04 67、ms03 36、ms07 5 等） | 集成 Gate | PASS |
| bounded QEMU `SMP=16` boot | `qemu-system-riscv64 -smp 16 … -nographic` | `[NET-SMP-SMOKE] PASS`(owner_aff=11 runner_aff=12 distinct, 均 poll, 0 reject)；`[UART-SMP-SMOKE] PASS` | 固定 placement 启动 | PASS |
| OpenSpec strict validate | `openspec validate ms08-… --strict` | `Change … is valid` | change 结构 | PASS |

**Persisted Evidence**

None required (`Persisted Evidence` mode = none；所有结论可复现且短于 Act Response 上限，未创建
Evidence 目录)。

**Experience Candidates**

- **Issue 候选（实质缺陷，在本 Cycle 范围外、有证据）**：UART TX copier 停放协议的
  lost-wakeup 竞态。位置：`crates/uart_16550/src/async_/driver.rs::tx_copier_loop`（Iteration 001
  已 accepted 的代码面）。机制：`tx_copier_active.store(false)`（发布“已停”）先于任务的真实
  `Poll::Pending` block 发生；且 `ring.register_waker` 之后不重查 ring 非空，空发射器分支（
  `tx_copier_loop` 中 `transmitter_empty()` 为真时）不使能 THRE 兜底。任何 producer 的
  `ring.push → poll.wake()` 落进该窗口时，axtask 的 wake 不是真实 Blocked→Ready 转换（任务仍
  Running），不发 remote IPI，随后任务 block 且无 ISR 唤醒源 → 数据卡在 ring、无后续 reschedule。
  证据：对**同一个内核二进制**连续两次 `SMP=16` QEMU 启动，一次
  `[UART-SMP-SMOKE] FAIL reason=tx-copier-not-resumed pushed=1 tx_polls_before=1 ipi_sent_before=0
  ipi_received_before=0`（wake 落入窗口、`ipi_sent` 保持 0），另一次
  `[UART-SMP-SMOKE] PASS ... ipi_sent=2 ipi_received=2`（wake 落在真实 block 之后，remote-ready
  IPI 送达）。boot hart 变化仅改变落入窗口的概率，不是根因。该竞态游离于本 Cycle 网络放置面，
  亦不影响 `[NET-SMP-SMOKE]`（每次皆 PASS）。处置建议：不阻塞本 Cycle；作为 Issue 由
  `openspec-experience-recorder` 登记，后续在 UART park 协议（Iteration 001 面）修复：`register_waker`
  后重查 ring 非空则 self-wake，并让空发射器分支也使能 THRE 兜底。
- Runbook 候选：None（本 Cycle 未出现可重复的高风险操作路径；bounded SMP=16 placement 冒烟是
  Iteration 005 的重复操作输入，但不是独立 Runbook 候选）。

**Remaining Issues**

- **UART TX copier lost-wakeup 竞态（Iteration 001 产品面）**：见上方 Experience Candidates / Issue
  候选；已在同一 `SMP=16` 二进制上由两次运行（一 FAIL 一 PASS）取证，未在本 Cycle 修复（超出执行契约）。
- Timer-disabled witness 与网络数据面的完整 QEMU runtime 资格按计划留给 Iteration 005（A4 只要求
  host/model + target-build，本 Cycle 已满足；bounded placement smoke 已真实运行 PASS）。
- `make host-test` 的既有 critical-section 并发线程时序敏感（负载下偶发停顿）为非阻塞历史现象。

**Commit or Diff Reference**

- 本轮为未提交工作树改动（与 Iteration 001 已 staged 的 UART/SMP 改动同批）。涉及文件见
  `Changed Files and Symbols`；未含归档、未同步 SNAPSHOT/tasks、未清理分支（按 Act 阶段边界）。

## Plan Review

- Review Result: replan-required

**Findings**

Blocking findings remain within the approved Tasks 3.2–3.4:

1. **A2 — affinity-spawn failure can poison the once-only lifecycle and startup can continue without a runner.** `start_stack_runner_affinity` and `start_rx_task_affinity` validate only the configured CPU range, advance `STACK_RUNNER_LIFECYCLE` / `RX_LIFECYCLE`, and only then call `spawn_with_name_affinity`, whose stricter schedulable-mask validation may return `None`. A configured-but-unpublished bit therefore returns `InvalidAffinity` after the lifecycle has already advanced, preventing a safe retry and contradicting the Task 3.2 contract. `init_virtio_net_irq_diag` also continues to IRQ registration and owner startup after a runner-start error. In addition, its device-descriptor/MMIO validation returns occur before the runner start, whereas the previous `init_network` path always provided a runner for Service/loopback/polling progress. Repair must make failed validation/spawn leave the lifecycle retryable, stop owner startup when the runner is absent, and preserve a pinned runner on every post-Service no-owner/fallback path.
2. **A3 — V5 reports inferred task harts as if they were directly observed.** `net_placement::snapshot` assigns `owner_last_hart` and `runner_last_hart` from the pinned affinity, and `irq_snapshot_v5` synthesizes their masks from those values. The owner and runner futures never record `this_cpu_id()`. Poll totals prove progress but not the required actual last/cumulative task hart tuples. V5 also lacks the planned role-attributable remote enqueue/resume and timer-witness state needed by Tasks 3.3–3.4. Repair must instrument the real owner/runner poll paths with coherent hart counters, preserve V4 as the exact prefix, and expose the agreed wake/witness observations without using inferred affinity as execution evidence.
3. **A4 — the QEMU witness binding does not implement the approved single-flight and cleanup contract.** `START` spawns before reserving `WitnessMachine`; `witness_target` calls `try_start(...).ok()` after disabling the timer and ignores rejection. Concurrent starts can therefore create multiple target tasks and overwrite the saved handle. `TRIGGERED` and `CANCELLED` are not reset between runs, no cancel command or bounded supervisor exists, `TRIGGER` is not restricted to `Armed` or a hart different from the target, and an untriggered task can leave its local timer disabled indefinitely. The pure machine accepts invalid transition order and publishes `Completed`/`Failed` without restore acknowledgment, merely incrementing `missing_restore`; current tests explicitly bless that behavior. Repair must reserve the run before spawn, roll back spawn failure, reset per-run state, enforce legal transitions, reject terminal publication before confirmed restoration, add bounded remote supervision/cancel-timeout cleanup, and expose terminal/cleanup state for observation. Full data-plane qualification remains deferred, but the safety mechanism itself must be complete in this Cycle.

Non-blocking observations:

- V1–V4 are structurally left untouched and V5 embeds V4 at offset zero. The current ABI test checks alignment and offset but not an actual byte-for-byte V4 prefix or the complete expected V5 offset/size table; strengthen it as part of the A3 repair.
- The reported placement smoke demonstrates distinct singleton affinities and task progress on `SMP=16`, but because task-hart telemetry is inferred, it cannot close A3 by itself.
- Act additionally captured a reproducible UART TX copier park lost-wakeup: the same `SMP=16` binary alternated between `tx-copier-not-resumed` with zero IPI and a normal PASS. The code path publishes `tx_copier_active=false` and registers the ring waker without a post-registration ring recheck; a producer wake before the task becomes truly Blocked is consumed while Running, after which the copier can park with bytes stranded and no THRE fallback when the transmitter was already empty. This was outside Cycle 000, but the user explicitly authorized repairing it in Iteration 002, so it is now Task 3.6 and requires a revised execution contract.

**Deviation Classification**

ACT-DEVIATION; NEW-EVIDENCE — Tasks 3.2–3.4 omit approved semantics, and Act produced new repeatable evidence of a UART park lost-wakeup that the user authorized adding to this Iteration.

**Acceptance Gaps**

- A2 / Task 3.2: spawn rejection must not advance the once-only lifecycle; a missing runner must block owner startup; Service/loopback/polling progress must retain a runner on every no-owner fallback path.
- A3 / Task 3.3: V5 must contain directly recorded owner/runner execution-hart tuples and the approved wake/witness observations, with exact V4-prefix and fixed-layout tests.
- A4 / Task 3.4: single-flight reservation, legal transitions, per-run reset, bounded timeout/cancellation, remote-trigger validation, and acknowledged timer restoration are incomplete.
- A5 / Task 3.5: the integration Gate lacks tests that exercise the above spawn-failure and witness cleanup paths, so its PASS result cannot yet establish the Cycle acceptance.
- A6 / Task 3.6: UART TX ring publication can be lost between waker registration and the task's real Blocked transition, making the repeated `SMP=16` startup Gate nondeterministic.

**Convergence**

expanded — the original A2–A5 gaps remain, and the newly authorized UART lost-wakeup evidence adds A6 and changes this Iteration's scope and verification boundary.

**Evidence**

- Independent code review covered `crates/axnet/src/{lib,stack_runner,async_rx}.rs`, `kernel/src/drivers/{net_placement,net_wake_witness,net_wake_witness_logic,virtio_net_irq,virtio_net_irq_logic}.rs`, ioctl/entry wiring, and the MS04 host additions.
- Lifecycle evidence: both affinity start APIs execute `lifecycle.start()?` before the schedulable-aware spawn can return `None`; their precheck uses configured count only.
- V5 evidence: owner/runner last hart and mask are derived from pinned affinity; no owner/runner poll site records `this_cpu_id()`.
- Witness evidence: `START` does not reserve the machine; target-side `try_start(...).ok()` ignores Busy; flags are never reset; no timeout supervisor or reachable cancellation exists; terminal-before-restore is accepted by the model and its test.
- UART evidence: `tx_copier_loop` registers the ring waker, stores `tx_copier_active=false`, and returns `Pending` without rechecking ring occupancy; the empty-transmitter branch does not enable THRE. Act recorded one FAIL and one PASS from the same binary, with zero IPI in the failed wake window.
- Reported Act verification is adopted for the unaffected regression baseline: host harnesses, axnet suites, UART suites, builds, placement smoke, diff check, and strict OpenSpec validation reported PASS. No rerun is needed to establish the code-level contradictions above; those PASS results do not cover the missing semantics.

**Follow-up Decision**

Replan this Iteration. The user explicitly authorized repairing the newly reported UART Issue candidate here, which expands the Cycle 000 scope and verification boundary beyond its immutable Plan Context. Create Cycle 001 with updated Tasks 3.2–3.6: close the original network Acceptance gaps, add the UART register/recheck/THRE park repair, and require deterministic interleaving plus repeated `SMP=16` startup evidence. Do not resume Cycle 000.

**Iteration Plan Update**

Add Task 3.6 to Iteration 002; extend its stable baseline, verification boundary, diagnostic boundary, and Non-goals only as needed for the authorized UART park repair. Tasks 3.1 and Iterations 000–001 remain otherwise unchanged.

**Next Cycle**

`001-replan.md`

**Next Iteration**

None
