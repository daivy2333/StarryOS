# Iteration 002 / Cycle 000: K3 临时启动与首字节

## Plan Context

- Status: ready
- Cycle Type: initial
- Gate 2: PASS — 用户 2026-10-01 批准实施（原话「开始实施，到真板操作给我手动进行就好，先执行你能做的」）；与契约一致，3.3 板上操作由操作者（用户）执行，Act 先行 host 侧 3.1/3.2 并准备 3.3 指令包

**Cycle Scope**

- Change tasks: 3.1–3.3（全局 T14 前半 + T14 后半的构建侧前置）
- Acceptance gaps: None
- Repair items: None
- Inherited scope: Iteration 001 accepted 的裁决表（UART `0xd4017000`/stride=4/U32、clock/pinctrl 事实、MAC 实例、安全 RAM 候选 `0x160000000`–`0x2f0000000`（payload ≥`0x180000000`）、恢复路径=复位回原系统、boot hart 0）；design D2（保留板载固件、RAM/FIT 临时路线）与 D3（单 hart 独立平台、AIA 延后、IrqIf fail-closed stub）。
- Excluded scope: tasks 4.1–5.2（MAC 寄存器读取、PHY/link）、AIA/APLIC/IMSIC 实现、DMA、异步 UART/NIC、rootfs/用户态、SMP、性能、持久刷写、采集程序与既有 QEMU/D1/VF2 语义修改。

**Objective**

K3 作为独立平台构建分流成功（单 hart、fail-closed 中断 stub、polling UART），kernel+FDT 以临时 FIT 封装且地址全部落在已裁决安全区间，经板载 U-Boot 可撤销 RAM 启动在真串口上重复观察阶段化首字节，复位回原系统；既有平台构建不回归。

**Background**

Iteration 001 已闭合当前板事实与安全 RAM 边界。本 Iteration 首次触碰内核代码面（此前两轮只交付用户态程序与现场记录），引入 K3 平台接入的最小内核路径。板上步骤（3.3）仍由操作者执行；3.1/3.2 为 host 侧构建与打包。U-Boot 控制台进入手法为 Iteration 001 遗留未裁决项，在本 Iteration 现场解决。

**Investigation Facts**

- Current Baseline: StarryOS `k3` @ `160d7967` + 未提交 MS09 文件（`Makefile` +8、两个 `tests/ms09` 源、change 目录、evidence、runbook/R67）。产品内核代码自 MS08 以来未动。`/usr/bin/mkimage`、`/usr/bin/dumpimage`、`dtc` 可用。Iteration 001 accepted Review：候选区间不重叠经全集复算；裁决表可回溯。
- Current-State Evidence（本轮直接读取的代码表面）：
  - 构建：`make ARCH=riscv64 APP_FEATURES=<f> MYPLAT=<axplat-crate> PLAT_CONFIG=<crate>/axconfig.toml MEM=<n> BUS=mmio DWARF=n build` 是板级构建既有模式（Makefile `lichee:`/`vf2:` 别名，行 301–321）；默认 `export APP_FEATURES := qemu`（行 19）。产物经 `make/build.mk`：`OUT_BIN=objcopy -O binary`，`OUT_UIMG` 为 legacy uImage（`mkimage -T kernel -C none -a axconfig-gen … kernel-base-paddr`，行 80–86），仓库无 FIT 规则。
  - feature 装配：根 `Cargo.toml [features]`——板级 feature 拉起 `dep:axplat-<plat>` + `axfeat/bus-mmio` + `starry-kernel/<f>`（`lichee-d1` 形状，行 71–78）；`smp` 传递 `axplat-riscv64-visionfive2?/smp`。`src/main.rs` 以 `#[cfg(feature)] extern crate axplat_*` 装配平台 crate，`main()` → `starry_kernel::entry::init`。
  - 平台选择：`kernel/src/platform/mod.rs::descriptor()`——`cfg(lichee-d1) → LICHEE_D1`，`cfg(not(lichee-d1)) → QEMU_VIRT`；已有 `compile_error!` 禁止 `qemu`+lichee-d1 组合。K3 不接入时，非 D1 构建落入 QEMU descriptor/驱动路径。
  - 内核入口：`kernel/src/entry.rs::init` 的 cfg 矩阵——`lichee-d1`(非 async) → `platform::smoke::run_lichee_d1_smoke()`（最小 smoke 路径样例）；`cfg(not(lichee-d1))` QEMU 路径 import `ASYNC_TTY/uart_init/load_user_app` 等（K3 不得落入）；`lichee-d1-async-uart` bench 路径。
  - axplat 形状（`crates/axplat-riscv64-lichee-d1/src/`，767 行）：`boot.rs` naked `_start`（a0=hartid/a1=dtb → boot stack → `init_boot_page_table`（SV39 1G block，T-Head 位）→ MMU → `axplat::call_main`）；模块 `boot/console/init/mem/power/time` + `irq|irq_stub`（feature `irq-if` 下 stub，即 D3 要求的 fail-closed IrqIf 模式）；`lib.rs` 经 `axconfig_macros::include_configs!` 从 crate 内 `axconfig.toml` 加载配置（`PACKAGE` 必须等于 `CARGO_PKG_NAME`）。
  - early console：`kernel/src/platform/early_console.rs` 已有 U8 与 stride-4/U32 两种 polling 实现（`DwApbUart32EarlyConsole`）；`tests/early-console-host-harness.rs` 六项 host 测试曾 GREEN（仅覆盖字符与偏移，不覆盖真板 clock/pinmux/MMIO）。K3 UART0 `reg-shift=2`+`reg-io-width=4` = 4 字节步进 × U32，与 stride-4/U32 形状吻合。
  - FDT 来源：`k3/others/Buildroot-K3-v1.0.7/bootfs.img ::/k3_com260_ifx.dtb`（本轮提取核对）`model = "SpacemiT K3 Com260 IFX"`、`serial@d4017000` clocks 形状与板上 `board-facts.txt` 一致——与运行 FDT model 匹配；板上权威回退来源为 `/sys/firmware/fdt` 导出。
  - 真板 RAM 观测（Iteration 001）：候选区间 `[0x160000000, 0x2f0000000)`，payload ≥`0x180000000`；`0x140000000` 为 U-Boot 内核加载址且落入 CMA，禁用；SWIOTLB 自 `0x2f0e00000` 起；U-Boot 工作区观测下界 `0x2f9700000`。第三方路线（fastboot stage `0x180000000`/64MiB + `bootm`）仅为可行性参考，地址与命令可用性都以本板现场为准；`Autoboot in 0 seconds` 证实需现场确定控制台进入手法。
