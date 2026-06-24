## Context

Q15 增量重融合从 pre-M4 基线（StarryOS `05291f2` / uart_16550 `95eb081`）出发。M4 Sync 曾因 TX backpressure + 100Hz tick 导致 64B write+tcdrain 退化 73.9x（406µs→29.99ms），已回退。当前需要先建立细粒度性能见证，才能在后继 M1 修复中证明改善。

关键约束：不改生产行为、不改外部 crate、不改全局 tick、ISR 极简。

## Goals / Non-Goals

**Goals:**
- 扩展 benchmark.c 到 1/15/16/17/31/32/33/48/49/64/256/1024/4096B FIFO 边界矩阵
- 输出 raw samples、P50/P95、每轮元数据（commit hash、tick 率、FIFO 深度）
- uart_16550 新增 `#[cfg(feature = "telemetry")]` 诊断计数器（不启用时零开销）
- 建立可重复的 pre-M4 性能基线，作为 M1 修复的 RED 证据

**Non-Goals:**
- 不修改 tx_copier_loop（那是 M1）
- 不修改 TtyWrite::write 返回值（那是 M3）
- 不修改 IER 所有权（那是 M4）
- 不修改 tcdrain/flush（那是 M2）
- 不增加全局 tick、不修改 axtask/axpoll/embassy-sync

## Decisions

### D1: 诊断计数器使用独立的 `telemetry` feature gate

**选择**：`#[cfg(feature = "telemetry")]` 包裹所有计数器读写。

**原因**：计数器是 `AtomicU64::fetch_add` 操作在热路径（tx_copier_loop 每次 poll），启用时引入原子开销。不启用时编译器完全消除，保证基准测试不受影响。

**替代方案**：always-on 原子计数器（`Ordering::Relaxed`）。拒绝——即使 Relaxed 在 RISC-V 上也非零成本，违背"M0 不改行为"原则。

### D2: benchmark 输出使用机器可解析的 `key=value` 格式

**选择**：每轮输出 `bytes=N raw_samples=[...] p50=X p95=Y commit=HASH tick=HZ fifo=SIZE` 等键值对。

**原因**：手工解析原生文本脆弱，`key=value` 支持 `grep` + `awk` 一键提取，也方便后续 CI 脚本消费。

**替代方案**：JSON。拒绝——内核态 printf 无 JSON 库，手拼 JSON 易出错且增加 benchmark 复杂度。

### D3: 计数器语义定义

| 计数器 | 位置 | 含义 |
|--------|------|------|
| `tx_poll` | tx_copier_loop 入口 | 每次 poll_fn 调用 |
| `tx_no_progress` | send_bytes()==0 分支 | 本轮未写入任何字节 |
| `tx_hw_bytes` | send_bytes() 成功后 | 本轮写入硬件 FIFO 的字节数 |

**原因**：这三个计数器足以区分"高效轮询"（tx_poll ↑，tx_hw_bytes > 0）和"空转"（tx_poll ↑，tx_no_progress ↑），无需更复杂的直方图。

## Risks / Trade-offs

| 风险 | 缓解 |
|------|------|
| benchmark 在 pre-M4 基线上全绿，无法展示退化证据 | 记录 detailed 输出（每轮 raw/P50/P95），M1 修复后 A/B 对比。pre-M4 数据本身就是 M1 的目标基线 |
| counter 原子操作在 RISC-V 上仍有开销（即使 feature 关闭） | `#[cfg]` 确保编译时完全消除，非运行时分支 |
| benchmark 输出格式变更破坏现有解析 | 新增尺寸和元数据字段，不删除已有输出项 |
