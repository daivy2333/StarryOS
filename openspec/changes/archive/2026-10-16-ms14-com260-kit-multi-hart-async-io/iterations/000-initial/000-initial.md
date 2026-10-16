# Iteration 000 / Cycle 000: AP 调度与中断底座

## Plan Context

- Status: ready
- Cycle Type: initial

**Cycle Scope**

- Change tasks: T1–T4（1.1–1.4），按T1→T2→T3→T4执行。
- Acceptance gaps: None。
- Repair items: None。
- Inherited scope: 用户原话“采用完整范围和 30 分钟长稳”，本轮落实16 AP启动、本地timer/soft与固定设备路由、跨簇事件直接唤醒及RAM回滚；其余设备/恢复/长稳留后轮。
- Excluded scope: NIC/UART数据面、用户shell、ISS02/ISS03修复、组合长稳、性能、存储、受控迁移、RP/RT24、固件替换和全局文档维护。

**Objective**

CoM260 Kit临时FIT中AP0–15各真正执行任务，拥有独立per-CPU初始化与timer；跨0/8的Blocked task在本地timer被关闭时仍因remote-ready IPI或boot-hart设备提示获得调度，退出前恢复timer；source1固定boot route经远端mask/unmask后可再投递。旧单hart入口保持原行为。

**Investigation Facts**

- Current Baseline: 2026-10-14，分支k3、HEAD9179205d。现有用户改动为references与ISS02/03未跟踪，不能覆盖；本change当前仅计划文件。既有K3主入口/APLIC/IMSIC、24MHz timer、RAM/FIT路径、单hartUART/NIC成立。
- QEMU软件基线：16 hart固定NIC placement、目标timer关闭的remote-ready IPI已accepted；UART专项/迁移/恢复/组合不在该资格。axtask、critical-section、placement、stack_runner相对5df918cd源码未变；不得改造这些机制或新建第二executor/IPI owner。最近AIA/timer生产harness和K3构建结果采信已验收单hart记录；本轮在相同入口补新RED，不先重建全量基线。
- 本板事实：既有Linux真实日志检出SBI TIME/IPI/RFENCE/HSM且两次启动16 CPUs，online=0-15。IFX DTB的AP为dense0–15，CPU0/8均声明ssaia/zicbom和64B cbom；0/8 ISA分别rv64imafdcvh/rv64imafdcv。把这些作为预期，不把Linux/FDT当StarryOS运行通过；boot raw hart来自SBI a0，不默认0。
- 主启动：`boot.rs::_start`先生成Sv39 boot表，kernelPA0x180000000及其high-half映射、MMIO所在block，调用axplat::call_main。axruntime随后percpu/early、allocator/paging、K3 init_later（记录boot hart、一次global APLIC、本地IMSIC/time）→axtask init_scheduler→拉secondary→注册timer→INITED_CPUS barrier→main。
- 次级现有runtime：`axruntime-0.3.0-preview.2/src/mp.rs::rust_main_secondary`依次percpu、init_early_secondary、ENTERED_CPUS发布、paging-secondary、init_later_secondary、scheduler-secondary（Release发布runqueue/schedulable）、barrier、SIE、idle。K3缺CPU boot和两个hooks，必须补接口；不复制或本地化整个runtime。
- `PowerIf::cpu_boot`无返回值，runtime调用后会等ENTERED_CPUS；HSM unavailable/start error不能只return。平台报告错误后park即可在该已知错误分支阻止进入runtime等待；SBI已成功但secondary不进/不ready为实板阻塞，不在本轮盲加通用watchdog。
- `axtask::init_scheduler`在runqueue初始化后对唯一S_SOFT注册assert；K3当前拒绝该vector，所以只放开SMPguard仍失败。成功Blocked→Ready远端入队经AxWaker与runqueue发SBI IPI；目标handler记per-hart计数并set_preempt_pending，正常IRQ guard退出调度。
- `IrqIf`当前timer独立slot、external AIA，S_SOFT没有register/handle/SSIE；send_ipi已调SBI。参考固件`lib/sbi/sbi_ipi.c::sbi_ipi_process_smode`置SSIP，不能用EID0/GENMSI替代软件trap。
- timer slot的handler是全局CAS，register仅开caller STIE；`time::init_percpu`只set_timer(0)。次级本地STIE/SSIE须独立enable；timer/soft set_enable只触碰local CSR。SMP两handler常驻，全局unregister拒绝None无副作用；旧单harttimer unregister仍mask-before-remove。
- AIA `init_global`只写APLIC并初始化domain/MSI地址；`init_percpu`只清本地EIE、threshold0、delivery1、SEXT；secondary不得跑global、不得覆盖BOOT_HART。`enable_source`按BOOT_HART写target，却对caller local EIE RMW。SMP固定device register/unregister只允许boot；远端set_enable设备只写全局APLIC SETIE/CLRIE，EIE稳定保留到boot unregister。旧单hart走原AiaChip路径；stopei原子claim+EOI先于handler，EIE掩码丢MSI的本板事实保持。
- TimerFuture创建/poll/drop访问当前percpu wheel。新native主任务在任何deadline创建前固定boot，所有测试task首次入队前singleton；不能先spawn再迁移。实际0/8跨簇执行必须直接观察，不用schedulable mask代替。
- Makefile顶层`APP_FEATURES += smp`会被K3递归命令行变量优先级覆盖，已以GNU make原生复现；新feature本身闭包含root/kernel smp与platform smp/runtime-irq。

