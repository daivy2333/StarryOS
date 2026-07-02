# Spec Delta: optimization — ARC-202607021535

> 归档日期: 2026-07-02
> 触发: 用户调用 `openspec-archivist` skill，要求"把 spec 里面几个完成的归一下档"
> 总计: 5 段 / ~179 行从 `openspec/specs/optimization/spec.md` 移至本 carrier spec
> 全文保留于下方"完整保留（Archive 区）"——本 delta 负责登记 REMOVED 关系

## REMOVED Requirements

### Requirement: Q5 内核态性能优化 — 已完成

**原 Requirement**: Q5 内核态性能优化（Q5 阶段已落地，禁止回退）
**移除原因**: Q5 状态已在 `tasks.md` Milestone 表 + `SNAPSHOT.md` §关键发现 + `learned/spec.md` L450 持久化
**移除日期**: 2026-07-02
**归档位置**: 本 carrier spec "Archive 区" #Q5 段（全文保留）
**恢复方式**: 用户说"恢复 §optimization #Q5" → Edit 复制回原位置

#### Scenario: Q5 详细实现回查需求

- **WHEN** 开发者需要回顾 Q5 阶段（IER 缓存 / ISR 合并 / 批量 I/O / 锁优化）的具体优化项编号与效果
- **THEN** MUST 查阅本 carrier spec 的 "Archive 区 #Q5 段"，原 17 行内容完整保留
- **AND** 引用必须标注 `openspec/archive/2026-07-02-arc-202607021535/specs/optimization/spec.md`

### Requirement: Q7 用户态性能修复 — 已完成

**原 Requirement**: Q7 用户态性能修复（O42 yield storm / O43 FIONBIO / O44 benchmark）
**移除原因**: Q7 状态已在 `tasks.md` Milestone 表 + `SNAPSHOT.md` §关键发现 + `learned/spec.md` L134-L145 持久化
**移除日期**: 2026-07-02
**归档位置**: 本 carrier spec "Archive 区" #Q7 段（全文保留）

#### Scenario: Q7 详细实现回查需求

- **WHEN** 开发者需要回顾 Q7 三项优化（O42 Manual→External / O43 FIONBIO 三入口 / O44 benchmark 修正）的实施细节
- **THEN** MUST 查阅本 carrier spec 的 "Archive 区 #Q7 段"，原 33 行内容完整保留

### Requirement: Q8 驱动引擎打磨 — 已完成

**原 Requirement**: Q8 驱动引擎打磨（Wave 1 正确性修复 + Wave 2 热路径优化 + Wave 3 O46 AtomicWaker 推广）
**移除原因**: Q8 状态已在 `tasks.md` Milestone 表 + `openspec/changes/archive/2026-06-11-q8-driver-polish/` 持久化
**移除日期**: 2026-07-02
**归档位置**: 本 carrier spec "Archive 区" #Q8 段（全文保留）

#### Scenario: Q8 详细实现回查需求

- **WHEN** 开发者需要回顾 Q8 阶段（Q8.1 NAPI 退出 / Q8.2 ISR 去锁化 / Q8.3 IER 规范化 / Q8.4-Q8.5 热路径 / Q8.6-Q8.9 O46 推广）的 Wave 划分
- **THEN** MUST 查阅本 carrier spec 的 "Archive 区 #Q8 段"，原 45 行内容完整保留

### Requirement: Q12 Embassy 调研驱动的近期优化 — 已完成（路径 A）

**原 Requirement**: Q12 Embassy 调研驱动的近期优化（O51 atomic_ring_buffer / O52 embedded_io_async / O53 TC tcdrain）
**移除原因**: Q12 状态已在 `tasks.md` Milestone 表 + `openspec/changes/archive/2026-06-15-q12-embassy-path-a/` 持久化，O51/O52/O53 已 tombstone
**移除日期**: 2026-07-02
**归档位置**: 本 carrier spec "Archive 区" #Q12 段（全文保留）

#### Scenario: Q12 详细实现回查需求

- **WHEN** 开发者需要回顾 Q12 阶段（O51 lock-free SPSC / O52 标准化 trait / O53 TC tcdrain）的性能收益
- **THEN** MUST 查阅本 carrier spec 的 "Archive 区 #Q12 段"，原 17 行内容完整保留

### Requirement: Q15 M0~M4 增量重融合 + Manual QA — 已完成（2026-06-25）