- Code and Critical Path: 新增 `crates/axplat-riscv64-k3`（boot/console/mem/power/time/irq_stub + axconfig.toml）→ 根 feature `k3` + `src/main.rs` extern → `kernel` feature `k3` + `kernel/src/platform/k3.rs` descriptor + `mod.rs::descriptor()` 分流与组合守卫 + `entry.rs` k3 最小分支 → `Makefile` k3 别名与 FIT 目标（ITS：kernel+FDT）→ 板上 fastboot/stage 或等效只读 RAM 装载 → `bootm` → 串口阶段标记 → 复位。数据流：FIT load/entry 必须与 axconfig `kernel-base-paddr`/ELF `e_entry` 交叉一致。

**Implementation Guidance**

先 3.1（host 分流与构建，无板需求）再 3.2（FIT 打包与区间核对），最后 3.3（板上）。axplat boot 页表用 Sv39 1G block 覆盖内核加载区与 UART MMIO（`0xd4017000` 位于 `[0xc0000000,0x100000000)` block），不用 D1 的 T-Head 位。axconfig 的 `kernel-base-paddr` 取候选区间内对齐值（≥`0x180000000`）；`MEM` 等容量参数为非实质选择，Act 在区间内取保守值即可。entry 的 k3 分支参照 D1 smoke 形状输出 ≥3 个可区分阶段标记（进入/页表与 MMU/console 就绪），失败时标记能区分「未跳转 / 页表 / UART clock-pinmux」。FDT 打包优先用本地 `k3_com260_ifx.dtb`（构建时以 model 与关键节点字段对 `board-facts.txt` 核对），板上如发现行为不符再导出 `/sys/firmware/fdt` 替换并记录。

**Task Contracts**

### 3.1: K3 单 hart 平台分流与最小入口（host）

- Requirement/Scenario: R6「K3 平台选择不改变既有平台」（选择 K3 / 构建既有平台）；R4「早期串口与现有异步设备独立」（仅最小平台服务可用）。
- Depends on: Iteration 001 accepted（UART/内存/裁决字段）。
- Targets: 根 `Cargo.toml [features]`+`[dependencies]`（新 `k3` feature 与 `axplat-riscv64-k3` 可选依赖，形状对齐 `lichee-d1`）；`src/main.rs`（extern crate）；`kernel/Cargo.toml [features]`（`k3`）；新 `crates/axplat-riscv64-k3/`（`src/{boot,console,init,mem,power,time,irq_stub}.rs` + `axconfig.toml`，`irq-if` feature 下提供 fail-closed IrqIf stub，不实现 APLIC/IMSIC）；`kernel/src/platform/k3.rs`（descriptor：UART0 `0xd4017000`、stride=4、U32、Iteration 001 裁决的 MemoryLayout；无虚构 PLIC/AIA 基址）；`kernel/src/platform/mod.rs`（descriptor() 分流 + `compile_error!` 禁止 k3 与 qemu/lichee-d1 变体组合）；`kernel/src/entry.rs`（`k3` cfg 最小分支：阶段化 polling 输出后受控停留，不 import QEMU 驱动路径）；`Makefile`（`k3:` 别名，参数模式对齐 `lichee:`）。
- Current behavior: 仓库无 K3 feature/axplat/入口；`APP_FEATURES=k3` 构建失败于未知 feature；非 D1 构建的 descriptor/entry/驱动全部落入 QEMU 路径。
- Required behavior: `make … APP_FEATURES=k3 MYPLAT=axplat-riscv64-k3 PLAT_CONFIG=…/axconfig.toml BUS=mmio … build` exit 0 产出 RISC-V ELF；ELF 含 K3 UART 常量与 K3 `_start`/entry 符号、不含 VirtIO/MMIO QEMU 设备驱动符号；`descriptor()` 在 k3 下返回 K3 descriptor；互斥 feature 组合触发 `compile_error!`；QEMU/D1/VF2 既有构建入口各自 exit 0。
- Preserve: 既有 QEMU/D1/VF2 构建与运行语义、异步网络契约、`tests/` 现有目标；不改 OpenSBI/U-Boot。
- Forbidden: 实现 AIA/APLIC/IMSIC 中断 delivery；启用 IRQ/DMA/async UART/rootfs；修改既有平台 descriptor 常量；在 K3 路径引入 VirtIO/PLIC 初始化；持久介质写入。
- Test witness: RED——当前 `make ARCH=riscv64 APP_FEATURES=k3 … build`（或等效 cargo `--features k3`）以未知 feature 失败，记录错误与退出码；变更后同命令 GREEN。
- GREEN condition: k3 构建 exit 0；`readelf -h`/`nm`/`objdump` 原生命令显示 entry/_start 存在、`objdump -d` 可见 `0xd4017000` 常量、无 virtio 驱动符号；qemu（`make ARCH=riscv64 … build` 默认）、`make lichee`、`make vf2` 各 exit 0；early-console host 六项测试重跑 exit 0。
- Verification: 构建退出码 + ELF 原生命令输出直接判定；平台分流以符号/常量存在性与互斥守卫编译错误直接判定。
- Stop when: axplat/axruntime 接口与 K3 S-mode handoff 假设不符（返回 Plan，按 D3 处置）；K3 构建需要改动 axfeat/axruntime 公共行为（返回 Plan）。

### 3.2: kernel+FDT 临时 FIT 封装与区间核对（host）

