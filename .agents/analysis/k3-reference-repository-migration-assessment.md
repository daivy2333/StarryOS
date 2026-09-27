# K3 参考仓库向 StarryOS 的真板迁移评估

> Snapshot: [SNAPSHOT](../docs/SNAPSHOT.md)
> Captured revision: `bea0aeac3a69059841961de2cecc4ac54585a5f8`
> Observed branch: `mul-hart-k3`
> Captured at: 2026-09-17
> External baseline: K3 reference `e6d055720de5f13f3cde6026817e03412c8acda6`; Rt-Async-AMP `ccb1ff0b487e4f49ea570c41f330741eecece935`; tgoskits `19219411d5dc1515496f910d04c93da12ee95be4`; OpenSBI fork `7a2df083ed06373c506e2e6f4e09bbd168202f2d`; rt-async gitlink `1ead7f09b88808ce79cab135044d8a0427261f6f`
> See also: [ArceOS 真板验证方法](arceos-true-board-validation.md)

## 结论

首轮 K3 CoM260 真板启动不应把 `Buildroot-K3-v1.0.7`、OpenSBI、ESOS、U-Boot 或 Linux rootfs 整体迁入 StarryOS。最小路线是保留板上已工作的 BootROM → FSBL → ESOS → OpenSBI → U-Boot 链，只把 StarryOS 作为新的 AP payload 通过 RAM/FIT 临时引导。

StarryOS 首字节所需迁移量可收敛为四类资产：

1. 一个 K3 `axplat` 平台 crate，覆盖入口、内存、polling console、时间、复位和最小单 hart 初始化。
2. 一个已与实物板型对应的 DTB，以及从 DTB/板上日志得到的平台配置。
3. 一个 FIT ITS 和构建入口，把 kernel 与 DTB 打包成 U-Boot 可加载的 `starryos.uimg`。
4. 一组 host 静态检查与真板分层 Gate，不包含持久烧录。

在第一字节之后，再按依赖顺序迁移 AIA、PXA UART、SMP、pinctrl/clock/reset 和 GMAC。进一步展开 `Rt-Async-AMP` 后，AP 侧近期相关的完整审阅面约为 6.4 千行平台/驱动代码，另有约 9.2 千行聚合 DTS；原先的 4.1 千行只统计了最小驱动文件，没有计入 AIA glue、DMA/PTE 和较完整的 adapter。这些数字只表示调查语料规模，不是可复制代码量。StarryOS 已有 UART copier、网络 queue owner、backpressure、completion 和 recovery 契约，硬件代码必须适配这些接口，不能整体替换为参考仓库的框架。

自定义 OpenSBI 不是 AP-only 首启的前置。只有现有板载 OpenSBI 缺少 StarryOS 实际使用的 SBI 扩展，或后续 AMP 共享 SRAM 确实需要修改 PMA 时，才应单独评估、构建和替换 OpenSBI。ESOS/RT24、Buildroot rootfs 和全盘镜像同样不属于首轮迁移。

对本项目而言，复用优先级应是：真板操作与排障经验 → 已验证的硬件事实和失败模式 → 寄存器机制 → 参考代码。`Rt-Async-AMP` 中可直接 cherry-pick 的产品代码应按接近 0 规划，但其启动、FIT/fastboot、串口、AIA 和 GMAC 调试过程可以直接转化为本项目的 Gate 与停止条件。条件触发的存储、OpenSBI 和 AMP 工作不得计入近期迁移量。

## 目标与范围

本文回答五个问题：

- K3 参考仓库展示的真实启动链是什么，StarryOS 应在何处接入？
- `others/Buildroot-K3-v1.0.7` 是什么，哪些内容是运行依赖、恢复资产或无关 Linux 载荷？
- AP-only 首字节、平台可用、网络可用和 AMP 四个目标分别需要迁移什么？
- 当前 StarryOS 已有什么接口，参考代码应适配到哪里？
- 后续真板操作应按哪些 Gate 推进，哪些未知项必须阻塞写盘或驱动实现？

范围限定为 K3 CoM260 AP 侧 StarryOS bring-up，以及与后续异步 UART/NIC 相关的板级资产。本文不授权刷写，不修改产品代码、active change、tasks 或 SNAPSHOT。

## 真板经验是首要复用资产

参考项目最有价值的成果不是一组待搬运的源码，而是一条已经在 CoM260 上走通过、也暴露过失败的操作链。对 StarryOS 应复用这条链的顺序、观察点和停止条件，再用本仓库的平台与驱动接口实现。

### RAM 引导与持久烧录必须分开

参考项目把三类动作放在同一 README 中，但它们的风险不同：

| 动作 | 参考项目用途 | StarryOS 处理 |
| --- | --- | --- |
| `fastboot stage` → `bootm` | 把 AP FIT 上传到 RAM，展开 kernel/DTB 后临时启动 | 首轮采用；复位后回到原系统，不改分区 |
| `mtd erase/write esos` | 更新 RP/RT24 固件 | AP-only 不执行；AMP 独立 change 才考虑 |
| `mtd erase/write opensbi` | 更新带共享 SRAM PMA 修改的 OpenSBI | 首启与普通 GMAC 不执行；确认固件缺口后单组件处理 |
| Buildroot `multi_flash`/整盘镜像 | 工厂恢复或全介质部署 | 只保留为恢复资料；不能作为 StarryOS 首次上板入口 |

