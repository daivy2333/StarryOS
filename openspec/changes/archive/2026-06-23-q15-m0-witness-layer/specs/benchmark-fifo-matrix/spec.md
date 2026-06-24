## ADDED Requirements

### Requirement: FIFO boundary benchmark matrix

The benchmark SHALL support testing UART write+tcdrain latency at the following byte sizes:
1, 15, 16, 17, 31, 32, 33, 48, 49, 64, 256, 1024, 4096.

#### Scenario: All sizes produce valid output

- **WHEN** benchmark runs with the extended size matrix
- **THEN** each size produces at least one raw latency sample in microseconds

#### Scenario: 16B boundary visibility

- **WHEN** benchmark runs with sizes 15, 16, 17
- **THEN** the P50 latency for 17B SHALL be higher than 16B by at least one FIFO refill cycle (when tick-dependent refill is present)

### Requirement: Machine-parseable benchmark output

The benchmark SHALL output per-round metadata including commit hash, scheduler tick rate, and NS16550 FIFO depth in `key=value` format.

#### Scenario: Metadata present in output

- **WHEN** benchmark completes a round
- **THEN** output SHALL contain `commit=<hash>`, `tick=<hz>`, `fifo=<depth>` fields

### Requirement: Raw sample and percentile output

The benchmark SHALL output raw latency samples and P50/P95 percentiles for each tested size.

#### Scenario: P50 and P95 computed

- **WHEN** benchmark runs 10+ iterations per size
- **THEN** output SHALL contain `p50=<value>` and `p95=<value>` for each size
