# Spec Delta: architecture — Carrier ARC-202607081429

## REMOVED Requirements

### Requirement: A014/A015 — 探索方向 A（渐进式集成 Console）

探索方向 A 策略（M1/M2 用 Console 验证架构，M3 替换为 AsyncUart）已从 active architecture spec 移除。M3 失败（IRQ 风暴 + TX busy-loop），核心架构由 ADR-025/027 继承。

#### Scenario: 恢复方向 A 决策正文

- **WHEN** 开发者需要回查方向 A 的完整决策正文
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: A016/A017 — 方向 A 失败教训

方向 A 失败教训（未 dump 寄存器 + 战略转向过激 + stride=4 + Console IER 冲突）已从 active architecture spec 移除。教训"集成前 dump 全部寄存器"在 learned L79，stride 根因由 ADR-026 纠正。

#### Scenario: 恢复方向 A 失败教训正文

- **WHEN** 开发者需要回查方向 A 失败的详细分析
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: A020/A021 — 探索方向 B（完全剔除 Console）

方向 B 策略（feat/uart-async-dev2，完全剔除 Console 从零开始）已从 active architecture spec 移除。stride=4 导致 LoadFault，最初误判为 MMIO 权限问题。核心设想由 ADR-025/027 继承，stride 根因由 ADR-026 纠正。

#### Scenario: 恢复方向 B 决策正文

- **WHEN** 开发者需要回查方向 B 的完整决策正文
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: A032 — uart_16550 提取决策原始版

ADR-032 原始提取决策（5 trait，"📋 待实施"）已从 active architecture spec 移除。已被 ADR-033（"已接受"）正式化，5-trait 由 ADR-036 缩减为 2-trait。

#### Scenario: 恢复 ADR-032 原始正文

- **WHEN** 开发者需要回查 ADR-032 原始决策的完整内容
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

---

## 压缩保留（Compress-Archive 区）

<!-- A014/A015 -->
### A014/A015 (Compress-Archive, 2026-07-08)
探索方向 A（渐进式集成 Console）— M3 失败（IRQ 风暴 + TX busy-loop）。
状态：已关闭（"MUST 不再继续使用"）；核心架构（Ring Buffer + ISR + copier）由 ADR-025/027 继承。

<!-- A016/A017 -->
### A016/A017 (Compress-Archive, 2026-07-08)
方向 A 失败教训 — 未 dump 寄存器 + 战略转向过激；具体失败为 stride=4 + Console IER 冲突。
状态：已修复；教训"集成前 dump 全部寄存器"在 learned L79；stride 根因由 ADR-026 纠正。

<!-- A020/A021 -->
### A020/A021 (Compress-Archive, 2026-07-08)
探索方向 B（完全剔除 Console）— stride=4 导致 LoadFault，最初误判为 MMIO 权限问题。
状态：已关闭；核心设想由 ADR-025/027 继承；stride 根因由 ADR-026 纠正；MMIO 权限由 ADR-024 澄清。

<!-- A032 -->
### A032 (Compress-Archive, 2026-07-08)
uart_16550 提取决策原始版 — 定义 5 个 OS 抽象 trait，"📋 待实施"。
状态：已被 ADR-033（"已接受"）正式化；5-trait 由 ADR-036 缩减为 2-trait（OsRuntime + OsWakerSet）。
