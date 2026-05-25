# archive.md — 文档归档

> 自动归档记录，由 project-archivist 维护。
> 按源文档分节，每条含日期、编号、置信度、理由、恢复条件。
> 恢复方式: 用户说"恢复 §{文档名} #{编号}"。
> 搜索方式: grep "关键词" archive.md 或 grep "编号" archive.md

---

## learned.md 归档

<!-- learned.md entries below -->

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

## optimization.md 归档

<!-- optimization.md entries below -->

---

## tasks.md 归档

<!-- tasks.md entries below -->

---

## architecture.md 归档

<!-- architecture.md entries below -->

---

## SNAPSHOT.md 归档

<!-- SNAPSHOT.md entries below -->

---

## references.md 归档

<!-- references.md entries below -->
