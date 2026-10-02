## Context

见 `proposal.md` 的动机和获批范围。StarryOS `k3` @ `160d7967` 仍无 K3 feature/axplat/build target。`src/main.rs` 仅显式链接 D1/VF2；`kernel/src/platform/mod.rs::descriptor()` 在非 D1 时选 QEMU；`kernel/src/entry.rs::init()` 与 `kernel/src/drivers/mod.rs` 对非 D1 编入 QEMU async UART/VirtIO 路径。`make/build.mk` 的通用 `OUT_UIMG` 是 legacy kernel uImage，不是带 DTB 的 FIT。D1 `axplat` 展示 `a0` hartid、`a1` FDT、栈/页表和 `axplat::call_main` 的接口形状，但其低地址映射、T-Head PTE 位、PLIC 和串口参数不适用于 K3。

`kernel/src/platform/early_console.rs` 已有 U8 与 stride-4/U32 polling 实现；`tests/early-console-host-harness.rs` 的 6 项 host 测试在前轮实际运行通过（`rustc --edition=2024 --test ...` 与测试二进制均 exit 0）。这只覆盖字符转换与寄存器偏移，不覆盖真板 clock/pinmux、MMIO、超时或首字节。当前环境没有 `/dev/serial/by-id`，尚未取得当前板的修订、运行 FDT、串口日志、U-Boot 内存图及 RJ45/link 观察。参考仓库 G7 仍未将产品修订唯一映射到顶层 DTS；其 Buildroot 包中的 Kit V02 DTB 含 UART0 `0xd4017000` 和 Ethernet `0xcac82000`/PHY@1，历史真机报告也记录了这两个 MMIO 与 Linux `carrier=1`，均可作为优先核对的基线，不自动证明当前板固件选择了该 DTB。历史报告的 UART IRQ `47` 与静态 `k3.dtsi` 中 UART0 source `42` 编号域未厘清，MS09 不据此启中断。

本 change 对应全局任务 T13–T15，现分为官方/第三方资料基线与 Linux 采集工具、当前板实跑与安全 RAM 边界、首字节、MAC 寄存器、PHY/link 五个可诊断阶段。首阶段只增加独立的用户态诊断程序和直接测试，不改内核；后续阶段消费经过当前板核对的参数，不在本设计中填入示例加载地址。

## Goals / Non-Goals

**Goals:** 把 `k3` 官方来源资料及 `others/` 真板成功经验纳入明确适用范围；提供可在官方 Linux 运行的只读 CoM260 信息采集程序，取得运行 FDT/设备状态并标明不可从 Linux 判定的字段；建立当前板资源表和 RAM 安全区间；K3 构建独立选路；用已有 bootloader 临时交接后取得可重复首字节；在无 IRQ/DMA 条件下验证 MAC 身份/状态与 PHY/link。每个失败能定位到资料、采集、镜像/入口、UART、MMIO、clock/reset 或 PHY/对端。

**Non-Goals:** 采集程序不改网络配置、设备树、sysfs 或 MMIO，不成为构建/运行身份型证据工具；不替换板载固件、写持久介质、实现 AIA/GMAC IRQ、DMA ring、packet I/O、异步 UART/NIC、SMP 或性能资格；不把历史 Linux root shell 当成 StarryOS 验证。

## Decisions

### D0 — 分层采信参考资料，并在官方 Linux 上只读采集

- **决定：**`k3/docs/reference/source-coverage.md` 中已直接观察、可追溯到厂商文档或官方 Linux 源码且目标范围匹配的字段默认作为参考基线；`partially-observed` 页面不补写未取得的正文。`others/Rt-Async-AMP/` 和历史真机报告作为已运行路径及故障经验，不提升为厂商规范。新增独立 C 用户态采集程序，复用本仓已有 `BENCH_CC` 静态 RISC-V Linux 交叉编译惯例、标准 libc/文件系统接口和 host C 测试，不引入依赖或独立判定器。程序从 Linux 只读 FDT 视图和 sysfs/procfs 直接输出字段及缺失原因；不访问 `/dev/mem`，不写 sysfs、设备、固件或网络设置。
- **原因：**本仓已有 `tests/*.c` 板上诊断程序及 `Makefile` 交叉编译入口，`riscv64-linux-musl-gcc`、`riscv64-linux-gnu-gcc`、`cc`、`dtc` 在当前 host 可用；当前却没有 CoM260 FDT/设备状态采集程序。官方静态值与运行实例间的剩余差距可由板载 Linux 缩小，而 U-Boot relocation 等 Linux 不可见事实仍须单独观察。
- **影响：**首个 Iteration 形成参考字段/来源边界及能构建、测试的程序；第二个 Iteration 才在当前 Kit 上运行并核对。程序输出可直接阅读，不含 revision pin、run-id、日志 Hash 或资格判定协议；运行缺项只阻塞依赖它的后续操作。
- **替代：**仅用手工汇总、不写用户要求的板上代码；或让 Linux 程序读写任意 MMIO/配置以“验证”资源。前者不满足范围，后者越过安全边界，均拒绝。

