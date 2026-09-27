#!/usr/bin/env python3
"""Pure-output validator for the MS08 network SMP qualification transcript.

It never imports networking/process control, never launches QEMU and never
drives HMP: it audits only captured text plus explicit result lines.  Strict
checks (Iteration 005 final profile: six network cases): frozen case order and
marker grammar, V5 snapshot relationships (placement singletons,
timer-disabled wake IPI causality, Full -> capacity recovery with
slot/ticket conservation, quiet-window stability), peer accounting, and a
successful harness exit.  Any missing/duplicate/reordered observation,
timer-only progress, role drift, resource imbalance, corrupt payload, fatal
guest output or nonzero harness outcome fails with a first difference.
"""
import argparse
import sys

EXPECTED_CASES = (
    "placement", "timer-disabled-wake", "tcp-bidirectional",
    "udp-bidirectional", "full-recovery", "readiness-quiet",
)

# Cases with peer traffic; the host peer's independent terminal result is
# joined after the harness exit line and is required for acceptance.
PEER_CASES = (
    "tcp-bidirectional", "udp-bidirectional", "full-recovery",
    "readiness-quiet",
)

# Per-case ordered marker grammar (V5 = snapshot, expanded in place).
CASE_GRAMMAR = {
    "placement": ("V5",),
    "timer-disabled-wake": ("V5", "V5", "WAKE"),
    "tcp-bidirectional": ("V5", "SOCK", "PEER"),
    "udp-bidirectional": ("V5", "SOCK", "PEER"),
    "full-recovery": ("V5", "DIAG", "DIAG", "V5", "SOCK", "PEER"),
    "readiness-quiet": ("V5", "SOCK", "PEER"),
}

MARKER_PREFIX = {
    "V5": "MS08_NET_V5: ",
    "WAKE": "MS08_NET_WAKE: ",
    "DIAG": "MS08_NET_DIAG: ",
    "SOCK": "MS08_NET_SOCK: ",
    "PEER": "MS08_NET_PEER: ",
}

V5_FIELDS = (
    "configured", "schedulable", "owner_aff", "runner_aff", "irq_last",
    "irq_mask", "irq_events", "owner_last", "owner_mask", "owner_events",
    "runner_last", "runner_mask", "runner_events", "ipi_sent", "ipi_received",
    "rejects", "own_mig", "run_mig", "wphase", "wcompleted", "wfailed",
    "wtimer_restored", "wstart_rejects", "wmissing_restore",
    "wtrigger_rejects", "willegal", "wlast_target", "wlast_trigger",
    "wipi_before", "wipi_after", "lifecycle", "q", "s", "l", "link",
    "available", "device_owned", "quarantined", "fault_valid", "fault_stage",
    "rx_occ", "tx_occ", "tx_enq", "tx_deq", "tx_submit", "tx_again",
    "tx_completion", "tx_reclaim", "tx_buf_avail", "tx_buf_inflight",
    "tx_desc_avail", "tx_desc_inflight", "live", "task_poll",
    "queue_generation",
)

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


def _number(fields, key):
    try:
        text = fields[key]
        value = int(text, 16) if text.startswith("0x") else int(text, 10)
    except (KeyError, ValueError):
        raise InvalidTranscript("missing or invalid numeric " + key) from None
    if value < 0 or value > (1 << 64) - 1:
        raise InvalidTranscript("numeric overflow " + key)
    return value


def _expect_fields(fields, **expected):
    if fields != expected:
        raise InvalidTranscript("unexpected marker fields: " + repr(fields))


def _parse_v5(line, case):
    fields = _fields(line, MARKER_PREFIX["V5"])
    if fields.pop("case", None) != case:
        raise InvalidTranscript("V5 case mismatch")
    if set(fields) != set(V5_FIELDS):
        raise InvalidTranscript("V5 grammar mismatch: " + repr(sorted(fields)))
    link = fields.pop("link")
    if link not in ("up", "down"):
        raise InvalidTranscript("V5 link must be up or down")
    snap = {key: _number(fields, key) for key in V5_FIELDS if key != "link"}
    snap["link"] = link
    if snap["configured"] == 0 or snap["schedulable"] == 0:
        raise InvalidTranscript("degenerate topology")
    if snap["lifecycle"] != 2:
        raise InvalidTranscript("V5 observation must be Active")
    if snap["fault_valid"] == 0 and snap["fault_stage"] != 0:
        raise InvalidTranscript("absent fault tuple must be all zero")
    return snap


