# Evidence: 001-raw-tx / 000-initial

- 来源: act-added；白名单理由——Blocker 需要保留一次性真板现场：stage12 串口输出中的 16 个 TerminalTimeout
  raw 值、reply 到达时序与 reply 帧内容是 Plan 裁决 TX doorbell/IOC 语义的决定性结构，无法低成本复现
  （需真板 + 操作者 + 对端拓扑），Act Response 的 ≤20 行摘录不足以承载完整时序。
- 结论: 命中 2.2 Task Contract「Stop when: 板上 terminal 不推进（doorbell 语义与板不符）」。16 个 TX 提交
  全部 TerminalTimeout（raw=0xb000003c，与 CPU 写入的 OWN|FD|LD|60 逐位相同——设备未可见回写 OWN），但对端
  确实收到请求并返回真实 ARP is-at 应答（≈15 帧 RX）。有界止损按设计工作（owner 保留、ring 满有界 not_sent=4、
  backing 保留、无 FBE/ES）。同一输出同时证明：stage12 gate 拒绝非 WindowEnded 的 stage11 时零 GMAC 访问
  （同日首跑 link-down 分支，V4 类，未留档、可复现）。
- 文件: board-stage12-tx-timeout.txt（操作者串口原文，自 stage8 起）、blocker.md（act-added）
- 限制: Wireshark 对端抓包计数未采集（操作者未提供）；操作者 reset 恢复结论未回报，可随下一轮板测补一句。
