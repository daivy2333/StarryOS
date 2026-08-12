#!/usr/bin/env python3
"""Bounded UDP RX stimulus for the MS04 guest probe."""

from __future__ import annotations

import argparse
import socket
import struct


MAGIC = 0x4D533034
DEFAULT_HOST = "0.0.0.0"
DEFAULT_PORT = 15556
MIN_COUNT = 65
MAX_COUNT = 256
MIN_PAYLOAD = 16
MAX_PAYLOAD = 1200


def parse_control(data: bytes, verb: str) -> tuple[int, int]:
    try:
        text = data.decode("ascii")
        marker, actual_verb, count_text, payload_text = text.split()
        count = int(count_text, 10)
        payload = int(payload_text, 10)
    except (UnicodeDecodeError, ValueError) as error:
        raise ValueError("malformed control datagram") from error
    if marker != "MS04" or actual_verb != verb:
        raise ValueError(f"expected MS04 {verb}")
    if not MIN_COUNT <= count <= MAX_COUNT:
        raise ValueError("count outside bounded range")
    if not MIN_PAYLOAD <= payload <= MAX_PAYLOAD:
        raise ValueError("payload outside bounded range")
    return count, payload


def make_packet(sequence: int, count: int, payload_size: int) -> bytes:
    payload = bytes((sequence + index) & 0xFF for index in range(payload_size))
    return struct.pack("!III", MAGIC, sequence, count) + payload


def serve_once(sock: socket.socket) -> tuple[int, int]:
    registration, peer = sock.recvfrom(256)
    count, payload = parse_control(registration, "REGISTER")
    sock.sendto(f"MS04 READY {count} {payload}".encode("ascii"), peer)

    start, start_peer = sock.recvfrom(256)
    start_count, start_payload = parse_control(start, "START")
    if start_peer != peer or (start_count, start_payload) != (count, payload):
        raise ValueError("START does not match REGISTER")

    for sequence in range(count):
        sock.sendto(make_packet(sequence, count, payload), peer)
    return count, payload


def self_test() -> None:
    assert parse_control(b"MS04 REGISTER 96 64", "REGISTER") == (96, 64)
    for malformed in (
        b"bad",
        b"MS04 REGISTER 64 64",
        b"MS04 REGISTER 96 15",
        b"MS04 START 96 64",
    ):
        try:
            parse_control(malformed, "REGISTER")
        except ValueError:
            pass
        else:
            raise AssertionError(f"malformed registration accepted: {malformed!r}")

    peer = ("guest", 4242)

    class ProtocolSocket:
        def __init__(self) -> None:
            self.incoming = [
                (b"MS04 REGISTER 96 64", peer),
                (b"MS04 START 96 64", peer),
            ]
            self.outgoing: list[tuple[bytes, tuple[str, int]]] = []

        def recvfrom(self, _size: int) -> tuple[bytes, tuple[str, int]]:
            return self.incoming.pop(0)

        def sendto(self, packet: bytes, destination: tuple[str, int]) -> None:
            self.outgoing.append((packet, destination))

    protocol_socket = ProtocolSocket()
    assert serve_once(protocol_socket) == (96, 64)  # type: ignore[arg-type]
    assert protocol_socket.outgoing[0] == (b"MS04 READY 96 64", peer)
    assert len(protocol_socket.outgoing) == 97
    for sequence, (packet, destination) in enumerate(protocol_socket.outgoing[1:]):
        assert destination == peer
        magic, actual_sequence, count = struct.unpack("!III", packet[:12])
        assert (magic, actual_sequence, count) == (MAGIC, sequence, 96)
        assert packet[12:] == bytes((sequence + index) & 0xFF for index in range(64))
    print("ms04 stimulus self-test: protocol=PASS packets=96 sequence=PASS bounds=PASS malformed=PASS")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        self_test()
        return 0
    if not 1 <= args.port <= 65535:
        parser.error("port must be in 1..65535")

    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as sock:
        sock.bind((args.host, args.port))
        count, payload = serve_once(sock)
    print(f"ms04 stimulus: PASS packets={count} payload={payload}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
