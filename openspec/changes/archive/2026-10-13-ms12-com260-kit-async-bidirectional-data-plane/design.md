## Context

本变更将K3已验收轮询GMAC接入既有异步queue/stack，R1–R7/S1–S20保持。2026-10-08重新调查基线为`k3` @ `cdf94713`，开始时工作树干净；本次只改规划文档。当前自包含实现事实与验证契约在[001-replan](iterations/000-initial/001-replan.md)，原000-initial保留历史现场，不再作为恢复入口。

| 当前接缝 | 已确认行为与计划差异 |
|---|---|
| 平台`irq.rs/time.rs/timer_irq.rs` | 串口交付已实现独立timer槽、CAS/STIE/完整vector分派与SBI set_timer(0)，8项native与真实sleep通过；复用，取消重新实现/新增timer harness工作 |
| `entry.rs::init`、`lib.rs`模式cfg | K3现有polling、raw UART、copier、stdio/shell、rootfs分支；网络新增独立feature，保持仅platform/kernel-task形态，不放开tty/userland；拒绝UART派生/smp/vf2组合，已有qemu/D1 guard复用 |
| `k3.rs::run_first_byte_park` | 网络共用stage1–10健康前置，正常stage10后进入异步分支；旧stage11–13只在polling形态执行，不与新owner共存 |
| AIA、`axtask::future`、共享critical-section | 已有source133路由、原子stopei、IRQ hook、scheduler、timeout与restore-state纪律；不重复初始化，不扩表，不借UART硬件锁 |
| `Gmac`、BoardBacking、K3Env | 18页arena/16RX/16TX、tail/cache/deadline/retain保持；当前IOC/IRQ_EN=0。新增异步通知模式、pre-ack fatal保存、任务重臂重查，并精准复用原GlobalPage/CBO绑定 |
| 普通axnet cfg | `net_link_step_diag`定义仍仅QEMU，调用仍无cfg；原exit101与event窗口3项GREEN继续适用，必要修复留T1 |
| dyn链接 | axdriver注册PCI回调仍引用iomap，runtime只在paging提供klib；网络不启paging，保留限定Unsupported shim，不能因rootfs dyn已链接就假定非paging同样可链接 |
| 构建/板测载荷 | 网络用现有K3 FIT与内核任务直接板测，模式从真实行为输出识别；没有字符串数、mtime、身份字段或专用判定器 |

### Current-State Evidence

- 本次只读`git status --short --branch`：`k3...origin/k3 [ahead 13]`，无文件条目；`git rev-parse --short HEAD`：`cdf94713`，exit0。只定位调查现场。
- `git diff aca5dfe9..HEAD -- crates/axnet/src/async_rx.rs kernel/src/platform/k3_gmac kernel/src/platform/k3_network_probe.rs kernel/src/platform/k3_device_irq_probe.rs` 无输出、exit0。原普通axnetexit101、diagnostics事件窗口3/3、轮询GMAC21项与polling-net-host17项的真实运行结论直接采信；实施改变覆盖表面后补对应GREEN/回归。
- `git diff f93c03f6..HEAD -- crates/axplat-riscv64-k3/src/irq.rs crates/axplat-riscv64-k3/src/time.rs crates/axplat-riscv64-k3/src/timer_irq.rs tests/k3-timer-host-harness.rs` 无输出、exit0；同区间critical-section/axtask future检查无差异。串口000最终accepted结果保留timer8/8、B2真实sleep与timeout；串口003最终accepted保留shell20/20、K3/stdio/shell/QEMU构建与host-test exit0。
- rootfs `archive/2026-10-08-ms18-com260-kit-standard-shell-rootfs/iterations/001-standard-shell/001-replan.md`最终accepted：2026-10-08 shell/stdio/`make ARCH=riscv64 SMP=1 build` exit0；RAM预留窗与DTB搬移为rootfs模式事实，本网络不复用其特有内存布局。
- 原ordinary编译错误与本次实码一致；不是timer问题。范围外VF2旧失败本次未重现，不新增缺陷结论。新网络IRQ/slot/flush/socket依然没有真板运行结论。

