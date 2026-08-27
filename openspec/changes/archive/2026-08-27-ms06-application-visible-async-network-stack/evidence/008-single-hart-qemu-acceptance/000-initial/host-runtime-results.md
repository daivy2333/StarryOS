# host-runtime-results（Iteration 008 / Cycle 000）

- Change: ms06-application-visible-async-network-stack
- 来源：用户 host 侧手工执行 MS04/MS05 stimulus 的决定性结果（会话转录提供）
- 覆盖：Acceptance 3-4（MS04 host 15556、MS05 host 15557 双边闭合）

## 1. MS04 host stimulus（port 15556）

```text
python3 scripts/ms04_rx_stimulus.py --host 0.0.0.0 --port 15556
ms04 stimulus: PASS packets=96 payload=64
```

- 与 guest `MS04 PASS mode=burst`（reaped/refilled/delivered=96，fault=0）双边闭合：96 包守恒，host 侧确认收到 96。

## 2. MS05 host stimulus（port 15557，每模式一次 exchange）

```text
python3 scripts/ms05_data_plane_stimulus.py --port 15557
ms05 stimulus: PASS mode=tx-only count=96 payload=64 received=96

python3 scripts/ms05_data_plane_stimulus.py --port 15557
ms05 stimulus: PASS mode=bidirectional count=96 payload=64 received=96

python3 scripts/ms05_data_plane_stimulus.py --port 15557
ms05 stimulus: PASS mode=descriptor-full count=96 payload=64 received=96

python3 scripts/ms05_data_plane_stimulus.py --port 15557
ms05 stimulus: PASS mode=slot-full count=96 payload=64 received=96

python3 scripts/ms05_data_plane_stimulus.py --port 15557
ms05 stimulus: PASS mode=flush count=96 payload=64 received=96
```

- 各 mode host `received=96` 与 guest `MS05 PASS mode=<m>` 闭合；snapshot 为纯快照一致性模式，无需 host exchange。
- 备注：这 5 次 stimulus 为成功 exchange 的运行；guest 侧 early 的 `reason=handshake` FAIL 是 stimulus 未先启动的操作顺序问题（R56 已记录：`request=handshake` 多为 host stimulus 未先启动），重跑后全部闭合，host 侧无真实 FAIL。
- 采集方式：用户直接运行未过 `tee`（无 `$EV/*-host.log` 文件）；决定性输出以上述转录为准，写入本文件。

## 3. 结论

- MS04 burst guest/host 双侧闭合：PASS。
- MS05 六 mode guest/host 双侧闭合：PASS（host `received=96` 全部对齐）。
- 与 qemu-runtime-markers.md 的 MS06 FAIL 无关；但 Task 7.1 GREEN 未达成，Cycle 需 Plan 后续处理。