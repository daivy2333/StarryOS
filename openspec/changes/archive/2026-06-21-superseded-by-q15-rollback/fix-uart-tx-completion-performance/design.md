## Context

双 repo 同方法回退已确认 M4 性能边界：pre-M4 StarryOS `04f8920` / uart_16550 `60c5729` 的 64B `write+tcdrain` 为 406µs，M4 StarryOS `11e60ff` / uart_16550 `2e91e40` 为 29.99ms。uart_16550 `7c1e9f4` 将 TX FIFO 满路径从 `Poll::Ready` 同步重试改成 `Poll::Pending`，消除了无限 busy-poll，却使每个最多 16B 的后续 FIFO refill 依赖 THRE IRQ 唤醒和 task 重新调度。axtask waker 使用 `unblock_task(task, false)`，StarryOS tick 为 100Hz，因而 64B 首批后的 3 次 refill 形成约 30ms 调度量化。

同一路径还有三项正确性债务：ring empty 不包含 copier staging、THRE 不保证在 TEMT 置位时产生新事件、同步 `TtyWrite` 忽略短写。StarryOS 还保留 `CACHED_IER + enable_* callback`，与 driver 内 `update_ier()` 构成双 owner。

约束：ISR 必须极简且禁止搬运数据；不修改外部 axtask/axpoll/embassy-sync；不提高全局 tick 掩盖局部问题；保持 stride=1、backend MMIO 封装、multi-producer 安全和 Console/Async 共存。

## Goals / Non-Goals

**Goals:**

- 用自动化 benchmark 固化 M4 `Ready→Pending` 回归和 16B FIFO 台阶。
- 在不恢复无限 busy-poll 的前提下消除 QEMU 快速可写路径的 10ms/refill 台阶。
- 使 drain 条件覆盖 ring queued、copier staged 和 UART TEMT 三阶段。
- 让 IER 只有 driver/UartPort 一个 owner，并修复 TTY 短写语义。
- 分离主修复与 metrics/锁微优化，使每项收益可独立归因。

**Non-Goals:**

- 不在 ISR 读写 ring 或 refill FIFO。
- 不修改 axtask waker、不新增 async runtime、不把 tick 提高到 1000Hz。
- 不实现 DMA、零拷贝或 Console TX 合并。
- 不以回退 `UnsafeCell<Writer>` 牺牲 multi-producer 内存安全。

## Decisions

### D1：先建立 selective-hunk 性能见证

测试矩阵固定 pre-M4、M4、仅恢复 `7c1e9f4` TX hunk 三个点，并覆盖 1/15/16/17/31/32/33/48/49/64/256/1024/4096B。每轮记录 wall time、CPU idle、`tx_poll/tx_hw_bytes/tx_no_progress/irq_storm` 增量。只有 selective hunk 恢复性能且暴露 busy-poll，才进入 GREEN 实现。

替代方案：只比较 HEAD 与理论线速。它不能归因 commit，也混淆 QEMU 时序，因此拒绝。

### D2：32 次有界 fast-path retry，随后 IRQ Pending

`tx_copier_loop` 在 `send_bytes()==0` 时最多立即重试 32 次；每次使用 `core::hint::spin_loop()`，期间不 await、不持有 UART 锁。任一重试成功即继续当前 staging；32 次均失败后执行：注册 TX waker → enable THRE → 再检查一次可写状态 → 仍不可写才 `Poll::Pending`。

32 是初始安全上限，不允许在实现中静默扩大。验收同时要求：QEMU 64B–4096B 不劣于阻塞基线 10%，空闲 10 秒 `tx_poll` 不持续增长，真板无法命中 fast path 时能稳定回落 IRQ Pending。若 32 次不能达标，任务回到设计 Gate，不得改成无限循环。

替代方案：恢复 pre-M4 `Poll::Ready` 无限重试会重新引入 busy-poll；无条件 Pending 已确认退化；全局 tick 调整影响整个内核；均拒绝。

### D3：显式 `tx_copier_active` 与 staged byte 状态

driver 在尝试从 TX ring 取 staging 前设置 `tx_copier_active=true`，在确认 ring 无数据时清零；staging 有效期间维护 `tx_staged_bytes`，每次成功提交 UART 后递减。drain 完成谓词必须同时满足：ring 无待处理 generation、`tx_copier_active=false`、`tx_staged_bytes=0`、TEMT=true。

