## Why

MS04 引入异步 RX 前，项目缺少可复现的轮询网卡性能数据。MS16 需要冻结 workload、指标、完成语义和 Evidence，使后续轮询、异步、QEMU 与真板实现使用同一测试口径。

## What Changes

- 增加 host 与 StarryOS guest 共用的 TCP/UDP benchmark 协议和程序。
- 增加版本化 manifest、capability、C1-C6 完成点和原始记录格式。
- 增加吞吐、PPS、RTT、delay variation、UDP 完整性、背压、QEMU CPU、单 hart 指令和 MS03 IRQ 指标。
- 增加报告器、Evidence 完整性检查和 A/B `comparison_key` 检查。
- 使用 user-net 执行功能 smoke，使用 TAP 执行正式 QEMU 轮询 B0。
- 保持人工启动 QEMU 和输入 guest 命令；host peer、采集与离线解析可自动执行。
- 为 netem、长稳、SMP、多队列和真板指标保留 Schema，本 change 不执行这些 profile。
- 不改变网络行为、队列容量、socket 容量或 10 ms polling fallback。
- 不新增 descriptor、copy、queue 或 scheduler telemetry；缺失能力记录为 `unavailable`。

## Capabilities

### New Capabilities

- `network-benchmark-baseline`: 定义跨平台 workload、指标、完成语义、QEMU 轮询 B0、Evidence 和 A/B 比较资格。

### Modified Capabilities

- None.

## Scenario Sketch

| Scenario | Pre-state | Action | Observable result | Failure boundary |
|---|---|---|---|---|
| 正常 TCP/UDP 传输 | 双端版本和配置一致 | warm-up 后执行 workload | C6 账本闭合并生成有效 round | 任一校验或摘要失败使 round invalid |
| 配置不一致 | hash、版本或角色不同 | 执行控制握手 | 数据传输前失败 | 不产生性能结果 |
| partial I/O | socket 只接受或返回部分数据 | benchmark 继续推进 | 字节账本闭合，调用数进入统计 | EOF、timeout 或零进展越界使 round invalid |
| 非阻塞背压 | sender 填满 socket buffer | 等待可写并恢复 | 记录 EAGAIN、等待和恢复时间 | 永久不可写或丢字节使测试失败 |
| peer 退出 | 测量尚未结束 | peer EOF 或进程退出 | 保留已完成账本和失败原因 | 不把部分结果计入 B0 |
| UDP 异常 | receiver 收到异常序号或 payload | 执行序号和校验检查 | 分类记录 loss、duplicate、reorder、corrupt、late | 不折叠成单一 loss |
| 可选计数器缺失 | capability 不支持 | 执行外部 workload | 指标为 unavailable | 要求该能力的 profile 阻塞 |
| 时钟异常 | monotonic 回退或校准无效 | 计算时间与速率 | round invalid | 不输出错误速率 |
| Evidence 缺失 | 正式运行结束 | 执行 Evidence checker | G6 失败并列出缺失项 | 不声明正式 B0 |
| A/B 环境不一致 | comparison key 字段不同 | 生成比较报告 | 拒绝改善比例并列出差异 | 不合并不可比数据 |

## Approved Scope Defaults

Gate 1 approval: 2026-08-04，用户回复“同意”。

- 一个 change 分为工具与协议、QEMU 校准、正式 B0 三个可验证批次。
- TCP benchmark 固定 `TCP_NODELAY=1`，并写入 manifest。
- UDP offered-load 由 pilot 得到零丢包基准，再执行 25%、50%、75%、90% 和 100%。
- 标准 profile 要求 TCP 账本闭合，UDP corruption、duplicate、reorder 和 loss 为零。
- 首轮不设置绝对性能阈值；B0 记录轮间波动。
- 正式 B0 使用 required Evidence。工具开发阶段的短验证写入 Act Response。

## Impact

- 新增 `tests/network_benchmark.c`、协议与平台适配头文件。
- 新增 host 报告和 Evidence 检查脚本及其测试夹具。
- 新增 benchmark payload 的构建入口和文档化运行参数。
- 正式运行沿用 R44、R45、R48 的人工 QEMU/TAP 边界。
- 不修改 axnet、smoltcp、VirtIO 驱动或 kernel 网络行为。
