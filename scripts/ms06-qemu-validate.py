#!/usr/bin/env python3
"""MS06 application-witness transcript validator (pure output auditor).

This tool ONLY reads a saved transcript produced by a manual QEMU run of
`tests/ms06_stack_readiness_probe`. It never launches QEMU, never drives a
guest shell, and never touches the network: the human operator saves the
serial output and exit code, then this validator decides whether the run is
an acceptable MS06 witness.

Marker grammar (canonical order, each exactly once):

    MS06_STACK_READINESS_START
    MS06_REVISION: <non-empty>
    MS06_ENVIRONMENT: <non-empty>
    PASS: tcp-timer
    PASS: udp-progress
    ... (all 12 cases, fixed order) ...
    MS06_STACK_READINESS_END
    MS06_HARNESS_EXIT: 0

Any FAIL line, timeout metadata, missing/duplicated/out-of-order/unknown case,
partial success, missing/empty/duplicated revision or environment record, or
missing/nonzero/duplicated exit marker makes the transcript invalid. The error
message names the first decisive difference.
"""

from __future__ import annotations

import argparse
import sys

START = "MS06_STACK_READINESS_START"
END = "MS06_STACK_READINESS_END"
REVISION_PREFIX = "MS06_REVISION:"
ENVIRONMENT_PREFIX = "MS06_ENVIRONMENT:"
EXIT_PREFIX = "MS06_HARNESS_EXIT:"

# The exact 12 application-visible readiness cases of the MS06 probe, in the
# order the probe must print them. There is no aggregate PASS: every case has
# its own unique marker.
EXPECTED_CASES = (
    "tcp-timer",
    "udp-progress",
    "listener",
    "nonblock-connect-error",
    "quiet",
    "continuous-traffic",
    "close-error",
    "poll-multiwaiter",
    "select-multiwaiter",
    "epoll-multiwaiter",
    "waiter-64",
    "waiter-65-reregister",
)


class InvalidTranscript(Exception):
    """Raised with the first decisive difference found in a transcript."""