因此，首轮常说的“烧录 StarryOS”实际应改成“RAM 临时引导 StarryOS”。只有 RAM 启动、复位恢复、平台事实和目标驱动均通过后，持久部署才有讨论价值。参考命令中的 `fastboot` 参数、上传地址、`Ctrl+C` 返回 U-Boot 和 `bootm` 语法是操作经验，不是未经现场确认即可执行的固定命令。

### 可直接转成 StarryOS Gate 的经验

| 参考项目已暴露的经验 | 在本项目中的复用动作 | 必须取得的证据 |
| --- | --- | --- |
| 出厂固件链曾能进入 Linux/root shell | 先保留同一串口与原系统启动基线 | 板型、介质、固件 banner、完整串口日志和可重复复位 |
| FIT 上传点与 kernel load 地址重叠会覆盖镜像自身 | 在 host 侧检查 FIT 子镜像和所有地址区间 | `dumpimage` 结果、kernel/DTB/FIT size、`bdinfo`、reserved-memory 和 U-Boot relocation |
| fastboot stage 后 U-Boot 不会自动回到提示符 | 把 host 上传与板端 `Ctrl+C`/`bootm` 作为两个明确阶段 | 每阶段提示符、返回状态和下一条命令入口 |
| 板载 U-Boot/OpenSBI 已能交接 AP payload | 首轮保留固件，只替换 RAM 中的 AP kernel+DTB | U-Boot 成功解析 FIT、跳转地址、SBI capability 和复位恢复 |
| 串口成功依赖基址、stride、访问宽度、clock/pinmux | 先用 polling early console，保留并记录 bootloader 状态 | first byte、boot hart、DTB model、memory/MMIO；无输出时不启 IRQ/SMP |
| AIA 曾因 `stopei` 读写分离吞 MSI | 将 claim、handler、device clear、complete 分层验证 | 连续多次 IRQ、source/EID 对应、设备 cause 和 EOI 后再次触发 |
| GMAC link up 不能证明 DMA 工作 | 先读 MAC/version/status，再观察 descriptor、current pointer 和 DMA state | 非全零/全一寄存器、TX/RX 引擎推进、OWN 转移、tail/doorbell 和抓包 |
| GMAC 常量错误导致多轮盲测 | 实现前逐项对照同版 U-Boot/Linux 的 offset、bit 和写序 | 常量对照表；每轮只改变一个可证伪假设 |
| 大于 4 GiB 地址、cache/PMA/PBMT 曾造成误判 | 分开验证 CPU PA、DMA bus address、descriptor/buffer cache ownership | 设备可见地址、clean/invalidate 时点、barrier、IOMMU/coherency 和完成终态 |
| 环境扰动曾掩盖 AMP race | 低层 Gate 不使用 shell、rootfs、wget 或 benchmark 充当成功条件 | 最小内核操作先通过；workload 只证明其所在的更高层 |

这组经验应成为后续 MS09-MS12 Plan 的调查输入。实际板上首次成功后，再由 Recorder 把本项目已验证的命令、提示符和恢复步骤写成 Runbook；在此之前，分析文档只保存来源、风险和 Gate，不能把第三方命令提升为本项目操作规程。

## `Rt-Async-AMP` 工作量与复用价值

### 仓库实际组成

`Rt-Async-AMP` 不是单一内核仓库，而是一个 AMP 产品集成仓：根仓负责编排、RP 固件、共享内存/RPC 和用户工具，四个 gitlink 分别指向 `ov-channels`、K3 OpenSBI、rt-async 和 tgoskits。根仓从 2026-05-25 到 2026-09-05 共 252 个提交，提交者名称虽有大小写差异，但邮箱均为 `oveln@outlook.com`。当前根仓有 171 个非 gitlink 文件，其中约 17.4 千行 Rust；这部分主要是 AMP 产品代码，不应与 AP 真板移植量相加。

| 代码域 | 可核对规模 | 对当前 StarryOS 的价值 |
| --- | ---: | --- |
| tgoskits K3/AIA 分支 | 作者 41 个提交、触及 149 个文件 | AP 侧价值最高；包含板级、AIA、PXA UART、pinctrl、GMAC、DMA/PTE、UFS/SDHCI 和 `rt_shm` |
| 根仓 AMP 集成 | 252 个提交、约 17.4 千行 Rust | 构建分层和诊断方法可借鉴；RT24、RPC、机器人应用近期不迁移 |
| rt-async | 100 个提交、约 7.7 千行 Rust | 仅 AMP/RT24 目标需要；不能替换本项目 `axtask` |
| K3 OpenSBI fork | 基于 `k3-br-v1.0.0` 的 5 个作者提交 | 只解决日志与 AMP SRAM PMA；AP-only 首启不需要 |
| tgoskits 许可证 | 顶层 Apache-2.0 | 仍需逐文件保留来源与许可证，并按本项目接口重写 |
| 根仓与 rt-async 许可证 | 未发现顶层 LICENSE 或 Cargo `license` 声明 | 在许可证澄清前，不复制其自有代码；只记录机制和事实 |

当前参考目录并不是根仓 README 所假设的标准递归 submodule 工作树：四个 gitlink 均显示未初始化，`ov-channels` 为空，OpenSBI 与 rt-async 位于额外一层子目录；rt-async 的观察 HEAD `d6f3186f` 是根仓固定点 `1ead7f09` 的后继。因而本次没有把该目录的构建结果当作可复现证据，也没有运行 `cargo xtask build` 或测试。

### 对近期路线有帮助的七组工作

