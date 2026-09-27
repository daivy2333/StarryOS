#!/usr/bin/env python3
"""Pure-output validator for the MS08 UART SMP qualification transcript.

It never imports networking/process control, never launches QEMU, and never
opens the guest serial socket: it audits only the captured text plus the
harness's explicit result lines.  Strict checks: frozen case order and marker
grammar, placement membership, numbered payload/sequence relations against
the injected-frame contract, Full -> capacity recovery, four-stage drain,
quiet-window counter stability, RX/TX copier migration phase/from/to and
logical identity, monotonic progress counters, and a successful harness exit.
Any missing/duplicate/reordered observation, corrupt payload, role drift,
fatal guest output or nonzero harness outcome fails with a first difference.
"""
import argparse
import sys

EXPECTED_CASES = (
    "placement", "rx", "tx-full-recovery", "readiness",
    "tcdrain", "quiet", "rx-migration", "tx-migration",
)

# Per-case ordered marker grammar.  "(RX, ECHO)" repeats with the frozen count.
CASE_GRAMMAR = {
    "placement": ("SNAP",),
    "rx": ("NEED", ("RX", "ECHO", 4), "SNAP"),
    "tx-full-recovery": ("SNAP", "SNAP"),
    "readiness": ("NEED", "RX", "ECHO", "SNAP"),
    "tcdrain": ("SNAP",),
    "quiet": ("SNAP", "SNAP"),
    "rx-migration": ("SNAP", "NEED", ("RX", "ECHO", 4), "SNAP", "SNAP"),
    "tx-migration": ("SNAP", "SNAP", "SNAP"),
}

NEED_CONTRACT = {
    "rx": (4, 32),
    "readiness": (1, 16),
    "rx-migration": (4, 32),
}

MARKER_PREFIX = {
    "SNAP": "MS08_UART_SNAP: ",
    "NEED": "MS08_UART_NEED: ",
    "RX": "MS08_UART_RX: ",
    "ECHO": "MS08UARTECHO ",
}

SNAP_FIELDS = (
    "configured", "schedulable", "rx_affinity", "tx_affinity", "irq_last",
    "irq_mask", "rx_last", "rx_mask", "tx_last", "tx_mask", "rx_occ",
    "tx_vac", "ring_empty", "copier_active", "staged", "temt", "irq_events",
    "rx_polls", "tx_polls", "ipi_sent", "ipi_received", "rejects",
    "rx_mstate", "rx_mfrom", "rx_mto", "tx_mstate", "tx_mfrom", "tx_mto",
)

SIGNED_FIELDS = {"irq_last", "rx_last", "tx_last"}

FATAL_LINES = ("panic", "trap", "oops", "fatal ownership drift", "abort")


class InvalidTranscript(ValueError):
    pass


def _fields(line, prefix):
    if not line.startswith(prefix):
        raise InvalidTranscript("expected " + prefix.rstrip())
    fields = {}
    for item in line[len(prefix):].split():
        key, sep, value = item.partition("=")
        if not sep or not key or not value or key in fields:
            raise InvalidTranscript("malformed marker: " + line)
        fields[key] = value
    return fields


def _number(fields, key, signed=False):
    try:
        text = fields[key]
        value = int(text, 16) if text.startswith("0x") else int(text, 10)
    except (KeyError, ValueError):
        raise InvalidTranscript("missing or invalid numeric " + key) from None
    if not signed and (value < 0 or value > (1 << 64) - 1):
        raise InvalidTranscript("numeric overflow " + key)
    if signed and (value < -(1 << 31) or value > (1 << 31) - 1):
        raise InvalidTranscript("signed overflow " + key)
    return value


def _expect_fields(fields, **expected):
    if fields != expected:
        raise InvalidTranscript("unexpected marker fields: " + repr(fields))


def _parse_snap(line, case):
    fields = _fields(line, MARKER_PREFIX["SNAP"])
    if fields.pop("case", None) != case:
        raise InvalidTranscript("SNAP case mismatch")
    if set(fields) != set(SNAP_FIELDS):
        raise InvalidTranscript("SNAP grammar mismatch: " + repr(sorted(fields)))
    snap = {}
    for key in SNAP_FIELDS:
        snap[key] = _number(fields, key, signed=key in SIGNED_FIELDS)
    if snap["configured"] == 0 or snap["schedulable"] == 0:
        raise InvalidTranscript("degenerate topology")
    return snap


