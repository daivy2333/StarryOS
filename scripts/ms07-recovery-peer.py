#!/usr/bin/env python3
"""Bounded UDP echo peer for the manual MS07 QEMU recovery protocol.

It never starts QEMU, opens a guest shell, or drives HMP.  The operator starts
it before the guest probe; the guest supplies a run id, phase, and sequence.
"""
import argparse
import select
import socket
import sys
import time


def decode_packet(payload):
    try:
        text = payload.decode("ascii")
        fields = dict(item.split("=", 1) for item in text.split())
    except (UnicodeDecodeError, ValueError):
        return None
    if set(fields) != {"run", "phase", "seq"} or not fields["run"] or not fields["phase"]:
        return None
    try:
        seq = int(fields["seq"], 10)
    except ValueError:
        return None
    return fields["run"], fields["phase"], seq


class PeerLedger:
    def __init__(self):
        self.expected = {}

    def accept(self, packet, address):
        if packet is None:
            return False
        run, phase, seq = packet
        key = (run, phase, address)
        expected = self.expected.get(key, 0)
        if seq != expected:
            return False
        self.expected[key] = expected + 1
        return True


def self_test():
    ledger = PeerLedger()
    address = ("127.0.0.1", 1234)
    assert decode_packet(b"run=a phase=pre seq=0") == ("a", "pre", 0)
    assert decode_packet(b"run=a phase=pre seq=x") is None
    assert decode_packet(b"run=a phase=pre seq=0 extra=x") is None
    assert ledger.accept(("a", "pre", 0), address)
    assert not ledger.accept(("a", "pre", 0), address)
    assert not ledger.accept(("a", "pre", 2), address)
    assert ledger.accept(("a", "pre", 1), address)


def serve(host, port, deadline_seconds):
    ledger = PeerLedger()
    deadline = time.monotonic() + deadline_seconds
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as listener:
        listener.bind((host, port))
        while time.monotonic() < deadline:
            timeout = max(0.0, deadline - time.monotonic())
            readable, _, _ = select.select([listener], [], [], timeout)
            if not readable:
                break
            payload, address = listener.recvfrom(512)
            packet = decode_packet(payload)
            if ledger.accept(packet, address):
                listener.sendto(payload, address)
        return 0


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=15572)
    parser.add_argument("--deadline-seconds", type=int, default=180)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if not 0 < args.port < 65536 or args.deadline_seconds <= 0:
        parser.error("port and deadline must be positive")
    return serve(args.host, args.port, args.deadline_seconds)


if __name__ == "__main__":
    sys.exit(main())