| 工作包 | 当前审阅面 | 帮助方式 | 迁移判断 |
| --- | ---: | --- | --- |
| K3 board/FIT/DTS | 约 9.2 千行，主要是反编译聚合 DTS | 提供 FIT 结构、启动地址、设备节点和板级连线候选 | 高价值事实源；只抽取已由目标板确认的节点和地址 |
| APLIC/IMSIC 与 IRQ domain | 约 1,471 行含 glue | 给出 FDT probe、EID 分配、APLIC→IMSIC 路由、`stopei` 原子 claim/complete | 高价值机制；重写为 `axplat::irq::IrqIf`，不移植 rdrive/rdif domain 框架 |
| PXA UART | 约 1,300 行，19 个单元测试 | 覆盖 32-bit MMIO、stride 4、64-byte FIFO、RX timeout、错误位保留和 CTI 丢失恢复 | 高价值硬件层；接入现有 `UartPort`/copier/waker |
| K3 pinctrl | 约 793 行 | 覆盖 mux、bias、drive-strength 和 IO 电源域 ASAR 解锁 | MS09/MS15 输入；先 preserve/dump，确认必须重配后再实现 |
| K3 GMAC | 约 2,079 行，未见模块内单元测试 | 提供 DWMAC 5.10a、MDIO/RTL8211F、descriptor、doorbell、IRQ 和 syscon 顺序 | MS09-MS12 的主要硬件参考；必须改造成现有 queue/recovery 契约 |
| RISC-V DMA/PTE | 约 795 行 | 提供 Zicbom clean/invalidate 和 PBMT 编码的实现与失败历史 | 用于 MS11 cache/DMA 设计；不能从共享 SRAM 结论泛化到 GMAC DMA |
| 环境/构建编排 | 根仓 xtask 约 1,895 行 | 展示 env profile、K3 OpenSBI/ESOS/AP FIT 分产物和手动刷写边界 | 借鉴产物职责和检查项；不引入完整 xtask 或 AMP 聚合构建 |

上述近期代码审阅面约 6.4 千行，不含 DTS、构建器和生成/搬运的 Linux 参考源码。它回答“硬件怎样工作”和“哪些错误已经踩过”，但不能回答“怎样满足当前 StarryOS 的所有权与异步契约”。近期可复用比例更适合按成果计：七组工作全部能减少调查量，产品代码则全部需要适配或重写。

### 条件触发与近期无关的工作

| 工作包 | 规模/证据 | 何时有用 |
| --- | ---: | --- |
| K3 UFS | 约 3,249 行、23 个单元测试 | 明确要从 UFS rootfs 启动或开展块设备 milestone 时；不应成为 first-byte 或 GMAC 前置 |
| K3 SDHCI | 约 761 行、12 个单元测试 | 目标介质确认是 SD/eMMC 且存储进入范围时 |
| AP `rt_shm` | 约 852 行 | AMP change 才需要；其 ioctl、mmap、mailbox 和 cache/PMA 契约与普通网卡无关 |
| RT24 chip/firmware | 根仓 chip 约 2,533 行、K3 app 约 4,317 行 | 用户明确要求 AP+RP 双内核时 |
| ov-shm/ov-rpc 与用户工具 | 根仓约 7,521 行 Rust，不含缺失的 ov-channels 子仓 | AMP 通信、RPC 或机器人 workload；不进入 AP 平台和网络主线 |
| OpenSBI PMA fork | 5 个提交 | 共享 SRAM 必须变为非缓存且板载 OpenSBI 无法配置时；作为独立高风险固件 change |

rt-async 的 executor、future、priority scheduler 和 IRQ latch 有独立价值，但本项目已经以 `axtask` 和现有异步驱动契约为唯一 runtime。引入它们会形成第二套 executor，违反当前项目边界。QEMU 双核分流补丁、第二 UART、RT24 PLIC、软串口、机器人控制和 RPC latency 优化也不服务 MS09-MS12。

### 最有价值的已验证教训

- FIT 上传缓冲区不能与 FIT 内 kernel/DTB 的展开地址重叠；参考仓库曾遇到 `new format image overwritten`，由 `0x180000000` stage、`0x140000000` kernel 和 `0x138000000` DTB 三段分离解决。地址仍需在本板重新核对。
- GMAC 真板最终通过 DHCP 和 ping，但此前存在 TSF 位误写为 FTQ、MTL channel base `0xc00`/`0xd00` 混淆、DMA reset/PBL 和大于 4 GiB DMA mask 等独立问题。MS09-MS11 应把寄存器定义、初始化写序和引擎状态寄存器逐项对照 U-Boot/Linux，而不是仅观察 link up。
- AIA 曾因把 `stopei` claim 与 complete 拆开而吞掉新的 MSI；最终改为 handler 前单条 `csrrw`。这直接支持 MS10 的 claim/handler/device-clear/EOI 分层，但当前实现没有可直接搬来的本仓接口。
- AMP 共享 SRAM 调试先后证明 PBMT 假设、CBO 补偿和最终 OpenSBI PMA 方案会随固件事实演进。只应采用最后确认的结论，不能把中间 CBO 版本或 `rt_shm` 同步点用于 GMAC descriptor。
- PXA UART 的 lost-CTI 修复表明只依赖一次 RX timeout IRQ 会遗留数据。迁移到现有 copier 时要保留状态检查和 register-recheck，而不是复制参考 runtime。
- 参考仓库明确记录 qemu-aia 目前只有 AP 侧完整，rt-async RP 侧没有 AIA 支持。它能作为 AP AIA 机制试验，不构成 K3 AMP 双端仿真通过。

