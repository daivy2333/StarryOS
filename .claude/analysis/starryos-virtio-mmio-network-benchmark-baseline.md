# StarryOS VirtIO-MMIO 轮询网卡基线测试分析

> Project: StarryOS
> Branch: net-k3
> Date: 2026-07-31
> Analysis revision: `c7df9fbbd5dac855e79cfaf019ad325dc371fb96`
> Status: MS03-a 规划输入，不是已批准 change
> See also: [网络开发总览](async-network-project-overview.md)、[MS03 设计](../../openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/design.md)、[QEMU 网络 Runbook](../runbooks/qemu-network-testing.md)、[MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md)、[UART benchmark](../../tests/benchmark.c)、[MS02 Evidence](../../openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/README.md)

本文回答一个遗漏的问题：异步网卡开发前，如何测量当前轮询网卡。目标是记录可复现的性能基线，并让 MS04 以后复用同一套测试。

本文不实现 benchmark，不产生性能结论，也不修改 MS03。建议在 MS03 运行 Gate 完成后创建 MS03-a，再记录正式基线。

## 结论

当前测试只能证明网络功能可用。它还不能回答吞吐、延迟、抖动、丢包、CPU 和背压成本。

[MS01](../../tests/ms01_socket_baseline.c) 主要测试回环 socket 语义。[MS02](../../tests/ms02_guest_service.c) 证明 QEMU user-net TCP/UDP 与 TAP ARP/ICMP 可用。MS02 只有一项性能相关数据：空闲 QEMU 进程在 30 秒内使用约 100% 至 111% 单核 CPU。该结果没有负载对照，也没有吞吐归一化。

MS03-a 应先建立测试工具和证据格式，再测量当前实现。正式基线应采用以下边界：

- TAP 是性能主拓扑。
- QEMU user-net 只做兼容回归。
- guest loopback 只做协议栈对照。
- 吞吐以接收端确认的有效数据为准。
- 延迟只测同一时钟域内的 RTT。
- QEMU 进程 CPU 是当前主 CPU 指标。
- `/proc/instret` 只作单 hart 整机代理。
- MS03 快照用于记录 IRQ 机制成本。
- 所有 A/B 必须使用同一后端和环境。
- QEMU 启动和 guest 命令保持手工执行。

MS03-a 不应开始异步 RX。它应冻结测试协议、完成轮询基线，并测出环境噪声。MS04 再用相同协议比较异步实现。

## 当前轮询基线

当前数据流如下：

```text
guest socket syscall
  -> axnet TCP/UDP
  -> poll_interfaces()
  -> Service::poll()
  -> Router + smoltcp
  -> EthernetDevice
  -> VirtIoNetDev
  -> VirtIO-MMIO queue
  -> QEMU net backend
  -> host peer
```

[Service](../../crates/axnet/src/service.rs) 在设备无 IRQ capability 时注册 10 ms fallback。[EthernetDevice](../../crates/axnet/src/device/ethernet.rs) 以 `irq_num().is_none()` 判断是否需要轮询。MS03 保持这个数据面，只增加 IRQ 诊断控制面。

当前设备边界为：

| 项目 | 当前值 | 测试影响 |
|---|---:|---|
| VirtIO queue pair | 1 | 不能把结果解释为多队列能力 |
| RX queue | 64 | burst 应覆盖 32、64、128 包 |
| TX queue | 64 | 背压测试应跨越 64 包边界 |
| 驱动 buffer | 1526 B | 基线使用 1500 MTU 内负载 |
| TCP RX/TX buffer | 各 64 KiB | 写入矩阵应覆盖 64 KiB |
| UDP RX/TX buffer | 各 64 KiB | datagram burst 应观察满队列 |
| UDP metadata | 256 项 | burst 应覆盖 255、256、257 |
| listener queue | 512 | 连接压力与稳态吞吐分开测 |
| IP packet burst | 64 | 单次 service 进度可能受此边界影响 |

