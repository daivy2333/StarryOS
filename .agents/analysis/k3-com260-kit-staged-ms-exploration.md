# CoM260 Kit 真板里程碑分层探索（待手动迁入 `.agents/analysis/`）

> 采集：2026-09-29；StarryOS `k3` @ `160d7967b585f2319e89188d0644c16f3c79b04b`；参考仓库 `/home/daivy/projects/serial/work/k3` @ `e6d055720de5f13f3cde6026817e03412c8acda6`。参考仓库有既存未提交改动，本次未修改。
>
> 权威范围：用户确认实物板型号为 **CoM260 Kit**；`.agents/docs/tasks.md:128-230` 是 MS09–MS15 路线及状态的权威。本文是 Explorer 调查输入，不更改 milestone、建立 change 或充当 Plan Context。接口接缝见仓库根目录 `k3-branch-board-adaptation-seams.md`。

## 结论与证据等级

CoM260 Kit 的**型号**已确认，MS09 的「目标板是谁」不再悬而未决；但目前没有当前实物板的板卡修订、启动日志、运行时 FDT、固件版本、内存图、寄存器读值或网络实测。参考仓库 `.claude/runbooks/k3-com260-uart-boot.md` 记录的是 2026-09-11 一次 Kit 上 BootROM→SPL/FSBL→OpenSBI→U-Boot→Linux root shell 的历史观察，可证明参考环境曾跑通，不能作为本轮 StarryOS 已启动的证据。`docs/reference/known-gaps.md` 的 G7 仍 open：`k3_com260_kit_v02.dts` 名字最接近 Kit，但它与用户指南中的 v03 产品版本不能直接等同，更不能仅凭 `model` 或 `compatible` 判定当前加载的 DTB。

调查证据按层使用：当前板运行观察 > 与该板修订和固件对应的运行 FDT/原理图/手册 > 参考仓库静态 DTS 与文档 > 第三方代码和固定配置示例。后两层只能产生候选参数，不能替代真板验收。当前 `openspec list` 为 `No active changes found`；MS09–MS15 均为 `planned`，确认型号本身不把 T13 或任一 Gate 改为完成。

## 逐级落地的探索边界

