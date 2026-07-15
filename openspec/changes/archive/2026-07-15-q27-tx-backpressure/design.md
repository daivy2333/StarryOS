## Context

Q27a 已在 `uart_16550` crate 提供 `can_write()` / `register_writable_waker()` 与 ring 总空间观测，TX copier 在 pop 释放空间后会 wake 现有 waker set。StarryOS `Tty::poll()` 目前无条件返回 OUT，`Tty::register()` 只注册 IN，`Tty::write_at()` 在 short write 时直接返回 partial。外层 `File::write()` 使用 `poll_io`，但 `poll_io` 遇到 `Ok(partial)` 会立即返回，不能为阻塞 TTY 累计到请求完成。

TTY writer 有两类：`AsyncUartWriter` 和 `PtyWriter`。ldisc echo 直接调用 `TtyWrite::write()` 并显式 best-effort，不应被 Q27 改成阻塞路径。F_SETFL 和 FIONBIO syscall 路径都会同步 `File::nonblock` 与 TTY 内部 `nonblocking`，因此 TTY slow path 可以使用本地状态决定 blocking/nonblocking 行为。

历史 M4 Sync 曾因 TX wake/schedule 路径引入 10ms FIFO refill 台阶，导致 64B write+tcdrain 退化 73.9x。Q27 必须先建立 pre-change witness，保留无等待 fast path，并将调度台阶与真板线速作为阻断 Gate。

## Goals / Non-Goals

**Goals:**

- UART OUT readiness 与 TX ring 可用空间一致。
- 阻塞 UART fd 在 ring full/short 时不忙等，累计到完整请求。
- 非阻塞 UART fd 保持 partial/`WouldBlock`，F_SETFL 与 FIONBIO 语义一致。
- ONLCR 转换不在非阻塞返回边界留下半个 `\r\n`。
- 保留 PTY、echo、`tcdrain` 和 Q15/Q20 fast path 行为与性能。

**Non-Goals:**

- 不收敛 writer clone/SPSC 契约，不引入 producer 锁或 MPSC ring。
- 不把 PTY 改成可靠阻塞输出，不改 echo best-effort。
- 不改 TX copier、ISR、IER 或 completion/drain 状态机。
- 不引入新 runtime、wait queue 或外部依赖。

## Decisions

### D1: 在 kernel 定义显式 `TtyWriteReady` 契约

**选择**:

- 保留 `uart_16550::TtyWrite::write() -> usize` 不变。
- 在 StarryOS TTY 层定义 `TtyWriteReady: TtyWrite`，包含 `can_write()`、`writable_len()` 和 `register_writable_waker()`。
- `AsyncUartWriter` 实现映射 Q27a readiness；`PtyWriter` 显式实现与当前 always-OUT 行为等价的兼容语义。

**理由**:

- `IoEvents` / fd policy 仍留在 OS 层。
- 显式 trait 比 callbacks/downcast 更可搜索，新 writer 缺 readiness 实现时会编译失败。
- 不使用隐式 blanket default，避免未来 writer 被默认标记为永远 writable。

**替代方案**:

- 直接扩展 crate `TtyWrite`：拒绝，会把 Q27 OS 策略扩散到可复用 crate。
- TTY 内 downcast `AsyncUartWriter`：拒绝，破坏泛型边界且难维护。

### D2: OUT poll/register 只映射 writer readiness

`Tty::poll()` MUST 用 `writer.can_write()` 设置 OUT；`Tty::register()` MUST 在请求 OUT 时调用 `register_writable_waker()`。等待协议依赖 `poll_io` 的 poll -> register -> poll/recheck，允许 spurious wake，不把 readiness hint 当 reservation。

### D3: 保留 fast path，short/full 才进 slow path

**选择**:

1. 空 buffer 立即返回 0。
2. 先执行一次现有 writer push；若完整接受，立即返回，不进入 async wait。
3. 若 short/full，阻塞模式复用 `poll_io(self, OUT, false, ...)` 累计剩余数据；只在 writer 返回 0 或仍有剩余时返回内部 `WouldBlock` 继续等待。
4. 非阻塞模式不等待；已接受前缀则 `Ok(partial)`，零进展则 `Err(WouldBlock)`。

