# StarryOS VirtIO-MMIO 轮询网卡基线测试分析

> Project: StarryOS
> Branch: net-k3
> Date: 2026-08-04
> Analysis revision: `2a9319a946dbe9c07cb0f448d82c0b7c14069015`
> Status: MS16 规划输入，不是已批准 change
> See also: [网络开发总览](async-network-project-overview.md)、[异步网络路线](starryos-async-network-roadmap.md)、[网络开发策略](starryos-network-development-strategy.md)、[MS03 设计](../../openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/design.md)、[QEMU 网络 Runbook](../runbooks/qemu-network-testing.md)、[MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md)、[MS03 Runbook](../runbooks/ms03-virtio-mmio-irq-evidence.md)、[UART benchmark](../../tests/benchmark.c)、[MS02 Evidence](../../openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/README.md)

本文定义异步网卡开发前的轮询基线。目标是固定 workload、指标、完成语义和 Evidence，使 MS04、QEMU 和真板沿用同一套测试口径。

本文不实现 benchmark，也不产生性能结论。MS01、MS02 和 MS03 已归档，roadmap 已将 MS16 放在 MS03 与 MS04 之间。

## 结论

当前测试只能证明网络功能可用。它还不能回答吞吐、延迟、delay variation、丢包、CPU、指令、复制和背压成本。

[MS01](../../tests/ms01_socket_baseline.c) 主要测试回环 socket 语义。[MS02](../../tests/ms02_guest_service.c) 证明 QEMU user-net TCP/UDP 与 TAP ARP/ICMP 可用。MS02 只有一项性能相关数据：空闲 QEMU 进程在 30 秒内使用约 100% 至 111% 单核 CPU。该结果没有负载对照，也没有吞吐归一化。

MS16 应先固定测试协议和证据格式，再测量当前实现。正式基线采用以下边界：

- TAP 是性能主拓扑。
- QEMU user-net 只做兼容回归。
- guest loopback 只做协议栈对照。
- 吞吐以接收端确认的有效数据为准。
- 延迟只测同一时钟域内的 RTT。
- QEMU 进程 CPU 是当前主 CPU 指标。
- `/proc/instret` 只作单 hart 整机代理。
- MS03 快照用于记录 IRQ 机制成本。
- 外部可观测指标与内部诊断指标分开。
- 每项结果记录明确的完成点。
- QEMU 与真板分别建立平台基线。
- 所有 A/B 必须使用同一后端和环境。
- QEMU 启动和 guest 命令保持手工执行。

MS16 不开始异步 RX。它冻结测试协议、完成轮询基线，并测出环境噪声。MS04 再用相同协议比较异步实现。

测试体系分为三层：

```text
统一 workload 协议
  -> 平台测量适配层
  -> 统一 Evidence 与报告 Schema
```

workload 协议不依赖驱动。平台适配层提供 QEMU host、guest、真板和可选驱动遥测。报告层只消费版本化原始记录。

以后更换轮询、异步、VirtIO 或真板设备时，只替换适配能力。workload、测试 ID、公式和无效样本规则保持不变。

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

## 完成语义与测量边界

网络发送至少有六个完成点：

```text
C1 syscall 返回
  -> C2 socket 或协议栈接受
  -> C3 descriptor 提交或 doorbell
  -> C4 descriptor 完成并回收
  -> C5 peer 网络栈收到
  -> C6 peer 应用校验并回复摘要
```

每项记录必须包含 `completion_point`。同名指标不得混用不同完成点。

| 测量 | 完成点 | 解释 |
|---|---|---|
| syscall/enqueue latency | C1 | 用户态调用与缓冲接受成本 |
| stack acceptance | C2 | socket 与协议栈推进成本 |
| device submit latency | C3 | queue 提交成本，需要遥测 |
| device completion latency | C4 | descriptor 回收成本，需要遥测 |
| peer receive | C5 | peer socket 可读，不含应用校验 |
| goodput 与完整性 | C6 | 接收端确认的有效结果 |

`send()` 返回不代表 C4 或 C6。`flush`、`drain` 和“发送完成”也必须声明目标完成点。

内部完成点属于可选诊断能力。外部 C1 与 C6 指标必须在没有驱动遥测时仍可运行。

## 统一能力模型

