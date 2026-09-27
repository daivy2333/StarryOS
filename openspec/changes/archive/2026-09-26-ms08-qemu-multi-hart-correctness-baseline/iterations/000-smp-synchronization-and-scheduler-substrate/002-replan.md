# Iteration 000 / Cycle 002: Schedulable run-queue readiness and foundation closure

## Plan Context

- Status: ready
- Iteration: 000-smp-synchronization-and-scheduler-substrate
- Cycle: 002-replan
- Cycle Type: replan
- Parent cycle: `001-replan.md`

**Iteration Scope**

- Change tasks: 1.1–1.7
- Depends on: None
- Stable baseline: kernel critical-section 可安全跨 hart 保护 waker；workspace `axtask` 只选择已发布的 schedulable run queue，并支持入队前 affinity 与 remote-ready IPI；QEMU `SMP=16` 越过 PLIC、primary/secondary scheduler 初始化。UART 和 network driver placement 尚不改变。
- Verification boundary: critical-section、schedulable mask、affinity、IPI 和 config focused tests；UART/axnet 邻接回归；ordinary/`SMP=16` target build；有界 16-hart early runtime。
- Diagnostic boundary: kernel critical-section、run-queue readiness 发布与选择、S_SOFT/IPI、Cargo feature 传播、QEMU PLIC 配置或早期 SMP 启动。
- Deferred tasks: 2.1–7.3

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: A1 未实现 nesting overflow fail-closed；A3 把 configured count 错当 schedulable 集合并可选择未初始化 run queue；Task 1.6 的 `EXTRA_CONFIG` 优先级契约与 `axconfig-gen 0.2.1` 行为冲突；A6 因 early network runner 触发未初始化 run queue page fault。
- Repair items: None
- Inherited scope: Tasks 1.2、1.4、1.5 的实现与未失效验证结论；SMP=16、全局 critical-section、workspace `axtask`、入队前 affinity、remote-ready IPI、单一 S_SOFT owner、QEMU PLIC 完整窗口、单 hart兼容和真板证据边界。
- Excluded scope: UART/network placement 接入、driver diagnostic ABI、task migration、guest probes、数据面压力、network reset/link 交错、D1/K3 真板 bring-up、CPU hotplug、multiqueue/RSS 和性能优化。

**Objective**

关闭 Iteration 000 的基础缺口：critical-section 异常深度 fail closed；scheduler 在 secondary run queue 尚未初始化时只选择已发布的 schedulable queue；QEMU PLIC 配置契约与实际生成器语义一致；`SMP=16` 能完成 primary/secondary scheduler 早期初始化而不提前修改 driver placement。

**Background**

Cycle 001 已完成大部分基础实现并修复 PLIC 映射，但 16-hart runtime 在 `init_network` 启动 stack runner 时选择了未初始化的 secondary run queue。Review 发现根因不只是 driver 启动时序：`validate_affinity`、`cpu_mask_full` 和 `select_run_queue` 都用 `axhal::cpu_num()` 代表可调度集合，而该值在 secondary scheduler 初始化前已经是 16。Cycle 001 还遗漏了 nesting depth overflow 检查，并把配置生成器不支持的 `EXTRA_CONFIG` 覆盖语义写成兼容性要求。

**Investigation Facts**

- Current Baseline:
  - 工作树位于 `mul-hart-k3`，包含 Cycle 001 未提交实现；本 Cycle 不回退这些修改。
  - Cycle 001 Act Response 的 Tasks 1.2、1.4、1.5 验证结论可采信：root 与 standalone axnet 解析同一 workspace `axtask`；remote/local/duplicate wake focused tests 通过；SMP feature graph 只有 `axtask/ipi` 拥有 S_SOFT。
  - Review 新鲜运行 `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms08-review-host-test && /tmp/ms08-review-host-test`：22 passed，exit 0。该结果证明现有 happy path，但没有覆盖 depth overflow；`cross_hart_second_acquire_waits_for_first_release` 的标志发布位于 global-lock release 之后，存在错误失败窗口。
  - Cycle 001 的 `make build`、`make build SMP=16`、D1 build、metadata、riscv axnet check 与 PLIC 生成值结论未因后续产品修改失效，可作为当前基线。16-hart runtime 的 page fault 由源码调用顺序与 Act 输出共同支持。