def _schedulable(snap, hart):
    return 0 <= hart < 64 and (snap["schedulable"] & (1 << hart)) != 0


def _mig_phase(packed):
    return packed & 0xff


def _mig_from(packed):
    return (packed >> 8) & 0xffffff


def _mig_to(packed):
    return (packed >> 32) & 0xffffff


def validate(lines):
    raw = [line.strip() for line in lines if line.strip()]
    for line in raw:
        lowered = line.lower()
        if any(fatal in lowered for fatal in FATAL_LINES):
            raise InvalidTranscript("fatal line present: " + line)
    body = [line for line in raw
            if line.startswith("MS08_NET") or line.startswith("PASS:")
            or line.startswith("FAIL:")]
    if len(body) < 6 or body[0] != "MS08_NET_START":
        raise InvalidTranscript("missing start")
    # The host peer's independent terminal result is joined after the guest
    # transcript and is required for acceptance; the guest-authored
    # MS08_NET_PEER marker is context, never evidence.
    peer_counts = _parse_peer_result(body[-1])
    if body[-2] != "MS08_NET_HARNESS_EXIT: 0":
        raise InvalidTranscript("missing successful harness exit")
    environment = body[1].removeprefix("MS08_NET_ENV: ")
    if not environment or body[1] == environment:
        raise InvalidTranscript("missing environment")
    cursor = 2
    observations = {}
    for case in EXPECTED_CASES:
        if cursor >= len(body) - 1 or body[cursor] != "MS08_NET_CASE_START: " + case:
            raise InvalidTranscript("missing or reordered case start: " + case)
        cursor += 1
        markers = []
        while cursor < len(body) - 1 and not body[cursor].startswith("PASS: "):
            line = body[cursor]
            if line.startswith("FAIL:"):
                raise InvalidTranscript("guest reported failure: " + line)
            if line.startswith("MS08_NET"):
                markers.append(line)
            cursor += 1
        if cursor >= len(body) - 1 or body[cursor] != "PASS: " + case:
            raise InvalidTranscript("missing PASS: " + case)
        cursor += 1
        observations[case] = markers
    if cursor != len(body) - 3 or body[cursor] != "MS08_NET_END":
        raise InvalidTranscript("missing end or trailing protocol line")
    _validate_protocol(observations, peer_counts)


def _check_grammar(obs, case):
    markers = obs[case]
    grammar = CASE_GRAMMAR[case]
    if len(markers) != len(grammar):
        raise InvalidTranscript(case + ": wrong marker count")
    for line, kind in zip(markers, grammar):
        if not line.startswith(MARKER_PREFIX[kind]):
            raise InvalidTranscript(case + ": marker kind mismatch, expected " +
                                    kind)


def _v5s(obs, case):
    snaps = [_parse_v5(line, case)
             for line in obs[case] if line.startswith(MARKER_PREFIX["V5"])]
    grammar_v5 = sum(1 for e in CASE_GRAMMAR[case] if e == "V5")
    if len(snaps) != grammar_v5:
        raise InvalidTranscript(case + ": wrong V5 count")
    return snaps


def _sock(obs, case, terminal):
    fields = _fields([line for line in obs[case]
                      if line.startswith(MARKER_PREFIX["SOCK"])][0],
                     MARKER_PREFIX["SOCK"])
    if fields.pop("case", None) != case:
        raise InvalidTranscript("SOCK case mismatch")
    if terminal is None:
        if fields.pop("result", None) != "rw":
            raise InvalidTranscript("SOCK result must be rw")
    else:
        if fields.pop("terminal", None) != terminal:
            raise InvalidTranscript("SOCK terminal mismatch")
    sent = _number(fields, "sent")
    received = _number(fields, "received")
    if sent == 0 or received == 0 or sent != received:
        raise InvalidTranscript("SOCK traffic accounting mismatch")
    fields.pop("sent")
    fields.pop("received")
    if fields:
        raise InvalidTranscript("SOCK grammar mismatch: " + repr(fields))
    return sent, received