## 已确认事实、推断与未确认项

### 已确认事实

- K3 参考仓库记录的 local boot 链是 BootROM → FSBL/SPL → ESOS → OpenSBI → U-Boot → payload/OS。[启动链](../../../k3/docs/boot/com260-boot-chain.md)
- 2026-09-11 的真板记录已观察到出厂 CoM260 从 BootROM 启动到 Bianbu 4.0.1 root shell，UART 物理链和板载固件链当时可用。[串口启动 Runbook](../../../k3/.claude/runbooks/k3-com260-uart-boot.md)
- 当前 StarryOS 只有 QEMU、VisionFive 2 和本地 Lichee D1 平台接入，没有 K3 platform crate、K3 feature 或 K3 构建目标。现有 D1 平台 crate 共 829 行，可作为 StarryOS `axplat` 接口形状的参考，但其中 PLIC、内存地址、T-Head PTE 属性和 UART 常量均不能用于 K3。
- StarryOS 的 RISC-V 平台代码直接调用 SBI TIME、IPI、HSM 和 SRST：`set_timer`、`send_ipi`、`hart_start`、`system_reset`。K3 使用板载 OpenSBI 时必须逐项验证这些扩展，而不能只看到 OpenSBI banner 就判定兼容。
- K3 AP 外部中断是 APLIC → IMSIC → hart INTC，不是 PLIC。官方 DTS 交叉证据给出 CLINT `0xe081c000`、IMSIC `0xe0400000`、APLIC `0xe0804000`，覆盖 16 个 AP hart。[中断与时间](../../../k3/docs/interrupts/k3-interrupt-and-time.md)
- Rt-Async-AMP/tgoskits 提供 K3 FIT、AIA、PXA UART、pinctrl 和 DWMAC5 参考实现；其 AP FIT 把 kernel 放到 `0x140000000`，DTB 放到 `0x138000000`，整包示例暂存在 `0x180000000`。这些是固定第三方实现值，未被当前实物板 DRAM/reserved-memory 证据确认。[启动与板级分析](../../../k3/.claude/analysis/rt-async-amp-k3-boot-platform.md)
- 当前 MS08 明确只证明 QEMU `SMP=16` 同构软件并发，不证明 K3 AIA、X100/A100、真板 online hart、DMA/cache 或 GMAC。K3 工作应进入 MS09 之后的独立 change，不能混入 MS08。

### 推断

- U-Boot 已运行时，OpenSBI 已驻留并负责 S-mode SBI 服务，因此 RAM 引导 StarryOS 不需要再次把 `fw_dynamic.itb` 与 kernel 打成同一 payload。这个结论仍依赖板上 U-Boot 的实际 handoff 行为与 SBI probe。
- 第一字节可以先使用 U-Boot 保留下来的 UART clock/pinmux 状态，采用只读状态记录和 polling TX；这能减少首轮变量，但不能替代后续对 clock/reset/pinctrl 的显式验证。
- StarryOS 可以沿用本地 D1 platform crate 的模块边界重新实现 K3 platform crate，同时从 tgoskits 借鉴 AIA/PXA/GMAC机制。直接 vendor tgoskits 的整套 somehal/driver framework 会与当前 ArceOS `0.3.0-preview.2` 平台接口和网络所有权模型冲突。

### 未确认项

- 当前实物板究竟对应 `k3_com260.dtb`、`k3_com260_kit_v02.dtb`、`k3_com260_ifx*.dtb` 或其他变体。Buildroot bootfs 同时包含六个 CoM260 命名候选，文件名不能裁决板型。
- 当前板载 OpenSBI 的版本、Domain0 hart 集合以及 TIME/IPI/RFENCE/HSM/SRST 扩展结果；历史 Runbook 只记录启动阶段标志，没有保存完整 OpenSBI capability 输出。
- 当前板的 DRAM、reserved-memory、U-Boot relocation/working area 与 `0x138000000`、`0x140000000`、`0x180000000` 是否无重叠。
- AP 16 个 hart 中哪些在当前固件、DTS 和 HSM 下 online/schedulable；X100 与 A100 的 ISA、cache 和启动约束不能由 QEMU `SMP=16` 推出。
- 目标 GMAC 实例、PHY 型号、RGMII delay、clock/reset、IRQ source、DMA aperture、IOMMU 和 coherency。参考 IFX DTS 与 tgoskits 代码不能代替板型确认和真板证据。

## `Buildroot-K3-v1.0.7` 的实际职责

`others/Buildroot-K3-v1.0.7` 名称像源码目录，实际内容是一套约 1.8 GiB 的预构建发布/恢复包，没有 Buildroot 源码、package tree 或可直接修改的 defconfig。主要空间来自 1.5 GiB `rootfs.ext4` 和 256 MiB `bootfs.img`。

