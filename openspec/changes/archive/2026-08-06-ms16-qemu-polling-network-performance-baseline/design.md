## Context

MS01 至 MS03 已证明 socket、VirtIO-MMIO 轮询收发和诊断 IRQ 可用。现有用例只覆盖功能，没有统一性能协议或 B0 数据集。

[MS02 guest service](../../../tests/ms02_guest_service.c) 已证明单进程 `poll()` 可管理 TCP 与 UDP。[UART benchmark](../../../tests/benchmark.c) 已提供 manifest、单调时钟、严格 `/proc/instret`、多轮样本和无效轮次保留模式。[MS03 probe](../../../tests/ms03_irq_probe.c) 已提供固定 IRQ snapshot ABI。

网络数据面保持不变。当前关键边界如下：

- [axnet constants](../../../crates/axnet/src/consts.rs) 固定 TCP/UDP 64 KiB、ARP pending 32 和 MTU 1500。
- [Router](../../../crates/axnet/src/router.rs) 使用 64 packet RX/TX buffer。
- registry `VirtIoNetDev` 使用 64 entry queue、128 个 1526 B buffer。
- [QEMU rules](../../../make/qemu.mk) 支持 user、tap、filter-dump 和 `ICOUNT`。
- [QEMU Runbook](../../../.claude/runbooks/qemu-network-testing.md) 要求人工启动 QEMU 和输入 guest 命令。

2026-08-04 的调查基线位于 revision `2a9319a946dbe9c07cb0f448d82c0b7c14069015`：

| Command | Result | Exit |
|---|---|---:|
| `make host-test` | early console 6/6、memtrack 8/8、MS03 IRQ 20/20 | 0 |
| `cargo test --manifest-path crates/axnet/Cargo.toml --locked --offline --lib service::tests -- --nocapture` | 8/8 | 0 |
| `python3 scripts/ms01-qemu-test.py --self-test` | harness self-test PASS | 0 |
| 两份现有网络 C payload 严格语法检查 | PASS | 0 |
| `make LOG=info build` | QEMU target release build 完成 | 0 |

target build 曾尝试安装 cargo-binutils。只读 home 和受限网络使安装失败，但已有工具继续完成构建。该输出属于环境噪声。

## Goals / Non-Goals

**Goals:**

- 固定 host、guest 和未来真板共用的 workload 协议。
- 固定 C1-C6 完成点、指标公式和 profile。
- 生成可机器校验的原始记录、manifest 和摘要。
- 完成 user-net smoke、TAP 校准和 polling B0。
- 让 MS04 原样复用 B0 形成 A/B。

**Non-Goals:**

- 修改 axnet、smoltcp、VirtIO 或 kernel 网络行为。
- 新增内部 queue、copy、descriptor 或 scheduler telemetry。
- 自动驱动 QEMU console。
- 执行 netem、长稳、SMP、多队列或真板 profile。
- 设置没有 B0 方差依据的性能阈值。

## Decisions

**D1：一个 change，三个 capability-separated 批次**

批次依次为工具与协议、QEMU 校准、正式 B0。每个批次完成后由 Plan Review 决定下一 iteration。

原因是 QEMU console 和 TAP 运行属于用户能力边界。单个超长 iteration 会把可由 agent 完成的工具工作与人工 Evidence 混合。

影响是 `000-initial` 只交付协议、Schema、工具骨架和 host 测试。后续 iteration 才要求运行 Evidence。

替代方案是拆成三个 change。该方案会让尚未形成稳定成果的中间件产生额外归档和同步成本，因此不采用。

**D2：单个 portable C 程序使用 `poll()`，不使用线程**

`tests/network_benchmark.c` 同时支持 server、client 和内建 loopback。多流、双向与控制连接由一个 event loop 管理。

原因是 StarryOS 已验证同进程 `poll()`，但没有为本 change 验证 pthread 调度和多进程协调。单 event loop 也符合当前单 waiter 边界。

影响是每个 fd 都有明确状态、deadline 和字节账本。任何连续 5 秒无进展使 round invalid；握手和 summary 默认各有 10 秒 timeout。

替代方案是每流一个线程或进程。它会引入调度、CPU 归因和多 waiter 变量，因此不采用。

**D3：控制面使用 framed TCP，数据面按测试使用 TCP 或 UDP**

默认端口为 5555。TCP listener 的首个连接承载控制消息，后续连接承载 flow；UDP 使用同一数字端口。

控制消息包括 `HELLO`、`READY`、`START`、`CANCEL`、`SUMMARY` 和 `ERROR`。每个 frame 包含 magic、版本、类型、body 长度、run、test、round 和配置 fingerprint。

所有整数显式按 network byte order 编解码。禁止发送原始 C struct。

原因是 UDP 控制消息会引入丢失和重传状态。TCP 控制连接可以为 TCP 与 UDP workload 提供一致屏障和摘要。

替代方案是文本控制协议。文本解析简单，但难以固定长度、上限和兼容错误，因此不采用。

**D4：payload 使用冻结的确定性生成器和 CRC32**

双方按 seed、flow、sequence 和 offset 生成 payload。每个 TCP record 和 UDP datagram 都包含长度、序号和 CRC32。

