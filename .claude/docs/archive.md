# archive.md — 文档归档（汇总）

> 自动归档记录，由 project-archivist 维护。
> 按源文档分节，每条含日期、编号、置信度、理由、恢复条件。
> 恢复方式: 用户说"恢复 §{文档名} #{编号}"。
> 搜索方式: grep "关键词" archive.md 或 grep "编号" archive.md

---

## architecture.md 归档

<!-- archive: A2 -->
**日期**: 2026-05-27
**条目**: A2: 串口与控制台关系——独立硬件 /dev/ttyS0
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已被 ADR-015 取代（渐进式开发策略：共用 UART0，M1/M2 用 Console 验证，M3 替换 AsyncUart）
**恢复条件**: 需要回顾早期"独立硬件"设计思路时

原始内容:

<!-- A2 --> ### 2026-05-24 - 串口与控制台关系——独立硬件 /dev/ttyS0

- **决策**: 新增独立硬件串口（QEMU `-serial` 多路配置），注册为 `/dev/ttyS0`，不影响现有 `/dev/console`
- **原因**: 隔离风险，独立开发测试；初期不破坏控制台稳定性
- **影响**: 需在 QEMU 启动参数添加第二个 `-serial`；`/dev/console` 和 `/dev/ttyS0` 是两个独立设备

---

<!-- archive: A7 -->
**日期**: 2026-05-27
**条目**: A7: Console与AsyncUart共存策略——先独立后统一
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已被 ADR-015 取代（渐进式开发策略：共用 UART0，不再有"先独立后统一"阶段）
**恢复条件**: 需要回顾早期共存策略时

原始内容:

<!-- A7 --> ### 2026-05-25 - Console与AsyncUart共存策略——先独立后统一

- **决策**: 采用方案 C"先独立后统一"——AsyncUart 作为独立 `/dev/ttyS0` 设备，Console 保持不变；远期统一
- **原因**: 初期隔离风险，不破坏控制台稳定性；AsyncUart 可独立开发测试

---

<!-- archive: A11 -->
**日期**: 2026-05-27
**条目**: A11: QEMU 双串口开发策略——独立硬件隔离风险
**原分类**: 架构决策
**置信度**: HIGH
**理由**: 已不适用（QEMU 第二串口需要补丁未合并，决策共用 UART0）
**恢复条件**: 需要回顾早期双串口策略时

原始内容:

<!-- A11 --> ### 2026-05-25 - QEMU 双串口开发策略——独立硬件隔离风险

- **决策**: M0-M2 阶段 QEMU 配置第二个 `-serial`，AsyncUart 操作第二 UART 硬件实例
- **原因**: Console 串口用于内核日志和 shell 交互，如果直接在上面测试中断驱动可能破坏调试信息输出

---

## learned.md 归档

<!-- archive: L24 -->
**日期**: 2026-05-27
**条目**: Q1: QEMU virt 平台是否支持第二个 16550 UART？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已解决（QEMU 第二串口需要补丁未合并，决策共用 UART0，参见 ADR-013、ADR-014、ADR-015）
**恢复条件**: 需要回顾第二串口调研结果时

原始内容:

- Q1: QEMU virt 平台是否支持第二个 16550 UART？ — 决定是否需要独立硬件还是复用同一 UART — QEMU 文档/实验
  → **已解决：标准 QEMU 不支持，需补丁。决策共用 UART0。**

---

<!-- archive: L25 -->
**日期**: 2026-05-25
**条目**: Q2: 修改 axplat/axhal crate 的方式？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-009：不修改，直接 path 依赖本地 uart_16550 crate）
**恢复条件**: 需要回顾此决策的上下文时

原始内容:

- Q2: 修改 axplat/axhal crate 的方式？~~fork 还是提 PR？~~ **已决策：不修改，内核直接用本地最新 uart_16550 crate** — ADR-009

---

<!-- archive: L28 -->
**日期**: 2026-05-25
**条目**: Q5: trap 上下文中读 MMIO 是否安全？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已解决（uart_16550 crate 封装了 volatile read，ISR 中安全）
**恢复条件**: 涉及 ISR 中 MMIO 读写的安全检查时

原始内容:

- Q5: trap 上下文中读 MMIO 是否安全？~~是否有内存序问题？~~ **已解决：uart_16550 crate 封装了 volatile read，ISR 中安全**

---

<!-- archive: L42 -->
**日期**: 2026-05-25
**条目**: Q19: 本地 uart_16550 crate 是否可发布到 crates.io？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-009：使用 path 依赖，暂不发布）
**恢复条件**: 需要发布到 crates.io 时

原始内容:

- Q19: 本地 uart_16550 crate 是否可发布到 crates.io？ — **已决策：使用本地 path 依赖，暂不发布** — ADR-009

---

<!-- archive: L43 -->
**日期**: 2026-05-25
**条目**: Q20: StarryOS 的 uart_16550 v0.4.0 和本地版本是否有 API 兼容性？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已确认（本地 v0.6.0 完整覆盖 v0.4.0 API，额外增加中断控制）
**恢复条件**: 需要确认 API 差异时

原始内容:

- Q20: StarryOS 的 uart_16550 v0.4.0 和本地版本是否有 API 兼容性？ — **已确认：本地 v0.6.0 完整覆盖 v0.4.0 API，额外增加中断控制** — ADR-009

---

<!-- archive: L44 -->
**日期**: 2026-05-25
**条目**: Q21: Console 和 AsyncUart 同时操作同一 UART 的协调方案？
**原分类**: 存疑问题
**置信度**: HIGH
**理由**: 已决策（ADR-007：先独立后统一，QEMU 配第二个 -serial）
**恢复条件**: 需要回顾共存策略时

原始内容:

- Q21: Console 和 AsyncUart 同时操作同一 UART 的协调方案？ — **已决策：先独立后统一（方案 C），QEMU 配第二个 -serial** — ADR-007

---

## docs/analysis/ 归档

<!-- archive: docs-analysis-batch-2026-05-28 -->
**日期**: 2026-05-28
**操作**: 批量归档过时分析文档
**文档数量**: 11
**置信度**: HIGH
**理由**: 已标记"⚠️ 此文档为早期分析，部分内容已过时"，M3 替换失败后分析失效
**恢复条件**: 需要回顾早期探索分析时（从 git history 恢复）

归档文档列表（已删除原文件）:

1. **async-io-framework.md** — 异步 IO 框架分析，M3 替换失败后失效
2. **async-runtime.md** — 异步运行时分析，axtask::future 选型已定
3. **async-uart-design-context.md** — AsyncUart 设计上下文，ADR-018 失败后需重新设计
4. **comparison-with-sdmmc.md** — SDMMC 对比分析，部分过时
5. **device-registration.md** — 设备注册分析，已标记过时
6. **feasibility-assessment.md** — 可行性评估，M3 失败后部分失效
7. **serial-interfaces-overview.md** — 串口接口概览，已标记过时
8. **serial-optimization-preview.md** — 性能优化预览，M4 待重启
9. **syscall-interface.md** — syscall 接口分析，已标记过时
10. **tty-console-stack.md** — Console 栈分析，ADR-018 失败后部分失效
11. **uart-hardware-driver.md** — UART 硬件驱动分析，状态异常未解决

**保留文档**（未标记过时，仍有参考价值）:

- boot-init.md — 启动流程通用知识
- interrupt-framework.md — 中断机制已实现
- project-overview.md — 项目概览
- reference-implementations.md — 参考实现
- task-process-model.md — 任务模型
- uart-16550-crate-reuse.md — uart_16550 已集成
