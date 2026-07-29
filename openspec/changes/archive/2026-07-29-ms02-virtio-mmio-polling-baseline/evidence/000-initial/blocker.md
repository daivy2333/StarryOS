# Blocker: Gate 3 host test dependency unavailable

- Discovered at: T1, current-state validation, Gate 3
- Expected: `cargo test --manifest-path crates/axnet/Cargo.toml --lib
  service::tests -- --nocapture` reaches the axnet test binary so the planned
  deadline-policy RED test can be established.
- Actual: Cargo attempted to download `libc 0.2.182`; the crate was not
  available in the local cache and DNS resolution for `static.crates.io`
  failed. Cargo exited 101 before compiling or running `service::tests`.
- Impact: The iteration explicitly requires Act to stop when the axnet host
  test cannot enter the test body. Product code cannot be changed without the
  required RED witness.
- Completed work: Rules, Runbook, change baseline, call path, revision,
  feature RED baseline, compiler availability, and OpenSpec readiness were
  checked.
- Partial work: None.
- Unstarted work: RED deadline tests, guest payload, Makefile target, timer
  fallback, ICMP feature change, agent build/regression Gate, and all manual
  QEMU evidence.
- Worktree state: No product files were modified. Only the active untracked
  change received Act Response and Evidence files.
- Resume condition: Make `libc 0.2.182` and any remaining locked Cargo
  dependencies available in the local Cargo cache, or run Act in an
  environment where the declared host test can resolve them. Because this
  iteration is blocked, Plan Review must create a new ready iteration before
  Act resumes.