VirtIO 驱动协商 `MAC`、`STATUS`、`RING_EVENT_IDX` 和 `RING_INDIRECT_DESC`。它不启用 checksum、GSO、TSO、UFO、MQ 或 mergeable RX buffer。基线 manifest 必须记录实际协商结果，不能只记录源码支持值。

当前 socket 行为也限制指标解释：

- TCP `send()` 返回表示数据进入 socket buffer。
- 它不表示 peer 已收到数据。
- UDP `send()` 也不能证明 datagram 已送达。
- `SO_SNDBUF` 和 `SO_RCVBUF` 设置尚未生效。
- `TCP_INFO` 尚未提供有效统计。
- `TCP_NODELAY` 可用，必须显式记录。

因此，发送端字节只能作为 enqueue 指标。正式 goodput 必须来自接收端报告。

## 测试拓扑

三个拓扑不得合并统计。

| 拓扑 | 用途 | 能证明什么 | 不能证明什么 |
|---|---|---|---|
| guest loopback | socket 与协议栈对照 | syscall、buffer、smoltcp 上层成本 | VirtIO、MMIO、QEMU backend |
| QEMU user-net | 兼容与易运行 smoke | NAT、hostfwd 下的功能 | 设备路径本身的上限 |
| QEMU TAP | 正式性能基线 | guest 到 host 的完整 VirtIO 路径 | 真板 DMA、cache、PHY |

QEMU user-net 在 QEMU 进程内增加用户态网络栈。入站连接还依赖 hostfwd。QEMU 官方文档也说明其 ICMP 能力受限。它适合 smoke，不适合作为主性能结果。

TAP 将 guest NIC 接到主机虚拟接口。主机可以运行原生 peer、抓包和流量整形。正式吞吐、RTT、丢包和 CPU 基线应固定使用 TAP。

建议保留两个 TAP 方向：

```text
TX: guest sender -> VirtIO -> TAP -> host receiver
RX: host sender  -> TAP -> VirtIO -> guest receiver
```

双向测试同时运行两条数据流。单向结果先通过，双向结果才有解释价值。

MS03-a 必须记录以下拓扑事实：

- QEMU 完整命令。
- QEMU 版本和机器类型。
- `virtio-net-device` 与 MMIO 地址。
- bus、IRQ、寄存器宽度和 stride。
- `NET_DEV`、TAP 名称和 IP。
- MTU、MAC、guest IP、host IP。
- SMP、内存、`ICOUNT` 和日志级别。
- rootfs、boot arguments 和 payload 来源。
- hostfwd、vhost、offload 是否启用。

墙钟性能必须使用 `ICOUNT=n`。QEMU 官方文档明确指出 `icount` 不是周期精确模型，也不等同于实际性能。

## Runbook 约束

[QEMU 网络 Runbook](../runbooks/qemu-network-testing.md) 是当前硬性政策。QEMU 启动、guest shell 命令和运行见证必须手工执行。禁止用 script、pipe、pexpect 或自动化框架驱动 QEMU。

这不禁止确定性 payload。guest 和 host benchmark 都由用户手工启动。payload 内部可执行固定 workload，但不能启动 QEMU、注入串口命令或消费 shell prompt。

[MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md) 可提供 MS03-a 的前置阶梯：

| 阶段 | MS03-a 继承内容 | 失败处理 |
|---|---|---|
| 1 | fmt、unit、feature tree、target build、OpenSpec、diff check | 任一失败即停止 |
| 2 | 无 hostfwd 启动与 MMIO probe | 保存完整串口日志 |
| 3 | user-net TCP/UDP 兼容 smoke | 等 `READY` 后手工刺激 |
| 4 | TAP ARP/ICMP 与 pcap | 检查路由并清理 TAP |
| 5 | 30 秒 idle CPU | 保存原始采样，不预设阈值 |
| 6 | MS01 14/14 与 MS02 回归 | 有 `FAIL` 即停止 |
| 7 | TAP benchmark | 前六阶段通过后执行 |

