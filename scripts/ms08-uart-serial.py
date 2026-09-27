#!/usr/bin/env python3
"""Bounded UART serial-socket harness for the MS08 QEMU SMP qualification.

It owns the QEMU serial Unix socket (``-chardev socket,... -serial chardev:``),
captures the guest console byte stream, waits for ``MS08_UART_READY``, injects
numbered ASCII payload frames when the guest prints ``MS08_UART_NEED`` markers,
and compares the guest's ``MS08UARTECHO`` frames byte-for-byte against what was
injected.  A console ``PASS`` marker alone never proves receipt: only exact
outbound bytes do.

It never launches QEMU and never runs a validator; the pure-output validator
(scripts/ms08-uart-validate.py) consumes the joined transcript plus this
harness's explicit result lines.  Every wait is bounded by one absolute
deadline; socket EOF, timeout, corrupt echo data and guest-reported failures
are distinct outcomes.
"""
import argparse
import select
import socket
import sys
import time

NEED_PREFIX = "MS08_UART_NEED: "
ECHO_PREFIX = "MS08UARTECHO "
# A serial TTY may echo injected frames back into the captured stream; those
# host-to-guest lines are transport noise, never guest evidence.
INJECT_PREFIX = "MS08RX "
READY_LINE = "MS08_UART_READY"
END_LINE = "MS08_UART_END"
# --launch-probe: the guest shell prompt to wait for, and the exact command
# sent exactly once when it appears (Iteration 005, task 6.1).  TTY input echo
# stays disabled for the whole probe run so injected frames never leak back
# into the captured stream; stty restores echo after the probe exits, keeping
# one serial client and the unchanged numbered payload grammar.
SHELL_PROMPT = b"starry:~#"
LAUNCH_CMD = (b"stty -echo; chmod +x /root/ms08_uart_smp_probe && "
              b"/root/ms08_uart_smp_probe --run; stty echo\n")
# Bounded parser: keep at most this many undelimited bytes so a bulk raw TX
# burst cannot exhaust memory; the raw bytes are streamed to the transcript
# separately, so trimming the marker parser's buffer loses nothing.
MAX_PARTIAL = 4096


def payload_pattern(seq, length):
    """Deterministic printable ASCII payload for one numbered frame."""
    alphabet = "abcdefghijklmnopqrstuvwxyz0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ"
    return "".join(alphabet[(seq - 1 + i) % len(alphabet)] for i in range(length))


def encode_frame(seq, payload):
    """Host-to-guest frame: one line, printable payload, no spaces."""
    if not payload or any(not (0x20 < ord(c) <= 0x7E) or c == " " for c in payload):
        raise ValueError("payload must be non-empty printable non-space ASCII")
    return f"MS08RX seq={seq} len={len(payload)} {payload}\n".encode("ascii")


def decode_need_line(line):
    """Parse ``MS08_UART_NEED: case=<case> count=N len=L``; None if unrelated."""
    if not line.startswith(NEED_PREFIX):
        return None
    fields = {}
    for item in line[len(NEED_PREFIX):].split():
        key, sep, value = item.partition("=")
        if not sep or not key or key in fields:
            return None
        fields[key] = value
    if set(fields) != {"case", "count", "len"} or not fields["case"]:
        return None
    try:
        count = int(fields["count"], 10)
        length = int(fields["len"], 10)
    except ValueError:
        return None
    if count <= 0 or count > 64 or length <= 0 or length > 48:
        return None
    return fields["case"], count, length


def decode_echo_line(line):
    """Parse ``MS08UARTECHO seq=N len=L <bytes>``; None if unrelated/malformed."""
    if not line.startswith(ECHO_PREFIX):
        return None
    rest = line[len(ECHO_PREFIX):]
    try:
        seq_s, len_s, payload = rest.split(" ", 2)
        seq = int(seq_s.removeprefix("seq="), 10)
        length = int(len_s.removeprefix("len="), 10)
    except (ValueError, AttributeError):
        return None
    if seq <= 0 or length <= 0 or length > 48:
        return None
    if len(payload) != length:
        return None
    if any(not (0x20 < ord(c) <= 0x7E) or c == " " for c in payload):
        return None
    return seq, payload