配置 fingerprint 使用 canonical 配置字符串的 64-bit FNV-1a。Evidence manifest 和文件仍由 host 计算 SHA-256。

原因是 C 程序不能依赖额外 crypto 库。CRC32 足以检测传输损坏，FNV fingerprint 只用于快速配置一致性，不承担安全用途。

替代方案是链接 OpenSSL 或实现 SHA-256。它会扩大 guest payload 和依赖，不采用。

**D5：原始输出使用受限 NDJSON Schema v1**

程序每行输出一个对象。字段名和 enum 值固定；错误文本使用数值原因码，避免任意字符串转义。

记录分为 manifest、capability、event、round 和 summary。每条记录包含 side、platform、driver mode、protocol、direction、completion point 和 config hash。

原因是 NDJSON 可流式保存，单行损坏不会使全部日志不可解析。Python stdlib 可以无依赖处理。

替代方案是自由 `key=value`。它缺少稳定类型和嵌套结构，不采用为规范格式。

**D6：workload 状态机固定完成点和 pacing**

client 提交配置，server 校验后返回 READY。双方依次进入 warm-up、measurement、receiver validation 和 SUMMARY。

TCP goodput 只使用 C6 字节。UDP receiver 维护唯一序号集合与 highest-seen，分别统计 loss、duplicate、reorder、corrupt 和 late。

UDP sender 使用 `clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME)` pacing。若落后超过一个 interval，scheduler 从当前时间重建 deadline，并增加 `pacing_resync`，禁止无界追赶 burst。

SIGINT 或 SIGTERM 设置取消标志。控制连接可用时发送 CANCEL；双方保留 invalid round。

替代方案是 busy wait pacing。它会污染 CPU 指标，不采用。

**D7：平台适配只读取现有接口**

`tests/network_benchmark_platform.h` 统一 monotonic clock、instret、IRQ snapshot 和 capability。host 不支持的 guest 指标返回 unavailable。

guest 单 hart读取 `/proc/instret`，沿用 UART benchmark 的严格 parser 和连续读取开销。MS03 snapshot 复用 ioctl `0x4e49_4431`，不得复制新 ABI。

QEMU CPU 由 `scripts/network-benchmark-collect.py` 读取指定 PID 的 `/proc/<pid>/stat` 和 `/proc/<pid>/status`。QEMU、peer 与 collector 分开记录。

替代方案是修改 kernel 增加 CPU 或 NIC telemetry。该方案会扰动 B0，且超出获批范围。

**D8：报告、采集和 Evidence 检查使用 Python stdlib**

新增三个脚本：

- `scripts/network_benchmark_collect.py`：host PID CPU/RSS 采样。
- `scripts/network_benchmark_report.py`：NDJSON 校验、逐轮指标和摘要。
- `scripts/network_benchmark_evidence.py`：文件、字段、账本、round 和 comparison key 检查。

测试使用 `unittest` 和 `tests/fixtures/network-benchmark/`。fixture 同时包含有效、配置不一致、缺文件、无效 round 和不可比 A/B。

原因是当前环境已有 Python 3.10，没有 `iperf3` 或 `pidstat`。stdlib 可以避免新增依赖。

替代方案是把采集和报告放入 C peer。它会把 host 专属逻辑带入 guest 程序，不采用。

**D9：Makefile 提供 host、guest 和 host-test 三类入口**

新增 host binary、RISC-V static guest binary和聚合 host-test target。guest 使用现有 `BENCH_CC`；host 使用 `CC`。

host-test 先运行独立 protocol/platform C tests，再运行 benchmark self-test 和 Python fixture tests。首个 RED 是目标或头文件缺失，不以产品回归作为新功能 RED。

替代方案是只在文档保存编译命令。它不能形成稳定 Gate，不采用。

**D10：正式 B0 只接受 TAP required Evidence**

user-net 只执行 smoke。TAP B0 使用 `ICOUNT=n`、固定 SMP、MTU、affinity、offload 和 vhost。

正式目录保存 manifest、完整 QEMU 命令、serial、guest/host NDJSON、host CPU、IRQ snapshots、pcap、results、summary 和 evidence-check。

报告器根据受控字段生成 comparison key。只有 `treatment` 可以变化。QEMU 与真板属于不同比较域。

替代方案是允许 README 声明替代缺失文件。MS03 已出现声明与文件不一致，因此不采用。

**D11：foundation 采用可证明边界的 wire 与 Schema 契约**

Iteration 000 Review 发现，协议与工具测试没有覆盖最小缓冲区、非对齐 offset、完整指标和账本失败。进入 socket workload 前，foundation 必须满足以下契约。

Wire 格式：

