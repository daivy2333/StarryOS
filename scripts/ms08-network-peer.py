#!/usr/bin/env python3
"""Bounded TCP/UDP echo peer for the MS08 QEMU SMP network qualification.

It never starts QEMU, opens a guest shell, or drives HMP.  The operator starts
it before the guest probe; the guest reaches it at the QEMU user-net host
10.0.2.2 on the MS08 port.  Every frame is one ``case=<case> seq=N`` text
frame: per case the sequence must start at 1 and increase strictly by one,
across whichever transport the case uses.  The peer echoes the exact frame
bytes back, rejects duplicate/out-of-order/foreign traffic, and reports
per-case received/sent counts plus an explicit outcome line joined to the
guest transcript by the validator.

All waits are bounded by one absolute deadline and re-checked after select
and after every receive, so a stale read can never produce a late echo.
"""
import argparse
import select
import socket
import sys
import time

PEER_PORT = 15578
PEER_CASES = (
    "tcp-bidirectional", "udp-bidirectional", "full-recovery",
    "readiness-quiet",
)


def decode_frame(payload):
    """Parse one ``case=<case> seq=N`` frame; None on any grammar drift."""
    try:
        text = payload.decode("ascii")
    except UnicodeDecodeError:
        return None
    if text.endswith("\n"):
        text = text[:-1]
    fields = {}
    for item in text.split():
        key, sep, value = item.partition("=")
        if not sep or not key or key in fields:
            return None
        fields[key] = value
    if set(fields) != {"case", "seq"} or not fields["case"]:
        return None
    if fields["case"] not in PEER_CASES:
        return None
    try:
        seq = int(fields["seq"], 10)
    except ValueError:
        return None
    if seq <= 0 or seq > (1 << 32) - 1:
        return None
    return fields["case"], seq


class PeerLedger:
    """Per-case strictly-increasing sequence ledger (seq starts at 1)."""

    def __init__(self):
        self.next_seq = {case: 1 for case in PEER_CASES}
        self.received = {case: 0 for case in PEER_CASES}
        self.sent = {case: 0 for case in PEER_CASES}
        self.rejects = 0

    def accept(self, packet):
        if packet is None:
            self.rejects += 1
            return False
        case, seq = packet
        if seq != self.next_seq[case]:
            self.rejects += 1
            return False
        self.next_seq[case] += 1
        self.received[case] += 1
        return True

    def record_echo(self, case):
        self.sent[case] += 1

    def summary(self):
        return " ".join(
            f"{case}:rx={self.received[case]},tx={self.sent[case]}"
            for case in PEER_CASES
        )


def serve_until_deadline(udp_listener, tcp_listener, ledger, deadline, now,
                         select_fn=select.select,
                         tcp_rx_buffers=None, send_all_fn=None):
    """Pump both listeners until the absolute deadline.  Re-checks the clock
    after select returns and after every receive, so no stale packet is ever
    echoed.  Per-connection stream buffers are owned by this service (keyed by
    the connection object, never stored as socket attributes: a real CPython
    ``socket.socket`` has no per-instance dictionary).  `now`/`select_fn` are
    injectable for the self-test."""
    if tcp_rx_buffers is None:
        tcp_rx_buffers = {}
    if send_all_fn is None:
        send_all_fn = lambda conn, payload, clock, end: send_all(
            conn, payload, clock, end, select_fn)
    tcp_conns = []
    while now() < deadline:
        timeout = max(0.0, deadline - now())
        readable, _, _ = select_fn(
            [udp_listener, tcp_listener] + tcp_conns, [], [], timeout)
        if not readable or not (now() < deadline):
            break
        for sock in readable:
            if sock is tcp_listener:
                conn, _addr = sock.accept()
                conn.setblocking(False)
                tcp_conns.append(conn)
                tcp_rx_buffers[conn] = bytearray()
                continue
            if sock is udp_listener:
                try:
                    payload, address = udp_listener.recvfrom(512)
                except BlockingIOError:
                    continue
                if not (now() < deadline):
                    continue
                packet = decode_frame(payload)
                if ledger.accept(packet):
                    udp_listener.sendto(payload, address)
                    ledger.record_echo(packet[0])
                continue
            # TCP stream: frames are newline-terminated; a connection may
            # deliver several frames in one segment, and one frame may arrive
            # split across segments.  The reassembly buffer belongs to this
            # service, not to the socket object.
            try:
                data = sock.recv(4096)
            except BlockingIOError:
                continue
            if data == b"":
                tcp_conns.remove(sock)
                tcp_rx_buffers.pop(sock, None)
                sock.close()
                continue
            if not (now() < deadline):
                continue
            buf = tcp_rx_buffers.setdefault(sock, bytearray())
            buf += data
            while b"\n" in buf:
                raw, _, rest = buf.partition(b"\n")
                tcp_rx_buffers[sock] = buf = rest
                payload = bytes(raw) + b"\n"
                packet = decode_frame(payload)
                if ledger.accept(packet):
                    if not send_all_fn(sock, payload, now, deadline):
                        break
                    ledger.record_echo(packet[0])
    for conn in tcp_conns:
        tcp_rx_buffers.pop(conn, None)
        conn.close()
    return 0


