# Spec Delta: learned — Carrier ARC-202607081429

## REMOVED Requirements

### Requirement: L078 — M3 替换失败

M3 替换失败的详细踩坑记录已从 active learned spec 移除。根因由 ADR-026 纠正，教训"dump 寄存器"在 L79。

#### Scenario: 恢复 L078

- **WHEN** 开发者需要回查 M3 替换失败的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L122 — Q1 critical-section 实现

Q1 critical-section 1.0 符号实现的详细记录已从 active learned spec 移除。实现已稳定，不需要重复查阅。

#### Scenario: 恢复 L122

- **WHEN** 开发者需要回查 critical-section 符号实现细节
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L123 — copier/Console FIFO 竞争

copier/Console FIFO 竞争的详细踩坑记录已从 active learned spec 移除。Q2/Q3 已解决，解决方案在 ADR-028。

#### Scenario: 恢复 L123

- **WHEN** 开发者需要回查 copier/Console 竞争的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L126 — TX copier 与 ax_println! 交错

TX copier 输出交错的详细踩坑记录已从 active learned spec 移除。Q4 已解决，解决方案在 ADR-029。

#### Scenario: 恢复 L126

- **WHEN** 开发者需要回查 TX 交错问题的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L131 — CPU 测试数据量统一

CPU 测试数据量不统一的踩坑记录已从 active learned spec 移除。Q5 已解决，benchmark 已统一。

#### Scenario: 恢复 L131

- **WHEN** 开发者需要回查数据量统一的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L134 — yield storm

三层嵌套 block_on/poll_io yield storm 的详细记录已从 active learned spec 移除。Q7 已解决。

#### Scenario: 恢复 L134

- **WHEN** 开发者需要回查 yield storm 的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L150 — NAPI 模式永不退出

NAPI 退出逻辑缺失的踩坑记录已从 active learned spec 移除。Q8 已修复。

#### Scenario: 恢复 L150

- **WHEN** 开发者需要回查 NAPI 退出 bug 的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L151 — ISR 中获取 SpinNoIrq 锁

ISR 中获取 SpinNoIrq 锁的踩坑记录已从 active learned spec 移除。Q8 已修复。

#### Scenario: 恢复 L151

- **WHEN** 开发者需要回查 ISR 锁问题的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L152 — IER 裸 write_volatile

IER 裸 write_volatile 绕过 API 的踩坑记录已从 active learned spec 移除。Q8 已修复。

#### Scenario: 恢复 L152

- **WHEN** 开发者需要回查 IER 裸写问题的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L153 — 读路径 5 次数据拷贝分析

读路径数据拷贝分析已从 active learned spec 移除。Q10 已优化。

#### Scenario: 恢复 L153

- **WHEN** 开发者需要回查读路径拷贝分析的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L154 — PollSet→AtomicWaker 迁移风险矩阵

PollSet 迁移风险矩阵已从 active learned spec 移除。O46 已完成。

#### Scenario: 恢复 L154

- **WHEN** 开发者需要回查 PollSet 迁移矩阵的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L155 — copier waker 去重简化

copier waker 去重的技巧记录已从 active learned spec 移除。已实现。

#### Scenario: 恢复 L155

- **WHEN** 开发者需要回查 waker 去重的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L158 — 5 OS 抽象 trait 表

5 OS 抽象 trait 表已从 active learned spec 移除。被 ADR-036 缩减为 2-trait，当前 API 见 L188-L192。

#### Scenario: 恢复 L158

- **WHEN** 开发者需要回查 5-trait 原始表的完整内容
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L159 — D1 决策需要推翻

D1 决策推翻的记录已从 active learned spec 移除。ADR-033/036 已完成提取。

#### Scenario: 恢复 L159

- **WHEN** 开发者需要回查 D1 决策推翻的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L221 — D1 当前无输出根因

D1 无输出根因的记录已从 active learned spec 移除。Q19 已解决，修复在 ADR-045。

#### Scenario: 恢复 L221

- **WHEN** 开发者需要回查 D1 无输出根因的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

### Requirement: L229 — D1/C906 Store/AMO fault 根因

D1 AMO fault 根因的记录已从 active learned spec 移除。Q19 已解决，PTE 修复在 ADR-046。

#### Scenario: 恢复 L229

- **WHEN** 开发者需要回查 D1 AMO fault 根因的完整记录
- **THEN** MUST 使用本 carrier spec 的 Compress-Archive 区

---

## 压缩保留（Compress-Archive 区）

<!-- L078 -->
### L078 (Compress-Archive, 2026-07-08)
M3 替换失败 — stride=4 + Console UART 状态不兼容（IER 冲突 + TX busy-loop）。
状态：已修复；根因由 ADR-026 纠正；教训"dump 寄存器"在 L79；Q0~Q7 独立实现替代。

