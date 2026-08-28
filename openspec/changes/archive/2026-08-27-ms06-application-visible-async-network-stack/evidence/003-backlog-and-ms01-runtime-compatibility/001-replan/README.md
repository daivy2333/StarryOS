# Evidence: 003-backlog-and-ms01-runtime-compatibility / 001-replan

- Change: ms06-application-visible-async-network-stack
- Iteration: 003-backlog-and-ms01-runtime-compatibility
- Cycle: 001-replan
- Captured at: 2026-08-26（用户在 guest shell 手动执行并回贴串口输出）
- Revision: `4396d264` (net-k3)，工作树含 Cycle 001-replan 未提交实现与本 Cycle OpenSpec 记录
- Environment: single-hart RISC-V `virt` QEMU，VirtIO-MMIO user-net（hostfwd 5555），内核镜像
  `StarryOS_riscv64-qemu-virt.bin` 构建于 2026-08-26 12:16；payloads `ms01_loopback_diagnostic`、
  `ms01_socket_baseline` 为 2026-08-26 新鲜静态构建；流程遵循 `.claude/runbooks/qemu-network-testing.md`
  手工政策（HTTP server `--bind 0.0.0.0`:18765）

| ID | Origin | Acceptance | Claim | Artifact | Result |
|---|---|---|---|---|---|
| EV-003-001-replan-01 | user-required | A6 / S5（R7） | fresh 单 hart QEMU：diagnostic `single` 与 `fork` 均 PASS（START/END 齐全）；MS01 一个 START、14 unique PASS（含 `tcp-adjacent`）、零 FAIL、一个 END、显式进程退出码 `MS01_EXIT:0` | [ms06-qemu-runtime-markers.md](ms06-qemu-runtime-markers.md) | PASS |

## 白名单与必要性说明

- 用户明确要求保存本次运行证据（"你创建证据文件保存这个证据"），满足白名单第 1 条。
- Act Response 仅保留 ≤20 行决定性摘录；本文件补足 Reviewer 要求的完整 marker 序列与显式退出码，
  不复制长日志之外的源码或测试输出。
- 适用限制：结论限定于单 hart QEMU VirtIO-MMIO 软件设备模型；不支持 SMP、真板、DMA/cache 或性能结论。

## 文件

- `README.md`：本文件。
- `ms06-qemu-runtime-markers.md`：三次运行的决定性 marker 序列（diagnostic single/fork + MS01 含退出码），
  共 ~100 行，符合单文件 ≤500 行 / ≤256 KiB 预算。本 Cycle 目录共 2 个文件（≤5）。
