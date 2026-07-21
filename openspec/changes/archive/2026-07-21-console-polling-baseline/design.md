## Context

当前 HEAD `ba61e1aa4f68783c864239acc4419b5ec7a41bec` 位于 `console-lichee`。异步测量基线为 `1ce95d7128e9c5583fc28628c72fb7c5c5e62db4`，已有 `docs/qemu_out.md` 与 `docs/d1_out.md`。

当前 `/dev/console` 绑定 `ASYNC_TTY`。启动路径初始化 async driver、运行 ring benchmark、启动 RX/TX copier，并由 UART IRQ 推进。`TCSBRK` 和 TX debug ioctl 也绕过 fd，读取全局 async driver。

TTY 的 `TtyRead`、`TtyWrite` 来自待删除 crate。TTY 已负责 ONLCR；D1 平台 Console 也转换 LF，因此用户态 writer 不能复用带换行策略的 `ConsoleIf::write_bytes()`。

## Goals / Non-Goals

**Goals:**

- 删除本分支的 async UART 代码、依赖和生命周期。
- 建立 QEMU/D1 共用语义、平台分离实现的 polling Console TTY。
- 保持现有 S 系列 workload 和计时方法。
- 采集 QEMU、D1 Console raw logs，并与冻结 async logs 同平台比较。

**Non-Goals:**

- 不保持本分支 async feature 可编译。
- 不修改原异步分支或其 specs。
- 不增加 CPU 占用率指标。
- 不处理 SMP、DMA、高波特率、SDMMC/rootfs 或 Q30 公平性。
- 不用 QEMU 结果声明物理线速。

## Decisions

### D1：删除 async 集成，不维护双后端

**Decision**：删除 `crates/uart_16550/`，移除 kernel async modules、依赖、features、entry 分支、IRQ/copier、telemetry 和 startup ring benchmark。

**Reason**：用户把当前分支定义为可自由修改的 Console 测量分支。保留双后端会增加无关兼容 Gate，并可能让两个 owner 同时操作 UART。

**Impact**：本分支不再验证 async modes。回滚依赖 Git 和冻结提交，不靠条件编译恢复。

**Alternatives**：保留 `polling-console` feature 与 async feature。拒绝，因为它偏离 Console-only 目标。

### D2：TTY traits 本地化

**Decision**：把 `TtyRead`、`TtyWrite` 的最小合同迁入 kernel TTY 模块。`TtyWriteReady` 继续表达 VFS readiness；Console writer 实现同步完整写、恒可写和立即唤醒注册者。

**Reason**：这些 trait 已成为 TTY/PTY 的内核合同，不能随 async crate 一起删除。

**Impact**：PTY 与 Console 共用本地 trait。移除 crate 后仍能编译 TTY。

**Alternatives**：创建新的通用 UART crate。拒绝，因为本分支只需要 Console 基线。

### D3：raw polling port 与换行策略分离

**Decision**：新增 kernel-local raw polling port，提供 `write_byte`、非阻塞 `read` 和 `transmitter_empty`。QEMU 使用 U8/stride 1，D1 使用 U32/stride 4。raw 层不做 LF 转换。

**Reason**：TTY 已按 termios 执行 ONLCR。复用 D1 `ConsoleIf::write_bytes()` 会产生 `CR CR LF`。

**Impact**：early/panic Console 保持现状；用户态 Console TTY 使用 raw port。两者写 UART 时共享 Console lock，降低字节交错。

**Alternatives**：关闭 TTY ONLCR。拒绝，因为会改变 termios 行为和异步基线 workload。

### D4：按需轮询 RX，不创建常驻 spinner

**Decision**：为 TTY 增加 polling input mode。VFS read/poll 触发 raw RX 检查；阻塞读取仅在存在 waiter 时重查。D1 无 RX 实现时显式声明 unsupported。

**Reason**：常驻自唤醒 reader 会污染 benchmark，并在没有读者时消耗 CPU。

**Impact**：QEMU 可保留 shell/FIONBIO smoke；D1 TX benchmark 不伪造 RX 能力。

**Alternatives**：常驻 polling task。拒绝，因为测试主要比较 TX，常驻任务会改变测量环境。

### D5：Console drain 只看物理 TEMT

**Decision**：`TCSBRK` 调用 Console port 的 drain。循环必须在 TEMT=1 后返回；THRE=1/TEMT=0 的 mock 是 RED/GREEN witness。运行 Gate 用外部 timeout 发现硬件 hang，不给 syscall 添加改变语义的超时。

**Reason**：Console 没有 ring、copier 或 staged bytes，唯一剩余完成条件是 UART shift register empty。

**Impact**：删除 `TxCompletion` 和 `DRAIN_WAKER` 依赖。TX debug ioctl 返回 unsupported。

**Alternatives**：只检查 THRE。拒绝，因为会在末字节仍在线上时提前返回。

### D6：保持 benchmark 方法，不强求能力对称

**Decision**：保留 section 顺序、payload、iteration、timer、write/drain 边界。增加 backend manifest。Console S40 与 startup ring 输出 unsupported/skipped；D1 RX 输出 unsupported。S11 明确是 blocking transmit，不称为 enqueue。

**Reason**：横向比较依赖 workload 一致，不依赖内部能力数量一致。

**Impact**：现有 benchmark 只做标签和能力分支，不新增 CPU 指标。

**Alternatives**：另写 Console benchmark。拒绝，因为会破坏对照方法。

### D7：分层采集证据

**Decision**：验证顺序为 host tests → QEMU/D1 build → QEMU boot → QEMU benchmark → D1 image → D1 benchmark → 同平台比较。保存 `docs/qemu_console_out.md` 与 `docs/d1_console_out.md`。

**Reason**：QEMU 不模拟物理线速；D1 烧录属于高风险人工步骤。

**Impact**：缺 D1 时标记 ENV BLOCK，不完成四格结论。Act 不自动烧录真板。

**Alternatives**：只采集 QEMU。拒绝，因为不能回答硬件性能对比。

## Risks / Trade-offs

- [删除范围遗漏] → 用 `rg` 审计 crate、feature、symbol 与 Cargo.lock 残留。
- [early log 与 TTY 字节交错] → 共用 Console lock，panic 路径仍保留可输出性。
- [polling RX 忙等] → 只在 read waiter 存在时重查；D1 RX 不支持则不启动 reader。
- [drain hang] → mock 覆盖 THRE/TEMT；QEMU/D1 Gate 使用外部 timeout 和完整日志。
- [benchmark 名称误导] → manifest 标 backend；S11 分列 write elapsed 与 final drain。
- [分支漂移] → 每份日志记录 async base、Console commit、benchmark version 和构建参数。
- [真板写入风险] → Act 只生成镜像和烧录说明；烧录由用户按 Runbook 手动执行。

## Migration Plan

1. 冻结并核对 async baseline、raw logs 与当前 Console 起点。
2. 先添加 local traits、mock port 和 RED tests。
3. 删除 async crate、依赖、features、modules 和 entry 生命周期。
4. 接入 raw polling port、Console TTY、VFS 与 ioctl。
5. 调整 benchmark 标签和 unsupported 输出。
6. 完成 host/QEMU/D1 build Gates。
7. 采集 QEMU log，再由用户执行 D1 烧录与采集。
8. 生成同平台比较；D1 缺失则保持 ENV BLOCK。

Git 保留完整回滚路径。本 change 不提供运行时回退到 async 的开关。

## Open Questions

无。CPU 占用率已由用户明确后置。