**原 Requirement**: Q15 阶段（pre-M4 基线出发 + 5 个 milestone 增量 + Manual QA 验证）
**移除原因**: Q15 状态已在 `SNAPSHOT.md` §Q15 增量重融合 + `architecture/spec.md` ADR-039 完整记录 + `learned/spec.md` L201-L211 教训持久化
**移除日期**: 2026-07-02
**归档位置**: 本 carrier spec "Archive 区" #Q15 段（全文保留）

#### Scenario: Q15 详细实现回查需求

- **WHEN** 开发者需要回顾 Q15 阶段（O62-M0~M4 增量融合策略 + Manual QA 性能基线 + Q16~Q23 触发条件）
- **THEN** MUST 查阅本 carrier spec 的 "Archive 区 #Q15 段"，原 67 行内容完整保留

---

## 完整保留（Archive 区）

### Q5 段 (Archive, optimization #Q5 2026-07-02)

**原行号**: L9-25
**归档原因**: Q5 内核态性能优化已落地，状态已在 tasks.md Milestone 表 ✅

```markdown
### Requirement: Q5 内核态性能优化 — 已完成

Q5 阶段（中断驱动 + NAPI 批量 I/O）所有优化 MUST 视为已落地且禁止回退；新增优化 MUST 在 Q5 基础上叠加，禁止重复造轮子。

| 编号 | 内容 | 效果 |
|------|------|------|
| **O2/O34** | NAPI 中断合并 + TX interleave 修复 | 高吞吐减少 90%+ IRQ |
| **O4/O35** | FCR 阈值日志 | FIFO 状态监控 |
| **O7** | uart_16550 批量读写 API | 减少函数调用开销 |
| **O25-O33** | 批量 I/O / IER 缓存 / ISR 合并 / 锁优化 | 热路径全面优化 |
| **O24** | stride=4 修复 | LoadFault 根因修复 |

#### Scenario: 优化热路径性能

- **WHEN** 开发者要提升 ISR / copier 性能
- **THEN** MUST 在 Q5 优化基础上叠加（IER 缓存、批量 I/O、waker skip、锁合并），禁止从零重写
```

### Q7 段 (Archive, optimization #Q7 2026-07-02)

**原行号**: L26-58
**归档原因**: Q7 用户态性能修复已完成 (2026-06-01)，状态已在 tasks.md + SNAPSHOT.md

```markdown
### Requirement: Q7 用户态性能修复 — 已完成

Q7 优化 MUST 视为已落地；任何回退 MUST 附带 commit 证明性能回退可接受。

**Q7 用户态性能修复（2026-06-01 已完成）**：

| 编号 | 内容 | 优先级 | 影响 | 状态 |
|------|------|--------|------|------|
| **O42** | 修复 yield storm | 🔴 高 | 消除无数据时高频 yield-re-schedule | ✅ Manual→External |
| **O43** | 传播 FIONBIO nonblocking | 🔴 高 | ioctl(FIONBIO) 对 TTY 读生效 | ✅ Tty+ldisc+ctl |
| **O44** | 修正 benchmark | 🟡 中 | TX /dev/console + tcdrain + FIONBIO | ✅ 新建 benchmark.c |

**O42 实施细节**：

- `ntty_async.rs`：创建 `Arc<PollSet>`，传入 `ProcessMode::External(Box::new(move |waker| poll_rx.register(waker)))`
- `ldisc.rs`：External 模式自动创建 tty-reader 任务，`register_rx_waker` 使用 PollSet（不再 `wake_by_ref`）
- **代价**：多一个内核任务（与旧 Console 相同）

**O43 实施细节**：

- `tty/mod.rs`：Tty struct 加字段 `nonblocking: AtomicBool`，`read_at()` 内用 `self.nonblocking.load(Acquire)`
- `tty/mod.rs`：DeviceOps ioctl 处理 FIONBIO → set nonblocking
- `ldisc.rs`：`read()` 方法接受 `nonblocking: bool` 参数 → `block_on(poll_io(...))` 用该参数

#### Scenario: 修改 ntty_async / ldisc 模式

- **WHEN** 开发者要改 `ProcessMode` 或 tty-reader 行为
- **THEN** MUST 保持 O42 的 External 模式（避免 yield storm），禁止回退到 Manual + `wake_by_ref`

#### Scenario: 修 FIONBIO 相关逻辑

- **WHEN** 开发者要改 nonblocking 状态传播
- **THEN** MUST 同时检查 `tty/mod.rs` / `ldisc.rs` / `syscall/fs/ctl.rs` 三个入口（O43 + L140 教训）
```

