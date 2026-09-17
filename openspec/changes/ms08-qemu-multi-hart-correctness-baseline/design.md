## Context

见 [proposal.md](proposal.md) 的动机和 [qemu-multi-hart-async-io-correctness](specs/qemu-multi-hart-async-io-correctness/spec.md) 的行为契约。当前 revision 为 `04fc3ce5101ebf6a2e30b91e7fc6512c2a17f8a3`，分支为 `k3`。

现有实现具备下列基础：

- `SMP=<n>` 会设置平台最大 CPU 数、启用 `axfeat/smp` 并传给 QEMU `-smp <n>`。
- QEMU 默认路径初始化 NS16550 异步 UART，注册 IRQ，在 startup benchmark 后各启动一个 RX/TX copier；TTY 保持唯一 raw reader/writer。
- UART RX/TX ring 使用 Acquire/Release SPSC 索引，`flush/tcdrain` 已跟踪 ring empty、copier active、staged bytes 和 hardware empty 四个完成条件。
- VirtIO-net IRQ 由 primary hart 注册，只执行 cause/ack/telemetry/event publish；唯一 queue owner 执行 descriptor service 和 recovery，stack runner 执行协议栈与 readiness。
- queue event、stack event、lifecycle、fault、ticket 和 epoch 已使用原子或锁表达同步角色，`Service → SocketSet` 锁序稳定。
- MS07 已取得单 hart恢复资格；其最终证据明确不证明 SMP。

当前限制决定了本设计不能只增加 QEMU 参数：

- kernel `critical-section` 只关闭当前 hart IRQ。Embassy `AtomicWaker` 用该临界区保护非原子 waker cell，因此多 hart并发 register/wake 不满足其全局互斥契约。
- registry `axtask` 在 task 入队后才允许当前 task设置 affinity；普通 spawn 先以全 CPU mask选择 run queue。stack runner 又在 secondary run queue 初始化前生成。
- `AsyncUartDriver::start_rx_copier/start_tx_copier` 通过 `OsRuntime::spawn` 立即普通入队，`ArceOsRuntime` 只调用 `axtask::spawn_with_name`，无法在首次 run-queue 选择前提交 copier affinity 或保存 task handle。
- UART ISR 使用全局 `RX_WAKER`/`TX_WAKER`/`DRAIN_WAKER`；RX/TX ring 内部的 `UnsafeCell<Reader/Writer>` 安全性依赖逻辑唯一 SPSC 角色，多 hart 迁移不能扩张成多 producer/consumer。
- QEMU `ArceOsUartPort::update_ier` 在 UART `SpinNoIrq` guard 内完成 cache RMW 与 MMIO write，计划将通过 SMP witness 锁定该边界，不预设需要重写。D1 adapter 只关闭本 hart IRQ；本 change 不为 D1 取得 SMP 资格。
- `AxWaker` 每次 wake 重新选择 run queue，remote unblock 不发 IPI。目标 hart可能只在周期 timer后调度 ready task。
- V4 snapshot 不包含 IRQ/task hart、affinity 或 remote wake/IPI 状态，既有 probes 也都是单 hart协议。

新鲜基线：`cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` 以 62 个 unit tests、8 个普通 doc-tests 和 10 个 compile-fail doc-tests 全部通过。axnet ordinary `474/474`、qemu-diagnostics `506/506` 通过；`make build SMP=8` 与后续 `make build SMP=16` 均成功生成 QEMU binary。`SMP=16` 启动探针中 OpenSBI 枚举 hart 0–15、选择非零 boot hart 11，StarryOS 报告 `smp = 16` 后在现有早期初始化路径以 `current task is uninitialized` panic；该结果只登记为 Iteration 000 的入口故障，不判定为 16-hart 专属原因。`make host-test` 在 MS03/MS04/MS07 Rust harness 和前置 C/Python tests 通过后，于 UDP loopback socket 创建处被沙箱 `EPERM` 阻塞，未形成产品失败。

