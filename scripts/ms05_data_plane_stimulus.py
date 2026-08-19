#!/usr/bin/env python3
"""Bounded host stimulus for the MS05 data-plane probe.

Validates the guest registration/start/traffic sequence, peer, count, payload
and mode with bounded socket timeouts. Rejects malformed control, wrong
peer/mode/count, duplicate/missing/out-of-order sequences and late replies.
"""

from __future__ import annotations

import argparse
import socket
import struct
import threading
import time

MAGIC = 0x4D533035
DEFAULT_HOST = "0.0.0.0"
DEFAULT_PORT = 15557
MODES = ("snapshot", "tx-only", "bidirectional", "slot-full",
         "descriptor-full", "flush")
HELD_MODES = ("slot-full", "descriptor-full")
MIN_COUNT = 1
MAX_COUNT = 4096
MIN_PAYLOAD = 1
MAX_PAYLOAD = 64
SELF_TEST_TIMEOUT = 2.0
GRACE_TIMEOUT = 2.0
EXCHANGE_TIMEOUT = 10.0
# Repair 6.2-R5: the operator-paced registration listen window is independent
# of the short data-exchange deadline. Waiting for the operator to start the
# guest in a manual QEMU session must not consume the exchange budget.
MANUAL_LISTEN_TIMEOUT = 120.0