def send_all(conn, payload, now, deadline, select_fn=select.select):
    """Send every byte of ``payload`` through a nonblocking socket, waiting
    for writable readiness on partial progress.  Returns False when the
    absolute deadline passes with bytes unsent."""
    view = memoryview(payload)
    while view:
        if not now() < deadline:
            return False
        try:
            sent = conn.send(view)
        except BlockingIOError:
            sent = 0
        except OSError:
            return False
        if sent > 0:
            view = view[sent:]
            continue
        _, writable, _ = select_fn([], [conn], [],
                                   max(0.0, deadline - now()))
        if not writable:
            return False
    return True


class FakeClock:
    def __init__(self, now):
        self._now = now

    def __call__(self):
        return self._now


class FakeUdp:
    def __init__(self):
        self.packets = []
        self.sent = []

    def recvfrom(self, _size):
        if not self.packets:
            raise BlockingIOError("no packet")
        return self.packets.pop(0)

    def sendto(self, payload, _address):
        self.sent.append(payload)


class FakeTcpListener:
    def __init__(self):
        self.pending = []

    def accept(self):
        conn = FakeTcpConn()
        self.pending.append(conn)
        return conn, ("127.0.0.1", 4000)


class FakeTcpConn:
    """Stream fake with ``send`` (partial writes and one-shot blockage) so the
    bounded send_all path is exercised without a real socket."""

    __slots__ = ("chunks", "sent", "closed", "block_next")

    def __init__(self):
        self.chunks = []
        self.sent = []
        self.closed = False
        self.block_next = False

    def setblocking(self, _flag):
        return None

    def recv(self, _n):
        if self.closed or not self.chunks:
            return b""
        data = self.chunks.pop(0)
        if data == b"":
            self.closed = True
        return data

    def send(self, payload):
        if self.closed:
            raise OSError("closed")
        if self.block_next:
            self.block_next = False
            raise BlockingIOError("would block")
        if len(payload) > 7:
            payload = payload[:7]  # short write: send_all must loop
        self.sent.append(bytes(payload))
        return len(payload)

    def sendall(self, payload):
        self.sent.append(bytes(payload))

    def close(self):
        self.closed = True


class StaticSelect:
    """Reports only the sockets whose work predicate is true, without
    consuming packets; the deadline is still enforced via the clock."""

    def __init__(self, socks):
        self.socks = socks

    def __call__(self, rlist, wlist, xlist, timeout):
        readable = []
        for sock in rlist:
            if isinstance(sock, FakeUdp) and sock.packets:
                readable.append(sock)
            elif isinstance(sock, _FakeAcceptor) and not sock.used:
                readable.append(sock)
            elif isinstance(sock, FakeTcpConn) and sock.chunks:
                readable.append(sock)
        return readable, [], []


