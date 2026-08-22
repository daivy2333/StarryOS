#!/usr/bin/env python3
"""Demo host chat server for the linshi weekly demonstration.

Listens on 0.0.0.0:15560, accepts a single TCP connection from the QEMU
guest, then relays lines between the local terminal (stdin/stdout) and the
connection using select(). Control command: "/quit" closes the session; a
peer EOF prints DEMO_CHAT_SERVER_PEER_CLOSED and returns to accept().

Line classification and the EOF/QUIT decisions are pure functions so the
paired test file can exercise them without network I/O.

Usage:
  python3 scripts/demo_chat_server.py [--host HOST] [--port PORT]
"""

from __future__ import annotations

import argparse
import os
import select
import socket
import sys

DEFAULT_HOST = "0.0.0.0"
DEFAULT_PORT = 15560

# Line classification results.
LINE_EMPTY = "EMPTY"
LINE_QUIT = "QUIT"
LINE_TEXT = "TEXT"

# Local-input actions.
ACTION_SKIP = "SKIP"
ACTION_SEND = "SEND"
ACTION_QUIT = "QUIT"

# Remote-EOF result.
RESULT_PEER_CLOSED = "PEER_CLOSED"

MARK_CONNECTED = "DEMO_CHAT_SERVER_CONNECTED peer={peer}"
MARK_PEER_CLOSED = "DEMO_CHAT_SERVER_PEER_CLOSED"

CHAT_LINE_END = "\n"
CHAT_BUFFER_SIZE = 4096


def classify_line(line: str) -> str:
    """Classify one input line (newline stripped): EMPTY / QUIT / TEXT."""
    if not line.strip():
        return LINE_EMPTY
    if line.strip() == "/quit":
        return LINE_QUIT
    return LINE_TEXT


def handle_local_input(line: str) -> tuple[str, str | None]:
    """Decide what to do with one locally entered line.

    Returns (action, payload): ACTION_SKIP drops the line, ACTION_QUIT closes
    the session, ACTION_SEND forwards `payload` (line + newline).
    """
    kind = classify_line(line)
    if kind == LINE_EMPTY:
        return ACTION_SKIP, None
    if kind == LINE_QUIT:
        return ACTION_QUIT, None
    return ACTION_SEND, line + CHAT_LINE_END


def handle_remote_line(line: str) -> str | None:
    """Decide what to display for one received remote line.

    Empty lines are dropped; everything else (including a remote "/quit"
    arriving as chat text) is printed as-is.
    """
    kind = classify_line(line)
    if kind == LINE_EMPTY:
        return None
    return line


def handle_remote_eof() -> str:
    """Result of a remote EOF: the peer closed the connection."""
    return RESULT_PEER_CLOSED


def drain_local(fd) -> str:
    """Read available bytes from a local terminal fd, decoded as text."""
    raw = os.read(fd, CHAT_BUFFER_SIZE)
    return raw.decode("utf-8", errors="replace")


def run_session(conn: socket.socket) -> None:
    """Relay lines between stdin/stdout and `conn` until quit or EOF."""
    local_fd = sys.stdin.fileno()
    local_buf = ""
    remote_buf = ""
    conn.setblocking(False)

    while True:
        readable, _, _ = select.select([local_fd, conn], [], [])
        if local_fd in readable:
            try:
                local_buf += drain_local(local_fd)
            except (OSError, ValueError):
                return  # local terminal closed
            while CHAT_LINE_END in local_buf:
                line, _, local_buf = local_buf.partition(CHAT_LINE_END)
                line = line.rstrip("\r")
                action, payload = handle_local_input(line)
                if action == ACTION_QUIT:
                    conn.close()
                    return
                if action == ACTION_SEND:
                    assert payload is not None
                    conn.sendall(payload.encode("utf-8"))

        if conn in readable:
            try:
                data = conn.recv(CHAT_BUFFER_SIZE)
            except (BlockingIOError, InterruptedError):
                continue
            except OSError:
                print(MARK_PEER_CLOSED)
                sys.stdout.flush()
                conn.close()
                return
            if not data:
                print(MARK_PEER_CLOSED)
                sys.stdout.flush()
                conn.close()
                return
            remote_buf += data.decode("utf-8", errors="replace")
            while CHAT_LINE_END in remote_buf:
                line, _, remote_buf = remote_buf.partition(CHAT_LINE_END)
                line = line.rstrip("\r")
                display = handle_remote_line(line)
                if display is not None:
                    print(display)
                    sys.stdout.flush()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--host", default=DEFAULT_HOST)
    parser.add_argument("--port", type=int, default=DEFAULT_PORT)
    args = parser.parse_args()

    if not 1 <= args.port <= 65535:
        parser.error("port must be in 1..65535")

    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as listener:
        listener.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        listener.bind((args.host, args.port))
        listener.listen(1)
        try:
            while True:
                conn, peer = listener.accept()
                with conn:
                    print(MARK_CONNECTED.format(
                        peer=f"{peer[0]}:{peer[1]}"))
                    sys.stdout.flush()
                    run_session(conn)
        except KeyboardInterrupt:
            return 0


if __name__ == "__main__":
    raise SystemExit(main())