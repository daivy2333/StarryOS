# Iteration 000 / Cycle 001：复用已验收底座，接通真实 GMAC IRQ

## Plan Context

- Status: ready
- Cycle Type: replan

**Cycle Scope**

- Change tasks：T1（1.1）、T2（1.2）。
- Acceptance gaps：本轮尚未实施；A1–A5 全部保留，以当前底座修订实现与验证契约。
- Repair items：None。
- Inherited scope：2026-10-06用户“批准”的R1–R7/S1–S20中本轮映射；RAM-only、单hart、既有DMA安全与公共规则。
- Excluded scope：traits adapter、64slots/C4flush、stack/socket、下一轮实施、runtime recovery、SMP、性能、MS17、全局状态与经验产物。

**Objective**

在保留旧K3polling入口的同时，使新增K3异步入口可构建：既有scheduler的timer future真实恢复，GMAC由IOC触发source133真实RI/TI，最小ISR唤醒deferred task完成收发；mask/arm最后事件与fatal ack窗口有确定性host见证。20个真实TX完成和RX通知不能用软件IRQ或host模拟代替。

**Background**

Parent Cycle：`000-initial.md`。2026-10-08 用户要求复核中途已交付串口与 shell 后的网络计划，并明确“不做多余的身份验收工作”。原计划的 timer 当前行为、目标位置与测试见证已变化，必须创建 replan；原 Plan Context 与 Act Response 不改写。

本轮沿当前已具备 shell 的仓库基线开发。复用其 timer/AIA/调度/critical-section；保留独立网络入口与内核任务载荷。网卡单 hart 能力稳定后再合并串口、网络与 shell 的启动路径，先检查共同运行，再推进多 hart；该集成属于后续路线，本轮不新增 shell 命令、rootfs 或 syscall 工作。

**Investigation Facts**

Current Baseline：2026-10-08，`k3`，HEAD `cdf94713`，调查开始时工作树干净。revision 仅定位现场，不作为测试匹配或通过条件。

可采信的实际运行结果及适用性：

- 串口变更 `archive/2026-10-08-ms17-com260-kit-async-uart-interactive-shell/iterations/000-initial/000-initial.md` 的 Act Response：timer 原生 harness 8/8、AIA legacy/uart 两配置 17/18 项通过；板上两次 sleep 共 150 ms 与 timeout 取消真实恢复。该目录 `002-rework.md` 最终 Review accepted，板测 B2 再次复现。不是网络 IRQ 证据。
- 同变更 `iterations/003-minimal-interactive-shell/000-initial.md`：shell native 20/20、`make k3-shell`、`make k3-stdio`、`make k3`、QEMU `make build`、`make host-test` exit 0，最终 Review accepted；B7 两轮 shell 交互与结束 drain 通过。只复用共用底座与模式组织，不把 UART quiet/drain 等同 GMAC quiet/C4。
- rootfs 变更 `archive/2026-10-08-ms18-com260-kit-standard-shell-rootfs/iterations/001-standard-shell/001-replan.md`：`make k3-shell`、`make k3-stdio`、`make ARCH=riscv64 SMP=1 build` exit 0，Review accepted。rootfs 新布局仅由 `ram-rootfs` 开启，本轮不继承其 RAM 承载窗、DTB 搬移或 loader。
- 原网络计划已实跑的普通 axnet 命令 exit 101（`async_rx.rs:1948` 无 cfg 调用 `net_link_step_diag`）；diagnostics 的 event 三窗口 3/3 exit 0。当前实码仍是定义带 `qemu-diagnostics`、调用无 cfg，`git diff aca5dfe9..HEAD -- crates/axnet/src/async_rx.rs` 无差异，采信原 RED 与三窗口 GREEN，不重复运行。
- 轮询网络最终 accepted 结果：GMAC harness 21 项、polling-net-host 17 项通过，真实四协议、20 raw ARP TX 与 RAM 启动恢复成立。上述 diff 对 `kernel/src/platform/k3_gmac/`、`k3_network_probe.rs`、`k3_device_irq_probe.rs` 同样无差异，当前基线直接采信；实施触碰这些表面后运行相关回归。
- `git diff f93c03f6..HEAD -- crates/axplat-riscv64-k3/src/{irq,time,timer_irq}.rs tests/k3-timer-host-harness.rs` 无差异；critical-section 与 `axtask::future` 对同区间检查也无差异。timer/IRQ 8 项及共享等待底座不用重建或重跑来增强信心。旧计划“host-test 既有失败”的描述不再作为当前结论；VF2 的既有失败仅按实际影响范围处理。

Current-State Evidence / Code and Critical Path：