def _parse_need(line, case):
    fields = _fields(line, MARKER_PREFIX["NEED"])
    _expect_fields(fields, case=case,
                   count=fields.get("count"), len=fields.get("len"))
    count = _number(fields, "count")
    length = _number(fields, "len")
    want_count, want_len = NEED_CONTRACT[case]
    if count != want_count or length != want_len:
        raise InvalidTranscript("NEED contract mismatch for " + case)
    return count, length


def _parse_rx(line, case):
    fields = _fields(line, MARKER_PREFIX["RX"])
    _expect_fields(fields, case=case, seq=fields.get("seq"),
                   len=fields.get("len"), ok="1")
    return _number(fields, "seq"), _number(fields, "len")


def _parse_echo(line):
    rest = line[len(MARKER_PREFIX["ECHO"]):]
    try:
        seq_s, len_s, payload = rest.split(" ", 2)
        seq = int(seq_s.removeprefix("seq="), 10)
        length = int(len_s.removeprefix("len="), 10)
    except (ValueError, AttributeError):
        raise InvalidTranscript("malformed echo frame: " + line) from None
    if seq <= 0 or length <= 0 or len(payload) != length:
        raise InvalidTranscript("echo length drift: " + line)
    if any(not (0x20 < ord(c) <= 0x7E) or c == " " for c in payload):
        raise InvalidTranscript("echo payload not printable ASCII: " + line)
    return seq, length


def _schedulable(snap, hart):
    return 0 <= hart < 64 and (snap["schedulable"] & (1 << hart)) != 0


def _expand_grammar(case):
    expanded = []
    for entry in CASE_GRAMMAR[case]:
        if isinstance(entry, tuple):
            kind_a, kind_b, count = entry
            for i in range(count):
                expanded.append(kind_a)
                expanded.append(kind_b)
        else:
            expanded.append(entry)
    return expanded


def validate(lines):
    raw = [line.strip() for line in lines if line.strip()]
    for line in raw:
        lowered = line.lower()
        if any(fatal in lowered for fatal in FATAL_LINES):
            raise InvalidTranscript("fatal line present: " + line)
    body = [line for line in raw
            if line.startswith("MS08_UART") or line.startswith("MS08UARTECHO")
            or line.startswith("PASS:") or line.startswith("FAIL:")]
    if len(body) < 5 or body[0] != "MS08_UART_START":
        raise InvalidTranscript("missing start")
    if body[-1] != "MS08_UART_HARNESS_EXIT: 0":
        raise InvalidTranscript("missing successful harness exit")
    environment = body[1].removeprefix("MS08_UART_ENV: ")
    if not environment or body[1] == environment:
        raise InvalidTranscript("missing environment")
    cursor = 2
    if cursor < len(body) and body[cursor] == "MS08_UART_READY":
        cursor += 1
    observations = {}
    for case in EXPECTED_CASES:
        if cursor >= len(body) - 1 or body[cursor] != "MS08_UART_CASE_START: " + case:
            raise InvalidTranscript("missing or reordered case start: " + case)
        cursor += 1
        markers = []
        while cursor < len(body) - 1 and not body[cursor].startswith("PASS: "):
            line = body[cursor]
            if line.startswith("FAIL:"):
                raise InvalidTranscript("guest reported failure: " + line)
            if (line.startswith("MS08_UART") or
                    line.startswith("MS08UARTECHO")):
                markers.append(line)
            cursor += 1
        if cursor >= len(body) - 1 or body[cursor] != "PASS: " + case:
            raise InvalidTranscript("missing PASS: " + case)
        cursor += 1
        observations[case] = markers
    # Tail: MS08_UART_END, the harness result line, then the harness exit.
    if cursor > len(body) - 3 or body[cursor] != "MS08_UART_END":
        raise InvalidTranscript("missing end or trailing protocol line")
    cursor += 1
    if not body[cursor].startswith("MS08_UART_HARNESS_RESULT: "):
        raise InvalidTranscript("missing harness result line")
    harness = _fields(body[cursor], "MS08_UART_HARNESS_RESULT: ")
    _expect_fields(harness, outcome="ok", injected=harness.get("injected"),
                   matched=harness.get("matched"))
    injected = _number(harness, "injected")
    matched = _number(harness, "matched")
    if injected != matched or injected == 0:
        raise InvalidTranscript("harness payload accounting imbalance")
    cursor += 1
    if cursor != len(body) - 1:
        raise InvalidTranscript("trailing lines after harness result")
    _validate_protocol(observations, injected)


