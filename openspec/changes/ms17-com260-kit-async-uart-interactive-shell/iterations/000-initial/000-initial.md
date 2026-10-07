# Iteration000 / Cycle000：K3 timer 与 PXA UART 异步硬件入口

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks：T1、T2。
- Acceptance gaps：None。
- Repair items：None。
- Inherited scope：已批准 R1–R8/S1–S28；本轮只关闭 R1、R2 的低层硬件部分与 R8 对应边界。
- Excluded scope：T3–T8、64KiB完整数据面/物理drain、tty/user ELF/shell、GMAC、SMP、性能、全局文档/Recorder。

**Objective**

在 SMP1 RAM 镜像中复用现有任务/timer，接通 K3 UART42/PXA port；真实有期限等待能恢复，两次RX IRQ及孤立末字节均进入任务上下文，原k3探针模式保持。首轮不以 shell 或网络可用作为硬件成功条件。

**Investigation Facts**

Current Baseline：`k3` 分支 MS11收尾 `aca5dfe9`。产品未改；`.agents/docs/SNAPSHOT.md/tasks.md`、improvements为既有用户改动，不覆盖。MS11 accepted 的原make k3/build/lichee与RAM/FIT/reset、AIA/DMA/PHY范围直接采信。MS12同日Plan调查普通axnet cfg缺陷，本模式不引入axnet，不修它。

本Plan已实际运行：

```sh
env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/uart_16550/Cargo.toml --locked --offline --features async,telemetry --lib -- --test-threads=1
rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-baseline
/tmp/ms17-k3-aia-baseline --test-threads=1
rustc --edition=2024 --test kernel/tests/tty_write_logic.rs -o /tmp/ms17-tty-write-baseline
/tmp/ms17-tty-write-baseline
```

分别 `69 passed; 0 failed`、`16 passed; 0 failed`、`6 passed; 0 failed`；所有命令exit0。可作未变表面的变更前GREEN，不代表K3 adapter/新timer已完成。本轮无ELF依赖，不使用musl工具链；未来静态payload已有系统GCC+现有musl库编译成功结论。

Current-State Evidence 与关键调用链：

- root `k3`→kernel/k3与平台irq-if；kernel k3→k3-polling-net。kernel axfeat已启用 irq/uspace/multitask/sched-rr，uspace不自动启用paging；k3缺少paging/axfs/task-ext是后轮的用户态接缝。
- `kernel/src/lib.rs` 的 `not(k3)` 排除drivers和用户态模块。`drivers/mod.rs` 的bench/ntty/AsyncTty依赖pseudofs；低层模式开放drivers时须排除这些模块。net_placement基本placement可复用，其axnet代码gated qemu。
- `entry.rs::init` 当前K3调用 `platform::k3::run_first_byte_park`，包含网络阶段与约30s链路等待。新UART分支优先返回自身入口，原分支加 `not(k3-async-uart)`，不调用GMAC/PHY。
- axruntime先执行平台init_later→time::init_percpu、record_boot_hart、AIA global/percpu，再初始化runtime IRQ并开启SIE。`axruntime::init_interrupt` 注册timer closure，回调中update_timer→SBI set_timer和axtask::on_timer_tick；它没有初始化时主动update_timer，所以平台必须安排首次事件。registry不修改。
- K3 `TimeIf::irq_num` 返回config TIMER_IRQ（完整supervisor bit5 vector）；`set_oneshot_timer` 已做24MHz换算+SBI set_timer。time::init_percpu目前空。`IrqIf` 当前只handle external `(1<<63)|9`；register/set_enable/unregister均走AiaChip，只支持1/60/133/277；STIE/SSIE未开。
- AIA `aia.rs::SUPPORTED_ROUTES` 固定4项，`route_for`用于register/unregister/enable与分派；claim stopei是原子取出+完成，发生在handler前。source42未支持。复用HandlerTable/AiaChip/HardwareBackend，不复制初始化。
- `uart_init.rs` 当前not(lichee-d1-async-uart)会选QEMU U8路径；K3不能落入此分支。现有rings/copy tasks在T3/T4再使用，本轮raw port资格不初始化rings、不start_copiers、不创建ASYNC_TTY。
- 现有critical_section_policy在crate root无feature门，保留IRQ恢复策略。K3新MMIO共享guard优先用既有kspin IRQ-save能力，覆盖platform early/fault console与port同一硬件实例。
- `axtask::future::sleep/timeout` 使用已有每CPU TimerRuntime；drop只取消自身TimerKey，不卸载平台handler。本轮固定boot hart，不迁移timer future。实际handler duplicate/unregister只在native fake中测，不在活动board上拆runtime handler。

硬件与参考操作事实（本轮必须可直接使用）：