| 产物 | 作用 | StarryOS 首轮处理 |
| --- | --- | --- |
| `factory/bootinfo_*.bin` | 让 BootROM 在不同介质定位 FSBL | 保留板上版本；不迁移、不刷写 |
| `factory/FSBL.bin` | DDR 初始化并加载 ESOS/OpenSBI/U-Boot | 保留；首轮绝不替换 |
| `esos.itb` | K3 RP/ESOS 容器，含多板型 RP DTB 和双 RT24 固件 | AP-only 不需要迁移；AMP 阶段再评估 |
| `fw_dynamic.itb` | OpenSBI fw_dynamic，FIT load `0x100000000` | 作为恢复/对照资产；首轮使用板载实例 |
| `u-boot.itb` | U-Boot 及多板型 DTB | 使用板载 U-Boot；不导入 StarryOS |
| `env.bin` | U-Boot 环境，包含内核/DTB地址和多介质启动逻辑 | 只读参考；不能覆盖现场环境 |
| `bootfs.img` | Linux `Image`、initramfs 和多板型 DTB | 可提取 DTB 作候选调查；不作为 StarryOS运行依赖 |
| `rootfs.ext4` | Buildroot/Bianbu Linux 用户空间 | 第一字节、平台和驱动 Gate 都不需要 |
| `ec.bin` | EC 固件及可选刷写动作 | 与 AP kernel 首启无关；没有板型证据时不触碰 |
| `fastboot.yaml` | BROM/U-Boot stage 和多分区刷写编排 | 只用于理解恢复包；当前不可执行 |
| `partition_*.json`、`genimage.cfg` | 4 MiB SPI-NOR、64 MiB SPI-NAND、GPT/SD 等布局 | 只读恢复参考；不能由通用名称推定现场介质 |

`fastboot.yaml` 会先探测 BROM、stage FSBL/U-Boot，再按 `mtd-size` 和 `blk-size` 选择布局并执行多分区写入。`genimage.cfg` 则描述一个包含 env、bootinfo、FSBL、ESOS、OpenSBI、U-Boot、bootfs 和 rootfs 的整盘 GPT 镜像。这两条路径都会扩大故障域，不适合作为第一次 StarryOS kernel 验证入口。

该包对当前工作的价值是：

- 保存一套恢复候选和固件布局样本；
- 提供 U-Boot env、DTB 候选、FIT load 地址和文件大小事实；
- 允许离线检查 FIT/DTB，不需要先连接开发板。

它不应被复制到 StarryOS 仓库，也不应成为 Cargo/Make 依赖。若未来需要可重建的 K3 官方镜像，应另行取得 SDK manifests、Buildroot/buildroot-ext 源码和精确板型配置；当前二进制包不能承担这个职责。

## 启动和迁移数据流

```text
板载持久固件（首轮保持不变）
BootROM → FSBL → ESOS → OpenSBI → U-Boot
                                      │
                                      │ RAM stage / FIT parse
                                      ▼
                    starryos.uimg = kernel + selected DTB
                                      │
                         OpenSBI handoff: a0=hartid, a1=DTB
                                      ▼
                    K3 axplat boot/stack/page-table entry
                                      │
                     polling UART first byte + platform facts
                                      │
                     AIA/timer/SMP → UART IRQ → GMAC DMA/IRQ
```

`0x180000000` 是第三方示例中的 FIT 上传缓冲区；FIT 内 kernel/DTB 的 load 地址分别是 `0x140000000` 和 `0x138000000`。三个地址承担不同角色，不能混写为一个 kernel entry。进入 Plan 前必须结合板上 `bdinfo`、DRAM、reserved-memory、U-Boot relocation 和实际 FIT 展开大小做区间检查。

## 分阶段迁移清单

### A. AP-only RAM 首字节：必须迁移

| 资产 | 来源/实现方向 | 说明 |
| --- | --- | --- |
| `axplat-riscv64-k3-com260` | 在 StarryOS 内新增；以 D1 `axplat` 接口形状为参考 | 实现 boot、mem、console、time、power/init 的最小集合；不复制 D1 常量或 PLIC |
| K3 platform config | 由目标 DTB和现场日志生成 | 至少包含 kernel base、RAM/reserved ranges、MMIO、timer frequency、UART base/width/stride |
| polling early console | 从 tgoskits PXA UART 读取机制，按 StarryOS console 接口重写 | 不依赖 IRQ、task、heap、rootfs 或 async copier；优先保留固件配置并记录原值 |
| 目标 DTB | 从官方/Buildroot候选中按实物身份选定 | 不把第三方固定 IFX DTB直接设为默认 |
| FIT ITS + build target | 参考 `spacemitk3-com260kit.its`，在 StarryOS构建系统实现 | 产物只含 kernel + DTB；host 侧检查 type/arch/load/entry/size |
| RAM boot 操作合同 | 后续 Recorder/Runbook | 先读取 U-Boot help、`bdinfo`、内存和命令语法；Explorer 本文不授权执行 |

规模上，这是一个平台 crate、一个平台配置、一个 ITS、一个构建目标和若干 host 检查。D1 平台 crate 的 829 行只能作为接口规模参照；K3 的页表、AIA和地址事实会改变实现，不能据此给出精确 LOC 承诺。

### B. 平台、IRQ 与 SMP：第二批迁移

| 资产 | 参考实现规模 | 适配要求 |
| --- | ---: | --- |
| APLIC/IMSIC + RISC-V trap glue | 完整审阅面约 1,471 行；其中 K3 核心子集约 717 行 | 映射到当前 `axplat::irq::IrqIf`；保持 `stopei` claim/complete 语义；增加每 hart init 和后续 affinity Gate |
| SBI capability 与 hart过滤 | 当前 StarryOS power/time/irq 接口 | 验证 TIME/IPI/HSM/SRST；按 DTB 和 SBI domain 取得 online/schedulable 集合，不把 RT24 纳入 AP |
| PXA UART 硬件层 | 通用硬件层、IRQ 和 adapter 完整审阅面约 1,300 行 | 保留 32-bit MMIO、stride 4、UUE/OUT2、RX timeout、THRE/TEMT 区分；接入已有 copier/waker/drain，不复制参考 runtime |
| pinctrl/clock/reset | K3 pinctrl 完整审阅面约 793 行，另有 syscon/clock 事实 | 首轮先 dump/preserve；需要重配时按 provider/consumer 顺序实现，不能用“自愈”抢占其他 owner |