- Current-State Evidence:
  - `critical_section_policy::acquire` 使用 `fetch_add(1)`；`u32::MAX` 会回绕，未满足 design D3 和 Task 1.1 的 overflow fail-closed。
  - `axtask::validate_affinity(mask, axhal::cpu_num())` 只检查最高 bit 小于 configured count；`cpu_mask_full()` 也由 configured count 构造。两者没有 run queue readiness 状态。
  - `run_queue::init` 与 `init_secondary` 先写 `RUN_QUEUES[cpu_id]`，但没有在写入后发布 schedulable bit；`select_run_queue` 直接对 affinity 做 round-robin，再对所选槽调用 `assume_init_mut()`。
  - `axruntime::rust_main` 在 `axtask::init_scheduler()` 后调用 `axnet_ng::init_network()`，最后才调用 `start_secondary_cpus()`。`init_network → start_stack_runner → spawn_with_name → select_run_queue` 因此可在只有 primary queue 初始化时选择 secondary。
  - `axconfig-gen 0.2.1` 先对全部 specification 调用 `Config::merge`，重复 key 直接报错；随后统一应用所有 `-w`。把 `QEMU_OVERLAY_ARG` 写在 `EXTRA_CONFIG` 参数之后或之前都不会让 specification 获得最终优先级。
  - `make/config.mk` 的 QEMU `-w` 已修复最终 PLIC 值并保持非 QEMU 构建不受影响，但当前 source guard 只检查字符串和平台 guard，没有检查重复 key 拒绝或配置契约文字。
- Code and Critical Path:
  - Critical section：`critical_section::Impl → critical_section_policy::{acquire,release} → GLOBAL_LOCK/NEST_DEPTH → axhal IRQ restore`。
  - Scheduler readiness：`run_queue::{init,init_secondary} → RUN_QUEUES[cpu] write → schedulable mask publish → api::{spawn_task,spawn_task_with_affinity} → select_run_queue → get_run_queue`。
  - Early boot：`axruntime::rust_main → axtask::init_scheduler → axnet_ng::init_network/start_stack_runner → plain spawn → start_secondary_cpus → axtask::init_scheduler_secondary`。
  - Config：`make/config.mk → axconfig-gen specification merge → -w updates → .axconfig.toml → kernel MMIO map → PLIC init_percpu`。

**Implementation Guidance**

先修正 tests：加入可注入的小深度 counter seam 或等价 checked-increment witness，并让 release-order test 在持锁期间发布明确状态，避免把调度窗口当作互斥失败。随后在 `axtask` 内由 run queue owner 发布 readiness：写入 `RUN_QUEUES[cpu]` 后 Release 设置对应 bit，选择与显式 affinity 校验时 Acquire 读取。普通 task 保留 configured full affinity，但每次选择使用 `affinity ∩ schedulable`；显式 spawn/update 要求整个 mask 已 schedulable。最后修正 config guard 和文档契约，再运行 16-hart early runtime。不要通过提前实施 UART/network placement 绕过 scheduler 缺口。

**Behavioral Change**

- critical-section nesting increment 在提交前检查 overflow；异常时 IRQ 保持关闭，不修改 depth 或 global owner。
- `axtask` 区分 configured CPU mask 与已初始化的 schedulable run-queue mask。普通 spawn 保留 full configured affinity，但只能从两者交集选择；显式 affinity spawn/update 拒绝未初始化候选。
- primary/secondary run queue 只在槽写入完成后发布 schedulable；Acquire 观察该 bit 后才允许解引用对应槽。
- QEMU PLIC 完整窗口继续由最终 `-w` 修正。`EXTRA_CONFIG` 保持非重复 specification 合并能力，不再声称可覆盖已有 `devices.mmio-ranges`。

**Task Contracts**

### 1.1: Close critical-section abnormal-depth semantics

- Requirement/Scenario: kernel critical-section 全局互斥；ISR/task 嵌套；depth 异常 fail closed。
- Depends on: None
- Targets: `kernel/src/critical_section_policy.rs::{acquire,release}`；`tests/ms04-async-rx-host-harness.rs`。
- Current behavior: 正常嵌套与跨 hart 互斥通过；increment 使用 wrapping `fetch_add`；一个 release-order witness 在释放锁后才发布完成标志。
- Required behavior: underflow、overflow、越界 hart 和非 owner release 都不得修改他人 ownership；测试不依赖 unlock 后的线程调度顺序。
- Required changes: 使用 checked atomic increment 或等价不回绕状态转换；补 overflow witness；把互斥 witness 的因果判断放在锁保护或明确的发布顺序内。
- Preserve: `restore-state-bool` ABI、Acquire/Release global lock、per-hart nesting、ISR `release(false)`、生产 glue 委托。
- Forbidden: 扩大 counter 规避 overflow；在临界区阻塞/yield；把测试 sleep 当同步协议；修改 driver waker API。
- Test witness: 变更前新增 overflow case 应失败；现有 22-test harness 为 GREEN 基线。命令：`rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms08-critical-review && /tmp/ms08-critical-review`。
- GREEN condition: 正常、并发、underflow、overflow、越界和 release-order cases 全绿，最大并发 owner 为 1。
- Verification: focused harness、`make host-test` 可执行部分、ordinary/`SMP=16` build。
- Stop when: 保留 `restore-state-bool` 无法表达匹配 release，或修正需要在 critical-section 内阻塞。

