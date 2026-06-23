## Context

M2 是 Q15 增量重融的正确性修复：确保 `flush()` / `tcdrain` 在硬件发送完成后才返回。当前 `flush()` 直接返回 `Ok(())`（`device_ops.rs:129`），StarryOS tcdrain（`ctl.rs:43`）绕开 driver 直接读 UART LSR 寄存器。两者都缺少对 TX copier 内部状态（已从 ring pop 但尚未提交到 UART 的 staged bytes）的可见性。

TX 数据流路径：
```
write() → TX ring buffer → tx_copier_loop.pop_batch() → write_buf[cursor..]
→ send_bytes() → UART FIFO (16B) → UART shift register → wire
```

`flush()` 必须确认所有四阶段都已完成：ring queue 为空、copier 不活跃、staged bytes 为 0、UART TEMT 位为 true。

## Goals / Non-Goals

**Goals:**
- 在 `AsyncUartDriver` 中跟踪 `tx_copier_active` 和 `tx_staged_bytes` 状态
- 提供 `TxCompletion` 快照结构体供 flush/tcdrain 查询
- `flush()` 等待四条件全部满足后才返回
- `UartPort` trait 新增 `transmitter_empty()` 方法查询 TEMT
- StarryOS tcdrain 改用 driver completion API（替代当前直接 MMIO 访问）

**Non-Goals:**
- 不改 `TtyWrite::write` 返回值（M3）
- 不移 IER 所有权（M4）
- 不改变 M1 fast retry 逻辑
- 不添加超时/重试机制（TEMT 依赖 ISR DRAIN_WAKER）
- 不改变 ring buffer 内部实现

## Decisions

### D1: `TxCompletion` 为不可变快照，不保证原子性

**选择**：`fn tx_completion(&self) -> TxCompletion` 返回按字段逐个 Relaxed 读取的快照。四个字段独立读取，不保证跨字段原子性。

**理由**：flush 是 polling 语义 — 多次调用直到四个条件全部满足。单次快照的字段间不一致性不影响最终正确性（false positive 宁晚勿早，最终轮询会收敛）。

**替代方案**：用单一 AtomicU64 打包所有状态 → 拒绝，过于复杂且字段语义不匹配。

### D2: `tx_copier_active` 在 poll_fn 入口设置，Pending 返回前清除

**选择**：`tx_copier_active.store(true)` 在 poll_fn 闭包开始时，`.store(false)` 在所有 `return Poll::Pending` 路径前。正常 Ready 返回不清除（因为 outer loop 会立即重入 poll_fn，重新设 true）。

**理由**：flush 调用者和 TX copier 运行在不同 task 上。flush 只需要知道"copier 当前是否在处理数据"，不需要区分"刚完成旧批次" vs "正在开始新批次"。

### D3: `tx_staged_bytes` 在 pop_batch 后递增，在 send_bytes >0 后递减

**选择**：`pop_batch()` 返回 N 字节后 `tx_staged_bytes.fetch_add(N)`；每次 `send_bytes` 返回 S > 0 后 `tx_staged_bytes.fetch_sub(S)`。`pop_batch` 覆盖旧数据时不递减旧 staging（pending 被覆盖时一起走了）。

**理由**：staged_bytes 表示"已经从 ring 取走但还没确认发送的字节"。retry 内部多次 send_bytes 每次成功都递减，retry 失败的尝试不减。

### D4: `UartPort::transmitter_empty()` 返回 `bool`

**选择**：新增 `fn transmitter_empty(&self) -> bool` 返回 `LSR::TRANSMITTER_EMPTY` 位。不暴露完整 LSR 寄存器。

**理由**：M2 只需要 TEMT 位。暴露完整 LSR 会增加 trait 耦合。M4 如果需要更多 LSR 位，可以再扩展。实现层（ArceOsUartPort）通过 `self.uart.lock().lsr()` 读取。

### D5: flush() 使用 poll_fn + AtomicWaker 模式

**选择**：`flush()` 返回 `async fn`，内部用 `poll_fn` 检查 `tx_completion()` 四个条件。全部满足返回 Ready，否则注册 DRAIN_WAKER + recheck + Pending。不引入独立超时或 yield loop。

**理由**：DRAIN_WAKER 已存在于 ISR 中（THRE 中断且 LSR::TRANSMITTER_EMPTY 时 wake）。flush 复用现有 ISR 驱动机制，不增加 CPU 浪费。poll_fn 模式保证"先注册 waker 再 recheck"的防丢唤醒顺序。

### D6: StarryOS tcdrain 改为高层 API 调用

**选择**：`ctl.rs` 的 `TCSBRK` 调用 `driver().tx_completion()` 检查状态，不再直接访问 `uart_instance().lock().lsr()`。

**理由**：消除架构分层违规（syscall 层直接读 UART 寄存器），同时通过 completion 快照覆盖了 ring empty + copier active + staged bytes 三个之前缺失的检查维度。

## Risks / Trade-offs

| Risk | Mitigation |
|------|------------|
| staged_bytes 计数溢出或下溢 | usize 在 64-bit 下不可能溢出；原子操作保证无下溢竞态 |
| tx_copier_active 在 fast retry 期间保持 true → flush 永远等待 | 正确：fast retry 内 send_bytes>0 时 staged_bytes 递减；即使 active 始终 true，staged_bytes 最终归零 |
| DRAIN_WAKER 丢唤醒 | flush 使用标准 register-recheck-Pending 模式，与 M1 的 D3 同序 |
| TEMT 查询在 copier 持锁时阻塞 | `transmitter_empty()` 是短暂 MMIO 读，不会显著阻塞；flush 侧只需短暂获取锁 |