### D1 — 当前板运行资料决定具体实例和安全操作

- **决定：**以官方来源资料作为默认参考，再由实物标识、板载 Linux 程序输出和当前串口/固件信息确定实际资源实例。缺少或冲突的关键字段保持未选定；程序不能替代 bootloader 内存自占及恢复观察。事实阶段不增加 K3 内核常量。
- **原因：**参考仓库 `docs/reference/known-gaps.md` G7 open；Kit V02 名称、用户指南 v03 产品版本与 IFX 第三方 DTS 不构成当前板唯一映射；Linux 的运行 DTB 与设备绑定能给出更窄的实例事实。
- **影响：**板级修订、FDT model/compatible/chosen/memory/reserved/CPU/UART/MAC/MDIO/PHY/clock/reset、固件 handoff 和实际启动介质必须先闭合。取不到足以选择 MMIO 和安全 RAM 区间的事实时，后续阶段保持未展开。
- **替代：**直接按 `k3_com260_kit_v02.dts` 文件名确定本板，或把第三方启动/寄存器常量当成无须核对的值；因会把适用范围与实例混淆而拒绝。

### D2 — 保留板载固件，先用 RAM 中的 AP payload

- **决定：**按获批条件保留 FSBL/ESOS/OpenSBI/U-Boot，离线构建适合当前板的 kernel+FDT FIT，经当前板已验证的 RAM stage/boot 命令临时交接；在任何跳转前逐段核对 stage、FIT 展开、kernel/DTB、U-Boot 自占和 reserved-memory 区间。操作失败或取消时只采用本板已验证的返回/复位路径。
- **原因：**首字节只需 AP handoff；持久固件更新会增加不可恢复风险，第三方 FIT 地址曾因包与展开区重叠失败。
- **影响：**`make/build.mk` 的 legacy uImage 不冒充 FIT；K3 构建需最小 ITS/打包入口，原有平台镜像规则保持不变。板上命令语法和地址由事实阶段确定，不在计划中固定第三方示例。
- **替代：**全盘刷写、替换 OpenSBI 或依赖 Linux rootfs 启动；不服务首字节且扩大故障域，拒绝。

### D3 — 单 hart K3 平台独立接入；AIA 延后

- **决定：**后续阶段新增 K3 专属 `axplat` 配置/入口和根 feature，显式排除 QEMU/D1/VF2 组合。K3 入口仅引导当前固件确认可用的 boot hart；保存/传递 hartid 与 FDT，使用按当前内存图设计的早期页表。MS09 对 `IrqIf` 只满足链接所需的 fail-closed stub，不启用 APLIC/IMSIC；平台 descriptor 不以虚构 PLIC 基址代表 K3。
- **原因：**现有非 D1 路径会误进 QEMU；D1 的 PLIC/页表是接口样例而非 K3 参数。中断 delivery 属 MS10，真板多 hart 属 MS14。
- **影响：**需核对 `axplat` 的 Mem/Console/Init/Time/Power 接口、kernel entry/driver cfg 和平台配置选择；原有 QEMU/D1/VF2 路径做兼容回归。若当前固件不能提供预期 S-mode handoff，必须返回 Plan 而非复制 D1 `_start`。
- **替代：**在 QEMU descriptor 上覆盖常量或直接 vendor `somehal`；前者会污染现有路径，后者接口与本仓不同，拒绝。

### D4 — 启动、控制器和链路逐级验证