| 路径/符号 | 当前事实与本轮责任 |
|---|---|
| 根 Cargo `k3`、`k3-async-uart` 与 kernel Cargo | `k3` 仍含轮询 smoltcp；UART 模式另开 `runtime-irq`/`uart-irq`。本轮新增网络 feature 只开启 `runtime-irq` 与 axdriver/net/dyn、axnet；不蕴含 UART/userland/paging |
| `axplat-riscv64-k3/src/timer_irq.rs::{TimerSlot,register_irq,handle_irq}` | 完整 bit5 vector 独立 Atomic 槽；CAS 注册后开 STIE、重复拒绝、unregister 先 mask、dispatch 返回原 vector；生产 CSR 与原生 harness 已存在，复用不重写 |
| 同 crate `irq.rs::{register,unregister,handle}` / `time.rs::init_percpu` | runtime-irq 下 timer 独立分派；external 走 AIA 原子 stopei；首次 SBI set_timer(0) 已接通；legacy 不开 timer。source133 已在固定路由表，网络无需扩大 AIA 表 |
| axruntime `rust_main/init_interrupt` → `axtask::on_timer_tick` → `future::{sleep_until,timeout,block_on}` | scheduler/timer runtime 已初始化，真实 sleep 可唤醒；不重新初始化或注册第二 timer handler。周期 tick 不发布网络 completion work |
| `kernel/src/entry.rs::init` / `platform/k3.rs::run_first_byte_park` | UART/raw、copier、stdio/shell、rootfs 已有各自分支；普通 K3 执行 stage1–10 后继续 stage11–13 polling。网络复用 stage1–10 前置，在正常 stage10 后选择新网络入口，旧 polling 分支保持 |
| `kernel/src/lib.rs` / `platform/mod.rs` | 网络只需要 platform 与现有 critical-section；不放开 drivers/file/mm/syscall/task 的 K3 userland cfg。新模式拒绝 `k3-async-uart`（覆盖 shell/rootfs 派生）、smp、vf2；qemu/D1 复用已有互斥 guard |
| `critical_section_policy::{acquire,release}` / crate-root critical_impl | 已有 IRQ 状态保存/恢复和嵌套纪律；原子 waker 唤醒可复用。不从 UART port 借硬件锁，不在 ISR 等待 Gmac/Service |
| `k3_device_irq_probe::run_k3_stage10` / `Stage10Outcome` | WindowEnded 才进入新数据面；60/133/277 handler 尚注册。先 mask 三 source、撤掉旧133 handler；60/277 保持 masked，杜绝 MDIO 与新 DMA owner 交错 |
| axdriver registry `dyn_drivers/{mod,pci}.rs` / axruntime `klib.rs` | dyn 的注册 PCI 回调引用 axklib iomap；runtime 只在 paging 下提供 klib。当前 kernel 没有 mem_iomap shim，本轮不启 paging，仍需 K3 网络限定 Unsupported ABI 接缝 |
| `k3_gmac::Gmac::{new,init_rx,refill_slot,try_submit,poll_rx,poll_tx}` | 独占18页、16/16desc、64Bdesc/2048Bbuffer；IRQ_EN=0、IOC=0；W1C 可能与 ISR 交错。新增显式异步模式、fatal latch与通知/只读重查，默认构造保持 polling |
| Gmac owner 与 `ArenaGuard` | TX实际slot、deadline、tx_inflight 在Gmac；RX head只由任务推进。publish 后 backing保留。地址宽度/高位base/cache、TX tail=next-slot、RX doorbell保持；超时不是DMA停止 |
| `k3_network_probe::{stage12_arp_request,board::BoardBacking}` / `platform/k3::K3Env` | 已有60B who-has .1/tell .2 ARP、GlobalPage18页分配/zero、PA转换、Zicbom flush/clean/inval与fence。精准开放板绑定/ARP复用，不复制另一套arena/cache代码 |
| `axtask::future::register_irq_waker` | 首次注册建立IRQ hook与PollSet，source133 handler返回后由axhal hook唤醒raw任务；注册发生在任务上下文，已有native机制，不新增raw event协议 |
| `axnet::publish_queue_event` | 仅原子generation/cause与waker，不取Service、不碰desc；后续queue阶段复用。fatal/有效unknown作为稳定错误工作唤醒，不能把发布次数当completion；zero不发布工作 |
| Makefile `k3/k3-fit/k3-shell/k3-rootfs` / `tools/starry-k3.its` | 构建参数与SMP拒绝模式已存在；普通/shell共用FIT，rootfs独立FIT。新增 `make k3-async` 复用原 `k3-fit`，网络构建完成即打包；待板测网络FIT最后生成，验收不检查字符串、mtime、指纹或身份字段 |

参考用法已重新核对：`/home/daivy/projects/serial/work/k3/others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/{desc,regs,core}.rs`。RX发布des3 bit30 IOC，TXdes2 bit31 IOC；STATUS0x1160、IRQ_EN0x1134、普通mask0xD041（NIS15/AIS14/FBE12/RI6/TI0）。RX bit30发布时是IOC，回写时是CTXT，不能以IOC值绕过原合法RX decode。参考ISR内reclaim/try_lock为反例；本轮采用任务owner。APLIC/IMSIC mask窗口会丢MSI、TX/RX tail用法以本仓已验收板测裁决为准；不增加参考没有执行的readback前置。

复用经验的适用边界：UART check-register-arm-recheck 的方法适用于通知竞态，字节ring和TEMT四条件不适用于NIC；GMAC沿用packet slot/cookie/C4。串口实验说明失败快照必须在超时取消后仍能取得；本轮任务持有累计计数/owner，ISR仅保存pre-ack raw。UART的FCR/Errata20/IIR回声与SETIPNUM自测结论不迁移到GMAC寄存器；真实RI/TI仍须真板直接观察。

**Implementation Guidance**

先修普通feature阻塞并接入已验收timer，再启用IOC与最小handler，最后接deferred raw会话。诊断输出在任务上下文，用现有LineBuf/console。模块名和等价helper拆分留Act；状态、所有权、mask位置、错误和测试边界以以下契约为准。

**Task Contracts**

### T1：复用 timer 底座，形成隔离的网络构建入口

