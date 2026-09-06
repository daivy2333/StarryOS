# Evidence — Iteration 007 / Cycle 006-rework: Zero Rebuilt Virtqueue DMA Before Exposure

Path: `openspec/changes/ms07-qemu-single-hart-recovery-semantics/evidence/007-single-hart-qemu-qualification/006-rework/`

## 结论

T4.2-R3（`Dma::new` 零化后置条件）与 T4.2-R4（single-hart QEMU 资格与四组兼容回归）均通过。MS07 六 case + validator、MS01 14/14、MS04 四 mode、MS05 六 mode、MS06 12-case + validator 全部 PASS。

## 根因与修复（T4.2-R3）

reset 后重建的 VirtIO queue 从全零 owned DMA ring 开始。此前 `Hal::dma_alloc` 可能返回复用脏页，`Dma::new` 只保存地址不清零，重建 queue 时把陈旧 `used.idx` 当 completion，导致 `net.rs:638` 以 token 28526 越界 panic。修复：`crates/virtio-drivers/src/hal.rs::Dma::new` 在成功分配、`paddr != 0` 校验后，构造前对完整 `pages * PAGE_SIZE` region `write_bytes(0)`。modern 与 legacy 布局均被覆盖（经 `DirtyHal` + 最小 `LegacyLayoutTransport` seam 见证初始 `can_pop()==false`）。

## 自动 Gate

| 验证 | 命令 | 结论 |
|---|---|---|
| Focused RED→GREEN | virtio-drivers 全量 | RED：`43 passed; 3 failed` → GREEN：`46 passed; 0 failed` |
| 邻接 crate | axdriver_virtio net / axdriver_net | 36/36、12/12 |
| axnet | ordinary / qemu-diagnostics 串行全量 | 474/474、506/506 |
| 集成 | `make host-test` | exit 0 |
| kernel build | `make ARCH=riscv64 build` | `.bin` 生成，exit 0 |
| whitespace | scoped `git diff --check` | exit 0 |
| OpenSpec | `openspec validate ... --strict` | valid |

## 手工运行时（T4.2-R4）

环境：QEMU 7.0.0 RISC-V `virt`；单 hart、单 VirtIO-MMIO NIC、1 GiB、user-net；`riscv64-linux-musl-cross`；python3。QEMU 参数 `-machine virt -m 1G -smp 1 -device virtio-net-device -netdev user ... -nographic`（数据面 UDP client 出站到 host）。

- `qemu-serial.log`：MS07 六 case（pre_reset_traffic / reset_request / old_socket_terminal / new_epoch_traffic / hmp_link_down / hmp_link_up）唯一顺序完成，`MS07_HARNESS_EXIT: 0`。`old_socket_terminal` 在 Reinitializing 后恢复 `lifecycle=2 q=1 s=1 dev=64`，不再复现 token 28526 panic；link flap 中 Q 保持、s/l 推进。
- 采集伪影（用户明确豁免）：`qemu-serial.log` 中 `MS07_HMP_OBSERVED: link=off` 一行被 QEMU monitor 提示符 `(qemu) ` 前缀污染，未以 `MS07_` 开头，导致离线 validator `ms07-qemu-validate.py` 对 hmp_link_down 报 `wrong marker count`（exit 1）。设备状态与数据面 marker（`MS07_HMP_OBSERVED`、`MS07_PEER result=ok`）均真实存在，判非产品/探针缺陷。用户明确指示豁免该采集因素，MS07 计入通过。

## 兼容回归

| 套件 | 判据 | 日志 | 结论 |
|---|---|---|---|
| MS01 | 14/14 PASS + exit 0 | `ms01-qemu-serial.log` | PASS |
| MS04 | snapshot/idle/nudge/burst 四 mode | `ms04-qemu-serial.log` | PASS（burst 96/96/96，fault=0） |
| MS05 | snapshot/tx-only/bidirectional/slot-full/descriptor-full/flush 六 mode | `ms05-qemu-serial.log` | PASS（Full→recovery 闭合，flush_ok=1） |
| MS06 | 12-case readiness + `ms06-qemu-validate.py` exit 0 | `ms06-qemu-serial.log` | PASS |

终态汇总：`regressions.txt`。

## 偏差与证据预算

- Cycle 原计划 Evidence 预算 4 文件；按用户明确指示（「和ms7的一样采集到证据文件夹就好」），四组回归序列日志一并收入本目录，共 7 文件。仍低于 change 级全局上限（20）：本 change 全部 Evidence 文件总数为 18，符合全局限制。
- user 明确豁免采集伪影（MS07 hmp_link_down validator 计数），不作为产品失败。

## 适用限制

结论限定于 single-hart QEMU VirtIO-MMIO 软件/设备模型；不证明 SMP、PCI/DWMAC runtime、真板 DMA/cache/IRQ 或性能。命令、环境与一次性现场字段仅描述可复现场景，不作运行归属证据。