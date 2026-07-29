# Review: 002-manual-verification-boundary

- Change: `ms02-virtio-mmio-polling-baseline`
- Iteration: `002-manual-verification-boundary`
- Reviewer: openspec-act
- Captured at: 2026-07-29T19:22:01+08:00
- Revision: `efcf08124294d523ccab4d3569ea97fe31ed96c1` (worktree modified)

## Diff Surface

| File | Change |
|---|---|
| `Makefile` | +3 lines: `tests/ms02_guest_service` target |
| `crates/axnet/Cargo.toml` | +1 line: `auto-icmp-echo-reply` feature |
| `crates/axnet/src/device/ethernet.rs` | +4 lines: `EthernetDevice::requires_polling` |
| `crates/axnet/src/device/mod.rs` | +5 lines: `Device::requires_polling` default |
| `crates/axnet/src/service.rs` | +69/-2: `POLLING_FALLBACK`, `select_wake_deadline`, `register_waker` update, 4 unit tests |
| `tests/ms02_guest_service.c` | New file: TCP/UDP single-`poll()` payload |

No out-of-scope files touched. `git diff HEAD -- crates/smoltcp/` is empty.

## Spec Compliance Review

### R1 / R2: QEMU channel separation + MMIO baseline

- No product code claimed by agent; witness is user-side no-hostfwd and
  probe log per Runbook.
- Agent-side witness: `make LOG=info build` PASS produces the kernel image
  used for user QEMU runs.
- Status: Covered by user batch; agent-side build PASS recorded.

### R3: No-IRQ synchronous progress

- `Device::requires_polling` defaults to `false` (device/mod.rs:27). Loopback
  and IRQ devices preserve original behavior.
- `EthernetDevice::requires_polling` returns `true` only when
  `self.inner.irq_num().is_none()` (ethernet.rs:336-338). Matches D1.
- `Service::register_waker` computes `polling_deadline = Some(now + 10ms)`
  only when a mask-hit device requests polling (service.rs:101-107). Mask
  semantics match D1.
- `select_wake_deadline` takes the earlier of protocol and polling deadlines
  (service.rs:23-33). When both are `None`, returns `None`, preserving the
  no-wait path for loopback-only or already-ready sockets.
- Scenario "external frame during wait": 10ms timer wakes the waiter, which
  retries via `poll_interfaces()`. Witnessed by 4/4 deadline tests.
- Scenario "frame arrives before waiter registration": `register_waker` runs
  every poll attempt; if `now + 10ms` is already past, `sleep_until` polls
  ready and `waker.wake_by_ref()` fires immediately (service.rs:119-121).
- Scenario "network idle": bounded 10ms fallback yields at most 100
  wakeups/sec; not a busy loop. R7 user batch records CPU baseline.
- Status: PASS.

### R4 / R6: Guest service + timeout diagnosis

- `tests/ms02_guest_service.c` binds TCP listener and UDP socket on port
  5555 (MS02_PORT=5555, lines 24, 50).
- Single `poll()` loop over 3 fds: TCP listener (or -1 when client active),
  UDP socket, active TCP client (lines 151-165).
- READY marker `MS02_READY tcp=5555 udp=5555` (line 148) is unique and
  identifiable.
- TCP PASS `MS02_TCP_PASS connection=N` (line 258); UDP PASS
  `MS02_UDP_PASS datagrams=1` (line 221); COMPLETE `MS02_COMPLETE tcp=2 udp=1`
  (line 275); FAIL `MS02_FAIL stage=<s> errno=<n> message=<s>` (line 34).
- Scenario "TCP handles connection": accept -> recv until `\n` -> send
  response -> close -> accept next. `MS02_TCP_ROUND_TRIPS = 2` (line 29).
- Scenario "UDP datagram boundary": `recvfrom` captures `peer` address,
  `sendto` replies to same peer (lines 200-219). Datagram boundary
  preserved.
- Scenario "guest service not ready": host `timeout 5 nc` (user batch)
  fails boundedly; guest code does not need to handle this.
- Status: PASS for agent-side source review. Compile and runtime markers
  are user witness.

### R5: Protocol-level independent witness

- ICMP: `auto-icmp-echo-reply` feature enabled (Cargo.toml:117). Feature
  tree PASS. No raw ICMP socket syscall added.
- ARP/UDP/TCP: agent-side witness is feature tree + target build; pcap
  and runtime logs are user batch.
- Status: PASS for agent-side. User batch covers pcap.

### R7: Idle CPU baseline

- No agent-side witness (Acceptance table says `None`). User batch records
  30-second `top` output.
- Status: N/A for agent.

### R8: MS02 scope isolation

- No IRQ, PLIC, AtomicWaker, async queue task, or multi-waiter state
  introduced.
- No PCI change; `bus-mmio` remains the active transport.
- `requires_polling` is a read-only capability query; it does not spawn
  tasks or register wakers.
- `auto-icmp-echo-reply` is a smoltcp feature flag; it does not modify
  smoltcp echo implementation or kernel socket syscall.
