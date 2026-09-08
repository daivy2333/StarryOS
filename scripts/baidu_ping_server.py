#!/usr/bin/env python3
"""Host-side Baidu reachability probe server for the QEMU guest.

Accepts a single TCP connection from the guest, then on the host performs a
real ICMP ping (and an HTTP check) to https://www.baidu.com, and returns a
single result line to the guest.

Intended topology:
  guest --TCP 10.0.2.2:15561--> host(baidu_ping_server) --ICMP/HTTP--> baidu

This is an ad-hoc demonstration, NOT part of any OpenSpec where change
Acceptance. QEMU user-net (SLIRP) does not forward the guest's own ICMP to
the internet, so the host performs the actual probe on the guest's behalf and
reports reachability + round-trip time back over TCP.

Usage:
  python3 scripts/baidu_ping_server.py [--host HOST] [--port PORT] [--target URL]
"""

from __future__ import annotations

import argparse
import re
import socket
import subprocess
import sys
import time
import urllib.request

DEFAULT_HOST = "0.0.0.0"
DEFAULT_PORT = 15561
DEFAULT_TARGET = "https://www.baidu.com"
PING_TARGET = "www.baidu.com"

# Result markers printed by the guest for easy judge on serial.
RESULT_OK = "BAIDU_PROBE_OK http={http} ping_rtt_ms={ping_rtt} total_ms={total}"
RESULT_ICMP_FAIL = "BAIDU_PROBE_ICMP_FAIL http={http} total_ms={total}"
RESULT_HTTP_FAIL = "BAIDU_PROBE_HTTP_FAIL ping_rtt_ms={ping_rtt} total_ms={total}"
RESULT_UNREACHABLE = "BAIDU_PROBE_UNREACHABLE total_ms={total}"


def parse_ping_rtt(ping_out: str) -> float | None:
    """Extract the round-trip time in ms from a `ping` run.

    Matches the second field of the `time=` marker present in both modern and
    busybox `ping` output. Returns None when no valid RTT is found.
    """
    match = re.search(r"time=([0-9.]+)\s*ms", ping_out)
    if match is None:
        return None
    try:
        return float(match.group(1))
    except ValueError:
        return None


def icmp_ping_rtt(timeout: float) -> float | None:
    """Run a single host ICMP ping to PING_TARGET and return RTT in ms."""
    cmd = ["ping", "-c", "1", "-W", "1", PING_TARGET]
    try:
        completed = subprocess.run(
            cmd,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
        )
    except (subprocess.TimeoutExpired, OSError):
        return None
    return parse_ping_rtt(completed.stdout)


def http_probe(url: str, timeout: float) -> tuple[bool, float]:
    """Perform an HTTP GET to `url`, returning (ok, elapsed_ms)."""
    start = time.monotonic()
    try:
        with urllib.request.urlopen(url, timeout=timeout) as response:
            elapsed = (time.monotonic() - start) * 1000.0
            return response.status == 200, elapsed
    except OSError:
        elapsed = (time.monotonic() - start) * 1000.0
        return False, elapsed


def run_ping_probe(target: str, timeout: float) -> tuple[float | None, float | None]:
    """Probe the host->baidu path. Returns (http_status_or_None, rtt_ms).

    The ICMP RTT is taken as the primary "ping" latency; HTTP is a secondary
    confirmation that the host really reached the site over TCP.
    """
    ping_rtt = icmp_ping_rtt(timeout)
    http_ok, _http_ms = http_probe(target, timeout)
    return http_ok, ping_rtt


def format_result(http_ok, ping_rtt, total_ms):
    if http_ok and ping_rtt is not None:
        return RESULT_OK.format(http=200, ping_rtt=f"{ping_rtt:.1f}",
                                total=f"{total_ms:.1f}")
    if http_ok and ping_rtt is None:
        return RESULT_ICMP_FAIL.format(http=200, total=f"{total_ms:.1f}")
    if ping_rtt is not None:
        return RESULT_HTTP_FAIL.format(ping_rtt=f"{ping_rtt:.1f}",
                                       total=f"{total_ms:.1f}")
    return RESULT_UNREACHABLE.format(total=f"{total_ms:.1f}")


def serve_once(listener: socket.socket, target: str, timeout: float) -> None:
    """Accept one guest connection, run the probe, and send back one line.

    The guest sends a single request byte ("\\n") after connecting. That byte
    must be drained before close(): a TCP close with process-unread data in
    the receive buffer sends RST instead of FIN, which races the guest's recv
    and intermittently surfaces as ECONNRESET (errno 107). After the result is
    written we half-close the write side so the reply is relayed with a clean
    FIN too.
    """
    conn, _addr = listener.accept()
    start = time.monotonic()
    with conn:
        try:
            conn.recv(64)  # drain the guest's request byte (may be empty)
        except OSError:
            pass
        http_ok, ping_rtt = run_ping_probe(target, timeout)
        total_ms = (time.monotonic() - start) * 1000.0
        result = format_result(http_ok, ping_rtt, total_ms)
        # Uses %s to force a single textual line; send() with text on a socket
        # requires bytes, so encode.
        conn.sendall((result + "\n").encode("ascii"))
        try:
            conn.shutdown(socket.SHUT_WR)  # clean FIN after the reply
        except OSError:
            pass


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    parser.add_argument("--target", default=DEFAULT_TARGET)
    parser.add_argument("--timeout", type=float, default=8.0)
    args = parser.parse_args()

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.host, args.port))
        listener.listen(1)
        print(f"BAIDU_PING_SERVER_LISTENING {args.host}:{args.port} target={args.target}",
              flush=True)
        while True:
            try:
                serve_once(listener, args.target, args.timeout)
            except KeyboardInterrupt:
                return 0


if __name__ == "__main__":
    sys.exit(main())