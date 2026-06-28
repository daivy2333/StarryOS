#!/usr/bin/env python3
"""Android boot image pack/inspect tool for D1 board (Sipeed Lichee RV Dock).

Header struct (little-endian, legacy Android boot image format v0):
  Offset  Size  Field
  0       8     magic ("ANDROID!")
  8       4     kernel_size
  12      4     kernel_addr
  16      4     ramdisk_size
  20      4     ramdisk_addr
  24      4     second_size
  28      4     second_addr
  32      4     tags_addr
  36      4     page_size
  40      4     header_version (0)
  44      4     os_version (0)
  48      16    name (null-padded)
  64      512   cmdline (null-padded)
  576     32    id (zeros)
  608     16    extra_cmdline (null-padded)
  ...padding to page_size (2048 bytes total header)
"""

import argparse
import struct
import sys

MAGIC = b"ANDROID!"
HEADER_FMT = "<8sIIIIIIIIII16s512s32s16s"
HEADER_SIZE = struct.calcsize(HEADER_FMT)  # 624 bytes
D1_PAGE_SIZE = 2048
D1_KERNEL_ADDR = 0x40200000
D1_RAMDISK_ADDR = 0x41200000
D1_SECOND_ADDR = 0x41100000
D1_TAGS_ADDR = 0x40200100
D1_NAME = "d1-nezha"


def align_up(value: int, alignment: int) -> int:
    """Round value up to next multiple of alignment."""
    return (value + alignment - 1) & ~(alignment - 1)


def parse_header(data: bytes) -> dict:
    """Parse Android boot image header from raw bytes."""
    if len(data) < HEADER_SIZE:
        raise ValueError(f"Header too short: {len(data)} < {HEADER_SIZE}")

    fields = struct.unpack_from(HEADER_FMT, data, 0)
    return {
        "magic": fields[0],
        "kernel_size": fields[1],
        "kernel_addr": fields[2],
        "ramdisk_size": fields[3],
        "ramdisk_addr": fields[4],
        "second_size": fields[5],
        "second_addr": fields[6],
        "tags_addr": fields[7],
        "page_size": fields[8],
        "header_version": fields[9],
        "os_version": fields[10],
        "name": fields[11].rstrip(b"\x00").decode("ascii", errors="replace"),
        "cmdline": fields[12].rstrip(b"\x00").decode("ascii", errors="replace"),
        "id": fields[13],
        "extra_cmdline": fields[14].rstrip(b"\x00").decode("ascii", errors="replace"),
    }


def build_header(
    kernel_size: int,
    kernel_addr: int,
    ramdisk_size: int,
    ramdisk_addr: int,
    second_size: int,
    second_addr: int,
    tags_addr: int,
    page_size: int,
    name: str,
    cmdline: str = "",
) -> bytes:
    """Build Android boot image header bytes."""
    name_bytes = name.encode("ascii")[:15].ljust(16, b"\x00")
    cmdline_bytes = cmdline.encode("ascii")[:511].ljust(512, b"\x00")
    id_bytes = b"\x00" * 32
    extra_bytes = b"\x00" * 16

    header = struct.pack(
        HEADER_FMT,
        MAGIC,
        kernel_size,
        kernel_addr,
        ramdisk_size,
        ramdisk_addr,
        second_size,
        second_addr,
        tags_addr,
        page_size,
        0,  # header_version
        0,  # os_version
        name_bytes,
        cmdline_bytes,
        id_bytes,
        extra_bytes,
    )
    # Pad header to page_size boundary
    return header.ljust(page_size, b"\x00")