class StaticSelectStream(StaticSelect):
    """Like StaticSelect, but stream connections are always writable so a
    short write can make forward progress inside the deadline."""

    def __call__(self, rlist, wlist, xlist, timeout):
        readable, _, _ = super().__call__(rlist, wlist, xlist, timeout)
        writable = [sock for sock in wlist if isinstance(sock, FakeTcpConn)]
        return readable, writable, []


def self_test():
    udp = FakeUdp()
    tcp = FakeTcpListener()
    ledger = PeerLedger()

    # Frame grammar: strict case/seq fields only.
    assert decode_frame(b"case=tcp-bidirectional seq=1") == ("tcp-bidirectional", 1)
    assert decode_frame(b"case=tcp-bidirectional seq=1\n") == ("tcp-bidirectional", 1)
    assert decode_frame(b"case=tcp-bidirectional seq=0") is None
    assert decode_frame(b"case=unknown seq=1") is None
    assert decode_frame(b"case=tcp-bidirectional seq=1 extra=2") is None
    assert decode_frame(b"case= seq=1") is None
    assert decode_frame(b"case=tcp-bidirectional seq=x") is None
    assert decode_frame(b"\xff\xfe") is None

    # Per-case ledger: strict 1..N per case, independent across cases.
    ledger.accept(decode_frame(b"case=tcp-bidirectional seq=1"))
    assert not ledger.accept(decode_frame(b"case=tcp-bidirectional seq=1"))
    assert not ledger.accept(decode_frame(b"case=tcp-bidirectional seq=3"))
    assert ledger.accept(decode_frame(b"case=tcp-bidirectional seq=2"))
    assert ledger.accept(decode_frame(b"case=udp-bidirectional seq=1"))
    assert ledger.received["tcp-bidirectional"] == 2
    assert ledger.received["udp-bidirectional"] == 1
    assert ledger.rejects == 2

    # UDP pump under a fake clock: echo only before the deadline.
    udp.packets = [(b"case=full-recovery seq=1", ("10.0.2.15", 5000))]
    clock = FakeClock(100.0)
    serve_until_deadline(udp, tcp, PeerLedger(), 101.0, clock,
                         StaticSelect([udp]))
    assert udp.sent == [b"case=full-recovery seq=1"]

    # Deadline stop: clock already past the deadline echoes nothing.
    udp2 = FakeUdp()
    udp2.packets = [(b"case=full-recovery seq=1", ("10.0.2.15", 5000))]
    late = FakeClock(102.0)
    serve_until_deadline(udp2, tcp, PeerLedger(), 101.0, late,
                         StaticSelect([udp2]))
    assert udp2.sent == []

    # TCP pump: several frames in one segment, echo exact bytes per frame.
    # The stream fake has no __dict__ (like a real socket.socket), so the
    # service-owned keyed buffers are mandatory; a per-socket attribute would
    # raise AttributeError here.
    conn = FakeTcpConn()
    assert not hasattr(conn, "__dict__")
    conn.chunks = [b"case=tcp-bidirectional seq=1\ncase=tcp-bidirectional seq=2\n"]
    tcp2 = FakeTcpListener()
    tcp2.pending.append(conn)
    udp3 = FakeUdp()
    tcp_listener2 = _FakeAcceptor(conn)
    clock3 = FakeClock(100.0)
    serve_until_deadline(udp3, tcp_listener2, PeerLedger(), 101.0, clock3,
                         StaticSelectStream([tcp_listener2]))
    assert b"".join(conn.sent) == (b"case=tcp-bidirectional seq=1\n"
                                   b"case=tcp-bidirectional seq=2\n")

    # Split frames across segments reassemble through the service buffer.
    conn_split = FakeTcpConn()
    conn_split.chunks = [b"case=tcp-bidirectional seq=1",
                         b"\ncase=tcp-bidirectional seq=2\n"]
    tcp_split = _FakeAcceptor(conn_split)
    udp_split = FakeUdp()
    clock_split = FakeClock(100.0)
    serve_until_deadline(udp_split, tcp_split, PeerLedger(), 101.0,
                         clock_split, StaticSelectStream([tcp_split]))
    assert b"".join(conn_split.sent) == (b"case=tcp-bidirectional seq=1\n"
                                         b"case=tcp-bidirectional seq=2\n")

    # One-shot blockage plus short writes still complete inside the deadline.
    conn_blocked = FakeTcpConn()
    conn_blocked.block_next = True
    conn_blocked.chunks = [b"case=tcp-bidirectional seq=1\n"]
    tcp_blocked = _FakeAcceptor(conn_blocked)
    udp_blocked = FakeUdp()
    clock_blocked = FakeClock(100.0)
    serve_until_deadline(udp_blocked, tcp_blocked, PeerLedger(), 101.0,
                         clock_blocked, StaticSelectStream([tcp_blocked]))
    assert b"".join(conn_blocked.sent) == b"case=tcp-bidirectional seq=1\n"

    # A send that can never proceed before the deadline reports failure.
    conn_stuck = FakeTcpConn()
    conn_stuck.block_next = True
    conn_stuck.chunks = [b"case=tcp-bidirectional seq=1\n"]
    tcp_stuck = _FakeAcceptor(conn_stuck)
    udp_stuck = FakeUdp()
    clock_stuck = FakeClock(100.0)
    serve_until_deadline(udp_stuck, tcp_stuck, PeerLedger(), 101.0,
                         clock_stuck, StaticSelect([tcp_stuck]))
    assert conn_stuck.sent == []

    # Foreign/dup TCP frames are rejected and never echoed.  (Frame bytes are
    # short enough to be sent in one write here.)
    conn2 = FakeTcpConn()
    conn2.chunks = [b"case=tcp-bidirectional seq=1\ncase=unknown seq=1\n"]
    tcp_listener3 = _FakeAcceptor(conn2)
    udp4 = FakeUdp()
    ledger4 = PeerLedger()
    clock4 = FakeClock(100.0)
    serve_until_deadline(udp4, tcp_listener3, ledger4, 101.0, clock4,
                         StaticSelectStream([tcp_listener3]))
    assert b"".join(conn2.sent) == b"case=tcp-bidirectional seq=1\n"
    assert ledger4.rejects == 1


