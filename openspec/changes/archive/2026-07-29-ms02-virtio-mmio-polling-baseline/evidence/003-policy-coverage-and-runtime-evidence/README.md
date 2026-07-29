# Evidence: 003-policy-coverage-and-runtime-evidence

- Change: `ms02-virtio-mmio-polling-baseline`
- Iteration: `003-policy-coverage-and-runtime-evidence`
- Captured at: 2026-07-29T20:30:00+08:00 (agent); 2026-07-29T20:38:00+08:00 (user)
- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1`; product worktree modified
- Environment: WSL2 x86_64; Rust nightly-2026-02-25; offline Cargo; QEMU manual

## Agent Evidence

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-003-01 | plan-required | Policy coverage: 4/4 original deadline tests + 4/4 new mask×polling eligibility tests = 8/8 PASS. Refactor preserves baseline GREEN. | [policy-tests.log](policy-tests.log) | PASS |
| EV-003-02 | plan-required | Agent batch verification: axnet fmt PASS, smoltcp `auto-icmp-echo-reply` feature enabled, target kernel build PASS, MS01 harness self-test PASS, openspec validate PASS, git diff --check clean. | [build.log](build.log) | PASS |
| EV-003-03 | plan-required | Spec compliance review and code quality review of the T1 diff; 0 Critical, 0 Important, 0 Minor. | [review.md](review.md) | PASS |

## User Evidence

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-003-04 | user-required | Payload compiled: `tests/ms02_guest_service`, SHA-256 `c2a252f9...877b3`, RISC-V static binary, `-static -O2`. | [payload-build.log](payload-build.log) | PASS |
| EV-003-05 | user-required | No-hostfwd QEMU: serial shell reached, virtio-net probed at PA:0x10007000, virtio-blk probed, eth0 created with MAC 52-54-00-12-34-56 and IP 10.0.2.15/24. No hostfwd configured. | [qemu-no-hostfwd.log](qemu-no-hostfwd.log) | PASS |
| EV-003-06 | user-required | User-net QEMU with hostfwd tcp/udp 5555: guest service started (MS02_READY), TCP PASS (MS02_TCP_RESPONSE received), UDP PASS (MS02_UDP_RESPONSE received). ARP request/reply in pcap. TCP 5555 handshake+data+close in pcap. | [qemu-usernet.log](qemu-usernet.log), [qemu-usernet.pcap](qemu-usernet.pcap) | PASS |
| EV-003-07 | user-required | TAP QEMU: ARP request who-has 10.0.2.15 + reply (MAC 52:54:00:12:34:56). 6/6 ICMP echo request/reply pairs (2 rounds of ping -c 3). 0% packet loss. | [qemu-tap.log](qemu-tap.log), [qemu-tap.pcap](qemu-tap.pcap) | PASS |
| EV-003-08 | user-required | Idle CPU baseline: 30-second `top` sampling, QEMU PID 139278, CPU 100-111% (single core), polling fallback expected behavior. No threshold set. | [idle-cpu.txt](idle-cpu.txt) | PASS |
| EV-003-09 | user-required | MS01 runtime regression: 14/14 PASS (tcp-accept, tcp-adjacent, tcp-512cap, tcp-512-recovery, tcp-relisten, udp-bidi, tcp-nonblock-accept, udp-nonblock, poll-readiness, udp-source, bind-getsockname, bind-ephemeral, bind-conflict, bind-close-cleanup). No FAIL. | [ms01-regression.log](ms01-regression.log) | PASS |

## Limits

- Agent-side verification covers the Rust unit test (8/8), feature graph,
  target build, MS01 harness self-test, openspec strict validation, and
  full diff review.
- User-side verification covers payload compile, no-hostfwd boot, user-net
  TCP/UDP, TAP ARP/ICMP, idle CPU, and MS01 runtime regression.
- `cargo fmt --all -- --check` reports 341 pre-existing diffs in
  `crates/smoltcp/` that are baseline state (`git diff HEAD -- crates/smoltcp/`
  is empty); axnet crate fmt is PASS.
- UDP 5555 not captured in user-net pcap (hostfwd UDP may not generate
  filter-dump entries); however host-side `nc -u` received `MS02_UDP_RESPONSE`
  confirming UDP path works.

## All Required Evidence Submitted

All plan-required Evidence files are present. Gate 5 is unblocked.