**Code and Critical Path**

| 入口/符号 | 本轮责任 |
|---|---|
| root/kernel Cargo、Makefile、`kernel/src/{lib,entry}.rs`、`platform/mod.rs` | 新k3-smp feature/目标与native入口选择，旧guard/旧entry不变 |
| `crates/axplat-riscv64-k3/{Cargo.toml,src/boot.rs,src/init.rs,src/power.rs}` | 平台smp feature、HSM/start error、次级独立SP与共享boot表、per-CPU hooks |
| `src/{time,timer_irq,irq}.rs`，最小fixed-soft core | local STIE/SSIE、global slot、SSIP ack、全vector dispatch、local mask |
| `src/aia.rs::{AiaChip,init_global,init_percpu,HardwareBackend}` | 固定boot route、global source gate与local文件隔离；existing claim语义 |
| `kernel/src/platform/k3_smp.rs`（本轮新增native平台入口） | 原生16 AP任务、timer/cancel、真正Blocked的跨簇IPI、source1 delivery、direct output；不包含GMAC/tty |
| axtask `spawn_*_affinity/current/set_cpumask_checked/ipi_received_count_by_hart` | 直接消费已有API，不改变scheduler或slot归属 |
| `tests/k3-{aia,timer}-host-harness.rs`及最小SMP/soft core rustc harness | 生产core include+fake CSR/SBI/APLIC，多hart模型用独立file，不新建判定协议 |
| `tools/starry-k3.its`与新SMP FIT形态、已有IFX DTB | 复用kernelPA0x180000000/FDT0x181000000、load0x2e0000000；保留已有FIT原生格式，不增manifest/身份字段 |

**Implementation Guidance**

先让新特性/boot接口可构建，再闭合local timer/soft，随后固定外设路由，最后跑原生平台功能。SMP新目标默认16，可用2/4/8定向诊断，SMP=1或>16拒绝；最终T4只以16计通过。APLIC设备IRQ保持boot hart，由跨hartwaker驱动远端任务，不做IRQ迁移。新任务固定affinity；主native入口不用legacyPHY观察或网络矩阵作为前置。

**Task Contracts**

### T1: 16 AP 的新模式与次级接口

- Requirement/Scenario: R1/S01–S02、R2/S03、R8/S26–S27。
- Depends on: None。
- Targets: root/kernel Cargo新`k3-smp`，Makefile新目标/FIT；`kernel/src/lib.rs/entry.rs/platform/mod.rs`；K3 platform Cargo/boot/init/power；新最小生产SMP startup helper与其rustc harness（按需用于真实HSM错误判断，不产生诊断协议）。
- Current behavior: K3无secondary入口/cpu_boot/hooks；oldasync拒绝smp；递归APP_FEATURES不会因顶层普通追加而含root smp。
- Required behavior: 新mode闭包显式k3+smp+platform runtime-irq/smp；平台smp转发axplat/smp。新target以16构建并产出`StarryOS_riscv64-k3-smp.fit`，保留旧target名称/行为。secondary接a0 hart/a1 SP-PA，安装主核已发布boot PT、fence.i/high-half SP，调用axplat secondary main；不clear BSS、不共用主SP、不重写PT。boot PT经已有64B clean+fence发布后HSM；HSM probe/错误/越界非AP目标直接报告后park，不return到runtime等待。early-secondary设trap；later-secondary只跑local初始化，global/BOOT_HART只主核一次。此时native入口可仅报告新mode并保留阶段结果，完整测试在T3补齐。
- Preserve: 已核对RAM/MMIO/PT地址、board firmware/IFX DTB、24MHz、既有QEMU/单hartK3入口；axtask/axruntime/critical-section机制不改。
- Forbidden: 替换OpenSBI、someboot移植、RP、hotplug、拓扑框架、通用启动watchdog、为验证新增hash/revision/run身份。
- First test: 实际`make k3-smp SMP=16`在未实现目标时RED（无rule/接口缺失）；在项目rustc harness include生产startup检查，`hart_start_refusal_stops_before_runtime_wait`、`secondary_target_and_stack_are_checked`、`secondary_init_never_runs_global_init`先RED，错误夹具验证error分支不继续下一启动动作。用原生构建证明实际secondary接口/入口可链接；host不能证明执行PC或物理cache。
- GREEN condition: 新mode/目标和接口构建成功；HSM所有已知拒绝被归类、停止；target/SP/PT发布场景GREEN；16真实执行归T4，不在本task声称。
- Verification: `rustc --edition=2024 --test tests/k3-smp-host-harness.rs -o /tmp/k3-smp-host && /tmp/k3-smp-host --test-threads=1`（若helper在既有harness测试则同一原生入口），`make k3-smp SMP=16`编译到实际新增代码。`make k3-smp SMP=1`与`SMP=17`拒绝且不执行boot，不以源码grep判定。
- Stop when: 现有runtime接口不足、dense映射/ISA被证据推翻、必须修改固件或扩大内存布局；HSM成功后实际无secondary进入时交T4阻塞返回Plan。

### T2: local timer/S_SOFT 接通且handler唯一

