# N00-N03 attempt 3

- Source: user-submitted QEMU and host output, 2026-08-05
- Guest artifact: SHA-256 `b863b060500c3a0977102e840d2d7160d75d7ea899567ab38cc72891ad5f1eb3`

| Check | Result |
|---|---|
| Guest payload hash | PASS、与 host 构建产物一致 |
| Workload self-test | PASS、exit 0 |
| UDP bidi 2-flow loopback | PASS、双端 358400 bytes、256 packets |
| UDP offered/accepted ledger | PASS、双端 `256/256` |
| UDP anomaly counters | PASS、loss/duplicate/reorder/corrupt/late 均为 0 |
| Payload replacement | PASS、`network_benchmark_v3` 已移动到 `/tmp/network_benchmark` |

Attempt 2 已覆盖 monotonic、instret、TCP loopback、boot interface 和 host ping。Attempt 3 使用修复后的同源产物关闭 UDP ledger 缺口，因此 N00-N03 整体通过。

`ip addr show eth0` 因 guest 不支持 AF_NETLINK 保持 SKIPPED。串口启动日志中的 `10.0.2.15/24` 与 host 3/3 ping 是替代见证；TAP pcap 仍须提供 ARP/ICMP 校准证据。