benchmark 在握手时交换 capability bitmap。缺失能力写为 `unavailable`，不能填零。

| 能力组 | 轮询 QEMU | 异步 QEMU | 真板 | 用途 |
|---|---:|---:|---:|---|
| socket workload | 必须 | 必须 | 必须 | TCP/UDP 行为与 goodput |
| receiver 校验 | 必须 | 必须 | 必须 | 正确性真值 |
| host CPU/RSS | 必须 | 必须 | 不适用 | QEMU 整机代价 |
| guest instret | 单 hart必须 | 单 hart必须 | 可选 | 指令效率 |
| IRQ snapshot | 必须 | 必须 | 可选适配 | IRQ 效率 |
| poll/service telemetry | 可选 | 不适用 | 可选 | 无进展轮询与批量 |
| wake/scheduler telemetry | 不适用 | 可选 | 可选 | IRQ 到任务运行 |
| descriptor telemetry | 可选 | 可选 | 可选 | queue residence 与回收 |
| DMA/cache/PLIC telemetry | 不适用 | 不适用 | 可选 | 真板机制成本 |
| per-hart CPU accounting | 当前缺失 | 当前缺失 | 待适配 | guest CPU utilization |

capability 缺失不自动使功能测试失败。要求该能力的 profile 才应阻塞。

## 测试拓扑

三个拓扑不得合并统计。

| 拓扑 | 用途 | 能证明什么 | 不能证明什么 |
|---|---|---|---|
| guest loopback | socket 与协议栈对照 | syscall、buffer、smoltcp 上层成本 | VirtIO、MMIO、QEMU backend |
| QEMU user-net | 兼容与易运行 smoke | NAT、hostfwd 下的功能 | 设备路径本身的上限 |
| QEMU TAP | 正式性能基线 | guest 到 host 的完整 VirtIO 路径 | 真板 DMA、cache、PHY |
| 真板外部 peer | 板级性能与可靠性 | DMA、cache、PLIC、PHY 和链路 | QEMU 设备模型成本 |

QEMU user-net 在 QEMU 进程内增加用户态网络栈。入站连接还依赖 hostfwd。QEMU 官方文档也说明其 ICMP 能力受限。它适合 smoke，不适合作为主性能结果。

TAP 将 guest NIC 接到主机虚拟接口。主机可以运行原生 peer、抓包和流量整形。正式吞吐、RTT、丢包和 CPU 基线应固定使用 TAP。

建议保留两个 TAP 方向：

```text
TX: guest sender -> VirtIO -> TAP -> host receiver
RX: host sender  -> TAP -> VirtIO -> guest receiver
```

双向测试同时运行两条数据流。单向结果先通过，双向结果才有解释价值。

MS16 必须记录以下拓扑事实：

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

QEMU 与真板不能比较绝对吞吐、绝对 RTT 或绝对 CPU。它们分别建立 B0，并比较轮询到异步的相对变化。

跨平台可以比较正确性、完成语义、backpressure、instructions/bit 趋势、IRQ/packet、batch size 和 copy amplification。报告必须保留平台标签，不能生成合并排名。

## Runbook 约束

[QEMU 网络 Runbook](../runbooks/qemu-network-testing.md) 是当前硬性政策。QEMU 启动、guest shell 命令和运行见证必须手工执行。禁止用 script、pipe、pexpect 或自动化框架驱动 QEMU。

这不禁止确定性 payload。guest 和 host benchmark 都由用户手工启动。payload 内部可执行固定 workload，但不能启动 QEMU、注入串口命令或消费 shell prompt。

[MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md) 可提供 MS16 的前置阶梯：

| 阶段 | MS16 继承内容 | 失败处理 |
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

[回归 Gate](../runbooks/regression-gate.md) 的“QEMU 只做相对比较”继续适用。该 Runbook 中的 1 B UART latency、64 B UART TX 和 drain 阈值不适用于网卡。MS16 应记录 QEMU 环境基线，但不能把绝对值当成真板目标。

## Benchmark 工具

建议复用 [UART benchmark](../../tests/benchmark.c) 的结构，不复用串口语义。可迁移的是 manifest、warm-up、多轮样本、分位数、错误计数和完成点。

UART 的 `tcdrain`、线速和 FIFO 边界不能迁移。网卡需要接收端确认、packet 序号、socket 背压和 descriptor 边界。

