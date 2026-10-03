# Evidence: 000-initial / 001-replan

- 来源: plan-required（用户要求保留板上 IRQ 原始串口留档；摘要会丢失 claim/status/ack
  与各相计数的决定性结构，实体板 + 操作者串口会话无法由 agent 低成本重跑）
- 结论: 支持本轮 Acceptance A1–A6——
  - A1（R1/R5）: `[k3:irq] init=global/percpu` 分层标记 + MS09 stage1–8 完整保持 +
    stage9 readback（domaincfg=0x80000104、delivery=1、threshold=0）；
  - A2（R1/R4）: SETIPNUM_LE 与 GENMSI 各 3 次投递，每次 raw=0x10001（EID=1、prio=1，
    claim 与 handler 增量均 1）；
  - A3（R4）: aplic-mask blocked=1 fresh=1（APLIC source mask 阻断 SETIPNUM_LE）；
    eie-mask blocked=1 fresh=1（本地 EIE mask 阻断 GENMSI）；
  - A4（R1）: retrap independent-claims=2（handler 内新 GENMSI 存活为独立 claim，
    原子 claim+EOI 先于 handler）；
  - A5（R1/R4）: unknown-eid delta=1 no-handler=1（EID2 被 claim 且诊断、无 panic）；
    storm events=32 sext-masked quiet=1（32 次非零 claim 预算止损，SEXT 关闭后 quiet）；
  - A6（R5）: 本文件即 raw 串口记录；reset 恢复见下方操作者一句话。
  - Result: `stage=9 pass`。板上观察 `eie_retained=0`（本板对 EIE 掩码下 MSI 为丢弃而非
    保留，与 fake 的规范保守模型不同）——不影响 A3 判据（mask 阻断 + 新触发到达均成立）。
    `register source 9223372036854775813 rejected` = runtime timer（向量 5）注册被固定
    路由正确拒绝，为本轮契约预期（timer 保持 stub）。
  - msicfg 读回判据的板级事实（runs 2–5 裁决）：本硅片 msiaddrcfg 为「写后读不回」形态
    （读恒 0），投递实验证明写入实际生效；domaincfg DM 为只读 1（MSI-only 域）、bit31
    恒 1（WARL）。init 的 msicfg/domaincfg 读回已降级为信息行，投递为准绳。
- 文件: `board-aia-irq.txt`（raw 串口，U-Boot 装载 → stage=9 pass）
- 操作者 reset 一句话: 用户原话「那个我顺手就做了，你不用管这一步」——reset 已执行、恢复原系统（2026-10-03，紧接本验收运行）。