### 1.3: Publish and enforce schedulable run-queue readiness

- Requirement/Scenario: configured、online、schedulable 分离；early ordinary spawn；显式无效 affinity fail closed。
- Depends on: 1.2
- Targets: `crates/axtask/src/run_queue.rs::{init,init_secondary,select_run_queue_index,select_run_queue,get_run_queue}`；`api.rs::{cpu_mask_full,validate_affinity,spawn_task_with_affinity}`；`task.rs::set_cpumask_checked`；crate tests。
- Current behavior: configured `cpu_num()` 被当作 schedulable count；selection 可对未写入的 `RUN_QUEUES` 调用 `assume_init_mut()`。
- Required behavior: run queue 写入完成后才能发布对应 schedulable bit；普通 spawn 只从 `task affinity ∩ schedulable` 选择且不缩小 task 保存的 full affinity；显式 spawn/update 仅接受 schedulable 子集。空交集 fail closed，不得访问未初始化槽。
- Required changes: 在 `axtask` 内新增最小 readiness state 和 host-testable selection/validation seam；以 Release 发布、Acquire 观察；选择前验证交集非空和所选 bit 已发布。
- Preserve: 旧 plain spawn API、task identity、round-robin 规则、后续自然迁移能力、driver placement 延期边界。
- Forbidden: 把 `cpu_num()` 重命名后继续充当 readiness；提前修改 network/UART startup；把 early task 永久改写为 primary-only affinity；读取 `MaybeUninit` 判断是否初始化。
- Test witness: configured=16 且仅 boot hart schedulable 时，现有选择逻辑应暴露未初始化候选；新增 pure/model tests 覆盖逐 hart 发布、稀疏集合、空交集、显式未初始化 mask、失败保留旧 mask和 Acquire/Release publication。
- GREEN condition: 任何 selection 返回的 CPU 都在 schedulable snapshot 中；plain task 保留 full affinity；显式无效 mask 稳定失败；secondary 发布后可成为后续选择候选。
- Verification: axtask focused tests；root/standalone metadata；ordinary/`SMP=16` build；16-hart early runtime 不再在 `RRScheduler::lock` 访问零地址。
- Stop when: run queue 初始化所有权不在 `axtask` 内，或正确发布需要修改 `axruntime`/driver placement 契约。

### 1.6: Align QEMU PLIC correction with generator semantics

- Requirement/Scenario: SMP=16 PLIC 映射；非 QEMU 兼容；配置接口保持真实可验证。
- Depends on: None
- Targets: `make/config.mk`；`tests/ms04-async-rx-host-harness.rs` config guard；change design/tasks。
- Current behavior: QEMU-only final `-w` 产生正确 `0x60_0000`；文档和旧 Task Contract 错误声称 `EXTRA_CONFIG` 可在其后覆盖。
- Required behavior: QEMU 最终 config 保留完整 MMIO 列表和 `0x60_0000` PLIC；非 QEMU 无该写入；`EXTRA_CONFIG` 继续合并非重复项，重复 `devices.mmio-ranges` 明确失败，不建立第二套 override 接口。
- Required changes: 修正文档和 source/config guard；若现有 `make/config.mk` 已满足产品行为，不为形式一致重写控制流。
- Preserve: D1/VF2 配置、registry 只读、用户非重复 extra specification、PLIC 修复。
- Forbidden: 声称 CLI 参数顺序能改变 `axconfig-gen` 的 spec-before-write 阶段；新增配置预处理器、专用 manifest 或第二套 override CLI；把 QEMU 修正外推到真板。
- Test witness: source/model test直接读取 `axconfig-gen` 阶段或用最小重复-key fixture证明 duplicate rejection；生成 QEMU 和 D1 config 比较目标项。
- GREEN condition: 文档、guard 和实际生成行为一致；QEMU PLIC 为 `0x60_0000`；非 QEMU 无该写入；重复 key 失败可判定。
- Verification: focused config guard、`make defconfig SMP=16` 最终值、D1 config/build、`git diff --check`。
- Stop when: 用户要求 `EXTRA_CONFIG` 覆盖已有 key；这需要新的配置接口设计并返回 Plan。

### 1.7: Re-run foundation integration Gate

