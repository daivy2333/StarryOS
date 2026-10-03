# Evidence: 001-phy-device / 000-initial

- 来源: plan-required。proposal 用户要求设备轮板上 raw 串口留档：实体板 + 操作者物理插拔会话无法由 agent 低成本重跑，摘要会丢失 source/INSR/快照/ack 先后与 quiet 的决定性结构，Act Response 摘要不足以承载 D2/D3/D5/D6 的板上判据（同 Iteration 000 的留档理由）。
- 结论: 第三次板测（2026-10-03，stage10 窗口 30 s）D2/D3/D5 板上判据直接成立——拔线 claim idx=0 source=60 insr=0x0010 link=down aligned=1；插回 claim idx=2 insr=0x0010 link=up aligned=1；ack 后投递成立，每物理事件恰一条 INSR=0 回声 claim（分类器判 aligned=0 不计入证据，无风暴）；稳定 1 s quiet（quiet=1 quiet-claims=0）；route 60/133/277 与 gmac1160/apmu-wake-bit12 只读展示，无软件触发；window-close edge 全零、setip60=0；result=link-down-and-up-observed claims=4 down=1 up=1 unaligned=2 mdio-err=0。setup 判据同板证实：iner written=rb=0x0030（使能生效）、insr-drain=0x0020、pad35 0xa441→0xa420 ok=1。前两次板测（同日，120 s/30 s 窗口，setup 未武装边沿、CLEAR hold 未释放）零 claim，是本轮实现修正的依据，关键行摘录见 Cycle Act Response。reset 已由操作者确认回原系统（2026-10-03 会话确认）。
- 文件: board-phy-irq.txt（第 3 次板测完整 raw 串口，U-Boot usb start 至 stage10 result 行，逐字保真）
