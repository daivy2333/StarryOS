# Iteration 000 / Cycle 001: Shared SMP substrate replan for async I/O

## Plan Context

- Status: draft
- Iteration: 000-smp-synchronization-and-scheduler-substrate
- Cycle: 001-replan
- Cycle Type: replan
- Parent cycle: `000-initial.md`

**Iteration Scope**

- Change tasks: 1.1–1.7
- Depends on: None
- Stable baseline: kernel critical-section 可安全跨 hart 保护 waker；workspace `axtask` 支持入队前 affinity 和 remote-ready IPI；QEMU `SMP=16` 可越过 PLIC 与 scheduler 早期初始化。UART 和 network 驱动 placement 尚不改变。
- Verification boundary: critical-section/axtask/config focused tests、UART 与 axnet 回归、ordinary/`SMP=16` target build 及有界 16-hart 早期启动通过。
- Diagnostic boundary: kernel critical-section、task 构造/run queue、S_SOFT 注册、IPI 发送/接收、Cargo feature 传播或 QEMU PLIC 映射。
- Deferred tasks: 2.1–7.3

**Cycle Scope**

- Trigger: replan-required
- Acceptance gaps: 父 Cycle 没有覆盖用户新批准的 UART 多 hart 范围，其 network-only Iteration Map、RTM 和验证边界已失效。
- Repair items: None
- Inherited scope: `SMP=16`、动态 online/schedulable 集合、全局 critical-section、入队前 affinity、remote-ready IPI、QEMU PLIC overlay、单 hart 兼容和真板证据边界继续有效。
- Excluded scope: UART/network placement 接入、diagnostic ABI、copier/owner/runner migration、guest probes、数据面压力、network reset/link 交错、D1/K3 真板 bring-up、CPU hotplug、multiqueue/RSS 和性能优化。

**Objective**

在不改变 UART 和网络当前 placement 的前提下，建立两个驱动共同依赖的 SMP 同步与调度基线，并让 QEMU `SMP=16` 稳定越过早期 PLIC/scheduler 初始化边界。

**Background**

用户在父 Cycle 交付后明确要求将既有异步 UART 与网卡一起纳入多核适配，并批准重写计划。范围变化不改变本 Iteration 的基础依赖：UART 和 network 都需要跨 hart critical-section、入队前 affinity 和事件直接触发的 remote scheduling。父 Cycle 的 Plan Context 保持不变，本 replan Cycle 取代它成为执行候选。

**Investigation Facts**

- Current Baseline:
  - Branch `k3`，revision `04fc3ce5101ebf6a2e30b91e7fc6512c2a17f8a3`。工作区已有用户修改：`.claude/docs/SNAPSHOT.md`、`.claude/docs/tasks.md`、`openspec/specs/references/spec.md`；Act 不得触碰或回退。
  - `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` 新鲜通过：62 unit、8 regular doc-tests、10 compile-fail doc-tests，exit 0。
  - 既有 axnet ordinary `474/474` 和 qemu-diagnostics `506/506` 基线通过；`make build SMP=16` 已能产生 binary。
  - 父计划的 16-hart 启动 witness：OpenSBI 枚举 hart 0–15 并选择非零 boot hart 11；现有 `0x21_0000` PLIC 映射不足以覆盖高编号 supervisor contexts，原始 page fault 后被 scheduler 未初始化的 `current task is uninitialized` panic 掩盖。
  - `make host-test` 在相关 Rust/C/Python tests 通过后可在 UDP loopback socket 创建处因沙箱 `EPERM` 停止；该边界不是产品失败，但不可宣称后续 socket runtime 已执行。