QEMU benchmark 必须位于 boot 和 smoke 之后。每条 guest 命令要在 prompt 返回后单独输入，避免内核日志打断串口输入。

payload 继续使用静态 RISC-V 编译。Evidence 记录编译命令、退出码、文件路径和 SHA-256。若通过 HTTP 下载，server 必须在 `tests/` 启动，并监听 `0.0.0.0`。

[回归 Gate](../runbooks/regression-gate.md) 的“QEMU 只做相对比较”继续适用。该 Runbook 中的 1 B UART latency、64 B UART TX 和 drain 阈值不适用于网卡。MS03-a 应记录 QEMU 环境基线，但不能把绝对值当成真板目标。

## Benchmark 工具

建议复用 [UART benchmark](../../tests/benchmark.c) 的结构，不复用串口语义。可迁移的是 manifest、warm-up、多轮样本、分位数、错误计数和完成点。

UART 的 `tcdrain`、线速和 FIFO 边界不能迁移。网卡需要接收端确认、packet 序号、socket 背压和 descriptor 边界。

MS03-a 可规划以下产物：

| 产物 | 作用 |
|---|---|
| `tests/network_benchmark.c` | guest 与 host 共用的原生 peer |
| `tests/network_benchmark_protocol.h` | 固定控制协议和记录头 |
| `scripts/network-benchmark-report.py` | 从原始记录生成 CSV/JSON 摘要 |

同一 C 程序应支持 client/server、TCP/UDP、TX/RX、RTT 和校验模式。host 使用本机编译，guest 使用静态 RISC-V 编译。这样可减少两端协议漂移。

host peer 可接收用户提供的 QEMU PID，并采样该进程。它不能生成 QEMU 命令或控制 guest。report 脚本只读取已经完成的日志。

不规划自动 runner。若以后要改变手工政策，应先用新证据更新 QEMU Runbook，再规划自动化 change。

每轮测试需要一个独立控制握手：

```text
双方交换版本与参数
  -> receiver 就绪
  -> warm-up
  -> 测量屏障
  -> data transfer
  -> receiver 完成校验
  -> 返回接收字节、包、错误和时间
  -> 双方输出固定记录
```

数据记录至少包含 test ID、round、方向、协议、payload、flow 数、seed 和配置 hash。输出使用固定 `key=value` 记录。原始日志始终保留，解析结果不能替代原始记录。

TCP stream 使用确定性 payload 和滚动校验。测试循环必须处理 partial send、partial recv、`EAGAIN`、超时和 EOF。TCP RTT 使用带长度、序号和校验的 application record。

UDP datagram 需要 test ID、序号、payload 长度和校验。接收端报告 missing、duplicate、reordered、corrupt 和 late datagram。发送端报告 requested、accepted 和 syscall error。

## 测试项目

测试分为必测基线、机制诊断和扩展压力。MS03-a 可先完成必测项，再决定扩展项。

