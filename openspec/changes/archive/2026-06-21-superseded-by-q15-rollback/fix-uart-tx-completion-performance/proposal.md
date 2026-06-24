## Why

M4 Sync 后 `write+tcdrain` 在 64B 场景测得 29.99ms（M4 前同样本为 406µs，根因已于 2026-06-21 经 git bisect 确认为 TX backpressure + 100Hz tick 调度量化），而 115200bps 理论线速为 5.56ms；现有“TX writer 锁与 ISR 竞争同一锁”的解释已被源码否定。与此同时，当前 TX completion 以 ring empty 代替 copier staging/hardware completion、依赖不可靠的 THRE→TEMT 唤醒，并会把 ring 满短写伪装为成功，因此必须在继续微优化前修复可验证的状态与契约。

## What Changes

- 固化已确认的同 commit、同 QEMU 配置、同 `write+tcdrain` before/after 证据，并增加 16/17B 等 FIFO 边界延迟与阶段计数作为永久回归见证。
- 保持 ISR 极简和 copier 搬运架构，禁止在 ISR 搬运数据；不修改外部 `axtask`，通过 StarryOS/uart_16550 可控边界消除每 16B refill 的 10ms 调度台阶。
- 为 TX 引入显式 queued/staged/hardware-in-flight completion 状态，使 `tcdrain` 只在全部阶段完成且 TEMT 成立时返回，并提供无独立 TEMT IRQ 时的可靠有界重查机制。
- 统一 IER 状态所有权，移除 `CACHED_IER + enable_*_intr callback` 与 per-port `update_ier()` 并存造成的 cache/hardware 分裂。
- **BREAKING**：调整同步 TTY writer 契约，使 ring 满时返回短写或 `WouldBlock`，禁止 `write_at()` 无条件报告全部成功。
- 缩小 TX producer 锁临界区，将 metrics 与 wake 移到解锁后；仅在主根因验证和正确性修复完成后进行 ISR 单事务等 P2 微优化。
- 保持 NS16550 stride=1、MMIO backend 封装、Console/Async 共存及 multi-producer 内存安全，不回退到无锁 `UnsafeCell<Writer>`。

## Capabilities

### New Capabilities

- `uart-tx-completion-performance`: 定义 TX 三阶段完成语义、可靠 tcdrain、短写/backpressure 契约、FIFO refill 性能基线和回归验收。

### Modified Capabilities

- `async-uart-core`: TX copier、writer 和 IRQ 状态必须暴露可验证且无竞态的 completion/backpressure 行为。
- `arceos-adapter`: StarryOS 适配层必须保持极简 ISR、单一 IER owner，并在不修改 axtask 的前提下提供及时的 copier 进展。

## Impact

- StarryOS：`kernel/src/drivers/{uart_init,os_arceos}.rs`、`kernel/src/syscall/fs/ctl.rs`、TTY `write_at` 调用链、`tests/benchmark.c` 与 QEMU benchmark 脚本。
- 本地 sibling `uart_16550`：`src/async_/{driver,ring_buffer,device_ops}.rs` 和对应 async 单元测试；执行阶段需要单独可写权限及协调变更记录。
- API：`TtyWrite`/writer 返回值与 TX completion 查询可能发生 breaking change，所有调用方必须由 CodeGraph callers 验证并迁移。
- 不修改 crates.io `axtask`、`axpoll`、`embassy-sync`；不新增 async runtime；不在 ISR 搬运 FIFO 数据。
- 性能验收：QEMU 同环境下 Async `write+tcdrain` 在 64B–4096B 不劣于阻塞基线 10%；1B 延迟相对 M4 前同方法基线退化不超过 10%。VisionFive2 真板必须验证无 hang、无丢字节且达到同样相对门槛，若当前阶段无硬件则作为发布阻塞项保留。