SMP 应在单 hart first-byte、timer 和 AIA 基线之后展开。当前 MS08 的动态 placement 可作为软件策略输入，但 K3 必须重新取得真实 hart集合、异构 ISA和跨 hart 中断证据。

### C. GMAC 网络：第三批迁移

K3 GMAC 参考实现由 `core.rs`、`desc.rs`、`mdio.rs`、`queue.rs`、`regs.rs`、`syscon.rs` 和 adapter 组成，约 2,079 行；pinctrl 另计。可复用的是硬件知识和状态机，不是整个网络栈：

- DWMAC5 MAC/MTL/DMA 初始化；
- MDIO/PHY、自协商和 RGMII delay-line 输入；
- descriptor/data buffer 分离的 ownership；
- non-coherent descriptor clean/invalidate 与 doorbell ordering；
- IRQ status/W1C、submit/reclaim 和 ring-full 处理。

接入 StarryOS 时必须保留 `NetQueueControl`、`NetTxQueue`、packet slot、唯一 owner、EVENT_IDX 无关的 transport-neutral 通知契约，以及 MS07 recovery epoch。参考 handler 在 `try_lock` 失败时返回空事件，未证明最后一次完成一定会再次触发 IRQ；迁移时必须建立 mask/poll/rearm/recheck 或等价的确定性推进契约。

### D. OpenSBI、ESOS 与 AMP：按触发条件迁移

| 组件 | 进入条件 | 当前结论 |
| --- | --- | --- |
| 自定义 OpenSBI | 板载 OpenSBI 缺少必需 SBI 扩展，或 AMP shared SRAM 需要经验证的 PMA 修改 | 不属于 AP-only首启。参考 fork 的 K3-specific 源约 1,586 行，但包含 core power 和实验性 AMP PMA修改，必须拆分审计 |
| ESOS / RT24 payload | 用户明确要求 AP/RP AMP，且 RP资源、共享窗、mailbox和恢复边界已规划 | 不迁入普通 StarryOS kernel；保持独立 firmware/ITB |
| `ov-shm` / RPC / mailbox | AMP 数据通路进入已批准 change | 只复用协议和 ownership调查；不能把 mailbox通知当作数据可见性 |
| Buildroot rootfs | StarryOS 已具备目标存储、块设备、文件系统和用户 ABI，并明确要复用该用户空间 | 当前无需求；首轮和网卡 bring-up均不需要 |

参考 OpenSBI fork 为 `0xc0800000..0xc0880000` AMP 窗口查找 PMA entry并改为 IO 属性。该行为只服务 AP/RP共享内存实验，不应为了启动普通 AP kernel而引入。替换 OpenSBI 还会改变 HSM、IPI、timer、PMP/PMA和 U-Boot handoff，必须作为独立高风险组件处理。

## 不迁移或只保留为参考的内容

- 不复制 `rootfs.ext4`、`bootfs.img`、`env.bin`、FSBL、U-Boot、ESOS或完整 Buildroot包到 StarryOS。
- 不 vendor tgoskits 的完整 somehal、driver framework、StarryOS fork 或网络栈。
- 不把 9,228 行聚合 DTS 原样纳入产品；应以确认后的目标板 DTS为输入，并把 StarryOS必须依赖的板级事实收敛到平台配置/解析层。
- 不把 RT24 PLIC/timer/atomic策略用于 AP；RP 的 local-IRQ critical section不能证明 AP SMP安全。
- 不把第三方 IFX DTB、`0x138000000`/`0x140000000`/`0x180000000`、GMAC phase或 PHY配置提升为当前实物板默认值。
- 不在 first-byte失败时通过替换 OpenSBI、ESOS、U-Boot或分区表扩大修改。

## 真板分层 Gate 与停止条件

| Gate | 最小迁移/操作 | 成功证据 | 停止条件 |
| --- | --- | --- | --- |
| 0 出厂基线 | 无代码迁移 | 同一串口可重复进入 U-Boot/原系统；记录板型、固件、DRAM、DTB、介质 | 原系统不可恢复或串口不可信 |
| 1 镜像 | FIT 构建和只读检查 | kernel/DTB节点、load/entry、展开大小和地址区间无冲突 | DTB未映射到实物；地址/命令未知 |
| 2 RAM跳转 | 仅 stage + `bootm`，不写盘 | U-Boot解析FIT并跳转；复位可回原系统 | 设备不唯一、命令语法或恢复路径未证 |
| 3 首字节 | K3 axplat + polling UART | 打印板名、boot hart、DTB model/compatible、memory/MMIO | 无输出时只查 entry/栈/页表/UART，不启 IRQ/SMP |
| 4 平台事实 | DTB解析/平台配置 | hart、内存、reserved、timer、AIA、UART逐项一致 | 任一关键资源全零、全一或冲突 |
| 5 中断 | APLIC/IMSIC | claim、handler、device clear、complete可重复 | 单次中断、storm、EID/source错配 |
| 6 UART/SMP | PXA IRQ + copier + HSM/IPI | RX/TX/readiness/drain和跨 hart wake分层通过 | 缺 SBI扩展、hart集合不明、第二 endpoint |
| 7 GMAC寄存器/链路 | pinctrl/clock/reset/MDIO | MAC寄存器可访问、PHY/link可重复 | 寄存器全零/全一、clock/reset/PHY未知 |
| 8 GMAC轮询数据面 | DMA ring/cache/address | descriptor和buffer ownership、ARP/ICMP逐层通过 | DMA地址/coherency/IOMMU未闭合 |
| 9 异步与恢复 | 接入既有 queue/recovery契约 | IRQ wake、backpressure、completion、reset/link恢复 | try-lock后无确定推进、永久Pending、stale completion |
| 10 持久/AMP | 单组件独立 change | 备份、恢复、组件兼容和写后分层Gate均闭合 | 任一介质、分区、包或恢复事实未知 |

