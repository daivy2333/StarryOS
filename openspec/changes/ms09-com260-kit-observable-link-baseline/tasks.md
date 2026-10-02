# MS09 CoM260 Kit 可观测链路任务

任务对应全局 T13–T15，状态以逐项勾选为准。当前展开至 Iteration 001；参考资料、板载 Linux 实测和 StarryOS 真板运行分别验收，不互相冒充。后继 Iteration 的具体板上参数由已接受的前轮事实写入各自 Cycle。

## 1. 官方参考基线与 Linux 只读采集程序（全局 T13 前半）

- [x] 1.1 按 `k3/docs/reference/source-coverage.md` 的来源与访问状态，核对 CoM260 启动、UART、内存、MAC/PHY 的可采信官方字段；整理 `others/Rt-Async-AMP/README.md` 的已运行 AP/FIT 路线和历史 Kit Linux 报告，区分成功经验与刷写/非目标板参数。验收为 Act Response 给出字段、来源、适用范围、已知冲突及待当前板核对项；不因无串口而丢弃已观察的官方结论。
- [x] 1.2 在 `tests/ms09_board_facts.c` 实现最小 Linux 用户态只读采集程序，并以 `tests/ms09_board_facts_test.c` 的原生 C 测试和根 `Makefile` 的现有交叉编译惯例验证。输出当前运行 FDT 的 model/compatible/chosen、memory/reserved-memory、Linux 可见 CPU、UART/MAC/MDIO/PHY 及资源关系、可取得的驱动绑定与 link 状态；缺失/权限错误/仅固件可见分别报告。先建立失败测试，再使 host 测试与 RISC-V Linux 静态编译通过；不写 sysfs、MMIO、网络配置或持久介质，不新增资格/身份型证据协议。

## 2. 当前板运行核对与安全 RAM 边界（全局 T13 后半）

- [x] 2.1 在当前 CoM260 Kit 官方 Linux 上运行 1.2 的程序，结合实物标识和串口启动信息核对板卡修订、启动介质、固件、运行 FDT、boot/可用 hart、UART/MAC/PHY 及 clock/reset/pinctrl；直接记录输出与官方基线的相符、差异和 Linux 不可见项，缺关键资源时阻止对应后端。（2026-10-01 完成，对照表见 Iteration 001 Act Response）
- [x] 2.2 用当前板 U-Boot/固件的只读信息和运行 FDT 核对 DRAM、reserved-memory、U-Boot 自占、payload/FDT 候选 RAM 区间及正常复位回原系统的结果；验收为不重叠与恢复路径可直接观察，未核实不执行 RAM stage。（2026-10-01 完成；复位观察与 RAM 候选推导见 Iteration 001 Act Response；U-Boot 控制台只读命令采集经用户 2026-10-01 豁免，原话与风险已保留）
- [x] 2.3 汇合官方资料、Linux 采集和 bootloader 观察，为后续 StarryOS 确定 UART 基址/stride/访问宽度/clock/pinmux、MAC compatible/reg/clock/reset、MDIO/PHY/RJ45 与安全 RAM 区间；保留 UART `42`/`47` 编号域等尚未裁决项，IRQ/DMA/cache/IOMMU 只记边界，不作为 MS09 数据面资格。（2026-10-01 完成：42/47 按编号域差异裁决为 DT hwirq 42；未裁决项仅剩 U-Boot 自占边界与控制台手法/stage 命令，均为用户豁免并标注 Iteration 002 现场解决）

## 3. K3 单 hart 构建与可重复首字节（全局 T14 前半）

- [x] 3.1 在根 `Cargo.toml`、`src/main.rs`、`kernel/Cargo.toml`、`kernel/src/platform/`、`kernel/src/entry.rs` 与新 K3 `axplat` 中建立独立 feature、平台资源和最小单 hart 入口；先有 K3 选择失败测试，再以 K3/QEMU/D1/VF2 构建与平台选择检查证明正确分流和原行为不回归。（2026-10-01 完成：k3 构建 exit 0 + ELF 符号/常量/0 virtio 符号；qemu/lichee 回归 exit 0；vf2 维持 pre-existing 失败且签名不变；互斥守卫经 cargo check 实际触发）
- [x] 3.2 在 K3 专属镜像入口中使用 2.2 已核实的内存区间封装 kernel+FDT 临时 FIT；先有 FIT 结构/区间失败见证，再用 `mkimage`/`dumpimage` 与 ELF 原生命令直接证明配置、load/entry、FDT 和不重叠，不写持久介质。（2026-10-01 完成：`make k3` 产出 276,000 B FIT；kernel load/entry `0x180000000`=FDT load 间隔 16 MiB，全部落在裁决区间；ELF e_entry/axconfig/ITS 三方一致；DTB 提取自参考仓库 bootfs 入仓 `tools/k3_com260_ifx.dtb`，model 与板上事实一致）
- [x] 3.3 在 K3 最小入口和 polling UART 路径输出阶段化首字节，并用当前板已验证的 RAM 启动/恢复步骤重复观察；先证明旧路径缺失或硬件静默，再以真串口输出、复位回原系统和 early-console host 测试证明结果，不启 IRQ/DMA/async/rootfs。（2026-10-01 完成：第一轮定位 page-alloc-4g 容量 panic 并以 64g 修复；第二轮 `setenv fdt_high` 后完整 `[k3:pt-mmu]`+stage=1/2/3 序列 + 用户确认 `reset` 回原 Linux；第三轮重复循环经用户豁免跳过（原话与风险记录于 Iteration 002 Act Response）；Evidence `board-firstbyte.txt` 通过条件满足）