- Requirement/Scenario: MS08 分层 Gate；16-hart early runtime；UART/network 邻接回归。
- Depends on: 1.1、1.3、1.6；继承已完成 1.2、1.4、1.5。
- Targets: Iteration 000 全部产品 diff、tests 和既有验证入口。
- Current behavior: PLIC 与 primary scheduler 已通过；network early plain spawn 选择未初始化 secondary queue 后 page fault。
- Required behavior: 16-hart runtime 完成 PLIC、primary scheduler、early network spawn 和全部 secondary scheduler 初始化，无 page fault、panic、handler conflict 或 uninitialized run queue；driver placement 仍未接入。
- Required changes: 只修复 1.1/1.3/1.6 契约内问题并运行 Gate。
- Preserve: Tasks 1.2/1.4/1.5 实现、early console、UART/D1 workaround、network owner/recovery 语义和用户工作树其他修改。
- Forbidden: 降低到 `SMP=8`；关闭 network；提前实施 Tasks 2.x/3.x；以 timeout exit 124 单独判 PASS；修改 registry。
- Test witness: Cycle 001 blocker 是 RED；修正后同一 `timeout 40s make justrun SMP=16 NET=n` 必须越过原 fault 点和 secondary scheduler init。
- GREEN condition: 所有非环境 Gate 通过；必需启动 marker 出现且禁止 marker 不出现。
- Verification: critical-section/axtask/config focused tests；UART tests；axnet 可执行回归与 riscv check；`make host-test` 可执行部分；ordinary/`SMP=16`/D1 build；有界 runtime；strict OpenSpec validation；full diff review。
- Stop when: runtime 仍需 driver placement、diagnostic ABI或数据面修改，或同一 readiness 设计连续三次失败。

**Invariants**

- UART 与 network 的 task 数量、SPSC/queue owner 身份和启动 adapter 在本 Cycle 不变。
- `RUN_QUEUES[cpu]` 写入先于 schedulable bit Release；selection 的 Acquire 观察先于槽解引用。
- plain task affinity 与当次可选 run queue 集合分离；readiness 过滤不得永久缩小 task affinity。
- remote IPI 只由成功状态转换触发；telemetry 不参与同步决定。
- Cargo registry 保持只读；S_SOFT 只有一个 handler owner。
- QEMU 结果不证明 D1/K3 真板或物理 SMP ordering。

**Non-goals**

- UART/network 固定 placement、secondary-ready driver startup、diagnostic ABI和 task handle 保存。
- timer-disabled witness、task migration、guest/host qualification protocol和数据面 runtime。
- CPU hotplug、动态 offline、通用 topology framework、multiqueue/RSS、性能和真板 bring-up。
- 为 `EXTRA_CONFIG` 新增覆盖已有 key 的配置机制。

**Acceptance**

- A1 / D3 / Task 1.1：跨 hart owner 最大并发为 1；正常嵌套与 IRQ restore 正确；underflow、overflow、越界和非 owner release fail closed；见证没有 unlock 后竞态。
- A2 / D1 / Task 1.2：root 与 standalone axnet 继续解析同一 exact workspace `axtask`；版本/license/API 基线保持。
- A3 / D1 / Task 1.3：run queue 写入后才发布 schedulable；configured=16/仅 primary ready 时 plain spawn 只选择 primary且保留 full affinity；显式未初始化 mask 失败；发布 secondary 后可用于后续选择。
- A4 / D2 / Tasks 1.4–1.5：remote successful unblock 恰好一次 IPI，本地 wake 只 pending，重复 wake 不通知，S_SOFT 单 owner。
- A5 / D0 / Task 1.6：QEMU 最终 PLIC 窗口为 `0x60_0000`，其他 MMIO 与非 QEMU 平台保持；配置文档与 spec-merge/`-w` 实际阶段一致。
- A6 / Task 1.7：focused、UART、axnet、host 可执行部分、ordinary/`SMP=16`/D1 build 和 16-hart early runtime 无产品失败；越过所有 secondary scheduler init；driver placement 未修改。

**Verification**

- critical-section host harness：直接观察互斥、嵌套、IRQ restore 与异常深度。
- axtask pure/model tests：直接观察 readiness publication、selection intersection、plain affinity 保留、显式 mask rejection 与 IPI 因果。
- root/standalone metadata 和 feature tree：确认 workspace patch 与 S_SOFT owner 未变化。
- config focused fixture、QEMU final config 与 D1 config/build：确认生成器阶段、PLIC 值和平台边界。
- UART crate tests、axnet 可执行回归/riscv check、`make host-test` 可执行部分：邻接回归。
- `make build SMP=16` 与有界 `make justrun SMP=16 NET=n`：必须出现 16 hart、primary scheduler、network init 和 secondary init 进度，不得出现 page fault、panic、handler conflict 或 uninitialized run queue。
- `git diff --check`、`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` 和 full diff review。

