#!/usr/bin/env python3
"""Build and run the strict MS01 socket witness under QEMU."""

from __future__ import annotations

import argparse
import contextlib
import http.server
import os
from pathlib import Path
import socket
import subprocess
import sys
import threading
import time


ROOT = Path(__file__).resolve().parents[1]
PAYLOAD_SOURCE = ROOT / "tests/ms01_socket_baseline.c"
PAYLOAD = ROOT / "tests/ms01_socket_baseline"
KERNEL = ROOT / "StarryOS_riscv64-qemu-virt.bin"
DISK = ROOT / "make/disk.img"
START = "MS01_SOCKET_BASELINE_START"
END = "MS01_SOCKET_BASELINE_END"
EXIT_PREFIX = "MS01_HARNESS_EXIT:"
EXPECTED = {
    "tcp-accept",
    "tcp-adjacent",
    "tcp-512cap",
    "tcp-512-recovery",
    "tcp-relisten",
    "udp-bidi",
    "tcp-nonblock-accept",
    "udp-nonblock",
    "poll-readiness",
    "udp-source",
    "bind-getsockname",
    "bind-ephemeral",
    "bind-conflict",
    "bind-close-cleanup",
}


class QuietHandler(http.server.SimpleHTTPRequestHandler):
    def log_message(self, _format: str, *_args: object) -> None:
        return


def validate_output(output: str, *, timed_out: bool = False, qemu_exit: int | None = None) -> None:
    if timed_out:
        raise RuntimeError("QEMU or payload timed out")
    if qemu_exit is not None:
        raise RuntimeError(f"QEMU exited before the witness completed: {qemu_exit}")
    if output.count(START) != 1 or output.count(END) != 1:
        raise RuntimeError("start/end markers are missing or duplicated")

    body = output.split(START, 1)[1].split(END, 1)[0]
    if "FAIL:" in body:
        raise RuntimeError("payload reported a failure")
    for marker in EXPECTED:
        if body.count(f"PASS: {marker}") != 1:
            raise RuntimeError(f"PASS marker {marker!r} is missing or duplicated")

    exits = [
        line.removeprefix(EXIT_PREFIX).strip()
        for line in output.splitlines()
        if line.startswith(EXIT_PREFIX)
    ]
    if exits != ["0"]:
        raise RuntimeError(f"unexpected payload exit markers: {exits!r}")


def self_test() -> None:
    markers = "\n".join(f"PASS: {marker}" for marker in sorted(EXPECTED))
    good = f"{START}\n{markers}\n{END}\n{EXIT_PREFIX}0\n"
    validate_output(good)

    failures = [
        good.replace("PASS: tcp-accept\n", ""),
        good.replace("PASS: tcp-accept\n", "PASS: tcp-accept\nPASS: tcp-accept\n"),
        good.replace(f"{EXIT_PREFIX}0", f"{EXIT_PREFIX}1"),
    ]
    for sample in failures:
        try:
            validate_output(sample)
        except RuntimeError:
            pass
        else:
            raise AssertionError("invalid synthetic output was accepted")

    for kwargs in ({"timed_out": True}, {"qemu_exit": 1}):
        try:
            validate_output(good, **kwargs)
        except RuntimeError:
            pass
        else:
            raise AssertionError(f"invalid state was accepted: {kwargs}")
    print("PASS: harness-self-test")


def free_tcp_port() -> int:
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return int(sock.getsockname()[1])


def wait_for_serial(port: int, proc: subprocess.Popen[bytes], deadline: float) -> socket.socket:
    while time.monotonic() < deadline:
        code = proc.poll()
        if code is not None:
            raise RuntimeError(f"QEMU exited before serial became ready: {code}")
        try:
            serial = socket.create_connection(("127.0.0.1", port), timeout=1)
            serial.settimeout(1)
            return serial
        except OSError:
            time.sleep(0.1)
    raise TimeoutError("serial connection timeout")


def read_until(serial: socket.socket, proc: subprocess.Popen[bytes], needle: bytes, deadline: float) -> bytes:
    data = bytearray()
    while needle not in data:
        code = proc.poll()
        if code is not None:
            raise RuntimeError(f"QEMU exited while waiting for {needle!r}: {code}")
        if time.monotonic() >= deadline:
            raise TimeoutError(f"timeout waiting for {needle!r}")
        try:
            chunk = serial.recv(4096)
        except socket.timeout:
            continue
        if not chunk:
            raise RuntimeError("serial connection closed")
        data.extend(chunk)
    return bytes(data)


def run() -> str:
    subprocess.run(
        [
            os.environ.get("MS01_CC", "riscv64-linux-gnu-gcc"),
            "-static",
            "-O2",
            "-o",
            str(PAYLOAD),
            str(PAYLOAD_SOURCE),
        ],
        check=True,
        cwd=ROOT,
    )
    for path in (KERNEL, DISK, PAYLOAD):
        if not path.is_file():
            raise FileNotFoundError(path)

    handler = lambda *args, **kwargs: QuietHandler(  # noqa: E731
        *args, directory=str(PAYLOAD.parent), **kwargs
    )
    httpd = http.server.ThreadingHTTPServer(("0.0.0.0", 0), handler)
    http_port = int(httpd.server_address[1])
    http_thread = threading.Thread(target=httpd.serve_forever, daemon=True)
    http_thread.start()

    serial_port = free_tcp_port()
    qemu = [
        "qemu-system-riscv64",
        "-machine",
        "virt",
        "-bios",
        "default",
        "-kernel",
        str(KERNEL),
        "-m",
        "1G",
        "-smp",
        "1",
        "-device",
        "virtio-blk-device,drive=disk0",
        "-drive",
        f"id=disk0,if=none,format=raw,file={DISK}",
        "-device",
        "virtio-net-device,netdev=net0",
        "-netdev",
        "user,id=net0",
        "-display",
        "none",
        "-monitor",
        "none",
        "-serial",
        f"tcp:127.0.0.1:{serial_port},server=on,wait=off",
    ]
    proc = subprocess.Popen(qemu, cwd=ROOT, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    serial: socket.socket | None = None
    output = b""
    try:
        serial = wait_for_serial(serial_port, proc, time.monotonic() + 30)
        output += read_until(serial, proc, b"starry:~#", time.monotonic() + 120)
        command = (
            f"wget -q -O /tmp/ms01_test http://10.0.2.2:{http_port}/{PAYLOAD.name}"
            " && chmod +x /tmp/ms01_test && /tmp/ms01_test"
            f"; echo {EXIT_PREFIX}$?\n"
        )
        serial.sendall(command.encode())
        output += read_until(serial, proc, EXIT_PREFIX.encode(), time.monotonic() + 360)
        output += read_until(serial, proc, b"\n", time.monotonic() + 10)
        text = output.decode(errors="replace").replace("\r", "")
        validate_output(text, qemu_exit=proc.poll())
        return text
    finally:
        if serial is not None:
            serial.close()
        httpd.shutdown()
        httpd.server_close()
        proc.terminate()
        with contextlib.suppress(subprocess.TimeoutExpired):
            proc.wait(timeout=5)
        if proc.poll() is None:
            proc.kill()
            proc.wait(timeout=5)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    try:
        if args.self_test:
            self_test()
        else:
            sys.stdout.write(run())
    except Exception as error:
        print(f"FAIL: harness: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
