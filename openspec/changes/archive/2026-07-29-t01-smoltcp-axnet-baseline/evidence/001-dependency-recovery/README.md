# Iteration 001 Evidence

## Result

The dependency boundary and socket runtime witness passed. The automatic QEMU
launcher was blocked by sandbox `EPERM`; the user ran the runbook commands and
provided the strict guest output.

The offline smoltcp lib test could not resolve uncached dev dependency `insta`.
The local axnet check and the new-image QEMU witness passed.

## Inputs

| Input | SHA-256 |
|---|---|
| Kernel | `c90a9dd80a65da721d01d8c1b70d454fa6966d05b455397d80fee80e26a7ce4b` |
| Rootfs | `ec14e1dc8728c9a54bd716d95b3f33e7676d1c04f41274dea6d7bb8458eb6300` |
| Payload | `f93bf3ab0dac8faf47d3a96555617457278e803a6a37c89ed390c9c8cf02d7b5` |
| Payload source | `01ffe5136e62260ed1fde1e2f750bfa5f476e775415390a54ee597b57f78f618` |
| Cargo lock | `b3a5340a80d4b79a7b0e187c6ae875ec2daaa10bb018f10258d9928ccab0f4a6` |

Toolchain: rustc 1.95.0-nightly, cargo 1.95.0-nightly, QEMU 7.0.0.

## Evidence Map

| Acceptance | Evidence |
|---|---|
| A1-A2 | `dependency-tree.txt`, `crate-gates.log` |
| A3-A7 | `qemu-socket-baseline.log` |
| A8 | `diff-lock-audit.txt`, `crate-gates.log` |
| A9 | `harness-cleanup.log`, authorized manual QEMU deviation |
| A10 | This directory |

## Authorized Deviations

- Global OpenSpec format error waiver: “不用管，这些格式错误，请直接开始，我授权了”.
- QEMU execution handoff: “这里进行测试你按runbook给我命令行我来手动做”.
- The automatic launcher remains implemented and self-tested. Its real QEMU
  path returned `Operation not permitted` in the sandbox.