目标拓扑依据：K3 静态资料区分 8 × X100 + 8 × A100 的 16-hart AP 域与 2 × RT24 的独立 RP 域，AP CLINT/IMSIC 覆盖 hart 0–15；但目标 CoM260 在特定固件下实际启动、在线和可调度的 hart 集合仍未由真板证据确认。因此本 change 以 `SMP=16` 模拟 AP 数量，只验证同构 QEMU 软件并发。

## Goals / Non-Goals

**Goals:**

- 先建立可由全 kernel共享的 SMP-safe critical-section 和 event-driven remote scheduler wake，再让 UART 和 NIC 依赖这些原语。
- 以一个适用于任意在线 hart集合的 policy 计算角色 affinity；QEMU 只提供连续 `0..cpu_num` 的当前平台事实。
- 让 UART RX/TX copier 在 secondary-ready 后以入队前 affinity 各启动一次，保留 SPSC、readiness 和四阶段 drain 语义。
- 让 queue owner 和 stack runner 在 secondary harts就绪后以入队前 affinity 启动，并保留唯一 owner、register-before-start 和 polling fallback 边界。
- 以 append-only network V5、独立 UART snapshot/control 和有限 QEMU control 直接观察 placement、remote wake、task poll、迁移和恢复结果。
- 复用 UART benchmark/`tcdrain`、MS03 串口激励以及 MS04–MS07 网络 probes 的行为判据，不建立第二套驱动。

**Non-Goals:**

- 不实现 CPU hotplug、运行期自动负载均衡或通用 scheduler topology framework。
- 不把一张硬件 queue 拆成多个 owner，不实现 multiqueue、RSS 或多 NIC placement。
- 不创建第二 UART copier/reader/writer，不将 SPSC ring 改成 MPMC。
- 不实现 UART/NIC IRQ 动态 affinity 管理；本 change 记录实际 IRQ 执行 hart。
- 不修改 socket API、packet-slot 容量、VirtIO queue contract 或 MS07 epoch/recovery 语义。
- 不替换 early console，不删除 D1 TX slow-poll workaround，不宣称 D1/K3 UART 硬件通过。
- 不修改 Cargo registry；不模拟 X100/A100 异构能力或 RT24 RP 域；不宣称 K3 AIA delivery、真板 online hart、DMA/cache、PCI/DWMAC 或性能资格。

## Decisions

### D0：工作区 QEMU 配置覆盖完整 PLIC MMIO 窗口

**Decision**：为 `riscv64-qemu-virt` 增加工作区自有的最终配置写入，只把平台已声明的 PLIC MMIO range 从 `0x0c00_0000/0x21_0000` 修正为 QEMU DT 与上游配置注释均给出的 `0x0c00_0000/0x60_0000`，其余 MMIO ranges 保持相同。`axconfig-gen 0.2.1` 会先合并全部 specification，再统一应用 `-w`；specification 遇到重复 key 会报错。因此 `EXTRA_CONFIG` 继续用于非重复配置项，不能覆盖 `devices.mmio-ranges`，PLIC 平台事实由最后的 `-w` 修正。

**Reason**：PLIC supervisor context地址随 hart ID增长。16-hart探针由 boot hart 11 执行 `init_percpu` 时访问超出 `0x21_0000` 映射并触发早期 page fault；page-fault handler随后因 scheduler尚未初始化而以 `current task is uninitialized` 掩盖原始故障。缩回8 hart只隐藏平台映射缺口。

**Impact**：`make build/run SMP=16` 无需修改 Cargo registry即可访问全部16个QEMU hart的PLIC context；overlay只适用于默认RISC-V QEMU平台，不改变D1/VF2或未来K3配置。早期启动Gate必须证明不再发生该page fault及其派生panic。

**Compatibility correction**：Cycle `001-replan` 曾要求 `EXTRA_CONFIG` 在 PLIC 修正之后取得最终优先级。实际工具不支持 specification 覆盖已有 key；现实现反而由最终 `-w` 覆盖 `EXTRA_CONFIG`。本 change 不新增配置预处理器或第二套 override 协议。该计划修订在 Cycle `002-replan` 获得用户批准前不进入执行就绪状态。

**Alternatives**：