- Requirement/Scenario: R3「首次启动保持板载持久固件不变」（安全 RAM 引导的地址不重叠前置）。
- Depends on: 3.1（k3 ELF）。
- Targets: `make/build.mk` 或 `Makefile` 新增 K3 FIT 目标（ITS：kernel + FDT，`mkimage -f`）；FDT 来源 `k3/others/Buildroot-K3-v1.0.7/bootfs.img ::/k3_com260_ifx.dtb` 提取入仓（或 build 时提取；提取物随 change 记录来源）；产物 `StarryOS_riscv64-k3*.fit`。
- Current behavior: 仓库只有 legacy uImage 规则（无 FDT、单 kernel）；无 K3 FIT；候选区间已裁决但无镜像落位。
- Required behavior: `make`（k3 FIT 目标）exit 0 产出 FIT；`dumpimage -l` 列出 kernel load/entry 与 FDT load；kernel load=entry 一致性由 ELF `readelf -h e_entry` 与 axconfig `kernel-base-paddr` 交叉核对；全部地址落在 `[0x160000000, 0x2f0000000)` 且 ≥`0x180000000`，kernel/FDT/staging 区间互不重叠、不触 CMA/reserved/U-Boot 观测区（对照 Iteration 001 区间表在 Act Response 复算记录）；FDT 与 `board-facts.txt` 的 model/serial 节点字段一致。全程 host-only，不写持久介质。
- Preserve: 既有 `OUT_UIMG`/D1/VF2 镜像规则不变；`k3` 参考仓库只读（提取到本仓或 /tmp）。
- Forbidden: 使用第三方示例地址作默认（`0x140000000` 落 CMA 禁用；stage 区间自行推导）；把 legacy uImage 冒充 FIT；在 ITS 中写入未裁决地址。
- Test witness: RED——FIT 目标存在前：目标命令失败（no rule / 产物不存在），`dumpimage -l` 于预期产物路径失败；区间核对先行：以 Iteration 001 区间表对 ITS 草案地址做失败/通过判定记录。
- GREEN condition: FIT 构建 exit 0；`dumpimage -l` 显示 kernel load/entry 与 FDT load 全部位于候选区间且互不重叠；ELF `e_entry`/`kernel-base-paddr`/ITS 三者一致；FDT model 字段核对记录在 Act Response。
- Verification: `mkimage`/`dumpimage`/`readelf`/`axconfig-gen` 原生输出直接判定；区间重叠以记录值算术核对写入 Act Response。
- Stop when: 候选区间无法同时容纳 kernel+FDT+staging（返回 Plan 调整）；FDT 字段与板上事实冲突（记录并返回 Plan）。

### 3.3: 板上 RAM 启动与阶段化首字节（board）

- Requirement/Scenario: R3（安全 RAM 引导 / 地址重叠或交接失败 / 操作者取消）；R4「早期串口独立」（仅最小平台服务可用 / UART 没有输出）。
- Depends on: 3.2（FIT）；Iteration 001 恢复路径已验证。
- Targets: 操作者按 Act 指令包执行：现场确定 U-Boot 控制台进入手法（`Autoboot in 0 seconds` 下按键/Ctrl-C 等非持久手段）→ 以可用只读 RAM 装载路线（fastboot stage/`fatload`/`loady` 等现场可用者）装入 FIT → `bootm` → 串口观察阶段标记 → 复位回原系统；`evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`。
- Current behavior: 板上从未运行 StarryOS；固件链（ROM→NOR→SPL→U-Boot→UFS Linux）两次观察稳定、复位可回原系统；U-Boot 控制台进入手法与 stage 命令可用性未裁决（Iteration 001 豁免遗留）。
- Required behavior: 首字节以 ≥3 个可区分阶段标记在真串口出现（115200 ttyS0），同一 FIT 至少两次启动均可重复观察标记；任一阶段缺失时能按标记定位「未跳转/页表/MMIO-clock」；每次尝试后以普通复位回原系统并记录（`k3 login:`）；全程不执行 `saveenv`/刷写/持久写入；若操作者取消，仅复位返回并记录。
- Preserve: 板载固件与持久分区不变；只使用候选区间内地址；恢复路径失败即停止并记录。
- Forbidden: 持久写入任何形式；为「出输出」盲写 clock/reset/pinctrl 寄存器（bootloader handoff 保持 Iteration 001 观测状态，不做恢复写序——需要时返回 Plan）；把静默计为成功；启用 IRQ/DMA/async。
- Test witness: 板上串口输出行与复位序列本身（观测型）；host 侧 early-console 六项测试 GREEN 回归。
- GREEN condition: 两次独立上电/复位循环中阶段标记完整出现且一致；每次复位后回 `k3 login:`；Act Response 记录装载命令序列、标记序列与复位观察；early-console host 测试 exit 0。
- Verification: 操作者最短步骤：进 U-Boot → 装载 FIT → `bootm` → 记录串口标记 → `reset` → 记录回到登录提示；重复一次。直接可观察结果：串口阶段标记行与复位后登录提示。
- Stop when: 控制台进入手法在现场多种非持久尝试后仍不可得（能力边界，交用户裁决）；FIT 装载/`bootm` 失败且无法区分介质/地址/命令层（记录后返回 Plan）；首字节静默且阶段标记无法区分故障层（返回 Plan）；复位未回原系统（立即停止记录，RAM 路线阻塞）。

**Invariants**

板载固件与持久介质不变；K3 不落入 QEMU/D1/VF2 路径、反之亦然；地址只来自 Iteration 001 裁决区间；无 AIA/IRQ/DMA/async/rootfs；既有平台构建与测试保持；`k3` 参考仓库只读。

**Non-goals**

MAC 寄存器读取（task 4.1）、PHY/link、中断 delivery、DMA、异步设备、用户态、多 hart、性能、 FIT 之外的镜像格式、U-Boot env 修改。

**Acceptance**

- C1 / R6+R4 / 3.1：K3 feature 独立分流构建成功，ELF 含 K3 平台事实、无既有平台设备路径；qemu/D1/VF2 构建与 early-console host 测试保持。
- C2 / R3 / 3.2：FIT 结构（kernel+FDT）与全部地址落在安全候选区间且互不重叠、与 ELF/axconfig 一致；host-only 无持久写入。
- C3 / R3+R4 / 3.3：真板 RAM 启动两次重复观察阶段化首字节；每次复位回原系统；静默可按阶段定位不被计为成功。
- C4 / R3/R4/R6 / 3.1–3.3：全程无持久写入、无 IRQ/DMA/async 启用、无盲写 clock/reset。

**Verification**

