# tx-bounded-fast-retry Specification

## Purpose
TBD - created by archiving change m1-tx-fast-retry. Update Purpose after archive.
## Requirements
### Requirement: TX copier bounded fast retry
TX copier SHALL execute up to 32 fast retries within the same `poll_fn` invocation when `send_bytes()` returns 0 (UART THR full), before falling back to interrupt-driven wakeup.

#### Scenario: Fast retry succeeds within budget
- **WHEN** `send_bytes()` returns 0 and retry count is less than `TX_FAST_RETRY_LIMIT` (32)
- **THEN** TX copier SHALL re-invoke `send_bytes()` within the same `poll_fn` without yielding to the scheduler

#### Scenario: Fast retry budget exhausted
- **WHEN** `send_bytes()` returns 0 for 33 consecutive attempts (32 retries + initial attempt)
- **THEN** TX copier SHALL register `TX_WAKER`, invoke `enable_tx_intr()`, perform one final `send_bytes()` recheck, and return `Poll::Pending` if still no progress

#### Scenario: Progress resets retry context implicitly
- **WHEN** `send_bytes()` returns > 0 after one or more 0-return attempts
- **THEN** TX copier SHALL advance the write cursor and continue polling without consuming retry budget

#### Scenario: Ring buffer empty after Pending resume
- **WHEN** TX copier resumes from `Poll::Pending` and `pop_batch()` returns 0
- **THEN** TX copier SHALL register ring buffer waker and return `Poll::Pending` (existing behavior preserved)

