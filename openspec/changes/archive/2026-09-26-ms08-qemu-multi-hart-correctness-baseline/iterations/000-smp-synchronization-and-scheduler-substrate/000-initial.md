# Iteration 000 / Cycle 000: SMP synchronization and scheduler substrate

## Plan Context

- Status: draft
- Iteration: 000-smp-synchronization-and-scheduler-substrate
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: None

**Iteration Scope**

- Change tasks: 1.1–1.6
- Depends on: None
- Stable baseline: kernel critical-section跨hart安全；工作区自有`axtask`能在首次入队前提交affinity，并以remote-ready IPI唤醒目标hart；QEMU的PLIC映射支持16 hart完成早期scheduler/secondary初始化；网络placement尚不改变。
- Verification boundary: focused互斥、嵌套、affinity和IPI tests、QEMU config overlay tests、两套axnet回归、`SMP=16` target build及有界早期启动通过；remote wake不再以timer为设计依赖，基线PLIC page fault与派生的未初始化current-task panic消失。
- Diagnostic boundary: 失败限制在critical-section、task构造/run queue、S_SOFT注册、IPI发送/接收、feature传播、QEMU PLIC配置映射或早期SMP启动。
- Deferred tasks: 2.1–5.4

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: proposal和delta spec中已批准的SMP-safe critical-section、事件驱动remote wake、动态online/schedulable placement前置能力、`SMP=16` QEMU目标规模及单hart兼容边界。
- Excluded scope: 网络owner/runner placement、V5 ABI、timer-disabled witness、迁移control、MS08 guest协议、数据面及recovery runtime资格。

**Objective**

建立多hart网络后续工作可依赖的内核同步和调度底座：全局互斥且正确恢复本地IRQ的critical-section、工作区锁定版本的affinity-aware `axtask`、成功remote unblock后的单次reschedule IPI，以及能让16-hart QEMU越过PLIC和scheduler初始化的工作区平台配置。

**Background**

现有MS04–MS07只在单hart QEMU上取得资格。kernel critical-section当前只屏蔽本hart IRQ；`axtask`在task首次入队后才允许当前task设置affinity；remote unblock忽略remote reschedule；默认QEMU平台配置只映射`0x0c00_0000/0x21_0000`的PLIC窗口。`SMP=16`探针中OpenSBI枚举hart 0–15并选择boot hart 11，PLIC supervisor context访问越过旧映射，早期page fault又被尚未初始化的`current()`调用掩盖为`current task is uninitialized`。本Cycle先关闭这些基础缺口，不接入网络角色placement。

**Investigation Facts**

