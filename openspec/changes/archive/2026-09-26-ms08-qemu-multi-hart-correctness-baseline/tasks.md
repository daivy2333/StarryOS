## 1. 共享 SMP 同步与调度基础

- [x] 1.1 在 `kernel/src/critical_section_policy.rs`、`kernel/src/lib.rs` 和 MS04/MS08 host harness 中将 local-IRQ-only policy 改为“本地 IRQ restore + 全局 Acquire/Release 锁 + per-hart 嵌套深度”；以双线程互斥、同 hart 嵌套、ISR entry、非 owner、underflow、overflow fail-closed 和既有 harness 验证，修正 release witness 的发布顺序，禁止在临界区内阻塞、yield 或取得驱动锁。
- [x] 1.2 从锁定的 `axtask 0.3.0-preview.2` 建立 `crates/axtask` 工作区副本，在 root 和 standalone `crates/axnet` manifest 中 patch 到同一副本；以 metadata、lock diff、license/version 和 UART/axnet 回归验证，禁止修改 registry 或 vendor 第二个 crate。
- [x] 1.3 在本地 `axtask` 中增加 Acquire/Release 发布的 schedulable run-queue 集合、只从已发布集合选择目标的普通 spawn，以及入队前 affinity spawn 和任意 task 的安全 mask 更新；以 configured=16/仅 primary schedulable、1/2/3/4/8/16、稀疏、空、越界、offline、未初始化和“失败不改变旧 mask”测试证明不会解引用未初始化 run queue，同时保持旧 spawn API 的 full-mask affinity。
- [x] 1.4 在本地 `axtask` 增加默认关闭的 `ipi` feature、唯一 S_SOFT handler 和 remote-ready IPI；以 mock tests 证明只有成功 `Blocked → Ready` 的 remote enqueue 发送一次 IPI，本地 wake 只提交 pending，重复 wake 不发送，handler 不直接切换 task。
- [x] 1.5 在 kernel `smp` feature 传播 `axtask/ipi`，并以 feature tree/source guard 确认不同时启用 `axruntime/ipi`/`axipi`；注册冲突、未初始化 current task 和无单目标 IPI 均是停止条件。
- [x] 1.6 为 RISC-V QEMU 增加最终配置写入，将 PLIC MMIO range 从 `0x0c00_0000/0x21_0000` 修正为 `0x0c00_0000/0x60_0000`，保留其他 MMIO ranges；`EXTRA_CONFIG` 保持非重复项合并能力，但不得被描述为可覆盖已有 `devices.mmio-ranges`。以生成配置、重复 key 拒绝、非 QEMU 不变和 `SMP=16` 早期启动越过 PLIC/scheduler init 验证。
- [x] 1.7 运行 foundation 集成 Gate：critical-section/axtask/config focused tests、`uart_16550 --features async`、axnet ordinary/qemu-diagnostics、`make host-test` 可执行部分、`make build SMP=16` 和有界 `make justrun SMP=16 NET=n`；任一非环境失败停止，不得进入驱动 placement。

## 2. UART 固定 placement、唯一性与观测

- [x] 2.1 为 UART 建立从 online/schedulable 集合选择 RX/TX copier singleton affinity 的纯 policy，与共享 policy 的角色占用一致；以 1/2/3/4/8/16、非零 boot 和稀疏/无效集合验证。
- [x] 2.2 在 `uart_16550::AsyncUartDriver` 中提供显式 RX/TX copier future 入口，保留既有 start wrapper；在 `kernel/src/drivers/uart_init.rs` 与 adapter 中改为 secondary-ready 后以入队前 affinity 各启动一次并保存 handle。以重复启动 fail-closed、唯一 reader/writer 与 SPSC 身份测试验证。
- [x] 2.3 对 QEMU UART 增加独立 snapshot/control，记录 configured/online mask、RX/TX affinity、实际 IRQ hart mask、copier last/cumulative hart、ring occupancy/vacancy、TX completion 四阶段、remote enqueue/IPI/resume 和无效 affinity 拒绝数；保持既有 TXDBG ABI 不变，复合 snapshot 不得撕裂。
- [x] 2.4 建立 UART host/model witnesses，覆盖 ISR→AtomicWaker→copier、ring→PollSet→TTY waiter、TX Full→容量恢复、event/register/recheck 和 `flush/tcdrain` 四阶段；先使失配 SMP 路径 RED，修改后 GREEN。
- [x] 2.5 保留 early console 与 D1 有界 TX slow-poll workaround，并对 QEMU 与 `lichee-d1-async-uart` 特性分别执行 compile/regression Gate；禁止以 QEMU 未触发 workaround 作为删除依据。
- [x] 2.6 运行 UART integration Gate：focused tests、`uart_16550 --features async`、kernel ordinary/`SMP=16` build 和有界 QEMU UART startup；任一第二 copier/endpoint、早期错误入队、ABI 破坏或 drain 契约回归必须停止。

