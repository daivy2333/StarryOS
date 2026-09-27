# uart_16550 异步串口提取可行性分析（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/uart-16550-async-extraction.md`（14K）
> **Q13 已完成（2026-06-16）**，完整记录见 `architecture/spec.md` ADR-032 + `tasks.md` §Q13

---

## 核心结论

将 StarryOS 异步串口实现（Q0~Q12 共 ~618 行）提取到 `uart_16550` 子项目 crate，**完全可行**。需要推翻 D1 决策（uart_16550 保持 sync-only），让 `uart_16550` 成为**完整、可复用的异步 UART crate**。

**ADR-032**：uart_16550 成为完整异步 UART crate（**推翻 D1**）

---

## Q13 三阶段实施（✅ 全部完成）

| 阶段 | 目标 | 实际产出 | commits |
|------|------|---------|---------|
| **Phase 1** | TtyRead/TtyWrite trait 提取 | `uart_16550/src/tty.rs`（+27 行）<br>StarryOS ldisc.rs re-export | `7bee89d` + `8aac223` |
| **Phase 2** | 5 个 OS 抽象 trait + 核心异步逻辑迁移 | `os/mod.rs` + `async_/{isr,ring_buffer,driver,device_ops}.rs` | `1005b71` `9ce5fe2` `e6cf219` `4a000ae` `8dd5cba` `be87a24` |
| **Phase 3** | StarryOS 适配层 + 删除本地代码 | `os_arceos.rs` 适配层<br>删除 4 个本地文件 | `9bed0c7` `842f8f4` |

**最终架构**：
- StarryOS `kernel/src/drivers/` 仅保留：`uart_init.rs`（硬件初始化） + `ntty_async.rs`（TTY 绑定） + `os_arceos.rs`（新增适配层）
- `uart_16550/src/async_/` 提供完整异步栈（`async` feature gate）
- 9 个原子提交，`cargo check` + `cargo clippy` 0 错误/警告

---

## 5 个 OS 抽象 trait

```rust
pub trait OsRuntime { fn spawn(...); fn block_on(...); }
pub trait OsIrq     { fn register_handler(irq, handler); }
pub trait OsMmio    { fn map_mmio(phys, size); fn phys_to_virt(phys); }
pub trait OsSpinNoIrq<T> { fn new(v); fn lock(&self); }
pub trait OsWakerSet { fn register(&self, waker); fn wake(&self) -> u32; }
```

StarryOS `os_arceos.rs` 实现这 5 个 trait，桥接到 ArceOS 原语（`axtask::spawn` / `axhal::register_irq_hook` / `axmm::iomap` / `kspin::SpinNoIrq` / `axpoll::PollSet`）。

---

## 推翻 D1 决策的论证

| 原 D1 决策 | 推翻理由 |
|----------|---------|
| 异步留在 StarryOS wrapper 层 | 复用需求 + Q12 基础设施就位 + 代码量可控（~400 行） |
| uart_16550 保持 sync-only | 4 个 trait + embedded_io_async 是社区标准，可移植性高 |

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| Q13 完整记录 | `tasks.md` §Q13（含 Phase 1/2/3 全部子任务 + commits）|
| ADR-032 决策 | `architecture/spec.md` §ADR-032 |
| 新 API 路径 | `learned/spec.md` L160~L175（含 `uart_16550::os::OsRuntime` 等） |
| 4 个新 spec | `openspec/specs/{arceos-adapter,async-uart-core,async-uart-traits,inline-batch-optimize}/spec.md` |

---

**恢复条件**：如需查看 7 文件 618 行迁移表、5 OS 抽象 trait 完整定义、三阶段详细工作量评估、风险与缓解措施，查阅 `_archive/uart-16550-async-extraction.md`
**生成日期**：2026-06-15（原始）→ 2026-06-17（摘要）
