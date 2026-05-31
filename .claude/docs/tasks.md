# tasks.md — 任务追踪

> 由 project-docs-assistant 维护，feat/uart-async-dev2 分支。
> 条目格式: <!-- Q{编号} --> 标记开头，支持 grep 精确定位。
> 方向 A（渐进式集成）和方向 B（完全剔除 Console 早期）已归档至 archive.md。

---

## 当前: 方向 C — kernel 层独立实现（feat/uart-async-dev2）

> 2026-05-31 stride=4 根因确认 + Q0-Q4 全部通过。异步串口在 kernel 层独立实现，Shell 双向异步。

### Milestone 概览

| Milestone | 目标 | Gate | 状态 |
|-----------|------|------|------|
| **Q0** | Spike 验证 | UART 寄存器可读写，ISR 正常 | ✅ |
| **Q1** | 驱动架构实现 | RX/TX copier + ISR + Ring Buffer | ✅ |
| **Q2** | VFS 集成 | DeviceOps + /dev/async_uart + Console 共存 | ✅ |
| **Q3** | AsyncUart RX 接管 | Tty<AsyncUartReader, ConsoleWriter> → Shell stdin | ✅ |
| **Q4** | 全异步 RX+TX | TX copier + ISR，Shell 双向异步 | ✅ |
| **Q5** | 性能优化 | 中断合并 + NAPI + 零拷贝 | ✅ |
| **Q6** | 真板验证 | VisionFive2 | ⏳ 等待硬件 |

---

## 最终状态

```
Q0 ✅ Q1 ✅ Q2 ✅ Q3 ✅ Q4 ✅ Q5 ✅ Q6 ⏳(硬件)
```

**已实现**: kernel 层独立异步串口栈，不修改任何外部 crate（axplat/axhal/axtask）。
- Shell stdin: ISR → RX copier → ring buffer → AsyncUartReader → Tty → Shell
- Shell stdout: Shell → Tty → AsyncUartWriter → ring buffer → TX copier → UART
- 内核日志: ax_println! → Console polling TX（共存）
- /dev/async_uart: DeviceOps + Pollable，用户态可 open/read/write/poll
- 性能优化: IER 缓存、ISR 合并、批量 I/O、rx/tx 独立锁、waker skip

### Q0: Spike 验证 ✅

<!-- Q0.1 --> - [x] 修复 UART_STRIDE=4→1（LoadFault 根因）✅
<!-- Q0.2 --> - [x] raw pointer 读 LSR 验证 MMIO 可访问 ✅
<!-- Q0.3 --> - [x] uart_16550 crate 读写 IER/ISR/LSR 验证 ✅
<!-- Q0.4 --> - [x] ISR handler 执行 + drain RX FIFO 验证 ✅
<!-- Q0.5 --> - [x] Gate Q0: 无 LoadFault/StoreFault，Shell 正常 ✅

### Q1: 驱动架构实现 ✅

<!-- Q1.1 --> - [x] 实现 AsyncBuffer（HeapRb + PollSet）✅
<!-- Q1.2 --> - [x] 实现 ISR AtomicWaker 分发 ✅
<!-- Q1.3 --> - [x] 实现 RX copier（ISR 唤醒 → 读 UART FIFO → 写 ringbuf）✅
<!-- Q1.4 --> - [x] 实现 TX copier（buf pop → 写 UART THR）✅
<!-- Q1.5 --> - [x] 实现 AsyncUartDriver + critical-section 适配 ✅
<!-- Q1.6 --> - [x] Gate Q1: copier 启动，Shell 正常，无 crash ✅

### Q2: VFS 集成 + Console 共存 ✅

<!-- Q2.1 --> - [x] DeviceOps trait for AsyncUartDriver ✅
<!-- Q2.2 --> - [x] Pollable trait ✅
<!-- Q2.3 --> - [x] 注册 /dev/async_uart 到 devfs ✅
<!-- Q2.4 --> - [x] copier OFF 时 Console 正常（避免 FIFO 竞争）✅

### Q3: AsyncUart RX 接管 ✅

<!-- Q3.1 --> - [x] AsyncUartReader/Writer + ConsoleWriter（TtyRead/TtyWrite）✅
<!-- Q3.2 --> - [x] RX copier 启用 + ISR AtomicWaker ✅
<!-- Q3.3 --> - [x] Tty<AsyncUartReader, ConsoleWriter> 绑定 Shell ✅
<!-- Q3.4 --> - [x] Gate Q3: Shell stdin 走异步，stdout 走 Console，输入输出正常 ✅

### Q4: 全异步 RX+TX ✅

<!-- Q4.1 --> - [x] 启用 TX copier + ISR TX 中断流程 ✅
<!-- Q4.2 --> - [x] 切换 AsyncUartWriter → ring buffer TX ✅
<!-- Q4.3 --> - [x] TX copier: enable_tx_intr on partial send ✅
<!-- Q4.4 --> - [x] Gate Q4: Shell stdin/stdout 双向异步，内核日志共存 ✅

### Q5: 性能优化 ⏳ 当前

<!-- Q5.1 --> - [ ] 中断合并（FCR 阈值 + 软件延迟）
<!-- Q5.2 --> - [ ] NAPI 风格批量轮询
<!-- Q5.3 --> - [ ] 零拷贝 RX 路径探索
<!-- Q5.4 --> - [ ] Gate Q5: 性能基准达标

### Q6: 真板验证 ⏳

<!-- Q6.1 --> - [ ] VisionFive2 编译适配
<!-- Q6.2 --> - [ ] 串口收发功能测试
<!-- Q6.3 --> - [ ] Gate Q6: 真板正常运行

---

## 关键经验

### 已验证的模式

1. Ring Buffer + 中断 + copier 任务模型 ✅
2. DeviceOps + 设备注册 + poll/epoll 支持 ✅
3. uart_16550 本地 path 依赖 + embassy-sync 集成 ✅
4. Tty<R,W> 泛型绑定：实现 reader/writer trait 即可替换终端栈 ✅

### 已修正的误判

1. **LoadFault 根因**: stride=4 越界，非"MMIO 权限阻塞"
2. **Console 能访问的原因**: 页表映射正常（mmio-ranges 中），非"初始化时机"
3. **无需修改 axplat**: kernel 层独立实现完全可行
4. **copier/Console 竞争**: RX copier 不能与 Console tty-reader 共用 FIFO

### 方向 A M3 的真正失败原因

IRQ 风暴 + TX busy-loop — Console + AsyncUart 共享 UART 时的 IER 冲突和 stride=4 错误

### Q5: 性能优化 ✅

<!-- Q5.1 --> - [x] O27 IER 缓存（AtomicU8，消除 RMW MMIO 读）✅
<!-- Q5.2 --> - [x] O28 ISR 合并（单次 SpinNoIrq 临界区）✅
<!-- Q5.3 --> - [x] O29 COPIER_BUF_SIZE 256→1024 ✅
<!-- Q5.4 --> - [x] O30 TX 消除 double buffer lock ✅
<!-- Q5.5 --> - [x] O25/O26 RX/TX 批量 I/O（单锁内排空/填满）✅
<!-- Q5.6 --> - [x] O31 AtomicWaker skip re-register（will_wake 检查）✅
<!-- Q5.7 --> - [x] Gate Q5: Shell 正常，无 crash，`ls /`、`cd`、`pwd` 全部工作 ✅
