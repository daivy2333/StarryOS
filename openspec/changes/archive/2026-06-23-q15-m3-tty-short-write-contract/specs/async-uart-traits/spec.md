## MODIFIED Requirements

### Requirement: TtyWrite reports accepted byte count

`TtyWrite::write` MUST return the number of bytes accepted into the output sink. Implementations MUST NOT report success for bytes that were not accepted into a hardware FIFO, ring buffer, PTY buffer, or equivalent backend.

#### Scenario: Output sink has enough capacity

- **WHEN** a caller writes a buffer and the output sink accepts every byte
- **THEN** `TtyWrite::write` MUST return `buf.len()`

#### Scenario: Output sink has partial capacity

- **WHEN** a caller writes a buffer and the output sink accepts only part of it
- **THEN** `TtyWrite::write` MUST return the accepted byte count
- **AND** StarryOS `Tty::write_at` MUST propagate that count to VFS callers

#### Scenario: Echo output ignores short write result

- **WHEN** line discipline emits an echo character sequence
- **THEN** it MAY ignore the returned count
- **AND** the implementation MUST make that best-effort behavior explicit in code
