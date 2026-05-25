# 中断框架深度分析

> 基于 StarryOS axhal 中断实现的分析
> 分析日期：2026-05-24

---

## 1. RISC-V 中断层次

RISC-V 中断从硬件到软件的传递路径：

```
硬件信号 → PLIC → Hart 0 M-mode trap → trap_handler → 软件分发
```

### 1.1 特权级与中断入口

当前 StarryOS 的中断处理在 S-mode（Supervisor）下完成：
- `stvec` 指向 trap 入口（axhal/src/platform/qemu_virt_riscv/trap.S）
- `scause` 标识中断类型和来源
- `sie`（Supervisor Interrupt Enable）控制 S-mode 中断使能

### 1.2 PLIC（Platform-Level Interrupt Controller）

QEMU virt 平台 PLIC 配置：
- 基地址：0x0C00_0000
- 支持的外部中断源：最多 127 个
- UART 对应中断号：10（0x0a）
- 优先级阈值机制：低于阈值的优先级不会触发中断

---

## 2. 当前中断框架实现

### 2.1 初始化流程

```
axhal::init()
  └─ irq::init()
       ├─ plic::init(plic_base, hart_id)  // 初始化 PLIC
       ├─ plic::set_threshold(0)            // 设置优先级阈值为 0（允许所有）
       ├─ plic::enable(UART_IRQ)            // 使能 UART 中断
       ├─ plic::set_priority(UART_IRQ, 1)   // 设置 UART 优先级
       └─ 开启 S-mode 外部中断 (sie::set_sext())
```

### 2.2 Trap 处理流程

```
trap_entry (trap.S, 汇编)
  → 保存上下文
  → 调用 rust_trap_handler(trap_frame)
    → axhal::trap::rust_trap_handler()
      → match scause:
          Interrupt::SupervisorExternal → handle_supervisor_external()
          Interrupt::SupervisorTimer   → timer handler
          Exception::*                  → exception handler
```

**`handle_supervisor_external()` 的实现**：
```rust
fn handle_supervisor_external() {
    let irq = plic::claim();       // 获取最高优先级待处理中断号
    if irq != 0 {
        dispatch_irq(irq);         // 分发到注册的回调
        plic::complete(irq);       // 通知 PLIC 处理完成
    }
}
```

### 2.3 IRQ 注册机制

**当前有两种注册机制**：

#### 机制 1：`register_irq(irq, handler)` —— 原始回调

```rust
// axhal/src/irq.rs
static IRQ_HANDLERS: [SpinLock<Option<fn()>>; 128] = ...;

pub fn register_irq(irq: u32, handler: fn()) {
    IRQ_HANDLERS[irq as usize].lock().replace(handler);
}
```

特点：
- 回调类型是 `fn()`，无参数无返回值
- 在 trap 上下文中直接调用（中断上下文）
- **不能阻塞**、不能获取锁（可能死锁）
- 适合简单的"设置标志 + 唤醒"操作

#### 机制 2：`register_irq_waker(irq, waker)` —— Waker 注册

```rust
// axhal/src/irq.rs（通过 axprocess 的 per-cpu 区域）
pub fn register_irq_waker(irq: u32, waker: &Waker) {
    // 将 Waker 存入 per-cpu IRQ waker 表
    // 中断到来时调用 waker.wake_by_ref()
}
```

这是 N_TTY 当前使用的方式。中断到来时直接唤醒对应的内核任务。

### 2.4 中断分发到 Waker 的完整路径

```
UART 中断信号
  → PLIC claim (获取 irq=10)
    → dispatch_irq(10)
      → 查找 IRQ_WAKERS[10]
        → waker.wake_by_ref()
          → 将对应任务加入就绪队列
            → axtask 调度器在下次 yield 时恢复任务
```

---

## 3. 当前框架的问题

### 3.1 Waker 与回调的冲突

如果同时注册 `register_irq(10, handler)` 和 `register_irq_waker(10, waker)`，哪个生效？当前代码中两者是独立的存储，可能同时触发。**存疑：同时注册两种机制的语义是什么？**

### 3.2 无中断源识别

`dispatch_irq(10)` 只知道 "UART 中断来了"，不知道是 RX 就绪还是 TX 空还是其他。ISR 需要读取 UART 的 IIR 寄存器来判断，但当前 `dispatch_irq` 不会做这件事。

### 3.3 单一 Waker 限制

