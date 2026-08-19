# R44 manual QEMU runbook — MS05 Iteration 011 / Cycle 003

Prepared by Act at worktree on `2af394e6` (net-k3). This Cycle changed only the
guest probe (`tests/ms05_data_plane_probe.c` → rebuilt `tests/ms05_data_plane_probe`)
and host stimulus (`scripts/ms05_data_plane_stimulus.py`); the kernel image
`StarryOS_riscv64-qemu-virt.bin` is **unchanged**. Per Plan 6.3-R3 the
conditional-rerun branch therefore requires rerunning the six MS05 modes with the
new probe/stimulus; the WGET/compat sessions may reuse Cycle 002 evidence only as
supporting (they are the same kernel image).

## Precondition

- `sha256sum -c <frozen artifacts.sha256>` must show 6/6 OK (Cycle 003 re-freezes
  the probe at the new hash `a567ec9149a68c68515253797243c4ce9b13b60d3d45a7118c97c1630c1d5621`).
- `make host-test` in an ordinary terminal must PASS (auto gates GREEN) before QEMU.
- Guest probe must be deployed into the rootfs image **after** the rebuild:
  `tests/ms05_data_plane_probe` (new hash above) is what runs inside the guest.

## Guest probe (6 modes) — manual, one terminal per peer

Start the host stimulus FIRST (waits up to 120 s for the operator's REGISTER),
then boot QEMU and run each probe mode. The new protocol separates the long
operator-paced listen from the short exchange budget, and completes with a
DONE/ACK shared count.

Terminal H (host stimulus, per mode):

```bash
cd /home/daivy/projects/serial/work/StarryOS
python3 scripts/ms05_data_plane_stimulus.py --port 15557 | tee ms05-<mode>-host.log
```

Terminal Q (QEMU; single hart, single VirtIO-MMIO NIC):

```bash
cd /home/daivy/projects/serial/work/StarryOS
qemu-system-riscv64 -machine virt -bios default -kernel StarryOS_riscv64-qemu-virt.bin \
  -m 1G -smp 1 -device virtio-blk-device,drive=disk0 \
  -drive id=disk0,if=none,format=raw,file=make/disk.img \
  -device virtio-net-device,netdev=net0 \
  -netdev user,id=net0,hostfwd=tcp::5555-:5555,hostfwd=udp::5555-:5555 \
  -nographic
```

Inside the guest shell run each mode and capture the serial marker + exits:

```bash
./ms05_data_plane_probe snapshot
./ms05_data_plane_probe tx-only 96 64
./ms05_data_plane_probe bidirectional 96 64
./ms05_data_plane_probe slot-full
./ms05_data_plane_probe descriptor-full
./ms05_data_plane_probe flush
```

Expected (per repair):
- `descriptor-full` prints `PRE→HELD→FULL(… inflight=64 avail=0 …)→RELEASED→POST`
  and exactly one `MS05 PASS mode=descriptor-full`; on timeout it prints one
  `MS05 TIMEOUT mode=descriptor-full …` (attributable) then `MS05 FAIL`.
- Each host-assisted mode sends `MS05 ACK <mode> <count>` after DONE and reports
  PASS only after the host validates the ACK; both peers must agree on the shared count.
- Snapshot and flush must also PASS (regression).

## Exit / marker ledger

Persist `runtime-exits.txt` (each producer's exit) and `ms05-markers.txt` (the
unique `MS05 PASS|FAIL mode=…` lines) under the Cycle 003 evidence root, plus
`qemu-serial-<mode>.log` and `ms05-<mode>-host.log` for each mode.

## Final review (Task 6.3)

Re-run `make host-test`, the affected C/Rust/Python tests, specs-vs-code and full
diff review; reconcile tasks/RTM/Gates with the returned raw outputs. Do NOT
archive, refresh SNAPSHOT, or update M/D/K/R/I (Act non-goal).

## Boundary

- These results qualify only the single-hart QEMU VirtIO-MMIO software/device
  model. No SMP, DWMAC, real-board, DMA/cache or performance conclusion.
- If any required marker is missing, interrupted, late, or a protocol count is
  mismatched, that mode is NOT PASS even if other modes passed.