- UART0 PA0xd4017000，kernel VA = PA+PHYS_VIRT_OFFSET（0xffffffc000000000），IRQ42；stride4，volatile U32访问低8位；FIFO64；1152008N1无硬件flow。config/boot保留clock/reset/pinctrl/divisor，不新增时钟来源或baud读回门。
- IER：RDI1、THRI2、RLSI4、RTOIE0x10、UUE0x40、DMAE0x80。IIR bit0 NO_INT，cause低4位RLS6/RDI4/CTI12/THRI2/MSI0。LSR DR1/OE2/PE4/FE8/BI0x10/THRE0x20/TEMT0x40/FIFOERR0x80。
- FCR enable1/clearRX2/clearTX4，RX1B阈值为0；MCR OUT2=8，AFE0x20保持关闭。LCR8bit=3。启动时仅未接受业务前清FIFO；不改变现有DLL/DLH；IER保持UUE，通知先masked。
- R69本地PXA来源 `/home/daivy/projects/serial/work/k3/others/Rt-Async-AMP/tgoskits/drivers/serial/some-serial/src/pxa_uart/{mod,regs,irq,rx}.rs` 已读并按M42默认采信。参考IRQ会读RBR，本项目ISR不搬运，RTOIE关闭→RBR读→按请求恢复的Errata20顺序改在task receive中完成；1B阈值+arm后LSR重查避免小包末字节靠下一输入推进。这是明确适配方案，若真板不成立交Plan，禁止新增periodic RX。
- 所有LSR读，包括TX和TEMT，统一锁内保存当前head PE/FE/BI；到RX读RBR时分类一次丢错误字节，OE独立计数且正常字节可交付；FIFOERR汇总不能当当前head错误。状态read-clear不得先读掉再无记录。
- ISR有限cause/status处理，RX组mask+wake RX_WAKER，THRI mask+wake TX_WAKER；NO_INT+DR视为有RX工作，组合cause均处理。最多8cause分类，不读RBR/写THR/打印/分配/等待task锁。未知cause计数并关闭UART通知，资格载荷deadline报故障；UUE和early故障输出保留。

**Implementation Guidance**

先做timer/feature入口，再接PXA port与route42；每任务先见证再变更，完成后Gate3先spec review再code review。fake只替换寄存器/CSR/时间和资源边界，直接调用生产核心；不为测试复制算法或检查源码字符串。

**Task Contracts**

### T1：K3 UART模式与timer任务恢复

- Requirement/Scenario：R1/R8，S1–S3/S27–S28。
- Depends on：None。
- Targets：`Cargo.toml`、`kernel/Cargo.toml`、Makefile；kernel lib/entry模式门；`crates/axplat-riscv64-k3/Cargo.toml`、irq.rs、time.rs，必要新timer_irq.rs生产可测核心及native harness。
- Current behavior：timer handler走不支持的AIA路由，STIE关闭、time init空；K3入口只跑网络；无独立UART构建。
- Required behavior：root/kernel k3-async-uart蕴含k3，平台runtime-irq启用timer，uart-irq独立启用42；root/kernel k3-uart-port-test蕴含async模式并只选raw资格入口。新增make k3-uart-port沿用ARCH=riscv64、MYPLAT=axplat-riscv64-k3、PLAT_CONFIG现config、MEM=6400M、BUS=mmio、DWARF=n、LOG=off、SMP=1、build后k3-fit。SMP>1或同时QEMU/D1/VF2与新K3模式明确拒绝，不能悄悄选错平台。后轮shell/stdio互斥门在相关任务添加。
- Required behavior：独立Atomic timer handler槽，完整bit5向量；register CAS成功才开STIE，duplicate不覆盖/不改变enable；unregister先mask再remove；handle仅调用自己的handler一次并返回该vector。runtime-irq下time init SBI set_timer(0)，已有runtime注册+SIE开启后第一次IRQ启动自续tick。旧irq-if仍不接受timer、不开STIE；SSIE off、external claim保持。entry真实sleep与timeout取消演示使用原生future，固定boot hart。
- Preserve：原root k3/Make k3、时间数学、MS10 global/percpu/claim、原task/critical-section/timerwheel。
- Forbidden：axnet依赖/修复、paging/user模块提前开放、registry或参考仓库修改、新executor/IPI/secondary启动、在board卸载活动runtime timer。
- Test witness：在生产timer核心加native RED：register/duplicate/mask-before-remove/完整vector/与external不串路；使用native `rustc --edition=2024 --test tests/k3-timer-host-harness.rs -o /tmp/ms17-k3-timer-test` 和 `/tmp/ms17-k3-timer-test --test-threads=1`。变更前AIA16 GREEN采信上文；feature新build先观察当前不支持模式的失败，再实现build。
- GREEN condition：新timer tests GREEN、legacy AIA仍GREEN、make k3-uart-port build/FIT exit0；真实sleep/取消恢复由本轮操作者执行，不能只凭host标R1完成。
- Verification：V1/V3和B1/B2。SMP=2新target在原生Make/build直接拒绝，有清楚unsupported single-hart信息；无负向资格审计脚本。
- Stop when：必须修改axruntime自续/重复初始化scheduler；timer未按bit5进入handler；需依赖网络或多hart才能运行。

### T2：PXA UART42、硬件ownership与raw资格