- Requirement/Scenario: R2/S03–S05、R3/S06、R8/S26。
- Depends on: T1。
- Targets: K3 `irq.rs::IrqIfImpl::{register,unregister,set_enable,handle,send_ipi}`、`timer_irq.rs`、`time.rs::init_percpu`与secondary later hook；最小`ipi_irq.rs`固定生产core及nativeCSR绑定；timer/IPI host harness。
- Current behavior: soft拒绝register/handle，SSIE关；timer handler全局但仅注册hart STIE开启；set_enable不识别完整timer/soft vector。
- Required behavior: soft全vector`(1<<63)|1`有唯一CAS slot，注册成功开caller SSIE，重复失败不改slot/CSR；handle清SSIP后调axtask唯一handler并返回vector，无handler时仍清pending但不造成功调用。secondary later hook独立STIE/SSIE on+set_timer(0)，不重新注册global handler。timer/soft set_enable只改caller CSR。SMP runtime timer/soft unregister returnsNone无slot/CSR变化，localmask是停止方式；旧非smp timer mask-before-remove行为保持。send_ipi保留SBI target mask，去掉错误EID0解释；不增第二axtask/axipi owner。全局enabled telemetry不作为perhart CSR状态判据。
- Preserve: timer与external路径隔离、首次timer自继续tick、原单harttimer lifecycle；pending状态先ack不在handler里直接切runqueue。
- Forbidden: GENMSI冒充S_SOFT、设备源号冒充timer、本轮改axtask通知条件、动态IRQ框架、重注册到每hart。
- First test: `tests/k3-timer-host-harness.rs`扩展`secondary_local_enable_does_not_register_handler_twice`与`local_timer_mask_preserves_other_hart`先RED；新的最小生产soft core harness `soft_ack_allows_second_delivery`、`duplicate_soft_registration_is_inert`、`smp_runtime_unregistration_preserves_all_harts`先RED。fake记录每hart CSR与handler调用，禁止只测试描述文本。
- GREEN condition: local开/关不改变其他hart或global handler，重复注册/unsupported vector inert，SSIP ack后独立后续事件可调用一次，SMP unregister不破坏全局；旧8项timer测试仍GREEN。
- Verification: `rustc --edition=2024 --test tests/k3-timer-host-harness.rs -o /tmp/k3-timer-host && /tmp/k3-timer-host --test-threads=1`；`rustc --edition=2024 --test tests/k3-ipi-host-harness.rs -o /tmp/k3-ipi-host && /tmp/k3-ipi-host --test-threads=1`。每次增量用filter跑RED/GREEN，轮末完整suite一次；真实每harttimer/IPI由T4验证。
- Stop when: 原固件SBI IPI接收不是SSIP、需要更改调度/中断owner契约或handler移除的全局停机机制；不把native异常伪装成host成功。

### T3: 固定设备路由与native平台验证入口

- Requirement/Scenario: R2/S03,S05,S07、R3/S06,S08,S09、R8/S26–S27。
- Depends on: T1/T2。
- Targets: `aia.rs`新增仅global source gate helper、`irq.rs`SMP设备分支；`tests/k3-aia-host-harness.rs`；`kernel/src/platform/k3_smp.rs`与entry mode分支；现有axtask原生affinity/waker/task-state/API。
- Current behavior: device target取boot，但远端caller的set_enable修改自己的EIE；K3没有platform-only多hartnative载荷。
- Required behavior: SMP device register/unregister只boot允许（remote false/None、无硬件变化），source白名单沿用。boot注册维护target/本地EIE；SMP任意caller set_enable只做global SETIE/CLRIE，不操作local EIE或改target；boot unregister关闭global后清本地EIE。secondary只初始化自己的file。原single-hartcore路径不变。native入口固定主task affinity后，在0–15各spawn一个singleton任务执行真实timer等待并报告actual hart/完成；构造真实Blocked target8、从boot独立两次软件wake观察目标per-hart IPI增量；另一次target8关本地timer、boot source1 SETIPNUM→claim/EOI→handler wake→remote-ready IPI→target resume，恢复本地timer后报告。target需先确认为TaskState::Blocked，5s绝对deadline由timer正常的boot监督；所有可执行退出分支先恢复STIE，无法resume则报失败/缺restore并外部RESET，不伪报通过。source1远端mask/unmask测试只在重开后发新SETIPNUM，不把掩码期间GENMSI当global gate证明。等待结束后再次native sleep确认targettimer已恢复。
- Preserve: IRQ只cause/ack/wake，原stopei原子claim+EOI先handler；AIA MSI地址和mask裁决不改；axtask原nativefuture/critical-section。
- Forbidden: 设备IRQ迁移、远端IMSIC MMIO、每hart预开所有设备EIE、复制完整QEMU ioctl/witness协议、validator、source1冒充GMAC completion、新run/session身份。
- First test: 扩展现有AIA fake为独立local file：`remote_source_gate_preserves_local_eie`、`remote_device_registration_is_rejected_without_mmio`、`secondary_init_preserves_boot_routes`先RED。现有affinity/placement/CS测试GREEN可采信；native先行测试在无对应接口状态编译/运行失败，用真实入口观察RED，再接线GREEN。native本身是项目内核测试代码，不另建结果审计器。
- GREEN condition: 主file target/EIE在remote门操作中不变，local文件互不清除，原AIA claim/unknown/retrap测试GREEN；nativeactual任务数/完成mask/目标IPI与timer恢复可直接观察。没有板测时只算agent准备完成，T4仍未完成。
- Verification: `rustc --edition=2024 --test tests/k3-aia-host-harness.rs -o /tmp/k3-aia-host && /tmp/k3-aia-host --test-threads=1`；native功能由T4实际FIT运行。source60/133/42真实设备与各自最后事件留后续专项，当前仅证明控制器路由/调度。
- Stop when: 测试需要设备bring-up/应用树才可证明平台、不得靠busy循环补IPI；AIA global/local文件事实不符或需要新路由架构时返回Plan。

