# K3 CoM260 交互 shell 可行性探索

> Snapshot: [SNAPSHOT](../docs/SNAPSHOT.md)
> Captured revision: `082339ba`（StarryOS `k3`）；外部 k3 参考仓库 `e6d0557`、Rt-Async-AMP `ccb1ff0b`（与 R62/R69 采集固定点一致）
> Captured at: 2026-10-02
> See also: [K3 参考仓库迁移评估](k3-reference-repository-migration-assessment.md)

## 目标与范围

回答三个问题：参考仓库（Rt-Async-AMP / tgoskits）是否在 K3 真板上做了交互 shell；哪些经验可按 M42 分级借鉴；若 StarryOS 要在 K3 上提供 shell，当前已承诺路线中哪些工作是前置、哪些属于新范围。范围为 AP 侧用户面可行性调查，不修改产品代码、不授权刷写或挂载板上分区。

## 已确认事实、推断与未确认项

### 已确认事实

- Rt-Async-AMP 的 AP 大核运行作者自己的 StarryOS fork（`github.com/Oveln/StarryOS`，与本仓同源），「AP 侧通用内核基本保持原状」（`rt-async/RT-Async/blogs/技术报告/2026-09-05-AMP双内核实时系统技术报告.md`）。
- 存在真板可用的用户态交互 shell：`user-apps/rtsh`——REPL 提示符 `rtsh>`、tty 规范模式行编辑（退格/Ctrl-U/Ctrl-W）、`rtsh <cmd>` 单发模式；经 `/dev/rt_shm`（open + mmap + ioctl 门铃 + ppoll）与实时核对话。AP 用户面刻意收敛为「rtsh 一个程序 + robot-py 一个 Python 库 + user-test-* 测试」。
- rtsh 依赖的内核能力即通用 shell 所需：tty 层、UART 交互输入、用户进程、mmap/ioctl/ppoll syscall；其 RPC 用途（跨核控制）与 shell 形态无关，模式可分离借鉴。
- K3 真板 AP 产物只有 `starryos.uimg`（FIT = kernel + DTB，`configs/board/spacemitk3-com260kit.its`）；rootfs 注入仅发生在 QEMU 环境（`xtask/src/build.rs:410` 注释「rootfs 注入用」）。
- K3 侧用户面完整形态由三个驱动 + bootargs 支撑：DTB `bootargs = "console=ttyS0,115200 earlycon=sbi root=PARTLABEL=rootfs rw"`（`configs/board/spacemit-k3-com260-ifx.dts:7949`）；板级 feature 启用 `ax-driver/{k3-pxa-uart, k3-ufs, k3-gmac, k3-pinctrl}` 与 `zicbom`（`configs/board/spacemitk3-com260kit.toml`）——即 UFS 挂 rootfs 分区、PXA UART 承担 console 交互、GMAC 提供网络投递（robot-py 注释「板上 wget 后 import 即用」）。
- 他们的 QEMU rootfs 与本仓同源：fork Makefile `ROOTFS_URL` 指向 `github.com/Starry-OS/rootfs` releases（与本仓 references 登记的 rootfs 同一发行源），本仓 QEMU 用户态经验可直接平移。
- tgoskits 可借鉴驱动面（规模与测试数沿 R62 采集）：PXA UART ~1,300 行含 19 单测（RX timeout、CTI 丢失恢复、THRE/TEMT 区分）；AIA glue ~1,471 行（APLIC→IMSIC 路由、`stopei` claim/complete 合并教训）；UFS ~3,249 行 23 单测；SDHCI ~761 行 12 单测。
- 许可证边界：tgoskits 顶层 Apache-2.0（可逐文件带来源改写）；Rt-Async-AMP 根仓与 rt-async 无 LICENSE 声明（只记机制，不复制其自有代码）。
- 板级坑在案：40pin 扩展口仅引出一路 R_UART 且 3.3V 侧无上拉（`blogs/技术报告/2026-09-04-K3平台开发问题反馈.md`）——console 健壮性设计需留意。
- 本仓现状：K3 构建为单 hart polling 探针内核（stage 探针后 `wfi`），`IrqIf` 为 fail-closed stub，无 timer/UART IRQ、无用户态；MS09 已 accepted（R68 启动环、polling 串口、MAC/PHY 可观测链路为底座）；MS10（目标板设备中断基线）将建立 AIA claim/dispatch/handler/EOI 底座。

### 推断

