## Why

异步 UART 已在提交 `1ce95d7128e9c5583fc28628c72fb7c5c5e62db4` 留下 QEMU 与 D1 基线。当前 `console-lichee` 分支要删除异步 UART，改用 polling Console 运行同一套用户态测试，形成同平台横向对比。

## What Changes

- **BREAKING**：本分支删除本地 `crates/uart_16550`、kernel async UART 模块、copier、UART IRQ、telemetry、startup ring benchmark 及相关依赖和 feature 接线。
- `/dev/console`、stdio 与 controlling TTY 改用 polling Console TTY。
- QEMU 使用 NS16550 byte-MMIO；D1 使用 DW APB UART stride 4、32-bit MMIO。
- TTY 负责 ONLCR，底层 raw writer 不再转换换行。
- Console `tcdrain` 等待 LSR TEMT；TX debug ioctl 返回明确的不支持结果。
- 复用异步基线的 S00、S10-S14、S20-S21、S30-S31、S40 测试顺序、数据量与计时方法。无对应能力的 section 输出 `UNSUPPORTED` 或 `SKIPPED`，不伪造 PASS。
- QEMU 与 D1 分别采集 raw log。只在同一平台内比较 async 与 Console。
- CPU 占用率后置。本 change 不增加新指标，以免改变测试方法。

## Capabilities

### New Capabilities

- `polling-console-baseline`：定义 `console-lichee` 的 Console-only 生命周期、TTY 行为、TEMT drain、测试兼容和证据边界。

### Modified Capabilities

无。现有 async UART specs 仍描述异步分支；本 change 是分支限定的测量能力，不修改正式 async UART 架构。

## Impact

- 删除：`crates/uart_16550/` 与 `kernel/src/drivers/` 下的 async UART 集成模块。
- 修改：workspace/kernel Cargo features 与依赖、`kernel/src/entry.rs`、`kernel/src/pseudofs/dev/`、TTY traits、`kernel/src/syscall/fs/ctl.rs`、平台 Console MMIO、`tests/benchmark.c`、Makefile。
- 证据：异步 raw logs 保持冻结；Console logs 使用独立文件并记录 async base、Console commit、构建命令、镜像和运行环境。
- 活跃 `q17-smp-memory-ordering` 不属于本 change；删除 async 路径不会把 Q17 的 multi-hart 未完成项声明为已验证。

## Approved Requirements

用户要求：“当前已经在测试分支上了，可以随意进行更改，清理异步uart变成console”；随后确认“在这个分支把异步uart替换回console。然后做和异步uart一样的测试，这样可以进行横向对比”，并要求先保持测试一致，暂不增加 CPU 占用率。

据此批准以下范围：

- Console-only，不保留双后端兼容。
- 删除 async UART crate 与产品集成。
- 保持 benchmark workload、顺序、数据量和计时方法一致。
- 能力差异必须显式标注，不用假结果填齐矩阵。
- CPU 指标、SMP、DMA、高波特率、SDMMC/rootfs 与实验代码是否回主线均不在范围内。

## Scenario Sketch

| 场景 | 前置状态与动作 | 可观察结果 | 失败边界 |
|---|---|---|---|
| Console TX | Console mode 启动并写 `/dev/console` | QEMU 与 D1 均由 polling writer 完整发送 | async init、copier 或 UART IRQ 被调用即失败 |
| ONLCR | 默认 termios 写入含 `\n` 的数据 | 线上字节仅出现一次 `\r\n` | `\r\r\n` 或漏转换即失败 |
| Drain | THRE=1、TEMT=0 时调用 `tcdrain` | 等待至 TEMT=1 后返回 | 提前返回或无界 hang 即失败 |
| Empty RX | benchmark 执行 S30/S31 | 支持的平台按原语义测试；不支持处输出 `UNSUPPORTED` | 把 0-byte/EAGAIN 伪装成 RX 能力 PASS 即失败 |
| Telemetry | benchmark 执行 S40 | 输出 `UNSUPPORTED backend=polling-console` | 访问已删除 async driver 即失败 |
| QEMU evidence | 相同 benchmark 在 QEMU 运行 | 完整退出并生成 Console raw log | 用 QEMU 数值声明真板线速即失败 |
| D1 evidence | 相同 benchmark 镜像在 D1 运行 | 完整退出，日志含镜像与板卡信息 | 无真板日志却声明四格比较完成即失败 |
| Build compatibility | 删除 async features 与依赖后构建 | QEMU 和 D1 Console targets 通过 | 残留 feature、模块或 Cargo 引用即失败 |
| Timeout/cancel | polling 等待遇到硬件永不 ready | Gate 记录可定位的 hang/timeout 证据并停止 | 添加无依据的静默丢字节或假成功即失败 |

