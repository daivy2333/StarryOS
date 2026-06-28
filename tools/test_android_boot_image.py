#!/usr/bin/env python3
"""Deterministic tests for android_boot_image.py."""

import subprocess
import sys
import tempfile
from pathlib import Path

TOOL = str(Path(__file__).parent / "android_boot_image.py")
FAKE_KERNEL = b"FAKE_KERNEL"


def run_tool(*args: str) -> subprocess.CompletedProcess:
    return subprocess.run(
        [sys.executable, TOOL, *args],
        capture_output=True,
        text=True,
    )


def test_pack_and_inspect_roundtrip(tmp_path: Path) -> None:
    kernel = tmp_path / "kernel.bin"
    kernel.write_bytes(FAKE_KERNEL)
    output = tmp_path / "boot.img"

    r = run_tool(
        "pack",
        "--kernel", str(kernel),
        "--output", str(output),
        "--page-size", "2048",
        "--kernel-addr", "0x40200000",
        "--ramdisk-addr", "0x41200000",
        "--second-addr", "0x41100000",
        "--tags-addr", "0x40200100",
        "--name", "d1-nezha",
    )
    assert r.returncode == 0, f"pack failed: {r.stderr}"

    r = run_tool("inspect", str(output))
    assert r.returncode == 0, f"inspect failed: {r.stderr}"
    assert "kernel_size:  11" in r.stdout
    assert "kernel_addr:  0x40200000" in r.stdout
    assert "ramdisk_size: 12" in r.stdout
    assert "ramdisk_addr: 0x41200000" in r.stdout
    assert "second_addr:  0x41100000" in r.stdout
    assert "tags_addr:    0x40200100" in r.stdout
    assert "page_size:    2048" in r.stdout
    assert "name:         d1-nezha" in r.stdout


def test_inspect_rejects_bad_magic(tmp_path: Path) -> None:
    bad = tmp_path / "bad.img"
    bad.write_bytes(b"BADMGIC!" + b"\x00" * 2040)

    r = run_tool("inspect", str(bad))
    assert r.returncode != 0
    assert "bad magic" in r.stderr


def test_page_size_enforced(tmp_path: Path) -> None:
    kernel = tmp_path / "kernel.bin"
    kernel.write_bytes(FAKE_KERNEL)
    output = tmp_path / "boot.img"

    r = run_tool(
        "pack",
        "--kernel", str(kernel),
        "--output", str(output),
        "--page-size", "4096",
        "--kernel-addr", "0x40200000",
        "--ramdisk-addr", "0x41200000",
        "--second-addr", "0x41100000",
        "--tags-addr", "0x40200100",
        "--name", "d1-nezha",
    )
    assert r.returncode == 0, f"pack failed: {r.stderr}"

    r = run_tool("inspect", str(output))
    assert r.returncode != 0
    assert "page_size" in r.stderr


def test_kernel_addr_enforced(tmp_path: Path) -> None:
    kernel = tmp_path / "kernel.bin"
    kernel.write_bytes(FAKE_KERNEL)
    output = tmp_path / "boot.img"

    r = run_tool(
        "pack",
        "--kernel", str(kernel),
        "--output", str(output),
        "--page-size", "2048",
        "--kernel-addr", "0x50000000",
        "--ramdisk-addr", "0x41200000",
        "--second-addr", "0x41100000",
        "--tags-addr", "0x40200100",
        "--name", "d1-nezha",
    )
    assert r.returncode == 0, f"pack failed: {r.stderr}"

    r = run_tool("inspect", str(output))
    assert r.returncode != 0
    assert "kernel_addr" in r.stderr


def test_deterministic_output(tmp_path: Path) -> None:
    kernel = tmp_path / "kernel.bin"
    kernel.write_bytes(FAKE_KERNEL)
    out1 = tmp_path / "boot1.img"
    out2 = tmp_path / "boot2.img"

    for out in (out1, out2):
        r = run_tool(
            "pack",
            "--kernel", str(kernel),
            "--output", str(out),
            "--page-size", "2048",
            "--kernel-addr", "0x40200000",
            "--ramdisk-addr", "0x41200000",
            "--second-addr", "0x41100000",
            "--tags-addr", "0x40200100",
            "--name", "d1-nezha",
        )
        assert r.returncode == 0, f"pack failed: {r.stderr}"

    assert out1.read_bytes() == out2.read_bytes()

