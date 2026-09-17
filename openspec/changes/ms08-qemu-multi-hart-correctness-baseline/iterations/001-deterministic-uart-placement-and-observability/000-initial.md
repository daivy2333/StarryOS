# Iteration 001 / Cycle 000: Deterministic UART placement and observability

## Plan Context

- Status: ready
- Iteration: 001-deterministic-uart-placement-and-observability
- Cycle: 000-initial
- Cycle Type: initial
- Parent cycle: `../000-smp-synchronization-and-scheduler-substrate/002-replan.md`

**Iteration Scope**

- Change tasks: 2.1–2.6
- Depends on: Iteration 000
- Stable baseline: RX/TX copier 在 secondary-ready 后各以 singleton affinity 启动一次，SPSC 身份、独立 UART snapshot 和四阶段 drain 观测稳定。
- Verification boundary: placement policy、copier lifecycle、UART SMP host witnesses、QEMU/D1 compile 和 `SMP=16` UART startup。
- Diagnostic boundary: UART startup、SPSC endpoint、waker/PollSet、IER、completion 或 snapshot。
- Deferred tasks: 3.1–7.3

**Cycle Scope**

- Trigger: initial
- Acceptance gaps: None
- Repair items: None
- Inherited scope: Iteration 000 已验收的全局 critical-section、schedulable run-queue publication、入队前 affinity、remote-ready IPI、单一 S_SOFT owner 和 QEMU PLIC 修正。
- Excluded scope: network placement/V5、task migration、guest qualification protocol、正式 UART 数据面资格、组合压力、真板 SMP 资格和性能优化。

**Objective**

在不改变 UART SPSC 和旧 TXDBG ABI 的前提下，让 RX/TX copier 在 scheduler secondary-ready 后以确定 singleton affinity 各启动一次，并提供足以观察 placement、IRQ/copy hart、ring、remote wake 和四阶段 TX completion 的 QEMU-only snapshot。

**Background**

Iteration 000 已使 `axtask` 只选择已发布 run queue，并提供首次入队前 affinity 与 remote-ready IPI。当前 UART hardware/ring/IRQ 初始化和 copier 启动已经分开，但 `kernel/src/entry.rs` 仍调用 `uart_init::start_copiers()`，后者经 `AsyncUartDriver::start_{rx,tx}_copier → ArceOsRuntime::spawn → axtask::spawn_with_name` 使用默认 full affinity，也不保存 task handle。UART crate 已有唯一 raw reader/writer、AtomicWaker、PollSet、ring readiness、TX debug 和 `ring_empty/copy_active/staged_bytes/transmitter_empty` completion 语义。

**Investigation Facts**

- Current Baseline:
  - Iteration 000 Cycle 002 Review accepted：critical-section、schedulable readiness、入队前 affinity、remote IPI、PLIC 和 16-hart early runtime 已通过。
  - `uart_16550 --features async` 的既有 crate suites、ordinary/`SMP=16`/D1 builds 和用户确认的 `make justrun SMP=16` 结论可采信；本 Iteration 修改对应代码后按任务重跑。
  - host critical-section harness 以用户批准的串行模式运行；本 Iteration 不扩大该测试隔离范围。
- Current-State Evidence:
  - registry `axruntime 0.3.0-preview.2::rust_main` 在调用 kernel `main()` 前执行 `start_secondary_cpus`；每个 secondary 在增加 `INITED_CPUS` 前调用 `axtask::init_scheduler_secondary()`，primary 等待 `is_init_ok()`。因此 `kernel/src/entry.rs::init` 是 QEMU/D1 copier placement 的 secondary-ready 边界。
  - `kernel/src/entry.rs` 在 `init_uart_hardware` 和 startup benchmark 后调用 `unsafe uart_init::start_copiers()`；QEMU 与 D1 async 路径各有一次调用。
  - `uart_init::start_copiers` 不防重复、不保存 handle，并调用 driver 的 spawn wrapper。`ArceOsRuntime::spawn` 最终使用 plain `axtask::spawn_with_name`。
  - `AsyncUartDriver::rx_copier_loop` 和 `tx_copier_loop` 是私有无限 future；公开 unsafe wrappers 负责 spawn，唯一性完全依赖调用者。
  - `axtask` 已有 `spawn_raw_with_affinity`、`AxTaskRef::set_cpumask_checked` 和内部 Acquire readiness snapshot，但没有供 kernel placement policy读取的公开只读 schedulable mask。
  - QEMU UART IRQ 入口为 `qemu_uart_irq_handler → uart_isr_wrapper → uart_16550::async_::isr::uart_isr_handler`；D1 使用独立 `ArceOsD1UartPort` 和有界 slow-poll fallback。
  - 旧 UART ioctl `0x5458_4431/0x5458_4432` 只承载 TXDBG snapshot/reset；新增复合 snapshot 必须使用独立 QEMU-only command 和 wire type。
