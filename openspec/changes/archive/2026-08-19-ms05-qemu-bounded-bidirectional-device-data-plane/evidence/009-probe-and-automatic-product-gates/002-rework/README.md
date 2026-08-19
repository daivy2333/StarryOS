# Evidence: 009-probe-and-automatic-product-gates / 002-rework

Cycle 002 of Iteration 009 (`probe-and-automatic-product-gates`) for change
`ms05-qemu-bounded-bidirectional-device-data-plane`. Second rework cycle
closing the Cycle 001 review gaps: vacuous/partial normal-mode traffic (C2/C4),
incomplete absolute guest/host deadlines (C2/C4-C5) and synthetic command
metadata plus missing persisted RED/audit witnesses (C6).

## Final source identity

- HEAD: `8dc3ef7d63da00c1966e9cb70820c337494d3c57` (`MS06:第六次提交`, branch `net-k3`)
- Worktree: modified (this Cycle's edits to `tests/ms05_data_plane_probe.c`,
  `tests/ms05_data_plane_probe_test.c`, `scripts/ms05_data_plane_stimulus.py`,
  `scripts/ms05_evidence_audit.py` plus the Cycle 002 Evidence directory)
- Source mtimes: probe.c `2026-08-15 18:52:11`, test.c `18:49:56`, stimulus.py
  `18:54:50` — all before the Gate window (`18:58:28` → `19:00:36`). No source
  edit occurred after a build; every artifact hash was re-read after build.
- `scripts/ms05_evidence_audit.py` is the Evidence-authoring/audit tool
  written as part of T5.2-R2 (mtime `19:04:27`). It is not a source
  dependency of any of the six artifacts and does not affect their
  provenance; the audit run itself (`19:04:36`) is recorded in
  `commands.txt` and `evidence-audit.log`.
- Collection window: 2026-08-15 18:58:28 → 19:04:36 +0800

## Gate index

| Gate | Command (see `commands.txt` for exact form + timestamps) | Result |
|---|---|---|
| RED witnesses (6) | C harness single-test fixtures + Python fixtures (see `traffic-and-deadline.log`) | RED (exit 134 / buggy acceptance) as required |
| Decision harness | `cc ... ms05_data_plane_probe_test.c -o /tmp/ms05-data-plane-probe-test` && run | PASS (22 passed, exit 0) |
| Python self-test | `python3 scripts/ms05_data_plane_stimulus.py --self-test` | PASS (drip=PASS added, exit 0) |
| Python loopback | `python3 scripts/ms05_data_plane_stimulus.py --loopback-self-test` | PASS (96 datagrams, exit 0) |
| host-test | `make host-test` | PASS (exit 0) |
| axnet qemu-diagnostics | `cargo test ... --features qemu-diagnostics --lib` | PASS (234) |
| axnet default | `cargo test ... --lib` | PASS (215) |
| axdriver_net | `cargo test --manifest-path crates/axdriver_net/Cargo.toml --offline` | PASS (7) |
| axdriver_virtio net | `cargo test ... --features net` | PASS (16) |
| virtio-drivers alloc | `cargo test ... --features alloc` | PASS (36 + 8 doctests) |
| uart_16550 async | `cargo test ... --features async` | PASS (62 + 8 + 10 doctests) |
| MS03 host harness | `rustc ... tests/ms03-irq-host-harness.rs ... && run` | PASS (33) |
| MS04 host harness | `rustc ... tests/ms04-async-rx-host-harness.rs ... && run` | PASS (16) |
| control witness 100x | literal loop (race-stability.log) | PASS (0 failures / 100) |
| V3 witness 100x | literal loop (race-stability.log) | PASS (0 failures / 100) |
| default-parallel full suite 100x | literal loop (race-stability.log) | PASS (100/100 "215 passed") |
| kernel QEMU check | `cargo check --offline -p starry-kernel --features qemu` | PASS (exit 0; 2 pre-existing axnet warnings) |
| D1 comparison | `cargo check ... --features lichee-d1` | PASS as comparison (exit 101, exactly 25 axfs/axtask errors: 20 E0432 + 5 E0433; raw in `d1-full.log`) |
| fresh image | `make LOG=info build` | PASS (StarryOS_riscv64-qemu-virt.bin 40190144 B) |
| payloads | musl-gcc MS01 + `make -B` MS02-MS05 | PASS (MS01 150832, MS02 134712, MS03 138600, MS04 134232, MS05 144576 B) |
| artifact identity | `file`/`stat`/`sha256sum` one indexed capture | PASS (artifacts.sha256, re-audited by evidence-audit) |
| rustfmt | literal 18 change-owned Rust files `--check` | PASS (exit 0) |
| OpenSpec strict | `openspec validate ms05-qemu-bounded-bidirectional-device-data-plane --strict` | PASS (`Change ... is valid`) |
| non-Evidence diff | `git diff --check -- . ':(exclude)...evidence/**'` | PASS (exit 0) |
| Evidence audit | `python3 scripts/ms05_evidence_audit.py --write-log evidence-audit.log` | PASS (7 negative fixtures fail for their intended reasons; positive audit PASS) |

## Artifact qualification

All six artifacts were built from the final source and hashes captured in
`build.log` and `artifacts.sha256` (identical digests). `evidence-audit.log`
re-reads every hash and confirms source mtimes predate the build window.
`env-blocked.txt` records `None` — no R44 capability failure occurred in this
run, so no Iteration 010 rerun list is required.

## Limits

- These are automatic Gates only. Guest runtime QEMU evidence, R51 regression
  and final closeout belong to Iteration 010 (Tasks 6.1-6.3).
- The RED witnesses in `traffic-and-deadline.log` were produced by restoring
  the Cycle 001 baseline decisions (fixtures in `red-fixtures/`) against the
  new production-shared tests, then re-running the same assertions GREEN
  against the final source.