MS16 可规划以下产物：

| 产物 | 作用 |
|---|---|
| `tests/network_benchmark.c` | guest 与 host 共用的原生 peer |
| `tests/network_benchmark_protocol.h` | 固定控制协议和记录头 |
| `tests/network_benchmark_platform.h` | 时钟、计数器和平台能力适配 |
| `scripts/network-benchmark-report.py` | 从原始记录生成 CSV/JSON 摘要 |
| `scripts/network-benchmark-evidence.py` | 检查 Evidence 完整性与比较资格 |

同一 C 程序应支持 client/server、TCP/UDP、TX/RX、RTT 和校验模式。host 使用本机编译，guest 与真板使用静态 RISC-V 编译。平台差异只进入测量适配层。

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

控制握手必须交换 protocol version、配置 hash、capability bitmap 和角色。任何不一致都在传输前失败。

原始记录建议使用一行一条的 NDJSON。若 StarryOS 用户态 JSON 输出成本过高，可先输出固定 `key=value`，再由 host 无损转换。

每条记录至少包含：

```text
schema_version run_id test_id profile round side platform
device driver_mode protocol direction completion_point
payload_size flow_count offered_load warmup_s duration_s seed
config_hash status invalid_reason
```

原始日志始终保留。CSV 和 JSON 摘要不能替代原始记录。

TCP stream 使用确定性 payload 和滚动校验。测试循环必须处理 partial send、partial recv、`EAGAIN`、超时和 EOF。TCP RTT 使用带长度、序号和校验的 application record。

UDP datagram 需要 test ID、序号、payload 长度和校验。接收端报告 missing、duplicate、reordered、corrupt 和 late datagram。发送端报告 requested、accepted 和 syscall error。

测试程序必须覆盖以下失败路径：

- partial send 与 partial recv。
- `EAGAIN`、超时和取消。
- peer 提前 EOF 或进程退出。
- 配置、版本或 capability 不匹配。
- 时钟回退或计数器不可用。
- UDP 丢失、重复、乱序、损坏和迟到。
- 摘要 ACK 丢失或账本不闭合。

失败轮次保留原始记录。补跑使用新的 round ID。

## 测试项目

测试分为必测基线、机制诊断和扩展压力。MS16 执行必测项，并为后续 profile 固定 Schema。

| ID | 项目 | 参数 | 主要结果 | 等级 |
|---|---|---|---|---|
| N00 | manifest | 每次运行一次 | 环境与配置 hash | 必测 |
| N01 | 计时校准 | clock、空循环、instret 读取 | 测量开销 | 必测 |
| N02 | loopback 对照 | TCP/UDP，固定矩阵 | 上层软件上限 | 必测 |
| N03 | 路径校准 | ARP、ICMP、MTU、基础校验 | 路径完整性 | 必测 |
| N10 | TCP 单向 goodput | TX/RX，1 flow | 接收 goodput、CPU | 必测 |
| N11 | TCP write size | 1 B 至 64 KiB | syscall/byte、goodput | 必测 |
| N12 | TCP 双向 | 1 flow/方向 | 双向 goodput、公平性 | 必测 |
| N13 | TCP 多流 | 1、2、4、8 flows | 聚合与单流 goodput | 必测 |
| N14 | TCP 稳定态 | warm-up 与时长阶梯 | equilibrium、漂移 | 必测 |
| N20 | TCP RTT | 1、64、512、1400 B | p50/p95/p99/max | 必测 |
| N21 | UDP 单向 goodput | TX/RX，受控速率 | goodput、loss、pps | 必测 |
| N22 | UDP RTT 与间隔误差 | 1、64、512、1400 B | RTT、抖动代理 | 必测 |
| N23 | UDP burst | 32、64、128、255、256、257 包 | loss、reorder、恢复 | 必测 |
| N24 | 负载下延迟 | idle 至 90% 基准负载 | RTT tail、delay variation | 必测 |
| N30 | 非阻塞背压 | send 到 `EAGAIN` | accepted、等待、恢复 | 必测 |
| N31 | 队列边界 | 63/64/65、127/128/129 包 | ring 与 buffer 边界 | 机制 |
| N32 | 连接周转 | 串行与并发 connect/close | connects/s、失败率 | 扩展 |
| N33 | 多流公平 | 2、4、8 TCP flows | 单流分布、min/max | 扩展 |
| N34 | 缓冲边界 | socket、UDP metadata、ARP 边界 | EAGAIN、loss、恢复 | 机制 |
| N35 | 复制效率 | TCP/UDP TX/RX | copy/byte、allocation/packet | 机制 |
| N40 | 空闲成本 | 无 socket、idle socket | QEMU CPU、IRQ | 必测 |
| N41 | CPU 效率 | TCP/UDP TX/RX/bidir | CPU-s/GiB、inst/byte | 必测 |
| N42 | IRQ 效率 | MS03 snapshot delta | IRQ/packet、IRQ/GiB | 机制 |
| N43 | 调度干扰 | timer idle vs load | wake overshoot 分位数 | 必测 |
| N44 | 内存稳定性 | churn 与短 soak | RSS、allocator 前后差 | 扩展 |
| N45 | 唤醒效率 | poll 或 IRQ 到任务推进 | wakes、wake-to-run、batch | 机制 |
| N46 | descriptor 效率 | submit、complete、recycle | occupancy、residence、stall | 机制 |
| N50 | 网络损伤 | delay/loss/reorder/rate | 降级与恢复 | 扩展 |
| N51 | 稳定运行 | 固定负载 5 分钟 | stall、loss、资源增长 | 扩展 |
| N52 | 过载恢复 | 超载后降低 offered load | 恢复时间、残留状态 | 扩展 |
| N53 | SMP 与多队列 | 1/2/4 hart、queue 数 | scaling、公平性 | 后续 |
| N54 | 真板机制 | DMA、cache、PLIC、PHY | cycles、cache 与链路成本 | 真板 |

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

