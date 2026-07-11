# Spec Delta: optimization — Carrier ARC-202607111510

## REMOVED Requirements

### Requirement: O78 — Lichee memory-root path loader benchmark

O78 已从 active optimization 清单移除。Q19C-M1/M2 已通过 D1 真板，当前只保留 loader/mm 后续 O80。

#### Scenario: 恢复 O78

- **WHEN** 开发者需要回查 Q19C memory-root path/command 完成事实
- **THEN** MUST 使用本 carrier spec 的压缩保留区

### Requirement: O79 — Lichee SDMMC/block/rootfs implementation

O79 已从 active optimization 清单移除。Storage/rootfs bring-up 已取消当前规划，需要时必须重新 propose。

#### Scenario: 恢复 O79

- **WHEN** 项目目标重新转向 D1 storage/rootfs
- **THEN** MUST 使用本 carrier spec 回查取消原因

### Requirement: O81 — M3 startup benchmark 隔离与 UART 刷出复核

O81 已从 active optimization 清单移除。M3/rootfs-probe 不再是 Q19C gate。

#### Scenario: 恢复 O81

- **WHEN** 后续重新打开 storage/rootfs bring-up 且需要 probe entry isolation
- **THEN** MUST 使用本 carrier spec 回查历史状态

---

## 压缩保留

### O78 (Compress-Archive, 2026-07-11)

Lichee memory-root path loader benchmark已完成：M1 从 memory-root `/bin/benchmark` 走 `FS_CONTEXT.resolve()/read()` + eager ELF mapping，M2 command-entry 真板通过。
状态：Q19C 已归档；性能验证结论见 architecture ADR-052/054/055、learned L259-L280。

### O79 (Compress-Archive, 2026-07-11)

Lichee SDMMC/block/rootfs implementation已取消当前规划：storage/rootfs 不再作为 async UART 性能验证后续项。
状态：需要真实 D1 storage/rootfs bring-up 时重新 propose。

### O81 (Compress-Archive, 2026-07-11)

M3 startup benchmark/probe entry isolation已取消当前规划：M3/rootfs-probe 不再作为 Q19C gate。
状态：仅在 storage/rootfs bring-up 重新打开时复核。