- Requirement/Scenario：R2/R8，S4–S5/S7–S8/S27–S28。
- Depends on：T1。
- Targets：`aia.rs::route_for`与source42生产分支、K3 console.rs共享硬件guard；drivers/mod.rs/uart_init.rs，新增k3_uart.rs与k3_uart_core.rs；K3 entry raw port资格；tests/k3-uart-host-harness.rs及受影响AIA harness。
- Current behavior：没有PXA port；只允许4路；K3 drivers打开后会选U8/依赖tty；重复启动可能在once前reinit ring。
- Required behavior：source42/EID42 LevelHigh仅uart-irq允许，默认SUPPORTED_ROUTES四项原样；register/mask/unregister复用AiaChip。port实现UartPort receive/send/TEMT/update_ier，U32stride4、每call最多64B、IER cache+LSR latch在同一短IRQ-save guard；收发函数不等待硬件变化，只返回实际字节数。RX组RDI/RLSI/RTOIE由receive任务arm、ISR可mask；THRI由send任务arm、ISR可mask；UUE保持。
- Required behavior：先检查未激活→准备→注册ISR→ready状态，duplicate明确返回，不清活动FIFO、不建立第二owner；失败只回滚本次资源，masked返回明确层级错误；ISR handler readiness已发布才开放通知。低层模式不初始化generic ring/copy/tty；QEMU条件排除K3且不引入axnet/axfs。普通async输出在后轮接ring，本raw资格只用唯一port RX/TX任务；早期和fatal console受共享硬件guard保护。
- Required behavior：按上文PXA事实保留clock/pins/divisor、8N1与UUE；1B RX阈值，task receive做RTOIE errata顺序；ISR有界只cause/status/mask/wake，组合/spurious/NO_INT+DR/错误/unknown分类；saved-error消费一次，错误字节丢弃但后续输入可恢复。raw等待check→register→arm→recheck，TX full后register/THRI/finalsend，无忙等；所有等待有30s native外层deadline。
- Required behavior：最小资格入口在timer测试之后启动port，接受两次不同输入与孤立字节，返回对应原字节/阶段结果；不生成kernel假shell。诊断直接输出目标IRQ/byte/error与task恢复，不新增run-id/哈希/身份协议/host validator。
- Preserve：原4route/default拒绝、MS10 claim/EOI顺序、early console、唯一MMIO实例、原QEMU/D1对外API。
- Forbidden：ISR读RBR/写THR/输出/分配、持锁等待硬件、boot时强制网络、DMA/flow-control、新driver/dispatcher、活动期polling fallback或热重新初始化。
- Test witness：生产PXA core RED cases：U32/stride真实backend调用、IER RMW保UUE/notification masks、CTI/NO_INT+DR末字节、mixed cause、read-clear PE/FE/BI/OE、unknown有界、failed/duplicate register。MMIO fake记录目标offset/access width与硬件状态，不做源码guard。现有AIA16先GREEN采信；新增42分支见证用同一harness加uart-irq cfg调用真实route/core。
- GREEN condition：V2/V3通过，B3/B4实际两次IRQ+孤立字节+reset成功；任何未执行board项保留pending。
- Verification：原生commands与最短操作见下。board不能人工注入parity/overrun等事件时，native fake直接证明分类路径，不能宣称真板错误注入执行过。
- Stop when：需要重写AIA、clock事实变化、任务上下文errata无法保最后字节、需periodic RX补活、注册失败后仍半激活、任何新增无归属TX写。

**Invariants**

1. 双向UART/timer均复用现有服务；不替代executor/scheduler/timer wheel，不给本轮增加NIC/storage依赖。
2. 每个硬件状态寄存器、IER cache与错误latch有同一个IRQ-save owner；ISR不打断持此锁的同hart task。MS17只SMP1，不能外推MS14。
3. 原K3模式与四route默认接受/拒绝保持；UART42和timer各自feature隔离。
4. RAM/FIT临时启动，无saveenv、固件/分区写入；不能把host结果冒充board。
5. raw port资格与后轮copier/tty不同时消费/生产字节。不得通过第二reader解决验证或日志。

**Non-goals**

完整generic copier修复、rings/物理drain验收、tty、用户态、命令、网络、SMP、长期性能/长稳、证据判定工具、经验文档和全局状态更新。

**Acceptance**

| ID | 条件 | Task/Scenario / 直接证据 |
|---|---|---|
| A1 | timer独立owner/向量，duplicate与mask/remove不破坏external；既有等待/取消可用 | T1 / S1–S3 / native生命周期+真实任务恢复 |
| A2 | PXA U32/64BFIFO、通知ownership/错误latch/ISR有界成立，失败不半激活 | T2 / S7–S8 / production fake tests、直接阶段错误 |
| A3 | board UART42真实两次RX、孤立一字节停止输入后仍推进，task返回原字节 | T2 / S4–S5 / 操作者直接输入与输出/IRQ观测 |
| A4 | 独立SMP1镜像无网络探针，RAM引导/reset成立，原k3与AIA模式保持 | T1/T2 / S27–S28 / nativebuild与操作者结果 |

RTM（本Cycle）：R1→T1/timer native+B2 Covered；R2低层→T2/PXA native+B3 Covered（其余copier窗口由T3/T4闭合）；R8本轮→T1/T2/build+B1/B4 Covered。R3–R7与R2后续场景有Map任务，不在本Cycle声明完成。

**Verification**

V1 agent：新生产timer harness先RED、修改后GREEN；使用Task T1命令。legacy AIA16变更前结论直接采信，覆盖改后V2运行。timer取消具体runtime行为已有TimerFuture::drop移除自身key，不重新建立Plan基线；真实task proof在B2。

V2 agent：

```sh
rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-test
/tmp/ms17-k3-uart-test --test-threads=1
rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-legacy
/tmp/ms17-k3-aia-legacy --test-threads=1
rustc --edition=2024 --cfg 'feature="uart-irq"' --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-uart
/tmp/ms17-k3-aia-uart --test-threads=1
```

新case先RED再GREEN；两个AIA配置测试均检查自己的接受/拒绝契约。默认42拒绝，opt-in42注册/两次claim/disable-before-remove成立；旧unsupported测试不能错误地坚持opt-in42也拒绝。

V3 agent：改共享gates后 `make k3` 保原模式，受改QEMU门时 `make build`、D1门时 `make lichee`；覆盖没变的MS11 accepted构建直接采信。原生 `make k3-uart-port SMP=2`应明确拒绝；最后 `make k3-uart-port` exit0，沿 k3-fit原生命令得到实际待测 `StarryOS_riscv64-k3.fit`。`git diff --check`；不运行已知失败的全量host/VF2追求全绿，不重复已成功且未变测试。原始K3/core timer开STIE结果不能只用compiled-feature字符串断言验证。

人工B1（操作依据R68，正文在此，Act不沿引用回读）：当前只需串口1152008N1无流控和FAT32 U盘，不接网线。复制agent最后构建的FIT到U盘、插板USB-A；RESET同时长按s到U-Boot，执行：

