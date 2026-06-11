# AsyncUart 实现历程 — 策略演进与架构决策

> Part of StarryOS codebase analysis (branch: asyncuart-dev) | Generated 2026-06-11
> Based on: `docs/analysis/async-uart-implementation-history.md` (2026-05-29)
> See also: `openspec/specs/architecture/spec.md` (ADR-019 ~ ADR-031), `openspec/specs/learned/spec.md` (L78–L140)

---

## §1 项目目标与初始状态

### 1.1 核心目标

为 StarryOS (RISC-V 宏内核，基于 ArceOS 组件化架构) 实现**高性能异步串口通信**，性能指标：

| 指标 | 目标值 | 说明 |
|------|--------|------|
| 波特率 | 115200 bps (可扩展至 1 Mbps) | 支持高速率 |
| RX 延迟 | P50 < 500 µs, P99 < 2 ms | 低延迟中断驱动 |
| CPU 空闲利用率 | 0% | 无 polling 空转 |
| 吞吐量 | > 90% 线速 | 接近硬件极限 |

### 1.2 初始状态

项目起点的 UART 栈由外部 crate `axplat` 管理：

- **Console TX**：同步阻塞 polling 空转（`for _ in 0..THR_EMPTY_RETRIES { try_write }`）
- **Console RX**：中断驱动（`IER::DATA_READY`）
- **Console UART 配置**：只使能 RX 中断，**禁用 TX 中断**（`IER::THR_EMPTY` 未置位）
- **MMIO 映射**：`axplat` 在 boot 阶段映射 UART 物理地址 0x10000000，封装为 `MmioSerialPort`

**架构约束**：`axplat` 为外部 crate，不可修改。其 UART 配置状态不透明，无法从 kernel 层查询或变更。

---

## §2 策略演进

项目经历了三个方向的探索，前两次失败暴露了关键架构约束，第三次在妥协中成功落地。

### 2.1 时间线总览

```
2026-05-24 ─┬─ Direction A: feat/uart-async (渐进式集成)
            │   M0–M2 ✅ → M3 ❌ (IRQ风暴 + TX busy-loop)
            │
2026-05-28 ─┼─ Direction B: feat/uart-async-dev2 (完全剔除 Console)
            │   P1.3 ❌ StoreFault → P2.1 ❌ LoadFault → 阻塞
            │
2026-05-30 ─┴─ Direction C: asyncuart-dev (kernel 独立 + Console 共存)
                Q0–Q7 ✅ (ISR极简 + copier任务 + AtomicWaker)
```

### 2.2 Direction A — 渐进式集成 (feat/uart-async) ❌

**策略**：复用 Console UART 初始化，渐进式替换 Console 软件路径。

- **M0–M2 (✅ 完成)**：ConsoleDriver、RX copier、VFS 集成
- **M3 (❌ 失败)**：AsyncUart 替换 Console TX

**失败症状**：

| 症状 | 表现 | 根因 |
|------|------|------|
| IRQ 风暴 | RX-COPIER 与 tty-reader 快速循环唤醒 | 两个任务竞争同一 IRQ |
| TX busy-loop | TX FIFO 满，LSR=0x00，retry 无效 | Console 禁用 TX 中断，AsyncUart 需要 TX 中断 |
| UART 状态异常 | 硬件未正常发送数据 | Console UART 配置不兼容 AsyncUart |

**关键教训**：
- Console 与 AsyncUart 共用 UART 硬件导致三类数据竞争：TX 写竞争、IRQ waker 冲突、重初始化冲突
- 未验证 UART 硬件状态就开始集成（缺少 IIR/MCR/LSR 调试）
- `THR_EMPTY` 状态理解错误

参见：ADR-019, learned.md L78–L80

### 2.3 Direction B — 完全剔除 Console (feat/uart-async-dev2) ❌

**策略**：完全回滚代码，从零实现 AsyncUart，使用本地 `uart_16550` crate 独立初始化 UART 硬件，剔除 Console 软件路径。

**关键架构决策 (ADR-021)**：
1. UART 硬件初始化替代：`uart_16550` crate 本地初始化，使能 `IER::DATA_READY | IER::THR_EMPTY`
2. earlycon 内核日志：复用 `axhal::console` polling TX
3. AsyncUart 设备注册：`DeviceOps` + `Pollable` trait，设备节点 `/dev/async_uart`
4. IRQ waker 分发：ISR 读 ISR 寄存器判断 `InterruptType`，精确唤醒 `rx_waker` / `tx_waker`

**阻塞发现**：

| 阻塞点 | 时间 | 症状 | 根因 |
|--------|------|------|------|
| P1.3 内核 UART 访问 | 05-28 | `StoreFault @ 0xffffffc01000001c` | axplat 在 boot 后限制 MMIO 权限 |
| P2.1 ISR UART 访问 | 05-29 | `LoadFault @ 0xffffffc010000008` | MMIO 限制对所有上下文生效（内核 + ISR） |

