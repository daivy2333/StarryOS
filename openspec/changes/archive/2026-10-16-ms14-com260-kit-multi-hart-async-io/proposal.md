## Why

CoM260 Kit 已交付网卡与串口的单 hart 异步基线；本变更的 000–003 轮已交付 K3 多 hart 平台、NIC/UART 专项及可构建的双设备 RAM rootfs 组合。最后一轮在 16 hart BusyBox shell 中验收真实使用，并按用户 2026-10-16 新增要求测量串口有效吞吐、对比 StarryOS 与原厂 Linux 的网卡吞吐；不设最低性能门槛。

当前项目描述见 `.agents/docs/SNAPSHOT.md`；路线与范围依据 `.agents/docs/tasks.md` 的最终多 hart 阶段。SNAPSHOT 的工作树记录已滞后，本轮历史现场为 2026-10-14、分支 `k3`、HEAD `9179205d`。

## What Changes

- 接通 16 个 AP hart 的启动、per-CPU 初始化、timer、S_SOFT IPI 和固定外设 IRQ 路由；排除 RP/RT24。
- 复用已有调度器、critical-section、placement、UART copiers、NIC owner/stack 和恢复控制面，不引入第二 executor、owner 或通用驱动框架。
- 保留 000–002 已完成的 UART/NIC 多 hart 专项及审查结果，不重新验收。
- 保留既有恢复实现、安全资源生命周期和必要研发；仅当缺陷阻碍最终 shell 的正常使用时，调查实际可达路径并给出有限修复契约，不预选 ISS02/ISS03 的修复。
- 建立可返回的 NIC 安装/启动步骤，与 UART copiers、普通 ext4 RAM rootfs、动态 BusyBox shell 组合；处理固定 affinity、子进程 affinity 继承、paging 下的 MMIO 地址与 axklib 唯一实现，以及单 tty reader。
- 承接 I20 中的 RAM rootfs 用户态网络工具范围，复用镜像内的 `wget` 和 `nc`，经内核 socket 使用异步 GMAC。用户选择 TCP/UDP，`ping`/新增 ICMP socket 留待后续。
- 取消后续独立恢复矩阵、双轮容量/C4、故障注入资格、quiet/readiness 压力、30 分钟长稳及历史全套回归门槛；研发先行测试、改动所需定向测试与构建保留。
- 最终功能验收观察 BusyBox shell 串口交互、`wget` HTTP 下载、`nc` TCP/UDP 双向交换及同一启动中串口与网络共同可用；性能测量由下述新增范围单列。不声明取消项目已经验证，不声明性能收益。
- 用户随后明确增加短时性能测量：固定 115200 8N1 测 UART RX/TX 有效载荷吞吐及线速占比；同一板子、物理网口、网线与 Windows PC 对端，对比 StarryOS 与原厂 Linux 的单连接 TCP 发送/接收吞吐。只测量并报告差异，不优化、不设最低占比，不恢复已取消的资格矩阵。

## Capabilities

### New Capabilities

- `k3-com260-kit-multi-hart-async-io`：K3 AP 多 hart 平台、已交付 NIC/UART 专项与 BusyBox RAM rootfs 串口/网络最终使用形态。

### Modified Capabilities

None。既有单 hart 规格及其已验收/豁免边界保留；新增独立多 hart 模式，不把历史单 hart 或 QEMU 结果改写为本板资格。

## Approved Requirements List

| ID | 要求 | 可观察验收 |
|---|---|---|
| R1 | 16 个 AP hart 启动与安全失败 | 0–15 各有实际调度进度；HSM 拒绝时明确停止，不伪报在线 |
| R2 | 本地 timer/IPI 与固定外设路由 | 每 hart timer 推进；远端 IPI 可重复、ack 后无风暴；任意 caller 的设备 mask/unmask 不操作错误 IMSIC 文件 |
| R3 | 固定 affinity 与事件直接唤醒 | 四后台角色唯一；实际 poll hart 与 affinity 一致；目标 timer 关闭后远端事件仍唤醒并恢复 timer；覆盖 X100/A100 跨簇 |
| R4 | UART 多 hart 契约 | 双向内容/次序、FIFO/ring wrap、短写/Full→恢复、readiness、错误/drop、物理 drain 与取消/timeout 成立 |
| R5 | NIC 多 hart 稳态契约 | 四协议、64 slot/16 descriptor 容量、预算公平、C4、poll/select/epoll 与 quiet 成立 |
| R6 | 保留既有恢复与安全边界；仅修最终使用阻塞 | 不删除既有恢复能力，不引入第二 owner、不释放所有权未明的 DMA backing；取消多 hart 恢复专项资格 |
| R7 | 两设备最终组合 | 同一个 16-hart RAM/FIT 启动中，普通 BusyBox shell 的输入/编辑/输出与真实网络交换均正常；取消 30 分钟及恢复交错资格 |
| R8 | 兼容与易失启动边界 | 旧模式和既有共享契约保持；仅执行修改所需定向测试/构建；RAM 镜像与启动无持久写入，外部 RESET 保留为回滚手段，不另设逐轮回滚验收 |
| R9 | BusyBox RAM rootfs 用户态网络 | `/bin/sh`、`wget`、`nc` 从 ext4 RAM rootfs 加载；wget 下载内容正确，nc 完成 TCP/UDP 双向交换；不绕过 syscall/socket/异步 GMAC |
| R10 | UART 短时 RX/TX 性能测量 | 115200 8N1、每方向 256 KiB 实际串口数据，报告有效 B/s、8N1 线速利用率及有效数据 bit/s 占波特率比例；发送以 PC 实收为结束，接收以板上读完为结束，不以软件入队速度代替 |
| R11 | 网卡与原厂系统对比 | 同板同物理链路、同一 Windows 对端、相同 BusyBox nc/dd 负载与接收端计时，测单连接 TCP 两方向，各固定 3 个 32 MiB 样本；报告原值、中位数、范围及 StarryOS/Linux 比例，不设最低性能门槛 |