- 保持 `0x21_0000` 并只使用 `SMP=8`：拒绝，不符合已批准的目标AP规模。
- vendor整个 `axplat-riscv64-qemu-virt`：拒绝，只需修正配置事实，会无谓扩大维护面。
- 只让早期page-fault handler避开 `current()`：拒绝，只会暴露原始未映射PLIC fault，不解决硬件上下文访问。

### D1：工作区 vendor `axtask`，不在驱动层模拟远端调度

**Decision**：从锁定的 `axtask 0.3.0-preview.2` 建立 `crates/axtask` 工作区副本，并在根 manifest 与独立的 `crates/axnet` manifest 中使用 `[patch.crates-io]`。扩展包括 run queue readiness 发布、入队前 affinity spawn、安全 affinity 更新、remote ready queue IPI 和必要 telemetry/tests。primary/secondary scheduler 只在对应 `RUN_QUEUES[cpu]` 写入完成后以 Release 发布 schedulable bit；选择 run queue 时以 Acquire 读取该集合，并使用 task affinity 与 schedulable 集合的交集。普通 spawn 保留配置的 full mask，但首次及后续选择不得解引用尚未发布的 run queue；显式 affinity spawn/update 则拒绝任何未 schedulable bit，失败不改变旧状态。

**Reason**：task 必须在选择 run queue 前带有最终 affinity；UART 或网络在 spawn 后纠正会留下 task 已进入未初始化或错误 run queue 的窗口。`axhal::cpu_num()` 是 configured count，不代表 secondary run queue 已初始化；仅用它校验 mask 会让早期普通 spawn 解引用 `MaybeUninit`。远端 ready task是否触发目标 CPU 调度也是 scheduler 责任，不能由每个 waker caller重复实现。

**Impact**：所有使用 `axtask` 的 root 构建会采用工作区副本；独立 axnet tests 也必须指向同一副本。vendor 基线必须保持上游版本、license、现有 features 和 API，新增 feature 默认关闭。

**Alternatives**：

- spawn 后调用 `set_current_affinity`：拒绝，task 已经入队并可能运行。
- 只保存 `AxTaskRef` 后调用 `set_cpumask`：拒绝，不能关闭首次入队竞态。
- 在每个驱动事件后直接 `send_ipi`：拒绝，会把 run queue选择和 remote-ready 判定复制到驱动层。
- 仅把早期 network runner 固定到 primary：拒绝，这会用 driver placement 掩盖 scheduler 对未初始化 run queue 的通用错误。
- 把 configured CPU count 当作 schedulable 集合：拒绝，secondary scheduler 初始化发生在 `cpu_num()` 已返回最终数量之后。
- 原地修改 Cargo registry：项目规则禁止。

### D2：`axtask/ipi` 直接承载 reschedule IPI

**Decision**：本地 `axtask` 增加默认关闭的 `ipi` feature，依赖 `preempt` 和 `axhal/ipi`。primary scheduler 初始化时唯一注册 software-interrupt handler；成功把 blocked task放入 remote run queue 后，向该 run queue 的 hart发送 IPI。handler 只记录 telemetry并为当前 task设置 reschedule pending，通用 IRQ guard退出时执行正常 preemption。kernel `smp` feature 显式启用 `axtask/ipi`，不同时启用 `axruntime/ipi`/`axipi`，避免争用同一个 S_SOFT handler。

**Reason**：QEMU platform 已在每个 hart启用 supervisor software interrupt，并提供 SBI `send_ipi`；现有 IRQ wrapper 在 handler返回时释放 `NoPreempt` guard，正好承载 reschedule。该路径不依赖周期 timer，也不需要引入另一套 IPI message dispatcher。

**Impact**：`AxWaker` 的成功 wake 使用 reschedule 语义：本地 ready 设置 preempt pending，远端 ready发送一次 IPI。重复 wake未完成 `Blocked → Ready` 时不得重复发送 IPI。

**Alternatives**：