- Current-State Evidence:
  - `kernel/src/critical_section_policy.rs::{IrqOps,acquire,release}` 只保存/关闭本 hart IRQ，没有全局 owner 和 per-hart nesting。`kernel/src/lib.rs::critical_impl` 委托该 policy。
  - `embassy_sync::waitqueue::AtomicWaker` 使用 kernel critical-section 保护内部非原子 waker cell。UART `RX_WAKER`/`TX_WAKER`/`DRAIN_WAKER` 与 network wakers 都受此问题影响。
  - registry `axtask 0.3.0-preview.2` 在 `TaskInner::new` 中使用 full mask，`spawn_task` 随即选择 run queue；公开 API 只支持当前 task 在入队后更新 affinity。
  - `run_queue::unblock_task` 可将 ready task 放入 remote queue，但现有路径忽略 remote reschedule 通知，目标 hart 可只在 timer/IRQ 后调度。
  - QEMU RISC-V `axhal` 提供单目标 `send_ipi`，S_SOFT 可由唯一 handler 处理；`axruntime/ipi`/`axipi` 不得与本 scheduler reschedule handler 同时占用该 IRQ。
  - QEMU 平台默认 PLIC MMIO range 为 `0x0c00_0000/0x21_0000`，QEMU DT/上游注释的完整窗口为 `0x0c00_0000/0x60_0000`。Make config 支持工作区 overlay 且用户 `EXTRA_CONFIG` 可最终覆盖。
  - UART 当前也通过 `ArceOsRuntime::spawn_with_name` 普通启动 copier，但本 Cycle 只保证未来的 affinity/IPI 能力可用，不修改 UART startup。
- Code and Critical Path:
  - Critical section: `kernel/src/critical_section_policy.rs` → `kernel/src/lib.rs::critical_impl` → Embassy UART/network wakers。
  - Task spawn: root/standalone manifests → workspace `crates/axtask` → `api::{spawn_task,spawn_raw,spawn_with_name}` → `TaskInner::new` → `run_queue::select_run_queue`。
  - Remote wake: `AxWaker::wake`/unblock → state transition → run-queue selection → local pending 或 remote `axhal::irq::send_ipi` → S_SOFT handler → IRQ guard exit/preemption。
  - QEMU boot: Make config merge → generated `.axconfig.toml` → page table MMIO map → per-hart PLIC `init_percpu` → scheduler/secondary readiness。

**Implementation Guidance**

依赖顺序为：先建立 host witness 和全局 critical-section；再 vendor 精确 `axtask` 基线，实现入队前 affinity 和 remote-ready IPI；然后传播 feature；最后加入 QEMU PLIC overlay 并运行 16-hart 早期启动。S_SOFT handler 只提交 preempt pending 和 telemetry，不在 handler 内直接切换 task。overlay 必须保留完整原 MMIO 列表，避免 TOML 数组覆盖丢失 UART/VirtIO/RTC 等条目。

**Behavioral Change**

- critical-section 从“仅本 hart IRQ 排除”变为“保存本地 IRQ、按 hart 支持嵌套、跨 hart 全局 Acquire/Release 互斥”。
- 新 task 可在首次 `select_run_queue` 前带有已验证 affinity；旧 spawn API 继续使用 full-online mask。任意 task mask 更新拒绝空、越界、offline 或未初始化候选，失败不改变旧值。
- 成功 remote `Blocked → Ready` 向目标 hart 发送一次 reschedule IPI；重复 wake 不通知，本地 wake 只设置本地 pending。
- 默认 RISC-V QEMU 构建使用工作区 overlay 映射完整 PLIC 窗口；非 QEMU 平台不变，用户 override 仍最后生效。

**Task Contracts**

### 1.1: SMP-safe kernel critical-section

- Requirement/Scenario: global critical-section；两 hart 并发 register/wake；ISR/task 嵌套与 IRQ restore。
- Depends on: None
- Targets: `kernel/src/critical_section_policy.rs::{IrqOps,acquire,release}`；`kernel/src/lib.rs::critical_impl`；`tests/ms04-async-rx-host-harness.rs`；必要的 MS08 foundation host harness。
- Current behavior: acquire 只关闭本 hart IRQ 并返回 bool restore state；release 无全局 owner 或 per-hart depth。
- Required behavior: 外层 acquire 关闭本地 IRQ 后以 Acquire 获得全局 owner；同 hart 嵌套只增加有界 depth；外层 release 以 Release 释放 owner 后按匹配 restore state 恢复 IRQ。
- Required changes: 使生产 glue 与 host witness 共享 policy；以实际 hart ID 索引 `MAX_CPU_NUM` 大小状态；保留 `restore-state-bool` ABI。
- Preserve: ISR 中 `release(false)` 不得提前开 IRQ；单 hart 行为；驱动 waker API。
- Forbidden: NIC/UART-local 替代锁；普通不可重入全局锁；临界区内阻塞、yield 或取得驱动锁；用 Relaxed telemetry 承担 owner/depth 同步。
- Test witness: 先扩展 host harness，观察跨线程互斥、同 hart 嵌套、ISR entry、restore 和非法 release RED；使用 `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`。
- GREEN condition: 互斥区最大并发为 1，嵌套不死锁，IRQ restore 计数匹配，非法 release fail closed。
- Verification: focused harness、`make host-test` 可执行部分、ordinary/`SMP=16` build。
- Stop when: ABI 不再是 `restore-state-bool`，平台 hart ID 不能安全映射到 `MAX_CPU_NUM`，或实现需要在全局临界区内阻塞。