Gate 0–3 是首个真板 change 的合理上限。AIA、SMP、UART IRQ、GMAC和持久部署应拆开，否则第一次失败无法区分镜像、页表、console、中断和设备问题。

## 测试、验证入口与影响面

本次只执行非破坏性检查：

- 完整读取 K3 启动、镜像、RAM boot、恢复、AIA、GMAC、DMA和既有复用分析；
- 用 `file`、`ls -lh`、`dumpimage -l` 检查 Buildroot包中的固件/FIT；
- 用 `mdir` 检查 `bootfs.img`，确认 Linux Image、initramfs和多个 CoM260 DTB候选；
- 用 `debugfs` 只读检查 `rootfs.ext4`，确认它是完整 Linux用户空间而非 StarryOS必需载荷；
- 用 `fdtget` 读取六个 CoM260候选的 model/compatible/bootargs；
- 读取 `fastboot.yaml`、四种 partition JSON、`genimage.cfg` 和 `env.bin` 可见字符串；
- 对照 StarryOS当前 Cargo features、Makefile和 D1 platform crate，确认没有 K3 platform接入；
- 核对 tgoskits 的 AIA、PXA UART、pinctrl、GMAC代码位置和规模，以及 OpenSBI fork 的 K3-specific实现。
- 统计 Rt-Async-AMP 根仓、rt-async 和 tgoskits 作者提交、当前源码规模、测试入口与 gitlink 固定点；
- 对照作者的 FIT、GMAC 和 AMP IPC 复盘，区分最终方案、曾被证伪的中间方案和仍未关闭的未知项。

未执行 StarryOS K3构建、Rt-Async-AMP `cargo xtask build`、参考仓库测试、FIT生成、Fastboot、U-Boot命令、复位、刷写、真板启动、IRQ、UART、GMAC、DMA或 workload。当前参考目录的 gitlink 没有按声明路径完整初始化，不能用它产生可复现构建；作者报告中的 DHCP、ping、RPC 和延迟结果属于第三方历史证据，不构成本项目运行通过。

后续实施会影响：根 Cargo feature、Makefile/platform解析、新 K3 `axplat` crate、早期页表和 linker地址、console、time/power/irq、DTB/FIT工具、UART adapter，以及更晚的 K3 GMAC backend。MS08当前 QEMU代码不应为迁就 K3而修改平台常量。

## 关键接口和数据结构

| 当前 StarryOS接口 | K3输入 | 适配边界 |
| --- | --- | --- |
| `axplat::init::InitIf` | boot hart、DTB、per-hart AIA/time init | primary/secondary分层，不默认 hart0 |
| `axplat::mem::MemIf` | DRAM、reserved-memory、MMIO、CPU PA/VA | 不由通用 DTS覆盖目标板现场 |
| `axplat::console::ConsoleIf` | PXA UART base/stride/width/LSR/THR | first-byte polling与 async UART分开 |
| `axplat::power::PowerIf` | SBI HSM/SRST、online hart集合 | HSM不可用时 fail closed，不静默声称 SMP |
| `axplat::time::TimeIf` | timebase-frequency、SBI TIME | 不用 QEMU/D1频率 |
| `axplat::irq::IrqIf` | CLINT local IRQ、APLIC source、IMSIC EID | 不能复用 PLIC context公式 |
| `uart_16550::UartPort`及 copier | K3 PXA UART硬件操作与 IRQ | 保留现有 SPSC、waker、short write和 drain |
| `NetQueueControl` / `NetTxQueue` | DWMAC descriptor、IRQ和回收 | transport-neutral owner/recheck/recovery契约保持 |

## 关键文件

### 当前 StarryOS

- [`Cargo.toml`](../../Cargo.toml)：现有平台 feature 和可选 platform依赖，没有 K3。
- [`Makefile`](../../Makefile)：QEMU、VF2、D1构建入口；需要独立 K3目标和 FIT封装。
- [`crates/axplat-riscv64-lichee-d1/`](../../crates/axplat-riscv64-lichee-d1/)：当前本地 platform crate的接口形状，不提供 K3硬件事实。
- [`kernel/src/drivers/uart_init.rs`](../../kernel/src/drivers/uart_init.rs)：现有 async UART adapter、copier与 IRQ接入边界。
- [`crates/axdriver_net/src/lib.rs`](../../crates/axdriver_net/src/lib.rs)：网络 queue、TX和 transport-neutral契约。

### K3 参考仓库

