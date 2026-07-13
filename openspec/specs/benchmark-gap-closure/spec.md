# Benchmark Gap Closure

> Q20 — TX jitter/counter gap closure between QEMU and D1 for Q21~Q23 决策基线。

## Purpose

为 Q21（UART completion queue）、Q22（user ring / zero-copy）、Q23（性能决策）建立 QEMU 和 D1 双平台可对比的 TX jitter 与 counter proxy 基线，不改变 UART 驱动语义。

## Requirements

### Requirement: Q20 TX jitter evidence

Q20 MUST produce comparable TX latency and jitter output for QEMU and D1 without changing UART driver semantics.

#### Scenario: TX jitter fields are present

- **WHEN** the Q20 benchmark runs S10, S14, S20, and S21
- **THEN** each applicable TX latency section MUST include P50, P99, max, `p99_p50_ratio`, and `max_p50_ratio`
- **AND** S10, S14, and S21 MUST retain `slow_over_line_plus10ms` or an equivalent line-time-relative slow count
- **AND** the output MUST keep the section label that identifies the tested policy and payload size.

#### Scenario: QEMU and D1 evidence are compared

- **WHEN** Q20 evidence is evaluated
- **THEN** QEMU output MUST be treated as path and relative-behavior evidence
- **AND** D1 output MUST be treated as true UART line-rate evidence
- **AND** QEMU throughput MUST NOT be used to claim physical serial line-rate performance.

### Requirement: Q20 TX counter proxy evidence

Q20 MUST report a stable TX counter section for QEMU and D1. D1 MUST provide effective CPU/copy proxy evidence. QEMU MAY report derived proxy fields as unavailable when the diagnostic counters are zero.

#### Scenario: TX counter fields are present

- **WHEN** the benchmark records TX counter evidence
- **THEN** the output MUST include user push, ring pop, hardware send, zero-send, no-progress, slow-poll exhausted, and yield exhausted counters when available
- **AND** unavailable counters MUST be represented explicitly rather than silently omitted
- **AND** D1 derived fields MUST include bytes per user call, bytes per ring pop, bytes per hardware send, zero sends per KiB, and no-progress events per KiB
- **AND** QEMU derived fields MAY be marked `not-available` when all TX debug counters are zero.

#### Scenario: Counter evidence is interpreted

- **WHEN** Q20 reports CPU/copy overhead
- **THEN** counter-derived data MUST be labeled as proxy evidence
- **AND** it MUST NOT be described as precise CPU utilization unless cycle-level measurement is added
- **AND** it MUST be interpreted together with throughput and latency.

### Requirement: Q20 raw evidence archive

Q20 MUST preserve raw benchmark logs separately from summary reports.

#### Scenario: Raw logs are saved

- **WHEN** Q20 validation is complete
- **THEN** `.claude/analysis/q20-evidence/qemu-rootfs.log` MUST contain the QEMU benchmark raw output
- **AND** `.claude/analysis/q20-evidence/d1-fullbench-command.log` MUST contain the D1 serial benchmark raw output
- **AND** `.claude/analysis/q20-evidence/README.md` MUST record build commands, run commands, benchmark macros, expected sections, and evidence status.

#### Scenario: Report is updated

- **WHEN** `docs/benchmark-report-async.md` is updated for Q20
- **THEN** it MUST link or name the raw evidence files
- **AND** it MUST keep QEMU and D1 results separated
- **AND** it MUST state that Q20 does not prove SMP correctness.

### Requirement: RX fixed payload is outside Q20

Q20 MUST NOT require RX fixed payload validation.

#### Scenario: RX fixed payload is skipped

- **WHEN** Q20 benchmark evidence is collected
- **THEN** S31 / RX fixed payload MAY remain skipped
- **AND** Q20 MUST still be allowed to pass from TX jitter, TX counter, and raw evidence gates
- **AND** the evidence README MUST record that RX fixed payload was intentionally excluded by user decision.

#### Scenario: RX performance is requested later

- **WHEN** RX fixed payload or RX throughput validation is requested after Q20
- **THEN** it MUST be planned as a separate change
- **AND** it MUST define its own input injection and evidence gates.

### Requirement: Q20 preserves driver behavior

Q20 MUST NOT alter UART driver behavior to satisfy benchmark output goals.

#### Scenario: Implementation scope is checked

- **WHEN** Q20 implementation is reviewed
- **THEN** changes to benchmark output, diagnostic export, build macros, reports, and raw evidence are allowed
- **AND** changes to `tx_copier_loop()`, waker ownership, IER ownership, TTY write/read semantics, `tcdrain`, or TX completion semantics MUST be rejected from Q20 scope
- **AND** such changes MUST be moved to a new correctness or optimization change.
