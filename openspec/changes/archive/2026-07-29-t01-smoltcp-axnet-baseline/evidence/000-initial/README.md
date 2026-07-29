# Evidence: 000-initial — Characterization Witness

## Environment

| Item | Value |
|------|-------|
| Date | 2026-07-28 |
| Kernel | `StarryOS_riscv64-qemu-virt.bin` |
| Kernel SHA256 | `eb0662af5b6d6d41f0751d0f9b8722e5665798ee251304eeb761719b2ea8b422` |
| Disk SHA256 | `f0cd8bf7c2b6a309759eebd1d926861213dc201c9568c05deea9825e93d00250` |
| Payload SHA256 | `674b88e99524d74f89b67a9ac8d6ee370b1e364b8626865bd32c35e2fd9b7f51` |
| Payload size | 150760 bytes |
| QEMU | `qemu-system-riscv64` 7.0.0 |
| QEMU args | `-machine virt -m 1G -smp 1 -device virtio-net-device -netdev user -nographic` |
| Build | `riscv64gc-unknown-none-elf`, registry `axnet-ng 0.3.0-preview.2` + `starry-smoltcp 0.12.1-preview.1` |
| Payload delivery | HTTP download via `10.0.2.2:18765` → guest `/tmp` |

## Files

| File | Purpose | Hash |
|------|---------|------|
| `qemu-socket-baseline.log` | Raw test output | 9 PASS, 0 FAIL |
| `blocker.md` | Task 2.1 dependency graph blocker | BLOCKED |

## Scenario Mapping

| Scenario | Marker | Result |
|----------|--------|--------|
| TCP bind/listen/accept | `tcp-accept` | PASS |
| Two adjacent connections | `tcp-adjacent` | PASS |
| 512 capacity | `tcp-512cap` | PASS (512/512) |
| Close/relisten | `tcp-relisten` | PASS |
| UDP bidirectional | `udp-bidi` | PASS |
| TCP nonblock EAGAIN | `tcp-nonblock-accept` | PASS |
| UDP nonblock | `udp-nonblock` | PASS (ENOTCONN — fork behavior) |
| Poll readiness | `poll-readiness` | PASS |
| UDP source address | `udp-source` | PASS (127.0.0.1:49153) |

## Notes

- `tcp-relisten` test requires `sleep(2)` between close and rebind due to fork-based smoltcp port release delay.
- `udp-nonblock` returns `ENOTCONN(107)` instead of standard `EAGAIN(11)` — known fork smoltcp behavior. Test accepts both.
- `udp-bidi` test requires `sleep(2)` in parent before sendto to avoid race with child bind.
- Payload delivered via HTTP (10.0.2.2) — no rootfs modification. See `.claude/runbooks/qemu-network-testing.md`.

## Act Evidence

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-000-02 | act-added | The path-only dependency strategy does not remove `starry-smoltcp` | [blocker.md](blocker.md) | BLOCKED |
