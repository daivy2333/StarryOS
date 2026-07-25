# Archive Note

This change was archived by `cleanup-uart-documentation-system` on 2026-07-25.

- **Status at archive**: 18/19 tasks complete
- **Deferred task**: Task 6.1 (multi-hart / real-board SMP stress) — NOT executed
  - Requires VisionFive2 or equivalent multi-hart QEMU configuration
  - QEMU single-hart verification completed (tasks 1-5 all pass)
  - Cross-hart correctness claim must NOT be made based on QEMU single-hart results
- **Why archived**: UART documentation exited active system; task 6.1 (multi-hart SMP stress) remains deferred. I05 was archived alongside this change as a tombstone. Cross-hart correctness claim requires VisionFive2 or equivalent multi-hart environment.
- **Restore path**: All files at `openspec/changes/archive/2026-07-25-q17-smp-memory-ordering/`