### Q8 段 (Archive, optimization #Q8 2026-07-02)

**原行号**: L60-104
**归档原因**: Q8 驱动引擎打磨已完成 (2026-06-11)，子任务已归档至 `openspec/changes/archive/2026-06-11-q8-driver-polish/`

```markdown
### Requirement: Q8 驱动引擎打磨 — 已完成

Q8 阶段（2026-06-11）MUST 视为已落地；任何回退 MUST 附带 commit 证明无正确性/性能退化。

**Wave 1 — 正确性修复**：

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **Q8.1** | NAPI 退出修复 | 🔴 BugFix | `async_driver.rs` — `total==0` 时重置 `consecutive=0` + `enable_rx_intr()`，消除 NAPI 永不退出导致 CPU 空转问题 |
| **Q8.2** | ISR 去锁化 | 🔴 BugFix | `isr.rs` — 消除 `SpinNoIrq` 锁，实现无锁 ISR 路径，符合 ISR 极简原则 |
| **Q8.3** | IER 写路径规范化 | 🔴 BugFix | `uart_init.rs` — 用 `uart_16550::set_ier()` 替代裸 `write_volatile`，消除规则违规；`uart_16550` crate 新增 `set_ier()` 公共方法 |

**Wave 2 — 热路径优化**：

| 编号 | 内容 | 优先级 | 说明 |
|------|------|--------|------|
| **Q8.4** | copier waker 去重简化 | 🟡 优化 | `async_driver.rs` — 仅 `will_wake` 不同时才 `clone()+register`，减少 ~20-40ns/poll |
| **Q8.5** | DRAIN_WAKER 条件唤醒 | 🟡 优化 | `isr.rs` — 仅在 tcdrain 活跃时 `DRAIN_WAKER.wake()`，减少无意义原子操作 |

**Wave 3 — O46 AtomicWaker 推广**：

| 编号 | 内容 | 说明 |
|------|------|------|
| **Q8.6** | signalfd PollSet→AtomicWaker | `signalfd.rs` — 1 PollSet → 1 AtomicWaker |
| **Q8.7** | event PollSet→AtomicWaker | `event.rs` — 2 PollSet → 2 AtomicWaker |
| **Q8.8** | pipe PollSet→AtomicWaker | `pipe.rs` — 3 PollSet → 3 AtomicWaker（交叉唤醒 read→TX / write→RX / close→close） |
| **Q8.9** | pidfd PollSet→AtomicWaker | `pidfd.rs` + `task/mod.rs` + `task/ops.rs` — Arc 共享重构，进程退出时 AtomicWaker::wake() |

**总收益**：唤醒延迟 ~200ns→~50ns（8 个唤醒点），ISR 延迟降低 ~200ns（去锁化），NAPI 空闲 CPU 归零，消除 2 处规则违规（IER 裸写 + ISR 锁）。

#### Scenario: NAPI 模式下数据流停止

- **WHEN** RX copier 在 NAPI 模式（consecutive ≥ NAPI_THRESHOLD）且 `receive_bytes()` 返回 0
- **THEN** consecutive 重置为 0，enable_rx_intr() 被调用，下次 ISR 正常触发

#### Scenario: ISR 无锁执行

- **WHEN** UART 产生中断
- **THEN** ISR 无锁读取 ISR 寄存器 → 禁用对应中断 → AtomicWaker::wake() → 返回（全流程 ~1.5 µs）

#### Scenario: IER 通过安全 API 写入

- **WHEN** copier 调用 enable/disable 中断函数
- **THEN** IER 通过 `uart_16550::Uart16550::set_ier()` 写入，CACHED_IER 与硬件 IER 一致
```

### Q12 段 (Archive, optimization #Q12 2026-07-02)

**原行号**: L296-312
**归档原因**: Q12 Embassy 路径 A 已完成 (2026-06-11)，子任务已归档至 `openspec/changes/archive/2026-06-15-q12-embassy-path-a/`，O51/O52/O53 已 tombstone