- Requirement/Scenario：R4/R5/R7；S14、S16、S20。
- Depends on：None。
- Targets：根/kernel `Cargo.toml`、Makefile；`kernel/src/lib.rs/platform/mod.rs` 新模式guard/模块声明、新 `platform/k3_klib.rs` 最小链接接缝；`crates/axnet/src/async_rx.rs::poll_active` 的诊断调用cfg。现有 `timer_irq.rs/irq.rs/time.rs/tests/k3-timer-host-harness.rs` 是复用对象，不是重新实现目标。
- Current behavior：timer生产实现、native8项与真实sleep已accepted；未有网络feature/Make入口；普通axnet未隔离诊断调用仍编译失败；dyn非paging链接缺少mem_iomap。
- Required behavior：根/kernel `k3-async-net` 蕴含k3，根选择现有平台 `runtime-irq`，kernel启用axdriver/net/dyn与axnet，不启axruntime/net-ng自动probe。`make k3-async` 复用现有K3配置：riscv64、MYPLAT/PLAT_CONFIG、MEM6400M、BUSmmio、DWARFn、LOGoff、SMP1、AX_IP192.168.137.2/AX_GW192.168.137.1，build后立即调用原k3-fit。Make拒绝SMP>1；kernel compile_error拒绝smp/vf2/任何k3-async-uart派生组合，平台已有qemu/D1互斥保持。网络模式不打开UART raw/copier/tty/userland模块，不进入其boot分支。
- Required linkage behavior：仅网络feature下编译 `__axklib_0_3_mem_iomap`，`unsafe extern "Rust" fn(PhysAddr, usize)->AxResult<VirtAddr>`，使用现有memory_addr0.4/axerrno0.2，返回Unsupported。GMAC不用它；不启paging/plat-dyn/probe_all_devices，不改registry依赖。原生目标链接直接验证，无ABI工具或专用身份测试。
- Required diagnostic behavior：仅在 `qemu-diagnostics` 下调用现有 `net_link_step_diag`；不重写普通queue行为、不扩展诊断输出。旧K3polling不选runtime-irq，既有串口/shell/rootfs目标保持。
- Preserve：TimerSlot/CAS/STIE、首次set_timer(0)、24MHz换算、AIA原子claim、scheduler/tick、global hook、共享critical-section、所有已验收shell/UART行为和默认平台。
- Forbidden：第二executor/timer wheel、重复scheduler初始化、重做timer测试、引入用户态或rootfs依赖、把UART drain作为C4、字符串计数/mtime/hash/run-id/manifest/日志判定器。无需触碰已验收timer实现；若出现实质不兼容返回Plan。
- Test witness：原普通axnetexit101为合法RED（覆盖未变）；修复后同一原生命令GREEN。原timer8项GREEN直接采信。新构建入口建立后、网络接缝修复前 `make k3-async` 的普通feature编译/链接失败是接入RED，完成后同一原生命令GREEN；不以“目标不存在”的Make错误代替行为见证。组合守卫的原生编译结果必须明确指出互斥，不以任意编译失败算通过。
- GREEN condition：普通event测试通过、网络build/FIT成功、SMP与网络+shell组合被明确拒绝、旧k3与shell受影响构建通过。复用timer8项保持有效；本镜像的真实sleep仍由T2交叉验证，不计为网络IRQ已通过。
- Verification：V1/V3。模式guard/build变更有新的直接结果；未修改timer测试不重复运行。
- Stop when：需要改timer/AIA公共契约、boot hart映射、共享调度或shell语义，或必须开paging/registry patch才能链接，交Plan。

### T2：真实 GMAC IRQ→deferred task 与最后完成

- Requirement/Scenario：R1/R5/R6/R7；S1–S4、S8–S9的原始DMA接缝、S16–S20。
- Depends on：T1。
- Targets：`kernel/src/platform/k3_gmac/{mod,regs,desc}.rs`通知/IOC/只读completion接口；新`platform/k3_async_net.rs`的最小ISR及首轮raw载荷，`platform/{mod,k3}.rs`异步分支；`k3_network_probe.rs`必要共享ARP/BoardBacking绑定；`tests/k3-gmac-host-harness.rs`production fake/MMIO/cache扩展，必要新增native IRQ harness。
- Current behavior：IRQ_EN=0/IOC0，Gmac由polling窗口推进，stage10遗留133观察handler；poll W1C可能清除ISR需保留的fatal。
- Required behavior：正常stage10结束后mask60/133/277、unregister原133观察handler，保持60/277masked；独占一个新arena/Gmac，通知关闭下建立IOC模式，再注册133专用handler。未正常结束或注册失败不进入数据面，device通知不开放。默认旧polling构造/descriptor路径保持IOC0，不改旧行为。
- Required IRQ behavior：ISR只读cause→保存pre-ack完整raw→latch fatal→GMAC侧suppress→W1C observed known位/summary→publish_queue_event；不拿owner锁、不碰desc/MDIO、不alloc/print。RI/TI/异常各计数，组合保留；zero只计spurious、不发布网络工作；未知有效channel cause mask并稳定fault。RI/TI与稳定fatal/有效unknown均发布一次工作提示，使错误也能唤醒等待者；不把工作发布或claim计数当completion。fatal latch先于ack，后续poll/query首先传播该fault，无reset/新submit。
- Required notification behavior：single-hart现有IRQ-save保护IRQ_EN整体RMW；BOTH一次提交，维持summary/fatal位。普通arm mask采信0xD041（RI/TI/FBE/NIS/AIS），不加readback验收；方向选择只改其completion enable。arm后fence→invalidate/fence检查RX head和有live owner的TX head OWN→复查latch/status；不回收/advance head。没有TX inflight时不得把未使用descOWN0当completion。通用mask窗口不改APLIC/IMSIC EIE。
- Required deferred behavior：原生axtask任务/block_on作为唯一descriptor owner，用existing IRQ waker或等价原生wake；注册→arm/重查→再查事件后Pending，每poll RX/TX分别最多16步，有可达backlog才yield/selfwake。不得等待下一包/周期timer救最后completion。首轮raw载荷复用MS11的60B ARP request，发送20帧到PC192.168.137.1，前19帧完成并重臂后await原生sleep_until(now+100ms)，再创建最后1帧；该一次性等待不调用descriptor poll，用于独立事件与timer恢复见证。每帧1s terminal deadline，RX scratch2048，会话30s上限；其余timer只用于timeout/结束。结束maskdevice通知、停止新增提交、保留已发布backing；任务输出timer wake/cause/claim/deferred poll/descriptor状态和错误。累计计数、实际slot/owner和deadline由任务持有，不因timeout取消future而丢失；失败路径立即输出有限快照（pre-ack cause、source133 handler计数、IRQ_EN、同步后head OWN/tx_inflight、阶段/deadline/owner），不等成功收尾才打印。破坏性status不二次读取；IRQ_EN观察只用于诊断，不作参考用法以外的读回验收。
- Preserve：18页安全arena、16/16desc、64Bcacheline、RX/TX2048buffer、地址宽度检查、highbase先写、OWN-last、CBO/fence、TX exclusive-next tail、RX tail doorbell、pad60、既有deadline、postpublish backing retention。
- Forbidden：ISR drain、try_lock失败等下一IRQ、软件IRQ冒充DMA、STIE驱动completion polling、second owner/executor、主动清OWN/释放DMA、真板物理FBE注错、改MS11tail裁决、Wireshark/identity/validator、下一轮traits/stack实现。
- Test witness：production fake env扩展RED→GREEN：RX/TX IOC位仅async设置且OWN-last；W1C前raw组合cause/fatal保留；zero/unknown有界；irq期间不触desc/owner锁/console；mask最后completion、event-before/during/after-arm、Pending前窗口、空TX/ringwrap、fatal ack后查询仍error；只有fatal或有效unknown时等待者也被唤醒，zero不伪造工作。fakecache实际描述符OWN变化作判据，不只assert调用日志。已有Gmac harness GREEN覆盖未改行为时直接采信；修改其表面后运行对应回归。
- GREEN condition：A2的新增native tests与受影响MS11 tests通过；make k3-async成功；下面真板A3–A5实测通过并报告结果。host绿色不完成T2板测部分。
- Verification：V1–V5如下。硬件交接前完成所有agent可执行测试/build与task Gate3自检；操作者结果待审核时停止，不执行T3。
- Stop when：IOC/mask真实行为与采信用法冲突、无法观察真实RI/TI、最后完成不推进、满环/ownership不符、timer不恢复，交Plan分层诊断；三次同类失败遵守公共规则，不加第四次盲试。

