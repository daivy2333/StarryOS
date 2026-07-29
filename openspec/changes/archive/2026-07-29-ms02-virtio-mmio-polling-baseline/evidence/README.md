# Evidence Index

| Iteration | Status | Summary |
|---|---|---|
| [000-initial](000-initial/README.md) | BLOCKED | Gate 3 host test could not enter the test body because Cargo could not obtain a missing cached dependency. |
| [001-environment-ready](001-environment-ready/README.md) | BLOCKED | Deadline policy reached RED and GREEN, but the target guest payload compiler was terminated with `Bad system call`. |
| [002-manual-verification-boundary](002-manual-verification-boundary/README.md) | reported | Agent batch PASS: fmt, deadline 4/4, feature tree, target build, MS01 self-test, openspec validate. Spec and code review PASS. QEMU, pcap, CPU, MS01 runtime regression pending user submission. |
| [003-policy-coverage-and-runtime-evidence](003-policy-coverage-and-runtime-evidence/README.md) | reported | T1 policy coverage PASS (8/8 unit tests). Agent batch PASS (fmt, build, MS01 self-test, openspec validate). User Evidence all submitted: payload compile, no-hostfwd boot, user-net TCP/UDP, TAP ARP/ICMP, idle CPU, MS01 runtime 14/14 PASS. Spec/code review PASS, 0 findings. |
