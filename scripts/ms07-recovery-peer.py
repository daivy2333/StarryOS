#!/usr/bin/env python3
"""Bounded UDP echo peer for the manual MS07 QEMU recovery protocol.

It never starts QEMU, opens a guest shell, or drives HMP.  The operator starts
it before the guest probe; the guest sends a phase and sequence.

It accepts exactly the three network phases in protocol order, each with
seq=0, regardless of the source address. Every wait is bounded by an absolute
deadline.
"""
import argparse
import select
import socket
import sys
import time

EXCHANGE_PHASES = (
    "pre_reset_traffic", "new_epoch_traffic", "hmp_link_up",
)


def decode_packet(payload):
    try:
        text = payload.decode("ascii")
        fields = dict(item.split("=", 1) for item in text.split())
    except (UnicodeDecodeError, ValueError):
        return None
    if set(fields) != {"phase", "seq"} or not fields["phase"]:
        return None
    if fields["phase"] not in EXCHANGE_PHASES:
        return None
    try:
        seq = int(fields["seq"], 10)
    except ValueError:
        return None
    if seq < 0 or seq > (1 << 32) - 1:
        return None
    return fields["phase"], seq


class PeerLedger:
    def __init__(self):
        self.next_phase = 0

    def accept(self, packet):
        if packet is None:
            return False
        phase, seq = packet
        if seq != 0:
            return False
        if self.next_phase >= len(EXCHANGE_PHASES) or phase != EXCHANGE_PHASES[self.next_phase]:
            return False
        self.next_phase += 1
        return True


def _serve_until_deadline(listener, ledger, deadline, now, select_fn=select.select,
                          on_accept=None):
    """Loop until `deadline` (monotonic seconds).  `now`/`select_fn` are
    injectable so the host test can drive a fake clock and fake socket; the
    production caller passes `time.monotonic` and `select.select`, and the
    listener is nonblocking so no receive can park past the deadline.

    `on_accept` (optional) is called with the decoded `(phase, seq)` after each
    accepted exchange, so a real manual run can observe which guest phases the
    peer actually received and echoed.  It is None in the synthetic tests.

    The deadline is re-checked after `select_fn` returns AND after each
    receive/decode before echo, so a peer that select reported readable after
    the deadline, or a nonblocking receive that advanced the clock past it,
    never causes a stale echo."""
    while now() < deadline:
        timeout = max(0.0, deadline - now())
        readable, _, _ = select_fn([listener], [], [], timeout)
        if not readable or not (now() < deadline):
            break
        payload, address = listener.recvfrom(512)
        if not (now() < deadline):
            break
        packet = decode_packet(payload)
        if ledger.accept(packet):
            if on_accept:
                on_accept(packet)
            listener.sendto(payload, address)
    return 0


class FakeClock:
    def __init__(self, now):
        self._now = now

    def __call__(self):
        return self._now


class FakeSocket:
    def __init__(self):
        self.packets = []
        self.sent = []

    def recvfrom(self, _size):
        if not self.packets:
            raise OSError("no packet")
        payload, address = self.packets.pop(0)
        return payload, address

    def sendto(self, payload, _address):
        self.sent.append(payload)


class FakeSelect:
    def select(self, rlist, wlist, xlist, timeout):
        sock = rlist[0]
        if sock.packets:
            return [sock], [], []
        return [], [], []


class ReadableSelect:
    """Reports the listener readable immediately without advancing the clock,
    so the deadline can be crossed only inside a later receive."""

    def select(self, rlist, wlist, xlist, timeout):
        return [rlist[0]], [], []


class RecvCrossesDeadlineSocket:
    """A fake listener whose `recvfrom` advances the fake clock past the
    deadline after select already reported it readable.  Proves the peer
    re-checks `now() < deadline` after receive/decode and before echo, so a
    packet observed with a stale clock is never sent back."""

    def __init__(self, clock, packet, address, jump_by):
        self.clock = clock
        self.packet = packet
        self.address = address
        self.jump_by = jump_by
        self.sent = []

    def recvfrom(self, _size):
        self.clock._now += self.jump_by
        return self.packet, self.address

    def sendto(self, payload, _address):
        self.sent.append(payload)


