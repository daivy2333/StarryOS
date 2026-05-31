# tasks.md — 任务追踪

> 由 project-docs-assistant 维护，feat/uart-async-dev2 分支。
> 条目格式: <!-- Q{编号} --> 标记开头，支持 grep 精确定位。
> 方向 A（渐进式集成）和方向 B（完全剔除 Console 早期）已归档至 archive.md。

---

## 当前: 方向 C — kernel 层独立实现（feat/uart-async-dev2）

> 2026-05-31 stride=4 根因确认 + Q0 Spike 通过。异步串口在 kernel 层独立实现，不改外部 crate。
> 继承方向 A M1/M2 已验证架构（Ring Buffer + ISR→AtomicWaker→copier + DeviceOps + VFS）

### Milestone 概览

| Milestone | 目标 | Gate | 状态 |
|-----------|------|------|------|
| **Q0** | Spike 验证 | UART 寄存器可读写，ISR 正常 | ✅ 完成 |
| **Q1** | 驱动架构实现 | RX/TX copier + ISR + Ring Buffer | ✅ 完成 |
| **Q2** | VFS 集成 | DeviceOps + 设备注册 + poll/epoll | ⏳ |
| **Q3** | Console 共存/替换 | 内核日志 + 用户态 Shell 正常 | ⏳ |
| **Q4** | 性能优化 | P50<500µs, >90% 线速 | ⏳ |
| **Q5** | 真板验证 | VisionFive2 实际验证 | ⏳ 远期 |

### Q0: Spike 验证 ✅

<!-- Q0.1 --> - [x] 修复 UART_STRIDE=4→1（LoadFault 根因）✅
<!-- Q0.2 --> - [x] raw pointer 读 LSR 验证 MMIO 可访问 ✅
<!-- Q0.3 --> - [x] uart_16550 crate 读写 IER/ISR/LSR 验证 ✅
<!-- Q0.4 --> - [x] ISR handler 执行 + drain RX FIFO 验证 ✅
<!-- Q0.5 --> - [x] Gate Q0: 无 LoadFault/StoreFault，Shell 正常 ✅

**根因**: UART_STRIDE=4 使 ISR 读到 base+8（超出 NS16550 0x00-0x07 寄存器范围）→ LoadFault。

### Q1: 驱动架构实现 ⏳

<!-- Q1.1 --> - [x] 实现 AsyncBuffer（HeapRb + PollSet）✅
<!-- Q1.2 --> - [x] 实现 ISR AtomicWaker 分发 ✅
<!-- Q1.3 --> - [x] 实现 RX copier（ISR 唤醒 → 读 UART FIFO → 写 ringbuf）✅
<!-- Q1.4 --> - [x] 实现 TX copier（buf pop → 写 UART THR）✅
<!-- Q1.5 --> - [x] 实现 AsyncUartDriver + critical-section 适配 ✅
<!-- Q1.6 --> - [x] Gate Q1: copier 启动，Shell 正常，无 crash ✅

### Q2: VFS 集成 ⏳

<!-- Q2.1 --> - [ ] DeviceOps trait for AsyncUartDriver
<!-- Q2.2 --> - [ ] Pollable trait（poll + register）
<!-- Q2.3 --> - [ ] 注册 /dev/async_uart 到 devfs
<!-- Q2.4 --> - [ ] Gate Q2: 用户态 read/write + poll/epoll 通过

### Q3: Console 共存/替换 ⏳

<!-- Q3.1 --> - [ ] earlycon 内核日志方案
<!-- Q3.2 --> - [ ] Console 与 AsyncUart 硬件共存测试
<!-- Q3.3 --> - [ ] N_TTY 绑定到 AsyncUart
<!-- Q3.4 --> - [ ] Gate Q3: Shell 走 AsyncUart 正常

### Q4: 性能优化 ⏳

<!-- Q4.1 --> - [ ] 中断合并（FCR 阈值 + 软件延迟）
<!-- Q4.2 --> - [ ] NAPI 风格批量轮询
<!-- Q4.3 --> - [ ] 零拷贝 RX 路径探索
<!-- Q4.4 --> - [ ] Gate Q4: 性能基准达标

### Q5: 真板验证 ⏳

<!-- Q5.1 --> - [ ] VisionFive2 编译适配
<!-- Q5.2 --> - [ ] 串口收发功能测试
<!-- Q5.3 --> - [ ] Gate Q5: 真板正常运行

---

## 关键经验

### 已验证的模式（来自方向 A M0-M2）

1. Ring Buffer + 中断 + copier 任务模型 ✅
2. DeviceOps + 设备注册 + poll/epoll 支持 ✅
3. uart_16550 本地 path 依赖 + embassy-sync 集成 ✅

### 已修正的误判

1. **LoadFault 根因**: stride=4 越界，非"MMIO 权限阻塞"
2. **Console 能访问的原因**: 页表映射正常（mmio-ranges 中），非"初始化时机"
3. **无需修改 axplat**: kernel 层 stride=1 即可正常访问所有 UART 寄存器

### 方向 A M3 的真正失败原因

IRQ 风暴 + TX busy-loop — Console 只使能 RX 中断，AsyncUart 需 TX 中断，IER 配置冲突 + UART 状态不兼容
