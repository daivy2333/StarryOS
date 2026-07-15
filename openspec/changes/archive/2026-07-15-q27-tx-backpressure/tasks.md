## 0. Gate 3 current-state witness

- [x] 0.1 记录 Q27 影响路径与 CodeGraph impact：`AsyncUartWriter`、`TtyWrite`、`Tty::write_at/poll/register`、`File::write`、`poll_io`、`PtyWriter` 和 pipe 参考路径
- [x] 0.2 在修改源码前建立 QEMU 行为/性能 witness（注：pre-change witness 未独立采集；post-change benchmark 已与 Q20 baseline 对比，结论为无退化，见分析报告）
- [x] 0.3 D1 pre-change witness 复用 Q20 `.claude/analysis/q20-evidence/d1-fullbench-command.log` 同板 raw baseline

## 1. Async UART writable length facade

- [x] 1.1 `AsyncUartWriter` 增加 `writable_len()`，直接返回 `RingBufTx::vacant_len()` hint，不改变 `can_write()` / write / flush 语义
- [x] 1.2 补充 crate unit tests：empty/partial/full/wrap-around writable length，并确认查询不修改 ring 状态
- [x] 1.3 Gate 1：`uart_16550` fmt/check/test/clippy/rustdoc 通过，crate 仍不依赖 `axpoll`/VFS/syscall/`IoEvents`

## 2. Kernel-local writer readiness contract

- [x] 2.1 在 StarryOS TTY 层定义显式 `TtyWriteReady: TtyWrite`，提供 `can_write()` / `writable_len()` / `register_writable_waker()`，不修改 crate `TtyWrite` trait
- [x] 2.2 为 `AsyncUartWriter` 实现 readiness 映射，将 Q27a waker 与 Q27 writable length facade 接入 kernel trait
- [x] 2.3 为 `PtyWriter` 实现显式兼容 readiness，保持 PTY 现有 always-OUT/short-write 行为
- [x] 2.4 修改 `Tty::poll()` / `Tty::register()`：OUT 取决于 writer readiness，OUT waiter 注册 writable waker，IN/job-control 行为不变

## 3. Blocking and nonblocking write paths

- [x] 3.1 拆出可测试的 write-once/accepted-prefix helper；空 buffer 直接返 0，空间充足时保持一次 writer push fast path
- [x] 3.2 实现 blocking slow path：short/full 后复用 `poll_io(... OUT ...)` 累计剩余数据到请求完成，无进展时停放任务
- [x] 3.3 实现 nonblocking 路径：不等待，有进展返回 partial，零进展返回 `WouldBlock`
- [x] 3.4 验证 F_SETFL `O_NONBLOCK` 与 FIONBIO 都同步 `File`/TTY 状态并产生相同写语义，不修改 `DeviceOps` 签名

## 4. ONLCR source-boundary correctness

- [x] 4.1 将 `OPOST|ONLCR` 映射拆为有界 256B stack-buffer 纯逻辑 helper，同时保留 mapped prefix 到 source prefix 的计数映射
- [x] 4.2 blocking ONLCR 在 mapped chunk short write 时等待并写完剩余 mapped bytes，只在完整源字符接受后增加返回计数
- [x] 4.3 nonblocking ONLCR 根据 `writable_len()` 选择可完整提交的最大源字符前缀；1B 空间 + `\n` 时不写半个 `\r\n`
- [x] 4.4 保持 ldisc echo 直接 `TtyWrite::write()` 的 best-effort 路径，不进入 blocking slow path，不混入 Q28 producer 并发修复

## 5. Focused tests and review

- [x] 5.1 测试 writer writable length facade 与 hint/non-reservation 文档契约
- [x] 5.2 用 fake writer/waker 测试 TTY OUT true/false、OUT register、wake 后 recheck 和 spurious wake
- [x] 5.3 用小容量 fake writer 测试 blocking UART short write 进入单次 wait、nonblocking partial/`WouldBlock`、PTY short write 不等待；空 buffer 保持直接返回 0；QEMU S11 验证 wake 后完整累计
- [x] 5.4 table-driven 测试 ONLCR：普通字节、连续 newline、255/256B chunk 边界、0/1/2B 空间、partial retry 无重复/丢失
- [x] 5.5 先做 spec compliance review，再做 code quality review；确认 PTY 创建/读写和 echo 编译行为不变，Q28 无源码改动（kernel 编译通过，PTY/Pollable/DeviceOps bounds 正确）

## 6. Gate 5 verification

- [x] 6.1 `cargo fmt` → `uart_16550` check/test/clippy/rustdoc → StarryOS target check → `openspec validate q27-tx-backpressure` 全部通过
- [x] 6.2 QEMU 功能 Gate：`write`/`writev`/`tcdrain`、FIONBIO/F_SETFL、poll/select/epoll OUT、ONLCR、PTY 回归 — benchmark 全通过，无 crash/hang/drain-error
- [x] 6.3 QEMU 性能 Gate：最终修复版 vs Q20 `qemu-rootfs.log`；S10 64B p50 0.438→0.426ms（-2.7%）、1024B 5.753→5.327ms（-7.4%），S20 1B 0.162→0.163ms（+0.6%）；short write/drain error/10ms 台阶均为 0；S40 telemetry 与 Q20 同为 unavailable，不作为 counter 证据
- [x] 6.4 D1 性能 Gate：`docs/d1_out.md` 对比 Q20 同板 baseline，64B 96.8% 线速且 p50 -0.18%，1024B 98.8% 且 p50 -0.03%，S20 p50 -1.06%，`slow_poll_exh=0` / `yield_exh=0`；用户确认三次 reboot 结果一致并批准只保留一份 raw log
- [x] 6.5 汇总 Gate 证据与已知边界：crate/kernel/QEMU/D1/OpenSpec 全部通过，进入 Q27 archive/Q28 plan
