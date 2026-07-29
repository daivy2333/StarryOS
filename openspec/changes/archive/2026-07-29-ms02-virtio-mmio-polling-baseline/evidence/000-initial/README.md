# Evidence: 000-initial

- Change: `ms02-virtio-mmio-polling-baseline`
- Iteration: `000-initial`
- Captured at: `2026-07-29T17:55:19+08:00`
- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1`; change directory untracked
- Environment: WSL2 x86_64; Rust nightly-2026-02-25; QEMU not executed

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-000-01 | plan-required | Gate 3 axnet host test did not enter the test body because `libc 0.2.182` was absent from the local cache and the restricted environment could not resolve `static.crates.io`. | [tdd-policy.log](tdd-policy.log) | BLOCKED |
| EV-000-02 | act-added | The blocker occurred before product edits; T1-T4 and all manual QEMU evidence remain unstarted. | [blocker.md](blocker.md) | BLOCKED |

## Limits

- The successful feature-tree query only establishes that
  `auto-icmp-echo-reply` is absent from the current dependency graph.
- No RED policy test was added or observed because the mandatory host test
  baseline failed before the test body.
- No QEMU, guest shell, packet capture, or CPU sampling operation was run.
- Plan-required build, QEMU, pcap, CPU, and MS01 evidence files were not
  created because their tasks were not reached.
