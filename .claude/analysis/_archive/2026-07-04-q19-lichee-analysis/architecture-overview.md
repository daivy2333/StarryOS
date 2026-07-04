# StarryOS 架构总览（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/architecture-overview.md`（26K）
> 内容大量已迁移至 `SNAPSHOT.md` + `openspec/project.md` + `architecture/spec.md`
> 路径引用基于 Q13 之前的 `kernel/src/drivers/async_driver.rs` 等（Q13 已迁至 `uart_16550` crate）

---

## 核心结论（1 页摘要）

StarryOS = **Linux ABI 兼容宏内核** + **ArceOS 组件化** + **Rust nightly-2026-02-25** + **RISC-V/LoongArch/AArch64 三架构**

| 维度 | 现状 |
|------|------|
| 内核模型 | 宏内核（兼容 Linux ABI） |
| 基础框架 | ArceOS unikernel（`ax*` 系列 crate） |
| 异步运行时 | `axtask::future::block_on` + `embassy_sync::AtomicWaker` |
| 异步串口 | Q13 后完全在 `uart_16550` crate（`async` feature） |
| 硬件 | NS16550 UART（QEMU virt） + IRQ 10（PLIC） |
| 仓库 | Cargo workspace，**唯一 member = `kernel/`** |
| 构建 | `make run` → `make defconfig` → `cargo build --features qemu` |

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| 项目状态 | `.claude/docs/SNAPSHOT.md` |
| 技术栈详情 | `openspec/project.md` + `SNAPSHOT.md` §技术栈 |
| 异步串口架构 | `architecture/spec.md`（ADR-001~034）|
| 关键文件路径 | `SNAPSHOT.md` §关键代码路径速查 + `learned/spec.md` L160-L175 |
| 构建/部署 | `references/spec.md` §核心 Rust 依赖与构建工具 |

---

## 关键文件指向（Q13 之后）

| 模块 | 当前路径 |
|------|---------|
| 异步串口 ISR | `uart_16550/src/async_/isr.rs` |
| 环形缓冲 | `uart_16550/src/async_/ring_buffer.rs` |
| Copier 驱动 | `uart_16550/src/async_/driver.rs` |
| 设备 ops | `uart_16550/src/async_/device_ops.rs` |
| StarryOS 适配层 | `kernel/src/drivers/os_arceos.rs` |
| 硬件初始化 | `kernel/src/drivers/uart_init.rs` |
| TTY 绑定 | `kernel/src/drivers/ntty_async.rs` |

---

**恢复条件**：如需查看完整版（含所有 §1~§4 章节、build 变量表、平台 feature 列表、启动链流程图），查阅 `_archive/architecture-overview.md`
**生成日期**：2026-06-11（原始）→ 2026-06-17（摘要）
