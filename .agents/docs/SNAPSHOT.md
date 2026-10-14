# SNAPSHOT.md — 当前项目描述

> Sync status: current
> Updated: 2026-10-14
> Revision: `0a8835c8`
> Branch: `k3`
> Worktree: clean

## 项目身份

StarryOS 是使用 Rust 编写、基于 ArceOS 组件化架构的宏内核操作系统。本仓库同时承载内核入口、内核子系统、本地组件、平台适配、用户态测试程序和 OpenSpec 工程文档。

## 技术栈

- Rust edition 2024，工具链 `nightly-2026-02-25`。
- ArceOS `0.3.0-preview.2` 组件族；其中 `axtask` 以工作区自有副本（`crates/axtask`）参与构建。
- 目标架构包括 RISC-V 64、LoongArch 64、AArch64 与 x86_64；当前仓库包含以 RISC-V 平台为主的板级适配。
- 异步执行与唤醒能力由本地内核组件、工作区 `axtask` 的 schedulable run queue 与 remote-ready IPI，以及 `axtask::future` 等依赖提供。
- CPU mask 由工作区 `axtask::AxCpuMask` 封装私有 `cpumask 0.1.0`，debug 与 release 构建的越界访问均 fail closed。
- 构建入口由 Cargo workspace 与 Makefile 共同组成。

## 组成与职责

| 组成 | 职责 |
|---|---|
| `src/` | 顶层内核入口与产品组装 |
| `kernel/` | 内核主体、系统调用、VFS、设备、平台和测试支持 |
| `crates/axtask/` | 工作区自有 ArceOS 任务组件副本：schedulable run queue 集合、入队前 affinity、remote-ready IPI、`AxCpuMask` 安全封装 |
| `crates/axnet/`、`crates/smoltcp/` | 本地网络接口与协议栈实现 |
| `crates/axdriver_net/`、`crates/axdriver_virtio/`、`crates/virtio-drivers/` | 本地化网络驱动依赖（workspace patch，含 EVENT_IDX 通知控制与 transport-neutral queue contract） |
| `crates/uart_16550/` | 本地 UART 驱动实现 |
| `crates/axfs-ng/` | 本地文件系统组件 |
| `crates/axplat-riscv64-lichee-d1/` | Lichee RV Dock D1 平台组件 |
| `crates/axplat-riscv64-k3/` | SpacemiT K3（CoM260 Kit）平台组件（MS09+MS10+MS11+MS17 交付：板级基线 + AIA 中断底座 + GMAC DMA 地址/cache 基础 + runtime timer/UART42 IRQ 路由；MS18 交付：RAM rootfs 承载窗预留/校验 `ram_layout`；MS12 交付：`k3-async-net`/`k3-async-probe` 独立入口与 GMAC source-133 IOC 异步底座；MS13 交付：runtime stop/SWR/原地重建与 PHY pad35 双沿武装 + INSR/INTN ack（arm 清 pending + owner link 读消费 latch）） |
| `tests/`、`kernel/tests/`、`scripts/` | 用户态与内核侧测试载荷、MS08 SMP guest probe、MS09 板上采集程序与 MAC/PHY 探针 host harness、MS10 AIA/设备 IRQ 探针 host harness、MS11 GMAC 轮询驱动 host harness 与 k3-polling-net-host Cargo 测试包、MS17 内嵌 stdio/shell 载荷（`k3_stdio_probe.c`、`k3-shell.c`、`k3_shell_test.c`）与 k3-embedded-user-host Cargo 测试包、MS18 RAM-rootfs 布局契约测试（同包 `ram_rootfs.rs`）与 `tools/starry-k3-rootfs.its`、MS12 `k3-async-net-host` 生产 adapter/Service/stack-socket host 测试包、host serial/peer harness 与纯输出 validator、MS13 `recovery_contract`/`recovery_socket` 恢复契约与 socket 组合 host 测试及 phy/k3-gmac harness 扩展 |
| `openspec/`、`.agents/` | 规范、变更、项目记忆、分析和操作文档 |

## 支持范围与交付形态

