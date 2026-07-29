## Context

MS01 已证明本地 socket 同步行为。
但其主要用例走 `127.0.0.1`。
外部入站流量仍缺少进度来源。

[TCP accept](../../../crates/axnet/src/tcp.rs#L340)
先调用 `poll_interfaces()`。
无连接时，它通过 `poll_io` 注册 waker。
[Service::register_waker](../../../crates/axnet/src/service.rs#L84)
只使用 smoltcp timer 与设备 waker。

[EthernetDevice::register_waker](../../../crates/axnet/src/device/ethernet.rs#L336)
仅在网卡有 IRQ 时注册。
当前 VirtIO-MMIO probe 的 IRQ 为 `None`。
因此外部 RX 可停留在设备队列中。

当前 QEMU 配置位于
[make/qemu.mk](../../../make/qemu.mk#L52)。
串口、user net 和 hostfwd 是独立通道。
操作必须遵守
[QEMU Network Testing Runbook](../../../.claude/runbooks/qemu-network-testing.md)。

当前基线 revision 为
`efcf08124294d523ccab4d3569ea97fe31ed96c1`。
依赖树证明 `bus-mmio` 与 `bus-pci` 同时存在。
K32 规定当前运行结果按 MMIO 解释。

smoltcp 的
[ICMP echo reply](../../../crates/smoltcp/src/iface/interface/ipv4.rs#L345)
受 `auto-icmp-echo-reply` 控制。
[axnet feature 列表](../../../crates/axnet/Cargo.toml#L100)
尚未启用该 feature。
kernel syscall 也不支持 raw ICMP socket。

## Goals / Non-Goals

Goals:

- 固定 MS02 的 QEMU MMIO 启动契约。
- 让无 IRQ socket 等待获得有界进度。
- 分开验证 ARP、ICMP、UDP 和 TCP。
- 记录 QEMU 空闲 CPU 基线。
- 保持 MS01 socket 行为。

Non-goals:

- 独立 stack runner 或后台网络任务。
- IRQ、PLIC、AtomicWaker 和 queue task。
- raw ICMP socket 与 BusyBox `ping` 兼容。
- PCI、SMP、VF2 或硬件性能证明。
- 通用 QEMU 自动化框架。

## Decisions

**D1：无 IRQ 设备使用 10 ms timer fallback**

在 `Device` trait 增加轮询需求查询。
loopback 默认不需要 timer。
Ethernet 在 `irq_num().is_none()` 时返回需要轮询。

`Service::register_waker` 取两个 deadline 的较早者：

- smoltcp `Interface::poll_at`。
- 当前时间加 10 ms。

仅 mask 命中的无 IRQ 设备启用 fallback。
timer 到期后唤醒现有 socket waiter。
waiter 重试时仍由 `poll_interfaces()` 推进协议栈。

选择该方案是为保持同步调用模型。
固定 10 ms 避免无界 busy loop。
空闲等待最多产生每秒 100 次唤醒。
本轮只记录其 CPU 结果。

未采用后台 runner。
它会提前引入 MS06 的状态所有权。
也未对所有设备强制 timer。
那会改变后续 IRQ 路径的行为。

**D2：单一 waiter 是 MS02 的并发边界**

`Service` 当前只保存一个 timeout future。
本轮不改多 waiter 所有权。
guest TCP/UDP payload 使用一次 `poll()` 等待。
两个 socket 共享同一进程 waker。

若用例需要两个独立阻塞进程，
实施必须停止并返回 Plan。
多 waiter 属于 MS06/T10。

**D3：ICMP 使用 smoltcp 自动 echo**

axnet 的 smoltcp feature 增加
`auto-icmp-echo-reply`。
不修改 syscall 或 socket 类型。

ICMP 手工用例使用 QEMU TAP backend。
宿主手工运行 `ping` 与 `tcpdump`。
guest 先启动阻塞 UDP `nc`。
该 waiter 维持同步 timer fallback。

TAP 名称固定为 `tap-ms02`。
宿主地址固定为 `10.0.2.2/24`。
guest 地址保持 `10.0.2.15/24`。

未采用 socket backend 或 packet 脚本。
它们违反 QEMU 手工测试政策。
未采用 bridge。
它会引入额外网络拓扑。
未实现 raw ICMP socket。
该工作超出 MS02。

**D4：TCP/UDP 使用静态 guest payload**

新增 `tests/ms02_guest_service.c`。
它绑定 TCP/UDP guest 端口 5555。
单个 `poll()` 循环处理两种协议。

payload 输出固定 READY、PASS、FAIL 标记。
每次请求使用固定内容。
TCP 完成后仍可接受下一条连接。
UDP 保持 datagram 边界与源地址。

payload 通过 Runbook 的 HTTP 方法下载。
不修改 [src/init.sh](../../../src/init.sh)。
服务未启动与 hostfwd 失败必须分开。

**D5：QEMU 只保留手工验证**

不新增脚本驱动 QEMU 或 guest shell。
Act 只准备 payload、命令和判定规则。
用户按 Runbook 手工执行 QEMU。

QEMU 验证是能力边界。
失败时 Act 写 `blocked`。
`Blocker Handoff` 记录最早失效层。
未取得手工证据时不得通过 Gate 5。

## Risks / Trade-offs

- 固定 10 ms 会增加等待延迟。
  MS02 不设延迟目标，并记录 CPU 基线。
- 单 timeout 不能证明多 waiter。
  payload 使用单一 `poll()`，T10 处理扩展。
- TAP 创建需要宿主权限。
  权限不足时写 blocked response。
- `10.0.2.2/24` 可能与宿主路由冲突。
  冲突时停止，不改 guest 固定地址。
- ICMP 依赖阻塞 UDP waiter 推进。
  若 waiter 未注册 timer，ICMP Gate 失败。
- QEMU 证据不代表真板或 SMP。
  Evidence 必须标为 QEMU、单 hart。

## Migration Plan

1. 先增加 payload 与纯逻辑测试。
2. 取得当前代码的 RED 见证。
3. 增加设备轮询需求与 timer fallback。
4. 启用 smoltcp ICMP echo feature。
5. 运行构建与 MS01 回归。
6. 到达 QEMU 手工能力边界后停止。
7. 用户提交协议和 CPU 证据。

回滚时删除新 payload，
移除 ICMP feature，
并撤销 timer fallback 与 trait 扩展。
MS01 代码路径必须保持可构建。

## Open Questions

None.
