#!/usr/bin/env python3
"""Pure-output validator for the MS07 manual QEMU recovery transcript."""
import argparse
import sys

EXPECTED_CASES = (
    "pre_reset_traffic", "reset_request", "old_socket_terminal",
    "new_epoch_traffic", "hmp_link_down", "hmp_link_up",
)


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


def validate(lines, expect_revision=None, expect_environment=None):
    body = [line.strip() for line in lines if line.strip()]
    if len(body) < 5 or body[0] != "MS07_RECOVERY_START" or body[-1] != "MS07_HARNESS_EXIT: 0":
        raise InvalidTranscript("missing start or successful exit")
    revision = body[1].removeprefix("MS07_REVISION: ")
    environment = body[2].removeprefix("MS07_ENVIRONMENT: ")
    if not revision or body[1] == revision or not environment or body[2] == environment:
        raise InvalidTranscript("missing revision or environment")
    if expect_revision is not None and revision != expect_revision:
        raise InvalidTranscript("revision mismatch")
    if expect_environment is not None and environment != expect_environment:
        raise InvalidTranscript("environment mismatch")
    cursor = 3
    observations = {}
    for case in EXPECTED_CASES:
        if cursor >= len(body) - 1 or body[cursor] != "MS07_CASE_START: " + case:
            raise InvalidTranscript("missing or reordered case start: " + case)
        cursor += 1
        markers = []
        while cursor < len(body) - 1 and not body[cursor].startswith("PASS: "):
            line = body[cursor]
            if line.startswith("FAIL:") or line.startswith("MS07_"):
                markers.append(line)
                cursor += 1
                continue
            raise InvalidTranscript("unexpected transcript line: " + line)
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


def _v4(markers, case):
    fields = _fields(_single_marker(markers, "MS07_V4: "), "MS07_V4: ")
    if fields.pop("case", None) != case:
        raise InvalidTranscript("V4 case mismatch")
    required = {"current_valid", "q", "s", "l", "link", "owned", "fault_valid"}
    if set(fields) != required:
        raise InvalidTranscript("V4 grammar mismatch")
    if _number(fields, "current_valid") != 1 or _number(fields, "owned") != 0:
        raise InvalidTranscript("invalid current observation")
    _number(fields, "fault_valid")
    return (_number(fields, "q"), _number(fields, "s"), _number(fields, "l"), fields["link"])


def _validate_protocol(observations):
    pre = _v4(observations["pre_reset_traffic"], "pre_reset_traffic")
    if pre[3] != "up":
        raise InvalidTranscript("pre-reset link is not up")
    _expect_fields(_fields(_single_marker(observations["pre_reset_traffic"], "MS07_PEER: "), "MS07_PEER: "),
                   case="pre_reset_traffic", result="ok")
    _expect_fields(_fields(_single_marker(observations["reset_request"], "MS07_RESET: "), "MS07_RESET: "),
                   accepted="1", duplicate="EAGAIN")
    old = _v4(observations["old_socket_terminal"], "old_socket_terminal")
    if old != (pre[0] + 1, pre[1] + 1, pre[2], "up"):
        raise InvalidTranscript("reset epoch relation failed")
    _expect_fields(_fields(_single_marker(observations["old_socket_terminal"], "MS07_SOCKET: "), "MS07_SOCKET: "),
                   case="old_socket_terminal", terminal="ECONNRESET")
    fresh = _v4(observations["new_epoch_traffic"], "new_epoch_traffic")
    if fresh != old:
        raise InvalidTranscript("new-epoch traffic identity drift")
    _expect_fields(_fields(_single_marker(observations["new_epoch_traffic"], "MS07_PEER: "), "MS07_PEER: "),
                   case="new_epoch_traffic", result="ok")
    down = _v4(observations["hmp_link_down"], "hmp_link_down")
    if down != (old[0], old[1], old[2] + 1, "down"):
        raise InvalidTranscript("link-down epoch relation failed")
    _expect_fields(_fields(_single_marker(observations["hmp_link_down"], "MS07_HMP_READY: "), "MS07_HMP_READY: "), link="off")
    _expect_fields(_fields(_single_marker(observations["hmp_link_down"], "MS07_HMP_OBSERVED: "), "MS07_HMP_OBSERVED: "), link="off")
    _expect_fields(_fields(_single_marker(observations["hmp_link_down"], "MS07_SOCKET: "), "MS07_SOCKET: "),
                   case="hmp_link_down", terminal="ENOTCONN")
    up = _v4(observations["hmp_link_up"], "hmp_link_up")
    if up != (old[0], old[1] + 1, down[2] + 1, "up"):
        raise InvalidTranscript("link-up epoch relation failed")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_HMP_READY: "), "MS07_HMP_READY: "), link="on")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_HMP_OBSERVED: "), "MS07_HMP_OBSERVED: "), link="on")
    _expect_fields(_fields(_single_marker(observations["hmp_link_up"], "MS07_PEER: "), "MS07_PEER: "),
                   case="hmp_link_up", result="ok")


