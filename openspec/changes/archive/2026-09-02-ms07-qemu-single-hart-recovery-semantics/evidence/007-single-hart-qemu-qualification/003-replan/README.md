# Evidence: 007-single-hart-qemu-qualification / 003-replan

- Change: ms07-qemu-single-hart-recovery-semantics
- Iteration: 007-single-hart-qemu-qualification
- Cycle: 003-replan
- Captured at: 2026-08-31
- Environment: QEMU 7.0.0 RISC-V `virt`；单 hart；1 GiB；user-net；`LOG=info`（`qemu-info-decisive.log`）；
  `LOG=warn` guest shell capture（`qemu-serial.log`）；Rust nightly-2026-02-25
- Source: user 在 single-hart QEMU 上按 R44 手工运行插桩 `tests/ms07_recovery_probe`

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-007-003-01 | plan-required | R8/A2-A6 | P7 首 case 被产品 `sys_poll` 空 nfds 缺陷阻断，非网络/owner/link/connect 问题 | [qemu-info-decisive.log](qemu-info-decisive.log) | BLOCKED |
| EV-007-003-02 | act-added | R8/P7 | 既有 guest shell 采集（21:38，尚未进新插桩，无 errno=14）；保留避免覆盖 | [qemu-serial.log](qemu-serial.log) | INFO |

## 结论

- 网络数据面健康：`eth0 ip: 10.0.2.15/24`、`mac: 52-54-00-12-34-56`、`Device: eth0`；UDP socket
  `*:49152` bound 成功（`open_peer_socket` 的 socket+bind+connect 已通）。
- 失败卡点：`wait_for_pre_reset` 第 2 次采样 `wait_until_sample()` → `poll(NULL, 0, remaining)` →
  内核 `sys_poll(null, 0, t)` 对 `nfds==0` 仍走 `check_region(NULL, ...)` → `Err(BadAddress)` →
  `EFAULT(14)`。probe 被 Makefile guard 禁止 `usleep/nanosleep/sleep(`，故用 poll 空数组作有界睡眠，
  该 POSIX 语义未被内核实现。
- 决定性行（`qemu-info-decisive.log`）：
  `DBG: wait_pre_reset iter=1 wait_until_sample_fail errno=14` → `FAIL: pre_reset_traffic
  reason=wait_for_pre_reset` → `MS07_HARNESS_EXIT: 1`。
- **推翻先前归因**：R60 runbook 与 Cycle 001/002 的 `Service.link_state=None`/OwnerSummary 不守恒/
  `open_peer_socket` connect 失败均被本证据否证——link 健康（snapshot `link=1 avail=64 dev=64`）、
  owner 全 0、socket 已建。真根因是内核 syscall 层 `sys_poll`/`sys_ppoll` 空 nfds 语义缺陷。

## 适用限制

- 只覆盖 single-hart QEMU VirtIO-MMIO 软件/设备模型；不扩大到 SMP、PCI/DWMAC、真板或性能。
- `qemu-serial.log`（21:38）为更早该 Cycle 的 guest shell 采集，未包含新插桩 `errno=14`，仅作
  保留参考；决定性判定依据 `qemu-info-decisive.log`。