#!/usr/bin/env python3
"""Pure-output validator for the MS07 manual QEMU recovery transcript.

It is a strict state machine: it never imports networking/process control,
never launches QEMU, and never opens the guest/HMP.  It audits the frozen
case order, environment identity, V4 current and historical-fault
tuples, epoch/ledger relations, permanent socket terminals, HMP
ready/observed markers, the recovery end and the harness exit.  Any FAIL,
panic, trap, ownership drift, unknown/duplicate/missing marker, illegal
validity or missing exit fails the transcript with a first difference.
"""
import argparse
import sys

# P1: the fixed single-hart VirtIO-MMIO model keeps `QS` resident RX owners and
# `QS` free TX buffers at idle, so a healthy/current owner tuple is
# `available==device_owned==QS` with no quarantine.  `device_owned==0` means the
# RX owners are absent and is never a healthy observation.
OWNER_BASELINE = 64

EXPECTED_CASES = (
    "pre_reset_traffic", "reset_request", "old_socket_terminal",
    "new_epoch_traffic", "hmp_link_down", "hmp_link_up",
)

REQUIRED_V4_FIELDS = {
    "lifecycle", "current_valid", "q", "s", "l", "link",
    "available", "device_owned", "quarantined",
    "fault_valid", "fault_stage", "fault_cause", "fault_q",
    "fault_available", "fault_device_owned", "fault_quarantined",
}

FATAL_LINES = (
    "panic", "trap", "oops", "fatal ownership drift", "abort",
)

ORDERED_MARKERS = {
    "pre_reset_traffic": ("MS07_V4: ", "MS07_PEER: "),
    "reset_request": ("MS07_RESET: ",),
    "old_socket_terminal": ("MS07_V4: ", "MS07_SOCKET: "),
    "new_epoch_traffic": ("MS07_V4: ", "MS07_SOCKET: ", "MS07_PEER: "),
    "hmp_link_down": (
        "MS07_HMP_READY: ", "MS07_HMP_OBSERVED: ", "MS07_V4: ", "MS07_SOCKET: ",
    ),
    "hmp_link_up": (
        "MS07_HMP_READY: ", "MS07_HMP_OBSERVED: ", "MS07_V4: ", "MS07_SOCKET: ",
        "MS07_PEER: ",
    ),
}


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
        value = int(fields[key], 10)
    except (KeyError, ValueError):
        raise InvalidTranscript("missing or invalid numeric " + key) from None
    if value < 0 or value > (1 << 64) - 1:
        raise InvalidTranscript("numeric overflow " + key)
    return value


def _expect_fields(fields, **expected):
    if fields != expected:
        raise InvalidTranscript("unexpected marker fields: " + repr(fields))


def validate(lines, expect_environment=None):
    raw = [line.strip() for line in lines if line.strip()]
    for line in raw:
        lowered = line.lower()
        if any(fatal in lowered for fatal in FATAL_LINES):
            raise InvalidTranscript("fatal line present: " + line)
    body = [
        line for line in raw
        if line.startswith("MS07_") or line.startswith("PASS:") or line.startswith("FAIL:")
    ]
    if len(body) < 5 or body[0] != "MS07_RECOVERY_START" or body[-1] != "MS07_HARNESS_EXIT: 0":
        raise InvalidTranscript("missing start or successful exit")
    environment = body[1].removeprefix("MS07_ENVIRONMENT: ")
    if not environment or body[1] == environment:
        raise InvalidTranscript("missing environment")
    if expect_environment is not None and environment != expect_environment:
        raise InvalidTranscript("environment mismatch")
    cursor = 2
    observations = {}
    for case in EXPECTED_CASES:
        if cursor >= len(body) - 1 or body[cursor] != "MS07_CASE_START: " + case:
            raise InvalidTranscript("missing or reordered case start: " + case)
        cursor += 1
        markers = []
        while cursor < len(body) - 1 and not body[cursor].startswith("PASS: "):
            line = body[cursor]
            if line.startswith("FAIL:"):
                raise InvalidTranscript("guest reported failure: " + line)
            if line.startswith("MS07_"):
                markers.append(line)
                cursor += 1
                continue
            # Serial consoles may add banners or ANSI-cleaned diagnostic noise.
            # Protocol markers, however, are fail-closed below.
            cursor += 1
        if cursor >= len(body) - 1 or body[cursor] != "PASS: " + case:
            raise InvalidTranscript("missing PASS: " + case)
        cursor += 1
        observations[case] = markers
    if cursor != len(body) - 2 or body[cursor] != "MS07_RECOVERY_END":
        raise InvalidTranscript("missing end or trailing protocol line")
    _validate_protocol(observations)


