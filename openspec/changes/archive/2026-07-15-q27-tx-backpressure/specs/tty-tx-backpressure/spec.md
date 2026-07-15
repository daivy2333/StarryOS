## ADDED Requirements

### Requirement: TTY writable readiness follows writer capacity

StarryOS TTY MUST expose `IoEvents::OUT` only when its output writer can currently accept data and MUST register the writer's writable waker for OUT waiters.

#### Scenario: OUT is clear when UART TX ring is full

- **WHEN** the UART TX ring has no writable space
- **THEN** `Tty::poll()` MUST NOT include `IoEvents::OUT`

#### Scenario: OUT becomes ready after TX consumption

- **WHEN** a waiter registers interest in `IoEvents::OUT`
- **AND** the TX copier releases ring space
- **THEN** the existing TX ring wake path MUST wake the waiter
- **AND** the waiter MUST recheck readiness before writing

#### Scenario: PTY readiness compatibility

- **WHEN** a `Tty` uses `PtyWriter`
- **THEN** Q27 MUST preserve the existing PTY OUT and short-write behavior

### Requirement: Blocking UART writes wait for complete acceptance

A blocking UART TTY write MUST preserve the fast path when the request fits and MUST wait without busy looping until the complete request has been accepted when the TX ring becomes full.

#### Scenario: Blocking fast path

- **WHEN** the TX ring accepts the entire request on the first writer call
- **THEN** `Tty::write_at()` MUST return the full source length without entering a wait path
- **AND** it MUST NOT allocate, acquire a new producer lock, yield, or register a waker

#### Scenario: Blocking short write continues after wake

- **WHEN** a blocking write accepts only a prefix because the TX ring becomes full
- **THEN** the TTY MUST retain the accepted count
- **AND** it MUST wait for OUT readiness before retrying the remaining suffix
- **AND** it MUST return the full source length only after every source byte has been accepted

#### Scenario: Blocking write does not busy loop

- **WHEN** the TX ring remains full
- **THEN** the blocked task MUST park through `poll_io` and the TX writable waker
- **AND** it MUST NOT repeatedly self-wake or consume scheduler ticks without TX progress

#### Scenario: Empty write

- **WHEN** the source buffer is empty
- **THEN** the write MUST return zero immediately without polling or waker registration

### Requirement: Nonblocking UART writes preserve partial and WouldBlock semantics

UART TTY writes configured through either F_SETFL `O_NONBLOCK` or FIONBIO MUST never wait for TX space.

#### Scenario: Nonblocking partial write

- **WHEN** a nonblocking write accepts a non-empty source prefix but not the complete request
- **THEN** it MUST return the accepted source byte count immediately

#### Scenario: Nonblocking zero progress

- **WHEN** a nonblocking write cannot accept any complete source byte
- **THEN** it MUST return `WouldBlock`
- **AND** it MUST NOT register a waiter or spin

#### Scenario: F_SETFL and FIONBIO parity

- **WHEN** nonblocking mode is enabled through F_SETFL or FIONBIO
- **THEN** both entry points MUST produce the same partial and `WouldBlock` write behavior

### Requirement: ONLCR writes preserve source-character boundaries

When `OPOST|ONLCR` maps a source newline to `\r\n`, TTY return counts MUST remain source-byte counts and a nonblocking return MUST NOT leave a partially committed mapped character.

#### Scenario: Blocking newline completes both mapped bytes

- **WHEN** a blocking write maps `\n` to `\r\n` and the ring fills between the two mapped bytes
- **THEN** the TTY MUST wait and submit the remaining mapped byte before counting the source newline as consumed

#### Scenario: Nonblocking newline with one byte free

- **WHEN** the next source byte is `\n` and writable capacity is one byte
- **THEN** the TTY MUST NOT submit only `\r`
- **AND** it MUST return the previously completed source prefix or `WouldBlock`

#### Scenario: Mixed transformed buffer returns source count

- **WHEN** a buffer contains ordinary bytes and newlines
- **THEN** every returned partial count MUST end at a complete source-character boundary
- **AND** retrying the unconsumed suffix MUST not duplicate or omit mapped bytes

### Requirement: Q27 preserves compatibility and performance

Q27 MUST leave echo best-effort behavior, PTY behavior, TX completion semantics, and the no-wait UART fast path unchanged, and MUST pass regression gates before completion.

#### Scenario: Echo remains best effort

- **WHEN** line discipline echo writes while its sink is full
- **THEN** echo MUST remain nonblocking and MAY ignore the short-write count

#### Scenario: Drain behavior remains separate from writable readiness

- **WHEN** `IoEvents::OUT` is ready
- **THEN** callers MUST NOT infer that previous bytes are physically drained
- **AND** `tcdrain` and `flush` MUST continue using TX completion/TEMT semantics

#### Scenario: QEMU performance regression gate

- **WHEN** pre-change and post-change QEMU 1B latency and 64B write-plus-tcdrain are each measured for three runs
- **THEN** the post-change median MUST NOT regress by more than 10 percent
- **AND** no 10ms FIFO refill staircase, hang, or exhausted retry counter is allowed

#### Scenario: D1 line-rate regression gate

- **WHEN** Q27 is validated on D1 using the Q20 S10, S20, and S40 paths
- **THEN** 64B throughput MUST remain at least 95 percent of line rate
- **AND** 1024B throughput MUST remain at least 98 percent of line rate
- **AND** the post-change median MUST NOT regress by more than 3 percent from the same-board pre-change witness
- **AND** `slow_poll_exh` and `yield_exh` MUST remain zero