- 启用 `axfeat/ipi` 和 `axipi`：拒绝，本 change 只需要 scheduler reschedule，额外 dispatcher 未在当前依赖图中锁定，也会占用相同 handler。
- 接受下一次 100Hz tick：拒绝，不满足已批准的事件直接唤醒契约。
- IPI handler内直接切换 task：拒绝，绕过既有 IRQ/preemption guard和 run queue路径。

### D3：critical-section 使用本地 IRQ 状态、全局锁和 per-hart嵌套深度

**Decision**：保留 `restore-state-bool` ABI。acquire 先保存并关闭本 hart IRQ；该 hart嵌套深度由 `MAX_CPU_NUM` 大小的原子数组记录，深度从 0 变 1 时以 Acquire取得全局锁。release 先递减深度，变为 0 时以 Release释放全局锁，再仅按匹配 restore state恢复本地 IRQ。

**Reason**：仅全局自旋锁会在同 hart嵌套时死锁，仅 IRQ mask不能排除其他 hart。固定数组不依赖 allocator，且 QEMU hart ID已受平台最大 CPU 数约束。

**Impact**：所有使用 `critical-section` 的 kernel `AtomicWaker` 获得相同 SMP语义；单 hart和 ISR restore行为保持。无效 hart ID、depth underflow/overflow 或非 owner release必须 fail closed并在测试中可见。

**Alternatives**：

- NIC-local waker replacement：拒绝，kernel其他 AtomicWaker仍不安全。
- 普通非重入 spinlock：拒绝，现有嵌套 critical-section会自锁。
- 依赖 telemetry depth：拒绝，嵌套深度参与同步，不能使用 Relaxed观测 counter代替状态。

### D4：共享 placement policy 分配逻辑角色，不推测 IRQ 路由

**Decision**：kernel 层增加纯 policy，输入为有序、去重的 online/schedulable hart 集合和当前 boot/registration hart，输出 UART RX copier、UART TX copier、network owner 和 network runner 的 singleton affinity。QEMU 在全部 secondary-ready 后使用已初始化的 `0..axhal::cpu_num()` 构造集合；小集合允许必要共置，三个及以上 hart 尽可能分离具有直接通知关系的角色，`SMP=16` 下四个后台角色使用不同 hart。实际 UART/NIC IRQ 执行 hart 通过 telemetry 记录，不从注册 hart 推测；资格必须直接观察至少一条远端 ISR→task 路径。

**Reason**：该算法不依赖 hart数量或连续 ID，host/model可覆盖稀疏集合；QEMU 当前只具备 CPU count事实，不虚构真板 reserved/topology信息。

**Impact**：具体 hart ID 只属于当次 snapshot，不是硬编码契约。额外 hart 继续供应用和普通内核任务使用。未来 K3 adapter 必须以目标 DTB、固件/HSM 与内核 bring-up 结果重新过滤 AP 集合，且不得把 RT24 纳入该集合。

**Alternatives**：

- 固定 hart0/1/2：拒绝，不能适配稀疏或平台保留 hart。
- 默认全 mask自由迁移：拒绝，不能形成确定性 Gate且混入 migration故障域。
- 为所有 hart创建 owner/copier：拒绝，违反 queue 和 SPSC 唯一身份。

### D5：设备安装与后台 task 启动解耦

**Decision**：UART hardware/ring/IRQ 初始化不再立即通过 `OsRuntime::spawn` 启动 copier；`AsyncUartDriver` 提供显式、仅可各执行一次的 RX/TX copier future 入口，kernel adapter 用入队前 affinity 启动并保存 handle。既有 `start_rx_copier/start_tx_copier` 作为兼容 wrapper 保留，其唯一启动安全契约不变。`init_network` 同样只安装 Service/registries，QEMU kernel 在 secondary-ready 后启动 pinned runner/owner 并保存 handle。

**Reason**：现有 UART copier 和 stack runner 都可能在 secondary run queue 初始化前以 full mask 入队。kernel 入口是当前唯一同时知道 secondary-ready、设备初始化边界和 QEMU diagnostic 范围的位置。

**Impact**：需要调整现有 UART startup 和“Service 安装即启动 runner”测试；应用启动前所有 copier/runner 仍必须可用。UART 重复启动与网络 lifecycle CAS 都 fail closed。

