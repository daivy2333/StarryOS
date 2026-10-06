# Evidence: 000-initial / 000-initial

- 来源: user-required。用户在 Act Response 标记 `reported` 后明确要求"把这个输出保存到证据文件"（公共规则 › Iteration 与 Cycle 线程白名单第一条：用户明确要求）。Act Response 不足的原因：A2/A5 的直接目标行为证据是完整串口原始帧十六进制（ARP 帧逐字节内容、raw descriptor 状态字）与有界失败输出的原始现场，超过公共规则 › 验证的单条 20 行摘录上限，且物理板测无法低成本重跑。
- 结论:
  - V3（board-stage11-v3-rx-arp.txt）支持 A1/A2/A4：真实 ARP 请求帧被接收并逐字节复制——`arp-req slot=1 len=60 raw=0x30010040`（FD|LD、ES=0、OWN=0、原始长度 64），`data=` 与刺激完全一致（广播 dst、源 MAC、ethertype 0806、opcode 0001、SPA c0a88901=192.168.137.1、TPA c0a88902=192.168.137.2、尾部零填充）；`frames=8 drops=0 revocations=0`，30 s 窗口精确有界；`masks=60/133/277` 先于一切前台访问；`init LinkConfig { speed: S1000, full_duplex: true }` 实证裁决 STAT1000 bit11 位图与本板 RTL8211F 一致。附带观察到 PC 后台广播帧（280/120 字节）同样被正确接收分类。
  - V4（board-stage11-v4-linkdown.txt）支持 A3/A5 的有界失败分支：拔线启动后 stage10 窗口正常结束（`no-device-event`），stage11 在 ≤5 s 内以 `err=init LinkTimeout { bmsr2: 31113 }`（0x7989，link/AN 位均 0）有界终止，未打印 `rx-running`、未打开 RX 窗口、无帧行。
  - 待操作者一句话确认（不因缺省计入）：两轮 reset 后原系统恢复结论；V3 轮 Wireshark 对端同帧一致性确认。
- 文件: board-stage11-v3-rx-arp.txt、board-stage11-v4-linkdown.txt（均为操作者粘贴的串口原文节选，自 stage9 起；stage1–8 不在粘贴范围内。环境信息仅为适用范围说明，不构成 Acceptance 证据）。
