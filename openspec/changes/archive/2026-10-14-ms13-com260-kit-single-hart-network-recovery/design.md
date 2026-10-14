# CoM260 Kit 单 hart 网络恢复设计

## Context

目标与已批准场景见 [proposal.md](proposal.md)。本次只规划；2026-10-13 用户批准需求并要求复用已有工作及参考仓库经验，未批准实施。

当前基线为 `k3` / `d07285c0`。`git diff --name-only 5f977f1e -- kernel/src/platform crates/axnet crates/axdriver_net tests/k3-async-net-host tests/k3-gmac-host-harness.rs crates/axplat-riscv64-k3 Makefile Cargo.toml kernel/Cargo.toml` 实际无输出：最近网络验收后相关生产与测试面没有变化。已有 AGENTS 修改与旧模板删除均保持。

### 可复用结论

| 输入 | 本次复用 | 本次不据此推定 |
|---|---|---|
| 2026-10-13 网络最终 accepted，归档 `2026-10-13-ms12-com260-kit-async-bidirectional-data-plane/iterations/002-stack-socket-protocol/000-initial.md` 的 Act/Review | K3 真实 RI/TI、唯一 owner、64 软件 slot/16 descriptor、C4、stack/socket、四协议、沉降后 quiet；host 15 项与 axnet 三面 482/506/514 GREEN | 运行时 reset、DMA 停止、link 恢复、旧包隔离未验收；孤立 resume 和外部 RESET 的历史豁免不扩大到本次恢复行为 |
| 已有 QEMU 单 hart恢复代码与 accepted 归档 | resident owner、1 s quiesce、2 s reset/reinit、10 ms 有界恢复进度 timer、checked QueueEpoch、SocketEpoch/LinkGeneration、取消和状态先提交后 wake | 不搬用 VirtIO status=0、queue_unset 或 DMA coherent 假设，不重建第二套状态机 |
| 已有真板 IRQ/轮询/串口基线 | AIA `stopei` 先 claim+EOI、source60/133、RTL8211F INSR COR、BMSR latch-low、MFPR35 CLEAR hold、真实 timer、18 页 DMA/cache/tail 契约 | 旧观察探针在 IRQ 内串行 MDIO 的形态不适合常驻网络 owner；普通 UART 日志不计 UART 专项结论 |
| 宿主 timer 经验（上述最终 Act 的 Deviations 1） | axhal 宿主 `wall_time()=0`；host 用已有 fixture clock/手动步进测状态，真实 deadline 放到板上 | 不再写宿主 timer wheel 替身或把未触发的 timeout 算 PASS |

### Current-State Evidence

以下均为本轮直接读取实际源码，非旧 Analysis 推断。