**ISR 验证过程**：
1. ✅ ISR handler 成功注册 (`axhal::register_irq_hook`)
2. ✅ ISR handler 成功执行（内核日志输出确认）
3. ❌ ISR 尝试读 UART ISR 寄存器 → `LoadFault` (stval=`0xffffffc010000008`)

**结论**：MMIO 权限限制对**所有上下文**生效。`phys_to_virt` 转换后虚拟地址无访存权限。axplat 的外部 crate 约束无法绕过 — **不彻底更改底层支持就无法实现异步 TX**。

参见：ADR-022, ADR-023, learned.md L113–L116

### 2.4 Direction C — kernel 独立 + Console 共存 (asyncuart-dev) ✅

**策略**：接受 axplat 的 Console UART 配置不可变更的约束，在 kernel 层实现**独立异步串口子系统**，与 Console 共存。

**核心设计原则 (ISR 极简原则)**：
1. ISR 仅做：读 ISR 寄存器 → 禁对应中断 → `AtomicWaker::wake()` → 返回
2. **禁止**在 ISR 中做数据搬运（FIFO → ring buffer）和锁操作
3. 数据搬运完全在 copier 任务中完成

**里程碑**：Q0 (基础架构) → Q1 (RX copier) → Q2 (TX copier) → Q3 (VFS 集成) → Q4 (epoll) → Q5 (性能优化) → Q6 (非阻塞模式) → Q7 (跨层状态传播) — **全部落地，等待真板验证**。

---

## §3 最终架构

### 3.1 架构全景

```
┌──────────────────────────────────────────────────────┐
│                    用户态                              │
│  read(fd) / write(fd) / poll(fds) / epoll_wait()    │
└──────────┬───────────────────────────────────────────┘
           │ VFS
┌──────────▼───────────────────────────────────────────┐
│                  kernel 层                             │
│                                                       │
│  ┌──────────────────┐    ┌──────────────────┐        │
│  │   AsyncUart       │    │   Console         │        │
│  │  (DeviceOps +     │    │  (axhal::console) │        │
│  │   Pollable)       │    │  polling TX       │        │
│  └──────┬───────────┘    └──────────────────┘        │
│         │                                               │
│  ┌──────▼──────────────────────────────────┐          │
│  │         uart_16550 crate                 │          │
│  │  Uart16550<MmioBackend> (stride=1)       │          │
│  │  NS16550 寄存器安全封装                    │          │
│  └──────┬──────────────────────────────────┘          │
│         │ MMIO                                         │
│  ┌──────▼──────────────────────────────────┐          │
│  │          axplat UART MMIO 映射            │          │
│  │  (boot 阶段映射，完整权限 RW)              │          │
│  └─────────────────────────────────────────┘          │
└──────────────────────────────────────────────────────┘
           │
┌──────────▼───────────────────────────────────────────┐
│                    硬件层                              │
│              NS16550 UART (0x10000000)                │
│              IRQ 10 (RISC-V PLIC)                     │
└──────────────────────────────────────────────────────┘
```

### 3.2 数据流

```
RX 方向:
  UART FIFO ──IRQ──▶ ISR ──AtomicWaker::wake()──▶ rx_copier 任务
    rx_copier: 读 FIFO → push ring buffer → PollSet::wake(rx) → 唤醒 read()

TX 方向:
  write() → poll_fn 等待 TX 空间 → 数据入 ring buffer
    → tx_copier 任务: ring buffer → pop → 写 THR → 使能 THR_EMPTY 中断
    UART THR_EMPTY ──IRQ──▶ ISR ──AtomicWaker::wake()──▶ tx_copier 继续写
```

### 3.3 关键设计决策

| 决策 | 依据 | 文档 |
|------|------|------|
| stride = 1 (非 4) | NS16550 寄存器仅 8 字节；stride=4 导致 LoadFault | learned.md L122 |
| ISR 极简 (4 步) | 防止 ISR 中数据竞争和死锁；保持中断延迟可控 | 项目规则 §ISR 极简原则 |
| 双 copier 任务 (rx + tx) | 与 TTY 单任务独占不同，支持独立读写并发 | ADR-026 |
| MMIO 封装在 `uart_16550` crate | 禁止裸写硬件地址；所有寄存器操作走安全 API | 项目规则 §MMIO 封装 |
| Console 共存 | axplat 不可修改；Console 保留 polling TX 用于内核日志 | ADR-021 earlycon 方案 |
| `AtomicWaker` (embassy_sync) | 替代 PollSet 用于 ISR→任务通知；ISR 中无锁安全 | ADR-027, Q8.6–9 |

