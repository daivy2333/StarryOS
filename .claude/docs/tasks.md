# tasks.md — 任务追踪（汇总）

> 由 project-docs-assistant 维护，汇总分支整合。
> 条目格式: <!-- T{编号} --> 标记开头，支持 grep 精确定位。
> 汇总两个方向（渐进式集成 + 完全剔除 Console）的全部任务。

---

## 方向 A: 渐进式集成（feat/uart-async）

### Milestone 概览

| Milestone | 目标 | 底层引擎 | Gate | 状态 |
|-----------|------|----------|------|------|
| **M0** | 基础设施就绪 | — | `make run` + 中断回调触发 | ✅ 完成 |
| **M1** | 架构验证 | Console（同步） | Ring Buffer + 中断 + copier 流程正确 | ✅ 完成 |
| **M2** | VFS 验证 | Console（同步） | DeviceOps + 设备注册 + poll/epoll | ✅ 验证通过 |
| **M3** | 异步引擎替换 | AsyncUart | 用户态高性能 + 内核日志共存 | ❌ 失败回滚 |
| **M4** | 性能优化 | AsyncUart | 性能基准达标 | ⏳ 待重启 |
| **M5** | 真板验证 | AsyncUart | VisionFive2 实际验证 | ⏳ 待重启 |
| **M6** | DMA 探索 | AsyncUart+DMA | DMA 通道可用（远期） | ⏳ 远期 |

### M0: 基础设施就绪 ✅

<!-- T0.1 --> - [x] 添加 uart_16550 本地 path 依赖到 kernel/Cargo.toml ✅
<!-- T0.2 --> - [x] 添加 embassy-sync 依赖到 kernel/Cargo.toml ✅
<!-- T0.3 --> - [x] 中断机制确认（IRQ 10 共存语义）✅
<!-- T0.4 --> - [x] Gate M0 验证 ✅

### M1: 架构验证（Console 同步引擎）✅

<!-- T1.1 --> - [x] Ring Buffer 实现 ✅
<!-- T1.2 --> - [x] 中断机制验证（IRQ 10 共存）✅
<!-- T1.3 --> - [x] RX copier 任务模型验证 ✅
<!-- T1.4 --> - [x] TX 路径模拟验证 ✅
<!-- T1.5 --> - [x] 设备注册到 devfs ✅
<!-- T1.6 --> - [x] Gate M1 验证 ✅

### M2: VFS 验证（Console 包装）✅

<!-- T2.1 --> - [x] ConsoleDriver 实现 DeviceOps ✅
<!-- T2.2 --> - [x] 注册测试设备到 devfs ✅
<!-- T2.3 --> - [x] 功能验证（内核内部测试）✅
<!-- T2.4 --> - [ ] termios 支持框架（延后到 M3）

### M3: 异步引擎替换 ❌ 失败回滚

<!-- T3.1 --> - [x] AsyncUart trait + Uart16550Async 实现 ✅
<!-- T3.2 --> - [x] AsyncBuffer（Ring Buffer + PollSet）实现 ✅
<!-- T3.3 --> - [x] ISR（IsrContext + AtomicWaker）实现 ✅
<!-- T3.4 --> - [x] AsyncUartDriver（RX/TX copier）实现 ✅
<!-- T3.5 --> - [x] Module exports 完成 ✅
<!-- T3.6 --> - [ ] M3 集成验证 ❌ 失败（IRQ 风暴 + TX busy-loop）

**失败原因**: Console UART 状态不兼容 AsyncUart，IRQ 风暴 + TX busy-loop
**教训**: 见 learned.md L78-L80
**回滚**: commit d29a28f

---

## 方向 B: 完全剔除 Console（feat/uart-async-dev2）

### Milestone 概览