## 3. 网络固定 placement 与直接观测

- [x] 3.1 将共享 placement policy 接入 `crates/axnet`，为 queue owner 和 stack runner 计算 singleton affinity；小集合合法退化，`SMP=16` 下与 UART copier 分配不同后台 hart。
- [x] 3.2 调整 `init_network`、`stack_runner`、`async_rx` 和 kernel IRQ adapter：Service 先安装，secondary-ready 后启动 pinned runner，IRQ 注册后启动 pinned 唯一 owner，保存 handle；保留 IRQ 注册失败的既有有界 fallback。
- [x] 3.3 以 V4 为字节级前缀增加 QEMU-only V5，追加 configured/online mask、owner/runner affinity、实际 IRQ/task hart、remote enqueue/IPI/resume、迁移和无效 affinity 字段；V1–V4 command/布局/语义不变。
- [x] 3.4 增加 single-flight timer-disabled remote-wake witness：目标 task 关闭本 hart timer 后 park，另一 hart wake，完成/取消/超时路径都恢复 timer；以状态机 host tests 和 target build 验证。
- [x] 3.5 运行 network placement Gate：policy/ABI/witness、MS03/MS04/MS07 harness、两套 axnet tests、ordinary/`SMP=16` build；任一第二 owner、早期 spawn、旧 ABI 破坏或 timer 未恢复必须停止。
- [x] 3.6 修复 UART TX copier 在 `register_waker` 到真实 `Poll::Pending` 之间的 lost-wakeup：注册 ring waker 后重查 ring，发现新数据时 self-wake/retry；空发射器停放路径保留 THRE 兜底。以真实 copier future 的确定性交错测试和重复 `SMP=16` startup 证明 publication 不再卡在 ring、remote-ready IPI 或本地重试最终产生进度，同时保持 SPSC、四阶段 drain、early console 与 D1 workaround。

## 4. 受控迁移与 ordering 闭合

- [x] 4.1 在工作区 `axtask` 以仓库自有 `AxCpuMask` 新类型封装私有的 `cpumask 0.1.0`，覆盖现有构造、集合运算、迭代和索引调用面；越界读取在 debug/release 均返回“不在集合”，越界写入返回错误且不修改 mask，原始 registry 类型不得从公开 API 泄漏。以容量边界、`usize::MAX`、写失败不变性和 release-mode focused tests 验证；不 fork/patch Cargo registry。
- [x] 4.2 在 QEMU-only control 中将既有 UART RX/TX copier 的 mask 从 singleton 扩展到两个有效 hart，通过自然 block/wake 观察迁移；验证 task identity、SPSC endpoint、ring index 和 staged state 不变，然后恢复固定 mask。
- [x] 4.3 以同样方式迁移 network owner/runner；验证 owner lifecycle、descriptor/slot/ticket 账本、runner generation 和 readiness 不变，旧 hart 无第二实例。
- [x] 4.4 审计 UART completion/IER cache/placement 与 network cause/lifecycle/fault/ticket/epoch/readiness 共享状态；将 telemetry 保持 Relaxed，publish/observe 使用 Release/Acquire，同步 RMW 使用 AcqRel，复合 tuple 使用锁或一致快照。对 QEMU `ArceOsUartPort::ier_cache` 必须以测试证明现有 UART `SpinNoIrq` 将 cache RMW 与 MMIO write 置于同一串行化边界；只在 witness 暴露 lost update 时修正。D1 adapter 的 local-IRQ-only RMW 不取得 SMP 资格，也不在本 QEMU change 中扩大修改。
- [x] 4.5 运行 mask-safety/migration/ordering Gate：`AxCpuMask` debug/release 越界行为、无效 affinity fail-closed、register/publish、generation wrap、terminal-before-wake、snapshot 一致性和 100 轮迁移 stress 全绿；任一原始 mask API 泄漏、垃圾 bit、非法写入、丢 wake、第二角色、复合状态撕裂或 IER lost update 必须停止。