历史批准为“采用完整范围和 30 分钟长稳”。2026-10-16 的下列裁决替代其尚未执行的恢复/组合范围；已交付 000–002 的契约、Act Response 和 Review 不变。

- 用户：“其中研发需要改代码的工作仍旧保留，但是整体各种验证的工作取消，我们改为bb rootfs shell的最终形态验证串口和网卡工作正常来作为最终验证”。
- 对保留必要研发和定向测试、取消专项验收矩阵、把 I20 纳入最终形态的建议，用户：“认可你的判断，那接下来进行工作规划的调整”。
- 工具集中决策：用户选择“wget + nc：验证 TCP/UDP，ping 留作后续方向（推荐）”。
- 默认沿用并已随上述建议认可：16 hart、RAM rootfs、异步 UART/GMAC、固定角色、无持久存储；真板操作继续由用户手动完成。
- 接受的覆盖缺口：不再要求 reset/拔插/取消完成交错的完整矩阵、故障注入资格、双轮 Full/C4、quiet/多 waiter 压力、30 分钟长稳或完整历史回归。最终正常使用不能证明这些行为，收尾不得表述为已验证。既有取消/超时/资源安全语义不因删去资格而删除。

最后一轮新增范围的用户原话：“现在展开最后一轮，另外本轮工作有额外补充就是，需要一些串口和网卡性能的测试，串口的话我们看能够达到波特率的百分之几，网卡的话我们要和板子的原本的网卡测速进行比对什么的，这个测试工作也放进来吧因为不是什么大的任务，开始计划”。集中决策已完成：用户选择“仅测量与对比，不设最低性能门槛（推荐）”，并选择“Windows 原生 Python / PowerShell，沿用当前串口和网卡（推荐）”。本指令授权更新未执行的 004 范围、展开计划；不追溯改写 000–003，不授权实施。I19 的本次短时测量子范围由此纳入；优化、CPU/p99、offload、多流/UDP 性能及完整性能矩阵仍不承诺，不修改全局 I 文档。

## Scenario Sketch