| ID | 项目 | 参数 | 主要结果 | 等级 |
|---|---|---|---|---|
| N00 | manifest | 每次运行一次 | 环境与配置 hash | 必测 |
| N01 | 计时校准 | clock、空循环、instret 读取 | 测量开销 | 必测 |
| N02 | loopback 对照 | TCP/UDP，固定矩阵 | 上层软件上限 | 必测 |
| N10 | TCP 单向 goodput | TX/RX，1 flow | 接收 goodput、CPU | 必测 |
| N11 | TCP write size | 1 B 至 64 KiB | syscall/byte、goodput | 必测 |
| N12 | TCP 双向 | 1 flow/方向 | 双向 goodput、公平性 | 必测 |
| N13 | TCP 多流 | 1、2、4、8 flows | 聚合与单流 goodput | 必测 |
| N20 | TCP RTT | 1、64、512、1400 B | p50/p95/p99/max | 必测 |
| N21 | UDP 单向 goodput | TX/RX，受控速率 | goodput、loss、pps | 必测 |
| N22 | UDP RTT 与间隔误差 | 1、64、512、1400 B | RTT、抖动代理 | 必测 |
| N23 | UDP burst | 32、64、128、255、256、257 包 | loss、reorder、恢复 | 必测 |
| N30 | 非阻塞背压 | send 到 `EAGAIN` | accepted、等待、恢复 | 必测 |
| N31 | 队列边界 | 32、64、128 包 burst | ring 边界效应 | 机制 |
| N32 | 连接周转 | 串行与并发 connect/close | connects/s、失败率 | 扩展 |
| N33 | 多流公平 | 2、4、8 TCP flows | 单流分布、min/max | 扩展 |
| N40 | 空闲成本 | 无 socket、idle socket | QEMU CPU、IRQ | 必测 |
| N41 | CPU 效率 | TCP/UDP TX/RX/bidir | CPU-s/GiB、inst/byte | 必测 |
| N42 | IRQ 效率 | MS03 snapshot delta | IRQ/packet、IRQ/GiB | 机制 |
| N43 | 调度干扰 | timer idle vs load | wake overshoot 分位数 | 必测 |
| N44 | 内存稳定性 | churn 与短 soak | RSS、allocator 前后差 | 扩展 |
| N50 | 网络损伤 | delay/loss/reorder/rate | 降级与恢复 | 扩展 |
| N51 | 稳定运行 | 固定负载 5 分钟 | stall、loss、资源增长 | 扩展 |

TCP write size 建议使用：

```text
1, 64, 256, 512, 1024, 1460, 4096, 16384, 65536 B
```

UDP 的有效 payload 建议使用：

```text
1, 64, 256, 512, 1024, 1400, 1472-H B
```

`H` 是固定 benchmark header 长度。header 与有效 payload 的总和不得超过 1472 B。超过该值的分片行为应作为独立功能测试，不能混入主性能曲线。

小 TCP RTT 必须固定 `TCP_NODELAY=1`。吞吐模式也要记录该选项。小写入矩阵可额外比较 Nagle 开关，但不能把两个结果混合。

UDP 最大速率不能只跑无节制 flood。建议先发现无损区间，再按该结果的固定比例施加 offered load。每档同时报告 offered、accepted 和 received。

网络损伤只在 TAP 上运行。Linux `netem` 支持 delay、jitter、loss、duplicate、reorder、rate 和固定 seed。整形方向必须记录；TCP 场景还要避免把错误 qdisc 位置当作接收端入站损伤。

## 指标与完成点

同一名称必须只有一个计算口径。

| 指标 | 定义 | 主要来源 |
|---|---|---|
| TCP goodput | receiver 校验字节 × 8 / 测量时间 | receiver |
| UDP goodput | 唯一且校验通过字节 × 8 / 时间 | receiver |
| enqueue rate | sender 已接受字节 / sender 时间 | sender |
| packet rate | 校验通过的 record 或 datagram / 时间 | receiver |
| RTT | origin 发出到匹配 reply 返回 | origin monotonic clock |
| RTT tail | p50、p95、p99、max | 原始 RTT 样本 |
| 间隔误差 | 接收间隔与发送计划间隔之差 | receiver |
| loss | 缺失唯一序号 / 已发送序号 | 双端摘要 |
| duplicate | 重复序号计数 | receiver |
| reorder | 小于最高已见序号的首次到达 | receiver |
| corruption | 长度或校验失败 | receiver |
| QEMU CPU | QEMU CPU seconds / wall seconds | host `/proc/<pid>/stat` |
| CPU 效率 | QEMU CPU seconds / receiver GiB | host + receiver |
| guest 指令效率 | instret delta / receiver bytes | guest `/proc/instret` |
| IRQ 效率 | MS03 counter delta / packet 或 GiB | IRQ snapshot + receiver |
| 恢复延迟 | `EAGAIN` 到再次可写并推进 | sender monotonic clock |
| timer 干扰 | 绝对唤醒 overshoot | guest monotonic clock |