### References actually consumed

R66 的资料路线、R71 的分析、R61/R72 的 `docs/network/k3-gmac-dma-irq.md`、`docs/interrupts/k3-interrupt-and-time.md`，以及 R69 实码已使用。实码路径基于 `/home/daivy/projects/serial/work/k3/others/Rt-Async-AMP/tgoskits/`：

- `drivers/ax-driver/src/net/k3_gmac/desc.rs`：RX des3 bit30 IOC、TX des2 bit31 IOC，OWN 最后发布。
- 同目录 `regs.rs` / `core.rs::{enable_irq,handle_irq,submit_rx,reclaim_rx,reclaim_tx}`：channel0 IRQ_EN `0x1134`、STATUS `0x1160`，默认 enable mask `0xD041`；NIS bit15、AIS bit14、FBE bit12、RI bit6、TI bit0；RX/TX cache 同步与 doorbell 用法。
- `queue.rs::K3GmacIrqHandler::handle_irq`：`try_lock` 失败返回空事件，成功时在 ISR reclaim。作为本轮应避免的策略，不复制其框架、锁或无限 drain。
- `platforms/somehal/src/arch/riscv64/imsic_aplic.rs`：原子 `stopei` claim/complete 保留 handler 期间新 MSI。当前 AIA 已实现，保留。
- 本仓 MS10 的 EIE masked-window 实测会丢 MSI，因此 suppress 必须在 GMAC，不在 IMSIC；本仓 MS11 的 TX tail 排他 next-slot 与 RX tail doorbell 是既有真板裁决，保留。资料中的未验证事项仍未验证；M42 默认信任可追溯用法，不追加寄存器 readback 资格工程。

## Goals / Non-Goals

设计把 K3 设备通知与 DMA 接到已有网络所有权、backpressure 和 socket 机制。职责按 M36/M41/M42；功能范围与非目标以 proposal 为准。共享库只增加接入所必需的 feature 隔离、测试和最小诊断入口，不改变其他平台 API、固定快照 ABI 或运行时恢复语义。

## Decisions

### D1：复用已交付 runtime，保持分层入口

根与kernel新增 `k3-async-net`，蕴含k3；根选现有平台runtime-irq，kernel开axdriver/net/dyn与axnet。不选uart-irq/k3-async-uart，不开axruntime/net-ng自动probe、paging、tty/userland或rootfs。`make k3-async`复用k3参数与k3-fit，SMP1、LOGoff、AX_IP=.2/AX_GW=.1；Make拒绝SMP>1，kernel拒绝smp/vf2与任何UART派生组合，平台已有qemu/D1guard保持。

timer独立槽、首次set_timer(0)、24MHz换算、scheduler/tick与critical-section全部复用，不再修改或新增第二实现。网络真实sleep在raw板测中交叉观察；tick不得发布completion work。已有shell使用同一底座，但先保留独立网络内核载荷，把设备bring-up与用户交互故障分开定位。

dyn注册PCI回调仍引用`__axklib_0_3_mem_iomap`，runtime只在paging提供Klib；本网络保持静态boot映射，新增仅网络feature编译的最小Unsupported入口（memory_addr0.4/axerrno0.2原有类型，unsafe extern Rust），实际GMAC不调用它。原生链接即判定；不造ABI检查工具。不因shell/rootfs已经启paging就把它扩成网络前置。

网卡单hartqueue/stack与恢复稳定后，后续组合阶段统一启动UARTcopier、NICqueue、stack runner和shell，先检查共同运行再推进多hart。按设备保持唯一owner、分开的等待/完成语义；不把两个完整boot函数串接、不再次初始化runtime。此为后续边界说明，不增加本变更的shell验收或提前授权集成实现。

替代：直接以shell/rootfs会话作为GMAC首轮门槛，会把tty/loader/应用进程加入本次诊断范围；重新做runtime会重复已验收底座。采用当前代码基线与独立板测入口即可覆盖本轮目标。