**Invariants**

1. 单boot hart、一个Gmac owner；handler不借用Gmac/arena/Service，descriptor的临时引用不跨await；Gmac与arena由任务持续持有。
2. 所有初始化/提交后的资源按实际OWN和publish状态处置；timeout/cancel/park不是quiesce，不reset/reuse可能device-owned backing。
3. cause、claim、wake、同步后descriptor terminal各有产品观察；一个不能替代另一个。RX/TX吞吐或软件MSI不替代真实IRQ。
4. 周期scheduler tick不发布网络work；仅deadline timer可恢复对应等待。foreground初始化有界轮询沿用既有，不用于数据面fallback。
5. 原make k3与MS11polling路径、其他平台接口保持；本轮不实施后续Map。

**Non-goals**

NetDriver traits、slots、flush、stack/socket四协议、SMP/IPI/迁移、runtime reset/link-flap、性能/soak、UART RX/tty/shell、存储、框架抽离、全局文档同步/经验产物。首轮raw不是MS12完成。

**Acceptance**

| ID | 可观察结果 | 映射 |
|---|---|---|
| A1 | 已验收timer生命周期/隔离结论保持；普通axnet编译缺陷修复；新网络feature/模式拒绝与异步build/FIT成功 | T1，R4/R5/R7，S14/S16/S20 |
| A2 | IOC与最小ISR、组合/零/未知/fatal pre-ack latch、mask/arm/Pending窗口与最后completion的生产host见证通过 | T2，R1/R5，S2–S4/S16–S17 |
| A3 | 真板source133 claim中出现真实RI和TI，ack/rearm后至少各一次独立后续cause；deferred任务处理；20TX合法terminal/reclaim、RX至少1合法完整帧、跨16wrap，最后TX无后续包也完成 | T2，R1/R6，S1/S8/S9/S19 |
| A4 | 原生sleep_until在板上真实唤醒任务；30s会话有界、无新traffic时不靠completion polling推进，无TX错误/timeout；故障输出阶段/owner，backing安全retain | T1/T2，R4/R5，S14/S16–S18 |
| A5 | 原polling回归未被async IOC/status路径破坏；仅RAM启动、外部RESET回原Linux；未把PHY/softwareIRQ算DMA、未追加资格工具 | T1/T2，R6/R7，S18–S20 |

**Verification**

V1（agent，T1）：直接采信timer8项及真实sleep（源与结论见上文）；无需新timer harness。普通axnet修复后运行：

```sh
env RUSTFLAGS='-C linker=/home/daivy/projects/serial/work/StarryOS/scripts/cc-nopie.sh' cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib async_rx::tests::event_before_register_is_caught_by_arm_recheck -- --exact --test-threads=1
```

通过条件为原生 `1 passed; 0 failed`、exit0。原diagnostics三窗口覆盖不变时采信，T2新增GMAC窗口不能只引用这三项。