负载下延迟使用该环境零丢包吞吐的 0%、25%、50%、75% 和 90%。基准负载必须先由同一配置的 pilot 得到。

边界矩阵至少包含：

```text
VirtIO queue:       63, 64, 65 packets
buffer pool:       127, 128, 129 packets
UDP metadata:      255, 256, 257 datagrams
pending ARP:        31, 32, 33 packets
socket buffer:   65535, 65536, 65537 bytes
```

标准 frame size 与应用 payload 必须分字段记录。主曲线使用 MTU 内 payload；IP 分片另设功能场景。

网络损伤只在 TAP 上运行。Linux `netem` 支持 delay、jitter、loss、duplicate、reorder、rate 和固定 seed。整形方向必须记录；TCP 场景还要避免把错误 qdisc 位置当作接收端入站损伤。

测试目录支持四个 profile：

| Profile | 用途 | MS16 是否执行 |
|---|---|---:|
| smoke | 功能、握手、短校准 | 是 |
| quick | 开发回归 | 是 |
| standard | 正式 B0/A1 比较 | 是 |
| soak/board | 长稳、损伤、SMP、真板机制 | 否，只冻结 Schema |

## 指标与完成点

同一名称必须只有一个计算口径。

| 指标 | 定义 | 主要来源 |
|---|---|---|
| TCP goodput | receiver 校验字节 × 8 / 测量时间 | receiver |
| UDP goodput | 唯一且校验通过字节 × 8 / 时间 | receiver |
| offered rate | sender 请求字节或包 / 计划时间 | sender |
| enqueue rate | sender 已接受字节 / sender 时间 | sender，C1 |
| packet rate | 校验通过的 record 或 datagram / 时间 | receiver |
| RTT | origin 发出到匹配 reply 返回 | origin monotonic clock |
| RTT tail | p50、p90、p95、p99、p99.9、max | 原始 RTT 样本 |
| RTT delay variation | `RTT_i - min(RTT)` | 原始 RTT 样本 |
| 间隔误差 | 接收间隔与发送计划间隔之差 | receiver |
| loss | 缺失唯一序号 / 已发送序号 | 双端摘要 |
| duplicate | 重复序号计数 | receiver |
| reorder | 小于最高已见序号的首次到达 | receiver |
| corruption | 长度或校验失败 | receiver |
| QEMU core equivalents | QEMU CPU seconds / wall seconds | host `/proc/<pid>/stat` |
| CPU 效率 | QEMU CPU seconds / receiver GiB | host + receiver |
| guest 指令效率 | instret delta / receiver bit、byte、packet | guest `/proc/instret` |
| copy amplification | copied bytes / receiver bytes | 可选内部遥测 |
| allocation efficiency | allocation count / packet | 可选内部遥测 |
| IRQ 效率 | MS03 counter delta / packet 或 GiB | IRQ snapshot + receiver |
| poll 效率 | progress/no-progress poll、packet/poll | 可选轮询遥测 |
| wake 效率 | wake/packet、packet/wake、wake-to-run | 可选异步遥测 |
| queue 效率 | occupancy、full、submit、complete、recycle | 可选设备遥测 |
| fairness | Jain fairness index | 每流 receiver goodput |
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

