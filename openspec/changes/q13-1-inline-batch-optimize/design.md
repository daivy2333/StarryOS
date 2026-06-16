## Context

Q13 完成异步串口提取后，性能测试显示：
- Q12 基线：1B avg 124µs / overhead 37.1µs
- Q13 实测：1B avg 140.1µs / overhead 53.3µs
- 性能退化：+13% avg latency，+44% overhead

退化原因：引入 trait 抽象层（UartPort、OsWakerSet 等）后，热路径上增加了间接调用开销。

## Goals / Non-Goals

**Goals:**
- 通过 `#[inline(always)]` 消除函数调用开销
- 通过批量操作减少锁获取次数
- 恢复到 Q12 基线水平（1B avg ≤ 130µs）
- 保持完全可移植性（无 feature gate）

**Non-Goals:**
- 不引入 feature gate 条件编译
- 不修改 trait 定义
- 不引入 DMA 或零拷贝优化

## Decisions

### Decision 1: #[inline(always)] 策略

**选择**：为热路径函数添加 `#[inline(always)]`

**替代方案**：
- `#[inline]`（编译器提示）→ 编译器可能不内联
- 不添加注解 → 编译器自行决定

**理由**：
- 热路径函数调用频繁，内联收益明确
- 函数体小，内联不会导致代码膨胀
- 编译器在 `release` 模式下通常会内联，但 `#[inline(always)]` 确保一致性

### Decision 2: 批量操作接口

**选择**：添加 `push_batch` 和 `pop_batch` 方法

**替代方案**：
- 修改现有 `push`/`pop` 方法 → 破坏现有 API
- 使用迭代器 → 增加复杂度

**理由**：
- 新增方法不破坏现有 API
- 批量操作减少锁获取次数
- 与现有 `receive_bytes`/`send_bytes` API 风格一致

### Decision 3: 锁优化策略

**选择**：保持现有锁粒度，通过批量操作减少锁获取次数

**替代方案**：
- 细粒度锁 → 增加复杂度
- 无锁设计 → 需要重新架构

**理由**：
- 批量操作已能显著减少锁获取次数
- 锁优化收益有限（~5-10ns/次）
- 保持代码简单性

## Risks / Trade-offs

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| 代码膨胀 | I-cache 命中率下降 | 仅对小函数内联，监控代码大小 |
| 批量延迟 | 增加单字节延迟 | 保持批量大小合理（16-64 字节） |
| 编译器优化不稳定 | 不同优化级别表现不同 | 使用 `#[inline(always)]` 确保一致性 |

## Migration Plan

**Phase 1: 内联优化**
1. 为热路径函数添加 `#[inline(always)]`
2. 验证编译和功能

**Phase 2: 批量操作**
1. 添加 `push_batch`/`pop_batch` 方法
2. 修改 copier loop 使用批量操作
3. 验证编译和功能

**Phase 3: 性能验证**
1. 运行 benchmark 对比
2. 验证功能完整性

**回滚策略**：
- 每个 Phase 独立提交，可单独回滚
- 保留优化前代码注释

## Open Questions

1. **批量大小**：最佳批量大小是多少？（当前 COPIER_BUF_SIZE=1024）
2. **内联范围**：是否需要为 trait 方法添加 `#[inline]`？
3. **编译器行为**：不同优化级别下内联行为是否一致？