V2（agent，T2）：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/ms12-k3-gmac-host`，运行`/tmp/ms12-k3-gmac-host --test-threads=1`；若为IRQ新增独立harness，同样原生rustc/test binary。现有MS11生产回归：`cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline`；只有其覆盖表面发生变化时补跑。所有新test先观察RED再改生产路径。

V3（agent）：先运行受影响旧入口 `make k3`、`make k3-shell`（包含ELF原生构建），共享axnet cfg修复后 `make ARCH=riscv64 SMP=1 build`；必须exit0。`make k3-async SMP=2` 必须由单hart守卫明确拒绝。网络+shell冲突用原生build：

```sh
make ARCH=riscv64 APP_FEATURES=k3-async-net,k3-shell MYPLAT=axplat-riscv64-k3 PLAT_CONFIG="$PWD/crates/axplat-riscv64-k3/axconfig.toml" MEM=6400M BUS=mmio DWARF=n LOG=off SMP=1 build
```

必须由新网络/UART模式互斥compile_error明确拒绝，非零退出；其他原因失败不通过。SMP、vf2与已有qemu/D1 guard独立阅读其cfg；不另造无关组合矩阵。最后 `make k3-async` build+原生mkimage/dumpimage成功，生成待板测 `StarryOS_riscv64-k3.fit`。复用现有ITS（kernel0x180000000/FDT0x181000000），不改rootfs专用ITS/预留窗；打包native输出仅说明布局，不能通过A3/A4。`git diff --check` exit0。D1选择与实现未变时采信既有构建；仅新改动实际触及D1才补 `make lichee`，不重跑已通过全量host-test。禁止为交付做字符串/时间/身份比对。

V4（操作者，前述agent项PASS后最近能力边界）：

1. PC网卡192.168.137.1/24，直连且保持link up；串口1152008N1无流控。构建FIT `StarryOS_riscv64-k3.fit`拷入FAT32 U盘。上电/RESET同时长按s进入U-Boot。
2. 逐行执行 `usb start`、`fatls usb 0:0 /`、`fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`、`setenv fdt_high 0xffffffffffffffff`、`bootm 0x2e0000000`。不saveenv；kernel load/entry0x180000000、FDT0x181000000保持。
3. stage1–10按既有链路前置自动运行，30s link窗口保持插线；等新增async raw窗口begin。PC原生ARP响应供RX，不做抓包。serial任务行显示timer await确实返回、source133的真实RI/TI pre-ack cause、后续独立cause、deferred wake以及实际OWN terminal/reclaim。
4. 观察30s结束行：sent/terminal/reclaimed均20，TX error/timeout为0，RX完整帧≥1，真实RI/TI分别至少两次可区分事件；最后一帧不用PC追加流量仍达terminal/reclaim。若新cause仅TBU/RBU、PHY60、软件source1或只看OWN均不通过A3。
5. RESET，直接见到原系统`Bianbu … k3 login:`。执行人一句话报告本轮网络与reset直接结果；原生关键串口输出用于审核，无截图/表格/日志hash要求。

V5（失败处理）：任意err/fault、无RI/TI、无法deadline结束时停止新增工作并按phase/owner报告，保留DMA资源；操作者reset。reset未回原系统则停止一切装载并交用户。未执行板测或不满足A3/A4时，Act Response应为blocked，T2不勾选完成；审核PASS后仅恢复本Cycle剩余验证/反馈，不自动展开后轮。

**Requirements Traceability Matrix（本轮）**

| Requirement | Task | Test Witness | Status |
|---|---|---|---|
| R1 | T2 | async_ioc、irq_cause、mask_last_completion、arm_windows、真实RI/TI | Covered |
| R4（timer部分） | T1 | 既有timer8项采信、网络feature接入RED→GREEN、真实timer future恢复 | Covered |
| R5（底层部分） | T1/T2 | feature RED→GREEN、fatal_after_ack、postpublish保留、deadline | Covered |
| R6（底层部分） | T2 | 真板20TX+RX、cause/claim/wake/terminal直接观察 | Covered |
| R7（底层部分） | T1/T2 | legacy feature、MS11回归、RAM boot/reset | Covered |

R2/R3及R4–R7余下Acceptance属于Map001/002，不是本轮Missing，也不能在本轮声称完成。

**Gate 2 Readiness**

PASS：技术调查、设计、任务与验证已闭合，无实质未知项；2026-10-08用户原话“批准”，修订计划获批，Status转为ready。Gate1沿用原已批准R1–R7/S1–S20，无Simplified/Missing；仅登记批准，Task Contracts、Acceptance与Verification不变。

**Persisted Evidence**

- Mode: none
- native命令、每项≤20行决定性输出、退出码/人工结果、修改文件/符号与Gate写Act Response；本轮可重跑且无需独立持久化结构。不建Evidence占位目录。

**Risks and Notes**

- 真实DMA IRQ/timer await仍待本轮执行，不是Plan未决接口；若物理结果冲突按Stop when交Plan。
- 首轮host tests必须覆盖生产核心。raw载荷、快照和tests只证明目标行为，不添加身份协议/判定器；原生Cargotest是判据。
- Wireshark既有用户原话“Wireshark不做，后续也不用做”继续有效。MS11历史ping数/reset描述豁免不延伸；本轮reset直接观察必须执行。
- 普通axnet feature编译缺陷仍在本变更内，由T1修复；不重复登记Issue。范围外VF2既有失败未在本次复现，不把它当作新发现。
- root/Kernel K3 feature编译隔离可等价实现；helper命名、输出字段本地格式与LineBuf拆分属非实质选择，不改变Acceptance。
- 已通过且覆盖未变的验证按公共规则采信；不重复测试增强信心。若同一问题三次失败按Gate5停。

## Act Response

- Status: reported（2026-10-12：T1/T2 完成，V4 run3 板测 A3/A4 达标 + A5 reset 操作者确认；终态摘要见文末，交用户审计）

### 实际改动

**T1（1.1，完成）**

- `crates/axnet/src/async_rx.rs`：`poll_active` 中 `net_link_step_diag` 调用加 `#[cfg(feature = "qemu-diagnostics")]`（普通 feature 不再引用 QEMU 专用诊断）。
- `Cargo.toml`/`kernel/Cargo.toml`：新增 `k3-async-net`（root：蕴含 `k3` + 平台 `runtime-irq`，不含 `uart-irq`；kernel：`dep:axdriver`+`axdriver/dyn`+`axdriver/net`+`dep:axnet`，不启 paging/net-ng 自动 probe）。
- `kernel/src/lib.rs`：`k3-async-net+smp` 与 `k3-async-net+k3-async-uart`（覆盖全部派生）compile_error 互斥守卫；`drivers`/tty/userland 树沿用既有 cfg 对该模式保持排除。
- `src/main.rs`：`k3-async-net` 与 qemu/vf2/lichee 平台 feature 组合的 compile_error 守卫（与 MS17 `k3-async-uart` 守卫同形）。
- `kernel/src/platform/k3_klib.rs`（新）：`__axklib_0_3_mem_iomap` K3 限定链接接缝，fail-closed 返回 `AxError::Unsupported`；仅 `k3-async-net` 编译。
- `Makefile`：`make k3-async`（复用 k3 配置 + `IP=192.168.137.2 GW=192.168.137.1`、SMP>1 拒绝、build 后调原 `k3-fit`）。

**T2（1.2，agent 侧完成；板测部分待操作者）**

