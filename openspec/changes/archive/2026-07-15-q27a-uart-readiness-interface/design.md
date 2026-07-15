## Context

`uart_16550` async stack 当前已经具备：

- `RingBufRx::push()` 在 RX copier 推入数据后调用 `poll.wake()`。
- `RingBufRx::register_waker()` 注册 data-available waiter。
- `RingBufTx::pop()` / `pop_batch()` 在 TX copier 消费 ring、释放空间后调用 `poll.wake()`。
- `RingBufTx::register_waker()` 注册 space-available waiter。
- `AsyncUartWriter::flush()` 已使用 register -> recheck 模式等待 drain。

缺口是：外层 OS 目前没有稳定的 crate API 判断 RX/TX ring readiness，也没有 reader/writer facade 注册 readable/writable waker。Q27a 只补这个薄接口。

## Goals / Non-Goals

**Goals:**

- 为 RX/TX ring 暴露只读 readiness hint。
- 为 `AsyncUartReader` / `AsyncUartWriter` 暴露 readable/writable waker 注册。
- 保持 `uart_16550` crate 与 StarryOS OS 语义解耦。
- 为 Q27 的 `poll_io(... OUT ...)` backpressure 提供最小依赖。

**Non-Goals:**

- 不实现阻塞写循环，不修改 StarryOS TTY。
- 不改变 write/read 返回值契约。
- 不修复 writer 多 producer 安全边界；该问题属于 Q28。
- 不引入新的 runtime、wait queue 或 OS adapter trait。

## Decisions

### D1: readiness 使用 ring snapshot，而不是 completion 状态

**选择**:

- RX readable = `RingBufRx::occupied_len() > 0`
- TX writable = `RingBufTx::vacant_len() > 0`

**理由**:

- Readiness 只表示下一次 read/write 可能取得进展，不表示 drain/completion。
- TX completion 已由 `tx_completion()` / `flush()` 表达，不能和 writable 混用。
- 后续 Q27 的 `poll(OUT)` 应绑定“可接收新数据”，不是“旧数据已物理发送完成”。

### D2: waker 注册复用现有 `poll: W`

**选择**:

- `AsyncUartReader::register_readable_waker()` 调用 `driver.rx.register_waker()`。
- `AsyncUartWriter::register_writable_waker()` 调用 `driver.tx.register_waker()`。

**理由**:

- RX push 已 wake reader waiters；TX pop/pop_batch 已 wake writer waiters。
- 新增等待队列会重复状态源，增加 lost wakeup 和 wake storm 风险。
- `OsWakerSet` 已是 crate 与 OS adapter 的边界。

### D3: 文档要求 register 后 recheck

**选择**:

接口文档必须说明：

```text
检查 readiness
若不 ready，注册 waker
重新检查 readiness
仍不 ready 才等待
```

**理由**:

- readiness 查询与注册之间存在正常竞态。
- crate 只提供 primitive，OS 层决定如何映射到 poll/select/epoll 或 blocking fd。
- 允许 spurious wakeup；不允许 API 文档暗示 hint 是 reservation。

### D4: 不扩展 `OsWakerSet`

**选择**:

不为 Q27a 修改 `OsWakerSet` trait。

**理由**:

- 当前 `register()` / `wake()` 已满足 readable/writable waker 注册。
- 修改 trait 会扩大所有 OS adapter 影响面，而 Q27a 不需要。
- 若未来需要 event counter 或多类 waiter，应在有 lost-wakeup 证据后单独设计。

## Implementation Plan

1. 在 `RingBufRx` 增加 `occupied_len()` / `has_data()`。
2. 在 `RingBufTx` 增加 `vacant_len()` / `has_space()`。
3. 在 `AsyncUartReader` 增加 `can_read()` / `register_readable_waker()`。
4. 在 `AsyncUartWriter` 增加 `can_write()` / `register_writable_waker()`。
5. 补充 rustdoc，明确 readiness hint 与 register-recheck 协议。
6. 增加或更新 crate 级测试；若测试环境有既有阻塞，记录并至少通过 check。

## Risks / Trade-offs

- **瞬时长度快照可能过期**：这是 readiness API 的正常语义，文档用 hint 限定，不提供 reservation。
- **`UnsafeCell<Reader/Writer>` 查询方法必须只读**：实现应使用 ring buffer 已有 read-only query，不调用 `pop_done` 或修改 head/tail。
- **新增 public API 后续需要维护**：接口保持薄而通用，只暴露 ring readiness 和 waker registration。
- **测试可能受既有 dev-dependency 阻塞**：若 `cargo test` 不能运行，Phase 3 必须贴出具体阻塞并保留 `cargo check` 证据。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1: RX ring readable hint | 1.1, 4.1 | 100% | None | Covered |
| R2: TX ring writable hint | 1.2, 4.1 | 100% | None | Covered |
| R3: Reader/Writer waker facade | 2.1, 2.2, 4.2 | 100% | None | Covered |
| R4: Readiness hint docs + register-recheck | 3.1, 3.2 | 100% | None | Covered |
| R5: Crate boundary excludes OS semantics | 3.3, 4.3 | 100% | None | Covered |
| R6: Verification before completion | 4.1, 4.2, 4.3, 4.4 | 100% | Existing test blockers may be recorded, not hidden | Covered |

Gate 2 result: no missing requirement and no unapproved simplification. Phase 3 implementation remains blocked until the user explicitly approves entering `openspec-act`.