- 场景「选择 K3 / 构建既有平台」（R6）：k3 与 qemu/d1/vf2 构建退出码；ELF 符号/常量存在性；互斥组合 `compile_error!`。
- 场景「安全 RAM 引导前置」（R3）：`dumpimage -l` 地址逐项与区间表对照；ELF/axconfig/ITS 三方一致。
- 场景「仅最小平台服务可用 / UART 没有输出」（R4）：板上阶段标记序列直接判定；缺失时按标记定位层级，不猜。
- 场景「地址重叠或交接失败 / 操作者取消」（R3）：装载或 bootm 失败停止并记录；取消仅复位返回。
- 兼容：`make host-test` 既有目标不回归（pre-existing ms04 既有失败维持原登记，不计入本轮）。

**Gate 2 Readiness**

PASS 条件：用户审计本 Cycle 计划并回复批准。板上串口/控制台手法与 stage 命令可用性是执行期能力边界（3.3 Stop when 已定义处置），不阻塞 Gate 2；U-Boot 自占精确边界未裁决已由候选区间余量缓解（用 `≥0x2f0000000` 地址前须先取 bdinfo），无 BLOCKED。

**Persisted Evidence**

- Mode: required

1. `evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt` — 板上装载命令序列、U-Boot 控制台手法记录、两次启动的串口阶段标记与复位观察、环境说明行。支持 C3：真板 RAM 启动输出与恢复是一次性现场事实，会话后不可低成本重取；摘要会丢失阶段定位与手法结论；缺失将阻塞 change 收尾的首字节验收与 MS09 结论。通过条件：含两次启动的标记序列与两次复位回原系统记录。

**Risks and Notes**

- U-Boot 控制台进入手法未知（`Autoboot in 0 seconds`）：3.3 现场以非持久手段尝试；不可得时按 Stop when 交用户，不猜测按键序列、不改 env。
- 实物丝印修订仍未采集（Iteration 001 Minor）：操作者到板边时顺手补记一行，闭合 G7 旁证，不阻塞本 Iteration。
- `MEM`/`kernel-base-paddr` 具体取值为非实质选择（区间内对齐即可）；axconfig `PACKAGE` 必须等于 `CARGO_PKG_NAME`（axplat 配置宏断言）。
- 本地 `k3_com260_ifx.dtb` 与板上 UFS 内同名 DTB 可能存在版本差异：以 model/关键节点字段核对为准，行为不符时以板上 `/sys/firmware/fdt` 导出替换并记录。
- host 侧产物（ELF/FIT）可低成本重建，Persisted Evidence 不含 host 构建输出。

## Act Response

- Status: reported（Cycle 000 全部任务完成：3.1/3.2 host 侧 Gate 3/4 通过；3.3 板上运行达成——完整标记序列 + 复位回原系统，第三轮重复循环经用户豁免跳过（原话与风险已记录）。等待 Plan Review 审计）

**Implemented**

- Task 3.1（完成）：
  - 新增 `crates/axplat-riscv64-k3/`（`Cargo.toml`、`axconfig.toml`、`src/{lib,boot,console,init,irq_stub,mem,power,time,time_math}.rs`）：镜像 lichee-d1 crate 形状；boot 页表为标准 Sv39 1G block（无 T-Head 位），恒等映射 block 3（UART `0xd4017000`）与 block 6（kernel 窗口 `0x1_8000_0000..0x1_C000_0000`），高半窗映射 `0x103/0x106`；`irq-if` 下仅提供 fail-closed IrqIf stub（无 APLIC/IMSIC）；axconfig 事实全部来自 Iteration 001 裁决 + 入仓 DTB（`timebase-frequency=0x16e3600`=24 MHz、`uart-irq=42` 为裁决的 DT hwirq、安全窗口 `0x1_6000_0000+0x1_9000_0000`、`kernel-base-paddr=0x1_8000_0000`）。
  - `kernel/src/platform/k3.rs`（新增）：K3 descriptor（UART stride=4/U32、MemoryLayout=裁决窗口、`BootKind::UBootImage`、`plic_base_paddr=0` 显式"未接通"哨兵、`virtio_net: None`）+ `run_first_byte_park()`（阶段化 polling 标记 stage=1/2/3 后 wfi 受控停留）。
  - `kernel/src/platform/mod.rs`：`descriptor()` 三路分流（k3/lichee-d1/qemu）+ 两条新 `compile_error!`（k3+qemu、k3+lichee-d1 变体）。
  - `kernel/src/entry.rs`：K3 最小分支（cfg `riscv64`+`k3`）调用 `run_first_byte_park()`；QEMU imports 与 QEMU init 块的 cfg 从 `not(lichee-d1)` 收窄为 `not(any(lichee-d1, k3))`。
  - `crates/axplat-riscv64-k3/src/boot.rs`：MMU 使能后、`axplat::call_main` 前经 UART0 MMIO 直写 `[k3:pt-mmu]\r\n` 原始标记（运行时无关的第一字节见证）。
  - 根 `Cargo.toml`（`k3` feature + 可选依赖）、`src/main.rs`（extern crate）、`kernel/Cargo.toml`（`k3` feature）、`Makefile`（`k3:` 别名，MEM=6400M 保持裁决窗口）。
- Task 3.2（完成）：
  - `tools/k3_com260_ifx.dtb`（入仓，提取自 `../k3/others/Buildroot-K3-v1.0.7/bootfs.img ::/k3_com260_ifx.dtb`，sha256 `5e04df6e55807980f6485fa1389b6d53da3742fc6b597be898ef548811e0c77b`；model `SpacemiT K3 Com260 IFX` 与 `board-facts.txt` 的 `MS09_FACTS_MODEL` 一致）。
  - `tools/starry-k3.its`（kernel+FDT，root `#address-cells=<2>` 2-cell 编码，对齐参考 `esos.itb` 惯例；kernel load/entry `<0x1 0x80000000>`，FDT load `<0x1 0x81000000>`）。
  - `Makefile`：`k3-fit` 目标（dtc+grep 单步核对 model → `mkimage -f` → `dumpimage -l`）；`k3:` 别名末尾调用 `k3-fit`。
- Task 3.3（host 侧就绪，板上待操作者执行）：FIT `StarryOS_riscv64-k3.fit`（276,000 B）已产出；操作者指令包见下。

**Deviations from Plan**