def _peer_accounting(case, peer_counts, sent, received):
    """Acceptance comes from the independent host peer: its received/echoed
    counts must equal the guest's successful exchanges exactly."""
    prx, ptx = peer_counts[case]
    if prx != sent or ptx != received:
        raise InvalidTranscript(case + ": guest/peer accounting mismatch")


def _peer(obs, case):
    for line in obs[case]:
        if line.startswith(MARKER_PREFIX["PEER"]):
            fields = _fields(line, MARKER_PREFIX["PEER"])
            _expect_fields(fields, case=case, result="ok")
            return
    raise InvalidTranscript("missing PEER marker for " + case)


def _healthy_owner(snap):
    if not (snap["available"] == snap["device_owned"] and
            snap["quarantined"] == 0):
        raise InvalidTranscript("owner not at the healthy ledger baseline")


def _validate_protocol(obs, peer_counts):
    for case in EXPECTED_CASES:
        _check_grammar(obs, case)
    placement = _v5s(obs, "placement")[0]
    if placement["owner_aff"] == placement["runner_aff"]:
        raise InvalidTranscript("placement: owner and runner share a hart")
    for role in ("owner_aff", "runner_aff"):
        if not _schedulable(placement, placement[role]):
            raise InvalidTranscript("placement: affinity outside schedulable set")
    if placement["owner_last"] != placement["owner_aff"]:
        raise InvalidTranscript("placement: owner observed off its pin")
    if placement["runner_last"] != placement["runner_aff"]:
        raise InvalidTranscript("placement: runner observed off its pin")
    if placement["owner_events"] == 0 or placement["runner_events"] == 0:
        raise InvalidTranscript("placement: no role progress")
    # The masks are cumulative lifetime telemetry, not the current singleton:
    # a hart bit is legal only when the current pin or the role's own
    # migration record explains it (Widened/Restored from/to, still
    # schedulable; Cycle 002, 6.2-R1).  An in-set but unrecorded hart is
    # rejected.
    for role, mask_key, packed_key in (("owner_aff", "owner_mask", "own_mig"),
                                       ("runner_aff", "runner_mask", "run_mig")):
        pin, mask = placement[role], placement[mask_key]
        if (mask & (1 << pin)) == 0:
            raise InvalidTranscript("placement: pin absent from " + mask_key)
        allowed = 1 << pin
        packed = placement[packed_key]
        if _mig_phase(packed) in (1, 2):
            for hart in (_mig_from(packed), _mig_to(packed)):
                if _schedulable(placement, hart):
                    allowed |= 1 << hart
        if mask & ~allowed:
            raise InvalidTranscript("placement: " + mask_key +
                                    " has an unexplained hart bit")
    _healthy_owner(placement)

    # timer-disabled-wake
    wake_before, wake_after = _v5s(obs, "timer-disabled-wake")
    fields = _fields(obs["timer-disabled-wake"][2], MARKER_PREFIX["WAKE"])
    _expect_fields(fields, case="timer-disabled-wake",
                   target=fields.get("target"), trigger=fields.get("trigger"),
                   ipi_before=fields.get("ipi_before"),
                   ipi_after=fields.get("ipi_after"),
                   completed=fields.get("completed"),
                   timer_restored=fields.get("timer_restored"),
                   missing_restore="0")
    target = _number(fields, "target")
    trigger = _number(fields, "trigger")
    if wake_after["wcompleted"] != wake_before["wcompleted"] + 1:
        raise InvalidTranscript("wake: not exactly one completed run")
    if wake_after["wtimer_restored"] != wake_before["wtimer_restored"] + 1:
        raise InvalidTranscript("wake: timer restoration not acknowledged")
    if wake_after["wmissing_restore"] != 0:
        raise InvalidTranscript("wake: terminal without timer restore")
    if target != wake_after["wlast_target"]:
        raise InvalidTranscript("wake: target hart drift")
    if trigger != wake_after["wlast_trigger"] or trigger == target:
        raise InvalidTranscript("wake: trigger was not remote")
    if not _schedulable(wake_after, trigger):
        raise InvalidTranscript("wake: trigger hart not schedulable")
    if not (_number(fields, "ipi_after") > _number(fields, "ipi_before") and
            wake_after["wipi_after"] > wake_after["wipi_before"]):
        raise InvalidTranscript("wake: no IPI causality (timer-only progress)")

    # bidirectional
    for case in ("tcp-bidirectional", "udp-bidirectional"):
        after = _v5s(obs, case)[0]
        if after["tx_submit"] <= placement["tx_submit"]:
            raise InvalidTranscript(case + ": no TX progress")
        sent, received = _sock(obs, case, None)
        _peer_accounting(case, peer_counts, sent, received)
        _peer(obs, case)

    # full-recovery
    full, recovered = _v5s(obs, "full-recovery")
    diags = [_fields(line, MARKER_PREFIX["DIAG"])
             for line in obs["full-recovery"]
             if line.startswith(MARKER_PREFIX["DIAG"])]
    _expect_fields(diags[0], case="full-recovery", op="hold-submit",
                   result="ok")
    _expect_fields(diags[1], case="full-recovery", op="release", result="ok")
    if full["tx_again"] == 0 and full["tx_occ"] == 0:
        raise InvalidTranscript("full-recovery: no capacity pressure evidence")
    if (recovered["tx_buf_inflight"] != 0 or
            recovered["tx_desc_inflight"] != 0 or recovered["live"] != 0):
        raise InvalidTranscript("full-recovery: resources left inflight")
    if not (recovered["tx_submit"] == recovered["tx_completion"] ==
            recovered["tx_reclaim"]):
        raise InvalidTranscript("full-recovery: ticket ledger imbalance")
    if recovered["tx_enq"] - recovered["tx_deq"] != recovered["tx_occ"]:
        raise InvalidTranscript("full-recovery: slot ledger imbalance")
    if recovered["tx_submit"] <= full["tx_submit"]:
        raise InvalidTranscript("full-recovery: no post-release progress")
    sent, received = _sock(obs, "full-recovery", None)
    _peer_accounting("full-recovery", peer_counts, sent, received)
    _peer(obs, "full-recovery")

    # readiness-quiet
    quiet = _v5s(obs, "readiness-quiet")[0]
    if quiet["task_poll"] <= placement["task_poll"]:
        raise InvalidTranscript("readiness-quiet: no stack progress")
    sent, received = _sock(obs, "readiness-quiet", None)
    _peer_accounting("readiness-quiet", peer_counts, sent, received)
    _peer(obs, "readiness-quiet")

    # The historical fault tuple is frozen for the whole session.
    fault = (placement["fault_valid"], placement["fault_stage"])
    for case in EXPECTED_CASES:
        for snap in _v5s(obs, case):
            if (snap["fault_valid"], snap["fault_stage"]) != fault:
                raise InvalidTranscript("fault tuple drifted at " + case)

    # Monotonic counters across the whole session.
    previous = placement
    for case in EXPECTED_CASES:
        for snap in _v5s(obs, case):
            for key in ("irq_events", "owner_events", "runner_events",
                        "ipi_sent", "ipi_received", "task_poll", "tx_submit",
                        "tx_completion", "tx_reclaim", "tx_enq", "tx_deq"):
                if snap[key] < previous[key]:
                    raise InvalidTranscript("counter rewind on " + key +
                                            " at " + case)
            previous = snap




