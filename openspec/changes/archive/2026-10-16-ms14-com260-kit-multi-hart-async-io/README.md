# ms14-com260-kit-multi-hart-async-io

K3 真板 NIC 与 UART 多 hart 异步 I/O。000–003 已 accepted；004 已执行（2026-10-16）：T12/T13/T15 完成并板验——16-hart BusyBox RAM rootfs shell 正常使用（wget/TCP/UDP），TCP 双向吞吐与原厂 Linux 同法对比（板→PC 76.5%、PC→板 25.6%，单样本裁决），T14 UART 测量按用户裁决取消。执行中两轮产品修复（exec 段 eager populate、用户态网络 syscall 门收窄至 k3-smp-combined）均先行板证后 GREEN。Act Response `reported`，等待 Plan Review。范围、豁免原话与结果见 proposal.md、tasks.md、004 Cycle 文件。