### T4: 构建、自检与首轮真板结果

- Requirement/Scenario: R1–R3/R8，S01–S09/S26–S27。
- Depends on: T1–T3。
- Targets: 本轮完整diff；现有host测试入口、Makefile k3/k3-smp产物；操作者RAM/FIT实际运行；本Cycle Act Response。
- Current behavior: 没有K3多hart运行结论；QEMU和Linux只能支持软件/硬件预期。
- Required behavior: task Gate3即时自检后，轮末完整host suites/受影响构建成功，准备具体FIT与最短操作；停在真板能力边界。操作者提供结果后审核真实执行、timer/IPI/local隔离与回滚，不以“启动看起来正常”勾选T4。全部通过再完整diff自检并写reported Response。
- Preserve: 用户已有改动，原RAM窗口、单hart入口和无持久写入边界。
- Forbidden: 自动访问串口/复位硬件、QEMU冒充K3、构建后无运行即报完成、新采集/身份/判定工具、自动展开001或同步全局文档。
- First test: 采信T1–T3实际GREEN作为先行观察；此task为集成运行，不新增镜像身份测试。旧mode测试按受影响面采信/运行，native场景首次板测预期尚无PASS。
- GREEN condition: 下述Acceptance全部真实满足，native结果直接观察，不含未执行或豁免项；Fail/timeout停止不盲试到PASS。
- Verification: 轮末受影响smp/ipi/timer/aia suites各一次；`make k3`保持旧boot形态，最后`make k3-smp SMP=16`生成待测FIT。测试步骤见下。`git diff --check`只检查编辑质量；它不证明设备行为。
- Stop when: native首个目标失败、task mask/实际hart不符、任一IPI缺timer恢复、ISA/CSR trap、外部RESET未回官方Linux；保留最短决定性结果并blocked交Plan。

**Invariants**

1. global APLIC/BOOT_HART只主核一次；每hart仅自己的IMSIC/timer/soft；固定device routes在boot，远端只global门。
2. 唯一S_SOFT owner是axtask；无axruntime/ipi或第二注册。原IRQ guard/preempt机制复用。
3. 每任务singleton首次入队前提交，真实deadline创建/poll/drop在同hart；无受控或隐式迁移。
4. 页表发布后secondary复用，独立SP，不重复BSS清理；RAM/FIT地址与旧平台隔离保持。
5. 完成声明按真实行为，SMPconfigured/FDT/构建/日志格式不替代实际执行；遵守公共规则验证与Scope Control。

**Non-goals**

NIC/UART数据面、源42/60/133真实设备资格、用户态shell、DMA恢复、ISS02/03修复、30min soak、性能、存储、RP、hotplug/迁移、固件替换、全局文档。

**Acceptance**

| ID | 覆盖 | 直接结果 |
|---|---|---|
| A1 | R1/S01–S02/T1,T4 | AP0–15各actual hart与任务完成，真实task mask覆盖0xffff；HSM拒绝host错误路径不继续 |
| A2 | R2/S03–S05/T2,T3,T4 | global初始化不被secondary重跑，各local timer独立；取消一个等待不影响其他；两次IPI ack后可再收，无风暴 |
| A3 | R2/R3/S06–S09/T3,T4 | 跨0/8目标真Blocked，timer-off后source1/远端wake导致目标IPI与resume，先恢复timer后终态，再真实timer等待成立；remote gate不改local EIE/target |
| A4 | R8/S26–S27/T1–T4 | 旧k3构建/受影响单hart语义保持，新FIT真实运行，外部RESET回原Linux，无持久写入 |

**Verification**

当前轮RTM（完整范围的剩余R4–R7由后继轮覆盖，不属于当前执行批次）：

| Requirement | Task | First Test | Status |
|---|---|---|---|
| R1 | T1,T4 | 新make目标RED、HSM拒绝core测试、真实16 AP任务 | Covered |
| R2 | T2–T4 | local timer/soft隔离、ack/重复注册、remote source gate生产core测试 | Covered |
| R3 | T3,T4 | 既有affinity拒绝GREEN、native真Blocked→IPI→resume→timer恢复 | Covered |
| R8 | T1–T4 | 受影响旧make k3、实际RAM/FIT运行与外部RESET | Covered |

host为原生rustc/Cargo项目测试，不读取输出再由脚本给判定。T1–T3按filter RED/GREEN；各受影响suite轮末一次全量。不重复已通过且未变化的测试。若axtask/CS/placement实际被修改，契约失效交Plan，不把它们变成未经调查的新实现面。

