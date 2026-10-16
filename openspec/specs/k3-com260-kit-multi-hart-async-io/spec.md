# k3-com260-kit-multi-hart-async-io Specification

## Purpose
规定 CoM260 Kit 在 16 hart SMP 下的平台底座、固定角色数据面与最终用户态组合行为：AP 启动与本地 timer/IPI/固定外设路由、UART/NIC 唯一角色跨 hart 唤醒与双向稳态、BusyBox RAM rootfs 普通 shell 中的串口交互与 wget/nc 真实网络使用，以及同板原厂 Linux 的单连接 TCP 吞吐对比。合并限于实际见证的行为——UART 线速测量按用户 2026-10-16 裁决取消（原话"uart的测试不做"），无板上 UART 吞吐见证，该要求与其场景不进入语料库；TCP 对比按用户单样本裁决完成（原话"我们不做什么多次测试，有当前数据已经足够"），每方向 1 个有效样本，原 3 样本中位数口径随裁决放宽；已取消的多 hart 恢复/长稳/资格矩阵不表述为已验证。000–003 轮专项结论保留于归档 change。

## Requirements

### Requirement: AP 多 hart 启动与失败边界

新增 K3 多 hart 模式 MUST 使用现有板载固件与 RAM/FIT 启动，接通 AP hart 0–15 的独立栈、per-CPU、内存管理及调度初始化；MUST 只把真正就绪的 run queue 发布为 schedulable。HSM 不可用或 hart_start 拒绝 MUST 在平台层报告并停止，不得虚报完整 SMP。RP/RT24、hotplug 和持久固件替换不属于该能力。

#### Scenario: S01 16 AP 实际执行

- **WHEN** 操作者沿既有 RAM/FIT 路径启动多 hart 模式
- **THEN** 系统 MUST 在 AP 0–15 分别实际运行任务并观察调度进度
- **AND** configured 数、DTB 声明或启动打印 MUST NOT 代替实际执行

#### Scenario: S02 次级启动拒绝

- **WHEN** HSM 不可用或 hart_start 返回错误
- **THEN** 系统 MUST 明确报告 hart 与失败阶段并停止后续启动，不声称完整 SMP

### Requirement: 本地中断与固定外设路由

APLIC 全局域 MUST 只初始化一次，各 hart MUST 只初始化自己的 IMSIC、timer 和 S_SOFT。IPI MUST 复用 axtask 的唯一 reschedule handler，ack 后可重复投递且无风暴。timer handler 注册与本 hart 使能 MUST 分离，并支持本地 mask/unmask。SMP runtime timer/IPI handler MUST 常驻，运行期全局unregister拒绝且无副作用；局部mask不移除其他hart的handler。外设 IRQ 可固定 boot hart，但其他 hart 的控制调用 MUST 不误改自己的 IMSIC 文件；设备 mask/unmask 后最后事件必须可达。

#### Scenario: S03 次级初始化不破坏全局域

- **WHEN** boot hart 已初始化 APLIC，次级 hart 初始化本地中断
- **THEN** 系统 MUST 保留全局路由和 handler，只改变对应本地文件

#### Scenario: S04 timer 与取消

- **WHEN** 多 hart 有真实 deadline 等待且其中一个被取消
- **THEN** 系统 MUST 只清该等待，并以真实 timer 推进其余等待

#### Scenario: S05 重复 IPI

- **WHEN** 两次独立远端 ready 转换及重复 wake 到达目标 hart
- **THEN** 合法转换 MUST 获得 IPI 调度机会，重复 wake 不额外造风暴，ack 后后续投递仍成立

#### Scenario: S07 跨 hart 外设重臂

- **WHEN** IRQ 固定 boot hart而远端 owner 控制通知与 APLIC mask/unmask
- **THEN** 最后一次真实设备事件 MUST 到达原目标并推动 owner，调用 hart 的本地文件不得被当作目标文件

### Requirement: 唯一固定角色与事件直接唤醒

