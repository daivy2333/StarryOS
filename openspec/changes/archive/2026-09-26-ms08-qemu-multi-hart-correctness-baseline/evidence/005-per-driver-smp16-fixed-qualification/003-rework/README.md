# Evidence: 005-per-driver-smp16-fixed-qualification / 003-rework

- Change: ms08-qemu-multi-hart-correctness-baseline
- Iteration: 005-per-driver-smp16-fixed-qualification
- Cycle: 003-rework
- Captured at: 2026-09-26 14:11–14:12 (+08:00)
- Environment: QEMU `riscv-virtio,qemu` v1.4 OpenSBI；SMP=16；VirtIO-MMIO；user-net；LOG=warn；Rust nightly-2026-02-25；probe 为 6-case 最终 profile（本轮裁剪后重新构建注入）

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-005-003-01 | user-required | Fixed placement → 6.2/6.3 | SMP=16 下 16 hart schedulable，owner/runner singleton affinity 与实际 poll hart 一致且有进度；迁移历史位被 Restored 记录解释 | [guest-transcript.log](guest-transcript.log) | PASS |
| EV-005-003-02 | user-required | Direct cross-hart wake → 6.2/6.3 | timer-disabled witness：remote enqueue + IPI send/receive + resume + timer 恢复，无 missing restore | [guest-transcript.log](guest-transcript.log) | PASS |
| EV-005-003-03 | user-required | Network data path → 6.2/6.3 | TCP/UDP 严格帧双向交换，guest/peer 计数一致 | [guest-transcript.log](guest-transcript.log) + [diag.pcap](diag.pcap) | PASS |
| EV-005-003-04 | user-required | Backpressure and completion → 6.2/6.3 | Full 观测（tx_occ=64）、恢复后 inflight=0 且 submit/completion/reclaim 闭合 | [guest-transcript.log](guest-transcript.log) | PASS |
| EV-005-003-05 | user-required | Readiness and idle → 6.2/6.3 | readiness-quiet：poll 与 I/O 一致，quiet 窗口无数据面进度 | [guest-transcript.log](guest-transcript.log) | PASS |

## 结论

一次有界 `SMP=16` network-only 运行完整走完六个最终 case（placement → timer-disabled-wake → tcp-bidirectional → udp-bidirectional → full-recovery → readiness-quiet），guest 探针六个 `PASS` + `MS08_NET_HARNESS_EXIT: 0`。V5 快照全部满足判定谓词：affinity 与 last hart 一致、IPI 因果（ipi_after > ipi_before）、TX 账本闭合（submit == completion == reclaim，enq − deq == occ）、fault tuple 不变、计数器单调。host peer 独立运行已由 pcap 证实（tcp/udp/readiness/full-recovery 双向 echo 帧数与 guest SOCK 计数逐项一致）。

## 采集限制

- 用户明确不再使用 validator 对 transcript 判定（validator 属历史遗留，见 Act Response Deviations）；本证据由 guest 原生 marker/退出码 + pcap 客观流量直接支持结论。
- `peer.log` 为空（peer 在 420s 超时前被停止，Python 管道输出块缓冲未落盘）；peer 独立性由 `diag.pcap` 的双向 echo 帧承担，已删除空文件。
- boot 时有 `[NET-MIG-SMOKE] FAIL`（owner_rej=1 runner_rej=1，已知间歇竞态 B1），属受控迁移稳定性范围；最终 profile 明确不包含迁移结论，不影响本 Cycle 六 case 通过判定。
- 环境信息仅定位现场，不作为 Acceptance 证据。