- Current Baseline: revision `04fc3ce5101ebf6a2e30b91e7fc6512c2a17f8a3`，branch `k3`。axnet ordinary `474/474`、qemu-diagnostics `506/506`通过；`make build SMP=8`和`make build SMP=16`均成功。`make host-test`已通过早期Rust harness与前置C/Python检查，随后因沙箱禁止UDP socket而以`EPERM`环境阻塞。`timeout 20s make justrun SMP=16 NET=n`观察到OpenSBI `Platform HART Count: 16`、boot hart 11和kernel `smp = 16`，随后在PLIC `init_percpu`期间page fault并派生未初始化current-task panic。
- Current-State Evidence: `kernel/src/critical_section_policy.rs::acquire/release`只保存、关闭和恢复本地IRQ，没有跨hart锁或per-hart嵌套状态；`kernel/src/lib.rs::critical_impl`通过`critical_section::set_impl!`委托该seam。`tests/ms04-async-rx-host-harness.rs`复用同一policy源，但只覆盖单线程restore语义。
- Current-State Evidence: registry `axtask 0.3.0-preview.2`的`api::spawn_task`先把默认full-mask task交给`select_run_queue`；`set_current_affinity`只能修改当前task。`run_queue::unblock_task`在remote run queue上明确忽略`resched`，只在local queue设置preempt pending。`RUN_QUEUES`按`MAX_CPU_NUM`静态分配，未初始化remote queue被选择会触发未定义前提。
- Current-State Evidence: `axhal::irq`在`ipi` feature下导出`IPI_IRQ`、`IpiTarget`和`send_ipi`；QEMU platform的S_SOFT handler槽只允许一次注册，platform `handle`调用handler后清除`sip.ssoft`。`axruntime/ipi`若启用会注册同一槽，因此本change只能由一个owner注册。
- Current-State Evidence: root `Cargo.toml`和`crates/axnet/Cargo.toml`都从registry解析`axtask = 0.3.0-preview.2`，当前没有workspace patch。root `smp`传播到`axfeat/smp`和`starry-kernel/smp`；kernel `smp`当前只传播`axfeat/smp`。
- Current-State Evidence: registry QEMU平台`axconfig.toml`注释记录DT PLIC `reg`大小`0x600000`，但`devices.mmio-ranges`实际只给`0x210000`。PLIC context为`hart_id * 2 + 1`，高编号hart的context寄存器位于该旧映射之外。`make/config.mk`按`defconfig.toml → PLAT_CONFIG → EXTRA_CONFIG`合并，允许在registry平台配置和用户override之间插入工作区overlay。
- Code and Critical Path: `critical_section::Impl → critical_section_policy::{acquire,release} → axhal IRQ primitives`；`spawn* → TaskInner::new → into_arc → select_run_queue → add_task`；`AxWaker/wait queue wake → select_run_queue → unblock_task → Blocked→Ready → local preempt pending或remote IPI`；`axhal IRQ wrapper → QEMU S_SOFT handler → preempt pending → guard drop → normal scheduler reschedule`；`make/config.mk → axconfig-gen → kernel MMIO map → axhal::init_later → PLIC::init_by_context → axtask scheduler init → secondary bring-up`。

**Implementation Guidance**

先用host witness固定critical-section、affinity选择和remote-notification决策，再实施对应原语。随后完成精确版本vendor和feature传播，最后加入QEMU config overlay并执行16-hart早期启动。S_SOFT handler只提交preempt pending和telemetry，不在handler内直接切换task。config overlay复制完整既有MMIO列表，只扩大PLIC条目，避免数组覆盖语义丢失其他设备；用户`EXTRA_CONFIG`保持最终优先级。

**Behavioral Change**

- critical-section由“仅本地IRQ排除”变为“保存本地IRQ状态、按hart支持嵌套、跨hart全局Acquire/Release互斥”。非法hart、depth underflow/overflow或非owner release不得释放他人所有权。
- 新task可在首次`select_run_queue`前携带已验证affinity；既有spawn API和默认full-mask行为保持。任意task affinity更新拒绝空、越界或offline mask，失败不改变旧值。
- 成功的`Blocked → Ready`若进入remote run queue，scheduler向该hart发送一次reschedule IPI；重复wake不发送，本地wake只设置本地preempt pending。handler返回后沿既有guard/preemption路径调度。
- 默认RISC-V QEMU构建使用工作区overlay映射完整PLIC窗口；非QEMU平台不变，用户override仍最后生效。

**Task Contracts**

### 1.1: SMP-safe kernel critical-section