- frame header 固定 8 B：magic 4 B、version 1 B、type 1 B、body length 2 B。
- 每个控制 frame 的 body 先放 24 B 公共前缀：run ID 8 B、test ID 4 B、round ID 4 B、config fingerprint 8 B。
- `HELLO` body 固定 48 B：公共前缀 24 B、role 1 B、capability bitmap 8 B，以及 protocol、direction、flow、payload、duration、warm-up、seed、offered load 和 Nagle 共 15 B。
- `READY`、`START`、`CANCEL` body 固定 24 B，只使用公共前缀。
- `SUMMARY` 指标区固定 100 B，因此 body length 为 124 B。`ERROR` 保存数值 reason 和 mismatch bitmap，不发送任意错误文本。
- `ERROR` body 固定 36 B：公共前缀 24 B、reason 2 B、reserved 2 B、mismatch bitmap 8 B。
- record header 固定 28 B。data record 固定区为 36 B，payload 上限等于 `NB_DATA_RECORD_MAX - 36`。
- encode 先用实际 wire size 检查容量。decode 在读取字段前检查完整长度，并拒绝类型长度不符、截断和 exact API 的 trailing bytes。
- TCP 流式调用先使用只读长度探测，再把完整单帧交给 exact decoder。长度探测不得复制 body 或修改目标对象。

Generator 与 fingerprint：

- generator 的字节顺序固定，不依赖 host endian。任意 offset 的分段结果必须与一次连续生成相同。
- config fingerprint 不包含本端 role、capability、platform 或 treatment。双方 role 必须互补，其余 workload 字段必须一致。
- canonical 输入字段为版本、test、protocol、direction、flow、payload、duration、warm-up、seed、offered load 和 Nagle 设置。

平台计数：

- 单次 instret 读取只返回一个原始计数。workload 分别保存 begin 与 end。
- 连续两次读取的校准值使用第二次减第一次，单位为 instruction，不得使用 wall time 代替。
- host 的 instret 与 IRQ 返回 `unavailable`。解析、回退、溢出和 counter regression 使用注入数据测试。

Schema 与工具：

- 每条 NDJSON 记录包含 `schema_version=1`、type、run、test、round、side、platform、driver mode、completion point 和 config fingerprint。
- malformed JSON、未知版本、重复 endpoint round、缺失 peer、状态不一致和账本不闭合使数据集失败。诊断保留文件、行号和数值 reason。
- goodput 与 PPS 使用 receiver C6 字节、包数和 receiver duration。RTT 分位数与 delay variation 从原始 RTT samples 计算，禁止聚合已经汇总的分位数。
- UDP 五类错误、QEMU CPU seconds、core equivalents、CPU seconds/GiB 和 guest instructions/bit、byte、packet、syscall 均需输出；缺失 capability 保持 `unavailable`。
- collector 记录 scope、PID、进程 starttime、`CLK_TCK`、monotonic timestamp、CPU ticks 和 RSS。PID 消失、PID 重用和 counter regression 产生数值状态。
- Evidence checker 提供 `foundation` 与 `b0` profile。`b0` 要求 D10 的全部文件，核对 manifest hash、双端 round set、TCP 精确账本、UDP 分类账本和 summary 重建。
- comparison key 覆盖 benchmark、kernel、rootfs、backend、MTU、offload、vhost、TAP、QEMU、machine、SMP、memory、icount、affinity、payload、flow、duration、seed、completion point、queue、socket buffer、telemetry 和日志级别。只有 treatment 可变。

## State and Data Flow

```text
CLI + profile
  -> canonical config + fingerprint
  -> TCP HELLO/READY
  -> warm-up
  -> START barrier
  -> TCP/UDP data event loop
  -> receiver sequence + CRC validation
  -> C6 SUMMARY
  -> NDJSON on both sides
  -> host collector NDJSON
  -> report CSV/JSON
  -> Evidence checker
```

每个 flow 的状态只由本进程 event loop 修改。控制连接拥有 round 状态；data fd 只拥有本 flow 的 sequence、buffer 和 deadline。

## Risks / Trade-offs

- [单 event loop 限制并行度] → MS16 测量当前单 hart 基线；多 hart 属于 N53。
- [NDJSON `printf` 干扰 guest] → 测量窗口只累计内存计数，窗口结束后输出。
- [CRC32 占用指令] → B0/A1 使用同一版本；loopback 提供上层控制。
- [host collector 采样扰动] → 固定采样周期与 affinity，并单独记录 collector CPU。
- [TAP 需要权限] → 权限失败记录为 capability block，不切换到 user-net headline。
- [rootfs payload 版本漂移] → 保存 binary SHA-256、编译命令和下载路径。
- [现有 socket buffer setter 无效] → 记录固定 64 KiB，不声称完成 BDP tuning。
- [当前 guest CPU accounting 不可靠] → 只用 host CPU 和单 hart instret；其他字段 unavailable。

## Migration Plan

1. Iteration 000 提交协议、Schema、host tools、fixtures 和构建 Gate 的初版。
2. Iteration 001 修复 Review 发现的边界与契约缺口，关闭 foundation tasks。
3. Foundation Review 通过后建立 guest workload 与 loopback/user-net/TAP 校准 iteration。
4. 校准通过后建立 standard profile B0 iteration，并保存 required Evidence。
5. Review 根据 B0 方差制定后续 A/B 回归阈值。

回滚只删除新增 benchmark、脚本、fixtures 和 Makefile target。产品网络代码没有迁移或数据变更。

## Open Questions

None.
