# Evidence: 003-mac-register-baseline / 001-rework

- 来源: plan-required（Plan Context Persisted Evidence：APMU 读回与 MAC 读值是一次性板上现场事实，身份对照、004 PHY 契约与 MS10 设计依赖原始读值，会话后不可低成本重取；Act Response 无法承载原始读值结构）
- 结论: 第 1/2 轮已采集（2026-10-01）。APMU CTRL 写前 `0x00000000`——证实 U-Boot handoff 后 GMAC 总线时钟门控（父 Cycle 000 阻塞根因）；step1 写 `0x00000009`、读回 `0x00000009`；step2 写 `0x0000000b`、读回 `0x0000000b`（bit0/bit1 均置位，写生效且读回粘滞）。MAC `version=0x00001054` ×2，与板上 Linux 驱动观测（`User ID: 0x10, Synopsys ID: 0x54`）精确一致，非全零/全一——D1 身份子项与 D3 可追溯性满足。`debug=0x00000000` ×2：字面命中父 Cycle Stop when，Act 曾写 Blocker Handoff；同日用户裁决其为非实质形式问题并接受（原话与风险见 Act Response Blocker Resolution），Round 2 与 reset 观察经用户豁免，Cycle 以 Round 1 证据收口（Act Response status `reported`）。debug 位域语义澄清移交 Iteration 004 计划。
- 文件: `board-mac-regs.txt`（Round 1 串口采集原文；TLV serial# 脱敏，其余逐字保留）