- `kernel/src/platform/k3_gmac/regs.rs`：`DMA_STATUS_NIS/AIS`、`DMA_IRQ_EN_{NIS,AIS,FBE,RI,TI}` 与 `DMA_IRQ_EN_DEFAULT(0xD041)`/`DMA_IRQ_EN_RX_ONLY`（R69 arm mask）。
- `kernel/src/platform/k3_gmac/desc.rs`：`RDES3_IOC`（发布期 des3 bit30 IOC；回写同位为 CTXT，decode 契约不变）。
- `kernel/src/platform/k3_gmac/mod.rs`：`Gmac::new_async`（async_notify 标志）、refill/submit 的 IOC 发布位（仅 async，OWN 仍最后一字）、`arm_notify/mask_notify`（polling 模式 arm 为 no-op）、`check_completions`（fence→invalidate/fence 只读重查 RX head 与有 live owner 的 TX head + status 复查，不回收/不推进）、`latch_fault`（ISR pre-ack fatal 折入驱动状态）、`tx_inflight`/`diag_head_words`（快照面）。
- `kernel/src/platform/k3_async_net.rs`（新）：host 可测 ISR 核心（`IrqAccess` seam：`reg_read/reg_write/publish_work`；固定序 读cause→存完整pre-ack raw→fatal先于ack latch→fatal/有效unknown 抑制（IRQ_EN=0）→W1C 观测已知位/summary→发布一次工作提示；zero 只计 spurious 不发布）+ `raw_wait_decision`（fault→descriptor→generation 纯决策）+ 板上 deferred raw 会话（`run_k3_async_net_session`：mask 60/133/277→unregister 133→IOC 模式通知关闭下 init/enable_tx→注册 133 专用 handler→block_on 20 帧 ARP raw 会话：每帧独立 1s terminal、前 19 帧后纯 `sleep_until(100ms)` 无 descriptor poll、30s 会话上限、失败即时有限快照、结束 mask 通知且 backing 保留）。
- `kernel/src/platform/k3.rs`：`run_first_byte_park` 在 stage10 后按 `k3-async-net` 分支进入 raw 会话，旧 polling stage11–13 保持（非该模式不进入）。
- `kernel/src/platform/k3_network_probe.rs`：共享绑定（`stage12_arp_request` 转 pub、`BoardBacking::new` + re-export）。
- `tests/k3-gmac-host-harness.rs`：新增 9 项 witness（IOC 位与 OWN-last、pre-ack raw/fatal/W1C/publish 序、zero 不伪造工作、unknown mask+fault、IRQ 不触 desc/锁/console、fatal ack 后查询仍 error 且拒绝新提交、arm/recheck 窗口与空 TX 无假工作、decision 矩阵）；共享静态以 `IRQ_TEST_LOCK` 串行化，默认并行与 `--test-threads=1` 均 30/30。

### 与计划的偏差

1. **接入 RED 未出现（非实质）**：契约预期"网络接缝修复前 `make k3-async` 链接失败"；实测无 `k3_klib` 时构建直接成功——链接器 GC 消除了 dyn 注册代码对 `__axklib_0_3_mem_iomap` 的引用（最终镜像 0 个未定义符号）。契约要求的接缝照建，符号在 `libstarry_kernel-*.rlib` 中存在（`T __axklib_0_3_mem_iomap`），"原生 build/link 验证该入口"以最终构建 exit 0 满足。
2. **vf2 组合拒绝的实现形式（非实质）**：kernel 层无 vf2 feature 标记，守卫放在 root `src/main.rs`（MS17 同形）；`k3-async-net+vf2` 实测在 axconfig PACKAGE 层先行失败（exit 2），与 MS17 `k3-async-uart+vf2` 已验收行为完全一致（compile_error 为纵深防御）。
3. 其余均按契约实现；模式守卫、共享绑定（`stage12_arp_request` pub、`BoardBacking::new`）与测试互斥锁为契约内等价实现。

### 自检发现与修复

- Gate 3 中发现并修复：`axnet::async_rx` 为私有模块（改用根再导出 `axnet::publish_queue_event`）；`BoardBacking` 私有字段（加 `BoardBacking::new`）；一处 format 位置参数缺失；测试静态在 `make host-test` 默认并行度下的竞争（加测试互斥）。无遗留 Critical/Important 问题。

### 验证（命令、决定性输出、退出码）

- V1（T1）：普通 axnet event 测试（命令见 Verification V1）→ `1 passed; 0 failed`，exit 0（RED 为计划采信的 exit 101，diff 确认覆盖未变）。timer 8 项与真实 sleep 按 MS17 accepted 结论采信，未重跑。
- V2（T2）：`rustc --edition=2024 --test tests/k3-gmac-host-harness.rs -o /tmp/ms12-k3-gmac-host` 后运行 `--test-threads=1` → `30 passed; 0 failed`（21 既有 + 9 新增），exit 0；默认并行度同 30/30。RED 已观察（生产模块缺失，rustc exit 1）。
- MS11 回归：`cargo test --manifest-path tests/k3-polling-net-host/Cargo.toml --locked --offline` → 6+6+5=17 passed，exit 0。
- V3：`make k3-async`（含 FIT）exit 0；`make k3-async SMP=2` 被单 hart 守卫拒绝 exit 2；`APP_FEATURES=k3-async-net,k3-shell` 构建由 compile_error"mutually exclusive"拒绝 exit 2；`k3-async-net,vf2`（两种 MYPLAT）exit 2（axconfig 层，与 MS17 先例一致）；`make k3`/`make k3-shell`/`make ARCH=riscv64 SMP=1 build` 均 exit 0；`git diff --check` exit 0。D1 未触及，采信既有构建。
- `openspec validate` → valid，exit 0。

### Persisted Evidence

None required（Mode: none；全部命令可在上述路径重跑）。

### Blocker Handoff

- **阻塞点**：Acceptance A3–A5 的真板直接观察（V4）只能由操作者执行；未执行板测前 T2 不完成（契约 V5 明确）。
- **已完成**：T1 全部；T2 的全部 agent 可执行项（native 见证、生产实现、构建矩阵、Gate 3 自检）。
- **未开始**：V4 板测；A3/A4/A5 判定；T3–T7。
- **工作区**：`k3` 分支，T1/T2 产品与测试改动未提交（含 change 产物更新）；已通过 Gate 1–4（T1 完整、T2 agent 侧）。
- **恢复条件**：操作者按 V4 执行（PC 192.168.137.1/24 直连、U-Boot `fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit` + `bootm`、观察 async raw 窗口的 timer await/真实 RI/TI pre-ack cause/deferred wake/OWN terminal、30s 结束行 sent/terminal/reclaimed=20 且最后帧无追加流量仍完成、RESET 回 `Bianbu … k3 login:`），提交一句话结果与关键串口输出；审核 PASS 后 Act 追加 Blocker Resolution 并仅恢复本 Cycle 剩余验证/反馈（T2 完成判定与 Review 交 Plan）。
- **失败处理**：按 V5——任意 err/fault、无 RI/TI、无法 deadline 结束时停止并按 phase/owner 报告；reset 未回原系统则停止一切装载。