| 场景 | 前置状态 | 动作 | 结果与失败边界 |
|---|---|---|---|
| S01 Happy：16 AP 启动 | 原固件、IFX DTB、临时 FIT | 拉起 AP 0–15 并运行任务 | 每 hart 实际调度；不得以 configured 数或 DTB 代替 |
| S02 Sad：启动拒绝 | 无 HSM 或 hart_start 返回错误 | 请求次级启动 | 在平台层明确报告并停止；不继续声称完整 SMP |
| S03 组合：全局/本地初始化 | 主 hart 已初始化 APLIC | 次级 hart 初始化 IMSIC/timer/IPI | 不重置全局域、不清其他 hart 文件或已注册 handler |
| S04 Edge：timer 等待/取消 | 两个有期限等待 | 取消其一并等另一到期 | 只清自身等待，其他任务按真实 timer 恢复 |
| S05 Sad：IPI 重复/空提示 | 目标 idle/blocked | 两次独立远端 wake、重复 wake | 真 ready 转换产生 IPI；重复不造风暴，SSIP ack 后可再收 |
| S06 组合：关闭目标 timer | 跨簇目标 task 已 Blocked | 远端发布真实唤醒 | 目标收到 IPI并执行；每条退出路径恢复本地 timer |
| S07 组合：路由与重臂 | IRQ 固定 boot hart，owner 远端 | 注册后远端 mask/unmask，最后设备事件 | 原目标可继续投递；不依赖下一包或误改调用 hart EIE |
| S08 Edge：非法 affinity/重复启动 | 未发布或越界 mask；已有角色 | 启动或重复启动 | 失败不入队/不毒化状态，不创建第二角色 |
| S09 组合：跨簇执行 | 全部 AP schedulable | 固定角色与跨簇应用交换事件/数据 | 实际 hart 观察覆盖 X100/A100，不按 mask 推断执行 |
| S10 Happy：UART 双向边界 | 唯一 RX/TX copier 与 tty | 真实串口输入/输出跨 FIFO、ring 与 wrap | 内容/次序正确，角色唯一 |
| S11 Edge：短 RX/行输入 | 无完整行或少量字节 | 末字节后不再输入；编辑、EOF、poll | 最后事件可达，readiness 与实际 read 一致 |
| S12 Sad：UART TX Full/drain | ring 满、物理 TX 未空 | 短写后释放容量并 drain | 只重试未接受字节；四条件全部成立才成功 |
| S13 Sad：RX 过载/公平 | 延迟 reader、双向 backlog | 超容量入流后恢复消费 | 有界分类 drop/overrun，两方向与 timer 仍推进；不承诺任意过载零丢失 |
| S14 Edge：UART 取消/timeout | read/write/drain Pending | 取消、竞争登记或 deadline 到期 | 等待有界结束，已接受字节与唯一 owner 保留 |
| S15 Happy：NIC 四协议 | owner/runner/app 分布多 hart | 原生 ARP/ICMP/UDP/TCP 交换 | 真实字节/关闭成立，不靠 caller 主动 poll |
| S16 Sad：两级 Full/C4 | 软件 slot 或硬件 descriptor 满 | 额外提交、释放、flush | typed 背压、容量恢复；submit/terminal/reclaim 与 ticket 闭合 |
| S17 组合：多 waiter/readiness | TCP/UDP/listener 与多个等待者 | poll/select/epoll、取消与 I/O | wake 后的 I/O 状态一致，无覆盖或丢 wake |
| S18 Edge：quiet 后新流量 | 尾流沉降、无到期协议工作 | quiet 2 s 后发新请求 | 无持续数据面 poll；新事件仍推进；背景流量不伪装 quiet |
| S19–S23 原恢复专项 | 运行时 reset、拔插、取消/完成、停止失败、门关闭入流 | SKIPPED：按上述用户裁决取消后续专项验证与预设故障修复 | 既有实现和 DMA backing 安全边界保留；只有最终使用实际阻塞才定向调查，不声明多 hart 恢复资格 |
| S24 组合：最终 shell 使用 | 四后台角色唯一、BusyBox shell 已启动 | 在该 shell 执行 wget/nc，并从串口输入与读取网络结果 | 同一镜像内 UART 与 GMAC 正常工作，shell 可继续输入命令；不计时、不执行恢复矩阵 |
| S25 原长稳恢复交错 | 原 30 分钟与两轮恢复设计 | SKIPPED：按用户裁决取消 | 不要求 Up/Down reset、30 组命令、末尾容量/C4 资格 |
| S26 兼容：旧模式 | 只修改必要组装或共享调用边 | 执行该修改的先行测试、定向测试与直接受影响构建 | 旧语义保持；未变表面采信，不运行全套历史矩阵 |
| S27 退出/取消/回滚 | shell 退出或操作者停止 | 保留既有结束处理与外部 RESET 手段 | 不确定 backing 不释放，无持久写入；不再另设逐轮 RESET 真板验收 |
| S28 Happy：普通 rootfs shell | RAM ext4/DTB 预留窗、动态 BusyBox/musl 已装载 | 普通 loader 启动 `/bin/sh` 并运行外部命令 | 真实 BusyBox 提示符与输出；缺失 bootargs、镜像或解释器时按层报错，无内嵌 shell fallback |
| S29 Happy：wget | host 静态 IPv4 提供已知文本 HTTP 文件 | shell 执行 `wget -Y off -T 5 -O /tmp/bb-download http://192.168.137.1:8000/hello.txt`，读文件 | 退出成功且文件内容与提供内容一致；限定局域网 HTTP，不要求 DNS/TLS/Internet |
| S30 组合：nc TCP/UDP | host 提供普通 TCP/UDP echo 对端 | shell 用 `nc -n -w 3`，分别选 TCP 与 `-u` 输入并接收短文本 | 两种 socket 均真实收发相同内容，命令结束后 shell 继续可用；UDP 对端须存活到数据报回复 |
| S31 Edge：shell 子进程与 deadline | 初始 shell singleton 已提交 | shell 经 clone/clone3 创建 wget/nc/sleep，再 exec | 子任务入队前继承父 cpumask，exec 不改变它，deadline 生命周期不跨 hart；定向先行测试覆盖真实 spawn 调用边 |
| S32 Sad：启动或工具错误 | 非法 affinity、NIC 启动失败、缺失 rootfs 文件或对端报错 | 生产路径返回错误 | 不重复初始化或 fallback；保留既有 errno/错误输出与 timeout/cancel 语义。错误边界按改动做定向测试，不新设真板故障注入验收 |
| S33 Happy：串口 TX 吞吐 | 组合 shell 已正常使用；Windows 独占 COM，115200 8N1 无流控 | BusyBox dd 发送 256 KiB 零字节，PC 持续按字节接收 | 首个有效字节到末字节实收计时，报告 B/s 与两个百分比；不采用 dd 入队结束时间，不在 GUI 渲染吞吐 |
| S34 Happy：串口 RX 吞吐 | 唯一 tty 已保存设置并切 raw/-echo，dd 已前台等待 | PC 连续发送 256 KiB 零字节，板上 dd 完整读入 RAM 文件 | 计时从 PC 首次发送到板上 dd 完成记录抵达 PC；报告含短回执开销的有效吞吐，恢复原 tty 并确认文件长度/内容与 shell 响应 |
| S35 组合：两系统 TCP 性能 | 同板同线同 PC，RAM 数据已准备；实际链路条件有记录 | StarryOS 与原厂 Linux 分别运行相同 BusyBox nc/dd 单连接负载，两方向各固定 3 次 | 每次实际累计接收完整 32 MiB并测时间；报告每次值、两方向中位数/范围及比值；链路/调度/offload差异作为解释，不归为纯驱动收益 |
| S36 Sad/Edge：测速无效 | 串口短收/夹杂日志、端口占用、TCP 超时/少字节或原厂基线缺失 | 原生命令报错或实际数据不完整 | 保留直接现象，停止该项并交接；不把无效数值/缺失基线记为完成，不改波特率/调优/自动复测直到满意 |