TCP 的发送完成分三层：

```text
send() 返回
  -> 数据进入 socket buffer
peer recv() 返回
  -> 数据到达 peer socket
peer summary/ACK 返回
  -> 本轮 receiver-confirmed completion
```

N10 至 N13 使用第三层。第一层只进入 N11 enqueue 指标。不能用 `send()` 返回时间替代链路 goodput。

RTT 不需要两端时钟同步。一端发送并接收 reply 即可。当前不应报告 one-way latency，因为 guest 与 host 没有已验证的同步时钟。

“抖动”需要明确口径。本文建议同时报告 RTT 分布和 UDP 接收间隔误差。只写一个 `jitter` 平均值会隐藏长尾。

## CPU 与可观测性

当前最可靠的 CPU 读数来自 host。手工启动的 host peer 可读取指定 QEMU PID 的 `utime + stime`，并按 `CLK_TCK` 换算。peer 进程要单独采样，不能并入 QEMU CPU。

QEMU CPU 指标包含 guest 执行、设备模拟和网络 backend。它不是网卡驱动独占成本。通过 idle/load 配对和 CPU-s/GiB，才能比较轮询与异步的整机代价。

guest 的 `/proc/instret` 可读取 RISC-V retired instruction。当前单 hart 下，它近似覆盖整个 guest 测量窗口。它不是进程指令数，也不是周期数。每次运行要测两次连续读取的开销，并保留原始 begin/end。

`CLOCK_PROCESS_CPUTIME_ID` 和 `CLOCK_THREAD_CPUTIME_ID` 当前共用线程 `TimeManager`。代码还标注了 preemption 不更新 timer state。该数据暂时只能观察，不能作为 MS03-a Gate。

现有 `/proc/interrupts` 只输出 timer callback 计数。它不能提供 NIC IRQ 计数。MS03 完成后的只读 snapshot 才是网卡 IRQ 来源。

当前 Ethernet 路径没有以下计数：

- ingress/egress packet 与 byte。
- device receive/transmit error。
- queue full 与 buffer allocation failure。
- service poll 次数和单轮预算。
- socket buffer 高水位。
- descriptor occupancy 与 completion。

这意味着外部 peer、序号和 pcap 是当前正确性真值。日志 warning 不能替代单调计数。

MS03-a 可评估 feature-gated telemetry。最小集合建议放在 axnet owner 边界：

- service poll 与 progress 次数。
- RX/TX frame 与 byte。
- RX/TX error 和 allocation failure。
- 单轮最大 packet 数。
- pending queue full。

不应仅为 benchmark 本地化 registry VirtIO driver。若需要 descriptor occupancy，Plan 应单独评估该改动对基线的扰动。

Relaxed atomic counter 也有成本。建议先做一个代表性 workload 的 telemetry on/off 对照。若差异超出噪声，headline 数据使用关闭版本，机制分析使用开启版本。

内存只能作为稳定性代理。host 记录 QEMU RSS；guest 可记录全局 allocator 使用前后差。当前没有可靠的 per-NIC 或 per-process 内存统计。

## 测量协议

建议的正式 profile 为：

| 阶段 | 参数 |
|---|---|
| 计时校准 | 每种读取 1000 次 |
| warm-up | 2 秒，不计入结果 |
| steady sample | 10 秒 |
| rounds | 5 个有效 round |
| round 间隔 | 2 秒 idle |
| RTT sample | 5 组，每组 200 次 |
| idle sample | 30 秒 |
| short soak | 5 分钟 |

这些值是 MS03-a 的起始建议，不是性能阈值。Plan 可根据一次 pilot 的总耗时调整，但必须固定到 manifest。

每个 round 都输出原始结果。吞吐按 round 报告 median、min 和 max。延迟按样本报告 p50、p95、p99 和 max。错误、短写、超时与无效样本单独计数。