def validate_output(
    output: str,
    *,
    timed_out: bool = False,
    expect_revision: str | None = None,
    expect_environment: str | None = None,
) -> None:
    """Validate a full saved transcript; raise InvalidTranscript on any gap."""
    if timed_out:
        raise InvalidTranscript("the saved run hit its overall timeout")

    lines = output.splitlines()
    start_at: int | None = None
    end_at: int | None = None
    for index, raw_line in enumerate(lines):
        line = raw_line.strip()
        if line == START:
            if start_at is not None:
                raise InvalidTranscript("start marker is duplicated")
            start_at = index
        elif line == END:
            if end_at is not None:
                raise InvalidTranscript("end marker is duplicated")
            end_at = index
    if start_at is None:
        raise InvalidTranscript("start marker is missing")
    if end_at is None:
        raise InvalidTranscript("end marker is missing")
    if end_at < start_at:
        raise InvalidTranscript("end marker appears before the start marker")

    for raw_line in lines[:start_at]:
        line = raw_line.strip()
        if line.startswith(("PASS:", "FAIL:", "MS06_")):
            raise InvalidTranscript(
                f"protocol marker before the witness start marker: {line!r}"
            )

    body = lines[start_at + 1 : end_at]
    tail = lines[end_at + 1 :]

    revision: str | None = None
    environment: str | None = None
    passed: list[str] = []
    remaining = list(EXPECTED_CASES)
    phase = 0  # 0: awaiting REVISION; 1: awaiting ENVIRONMENT; 2+: awaiting PASS n

    for raw_line in body:
        line = raw_line.strip()
        if line.startswith("PASS:"):
            name = line[len("PASS:"):].strip()
            if phase < 2:
                raise InvalidTranscript(
                    f"PASS marker {name!r} appears before revision/environment metadata"
                )
            if not remaining:
                raise InvalidTranscript(f"unexpected PASS marker {name!r}: all 12 cases already reported")
            if name == remaining[0]:
                passed.append(name)
                remaining.pop(0)
            elif name in remaining:
                raise InvalidTranscript(
                    f"out-of-order PASS {name!r}; expected {remaining[0]!r} next"
                )
            else:
                raise InvalidTranscript(f"unknown or duplicated PASS marker {name!r}")
        elif line.startswith(REVISION_PREFIX):
            if revision is not None:
                raise InvalidTranscript("revision record is duplicated")
            if phase != 0:
                raise InvalidTranscript(
                    "revision record appears after the environment record or PASS markers"
                )
            value = line[len(REVISION_PREFIX):].strip()
            if not value:
                raise InvalidTranscript("revision record is empty")
            revision = value
            phase = 1
        elif line.startswith(ENVIRONMENT_PREFIX):
            if environment is not None:
                raise InvalidTranscript("environment record is duplicated")
            if phase != 1:
                raise InvalidTranscript(
                    "environment record appears before the revision record or after PASS markers"
                )
            value = line[len(ENVIRONMENT_PREFIX):].strip()
            if not value:
                raise InvalidTranscript("environment record is empty")
            environment = value
            phase = 2
        elif line.startswith("FAIL:"):
            raise InvalidTranscript(f"payload reported a failure: {line}")
        elif line.startswith("MS06_"):
            raise InvalidTranscript(f"unknown MS06 protocol line inside witness body: {line!r}")
        # Anything else is serial noise around the witness and is ignored.

    if len(passed) != len(EXPECTED_CASES):
        raise InvalidTranscript(
            f"witness ended early: {len(passed)}/12 cases passed; "
            f"{remaining[0]!r} and later are missing (partial success)"
        )
    if revision is None:
        raise InvalidTranscript("revision record is missing")
    if environment is None:
        raise InvalidTranscript("environment record is missing")
    if expect_revision is not None and revision != expect_revision:
        raise InvalidTranscript(
            f"revision mismatch: recorded {revision!r}, expected {expect_revision!r}"
        )
    if expect_environment is not None and environment != expect_environment:
        raise InvalidTranscript(
            f"environment mismatch: recorded {environment!r}, expected {expect_environment!r}"
        )

    exits: list[str] = []
    for raw_line in tail:
        line = raw_line.strip()
        if line.startswith(EXIT_PREFIX):
            exits.append(line[len(EXIT_PREFIX):].strip())
        elif line.startswith(("PASS:", "FAIL:", "MS06_")):
            raise InvalidTranscript(
                f"protocol marker after the end marker: {line!r}"
            )

    if not exits:
        raise InvalidTranscript("harness exit marker is missing")
    if len(exits) > 1:
        raise InvalidTranscript(f"harness exit marker is duplicated: {exits!r}")
    if exits != ["0"]:
        raise InvalidTranscript(f"nonzero harness exit: {exits[0]!r}")


def _transcript(
    *,
    revision: str = "r0",
    environment: str = "qemu-virt",
    cases: tuple[str, ...] = EXPECTED_CASES,
    noise_before: str = "",
    noise_middle: str = "",
    noise_after: str = "",
    exit_line: str = f"{EXIT_PREFIX}0",
    extra_lines: tuple[str, ...] = (),
) -> str:
    lines: list[str] = []
    if noise_before:
        lines.append(noise_before)
    lines.append(START)
    lines.append(f"{REVISION_PREFIX} {revision}")
    lines.append(f"{ENVIRONMENT_PREFIX} {environment}")
    if noise_middle:
        lines.append(noise_middle)
    lines.extend(f"PASS: {case}" for case in cases)
    lines.extend(extra_lines)
    lines.append(END)
    if exit_line:
        lines.append(exit_line)
    if noise_after:
        lines.append(noise_after)
    return "\n".join(lines) + "\n"


