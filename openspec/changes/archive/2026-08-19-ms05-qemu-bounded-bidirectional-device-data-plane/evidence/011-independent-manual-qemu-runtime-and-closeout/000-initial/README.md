# MS05 Iteration 011 / Cycle 000 — Independent Manual QEMU Runtime and Closeout — Evidence index

> Cycle: 011-independent-manual-qemu-runtime-and-closeout-review / 000-initial
> Plan Context: `iterations/011-independent-manual-qemu-runtime-and-closeout-review/000-initial.md`
> Persisted Evidence mode: **required**

## Scope

Tasks 6.1 (R44 ordinary-terminal rerun + artifact freeze), 6.2 (manual QEMU six
MS05 modes + R51 MS04 regression + R45/MS01 network regression) and 6.3 (final
specs-vs-code / full-diff / Evidence audit) of change
`ms05-qemu-bounded-bidirectional-device-data-plane`.

QEMU guest interaction is manual per R44. This directory collects the frozen
baseline prepared by Act and the raw outputs returned by the user's manual run.

## Baseline (prepared by Act, 2026-08-17)

- Revision: `2af394e6cc8e6aa9ae7026d7ede136382258a98b` (`net-k3`, worktree clean)
- Environment: `environment.txt`
- Manual command list: `commands.txt`
- Artifact freeze before QEMU boot: `artifacts-before.txt`, `artifacts.sha256`
- Automatic preflight `make host-test` (this sandbox): `host-test-preflight.log`
  — exit 0, all harnesses and 25 negative fixtures PASS (no R44 EPERM here)

## Evidence files (to be filled by user manual run)

| File | Required by | Status |
|---|---|---|
| `environment.txt` | 6.1 | created |
| `commands.txt` | 6.1 | created |
| `artifacts-before.txt` | 6.1 | created |
| `artifacts.sha256` | 6.1 | created |
| `host-test.log` | 6.1 | **pending — user ordinary-terminal rerun** |
| `host-test-preflight.log` | 6.1 (supplement) | created (sandbox preflight, exit 0) |
| `qemu-serial.log` | 6.2 | pending |
| `ms05-snapshot-host.log` | 6.2 | pending |
| `ms05-txonly-host.log` | 6.2 | pending |
| `ms05-bidirectional-host.log` | 6.2 | pending |
| `ms05-slotfull-host.log` | 6.2 | pending |
| `ms05-descfull-host.log` | 6.2 | pending |
| `ms05-flush-host.log` | 6.2 | pending |
| `ms05-markers.txt` | 6.2 | pending |
| `ms04-burst-host.log` | 6.2 | pending |
| `ms04-markers.txt` | 6.2 | pending |
| `network-host.log` / pcaps | 6.2 | pending |
| `runtime-exits.txt` | 6.2 | pending |
| `review.md` | 6.3 | pending |

## Gate status

| Gate | Task | Status |
|---|---|---|
| 6.1 unchanged-argv ordinary-terminal gate | 6.1 | pending (user) |
| artifact/revision freeze | 6.1 | frozen (Act) |
| six MS05 runtime gates | 6.2 | pending (user) |
| R51 MS04 compatibility | 6.2 | pending (user) |
| R45/MS01 network/socket compatibility | 6.2 | pending (user) |
| final Evidence hash/index, specs-vs-code, strict validation, diff Review | 6.3 | pending |

No gate is PASS until the user's manual raw output is collected and audited.