```tcl
usb start
fatls usb 0:0 /
fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit
setenv fdt_high 0xffffffffffffffff
bootm 0x2e0000000
```

load/entry PA0x180000000、FDT0x181000000；不得用kernel_addr_r（CMA），不得saveenv。所见：early启动与UART资格阶段出现，没有GMAC/PHY/link等待；失败原样给出首个阶段/错误并停止。

人工B2：入口自动创建两个有效sleep等待（50ms/100ms），另一个用timeout(5ms,sleep30ms)结束并取消内部wait；观察timeout=elapsed及两项sleep恢复，之后资格阶段继续。每项有1s native外层deadline；未恢复直接报timer/task错误，不忙延时伪造输出。host lifecycle证明duplicate/unregister；board不卸载runtime handler。

人工B3：port阶段先等候输入，operator依次输入 `ms17-a`+Enter与 `ms17-b`+Enter，每次都看到原文本从任务收回并异步回传；进入孤立字节阶段只输入 `Z`，停止继续输入，看到单字节Z返回。输入各在30s窗口内；诊断显示本轮UART42真实claim/handler/wake及实际收到字节，不用软件IRQ替代。普通整行包含超过一次输入，与单字节分开证明last-byte；native fake覆盖64BFIFO/cause/error。

人工B4：完成/异常后按RESET，直接观察回原系统 `Bianbu … k3 login:`。操作者以一句结果说明B1–B4直接所见；不要求表格/截图/日志hash。没有结果时只交付代码/host/build反馈，T1/T2硬件Acceptance保持pending，不能展开Iteration001。

**Gate 2 Readiness**

PASS：调查、设计、追踪与首轮契约已闭合，用户在方案交付和RAM/FIT说明后明确“批准”（2026-10-06），执行计划获批。此次仅完成ready交接，Act等待明确执行指令；后续真板属于验证能力边界，不是待用户补做设计决定。

**Persisted Evidence**

- Mode: none

命令/≤20行决定性输出/exit和人工一句结果足以决定本轮验收，可低成本重跑；不创建Evidence目录。若真实blocker出现，仅按公共规则白名单保存决定性现场，不扩建证据协议。

**Risks and Notes**

M42适用参考操作已采信；真实硬件是否推进仍需上述板测。SMP/迁移/多hart IRQ-safe是MS14验收，不在此轮推断。helper命名、等价控制流和局部文件拆分可由Act选择，但硬件/owner/错误/预算契约不得另作决定。同一问题三次失败遵守Gate5返回Plan；不开始第四次同类尝试。

## Act Response

- Status: blocked（2026-10-07 操作者板测 B3 stage=1 失败，原因未知，按 Gate 5 阻塞交接；下方"实际改动/验证"为 agent 部分已完成的 reported 内容，保留不覆盖）

### 实际改动

T1（timer 底座）：

- `crates/axplat-riscv64-k3/src/timer_irq.rs`（新增）：`TIMER_IRQ_VECTOR=(1<<63)|5`、`TimerSlot`（AtomicUsize handler 槽 + AtomicBool STIE 镜像）、`TimerCsr` seam、生产 `ProductionCsr`（`sie::set_stimer/clear_stimer`）与 `register_irq/unregister_irq/handle_irq` 绑定。register CAS 成功才开 STIE；duplicate 拒绝且不触 CSR；unregister 先 mask 再移除；handle 只认 timer 向量、每次调用恰一次并返回原向量。
- `crates/axplat-riscv64-k3/src/irq.rs`：`IrqIf register/unregister/handle` 在 `runtime-irq` cfg 下把 timer 向量路由到独立槽；external/AIA 路径不变，legacy（无 runtime-irq）行为逐字不变。
- `crates/axplat-riscv64-k3/src/time.rs`：`init_percpu` 在 `runtime-irq` 下 `sbi_rt::set_timer(0)` 安排首个 timer 事件（runtime 注册 handler + SIE 开启后自续 tick）。
- `crates/axplat-riscv64-k3/Cargo.toml`：`runtime-irq = ["irq-if"]`、`uart-irq = ["irq-if"]`；`lib.rs` 增加 `pub mod timer_irq`。
- 根/kernel feature `k3-async-uart`（蕴含 k3 + 平台 runtime-irq + uart-irq）、`k3-uart-port-test`（蕴含 async 模式）；`Makefile` 新增 `k3-uart-port`（ARCH/MYPLAT/PLAT_CONFIG/MEM=6400M/BUS=mmio/DWARF=n/LOG=off/SMP=1，build 后 k3-fit，SMP>1 直接拒绝）；`kernel/src/lib.rs` 增加 `k3-async-uart + smp` compile_error；`src/main.rs` 增加 k3-async-uart 与 qemu/vf2/lichee-* 互斥 compile_error。
- `tests/k3-timer-host-harness.rs`（新增）：8 用例覆盖完整向量值、register/duplicate、mask-before-remove、无 handler unregister 无 CSR 副作用、单次 dispatch、timer/external 不串路。

T2（PXA port + source42 + raw 资格）：

