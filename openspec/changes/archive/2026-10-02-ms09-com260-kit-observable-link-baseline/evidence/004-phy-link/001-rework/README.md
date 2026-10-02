# Evidence: 004-phy-link / 001-rework

- 来源: user-required（用户在 V3 板上运行完成后明确要求保存本轮串口日志。Plan Context Persisted Evidence 为 `none`，其「R68 路径可低成本复现」理由被用户明确指令覆盖，属公共规则白名单第 1 项）
- 结论: 支持本 Cycle A1/A2/A4 与 V3 通过条件（2026-10-02，操作者执行，R68 路径，日志逐字保存）。
  - stage1–5 基线行逐字复现 003 已接受现场（apmu 0→9→b、version=0x1054×2 class=ok、debug=0×2 sem=idle）。
  - stage6 `gate=proceed` 后三写序列生效：`iopd write=0x00000004 ok`，且 mux 行紧随——总线未挂，R69 用法对齐的 IO-PD 整字写在真板成立（父 Cycle 000 读前置阻塞的修复验证）。16 个 pad 行：pin 21–35 mux 位置 1 且其余位保持（`0xa440/0xc440 → 0xa441/0xc441`）、pin 37 keep 无写；GPIO1_5 恰一次复位完成。
  - stage7 真实 PHYID `0x001cc916 result=rtl8211f` 精确匹配；`bmcr=0x1040 bmsr=0x7989 link=down an-capable=1 an-complete=0`——无对端初始相 link=0 直接可观察（状态标签 negotiating 为 AN 未完成的契约预期形态），与错误状态（无 mdio timeout、无 all-zero/all-one）明确区分。
  - stage8 一次 bits-preserving AN restart（`0x1040 → 0x1240`）+ 只读广告诊断（reg4=0x01e1 reg9=0x0200）后，操作者多次插拔对端产生 13 个状态变更样本：6 次 negotiating→up 转换（≥5 个完整 up→down→up 中段循环，契约要求两次），每次 up 带 `latch-cleared`（BMSR latch-low 语义按设计工作，首读 `0x79a9` link=0、次读 `0x79ad` link=1）；窗口未被插拔延长，以 `observation-ended last=negotiating up-observed=yes` 结束。普通 `reset` 回原系统由操作者确认完成。
  - Stop when 全部未触发。Repair item 5.1-R1 与任务 5.1–5.2 的板上验收条件全部直接观察满足。
- 文件: `board-phy-link.txt`（串口采集原文，逐字保留；无敏感字段，未脱敏）
