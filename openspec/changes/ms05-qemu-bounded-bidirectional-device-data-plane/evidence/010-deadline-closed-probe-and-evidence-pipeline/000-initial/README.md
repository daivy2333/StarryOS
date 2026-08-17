# MS05 Iteration 010 / Cycle 000 — Evidence index

Derived from `manifest.json`; the manifest and raw logs are the sole authorities.

- schema_version: 1
- created: 2026-08-15T12:44:18.808879Z
- source freeze HEAD: 8dc3ef7d63da
- records: 44
- artifacts: 6

## Gate summary

| gate_id | exit | classification | log |
|---|---|---|---|
| host-test | 0 | pass | logs/host-test.log |
| axnet-qemu-diagnostics | 0 | pass | logs/axnet-qemu-diagnostics.log |
| axnet-default | 0 | pass | logs/axnet-default.log |
| axdriver-net | 0 | pass | logs/axdriver-net.log |
| axdriver-virtio | 0 | pass | logs/axdriver-virtio.log |
| virtio-drivers | 0 | pass | logs/virtio-drivers.log |
| uart-async | 0 | pass | logs/uart-async.log |
| ms03-harness-compile | 0 | pass | logs/ms03-harness-compile.log |
| ms03-harness-run | 0 | pass | logs/ms03-harness-run.log |
| ms04-harness-compile | 0 | pass | logs/ms04-harness-compile.log |
| ms04-harness-run | 0 | pass | logs/ms04-harness-run.log |
| evidence-tools-unittest | 0 | pass | logs/evidence-tools-unittest.log |
| capture-self-test | 0 | pass | logs/capture-self-test.log |
| audit-self-test | 0 | pass | logs/audit-self-test.log |
| race-control-100x | 0 | pass | logs/race-control-100x.summary.log |
| race-v3-100x | 0 | pass | logs/race-v3-100x.summary.log |
| race-full-suite-100x | 0 | pass | logs/race-full-suite-100x.summary.log |
| kernel-qemu-check | 0 | pass | logs/kernel-qemu-check.log |
| kernel-lichee-d1-check | 101 | pass | logs/kernel-lichee-d1-check.log |
| build-image | 0 | pass | logs/build-image.log |
| build-ms01 | 0 | pass | logs/build-ms01.log |
| build-payloads | 0 | pass | logs/build-payloads.log |
| rustfmt-check | 0 | pass | logs/rustfmt-check.log |
| openspec-validate-strict | 0 | pass | logs/openspec-validate-strict.log |
| diff-check | 0 | pass | logs/diff-check.log |
| diff-cached-check | 0 | pass | logs/diff-cached-check.log |
| artifact-StarryOS_riscv64-qemu-virt.bin-file-0 | 0 | pass | logs/artifact-StarryOS_riscv64-qemu-virt.bin-file-0.log |
| artifact-StarryOS_riscv64-qemu-virt.bin-stat-1 | 0 | pass | logs/artifact-StarryOS_riscv64-qemu-virt.bin-stat-1.log |
| artifact-StarryOS_riscv64-qemu-virt.bin-sha256sum-2 | 0 | pass | logs/artifact-StarryOS_riscv64-qemu-virt.bin-sha256sum-2.log |
| artifact-ms01_socket_baseline-file-3 | 0 | pass | logs/artifact-ms01_socket_baseline-file-3.log |
| artifact-ms01_socket_baseline-stat-4 | 0 | pass | logs/artifact-ms01_socket_baseline-stat-4.log |
| artifact-ms01_socket_baseline-sha256sum-5 | 0 | pass | logs/artifact-ms01_socket_baseline-sha256sum-5.log |
| artifact-ms02_guest_service-file-6 | 0 | pass | logs/artifact-ms02_guest_service-file-6.log |
| artifact-ms02_guest_service-stat-7 | 0 | pass | logs/artifact-ms02_guest_service-stat-7.log |
| artifact-ms02_guest_service-sha256sum-8 | 0 | pass | logs/artifact-ms02_guest_service-sha256sum-8.log |
| artifact-ms03_irq_probe-file-9 | 0 | pass | logs/artifact-ms03_irq_probe-file-9.log |
| artifact-ms03_irq_probe-stat-10 | 0 | pass | logs/artifact-ms03_irq_probe-stat-10.log |
| artifact-ms03_irq_probe-sha256sum-11 | 0 | pass | logs/artifact-ms03_irq_probe-sha256sum-11.log |
| artifact-ms04_rx_probe-file-12 | 0 | pass | logs/artifact-ms04_rx_probe-file-12.log |
| artifact-ms04_rx_probe-stat-13 | 0 | pass | logs/artifact-ms04_rx_probe-stat-13.log |
| artifact-ms04_rx_probe-sha256sum-14 | 0 | pass | logs/artifact-ms04_rx_probe-sha256sum-14.log |
| artifact-ms05_data_plane_probe-file-15 | 0 | pass | logs/artifact-ms05_data_plane_probe-file-15.log |
| artifact-ms05_data_plane_probe-stat-16 | 0 | pass | logs/artifact-ms05_data_plane_probe-stat-16.log |
| artifact-ms05_data_plane_probe-sha256sum-17 | 0 | pass | logs/artifact-ms05_data_plane_probe-sha256sum-17.log |

## Artifacts

| path | size | sha256 | generating gate |
|---|---|---|---|
| StarryOS_riscv64-qemu-virt.bin | 40190144 | 57b672cfbea84c6f… | build-image |
| tests/ms01_socket_baseline | 150832 | 168036806819a73f… | build-payloads |
| tests/ms02_guest_service | 134712 | c2a252f9fc473539… | build-payloads |
| tests/ms03_irq_probe | 138600 | 9cd43fa87ef6a7a0… | build-payloads |
| tests/ms04_rx_probe | 134232 | 11b567a1a071999f… | build-payloads |
| tests/ms05_data_plane_probe | 145008 | 8505e46787b5bbee… | build-payloads |

## Qualification
- verdict: PASS
- manifest_sha256: f0ac0f38b181448a…
- audit_log_sha256: e249ce818a7a0e50…