**Alternatives**：

- 在早期 spawn 后自迁移：拒绝，首次 run queue选择仍依赖未初始化状态。
- 修改 axruntime启动顺序：拒绝，影响所有设备和平台，超过本 change 边界。

### D6：UART 和网络分开观测，旧 ABI 保持不变

**Decision**：network 新增 QEMU-only V5 ioctl/wire type，以 V4 为字节级前缀追加 placement、实际 IRQ/task hart、remote enqueue/IPI、迁移和无效 affinity 字段。UART 使用独立 QEMU-only snapshot/control，记录 RX/TX copier affinity、最近/累计 hart、IRQ hart mask、ring occupancy/vacancy、copier active/staged/hardware-empty、waker/IPI 因果和迁移计数。既有 network V1–V4 和 UART TXDBG command 的布局与语义不变。

**Reason**：两个驱动的所有权和完成状态不同，不应强行塞入同一 snapshot。资格必须直接证明角色实际 placement、远端 wake 和 task 重新 poll；普通输出或流量成功不足。

**Impact**：纯 telemetry用 Relaxed；同一 placement tuple通过一个锁或一致快照读取。V5 不进入普通/真板构建。

**Alternatives**：

- 只打印 boot日志：拒绝，不能与事件前后 snapshot组合，也不能验证迁移。
- 修改 V4：拒绝，会破坏 MS07 guest binary和validator。
- 建立 revision/run identity协议：拒绝，属于身份型证据工程，且不直接证明行为。

### D7：固定 remote-wake witness临时关闭目标 hart timer

**Decision**：QEMU diagnostics 提供一个有限状态 control：创建一次性 task并以 singleton affinity固定到目标 hart；该 task在目标 hart关闭本地 timer IRQ、发布 armed状态并 park。另一 hart触发其 waker后，scheduler必须发送 IPI；目标 handler记录 receive并调度 task，task恢复 timer IRQ、提交完成状态后退出。每次操作有固定 deadline、single-flight和失败后 timer恢复路径。

**Reason**：单看最终流量不能排除 100Hz timer救活。该 witness直接观察“remote ready enqueue → IPI send → target IPI receive → task resume”，并在关键窗口移除 timer替代路径。

**Impact**：仅 `qemu-diagnostics` 构建包含 control；不得复用 NIC descriptor、修改网络 owner或长期关闭 timer。Act 必须证明失败、取消和超时都恢复 timer。

**Alternatives**：

- 以小于 10ms 的时间阈值推断：拒绝，host/QEMU调度抖动会产生假结果。
- 完全关闭所有系统 timer：拒绝，扩大故障面并破坏 deadline。
- 只用 host mock验证 IPI调用：拒绝，不能证明 QEMU SBI/handler路径。

### D8：迁移通过 affinity扩展和自然 block/wake完成

**Decision**：固定 placement Gate 通过后，QEMU-only control 分别把保存的 UART RX/TX copier、network owner 或 runner task mask 从 singleton 扩展到两个已验证 online hart。运行中 task 可完成当前有界 round；下一次 block/wake 由 scheduler 选择允许 run queue。control 拒绝空、offline、reserved、未初始化或不含原安全集合的 mask，失败不改变旧 mask。

**Reason**：不需要即时抢占迁移，也不修改逻辑 owner。自然 wake迁移覆盖当前 scheduler真实路径，并与固定资格场景隔离。

**Impact**：UART 迁移必须保持同一 copier 身份、ring indices 和 staged state；网络迁移必须保持 owner lifecycle 和资源账本。完成后恢复固定 mask。迁移不能替代固定 placement runtime。

**Alternatives**：

- 新建第二 owner/copier 验证迁移：拒绝，改变被测语义。
- 要求运行中的 task立即迁出：拒绝，需要更广泛的跨 CPU stop/migration协议。

### D9：SMP=16 验证分为原语、固定数据面、迁移和恢复交错

