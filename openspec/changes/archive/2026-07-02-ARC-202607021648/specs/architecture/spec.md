# Spec Delta: architecture — ARC-202607021648

## REMOVED Requirements

### Requirement: ADR-035 5-trait OS abstraction is superseded

ADR-035 MUST be treated as archived history because ADR-036 replaced its 5-trait interface with the active 2-trait minimum interface.

#### Scenario: Looking up async UART OS traits

- **WHEN** developers need the current async UART OS abstraction contract
- **THEN** they MUST read ADR-036 instead of ADR-035
- **AND** ADR-035 MAY only be used as historical rationale for why the 5-trait design was removed.

## 完整保留（Archive 区）

### A035 (Archive, architecture 2026-07-02)

**原编号**: `<!-- A035 -->`
**标题**: ADR-035: 异步 UART 通过 5 个 OS 抽象 trait 跨 OS 可移植
**归档原因**: 被 ADR-036 “OS abstraction 缩减至 2-trait 最小接口”明确替代。

```markdown
<!-- A035 -->
### Requirement: ADR-035: 异步 UART 通过 5 个 OS 抽象 trait 跨 OS 可移植

The async UART stack MUST NOT call any concrete OS API directly; all OS dependencies SHALL go through abstraction traits.

**日期**: 2026-06-17
**状态**: 已接受
**决策**: `uart_16550` 异步栈不直接调用任何具体 OS 服务，而是定义 5 个 OS 抽象 trait（`OsRuntime` / `OsIrq` / `OsMmio` / `OsSpinNoIrq` / `OsWakerSet`），由目标 OS 实现这些 trait 后即可复用异步栈。

**背景**: Q13 提取后必须解决具体 OS 调用边界；当时采用 5-trait 抽象。

**核心内容**:
- `OsRuntime`: 异步任务生成 + 同步等待
- `OsIrq`: 中断处理函数注册
- `OsMmio`: 物理地址到虚拟地址映射
- `OsSpinNoIrq<T>`: 关中断自旋锁
- `OsWakerSet`: 多 waker 集合

**后续修正**: ADR-036 证明其中 `OsIrq` / `OsMmio` / `OsSpinNoIrq` 未被 driver code 使用，实际最小接口为 `OsRuntime` + `OsWakerSet`。
```