- QEMU virt 是仓库内可配置的虚拟平台交付形态。
- QEMU virt 的单 hart VirtIO-MMIO 已具备 IRQ 唤醒、唯一双向 queue service、EVENT_IDX
  通知控制、固定容量 RX/TX packet slots、typed backpressure、TX completion/reclaim、
  ticketed C4 flush、常驻 stack runner 与 per-socket readiness bridge（多 waiter、
  listener accept bridge、terminal fault 发布），并已实现单 hart 恢复语义：epoch-scoped
  socket terminal、resident recovery owner、reset/link 下的 queue 重建（`Dma::new` 零化）、
  QueueEpoch/LinkGeneration/SocketEpoch 分层推进。MS06 单 hart 手工验收已接收；MS07 恢复语义
  Iteration 000–007 已 accepted。reset 的 SMP 放大、真板与性能资格不在该结论内。
- QEMU virt 的 `SMP=16` 多 hart 网络基线已 accepted：16 个 schedulable hart 上，网络 queue
  owner 与 stack runner 各自固定 singleton affinity，跨 hart 唤醒由 remote-ready IPI 直接驱动
  （含关闭目标 hart timer 的 witness 与 timer 恢复检查），TCP/UDP 双向帧交换、queue Full→
  恢复、poll/select/epoll readiness 与 quiet 窗口均有直接观测，descriptor/slot/ticket 账本
  闭合。结论限定 QEMU virt 的 VirtIO-MMIO 模型与 16 个同构虚拟 hart。
- UART 多 hart 实现、copier 固定 placement、snapshot 观测与 D1 有界 TX slow-poll workaround
  保留；UART 专项 runtime 资格未取得（console 输出只作为测试基础设施，不构成 UART 异步
  语义结论）。受控迁移稳定性、reset/link 交错、组合压力和真板性能均无结论。
- Lichee RV Dock D1 与 VisionFive 2 是仓库覆盖的 RISC-V 真实平台形态；SpacemiT K3 CoM260 Kit
  是异步 NIC 的目标板。MS09 板级基线已完成并归档：板级事实与安全 RAM 边界核对、FIT 临时 RAM
  启动可重复且回滚原系统、APMU 两步使能后 GMAC `version=0x1054` 可读、IO-PD 按 R69 用法
  APBC ASAR 解锁+整字写、MDIO Clause 22 读得真实 PHYID `0x001cc916`（RTL8211F）、120 s 窗口
  内 ≥5 个完整 link up→down→up 插拔循环可重复。结论不含设备 IRQ、DMA、网络收发与异步队列
  （MS10+ 范围）。VisionFive 2 支持和 ArceOS DWMAC 经验不构成该目标板证据。
- SpacemiT K3 的可诊断设备中断基线（MS10）已交付：AIA 软件投递底座（APLIC MSI + IMSIC
  S-mode，`stopei` 原子 claim+EOI 先于 handler，SETIPNUM/GENMSI 双入口、双 mask、retrap、
  unknown、风暴预算止损）与 PHY 设备 IRQ/status 基线（pin35 按 DTB 武装 GPIO+下降沿并释放
  CLEAR hold、INSR COR 前快照、真实插拔 source60/INSR bit4/link 对齐 claim、1 s quiet、
  MAC 133/277 只读观察）均真板验证。本板裁决事实：EIE 掩码窗口 MSI 丢弃（掩码只作止损、须
  触发后置掩码）、MFPR EDGE CLEAR 复位默认置位（hold-in-clear）、每物理事件一条 INSR=0 回声
  claim（分类器排除）。stage10 与 stage8 观察窗为 30 s（操作者指示）。结论不含 DMA 收发、
  异步 queue task、多 hart 流量与 timer/UART IRQ 的调度用途（MS11+/MS17 范围）。
