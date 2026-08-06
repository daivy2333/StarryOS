## 1. Protocol and Tool Foundation

Iteration 001 已修复 wire 边界与 generator，并通过 foundation 和产品回归。003 Review 确认 collector、report、Evidence 的集成语义仍未收口；1.6-1.8 在 iteration 004 与 runtime readiness 一起完成。

- [x] 1.1 Add `tests/network_benchmark_protocol_test.c` with RED cases for frame bounds, network byte order, version mismatch, canonical config fingerprint, deterministic payload and CRC32; GREEN requires all malformed inputs to fail without partial state. Decoder failure atomicity carries into 2.2 before state-machine use.
- [x] 1.2 Add `tests/network_benchmark_protocol.h` and `.c` to implement the bounded framed control/data codec required by 1.1; preserve explicit serialization and reject raw-struct ABI or unbounded lengths. Remaining common-prefix message types carry into 2.2.
- [x] 1.3 Add `tests/network_benchmark_platform_test.c` with RED cases for monotonic regression, strict numeric counter parsing, unavailable capabilities and counter underflow; GREEN requires reason-coded results without treating unavailable as zero. Injectable instret calibration carries into 2.3.
- [x] 1.4 Add `tests/network_benchmark_platform.h` and `.c` to wrap monotonic time, `/proc/instret` and MS03 snapshot reads; host builds must expose unsupported guest counters as unavailable and must not add kernel ABI.
- [x] 1.5 Add valid and invalid NDJSON/manifest fixtures under `tests/fixtures/network-benchmark/` plus `tests/test_network_benchmark_tools.py`; RED must cover malformed schema, missing Evidence, invalid rounds and mismatched comparison keys. Missing integration cases carry into 1.6-1.8.
- [ ] 1.6 Add `scripts/network_benchmark_collect.py` using Python stdlib to sample QEMU, peer and collector PID CPU/RSS separately; GREEN requires fixture or self-test coverage for PID exit, counter regression and sampling scope.
- [ ] 1.7 Add `scripts/network_benchmark_report.py` to validate Schema v1 and derive C6 goodput, PPS, RTT distribution, delay variation, UDP errors, CPU and instruction efficiency; GREEN requires summaries to reconstruct from fixtures and retain invalid rounds.
- [ ] 1.8 Add `scripts/network_benchmark_evidence.py` to check required files, hashes, fields, round sets, endpoint ledgers and A/B comparison keys; GREEN requires missing or incomparable fixtures to fail with exact reasons.
- [x] 1.9 Add a Makefile foundation-test target for protocol/platform C tests and Python tool tests; GREEN requires the new aggregate target, existing `host-test`, axnet 8/8, MS01 parser self-test and QEMU target build to pass without product-code changes. RISC-V syntax remains ENV BLOCK with exit 159.

## 2. Portable Workload and Calibration

Iteration 003 Review 复现 local Gate 失败，并确认双端状态机、数据校验和工具仍不可用于校准。Tasks 1.6-1.8 与 2.1-2.7 在 iteration 004 先通过 Runtime Readiness Gate；2.8 随后在同轮进入人工 QEMU 边界。

- [ ] 2.1 Add `tests/network_benchmark.c` CLI, canonical configuration, Schema v1 output and signal cancellation; tests must reject missing, conflicting or out-of-range arguments before opening data sockets.
- [ ] 2.2 Implement the TCP control state machine with default port 5555 and topology-specific effective ports from R44/R45/R48; user-net guest-to-host uses host 15555 because QEMU hostfwd owns host 5555. Tests must cover HELLO/READY/START/CANCEL/SUMMARY/ERROR, version/config mismatch, peer EOF and summary timeout.
- [ ] 2.3 Implement TCP record TX/RX, RTT, bidirectional and 1/2/4/8-flow event-loop states with `TCP_NODELAY=1`; tests must witness partial I/O, C1 versus C6 accounting and bounded no-progress failure.
- [ ] 2.4 Implement UDP sequence/CRC validation and absolute-deadline pacing; tests must distinguish loss, duplicate, reorder, corrupt and late, and must record pacing resync without catch-up busy loops.
- [ ] 2.5 Implement nonblocking EAGAIN recovery, boundary matrices and smoke/quick/standard profile expansion; tests must prove recovery or a bounded invalid result at 64-entry, 128-buffer, UDP metadata, ARP and 64 KiB boundaries.
- [ ] 2.6 Implement in-process loopback mode and host-side local integration tests so N00-N03 and protocol failure paths run without QEMU; GREEN requires deterministic repeated summaries for a fixed seed.
- [ ] 2.7 Add Makefile host and RISC-V static benchmark targets, then build both binaries with recorded compiler flags and SHA-256; syntax, self-test and fixture tests must pass on both supported compile paths before runtime work.
- [ ] 2.8 Run manual user-net smoke and TAP calibration under R44/R45/R48, using hostfwd 5555 for host-to-guest, host listener 15555 for guest-to-host, and TAP port 5555 without hostfwd. Save required calibration Evidence; failures must identify console, topology, protocol, timer, instret, IRQ or collector as the earliest layer.

## 3. Polling B0 Runtime Evidence

- [ ] 3.1 Record N00 manifest, N01 timing/instret calibration, N02 loopback and N03 ARP/ICMP/MTU path checks with fixed QEMU/TAP facts and binary hashes.
- [ ] 3.2 Run TCP N10-N14 TX/RX/bidirectional/multiflow/write-size/steady-state rounds; every headline result must use receiver C6 bytes and retain partial or invalid rounds.
- [ ] 3.3 Run latency N20/N24 and UDP N21-N23 workloads, including the pilot-derived 25/50/75/90/100% offered-load staircase; standard correctness requires zero corruption, duplicate, reorder and loss.
- [ ] 3.4 Run N30 backpressure, N40 idle controls, N41 CPU/instruction efficiency, N42 MS03 IRQ efficiency and N43 timer interference; unsupported internal telemetry must remain unavailable.
- [ ] 3.5 Save manifest, QEMU command, serial, guest/host/collector NDJSON, IRQ snapshots, TAP pcap, results, summary and evidence-check under required Evidence; the checker must pass before B0 is declared complete.

## 4. Review and Handoff

- [ ] 4.1 Re-run static, host, guest-build, MS01-MS03 regression and OpenSpec validation Gates; preserve environment failures separately from source failures.
- [ ] 4.2 Review specs, full diff, raw Evidence and reconstructed summary; reject silent reruns, missing invalid rounds, README-only claims and changes outside benchmark/build artifacts.
- [ ] 4.3 Record B0 variance and comparison domain without setting unsupported absolute thresholds; hand MS04 the frozen benchmark version, comparison key and rerun procedure.