最短人工步骤（agent准备镜像后交操作者；按已验证RAM Runbook，不要求截图或填表）：

1. 串口115200 8N1，上电/RESET同时长按`s`到U-Boot；`usb start`、`fatls usb 0:0 /`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3-smp.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。不saveenv。直接观察PT marker与平台入口；装载/entry失败停。
2. 等native平台测试：16行actual task hart/真实timer结果，完成mask0xffff；只打印configured16不通过。直接观察两次0→8 wake/IPI及取消等待结果，错误行或超期停。
3. 观察source1 route与remote门场景：IRQ实际boot hart，target8真Blocked/关timer后由SETIPNUM提示获得IPI和resume；terminal前timer restored，随后target8的真实sleep返回。source1不是GMAC/UART证据。
4. native完成后无持续软中断风暴；外部RESET回官方Linux登录。操作者以一句话报告各直接结果及任何首个失败；Act记录native决定性行和原话，不追加专用表格/判定器。

默认板测一次。第一次失败按场景停止并定向诊断；三次同问题按公共规则反思，不创建第四次盲试。

**Gate 2 Readiness**

技术检查PASS：当前000调查、设计、4个任务、先行测试、真实入口、停止边界和RTM已闭合。Gate2 PASS：用户2026-10-15批准计划并授权实施（原话"批准实施，更改gate状态，要求遵守各种规范，真板操作测试交由我手动进行"）；Status改为ready，交Act。真板操作步骤按计划由操作者手动执行，Act停在真板能力边界。

**Persisted Evidence**

- Mode: none。
- 命令、退出码、task/IRQ/IPI实际hart、native目标结果与回滚一句话写Act Response足够；不创建Evidence占位或完整日志目录。

**Risks and Notes**

- Linux已有HSM/IPI/16CPU事实不需要重新采集；StarryOS首次次级执行/cache/CSR结果未取得，A1–A3必须真板。
- SBI成功却无secondary entry/ready时现有runtime可停在等待；当前明确能力边界与Stop when，不能误标完成。若该实际异常发生，Plan按nativePC/CSR与现有层定位再给契约，不先引入通用watchdog。
- 固定boot IRQ无需IRQ balancing；不会把configured16当跨簇执行。每个AP都需要actual任务，A3指定0/8覆盖两个簇。
- ISS03锁类型待验收相关轮次使用实际axsync/multitask实现；此轮不修Issue，不受历史纯spin叙述驱动。
- 后续001/002/003/004只在tasks保留路线，当前不创建目录、Cycle或实施批次。

## Act Response

- Status: reported（T1–T4 完成：真板会话已执行并审核通过，A1–A4 全部真实满足；Plan Review pending）

**实际改动**（agent 侧，分支 k3，工作树未提交）

- T1（16 AP 新模式与次级接口）：
  - `Cargo.toml`（root）：新增 `k3-smp` feature 闭包（`k3` + `smp` + `axplat-riscv64-k3/{smp,runtime-irq}` + `starry-kernel/k3-smp`）。K3 递归 make 以命令行变量传 `APP_FEATURES`，顶层 `+= smp` 不生效，闭包必须显式（计划已预判）。
  - `kernel/Cargo.toml`：新增 kernel `k3-smp = ["k3", "smp"]`。
  - `crates/axplat-riscv64-k3/Cargo.toml`：新增平台 `smp = ["axplat/smp"]`。
  - `crates/axplat-riscv64-k3/src/smp_boot.rs`（新增）：`cpu_boot` 验收核心（`check_boot_request` 目标越界/0 号拒绝与栈对齐检查、`check_hsm`），later-init 步骤表 `PRIMARY_LATER_STEPS`/`SECONDARY_LATER_STEPS` 与 `steps_are_local_only`。纯 core，host harness 直接 include。
  - `boot.rs`：新增 `_start_secondary`（cfg smp）：a0=hartid、a1=SP(PA)，装主核已发布 boot PT（satp+sfence）、fence.i、high-half SP，调 `axplat::call_secondary_main`；不重写 PT、不清 BSS、不共用主 SP。
  - `power.rs`：新增 `cpu_boot`（cfg smp）：`check_boot_request` → `check_hsm`（SBI probe）→ `hart_start`；任一拒绝走 `smp_boot_failed() -> !`（报告后 park，结构上不可能回到 runtime 的 ENTERED_CPUS 等待）。
  - `init.rs`：`init_early_secondary`（仅 trap 向量）；`init_later_secondary` 按 `SECONDARY_LATER_STEPS` 分发（set_timer(0) → 本地 IMSIC 文件 → 本地 STIE/SSIE），boot-only 步骤命中即 park；boot 侧 `init_later` 改按 `PRIMARY_LATER_STEPS` 分发，单 hart 顺序不变。
  - `Makefile`：新增 `k3-smp`（默认 SMP=16，`SMP=1/>16` 拒绝；`TARGET_DIR=target/k3-smp`、`OUT_CONFIG=.axconfig-k3-smp.toml`、`OUT_DIR=out-k3-smp` 全隔离，不覆盖单 hart 产物）与 `k3-smp-fit`；`tools/starry-k3-smp.its`（load/entry 0x180000000、FDT 0x181000000，与单 hart 同布局）。`.gitignore` 补 `out-k3-smp/`。