1. cfg 缝隙统一（超出 3.1 Targets 文件清单）：内核 crate 的 QEMU 完整路径以 `cfg(not(feature = "lichee-d1"))` 为缝（`drivers/mod.rs`、`file/mod.rs`、`pseudofs/dev/mod.rs`、`syscall/mod.rs`、`syscall/fs/ctl.rs` 共 28 处），且 `kernel/src/lib.rs` 以 `lichee-d1-smoke/kbench` 整模块排除 drivers/file/mm/pseudofs/syscall/task/time。K3 必须并入该缝才能在无 axfs/axnet 下编译内核 crate（首次构建 exit 2 复现）。全部改为 `not(any(lichee-d1, k3))` 并把 `k3` 加入模块排除门与 crate 级 `allow(dead_code…)` cfg_attr；k3 关闭时全部为 no-op，qemu/lichee 回归 exit 0、vf2 失败签名逐字不变证明无行为回归。属机械性等价实现，记录为偏差。
2. boot.rs 预运行原始标记：Plan 的标记区分（未跳转/页表/UART clock-pinmux）需要 axruntime 之前的一层见证，`[k3:pt-mmu]` 即其最小实现。
3. 互斥守卫的可达性：make 层的 k3+qemu/k3+lichee 组合在 axplat 层即被单一 `AX_CONFIG_PATH` 的 PACKAGE 断言拒绝（exit 2，fail-closed，不产出镜像）；kernel `compile_error!` 守卫经 `cargo check`（AX_CONFIG_PATH 未导出、各 axplat crate 用本地 fallback 配置）实际触发两处文案。守卫本身按契约保留为第二道防线。
4. `timebase-frequency`/`uart-irq` 的来源：裁决表未含 timebase；取自入仓 DTB（与运行 FDT model 一致）`timebase-frequency = <0x16e3600>`；`uart-irq=42` 取 2.3 裁决的 DT hwirq。两者仅服务 fail-closed stub 下的接口符号，不构成 IRQ 语义结论。
5. mkimage `/incbin/` 相对 ITS 文件目录解析：ITS 内 kernel 路径写 `../StarryOS_riscv64-k3.bin`（等价调整）。
6. `InterruptConfig.plic_base_paddr` 为 struct 必填字段，K3 取 0 并注释为"未接通"哨兵，不虚构 PLIC/AIA 基址。
7. `power.rs` 未含 `cfg(smp)` 的 `cpu_boot`（K3 crate 只定义 `irq-if` feature，单 hart）；`Makefile` k3 别名 `MEM=6400M` 经 `strtosz` 保持 `plat.phys-memory-size=0x1_9000_0000`（已用 `axconfig-gen` 读回核实）。

**Self-Review（Gate 3）**

- Spec compliance：3.1 GREEN 条件逐项满足（见下）；K3 路径无 VirtIO/PLIC/AIA/DMA/async 符号（nm 计 0）；不变量保持（既有平台构建与测试通过，vf2 维持 pre-existing 失败且签名不变）。3.2 GREEN 条件逐项满足（结构、区间、三方一致、host-only 无持久写入）。
- Code quality：k3 新增面无新增警告类别（仅 2 个既有辅助函数 `select_schedulable_cpu`/`select_wake_cpu` 在单 hart 最小 feature 集下 unused，Minor 记录）；diff 无计划外修改；无身份型证据机制或判定层（dtc|grep 为构建前置的单步原生管道，`dumpimage -l` 为原生输出）。
- Critical/Important：无。

**Verification Evidence（Gate 4）**

- 3.1 RED：`make ARCH=riscv64 APP_FEATURES=k3 build` → exit 2，`error: the package 'starryos' does not contain this feature: k3`。
- 3.1 GREEN：`make k3` → exit 0（`StarryOS_riscv64-k3.elf` 365,352 B / `.bin` 123,072 B）。`readelf -h`：`Entry point address: 0xffffffc180000000`；`nm`：`ffffffc180000000 T _start`、9 个 `axplat_riscv64_k3` 符号、virtio 符号 0；ELF 内含 `[starry-k3] stage=1 entry-reached`、`stage=2 console uart=0xd4017000 …` 标记字符串。
- 互斥：`cargo check --features "… k3 qemu"` → `error: features `k3` and `qemu` cannot be enabled together`；`cargo check --features "… k3 lichee-d1"` → `error: features `k3` and lichee-d1 variants cannot be enabled together`；make 层两种组合均 exit 2（PACKAGE 断言，fail-closed）。
- 回归：`make ARCH=riscv64 build`（qemu）→ exit 0；`make lichee` → exit 0（Android boot image 打包成功）；early-console host 六项测试 `ok. 6 passed` exit 0；`make vf2` → exit 2，错误签名与改动前基线一致（`unresolved imports axfs…/axnet…`，grep -c "^error" 均为 50）——pre-existing 失败，与本次改动无关（见 Experience Candidates）。
- 3.2 RED：`make k3-fit` → exit 2 `No rule to make target 'k3-fit'`；`dumpimage -l StarryOS_riscv64-k3.fit` → exit 1 `No such file or directory`。
- 3.2 GREEN：`make k3` → exit 0，产出 `StarryOS_riscv64-k3.fit`（276,000 B）。`dumpimage -l`：kernel `Load 0x180000000 / Entry 0x180000000 / 123,072 B`；FDT `Load 0x181000000 / 150,998 B`。
- 三方一致：ELF `e_entry 0xffffffc180000000` − `PHYS_VIRT_OFFSET 0xffffffc000000000` = `0x180000000` = `axconfig-gen .axconfig.toml -r plat.kernel-base-paddr`（`0x1_8000_0000`）= ITS load/entry。
- 区间算术（裁决窗口 `[0x160000000, 0x2f0000000)`，payload ≥`0x180000000`）：kernel `[0x180000000, 0x18001E100)`；FDT `[0x181000000, 0x18124E36)`；staging（操作者装载位 `0x2E0000000` + FIT 0x436A0）`[0x2E0000000, 0x2E00436A0)`。两两不重叠（0x18001E100 < 0x181000000；staging 在窗口尾段）；不触 CMA（终点 `0x160000000` 恰为窗口下界）、SWIOTLB（`0x2f0e00000` 起）、framebuffer（`0x2fe000000` 起）、U-Boot 观测工作区（≥`0x2f9700000`）；reserved-memory 全集不在窗口内（Iteration 001 Review 已复算，采信）。
- DTB 核对：入仓 DTB sha256 与提取件一致；`dtc | grep 'model = "SpacemiT K3 Com260 IFX"'` 构建守卫通过 = 与 `board-facts.txt` 的 `MS09_FACTS_MODEL` 一致；`serial@d4017000` 字段与 2.1 对照表相符。