无效 round 不得静默重跑。原始记录保留失败原因。补跑使用新的 round ID。

不要删除 outlier。环境干扰应通过 host load、CPU 绑定和 round 记录解释。必要时重新取得完整数据集，不能只保留好看的样本。

基线前应固定：

- source revision 与 dirty 状态。
- benchmark 版本和二进制 SHA-256。
- toolchain、QEMU、host kernel。
- host CPU 型号与虚拟化环境。
- QEMU 与 peer CPU affinity。
- 网络 backend 与 MTU。
- SMP、内存、日志和 `ICOUNT`。
- payload、duration、round、seed。
- telemetry 和 MS03 IRQ 状态。

当前环境有 `cc`、`taskset`、`tc`、`tcpdump` 和 Python。`iperf3` 与 `pidstat` 当前未安装。MS03-a 不应把后两者设为硬依赖。

`iperf3` 可作为方法参考。它区分 sender/receiver 结果，支持 reverse、bidirectional、parallel、warm-up omission 和 JSON。StarryOS guest 不应为了基线先移植 iperf3。

## Evidence 与 Gate

运行顺序必须是静态检查、QEMU boot、网络 smoke、回归、benchmark。benchmark 不能替代前面的设备与协议见证。

QEMU 阶段属于用户能力边界。用户手工启动 QEMU、输入 guest 命令并启动 host peer。离线 parser 可以在运行结束后处理日志。

每次正式运行应保存：

| 文件 | 内容 |
|---|---|
| `README.md` | 环境、命令、Gate、限制 |
| `manifest.txt` | 完整配置与 hash |
| `qemu-command.txt` | 展开后的 QEMU 命令 |
| `qemu.log` | 完整串口与 QEMU 日志 |
| `guest-netbench.log` | guest 原始记录 |
| `host-netbench.log` | host 原始记录 |
| `host-cpu.csv` | QEMU 与 peer CPU/RSS |
| `irq-snapshots.log` | MS03 前后快照 |
| `capture.pcap` | TAP 数据包见证 |
| `results.csv` | 逐 round 规范化数据 |
| `summary.json` | 汇总和有效性状态 |

QEMU `filter-dump` 能生成 libpcap 文件。MS02 已发现 user-net UDP hostfwd 未进入该 pcap。TAP 基线应同时使用 TAP 侧抓包，避免再次把缺失 capture 当作无流量。

性能 Gate 前先过正确性 Gate：

- 双端配置 hash 一致。
- receiver 完成全部校验。
- TCP 没有未解释 EOF、timeout 或 short transfer。
- UDP loss、duplicate、reorder 与场景预期一致。
- MS03 没有 unknown cause、残留或 IRQ storm。
- MS01 14/14 与 MS02 TCP/UDP 回归仍通过。

首轮基线不应预设绝对性能阈值。先用 5 个 round 得到环境波动。MS03-a Review 再决定回归阈值，且要写明统计口径。

QEMU 结果只支持当前 host、QEMU 版本、单 hart 和 VirtIO 设备模型。它不能作为 VisionFive2 DMA、cache、PHY 或真板吞吐证据。

## 后续 A/B 协议

轮询基线记为 B0。异步候选记为 A1、A2。每个候选必须复用同一 benchmark 版本。

推荐使用成对运行：

```text
B0 -> A1 -> A1 -> B0
```

每组交替顺序，减少 host 漂移。至少保留 5 对有效结果。每对必须使用同一 TAP、QEMU 参数、affinity、payload 和测量时长。

比较顺序如下：

1. 正确性与丢包。
2. 空闲 QEMU CPU。
3. TCP/UDP receiver goodput。
4. RTT p99 和 max。
5. CPU-s/GiB 与 instret/byte。
6. IRQ/packet 与 IRQ/GiB。
7. 背压恢复和 timer 干扰。

任何一项只改变一个变量。不能同时更换 backend、SMP、queue size 和调度模型。