- shell 最小路径不需要存储驱动：参考仓库 QEMU 线存在 musl 静态单程序形态，用户程序内嵌内核镜像即可获得首个交互 shell；UFS/rootfs 属于「复用 Linux 生态」的增强路线而非前置。
- 当前已承诺路线中，MS09（已完成）与 MS10（AIA 底座）是 shell 的全部实质前置：timer IRQ（调度/超时）与 UART RX IRQ（交互输入）与 MS10 的 MAC IRQ 同层；MS11–MS15（DMA、异步数据面、恢复、SMP、性能）与交互性正交，不构成前置。
- 走 rootfs 路线时，本仓 QEMU 形态的 rootfs/用户程序经验（R44 注入流程）与对方 K3 经验同构，迁移成本低。

### 未确认项

- rtsh 类 musl 静态程序在其 K3 镜像中的确切打包通道（嵌入 app section 还是经网络落入 rootfs）——立项调查时确认，不影响分层结论。
- `root=PARTLABEL=rootfs` 指向的分区为出厂分区或自写分区：robot-py 注释「板上 rootfs 的 Python（Yocto 构建）」倾向出厂分区，但无挂载来源的直接书面证据；若本仓走 rootfs 路线，挂载目标与读写风险需独立裁决。
- 出厂 UFS 分区布局与 R70 partition JSON 的对应关系未在当前板核对。

## 调用链或数据流

参考仓库 K3 交互链（已验证形态）：

```text
U-Boot bootm(starryos.uimg: kernel+DTB)
  → StarryOS fork：AIA(APLIC/IMSIC) + k3-pxa-uart(console=ttyS0) + k3-ufs(root=PARTLABEL=rootfs)
  → 用户态 rtsh（tty REPL / 单发）→ /dev/rt_shm mmap+ioctl+ppoll → RT 核
  （GMAC 网络用于程序投递：板上 wget）
```

本仓目标最小链（建议形态）：

```text
R68 启动环 → K3 axplat：AIA claim/dispatch（MS10 底座）→ timer IRQ + UART RX IRQ
  → K3 异步 UART（PXA 硬件层 + 现有 copier/waker/poll 契约）
  → tty console 输入 → 用户态 spawn（现有 syscall/VFS/进程加载）
  → 内嵌 shell 程序（最小路线）；rootfs/UFS 为后续独立里程碑（增强路线）
```

## 边界与失败路径

- 分层故障域与单变量原则一致：IRQ 底座（MS10 层）→ UART RX 驱动 → tty/用户态 → 用户程序载体，逐层独立验收，任一层失败不向上冒充。
- rootfs 路线的专属风险：挂载出厂分区读写属于对方的风险选择；本仓采用前需独立评估（分区事实、恢复路径、写入边界）。
- 许可证边界：根仓/rt-async 自有代码不复制；tgoskits 逐文件保留 Apache-2.0 来源标注。
- 本分析不授权任何刷写、分区挂载或代码迁移；具体范围由后续 milestone 与 change 裁决。

## 测试、验证入口与影响面

- 现有验证入口：R68（RAM 启动环）、MS09 host harness（early-console/mac/phy，本仓 `tests/`）、QEMU rootfs 用户态流程（R44 注入先例）。
- 若立项 shell 里程碑，受影响面：`crates/axplat-riscv64-k3`（IrqIf/time/irq 实化）、新增 K3 异步 UART 适配（参照 `kernel/src/drivers/uart_init.rs` 既有边界）、内核 tty/console 输入路径、用户程序构建入口；FIT 结构与 R68 流程保持不变。

## 关键文件

### 参考仓库（只读）

- `k3/others/Rt-Async-AMP/user-apps/rtsh/src/{main,lib}.rs` — 交互 shell 形态与依赖的内核能力面。
- `k3/others/Rt-Async-AMP/tgoskits/os/StarryOS/configs/board/{spacemitk3-com260kit.toml,spacemitk3-com260kit.its,spacemit-k3-com260-ifx.dts}` — feature 面、FIT 组成与 bootargs。
- `k3/others/Rt-Async-AMP/xtask/src/build.rs` — 产物职责（QEMU rootfs 注入 vs K3 uimg）与用户程序构建。
- `k3/others/Rt-Async-AMP/tgoskits/drivers/serial/some-serial/src/pxa_uart/`、`tgoskits/drivers/ax-driver/src/soc/k3/` — PXA UART/AIA/pinctrl 参考实现（Apache-2.0）。
- `k3/others/Rt-Async-AMP/rt-async/RT-Async/blogs/技术报告/` — 终版技术报告与 K3 平台问题反馈。

### 本仓

- [`crates/axplat-riscv64-k3/`](../../crates/axplat-riscv64-k3/) — 现有平台面（IrqIf stub、TimeIf、mmio-ranges）。
- [`kernel/src/drivers/uart_init.rs`](../../kernel/src/drivers/uart_init.rs) — 既有异步 UART adapter/copier/IRQ 接入边界。
- [R62 迁移评估](k3-reference-repository-migration-assessment.md) — 驱动复用价值与规模的先行量化。
