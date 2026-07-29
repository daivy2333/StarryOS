## Why

MS02 需要可复现的 QEMU 网络基线。
现有 MS01 证据以 guest loopback 为主。
它未证明外部入站流量能推进。

本轮合并 T02 与 T03。
先分开串口、设备探测和网络证据，
再建立 VirtIO-MMIO 同步轮询收发基线。

## What Changes

- 固定 QEMU `virt`、单 hart、VirtIO-MMIO net/block 启动签名。
- 分开记录串口、MMIO probe、网络和 hostfwd 结果。
- 提供明确的 guest TCP/UDP 服务与宿主端用例。
- 为无 IRQ 的网卡补充有界同步轮询进度。
- 启用协议栈 ICMP echo reply，不新增 raw socket。
- 分别验证 ARP、ICMP、UDP 和 TCP 5555。
- 记录空闲 CPU 基线，不设性能通过阈值。
- 保持 PCI、IRQ、异步队列和性能优化不变。

## BDD Scenario Sketch

### Happy Path

- QEMU 无 hostfwd 启动时，串口仍进入 shell。
- MMIO net/block 被探测，`eth0` 完成初始化。
- guest 服务启动后，宿主可分别完成 UDP 和 TCP 用例。
- 外部 ICMP echo request 得到 guest echo reply。
- 每种协议都有独立日志或抓包见证。

### Sad Path

- guest 服务未启动时，hostfwd 失败不得计为网卡失败。
- 网卡未探测时，串口成功不得计为网络成功。
- 无外部流量时，轮询机制不得形成无界 busy loop。
- payload 或端口不匹配时，用例必须失败并标出路径。

### Edge Case

- 流量在 socket 等待者注册前到达时，后续 I/O 仍能推进。
- UDP datagram 边界与源地址必须保持。
- TCP 连接关闭后，服务可接受下一条连接。
- hostfwd TCP 与 UDP 使用同一端口时，证据仍需分开。

### Error, Timeout, Cancel, and Compatibility

- 宿主用例必须有有界 timeout。
- timeout 后需保留串口、服务和协议路径分类。
- 本轮不新增 socket cancel 或 async future 语义。
- 保持 MS01 的 bind、listen、UDP、poll 和 errno 语义。
- 保持当前 VirtIO-MMIO feature 选择。

## Capabilities

### New Capabilities

- `qemu-mmio-polling-baseline`: 定义 MS02 的环境边界、
  同步进度和协议级验收。

### Modified Capabilities

- None.

## Impact

- QEMU 启动参数与网络测试入口。
- guest 手工启动命令和 MS02 测试 payload。
- 本地 axnet 的同步轮询推进路径。
- 本地 smoltcp 的 ICMP feature 配置。
- QEMU 日志、抓包和 CPU 基线采集方式。

## Non-goals

- VirtIO IRQ、PLIC、AtomicWaker 或异步 queue task。
- PCI、VF2、SMP 或真实硬件证明。
- raw ICMP socket、`ping(8)` syscall 兼容。
- 吞吐、延迟或 CPU 优化。
- 自动化基础设施升级与全局状态同步。

## Gate 1

- Status: approved
- Decision: 用户于 2026-07-29 回复
  “同意你的建议，开始计划吧”，批准探索报告中的
  MS02 范围和默认取舍。
- Assumption: ICMP 采用协议栈 echo reply 与独立外部注入。
- Boundary: 若本机无法提供可执行的注入环境，
  Gate 2 必须阻塞，不得把环境选择交给 Act。