def _check_grammar(obs, case):
    """Every case marker must match the expanded grammar exactly (count and
    kind order); unknown or extra markers fail closed."""
    markers = obs[case]
    grammar = _expand_grammar(case)
    if len(markers) != len(grammar):
        raise InvalidTranscript(case + ": wrong marker count")
    for line, kind in zip(markers, grammar):
        if not line.startswith(MARKER_PREFIX[kind]):
            raise InvalidTranscript(case + ": marker kind mismatch, expected " +
                                    kind)


def _validate_protocol(obs, injected):
    for case in EXPECTED_CASES:
        _check_grammar(obs, case)
    placement = _snaps(obs, "placement")[0]
    if placement["rx_affinity"] == placement["tx_affinity"]:
        raise InvalidTranscript("placement: RX/TX copiers share a hart")
    for role in ("rx_affinity", "tx_affinity"):
        if not _schedulable(placement, placement[role]):
            raise InvalidTranscript("placement: affinity outside schedulable set")
    # The copier must actually have polled on its pinned hart (affinity alone
    # is not placement) with attributable progress.
    if placement["rx_last"] != placement["rx_affinity"] or \
            placement["tx_last"] != placement["tx_affinity"]:
        raise InvalidTranscript("placement: copier never polled on its pin")
    if placement["rx_polls"] == 0 or placement["tx_polls"] == 0:
        raise InvalidTranscript("placement: no copier progress")
    # Cumulative history masks are lifetime telemetry: a hart bit is legal
    # only when the current pin or this copier's own migration record
    # explains it (Widened/Restored from/to, still schedulable; Cycle 002,
    # 6.1-R1).  An in-set but unrecorded hart is rejected.
    for role, mask_key, mstate, mfrom, mto in (
            ("rx_affinity", "rx_mask", "rx_mstate", "rx_mfrom", "rx_mto"),
            ("tx_affinity", "tx_mask", "tx_mstate", "tx_mfrom", "tx_mto")):
        pin, mask = placement[role], placement[mask_key]
        if (mask & (1 << pin)) == 0:
            raise InvalidTranscript("placement: pin absent from " + mask_key)
        allowed = 1 << pin
        if placement[mstate] in (1, 2):
            for hart in (placement[mfrom], placement[mto]):
                if _schedulable(placement, hart):
                    allowed |= 1 << hart
        if mask & ~allowed:
            raise InvalidTranscript("placement: " + mask_key +
                                    " has an unexplained hart bit")
    # rx
    rx_snaps = _snaps(obs, "rx")
    _need_rx_echo(obs, "rx", 4, 32)
    if rx_snaps[-1]["rx_polls"] <= placement["rx_polls"]:
        raise InvalidTranscript("rx: no attributable RX copier progress")
    if rx_snaps[-1]["irq_events"] <= placement["irq_events"]:
        raise InvalidTranscript("rx: no attributable IRQ progress")
    # tx-full-recovery
    full, recovered = _snaps(obs, "tx-full-recovery")
    if full["tx_vac"] != 0:
        raise InvalidTranscript("tx-full-recovery: ring never reached vacancy zero")
    _drained(recovered, "tx-full-recovery")
    if recovered["tx_vac"] == 0 or recovered["tx_polls"] <= full["tx_polls"]:
        raise InvalidTranscript("tx-full-recovery: no post-drain recovery")
    # readiness
    ready = _snaps(obs, "readiness")[-1]
    _need_rx_echo(obs, "readiness", 1, 16)
    if ready["rx_polls"] <= placement["rx_polls"]:
        raise InvalidTranscript("readiness: no RX progress")
    # tcdrain
    _drained(_snaps(obs, "tcdrain")[0], "tcdrain")
    # quiet
    qa, qb = _snaps(obs, "quiet")
    for key in ("irq_events", "rx_polls", "tx_polls", "ipi_sent", "ipi_received"):
        if qa[key] != qb[key]:
            raise InvalidTranscript("quiet: counter drift on " + key)
    # migrations
    pre, mid, post = _snaps(obs, "rx-migration")
    _need_rx_echo(obs, "rx-migration", 4, 32)
    _migration(pre, mid, post, placement, rx=True)
    pre, mid, post = _snaps(obs, "tx-migration")
    _migration(pre, mid, post, placement, rx=False)
    # Monotonic progress counters across the whole session.
    previous = placement
    for case in ("rx", "tx-full-recovery", "readiness", "tcdrain", "quiet",
                 "rx-migration", "tx-migration"):
        for snap in _snaps(obs, case):
            for key in ("irq_events", "rx_polls", "tx_polls",
                        "ipi_sent", "ipi_received"):
                if snap[key] < previous[key]:
                    raise InvalidTranscript(
                        "counter rewind on " + key + " at " + case)
            previous = snap
    if injected != 9:  # rx 4 + readiness 1 + rx-migration 4
        raise InvalidTranscript("harness injected-frame accounting mismatch")