- [`Buildroot-K3-v1.0.7`](../../../k3/others/Buildroot-K3-v1.0.7/)：预构建恢复/发布包，不是 Buildroot源码树。
- [`com260-boot-chain.md`](../../../k3/docs/boot/com260-boot-chain.md)：BootROM到 payload的阶段和介质边界。
- [`k3-image-build-and-artifacts.md`](../../../k3/docs/boot/k3-image-build-and-artifacts.md)：镜像生产者、消费者和产物关系。
- [`k3-ram-boot-and-fastboot.md`](../../../k3/docs/boot/k3-ram-boot-and-fastboot.md)：RAM stage、地址检查和失败分层。
- [`k3-flashing-and-recovery.md`](../../../k3/docs/boot/k3-flashing-and-recovery.md)：持久写入与恢复门。
- [`k3-interrupt-and-time.md`](../../../k3/docs/interrupts/k3-interrupt-and-time.md)：AP APLIC/IMSIC/CLINT拓扑。
- [`com260-uart.md`](../../../k3/docs/serial/com260-uart.md)：PXA UART、clock/pinctrl和板级串口事实。
- [`com260-gmac-phy.md`](../../../k3/docs/network/com260-gmac-phy.md)：GMAC/PHY/RGMII输入与变体边界。
- [`k3-gmac-dma-irq.md`](../../../k3/docs/network/k3-gmac-dma-irq.md)：descriptor、DMA、IRQ和恢复风险。
- [`k3-cache-pma-address-translation.md`](../../../k3/docs/dma/k3-cache-pma-address-translation.md)：cache/PMA/PBMT/IOMMU边界。

### 固定第三方参考实现

- [`Rt-Async-AMP README`](../../../k3/others/Rt-Async-AMP/README.md)：项目组成、环境模型、构建产物、K3 手工引导与已知边界。
- [`k3-com260.toml`](../../../k3/others/Rt-Async-AMP/envs/k3-com260.toml)：AP/RP 分产物和 K3 环境配置。
- [`xtask build.rs`](../../../k3/others/Rt-Async-AMP/xtask/src/build.rs)：OpenSBI、ESOS、StarryOS FIT 和用户工具的构建编排参考。
- [`spacemitk3-com260kit.its`](../../../k3/others/Rt-Async-AMP/tgoskits/os/StarryOS/configs/board/spacemitk3-com260kit.its)：AP kernel + DTB FIT参考。
- [`spacemit-k3-com260-ifx.dts`](../../../k3/others/Rt-Async-AMP/tgoskits/os/StarryOS/configs/board/spacemit-k3-com260-ifx.dts)：固定 IFX聚合 DTS；仅作来源导航。
- [`imsic_aplic.rs`](../../../k3/others/Rt-Async-AMP/tgoskits/platforms/somehal/src/arch/riscv64/imsic_aplic.rs)：AIA domain和 `stopei`处理参考。
- [`pxa_uart`](../../../k3/others/Rt-Async-AMP/tgoskits/drivers/serial/some-serial/src/pxa_uart/)：K3适用的 PXA UART硬件/IRQ机制参考。
- [`k3_gmac`](../../../k3/others/Rt-Async-AMP/tgoskits/drivers/ax-driver/src/net/k3_gmac/)：DWMAC5/PHY/DMA/IRQ参考。
- [`spacemit_k3.c`](../../../k3/others/Rt-Async-AMP/opensbi-k3/opensbi-spacemit/platform/generic/spacemit/spacemit_k3.c)：K3 OpenSBI override与实验性 AMP PMA修改。
- [`FIT 烧录复盘`](../../../k3/others/Rt-Async-AMP/rt-async/RT-Async/blogs/技术报告/2026-08-01-K3-COM260Kit-FIT烧录与可编辑设备树.md)：FIT 重叠失败、地址分离和 rootfs bootargs 历史。
- [`GMAC 调试复盘`](../../../k3/others/Rt-Async-AMP/rt-async/RT-Async/blogs/技术报告/2026-08-14-K3-GMAC驱动调试教训.md)：MTL/TSF 常量错误、DMA 状态诊断和真板 DHCP/ping 记录。
- [`AMP IPC 挂死复盘`](../../../k3/others/Rt-Async-AMP/rt-async/RT-Async/blogs/技术报告/2026-08-16-AMP-IPC挂死三层排查.md)：PBMT/cache、`stopei` 与通知协议三层故障；中间 CBO 方案不是最终 PMA 方案。

## 对后续规划的建议

第一个真板 change 只承诺“确认目标板和 DTB → 构建/检查 AP FIT → RAM临时引导 → polling UART首字节 → 复位恢复原系统”。这一步不需要迁移 Buildroot、OpenSBI、ESOS或 rootfs。

该 change 应把操作经验作为主线：先建立出厂恢复基线，再离线检查 FIT 与地址，随后完成一次 RAM stage/bootm 和 first-byte；源码迁移只服务这些 Gate。不要以“先把参考仓库代码搬过来”作为起点，也不要把 `mtd erase/write` 或 Buildroot `multi_flash` 纳入首次启动。

首字节 accepted 后，再按 platform facts/AIA、UART IRQ/SMP、GMAC寄存器与链路、轮询 DMA、异步数据面、恢复和持久部署依次建立 change。若首轮就把固件、AIA、SMP、UART IRQ和 GMAC并入同一 change，任一无输出或 fault 都无法保持可诊断边界。

可作为项目模型候选、但本次不自动登记的约束：K3 AP首轮 bring-up必须把板载 FSBL、ESOS、OpenSBI和 U-Boot视为外部稳定基线；只有已证明的 SBI/PMA/AMP缺口才能授权替换其中一个组件。