系统 MUST 复用既有 placement 和入队前 singleton affinity，为 UART RX/TX copier、NIC queue owner/stack runner 分别建立唯一角色；mask 必须来自已发布 schedulable 集合。非法 affinity、重复启动或 spawn 拒绝 MUST 不创建第二角色或毒化可重试状态。实际执行与跨簇唤醒 MUST 直接观察；远端 ready 不依赖目标周期 timer。关闭目标 timer 的验证在所有退出路径 MUST 恢复 timer。

#### Scenario: S06 无目标 timer 的跨簇唤醒

- **WHEN** 跨簇目标 task 已 Blocked 且本地 timer 关闭，远端发布唤醒
- **THEN** task MUST 因 remote-ready IPI 获得调度，并在终态前恢复 timer

#### Scenario: S08 非法或重复角色启动

- **WHEN** mask 越界、未发布，或角色重复启动
- **THEN** 系统 MUST 明确拒绝，不入队非法 task、不生成第二 owner/copier

#### Scenario: S09 实际跨簇执行

- **WHEN** 后台角色和应用以固定 affinity 跨 X100/A100 交换事件与数据
- **THEN** 实际 poll/任务执行 hart MUST 与 affinity 一致且覆盖两簇，不按 configured mask 推断

### Requirement: UART 多 hart 字节流与物理完成

PXA UART MUST 复用唯一 port、共享硬件锁、copier/ring/tty，保留 SPSC、字节次序、FIFO/ring wrap、短写、readiness 和有界公平。少量 RX 的最后事件必须可达；过载损失必须分类且容量有界。drain 只有 ring empty、copier inactive、staged bytes 为零及 hardware transmitter empty 全部满足时才成功。timeout/cancel 只结束等待，不撤销已接受 TX 或重新初始化设备。

#### Scenario: S10 双向跨边界字节流

- **WHEN** 真实串口双向 I/O 跨 FIFO/ring 与 wrap
- **THEN** 系统 MUST 保持正确内容、次序及唯一 producer/consumer

#### Scenario: S11 末次短输入与行 readiness

- **WHEN** 少量末字节后无新输入，或用户执行行编辑、EOF 与 poll
- **THEN** 真实 UART 通知 MUST 推进末次输入，readiness 与实际 read 一致

#### Scenario: S12 Full 后恢复与 drain

- **WHEN** TX Full 产生短写，随后 copier 释放空间且物理发送完成
- **THEN** writer MUST 只重试未接受字节，drain 必须等待全部四条件

#### Scenario: S13 过载与公平

- **WHEN** RX 超容量且双向 backlog 持续，随后恢复消费
- **THEN** 系统 MUST 有界分类 drop/overrun并恢复接收，两方向与 timer 均有进度

#### Scenario: S14 等待取消或超时

- **WHEN** read/write/drain 等待被取消、竞争登记或超过 deadline
- **THEN** 系统 MUST 返回既有明确结果，保留已接受字节和唯一 owner

### Requirement: NIC 多 hart 稳态数据面

GMAC MUST 沿用既有 adapter、DMA/cache、16 descriptor、64 packet slot、唯一双向 owner 与独立 stack runner；ISR 只处理 cause/ack/mask/wake。真实四协议、两级 typed Full→恢复、单次回收、ticketed C4、多 waiter/readiness及 quiet MUST 在多 hart 成立；无工作时不得依赖 completion 周期轮询或 busy self-wake。

#### Scenario: S15 四协议真实交换

- **WHEN** 多 hart owner/runner/app 经 GMAC 与物理对端运行 ARP/ICMP/UDP/TCP
- **THEN** MUST 完成真实双向内容及 TCP 正常关闭，不靠 caller 主动 poll

#### Scenario: S16 两级容量与 C4

- **WHEN** 软件 slot 或 descriptor 达到容量上限，额外提交后释放空间并 flush
- **THEN** MUST 返回 typed 背压、恢复容量，并单次闭合 packet/descriptor/buffer/ticket；C4不得等同 peer delivery

#### Scenario: S17 多 waiter 与 readiness

- **WHEN** TCP/UDP/listener 的多个 waiter 进行 poll/select/epoll、取消和实际 I/O
- **THEN** MUST 不覆盖或丢失有效 waiter，通知与紧随其后的 I/O 状态一致

#### Scenario: S18 quiet 后恢复

