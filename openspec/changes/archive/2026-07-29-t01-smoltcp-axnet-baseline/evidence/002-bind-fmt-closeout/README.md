# Evidence: 002-bind-fmt-closeout

- Change: t01-smoltcp-axnet-baseline
- Iteration: 002-bind-harness-closeout
- Captured at: 2026-07-29
- Revision: worktree (net-k3 branch, uncommitted)
- Environment: riscv64-linux-musl-gcc, rustc nightly-2026-02-25, QEMU riscv64 virt

## Input Hashes

| Artifact | SHA-256 |
|---|---|
| Payload binary | `168036806819a73fb5a098cbda31a1d383423d9e709b9ac59cd5263065f07a75` |
| Kernel binary | `1476fa0d617bd7901cd4e5aa18dfa84c15af436b3c756b4f741e09ad6d3f9fc0` |
| Payload source | `tests/ms01_socket_baseline.c` (worktree, 4 new test functions added) |

## Toolchain

- riscv64-linux-musl-gcc (static, -O2)
- rustc 1.95.0-nightly (2026-02-25)
- QEMU riscv64 virt, VirtIO-MMIO networking

## Evidence Files

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-002-01 | plan-required | A6 fmt check: axnet crate passes cargo fmt --check | [fmt-check.log](fmt-check.log) | PASS |
| EV-002-02 | plan-required | A7 smoltcp lib test: insta unavailable, user exempted | [smoltcp-lib-test.log](smoltcp-lib-test.log) | EXEMPTED |
| EV-002-03 | plan-required | A1-A5 bind witness: 14/14 PASS, all markers green | [qemu-bind-witness.log](qemu-bind-witness.log) | PASS |
| EV-002-04 | plan-required | A8 lock audit: no unrelated registry drift | [diff-lock-audit.txt](diff-lock-audit.txt) | PASS |

## Acceptance Mapping

| Acceptance | Requirement | Evidence | Verdict |
|---|---|---|---|
| A1 bind getsockname | R2 TCP bind | EV-002-03: `PASS: bind-getsockname: port 18012` | PASS |
| A2 ephemeral bind | R2 TCP bind | EV-002-03: `PASS: bind-ephemeral: port 49673` | PASS |
| A3 bind conflict | R2 TCP bind | EV-002-03: `PASS: bind-conflict: EADDRINUSE` | PASS |
| A4 bind close cleanup | R2 TCP bind | EV-002-03: `PASS: bind-close-cleanup` | PASS |
| A5 original regression | R1-R5 all | EV-002-03: 10 original markers all PASS | PASS |
| A6 formatting | R6 isolation | EV-002-01: exit 0 | PASS |
| A7 smoltcp lib test | R1 dependency | EV-002-02: user exempted | EXEMPTED |
| A8 evidence complete | A10 evidence | All 4 required files present with hashes | PASS |
| A9 tasks consistent | closeout | See iteration 002 Act Response | PENDING |
| A10 Act Response clean | closeout | See iteration 002 Act Response | PENDING |

## Build Commands

```bash
# Kernel build (includes bind_check fix in wrapper.rs)
make ARCH=riscv64 BUS=mmio NET=y build  # exit 0

# Payload compile
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c  # exit 0

# QEMU parameters
qemu-system-riscv64 -machine virt -bios default \
  -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 \
  -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -nographic
```

## Limitations

- QEMU test: manual execution per project policy (hard policy, 2026-07-29).
  Automated harness not attempted.
- Smoltcp lib test: offline cache lacks `insta` dev dependency. Exempted by
  user directive — smoltcp is unmodified upstream code.
- Full `cargo check --offline` on axnet: blocked by sandbox environment
  (same ENV BLOCK as iter 001). Kernel full build passes (exit 0).
- Single-hart only; SMP evidence not in scope for MS01.
