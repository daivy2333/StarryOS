# Evidence: 002-manual-verification-boundary

- Change: `ms02-virtio-mmio-polling-baseline`
- Iteration: `002-manual-verification-boundary`
- Captured at: `2026-07-29T19:22:01+08:00`
- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1`; product worktree modified
- Environment: WSL2 x86_64; Rust nightly-2026-02-25; offline Cargo; QEMU not executed by agent

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-002-01 | plan-required | Agent batch verification: axnet fmt PASS, deadline policy 4/4 PASS, smoltcp `auto-icmp-echo-reply` feature enabled, target kernel build PASS, MS01 harness self-test PASS, openspec validate PASS. | [build.log](build.log) | PASS |
| EV-002-02 | plan-required | Spec compliance review and code quality review of the full product diff; 0 Critical, 0 Important, 1 Minor (justified, not fixed). | [review.md](review.md) | PASS |

## Limits

- Agent-side verification covers the Rust unit test, feature graph, target
  build, MS01 harness self-test, and full diff review.
- Payload C source reviewed; user-compiled binary exists in worktree but
  is not claimed as agent evidence.
- QEMU, guest shell, packet capture, idle CPU, and MS01 runtime regression
  are user batch. User-submitted `qemu-*.log`, `*.pcap`, `idle-cpu.txt`,
  and `ms01-regression.log` are pending formal submission; worktree
  artifacts `ms02-usernet.pcap` and `tests/ms02_guest_service` are listed
  in `build.log` for traceability but not claimed by agent.
- `cargo fmt --all -- --check` reports 341 pre-existing diffs in
  `crates/smoltcp/` that are baseline state (`git diff HEAD -- crates/smoltcp/`
  is empty); axnet crate fmt is PASS.

## User Evidence Pending

Per iteration 002 `Persisted Evidence`, the following files are to be
submitted by the user after manual QEMU verification:

- `payload-build.log`
- `qemu-no-hostfwd.log`
- `qemu-usernet.log`
- `qemu-usernet.pcap`
- `qemu-tap.log`
- `qemu-tap.pcap`
- `idle-cpu.txt`
- `ms01-regression.log`

Their absence does not block the agent-side `reported` status per
iteration 002 Task Contracts.
