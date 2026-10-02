# Evidence: 001-board-ram-boundary / 000-initial

- 来源: plan-required（Persisted Evidence 1/2）；启动日志保存范围按用户 2026-10-01 要求从 Plan 的「关键行」扩展为两次日志全文（超出 Plan 最小范围的部分按 user-required 记录）
- 结论: 支持 B1——采集程序在当前板官方 Linux 真实运行，`board-facts.txt` 含 `MS09_FACTS_BEGIN`…`MS09_FACTS_END` 完整序列与环境说明行。支持 B2——`uboot-readonly.txt` 含两次完整启动日志（上电 + 复位按钮）与复位观察记录（复位后回到原 Linux 登录提示 `k3 login:`），两次启动载荷地址逐一相同（重复启动稳定）；提供内存图输入：DRAM 0x102000000–0x2ffffffff、CMA 0x140000000/512MiB、framebuffer 0x2fe000000/32MiB、SWIOTLB 0x2f0e00000–0x2f1600000、initrd/FDT 终址、U-Boot 载荷地址（kernel 0x140000000→0x102200000、FDT@0x138000000）。U-Boot 控制台只读命令（version/bdinfo/printenv/help）采集经用户 2026-10-01 豁免，原话与风险见 Iteration 001 Cycle 000 Act Response。逐项对照与 RAM 候选区间推导、2.3 裁决表见该 Act Response。
- 文件: `board-facts.txt`（采集程序完整输出 + 环境行）；`uboot-readonly.txt`（两次串口启动日志全文，serial#/USB SerialNumber 已脱敏，共 14 处替换）