`jitter` 需要明确口径。本文报告 RTT delay variation 和 UDP 接收间隔误差，不输出含义不明的单一平均值。

用户关心的“搬运一个比特需要多少指令”使用：

```text
instructions_per_bit
  = (instret_end - instret_begin - read_overhead)
    / (receiver_verified_bytes * 8)
```

同轮还应输出 instructions/byte、instructions/packet 和 instructions/syscall。分母使用 C6 字节，避免丢包或短写使结果过于乐观。

该指标是测量窗口内的 guest 整机代理。它不是驱动独占指令，也不能解释为真板周期。

## CPU 与可观测性

当前最可靠的 CPU 读数来自 host。手工启动的 host peer 可读取指定 QEMU PID 的 `utime + stime`，并按 `CLK_TCK` 换算。peer 和采集器要单独采样。

QEMU CPU 指标包含 guest 执行、设备模拟和网络 backend。它不是网卡驱动独占成本。报告 `CPU seconds` 和 `core equivalents`，不除以 host 总核数。

空闲成本至少需要三组控制：

```text
QEMU boot without network device
QEMU with idle network device and no socket workload
QEMU with idle socket/service
```

只有控制组差值能支持网络轮询成本归因。MS02 的约 100% 至 111% 单核样本不能单独归因于 10 ms fallback。

guest 的 `/proc/instret` 可读取 RISC-V retired instruction。当前单 hart 下，它近似覆盖整个 guest 测量窗口。它不是进程指令数，也不是周期数。每次运行要测两次连续读取的开销，并保留原始 begin/end。

`CLOCK_PROCESS_CPUTIME_ID` 和 `CLOCK_THREAD_CPUTIME_ID` 当前共用线程 `TimeManager`。[TimeManager](../../kernel/src/task/timer.rs) 标注了 preemption 不更新 timer state。该数据暂时只能观察。

[`/proc/[pid]/stat`](../../kernel/src/task/stat.rs) 当前只填充少量进程字段。`utime`、`stime` 等字段仍为默认值。因此，StarryOS 进程 CPU 与全系统 utilization 不能作为 MS16 Gate。

SMP 指令统计也未就绪。当前 `/proc/instret` 只读取执行该调用的 hart。后续 SMP profile 需要逐 hart 快照并求和。

现有 `/proc/interrupts` 只输出 timer callback 计数。它不能提供 NIC IRQ 计数。MS03 完成后的只读 snapshot 才是网卡 IRQ 来源。

当前 Ethernet 路径没有以下计数：

- ingress/egress packet 与 byte。
- device receive/transmit error。
- queue full 与 buffer allocation failure。
- service poll 次数和单轮预算。
- socket buffer 高水位。
- descriptor occupancy 与 completion。

这意味着外部 peer、序号和 pcap 是当前正确性真值。日志 warning 不能替代单调计数。

MS16 可评估 feature-gated telemetry。最小集合建议放在 axnet owner 边界：

- service poll 与 progress 次数。
- RX/TX frame 与 byte。
- RX/TX error 和 allocation failure。
- 单轮最大 packet 数。
- pending queue full。

轮询实现的扩展计数应包括：

- poll 调用与无进展 poll。
- fallback timer wake。
- 每轮 packet batch。
- budget exhausted 与自重调度。

异步实现的同一 Schema 应包括：

- IRQ、wake、task run 和 queue drain。
- event-before-register 重检。
- spurious wake 和 lost wakeup。
- wake-to-run latency。
- queue 非空但 task sleeping 的违规计数。

lost wakeup 和“队列非空但任务休眠”属于正确性错误，不是性能退化。

真板适配可增加：