- T2（local timer/S_SOFT）：
  - `timer_irq.rs`：新增 `set_local_timer_enabled`（generic，host 可测）与生产 `set_local_timer`；全局槽与注销路径未动（旧单 hart mask-before-remove 保持）。
  - `ipi_irq.rs`（新增，cfg smp）：`SOFT_IRQ_VECTOR=(1<<63)|1`、`SoftSlot` CAS 槽（register 开 caller SSIE、重复 inert）、`handle` 先 ack SSIP 再调 handler（无 handler 仍 ack 不造成功调用）、`unregister_irq()` 恒 None（SMP 停止方式=本地掩码）、`set_local_soft`。
  - `irq.rs` IrqIf 接线：register/handle 接 soft 槽（仅 smp）；set_enable 对 timer/soft 向量只动 caller CSR；SMP 下 unregister 对 timer/soft 返回 None；旧非 smp 分支逐字节保留。
- T3（固定设备路由 + native 入口）：
  - `aia.rs`：`BOOT_HART`/`record_boot_hart` 去 cfg 并新增 `boot_hart()`；新增 `set_source_gate`（仅 SETIE/CLRIE 单写）；`AiaChip` 新增 `smp_register`/`smp_unregister`（caller≠boot 拒绝、零硬件副作用）与 `smp_set_enable`（仅全局门）。
  - `irq.rs`：SMP 下设备 register/unregister 走 `smp_register/smp_unregister`（caller=this_cpu_id），set_enable 设备源走 `smp_set_enable`；非 SMP 的 MS13 GMAC 恢复语义（本地 EIE+门）原样保留。
  - `kernel/src/platform/k3_smp.rs`（新增）：native 平台入口。Stage A（16 hart singleton 各 50ms 真实 timer 等待、actual hart、mask 0xffff、5s 监督）→ Stage B（两轮 WQ 软件唤醒真 Blocked 的 hart-8 目标，per-hart IPI 增量见证）→ Stage C（目标自关 STIE 后 Blocked，source-1 SETIPNUM 注入 → boot claim/EOI → handler → remote-ready IPI 唤醒，恢复 STIE 后真实 sleep 复验）→ Stage D（hart-1 远端 mask/unmask 仅动全局门：boot EIE 位与 route 寄存器读回不变，重开后新 SETIPNUM 证明投递）→ 末次 boot sleep + summary + park。`entry.rs` k3-smp 分支先于轮询分支（并给轮询分支补 `not(feature="k3-smp")`）；`platform/mod.rs` 挂载模块。

**与计划的偏差**

- 计划 Code 表中次级入口写"fence.i/high-half SP"——已含 fence.i；其余按 QEMU virt 上游同构实现（参考件核对，无偏差）。
- Stage D 原设想在板上断言"远端 caller 自身 EIE 文件未被写"：生产 seam 只能访问当前 hart 文件，远端 IMSIC MMIO 在计划 Forbidden 清单内。改为 host harness 以独立 fake 双文件证明该性质（`remote_source_gate_preserves_local_eie`），板上见证收敛为 boot EIE 位 + route sourcecfg/target 读回 + 重开后新投递（A3 的"remote gate 不改 local EIE/target"以路由目标文件为对象，语义等价且更强）。Gate 3 中发现并修正，先行测试未失效。
- `init.rs` RecordBootHart 步骤改从 per-CPU 状态取 hart（`rust_main` 已用同一 id 初始化 percpu），消除 `cpu_id` 未用警告；值语义不变。
- harness 侧：`k3-ipi-host-harness` 一处用例补 `reset_calls()`（同进程静态计数跨用例残留，测试自身缺陷）；`unregister_irq()==None` 断言改 `is_none()` 消除 fn 指针比较警告。

**自检发现与遗留 Minor**

- 已修复：Stage D 远端文件断言（见偏差）；`init_later` 未用参数；两处 harness 自身缺陷。
- 遗留 Minor（不阻塞）：native 载荷 `wait_until` 以 10ms timer sleep 轮询监督条件——属一次性平台见证载荷，非生产数据面，与"空闲不轮询"的生产契约不冲突。
- 范围外 Issue 候选（只报告不落账）：`tests/k3-device-irq-host-harness.rs` 有 2 个既有失败（`stage10_setup_arms_pin35_falling_edge_gpio_mux`、`window_close_dumps_raw_state_without_claims`）。HEAD 对照（/tmp 以 HEAD 版 5 个被 include 源文件复跑）同样 16/2，证明先于本变更存在，非本轮回归；该套件不在本轮受影响清单。建议后续由 Recorder 定位登记。

**验证命令与实际结果**（全部真实运行）

- RED 先行（实现前）：
  - `make k3-smp SMP=16` → `No rule to make target 'k3-smp'`。
  - `rustc --test tests/k3-smp-host-harness.rs` → smp_boot.rs 缺失编译失败。
  - timer harness → `cannot find function set_local_timer_enabled`；ipi harness → ipi_irq.rs 缺失；aia harness → `unresolved import aia::set_source_gate` / `cannot find function record_boot_hart`。
