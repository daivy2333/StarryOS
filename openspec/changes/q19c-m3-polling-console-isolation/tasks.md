## 1. Feature Boundary

- [ ] 1.1 Remove `lichee-d1-async-uart` from `lichee-d1-rootfs-probe`.
- [ ] 1.2 Add an explicit guard for `lichee-d1-rootfs-probe + lichee-d1-async-uart`.
- [ ] 1.3 Confirm M3 still selects D1 platform support and MMIO bus support through build commands.
- [ ] 1.4 Confirm M2/fullbench feature dependencies are unchanged.

## 2. M3 Entry

- [ ] 2.1 Add or refactor a D1 rootfs-probe entry that does not call async UART init.
- [ ] 2.2 Print the full existing probe table through polling console.
- [ ] 2.3 Keep `rootfs_init=NOT called` and avoid `axfs-ng::init_filesystems()`.
- [ ] 2.4 Halt after `probe complete, halting. No panic.`

## 3. Regression

- [ ] 3.1 Check M2 command-entry still compiles.
- [ ] 3.2 Build `starry-lichee-fullbench-command-boot.img`.
- [ ] 3.3 Check M3 rootfs-probe still compiles.
- [ ] 3.4 Build `starry-lichee-rootfs-probe-boot.img`.

## 4. Verification

- [ ] 4.1 `openspec validate q19c-m3-polling-console-isolation --strict`
- [ ] 4.2 Negative feature-combination check for rootfs-probe plus async UART.
- [ ] 4.3 Host cargo/image gates pass for M2 and M3.
- [ ] 4.4 D1 board gate: M3 prints the complete probe table and no rootfs panic.

## 5. Documentation

- [ ] 5.1 Update `docs/M3.md` with the new board log after testing.
- [ ] 5.2 Update `.claude/docs/tasks.md` Q19C.11c with the new status.
- [ ] 5.3 Record async UART reintroduction as a follow-up only after polling-console evidence passes.