| 位置与符号 | 当前行为、调用边和缺口 |
|---|---|
| `kernel/src/platform/k3.rs::run_first_byte_park` → `k3_async_net::board::run_k3_async_net_service` | stages1–10 后进入网络独立入口；屏蔽60/133/277，撤销133旧观察 handler，创建 Gmac/adapter，安装唯一 NIC，依次启动 queue 与 stack；source60 保持屏蔽 |
| `k3_gmac/mod.rs::Gmac::init_rx/decode_link/clear_rings/refill_slot/enable_tx` | 启动时 SWR≤1 s、同步 PHY 协商≤5 s，再清环、写高/低基址、16-slot refill。运行时不得调用该同步初始化作为恢复 step；它有 spin 与重置状态的副作用 |
| `Gmac::bounded_stop/stop_tx/observe_async_fault` | 清 RE/SR、TE/ST、mask IRQ；注释明确只是止损，不清 device-owned OWN，不提供 DMA 停止证明 |
| `Gmac::poll_tx/try_submit` | 16 槽，1 s terminal deadline；TimedOut 保留槽和 backing；合法 OWN=0 才回收。原始硬件 descriptor 没有软件 epoch 字段 |
| `ArenaGuard::publish/Drop` | 18 页 backing 发布后一直保留。可在可信停止后原地重建并复用，不需要本次新增解分配协议或扩大 DMA 抽象 |
| `k3_gmac/adapter.rs::K3NetAdapter::{new,submit_tx,reclaim_tx,tx_resource_ledger}` | mutex 内独占 Gmac/env、16 CPU TX buffers、1 RX scratch、16 owner cookie 表；无 recovery_control override。`TxCookie` 原样携带上层 epoch，未验证其属于当前 K3 queue 实例 |
| `crates/axdriver_net/src/lib.rs::NetRecoveryControl/RecoveryProgress/OwnerSummary/QueueEpoch/TxCookie` | 已有有界 begin/step、link snapshot 和 checked epoch；无需添加新 trait 或第二 epoch 类型 |
| `crates/axnet/src/async_rx.rs::RxQueueTask::{poll_active,poll_recovery,recovery_step}` → `Service::recovery_*_target` → Router → adapter | 同一常驻 owner 先关闭 socket、quiesce、driver begin/step，再结清旧 ticket/flush、推进 epoch；恢复 Pending 由既有 10 ms one-shot timer 推进。BadState 直接 quarantine，不 reset 掩盖 ownership drift |
| `async_rx.rs::RecoveryRequestState/recovery_reset_request_shared/claim_recovery_reset_request` 与 `axnet::recovery_reset_request` | 请求合并、Busy 与自然恢复吸收机制齐全，但相关结构、static、入口、claim 和 transition cfg 都限定 qemu-diagnostics；K3 probe 只有 queue-diagnostics |
| `async_rx.rs::poll_active` → `Service::link_policy_step_target` → `NetRecoveryControl::read_link_status` | 初始 snapshot 已有；Down 关闭旧 socket并取消 TX queued/ARP pending，Up 新开 socket epoch而不推进 queue epoch。Again 当前立即 republish/self-wake，不适合跨 tick 的 MDIO；read 错误产生的 LinkStep::Fault 当前未转成稳定 owner fault |
| `device/ethernet.rs::rx_slots/recv_dormant/tx_cancel_*`，`router.rs::rx_buffer/tx_buffer` | 旧 TX slots/ARP pending 有撤销；旧 RX slots 和 Router 已复制 IP 缓冲没有生命周期关闭入口。恢复期间它们可在新 socket 建立后才被消费，需与旧 socket epoch 一起作显式 discard |
| `k3_device_irq_probe.rs::source60_handler/service_one_device_event` 与 `phy_probe.rs` | 观察 handler 内读 INSR/BMSR，再 pulse MFPR CLEAR；其32槽只写一次记录不适合常驻运行。复用底层位域与 ack 用法，不复用 handler/固定窗口/有限快照存储 |
| `tests/k3-async-net-host/{src/fake.rs,tests/*}` 与 `tests/k3-gmac-host-harness.rs` | 直接 include 生产 Gmac/adapter；FakeEnv 当前 SWR立即清、缺少 stuck/延迟/真实清状态模型。FakeWire 直接写同一 arena；TrackingBacking 可观察 Drop。已有 Service/socket fixture 用独立测试二进制隔离全局 singleton |

现有 `adapter_contract.rs::cookies_keep_owner_epoch_across_the_device` 人工构造 MIN+1 cookie，但adapter实际上尚在MIN。接通恢复后应先实际恢复到MIN+1，再保留原“cookie原样返回”断言；另测旧/未来epoch拒绝。该夹具调整是新所有权契约的必要后果，不能删除断言或将任意epoch继续当合法。MAX epoch等private状态边界放在adapter本模块的cfg(test)单测直接设置，不为integration test增设生产seed API或专用feature。

### 参考资料与采用方式

参考根目录为 `/home/daivy/projects/serial/work/k3/`，全部只读。按照 M42 默认采信已登记资料的寄存器与用法，不对既有板级基线增设复验前置。