- Code and Critical Path:
  - Boot/startup：`axruntime::rust_main → start_secondary_cpus/init_scheduler_secondary → kernel main → entry::init → init_uart_hardware → benchmark → start_copiers`。
  - Placement：`axtask schedulable snapshot → kernel pure placement policy → TaskInner/future adapter → spawn_raw_with_affinity → singleton run queue`。
  - RX：UART IRQ → `RX_WAKER` → RX copier → RX ring → PollSet/TTY reader。
  - TX：TTY writer → TX ring → TX copier → FIFO → TEMT；Full 恢复通过 ring waker，drain 使用 ring/copy-active/staged/TEMT 四阶段。
  - Observation：IRQ/copy hooks、scheduler IPI counters、placement state和 driver ring/completion snapshot → QEMU-only UART snapshot ioctl。

**Implementation Guidance**

先建立不依赖 MMIO 的 placement 与 lifecycle RED witnesses，再公开最小只读 schedulable mask。随后让 driver 暴露不自行 spawn 的 RX/TX future，kernel 在已知 secondary-ready 的现有入口以入队前 affinity启动并保存两个 handle；兼容 wrapper 复用同一 future。最后增加独立 snapshot 和 wake/drain witnesses。不要改变 axruntime 启动顺序，不要通过先 plain spawn 再改 mask 修补首次入队。

**Behavioral Change**

- UART RX/TX copier 从默认 full affinity 改为确定的 singleton affinity；小 hart 集合可按 policy 共置，`SMP=16` 下二者分离。
- copier 仍在 hardware/IRQ 初始化与 startup benchmark 之后启动，但首次入队已绑定目标 mask，且 kernel 保存原 task handle。
- 重复启动在创建第二 copier 前 fail closed；同一 RX writer/TX reader 的 SPSC 身份不变。
- QEMU 增加独立 UART placement/运行 snapshot；旧 TXDBG command、布局和语义不变。D1 只保留编译与既有行为，不取得 SMP snapshot 资格。

**Task Contracts**

### 2.1: Establish shared deterministic placement policy

- Requirement/Scenario: placement 只使用已在线且可调度 hart；单 hart退化、多 hart固定 placement、稀疏/无效集合。
- Depends on: Iteration 000
- Targets: `crates/axtask/src/{api.rs,lib.rs}` read-only schedulable accessor；新增或既有 `kernel/src/drivers/*placement*` pure policy；host/model tests。
- Current behavior: kernel 看不到实际 schedulable mask；UART copier 使用 default full affinity，无角色 policy。
- Required behavior: policy 只消费有序去重的 schedulable set与 anchor/boot hart事实，返回有效 RX/TX singleton mask；1 hart可共置，至少 2 hart尽可能分离，未来 network roles 的占用顺序保持确定；空、越界或未发布集合 fail closed。
- Required changes: 公开 Acquire 读取的只读 schedulable snapshot；实现纯 policy及 1/2/3/4/8/16、非零 anchor、稀疏和无效集合 witness。
- Preserve: readiness owner仍在 `axtask`；普通调度可使用未分配 hart；具体 hart ID不成为长期 ABI。
- Forbidden: 使用 configured count代替 readiness；硬编码 hart0/1；创建 per-hart copier；引入 topology/hotplug框架。
- Test witness: 当前无 policy；新增 model tests 先暴露 full-mask/无效选择，再 GREEN。
- GREEN condition: 所有返回 bit均属于输入集合，每个角色为 singleton；退化和分离规则确定且无 panic。
- Verification: placement focused tests、axtask readiness suite、ordinary/`SMP=16` build。
- Stop when: kernel 无法在不改变 axruntime契约的情况下取得 schedulable snapshot，或 policy需要真板 reserved/topology事实。

### 2.2: Start exactly one pinned RX and TX copier after secondary-ready

