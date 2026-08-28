# qemu-runtime-markers（Iteration 008 / Cycle 000）

- Change: ms06-application-visible-async-network-stack
- 来源：用户手工 single-hart QEMU 会话的决定性 marker 摘录（按原始顺序；原始转录由用户在会话中提供）
- Revision/Environment：probe 内嵌 `832abfead57e7ae0870d5b729b6875665d588582` / `qemu-virt-riscv64-single-hart`
- 判定：**Task 7.1 MS06 GREEN 未达成（2 FAIL）**；其余回归层全部 PASS

## 1. MS06 应用见证（Task 7.1）

```text
MS06_STACK_READINESS_START
MS06_REVISION: 832abfead57e7ae0870d5b729b6875665d588582
MS06_ENVIRONMENT: qemu-virt-riscv64-single-hart
PASS: tcp-timer
PASS: udp-progress
FAIL: listener backlog connections not accepted uniquely inside deadline
PASS: nonblock-connect-error
PASS: quiet
PASS: continuous-traffic
FAIL: close-error graceful close misclassified or unstable after EOF
PASS: poll-multiwaiter
PASS: select-multiwaiter
PASS: epoll-multiwaiter
PASS: waiter-64
PASS: waiter-65-reregister
MS06_STACK_READINESS_END
```

- Objective：12 case → 实际 10 PASS + 2 FAIL；validator exit 1。
- 决定性输出（validator，首失败层）：
  `FAIL: ms06-validator: payload reported a failure: FAIL: listener backlog connections not accepted uniquely inside deadline`（exit=1）
- 采集缺口：转录中未发布 `MS06_HARNESS_EXIT`（用户运行 probe 时未附带 echo）；由 probe 已打印的 2 个 FAIL 决定失败结论，不改变判定。

## 2. MS01 兼容回归（Task 7.2）

```text
MS01_SOCKET_BASELINE_START
PASS: tcp-accept
PASS: tcp-adjacent
PASS: tcp-512cap: accepted 512 of 512 initial connections
PASS: tcp-512-recovery
PASS: tcp-relisten
PASS: udp-bidi
PASS: tcp-nonblock-accept
PASS: udp-nonblock
PASS: poll-readiness
PASS: udp-source: 127.0.0.1:49155
PASS: bind-getsockname: port 18012
PASS: bind-ephemeral: port 49698
PASS: bind-conflict: EADDRINUSE
PASS: bind-close-cleanup
MS01_SOCKET_BASELINE_END
```

- 判定：14/14 PASS；转录中未发布 `MS01_HARNESS_EXIT`。

## 3. MS04 异步 RX（Task 7.2）

- snapshot：`MS04 PASS mode=snapshot`；PRE/POST 均 `lifecycle=2 owner=1`，DELTA 全 0（irq/publish/task/descriptor/budget 均无变化）。
- idle：`MS04 PASS mode=idle`；DELTA 全 0（含 isr_publish=0 isr_wake=0 task=0）→ `MS04_HARNESS_EXIT: 0`。
- nudge：`MS04 PASS mode=nudge`；DELTA `nudge=1 task=1 empty=1`，descriptor delta 0 → `MS04_HARNESS_EXIT: 0`。
- burst：`MS04 PASS mode=burst`；DELTA `isr_publish=2 isr_wake=2 task=5 reaped=96 refilled=96 delivered=96 budget=3 yield=2 fault=0` → 96 包守恒、budget/self-yield 推进、fault=0 → `MS04_HARNESS_EXIT: 0`。

## 4. MS05 双向数据面（Task 7.2）

- snapshot：`MS05 PASS mode=snapshot`；DELTA 全 0 → `MS05_HARNESS_EXIT: 0`。
- tx-only 96 64：`MS05 PASS mode=tx-only`；WITNESS `sent=96 received=96` → `MS05_HARNESS_EXIT: 0`。
- bidirectional 96 64：首次 `FAIL reason=handshake`（host stimulus 未先启动，R56 已记录的操作顺序问题）→ 重跑 `MS05 PASS mode=bidirectional`；WITNESS `tx_sent=96 rx_received=96 host_received=96` → `MS05_HARNESS_EXIT: 0`。
- slot-full：两次 `FAIL reason=handshake` → 第三次 `MS05 PASS mode=slot-full`；`FULL tx_occ=64 tx_full=1` → `RELEASED` → `POST tx_occ=0 tx_enq=481 tx_deq=481 live=0` 闭合；WITNESS `sent=96 host_received=96` → `MS05_HARNESS_EXIT: 0`。
- descriptor-full：两次 `FAIL reason=handshake` → 第三次 `MS05 PASS mode=descriptor-full`；`FULL buf_avail=0 inflight=64 desc_avail=0 desc_inflight=64` → `POST buf_avail=64 inflight=0` 闭合；WITNESS `sent=96 host_received=96` → `MS05_HARNESS_EXIT: 0`。
- flush：首次 `FAIL reason=handshake` → 重跑 `MS05 PASS mode=flush`；DELTA `flush_ok=1 flush_err/busy/cancel=0`；WITNESS `sent=96 host_received=96` → `MS05_HARNESS_EXIT: 0`。

## 5. Artifact 身份（前后核对）

| Artifact | 会话前 size/mtime | 会话后 size/mtime | 变化 |
|---|---|---|---|
| `StarryOS_riscv64-qemu-virt.bin` | 40,763,584 / 20:08:19 | 40,763,584 / **21:13:42** | mtime 变化（用户经 `make run` 启动时重建）；bytes 不变 |
| `tests/ms01_socket_baseline` | 155,272 / 20:08:30 | 155,272 / 20:08:30 | 无 |
| `tests/ms04_rx_probe` | 134,232 / 20:08:30 | 134,232 / 20:08:30 | 无 |
| `tests/ms05_data_plane_probe` | 149,528 / 20:08:31 | 149,528 / 20:08:31 | 无 |
| `tests/ms06_stack_readiness_probe` | 147,024 / 20:08:31 | 147,024 / 20:08:31 | 无 |

- MS06 probe 内嵌 revision 仍为 `832abfead…`，validator 期望匹配；exact-binary 声明受影响项仅为 boot bin（Runbook 变更将 QEMU 启动改为 `make run`，见 5 节与文档变更记录）。

## 6. 结论

- Task 7.1（MS06）：**FAIL** —— 2/12 case 未达（listener backlog 唯一接受、close-error/EOF 分类），validator exit 1。
- Task 7.2：MS01 14/14、MS04 4/4、MS05 6/6 全部 PASS（MS05 handshake 重跑为 stimulus 未先启动的操作顺序问题，按 R56 处理；无产品 FAIL）。
- 第一失败层：MS06 guest probe 的 listener backlog case。后续 Acceptances 5/6 涉及的 identity 与二分回归不因 MS06 FAIL 而撤销，但 Task 7.1 GREEN 未达成，需 Plan 创建修复 Cycle。