- `cycle`、`instret` 和 hart 集合。
- DMA map、unmap、cache clean 和 invalidate。
- descriptor ownership 迁移。
- PLIC claim 与 complete。
- PHY link speed、duplex、温度和频率。
- 可用时记录 energy/bit。

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

这些值是 MS16 的起始建议，不是性能阈值。Plan 可根据一次 pilot 的总耗时调整，但必须固定到 manifest。

每个 round 都输出原始结果。吞吐按 round 报告 median、min 和 max。延迟按样本报告 p50、p95、p99 和 max。错误、短写、超时与无效样本单独计数。

负载下延迟增加 p90 与 p99.9。样本不足以稳定估计某个分位数时，报告器应标记 `insufficient_samples`。

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

真板 manifest 还要固定 CPU 频率、温度、PHY、link speed、duplex、DMA ring、cache 操作和 hart 集合。

当前环境有 `cc`、`taskset`、`tc`、`tcpdump` 和 Python。`iperf3` 与 `pidstat` 当前未安装。MS16 不应把后两者设为硬依赖。

`iperf3` 可作为方法参考。它区分 sender/receiver 结果，支持 reverse、bidirectional、parallel、warm-up omission 和 JSON。StarryOS guest 不应为了基线先移植 iperf3。

## Evidence 与 Gate

运行顺序必须是静态检查、QEMU boot、网络 smoke、回归、benchmark。benchmark 不能替代前面的设备与协议见证。

QEMU 阶段属于用户能力边界。用户手工启动 QEMU、输入 guest 命令并启动 host peer。离线 parser 可以在运行结束后处理日志。

每次正式运行应保存：

| 文件 | 内容 |
|---|---|
| `README.md` | 环境、命令、Gate、限制 |
| `manifest.json` | 版本化完整配置、capability 与 hash |
| `qemu-command.txt` | 展开后的 QEMU 命令 |
| `qemu-serial.log` | 完整串口与 QEMU 日志 |
| `guest-netbench.ndjson` | guest 原始记录 |
| `host-netbench.ndjson` | host 原始记录 |
| `host-cpu.ndjson` | QEMU、peer、collector CPU/RSS |
| `irq-snapshots.log` | MS03 前后快照 |
| `capture.pcap` | TAP 数据包见证 |
| `results.csv` | 逐 round 规范化数据 |
| `summary.json` | 汇总和有效性状态 |
| `evidence-check.json` | 完整性与比较资格检查 |

QEMU `filter-dump` 能生成 libpcap 文件。MS02 已发现 user-net UDP hostfwd 未进入该 pcap。TAP 基线应同时使用 TAP 侧抓包，避免再次把缺失 capture 当作无流量。

性能 Gate 前先过正确性 Gate：

- 双端配置 hash 一致。
- receiver 完成全部校验。
- TCP 没有未解释 EOF、timeout 或 short transfer。
- UDP loss、duplicate、reorder 与场景预期一致。
- MS03 没有 unknown cause、残留或 IRQ storm。
- MS01 14/14 与 MS02 TCP/UDP 回归仍通过。

建议 Gate 分层：

| Gate | 通过条件 |
|---|---|
| G0 静态 | fmt、unit、feature、target build、OpenSpec、diff check |
| G1 工具自测 | 协议、parser、checksum、错误场景和 golden data 通过 |
| G2 校准 | 时钟、instret、loopback 和路径校准有效 |
| G3 smoke | user-net TCP/UDP 与 MS01 至 MS03 回归通过 |
| G4 TAP 正确性 | 双端账本闭合，完整性符合场景 |
| G5 性能轮次 | standard profile 轮次与原始样本完整 |
| G6 Evidence | 必需文件和字段通过机器检查 |
| G7 比较资格 | A/B comparison key 除 treatment 外一致 |

G0 至 G4 失败时，不执行 headline 性能测试。G5 或 G6 失败时，不生成正式 B0 摘要。

MS03 Evidence 已出现 README 列出 `qemu-serial.log`，实际目录未保存该文件的偏差。MS16 必须由 `evidence-check` 检查文件存在性，不能只检查 README 声明。

首轮基线不应预设绝对性能阈值。先用 5 个 round 得到环境波动。MS16 Review 再决定回归阈值，且要写明统计口径。