```markdown
### Requirement: Q12 Embassy 调研驱动的近期优化 — 已完成（路径 A）

Q12 阶段（2026-06-11）MUST 视为已落地；任何回退 MUST 附带 commit 证明无正确性/性能退化。归档：2026-06-15（`openspec/changes/archive/2026-06-15-q12-embassy-path-a/`）。

| 编号 | 内容 | 状态 | 关键收益 |
|------|------|------|----------|
| **O51** | `atomic_ring_buffer` 替换 `HeapRb + Mutex` | ✅ | overhead 53.9→37.1 µs（↓31%） |
| **O52** | `embedded_io_async` trait 实现 | ✅ | 标准化接口，生态互通 |
| **O53** | TC 硬件寄存器 tcdrain | ✅ | 删除 TCDRAIN_ACTIVE 软件状态 |

**总收益**：software overhead ↓31%，1B avg latency 118→123.9 µs

#### Scenario: 维护 Q12 Embassy 路径 A 优化成果

- **WHEN** 考虑对 Q12 阶段已落地的 O51/O52/O53 优化做修改或回退
- **THEN** MUST 在新 OpenSpec 变更中说明理由并附 commit 证明
- **AND** MUST 保持 O51 的 lock-free SPSC 收益、O52 的 trait 标准化、O53 的 TC tcdrain 行为
```

### Q15 段 (Archive, optimization #Q15 2026-07-02)

**原行号**: L628-694
**归档原因**: Q15 M0~M4 增量重融合 + Manual QA 已完成 (2026-06-25)，状态已在 SNAPSHOT.md §Q15 增量重融合，architecture A039 记录完整设计决策