def _drained(snap, case):
    if not (snap["ring_empty"] == 1 and snap["copier_active"] == 0 and
            snap["staged"] == 0 and snap["temt"] == 1):
        raise InvalidTranscript(case + ": four-stage completion not settled")


def _snaps(obs, case):
    snaps = [_parse_snap(line, case)
             for line in obs[case] if line.startswith(MARKER_PREFIX["SNAP"])]
    grammar_snaps = sum(1 for e in _expand_grammar(case) if e == "SNAP")
    if len(snaps) != grammar_snaps:
        raise InvalidTranscript(case + ": wrong SNAP count")
    return snaps


def _need_rx_echo(obs, case, want_count, want_len):
    markers = obs[case]
    grammar = _expand_grammar(case)
    if len(markers) != len(grammar):
        raise InvalidTranscript(case + ": wrong marker count")
    rx_seqs = []
    for line, kind in zip(markers, grammar):
        if not line.startswith(MARKER_PREFIX[kind]):
            raise InvalidTranscript(case + ": reordered marker, expected " + kind)
        if kind == "NEED":
            _parse_need(line, case)
        elif kind == "RX":
            seq, length = _parse_rx(line, case)
            if length != want_len:
                raise InvalidTranscript(case + ": RX length drift")
            rx_seqs.append(seq)
        elif kind == "ECHO":
            seq, length = _parse_echo(line)
            if length != want_len:
                raise InvalidTranscript(case + ": echo length drift")
            if not rx_seqs or rx_seqs[-1] != seq:
                raise InvalidTranscript(case + ": echo/RX sequence mismatch")
            rx_seqs.pop()
    if rx_seqs:
        raise InvalidTranscript(case + ": unpaired RX/echo frames")
    flat = [k for e in grammar for k in (e if isinstance(e, tuple) else (e,))]
    # Sequence strictly 1..N across the case's RX markers.
    seqs = [_parse_rx(line, case)[0] for line in markers
            if line.startswith(MARKER_PREFIX["RX"])]
    if seqs != list(range(1, want_count + 1)):
        raise InvalidTranscript(case + ": RX sequence not 1..N")


def _migration(pre, mid, post, placement, rx):
    aff = "rx_affinity" if rx else "tx_affinity"
    last = "rx_last" if rx else "tx_last"
    polls = "rx_polls" if rx else "tx_polls"
    mstate = "rx_mstate" if rx else "tx_mstate"
    mfrom = "rx_mfrom" if rx else "tx_mfrom"
    mto = "rx_mto" if rx else "tx_mto"
    other_last = "tx_last" if rx else "rx_last"
    if pre[mstate] != 0 or mid[mstate] != 1 or post[mstate] != 2:
        raise InvalidTranscript("migration: phase chain is not 0->1->2")
    if mid[mfrom] != placement[aff] or mid[mfrom] == mid[mto]:
        raise InvalidTranscript("migration: from/to announcement broken")
    if not _schedulable(mid, mid[mto]):
        raise InvalidTranscript("migration: second hart not schedulable")
    if mid[last] != mid[mto]:
        raise InvalidTranscript("migration: copier not observed on second hart")
    if mid[other_last] != pre[other_last]:
        raise InvalidTranscript("migration: the other copier drifted")
    if mid[polls] <= pre[polls]:
        raise InvalidTranscript("migration: no poll progress on second hart")
    if post[aff] != mid[mfrom] or post[last] != mid[mfrom]:
        raise InvalidTranscript("migration: singleton pin not restored")
    if post[polls] <= mid[polls]:
        raise InvalidTranscript("migration: no progress after restore")


