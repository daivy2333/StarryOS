## 1. Contract Change

> Status 2026-06-23: implemented. Code changes complete — 5 files modified across 2 repos. uart_16550 cargo check + test all pass (54 tests), StarryOS cargo check passes.

- [x] 1.1 Change `uart_16550::TtyWrite::write` to return `usize`
- [x] 1.2 Update `AsyncUartWriter` TTY impl to return `RingBufTx::push`
- [x] 1.3 Update StarryOS `Tty::write_at` to return the writer count
- [x] 1.4 Update `PtyWriter` to return the accepted count
- [x] 1.5 Make ldisc echo explicitly ignore best-effort write counts

## 2. Witness Updates

- [x] 2.1 Update benchmark write helpers to loop on short writes
- [x] 2.2 Ensure write+tcdrain paths still measure complete transmission

## 3. Verification

- [x] 3.1 Run `cargo check --features async` in uart_16550
- [x] 3.2 Run `cargo test --features async` in uart_16550
- [x] 3.3 Run StarryOS kernel check/build
- [x] 3.4 Run QEMU benchmark/manual QA if build succeeds
  - QEMU Shell 启动正常，交互正常
  - benchmark 全部通过：TX Throughput 64B 210KB/s, 1B avg 0.134ms, FIONBIO PASS
  - 无 10ms 调度台阶回归
  - 延迟矩阵与 pre-M4 基线持平（±8% 以内）