## 5. MS08 guest/host 协议与自动判定

- [x] 5.1 新增 UART SMP guest probe 和 host serial harness，通过 QEMU serial socket 注入/捕获带序号 payload，定义 environment、placement、RX、TX Full→恢复、readiness、`tcdrain`、quiet 和 copier migration cases；不得 sleep-poll 或只靠 console PASS marker。
- [x] 5.2 扩展 network SMP probe，定义 environment/placement、timer-disabled wake、双向压力、Full→恢复、readiness/quiet、owner/runner migration、reset/I/O 和 link 交错 cases。
- [x] 5.3 新增纯输出 validator，分别严格检查 UART 和 network case 顺序、placement、IPI 因果、payload/资源守恒、quiet delta、migration identity 和终态；以缺 marker、重复 case、timer-only 进度、数据损坏、owner/copier drift 和 stale epoch 负向 fixtures 自测。
- [x] 5.4 建立协议/schema/source guard 和 Makefile build/test 入口，保持 MS03–MS07 旧 probe/validator 不变；禁止 validator 启动 QEMU、建立 revision/run identity 或用工具自证工具。

## 6. `SMP=16` 网络固定 placement 资格

- [x] 6.1 SKIPPED：用户于 2026-09-26 确认 UART 正常，并明确不再执行 UART 专项 runtime；console 只作为本轮网络测试基础设施。风险：本轮不新增 UART 异步 RX、背压、drain 或迁移的资格结论。
- [x] 6.2 将现有 network guest probe、TCP/UDP peer 和 validator 收敛为固定的 `SMP=16` 网络 profile：直接检查 configured/online 集合、owner/runner singleton affinity 与实际 poll hart、timer-disabled remote wake、TCP/UDP 双向、Full→恢复、poll/select/epoll readiness、quiet 和 descriptor/slot/ticket 终态守恒；移除最终 profile 的 owner/runner migration、reset/link case，不修改产品网络语义。
- [x] 6.3 运行网络测试代码/服务器自测、受影响的静态 guest build、`SMP=16` kernel build，以及一次有界 network-only QEMU session；guest、peer 和 validator 均退出 0 后做 full diff review。固定 placement、跨 hart wake、数据完整性或资源守恒缺失时不得用 console 输出、编译成功或历史单 hart结果替代。

## 7. 组合压力、恢复交错与最终回归

- [x] 7.1 SKIPPED：用户于 2026-09-26 将最终结论收敛到网络，不再执行 UART+network 组合压力。
- [x] 7.2 SKIPPED：现有运行已观察 reset 与 link 恢复；用户明确不要求再次执行多 hart reset/link 交错，本轮不新增该项资格结论。
- [x] 7.3 SKIPPED：用户明确不逐项重跑旧阶段。最终只运行 Task 6.3 的直接网络 Gate、格式/结构检查和 full diff review。

## Iteration Plan

### Iteration 000: Shared SMP synchronization and scheduler substrate

- Tasks: 1.1–1.7
- Depends on: None
- Stable baseline: kernel critical-section 跨 hart 安全；workspace `axtask` 只选择已发布的 schedulable run queue，并支持入队前 affinity 和 remote-ready IPI；UART/网络尚不改变 placement。
- Verification boundary: focused 互斥/affinity/IPI/config tests、UART/axnet 回归、`SMP=16` build 及早期启动通过。
- Diagnostic boundary: critical-section、task 构造/run queue、S_SOFT/IPI、feature 传播或 PLIC 映射。
- Non-goals: 驱动 placement、diagnostic ABI、迁移和数据面 runtime。

### Iteration 001: Deterministic UART placement and observability