### 1.2: Workspace-owned exact `axtask` baseline

- Requirement/Scenario: 入队前 affinity 与 remote wake 的 scheduler 责任边界；Cargo registry 保持只读。
- Depends on: None
- Targets: `crates/axtask/**`；root `Cargo.toml`；`crates/axnet/Cargo.toml`；`Cargo.lock`。
- Current behavior: root 和 standalone axnet 都解析 registry `axtask 0.3.0-preview.2`。
- Required behavior: 两个构建面都通过 `[patch.crates-io]` 解析到工作区内精确 `0.3.0-preview.2` 副本，默认 feature/API/license 和未修改行为与锁定上游一致。
- Required changes: 从本机锁定 registry 源复制 crate；保留来源和 license；只让 1.3–1.5 的调度扩展进入 vendor diff。
- Preserve: root 与 standalone axnet 使用同一副本；ordinary、non-SMP、单 hart 和既有 feature 可构建。
- Forbidden: 修改 `~/.cargo/registry`；vendor 其他 ArceOS crate；顺带升级依赖或格式化无关文件。
- Test witness: patch 前 metadata/source guard 证明 registry 来源；patch 后 root 和 standalone metadata 以及 lock diff 证明只切换 `axtask`。
- GREEN condition: 两个 manifest 均解析 workspace 副本，UART 基线与 axnet ordinary/qemu-diagnostics 继续通过。
- Verification: `cargo metadata --offline --no-deps`、standalone metadata、UART tests、两套 axnet tests 和 manifest/lock diff review。
- Stop when: 锁定源不完整、版本/license 不匹配，或解析必须 vendor 第二个 crate。

### 1.3: Affinity committed before first enqueue

- Requirement/Scenario: online/schedulable placement 前置；1/2/3/4/8/16、稀疏/无效集合；受控迁移的 scheduler 能力。
- Depends on: 1.2
- Targets: `crates/axtask/src/api.rs::{spawn_task,spawn_raw,spawn_with_name}` 及新兼容 API；`task.rs::{cpumask,set_cpumask}`；`run_queue.rs::{select_run_queue_index,select_run_queue}`；crate tests。
- Current behavior: `TaskInner::new` 使用 full mask，spawn 立即选择 run queue；公开更新只面向 current task，对 offline/越界 mask 没有闭合错误语义。
- Required behavior: 新 API 在 task 可运行且首次选择 run queue 前提交已验证 mask；旧 API 默认 full-online mask。任意 task 更新拒绝空、越界、offline 或未初始化 mask，失败不改变旧值。
- Required changes: 建立纯 mask validation/selection seam 和 pre-enqueue spawn 路径；返回/保存 `AxTaskRef` 以便后续 Iteration 扩展 mask。
- Preserve: `spawn_with_name` ABI、default spawn 行为、task identity 和 scheduler 现有选择规则。
- Forbidden: 先入队后纠正；选择未初始化 `RUN_QUEUES`；无效 mask panic；立即强迁移正在运行 task。
- Test witness: crate/host tests 先证明现有 spawn 无法提交初始 singleton，覆盖 1/2/3/4/8/16、稀疏、空、越界、offline 和失败不变。
- GREEN condition: 测试直接观察 mask 在 `select_run_queue` 前存在，所有有效规模与错误语义通过，旧 API 回归不变。
- Verification: vendor crate focused tests、metadata、UART/axnet 回归、`make build SMP=16`。
- Stop when: scheduler 无法取得 initialized/online 集合且调用者也无法显式传入，或安全更新要求 stop-the-world 迁移协议。

