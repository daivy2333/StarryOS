# SNAPSHOT.md - 项目快照（汇总分支）

> Generated at 2026-05-24
> Last updated: 2026-05-29
> 汇总分支：整合 feat/uart-async 和 feat/uart-async-dev2 两个方向的所有经验

---

## 当前状态

**分支**: feat/uart-async-consolidated（汇总分支）
**目标**: 整合两个探索方向的全部经验，形成统一知识库
**阶段**: 知识整合完成，等待架构层面决策

### 两个探索方向总结

| 方向 | 分支 | 策略 | 结果 | 关键发现 |
|------|------|------|------|----------|
| **A: 渐进式集成** | feat/uart-async (m0→m1→m2→m3→dev) | 复用 Console，逐步替换 | M0-M2 ✅, M3 ❌ | IRQ 风暴 + TX busy-loop，Console UART 状态不兼容 |
| **B: 完全剔除 Console** | feat/uart-async-dev2 | 从零开始，uart_16550 独立初始化 | P0-P1 ✅, P2 ❌ | MMIO 权限问题，ISR 也无法访问 UART 寄存器 |

### 关键阻塞

**MMIO 权限问题**（方向 B 发现）：
- axplat 在 boot 阶段映射 UART MMIO，权限被限制
- 内核上下文和 ISR 上下文都无法访问 UART 寄存器
- 已验证：内核访问 → StoreFault/LoadFault，ISR 访问 → LoadFault
- **结论**：不彻底更改底层支持（axplat）就无法使用异步串口

### 可行方案（待决策）

| 方案 | 描述 | 优点 | 缺点 |
|------|------|------|------|
| **A: Polling TX** | RX 中断驱动 + TX polling | 简单可行，无需修改外部 crate | TX 性能受限 |
| **B: 修改 axplat** | Boot 阶段修改 UART MMIO 映射权限 | 完整异步 TX | 需修改外部 crate，复杂度高 |
| **C: 回退 Console** | 完全依赖 Console（渐进式方案） | Console RX 已中断驱动 | 放弃 AsyncUart 独占目标 |

---

## 项目结构

```
StarryOS/
├── kernel/src/
│   ├── config/           # 内核配置
│   ├── drivers/          # 设备驱动（方向 B 新增，已回滚）
│   │   └── serial/       # 异步串口驱动（方向 A，已回滚）
│   ├── entry.rs          # 内核入口
│   ├── file/             # 文件系统核心
│   │   ├── pipe.rs       # 异步管道（参考实现）
│   │   └── event.rs      # EventFd（参考实现）
│   ├── lib.rs            # 模块注册
│   ├── mm/               # 内存管理
│   ├── pseudofs/         # 伪文件系统
│   │   └── dev/          # /dev 设备注册
│   │       └── tty/      # TTY/Console/ldisc
│   ├── syscall/          # 系统调用
│   └── task/             # 任务管理
├── docs/analysis/        # 设计分析文档
├── .claude/docs/         # 开发文档体系（本文件所在）
│   ├── SNAPSHOT.md       # 本文件
│   ├── architecture.md   # 架构决策记录（ADR-001~023）
│   ├── tasks.md          # 任务追踪（M0~M6 + P0~P6）
│   ├── learned.md        # 学习记忆（116+ 条目）
│   ├── references.md     # 外部参考
│   ├── optimization.md   # 优化记录
│   ├── rules.md          # 编码规范
│   ├── archive.md        # 归档内容
│   └── superpowers/      # 设计文档和实现计划
│       ├── specs/        # Spec 文档
│       └── plans/        # Plan 文档
└── CLAUDE.md             # 项目约束规则
```

---

## 技术栈

| 类别 | 技术 | 版本 | 备注 |
|------|------|------|------|
| 语言 | Rust | nightly-2026-02-25 | |
| 目标 | RISC-V 64-bit | qemu-riscv64 | |
| 异步 | axtask::future | 0.3.0-preview.2 | 项目内部 |
| 异步同步 | embassy-sync | v0.6.2 | AtomicWaker |
| 轮询 | axpoll | 0.1.2 | PollSet + Pollable |
| 硬件 | NS16550 UART | QEMU virt | |
| UART 驱动 | uart_16550（本地 v0.6.0） | path 依赖 | |
| 缓冲 | ringbuf | 0.4.8 | HeapRb |
| 构建 | Make + Cargo | | |
| 交叉编译 | riscv64-linux-musl | /opt/musl/riscv64-linux-musl-cross | |
| rootfs | rootfs-riscv64.img | 1GB | |

---

## 文档体系索引

| 文档 | 内容 | 条目数 |
|------|------|--------|
| architecture.md | ADR-001~023，两个方向的全部决策历史 | 23 |
| tasks.md | M0~M6（方向 A）+ P0~P6（方向 B）任务追踪 | ~30 |
| learned.md | API 路径、文件速查、踩坑档案、技巧模式 | 116+ |
| references.md | 依赖文档、规范、设计文档索引 | 48+ |
| optimization.md | 性能洞察、优化方向、基准目标 | 23 |
| rules.md | Karpathy Guidelines + 十大铁律 + Workflow | 唯一事实来源 |
| archive.md | 已归档的过时内容 | ~15 |

---

## 关键代码路径速查

| 模块 | 路径 | 用途 |
|------|------|------|
| Pipe 异步参考 | kernel/src/file/pipe.rs | poll_io + register_irq_waker 模式 |
| EventFd 参考 | kernel/src/file/event.rs | 轻量异步通知 |
| DeviceOps | kernel/src/pseudofs/device.rs | 设备注册 trait |
| UART 硬件 | axhal/src/platform/riscv64_qemu_virt/uart.rs | MMIO 寄存器 |
| PLIC 中断 | axhal/src/platform/riscv64_qemu_virt/mod.rs | 中断号映射 |
| Console 驱动 | kernel/src/pseudofs/dev/tty/ntty.rs | Console struct |
| tty-reader | kernel/src/pseudofs/dev/tty/terminal/ldisc.rs | RX copier 参考 |