class EchoLedger:
    """Expected echo frames for the current NEED batch, matched exactly once
    in strict sequence order (seq 1..count, no gaps or reordering)."""

    def __init__(self):
        self.expected = {}
        self.next_seq = 1

    def arm(self, count, length):
        self.expected = {
            seq: payload_pattern(seq, length) for seq in range(1, count + 1)
        }
        self.next_seq = 1

    def match(self, seq, payload):
        if seq != self.next_seq:
            return False
        if self.expected.get(seq) != payload:
            return False
        del self.expected[seq]
        self.next_seq += 1
        return True

    def complete(self):
        return not self.expected


class Session:
    """Guest-console state machine.  ``feed`` returns bytes to inject back.

    Outcomes: ``ok`` (END + every armed echo matched), ``guest-fail`` (a guest
    FAIL marker), ``corrupt`` (a malformed or mismatched echo), ``guest-eof``
    (stream ended before MS08_UART_END).
    """

    def __init__(self):
        self.state = "wait-ready"
        self.outcome = None
        self.ledger = EchoLedger()
        self.pending_arm = None
        self.injected = 0
        self.matched = 0
        self.seen_end = False

    def feed_line(self, line):
        """Returns a list of byte strings to send back to the guest."""
        if self.outcome is not None:
            return []
        if self.state == "wait-ready":
            if line == READY_LINE:
                self.state = "running"
            return []
        if line.startswith("FAIL:"):
            self.outcome = "guest-fail"
            return []
        need = decode_need_line(line)
        if need is not None:
            _case, count, length = need
            self.ledger.arm(count, length)
            self.pending_arm = (count, length)
            self.injected += count
            return [encode_frame(seq, payload_pattern(seq, length))
                    for seq in range(1, count + 1)]
        echo = decode_echo_line(line)
        if echo is not None:
            seq, payload = echo
            if not self.ledger.match(seq, payload):
                self.outcome = "corrupt"
            else:
                self.matched += 1
            return []
        if line == END_LINE:
            self.seen_end = True
            if not self.ledger.complete():
                self.outcome = "corrupt"
            else:
                self.outcome = "ok"
            return []
        return []

    def feed_eof(self):
        if self.outcome is None:
            self.outcome = "guest-eof"


class LineReader:
    """Reassemble '\n'-terminated lines from an arbitrarily chunked byte
    stream; tolerates trailing '\r' (serial consoles).  The no-newline buffer
    is bounded to `max_partial` bytes so an unframed bulk TX burst cannot grow
    without bound; any trim only discards raw bytes that are also streamed to
    the transcript, never a marker."""

    def __init__(self, max_partial=None):
        self.buf = bytearray()
        self.max_partial = max_partial if max_partial is not None else MAX_PARTIAL

    def push(self, chunk):
        lines = []
        self.buf.extend(chunk)
        while True:
            idx = self.buf.find(b"\n")
            if idx < 0:
                break
            raw = bytes(self.buf[:idx])
            del self.buf[:idx + 1]
            if raw.endswith(b"\r"):
                raw = raw[:-1]
            lines.append(raw.decode("ascii", errors="replace"))
        if len(self.buf) > self.max_partial:
            # Discard only the undelimited head; later bytes are more likely
            # to begin a real marker.  Raw bytes are still in the transcript.
            del self.buf[: len(self.buf) - self.max_partial]
        return lines


class Launcher:
    """Wait for the guest shell prompt once, then send the probe launch
    command.  Detect the prompt as a byte substring across arbitrary TCP
    chunks; never re-send on a later prompt appearance."""

    def __init__(self, prompt=SHELL_PROMPT, cmd=LAUNCH_CMD):
        self.prompt = prompt
        self.cmd = cmd
        self.sent = False
        self.acc = bytearray()
        self.seen_prompt = False

    def push(self, chunk):
        """Return True exactly once, when the prompt first appears (the caller
        then sends `cmd` once).  Never arm again on a later appearance."""
        if self.sent:
            return False
        self.acc.extend(chunk)
        if len(self.acc) > MAX_PARTIAL:
            self.acc = self.acc[-MAX_PARTIAL:]
        if self.prompt in self.acc:
            self.sent = True
            return True
        return False

    def launch_bytes(self):
        """The bytes to send once the prompt is observed."""
        return self.cmd


