# Evidence: Iteration 009 / Cycle 001-rework

Cycle 001 of Iteration 009 (`probe-and-automatic-product-gates`) for change
`ms05-qemu-bounded-bidirectional-device-data-plane`. Rework cycle closing the
Cycle 000 review gaps: wire/peer/bounded-server mismatch (C5), deadline and
ledger false positives (C2-C4) and stale/incomplete Evidence (C6).

## Final source identity

- HEAD: `8dc3ef7d63da00c1966e9cb70820c337494d3c57` (`MS06:第六次提交`, branch `net-k3`)
- Worktree: modified (this Cycle's repair edits to `tests/ms05_data_plane_probe.c`,
  `tests/ms05_data_plane_probe_test.c`, `scripts/ms05_data_plane_stimulus.py`, `Makefile`
  plus Cycle 000 staged probe/stimulus/Evidence)
- Source mtimes: probe.c `2026-08-15 18:18:52`, test.c `18:15:06`, stimulus.py `18:19:09` —
  all before the Gate window (`18:20:01` → `18:22:18`). No source edit occurred after a build.
- Collection window: 2026-08-15 18:20:01 → 18:22:18 +0800

## Gate index

| Gate | Command (see `commands.txt` for exact form + timestamps) | Result |
|---|---|---|
| Probe syntax | `cc -std=c11 -Wall -Wextra -Werror -fsyntax-only tests/ms05_data_plane_probe.c` | PASS (exit 0) |
| Decision harness | `cc ... ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test` && run | PASS (17 passed, exit 0) |
| Protocol self-test | `python3 scripts/ms05_data_plane_stimulus.py --self-test` | PASS (peer/timeout/grace added, exit 0) |
| Loopback self-test | `python3 scripts/ms05_data_plane_stimulus.py --loopback-self-test` | PASS (96 datagrams, exit 0) |
| host-test | `make host-test` | PASS (exit 0) |
| axnet qemu-diagnostics | `cargo test ... --features qemu-diagnostics --lib` | PASS (234) |
| axnet default | `cargo test ... --lib` | PASS (215) |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | PASS (7) |
| axdriver_virtio net | `cargo test ... --features net` | PASS (16) |
| virtio-drivers alloc | `cargo test ... --features alloc` | PASS (36 + 8 doctests) |
| uart_16550 async | `cargo test ... --features async` | PASS (62 + 8 + 10 doctests) |
| control witness 100x | literal loop command (race-stability.log) | PASS (0 failures / 100) |
| V3 witness 100x | literal loop command (race-stability.log) | PASS (0 failures / 100) |
| default-parallel full suite 100x | literal loop command (race-stability.log) | PASS (0 failures / 100) |
| kernel QEMU check | `cargo check --offline -p starry-kernel --features qemu` | PASS (exit 0) |
| D1 comparison | `cargo check --offline -p starry-kernel --features lichee-d1` | PASS as comparison (exit 101, exactly 25 axfs/axtask errors: 20 E0432 + 5 E0433) |
| fresh image | `make LOG=info build` | PASS (StarryOS_riscv64-qemu-virt.bin 40190144 B) |
| payloads | `riscv64-linux-musl-gcc` MS01 + `make -B` MS02-MS05 | PASS (MS01 150832, MS02 134712, MS03 138600, MS04 134232, MS05 144520 B) |
| artifact identity | `file`/`stat`/`sha256sum` one indexed capture | PASS (artifacts.sha256, re-audited below) |
| rustfmt | literal 18-file change-owned list, `--check` | PASS (exit 0) |
| OpenSpec | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | PASS (`Change ... is valid`) |
| diff check | `git diff --check -- . ':(exclude)...evidence/**'` | PASS (exit 0) |
| specs-vs-code + full diff + Evidence audit | `review.md` | PASS (0 Critical/Important unresolved) |

## Artifact qualification

All six artifacts are fresh (rebuilt in this Cycle after the final source) and
their SHA-256 were re-read from disk during the Evidence audit and match
`artifacts.sha256`:

- `StarryOS_riscv64-qemu-virt.bin` `57b672cf...` (40190144 B, 18:21:42)
- `tests/ms01_socket_baseline` `16803680...` (150832 B, 18:21:48)
- `tests/ms02_guest_service` `c2a252f9...` (134712 B, 18:21:53)
- `tests/ms03_irq_probe` `9cd43fa8...` (138600 B, 18:21:53)
- `tests/ms04_rx_probe` `11b567a1...` (134232 B, 18:21:53)
- `tests/ms05_data_plane_probe` `6a7189e2...` (144520 B, 18:21:53)

Cycle 000 `build.log`/`artifacts.sha256` identified an older binary (144136/144240 B) than the
final source; it is preserved as historical input and is NOT reused here.

## Scope limits

- No product/kernel/driver/axnet code was modified in this Cycle; only the probe, its
  harness, the host stimulus and the Makefile Gates.
- No manual QEMU console was started; no guest runtime PASS is claimed. Iteration 010
  (Tasks 6.1-6.3) remains the manual runtime boundary.
- V1/V2/V3 ABI, controls, flush semantics, MS01-MS04 sources and QEMU-only feature
  boundaries are unchanged (verified by the driver/axnet/kernel Gates and full diff review).