场景扫描覆盖正常、错误、边界、组合及超时/取消/兼容路径；S19–S23/S25 和 S26/S27 的缩减已由用户批准。已完成的 S01–S18 仅保留历史专项结论，本次不重复执行。S28/S29/S30/S24 及 S33–S36 必须来自真实板上操作。UART/TCP 分方向短时测量不构成同时双向饱和、恢复、长稳或最大硬件性能资格。

## Impact

- 平台：`crates/axplat-riscv64-k3` 的 boot/init/power/time/timer_irq/irq/aia 与特性接线。
- 组装：root/kernel Cargo、Makefile、`kernel/src/entry.rs`、平台入口、rootfs/FIT、paging/axklib 唯一实现及 clone 子任务 affinity。
- 设备：`k3_async_net`、`k3_gmac::adapter`、既有 UART port/copier/tty；共享 axnet 仅修改验收必需的可达缺陷。
- 验证：仅改动所需的既有测试入口、先行测试和构建，加一次最终 BusyBox shell 真板正常使用；不创建 validator、身份协议、专用判定工具或独立资格矩阵。
- 最后一次 StarryOS 真板会话同时完成正常使用与 UART/TCP 测量，另一次原厂 Linux 阶段取得同板 TCP 基线。使用现有 BusyBox 和 Windows 原生 I/O 命令，不新增测速框架、专用 CLI 或判定脚本；原生结果与算术比较写实施反馈。
- 不写全局 tasks、SNAPSHOT、M/R/I、Issue 或 Runbook；不修改参考仓库和板载固件。

## Phase and Gate Status

| 阶段 | 状态 |
|---|---|
| 需求提取、场景缺口扫描 | completed |
| 用户集中决策 | completed：最后004新增性能测量，无最低门槛，Windows原生对端；既有wget+nc和缩减批准保持 |
| 需求基线与场景草图 | completed：R1–R9保持，新增R10/R11及S33–S36，既有已取消覆盖不恢复 |
| Gate 1 | PASS：用户明确展开最后一轮及性能范围，集中决策已处理 |
| 实现调查与设计 | completed：复用003 accepted，补查socket/tty/clock/devfs与实际工具CLI/计时，见design D9及004 |
| 任务分轮与 RTM | completed：最后004保留T12/T13，新增T14/T15；无Missing，既有Simplified批准保持 |
| 004 展开 | completed：004/000-initial ready，用户2026-10-16原话“批准”；000–003历史冻结保持 |
| Gate 2 | PASS：用户已批准具体计划，尚未调用Act |
| 产品实施与真板测试 | SKIPPED：本次只规划；用户态工具只读检查不计产品结果 |
| 文档验证 | completed：`openspec validate ms14-com260-kit-multi-hart-async-io --strict` exit0；`git diff --check`无输出 |
| 经验落账、全局状态、归档 | SKIPPED：不在本次授权范围 |