### D2：ISR 与 descriptor owner 分开

GMAC 低层增加显式异步 notification 模式；默认 polling 模式保持原样。配置 IOC 的同时仍按原 cache/fence 和 OWN-last 顺序发布；不对已提交 OWN descriptor 就地补 IOC。

ISR 使用固定 MMIO、静态原子 cause/fault 状态和既有唤醒入口，不取得 Service/Gmac/arena 锁，不碰 descriptor、不分配、不输出串口、不跑协议。先读并保存原 cause，设备侧 suppress 本 channel 通知，W1C 只写已观察且认识的 cause/summary 位，分类后发布事件。fatal 原始 status 先 latch 后 ack，直到外部 reset 保留；未知/零 cause 计数，未知有效 channel cause 使设备通知关闭并进入稳定错误。RI/TI及稳定fatal/有效unknown都发布一次工作提示，使错误等待者也能恢复；zero不发布工作，不伪造完成。

任务 suppress/arm 的 IRQ_EN RMW 在现有 single-hart IRQ-save 临界区内完成；请求 BOTH 必须一次整体提交。正常 enable 使用 R69 的 `0xD041`，directions mask 保留 summary/fatal 所需位。APLIC/IMSIC source133/EIE 保持开启，不以 EIE mask 压制 completion。arm 后 fence，invalidate/fence/read 当前 RX head OWN 与有 live owner 的 TX head OWN，再查 latch/raw fatal；completion_pending 不推进 head、不回收。仅有 cause hint 不能报完成；空 RX ring 或零 TX inflight 必须为 NONE。现有 QueueEvent generation 负责 register 前后窗口，descriptor 重查负责 ack/masked 窗口；各自职责无需新增事件协议。

理由：避免最后事件丢失和锁竞争。替代：ISR reclaim、参考 try_lock 返回空事件、周期轮询补救，均不符合 R1。

### D3：薄适配器实现现有 traits

新增 `kernel/src/platform/k3_gmac/adapter.rs`，持有唯一 Gmac、固定 software pool、16 项 TX cookie/buffer owner 表与 scratch；板级环境继续复用 native MMIO/CBO、GlobalPage backing。实现 `BaseDriverOps`（`irq_num=Some(133)`）、`NetDriverOps`、`NetQueueControl`、`NetTxQueue`，返回 `recovery_control=None`、epoch MIN。`Box<dyn NetDriverOps>` 经 `AxDeviceContainer::from_one` 交 axnet。

TX pool 固定 16 × 2048 B，RX 临时 pool 固定 1 × 2048 B；复用 `NetBufPool`。这是 CPU copy buffer，不作为 DMA backing。TX `alloc_tx_buffer` 检查 14..=1514，零 padding 仍由 Gmac 执行；pre-accept Full/Invalid/error 还原并 drop 原生 NetBuf，slot/ticket 留在上游。OWN 发布成功后将 buffer+cookie 绑定实际 descriptor slot，直到合法 terminal 才归还 pool 并返回 cookie；调用错误若发生在 OWN 发布后按 post-accept fault 保留，不因返回 Err 误释放。使用 Gmac 的实际发布状态决定该分界。

`reclaim_tx` 每次最多一个完成；Completed 归还 cookie 一次；LocalError 可安全释放已 terminal 的 CPU buffer，但返回 Io 并 fault 所属 ticket/flush，不能冒充成功 C4；TerminalTimeout 保留 descriptor 与 owner，返回 TimedOut。duplicate/缺 cookie/错 epoch 为 BadState。资源 ledger 从 pool 实际 owner 与 Gmac descriptor 状态取得，不从软件 slots 推导；`completions_seen` 计首次同步看到合法 OWN=0，与 reclaim 计数分开。