- **WHEN** 尾流沉降且无到期协议工作后安静 2 s，再发送新请求
- **THEN** MUST 无持续数据面 poll/self-wake，新真实 IRQ 仍推动交换；不得隐藏背景流量

### Requirement: 既有网络恢复与资源安全边界

最终组合 MUST 复用既有 resident owner、queue/link/socket 分层与恢复实现，不得因取消专项资格而删除恢复代码或引入第二 owner。DMA 所有权不明确时 MUST 保留 backing，不得将等待结束当作设备停止或 C4。

本要求的后续验收已由用户缩减为保持现有实现与新增修改的定向检查；不新增或声明多 hart reset/拔插、取消/完成交错、门关闭入流、双轮容量/C4 和故障注入资格。具体批准与原场景处置见 proposal。

#### Scenario: 新组合初始化失败

- **WHEN** NIC 硬件初始化、IRQ 注册、owner/runner 启动或 Active 等待失败
- **THEN** 系统 MUST 按既有错误边界停止后续 shell 启动，保留已发布资源，不重复初始化 Service 或创建第二 owner

#### Scenario: 正常使用遇到设备错误

- **WHEN** BusyBox 网络工具读写过程中设备报告既有故障
- **THEN** 系统 MUST 沿既有 socket 错误路径传播结果并保留不确定 backing，不得创建第二 owner、伪造传输成功或无限自动 reset

### Requirement: BusyBox shell 与双设备最终组合

新增 `k3-smp-combined` MUST 在同一 16-hart RAM/FIT 启动中运行唯一 NIC owner/runner、UART RX/TX copiers 和普通 BusyBox RAM rootfs shell；MUST 保持单 tty reader、固定 singleton affinity、paging 下唯一 axklib provider 及既有 DMA/cache 契约。初始 shell MUST 通过普通动态 loader 从 ext4 RAM rootfs 加载，不得以内嵌最小 shell 或内核网络载荷代替最终形态。旧独立模式的 feature 边界 MUST 保持。

本要求取消原 30 分钟长稳、定时 reset/link 交错及末尾专项账本资格；最终验收只观察正常用户使用，不声明这些被取消的覆盖已通过。

用户随后增加的短时性能测量由 R10/R11 单独规定，不恢复上述已取消资格。

#### Scenario: S24 同一 shell 中串口与网络正常使用

- **WHEN** 用户在 BusyBox 提示符输入/编辑命令、使用 wget/nc 交换数据并继续输入命令
- **THEN** UART 输入输出及经 GMAC 的真实数据内容 MUST 正确，网络工具运行后 shell MUST 继续响应
- **AND** 不得用另一个镜像、内嵌最小 shell、kernel echo 载荷或 configured mask 代替该最终使用

#### Scenario: S28 普通 RAM rootfs 启动

- **WHEN** 临时加载的 RAM ext4 与 DTB 通过现有 initrdmem/布局校验
- **THEN** 系统 MUST 挂载唯一 rootfs、启动普通 `/bin/sh`、使用异步 UART tty，并运行文件系统中的外部命令
- **AND** 缺少启动依赖 MUST 在对应层报告，不能 fallback 为内嵌 shell；rootfs 与 DTB 窗保持既有预留布局

#### Scenario: S31 用户子任务继承固定 affinity

- **WHEN** 固定 affinity 的初始 `/bin/sh` 经 clone/clone3 创建 login shell、wget、nc 或 sleep 并 exec
- **THEN** 子任务 MUST 在首次入队前继承父 cpumask，exec 保持它，不能因默认 full mask 使 deadline 生命周期迁移到其他 hart

### Requirement: 兼容、直接验证与易失启动

新增模式 MUST 保持既有单 hart K3、QEMU/D1 与共享调用契约；pre-existing 失败独立说明。研发 MUST 保留先行测试、实际修改所需的最小定向测试/构建；已通过且未变化表面的结论直接采信，不附加完整历史回归门槛。最终验收 MUST 直接观察真实 BusyBox 命令、输出与文件内容，不新增验证身份协议、validator 或专用判定层。