def canonical():
    lines = ["MS08_UART_START",
             "MS08_UART_ENV: qemu-virt-riscv64-smp16-ns16550-serial-socket",
             "MS08_UART_READY"]

    def snap(case, rx_aff=1, tx_aff=2, sched=0xffff, configured=16,
             irq_last=0, irq_mask=0x1, rx_last=1, rx_mask=0x2, tx_last=2,
             tx_mask=0x4, rx_occ=0, tx_vac=4096, ring_empty=1,
             copier_active=0, staged=0, temt=1, irq_events=10, rx_polls=20,
             tx_polls=30, ipi_sent=0, ipi_received=0, rejects=0,
             rx_mstate=0, rx_mfrom=0, rx_mto=0, tx_mstate=0, tx_mfrom=0,
             tx_mto=0):
        return ("MS08_UART_SNAP: case={} configured={} schedulable=0x{:x} "
                "rx_affinity={} tx_affinity={} irq_last={} irq_mask=0x{:x} "
                "rx_last={} rx_mask=0x{:x} tx_last={} tx_mask=0x{:x} "
                "rx_occ={} tx_vac={} ring_empty={} copier_active={} staged={} "
                "temt={} irq_events={} rx_polls={} tx_polls={} ipi_sent={} "
                "ipi_received={} rejects={} rx_mstate={} rx_mfrom={} "
                "rx_mto={} tx_mstate={} tx_mfrom={} tx_mto={}").format(
            case, configured, sched, rx_aff, tx_aff, irq_last, irq_mask,
            rx_last, rx_mask, tx_last, tx_mask, rx_occ, tx_vac, ring_empty,
            copier_active, staged, temt, irq_events, rx_polls, tx_polls,
            ipi_sent, ipi_received, rejects, rx_mstate, rx_mfrom, rx_mto,
            tx_mstate, tx_mfrom, tx_mto)

    payload = "abcdefghijklmnopqrstuvwxyz012345"  # 32 printable chars
    lines.append("MS08_UART_CASE_START: placement")
    lines.append(snap("placement"))
    lines.append("PASS: placement")
    lines.append("MS08_UART_CASE_START: rx")
    lines.append("MS08_UART_NEED: case=rx count=4 len=32")
    for seq in range(1, 5):
        lines.append(f"MS08_UART_RX: case=rx seq={seq} len=32 ok=1")
        lines.append(f"MS08UARTECHO seq={seq} len=32 {payload}")
    lines.append(snap("rx", irq_events=12, rx_polls=26))
    lines.append("PASS: rx")
    lines.append("MS08_UART_CASE_START: tx-full-recovery")
    lines.append(snap("tx-full-recovery", tx_vac=0, ring_empty=0, temt=0,
                      irq_events=12, rx_polls=26, tx_polls=40))
    lines.append(snap("tx-full-recovery", irq_events=12, rx_polls=26, tx_polls=52))
    lines.append("PASS: tx-full-recovery")
    lines.append("MS08_UART_CASE_START: readiness")
    lines.append("MS08_UART_NEED: case=readiness count=1 len=16")
    short = "abcdefghijklmnop"
    lines.append("MS08_UART_RX: case=readiness seq=1 len=16 ok=1")
    lines.append(f"MS08UARTECHO seq=1 len=16 {short}")
    lines.append(snap("readiness", irq_events=13, rx_polls=28, tx_polls=52))
    lines.append("PASS: readiness")
    lines.append("MS08_UART_CASE_START: tcdrain")
    lines.append(snap("tcdrain", irq_events=13, rx_polls=28, tx_polls=52))
    lines.append("PASS: tcdrain")
    lines.append("MS08_UART_CASE_START: quiet")
    lines.append(snap("quiet", irq_events=13, rx_polls=28, tx_polls=52))
    lines.append(snap("quiet", irq_events=13, rx_polls=28, tx_polls=52))
    lines.append("PASS: quiet")
    lines.append("MS08_UART_CASE_START: rx-migration")
    lines.append(snap("rx-migration", irq_events=13, rx_polls=28, tx_polls=52))
    lines.append("MS08_UART_NEED: case=rx-migration count=4 len=32")
    for seq in range(1, 5):
        lines.append(f"MS08_UART_RX: case=rx-migration seq={seq} len=32 ok=1")
        lines.append(f"MS08UARTECHO seq={seq} len=32 {payload}")
    lines.append(snap("rx-migration", irq_events=17, rx_polls=40, tx_polls=52,
                      rx_last=3, rx_mask=0xa, rx_mstate=1, rx_mfrom=1, rx_mto=3))
    lines.append(snap("rx-migration", irq_events=17, rx_polls=42, tx_polls=54,
                      rx_mstate=2, rx_mfrom=1, rx_mto=3))
    lines.append("PASS: rx-migration")
    lines.append("MS08_UART_CASE_START: tx-migration")
    lines.append(snap("tx-migration", irq_events=17, rx_polls=42, tx_polls=54,
                      rx_mstate=2, rx_mfrom=1, rx_mto=3))
    lines.append(snap("tx-migration", irq_events=18, rx_polls=43, tx_polls=60,
                      tx_last=5, tx_mask=0x24, tx_mstate=1, tx_mfrom=2, tx_mto=5))
    lines.append(snap("tx-migration", irq_events=18, rx_polls=43, tx_polls=62,
                      tx_mstate=2, tx_mfrom=2, tx_mto=5))
    lines.append("PASS: tx-migration")
    lines.append("MS08_UART_END")
    lines.append("MS08_UART_HARNESS_RESULT: outcome=ok injected=9 matched=9")
    lines.append("MS08_UART_HARNESS_EXIT: 0")
    return lines