| 阶段 | 本轮已知的实现/硬件接缝 | 下一轮须取得的直接事实 | 进入后继阶段的判据 |
|---|---|---|---|
| MS09 / T13–T15，板级事实与可观测链路 | K3 平台尚未接入根 feature、Makefile 和 `kernel/src/platform`；非 D1 默认落入 QEMU descriptor、early UART 与 VirtIO-net IRQ 路径。参考 DTS 给出 UART0、`&eth1`/RGMII/PHY 地址 1 候选。 | 当前板修订与所加载 DTB；固件/SBI 能力；启动介质、DRAM/reserved/U-Boot 占用；boot hart 与可用 hart；串口 pinmux/clock；MAC 实例、MMIO、IRQ、reset/clock、PHY/MDIO 与 RJ45 对应关系。 | 可重复 RAM 引导并观察首字节；MAC 寄存器不全零/全一，PHY/link 结果可解释、可重复，因而能选择后端。未证明实际控制器兼容前不预选 DWMAC。 |
| MS10 / T16，设备中断 | K3 参考 `docs/interrupts/k3-interrupt-and-time.md` 给出 APLIC→IMSIC→hart 路径及候选地址；StarryOS D1 `irq.rs` 是 PLIC claim/complete，`descriptor.rs` 仅表达 PLIC。 | 当前 FDT 的 source、trigger、EID、target hart；设备 cause 的 W1C/clear-on-read 语义；mask/ack/complete 顺序及重复投递。 | MAC IRQ 的 controller claim 与设备 status 对齐；破坏性 cause 在 ack 前保留，EOI 后再次触发且无风暴。IRQ 到达不等于 descriptor 已完成。 |
| MS11 / T17–T19，轮询 RX/TX | 第三方 `k3_gmac` 有描述符、ring 与 DMA 参考实现，但用 `rdrive`/`rd_net`，不是本仓后端。参考文档 G5 尚未关闭目标板 DMA coherency/IOMMU。 | CPU PA→device bus address、DMA aperture、cache line/维护范围、descriptor 与 data buffer 可见性、DMA OWN/doorbell/terminal/reclaim、真实包抓取。 | CPU 与设备观察一致的所有权变迁；坏帧、ring full、timeout 能定位 submit/terminal/reclaim 阶段；ARP/ICMP/UDP/TCP 与抓包一致。此阶段仍轮询，不引入异步 wake。 |
| MS12 / T20–T21，异步双向数据面 | 本仓 `axdriver_net` 有 `NetTxQueue`/epoch cookie 与 `NetQueueControl`；`axnet::async_rx` 和 VirtIO IRQ 路径已有 register→arm→recheck、唯一 queue owner 的软件契约。 | 真板 RX/TX completion 与 IRQ、budget、slot/backpressure、flush/drain 之间的可观测配合；最后一次 completion 与 wait 注册交错。 | RX/TX burst、queue full、drop、occupancy、flush 可观测，无丢失唤醒、双重 owner 或永久 Pending；QEMU 契约回归只能证明软件未退化。 |
| MS13 / T22，单 hart 恢复 | 本仓 `NetRecoveryControl` 和 VirtIO reset/epoch 逻辑是契约参照；参考 GMAC reset 超时后继续初始化不能作为安全恢复策略。 | link flap 与完整 reset 的故障注入；bus mastering/DMA 停止的设备侧证据；stale completion、等待者错误传播、backing retention。 | quiesce 未确认则保持 faulted owner、保留 backing、拒绝新提交；确认 DMA 停止后才释放/复用，reset 前后 epoch 不混用。寄存器读回不单独证明 DMA 已停止。 |
| MS14 / T23–T24，真板多 hart 与长稳 | K3 SoC 资料称 8 X100 + 8 A100，但不证明固件使其全在线或 ISA/中断能力一致。`axtask` 只对已发布 schedulable run queue 设置 affinity；QEMU SMP16 不是板级证明。 | 实际 online/可调度 hart、每类核 ISA 与 IPI/IMSIC 路由；跨 hart wake、ring full 与 reset 交错；长稳 drop、p99、occupancy、IRQ/CPU 指标。 | 组合压力和 soak 无 stall，指标可复现并能归因。MS15 仅在这些数据明确触发一个瓶颈后进入，不预先做 batching/offload/零拷贝等优化。 |

## MS09 首轮：先取得可改变设计的现场事实

1. **身份与启动边界。**记录实际板卡修订、启动介质、串口日志中的固件与 U-Boot/OpenSBI 版本、U-Boot 所加载的 DTB（完整 FDT 或可核对的板级节点），以及当前 boot hart。先读出 DRAM、reserved-memory、U-Boot 自占区，再确定 RAM payload/FIT 的 `load` 与 `entry`；参考仓库 `docs/boot/k3-ram-boot-and-fastboot.md` 中 `0x180000000`、`0x140000000`、`0x138000000` 只是固定第三方示例。尚未核实地址前不刷写持久介质。
2. **最小首字节。**在保留现有 bootloader 的假设下，核对 `a0` hartid/`a1` FDT handoff、页表覆盖和 polling UART 输出。参考 `docs/serial/com260-uart.md` 给出的 `uart0` `0xd4017000`、stride 4、32-bit 访问、115200-8N1 是候选；若静默，须分别区分镜像未执行、页表/MMIO 不达、UART clock/pinmux 未开与串口物理连接。`kernel/src/platform/early_console.rs` 的实现形状可复用，硬件行为未验证。
3. **控制器与链路。**运行 FDT/板级资料确定 MAC 实例与 MMIO、clock/reset、PHY handle/MDIO、RGMII delay、物理网口映射，再做受控的寄存器和 PHY/link 观察。`docs/network/com260-gmac-phy.md` 的 `&eth1` 和 PHY 地址 1 是候选；`0xcac82000/0x2000` 来自 IFX 变体，不是当前 Kit 的已证地址。寄存器全零/全一先回查地址、clock、reset、映射，不据此实现 DMA。

