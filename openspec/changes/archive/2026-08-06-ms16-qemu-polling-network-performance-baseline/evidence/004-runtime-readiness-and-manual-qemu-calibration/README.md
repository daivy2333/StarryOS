# Evidence: 004-runtime-readiness-and-manual-qemu-calibration

- Change: `ms16-qemu-polling-network-performance-baseline`
- Iteration: `004-runtime-readiness-and-manual-qemu-calibration`
- Captured at: 2026-08-05 Asia/Shanghai
- Revision: `2a9319a946dbe9c07cb0f448d82c0b7c14069015` with uncommitted worktree
- Environment: x86_64 sandbox、C11 host、RISC-V musl cross compiler、manual QEMU boundary

| ID | Origin | Claim | Artifact | Result |
|---|---|---|---|---|
| EV-004-01 | plan-required | RED witnesses and host/local GREEN are reproducible | [focused-verification.log](focused-verification.log) | PASS |
| EV-004-02 | plan-required | Fresh guest artifact cannot be produced in the current sandbox | [guest-build.log](guest-build.log) | BLOCKED |
| EV-004-03 | act-added | T6 stops before manual QEMU calibration | [blocker.md](blocker.md) | BLOCKED |

T7 runtime files are absent because the Runtime Readiness Gate did not pass. Existing `tests/network_benchmark` predates the current source and is not accepted as Evidence.