- `crates/axplat-riscv64-k3/src/aia.rs`：`route_for` 在 `uart-irq` cfg 下追加 source42 LevelHigh 路由；默认四路由与拒绝语义不变。
- `crates/axplat-riscv64-k3/src/console.rs`：新增共享硬件 guard `with_uart_hardware`（IRQ-save、恢复如初），`ConsoleIf::write_bytes` 全部经 guard；`lib.rs` 将 `console` 改为 pub。
- `kernel/src/drivers/k3_uart_core.rs`（新增，host 可测核心）：PxHw/PxRegs seam；激活状态机（idle→preparing→ready，duplicate 显式 `AlreadyActive` 且零硬件写，IRQ 注册失败按阶段报 `IrqRegister` 并回滚、通知保持 masked）；IER cache RMW 恒保 UUE；每 poll 256B / 每硬件调用 64B；RTOIE Errata-20 顺序包裹 RBR 批；全 LSR 读（含 TX/TEMT）锁内锁存 head PE/FE/BI、OE 独立计数、FIFOERR 只聚合；ISR ≤8 cause、NO_INT+DR 视为 RX 工作、RDI/CTI/RLS mask RX 组、THRI mask、MSI 读 MSR 清源、unknown 计数并关闭通知至 UUE；激活时清 FIFO/LCR=3/MCR=0x08 且激活清空激活前锁存（见自检修复）。
- `kernel/src/drivers/k3_uart.rs`（新增）：生产 MMIO 绑定（UART0 0xd4017000，U32 stride4，经平台共享 guard）、单例 `PORT`、`k3_uart42_irq_handler`（guard 内分类，按 report 唤醒 `RX_WAKER/TX_WAKER`）、`activate_port`（axhal 注册 route42）、`diag_line`（锁存感知诊断行）、`UartPort` trait 绑定、raw 等待协议（check→register→arm→recheck，TX full 后 register/THRI/finalsend，无忙等）。
- `kernel/src/drivers/mod.rs`：k3-async-uart 形状下仅开放 `k3_uart/k3_uart_core`，QEMU/D1 形状模块全部保持排除；其余形状 cfg 求值等价不变。
- `kernel/src/platform/k3.rs`：`run_uart_port_bringup`（B2 timer 阶段：50ms+100ms sleep 于 1s 外层 deadline、timeout(5ms, sleep 30ms) 取消并打印 elapsed；B3：activate→两条输入线回显→孤立单字节→counts 汇总；所有等待 30s 外层 deadline，故障按阶段原样输出并 park）。
- `kernel/src/entry.rs`：`k3-async-uart` 分支优先进入 bringup；原 k3 分支加 `not(k3-async-uart)`。
- `tests/k3-uart-host-harness.rs`（新增）：16 用例覆盖激活序列/duplicate 零写/失败回滚、IER RMW 保 UUE、RDI/CTI/NO_INT+DR 末字节、mixed cause 预算、unknown 有界关通知、read-clear PE/FE/BI/OE 分类、TX THRE 门控与 64 上限、RTOIE errata 顺序、PXA stride4 offset 全集、RLS 不读 RBR、激活前锁存复位回归。

### 与计划的偏差

1. `uart_init.rs` 文件本身未改动：drivers/mod.rs 的形状门控在结构上使 K3 async 构建不包含整个 QEMU/D1 驱动树，QEMU U8 分支在 K3 下不可达（等价局部实现，属计划内"低层模式开放 drivers 时须排除这些模块"的落点）；Iteration 001 T4 再把 copier 侧接入 uart_init。
2. 两个新 harness 的 fn() 计数器为全局静态，测试间需显式 reset（测试基础设施细节，不涉及生产行为）。
3. timer harness 首轮 RED 后因共享计数泄漏出现 3 个假失败、uart harness 因 fake 的 DR 未随 FIFO 动态求值出现 5 个假失败，均为 harness 模型修正，生产实现未因此改动。

### 自检发现与修复

- Important（已修复）：激活前的 `diag_line`/console 轮询会按同一锁存纪律把陈旧 LSR 错误位锁入 `head_errors`，若无处理将丢弃激活后第一个业务字节。修复为 `K3UartCore::activate` 在准备批之后清空字节流锁存状态；RED 见证为移除该修复后 `activation_resets_pre_activation_console_latch` 实际 FAILED（`test result: FAILED. 0 passed; 1 failed; 15 filtered out`），恢复修复后同用例 GREEN（16/16）。
- 遗留 Minor：`with_uart_hardware` 非 panic-safe（panic=abort 下 hart 停止，锁泄漏无后果）；激活后 panic 路径（axplat console）走 guard 但无锁存纪律，错误位可能被 panic 输出读走——资格载荷不产生 panic，留待后续轮按需处理。
- 遗留 Minor：激活前 console 轮询可能把陈旧 OE/FIFOERR 计入 counters（仅诊断计数，不影响行为）。

### 验证

V1/V2（agent，native）：

```text
rustc --edition=2024 --test tests/k3-timer-host-harness.rs -o /tmp/ms17-k3-timer-test
/tmp/ms17-k3-timer-test --test-threads=1
test result: ok. 8 passed; 0 failed; 0 ignored; 0 measured   exit=0

rustc --edition=2024 --test tests/k3-uart-host-harness.rs -o /tmp/ms17-k3-uart-test
/tmp/ms17-k3-uart-test --test-threads=1
test result: ok. 16 passed; 0 failed; 0 ignored; 0 measured   exit=0

rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-legacy
/tmp/ms17-k3-aia-legacy --test-threads=1
test result: ok. 17 passed; 0 failed; ... （含 route42_rejected_without_uart_irq_optin）exit=0

rustc --edition=2024 --cfg 'feature="uart-irq"' --test tests/k3-aia-host-harness.rs -o /tmp/ms17-k3-aia-uart
/tmp/ms17-k3-aia-uart --test-threads=1
test result: ok. 18 passed; 0 failed; ... （含 route42_optin 两个用例）exit=0
```

RED 见证（变更前实际运行）：timer harness 首跑 `couldn't read timer_irq.rs`；uart harness 首跑 `couldn't read k3_uart_core.rs`（生产模块缺失即编译失败）。