**Decision**：host/model 先覆盖 critical-section、placement、remote wake policy、ordering 和迁移状态，拓扑包含 1/2/3/4/8/16、非零 boot/IRQ hart 及稀疏集合；target build 随后证明 feature/ABI 集成。正式 QEMU runtime 只使用 `SMP=16`：先运行 timer-disabled wake witness，再分别运行 UART 固定 placement 的 RX/TX/Full/readiness/drain/quiet 和网络固定 placement 的双向/Full/readiness/quiet，然后运行独立 migration、UART+网络组合压力、网络 reset/I/O/link 交错与单 hart 回归。UART 作为被测 console 时，必须以 host timeout、内存 snapshot 和独立退出结果补充串口 marker。

**Reason**：按故障域排序可把全局原语、UART、网络和 recovery 失败分别定位；先运行组合压力会无法判断是 console 失效、scheduler 失效还是网络停滞。

**Impact**：任一自动 Gate失败即停止后续 QEMU资格；任一 runtime case失败不得临时修改产品后继续计入同一结果。单 hart回归不计入多 hart通过。

**Alternatives**：

- 一次综合 soak：拒绝，无法判定竞态来源。
- 同时要求 SMP=2/4/8/16 runtime：拒绝，用户已批准只使用目标规模 `SMP=16` 作为正式多 hart runtime，host/model承担小拓扑边界。
- 用历史 UART 或 MS07 Evidence 替代：拒绝，其范围明确为单 hart。

## Risks / Trade-offs

- **[vendor `axtask` 扩大维护面]** → 保持版本和上游文件结构，patch仅含 affinity/IPI/telemetry；邻接 root、axnet standalone和 target build均回归，禁止顺带清理。
- **[IPI storm或重复发送]** → 只在成功 `Blocked → Ready` 且目标 run queue为 remote时发送；重复 wake不发送，telemetry和burst测试检查上界。
- **[global critical-section竞争]** → 临界区只保护短小 waker cell；禁止在锁内阻塞、yield或取得网络锁，host stress检查互斥与嵌套。
- **[UART SPSC 被误当作多生产者能力]** → 一次启动和唯一 raw reader/writer 继续是 safety 契约；迁移只改变同一 task 的执行 hart。
- **[UART 失败同时破坏日志通道]** → UART runtime Gate 使用有界 host timeout、guest snapshot/状态和独立退出结果；不只依赖最终 console marker。
- **[D1 workaround 被 QEMU 泛化或删除]** → 保留有界 slow-poll 和平台注释；QEMU 不触发该路径，也不能用来否定它。
- **[timer-disabled witness异常退出]** → 使用 single-flight状态和作用域恢复；任何 timeout/fault路径先恢复本地 timer再报告失败。
- **[诊断 ABI增长]** → V5独立 command并保持V4完整前缀；仅追加直接证明 requirement的字段。
- **[延迟启动改变 fallback时序]** → Service仍先安装；runner在应用启动前生成；IRQ失败路径显式启动runner并保留既有有界 polling owner。
- **[SMP=16增加 QEMU抖动]** → deadline判断基于有界完成而非吞吐阈值；性能不作为 Acceptance。
- **[数量相同掩盖异构差异]** → QEMU只证明16个同构hart的软件并发；X100/A100 capability、AIA routing与真板online集合保留给后续真板 Gate。

## Migration Plan

1. 建立并验证 SMP-safe critical-section、patched `axtask` 和 QEMU PLIC 映射，但不改变驱动 placement。
2. 先将 UART copier 启动移到 secondary-ready adapter，建立固定 placement、snapshot 和 UART-only runtime Gate。
3. 再将网络 owner/runner 移到同一 placement 基线，建立 V5 和 network-only runtime Gate。
4. 增加两类后台任务的 migration control 和 ordering witnesses，保持默认固定布局。
5. 增加 MS08 guest/host protocol 与 validator，执行 `SMP=16` UART、network、combined、migration 和 recovery 资格。
6. 基础原语未通过时回退 root/axnet 的 `axtask` patch 和 kernel `smp` feature 传播；某一驱动集成未通过时恢复其旧启动入口，但不得声明 SMP 资格。early console、UART 公开契约、MS07 单 hart 行为和 network V1–V4 ABI 始终保留。