## 4. 目标 MAC 寄存器基线（全局 T14 后半）

- [ ] 4.1 在 K3 最小平台探针中按 2.3 已核实的 MMIO/clock handoff 只读目标 MAC 身份与状态；先有异常/全零/全一失败见证，再以重复真板读值和可解释的控制器身份验收，失败停在 MMIO/clock/reset 诊断边界。

## 5. PHY 与物理 link 基线（全局 T15）

- [ ] 5.1 在 K3 板级控制/MDIO 最小路径中依据 2.3 的 handoff 保留或恢复必要 clock/reset，并读取实际 PHY；先见证不可读/错误状态，再以 PHY 可读、身份可解释及错误分层验收，不进入 DMA/IRQ。
- [ ] 5.2 在当前 Kit 已确认的 RJ45 与对端连接条件下观察 link 协商和断开状态；以重复 link up/down 及与无对端、PHY 不可读相区分的结果验收，保持 QEMU/D1/VF2 兼容回归通过。

## Iteration Plan

### Iteration 000: 参考基线与可运行采集程序

- Tasks: 1.1–1.2
- Depends on: MS08 accepted；本机可读取 `k3` 仓库并使用已有 host C/RISC-V Linux 编译器；不依赖当前板在线。
- Stable baseline: 官方来源字段、第三方成功经验和冲突边界已核对；只读 Linux 采集程序有 host 测试和可传板的静态 RISC-V Linux 产物。
- Verification boundary: 来源/适用范围能直接回到仓库原文；原生 C 测试通过、交叉编译 exit 0、产物为 RISC-V Linux ELF；不把 host 运行计为板上采集。
- Diagnostic boundary: 失败限于来源归属、FDT 属性解析、Linux 文件读取/错误分类或交叉编译，不涉及固件和 StarryOS 内核。
- Non-goals: 当前板运行、RAM 区间裁决、K3 内核构建和寄存器读写。

### Iteration 001: 当前板资源与可恢复 RAM 边界

- Tasks: 2.1–2.3
- Depends on: Iteration 000 accepted。
- Stable baseline: 当前修订、运行 FDT/设备、固件和内存图相互核对；安全 RAM 候选与恢复路径有现场依据。
- Verification boundary: 程序在板上真实运行并给出字段/缺项；U-Boot 自占和普通复位单独观察；冲突不由静态候选代填。
- Diagnostic boundary: 失败限于 Linux/固件视图、实物映射、内存与恢复，不涉及 StarryOS 产品代码。
- Non-goals: RAM 跳转、MAC 寄存器、IRQ/DMA。

### Iteration 002: K3 临时启动与首字节

- Tasks: 3.1–3.3
- Depends on: Iteration 001 accepted。
- Stable baseline: K3 单 hart 独立构建、FIT 不重叠、RAM 启动可重复输出首字节且复位回原系统。
- Verification boundary: host 构建/FIT/ELF 与真板串口、复位结果都通过；既有平台构建保持。
- Diagnostic boundary: 失败限于 build/link、FIT/地址、固件跳转、早期页表或 polling UART。
- Non-goals: MAC 探针、设备 IRQ、DMA、异步设备。

### Iteration 003: MAC 只读寄存器基线

- Tasks: 4.1
- Depends on: Iteration 002 accepted。
- Stable baseline: 目标 MAC 寄存器非全零/全一，身份和状态在重复启动下可解释。
- Verification boundary: 真板控制器读值直接判定；异常访问与 clock/reset handoff 问题分层。
- Diagnostic boundary: 失败限于已确认 MMIO 映射、控制器身份或 handoff 状态。
- Non-goals: MDIO/link、MAC IRQ、DMA ring。

### Iteration 004: PHY 与 link

- Tasks: 5.1–5.2
- Depends on: Iteration 003 accepted。
- Stable baseline: PHY/MDIO 可读且物理 link 在相同对端条件下可重复，后端候选有当前板依据。
- Verification boundary: PHY 可达、link up/down、无对端与错误状态分开观察。
- Diagnostic boundary: 失败限于 clock/reset、MDIO/PHY、RGMII 或对端连接。
- Non-goals: packet I/O、IRQ、DMA、异步网络和恢复。

## 平衡审计与当前边界

Iteration 000 的来源核对与采集程序共同形成“可在当前板执行的调查入口”，host 侧可独立验证；单独只写来源表或只编译空程序都不是稳定成果。Iteration 001 才形成当前板事实与安全边界；Iteration 002–004 依次分离启动、MAC MMIO 和外部链路故障域。各任务只属于一个 Iteration，不把当前板无法提供的值提前固化。若 T13 证明 MAC handoff 时 clock/reset 尚不可用，先由 Plan 调整 003/004 的依赖和任务契约，不让 Act 在 003 猜测写序。

当前边界：Iteration 000–002 均已 accepted（1.1–3.3 闭合；两项用户豁免——U-Boot 控制台命令采集、第三轮重复循环——原话与风险保留于对应 Act Response；vf2 构建入口 pre-existing 失败为 Issue 候选未落账）。Iteration 003 Cycle `000-initial` Act `blocked` 于 clock/reset handoff（板上三轮定位 GMAC 总线时钟门控挂死），Plan Review `rework-required`（2026-10-01）；Cycle `001-rework`（repair item 4.1-R1：APMU `0x3ec` 最小 clock 使能两步写序 + stage=4/5 观测扩展，依据 R69 + 板 DTB + 上游标注）已创建，`Plan Context` 为 `draft` 待用户批准后置 `ready`，再由用户指令 `openspec-act` 执行；板上两轮启动按 R68 Runbook 执行。其余 Iteration 不创建目录。
