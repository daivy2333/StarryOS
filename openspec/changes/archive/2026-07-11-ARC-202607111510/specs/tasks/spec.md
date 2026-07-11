# Spec Delta: tasks — Carrier ARC-202607111510

## REMOVED Requirements

### Requirement: Q19C.1-Q19C.12 — Lichee async UART board benchmark

Q19C 已完成 D1 async UART 验证并归档。active `tasks.md` 只保留结束摘要，逐项实施清单移入本 carrier。

#### Scenario: 恢复 Q19C 明细

- **WHEN** 开发者需要回查 Q19C 逐项执行历史
- **THEN** MUST 使用本 carrier spec 的完整保留区

### Requirement: Q19D.1-Q19D.6 — Lichee SDMMC/rootfs implementation

Q19D 已取消当前规划。active `tasks.md` 只保留 storage/rootfs 需重新 propose 的边界，逐项取消清单移入本 carrier。

#### Scenario: 恢复 Q19D 明细

- **WHEN** 开发者需要回查 Q19D 取消项
- **THEN** MUST 使用本 carrier spec 的完整保留区

---

## 完整保留

### Q19C.1-Q19C.12 (Archive, 2026-07-11)

<!-- Q19C.1 --> - [x] Phase 1: 需求探索 + CodeGraph 路径追踪，确认 QEMU full path 为 `mount_all()` → `FS_CONTEXT.resolve()` → `load_user_app()`，Q19B Lichee 当前为 embedded `load_embedded_user_app()`
<!-- Q19C.2 --> - [x] BDD 缺口扫描：区分 Happy Path（memory-root/rootfs benchmark）、Sad Path（无 block device / path missing）、Edge（Q19B regression 不退化）
<!-- Q19C.3 --> - [x] 创建 OpenSpec change：`proposal.md`、`design.md`、`tasks.md`、`specs/lichee-d1-fullbench/spec.md`
<!-- Q19C.4 --> - [x] Phase 2: 原方案拆分为 M0 benchmark evidence cleanup、M1 memory-root path loader、M2 optional shell/script parity、M3 SDMMC/block probe-only、future rootfs deferred；2026-07-11 方向更新后，当前 gate 收敛为 M0/M1/M2 async UART 性能验证
<!-- Q19C.4a --> - [x] Review 修订: 补齐 `FsContext::create_dir/write` 注入 API、feature/Makefile/entry 脚手架任务、mode feature/log label 映射、显式 benchmark section gate、ELF/boot image size 检查与 SKIPPED evidence 规则
<!-- Q19C.5 --> - [x] 实施入口：开始源码变更前建立 Q19B regression witness、`codegraph_impact` 与 Verify Current State
<!-- Q19C.6 --> - [x] M0 Phase 1/2: 梳理并规划 `benchmark.c` 参数/manifest，使 QEMU、Q19B embedded、Q19C memory-root/shell/rootfs 数据可按配置横向解释
<!-- Q19C.7 --> - [x] M0 Phase 1/2: 补齐真板 RX 测试方案，至少保留无输入 `EAGAIN` regression，并规划 fixed-payload/manual-input 或 loopback witness
<!-- Q19C.8 --> - [x] M0 Phase 1/2: 保留 64B 小包结果 `size=64 / iters=100 / 1.01 KB/s / 8.8% line rate`，探索批量 drain、no-drain enqueue、`writev`、TX wake/drain path、64/128/256B break-even 优化方向
<!-- Q19C.8a --> - [x] M0 Phase 3 执行入口：修改 `tests/benchmark.c` 前先跑 current-state witness，并等待用户确认进入实施
<!-- Q19C.8b --> - [x] M0 实施：统一 QEMU/userbench benchmark manifest 与 section 输出，默认移除 4096B 测试，补齐 section pre-drain/latency 相对线时诊断和 D1 gated TX debug snapshot
<!-- Q19C.8c --> - [x] M0 真板数据分析：确认 64B 小包旧异常主要来自 stdout backlog 测量污染；隔离后 D1 64B `write+tcdrain` 接近线速
<!-- Q19C.8d --> - [x] D1 TX 已验证修复：`send_bytes()` 在 THRE 后一次填最多 16B FIFO，TTY OPOST/ONLCR short-write 计数修复；S11 1024B 正确发送恢复
<!-- Q19C.8e --> - [x] Q19C.8e 已完成：slow-pool（`TX_SLOW_POLL_LIMIT=4096`）+ yield 重试（`TX_YIELD_RETRIES=4`）已实施；真板 `slow_poll_exh=0` `yield_exh=0` 证明 slow-pool 100% 成功；P99 长尾（50.86ms）根因未探明，当前影响可接受（吞吐量 <2%），暂不继续优化，Q20 复验时再探明（O77/L275 已记录）
<!-- Q19C.9 --> - [x] M1: 在 memory-root 中提供 benchmark ELF 文件节点，通过 `FS_CONTEXT.resolve()/read()` + eager ELF mapping 启动 benchmark；真板 `.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/Q19cM1.md` 已完整输出 manifest、TX/RX sections 和 exit code 0；`load_user_app()` lazy file-backed COW 问题后置为 O80
<!-- Q19C.10a --> - [x] M2 host gate: `lichee-d1-fullbench-command` feature, `make lichee-fullbench-command`, `starry-lichee-fullbench-command-boot.img` kernel_size=999616, cargo check 通过, image build 通过；单模式 feature 互斥 guard 已加入（`compile_error!`）
<!-- Q19C.10b --> - [x] M2 acceptance: true shell path deferred to future optional；M2 必达目标收敛为 `lichee-memory-root-command`；`shell_status=SKIPPED: no known-good static /bin/sh` 为合法证据
<!-- Q19C.10c --> - [x] M2 board gate: D1 真板运行通过；`.claude/analysis/_archive/2026-07-11-q19c-d1-async-uart-closeout/lichee/M2.md` 含 `lichee-memory-root-command`、`shell_status=SKIPPED`、argv/envp evidence、benchmark sections、`Done.`、`benchmark exited with code: 0` 和 `halting.`
<!-- Q19C.11a --> - [x] M3 host gate（历史事实）: `lichee-d1-rootfs-probe` feature, `make lichee-rootfs-probe`, `starry-lichee-rootfs-probe-boot.img` kernel_size=159936, cargo check 通过, image build 通过, 无 `init_filesystems()` 调用路径；单模式 feature 互斥 guard 已加入
<!-- Q19C.11b --> - [x] M3 acceptance（历史事实）: rootfs-probe 为 blocker report（非 register probe success / rootfs benchmark success）；日志标注 TBD/SKIPPED
<!-- Q19C.11c --> - [x] M3 board gate 取消: D1 真板日志只到 `d1_sdmmc_controller_base=TBD`，未形成完整 probe table；2026-07-11 方向更新后，M3/rootfs-probe 不再作为 async UART 性能验证 gate
<!-- Q19C.11d --> - [x] M3 深度探索取消: 旧 `q19c-m3-polling-console-isolation` change 已归档；不再继续定位 probe table 输出中断，除非后续目标转向 storage/rootfs bring-up
<!-- Q19C.12 --> - [x] Future rootfs 取消当前规划: 真实 block/rootfs 与 shell 不再作为 Q19C 收尾条件；需要时重新 propose 独立 storage/rootfs change

### Q19D.1-Q19D.6 (Archive, 2026-07-11)

<!-- Q19D.1 --> - [x] 取消当前规划：不创建 `q19d-lichee-sdmmc-rootfs`，除非用户重新确认 storage/rootfs bring-up 为目标
<!-- Q19D.2 --> - [x] 取消当前规划：不再要求基于 Q19C SDMMC probe 表确认 controller base/IRQ/clock/reset/pinmux/card-detect
<!-- Q19D.3 --> - [x] 取消当前规划：不设计 D1 SDMMC PIO-first block read 路径
<!-- Q19D.4 --> - [x] 取消当前规划：不注册 D1 SDMMC `AxBlockDevice`
<!-- Q19D.5 --> - [x] 取消当前规划：不准备 ext4/FAT rootfs 镜像、shell 或 init script
<!-- Q19D.6 --> - [x] 取消当前规划：不运行 real rootfs path benchmark