- Requirement/Scenario: critical-section在SMP下提供进程级互斥；两个hart并发register/wake；ISR嵌套和task IRQ restore。
- Depends on: None
- Targets: `kernel/src/critical_section_policy.rs::{IrqOps,acquire,release}`；`kernel/src/lib.rs::critical_impl`；`tests/ms04-async-rx-host-harness.rs`；必要的新MS08 foundation host harness。
- Current behavior: acquire只关闭本hartIRQ并返回旧状态；release按bool恢复，没有全局owner或per-hart嵌套。
- Required behavior: 外层acquire关闭本地IRQ后以Acquire取得全局owner；同hart嵌套只增加有界depth；外层release以Release释放owner后按匹配restore state恢复IRQ。跨hart同时进入必须串行；非法状态fail closed且可测试。
- Required changes: 为生产和host witness共享同步policy；以实际hart ID索引`MAX_CPU_NUM`大小状态；保留`restore-state-bool` ABI和`critical_section::Impl`委托。
- Preserve: ISR中`release(false)`绝不提前开IRQ；单hart行为；临界区内不阻塞、不yield、不取得网络锁。
- Forbidden: NIC-local替代waker；普通不可重入全局锁；用Relaxed telemetry承担owner/depth同步；删除既有production source guard。
- Test witness: 先扩展host harness形成跨线程互斥、同hart嵌套、ISR entry、restore和非法release RED；命令为`rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`。
- GREEN condition: 新旧critical-section测试全绿，互斥区最大并发为1，嵌套不死锁，所有IRQ restore计数匹配。
- Verification: 上述focused harness、`make host-test`可执行部分、普通和SMP target build；任一数据竞争、死锁、提前开IRQ或所有权误释放均失败。
- Stop when: `critical-section` ABI不再是`restore-state-bool`，平台hart ID无法安全映射到`MAX_CPU_NUM`，或实现需要在临界区内阻塞。

### 1.2: Workspace-owned exact `axtask` patch baseline

- Requirement/Scenario: remote wake和启动期placement的调度责任边界；禁止修改Cargo registry。
- Depends on: None
- Targets: `crates/axtask/**`；root `Cargo.toml`；`crates/axnet/Cargo.toml`；`Cargo.lock`。
- Current behavior: 两个manifest都解析registry `axtask 0.3.0-preview.2`，无法提交项目拥有的affinity/IPI修改。
- Required behavior: 两个构建面都通过`[patch.crates-io]`解析到工作区内精确的`0.3.0-preview.2`副本，默认feature/API/许可证和未修改行为与锁定上游一致。
- Required changes: 从本机锁定registry源码复制该crate；保留来源和license；只让后续1.3/1.4修改进入vendor diff；锁文件明确解析本地path。
- Preserve: root与standalone axnet使用同一副本；普通非SMP、单hart和既有features可构建。
- Forbidden: 原地修改`~/.cargo/registry`；vendor axhal、axruntime或其他ArceOS crate；顺带格式化或升级依赖。
- Test witness: patch前用metadata/source guard证明仍指向registry；patch后运行root与`crates/axnet` metadata并检查lock diff只改变axtask来源。
- GREEN condition: 两个manifest均解析工作区副本，普通axnet `474/474`和qemu-diagnostics `506/506`继续通过。
- Verification: `cargo metadata --offline --no-deps`、`cargo metadata --offline --no-deps --manifest-path crates/axnet/Cargo.toml`、两套axnet tests和full manifest/lock diff review。
- Stop when: 锁定源码不完整、版本/license不匹配，或需要vendor第二个crate才能解析。

### 1.3: Affinity committed before first enqueue

- Requirement/Scenario: 启动期placement按有效集合计算；稀疏/无效hart集合；单/双/三及以上hart退化的调度前置能力。
- Depends on: 1.2
- Targets: `crates/axtask/src/api.rs::{spawn_task,spawn_raw,spawn_with_name}`及新增兼容API；`crates/axtask/src/task.rs::{cpumask,set_cpumask}`；`crates/axtask/src/run_queue.rs::{select_run_queue_index,select_run_queue}`；crate tests。
- Current behavior: `TaskInner::new`得到full mask，`spawn_task`立即选择run queue；公开更新只支持current task，空mask以外的越界/offline语义未闭合。
- Required behavior: 新API在task成为可运行对象且首次选择run queue前提交已验证mask；旧API继续默认full online mask。任意task更新为空、越界或offline时返回稳定失败且旧mask不变。
- Required changes: 建立纯mask验证/选择seam和入队前spawn路径；mask只允许`cpu_num()`范围内已初始化候选；提供后续Cycle保存`AxTaskRef`后安全扩展mask的能力。
- Preserve: `spawn_with_name`现有ABI、default spawn行为、task identity和scheduler选择规则。
- Forbidden: 先入队后纠正；选择未初始化`RUN_QUEUES`；无效mask panic；立即强迁移正在运行task。
- Test witness: crate/host tests先证明现有spawn无法提交初始singleton，并覆盖1/2/8/16、空、越界、offline和失败不变。
- GREEN condition: 测试直接观察mask在`select_run_queue`前存在；所有有效规模和失败语义通过，旧API回归不变。
- Verification: vendor crate focused tests、metadata、两套axnet tests、`make build SMP=16`；任何首次错误入队或无效更新破坏旧mask均失败。
- Stop when: online/initialized集合在scheduler API层不可取得且无法由调用者显式提供，或安全更新需要即时stop-the-world迁移协议。