这三组现场事实是后续 Plan 的输入，不是本文要求立即操作真板的命令清单。当前尚不知板卡修订及实际 DTB，因此任何写死的镜像地址、UART、MAC 或 DMA 参数都只能作为待验证假设。

## 跨阶段风险与已有软件接缝

- **IRQ 与 DMA 不合并验收。**APLIC/IMSIC 的 source→EID→hart 和 device cause 要先在 MS10 闭合；MS11 再看 descriptor/data buffer 的 DMA 可见性。参考 `docs/dma/k3-dma-and-memory-ownership.md` 将 GMAC 内建 DMA 与通用 DMA、AMP 共享内存区分；后两者不自动提供 GMAC 的地址或 cache 契约。
- **第三方驱动只能局部审计。**`others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/core.rs` 采用 64 项 ring、2048 字节 buffer；`submit_tx` 对超过 buffer 的包使用 `len.min(BUFFER_SIZE)`，有静默截短风险；reset timeout 后仍继续初始化；TX/RX descriptor error 的向上传播不满足本仓 owner/错误契约。`queue.rs` 的 IRQ `try_lock` 失败直接返回空事件，尚无最后一次 completion 的确定性重查证明。代码存在不代表已适配 StarryOS，也不能直接把 `rd_net` 接到 `axdriver_net`。
- **硬件差异不污染已验证软件契约。**MS12 的接点是 `crates/axdriver_net/src/lib.rs`、`crates/axnet/src/async_rx.rs` 和 `kernel/src/drivers/virtio_net_irq.rs` 的 owner、epoch、arm/recheck、服务启动顺序；只有真板后端或可达公共接口确有需要时才改共用逻辑。MS13 的 DMA stop 与 backing retention 不能仅靠 QEMU reset 结果宣称。
- **异构核不能按核心总数安排任务。**`docs/platform/k3-soc-overview.md` 的 8+8 是 SoC 能力；MS14 须用实际 online 集合、ISA/中断与调度证据决定 affinity，不能把 A100 默认当成与 X100 等价的网络 worker。

## 交接与验证状态

下一次应从 MS09 的运行 DTB、固件/内存 handoff、串口首字节与 MAC/PHY 实况开始；若用户尚不能提供板上访问，可先依据具体板卡修订和固件包收窄 DTS 候选，但不能关闭 G7。MS09 形成稳定基线后才为 MS10 制定当前 Cycle；此后各阶段只使用上阶段已验收的真板事实。Plan 仍需独立核对代码与现场、写 BDD/RTM/测试见证；本文不使 Gate 1–4 自动通过。

本轮仅使用 `git status --short --branch`、`git rev-parse HEAD`、`openspec list`、`rg` 和定向文件读取。没有构建、测试、QEMU、FIT、Fastboot、刷写或真板操作，故**没有 StarryOS 在 CoM260 Kit 上工作的运行结论**。根目录文档待用户手动放入 `.agents/analysis/`；放入前不登记 R 引用。

关键来源：本仓 `.agents/docs/tasks.md:128-230`、`Cargo.toml`、`Makefile`、`kernel/src/platform/{mod,descriptor,early_console}.rs`、`kernel/src/entry.rs`、`crates/axdriver_net/src/lib.rs`、`crates/axnet/src/async_rx.rs`、`crates/axtask/src/api.rs`；参考仓库 `docs/reference/known-gaps.md`（G3/G4/G5/G7）、`docs/boot/{com260-boot-chain,k3-ram-boot-and-fastboot}.md`、`docs/serial/com260-uart.md`、`docs/interrupts/k3-interrupt-and-time.md`、`docs/network/{com260-gmac-phy,k3-gmac-dma-irq}.md`、`docs/dma/{k3-dma-and-memory-ownership,k3-cache-pma-address-translation}.md`、`.claude/runbooks/k3-com260-uart-boot.md`、`others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/`。