### 3.3 第一轮板上结果与修复（2026-10-01）

第一轮执行（操作者，完整记录见 Evidence）：长按 `s` 进入 U-Boot Shell（板上确认）→ `fatload usb 0:0 0x2e0000000 StarryOS_riscv64-k3.fit`（276,000 B，55 ms）→ `iminfo` FIT 解析与 crc32 校验全过 → `bootm`：kernel 装载 0x180000000、FDT 先落 0x181000000 后被 U-Boot 重定位至 0x2fbd4f000 → `Starting kernel` → **首字节成功**：`[k3:pt-mmu]`（跳转/引导页表/MMU/UART0 clock-pinmux/MMIO 全链路工作）+ axruntime 横幅（riscv64-k3 / smp=1）→ panic 于 `axallocator` 位图初始化：`bitmap capacity exceeded: need 1507328 pages but CAP is 1048576`（`page-alloc-4g` 的 4 GiB CAP < 声明窗口 5.75 GiB）。诊断层级与标记设计一致：页表/MMU/UART 层通过，失败精确定位在 axruntime 内存初始化的配置面。

修复（非实质容量选择，Plan Context 明示 MEM 等容量参数由 Act 在区间内取保守值）：根 `Cargo.toml` 的 `k3` feature 追加 `axfeat/page-alloc-64g`（→ `axalloc` → `axallocator/page-alloc-64g`，BitAlloc16M，CAP 16,777,216 页 = 64 GiB）。axallocator 的 cfg 链为 first-match（1t > 64g > 4g > 256m），与既有 `page-alloc-4g` 共存时 64g 生效；其他平台不启用该 feature，行为不变。host 侧证据：`cargo tree --features k3 -e features -i axallocator` 显示 `page-alloc-64g` 在启用集；`.bss` 增至 2.4 MiB（0x263000），内核物理足迹终点 `0x180282000`，与 FDT `0x181000000` 间隔约 3.5 MiB，区间不重叠复核通过；FIT load/entry 不变。

第一轮附带事实（回填 Iteration 001 豁免项）：`bdinfo` lmb `reserved[4]=[0x2fbd7c000, 0x2ffffffff]`——观测到的 U-Boot 自占区起点高于裁决窗口上界，窗口安全性获现场佐证（豁免项部分闭合）；`kernel_addr_r=0x140000000` 证实 stock 加载址落入 CMA（禁用区），佐证 staging `0x2e0000000` 选择；`fatload` 文件名大小写敏感（须 `StarryOS_riscv64-k3.fit` 原大小写）。

第二轮操作差异（其余同指令包）：装载后、`bootm` 前执行 `setenv fdt_high 0xffffffffffffffff`（易失，不 saveenv）——阻止 U-Boot 把 FDT 重定位到裁决窗口外的 0x2fbd4f000（该地址不在引导页表 block 3/6 映射内，内核若按 a1 访问将缺页）；设 `fdt_high` 后 FDT 停留在 `0x181000000`（block 6 已映射、managed RAM 之内），a1 指针可安全解引用。

第二轮结果（2026-10-01，操作者执行，完整记录见 Evidence §1.6）：`setenv fdt_high 0xffffffffffffffff` 后 `bootm` 全程跑通——`Using Device Tree in place at 0x181000000`（重定位被阻止）→ `[k3:pt-mmu]` → axruntime 横幅 → **`[starry-k3] stage=1/2/3` 完整序列** → 受控停留，无 panic。kernel crc32 `e4e9f37d`（64g 修复后 FIT）与第一轮 `c57cb0f5` 区分两次镜像。

**3.3 收口（GREEN，含用户豁免）**：复位回原系统由用户确认（原话「我已经触发复位，正常回到原linux」，2026-10-01）；第一轮 panic 后操作者复位重启进入第二轮，panic 场景恢复路径间接有效。第三轮重复循环经用户豁免跳过（原话「这一步不用做……我们就不麻烦流程了」）——风险：固定 FIT 完整标记序列为单次完整循环；缓解：第一轮独立证实同一跳转/页表/UART 路径（不同 FIT、至 panic 前），Iteration 001 已两次观察 stock 启动链重复稳定。early-console host 六项测试 GREEN（3.1 已验证，覆盖未变化，采信）。交付对照 Acceptance：C1（3.1 分流构建+回归）、C2（3.2 FIT 区间/三方一致）、C4（全程无持久写入/无 IRQ/DMA/async/盲写）达成；C3 达成（完整标记序列 + 复位回原系统 + 静默可定位——重复观察按上述豁免记录）。Persisted Evidence `board-firstbyte.txt` 已创建并通过条件（两次启动记录 + 复位观察 + 豁免注记）。

### 3.3 操作者指令包（板上步骤，用户执行；2026-10-01 两次修订）

用户豁免与入口闭合：用户 2026-10-01 豁免「进 U-Boot 控制台」的专门探索（原话「进 U-Boot 控制台这个给出豁免，我们不做，我们直接做装载，阅读已有相关runbook给我流程命令行什么的」）；随后用户提供官方交叉验证来源——参考仓库 `../k3/docs/boot/com260-boot-chain.md:51`（引 docs-buildroot boot.md §U-Boot Fastboot 模式，2026-09-07 直接打开）：**板子启动时串口长按 `s` 键进入 U-Boot Shell**（K3 系列定制入口，不走标准 autoboot 打断路径，与 `Autoboot in 0 seconds` 实测不矛盾）；并由用户板上实测确认有效（原话「不用再验证了，我试了一下确实可以」，2026-10-01）。Iteration 001 遗留的「U-Boot 控制台进入手法」未裁决项就此闭合：官方交叉验证 + 板上确认。装载命令形态的板上先例：参考仓库 `.claude/runbooks/开发流程.md`（validated 2026-09-08）实跑验证 `usb start` → `fatls usb 0:0 /` → `fatload usb 0:0 <addr> <file>`（U 盘 USB Path `usb 2-1.2`，`/dev/sdb`，FAT32 整盘）；其 `fastboot` 路线附关键警告（源 `others/Rt-Async-AMP/README.md`）：**fastboot stage 传输完成后不会自动退出，必须 Ctrl+C 回到提示符**才能执行下一条命令。