### 1.4: Event-driven remote scheduler wake

- Requirement/Scenario: IRQ hart唤醒远端owner；event/register交错；远端wake超时；唯一S_SOFT owner。
- Depends on: 1.2, 1.3
- Targets: `crates/axtask/Cargo.toml` feature graph；`crates/axtask/src/{api.rs,run_queue.rs,task.rs}`及必要的新IPI模块；root/kernel feature传播；mock tests。
- Current behavior: 成功remote unblock把task放入remote queue后忽略`resched`，依赖该hart后续timer/IRQ；没有axtask-owned S_SOFT handler或因果telemetry。
- Required behavior: 仅成功`Blocked → Ready`且目标queue为remote时发送一次`axhal::irq::send_ipi`；本地成功wake设置preempt pending；重复wake不通知。primary scheduler唯一注册`IPI_IRQ` handler，handler只记录receive并为已初始化current task提交preempt pending，正常IRQ guard退出完成调度。
- Required changes: 新增默认关闭的`ipi` feature并依赖`preempt`、`axhal/ipi`；实现可mock的通知决策、send/receive telemetry和注册失败处理；kernel `smp`显式启用该feature。
- Preserve: QEMU platform负责清除S_SOFT；现有run queue状态机和preemption guard；timer仍可用于系统时钟但不承担wake正确性。
- Forbidden: 同时启用`axruntime/ipi`/`axipi`；handler内直接context switch；每次重复wake发IPI；网络层直接调用`send_ipi`。
- Test witness: mock run-queue/IPI tests先锁定remote成功一次、本地成功、重复wake、注册冲突和未初始化current-task边界。
- GREEN condition: 因果计数与状态迁移一一对应；feature graph只有axtask拥有S_SOFT；handler只提交pending并安全返回。
- Verification: focused crate tests、Cargo feature tree/source guard、普通/SMP build、16-hart早期启动无handler冲突；任一重复IPI、timer-only设计或双重注册均失败。
- Stop when: 当前QEMU axhal不再提供单目标IPI或S_SOFT槽已被不可关闭的runtime功能占用。

### 1.5: Complete QEMU PLIC mapping for 16 harts