`receive` 每次最多检查 16 个 descriptor，合法帧 copy→原 Gmac refill→NetBufPtr，axnet copy 到 RX slot 后原生 recycle；分类 drop 后可继续有界检查，耗尽时靠 descriptor pending 重查继续下一轮。无工作 Again，不把坏帧当 fatal；实际 raw drop、RBU/overflow 观察分别记录，不推算丢失包数量。scratch 必须足够容纳 1514，禁止使用 `raw_poll_rx_desc` 的小 scratch 截断行为作为成功结果。

错误映射：pre-accept Full→Again；长度/地址拒绝→InvalidParam；已提交 deadline→TimedOut；FBE/local TX error→Io；重复所有权/不支持 activation 状态→BadState。保留 phase/raw cause/owner 的产品侧观测；进入 fault 后拒绝新 submit，通知关闭、合法软件资源可释放，全部可能 device-owned backing 保留。

理由：既有 traits 和 pool 覆盖需求。替代：引入 rd-net/rdrive/新 DMA 库、抽离通用驱动框架、重写 slots/flush，均扩大范围。

### D4：沿用唯一 queue 和 stack runner

板级 stage1–10 成功后，先mask60/133/277；stage10的 `mac133_handler` 仍已注册，须用现有unregister撤下133，60/277保持masked，不再在网络期间读PHY/MDIO。准备 Gmac/adapter（通知关闭）→注册133专用 handler→`axnet::init_network`→以实际 `axtask::schedulable_cpu_mask()` 启动 queue owner→确认 Active，再启动 stack runner 和 socket 载荷。只在初始化阶段，前台以原生yield_now让出CPU并检查既有rx_snapshot生命周期，5s绝对上限；Active继续，Faulted/Unavailable或超时停，不调用poll_interfaces或额外nudge。数据面激活后不保留该初始化等待循环。实际运行 hart 由 native tuple 读取，不把 affinity 配置当运行证据。任何准备失败均不得启动第二 owner；Active 后 fault 不回到 polling。旧 stage11–13 不与新 owner 同时运行，也不继承其 Gmac 对象。

queue owner 保持 reclaim/RX/submit 各32、64 RX/64 TX slots 与现有 ticket容量。K3 物理 ring16，budget31/32/33 的阶段精确见证用 host fixture 在 refill 后继续产生 completion；真板只要求真实 exhaustion 发生时有界、双向推进，不能凭流量数量伪称碰到 budget。

flush 使用既有 `axnet::flush()`；载荷以原生 timer future 对 flush 作1s外部等待 deadline，drop future 只撤 waiter。submit/completion/reclaim 原有绝对1s阶段 deadline 不续期；Gmac 原 reset1s、link5s、CPU refill/reclaim10ms保留。原 owner fault 唤醒 stack/socket/flush；DMA 所有权不因 waiter cancel/timeout 改变。不得增加恢复 control、link-flap 自动 reset。

理由：机制已有且跨平台验收过。替代：K3 独立 polling smoltcp session不能满足 R4。

### D5：复用压力 lease，直接看生产状态

增加 axnet `queue-diagnostics` feature，仅承载现有 submit/reclaim hold、lease timer、相应快照；`qemu-diagnostics` 蕴含它，原 QEMU API/数值/ABI 保持。移动 hold 相关 cfg 到该中性 feature；显式 recovery/reset request、QEMU link日志、migration 与 userspace ioctl仍仅 QEMU。K3 专用 `k3-net-diagnostics` / `make k3-async-probe` 启用共享压力能力，普通 K3异步构建不含控制入口。

唯一必要新增入口是在诊断 feature 下向目标 EthernetDevice 的**现有完整帧 slot enqueue**提交一帧、读取既有 ledger，并在解锁后 software_nudge。最多一帧/调用；不直接注入 ticket、修改 descriptor、伪造 completion。它供内核载荷依次测试 64/65软件slots与16/17硬件资源，普通 socket路径不调用。复用 `EthernetDevice::send` 使用的 enqueue/checked ticket路径；具体辅助函数拆分留 Act，输入、输出和 owner分界不得改变。