def parse_control(data: bytes, verb: str) -> tuple[str, int, int]:
    try:
        text = data.decode("ascii")
        marker, actual_verb, mode, count_text, payload_text = text.split()
        count = int(count_text, 10)
        payload = int(payload_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed control datagram") from error
    if marker != "MS05" or actual_verb != verb:
        raise ValueError(f"expected MS05 {verb}")
    if mode not in MODES:
        raise ValueError("unknown mode")
    if not MIN_COUNT <= count <= MAX_COUNT:
        raise ValueError("count outside bounded range")
    if not MIN_PAYLOAD <= payload <= MAX_PAYLOAD:
        raise ValueError("payload outside bounded range")
    return mode, count, payload


def parse_done(data: bytes, expected_mode: str) -> int:
    try:
        text = data.decode("ascii")
        marker, verb, mode, count_text = text.split()
        received = int(count_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed done datagram") from error
    if marker != "MS05" or verb != "DONE" or mode != expected_mode:
        raise ValueError("done does not match mode")
    if received < 0:
        raise ValueError("negative received count")
    return received


def parse_ack(data: bytes, expected_mode: str) -> tuple[str, int]:
    """Parse an ACK control agreeing on the DONE count for `expected_mode`.

    The ACK is valid only when it names the same mode and carries the shared
    count the host just sent in DONE (repair 6.2-R5).
    """
    try:
        text = data.decode("ascii")
        marker, verb, mode, count_text = text.split()
        count = int(count_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed ack datagram") from error
    if marker != "MS05" or verb != "ACK" or mode != expected_mode:
        raise ValueError("ack does not match mode")
    if count < 0:
        raise ValueError("negative ack count")
    return mode, count


def parse_sent(data: bytes, expected_mode: str, count: int) -> int:
    """Parse an SENT control with a mode-aware count rule.

    Normal modes require the exact nonzero registered count; held modes allow
    a nonzero short send within the registered count. A zero or partial
    normal-mode SENT is a deterministic protocol failure, never a PASS.
    """
    try:
        text = data.decode("ascii")
        marker, verb, mode, sent_text = text.split()
        sent = int(sent_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed sent datagram") from error
    if marker != "MS05" or verb != "SENT" or mode != expected_mode:
        raise ValueError("sent does not match mode")
    if mode in HELD_MODES:
        if not 1 <= sent <= count:
            raise ValueError("held-mode sent count outside bounded range")
    else:
        if sent != count:
            raise ValueError("normal-mode sent must equal registered count")
    return sent


def make_packet(sequence: int, count: int, payload_size: int) -> bytes:
    payload = bytes((sequence + index) & 0xFF for index in range(payload_size))
    return struct.pack("!III", MAGIC, sequence, count) + payload


def validate_packet(packet: bytes, sequence: int, count: int,
                    payload_size: int) -> None:
    expected = 12 + payload_size
    if len(packet) != expected:
        raise ValueError("data datagram length mismatch")
    magic, actual_sequence, actual_count = struct.unpack("!III", packet[:12])
    if (magic, actual_sequence, actual_count) != (MAGIC, sequence, count):
        raise ValueError("data datagram header mismatch")
    payload = packet[12:]
    if payload != bytes((sequence + index) & 0xFF
                        for index in range(payload_size)):
        raise ValueError("data datagram payload mismatch")


def serve_once(sock: socket.socket, clock=time.monotonic,
               listen_timeout: float = MANUAL_LISTEN_TIMEOUT,
               exchange_timeout: float = EXCHANGE_TIMEOUT
               ) -> tuple[str, int, int, int, int]:
    """Serve one guest exchange under two bounded deadlines.

    Phase 1 (manual listen): wait for the first valid REGISTER under the
    operator-paced `listen_timeout`. This is the manual QEMU registration and
    must not consume the exchange budget. Phase 2 (exchange): starting from the
    moment a valid REGISTER arrives, run the READY/START/data/SENT/DONE and
    ACK exchange under a fresh short `exchange_timeout`. A receive timeout
    anywhere is a deterministic failure, never an infinite wait; every
    post-registration datagram — including SENT and ACK — is rejected when its
    source differs from the registered peer, before its contents are parsed.
    `clock`, `listen_timeout` and `exchange_timeout` allow deterministic tests.
    """
    try:
        old_timeout = sock.gettimeout()
    except AttributeError:
        old_timeout = None
    try:
        registration, peer = listen_for_register(sock, clock() + listen_timeout,
                                                 clock)
        deadline = clock() + exchange_timeout
        sock.settimeout(GRACE_TIMEOUT)
        return _serve_exchange(sock, peer, registration, deadline, clock)
    except socket.timeout as error:
        raise ValueError("protocol phase timeout") from error
    finally:
        try:
            sock.settimeout(old_timeout)
        except AttributeError:
            pass


def listen_for_register(sock: socket.socket, listen_deadline: float,
                        clock) -> tuple[bytes, tuple[str, int]]:
    """Wait for a valid REGISTER under the operator-paced listen deadline.

    A valid REGISTER is accepted only when it arrives strictly before the
    listen deadline (equal/late registration fails). This phase is independent
    of the exchange deadline: the operator may take a long, finite time to
    start the guest in a manual QEMU session. Intermediate receive timeouts do
    not end registration, and an invalid pre-registration datagram is skipped
    rather than starting the exchange: the loop keeps listening under the same
    absolute deadline until a well-formed REGISTER parses or the budget is
    exhausted. The remaining listen budget is re-checked before and after each
    receive, mirroring the exchange `bounded_recv`.
    """
    while True:
        remaining = listen_deadline - clock()
        if remaining <= 0:
            raise ValueError("listen deadline exceeded before receive")
        # An intermediate timeout or an invalid datagram must not consume the
        # full listen window; clamp the socket timeout to the remaining budget
        # so a blocking receive can never outlive the absolute deadline.
        sock.settimeout(min(GRACE_TIMEOUT, remaining))
        remaining = listen_deadline - clock()
        if remaining <= 0:
            raise ValueError("listen deadline exceeded before receive")
        try:
            datagram, peer = sock.recvfrom(256)
        except socket.timeout:
            continue
        remaining = listen_deadline - clock()
        if remaining <= 0:
            raise ValueError("listen deadline exceeded after receive")
        try:
            parse_control(datagram, "REGISTER")
        except ValueError:
            continue
        return datagram, peer


def send_bounded(sock: socket.socket, packet: bytes,
                 destination: tuple[str, int], exchange_deadline: float,
                 clock) -> None:
    """Send one datagram strictly within the single exchange deadline.

    The deadline is checked before and after the send; a delayed send that
    crosses the deadline fails the exchange instead of renewing it. The
    clock is re-read after the timeout setter and before the send, so a
    setter that consumes the final budget still prevents the send from
    starting. An equal or late start never sends, and the socket timeout is
    clamped to the current remaining budget so a blocking send cannot
    outlive it.
    """
    remaining = exchange_deadline - clock()
    if remaining <= 0:
        raise ValueError("exchange deadline exceeded before send")
    sock.settimeout(min(GRACE_TIMEOUT, remaining))
    remaining = exchange_deadline - clock()
    if remaining <= 0:
        raise ValueError("exchange deadline exceeded before send")
    sock.sendto(packet, destination)
    remaining = exchange_deadline - clock()
    if remaining <= 0:
        raise ValueError("exchange deadline exceeded after send")


def _serve_exchange(sock: socket.socket, peer: tuple[str, int],
                    registration: bytes, exchange_deadline: float,
                    clock) -> tuple[str, int, int, int, int]:
    def bounded_recv(size: int) -> tuple[bytes, tuple[str, int]]:
        remaining = exchange_deadline - clock()
        if remaining <= 0:
            raise ValueError("exchange deadline exceeded before receive")
        sock.settimeout(min(GRACE_TIMEOUT, remaining))
        remaining = exchange_deadline - clock()
        if remaining <= 0:
            raise ValueError("exchange deadline exceeded before receive")
        datagram, source = sock.recvfrom(size)
        remaining = exchange_deadline - clock()
        if remaining <= 0:
            raise ValueError("exchange deadline exceeded after receive")
        return datagram, source

    mode, count, payload = parse_control(registration, "REGISTER")
    send_bounded(sock, f"MS05 READY {mode} {count} {payload}".encode("ascii"),
                 peer, exchange_deadline, clock)

    start, start_peer = bounded_recv(256)
    if start_peer != peer:
        raise ValueError("START from unexpected peer")
    start_mode, start_count, start_payload = parse_control(start, "START")
    if (start_mode, start_count, start_payload) != (mode, count, payload):
        raise ValueError("START does not match REGISTER")

    # RX direction (host -> guest): the host sends `count` datagrams first.
    if mode == "bidirectional":
        for sequence in range(count):
            send_bounded(sock, make_packet(sequence, count, payload), peer,
                         exchange_deadline, clock)

    # TX direction (guest -> host): validate each datagram strictly in order,
    # then await the SENT control that reports the guest's accepted count.
    received = 0
    sent = -1
    while True:
        datagram, source = bounded_recv(12 + MAX_PAYLOAD)
        if source != peer:
            raise ValueError("datagram from unexpected peer")
        if datagram.startswith(b"MS05 SENT"):
            sent = parse_sent(datagram, mode, count)
            break
        validate_packet(datagram, received, count, payload)
        received += 1
        if received > count:
            raise ValueError("more datagrams than registered count")

    # The guest drains the stack before SENT, but smoltcp-buffered residue
    # frames may still be in flight when SENT arrives. Keep receiving with a
    # bounded grace period until every reported datagram has been validated.
    while received < sent:
        datagram, source = bounded_recv(12 + MAX_PAYLOAD)
        if source != peer:
            raise ValueError("unexpected peer during grace period")
        if datagram.startswith(b"MS05 SENT"):
            raise ValueError("duplicate SENT during grace period")
        validate_packet(datagram, received, count, payload)
        received += 1

    if received != sent:
        raise ValueError("SENT count does not match validated datagrams")

    send_bounded(sock, f"MS05 DONE {mode} {received}".encode("ascii"), peer,
                 exchange_deadline, clock)

    # DONE/ACK (repair 6.2-R5): the host reports PASS only after a valid ACK
    # from the registered peer agreeing on the exact count it just sent in
    # DONE. Missing, late, wrong-peer, wrong-mode or wrong-count ACK fails.
    ack, ack_peer = bounded_recv(256)
    if ack_peer != peer:
        raise ValueError("ACK from unexpected peer")
    ack_mode, ack_count = parse_ack(ack, mode)
    if ack_count != received:
        raise ValueError("ACK count does not match DONE count")
    return mode, count, payload, received, sent


class FakeClock:
    """Deterministic monotonic clock for exchange-deadline tests."""

    def __init__(self, start: float = 0.0) -> None:
        self.now = start

    def advance(self, dt: float) -> None:
        self.now += dt

    def __call__(self) -> float:
        return self.now


class DripFeedSocket:
    """Models a real socket: every recvfrom blocks up to the currently-set
    timeout measured from when the recv starts. A drip feed that delivers one
    valid datagram within every window keeps succeeding indefinitely — only
    the exchange deadline (clamping the timeout to the remaining budget) can
    stop it. `send_delay` models a blocking send that consumes wall time."""

    def __init__(self, count: int, delta: float, clock: FakeClock,
                 send_delay: float = 0.0) -> None:
        peer = ("guest", 4242)
        self.clock = clock
        self.delta = delta
        self.send_delay = send_delay
        self.timeout: float | None = None
        self.incoming = [
            (b"MS05 REGISTER tx-only 96 64", peer),
            (b"MS05 START tx-only 96 64", peer),
        ] + [(make_packet(sequence, 96, 64), peer) for sequence in range(count)
             ] + [(b"MS05 SENT tx-only 96", peer), (b"MS05 ACK tx-only 96", peer)]
        self.outgoing: list[tuple[bytes, tuple[str, int]]] = []

    def settimeout(self, value: float | None) -> None:
        self.timeout = value

    def gettimeout(self) -> float | None:
        return self.timeout

    def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
        start = self.clock.now
        self.clock.advance(self.delta)
        elapsed = self.clock.now - start
        if self.timeout is not None and elapsed > self.timeout:
            raise socket.timeout("drip feed exceeded recv timeout")
        if not self.incoming:
            raise socket.timeout("no more datagrams")
        return self.incoming.pop(0)

    def sendto(self, packet: bytes, destination: tuple[str, int]) -> None:
        if self.send_delay > 0.0:
            self.clock.advance(self.send_delay)
        self.outgoing.append((packet, destination))


def self_test() -> None:
    assert parse_control(b"MS05 REGISTER tx-only 96 64", "REGISTER") == \
        ("tx-only", 96, 64)
    for malformed in (
        b"bad",
        b"MS05 REGISTER nope 96 64",
        b"MS05 REGISTER tx-only 0 64",
        b"MS05 REGISTER tx-only 4097 64",
        b"MS05 REGISTER tx-only 96 0",
        b"MS05 REGISTER tx-only 96 65",
        b"MS05 START tx-only 96 64",
    ):
        try:
            parse_control(malformed, "REGISTER")
        except ValueError:
            pass
        else:
            raise AssertionError(f"malformed control accepted: {malformed!r}")

    assert parse_done(b"MS05 DONE tx-only 96", "tx-only") == 96
    for malformed in (
        b"MS05 DONE other 96",
        b"MS05 DONE tx-only -1",
        b"bad",
    ):
        try:
            parse_done(malformed, "tx-only")
        except ValueError:
            pass
        else:
            raise AssertionError(f"malformed done accepted: {malformed!r}")

    assert parse_sent(b"MS05 SENT tx-only 96", "tx-only", 96) == 96
    for malformed in (b"MS05 SENT other 96", b"MS05 SENT tx-only -1",
                      b"MS05 SENT tx-only 4097", b"bad"):
        try:
            parse_sent(malformed, "tx-only", 96)
        except ValueError:
            pass
        else:
            raise AssertionError(f"malformed sent accepted: {malformed!r}")

    # Mode-aware SENT rules: a zero or partial normal-mode SENT is a
    # deterministic protocol failure; a held-mode SENT allows a nonzero
    # short send only within the registered count.
    for zero_partial in (b"MS05 SENT tx-only 0", b"MS05 SENT tx-only 95"):
        try:
            parse_sent(zero_partial, "tx-only", 96)
        except ValueError:
            pass
        else:
            raise AssertionError(
                f"vacuous/partial normal-mode sent accepted: {zero_partial!r}")
    assert parse_sent(b"MS05 SENT slot-full 40", "slot-full", 96) == 40
    assert parse_sent(b"MS05 SENT slot-full 96", "slot-full", 96) == 96
    for bad_held in (b"MS05 SENT slot-full 0", b"MS05 SENT slot-full 97"):
        try:
            parse_sent(bad_held, "slot-full", 96)
        except ValueError:
            pass
        else:
            raise AssertionError(f"invalid held-mode sent accepted: {bad_held!r}")

    packet = make_packet(3, 96, 64)
    validate_packet(packet, 3, 96, 64)
    for bad in (
        make_packet(4, 96, 64),
        make_packet(3, 95, 64),
        make_packet(3, 96, 64)[:-1],
        bytes([0xDE]) + make_packet(3, 96, 64)[1:],
        make_packet(3, 96, 64)[:12] + bytes(64),
    ):
        try:
            validate_packet(bad, 3, 96, 64)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid data datagram accepted")

    peer = ("guest", 4242)

    class ProtocolSocket:
        def __init__(self, count: int = 96, mode: str = "tx-only",
                     send_delay: float = 0.0,
                     clock: FakeClock | None = None) -> None:
            self.incoming = [
                (f"MS05 REGISTER {mode} {count} 64".encode("ascii"), peer),
                (f"MS05 START {mode} {count} 64".encode("ascii"), peer),
            ] + [
                (make_packet(sequence, count, 64), peer)
                for sequence in range(count)
            ] + [
                (f"MS05 SENT {mode} {count}".encode("ascii"), peer),
                (f"MS05 ACK {mode} {count}".encode("ascii"), peer),
            ]
            self.outgoing: list[tuple[bytes, tuple[str, int]]] = []
            self.send_delay = send_delay
            self.clock = clock if clock is not None else FakeClock()

        def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
            if not self.incoming:
                raise socket.timeout("no more datagrams")
            return self.incoming.pop(0)

        def settimeout(self, _value: float | None) -> None:
            pass

        def gettimeout(self) -> None:
            return None

        def sendto(self, packet: bytes, destination: tuple[str, int]) -> None:
            if self.send_delay > 0.0:
                self.clock.advance(self.send_delay)
            self.outgoing.append((packet, destination))

    protocol_socket = ProtocolSocket()
    result = serve_once(protocol_socket)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)
    assert protocol_socket.outgoing[0] == (b"MS05 READY tx-only 96 64", peer)
    assert protocol_socket.outgoing[-1][0] == b"MS05 DONE tx-only 96"

    # SENT reporting a count different from the validated datagrams fails.
    mismatched = ProtocolSocket()
    mismatched.incoming[-1] = (b"MS05 SENT tx-only 95", peer)
    try:
        serve_once(mismatched)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("mismatched SENT count accepted")

    # Out-of-order data datagrams must be rejected.
    class ReorderedSocket(ProtocolSocket):
        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            if len(self.incoming) >= 3 and self.incoming[0][0].startswith(
                    b"MS05 REGISTER"):
                pass
            return super().recvfrom(size)

    reordered = ReorderedSocket()
    reordered.incoming = reordered.incoming[:2] + [
        reordered.incoming[3], reordered.incoming[2],
    ] + reordered.incoming[4:]
    try:
        serve_once(reordered)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("out-of-order datagrams accepted")

    # Duplicate datagrams must be rejected.
    duplicated = ProtocolSocket()
    duplicated.incoming = duplicated.incoming[:3] + [duplicated.incoming[3]] + \
        duplicated.incoming[3:]
    try:
        serve_once(duplicated)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("duplicate datagrams accepted")

    # A missing datagram (gap) must fail before the deadline.
    missing = ProtocolSocket()
    missing.incoming = missing.incoming[:3] + missing.incoming[4:]
    try:
        serve_once(missing)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("missing datagram accepted")

    # SENT from an unexpected peer must be rejected before it is parsed.
    wrong_peer_sent = ProtocolSocket()
    wrong_peer_sent.incoming[-1] = (b"MS05 SENT tx-only 96", ("attacker", 9999))
    try:
        serve_once(wrong_peer_sent)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("SENT from unexpected peer accepted")

    # START from an unexpected peer is rejected before its contents matter.
    wrong_peer_start = ProtocolSocket()
    wrong_peer_start.incoming[1] = (b"MS05 START tx-only 96 64",
                                    ("attacker", 9999))
    try:
        serve_once(wrong_peer_start)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("START from unexpected peer accepted")

    # A persistent receive timeout never yields a REGISTER; the bounded
    # registration loop must keep listening only until the absolute listen
    # deadline, and fail there instead of hanging or accepting anything. The
    # fake models real time advancing across each timeout so the deadline
    # binds (with real `time.monotonic`, each recvfrom timeout consumes wall
    # time until the deadline is exhausted).
    class TimeoutAtFirstRecv(ProtocolSocket):
        def __init__(self, clock: FakeClock) -> None:
            super().__init__(clock=clock)

        def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
            self.clock.advance(MANUAL_LISTEN_TIMEOUT + 1.0)
            raise socket.timeout("simulated timeout")

    clock = FakeClock()
    try:
        serve_once(TimeoutAtFirstRecv(clock), clock=clock)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("missing registration accepted")

    class TimeoutBeforeSent(ProtocolSocket):
        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            if self.incoming and self.incoming[0][0].startswith(
                    b"MS05 SENT"):
                raise socket.timeout("simulated timeout")
            return super().recvfrom(size)

    try:
        serve_once(TimeoutBeforeSent())  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("missing SENT accepted")

    # A grace period that never meets the reported SENT count times out.
    grace_short = ProtocolSocket()
    grace_short.incoming[-1] = (b"MS05 SENT tx-only 97", peer)
    try:
        serve_once(grace_short)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("short grace count accepted")

    # A drip-fed exchange delivers every datagram within its per-recv window,
    # so only the single absolute exchange deadline can stop it. Once the
    # remaining exchange budget falls below the drip delta, the clamped
    # receive timeout fires and the exchange fails deterministically.
    clock = FakeClock()
    drip = DripFeedSocket(count=96, delta=0.9, clock=clock)
    try:
        serve_once(drip, clock=clock, exchange_timeout=2.0)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("drip-fed exchange renewed past exchange deadline")

    # A delayed READY send that crosses the exchange deadline must fail
    # instead of renewing the exchange lifetime.
    clock = FakeClock()
    delayed_ready = ProtocolSocket(send_delay=2.5, clock=clock)
    try:
        serve_once(delayed_ready, clock=clock,
                   exchange_timeout=2.0)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("delayed READY send renewed exchange deadline")

    # A delayed bidirectional data send that crosses the deadline must fail;
    # the host cannot renew the deadline by sending the next datagram.
    clock = FakeClock()
    delayed_data = ProtocolSocket(mode="bidirectional", send_delay=2.5,
                                  clock=clock)
    try:
        serve_once(delayed_data, clock=clock,
                   exchange_timeout=2.0)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("delayed data send renewed exchange deadline")

    # A delayed DONE send that crosses the deadline must fail after SENT, so
    # the exchange cannot renew its lifetime to deliver a late DONE.
    clock = FakeClock()
    delayed_done = ProtocolSocket(send_delay=2.5, clock=clock)
    try:
        serve_once(delayed_done, clock=clock,
                   exchange_timeout=2.0)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("delayed DONE send renewed exchange deadline")

    # A send within the budget succeeds: the full protocol exchange closes
    # before the deadline despite a non-zero per-send delay.
    clock = FakeClock()
    affordable = ProtocolSocket(send_delay=0.05, clock=clock)
    result = serve_once(affordable, clock=clock,
                        exchange_timeout=2.0)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)

    # RED: a receive that completes at the exchange deadline must be rejected
    # at the receive boundary, not accepted and only caught at the next send.
    clock = FakeClock()

    class LateRecvSocket(ProtocolSocket):
        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            data, source = super().recvfrom(size)
            clock.advance(EXCHANGE_TIMEOUT)  # reach exactly the deadline
            return data, source

    try:
        serve_once(LateRecvSocket(clock=clock), clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError as error:
        assert "after receive" in str(error), error
    else:
        raise AssertionError("late receive accepted")

    # RED: a send must install a socket timeout no larger than the current
    # exchange remainder; today it inherits the last receive timeout.
    clock = FakeClock()

    class ClampSocket(ProtocolSocket):
        def __init__(self) -> None:
            super().__init__(count=96, mode="tx-only", clock=clock)
            self.timeout: float | None = None
            self.sends: list[tuple[float | None, float]] = []

        def settimeout(self, value: float | None) -> None:
            self.timeout = value

        def gettimeout(self) -> float | None:
            return self.timeout

        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            data, source = super().recvfrom(size)
            # The listen-phase REGISTER is operator-paced and consumes zero
            # exchange budget; only exchange-phase recvs advance the clock.
            if data.startswith(b"MS05 REGISTER"):
                return data, source
            if not self.outgoing:
                clock.advance(EXCHANGE_TIMEOUT - 0.5)
            else:
                clock.advance(0.5)
            return data, source

        def sendto(self, packet: bytes,
                   destination: tuple[str, int]) -> None:
            self.sends.append((self.timeout,
                               EXCHANGE_TIMEOUT - clock.now))
            super().sendto(packet, destination)

    clamp_sock = ClampSocket()
    try:
        serve_once(clamp_sock, clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("clamped exchange unexpectedly succeeded")
    installed, remaining = clamp_sock.sends[0]
    assert installed is not None and installed <= remaining + 1e-9, \
        (installed, remaining)

    # RED→GREEN: a timeout setter that itself consumes the final budget must
    # not let the following send/receive start. `settimeout()` advances the
    # fake clock by one configured delay; the next operation starts only when
    # a fresh clock re-read after the setter still has budget.
    class SetterDelaySocket(ProtocolSocket):
        def __init__(self, setter_delays, clock):
            super().__init__(count=96, mode="tx-only", clock=clock)
            self.setter_delays = list(setter_delays)
            self.send_calls = 0
            self.recv_calls = 0
            self.timeout = None

        def settimeout(self, value: float | None) -> None:
            self.timeout = value
            if self.setter_delays:
                self.clock.advance(self.setter_delays.pop(0))

        def gettimeout(self) -> float | None:
            return self.timeout

        def sendto(self, packet: bytes,
                   destination: tuple[str, int]) -> None:
            self.send_calls += 1
            super().sendto(packet, destination)

        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            self.recv_calls += 1
            return super().recvfrom(size)

    # The 2nd settimeout is the READY send's (the 1st is the inert listen
    # settimeout): it lands exactly at the exchange deadline, so the send must
    # never start.
    clock = FakeClock()
    late_send = SetterDelaySocket([0.0, EXCHANGE_TIMEOUT], clock)
    try:
        serve_once(late_send, clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("late send setter accepted")
    assert late_send.send_calls == 0, late_send.send_calls
    assert late_send.outgoing == [], late_send.outgoing

    # The 2nd settimeout landing past the deadline behaves identically.
    clock = FakeClock()
    late_send_past = SetterDelaySocket(
        [0.0, EXCHANGE_TIMEOUT + 0.5], clock)
    try:
        serve_once(late_send_past, clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("past-deadline send setter accepted")
    assert late_send_past.send_calls == 0, late_send_past.send_calls

    # A late READY-send setter on the exchange budget still lets the listen
    # phase complete its REGISTER receive, but no READY send follows.
    clock = FakeClock()
    late_recv = SetterDelaySocket([0.0, EXCHANGE_TIMEOUT], clock)
    try:
        serve_once(late_recv, clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("late receive setter accepted")
    assert late_recv.recv_calls == 1, late_recv.recv_calls
    assert late_recv.outgoing == [], late_recv.outgoing

    # Same for a send setter landing past the exchange deadline.
    clock = FakeClock()
    late_recv_past = SetterDelaySocket(
        [0.0, EXCHANGE_TIMEOUT + 0.5], clock)
    try:
        serve_once(late_recv_past, clock=clock,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("past-deadline receive setter accepted")
    assert late_recv_past.recv_calls == 1, late_recv_past.recv_calls

    # An affordable setter delay must not break the exchange: fresh prechecks
    # still allow I/O while budget remains.
    clock = FakeClock()
    affordable_setter = SetterDelaySocket([0.05] * 200, clock)
    result = serve_once(affordable_setter, clock=clock,
                        exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)
    assert affordable_setter.send_calls == 2, affordable_setter.send_calls

    # DONE/ACK (repair 6.2-R5): parse_ack accepts the correct mode/count and
    # rejects malformed, wrong-mode and negative ACKs.
    assert parse_ack(b"MS05 ACK tx-only 96", "tx-only") == ("tx-only", 96)
    for bad_ack in (
        b"MS05 ACK other 96",
        b"MS05 ACK tx-only -1",
        b"MS05 ACK tx-only bad",
        b"bad",
    ):
        try:
            parse_ack(bad_ack, "tx-only")
        except ValueError:
            pass
        else:
            raise AssertionError(f"malformed ack accepted: {bad_ack!r}")

    # Wrong ACK count (differs from the DONE count the host sent) must fail.
    wrong_count_ack = ProtocolSocket()
    wrong_count_ack.incoming[-1] = (b"MS05 ACK tx-only 95", peer)
    try:
        serve_once(wrong_count_ack)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("wrong-count ACK accepted")

    # Missing ACK (the exchange completes on the host side only after a valid
    # ACK, so a guest that never ACKs fails deterministically).
    missing_ack = ProtocolSocket()
    missing_ack.incoming = missing_ack.incoming[:-1]
    try:
        serve_once(missing_ack)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("missing ACK accepted")

    # ACK from an unexpected peer fails before its contents matter.
    wrong_peer_ack = ProtocolSocket()
    wrong_peer_ack.incoming[-1] = (b"MS05 ACK tx-only 96", ("attacker", 9999))
    try:
        serve_once(wrong_peer_ack)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("ACK from unexpected peer accepted")

    # ACK for the wrong mode fails even when the count matches.
    wrong_mode_ack = ProtocolSocket()
    wrong_mode_ack.incoming[-1] = (b"MS05 ACK bidirectional 96", peer)
    try:
        serve_once(wrong_mode_ack)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("wrong-mode ACK accepted")

    # Manual-listen/exchange split (repair 6.2-R5): a REGISTER arriving after a
    # long operator delay still consumes none of the short exchange budget, so
    # the full exchange completes with a fresh deadline. The listen deadline
    # governs only the REGISTER wait; the exchange deadline starts fresh after
    # it. A delayed-registration socket advances the clock only during the
    # listen phase and verifies Register before the exchange deadline is set.
    class DelayedRegistration(ProtocolSocket):
        def __init__(self, listen_delay: float, clock: FakeClock) -> None:
            super().__init__(clock=clock)
            self.listen_delay = listen_delay

        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            data, source = super().recvfrom(size)
            if data.startswith(b"MS05 REGISTER"):
                self.clock.advance(self.listen_delay)
            return data, source

    clock = FakeClock()
    delayed = DelayedRegistration(listen_delay=50.0, clock=clock)
    result = serve_once(delayed, clock=clock,
                        exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)
    # The exchange deadline is set after the listen delay has already advanced
    # the clock, so a 50s operator delay leaves the full exchange budget intact.
    assert clock.now >= 50.0

    # A REGISTER arriving at/after the listen deadline fails the listen phase,
    # independent of the (unstarted) exchange budget.
    class LateRegistration(ProtocolSocket):
        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            if self.incoming and self.incoming[0][0].startswith(b"MS05 REGISTER"):
                clock.advance(EXCHANGE_TIMEOUT + 1.0)
            return super().recvfrom(size)

    clock = FakeClock()
    try:
        serve_once(LateRegistration(clock=clock), clock=clock,
                   listen_timeout=EXCHANGE_TIMEOUT,
                   exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    except ValueError:
        pass
    else:
        raise AssertionError("late registration accepted")

    # RED (repair 6.2-R6): registration stays open across intermediate receive
    # timeouts. An operator-paced listen that times out once before the valid
    # REGISTER must keep listening under the same absolute deadline, not exit
    # after the first timeout. This fails today because `listen_for_register`
    # performs a single recvfrom and a timeout aborts the exchange.
    class TimeoutThenRegister(ProtocolSocket):
        def __init__(self, clock: FakeClock) -> None:
            super().__init__(clock=clock)
            self.first = True

        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            if self.first:
                self.first = False
                raise socket.timeout("intermediate listen timeout")
            return super().recvfrom(size)

    clock = FakeClock()
    timeout_then = TimeoutThenRegister(clock=clock)
    result = serve_once(timeout_then, clock=clock,
                        listen_timeout=EXCHANGE_TIMEOUT,
                        exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)

    # RED (repair 6.2-R6): an invalid pre-registration datagram does not start
    # the exchange. A noise-then-valid witness must skip the noise and accept
    # the later valid REGISTER under the same absolute deadline. This fails
    # today because `listen_for_register` returns the first datagram verbatim,
    # and `parse_control` then rejects the noise and aborts the exchange.
    class NoiseThenRegister(ProtocolSocket):
        def __init__(self, clock: FakeClock) -> None:
            super().__init__(clock=clock)
            self.noise = True

        def recvfrom(self, size: int) -> tuple[bytes, tuple[str, int]]:
            if self.noise:
                self.noise = False
                return (b"garbage-not-a-register", ("guest", 4242))
            return super().recvfrom(size)

    clock = FakeClock()
    noise_then = NoiseThenRegister(clock=clock)
    result = serve_once(noise_then, clock=clock,
                        listen_timeout=EXCHANGE_TIMEOUT,
                        exchange_timeout=EXCHANGE_TIMEOUT)  # type: ignore[arg-type]
    assert result == ("tx-only", 96, 64, 96, 96)

    print("ms05 stimulus self-test: protocol=PASS "
          "malformed=PASS reorder=PASS duplicate=PASS missing=PASS")
    print("ms05 stimulus self-test: done=PASS sent=PASS payload=PASS "
          "peer=PASS timeout=PASS grace=PASS drip=PASS")
    print("ms05 stimulus self-test: delayed-ready=PASS delayed-data=PASS "
          "delayed-done=PASS affordable-send=PASS")
    print("ms05 stimulus self-test: late-send-setter=PASS "
          "past-send-setter=PASS late-recv-setter=PASS "
          "past-recv-setter=PASS affordable-setter=PASS")
    print("ms05 stimulus self-test: ack=PASS wrong-count=PASS missing=PASS "
          "wrong-peer=PASS wrong-mode=PASS listen-split=PASS late-register=PASS")


def loopback_self_test() -> None:
    worker_result: list[tuple[str, int, int, int, int]] = []
    worker_error: list[Exception] = []

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as server:
        server.settimeout(SELF_TEST_TIMEOUT)
        server.bind(("127.0.0.1", 0))
        server_address = server.getsockname()

        def run_server() -> None:
            try:
                worker_result.append(serve_once(server))
            except Exception as error:
                worker_error.append(error)

        worker = threading.Thread(target=run_server, name="ms05-loopback-server")
        worker.start()
        try:
            with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as client:
                client.settimeout(SELF_TEST_TIMEOUT)
                client.connect(server_address)
                client.send(b"MS05 REGISTER tx-only 96 64")
                assert client.recv(256) == b"MS05 READY tx-only 96 64"
                client.send(b"MS05 START tx-only 96 64")
                for sequence in range(96):
                    client.send(make_packet(sequence, 96, 64))
                client.send(b"MS05 SENT tx-only 96")
                assert client.recv(256) == b"MS05 DONE tx-only 96"
                client.send(b"MS05 ACK tx-only 96")
        finally:
            worker.join(SELF_TEST_TIMEOUT)

        if worker.is_alive():
            raise TimeoutError("real-loopback worker did not finish")
        if worker_error:
            raise worker_error[0]
        assert worker_result == [("tx-only", 96, 64, 96, 96)]
    print("ms05 stimulus loopback self-test: protocol=PASS "
          "datagrams=96 sequence=PASS bounded=PASS")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--loopback-self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test and args.loopback_self_test:
        parser.error("self-test modes are mutually exclusive")
    if args.self_test:
        self_test()
        return 0
    if args.loopback_self_test:
        loopback_self_test()
        return 0
    if not 1 <= args.port <= 65535:
        parser.error("port must be in 1..65535")

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((args.host, args.port))
        mode, count, payload, received, _sent = serve_once(sock)
    print(f"ms05 stimulus: PASS mode={mode} count={count} "
          f"payload={payload} received={received}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