def schema_lines():
    # Must match tests/ms08_uart_smp_probe.c --print-schema byte for byte:
    # full marker names, full emission sequence including repeated SNAPs.
    return [
        "placement:MS08_UART_SNAP",
        "rx:MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP",
        "tx-full-recovery:MS08_UART_SNAP,MS08_UART_SNAP",
        "readiness:MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP",
        "tcdrain:MS08_UART_SNAP",
        "quiet:MS08_UART_SNAP,MS08_UART_SNAP",
        "rx-migration:MS08_UART_SNAP,MS08_UART_NEED,MS08_UART_RX,MS08UARTECHO,MS08_UART_SNAP,MS08_UART_SNAP",
        "tx-migration:MS08_UART_SNAP,MS08_UART_SNAP,MS08_UART_SNAP",
    ]


def _rejects(func, label):
    try:
        func()
    except InvalidTranscript:
        return
    raise AssertionError("negative fixture accepted: " + label)


def _mutate(lines, pos, key, newval):
    prefix, rest = lines[pos].split(" ", 1)
    out = []
    found = False
    for tok in rest.split():
        if tok.startswith(key + "="):
            out.append(f"{key}={newval}")
            found = True
        else:
            out.append(tok)
    if not found:
        raise AssertionError("key missing from mutated line: " + key)
    return prefix + " " + " ".join(out)