QEMU 结果只支持当前 host、QEMU 版本、单 hart 和 VirtIO 设备模型。它不能作为 VisionFive2 DMA、cache、PHY 或真板吞吐证据。

原始 Evidence 只追加，不覆盖。无效轮次、异常值和补跑记录必须同时保留。

## 后续 A/B 协议

轮询基线记为 B0。异步候选记为 A1、A2。每个候选必须复用同一 benchmark 版本。

推荐使用成对运行：

```text
B0 -> A1 -> A1 -> B0
```

每组交替顺序，减少 host 漂移。至少保留 5 对有效结果。每对必须使用同一 TAP、QEMU 参数、affinity、payload 和测量时长。

报告器根据受控字段生成 `comparison_key`。以下字段变化会拒绝自动生成改善比例：

- benchmark、kernel 或 rootfs hash。
- backend、MTU、offload、vhost 或 TAP 配置。
- QEMU 版本、机器、SMP、内存、icount 或 affinity。
- payload、flow、duration、seed 或完成点。
- queue、socket buffer、telemetry 或日志级别。

只有 `treatment` 可以不同。例如 `polling` 与 `async`，或某一项 queue budget。

比较顺序如下：

1. 正确性与丢包。
2. 空闲 QEMU CPU。
3. TCP/UDP receiver goodput。
4. RTT p99 和 max。
5. CPU-s/GiB 与 instret/byte。
6. IRQ/packet 与 IRQ/GiB。
7. 背压恢复和 timer 干扰。

任何一项只改变一个变量。不能同时更换 backend、SMP、queue size 和调度模型。

QEMU B0/A1 与真板 B0/A1 是两个比较域。跨域报告相对变化和机制趋势，不报告合并的胜负结论。

MS04 的主要预期不是单纯提高峰值带宽。它还应降低无流量时的轮询成本，并保持 tail latency、丢包和背压语义。最终 Gate 要由 B0 波动决定，不能在没有数据时写死百分比。

若 telemetry 结构在异步实现中变化，公共指标仍以 receiver、host CPU 和 pcap 为准。机制计数只在定义相同或明确换算时比较。

## MS16 的执行边界

MS16 位于 MS03 与 MS04 之间。它的输入是 MS03 已通过的诊断 IRQ 和由 MS02 保持的轮询数据面。

建议目标：

- 固定 workload、完成语义、capability 和 Evidence Schema。
- 建立 guest/host 共用 benchmark 协议。
- 建立 TAP 性能运行流程。
- 记录 N00 至 N43 中标为必测的 B0 基线。
- 量化当前环境波动。
- 固定 MS04 以后使用的 A/B 规则。

建议非目标：

- 异步 RX/TX queue task。
- 删除 10 ms fallback。
- 改变 queue size 或 socket buffer。
- 零拷贝、多队列或 offload。
- 用 QEMU 数据推断真板性能。
- 为取得好看数据修改网络行为。
- 在 MS16 执行 netem、长期 soak、SMP、多队列或真板 N54。

MS16 若需要低开销 axnet telemetry，应把它当成测量设施。实现必须 feature-gated，并先量化自身开销。该改动不能改变 packet ownership、poll 频率或 socket 语义。

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
| guest CPU utilization | 是否另立 change 补 per-hart idle/runtime accounting |
| MS16 change 拆分 | 工具与 Schema、QEMU 校准、正式 B0 是否分三轮验收 |

建议 MS16 在 Plan 中拆成三个可验证批次：

1. 协议、平台适配、parser 与 Evidence checker。
2. guest portability、loopback、user-net 与 TAP 校准。
3. polling B0 standard profile 与持久化 Evidence。

若内部遥测会改动产品代码，应作为独立任务或 change。外部 headline 指标不能依赖它。

## BDD 场景草图

