## 1. Ring readiness hint

- [x] 1.1 `RingBufRx` 增加 `occupied_len()` / `has_data()`，语义为当前 RX ring 中可读取字节数 / 是否有数据
- [x] 1.2 `RingBufTx` 增加 `vacant_len()` / `has_space()`，语义为当前 TX ring 中可接收字节数 / 是否有空间
- [x] 1.3 确认实现只做有限、非阻塞、只读查询，不消费 ring 数据，不修改 ring head/tail

## 2. Reader / Writer facade

- [x] 2.1 `AsyncUartReader` 增加 `can_read()` / `register_readable_waker()`
- [x] 2.2 `AsyncUartWriter` 增加 `can_write()` / `register_writable_waker()`
- [x] 2.3 保持 `TtyRead` / `TtyWrite` / `embedded_io_async` 现有行为不变

## 3. API 文档与边界

- [x] 3.1 rustdoc 明确 `occupied_len()` / `vacant_len()` 是 readiness hint，不保证后续 pop/push 成功
- [x] 3.2 rustdoc 明确 OS 层必须使用 check -> register -> recheck 协议关闭 lost-wakeup 窗口
- [x] 3.3 确认 `uart_16550` crate 不依赖 `axpoll`、VFS、syscall、`IoEvents` 或 fd nonblocking 语义

## 4. Gate 验证

- [x] 4.1 `cargo check --manifest-path crates/uart_16550/Cargo.toml --features async` 通过
- [x] 4.2 `cargo test --manifest-path crates/uart_16550/Cargo.toml --features async` 通过（59 passed, 0 failed，含 14 个新增 readiness 测试）
- [x] 4.3 用户已手动执行 QEMU `make run` 并确认 StarryOS 正常启动、测试正常运行，`/dev/console` 当前路径行为不变
- [x] 4.4 `openspec validate q27a-uart-readiness-interface` 通过

## 5. Deferred to later changes

- 5.1 Q27：将 `Tty::poll()` / `Tty::register()` 的 `IoEvents::OUT` 绑定到 TX ring space
- 5.2 Q27：阻塞 fd 使用 `poll_io(... OUT ...)` 循环完成写入，非阻塞 fd 保持 partial / `WouldBlock`
- 5.3 Q28：收敛 `AsyncUartWriter::Clone` 与 `RingBufTx` SPSC 安全契约