hold 使用200ms lease，显式 release，不到1s data deadline；auto-release 计数须为0。用于真实 TX Full/flush，不暂停 clock 或破坏 OWN。RX slot Full及坏帧、严格 budget、fatal、安全保留用生产 adapter+既有 local Service fixture 确定性测试；真板同时观察 RX burst/drop与恢复，不要求物理注错或制造必现RX软件Full。诊断载荷不是结果判定器，不新增协议/CLI/日志审计或工具专用测试。

理由：R6 需要实际容量边界，既有 lease 能安全暂停 sole owner 的一个阶段。替代：流量碰运气、复制压力状态机、用独立 audit 工具，均无必要。

### D6：逐层验收并停止于操作者边界

分三轮：runtime+真实DMA IRQ；traits+既有queue/flush容量；stack/socket+四协议及quiet。首轮用20个原 MS11 ARP request 经 deferred task发送，真实PC回应供RX；前19帧完成并重臂后，原生timer future等待100ms再创建最后1帧，保证存在独立后续设备事件。这个一次性应用等待只释放新工作，不轮询completion；每个submit最多1s，raw会话30s结束，至少20TX合法完成且真实RI/TI分别重复出现；最后TX无后续包也回收。初始化允许既有有界轮询；启用后只因事件、可达backlog或期限推进。

后两轮精确动作随前轮 accepted 基线展开，设计约束已确定：200ms hold，64/65slots、16/17driver resources；最终60s socket窗口，PC先一包暖邻居（不计验收），随后 ping10/10、UDP一次 `ms12-udp` 全字节echo、TCP一次 `ms12-tcp` 全字节echo及正常EOF。payload TCP主动正常关闭；不把单次recv保证为完整TCP数据，PC读至8字节再检查EOF。quiet窗口2s：无新增cause且无到期协议timer时queue/stack polls与selfwake不持续增加，再一包恢复。若PC背景流量或协议timer到期，窗口不满足前提，报告该观察，不能标PASS。

全程使用R68 RAM FIT路线，1152008N1无流控、PC .1/24、板 .2/24、MAC02:00:00:00:26:01、UDP26001/TCP26002。Wireshark按用户原话永久豁免；没有新的计数/reset豁免。Evidence默认none，原生命令与每项≤20行决定性结果写Act Response。正常和失败路径均在任务上下文报告有界pre-ack cause/claim/owner/deadline/同步后OWN状态，累计由任务持有以避免超时取消丢失诊断；不以第二次破坏性读补快照。板测提交用户前先完成该轮agent可执行测试和build；未执行板测不算通过。

## Verification mapping

| 场景 | 最简目标验证 | 轮次 |
|---|---|---|
| S1 | 真板RI/TI原cause+133claim+deferred wake+20合法TX/RX；host快照/IOC/W1C | 000 |
| S2–S4 | 生产notify/descriptor路径fake MMIO：注册前中后、mask、最后completion、组合/零/未知/FBE；既有event窗口测试 | 000、001 |
| S5 | ready前置、失败与重复start，现有local lifecycle/affinity fixture，无第二owner | 001 |
| S6 | production adapter+local Service fixture31/32/33、连续双向、每阶段计数与yield | 001 |
| S7 | host RX64/65、TX64/65与driver16/17；真板TX两级Full→release→reclaim→账本恢复 | 001 |
| S8–S9 | 实际arena descriptor/fakecache/NetBufPool，长度/错误/wrap/refill/单次cookie回收；板侧跨16 wrap | 001 |
| S10–S12 | 原flush future目标live集合、乱序/后续ticket、空/Busy/drop/1s deadline/fatal；真板hold下flush Pending后release完成 | 001 |
| S13–S15 | local stack/socket fixture三源wake与quiet；真板四协议、EOF及2squiet→新IRQ | 002 |
| S16–S18 | 原Gmacfake init/reset/deadline + adapterpostpublish故障与Backing drop witness；真板报告阶段并外部reset回原系统 | 000–002 |
| S19 | 产品native snapshot和Cargo原生结果；无新增判定层 | 全部 |
| S20 | 独立features测试、K3polling受影响host回归，make k3/build/lichee；VF2既有失败按影响范围单列，host-test采信较新的accepted通过结论 | 全部 |