- SpacemiT K3 的目标控制器轮询双向网络基线（MS11）已交付：独占 18 页 DMA arena 与地址/cache
  契约（checked 布局、线性 PA、Zicbom 发布/回收序、高位地址与 4 GiB 环检查、发布后 backing
  保留到外部复位）、16 slot 轮询 RX/TX（publish/reap/refill/wrap、单次回收、Full 不覆盖、
  分类 drop、阶段化 deadline/cancel/fail-stop owner 语义）、smoltcp 0.13.1 轮询 Device 与有界
  四协议会话（token 预留/背压、静态 socket、echo 1024B 上限、5 秒有界等待/close）与 stage13
  真板验收（60 s 会话窗 ping/UDP/TCP/ARP 交换成立，零 drop、16/16 terminal）。本板裁决事实：
  TX tail 为排他 fetch 上界（doorbell 写 next slot）、RX tail 写为唤醒 doorbell（fetch 由 OWN
  驱动）。结论不含异步 wake、运行时 reset/link-flap 恢复、SMP、offload 与性能（MS12+ 范围）。
- 根 Cargo features 提供 `qemu`、`lichee-d1`、`lichee-d1-async`、`vf2`、`k3` 与 `smp` 等产品组装入口。
- SpacemiT K3 的单 hart 异步串口与交互 shell 基线（MS17）已交付：`k3-async-uart`/`k3-uart-port`/`k3-uart`/`k3-stdio`/`k3-shell` 模式族（互斥、SMP>1 构建拒绝）沿 R68 RAM/FIT 启动；runtime timer（独立 handler 槽、set_timer(0) 首 tick、与 UART42 隔离）与 PXA UART port（115200 8N1、三步 FCR+PXAR8 触发线、Errata20 序、唯一 owner 激活）接入既有 copier/ring/tty/用户态契约——64KiB rings、check-register-arm-recheck、四条件物理 drain（末字节 TEMT 由 native 1 ms timer hook 推进）、tty canonical（空格/tab、ERASE/KILL、1024B 行界整行 discard、EOF 0-read）与内存根内嵌静态 ELF 用户态（stdio、失败分层未发布）；最小交互 shell（help/echo/sleep/exit、异常输入恢复）B7 板测两轮全场景通过（含 SIGINT 对照与 R68 复位）。本板裁决事实：FCR 触发 00 无 RX 中断推进（须 PXAR8）、APLIC SETIPNUM 对输入已 deassert 的 LevelHigh 源不生效、PXA IIR 对 IER 已掩码 cause 照报且 ISR 以 8-cause 预算收尾、`task.join()` 对 exit_group 恒 0（真实码在 process 层）、VERASE 默认 0x7f 与 Linux 默认一致。结论不含 GMAC 异步数据面（MS12/MS13）、多 hart（MS14）、存储/rootfs（MS18）与性能优化（I19）。
- SpacemiT K3 的 RAM rootfs 普通 shell 基线（MS18 RAM 部分）已交付：`k3-rootfs` 单 hart 模式按 D1-r1 预留布局（rootfs 承载窗 `[0x240000000,0x250000000)`、DTB 窗 `[0x2ef000000,0x2ef100000)`，post-kernel FREE 区连续性由 host 布局不变量长期承载）从临时加载的 ext4 RAM 镜像挂载根文件系统，经普通 loader 启动动态 BusyBox `/bin/sh`（musl 解释器）并执行文件系统外部命令；启动依赖缺失按层报告无 fallback（range 层板上见证，resolve/load 层 host 契约见证）、命令错误报告与前台 Ctrl-C 取消恢复板测观察、外部 RESET 回官方 Linux 用户实测；k3-rootfs 与 k3-stdio/k3-shell/raw-port 互斥、SMP>1 拒绝、独立 FIT（`StarryOS_riscv64-k3-rootfs.fit`）。本板裁决事实：axruntime 页分配器主区取 `.bss` 后首个 FREE 区，预留窗不得落于该区间（MS18 OOM 根因，host 不变量承载）；U-Boot 在新 DTB 窗装载/就地改写无异常。脚本/管道/重定向、易失写入对照、空闲编辑与 exit 收尾未取得正式见证（用户裁决取消 Iteration 002）；不含真实块设备存储（原 MS18 剩余范围）、GMAC 数据面、多 hart 与性能优化。
- SpacemiT K3 的单 hart 异步双向网络数据面（MS12）已交付：`k3-async-net`/`k3-async-probe` 独立入口（SMP>1 与 shell 组合拒绝）、GMAC source-133 IOC 底座（注册/重臂/重查协议、deferred owner、fatal latch；V4 run3 真板 20 ARP TX 真实 RI/TI 零丢、外部 RESET 回原系统）、`K3NetAdapter` 接入既有 NetDriverOps/queue/stack 契约（唯一 owner、64 slot/16 descriptor 账本、两级 Full→恢复、C4 flush Pending→release→C4，V3 run3 板测全项）、stack runner + socket 载荷（UDP 26001/TCP 26002 全字节 echo、TCP 主动关闭 EOF）与真板四协议（两轮 ping 10/10、UDP/TCP 全字节回显、quiet 经沉降相后 2 s 计数逐字节冻结直证；孤立 resume 行按用户裁决豁免——唤醒路径由全程逐包真实中断行使）。本板裁决事实：quiet 观察窗前须先经沉降相吸收 TCP 关闭尾流（claim 流静止 500 ms 再开窗），宿主 axhal 无平台后端（`wall_time()≡0`）故 timer wheel 仅真板/QEMU 可验。结论不含真板 reset/link-flap 恢复与 DMA quiesce 证明（MS13）、多 hart（MS14）与性能优化（I19）。
- SpacemiT K3 的单 hart 网络恢复语义（MS13）已交付：受控恢复矩阵在 k3-async/probe 独立入口运行——18 页 arena 原地 stop/SWR/重建、QueueEpoch/cookie 旧新隔离、resident owner、旧 socket connection-reset 终态与 flush abort、容量第 1 轮 81 帧两级边界、C4 flush、真实 source-60 拔插事件（含插回上升沿可达：RTL8211F INTN 电平型，arm 时清 INSR pending、owner link 读路径消费 latch，ISR 保持无 MDIO）与插回后 new-2 真实逐字节 UDP/TCP 会话交换；host 生产契约全绿。本板裁决事实：pad35 MFPR 复位即双沿武装（运行时写幂等）、INTN 置低后靠读 INSR 复位、事件路径依赖 owner 消费无兜底轮询。残余按用户收口裁决留档不追：容量/C4 第 2 轮、fail-stop 末场景、矩阵形态 new-2 判定行、插回后 link 不稳定三候选（事件通路单次失手/末次 Down 门永久关/DMA 静默停摆，有 trace 证据未定位）。结论不含多 hart（MS14）、长稳与性能（I19）。
- 交付物包括可启动内核镜像、平台构建产物，以及配套的内核态和用户态测试载荷。