<!-- L122 -->
### L122 (Compress-Archive, 2026-07-08)
Q1 critical-section 1.0 符号实现 — `_critical_section_1_0_acquire/release` + `disable_irqs/enable_irqs` 在 lib.rs。
状态：已稳定；实现已固化，不需要重复查阅。

<!-- L123 -->
### L123 (Compress-Archive, 2026-07-08)
copier/Console FIFO 竞争 — RX copier 抢先读 FIFO 导致 Shell 无键盘输入。
状态：已解决（Q2/Q3）；解决方案在 ADR-028（阶段切换）；教训"互斥 drain"已沉淀。

<!-- L126 -->
### L126 (Compress-Archive, 2026-07-08)
TX copier 与 ax_println! 输出交错 — 部分数据推回 ring buffer 导致乱序。
状态：已解决（Q4）；解决方案在 ADR-029（cursor 追踪，不推回）；教训"原子发送"已沉淀。

<!-- L131 -->
### L131 (Compress-Archive, 2026-07-08)
CPU 测试数据量统一 — Console 120B vs Async 102400B 差 853 倍，CPU 占用无法公平对比。
状态：已解决（Q5）；benchmark 已统一数据量 102400 字节；Async 效率高 14.3 倍。

<!-- L134 -->
### L134 (Compress-Archive, 2026-07-08)
三层嵌套 block_on/poll_io yield storm — Manual 模式 `waker.wake_by_ref()` 立即唤醒。
状态：已解决（Q7）；ProcessMode::External 消除立即唤醒。

<!-- L150 -->
### L150 (Compress-Archive, 2026-07-08)
NAPI 模式永不退出 — `consecutive` 只增不减，RX 中断永久禁用，CPU 空转。
状态：已修复（Q8）；添加 `if total == 0 { consecutive = 0; enable_rx_intr(); }` 退出分支。

<!-- L151 -->
### L151 (Compress-Archive, 2026-07-08)
ISR 中获取 SpinNoIrq 锁 — `uart_instance().lock()` 在 ISR 上下文，违反极简原则。
状态：已修复（Q8）；`read_isr_unlocked()` 无锁读取已实现。

<!-- L152 -->
### L152 (Compress-Archive, 2026-07-08)
IER 裸 `write_volatile` 绕过 uart_16550 API — 违反 MMIO 封装规则。
状态：已修复（Q8）；`set_ier()` 公共方法已添加。

<!-- L153 -->
### L153 (Compress-Archive, 2026-07-08)
读路径 5 次数据拷贝分析 — UART FIFO → copier → driver ringbuf → InputReader → ldisc → user。
状态：已优化（Q10）；C3/C4 合并已评估，每字节减少 1 次 memcpy。

<!-- L154 -->
### L154 (Compress-Archive, 2026-07-08)
PollSet→AtomicWaker 迁移风险矩阵 — pipe(HIGH)/signalfd(LOW)/pidfd(HIGH)/event(MEDIUM)。
状态：已完成（O46）；8 处迁移已完成。

<!-- L155 -->
### L155 (Compress-Archive, 2026-07-08)
copier waker 去重简化 — `will_wake` + `Cell<Option<Waker>>` 避免重复注册。
状态：已完成；已实现，每 poll 节省 ~2 次 Arc 原子操作。

<!-- L158 -->
### L158 (Compress-Archive, 2026-07-08)
5 OS 抽象 trait 表（OsRuntime/OsIrq/OsMmio/OsSpinNoIrq/OsWakerSet）— 跨平台 async UART 接口。
状态：已替代；ADR-036 缩减为 2-trait（OsRuntime + OsWakerSet）；当前 API 见 L188-L192。

<!-- L159 -->
### L159 (Compress-Archive, 2026-07-08)
D1 决策需要推翻 — uart_16550 ADR-7 "异步留在 wrapper 层"应改为完整 async crate。
状态：已执行；ADR-033/036 已完成提取；uart_16550 现为完整异步 UART crate。

<!-- L221 -->
### L221 (Compress-Archive, 2026-07-08)
D1 当前无输出根因 — ELF 链接到 QEMU 地址 `0xffffffc080200000`，lichee-d1 feature 只影响 entry 层。
状态：已解决（Q19）；根因和修复在 ADR-045（D1 axplat 接入）。

<!-- L229 -->
### L229 (Compress-Archive, 2026-07-08)
D1/C906 Store/AMO fault 根因 — early page table DDR 映射缺少 T-Head C9xx normal-memory PTE 属性。
状态：已解决（Q19）；PTE 修复在 ADR-046（`SH|B|C` bits 60/61/62）。