## Risks / Trade-offs

- 真实 DMA IRQ 尚未被本仓 MS11 证明 → 放在首轮硬件Acceptance；注册成功、软件MSI、PHY IRQ或OWN变化都不替代RI/TI实测。
- GMAC W1C与ISR交错可能掩盖fatal → 先latch原cause，所有queue入口先检查稳定fault；host确定性注错。
- tail满环、mask窗口及最后completion可能揭示参考用法与板行为差异 → 按当前16-slot契约验证，冲突返回Plan，不擅减容量或加轮询fallback。
- 共享hold feature影响axnet → 限hold cfg及同一native接口，ordinary/queue-diagnostics/qemu-diagnostics三面测试；原QEMU恢复边界不扩写。
- RAW层成功尚不等于socket成功 → 每轮独立验收，MS12只有最终四协议与quiet通过才完成。
- CPU池copy比zero-copy多一次拷贝 → 已批准无性能门槛；固定容量简化所有权且复用MS11安全DMA。
- 全部已发布DMA backing直到外部reset保留 → 这是安全边界，不计运行时泄漏修复或DMA stop资格。

## Migration Plan

新增K3异步feature与make入口；原K3polling及其他平台选择保持。每轮按host→build→RAM真板推进，仅accepted后展开下一轮。各K3构建共用FIT文件名，先完成旧入口/其他平台build回归，再最后构建该轮待测异步入口，避免随后make k3覆盖镜像；不增加身份校验。最终正常异步路径使用既有queue/stack，首轮raw工作负载仅保留在诊断路径以检查底层，不与网络owner共存。失败停止新增工作、保留owner，RESET回原系统；不saveenv、不刷写、不修改参考仓库。

技术规划已闭合，无新增需求简化或实质技术未知项。Gate1沿用已批准范围；2026-10-08用户原话“批准”，Gate2 PASS，最新Cycle ready；设计、任务契约与验收不变，尚未进入实施。

## Reuse and boundaries（2026-10-08）

| 已有交付或经验 | 本计划用法 | 不适用边界 |
|---|---|---|
| timer/AIA、IRQ hook、task future、critical-section | 复用实码与accepted运行结果，仅补网络feature接入 | 串口IRQ/sleep不证明GMAC RI/TI或descriptor完成 |
| copier的check-register-arm-recheck | 网络沿用现有QueueEvent+descriptor重查，并补目标生产fixture | 不复制UART字节ring或TEMT drain；NIC使用packet/cookie/C4 |
| shell多模式boot cfg、内核诊断与静态载荷组织 | 复用单入口/互斥方式，网络直接内核任务诊断 | 不新增shell命令、userload、网络syscall或rootfs依赖 |
| 串口失败后acc不可见的排障经验 | 计数与owner由raw任务持有，timeout后仍有有限快照 | 不复制UARTFCR/Errata/IIR/SETIP假设，不新增软件IRQ资格流程 |
| shell结束drain与owner保持 | 保留网络的cancel仅结束等待、device-owned backing保留 | 串口drain不替代TX C4，reset回原系统不证明DMA停止 |
| shell/rootfs构建与镜像选择经验 | 网络目标build后立即按原ITS打包；待测FIT最后生成 | 不增加strings/mtime/hash/run-id/manifest/validator验收 |
| rootfs OOM/预留布局经验 | 识别rootfs特有内存约束，网络保持原非rootfs布局 | 不把Issue旧状态作为当前代码约束，不维护Issue或引入存储 |

本次仅更新本变更的计划产物；旧Cycle Plan Context/Act Response、已归档串口/rootfs产物和全局状态不改写。三轮依赖与诊断边界保持，最新执行入口为001-replan。