V3（agent，build）：

```text
make k3            → exit=0（legacy polling 模式回归，FIT 重新打包）
make build         → exit=0（QEMU 共享 gate 回归）
make lichee        → exit=0（D1 共享 gate 回归）
make k3-uart-port  → exit=0（SMP=1、LOG=off，FIT 打包，kernel 0x180000000 / FDT 0x181000000）
make k3-uart-port SMP=2 → ERROR: k3-uart-port is a single-hart qualification mode; SMP>1 is unsupported (see MS17/MS14 boundary)（make Error 1，未进入编译）
git diff --check   → clean
```

最终 FIT 为资格镜像的判据：`strings StarryOS_riscv64-k3.bin | grep -c ms17` = 14，且不含 legacy 的 `stage=1 entry-reached` 标记；每次 `make k3` 后已按同名覆盖顺序重跑 `make k3-uart-port` 保证交付 FIT 是本模式镜像。新改文件零编译 warning（smoltcp 既有 warning 不在本范围）。

采信与不重复运行说明：uart_16550 69 项、AIA16、tty_write_logic 6 为 Plan 采集的变更前 GREEN，本 Cycle 未改动这些表面（aia.rs 的 route_for 增量已由上述两 cfg 配置的新鲜运行覆盖）；`make host-test` 已知 pre-existing 失败（SNAPSHOT 记录）与本 VF2 pre-existing 失败均不纳入，不追求全绿。

### Persisted Evidence

None required（Mode: none；命令与决定性输出已录本 Response，可低成本重跑）。

### Experience Candidates

None。

### 未解决问题（能力边界，等待操作者）

板测 B1–B4 未执行，T1/T2 的硬件 Acceptance（A3/A4 板上部分、R1/R2 真板部分）保持 pending，`tasks.md` 相应任务不勾选。操作者按上文人工 B1–B4 步骤执行（FIT 已就绪：`StarryOS_riscv64-k3.fit`）；任何阶段失败或 B1–B4 任一项不成立时，Act Response 转 blocked 交 Plan。板测结果不通过前不得展开 Iteration 001。

### Blocker Handoff（2026-10-07 操作者板测结果）

操作者串口原始记录（决策性现场，逐字保留）：

```text
Boot at 1970-01-01 00:00:40.218582416 UTC

[k3:irq] init=global begin
[k3:irq] probe domaincfg=0x80000004 smsicfg=0x0/0x0 mmsicfg=0x0/0x0 srcfg1=0x0->0x1
[k3:irq] msicfg written=0xe0400/0x80604000 readback=0x0/0x0 readback-refused (delivery test adjudicates)
[k3:irq] domaincfg enabled=0x104 readback=0x80000104
[k3:irq] init=global done
[k3:irq] init=percpu begin
[k3:irq] init=percpu done
[ms17] mode=k3-uart-port-test begin uart=0xd4017000 irq=42 smp=1
[ms17] timer result=sleeps_ok expect_ms=150 elapsed_ms=164
[ms17] timer result=timeout_cancelled elapsed_ms=7 inner=30ms-sleep
[m[ms17] uart activate=ready (notifications masked; tasks arm next)
[ms17] uart fault stage=1 rx deadline=30s
```

- 发现位置：Task T2 / 板测 B3 stage=1（两条输入线的第一条）/ Gate 4 板上验证（A3）。
- Plan 预期 vs 实际：预期 30 s 内 `ms17-a\n` 经真实 UART42 IRQ 进入任务并回传；实际 kernel 侧无任何 RX 推进，30 s 外层 deadline 触发 fault 输出并 park。
- 已达成：B1（RAM 启动进入资格模式、无 GMAC/PHY 阶段）PASS；B2（真板 timer：两个真实 sleep 164 ms ≈150 ms+tick 量化、timeout 取消 7 ms）PASS —— T1 板上验收完成，runtime-irq timer 槽在真硅上工作。T2 native 16 用例与全部 build 已过（见上文 agent 部分）。
- 未达成：B3 三阶段（两条输入线 + 孤立字节）、B4（reset 回原系统，操作者可随时执行，预期不受影响）。
- 工作区状态：本 Response"实际改动"所列文件即当前全部修改；`git diff --check` clean；未做任何阻塞后的计划外修改。
- 原因假设（仅供 Plan 诊断设计，非结论）：
  1. UART0 设备线 → APLIC source 42 的硬件交付从未在本板被证实（MS10 只证实了软件注入 1 与 GPIO60/物理插拔的 133/277；42 是本 Cycle 首次使用的路由），线路/触发方式/中断号均可能与本假设冲突；
  2. PXA UART 通知配置在真板上的真实语义（IER 位效果、FCR 1B 阈值、中断类型）未取得板上证据；
  3. 软件 wake 链嫌疑较低：B2 的 timer wake 与 RX wake 走同一 block_on/唤醒机制且已板上通过。
  4. `[m` 残片疑为终端本地回显/渲染伪影，Plan 应先向操作者确认输入会话与时机。
- 恢复条件：用户调用 `openspec-plan` 做 RX 诊断设计（例如带 irq_count/IER readback/claim 观测或一次性 RBR 对照的 rework 载荷，是否允许有界 RX 轮询对照由 Plan 裁决，Act 不自行新增 periodic RX）；或用户提供该板 UART0 中断号/触发方式的固件证据。板测无新事实前不开始第四次同类尝试。
- 操作者提示：RESET 随时安全回原系统；请保留本轮串口现场并确认输入所用会话与 kernel 输出为同一路串口。

### Blocker 补充（2026-10-07 第二次板测，同镜像重试）