- Requirement/Scenario: 多 hart固定 placement；UART copier唯一所有权与首次入队正确。
- Depends on: 2.1
- Targets: `crates/uart_16550/src/async_/driver.rs`；`kernel/src/drivers/{uart_init.rs,os_arceos.rs}`；`kernel/src/entry.rs`；lifecycle tests。
- Current behavior: driver只公开自行调用 `R::spawn` 的 wrappers；kernel启动不保存 handle，重复调用依赖 unsafe约定，首次入队为 full affinity。
- Required behavior: driver提供不自行 spawn 的显式 RX/TX future入口，旧 wrappers保留；kernel在现有 secondary-ready/benchmark后边界以入队前 singleton affinity各启动一次并保存 handle；重复启动在第二实例入队前失败。
- Required changes: 将 copier loop接到显式 future；在 adapter中用 `spawn_raw_with_affinity` 或等价首次入队 API；加入原子/Once lifecycle状态和 handle存储。
- Preserve: hardware/ring/IRQ先安装、benchmark先于 copier、task名称、旧 wrapper、应用启动前可用、唯一 raw reader/writer和SPSC方向。
- Forbidden: plain spawn后再 `set_cpumask`；提前到 benchmark之前；复制 loop；为验证创建第二 copier。
- Test witness: lifecycle/model test证明当前重复启动和handle缺失；compile/source guard证明wrapper委托与首次入队API。
- GREEN condition: 每方向恰好一个 task/handle，mask在首次 enqueue前生效；失败无第二实例或部分重复启动。
- Verification: UART crate tests、kernel host guards、ordinary/`SMP=16`/D1 builds、QEMU startup markers。
- Stop when: 显式 future无法保持现有 wrapper兼容，或唯一性需要改变SPSC API。

### 2.3: Add independent QEMU UART placement and progress snapshot

- Requirement/Scenario: UART 固定 placement可直接观测；复合 snapshot一致；旧 ABI兼容。
- Depends on: 2.1、2.2
- Targets: `kernel/src/drivers/uart_init.rs`及必要的纯 logic module；`kernel/src/syscall/fs/ctl.rs`；`crates/uart_16550`只读 ring/completion hooks；必要的只读 `axtask` IPI counters；ABI/model tests。
- Current behavior: TXDBG只含TX计数和四阶段completion；没有configured/schedulable、affinity、IRQ/copy hart、remote wake或RX/TX ring组合视图。
- Required behavior: 独立QEMU-only snapshot记录 configured/schedulable mask、RX/TX affinity、实际 IRQ hart mask、copier last/cumulative hart、RX occupancy、TX vacancy、四阶段 completion、remote enqueue/IPI/resume和 invalid-affinity rejects；同一 placement tuple一致读取。
- Required changes: 定义独立 `repr(C)` wire type和command；在实际IRQ/copier/placement路径记录；复合placement用锁或一致快照，纯telemetry使用Relaxed；snapshot读取不参与同步决策。
- Preserve: `UART_TXDBG_SNAPSHOT/RESET` 数值、布局与语义；network V1–V4；普通与D1构建不暴露QEMU资格接口。
- Forbidden: 以boot日志替代snapshot；修改旧TXDBG结构；从注册hart推测IRQ hart；使用revision/run identity。
- Test witness: layout/offset/source guards、tuple并发模型和counter单调性 tests。
- GREEN condition: snapshot能区分目标与实际hart、四阶段状态和远端因果；并发读取无混合placement tuple；旧ABI字节级不变。
- Verification: host ABI/model tests、kernel QEMU build、旧MS harness回归。
- Stop when: 所需远端因果只能通过改变scheduler决策语义获得，或新字段无法在事件源直接记录。

### 2.4: Close UART SMP wake, readiness and drain witnesses

