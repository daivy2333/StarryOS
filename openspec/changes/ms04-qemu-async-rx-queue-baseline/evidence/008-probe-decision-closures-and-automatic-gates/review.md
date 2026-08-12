# Iteration 008 Review

Result: PASS. The review covered the complete change range from
`16d9a16a2b65a574022faaee39b465f6f7aebd45` through Act HEAD
`78e1f7abfa1614c188a24ebe7150ffb7c71e46d0` plus the working-tree diff.
Unresolved findings: Critical 0, Important 0, Minor 0.

## Specs versus code

- The ISR path records raw telemetry, acknowledges the device and publishes work;
  descriptor reaping and protocol delivery remain in task context.
- The async RX lifecycle retains one queue task and waker. Polling/Async ownership,
  budget 32, register-then-recheck and Router-full backpressure agree with the delta
  spec and design.
- V1 remains 64 bytes. V2 remains 224 bytes and reports software nudge separately.
  EVENT_IDX remains in supported features and its negotiated bit reaches both queues.
- The probe now rejects nonzero boot-history safety counters, applies exact idle and
  nudge allow-lists, checks the deadline before accepting an equal snapshot, and
  emits terminal markers through the two centralized marker helpers.
- The production UDP stimulus path is exercised by the bounded loopback self-test;
  this sandbox refused socket creation before protocol execution, so that exact
  command remains a T8.1 handoff.
- Synchronous TX and ordinary protocol polling were not changed. No QEMU runtime,
  rootfs or guest-shell work was performed.

## Full-range review

The range includes repository-local copies of axdriver_net, axdriver_virtio and
virtio-drivers. They were compared with their registry sources. Change-owned edits
are the queue-control interface, RX EVENT_IDX behavior, network adapter wiring and
tests; the remaining copied source and prior compiler/test-lifetime compatibility
edits were reviewed separately. Production `unsafe`, panic, unwrap and TODO sites
were scanned. Newly relevant unsafe operations are confined to the already reviewed
VirtIO/MMIO boundaries; no unreviewed unsafe path was introduced by iteration 008.

Warnings in the automatic logs come from unchanged copied vendor/smoltcp code and
the deprecated user-level Cargo config path. No iteration-owned warning remains.

## Findings resolved during Act

1. Important: idle/nudge validation initially omitted raw V1 IRQ counters from the
   exact forbidden-progress matrix. Added `irq_delta_quiet` and per-field mutation
   coverage; the C decision suite passes 10/10.
2. Important: the burst failure path emitted a terminal marker directly. Routed all
   recognized modes through `finish_mode`/`fail_mode` and added a source guard; the
   host harness passes 14/14.
3. Minor: the three scoped VirtIO files failed directed rustfmt. Applied formatting
   only to those files and reran their tests.
4. Minor: the full-range whitespace check found a trailing blank line at the end of
   `.gitignore`. Removed it and reran the check.
5. Minor: async RX and Router comments still described pre-T5 behavior. Updated the
   comments without changing behavior.

The Plan expected `e0fac50` as its current layer. Act began at `78e1f7a`, a commit
that captured the already reviewed iteration 007 index. The semantic baseline was
unchanged, so this is recorded as a revision-layer deviation rather than a scope or
product deviation.