- Requirement/Scenario: `SMP=16`固定资格的环境前提；非零boot/IRQ hart；自动Gate不得通过降低CPU数绕过。
- Depends on: None
- Targets: 新的工作区QEMU config overlay；`make/config.mk`合并顺序；配置source guard/host test。
- Current behavior: registry平台的PLIC MMIO range为`0x0c00_0000/0x21_0000`；boot hart 11的supervisor context访问越界并在scheduler初始化前page fault。
- Required behavior: 默认`riscv64-qemu-virt`最终config包含`0x0c00_0000/0x60_0000` PLIC mapping和全部原有MMIO ranges；其他平台不合并；用户`EXTRA_CONFIG`最后生效。
- Required changes: 添加最小overlay和条件合并；测试最终`.axconfig.toml`值与优先级；保持registry只读。
- Preserve: RTC、UART、VirtIO、PCI ranges及平台package identity；D1/VF2配置；用户显式override。
- Forbidden: vendor整个QEMU platform crate；修改registry；只更改page-fault handler掩盖故障；把overlay用于K3真板。
- Test witness: source/config test证明旧生成值为`0x21_0000`并复现16-hart早期fault；overlay后读取axconfig生成值。
- GREEN condition: 16-hart启动越过primary和secondary PLIC `init_percpu`，无PLIC page fault或派生current-task panic；非QEMU和override用例保持预期。
- Verification: axconfig queries/source guard、`make build SMP=16`、`timeout 20s make justrun SMP=16 NET=n`的串口判定；健康等待可由timeout结束，但日志必须含16-hart和secondary初始化进度且不含panic/page fault。
- Stop when: 当前QEMU运行时DT报告的PLIC窗口不是`0x600000`，或越界地址不落在PLIC context区域，此时返回Plan重新判定原因。

### 1.6: Iteration integration Gate

- Requirement/Scenario: MS08自动Gate失败；单hart兼容；16-hart基础资格不得由编译成功替代。
- Depends on: 1.1–1.5
- Targets: 本Iteration全部diff和测试入口；不新增产品行为。
- Current behavior: 单项基线可构建，但SMP同步、remote wake和16-hart早期启动未共同成立。
- Required behavior: focused、回归、target build和有界早期runtime形成一致通过结论；非环境失败停止后续Iteration。
- Required changes: 运行并记录命令、决定性输出、退出码和full diff self-review；只修复1.1–1.5契约内问题。
- Preserve: 工作区既有用户修改；MS01/MS04/MS07行为；无关平台和ABI。
- Forbidden: 以`SMP=8`、polling fallback、关闭IRQ/EVENT_IDX或历史日志替代；在Gate中扩大到网络placement。
- Test witness: 1.1–1.5各自RED/GREEN及现有基线。
- GREEN condition: 所有非环境Gate通过；沙箱UDP `EPERM`可按既有环境边界记录，但其前置测试必须全绿；16-hart runtime越过本Iteration初始化边界。
- Verification: focused tests、两套axnet全量、`make host-test`可执行部分、`make build SMP=16`、有界`make justrun SMP=16 NET=n`、`git diff --check`和full diff review。
- Stop when: 任一Acceptance需要修改network placement/V5/data plane，或同一基础失败无法在本Cycle契约内收敛。

**Invariants**

- 每个硬件queue仍只有一个逻辑owner；本Cycle不改变网络task placement或descriptor/recovery契约。
- 远端通知只有成功状态迁移才能发送；telemetry不参与同步决定。
- critical-section嵌套和IRQ restore必须保持，且不得在全局锁内执行阻塞操作。
- Cargo registry保持只读；只vendor精确`axtask 0.3.0-preview.2`。
- S_SOFT只有一个handler owner；不启用`axruntime/ipi`。
- QEMU overlay只修正QEMU已声明的PLIC窗口，不代表K3 AIA或真板事实。

**Non-goals**

- owner/runner/IRQ角色选择与启动、V5诊断ABI、timer-disabled witness。
- task自然迁移、共享网络状态ordering审计、guest probe和validator。
- 数据面压力、reset/link交错、性能、CPU hotplug、multiqueue/RSS。
- X100/A100异构调度、RT24、K3 APLIC/IMSIC或真板bring-up。

**Requirements Traceability Matrix**