- GREEN：
  - `/tmp/k3-smp-host --test-threads=1` → 3/3 ok。
  - `/tmp/k3-ipi-host --test-threads=1` → 7/7 ok（零警告）。
  - `/tmp/k3-timer-host --test-threads=1` → 10/10 ok。
  - `/tmp/k3-aia-host --test-threads=1` → 20/20 ok（16 既有 + 4 新增）。
  - `make k3-smp SMP=16` → exit 0，生成 `StarryOS_riscv64-k3-smp.fit`（333180 B）与 `out-k3-smp/StarryOS_riscv64-k3.{bin,elf}`；配置实测 `plat.max-cpu-num=16` 写入隔离 `.axconfig-k3-smp.toml`；改动文件零新警告（smoltcp/uart_16550 警告为既有）。
  - `make k3-smp SMP=1` / `SMP=17` → 均以 `ERROR: k3-smp boots 2..=16 harts` 拒绝退出，不执行构建。
  - `make k3` → exit 0，单 hart FIT 正常重打包（旧入口/形态保持）。
  - 受影响相邻套件 `k3-aia-probe-host-harness` → 11/11 ok。
  - `git diff --check` → 干净。
- 真实每 hart 执行/IPI/timer 恢复/RAM 回滚：真板会话已执行，见下方「真板结果」节。

**真板结果（T4，操作者 2026-10-15 会话，串口原文逐行采集，Act 据实审核）**

- 启动与底座：`[k3:pt-mmu]` 后 axruntime 正常；`init=global` 全程恰好 1 次、`init=percpu` 16 组 begin/done（并发交错，S03 全局域未被次级重放）；`boot_hart=0` 实测自 SBI a0。`msicfg readback-refused` 与 `domaincfg readback=0x80000104` 为 MS10 已裁决硅行为，本轮 source-1 投递实测成立。
- A1 ✓（S01）：16 行 `task stage=a hart_actual=N timer_wait_ok=true` 实际 hart 覆盖 0–15 各一次；`mask stage=a tasks=0xffff harts=16`。
- A2 ✓（S05）：两轮 0→8 独立软件唤醒 `ipi8` 0→1→2，ack 后续投递成立；`summary stage=done` 后串口静默，无软中断风暴。
- A2/S04 偏差（用户裁决接受现证，风险留档）：板上无"多 hart deadline 等待中取消其一、其余推进"的直接观测行——stage C 以更强形态承载隔离语义（hart-8 整个 STIE 关闭期间 boot timer 监督照常推进），取消机制在每轮监督等待中真实运行但不可见；host 面 `local_timer_mask_preserves_other_hart` 已证 per-hart 掩码隔离。计划 operator 步骤中"取消等待结果"因此未形成独立板面行，属实施对 operator 观测点的偏差。用户在「接受现证留档 / 补场景重跑一次」两选项中选择不重跑（原话"确认回到，填写回复吧"）。
- A3 ✓（S06/S07/S09）：hart-8 目标自关 STIE 后 Blocked，经 source-1 注入链（SETIPNUM→boot claim/EOI→handler→remote-ready IPI）唤醒（`wake=source1 ipi_after=3`），终态前 `timer_restored=1`、随后真实 sleep `fresh_sleep_ok=true`；stage D 远端门后 `boot_eie1=1 target=0x1 unchanged=1`（hart0|eid1 编码正确）且重开新 SETIPNUM `delivered=1 irq_hart=0`——最后真实事件到达原目标 boot；跨簇 witness 落 hart 8（A100）与 hart 1（X100），实际 hart 与 affinity 一致。
- A4 ✓（S26/S27）：`make k3` exit 0 单 hart 形态保持；新 FIT 真实运行全流程到 summary；外部 RESET 回官方 Linux（操作者确认原话"确认回到"）；全程未 `saveenv`，无持久写入。
- **T4 判定：通过**；本 Cycle 全部 Acceptance（A1–A4）真实满足，任务 1.4 已勾选。

**Persisted Evidence**

None required（模式 none；命令与判定行已录本 Response，真板结果由操作者一句话报告录入）。

**Experience Candidates**

- Issue 候选：`k3-device-irq-host-harness` 2 个既有失败（证据：上方 HEAD 对照复跑，同一失败集 16/2）。属当前 change 范围外的实质缺陷候选，未落账。

**未解决问题**

None（本轮无遗留。S04 板面观测偏差已按用户裁决接受现证并留档，见上方真板结果节；Plan Review 保持 pending。）

## Plan Review

- Review Result: accepted

Reviewed 2026-10-15。独立阅读全部暂存 diff（23 文件，+1623/−28），逐项核对 Act Response 声明与代码一致；host/构建验证结论按公共规则 › 验证采信（覆盖未变化，只读 git 检查确认暂存面即 Act 结论产生后的同一代码面），真板结论按操作者串口原文记录核对。未重复运行已采信验证；定向审计仅针对声明与代码矛盾点（见 Finding 2）。

**声明与代码一致性（独立检查通过）**