- Requirement/Scenario: ISR到copier、ring到TTY、Full恢复和四阶段drain在SMP placement下保持。
- Depends on: 2.2、2.3
- Targets: `crates/uart_16550/src/async_/{isr.rs,driver.rs,ring_buffer.rs,device_ops.rs}` tests；kernel PollSet/TTY host witnesses。
- Current behavior: 单组件已有waker、ring、readiness和drain tests，但没有把固定placement、remote wake观测与既有状态机组合验证。
- Required behavior: witnesses覆盖 ISR→AtomicWaker→copier、RX ring→PollSet→TTY、TX Full→capacity恢复、check/register/recheck及 ring empty→copier inactive→staged zero→TEMT 四阶段；先观察至少一个旧SMP假设失配，再保持数据面语义GREEN。
- Required changes: 增加pure/model及现有fixture tests；只有witness暴露产品缺陷时才修正最小数据面代码。
- Preserve: short write、spurious wake可接受、drain不等于ring empty、ISR只ack/mask/wake。
- Forbidden: sleep作为同步协议；ISR搬运数据；unbounded retry；以最终console输出替代状态见证。
- Test witness: UART async suites及kernel host harness中的因果模型。
- GREEN condition: 无lost wake、Full后可恢复、readiness重查、drain仅在四阶段全满足时完成。
- Verification: `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async`及kernel host gates。
- Stop when: 失败来自新的scheduler/IPI设计而非既有Iteration 000契约。

### 2.5: Preserve QEMU console and D1 bounded fallback

- Requirement/Scenario: 单hart兼容、early console恢复路径、D1慢速THRE workaround保持。
- Depends on: 2.2、2.4
- Targets: `kernel/src/entry.rs`、`kernel/src/drivers/{uart_init.rs,d1_uart.rs}`、`crates/uart_16550/src/async_/driver.rs`及feature guards。
- Current behavior: QEMU early console独立；D1 TX有 fast retry、bounded slow-poll、有限self-wake后ISR wait。
- Required behavior: placement改动不影响early console；QEMU与D1异步UART均可编译；D1 workaround常量、边界与注释保持，除非D1专属witness直接证明需要修正。
- Required changes: 更新受startup API影响的QEMU/D1 adapter和guards；不重写D1 data path。
- Preserve: MMIO宽度、IER序列化、D1 direct-map、panic/early输出。
- Forbidden: 因QEMU未触发而删除D1 fallback；把QEMU placement事实外推到D1 SMP；扩大真板范围。
- Test witness: source guards、UART suites、QEMU和D1 feature build。
- GREEN condition: 两平台编译回归通过，early output路径仍可达，D1 fallback结构未被削弱。
- Verification: ordinary/`SMP=16` QEMU build和既有D1 build命令。
- Stop when: D1适配需要新的硬件事实或实板验证才能安全改变。

### 2.6: Run UART placement integration Gate

- Requirement/Scenario: Iteration 001全部 UART placement/observability Acceptance。
- Depends on: 2.1–2.5
- Targets: 本 Iteration 全量产品diff、tests和既有验证入口。
- Current behavior: UART可工作但copier placement不可判定，且无保存handle/独立snapshot。
- Required behavior: focused与回归测试通过；ordinary/`SMP=16`/D1 build通过；有界QEMU startup显示仅一个RX和一个TX copier在有效目标hart启动，无早期错误入队、ABI破坏或drain回归。
- Required changes: 运行分层Gate并完整Review diff；只修本Iteration契约内失败。
- Preserve: Iteration 000、network startup/owner、旧milestone行为和用户工作树无关修改。
- Forbidden: 进入正式UART数据面资格、network placement、migration或guest协议；以timeout退出码单独判PASS。
- Test witness: 2.1–2.5全部直接witness先通过，再运行integration。
- GREEN condition: policy/lifecycle/ABI/wake/drain gates全绿；startup必需marker和snapshot一致，禁止marker缺失。
- Verification: focused host tests、UART suites、旧harness、三类build、`SMP=16`有界startup、diff check、strict OpenSpec。
- Stop when: 需要network改动、新guest协议、迁移control或真板证据才能继续。

**Invariants**

- 每个driver实例只有一个RX ring writer/copier和一个TX ring reader/copier；迁移不得通过第二实例模拟。
- copier首次入队前已提交singleton affinity；目标bit必须已schedulable。
- hardware、ring、IRQ和benchmark先于copier；copier先于用户TTY依赖路径。
- ISR只确认/屏蔽/唤醒；register后recheck关闭lost-edge窗口。
- TX drain同时要求ring empty、copier inactive、staged zero和hardware TEMT。
- snapshot telemetry不驱动同步；placement复合状态必须一致。
- early console、旧TXDBG ABI、D1有界slow-poll和network行为保持。

**Non-goals**

- Network owner/runner placement与V5。
- UART copier migration、动态CPU offline/hotplug或通用topology。
- UART guest/host qualification protocol、正式RX/TX压力与性能结论。
- 真板SMP placement或IRQ routing声明。

