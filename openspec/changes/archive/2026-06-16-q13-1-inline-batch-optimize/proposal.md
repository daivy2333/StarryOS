## Why

Q13 完成异步串口提取后，性能测试显示 trait 抽象开销导致 +13% avg latency 退化（124µs → 140.1µs）。需要通过 `#[inline(always)]` 和批量操作优化减少开销，目标是恢复到 Q12 基线水平。

**为什么现在做**：
1. Q13 已完成基本功能，需要验证性能
2. 短期优化（inline + batch）无可移植性损失
3. 为后续优化建立性能基线

## What Changes

- **修改**：uart_16550 热路径函数添加 `#[inline(always)]`
- **修改**：uart_16550 ring buffer 添加批量 push/pop 接口
- **修改**：StarryOS copier loop 使用批量操作
- **验证**：benchmark 对比 Q12 基线

## Capabilities

### New Capabilities

- `inline-batch-optimize`: 热路径内联和批量操作优化

### Modified Capabilities

- 无

## Impact

**受影响的代码**：
- `uart_16550/src/async_/driver.rs` — copier loop 热路径
- `uart_16550/src/async_/ring_buffer.rs` — push/pop 方法
- `uart_16550/src/os/mod.rs` — trait 方法（可选）
- `kernel/src/drivers/os_arceos.rs` — ArceOS 适配层方法

**预期收益**：
- 1B avg latency: 140.1µs → 125~130µs
- overhead: 53.3µs → 38~43µs