def _single_marker(markers, prefix):
    matches = [line for line in markers if line.startswith(prefix)]
    if len(matches) != 1:
        raise InvalidTranscript("expected exactly one " + prefix.rstrip())
    return matches[0]


def _reject_unconsumed(markers, *prefixes):
    for line in markers:
        if not line.startswith(prefixes):
            raise InvalidTranscript("unknown or misplaced protocol marker: " + line)


def _expect_marker_order(markers, case):
    expected = ORDERED_MARKERS[case]
    if len(markers) != len(expected):
        raise InvalidTranscript("wrong marker count for " + case)
    for line, prefix in zip(markers, expected):
        if not line.startswith(prefix):
            raise InvalidTranscript("reordered marker for " + case + ": expected " + prefix.rstrip())


def _v4(markers, case):
    fields = _fields(_single_marker(markers, "MS07_V4: "), "MS07_V4: ")
    if fields.pop("case", None) != case:
        raise InvalidTranscript("V4 case mismatch")
    if set(fields) != REQUIRED_V4_FIELDS:
        raise InvalidTranscript("V4 grammar mismatch: " + repr(fields))
    link = fields.pop("link")
    if link not in ("up", "down"):
        raise InvalidTranscript("V4 link must be up or down")
    obs = {key: _number(fields, key) for key in fields}
    obs["link"] = link
    if obs["current_valid"] != 1:
        raise InvalidTranscript("V4 marker requires a current tuple")
    if obs["fault_valid"] not in (0, 1):
        raise InvalidTranscript("invalid fault validity")
    if obs["fault_valid"] == 0 and any(obs[f] for f in (
            "fault_stage", "fault_cause", "fault_q", "fault_available",
            "fault_device_owned", "fault_quarantined")):
        raise InvalidTranscript("absent fault tuple must be all zero")
    if obs["lifecycle"] != 2:
        raise InvalidTranscript("current observation must be Active")
    _healthy_owner(obs)
    return obs


def _healthy_owner(obs):
    if (obs["available"] != OWNER_BASELINE or obs["device_owned"] != OWNER_BASELINE
            or obs["quarantined"] != 0):
        raise InvalidTranscript("successful V4 owner must be at the healthy VirtIO baseline")


def _socket(markers, case, expected_terminal):
    fields = _fields(_single_marker(markers, "MS07_SOCKET: "), "MS07_SOCKET: ")
    _expect_fields(fields, case=case, terminal=expected_terminal)


def _fault_tuple(obs):
    """The six V4 markers of one recovery session must all carry the SAME
    historical fault tuple: the coherent fault is frozen once at the reset and
    never changes or drifts across later link epochs.  A single-field mutate of
    one marker (even into the legal 0/1 validity domain) is therefore a drift."""
    return tuple(obs[k] for k in (
        "fault_valid", "fault_stage", "fault_cause", "fault_q",
        "fault_available", "fault_device_owned", "fault_quarantined"))


def _reset_gap(before, after):
    return (after["lifecycle"], after["current_valid"], after["q"], after["s"],
            after["l"], after["link"]) == (
        2, 1, before["q"] + 1, before["s"] + 1, before["l"], "up")