### 1.4: Event-driven remote scheduler wake

- Requirement/Scenario: UART/NIC IRQ 唤醒远端任务；event/register 交错；唯一 S_SOFT owner。
- Depends on: 1.2, 1.3
- Targets: `crates/axtask/Cargo.toml`；`crates/axtask/src/{api.rs,run_queue.rs,task.rs}` 及必要 IPI 模块；mock tests。
- Current behavior: remote unblock 将 task 放入 remote queue 后不通知目标 hart，依赖后续 timer/IRQ；没有 axtask-owned S_SOFT handler 或因果 telemetry。
- Required behavior: 仅成功 `Blocked → Ready` 且目标 queue 为 remote 时发送一次 `send_ipi`；本地成功 wake 设置 pending；重复 wake 不通知。S_SOFT handler 只记录 receive 并提交 preempt pending。
- Required changes: 建立默认关闭的 `ipi` feature，可 mock 的通知决策，send/receive telemetry 和注册失败处理。
- Preserve: QEMU platform 清除 S_SOFT；既有 run-queue 状态机与 IRQ/preemption guard；timer 作为时钟存在但不承担 wake 正确性。
- Forbidden: 同时启用 `axruntime/ipi`/`axipi`；handler 内直接 context switch；重复 wake 发 IPI；UART/network 直接调用 `send_ipi`。
- Test witness: mock run-queue/IPI tests 覆盖 remote success once、local success、duplicate wake、registration conflict 和 uninitialized current-task boundary。
- GREEN condition: telemetry 与状态转移一一对应，feature graph 只有 axtask 拥有 S_SOFT，handler 安全返回。
- Verification: focused crate tests、feature tree/source guard、ordinary/`SMP=16` build 和 16-hart 早期启动。
- Stop when: QEMU axhal 不再提供单目标 IPI，或 S_SOFT 已被不可关闭 runtime 功能占用。

### 1.5: Feature ownership and integration

- Requirement/Scenario: scheduler 唯一拥有 reschedule IPI；ordinary/non-SMP 兼容。
- Depends on: 1.4
- Targets: root/kernel Cargo features；必要的 feature/source guards。
- Current behavior: kernel `smp` 未启用 axtask-owned IPI，也没有锁定 S_SOFT 唯一所有者。
- Required behavior: 仅 SMP kernel 显式启用 `axtask/ipi`；ordinary/non-SMP 不引入 handler；任一构建图不同时启用竞争 owner。
- Required changes: 最小 feature 传播与 guard；不改变 UART/network 自身 feature 契约。
- Preserve: 旧 ordinary feature 集、单 hart 构建和无 IPI 平台。
- Forbidden: 无条件启用 IPI；引入第二 dispatcher；让驱动拥有 S_SOFT handler。
- Test witness: feature tree/source guard 先锁定当前缺失与冲突边界。
- GREEN condition: SMP graph 恰好一个 S_SOFT owner，ordinary graph 不启用 IPI，两者都可构建。
- Verification: Cargo feature tree/source guards、ordinary 和 `SMP=16` build。
- Stop when: feature unification 使冲突 owner 不可避免，或修正需要 vendor 其他 runtime crate。

### 1.6: Complete QEMU PLIC mapping for 16 harts