### 3.4 关键文件

| 文件 | 职责 |
|------|------|
| `kernel/src/drivers/uart.rs` | AsyncUart 设备实现 (DeviceOps + Pollable) |
| `kernel/src/drivers/copier.rs` | RX/TX copier 任务 |
| `kernel/src/drivers/isr.rs` | UART ISR handler (极简) |
| `kernel/src/entry.rs` | 内核入口：ISR 注册、UART 初始化 |
| `uart_16550/src/` | NS16550 寄存器定义与安全封装 (子项目) |
| `axplat-riscv64-qemu-virt/console.rs` | Console UART (外部 crate，不可修改) |

---

## §4 经验教训

### 4.1 架构约束类

**MMIO 权限不可绕过** (learned.md L113–L116)

axplat 在 boot 后限制 UART MMIO 访问权限，该限制对所有上下文（内核任务、ISR）生效。`phys_to_virt` 转换无效。**唯一可行路径**：在 axplat 的 boot 映射阶段完成 UART 初始化，或接受其初始配置。

**外部 crate 约束链** (learned.md L88)

Console (`axhal::console`) 是外部 crate，其 UART 配置不透明、TX 中断禁用、TX 使用 polling。任何共享 UART 硬件的方案都必须兼容这些约束，否则触发数据竞争。Direction C 的"共存"方案正是接受此约束的结果。

**跨层状态传播需穷举入口** (learned.md L140)

`O_NONBLOCK` / `FIONBIO` 等跨层状态有三个入口（`open`、`fcntl`、`ioctl`），遗漏任一即功能不完整。

### 4.2 硬件操作类

**stride=4 导致 LoadFault** (learned.md L122)

NS16550 寄存器 stride 必须为 1（寄存器仅 8 字节）。传 4 会触发 `LoadFault @ UART_BASE + 4*offset`。`Uart16550<MmioBackend>::new_mmio(addr, 1)` 为唯一正确构造。

**ISR 禁止锁与阻塞操作** (项目规则 §ISR 极简原则)

ISR 中获取 Mutex 或操作 ring buffer 会导致死锁（Mutex 可能被挂起的任务持有）。数据搬运必须在 copier 任务中完成。

**IER 控制是安全关键** (learned.md L80)

`IER::THR_EMPTY` 未使能时 TX 中断不会触发，copier 无法知道 TX FIFO 有空位。Console 默认禁用此中断，导致 Direction A 的 TX 路径 busy-loop。

### 4.3 流程类

**未验证硬件状态不可集成** (ADR-019)

Direction A 在 M3 替换前未验证 UART IIR/MCR/LSR 寄存器状态，假设 Console 初始化的 UART 配置正常。实际状态不兼容 AsyncUart。

**ISR 验证需独立于功能实现** (ADR-023)

Direction B 通过独立的 ISR handler 测试验证了 MMIO 权限约束，避免了在完整功能实现后才发现的昂贵回滚。**原则**：关键假设必须在写功能代码前通过最小化测试验证。

**critical-section 是异步安全基础** (learned.md)

ISR 中使用 `AtomicWaker` 的 `wake()` 必须在 `critical_section` 内调用，防止与 copier 任务中的 `register()` 竞争。`embassy_sync::AtomicWaker` 的 API 设计正确保证了这一点。

### 4.4 技术验证清单

| 验证项 | 方法 | 结果 | 参考 |
|--------|------|------|------|
| ISR handler 注册 | `axhal::register_irq_hook(uart_isr_handler)` | ✅ | entry.rs |
| ISR handler 执行 | 内核日志输出 | ✅ | learned.md L116 |
| ISR 访问 UART 寄存器 | `uart.isr()` 读 ISR | ❌ LoadFault | learned.md L116 |
| 内核访问 UART 寄存器 | `uart.init()` 配置寄存器 | ❌ StoreFault | learned.md L113 |
| phys_to_virt 转换 | 虚拟地址访问 | ❌ 权限限制 | learned.md L114 |

---

## 附录：文档索引

| 文档 | 位置 | 内容 |
|------|------|------|
| 架构决策 ADR-019 ~ ADR-031 | `openspec/specs/architecture/spec.md` | 全部架构决策 |
| 踩坑档案 L78–L140 | `openspec/specs/learned/spec.md` | ISR 失败、MMIO 权限、stride、跨层状态 |
| Console UART 机制研究 | `docs/analysis/console-uart-mechanism.md` | Console TX 阻塞、RX 中断、数据竞争分析 |
| Console 剔除范围分析 | `docs/analysis/console-removal-scope-analysis.md` | 软件路径架构、剔除清单 |
| 优化记录 Q5/Q7 | `openspec/specs/optimization/spec.md` | 性能优化详情 |