def _v5(case, **kw):
    base = dict(
        configured=16, schedulable=0xffff, owner_aff=2, runner_aff=3,
        irq_last=2, irq_mask=0x4, irq_events=100, owner_last=2,
        owner_mask=0x4, owner_events=500, runner_last=3, runner_mask=0x8,
        runner_events=700, ipi_sent=9, ipi_received=9, rejects=0,
        own_mig=0, run_mig=0, wphase=0, wcompleted=0, wfailed=0,
        wtimer_restored=0, wstart_rejects=0, wmissing_restore=0,
        wtrigger_rejects=0, willegal=0, wlast_target=0, wlast_trigger=0,
        wipi_before=0, wipi_after=0, lifecycle=2, q=7, s=11, l=13,
        link="up", available=64, device_owned=64, quarantined=0,
        fault_valid=0, fault_stage=0, rx_occ=0, tx_occ=0, tx_enq=40,
        tx_deq=40, tx_submit=40, tx_again=0, tx_completion=40,
        tx_reclaim=40, tx_buf_avail=64, tx_buf_inflight=0,
        tx_desc_avail=64, tx_desc_inflight=0, live=0, task_poll=300,
        queue_generation=4,
    )
    base.update(kw)
    return ("MS08_NET_V5: case={case} configured={configured} "
            "schedulable=0x{schedulable:x} owner_aff={owner_aff} "
            "runner_aff={runner_aff} irq_last={irq_last} "
            "irq_mask=0x{irq_mask:x} irq_events={irq_events} "
            "owner_last={owner_last} owner_mask=0x{owner_mask:x} "
            "owner_events={owner_events} runner_last={runner_last} "
            "runner_mask=0x{runner_mask:x} runner_events={runner_events} "
            "ipi_sent={ipi_sent} ipi_received={ipi_received} "
            "rejects={rejects} own_mig=0x{own_mig:x} run_mig=0x{run_mig:x} "
            "wphase={wphase} wcompleted={wcompleted} wfailed={wfailed} "
            "wtimer_restored={wtimer_restored} "
            "wstart_rejects={wstart_rejects} "
            "wmissing_restore={wmissing_restore} "
            "wtrigger_rejects={wtrigger_rejects} willegal={willegal} "
            "wlast_target={wlast_target} wlast_trigger={wlast_trigger} "
            "wipi_before={wipi_before} wipi_after={wipi_after} "
            "lifecycle={lifecycle} q={q} s={s} l={l} link={link} "
            "available={available} device_owned={device_owned} "
            "quarantined={quarantined} fault_valid={fault_valid} "
            "fault_stage={fault_stage} rx_occ={rx_occ} tx_occ={tx_occ} "
            "tx_enq={tx_enq} tx_deq={tx_deq} tx_submit={tx_submit} "
            "tx_again={tx_again} tx_completion={tx_completion} "
            "tx_reclaim={tx_reclaim} tx_buf_avail={tx_buf_avail} "
            "tx_buf_inflight={tx_buf_inflight} "
            "tx_desc_avail={tx_desc_avail} "
            "tx_desc_inflight={tx_desc_inflight} live={live} "
            "task_poll={task_poll} "
            "queue_generation={queue_generation}").format(case=case, **base)