- Requirement/Scenario: `SMP=16` 环境前提；非零 boot hart；非 QEMU 与用户 override 兼容。
- Depends on: None
- Targets: 新工作区 QEMU config overlay；`make/config.mk` 合并顺序；config source guard/host test。
- Current behavior: PLIC range 为 `0x0c00_0000/0x21_0000`，高编号 hart supervisor context 可在 scheduler 初始化前 page fault。
- Required behavior: 默认 `riscv64-qemu-virt` 最终 config 包含 `0x0c00_0000/0x60_0000` 与全部原 MMIO ranges；非 QEMU 不合并；用户 `EXTRA_CONFIG` 最后生效。
- Required changes: 新增最小 overlay 和条件合并；直接检查最终 `.axconfig.toml` 值与优先级。
- Preserve: RTC、UART、VirtIO、PCI ranges；D1/VF2 配置；平台 package identity。
- Forbidden: vendor 整个 QEMU platform crate；修改 registry；用 page-fault handler 掩盖问题；把 overlay 用于 K3。
- Test witness: source/config test 证明旧值并使用现有 16-hart 早期 fault 作为 RED；overlay 后读取最终生成值。
- GREEN condition: 16-hart 启动越过 primary/secondary PLIC `init_percpu` 和 scheduler 早期初始化，无 PLIC page fault 或派生 current-task panic。
- Verification: config queries/source guard、`make build SMP=16`、`timeout 20s make justrun SMP=16 NET=n`。健康等待导致 timeout 124 可接受，但必须出现 16-hart/初始化 marker 且不得出现 panic/page fault。
- Stop when: QEMU runtime DT 报告窗口不是 `0x600000`，或 fault 地址不属于 PLIC context 区域。

### 1.7: Foundation integration Gate

- Requirement/Scenario: MS08 分层 Gate；UART/network 邻接回归；16-hart runtime 不能由编译替代。
- Depends on: 1.1–1.6
- Targets: 本 Iteration 全部 diff 和既有测试入口；不新增产品行为。
- Current behavior: 各基线可单独构建，但全局 SMP 互斥、remote-ready IPI 和 16-hart 早期启动未共同成立。
- Required behavior: focused、UART、axnet、host、target build 和有界 early runtime 形成一致通过结论；任一产品失败阻止后续 Iteration。
- Required changes: 只运行并记录命令、决定性输出、退出码和 full diff self-review；只修复 1.1–1.6 契约内问题。
- Preserve: 工作区用户修改；early console；UART 公开契约与 D1 workaround；MS01/MS04/MS07 行为。
- Forbidden: 降低到 `SMP=8`；启用 polling fallback、关闭 IRQ/EVENT_IDX；开始 UART/network placement；以历史日志代替。
- Test witness: 1.1–1.6 各自 RED/GREEN 与当前 UART/axnet 基线。
- GREEN condition: 所有非环境 Gate 通过；沙箱 UDP `EPERM` 可按既有边界记录，但其前置 tests 必须全绿；16-hart runtime 越过本 Iteration 的初始化边界。
- Verification: focused tests；`cargo test --manifest-path crates/uart_16550/Cargo.toml --features async`；两套 axnet tests；`make host-test`；`make build SMP=16`；有界 `make justrun SMP=16 NET=n`；`git diff --check`、strict OpenSpec validation 和 full diff review。
- Stop when: 任一 Acceptance 需要修改 UART/network placement、diagnostic ABI 或数据面，或同一基础失败无法在本 Cycle 契约内收敛。

**Invariants**

- UART 继续恰好一个 RX copier、一个 TX copier、一个 raw reader 和一个逻辑 raw writer；本 Cycle 不修改其 spawn 或 SPSC 拓扑。
- 每个网络硬件 queue 仍只有一个逻辑 owner；本 Cycle 不修改 owner/runner placement、descriptor 或 recovery 契约。
- remote notification 只由成功 task 状态转移触发；telemetry 不参与同步决定。
- critical-section 保持嵌套和 IRQ restore，全局锁内不执行阻塞操作。
- Cargo registry 只读；只 vendor 精确 `axtask 0.3.0-preview.2`。
- S_SOFT 只有一个 handler owner；QEMU overlay 只修正 QEMU PLIC 窗口，不代表 K3 AIA 或真板事实。

**Non-goals**

