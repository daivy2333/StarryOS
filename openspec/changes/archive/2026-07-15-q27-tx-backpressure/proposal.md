## Why

Q27a 已提供 TX ring readiness hint 与 waker 注册，但 StarryOS TTY 仍无条件报告 `IoEvents::OUT`，阻塞 fd 在 ring 满时仍只能 short write。Q27 需要把这些 primitive 接入 TTY/VFS，在不重开 user ring/CQ、不处理 Q28 writer 并发契约的前提下，补齐可维护的 TX backpressure。

## What Changes

- 在 kernel TTY 层引入 OS-local writer readiness 契约，UART writer 映射 Q27a ring readiness，PTY/echo 保持当前行为。
- `Tty::poll()` 只在 TX writer 可接受数据时返回 `IoEvents::OUT`，`Tty::register()` 为 OUT 注册 writable waker。
- `Tty::write_at()` 保留快路径；ring 空间不足时，阻塞 fd 用 `poll_io` 累计到请求完成，非阻塞 fd 返回 partial 或 `WouldBlock`。
- `OPOST|ONLCR` 转换按完整源字符边界提交，禁止非阻塞 short write 在 `\r\n` 中间返回导致重试重复 `\r`。
- `AsyncUartWriter` 增加 OS-neutral `writable_len()` hint，不改变 `TtyWrite` short-write 契约，不引入 VFS/fd/`axpoll` 依赖。
- 增加小 ring/大输出、`write`/`writev`、FIONBIO/`O_NONBLOCK`、poll/select/epoll OUT、ONLCR 与 `tcdrain` 验证，并对比 Q15/Q20 性能基线。

## Non-Goals

- 不收敛 `AsyncUartWriter::Clone` 与 `RingBufTx` SPSC 契约；该项属于 Q28。
- 不引入 MPSC ring、user completion queue、`mmap` user ring 或 zero-copy。
- 不让 `uart_16550` crate 知道 StarryOS `IoEvents`、VFS、syscall 或 fd nonblocking 语义。
- 不把 echo 改为可靠阻塞输出，不扩展 PTY backpressure 行为。

## Capabilities

### New Capabilities

- `tty-tx-backpressure`: 定义 StarryOS TTY 的 TX writable readiness、阻塞/非阻塞写、ONLCR 边界与性能回归契约。

### Modified Capabilities

- `async-uart-core`: 在 Q27a readiness facade 上补充 OS-neutral writable length hint，不改变现有 I/O trait 行为。

## Impact

- `crates/uart_16550/src/async_/device_ops.rs`：新增 `AsyncUartWriter::writable_len()` 薄 facade。
- `kernel/src/pseudofs/dev/tty/mod.rs`：TTY write fast/slow path、OUT poll/register 映射。
- `kernel/src/pseudofs/dev/tty/terminal/ldisc.rs`：kernel-local writer readiness 契约；echo 仍 best-effort。
- `kernel/src/pseudofs/dev/tty/pty.rs`：仅提供保持当前语义的兼容 readiness 实现。
- 测试/见证：优先使用纯逻辑 unit tests 覆盖 partial/ONLCR 边界；QEMU 验证 syscall/poll 行为，D1 验证真实线速不退化。

## Workflow Phase 1 BDD Gap Scan

> 2026-07-15：用户选择“用默认假设补充”，并强调可维护性与不降低性能。

### Happy Path

- TX ring 有空间时，TTY OUT ready，现有 fast path 用一次 writer push 完成写入。
- 阻塞 `write`/`writev` 在 ring 满时等待 TX copier 释放空间，最终返回完整请求长度。
- poll/select/epoll 的 OUT readiness 与 TX ring 空间一致，pop wake 后 waiter 可继续写。

### Sad Path

- 非阻塞 fd 在已写前缀后 ring 满时返回 partial，完全无法取得进展时返回 `WouldBlock`。
- waker 注册与 ring 释放空间竞态必须由 check -> register -> recheck 关闭，不允许 lost wakeup 或 busy loop。
- 任何出现 10ms FIFO refill 调度台阶、`tcdrain` hang 或输出前缀丢失都阻断 Q27。

### Edge

- 空 buffer write 立即返回 0，不注册 waker。
- `OPOST|ONLCR` 下的 `\n -> \r\n` 必须以源字符为返回计数边界，非阻塞路径不提交半个映射。
- PTY 的现有 always-OUT/short-write 行为与 ldisc echo best-effort 行为保持不变。
- Q28 前仍以单 UART producer 为安全前提，Q27 不用性能锁掩盖 writer clone 问题。