def _mig_view(phase, frm, to):
    return phase | ((frm & 0xffffff) << 8) | ((to & 0xffffff) << 32)


def canonical():
    lines = ["MS08_NET_START",
             "MS08_NET_ENV: qemu-virt-riscv64-smp16-virtio-mmio-user-net"]
    lines.append("MS08_NET_CASE_START: placement")
    lines.append(_v5("placement"))
    lines.append("PASS: placement")
    lines.append("MS08_NET_CASE_START: timer-disabled-wake")
    lines.append(_v5("timer-disabled-wake"))
    lines.append(_v5("timer-disabled-wake", wcompleted=1, wtimer_restored=1,
                     wlast_target=0, wlast_trigger=9, wipi_before=3,
                     wipi_after=4, ipi_sent=10, ipi_received=10))
    lines.append("MS08_NET_WAKE: case=timer-disabled-wake target=0 trigger=9 "
                 "ipi_before=3 ipi_after=4 completed=1 timer_restored=1 "
                 "missing_restore=0")
    lines.append("PASS: timer-disabled-wake")
    lines.append("MS08_NET_CASE_START: tcp-bidirectional")
    lines.append(_v5("tcp-bidirectional", irq_events=110, owner_events=520,
                     runner_events=730, ipi_sent=11, ipi_received=11,
                     task_poll=330, tx_enq=48, tx_deq=48, tx_submit=48,
                     tx_completion=48, tx_reclaim=48))
    lines.append("MS08_NET_SOCK: case=tcp-bidirectional result=rw sent=8 "
                 "received=8")
    lines.append("MS08_NET_PEER: case=tcp-bidirectional result=ok")
    lines.append("PASS: tcp-bidirectional")
    lines.append("MS08_NET_CASE_START: udp-bidirectional")
    lines.append(_v5("udp-bidirectional", irq_events=120, owner_events=540,
                     runner_events=760, ipi_sent=12, ipi_received=12,
                     task_poll=360, tx_enq=56, tx_deq=56, tx_submit=56,
                     tx_completion=56, tx_reclaim=56))
    lines.append("MS08_NET_SOCK: case=udp-bidirectional result=rw sent=8 "
                 "received=8")
    lines.append("MS08_NET_PEER: case=udp-bidirectional result=ok")
    lines.append("PASS: udp-bidirectional")
    lines.append("MS08_NET_CASE_START: full-recovery")
    lines.append(_v5("full-recovery", irq_events=130, owner_events=560,
                     runner_events=790, ipi_sent=13, ipi_received=13,
                     task_poll=390, tx_enq=76, tx_deq=60, tx_submit=76,
                     tx_again=4, tx_completion=60, tx_reclaim=60,
                     tx_occ=16, tx_buf_inflight=16, tx_desc_inflight=16,
                     live=16))
    lines.append("MS08_NET_DIAG: case=full-recovery op=hold-submit result=ok")
    lines.append("MS08_NET_DIAG: case=full-recovery op=release result=ok")
    lines.append(_v5("full-recovery", irq_events=140, owner_events=580,
                     runner_events=820, ipi_sent=14, ipi_received=14,
                     task_poll=420, tx_enq=80, tx_deq=80, tx_submit=80,
                     tx_again=4, tx_completion=80, tx_reclaim=80))
    lines.append("MS08_NET_SOCK: case=full-recovery result=rw sent=32 "
                 "received=32")
    lines.append("MS08_NET_PEER: case=full-recovery result=ok")
    lines.append("PASS: full-recovery")
    lines.append("MS08_NET_CASE_START: readiness-quiet")
    lines.append(_v5("readiness-quiet", irq_events=140, owner_events=580,
                     runner_events=820, ipi_sent=14, ipi_received=14,
                     task_poll=420, tx_enq=80, tx_deq=80, tx_submit=80,
                     tx_again=4, tx_completion=80, tx_reclaim=80))
    lines.append("MS08_NET_SOCK: case=readiness-quiet result=rw sent=1 "
                 "received=1")
    lines.append("MS08_NET_PEER: case=readiness-quiet result=ok")
    lines.append("PASS: readiness-quiet")
    lines.append("MS08_NET_END")
    lines.append("MS08_NET_HARNESS_EXIT: 0")
    lines.append(_peer_result_line({
        "tcp-bidirectional": (8, 8), "udp-bidirectional": (8, 8),
        "full-recovery": (32, 32), "readiness-quiet": (1, 1),
    }))
    return lines