- UART copier、network owner/runner 的 placement 计算与启动。
- UART/network diagnostic ABI、timer-disabled witness、task 迁移与数据面资格。
- UART SPSC/TTY 公开契约、network descriptor/slot/epoch 语义的重设计。
- X100/A100 异构调度、RT24、K3 APLIC/IMSIC、D1/K3 UART 硬件资格、CPU hotplug、multiqueue/RSS 和性能。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| online/schedulable placement | 1/2/3/4/8/16、稀疏/无效 | D1, D4, D5 | 1.3；后续 2.1, 3.1 | 000 前置；001–002 完成 | `axtask::spawn*`、后续 policy | mask tests；后续 placement tests | None | Covered |
| SMP critical-section | 并发 register/wake、ISR/task 嵌套 | D3 | 1.1 | 000 | critical-section policy/glue | MS04/MS08 host stress | None | Covered |
| remote-ready wake | IRQ→copier/owner、event/register | D2, D7 | 1.4–1.5；后续 3.4 | 000 前置；002 runtime witness | axtask unblock/S_SOFT | mock causality；timer-disabled witness | None | Covered |
| ordering | publish-before-wake、复合 snapshot | D3, D6, D8 | 1.1；后续 4.3–4.4 | 000 前置；003 完成 | kernel critical section；driver states | mutex stress；publication stress | None | Covered |
| UART 唯一 SPSC 角色 | one-time startup、copier migration | D5, D8 | 后续 2.2, 4.1 | 001, 003 | UART driver/init/TTY | lifecycle/SPSC tests | None | Covered |
| UART 数据/readiness/drain | RX/TX、Full、tcdrain、quiet | D6, D9 | 后续 2.3–2.6, 5.1, 6.1 | 001, 004–005 | UART driver/device ops/ioctl | host model + SMP=16 serial | None | Covered |
| network 唯一 owner | non-owner publish、owner fault | D4–D6 | 后续 3.1–3.5 | 002 | axnet owner lifecycle | MS07 + placement tests | None | Covered |
| network 数据/recovery | 双向、Full、reset/waiter | D6, D9 | 后续 5.2–5.4, 6.2, 7.2 | 004–006 | axnet/probes/validator | SMP=16 network runtime | None | Covered |
| 受控迁移 | UART copier、owner/runner、无效 mask | D8 | 后续 4.1–4.4, 6.1–6.2 | 003, 005 | axtask + QEMU controls | identity/accounting stress | None | Covered |
| MS08 验证边界 | per-driver、combined、single-hart | D0, D9 | 1.6–1.7；后续 5.1–7.3 | 000–006 | config/build/probes | early boot + final runtime | None | Covered |

**Acceptance**

- A1 / critical-section / D3 / 1.1：两个 host thread 不能同时进入受保护区；嵌套、ISR 与 IRQ restore 结果正确；production glue 仍委托共享 policy。
- A2 / workspace axtask / D1 / 1.2：root 和 standalone axnet 使用同一 workspace 副本，版本/license/API 基线不变，registry 未修改。
- A3 / pre-enqueue affinity / D1 / 1.3：有效 mask 在首次 run-queue 选择前提交；1/2/3/4/8/16、稀疏和无效 mask 语义通过；旧 API 回归不变。
- A4 / remote wake / D2 / 1.4–1.5：成功 remote unblock 恰好一次 IPI，本地 wake 只提交 pending，重复 wake 不通知，S_SOFT 无 owner 冲突。
- A5 / `SMP=16` environment / D0 / 1.6：最终 QEMU config 映射完整 PLIC 窗口，非零 boot hart 可完成 PLIC/scheduler/secondary 早期初始化，不再出现基线 fault/panic。
- A6 / integration / 1.7：focused、UART、axnet、host 可执行部分、`SMP=16` build/runtime 和 full diff review 无产品失败；UART/network placement 仍未修改。

**Verification**

- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`：直接验证 critical-section policy 与 production 委托。
- workspace `axtask` focused tests：验证 mask validation、首次入队顺序、remote/local/duplicate wake 和 handler 决策。
- `cargo metadata --offline --no-deps` 及 standalone axnet metadata：验证 workspace patch 和 feature graph。
- `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async`：保持至少当前 62 unit + 8 regular doc + 10 compile-fail doc 基线，数量变化必须来自可解释的新测试。
- axnet ordinary/qemu-diagnostics 全量 tests：保持至少当前 `474/474` 与 `506/506`，数量变化必须解释。
- `make host-test`：沙箱 UDP `EPERM` 仅可作环境阻塞记录，其前置决定性 tests 必须全绿。
- 最终 config 检查、`make build SMP=16`、`timeout 20s make justrun SMP=16 NET=n`：必须观察 16 hart 和早期初始化进度，不得出现 page fault、panic、handler conflict 或未初始化 run queue。
- `git diff --check`、`openspec validate ms08-qemu-multi-hart-correctness-baseline --strict` 和 full diff review：无格式错误、越界修改、未解释依赖变化或文档不一致。

**Gate 2 Readiness**

- No Missing requirements: PASS — RTM 覆盖共享原语、UART、network、迁移和资格边界。
- Simplified requirements approved: PASS — `Simplification` 全部为 None。
- Investigation complete: PASS — 已定位 critical-section、spawn/run queue、remote unblock、S_SOFT、feature 传播、PLIC 映射，并补入 UART waker/spawn/SPSC 的本 Cycle 影响。
- Design closed: PASS — D0–D9 明确基础原语、驱动分层、观测、迁移和资格边界；本 Cycle 无实质 TBD。
- Tasks executable: PASS — 1.1–1.7 均有 targets、当前/目标行为、preserve/forbidden、witness、GREEN、verification 和 stop condition。
- Iteration plan ordered and balanced: PASS — 000 建立共享基础；001/002 分开 UART/network；003 闭合迁移与 ordering；004–006 分开协议、单驱动资格和组合恢复。
- Traceability complete: PASS — RTM 将每项 requirement 映射到 design、task、Iteration、代码面和 witness。
- Verification sufficient: PASS — host 并发、crate mock、metadata、UART/axnet 回归、target build 和 16-hart 早期 runtime 分别直接观察目标。
- No identity-style evidence engineering: PASS — 验证读取目标行为、状态、输出和退出结果，不建立 revision/run/session 身份协议。
- No material TBD for Act: PASS — PLIC 窗口、vendor 边界、affinity 错误语义、IPI ownership 与本 Cycle 停止条件已闭合。
- Change/Cycle consistency: PASS — proposal、async-I/O delta spec、design、tasks 和本 replan Cycle 统一使用 `SMP=16`，并将 UART/network placement 延期到独立 Iteration。
- Persisted Evidence mode valid: PASS — Mode 为 `none`，可重跑命令与决定性输出由 Act Response 承载。
- User plan approval: BLOCKED — 用户已批准范围重规划，但修订后的 Implementation Plan 和本 replan Cycle 执行契约尚待本次交付后审计批准。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、修改文件和符号写入 Act Response；本 Cycle 不包含不可低成本重跑的外部证据。

**Risks and Notes**

- 16-hart QEMU 会增加调度抖动；本 Cycle 只判断有界初始化和禁止错误，不使用吞吐阈值。
- `timeout` 可以以 124 结束健康等待；Act 必须同时检查必需 marker 与禁止 marker，不能只看退出码。
- PLIC 原因由配置、地址和栈共同支持；overlay 后仍在同点 fault 时触发 1.6 stop condition，不扩大到 page-fault handler 规避。
- UART 在本 Cycle 只是 critical-section 和 scheduler 的邻接回归消费者；不得提前修改 copier API、IER cache 或 placement。

## Act Response

- Status: pending

**Implemented**

None

**Changed Files and Symbols**

None

**Deviations from Plan**

None

**Blocker Handoff**

None

**Blocker Resolution**

None

**Self-Review**

- Plan compliance: BLOCKED
- Full diff reviewed: BLOCKED
- Critical findings unresolved: 0
- Important findings unresolved: 0
- Minor findings unresolved: 0

Implementation has not started.

**Verification Evidence**

None

**Persisted Evidence**

None required

**Experience Candidates**

None

**Remaining Issues**

Awaiting Gate 2 plan approval.

**Commit or Diff Reference**

None

## Plan Review

- Review Result: pending

**Findings**

Not reviewed; implementation has not started.

**Deviation Classification**

None

**Acceptance Gaps**

None assessed.

**Convergence**

N/A

**Evidence**

None

**Follow-up Decision**

Await user Gate 2 approval, then mark this Plan Context ready and hand off to `openspec-act`.

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

None