def run_session(reader, session, now, deadline, select_fn, recv_fn, send_fn,
                out=None, launcher=None):
    """Generic bounded pump.  `now`/`select_fn`/`recv_fn`/`send_fn` are
    injectable so the self-test can drive fake clocks and sockets; the deadline
    is re-checked after select returns and after every receive.  Every received
    byte is echoed to `out` (the raw transcript) if provided.  `launcher`
    (a Launcher) sends the probe launch command exactly once after the shell
    prompt; the resulting transcript bytes are still streamed to `out`."""
    while session.outcome is None:
        if not (now() < deadline):
            break
        timeout = max(0.0, deadline - now())
        readable = select_fn(timeout)
        if not readable or not (now() < deadline):
            break
        chunk = recv_fn(65536)
        if chunk == b"":
            session.feed_eof()
            break
        if launcher is not None and launcher.push(chunk):
            send_fn(launcher.launch_bytes())
        if not (now() < deadline):
            break
        if out is not None:
            out.write(chunk)
        for line in reader.push(chunk):
            for outbound in session.feed_line(line):
                send_fn(outbound)
    if session.outcome is None:
        return "timeout"
    return session.outcome


def self_test():
    def drive(script):
        """Feed a scripted byte stream through the session in one pass."""
        reader = LineReader()
        session = Session()
        sent = []
        for chunk in script:
            for line in reader.push(chunk):
                sent.extend(session.feed_line(line))
        session.feed_eof()
        return session, sent

    # 1) Full happy path: READY, one NEED batch of 2 frames, echoes, END.
    payload1 = payload_pattern(1, 8)
    payload2 = payload_pattern(2, 8)
    guest = [
        b"boot noise\nMS08_UART_START\n",
        b"MS08_UART_ENV: qemu-virt-riscv64-smp16-ns16550-serial-socket\nMS08_UART_READY\n",
        b"MS08_UART_CASE_START: rx\nMS08_UART_NEED: case=rx count=2 len=8\n",
        # A TTY may loop the injected frames back into the capture; the
        # harness must treat them as noise, not as guest echoes.
        f"MS08RX seq=1 len=8 {payload1}\n".encode(),
        f"MS08UARTECHO seq=1 len=8 {payload1}\n".encode(),
        f"MS08UARTECHO seq=2 len=8 {payload2}\n".encode(),
        b"MS08_UART_END\n",
    ]
    session, sent = drive(guest)
    assert session.outcome == "ok", session.outcome
    assert sent == [encode_frame(1, payload1), encode_frame(2, payload2)]
    assert session.injected == 2 and session.matched == 2

    # 2) Re-arm: a second NEED batch restarts the sequence at 1.
    guest2 = guest[:3] + [
        f"MS08UARTECHO seq=1 len=8 {payload1}\n".encode(),
        f"MS08UARTECHO seq=2 len=8 {payload2}\n".encode(),
        b"MS08_UART_NEED: case=readiness count=1 len=4\n",
        f"MS08UARTECHO seq=1 len=4 {payload_pattern(1, 4)}\n".encode(),
        b"MS08_UART_END\n",
    ]
    session2, sent2 = drive(guest2)
    assert session2.outcome == "ok", session2.outcome
    assert len(sent2) == 3
    assert encode_frame(1, payload_pattern(1, 4)) in sent2

    # 3) Corrupt echo: mismatched bytes fail closed.
    bad = list(guest)
    bad[4] = b"MS08UARTECHO seq=1 len=8 XXXXXXXX\n"
    session3, _ = drive(bad)
    assert session3.outcome == "corrupt"

    # 4) Out-of-order / unexpected seq fails closed.
    bad2 = list(guest)
    bad2[4], bad2[5] = bad2[5], bad2[4]
    session4, _ = drive(bad2)
    assert session4.outcome == "corrupt"

    # 5) Guest FAIL marker and EOF-before-END are distinct outcomes.
    fail_guest = [b"MS08_UART_READY\nFAIL: rx reason=rx-corrupt-frame\n"]
    session5, _ = drive(fail_guest)
    assert session5.outcome == "guest-fail"
    eof_guest = [b"MS08_UART_READY\nMS08_UART_CASE_START: rx\n"]
    session6, _ = drive(eof_guest)
    assert session6.outcome == "guest-eof"

    # 6) END with unmatched echoes is corrupt, not ok.
    early_end = [b"MS08_UART_READY\nMS08_UART_NEED: case=rx count=2 len=8\n",
                 b"MS08_UART_END\n"]
    session7, _ = drive(early_end)
    assert session7.outcome == "corrupt"

    # 7) Deadline discipline under a fake clock: a select that reports the
    # socket readable while advancing the clock past the deadline must not
    # deliver any bytes.
    class FakeClock:
        def __init__(self, now):
            self._now = now

        def __call__(self):
            return self._now

    class LateReadableSelect:
        def __init__(self, clock, jump):
            self.clock = clock
            self.jump = jump

        def __call__(self, _timeout):
            self.clock._now += self.jump
            return True

    class FakeSock:
        def __init__(self):
            self.chunks = [b"MS08_UART_READY\n"]
            self.sent = []

        def recv(self, _n):
            return self.chunks.pop(0) if self.chunks else b""

        def send(self, data):
            self.sent.append(data)

    clock = FakeClock(100.0)
    sock = FakeSock()
    session8 = Session()
    outcome = run_session(LineReader(), session8, clock, 101.0,
                          LateReadableSelect(clock, 5.0), sock.recv, sock.send)
    assert outcome == "timeout", outcome
    assert not sock.sent

    # 8) decode helpers fail closed on garbage.
    assert decode_need_line("MS08_UART_NEED: case=rx count=0 len=8") is None
    assert decode_need_line("MS08_UART_NEED: case=rx count=2 len=8 extra=1") is None
    assert decode_need_line("MS08_UART_SNAP: case=rx") is None
    assert decode_echo_line("MS08UARTECHO seq=1 len=4 ab") is None
    assert decode_echo_line("MS08UARTECHO seq=1 len=4 abcd extra") is None
    assert decode_echo_line("MS08_UART_RX: case=rx") is None
    try:
        encode_frame(1, "has space")
        raise AssertionError("space payload accepted")
    except ValueError:
        pass

    # 9) Launch probe exactly once on the shell prompt, and bound the parser
    #    on a long undelimited bulk-TX burst.  (Iteration 005, task 6.1)
    launch = Launcher()
    assert launch.push(b"boot noise\nstarry:~#") is True
    assert launch.launch_bytes() == LAUNCH_CMD
    assert launch.push(b"starry:~#") is False   # never re-arms

    #   One launch command must disable TTY input echo before the probe and
    #   restore it after exit so injected frames never echo into the capture.
    assert b"stty -echo" in LAUNCH_CMD
    assert b"--run" in LAUNCH_CMD
    assert b"; stty echo" in LAUNCH_CMD

    # Prompt split across an arbitrary chunk boundary still arms once.
    split = Launcher()
    assert split.push(b"boot noise\nstarry:") is False
    assert split.push(b"~#") is True
    assert split.push(b" rest") is False

    # Missing prompt within the deadline must not arm.
    quiet = Launcher()
    assert quiet.push(b"some boot text\n") is False

    # Bounded parser: a huge undelimited burst is trimmed (memory bounded);
    # after the guest emits a fresh line, its marker is recovered unchanged.
    reader = LineReader()
    assert reader.push(b"T" * (MAX_PARTIAL * 8)) == []
    assert len(reader.buf) <= MAX_PARTIAL
    assert len(reader.push(b"\n")) == 1        # the burst line (bulk, ignored)
    lines = reader.push(b"MS08_UART_END\n")
    assert "MS08_UART_END" in lines

    # Existing default LineReader is still bounded.
    assert len(LineReader().buf) == 0

    # 10) A marker that is NOT on a fresh line after raw bulk bytes is
    #     indistinguishable from the payload: the guest must begin markers on a
    #     fresh line (Iteration 005, task 6.1) or the validator drops them.
    rr = LineReader()
    merged = rr.push(b"T" * 256 + b"MS08_UART_CASE_START: ready\n")
    assert len(merged) == 1 and merged[0].startswith("T")
    # The marker is buried in the bulk line, not a standalone protocol line.
    assert not (merged[0].startswith("MS08_UART") or
                merged[0].startswith("PASS:") or
                merged[0].startswith("FAIL:"))

    # With a fresh line emitted by the fixed guest, the marker is recovered.
    rr2 = LineReader()
    rr2.push(b"T" * 256 + b"\n")          # bulk burst then guest fresh line
    fresh = rr2.push(b"MS08_UART_CASE_START: ready\n")
    assert "MS08_UART_CASE_START: ready" in fresh