def _validate_protocol(observations):
    for case in EXPECTED_CASES:
        _expect_marker_order(observations[case], case)
    _reject_unconsumed(observations["pre_reset_traffic"], "MS07_V4: ", "MS07_PEER: ")
    pre = _v4(observations["pre_reset_traffic"], "pre_reset_traffic")
    if pre["link"] != "up":
        raise InvalidTranscript("pre-reset link is not up")
    _expect_fields(_fields(_single_marker(observations["pre_reset_traffic"], "MS07_PEER: "), "MS07_PEER: "),
                   case="pre_reset_traffic", result="ok")
    _reject_unconsumed(observations["reset_request"], "MS07_RESET: ")
    _expect_fields(_fields(_single_marker(observations["reset_request"], "MS07_RESET: "), "MS07_RESET: "),
                   accepted="1", duplicate="EAGAIN")
    _reject_unconsumed(observations["old_socket_terminal"], "MS07_V4: ", "MS07_SOCKET: ")
    old = _v4(observations["old_socket_terminal"], "old_socket_terminal")
    # The reset transition advances exactly QueueEpoch and SocketEpoch by one;
    # link generation/state stay unchanged and the owner is Active.
    if not _reset_gap(pre, old):
        raise InvalidTranscript("reset epoch relation failed")
    _socket(observations["old_socket_terminal"], "old_socket_terminal", "ECONNRESET")
    _reject_unconsumed(observations["new_epoch_traffic"],
                       "MS07_V4: ", "MS07_SOCKET: ", "MS07_PEER: ")
    fresh = _v4(observations["new_epoch_traffic"], "new_epoch_traffic")
    if fresh["q"] != old["q"] or fresh["s"] != old["s"] or fresh["link"] != "up":
        raise InvalidTranscript("new-epoch traffic identity drift")
    if (fresh["available"] != pre["available"]
            or fresh["device_owned"] != pre["device_owned"]):
        raise InvalidTranscript("new-epoch owner ledger not conserved")
    _socket(observations["new_epoch_traffic"], "new_epoch_traffic", "ECONNRESET")
    _expect_fields(_fields(_single_marker(observations["new_epoch_traffic"], "MS07_PEER: "), "MS07_PEER: "),
                   case="new_epoch_traffic", result="ok")
    _reject_unconsumed(observations["hmp_link_down"],
                       "MS07_HMP_READY: ", "MS07_HMP_OBSERVED: ", "MS07_V4: ", "MS07_SOCKET: ")
    down = _v4(observations["hmp_link_down"], "hmp_link_down")
    if down["q"] != old["q"] or down["s"] != old["s"] or down["l"] != old["l"] + 1:
        raise InvalidTranscript("link-down epoch relation failed")
    if down["link"] != "down":
        raise InvalidTranscript("link-down must observe link down")
    _expect_fields(_fields(_single_marker(observations["hmp_link_down"], "MS07_HMP_READY: "), "MS07_HMP_READY: "), link="off")
    _expect_fields(_fields(_single_marker(observations["hmp_link_down"], "MS07_HMP_OBSERVED: "), "MS07_HMP_OBSERVED: "), link="off")
    _socket(observations["hmp_link_down"], "hmp_link_down", "ENOTCONN")
    _reject_unconsumed(observations["hmp_link_up"],
                       "MS07_HMP_READY: ", "MS07_HMP_OBSERVED: ", "MS07_V4: ", "MS07_SOCKET: ", "MS07_PEER: ")
    up = _v4(observations["hmp_link_up"], "hmp_link_up")
    if up["q"] != old["q"] or up["s"] != old["s"] + 1 or up["l"] != down["l"] + 1:
        raise InvalidTranscript("link-up epoch relation failed")
    if up["link"] != "up":
        raise InvalidTranscript("link-up must observe link up")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_HMP_READY: "), "MS07_HMP_READY: "), link="on")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_HMP_OBSERVED: "), "MS07_HMP_OBSERVED: "), link="on")
    _socket(observations["hmp_link_up"], "hmp_link_up", "ENOTCONN")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_PEER: "), "MS07_PEER: "),
                   case="hmp_link_up", result="ok")
    # A4: the historical fault tuple is frozen once at reset; every V4 marker of
    # the session (pre, old, new, down, up) must display the SAME coherent fault,
    # and a single-field drift on any one marker is rejected.
    for label, obs in (("old_socket_terminal", old), ("new_epoch_traffic", fresh),
                       ("hmp_link_down", down), ("hmp_link_up", up)):
        if _fault_tuple(obs) != _fault_tuple(pre):
            raise InvalidTranscript("historical fault tuple drifted at " + label)
    # A4: a link flap does not touch the owner ledger at all: both `available`
    # and `device_owned` are conserved across the new-epoch/down/up observations.
    if (down["available"] != pre["available"] or up["available"] != pre["available"]
            or down["device_owned"] != pre["device_owned"]
            or up["device_owned"] != pre["device_owned"]):
        raise InvalidTranscript("link-phase owner ledger not conserved")