**Gate 2 Readiness**

- No Missing requirements: PASS — Cycle 001 的四个 Acceptance gap 已映射到 1.1、1.3、1.6、1.7。
- Simplified requirements approved: PASS — 用户于 2026-09-17 以“批准”明确接受：PLIC 平台修正最终生效，`EXTRA_CONFIG` 保留非重复项合并能力，但不覆盖已有 `devices.mmio-ranges`。
- Investigation complete: PASS — 已定位 depth increment、run queue 写入/选择、axruntime 启动顺序、network plain spawn 和 axconfig-gen merge/write 阶段。
- Design closed: PASS — readiness owner、发布顺序、plain/explicit affinity差异、配置边界与禁止方案明确。
- Tasks executable: PASS — 1.1、1.3、1.6、1.7 均有 targets、witness、GREEN、verification 和 stop condition；1.2、1.4、1.5 继承未失效结论。
- Iteration plan ordered and balanced: PASS — Iteration 000 仍只交付共享基础；driver placement 继续留在 001/002，后续 Map 不变。
- Traceability complete: PASS — requirement、scenario、design、task、代码面和测试见证已形成链路。
- Verification sufficient: PASS — host 并发、scheduler model、config fixture、target build 和 16-hart runtime 分别直接观察目标。
- No identity-style evidence engineering: PASS — 没有 revision/run/session 协议或证据身份机制。
- No material TBD for Act: PASS — 配置兼容性修订已获批准，没有留给 Act 的契约选择。
- Change/Cycle consistency: PASS — delta spec、design、tasks 与本 Cycle 使用同一 schedulable readiness 语义；Cycle 001 保持冻结。
- Persisted Evidence mode valid: PASS — Mode 为 `none`，所有检查可低成本重跑并由 Act Response 承载。
- User plan approval: PASS — 用户于 2026-09-17 以“批准”明确批准 Cycle 002 执行契约及配置兼容性修订。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、修改文件和符号写入 Act Response；不创建 Evidence 目录。

**Risks and Notes**

- run queue readiness 是单向启动状态，不包含 CPU hotplug/offline；后者仍是 Non-goal。
- source guard 不能代替真实 16-hart runtime；runtime timeout 124 只有在必需 marker 完整且禁止 marker 缺失时才可能是健康等待。
- Cycle 001 的 axnet host-link 限制仍存在；使用 riscv target check、root product build 和可执行 host gates，不把未运行的 host lib tests 标记为 PASS。
- 若用户要求 `EXTRA_CONFIG` 覆盖已有 key，本 Cycle 保持 draft，并返回 Plan 设计新的通用配置接口。

## Act Response

- Status: reported

**Implemented**

Cycle 002 closes the four Cycle 001 acceptance gaps (A1/A3/config-contract/A6) and re-runs the foundation Gate:

- **1.1 critical-section abnormal-depth fail-closed**: replaced the wrapping `fetch_add(1)` with a checked `fetch_update` keyed to the *real* `u32` counter width. The transition is delegated to a pure seam `checked_increment_depth` (`depth.checked_add(1)`), so a nest that would overflow at `u32::MAX` returns `None`, `fetch_update` leaves the counter and the global owner untouched, and IRQs stay disabled on the failing hart. No artificial `MAX_NEST_DEPTH` bound restricts legitimate production nesting; overflow is witnessed by direct seam-boundary and `fetch_update`-preservation model tests rather than a 4-billion-level drive. Corrected the racy `cross_hart_second_acquire_waits_for_first_release` witness to publish/clear the occupant *inside* the critical section (no post-unlock window).
- **1.3 run-queue schedulable readiness**: added a published `SCHEDULABLE` bitset in `run_queue.rs`; `init()`/`init_secondary()` `mark_schedulable(cpu)` (Release) only after writing `RUN_QUEUES[cpu]`; `schedulable()` reads with Acquire; `select_run_queue_index` (SMP) now selects from `task affinity ∩ schedulable` via the pure `select_schedulable_cpu` seam and fails closed on an empty intersection (no more `assume_init_mut` on an unwritten slot). Plain spawn keeps full affinity; explicit spawn/update (`spawn_task_with_affinity`, `set_cpumask_checked`) use new `validate_schedulable` which rejects any bit not yet schedulable (including out-of-range bits above `MAX_CPU_NUM`). A build-time assert caps `MAX_CPU_NUM <= 64` (the usize-width bitset).
- **1.6 config contract alignment**: confirmed `make/config.mk` already fixes PLIC via the last-applied `-w`; added a deterministic guard `config_extra_duplicate_mmio_key_is_rejected` proving `axconfig-gen` rejects a second spec defining `devices.mmio-ranges` at merge time ("Duplicate key", nonzero exit, no output), so `EXTRA_CONFIG` is documented/guarded as non-duplicate-merge only. Verified QEMU final config carries `[0x0c00_0000, 0x60_0000]` and D1 config/build carry no overlay.
- **1.7 foundation integration Gate**: all focused tests, UART regression, axnet riscv check, `make host-test`, ordinary/SMP=16/D1 builds, and both 16-hart early runtimes (NET=n and NET=y) green with zero forbidden markers; `SMP=16` boots all 16 harts, registers the network IRQ, starts the async RX queue task during early boot, and reaches the interactive shell.
- **Audit rework (final)**: independent audit of the Cycle 002 diff surfaced and fixed two in-contract issues — (a) `select_schedulable_cpu` now reduces `seed % n` before probing so a `usize`-wide seed near `usize::MAX` cannot wrap a non-power-of-two modulus during round-robin (the policy must support the `1/2/3/4/8/16` sparse sets, which include 3/5), with a wrapped-seed model test; (b) the overflow witness dropped its post-release `assert!(!is_global_locked())`, which was schedule-dependent under parallel test threads (another test may hold the lock) — the leak-free guarantee is covered by `depth(cpu)==0`, so the witness is now deterministic.
- **Plan Review follow-up (this pass, 002 post-Review)**: resolved the two open review findings and re-ran the serial gates. (1) A1 — removed the production `MAX_NEST_DEPTH = 64` cap; `acquire` now rejects only at the true `u32` overflow via the pure `checked_increment_depth` seam, and the harness witnesses the overflow boundary two ways: the seam itself (`u32::MAX - 1 → Some(MAX)`, `u32::MAX → None`) and the exact `fetch_update` mutation `acquire` performs leaving a full `u32` counter untouched on rejection. The now-orphaned `nest_depth`/`is_global_locked` observation seams were removed. (2) A3 — replaced `schedulable_published_after_mark_and_selected` (which called the real `mark_schedulable(15)` on an uninitialized `RUN_QUEUES[15]`, fabricating a published-but-uninitialized global) with a pure mask model `schedulable_publish_model_flips_only_target_bit` that builds the before/after snapshots as ordinary masks and never touches the real `SCHEDULABLE`, preserving the `write slot → Release publish → Acquire observe → select` invariant. (3) Re-ran serial critical-section harness, axtask `multitask,test,smp` suite, ordinary/SMP=16 builds, `git diff --check` and strict validation; the un-failed 16-hart early-boot evidence is reused as the Review directed.

**Changed Files and Symbols**

- `kernel/src/critical_section_policy.rs`: pure `checked_increment_depth` seam, checked `acquire` increment at the true `u32` width; removed `MAX_NEST_DEPTH` and the orphaned `nest_depth`/`is_global_locked` seams.
- `crates/axtask/src/run_queue.rs`: `SCHEDULABLE`, `mark_schedulable`, `schedulable()`, pure `select_schedulable_cpu` (+ `seed % n` bound), `select_run_queue_index` intersection, `mark_schedulable` in `init`/`init_secondary`.
- `crates/axtask/src/api.rs`: `validate_schedulable`, `spawn_task_with_affinity` now validates schedulable.
- `crates/axtask/src/task.rs`: `set_cpumask_checked` now validates schedulable.
- `crates/axtask/src/tests.rs`: 7 tests (6 schedulable-readiness/selection/publish-model/preserve + wrapped-seed); `schedulable_published_after_mark_and_selected` rewritten as the pure `schedulable_publish_model_flips_only_target_bit`.
- `tests/ms04-async-rx-host-harness.rs`: import seam; pure `checked_increment_depth` boundary witness and `fetch_update` overflow-preservation witness (replacing the 64-level depth-cap drive); rewritten cross-hart witness (lock-protected publish); `config_extra_duplicate_mmio_key_is_rejected` guard; removed `is_global_locked`/`nest_depth` imports.

**Deviations from Plan**