def self_test() -> None:
    """Full positive transcript plus at least one minimal negative per class."""
    def transcript_lines(lines: list[str]) -> str:
        return "\n".join(lines) + "\n"

    good = _transcript(noise_before="starry:~# ./ms06", noise_middle="", noise_after="starry:~#")
    validate_output(good)
    validate_output(
        good, expect_revision="r0", expect_environment="qemu-virt"
    )

    def rejected(sample: str, **kwargs: object) -> None:
        try:
            validate_output(sample, **kwargs)  # type: ignore[arg-type]
        except InvalidTranscript:
            return
        raise AssertionError("invalid synthetic output was accepted")

    # Structural markers.
    rejected(good.replace(START + "\n", ""))
    rejected(good.replace(START + "\n", START + "\n" + START + "\n"))
    rejected(good.replace(END + "\n", ""))
    rejected(good.replace(END + "\n", END + "\n" + END + "\n"))

    # START and END must be recognised as whole physical lines after a full
    # trim: a line that merely contains the marker as a substring must never
    # act as a witness boundary (Plan Review Task 4.1 gap).
    rejected(good.replace(START + "\n", "shell-noise-" + START + "\n", 1))
    rejected(good.replace(START + "\n", START + "-trailing-noise\n", 1))
    rejected(good.replace(END + "\n", "shell-noise-" + END + "\n", 1))
    rejected(good.replace(END + "\n", END + "-trailing-noise\n", 1))
    validate_output(good)
    validate_output(good.replace(START + "\n", "shell-noise\n" + START + "\n", 1))
    validate_output(good.replace(END + "\n", END + "\nshell-noise\n", 1))

    # Case coverage: missing, duplicated, out-of-order, unknown, partial.
    rejected(_transcript(cases=EXPECTED_CASES[1:]))
    rejected(good.replace("PASS: quiet\n", "PASS: quiet\nPASS: quiet\n"))
    rejected(_transcript(cases=(EXPECTED_CASES[1], EXPECTED_CASES[0]) + EXPECTED_CASES[2:]))
    rejected(_transcript(cases=EXPECTED_CASES[:-1] + ("mystery-case",)))
    rejected(_transcript(cases=EXPECTED_CASES[:11]))  # partial success

    # Failure and timeout reporting.
    rejected(_transcript(extra_lines=("FAIL: quiet deadline expired",)))
    rejected(good, timed_out=True)

    # Protocol markers before the witness START are always fatal; ordinary
    # shell/serial noise before START remains legal. (Task 4.1 pre-START scan.)
    def with_prestart(line: str) -> str:
        return _transcript().replace(START + "\n", line + "\n" + START + "\n", 1)

    rejected(with_prestart("FAIL: stale-before-start"))
    rejected(with_prestart("MS06_HARNESS_EXIT: 1"))
    rejected(with_prestart("PASS: tcp-timer"))
    validate_output(with_prestart("starry:~# ./ms06"))  # pure noise stays legal

    # Leading-whitespace protocol markers must be classified after a full
    # trim; otherwise a marker indented by the serial driver is accepted as
    # noise (Plan Review Task 4.1 gap). Every phase gets its own negatives.
    def indented(line: str) -> str:
        return "  " + line

    rejected(with_prestart(indented("FAIL: stale-before-start")))
    rejected(with_prestart(indented("MS06_HARNESS_EXIT: 1")))
    rejected(with_prestart(indented("PASS: tcp-timer")))
    validate_output(with_prestart(indented("starry:~# ./ms06")))

    indented_fail_before_meta = transcript_lines([
        START,
        indented("FAIL: drifted-into-body"),
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        END,
        f"{EXIT_PREFIX}0",
    ])
    rejected(indented_fail_before_meta)

    indented_ms06_before_meta = transcript_lines([
        START,
        indented("MS06_UNKNOWN: x"),
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        END,
        f"{EXIT_PREFIX}0",
    ])
    rejected(indented_ms06_before_meta)

    indented_pass_after_end = transcript_lines([
        START,
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        END,
        indented("PASS: tcp-timer"),
        f"{EXIT_PREFIX}0",
    ])
    rejected(indented_pass_after_end)

    indented_fail_after_exit = transcript_lines([
        START,
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        END,
        f"{EXIT_PREFIX}0",
        indented("FAIL: stale-after-exit"),
    ])
    rejected(indented_fail_after_exit)

    # Revision / environment metadata.
    no_revision = good.replace(f"{REVISION_PREFIX} r0\n", "")
    rejected(no_revision)
    rejected(good.replace(f"{REVISION_PREFIX} r0", f"{REVISION_PREFIX} "))
    rejected(
        good.replace(
            f"{ENVIRONMENT_PREFIX} qemu-virt\n",
            f"{ENVIRONMENT_PREFIX} qemu-virt\n{REVISION_PREFIX} r0\n",
        )
    )
    no_environment = good.replace(f"{ENVIRONMENT_PREFIX} qemu-virt\n", "")
    rejected(no_environment)
    swapped_metadata = good.replace(
        f"{REVISION_PREFIX} r0\n{ENVIRONMENT_PREFIX} qemu-virt\n",
        f"{ENVIRONMENT_PREFIX} qemu-virt\n{REVISION_PREFIX} r0\n",
    )
    rejected(swapped_metadata)
    rejected(
        good.replace(
            f"{EXIT_PREFIX}0\n",
            f"{EXIT_PREFIX}0\n{ENVIRONMENT_PREFIX} qemu-virt\n",
        )
    )
    rejected(good, expect_revision="r9")
    rejected(good, expect_environment="bare-metal")

    # Exit contract.
    rejected(good.replace(f"{EXIT_PREFIX}0", f"{EXIT_PREFIX}1"))
    rejected(good.replace(f"{EXIT_PREFIX}0\n", ""))
    rejected(good.replace(f"{EXIT_PREFIX}0\n", f"{EXIT_PREFIX}0\n{EXIT_PREFIX}0\n"))
    rejected(_transcript(exit_line=f"PASS: waiter-64\n{EXIT_PREFIX}0"))

    # Protocol phase order (Plan Review A1): START -> REVISION -> ENVIRONMENT
    # -> 12 PASS -> END -> EXIT. Any marker outside its phase is rejected.
    passes_before_meta = transcript_lines([
        START,
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        END,
        f"{EXIT_PREFIX}0",
    ])
    rejected(passes_before_meta)  # old parser accepted this as a valid transcript

    pass_between_meta = transcript_lines([
        START,
        f"{REVISION_PREFIX} r0",
        "PASS: tcp-timer",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES[1:]],
        END,
        f"{EXIT_PREFIX}0",
    ])
    rejected(pass_between_meta)

    metadata_after_pass = transcript_lines([
        START,
        "PASS: tcp-timer",
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES[1:]],
        END,
        f"{EXIT_PREFIX}0",
    ])
    rejected(metadata_after_pass)

    exit_before_end = transcript_lines([
        START,
        f"{REVISION_PREFIX} r0",
        f"{ENVIRONMENT_PREFIX} qemu-virt",
        *[f"PASS: {c}" for c in EXPECTED_CASES],
        f"{EXIT_PREFIX}0",
        END,
    ])
    rejected(exit_before_end)

    print("PASS: ms06-validator-self-test")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--self-test", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--print-cases", action="store_true", help=argparse.SUPPRESS)
    parser.add_argument("--timed-out", action="store_true",
                        help="the saved run hit its overall timeout")
    parser.add_argument("--expect-revision", default=None,
                        help="fail unless MS06_REVISION equals this value")
    parser.add_argument("--expect-environment", default=None,
                        help="fail unless MS06_ENVIRONMENT equals this value")
    parser.add_argument("transcript", nargs="?",
                        help="path to the saved serial transcript ('-' for stdin)")
    args = parser.parse_args()

    try:
        if args.self_test:
            self_test()
            return 0
        if args.print_cases:
            for case in EXPECTED_CASES:
                print(case)
            return 0
        if not args.transcript:
            parser.error("a transcript path (or --self-test / --print-cases) is required")
        if args.transcript == "-":
            output = sys.stdin.read()
        else:
            with open(args.transcript, encoding="utf-8", errors="replace") as handle:
                output = handle.read()
        validate_output(
            output,
            timed_out=args.timed_out,
            expect_revision=args.expect_revision,
            expect_environment=args.expect_environment,
        )
    except InvalidTranscript as error:
        print(f"FAIL: ms06-validator: {error}", file=sys.stderr)
        return 1
    except (OSError, AssertionError, NotImplementedError) as error:
        print(f"FAIL: ms06-validator: {error}", file=sys.stderr)
        return 1
    print("PASS: ms06-transcript-valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
