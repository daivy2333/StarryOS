#!/bin/bash
# ms01-prepare.sh — compile MS01 payload and print manual test commands
set -e
cd "$(dirname "$0")/.."

echo "=== MS01 Payload Ready ==="
riscv64-linux-musl-gcc -static -O2 -o tests/ms01_socket_baseline tests/ms01_socket_baseline.c
echo "  Binary: tests/ms01_socket_baseline"
echo "  SHA256: $(sha256sum tests/ms01_socket_baseline | cut -d' ' -f1)"
echo "  Size:   $(wc -c < tests/ms01_socket_baseline) bytes"
echo ""

echo "=== How to test ==="
echo ""
echo "# Terminal 1 — serve the binary:"
echo "  cd tests && python3 -m http.server 18765 --bind 0.0.0.0"
echo ""
echo "# Terminal 2 — start QEMU:"
echo "  qemu-system-riscv64 \\"
echo "    -machine virt -bios default \\"
echo "    -kernel StarryOS_riscv64-qemu-virt.bin \\"
echo "    -m 1G -smp 1 \\"
echo "    -device virtio-blk-device,drive=disk0 \\"
echo "    -drive id=disk0,if=none,format=raw,file=make/disk.img \\"
echo "    -device virtio-net-device,netdev=net0 \\"
echo "    -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \\"
echo "    -nographic"
echo ""
echo "# In QEMU guest (starry:~#):"
echo "  wget -q -O /tmp/ms01_test http://10.0.2.2:18765/ms01_socket_baseline"
echo "  chmod +x /tmp/ms01_test"
echo "  /tmp/ms01_test"
echo ""
echo "# Expect: 9 PASS markers between MS01_SOCKET_BASELINE_START and _END"