- Explicit-affinity validation additionally bounds each mask by `MAX_CPU_NUM` (rejects set bits at/above the configured count) so an index beyond the readiness loop can never slip through; this is a fail-closed superset of "every bit schedulable".
- The readiness bitset is one `usize` wide with a compile-time `MAX_CPU_NUM <= 64` assert; every current product target (QEMU `SMP=16`, D1, VF2; all 64-bit) fits. A wider CPU count is a build error, not a silent truncation.
- `select_schedulable_cpu` bounds the seed (`seed % n`) before probing so a wrapped `usize` seed cannot skew a non-power-of-two round-robin; behavior is identical for the current power-of-two `MAX_CPU_NUM` (16/64) but is load-bearing for the `1/3/5/...` sparse sets the policy documents.
- Post-Review, production nesting no longer uses any artificial depth cap: `acquire` rejects only at the real `u32` overflow. The old swept-depth witness is replaced by two model witnesses that are reachable at the true width (seam boundary + `fetch_update` non-store), so no 4-billion-level drive is needed and no artificial cap restricts legitimate nesting.
- Post-Review, the run-queue publication witness (`schedulable_publish_model_flips_only_target_bit`) is a pure mask model. It no longer calls the real global `mark_schedulable`/`schedulable`, so it cannot fabricate a "published but uninitialized" run queue; the real global publication path is witnessed by `explicit_affinity_rejects_unpublished_cpu_and_preserves_old_mask` (rejects an unpublished bit) and by the validated SMP=16 early-boot evidence (init publishes only written slots).
- Host axtask integration test `spawn_with_affinity_runs_when_mask_valid` aborts under the `ipi`/`preempt` feature on the fake host platform — the documented Cycle 001 host-fake limitation, not a regression — so it is validated under `multitask,test` (where the FIFO scheduler runs); `spawn_with_affinity_rejects_invalid_mask` validates under `ipi`. The host config needs `max-cpu-num >= 64` so `one_shot(63)` is constructible.
- axnet host lib-test cannot link an x86 binary against RISC-V `axplat`/`percpu` relocations (pre-existing); axnet regression is covered by the riscv-target `cargo check` and the ordinary/SMP=16 root product builds, which all pass.

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: PASS
- Full diff reviewed: PASS (all product/test files reviewed end-to-end, including the post-Review A1/A3 fixes; no identity-type evidence engineering; no out-of-scope edits)
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0 — both open Plan Review findings (production depth cap, fabricated publication witness) are closed; the audit-driven witness-reliability and wrapped-seed issues remain fixed.

**Verification Evidence**

| Verification item | Command / operation | Decisive output (excerpt) | Coverage | Conclusion |
|---|---|---|---|---|
| critical-section + config guard | `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms08-critical-review && /tmp/ms08-critical-review --test-threads=1` | `test result: ok. 25 passed; 0 failed` | overflow seam at `u32::MAX`, underflow/range fail-closed, IRQ restore, cross-hart mutual exclusion, duplicate-key config guard | PASS |
| seam overflow model | fresh serial harness (above) | `checked_increment_depth_fails_closed_at_u32_overflow ok`; `fetch_update_with_seam_preserves_counter_on_overflow ok` | counter rejects only at true `u32` overflow; `fetch_update` non-store on rejection | PASS |
| schedulable-readiness model | `AX_CONFIG_PATH=/tmp/axtask-test-config.toml cargo test --manifest-path crates/axtask/Cargo.toml --features multitask,test select/published/explicit_affinity` | `select 5 passed` (incl. wrapped-seed), `published 3 passed`, `explicit_affinity 1 passed` | intersection ∩ schedulable, empty-intersection fail-closed, pure publish-model transition, old-mask preserved, wrapped-seed determinism | PASS |
| axtask suite (post-Review) | `AX_CONFIG_PATH=... cargo test --manifest-path crates/axtask/Cargo.toml --features multitask,test,smp -- --test-threads=1` | `16 passed; 0 failed` + `1 doctest passed` | schedulable-publish model, selection, affinity validation, FIFO/join/wait-queue regression | PASS |
| affinity in practice | `... test ... spawn_with` | `rejects_invalid_mask ok` (under `ipi`); `runs_when_mask_valid ok` (under `multitask,test`) | pre-enqueue affinity with schedulable validation | PASS |
| legacy validate_affinity | `... test validate_affinity` | `4 passed` | pure configured-range predicate unchanged | PASS |
| UART regression | `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` | `62 passed; 8 passed; 10 passed` (exit 0) | baseline preserved | PASS |
| axnet riscv check | `cargo check --manifest-path crates/axnet/Cargo.toml --target riscv64gc-unknown-none-elf` | `Finished dev ... (exit 0)` | vendored axtask usable by axnet | PASS |
| root product build | `make build`, `make build SMP=16`, D1 `make build` | `Finished release ... (exit 0)` ×3 (SMP=16 rebuilt after `seed % n` fix and after seam removal) | critical-section + axtask readiness integration; non-QEMU unaffected | PASS |
| host-test | `make host-test` | ms04 harness `24->25 passed`; ms03/ms04/ms05/ms06 probe suites PASS (exit 0) | full host gate | PASS |
| post-Review SMP=16 build | `make build SMP=16` (fresh, after seam removal) | `Finished release ... (exit 0)` | seam removal + axtask readiness still integrate | PASS |
| whitespace (post-Review) | `git diff --check`, `git diff --cached --check` | clean (exit 0) | no trailing/whitespace errors | PASS |
| 16-hart runtime (NET=n) | `timeout 60 make justrun SMP=16 NET=n` *(reused per Review)* | 16 HARTs; reaches shell; 0 forbidden markers; exit 124 | PLIC + scheduler + full boot | PASS |
| 16-hart runtime (NET=y, A6 repro) | `timeout 90 make justrun SMP=16` *(reused per Review)* | 16 HARTs; network IRQ registered; async RX task starts early; reaches shell; 0 forbidden markers; exit 124 | early network spawn before secondaries selects a schedulable run queue | PASS |
| OpenSpec | `openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` | `Change ... is valid` | change structure self-check | PASS |

