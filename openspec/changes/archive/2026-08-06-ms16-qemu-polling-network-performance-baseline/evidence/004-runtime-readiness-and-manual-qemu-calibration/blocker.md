# T6 guest build blocker

- Discovered at: T6 Runtime Readiness Gate, guest static artifact build
- Expected: fresh RISC-V static binary, `file` facts and SHA-256
- Actual: musl cross compiler is terminated by sandbox SIGSYS; make reports `Bad system call`
- Impact: the existing guest binary predates the implementation and cannot be used for QEMU calibration
- Completed: permanent RED tests, protocol failure atomicity, strict CLI, dual-endpoint local ledger, collector/report/Evidence fail-closed behavior, host and sanitizer builds, manual guide draft
- Partial: formal network path is implemented but has no guest runtime witness
- Unstarted: preflight PASS, product regressions, T7 user-net/TAP runtime Evidence
- Resume condition: run the exact guest build outside the restricted sandbox, then submit exit 0, `file` output and SHA-256 for independent Review in a new iteration

No QEMU command was run. No TAP device was created or deleted.
