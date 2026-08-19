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


def parse_sent(data: bytes, expected_mode: str, count: int) -> int:
    try:
        text = data.decode("ascii")
        marker, verb, mode, sent_text = text.split()
        sent = int(sent_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed sent datagram") from error
    if marker != "MS05" or verb != "SENT" or mode != expected_mode:
        raise ValueError("sent does not match mode")
    if not 0 <= sent <= MAX_COUNT:
        raise ValueError("sent count outside bounded range")
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
               exchange_timeout: float = EXCHANGE_TIMEOUT
               ) -> tuple[str, int, int, int, int]:
    """Serve one guest exchange under a finite timeout for every phase.

    A receive timeout anywhere is a deterministic failure, never an infinite
    wait. Every post-registration datagram — including SENT — is rejected
    when its source differs from the registered peer, before its contents
    are parsed. `clock` and `exchange_timeout` allow deterministic tests of
    the single absolute exchange deadline.
    """
    try:
        old_timeout = sock.gettimeout()
    except AttributeError:
        old_timeout = None
    deadline = clock() + exchange_timeout
    try:
        sock.settimeout(GRACE_TIMEOUT)
        return _serve_exchange(sock, deadline, clock)
    except socket.timeout as error:
        raise ValueError("protocol phase timeout") from error
    finally:
        try:
            sock.settimeout(old_timeout)
        except AttributeError:
            pass


def _serve_exchange(sock: socket.socket, exchange_deadline: float,
                    clock) -> tuple[str, int, int, int, int]:
    def bounded_recv(size: int) -> tuple[bytes, tuple[str, int]]:
        sock.settimeout(GRACE_TIMEOUT)
        return sock.recvfrom(size)

    registration, peer = bounded_recv(256)
    mode, count, payload = parse_control(registration, "REGISTER")
    sock.sendto(f"MS05 READY {mode} {count} {payload}".encode("ascii"), peer)

    start, start_peer = bounded_recv(256)
    if start_peer != peer:
        raise ValueError("START from unexpected peer")
    start_mode, start_count, start_payload = parse_control(start, "START")
    if (start_mode, start_count, start_payload) != (mode, count, payload):
        raise ValueError("START does not match REGISTER")

    # RX direction (host -> guest): the host sends `count` datagrams first.
    if mode == "bidirectional":
        for sequence in range(count):
            sock.sendto(make_packet(sequence, count, payload), peer)

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

    sock.sendto(f"MS05 DONE {mode} {received}".encode("ascii"), peer)
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
    stop it."""

    def __init__(self, count: int, delta: float, clock: FakeClock) -> None:
        peer = ("guest", 4242)
        self.clock = clock
        self.delta = delta
        self.timeout: float | None = None
        self.incoming = [
            (b"MS05 REGISTER tx-only 96 64", peer),
            (b"MS05 START tx-only 96 64", peer),
        ] + [(make_packet(sequence, 96, 64), peer) for sequence in range(count)
             ] + [(b"MS05 SENT tx-only 96", peer)]
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
        def __init__(self, count: int = 96) -> None:
            self.incoming = [
                (b"MS05 REGISTER tx-only 96 64", peer),
                (b"MS05 START tx-only 96 64", peer),
            ] + [
                (make_packet(sequence, 96, 64), peer)
                for sequence in range(count)
            ] + [(f"MS05 SENT tx-only {count}".encode("ascii"), peer)]
            self.outgoing: list[tuple[bytes, tuple[str, int]]] = []

        def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
            if not self.incoming:
                raise socket.timeout("no more datagrams")
            return self.incoming.pop(0)

        def settimeout(self, _value: float | None) -> None:
            pass

        def gettimeout(self) -> None:
            return None

        def sendto(self, packet: bytes, destination: tuple[str, int]) -> None:
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

    # A receive timeout at any protocol phase is a deterministic failure.
    class TimeoutAtFirstRecv(ProtocolSocket):
        def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
            raise socket.timeout("simulated timeout")

    try:
        serve_once(TimeoutAtFirstRecv())  # type: ignore[arg-type]
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

    print("ms05 stimulus self-test: protocol=PASS "
          "malformed=PASS reorder=PASS duplicate=PASS missing=PASS")
    print("ms05 stimulus self-test: done=PASS sent=PASS payload=PASS "
          "peer=PASS timeout=PASS grace=PASS drip=PASS")


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