| 场景 | 前置状态 | 动作 | 结果与失败边界 |
|---|---|---|---|
| 正常吞吐 | 双端配置一致 | warm-up 后传输 | C6 账本闭合并生成有效 round |
| 配置不一致 | hash 或版本不同 | 开始握手 | 传输前失败，不产生性能结果 |
| partial send | socket 只接受部分数据 | 继续发送 | 无丢字节，调用数进入统计 |
| backpressure | nonblocking buffer 满 | 等待可写并恢复 | 记录 EAGAIN、等待与恢复时间 |
| peer 退出 | 测量中 EOF | sender/receiver 收尾 | round invalid，保留已完成账本 |
| UDP 异常 | 丢失、重复、乱序或损坏 | receiver 校验 | 分类别计数，不折叠为单一 loss |
| 计数器缺失 | capability 不支持 | 执行外部 workload | 可选指标 unavailable；必需 profile 阻塞 |
| 时钟异常 | monotonic 回退或分辨率无效 | 计算时间 | round invalid，不输出速率 |
| Evidence 缺失 | 必需文件未保存 | 执行 checker | G6 失败，不声明正式基线 |
| A/B 不可比 | comparison key 不同 | 生成对比 | 拒绝改善比例，列出差异字段 |

Plan 必须把这些场景映射到测试见证。错误处理不能留给 Act 临时决定。

## 关键文件与资料

| 来源 | 用途 |
|---|---|
| [axnet service](../../crates/axnet/src/service.rs) | poll、timer fallback、进度循环 |
| [Ethernet device](../../crates/axnet/src/device/ethernet.rs) | RX/TX owner 与 polling capability |
| [Router](../../crates/axnet/src/router.rs) | 64 packet burst 与 PacketBuffer 边界 |
| [TCP socket](../../crates/axnet/src/tcp.rs) | completion、buffer、TCP options |
| [UDP socket](../../crates/axnet/src/udp.rs) | datagram buffer 与 metadata |
| [network constants](../../crates/axnet/src/consts.rs) | 64 KiB、256、512 等边界 |
| [time syscall](../../kernel/src/syscall/time.rs) | monotonic 与 CPU clock 边界 |
| [task timer](../../kernel/src/task/timer.rs) | utime、stime 与 preemption 限制 |
| [task stat](../../kernel/src/task/stat.rs) | `/proc/[pid]/stat` 当前填充范围 |
| [procfs](../../kernel/src/pseudofs/proc.rs) | instret 与 interrupts 现状 |
| [QEMU make rules](../../make/qemu.mk) | backend、filter-dump、icount |
| [MS16 roadmap](../docs/tasks.md) | 已批准里程碑范围、依赖与拆分信号 |
| [quality gate baseline](../../openspec/specs/quality-gate-baseline/spec.md) | CPU、复现、完整性与完成语义要求 |
| [MS03 tasks](../../openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/tasks.md) | IRQ snapshot 与手工 Evidence |
| [MS03 Evidence](../../openspec/changes/archive/2026-08-03-ms03-virtio-mmio-diagnostic-irq-baseline/evidence/000-initial/README.md) | IRQ 基线与 Evidence 完整性偏差 |
| [MS02 idle CPU](../../openspec/changes/archive/2026-07-29-ms02-virtio-mmio-polling-baseline/evidence/003-policy-coverage-and-runtime-evidence/idle-cpu.txt) | 现有轮询成本样本 |
| [QEMU 网络 Runbook](../runbooks/qemu-network-testing.md) | 手工 QEMU 硬性政策与 payload 下载 |
| [MS02 Runbook](../runbooks/ms02-virtio-mmio-evidence.md) | MMIO、user-net、TAP、CPU 和回归阶梯 |
| [MS03 Runbook](../runbooks/ms03-virtio-mmio-irq-evidence.md) | IRQ snapshot 与回归操作 |
| [回归 Gate](../runbooks/regression-gate.md) | QEMU 相对比较与真板证据边界 |
| [QEMU networking](https://www.qemu.org/docs/master/system/devices/net.html) | TAP 与 user-net 语义 |
| [QEMU command reference](https://www.qemu.org/docs/master/system/invocation.html) | filter-dump 与 icount 边界 |
| [RFC 2544](https://www.rfc-editor.org/info/rfc2544/) | throughput、latency、loss、burst 与恢复概念 |
| [RFC 5481](https://www.rfc-editor.org/info/rfc5481/) | IPDV 与 PDV 术语边界 |
| [RFC 6349](https://www.rfc-editor.org/info/rfc6349/) | TCP RTT、BDP、buffer 与多流方法 |
| [iperf3 documentation](https://software.es.net/iperf/invoking.html) | 双端、反向、多流和 JSON 方法 |
| [tc-netem manual](https://man7.org/linux/man-pages/man8/tc-netem.8.html) | delay、loss、reorder、rate 与 seed |