每个 IRQ 只能注册一个 Waker。如果同时有 RX 任务和 TX 任务等待同一个 UART 中断，当前机制不支持分别唤醒。

### 3.4 中断上下文约束

`dispatch_irq` 在 trap 上下文中执行，不能：
- 获取 SpinLock（可能死锁，如果被中断的任务持有该锁）
- 调用任何可能阻塞的函数
- 访问 per-CPU 数据（trap 上下文中 current_task 可能不正确）

只能做：
- 写原子变量
- 调用 `waker.wake_by_ref()`
- 读 MMIO 寄存器（无锁）

---

## 4. 异步串口对中断框架的需求

### 4.1 ISR 需要做的事

```
UART ISR:
  1. 读 IIR → 判断中断源 (RX/TX/其他)
  2. 根据中断源:
     RX 就绪 → 禁用 RX 中断 + 唤醒 RX copier
     TX 空   → 禁用 TX 中断 + 唤醒 TX copier
  3. 中断处理完毕（不需要额外清除，PLIC complete 已做）
```

### 4.2 需要的扩展

| 需求 | 当前状态 | 需要做的 |
|------|---------|---------|
| 区分 RX/TX 中断源 | 不支持 | ISR 中读 IIR |
| 分别唤醒 RX/TX 任务 | 不支持 | 双 Waker 或 AtomicWaker |
| ISR 中操作 UART 寄存器 | 不支持 | ISR 需要访问 MMIO |
| 禁用特定 UART 中断 | 不支持 | ISR 写 IER |

### 4.3 方案：ISR + AtomicWaker

```rust
// 异步 UART 的中断分发模型
static RX_WAKER: AtomicWaker = AtomicWaker::new();
static TX_WAKER: AtomicWaker = AtomicWaker::new();

fn uart_isr() {
    let iir = read_iir();
    let source = (iir >> 1) & 0x7;
    match source {
        0b010 => { // RX 就绪
            disable_rx_intr();   // IER &= ~0x01
            RX_WAKER.wake();     // 唤醒 RX copier
        }
        0b001 => { // TX 空
            disable_tx_intr();   // IER &= ~0x02
            TX_WAKER.wake();     // 唤醒 TX copier
        }
        _ => {}
    }
}
```

**关键约束**：`AtomicWaker::wake()` 和 MMIO 寄存器写入在中断上下文中是安全的（无锁、无阻塞）。

---

## 5. PLIC 操作参考

基于 `plic` crate 或直接 MMIO 操作：

| 操作 | 函数 | 说明 |
|------|------|------|
| 获取待处理中断 | `plic::claim()` | 返回最高优先级中断号，同时开始处理 |
| 完成中断处理 | `plic::complete(irq)` | 通知 PLIC 可以再次触发该中断 |
| 使能中断源 | `plic::enable(irq)` | 在 PLIC 层面使能 |
| 禁用中断源 | `plic::disable(irq)` | 在 PLIC 层面禁用 |
| 设置优先级 | `plic::set_priority(irq, prio)` | 优先级越高值越大 |
| 设置阈值 | `plic::set_threshold(thresh)` | 低于阈值的中断不会触发 |

**注意**：PLIC 的 enable/disable 与 UART 的 IER 是两层独立的使能控制。UART 中断路径需要两层都使能：
```
UART IER 使能 → PLIC enable → S-mode sie.sext → CPU 响应
```

禁用 UART 中断可以在任何一层做：
- 写 IER（推荐，精确控制 RX/TX）
- PLIC disable（粗粒度，影响整个 UART）
- 清 sie.sext（影响所有外部中断）

---

## 6. 存疑问题

| 编号 | 问题 | 影响 | 需要确认 |
|------|------|------|---------|
| Q4 | `register_irq` 和 `register_irq_waker` 同时注册同一 IRQ 时的语义？ | 决定是否需要统一中断分发机制 | 代码审查/实验 |
| Q5 | trap 上下文中读 MMIO 是否安全？是否有内存序问题？ | ISR 读 IIR 的可行性 | RISC-V 规范 |
| Q6 | 当前 N_TTY 的 tty-reader 任务具体是如何与 `register_irq_waker` 配合的？ | 理解现有异步模式 | 代码追踪 |
| Q7 | 多核场景下 PLIC claim/complete 的竞态？ | 当前单核可忽略，但长期需考虑 | RISC-V PLIC 规范 |