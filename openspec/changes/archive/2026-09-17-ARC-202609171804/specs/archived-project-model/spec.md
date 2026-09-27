## Purpose

保存从 project-model 移出的 M32 原文，使其在不继续充当当前约束的情况下仍可恢复。

## ADDED Requirements

### Requirement: M32 原文可恢复

Carrier MUST 保留以下原始条目，不改变其状态、场景或措辞：

```markdown
### Requirement: M32 — lint 与测试 Gate 分层

后续 clippy/test 清理 proposal MUST 按 artifact、feature、target 和平台配置分层。可复用 crate 用 host check/test/clippy；kernel 用目标架构 + feature compile gate；IRQ/TTY/rootfs 行为用 QEMU/真板 gate。

**Legacy**: ADR-059 (A059), 2026-07-13 | **状态**: 候选

#### Scenario: 定义 clippy 和测试 gate

- **WHEN** 后续 change 清理 StarryOS 或 `uart_16550` 的 warning、clippy 和 tests
- **THEN** MUST 为可复用 crate、kernel target build 和系统 runtime 定义分离的 gate
```

#### Scenario: 恢复 M32

- **WHEN** Maintainer 收到经批准的 M32 恢复请求
- **THEN** MUST 从上述原文恢复编号、正文、状态和场景
