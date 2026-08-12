# Evidence: 009-final-sandbox-rerun-and-qemu-runtime

- Change: `ms04-qemu-async-rx-queue-baseline`
- Iteration: `009-final-sandbox-rerun-and-qemu-runtime`
- Captured from: 2026-08-12T16:54:12+08:00
- Current revision: `8f5b5228747dc817a5a9de7a3461dccdf06e0c24`, plus this Evidence worktree diff
- Environment: WSL2 x86_64; restricted agent sandbox followed by user-run external commands; QEMU runtime limited to one RISC-V hart and one VirtIO-MMIO NIC

T7.3R and T8.1 passed. The supplied T8.2 log proves the core MS04 runtime baseline. The user explicitly
waived missing boot, repeat and compatibility cases; those cases are `WAIVED/SKIPPED`, not PASS.

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-009-01 | plan-required | Revision roles and raw-log whitespace boundary are unambiguous | [provenance.txt](provenance.txt) | PASS |
| EV-009-02 | plan-required | Executed guest commands are captured; missing terminal timing/termination metadata is waived | [commands.txt](commands.txt) | PASS WITH WAIVER |
| EV-009-03 | plan-required | Host, toolchain and QEMU single-hart contract are recorded; session termination is waived | [environment.txt](environment.txt) | PASS WITH WAIVER |
| EV-009-04 | user-required | External loopback and repaired static-probe clean rerun passed; first failure preserved | [sandbox-rerun.log](sandbox-rerun.log) | PASS |
| EV-009-05 | user-required | Four fresh static RISC-V payload builds passed | [build.log](build.log) | PASS |
| EV-009-06 | user-required | Kernel and payload sizes, producers and SHA-256 values are complete | [artifacts.sha256](artifacts.sha256) | PASS |
| EV-009-07 | user-required | Serial excerpt covers payload execution through MS02 TCP#1; boot/final tail are waived | [qemu-serial.log](qemu-serial.log) | PASS WITH WAIVER |
| EV-009-08 | user-required | MS04 snapshot, idle, nudge and burst satisfy the core runtime gates | [ms04-probe.log](ms04-probe.log) | PASS |
| EV-009-09 | user-required | MS03 idle/UART/RX/TX pass; both/repeat are waived | [ms03-regression.log](ms03-regression.log) | PASS WITH WAIVER |
| EV-009-10 | user-required | MS02 TCP#1 passes; remaining compatibility cases are waived and not claimed | [ms01-ms02-regression.log](ms01-ms02-regression.log) | PASS WITH WAIVER |
| EV-009-11 | plan-required | Evidence, runtime counters, waivers and full scope were reviewed | [final-review.md](final-review.md) | PASS |

## Collection rule

- Preserve the first failed or interrupted output. A clean rerun uses a new clearly labelled section;
  it must not replace the failed attempt with a summary.
- Raw output was preserved verbatim; derived logs record their source line ranges and waiver limits.
- `script -q -f .../qemu-serial.log -c 'qemu-system-riscv64 ...'` overwrites the prepared placeholder
  and records the complete interactive serial session. It records only; the user enters guest commands.
- No QEMU result in this directory is hardware, SMP, PCI, physical-timing or performance evidence.

## T8.1 repair note

The first external rerun is preserved unchanged. It passed the 96-packet loopback but failed the
`ms04_rx_probe` musl build because `struct timeval` lacked its direct `<sys/time.h>` include. The user
authorized this small fix in iteration 009. The permanent source guard, 15-test MS04 host harness,
10-test C decision suite and host syntax check pass. The restricted sandbox cross-build now reaches
the compiler and stops at the already classified `SIGSYS`; the external clean rebuild passed.

## T8.2 waiver

User instruction: “当前没有遇到fatal，至于少的几个测试我都觉得没必要重复进行了，我授权的，
你看看，没有问题就填写回复”。The waived items are boot signatures, MS03 both/repeat-rx2,
MS02 TCP#2/UDP/COMPLETE, MS01 14/14, the post-regression final snapshot and session termination
metadata. This Evidence therefore supports the core MS04 single-hart VirtIO-MMIO runtime baseline,
not a complete MS01/MS02/MS03 compatibility claim.