MS04 的主要预期不是单纯提高峰值带宽。它还应降低无流量时的轮询成本，并保持 tail latency、丢包和背压语义。最终 Gate 要由 B0 波动决定，不能在没有数据时写死百分比。

若 telemetry 结构在异步实现中变化，公共指标仍以 receiver、host CPU 和 pcap 为准。机制计数只在定义相同或明确换算时比较。

## MS03-a 的建议边界

MS03-a 建议位于 MS03 与 MS04 之间。它的输入是 MS03 已通过的诊断 IRQ 和仍由 MS02 推进的轮询数据面。

建议目标：

- 建立 guest/host 共用 benchmark 协议。
- 建立 TAP 性能运行流程。
- 记录 N00 至 N43 的必测基线。
- 量化当前环境波动。
- 固定 MS04 以后使用的 A/B 规则。

建议非目标：

- 异步 RX/TX queue task。
- 删除 10 ms fallback。
- 改变 queue size 或 socket buffer。
- 零拷贝、多队列或 offload。
- 用 QEMU 数据推断真板性能。
- 为取得好看数据修改网络行为。

MS03-a 若需要低开销 axnet telemetry，应把它当成测量设施。实现必须 feature-gated，并先量化自身开销。该改动不能改变 packet ownership、poll 频率或 socket 语义。

当前未确认项：

| 问题 | 规划时需要决定 |
|---|---|
| QEMU 自动化政策 | 只自动 host peer，还是批准 console 自动化 |
| headline 是否启用 telemetry | 取决于 on/off 开销 |
| TCP 吞吐的 Nagle 默认值 | pilot 后固定，必须进入 manifest |
| UDP offered-load 档位 | 由无损 pilot 推导 |
| regression 阈值 | 由 B0 round 波动推导 |
| netem profiles | 作为扩展 Gate 还是独立 change |
| descriptor 计数 | 是否值得本地化 driver seam |

## 关键文件与资料

| 来源 | 用途 |
|---|---|
| [axnet service](../../crates/axnet/src/service.rs) | poll、timer fallback、进度循环 |
| [Ethernet device](../../crates/axnet/src/device/ethernet.rs) | RX/TX owner 与 polling capability |
| [TCP socket](../../crates/axnet/src/tcp.rs) | completion、buffer、TCP options |
| [UDP socket](../../crates/axnet/src/udp.rs) | datagram buffer 与 metadata |
| [network constants](../../crates/axnet/src/consts.rs) | 64 KiB、256、512 等边界 |
| [time syscall](../../kernel/src/syscall/time.rs) | monotonic 与 CPU clock 边界 |
| [procfs](../../kernel/src/pseudofs/proc.rs) | instret 与 interrupts 现状 |
| [QEMU make rules](../../make/qemu.mk) | backend、filter-dump、icount |
| [MS03 tasks](../../openspec/changes/ms03-virtio-mmio-diagnostic-irq-baseline/tasks.md) | IRQ snapshot 与手工 Evidence |
| [MS02 idle CPU](../../openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/idle-cpu.txt) | 现有轮询成本样本 |
| [QEMU 网络 Runbook](../runbooks/qemu-network-testing.md) | 手工 QEMU 硬性政策与 payload 下载 |
| [MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md) | MMIO、user-net、TAP、CPU 和回归阶梯 |
| [回归 Gate](../runbooks/regression-gate.md) | QEMU 相对比较与真板证据边界 |
| [QEMU networking](https://www.qemu.org/docs/master/system/devices/net.html) | TAP 与 user-net 语义 |
| [QEMU command reference](https://www.qemu.org/docs/master/system/qemu-manpage.html) | filter-dump 与 icount 边界 |
| [iperf3 documentation](https://software.es.net/iperf/invoking.html) | 双端、反向、多流和 JSON 方法 |
| [tc-netem manual](https://man7.org/linux/man-pages/man8/tc-netem.8.html) | delay、loss、reorder、rate 与 seed |
