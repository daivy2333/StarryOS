# Evidence: 005-runtime-readiness-closure-and-manual-handoff

- Change: `ms16-qemu-polling-network-performance-baseline`
- Iteration: `005-runtime-readiness-closure-and-manual-handoff`
- Captured at: 2026-08-05 Asia/Shanghai
- Revision: `2a9319a946dbe9c07cb0f448d82c0b7c14069015` with uncommitted worktree
- Environment: x86_64 sandbox、C11 host、manual external/QEMU boundary

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-005-01 | plan-required | T1-T4 RED/GREEN 与 host/local/sanitizer Gates 可重复 | [runtime-readiness.log](runtime-readiness.log) | PASS |
| EV-005-02 | plan-required | 产品回归、target build、OpenSpec 和 diff Gate | [product-regression.log](product-regression.log) | PASS with recorded warnings |
| EV-005-03 | user-required | guest artifact freshness and external preflight | [external-handoff.md](external-handoff.md) | PASS |
| EV-005-04 | user-required | N00 first attempt reached guest network but used an invalid guest directory | [qemu-n00-attempt-1.md](qemu-n00-attempt-1.md) | FAIL / RETAINED |
| EV-005-05 | user-required | N00-N03 rerun validates artifact, calibration, TCP local path and ICMP; UDP ledger gap found | [qemu-n00-n03-attempt-2.md](qemu-n00-n03-attempt-2.md) | PARTIAL / RETAINED |
| EV-005-06 | user-required | Fresh guest artifact closes UDP ledger gap and completes N00-N03 | [qemu-n00-n03-attempt-3.md](qemu-n00-n03-attempt-3.md) | PASS |
| EV-005-07 | user-required | User-net six-direction smoke reaches all endpoint paths; one valid and five diagnostic invalid rounds retained | [qemu-usernet-smoke-attempt-1.md](qemu-usernet-smoke-attempt-1.md) | PARTIAL / RETAINED |

N00 首次失败与 UDP ledger 失败均已保留。N00-N03 已通过。user-net 六方向已执行，但只有 TCP TX valid；TAP、sudo 和 calibration pcap 尚未执行。
