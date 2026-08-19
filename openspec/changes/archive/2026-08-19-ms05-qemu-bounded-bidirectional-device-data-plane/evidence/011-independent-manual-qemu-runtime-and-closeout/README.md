# Evidence — 011-independent-manual-qemu-runtime-and-closeout

| Cycle | Status |
|---|---|
| [000-initial](000-initial/README.md) | in-progress — manual QEMU runtime; baseline frozen, user collection pending |
| [001-rework](001-rework/README.md) | in-progress — 自动 manifest 44/44 PASS + audit PASS + freeze；等待用户手工 QEMU (6.2-R2) |
| [002-rework](002-rework/README.md) | BLOCKED — schema-v2 自动资质 + 精确 handoff 完成；手工 MS05 descriptor-full full-deadline 及 host-stimulus timing 阻塞，回 Plan 下一轮 |
| [003-rework](003-rework/README.md) | BLOCKED — Act 完成 6.2-R4 (descriptor-Full 从守恒 ledger 证明 + timeout tuple) 与 6.2-R5 (manual listen/exchange 分离 + DONE/ACK)；全部 host/自动见证 GREEN；手工 QEMU 运行时 (Gates 5-7) 由用户决定延后到下一 Cycle |
| [004-rework](004-rework/README.md) | BLOCKED — Act 完成 6.2-R6 (bounded registration + exact DONE)；六 mode manual QEMU 运行时全部终态 PASS；6.1-R1 资格 audit 因 WORKTREE_DRIFT + manifest 删除阻塞；内核镜像 hash 变化需用户基线决定后走 Plan/Review |