## 仓库现场

- 当前 Git 分支为 `k3`，HEAD 为 `0a8835c8`；工作树 clean。
- 当前无活跃 change；归档目录最新保留 MS13 单 hart 网络恢复语义（`archive/2026-10-14-ms13-com260-kit-single-hart-network-recovery/`）、MS12 异步双向数据面（`archive/2026-10-13-ms12-com260-kit-async-bidirectional-data-plane/`）与 MS11 轮询网络、MS17 异步串口/交互 shell、MS18 RAM rootfs 普通 shell 产物。当前行为规格包括 `openspec/specs/k3-com260-kit-single-hart-network-recovery/`、`k3-com260-kit-async-network-data-plane/`、`k3-com260-kit-polling-network-baseline/`、`k3-com260-kit-async-uart-interactive-shell/` 与 `k3-com260-kit-standard-shell-rootfs/`；项目支持范围仍以已验收行为为准。
- 已知遗留：vf2 构建入口 pre-existing 失败（`axfs`/`axnet`/`axtask` import 解析错误）为 Issue 候选未落账；SNAPSHOT 此前记录的 `make host-test` ms04 失败在当前工作区不复现（2026-10-07 host-test exit 0）。
- 详细里程碑状态见 [`tasks.md`](tasks.md)。

## 权威入口

- 公共流程和编辑规则：[`AGENTS.md`](../../AGENTS.md)
- Milestone 与任务状态：[`tasks.md`](tasks.md)
- 活跃及归档变更：[`openspec/changes/`](../../openspec/changes/)
- 项目模型、决策、知识、参考与改进：[`openspec/specs/`](../../openspec/specs/)