class LateReadableSelect:
    """Returns a readable socket while advancing the clock past the deadline,
    proving the peer re-checks `now() < deadline` after select returns and
    therefore never echoes a packet observed with an expired deadline."""

    def __init__(self, clock, jump_by):
        self.clock = clock
        self.jump_by = jump_by

    def select(self, rlist, wlist, xlist, timeout):
        self.clock._now += self.jump_by
        return [rlist[0]], [], []


def self_test():
    address = ("127.0.0.1", 1234)
    foreign = ("10.1.2.3", 99)
    assert decode_packet(b"phase=pre_reset_traffic seq=0") == ("pre_reset_traffic", 0)
    assert decode_packet(b"phase=pre_reset_traffic seq=x") is None
    assert decode_packet(b"phase=pre_reset_traffic seq=0 extra=x") is None
    assert decode_packet(b"phase=not_a_case seq=0") is None
    assert decode_packet(b"phase= seq=0") is None
    assert decode_packet(b"phase=pre_reset_traffic seq=18446744073709551616") is None

    # The runtime ledger carries no run id and does not pin the guest IP: it
    # accepts exactly the three exchange phases in protocol order regardless of
    # source address, and rejects a nonzero seq, an unknown/absolute phase, or
    # a duplicate of the current phase.
    ledger = PeerLedger()
    assert ledger.accept(("pre_reset_traffic", 0))
    assert not ledger.accept(("pre_reset_traffic", 0))
    assert not ledger.accept(("reset_request", 0))
    assert not ledger.accept(("hmp_link_up", 0))
    assert not ledger.accept(("pre_reset_traffic", 1))
    assert ledger.accept(("new_epoch_traffic", 0))
    assert ledger.accept(("hmp_link_up", 0))
    assert not ledger.accept(("pre_reset_traffic", 0))

    # Fake-clock / fake-socket: a packet accepted before deadline is echoed; the
    # loop stops cleanly once the clock passes the deadline with no packet.
    fake = FakeSocket()
    fake.packets = [(b"phase=pre_reset_traffic seq=0", address)]
    clock = FakeClock(100.0)
    fake_select = FakeSelect()
    _serve_until_deadline(fake, PeerLedger(), 101.0, clock, fake_select.select)
    assert fake.sent == [b"phase=pre_reset_traffic seq=0"]
    before = len(fake.sent)
    _serve_until_deadline(fake, PeerLedger(), 101.0, clock, fake_select.select)
    assert len(fake.sent) == before, "no echo after the absolute deadline"

    # Adversarial: select reports the socket readable while advancing the clock
    # past the deadline. The peer must re-check now() < deadline after select
    # returns and must NOT recv/send a stale packet.
    late = FakeSocket()
    late.packets = [(b"phase=new_epoch_traffic seq=0", foreign)]
    adv = FakeClock(100.0)
    late_select = LateReadableSelect(adv, 2.0)
    _serve_until_deadline(late, PeerLedger(), 101.0, adv, late_select.select)
    assert len(late.sent) == 0, "stale packet echoed after readable-past-deadline"

    # A3: select reports the socket readable while the clock is still under the
    # deadline, but the blocking `recvfrom` itself advances the clock past it.
    # The peer must re-check `now() < deadline` after receive/decode and must
    # NOT echo a packet observed with a stale clock.  This is a different window
    # from LateReadableSelect (which crosses at the select) because the receive
    # op is the crossing point here.
    cross = FakeClock(100.0)
    cross_sock = RecvCrossesDeadlineSocket(cross, b"phase=pre_reset_traffic seq=0", address, 2.0)
    readable = ReadableSelect()
    _serve_until_deadline(cross_sock, PeerLedger(), 101.0, cross, readable.select)
    assert len(cross_sock.sent) == 0, "stale packet echoed after recv crossed the deadline"


def serve(host, port, deadline_seconds):
    deadline = time.monotonic() + deadline_seconds
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as listener:
        listener.bind((host, port))
        # Nonblocking so a receive that selects readable can never park the
        # peer past its absolute deadline; the loop re-checks the clock after.
        listener.setblocking(False)
        return _serve_until_deadline(
            listener, PeerLedger(), deadline, time.monotonic,
            on_accept=lambda packet: print(f"peer: accepted phase={packet[0]} seq={packet[1]}", flush=True),
        )


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
