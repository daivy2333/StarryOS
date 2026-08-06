# ms16-qemu-polling-network-performance-baseline

MS16 QEMU VirtIO-MMIO polling NIC benchmark protocol, tooling, calibration, and B0 evidence

## Closure

2026-08-06，用户确认本 change 以测试矩阵设计、portable workload、user-net 六方向执行资格和可重复 Runbook 为完成边界。TAP standard B0、完整性能运行和网卡缺陷修复不属于本次收口结果。

`tasks.md` 保留 6/25 完成状态。未勾选项不得解释为已实现或已运行：已有命令但未执行的项目由 R49 标为 `not-run`；当前 workload 或采集器不能表达的项目登记为 I16 `infrastructure-unavailable`。EV-005-07 只证明 user-net compatibility smoke 和失败分类，不构成性能基线。