- Tasks: 2.1–2.6
- Depends on: Iteration 000
- Stable baseline: RX/TX copier 在 secondary-ready 后各以 singleton affinity 启动一次，SPSC 身份、snapshot 和四阶段 drain 观测稳定。
- Verification boundary: policy、copier lifecycle、UART SMP host witnesses、QEMU/D1 compile 和 `SMP=16` UART startup 通过。
- Diagnostic boundary: UART startup、SPSC endpoint、waker/PollSet、IER 序列化、completion 或 snapshot。
- Non-goals: network placement、UART runtime 资格和迁移。

### Iteration 002: Deterministic network placement and observability

- Tasks: 3.1–3.6
- Depends on: Iteration 001
- Stable baseline: network owner/runner 在 secondary-ready 后固定，V5 和 timer-disabled wake witness 可用；UART TX copier 的 park/register 交错不再丢失 producer wake。
- Verification boundary: placement/startup/fallback、V5 ABI、witness、UART park 交错和重复 `SMP=16` startup 通过。
- Diagnostic boundary: network placement、启动顺序、IRQ 注册、V5、timer 恢复或 UART TX register/recheck/THRE 停放协议。
- Non-goals: migration、guest protocol、完整 runtime 资格，以及 UART park 修复之外的 UART 行为扩张。

### Iteration 003: Controlled migration and ordering closure

- Tasks: 4.1–4.5
- Depends on: Iteration 002
- Stable baseline: `AxCpuMask` 的边界行为在 debug/release 均 fail closed；UART copier 和 network owner/runner 可通过扩展 mask 自然迁移，默认仍为固定 placement，共享状态 ordering 闭合。
- Verification boundary: mask 容量边界与 release 越界、无效 affinity、逻辑身份、IER/cache、publication 和 snapshot stress 全绿。
- Diagnostic boundary: 本地 mask 封装、affinity 更新、wake 后入队、SPSC/owner identity、内存序或快照。
- Non-goals: guest/host protocol 和 QEMU runtime 资格。

### Iteration 004: MS08 UART and network qualification protocols

- Tasks: 5.1–5.4
- Depends on: Iteration 003
- Stable baseline: UART serial harness、network probe 和纯输出 validator 对全部 case 形成唯一可判定协议。
- Verification boundary: guest static build、host fixtures、validator negative self-tests、schema/source guards 和旧 probe 回归通过。
- Diagnostic boundary: serial transport harness、guest protocol、snapshot ABI 或 validator 语法/判定。
- Non-goals: 完整 QEMU 资格和 recovery 交错。

### Iteration 005: Network-only SMP=16 fixed qualification

- Tasks: 6.1–6.3
- Depends on: Iteration 004
- Stable baseline: QEMU VirtIO-MMIO 在 `SMP=16` 固定 placement 下具备已验证的跨 hart wake、TCP/UDP 双向、Full→恢复、readiness/quiet 和资源守恒。
- Verification boundary: 一个 network-only runtime 由 guest、peer 和 validator 直接接受；当前 affinity、实际 poll-site hart、IPI 因果、payload 和资源账本均为通过条件。
- Diagnostic boundary: network guest/peer protocol、IRQ/owner/runner placement、remote wake、数据面、backpressure、readiness/quiet 或资源账本。
- Non-goals: UART 专项、受控迁移稳定性、组合压力、reset/link 重跑、旧阶段逐项回归、真板和性能。

## Balance Audit

- Iteration 000 只交付全局原语和 16-hart 启动前提；UART 与网络都依赖该基线，拆开后无法独立支撑驱动实施。
- Iteration 001 与 002 原则上分开 UART 和 network 的所有权/观测故障域。Iteration 002 只额外接纳 Task 3.6：该已取证 UART park 竞态直接使本 Iteration 的重复 `SMP=16` startup Gate 不确定，用户已明确授权在当前 Iteration 修复；不因此重开其他 UART 范围。
- Iteration 003 合并两类任务的 migration 与 ordering，因为它们共同验证同一 scheduler/critical-section 基线，但测试仍分别维护 SPSC 和 queue-owner 不变量。
- Iteration 004 建立的完整协议保留；Iteration 005 按用户修订后的结论裁剪为 network-only profile，复用既有 probe、peer 和 validator，不建立第二套测试服务器。
- 原 Iteration 006 的 Tasks 7.1–7.3 已按用户决定逐项标记 SKIPPED；修订后的 Iteration 005 是最后一个执行单元。