def cmd_inspect(args: argparse.Namespace) -> int:
    """Inspect an Android boot image."""
    with open(args.image, "rb") as f:
        header_data = f.read(D1_PAGE_SIZE)

    try:
        hdr = parse_header(header_data)
    except ValueError as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 1

    if hdr["magic"] != MAGIC:
        print(f"ERROR: bad magic: {hdr['magic']!r} (expected {MAGIC!r})", file=sys.stderr)
        return 1

    print(f"magic:        {hdr['magic'].decode()}")
    print(f"kernel_size:  {hdr['kernel_size']} (0x{hdr['kernel_size']:x})")
    print(f"kernel_addr:  0x{hdr['kernel_addr']:08x}")
    print(f"ramdisk_size: {hdr['ramdisk_size']} (0x{hdr['ramdisk_size']:x})")
    print(f"ramdisk_addr: 0x{hdr['ramdisk_addr']:08x}")
    print(f"second_size:  {hdr['second_size']} (0x{hdr['second_size']:x})")
    print(f"second_addr:  0x{hdr['second_addr']:08x}")
    print(f"tags_addr:    0x{hdr['tags_addr']:08x}")
    print(f"page_size:    {hdr['page_size']}")
    print(f"name:         {hdr['name']}")
    print(f"cmdline:      {hdr['cmdline']}")

    # D1 contract validation
    errors = []
    if hdr["page_size"] != D1_PAGE_SIZE:
        errors.append(f"page_size={hdr['page_size']} (expected {D1_PAGE_SIZE})")
    if hdr["kernel_addr"] != D1_KERNEL_ADDR:
        errors.append(
            f"kernel_addr=0x{hdr['kernel_addr']:08x} (expected 0x{D1_KERNEL_ADDR:08x})"
        )

    if errors:
        for err in errors:
            print(f"ERROR: D1 contract violation: {err}", file=sys.stderr)
        return 1

    return 0


def cmd_pack(args: argparse.Namespace) -> int:
    """Pack an Android boot image."""
    with open(args.kernel, "rb") as f:
        kernel_data = f.read()

    kernel_size = len(kernel_data)
    ramdisk_size = 12
    ramdisk_data = b"\x00" * ramdisk_size
    second_size = 0
    second_data = b""

    header = build_header(
        kernel_size=kernel_size,
        kernel_addr=args.kernel_addr,
        ramdisk_size=ramdisk_size,
        ramdisk_addr=args.ramdisk_addr,
        second_size=second_size,
        second_addr=args.second_addr,
        tags_addr=args.tags_addr,
        page_size=args.page_size,
        name=args.name,
    )

    # Layout: [header page] [kernel aligned to page] [ramdisk aligned to page] [second aligned to page]
    kernel_offset = align_up(len(header), args.page_size)
    ramdisk_offset = kernel_offset + align_up(kernel_size, args.page_size)
    total_size = ramdisk_offset + align_up(ramdisk_size, args.page_size)

    image = bytearray(total_size)
    image[: len(header)] = header
    image[kernel_offset : kernel_offset + kernel_size] = kernel_data
    image[ramdisk_offset : ramdisk_offset + ramdisk_size] = ramdisk_data

    with open(args.output, "wb") as f:
        f.write(image)

    print(f"Packed: {args.output}")
    print(f"  kernel:  {kernel_size} bytes @ offset 0x{kernel_offset:x}")
    print(f"  ramdisk: {ramdisk_size} bytes @ offset 0x{ramdisk_offset:x}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(
        description="Android boot image pack/inspect tool for D1 board"
    )
    sub = parser.add_subparsers(dest="command")

    p_inspect = sub.add_parser("inspect", help="Inspect an Android boot image")
    p_inspect.add_argument("image", help="Path to boot image")

    p_pack = sub.add_parser("pack", help="Pack an Android boot image")
    p_pack.add_argument("--kernel", required=True, help="Path to kernel binary")
    p_pack.add_argument("--output", required=True, help="Output image path")
    p_pack.add_argument("--page-size", type=int, default=D1_PAGE_SIZE, help="Page size (default: 2048)")
    p_pack.add_argument("--kernel-addr", type=lambda x: int(x, 0), default=D1_KERNEL_ADDR, help="Kernel load address")
    p_pack.add_argument("--ramdisk-addr", type=lambda x: int(x, 0), default=D1_RAMDISK_ADDR, help="Ramdisk load address")
    p_pack.add_argument("--second-addr", type=lambda x: int(x, 0), default=D1_SECOND_ADDR, help="Second stage load address")
    p_pack.add_argument("--tags-addr", type=lambda x: int(x, 0), default=D1_TAGS_ADDR, help="Kernel tags address")
    p_pack.add_argument("--name", default=D1_NAME, help="Image name (default: d1-nezha)")

    args = parser.parse_args()
    if args.command is None:
        parser.print_help()
        return 1

    if args.command == "inspect":
        return cmd_inspect(args)
    elif args.command == "pack":
        return cmd_pack(args)
    return 1


if __name__ == "__main__":
    sys.exit(main())
