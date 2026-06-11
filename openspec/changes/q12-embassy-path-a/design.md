## Context

StarryOS 当前 UART ring buffer 使用 `HeapRb<u8>` + `axsync::Mutex` 保护并发访问。虽然 `HeapRb` 本身是 SPSC，但 `Mutex` 在每次 push/pop 时产生 ~100ns 开销。embassy 使用 `atomic_ring_buffer::RingBuffer`（lock-free SPSC，纯 Rust，768 行，有完整单元测试），可消除此开销。

tcdrain 当前使用 `TCDRAIN_ACTIVE: AtomicBool` 软件标志控制 `DRAIN_WAKER` 的条件唤醒。NS16550 硬件提供 `LSR::TRANSMITTER_EMPTY`（bit 6，THR + 移位寄存器全空），可直接在 ISR 中判断并唤醒，删除软件状态。

`embedded_io_async` 是 Rust 嵌入式社区标准异步 I/O trait（非 embassy 专属），为 `AsyncUartReader`/`AsyncUartWriter` 新增 impl 可标准化接口而不改核心路径。

## Goals / Non-Goals

**Goals:**
- O51: `RingBufRx`/`RingBufTx` 改用 `atomic_ring_buffer::RingBuffer`，公共 API 签名不变（`push`/`pop`/`is_empty`/`register_waker` 语义保持）
- O52: `AsyncUartReader` impl `embedded_io_async::Read`，`AsyncUartWriter` impl `embedded_io_async::Write`（纯新增 trait impl）
- O53: 在 `isr.rs` TX handler 中追加 LS TEMT 检查并直接 `DRAIN_WAKER.wake()`，删除 `TCDRAIN_ACTIVE` 及相关 `load`/`store` 代码
- 性能不低于 Q11 基线（1B avg latency ≤ 118µs）

**Non-Goals:**
- 不修改 ISR 架构（仍为极简模式，不引入 ISR 直接搬运）
- 不引入 `embassy-executor` 或任何 embassy HAL
- 不修改 `TtyRead`/`TtyWrite` trait 体系

## Decisions

| # | 决策 | 理由 | 替代方案 |
|---|------|------|----------|
| D1 | 使用 `embassy_hal_internal::atomic_ring_buffer` 而非自建 lock-free ring buffer | 已有完整实现 + 单元测试（L527-768），避免重复造轮子 | 自建（风险：bug 潜伏期长） |
| D2 | `RingBuffer` 内存在 `static` 分配（`AtomicPtr<u8>` + 外部 `&mut [u8]`）替代 `HeapRb` 堆分配 | embassy `RingBuffer` 设计为 `static` 安全，避免堆分配 | 保持 `HeapRb`（浪费 mutex 开销） |
| D3 | `embedded_io_async` trait 在 `device_ops.rs` 中作为独立 `impl` 块新增 | 零侵入：现有 `TtyRead`/`TtyWrite` 不动，纯新增 | 替换 `TtyRead`/`TtyWrite` 为 `embedded_io_async`（breaking） |
| D4 | TC tcdrain 使用 `LSR::TRANSMITTER_EMPTY`（bit 6）在 ISR 中检查 | NS16550 TEMT 表示 THR + TSR 全空（真正的 drain），与 embassy `tcie` 语义等价 | 保留 `TCDRAIN_ACTIVE`（多一个软件状态） |

## Risks / Trade-offs

- [Risk] `atomic_ring_buffer` 的 `Acquire`/`Release` 内存序错误使用可能导致数据竞争 → 严格参照 embassy 原始实现的 L236-260 Acquire/Release 模式
- [Risk] `RingBuffer` 需要 `static` 生命周期缓冲区，当前 `HeapRb::new(BUF_SIZE)` 是动态分配 → 改为模块级 `static mut BUF: [u8; 65536]` 并通过 `RingBuffer::init(&mut buf)` 初始化
- [Risk] TC tcdrain 删除 `TCDRAIN_ACTIVE` 双重检查后可能丢失唤醒 → NS16550 TEMT 由硬件保证（THR+TSR 全空时 LSR bit 6 自动置位），ISR 每次 TX 中断都检查，不会丢唤醒