def self_test():
    valid = canonical()
    validate(valid)
    validate(["boot noise"] + valid + ["shutdown noise"])

    # Environment is diagnostic only: a behaviorally valid transcript with
    # any nonempty environment text must be accepted (5.3-R1).
    env_idx = next(i for i, line in enumerate(valid)
                   if line.startswith("MS08_UART_ENV: "))
    other_env = list(valid)
    other_env[env_idx] = "MS08_UART_ENV: handwritten-operator-notes"
    validate(other_env)

    # Envelope: missing exit / end / environment / harness result.
    _rejects(lambda: validate(valid[:-1]), "missing harness exit")
    _rejects(lambda: validate(valid[:-3] + valid[-2:]), "missing end")
    _rejects(lambda: validate(valid[:1] + ["MS08_UART_ENV: "] + valid[1:]),
             "empty environment")
    no_result = [line for line in valid
                 if not line.startswith("MS08_UART_HARNESS_RESULT: ")]
    _rejects(lambda: validate(no_result), "missing harness result")

    # Structure: reordered / duplicated / missing case, unknown PASS, FAIL,
    # foreign marker, fatal line.
    _rejects(lambda: validate(valid[:3] + ["PASS: bogus"] + valid[3:]),
             "unknown PASS")
    swapped = list(valid)
    idx_a = swapped.index("PASS: placement")
    idx_b = swapped.index("PASS: rx")
    swapped[idx_a], swapped[idx_b] = swapped[idx_b], swapped[idx_a]
    _rejects(lambda: validate(swapped), "reordered case")
    dup = list(valid)
    snap_idx = next(i for i, line in enumerate(dup)
                    if line.startswith("MS08_UART_SNAP: case=placement"))
    dup.insert(snap_idx + 1, dup[snap_idx])
    _rejects(lambda: validate(dup), "duplicate placement SNAP")
    _rejects(lambda: validate(valid[:4] + ["FAIL: rx reason=io"] + valid[4:]),
             "embedded FAIL")
    _rejects(lambda: validate(valid[:4] + ["MS08_UART_UNKNOWN: nope"] + valid[4:]),
             "foreign marker")
    _rejects(lambda: validate(valid[:4] + ["kernel panic: test"] + valid[4:]),
             "fatal line")

    # Corrupt payload: echo length drift, non-printable echo, RX/echo mismatch.
    lines = list(valid)
    for i, line in enumerate(lines):
        if line.startswith("MS08UARTECHO seq=2 "):
            lines[i] = "MS08UARTECHO seq=2 len=31 " + "a" * 31
            break
    _rejects(lambda: validate(lines), "echo length drift")
    lines = list(valid)
    for i, line in enumerate(lines):
        if line.startswith("MS08UARTECHO seq=1 len=32"):
            lines[i] = "MS08UARTECHO seq=1 len=32 " + "a" * 31 + " "
            break
    _rejects(lambda: validate(lines), "echo payload length mismatch")
    lines = list(valid)
    for i, line in enumerate(lines):
        if line.startswith("MS08UARTECHO seq=3 "):
            lines[i] = line.replace("MS08UARTECHO seq=3", "MS08UARTECHO seq=4")
            break
    _rejects(lambda: validate(lines), "echo/RX sequence mismatch")

    # Placement violations.
    snap_idx = next(i for i, line in enumerate(valid)
                    if line.startswith("MS08_UART_SNAP: case=placement"))
    for key, val in (("rx_affinity", 2), ("tx_affinity", 17)):
        lines = list(valid)
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key)
    # Copier never polled on its pin / no progress / unknown mask bit / pin
    # absent from its history mask are all placement failures.
    for key, val in (("rx_last", 3), ("tx_last", 1)):
        lines = list(valid)
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key + " off pin")
    for key, val in (("rx_polls", 0), ("tx_polls", 0)):
        lines = list(valid)
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key + " zero")
    for key, val in (("rx_mask", 0x1), ("tx_mask", 0x2)):
        lines = list(valid)
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key + " missing pin")
    for key, val in (("rx_mask", 0x10002), ("tx_mask", 0x20000)):
        lines = list(valid)
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key + " out of schedulable")
    # Cycle 002 (6.1-R1): an in-set but unrecorded history bit is rejected.
    lines = list(valid)
    lines[snap_idx] = _mutate(lines, snap_idx, "rx_mask", 0x6)
    _rejects(lambda: validate(lines), "placement unrecorded in-set hart")
    # Recorded boot migration history (Restored from/to) explains extra bits.
    lines = list(valid)
    for key, val in (("rx_mstate", 2), ("rx_mfrom", 1), ("rx_mto", 3),
                     ("rx_mask", 0xa)):
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
    validate(lines)
    # A recorded hart outside the schedulable set still explains nothing.
    lines = list(valid)
    for key, val in (("rx_mstate", 2), ("rx_mfrom", 1), ("rx_mto", 17),
                     ("rx_mask", 0x20002)):
        lines[snap_idx] = _mutate(lines, snap_idx, key, val)
    _rejects(lambda: validate(lines),
             "placement recorded hart outside schedulable")

    # tx-full-recovery: never full / no recovery progress.
    full_idx = next(i for i, line in enumerate(valid)
                    if line.startswith("MS08_UART_SNAP: case=tx-full-recovery")
                    and "tx_vac=0" in line)
    after_idx = next(i for i, line in enumerate(valid)
                     if line.startswith("MS08_UART_SNAP: case=tx-full-recovery")
                     and "tx_vac=4096" in line)
    lines = list(valid)
    lines[full_idx] = _mutate(lines, full_idx, "tx_vac", 1)
    _rejects(lambda: validate(lines), "full-recovery never full")
    lines = list(valid)
    lines[after_idx] = _mutate(lines, after_idx, "tx_polls", 40)
    _rejects(lambda: validate(lines), "full-recovery no progress")

    # quiet: counter drift.
    quiet_idx = next(i for i, line in enumerate(valid)
                     if line.startswith("MS08_UART_SNAP: case=quiet"))
    lines = list(valid)
    lines[quiet_idx + 1] = _mutate(lines, quiet_idx + 1, "rx_polls", 29)
    _rejects(lambda: validate(lines), "quiet counter drift")

    # Migration: role drift (TX migration moving the RX copier), wrong phase
    # chain, second hart not schedulable, pin not restored.
    lines = list(valid)
    for i, line in enumerate(lines):
        if "case=tx-migration" in line and "tx_mstate=1" in line:
            lines[i] = _mutate(lines, i, "rx_last", 5)
            break
    _rejects(lambda: validate(lines), "tx-migration RX copier drift")
    lines = list(valid)
    for i, line in enumerate(lines):
        if "case=rx-migration" in line and "rx_mstate=2" in line:
            lines[i] = _mutate(lines, i, "rx_mstate", 1)
            break
    _rejects(lambda: validate(lines), "rx-migration phase not restored")
    lines = list(valid)
    for i, line in enumerate(lines):
        if "case=rx-migration" in line and "rx_mstate=1" in line:
            lines[i] = _mutate(lines, i, "rx_mto", 40)
            break
    _rejects(lambda: validate(lines), "rx-migration second hart unschedulable")
    lines = list(valid)
    for i, line in enumerate(lines):
        if "case=tx-migration" in line and "tx_mstate=2" in line:
            lines[i] = _mutate(lines, i, "tx_last", 5)
            break
    _rejects(lambda: validate(lines), "tx-migration pin not restored")

    # Harness accounting: imbalance or wrong frame total.
    lines = list(valid)
    lines[-2] = "MS08_UART_HARNESS_RESULT: outcome=ok injected=9 matched=8"
    _rejects(lambda: validate(lines), "harness imbalance")
    lines = list(valid)
    lines[-2] = "MS08_UART_HARNESS_RESULT: outcome=ok injected=4 matched=4"
    _rejects(lambda: validate(lines), "harness frame total mismatch")
    lines = list(valid)
    lines[-2] = "MS08_UART_HARNESS_RESULT: outcome=timeout injected=9 matched=9"
    _rejects(lambda: validate(lines), "harness timeout outcome")

    # Monotonic counters: a counter rewind across cases is rejected.
    lines = list(valid)
    tcdrain_idx = next(i for i, line in enumerate(lines)
                       if line.startswith("MS08_UART_SNAP: case=tcdrain"))
    lines[tcdrain_idx] = _mutate(lines, tcdrain_idx, "irq_events", 1)
    _rejects(lambda: validate(lines), "counter rewind")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--print-cases", action="store_true")
    parser.add_argument("--print-schema", action="store_true")
    parser.add_argument("transcript", nargs="?")
    args = parser.parse_args()
    if args.print_cases:
        print("\n".join(EXPECTED_CASES))
        return 0
    if args.print_schema:
        print("\n".join(schema_lines()))
        return 0
    if args.self_test:
        self_test()
        return 0
    if not args.transcript:
        parser.error("transcript is required unless --self-test, --print-cases, or --print-schema")
    with open(args.transcript, encoding="utf-8") as source:
        validate(source)
    return 0


if __name__ == "__main__":
    sys.exit(main())
