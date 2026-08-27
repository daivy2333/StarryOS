# Evidence: single-hart-qemu-acceptance / 000-initial

- Change: ms06-application-visible-async-network-stack
- Iteration: 008-single-hart-qemu-acceptance
- Cycle: 000-initial
- Captured at: 2026-08-27（用户手工 single-hart QEMU 会话；结果已补录）
- Revision: repo HEAD `1d0313ad8d0f36d918d1a101dd0ceda5c2ba336b`（net-k3）；
  frozen payload 内嵌 revision `832abfead57e7ae0870d5b729b6875665d588582`
- Environment: 单 hart、单 VirtIO-MMIO NIC QEMU RISC-V `virt`；1 GiB；user-net；
  QEMU 7.0.0；util-linux `script` 2.37.2；python3 3.10.12；R44 手工输入政策；
  R58 `script`/`tee` 采集模式

## 冻结 artifact 身份（启动前记录，本 Cycle 不重建）

| Artifact | bytes | mtime (2026-08-27 CST) | 类型 |
|---|---|---|---|
| `StarryOS_riscv64-qemu-virt.bin` | 40,763,584 | 20:08:19 | boot kernel |
| `make/disk.img` | 1,073,741,824 | 2026-08-26 | virtio-blk rootfs |
| `tests/ms01_socket_baseline` | 155,272 | 20:08:30 | RISC-V static-pie ELF |
| `tests/ms04_rx_probe` | 134,232 | 20:08:30 | RISC-V static non-PIE ELF |
| `tests/ms05_data_plane_probe` | 149,528 | 20:08:31 | RISC-V static non-PIE ELF |
| `tests/ms06_stack_readiness_probe` | 147,024 | 20:08:31 | RISC-V static non-PIE ELF，内嵌 revision/env |

## 证据文件映射

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-008-000-01 | plan-required | A1-A4 | MS06 串口 10/12 PASS + 2 FAIL（listener backlog、close-error）；MS01 14/14；MS04 四 mode、MS05 六 mode 终态 marker 与显式 exit 全闭合 | [qemu-runtime-markers.md](qemu-runtime-markers.md) | **FAIL（MS06 Task 7.1 GREEN 未达成）** |
| EV-008-000-02 | plan-required | A3-A4 | MS04 host 15556 与 MS05 六 mode host 15557 stimulus 双边闭合（received=96 全部对齐） | [host-runtime-results.md](host-runtime-results.md) | PASS |

- 完整 `script` 串口作为一次性人工输入外部保存：`/tmp/ms06-iteration-008-qemu-serial.log`
  （首启 session；若因其超过 500 行/256 KiB 不入库，qemu-runtime-markers.md 承担其
  决定性项并按原顺序摘录）。
- 会话后 artifact 核对：probe 四件 size/mtime 未变；boot bin bytes 未变但 mtime
  更新为 21:13:42（用户经 `make run` 启动触发了重建；R44/R58 采集命令已同步改为
  `make run`）——exact-binary 声明项仅 boot bin 受影响。
- 结论严格限定 single-hart QEMU VirtIO-MMIO 软件/设备模型；不外推 reset、SMP、
  PCI/DWMAC、真板、DMA/cache 或性能。
- 预算：本 Cycle 3 文件（含 README），≤5；整个 change 预计 8/20。