def self_test():
    valid = ["MS07_RECOVERY_START", "MS07_REVISION: test", "MS07_ENVIRONMENT: test"]
    valid += [
        "MS07_CASE_START: pre_reset_traffic", "MS07_V4: case=pre_reset_traffic current_valid=1 q=7 s=11 l=13 link=up owned=0 fault_valid=0", "MS07_PEER: case=pre_reset_traffic result=ok", "PASS: pre_reset_traffic",
        "MS07_CASE_START: reset_request", "MS07_RESET: accepted=1 duplicate=EAGAIN", "PASS: reset_request",
        "MS07_CASE_START: old_socket_terminal", "MS07_V4: case=old_socket_terminal current_valid=1 q=8 s=12 l=13 link=up owned=0 fault_valid=0", "MS07_SOCKET: case=old_socket_terminal terminal=ECONNRESET", "PASS: old_socket_terminal",
        "MS07_CASE_START: new_epoch_traffic", "MS07_V4: case=new_epoch_traffic current_valid=1 q=8 s=12 l=13 link=up owned=0 fault_valid=0", "MS07_PEER: case=new_epoch_traffic result=ok", "PASS: new_epoch_traffic",
        "MS07_CASE_START: hmp_link_down", "MS07_HMP_READY: link=off", "MS07_HMP_OBSERVED: link=off", "MS07_V4: case=hmp_link_down current_valid=1 q=8 s=12 l=14 link=down owned=0 fault_valid=0", "MS07_SOCKET: case=hmp_link_down terminal=ENOTCONN", "PASS: hmp_link_down",
        "MS07_CASE_START: hmp_link_up", "MS07_HMP_READY: link=on", "MS07_HMP_OBSERVED: link=on", "MS07_V4: case=hmp_link_up current_valid=1 q=8 s=13 l=15 link=up owned=0 fault_valid=0", "MS07_PEER: case=hmp_link_up result=ok", "PASS: hmp_link_up",
        "MS07_RECOVERY_END", "MS07_HARNESS_EXIT: 0",
    ]
    validate(valid)
    try:
        validate(valid, expect_revision="other")
    except InvalidTranscript:
        pass
    else:
        raise AssertionError("wrong expected revision accepted")
    missing_identity = ["MS07_RECOVERY_START"]
    missing_identity += ["PASS: " + case for case in EXPECTED_CASES]
    missing_identity += ["MS07_HARNESS_EXIT: 0"]
    try:
        validate(missing_identity)
    except InvalidTranscript:
        pass
    else:
        raise AssertionError("transcript without identity metadata accepted")
    for bad in (valid[:-1], valid[:4] + ["PASS: bogus"] + valid[5:], valid[:3] + valid[4:8] + valid[3:4] + valid[8:]):
        try:
            validate(bad)
        except InvalidTranscript:
            continue
        raise AssertionError("negative fixture accepted")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--print-cases", action="store_true")
    parser.add_argument("--expect-revision")
    parser.add_argument("--expect-environment")
    parser.add_argument("transcript", nargs="?")
    args = parser.parse_args()
    if args.print_cases:
        print("\n".join(EXPECTED_CASES))
        return 0
    if args.self_test:
        self_test()
        return 0
    if not args.transcript:
        parser.error("transcript is required unless --self-test or --print-cases")
    with open(args.transcript, encoding="utf-8") as source:
        validate(source, args.expect_revision, args.expect_environment)
    return 0


if __name__ == "__main__":
    sys.exit(main())