- `docs/network/k3-gmac-dma-irq.md` §8–10、`docs/dma/{k3-dma-and-memory-ownership,k3-cache-pma-address-translation}.md`：把 FBE、reset、link、ownership 分开；其采集时 G5/G6/G7 不覆盖后来本仓已验收 DMA/cache/DTB 事实，不将历史 unknown 重开成前置阻塞。
- `others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/{core,regs}.rs`：SWR `+0x1000 bit0` 重启内部 fetch；stop 清 ST/SR 和 TE/RE；ring base 高低、DSL、MTL 用法可复用。拒绝 `init_hardware` 捕获 reset timeout 后 warning 继续，以及 IRQ try_lock 失败返回空事件的策略。第三方框架不复制入本仓。
- `.claude/analysis/rt-async-amp-k3-drivers.md` 的 AP GMAC/AIA 与复用边界、`openspec/specs/k3-com260-gmac-network-baseline/spec.md`：复用具体经验和来源；该仓主要交付资料规格，不宣称它已验收本仓 runtime recovery。
- 本仓 `.agents/runbooks/com260-uboot-usb-starryos-ram-boot.md`：只复用 RAM/FIT 装载与外部回滚命令，不照搬轮询 stage11–13 的网络验收窗口。
- 本仓 QEMU 恢复 Runbook：复用恢复期间缺少进度 wake 导致睡到 deadline 的历史教训，以及 DMA 重建前零化要求。其旧 validator/Evidence 目录步骤不进入本计划。
- 外部主源交叉核对：[Linux dwmac4_lib.c](https://raw.githubusercontent.com/torvalds/linux/master/drivers/net/ethernet/stmicro/stmmac/dwmac4_lib.c) 的 `dwmac4_dma_reset` 将 SWR 自清作为 reset 完成；[U-Boot EQOS](https://raw.githubusercontent.com/u-boot/u-boot/master/drivers/net/dwc_eth_qos.c) 的 start/stop 分开 DMA/MAC 顺序。它们辅助解释同 IP 用法，不能独立证明 K3 AXI 已停止。SpacemiT 对应分支 raw/GitHub URL 本轮返回 Cache miss，未将未取得的内容写为事实。

## Goals / Non-Goals

实现边界为 K3 adapter + 最小共享生命周期接缝；沿用 executor、NetRecoveryControl 和既有 socket terminal，不新增驱动框架。范围外内容见提案。普通 K3、UART/shell/rootfs、D1、VF2 与 QEMU 入口继续独立。

## Decisions

### D1：同一常驻 owner，复用现有恢复与错误模型

生产恢复只能由 RxQueueTask 通过 Service 驱动。显式请求只提交事件，入口从 qemu-diagnostics 移到已有 queue-diagnostics；QEMU trace、migration 和 ioctl 的平台门控不跟着扩开。request/claim/static/自然恢复吸收/terminal 清理的全部 cfg 必须成组调整，不能只开放 public 函数。

错误规则：link down 为 `NotConnected`；正常 reset 关闭旧 socket为 `ConnectionReset`；MDIO/stop/reset/reinit 超时与设备不可恢复错误为 `Io`，具体阶段留在原生 driver/recovery snapshot；duplicate/unknown owner 或 epoch exhausted 为 `BadState`，稳定 quarantine。`Again` 仅表示可重试 snapshot 或容量，不能表示 terminal。FBE/unknown/ownership drift 不要求强行恢复；安全失败是本范围的合法错误分支。

理由：共享 owner 已有取消、flush、状态/wake 次序与 bounded timer；重写会复制已验收高风险状态机。替代方案：调用者直接 reset、第二 recovery task 或引入通用驱动框架，均破坏状态所有权或扩范围。

### D2：SWR 完成作为硬件停止边界，原 arena 固定保留并复用

恢复底层分为单步 stop-register 操作、请求 SWR、等待自清、Stopped、分步 rebuild、可服务。进入 stop 后不再 submit/reap/refill；保留旧 owner表与 DMA 内存。依次 device mask、清 ST/SR 与 TE/RE，写 `DMA_MODE.SWR`；每步至多一个 MMIO读/写或一个固定 slot 重建单位，锁不跨等待。SWR deadline 1 s，不自旋、不按 poll 次数冒充墙钟。

SWR 未确认时，禁止 clear ring、改 owner表、释放 CPU carrier或写新基址。确认后才能封闭旧实例并分步原地重建；始终使用同一18页 arena，ArenaGuard Drop 保留策略不变。失败时如果没有停止依据，device-owned 数仍按可能可达的旧资源报告，不能因为软件标 Faulted 就报告零。

SWR 在 R69 中明确承担内部 fetch reset，因此选择它而非发明未取得依据的 AXI idle/总线监控位。这个软件停止边界必须经首轮真板验收：在已有真实流量和发布 RX owner 的条件下执行 stop/reset；保持旧 arena 不改写，PC 持续发包时检查同步后的 descriptor 和数据区没有新 DMA 写回，再原地重建并取得新的 RX/TX/IRQ 与容量恢复结果。SWR=0、安静 IRQ或空闲 descriptor 单项不计通过。原生探针只报告设备状态、内存写回变化与收发结果，不添加 host 判定器、Hash、身份字段或采集协议。

该见证的边界是目标控制器 reset 语义与本板覆盖到的停止/复用路径，不宣称任意故障下的无限期总线静止。发现 SWR 已清但仍有写回、无法保持旧 arena观测或任何旧/新资源混用时，首轮不接受、后续不展开，返回 Plan；不得通过延长采样窗、补证据链或新分配掩盖。

替代方案：每轮分配新 arena会掩盖泄漏；永久放弃旧 arena再继续接单会逐次耗尽内存；仅清 OWN/读 ST/SR 或 gate APMU clock 不闭合所有权；加入未证实的 AXI idle 位改变安全依据。均不采用。

### D3：恢复重建不复用同步 init，完成边界有明确 owner 账本

保留 `init_rx` 的已验收启动形态；提取可复用的寄存器配置和单槽 clear/refill 单位，运行时使用有界 step，不再次执行5秒 decode_link、自旋 SWR或重配整个板级 clock/reset。

reset 控制器配置、MTL、ring、MAC、RX refill及 TX enable 顺序沿用本仓当前生产用法，IOC/high-address/cache/tail 契约不变。配置先使用最后一次已知 LinkConfig；恢复后重新读取 PHY，确认速率/双工并更新 MAC 对应位后才放开 link gate。link down 不使 reset 卡在5秒协商等待；queue 可以重建完成但 enqueue 继续被 link gate阻止。

`Recovered` 只在全部16 RX refill和通知准备成立后返回目标 epoch。上层随后结清旧 DeviceOwned为 ResetAborted、旧 flush错误并推进软件 epoch；不能把 reset 回收当成正常 C4。IRQ notify 的最终 arm仍由 owner register/arm/recheck执行。ISR 旧工作 hint可触发当前重查，不能证明旧 completion；FBE/unknown latch 只在真正完成reset隔离后按单 hart短临界区清理，累计诊断计数不重置成新运行身份。

OwnerSummary 以16个 TX carrier +16个 RX carrier 为固定32单位，不与64软件slots混加。健康 idle为 available16/device_owned16/quarantined0；TX在途 N 时为16−N/16+N/0；SWR未确认保持真实可达分类；确认停止、重建中的未发布 carrier为 quarantined，新发布RX转 device_owned。TX CPU copy buffer与对应DMA槽是一个逻辑carrier，详细TX ledger仍分别报告pool和descriptor容量。

raw descriptor不含epoch；软件 epoch不能辨别“硬件在新基址上写了旧结果”。这种风险必须由停止边界解决，不能伪造 descriptor generation字段。Adapter 只接受当前epoch cookie；保留/关闭旧表后不再输出旧cookie，旧 IRQ只重查当前OWN。host stale场景分别覆盖旧cookie归属和旧hint，禁止把模拟器任意写坏新descriptor说成已验证物理stop。

### D4：PHY IRQ只留提示，MDIO由同一 owner推进

退役source60旧观察handler，注册网络自己的最小handler。ISR读取并保留GPIO/MFPR原始状态，置 MFPR35 CLEAR hold止住该GPIO源，发布 config事件；不读MDIO、不操作descriptor。source60 APLIC/IMSIC保持既有路由，不用EIE/CLRIE屏蔽窗口暂存MSI。

Adapter的 `read_link_status` 驱动有界MDIO状态机：沿用Clause22命令字段、page save/restore、a43:1d INSR一次COR快照、BMSR两读与ANA/LPA/CTRL1000/STAT1000能力解码。每次只推进固定MMIO单位，单transaction沿用10 ms bound，整个事件最多500 ms；超时稳定Io并保留失败阶段。其余运行期路径禁止独立访问MDIO。

读完PHY再释放MFPR CLEAR、重查edge与PHY状态；若期间又变化，保留一次后续config处理，不靠下一物理事件。INSR=0回声不伪造 link转换。BMSR latch-low表明down曾发生时，先提交down再允许后续up，避免短down/up使旧socket透明续存。初始snapshot也走同一生产路径。

共享owner将 link Again保留为pending，用1 ms one-shot timer安排下一step；只有真实新事件/工作才可额外唤醒，不每poll自唤醒。timer与设备completion机制分开，ready后取消。LinkStep::Fault在同轮进入稳定fault、先关门/提交错误后wake，不能继续向失真link提交。

理由：现有source60观察模式是有限独占窗口；runtime MDIO与reset会共享MAC寄存器，必须由同一owner串行化。替代方案是旧IRQ内MDIO、独立PHY访问任务或周期读BMSR兜底，均引入阻塞、共享访问或quiet退化。

### D5：关闭旧生命周期时处理所有已复制旧数据

在Service guard下随reset/link-down关闭旧socket epoch，撤销TX slots与ARP pending，并discard旧RX slots及Router rx/tx PacketBuffer内的旧IP数据，记录discard数量/原因。Router共享缓冲属于本次关闭的全局socket epoch，不能保留后让新socket收到旧数据；不增设多NIC归属协议。

关闭期间RX硬件completion仍可由原owner有限回收/refill，但复制数据不交付给新生命周期。恢复提交时再清一次可能在关闭窗口内进入的旧软件数据，再开启新socketepoch。无包时是no-op；loopback既有旧socket同受当前registry terminal，旧数据不自动转发到新socket。

状态与terminal先提交，Service/adapter锁释放后再wake。flush只按原目标集合结算；waiter drop不终止resident recovery。恢复后强制一次新link snapshot，link gate只在一致up和queue ready共同成立时清除，防止reset时用旧up缓存提前接单。

### D6：验证复用、边界与预算

先行测试全部进入既有rustc harness或Cargo测试包，直接调用生产Gmac/adapter/Service/SocketOps；新函数缺失或缺行为的RED由Act建立并观察。本Plan不提前改测试或实现。

host不跑真实timer等待：底层step用FakeEnv时钟，shared owner用既有RecoveryTestClock；恢复Service端到端与timer链由真板和单hartQEMU真实入口证明。不增加宿主时钟后端、验证CLI、validator或身份协议。

末尾真板失败用例只暂缓adapter reset确认step中的设备读取，并保留正常绝对deadline检查，触达真实timer→未确认错误→Faulted/资源保留。测试接缝只在现有网络诊断模式供固定用例使用，不扩生产CLI或伪造MMIO原值；不将“主动不确认”报告成硬件自然reset失败。这个用例观察的是缺少停止依据时的实际安全分支；自然stuck/reinit失败的细分由生产host fixture覆盖。

昂贵验证每轮一次，详见tasks。首轮只验收DMA停止/复用底层；第二轮验收resident owner与PHY，并完成受影响QEMU恢复回归；第三轮验收socket组合和最终真板故障矩阵。每轮到能力边界前只列agent可执行工作，板测结果未审核PASS不展开下一轮。

Persisted Evidence均为none：普通命令、决定性输出和操作者一句话结果足以支持Review；安全RAM启动可低成本重跑，无一次性环境或不可替代结构。Plan不创建evidence目录。

## Risks / Trade-offs

- SWR/PHY在拔线条件下可能因参考时钟不满足而超时 → 不扩大deadline掩盖，保持backing并报准确stage；本轮必须同时验收成功路径和该失败安全边界。
- IRQ源60 hold期间可能合并边沿，ISR旧echo仍可能到达 → INSR/BMSR任务快照、CLEAR释放后重查、稳定状态去重；失败不伪造第二物理事件。
- host FakeEnv当前不模拟reset寄存器清空与MDIO page写 → T1/T2补实际需要的设备状态模型，不能靠立刻自清fake证明真板安全。
- adapter控制面接通后，原genericowner会对可恢复timeout尝试恢复 → FBE/unknown/drift与故障恢复阶段失败仍fail-stop，正常初始数据面契约增量同步，不能悄悄扩大自动重试。
- 共享buffer discard与linkretry影响QEMU → 第二轮必须独立覆盖实际调用路径、旧新socket、flush和正常quiet；D1/VF2未涉及的硬件不重新bring-up。
- 本次基线核对未发现需独立落账的范围外新缺陷；VF2既有构建失败继续按原记录报告。

## Migration Plan

按tasks三轮推进；首轮只展开一个Cycle，初建draft，2026-10-13用户批准详细计划后转ready。测试通过后先Review，再展开下一轮；所有板测为现有FIT RAM临时启动，Wireshark沿用永久豁免，其他恢复验收无豁免。普通镜像先构建，probe最后构建，避免同名FIT被旧模式覆盖；文件名/版本仅定位操作，不作为验收匹配条件。

失败时停止新增操作、保留不确定backing；外部RESET回官方Linux。既有用户“reset顺手做”的操作习惯可复用，不要求额外截图/身份表，但不得把未做或失败的runtime reset计入完成。

## Readiness

设计选择已明确，无待Act决定的接口/所有权语义。尚无本轮产品运行结果；真实DMA停止、link交错和deadline是各轮必须取得的目标行为，不是已通过结论。2026-10-13用户批准详细计划，Gate2 PASS，首轮ready；任何运行结果否定D2停止边界即阻塞该轮并返回Plan。