def canonical():
    v4 = ("MS07_V4: case={case} lifecycle=2 current_valid=1 q={q} s={s} l={l} link={link} "
          "available={available} device_owned=64 quarantined=0 fault_valid=0 fault_stage=0 "
          "fault_cause=0 fault_q=0 fault_available=0 fault_device_owned=0 fault_quarantined=0")
    lines = ["MS07_RECOVERY_START", "MS07_ENVIRONMENT: test"]
    lines += ["MS07_CASE_START: pre_reset_traffic",
              v4.format(case="pre_reset_traffic", q=7, s=11, l=13, link="up", available=64),
              "MS07_PEER: case=pre_reset_traffic result=ok", "PASS: pre_reset_traffic"]
    lines += ["MS07_CASE_START: reset_request", "MS07_RESET: accepted=1 duplicate=EAGAIN",
              "PASS: reset_request"]
    lines += ["MS07_CASE_START: old_socket_terminal",
              v4.format(case="old_socket_terminal", q=8, s=12, l=13, link="up", available=64),
              "MS07_SOCKET: case=old_socket_terminal terminal=ECONNRESET",
              "PASS: old_socket_terminal"]
    lines += ["MS07_CASE_START: new_epoch_traffic",
              v4.format(case="new_epoch_traffic", q=8, s=12, l=13, link="up", available=64),
              "MS07_SOCKET: case=new_epoch_traffic terminal=ECONNRESET",
              "MS07_PEER: case=new_epoch_traffic result=ok", "PASS: new_epoch_traffic"]
    lines += ["MS07_CASE_START: hmp_link_down", "MS07_HMP_READY: link=off",
              "MS07_HMP_OBSERVED: link=off",
              v4.format(case="hmp_link_down", q=8, s=12, l=14, link="down", available=64),
              "MS07_SOCKET: case=hmp_link_down terminal=ENOTCONN", "PASS: hmp_link_down"]
    lines += ["MS07_CASE_START: hmp_link_up", "MS07_HMP_READY: link=on",
              "MS07_HMP_OBSERVED: link=on",
              v4.format(case="hmp_link_up", q=8, s=13, l=15, link="up", available=64),
              "MS07_SOCKET: case=hmp_link_up terminal=ENOTCONN",
              "MS07_PEER: case=hmp_link_up result=ok", "PASS: hmp_link_up"]
    lines += ["MS07_RECOVERY_END", "MS07_HARNESS_EXIT: 0"]
    return lines


def schema_lines():
    return [
        case + ":" + ",".join(prefix.removesuffix(": ") for prefix in ORDERED_MARKERS[case])
        for case in EXPECTED_CASES
    ]


def _rejects(func, label):
    try:
        func()
    except InvalidTranscript:
        return
    raise AssertionError("negative fixture accepted: " + label)