def serve(sock_path, deadline_seconds, launch_probe=False, out=None):
    """Connect to the QEMU serial socket (QEMU is the chardev server with
    ``server=on,wait=off``; this harness is the client) under one absolute
    deadline, then pump the session.  All waits are select-based; no sleep.
    With `launch_probe`, wait for the guest shell prompt and send the probe
    launch command exactly once.  Every received byte is written to `out`
    (default None: discard) as the raw transcript."""
    deadline = time.monotonic() + deadline_seconds
    session = Session()
    reader = LineReader()
    launcher = Launcher() if launch_probe else None
    # QEMU may still be binding its chardev listener when the harness starts:
    # retry the connect under the same absolute deadline.  Each attempt uses a
    # fresh socket; between attempts we wait on an empty select with a small
    # timeout slice, so a missing listener never spins the CPU.
    while True:
        conn = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        conn.setblocking(False)
        err = conn.connect_ex(sock_path)
        if err == 0:
            break
        conn.close()
        remaining = deadline - time.monotonic()
        if remaining <= 0:
            return "connect-timeout", session
        select.select([], [], [], min(0.05, remaining))
    try:
        def select_fn(timeout):
            readable, _, _ = select.select([conn], [], [], timeout)
            return bool(readable)

        def recv_fn(n):
            try:
                return conn.recv(n)
            except BlockingIOError:
                return b""

        outcome = run_session(reader, session, time.monotonic, deadline,
                              select_fn, recv_fn, conn.sendall, out=out,
                              launcher=launcher)
    finally:
        conn.close()
    return outcome, session


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--socket", help="QEMU serial Unix socket path")
    parser.add_argument("--deadline-seconds", type=int, default=300)
    parser.add_argument("--launch-probe", action="store_true",
                        help="wait for shell prompt, then run the UART probe once")
    parser.add_argument("--transcript",
                        help="path to write raw serial bytes (transcript)")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if not args.socket:
        parser.error("--socket is required unless --self-test")
    if args.deadline_seconds <= 0:
        parser.error("--deadline-seconds must be positive")
    out = open(args.transcript, "wb") if args.transcript else None
    try:
        outcome, session = serve(args.socket, args.deadline_seconds,
                                 launch_probe=args.launch_probe, out=out)
    finally:
        pass
    result_line = (f"MS08_UART_HARNESS_RESULT: outcome={outcome} "
                   f"injected={session.injected} matched={session.matched}\n")
    exit_line = f"MS08_UART_HARNESS_EXIT: {0 if outcome == 'ok' else 1}\n"
    # Append the harness decision to the same transcript the validator reads,
    # so a single file carries both the raw guest bytes and the result lines.
    if out is not None:
        out.write(result_line.encode("ascii"))
        out.write(exit_line.encode("ascii"))
        out.close()
    sys.stdout.write(result_line)
    sys.stdout.write(exit_line)
    return 0 if outcome == "ok" else 1


if __name__ == "__main__":
    sys.exit(main())
