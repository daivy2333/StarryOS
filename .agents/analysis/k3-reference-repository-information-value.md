# k3 参考仓库的信息资产与 StarryOS 真板工作用途（待手动迁入 `.agents/analysis/`）

> 采集：2026-09-29；StarryOS `k3` @ `160d7967b585f2319e89188d0644c16f3c79b04b`；参考仓库 `/home/daivy/projects/serial/work/k3` 的 `main` @ `e6d055720de5f13f3cde6026817e03412c8acda6`。参考仓库有既存未提交及未跟踪文件，本次只读。
>
> 用途：给后续真板调查、变更计划和排障选择资料入口。用户已确认实物板为 CoM260 Kit；本仓库仍不是当前板的运行时事实来源。已有 `.agents/analysis/k3-reference-repository-migration-assessment.md`（R62）详述迁移量与首启路线；根目录 `k3-com260-kit-staged-ms-exploration.md` 详述各里程碑边界。本文不重复它们的实现方案，也不修改产品代码或项目状态。

## 结论

`k3` 最有价值的是**可追溯的板级资料索引、已暴露的硬件缺口、历史真板排障经验和局部参考实现**。它能缩小 StarryOS 的调查范围、帮助决定先测什么、失败时先检查哪一层；不能直接证明当前 CoM260 Kit 的 DTB、固件、地址、IRQ、DMA 或网络状态，也不是可直接并入 StarryOS 的平台/驱动源码仓。`docs/index.md` 明确其目标是 K3 CoM260 Kit，并写明该仓库不修改 StarryOS；`others/` 则另存发布包和第三方工程，不改变这个边界。

## 仓库地图：从哪里取什么信息

| 位置 | 内容及核对结果 | 对后续工作的具体帮助 | 使用边界 |
|---|---|---|---|
| `docs/index.md`、`docs/reference/source-coverage.md` | 总入口按 platform、boot、serial、interrupts、dma、network 等主题导航；覆盖表登记 76 个唯一 URL，逐项记录来源职责、目标板范围、观察日期、访问状态。 | 计划调查时快速定位原始出处，区分 CoM260、K3 通用和非目标板资料；发现资料变更时知道哪些结论需重核。 | `active` 是聚合状态，不等于原文已取得。多个官网页面标为 `partially-observed`，不能把入口或检索片段当成完整官方正文。 |
| `docs/reference/known-gaps.md` | 汇总 G1–G15；G3（GMAC/PHY）、G4（IRQ）、G5（DMA/cache）为 `partial`，G6（GMAC programmer reference）、G7（Kit 默认 DTS）仍 `open`。每项列出禁止推断和解除条件。 | 直接形成各阶段的调查问题和停止条件；可防止把静态 DTS、通用标准或第三方运行结果当作目标板证据。 | 缺口是参考仓库采集时的状态；当前板上新证据需按实际修订和固件重新判定。用户确认板型并未关闭 G7。 |
| `docs/platform/`、`docs/serial/`、`docs/boot/` | 将 SoC 能力、CoM260 模组、Kit 底板分层；梳理 UART0、clock/reset/pinctrl、启动链、镜像和 7 个 CoM260 命名 DTS 候选；`k3-ram-boot-and-fastboot.md` 分开 RAM stage、FIT load/entry 与持久刷写。 | 服务首轮身份核对、串口首字节和可恢复的 RAM 引导；从板卡修订与运行 FDT 选具体节点，避免套用 D1/QEMU 参数。 | `k3_com260_kit_v02.dts` 仍未唯一映射当前板；示例地址和第三方命令不能未经 DRAM/reserved/U-Boot 占用检查就执行。 |
| `docs/interrupts/`、`docs/dma/`、`docs/network/` | AP APLIC→IMSIC→hart、GMAC/MDIO/PHY/RGMII、descriptor/data buffer ownership、cache/PMA/IOMMU 的静态事实与未知项分开记录。 | 帮助把“寄存器可读、链路 up、IRQ 到达、DMA 完成、包到达”拆为独立可诊断阶段；为目标后端设计最小观测点。 | G3–G6 和 G7 均影响这些主题；参考文档不证明当前设备 cause、source→EID、bus address、coherency 或 DMA stop。 |
| `docs/storage/`、`docs/buses/`、`docs/peripherals/`、`docs/amp/` | 提供 UFS/SD、USB/I²C/PCIe/CAN、GPIO/音频/RTC 及 AP↔RP 共享内存的主题索引。 | 当后续工作明确进入存储启动、外设或 AMP 时，可按已有事实与缺口继续调查，无须从零盘点 SoC。 | 这些能力不是本次 AP 首字节、GMAC/异步网络的自动前置；不能因仓库有资料就扩充当前变更范围。 |
| `others/Buildroot-K3-v1.0.7/` | 实际为预构建包：`FSBL.bin`、`esos.itb`、`fw_dynamic.itb`、`u-boot.itb`、`bootfs.img`、`rootfs.ext4`、分区 JSON 与 `fastboot.yaml`；不是 Buildroot 源码树。`file` 只读检查确认 ITB 为 FDT 容器、bootfs 为 FAT、rootfs 为 ext4。 | 可离线检查固件/DTB 候选和恢复包组成；帮助辨认现有启动链与镜像角色。 | 文件名和镜像内容不证明当前板实际采用的版本或分区；刷写配置会改持久介质，不能作为 StarryOS 首启步骤。 |
| `others/Rt-Async-AMP/` | 第三方 AMP 集成工程及嵌套代码树，含 K3 平台、AIA、UART、GMAC、DMA、OpenSBI、RP 固件与工具。根仓及嵌套目录不是一个可无条件复现的单一工作树，详见 R62。 | 用实现路径和历史故障定位寄存器、描述符、IRQ 顺序及 FIT 打包问题；作为局部机制审计对象。 | 使用 `somehal`、`rdrive`、`rd_net` 等另一套接口，不能直接注册为 StarryOS `axplat`/`axdriver_net` 后端；第三方代码的错误和所有权语义要重新审查。 |
| `.claude/runbooks/`、`.claude/analysis/` | `k3-com260-uart-boot.md` 记录 2026-09-11 Kit 上从 BootROM 到 Bianbu Linux root shell 的实跑；`k3-com260-network-dev-and-file-transfer.md` 明标 `draft / unverified`。分析文档保存采集时的版本和推论。 | 实跑记录能给首轮串口连接、阶段标志和失败定位一个历史参照；分析资料可快速找到此前假设与反例。 | 历史板上结果不等于本轮 StarryOS 或同一固件状态；草稿命令/IP/设备名不得提升为已验证操作规程。 |

