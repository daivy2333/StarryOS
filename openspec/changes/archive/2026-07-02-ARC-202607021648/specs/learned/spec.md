# Spec Delta: learned — ARC-202607021648

## REMOVED Requirements

### Requirement: Archived learned entries remain recoverable

Historical learned entries removed from the active learned spec MUST remain recoverable from this carrier spec.

#### Scenario: Restoring a compressed learned item

- **WHEN** a developer needs an archived learned item
- **THEN** they MUST restore it from this carrier spec using the original L-number.

## 压缩保留（Compress-Archive 区）

### L161-L164 / L189-L191 / L194-L195 (Compress-Archive, old 5-trait API)

- **状态**: 已替代。旧 5-trait 抽象中的 `OsIrq` / `OsMmio` / `OsSpinNoIrq` 及对应 ArceOS adapters 已被 ADR-036 删除。
- **当前事实**: active OS abstraction 只保留 `OsRuntime` + `OsWakerSet`；CodeGraph 查无 `OsIrq` / `OsSpinNoIrq` / `ArceOsMmio`。
- **恢复条件**: 仅在研究 Q13 5-trait 历史设计时恢复。

### L236-L239 (Compress-Archive, Q19B planning phase)

- **状态**: 已执行。Q19B 从 smoke 推进到 async UART、PLIC IRQ 18、`/dev/console`、embedded benchmark 的路线已完成。
- **替代入口**: L240-L258、ADR-047~ADR-051、`openspec/changes/archive/2026-07-02-q19b-lichee-d1-benchmark/`。
- **恢复条件**: 需要回看 Q19B 初始计划/推荐路线时恢复。

### L243 / L246 / L247 (Compress-Archive, Q19B blockers and next plan)

- **状态**: 已解决。feature 继承、axfs/devfs/syscall、embedded ELF、真板证据等阻塞均已在 Q19B 收尾阶段解决。
- **保留事实**: 禁止启用 QEMU PCI/virtio 绕过 D1 userbench；SDMMC/rootfs parity 另立 Q19C。
- **恢复条件**: 需要排查 Q19B 历史阻塞或复盘计划执行时恢复。