**理由**:

- fast path 不新增 allocation、lock、wake 或 task yield。
- slow path 与 pipe backpressure 模式一致，但局限于 TTY，不修改全局 `File::write()` 语义。
- F_SETFL/FIONBIO 已同步 TTY 本地 nonblocking 状态，无需改 `DeviceOps` 签名。

### D4: ONLCR 以源字符边界计数

- 继续使用有界 256B stack buffer，不 allocation。
- blocking 模式可在一个 mapped 字符内分多次 push，但只在整个 mapped chunk 接受后增加已消耗源字节数，不向用户态暴露半字符状态。
- nonblocking 模式先用 `writable_len()` hint 选择能够完整映射的最大源字符前缀，再进行一次 push。若下一个 `\n` 需要 2B 而当前只有 1B，返回已完成前缀或 `WouldBlock`，不先写入 `\r`。
- 该保证依赖 Q28 前的单 UART producer 前提；Q27 不为 clone writer 加锁。

### D5: 性能使用结构 Gate + 实测 Gate

- 结构 Gate：空间充足时仍是一次 writer push；无 heap allocation、无 producer lock、无 await/yield。
- QEMU Gate：实施前后各 3 轮，对比 1B latency 和 64B write+tcdrain 中位数；性能下降 >10% 或出现 10ms 台阶则 BLOCK。
- D1 Gate：复跑 Q20 S10/S20/S40 关键路径；64B 保持 >=95% 线速，1024B 保持 >=98% 线速，`slow_poll_exh=0` / `yield_exh=0`；与 pre-change 中位数相比不得退化 >3%。
- 若真板不可用，Q27 可完成功能验证，但不得声明“D1 性能无退化”或归档性能 Gate。

## Risks / Trade-offs

- **[nested poll 开销]** 外层 `File::write()` 已使用 `poll_io`，TTY slow path 可能再进入本地 wait → fast path 不进本地 wait，并用 QEMU/D1 基线量化。
- **[readiness 只是 hint]** poll 后 ring 状态可变 → slow path 只依赖实际 `write()` 返回数，始终 recheck。
- **[ONLCR 代码复杂度]** transformed bytes 与 source count 不同 → 拆出纯逻辑 prefix mapping helper，用 table-driven tests 覆盖 0/1/2B 空间和 chunk 边界。
- **[PTY 共用 TTY]** readiness bound 可影响 PTY 编译/行为 → `PtyWriter` 显式兼容实现，PTY 创建/读写回归纳入 Gate。
- **[Q28 未完成]** 并发 cloned writer 可使 writable snapshot 过期 → 记录单 producer 前提，Q27 不开启并发 writer stress 或添加锁。

## Migration Plan

1. 先建立 QEMU/D1 pre-change 性能与行为 witness。
2. 单独提交 crate writable length facade 与 unit tests。
3. 单独提交 kernel readiness trait + poll/register，验证 poll/select/epoll。
4. 单独提交 write fast/slow path 与 ONLCR 边界，验证 blocking/nonblocking。
5. 完成 QEMU/D1 post-change A/B Gate；任一性能或 hang Gate 失败则回退对应原子步骤，不进入 Q28。

## Open Questions

无。Phase 3 开始前需要用户批准 Gate 2 完整性。

## Requirements Traceability Matrix

| Requirement | Task(s) | Coverage | Simplification | Status |
|-------------|---------|----------|----------------|--------|
| R1: writer writable length facade | 1.1-1.3, 5.1 | 100% | None | Covered |
| R2: TTY OUT readiness/waker | 2.1-2.4, 5.2 | 100% | None | Covered |
| R3: blocking complete write | 3.1-3.4, 5.3 | 100% | None | Covered |
| R4: nonblocking partial/WouldBlock | 3.1-3.4, 5.3 | 100% | None | Covered |
| R5: ONLCR complete-source boundary | 4.1-4.4, 5.4 | 100% | None | Covered |
| R6: PTY/echo/Q28 boundary | 2.3, 4.4, 5.5 | 100% | None | Covered |
| R7: no performance regression | 0.1-0.3, 6.1-6.4 | 100% | None | Covered |

