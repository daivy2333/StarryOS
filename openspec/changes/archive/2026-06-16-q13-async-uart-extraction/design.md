## Context

StarryOS 已完成 Q0~Q12 共 ~618 行异步串口实现，Q13 Phase 1 已将 TtyRead/TtyWrite trait 提取到 uart_16550 crate。当前架构：

- **StarryOS 内核**：7 个文件实现完整异步串口栈（uart_init.rs, isr.rs, ring_buffer.rs, async_driver.rs, device_ops.rs, ntty_async.rs, mod.rs）
- **uart_16550 crate**：仅提供同步 HAL（Uart16550<MmioBackend>），无异步支持
- **依赖关系**：StarryOS 依赖 uart_16550 的同步 API，异步逻辑完全在内核层

**约束**：
1. 不修改 uart_16550 的现有同步 API
2. 不引入 embassy-executor（仅使用 embassy-sync 的 AtomicWaker）
3. 保持 ISR 极简原则（只读 ISR / 禁中断 / wake / 返回）
4. 保持 NS16550 stride = 1

## Goals / Non-Goals

**Goals:**
- 将 StarryOS 异步串口实现提取到 uart_16550 crate
- 定义 5 个 OS 抽象 trait 实现跨平台可复用
- 保持性能不退化（benchmark 对比 Q12 基线）
- 提供 `async` feature gate 允许用户选择启用

**Non-Goals:**
- 不修改 uart_16550 的同步 API
- 不引入 embassy-executor
- 不支持 DMA（保留给未来 Q6 真板验证）
- 不向上游 rust-osdev/uart_16550 提交 PR（短期）

## Decisions

### Decision 1: OS 抽象 trait 设计

**选择**：5 个独立 trait（OsRuntime, OsIrq, OsMmio, OsSpinNoIrq, OsWakerSet）

**替代方案**：
- 单一 `Os` trait 包含所有方法 → 耦合度高，用户只需实现部分能力
- 使用 `embedded-hal` traits → 不覆盖 ISR 注册、MMIO 映射等 OS 特定需求

**理由**：
- 正交性：每个 trait 覆盖一个独立的 OS 能力
- 可组合：用户只需实现实际需要的 trait
- 可测试：每个 trait 可独立 mock 测试

### Decision 2: 全局状态管理

**选择**：`OnceLock` + 泛型参数，允许调用方提供静态存储

**替代方案**：
- `lazy_static!` → 无法跨平台（依赖 std）
- `static mut` → unsafe，不符合 Rust 最佳实践

**理由**：
- `OnceLock` 是 `no_std` 兼容的
- 泛型参数允许调用方控制静态存储位置
- 避免 `lazy_static` 的 `std` 依赖

### Decision 3: Feature Gate 策略

**选择**：`async` feature gate 控制异步模块编译

**替代方案**：
- 独立 crate（uart_16550-async）→ 增加维护负担
- 默认启用 → 破坏现有用户

**理由**：
- 单 crate 简化依赖管理
- feature gate 保持向后兼容
- 用户显式选择启用异步支持

### Decision 4: 中断注册时机

**选择**：通过 `OsIrq::register_handler` 延迟到 `init()` 调用

**替代方案**：
- 编译时静态注册 → 无法跨平台
- 构造函数注册 → 过早，硬件未初始化

**理由**：
- 延迟注册允许调用方控制初始化顺序
- 与 StarryOS 现有 `init_uart_hardware()` 流程一致

## Risks / Trade-offs

| 风险 | 影响 | 缓解措施 |
|------|------|----------|
| **trait 抽象层开销** | 间接调用可能影响性能 | 使用 `#[inline(always)]` + 泛型单态化，确保零成本抽象 |
| **embassy 版本锁定** | `embassy-sync` v0.6.2 可能与用户项目冲突 | 使用 feature gate 允许用户选择 embassy 版本 |
| **全局状态管理** | `OnceLock` 初始化顺序依赖 | 文档明确初始化顺序要求 |
| **测试覆盖** | QEMU 无法测试真板行为 | 保留 Q6（VisionFive2 真板验证）作为最终验收 |
| **代码量增加** | uart_16550 crate 代码量增加 ~400 行 | 代码结构清晰，模块化良好 |

## Migration Plan

**Phase 2（核心异步逻辑迁移）**：
1. 在 uart_16550 中定义 5 个 OS trait
2. 迁移 isr.rs, ring_buffer.rs, async_driver.rs, device_ops.rs
3. 验证：`cargo check` + 单元测试

**Phase 3（StarryOS 适配层）**：
1. 实现 ArceOS 适配层（os_arceos.rs）
2. StarryOS 从 uart_16550 导入异步实现
3. 删除已迁移的本地代码
4. 验证：全量 benchmark 回归测试

**回滚策略**：
- 每个 Phase 独立提交，可单独回滚
- 保留本地代码注释（不删除），便于快速恢复

## Open Questions

1. **embassy 版本兼容性**：是否需要支持多个 embassy 版本？
   - 当前锁定 v0.6.2，与 StarryOS 一致
   - 如果用户需要其他版本，可通过 feature gate 选择

2. **测试策略**：是否需要在 uart_16550 中添加单元测试？
   - 当前仅依赖 QEMU 集成测试
   - 可考虑添加 mock 测试（future 优化）

3. **文档策略**：是否需要为 async feature 编写独立文档？
   - 当前文档集中在 StarryOS
   - 需要为 uart_16550 的 async feature 编写使用指南
