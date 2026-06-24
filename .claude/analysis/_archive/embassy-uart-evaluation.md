# Embassy UART 架构评估（摘要）

> ⚠️ **STALE [2026-06-17]** — 完整版已归档至 `_archive/embassy-uart-evaluation.md`（16K）
> **Q12 已完成（2026-06-11）**，完整决策见 `openspec/specs/optimization/spec.md` §Q12

---

## 核心结论

StarryOS 自研架构与 embassy `BufferedUart` **在宏观架构上同构**（ISR → ring buffer → waker → 用户 I/O），**无根本性缺陷需要重写**。主要差异在 ISR 职责分配（embassy 在 ISR 中直接搬运数据，StarryOS 用 copier 任务分离）。

| 维度 | Embassy `BufferedUart` | StarryOS 自研 |
|------|----------------------|--------------|
| ISR 延迟 | 取决于 FIFO 深度（~50-100 cycles/byte） | 固定 ~1.5 µs（NS16550 16 字节） |
| 任务切换 | 半满唤醒一次 | 每次中断→copier 切换 1 次 |
| Ring buffer | `atomic_ring_buffer::RingBuffer`（lock-free SPSC） | `HeapRb + Mutex` → Q12 后改 `atomic_ring_buffer` |
| 评估 | O17 不需要 BTreeMap 分发 | ✅ Q12 验证 ISR 极简正确 |

---

## Q12 路径 A 落地清单（✅ 已完成）

| 编号 | 借鉴项 | 实际收益 | spec 位置 |
|------|--------|---------|-----------|
| **O51** | `atomic_ring_buffer` 替换 `HeapRb + Mutex` | overhead 53.9→37.1 µs（↓31%） | `optimization/spec.md` §Q12 |
| **O52** | `embedded_io_async` trait 实现 | 标准化接口，生态互通 | `optimization/spec.md` §Q12 |
| **O53** | TC 硬件寄存器 tcdrain（LSR::TRANSMITTER_EMPTY） | 删除 `TCDRAIN_ACTIVE` 软件状态 | `optimization/spec.md` §Q12 + `O45` |

**总收益**：software overhead ↓31%，1B avg latency 118→123.9 µs

---

## 不应采用（OE1~OE5 反模式）

- ❌ `embassy-executor` — 与 axtask 冲突（L10）
- ❌ `embassy-stm32::BufferedUart` — 芯片特定，移植成本≈重写
- ❌ `embassy-sync` 其他同步原语（Channel/Mutex/Watch/Semaphore）— 验证为反优化
- ❌ `embassy_futures::select!` — 需 embassy executor
- ❌ `InterruptExecutor` — 直接违反 ISR 极简原则
- ❌ `RingBufferedUartRx`（DMA） — NS16550 无 DMA 控制器

---

## 当前权威文档（取代本文）

| 主题 | 当前权威文档 |
|------|-------------|
| Q12 落地记录 | `optimization/spec.md` §Q12 Embassy 调研驱动的近期优化 |
| 路径 B 远期优化 | `optimization/spec.md` §远期优化（O54 ISR 搬运 / O55 半满唤醒） |
| Embassy 选型边界 | `learned/spec.md` L81~L84（OE1~OE5 教训） |

---

**恢复条件**：如需查看完整 embassy `BufferedUart` ISR 代码对比、迁移路径 A/B/C 详细分析、embassy-stm32 源码引用，查阅 `_archive/embassy-uart-evaluation.md`
**生成日期**：2026-06-11（原始）→ 2026-06-17（摘要）