状态更新使用 Release，drain 读取使用 Acquire；任何 drain wait 均采用 check→register→recheck，消除 pop 后 store 前的提前返回窗口。

### D4：TEMT 收尾使用协作 yield，不假设独立 TEMT IRQ

THRE handler 可以 opportunistic wake drain，但不得作为唯一事件源。ring/staging 已空而 TEMT=false 时，`flush`/`tcdrain` 调用 `yield_now().await` 后重查 TEMT；它不持锁、不在 ISR 执行，且等待范围只覆盖最后 shift register。该路径保留 POSIX tcdrain 的“硬件未完成则继续等待”语义。

替代方案：纯 `drain_waker` 可能永久睡眠；把 ring empty 当完成会提前返回；二者拒绝。

### D5：driver 独占 IER 状态

移除 `start_rx_copier/start_tx_copier` 的 enable callback 参数和 StarryOS `CACHED_IER/write_ier/enable_*_intr`。copier 通过 `UartPort::update_ier(set, clear)` 直接使能中断；ISR 在同一 UartPort 临界区清位。cache 若保留必须位于 UartPort 内且每次硬件写同步更新，禁止 OS callback 与 driver 双写。

### D6：`TtyWrite` 返回实际接收字节数

`TtyWrite::write(&self, &[u8])` 改为返回 `usize`。Async writer 返回 `RingBufTx::push` 的实际值；TTY `write_at` 返回该值，0 字节且输入非空时由现有 blocking/nonblocking 层转换为等待或 `WouldBlock`。PTY writer 同步迁移并保留其 PollSet wake。echo 调用明确按 best-effort 处理返回值。

### D7：metrics 与 wake 移出 producer 临界区

`RingBufTx::push` 的 mutex 只保护 `Writer::push`；accepted/dropped/high_water 与 `poll.wake()` 在解锁后执行。O65 metrics 降成本必须作为独立提交，在 O63 达标后 A/B；优先 feature gate 或批量累计，不删除可观测字段。

## Risks / Trade-offs

- [32 次 fast retry 在慢硬件上增加 MMIO 读] → 严格上限并用真板 `tx_no_progress`/CPU 指标验证，失败立即回落 IRQ。
- [QEMU 达标但真板收益有限] → QEMU 验证调度回归，VisionFive2 验证线速、CPU 和无 hang；两类数据分开报告。
- [TtyWrite breaking change 影响 PTY/echo] → CodeGraph callers 全量迁移，先加 compile/behavior RED tests。
- [completion atomics顺序错误导致提前 drain] → 可控 mock 在 pop→stage、最后 write→TEMT 两个窗口注入并发。
- [协作 yield 形成 drain storm] → 仅在 ring/staging 均空后启用，记录 drain poll count；持续增长即 Gate 5 失败。
- [双 repo 变更无法原子提交] → 先 uart_16550 API/tests，再 StarryOS adapter；每阶段保持明确兼容点和回滚 commit。

## Migration Plan

1. 在 uart_16550 建立 M4 backpressure、completion 和短写 RED tests；StarryOS 增加 FIFO 边界 benchmark，不改生产行为。
2. 实现 D2 bounded fast-path，验证性能和 idle CPU；失败回滚该提交，不进入后续任务。
3. 实现 D3/D4 completion state 与 drain，迁移 flush/tcdrain。
4. 实现 D5 IER 单 owner，删除 StarryOS callback/cache 双路径。
5. 实现 D6 TtyWrite breaking migration，再做 D7 临界区与 metrics 独立优化。
6. 运行 uart_16550 tests/clippy/doc、StarryOS check/clippy/QEMU benchmark；VisionFive2 未执行时保持发布阻塞项。

回滚顺序相反：先恢复 StarryOS 旧 API 接线，再恢复 uart_16550 公共 API；性能 fast-path 可独立回滚，不得恢复无限 busy-poll。

## Open Questions

无。若 32 次 fast retry 无法满足 QEMU 门槛，按 Gate 6 返回设计阶段评估 runtime resched 能力，不自动扩大预算或修改外部 axtask。