临时 RAM/FIT 运行 MUST NOT 修改板载持久固件/介质；保留外部 RESET 回官方系统的既有手段和退出时的资源安全边界。后续不另设逐轮 RESET 或 EOF/SIGINT/drain 资格；没有实际执行的验证不得写为通过。

#### Scenario: S26 修改所需兼容检查

- **WHEN** 组装、网络包装器、rootfs 或共享 clone 发布路径发生修改
- **THEN** MUST 对修改做先行测试及必要定向检查，保持旧语义与平台隔离；不重复未失效验证，不将 pre-existing 失败归入本次修改

#### Scenario: S27 结束、取消与回滚

- **WHEN** shell 退出、启动失败或用户结束临时运行
- **THEN** 系统 MUST 保持既有结束处理与不确定 backing 的保留，操作者可沿原外部 RESET 回滚；不得发生持久写入或宣称未执行的恢复验收通过

### Requirement: 用户态 wget 与 nc 经异步 GMAC 通信

最终 RAM rootfs MUST 使用现有 BusyBox wget/nc applets，经标准 syscall/FD/poll、axnet SocketOps 与本仓异步 GMAC 实现局域网 HTTP、TCP 和 UDP 通信；不得直接访问 MMIO 或绕过内核数据面。目标使用静态 IPv4 与普通对端，不要求 DNS、DHCP、TLS、IPv6、Internet、ping/ICMP socket 或完整 BusyBox/Linux 兼容。

#### Scenario: S29 wget 下载

- **WHEN** host 提供已知内容 HTTP 文件，用户在板上 shell 执行 wget 并读取下载文件
- **THEN** wget MUST 成功退出，文件内容 MUST 与对端提供内容一致，shell 可继续使用

#### Scenario: S30 nc TCP 与 UDP 双向交换

- **WHEN** 用户从板上 shell 分别以 nc TCP 和 nc UDP 向普通 echo 对端输入短文本
- **THEN** 两种 socket MUST 真实发送并收到相同内容，经串口看到回复；结束后 shell MUST 继续可交互

#### Scenario: S32 工具错误边界

- **WHEN** 已选工具在创建 socket、连接、读写或等待中返回既有错误、超时或取消结果
- **THEN** 系统 MUST 保持对应 errno/错误输出与资源所有权语义，不伪造数据或成功；必要修改按真实可达缺口定向测试，不增加独立真板故障矩阵

### Requirement: 同板原厂 Linux 的 TCP 吞吐对比

本轮 MUST 对 StarryOS 和该板原厂 Linux 的实际有线网络路径各测单连接 TCP 板→PC、PC→板有效吞吐，沿用当前板口、网线和 Windows PC。两侧 MUST 使用相同 BusyBox nc/dd 负载、RAM 数据与接收端计时方法；按用户 2026-10-16 单样本裁决每方向采集 1 个 33554432-byte 完整样本，报告实际接收字节、时间与同方向 StarryOS/Linux 比例（原 3 样本/中位数/min-max 口径随裁决放宽）。MUST 记录影响解释的实际条件与默认配置差异，不能宣称该对比隔离了纯驱动性能；不得将环境元数据作为运行身份判定。

没有最低性能比例要求，不新增性能优化、UDP性能、多流/同时双向饱和、CPU/p99、长稳或完整资格矩阵。

#### Scenario: S35 两系统的分方向接收吞吐

- **WHEN** 用户在组合 shell 与原厂 Linux 分别执行单连接 TCP 发送/接收负载（32 MiB），Windows 提供普通数据对端与接收端计时
- **THEN** MUST 以实际接收的 33554432 bytes 与接收端时间计算有效 Mbps，分别报告两个方向的原值与同方向比例
- **AND** MUST 不把发送方缓冲接受速度当作对端收到的吞吐，不择优删样本或自动复跑直到满意

#### Scenario: S36 测量失败或原厂基线缺失

- **WHEN** 原生命令超时、提前 EOF、数据损坏、串口占用，或原厂 Linux 尚未执行
- **THEN** MUST 保留实际结果和未完成项，不把缺失数据写成零或已完成，不伪造对比
- **AND** 新接口/工具/恢复设计需求 MUST 返回 Plan；性能数值低本身不触发修复或优化