def _peer_result_line(counts):
    """The host peer's independent terminal result, joined after the guest
    transcript.  rx = frames the peer received from the guest; tx = echoes
    the peer sent back."""
    parts = [f"{case}:rx={counts[case][0]},tx={counts[case][1]}"
             for case in PEER_CASES]
    return ("MS08_NET_PEER_RESULT: outcome=ok rejects=0 " + " ".join(parts))


def _parse_peer_result(line):
    fields = _fields(line, "MS08_NET_PEER_RESULT: ")
    if fields.pop("outcome", None) != "ok":
        raise InvalidTranscript("host peer did not finish ok")
    if _number(fields, "rejects") != 0:
        raise InvalidTranscript("host peer rejected traffic")
    fields.pop("rejects")
    counts = {}
    for case in PEER_CASES:
        text = fields.pop(case + ":rx", None)
        if text is None:
            raise InvalidTranscript("host peer missing case " + case)
        rx_s, sep, tx_s = text.partition(",tx=")
        if not sep:
            raise InvalidTranscript("malformed peer accounting for " + case)
        rx = int(rx_s, 10)
        tx = int(tx_s, 10)
        if rx <= 0 or tx <= 0:
            raise InvalidTranscript("zero peer traffic for " + case)
        counts[case] = (rx, tx)
    if fields:
        raise InvalidTranscript("peer result grammar mismatch: " + repr(fields))
    return counts