class _FakeAcceptor:
    """One-shot acceptor that hands out a pre-made fake connection."""

    def __init__(self, conn):
        self.conn = conn
        self.used = False

    def accept(self):
        if self.used:
            raise OSError("no pending connection")
        self.used = True
        return self.conn, ("127.0.0.1", 4000)


def serve(host, port, deadline_seconds):
    deadline = time.monotonic() + deadline_seconds
    ledger = PeerLedger()
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as udp_listener, \
            socket.socket(socket.AF_INET, socket.SOCK_STREAM) as tcp_listener:
        udp_listener.bind((host, port))
        udp_listener.setblocking(False)
        tcp_listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        tcp_listener.bind((host, port))
        tcp_listener.listen(8)
        tcp_listener.setblocking(False)
        serve_until_deadline(udp_listener, tcp_listener, ledger, deadline,
                             time.monotonic)
    ok = ledger.rejects == 0
    print(f"MS08_NET_PEER_RESULT: outcome={'ok' if ok else 'rejects'} "
          f"rejects={ledger.rejects} {ledger.summary()}")
    return 0 if ok else 1


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    parser.add_argument("--host", default="0.0.0.0")
    parser.add_argument("--port", type=int, default=PEER_PORT)
    parser.add_argument("--deadline-seconds", type=int, default=600)
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    if not 0 < args.port < 65536 or args.deadline_seconds <= 0:
        parser.error("port and deadline must be positive")
    return serve(args.host, args.port, args.deadline_seconds)


if __name__ == "__main__":
    sys.exit(main())