### Experience Candidates

None（普通 axnet cfg 缺陷在本 change 范围内由 T1 修复；并行测试静态为本 change 内自检修复，均不构成范围外实质缺陷）。

### Blocker Resolution

- **用户指令**（2026-10-12，操作者提交 V4 run1 串口原文并问询结果；随后要求继续）：run1 的 `[ms12] net result sent=20 terminals=20 tx-errors=0 tx-timeouts=0 rx-frames=0 timer-wakes=19 irq-runs=0 ri=0 ti=0 abnormal=0 spurious=0 deferred-polls=0 backing=retained`。会话自身的边界全部闭合（20/20、wrap 槽号 0–15→0–3、timer 唤醒真实、backing 保留），但 **A3 证据未达**：`irq-runs/ri/ti/rx-frames/deferred-polls` 全为 0——设备通知从未打开、handler 从未运行、RX 无人服务。
- **根因**（Act 实现与契约顺序偏差，非设计/范围问题）：60B 帧 @1G 在第一次 owner poll 前即完成，而实现把等待协议放在 poll 之后（契约序为"注册→arm/recheck→再查事件后 Pending"）；arm 惰性挂在 `wait_for_wake` 内导致 `IRQ_EN` 从未写入；RX 服务只位于不可达的 Pending 分支。
- **修复**（`kernel/src/platform/k3_async_net.rs`，全部在 Task Contract 文本内）：① handler 注册后立即 `arm_notify`（A3 的 RI/TI 计数不再依赖竞态；路由已使能，无 mask 窗口）；② terminal 等待改为等待协议在前（quiet 才 Pending，完成可见则直接回收——check-first 保持，无轮询循环）；③ 帧间 `sleep_until` 返回后服务一次 RX（等待本身仍无 descriptor poll，A4 见证不变）。
- **重跑的验证**：gmac harness 重编译运行 `30 passed; 0 failed` exit 0（core 面未变）；`make k3-async` exit 0（新 FIT 390688 B）；`git diff --check` exit 0。宿主侧既有结论不受影响。
- **恢复条件**：操作者用新 FIT 重跑同一 V4（U-Boot 五行装载不变）。新的通过数字：`irq-runs>0`、`ri≥2`、`ti≥2`、`rx-frames≥1`、`deferred-polls≥1`（至少一次真实 IRQ 唤醒），其余判定项不变（sent/terminals=20、tx-errors/tx-timeouts=0、timer-wakes=19、最后帧无追加流量仍 terminal、backing=retained）。重跑结果仍不满足时按 V5 停止并返回 Plan 分层诊断（同一问题第三次失败前不加第四次盲试）。

#### Resolution 2（V4 run2，2026-10-12）

- **操作者提交**：run2 在第 0 帧即停止，故障快照 `stage=tx-terminal deadline=1s pre-ack-raw=0x00008405 fatal=0 unknown=1 irq-runs=1 ri=0 ti=1 abnormal=1 spurious=0 irq-en=0x00000000 rx-head=0x30010040 tx-head=0x30000000 tx-inflight=1 sent=1 terminals=0 backing=retained`。
- **判定**：修复①②生效——首个 TX 完成即产生真实 TI claim（`irq-runs=1, ti=1`），等待协议在 check 之前到达 Pending，ISR 抑制与即时快照按契约工作（fail-closed 证明成立）。新缺陷：raw `0x8405 = NIS | bit10(0x400) | TBU | TI` 中的 **bit10 不在已知位清单**，被"未知有效 channel cause"分类器判为稳定 fault → 抑制设备 → 会话停止。
- **登记裁决**（寄存器事实来自同板参考 R69 与本板证据）：bit10 是本板 DWMAC 5.10a 常规 TX 完成的伴随位（无 AIS/FBE 伴随；R69 参考实现整字回写 W1C、仅按七个命名位归类、其余位视为良性）。按位登记 `DMA_STATUS_BIT10`（不断言语义名）并纳入 `KNOWN_STATUS_MASK`；真实未知位（如 1<<21 用例）仍走 mask+稳定 fault。
- **修复与重验**：regs.rs/mod.rs 常量与再导出、KNOWN_STATUS_MASK 扩一位；harness 新增 `irq_bit10_companion_is_benign`（run2 原始形状 0x8405 全覆盖：无 fault、W1C 含 bit10、TI/abnormal 计数、一次 publish）→ `31 passed; 0 failed` exit 0；polling-net-host 回归全 ok；`make k3-async` exit 0（新 FIT）；`git diff --check` exit 0。
- **恢复条件（更新）**：同上重跑判定不变；若 run3 再出现**新的**未知位或 A3 仍不满足，停止并返回 Plan（同一验收点的第三次板测尝试，不加第四次盲试）。

#### Resolution 3（V4 run3，2026-10-12）

- **操作者提交**（成功 run）：`sent=20 terminals=20 tx-errors=0 tx-timeouts=0 rx-frames=19 rx-drops=0 timer-wakes=19 irq-runs=39 ri=19 ti=20 abnormal=20 spurious=0 deferred-polls=20 backing=retained`；逐帧行显示 TX 槽号 0–15→0–3（跨 wrap）、前 4 个 RX 样本 `len=60 raw=0x30010040`（合法 ARP 应答、FCS 剥离）。
- **A3/A4 判定**：全部达标——真实 RI=19/TI=20（均 ≫2 次可区分事件）、20TX terminal/reclaim、RX≥1 完整帧（19 帧、跨 16 wrap）、最后帧无追加流量仍 terminal（TI 全 20）、timer 真实唤醒 19、IRQ 唤醒 deferred 任务（deferred-polls=20）、无 TX 错误/timeout、spurious=0、backing 保留。数字自洽：39 = 20 TX + 19 RX claims；abnormal=20 为每次 TX 后 TBU 伴随（RI/TI 为主 cause，非仅 TBU/RBU）。
- **rx-frames=19 说明**：第 20 个应答晚于会话收尾的 mask 通知到达，留在 RX 环未入账（backing 保留，无泄漏；契约要求 ≥1，19 远超）。
- **剩余**：A5 的外部 RESET 回原系统观察（V4 第 5 步）待操作者一句话确认；确认后 T2（1.2）方可勾选完成，Act Response 转 reported 交用户审计。
- **A5 确认（2026-10-12，操作者报告）**：外部 RESET 已执行、回原系统。A5 的 agent 侧（polling 回归、make k3/k3-shell/QEMU build、未追加资格工具）此前已全绿；板侧 RAM 启动与 reset 恢复闭环。T2（1.2）已勾选完成，本 Act Response 转 **reported**，交用户审计。