def schema_lines():
    # Must match tests/ms08_network_smp_probe.c --print-schema byte for byte.
    return [
        "placement:MS08_NET_V5",
        "timer-disabled-wake:MS08_NET_V5,MS08_NET_V5,MS08_NET_WAKE",
        "tcp-bidirectional:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
        "udp-bidirectional:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
        "full-recovery:MS08_NET_V5,MS08_NET_DIAG,MS08_NET_DIAG,MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
        "readiness-quiet:MS08_NET_V5,MS08_NET_SOCK,MS08_NET_PEER",
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
                   if line.startswith("MS08_NET_ENV: "))
    other_env = list(valid)
    other_env[env_idx] = "MS08_NET_ENV: handwritten-operator-notes"
    validate(other_env)

    # Host peer result: missing, duplicate, wrong counts, nonzero rejects and
    # zero traffic all fail; the guest-authored MS08_NET_PEER marker alone
    # never constitutes peer evidence (5.3-R1).
    peer_idx = len(valid) - 1
    _rejects(lambda: validate(valid[:peer_idx]), "missing peer result")
    dup = list(valid)
    dup.append(valid[peer_idx])
    _rejects(lambda: validate(dup), "duplicate peer result")
    lines = list(valid)
    lines[peer_idx] = lines[peer_idx].replace("tcp-bidirectional:rx=8",
                                              "tcp-bidirectional:rx=7", 1)
    _rejects(lambda: validate(lines), "peer/guest count mismatch")
    lines = list(valid)
    lines[peer_idx] = lines[peer_idx].replace("outcome=ok", "outcome=rejects")
    _rejects(lambda: validate(lines), "peer rejects outcome")
    lines = list(valid)
    lines[peer_idx] = lines[peer_idx].replace("rejects=0", "rejects=1")
    _rejects(lambda: validate(lines), "peer nonzero rejects")
    lines = list(valid)
    lines[peer_idx] = lines[peer_idx].replace("readiness-quiet:rx=1,tx=1",
                                              "readiness-quiet:rx=0,tx=0")
    _rejects(lambda: validate(lines), "zero peer traffic")

    # Envelope: missing exit / end / environment.
    _rejects(lambda: validate(valid[:-2]), "missing harness exit")
    end_idx = valid.index("MS08_NET_END")
    _rejects(lambda: validate(valid[:end_idx] + valid[end_idx + 1:]),
             "missing end")
    _rejects(lambda: validate(valid[:1] + ["MS08_NET_ENV: "] + valid[1:]),
             "empty environment")

    # Structure: reordered case, unknown PASS, FAIL, foreign marker, fatal.
    _rejects(lambda: validate(valid[:2] + ["PASS: bogus"] + valid[2:]),
             "unknown PASS")
    swapped = list(valid)
    idx_a = swapped.index("PASS: placement")
    idx_b = swapped.index("PASS: timer-disabled-wake")
    swapped[idx_a], swapped[idx_b] = swapped[idx_b], swapped[idx_a]
    _rejects(lambda: validate(swapped), "reordered case")
    _rejects(lambda: validate(valid[:4] + ["FAIL: placement reason=io"] +
                              valid[4:]), "embedded FAIL")
    _rejects(lambda: validate(valid[:4] + ["MS08_NET_UNKNOWN: nope"] +
                              valid[4:]), "foreign marker")
    _rejects(lambda: validate(valid[:4] + ["kernel panic: test"] +
                              valid[4:]), "fatal line")

    # Placement violations: shared hart, off-pin observation, zero role progress,
    # or an out-of-schedulable / missing-pin mask bit.  A mask with extra
    # schedulable harts is legal only when the role's own migration record
    # explains them (Cycle 002, 6.2-R1).
    place_idx = next(i for i, line in enumerate(valid)
                     if line.startswith("MS08_NET_V5: case=placement"))
    for key, val in (("owner_aff", 3), ("owner_last", 5)):
        lines = list(valid)
        lines[place_idx] = _mutate(lines, place_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key)
    for key, val in (("owner_events", 0), ("runner_events", 0)):
        lines = list(valid)
        lines[place_idx] = _mutate(lines, place_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key)
    for key, val in (("owner_mask", 0x10004), ("runner_mask", 0x20000)):
        lines = list(valid)
        lines[place_idx] = _mutate(lines, place_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key +
                 " out of schedulable")
    for key, val in (("owner_mask", 0x1), ("runner_mask", 0x2)):
        lines = list(valid)
        lines[place_idx] = _mutate(lines, place_idx, key, val)
        _rejects(lambda l=lines: validate(l), "placement " + key +
                 " missing pin")
    # An in-set but unrecorded history bit is rejected.
    lines = list(valid)
    lines[place_idx] = _mutate(lines, place_idx, "owner_mask", 0x44)
    _rejects(lambda: validate(lines), "placement unrecorded in-set hart")
    # Legal history: owner migrated to hart 6 at boot (0x44 = {2,6}), recorded.
    lines = list(valid)
    lines[place_idx] = _mutate(lines, place_idx, "owner_mask", 0x44)
    lines[place_idx] = _mutate(lines, place_idx, "own_mig",
                               hex(_mig_view(2, 2, 6)))
    validate(lines)
    # A recorded hart outside the schedulable set still explains nothing.
    lines = list(valid)
    lines[place_idx] = _mutate(lines, place_idx, "own_mig",
                               hex(_mig_view(2, 2, 17)))
    lines[place_idx] = _mutate(lines, place_idx, "owner_mask", 0x20004)
    _rejects(lambda: validate(lines),
             "placement recorded hart outside schedulable")

    # Wake: timer-only progress (no IPI), local trigger, missing restore,
    # wrong run count.
    wake_idx = next(i for i, line in enumerate(valid)
                    if line.startswith("MS08_NET_V5: case=timer-disabled-wake")
                    and "wcompleted=1" in line)
    lines = list(valid)
    lines[wake_idx] = _mutate(lines, wake_idx, "wipi_after", 3)
    _rejects(lambda: validate(lines), "wake timer-only progress")
    lines = list(valid)
    lines[wake_idx] = _mutate(lines, wake_idx, "wlast_trigger", 0)
    _rejects(lambda: validate(lines), "wake local trigger")
    lines = list(valid)
    lines[wake_idx] = _mutate(lines, wake_idx, "wmissing_restore", 1)
    _rejects(lambda: validate(lines), "wake missing restore")
    lines = list(valid)
    lines[wake_idx] = _mutate(lines, wake_idx, "wcompleted", 2)
    _rejects(lambda: validate(lines), "wake run count")

    # Full-recovery: no pressure, leaked buffer, ticket/slot imbalance.
    full_idx = next(i for i, line in enumerate(valid)
                    if line.startswith("MS08_NET_V5: case=full-recovery")
                    and "tx_again=4" in line and "live=16" in line)
    lines = list(valid)
    lines[full_idx] = _mutate(lines, full_idx, "tx_again", 0)
    lines[full_idx] = _mutate(lines, full_idx, "tx_occ", 0)
    _rejects(lambda: validate(lines), "full-recovery no pressure evidence")
    rec_idx = next(i for i, line in enumerate(valid)
                   if line.startswith("MS08_NET_V5: case=full-recovery")
                   and "tx_reclaim=80" in line)
    lines = list(valid)
    lines[rec_idx] = _mutate(lines, rec_idx, "live", 3)
    _rejects(lambda: validate(lines), "full-recovery leaked ticket")
    lines = list(valid)
    lines[rec_idx] = _mutate(lines, rec_idx, "tx_reclaim", 79)
    _rejects(lambda: validate(lines), "full-recovery ticket imbalance")
    lines = list(valid)
    lines[rec_idx] = _mutate(lines, rec_idx, "tx_deq", 79)
    _rejects(lambda: validate(lines), "full-recovery slot imbalance")

    # Fault tuple drift and counter rewind.
    lines = list(valid)
    lines[rec_idx] = _mutate(lines, rec_idx, "fault_valid", 1)
    _rejects(lambda: validate(lines), "fault tuple drift")
    lines = list(valid)
    lines[rec_idx] = _mutate(lines, rec_idx, "tx_submit", 10)
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