| Requirement | Scenario | Design | Task | Iteration | Code Surface | Test Witness | Simplification | Status |
|---|---|---|---|---|---|---|---|---|
| 启动期动态placement | 稀疏/无效集合、非零boot/IRQ | D1, D4, D5 | 1.3；后续2.1–2.2 | 000前置；001完成 | `axtask::spawn*`、后续axnet policy | 1/2/8/16 mask tests；后续placement tests | None | Covered |
| 唯一queue owner | 非owner只发布工作 | D1, D5, D8 | 后续2.2、3.1 | 001–002 | axnet owner lifecycle | MS07回归、后续identity tests | None | Covered |
| SMP critical-section | 并发waker、ISR嵌套 | D3 | 1.1 | 000 | critical-section policy/glue | MS04/MS08 host stress | None | Covered |
| 跨hart wake | IRQ→owner、register交错、超时 | D2, D7 | 1.4；后续2.4 | 000前置；001 runtime witness | axtask unblock/S_SOFT | mock因果tests；后续timer-disabled witness | None | Covered |
| 共享状态ordering | publish/wake、复合tuple | D3, D6, D8 | 1.1；后续3.2 | 000前置；002完成 | policy、network atomics/locks | mutex stress；后续publication stress | None | Covered |
| 多hart数据面 | 双向、Full、quiet | D9 | 后续5.1 | 003 | axnet/guest protocol | SMP=16 runtime validator | None | Covered |
| reset/I/O交错 | device-owned、waiter、late event | D9 | 后续5.3 | 004 | MS07 recovery surfaces | SMP=16 recovery runtime | None | Covered |
| 受控迁移 | owner/runner迁移、无效目标 | D8 | 后续3.1、5.2 | 002–003 | axtask mask、QEMU control | migration host/runtime tests | None | Covered |
| MS08验证边界 | 自动失败、SMP=16、结论范围 | D0, D9 | 1.5–1.6；后续4.1–5.4 | 000–004 | config/build/probe/validator | build、early boot、最终runtime | None | Covered |

**Acceptance**

- A1 / critical-section / D3 / 1.1：两个host线程不能同时进入受保护区；嵌套与ISR restore结果正确；生产glue仍委托共享policy。
- A2 / placement前置 / D1 / 1.2–1.3：root和standalone axnet使用同一workspace axtask；有效mask在首次run queue选择前提交；1/2/8/16与无效mask语义通过。
- A3 / remote wake / D2 / 1.4：成功remote unblock恰好一次IPI，本地wake只提交pending，重复wake不通知，S_SOFT无注册冲突。
- A4 / SMP=16环境 / D0 / 1.5：最终QEMU config映射完整PLIC窗口；非零boot hart可完成PLIC和scheduler/secondary早期初始化，不再出现基线fault/panic。
- A5 / integration / 1.6：focused、axnet、host可执行部分、SMP=16 build/runtime和diff review没有产品失败；网络placement仍未改变。

**Verification**

- `rustc --edition=2024 --test tests/ms04-async-rx-host-harness.rs -o /tmp/ms04-async-rx-host-test && /tmp/ms04-async-rx-host-test`：直接证明critical-section policy和生产委托。
- vendor axtask focused test命令：证明mask验证、首次入队顺序、remote/local/duplicate wake和handler决策。
- `cargo metadata --offline --no-deps`及standalone axnet metadata：证明同一workspace patch和feature graph。
- axnet ordinary与qemu-diagnostics全量tests：预期分别保持至少现有`474/474`、`506/506`，数量变化必须解释为新增测试。
- `make host-test`：采信沙箱socket创建前全部决定性结果；仅既有UDP `EPERM`可记为环境阻塞。
- axconfig最终值检查、`make build SMP=16`、`timeout 20s make justrun SMP=16 NET=n`：必须观察16 hart和早期初始化进度，不得出现page fault、panic、handler冲突或未初始化run queue；健康等待导致的timeout不作为产品失败。
- `git diff --check`、OpenSpec strict validation和full diff review：无格式错误、越界修改或未解释依赖变化。

**Gate 2 Readiness**