- **决定：**先以独立 polling 输出确认内核入口和最小平台，再读取已核实 MAC MMIO 的只读身份/状态；只有资源与 handoff 证实需要时，才执行最小 clock/reset 处理，继而读 MDIO/PHY 与 link。MAC 全零/全一、异常访问、PHY 不可读、link down 各自保持独立结果。每个等待/轮询都有明确阶段和有界停止，错误不得静默推进。
- **原因：**串口首字节、寄存器可达、PHY 可达和外部链路是四个不同故障域；link up 也不证明 DMA/IRQ。
- **影响：**可能需要 K3 专属有界早期探针，而不是改变 `EarlyConsole::putchar` 的通用阻塞签名。若 T13 表明 MAC 在 U-Boot handoff 时处于 reset/clock-off，必须在展开 MAC 阶段前修订该阶段契约，不允许 Act 猜测写序。
- **替代：**一次性移植完整 DWMAC/异步队列并用 ping 判定；会遮蔽首个故障点，拒绝。

### D5 — 直接判定与最小持久化

- **决定：**host 使用项目现有测试、`cargo`/`make`、`mkimage`/`dumpimage`、ELF 工具直接判定构建和镜像；真板只用可见串口阶段、寄存器/PHY 结果与复位观察判定。Act Response 记录命令、≤20 行决定性输出和结果；默认不建 Evidence 目录。
- **原因：**这些结果足以支持当前阶段判断，额外日志认证、运行身份或自定义判定脚本不证明目标行为。
- **影响：**首次现场数据若不能低成本重取、且摘要会丢失决定性结构，后续 Plan 再按公共 Evidence 准入规则修订；不能为当前未知环境预建日志仓或校验器。
- **替代：**新增资格封装器、Hash/manifest/时间审计；项目规则禁止，拒绝。

## Risks / Trade-offs

- [当前环境无串口设备或用户尚未提供板上资料] → 第一阶段仍可完成官方来源梳理、只读采集程序与 host 测试；第二阶段的板上实跑及安全 RAM 判定等待现场，不制造目标板数值。
- [官方资料只是入口或部分正文、第三方成功路径含刷写操作] → 按字段标注观察范围；只借鉴可撤销 AP RAM 路线，第三方固件/RP 刷写序列不进入 MS09 操作契约。
- [固件版本/DTB 与参考不同] → 优先按当前板运行 FDT 和实物修订裁决；冲突影响地址或接口时返回 Plan，禁止试错写寄存器。
- [RAM/FIT 区间或复位恢复未证] → 不执行 RAM stage/boot；先核实内存、U-Boot 自占和恢复路径，持久分区始终不写。
- [bootloader 未保留 UART 或 MAC clock] → 串口与 MAC 阶段分别报告，不以全零/全一推断控制器坏；取得 provider/写序依据后再规划最小恢复。
- [当前 early console 的 polling 无界] → K3 首字节探针采用有界状态观察，失败由直接的串口/固件阶段与主机超时裁决；不改变 QEMU/D1 现有输出语义。
- [R40 真板阶梯包含 PLIC 等旧平台步骤] → 仅采信其单变量与首字节原则；K3 APLIC/IMSIC 不在 MS09 实现。

## Migration Plan

1. 梳理可追溯的官方字段与第三方成功路径；实现只读 Linux 板级采集程序并在 host 测试、交叉编译。
2. 在当前 Kit 的官方 Linux 上运行程序，配合实物标识、串口和 U-Boot 只读命令核对实际 FDT、资源、内存占用与恢复路径；未闭合安全区间不写内核硬件常量、不执行 RAM stage。
3. 建立 K3 单 hart构建与临时镜像；host 检查 ELF/FIT 区间与现有平台编译回归；通过板载 U-Boot 可撤销 RAM 启动，重复观察第一字节与复位回原系统。
4. 分别观察 MAC 身份/状态、PHY/MDIO 和 link；任一层失败只回退到上一稳定基线，不展开 IRQ/DMA。

## Execution Boundary

首次 Cycle 只覆盖参考资料映射和 Linux 采集程序的 host 侧实现/测试，不需要本机 `/dev/serial/by-id`，不要求当前板数值。板上运行、固件内存图和恢复结果是第二个 Iteration 的能力边界；第一轮不能以参考资料、历史 Linux 输出或尚未运行的程序结果宣称当前板 RAM 路线已安全。用户已批准详细计划，首次 Cycle Gate 2 通过；Act 须由用户另行指令启动。