- T1：root/kernel/平台 Cargo 的 `k3-smp` 闭包（k3+smp+平台 smp/runtime-irq+kernel k3-smp）与递归 make 变量优先级注释一致；Makefile `k3-smp` 目标 SMP=1/>16 拒绝且 TARGET_DIR/OUT_CONFIG/OUT_DIR 全隔离；`smp_boot.rs` 决策核心（0 号/越界拒绝、16B 栈对齐、check_hsm、PRIMARY/SECONDARY_LATER_STEPS、steps_are_local_only）；`boot.rs::_start_secondary` 装已发布 PT+fence.i+high-half SP，不清 BSS、不共用主 SP；`power.rs::cpu_boot` 三段检查任一拒绝走 `smp_boot_failed() -> !`（结构上不可能返回 runtime 等待）；`init.rs` 次级 early 仅 trap、later 按本地步进表分发且 boot-only 步进 fail-closed park。
- T2：`ipi_irq.rs`（`SOFT_IRQ_VECTOR=(1<<63)|1`、SoftSlot CAS 单槽、register 只开 caller SSIE、重复 inert、handle 先 ack SSIP 再调 handler、无 handler 仍 ack 不造成功调用、`unregister_irq()==None`、set_local_soft）；`timer_irq.rs::set_local_timer_enabled/set_local_timer` 纯增；`irq.rs` set_enable 对 timer/soft 只动 caller CSR、SMP 注销恒 None、单 hart mask-before-remove 在 `all(runtime-irq, not(smp))` 下原样保留、非 smp 设备分支逐字保留。
- T3：`aia.rs` BOOT_HART 去 cfg + `boot_hart()`、`set_source_gate` 单写 SETIE/CLRIE、`smp_register/smp_unregister` 远端拒绝零硬件副作用、`smp_set_enable` 仅全局门且白名单外 no-op；`k3_smp.rs` Stage A–D 与 Task Contract 对应（confirm_blocked 先证真 Blocked、STIE 恢复先于终态、Stage D 重开后新 SETIPNUM 才作投递证明、`run_native_platform() -> !`）；`entry.rs` k3-smp 分支先于轮询分支且轮询分支补 `not(feature="k3-smp")`；四个 harness 均以 `#[path]` include 真实生产模块对 fake CSR/APLIC 后端行使（aia 独立双文件模型），非同义反复。
- T4：真板结果由操作者串口原文录入 Act Response，A1–A4 判定行与代码输出格式一一对应；S04 板面观测偏差有用户裁决原话与风险留档，接受现证合理（host `local_timer_mask_preserves_other_hart` 已证同一隔离语义，Stage C 以更强形态承载）；`make k3` 目标未动；暂存集无 axtask/axruntime/critical-section 文件（直接消费既有 API 的声明成立）；Invariants 1–5 在代码中成立；Evidence mode none 被遵守（change 内无 evidence 目录，正确）。

**Findings（按偏差分类）**

1. [Act 偏离 / Minor，不阻塞 Acceptance] `.gitignore` 既有尾行 `tests/network_benchmark-host-asan`（文件无结尾换行）被追加内容拼接成单行 `tests/network_benchmark-host-asan/out-k3-smp/`：Act 声明的"补 `out-k3-smp/`"未实际成立，且旧产物 `tests/network_benchmark-host-asan`（531 KB ELF，8 月 5 日既有构建，先于本变更）失去忽略规则、现已随暂存集进入提交范围。`out-k3-smp/` 现仅含 .bin/.elf，恰被 `*.bin`/`*.elf` 覆盖，故暂无实际 git 噪声。提交前修复建议：该行拆回两行（`tests/network_benchmark-host-asan` + `out-k3-smp/`）并 `git restore --staged tests/network_benchmark-host-asan`。
2. [Act Response 笔误 / Minor，不阻塞] aia harness 声明"20/20 ok（16 既有 + 4 新增）"：文件内实有 22 个 `#[test]`，其中 2 个被 `uart-irq` cfg 门关闭，可运行恰为 20，运行结论与代码一致；但分解实为 17 既有可运行 + 3 个新测试（第 4 个新增 fn `remote_eie_all_zero` 为 helper，误计入）。运行结果与全部 Plan 要求的新测试名均在，不影响判据。
3. [范围外实质缺陷 / Issue 候选，不落账] 暂存的 `.agents/memory/references.md`（MS13 收尾遗留的在先改动，非本 change Act 产物，Plan Context 已记录"不能覆盖"且 Act 确未触碰）新增 ISS02 行使用 `<!-- R77 -->`，与 HEAD 既有的 gdb 取证 Runbook R77 重号；R78（ISS03）无冲突。建议提交前由 docs-maintainer 将 ISS02 引用改号为 R79。
4. [提交集卫生 / 用户决策] `.axconfig-k3-smp.toml` 为 Makefile OUT_CONFIG 每次构建再生成产物，`/.axconfig.*` 忽略模式覆盖它且仓库不跟踪根级 `.axconfig.toml`；现已暂存入库。建议不跟踪，随 Finding 1 一并处理。
5. [既有改动保全 / 无偏差] references.md R 行与 ISS02/ISS03 文件为在先用户改动，Act 未覆盖未删改；是否随本 change 提交由用户裁决。

**Acceptance Gaps**: None。A1–A4 全部满足且有真实运行记录；上述 Minor findings 不阻塞任何 Acceptance，按公共规则不创建返工。

**Issue 候选（只报告不落账）**：`k3-device-irq-host-harness` 2 个既有失败（Act 已做 HEAD 对照复跑，同一失败集，证明先于本变更）；references.md R77 重号（Finding 3）。

**Next Iteration**: `iterations/001-nic-multi-hart-steady-state/000-initial.md`（Plan Context draft，待用户 Gate 2 批准后交 Act）。