**Persisted Evidence**

None required (Mode `none`; all checks cheaply reproducible, decisive outputs captured in this Response).

**Experience Candidates**

None — this Cycle's 16-hart early-boot and schedulable-readiness changes were validated to a shell prompt but do not constitute an end-to-end repeatable node Oper path beyond the existing gates, and no out-of-change defect with fresh evidence was surfaced.

**Remaining Issues**

None blocking. The two Plan Review acceptance gaps are resolved on the current code/diff:
- A1: production nesting now rejects only at the true `u32` overflow via the pure `checked_increment_depth` seam; no artificial depth cap remains, and the seam/`fetch_update`-preservation witnesses close the reachable-overflow gap.
- A3: the publication witness is a pure mask model and no longer publishes readiness on an uninitialized `RUN_QUEUES[15]`; the production write→Release→Acquire→dereference invariant holds.
Iteration 000 tasks 1.1–1.7 are complete and Acceptance A1–A6 are met on the current code/diff.

**Commit or Diff Reference**

Uncommitted working tree on `mul-hart-k3` (Cycle 001 implementation + this Cycle's changes staged/unstaged as described). Files changed by this cycle listed under **Changed Files and Symbols**.

## Plan Review

- Review Result: accepted

**Findings**

- Accepted — `critical_section_policy::acquire` 已移除人工 64 层上限，通过 `checked_increment_depth` 和 `fetch_update` 在真实 `u32` 边界拒绝且不写回。纯 seam boundary 与 atomic non-store witnesses 直接覆盖 overflow fail-closed。
- Accepted — readiness publication witness 已改为纯 `AxCpuMask` model，不再调用真实 `mark_schedulable(15)`，不会制造“已发布但未初始化”的全局状态。生产路径仍保持 slot write、Release publish、Acquire observe 后才选择。
- Accepted — round-robin seed先归一化再探测；接近 `usize::MAX` 的witness通过。
- Accepted with user decision — 2026-09-17 用户明确接受critical-section harness串行通过作为本Cycle证据，不要求默认test runner并行稳定。Review新鲜串行复验25/25 PASS；并行runner的共享fixture干扰风险不阻塞A1/A6。
- Accepted — 配置、UART/axnet邻接、ordinary/`SMP=16`/D1 build和16-hart early runtime证据未失效；用户与前次Review均确认`SMP=16`到达shell且无原page fault。

**Deviation Classification**

None — 最新Act Response落实了上一版Review的两项剩余修复，没有新增阻塞偏差。

**Acceptance Gaps**

None — A1–A6均满足。

**Convergence**

Reduced to zero — 人工nesting上限和publication witness污染均已移除；上一版剩余Acceptance gaps全部关闭。

**Evidence**

- 采信最新Act Response中未失效的UART/axnet回归、ordinary/`SMP=16`/D1 builds与16-hart runtime证据；用户手动复验`make justrun SMP=16`正常。
- Review：`rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms08-review4-host && /tmp/ms08-review4-host --test-threads=1`，25/25 PASS。
- Review：`AX_CONFIG_PATH=/tmp/axtask-test-config.toml cargo test --manifest-path crates/axtask/Cargo.toml --features multitask,test,smp -- --test-threads=1`，16 unit + 1 doctest PASS。
- Review代码检查：无`MAX_NEST_DEPTH`；`checked_increment_depth`使用`u32::checked_add`；publication test只操作局部mask。
- Review：`git diff --check && git diff --cached --check` PASS。
- Review：`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` PASS。

**Follow-up Decision**

接受Cycle 002并完成Iteration 000。按既有Iteration Map展开Iteration 001 Cycle 000；其Plan Context保持`draft`，等待用户批准Gate 2后才能执行。

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

`../001-deterministic-uart-placement-and-observability/000-initial.md`
