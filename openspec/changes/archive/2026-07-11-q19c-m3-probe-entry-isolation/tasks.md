## 1. Entry isolation

- [x] 1.1 Prevent `lichee-d1-rootfs-probe` from requiring `bench::run_startup_benchmark()` before probe output.
- [x] 1.2 Keep M2/fullbench startup benchmark behavior unchanged.
- [x] 1.3 Keep the M3 final halt after `probe complete, halting. No panic.`

## 2. M3 evidence

- [ ] 2.1 Confirm M3 serial output includes `log_label=lichee-rootfs-probe`.
- [ ] 2.2 Confirm M3 still prints TBD/SKIPPED for SDMMC MMIO and first block read.
- [ ] 2.3 Confirm M3 does not claim rootfs benchmark success.
- [ ] 2.4 Confirm M3 does not call `axfs-ng::init_filesystems()` without a D1 block device.

## 3. Regression

- [x] 3.1 M2 command-entry cargo check still passes.
- [x] 3.2 M3 rootfs-probe cargo check passes.
- [x] 3.3 Incompatible fullbench feature combinations still fail with explicit `compile_error!`.
- [x] 3.4 `make lichee-rootfs-probe` builds the Android boot image.

## 4. Board gate

- [ ] 4.1 Burn the updated `starry-lichee-rootfs-probe-boot.img` to the D1 boot partition.
- [ ] 4.2 Capture serial output to `docs/M3.md` or a new dated board log.
- [ ] 4.3 Confirm the log contains the probe table and `probe complete, halting. No panic.`
- [ ] 4.4 Confirm the log does not contain `No block device found!` or an exception/panic.

## 5. O81 follow-up

- [x] 5.1 Keep startup benchmark truncation recorded as O81.
- [x] 5.2 Do not fix O81 inside this change unless M3 still cannot print probe evidence after entry isolation.