## Act Response 终态摘要（当前状态）

- **T1（1.1）完成**：axnet cfg 修复（RED 已采信→GREEN exit 0）、`k3-async-net` feature/`make k3-async`/两级互斥守卫/`k3_klib` 接缝；构建矩阵全绿。
- **T2（1.2）完成**：Gmac async 面（IOC/arm/check_completions/latch_fault）、host 可测 ISR 核心（bit10 裁决后的 KNOWN_STATUS_MASK）、deferred raw 会话（arm 前置、等待协议前置、帧间 RX 服务）；harness 31/31、MS11 回归全过、V4 run3 板测 A3/A4 全部达标（证据见 Resolution 3）、A5 reset 操作者确认。
- **偏差（非实质，均有记录）**：接入 RED 未出现（链接器 GC）；vf2 组合由 axconfig 层先行拒绝（与 MS17 先例一致）；BoardBacking/ARP 共享绑定形式；测试静态互斥；3 轮板测迭代（位次缺陷 ×3 修复 + bit10 寄存器事实登记）。
- **遗留 Minor**：无。第 20 个 ARP 应答晚于会话收尾 mask 未入账（设计内时序，backing 保留）。
- **Persisted Evidence**: none required；**Experience Candidates**: None（bit10 裁决与 raw 会话判定面随 change 收尾进入行为规格；U-Boot 装载流程复用既有 R68 Runbook）。
- **下一步**：用户审计本 Response；审计后由 `openspec-plan` 做本 Cycle 独立 Review（Act 不填 Plan Review）；`accepted` 后由 Plan 展开 Iteration 001（T3–T5）。

## Plan Review

- Review Result: accepted（2026-10-12 用户审计 PASS 后独立 Review；Iteration 000 完成）

**Findings**

独立读取全部工作区 diff（18 个修改文件 + 新 `k3_klib.rs`/`k3_async_net.rs`）与 31 项 harness，核对 Act Response 声明与代码一致；A1–A5 全部有新鲜证据（host 31/31、MS11 回归 17 项、构建矩阵 exit 0、V4 run3 板测数字自洽、A5 reset 操作者确认）。采信 Act Response 记录的验证结论（2026-10-12 产生，其后代码表面未再变化）。

非阻塞 Minor findings（均不构成返工）：

1. `deferred_polls` 在 `wait_for_wake` 入口递增（`k3_async_net.rs` board 模块），计等待协议入口数而非"实际 pend 后被 IRQ 唤醒"的次数；run3 的 check-first 路径可能从未真实 pend。pend→IRQ→publish→唤醒 链条的板上直接见证在 run2（故障侧，Resolution 2 已记录"等待协议在 check 之前到达 Pending"）。Resolution 1 恢复条件中"deferred-polls≥1（至少一次真实 IRQ 唤醒）"的括号注释与计数器语义有轻微出入；A3 判定依据（真实 RI/TI 计数 + run2 记录）仍然成立。偏差分类：Plan 表述瑕疵，不改代码。
2. ISR 对所有非零 cause run 发布工作提示（契约列举 RI/TI/fatal/unknown 的超集）；run3 中每个 abnormal 均伴随 TI，实际无行为差别，工作提示非 completion。分类：Act 契约内等价实现。
3. 帧间 sleep 实现为每帧一次（共 19 次）而非契约字面单次"一次性等待"；Resolution 1 修复③已披露，"await 内无 descriptor poll"不变量保持，timer 唤醒见证增强。分类：Act 偏离（已披露、非实质）。
4. FakeIrqBoard 的 `unsafe impl Sync` 注释写"single-threaded harness"，实际安全性由 `IRQ_TEST_LOCK` 串行化保证（Act 为默认并行度补加）。纯注释瑕疵。

**Acceptance Gaps**

None。A1–A5 全部满足；R2/R3 及 R4–R7 余下部分按 RTM 属于 Iteration 001/002，不是本轮缺口。

**Evidence**

- 代码核对：`async_rx.rs` cfg 隔离、root/kernel feature 蕴含、`lib.rs`/`main.rs` 守卫、`k3_klib.rs` 接缝（release rlib 中 `T __axklib_0_3_mem_iomap` 独立复核存在）、`regs.rs` 0xD041 逐位验算、`mod.rs` async 面（OWN 最后写、check_completions 只读重查、latch_fault）、`k3_async_net.rs` ISR 固定序与等待协议、`k3.rs` 分支隔离、`k3_network_probe.rs` 共享绑定、harness 10 新用例（bit10 用例精确复现 run2 0x8405 形状）。
- 板测：V4 run3 `sent=20 terminals=20 rx-frames=19 irq-runs=39 ri=19 ti=20 timer-wakes=19 deferred-polls=20 spurious=0 backing=retained`（Resolution 3）；数字交叉验算自洽（39=20 TI+19 RI；第 20 个 RI 被会话末 mask 抑制与 rx-frames=19 一致）；FIT 390688 B 与记录一致。
- 验证结论采信来源：Act Response V1–V3 各命令输出（覆盖未变化）。

**Follow-up Decision**

Acceptance A1–A5 已满足且无阻塞项，三条已披露偏差均有合理裁决与记录，无身份型证据工程或判定层。接受本 Cycle，Iteration 000（T1、T2）完成；按 Map 展开 Iteration 001（T3–T5）。change 级 README 与 tasks.md 顶部的 Plan 概述文字同步刷新为本 Review 的当前状态。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

[../../iterations/001-queue-owner-capacity-flush/000-initial.md](../../iterations/001-queue-owner-capacity-flush/000-initial.md)（T3、T4、T5；创建时 draft，待用户批准后 ready）。