- MS01 socket behavior preserved: `register_waker` still falls through to
  `device.register_waker(waker)` for IRQ devices, and protocol deadline
  still drives timer for sockets with pending protocol work.
- Status: PASS.

## Code Quality Review

### Makefile target

- `tests/ms02_guest_service` uses `$(BENCH_CC) -static -O2` (line 52),
  while sibling targets use `$(BENCH_CC) $(BENCH_CFLAGS)` with
  `BENCH_CFLAGS ?= -static -no-pie -fno-pie -Os -s`.
- This matches the Manual Commands in 001 and 002 iterations verbatim.
  `-O2` is acceptable for a test payload; `-no-pie -fno-pie` and `-s` are
  not required for QEMU guest execution.
- Minor: style inconsistency with sibling targets. Not fixing because
  Plan's Manual Commands explicitly specify `-static -O2`.

### axnet Cargo.toml

- `auto-icmp-echo-reply` placed between `proto-ipv6` and `socket-raw`.
  Feature list is not strictly alphabetically ordered but groups proto-*
  and socket-*; the new entry sits between groups, which is acceptable.
- No dependency added beyond the feature flag.
- Status: PASS.

### device/mod.rs

- New trait method `requires_polling` with default `false` and a doc
  comment explaining its purpose. Placed between `send` and
  `register_waker`, which is a reasonable location.
- Default `false` preserves loopback and IRQ-device behavior.
- Status: PASS.

### device/ethernet.rs

- `EthernetDevice::requires_polling` delegates to `self.inner.irq_num().is_none()`.
  Minimal and correct. Placed before `register_waker`, matching the trait
  method order.
- Status: PASS.

### service.rs

- `POLLING_FALLBACK = Duration::from_millis(10)` is a named constant, not
  a magic number. Matches D1.
- `select_wake_deadline` is a pure function with exhaustive match over
  `(Option<Instant>, Option<Instant>)`. No partial case missing.
- `register_waker` computes `polling_deadline` via
  `router.devices.iter().enumerate().any(|(i, device)| mask & (1 << i) != 0 && device.requires_polling())`.
  The `any()` short-circuits; `.then_some(timestamp + POLLING_FALLBACK)`
  yields `Option<Duration>`-shaped result correctly.
- Old `timeout` future is still dropped before creating a new one
  (service.rs:114). No leak.
- Device waker registration loop is unchanged for mask-hit devices
  (service.rs:127-131). IRQ devices still get their waker registered; the
  polling deadline only adds a timer, not a replacement.
- Four unit tests cover all four input combinations of
  `select_wake_deadline`. Tests use `Instant::from_millis_const`, which is
  const-eval friendly and deterministic.
- Status: PASS.

### tests/ms02_guest_service.c

- Single `poll()` loop, no threads, no async. Matches D2 single-waiter
  boundary.
- SIGPIPE ignored (line 137), preventing guest termination on `send` to
  closed socket.
- Resource cleanup on failure path (lines 281-286): closes all open fds.
- Buffer overflow protection: `tcp_buffer` is 256 bytes, `recv` limits to
  `sizeof - tcp_length - 1`, and `tcp_length == sizeof - 1` triggers
  `EMSGSIZE` fail (lines 260-264).
- UDP `recvfrom` allocates `buffer[MS02_BUFFER_SIZE]` on each iteration;
  acceptable for a test payload.
- `trim_line` handles both `\n` and `\r` (line 100-101), robust against
  host `nc` line ending variations.
- `MS02_TCP_RESPONSE` and `MS02_UDP_RESPONSE` include trailing `\n`
  (lines 26, 28), making `nc` output readable.
- Status: PASS.

## Cross-Task Interaction

- T1 tests (deadline policy) indirectly exercise the T2 contract via
  `select_wake_deadline`, which is the function T2's `register_waker`
  depends on.
- T3 ICMP feature does not interfere with T1/T2: it is a smoltcp feature
  flag that enables `process_icmpv4` echo reply path, independent of
  device polling.
- T4 agent batch (fmt, unit tests, feature tree, build, MS01 self-test,
  openspec validate) all PASS.
- Full diff review: no out-of-scope modifications, no orphaned code, no
  stale comments.

## Findings Summary

| Severity | Finding | Resolution |
|---|---|---|
| Critical | None | N/A |
| Important | None | N/A |
| Minor | Makefile target uses `-static -O2` instead of `$(BENCH_CFLAGS)` | Not fixing; matches Plan Manual Commands verbatim. Sibling target style divergence is acceptable for test-only payload. |

## Regression Risk

- MS01 socket behavior: `register_waker` preserves the original protocol
  deadline path. For loopback-only or IRQ-device sockets,
  `polling_deadline` is `None`, so `select_wake_deadline` returns the
  protocol deadline unchanged. MS01 self-test PASS confirms harness
  integrity; MS01 runtime regression is user batch.
- smoltcp warnings: 11 pre-existing warnings remain; no new warnings
  introduced by this change (axnet compiles clean).

## Conclusion

Spec compliance: PASS.
Code quality: PASS with 1 Minor finding (not fixed, justified).
No Critical or Important findings.
