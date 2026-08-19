# MS05 Iteration 011 / Cycle 001 — Evidence index

> Cycle: 011-independent-manual-qemu-runtime-and-closeout-review / 001-rework
> Plan Context: `iterations/011-independent-manual-qemu-runtime-and-closeout-review/001-rework.md`
> Persisted Evidence mode: **required**

## Scope

Repair item 6.2-R1 (first-TX queue-owner wake after ARP flush), 6.1-R1
(requalify + refreeze artifacts), 6.2-R2 (manual QEMU with isolated evidence)
and 6.3-R1 (final review) of change
`ms05-qemu-bounded-bidirectional-device-data-plane`.

## Automatic qualification (Act, 2026-08-17)

- Manifest: `manifest.json` — 44/44 records `pass`, single source freeze,
  six artifacts, 18 artifact file/stat/sha256 records. Captured by
  `scripts/ms05_evidence_capture.py --run automatic` from the repo root on the
  repaired (dirty) tree at HEAD `2af394e6cc8e6aa9ae7026d7ede136382258a98b`.
- Positive audit: `evidence-audit.log` + `qualification.json` (verdict PASS) +
  `env-blocked.json` (none) — `scripts/ms05_evidence_audit.py --root … --write-qualification`;
  binding re-verified with `--verify-qualification` (`qualification binding VERIFIED`).
- Automatic gates incl.: `make host-test` (exit 0), axnet default (218) +
  qemu-diagnostics (237), axdriver-net/virtio, virtio-drivers, uart-async,
  MS03/MS04 harnesses, evidence tools unit tests, capture/audit self-tests,
  race control/v3/full-suite 100×, kernel qemu check, kernel D1 check
  (expected exit 101 with the exact E0432/E0433 diagnostic contract), build
  image + ms01 + payloads, rustfmt, openspec strict validation, dual diff
  checks, and artifact records.
- Freeze: `artifacts-before.txt` + `artifacts.sha256` — 6/6 verified with
  `sha256sum -c`. The repaired image sha256 `4018d326e828…` supersedes the
  Cycle 000 freeze (`fe20b5b2…`); the ms05 probe hash `db27b567…` is
  unchanged from the Cycle 000 freeze because the probe source was not edited
  by the repair.

## Task 6.1 files

| File | Status |
|---|---|
| `environment.txt` | created (dirty-tree note included) |
| `commands.txt` | created (supersedes Cycle 000 command list) |
| `host-test.log` | raw copy of `logs/host-test.log`, `# ms05 capture exit=0` |
| `automatic-gates.log` | derived manifest summary, 44/44 pass |
| `artifacts-before.txt` | frozen sizes, 6 artifacts |
| `artifacts.sha256` | frozen SHA-256, 6/6 OK |

## Task 6.2 (manual QEMU) files — to be returned by the user

| File | Required by | Status |
|---|---|---|
| `qemu-wget-serial.log`, `wget.pcap` | 6.2-R2 session 0 | pending (user) |
| `qemu-ms05-serial.log` | 6.2-R2 sessions 1-2 | pending (user) |
| `ms05-txonly-host.log`, `ms05-bidirectional-host.log`, `ms05-slotfull-host.log`, `ms05-descfull-host.log`, `ms05-flush-host.log` | 6.2-R2 session 2 | pending (user) |
| `ms05-markers.txt` | 6.2-R2 | pending (user) |
| `qemu-ms04-serial.log`, `ms04-burst-host.log`, `ms04-markers.txt` | 6.2-R2 session 3 | pending (user) |
| `qemu-network-serial.log`, `network-host.log` + pcaps | 6.2-R2 session 4 | pending (user) |
| `runtime-exits.txt` | 6.2-R2 | pending (user) |

No QEMU gate is PASS until the user's raw output is returned and audited.

## Limitations

- Conclusions are limited to the declared single-hart QEMU VirtIO-MMIO software
  /device model; nothing here qualifies SMP, DWMAC, real board, DMA/cache or
  performance behavior.
- The Cycle 000 evidence remains immutable diagnostic history; this Cycle 001
  root owns the repaired qualification.