第二次板测串口输出与第一次完全一致（同一位置同一残片、同一 stage=1 fault）。操作者补充事实：**在 stage=1 等待窗口内敲键盘"并没有能够进行输入操作"**——且操作者此前在同一物理连接上曾向 U-Boot 成功键入命令，说明线缆与 RX→SoC 的物理路径本身是通的。

判读更新：

1. `[m` 残片为确定性 kernel 输出，非终端伪影（两次在同一位置复现）。Act 自查定位根因：`[ms17] uart stage=activate irq=42` 这行诊断在 console_write 返回时仍在 TX FIFO 中移位发送，紧接着 `activate` 的 `FCR_SETUP`（clear-TX）把未发完的 FIFO 内容清掉，仅前两字节已移出。属诊断输出完整性缺陷（激活前诊断被激活 FCR 清 FIFO 截断），不是 RX 阻塞的原因（纯 TX 侧、外观性）；修法（console_write 批末等 TEMT 排空，或激活前先排空 TX）留 rework Cycle 实施。
2. 操作者"无法输入"的表述需澄清：kernel 在 stage 完成前不回显属设计行为，敲键本身不会有任何可见反应；因此"看不到反应"不构成独立证据，与 stage=1 超时合并仍是同一条结论——RX 通知链在板上不工作。
3. 由于 U-Boot 能接收键入，字节应能到达 UART0 RX FIFO；嫌疑进一步收敛为：UART 内部中断产生（IER/FCR 真实语义）→ APLIC source 42 交付（中断号/触发方式与 DTB 假设的核对）→ handler/wake 链（嫌疑最低，B2 已证同机制）。区分性诊断（有界轮询 IIR/LSR、APLIC SETIP 位、EIP 快照的一次性对照）由 openspec-plan 设计为 rework 载荷。

### Blocker 补充（2026-10-07 参考仓事实核对，R69 tgoskits + 本地 k3 docs）

用户指令搜寻参考仓库后取得以下事实（来源：`/home/daivy/projects/serial/work/k3/others/Rt-Async-AMP/tgoskits`（R69）与 `/home/daivy/projects/serial/work/k3/docs/interrupts/k3-interrupt-and-time.md`）：

**被证实的假设（路由假设全部成立，排除出嫌疑首位）**：

1. 参考板 DTS（`os/StarryOS/configs/board/spacemit-k3-com260-ifx.dts`）`serial@d4017000`：`interrupts = <0x2a 0x04>` —— **中断号 0x2a=42 确认**，第二 cell 0x04 = **LevelHigh 确认**；`interrupt-parent = <0x82>` 解析为 `interrupt-controller@e0804000`（`riscv,aplic`）——**与本项目驱动的同一 APLIC**，且该平台栈为 **MSI 模式**（`imsic_aplic.rs::init_msi_mode` + IMSIC@e0400000 provider），与本项目 MS10 底座同构。本地 k3 中断文档交叉证实 AP 设备（含 UART）`interrupt-parent` 全部指向 `&saplic@e0804000`。
2. 寄存器位定义逐位一致（`pxa_uart/regs.rs`）：IER（RDI=0x01、THRI=0x02、RLSI=0x04、MSI=0x08、RTOIE=0x10、UUE=0x40、DMAE=0x80），RX 组 = RDI|RLSI|RTOIE 与本项目一致；IIR（NO_INT=0x01、ID 掩码 0x0E、RLS=0x06、RDI=0x04、CTI=0x0C、THRI=0x02、MSI=0x00）；MCR（OUT2=0x08、AFE=0x20）；LSR/MSR 全一致；stride 4、FIFO 64、u32 访问低字节一致。
3. Errata-20 处理与本项目同型：参考 `irq.rs` 在 drain 前后做 RTOIE off→on，且其生产注释明确本硅 Errata-20 真实形态——**低于 FIFO 触发线的字节会丢失 timeout 条件**，handler 需以 NO_INT+DR 兜底 drain（本项目 core 已实现）。

**与参考生产用法的实质差异（新首要嫌疑）**：

