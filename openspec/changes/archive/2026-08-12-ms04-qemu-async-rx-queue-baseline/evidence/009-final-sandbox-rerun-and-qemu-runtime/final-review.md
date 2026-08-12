# Iteration 009 Final Review

- Status: PASS WITH EXPLICIT USER WAIVER
- Review revision: `8f5b5228747dc817a5a9de7a3461dccdf06e0c24` plus staged/worktree iteration 009 diff
- Runtime scope: single-hart QEMU virt with one VirtIO-MMIO NIC and user-net
- Unresolved findings: Critical 0; Important 0; Minor 0

## Evidence completeness

T7.3R provenance and raw-log integrity checks pass. T8.1 passes after the authorized one-line
`<sys/time.h>` fix, permanent source guard and clean external rebuild. The first failure and clean
rerun are both preserved.

The supplied 5,728-byte CRLF serial log contains the guest workload excerpt, not boot or session
termination. Its SHA-256 is
`186ad7c39dc13831892faf2459a5071c7455244bcb66a8662c288062e859a4e7`.

## Runtime review

MS04 passes all four supplied modes:

- snapshot: lifecycle 2, owner 1 and all safety/fault counters zero.
- idle: zero IRQ, software, task, descriptor, budget and fault progress.
- nudge: exactly one software nudge, one task poll and one empty check; no descriptor progress.
- burst: 96 reaped, 96 refilled and 96 delivered; ISR publish/wake each advance once; budget
  exhaustion and self-yield each advance twice; Router full and space wake each advance once.

MS03 idle, UART isolation, rx2 and tx2 pass. RX advances used/ACK by 3; TX advances used/ACK by 1.
MS02 TCP connection 1 passes. No `FAIL`, fatal or panic marker appears.

## Explicit waiver and claim boundary

User instruction: “当前没有遇到fatal，至于少的几个测试我都觉得没必要重复进行了，我授权的，
你看看，没有问题就填写回复”。The following are `WAIVED/SKIPPED`, not PASS:

- boot UART IRQ 10, VirtIO-MMIO validation and IRQ 7 startup lines;
- MS03 `both` and repeat `rx2`;
- MS02 TCP connection 2, UDP and `MS02_COMPLETE`;
- MS01 14/14 socket regression;
- post-regression final MS04 snapshot and session termination metadata.

The accepted conclusion is the MS04 core single-hart VirtIO-MMIO asynchronous RX runtime baseline.
The Evidence does not support a complete MS01/MS02/MS03 compatibility or post-regression safety claim.

## Full-range and scope review

The iteration-owned product diff is limited to the direct timeval include and its permanent source
guard. Host harness 15/15, C decisions 10/10, strict host syntax, external static builds, OpenSpec
strict validation and non-Evidence whitespace checks pass. No unreviewed ownership, ISR, descriptor,
protocol or runtime-verdict change was introduced. No unresolved Critical, Important or Minor finding
remains.