def _mutate_key(lines, pos, key, newval):
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
    _rejects(lambda: validate(valid, expect_environment="wrong-env"), "wrong expected environment")
    missing_identity = ["MS07_RECOVERY_START"]
    missing_identity += ["PASS: " + case for case in EXPECTED_CASES]
    missing_identity += ["MS07_HARNESS_EXIT: 0"]
    _rejects(lambda: validate(missing_identity), "transcript without environment metadata")

    # Structural: missing exit / unknown PASS / reordered case / embedded
    # FAIL / foreign marker / fatal line.
    _rejects(lambda: validate(valid[:-1]), "missing harness exit")
    _rejects(lambda: validate(valid[:4] + ["PASS: bogus"] + valid[5:]), "unknown PASS")
    _reordered = [valid[0], valid[1]] + valid[6:9] + valid[2:6] + valid[9:]
    _rejects(lambda: validate(_reordered), "reordered case")
    _rejects(lambda: validate(valid[:4] + ["FAIL: pre_reset_traffic reason=io"] + valid[4:]), "embedded FAIL")
    _rejects(lambda: validate(valid[:4] + ["MS07_UNKNOWN: nope"] + valid[4:]), "foreign marker")
    _rejects(lambda: validate(valid[:4] + ["kernel panic: test"] + valid[4:]), "embedded fatal line")
    _rejects(lambda: validate(valid[:4] + ["KERNEL PANIC: test"] + valid[4:]), "uppercase fatal line")

    lines = list(valid)
    lines[3], lines[4] = lines[4], lines[3]
    _rejects(lambda: validate(lines), "pre marker order")
    lines = list(valid)
    lines[19], lines[20] = lines[20], lines[19]
    _rejects(lambda: validate(lines), "HMP observed before ready")
    lines = list(valid)
    lines[3] = _mutate_key(lines, 3, "current_valid", 0)
    _rejects(lambda: validate(lines), "pre current tuple absent")
    lines = list(valid)
    lines[10] = _mutate_key(lines, 10, "device_owned", 1)
    _rejects(lambda: validate(lines), "old socket owner not healthy")

    # Per-field V4 value mutation: any single-field drift must be rejected
    # either by the explicit validity/lifecycle/link grammar or by the frozen
    # reset/link/drain/conservation relations.
    for key, val in (("lifecycle", 5), ("current_valid", 5), ("q", 1 << 40),
                     ("s", 1 << 40), ("l", 1 << 40), ("available", 1 << 40),
                     ("device_owned", 1), ("quarantined", 2), ("fault_valid", 2),
                     ("fault_stage", 1 << 40), ("fault_cause", 1 << 40),
                     ("fault_q", 1 << 40), ("fault_available", 1 << 40),
                     ("fault_device_owned", 1 << 40), ("fault_quarantined", 1 << 40)):
        lines = list(valid)
        lines[3] = _mutate_key(lines, 3, key, val)
        _rejects(lambda l=lines: validate(l), "pre V4 " + key + " mutation")
    lines = list(valid)
    lines[3] = _mutate_key(lines, 3, "link", "half")
    _rejects(lambda: validate(lines), "pre V4 link domain")
    # link-down must advance LinkGeneration by exactly one and link state to down.
    lines = list(valid)
    lines[21] = _mutate_key(lines, 21, "l", 13)
    _rejects(lambda: validate(lines), "link-down no generation advance")
    lines = list(valid)
    lines[21] = _mutate_key(lines, 21, "link", "up")
    _rejects(lambda: validate(lines), "link-down link stuck up")
    # A4: the historical fault tuple must stay identical across all markers, and
    # the link-phase owner `available` ledger must be conserved (a single-field
    # drift within the legal validity domain is still a drift).
    lines = list(valid)
    lines[10] = _mutate_key(lines, 10, "fault_valid", 1)
    _rejects(lambda: validate(lines), "old-socket fault tuple drifts from session")
    lines = list(valid)
    lines[21] = _mutate_key(lines, 21, "available", 63)
    _rejects(lambda: validate(lines), "link-down available not conserved")
    lines = list(valid)
    lines[27] = _mutate_key(lines, 27, "available", 63)
    _rejects(lambda: validate(lines), "link-up available not conserved")
    # new-epoch must conserve available and drain DeviceOwned.
    lines = list(valid)
    lines[14] = _mutate_key(lines, 14, "available", 63)
    _rejects(lambda: validate(lines), "new-epoch available not conserved")
    lines = list(valid)
    lines[14] = _mutate_key(lines, 14, "device_owned", 1)
    _rejects(lambda: validate(lines), "new-epoch not drained")
    # P1: the healthy VirtIO owner baseline requires `available==device_owned==QS`.
    # A 63/64 or 64/63 imbalance at any successful marker is rejected by the
    # healthy-owner predicate, independent of ledger conservation.
    lines = list(valid)
    lines[3] = _mutate_key(lines, 3, "available", 63)
    _rejects(lambda: validate(lines), "pre V4 owner imbalanced 63/64")
    lines = list(valid)
    lines[3] = _mutate_key(lines, 3, "device_owned", 63)
    _rejects(lambda: validate(lines), "pre V4 owner imbalanced 64/63")
    # hmp_link_up socket terminal and duplicated/missing markers.
    lines = list(valid)
    lines[28] = _mutate_key(lines, 28, "case", "bogus")
    _rejects(lambda: validate(lines), "link-up socket terminal case drift")
    _rejects(lambda: validate(valid[:22] + valid[22:23] + valid[22:]), "duplicate link-down socket marker")
    _rejects(lambda: validate(valid[:22] + valid[23:]), "missing link-down socket marker")
    # HMP ready and observed must both be present and agree.
    lines = list(valid)
    lines[26] = lines[26].replace("MS07_HMP_OBSERVED: link=on", "MS07_HMP_OBSERVED: link=off")
    _rejects(lambda: validate(lines), "HMP on/off mismatch")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--print-cases", action="store_true")
    parser.add_argument("--print-schema", action="store_true")
    parser.add_argument("--expect-environment")
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
        validate(source, args.expect_environment)
    return 0


if __name__ == "__main__":
    sys.exit(main())