前置：串口终端 115200 8N1 无流控（接线见参考 runbook：TX/RX 交叉、共地、**不接 VCC**）；FIT 在仓库根 `StarryOS_riscv64-k3.fit`。

0. 准备（按计划路线二选一）：
   - U 盘路线（命令形态有板上先例）：`cp StarryOS_riscv64-k3.fit /mnt/<盘符>/` 拷入 FAT32 U 盘（整盘无分区表）后插板 USB-A。
   - 串口路线：无需 U 盘；串口终端需支持 YMODEM 发送（MobaXterm / Tera Term 支持，PuTTY 裸版不支持）。
1. 进 U-Boot Shell（板上已验证）：给板上电或按 RESET 的同时，在串口终端**持续长按 `s` 键**（按住不放，不是单击），直到出现 `U-Boot>` 提示符。若无效 → 原样记录现象后停止回传（不盲试其他按键序列）。
2. 只读装载 FIT 到 staging `0x2e0000000`（命令不可用即换路线并原样记录报错）：
   - B1 U 盘（命令形态 2026-09-08 板上实跑过）：
     ```
     usb start
     fatls usb 0:0 /
     fatload usb 0:0 0x2e0000000 starryos-k3.fit
     ```
     警告：**不要**用 `${kernel_addr_r}`（参考 runbook 对其自有镜像使用该变量；stock 值大概率指向 `0x140000000` 一带，落入 CMA，属禁用区）——固定使用 `0x2e0000000`；`kernel_addr_r` 只读记录不做装载地址。
   - B2 串口 ymodem（B1 不可用时）：`loady 0x2e0000000` → 终端发起 YMODEM 发送选 `StarryOS_riscv64-k3.fit`（约 276 KB，30–60 秒）。
   - B3 fastboot（仅当 B1/B2 都不可用）：串口 `fastboot 0`（官方命令，进入 U-Boot Fastboot 服务）→ host（WSL2 需先 usbipd 绑定 USB）`fastboot devices` → `fastboot stage StarryOS_riscv64-k3.fit` → **传完必须串口 Ctrl+C 回 `U-Boot>` 提示符**（stage 不自动退出）→ 缓冲地址以 `printenv fastboot*` 现场为准，必要时 `cp.b` 搬到 `0x2e0000000` 并记录全程。
   - 在提示符顺手只读采集（可选，补 Iteration 001 豁免项）：`version`、`bdinfo`、`printenv kernel_addr_r bootcmd bootdelay fdt_high fastboot*`、`help loady fatload`。
3. 只读校验并启动：
   ```
   md.b 0x2e0000000 0x10        # 首字节应为 27 05 19 56（FIT magic）
   iminfo 0x2e0000000
   bootm 0x2e0000000
   ```
4. 串口期望序列（逐行记录）：`[k3:pt-mmu]` → `[starry-k3] stage=1 entry-reached` → `[starry-k3] stage=2 console uart=0xd4017000 stride=4 width=u32 baud=115200` → `[starry-k3] stage=3 parked, halting (reset to recover)`（此后静默=受控停留）。缺失即定位：全无=未跳转/页表 fault/UART clock-pinmux 死；仅 `[k3:pt-mmu]`=页表+MMU+UART 活，axruntime/entry 失败；乱码=时钟/波特率不符。`bootm` 报错或静默 → 原样记录后停。
5. 恢复：`reset` → 应回 `k3 login:`，记录一句话。
6. 重复第 2–5 步一次（两次独立循环）。
7. 结果回填：两次装载命令、串口标记序列、两次复位观察存入 `evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`（格式：首行环境说明（日期/串口通道/装载路线）→ 每轮「命令序列 + 串口输出 + 复位结果」）。
8. 禁止：`saveenv`、任何刷写/持久写入（含向 UFS bootfs 拷文件）、盲写 clock/reset/pinctrl 寄存器、把静默计为成功。
9. 顺手项（非阻塞）：补记板卡丝印修订一行（Iteration 001 Minor/G7 旁证）。

回传后 Act 补齐本 Response（evidence 通过条件：两次启动标记序列 + 两次复位回原系统记录）并置 `reported`；早前 `make k3` 的 host 侧结论在板级步骤不改变代码的前提下继续有效。

**Persisted Evidence**

- Mode: required
- `evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`：**已创建并通过通过条件**——含第一轮（U-Boot 采集/装载/iminfo/bootm/panic 全程）、第二轮（64g 修复后完整 stage=1/2/3 + `setenv fdt_high` 观察）、复位回原系统用户确认、豁免注记与风险、环境说明行；MAC 已脱敏。支持 C3。

**Experience Candidates**

- Issue 候选：`make vf2` 构建入口 pre-existing 失败（改动前基线与改动后均 exit 2，`starry-kernel` 49–50 个错误：vf2-only feature 集下 `axfs`/`axnet`/`axtask::AxTaskExt` 未解析——entry.rs 等 QEMU 路径 import 未随 vf2 feature 关闭）。证据：`/tmp/ms09_baseline_vf2.log` 与 `/tmp/ms09_vf2_after.log` 同签名；本 change 未引入也未修复（范围外）。留 Recorder 落账。
- Runbook：**用户已明确请求**创建「U-Boot + USB → StarryOS RAM 启动测试」Runbook（原话「写一个新的runbook把我们通过u-boot和usb进入starryos进行测试这个流程记录一下这个很关键」，2026-10-01）；依据本 Response 与 `evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`，由 openspec-experience-recorder 按用户指令执行（Act 不落账，候选转交）。

**未解决问题**

- 无阻塞项。Iteration 001 遗留「U-Boot 控制台进入手法」已闭合（官方交叉验证 + 板上确认：启动时长按 `s`）；「U-Boot 自占精确边界」获 lmb reserved 现场佐证（部分闭合）。第三轮重复循环的豁免风险已记录，待 Plan Review 审计认定。

## Plan Review

- Review Result: accepted

**Findings**

本 Review 独立核对全部代码 diff（14 个修改文件 + 新 crate/ITS/DTB/`k3.rs`）、板上 Evidence 全文、构建产物与板上记录的一致性。