4. **FCR 触发线用法**：参考驱动为 Linux `serial_spacemit.c` 直译。其 `startup()` 为三步序列（`EN` → `EN|clrRX|clrTX` → **FCR=0 关 FIFO**），运行态由 `set_config` 按波特率重写触发线：115200 波特下 `select_fcr_flags_for` 落在 **`FCR = EN|PXAR8 = 0x41`（8 字节触发）**；`PXAR1`（触发字段 00，即本项目 `FCR_SETUP=0x07` 遗留的 1 字节触发）**仅在 <2400 波特时使用**。本项目激活后 FCR 停留在 `EN|1B 触发`——若本硅对触发字段 00 支持不完整，字节低于触发线时 RDI 与 CTI 都可能不产生，与板上症状（TX 正常、RX 无任何中断迹象）完全吻合。
5. 次要差异：参考在 startup 两次读 LSR/RBR/**IIR/MSR** 清 pending 状态（本项目激活只读 LSR）；参考 rearm(RX) 后立即 read_lsr_preserving 即时回收（与本项目 arm→recheck 同型，无差异）。

**对诊断轮的输入（供 Plan 采纳或裁决）**：(i) rework 载荷改用参考三步 FCR 序列 + `EN|PXAR8` 运行态触发线，孤立字节/末字节依赖 RTOIE+NO_INT+DR 兜底（core 已具备）；(ii) 保留一次性对照观测（IIR/LSR、APLIC SETIP 位、EIP 快照）以在单轮内定位断点；(iii) 连带修复 `[m` 截断（console_write 批末等 TEMT 排空后再允许清 FIFO，或激活序列先排空 TX）。以上为诊断设计输入，Act 未实施。

## Plan Review

- Review Result: rework-required

**Findings**

1. 阻塞（新证据，板上）：B3 stage=1 RX 30 s deadline 触发，两次板测同位复现，kernel 侧无任何 RX 推进（本 Cycle Act Response Blocker Handoff 串口记录）。B1/B2（RAM 启动 + 真硅 timer sleep/timeout 取消）PASS。
2. 证实（Plan 错误）：`k3_uart_core.rs::activate` 按契约停留在 `FCR_SETUP=0x07`（EN|清两 FIFO，触发字段 00=1B）、无 TX TEMT 排空、无陈旧 RBR 排空（kernel/src/drivers/k3_uart_core.rs:249-262）。Plan 独立核对参考仓实码：R69 tgoskits `pxa_uart/mod.rs::startup()` 为三步 FCR 序列（EN → EN|clrRX|clrTX → 0）加 LSR/MSR pending 清与 `drain_rx`，运行态触发线由 `set_config` 写 `select_fcr_flags_for(clock/divisor)`：115200 落 **PXAR8（FCR=0x41，8 字节触发）**，PXAR1（触发 00）仅在 <2400 波特使用（`pxa_uart/regs.rs:596`、`mod.rs:590-603`）。design D3"选择 FIFO 1B 阈值"依据的参考阅读遗漏了该语义，Act 按契约实现，偏离不在 Act。该差异与本板"TX 正常、RX 无任何中断迹象"症状吻合，是当前首要嫌疑。
3. 证实（Plan 遗漏）：`[m` 截断机制成立——`console_write` 每 byte 仅等 THRE，diag 行返回时尾字节仍在 TX FIFO，`activate` 的 FCR clear-TX 随即清掉（k3_uart_core.rs:257 写 FCR 与 console_putbyte_locked 的 THRE 门控）。纯 TX 外观缺陷，与 RX 阻塞无关；修法属激活序列变更。
4. 阻塞（Plan 遗漏）：故障路径无可定位观测——stage=1 失败只打印一行 fault，`counts` 仅在全阶段完成后输出（fault 后不可达），板上无法区分 UART 中断生成 / APLIC source42 交付 / 输入字节未达 FIFO 三层。DTS 路由事实已由参考仓逐项证实（`spacemit-k3-com260-ifx.dts:6175` `interrupts = <0x2a 0x04>`，parent 即本项目驱动的同一 `riscv,aplic`@e0804000），路由假设移出嫌疑首位。
5. 代码/diff 一致性核对通过：工作区改动与本 Cycle Act Response"实际改动"清单一一对应（`git status` 只读检查；`git diff --check` clean）；两个新 harness 经 `mod timer_irq;`/`mod k3_uart_core;` 绑定生产模块，用例数 8/16 属实；timer 槽 CAS/STIE、route42 opt-in、共享 guard、feature/Make/SMP 拒绝门均与声明一致。
6. 非阻塞 Minor（记录，不返工）：IIR cause 掩码用 0x0f 而参考 ID 掩码 0x0e，对良构 IIR 等价（bit0 NO_INT 先判）；`with_uart_hardware` 非 panic-safe、激活后 panic 路径 latch 纪律、激活前 console 轮询可能计入陈旧 OE/FIFOERR 计数——Act 已如实报告，维持遗留。
7. Act 偏差复核：deviation 1（`uart_init.rs` 未改、由 drivers/mod.rs 形状门控结构性排除 QEMU U8 分支）成立且等价；deviation 2/3（harness 全局计数 reset、fake 模型修正）为测试基础设施细节，不影响生产结论。

**Acceptance Gaps**

- A3（板上 UART42 两次真实 RX IRQ + 孤立字节）未达成——本返工目标。
- A4 板上部分：B1/B2 已过；B4（外部 RESET 回原系统）未执行。
- A1/A2 native 与全部 build 门已闭合；A4 构建部分（SMP 拒绝、原 k3 保持）已闭合。

**Evidence**

- 采信 Act Response（覆盖未变，工作区一致）：native timer 8 / uart 16 / AIA 17+18（两 cfg）全 GREEN、`make k3`/`make build`/`make lichee`/`make k3-uart-port` exit0、`SMP=2` 拒绝、RED 见证记录。
- 本 Review 只读新证据：参考仓 `tgoskits/drivers/serial/some-serial/src/pxa_uart/{regs,mod}.rs` 的 FCR 位定义、startup 三步、`select_fcr_flags_for` 分档实测 grep；`os/StarryOS/configs/board/spacemit-k3-com260-ifx.dts` 的 serial@d4017000 `interrupts=<0x2a 0x04>` 与 `riscv,aplic`@e0804000 parent 关系。
- Persisted Evidence Mode none，本 Review 不要求补建。

**Follow-up Decision**

rework-required。修复需要反转本轮契约载明的 FCR 设计决策（1B 阈值 → 参考 startup 三步 + 运行态 PXAR8 + 激活前 TX TEMT 排空与陈旧 RBR 排空），并新增故障定位观测与一次性软件投递自检表面；二者都超出原 Task Contract（含变更面）与不变量可约束的有限修复，Act 需要新的自包含执行契约。Iteration Plan 不变（仍属 Iteration 000 的 A2/A3/A4）。板测无新事实前不做第四次盲试的要求已满足：本轮后有两次板测复现 + 操作者输入事实 + 参考仓实码三重新证据。

**Next Cycle**

`iterations/000-initial/001-rework.md`（rework；draft，待用户批准后转 ready）。

**Next Iteration**

None；须本 Iteration accepted 后按 Map 展开 Iteration001。