- No Missing requirements: PASS — change-level RTM九项全部映射到design、task、Iteration、代码面和witness。
- Simplified requirements approved: PASS — `Simplification`全部为None。
- Investigation complete: PASS — 已定位critical-section、spawn/run queue、remote unblock、S_SOFT、feature传播、config merge、PLIC地址和早期panic调用链。
- Design closed: PASS — D0–D3明确平台映射、vendor、affinity、IPI及错误边界，没有实现语义TBD。
- Tasks executable: PASS — 1.1–1.6各含targets、当前/目标行为、preserve/forbidden、RED/GREEN、验证和停止条件。
- Iteration plan ordered and balanced: PASS — 000只建立后续共同底座；placement、migration、protocol和recovery分别延期到001–004。
- Traceability complete: PASS — RTM覆盖全部requirements；本CycleAcceptance映射1.1–1.6。
- Verification sufficient: PASS — host并发、crate mock、metadata、全量axnet、target build和真实16-hart早期runtime分别直接观察目标行为。
- No identity-style evidence engineering: PASS — 验证读取行为、状态、输出和退出结果，不建立revision/run认证协议。
- No material TBD for Act: PASS — QEMU PLIC窗口和merge策略已闭合；局部命名与测试拆分留给Act。
- Change/Cycle consistency: PASS — proposal、spec、design、tasks与本Cycle统一使用`SMP=16`及动态online/schedulable集合。
- Persisted Evidence mode valid: PASS — Mode为`none`，可重跑命令和决定性输出由Act Response承载。
- User plan approval: BLOCKED — 用户已批准从`SMP=8`调整到`SMP=16`及动态placement方向；完整Implementation Plan和本Cycle执行契约尚待本次交付后明确批准。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、修改文件和符号写入Act Response；本Cycle不需要不可低成本重跑的外部证据。

**Risks and Notes**

- 16-hart QEMU增加调度抖动，runtime只判断有界初始化和错误缺失，不使用吞吐阈值。
- `timeout`可能以124结束健康等待；Act必须同时检查必需marker和禁止marker，不能只看退出码。
- PLIC原因由地址、配置和栈共同支持；若overlay后仍在同一点fault，满足1.5停止条件并返回Plan，不扩大到page-fault handler绕过。
- 工作区已有用户修改位于`.claude/docs/SNAPSHOT.md`、`.claude/docs/tasks.md`和`openspec/specs/references/spec.md`；Act必须保持并避开这些文件。

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

- Review Result: replan-required

**Findings**

Before implementation, the user expanded MS08 from a network-only SMP baseline to a shared async-I/O SMP baseline that includes the existing async UART. The original Plan Context remains unimplemented but no longer covers the approved requirement set.

**Deviation Classification**

NEW-EVIDENCE

**Acceptance Gaps**

- The original scope does not cover UART copier placement, UART SPSC ownership, ISR/copier/TTY cross-hart wake, readiness, `flush/tcdrain`, UART migration, or UART-specific runtime qualification.
- The original Iteration Map cannot preserve separate UART and network diagnostic boundaries.

**Convergence**

expanded

**Evidence**

- User instruction on 2026-09-16: include the existing async UART with the NIC in the multi-core adaptation, followed by explicit approval to rewrite the plan.
- Current code: `kernel/src/entry.rs` initializes async UART and VirtIO-net in the same QEMU boot path; `uart_init::start_copiers` starts two unpinned tasks; `ArceOsRuntime::spawn` calls `axtask::spawn_with_name`; UART ISR uses global `AtomicWaker`s; RX/TX rings rely on SPSC identity.
- Fresh UART baseline: `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` passed 62 unit tests, 8 regular doc-tests and 10 compile-fail doc-tests.

**Follow-up Decision**

The approved requirement and verification boundaries changed before Act. Preserve this Cycle's Plan Context, update the change-level artifacts, and use `001-replan.md` as the only execution candidate. No implementation from this Cycle may start.

**Iteration Plan Update**

The Iteration Map now separates shared SMP substrate, UART placement, network placement, shared migration/ordering, qualification protocols, per-driver runtime qualification, and combined recovery/final regression.

**Next Cycle**

`001-replan.md`

**Next Iteration**

None