- 3.1 满足（C1）：feature 装配（根 `k3` → `dep:axplat-riscv64-k3` + `irq-if` + `bus-mmio` + `starry-kernel/k3`）、`descriptor()` 三路分流与两条新 `compile_error!`、entry K3 最小分支与 QEMU cfg 收窄，逐行核对无误。偏差 1 的 28 处 cfg 缝改动逐处检查：全部为 `not(lichee-d1)` → `not(any(lichee-d1, k3))` 的机械等价改写，k3 关闭时 cfg 求值与 HEAD 完全一致，既有平台按构造不回归；`lib.rs` 模块排除门与 `cfg_attr` 沿用 lichee-d1-smoke 同一模式。`k3.rs` 自包含且事实全部来自 Iteration 001 裁决；`pub mod k3` 无条件编译沿 entry.rs 既有 `riscv::asm::wfi` 先例（riscv 依赖本就 target 门控），非新增限制。RED（未知 feature exit 2）、GREEN（构建/符号/互斥守卫/qemu/lichee 回归/early-console 六项）采信 Act 验证，覆盖未失效。
- 3.2 满足（C2）：`tools/k3_com260_ifx.dtb` sha256 与本 Review 会话内从 `bootfs.img` 的独立提取件完全一致（`5e04df6e…`）——FDT 溯源独立验证。ITS kernel load/entry `<0x1 0x80000000>`、FDT `<0x1 0x81000000>`，与 axconfig `kernel-base-paddr=0x1_8000_0000`（直接读取核对）、descriptor `link_vaddr=0xffff_ffc1_8000_0000` 三方一致。区间算术复算：kernel `[0x180000000, 0x18001E100)`、FDT `[0x181000000, 0x18124E36)`、64g 修复后物理足迹终点 `0x180282000`（`0x180000000+0x282000`）、staging `[0x2E0000000, 0x2E00436A0)`——两两不重叠且全在裁决窗口内，不触 CMA/SWIOTLB/framebuffer/U-Boot 观测区。`k3-fit` 的 `dtc|grep` model 守卫为单步原生管道，合规。
- 3.3 满足（C3，含记录在案的用户豁免）：Evidence 两轮记录完整自洽。决定性交叉验证——仓库 `StarryOS_riscv64-k3.fit` 的 kernel crc32 `e4e9f37d` 与板上第二轮记录一致，磁盘产物即板上跑通完整 stage 序列的镜像；第一轮（crc `c57cb0f5`）独立证实跳转/引导页表/MMU/UART 链路（`[k3:pt-mmu]` + axruntime 横幅 `platform = riscv64-k3`），panic 精确定位分配器容量，修复 `axfeat/page-alloc-64g` 属 Plan Context 明示的容量非实质选择（feature 定义注释含板上依据，其他平台不启用）。`setenv fdt_high` 为易失设置且是必要修正——U-Boot 默认把 FDT 重定位到 `0x2fbd4f000`（引导页表映射外），证据 §2/§1.6 完整记录该诊断；不触 Forbidden（禁令针对 `saveenv`/持久写入）。复位回原系统有用户原话确认；第三轮重复循环按用户原话豁免，风险与缓解在案。静默/异常分层由 `[k3:pt-mmu]` + stage 标记直接支撑。
- 附加收获：`bdinfo` lmb `reserved[4]=[0x2fbd7c000,0x2ffffffff]` 高于窗口上界，Iteration 001「U-Boot 自占边界」豁免项获现场佐证；`kernel_addr_r=0x140000000` 证实落入 CMA 禁用区；控制台手法（长按 `s`）经官方交叉验证 + 板上确认闭合。
- Plan 错误（非阻塞）：3.1 GREEN 写「`make vf2` 各 exit 0」，实际 vf2 入口在本改动前即失败（`/tmp/ms09_baseline_vf2.log` 与 after 日志均 50 errors、同签名、Error 2——entry.rs QEMU 路径 import 未随 vf2 关闭的既有问题）。「既有平台构建保持」按无回归解释成立（qemu/lichee 实际 exit 0）；vf2 既有失败作为 Issue 候选报告，不落账。
- Minor（记录，不阻塞）：① 仓库根 `StarryOS_riscv64-k3.fit` 未被 `.gitignore` 覆盖（`*.bin`/`*.img` 有条目、`*.fit` 无）——一行卫生项，建议下一 Cycle 或收尾时补 `.gitignore`；② `boot_stage_marker` 的 LSR 等待为无界自旋，与 design 风险段「有界状态观察」措辞有差——诊断结果等价（UART 死则静默、不产生假成功），不要求返工；③ stage=2 字符串硬编码 console 事实而非从 descriptor 格式化（纯外观）。

**Acceptance Gaps**

None。C1–C4 全部满足；两项用户豁免（第三轮重复、控制台探索→后经板上确认闭合）原话与风险在案。

**Evidence**

- 代码：`git diff`（14 文件 +131/−42）逐块核对；`crates/axplat-riscv64-k3/{boot.rs,axconfig.toml,Cargo.toml}`、`kernel/src/platform/k3.rs`、`tools/starry-k3.its` 直接读取。
- 产物一致性：`dumpimage -l StarryOS_riscv64-k3.fit` kernel crc32 `e4e9f37d` = 板上第二轮记录；FDT crc `8711c0d6` 两轮一致。
- `evidence/002-k3-boot-first-byte/000-initial/board-firstbyte.txt`（267 行）：两轮命令/输出/判定完整，`ethaddr` 已脱敏，豁免原话在案。
- 采信来源：Act Response Verification（RED 两条、GREEN 构建/符号/互斥/回归、三方一致算术）；vf2 pre-existing 双日志。本 Review 未改动产品代码。

**Follow-up Decision**

三轮任务全部达到 Acceptance（用户豁免项合规记录）；遗留为非阻塞 Minor。Iteration 002 完成，按 Map 展开 Iteration 003（MAC 只读寄存器基线，task 4.1）；不创建后继 Cycle。R68 Runbook 已由 Recorder 按用户指令登记，本 Review 核对其内容与 Evidence 一致。

**Iteration Plan Update**

None。

**Next Cycle**

None。

**Next Iteration**

`iterations/003-mac-register-baseline/000-initial.md`（MAC 只读寄存器基线，task 4.1）。
