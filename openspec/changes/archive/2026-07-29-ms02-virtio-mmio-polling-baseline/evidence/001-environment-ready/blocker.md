# Blocker: RISC-V guest payload compiler terminated

- Discovered at: T1 payload GREEN verification, before T4, Gate 5
- Expected: `make tests/ms02_guest_service` invokes the declared static
  RISC-V musl compiler and produces `tests/ms02_guest_service`.
- Actual: the compiler process was terminated with `Bad system call`; Make
  exited 2 and no payload binary was produced.
- Impact: T1 cannot pass its payload compilation condition. T3 lacks its
  target-build witness, and T4 cannot begin. The iteration requires Act to
  stop on an agent Gate failure.
- Completed work: T2 timer fallback and its RED/GREEN policy witness.
- Partial work: T1 policy tests, C source, and Makefile target; T3 Cargo
  feature and feature-tree witness.
- Unstarted work: formatting Gate, target kernel build, MS01 self-test,
  complete task/full-diff completion review, and all user-operated QEMU
  evidence.
- Worktree state: modified `Makefile`, axnet Cargo/features and network
  service/device sources; new `tests/ms02_guest_service.c`; current change
  remains untracked. No payload binary or core file was left behind.
- Gates: Gate 1 PASS; Gate 2 PASS; Gate 3 PASS for T1 RED; Gate 4 PASS for
  completed T2; Gate 5 BLOCKED for T1; Gate 6 applied.
- Resume condition: provide a working `riscv64-linux-musl-gcc` execution path
  for this environment, then use Plan Review to create a new ready iteration.
  This blocked iteration must not be resumed.