| Milestone | 目标 | 底层引擎 | Gate | 状态 |
|-----------|------|----------|------|------|
| **P0** | 项目规划与设计 | — | 分支创建 + 文档更新 | ✅ 完成 |
| **P1** | UART 硬件初始化替代 | uart_16550（本地） | UART 初始化成功 + 内核启动 | ⚠️ 部分通过 |
| **P2** | 异步串口架构实现 | uart_16550 + axtask | RX/TX copier + Ring Buffer + ISR | ❌ 阻塞 |
| **P3** | Console 软件路径剔除 | AsyncUart | Console 剔除 + AsyncUart 独占 | ⏳ 待重启 |
| **P4** | VFS 集成验证 | AsyncUart | DeviceOps + 设备注册 + 用户态 API | ⏳ 待重启 |
| **P5** | 性能优化 | AsyncUart | 性能基准达标 | ⏳ 待重启 |
| **P6** | 真板验证 | AsyncUart | VisionFive2 实际验证 | ⏳ 待重启 |

### P0: 项目规划与设计 ✅

<!-- P0.1 --> - [x] 创建 feat/uart-async-dev2 分支 ✅
<!-- P0.2 --> - [x] 提交 feat/uart-async 分支文档 ✅
<!-- P0.3 --> - [x] 回滚代码变更 ✅
<!-- P0.4 --> - [x] 更新文档体系 ✅
<!-- P0.5 --> - [x] 设计完全剔除 Console 方案 ✅

### P1: UART 硬件初始化替代 ⚠️ 部分通过

<!-- P1.1 --> - [x] 添加 uart_16550 + embassy-sync 依赖 ✅
<!-- P1.2 --> - [x] 创建驱动模块结构 ✅
<!-- P1.3 --> - [x] 实现 UART 硬件初始化函数 ✅
<!-- P1.4 --> - [x] 在内核启动流程调用 UART 初始化 ⚠️ MMIO 权限问题
<!-- P1.5 --> - [x] Gate P1 验证 ⚠️ 内核启动成功，但 UART 配置无法验证

**关键阻塞**: UART MMIO 权限问题（见 learned.md L110）

### P2: 异步串口架构实现 ❌ 阻塞

<!-- P2.0 --> - [x] ISR UART MMIO 权限测试 ✅ 失败验证
<!-- P2.1 --> - [x] 实现 ISR 分发机制（测试版）✅ 无法访问 UART

**关键阻塞**: ISR 也无法访问 UART 寄存器（见 ADR-023）

---

## 汇总: 关键经验

### 成功经验

1. **M0-M2 架构验证成功**: Ring Buffer + 中断 + copier 任务模型验证通过
2. **VFS 集成成功**: DeviceOps trait + 设备注册 + poll/epoll 支持验证通过
3. **依赖管理成功**: uart_16550 本地 path 依赖 + embassy-sync 集成
4. **文档体系完善**: 两个方向都建立了完整的文档体系

### 失败经验

1. **M3 替换失败**: Console UART 状态不兼容 AsyncUart（IRQ 风暴 + TX busy-loop）
2. **MMIO 权限阻塞**: axplat 限制 UART MMIO 权限，内核/ISR 都无法访问
3. **ISR 测试失败**: ISR 上下文也无法访问 UART 寄存器（LoadFault）

### 关键发现

1. **Console 配置限制**: Console 只使能 RX 中断，不使能 TX 中断
2. **MMIO 权限根因**: axplat 在 boot 阶段映射 UART MMIO，权限被限制
3. **外部 crate 约束**: axplat/axhal/axtask 均来自 crates.io，不可直接修改
4. **绕过路径**: 需要修改 axplat 或在 boot 阶段修改页表权限

---

## 下一步（待决策）

### 方案 A: Polling TX（半异步）
- RX 中断驱动 + TX polling
- 优点：简单可行，无需修改外部 crate
- 缺点：TX 性能受限
- 工作量：~1 周

### 方案 B: 修改 axplat（完整异步）
- Boot 阶段修改 UART MMIO 映射权限
- 优点：完整异步 TX
- 缺点：需修改外部 crate，复杂度高
- 工作量：~2 周

### 方案 C: 回退 Console（渐进式）
- 完全依赖 Console（方向 A 的回退方案）
- 优点：Console RX 已中断驱动
- 缺点：放弃 AsyncUart 独占目标
- 工作量：~1 周
