# User-net six-direction smoke attempt 1

- Source: user-submitted manual record, 2026-08-05
- Raw log: [qemu-usernet-smoke-attempt-1.log](qemu-usernet-smoke-attempt-1.log)
- Raw SHA-256: `9d4fb503ca492a8b1a78dfe4df63264fce15da296e3a2105a49712285ec7dada`
- Guest artifact: `b863b060500c3a0977102e840d2d7160d75d7ea899567ab38cc72891ad5f1eb3`
- Topology: QEMU user-net；hostfwd 5555；guest 出站到 `10.0.2.2:15555`

原始记录含 12 个 manifest、12 个 round 和 45 个 CPU sample。六组双端 fingerprint 均一致。

| Test | Execution | Round | Result |
|---|---|---|---|
| TCP RX 201 | PASS | 双端 invalid partial | host 9964 TX；guest 7788 RX |
| UDP RX 202 | PASS | 双端 invalid partial | host 60822 TX；guest 27 late；15 次 pending buffer full warning |
| TCP TX 203 | PASS | 双端 valid | 4702 packets、6582800 payload bytes |
| UDP TX 204 | PASS | 双端 invalid partial | guest 4819 TX；host 4812 RX、7 late |
| TCP BIDI 205 | PASS | 双端 invalid partial | guest TX 1335 与 host RX 1335 闭合；反向未闭合 |
| UDP BIDI 206 | PASS | 双端 invalid partial | 双向有流量；账本未闭合 |

TCP TX 是本次唯一有效性能 round。其余五组只证明建连、传输、记录和失败分类可执行，不能用于吞吐对比。

IRQ probe 存在。idle window 为 2000 ms，total、used-ring 和 ack delta 均为 0，结果 `PASS idle`。

Collector exit 0，每个 scope 各有 15 个 sample。peer 的 user/system ticks 全程为 0，因此采样未覆盖有效负载，不能计算 CPU/GiB。

本记录不含 TAP、独立 pcap、完整 QEMU 启动日志或 standard B0。`/tmp` 原始文件已随环境重启消失；本 raw log 是现存来源。