## 信息如何服务后续里程碑

| 工作阶段 | 优先读取的资料 | 它实际减少的未知项 | 仍需当前板给出的证据 |
|---|---|---|---|
| 板级事实、首字节、链路（MS09） | `docs/platform/com260-board-resources.md`、`docs/boot/{com260-image-and-dts,k3-ram-boot-and-fastboot}.md`、`docs/serial/com260-uart.md`、`docs/network/com260-gmac-phy.md`、已实跑串口 Runbook | 供电/串口与 RJ45 的物理入口、启动阶段、DTS 候选、MAC/PHY 连接和禁止刷写的边界。 | 板卡修订、运行 FDT、固件与 SBI、内存区间、实际 UART/GMAC/PHY 寄存器和 link。 |
| MAC 中断（MS10） | `docs/interrupts/k3-interrupt-and-time.md`、`docs/network/k3-gmac-dma-irq.md`、第三方 AIA/GMAC handler | 缩小 AP 中断路由、cause/ack/EOI 的审计面，并暴露一次投递后停顿的可能原因。 | 目标 IRQ source→EID→hart、trigger、设备 cause 语义及重复投递。 |
| 轮询双向网络（MS11） | `docs/dma/{k3-dma-and-memory-ownership,k3-cache-pma-address-translation}.md`、`docs/network/k3-gmac-dma-irq.md`、第三方 descriptor/ring 实现 | 区分 CPU PA、device address、descriptor 与数据缓冲区的所有权；定位 submit、doorbell、terminal 和 reclaim 阶段。 | 当前板 DMA aperture/IOMMU/coherency、cache 操作、TX/RX descriptor 终态和抓包。 |
| 异步、恢复、多 hart（MS12–MS14） | 第三方 IRQ/queue/reset 失败记录，配合 StarryOS 的 `axdriver_net`、`axnet`、`axtask` 现有契约 | 提供必须防范的 `try_lock` 丢事件、reset 超时仍继续、stale completion 与异构核假设等反例。 | 无丢失唤醒的双向流量、DMA quiesce、epoch 隔离、实际 online/可调度 hart 及长稳指标。 |

这张表只说明**资料如何帮助调查**，不是实施授权或里程碑完成状态。每一阶段仍以 StarryOS 当前代码、获批变更及本轮真板观察作为设计和验证依据；参考仓库不能替代测试见证。

## 复用时应优先保留的判断边界

1. **来源版本。**`source-coverage.md` 把官网入口、官方 GitHub 对应页、Linux DTS/驱动和第三方代码分级；许多官网入口仅 `partially-observed`。后续引用具体参数时应保留文件/分支/观察日期，并核对当前板的固件和 FDT，不用“官方资料”四字覆盖来源差异。
2. **SoC、模组、Kit、运行状态。**`com260-board-resources.md` 明示 SoC 有 8 X100 + 8 A100 AP 核、模组和 Kit 各有自己的引出/启用边界；核心数量不证明固件 online hart、调度兼容或当前网络端口映射。
3. **参考实现的负面样本。**第三方 GMAC `core.rs` 的 TX 长度截断、reset timeout 后继续，`queue.rs` 的 IRQ `try_lock` 失败返回空事件，都提醒后续 Plan 检查错误传播、硬件停止与最后事件重查；不能仅因某路径在第三方工程跑通过就复制其语义。
4. **操作风险。**RAM 临时引导、固件替换和全盘刷写属于不同风险等级。参考仓库的 Buildroot 发布包和 Fastboot 配置优先用于只读辨认与恢复准备；任何持久写入都须另有目标介质、布局和恢复证据。

## 本次验证与交接

本次运行了只读 `git status --short --branch`、`git rev-parse HEAD`、`rg`、`find`、`sed` 和 `file`；未构建、测试、连接真板、加载 FIT 或写入参考仓库。由此只能确认仓库资料结构、文件类型和文档中所记录的证据边界，不能声明 StarryOS 已在 CoM260 Kit 上通过任何真板 Gate。

后续若用户提供当前 Kit 的板卡修订、串口启动日志和运行 FDT，应先用 `source-coverage.md` 与 `known-gaps.md` 精确收窄候选，再针对匹配的源码/文档展开 Plan 调查。本文暂在项目根目录，待用户手动移入 `.agents/analysis/`；移入前不登记 R 引用。