**Acceptance**

- A1 / D4 / Task 2.1：对1/2/3/4/8/16、非零anchor和稀疏集合，policy只返回输入内的singleton；无安全候选fail closed。
- A2 / D5 / Task 2.2：RX/TX copier在secondary-ready与benchmark后各启动一次，首次入队affinity正确并保存handle；重复启动不创建第二实例。
- A3 / D6 / Task 2.3：QEMU独立snapshot直接报告placement、实际IRQ/copier hart、ring、remote wake和四阶段completion；tuple一致，旧TXDBG不变。
- A4 / Tasks 2.2–2.4：SPSC identity、ISR/waker、PollSet/readiness、Full恢复和drain因果在固定placement下通过。
- A5 / Task 2.5：early console和D1 bounded fallback保持；QEMU ordinary/`SMP=16`与D1 builds通过。
- A6 / Task 2.6：`SMP=16`有界UART startup中仅一个RX和一个TX copier使用有效目标hart，无早期错误入队、panic、第二endpoint或ABI回归。

**Verification**

- placement/lifecycle pure tests直接观察mask、退化、无效集合、一次启动与handle保存。
- UART crate和kernel host witnesses直接观察waker、ring readiness、Full恢复、四阶段drain及snapshot一致性。
- ABI/source guards确认新command独立、旧TXDBG布局不变、D1 fallback保留。
- ordinary/`SMP=16`/D1 builds确认feature与target集成。
- 有界`make justrun SMP=16`只验证QEMU startup与placement snapshot，不替代后续正式数据面资格；必需marker完整且无panic/page fault/重复copier才PASS。
- `git diff --check`、strict OpenSpec validation和full diff review。

**Gate 2 Readiness**

- No Missing requirements: PASS — Tasks 2.1–2.6映射到placement、lifecycle、snapshot、wake/drain、兼容与integration。
- Simplified requirements approved: PASS — 无新增裁剪；正式数据面资格按既有Map留在Iterations 004–005。
- Investigation complete: PASS — 已定位axruntime secondary-ready边界、kernel启动入口、driver wrappers/loops、axtask affinity/readiness、IRQ、ring、completion和ioctl。
- Design closed: PASS — policy owner、首次入队、唯一性、handle、snapshot一致性、平台边界与禁止方案明确。
- Tasks executable: PASS — 每项含targets、行为、witness、GREEN、verification和stop condition。
- Iteration plan ordered and balanced: PASS — 只交付UART固定placement与观测；network、migration和资格继续分轮。
- Traceability complete: PASS — D4–D6、Tasks 2.1–2.6、代码面和test witness形成链路。
- Verification sufficient: PASS — pure/model、crate、host、target build和有界startup分别观察目标。
- No identity-style evidence engineering: PASS — 没有revision/run/session协议。
- No material TBD for Act: PASS — command数值和局部类型命名可由Act在独立UART namespace内选择，不改变ABI边界。
- Change/Cycle consistency: PASS — 与现有spec、design和Iteration Map一致。
- Persisted Evidence mode valid: PASS — Mode为`none`，结果可低成本重跑并由Act Response承载。
- User plan approval: PASS — 用户于 2026-09-17 以“批准”明确批准 Iteration 001 Cycle 000 的执行契约。

**Persisted Evidence**

- Mode: none
- Budget: 命令、决定性输出、退出码、修改文件和符号写入Act Response；不创建Evidence目录。

**Risks and Notes**

- kernel `main()` 当前是secondary-ready边界；若axruntime依赖版本或启动顺序变化，先返回Plan，不在kernel增加第二套CPU barrier。
- snapshot中的remote因果必须来自事件源与scheduler只读counter，不能凭hart不同推断已发送/接收IPI。
- UART是console也是被测设备；本Iteration只做startup Gate，后续正式资格将使用独立判定，不能把console marker当数据面证明。
- D1为兼容build边界，不获得QEMU `SMP=16`结论。

## Act Response

- Status: pending

**Implemented**

Not started.

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

**Verification Evidence**

None

**Persisted Evidence**

None required

**Experience Candidates**

None

**Remaining Issues**

Awaiting Gate 2 user approval.

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

Gate 2 已通过；等待用户调用 `openspec-act` 执行本 Cycle。

**Iteration Plan Update**

None

**Next Cycle**

None

**Next Iteration**

None