```markdown
### Requirement: Q15 M0~M4 增量重融合 + Manual QA — 已完成（2026-06-25）

Q15 阶段（2026-06-21 开启，2026-06-25 完成）从 pre-M4 基线出发，将原 `feat/uart-16550-async-temp` 分支的 M4+ 正确性修复按最小可验证单元重新 apply，每步 QEMU benchmark 验证无退化。Q15 完成后 MUST 视为已落地；任何回退 MUST 附带 commit 证明无正确性/性能退化。

**O62 — Q15 增量重融合 5 个 milestone**：

| 编号 | 内容 | 关键收益 | 状态 |
|------|------|----------|------|
| **O62-M0** | 见证层（RawMutex / per-port ISR / FIFO 边界矩阵 + telemetry） | 隔离验证基线，量化每次融合的退化 | ✅ (2026-06-23) |
| **O62-M1** | 有界 TX fast retry（`TX_FAST_RETRY_LIMIT=32`） | 消除 16B FIFO refill 的 10ms tick 台阶 | ✅ (2026-06-23) |
| **O62-M2** | TX completion 三阶段 drain（flush / tcdrain）| ring/copier/staged 检查，TEMT corner-case 修复 | ✅ (2026-06-23) |
| **O62-M4** | IER 单 owner（`UartPort::update_ier()` 统一管理）| 删除 CACHED_IER / write_ier / enable_*，uart_16550 真正独立 | ✅ (2026-06-23) |
| **O62-M3** | TtyWrite 短写契约（`write(&[u8]) -> usize`）| VFS/sys_write 层看到真实接受字节数，消除 silent data loss | ✅ (2026-06-23) |

**关键约束**（增量融合过程中已严格执行）：

- 不修改任何外部 crate（`axtask` / `axpoll` / `embassy-sync`）
- 不提高调度 tick 频率
- ISR 极简原则不变
- 每步必须 `cargo check` 通过 + QEMU benchmark 验证无退化

**Manual QA 验证结果（2026-06-25）**：

| 指标 | Q13.1 基线 | Q15 后（无 LTO per ADR-034）| 趋势 | 备注 |
|------|----------|----------------------------|------|------|
| 内核态 Ring Buffer TX | 385 MB/s（LTO off）/ 652 MB/s（LTO on）| **456 MB/s** | ↑（较 LTO off）| Q15-M0 telemetry 开销抵消 + lock-free 改进 |
| 内核态 Ring Buffer RX | 898 MB/s（LTO on）| **1,148 MB/s** | **↑27.9%** | Q15-M0/M4 lock-free 改进显著 |
| 用户态 1B e2e 延迟（avg）| 129.5 µs | **134 µs** | +3.5% | 调度瓶颈未变，noise 范围内 |
| 用户态 1B e2e 延迟（P50）| 125.5 µs | **118.5 µs** | -5.6% | 改善 |
| 用户态 64B TX 吞吐 | 184 KB/s（M4 单测）| **170 KB/s** | -7.6% | QEMU 噪声范围内，无 TX backpressure 退化 |
| 非阻塞三入口 | ✅ | **✅** | 不变 | FIONBIO 行为正确 |

**结论**：

- ✅ **无 64B write+tcdrain 退化**（TX backpressure 风险解除）
- ✅ **用户态延迟与 Q13.1 基线持平**（e2e 瓶颈在调度，不在驱动层）
- ✅ **内核态 Ring Buffer RX 显著提升**（Q15-M0/M4 lock-free 改进）
- ✅ **IER 单 owner 达成**（uart_16550 crate 真正独立可复用）
- ✅ **TtyWrite 短写契约落地**（VFS 契约正确，消除 silent data loss）

**Q16~Q23 触发条件**（Q15 后 roadmap）：

- Q16 文档/规格收敛 MUST 先完成，确保 tasks / SNAPSHOT / optimization / capability specs 的 roadmap 一致
- Q17 / O63 MUST 在真板前优先修复（QEMU 单 hart 掩盖 SMP 内存序问题）
- Q18 / O74-O75 MUST 先完成平台参数解耦和 early console 基础，避免继续把板级参数写入 driver init
- Q19 / O76 使用 Lichee RV Dock 演练 Android boot image + D1 polling early console；2026-06-29 已确认 U-Boot 能加载 StarryOS D1 payload，并完成 `[starry-d1] smoke complete, halting.` 真板验收
- VisionFive2 真板到位 → Q20 O66/O64/O65 观测与 trust-u-boot 验证 → O38（时钟适配）→ O39（真板 FIFO 深度验证）→ Q15 Manual QA 真板复跑
- Q21 O3/O40/O69（DMA 探索）与 O41（高速波特率）MUST 依赖 Q20 真板数据，禁止在 QEMU 上直接下结论
- **📐 物理定律**：真板 NS16550 硬件时间 86.8 µs/byte @ 115200 bps（10 bits/byte × 1/115200 s）与 QEMU 0 µs 硬件时间形成本质差异

#### Scenario: Q15 后再次合并 async-uart 优化

- **WHEN** 开发者从 `feat/uart-16550-async-temp` 或其他临时分支再次提取优化 commit
- **THEN** MUST 遵循 Q15 增量融合策略：摘取原子 commit → cargo check → QEMU benchmark → 无退化才继续
- **AND** 禁止一次性大批量 apply（避免 Q13 M4 Sync 退化的 73.9x 性能灾难复现）

#### Scenario: 评估 Q15 后新优化方向

- **WHEN** 开发者发现新的 async-uart 优化方向（如 IRQ affinity、零拷贝 RX、DMA）
- **THEN** MUST 先创建 OpenSpec 变更提案，量化预期收益与风险
- **AND** MUST 在 Q20 真板验证完成后启动硬件依赖方向（QEMU 仿真限制决定绝对吞吐无法在 QEMU 上验证）

#### Scenario: 回退 Q15 任一 milestone

- **WHEN** 开发者考虑回退 O62-M0/M1/M2/M3/M4 任一项
- **THEN** MUST 附带 commit 证明：(1) 当前存在正确性 bug 或可量化的性能退化，(2) 回退后其他 milestone 不受影响
- **AND** 禁止以"未来可重做"为由回退（Q15 已验证状态机，回归成本高于保留）
```

---

## 归档元信息

- **ARC ID**: ARC-202607021535
- **归档日期**: 2026-07-02
- **归档人**: openspec-archivist skill（用户授权 Gate 1 通过）
- **源文档影响**: `openspec/specs/optimization/spec.md` 释放 ~179 行
- **归档后位置**: `openspec/changes/archive/2026-07-02-ARC-202607021535/`
- **恢复协议**: 用户说"恢复 §optimization #Q{5|7|8|12|15}" → Edit 复制回原位置
- **排除项**: architecture A033-A051（设计基线 Keep）、learned L78-L258（踩坑档案 Keep）、references（依赖索引 Keep）
- **状态信息保留**: tasks.md Milestone 表 + SNAPSHOT.md §关键发现 + architecture ADR-039 + learned L134-L159/L201-L211